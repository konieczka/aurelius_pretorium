import { invoke } from "@tauri-apps/api/core";
import { ask, message } from "@tauri-apps/plugin-dialog";
import * as L from "./layout";
import { disposeTerm, queueInput } from "./Terminal.svelte";
import { disposeEditor } from "./Editor.svelte";
import { disposeBrowser, focusAddress } from "./Browser.svelte";
import { hasPreview, isDrawing } from "./media";
import { closeUsage } from "./usageView.svelte";
import { closeSkills } from "./skillsView.svelte";
import { shells } from "./agents.svelte";

/** Opening a tab brings the panes back from the Usage or Skills view. */
const closeViews = () => (closeUsage(), closeSkills());

// localStorage is shared by all windows of the app.
export const store = {
  get: (k: string) => { try { return localStorage.getItem(k); } catch { return null; } },
  set: (k: string, v: string) => { try { localStorage.setItem(k, v); } catch {} },
  remove: (k: string) => { try { localStorage.removeItem(k); } catch {} },
};

export function load<T>(key: string, fallback: T): T {
  try { return JSON.parse(store.get(key) ?? "null") ?? fallback; } catch { return fallback; }
}

// root/minimized/focused/workspace belong to the active workspace (workspaces.svelte.ts swaps them); tabs from any folder can
// sit in it while the sidebar shows another one.
export const app = $state({
  workspace: "", // folder shown in the sidebar; new terminals start here
  root: L.rootWith(L.pane()),
  minimized: [] as L.Minimized[], // terminal tabs parked in the status bar, oldest first
  recent: load<string[]>("recent", []), // opened folders, most recent first
  repos: {} as Record<string, Repo>, // folder -> its git checkout, for the header above each tab
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
  renaming: "", // tree path with the inline rename input
  cut: "", // tree path Cut to the clipboard: pasting it in the tree moves it (elsewhere, e.g. Finder, it's a copy)
  sidebar: "files" as "files" | "git" | "fleet",
  sidebarOpen: load<boolean>("sidebarOpen", true), // ⌘B / the header button hide the whole sidebar
  conflicts: {} as Record<string, number>, // tab id -> conflict blocks in the editor
  gitVersion: 0, // bumped on save / git ops: diff views refetch
  treeFocused: false, // last interaction was in the sidebar: ⌘Z / ⌘⌫ go to the tree, not the editor/terminal
});

const under = (p: string, root: string) => p === root || p.startsWith(root + "/");
/** The opened folder a path belongs to (deepest match); if none, the path itself for a folder, else its parent dir. */
export const ownerOf = (path: string, dir = false) =>
  app.recent.filter((r) => under(path, r)).sort((a, b) => b.length - a.length)[0] ?? (dir ? path : path.slice(0, path.lastIndexOf("/")));
const tabPath = (t: L.Tab) => (t.kind === "term" ? t.cwd : t.kind === "web" ? "" : t.path);
/** Folder a tab belongs to ("" for an unsaved note or a web page). A terminal's follows its shell's `cd`. */
export const tabOwner = (t: L.Tab) => (tabPath(t) ? ownerOf(tabPath(t), t.kind === "term") : "");
/** Folder whose checkout (worktree / branch) a tab shows: a terminal's own directory, as an agent may have moved into
 * a worktree nested in the opened folder (e.g. .claude/worktrees/x); else the folder it belongs to. */
export const repoKey = (t: L.Tab) => (t.kind === "term" ? t.cwd : tabOwner(t));
/** Folder a tab belongs to, or "" when it's in the current workspace. */
export function foreignOwner(t: L.Tab) {
  const p = tabPath(t);
  return !p || under(p, app.workspace) ? "" : tabOwner(t);
}

/** root = toplevel; main = the main checkout's toplevel when this is a linked worktree. "" root = not a repo. */
export type Repo = { root: string; branch: string; main: string };
/** Look up `folder`'s checkout; agents may switch branches at any time, so GitPanel calls this on every poll. */
export async function refreshRepo(folder: string) {
  const git = (args: string[]) => invoke<string>("git", { cwd: folder, args }).then((o) => o.trim());
  try {
    const [[root, gitDir, common], branch] = await Promise.all([
      git(["rev-parse", "--path-format=absolute", "--show-toplevel", "--git-dir", "--git-common-dir"]).then((o) => o.split("\n")),
      git(["branch", "--show-current"]).then((b) => b || git(["rev-parse", "--short", "HEAD"])).catch(() => ""),
    ]);
    const main = gitDir === common ? "" : common.replace(/\/\.git\/?$/, "");
    app.repos[folder] = { root, branch, main };
  } catch {
    app.repos[folder] = { root: "", branch: "", main: "" };
  }
}

