<script lang="ts">
  import { GitBranch, MessageCircleQuestion, ShieldQuestion } from "@lucide/svelte";
  import ProviderIcon from "./ProviderIcon.svelte";
  import { agentProvider } from "./agents.svelte";
  import { fmtSpan, STATUSES, type Row, type Status } from "./fleet";
  import { app } from "./state.svelte";
  import { nameOf } from "./workspaces.svelte";
  import { providerName } from "./usageStats";
  import { BANDS, F, fmtNum, langColor, langShares, tally, type Band, type Front, type Project, type Stats, type Tokens } from "./war";

  // Front line: three bands across the table (needs you, working, idle), a column per project through them, and its
  // servers in a supply strip at the column's foot. A card slides to its new band when its agent changes state.
  let { projects, layout, info, v, focus, selected }: {
    projects: Project[];
    layout: Front;
    info: { stats: Record<string, Stats>; tokens: Map<string, Tokens>; branch: (key: string) => string };
    v: { x: number; y: number; k: number };
    focus: string;
    selected: string;
  } = $props();

  const byKey = $derived(new Map(projects.map((p) => [p.key, p])));
  const words: Record<Status, string> = { attention: "needs you", busy: "working", idle: "idle", asleep: "not started" };
  const bandName: Record<Band, string> = { front: "Needs you", work: "Working", rear: "Idle or not started", supply: "Servers" };
  const bandGlyph: Record<Band, string> = { front: "attention", work: "busy", rear: "idle", supply: "up" };
  const title = (r: Row) => r.label || (agentProvider[r.agent] ? providerName[agentProvider[r.agent]] : r.agent);
  const base = (p: string) => p.slice(p.lastIndexOf("/") + 1);

  const agents = $derived(projects.flatMap((p) => p.agents.map((a) => ({ a, p }))));
  const services = $derived(projects.flatMap((p) => p.services.map((s) => ({ s, p }))));
  const counts = $derived.by((): Record<Band, string> => {
    const t = tally(projects.flatMap((p) => p.agents));
    const all = services.length, up = services.filter((x) => x.s.up).length;
    return { front: String(t.attention), work: String(t.busy), rear: String(t.idle + t.asleep), supply: all ? `${up} of ${all} up` : "none" };
  });

  // Hovering an agent or a server lights its links
  let hot = $state("");
  function over(e: PointerEvent) {
    const el = (e.target as Element).closest<HTMLElement>("[data-agent], [data-service]");
    hot = el?.dataset.agent ?? el?.dataset.service ?? "";
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="world" style:transform="translate({v.x}px, {v.y}px) scale({v.k})" onpointerover={over} onpointerleave={() => (hot = "")}>
  {#each BANDS as b (b)}
    <div class="band" data-band={b} style:transform="translateY({layout.bands[b].y}px)" style:height="{layout.bands[b].h}px" style:width="{layout.w + 8400}px"></div>
  {/each}
  {#each layout.lanes as l (l.key)}
    {@const p = byKey.get(l.key)!}
    {@const s = info.stats[l.key]}
    {@const tok = info.tokens.get(l.key)}
    {@const t = tally(p.agents)}
    {@const branch = info.branch(l.key)}
    {@const shares = s ? langShares(s) : []}
    <div class="lane" data-project={l.key} class:dim={focus && focus !== l.key} style:transform="translateX({l.x}px)" style:height="{layout.h}px"></div>
    <div class="head" data-project={l.key} data-project-hit={l.key} class:dim={focus && focus !== l.key} class:lit={focus === l.key} style:transform="translateX({l.x}px)" title={l.key}>
      <div class="hin">
        <div class="name">{p.name}</div>
        <div class="branch">{#if branch}<GitBranch size={11} strokeWidth={1.5} /><span>{branch}</span>{/if}</div>
        <div class="tally">
          {#each STATUSES.filter((x) => t[x]) as x (x)}<span class="t" class:hot={x === "attention"}><i class="glyph {x}"></i>{t[x]} {words[x]}</span>{:else}<span class="t">No agents</span>{/each}
        </div>
        <dl class="stats">
          <div><dt>lines</dt><dd>{s ? fmtNum(s.lines) : "…"}</dd></div>
          <div><dt>files</dt><dd>{s ? fmtNum(s.files) : "…"}</dd></div>
          <div><dt>tokens 30d</dt><dd>{tok ? fmtNum(tok.month) : "…"}<small>{tok ? (tok.today ? fmtNum(tok.today) + " today" : "none today") : ""}</small></dd></div>
          <div><dt>commits 7d</dt><dd>{s ? s.commits7d : "…"}</dd></div>
        </dl>
        <div class="strip">{#each shares as x (x.name)}<i style:flex={x.share} style:background={langColor(x.name)} title="{x.name} {Math.round(x.share * 100)}%"></i>{/each}</div>
        <div class="legend">{#each shares.slice(0, 3) as x (x.name)}<span><i style:background={langColor(x.name)}></i>{x.name} <b>{Math.round(x.share * 100)}%</b></span>{/each}{#if shares.length > 3}<span class="more">+{shares.length - 3}</span>{/if}</div>
      </div>
      <div class="far"><b>{p.name}</b></div>
    </div>
    {#if l.calm}
      <div class="calm" data-project={l.key} class:dim={focus && focus !== l.key} style:transform="translate({l.x + F.PADX}px, {layout.bands.front.y + F.PT}px)">Nothing needs you here</div>
    {/if}
  {/each}

  <svg class="links" width={layout.w} height={layout.h} aria-hidden="true">
    {#each layout.links as k (k.service)}
      {@const srv = services.find((x) => x.s.id === k.service)?.s}
      <g class:dim={focus && focus !== k.key} class:hot={hot === k.agent || hot === k.service}>
        <path class="ln" class:down={!srv?.up} d={k.d} />
        <circle class="nd" cx={k.dot[0]} cy={k.dot[1]} r="2.6" />
        <circle class="nd" cx={k.end[0]} cy={k.end[1]} r="2.6" />
      </g>
    {/each}
  </svg>

  {#each agents as { a, p } (a.id)}
    {@const c = layout.cards[a.id]}
    {@const prov = agentProvider[a.agent]}
    {@const repo = app.repos[a.cwd]}
    {#if c}
      <div class="card" data-agent={a.id} data-project={p.key} data-status={a.status} class:dim={focus && focus !== p.key} class:sel={selected === a.id} tabindex="0" role="button" aria-label="{title(a)}: {words[a.status]}" style:transform="translate({c.x}px, {c.y}px)" style:height="{c.h}px">
        <div class="row">
          <span class="m" style:color={prov ? `var(--usage-${prov})` : undefined}>{#if prov}<ProviderIcon provider={prov} size={17} />{/if}<i class="arc"></i></span>
          <div class="ti"><b>{title(a)}</b><span class="st">{a.since === null ? "Not started" : `${words[a.status][0].toUpperCase()}${words[a.status].slice(1)} · ${fmtSpan(a.since)}`}</span></div>
        </div>
        {#if a.status === "attention"}
          <div class="ask">
            {#if a.asking === "approval"}<ShieldQuestion size={16} strokeWidth={2} />Waiting for your approval{:else if a.asking === "question"}<MessageCircleQuestion size={16} strokeWidth={2} />Asking you a question{:else}Your turn{/if}
          </div>
        {/if}
        <div class="meta">
          <span class="where"><span>{a.ws ? nameOf(a.ws) : "Detached"}{base(repo?.root || a.cwd) !== p.name ? ` / ${base(repo?.root || a.cwd)}` : ""}</span>{#if repo?.branch}<span class="br"><GitBranch size={10} strokeWidth={1.5} /><span>{repo.branch}</span></span>{/if}</span>
          {#if prov && a.label}<span class="mt">{providerName[prov]}</span>{/if}
        </div>
        <i class="sweep"></i>
      </div>
    {/if}
  {/each}

  {#each services as { s, p } (s.id)}
    {@const c = layout.chips[s.id]}
    {#if c}
      <div class="chip" data-service={s.id} data-project={p.key} class:down={!s.up} class:dim={focus && focus !== p.key} class:sel={selected === s.id} tabindex="0" role="button" aria-label="{s.name}: {s.up ? 'running' : 'stopped'}" style:transform="translate({c.x}px, {c.y}px)">
        <i class="dot"></i><span class="cn">{s.name}</span><span class="kind">{s.up ? (s.type === "container" ? "docker" : "process") : "stopped"}</span><span class="port">{s.ports.length ? s.ports.map((x) => ":" + x).join(" ") : "no port"}</span>
      </div>
    {/if}
  {/each}
</div>

<!-- Band labels: in the gutter left of the first column, held at the pane's edge once it's panned past -->
{#each BANDS as b (b)}
  <div class="blabel" data-band={b} style:left="max(10px, {v.x - 200}px)" style:top="{v.y + (layout.bands[b].y + 9) * v.k}px">
    <span class="pill"><i class="glyph {bandGlyph[b]}"></i>{bandName[b]}<b>{counts[b]}</b></span>
  </div>
{/each}

<style>
  .world {
    position: absolute;
    left: 0;
    top: 0;
    transform-origin: 0 0;
    will-change: transform;
    --mv: 640ms var(--ease);
  }
  .world > * {
    position: absolute;
    left: 0;
    top: 0;
  }
  .dim {
    opacity: 0.22;
    filter: saturate(0.4);
  }

  /* Bands */
  .band {
    left: -4200px !important;
    pointer-events: none;
    transition: transform var(--mv), height var(--mv);
  }
  .band[data-band="front"] {
    background: linear-gradient(to bottom, color-mix(in oklch, var(--ember) 10%, transparent), var(--ember-wash) 70%);
    box-shadow: inset 0 1px 0 var(--ember-edge), inset 0 -1px 0 color-mix(in oklch, var(--ember) 22%, transparent);
  }
  .band[data-band="work"] {
    box-shadow: inset 0 -1px 0 var(--border);
  }
  .band[data-band="rear"] {
    background: color-mix(in oklch, var(--foreground) 2.5%, transparent);
    box-shadow: inset 0 -1px 0 var(--border);
  }
  .band[data-band="supply"] {
    background: repeating-linear-gradient(135deg, color-mix(in oklch, var(--foreground) 4%, transparent) 0 1px, transparent 1px 9px), color-mix(in oklch, var(--foreground) 3%, transparent);
  }
  .blabel {
    position: absolute;
    z-index: 2;
    width: 188px;
    display: flex;
    justify-content: flex-end;
    pointer-events: none;
  }
  .pill {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    padding: 5px 11px 5px 9px;
    border-radius: 9999px;
    white-space: nowrap;
    background: var(--float);
    box-shadow: var(--sh-chrome);
    backdrop-filter: var(--blur-chrome);
    color: var(--muted-foreground);
    font: 500 12px/1.2 var(--font-sans);
  }
  .pill b {
    color: var(--faint);
    font: 500 11px var(--font-mono);
    font-variant-numeric: tabular-nums;
  }
  .blabel[data-band="front"] .pill {
    color: var(--ember-ink);
    font-weight: 600;
    box-shadow: var(--sh-chrome), inset 0 0 0 1px var(--ember-edge);
  }
  .blabel[data-band="front"] .pill b {
    color: var(--ember-ink);
  }
  .blabel[data-band="work"] .pill {
    color: var(--foreground);
  }

  /* Glyphs (as in Fleet) */
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

  /* Columns and their heads */
  .lane {
    width: 288px;
    border-radius: 22px;
    background: color-mix(in oklch, var(--surface) 58%, transparent);
    box-shadow: inset 0 0 0 1px var(--border), 0 1px 2px oklch(var(--shadow-tint) / 0.06);
    transition: transform var(--mv), height var(--mv), opacity 220ms var(--ease), filter 220ms var(--ease);
  }
  .head {
    width: 288px;
    height: 196px;
    border-radius: 22px 22px 10px 10px;
    background: var(--slab-head);
    box-shadow: var(--sh-slab);
    cursor: pointer;
    transition: transform var(--mv), opacity 220ms var(--ease), filter 220ms var(--ease), box-shadow 180ms var(--ease);
  }
  .head:hover {
    box-shadow: var(--sh-slab), inset 0 0 0 1px var(--border-strong);
  }
  .head.lit {
    box-shadow: var(--sh-slab), 0 0 0 1.5px var(--gold-ring), 0 0 0 5px var(--gold-soft);
  }
  .hin {
    display: flex;
    flex-direction: column;
    height: 100%;
    padding: 15px 18px 13px;
    overflow: hidden;
    border-radius: inherit;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--foreground);
    font: 600 19px/1.15 var(--font-cinzel);
    letter-spacing: 0.02em;
  }
  .branch {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    min-height: 16px;
    margin-top: 3px;
    color: var(--muted-foreground);
    font: 400 12px/1.3 var(--font-mono);
  }
  .branch span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .branch :global(svg) {
    flex: none;
  }
  .tally {
    display: flex;
    flex-wrap: wrap;
    gap: 2px 11px;
    margin-top: 9px;
    color: var(--muted-foreground);
    font-size: 12.5px;
  }
  .t {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .t.hot {
    color: var(--ember-ink);
    font-weight: 600;
  }
  .stats {
    display: grid;
    grid-template-columns: repeat(4, auto);
    justify-content: space-between;
    gap: 0 10px;
    margin: 11px 0 0;
    padding-top: 9px;
    border-top: 1px solid var(--border);
  }
  .stats div {
    display: flex;
    flex-direction: column-reverse;
    justify-content: flex-end;
    min-width: 0;
  }
  .stats dt {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--faint);
    font-size: 11px;
    line-height: 1.25;
  }
  .stats dd {
    margin: 0;
    white-space: nowrap;
    color: var(--foreground);
    font: 500 15px/1.25 var(--font-mono);
    font-variant-numeric: tabular-nums;
  }
  .stats dd small {
    display: block;
    color: var(--muted-foreground);
    font: 400 10.5px/1.3 var(--font-mono);
  }
  .strip {
    flex: none;
    display: flex;
    gap: 2px;
    height: 6px;
    margin-top: auto;
    border-radius: 3px;
    overflow: hidden;
    background: color-mix(in oklch, var(--foreground) 5%, transparent);
  }
  .strip i {
    display: block;
    min-width: 2px;
  }
  .legend {
    display: flex;
    gap: 4px 10px;
    margin-top: 7px;
    overflow: hidden;
    white-space: nowrap;
    color: var(--muted-foreground);
    font-size: 11px;
  }
  .legend span {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
  .legend i {
    width: 7px;
    height: 7px;
    border-radius: 2px;
  }
  .legend b {
    color: var(--faint);
    font: 400 10.5px var(--font-mono);
  }
  .more {
    color: var(--faint);
  }

  /* Mid: fewer, larger words, so the table reads at 50-90% */
  :global(.war[data-lod="mid"]) .name {
    font-size: 24px;
  }
  :global(.war[data-lod="mid"]) .branch {
    margin-top: 4px;
    font-size: 13.5px;
  }
  :global(.war[data-lod="mid"]) .tally {
    margin-top: 10px;
    font-size: 14.5px;
  }
  :global(.war[data-lod="mid"]) .stats dd {
    font-size: 17px;
  }
  :global(.war[data-lod="mid"]) .stats dt {
    font-size: 12.5px;
  }
  :global(.war[data-lod="mid"]) .stats dd small,
  :global(.war[data-lod="mid"]) .legend {
    display: none;
  }
  :global(.war[data-lod="mid"]) .strip {
    height: 8px;
    margin-bottom: 3px;
    border-radius: 4px;
  }
  :global(.war[data-lod="mid"]) .ti b {
    font-size: 17px;
  }
  :global(.war[data-lod="mid"]) .st {
    font-size: 13.5px;
  }
  :global(.war[data-lod="mid"]) .card[data-status="attention"] .ask {
    font-size: 20px;
    line-height: 1.12;
  }
  :global(.war[data-lod="mid"]) .cn {
    font-size: 14px;
  }
  :global(.war[data-lod="mid"]) .port {
    font-size: 13px;
  }
  :global(.war[data-lod="mid"]) .calm {
    font-size: 15px;
  }

  /* Far: the column's name at a constant size on screen, the language strip thickens */
  .far {
    display: none;
  }
  :global(.war[data-lod="far"]) .hin > :not(.strip) {
    visibility: hidden;
  }
  :global(.war[data-lod="far"]) .strip {
    height: calc(5px * var(--ik));
    margin-bottom: -6px;
    gap: calc(1px * var(--ik));
    border-radius: calc(3px * var(--ik));
  }
  :global(.war[data-lod="far"]) .far {
    position: absolute;
    left: 50%;
    top: 14px;
    display: flex;
    justify-content: center;
    width: calc(264px * var(--k));
    transform-origin: 50% 0;
    transform: translateX(-50%) scale(var(--ik));
    text-align: center;
    pointer-events: none;
  }
  .far b {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--foreground);
    font: 600 13.5px/1.2 var(--font-sans);
  }

  /* Links from an agent to the servers it started */
  .links {
    overflow: visible;
    pointer-events: none;
  }
  .ln {
    fill: none;
    stroke: color-mix(in oklch, var(--foreground) 26%, transparent);
    stroke-width: 1.25;
    vector-effect: non-scaling-stroke;
    transition: stroke 160ms var(--ease);
  }
  .ln.down {
    stroke-dasharray: 3 4;
  }
  .nd {
    fill: color-mix(in oklch, var(--foreground) 34%, var(--surface));
  }
  .hot .ln {
    stroke: var(--foreground);
    stroke-width: 1.75;
  }
  .hot .nd {
    fill: var(--foreground);
  }
  :global(.war[data-lod="far"]) .nd {
    display: none;
  }

  /* "Nothing waiting" in the front band */
  .calm {
    display: grid;
    place-items: center;
    width: 244px;
    height: 64px;
    border-radius: 16px;
    border: 1px dashed color-mix(in oklch, var(--ember) 30%, transparent);
    color: var(--faint);
    font-size: 13px;
    transition: transform var(--mv), opacity 220ms var(--ease);
  }
  :global(.war[data-lod="far"]) .calm {
    color: transparent;
    border-width: calc(1px * var(--ik));
  }

  /* Agent cards */
  .card {
    width: 244px;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 7px;
    padding: 10px 13px;
    overflow: hidden;
    border-radius: 16px;
    border: 1px solid var(--border);
    background: color-mix(in oklch, var(--surface) 70%, transparent);
    outline: none;
    transition: transform var(--mv), height var(--mv), background-color 240ms var(--ease), border-color 240ms var(--ease), box-shadow 240ms var(--ease), opacity 220ms var(--ease), filter 220ms var(--ease);
  }
  .card:focus-visible {
    outline: 2px solid var(--gold);
    outline-offset: 3px;
  }
  .card.sel {
    outline: 2px solid var(--gold-ring);
    outline-offset: 4px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 11px;
    min-width: 0;
  }
  .m {
    position: relative;
    flex: none;
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    border-radius: 9999px;
    background: color-mix(in oklch, currentColor 12%, var(--surface));
    box-shadow: inset 0 0 0 1px color-mix(in oklch, currentColor 35%, transparent);
  }
  .arc {
    position: absolute;
    inset: -4px;
    display: none;
    border-radius: 9999px;
  }
  .ti {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .ti b {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 600;
    font-size: 14.5px;
    line-height: 1.25;
  }
  .st {
    color: var(--muted-foreground);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
  .ask {
    display: flex;
    align-items: center;
    gap: 7px;
    color: var(--ember-ink);
    font: 600 17px/1.18 var(--font-sans);
    letter-spacing: -0.012em;
    text-wrap: balance;
  }
  .ask :global(svg) {
    flex: none;
  }
  .meta {
    display: none;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
    padding-top: 6px;
    border-top: 1px solid var(--border);
    color: var(--muted-foreground);
    font-size: 11.5px;
    line-height: 1.35;
  }
  :global(.war[data-lod="near"]) .meta {
    display: flex;
  }
  .where {
    display: flex;
    gap: 8px;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
  }
  .where > :first-child {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .br {
    display: inline-flex;
    flex: 0 1 auto;
    align-items: center;
    gap: 4px;
    min-width: 0;
    max-width: 58%;
    margin-left: auto;
    font: 11px var(--font-mono);
  }
  .br span {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .mt {
    color: var(--faint);
    font: 11px var(--font-mono);
  }
  .sweep {
    display: none;
  }

  /* Waiting: the loudest thing on the table. Filled ember, a still ring, the ask written large */
  .card[data-status="attention"] {
    border: 1.5px solid var(--ember);
    background: color-mix(in oklch, var(--ember) 16%, var(--surface));
    box-shadow: 0 0 0 6px var(--ember-wash), var(--sh-chip);
  }
  .card[data-status="attention"] .arc {
    display: block;
    border: 2px solid var(--ember);
  }
  .card[data-status="attention"] .st {
    color: var(--ember-ink);
  }
  .card[data-status="attention"] .meta {
    border-top-color: var(--ember-edge);
  }
  /* Working: gold, and it moves */
  .card[data-status="busy"] {
    border-color: var(--border-strong);
    background: var(--surface);
    box-shadow: var(--sh-chip);
  }
  .card[data-status="busy"] .arc {
    display: block;
    border: 2px solid var(--gold-soft);
    border-top-color: var(--gold);
    border-right-color: var(--gold);
    animation: spin 1.5s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(1turn);
    }
  }
  /* Idle: a plain hairline, dimmed. Not started: dashed */
  .card[data-status="idle"] .ti b,
  .card[data-status="asleep"] .ti b {
    color: var(--muted-foreground);
    font-weight: 500;
  }
  .card[data-status="idle"] .m,
  .card[data-status="asleep"] .m {
    opacity: 0.7;
    filter: saturate(0.35);
  }
  .card[data-status="asleep"] {
    border: 1px dashed var(--border-strong);
    background: transparent;
  }
  .card[data-status="asleep"] .st {
    color: var(--faint);
  }
  /* Far: cards become solid blocks; working ones keep moving with a sweep */
  :global(.war[data-lod="far"]) .card > :not(.sweep) {
    visibility: hidden;
  }
  :global(.war[data-lod="far"]) .card {
    border-width: calc(1px * var(--ik));
  }
  :global(.war[data-lod="far"]) .card[data-status="attention"] {
    border-color: var(--ember);
    background: var(--ember);
    box-shadow: 0 0 0 calc(5px * var(--ik)) var(--ember-wash);
  }
  :global(.war[data-lod="far"]) .card[data-status="busy"] {
    border-color: var(--gold);
    background: var(--gold-soft);
    box-shadow: none;
  }
  :global(.war[data-lod="far"]) .card[data-status="idle"] {
    border-color: var(--border-strong);
    background: transparent;
  }
  :global(.war[data-lod="far"]) .card[data-status="busy"] .sweep {
    position: absolute;
    left: 0;
    bottom: 0;
    display: block;
    width: 45%;
    height: calc(4px * var(--ik));
    background: linear-gradient(90deg, transparent, var(--gold), transparent);
    animation: sweep 1.8s var(--ease) infinite;
  }
  @keyframes sweep {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(240%);
    }
  }

  /* Servers */
  .chip {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 244px;
    height: 30px;
    padding: 0 11px;
    border-radius: 10px;
    outline: none;
    white-space: nowrap;
    background: var(--surface);
    box-shadow: inset 0 0 0 1px var(--border), 0 1px 2px oklch(var(--shadow-tint) / 0.08);
    font-size: 12.5px;
    transition: transform var(--mv), opacity 220ms var(--ease), filter 220ms var(--ease);
  }
  .chip:focus-visible {
    outline: 2px solid var(--gold);
    outline-offset: 3px;
  }
  .chip.sel {
    outline: 2px solid var(--gold-ring);
    outline-offset: 3px;
  }
  .dot {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 9999px;
    background: var(--ok);
    box-shadow: 0 0 0 2.5px color-mix(in oklch, var(--ok) 20%, transparent);
  }
  .cn {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    font-weight: 500;
  }
  .kind {
    display: none;
    color: var(--faint);
    font-size: 11px;
  }
  :global(.war[data-lod="near"]) .kind,
  .chip.down .kind {
    display: inline;
  }
  .port {
    margin-left: auto;
    color: var(--muted-foreground);
    font: 400 11.5px var(--font-mono);
    font-variant-numeric: tabular-nums;
  }
  .chip.down {
    background: transparent;
    box-shadow: none;
    border: 1px dashed var(--border-strong);
  }
  .chip.down .dot {
    background: transparent;
    box-shadow: inset 0 0 0 1.5px var(--muted-foreground);
  }
  .chip.down .cn,
  .chip.down .port {
    color: var(--faint);
  }
  :global(.war[data-lod="far"]) .chip > * {
    visibility: hidden;
  }
  :global(.war[data-lod="far"]) .chip {
    background: color-mix(in oklch, var(--ok) 30%, var(--surface));
    box-shadow: none;
  }
  :global(.war[data-lod="far"]) .chip.down {
    background: transparent;
    border-width: calc(1px * var(--ik));
  }
</style>
