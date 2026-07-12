import { app, BrowserWindow, globalShortcut, ipcMain, screen } from "electron";
import path from "node:path";
import fs from "node:fs";

type OverlayId = "player" | "opponent";
type Mode = "interactive" | "passthrough" | "edit";
type Geometry = { x: number; y: number; width: number; height: number };
type GeometryStore = Record<OverlayId, Geometry>;

let mode: Mode = "interactive";
const windows = new Map<OverlayId, BrowserWindow>();

const defaultGeometry: GeometryStore = {
  player: { x: 40, y: 120, width: 230, height: 460 },
  opponent: { x: 1260, y: 120, width: 230, height: 460 }
};

if (process.platform === "linux") {
  app.commandLine.appendSwitch("ozone-platform", "x11");
}

function geometryPath(): string {
  return path.join(app.getPath("userData"), "electron-spike-geometry.json");
}

function readGeometry(): GeometryStore {
  try {
    const parsed = JSON.parse(fs.readFileSync(geometryPath(), "utf8")) as Partial<GeometryStore>;
    return {
      player: { ...defaultGeometry.player, ...parsed.player },
      opponent: { ...defaultGeometry.opponent, ...parsed.opponent }
    };
  } catch {
    return defaultGeometry;
  }
}

function writeGeometry(): void {
  const data: Partial<GeometryStore> = {};
  for (const [id, win] of windows) {
    data[id] = win.getBounds();
  }
  fs.mkdirSync(path.dirname(geometryPath()), { recursive: true });
  fs.writeFileSync(geometryPath(), JSON.stringify(data, null, 2));
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
    ...geometry,
    x: Math.min(Math.max(geometry.x, union.minX), union.maxX - 80),
    y: Math.min(Math.max(geometry.y, union.minY), union.maxY - 80)
  };
}

function createOverlay(id: OverlayId, geometry: Geometry): BrowserWindow {
  const win = new BrowserWindow({
    ...clampToDisplays(geometry),
    minWidth: 180,
    minHeight: 300,
    frame: false,
    transparent: true,
    alwaysOnTop: true,
    resizable: false,
    skipTaskbar: true,
    focusable: false,
    hasShadow: false,
    title: `OpenSnapTracker ${id}`,
    webPreferences: {
      nodeIntegration: true,
      contextIsolation: false
    }
  });

  win.setAlwaysOnTop(true, "screen-saver");
  win.setVisibleOnAllWorkspaces(true, { visibleOnFullScreen: true });
  win.loadFile(path.join(__dirname, "../src/index.html"), { query: { id } });
  win.on("moved", writeGeometry);
  win.on("resized", writeGeometry);
  windows.set(id, win);
  return win;
}

function applyMode(next: Mode): void {
  mode = next;
  for (const win of windows.values()) {
    win.setResizable(mode === "edit");
    win.setMovable(mode === "edit");
    win.setFocusable(mode === "edit");
    win.setIgnoreMouseEvents(mode === "passthrough", { forward: true });
    win.webContents.send("mode", mode);
  }
}

app.whenReady().then(() => {
  const geometry = readGeometry();
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

  ipcMain.on("set-mode", (_event, next: Mode) => applyMode(next));
  applyMode("interactive");
});

app.on("will-quit", () => {
  writeGeometry();
  globalShortcut.unregisterAll();
});
