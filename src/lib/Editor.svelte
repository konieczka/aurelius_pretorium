<script module lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { EditorView, basicSetup } from "codemirror";
  import { Compartment } from "@codemirror/state";
  import { LanguageDescription } from "@codemirror/language";
  import { languages } from "@codemirror/language-data";
  import { oneDark } from "@codemirror/theme-one-dark";
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
            oneDark,
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
    background: #282c34;
  }
  .hidden {
    display: none;
  }
  .editor :global(.editor-host),
  .editor :global(.cm-editor) {
    height: 100%;
  }
  .editor :global(.cm-scroller) {
    font-family: Menlo, Monaco, monospace;
    font-size: 13px;
  }
  .md {
    padding: 24px 40px;
    color: #d7dae0;
    font-size: 15px;
    line-height: 1.6;
  }
  .md :global(> *) {
    max-width: 820px;
  }
  .md :global(h1),
  .md :global(h2) {
    border-bottom: 1px solid #3b4048;
    padding-bottom: 0.3em;
  }
  .md :global(a) {
    color: #61afef;
  }
  .md :global(code) {
    font-family: Menlo, Monaco, monospace;
    font-size: 0.88em;
    background: #21252b;
    padding: 0.15em 0.35em;
    border-radius: 4px;
  }
  .md :global(pre) {
    background: #21252b;
    padding: 12px 16px;
    border-radius: 6px;
    overflow: auto;
  }
  .md :global(pre code) {
    padding: 0;
    background: none;
  }
  .md :global(blockquote) {
    margin: 0;
    padding-left: 1em;
    border-left: 3px solid #4b5263;
    color: #9da5b4;
  }
  .md :global(table) {
    border-collapse: collapse;
  }
  .md :global(th),
  .md :global(td) {
    border: 1px solid #3b4048;
    padding: 6px 12px;
  }
  .md :global(img) {
    max-width: 100%;
  }
</style>
