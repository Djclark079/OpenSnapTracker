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

Experimental focus-bounce state refresh:

```fish
env OST_FOCUS_BOUNCE=1 OST_LIVE_STATE_DIR="$HOME/.steam/steam/steamapps/compatdata/1997040/pfx/drive_c/users/steamuser/AppData/LocalLow/Second Dinner/SNAP/Standalone/States/nvprod" npm run start:live
```

This listens for the `Player.log` transition into card-resolution playback, briefly focuses the overlay every 500ms, then restores focus to the `SNAP` X11/XWayland window through the repo-owned Rust X11 helper. The loop stops at the next turn start, detected match end, or a 30 second safety cap; turn start and match end also schedule one final 250ms delayed refresh. Older one-off hidden-zone clue bounces are intentionally dormant while this KISS refresh mode is tested.

The focus bounce now uses the repo-owned Rust `x11-focus-helper` in development instead of requiring `xdotool`. To inspect what X11/XWayland windows the helper can see:

```fish
npm run focus:list
npm run focus:find
```

If KDE shows the game title differently, override the search title:

```fish
env OST_SNAP_WINDOW_TITLE="SNAP" OST_FOCUS_BOUNCE=1 OST_LIVE_STATE_DIR="$HOME/.steam/steam/steamapps/compatdata/1997040/pfx/drive_c/users/steamuser/AppData/LocalLow/Second Dinner/SNAP/Standalone/States/nvprod" npm run start:live
```

If the helper reports a numeric or hex window id, force that exact target:

```fish
env OST_SNAP_WINDOW_ID="0x12345678" OST_FOCUS_BOUNCE=1 OST_LIVE_STATE_DIR="$HOME/.steam/steam/steamapps/compatdata/1997040/pfx/drive_c/users/steamuser/AppData/LocalLow/Second Dinner/SNAP/Standalone/States/nvprod" npm run start:live
```

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