/** Every tab of the workspace, minimized ones included. */
export const allTabs = () => [...L.panes(app.root).flatMap((p) => p.tabs), ...app.minimized.map((m) => m.tab)];

export function focusedPane(): L.Pane {
  const all = L.panes(app.root);
  return all.find((p) => p.id === app.focused) ?? all[0];
}

export function activeTab(): L.Tab | undefined {
  const p = focusedPane();
  return p.tabs[p.active];
}

export function toggleSidebar() {
  app.sidebarOpen = !app.sidebarOpen;
  store.set("sidebarOpen", String(app.sidebarOpen));
}

/** ⇧⌘J: show the sidebar's Fleet mode, or hide the sidebar when it's already showing it. */
export function toggleFleet() {
  if (app.sidebarOpen && app.sidebar === "fleet") return toggleSidebar();
  app.sidebar = "fleet";
  if (!app.sidebarOpen) toggleSidebar();
}

/** Share the workspace's space evenly between its panes. */
export const arrangePanes = () => L.equalize(app.root);

const term = (): L.Tab => ({ id: L.uid(), kind: "term", cwd: app.workspace });

// Re-read before changing: another window may have opened or forgotten a folder since this one loaded the list.
const recentList = () => load<string[]>("recent", app.recent);
function setRecent(list: string[]) {
  app.recent = list;
  store.set("recent", JSON.stringify(list));
}

/** Stop tracking `path`: it leaves the folder switcher. Its files and any open tabs are untouched. */
export const forgetFolder = (path: string) => setRecent(recentList().filter((r) => r !== path));

/** Show `path` in the sidebar. Open tabs are untouched. */
export function openWorkspace(path: string) {
  app.workspace = path;
  setRecent([path, ...recentList().filter((r) => r !== path)].slice(0, 15));
  app.selectedDir = path;
  app.creating = null;
  app.selectedPath = "";
  if (!app.minimized.length && !L.panes(app.root).some((p) => p.tabs.length)) app.root = L.rootWith(L.pane([term()]));
  if (!L.panes(app.root).some((p) => p.id === app.focused)) app.focused = L.panes(app.root)[0].id;
}

export function newTerminal() {
  closeViews();
  const p = focusedPane();
  p.tabs.push(term());
  p.active = p.tabs.length - 1;
}

/** Untitled editor tab (path ""); ⌘S asks where to save it. */
export function newNote() {
  closeViews();
  const p = focusedPane();
  p.tabs.push({ id: L.uid(), kind: "file", path: "" });
  p.active = p.tabs.length - 1;
}

/** Browser tab in the focused pane, or right after tab `after` in its pane. No `url`: its address bar waits for one. */
export function openBrowser(url = "", after?: string) {
  closeViews();
  const at = after ? L.findTab(app.root, after) : null;
  const p = at?.pane ?? focusedPane();
  const t: L.Tab = { id: L.uid(), kind: "web", url };
  p.tabs.splice(at ? at.index + 1 : p.tabs.length, 0, t);
  p.active = p.tabs.indexOf(t);
  app.focused = p.id;
  if (!url) setTimeout(() => focusAddress(t.id)); // once it's mounted
}

/** Browser tab in a new pane to the right of tab `beside`'s (a link opened from a terminal). */
export function openBrowserBeside(url: string, beside: string) {
  const at = L.findTab(app.root, beside);
  if (!at) return openBrowser(url);
  closeViews();
  app.focused = L.split(app.root, at.pane.id, "row", { id: L.uid(), kind: "web", url }).id;
}

/** New terminal tab that runs `command` once its shell starts: in the focused pane, or the rightmost one. Returns its id. */
export function runInTerminal(cwd: string, command: string, where: "focused" | "rightmost" = "focused") {
  closeViews();
  const p = where === "rightmost" ? L.rightmost(app.root) : focusedPane();
  const t: L.Tab = { id: L.uid(), kind: "term", cwd };
  queueInput(t.id, command + "\r");
  p.tabs.push(t);
  p.active = p.tabs.length - 1;
  app.focused = p.id;
  return t.id;
}

export function splitFocused(dir: "row" | "col") {
  closeViews();
  app.focused = L.split(app.root, focusedPane().id, dir, term()).id;
}

export function openFile(path: string) {
  closeViews();
  const existing = L.panes(app.root).flatMap((p) => p.tabs).find((t) => t.kind === "file" && t.path === path);
  if (existing) {
    if (existing.kind === "file" && isDrawing(path)) existing.preview = true; // a drawing opens as one, not its JSON
    return activate(existing.id);
  }
  const p = focusedPane();
  p.tabs.push({ id: L.uid(), kind: "file", path, preview: hasPreview(path) });
  p.active = p.tabs.length - 1;
}

