use anyhow::{Context, Result, bail};
use clap::Parser;
use domain::{CardKnowledge, MatchEvent};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use state_reader::{
    OverlayProjection, OverlayProjector, ParticipantOverlayProjection, ReadOptions,
    ReconciliationInput, SnapshotObservation, observe_game_state, read_json_snapshot,
    reconcile_observation, text_overlay_payload,
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use walkdir::WalkDir;

const DEFAULT_STATE_SUFFIX: &str = ".steam/steam/steamapps/compatdata/1997040/pfx/drive_c/users/steamuser/AppData/LocalLow/Second Dinner/SNAP/Standalone/States/nvprod";
const DEFAULT_FILENAMES: &[&str] = &["GameState.json", "PlayState.json", "CollectionState.json"];

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Capture read-only Marvel Snap state snapshots"
)]
struct Args {
    #[arg(long)]
    state_dir: Option<PathBuf>,
    #[arg(long, default_value = "captures")]
    output_dir: PathBuf,
    #[arg(long, default_value_t = 1000)]
    interval_ms: u64,
    #[arg(
        long,
        value_delimiter = ',',
        default_value = "GameState.json,PlayState.json,CollectionState.json"
    )]
    files: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    redact: Vec<String>,
    #[arg(long, default_value_t = false)]
    once: bool,
    #[arg(long, value_name = "CAPTURE_DIR")]
    inspect_captures: Option<PathBuf>,
    #[arg(long, default_value_t = 24)]
    inspect_card_limit: usize,
    #[arg(long, value_name = "CAPTURE_DIR")]
    replay_captures: Option<PathBuf>,
    #[arg(long, default_value_t = false)]
    replay_chronological: bool,
    #[arg(long, value_name = "PATH")]
    export_overlay_json: Option<PathBuf>,
}

#[derive(Clone, Debug)]
struct CaptureConfig {
    state_dir: PathBuf,
    output_dir: PathBuf,
    interval: Duration,
    files: Vec<String>,
    redactions: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub capture_timestamp: String,
    pub source_filename: String,
    pub output_filename: Option<String>,
    pub content_hash: String,
    pub parse_status: ParseStatus,
    pub game_state_fingerprint: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParseStatus {
    Parsed,
    Malformed { message: String },
    Missing { message: String },
}

fn main() -> Result<()> {
    let args = Args::parse();

    if let Some(capture_dir) = &args.inspect_captures {
        let report = inspect_captures(capture_dir, args.inspect_card_limit)?;
        print!("{report}");
        return Ok(());
    }

    if let Some(capture_dir) = &args.replay_captures {
        let report = replay_captures(
            capture_dir,
            args.replay_chronological,
            args.export_overlay_json.as_deref(),
        )?;
        print!("{report}");
        return Ok(());
    }

    let state_dir = args
        .state_dir
        .or_else(default_state_dir)
        .context("could not determine a state directory; pass --state-dir")?;

    let config = CaptureConfig {
        state_dir,
        output_dir: args.output_dir,
        interval: Duration::from_millis(args.interval_ms),
        files: args.files,
        redactions: args.redact,
    };

    let running = Arc::new(AtomicBool::new(true));
    let signal_flag = Arc::clone(&running);
    ctrlc::set_handler(move || {
        signal_flag.store(false, Ordering::SeqCst);
    })
    .context("install SIGINT handler")?;

    let mut seen_hashes = HashSet::new();
    while running.load(Ordering::SeqCst) {
        capture_once(&config, &mut seen_hashes)?;
        if args.once {
            break;
        }
        thread::sleep(config.interval);
    }

    Ok(())
}

fn default_state_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(DEFAULT_STATE_SUFFIX))
}

fn capture_once(
    config: &CaptureConfig,
    seen_hashes: &mut HashSet<String>,
) -> Result<Vec<ManifestEntry>> {
    fs::create_dir_all(&config.output_dir).context("create output directory")?;
    let mut entries = Vec::new();

    for filename in &config.files {
        validate_filename(filename)?;
        let source = config.state_dir.join(filename);
        let timestamp = OffsetDateTime::now_utc().format(&Rfc3339)?;

        match read_json_snapshot(&source, ReadOptions::default()) {
            Ok(snapshot) => {
                if seen_hashes.insert(snapshot.sha256.clone()) {
                    let mut redacted = snapshot.parsed.clone();
                    redact_json_paths(&mut redacted, &config.redactions);
                    let output_filename = snapshot_filename(filename, &timestamp, &snapshot.sha256);
                    let output_path = config.output_dir.join(&output_filename);
                    let bytes = serde_json::to_vec_pretty(&redacted)
                        .context("serialize redacted snapshot")?;
                    fs::write(&output_path, bytes)
                        .with_context(|| format!("write {}", output_path.display()))?;
                    entries.push(ManifestEntry {
                        capture_timestamp: timestamp,
                        source_filename: filename.clone(),
                        output_filename: Some(output_filename),
                        content_hash: snapshot.sha256,
                        parse_status: ParseStatus::Parsed,
                        game_state_fingerprint: fingerprint(&snapshot.parsed),
                    });
                }
            }
            Err(state_reader::SnapshotReadError::Malformed { message, .. }) => {
                let raw = fs::read(&source).unwrap_or_default();
                let hash = hex::encode(Sha256::digest(&raw));
                entries.push(ManifestEntry {
                    capture_timestamp: timestamp,
                    source_filename: filename.clone(),
                    output_filename: None,
                    content_hash: hash,
                    parse_status: ParseStatus::Malformed { message },
                    game_state_fingerprint: None,
                });
            }
            Err(state_reader::SnapshotReadError::Io { source, .. }) => {
                entries.push(ManifestEntry {
                    capture_timestamp: timestamp,
                    source_filename: filename.clone(),
                    output_filename: None,
                    content_hash: String::new(),
                    parse_status: ParseStatus::Missing {
                        message: source.to_string(),
                    },
                    game_state_fingerprint: None,
                });
            }
            Err(error) => return Err(error).context("read snapshot"),
        }
    }

    append_manifest(&config.output_dir, &entries)?;
    Ok(entries)
}

