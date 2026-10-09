import { invoke } from "@tauri-apps/api/core";
import * as L from "./layout";
import { app } from "./state.svelte";
import type { Provider } from "./usageStats";
import { agentBusy, TITLED } from "./agentBusy";

// Coding agents running in terminals. A term tab's `agent` is the agent its shell last had in the foreground; it's
// saved with the layout, so a shell started fresh for the tab (app relaunch, workspace reopened) starts it again.

/** Agent command (what the backend reports) -> its provider, for the tab's name and icon. */
export const agentProvider: Record<string, Provider> = { claude: "claude", codex: "codex", agy: "antigravity", grok: "grok", opencode: "opencode" };

const SETTLED = 3000; // output-judged agents: shorter bursts (startup banner) aren't a finished turn

/** Transient per-tab state: busy = the agent is working; attention = it rang, or finished while you weren't looking. */
export const activity = $state<Record<string, "busy" | "attention">>({});
const busySince = new Map<string, number>();

// Tabs whose agent was just typed into a fresh shell: until it shows up (login shells can take a while) or this
// deadline passes, "no agent" doesn't clear the tab's.
const starting = new Map<string, number>();

/** Command to type into a fresh shell for tab `t`, if it had an agent running. */
export function relaunchCommand(t: L.Tab) {
  if (t.kind !== "term" || !t.agent) return null;
  starting.set(t.id, Date.now() + 20_000);
  return t.agent;
}

/** Is the tab on screen: the active tab of some pane, in a focused window. */
function inView(id: string) {
  return document.hasFocus() && L.panes(app.root).some((p) => p.tabs[p.active]?.id === id);
}

/** The terminal rang its bell or posted a notification. */
export function ring(id: string) {
  const f = L.findTab(app.root, id);
  const t = f?.pane.tabs[f.index];
  if (t?.kind === "term" && t.agent && !inView(id)) activity[id] = "attention";
}

type Shell = { agent: string | null; quiet: number | null };

async function poll() {
  let found: Record<string, Shell>;
  try {
    found = await invoke("pty_status");
  } catch {
    return;
  }
  const now = Date.now();
  for (const t of L.panes(app.root).flatMap((p) => p.tabs)) {
    if (t.kind !== "term" || !(t.id in found)) continue; // no shell yet (tab never shown): keep what was saved
    const s = found[t.id];
    const agent = s.agent ?? undefined;
    if (agent) starting.delete(t.id);
    else if ((starting.get(t.id) ?? 0) > now) continue;
    if (t.agent !== agent) t.agent = agent; // only on change: every write re-saves the layout

    const busy = !!agent && agentBusy(agent, app.titles[t.id] ?? "", s.quiet);
    if (busy) {
      if (!busySince.has(t.id)) busySince.set(t.id, now);
      activity[t.id] = "busy";
      continue;
    }
    const since = busySince.get(t.id);
    busySince.delete(t.id);
    const finished = activity[t.id] === "busy" && !!agent && since !== undefined && (TITLED.has(agent) || now - since >= SETTLED);
    if (finished && !inView(t.id)) activity[t.id] = "attention";
    else if (activity[t.id] === "busy" || !agent || inView(t.id)) delete activity[t.id];
  }
  for (const id of Object.keys(activity)) if (!(id in found)) delete activity[id]; // shell gone
}

export function watchAgents() {
  poll();
  const t = setInterval(poll, 1000);
  return () => clearInterval(t);
}
