import { getCurrentWindow } from "@tauri-apps/api/window";

export type Mode = "light" | "dark" | "system";
const media = matchMedia("(prefers-color-scheme: dark)");
let stored: string | null = null;
try {
  stored = localStorage.getItem("theme");
} catch {}

export const theme = $state({ mode: (["light", "dark"].includes(stored!) ? stored : "system") as Mode, dark: false });

function apply() {
  theme.dark = theme.mode === "dark" || (theme.mode === "system" && media.matches);
  document.documentElement.classList.toggle("dark", theme.dark);
  // Native title bar follows too (null = follow OS). Outside Tauri (browser dev) this just fails.
  getCurrentWindow().setTheme(theme.mode === "system" ? null : theme.mode).catch(() => {});
}

export function cycleTheme() {
  theme.mode = ({ light: "dark", dark: "system", system: "light" } as const)[theme.mode];
  try {
    localStorage.setItem("theme", theme.mode);
  } catch {}
  apply();
}

media.addEventListener("change", apply);
apply();
