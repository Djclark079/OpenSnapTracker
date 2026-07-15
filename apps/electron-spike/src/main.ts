import { app, BrowserWindow, globalShortcut, ipcMain, screen } from "electron";
import path from "node:path";
import fs from "node:fs";
import { createHash } from "node:crypto";
import { execFile, spawn, type ChildProcess } from "node:child_process";

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
  startExpansionHeight: number;
  startScreenX: number;
  startScreenY: number;
};
type OverlayPayload = Record<string, unknown> & {
  player?: unknown;
  opponent?: unknown;
};
type SidecarEvent = {
  event?: string;
  path?: string;
  source?: string;
  changed?: boolean;
  game_hash?: string | null;
  reason?: string;
};
type FocusHelperWindow = {
  id: string;
  id_decimal: number;
  title?: string | null;
  class?: string | null;
  instance?: string | null;
  mapped: boolean;
};
type FocusHelperActivateResult = {
  activated: boolean;
  window?: FocusHelperWindow | null;
  candidates?: FocusHelperWindow[];
};
type FocusBounceOptions = {
  minIntervalMs?: number;
  settle?: boolean;
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
const baseBounds = new Map<OverlayId, Geometry>();
const expansionHeights = new Map<OverlayId, number>();
const activeResizes = new Map<OverlayId, ActiveResize>();
let overlayPayload: OverlayPayload | null = null;
let overlayPayloadHash: string | null = null;
let overlayPayloadSequence = 0;
let overlayPayloadPollTimer: NodeJS.Timeout | null = null;
let sidecar: ChildProcess | null = null;
let overlayPayloadWatchPath: string | null = null;
let lastFocusBounceAt = 0;
let focusBounceInFlight = false;
let delayedFocusBounceTimer: NodeJS.Timeout | null = null;
let delayedFocusBounceReason: string | null = null;
let resolutionBounceTimer: NodeJS.Timeout | null = null;
let resolutionBounceStartedAt = 0;
let resolutionBounceActive = false;

if (process.platform === "linux") {
  app.disableHardwareAcceleration();
  app.commandLine.appendSwitch("ozone-platform", "x11");
  app.commandLine.appendSwitch("disable-gpu");
  app.commandLine.appendSwitch("disable-renderer-backgrounding");
  app.commandLine.appendSwitch("disable-background-timer-throttling");
  app.commandLine.appendSwitch("disable-backgrounding-occluded-windows");
  app.commandLine.appendSwitch("disable-features", "CalculateNativeWinOcclusion");
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

function maxSizeForGeometry(
  geometry: Geometry,
  expansionHeight = 0
): Pick<Geometry, "width" | "height"> {
  const display = screen.getDisplayMatching(geometry);
  const maxDisplayHeight = display.workArea.height - maxVerticalReserve - expansionHeight;
  const maxHeight = Math.max(
    minHeight,
    Math.min(maxDisplayHeight, Math.round((display.workArea.width - maxHorizontalReserve) / overlayAspectRatio))
  );
  const maxWidth = Math.max(minWidth, display.workArea.width - maxHorizontalReserve);
  const heightFromWidth = Math.round(maxWidth / overlayAspectRatio);
  const height = Math.min(maxHeight, heightFromWidth);
  return {
    width: Math.round(height * overlayAspectRatio),
    height
  };
}

function clampResizeGeometryToDisplay(
  geometry: Geometry,
  expansionHeight: number,
  anchorRight: boolean
): Geometry {
  const display = screen.getDisplayMatching(geometry);
  const topLimit = display.workArea.y + screenEdgeReserve;
  const bottomLimit = display.workArea.y + display.workArea.height - screenEdgeReserve;
  const maxSize = maxSizeForGeometry(geometry, expansionHeight);
  const baseHeight = Math.min(
    Math.max(minHeight, geometry.height),
    maxSize.height,
    Math.max(minHeight, bottomLimit - topLimit - expansionHeight)
  );
  const baseWidth = Math.round(baseHeight * overlayAspectRatio);
  let x = anchorRight ? geometry.x + geometry.width - baseWidth : geometry.x;
  let y = geometry.y;
  const bottomOverflow = y + baseHeight + expansionHeight - bottomLimit;
  if (bottomOverflow > 0) {
    y -= bottomOverflow;
  }
  if (y < topLimit) {
    y = topLimit;
  }
  return {
    x,
    y,
    width: baseWidth,
    height: baseHeight
  };
}

function totalBounds(id: OverlayId, geometry: Geometry): Geometry {
  return {
    ...geometry,
    height: geometry.height + (expansionHeights.get(id) ?? 0)
  };
}

function currentBaseBounds(id: OverlayId, win: BrowserWindow): Geometry {
  const bounds = win.getBounds();
  return {
    ...bounds,
    height: Math.max(minHeight, bounds.height - (expansionHeights.get(id) ?? 0))
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
  for (const [id] of windows) {
    data[id] = baseBounds.get(id);
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

  const candidate = {
    x,
    y: startBounds.y,
    width,
    height
  };
  return clampResizeGeometryToDisplay(
    candidate,
    activeResize.startExpansionHeight,
    activeResize.edge === "west" || activeResize.edge === "south-west"
  );
}

function startCustomResize(request: ResizeStartRequest): void {
  if (mode !== "edit") return;
  const win = windows.get(request.id);
  if (!win || win.isDestroyed()) return;
  const currentBase = currentBaseBounds(request.id, win);
  baseBounds.set(request.id, currentBase);

  activeResizes.set(request.id, {
    edge: request.edge,
    startBounds: currentBase,
    startExpansionHeight: expansionHeights.get(request.id) ?? 0,
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
  baseBounds.set(request.id, bounds);
  win.setBounds(totalBounds(request.id, bounds));
  win.webContents.send("debug-state", { id: request.id, mode, bounds: totalBounds(request.id, bounds) });
}

function finishCustomResize(): void {
  for (const [id, win] of windows) {
    if (!win.isDestroyed()) {
      baseBounds.set(id, currentBaseBounds(id, win));
    }
  }
  activeResizes.clear();
  writeGeometry();
}

function resetGeometry(): void {
  const geometry = defaultGeometry();
  for (const [id, win] of windows) {
    expansionHeights.set(id, 0);
    baseBounds.set(id, geometry[id]);
    win.setBounds(geometry[id]);
    win.webContents.send("supplemental-expansion-reset");
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
    baseBounds: baseBounds.get(id),
    expansionHeight: expansionHeights.get(id) ?? 0,
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

function focusBounceEnabled(): boolean {
  return process.argv.includes("--focus-bounce") || process.env.OST_FOCUS_BOUNCE === "1";
}

function gameStatePath(): string | null {
  const stateDir = liveStateDir();
  return stateDir ? path.join(stateDir, "GameState.json") : null;
}

function fileFingerprint(filePath: string | null): string | null {
  if (!filePath || !fs.existsSync(filePath)) return null;
  try {
    const stat = fs.statSync(filePath);
    const hash = createHash("sha256").update(fs.readFileSync(filePath)).digest("hex");
    return `${stat.mtimeMs}:${stat.size}:${hash.slice(0, 12)}`;
  } catch {
    return null;
  }
}

function repoRoot(): string {
  return path.resolve(__dirname, "../../..");
}

function focusHelperInvocation(extraArgs: string[]): { command: string; args: string[]; cwd?: string } {
  const configured = process.env.OST_FOCUS_HELPER;
  if (configured) {
    return { command: configured, args: extraArgs };
  }

  const packagedHelper = path.join(process.resourcesPath, "x11-focus-helper");
  if (app.isPackaged && fs.existsSync(packagedHelper)) {
    return { command: packagedHelper, args: extraArgs };
  }

  return {
    command: "cargo",
    args: ["run", "-q", "-p", "x11-focus-helper", "--", ...extraArgs],
    cwd: repoRoot()
  };
}

function activateSnapWindow(
  callback: (result: FocusHelperActivateResult | null, error: Error | null) => void
): void {
  const args = ["activate", "--title", process.env.OST_SNAP_WINDOW_TITLE || "SNAP"];
  const configuredWindow = process.env.OST_SNAP_WINDOW_ID;
  if (configuredWindow) {
    args.push("--window", configuredWindow);
  }

  const invocation = focusHelperInvocation(args);
  execFile(invocation.command, invocation.args, { cwd: invocation.cwd }, (error, stdout, stderr) => {
    if (error) {
      const message = stderr.trim() || error.message;
      callback(null, new Error(message));
      return;
    }

    try {
      callback(JSON.parse(stdout) as FocusHelperActivateResult, null);
    } catch (parseError) {
      callback(null, parseError instanceof Error ? parseError : new Error(String(parseError)));
    }
  });
}

function scheduleFocusBounce(reason: string, delayMs: number): void {
  delayedFocusBounceReason = reason;
  if (delayedFocusBounceTimer) return;

  delayedFocusBounceTimer = setTimeout(() => {
    const nextReason = delayedFocusBounceReason ?? "hidden-zone-change";
    delayedFocusBounceTimer = null;
    delayedFocusBounceReason = null;
    requestFocusBounce(nextReason);
  }, delayMs);
}

function scheduleSettledFocusBounce(reason: string): void {
  if (reason.endsWith(":settle")) return;
  scheduleFocusBounce(`${reason}:settle`, 1500);
}

function requestFocusBounce(reason: string, options: FocusBounceOptions = {}): void {
  if (!focusBounceEnabled()) return;
  const now = Date.now();
  const minIntervalMs = options.minIntervalMs ?? 1200;
  const settle = options.settle ?? true;
  if (focusBounceInFlight || now - lastFocusBounceAt < minIntervalMs) {
    const retryDelay = focusBounceInFlight
      ? 350
      : Math.max(150, minIntervalMs + 50 - (now - lastFocusBounceAt));
    console.log("[overlay-spike] focus bounce skipped", {
      reason,
      inFlight: focusBounceInFlight,
      sinceMs: now - lastFocusBounceAt,
      retryDelay
    });
    if (settle) {
      scheduleFocusBounce(reason, retryDelay);
    }
    return;
  }

  const visibleWindow = [...windows.values()].find((win) => !win.isDestroyed() && win.isVisible());
  if (!visibleWindow) return;

  focusBounceInFlight = true;
  lastFocusBounceAt = now;
  const before = fileFingerprint(gameStatePath());
  visibleWindow.focus();

  setTimeout(() => {
    activateSnapWindow((result, error) => {
      if (error) {
        console.warn("[overlay-spike] focus bounce helper failed", {
          reason,
          message: error.message
        });
        focusBounceInFlight = false;
        return;
      }

      if (!result?.activated || !result.window) {
        console.warn("[overlay-spike] focus bounce could not find SNAP window", {
          reason,
          candidates: result?.candidates?.slice(-8) ?? []
        });
        focusBounceInFlight = false;
        return;
      }

      setTimeout(() => {
        const after = fileFingerprint(gameStatePath());
        console.log("[overlay-spike] focus bounce completed", {
          reason,
          snapWindow: result.window,
          gameStateChanged: before !== after,
          before,
          after
        });
        focusBounceInFlight = false;
        if (settle) {
          scheduleSettledFocusBounce(reason);
        }
      }, 250);
    });
  }, 50);
}

function requestResolutionBounce(): void {
  requestFocusBounce("resolution-loop", {
    minIntervalMs: 450,
    settle: false
  });
}

function startResolutionBounceLoop(): void {
  if (resolutionBounceActive) return;
  resolutionBounceActive = true;
  resolutionBounceStartedAt = Date.now();
  console.log("[overlay-spike] resolution bounce loop started");
  requestResolutionBounce();
  resolutionBounceTimer = setInterval(() => {
    if (Date.now() - resolutionBounceStartedAt > 30000) {
      stopResolutionBounceLoop("max-duration");
      return;
    }
    requestResolutionBounce();
  }, 500);
}

function stopResolutionBounceLoop(reason: string): void {
  if (!resolutionBounceActive && !resolutionBounceTimer) return;
  if (resolutionBounceTimer) {
    clearInterval(resolutionBounceTimer);
    resolutionBounceTimer = null;
  }
  resolutionBounceActive = false;
  console.log("[overlay-spike] resolution bounce loop stopped", { reason });
}

function defaultLivePayloadPath(): string {
  return path.join(app.getPath("userData"), "live-overlay.json");
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function readOverlayPayload(): { payload: OverlayPayload; hash: string } | null {
  const payloadPath = overlayPayloadWatchPath ?? overlayPayloadPath();
  if (!payloadPath) return null;
  if (!fs.existsSync(payloadPath)) return null;

  try {
    const bytes = fs.readFileSync(payloadPath);
    const hash = createHash("sha256").update(bytes).digest("hex");
    const parsed: unknown = JSON.parse(bytes.toString("utf8"));
    if (!isRecord(parsed) || !isRecord(parsed.player) || !isRecord(parsed.opponent)) {
      console.warn("[overlay-spike] ignored overlay payload with unexpected shape", payloadPath);
      return null;
    }
    return { payload: parsed as OverlayPayload, hash };
  } catch (error) {
    console.warn("[overlay-spike] could not load overlay payload", payloadPath, error);
    return null;
  }
}

function payloadPanel(id: OverlayId): unknown {
  return overlayPayload?.[id] ?? null;
}

function broadcastOverlayPayload(): void {
  overlayPayloadSequence += 1;
  for (const [id, win] of windows) {
    if (win.isDestroyed()) continue;
    win.webContents.send("overlay-payload", {
      id,
      panel: payloadPanel(id),
      sourcePath: overlayPayloadWatchPath ?? overlayPayloadPath() ?? null,
      sequence: overlayPayloadSequence
    });
    win.webContents
      .executeJavaScript("globalThis.__ostForcePaint?.()", true)
      .catch(() => undefined);
  }
}

function reloadOverlayPayload(force = false): void {
  const next = readOverlayPayload();
  if (!next) return;
  if (!force && next.hash === overlayPayloadHash) return;
  overlayPayload = next.payload;
  overlayPayloadHash = next.hash;
  console.log("[overlay-spike] loaded overlay payload", {
    path: overlayPayloadWatchPath ?? overlayPayloadPath() ?? null,
    sequence: overlayPayloadSequence + 1
  });
  broadcastOverlayPayload();
}

function startOverlayPayloadWatcher(): void {
  const payloadPath = overlayPayloadWatchPath ?? overlayPayloadPath();
  if (!payloadPath) return;
  overlayPayloadWatchPath = payloadPath;
  reloadOverlayPayload(true);
  fs.watchFile(payloadPath, { interval: 250 }, (current, previous) => {
    if (current.mtimeMs !== previous.mtimeMs || current.size !== previous.size) {
      reloadOverlayPayload();
    }
  });
  overlayPayloadPollTimer = setInterval(() => reloadOverlayPayload(), 250);
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
      "--quiet",
      "-p",
      "tracker-sidecar",
      "--",
      "--state-dir",
      stateDir,
      "--output-json",
      outputJson,
      "--interval-ms",
      "250",
      "--stdout-events",
      "--debug-polls"
    ],
    {
      cwd: path.resolve(__dirname, "../../.."),
      stdio: ["ignore", "pipe", "inherit"]
    }
  );
  let stdoutBuffer = "";
  sidecar.stdout?.setEncoding("utf8");
  sidecar.stdout?.on("data", (chunk: string) => {
    stdoutBuffer += chunk;
    const lines = stdoutBuffer.split(/\r?\n/);
    stdoutBuffer = lines.pop() ?? "";
    for (const line of lines) {
      handleSidecarEventLine(line);
    }
  });
  sidecar.on("exit", (code, signal) => {
    console.log("[overlay-spike] tracker sidecar exited", { code, signal });
    sidecar = null;
  });
}

function handleSidecarEventLine(line: string): void {
  if (!line.trim()) return;
  let event: SidecarEvent;
  try {
    event = JSON.parse(line) as SidecarEvent;
  } catch {
    console.log("[tracker-sidecar]", line);
    return;
  }

  if (event.event === "payload-written") {
    console.log("[tracker-sidecar] payload-written", {
      path: event.path,
      source: event.source ?? null
    });
    reloadOverlayPayload(true);
    return;
  }

  if (event.event === "poll") {
    console.log("[tracker-sidecar] poll", {
      changed: event.changed,
      gameHash: event.game_hash?.slice(0, 12) ?? null
    });
    return;
  }

  if (event.event === "focus-bounce-requested") {
    const reason = event.reason ?? "hidden-zone-change";
    console.log("[tracker-sidecar] focus-bounce-requested", {
      reason
    });
    if (reason === "resolution-started") {
      startResolutionBounceLoop();
    } else if (reason === "turn-start") {
      stopResolutionBounceLoop(reason);
      scheduleFocusBounce(reason, 250);
    } else if (reason === "match-ended") {
      stopResolutionBounceLoop(reason);
      scheduleFocusBounce(reason, 250);
    } else {
      requestFocusBounce(reason);
    }
    return;
  }

  console.log("[tracker-sidecar]", event);
}

function createOverlay(id: OverlayId, geometry: Geometry): BrowserWindow {
  const base = clampToDisplays(geometry);
  baseBounds.set(id, base);
  expansionHeights.set(id, 0);
  const win = new BrowserWindow({
    ...base,
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
      sourcePath: overlayPayloadPath() ?? null,
      sequence: overlayPayloadSequence
    });
  });
  win.on("moved", () => {
    baseBounds.set(id, currentBaseBounds(id, win));
    logBounds(id, win, "moved");
    writeGeometry();
  });
  win.on("resized", () => {
    logBounds(id, win, "resized");
  });
  windows.set(id, win);
  logBounds(id, win, "created");
  return win;
}

function setSupplementalExpansion(id: OverlayId, requestedHeight: number): void {
  const win = windows.get(id);
  if (!win || win.isDestroyed()) return;
  const nextHeight = Math.max(0, Math.round(requestedHeight));
  if ((expansionHeights.get(id) ?? 0) === nextHeight) return;
  const base = currentBaseBounds(id, win);
  baseBounds.set(id, base);
  expansionHeights.set(id, nextHeight);
  const maxSize = maxSizeForGeometry(base, nextHeight);
  const baseHeight = Math.min(base.height, maxSize.height);
  const width = Math.min(base.width, Math.round(baseHeight * overlayAspectRatio));
  const clampedBase = {
    ...base,
    width,
    height: baseHeight
  };
  baseBounds.set(id, clampedBase);
  win.setBounds(totalBounds(id, clampedBase));
  win.webContents.send("debug-state", { id, mode, bounds: win.getBounds() });
  writeGeometry();
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
  ipcMain.on("set-supplemental-expansion", (_event, request: { id: OverlayId; height: number }) =>
    setSupplementalExpansion(request.id, request.height)
  );
  applyMode(mode);
});

app.on("will-quit", () => {
  if (overlayPayloadWatchPath) {
    fs.unwatchFile(overlayPayloadWatchPath);
  }
  if (overlayPayloadPollTimer) {
    clearInterval(overlayPayloadPollTimer);
  }
  if (delayedFocusBounceTimer) {
    clearTimeout(delayedFocusBounceTimer);
  }
  if (resolutionBounceTimer) {
    clearInterval(resolutionBounceTimer);
  }
  if (sidecar && !sidecar.killed) {
    sidecar.kill();
  }
  writeGeometry();
  globalShortcut.unregisterAll();
});
