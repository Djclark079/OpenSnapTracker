use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::Serialize;
use std::collections::HashSet;
use x11rb::{
    connection::Connection,
    protocol::{
        Event,
        xproto::{
            Atom, AtomEnum, ChangeWindowAttributesAux, ClientMessageData, ClientMessageEvent,
            ConnectionExt, EventMask, GetPropertyReply, InputFocus, Window,
        },
    },
    rust_connection::RustConnection,
};

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "X11/XWayland window discovery and focus helper for OpenSnapTracker"
)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    List,
    Find {
        #[arg(long, default_value = "SNAP")]
        title: String,
    },
    Activate {
        #[arg(long)]
        window: Option<String>,
        #[arg(long, default_value = "SNAP")]
        title: String,
    },
}

#[derive(Clone, Debug)]
struct Atoms {
    net_active_window: Atom,
    net_client_list: Atom,
    net_client_list_stacking: Atom,
    net_wm_name: Atom,
    utf8_string: Atom,
    wm_class: Atom,
    wm_name: Atom,
}

#[derive(Clone, Debug, Serialize)]
struct WindowInfo {
    id: String,
    id_decimal: u32,
    title: Option<String>,
    class: Option<String>,
    instance: Option<String>,
    mapped: bool,
}

#[derive(Debug, Serialize)]
struct FindResult {
    window: Option<WindowInfo>,
    candidates: Vec<WindowInfo>,
}

#[derive(Debug, Serialize)]
struct ActivateResult {
    activated: bool,
    window: Option<WindowInfo>,
    candidates: Vec<WindowInfo>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let (conn, screen_num) = x11rb::connect(None).context("connect to X11 display")?;
    let screen = &conn.setup().roots[screen_num];
    let atoms = Atoms::intern(&conn)?;

    match args.command {
        Command::List => {
            print_json(&list_windows(&conn, screen.root, &atoms)?)?;
        }
        Command::Find { title } => {
            let candidates = list_windows(&conn, screen.root, &atoms)?;
            let window = find_window(&candidates, &title).cloned();
            print_json(&FindResult { window, candidates })?;
        }
        Command::Activate { window, title } => {
            let candidates = list_windows(&conn, screen.root, &atoms)?;
            let selected = match window {
                Some(id) => {
                    let id = parse_window_id(&id)?;
                    candidates
                        .iter()
                        .find(|candidate| candidate.id_decimal == id)
                }
                None => find_window(&candidates, &title),
            }
            .cloned();

            if let Some(selected) = &selected {
                activate_window(&conn, screen.root, selected.id_decimal, &atoms)?;
            }
            print_json(&ActivateResult {
                activated: selected.is_some(),
                window: selected,
                candidates,
            })?;
        }
    }

    Ok(())
}

impl Atoms {
    fn intern(conn: &RustConnection) -> Result<Self> {
        Ok(Self {
            net_active_window: intern(conn, b"_NET_ACTIVE_WINDOW")?,
            net_client_list: intern(conn, b"_NET_CLIENT_LIST")?,
            net_client_list_stacking: intern(conn, b"_NET_CLIENT_LIST_STACKING")?,
            net_wm_name: intern(conn, b"_NET_WM_NAME")?,
            utf8_string: intern(conn, b"UTF8_STRING")?,
            wm_class: AtomEnum::WM_CLASS.into(),
            wm_name: AtomEnum::WM_NAME.into(),
        })
    }
}

fn intern(conn: &RustConnection, name: &[u8]) -> Result<Atom> {
    Ok(conn.intern_atom(false, name)?.reply()?.atom)
}

fn list_windows(conn: &RustConnection, root: Window, atoms: &Atoms) -> Result<Vec<WindowInfo>> {
    let mut ids = read_window_list(conn, root, atoms.net_client_list_stacking)?;
    ids.extend(read_window_list(conn, root, atoms.net_client_list)?);
    ids.extend(conn.query_tree(root)?.reply()?.children);

    let mut seen = HashSet::new();
    let ids = ids
        .into_iter()
        .filter(|id| seen.insert(*id))
        .collect::<Vec<_>>();

    let mut windows = Vec::new();
    for id in ids {
        if let Some(info) = window_info(conn, id, atoms)? {
            windows.push(info);
        }
    }
    windows.sort_by_key(|window| (window.mapped, window.title.is_some(), window.id_decimal));
    Ok(windows)
}

fn read_window_list(conn: &RustConnection, root: Window, atom: Atom) -> Result<Vec<Window>> {
    let reply = conn
        .get_property(false, root, atom, AtomEnum::WINDOW, 0, u32::MAX)?
        .reply()?;
    Ok(reply.value32().map(Iterator::collect).unwrap_or_default())
}

