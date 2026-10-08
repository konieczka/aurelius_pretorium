// Layout tree: the root is always a split; leaves are panes holding tabs.
// Ops mutate in place (works on plain objects and on Svelte $state proxies).

export type Tab =
  | { id: string; kind: "term"; cwd: string }
  | { id: string; kind: "file"; path: string; preview?: boolean };
export type Pane = { id: string; type: "pane"; tabs: Tab[]; active: number };
export type Split = { id: string; type: "split"; dir: "row" | "col"; children: LNode[]; sizes: number[] };
export type LNode = Pane | Split;

export const uid = () => crypto.randomUUID();
export const pane = (tabs: Tab[] = []): Pane => ({ id: uid(), type: "pane", tabs, active: 0 });
export const rootWith = (p: Pane): Split => ({ id: uid(), type: "split", dir: "row", children: [p], sizes: [1] });

export function panes(n: LNode): Pane[] {
  return n.type === "pane" ? [n] : n.children.flatMap(panes);
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
