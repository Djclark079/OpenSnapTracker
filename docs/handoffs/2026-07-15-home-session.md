# Session handoff

## Work completed
- Electron is the selected v1 shell; Tauri remains documented comparison evidence.
- Live Electron overlay now uses a Rust sidecar plus text-only payloads for player and opponent deck panels, counters, zone views, and supplemental-card drawers.
- Player deck title and full 12-card list are seeded from `PlayState.json` selected deck ids joined to `CollectionState.json`.
- `Player.log` parser handles live visible movement signals, match reset signals, opponent discard clues, The Peak away-state heuristic, self-returning discard cards, supplemental-card detection, and opponent supplemental dimming after play.
- `GameState.json` is again treated as authoritative for hidden zones and deck/hand counts when focus-bounce refreshes force Snap to write it.
- KISS refresh mode is active: resolution start begins a 500ms focus-bounce loop, turn start and match end stop it and schedule a final 250ms refresh, and the loop has a 30 second safety cap.
- Older clue-specific bounce requests are intentionally disabled but left in code for comparison.
- Added repo-owned `x11-focus-helper` tooling and `scripts/snap-file-activity-probe.sh`.
- Updated docs and `PLANS.md` to reflect the KISS refresh decision and disabled counter-inference experiment.

## Current branch
- Branch: `main`
- Remote: `origin` -> `https://github.com/Djclark079/OpenSnapTracker.git`
- Raw local captures and reference zip archives are present locally but ignored and must not be committed.

## Commands/tests run
- `cargo fmt --check`
- `cargo test -p tracker-sidecar`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo build --workspace`
- `cd apps/electron-spike && npm run typecheck`
- `cd apps/electron-spike && npm run build`

## Important decisions
- Electron remains the v1 desktop shell based on KDE Wayland/XWayland testing.
- The live tracker should avoid bespoke per-card/per-trigger focus bounces where possible.
- Current preferred live experiment is KISS refresh mode:
  - start loop on `ApplyGameWaitingForEndTurnChange(GameWaitingForEndTurnChange)`
  - bounce/read every 500ms during resolution
  - stop on `StartTurnRequest`, detected match end, or 30 second cap
  - run final 250ms delayed bounce on turn start or match end
- `GameState.json` is authoritative for hidden-zone identities and deck/hand counts after a forced write.
- `Player.log` remains useful for visible card movement, match/turn timing, and clues that help reconcile the next JSON snapshot.
- Disabled live counter inference and older clue-specific bounce code should stay in place until KISS mode has more live evidence.

## In-progress work
- KISS refresh mode needs more live validation, especially long resolution turns, Conquest individual-game ends, and overall Conquest match ends.
- Supplemental-card display is functional, but created-card source/origin semantics still need broader fixtures.
- Removed/transform/merge semantics still need targeted fixture validation.
- Card art and metadata are intentionally deferred; current overlay can continue using text-only cards.

## Known failures
- No current build/test failures.
- Two tracker-sidecar tests are intentionally ignored while live counter inference is disabled:
  - `live_counters_track_ordinary_opponent_turn_draws_and_plays`
  - `live_counters_track_player_draw_play_and_returning_discard`
- Graphical behavior must still be manually verified on the target KDE Wayland/XWayland setup after each overlay behavior change.

## Recommended next task
- On the next machine, pull `main`, install dependencies, and run:

```fish
cd apps/electron-spike
env OST_FOCUS_BOUNCE=1 OST_LIVE_STATE_DIR="$HOME/.steam/steam/steamapps/compatdata/1997040/pfx/drive_c/users/steamuser/AppData/LocalLow/Second Dinner/SNAP/Standalone/States/nvprod" npm run start:live
```

- Play a Conquest run and watch the terminal for:

```text
focus-bounce-requested { reason: 'resolution-started' }
resolution bounce loop started
focus bounce completed { reason: 'resolution-loop', ... }
resolution bounce loop stopped { reason: 'turn-start' }
focus bounce completed { reason: 'turn-start', ... }
```

- Verify destroy/discard/removed/supplemental updates during long resolution turns, Apocalypse/Scorn returning discards, Bucky/Winter Soldier-style generated cards, and both individual Conquest game end plus overall Conquest match end.
