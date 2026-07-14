use anyhow::{Context, Result};
use clap::Parser;
use state_reader::{
    OverlayProjector, ReadOptions, ReconciliationInput, SnapshotObservation, observe_game_state,
    read_json_snapshot, reconcile_observation, text_overlay_payload,
};
use std::{
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
    output_json: PathBuf,
    interval: Duration,
    previous_hash: Option<String>,
    previous_observation: Option<SnapshotObservation>,
    projector: OverlayProjector,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let state_dir = args
        .state_dir
        .or_else(default_state_dir)
        .context("could not determine a state directory; pass --state-dir")?;
    let mut tracker = LiveTracker::new(
        state_dir.join(GAME_STATE_FILENAME),
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
    fn new(state_file: PathBuf, output_json: PathBuf, interval: Duration) -> Self {
        Self {
            state_file,
            output_json,
            interval,
            previous_hash: None,
            previous_observation: None,
            projector: OverlayProjector::new(),
        }
    }

    fn tick(&mut self) -> Result<bool> {
        let snapshot = read_json_snapshot(&self.state_file, ReadOptions::default())
            .with_context(|| format!("read {}", self.state_file.display()))?;
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
        let payload = text_overlay_payload(&projection);
        write_json_atomic(&self.output_json, &serde_json::to_vec_pretty(&payload)?)?;

        self.previous_hash = Some(snapshot.sha256);
        self.previous_observation = Some(observation);
        Ok(true)
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn live_tracker_writes_payload_once_and_skips_unchanged_hash() {
        let temp = tempdir().expect("tempdir");
        let state_file = temp.path().join(GAME_STATE_FILENAME);
        let output_json = temp.path().join("overlay.json");
        fs::write(
            &state_file,
            include_str!("../../../fixtures/snapshots/sanitized-active-turn.json"),
        )
        .expect("write fixture");
        let mut tracker = LiveTracker::new(state_file, output_json.clone(), Duration::ZERO);

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
}
