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
- Stops cleanly on SIGINT.

Do not commit raw captures. Redact and review first.
