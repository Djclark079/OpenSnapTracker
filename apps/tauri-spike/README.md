# Tauri Overlay Spike

Run frontend-only build checks:

```sh
npm install
npm run typecheck
npm run build
```

Run the Tauri app:

```sh
npm run tauri:dev
```

Try X11/XWayland launch:

```sh
npm run tauri:dev-x11
```

Try plain decorated windows if the overlay launch fails:

```sh
npm run tauri:dev-plain
```

Package attempt:

```sh
npm run tauri:build
```

This spike still needs manual verification for startup stability, transparency, passthrough, and global hotkeys. Record results in `../../docs/overlay-spike.md`.

Hotkeys:
- `Ctrl+Shift+P`: toggle passthrough.
- `Ctrl+Shift+E`: toggle edit mode.
- `Ctrl+Shift+H`: toggle visibility.
- `Ctrl+Shift+R`: reset both windows.
