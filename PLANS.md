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
- [ ] Reconciliation engine maps observation diffs into domain events.
- [ ] Destroy/discard/remove semantics validated with targeted fixtures.

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
- 2026-07-14: The first normalizer layer is an observation model, not the final event engine. It preserves raw zone names and maps `Graveyard` to `UnknownTransition` until destroy/discard semantics are proven by transition fixtures.
