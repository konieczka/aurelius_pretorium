import { invoke } from "@tauri-apps/api/core";
import { message } from "@tauri-apps/plugin-dialog";
import type { ConfigFile, Scan } from "./agentConfig";
import type { Provider } from "./usageStats";
import { closeUsage } from "./usageView.svelte";
import { closeSkills } from "./skillsView.svelte";
import { closeSettings } from "./settingsView.svelte";
import { closeMcp } from "./mcpView.svelte";
import { closeWar } from "./warView.svelte";

// The Agents view stands in for this window's panes while open, like Usage and Skills (they take turns). Per window,
// not saved.
export const agents = $state({
  open: false,
  scan: null as Scan | null,
  error: "",
  project: "", // folder of the last scan
  selected: "claude" as Provider,
  tab: "settings" as "settings" | "files",
  target: {} as Partial<Record<Provider, string>>, // settings form: the file it writes, per agent
  file: {} as Partial<Record<Provider, string>>, // files tab: the file shown, per agent
  drafts: {} as Record<string, string>, // path -> unsaved editor text (kept while the view is closed)
  saving: "", // path being written
  formError: "",
});

export function toggleAgents() {
  agents.open = !agents.open;
  if (agents.open) (closeUsage(), closeSkills(), closeSettings(), closeMcp(), closeWar());
}
export const closeAgents = () => (agents.open = false);

/** Reread every agent's config files for `project` (the sidebar's folder) and home. */
export async function loadAgents(project: string) {
  agents.project = project;
  try {
    agents.scan = await invoke<Scan>("agent_configs", { project });
    agents.error = "";
  } catch (e) {
    agents.error = String(e);
  }
}

export const fileAt = (path: string | undefined) => agents.scan?.agents.flatMap((a) => a.files).find((f) => f.path === path);

/** Set or unset (undefined) one setting in `f`, written over what was last read of it. */
export async function setValue(f: ConfigFile, key: string[], value: unknown) {
  agents.saving = f.path;
  agents.formError = "";
  try {
    f.text = await invoke<string>("agent_config_set", { project: agents.project, path: f.path, base: f.text, edits: [{ key, value: value ?? null }] });
  } catch (e) {
    agents.formError = String(e);
  }
  agents.saving = "";
  await loadAgents(agents.project);
}

/** ⌘S on the Files tab: write the shown file's edits. */
export async function saveAgentFile() {
  if (agents.tab !== "files") return;
  const f = fileAt(agents.file[agents.selected]);
  const text = f && agents.drafts[f.path];
  if (!f || text === undefined || f.readOnly) return;
  agents.saving = f.path;
  try {
    await invoke("agent_config_save", { project: agents.project, path: f.path, base: f.text, content: text });
    delete agents.drafts[f.path];
  } catch (e) {
    message(String(e), { kind: "error" });
  }
  agents.saving = "";
  await loadAgents(agents.project);
}
