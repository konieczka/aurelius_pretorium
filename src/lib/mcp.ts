// MCP view: what `mcp_scan` returns, and the comparisons the page makes over it.

import { PROVIDERS, type Provider } from "./usageStats";

export type Scope = "project" | "local" | "global" | "managed";
export type Dialect = "claude" | "codex" | "grok" | "opencode" | "antigravity" | "muse";
export type Transport = "stdio" | "http" | "sse" | "ws";
/** An env or header value; null for a literal, hidden until revealed. */
export type Pair = { key: string; value: string | null };
export type Shown = { transport: Transport; command: string; args: string[]; env: Pair[]; cwd: string; url: string; headers: Pair[] };
/** An agent that reads a definition but won't use it; `by`: the file whose definition it uses instead. */
export type Block = { agent: Provider; reason: string; by: string | null };
/** One server in one file. */
export type Entry = {
  name: string;
  path: string;
  table: string[];
  scope: Scope;
  dialect: Dialect;
  label: string | null;
  agents: Provider[];
  blocked: Block[];
  spec: Shown;
  enabled: boolean;
  extra: string[];
  problems: string[];
  fingerprint: string;
  readOnly: boolean;
};
/** A file agents read servers from. */
export type Source = { path: string; table: string[]; scope: Scope; dialect: Dialect; label: string | null; agents: Provider[]; exists: boolean; readOnly: boolean; error: string | null };
export type Target = { path: string; table: string[] };
export type Place = Target & { name: string };
export type Scan = { entries: Entry[]; sources: Source[]; presets: { scope: Scope; targets: Target[] }[]; home: string };
export type Report = { path: string; table: string[]; lost: string[]; warnings: string[]; error: string | null };
/** A server as the page lists it: the definitions of one name in one scope. */
export type Server = { key: string; name: string; scope: Scope; places: Entry[] };

/** Same rule as the backend's (Grok's, the strictest): a letter or _, then letters, digits, - and _. */
export const validName = (s: string) => /^[A-Za-z_][A-Za-z0-9_-]*$/.test(s) && !s.endsWith("_") && s.length <= 64;

export const sameFile = (a: Target, b: Target) => a.path === b.path && a.table.join("\0") === b.table.join("\0");
export const placeOf = (e: Entry): Place => ({ path: e.path, table: e.table, name: e.name });

const byOrder = (a: Iterable<Provider>) => {
  const set = new Set(a);
  return PROVIDERS.filter((p) => set.has(p));
};

export function group(entries: Entry[]): Server[] {
  const out = new Map<string, Server>();
  for (const e of entries) {
    const key = `${e.scope}:${e.name}`;
    const s = out.get(key) ?? { key, name: e.name, scope: e.scope, places: [] };
    s.places.push(e);
    out.set(key, s);
  }
  return [...out.values()];
}

/** Agents reading any definition of `s`. */
export const readers = (s: Server) => byOrder(s.places.flatMap((e) => e.agents));

/** Agents that load `s`: they read a definition of it and nothing stops them. */
export const users = (s: Server) => byOrder(s.places.flatMap((e) => e.agents.filter((a) => !e.blocked.some((b) => b.agent === a))));

/** Why `agent` reads `s` but doesn't load it. */
export const blocksFor = (s: Server, agent: Provider) => s.places.flatMap((e) => e.blocked.filter((b) => b.agent === agent));

/** Its copies don't all say the same (literal values included). */
export const drifted = (s: Server) => new Set(s.places.map((e) => e.fingerprint)).size > 1;

/** Servers of the same name in another scope that some of the same agents read: those agents use one of them. */
export function clashes(s: Server, all: Server[]) {
  const mine = readers(s);
  return all
    .filter((o) => o !== s && o.name === s.name)
    .map((other) => ({ other, agents: readers(other).filter((a) => mine.includes(a)) }))
    .filter((c) => c.agents.length);
}

export const flagged = (s: Server) => s.places.some((e) => e.problems.length) || drifted(s);

export function matches(s: Server, query: string, agent: Provider | null) {
  if (agent && !readers(s).includes(agent)) return false;
  const q = query.trim().toLowerCase();
  return !q || s.name.toLowerCase().includes(q) || s.places.some((e) => (e.spec.command + " " + e.spec.args.join(" ") + " " + e.spec.url).toLowerCase().includes(q));
}

/** The command line or URL. */
export const summary = (spec: Shown) => (spec.transport === "stdio" ? [spec.command, ...spec.args].join(" ") : spec.url);

/**
 * Files of `s`'s scope to copy it into so `agents` see it too, files read by the most of them first. A file also
 * reaches the other agents reading it. None for managed servers, and none where the file already has the name.
 */
export function copyPlan(s: Server, agents: Provider[], sources: Source[]) {
  if (s.scope === "managed") return [];
  let need = agents.filter((a) => !readers(s).includes(a));
  const out: Source[] = [];
  while (need.length) {
    // Stable sort: on a tie the backend's order wins (.mcp.json first).
    const best = sources
      .filter((f) => f.scope === s.scope && !f.readOnly && !f.error && !s.places.some((e) => sameFile(e, f)) && !out.includes(f))
      .map((f) => ({ f, n: f.agents.filter((a) => need.includes(a)).length }))
      .filter((x) => x.n)
      .sort((a, b) => b.n - a.n)[0];
    if (!best) break;
    out.push(best.f);
    need = need.filter((a) => !best.f.agents.includes(a));
  }
  return out;
}

/** Arguments typed one per line. */
export const argLines = (text: string) => text.split("\n").map((l) => l.trim()).filter(Boolean);

/** Lines for a confirm dialog from a dry run: what's lost and worth knowing, per file. */
export function reportText(reports: Report[], short: (p: string) => string) {
  return reports
    .flatMap((r) => [
      ...r.lost.map((l) => `${short(r.path)}: can't hold ${l}.`),
      ...r.warnings.map((w) => `${short(r.path)}: ${w}.`),
    ])
    .join("\n");
}
