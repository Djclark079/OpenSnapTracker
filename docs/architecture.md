# Architecture

OpenSnapTracker is Rust-first. The shell may be Electron or Tauri, but state reading, normalization, reconciliation, metadata, storage, image caching, and diagnostics should stay in Rust where practical.

## Pipeline

```text
State files -> Snapshot Reader -> Normalizer -> Reconciliation -> Events -> Match State -> Overlay Views
```

The snapshot reader treats files as externally owned and possibly mid-write. It retries parse failures with bounded backoff and never repairs malformed JSON.

## Domain Principles

- Original deck slots stay stable.
- Supplemental cards never enter the original 12-card grid.
- Destroyed, discarded, transformed, merged, returned, confirmed removed, and unknown transitions remain distinct internally.
- Opponent cards can be unknown, known, or inferred.
- Unknown snapshot fields are preserved where practical for diagnostics.

## Storage

SQLite is used for card definitions, catalogue revision, image cache metadata, settings, overlay geometry, optional diagnostic snapshots, and future match events. Migrations are required from the beginning.

Image files are stored on disk, not as SQLite blobs.

## Privacy

The application reads local game state files only, sends no match data by default, has no telemetry, and requires no account. Raw captures can contain identifiers and must not be committed without redaction.
