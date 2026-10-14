import { invoke } from "@tauri-apps/api/core";
import { cursorPosition, getCurrentWindow, LogicalPosition, Window } from "@tauri-apps/api/window";
import { emit, emitTo, listen } from "@tauri-apps/api/event";
import { getAllWebviewWindows, getCurrentWebviewWindow, WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { ask, message } from "@tauri-apps/plugin-dialog";
import * as L from "./layout";
import { activate as activateTab, allTabs, app, dispose, load, openWorkspace, restoreTab, store, tabOwner } from "./state.svelte";
import { focusTerm } from "./Terminal.svelte";
import { closeUsage } from "./usageView.svelte";
import { closeSkills } from "./skillsView.svelte";
import { closeAgents } from "./agentsView.svelte";
import { closeSettings } from "./settingsView.svelte";
import { draftOf, queueDraft } from "./Editor.svelte";
import { shells } from "./agents.svelte";
import { applyWorkspacePalette, cssHex, theme } from "./theme.svelte";
import { paletteOverride, resolvePalette, type Palette } from "./palette";

// A workspace is a named arrangement of panes and tabs, plus the folder its sidebar shows.
// Each one is open in at most one window at a time (terminal views live in a window); its shells live in the
// backend, so a closed or moved workspace's terminals keep running and re-attach when it's shown again.
// Storage (localStorage, shared by all windows):
//   "workspaces"  [{id, name, palette?}] in creation order; any window writes it (create / rename / delete /
//                 per-workspace palette). `palette` is that workspace's own colour scheme; absent = the global
//                 palette (theme.svelte.ts) is its fallback.
//   "ws:<id>"     {folder, root, focused, minimized?, drafts?}; written only by the window that has it open. drafts = unsaved
//                 editor text (tab id -> text) carried while the workspace isn't shown in any window
//   "win:<label>" {open, active}: a window's workspace tabs. Removed when the window is closed, kept on quit,
//                 so on the next launch the main window reopens the others.
type Meta = { id: string; name: string; palette?: Palette };
type Layout = { folder: string; root: L.Split; focused: string; minimized?: L.Minimized[]; drafts?: Record<string, string> };
type Win = { open: string[]; active: string };

// Storage is read back defensively: anything malformed is dropped rather than breaking the window.
const loadMeta = (): Meta[] => {
  const all = load<unknown>("workspaces", []);
  if (!Array.isArray(all)) return [];
  return all.flatMap((m): Meta[] => {
    if (!m || typeof m !== "object") return [];
    const { id, name, palette } = m as Partial<Meta>;
    if (typeof id !== "string" || typeof name !== "string") return [];
    const own = paletteOverride(palette);
    return [own ? { id, name, palette: own } : { id, name }];
  });
};
function loadLayout(id: string): Layout | null {
  const l = load<Partial<Layout> | null>("ws:" + id, null);
  if (!l || !L.isRoot(l.root)) return null;
  return {
    folder: typeof l.folder === "string" ? l.folder : "",
    root: l.root,
    focused: typeof l.focused === "string" ? l.focused : "",
    minimized: Array.isArray(l.minimized) ? l.minimized.filter(L.isMinimized) : [],
    drafts: l.drafts && typeof l.drafts === "object" ? l.drafts : undefined,
  };
}
function loadWin(key: string): Win | null {
  const w = load<Partial<Win> | null>(key, null);
  if (!w || !Array.isArray(w.open)) return null;
  return { open: w.open.filter((id) => typeof id === "string"), active: typeof w.active === "string" ? w.active : "" };
}

export const ws = $state({
  all: loadMeta(),
  open: [] as string[],
  active: "",
  incoming: "", // workspace being dragged over this window's bar (another window's tear-off): drop merges it here
});

const win = getCurrentWindow();
const held = new Map<string, Layout>(); // this window's open but inactive workspaces

const saveMeta = () => store.set("workspaces", JSON.stringify(ws.all));
const snapshot = (): Layout => ({ folder: app.workspace, root: app.root, focused: app.focused, minimized: app.minimized });
const tabsOf = (l: Layout) => [...L.panes(l.root).flatMap((p) => p.tabs), ...(l.minimized ?? []).map((m) => m.tab)];
export const nameOf = (id: string) => ws.all.find((m) => m.id === id)?.name ?? "";

/** Workspace `id`'s own palette, or undefined when it follows the global palette. */
export const paletteOf = (id: string): Palette | undefined => paletteOverride(ws.all.find((m) => m.id === id)?.palette);

/** The palette workspace `id` is shown in: its own, else the global fallback. */
export const effectivePaletteOf = (id: string): Palette => resolvePalette(paletteOf(id), theme.globalPalette);

/** Give workspace `id` its own palette, or clear it (null) so it follows the global palette again. */
export function setWorkspacePalette(id: string, palette: Palette | null) {
  const m = ws.all.find((m) => m.id === id);
  if (!m) return;
  if (palette) m.palette = palette;
  else delete m.palette;
  saveMeta();
  if (id === ws.active) applyWorkspacePalette(palette);
}

// This window is going away (closed, reloaded, quitting) and has put its workspaces in storage with their unsaved
// edits (parkAll): from here on nothing may overwrite that.
let leaving = false;

/** Persist the active workspace and this window's tabs (run as an effect: re-runs on any layout change). */
export function save() {
  if (!ws.active || leaving) return;
  store.set("ws:" + ws.active, JSON.stringify(snapshot()));
  store.set("win:" + win.label, JSON.stringify({ open: ws.open, active: ws.active } satisfies Win));
}

/** Label of the other window that has `id` open, if any. */
export function ownerWindow(id: string) {
  for (let i = 0; i < localStorage.length; i++) {
    const k = localStorage.key(i)!;
    if (k.startsWith("win:") && k !== "win:" + win.label && loadWin(k)?.open.includes(id)) return k.slice(4);
  }
  return "";
}

/** Show workspace `id` in this window, adding it to the tab bar if needed. */
export function activate(id: string) {
  if (id === ws.active) return;
  if (ws.active) held.set(ws.active, snapshot()), save();
  const l = held.get(id) ?? loadLayout(id) ?? { folder: app.workspace, root: L.rootWith(L.pane()), focused: "" };
  held.delete(id);
  for (const [tab, text] of Object.entries(l.drafts ?? {})) queueDraft(tab, text);
  if (!ws.open.includes(id)) ws.open.push(id);
  ws.active = id;
  app.root = l.root;
  app.minimized = l.minimized ?? [];
  app.focused = l.focused;
  if (l.folder) openWorkspace(l.folder); // also seeds a terminal into an empty layout
  else app.workspace = "";
  applyWorkspacePalette(paletteOf(id) ?? null); // this workspace's own palette, else the global fallback
  win.setTitle(nameOf(id));
}

/** Every tab of workspace `id`, minimized ones included, wherever its layout is: shown or held here, or in storage
 * (another window's, which saves it on every change, or a closed one's). */
export function tabsIn(id: string): L.Tab[] {
  if (id === ws.active) return allTabs();
  const l = held.get(id) ?? loadLayout(id);
  return l ? tabsOf(l) : [];
}

/** Bring up tab `tabId` of workspace `id`: in the window that has the workspace open, else here. */
export async function showTab(id: string, tabId: string) {
  const other = ownerWindow(id);
  if (other) {
    await emitTo(other, "show-tab", { id, tab: tabId });
    return (await Window.getByLabel(other))?.setFocus();
  }
  activate(id);
  reveal(tabId);
}

function reveal(tabId: string) {
  closeUsage(), closeSkills(), closeAgents(), closeSettings(); // the panes come back from a full-page view

  if (app.minimized.some((m) => m.tab.id === tabId)) restoreTab(tabId);
  else activateTab(tabId);
  setTimeout(() => focusTerm(tabId)); // a terminal that was already its pane's active one doesn't remount (and focus)
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

/** Add `tab` to a workspace's layout, in its focused pane. */
function addTo(l: Layout, tab: L.Tab) {
  const all = L.panes(l.root);
  const p = all.find((p) => p.id === l.focused) ?? all[0];
  p.tabs.push(tab);
  p.active = p.tabs.length - 1;
}

/** Hand `tab` to workspace `id`, wherever it is: active or in the background here, open in another window, or closed.
 * `draft`: the tab's unsaved editor text, which goes with it. */
function deliver(id: string, tab: L.Tab, draft?: string) {
  const here = id === ws.active || held.has(id);
  if (here && draft !== undefined) queueDraft(tab.id, draft);
  if (id === ws.active) return addTo(snapshot(), tab);
  const h = held.get(id);
  if (h) return addTo(h, tab), store.set("ws:" + id, JSON.stringify(h)); // held ones are saved only when left
  const other = ownerWindow(id);
  if (other) return void emitTo(other, "adopt-tab", { id, tab, draft });
  const l = loadLayout(id) ?? { folder: tab.kind === "term" ? tabOwner(tab) : "", root: L.rootWith(L.pane()), focused: "" };
  addTo(l, tab);
  if (draft !== undefined) l.drafts = { ...l.drafts, [tab.id]: draft };
  store.set("ws:" + id, JSON.stringify(l));
}

/** Move a tab of the active workspace to workspace `to` ("" = a new one, opened in the background here). A terminal's
 * shell keeps running and re-attaches there. */
export function moveTabTo(tabId: string, to: string) {
  const f = L.findTab(app.root, tabId);
  if (!f) return;
  const tab = $state.snapshot(f.pane.tabs[f.index]) as L.Tab;
  const draft = tab.kind === "file" ? draftOf(tab.id) : undefined; // unsaved edits go with it
  L.closeTab(app.root, tabId);
  if (!L.panes(app.root).some((p) => p.id === app.focused)) app.focused = L.panes(app.root)[0].id;
  dispose(tab, false);
  if (to) return deliver(to, tab, draft);
  const id = L.uid();
  let n = 1;
  while (ws.all.some((m) => m.name === `Workspace ${n}`)) n++;
  ws.all.push({ id, name: `Workspace ${n}` });
  saveMeta();
  const fresh = L.pane([tab]);
  const l = { folder: (tab.kind === "term" ? tabOwner(tab) : "") || app.workspace, root: L.rootWith(fresh), focused: fresh.id };
  held.set(id, l);
  store.set("ws:" + id, JSON.stringify(l));
  ws.open.push(id);
  if (draft !== undefined) queueDraft(tab.id, draft);
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
  const i = ws.open.indexOf(id);
  // Switch away first: leaving a workspace saves it (without drafts), which would overwrite what park stores.
  if (id === ws.active) activate(ws.open[i + 1] ?? ws.open[i - 1]);
  const tabs = park(id);
  held.delete(id);
  ws.open.splice(ws.open.indexOf(id), 1);
  for (const t of tabs) dispose(t, false);
  save();
}

/** Close a workspace tab (the workspace stays in the list; reopen it from +). A window keeps at least one. */
export async function closeWorkspace(id: string) {
  if (ws.open.length > 1) release(id);
}

/** Delete `id` for good, once confirmed (wherever it is but another window). Resolves to whether it was deleted. */
export async function deleteWorkspace(id: string) {
  if (ownerWindow(id)) return message(`“${nameOf(id)}” is open in another window. Delete it from there.`), false;
  if (!(await ask(`Delete workspace “${nameOf(id)}”? Its terminals will be closed and unsaved changes discarded.`, { kind: "warning" }))) return false;
  const l = id === ws.active ? snapshot() : (held.get(id) ?? loadLayout(id));
  if (ws.open.includes(id)) {
    if (ws.open.length === 1) createWorkspace();
    release(id);
  }
  if (l) for (const t of tabsOf(l)) dispose(t); // kills shells, also ones no window is showing
  ws.all = ws.all.filter((m) => m.id !== id);
  saveMeta();
  store.remove("ws:" + id);
  return true;
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
    newWindow(label, { x: here.x + 40, y: here.y + 40, width: Math.max(size.width, 800), height: Math.max(size.height, 500) }, true, id);
  } catch (e) {
    newWindow(label, { width: 1400, height: 900 }, true, id);
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
  if (label === win.label && ws.open.length === 1 && (await win.isFullscreen())) return; // a full-screen window stays put
  if (ws.open.length > 1) {
    const size = (await win.innerSize()).toLogical(scale);
    const c = await cursorPosition();
    release(id);
    label = "w-" + L.uid().slice(0, 8);
    store.set("win:" + label, JSON.stringify({ open: [id], active: id } satisfies Win));
    newWindow(label, { x: (c.x - dx) / scale, y: (c.y - dy) / scale, width: size.width, height: size.height, focus: false }, false, id);
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

/** Open a window for `label`. It starts hidden and the backend shows it (`show`: right away; else follow_cursor does,
 * for a tear-off): created visible while the app is in full screen, it would hang the app (see show_window). */
function newWindow(label: string, opts: { x?: number; y?: number; width: number; height: number; focus?: boolean }, show = true, forId?: string) {
  // Background = the surface of the palette the new window will show (its workspace's own, else the global),
  // so a torn-off window doesn't flash white (or another workspace's colour) while loading.
  const pal = forId ? effectivePaletteOf(forId) : theme.palette;
  const el = document.createElement("div");
  el.dataset.palette = pal;
  el.hidden = true;
  document.documentElement.appendChild(el);
  let backgroundColor: string;
  try {
    backgroundColor = cssHex("--surface", el).slice(0, 7);
  } finally {
    el.remove();
  }
  const w = new WebviewWindow(label, {
    url: "/", title: "Pretorium", dragDropEnabled: false, focus: true, backgroundColor,
    // Same chrome as the main window (tauri.conf.json): the top bar is the title bar
    titleBarStyle: "overlay", hiddenTitle: true, trafficLightPosition: new LogicalPosition(18, 22),
    ...opts,
    visible: false,
  });
  if (show) w.once("tauri://created", () => invoke("show_window", { label }));
  w.once("tauri://error", (e) => {
    store.remove("win:" + label); // else its workspaces stay "open in another window" that doesn't exist
    message(`Couldn't open the window: ${JSON.stringify(e.payload)}`, { kind: "error" });
  });
  return w;
}

/** Reorder this window's workspace tabs: `id` takes `to`'s place. */
export function moveWorkspace(id: string, to: string) {
  const [i, j] = [ws.open.indexOf(id), ws.open.indexOf(to)];
  if (i < 0 || j < 0 || i === j) return;
  ws.open.splice(i, 1);
  ws.open.splice(j, 0, id);
}

/** Put every workspace of this window in storage with its unsaved edits (they reopen with them): the window is going
 * away, closed, reloaded or quitting. */
function parkAll() {
  save();
  leaving = true;
  for (const id of ws.open) park(id);
}

/** Quitting (or restarting, or closing the last window) stops every shell: ask first if agents are still working. */
async function confirmQuit(restart = false) {
  const n = Object.values(shells.all).filter((s) => s.busy).length;
  if (!n) return true;
  const [verb, Verb] = restart ? ["Restarting", "Restart"] : ["Quitting", "Quit"];
  return ask(`${n === 1 ? "An agent is" : `${n} agents are`} still working. ${verb} stops ${n === 1 ? "it" : "them"}.`, {
    title: `${Verb} Pretorium?`, kind: "warning", okLabel: Verb, cancelLabel: "Cancel",
  });
}

/** ⌘Q (the backend's Quit sends it to one window): ask, have every window store its workspaces, then quit. */
async function quit(restart = false) {
  if (!(await confirmQuit(restart))) return;
  let left = (await getAllWebviewWindows()).length;
  await new Promise<void>((done) => {
    const timeout = setTimeout(done, 1500); // a window that doesn't answer mustn't keep the app from quitting
    listen("parked", () => --left || (clearTimeout(timeout), done())).then(() => emit("park"));
  });
  invoke("quit", { restart });
}

/** Restart into a freshly installed update, the way ⌘Q quits (agents asked about, unsaved edits kept). */
export const restartApp = () => quit(true);

/** Start this window's workspaces. If what's stored can't be used, offer to start over instead of a blank window. */
export async function initWorkspaces() {
  try {
    init();
  } catch (e) {
    console.error(e);
    const reset = await ask(`Pretorium couldn't restore your workspaces (${e}).\n\nReset them? Your files and folders aren't touched; open tabs and layouts are.`, {
      title: "Couldn't restore workspaces", kind: "error", okLabel: "Reset", cancelLabel: "Quit",
    });
    if (!reset) return invoke("quit");
    for (const k of Object.keys(localStorage)) if (k === "workspaces" || /^(ws|win|merge):/.test(k)) store.remove(k);
    location.reload();
  }
}

function init() {
  // First run with workspaces: the single shared layout becomes "Workspace 1".
  if (!ws.all.length) {
    const id = L.uid();
    const folder = store.get("workspace") ?? "";
    const legacy = load<unknown>("layout", load<unknown>("layout:" + folder, null));
    const root = L.isRoot(legacy) ? legacy : L.rootWith(L.pane());
    ws.all = [{ id, name: "Workspace 1" }];
    saveMeta();
    store.set("ws:" + id, JSON.stringify({ folder, root, focused: "" }));
    store.set("win:main", JSON.stringify({ open: [id], active: id }));
  }
  // The main window always opens on launch, even if it was closed before quitting: then it becomes one of the windows
  // that were still open, rather than coming back with a workspace no window had.
  if (win.label === "main" && !store.get("win:main")) {
    const k = Object.keys(localStorage).find((k) => k.startsWith("win:"));
    if (k) store.set("win:main", store.get(k)!), store.remove(k);
  }

  const w = loadWin("win:" + win.label);
  const open = (w?.open ?? []).filter((id) => ws.all.some((m) => m.id === id) && !ownerWindow(id));
  if (!open.length) {
    const free = ws.all.find((m) => !ownerWindow(m.id));
    if (free) open.push(free.id);
  }
  // Everything else is open elsewhere: a fresh one. (Not a return: the listeners below, the close handler and
  // reopening last session's windows must still be set up.)
  if (!open.length) createWorkspace();
  else {
    ws.open = open;
    activate(w && open.includes(w.active) ? w.active : open[0]);
  }

  // Another window dropped one of its workspaces onto this one.
  getCurrentWebviewWindow().listen<string>("adopt-workspace", ({ payload }) => (activate(payload), win.setFocus()));
  // Tear-offs: ours was released / we were torn off and dropped onto another window's bar.
  getCurrentWebviewWindow().listen<{ label: string; target: string | null }>("tear-dropped", ({ payload }) => tearDropped(payload));
  getCurrentWebviewWindow().listen<string | null>("merge-hint", ({ payload }) => (ws.incoming = payload ?? ""));
  getCurrentWebviewWindow().listen<string>("merge-into", ({ payload }) => mergeInto(payload));
  getCurrentWebviewWindow().listen<{ id: string; tab: L.Tab; draft?: string }>("adopt-tab", ({ payload: { id, tab, draft } }) => {
    if (ws.open.includes(id)) deliver(id, tab, draft);
    else {
      // Left this window in the meantime: it's in storage now (or another window's, which will pick it up from there)
      const l = loadLayout(id);
      if (!l) return;
      addTo(l, tab);
      if (draft !== undefined) l.drafts = { ...l.drafts, [tab.id]: draft };
      store.set("ws:" + id, JSON.stringify(l));
    }
  });
  // The sidebar's Fleet in another window jumps to one of our tabs.
  getCurrentWebviewWindow().listen<{ id: string; tab: string }>("show-tab", ({ payload }) => {
    if (!ws.open.includes(payload.id)) return;
    activate(payload.id);
    reveal(payload.tab);
  });
  const merge = store.get("merge:" + win.label);
  if (merge) mergeInto(merge);

  // Names, deletions and palette changes made in other windows.
  window.addEventListener("storage", (e) => {
    if (e.key !== "workspaces") return;
    ws.all = loadMeta();
    if (ws.active) applyWorkspacePalette(paletteOf(ws.active) ?? null);
  });

  // Closing a window gives its workspaces back, unsaved edits included; the last window's are kept to reopen on
  // launch (closing it quits, so it asks about working agents first).
  win.onCloseRequested(async (event) => {
    const last = (await getAllWebviewWindows()).length <= 1;
    if (last && !(await confirmQuit())) return event.preventDefault();
    parkAll();
    if (!last) store.remove("win:" + win.label);
  });
  // Reload (⌘R): the editors go, their unsaved text comes back from storage.
  window.addEventListener("pagehide", () => leaving || parkAll());
  getCurrentWebviewWindow().listen("quit-requested", () => quit());
  listen("park", () => (leaving || parkAll(), emit("parked")));

  // Reopen the other windows from last session (skipped when it's just the main window reloading).
  if (win.label === "main")
    for (let i = 0; i < localStorage.length; i++) {
      const k = localStorage.key(i)!;
      if (!k.startsWith("win:") || k === "win:main") continue;
      const label = k.slice(4);
      WebviewWindow.getByLabel(label).then((existing) => existing || newWindow(label, { width: 1400, height: 900 }, true, loadWin(k)?.active || undefined));
    }
}
