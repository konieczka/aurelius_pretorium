<script module lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import type { Row as UsageRow } from "./usageStats";
  import type { Stats } from "./war";

  // Kept between openings of the War Table: project stats are slow to count, so each project's are
  // kept for a few minutes; token rows come from the same logs the Usage view reads.
  const STATS_TTL = 5 * 60_000;
  const statsOf = $state<Record<string, Stats>>({});
  const statsAt: Record<string, number> = {};
  let counting = false;
  async function countStats(keys: string[]) {
    if (counting) return;
    counting = true;
    try {
      for (const k of keys) {
        if (Date.now() - (statsAt[k] ?? 0) < STATS_TTL) continue;
        statsAt[k] = Date.now();
        statsOf[k] = await invoke<Stats>("project_stats", { root: k }).catch(() => statsOf[k]);
      }
    } finally {
      counting = false;
    }
  }

  const usage = $state({ rows: null as UsageRow[] | null, at: 0 });
  async function readUsage() {
    if (Date.now() - usage.at < 60_000) return;
    usage.at = Date.now();
    usage.rows = await invoke<UsageRow[]>("usage_stats", { days: 30 }).catch(() => usage.rows);
  }

  // Where the table was looking, per view, while it's closed or switched away from
  const views = new Map<string, { x: number; y: number; k: number; auto: boolean }>();
</script>

