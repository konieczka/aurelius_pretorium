<script module lang="ts">
  import { app } from "./state.svelte";
  import { agentProvider } from "./agents.svelte";
  import { providerName } from "./usageStats";
  import type { Tab } from "./layout";
  import { hostOf } from "./web";

  export const tabLabel = (t: Tab) =>
    t.kind === "file" ? t.path.split("/").pop() || "Untitled" :
    t.kind === "web" ? t.title || (t.url ? hostOf(t.url) : "New Tab") :
    t.kind === "diff" ? `${t.path.split("/").pop()} (${t.base && t.ref ? `${t.base.slice(0, 7)}..${t.ref.slice(0, 7)}` : t.ref ? t.ref.slice(0, 7) : t.staged ? "Index" : "Working Tree"})` :
    t.name || (t.agent && agentProvider[t.agent] ? providerName[agentProvider[t.agent]] : app.titles[t.id] || "Terminal");
</script>

<script lang="ts">
  import { popupMenu, sep } from "./contextMenu";
  import { Columns2, Eye, Folder, FolderGit2, GitBranch, GitCompare, GitFork, Globe, NotebookPen, Pencil, Rows2, SquareTerminal, TextWrap, X } from "@lucide/svelte";
  import type { Pane } from "./layout";
  import { dockTab, moveTab, type Side } from "./layout";
  import { closeTab, foreignOwner, minimizeTab, newNote, newTerminal, openBrowser, refreshRepo, repoKey, splitFocused, tabOwner } from "./state.svelte";
  import { moveTabTo, ws } from "./workspaces.svelte";
  import Terminal from "./Terminal.svelte";
  import Editor from "./Editor.svelte";
  import Diff from "./Diff.svelte";
  import Media from "./Media.svelte";
  import Browser, { icons } from "./Browser.svelte";
  import { hasPreview, mediaKind } from "./media";
  import ProviderIcon from "./ProviderIcon.svelte";
  import { activity } from "./agents.svelte";

  let { pane = $bindable() }: { pane: Pane } = $props();
  const tab = $derived(pane.tabs[pane.active]);

  let renaming = $state(""); // tab id with the name input
  function finishRename(t: Tab, value: string) {
    if (renaming !== t.id) return; // Enter, then the blur from unmounting
    renaming = "";
    if (t.kind === "term") t.name = value.trim() || undefined; // empty = back to the automatic name
  }
  const focusInput = (el: HTMLInputElement) => (el.focus(), el.select());

  /** Right-click on a terminal tab. */
  async function tabMenu(e: MouseEvent, t: Tab) {
    if (t.kind !== "term") return;
    e.preventDefault();
    const others = ws.all.filter((m) => m.id !== ws.active);
    await popupMenu([
      { text: "Rename…", action: () => (renaming = t.id) },
      {
        text: "Move to Workspace",
        items: [
          ...others.map((m) => ({ text: m.name, action: () => moveTabTo(t.id, m.id) })),
          ...(others.length ? [sep] : []),
          { text: "New Workspace", action: () => moveTabTo(t.id, "") },
        ],
      },
      { text: "Minimize", action: () => minimizeTab(t.id) },
      sep,
      { text: "Close", action: () => closeTab(t.id) },
    ]);
  }

  // Header above the content: which project / worktree / branch this tab belongs to, and where in it.
  const base = (p: string) => p.slice(p.lastIndexOf("/") + 1);
  const owner = $derived(tab ? tabOwner(tab) : "");
  const key = $derived(tab ? repoKey(tab) : "");
  const repo = $derived(app.repos[key]);
  $effect(() => {
    if (key && !(key in app.repos)) refreshRepo(key); // new folder: don't wait for the next git poll
  });
  const where = $derived.by(() => {
    if (!tab || !owner) return null;
    const root = repo?.root || owner;
    const project = base(repo?.main || root);
    // Worktrees made here are named <main>-<branch>; the prefix just repeats the project
    const wt = !repo?.main ? "" : base(root).startsWith(project + "-") ? base(root).slice(project.length + 1) : base(root);
    if (tab.kind === "web") return null; // no folder: the page has its own address bar
    const full = tab.kind === "term" ? tab.cwd : tab.path;
    const rel = full.startsWith(root + "/") ? full.slice(root.length + 1) : full === root ? "" : full;
    const branch = repo?.branch ?? "";
    return { project, wt: wt && wt === branch ? "worktree" : wt, branch, git: !!repo?.root, rel: tab.kind === "term" ? rel : rel.slice(0, rel.lastIndexOf("/") + 1), foreign: !!foreignOwner(tab) };
  });

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

  // Too many tabs to fit: the strip scrolls sideways. A plain mouse wheel scrolls it too, and the active tab is kept in view.
  let strip = $state<HTMLElement>();
  let overflowing = $state(false);
  $effect(() => {
    if (!strip) return;
    const el = strip;
    const check = () => (overflowing = el.scrollWidth > el.clientWidth);
    const ro = new ResizeObserver(check);
    ro.observe(el);
    const mo = new MutationObserver(check); // tabs added, closed or renamed
    mo.observe(el, { childList: true, subtree: true, characterData: true });
    return () => (ro.disconnect(), mo.disconnect());
  });
  function wheel(e: WheelEvent) {
    if (!strip || !overflowing || Math.abs(e.deltaX) >= Math.abs(e.deltaY)) return;
    e.preventDefault();
    strip.scrollLeft += e.deltaY;
  }
  // Sideways only: scrollIntoView would also nudge the strip vertically
  $effect(() => {
    void pane.tabs.length;
    const t = strip?.children[pane.active] as HTMLElement | undefined;
    if (!strip || !t) return;
    const left = t.offsetLeft, right = left + t.offsetWidth;
    if (left < strip.scrollLeft) strip.scrollLeft = left;
    else if (right > strip.scrollLeft + strip.clientWidth) strip.scrollLeft = right - strip.clientWidth;
  });
  // The strip is a fixed-height row: overflow-y hidden still lets WebKit scroll it vertically, so pin it to the top
  const pinTop = (e: Event) => ((e.currentTarget as HTMLElement).scrollTop = 0);

  // Dragging a tab near either end of the strip scrolls it, faster the closer to the edge
  const EDGE = 40, MAX_SPEED = 14;
  let edgeSpeed = 0, edgeFrame = 0;
  function dragEdge(e: DragEvent) {
    if (!strip || !overflowing) return;
    const r = strip.getBoundingClientRect();
    const fromLeft = e.clientX - r.left, fromRight = r.right - e.clientX;
    edgeSpeed = fromLeft < EDGE ? -MAX_SPEED * (1 - fromLeft / EDGE) : fromRight < EDGE ? MAX_SPEED * (1 - fromRight / EDGE) : 0;
    if (edgeSpeed && !edgeFrame) edgeFrame = requestAnimationFrame(edgeStep);
  }
  function edgeStep() {
    if (!strip || !edgeSpeed || !app.dragging) return stopEdge();
    strip.scrollLeft += edgeSpeed;
    edgeFrame = requestAnimationFrame(edgeStep);
  }
  function stopEdge() {
    cancelAnimationFrame(edgeFrame);
    edgeSpeed = edgeFrame = 0;
  }
  function leaveStrip(e: DragEvent) {
    if (!strip?.contains(e.relatedTarget as Node)) stopEdge();
  }
  $effect(() => stopEdge);

  // Run an action against this pane rather than whichever was focused before.
  const here = (fn: () => void) => () => ((app.focused = pane.id), fn());
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<section class="pane slab" class:focused={app.focused === pane.id} onfocusin={() => (app.focused = pane.id)} onpointerdown={() => (app.focused = pane.id)}>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="bar" ondragover={(e) => e.preventDefault()} ondrop={(e) => drop(e, pane.tabs.length)}>
    <div class="tabs" class:overflowing bind:this={strip} onwheel={wheel} onscroll={pinTop} ondragover={dragEdge} ondragleave={leaveStrip} ondrop={stopEdge}>
    {#each pane.tabs as t, i (t.id)}
      <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
      <div
        class="tab"
        class:active={i === pane.active}
        draggable={renaming !== t.id}
        title={t.kind === "term" ? t.cwd : t.kind === "web" ? t.url : t.path}
        onclick={() => (pane.active = i)}
        onauxclick={(e) => e.button === 1 && closeTab(t.id)}
        oncontextmenu={(e) => tabMenu(e, t)}
        ondblclick={() => t.kind === "term" && (renaming = t.id)}
        ondragstart={(e) => (e.dataTransfer?.setData("text/x-tab", t.id), (app.dragging = true))}
        ondragend={() => (app.dragging = false)}
        ondrop={(e) => drop(e, i)}
      >
        {#if t.kind === "term" && t.agent && agentProvider[t.agent]}<span class="agent" class:working={activity[t.id] === "busy"} title={activity[t.id] === "busy" ? "Working" : undefined}><ProviderIcon provider={agentProvider[t.agent]} size={13} /></span>{:else if t.kind === "term"}<SquareTerminal size={13} strokeWidth={1.5} />{:else if t.kind === "diff"}<GitCompare size={13} strokeWidth={1.5} />{:else if t.kind === "web" && icons[t.id]}<img class="favicon" src={icons[t.id].src} alt="" onerror={() => delete icons[t.id]} />{:else if t.kind === "web"}<Globe size={13} strokeWidth={1.5} />{:else if !t.path}<NotebookPen size={13} strokeWidth={1.5} />{/if}
        {#if renaming === t.id}
          <input
            class="rename"
            value={tabLabel(t)}
            use:focusInput
            onclick={(e) => e.stopPropagation()}
            onblur={(e) => finishRename(t, e.currentTarget.value)}
            onkeydown={(e) => {
              if (e.key === "Enter") finishRename(t, e.currentTarget.value);
              else if (e.key === "Escape") renaming = "";
            }}
          />
        {:else}
          <span class="name">{tabLabel(t)}</span>
        {/if}
        {#if activity[t.id] === "attention"}<span class="attention" title="Needs your attention"></span>{/if}
        {#if app.dirty[t.id]}<span class="dot" title="Unsaved"></span>{/if}
        <button class="icon-btn x" onclick={(e) => (e.stopPropagation(), closeTab(t.id))} aria-label="Close tab"><X size={13} strokeWidth={1.5} /></button>
      </div>
    {/each}
    </div>
    <button class="icon-btn" onclick={here(newNote)} title="New note (⌘N)"><NotebookPen size={14} strokeWidth={1.5} /></button>
    <button class="icon-btn" onclick={here(newTerminal)} title="New terminal (⌘T)"><SquareTerminal size={14} strokeWidth={1.5} /></button>
    <button class="icon-btn" onclick={here(() => openBrowser())} title="New browser tab (⇧⌘T)"><Globe size={14} strokeWidth={1.5} /></button>
    <button class="icon-btn" onclick={here(() => splitFocused("row"))} title="Split right (⌘D)"><Columns2 size={14} strokeWidth={1.5} /></button>
    <button class="icon-btn" onclick={here(() => splitFocused("col"))} title="Split down (⇧⌘D)"><Rows2 size={14} strokeWidth={1.5} /></button>
  </div>
  {#if where}
    <div class="where" title={(tab.kind === "term" ? tab.cwd : tab.kind === "web" ? tab.url : tab.path) + (where.foreign ? "\nNot the folder in the sidebar" : "")}>
      <span class="seg project">{#if where.git}<FolderGit2 size={12} strokeWidth={1.5} />{:else}<Folder size={12} strokeWidth={1.5} />{/if}{where.project}</span>
      {#if where.wt}<span class="seg" title="Worktree: {repo?.root}"><GitFork size={12} strokeWidth={1.5} />{where.wt}</span>{/if}
      {#if where.branch}<span class="seg" title="Branch"><GitBranch size={12} strokeWidth={1.5} />{where.branch}</span>{/if}
      {#if where.rel}<span class="rel">{where.rel}</span>{/if}
      {#if tab.kind === "file" && !mediaKind(tab.path)}
        <span class="actions">
          {#if !tab.preview}
            <button class="icon-btn wrap" class:on={tab.wrap !== false} onclick={() => (tab.wrap = tab.wrap === false)} title={tab.wrap !== false ? "Don't wrap lines" : "Wrap lines"} aria-pressed={tab.wrap !== false}><TextWrap size={13} strokeWidth={1.5} /></button>
          {/if}
          {#if hasPreview(tab.path)}
            <button class="icon-btn" onclick={() => (tab.preview = !tab.preview)} title={tab.preview ? "Edit (⌘E)" : "Preview (⌘E)"}>
              {#if tab.preview}<Pencil size={13} strokeWidth={1.5} />{:else}<Eye size={13} strokeWidth={1.5} />{/if}
            </button>
          {/if}
        </span>
      {/if}
    </div>
  {/if}
  <div class="body">
    {#if tab}
      {#key tab.id}
        {#if tab.kind === "term"}
          <Terminal tabId={tab.id} cwd={tab.cwd} />
        {:else if tab.kind === "web"}
          <Browser {tab} />
        {:else if tab.kind === "diff"}
          <Diff path={tab.path} staged={tab.staged} orig={tab.orig} ref={tab.ref} base={tab.base} />
        {:else if mediaKind(tab.path)}
          <Media path={tab.path} />
        {:else}
          <Editor tabId={tab.id} path={tab.path} preview={!!tab.preview} wrap={tab.wrap !== false} />
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
    transition: box-shadow var(--dur-base) var(--ease-standard);
  }
  /* Gold = focus: the focused slab gets a gold hairline and halo, its active tab a gold tick */
  .pane.focused {
    box-shadow: var(--sh-slab), var(--ring-focus);
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 3px;
    height: 40px;
    flex: none;
    padding: 0 6px;
    background: var(--slab-head);
  }
  /* The tabs scroll on their own, under a thin scrollbar along the bottom edge, so the pane actions stay put.
     Tabs sit at the top of a full-height strip so they don't shift when the scrollbar appears */
  .tabs {
    position: relative; /* offsetParent for the tabs */
    display: flex;
    align-items: flex-start;
    gap: 3px;
    flex: 1;
    min-width: 0;
    height: 100%;
    padding-top: 6px;
    overflow-x: auto;
    overflow-y: hidden;
  }
  .tabs::-webkit-scrollbar {
    height: 6px;
  }
  /* The scrollbar takes the bottom 6px and clips what's under it: lift the tabs and tuck the gold tick in above it */
  .tabs.overflowing {
    padding-top: 2px;
  }
  .focused .tabs.overflowing .tab.active::before {
    bottom: -3px;
  }
  /* Tabs are chips, not notches */
  .tab {
    position: relative;
    display: flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 4px 0 10px;
    border-radius: var(--r-control);
    color: var(--muted-foreground);
    font-size: 12.5px;
    flex: none;
    cursor: pointer;
    white-space: nowrap;
    transition: color var(--dur-quick) var(--ease-standard), background-color var(--dur-quick) var(--ease-standard);
  }
  .tab:hover {
    background: var(--hover);
    color: var(--foreground);
  }
  .tab.active {
    background: var(--chip-on);
    box-shadow: var(--sh-chip);
    color: var(--foreground);
  }
  .focused .tab.active::before {
    content: "";
    position: absolute;
    left: 50%;
    bottom: -6px;
    width: 18px;
    height: 2px;
    margin-left: -9px;
    border-radius: 9999px;
    background: var(--gold);
    box-shadow: 0 0 8px color-mix(in oklch, var(--gold) 70%, transparent);
  }
  .name {
    max-width: 200px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .rename {
    width: 140px;
    padding: 1px 4px;
    border: 1px solid var(--gold-ring);
    border-radius: 4px;
    background: var(--background);
    color: var(--foreground);
    font: inherit;
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 9999px;
    background: var(--gold);
  }
  /* Agent state: while it works its icon breathes, with a dot pulsing beneath in step; an ember dot after the name
     when it's done or asking while you were elsewhere */
  .favicon {
    width: 13px;
    height: 13px;
    flex: none;
    object-fit: contain;
    border-radius: 2px;
  }
  .agent {
    position: relative;
    display: flex;
  }
  .agent.working :global(svg) {
    animation: breathe 2.4s ease-in-out infinite;
  }
  .agent.working::after {
    content: "";
    position: absolute;
    left: 50%;
    bottom: -5px;
    width: 3px;
    height: 3px;
    margin-left: -1.5px;
    border-radius: 9999px;
    background: var(--gold);
    animation: pulse 2.4s ease-in-out infinite;
  }
  /* Only brightness and glow breathe; the shape stays put */
  @keyframes breathe {
    0%,
    100% {
      opacity: 0.55;
      filter: none;
    }
    50% {
      opacity: 1;
      filter: drop-shadow(0 0 3px color-mix(in oklch, var(--gold) 70%, transparent));
    }
  }
  @keyframes pulse {
    0%,
    100% {
      opacity: 0.35;
      transform: scale(0.8);
    }
    50% {
      opacity: 1;
      transform: scale(1.3);
    }
  }
  .attention {
    width: 7px;
    height: 7px;
    border-radius: 9999px;
    background: oklch(0.7 0.17 50);
    box-shadow: 0 0 0 2px oklch(0.7 0.17 50 / 25%);
  }
  /* The global reduced-motion rule shortens animations to 1ms, which would make an infinite one strobe */
  @media (prefers-reduced-motion: reduce) {
    .agent.working :global(svg),
    .agent.working::after {
      animation: none;
    }
    .agent.working :global(svg) {
      filter: drop-shadow(0 0 2px color-mix(in oklch, var(--gold) 70%, transparent));
    }
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
    border-radius: 6px;
    visibility: hidden;
  }
  .tab:hover .x,
  .tab.active .x {
    visibility: visible;
  }
  .where {
    display: flex;
    align-items: center;
    gap: 12px;
    height: 26px;
    flex: none;
    padding: 0 14px;
    background: var(--surface);
    border-bottom: 1px solid color-mix(in oklch, var(--border) 60%, transparent);
    color: var(--muted-foreground);
    font-size: 11.5px;
    white-space: nowrap;
    overflow: hidden;
  }
  .actions {
    display: flex;
    margin-left: auto;
    flex: none;
  }
  .actions > .icon-btn {
    width: 20px;
    height: 20px;
  }
  .wrap.on {
    color: var(--gold);
  }
  .seg {
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
    flex: none;
  }
  .project {
    color: var(--foreground);
    font-weight: 500;
  }
  .rel {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
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
    inset: 8px;
    background: var(--gold-soft);
    border: 1px solid var(--gold-ring);
    border-radius: var(--r-control);
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
