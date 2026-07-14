import { app, BrowserWindow, globalShortcut, ipcMain, screen } from "electron";
import path from "node:path";
import fs from "node:fs";
import { spawn, type ChildProcess } from "node:child_process";

type OverlayId = "player" | "opponent";
type Mode = "interactive" | "passthrough" | "edit";
type Geometry = { x: number; y: number; width: number; height: number };
type GeometryStore = Record<OverlayId, Geometry>;
type ResizeEdge = "east" | "south" | "south-east" | "west" | "south-west";
type ResizeRequest = {
  id: OverlayId;
  screenX: number;
  screenY: number;
};
type ResizeStartRequest = {
  id: OverlayId;
  edge: ResizeEdge;
  startScreenX: number;
  startScreenY: number;
};
type ActiveResize = {
  edge: ResizeEdge;
  startBounds: Geometry;
  startScreenX: number;
  startScreenY: number;
};
type OverlayPayload = Record<string, unknown> & {
  player?: unknown;
  opponent?: unknown;
};

const minWidth = 288;
const minHeight = 560;
const defaultWidth = 310;
const defaultHeight = 600;
const overlayAspectRatio = defaultWidth / defaultHeight;
const maxVerticalReserve = 200;
const maxHorizontalReserve = 80;
const screenEdgeReserve = 20;

let mode: Mode = process.argv.includes("--start-edit") ? "edit" : "interactive";
const windows = new Map<OverlayId, BrowserWindow>();
const lastBounds = new Map<OverlayId, Geometry>();
const activeResizes = new Map<OverlayId, ActiveResize>();
let overlayPayload: OverlayPayload | null = null;
let sidecar: ChildProcess | null = null;
let overlayPayloadWatchPath: string | null = null;

if (process.platform === "linux") {
  app.disableHardwareAcceleration();
  app.commandLine.appendSwitch("ozone-platform", "x11");
  app.commandLine.appendSwitch("disable-gpu");
}

function geometryPath(): string {
  return path.join(app.getPath("userData"), "electron-spike-geometry.json");
}

function defaultGeometry(): GeometryStore {
  const primary = screen.getPrimaryDisplay().workArea;
  const width = defaultWidth;
  const height = defaultHeight;
  const gap = 28;
  const totalWidth = width * 2 + gap;
  const startX = primary.x + Math.max(20, Math.round((primary.width - totalWidth) / 2));
  const y = primary.y + Math.max(20, Math.round((primary.height - height) / 2));

  return {
    player: { x: startX, y, width, height },
    opponent: { x: startX + width + gap, y, width, height }
  };
}

function readGeometry(defaults: GeometryStore): GeometryStore {
  if (process.argv.includes("--reset-geometry")) {
    return defaults;
  }

  try {
    const parsed = JSON.parse(fs.readFileSync(geometryPath(), "utf8")) as Partial<GeometryStore>;
    return {
      player: enforceMinimumGeometry({ ...defaults.player, ...parsed.player }),
      opponent: enforceMinimumGeometry({ ...defaults.opponent, ...parsed.opponent })
    };
  } catch {
    return defaults;
  }
}

function enforceMinimumGeometry(geometry: Geometry): Geometry {
  return {
    ...geometry,
    width: Math.max(minWidth, geometry.width),
    height: Math.max(minHeight, geometry.height)
  };
}

function maxSizeForGeometry(geometry: Geometry): Pick<Geometry, "width" | "height"> {
  const display = screen.getDisplayMatching(geometry);
  const availableBelow = display.workArea.y + display.workArea.height - geometry.y - screenEdgeReserve;
  const maxHeight = Math.max(
    minHeight,
    Math.min(display.workArea.height - maxVerticalReserve, availableBelow)
  );
  const maxWidth = Math.max(minWidth, display.workArea.width - maxHorizontalReserve);
  const heightFromWidth = Math.round(maxWidth / overlayAspectRatio);
  const height = Math.min(maxHeight, heightFromWidth);
  return {
    width: Math.round(height * overlayAspectRatio),
    height
  };
}

