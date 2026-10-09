import { invoke } from "@tauri-apps/api/core";
import type { Scan, Scope } from "./skills";
import type { Provider } from "./usageStats";
import { closeUsage } from "./usageView.svelte";

// The Skills view stands in for this window's panes while open, like Usage (the two take turns). Per window, not saved.
export const skills = $state({
  open: false,
  scan: null as Scan | null,
  error: "",
  selected: "", // skill's real folder
  query: "",
  agent: null as Provider | null, // filter: skills this agent sees
  creating: null as null | { scope: Scope; dir: string | null }, // add form; dir null = all agents
});

export function toggleSkills() {
  skills.open = !skills.open;
  if (skills.open) closeUsage();
}
export const closeSkills = () => (skills.open = false);

/** Rescan the skills folders of `project` (the sidebar's folder) and home. */
export async function loadSkills(project: string) {
  try {
    skills.scan = await invoke<Scan>("skills_scan", { project });
    skills.error = "";
  } catch (e) {
    skills.error = String(e);
  }
}
