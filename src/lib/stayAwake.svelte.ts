import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// The app owns one caffeinate process; every window mirrors its state. Remembered across launches.
export const awake = $state({ on: false });

function set(on?: boolean) {
  invoke<boolean>("stay_awake", { on }).then((v) => (awake.on = v)).catch(() => {});
}

export function toggleStayAwake() {
  const on = !awake.on;
  try {
    localStorage.setItem("stayAwake", on ? "1" : "");
  } catch {}
  set(on);
}

listen<boolean>("stay-awake", ({ payload }) => (awake.on = payload)).catch(() => {});
let stored = false;
try {
  stored = !!localStorage.getItem("stayAwake");
} catch {}
set(stored || undefined);