function clampGeometrySizeToDisplay(geometry: Geometry, anchorRight = false): Geometry {
  const maxSize = maxSizeForGeometry(geometry);
  const width = Math.min(Math.max(minWidth, geometry.width), maxSize.width);
  const height = Math.min(Math.max(minHeight, geometry.height), maxSize.height);
  return {
    ...geometry,
    x: anchorRight ? geometry.x + geometry.width - width : geometry.x,
    width,
    height
  };
}

function writeGeometry(): void {
  const data: Partial<GeometryStore> = {};
  for (const [id, win] of windows) {
    data[id] = win.getBounds();
  }
  fs.mkdirSync(path.dirname(geometryPath()), { recursive: true });
  fs.writeFileSync(geometryPath(), JSON.stringify(data, null, 2));
}

function resizeGeometry(request: ResizeRequest, activeResize: ActiveResize): Geometry {
  const startBounds = activeResize.startBounds;
  const dx = request.screenX - activeResize.startScreenX;
  const dy = request.screenY - activeResize.startScreenY;
  let width = startBounds.width;
  let height = startBounds.height;
  let x = startBounds.x;

  if (activeResize.edge === "east") {
    width = Math.max(minWidth, startBounds.width + dx);
    height = Math.max(minHeight, Math.round(width / overlayAspectRatio));
    width = Math.round(height * overlayAspectRatio);
  } else if (activeResize.edge === "west") {
    width = Math.max(minWidth, startBounds.width - dx);
    height = Math.max(minHeight, Math.round(width / overlayAspectRatio));
    width = Math.round(height * overlayAspectRatio);
    x = startBounds.x + startBounds.width - width;
  } else if (activeResize.edge === "south") {
    height = Math.max(minHeight, startBounds.height + dy);
    width = Math.max(minWidth, Math.round(height * overlayAspectRatio));
    height = Math.round(width / overlayAspectRatio);
  } else if (activeResize.edge === "south-east") {
    const widthScale = (startBounds.width + dx) / startBounds.width;
    const heightScale = (startBounds.height + dy) / startBounds.height;
    const scale = Math.max(
      minWidth / startBounds.width,
      minHeight / startBounds.height,
      widthScale,
      heightScale
    );
    width = Math.max(minWidth, Math.round(startBounds.width * scale));
    height = Math.max(minHeight, Math.round(width / overlayAspectRatio));
    width = Math.round(height * overlayAspectRatio);
  } else {
    const widthScale = (startBounds.width - dx) / startBounds.width;
    const heightScale = (startBounds.height + dy) / startBounds.height;
    const scale = Math.max(
      minWidth / startBounds.width,
      minHeight / startBounds.height,
      widthScale,
      heightScale
    );
    width = Math.max(minWidth, Math.round(startBounds.width * scale));
    height = Math.max(minHeight, Math.round(width / overlayAspectRatio));
    width = Math.round(height * overlayAspectRatio);
    x = startBounds.x + startBounds.width - width;
  }

  return clampGeometrySizeToDisplay({
    x,
    y: startBounds.y,
    width,
    height
  }, activeResize.edge === "west" || activeResize.edge === "south-west");
}

function startCustomResize(request: ResizeStartRequest): void {
  if (mode !== "edit") return;
  const win = windows.get(request.id);
  if (!win || win.isDestroyed()) return;

  activeResizes.set(request.id, {
    edge: request.edge,
    startBounds: win.getBounds(),
    startScreenX: request.startScreenX,
    startScreenY: request.startScreenY
  });
}

function applyCustomResize(request: ResizeRequest): void {
  if (mode !== "edit") return;
  const win = windows.get(request.id);
  if (!win || win.isDestroyed()) return;
  const activeResize = activeResizes.get(request.id);
  if (!activeResize) return;

  const bounds = resizeGeometry(request, activeResize);
  win.setBounds(bounds);
  win.webContents.send("debug-state", { id: request.id, mode, bounds });
}

function finishCustomResize(): void {
  for (const [id, win] of windows) {
    if (!win.isDestroyed()) {
      lastBounds.set(id, win.getBounds());
    }
  }
  activeResizes.clear();
  writeGeometry();
}

