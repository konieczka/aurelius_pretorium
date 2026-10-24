import { closeUsage } from "./usageView.svelte";
import { closeSkills } from "./skillsView.svelte";
import { closeAgents } from "./agentsView.svelte";
import { closeSettings } from "./settingsView.svelte";
import { closeMcp } from "./mcpView.svelte";
import type { WarView } from "./war";

// The War Table stands in for this window's panes while open, taking turns with Usage, Skills, MCP, Agents and
// Settings. Open per window, not saved; the layout picked (Castra or Front line) is remembered.
const KEY = "war:view";
const saved = (() => {
  try {
    return localStorage.getItem(KEY);
  } catch {
    return null;
  }
})();
export const war = $state({ open: false, view: (saved === "front" ? "front" : "castra") as WarView });

export function toggleWar() {
  war.open = !war.open;
  if (war.open) (closeUsage(), closeSkills(), closeAgents(), closeSettings(), closeMcp());
}
export const closeWar = () => (war.open = false);

export function setWarView(v: WarView) {
  war.view = v;
  try {
    localStorage.setItem(KEY, v);
  } catch {}
}
