import { emit, listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { register } from "@tauri-apps/plugin-global-shortcut";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "./styles.css";

const params = new URLSearchParams(location.search);
const id = params.get("id") ?? "player";
const appWindow = getCurrentWindow();

document.querySelector<HTMLDivElement>("#app")!.innerHTML = `
  <section class="overlay">
    <header id="titlebar" data-tauri-drag-region="deep">
      <button id="interactive" title="Interactive">I</button>
      <strong>${id === "player" ? "Player Deck" : "Opponent"}</strong>
      <span id="debug-badge" class="debug-badge">${id.toUpperCase()}</span>
      <button id="reset" title="Reset geometry">R</button>
      <button id="edit" title="Edit">E</button>
    </header>
    <div id="debug-line" class="debug-line">${id} | edit | native title tooltip</div>
    <main class="grid"></main>
    <footer>
      <button><span>Deck</span><strong>8</strong></button>
      <button><span>Hand</span><strong>4</strong></button>
      <button id="zone">Destroyed <span>0</span></button>
      <button><span>Discard</span><strong>0</strong></button>
      <button><span>Removed</span><strong>0</strong></button>
    </footer>
  </section>
`;

let mode: "interactive" | "passthrough" | "edit" = "interactive";
const grid = document.querySelector<HTMLDivElement>(".grid")!;
const debugLine = document.querySelector<HTMLDivElement>("#debug-line")!;

function render(): void {
  grid.innerHTML = "";
  for (let i = 0; i < 12; i += 1) {
    const card = document.createElement("button");
    card.className = "card";
    if (id === "opponent" && i > 4) card.classList.add("unknown");
    if (i === 1 || i === 6) card.classList.add("consumed");
    card.textContent = id === "opponent" && i > 4 ? "?" : String(i + 1);
    card.title = `Placeholder ${i + 1} | Cost ${i % 6} | Power ${(i + 1) % 10} | Canonical ability text`;
    card.addEventListener("mouseenter", (event) => {
      if (mode === "passthrough") {
        event.currentTarget instanceof HTMLElement && event.currentTarget.removeAttribute("title");
      } else if (event.currentTarget instanceof HTMLElement) {
        event.currentTarget.title = `Placeholder ${i + 1} | Cost ${i % 6} | Power ${(i + 1) % 10} | Canonical ability text`;
      }
    });
    grid.append(card);
  }
}

async function setMode(next: typeof mode): Promise<void> {
  mode = next;
  document.body.dataset.mode = mode;
  debugLine.textContent = `${id} | ${mode}`;
  await invoke("set_mode", { mode });
  if (mode === "edit") await appWindow.setFocus();
}

document.querySelector("#titlebar")?.addEventListener("mousedown", (event) => {
  if (mode !== "edit" || event.target instanceof HTMLButtonElement) return;
  void appWindow.startDragging();
});
document.querySelector("#interactive")?.addEventListener("click", () => void setMode("interactive"));
document.querySelector("#edit")?.addEventListener("click", () => void invoke("toggle_edit"));
document.querySelector("#reset")?.addEventListener("click", () => void invoke("reset_geometry"));
document.querySelector("#zone")?.addEventListener("click", () => {
  document.querySelector("strong")!.textContent = "Destroyed";
});

void listen<string>("mode", (event) => {
  mode = event.payload as typeof mode;
  document.body.dataset.mode = mode;
  debugLine.textContent = `${id} | ${mode}`;
  if (mode === "passthrough") {
    document.querySelectorAll<HTMLElement>(".card").forEach((card) => card.removeAttribute("title"));
  }
});

if (id === "player") {
  await register("CommandOrControl+Shift+P", (event) => {
    if (event.state === "Pressed") void invoke("toggle_passthrough");
  });
  await register("CommandOrControl+Shift+E", (event) => {
    if (event.state === "Pressed") void invoke("toggle_edit");
  });
  await register("CommandOrControl+Shift+H", (event) => {
    if (event.state === "Pressed") void invoke("toggle_visibility");
  });
  await register("CommandOrControl+Shift+R", (event) => {
    if (event.state === "Pressed") void invoke("reset_geometry");
  });
  await emit("tauri-spike-shortcuts-ready");
}

await setMode("edit");
render();
