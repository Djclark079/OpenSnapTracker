# Agent Notes

OpenSnapTracker is a Linux-native Marvel Snap overlay tracker. The target is KDE Plasma Wayland with Marvel Snap running through Steam/Proton in an XWayland/X11 game window, distributed primarily as an AppImage.

Keep these constraints intact:
- Read game state files only; never modify Marvel Snap files or inject into the game process.
- Do not commit raw user captures. They may contain private identifiers.
- Do not fabricate graphical, Wayland/XWayland, AppImage, or performance test results.
- Verify overlay behavior manually on KDE Wayland/XWayland when no graphical test environment is available.
- Never commit upstream API keys. The public client must not contain the private oanor key.
- Image licensing and direct automated access to upstream card art remain unresolved.
- Changes to core domain semantics require fixture-backed tests.

Build and test:
- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo build --workspace`
- Electron spike: `cd apps/electron-spike && npm install && npm run typecheck && npm run build`
- Tauri spike: `cd apps/tauri-spike && npm install && npm run typecheck && npm run build`

State capture:
- Default Proton path: `~/.steam/steam/steamapps/compatdata/1997040/pfx/drive_c/users/steamuser/AppData/LocalLow/Second Dinner/SNAP/Standalone/States/nvprod`
- Run once: `cargo run -p state-capture -- --state-dir <path> --output-dir captures --once`
- Run during matches: `cargo run -p state-capture -- --state-dir <path> --output-dir captures --interval-ms 1000`

Generated or third-party files:
- Do not edit `Cargo.lock` manually.
- Do not edit `package-lock.json` manually.
- Do not edit `apps/tauri-spike/src-tauri/gen/` generated schema files.
- Do not unpack or modify the supplied reference zip archives unless explicitly needed for inspection.
