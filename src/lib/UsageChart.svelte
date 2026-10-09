<script lang="ts">
  import { fmtTokens, providerName, type Day, type Provider } from "./usageStats";

  // Daily processed tokens: per day a group of bars side by side, one per provider (fixed order), a tick under every day.
  let { days, providers }: { days: Day[]; providers: Provider[] } = $props();

  const H = 220, TOP = 8, BOTTOM = 26, LEFT = 44, RIGHT = 4, R = 3;
  let width = $state(600);
  let hover = $state<number | null>(null);

  /** Round the axis up to 1/2/2.5/5 x 10^k so gridlines land on readable values. */
  function niceMax(v: number) {
    if (v <= 0) return 1;
    const k = 10 ** Math.floor(Math.log10(v));
    return [1, 2, 2.5, 5, 10].map((m) => m * k).find((m) => m >= v)!;
  }
  const max = $derived(niceMax(Math.max(0, ...days.flatMap((d) => providers.map((p) => d.by[p])))));
  const plotW = $derived(Math.max(1, width - LEFT - RIGHT));
  const slot = $derived(plotW / days.length);
  // Bars at most 14px wide with a 2px gap between them; on long ranges they shrink (and the gap goes) to fit the day.
  const gap = $derived(slot > 16 ? 2 : 0);
  const barW = $derived(Math.max(1, Math.min(14, (slot * 0.8 - gap * (providers.length - 1)) / Math.max(1, providers.length))));
  const groupW = $derived(barW * providers.length + gap * (providers.length - 1));
  const base = H - BOTTOM;
  const cx = (i: number) => LEFT + slot * (i + 0.5);
  const y = (v: number) => TOP + (1 - v / max) * (base - TOP);
  // Label every day when they fit, else every k-th day counted back from today (so today is always labelled).
  const every = $derived(Math.max(1, Math.ceil(52 / slot)));
  const labelled = (i: number) => (days.length - 1 - i) % every === 0;

  const bars = $derived(
    days.map((d, i) => ({
      i,
      bars: providers.map((p, k) => ({ p, x: cx(i) - groupW / 2 + k * (barW + gap), y: y(d.by[p]), h: base - y(d.by[p]) })).filter((b) => b.h > 0),
    })),
  );
  /** A rect with only its top corners rounded. */
  function topRounded(x: number, y: number, w: number, h: number) {
    const r = Math.min(R, h, w / 2);
    return `M${x},${y + h}V${y + r}Q${x},${y} ${x + r},${y}H${x + w - r}Q${x + w},${y} ${x + w},${y + r}V${y + h}Z`;
  }

  const fmtDay = (t: number, o: Intl.DateTimeFormatOptions) => new Date(t).toLocaleDateString(undefined, o);

  function move(e: PointerEvent) {
    const r = (e.currentTarget as SVGElement).getBoundingClientRect();
    const i = Math.floor((e.clientX - r.left - LEFT) / slot);
    hover = i >= 0 && i < days.length ? i : null;
  }
</script>

<div class="chart" bind:clientWidth={width}>
  <svg {width} height={H} onpointermove={move} onpointerleave={() => (hover = null)} role="img" aria-label="Daily processed tokens by tool">
    {#each [0.5, 1] as f}
      <line class="grid" x1={LEFT} x2={width - RIGHT} y1={y(max * f)} y2={y(max * f)} />
    {/each}
    {#each [0, 0.5, 1] as f}
      <text class="axis" x={LEFT - 8} y={y(max * f)} dy="0.32em" text-anchor="end">{fmtTokens(max * f)}</text>
    {/each}
    {#each bars as b (b.i)}
      <g class:dim={hover !== null && hover !== b.i}>
        {#each b.bars as s (s.p)}
          <path d={topRounded(s.x, s.y, barW, Math.max(1, s.h))} style:fill="var(--usage-{s.p})" />
        {/each}
      </g>
    {/each}
    <line class="baseline" x1={LEFT} x2={width - RIGHT} y1={base} y2={base} />
    {#each days as d, i (d.start)}
      <line class="tick" class:major={labelled(i)} x1={cx(i)} x2={cx(i)} y1={base} y2={base + (labelled(i) ? 5 : 3)} />
      {#if labelled(i)}
        <text class="axis" class:on={hover === i} x={cx(i)} y={H - 4} text-anchor="middle">
          {days.length <= 7 ? fmtDay(d.start, { weekday: "short", day: "numeric" }) : fmtDay(d.start, { month: "short", day: "numeric" })}
        </text>
      {/if}
    {/each}
  </svg>
  {#if hover !== null}
    {@const d = days[hover]}
    <div class="tip" class:left={cx(hover) > width / 2} style:--x="{cx(hover) + (cx(hover) > width / 2 ? -slot / 2 : slot / 2)}px">
      <div class="when">{fmtDay(d.start, { weekday: "short", month: "short", day: "numeric" })}</div>
      {#each providers.filter((p) => d.by[p]) as p}
        <div class="row"><span class="dot" style:background="var(--usage-{p})"></span>{providerName[p]}<b>{fmtTokens(d.by[p])}</b></div>
      {/each}
      <div class="row total">{d.sessions} session{d.sessions === 1 ? "" : "s"}<b>{fmtTokens(d.total)}</b></div>
    </div>
  {/if}
</div>

<style>
  .chart {
    position: relative;
    min-width: 0;
  }
  svg {
    display: block;
    overflow: visible;
  }
  .grid {
    stroke: var(--border);
    stroke-dasharray: 2 3;
  }
  .baseline,
  .tick {
    stroke: var(--border-strong);
  }
  .axis {
    fill: var(--muted-foreground);
    font-size: 10.5px;
    font-variant-numeric: tabular-nums;
  }
  .axis.on {
    fill: var(--foreground);
  }
  g {
    transition: opacity var(--dur-quick) var(--ease-standard);
  }
  g.dim {
    opacity: 0.4;
  }
  .tip {
    position: absolute;
    top: 4px;
    left: calc(var(--x) + 8px);
    min-width: 168px;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--popover);
    box-shadow: var(--shadow-raised);
    backdrop-filter: blur(12px);
    font-size: 12px;
    pointer-events: none;
  }
  .tip.left {
    left: auto;
    right: calc(100% - var(--x) + 8px);
  }
  .when {
    margin-bottom: 4px;
    color: var(--muted-foreground);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    line-height: 20px;
  }
  .row b {
    margin-left: auto;
    padding-left: 12px;
    font-weight: 500;
    font-variant-numeric: tabular-nums;
  }
  .row.total {
    margin-top: 2px;
    border-top: 1px solid var(--border);
    padding-top: 2px;
    color: var(--muted-foreground);
  }
  .row.total b {
    color: var(--foreground);
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 2px;
  }
</style>
