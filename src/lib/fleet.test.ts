import { expect, test } from "bun:test";
import { byProject, cleanTitle, fleetRows, fmtSpan, type Row } from "./fleet";
import type { Shell } from "./agents.svelte";
import type { Tab } from "./layout";

const term = (id: string, more: Partial<Tab & { kind: "term" }> = {}): Tab => ({ id, kind: "term", cwd: "/p", ...more });
const shell = (more: Partial<Shell>): Shell => ({ agent: "claude", quiet: null, title: "", cwd: "/p", busy: false, attention: false, asking: null, since: 0, ...more });

test("one row per agent, most urgent first", () => {
  const rows = fleetRows(
    [
      { id: "w1", tabs: [term("idle"), term("plain"), term("busy"), { id: "f", kind: "file", path: "/p/a" }] },
      { id: "w2", tabs: [term("asleep", { agent: "codex" }), term("waiting", { name: "Mine" })] },
    ],
    {
      idle: shell({ title: "✳ Fix the bug", since: 5000 }),
      plain: shell({ agent: null }),
      busy: shell({ busy: true, title: "⠐ Refactor", since: 100 }),
      waiting: shell({ attention: true, title: "✳ Ignored" }),
      lost: shell({ agent: "grok", cwd: "/q", since: 9 }),
    },
  );
  expect(rows.map((r) => [r.id, r.status, r.ws, r.label])).toEqual([
    ["waiting", "attention", "w2", "Mine"],
    ["busy", "busy", "w1", "Refactor"],
    ["lost", "idle", null, ""],
    ["idle", "idle", "w1", "Fix the bug"],
    ["asleep", "asleep", "w2", ""],
  ]);
  expect(rows.find((r) => r.id === "asleep")?.agent).toBe("codex");
});

test("a prompt that's up needs you, even under a spinner", () => {
  const rows = fleetRows([{ id: "w", tabs: [term("a"), term("b")] }], { a: shell({ busy: true, since: 5 }), b: shell({ busy: true, asking: "approval", since: 9 }) });
  expect(rows.map((r) => [r.id, r.status, r.asking])).toEqual([
    ["b", "attention", "approval"],
    ["a", "busy", null],
  ]);
});

test("a shell whose agent quit drops the saved one", () => {
  expect(fleetRows([{ id: "w", tabs: [term("t", { agent: "claude" })] }], { t: shell({ agent: null }) })).toEqual([]);
});

test("projects in order of their most urgent agent", () => {
  const row = (id: string, cwd: string) => ({ id, cwd }) as Row;
  const groups = byProject([row("a", "/x/wt"), row("b", "/y"), row("c", "/x"), row("d", "/y")], (r) => (r.cwd === "/x/wt" ? "/x" : r.cwd));
  expect(groups.map((g) => [g.key, g.rows.map((r) => r.id)])).toEqual([
    ["/x", ["a", "c"]],
    ["/y", ["b", "d"]],
  ]);
});

test("titles lose the status glyph", () => {
  expect(cleanTitle("⠐ Fix it")).toBe("Fix it");
  expect(cleanTitle("✳ Claude Code")).toBe("Claude Code");
  expect(cleanTitle("- Responding - grok")).toBe("Responding - grok");
  expect(cleanTitle("◐ ")).toBe("");
  expect(cleanTitle("⠦ fleet_command", "/dev/fleet_command")).toBe("");
  expect(cleanTitle("Inspect first two commits | fleet_command", "/dev/fleet_command")).toBe("Inspect first two commits");
  expect(cleanTitle("⠐ Fix it — app", "/x/app")).toBe("Fix it");
  expect(cleanTitle("Rename the webapp", "/x/app")).toBe("Rename the webapp");
  expect(cleanTitle("Fix it | app", "/x/other")).toBe("Fix it | app");
  expect(cleanTitle("grok", "/x/app", "grok")).toBe("");
  expect(cleanTitle("⠧ - Responding - grok", "/x/app", "grok")).toBe("Responding");
  expect(cleanTitle("Fix it - grok | app", "/x/app", "grok")).toBe("Fix it");
  expect(cleanTitle("Teach grok", "/x/app", "grok")).toBe("Teach grok");
});

test("spans", () => {
  expect([fmtSpan(4200), fmtSpan(125_000), fmtSpan(3 * 3600_000 + 5), fmtSpan(50 * 3600_000)]).toEqual(["4s", "2m", "3h", "2d"]);
});
