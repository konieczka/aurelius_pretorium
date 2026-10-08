<script module lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { EditorView, drawSelection, highlightSpecialChars, keymap, lineNumbers } from "@codemirror/view";
  import { EditorState } from "@codemirror/state";
  import { getChunks, unifiedMergeView } from "@codemirror/merge";
  import { aurelius, languageFor } from "./Editor.svelte";
  import { app } from "./state.svelte";

  // Overrides @codemirror/merge's base colors with the Aurelius diff tokens.
  const diffTheme = EditorView.theme({
    "&.cm-merge-b .cm-changedLine": { backgroundColor: "var(--diff-add-bg)" },
    "&.cm-merge-b .cm-changedText": { background: "var(--diff-add-strong)" },
    ".cm-deletedChunk": { backgroundColor: "var(--diff-del-bg)", paddingLeft: "6px" },
    ".cm-deletedChunk .cm-deletedText": { background: "var(--diff-del-strong)" },
    ".cm-changedLineGutter": { background: "var(--diff-add-gutter)" },
    ".cm-deletedLineGutter": { background: "var(--diff-del-gutter)" },
  });

  /** Move to the next (dir 1) / previous (-1) change, wrapping around; centers it. Returns its 1-based index. */
  function go(view: EditorView, dir: 1 | -1, from = view.state.selection.main.head) {
    const chunks = getChunks(view.state)?.chunks ?? [];
    if (!chunks.length) return 0;
    let i = dir > 0 ? chunks.findIndex((c) => c.fromB > from) : chunks.findLastIndex((c) => c.fromB < from);
    if (i < 0) i = dir > 0 ? 0 : chunks.length - 1;
    const pos = chunks[i].fromB;
    view.dispatch({ selection: { anchor: pos }, effects: EditorView.scrollIntoView(pos, { y: "center" }) });
    return i + 1;
  }

  /** `git show <spec>`, or "" when it doesn't exist there (new / untracked / deleted file). */
  const show = (cwd: string, spec: string) => invoke<string>("git", { cwd, args: ["show", spec] }).catch(() => "");
</script>

<script lang="ts">
  import { ChevronDown, ChevronUp } from "@lucide/svelte";
  let { path, staged, orig, ref }: { path: string; staged: boolean; orig?: string; ref?: string } = $props();
  let el: HTMLDivElement;
  let note = $state("");
  let scroll = -1; // kept across refetches; -1 = first open (jump to the first change)
  let view: EditorView | undefined;
  let at = $state(0); // current change (1-based), 0 = none selected yet
  let total = $state(0);
  const nav = (dir: 1 | -1) => view && ((at = go(view, dir)), view.focus());

  $effect(() => {
    void app.gitVersion; // refetch after saves and git operations
    const dir = path.slice(0, path.lastIndexOf("/"));
    const rel = "./" + path.slice(dir.length + 1); // `:./x` resolves relative to cwd; `orig` is repo-root relative
    let dead = false;
    (async () => {
      // Commit: ref^ -> ref. Staged: HEAD -> index. Unstaged: index -> working tree.
      const [a, b] = ref
        ? await Promise.all([show(dir, `${ref}^:${orig ?? rel}`), show(dir, `${ref}:${rel}`)])
        : staged
          ? await Promise.all([show(dir, `HEAD:${orig ?? rel}`), show(dir, `:${rel}`)])
          : await Promise.all([show(dir, `:${rel}`), invoke<string>("read_file", { path }).catch(() => "")]);
      if (dead) return;
      if (a.includes("\0") || b.includes("\0")) return void (note = "Binary file");
      note = a === b ? "No changes" : "";
      const lang = await languageFor(path);
      if (dead) return;
      view = new EditorView({
        parent: el,
        state: EditorState.create({
          doc: b,
          extensions: [
            lineNumbers(),
            highlightSpecialChars(),
            drawSelection(),
            EditorState.readOnly.of(true),
            keymap.of([
              { key: "F7", run: () => (nav(1), true) },
              { key: "Shift-F7", run: () => (nav(-1), true) },
            ]),
            aurelius,
            diffTheme,
            lang ?? [],
            unifiedMergeView({ original: a, mergeControls: false, gutter: true, highlightChanges: true, syntaxHighlightDeletions: true }),
          ],
        }),
      });
      total = getChunks(view.state)?.chunks.length ?? 0;
      if (scroll < 0) at = go(view, 1, -1);
      else (view.scrollDOM.scrollTop = scroll), (at = Math.min(at, total));
    })();
    return () => {
      dead = true;
      if (view) scroll = view.scrollDOM.scrollTop;
      view?.destroy();
      view = undefined;
    };
  });
</script>

<div class="diff" bind:this={el}>
  {#if note}
    <div class="note">{note}</div>
  {:else if total}
    <div class="nav">
      <span class="count">{at ? `${at} of ${total}` : total} {total === 1 ? "change" : "changes"}</span>
      <button class="icon-btn" onclick={() => nav(-1)} title="Previous change (⇧F7)"><ChevronUp size={14} strokeWidth={1.5} /></button>
      <button class="icon-btn" onclick={() => nav(1)} title="Next change (F7)"><ChevronDown size={14} strokeWidth={1.5} /></button>
    </div>
  {/if}
</div>

<style>
  .diff {
    position: absolute;
    inset: 0;
    background: var(--surface);
  }
  .diff :global(.cm-editor) {
    height: 100%;
  }
  .nav {
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
  .note {
    position: absolute;
    top: 12px;
    right: 16px;
    z-index: 5;
    padding: 2px 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--muted);
    color: var(--muted-foreground);
    font-size: 12px;
  }
</style>