function resetGeometry(): void {
  const geometry = defaultGeometry();
  for (const [id, win] of windows) {
    win.setBounds(geometry[id]);
    lastBounds.set(id, geometry[id]);
    win.show();
    logBounds(id, win, "reset");
  }
  writeGeometry();
}

function clampToDisplays(geometry: Geometry): Geometry {
  const displays = screen.getAllDisplays();
  const union = displays.reduce(
    (acc, display) => ({
      minX: Math.min(acc.minX, display.bounds.x),
      minY: Math.min(acc.minY, display.bounds.y),
      maxX: Math.max(acc.maxX, display.bounds.x + display.bounds.width),
      maxY: Math.max(acc.maxY, display.bounds.y + display.bounds.height)
    }),
    { minX: 0, minY: 0, maxX: 1920, maxY: 1080 }
  );
  return {
    ...clampGeometrySizeToDisplay(enforceMinimumGeometry(geometry)),
    x: Math.min(Math.max(geometry.x, union.minX), union.maxX - 80),
    y: Math.min(Math.max(geometry.y, union.minY), union.maxY - 80)
  };
}

function logBounds(id: OverlayId, win: BrowserWindow, event: string): void {
  console.log(`[overlay:${id}] ${event}`, {
    bounds: win.getBounds(),
    visible: win.isVisible(),
    focused: win.isFocused(),
    mode
  });
}

function overlayPayloadPath(): string | undefined {
  const arg = process.argv.find((value) => value.startsWith("--overlay-payload="));
  const fromArg = arg?.slice("--overlay-payload=".length);
  return fromArg || process.env.OST_OVERLAY_PAYLOAD;
}

function liveStateDir(): string | undefined {
  const arg = process.argv.find((value) => value.startsWith("--live-state-dir="));
  const fromArg = arg?.slice("--live-state-dir=".length);
  return fromArg || process.env.OST_LIVE_STATE_DIR;
}

