import { closeUsage } from "./usageView.svelte";
import { closeSkills } from "./skillsView.svelte";
import { closeAgents } from "./agentsView.svelte";
import { closeMcp } from "./mcpView.svelte";

// The Settings view stands in for this window's panes while open, taking turns with Usage, Skills and Agents.
// Per window, not saved.
export const settings = $state({ open: false });

export function toggleSettings() {
  settings.open = !settings.open;
  if (settings.open) (closeUsage(), closeSkills(), closeAgents(), closeMcp());
}
export const closeSettings = () => (settings.open = false);
