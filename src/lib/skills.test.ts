import { expect, test } from "bun:test";
import { agentsOf, clashes, listNames, lostWithout, matches, shortPath, validName, type Location, type Skill } from "./skills";

const locations: Location[] = [
  { dir: "/h/.claude/skills", scope: "global", agents: ["claude", "grok", "opencode"], label: null },
  { dir: "/h/.agents/skills", scope: "global", agents: ["codex", "opencode"], label: null },
  { dir: "/p/.claude/skills", scope: "project", agents: ["claude", "grok", "opencode"], label: null },
];
const skill = (k: Partial<Skill>): Skill => ({
  name: "wrangler", description: "Workers CLI", dir: "/h/.agents/skills/wrangler", file: "", scope: "global", readOnly: false,
  agents: ["claude", "codex", "grok", "opencode"], places: [], files: [], problems: [], ...k,
});

test("names follow the Agent Skills rule", () => {
  expect(validName("git-commit-2")).toBe(true);
  for (const bad of ["", "Git", "a_b", "-a", "a-", "a--b", "a".repeat(65)]) expect(validName(bad)).toBe(false);
});

test("agents of folders come in the fixed provider order", () => {
  expect(agentsOf(["/h/.agents/skills", "/h/.claude/skills"], locations)).toEqual(["claude", "codex", "grok", "opencode"]);
});

test("removing a link loses only the agents no other place covers", () => {
  const link = { path: "/h/.claude/skills/wrangler", location: "/h/.claude/skills", scope: "global" as const, link: true };
  const own = { path: "/h/.agents/skills/wrangler", location: "/h/.agents/skills", scope: "global" as const, link: false };
  const k = skill({ places: [own, link] });
  expect(lostWithout(k, link, locations)).toEqual(["claude", "grok"]); // OpenCode still reads .agents
  expect(lostWithout(k, own, locations)).toEqual(["codex"]);
});

test("same-named skills clash only where agents overlap", () => {
  const global = skill({});
  const project = skill({ dir: "/p/.claude/skills/wrangler", scope: "project", agents: ["claude", "grok", "opencode"] });
  const other = skill({ name: "other" });
  const codexOnly = skill({ dir: "/h/.codex/skills/wrangler", agents: ["codex"] });
  const all = [global, project, other, codexOnly];
  expect(clashes(project, all).map((c) => c.agents)).toEqual([["claude", "grok", "opencode"]]);
  expect(clashes(global, all).map((c) => c.other)).toEqual([project, codexOnly]);
  expect(clashes(other, all)).toEqual([]);
});

test("paths shorten to the project, then home", () => {
  expect(shortPath("/h/p/.claude/skills/x", "/h", "/h/p")).toBe(".claude/skills/x");
  expect(shortPath("/h/.agents/skills", "/h", "/h/p")).toBe("~/.agents/skills");
  expect(shortPath("/hx/a", "/h", "")).toBe("/hx/a");
  expect(shortPath("/h/p", "/h", "/h/p")).toBe(".");
});

test("search matches name or description, within an agent", () => {
  const k = skill({});
  expect(matches(k, "WORKERS", null)).toBe(true);
  expect(matches(k, "wrang", "codex")).toBe(true);
  expect(matches(k, "", "antigravity")).toBe(false);
  expect(matches(k, "deploy", null)).toBe(false);
});

test("lists read naturally", () => {
  expect(listNames(["Codex"])).toBe("Codex");
  expect(listNames(["A", "B", "C"])).toBe("A, B and C");
});
