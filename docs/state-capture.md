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
- Stops cleanly on SIGINT.

Inspection findings so far:
- Marvel Snap state uses JSON.NET-style `$id` and `$ref` object references. Normalizers must resolve references before interpreting player, zone, and card relationships.
- `GameState.json` can contain stale completed-match data outside an active match. Lifecycle detection must use state fields, not file existence alone.
- Conquest captures expose both regular match result data and battle/conquest result structures.

Do not commit raw captures. Redact and review first. Sanitized inspection reports are safer than raw captures, but still review them before sharing.
