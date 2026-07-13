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
