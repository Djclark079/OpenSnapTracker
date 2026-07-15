# OpenSnapTracker Plans

## Milestone 1: Foundation

Status: in progress

Acceptance criteria:
- Rust workspace builds and tests.
- Core domain model distinguishes original deck, supplemental cards, and transition reasons.
- Snapshot reader retries boundedly and never repairs JSON.
- State-capture utility copies changed snapshots read-only and writes a manifest.
- SQLite migrations exist from the beginning.
- Metadata and image-cache contracts are documented and tested.
- Electron and Tauri spikes can be built or have precise blocker notes.
- Electron shell decision is recorded from KDE Wayland/XWayland evidence.

Progress:
- [x] Product reference captured in `PROJECT.md`.
- [x] Foundation plan approved.
- [x] Rust workspace scaffolded.
- [x] Core crates added.
- [x] Documentation scaffold added.
- [x] Electron spike builds and packages an AppImage in this environment.
- [x] Tauri spike builds frontend and release binary in this environment.
- [x] Overlay spike behavior manually tested on KDE Wayland/XWayland.
- [x] Shell recommendation made from prototype evidence.
- [x] Electron selected for v1 desktop shell.
- [x] Tauri rejected for v1 and retained as comparison evidence.

## Milestone 2: Capture Real State Fixtures

Acceptance criteria:
- Controlled captures for menu, matchmaking, match start, draw, play, destroy, discard, generate, transfer, transform, merge, and match end.
- Captures redacted before commit.
- Local capture inspector resolves JSON.NET references and emits sanitized structural reports.
- Fixture-backed normalizer tests describe observed schema without invented semantics.

Progress:
- [x] Captures collected locally for menu, matchmaking, match start, draw, play, destroy, discard, and match end.
- [x] Capture reader accepts BOM-prefixed Marvel Snap JSON.
- [x] Dev inspection command added for ignored local captures.
- [x] Sanitized fixtures derived from observed capture structure.
- [x] Snapshot observation normalizer implemented against fixture-backed schema observations.
- [x] First reconciliation pass maps observation diffs into conservative domain events.
- [x] Capture replay command emits conservative event counts and overlay projection summaries from ignored local captures.
- [x] Initial destroy/discard semantics validated with targeted fixtures.
- [x] Text-only overlay payload export added for replay-derived Electron integration.
- [x] Live tracker sidecar emits text-only overlay payloads from read-only `GameState.json`.
- [x] Electron spike can spawn the live sidecar and reload overlay payload updates.
- [x] Live sidecar enriches player deck title and 12-card list by joining `PlayState.json` selected-deck ids to `CollectionState.json` decks, with observed-card matching only as a fallback.
- [x] Live sidecar now treats `Player.log` as the primary real-time signal for card movement, while JSON state files seed the selected deck and initial payload only.
- [x] Supplemental cards moved out of the cramped footer and into a compact Electron drawer.
- [x] The Peak transfer capture inspected; parser now has a bounded away-state heuristic for a non-deck incoming hand highlight after The Peak.
- [x] Opponent supplemental cards can auto-open as actionable bright hand knowledge and dim after observed play.
- [x] KISS refresh mode added: `Player.log` detects resolution start, Electron runs a bounded focus-bounce loop during card resolution, and `GameState.json` remains authoritative for hidden zones and deck/hand counts.
- [ ] Removed/transform/merge semantics validated with targeted fixtures.
- [ ] `Player.log` created-card patterns validated with broader targeted live captures.

## Milestone 3: Overlay Shell Decision

Status: complete

Acceptance criteria:
- Electron and Tauri spike matrix completed far enough to make a v1 shell decision.
- AppImage attempts recorded.
- Package size measured where available; idle memory measurement deferred.
- Recommendation documented in `docs/overlay-spike.md`.

## Decision Log

- 2026-07-11: Use MIT license.
- 2026-07-11: Keep desktop shell undecided until overlay spikes are tested.
- 2026-07-11: Use the helper-for-marvel-snap state path only as a hint; do not inherit its JSON repair or polling architecture.
- 2026-07-11: Use Exiled Exchange 2 as Linux overlay/AppImage reference only; OpenSnapTracker needs two floating windows, not one game-attached full overlay.
- 2026-07-11: Electron AppImage packaging works in the current environment; Tauri release binary works but AppImage bundling currently fails inside `linuxdeploy`.
- 2026-07-13: Electron selected for v1 desktop shell. KDE Wayland/XWayland testing showed Electron working for two transparent windows, always-on-top over Marvel Snap, passthrough, hover without focus theft, visibility toggle, edit move/resize, second-monitor movement, reset, relaunch geometry, and AppImage packaging. Tauri is rejected for v1 because it required WebKitGTK/X11 workarounds, had transparent-window repaint artifacts, needed more custom shortcut/window plumbing, and still had AppImage bundling risk.
- 2026-07-14: Initial real Conquest captures show Marvel Snap state uses JSON.NET-style `$id`/`$ref` references. Parser normalization must resolve references before interpreting players, zones, card instances, and transitions. GameState file existence alone is not an active-match signal because stale completed-match state can remain present.
- 2026-07-14: The first normalizer layer is an observation model, not the final event engine. It preserves raw zone names and leaves raw `Graveyard` interpretation to reconciliation.
- 2026-07-14: First reconciliation pass emits conservative events from fixture-backed observations: match start, card instance observed, draw, play, reveal, generated, discard, destroy, and unknown transition. Only hand-to-raw-`Graveyard` is classified as discard and board-to-raw-`Graveyard` as destroy; other graveyard paths remain unknown.
- 2026-07-14: Electron integration should start with the replay-exported text-only overlay payload: fixed 12-slot panels, known-card labels, unknown placeholders, and separate supplemental/destroyed/discarded/removed/unknown-transition buckets. Card art and metadata remain separate follow-up work.
- 2026-07-14: First live tracker loop uses a Rust sidecar that reads `GameState.json`, skips unchanged hashes, reconciles snapshots in memory, and atomically writes the same text-only overlay payload Electron already renders. This is a dev bridge, not final packaged sidecar wiring.
- 2026-07-14: Player full deck/title should be seeded from `PlayState.json` selected deck ids (`SerializedSelectedDeckId`, with legacy `SelectedDeckId.Value` support) joined to `CollectionState.ServerState.Decks[].Id`. Observed-card matching is fallback only when selected-deck state is unavailable. Graveyard classification can use in-snapshot `_previousZone` as transition context when polling misses the exact move frame.
- 2026-07-14: `GameState.json` appears to save/update sparsely during focused gameplay under Proton; `Player.log` is a better live event source. The sidecar now treats `Player.log` as live truth for card movement, keeps `PlayState.json`/`CollectionState.json` for selected-deck identity, and only uses `GameState.json` to seed the initial overlay payload when needed.
- 2026-07-14: The Peak capture showed location reveal VFX followed by a non-deck local hand highlight for the incoming swapped card. The parser marks a bounded earliest-known-hand card as `away` with a magenta overlay state rather than classifying it as Removed.
- 2026-07-15: KISS refresh mode is the preferred live experiment. Instead of chasing every destroy/discard/banish/generated-card clue for individual bounces, `ApplyGameWaitingForEndTurnChange(GameWaitingForEndTurnChange)` starts a 500ms focus-bounce loop during the resolution window with a 30 second safety cap, `StartTurnRequest` stops it and schedules a final 250ms refresh, and match-end signals also stop it and schedule a final refresh. Older clue-specific bounce code is kept dormant for comparison.
