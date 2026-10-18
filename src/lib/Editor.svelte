<script module lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { ask, message, save } from "@tauri-apps/plugin-dialog";
  import { EditorView, basicSetup } from "codemirror";
  import { Compartment, Transaction } from "@codemirror/state";
  import { keymap } from "@codemirror/view";
  import { conflictMarkers, conflictsIn, goToConflict } from "./conflictView";
  import { HighlightStyle, LanguageDescription, syntaxHighlighting } from "@codemirror/language";
  import { languages } from "@codemirror/language-data";
  import { tags as t } from "@lezer/highlight";
  import { blockLinks, renderMarkdown } from "./markdown";
  import { app, openBrowserBeside } from "./state.svelte";
  import { findTab } from "./layout";

  // Editors outlive their component so unsaved edits survive tab moves.
  // stamp: the file's version on disk the text was read or saved at (see open_file); eol: its line breaks, kept on save.
  type Entry = { view?: EditorView; host: HTMLDivElement; error?: string; lang?: Compartment; wrap?: Compartment; stamp?: string | null; eol: string };
  type Opened = { text: string; stamp: string | null };
  const entries = new Map<string, Entry>();
  // Drawings (.excalidraw) edit the text too: their latest edits, not yet written to it, and remounts on reload.
  const flushes = new Map<string, () => void>();
  const reloads = $state<Record<string, number>>({});

  export function disposeEditor(tabId: string) {
    entries.get(tabId)?.view?.destroy();
    entries.get(tabId)?.host.remove();
    entries.delete(tabId);
    lines.delete(tabId);
  }

  /** The editor's text as it goes to disk: with the file's own line breaks (CodeMirror keeps lines, not breaks). */
  const textOf = (e: Entry) => e.view!.state.doc.sliceString(0, undefined, e.eol);
  const eolOf = (text: string) => (text.includes("\r\n") ? "\r\n" : "\n");
  const CHANGED_ON_DISK = "changed on disk"; // write_file's refusal to overwrite a newer file

  /** Save the tab's text. Returns whether it was written. */
  export async function saveEditor(tabId: string): Promise<boolean> {
    const e = entries.get(tabId);
    if (!e?.view) return false;
    flushes.get(tabId)?.();
    // The tab's path, not e.path: the file may have been moved since it was opened.
    const f = findTab(app.root, tabId);
    const tab = f?.pane.tabs[f.index];
    if (tab?.kind !== "file") return false;
    const untitled = !tab.path;
    const path = tab.path || (await save({ defaultPath: `${app.workspace}/Untitled.md` }));
    if (!path) return false;
    const content = textOf(e);
    const write = (expect: string | null | undefined) => invoke<string | null>("write_file", { path, content, expect: expect ?? null });
    try {
      // An agent (or anything else) may have changed the file since it was read: don't silently overwrite that.
      e.stamp = await write(untitled ? null : e.stamp).catch(async (err) => {
        if (String(err) !== CHANGED_ON_DISK) throw err;
        const name = path.slice(path.lastIndexOf("/") + 1);
        if (!(await ask(`“${name}” was changed on disk since you opened it. Overwrite it with your version?`, { title: "File changed on disk", kind: "warning", okLabel: "Overwrite", cancelLabel: "Cancel" }))) throw null;
        return write(null);
      });
    } catch (err) {
      if (err !== null) message(`Couldn't save ${path}:\n${err}`, { kind: "error" });
      return false;
    }
    if (untitled) tab.path = path; // only once it's really there
    app.dirty[tabId] = false;
    app.gitVersion++;
    if (untitled) {
      app.treeVersion++;
      languageFor(path)?.then((support) => e.lang && e.view?.dispatch({ effects: e.lang.reconfigure(support) }));
    }
    return true;
  }

  /** Pick up external changes (agents, git pull/checkout) when the tab has no unsaved edits. `stamp`: the version
   * on disk if the caller already knows it (unchanged = nothing to do). */
  export async function reloadEditor(tabId: string, path: string, stamp?: string | null) {
    const e = entries.get(tabId);
    if (!e?.view || app.dirty[tabId] || !path) return;
    if (stamp !== undefined && stamp === e.stamp) return;
    const disk = await invoke<Opened>("open_file", { path }).catch(() => null);
    if (!disk || !e.view || app.dirty[tabId]) return;
    e.stamp = disk.stamp;
    e.eol = eolOf(disk.text);
    const cur = textOf(e);
    if (disk.text === cur) return;
    e.view.dispatch({ changes: { from: 0, to: e.view.state.doc.length, insert: disk.text }, annotations: [Transaction.remote.of(true), Transaction.addToHistory.of(false)] });
    reloads[tabId] = (reloads[tabId] ?? 0) + 1;
  }

  /** Has the file changed on disk since the editor read it? Checked every few seconds for the tab on screen: git
   * status doesn't change when an already modified file is edited again, or for ignored files. */
  async function checkDisk(tabId: string, path: string) {
    const e = entries.get(tabId);
    if (!e?.view || !path || e.stamp === undefined) return;
    const stamp = await invoke<string | null>("file_stamp", { path }).catch(() => undefined);
    if (stamp) reloadEditor(tabId, path, stamp);
  }

  /** Conflict navigation for the toolbar; returns the 1-based index. */
  export const jumpConflict = (tabId: string, dir: 1 | -1) => {
    const v = entries.get(tabId)?.view;
    return v ? (v.focus(), goToConflict(v, dir)) : 0;
  };

  export const focusEditor = (tabId: string) => entries.get(tabId)?.view?.focus();

  // Where to put the cursor once the tab's text has loaded (a terminal's link to "file:line").
  const lines = new Map<string, { line: number; col: number }>();
  /** Put the cursor on `line` (1-based), at column `col`, and scroll it into the middle of the editor. */
  export function revealLine(tabId: string, line: number, col = 1) {
    const v = entries.get(tabId)?.view;
    if (!v) return void lines.set(tabId, { line, col });
    const l = v.state.doc.line(Math.min(Math.max(line, 1), v.state.doc.lines));
    const anchor = l.from + Math.min(Math.max(col - 1, 0), l.length);
    v.dispatch({ selection: { anchor }, effects: EditorView.scrollIntoView(anchor, { y: "center" }) });
    v.focus();
  }
  const text = (tabId: string) => entries.get(tabId)?.view?.state.doc.toString() ?? "";

  // Unsaved text carried over from another window (its workspace moved here): opens in place of the file, still unsaved.
  const drafts = new Map<string, string>();
  export function queueDraft(tabId: string, text: string) {
    drafts.set(tabId, text);
    app.dirty[tabId] = true; // the tab shows as unsaved before its editor opens
  }
  /** A tab's unsaved text, including a carried-over draft whose editor hasn't opened yet. */
  export const draftOf = (tabId: string) => (flushes.get(tabId)?.(), entries.get(tabId)?.view ? (app.dirty[tabId] ? text(tabId) : undefined) : drafts.get(tabId));

  /** A drawing's edit: the whole text, replaced (marks the tab unsaved). Not undoable here: the drawing has its undo. */
  function setText(tabId: string, text: string) {
    const v = entries.get(tabId)?.view;
    v?.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: text }, annotations: Transaction.addToHistory.of(false) });
  }

  // Aurelius theme in CSS variables only, so light/dark switching restyles editors live.
  export const aurelius = [
    EditorView.contentAttributes.compute(["selection"], (st) => ({ class: st.selection.ranges.some((r) => !r.empty) ? "cm-hasSelection" : "" })),
    EditorView.theme({
      "&": { height: "100%", color: "var(--foreground)", backgroundColor: "var(--surface)" },
      ".cm-scroller": { fontFamily: "var(--font-mono)", fontSize: "13px", lineHeight: "1.55" },
      ".cm-content": { caretColor: "var(--foreground)" },
      ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--foreground)" },
      "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, ::selection": { backgroundColor: "color-mix(in oklch, var(--gold) 28%, var(--surface)) !important" },
      ".cm-gutters": { backgroundColor: "var(--surface)", color: "var(--muted-foreground)", border: "none" },
      ".cm-activeLine": { backgroundColor: "var(--accent)" },
      // The selection is solid and sits under the text, so the active line (painted on the text lines) must not cover it.
      ".cm-hasSelection .cm-activeLine": { backgroundColor: "transparent" },
      ".cm-activeLineGutter": { backgroundColor: "transparent", color: "var(--foreground)" },
      ".cm-selectionMatch": { backgroundColor: "var(--gold-soft)" },
      "&.cm-focused .cm-matchingBracket": { backgroundColor: "var(--gold-soft)", outline: "1px solid var(--gold-ring)" },
      ".cm-searchMatch": { backgroundColor: "var(--gold-soft)", outline: "1px solid var(--gold-ring)" },
      ".cm-foldPlaceholder": { backgroundColor: "var(--muted)", color: "var(--muted-foreground)", border: "none" },
      ".cm-panels": { backgroundColor: "var(--card)", color: "var(--foreground)" },
      ".cm-panels.cm-panels-top": { borderBottom: "1px solid var(--border)" },
      ".cm-tooltip": { backgroundColor: "var(--popover)", border: "1px solid var(--border-strong)", borderRadius: "8px" },
      ".cm-tooltip-autocomplete > ul > li[aria-selected]": { backgroundColor: "var(--accent)", color: "var(--foreground)" },
    }),
    syntaxHighlighting(
      HighlightStyle.define([
        { tag: [t.comment, t.quote], color: "var(--syn-comment)", fontStyle: "italic" },
        { tag: [t.keyword, t.literal, t.bool, t.null, t.modifier, t.operatorKeyword], color: "var(--syn-keyword)" },
        { tag: [t.string, t.regexp, t.inserted, t.special(t.string)], color: "var(--syn-string)" },
        { tag: [t.number, t.atom, t.meta, t.escape], color: "var(--syn-number)" },
        { tag: [t.function(t.variableName), t.function(t.propertyName), t.definition(t.function(t.variableName)), t.heading, t.className], color: "var(--syn-title)" },
        { tag: [t.attributeName, t.typeName, t.standard(t.variableName), t.propertyName, t.tagName, t.labelName], color: "var(--syn-attr)" },
        { tag: [t.deleted, t.invalid], color: "var(--syn-deletion)" },
        { tag: t.heading, fontWeight: "600" },
        { tag: t.emphasis, fontStyle: "italic" },
        { tag: t.strong, fontWeight: "600" },
        { tag: t.link, color: "var(--syn-title)", textDecoration: "underline" },
      ]),
    ),
  ];

  export const languageFor = (path: string) => LanguageDescription.matchFilename(languages, path)?.load();

  function create(tabId: string, path: string, wrap: boolean): Entry {
    const e: Entry = { host: document.createElement("div"), eol: "\n" };
    e.host.className = "editor-host selectable"; // code, or the read error
    entries.set(tabId, e);
    const draft = drafts.get(tabId);
    drafts.delete(tabId);
    const blank: Opened = { text: draft ?? "", stamp: null };
    (draft !== undefined || !path ? Promise.resolve(blank) : invoke<Opened>("open_file", { path })).then(
      ({ text: doc, stamp }) => {
        // A carried-over draft was read at some unknown version: saving it checks nothing (as before drafts had stamps).
        e.stamp = draft !== undefined ? null : stamp;
        e.eol = eolOf(doc);
        const lang = (e.lang = new Compartment());
        const wrapping = (e.wrap = new Compartment());
        e.view = new EditorView({
          doc,
          parent: e.host,
          extensions: [
            basicSetup,
            aurelius,
            lang.of([]),
            wrapping.of(wrap ? EditorView.lineWrapping : []),
            conflictMarkers,
            keymap.of([
              { key: "F7", run: (v) => goToConflict(v, 1) > 0 },
              { key: "Shift-F7", run: (v) => goToConflict(v, -1) > 0 },
            ]),
            EditorView.updateListener.of((u) => {
              if (!u.docChanged) return;
              if (!u.transactions.some((t) => t.annotation(Transaction.remote))) app.dirty[tabId] = true;
              app.conflicts[tabId] = conflictsIn(u.state).length;
            }),
          ],
        });
        languageFor(path)?.then((support) => e.view?.dispatch({ effects: lang.reconfigure(support) }));
        app.conflicts[tabId] = conflictsIn(e.view.state).length;
        app.dirty[tabId] = draft !== undefined; // also nudges reactivity so the preview renders once loaded
        const at = lines.get(tabId);
        if (at) lines.delete(tabId), revealLine(tabId, at.line, at.col);
      },
      (err) => {
        // read_file reads text: anything else (fonts, archives, binaries) fails to decode.
        e.error = /valid UTF-8/.test(String(err)) ? "This is a binary file, so it can't be shown as text." : String(err);
        e.host.classList.add("unreadable");
        e.host.textContent = e.error;
        app.dirty[tabId] = false;
      },
    );
    return e;
  }