/** Diff tab: staged = index vs HEAD, else working tree vs index; with `ref`, that commit's change. */
export function openDiff(path: string, staged: boolean, orig?: string, ref?: string, base?: string) {
  closeViews();
  const existing = L.panes(app.root).flatMap((p) => p.tabs).find((t) => t.kind === "diff" && t.path === path && t.staged === staged && t.ref === ref && t.base === base);
  if (existing) return activate(existing.id);
  const p = focusedPane();
  p.tabs.push({ id: L.uid(), kind: "diff", path, staged, orig, ref, base });
  p.active = p.tabs.length - 1;
}

export function activate(tabId: string) {
  const f = L.findTab(app.root, tabId);
  if (!f) return;
  f.pane.active = f.index;
  app.focused = f.pane.id;
}

/** Closing `t` loses work: unsaved edits, or a terminal's agent (closing ends the shell). Ask first. */
async function okToClose(t: L.Tab) {
  if (app.dirty[t.id]) return ask("Discard unsaved changes?", { kind: "warning" });
  const agent = t.kind === "term" && shells.all[t.id]?.agent;
  if (!agent) return true;
  return ask(`${agent} is running in this terminal. Closing it stops ${agent}${shells.all[t.id].busy ? " in the middle of its work" : ""}.`, {
    title: "Close terminal?", kind: "warning", okLabel: "Close", cancelLabel: "Cancel",
  });
}

export async function closeTab(tabId: string) {
  const mi = app.minimized.findIndex((m) => m.tab.id === tabId);
  if (mi >= 0) {
    if (!(await okToClose(app.minimized[mi].tab))) return;
    const i = app.minimized.findIndex((m) => m.tab.id === tabId); // may have moved while asking
    if (i >= 0) dispose(app.minimized.splice(i, 1)[0].tab);
    return;
  }
  const t = L.findTab(app.root, tabId)?.pane.tabs.find((t) => t.id === tabId);
  if (!t || !(await okToClose(t))) return;
  if (!L.findTab(app.root, tabId)) return; // closed some other way while asking
  dispose(t);
  L.closeTab(app.root, tabId);
  fixFocus();
}

const fixFocus = () => L.panes(app.root).some((p) => p.id === app.focused) || (app.focused = L.panes(app.root)[0].id);

/** Park a tab in the status bar; its shell keeps running. */
export function minimizeTab(tabId: string) {
  const m = L.minimize(app.root, tabId);
  if (!m) return;
  app.minimized.push(m);
  fixFocus();
}

/** Put a minimized tab back where it was (or as near as the layout now allows) and focus it. */
export function restoreTab(tabId: string) {
  const i = app.minimized.findIndex((m) => m.tab.id === tabId);
  if (i < 0) return;
  closeViews();
  const [m] = app.minimized.splice(i, 1);
  app.focused = L.restore(app.root, m).id;
}

/** Drop a tab's view; `kill = false` leaves a terminal's shell or a page running (its workspace moved or was closed). */
export function dispose(t: L.Tab, kill = true) {
  if (t.kind === "term") disposeTerm(t.id, kill);
  else if (t.kind === "web") disposeBrowser(t.id, kill);
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

/** Tree rename (inline input). */
export async function finishRename(from: string, name: string) {
  if (app.renaming !== from) return; // already finished (Enter, then the blur from unmounting)
  app.renaming = "";
  name = name.trim();
  if (!name || name === from.slice(from.lastIndexOf("/") + 1)) return;
  if (name.includes("/") || name === "." || name === "..") return fail(`Invalid name: ${name}`);
  const to = `${parent(from)}/${name}`;
  if (await doMove(from, to)) record({ kind: "move", from, to });
}

/** Tree Cut/Copy: the file goes on the system clipboard, like Finder's Copy. */
export async function clipFile(path: string, cut: boolean) {
  try {
    await invoke("clipboard_set_file", { path });
  } catch (e) {
    return fail(e);
  }
  app.cut = cut ? path : "";
}

export const clipboardFiles = () => invoke<string[]>("clipboard_files");

/** Paste the clipboard's files (from the tree, Finder...) into `dir`: our own Cut moves, the rest is copied. */
export async function paste(dir: string) {
  for (const from of await clipboardFiles()) {
    if (from === app.cut) {
      app.cut = "";
      await moveInto(from, dir);
      continue;
    }
    if (under(dir, from)) {
      fail("Can't paste a folder into itself.");
      continue;
    }
    let path: string;
    try {
      path = await invoke<string>("copy_path", { from, dir });
    } catch (e) {
      fail(e);
      continue;
    }
    record({ kind: "create", path });
    app.expanded[dir] = true;
    app.selectedPath = path;
    app.treeVersion++;
  }
}

/** Resolves to whether it worked (a failure is shown). */
export const copyText = (text: string) => navigator.clipboard.writeText(text).then(() => true, (e) => (fail(e), false));

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