<script lang="ts">
  import { untrack } from "svelte";
  import { GitBranch, MessageCircleQuestion, ShieldQuestion } from "@lucide/svelte";
  import { agentProvider, shells } from "./agents.svelte";
  import { fleetRows, fmtSpan, STATUSES, type Row, type Status } from "./fleet";
  import { app, openBrowser, ownerOf, refreshRepo } from "./state.svelte";
  import { activate, folderOf, nameOf, ownerWindow, showTab, tabsIn, ws } from "./workspaces.svelte";
  import { infra, refreshInfra, webContainerPort, webPort } from "./Infra.svelte";
  import { providerName, rangeStart } from "./usageStats";
  import { buildProjects, castraLayout, frontLayout, tally, tokensByProject, union, F, type Project, type Rect, type Service, type WarView } from "./war";
  import ProviderIcon from "./ProviderIcon.svelte";
  import WarCastra from "./WarCastra.svelte";
  import WarFront from "./WarFront.svelte";
  import { setWarView, war } from "./warView.svelte";

  const view = $derived<WarView>(war.view);

  // ---- data: Fleet's agents, the Servers scan, every workspace's folder ----
  const rows = $derived.by(() => {
    void shells.polled;
    return fleetRows(ws.all.map((m) => ({ id: m.id, tabs: tabsIn(m.id) })), shells.all);
  });
  const folders = $derived.by(() => {
    void shells.polled; // other windows' workspaces change folder without telling us
    return [...new Set(ws.all.map((m) => folderOf(m.id)).filter(Boolean))];
  });
  // A project is a repo, its worktrees included; else the opened folder it's in, or the folder itself (as Fleet)
  const keyOf = (path: string) => {
    const repo = app.repos[path];
    if (repo?.root) return repo.main || repo.root;
    const owner = ownerOf(path, true);
    return app.recent.includes(owner) ? owner : path;
  };
  const projects = $derived(buildProjects(rows, infra.scan?.listeners ?? [], infra.scan?.containers ?? [], folders, keyOf));
  const keys = $derived(projects.map((p) => p.key));

  // Branches and checkouts of every folder in sight, looked up once and every 15s (agents switch branches)
  const paths = $derived([...new Set([...rows.map((r) => r.cwd), ...folders, ...(infra.scan?.listeners ?? []).map((l) => l.root), ...(infra.scan?.containers ?? []).map((c) => c.root)].filter(Boolean))]);
  const asked = new Set<string>();
  $effect(() => {
    for (const p of paths) if (!asked.has(p)) asked.add(p), refreshRepo(p);
  });
  $effect(() => {
    // The Servers scan polls slowly unless its sidebar mode is up; the table wants it live
    refreshInfra();
    let ticks = 0;
    const t = setInterval(() => {
      if (!document.hasFocus()) return;
      refreshInfra();
      if (++ticks % 5 === 0) untrack(() => paths).forEach(refreshRepo);
      if (ticks % 20 === 0) untrack(() => (countStats(keys), readUsage()));
    }, 3000);
    return () => clearInterval(t);
  });
  $effect(() => void countStats(keys));
  $effect(() => void readUsage());
  const tokens = $derived(tokensByProject(usage.rows ?? [], keys, rangeStart(1)));

  const info = $derived({ stats: statsOf, tokens, branch: (k: string) => app.repos[k]?.branch ?? "" });

  // ---- layout ----
  let stage = $state<HTMLElement>();
  let sw = $state(0), sh = $state(0);
  $effect(() => {
    if (!stage) return;
    const el = stage;
    const ro = new ResizeObserver(() => ((sw = el.clientWidth), (sh = el.clientHeight)));
    ro.observe(el);
    return () => ro.disconnect();
  });
  const camps = $derived(view === "castra" ? castraLayout(projects, sw || 1200, sh || 800) : []);
  const front = $derived(view === "front" ? frontLayout(projects) : null);
  const rects = $derived.by((): Record<string, Rect> => {
    if (front) return Object.fromEntries(front.lanes.map((l) => [l.key, { x: l.x, y: 0, w: F.LANE_W, h: front.h }]));
    return Object.fromEntries(camps.map((c) => [c.key, { x: c.x, y: c.y, w: c.w, h: c.h }]));
  });
  // Front line keeps a gutter left of its first column for the band labels
  const bounds = $derived(union([...Object.values(rects), ...(front ? [{ x: -210, y: 0, w: 1, h: 1 }] : [])]));

  // ---- viewport ----
  const v = $state({ x: 0, y: 0, k: 1 });
  let auto = true; // keep fitting the table as it grows, until it's panned or zoomed by hand
  const lod = $derived(v.k < 0.5 ? "far" : v.k < 0.95 ? "mid" : "near");
  const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
  let frame = 0;
  const clampK = (k: number) => Math.min(2, Math.max(0.15, k));

  function zoomAt(k: number, cx: number, cy: number) {
    cancelAnimationFrame(frame);
    k = clampK(k);
    v.x = cx - ((cx - v.x) * k) / v.k;
    v.y = cy - ((cy - v.y) * k) / v.k;
    v.k = k;
    auto = false;
  }
  function animateTo(x: number, y: number, k: number, ms = 420) {
    cancelAnimationFrame(frame);
    if (reduced || !ms) return void Object.assign(v, { x, y, k });
    const from = { ...v }, t0 = performance.now();
    const step = (t: number) => {
      const p = Math.min(1, (t - t0) / ms), e = 1 - Math.pow(1 - p, 5);
      Object.assign(v, { x: from.x + (x - from.x) * e, y: from.y + (y - from.y) * e, k: from.k + (k - from.k) * e });
      if (p < 1) frame = requestAnimationFrame(step);
    };
    frame = requestAnimationFrame(step);
  }
  const TOP = 44; // the toolbar floats over the table's top edge: fit below it
  function fitRect(r: Rect | null, pad = 0.07, maxK = 1, ms?: number) {
    if (!r || !sw || !sh) return;
    const h = sh - TOP;
    const k = clampK(Math.min(maxK, (sw * (1 - pad * 2)) / r.w, (h * (1 - pad * 2)) / r.h));
    animateTo(sw / 2 - (r.x + r.w / 2) * k, TOP + h / 2 - (r.y + r.h / 2) * k, k, ms);
  }
  function fit(ms?: number) {
    auto = true;
    fitRect(bounds, 0.07, 1, ms);
  }
  // A project's column is tall and narrow: fit it with a slimmer margin, so its cards stay readable
  const fitProject = (key: string) => (front ? fitRect(rects[key], 0.045, 1.1, 360) : fitRect(rects[key], 0.12, 1.1));

  // Back where this table was looking in this view; else fitted (and kept fitted as it grows)
  const viewKey = $derived(view);
  $effect(() => {
    const saved = views.get(viewKey);
    untrack(() => {
      if (saved) Object.assign(v, { x: saved.x, y: saved.y, k: saved.k }), (auto = saved.auto);
      else auto = true;
    });
    return () => views.set(viewKey, { x: v.x, y: v.y, k: v.k, auto });
  });
  $effect(() => {
    void bounds?.w, bounds?.h, bounds?.x, bounds?.y, sw, sh;
    if (untrack(() => auto)) untrack(() => fitRect(bounds, 0.07, 1, 0));
  });

  // ---- focus: one project lit, the rest dimmed; one agent or server selected ----
  let focus = $state("");
  let selected = $state("");
  $effect(() => {
    if (focus && !keys.includes(focus)) (focus = ""), (selected = "");
  });

  const agentOf = (id: string) => rows.find((r) => r.id === id);
  const serviceOf = (id: string) => projects.flatMap((p) => p.services).find((s) => s.id === id);
  const projectOfItem = (el: HTMLElement) => el.closest<HTMLElement>("[data-project]")?.dataset.project ?? "";

  let drag: { x: number; y: number; vx: number; vy: number; moved: boolean } | null = null;
  const onItem = (e: Event) => !!(e.target as Element).closest("[data-agent], [data-service], [data-project-hit], .hud");
  function pointerdown(e: PointerEvent) {
    if (e.button !== 0 || onItem(e)) return;
    drag = { x: e.clientX, y: e.clientY, vx: v.x, vy: v.y, moved: false };
    stage?.setPointerCapture(e.pointerId);
  }
  function pointermove(e: PointerEvent) {
    if (drag) {
      const dx = e.clientX - drag.x, dy = e.clientY - drag.y;
      if (Math.abs(dx) + Math.abs(dy) > 3) drag.moved = true;
      if (drag.moved) cancelAnimationFrame(frame), (v.x = drag.vx + dx), (v.y = drag.vy + dy), (auto = false);
    }
    placeCard(e);
  }
  function pointerup() {
    if (drag && !drag.moved) (focus = ""), (selected = "");
    drag = null;
  }
  function wheel(e: WheelEvent) {
    e.preventDefault();
    const r = stage!.getBoundingClientRect();
    // Pinch arrives as ctrl+wheel; ⌘-scroll zooms too, plain scroll pans (as in Aurelius)
    if (e.ctrlKey || e.metaKey) zoomAt(v.k * Math.exp(-e.deltaY * (e.ctrlKey && !e.metaKey ? 0.012 : 0.004)), e.clientX - r.left, e.clientY - r.top);
    else cancelAnimationFrame(frame), (v.x -= e.deltaX), (v.y -= e.deltaY), (auto = false);
  }
  function click(e: MouseEvent) {
    const item = (e.target as Element).closest<HTMLElement>("[data-agent], [data-service]");
    if (item) {
      const key = projectOfItem(item);
      selected = item.dataset.agent || item.dataset.service || "";
      focus = key;
      if (v.k < 0.95) fitProject(key);
      return;
    }
    const hit = (e.target as Element).closest<HTMLElement>("[data-project-hit]");
    if (hit) (focus = hit.dataset.projectHit!), (selected = ""), fitProject(focus);
  }
  function dblclick(e: MouseEvent) {
    const item = (e.target as Element).closest<HTMLElement>("[data-agent], [data-service]");
    if (item?.dataset.agent) return openAgent(item.dataset.agent);
    if (item?.dataset.service) return openService(item.dataset.service);
    if (!onItem(e)) {
      const r = stage!.getBoundingClientRect();
      zoomAt(v.k * 1.6, e.clientX - r.left, e.clientY - r.top);
    }
  }
  function keydown(e: KeyboardEvent) {
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    const item = (e.target as Element).closest<HTMLElement>("[data-agent], [data-service]");
    if (item && e.key === "Enter") return item.dataset.agent ? openAgent(item.dataset.agent) : openService(item.dataset.service!);
    if (item && e.key === " ") return e.preventDefault(), item.click();
    const act =
      e.key === "Escape" ? () => ((focus = ""), (selected = "")) :
      e.key === "=" || e.key === "+" ? () => zoomAt(v.k * 1.25, sw / 2, sh / 2) :
      e.key === "-" ? () => zoomAt(v.k / 1.25, sw / 2, sh / 2) :
      e.key === "0" || e.key === "f" ? () => fit() :
      null;
    if (act) e.preventDefault(), act();
  }

  // Double-click (or Enter): an agent's terminal; a server's page, else the terminal it runs in
  function openAgent(id: string) {
    const r = agentOf(id);
    if (r?.ws) showTab(r.ws, r.id);
  }
  const workspaceOf = (tabId: string) => ws.all.find((m) => tabsIn(m.id).some((t) => t.id === tabId))?.id;
  function openService(id: string) {
    const s = serviceOf(id);
    const port = s && (s.listener ? webPort(s.listener) : s.container ? webContainerPort(s.container) : undefined);
    const w = s?.tab ? workspaceOf(s.tab) : undefined;
    if (port) {
      // In the workspace of the server's project (its terminal's, else the one showing its folder), unless another
      // window has that one
      const key = projects.find((p) => p.services.some((x) => x.id === id))?.key;
      const home = w ?? ws.all.find((m) => key && keyOf(folderOf(m.id)) === key)?.id;
      if (home && !ownerWindow(home)) activate(home);
      openBrowser(`http://localhost:${port}`);
    } else if (s?.tab && w) showTab(w, s.tab);
  }
  function serviceAction(s: Service) {
    const port = s.listener ? webPort(s.listener) : s.container ? webContainerPort(s.container) : undefined;
    return port ? `Double-click to open localhost:${port} in a browser tab` : s.tab && workspaceOf(s.tab) ? "Double-click to go to its terminal" : "";
  }

  // ---- hover card: the same facts as Fleet's row ----
  let hovered = $state<{ kind: "agent" | "service"; id: string } | null>(null);
  let card = $state<HTMLElement>();
  let cardAt = $state({ x: 0, y: 0 });
  function pointerover(e: PointerEvent) {
    const el = (e.target as Element).closest<HTMLElement>("[data-agent], [data-service]");
    hovered = !el || drag?.moved ? null : el.dataset.agent ? { kind: "agent", id: el.dataset.agent } : { kind: "service", id: el.dataset.service! };
    placeCard(e);
  }
  function placeCard(e: PointerEvent) {
    if (!hovered || !stage) return;
    const s = stage.getBoundingClientRect();
    const cw = card?.offsetWidth ?? 280, ch = card?.offsetHeight ?? 160;
    let x = e.clientX - s.left + 16, y = e.clientY - s.top + 18;
    if (x + cw > s.width - 8) x = e.clientX - s.left - cw - 12;
    if (y + ch > s.height - 8) y = e.clientY - s.top - ch - 12;
    cardAt = { x: Math.max(8, x), y: Math.max(8, y) };
  }
  const hoverAgent = $derived(hovered?.kind === "agent" ? agentOf(hovered.id) : undefined);
  const hoverService = $derived(hovered?.kind === "service" ? serviceOf(hovered.id) : undefined);

  const title = (r: Row) => r.label || (agentProvider[r.agent] ? providerName[agentProvider[r.agent]] : r.agent);
  const heading: Record<Status, string> = { attention: "Needs you", busy: "Working", idle: "Idle", asleep: "Not started" };
  const askText = (r: Row) => (r.asking === "approval" ? "Waiting for your approval" : r.asking === "question" ? "Asking you a question" : r.status === "attention" ? "Your turn" : "");
  const base = (p: string) => p.slice(p.lastIndexOf("/") + 1);

  // ---- minimap ----
  const MW = 168, MH = 104;
  const mini = $derived.by(() => {
    if (!bounds || !sw) return null;
    const vr = { x: -v.x / v.k, y: -v.y / v.k, w: sw / v.k, h: sh / v.k };
    const b = union([bounds, vr])!;
    const s = Math.min(MW / b.w, MH / b.h), ox = (MW - b.w * s) / 2, oy = (MH - b.h * s) / 2;
    const at = (r: Rect) => ({ x: ox + (r.x - b.x) * s, y: oy + (r.y - b.y) * s, w: Math.max(2, r.w * s), h: Math.max(2, r.h * s) });
    return { b, s, ox, oy, view: at(vr), projects: projects.map((p) => ({ key: p.key, hot: p.agents.some((a) => a.status === "attention"), ...at(rects[p.key]) })) };
  });
  function miniJump(e: MouseEvent) {
    if (!mini) return;
    const m = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const wx = mini.b.x + (e.clientX - m.left - 5 - mini.ox) / mini.s, wy = mini.b.y + (e.clientY - m.top - 5 - mini.oy) / mini.s;
    auto = false;
    animateTo(sw / 2 - wx * v.k, sh / 2 - wy * v.k, v.k, 300);
  }

  const count = $derived(tally(rows));
  const up = $derived(projects.reduce((n, p) => n + p.services.filter((s) => s.up).length, 0));
  const servers = $derived(projects.reduce((n, p) => n + p.services.length, 0));
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<section
  class="war slab"
  data-lod={lod}
  data-view={view}
  class:focusing={!!focus}
  bind:this={stage}
  style:--k={v.k}
  style:--ik={1 / v.k}
  style:--tx="{v.x}px"
  style:--ty="{v.y}px"
  tabindex="0"
  aria-label="War Table"
  onpointerdown={pointerdown}
  onpointermove={pointermove}
  onpointerup={pointerup}
  onpointerover={pointerover}
  onpointerleave={() => (hovered = null)}
  onwheel={wheel}
  onclick={click}
  ondblclick={dblclick}
  onkeydown={keydown}
