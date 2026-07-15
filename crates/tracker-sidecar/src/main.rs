mod live_log;

use anyhow::{Context, Result};
use clap::Parser;
use domain::{CardInstanceId, CardKey, CardKnowledge, MatchLifecycle, Participant, Zone};
use live_log::{LiveLogState, parse_live_log_line};
use state_reader::{
    OverlayProjector, ReadOptions, ReconciliationInput, SnapshotObservation, TextOverlayCard,
    TextOverlayCounters, TextOverlayPanel, TextOverlayPayload, TextOverlaySlot,
    TextOverlaySlotState, observe_game_state, read_json_snapshot, reconcile_observation,
    text_overlay_payload,
};
use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const DEFAULT_STATE_SUFFIX: &str = ".steam/steam/steamapps/compatdata/1997040/pfx/drive_c/users/steamuser/AppData/LocalLow/Second Dinner/SNAP/Standalone/States/nvprod";
const GAME_STATE_FILENAME: &str = "GameState.json";
const COLLECTION_STATE_FILENAME: &str = "CollectionState.json";
const PLAY_STATE_FILENAME: &str = "PlayState.json";

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Emit live OpenSnapTracker overlay payloads from read-only Marvel Snap state files"
)]
struct Args {
    #[arg(long)]
    state_dir: Option<PathBuf>,
    #[arg(long, value_name = "PATH")]
    output_json: PathBuf,
    #[arg(long, default_value_t = 250)]
    interval_ms: u64,
    #[arg(long, default_value_t = false)]
    once: bool,
    #[arg(long, default_value_t = false)]
    stdout_events: bool,
    #[arg(long, default_value_t = false)]
    debug_polls: bool,
}

#[derive(Debug)]
struct LiveTracker {
    state_file: PathBuf,
    collection_file: PathBuf,
    play_file: PathBuf,
    player_log_file: PathBuf,
    output_json: PathBuf,
    interval: Duration,
    previous_hash: Option<String>,
    previous_collection_hash: Option<String>,
    previous_play_hash: Option<String>,
    previous_log_position: Option<u64>,
    selected_deck_id: Option<String>,
    live_log_state: LiveLogState,
    collection_decks: Vec<CollectionDeck>,
    matched_player_deck: Option<CollectionDeck>,
    previous_observation: Option<SnapshotObservation>,
    current_payload: Option<TextOverlayPayload>,
    projector: OverlayProjector,
    last_update_source: Option<&'static str>,
    pending_focus_bounce_reasons: Vec<String>,
    pending_opponent_discard_clues: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CollectionDeck {
    id: String,
    name: String,
    cards: Vec<CardKey>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let state_dir = args
        .state_dir
        .or_else(default_state_dir)
        .context("could not determine a state directory; pass --state-dir")?;
    let mut tracker = LiveTracker::new(
        state_dir,
        args.output_json,
        Duration::from_millis(args.interval_ms),
    );

    let running = Arc::new(AtomicBool::new(true));
    let signal_flag = Arc::clone(&running);
    ctrlc::set_handler(move || {
        signal_flag.store(false, Ordering::SeqCst);
    })
    .context("install SIGINT handler")?;

    let mut last_poll_event = Instant::now()
        .checked_sub(Duration::from_secs(2))
        .unwrap_or_else(Instant::now);
    while running.load(Ordering::SeqCst) {
        match tracker.tick() {
            Ok(changed) => {
                if args.stdout_events && changed {
                    emit_stdout_event(&serde_json::json!({
                        "event": "payload-written",
                        "path": tracker.output_json.display().to_string(),
                        "source": tracker.last_update_source,
                    }))?;
                }
                for reason in tracker.take_focus_bounce_reasons() {
                    emit_stdout_event(&serde_json::json!({
                        "event": "focus-bounce-requested",
                        "reason": reason,
                    }))?;
                }
                if args.stdout_events
                    && args.debug_polls
                    && last_poll_event.elapsed() >= Duration::from_secs(1)
                {
                    emit_stdout_event(&serde_json::json!({
                        "event": "poll",
                        "changed": changed,
                        "game_hash": tracker.previous_hash.as_deref(),
                    }))?;
                    last_poll_event = Instant::now();
                }
            }
            Err(error) => {
                eprintln!("[tracker-sidecar] {error:#}");
            }
        }
        if args.once {
            break;
        }
        thread::sleep(tracker.interval);
    }

    Ok(())
}

fn emit_stdout_event(value: &serde_json::Value) -> Result<()> {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, value)?;
    writeln!(stdout)?;
    stdout.flush()?;
    Ok(())
}

