import { invoke } from "@tauri-apps/api/core";
import * as L from "./layout";
import { allTabs, app } from "./state.svelte";
import type { Provider } from "./usageStats";
import { turnDone, type Turn } from "./agentTurn";

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

/** What to type into an agent: "ready" once it's up and its screen has settled, "done" once it has worked and
 * stopped (finished the turn the step before started). `keys` go in one chunk at a time, apart: a burst reads as a paste. */
export type Step = { when: "ready" | "done"; keys: string[] };
type Script = { agent: string; steps: Step[]; from: number; up: number | null; turn: Turn; typing: boolean };

// Tabs started with something to type into their agent (a review, then its fixes), until it's all typed or the agent
// quits (or never shows up).
const scripts = new Map<string, Script>();

/** Type `steps` into tab `tabId`'s agent (`agent`: its command, as the backend reports it) as it gets to each. */
export function typeIntoAgent(tabId: string, agent: string, steps: Step[]) {
  if (steps.length) scripts.set(tabId, { agent, steps, from: Date.now(), up: null, turn: { requested: false, started: null }, typing: false });
}

async function typeKeys(id: string, keys: string[]) {
  for (const [i, k] of keys.entries()) {
    if (i) await new Promise((r) => setTimeout(r, 500));
    await invoke("pty_write", { id, data: k }).catch(() => {});
  }
}

function runScripts(found: Record<string, Shell>, now: number) {
  for (const [id, sc] of scripts) {
    const s = found[id];
    if (s?.agent !== sc.agent) {
      if (sc.up !== null || now - sc.from > 60_000) scripts.delete(id); // it quit, or never started
      continue;
    }
    sc.up ??= now;
    if (sc.typing) continue;
    const step = sc.steps[0];
    let go = false;
    // Ready: nothing drawn for a while (Codex has nothing to draw when idle), or up long enough that it must be.
    if (step.when === "ready") go = !s.busy && !s.asking && ((s.quiet ?? 0) >= 1500 || now - sc.up > 10_000);
    else go = turnDone(sc.turn, s, now);
    if (!go) continue;
    sc.steps.shift();
    sc.turn = { requested: true, started: null };
    if (!sc.steps.length) scripts.delete(id);
    sc.typing = true;
    void typeKeys(id, step.keys).finally(() => { sc.typing = false; });
  }
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
  runScripts(found, now);
}

export function watchAgents() {
  poll();
  const t = setInterval(poll, 1000);
  return () => clearInterval(t);
}
