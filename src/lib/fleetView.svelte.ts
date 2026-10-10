import { closeUsage } from "./usageView.svelte";
import { closeSkills } from "./skillsView.svelte";
import { closeAgents } from "./agentsView.svelte";
import { closeSettings } from "./settingsView.svelte";

// The Fleet view (every agent, in every workspace) stands in for this window's panes while open, like Usage, Skills and
// Agents (they take turns). Per window, not saved.
export const fleet = $state({ open: false });

export function toggleFleet() {
  fleet.open = !fleet.open;
  if (fleet.open) (closeUsage(), closeSkills(), closeAgents(), closeSettings());
}
export const closeFleet = () => (fleet.open = false);
