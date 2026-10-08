import { invoke } from "@tauri-apps/api/core";
import { ask, message } from "@tauri-apps/plugin-dialog";
import * as L from "./layout";
import { disposeTerm, queueInput } from "./Terminal.svelte";
import { disposeEditor } from "./Editor.svelte";

export const app = $state({
  workspace: "",
  root: L.rootWith(L.pane()),
  focused: "", // pane id
  dirty: {} as Record<string, boolean>, // tab id -> unsaved
  titles: {} as Record<string, string>, // tab id -> terminal title
  treeVersion: 0,
  dragging: false, // a tab is being dragged: panes show drop zones
  expanded: {} as Record<string, boolean>, // folder path -> open in the tree
  selectedDir: "", // where New File / New Folder go (last clicked folder, or the clicked file's folder)
  creating: null as null | { dir: string; kind: "file" | "dir" }, // inline name input in the tree
  dropDir: "", // folder highlighted while dragging a tree entry
  selectedPath: "", // tree row ⌘⌫ deletes
  sidebar: "files" as "files" | "git",
  conflicts: {} as Record<string, number>, // tab id -> conflict blocks in the editor
  gitVersion: 0, // bumped on save / git ops: diff views refetch
  treeFocused: false, // last interaction was in the sidebar: ⌘Z / ⌘⌫ go to the tree, not the editor/terminal
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
  app.expanded = {};
  app.selectedDir = path;
  app.creating = null;
  app.selectedPath = "";
  history = [];
}

export function newTerminal() {
  const p = focusedPane();
  p.tabs.push(term());
  p.active = p.tabs.length - 1;
}

/** New terminal tab in the focused pane that runs `command` once its shell starts. */
export function runInTerminal(cwd: string, command: string) {
  const p = focusedPane();
  const t: L.Tab = { id: L.uid(), kind: "term", cwd };
  queueInput(t.id, command + "\r");
  p.tabs.push(t);
  p.active = p.tabs.length - 1;
  app.focused = p.id;
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

/** Diff tab: staged = index vs HEAD, else working tree vs index; with `ref`, that commit's change. */
export function openDiff(path: string, staged: boolean, orig?: string, ref?: string) {
  const existing = L.panes(app.root).flatMap((p) => p.tabs).find((t) => t.kind === "diff" && t.path === path && t.staged === staged && t.ref === ref);
  if (existing) return activate(existing.id);
  const p = focusedPane();
  p.tabs.push({ id: L.uid(), kind: "diff", path, staged, orig, ref });
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
  else if (t.kind === "file") disposeEditor(t.id);
  delete app.dirty[t.id];
  delete app.titles[t.id];
  delete app.conflicts[t.id];
}

const parent = (path: string) => path.slice(0, path.lastIndexOf("/"));
const fail = (e: unknown) => message(String(e), { kind: "error" });

export function startCreate(kind: "file" | "dir") {
  const dir = app.selectedDir || app.workspace;
  app.expanded[dir] = true;
  app.creating = { dir, kind };
}

export async function finishCreate(name: string) {
  const c = app.creating;
  app.creating = null;
  name = name.trim();
  if (!c || !name) return;
  if (name.includes("/") || name === "." || name === "..") return fail(`Invalid name: ${name}`);
  const path = `${c.dir}/${name}`;
  try {
    await invoke(c.kind === "file" ? "create_file" : "create_dir", { path });
  } catch (e) {
    return fail(e);
  }
  record({ kind: "create", path });
  app.treeVersion++;
  app.selectedPath = path;
  if (c.kind === "file") openFile(path);
  else app.selectedDir = path;
}

// Undo history for tree operations (in memory, per workspace).
type Op = { kind: "move"; from: string; to: string } | { kind: "create"; path: string } | { kind: "delete"; path: string; held: string };
let history: Op[] = [];
function record(op: Op) {
  history.push(op);
  if (history.length > 50) history.shift();
}
const under = (p: string, root: string) => p === root || p.startsWith(root + "/");

async function doMove(from: string, to: string) {
  try {
    await invoke("move_path", { from, to });
  } catch (e) {
    fail(e);
    return false;
  }
  L.movePath(app.root, from, to);
  if (app.expanded[from]) app.expanded[to] = true;
  if (under(app.selectedDir, from)) app.selectedDir = parent(to);
  if (under(app.selectedPath, from)) app.selectedPath = to;
  app.treeVersion++;
  return true;
}

/** Move a file or folder into `dir` (drag and drop in the tree). */
export async function moveInto(from: string, dir: string) {
  // No-op onto its own folder; a folder can't go inside itself.
  if (parent(from) === dir || under(dir, from)) return;
  const to = `${dir}/${from.slice(from.lastIndexOf("/") + 1)}`;
  if (await doMove(from, to)) record({ kind: "move", from, to });
}

/**
 * Recoverable delete: the backend parks the item in a holding folder (undo moves it back).
 * Closes tabs under `path` first, asking about unsaved ones.
 * Returns the held path; "" if done but not undoable (went to system Trash, or failed); null if the user cancelled.
 */
async function softDelete(path: string): Promise<string | null> {
  const tabsUnder = () => L.panes(app.root).flatMap((p) => p.tabs).filter((t) => t.kind === "file" && under(t.path, path));
  for (const t of tabsUnder()) await closeTab(t.id);
  if (tabsUnder().length) return null;
  let held: string | null;
  try {
    held = await invoke<string | null>("trash_path", { path });
  } catch (e) {
    fail(e);
    return "";
  }
  if (under(app.selectedDir, path)) app.selectedDir = parent(path);
  if (under(app.selectedPath, path)) app.selectedPath = "";
  app.treeVersion++;
  if (held === null) message(`Moved to the Trash: ${path}\n(It's on another disk, so this can't be undone here.)`);
  return held ?? "";
}

export async function deleteSelected() {
  const path = app.selectedPath;
  if (!path || path === app.workspace) return;
  const held = await softDelete(path);
  if (held) record({ kind: "delete", path, held });
}

export async function undoTree() {
  const op = history.pop();
  if (!op) return;
  if (op.kind === "move") await doMove(op.to, op.from);
  else if (op.kind === "delete") (await doMove(op.held, op.path)) && (app.selectedPath = op.path);
  else if ((await softDelete(op.path)) === null) history.push(op); // kept unsaved changes: undo stays available
}
