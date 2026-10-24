<script lang="ts">
  import { GitBranch } from "@lucide/svelte";
  import ProviderIcon from "./ProviderIcon.svelte";
  import { agentProvider } from "./agents.svelte";
  import { fmtSpan, STATUSES, type Row, type Status } from "./fleet";
  import { nameOf } from "./workspaces.svelte";
  import { providerName } from "./usageStats";
  import { C, fmtNum, langColor, langShares, tally, type Camp, type Project, type Stats, type Tokens } from "./war";

  // Castra: every project a walled camp on the survey grid. Agents stand inside as coins in ranks (sorted by
  // urgency), its servers and containers are towers set into the wall, and roads run from a coin to what it started.
  let { projects, camps, info, v, focus, selected, lifted = "", dropOn = "" }: {
    projects: Project[];
    camps: Camp[];
    info: { stats: Record<string, Stats>; tokens: Map<string, Tokens>; branch: (key: string) => string };
    v: { x: number; y: number; k: number };
    focus: string;
    selected: string;
    lifted?: string; // the camp being dragged to a new place
    dropOn?: string; // the camp whose place it would take
  } = $props();

  const byKey = $derived(new Map(projects.map((p) => [p.key, p])));
  const words: Record<Status, string> = { attention: "needs you", busy: "working", idle: "idle", asleep: "not started" };
  const title = (r: Row) => r.label || (agentProvider[r.agent] ? providerName[agentProvider[r.agent]] : r.agent);
  const ask = (r: Row) => (r.asking === "approval" ? "Waiting for your approval" : r.asking === "question" ? "Asking you a question" : "Your turn");
  const home = (p: string) => p.replace(/^\/Users\/[^/]+/, "~");
  const base = (p: string) => p.slice(p.lastIndexOf("/") + 1);
  const tallyTitle = (t: Record<Status, number>) => STATUSES.filter((s) => t[s]).map((s) => `${t[s]} ${words[s]}`).join(", ");
</script>

