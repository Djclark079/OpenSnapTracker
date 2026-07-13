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

Initial KDE/XWayland manual findings:
- Electron normal launch opened two taskbar entries but no visible overlay content.
- Electron with `XDG_SESSION_TYPE=x11`, `--ozone-platform=x11`, and `--disable-gpu` rendered one visible opponent overlay.
- `--force-device-scale-factor=1` did not change that result in the first test.
- Hover tooltips, passthrough toggle, and visibility toggle worked in the visible Electron overlay.
- Edit mode changed the UI state, but move/resize did not work with the original spike flags.
- After the debug pass, both Electron windows rendered under KDE/XWayland, passthrough worked, reset geometry worked, and edit mode allowed move/resize.
- The first debug pass exposed too-small default/minimum geometry, causing footer/card clipping.
- After increasing default/minimum geometry, both Electron windows sized correctly and no longer clipped the deck grid or footer.
- Electron overlays stayed always-on-top over Marvel Snap.
- Passthrough clicks landed on game buttons/cards.
- Hover did not steal keyboard focus from the game.
- Visibility toggle worked during gameplay.
- Moving windows to a second monitor, resetting, relaunching, and geometry persistence/reset behavior all worked sanely.

Tauri has not yet been manually tested on KDE Wayland/XWayland. Do not infer Tauri graphical behavior from builds alone.
