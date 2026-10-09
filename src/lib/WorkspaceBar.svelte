<script lang="ts">
  import { Flame, Gauge, Monitor, Moon, PanelLeft, Plus, Brain, Sun, X } from "@lucide/svelte";
  import { cycleTheme, theme } from "./theme.svelte";
  import { awake, toggleStayAwake } from "./stayAwake.svelte";
  import { toggleUsage, usage } from "./usageView.svelte";
  import { skills, toggleSkills } from "./skillsView.svelte";
  import { app, toggleSidebar } from "./state.svelte";
  import { pick } from "./QuickPick.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { cursorPosition, getCurrentWindow } from "@tauri-apps/api/window";
  import { zoomLevel } from "./zoom";
  import {
    activate, closeWorkspace, createWorkspace, deleteWorkspace, moveWorkspace, tearOut, nameOf, openHere, openInNewWindow, ownerWindow, renameWorkspace, ws,
  } from "./workspaces.svelte";

  let editing = $state(""); // workspace id being renamed
  const ThemeIcon = $derived({ light: Sun, dark: Moon, system: Monitor }[theme.mode]);

  async function menu(id: string) {
    const v = await pick(nameOf(id), [
      { label: "Rename", value: "rename" },
      ...(ws.open.length > 1 ? [{ label: "Open in New Window", value: "window" }, { label: "Close", value: "close" }] : []),
      { label: "Delete…", value: "delete" },
    ]);
    if (v === "rename") editing = id;
    else if (v === "window") openInNewWindow(id);
    else if (v === "close") closeWorkspace(id);
    else if (v === "delete") deleteWorkspace(id);
  }

  async function add() {
    const v = await pick("New workspace, or open one", [
      { label: "+ New workspace", value: "\0new" },
      ...ws.all
        .filter((m) => !ws.open.includes(m.id))
        .map((m) => ({ label: m.name, detail: ownerWindow(m.id) ? "Open in another window" : undefined, value: m.id })),
    ]);
    if (v === "\0new") createWorkspace();
    else if (v) openHere(v);
  }

  function finishRename(id: string, value: string) {
    if (editing !== id) return;
    editing = "";
    renameWorkspace(id, value);
  }

  // Tab drag without HTML5 drag-and-drop (it reports nothing until the drop): along the bar, pointer moves reorder.
  // Leaving the bar is detected from the OS cursor, since the webview gets no events past its edge (e.g. over the
  // title bar); then the tab tears off into its own window, which follows the cursor (Chrome-style).
  const win = getCurrentWindow();
  let bar: HTMLDivElement;
  let press: { id: string; grabX: number; x: number; at: number; moved: boolean } | null = null;
  // Thresholds that keep a slightly sloppy click a click: a drag only reorders after REORDER px, and tears off only
  // once the cursor is TEAR px past the bar (above it that's past the title bar) and the tab was held HOLD ms.
  const REORDER = 6, TEAR = 48, HOLD = 150;

  function down(e: PointerEvent, id: string) {
    if (e.button !== 0 || editing === id || (e.target as Element).closest("button")) return;
    activate(id);
    const grabX = e.clientX - (e.currentTarget as HTMLElement).getBoundingClientRect().left;
    const p = (press = { id, grabX, x: e.clientX, at: performance.now(), moved: false });
    bar.setPointerCapture(e.pointerId);
    watch(p);
  }

  function move(e: PointerEvent) {
    if (!press || !(press.moved ||= Math.abs(e.clientX - press.x) > REORDER)) return;
    const b = bar.getBoundingClientRect();
    const over = document.elementFromPoint(e.clientX, b.top + b.height / 2)?.closest<HTMLElement>("[data-ws]")?.dataset.ws;
    if (over) moveWorkspace(press.id, over);
  }

  async function watch(p: NonNullable<typeof press>) {
    const [scale, inner, size] = await Promise.all([win.scaleFactor(), win.innerPosition(), win.innerSize()]);
    const zoom = zoomLevel();
    const px = zoom * scale; // CSS px -> physical px
    const b = bar.getBoundingClientRect();
    while (press === p) {
      const [c, held] = await Promise.all([cursorPosition(), invoke<boolean>("mouse_down")]);
      if (press !== p) return;
      if (!held) return void (press = null); // released (maybe outside the window, where we'd get no pointerup)
      const [x, y] = [(c.x - inner.x) / px, (c.y - inner.y) / px]; // cursor in this page's CSS px
      const off = y < b.top - TEAR || y > b.bottom + TEAR || x < -TEAR || x > size.width / px + TEAR;
      if (off && performance.now() - p.at > HOLD) {
        press = null;
        // Only workspace: the window keeps its place under the cursor. Else the cursor holds the new window's (first) tab.
        tearOut(p.id, ws.open.length === 1 ? { x: x * zoom, y: y * zoom } : { x: (6 + p.grabX) * zoom, y: (b.height / 2) * zoom });
        return;
      }
      await new Promise(requestAnimationFrame);
    }
  }

  const up = () => (press = null);

  const focus = (el: HTMLInputElement) => (el.focus(), el.select());
</script>

