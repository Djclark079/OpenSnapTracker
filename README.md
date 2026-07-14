# OpenSnapTracker

OpenSnapTracker is an early-stage Linux-native real-time overlay tracker for Marvel Snap. It will read local game state files produced by Marvel Snap under Steam/Proton and display two lightweight overlay windows: Player Deck and Opponent Deck.

This is not a web application and does not inject into or modify the game process.

## Current Status

The repository is a foundation and technology spike, not a finished tracker. It contains:
- Rust domain, snapshot-reader, capture, storage, metadata, and image-cache foundations.
- An Electron overlay spike selected as the v1 desktop shell baseline.
- A Tauri overlay spike retained as a rejected-for-v1 comparison.
- Documentation and manual KDE Wayland/XWayland test checklists.

Electron is selected for the v1 desktop shell based on KDE Wayland/XWayland spike results. Rust remains the preferred implementation language for the core tracker engine.

## Target

- Linux desktop, initially KDE Plasma Wayland.
- Marvel Snap through Steam/Proton.
- Overlay through XWayland/X11 when required.
- AppImage as the primary distribution target.

## Architecture

Expected pipeline:

```text
Marvel Snap state files
  -> Snapshot Reader
  -> Snapshot Normalizer
  -> State Reconciliation Engine
  -> Structured Events
  -> Current Match State
  -> Electron Overlay Views
```

The client works offline after metadata and art are cached. The public client never contains the private oanor API key.

## Prerequisites

- Rust toolchain with `cargo`, `rustfmt`, and `clippy`.
- Node.js and npm for the Electron shell and overlay spike.
- Linux packages required by Electron/AppImage may vary by distribution.

## Commands

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
```

Capture snapshots:

```sh
cargo run -p state-capture -- \
  --state-dir "$HOME/.steam/steam/steamapps/compatdata/1997040/pfx/drive_c/users/steamuser/AppData/LocalLow/Second Dinner/SNAP/Standalone/States/nvprod" \
  --output-dir captures \
  --interval-ms 1000
```

## Known Unknowns

- Exact Marvel Snap state semantics for card instances and transitions.
- Exact production XWayland launch flags and AppImage desktop entry details.
- How transformed, merged, generated, transferred, and stolen cards appear in state files.
- Whether direct automated fetching from upstream image URLs is permitted for distribution.

See [PROJECT.md](PROJECT.md), [PLANS.md](PLANS.md), and [docs/architecture.md](docs/architecture.md).