>
  <div class="bg"></div>
  {#if projects.length}
    {#if front}
      <WarFront {projects} layout={front} {info} {v} {focus} {selected} />
    {:else}
      <WarCastra {projects} {camps} {info} {v} {focus} {selected} />
    {/if}
  {:else if shells.polled}
    <div class="empty">
      <b>Nothing on the table yet</b>
      <p>Start <code>claude</code>, <code>codex</code> or another agent in a terminal, or run a dev server, and its project shows up here.</p>
    </div>
  {/if}

  <div class="hud bar">
    <div class="seg" role="group" aria-label="Layout">
      <button type="button" aria-pressed={view === "castra"} onclick={() => setWarView("castra")} title="A walled camp per project: agents as coins, servers on its wall">Castra</button>
      <button type="button" aria-pressed={view === "front"} onclick={() => setWarView("front")} title="Bands by urgency across every project: what needs you is on top">Front line</button>
    </div>
    <p class="sum">
      {#each STATUSES.filter((s) => count[s]) as s (s)}<span class="tally" class:hot={s === "attention"}><span class="glyph {s}"></span>{count[s]} {heading[s].toLowerCase()}</span>{/each}
      {#if servers}<span class="tally"><span class="glyph up"></span>{up} of {servers} servers up</span>{/if}
    </p>
  </div>
  <div class="hud zoom" aria-label="Zoom">
    <button type="button" onclick={() => zoomAt(v.k / 1.25, sw / 2, sh / 2)} aria-label="Zoom out" title="Zoom out (−)">−</button>
    <span class="pct">{Math.round(v.k * 100)}%</span>
    <button type="button" onclick={() => zoomAt(v.k * 1.25, sw / 2, sh / 2)} aria-label="Zoom in" title="Zoom in (+)">+</button>
    <button type="button" class="fit" onclick={() => ((focus = ""), (selected = ""), fit())} title="Fit everything (0)">Fit</button>
  </div>
  {#if mini && projects.length > 1}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="hud minimap" onclick={miniJump} title="Click to look there">
      <svg width={MW} height={MH} viewBox="0 0 {MW} {MH}">
        {#each mini.projects as p (p.key)}<rect x={p.x} y={p.y} width={p.w} height={p.h} rx="2" class:hot={p.hot} class:dim={focus && focus !== p.key} />{/each}
        <rect class="view" x={mini.view.x} y={mini.view.y} width={mini.view.w} height={mini.view.h} rx="3" />
      </svg>
    </div>
  {/if}

  {#if hoverAgent || hoverService}
    <div class="card glass-float" bind:this={card} style:transform="translate({cardAt.x}px, {cardAt.y}px)">
      {#if hoverAgent}
        {@const r = hoverAgent}
        {@const p = agentProvider[r.agent]}
        {@const repo = app.repos[r.cwd]}
        <div class="head">
          <span class="badge" style:color={p ? `var(--usage-${p})` : undefined}>{#if p}<ProviderIcon provider={p} size={14} />{/if}</span>
          <div><b>{title(r)}</b>{#if r.label && p}<small>{providerName[p]}</small>{/if}</div>
        </div>
        <div class="state" class:hot={r.status === "attention"}>
          <span class="glyph {r.status}"></span>
          {#if r.asking === "approval"}<ShieldQuestion size={12} strokeWidth={2} />{:else if r.asking === "question"}<MessageCircleQuestion size={12} strokeWidth={2} />{/if}
          {r.status === "attention" ? askText(r) : heading[r.status]}{#if r.since !== null}<span class="faint">for {fmtSpan(r.since)}</span>{/if}
        </div>
        <dl>
          <dt>Workspace</dt><dd>{r.ws ? nameOf(r.ws) : "Detached: no workspace has its tab"}</dd>
          <dt>Folder</dt><dd>{base(repo?.root || r.cwd)}</dd>
          {#if repo?.branch}<dt>Branch</dt><dd class="br"><GitBranch size={11} strokeWidth={1.5} />{repo.branch}</dd>{/if}
        </dl>
        {#if r.ws}<p class="hint">Double-click to open its terminal</p>{/if}
      {:else if hoverService}
        {@const s = hoverService}
        {@const by = s.agent ? agentOf(s.agent) : undefined}
        <div class="head">
          <span class="badge srv" data-type={s.type}></span>
          <div><b>{s.name}</b><small>{s.type === "container" ? "Docker container" : `Process · pid ${s.listener?.pid}`}</small></div>
        </div>
        <div class="state"><span class="glyph {s.up ? 'up' : 'down'}"></span>{s.up ? "Running" : s.container?.status || "Stopped"}</div>
        <dl>
          <dt>Ports</dt><dd class="mono">{s.ports.length ? s.ports.map((p) => `:${p}`).join(" ") : "none"}</dd>
          <dt>Runs</dt><dd class="mono">{s.detail}</dd>
          <dt>Started</dt><dd>{by ? `by ${title(by)}` : s.tab ? "in a Pretorium terminal" : s.container ? (s.container.project ? `by Compose (${s.container.project})` : "by Docker") : "outside Pretorium"}</dd>
        </dl>
        {#if serviceAction(s)}<p class="hint">{serviceAction(s)}</p>{/if}
      {/if}
    </div>
  {/if}
</section>

<style>
  /* The table: a pannable, zoomable world on a tile grid. Ember marks what needs you (as in Fleet), gold what works. */
  .war {
    --ember: oklch(0.68 0.18 52);
    --ember-ink: oklch(0.52 0.16 48);
    --ember-wash: oklch(0.7 0.17 50 / 9%);
    --ember-edge: oklch(0.7 0.17 50 / 42%);
    --ok: var(--diff-add-gutter);
    --ease: var(--ease-out-quint);
    /* Plane 2: a slab standing in for the panes, as Usage does */
    grid-column: 2;
    grid-row: 2;
    position: relative;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    background: var(--surface);
    touch-action: none;
    user-select: none;
    cursor: grab;
    outline: none;
    font-size: 13px;
    container-type: size;
  }
  .war :global(*) {
    box-sizing: border-box;
  }
  :global(.dark) .war {
    --ember: oklch(0.78 0.16 58);
    --ember-ink: oklch(0.82 0.13 60);
    --ember-wash: oklch(0.75 0.16 55 / 11%);
    --ember-edge: oklch(0.75 0.16 55 / 40%);
  }
  .war:active {
    cursor: grabbing;
  }
  .bg {
    position: absolute;
    inset: 0;
    pointer-events: none;
    --minor: color-mix(in oklch, var(--foreground) 4.5%, transparent);
    --major: color-mix(in oklch, var(--foreground) 8.5%, transparent);
    --t: 32px;
    background-image:
      linear-gradient(to right, var(--major) 1px, transparent 1px), linear-gradient(to bottom, var(--major) 1px, transparent 1px),
      linear-gradient(to right, var(--minor) 1px, transparent 1px), linear-gradient(to bottom, var(--minor) 1px, transparent 1px);
    background-size:
      calc(var(--t) * 4 * var(--k)) calc(var(--t) * 4 * var(--k)), calc(var(--t) * 4 * var(--k)) calc(var(--t) * 4 * var(--k)),
      calc(var(--t) * var(--k)) calc(var(--t) * var(--k)), calc(var(--t) * var(--k)) calc(var(--t) * var(--k));
    background-position: var(--tx) var(--ty);
  }
  .war[data-view="front"] .bg {
    --t: 40px;
  }
  .war[data-lod="far"] .bg {
    --minor: transparent;
  }
  .empty {
    position: absolute;
    inset: 0;
    display: grid;
    place-content: center;
    gap: 6px;
    padding: 24px;
    text-align: center;
    color: var(--muted-foreground);
  }
  .empty b {
    color: var(--foreground);
    font: 600 18px/1.3 var(--font-cinzel);
    letter-spacing: 0.03em;
  }
  .empty p {
    margin: 0;
    max-width: 46ch;
  }
  .empty code {
    font-size: 12px;
  }

  /* Status glyphs: Fleet's three channels, so tallies read the same everywhere */
  .glyph {
    flex: none;
    display: inline-block;
    width: 7px;
    height: 7px;
    box-sizing: border-box;
    border-radius: 9999px;
  }
  .glyph.attention {
    background: var(--ember);
    box-shadow: 0 0 0 2px color-mix(in oklch, var(--ember) 22%, transparent);
  }
  .glyph.busy {
    border: 1.5px solid var(--gold);
    background: color-mix(in oklch, var(--gold) 30%, transparent);
  }
  .glyph.idle {
    border: 1.5px solid var(--faint);
  }
  .glyph.asleep {
    border: 1.5px dashed var(--faint);
  }
  .glyph.up {
    background: var(--ok);
  }
  .glyph.down {
    border: 1.5px solid var(--muted-foreground);
  }

  /* HUD: floating glass over the table */
  .hud {
    position: absolute;
    z-index: 3;
    cursor: default;
  }
  .bar {
    left: 10px;
    top: 10px;
    right: 10px;
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px 14px;
    pointer-events: none;
  }
  .bar > * {
    pointer-events: auto;
  }
  .seg {
    display: flex;
    gap: 2px;
    padding: 3px;
    border-radius: 12px;
    background: var(--float);
    box-shadow: var(--sh-chrome);
    backdrop-filter: var(--blur-chrome);
  }
  .seg button {
    border: 0;
    background: none;
    padding: 4px 11px;
    border-radius: 9px;
    color: var(--muted-foreground);
    font: 500 12.5px var(--font-sans);
    cursor: pointer;
    transition: background var(--dur-quick) var(--ease), color var(--dur-quick) var(--ease);
  }
  .seg button:hover {
    color: var(--foreground);
    background: var(--hover);
  }
  .seg button[aria-pressed="true"] {
    color: var(--foreground);
    background: var(--chip-on);
    box-shadow: var(--sh-chip);
  }
  .sum {
    display: flex;
    flex-wrap: wrap;
    gap: 3px 12px;
    margin: 0;
    padding: 5px 11px;
    border-radius: 12px;
    background: var(--float);
    box-shadow: var(--sh-chrome);
    backdrop-filter: var(--blur-chrome);
    color: var(--muted-foreground);
    font-size: 12px;
  }
  .sum:empty {
    display: none;
  }
  .tally {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-variant-numeric: tabular-nums;
  }
  .tally.hot {
    color: var(--ember-ink);
    font-weight: 500;
  }
  .zoom {
    left: 10px;
    bottom: 10px;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 3px;
    border-radius: 12px;
    background: var(--float);
    box-shadow: var(--sh-chrome);
    backdrop-filter: var(--blur-chrome);
  }
  .zoom button {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border: 0;
    border-radius: 9px;
    background: none;
    color: var(--muted-foreground);
    font-size: 15px;
    line-height: 1;
    cursor: pointer;
  }
  .zoom button:hover {
    background: var(--hover);
    color: var(--foreground);
  }
  .zoom .fit {
    width: auto;
    padding: 0 9px;
    font-size: 12px;
  }
  .pct {
    min-width: 42px;
    text-align: center;
    font: 500 11px var(--font-mono);
    color: var(--muted-foreground);
    font-variant-numeric: tabular-nums;
  }
  .minimap {
    right: 10px;
    bottom: 10px;
    padding: 5px;
    border-radius: 12px;
    background: var(--float);
    box-shadow: var(--sh-chrome);
    backdrop-filter: var(--blur-chrome);
    line-height: 0;
    cursor: pointer;
  }
  .minimap rect {
    fill: color-mix(in oklch, var(--foreground) 16%, transparent);
  }
  .minimap rect.hot {
    fill: color-mix(in oklch, var(--ember) 55%, transparent);
  }
  .minimap rect.dim {
    opacity: 0.35;
  }
  .minimap rect.view {
    fill: color-mix(in oklch, var(--gold) 10%, transparent);
    stroke: var(--gold);
    stroke-width: 1;
  }
  @container (max-width: 520px) {
    .minimap {
      display: none;
    }
  }

  /* Hover card */
  .card {
    position: absolute;
    left: 0;
    top: 0;
    z-index: 4;
    width: 280px;
    padding: 12px;
    border-radius: 14px;
    pointer-events: none;
    font-size: 12.5px;
    color: var(--foreground);
  }
  .head {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .head b {
    display: block;
    font-weight: 600;
    font-size: 13.5px;
    line-height: 1.25;
  }
  .head small {
    color: var(--muted-foreground);
    font-size: 11.5px;
  }
  .badge {
    flex: none;
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: 9999px;
    background: color-mix(in oklch, currentColor 12%, var(--surface));
    box-shadow: inset 0 0 0 1px color-mix(in oklch, currentColor 35%, transparent);
  }
  .badge.srv {
    border-radius: 7px;
    color: var(--muted-foreground);
    position: relative;
  }
  .badge.srv::before {
    content: "";
    position: absolute;
    inset: 8px 7px;
    border-radius: 2px;
    border: 1.5px solid currentColor;
  }
  .badge.srv[data-type="container"]::before {
    inset: 7px;
    border-radius: 3px;
    box-shadow: inset 0 -4px 0 -2.5px currentColor;
  }
  .state {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 10px 0 8px;
    font-weight: 500;
  }
  .state.hot {
    color: var(--ember-ink);
  }
  .faint {
    color: var(--faint);
    font-weight: 400;
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 3px 12px;
    margin: 0;
  }
  dt {
    color: var(--faint);
  }
  dd {
    margin: 0;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  dd.br {
    display: flex;
    align-items: center;
    gap: 4px;
    font-family: var(--font-mono);
    font-size: 11.5px;
  }
  dd.mono {
    font: 11.5px var(--font-mono);
  }
  .hint {
    margin: 9px 0 0;
    padding-top: 8px;
    border-top: 1px solid var(--border);
    color: var(--faint);
    font-size: 11.5px;
  }
</style>
