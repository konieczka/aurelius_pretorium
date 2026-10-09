import { invoke } from "@tauri-apps/api/core";
import { message } from "@tauri-apps/plugin-dialog";
import type { Scan, Scope } from "./skills";
import type { Provider } from "./usageStats";
import { closeUsage } from "./usageView.svelte";
import { closeAgents } from "./agentsView.svelte";
import { closeFleet } from "./fleetView.svelte";

// The Skills view stands in for this window's panes while open, like Usage, Agents and Fleet (they take turns). Per window, not saved.
export const skills = $state({
  open: false,
  scan: null as Scan | null,
  error: "",
  selected: "", // skill's real folder
  query: "",
  agent: null as Provider | null, // filter: skills this agent sees
  creating: null as null | { scope: Scope; dir: string | null }, // add form; dir null = all agents
  project: "", // folder of the last scan
  // The selected skill's SKILL.md, shown inline: rendered, or in an editor.
  current: null as null | { file: string; readOnly: boolean },
  mode: "preview" as "preview" | "edit",
  saved: {} as Record<string, string>, // SKILL.md path -> text on disk
  drafts: {} as Record<string, string>, // SKILL.md path -> unsaved edits (kept while the view is closed)
});

export function toggleSkills() {
  skills.open = !skills.open;
  if (skills.open) (closeUsage(), closeAgents(), closeFleet());
}
export const closeSkills = () => (skills.open = false);

/** Rescan the skills folders of `project` (the sidebar's folder) and home. */
export async function loadSkills(project: string) {
  skills.project = project;
  try {
    skills.scan = await invoke<Scan>("skills_scan", { project });
    skills.error = "";
  } catch (e) {
    skills.error = String(e);
  }
}

/** (Re)read a SKILL.md; its unsaved edits, if any, stay on top. */
export async function loadDoc(file: string) {
  try {
    const text = await invoke<string>("read_file", { path: file });
    if (skills.saved[file] !== text) skills.saved[file] = text;
  } catch (e) {
    skills.saved[file] = `Can't read ${file}: ${e}`;
  }
}

/** ⌘S: write the shown SKILL.md's edits. */
export async function saveSkill() {
  const file = skills.current?.file;
  const text = file && skills.drafts[file];
  if (!file || text === undefined || skills.current?.readOnly) return;
  try {
    await invoke("write_file", { path: file, content: text });
  } catch (e) {
    return message(String(e), { kind: "error" });
  }
  skills.saved[file] = text;
  delete skills.drafts[file];
  loadSkills(skills.project); // name, description and problems come from the frontmatter
}

/** ⌘E: rendered SKILL.md or its editor. */
export function toggleSkillMode() {
  skills.mode = skills.mode === "edit" || skills.current?.readOnly ? "preview" : "edit";
}
