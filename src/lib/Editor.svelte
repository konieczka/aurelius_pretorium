<script module lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { save } from "@tauri-apps/plugin-dialog";
  import { EditorView, basicSetup } from "codemirror";
  import { Compartment, Transaction } from "@codemirror/state";
  import { keymap } from "@codemirror/view";
  import { conflictMarkers, conflictsIn, goToConflict } from "./conflictView";
  import { HighlightStyle, LanguageDescription, syntaxHighlighting } from "@codemirror/language";
  import { languages } from "@codemirror/language-data";
  import { tags as t } from "@lezer/highlight";
  import { marked } from "marked";
  import DOMPurify from "dompurify";
  import { app } from "./state.svelte";
  import { findTab } from "./layout";

  // Editors outlive their component so unsaved edits survive tab moves.
  type Entry = { view?: EditorView; host: HTMLDivElement; error?: string; lang?: Compartment; wrap?: Compartment };
  const entries = new Map<string, Entry>();

  export function disposeEditor(tabId: string) {
    entries.get(tabId)?.view?.destroy();
    entries.get(tabId)?.host.remove();
    entries.delete(tabId);
  }

  export async function saveEditor(tabId: string) {
    const e = entries.get(tabId);
    if (!e?.view) return;
    // The tab's path, not e.path: the file may have been moved since it was opened.
    const f = findTab(app.root, tabId);
    const tab = f?.pane.tabs[f.index];
    if (tab?.kind !== "file") return;
    const untitled = !tab.path;
    if (untitled) {
      const p = await save({ defaultPath: `${app.workspace}/Untitled.md` });
      if (!p) return;
      tab.path = p;
    }
    await invoke("write_file", { path: tab.path, content: e.view.state.doc.toString() });
    app.dirty[tabId] = false;
    app.gitVersion++;
    if (untitled) {
      app.treeVersion++;
      languageFor(tab.path)?.then((support) => e.lang && e.view?.dispatch({ effects: e.lang.reconfigure(support) }));
    }
  }

  /** Pick up external changes (agents, git pull/checkout) when the tab has no unsaved edits. */
  export async function reloadEditor(tabId: string, path: string) {
    const e = entries.get(tabId);
    if (!e?.view || app.dirty[tabId]) return;
    const disk = await invoke<string>("read_file", { path }).catch(() => null);
    const cur = e.view?.state.doc.toString();
    if (disk === null || cur === undefined || disk === cur || app.dirty[tabId]) return;
    e.view!.dispatch({ changes: { from: 0, to: cur.length, insert: disk }, annotations: [Transaction.remote.of(true), Transaction.addToHistory.of(false)] });
  }

  /** Conflict navigation for the toolbar; returns the 1-based index. */
  export const jumpConflict = (tabId: string, dir: 1 | -1) => {
    const v = entries.get(tabId)?.view;
    return v ? (v.focus(), goToConflict(v, dir)) : 0;
  };

  export const focusEditor = (tabId: string) => entries.get(tabId)?.view?.focus();
  const text = (tabId: string) => entries.get(tabId)?.view?.state.doc.toString() ?? "";

  // Unsaved text carried over from another window (its workspace moved here): opens in place of the file, still unsaved.
  const drafts = new Map<string, string>();
  export function queueDraft(tabId: string, text: string) {
    drafts.set(tabId, text);
    app.dirty[tabId] = true; // the tab shows as unsaved before its editor opens
  }
  /** A tab's unsaved text, including a carried-over draft whose editor hasn't opened yet. */
  export const draftOf = (tabId: string) => (entries.get(tabId)?.view ? (app.dirty[tabId] ? text(tabId) : undefined) : drafts.get(tabId));

  // Aurelius theme in CSS variables only, so light/dark switching restyles editors live.
  export const aurelius = [
    EditorView.theme({
      "&": { height: "100%", color: "var(--foreground)", backgroundColor: "var(--surface)" },
      ".cm-scroller": { fontFamily: "var(--font-mono)", fontSize: "13px", lineHeight: "1.55" },
      ".cm-content": { caretColor: "var(--foreground)" },
      ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--foreground)" },
      "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, ::selection": { backgroundColor: "var(--gold-soft) !important" },
      ".cm-gutters": { backgroundColor: "var(--surface)", color: "var(--muted-foreground)", border: "none" },
      ".cm-activeLine": { backgroundColor: "var(--accent)" },
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
    const e: Entry = { host: document.createElement("div") };
    e.host.className = "editor-host selectable"; // code, or the read error
    entries.set(tabId, e);
    const draft = drafts.get(tabId);
    drafts.delete(tabId);
    (draft !== undefined ? Promise.resolve(draft) : !path ? Promise.resolve("") : invoke<string>("read_file", { path })).then(
      (doc) => {
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
  import { isSvg } from "./media";
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

  // Conflict toolbar: shown while the file has markers, or is still unmerged in git.
  const conflicts = $derived(app.conflicts[tabId] ?? 0);
  const unmerged = $derived(!!git.status?.files.some((f) => f.kind === "conflict" && `${git.root}/${f.path}` === path));
  let at = $state(0);
  async function resolved() {
    if (app.dirty[tabId]) await saveEditor(tabId);
    markResolved(path);
  }

  // Sanitizing is mandatory: preview HTML runs in a window that can call the pty commands.
  const svg = $derived(isSvg(path));
  const html = $derived(preview && !svg && app.dirty[tabId] !== undefined ? DOMPurify.sanitize(marked.parse(text(tabId), { async: false })) : "");
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
{#if preview && svg}
  {#if svgSrc}{#key svgSrc}<Media {path} src={svgSrc} />{/key}{/if}
{:else if preview}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="md md-content selectable" onclick={(ev) => (ev.target as HTMLElement).closest("a") && ev.preventDefault()}>
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
