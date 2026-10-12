import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// Sound Cues: the app plays a sound when an agent asks something and another when it finishes (see `play` in
// lib.rs); every window mirrors the switch. On unless turned off, remembered across launches.
export const cues = $state({ on: false });

function set(on: boolean, preview = false) {
  invoke<boolean>("sound_cues", { on, preview }).then((v) => (cues.on = v)).catch(() => {});
}

export function toggleSoundCues() {
  const on = !cues.on;
  try {
    localStorage.setItem("soundCues", on ? "on" : "off");
  } catch {}
  set(on, on); // turning them on plays one, so you know what to listen for
}

listen<boolean>("sound-cues", ({ payload }) => (cues.on = payload)).catch(() => {});
let stored: string | null = null;
try {
  stored = localStorage.getItem("soundCues");
} catch {}
set(stored !== "off");