fn append_manifest(output_dir: &Path, entries: &[ManifestEntry]) -> Result<()> {
    if entries.is_empty() {
        return Ok(());
    }
    let path = output_dir.join("manifest.ndjson");
    let mut existing = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    for entry in entries {
        serde_json::to_writer(&mut existing, entry).context("write manifest entry")?;
        use std::io::Write as _;
        existing
            .write_all(b"\n")
            .context("write manifest newline")?;
    }
    Ok(())
}

fn validate_filename(filename: &str) -> Result<()> {
    let path = Path::new(filename);
    if filename.is_empty() || path.components().count() != 1 {
        bail!("capture filename must be a plain file name: {filename}");
    }
    Ok(())
}

fn snapshot_filename(source_filename: &str, timestamp: &str, hash: &str) -> String {
    let safe_timestamp = timestamp.replace([':', '.'], "-");
    let short_hash = hash.get(0..12).unwrap_or(hash);
    format!("{safe_timestamp}_{short_hash}_{source_filename}")
}

fn redact_json_paths(value: &mut serde_json::Value, redactions: &[String]) {
    for path in redactions {
        let segments: Vec<&str> = path
            .trim_matches('.')
            .split('.')
            .filter(|s| !s.is_empty())
            .collect();
        redact_one(value, &segments);
    }
}

fn redact_one(value: &mut serde_json::Value, segments: &[&str]) {
    if segments.is_empty() {
        *value = serde_json::Value::String("<redacted>".to_string());
        return;
    }
    match value {
        serde_json::Value::Object(map) => {
            if let Some(next) = map.get_mut(segments[0]) {
                redact_one(next, &segments[1..]);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                redact_one(item, segments);
            }
        }
        _ => {}
    }
}

fn fingerprint(value: &serde_json::Value) -> Option<String> {
    let remote_game = value.pointer("/RemoteGame/GameState")?;
    let bytes = serde_json::to_vec(remote_game).ok()?;
    Some(hex::encode(Sha256::digest(bytes)))
}

fn inspect_captures(capture_dir: &Path, card_limit: usize) -> Result<String> {
    let mut report = String::new();
    report.push_str("# OpenSnapTracker Capture Inspection\n\n");
    report.push_str(
        "Sanitized structural report. Raw player names, account IDs, and full JSON records are not printed.\n\n",
    );

    let mut scenario_dirs = scenario_dirs(capture_dir)?;
    scenario_dirs.sort();
    if scenario_dirs.is_empty() {
        scenario_dirs.push(capture_dir.to_path_buf());
    }

    for dir in scenario_dirs {
        inspect_scenario(&dir, card_limit, &mut report)?;
    }

    Ok(report)
}

fn replay_captures(
    capture_dir: &Path,
    chronological: bool,
    export_overlay_json: Option<&Path>,
) -> Result<String> {
    let mut report = String::new();
    report.push_str("# OpenSnapTracker Capture Replay\n\n");
    report.push_str(
        "Sanitized replay report. Events and overlay counts are derived from captured GameState snapshots; raw records are not printed.\n\n",
    );

    let final_projection = if chronological {
        replay_chronological_captures(capture_dir, &mut report)?
    } else {
        let mut scenario_dirs = scenario_dirs(capture_dir)?;
        scenario_dirs.sort();
        if scenario_dirs.is_empty() {
            scenario_dirs.push(capture_dir.to_path_buf());
        }

        let mut final_projection = None;
        for dir in scenario_dirs {
            final_projection = replay_scenario(&dir, &mut report)?.or(final_projection);
        }
        final_projection
    };

    if let Some(path) = export_overlay_json {
        let projection = final_projection.context("no replayed GameState snapshots to export")?;
        let payload = text_overlay_payload(&projection);
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)
                .with_context(|| format!("create overlay export directory {}", parent.display()))?;
        }
        let bytes = serde_json::to_vec_pretty(&payload).context("serialize overlay payload")?;
        fs::write(path, bytes).with_context(|| format!("write {}", path.display()))?;
        report.push_str(&format!(
            "Exported final text overlay payload to `{}`.\n",
            path.display()
        ));
    }

    Ok(report)
}

#[derive(Clone, Debug)]
struct ReplaySnapshot {
    scenario: String,
    entry: ManifestEntry,
    path: PathBuf,
}

