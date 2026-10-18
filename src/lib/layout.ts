// Layout tree: the root is always a split; leaves are panes holding tabs.
// Ops mutate in place (works on plain objects and on Svelte $state proxies).

export type Tab =
  // agent: coding agent CLI running in it when last looked (see agents.svelte.ts); name: set by the user (Rename)
  | { id: string; kind: "term"; cwd: string; agent?: string; name?: string }
  | { id: string; kind: "file"; path: string; preview?: boolean; wrap?: boolean /* on unless false */ }
  // path absolute, orig repo-relative; ref = a commit (shows ref^ -> ref instead of index/worktree), or base -> ref
  | { id: string; kind: "diff"; path: string; staged: boolean; orig?: string; ref?: string; base?: string }
  // url "" = a new tab waiting for an address; title = the page's, last seen
  | { id: string; kind: "web"; url: string; title?: string }
  // a database on this machine (see db/); name: what the tab says; write: statements may change data
  | { id: string; kind: "db"; name: string; target: DbTarget; write?: boolean };
/** Where a database is: a server's port, a container (exec'd into without one), or a SQLite file. root: its project. */
export type DbTarget = {
  engine: "postgres" | "mysql" | "redis" | "mongo" | "sqlite";
  host?: string;
  port?: number;
  container?: string;
  file?: string;
  root: string;
  database?: string;
};
export type Pane = { id: string; type: "pane"; tabs: Tab[]; active: number };
export type Split = { id: string; type: "split"; dir: "row" | "col"; children: LNode[]; sizes: number[] };
export type LNode = Pane | Split;

export const uid = () => crypto.randomUUID();
export const pane = (tabs: Tab[] = []): Pane => ({ id: uid(), type: "pane", tabs, active: 0 });
export const rootWith = (p: Pane): Split => ({ id: uid(), type: "split", dir: "row", children: [p], sizes: [1] });

export function panes(n: LNode): Pane[] {
  return n.type === "pane" ? [n] : n.children.flatMap(panes);
}

/** The pane at the right edge: the last of a row, the top one of a column. */
export function rightmost(n: LNode): Pane {
  return n.type === "pane" ? n : rightmost(n.dir === "row" ? n.children[n.children.length - 1] : n.children[0]);
}

function parentOf(root: Split, id: string): Split | null {
  for (const c of root.children) {
    if (c.id === id) return root;
    if (c.type === "split") {
      const p = parentOf(c, id);
      if (p) return p;
    }
  }
  return null;
}

export function findTab(root: Split, tabId: string): { pane: Pane; index: number } | null {
  for (const p of panes(root)) {
    const index = p.tabs.findIndex((t) => t.id === tabId);
    if (index >= 0) return { pane: p, index };
  }
  return null;
}

/** Put `tab` in a new pane next to `paneId` (after it, or before), splitting in `dir`. Returns the new pane. */
export function split(root: Split, paneId: string, dir: "row" | "col", tab: Tab, before = false): Pane {
  const parent = parentOf(root, paneId)!;
  const i = parent.children.findIndex((c) => c.id === paneId);
  const fresh = pane([tab]);
  if (parent.children.length === 1) parent.dir = dir;
  if (parent.dir === dir) {
    const half = parent.sizes[i] / 2;
    parent.children.splice(before ? i : i + 1, 0, fresh);
    parent.sizes.splice(i, 1, half, half);
  } else {
    parent.children[i] = { id: uid(), type: "split", dir, children: before ? [fresh, parent.children[i]] : [parent.children[i], fresh], sizes: [1, 1] };
  }
  return fresh;
}

function removePane(root: Split, paneId: string) {
  if (panes(root).length === 1) return; // always keep one pane
  const parent = parentOf(root, paneId)!;
  const i = parent.children.findIndex((c) => c.id === paneId);
  parent.children.splice(i, 1);
  parent.sizes.splice(i, 1);
  if (parent.children.length === 1) {
    const only = parent.children[0];
    if (parent === root) {
      if (only.type === "split") Object.assign(root, { dir: only.dir, children: only.children, sizes: only.sizes });
    } else {
      const gp = parentOf(root, parent.id)!;
      gp.children[gp.children.findIndex((c) => c.id === parent.id)] = only;
    }
  }
}

export function closeTab(root: Split, tabId: string) {
  const f = findTab(root, tabId);
  if (!f) return;
  f.pane.tabs.splice(f.index, 1);
  f.pane.active = Math.max(0, Math.min(f.pane.active, f.pane.tabs.length - 1));
  if (!f.pane.tabs.length) removePane(root, f.pane.id);
}

export function moveTab(root: Split, tabId: string, toPaneId: string, index: number) {
  const f = findTab(root, tabId);
  const to = panes(root).find((p) => p.id === toPaneId);
  if (!f || !to) return;
  const [tab] = f.pane.tabs.splice(f.index, 1);
  if (f.pane === to && f.index < index) index--;
  to.tabs.splice(index, 0, tab);
  to.active = to.tabs.indexOf(tab);
  if (f.pane !== to) {
    f.pane.active = Math.max(0, Math.min(f.pane.active, f.pane.tabs.length - 1));
    if (!f.pane.tabs.length) removePane(root, f.pane.id);
  }
}

export type Side = "left" | "right" | "top" | "bottom";

