# Testing

Required local checks:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
```

Frontend spike checks:

```sh
cd apps/electron-spike
npm install
npm run typecheck
npm run build

cd ../tauri-spike
npm install
npm run typecheck
npm run build
```

Core tests must not require network access. Overlay behavior, AppImage behavior, and XWayland behavior require manual verification when no graphical KDE Wayland session is available.

Snapshot normalizer tests use sanitized synthetic fixtures under `fixtures/snapshots/`. Do not commit raw user captures; reduce observations into minimal fixtures that preserve structure while removing player names, account IDs, game IDs, and other private identifiers.
