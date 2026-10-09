<script module lang="ts">
  import { RangeSetBuilder } from "@codemirror/state";
  import { Decoration, ViewPlugin, type DecorationSet, type EditorView as View, type ViewUpdate } from "@codemirror/view";
  import { secretRanges } from "./agentConfig";

  // Credentials (API keys, tokens, auth headers) stay blurred while you edit around them; hover one to read it.
  const secret = Decoration.mark({ class: "cm-secret" });
  function secrets(view: View) {
    const b = new RangeSetBuilder<Decoration>();
    for (const { from, to } of view.visibleRanges) {
      for (let pos = from; pos <= to; ) {
        const line = view.state.doc.lineAt(pos);
        for (const [a, z] of secretRanges(line.text)) b.add(line.from + a, line.from + z, secret);
        pos = line.to + 1;
      }
    }
    return b.finish();
  }
  const blurSecrets = ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      constructor(view: View) {
        this.decorations = secrets(view);
      }
      update(u: ViewUpdate) {
        if (u.docChanged || u.viewportChanged) this.decorations = secrets(u.view);
      }
    },
    { decorations: (v) => v.decorations },
  );
</script>

<script lang="ts">
  import { untrack } from "svelte";
  import { EditorView, basicSetup } from "codemirror";
  import { Compartment, EditorState } from "@codemirror/state";
  import { SquareArrowOutUpRight } from "@lucide/svelte";
  import { aurelius, languageFor } from "./Editor.svelte";
  import { openFile } from "./state.svelte";
  import { blank, type ConfigFile } from "./agentConfig";
  import { agents, saveAgentFile } from "./agentsView.svelte";

  // One agent config file in an editor. Unsaved edits live in agents.drafts, so they survive switching files and
  // closing the view; with none, the editor follows the file on disk (rescans on focus and after saves).
  let { file }: { file: ConfigFile } = $props();

  const path = $derived(file.path);
  const disk = $derived(file.exists ? file.text : blank(file));
  const text = $derived(agents.drafts[path] ?? disk);
  const dirty = $derived(agents.drafts[path] !== undefined);
  const name = $derived(path.slice(path.lastIndexOf("/") + 1));

  let host = $state<HTMLDivElement>();
  let view: EditorView | undefined;

  $effect(() => {
    if (!host) return;
    const f = path;
    const ro = file.readOnly;
    const lang = new Compartment();
    const v = (view = new EditorView({
      doc: untrack(() => text),
      parent: host,
      extensions: [
        basicSetup,
        aurelius,
        lang.of([]),
        blurSecrets,
        EditorState.readOnly.of(ro),
        EditorView.updateListener.of((u) => {
          if (!u.docChanged || ro) return;
          const s = u.state.doc.toString();
          if (s === disk) delete agents.drafts[f];
          else agents.drafts[f] = s;
        }),
      ],
    }));
    languageFor(f.endsWith(".jsonc") ? "x.json" : f)?.then((support) => v.dispatch({ effects: lang.reconfigure(support) }));
    return () => {
      v.destroy();
      if (view === v) view = undefined;
    };
  });

  // Discarded edits, or the file changed on disk with none: the editor follows.
  $effect(() => {
    const want = text;
    if (view && view.state.doc.toString() !== want) view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: want } });
  });
</script>

<div class="doc">
  <div class="bar">
    <span class="file" title={path}>{name}{#if dirty}<span class="dot" title="Unsaved changes"></span>{/if}</span>
    {#if file.readOnly}<span class="note">Read-only</span>{:else if !file.exists && !dirty}<span class="note">Not created yet; saving creates it</span>{/if}
    {#if dirty}
      <button class="btn" onclick={() => delete agents.drafts[path]}>Discard</button>
      <button class="btn primary" onclick={saveAgentFile} disabled={agents.saving === path} title="Save (⌘S)">Save</button>
    {/if}
    {#if file.exists}
      <button class="icon-btn" onclick={() => openFile(path)} title="Open in a tab" aria-label="Open {name} in a tab"><SquareArrowOutUpRight size={13} strokeWidth={1.5} /></button>
    {/if}
  </div>
  <div class="editor" bind:this={host}></div>
</div>

<style>
  .doc {
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
  .note {
    color: var(--muted-foreground);
    font-size: 12px;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--gold);
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
    height: 520px;
    min-height: 160px;
    resize: vertical;
    overflow: hidden;
  }
  .editor :global(.cm-secret) {
    filter: blur(4px);
    transition: filter var(--dur-quick) var(--ease-standard);
  }
  .editor :global(.cm-secret:hover) {
    filter: none;
  }
</style>
