# Electron Overlay Spike

Run:

```sh
npm install
npm start
```

X11/XWayland variant:

```sh
npm run start:x11
```

KDE/XWayland debugging variant:

```sh
npm run start:kde-x11
```

This resets geometry and starts in edit mode so both windows can be placed immediately.

Run against a replay-exported text overlay payload:

```sh
npm run start:payload
```

That script expects `../../captures/_derived/overlay.json` relative to this app directory. Regenerate it from the repository root with:

```sh
cargo run -p state-capture -- --replay-captures captures --replay-chronological --export-overlay-json captures/_derived/overlay.json
```

You can also pass another payload path:

```sh
env OST_OVERLAY_PAYLOAD=/path/to/overlay.json npm run start:kde-x11
```

Run against live Marvel Snap state files with the Rust sidecar:

```sh
env OST_LIVE_STATE_DIR="$HOME/.steam/steam/steamapps/compatdata/1997040/pfx/drive_c/users/steamuser/AppData/LocalLow/Second Dinner/SNAP/Standalone/States/nvprod" npm run start:live
```

For fish:

```fish
env OST_LIVE_STATE_DIR="$HOME/.steam/steam/steamapps/compatdata/1997040/pfx/drive_c/users/steamuser/AppData/LocalLow/Second Dinner/SNAP/Standalone/States/nvprod" npm run start:live
```

The live mode starts `cargo run -p tracker-sidecar` in development, writes a derived overlay payload under Electron's user-data directory, and reloads both overlay windows whenever that payload changes.

Hotkeys:
- `Ctrl+Shift+P`: toggle passthrough.
- `Ctrl+Shift+E`: toggle edit mode.
- `Ctrl+Shift+H`: toggle visibility.
- `Ctrl+Shift+R`: reset both windows to the primary display.

Package attempt:

```sh
npm run package:appimage
```

Manual behavior must be recorded in `../../docs/overlay-spike.md`.