fn replay_chronological_captures(
    capture_dir: &Path,
    report: &mut String,
) -> Result<Option<OverlayProjection>> {
    report.push_str("## Chronological Timeline\n\n");
    let mut snapshots = Vec::new();
    let mut scenario_dirs = scenario_dirs(capture_dir)?;
    scenario_dirs.sort();
    if scenario_dirs.is_empty() {
        scenario_dirs.push(capture_dir.to_path_buf());
    }

    for dir in scenario_dirs {
        let scenario = dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("<capture>")
            .to_string();
        let manifest_path = dir.join("manifest.ndjson");
        if !manifest_path.exists() {
            continue;
        }
        for entry in read_manifest(&manifest_path)?
            .into_iter()
            .filter(|entry| entry.source_filename == "GameState.json")
        {
            let Some(filename) = &entry.output_filename else {
                continue;
            };
            snapshots.push(ReplaySnapshot {
                scenario: scenario.clone(),
                path: dir.join(filename),
                entry,
            });
        }
    }

    snapshots.sort_by(|left, right| {
        left.entry
            .capture_timestamp
            .cmp(&right.entry.capture_timestamp)
            .then_with(|| left.entry.content_hash.cmp(&right.entry.content_hash))
    });
    snapshots.dedup_by(|left, right| left.entry.content_hash == right.entry.content_hash);

    let mut previous: Option<SnapshotObservation> = None;
    let mut projector = OverlayProjector::new();
    let mut final_projection = None;
    for snapshot in snapshots {
        let value = read_json_file(&snapshot.path)?;
        let observation = observe_game_state(&value)
            .with_context(|| format!("observe {}", snapshot.path.display()))?;
        let events = reconcile_observation(ReconciliationInput {
            previous: previous.as_ref(),
            current: &observation,
            snapshot_version: snapshot.entry.game_state_fingerprint.clone(),
        });
        let projection = projector.project(&observation, &events);

        report.push_str(&format!("Scenario: {}\n\n", snapshot.scenario));
        write_replay_step(&snapshot.entry, &observation, &events, &projection, report);
        final_projection = Some(projection);
        previous = Some(observation);
    }

    Ok(final_projection)
}

fn replay_scenario(dir: &Path, report: &mut String) -> Result<Option<OverlayProjection>> {
    let name = dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("<capture>");
    report.push_str("## ");
    report.push_str(name);
    report.push_str("\n\n");

    let manifest_path = dir.join("manifest.ndjson");
    if !manifest_path.exists() {
        report.push_str("- No manifest.ndjson found.\n\n");
        return Ok(None);
    }

    let entries = read_manifest(&manifest_path)?;
    let mut previous: Option<SnapshotObservation> = None;
    let mut projector = OverlayProjector::new();
    let mut replayed = 0usize;
    let mut final_projection = None;

    for entry in entries
        .iter()
        .filter(|entry| entry.source_filename == "GameState.json")
    {
        let Some(filename) = &entry.output_filename else {
            continue;
        };
        let path = dir.join(filename);
        let value = read_json_file(&path)?;
        let observation =
            observe_game_state(&value).with_context(|| format!("observe {}", path.display()))?;
        let events = reconcile_observation(ReconciliationInput {
            previous: previous.as_ref(),
            current: &observation,
            snapshot_version: entry.game_state_fingerprint.clone(),
        });
        let projection = projector.project(&observation, &events);

        replayed += 1;
        write_replay_step(entry, &observation, &events, &projection, report);
        final_projection = Some(projection);
        previous = Some(observation);
    }

    if replayed == 0 {
        report.push_str("- No parsed GameState snapshots to replay.\n");
    }
    report.push('\n');
    Ok(final_projection)
}

fn write_replay_step(
    entry: &ManifestEntry,
    observation: &SnapshotObservation,
    events: &[MatchEvent],
    projection: &OverlayProjection,
    report: &mut String,
) {
    report.push_str(&format!(
        "### GameState {} at {}\n\n",
        short_hash(&entry.content_hash),
        entry.capture_timestamp
    ));
    report.push_str(&format!(
        "- lifecycle={:?} turn={}/{} events={}\n",
        observation.lifecycle,
        fmt_opt_i64(observation.turn),
        fmt_opt_i64(observation.total_turns),
        events.len()
    ));
    report.push_str("- event counts:");
    for (event_type, count) in event_counts(events) {
        report.push_str(&format!(" {event_type}={count}"));
    }
    report.push('\n');

    write_participant_projection(&projection.player, report);
    write_participant_projection(&projection.opponent, report);

    if !projection.warnings.is_empty() {
        report.push_str("- warnings:\n");
        for warning in &projection.warnings {
            report.push_str("  - ");
            report.push_str(warning);
            report.push('\n');
        }
    }
    report.push('\n');
}

fn write_participant_projection(participant: &ParticipantOverlayProjection, report: &mut String) {
    let known = participant
        .cards
        .iter()
        .filter(|card| card.knowledge == CardKnowledge::KnownCard)
        .count();
    let hidden = participant.cards.len().saturating_sub(known);
    let consumed_original = participant
        .cards
        .iter()
        .filter(|card| card.consumed_from_deck)
        .count();

    report.push_str(&format!(
        "- {:?}: deck={} hand={} board={} destroyed={} discarded={} removed={} unknown_transition={} cards={} known={} hidden={} consumed_original={}\n",
        participant.participant,
        participant.deck_count,
        participant.hand_count,
        participant.board_count,
        participant.destroyed_count,
        participant.discarded_count,
        participant.removed_count,
        participant.unknown_transition_count,
        participant.cards.len(),
        known,
        hidden,
        consumed_original
    ));
}