impl LiveTracker {
    fn new(state_dir: PathBuf, output_json: PathBuf, interval: Duration) -> Self {
        Self {
            state_file: state_dir.join(GAME_STATE_FILENAME),
            collection_file: state_dir.join(COLLECTION_STATE_FILENAME),
            play_file: state_dir.join(PLAY_STATE_FILENAME),
            player_log_file: player_log_path(&state_dir),
            output_json,
            interval,
            previous_hash: None,
            previous_collection_hash: None,
            previous_play_hash: None,
            previous_log_position: None,
            selected_deck_id: None,
            live_log_state: LiveLogState::default(),
            collection_decks: Vec::new(),
            matched_player_deck: None,
            previous_observation: None,
            current_payload: None,
            projector: OverlayProjector::new(),
            last_update_source: None,
            pending_focus_bounce_reasons: Vec::new(),
            pending_opponent_discard_clues: 0,
        }
    }

    fn tick(&mut self) -> Result<bool> {
        self.refresh_play_state()?;
        self.refresh_collection_decks()?;
        let log_changed = self.refresh_player_log()?;
        let snapshot_changed = if self.current_payload.is_some() {
            self.refresh_game_state_snapshot()?
        } else {
            false
        };

        if self.current_payload.is_none() {
            self.seed_payload_from_game_state()?;
            self.write_current_payload()?;
            self.last_update_source = if log_changed || snapshot_changed {
                Some("game-state-seed+live")
            } else {
                Some("game-state-seed")
            };
            return Ok(true);
        }

        if log_changed || snapshot_changed {
            if snapshot_changed {
                self.write_current_snapshot_payload()?;
            } else {
                self.write_current_payload()?;
            }
            self.last_update_source = if log_changed && snapshot_changed {
                Some("player-log+game-state")
            } else if log_changed {
                Some("player-log")
            } else {
                Some("game-state")
            };
            return Ok(true);
        }

        Ok(false)
    }

    fn seed_payload_from_game_state(&mut self) -> Result<()> {
        let snapshot = read_json_snapshot(&self.state_file, ReadOptions::default())
            .with_context(|| format!("read {}", self.state_file.display()))?;
        let observation = observe_game_state(&snapshot.parsed)
            .with_context(|| format!("observe {}", self.state_file.display()))?;
        let events = reconcile_observation(ReconciliationInput {
            previous: self.previous_observation.as_ref(),
            current: &observation,
            snapshot_version: Some(snapshot.sha256.clone()),
        });
        let projection = self.projector.project(&observation, &events);
        let mut payload = text_overlay_payload(&projection);
        self.enrich_player_deck(&mut payload);
        self.current_payload = Some(payload);

        self.previous_hash = Some(snapshot.sha256);
        self.previous_observation = Some(observation);
        Ok(())
    }