</script>

<script lang="ts">
  import { untrack } from "svelte";
  import { Check, ChevronDown, ChevronUp } from "@lucide/svelte";
  import { git, markResolved } from "./GitPanel.svelte";
  import Media from "./Media.svelte";
  import Excalidraw from "./Excalidraw.svelte";
  import { isDrawing, isSvg } from "./media";
  let { tabId, path, preview, wrap }: { tabId: string; path: string; preview: boolean; wrap: boolean } = $props();
  let el: HTMLDivElement;

  $effect(() => {
    // untrack: a moved file changes `path`, which must not re-run this (and steal focus).
    const e = entries.get(tabId) ?? create(tabId, untrack(() => path), untrack(() => wrap));
    el.appendChild(e.host);
    if (!preview) e.view?.focus();
  });

  $effect(() => {
    const on = wrap;
    const e = entries.get(tabId);
    e?.view?.dispatch({ effects: e.wrap!.reconfigure(on ? EditorView.lineWrapping : []) });
  });

  $effect(() => {
    void app.gitVersion; // bumped when git status changes (pull, checkout, agents editing files)
    reloadEditor(tabId, untrack(() => path));
  });

  $effect(() => {
    const t = setInterval(() => document.visibilityState === "visible" && checkDisk(tabId, path), 2000);
    return () => clearInterval(t);
  });

  // Conflict toolbar: shown while the file has markers, or is still unmerged in git.
  const conflicts = $derived(app.conflicts[tabId] ?? 0);
  const unmerged = $derived(!!git.status?.files.some((f) => f.kind === "conflict" && `${git.root}/${f.path}` === path));
  let at = $state(0);
  async function resolved() {
    if (app.dirty[tabId] && !(await saveEditor(tabId))) return;
    markResolved(path);
  }

  // Sanitizing is mandatory: preview HTML runs in a window that can call the pty commands.
  const svg = $derived(isSvg(path));
  const drawing = $derived(isDrawing(path));
  const html = $derived(preview && !svg && !drawing && app.dirty[tabId] !== undefined ? renderMarkdown(text(tabId)) : "");
  // SVG preview draws the editor's text (unsaved edits too) as an <img>: scripts inside never run.
  const svgSrc = $derived(preview && svg && app.dirty[tabId] !== undefined ? `data:image/svg+xml;charset=utf-8,${encodeURIComponent(text(tabId))}` : "");