function defaultLivePayloadPath(): string {
  return path.join(app.getPath("userData"), "live-overlay.json");
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function readOverlayPayload(): OverlayPayload | null {
  const payloadPath = overlayPayloadWatchPath ?? overlayPayloadPath();
  if (!payloadPath) return null;
  if (!fs.existsSync(payloadPath)) return null;

  try {
    const parsed: unknown = JSON.parse(fs.readFileSync(payloadPath, "utf8"));
    if (!isRecord(parsed) || !isRecord(parsed.player) || !isRecord(parsed.opponent)) {
      console.warn("[overlay-spike] ignored overlay payload with unexpected shape", payloadPath);
      return null;
    }
    console.log("[overlay-spike] loaded overlay payload", payloadPath);
    return parsed as OverlayPayload;
  } catch (error) {
    console.warn("[overlay-spike] could not load overlay payload", payloadPath, error);
    return null;
  }
}

function payloadPanel(id: OverlayId): unknown {
  return overlayPayload?.[id] ?? null;
}

function broadcastOverlayPayload(): void {
  for (const [id, win] of windows) {
    if (win.isDestroyed()) continue;
    win.webContents.send("overlay-payload", {
      id,
      panel: payloadPanel(id),
      sourcePath: overlayPayloadWatchPath ?? overlayPayloadPath() ?? null
    });
  }
}

function reloadOverlayPayload(): void {
  const next = readOverlayPayload();
  if (!next) return;
  overlayPayload = next;
  broadcastOverlayPayload();
}

function startOverlayPayloadWatcher(): void {
  const payloadPath = overlayPayloadWatchPath ?? overlayPayloadPath();
  if (!payloadPath) return;
  overlayPayloadWatchPath = payloadPath;
  overlayPayload = readOverlayPayload();
  fs.watchFile(payloadPath, { interval: 250 }, (current, previous) => {
    if (current.mtimeMs !== previous.mtimeMs || current.size !== previous.size) {
      reloadOverlayPayload();
    }
  });
}

function startTrackerSidecar(): void {
  const stateDir = liveStateDir();
  if (!stateDir) return;
  const outputJson = overlayPayloadPath() ?? defaultLivePayloadPath();
  overlayPayloadWatchPath = outputJson;
  fs.rmSync(outputJson, { force: true });
  sidecar = spawn(
    "cargo",
    [
      "run",
      "-p",
      "tracker-sidecar",
      "--",
      "--state-dir",
      stateDir,
      "--output-json",
      outputJson,
      "--interval-ms",
      "250"
    ],
    {
      cwd: path.resolve(__dirname, "../../.."),
      stdio: "inherit"
    }
  );
  sidecar.on("exit", (code, signal) => {
    console.log("[overlay-spike] tracker sidecar exited", { code, signal });
    sidecar = null;
  });
}

function createOverlay(id: OverlayId, geometry: Geometry): BrowserWindow {
  const win = new BrowserWindow({
    ...clampToDisplays(geometry),
    minWidth,
    minHeight,
    frame: false,
    transparent: true,
    alwaysOnTop: true,
    resizable: false,
    skipTaskbar: false,
    focusable: true,
    hasShadow: false,
    title: `OpenSnapTracker ${id}`,
    backgroundColor: "#00000000",
    webPreferences: {
      nodeIntegration: true,
      contextIsolation: false,
      backgroundThrottling: false
    }
  });

  win.setAlwaysOnTop(true, "screen-saver");
  win.setAspectRatio(overlayAspectRatio);
  win.setVisibleOnAllWorkspaces(true, { visibleOnFullScreen: true });
  win.loadFile(path.join(__dirname, "../src/index.html"), { query: { id } });
  win.once("ready-to-show", () => {
    win.show();
    logBounds(id, win, "ready-to-show");
  });
  win.webContents.once("did-finish-load", () => {
    win.webContents.send("mode", mode);
    win.webContents.send("debug-state", { id, mode, bounds: win.getBounds() });
    win.webContents.send("overlay-payload", {
      id,
      panel: payloadPanel(id),
      sourcePath: overlayPayloadPath() ?? null
    });
  });
  win.on("moved", () => {
    lastBounds.set(id, win.getBounds());
    logBounds(id, win, "moved");
    writeGeometry();
  });
  win.on("resized", () => {
    logBounds(id, win, "resized");
  });
  windows.set(id, win);
  lastBounds.set(id, win.getBounds());
  logBounds(id, win, "created");
  return win;
}

function applyMode(next: Mode): void {
  mode = next;
  for (const [id, win] of windows) {
    win.setResizable(false);
    win.setIgnoreMouseEvents(mode === "passthrough", { forward: true });
    win.webContents.send("mode", mode);
    win.webContents.send("debug-state", { id, mode, bounds: win.getBounds() });
    logBounds(id, win, "mode");
  }
}

app.whenReady().then(() => {
  console.log("[overlay-spike] displays", screen.getAllDisplays().map((display) => display.bounds));
  startTrackerSidecar();
  startOverlayPayloadWatcher();
  const geometry = readGeometry(defaultGeometry());
  createOverlay("player", geometry.player);
  createOverlay("opponent", geometry.opponent);

  globalShortcut.register("CommandOrControl+Shift+P", () => {
    applyMode(mode === "passthrough" ? "interactive" : "passthrough");
  });
  globalShortcut.register("CommandOrControl+Shift+E", () => {
    applyMode(mode === "edit" ? "interactive" : "edit");
  });
  globalShortcut.register("CommandOrControl+Shift+H", () => {
    for (const win of windows.values()) {
      if (win.isVisible()) win.hide();
      else win.showInactive();
    }
  });
  globalShortcut.register("CommandOrControl+Shift+R", resetGeometry);

  ipcMain.on("set-mode", (_event, next: Mode) => applyMode(next));
  ipcMain.on("reset-geometry", resetGeometry);
  ipcMain.on("resize-overlay-start", (_event, request: ResizeStartRequest) =>
    startCustomResize(request)
  );
  ipcMain.on("resize-overlay", (_event, request: ResizeRequest) => applyCustomResize(request));
  ipcMain.on("resize-overlay-finished", finishCustomResize);
  applyMode(mode);
});

app.on("will-quit", () => {
  if (overlayPayloadWatchPath) {
    fs.unwatchFile(overlayPayloadWatchPath);
  }
  if (sidecar && !sidecar.killed) {
    sidecar.kill();
  }
  writeGeometry();
  globalShortcut.unregisterAll();
});
