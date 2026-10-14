import { getCurrentWindow } from "@tauri-apps/api/window";
import { isPalette, PALETTE_COLOR_PROPERTIES, PALETTE_TRANSITION_MS, resolvePalette, shouldAnimatePaletteChange, type Palette } from "./palette";

export type Mode = "light" | "dark" | "system";

export { PALETTES, type Palette } from "./palette";

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
  /** The global palette (localStorage "palette"): the fallback for workspaces without their own. */
  globalPalette: resolvePalette(null, storedPalette),
  /** The palette actually shown in this window: the active workspace's own, else `globalPalette`. */
  palette: resolvePalette(document.documentElement.dataset.palette, storedPalette),
  dark: false,
  transitioning: false,
});

// This window's active workspace override (workspaces.svelte.ts sets it on activate); null = use the global.
let workspaceOverride: Palette | null = null;
// The first palette a window shows is its launch palette: it must not fade in from the global default.
let sawWorkspacePalette = false;
let animFrame = 0;

const reducedMotion = () => {
  try {
    return matchMedia("(prefers-reduced-motion: reduce)").matches;
  } catch {
    return false;
  }
};

function finishPaletteTransition() {
  cancelAnimationFrame(animFrame);
  const root = document.documentElement;
  for (const name of PALETTE_COLOR_PROPERTIES) root.style.removeProperty(name);
  root.style.removeProperty("--shadow-tint");
  theme.transitioning = false;
}

/** Drive the inherited colours each frame: WebKit's custom-property CSS transitions
 * did not interpolate in the full workspace view, even though they were running. */
function beginPaletteTransition(next: Palette, first: boolean) {
  const root = document.documentElement;
  if (first || reducedMotion()) {
    finishPaletteTransition();
    return;
  }
  if (!shouldAnimatePaletteChange(theme.palette, next)) return;
  cancelAnimationFrame(animFrame);
  // A scoped swatch resolves the target even while the root still shows an earlier
  // frame. This also lets a rapid workspace switch continue from its visible colours.
  const target = document.createElement("div");
  target.dataset.palette = next;
  target.hidden = true;
  root.appendChild(target);
  const channels = (hex: string) => [1, 3, 5, 7].map((i) => parseInt(hex.slice(i, i + 2) || "ff", 16));
  const colors = PALETTE_COLOR_PROPERTIES.map((name) => ({ name, from: channels(cssHex(name)), to: channels(cssHex(name, target)) }));
  const shadowFrom = getComputedStyle(root).getPropertyValue("--shadow-tint").trim().split(/\s+/).map(Number);
  const shadowTo = getComputedStyle(target).getPropertyValue("--shadow-tint").trim().split(/\s+/).map(Number);
  target.remove();
  const paint = (progress: number) => {
    const mix = (from: number, to: number) => from + (to - from) * progress;
    for (const { name, from, to } of colors) {
      const [r, g, b, a] = from.map((v, i) => mix(v, to[i]));
      root.style.setProperty(name, `rgb(${r} ${g} ${b} / ${a / 255})`);
    }
    root.style.setProperty("--shadow-tint", shadowFrom.map((v, i) => mix(v, shadowTo[i])).join(" "));
  };
  paint(0); // hold the previous colours while apply() changes the palette attribute
  theme.transitioning = true;
  const started = performance.now();
  const tick = (now: number) => {
    const progress = Math.min((now - started) / PALETTE_TRANSITION_MS, 1);
    if (progress >= 1 || reducedMotion()) return finishPaletteTransition();
    paint(progress * progress * (3 - 2 * progress));
    animFrame = requestAnimationFrame(tick);
  };
  animFrame = requestAnimationFrame(tick);
}

function recompute(first = false) {
  const next = resolvePalette(workspaceOverride, theme.globalPalette);
  beginPaletteTransition(next, first);
  theme.palette = next;
  apply();
}

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
  finishPaletteTransition();
  theme.mode = mode;
  write("theme", mode);
  apply();
}

/** Set the global palette. A workspace with its own palette keeps it in its window; this stays its fallback. */
export function setPalette(palette: Palette) {
  theme.globalPalette = palette;
  write("palette", palette);
  recompute();
}

/** Show `override` in this window (the active workspace's own palette), or the global palette when null.
 * Switching workspace cross-fades to it, except the first workspace a window shows. */
export function applyWorkspacePalette(override: Palette | null | undefined) {
  workspaceOverride = isPalette(override) ? override : null;
  const first = !sawWorkspacePalette;
  sawWorkspacePalette = true;
  recompute(first);
}

// Other windows share localStorage: follow their changes
window.addEventListener("storage", (e) => {
  if (e.key === "theme") {
    finishPaletteTransition();
    theme.mode = e.newValue === "light" || e.newValue === "dark" ? e.newValue : "system";
    apply();
  } else if (e.key === "palette") {
    theme.globalPalette = resolvePalette(null, e.newValue);
    recompute();
  }
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

media.addEventListener("change", () => {
  finishPaletteTransition();
  apply();
});
apply();
