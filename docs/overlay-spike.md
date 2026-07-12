# Overlay Spike

The shell decision is open. Electron and Tauri must be tested with two independent transparent overlay windows under KDE Plasma Wayland while Marvel Snap runs through Steam/Proton/XWayland.

## Launch Variants

Test each spike with:
- normal launch
- `--ozone-platform=x11`
- `XDG_SESSION_TYPE=x11`
- `--force-device-scale-factor=1`
- combined: `env XDG_SESSION_TYPE=x11 <app> --ozone-platform=x11 --force-device-scale-factor=1`

## Manual Matrix

Record for each shell:
- two separate transparent frameless windows
- always on top
- independent dragging
- independent resizing
- minimum dimensions
- persistent geometry
- placeholder 3x4 card grids
- hover tooltip
- clickable footer button
- passthrough toggle
- edit mode
- global passthrough hotkey
- global visibility hotkey
- no keyboard focus theft on hover
- multi-monitor positioning, including negative coordinates
- mixed-DPI behavior
- AppImage build result
- package size
- idle memory usage, if measurable

## Current Result

Build/package status in the current non-graphical environment:
- Electron source typecheck/build passed.
- Electron AppImage build passed; artifact measured locally at about 223 MiB.
- Tauri frontend typecheck/build passed.
- Tauri release binary build passed; binary measured locally at about 11 MiB.
- Tauri AppImage bundling reached `linuxdeploy` and failed with `failed to run linuxdeploy`; no more specific diagnostic was emitted by the CLI.

Not manually tested on KDE Wayland/XWayland. Do not infer graphical behavior from builds alone.
