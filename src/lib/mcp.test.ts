import { expect, test } from "bun:test";
import { argLines, blocksFor, clashes, copyPlan, drifted, group, matches, readers, reportText, summary, users, validName, type Entry, type Source } from "./mcp";

const entry = (e: Partial<Entry>): Entry => ({
  name: "github", path: "/p/.mcp.json", table: ["mcpServers"], scope: "project", dialect: "claude", label: null,
  agents: ["claude", "grok", "muse"], blocked: [], enabled: true, extra: [], problems: [], fingerprint: "a", readOnly: false,
  spec: { transport: "stdio", command: "npx", args: ["-y", "gh"], env: [], cwd: "", url: "", headers: [] }, ...e,
});
const source = (s: Partial<Source>): Source => ({
  path: "/p/.mcp.json", table: ["mcpServers"], scope: "project", dialect: "claude", label: null, agents: ["claude", "grok", "muse"],
  exists: true, readOnly: false, error: null, ...s,
});
const sources = [
  source({}),
  source({ path: "/p/.codex/config.toml", table: ["mcp_servers"], dialect: "codex", agents: ["codex"] }),
  source({ path: "/p/.grok/config.toml", table: ["mcp_servers"], dialect: "grok", agents: ["grok"] }),
  source({ path: "/p/opencode.json", table: ["mcp"], dialect: "opencode", agents: ["opencode"] }),
  source({ path: "/h/.claude.json", scope: "global", agents: ["claude", "grok"] }),
];

test("names follow the strictest agent's rule", () => {
  expect(validName("github") && validName("_x-1")).toBe(true);
  for (const bad of ["", "1x", "a_", "a.b", "a b", "a".repeat(65)]) expect(validName(bad)).toBe(false);
});

test("definitions group by name within a scope", () => {
  const all = group([entry({}), entry({ path: "/p/.codex/config.toml", agents: ["codex"] }), entry({ scope: "global", path: "/h/.claude.json", agents: ["claude", "grok"] })]);
  expect(all.map((s) => [s.key, s.places.length])).toEqual([["project:github", 2], ["global:github", 1]]);
  expect(readers(all[0])).toEqual(["claude", "codex", "grok", "muse"]);
  expect(clashes(all[1], all)[0].agents).toEqual(["claude", "grok"]);
});

test("blocked agents read a server but don't load it", () => {
  const [s] = group([entry({ blocked: [{ agent: "muse", reason: "Untrusted", by: null }] })]);
  expect(users(s)).toEqual(["claude", "grok"]);
  expect(blocksFor(s, "muse")[0].reason).toBe("Untrusted");
});

test("copies that differ are drift", () => {
  expect(drifted(group([entry({}), entry({ path: "/p/x", fingerprint: "b" })])[0])).toBe(true);
  expect(drifted(group([entry({}), entry({ path: "/p/x" })])[0])).toBe(false);
});

test("copy plan picks the fewest files of the same scope", () => {
  const [s] = group([entry({})]);
  expect(copyPlan(s, ["codex", "opencode"], sources).map((f) => f.path)).toEqual(["/p/.codex/config.toml", "/p/opencode.json"]);
  expect(copyPlan(s, ["grok"], sources)).toEqual([]); // already reads .mcp.json
  const [g] = group([entry({ scope: "global", path: "/h/x", agents: ["codex"] })]);
  expect(copyPlan(g, ["claude", "grok"], sources).map((f) => f.path)).toEqual(["/h/.claude.json"]);
  expect(copyPlan(group([entry({ scope: "managed" })])[0], ["codex"], sources)).toEqual([]);
});

test("search covers names, commands and URLs; the agent filter what it reads", () => {
  const [s] = group([entry({})]);
  expect(matches(s, "GIT", null) && matches(s, "gh", null) && matches(s, "", "muse")).toBe(true);
  expect(matches(s, "", "codex") || matches(s, "nope", null)).toBe(false);
  expect(summary(s.places[0].spec)).toBe("npx -y gh");
});

test("arguments are one per line; reports read per file", () => {
  expect(argLines(" -y \n\n gh ")).toEqual(["-y", "gh"]);
  const text = reportText([{ path: "/p/a", table: [], lost: ["cwd"], warnings: ["Careful"], error: null }], (p) => p.slice(3));
  expect(text).toBe("a: can't hold cwd.\na: Careful.");
});
