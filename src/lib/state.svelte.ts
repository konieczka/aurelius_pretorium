import { ask } from "@tauri-apps/plugin-dialog";
import * as L from "./layout";
import { disposeTerm } from "./Terminal.svelte";
import { disposeEditor } from "./Editor.svelte";

export const app = $state({
  workspace: "",
  root: L.rootWith(L.pane()),
  focused: "", // pane id
  dirty: {} as Record<string, boolean>, // tab id -> unsaved
  titles: {} as Record<string, string>, // tab id -> terminal title
  treeVersion: 0,
  dragging: false, // a tab is being dragged: panes show drop zones
});

const store = {
  get: (k: string) => { try { return localStorage.getItem(k); } catch { return null; } },
  set: (k: string, v: string) => { try { localStorage.setItem(k, v); } catch {} },
};

export const lastWorkspace = () => store.get("workspace");
export function saveLayout() {
  if (app.workspace) store.set("layout:" + app.workspace, JSON.stringify(app.root));
}

export function focusedPane(): L.Pane {
  const all = L.panes(app.root);
  return all.find((p) => p.id === app.focused) ?? all[0];
}

export function activeTab(): L.Tab | undefined {
  const p = focusedPane();
  return p.tabs[p.active];
}

const term = (): L.Tab => ({ id: L.uid(), kind: "term", cwd: app.workspace });

export function openWorkspace(path: string) {
  for (const p of L.panes(app.root)) for (const t of p.tabs) dispose(t);
  app.workspace = path;
  store.set("workspace", path);
  let root: L.Split | null = null;
  try { root = JSON.parse(store.get("layout:" + path) ?? "null"); } catch {}
  app.root = root ?? L.rootWith(L.pane([term()]));
  app.focused = L.panes(app.root)[0].id;
  app.dirty = {};
}

export function newTerminal() {
  const p = focusedPane();
  p.tabs.push(term());
  p.active = p.tabs.length - 1;
}

export function splitFocused(dir: "row" | "col") {
  app.focused = L.split(app.root, focusedPane().id, dir, term()).id;
}

export function openFile(path: string) {
  const existing = L.panes(app.root).flatMap((p) => p.tabs).find((t) => t.kind === "file" && t.path === path);
  if (existing) return activate(existing.id);
  const p = focusedPane();
  p.tabs.push({ id: L.uid(), kind: "file", path, preview: path.endsWith(".md") });
  p.active = p.tabs.length - 1;
}

export function activate(tabId: string) {
  const f = L.findTab(app.root, tabId);
  if (!f) return;
  f.pane.active = f.index;
  app.focused = f.pane.id;
}

export async function closeTab(tabId: string) {
  const f = L.findTab(app.root, tabId);
  if (!f) return;
  const t = f.pane.tabs[f.index];
  if (app.dirty[t.id] && !(await ask("Discard unsaved changes?", { kind: "warning" }))) return;
  dispose(t);
  L.closeTab(app.root, tabId);
  if (!L.panes(app.root).some((p) => p.id === app.focused)) app.focused = L.panes(app.root)[0].id;
}

function dispose(t: L.Tab) {
  if (t.kind === "term") disposeTerm(t.id);
  else disposeEditor(t.id);
  delete app.dirty[t.id];
  delete app.titles[t.id];
}
