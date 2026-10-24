<script lang="ts">
  import { Bell, BellOff, Brain, Flame, Gauge, LayoutGrid, Monitor, Moon, Plug, Sun, PanelLeft, PanelLeftClose, Plus, Settings, SlidersHorizontal, Swords, X } from "@lucide/svelte";
  import { PALETTES, setMode, theme, type Mode, type Palette } from "./theme.svelte";
  import { paletteName } from "./palette";
  import { settings, toggleSettings } from "./settingsView.svelte";
  import { awake, toggleStayAwake } from "./stayAwake.svelte";
  import { cues, toggleSoundCues } from "./soundCues.svelte";
  import { toggleUsage, usage } from "./usageView.svelte";
  import { skills, toggleSkills } from "./skillsView.svelte";
  import { mcp, toggleMcp } from "./mcpView.svelte";
  import { agents, toggleAgents } from "./agentsView.svelte";
  import { toggleWar, war } from "./warView.svelte";
  import { activity, shells } from "./agents.svelte";
  import { app, arrangePanes, toggleSidebar } from "./state.svelte";
  import { pick } from "./QuickPick.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { cursorPosition, getCurrentWindow } from "@tauri-apps/api/window";
  import { zoomLevel } from "./zoom";
  import {
    activate, closeWorkspace, createWorkspace, deleteWorkspace, moveWorkspace, tearOut, nameOf, openHere, openInNewWindow, ownerWindow, paletteOf, renameWorkspace, setWorkspacePalette, tabsIn, ws,
  } from "./workspaces.svelte";

  let editing = $state(""); // workspace id being renamed

  // Background workspaces' agents: a dot on their tab while one needs you, or works. Re-read every poll: tabs move
  // between workspaces without telling us.
  const wsActivity = $derived.by(() => {
    void shells.polled;
    return Object.fromEntries(
      ws.open.map((id) => {
        const a = tabsIn(id).map((t) => activity[t.id]);
        return [id, a.includes("attention") ? "attention" : a.includes("busy") ? "busy" : ""];
      }),
    );
  });

  // Light, dark, or following macOS: one click to the next (Settings has all three side by side).
  const modes: Record<Mode, { label: string; icon: typeof Sun }> = { light: { label: "Light", icon: Sun }, dark: { label: "Dark", icon: Moon }, system: { label: "System", icon: Monitor } };
  const nextMode: Record<Mode, Mode> = { light: "dark", dark: "system", system: "light" };
  const cycleTheme = () => setMode(nextMode[theme.mode]);
  const ThemeIcon = $derived(modes[theme.mode].icon);

  async function paletteMenu(id: string) {
    const own = paletteOf(id);
    const v = await pick(`Palette for “${nameOf(id)}”`, [
      { label: "Use global palette", detail: paletteName(theme.globalPalette), value: "__global", hint: own === undefined ? "Current" : undefined },
      ...PALETTES.map((p) => ({ label: p.name, detail: p.blurb, value: p.id, hint: own === p.id ? "Current" : undefined })),
    ]);
    if (v === null) return;
    setWorkspacePalette(id, v === "__global" ? null : (v as Palette));
  }

  async function menu(id: string) {
    const own = paletteOf(id);
    const v = await pick(nameOf(id), [
      { label: "Rename", value: "rename" },
      { label: "Palette…", detail: own ? paletteName(own) : `Global · ${paletteName(theme.globalPalette)}`, value: "palette" },
      ...(ws.open.length > 1 ? [{ label: "Open in New Window", value: "window" }, { label: "Close", value: "close" }] : []),
      { label: "Delete…", value: "delete" },
    ]);
    if (v === "rename") editing = id;
    else if (v === "palette") paletteMenu(id);
    else if (v === "window") openInNewWindow(id);
    else if (v === "close") closeWorkspace(id);
    else if (v === "delete") deleteWorkspace(id);
  }

  // Closed workspaces can be deleted from here (open ones from their tab's menu).
  async function add() {
    const v = await pick("New workspace, or open one", [
      { label: "+ New workspace", value: "\0new" },
      ...ws.all
        .filter((m) => !ws.open.includes(m.id))
        .map((m) => {
          const elsewhere = !!ownerWindow(m.id);
          return { label: m.name, detail: elsewhere ? "Open in another window" : undefined, value: m.id, removable: !elsewhere && "Delete workspace" };
        }),
    ], deleteWorkspace);
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

  // The bar is the title bar (overlay style): it leaves room for the traffic lights, except in full screen where
  // macOS hides them.
  let fullscreen = $state(false);
  $effect(() => {
    const check = () => win.isFullscreen().then((f) => (fullscreen = f), () => {});
    check();
    const off = win.onResized(check);
    return () => void off.then((f) => f());
  });

  const focus = (el: HTMLInputElement) => (el.focus(), el.select());
</script>

<!-- Plane 0: drawn straight on the field. Bare field (data-tauri-drag-region, which doesn't reach children) drags the
     window; the workspace tabs and the tools sit in glass capsules. -->
<div class="bar" class:merging={ws.incoming} class:fullscreen data-tauri-drag-region role="tablist" tabindex="-1" bind:this={bar} onpointermove={move} onpointerup={up} onlostpointercapture={up}>
  <!-- Always here, before the logo, so toggling the sidebar doesn't shift the tabs -->
  <button
    class="icon-btn"
    onclick={toggleSidebar}
    title="{app.sidebarOpen ? 'Hide' : 'Show'} sidebar (⌘B)"
    aria-label="{app.sidebarOpen ? 'Hide' : 'Show'} sidebar"
  >
    {#if app.sidebarOpen}<PanelLeftClose size={14} strokeWidth={1.5} />{:else}<PanelLeft size={14} strokeWidth={1.5} />{/if}
  </button>
  <span class="mark" data-tauri-drag-region></span>
  <span class="wordmark" data-tauri-drag-region>Pretorium</span>
  <div class="wss glass">
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
        {#if id !== ws.active && paletteOf(id)}<span class="ws-dot" data-palette={paletteOf(id)} title="Palette: {paletteName(paletteOf(id)!)}" aria-hidden="true"></span>{/if}
        <span class="name">{nameOf(id)}</span>
      {/if}
      {#if id !== ws.active && wsActivity[id] === "attention"}<span class="attention" title="An agent here needs your attention"></span>
      {:else if id !== ws.active && wsActivity[id] === "busy"}<span class="working" title="Agents working"></span>{/if}
      {#if ws.open.length > 1}
        <button class="icon-btn x" onclick={(e) => (e.stopPropagation(), closeWorkspace(id))} aria-label="Close workspace"><X size={12} strokeWidth={1.5} /></button>
      {/if}
    </div>
  {/each}
  {#if ws.incoming}<div class="ws ghost"><span class="name">{nameOf(ws.incoming)}</span></div>{/if}
  <button class="icon-btn" onclick={add} title="New workspace, or open one"><Plus size={14} strokeWidth={1.5} /></button>
  </div>
  <span class="drag" data-tauri-drag-region></span>
  <div class="dock glass">
  <button class="icon-btn" onclick={arrangePanes} title="Auto-arrange panes" aria-label="Auto-arrange panes">
    <LayoutGrid size={14} strokeWidth={1.5} />
  </button>
  <button class="icon-btn" onclick={cycleTheme} title="Theme: {modes[theme.mode].label} (next: {modes[nextMode[theme.mode]].label})" aria-label="Theme: {modes[theme.mode].label}">
    <ThemeIcon size={14} strokeWidth={1.5} />
  </button>
  <button
    class="icon-btn view"
    class:on={war.open}
    onclick={toggleWar}
    title="War Table: every project's agents and servers on one canvas (⇧⌘Y)"
    aria-label="War Table"
    aria-pressed={war.open}
  >
    <Swords size={14} strokeWidth={1.5} />
  </button>
  <span class="sep"></span>
  <button
    class="icon-btn view"
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
    class:on={mcp.open}
    onclick={toggleMcp}
    title="MCP servers (⇧⌘M)"
    aria-label="MCP servers"
    aria-pressed={mcp.open}
  >
    <Plug size={14} strokeWidth={1.5} />
  </button>
  <button
    class="icon-btn view"
    class:on={agents.open}
    onclick={toggleAgents}
    title="Agents (⇧⌘A)"
    aria-label="Agents"
    aria-pressed={agents.open}
  >
    <SlidersHorizontal size={14} strokeWidth={1.5} />
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
  <span class="sep"></span>
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
  <button
    class="icon-btn cues"
    class:on={cues.on}
    onclick={toggleSoundCues}
    title="Sound Cues: {cues.on ? 'on' : 'off'} (a sound when an agent off screen asks something or finishes)"
    aria-label="Sound Cues"
    aria-pressed={cues.on}
  >
    {#if cues.on}<Bell size={14} strokeWidth={1.5} />{:else}<BellOff size={14} strokeWidth={1.5} />{/if}
  </button>
  <button
    class="icon-btn view"
    class:on={settings.open}
    onclick={toggleSettings}
    title="Settings (⌘,)"
    aria-label="Settings"
    aria-pressed={settings.open}
  >
    <Settings size={14} strokeWidth={1.5} />
  </button>
  </div>
</div>

<style>
  .bar {
    grid-column: 1 / -1;
    display: flex;
    align-items: center;
    gap: 6px;
    height: 44px;
    /* Room for the traffic lights (trafficLightPosition in tauri.conf.json / newWindow) */
    padding: 0 0 0 78px;
    margin: 0 calc(-1 * var(--gap));
    padding-right: var(--gap);
    overflow-x: auto;
    scrollbar-width: none;
  }
  .bar.fullscreen {
    padding-left: var(--gap);
  }
  /* Another window's workspace is dragged over this bar: release merges it in, where the ghost tab is */
  .bar.merging .wss {
    background: var(--gold-soft);
    box-shadow: var(--sh-chrome), inset 0 0 0 1px var(--gold-ring);
  }
  .ghost {
    border: 1px dashed var(--gold-ring);
    color: var(--foreground);
    animation: ghost-in var(--dur-quick, 120ms) var(--ease-standard, ease-out);
  }
  @keyframes ghost-in {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
  }
  .bar > .mark {
    width: 19px;
    height: 19px;
    margin-left: 4px;
  }
  .wordmark {
    margin: 0 8px 0 2px;
    font-family: var(--font-cinzel);
    font-size: 12.5px;
    letter-spacing: 0.14em;
    text-transform: uppercase;
    color: var(--muted-foreground);
    white-space: nowrap;
  }
  /* Glass capsules: workspace tabs on the left, tools on the right */
  .wss,
  .dock {
    display: flex;
    align-items: center;
    flex: none;
    gap: 2px;
    height: 34px;
    padding: 0 3px;
    box-sizing: border-box;
    border-radius: 13px;
  }
  .wss .icon-btn,
  .dock .icon-btn {
    width: 28px;
    height: 28px;
  }
  .drag {
    flex: 1;
    align-self: stretch;
    min-width: 24px;
  }
  .sep {
    flex: none;
    width: 1px;
    height: 16px;
    margin: 0 3px;
    background: var(--chrome-edge);
  }
  :global(:root:not(.dark)) .sep {
    background: oklch(var(--shadow-tint) / 0.12);
  }
  .view.on {
    background: var(--chip-on);
    box-shadow: var(--sh-chip);
    color: var(--foreground);
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
  /* Lit while sounds play: a gold bell (off shows the struck-through one, dimmed like any idle tool) */
  .cues.on {
    color: var(--gold);
  }
  .ws {
    position: relative;
    display: flex;
    align-items: center;
    gap: 7px;
    max-width: 220px;
    height: 28px;
    box-sizing: border-box;
    padding: 0 4px 0 12px;
    border-radius: var(--r-control);
    color: var(--muted-foreground);
    font-size: 12.5px;
    cursor: default;
    transition: color var(--dur-quick) var(--ease-standard), background-color var(--dur-quick) var(--ease-standard);
  }
  .ws:hover {
    background: var(--hover);
    color: var(--foreground);
  }
  /* The active workspace is a raised chip with a gold lamp */
  .ws.active {
    background: var(--chip-on);
    box-shadow: var(--sh-chip);
    color: var(--foreground);
  }
  .ws.active::before {
    content: "";
    flex: none;
    width: 5px;
    height: 5px;
    margin-left: -2px;
    border-radius: 9999px;
    background: var(--gold);
    box-shadow: 0 0 8px var(--gold);
  }
  /* Only workspace: no close button, so keep the right padding even */
  .ws:not(:has(.x)) {
    padding-right: 12px;
  }
  /* A workspace with its own palette: a dot in that palette's accent colour */
  .ws-dot {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 9999px;
    background: var(--gold);
    box-shadow: 0 0 6px color-mix(in oklch, var(--gold) 60%, transparent);
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
    border-radius: 6px;
    background: var(--background);
    color: var(--foreground);
    font: inherit;
  }
  /* Same marks as the pane tabs: an ember dot when an agent needs you; a gold one breathing while they work */
  .attention,
  .working {
    flex: none;
    width: 6px;
    height: 6px;
    border-radius: 9999px;
  }
  .attention {
    background: oklch(0.7 0.17 50);
    box-shadow: 0 0 0 2px oklch(0.7 0.17 50 / 25%);
  }
  .working {
    background: var(--gold);
    animation: pulse 2.4s ease-in-out infinite;
  }
  @keyframes pulse {
    0%,
    100% {
      opacity: 0.35;
      transform: scale(0.8);
    }
    50% {
      opacity: 1;
      transform: scale(1.1);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .working {
      animation: none;
    }
  }
  .x {
    width: 20px !important;
    height: 20px !important;
    border-radius: 6px;
    opacity: 0;
  }
  .ws:hover .x,
  .ws.active .x {
    opacity: 1;
  }
</style>
