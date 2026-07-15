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

The sidecar reads `PlayState.json` and `CollectionState.json` for selected-deck identity, tails `Player.log` for real-time visible card movement and hidden-zone clues, and re-reads `GameState.json` whenever its hash changes. Snapshot payloads own richer hidden-zone buckets such as destroyed, discarded, removed/banished, and supplemental cards once a JSON write exposes them; later `Player.log` ticks must preserve those snapshot-derived buckets instead of clearing them. The sidecar atomically writes the same text-only overlay payload used by the Electron spike.

Current live parser status:

- Clear live signals found: `DrawCard`, `StageCard`, `ResolveCardPlayed`, `ThisCardDestroyedTrigger`, playable/unplayable hand highlights, selected location VFX, stage responses, and leave-game markers.
- `ThisCardDiscardedTrigger` is available in `Player.log` and is used for player discard-zone tracking. Returning discard cards need card-specific handling: Apocalypse and Scorn currently stay in hand immediately, while Khonshu forms clear the prior discarded form when the next form is observed in hand.
- Opponent discard identity is not always present in `Player.log`. The sidecar records opponent discard clues such as Moon Knight audio, and classifies newly observed opponent graveyard cards from the next `GameState.json` write as discarded. Under the current KISS refresh experiment, resolution-window bounce loops replace one-off opponent-discard bounces.
- Broad `InHandOngoing` VFX events are not treated as hand observations because normal cards like Lady Sif emit them without entering hand. Only known return-to-hand forms are accepted through that path.
- Player deck draw/play updates are driven by `Player.log` so they do not wait for `GameState.json` saves.
- Deck and hand count inference from `Player.log` remains in the code as an experiment, but is disabled while testing JSON-authoritative refreshes. In sampled logs, `ApplyGameWaitingForEndTurnChange(GameWaitingForEndTurnChange)` marks the transition from waiting into card-resolution playback. The Electron spike now starts a bounded 500ms focus-bounce loop during that resolution window, stops it at `StartTurnRequest`, detected match end, or a 30 second safety cap, and runs one final 250ms delayed bounce for hand/deck counts.
- `Player.log` created-card handling now treats non-deck local hand highlight events as supplemental cards. When a focus bounce or normal save produces a fresh `GameState.json`, snapshot reconciliation can also refresh supplemental cards from the richer state file. This covers visible cases like generated or transferred cards entering the hand, but it remains a conservative heuristic until more fixtures exist.
- The Peak hand-swap capture showed `LocationVfxDefs/ThePeak.asset` followed by a non-deck local hand highlight for the incoming card. The current parser marks the earliest tracked local hand card as `away` in that bounded context and does not classify it as Removed.
- Supplemental overlay brightness is availability-oriented: known opponent supplemental cards in hand stay bright until played, while player supplemental hand/play cards are dimmed because the player can already see them in-game. Opponent supplemental cards auto-open in the Electron spike when newly detected.

Experimental hidden-zone refresh:

```sh
env OST_FOCUS_BOUNCE=1 OST_LIVE_STATE_DIR="$HOME/.steam/steam/steamapps/compatdata/1997040/pfx/drive_c/users/steamuser/AppData/LocalLow/Second Dinner/SNAP/Standalone/States/nvprod" npm run start:live
```

When enabled, the Electron spike listens for sidecar hints that hidden zones may have changed, briefly focuses an overlay window, then attempts to restore focus to the `SNAP` X11/XWayland window with the repo-owned Rust `x11-focus-helper`. This is a diagnostic spike to test whether focus transitions force `GameState.json` writes. It must not be treated as production behavior until manually verified during live play.

The helper can be inspected directly:

```sh
cargo run -q -p x11-focus-helper -- list
cargo run -q -p x11-focus-helper -- find --title SNAP
```

File activity probe:

```sh
scripts/snap-file-activity-probe.sh "$HOME/.steam/steam/steamapps/compatdata/1997040/pfx/drive_c/users/steamuser/AppData/LocalLow/Second Dinner/SNAP"
```

Run the probe during a match to list files whose mtime, size, or content hash changes. This is intended to discover whether another local file contains real-time hidden-zone updates before considering process or network inspection.

Latest probe notes: `Player.log` and `ErrorLog.txt` update continuously during active play, while `GameState.json` still appears to update only intermittently. Sentry and Braze files update often but have not shown useful card-state payloads. No file observed by the probe has replaced `Player.log` as a complete real-time source; `GameState.json` is still needed for hidden-zone identities when the log only gives a clue.

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
- `GameState.json` can contain stale completed-match data outside an active match and may update sparsely during focused gameplay. Live overlay timing should come from `Player.log`; whenever `GameState.json` does change, the sidecar now runs the normal snapshot reconciliation path again so destroyed, discarded, removed/banished, and hidden-zone identity buckets can be refreshed from the richer snapshot.
- Conquest captures expose both regular match result data and battle/conquest result structures.

Do not commit raw captures. Redact and review first. Sanitized inspection reports are safer than raw captures, but still review them before sharing.
