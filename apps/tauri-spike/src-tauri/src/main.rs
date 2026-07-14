use std::sync::Mutex;
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, State, WebviewWindow,
    WebviewWindowBuilder, WebviewUrl,
};

const MIN_WIDTH: f64 = 288.0;
const MIN_HEIGHT: f64 = 560.0;
const DEFAULT_WIDTH: f64 = 310.0;
const DEFAULT_HEIGHT: f64 = 600.0;
const GAP: f64 = 28.0;
const WINDOW_LABELS: [&str; 2] = ["player", "opponent"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OverlayMode {
    Interactive,
    Passthrough,
    Edit,
}

impl OverlayMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Interactive => "interactive",
            Self::Passthrough => "passthrough",
            Self::Edit => "edit",
        }
    }
}

#[derive(Debug)]
struct OverlayState {
    mode: OverlayMode,
    visible: bool,
    geometry: Vec<SavedGeometry>,
}

#[derive(Clone, Copy, Debug)]
struct WindowSpec {
    label: &'static str,
    title: &'static str,
    x: f64,
    y: f64,
}

#[derive(Clone, Copy, Debug)]
struct SavedGeometry {
    label: &'static str,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(Mutex::new(OverlayState {
            mode: OverlayMode::Edit,
            visible: true,
            geometry: Vec::new(),
        }))
        .invoke_handler(tauri::generate_handler![
            set_mode,
            toggle_passthrough,
            toggle_edit,
            toggle_visibility,
            reset_geometry
        ])
        .setup(|app| {
            let plain_windows = std::env::var_os("OST_TAURI_PLAIN_WINDOWS").is_some();
            println!(
                "[tauri-spike] plain_windows={plain_windows} GDK_BACKEND={:?} XDG_SESSION_TYPE={:?}",
                std::env::var("GDK_BACKEND").ok(),
                std::env::var("XDG_SESSION_TYPE").ok()
            );

            let windows = default_window_specs(app.handle())?;

            for spec in windows {
                let window = build_window(app, spec, plain_windows)?;
                println!(
                    "[tauri-spike:{}] created outer={:?} inner={:?}",
                    spec.label,
                    window.outer_position().ok(),
                    window.inner_size().ok()
                );
            }

            apply_mode(app.handle(), OverlayMode::Edit)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run tauri spike");
}

#[tauri::command]
fn set_mode(
    app: AppHandle,
    state: State<'_, Mutex<OverlayState>>,
    mode: String,
) -> Result<(), String> {
    let mode = match mode.as_str() {
        "interactive" => OverlayMode::Interactive,
        "passthrough" => OverlayMode::Passthrough,
        "edit" => OverlayMode::Edit,
        other => return Err(format!("unknown mode: {other}")),
    };
    state.lock().map_err(|error| error.to_string())?.mode = mode;
    apply_mode(&app, mode).map_err(|error| error.to_string())
}

#[tauri::command]
fn toggle_passthrough(
    app: AppHandle,
    state: State<'_, Mutex<OverlayState>>,
) -> Result<(), String> {
    let mut state = state.lock().map_err(|error| error.to_string())?;
    state.mode = if state.mode == OverlayMode::Passthrough {
        OverlayMode::Interactive
    } else {
        OverlayMode::Passthrough
    };
    apply_mode(&app, state.mode).map_err(|error| error.to_string())
}

#[tauri::command]
fn toggle_edit(app: AppHandle, state: State<'_, Mutex<OverlayState>>) -> Result<(), String> {
    let mut state = state.lock().map_err(|error| error.to_string())?;
    state.mode = if state.mode == OverlayMode::Edit {
        OverlayMode::Interactive
    } else {
        OverlayMode::Edit
    };
    apply_mode(&app, state.mode).map_err(|error| error.to_string())
}

#[tauri::command]
fn toggle_visibility(
    app: AppHandle,
    state: State<'_, Mutex<OverlayState>>,
) -> Result<(), String> {
    let mut state = state.lock().map_err(|error| error.to_string())?;
    state.visible = !state.visible;

    for label in WINDOW_LABELS {
        if let Some(window) = app.get_webview_window(label) {
            if state.visible {
                if let Some(geometry) = state.geometry.iter().find(|geometry| geometry.label == label)
                {
                    restore_geometry(&window, *geometry)?;
                }
                window.show().map_err(|error| error.to_string())?;
            } else {
                if let Some(geometry) = capture_geometry(label, &window) {
                    if let Some(existing) = state
                        .geometry
                        .iter_mut()
                        .find(|geometry| geometry.label == label)
                    {
                        *existing = geometry;
                    } else {
                        state.geometry.push(geometry);
                    }
                }
                window.hide().map_err(|error| error.to_string())?;
            }
        }
    }

    Ok(())
}

#[tauri::command]
fn reset_geometry(app: AppHandle) -> Result<(), String> {
    let specs = default_window_specs(&app).map_err(|error| error.to_string())?;
    for spec in specs {
        if let Some(window) = app.get_webview_window(spec.label) {
            window
                .set_position(LogicalPosition { x: spec.x, y: spec.y })
                .map_err(|error| error.to_string())?;
            window
                .set_size(LogicalSize {
                    width: DEFAULT_WIDTH,
                    height: DEFAULT_HEIGHT,
                })
                .map_err(|error| error.to_string())?;
            window.show().map_err(|error| error.to_string())?;
        }
    }

    Ok(())
}

fn apply_mode(app: &AppHandle, mode: OverlayMode) -> tauri::Result<()> {
    for label in WINDOW_LABELS {
        if let Some(window) = app.get_webview_window(label) {
            window.set_resizable(mode == OverlayMode::Edit)?;
            window.set_ignore_cursor_events(mode == OverlayMode::Passthrough)?;
            window.emit("mode", mode.as_str())?;
            println!(
                "[tauri-spike:{label}] mode={} outer={:?} inner={:?}",
                mode.as_str(),
                window.outer_position().ok(),
                window.inner_size().ok()
            );
        }
    }

    Ok(())
}

fn build_window(
    app: &tauri::App,
    spec: WindowSpec,
    plain_windows: bool,
) -> tauri::Result<WebviewWindow> {
    let mut builder = WebviewWindowBuilder::new(
        app,
        spec.label,
        WebviewUrl::App(format!("index.html?id={}", spec.label).into()),
    )
    .title(spec.title)
    .inner_size(DEFAULT_WIDTH, DEFAULT_HEIGHT)
    .min_inner_size(MIN_WIDTH, MIN_HEIGHT)
    .position(spec.x, spec.y)
    .resizable(true)
    .focused(plain_windows)
    .visible(true);

    if plain_windows {
        builder = builder
            .transparent(false)
            .decorations(true)
            .always_on_top(false)
            .skip_taskbar(false);
    } else {
        builder = builder
            .transparent(true)
            .decorations(false)
            .always_on_top(true)
            .visible_on_all_workspaces(true)
            .skip_taskbar(false);
    }

    builder.build()
}

fn default_window_specs(app: &AppHandle) -> tauri::Result<[WindowSpec; 2]> {
    let primary = app
        .primary_monitor()?
        .map(|monitor| monitor.position().to_logical::<f64>(monitor.scale_factor()))
        .unwrap_or_else(|| tauri::LogicalPosition { x: 40.0, y: 80.0 });
    let start_x = primary.x + 80.0;
    let start_y = primary.y + 80.0;

    Ok([
        WindowSpec {
            label: "player",
            title: "OpenSnapTracker Player",
            x: start_x,
            y: start_y,
        },
        WindowSpec {
            label: "opponent",
            title: "OpenSnapTracker Opponent",
            x: start_x + DEFAULT_WIDTH + GAP,
            y: start_y,
        },
    ])
}

fn capture_geometry(label: &'static str, window: &WebviewWindow) -> Option<SavedGeometry> {
    let position = window.outer_position().ok()?;
    let size = window.inner_size().ok()?;
    Some(SavedGeometry {
        label,
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    })
}

fn restore_geometry(window: &WebviewWindow, geometry: SavedGeometry) -> Result<(), String> {
    window
        .set_position(tauri::PhysicalPosition {
            x: geometry.x,
            y: geometry.y,
        })
        .map_err(|error| error.to_string())?;
    window
        .set_size(tauri::PhysicalSize {
            width: geometry.width,
            height: geometry.height,
        })
        .map_err(|error| error.to_string())
}