fn event_counts(events: &[MatchEvent]) -> BTreeMap<&'static str, usize> {
    let mut counts = BTreeMap::new();
    for event in events {
        *counts.entry(event_type(event)).or_insert(0) += 1;
    }
    counts
}

fn event_type(event: &MatchEvent) -> &'static str {
    match event {
        MatchEvent::MatchStarted { .. } => "match_started",
        MatchEvent::MatchEnded => "match_ended",
        MatchEvent::DeckIdentified { .. } => "deck_identified",
        MatchEvent::CardInstanceObserved { .. } => "card_instance_observed",
        MatchEvent::CardDrawn { .. } => "card_drawn",
        MatchEvent::CardPlayed { .. } => "card_played",
        MatchEvent::CardRevealed { .. } => "card_revealed",
        MatchEvent::CardReturned { .. } => "card_returned",
        MatchEvent::CardDestroyed { .. } => "card_destroyed",
        MatchEvent::CardDiscarded { .. } => "card_discarded",
        MatchEvent::CardRemoved { .. } => "card_removed",
        MatchEvent::CardGenerated { .. } => "card_generated",
        MatchEvent::CardTransferred { .. } => "card_transferred",
        MatchEvent::CardTransformed { .. } => "card_transformed",
        MatchEvent::CardMerged { .. } => "card_merged",
        MatchEvent::SnapshotParseWarning { .. } => "snapshot_parse_warning",
        MatchEvent::UnknownTransitionObserved { .. } => "unknown_transition_observed",
    }
}

fn scenario_dirs(capture_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut dirs = Vec::new();
    for entry in fs::read_dir(capture_dir)
        .with_context(|| format!("read capture directory {}", capture_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let is_helper_dir = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with('_'));
        if path.is_dir() && !is_helper_dir && path.join("manifest.ndjson").exists() {
            dirs.push(path);
        }
    }
    Ok(dirs)
}

fn inspect_scenario(dir: &Path, card_limit: usize, report: &mut String) -> Result<()> {
    let name = dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("<capture>");
    report.push_str("## ");
    report.push_str(name);
    report.push_str("\n\n");

    let manifest_path = dir.join("manifest.ndjson");
    if !manifest_path.exists() {
        report.push_str("- No manifest.ndjson found.\n\n");
        return Ok(());
    }

    let entries = read_manifest(&manifest_path)?;
    let parsed = entries
        .iter()
        .filter(|entry| entry.parse_status == ParseStatus::Parsed)
        .count();
    report.push_str(&format!(
        "- manifest entries: {} parsed: {}\n",
        entries.len(),
        parsed
    ));

    let mut previous: Option<SnapshotSummary> = None;
    for entry in entries
        .iter()
        .filter(|entry| entry.source_filename == "GameState.json")
    {
        let Some(filename) = &entry.output_filename else {
            report.push_str(&format!(
                "- GameState {} parse_status={:?}\n",
                short_hash(&entry.content_hash),
                entry.parse_status
            ));
            continue;
        };
        let path = dir.join(filename);
        let value = read_json_file(&path)?;
        let summary = summarize_snapshot(entry, &value);
        write_snapshot_summary(&summary, card_limit, report);
        if let Some(previous_summary) = &previous {
            write_transition_summary(previous_summary, &summary, report);
        }
        previous = Some(summary);
    }

    report.push('\n');
    Ok(())
}

fn read_manifest(path: &Path) -> Result<Vec<ManifestEntry>> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).context("parse manifest line"))
        .collect()
}

