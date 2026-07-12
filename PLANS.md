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

Progress:
- [x] Product reference captured in `PROJECT.md`.
- [x] Foundation plan approved.
- [x] Rust workspace scaffolded.
- [x] Core crates added.
- [x] Documentation scaffold added.
- [x] Electron spike builds and packages an AppImage in this environment.
- [x] Tauri spike builds frontend and release binary in this environment.
- [ ] Tauri AppImage bundling succeeds.
- [ ] Overlay spike behavior manually tested on KDE Wayland/XWayland.
- [ ] Shell recommendation made from prototype evidence.

## Milestone 2: Capture Real State Fixtures

Acceptance criteria:
- Controlled captures for menu, matchmaking, match start, draw, play, destroy, discard, generate, transfer, transform, merge, and match end.
- Captures redacted before commit.
- Fixture-backed normalizer tests describe observed schema without invented semantics.

## Milestone 3: Overlay Shell Decision

Acceptance criteria:
- Electron and Tauri spike matrix completed on KDE Wayland/XWayland.
- AppImage attempts recorded.
- Package size and idle memory measured when possible.
- Recommendation documented in `docs/overlay-spike.md`.

## Decision Log

- 2026-07-11: Use MIT license.
- 2026-07-11: Keep desktop shell undecided until overlay spikes are tested.
- 2026-07-11: Use the helper-for-marvel-snap state path only as a hint; do not inherit its JSON repair or polling architecture.
- 2026-07-11: Use Exiled Exchange 2 as Linux overlay/AppImage reference only; OpenSnapTracker needs two floating windows, not one game-attached full overlay.
- 2026-07-11: Electron AppImage packaging works in the current environment; Tauri release binary works but AppImage bundling currently fails inside `linuxdeploy`.
