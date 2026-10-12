import { expect, test } from "bun:test";
import { closeTab, dockTab, equalize, minimize, movePath, moveTab, pane, panes, restore, rightmost, rootWith, split, type Tab } from "./layout";

const t = (id: string): Tab => ({ id, kind: "term", cwd: "/" });

test("the rightmost pane: the last column's top one", () => {
  const a = pane([t("a")]);
  const root = rootWith(a);
  expect(rightmost(root)).toBe(a);
  const b = split(root, a.id, "row", t("b")); // [a | b]
  split(root, b.id, "col", t("c")); // [a | b/c]
  expect(rightmost(root).id).toBe(b.id);
  split(root, a.id, "col", t("d")); // [a/d | b/c]
  expect(rightmost(root).id).toBe(b.id);
});

test("split, move, close collapse back to one pane", () => {
  const a = pane([t("a")]);
  const root = rootWith(a);

  const b = split(root, a.id, "row", t("b")); // [a | b]
  expect(root.dir).toBe("row");
  expect(panes(root).map((p) => p.id)).toEqual([a.id, b.id]);

  const c = split(root, b.id, "col", t("c")); // [a | (b / c)]
  expect(root.children[1].type).toBe("split");
  expect(panes(root).length).toBe(3);

  moveTab(root, "c", a.id, 0); // c joins a; empty pane c removed, nested split collapses
  expect(panes(root).length).toBe(2);
  expect(root.children.every((n) => n.type === "pane")).toBe(true);
  expect(a.tabs.map((x) => x.id)).toEqual(["c", "a"]);
  expect(a.active).toBe(0);

  closeTab(root, "b"); // pane b gone
  expect(panes(root)).toEqual([a]);
  closeTab(root, "a");
  closeTab(root, "c"); // last pane is kept, just empty
  expect(panes(root).length).toBe(1);
  expect(a.tabs.length).toBe(0);
  void c;
});

test("reorder within a pane", () => {
  const a = pane([t("1"), t("2"), t("3")]);
  const root = rootWith(a);
  moveTab(root, "1", a.id, 3);
  expect(a.tabs.map((x) => x.id)).toEqual(["2", "3", "1"]);
  expect(a.active).toBe(2);
});

test("dock a tab onto pane edges", () => {
  const a = pane([t("1"), t("2")]);
  const root = rootWith(a);
  const b = dockTab(root, "2", a.id, "left")!; // [2 | 1]
  expect(panes(root).map((p) => p.tabs[0].id)).toEqual(["2", "1"]);
  dockTab(root, "2", a.id, "bottom"); // pane b emptied & removed: [1 / 2]
  expect(root.dir).toBe("col"); // nested split lifted into root
  expect(panes(root).map((p) => p.tabs[0].id)).toEqual(["1", "2"]);
  expect(panes(root).some((p) => p.id === b.id)).toBe(false);
  dockTab(root, "2", panes(root)[1].id, "right"); // lone tab onto itself: no-op
  expect(panes(root).length).toBe(2);
});

test("movePath re-points tabs inside a moved folder only", () => {
  const f = (path: string): Tab => ({ id: path, kind: "file", path });
  const a = pane([f("/w/src/a.ts"), f("/w/src/lib/b.ts"), f("/w/srcx/c.ts"), t("term")]);
  const root = rootWith(a);
  movePath(root, "/w/src", "/w/old/src");
  expect(a.tabs.map((x) => (x.kind === "term" ? x.cwd : "path" in x ? x.path : ""))).toEqual(["/w/old/src/a.ts", "/w/old/src/lib/b.ts", "/w/srcx/c.ts", "/"]);
});

test("equalize gives panes an even share of each axis", () => {
  const a = pane([t("a")]);
  const root = rootWith(a);
  const b = split(root, a.id, "row", t("b"));
  const c = split(root, b.id, "row", t("c"));
  split(root, c.id, "col", t("d")); // [a | b | (c / d)]
  root.sizes = [5, 1, 2];
  equalize(root);
  expect(root.sizes).toEqual([1, 1, 1]);
  expect((root.children[2] as { sizes: number[] }).sizes).toEqual([1, 1]);

  split(root, a.id, "col", t("e"));
  const ae = root.children[0];
  split(root, (ae as { children: { id: string }[] }).children[1].id, "row", t("f")); // [(a / (e | f)) | b | (c / d)]
  equalize(root);
  expect(root.sizes).toEqual([2, 1, 1]); // e and f each get a column's width, like b
});

const ids = (root: ReturnType<typeof rootWith>) => panes(root).map((p) => p.tabs.map((x) => x.id).join(","));

test("minimize then restore puts a tab back in its pane and slot", () => {
  const a = pane([t("a"), t("b"), t("c")]);
  const root = rootWith(a);
  const m = minimize(root, "b")!;
  expect(ids(root)).toEqual(["a,c"]);
  expect(restore(root, m)).toBe(a);
  expect(ids(root)).toEqual(["a,b,c"]);
  expect(a.active).toBe(1);
});

test("restore recreates a removed pane on the side it was on", () => {
  const a = pane([t("a")]);
  const root = rootWith(a);
  const b = split(root, a.id, "row", t("b")); // [a | b]
  split(root, b.id, "col", t("c")); // [a | (b / c)]

  const mc = minimize(root, "c")!; // [a | b]
  expect(ids(root)).toEqual(["a", "b"]);
  restore(root, mc);
  expect(ids(root)).toEqual(["a", "b", "c"]);
  expect(root.children[1].type).toBe("split"); // c is under b again

  const ma = minimize(root, "a")!; // (b / c) hoisted into the root
  expect(ids(root)).toEqual(["b", "c"]);
  restore(root, ma);
  expect(ids(root)).toEqual(["a", "b", "c"]);
  expect(root.dir).toBe("row");
  expect(root.children[0].type).toBe("pane"); // a on the left of the rest
});

test("restore into the only, empty pane", () => {
  const a = pane([t("a")]);
  const root = rootWith(a);
  const m = minimize(root, "a")!;
  expect(ids(root)).toEqual([""]);
  restore(root, { ...m, pane: "gone" });
  expect(ids(root)).toEqual(["a"]);
});

test("stored layouts are checked before use", async () => {
  const { isRoot, isMinimized } = await import("./layout");
  const root = rootWith(pane([t("a"), { id: "f", kind: "file", path: "/x" }]));
  split(root, root.children[0].id, "col", t("b"));
  expect(isRoot(JSON.parse(JSON.stringify(root)))).toBe(true);
  expect(isRoot(null)).toBe(false);
  expect(isRoot(pane())).toBe(false); // the root is always a split
  expect(isRoot({ ...root, children: [] })).toBe(false);
  expect(isRoot({ ...root, sizes: [] })).toBe(false);
  expect(isRoot(rootWith(pane([{ id: "x", kind: "term" } as unknown as Tab])))).toBe(false);
  expect(isMinimized({ tab: t("a"), pane: "p", index: 0 })).toBe(true);
  expect(isMinimized({ tab: t("a"), pane: "p" })).toBe(false);
});
