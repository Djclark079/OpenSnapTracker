fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let player = tauri::WebviewWindowBuilder::new(
                app,
                "player",
                tauri::WebviewUrl::App("index.html?id=player".into()),
            )
            .title("OpenSnapTracker Player")
            .transparent(true)
            .decorations(false)
            .always_on_top(true)
            .inner_size(230.0, 460.0)
            .min_inner_size(180.0, 300.0)
            .resizable(false)
            .focused(false)
            .build()?;

            let opponent = tauri::WebviewWindowBuilder::new(
                app,
                "opponent",
                tauri::WebviewUrl::App("index.html?id=opponent".into()),
            )
            .title("OpenSnapTracker Opponent")
            .transparent(true)
            .decorations(false)
            .always_on_top(true)
            .inner_size(230.0, 460.0)
            .min_inner_size(180.0, 300.0)
            .position(1260.0, 120.0)
            .resizable(false)
            .focused(false)
            .build()?;

            let _ = player;
            let _ = opponent;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run tauri spike");
}