fn read_json_file(path: &Path) -> Result<Value> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let json_text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    serde_json::from_str(json_text).with_context(|| format!("parse {}", path.display()))
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct SnapshotSummary {
    timestamp: String,
    hash: String,
    fingerprint: Option<String>,
    application_version: Option<String>,
    turn: Option<i64>,
    total_turns: Option<i64>,
    winner_present: bool,
    client_result_present: bool,
    battle_result_present: bool,
    game_mode_type: Option<String>,
    local_player: Option<i64>,
    enemy_player: Option<i64>,
    players: Vec<PlayerSummary>,
    cards: Vec<CardSummary>,
    client_cards_drawn: usize,
    client_cards_played: usize,
    client_stage_requests: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct PlayerSummary {
    entity_id: Option<i64>,
    role: String,
    zones: BTreeMap<String, ZoneSummary>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct ZoneSummary {
    cards: usize,
    known: usize,
    hidden: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CardSummary {
    entity_id: i64,
    owner_role: String,
    def: Option<String>,
    zone: Option<String>,
    previous_zone: Option<String>,
    zone_position: Option<i64>,
    started_in_deck_entity_id: Option<i64>,
}

fn summarize_snapshot(entry: &ManifestEntry, root: &Value) -> SnapshotSummary {
    let index = JsonNetIndex::new(root);
    let remote = root.pointer("/RemoteGame").unwrap_or(&Value::Null);
    let game_state = remote.pointer("/GameState").unwrap_or(&Value::Null);
    let client_info = remote.pointer("/ClientPlayerInfo").unwrap_or(&Value::Null);
    let local_player = remote
        .pointer("/ClientGameInfo/LocalPlayerEntityId")
        .and_then(Value::as_i64);
    let enemy_player = remote
        .pointer("/ClientGameInfo/EnemyPlayerEntityId")
        .and_then(Value::as_i64);

    let mut summary = SnapshotSummary {
        timestamp: entry.capture_timestamp.clone(),
        hash: entry.content_hash.clone(),
        fingerprint: entry.game_state_fingerprint.clone(),
        application_version: root
            .get("ApplicationVersion")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        turn: game_state.get("Turn").and_then(Value::as_i64),
        total_turns: game_state.get("TotalTurns").and_then(Value::as_i64),
        winner_present: game_state.get("Winner").is_some(),
        client_result_present: game_state.get("ClientResultMessage").is_some(),
        battle_result_present: game_state
            .pointer("/GameMode/Data/ClientBattleResultMessage")
            .is_some(),
        game_mode_type: game_state
            .pointer("/GameMode/$type")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        local_player,
        enemy_player,
        players: Vec::new(),
        cards: Vec::new(),
        client_cards_drawn: array_len(client_info.get("CardsDrawn")),
        client_cards_played: array_len(client_info.get("CardsPlayed")),
        client_stage_requests: array_len(client_info.get("ClientStageRequests")),
    };

    let mut player_roles = HashMap::new();
    if let Some(players) = game_state.get("_players").and_then(Value::as_array) {
        for player_value in players {
            let player = index.resolve(player_value);
            let entity_id = player.get("EntityId").and_then(Value::as_i64);
            let role = player_role(entity_id, local_player, enemy_player);
            if let Some(entity_id) = entity_id {
                player_roles.insert(entity_id, role.clone());
            }

            let mut player_summary = PlayerSummary {
                entity_id,
                role,
                zones: BTreeMap::new(),
            };
            for zone_name in ["Deck", "Hand", "Graveyard", "Banished"] {
                let zone = index.resolve(player.get(zone_name).unwrap_or(&Value::Null));
                let zone_summary = summarize_zone(zone, &index);
                player_summary
                    .zones
                    .insert(zone_name.to_string(), zone_summary);
            }
            summary.players.push(player_summary);
        }
    }

    summary.cards = collect_cards(root, &index, &player_roles);
    summary.cards.sort_by(|left, right| {
        left.owner_role
            .cmp(&right.owner_role)
            .then_with(|| left.zone.cmp(&right.zone))
            .then_with(|| left.zone_position.cmp(&right.zone_position))
            .then_with(|| left.entity_id.cmp(&right.entity_id))
    });

    summary
}

fn array_len(value: Option<&Value>) -> usize {
    value.and_then(Value::as_array).map_or(0, Vec::len)
}

fn player_role(
    entity_id: Option<i64>,
    local_player: Option<i64>,
    enemy_player: Option<i64>,
) -> String {
    match entity_id {
        Some(id) if Some(id) == local_player => "local".to_string(),
        Some(id) if Some(id) == enemy_player => "enemy".to_string(),
        Some(_) => "unknown".to_string(),
        None => "unresolved_ref".to_string(),
    }
}

fn summarize_zone(zone: &Value, index: &JsonNetIndex<'_>) -> ZoneSummary {
    let mut summary = ZoneSummary::default();
    let Some(cards) = zone.get("_cards").and_then(Value::as_array) else {
        return summary;
    };
    summary.cards = cards.len();
    for card_ref in cards {
        let card = index.resolve(card_ref);
        if card.get("CardDefId").and_then(Value::as_str).is_some() {
            summary.known += 1;
        } else {
            summary.hidden += 1;
        }
    }
    summary
}

fn collect_cards(
    root: &Value,
    index: &JsonNetIndex<'_>,
    player_roles: &HashMap<i64, String>,
) -> Vec<CardSummary> {
    let mut cards = Vec::new();
    collect_cards_inner(root, index, player_roles, &mut cards);
    cards.sort_by_key(|card| card.entity_id);
    cards.dedup_by_key(|card| card.entity_id);
    cards
}

fn collect_cards_inner(
    value: &Value,
    index: &JsonNetIndex<'_>,
    player_roles: &HashMap<i64, String>,
    cards: &mut Vec<CardSummary>,
) {
    match value {
        Value::Object(map) => {
            if map
                .get("$type")
                .and_then(Value::as_str)
                .is_some_and(|type_name| type_name.starts_with("CubeGame.Card,"))
                && let Some(entity_id) = map.get("EntityId").and_then(Value::as_i64)
            {
                let owner = index.resolve(map.get("Owner").unwrap_or(&Value::Null));
                let owner_entity = owner.get("EntityId").and_then(Value::as_i64);
                let owner_role = owner_entity
                    .and_then(|id| player_roles.get(&id).cloned())
                    .unwrap_or_else(|| "unknown".to_string());
                let zone = index.resolve(map.get("_zone").unwrap_or(&Value::Null));
                let previous_zone = index.resolve(map.get("_previousZone").unwrap_or(&Value::Null));
                cards.push(CardSummary {
                    entity_id,
                    owner_role,
                    def: map
                        .get("CardDefId")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                    zone: zone
                        .get("ZoneId")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                    previous_zone: previous_zone
                        .get("ZoneId")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                    zone_position: map.get("ZonePosition").and_then(Value::as_i64),
                    started_in_deck_entity_id: map
                        .get("StartedInDeckEntityId")
                        .and_then(Value::as_i64),
                });
            }

            for child in map.values() {
                collect_cards_inner(child, index, player_roles, cards);
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_cards_inner(child, index, player_roles, cards);
            }
        }
        _ => {}
    }
}

fn write_snapshot_summary(summary: &SnapshotSummary, card_limit: usize, report: &mut String) {
    report.push_str(&format!(
        "\n### GameState {} at {}\n\n",
        short_hash(&summary.hash),
        summary.timestamp
    ));
    report.push_str(&format!(
        "- fingerprint: {} app: {}\n",
        summary
            .fingerprint
            .as_deref()
            .map(short_hash)
            .unwrap_or("-"),
        summary.application_version.as_deref().unwrap_or("-")
    ));
    report.push_str(&format!(
        "- turn: {} / {} winner_present: {} result: {} battle_result: {}\n",
        fmt_opt_i64(summary.turn),
        fmt_opt_i64(summary.total_turns),
        summary.winner_present,
        summary.client_result_present,
        summary.battle_result_present
    ));
    report.push_str(&format!(
        "- players: local={} enemy={} game_mode={}\n",
        fmt_opt_i64(summary.local_player),
        fmt_opt_i64(summary.enemy_player),
        summary.game_mode_type.as_deref().unwrap_or("-")
    ));
    report.push_str(&format!(
        "- client lists: drawn={} played={} stage_requests={}\n",
        summary.client_cards_drawn, summary.client_cards_played, summary.client_stage_requests
    ));

    for player in &summary.players {
        report.push_str(&format!(
            "- player role={} entity={}:",
            player.role,
            fmt_opt_i64(player.entity_id)
        ));
        for (zone_name, zone) in &player.zones {
            report.push_str(&format!(
                " {zone_name}={}({} known/{} hidden)",
                zone.cards, zone.known, zone.hidden
            ));
        }
        report.push('\n');
    }

    let known = summary
        .cards
        .iter()
        .filter(|card| card.def.is_some())
        .count();
    let hidden = summary.cards.len().saturating_sub(known);
    let started = summary
        .cards
        .iter()
        .filter(|card| card.started_in_deck_entity_id.is_some())
        .count();
    report.push_str(&format!(
        "- unique cards observed: {} known={} hidden={} started_in_deck_refs={}\n",
        summary.cards.len(),
        known,
        hidden,
        started
    ));

    for card in summary.cards.iter().take(card_limit) {
        report.push_str(&format!(
            "  - card entity={} owner={} def={} zone={} prev={} pos={} started_deck={}\n",
            card.entity_id,
            card.owner_role,
            card.def.as_deref().unwrap_or("<hidden>"),
            card.zone.as_deref().unwrap_or("-"),
            card.previous_zone.as_deref().unwrap_or("-"),
            fmt_opt_i64(card.zone_position),
            fmt_opt_i64(card.started_in_deck_entity_id)
        ));
    }
    if summary.cards.len() > card_limit {
        report.push_str(&format!(
            "  - ... {} more cards omitted by --inspect-card-limit\n",
            summary.cards.len() - card_limit
        ));
    }
}

fn write_transition_summary(
    previous: &SnapshotSummary,
    current: &SnapshotSummary,
    report: &mut String,
) {
    let previous_cards: HashMap<i64, &CardSummary> = previous
        .cards
        .iter()
        .map(|card| (card.entity_id, card))
        .collect();
    let mut changes = Vec::new();

    for card in &current.cards {
        if let Some(old) = previous_cards.get(&card.entity_id) {
            if old.zone != card.zone || old.def != card.def || old.owner_role != card.owner_role {
                changes.push(format!(
                    "  - entity={} owner {}->{} def {}->{} zone {}->{}\n",
                    card.entity_id,
                    old.owner_role,
                    card.owner_role,
                    old.def.as_deref().unwrap_or("<hidden>"),
                    card.def.as_deref().unwrap_or("<hidden>"),
                    old.zone.as_deref().unwrap_or("-"),
                    card.zone.as_deref().unwrap_or("-")
                ));
            }
        } else {
            changes.push(format!(
                "  - entity={} appeared owner={} def={} zone={}\n",
                card.entity_id,
                card.owner_role,
                card.def.as_deref().unwrap_or("<hidden>"),
                card.zone.as_deref().unwrap_or("-")
            ));
        }
    }

    if changes.is_empty() {
        report.push_str(
            "\nTransition from previous GameState: no card identity/zone changes detected.\n",
        );
        return;
    }

    report.push_str("\nTransition from previous GameState:\n");
    for change in changes.iter().take(24) {
        report.push_str(change);
    }
    if changes.len() > 24 {
        report.push_str(&format!(
            "  - ... {} more transition rows omitted\n",
            changes.len() - 24
        ));
    }
}

fn fmt_opt_i64(value: Option<i64>) -> String {
    value.map_or_else(|| "-".to_string(), |value| value.to_string())
}

fn short_hash(hash: &str) -> &str {
    hash.get(0..12).unwrap_or(hash)
}

#[derive(Debug)]
struct JsonNetIndex<'a> {
    values_by_id: HashMap<&'a str, &'a Value>,
}

impl<'a> JsonNetIndex<'a> {
    fn new(root: &'a Value) -> Self {
        let mut index = Self {
            values_by_id: HashMap::new(),
        };
        index.collect(root);
        index
    }

    fn resolve<'b>(&'b self, value: &'b Value) -> &'b Value
    where
        'a: 'b,
    {
        value
            .get("$ref")
            .and_then(Value::as_str)
            .and_then(|id| self.values_by_id.get(id).copied())
            .unwrap_or(value)
    }

    fn collect(&mut self, value: &'a Value) {
        match value {
            Value::Object(map) => {
                if let Some(id) = map.get("$id").and_then(Value::as_str) {
                    self.values_by_id.insert(id, value);
                }
                for child in map.values() {
                    self.collect(child);
                }
            }
            Value::Array(items) => {
                for child in items {
                    self.collect(child);
                }
            }
            _ => {}
        }
    }
}

#[allow(dead_code)]
fn available_snapshots(dir: &Path) -> Vec<PathBuf> {
    WalkDir::new(dir)
        .max_depth(1)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.into_path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| DEFAULT_FILENAMES.contains(&name))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn captures_changed_content_once_and_redacts() {
        let state = tempdir().expect("state tempdir");
        let output = tempdir().expect("output tempdir");
        fs::write(
            state.path().join("GameState.json"),
            r#"{"AccountId":"secret","RemoteGame":{"GameState":{"Turn":1}}}"#,
        )
        .expect("write state");
        let config = CaptureConfig {
            state_dir: state.path().to_path_buf(),
            output_dir: output.path().to_path_buf(),
            interval: Duration::ZERO,
            files: vec!["GameState.json".to_string()],
            redactions: vec!["AccountId".to_string()],
        };
        let mut seen = HashSet::new();

        let first = capture_once(&config, &mut seen).expect("first capture");
        let second = capture_once(&config, &mut seen).expect("second capture");

        assert_eq!(first.len(), 1);
        assert!(second.is_empty());
        let captured = fs::read_to_string(
            output
                .path()
                .join(first[0].output_filename.as_ref().expect("filename")),
        )
        .expect("read captured snapshot");
        assert!(captured.contains("<redacted>"));
        assert!(!captured.contains("secret"));
    }

    #[test]
    fn rejects_nested_capture_file_names() {
        let err = validate_filename("../GameState.json").expect_err("nested path rejected");
        assert!(err.to_string().contains("plain file name"));
    }

    #[test]
    fn inspect_report_resolves_json_net_references_without_private_fields() {
        let captures = tempdir().expect("captures tempdir");
        let scenario = captures.path().join("001-synthetic");
        fs::create_dir(&scenario).expect("scenario dir");
        let game_state = serde_json::json!({
            "$id": "1",
            "ApplicationVersion": "test",
            "RemoteGame": {
                "ClientGameInfo": {
                    "LocalPlayerEntityId": 2,
                    "EnemyPlayerEntityId": 7
                },
                "ClientPlayerInfo": {
                    "Name": "private name",
                    "AccountId": "private account",
                    "CardsDrawn": [21],
                    "CardsPlayed": [],
                    "ClientStageRequests": []
                },
                "GameState": {
                    "Turn": 1,
                    "TotalTurns": 6,
                    "GameMode": {"$type": "CubeGame.BattleGameMode, SecondDinner.CubeGame.Logic"},
                    "_players": [
                        {
                            "$id": "2",
                            "$type": "CubeGame.Player, SecondDinner.CubeGame.Logic",
                            "EntityId": 2,
                            "Deck": {
                                "$id": "3",
                                "$type": "CubeGame.Deck, SecondDinner.CubeGame.Logic",
                                "ZoneId": "Deck",
                                "_cards": [{"$ref": "4"}]
                            },
                            "Hand": {"$id": "5", "ZoneId": "Hand", "_cards": []},
                            "Graveyard": {"$id": "6", "ZoneId": "Graveyard", "_cards": []},
                            "Banished": {"$id": "7", "ZoneId": "Banished", "_cards": []}
                        },
                        {"$id": "8", "$type": "CubeGame.Player, SecondDinner.CubeGame.Logic", "EntityId": 7}
                    ],
                    "card": {
                        "$id": "4",
                        "$type": "CubeGame.Card, SecondDinner.CubeGame.Logic",
                        "EntityId": 21,
                        "CardDefId": "Abomination",
                        "Owner": {"$ref": "2"},
                        "_zone": {"$ref": "3"},
                        "StartedInDeckEntityId": 3,
                        "ZonePosition": 0
                    }
                }
            }
        });
        let output_filename = "2026-07-14T00-00-00Z_abc_GameState.json";
        fs::write(
            scenario.join(output_filename),
            serde_json::to_vec_pretty(&game_state).expect("serialize"),
        )
        .expect("write game state");
        fs::write(
            scenario.join("manifest.ndjson"),
            serde_json::to_string(&ManifestEntry {
                capture_timestamp: "2026-07-14T00:00:00Z".to_string(),
                source_filename: "GameState.json".to_string(),
                output_filename: Some(output_filename.to_string()),
                content_hash: "abcdef1234567890".to_string(),
                parse_status: ParseStatus::Parsed,
                game_state_fingerprint: Some("fedcba654321".to_string()),
            })
            .expect("manifest json")
                + "\n",
        )
        .expect("write manifest");

        let report = inspect_captures(captures.path(), 10).expect("inspect captures");

        assert!(report.contains("role=local"));
        assert!(report.contains("Deck=1(1 known/0 hidden)"));
        assert!(report.contains("def=Abomination"));
        assert!(report.contains("zone=Deck"));
        assert!(!report.contains("private name"));
        assert!(!report.contains("private account"));
    }

    #[test]
    fn replay_report_summarizes_events_and_projection() {
        let captures = tempdir().expect("captures tempdir");
        let scenario = captures.path().join("001-replay");
        fs::create_dir(&scenario).expect("scenario dir");

        let first_filename = "2026-07-14T00-00-00Z_aaa_GameState.json";
        let second_filename = "2026-07-14T00-00-01Z_bbb_GameState.json";
        fs::write(
            scenario.join(first_filename),
            include_str!("../../../fixtures/snapshots/sanitized-active-turn.json"),
        )
        .expect("write first fixture");
        fs::write(
            scenario.join(second_filename),
            include_str!("../../../fixtures/snapshots/sanitized-draw-after.json"),
        )
        .expect("write second fixture");

        let manifest = [
            ManifestEntry {
                capture_timestamp: "2026-07-14T00:00:00Z".to_string(),
                source_filename: "GameState.json".to_string(),
                output_filename: Some(first_filename.to_string()),
                content_hash: "aaaaaaaaaaaa".to_string(),
                parse_status: ParseStatus::Parsed,
                game_state_fingerprint: Some("fingerprint-a".to_string()),
            },
            ManifestEntry {
                capture_timestamp: "2026-07-14T00:00:01Z".to_string(),
                source_filename: "GameState.json".to_string(),
                output_filename: Some(second_filename.to_string()),
                content_hash: "bbbbbbbbbbbb".to_string(),
                parse_status: ParseStatus::Parsed,
                game_state_fingerprint: Some("fingerprint-b".to_string()),
            },
        ];
        let manifest_text = manifest
            .iter()
            .map(|entry| serde_json::to_string(entry).expect("manifest entry serializes"))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        fs::write(scenario.join("manifest.ndjson"), manifest_text).expect("write manifest");

        let report = replay_captures(captures.path(), false, None).expect("replay captures");

        assert!(report.contains("card_drawn=1"));
        assert!(report.contains("card_revealed=1"));
        assert!(report.contains("Player: deck=0 hand=2 board=0 destroyed=0 discarded=0"));
        assert!(report.contains("Opponent: deck=1 hand=1 board=0 destroyed=0 discarded=0"));
    }

    #[test]
    fn replay_can_export_final_text_overlay_payload() {
        let captures = tempdir().expect("captures tempdir");
        let scenario = captures.path().join("001-replay");
        fs::create_dir(&scenario).expect("scenario dir");

        let first_filename = "2026-07-14T00-00-00Z_aaa_GameState.json";
        let second_filename = "2026-07-14T00-00-01Z_bbb_GameState.json";
        fs::write(
            scenario.join(first_filename),
            include_str!("../../../fixtures/snapshots/sanitized-active-turn.json"),
        )
        .expect("write first fixture");
        fs::write(
            scenario.join(second_filename),
            include_str!("../../../fixtures/snapshots/sanitized-draw-after.json"),
        )
        .expect("write second fixture");

        let manifest = [
            ManifestEntry {
                capture_timestamp: "2026-07-14T00:00:00Z".to_string(),
                source_filename: "GameState.json".to_string(),
                output_filename: Some(first_filename.to_string()),
                content_hash: "aaaaaaaaaaaa".to_string(),
                parse_status: ParseStatus::Parsed,
                game_state_fingerprint: Some("fingerprint-a".to_string()),
            },
            ManifestEntry {
                capture_timestamp: "2026-07-14T00:00:01Z".to_string(),
                source_filename: "GameState.json".to_string(),
                output_filename: Some(second_filename.to_string()),
                content_hash: "bbbbbbbbbbbb".to_string(),
                parse_status: ParseStatus::Parsed,
                game_state_fingerprint: Some("fingerprint-b".to_string()),
            },
        ];
        let manifest_text = manifest
            .iter()
            .map(|entry| serde_json::to_string(entry).expect("manifest entry serializes"))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        fs::write(scenario.join("manifest.ndjson"), manifest_text).expect("write manifest");

        let export_path = captures.path().join("_derived").join("overlay.json");
        let report = replay_captures(captures.path(), true, Some(&export_path))
            .expect("replay captures with export");
        let payload: serde_json::Value =
            serde_json::from_slice(&fs::read(&export_path).expect("read exported payload"))
                .expect("payload parses");

        assert!(report.contains("Exported final text overlay payload"));
        assert_eq!(payload["schema_version"], 1);
        assert_eq!(
            payload["player"]["deck_slots"]
                .as_array()
                .expect("player slots array")
                .len(),
            12
        );
        assert_eq!(
            payload["opponent"]["deck_slots"]
                .as_array()
                .expect("opponent slots array")
                .len(),
            12
        );
        assert_eq!(payload["player"]["counters"]["hand"], 2);
    }
}