</script>

<div class="editor" bind:this={el} class:hidden={preview}></div>
{#if !preview && (conflicts || unmerged)}
  <div class="conflictbar">
    {#if conflicts}
      <span class="count">{at && at <= conflicts ? `Conflict ${at} of ${conflicts}` : `${conflicts} ${conflicts === 1 ? "conflict" : "conflicts"}`}</span>
      <button class="icon-btn" onclick={() => (at = jumpConflict(tabId, -1))} title="Previous conflict (⇧F7)"><ChevronUp size={14} strokeWidth={1.5} /></button>
      <button class="icon-btn" onclick={() => (at = jumpConflict(tabId, 1))} title="Next conflict (F7)"><ChevronDown size={14} strokeWidth={1.5} /></button>
    {:else}
      <span class="count">No conflicts left</span>
      <button class="resolve" onclick={resolved} title="Save and stage (git add)"><Check size={13} strokeWidth={1.75} />Mark as Resolved</button>
    {/if}
  </div>
{/if}
{#if preview && drawing}
  {#if app.dirty[tabId] !== undefined && !entries.get(tabId)?.error}
    {#key reloads[tabId]}
      <Excalidraw
        source={text(tabId)}
        onedit={() => (app.dirty[tabId] = true)}
        oncommit={(json) => setText(tabId, json)}
        onlink={(url) => openBrowserBeside(url, tabId)}
        register={(flush) => (flush ? flushes.set(tabId, flush) : flushes.delete(tabId))}
      />
    {/key}
  {/if}
{:else if preview && svg}
  {#if svgSrc}{#key svgSrc}<Media {path} src={svgSrc} />{/key}{/if}
{:else if preview}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="md md-content selectable" onclick={blockLinks}>
    {@html html}
  </div>
{/if}

<style>
  .editor,
  .md {
    position: absolute;
    inset: 0;
    overflow: auto;
    background: var(--surface);
  }
  .conflictbar {
    position: absolute;
    top: 8px;
    right: 18px;
    z-index: 5;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 2px 2px 2px 10px;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background: var(--popover);
    box-shadow: var(--shadow-raised);
  }
  .count {
    margin-right: 4px;
    color: var(--muted-foreground);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
  .resolve {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 24px;
    padding: 0 8px;
    border: 0;
    border-radius: var(--radius-sm);
    background: var(--primary);
    color: var(--primary-foreground);
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
  }
  .hidden {
    display: none;
  }
  .editor :global(.editor-host) {
    height: 100%;
  }
  .editor :global(.editor-host.unreadable) {
    display: grid;
    place-items: center;
    color: var(--muted-foreground);
    font-size: 13px;
  }
  .md {
    padding: 28px 44px;
  }
  .md > :global(*) {
    max-width: 780px;
  }
</style>
