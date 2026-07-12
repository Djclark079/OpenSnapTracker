# OpenSnapTracker

OpenSnapTracker is an early-stage Linux-native real-time overlay tracker for Marvel Snap. It will read local game state files produced by Marvel Snap under Steam/Proton and display two lightweight overlay windows: Player Deck and Opponent Deck.

This is not a web application and does not inject into or modify the game process.

## Current Status

The repository is a foundation and technology spike, not a finished tracker. It contains:
- Rust domain, snapshot-reader, capture, storage, metadata, and image-cache foundations.
- Electron and Tauri overlay spike scaffolds for comparing Linux overlay behavior.
- Documentation and manual KDE Wayland/XWayland test checklists.

The final desktop shell is intentionally undecided until the spike results are collected.

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
  -> Overlay Views
```

The client works offline after metadata and art are cached. The public client never contains the private oanor API key.

## Prerequisites

- Rust toolchain with `cargo`, `rustfmt`, and `clippy`.
- Node.js and npm for overlay spikes.
- Linux packages required by Electron/Tauri may vary by distribution.

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
- Whether Electron or Tauri is more reliable for two floating overlay windows.
- Which XWayland launch flags are truly required.
- How transformed, merged, generated, transferred, and stolen cards appear in state files.
- Whether direct automated fetching from upstream image URLs is permitted for distribution.

See [PROJECT.md](PROJECT.md), [PLANS.md](PLANS.md), and [docs/architecture.md](docs/architecture.md).
