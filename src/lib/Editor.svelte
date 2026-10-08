<script module lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { EditorView, basicSetup } from "codemirror";
  import { Compartment } from "@codemirror/state";
  import { HighlightStyle, LanguageDescription, syntaxHighlighting } from "@codemirror/language";
  import { languages } from "@codemirror/language-data";
  import { tags as t } from "@lezer/highlight";
  import { marked } from "marked";
  import DOMPurify from "dompurify";
  import { app } from "./state.svelte";

  // Editors outlive their component so unsaved edits survive tab moves.
  type Entry = { view?: EditorView; host: HTMLDivElement; path: string; error?: string };
  const entries = new Map<string, Entry>();

  export function disposeEditor(tabId: string) {
    entries.get(tabId)?.view?.destroy();
    entries.get(tabId)?.host.remove();
    entries.delete(tabId);
  }

  export async function saveEditor(tabId: string) {
    const e = entries.get(tabId);
    if (!e?.view) return;
    await invoke("write_file", { path: e.path, content: e.view.state.doc.toString() });
    app.dirty[tabId] = false;
  }

  export const focusEditor = (tabId: string) => entries.get(tabId)?.view?.focus();
  const text = (tabId: string) => entries.get(tabId)?.view?.state.doc.toString() ?? "";

  // Aurelius theme in CSS variables only, so light/dark switching restyles editors live.
  const aurelius = [
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

  function create(tabId: string, path: string): Entry {
    const e: Entry = { host: document.createElement("div"), path };
    e.host.className = "editor-host";
    entries.set(tabId, e);
    invoke<string>("read_file", { path }).then(
      (doc) => {
        const lang = new Compartment();
        e.view = new EditorView({
          doc,
          parent: e.host,
          extensions: [
            basicSetup,
            aurelius,
            lang.of([]),
            EditorView.updateListener.of((u) => u.docChanged && (app.dirty[tabId] = true)),
          ],
        });
        LanguageDescription.matchFilename(languages, path)
          ?.load()
          .then((support) => e.view?.dispatch({ effects: lang.reconfigure(support) }));
        app.dirty[tabId] = false; // nudge reactivity so the preview renders once loaded
      },
      (err) => {
        e.error = String(err);
        e.host.textContent = e.error;
        app.dirty[tabId] = false;
      },
    );
    return e;
  }
</script>

<script lang="ts">
  let { tabId, path, preview }: { tabId: string; path: string; preview: boolean } = $props();
  let el: HTMLDivElement;

  $effect(() => {
    const e = entries.get(tabId) ?? create(tabId, path);
    el.appendChild(e.host);
    if (!preview) e.view?.focus();
  });

  // Sanitizing is mandatory: preview HTML runs in a window that can call the pty commands.
  const html = $derived(preview && app.dirty[tabId] !== undefined ? DOMPurify.sanitize(marked.parse(text(tabId), { async: false })) : "");
</script>

<div class="editor" bind:this={el} class:hidden={preview}></div>
{#if preview}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="md" onclick={(ev) => (ev.target as HTMLElement).closest("a") && ev.preventDefault()}>
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
  .hidden {
    display: none;
  }
  .editor :global(.editor-host) {
    height: 100%;
  }
  /* Markdown preview: Aurelius MarkdownContent rules */
  .md {
    padding: 28px 44px;
    color: var(--foreground);
    font-size: 14px;
    line-height: 1.625;
  }
  .md :global(> *) {
    max-width: 780px;
  }
  .md :global(:is(h1, h2, h3, h4, h5, h6)) {
    font-weight: 600;
    line-height: 1.3;
  }
  .md :global(h1) { font-size: 1.25rem; margin: 1.25rem 0 0.75rem; }
  .md :global(h2) { font-size: 1.125rem; margin: 1.25rem 0 0.5rem; }
  .md :global(h3) { font-size: 1rem; margin: 1rem 0 0.5rem; }
  .md :global(:is(h4, h5)) { font-size: 0.875rem; }
  .md :global(h6) { font-size: 0.875rem; font-weight: 500; color: var(--muted-foreground); }
  .md :global(p) { margin: 0 0 0.75rem; }
  .md :global(strong) { font-weight: 800; }
  .md :global(a) {
    color: var(--foreground);
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .md :global(a:hover) { opacity: 0.8; }
  .md :global(:is(ul, ol)) { margin: 0 0 0.75rem; padding-left: 1.25rem; }
  .md :global(li + li) { margin-top: 0.25rem; }
  .md :global(code) {
    font-family: var(--font-mono);
    font-size: 0.85em;
    background: var(--muted);
    padding: 0.125rem 0.25rem;
    border-radius: 4px;
  }
  .md :global(pre) {
    background: var(--muted);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    padding: 12px;
    font-size: 12px;
    overflow-x: auto;
  }
  .md :global(pre code) {
    padding: 0;
    background: none;
    font-size: inherit;
  }
  .md :global(blockquote) {
    margin: 0 0 0.75rem;
    padding-left: 0.75rem;
    border-left: 2px solid var(--border);
    font-style: italic;
    color: var(--muted-foreground);
  }
  .md :global(table) {
    border-collapse: collapse;
    font-size: 12px;
    margin-bottom: 0.75rem;
  }
  .md :global(thead) { background: var(--muted); }
  .md :global(:is(th, td)) {
    border: 1px solid var(--border);
    padding: 6px 8px;
  }
  .md :global(th) { font-weight: 600; text-align: left; }
  .md :global(hr) { border: 0; border-top: 1px solid var(--border); margin: 1rem 0; }
  .md :global(img) { max-width: 100%; border-radius: var(--radius-md); }
</style>
