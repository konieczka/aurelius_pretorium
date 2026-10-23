import { expect, test } from "bun:test";
import { buildProjects, castraLayout, CARD_H, F, frontLayout, langShares, tokensByProject } from "./war";
import type { Row } from "./fleet";
import type { Container, Listener } from "./Infra.svelte";
import type { Row as UsageRow } from "./usageStats";

const row = (id: string, cwd: string, more: Partial<Row> = {}): Row => ({ id, ws: "w", tab: null, agent: "claude", status: "idle", asking: null, label: "", cwd, since: 0, ...more });
const listener = (pid: number, root: string, tab: string | null = null): Listener => ({ pid, ports: [3000], name: "vite", args: "vite", cwd: root, root, kind: "project", tab });
const container = (id: string, root: string, state = "running"): Container => ({ id, name: `${id}-1`, image: "postgres:17", state, status: "", ports: [{ host: 5432, inner: 5432 }], project: "", service: id, root, dir: root });
// A worktree belongs to its main checkout, like Fleet groups it
const keyOf = (p: string) => (p.startsWith("/w/app") ? "/w/app" : p);

test("projects gather agents, servers and workspace folders, most urgent first", () => {
  const ps = buildProjects(
    [row("a1", "/w/app/src", { status: "busy" }), row("a2", "/w/app-feature", { status: "attention" }), row("b1", "/w/lib")],
    [listener(10, "/w/app", "a1"), listener(11, "/w/app", "plain-term"), { ...listener(12, "/"), kind: "system" }],
    [container("db", "/w/app", "exited"), container("loose", "")],
    ["/w/notes", "/w/lib"],
    keyOf,
  );
  expect(ps.map((p) => p.key)).toEqual(["/w/app", "/w/lib", "/w/notes"]);
  const app = ps[0];
  expect(app.agents.map((a) => a.id)).toEqual(["a2", "a1"]); // waiting before working
  expect(app.services.map((s) => [s.id, s.agent, s.tab, s.up])).toEqual([["p10", "a1", "a1", true], ["p11", null, "plain-term", true], ["cdb", null, null, false]]);
});

test("tokens count where their session ran, worktrees included", () => {
  const u = (cwd: string, total: number, hour: number): UsageRow => ({ provider: "claude", model: "m", session: "s", cwd, hour, uncached: 0, cached: 0, output: 0, total });
  const t = tokensByProject([u("/w/app", 10, 0), u("/w/app/src", 5, 100), u("/w/app-feature", 7, 100), u("/w/application", 1, 100), u("", 99, 100), u("/w/app/vendor/lib", 3, 100)], ["/w/app", "/w/app/vendor/lib"], 50);
  expect(t.get("/w/app")).toEqual({ month: 22, today: 12 }); // "/w/application" is no worktree of it
  expect(t.get("/w/app/vendor/lib")).toEqual({ month: 3, today: 3 }); // the deepest project wins
});

test("language shares keep the top ones and fold the rest into Other", () => {
  const s = { lines: 100, files: 3, commits7d: 0, langs: [{ name: "TypeScript", lines: 60 }, { name: "Other", lines: 25 }, { name: "CSS", lines: 15 }] };
  expect(langShares(s)).toEqual([{ name: "TypeScript", share: 0.6 }, { name: "CSS", share: 0.15 }, { name: "Other", share: 0.25 }]);
  expect(langShares({ ...s, lines: 0, langs: [] })).toEqual([]);
});

test("castra camps don't overlap and every agent and server has a place", () => {
  const ps = buildProjects(
    Array.from({ length: 7 }, (_, i) => row(`a${i}`, `/w/p${i % 3}`)),
    [listener(1, "/w/p0", "a0"), listener(2, "/w/p0", "a3")],
    [],
    ["/w/empty"],
    (p) => p,
  );
  const camps = castraLayout(ps, 1400, 800);
  expect(camps.length).toBe(4);
  for (const a of camps) for (const b of camps) if (a !== b) expect(a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y).toBe(true);
  expect(camps.flatMap((c) => c.units).length).toBe(7);
  expect(camps.find((c) => c.key === "/w/p0")!.roads.length).toBe(2);
});

test("front line stacks cards in their band and sizes bands to the tallest stack", () => {
  const ps = buildProjects(
    [row("w1", "/w/a", { status: "attention" }), row("w2", "/w/a", { status: "attention" }), row("b1", "/w/b", { status: "busy" }), row("i1", "/w/b")],
    [listener(1, "/w/b", "b1")],
    [],
    [],
    (p) => p,
  );
  const f = frontLayout(ps);
  expect(f.lanes.map((l) => [l.key, l.calm])).toEqual([["/w/a", false], ["/w/b", true]]);
  expect(f.bands.front.h).toBe(F.PT + CARD_H.attention * 2 + F.CGAP + F.PB);
  expect(f.cards.b1.y).toBe(f.bands.work.y + F.PT);
  expect(f.cards.i1.y).toBe(f.bands.rear.y + F.PT);
  expect(f.links.map((l) => [l.agent, l.service])).toEqual([["b1", "p1"]]);
});