    fn refresh_collection_decks(&mut self) -> Result<()> {
        let snapshot = match read_json_snapshot(&self.collection_file, ReadOptions::default()) {
            Ok(snapshot) => snapshot,
            Err(state_reader::SnapshotReadError::Io { .. }) => return Ok(()),
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("read {}", self.collection_file.display()));
            }
        };
        if self.previous_collection_hash.as_deref() == Some(snapshot.sha256.as_str()) {
            return Ok(());
        }
        self.collection_decks = parse_collection_decks(&snapshot.parsed);
        self.previous_collection_hash = Some(snapshot.sha256);
        Ok(())
    }

    fn refresh_play_state(&mut self) -> Result<()> {
        let snapshot = match read_json_snapshot(&self.play_file, ReadOptions::default()) {
            Ok(snapshot) => snapshot,
            Err(state_reader::SnapshotReadError::Io { .. }) => return Ok(()),
            Err(error) => {
                return Err(error).with_context(|| format!("read {}", self.play_file.display()));
            }
        };
        if self.previous_play_hash.as_deref() == Some(snapshot.sha256.as_str()) {
            return Ok(());
        }
        let next_deck_id = parse_selected_deck_id(&snapshot.parsed);
        if self.selected_deck_id != next_deck_id {
            self.live_log_state.reset_match();
            self.current_payload = None;
            self.previous_hash = None;
            self.previous_observation = None;
            self.matched_player_deck = None;
        }
        self.selected_deck_id = next_deck_id;
        self.previous_play_hash = Some(snapshot.sha256);
        Ok(())
    }

    fn refresh_player_log(&mut self) -> Result<bool> {
        let metadata = match fs::metadata(&self.player_log_file) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("stat {}", self.player_log_file.display()));
            }
        };
        let len = metadata.len();
        let Some(position) = self.previous_log_position else {
            self.previous_log_position = Some(len);
            return Ok(false);
        };
        if len < position {
            self.previous_log_position = Some(len);
            self.live_log_state = LiveLogState::default();
            return Ok(false);
        }
        if len == position {
            return Ok(false);
        }

        let bytes = fs::read(&self.player_log_file)
            .with_context(|| format!("read {}", self.player_log_file.display()))?;
        let start = usize::try_from(position)
            .unwrap_or(bytes.len())
            .min(bytes.len());
        let text = String::from_utf8_lossy(&bytes[start..]);
        let mut changed = false;
        for line in text.lines() {
            if let Some(event) = parse_live_log_line(line) {
                match &event {
                    live_log::LiveLogEvent::OpponentDiscardClue => {
                        self.pending_opponent_discard_clues =
                            self.pending_opponent_discard_clues.saturating_add(1);
                    }
                    live_log::LiveLogEvent::ResolutionStarted => {
                        self.pending_focus_bounce_reasons
                            .push("resolution-started".to_string());
                    }
                    live_log::LiveLogEvent::TurnStarted => {
                        self.pending_focus_bounce_reasons
                            .push("turn-start".to_string());
                    }
                    live_log::LiveLogEvent::MatchEnded | live_log::LiveLogEvent::MatchLeft => {
                        self.pending_focus_bounce_reasons
                            .push("match-ended".to_string());
                    }
                    _ => {}
                }
                let reset_payload = matches!(
                    event,
                    live_log::LiveLogEvent::MatchStarted { .. } | live_log::LiveLogEvent::MatchLeft
                );
                changed |= self.live_log_state.apply(event);
                if reset_payload {
                    self.reset_current_match_payload();
                    changed = true;
                }
            }
        }
        self.previous_log_position = Some(len);
        Ok(changed)
    }

    fn take_focus_bounce_reasons(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_focus_bounce_reasons)
    }

    fn refresh_game_state_snapshot(&mut self) -> Result<bool> {
        let snapshot = match read_json_snapshot(&self.state_file, ReadOptions::default()) {
            Ok(snapshot) => snapshot,
            Err(state_reader::SnapshotReadError::Io { .. }) => return Ok(false),
            Err(error) => {
                return Err(error).with_context(|| format!("read {}", self.state_file.display()));
            }
        };
        if self.previous_hash.as_deref() == Some(snapshot.sha256.as_str()) {
            return Ok(false);
        }

        let observation = observe_game_state(&snapshot.parsed)
            .with_context(|| format!("observe {}", self.state_file.display()))?;
        let previous_observation = self.previous_observation.clone();
        let events = reconcile_observation(ReconciliationInput {
            previous: previous_observation.as_ref(),
            current: &observation,
            snapshot_version: Some(snapshot.sha256.clone()),
        });
        let projection = self.projector.project(&observation, &events);
        let mut payload = text_overlay_payload(&projection);
        self.enrich_player_deck(&mut payload);

        for card in &observation.cards {
            if card.owner == Participant::Player
                && card.raw_zone.as_deref() == Some("Banished")
                && let Some(key) = &card.card_definition_key
            {
                self.live_log_state
                    .apply(live_log::LiveLogEvent::PlayerCardRemoved { key: key.clone() });
            }
        }
        if self.pending_opponent_discard_clues > 0 {
            for key in
                opponent_graveyard_discard_candidates(previous_observation.as_ref(), &observation)
                    .into_iter()
                    .take(self.pending_opponent_discard_clues)
            {
                self.live_log_state
                    .apply(live_log::LiveLogEvent::OpponentCardDiscarded { key });
                self.pending_opponent_discard_clues =
                    self.pending_opponent_discard_clues.saturating_sub(1);
            }
        }
        self.current_payload = Some(payload);
        self.previous_hash = Some(snapshot.sha256);
        self.previous_observation = Some(observation);
        Ok(true)
    }

    fn enrich_player_deck(&mut self, payload: &mut TextOverlayPayload) {
        if self.collection_decks.is_empty() {
            return;
        }
        if let Some(selected_deck_id) = &self.selected_deck_id
            && let Some(deck) = self
                .collection_decks
                .iter()
                .find(|deck| deck.id == *selected_deck_id)
                .cloned()
        {
            self.live_log_state.set_player_deck(deck.cards.clone());
            Self::apply_player_deck(payload, &deck);
            self.matched_player_deck = Some(deck);
            return;
        }

        let observed_keys = observed_player_keys(payload);
        if observed_keys.is_empty() {
            return;
        }
        if self
            .matched_player_deck
            .as_ref()
            .is_none_or(|deck| !deck_matches(deck, &observed_keys))
            && let Some(deck) = best_matching_deck(&self.collection_decks, &observed_keys)
        {
            self.live_log_state.set_player_deck(deck.cards.clone());
            self.matched_player_deck = Some(deck.clone());
        }

        let Some(deck) = &self.matched_player_deck else {
            return;
        };
        self.live_log_state.set_player_deck(deck.cards.clone());
        Self::apply_player_deck(payload, deck);
    }

    fn apply_player_deck(payload: &mut TextOverlayPayload, deck: &CollectionDeck) {
        let observed_by_key = payload
            .player
            .deck_slots
            .iter()
            .filter_map(|slot| slot.card.as_ref())
            .filter_map(|card| {
                card.card_definition_key
                    .as_ref()
                    .map(|key| (key.clone(), card.clone()))
            })
            .collect::<HashMap<_, _>>();

        payload.player.title = deck.name.clone();
        payload.player.deck_slots = deck
            .cards
            .iter()
            .take(12)
            .enumerate()
            .map(|(index, key)| {
                let card = observed_by_key
                    .get(key)
                    .cloned()
                    .unwrap_or_else(|| deck_text_card(index, key));
                TextOverlaySlot {
                    slot_index: index as u8,
                    state: TextOverlaySlotState::Known,
                    card: Some(card),
                }
            })
            .collect();
    }

    fn write_current_payload(&mut self) -> Result<()> {
        let Some(mut payload) = self.current_payload.take() else {
            return Ok(());
        };
        self.enrich_player_deck(&mut payload);
        self.live_log_state.seed_counters_from_payload(&payload);
        self.live_log_state.apply_to_payload(&mut payload);
        write_json_atomic(&self.output_json, &serde_json::to_vec_pretty(&payload)?)?;
        self.current_payload = Some(payload);
        Ok(())
    }

    fn write_current_snapshot_payload(&mut self) -> Result<()> {
        let Some(mut payload) = self.current_payload.take() else {
            return Ok(());
        };
        self.enrich_player_deck(&mut payload);
        self.live_log_state.seed_counters_from_payload(&payload);
        self.live_log_state.apply_to_snapshot_payload(&mut payload);
        write_json_atomic(&self.output_json, &serde_json::to_vec_pretty(&payload)?)?;
        self.current_payload = Some(payload);
        Ok(())
    }

    fn reset_current_match_payload(&mut self) {
        self.previous_hash = None;
        self.previous_observation = None;
        self.projector = OverlayProjector::new();
        self.pending_opponent_discard_clues = 0;
        self.pending_focus_bounce_reasons.clear();
        self.current_payload = Some(blank_overlay_payload());
        if let Some(deck) = self
            .selected_deck_id
            .as_ref()
            .and_then(|selected_deck_id| {
                self.collection_decks
                    .iter()
                    .find(|deck| deck.id == *selected_deck_id)
                    .cloned()
            })
            .or_else(|| self.matched_player_deck.clone())
        {
            self.live_log_state.set_player_deck(deck.cards.clone());
            if let Some(payload) = self.current_payload.as_mut() {
                Self::apply_player_deck(payload, &deck);
            }
            self.matched_player_deck = Some(deck);
        }
    }
}

