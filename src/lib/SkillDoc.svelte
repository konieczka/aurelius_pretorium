<script lang="ts">
  import { untrack } from "svelte";
  import { EditorView, basicSetup } from "codemirror";
  import { Compartment } from "@codemirror/state";
  import { marked } from "marked";
  import DOMPurify from "dompurify";
  import { SquareArrowOutUpRight } from "@lucide/svelte";
  import { aurelius, languageFor } from "./Editor.svelte";
  import { openFile } from "./state.svelte";
  import { body } from "./skills";
  import { loadDoc, saveSkill, skills } from "./skillsView.svelte";

  // The selected skill's SKILL.md inside the Skills view: rendered, or in an editor. Unsaved edits live in
  // skills.drafts, so they survive switching skills and closing the view.
  let { file, readOnly }: { file: string; readOnly: boolean } = $props();

  $effect(() => {
    void skills.scan; // every rescan (window focus, refresh) picks up edits made elsewhere
    loadDoc(file);
  });

  const text = $derived(skills.drafts[file] ?? skills.saved[file]);
  const dirty = $derived(skills.drafts[file] !== undefined);
  const editing = $derived(skills.mode === "edit" && !readOnly);
  const loaded = $derived(skills.saved[file] !== undefined);
  // Sanitizing is mandatory: the HTML runs in a window that can call the pty commands.
  const html = $derived(!editing && text !== undefined ? DOMPurify.sanitize(marked.parse(body(text), { async: false })) : "");

  let host = $state<HTMLDivElement>();
  let view: EditorView | undefined;

  // One editor per shown file, made once its text is in.
  $effect(() => {
    if (!editing || !loaded || !host) return;
    const f = file;
    const lang = new Compartment();
    const v = (view = new EditorView({
      doc: untrack(() => skills.drafts[f] ?? skills.saved[f]),
      parent: host,
      extensions: [
        basicSetup,
        aurelius,
        EditorView.lineWrapping,
        lang.of([]),
        EditorView.updateListener.of((u) => {
          if (!u.docChanged) return;
          const s = u.state.doc.toString();
          if (s === skills.saved[f]) delete skills.drafts[f];
          else skills.drafts[f] = s;
        }),
      ],
    }));
    languageFor("SKILL.md")?.then((support) => v.dispatch({ effects: lang.reconfigure(support) }));
    v.focus();
    return () => {
      v.destroy();
      if (view === v) view = undefined;
    };
  });

  // Discarded edits, or the file changed on disk with none: the editor follows.
  $effect(() => {
    const want = text;
    if (view && want !== undefined && view.state.doc.toString() !== want) view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: want } });
  });
</script>

<div class="doc">
  <div class="bar">
    <span class="file">SKILL.md{#if dirty}<span class="dot" title="Unsaved changes"></span>{/if}</span>
    {#if dirty}
      <button class="btn" onclick={() => delete skills.drafts[file]}>Discard</button>
      <button class="btn primary" onclick={saveSkill} title="Save (⌘S)">Save</button>
    {/if}
    <div class="seg" role="tablist" aria-label="SKILL.md view">
      <button role="tab" aria-selected={!editing} class:on={!editing} onclick={() => (skills.mode = "preview")} title="Preview (⌘E)">Preview</button>
      <button role="tab" aria-selected={editing} class:on={editing} disabled={readOnly} onclick={() => (skills.mode = "edit")} title={readOnly ? "Managed by its agent: read-only here" : "Edit (⌘E)"}>Edit</button>
    </div>
    <button class="icon-btn" onclick={() => openFile(file)} title="Open in a tab" aria-label="Open SKILL.md in a tab"><SquareArrowOutUpRight size={13} strokeWidth={1.5} /></button>
  </div>
  {#if editing}
    <div class="editor" bind:this={host}></div>
  {:else if text === undefined}
    <p class="muted">Loading…</p>
  {:else}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="md md-content selectable" onclick={(ev) => (ev.target as HTMLElement).closest("a") && ev.preventDefault()}>
      {#if html.trim()}{@html html}{:else}<p class="muted">Nothing below the frontmatter yet.</p>{/if}
    </div>
  {/if}
</div>

<style>
  .doc {
    margin-top: 22px;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    overflow: hidden;
    background: var(--surface);
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 38px;
    padding: 0 6px 0 12px;
    border-bottom: 1px solid var(--border);
    background: var(--card);
  }
  .file {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin-right: auto;
    font-family: var(--font-mono);
    font-size: 12px;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--gold);
  }
  .seg {
    display: inline-flex;
    padding: 2px;
    border-radius: var(--radius-md);
    background: var(--muted);
  }
  .seg button {
    height: 24px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--muted-foreground);
    font-size: 12px;
    cursor: pointer;
  }
  .seg button.on {
    background: var(--background);
    color: var(--foreground);
    box-shadow: var(--shadow-raised);
  }
  .seg button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .btn {
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--input);
    border-radius: var(--radius-sm);
    background: var(--background);
    color: var(--foreground);
    font-size: 12px;
    cursor: pointer;
  }
  .btn:hover {
    background: var(--accent);
  }
  .btn.primary {
    border-color: var(--primary);
    background: var(--primary);
    color: var(--primary-foreground);
  }
  .btn.primary:hover {
    opacity: 0.9;
  }
  /* Drag the bottom edge for more room */
  .editor {
    height: 460px;
    min-height: 160px;
    resize: vertical;
    overflow: hidden;
  }
  .md {
    padding: 16px 22px 6px;
    font-size: 13.5px;
  }
  .muted {
    margin: 0;
    padding: 16px 22px;
    color: var(--muted-foreground);
    font-size: 13px;
  }
  .md .muted {
    padding: 0 0 10px;
  }
</style>
