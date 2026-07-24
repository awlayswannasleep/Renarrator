// Renarrator — custom liquid-glass tray menu window.
"use strict";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

let paused = false;

const pauseBtn = document.getElementById("mi-pause");
const pauseLabel = document.getElementById("pause-label");

function renderPause() {
  pauseBtn.classList.toggle("checked", paused);
  pauseLabel.textContent = paused ? "Resume Detection" : "Pause Detection";
}

function closeMenu() {
  invoke("hide_tray_menu").catch(() => {});
}

document.getElementById("mi-open").addEventListener("click", () => {
  invoke("show_settings").catch(() => {});
  closeMenu();
});

pauseBtn.addEventListener("click", () => {
  paused = !paused;
  invoke("toggle_pause", { paused }).catch(() => {});
  renderPause();
  closeMenu();
});

document.getElementById("mi-quit").addEventListener("click", () => {
  invoke("quit_app").catch(() => {});
});

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") closeMenu();
});

async function invokeRetry(cmd, args, attempts = 8, delayMs = 250) {
  let lastErr;
  for (let i = 0; i < attempts; i++) {
    try {
      return await invoke(cmd, args);
    } catch (e) {
      lastErr = e;
      if (!String(e).includes("not managed")) throw e;
      await new Promise((r) => setTimeout(r, delayMs));
    }
  }
  throw lastErr;
}

(async () => {
  try {
    const st = await invokeRetry("get_state");
    paused = st.paused;
    renderPause();
    await listen("pause-changed", (e) => {
      paused = e.payload;
      renderPause();
    });
  } catch (e) {
    // Меню немодальное — молча живём без состояния.
  }
})();