fn blank_overlay_payload() -> TextOverlayPayload {
    TextOverlayPayload {
        schema_version: 1,
        lifecycle: MatchLifecycle::Unknown,
        turn: None,
        total_turns: None,
        player: blank_overlay_panel(Participant::Player, "Player Deck"),
        opponent: blank_overlay_panel(Participant::Opponent, "Opponent"),
        warnings: Vec::new(),
    }
}

fn blank_overlay_panel(participant: Participant, title: &str) -> TextOverlayPanel {
    TextOverlayPanel {
        participant,
        title: title.to_string(),
        deck_slots: (0u8..12)
            .map(|slot_index| TextOverlaySlot {
                slot_index,
                state: TextOverlaySlotState::Unknown,
                card: None,
            })
            .collect(),
        supplemental: Vec::new(),
        destroyed: Vec::new(),
        discarded: Vec::new(),
        removed: Vec::new(),
        unknown_transition: Vec::new(),
        counters: TextOverlayCounters {
            deck: 0,
            hand: 0,
            board: 0,
            destroyed: 0,
            discarded: 0,
            removed: 0,
            supplemental: 0,
            unknown_transition: 0,
        },
    }
}

fn default_state_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(DEFAULT_STATE_SUFFIX))
}

fn player_log_path(state_dir: &Path) -> PathBuf {
    state_dir
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(|snap_root| snap_root.join("Player.log"))
        .unwrap_or_else(|| state_dir.join("Player.log"))
}