fn window_info(conn: &RustConnection, id: Window, atoms: &Atoms) -> Result<Option<WindowInfo>> {
    let attributes = match conn.get_window_attributes(id)?.reply() {
        Ok(attributes) => attributes,
        Err(_) => return Ok(None),
    };
    let title =
        read_string_property(conn, id, atoms.net_wm_name, atoms.utf8_string)?.or_else(|| {
            read_string_property(conn, id, atoms.wm_name, AtomEnum::STRING.into())
                .ok()
                .flatten()
        });
    let (instance, class) = read_wm_class(conn, id, atoms.wm_class)?;
    Ok(Some(WindowInfo {
        id: format!("0x{id:08x}"),
        id_decimal: id,
        title,
        class,
        instance,
        mapped: attributes.map_state == x11rb::protocol::xproto::MapState::VIEWABLE,
    }))
}

fn read_string_property(
    conn: &RustConnection,
    window: Window,
    property: Atom,
    property_type: Atom,
) -> Result<Option<String>> {
    let reply = conn
        .get_property(false, window, property, property_type, 0, 4096)?
        .reply()?;
    string_from_property(&reply)
}

fn string_from_property(reply: &GetPropertyReply) -> Result<Option<String>> {
    if reply.value.is_empty() {
        return Ok(None);
    }
    let raw = reply
        .value
        .split(|byte| *byte == 0)
        .next()
        .unwrap_or_default();
    if raw.is_empty() {
        return Ok(None);
    }
    Ok(Some(String::from_utf8_lossy(raw).trim().to_string()))
}

fn read_wm_class(
    conn: &RustConnection,
    window: Window,
    wm_class: Atom,
) -> Result<(Option<String>, Option<String>)> {
    let reply = conn
        .get_property(false, window, wm_class, AtomEnum::STRING, 0, 4096)?
        .reply()?;
    let mut parts = reply
        .value
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).trim().to_string());
    Ok((parts.next(), parts.next()))
}

fn find_window<'a>(windows: &'a [WindowInfo], title: &str) -> Option<&'a WindowInfo> {
    let title_lower = title.to_lowercase();
    windows
        .iter()
        .rev()
        .find(|window| window.mapped && exact_match(window, &title_lower))
        .or_else(|| {
            windows
                .iter()
                .rev()
                .find(|window| window.mapped && contains_match(window, &title_lower))
        })
}

fn exact_match(window: &WindowInfo, title_lower: &str) -> bool {
    window
        .title
        .as_ref()
        .is_some_and(|title| title.to_lowercase() == title_lower)
        || window
            .class
            .as_ref()
            .is_some_and(|class| class.to_lowercase() == title_lower)
        || window
            .instance
            .as_ref()
            .is_some_and(|instance| instance.to_lowercase() == title_lower)
}

fn contains_match(window: &WindowInfo, title_lower: &str) -> bool {
    window
        .title
        .as_ref()
        .is_some_and(|title| title.to_lowercase().contains(title_lower))
        || window
            .class
            .as_ref()
            .is_some_and(|class| class.to_lowercase().contains(title_lower))
        || window
            .instance
            .as_ref()
            .is_some_and(|instance| instance.to_lowercase().contains(title_lower))
}

fn activate_window(
    conn: &RustConnection,
    root: Window,
    window: Window,
    atoms: &Atoms,
) -> Result<()> {
    let data = ClientMessageData::from([1, 0, 0, 0, 0]);
    let event = ClientMessageEvent::new(32, window, atoms.net_active_window, data);
    conn.send_event(
        false,
        root,
        EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
        event,
    )?;
    conn.configure_window(
        window,
        &x11rb::protocol::xproto::ConfigureWindowAux::new()
            .stack_mode(x11rb::protocol::xproto::StackMode::ABOVE),
    )?;
    conn.set_input_focus(InputFocus::PARENT, window, x11rb::CURRENT_TIME)?;
    conn.change_window_attributes(
        window,
        &ChangeWindowAttributesAux::new().event_mask(EventMask::FOCUS_CHANGE),
    )
    .ok();
    conn.flush()?;

    let _ = conn
        .poll_for_event()
        .map(|event| matches!(event, Some(Event::FocusIn(_))));
    Ok(())
}

fn parse_window_id(id: &str) -> Result<u32> {
    let trimmed = id.trim();
    if let Some(hex) = trimmed.strip_prefix("0x") {
        u32::from_str_radix(hex, 16).with_context(|| format!("parse window id {trimmed}"))
    } else {
        trimmed
            .parse::<u32>()
            .with_context(|| format!("parse window id {trimmed}"))
    }
}

fn print_json<T>(value: &T) -> Result<()>
where
    T: Serialize,
{
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
