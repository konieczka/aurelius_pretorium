// Skills view: what `skills_scan` returns, and the comparisons the page makes over it.

import { PROVIDERS, type Provider } from "./usageStats";

export type Scope = "project" | "global" | "managed";
/** A skills folder and the agents that read it. */
export type Location = { dir: string; scope: Scope; agents: Provider[]; label: string | null };
/** Where a skill shows up: its own folder, or a symlink to it from another skills folder. */
export type Place = { path: string; location: string; scope: Scope; link: boolean };
export type Skill = {
  name: string;
  description: string;
  dir: string; // real folder
  file: string; // its SKILL.md ("" for a broken link)
  scope: Scope;
  readOnly: boolean;
  agents: Provider[];
  places: Place[];
  files: string[];
  problems: string[];
};
export type Shared = { scope: Scope; dir: string; links: string[] };
export type Scan = { skills: Skill[]; locations: Location[]; shared: Shared[]; home: string };

/** Same rule as the backend's: lowercase letters, digits and single hyphens, at most 64 characters. */
export const validName = (s: string) => /^[a-z0-9]+(-[a-z0-9]+)*$/.test(s) && s.length <= 64;

const byOrder = (a: Provider[]) => PROVIDERS.filter((p) => a.includes(p));

/** Agents that read any of `dirs`. */
export function agentsOf(dirs: string[], locations: Location[]) {
  return byOrder(locations.filter((l) => dirs.includes(l.dir)).flatMap((l) => l.agents));
}

/** Agents that stop seeing `k` once `place` is removed. */
export function lostWithout(k: Skill, place: Place, locations: Location[]) {
  const kept = agentsOf(k.places.filter((p) => p !== place).map((p) => p.location), locations);
  return agentsOf([place.location], locations).filter((a) => !kept.includes(a));
}

/** Other skills with the same name that some of the same agents load: those agents get only one of them. */
export function clashes(k: Skill, all: Skill[]) {
  return all
    .filter((o) => o !== k && o.name === k.name)
    .map((other) => ({ other, agents: k.agents.filter((a) => other.agents.includes(a)) }))
    .filter((c) => c.agents.length);
}

/** `path` relative to the project, or under home as `~/…`. */
export function shortPath(path: string, home: string, project: string) {
  const under = (root: string) => root && (path === root || path.startsWith(root + "/"));
  if (under(project)) return path.slice(project.length + 1) || ".";
  if (under(home)) return "~" + path.slice(home.length);
  return path;
}

export function matches(k: Skill, query: string, agent: Provider | null) {
  if (agent && !k.agents.includes(agent)) return false;
  const q = query.trim().toLowerCase();
  return !q || k.name.includes(q) || k.description.toLowerCase().includes(q);
}

/** "Claude Code, Codex and Grok Build". */
export function listNames(names: string[]) {
  return names.length < 2 ? (names[0] ?? "") : `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}
