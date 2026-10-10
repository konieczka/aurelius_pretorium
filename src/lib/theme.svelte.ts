import { getCurrentWindow } from "@tauri-apps/api/window";

export type Mode = "light" | "dark" | "system";

/** Colour palettes (Settings). Aurelius is the default: app.css :root / .dark; the rest are blocks in palettes.css. */
export const PALETTES = [
  { id: "aurelius", name: "Aurelius", blurb: "The house gold on warm parchment" },
  { id: "mare", name: "Mare Nostrum", blurb: "Lapis sea under lamp light" },
  { id: "tyrian", name: "Tyrian", blurb: "Imperial murex purple" },
  { id: "verdigris", name: "Verdigris", blurb: "Weathered bronze, copper accents" },
  { id: "marmor", name: "Marmor", blurb: "Cool marble, a lapis accent" },
  { id: "vesuvius", name: "Vesuvius", blurb: "Volcanic ash and lava" },
] as const;
export type Palette = (typeof PALETTES)[number]["id"];

const media = matchMedia("(prefers-color-scheme: dark)");
const read = (k: string) => {
  try {
    return localStorage.getItem(k);
  } catch {
    return null;
  }
};
const write = (k: string, v: string) => {
  try {
    localStorage.setItem(k, v);
  } catch {}
};
const storedMode = read("theme");
const storedPalette = read("palette");

export const theme = $state({
  mode: (["light", "dark"].includes(storedMode!) ? storedMode : "system") as Mode,
  palette: (PALETTES.some((p) => p.id === storedPalette) ? storedPalette : "aurelius") as Palette,
  dark: false,
});

function apply() {
  theme.dark = theme.mode === "dark" || (theme.mode === "system" && media.matches);
  const root = document.documentElement;
  root.classList.toggle("dark", theme.dark);
  if (theme.palette === "aurelius") root.removeAttribute("data-palette");
  else root.dataset.palette = theme.palette;
  // Native title bar follows too (null = follow OS). Outside Tauri (browser dev) this just fails.
  getCurrentWindow().setTheme(theme.mode === "system" ? null : theme.mode).catch(() => {});
}

export function setMode(mode: Mode) {
  theme.mode = mode;
  write("theme", mode);
  apply();
}

export function setPalette(palette: Palette) {
  theme.palette = palette;
  write("palette", palette);
  apply();
}

// Other windows share localStorage: follow their changes
window.addEventListener("storage", (e) => {
  if (e.key === "theme" && e.newValue) theme.mode = e.newValue as Mode;
  else if (e.key === "palette" && e.newValue) theme.palette = e.newValue as Palette;
  else return;
  apply();
});

const probe = document.createElement("canvas").getContext("2d", { willReadFrequently: true })!;
probe.canvas.width = probe.canvas.height = 1;
/** A CSS colour variable as #rrggbb[aa], for APIs that take hex only (xterm, native window background). Reads the
 * current theme, so call it after apply() (in an effect on theme.dark / theme.palette). */
export function cssHex(name: string, el: Element = document.documentElement) {
  probe.clearRect(0, 0, 1, 1);
  probe.fillStyle = "#000";
  probe.fillStyle = getComputedStyle(el).getPropertyValue(name).trim() || "#000";
  probe.fillRect(0, 0, 1, 1);
  const [r, g, b, a] = probe.getImageData(0, 0, 1, 1).data;
  const hex = (n: number) => n.toString(16).padStart(2, "0");
  return "#" + hex(r) + hex(g) + hex(b) + (a < 255 ? hex(a) : "");
}

media.addEventListener("change", apply);
apply();
