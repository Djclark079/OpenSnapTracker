//! Robust read primitives for Marvel Snap state snapshots.
//!
//! This crate reads JSON snapshots as externally-owned files: they may be
//! replaced or rewritten while the game is running. It retries boundedly on
//! transient parse failures and never repairs malformed JSON.

use sha2::{Digest, Sha256};
use std::{fs, io, path::Path, thread, time::Duration};
use thiserror::Error;
use time::OffsetDateTime;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadOptions {
    pub max_attempts: usize,
    pub initial_backoff: Duration,
}

impl Default for ReadOptions {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            initial_backoff: Duration::from_millis(25),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RawSnapshot {
    pub source_filename: String,
    pub raw_text: String,
    pub parsed: serde_json::Value,
    pub sha256: String,
    pub byte_len: usize,
    pub attempts: usize,
    pub captured_at: OffsetDateTime,
}

#[derive(Debug, Error)]
pub enum SnapshotReadError {
    #[error("could not read {path}: {source}")]
    Io { path: String, source: io::Error },
    #[error("snapshot {path} was malformed after {attempts} attempts: {message}")]
    Malformed {
        path: String,
        attempts: usize,
        message: String,
    },
    #[error("read options must allow at least one attempt")]
    InvalidOptions,
}

pub fn read_json_snapshot(
    path: impl AsRef<Path>,
    options: ReadOptions,
) -> Result<RawSnapshot, SnapshotReadError> {
    if options.max_attempts == 0 {
        return Err(SnapshotReadError::InvalidOptions);
    }

    let path = path.as_ref();
    let mut backoff = options.initial_backoff;
    let mut last_parse_error = None;

    for attempt in 1..=options.max_attempts {
        let raw_text = fs::read_to_string(path).map_err(|source| SnapshotReadError::Io {
            path: path.display().to_string(),
            source,
        })?;

        match serde_json::from_str::<serde_json::Value>(&raw_text) {
            Ok(parsed) => {
                let sha256 = sha256_hex(raw_text.as_bytes());
                return Ok(RawSnapshot {
                    source_filename: path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("snapshot.json")
                        .to_string(),
                    byte_len: raw_text.len(),
                    raw_text,
                    parsed,
                    sha256,
                    attempts: attempt,
                    captured_at: OffsetDateTime::now_utc(),
                });
            }
            Err(error) => {
                last_parse_error = Some(error.to_string());
                if attempt < options.max_attempts {
                    thread::sleep(backoff);
                    backoff = backoff.saturating_mul(2);
                }
            }
        }
    }

    Err(SnapshotReadError::Malformed {
        path: path.display().to_string(),
        attempts: options.max_attempts,
        message: last_parse_error.unwrap_or_else(|| "unknown parse error".to_string()),
    })
}

#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, time::Duration};
    use tempfile::tempdir;

    #[test]
    fn reads_valid_json_and_preserves_unknown_fields() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("GameState.json");
        fs::write(&path, r#"{"RemoteGame":{"unexpected":true},"unknown":42}"#)
            .expect("write fixture");

        let snapshot = read_json_snapshot(
            &path,
            ReadOptions {
                max_attempts: 1,
                initial_backoff: Duration::ZERO,
            },
        )
        .expect("snapshot reads");

        assert_eq!(snapshot.source_filename, "GameState.json");
        assert_eq!(snapshot.parsed["RemoteGame"]["unexpected"], true);
        assert_eq!(snapshot.parsed["unknown"], 42);
        assert_eq!(snapshot.sha256, sha256_hex(snapshot.raw_text.as_bytes()));
    }

    #[test]
    fn malformed_json_is_not_repaired() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("GameState.json");
        fs::write(&path, r#"{"RemoteGame":"#).expect("write fixture");

        let err = read_json_snapshot(
            &path,
            ReadOptions {
                max_attempts: 2,
                initial_backoff: Duration::ZERO,
            },
        )
        .expect_err("malformed json should fail");

        assert!(matches!(
            err,
            SnapshotReadError::Malformed { attempts: 2, .. }
        ));
    }

    #[test]
    fn eventually_consistent_read_can_succeed() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("GameState.json");
        fs::write(&path, r#"{"RemoteGame":"#).expect("write partial");

        let writer_path = path.clone();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            fs::write(writer_path, r#"{"RemoteGame":{"GameState":{}}}"#).expect("write complete");
        });

        let snapshot = read_json_snapshot(
            &path,
            ReadOptions {
                max_attempts: 5,
                initial_backoff: Duration::from_millis(10),
            },
        )
        .expect("eventually reads");
        handle.join().expect("writer joins");

        assert_eq!(
            snapshot.parsed["RemoteGame"]["GameState"],
            serde_json::json!({})
        );
        assert!(snapshot.attempts > 1);
    }
}