{#snippet tallies(t: Record<Status, number>, all: boolean)}
  {#each STATUSES.filter((s) => t[s]) as s (s)}<span class="t" data-s={s}><i class="g" data-s={s}></i>{t[s]}{#if all || s === "attention"}<em>{" " + words[s]}</em>{/if}</span>{/each}
{/snippet}

<div class="world" style:transform="translate({v.x}px, {v.y}px) scale({v.k})">
  {#each camps as c (c.key)}
    {@const p = byKey.get(c.key)!}
    {@const t = tally(p.agents)}
    {@const s = info.stats[c.key]}
    {@const tok = info.tokens.get(c.key)}
    {@const branch = info.branch(c.key)}
    {@const units = new Map(c.units.map((u) => [u.id, u]))}
    {@const towers = new Map(c.towers.map((u) => [u.id, u]))}
    <div class="camp" data-project={c.key} class:dim={focus && focus !== c.key} class:lit={focus === c.key} class:hot={t.attention > 0} class:lifted={lifted === c.key} class:drop={dropOn === c.key} style:transform="translate({c.x}px, {c.y}px)" style:width="{c.w}px" style:height="{c.h}px">
      <div class="plate" data-project-hit={c.key} title="Drag onto another camp to put it in that one's place" style:height="{c.plateH}px">
        <div class="head">
          <div class="h1"><h3 class="name" title={c.key}>{p.name}</h3><span class="tally" title={tallyTitle(t)}>{@render tallies(t, false)}</span></div>
          <div class="branch">{#if branch}<GitBranch size={12} strokeWidth={1.5} /><span class="bn">{branch}</span>{/if}<span class="path">{home(c.key)}</span></div>
          <div class="stats">
            <div><b>{s ? fmtNum(s.lines) : "…"}</b><span>lines<em>{s ? ` · ${fmtNum(s.files)} files` : ""}</em></span></div>
            <div><b>{tok ? fmtNum(tok.month) : "…"}</b><span>tokens<em>{" in 30 days"}{tok ? ` · ${tok.today ? fmtNum(tok.today) + " today" : "none today"}` : ""}</em></span></div>
            <div><b>{s ? s.commits7d : "…"}</b><span>commit{s?.commits7d === 1 ? "" : "s"}<em>{" in 7 days"}</em></span></div>
          </div>
          <div class="strip">{#each s ? langShares(s) : [] as l (l.name)}<i style:flex={l.share} style:background={langColor(l.name)} title="{l.name} {Math.round(l.share * 100)}%"></i>{/each}</div>
          <div class="legend">{#each s ? langShares(s) : [] as l (l.name)}<span><i style:background={langColor(l.name)}></i>{l.name} <em>{Math.round(l.share * 100)}%</em></span>{/each}</div>
        </div>
      </div>
      <svg class="roads" width={c.w} height={c.h} aria-hidden="true">
        {#each c.roads as r (r.service)}
          {@const a = p.agents.find((x) => x.id === r.agent)}
          {@const srv = p.services.find((x) => x.id === r.service)}
          <path class="road" data-status={a?.status} class:down={!srv?.up} d={r.d} />
        {/each}
      </svg>
      {#each p.agents as a (a.id)}
        {@const u = units.get(a.id)}
        {@const prov = agentProvider[a.agent]}
        {#if u}
          <div class="unit" data-agent={a.id} data-status={a.status} class:sel={selected === a.id} tabindex="0" role="button" aria-label="{title(a)}: {a.status === 'attention' ? ask(a) : words[a.status]}" style:transform="translate({u.x}px, {u.y}px)">
            <div class="coin" style:color={prov ? `var(--usage-${prov})` : undefined}>
              <i class="halo"></i>
              <svg class="ring" viewBox="0 0 72 72" aria-hidden="true"><circle class="track" cx="36" cy="36" r="33" /><circle class="arc" cx="36" cy="36" r="33" /></svg>
              <span class="icon">{#if prov}<ProviderIcon provider={prov} size={24} />{/if}</span>
            </div>
            <div class="lbl">
              <b class="ttl">{title(a)}</b>
              <span class="st">{#if a.status === "attention"}{ask(a)}{:else}{words[a.status][0].toUpperCase() + words[a.status].slice(1)}{/if}{#if a.since !== null}<span class="since">{" · " + fmtSpan(a.since)}</span>{/if}</span>
              <span class="near where">{a.ws ? nameOf(a.ws) : "Detached"}{base(a.cwd) !== p.name ? ` / ${base(a.cwd)}` : ""}</span>
              {#if prov && a.label}<span class="near prov">{providerName[prov]}</span>{/if}
            </div>
          </div>
        {/if}
      {/each}
      {#each p.services as srv (srv.id)}
        {@const tw = towers.get(srv.id)}
        {#if tw}
          <div class="tower" data-service={srv.id} class:down={!srv.up} class:sel={selected === srv.id} tabindex="0" role="button" aria-label="{srv.name}: {srv.up ? 'running' : 'stopped'}" style:transform="translate({tw.x}px, {tw.y}px)">
            <div class="tn"><span class="ico" data-type={srv.type}></span><span>{srv.name}</span></div>
            <div class="tp"><span class="port">{#if srv.ports.length}:{srv.ports[0]}{#if srv.ports.length > 1}<span class="more">{" +" + (srv.ports.length - 1)}</span>{/if}{:else}no port{/if}</span><span class="ts"><i></i>{srv.up ? "up" : "down"}</span></div>
          </div>
        {/if}
      {/each}
      <div class="far" style:--fw={c.w - C.T - 8}><b>{p.name}</b><span class="tally">{@render tallies(t, false)}</span></div>
    </div>
  {/each}
</div>

<style>
  .world {
    position: absolute;
    left: 0;
    top: 0;
    transform-origin: 0 0;
    will-change: transform;
  }
  .camp {
    position: absolute;
    left: 0;
    top: 0;
    transition: transform 420ms var(--ease), opacity 220ms var(--ease), filter 220ms var(--ease);
  }
  .camp.lifted {
    opacity: 0.55;
  }
  .camp.drop .plate {
    box-shadow: 0 0 0 2px var(--gold), 0 18px 36px -24px oklch(var(--shadow-tint) / 0.4);
  }
  .camp.dim {
    opacity: 0.22;
    filter: saturate(0.4);
  }
  .camp.dim:hover {
    opacity: 0.6;
  }

  /* The camp plate: a double-hairline rampart */
  .plate {
    position: absolute;
    left: 0;
    top: 0;
    width: 100%;
    border-radius: 10px;
    background:
      linear-gradient(to right, color-mix(in oklch, var(--foreground) 2.6%, transparent) 1px, transparent 1px) 0 0 / 32px 32px,
      linear-gradient(to bottom, color-mix(in oklch, var(--foreground) 2.6%, transparent) 1px, transparent 1px) 0 0 / 32px 32px,
      color-mix(in oklch, var(--chip-on) 88%, var(--surface));
    box-shadow: 0 0 0 1.5px var(--border-strong), 0 1px 2px oklch(var(--shadow-tint) / 0.1), 0 18px 36px -24px oklch(var(--shadow-tint) / 0.4);
    transition: box-shadow 220ms var(--ease);
  }
  .plate::before {
    content: "";
    position: absolute;
    inset: 4px;
    border-radius: 7px;
    border: 1px solid var(--border);
    pointer-events: none;
  }
  .plate:hover {
    box-shadow: 0 0 0 1.5px color-mix(in oklch, var(--foreground) 30%, transparent), 0 1px 2px oklch(var(--shadow-tint) / 0.1), 0 18px 36px -24px oklch(var(--shadow-tint) / 0.4);
  }
  .lit .plate {
    box-shadow: 0 0 0 1.5px var(--gold-ring), 0 0 0 5px var(--gold-soft), 0 18px 36px -24px oklch(var(--shadow-tint) / 0.4);
  }

  .head {
    position: absolute;
    left: 0;
    right: 0;
    top: 0;
    height: 192px;
    padding: 20px 32px 0;
    border-bottom: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    cursor: pointer;
    transition: opacity 180ms var(--ease);
  }
  .h1 {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-width: 0;
  }
  .name {
    margin: 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font: 600 max(22px, calc(13px * var(--ik))) / 1.15 var(--font-cinzel);
    letter-spacing: 0.03em;
  }
  .branch {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    margin-top: 3px;
    color: var(--muted-foreground);
    font: 12px/1.3 var(--font-mono);
    white-space: nowrap;
  }
  .branch :global(svg) {
    flex: none;
  }
  .bn {
    color: var(--foreground);
  }
  .path {
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--faint);
    font: 11.5px var(--font-sans);
  }
  .stats {
    display: flex;
    gap: 6px 22px;
    margin-top: 14px;
  }
  .stats > div {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .stats b {
    font: 500 max(16px, calc(9.5px * var(--ik))) / 1.15 var(--font-mono);
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.01em;
  }
  .stats span {
    color: var(--muted-foreground);
    font-size: 11.5px;
    line-height: 1.3;
  }
  .stats em {
    font-style: normal;
  }
  :global(.war[data-lod="mid"]) .stats > div {
    flex-direction: row;
    align-items: baseline;
    gap: 5px;
  }
  :global(.war[data-lod="mid"]) .stats {
    gap: 6px 26px;
  }
  :global(.war[data-lod="mid"]) .stats em,
  :global(.war[data-lod="mid"]) .path {
    display: none;
  }
  :global(.war[data-lod="mid"]) .stats span {
    font-size: max(11.5px, calc(8.5px * var(--ik)));
  }
  .strip {
    display: flex;
    gap: 2px;
    height: 6px;
    margin-top: 14px;
    border-radius: 3px;
    overflow: hidden;
    background: color-mix(in oklch, var(--foreground) 5%, transparent);
  }
  .strip i {
    min-width: 2px;
  }
  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: 2px 12px;
    margin-top: 8px;
    color: var(--muted-foreground);
    font-size: 11px;
    transition: opacity 180ms var(--ease);
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
  .legend em {
    color: var(--faint);
    font: normal 10.5px var(--font-mono);
  }
  :global(.war:not([data-lod="near"])) .legend {
    opacity: 0;
  }

  /* Tallies, in Fleet's marks */
  .tally {
    display: inline-flex;
    flex: none;
    align-items: center;
    gap: 10px;
    color: var(--muted-foreground);
    font: 500 max(12px, calc(8.5px * var(--ik))) / 1 var(--font-mono);
    font-variant-numeric: tabular-nums;
  }
  .t {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
  .t em {
    font: 500 1em var(--font-sans);
    font-style: normal;
  }
  .t[data-s="attention"] {
    color: var(--ember-ink);
  }
  .g {
    flex: none;
    width: 9px;
    height: 9px;
    box-sizing: border-box;
    border-radius: 9999px;
  }
  .g[data-s="attention"] {
    background: var(--ember);
    box-shadow: 0 0 0 2.5px var(--ember-wash), 0 0 0 3.5px var(--ember-edge);
  }
  .g[data-s="busy"] {
    border: 1.5px solid var(--gold-soft);
    border-top-color: var(--gold);
    border-right-color: var(--gold);
  }
  .g[data-s="idle"] {
    border: 1px solid var(--faint);
  }
  .g[data-s="asleep"] {
    border: 1px dashed var(--faint);
  }

  /* Agents: coins in ranks. Waiting is a still, filled ember coin; working has a turning gold arc; idle a dimmed
     hairline; not started a dashed one. */
  .unit {
    position: absolute;
    left: 0;
    top: 0;
    width: 160px;
    height: 224px;
    outline: none;
    transition: transform 520ms var(--ease);
  }
  .coin {
    position: absolute;
    left: 52px;
    top: 16px;
    width: 56px;
    height: 56px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: color-mix(in oklch, var(--chip-on) 70%, var(--surface));
    box-shadow: inset 0 0 0 1px var(--border-strong), inset 0 1px 0 var(--hl), 0 1px 2px oklch(var(--shadow-tint) / 0.18), 0 4px 10px -6px oklch(var(--shadow-tint) / 0.4);
    transition: background 220ms var(--ease), box-shadow 220ms var(--ease), transform 180ms var(--ease);
  }
  .coin::after {
    content: "";
    position: absolute;
    inset: 3px;
    border-radius: 50%;
    border: 1px solid var(--border);
    pointer-events: none;
  }
  .icon {
    position: relative;
    z-index: 1;
    display: grid;
    transition: opacity 220ms var(--ease), filter 220ms var(--ease);
  }
  .halo {
    position: absolute;
    inset: -11px;
    border-radius: 50%;
    opacity: 0;
    background: var(--ember-wash);
    box-shadow: inset 0 0 0 1px var(--ember-edge);
    transition: opacity 220ms var(--ease);
  }
  .ring {
    position: absolute;
    inset: -8px;
    width: 72px;
    height: 72px;
    overflow: visible;
    pointer-events: none;
  }
  .ring circle {
    fill: none;
    stroke-width: 2.25;
    opacity: 0;
    transition: opacity 220ms var(--ease);
  }
  .track {
    stroke: var(--gold-soft);
  }
  .arc {
    stroke: var(--gold);
    stroke-linecap: round;
    stroke-dasharray: 46 162;
    transform-box: fill-box;
    transform-origin: center;
    animation: spin 1.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .unit[data-status="busy"] .ring circle {
    opacity: 1;
  }
  .unit[data-status="attention"] .halo {
    opacity: 1;
  }
  .unit[data-status="attention"] .coin {
    background: var(--ember);
    color: var(--surface) !important;
    box-shadow: inset 0 0 0 1px color-mix(in oklch, var(--ember-ink) 60%, transparent), 0 1px 2px oklch(var(--shadow-tint) / 0.2);
  }
  .unit[data-status="attention"] .coin :global(.pi) {
    color: var(--surface);
  }
  .unit[data-status="attention"] .coin::after {
    border-color: color-mix(in oklch, var(--surface) 45%, transparent);
  }
  .unit[data-status="idle"] .icon {
    opacity: 0.6;
    filter: saturate(0.45);
  }
  .unit[data-status="asleep"] .coin {
    background: transparent;
    box-shadow: none;
    outline: 1px dashed var(--faint);
    outline-offset: -1px;
  }
  .unit[data-status="asleep"] .coin::after {
    border-style: dashed;
  }
  .unit[data-status="asleep"] .icon {
    opacity: 0.45;
    filter: grayscale(1);
  }
  .unit:hover .coin {
    transform: translateY(-1px);
  }
  .unit.sel .coin,
  .unit:focus-visible .coin {
    outline: 2px solid var(--gold-ring);
    outline-offset: 13px;
  }
  .unit.sel .lbl,
  .tower.sel {
    box-shadow: 0 0 0 1.5px var(--gold-ring), 0 0 0 4px var(--gold-soft);
  }

  /* The label plate under each coin */
  .lbl {
    position: absolute;
    left: 8px;
    right: 8px;
    top: 82px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    padding: 7px 8px 8px;
    border-radius: 7px;
    text-align: center;
    background: var(--surface);
    box-shadow: 0 0 0 1px var(--border);
    transition: opacity 180ms var(--ease);
  }
  .ttl {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    overflow-wrap: anywhere;
    font-weight: 600;
    font-size: max(12.5px, calc(10px * var(--ik)));
    line-height: 1.22;
  }
  .st {
    max-width: 100%;
    color: var(--muted-foreground);
    font-size: max(11.5px, calc(8.6px * var(--ik)));
    line-height: 1.25;
  }
  .since {
    font-family: var(--font-mono);
    font-size: 0.92em;
    font-weight: 400;
    white-space: nowrap;
  }
  .unit[data-status="busy"] .st {
    color: var(--foreground);
  }
  .unit[data-status="attention"] .st {
    color: var(--ember-ink);
    font-weight: 500;
  }
  .unit[data-status="attention"] .lbl {
    background: color-mix(in oklch, var(--ember-wash) 100%, var(--surface));
    box-shadow: 0 0 0 1px var(--ember-edge);
  }
  .unit[data-status="idle"] .ttl,
  .unit[data-status="asleep"] .ttl {
    color: var(--muted-foreground);
    font-weight: 500;
  }
  .near {
    display: none;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--faint);
    font-size: 11px;
    line-height: 1.35;
  }
  :global(.war[data-lod="near"]) .near {
    display: block;
  }
  .where {
    align-self: stretch;
    margin-top: 4px;
    padding-top: 5px;
    border-top: 1px solid var(--border);
    color: var(--muted-foreground);
  }

  /* Towers on the rampart: one per server or container */
  .tower {
    position: absolute;
    left: 0;
    top: 0;
    width: 120px;
    height: 64px;
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 9px 11px 0;
    overflow: hidden;
    border-radius: 7px;
    outline: none;
    background: color-mix(in oklch, var(--chip-on) 80%, var(--surface));
    box-shadow: 0 0 0 1.5px var(--border-strong), 0 2px 6px -3px oklch(var(--shadow-tint) / 0.35);
    transition: box-shadow 180ms var(--ease);
  }
  .tower::before {
    content: "";
    position: absolute;
    inset: 3px;
    border-radius: 4px;
    border: 1px solid var(--border);
    pointer-events: none;
  }
  .tower:hover {
    box-shadow: 0 0 0 1.5px color-mix(in oklch, var(--foreground) 34%, transparent), 0 2px 6px -3px oklch(var(--shadow-tint) / 0.35);
  }
  .tower:focus-visible {
    box-shadow: 0 0 0 2px var(--gold-ring);
  }
  .tn {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    font-weight: 600;
    font-size: max(12.5px, calc(7.4px * var(--ik)));
    line-height: 1.2;
  }
  .tn > span:last-child {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ico {
    position: relative;
    flex: none;
    width: 14px;
    height: 14px;
    color: var(--muted-foreground);
  }
  .ico::before {
    content: "";
    position: absolute;
    inset: 3px 2px;
    border-radius: 2px;
    border: 1.5px solid currentColor;
  }
  .ico[data-type="container"]::before {
    inset: 2px;
    border-radius: 3px;
    box-shadow: inset 0 -3px 0 -1.5px currentColor;
  }
  .tp {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
    min-width: 0;
    color: var(--muted-foreground);
    font: max(11px, calc(6.2px * var(--ik))) / 1.2 var(--font-mono);
    white-space: nowrap;
  }
  .port {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .more {
    color: var(--faint);
  }
  .ts {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--ok);
    font-family: var(--font-sans);
  }
  .ts i {
    width: 7px;
    height: 7px;
    box-sizing: border-box;
    border-radius: 9999px;
    background: var(--ok);
  }
  .tower.down {
    background: color-mix(in oklch, var(--surface) 92%, transparent);
    box-shadow: none;
    outline: 1.5px dashed var(--border-strong);
  }
  .tower.down .tn,
  .tower.down .ts {
    color: var(--muted-foreground);
  }
  .tower.down .ts i {
    background: none;
    border: 1.5px solid var(--muted-foreground);
  }

  /* Roads from a coin to the towers it started */
  .roads {
    position: absolute;
    left: 0;
    top: 0;
    overflow: visible;
    pointer-events: none;
  }
  .road {
    fill: none;
    stroke: var(--border-strong);
    stroke-width: 1.5;
    transition: stroke 220ms var(--ease);
  }
  .road[data-status="busy"] {
    stroke: color-mix(in oklch, var(--gold-ring) 70%, transparent);
  }
  .road[data-status="attention"] {
    stroke: var(--ember-edge);
  }
  .road.down {
    stroke-dasharray: 4 4;
  }

  /* Far: an overview of camps. Names and tallies keep their size on screen; waiting camps' walls turn ember. */
  .far {
    position: absolute;
    left: 32px;
    top: 22px;
    display: none;
    flex-direction: column;
    gap: 6px;
    width: calc(var(--fw) * 1px * var(--k));
    transform-origin: 0 0;
    transform: scale(var(--ik));
    pointer-events: none;
  }
  :global(.war[data-lod="far"]) .far {
    display: flex;
  }
  .far b {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font: 600 17px/1.1 var(--font-cinzel);
    letter-spacing: 0.03em;
  }
  .far .tally {
    gap: 8px;
    font-size: 12.5px;
    white-space: nowrap;
  }
  :global(.war[data-lod="far"]) .head,
  :global(.war[data-lod="far"]) .lbl,
  :global(.war[data-lod="far"]) .tower > * {
    opacity: 0;
  }
  :global(.war[data-lod="far"]) .tower:not(.down) {
    background: color-mix(in oklch, var(--ok) 26%, var(--surface));
  }
  :global(.war[data-lod="far"]) .camp.hot .plate {
    box-shadow: 0 0 0 1.5px var(--ember-edge), 0 0 0 6px var(--ember-wash), 0 18px 36px -24px oklch(var(--shadow-tint) / 0.4);
  }
  :global(.war[data-lod="far"]) .coin {
    transform: scale(min(1.8, calc(0.55 * var(--ik))));
  }
  :global(.war[data-lod="far"]) .icon {
    opacity: 0;
  }
  :global(.war[data-lod="far"]) .unit[data-status="attention"] .halo {
    inset: -14px;
  }
</style>
