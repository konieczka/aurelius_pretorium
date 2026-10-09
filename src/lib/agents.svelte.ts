import { invoke } from "@tauri-apps/api/core";
import * as L from "./layout";
import { app } from "./state.svelte";
import type { Provider } from "./usageStats";

// Coding agents running in terminals. A term tab's `agent` is the agent its shell last had in the foreground; it's
// saved with the layout, so a shell started fresh for the tab (app relaunch, workspace reopened) starts it again.

/** Agent command (what the backend reports) -> its provider, for the tab's name and icon. */
export const agentProvider: Record<string, Provider> = { claude: "claude", codex: "codex", agy: "antigravity", grok: "grok", opencode: "opencode" };

// Tabs whose agent was just typed into a fresh shell: until it shows up (login shells can take a while) or this
// deadline passes, "no agent" doesn't clear the tab's.
const starting = new Map<string, number>();

/** Command to type into a fresh shell for tab `t`, if it had an agent running. */
export function relaunchCommand(t: L.Tab) {
  if (t.kind !== "term" || !t.agent) return null;
  starting.set(t.id, Date.now() + 20_000);
  return t.agent;
}

async function poll() {
  let found: Record<string, string | null>;
  try {
    found = await invoke("pty_agents");
  } catch {
    return;
  }
  for (const t of L.panes(app.root).flatMap((p) => p.tabs)) {
    if (t.kind !== "term" || !(t.id in found)) continue; // no shell yet (tab never shown): keep what was saved
    const agent = found[t.id] ?? undefined;
    if (agent) starting.delete(t.id);
    else if ((starting.get(t.id) ?? 0) > Date.now()) continue;
    if (t.agent !== agent) t.agent = agent; // only on change: every write re-saves the layout
  }
}

export function watchAgents() {
  poll();
  const t = setInterval(poll, 2000);
  return () => clearInterval(t);
}
