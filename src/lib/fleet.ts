// Fleet view math: every workspace's terminal tabs + every shell's state -> one row per agent.
import type { Tab } from "./layout";
import type { Shell } from "./agents.svelte";

/** attention = it asks you something, or rang or finished out of sight; asleep = its workspace hasn't been shown since
 * launch, so no shell yet (showing it starts the agent again). */
export type Status = "attention" | "busy" | "idle" | "asleep";
export const STATUSES: Status[] = ["attention", "busy", "idle", "asleep"];

export type Row = {
  id: string; // tab id = shell id
  ws: string | null; // null: a shell no layout has (its tab was lost)
  tab: (Tab & { kind: "term" }) | null;
  agent: string;
  status: Status;
  asking: Shell["asking"];
  label: string; // the tab's name, or what the agent put in the terminal title ("" = neither)
  cwd: string;
  since: number | null; // ms in this status, as of the last poll
};

/** A title without the agent's status glyph in front, its folder (the rows are grouped by project, and show it
 * otherwise) or its own command (the row names the agent): "⠐ Fix the bug" -> "Fix the bug", "Fix the bug | app" ->
 * "Fix the bug", "- Responding - grok" -> "Responding". Codex's and Muse's are just its folder, Grok's idle one just "grok": "". */
export function cleanTitle(t: string, cwd = "", agent = "") {
  let c = t.replace(/^[^\p{L}\p{N}]+/u, "").trim();
  for (const name of [cwd.slice(cwd.lastIndexOf("/") + 1), agent]) {
    if (!name) continue;
    if (c === name) return "";
    const end = c.lastIndexOf(name);
    if (end > 0 && end + name.length === c.length && /\s[|·•—–:-]\s*$/.test(c.slice(0, end))) c = c.slice(0, end).replace(/\s*[|·•—–:-]\s*$/, "");
  }
  return c;
}

export function fleetRows(workspaces: { id: string; tabs: Tab[] }[], shells: Record<string, Shell>): Row[] {
  const rows: Row[] = [];
  const placed = new Set<string>();
  for (const w of workspaces)
    for (const t of w.tabs) {
      if (t.kind !== "term" || placed.has(t.id)) continue;
      placed.add(t.id);
      const s = shells[t.id];
      const agent = s ? s.agent : t.agent;
      if (!agent) continue;
      rows.push({ id: t.id, ws: w.id, tab: t, agent, status: s ? statusOf(s) : "asleep", asking: s?.asking ?? null, label: t.name || cleanTitle(s?.title ?? "", s?.cwd || t.cwd, agent), cwd: s?.cwd || t.cwd, since: s ? s.since : null });
    }
  for (const [id, s] of Object.entries(shells))
    if (s.agent && !placed.has(id)) rows.push({ id, ws: null, tab: null, agent: s.agent, status: statusOf(s), asking: s.asking, label: cleanTitle(s.title, s.cwd, s.agent), cwd: s.cwd, since: s.since });
  // Most urgent first; within a status, the latest change first (asleep ones keep workspace order).
  const rank = (r: Row) => STATUSES.indexOf(r.status);
  return rows.sort((a, b) => rank(a) - rank(b) || (a.since ?? Infinity) - (b.since ?? Infinity));
}

export type Group = { key: string; rows: Row[] };

/** Rows by project (`projectOf` -> its folder), keeping their order: the project with the most urgent agent comes first. */
export function byProject(rows: Row[], projectOf: (r: Row) => string): Group[] {
  const groups = new Map<string, Row[]>();
  for (const r of rows) {
    const k = projectOf(r);
    groups.set(k, [...(groups.get(k) ?? []), r]);
  }
  return [...groups].map(([key, rows]) => ({ key, rows }));
}

// A prompt that's up outranks the spinner: some agents keep it going while they wait for an answer.
const statusOf = (s: Shell): Status => (s.asking ? "attention" : s.busy ? "busy" : s.attention ? "attention" : "idle");

/** 42s, 5m, 3h, 2d */
export function fmtSpan(ms: number) {
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86400)}d`;
}