fn write_json_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let temp_path = path.with_extension("json.tmp");
    fs::write(&temp_path, bytes).with_context(|| format!("write {}", temp_path.display()))?;
    fs::rename(&temp_path, path)
        .with_context(|| format!("rename {} to {}", temp_path.display(), path.display()))?;
    Ok(())
}

fn parse_collection_decks(value: &serde_json::Value) -> Vec<CollectionDeck> {
    value
        .pointer("/ServerState/Decks")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|deck| {
            let id = deck.get("Id")?.as_str()?.to_string();
            let name = deck.get("Name")?.as_str()?.to_string();
            let cards = deck
                .get("Cards")?
                .as_array()?
                .iter()
                .filter_map(|card| card.get("CardDefId")?.as_str())
                .map(|key| CardKey(key.to_string()))
                .collect::<Vec<_>>();
            (cards.len() >= 12).then_some(CollectionDeck { id, name, cards })
        })
        .collect()
}

fn parse_selected_deck_id(value: &serde_json::Value) -> Option<String> {
    value
        .get("SerializedSelectedDeckId")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            value
                .pointer("/SelectedDeckId/Value")
                .and_then(serde_json::Value::as_str)
        })
        .filter(|deck_id| !deck_id.trim().is_empty())
        .map(ToOwned::to_owned)
}

fn observed_player_keys(payload: &TextOverlayPayload) -> HashSet<CardKey> {
    payload
        .player
        .deck_slots
        .iter()
        .filter_map(|slot| slot.card.as_ref())
        .filter_map(|card| card.card_definition_key.clone())
        .collect()
}

fn deck_matches(deck: &CollectionDeck, observed_keys: &HashSet<CardKey>) -> bool {
    observed_keys.iter().all(|key| deck.cards.contains(key))
}

