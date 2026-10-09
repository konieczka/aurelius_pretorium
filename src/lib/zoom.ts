import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

// Native webview zoom (like VS Code's): scales everything, and xterm/CodeMirror re-measure on their own.
let level = 1;
try {
  level = Number(localStorage.getItem("zoom")) || 1;
} catch {}

/** Current webview zoom: CSS px × level = logical window px. */
export const zoomLevel = () => level;

export function zoom(delta: number | null) {
  // null = reset; round to avoid 1.0999999 drift
  level = delta === null ? 1 : Math.round(Math.min(2, Math.max(0.5, level + delta)) * 10) / 10;
  try {
    localStorage.setItem("zoom", String(level));
  } catch {}
  getCurrentWebview().setZoom(level).catch(() => {});
}

zoom(0); // restore saved level on startup

// View menu (⌘= / ⌘- / ⌘0), sent to the focused window only.
getCurrentWebviewWindow()
  .listen<string>("zoom", ({ payload }) => zoom(payload === "zoom-in" ? 0.1 : payload === "zoom-out" ? -0.1 : null))
  .catch(() => {});
