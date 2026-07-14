use anyhow::{Context, Result};
use clap::Parser;
use domain::{CardInstanceId, CardKey, CardKnowledge, Zone};
use state_reader::{
    OverlayProjector, ReadOptions, ReconciliationInput, SnapshotObservation, TextOverlayCard,
    TextOverlayPayload, TextOverlaySlot, TextOverlaySlotState, observe_game_state,
    read_json_snapshot, reconcile_observation, text_overlay_payload,
};
use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
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
}

#[derive(Debug)]
struct LiveTracker {
    state_file: PathBuf,
    collection_file: PathBuf,
    play_file: PathBuf,
    output_json: PathBuf,
    interval: Duration,
    previous_hash: Option<String>,
    previous_collection_hash: Option<String>,
    previous_play_hash: Option<String>,
    selected_deck_id: Option<String>,
    collection_decks: Vec<CollectionDeck>,
    matched_player_deck: Option<CollectionDeck>,
    previous_observation: Option<SnapshotObservation>,
    projector: OverlayProjector,
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

    while running.load(Ordering::SeqCst) {
        if let Err(error) = tracker.tick() {
            eprintln!("[tracker-sidecar] {error:#}");
        }
        if args.once {
            break;
        }
        thread::sleep(tracker.interval);
    }

    Ok(())
}

impl LiveTracker {
    fn new(state_dir: PathBuf, output_json: PathBuf, interval: Duration) -> Self {
        Self {
            state_file: state_dir.join(GAME_STATE_FILENAME),
            collection_file: state_dir.join(COLLECTION_STATE_FILENAME),
            play_file: state_dir.join(PLAY_STATE_FILENAME),
            output_json,
            interval,
            previous_hash: None,
            previous_collection_hash: None,
            previous_play_hash: None,
            selected_deck_id: None,
            collection_decks: Vec::new(),
            matched_player_deck: None,
            previous_observation: None,
            projector: OverlayProjector::new(),
        }
    }

    fn tick(&mut self) -> Result<bool> {
        let snapshot = read_json_snapshot(&self.state_file, ReadOptions::default())
            .with_context(|| format!("read {}", self.state_file.display()))?;
        self.refresh_play_state()?;
        self.refresh_collection_decks()?;
        if self.previous_hash.as_deref() == Some(snapshot.sha256.as_str()) {
            return Ok(false);
        }

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
        write_json_atomic(&self.output_json, &serde_json::to_vec_pretty(&payload)?)?;

        self.previous_hash = Some(snapshot.sha256);
        self.previous_observation = Some(observation);
        Ok(true)
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
        self.selected_deck_id = parse_selected_deck_id(&snapshot.parsed);
        self.previous_play_hash = Some(snapshot.sha256);
        self.matched_player_deck = None;
        Ok(())
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
            self.apply_player_deck(payload, &deck);
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
            self.matched_player_deck = Some(deck.clone());
        }

        let Some(deck) = &self.matched_player_deck else {
            return;
        };
        self.apply_player_deck(payload, deck);
    }

    fn apply_player_deck(&self, payload: &mut TextOverlayPayload, deck: &CollectionDeck) {
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
}

fn default_state_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(DEFAULT_STATE_SUFFIX))
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
}