/** Drag a tab onto an edge of a pane: split that pane and put the tab on that side. */
export function dockTab(root: Split, tabId: string, paneId: string, side: Side) {
  const f = findTab(root, tabId);
  if (!f || (f.pane.id === paneId && f.pane.tabs.length === 1)) return;
  const [tab] = f.pane.tabs.splice(f.index, 1);
  f.pane.active = Math.max(0, Math.min(f.pane.active, f.pane.tabs.length - 1));
  const fresh = split(root, paneId, side === "left" || side === "right" ? "row" : "col", tab, side === "left" || side === "top");
  if (!f.pane.tabs.length) removePane(root, f.pane.id);
  return fresh;
}

/** A file or folder moved from `from` to `to`: re-point file tabs at it or inside it. */
export function movePath(root: Split, from: string, to: string) {
  for (const p of panes(root))
    for (const t of p.tabs)
      if (t.kind === "file" && (t.path === from || t.path.startsWith(from + "/"))) t.path = to + t.path.slice(from.length);
}

/** Panes side by side along `dir` in the widest stretch of `n` (its columns for "row", rows for "col"). */
function span(n: LNode, dir: "row" | "col"): number {
  if (n.type === "pane") return 1;
  const s = n.children.map((c) => span(c, dir));
  return n.dir === dir ? s.reduce((a, b) => a + b, 0) : Math.max(...s);
}

/** Auto-arrange: size every split so panes share width and height evenly, like a grid. */
export function equalize(n: LNode) {
  if (n.type === "pane") return;
  n.sizes = n.children.map((c) => span(c, n.dir));
  n.children.forEach(equalize);
}

/** Put `tab` in a new pane along the root's `side` edge, next to everything else. */
function dockRoot(root: Split, tab: Tab, side: Side): Pane {
  const dir = side === "left" || side === "right" ? "row" : "col";
  const before = side === "left" || side === "top";
  const fresh = pane([tab]);
  if (root.children.length > 1 && root.dir !== dir) {
    const rest: Split = { id: uid(), type: "split", dir: root.dir, children: root.children, sizes: root.sizes };
    Object.assign(root, { dir, children: [rest], sizes: [1] });
  }
  root.dir = dir;
  const n = root.children.length;
  root.children.splice(before ? 0 : n, 0, fresh);
  root.sizes.splice(before ? 0 : n, 0, root.sizes.reduce((a, b) => a + b, 0) / n);
  return fresh;
}

/** A minimized tab and where it was: its pane and slot, and the node beside the pane in case the pane goes away
 * (`side` = which side of that node the pane was on). */
export type Minimized = { tab: Tab; pane: string; index: number; near?: { node: string; side: Side } };

/** Take a tab out of the layout, remembering where it was. */
export function minimize(root: Split, tabId: string): Minimized | null {
  const f = findTab(root, tabId);
  if (!f) return null;
  const parent = parentOf(root, f.pane.id)!;
  const i = parent.children.findIndex((c) => c.id === f.pane.id);
  const row = parent.dir === "row";
  const next = parent.children[i + 1], prev = parent.children[i - 1];
  const near = next ? { node: next.id, side: (row ? "left" : "top") as Side }
    : prev ? { node: prev.id, side: (row ? "right" : "bottom") as Side } : undefined;
  const tab = f.pane.tabs[f.index];
  closeTab(root, tabId);
  return { tab, pane: f.pane.id, index: f.index, near };
}

/** Put a minimized tab back: in its pane if that's still there, else a new pane where that one was. Returns its pane. */
export function restore(root: Split, m: Minimized): Pane {
  const all = panes(root);
  const at = all.find((p) => p.id === m.pane) ?? (all.length === 1 && !all[0].tabs.length ? all[0] : null);
  if (at) {
    at.tabs.splice(Math.min(m.index, at.tabs.length), 0, m.tab);
    at.active = at.tabs.indexOf(m.tab);
    return at;
  }
  const side = m.near?.side ?? "right";
  // The neighbour was hoisted into the root when the pane went (so it was the whole rest): dock on the root's edge
  if (!m.near || !parentOf(root, m.near.node)) return dockRoot(root, m.tab, side);
  return split(root, m.near.node, side === "left" || side === "right" ? "row" : "col", m.tab, side === "left" || side === "top");
}

// Shape checks for layouts read back from storage: a bad one (an older or newer version's, a broken write) is
// dropped for a fresh layout instead of breaking the window.
type Raw = any;
const str = (v: unknown) => typeof v === "string";
export const isTab = (t: Raw): t is Tab =>
  !!t && str(t.id) && (t.kind === "term" ? str(t.cwd) : t.kind === "web" ? str(t.url) : t.kind === "db" ? str(t.name) && str(t.target?.engine) && str(t.target?.root) : (t.kind === "file" || t.kind === "diff") && str(t.path));
function isNode(n: Raw): n is LNode {
  if (n?.type === "pane") return str(n.id) && Array.isArray(n.tabs) && n.tabs.every(isTab) && Number.isInteger(n.active);
  return (
    n?.type === "split" && str(n.id) && (n.dir === "row" || n.dir === "col") &&
    Array.isArray(n.children) && n.children.length > 0 && n.children.every(isNode) &&
    Array.isArray(n.sizes) && n.sizes.length === n.children.length && n.sizes.every((s: unknown) => typeof s === "number" && s >= 0)
  );
}
export const isRoot = (n: unknown): n is Split => isNode(n) && (n as LNode).type === "split";
export const isMinimized = (m: Raw): m is Minimized => !!m && isTab(m.tab) && str(m.pane) && Number.isInteger(m.index);
