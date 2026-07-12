import { getCurrentWindow } from "@tauri-apps/api/window";
import "./styles.css";

const params = new URLSearchParams(location.search);
const id = params.get("id") ?? "player";
const appWindow = getCurrentWindow();

document.querySelector<HTMLDivElement>("#app")!.innerHTML = `
  <section class="overlay">
    <header data-tauri-drag-region>
      <button id="interactive" title="Interactive">I</button>
      <strong>${id === "player" ? "Player Deck" : "Opponent"}</strong>
      <button id="edit" title="Edit">E</button>
    </header>
    <main class="grid"></main>
    <footer>
      <button>Deck <span>8</span></button>
      <button>Hand <span>4</span></button>
      <button id="zone">Destroyed <span>0</span></button>
      <button>Discarded <span>0</span></button>
      <button>Removed <span>0</span></button>
    </footer>
    <div id="tooltip" class="tooltip" hidden></div>
  </section>
`;

let mode: "interactive" | "passthrough" | "edit" = "interactive";
const grid = document.querySelector<HTMLDivElement>(".grid")!;
const tooltip = document.querySelector<HTMLDivElement>("#tooltip")!;

function render(): void {
  grid.innerHTML = "";
  for (let i = 0; i < 12; i += 1) {
    const card = document.createElement("button");
    card.className = "card";
    if (id === "opponent" && i > 4) card.classList.add("unknown");
    if (i === 1 || i === 6) card.classList.add("consumed");
    card.textContent = id === "opponent" && i > 4 ? "?" : String(i + 1);
    let timer = 0;
    card.addEventListener("mouseenter", () => {
      if (mode === "passthrough") return;
      timer = window.setTimeout(() => {
        tooltip.textContent = `Placeholder ${i + 1} | Cost ${i % 6} | Power ${(i + 1) % 10} | Canonical ability text`;
        tooltip.hidden = false;
      }, 250);
    });
    card.addEventListener("mouseleave", () => {
      window.clearTimeout(timer);
      tooltip.hidden = true;
    });
    grid.append(card);
  }
}

async function setMode(next: typeof mode): Promise<void> {
  mode = next;
  document.body.dataset.mode = mode;
  await appWindow.setResizable(mode === "edit");
  await appWindow.setFocus();
}

document.querySelector("#interactive")?.addEventListener("click", () => void setMode("interactive"));
document.querySelector("#edit")?.addEventListener("click", () => void setMode("edit"));
document.querySelector("#zone")?.addEventListener("click", () => {
  document.querySelector("strong")!.textContent = "Destroyed";
});

render();