fn best_matching_deck<'a>(
    decks: &'a [CollectionDeck],
    observed_keys: &HashSet<CardKey>,
) -> Option<&'a CollectionDeck> {
    let mut scored = decks
        .iter()
        .map(|deck| {
            let score = observed_keys
                .iter()
                .filter(|key| deck.cards.contains(key))
                .count();
            (score, deck)
        })
        .filter(|(score, _)| *score > 0)
        .collect::<Vec<_>>();
    scored.sort_by_key(|(score, _)| Reverse(*score));
    let (best_score, best_deck) = scored.first()?;
    let tied = scored
        .iter()
        .filter(|(score, _)| score == best_score)
        .count();
    (tied == 1).then_some(*best_deck)
}

fn deck_text_card(index: usize, key: &CardKey) -> TextOverlayCard {
    TextOverlayCard {
        instance_id: CardInstanceId(format!("collection:{index}:{}", key.0)),
        label: key.0.clone(),
        card_definition_key: Some(key.clone()),
        knowledge: CardKnowledge::KnownCard,
        zone: Zone::Deck,
        raw_zone: None,
        original_deck_candidate: true,
        consumed_from_deck: false,
    }
}

fn opponent_graveyard_discard_candidates(
    previous: Option<&SnapshotObservation>,
    current: &SnapshotObservation,
) -> Vec<CardKey> {
    let previous_graveyard = previous
        .into_iter()
        .flat_map(|observation| &observation.cards)
        .filter(|card| {
            card.owner == Participant::Opponent && card.raw_zone.as_deref() == Some("Graveyard")
        })
        .filter_map(|card| card.card_definition_key.clone())
        .collect::<HashSet<_>>();

    current
        .cards
        .iter()
        .filter(|card| {
            card.owner == Participant::Opponent && card.raw_zone.as_deref() == Some("Graveyard")
        })
        .filter_map(|card| card.card_definition_key.clone())
        .filter(|key| !previous_graveyard.contains(key))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn live_tracker_writes_payload_once_and_skips_unchanged_hash() {
        let temp = tempdir().expect("tempdir");
        let state_dir = temp.path().to_path_buf();
        let state_file = state_dir.join(GAME_STATE_FILENAME);
        let output_json = temp.path().join("overlay.json");
        fs::write(
            &state_file,
            include_str!("../../../fixtures/snapshots/sanitized-active-turn.json"),
        )
        .expect("write fixture");
        let mut tracker = LiveTracker::new(state_dir, output_json.clone(), Duration::ZERO);

        assert!(tracker.tick().expect("first tick"));
        assert!(!tracker.tick().expect("second tick skips unchanged"));

        let payload: serde_json::Value =
            serde_json::from_slice(&fs::read(output_json).expect("read payload"))
                .expect("payload parses");
        assert_eq!(payload["schema_version"], 1);
        assert_eq!(
            payload["player"]["deck_slots"].as_array().unwrap().len(),
            12
        );
        assert_eq!(
            payload["opponent"]["deck_slots"].as_array().unwrap().len(),
            12
        );
    }

    #[test]
    fn collection_deck_enrichment_seeds_player_slots_and_title() {
        let temp = tempdir().expect("tempdir");
        let state_dir = temp.path().to_path_buf();
        let state_file = state_dir.join(GAME_STATE_FILENAME);
        let collection_file = state_dir.join(COLLECTION_STATE_FILENAME);
        let output_json = temp.path().join("overlay.json");
        fs::write(
            &state_file,
            include_str!("../../../fixtures/snapshots/sanitized-active-turn.json"),
        )
        .expect("write fixture");
        fs::write(
            &collection_file,
            serde_json::json!({
                "ServerState": {
                    "Decks": [{
                        "Id": "fixture-discard",
                        "Name": "Fixture Discard",
                        "Cards": [
                            {"CardDefId": "Abomination"},
                            {"CardDefId": "Blade"},
                            {"CardDefId": "Dracula"},
                            {"CardDefId": "Gambit"},
                            {"CardDefId": "MoonKnight"},
                            {"CardDefId": "Apocalypse"},
                            {"CardDefId": "Morbius"},
                            {"CardDefId": "Modok"},
                            {"CardDefId": "LadySif"},
                            {"CardDefId": "Scorn"},
                            {"CardDefId": "Loki"},
                            {"CardDefId": "CorvusGlaive"}
                        ]
                    }]
                }
            })
            .to_string(),
        )
        .expect("write collection");
        let mut tracker = LiveTracker::new(state_dir, output_json.clone(), Duration::ZERO);

        assert!(tracker.tick().expect("tick"));

        let payload: serde_json::Value =
            serde_json::from_slice(&fs::read(output_json).expect("read payload"))
                .expect("payload parses");
        assert_eq!(payload["player"]["title"], "Fixture Discard");
        assert_eq!(
            payload["player"]["deck_slots"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|slot| slot["state"] == "known")
                .count(),
            12
        );
    }

    #[test]
    fn play_state_selected_deck_id_beats_observed_card_matching() {
        let temp = tempdir().expect("tempdir");
        let state_dir = temp.path().to_path_buf();
        let state_file = state_dir.join(GAME_STATE_FILENAME);
        let collection_file = state_dir.join(COLLECTION_STATE_FILENAME);
        let play_file = state_dir.join(PLAY_STATE_FILENAME);
        let output_json = temp.path().join("overlay.json");
        fs::write(
            &state_file,
            include_str!("../../../fixtures/snapshots/sanitized-active-turn.json"),
        )
        .expect("write fixture");
        fs::write(
            &play_file,
            serde_json::json!({
                "SerializedSelectedDeckId": "selected-deck"
            })
            .to_string(),
        )
        .expect("write play state");
        fs::write(
            &collection_file,
            serde_json::json!({
                "ServerState": {
                    "Decks": [
                        {
                            "Id": "wrong-observed-match",
                            "Name": "Wrong Observed Match",
                            "Cards": [
                                {"CardDefId": "Abomination"},
                                {"CardDefId": "Blade"},
                                {"CardDefId": "Dracula"},
                                {"CardDefId": "Gambit"},
                                {"CardDefId": "MoonKnight"},
                                {"CardDefId": "Apocalypse"},
                                {"CardDefId": "Morbius"},
                                {"CardDefId": "Modok"},
                                {"CardDefId": "LadySif"},
                                {"CardDefId": "Scorn"},
                                {"CardDefId": "Loki"},
                                {"CardDefId": "CorvusGlaive"}
                            ]
                        },
                        {
                            "Id": "selected-deck",
                            "Name": "Selected From PlayState",
                            "Cards": [
                                {"CardDefId": "Apocalypse"},
                                {"CardDefId": "Khonshu"},
                                {"CardDefId": "Modok"},
                                {"CardDefId": "Dracula"},
                                {"CardDefId": "Gambit"},
                                {"CardDefId": "LadySif"},
                                {"CardDefId": "Scorn"},
                                {"CardDefId": "Morbius"},
                                {"CardDefId": "ColleenWing"},
                                {"CardDefId": "MoonKnight"},
                                {"CardDefId": "CorvusGlaive"},
                                {"CardDefId": "Blade"}
                            ]
                        }
                    ]
                }
            })
            .to_string(),
        )
        .expect("write collection");
        let mut tracker = LiveTracker::new(state_dir, output_json.clone(), Duration::ZERO);

        assert!(tracker.tick().expect("tick"));

        let payload: serde_json::Value =
            serde_json::from_slice(&fs::read(output_json).expect("read payload"))
                .expect("payload parses");
        assert_eq!(payload["player"]["title"], "Selected From PlayState");
        assert_eq!(
            payload["player"]["deck_slots"][0]["card"]["label"],
            "Apocalypse"
        );
    }

    #[test]
    fn banished_snapshot_cards_are_projected_as_removed() {
        let temp = tempdir().expect("tempdir");
        let state_dir = temp.path().to_path_buf();
        let state_file = state_dir.join(GAME_STATE_FILENAME);
        let output_json = temp.path().join("overlay.json");
        let mut game_state: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/snapshots/sanitized-active-turn.json"
        ))
        .expect("fixture parses");

        game_state["RemoteGame"]["ClientGameInfo"] = serde_json::json!({
            "LocalPlayerEntityId": 2,
            "EnemyPlayerEntityId": 7
        });
        game_state["RemoteGame"]["GameState"]["_players"][0]["Banished"]["_cards"] =
            serde_json::json!([{ "$ref": "11" }]);
        game_state["RemoteGame"]["GameState"]["_players"][0]["Hand"]["_cards"] =
            serde_json::json!([]);
        game_state["RemoteGame"]["GameState"]["CardsByEntityId"]["21"]["CardDefId"] =
            serde_json::json!("Brood");
        game_state["RemoteGame"]["GameState"]["CardsByEntityId"]["21"]["_zone"] =
            serde_json::json!({ "$ref": "6" });

        fs::write(&state_file, game_state.to_string()).expect("write fixture");
        let mut tracker = LiveTracker::new(state_dir, output_json.clone(), Duration::ZERO);
        let deck = CollectionDeck {
            id: "deck".to_string(),
            name: "Fixture Deck".to_string(),
            cards: vec![
                CardKey("Brood".to_string()),
                CardKey("SilverSurfer".to_string()),
            ],
        };
        tracker.live_log_state.set_player_deck(deck.cards.clone());
        tracker.collection_decks = vec![deck.clone()];
        tracker.selected_deck_id = Some(deck.id.clone());
        tracker.matched_player_deck = Some(deck);
        tracker.current_payload = Some(blank_overlay_payload());

        assert!(
            tracker
                .refresh_game_state_snapshot()
                .expect("refresh game state")
        );
        tracker
            .write_current_snapshot_payload()
            .expect("write payload");

        let payload: TextOverlayPayload =
            serde_json::from_slice(&fs::read(output_json).expect("read payload"))
                .expect("payload parses");
        assert_eq!(payload.player.counters.removed, 1);
        assert_eq!(payload.player.removed[0].label, "Brood");
        assert!(
            payload
                .player
                .deck_slots
                .iter()
                .filter_map(|slot| slot.card.as_ref())
                .any(|card| card.label == "Brood" && card.consumed_from_deck)
        );
    }

    #[test]
    fn opponent_graveyard_candidates_only_include_new_opponent_cards() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/snapshots/sanitized-active-turn.json"
        ))
        .expect("fixture parses");
        let mut previous = observe_game_state(&fixture).expect("previous observation");
        let mut current = previous.clone();

        previous.cards.push(observed_card(
            "prev-opponent-grave",
            "OldOpponentDiscard",
            Participant::Opponent,
        ));
        current.cards = previous.cards.clone();
        current.cards.push(observed_card(
            "new-opponent-grave",
            "ArnimZola",
            Participant::Opponent,
        ));
        current.cards.push(observed_card(
            "new-player-grave",
            "LadySif",
            Participant::Player,
        ));

        assert_eq!(
            opponent_graveyard_discard_candidates(Some(&previous), &current),
            vec![CardKey("ArnimZola".to_string())]
        );
    }

    #[test]
    fn parses_current_and_legacy_selected_deck_shapes() {
        assert_eq!(
            parse_selected_deck_id(&serde_json::json!({
                "SerializedSelectedDeckId": "serialized-id"
            })),
            Some("serialized-id".to_string())
        );
        assert_eq!(
            parse_selected_deck_id(&serde_json::json!({
                "SelectedDeckId": {
                    "Value": "legacy-id"
                }
            })),
            Some("legacy-id".to_string())
        );
    }

    fn observed_card(
        id: &str,
        key: &str,
        participant: Participant,
    ) -> state_reader::CardObservation {
        state_reader::CardObservation {
            entity_id: id.bytes().map(i64::from).sum(),
            card_definition_key: Some(CardKey(key.to_string())),
            owner: participant,
            raw_zone: Some("Graveyard".to_string()),
            domain_zone: Zone::Discarded,
            previous_raw_zone: None,
            previous_domain_zone: None,
            zone_position: None,
            started_in_deck_entity_id: Some(1),
        }
    }
}
