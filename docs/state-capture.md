# State Capture

The `state-capture` utility collects read-only snapshots from Marvel Snap state files so the project can build fixture-backed normalizers and transition tests.

Default path hint:

```text
~/.steam/steam/steamapps/compatdata/1997040/pfx/drive_c/users/steamuser/AppData/LocalLow/Second Dinner/SNAP/Standalone/States/nvprod
```

Run once:

```sh
cargo run -p state-capture -- --state-dir <state-dir> --output-dir captures --once
```

Run during test matches:

```sh
cargo run -p state-capture -- --state-dir <state-dir> --output-dir captures --interval-ms 1000
```

Inspect local captures without printing raw account or player identity fields:

```sh
cargo run -p state-capture -- --inspect-captures captures --inspect-card-limit 24
```

Replay local captures through observation, reconciliation, and overlay projection:

```sh
cargo run -p state-capture -- --replay-captures captures
```

Replay all scenario folders as one timestamp-ordered timeline:

```sh
cargo run -p state-capture -- --replay-captures captures --replay-chronological
```

Export the final replay projection as a text-only overlay payload:

```sh
cargo run -p state-capture -- --replay-captures captures --replay-chronological --export-overlay-json captures/_derived/overlay.json
```

Run the live read-only tracker sidecar:

```sh
cargo run -p tracker-sidecar -- --state-dir <state-dir> --output-json /tmp/opensnaptracker-live-overlay.json
```

The sidecar reads `GameState.json`, skips unchanged file hashes, reconciles against the previous observed snapshot, and atomically writes the same text-only overlay payload used by the Electron spike.

Optional redaction uses dotted JSON paths:

```sh
cargo run -p state-capture -- --state-dir <state-dir> --redact AccountId,PlayerId
```

The tool:
- Reads only from game files.
- Captures `GameState.json`, `PlayState.json`, and `CollectionState.json` by default.
- Avoids duplicate content by SHA-256 hash.
- Writes redacted JSON snapshots and `manifest.ndjson`.
- Records timestamp, source filename, hash, parse status, and a generic game-state fingerprint when `/RemoteGame/GameState` exists.
- Produces a sanitized inspection report for local captures, including turn state, player zone counts, known/hidden card counts, card zone summaries, JSON.NET `$id`/`$ref` resolution, and per-scenario transition hints.
- Produces a sanitized replay report with conservative event counts and overlay-oriented player/opponent buckets, including destroyed and discarded counts when reconciliation can classify them.
- Can export a sanitized text-only overlay payload for development UI rendering. The payload uses fixed 12-slot player/opponent panels, text labels for known cards, `?` placeholders for unknown slots, and separate supplemental/destroyed/discarded/removed/unknown-transition collections.
- Includes a live sidecar that emits derived overlay payload JSON from local game state without modifying game files.
- Stops cleanly on SIGINT.

Inspection findings so far:
- Marvel Snap state uses JSON.NET-style `$id` and `$ref` object references. Normalizers must resolve references before interpreting player, zone, and card relationships.
- `GameState.json` can contain stale completed-match data outside an active match. Lifecycle detection must use state fields, not file existence alone.
- Conquest captures expose both regular match result data and battle/conquest result structures.

Do not commit raw captures. Redact and review first. Sanitized inspection reports are safer than raw captures, but still review them before sharing.
