import { invoke } from "@tauri-apps/api/core";
import * as L from "./layout";
import { allTabs, app } from "./state.svelte";
import type { Provider } from "./usageStats";

// Coding agents running in terminals. A term tab's `agent` is the agent its shell last had in the foreground; it's
// saved with the layout, so a shell started fresh for the tab (app relaunch, workspace reopened) starts it again.
// Its `cwd` likewise follows the shell's working directory, or its agent's (which may move into another worktree).

/** Agent command (what the backend reports) -> its provider, for the tab's name and icon. */
export const agentProvider: Record<string, Provider> = { claude: "claude", codex: "codex", agy: "antigravity", grok: "grok", opencode: "opencode" };

/** Every shell's state, by tab id, from the backend: it judges busy / attention for all of them, whichever window or
 * workspace (if any) shows the tab. */
export type Shell = {
  agent: string | null;
  quiet: number | null;
  title: string;
  cwd: string; // the shell's working directory now
  busy: boolean;
  attention: boolean; // it rang, or finished, while no window had it on screen
  asking: "question" | "approval" | null; // a prompt is up that hasn't been typed into
  since: number; // ms since the agent came or went, or started or stopped working (as of `polled`)
};
export const shells = $state({ all: {} as Record<string, Shell>, polled: 0 });

/** busy = the agent is working; attention = it rang, or finished while you weren't looking. */
export const activity = $state<Record<string, "busy" | "attention">>({});

// Tabs whose agent was just typed into a fresh shell: until it shows up (login shells can take a while) or this
// deadline passes, "no agent" doesn't clear the tab's.
const starting = new Map<string, number>();

/** Command to type into a fresh shell for tab `t`, if it had an agent running. */
export function relaunchCommand(t: L.Tab) {
  if (t.kind !== "term" || !t.agent) return null;
  starting.set(t.id, Date.now() + 20_000);
  return t.agent;
}

/** Tabs on screen here: the active tab of each pane, if this window has focus. */
const inView = () => (document.hasFocus() ? L.panes(app.root).flatMap((p) => p.tabs[p.active]?.id ?? []) : []);

async function poll() {
  let found: Record<string, Shell>;
  try {
    found = await invoke("pty_status", { seen: inView() });
  } catch {
    return;
  }
  const now = Date.now();
  shells.all = found;
  shells.polled = now;
  // Only this workspace's tabs: their agent is saved with the layout (to start it again in a fresh shell).
  for (const t of allTabs()) {
    if (t.kind !== "term" || !(t.id in found)) continue; // no shell yet (tab never shown): keep what was saved
    // Follow `cd`: the header's project / branch go by it, and a fresh shell for the tab starts where this one was.
    if (found[t.id].cwd && t.cwd !== found[t.id].cwd) t.cwd = found[t.id].cwd;
    const agent = found[t.id].agent ?? undefined;
    if (agent) starting.delete(t.id);
    else if ((starting.get(t.id) ?? 0) > now) continue;
    if (t.agent !== agent) t.agent = agent; // only on change: every write re-saves the layout
  }
  for (const [id, s] of Object.entries(found)) {
    const a = s.busy ? "busy" : s.attention ? "attention" : undefined;
    if (activity[id] !== a) a ? (activity[id] = a) : delete activity[id];
  }
  for (const id of Object.keys(activity)) if (!(id in found)) delete activity[id]; // shell gone
}

export function watchAgents() {
  poll();
  const t = setInterval(poll, 1000);
  return () => clearInterval(t);
}
