import { invoke } from "@tauri-apps/api/core";
import { cursorPosition, getCurrentWindow, Window } from "@tauri-apps/api/window";
import { emitTo } from "@tauri-apps/api/event";
import { getAllWebviewWindows, getCurrentWebviewWindow, WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { ask, message } from "@tauri-apps/plugin-dialog";
import * as L from "./layout";
import { app, dispose, load, openWorkspace, store } from "./state.svelte";
import { draftOf, queueDraft } from "./Editor.svelte";
import { theme } from "./theme.svelte";

// A workspace is a named arrangement of panes and tabs, plus the folder its sidebar shows.
// Each one is open in at most one window at a time (terminal views live in a window); its shells live in the
// backend, so a closed or moved workspace's terminals keep running and re-attach when it's shown again.
// Storage (localStorage, shared by all windows):
//   "workspaces"  [{id, name}] in creation order; any window writes it (create / rename / delete)
//   "ws:<id>"     {folder, root, focused, drafts?}; written only by the window that has it open. drafts = unsaved
//                 editor text (tab id -> text) carried while the workspace isn't shown in any window
//   "win:<label>" {open, active}: a window's workspace tabs. Removed when the window is closed, kept on quit,
//                 so on the next launch the main window reopens the others.
type Meta = { id: string; name: string };
type Layout = { folder: string; root: L.Split; focused: string; drafts?: Record<string, string> };
type Win = { open: string[]; active: string };

export const ws = $state({
  all: load<Meta[]>("workspaces", []),
  open: [] as string[],
  active: "",
  incoming: "", // workspace being dragged over this window's bar (another window's tear-off): drop merges it here
});

const win = getCurrentWindow();
const held = new Map<string, Layout>(); // this window's open but inactive workspaces

const saveMeta = () => store.set("workspaces", JSON.stringify(ws.all));
const snapshot = (): Layout => ({ folder: app.workspace, root: app.root, focused: app.focused });
const tabsOf = (l: Layout) => L.panes(l.root).flatMap((p) => p.tabs);
export const nameOf = (id: string) => ws.all.find((m) => m.id === id)?.name ?? "";

/** Persist the active workspace and this window's tabs (run as an effect: re-runs on any layout change). */
export function save() {
  if (!ws.active) return;
  store.set("ws:" + ws.active, JSON.stringify(snapshot()));
  store.set("win:" + win.label, JSON.stringify({ open: ws.open, active: ws.active } satisfies Win));
}

/** Label of the other window that has `id` open, if any. */
export function ownerWindow(id: string) {
  for (let i = 0; i < localStorage.length; i++) {
    const k = localStorage.key(i)!;
    if (k.startsWith("win:") && k !== "win:" + win.label && load<Win | null>(k, null)?.open.includes(id)) return k.slice(4);
  }
  return "";
}

/** Show workspace `id` in this window, adding it to the tab bar if needed. */
export function activate(id: string) {
  if (id === ws.active) return;
  if (ws.active) held.set(ws.active, snapshot()), save();
  const l = held.get(id) ?? load<Layout | null>("ws:" + id, null) ?? { folder: app.workspace, root: L.rootWith(L.pane()), focused: "" };
  held.delete(id);
  for (const [tab, text] of Object.entries(l.drafts ?? {})) queueDraft(tab, text);
  if (!ws.open.includes(id)) ws.open.push(id);
  ws.active = id;
  app.root = l.root;
  app.focused = l.focused;
  if (l.folder) openWorkspace(l.folder); // also seeds a terminal into an empty layout
  else app.workspace = "";
  win.setTitle(nameOf(id));
}

/** Open `id` here, or bring up the window that already has it. */
export async function openHere(id: string) {
  const other = ownerWindow(id);
  if (other) return (await Window.getByLabel(other))?.setFocus();
  activate(id);
}

export function createWorkspace() {
  const id = L.uid();
  let n = 1;
  while (ws.all.some((m) => m.name === `Workspace ${n}`)) n++;
  ws.all.push({ id, name: `Workspace ${n}` });
  saveMeta();
  store.set("ws:" + id, JSON.stringify({ folder: app.workspace || app.recent[0] || "", root: L.rootWith(L.pane()), focused: "" }));
  activate(id);
}

export function renameWorkspace(id: string, name: string) {
  const m = ws.all.find((m) => m.id === id);
  if (!m || !name.trim()) return;
  m.name = name.trim();
  saveMeta();
  if (id === ws.active) win.setTitle(m.name);
}

/** Persist `id` with its unsaved editor text, so whichever window shows it next gets both. */
function park(id: string) {
  const l = id === ws.active ? snapshot() : held.get(id);
  if (!l) return [];
  const tabs = tabsOf(l);
  const drafts = Object.fromEntries(tabs.flatMap((t) => (t.kind === "file" && draftOf(t.id) !== undefined ? [[t.id, draftOf(t.id)!]] : [])));
  store.set("ws:" + id, JSON.stringify({ ...l, drafts }));
  return tabs;
}

/** Take `id` out of this window. Its shells keep running and unsaved edits are kept for when it's shown again. */
function release(id: string) {
  const tabs = park(id);
  const i = ws.open.indexOf(id);
  if (id === ws.active) activate(ws.open[i + 1] ?? ws.open[i - 1]);
  held.delete(id);
  ws.open.splice(ws.open.indexOf(id), 1);
  for (const t of tabs) dispose(t, false);
  save();
}

/** Close a workspace tab (the workspace stays in the list; reopen it from +). A window keeps at least one. */
export async function closeWorkspace(id: string) {
  if (ws.open.length > 1) release(id);
}

export async function deleteWorkspace(id: string) {
  if (ownerWindow(id)) return message(`“${nameOf(id)}” is open in another window. Delete it from there.`);
  if (!(await ask(`Delete workspace “${nameOf(id)}”? Its terminals will be closed and unsaved changes discarded.`, { kind: "warning" }))) return;
  const l = id === ws.active ? snapshot() : (held.get(id) ?? load<Layout | null>("ws:" + id, null));
  if (ws.open.includes(id)) {
    if (ws.open.length === 1) createWorkspace();
    release(id);
  }
  if (l) for (const t of tabsOf(l)) dispose(t); // kills shells, also ones no window is showing
  ws.all = ws.all.filter((m) => m.id !== id);
  saveMeta();
  store.remove("ws:" + id);
}

/** Move `id` into a new window next to this one (menu action; dragging a tab out does this live, see tearOut). */
export async function openInNewWindow(id: string) {
  if (ws.open.length < 2) return;
  release(id);
  const label = "w-" + L.uid().slice(0, 8);
  store.set("win:" + label, JSON.stringify({ open: [id], active: id } satisfies Win));
  try {
    // Sizes and positions from Tauri, not the DOM: WKWebView reports window.outerWidth/screenX as 0.
    const scale = await win.scaleFactor();
    const size = (await win.innerSize()).toLogical(scale);
    const here = (await win.outerPosition()).toLogical(scale);
    newWindow(label, { x: here.x + 40, y: here.y + 40, width: Math.max(size.width, 800), height: Math.max(size.height, 500) });
  } catch (e) {
    newWindow(label, { width: 1400, height: 900 });
    message(`Couldn't place the new window: ${e}`, { kind: "warning" });
  }
}

// Chrome-style tear-off: once a tab is dragged off the bar, its workspace's window follows the cursor until the
// mouse button is released. The following runs in the backend (follow_cursor), which tells us when it's dropped.

/** Start a tear-off. `grab` = where the cursor sits in the follower's content area, logical px. */
export async function tearOut(id: string, grab: { x: number; y: number }) {
  const scale = await win.scaleFactor();
  const titleBar = (await win.innerPosition()).y - (await win.outerPosition()).y;
  const [dx, dy] = [grab.x * scale, grab.y * scale + titleBar];
  let label = win.label; // the window's only workspace: move the window itself
  if (ws.open.length > 1) {
    const size = (await win.innerSize()).toLogical(scale);
    const c = await cursorPosition();
    release(id);
    label = "w-" + L.uid().slice(0, 8);
    store.set("win:" + label, JSON.stringify({ open: [id], active: id } satisfies Win));
    newWindow(label, { x: (c.x - dx) / scale, y: (c.y - dy) / scale, width: size.width, height: size.height, focus: false });
  }
  invoke("follow_cursor", { label, id, source: win.label, dx, dy });
}

/** Our tear-off was released: over another window's bar (`target`, found by the backend) → merge into it, else the
 * follower stays where it was dropped. */
async function tearDropped({ label, target }: { label: string; target: string | null }) {
  if (!target) return (await Window.getByLabel(label))?.setFocus();
  if (label === win.label) return mergeInto(target);
  // The torn-off window holds the workspace (and its unsaved edits) now, so it merges itself. It may still be
  // starting up: it also checks this key on boot.
  store.set("merge:" + label, target);
  emitTo(label, "merge-into", target);
}

/** Move this window's active workspace into window `target`; an emptied window closes. */
async function mergeInto(target: string) {
  store.remove("merge:" + win.label);
  const id = ws.active;
  if (!id) return;
  if (ws.open.length > 1) release(id);
  else {
    park(id);
    ws.open = [];
    ws.active = "";
    store.remove("win:" + win.label);
  }
  await emitTo(target, "adopt-workspace", id);
  if (!ws.open.length) win.destroy();
}

function newWindow(label: string, opts: { x?: number; y?: number; width: number; height: number; focus?: boolean }) {
  // Background = the theme's surface (as in Terminal.svelte), so a torn-off window doesn't flash white while loading.
  const backgroundColor = theme.dark ? "#110d07" : "#fff9ee";
  const w = new WebviewWindow(label, { url: "/", title: "Fleet Command", dragDropEnabled: false, focus: true, backgroundColor, ...opts });
  w.once("tauri://error", (e) => message(`Couldn't open the window: ${JSON.stringify(e.payload)}`, { kind: "error" }));
  return w;
}

/** Reorder this window's workspace tabs: `id` takes `to`'s place. */
export function moveWorkspace(id: string, to: string) {
  const [i, j] = [ws.open.indexOf(id), ws.open.indexOf(to)];
  if (i < 0 || j < 0 || i === j) return;
  ws.open.splice(i, 1);
  ws.open.splice(j, 0, id);
}

export function initWorkspaces() {
  // First run with workspaces: the single shared layout becomes "Workspace 1".
  if (!ws.all.length) {
    const id = L.uid();
    const folder = store.get("workspace") ?? "";
    const root = load<L.Split | null>("layout", load<L.Split | null>("layout:" + folder, null)) ?? L.rootWith(L.pane());
    ws.all = [{ id, name: "Workspace 1" }];
    saveMeta();
    store.set("ws:" + id, JSON.stringify({ folder, root, focused: "" }));
    store.set("win:main", JSON.stringify({ open: [id], active: id }));
  }

  const w = load<Win | null>("win:" + win.label, null);
  const open = (w?.open ?? []).filter((id) => ws.all.some((m) => m.id === id) && !ownerWindow(id));
  if (!open.length) {
    const free = ws.all.find((m) => !ownerWindow(m.id));
    if (!free) return createWorkspace();
    open.push(free.id);
  }
  ws.open = open;
  activate(w && open.includes(w.active) ? w.active : open[0]);

  // Another window dropped one of its workspaces onto this one.
  getCurrentWebviewWindow().listen<string>("adopt-workspace", ({ payload }) => (activate(payload), win.setFocus()));
  // Tear-offs: ours was released / we were torn off and dropped onto another window's bar.
  getCurrentWebviewWindow().listen<{ label: string; target: string | null }>("tear-dropped", ({ payload }) => tearDropped(payload));
  getCurrentWebviewWindow().listen<string | null>("merge-hint", ({ payload }) => (ws.incoming = payload ?? ""));
  getCurrentWebviewWindow().listen<string>("merge-into", ({ payload }) => mergeInto(payload));
  const merge = store.get("merge:" + win.label);
  if (merge) mergeInto(merge);

  // Names and deletions made in other windows.
  window.addEventListener("storage", (e) => e.key === "workspaces" && (ws.all = load("workspaces", [])));

  // Closing a window (not quitting) gives its workspaces back; the last window's are kept to reopen on launch.
  win.onCloseRequested(async () => {
    save();
    if ((await getAllWebviewWindows()).length > 1) store.remove("win:" + win.label);
  });

  // Reopen the other windows from last session (skipped when it's just the main window reloading).
  if (win.label === "main")
    for (let i = 0; i < localStorage.length; i++) {
      const k = localStorage.key(i)!;
      if (!k.startsWith("win:") || k === "win:main") continue;
      const label = k.slice(4);
      WebviewWindow.getByLabel(label).then((existing) => existing || newWindow(label, { width: 1400, height: 900 }));
    }
}
