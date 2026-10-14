import { invoke } from "@tauri-apps/api/core";
import type { Place, Scan, Scope } from "./mcp";
import type { Provider } from "./usageStats";
import { closeUsage } from "./usageView.svelte";
import { closeSkills } from "./skillsView.svelte";
import { closeAgents } from "./agentsView.svelte";
import { closeSettings } from "./settingsView.svelte";

// The MCP view stands in for this window's panes while open, like Skills, Usage and Agents (they take turns). Per window, not saved.
export const mcp = $state({
  open: false,
  scan: null as Scan | null,
  error: "",
  selected: "", // server key: "<scope>:<name>"
  query: "",
  agent: null as Provider | null, // filter: servers this agent reads
  // The form: a new server for a scope, or one definition being edited.
  // Edit keeps the fingerprint of what it was filled from, so a save over a change made since is refused.
  form: null as null | { kind: "new"; scope: Scope } | { kind: "edit"; place: Place; fingerprint: string },
  project: "", // folder of the last scan
  revealed: {} as Record<string, string>, // revealKey -> literal value the user asked to see
});

export function toggleMcp() {
  mcp.open = !mcp.open;
  if (mcp.open) (closeUsage(), closeSkills(), closeAgents(), closeSettings());
  else closeMcp();
}
export const closeMcp = () => {
  mcp.open = false;
  mcp.revealed = {}; // literal values don't outlive the view
  mcp.form = null; // its fields live in the page, which goes when the view closes
};

/** Rescan the MCP files of `project` (the sidebar's folder) and home. */
export async function loadMcp(project: string) {
  if (project !== mcp.project) (mcp.revealed = {}), (mcp.form = null);
  mcp.project = project;
  try {
    mcp.scan = await invoke<Scan>("mcp_scan", { project });
    mcp.error = "";
  } catch (e) {
    mcp.error = String(e);
  }
}

export const revealKey = (p: Place, field: "env" | "headers", key: string) => [p.path, p.table.join("."), p.name, field, key].join("\0");
