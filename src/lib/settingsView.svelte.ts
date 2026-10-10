import { closeUsage } from "./usageView.svelte";
import { closeSkills } from "./skillsView.svelte";
import { closeAgents } from "./agentsView.svelte";
import { closeFleet } from "./fleetView.svelte";

// The Settings view stands in for this window's panes while open, taking turns with Usage, Skills, Agents and Fleet.
// Per window, not saved.
export const settings = $state({ open: false });

export function toggleSettings() {
  settings.open = !settings.open;
  if (settings.open) (closeUsage(), closeSkills(), closeAgents(), closeFleet());
}
export const closeSettings = () => (settings.open = false);
