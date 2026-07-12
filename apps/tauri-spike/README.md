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
env XDG_SESSION_TYPE=x11 npm run tauri:dev -- -- --ozone-platform=x11 --force-device-scale-factor=1
```

Package attempt:

```sh
npm run tauri:build
```

This spike still needs manual verification for reliable passthrough and global hotkeys. Record results in `../../docs/overlay-spike.md`.
