CREATE TABLE card_definitions (
    card_key TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    base_cost INTEGER NOT NULL,
    base_power INTEGER NOT NULL,
    canonical_ability_text TEXT NOT NULL,
    collectable_state TEXT NOT NULL,
    release_state TEXT NOT NULL,
    series TEXT,
    image_url TEXT,
    metadata_revision INTEGER NOT NULL,
    unknown_json TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE catalogue_revision (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    current_revision INTEGER NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE image_cache (
    image_url TEXT PRIMARY KEY,
    local_path TEXT NOT NULL,
    content_type TEXT,
    content_length INTEGER,
    last_modified TEXT,
    downloaded_at TEXT,
    last_accessed_at TEXT,
    download_status TEXT NOT NULL,
    failure_count INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE app_settings (
    key TEXT PRIMARY KEY,
    value_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE overlay_geometry (
    overlay_id TEXT PRIMARY KEY,
    x INTEGER NOT NULL,
    y INTEGER NOT NULL,
    width INTEGER NOT NULL,
    height INTEGER NOT NULL,
    monitor_name TEXT,
    scale_factor REAL,
    updated_at TEXT NOT NULL
);

CREATE TABLE diagnostic_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    captured_at TEXT NOT NULL,
    source_filename TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    raw_json TEXT,
    parse_status TEXT NOT NULL
);

CREATE TABLE match_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    match_id TEXT,
    event_index INTEGER NOT NULL,
    event_type TEXT NOT NULL,
    event_json TEXT NOT NULL,
    snapshot_hash TEXT,
    observed_at TEXT NOT NULL
);

CREATE INDEX idx_match_events_match_id ON match_events(match_id, event_index);