<div class="bar" class:merging={ws.incoming} role="tablist" tabindex="-1" bind:this={bar} onpointermove={move} onpointerup={up} onlostpointercapture={up}>
  <span class="mark"></span>
  {#if !app.sidebarOpen}
    <button class="icon-btn" onclick={toggleSidebar} title="Show sidebar (⌘B)" aria-label="Show sidebar"><PanelLeft size={14} strokeWidth={1.5} /></button>
  {/if}
  {#each ws.open as id (id)}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
      class="ws"
      class:active={id === ws.active}
      role="tab"
      tabindex="0"
      aria-selected={id === ws.active}
      data-ws={id}
      title="{nameOf(id)}: double-click to rename, right-click for more, drag out for a new window"
      onpointerdown={(e) => down(e, id)}
      ondblclick={() => (editing = id)}
      oncontextmenu={(e) => (e.preventDefault(), menu(id))}
      onauxclick={(e) => e.button === 1 && closeWorkspace(id)}
    >
      {#if editing === id}
        <input
          value={nameOf(id)}
          use:focus
          onblur={(e) => finishRename(id, e.currentTarget.value)}
          onkeydown={(e) => {
            if (e.key === "Enter") finishRename(id, e.currentTarget.value);
            else if (e.key === "Escape") editing = "";
          }}
        />
      {:else}
        <span class="name">{nameOf(id)}</span>
      {/if}
      {#if ws.open.length > 1}
        <button class="icon-btn x" onclick={(e) => (e.stopPropagation(), closeWorkspace(id))} aria-label="Close workspace"><X size={12} strokeWidth={1.5} /></button>
      {/if}
    </div>
  {/each}
  {#if ws.incoming}<div class="ws ghost"><span class="name">{nameOf(ws.incoming)}</span></div>{/if}
  <button class="icon-btn" onclick={add} title="New workspace, or open one"><Plus size={14} strokeWidth={1.5} /></button>
  <button
    class="icon-btn view skills"
    class:on={skills.open}
    onclick={toggleSkills}
    title="Skills (⇧⌘K)"
    aria-label="Skills"
    aria-pressed={skills.open}
  >
    <Brain size={14} strokeWidth={1.5} />
  </button>
  <button
    class="icon-btn view"
    class:on={usage.open}
    onclick={toggleUsage}
    title="Usage (⇧⌘U)"
    aria-label="Usage"
    aria-pressed={usage.open}
  >
    <Gauge size={14} strokeWidth={1.5} />
  </button>
  <button
    class="icon-btn awake"
    class:on={awake.on}
    onclick={toggleStayAwake}
    title="Stay Awake: {awake.on ? 'on' : 'off'}"
    aria-label="Stay Awake"
    aria-pressed={awake.on}
  >
    <Flame size={14} strokeWidth={1.5} />
  </button>
  <button class="icon-btn" onclick={cycleTheme} title="Theme: {theme.mode}" aria-label="Theme: {theme.mode}. Click to change.">
    <ThemeIcon size={14} strokeWidth={1.5} />
  </button>
</div>

<style>
  .bar {
    grid-column: 1 / -1;
    display: flex;
    align-items: stretch;
    gap: 2px;
    height: 32px;
    padding: 0 6px;
    background: var(--sidebar);
    border-bottom: 1px solid var(--border-strong);
    overflow-x: auto;
  }
  /* Another window's workspace is dragged over this bar: release merges it in, where the ghost tab is */
  .bar.merging {
    background: var(--gold-soft);
    box-shadow: inset 0 0 0 1px var(--gold-ring);
  }
  .ghost {
    margin: 4px 0;
    border: 1px dashed var(--gold-ring);
    border-radius: 6px;
    color: var(--foreground);
    animation: ghost-in var(--dur-quick, 120ms) var(--ease-standard, ease-out);
  }
  @keyframes ghost-in {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
  }
  .bar > .icon-btn,
  .bar > .mark {
    align-self: center;
  }
  .bar > .mark {
    margin: 0 6px 0 4px;
  }
  .skills {
    margin-left: auto;
  }
  .view.on {
    background: var(--sidebar-accent);
    color: var(--sidebar-foreground);
  }
  /* Lit while keeping the Mac awake: an amber flame with a slow, soft pulse */
  .awake {
    --ember: oklch(0.68 0.18 52);
    --ember-glow: oklch(0.72 0.19 50 / 40%);
  }
  :global(.dark) .awake {
    --ember: oklch(0.8 0.16 62);
    --ember-glow: oklch(0.78 0.18 55 / 45%);
  }
  .awake.on {
    color: var(--ember);
  }
  .awake.on :global(svg) {
    fill: color-mix(in oklch, var(--ember) 35%, transparent);
    animation: glow 3s ease-in-out infinite;
  }
  /* Only colour and glow breathe; the shape stays put */
  @keyframes glow {
    0%,
    100% {
      fill: color-mix(in oklch, var(--ember) 25%, transparent);
      filter: drop-shadow(0 0 1px var(--ember-glow));
    }
    50% {
      fill: color-mix(in oklch, var(--ember) 50%, transparent);
      filter: drop-shadow(0 0 4px var(--ember-glow));
    }
  }
  /* The global reduced-motion rule shortens animations to 1ms, which would make an infinite one strobe */
  @media (prefers-reduced-motion: reduce) {
    .awake.on :global(svg) {
      animation: none;
      filter: drop-shadow(0 0 3px var(--ember-glow));
    }
  }
  .ws {
    position: relative;
    display: flex;
    align-items: center;
    gap: 6px;
    max-width: 220px;
    padding: 0 6px 0 12px;
    color: var(--muted-foreground);
    font-size: 12px;
    cursor: default;
  }
  .ws:hover,
  .ws.active {
    color: var(--foreground);
  }
  /* Gold marks the active workspace, like the sidebar tools */
  .ws.active::after {
    content: "";
    position: absolute;
    left: 8px;
    right: 8px;
    bottom: 0;
    height: 2px;
    border-radius: 9999px;
    background: var(--gold);
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  input {
    width: 140px;
    padding: 1px 4px;
    border: 1px solid var(--gold-ring);
    border-radius: 4px;
    background: var(--background);
    color: var(--foreground);
    font: inherit;
  }
  .x {
    opacity: 0;
  }
  .ws:hover .x,
  .ws.active .x {
    opacity: 1;
  }
</style>
