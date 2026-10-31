import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";
import Detached from "./lib/Detached.svelte";
import { tooltips } from "./lib/tooltip";

// Keep the webview's own shortcuts (reload, print, find) and context menu out of the way:
// reloading would drop every open session.
window.addEventListener(
  "keydown",
  (e) => {
    const k = e.key.toLowerCase();
    // Ctrl+Shift+arrows would select page text; outside text fields they belong to the terminal.
    // a synthetic event aimed at the document has no element to ask
    const field = (e.target as HTMLElement).closest?.("input, select");
    if (e.ctrlKey && e.shiftKey && e.key.startsWith("Arrow") && !field) e.preventDefault();
    if (e.key === "F5" || e.key === "F7" || ((e.ctrlKey || e.metaKey) && ["r", "p", "f", "g", "j", "u"].includes(k))) {
      e.preventDefault();
    }
  },
  true,
);
window.addEventListener("contextmenu", (e) => {
  const t = e.target as HTMLElement;
  if (!t.closest?.("input, textarea")) e.preventDefault();
});

tooltips();

// A detached tab's window (src/main.rs `detach_tab`) shows just that terminal.
mount("__DETACHED__" in window ? Detached : App, { target: document.getElementById("app")! });
