<script lang="ts">
  import { Columns2, Eye, GitCompare, NotebookPen, Pencil, Rows2, SquareTerminal, X } from "@lucide/svelte";
  import type { Pane } from "./layout";
  import { dockTab, moveTab, type Side } from "./layout";
  import { app, closeTab, foreignOwner, newNote, newTerminal, splitFocused } from "./state.svelte";
  import Terminal from "./Terminal.svelte";
  import Editor from "./Editor.svelte";
  import Diff from "./Diff.svelte";
  import ProviderIcon from "./ProviderIcon.svelte";
  import { agentProvider } from "./agents.svelte";
  import { providerName } from "./usageStats";

  let { pane = $bindable() }: { pane: Pane } = $props();
  const tab = $derived(pane.tabs[pane.active]);
  const label = (t: Pane["tabs"][number]) =>
    t.kind === "file" ? t.path.split("/").pop() || "Untitled" :
    t.kind === "diff" ? `${t.path.split("/").pop()} (${t.ref ? t.ref.slice(0, 7) : t.staged ? "Index" : "Working Tree"})` :
    t.agent && agentProvider[t.agent] ? providerName[agentProvider[t.agent]] :
    app.titles[t.id] || "Terminal";
  /** "folder · branch" for tabs that belong to another folder than the one in the sidebar. */
  const hint = (t: Pane["tabs"][number]) => {
    const o = foreignOwner(t);
    return o && [o.slice(o.lastIndexOf("/") + 1), app.branches[o]].filter(Boolean).join(" · ");
  };

  function drop(ev: DragEvent, index: number) {
    const id = ev.dataTransfer?.getData("text/x-tab");
    if (!id) return;
    ev.preventDefault();
    ev.stopPropagation();
    app.dragging = false;
    moveTab(app.root, id, pane.id, index);
    app.focused = pane.id;
  }

  // Drop zone under the cursor: outer 25% of each edge docks there, the middle joins this pane.
  let zone = $state<Side | "center" | null>(null);
  function over(ev: DragEvent) {
    ev.preventDefault();
    const r = (ev.currentTarget as HTMLElement).getBoundingClientRect();
    const x = (ev.clientX - r.left) / r.width, y = (ev.clientY - r.top) / r.height;
    const d = { left: x, right: 1 - x, top: y, bottom: 1 - y };
    const [side, dist] = Object.entries(d).sort((a, b) => a[1] - b[1])[0];
    zone = dist < 0.25 ? (side as Side) : "center";
  }
  function dropBody(ev: DragEvent) {
    const id = ev.dataTransfer?.getData("text/x-tab");
    const z = zone;
    zone = null;
    app.dragging = false;
    if (!id || !z) return;
    ev.preventDefault();
    if (z === "center") moveTab(app.root, id, pane.id, pane.tabs.length);
    else {
      const fresh = dockTab(app.root, id, pane.id, z);
      if (fresh) app.focused = fresh.id;
    }
  }

  // Run an action against this pane rather than whichever was focused before.
  const here = (fn: () => void) => () => ((app.focused = pane.id), fn());
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<section class="pane" class:focused={app.focused === pane.id} onfocusin={() => (app.focused = pane.id)} onpointerdown={() => (app.focused = pane.id)}>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="bar" ondragover={(e) => e.preventDefault()} ondrop={(e) => drop(e, pane.tabs.length)}>
    {#each pane.tabs as t, i (t.id)}
      <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
      <div
        class="tab"
        class:active={i === pane.active}
        draggable="true"
        title={(t.kind === "term" ? t.cwd : t.path) + (app.branches[foreignOwner(t)] ? ` (${app.branches[foreignOwner(t)]})` : "")}
        onclick={() => (pane.active = i)}
        onauxclick={(e) => e.button === 1 && closeTab(t.id)}
        ondragstart={(e) => (e.dataTransfer?.setData("text/x-tab", t.id), (app.dragging = true))}
        ondragend={() => (app.dragging = false)}
        ondrop={(e) => drop(e, i)}
      >
        {#if t.kind === "term" && t.agent && agentProvider[t.agent]}<ProviderIcon provider={agentProvider[t.agent]} size={13} />{:else if t.kind === "term"}<SquareTerminal size={13} strokeWidth={1.5} />{:else if t.kind === "diff"}<GitCompare size={13} strokeWidth={1.5} />{:else if !t.path}<NotebookPen size={13} strokeWidth={1.5} />{/if}
        <span class="name">{label(t)}</span>
        {#if hint(t)}<span class="hint">{hint(t)}</span>{/if}
        {#if app.dirty[t.id]}<span class="dot" title="Unsaved"></span>{/if}
        <button class="icon-btn x" onclick={(e) => (e.stopPropagation(), closeTab(t.id))} aria-label="Close tab"><X size={13} strokeWidth={1.5} /></button>
      </div>
    {/each}
    <span class="spacer"></span>
    {#if tab?.kind === "file" && tab.path.endsWith(".md")}
      <button class="icon-btn" onclick={() => (tab.preview = !tab.preview)} title={tab.preview ? "Edit (⌘E)" : "Preview (⌘E)"}>
        {#if tab.preview}<Pencil size={14} strokeWidth={1.5} />{:else}<Eye size={14} strokeWidth={1.5} />{/if}
      </button>
    {/if}
    <button class="icon-btn" onclick={here(newNote)} title="New note (⌘N)"><NotebookPen size={14} strokeWidth={1.5} /></button>
    <button class="icon-btn" onclick={here(newTerminal)} title="New terminal (⌘T)"><SquareTerminal size={14} strokeWidth={1.5} /></button>
    <button class="icon-btn" onclick={here(() => splitFocused("row"))} title="Split right (⌘D)"><Columns2 size={14} strokeWidth={1.5} /></button>
    <button class="icon-btn" onclick={here(() => splitFocused("col"))} title="Split down (⇧⌘D)"><Rows2 size={14} strokeWidth={1.5} /></button>
  </div>
  <div class="body">
    {#if tab}
      {#key tab.id}
        {#if tab.kind === "term"}
          <Terminal tabId={tab.id} cwd={tab.cwd} />
        {:else if tab.kind === "diff"}
          <Diff path={tab.path} staged={tab.staged} orig={tab.orig} ref={tab.ref} />
        {:else}
          <Editor tabId={tab.id} path={tab.path} preview={!!tab.preview} />
        {/if}
      {/key}
    {:else}
      <div class="empty"><span><kbd>⌘T</kbd> new terminal</span><span><kbd>⌘N</kbd> new note</span><span><kbd>⌘O</kbd> open folder</span></div>
    {/if}
    {#if app.dragging}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="drop" ondragover={over} ondragleave={() => (zone = null)} ondrop={dropBody}>
        {#if zone}<div class="zone {zone}"></div>{/if}
      </div>
    {/if}
  </div>
</section>

<style>
  .pane {
    position: relative;
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
    min-width: 0;
    min-height: 0;
  }
  .bar {
    display: flex;
    align-items: stretch;
    gap: 2px;
    height: 34px;
    flex: none;
    padding: 0 4px 0 0;
    background: var(--card);
    border-bottom: 1px solid var(--border);
    overflow-x: auto;
    scrollbar-width: none;
  }
  .tab {
    position: relative;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 4px 0 12px;
    color: var(--muted-foreground);
    font-size: 12.5px;
    cursor: pointer;
    white-space: nowrap;
    transition: color var(--dur-quick) var(--ease-standard), background-color var(--dur-quick) var(--ease-standard);
  }
  .tab:hover {
    color: var(--foreground);
  }
  .tab.active {
    background: var(--surface);
    color: var(--foreground);
    box-shadow: inset 1px 0 0 var(--border), inset -1px 0 0 var(--border);
  }
  .tab.active::after {
    content: "";
    position: absolute;
    left: 0;
    right: 0;
    bottom: -1px;
    height: 1px;
    background: var(--surface);
  }
  /* Gold = focus: only the focused pane's active tab gets the bar */
  .focused .tab.active::before {
    content: "";
    position: absolute;
    left: 0;
    right: 0;
    top: 0;
    height: 2px;
    background: var(--gold);
  }
  .name {
    max-width: 200px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .hint {
    max-width: 140px;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 11px;
    color: var(--muted-foreground);
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 9999px;
    background: var(--gold);
  }
  .spacer {
    flex: 1;
  }
  .bar > .icon-btn {
    align-self: center;
    visibility: hidden;
  }
  /* Pane actions appear only while the bar is hovered (all have shortcuts: ⌘N/⌘T/⌘D/⇧⌘D/⌘E).
     No fade: WKWebView doesn't repaint the end of a transition inside this scrolling bar, so they stayed on screen */
  .bar:hover > .icon-btn {
    visibility: visible;
  }
  .x {
    width: 20px;
    height: 20px;
    visibility: hidden;
  }
  .tab:hover .x,
  .tab.active .x {
    visibility: visible;
  }
  .body {
    position: relative;
    flex: 1;
    min-height: 0;
  }
  .drop {
    position: absolute;
    inset: 0;
    z-index: 10;
  }
  .zone {
    position: absolute;
    inset: 6px;
    background: var(--gold-soft);
    border: 1px solid var(--gold-ring);
    border-radius: var(--radius-md);
    pointer-events: none;
    transition: all var(--dur-quick) var(--ease-out-quint);
  }
  .zone.left { right: 50%; }
  .zone.right { left: 50%; }
  .zone.top { bottom: 50%; }
  .zone.bottom { top: 50%; }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    height: 100%;
    background: var(--surface);
    color: var(--muted-foreground);
    font-size: 13px;
  }
  kbd {
    display: inline-block;
    min-width: 28px;
    margin-right: 8px;
    padding: 1px 6px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--muted);
    font-family: inherit;
    font-size: 11px;
    text-align: center;
  }
</style>
