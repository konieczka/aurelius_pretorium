<script lang="ts">
  import type { Pane } from "./layout";
  import { dockTab, moveTab, type Side } from "./layout";
  import { app, closeTab, newTerminal, splitFocused } from "./state.svelte";
  import Terminal from "./Terminal.svelte";
  import Editor from "./Editor.svelte";

  let { pane = $bindable() }: { pane: Pane } = $props();
  const tab = $derived(pane.tabs[pane.active]);
  const label = (t: Pane["tabs"][number]) =>
    t.kind === "file" ? t.path.split("/").pop() : app.titles[t.id] || "Terminal";

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
        title={t.kind === "file" ? t.path : t.cwd}
        onclick={() => (pane.active = i)}
        onauxclick={(e) => e.button === 1 && closeTab(t.id)}
        ondragstart={(e) => (e.dataTransfer?.setData("text/x-tab", t.id), (app.dragging = true))}
        ondragend={() => (app.dragging = false)}
        ondrop={(e) => drop(e, i)}
      >
        <span class="name">{label(t)}</span>
        {#if app.dirty[t.id]}<span class="dot">●</span>{/if}
        <button class="x" onclick={(e) => (e.stopPropagation(), closeTab(t.id))} aria-label="Close tab">×</button>
      </div>
    {/each}
    <span class="spacer"></span>
    {#if tab?.kind === "file" && tab.path.endsWith(".md")}
      <button onclick={() => (tab.preview = !tab.preview)} title="Toggle preview (⌘E)">{tab.preview ? "Edit" : "Preview"}</button>
    {/if}
    <button onclick={here(newTerminal)} title="New terminal (⌘T)">+</button>
    <button onclick={here(() => splitFocused("row"))} title="Split right (⌘D)">◫</button>
    <button onclick={here(() => splitFocused("col"))} title="Split down (⇧⌘D)">⊟</button>
  </div>
  <div class="body">
    {#if tab}
      {#key tab.id}
        {#if tab.kind === "term"}
          <Terminal tabId={tab.id} cwd={tab.cwd} />
        {:else}
          <Editor tabId={tab.id} path={tab.path} preview={!!tab.preview} />
        {/if}
      {/key}
    {:else}
      <div class="empty">⌘T new terminal · ⌘O open folder</div>
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
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
    min-width: 0;
    min-height: 0;
    outline: 1px solid transparent;
    outline-offset: -1px;
  }
  .pane.focused {
    outline-color: #3d4a63;
  }
  .bar {
    display: flex;
    align-items: stretch;
    height: 30px;
    flex: none;
    background: #21252b;
    overflow-x: auto;
    scrollbar-width: none;
  }
  .tab {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 6px 0 12px;
    color: #9da5b4;
    font-size: 12px;
    cursor: pointer;
    white-space: nowrap;
    border-right: 1px solid #181a1f;
  }
  .tab.active {
    background: #282c34;
    color: #e6e6e6;
  }
  .focused .tab.active {
    box-shadow: inset 0 2px 0 #61afef;
  }
  .name {
    max-width: 200px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .dot {
    font-size: 9px;
    color: #e5c07b;
  }
  .spacer {
    flex: 1;
  }
  button {
    background: none;
    border: 0;
    color: #9da5b4;
    font-size: 13px;
    padding: 0 8px;
    cursor: pointer;
  }
  button:hover {
    color: #fff;
  }
  .x {
    padding: 0 4px;
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
    inset: 0;
    background: rgba(97, 175, 239, 0.18);
    border: 2px solid rgba(97, 175, 239, 0.6);
    pointer-events: none;
    transition: all 80ms;
  }
  .zone.left { right: 50%; }
  .zone.right { left: 50%; }
  .zone.top { bottom: 50%; }
  .zone.bottom { top: 50%; }
  .empty {
    display: grid;
    place-items: center;
    height: 100%;
    color: #5c6370;
    font-size: 13px;
  }
</style>
