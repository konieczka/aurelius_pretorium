import { closeUsage } from "./usageView.svelte";
import { closeSkills } from "./skillsView.svelte";
import { closeAgents } from "./agentsView.svelte";

// The Settings view stands in for this window's panes while open, taking turns with Usage, Skills and Agents.
// Per window, not saved.
export const settings = $state({ open: false });

export function toggleSettings() {
  settings.open = !settings.open;
  if (settings.open) (closeUsage(), closeSkills(), closeAgents());
}
export const closeSettings = () => (settings.open = false);
