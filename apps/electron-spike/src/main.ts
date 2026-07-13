import { app, BrowserWindow, globalShortcut, ipcMain, screen } from "electron";
import path from "node:path";
import fs from "node:fs";

type OverlayId = "player" | "opponent";
type Mode = "interactive" | "passthrough" | "edit";
type Geometry = { x: number; y: number; width: number; height: number };
type GeometryStore = Record<OverlayId, Geometry>;

const minWidth = 288;
const minHeight = 560;
const defaultWidth = 310;
const defaultHeight = 600;

let mode: Mode = process.argv.includes("--start-edit") ? "edit" : "interactive";
const windows = new Map<OverlayId, BrowserWindow>();

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

function writeGeometry(): void {
  const data: Partial<GeometryStore> = {};
  for (const [id, win] of windows) {
    data[id] = win.getBounds();
  }
  fs.mkdirSync(path.dirname(geometryPath()), { recursive: true });
  fs.writeFileSync(geometryPath(), JSON.stringify(data, null, 2));
}

function resetGeometry(): void {
  const geometry = defaultGeometry();
  for (const [id, win] of windows) {
    win.setBounds(geometry[id]);
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
    ...enforceMinimumGeometry(geometry),
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
      contextIsolation: false
    }
  });

  win.setAlwaysOnTop(true, "screen-saver");
  win.setVisibleOnAllWorkspaces(true, { visibleOnFullScreen: true });
  win.loadFile(path.join(__dirname, "../src/index.html"), { query: { id } });
  win.once("ready-to-show", () => {
    win.show();
    logBounds(id, win, "ready-to-show");
  });
  win.webContents.once("did-finish-load", () => {
    win.webContents.send("debug-state", { id, mode, bounds: win.getBounds() });
  });
  win.on("moved", () => {
    logBounds(id, win, "moved");
    writeGeometry();
  });
  win.on("resized", () => {
    logBounds(id, win, "resized");
    writeGeometry();
  });
  windows.set(id, win);
  logBounds(id, win, "created");
  return win;
}

function applyMode(next: Mode): void {
  mode = next;
  for (const [id, win] of windows) {
    win.setResizable(mode === "edit");
    win.setIgnoreMouseEvents(mode === "passthrough", { forward: true });
    win.webContents.send("mode", mode);
    win.webContents.send("debug-state", { id, mode, bounds: win.getBounds() });
    logBounds(id, win, "mode");
  }
}

app.whenReady().then(() => {
  console.log("[overlay-spike] displays", screen.getAllDisplays().map((display) => display.bounds));
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
  applyMode(mode);
});

app.on("will-quit", () => {
  writeGeometry();
  globalShortcut.unregisterAll();
});
