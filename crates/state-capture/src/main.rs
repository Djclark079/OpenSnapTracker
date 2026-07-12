use anyhow::{Context, Result, bail};
use clap::Parser;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use state_reader::{ReadOptions, read_json_snapshot};
use std::{
    collections::HashSet,
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
}
