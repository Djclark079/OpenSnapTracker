# Overlay Spike

The shell decision is complete for v1: OpenSnapTracker will use Electron for the desktop shell and Rust for the core tracker engine.

Tauri is retained in the repository only as comparison evidence. It should not receive further work unless a future milestone explicitly reopens the shell decision.

## Decision

Electron is selected for v1 because it successfully demonstrated the required KDE Plasma Wayland/XWayland overlay behavior:
- two separate transparent frameless windows
- always-on-top over Marvel Snap
- independent dragging and resizing
- minimum dimensions
- geometry reset and sane relaunch behavior
- hover without keyboard focus theft
- click-through/passthrough into the game
- global hotkeys for passthrough, edit, visibility, and reset
- second-monitor movement
- AppImage packaging

Tauri is rejected for v1 because it required WebKitGTK/X11 workarounds, had transparent-window tooltip repaint artifacts, needed more custom shortcut/window plumbing, and still carried AppImage bundling risk.

## Launch Variants Tested

Variants considered:
- normal launch
- `--ozone-platform=x11`
- `XDG_SESSION_TYPE=x11`
- `--force-device-scale-factor=1`
- combined: `env XDG_SESSION_TYPE=x11 <app> --ozone-platform=x11 --force-device-scale-factor=1`
- `--disable-gpu`
- Tauri `GDK_BACKEND=x11`
- Tauri `WEBKIT_DISABLE_DMABUF_RENDERER=1`

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

## Electron Result

Build/package status in the current non-graphical environment:
- Electron source typecheck/build passed.
- Electron AppImage build passed; artifact measured locally at about 223 MiB.

KDE/XWayland manual findings:
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

Recommended Electron launch baseline for continued spike/dev testing:

```fish
cd /home/dan/Documents/opensnaptracker/apps/electron-spike
set -e ELECTRON_RUN_AS_NODE
npm run start:kde-x11
```

## Tauri Result

Build/package status:
- Tauri frontend typecheck/build passed.
- Tauri release binary build passed; binary measured locally at about 11 MiB.
- Tauri AppImage bundling reached `linuxdeploy` and failed with `failed to run linuxdeploy`; no more specific diagnostic was emitted by the CLI.

Manual findings:
- `npm run tauri:dev` and `env XDG_SESSION_TYPE=x11 npm run tauri:dev` both built and started Vite, then the native Tauri process exited with `Gdk-Message: Error flushing display: Protocol error` before any overlay behavior could be tested.
- A Tauri debug pass now includes `tauri:dev-x11` and `tauri:dev-plain` to distinguish overlay-window flag failures from a more general GTK/WebKit display failure.
- With `GDK_BACKEND=x11` and `WEBKIT_DISABLE_DMABUF_RENDERER=1`, both Tauri plain and transparent launches run.
- Plain Tauri windows are movable/resizable through normal decorations; transparent frameless windows render but were not movable/resizable before adding explicit controls.
- Initial Tauri hotkeys did not work because global shortcut handling was not implemented in the spike.
- After adding explicit controls, transparent Tauri windows moved and resized, but global shortcuts initially acted on both key press and key release, causing toggles to revert until filtered to `Pressed` events only.
- Tauri showed WebKit repaint artifacts with a reusable hidden tooltip element, so the spike now creates/removes tooltip nodes per hover.
- Tauri hide/show could restore frameless windows in the wrong place, so visibility toggling now captures bounds before hide and reapplies them before show.
- Tauri tooltip rendering still showed a ghost artifact after node removal, so the spike now positions tooltips at the mouse pointer and clamps them inside the overlay.
- The tooltip ghost remained at the last rendered tooltip position, consistent with a WebKitGTK transparent-window repaint issue; the spike now blanks the tooltip in place, forces layout, then removes it on the next animation frame.
- A faint tooltip ghost still remained, so the Tauri spike now uses native browser `title` tooltips instead of drawing custom tooltip DOM inside the transparent webview.
- Native browser `title` tooltips did not appear in the Tauri transparent-window test.

Conclusion: Tauri can be pushed toward parity, but doing so creates avoidable Linux WebKit/GTK overlay risk. It is not selected for v1.
