import { closeUsage } from "./usageView.svelte";
import { closeSkills } from "./skillsView.svelte";
import { closeAgents } from "./agentsView.svelte";
import { closeSettings } from "./settingsView.svelte";
import { closeMcp } from "./mcpView.svelte";
import type { WarView } from "./war";

// The War Table stands in for this window's panes while open, taking turns with Usage, Skills, MCP, Agents and
// Settings. Open per window, not saved; the layout picked (Castra or Front line) and the Front line's columns, if
// arranged by hand, are remembered.
const KEY = "war:view", ORDER = "war:order";
const read = (k: string) => {
  try {
    return localStorage.getItem(k);
  } catch {
    return null;
  }
};
const write = (k: string, v: string | null) => {
  try {
    if (v === null) localStorage.removeItem(k);
    else localStorage.setItem(k, v);
  } catch {}
};
const savedOrder = (() => {
  try {
    const o = JSON.parse(read(ORDER) ?? "[]");
    return Array.isArray(o) ? o.filter((k): k is string => typeof k === "string") : [];
  } catch {
    return [];
  }
})();
export const war = $state({ open: false, view: (read(KEY) === "front" ? "front" : "castra") as WarView, order: savedOrder });

export function toggleWar() {
  war.open = !war.open;
  if (war.open) (closeUsage(), closeSkills(), closeAgents(), closeSettings(), closeMcp());
}
export const closeWar = () => (war.open = false);

export function setWarView(v: WarView) {
  war.view = v;
  write(KEY, v);
}

/** The Front line's columns by project, left to right; [] goes back to the workspaces' order. */
export function setWarOrder(keys: string[]) {
  war.order = keys;
  write(ORDER, keys.length ? JSON.stringify(keys) : null);
}
