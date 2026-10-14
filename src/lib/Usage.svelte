<script lang="ts">
  import { RotateCw, TrendingDown, TrendingUp, TriangleAlert, X } from "@lucide/svelte";
  import ProviderIcon from "./ProviderIcon.svelte";
  import UsageChart from "./UsageChart.svelte";
  import { closeUsage, loadLimits, loadStats, poll, usage } from "./usageView.svelte";
  import { PROVIDERS, fmtLeft, fmtShare, fmtTokens, pace, providerName, summarize, type Limit, type Provider } from "./usageStats";

  const short: Record<Provider, string> = { claude: "Claude", codex: "Codex", grok: "Grok", antigravity: "Antigravity", opencode: "OpenCode", muse: "Muse" };

  $effect(poll);

  const s = $derived(usage.rows && summarize(usage.rows, usage.days, usage.now));
  const limitGroups = $derived(
    PROVIDERS.map((p) => ({
      p,
      limits: (usage.limits ?? []).filter((l) => l.provider === p),
      error: usage.errors.find(([e]) => e === p)?.[1],
    })).filter((g) => g.limits.length || g.error),
  );
  const shown = $derived(s?.providers.map((p) => p.provider) ?? []);

  const used = (l: Limit) => Math.max(0, Math.min(100, Math.round(l.used)));
  function paceTitle(l: Limit) {
    const p = pace(l, usage.now);
    if (!p) return "";
    return p.over ? `At this rate you'll run out before it resets (~${Math.round(p.projected)}% of the limit)` : `At this rate you'll use ~${Math.round(p.projected)}% before it resets`;
  }
  const dayLabel = (t: number) => new Date(t).toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" });

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape" && !e.defaultPrevented && !(e.target as Element).closest?.("input, textarea")) closeUsage();
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="usage slab">
  <div class="page">
    <header>
      <div>
        <h1>Usage</h1>
        <p class="sub">Subscription limits and token use across your AI coding tools</p>
      </div>
      <button class="icon-btn" onclick={closeUsage} title="Close (Esc)" aria-label="Close usage"><X size={16} strokeWidth={1.5} /></button>
    </header>

    <section>
      <h2>Limits</h2>
      {#if !usage.limits && usage.loadingLimits}
        <p class="muted">Checking limits…</p>
      {/if}
      <div class="limits">
        {#each limitGroups as g (g.p)}
          <div class="card group">
            <div class="provider"><ProviderIcon provider={g.p} />{short[g.p]}</div>
            {#each g.limits as l (l.label)}
              {@const p = pace(l, usage.now)}
              {@const low = used(l) >= 90}
              <div class="limit" class:low title={low ? (used(l) < 100 ? "Almost out" : "Limit reached") : paceTitle(l)}>
                <span class="label" title={l.label}>{l.label}</span>
                <div class="bar" style:--c="var(--usage-{g.p})" role="meter" aria-valuenow={used(l)} aria-valuemin={0} aria-valuemax={100} aria-label="{l.label}: {used(l)}% used">
                  <div class="fill" style:width="{used(l)}%"></div>
                </div>
                <span class="used">
                  {used(l)}%
                  {#if low}<TriangleAlert size={12} strokeWidth={2} />{:else if p?.over}<TrendingUp size={12} strokeWidth={2} />{:else if p}<TrendingDown size={12} strokeWidth={2} />{/if}
                </span>
                <span class="reset" title={l.resetsAt ? `Resets ${new Date(l.resetsAt).toLocaleString()}` : ""}>
                  {#if l.resetsAt}<RotateCw size={11} strokeWidth={2} />{fmtLeft(l.resetsAt, usage.now)}{:else}{l.note ?? ""}{/if}
                </span>
              </div>
              {#if l.resetsAt && l.note}<div class="limit-note">{l.note}</div>{/if}
            {/each}
            {#if g.error}
              <div class="error">
                <span>{g.error}</span>
                <button class="btn" onclick={() => loadLimits(true)} disabled={usage.loadingLimits}>Retry</button>
              </div>
            {/if}
          </div>
        {/each}
      </div>
      {#if usage.limits}
        <p class="note">
          % of each limit used, and when it resets.{#if limitGroups.some((g) => g.p === "codex" && g.limits.length)}{" "}Codex limits are as of its last response.{/if}{#if shown.includes("opencode")}{" "}OpenCode doesn't report its limits.{/if}{#if shown.includes("muse")}{" "}Muse Code doesn't report its limits.{/if}
        </p>
      {/if}
    </section>

    <section>
      <div class="section-head">
        <h2>Tokens</h2>
        <div class="seg" role="tablist" aria-label="Range">
          {#each [7, 30, 90] as const as d}
            <button role="tab" aria-selected={usage.days === d} class:on={usage.days === d} onclick={() => (usage.days = d)}>{d}D</button>
          {/each}
        </div>
      </div>
      {#if usage.statsError}
        <div class="card error"><span>{usage.statsError}</span><button class="btn" onclick={loadStats}>Retry</button></div>
      {:else if !s}
        <p class="muted">Reading session logs…</p>
      {:else if !s.sessions}
        <p class="muted">No Claude Code, Codex, Grok Build or Muse Code sessions in the last {usage.days} days.</p>
      {:else}
        <div class="overview">
          <div class="legend">
            <div class="hero">{fmtTokens(s.totals.processed)}</div>
            <div class="muted">{s.sessions} session{s.sessions === 1 ? "" : "s"}</div>
            <ul>
              {#each s.providers as p (p.provider)}
                <li>
                  <span class="dot" style:background="var(--usage-{p.provider})"></span>
                  <ProviderIcon provider={p.provider} />
                  <span class="name">{providerName[p.provider]}</span>
                  <span class="muted small">{p.sessions} session{p.sessions === 1 ? "" : "s"}</span>
                  <b>{fmtTokens(p.tokens)}</b>
                  <span class="muted small share">{fmtShare(p.share)} of tokens</span>
                </li>
              {/each}
            </ul>
          </div>
          <div class="plot">
            <h3>Daily processed tokens</h3>
            <UsageChart days={s.days} providers={shown} />
          </div>
        </div>

        <h3>Totals</h3>
        <div class="totals">
          <div><span class="muted">Processed tokens</span><b>{fmtTokens(s.totals.processed)}</b></div>
          <div title="Prompt tokens served from cache"><span class="muted">Cached input</span><b>{fmtTokens(s.totals.cached)}</b></div>
          <div title="Prompt tokens read fresh, including cache writes"><span class="muted">Uncached input</span><b>{fmtTokens(s.totals.uncached)}</b></div>
          <div><span class="muted">Output</span><b>{fmtTokens(s.totals.output)}</b></div>
        </div>

        <div class="section-head">
          <h3>Breakdown</h3>
          <div class="seg" role="tablist" aria-label="Breakdown">
            <button role="tab" aria-selected={usage.breakdown === "model"} class:on={usage.breakdown === "model"} onclick={() => (usage.breakdown = "model")}>Model</button>
            <button role="tab" aria-selected={usage.breakdown === "day"} class:on={usage.breakdown === "day"} onclick={() => (usage.breakdown = "day")}>Day</button>
          </div>
        </div>
        <table>
          {#if usage.breakdown === "model"}
            <thead><tr><th>Model</th><th>Share</th><th>Tokens</th></tr></thead>
            <tbody>
              {#each s.models as m (m.provider + m.model)}
                <tr>
                  <td><span class="cell"><ProviderIcon provider={m.provider} />{m.model}</span></td>
                  <td class="muted">{fmtShare(m.share)}</td>
                  <td>{fmtTokens(m.tokens)}</td>
                </tr>
              {/each}
            </tbody>
          {:else}
            <thead><tr><th>Day</th><th>Sessions</th><th>Share</th><th>Tokens</th></tr></thead>
            <tbody>
              {#each s.days.toReversed() as d (d.start)}
                <tr class:quiet={!d.total}>
                  <td>{dayLabel(d.start)}</td>
                  <td class="muted">{d.sessions || "—"}</td>
                  <td class="muted">{d.total ? fmtShare(s.share(d.total)) : "—"}</td>
                  <td>{d.total ? fmtTokens(d.total) : "—"}</td>
                </tr>
              {/each}
            </tbody>
          {/if}
        </table>
      {/if}
    </section>
  </div>
</div>

<style>
  /* Plane 2: a slab, like the panes it stands in for */
  .usage {
    grid-column: 2;
    grid-row: 2;
    min-width: 0;
    overflow: auto;
    /* Room for the scrollbar on both sides, so the column stays centred (and as wide) whether or not it scrolls */
    scrollbar-gutter: stable both-edges;
  }
  .page {
    box-sizing: border-box; /* like Skills and Agents: --page-width includes the padding */
    container-type: inline-size;
    max-width: var(--page-width);
    margin: 0 auto;
    padding: 28px 32px 48px;
  }
  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    margin-bottom: 8px;
  }
  h1 {
    margin: 0;
    font-size: 20px;
    font-weight: 600;
  }
  .sub {
    margin: 4px 0 0;
    color: var(--muted-foreground);
    font-size: 13px;
  }
  section {
    margin-top: 28px;
  }
  h2 {
    margin: 0 0 12px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--muted-foreground);
  }
  h3 {
    margin: 28px 0 12px;
    font-size: 13px;
    font-weight: 500;
  }
  .section-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 12px;
  }
  .section-head h2,
  .section-head h3 {
    margin: 0;
  }
  .section-head h3 {
    margin-top: 28px;
  }
  .section-head:has(h3) {
    align-items: flex-end;
  }
  .muted {
    color: var(--muted-foreground);
  }
  .small {
    font-size: 12px;
    white-space: nowrap;
  }
  .note {
    margin: 10px 0 0;
    color: var(--muted-foreground);
    font-size: 12px;
  }
  .limits {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(340px, 1fr));
    gap: 10px;
  }
  .card {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--card);
  }
  .group {
    padding: 12px 16px 10px;
  }
  .provider {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 6px;
    font-size: 13px;
    font-weight: 500;
  }
  /* One line per limit: label · bar (what's used) · % used · time to reset */
  .limit {
    display: grid;
    grid-template-columns: 136px minmax(0, 1fr) 58px 62px;
    align-items: center;
    gap: 12px;
    height: 28px;
    font-size: 12.5px;
  }
  .label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--muted-foreground);
  }
  .bar {
    height: 6px;
    border-radius: 9999px;
    background: var(--muted);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    border-radius: 9999px;
    background: var(--c);
    transition: width var(--dur-base) var(--ease-standard);
  }
  .used {
    display: inline-flex;
    align-items: center;
    justify-content: flex-end;
    gap: 4px;
    font-weight: 500;
    font-variant-numeric: tabular-nums;
  }
  .used :global(svg) {
    color: var(--muted-foreground);
  }
  .low .used,
  .low .used :global(svg) {
    color: var(--destructive);
  }
  .reset {
    display: inline-flex;
    align-items: center;
    justify-content: flex-end;
    gap: 4px;
    color: var(--muted-foreground);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .card.error {
    padding: 10px 16px;
  }
  .limit-note {
    margin: -4px 0 2px 148px;
    color: var(--muted-foreground);
    font-size: 11.5px;
  }
  .error {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 4px 0;
    color: var(--muted-foreground);
    font-size: 12.5px;
  }
  .btn {
    flex: none;
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--input);
    border-radius: var(--radius-sm);
    background: var(--background);
    color: var(--foreground);
    font-size: 12px;
    cursor: pointer;
  }
  .btn:hover {
    background: var(--accent);
  }
  .seg {
    display: inline-flex;
    padding: 2px;
    border-radius: var(--radius-md);
    background: var(--muted);
  }
  .seg button {
    height: 24px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--muted-foreground);
    font-size: 12px;
    cursor: pointer;
  }
  .seg button.on {
    background: var(--background);
    color: var(--foreground);
    box-shadow: var(--shadow-raised);
  }
  .overview {
    display: grid;
    grid-template-columns: minmax(220px, 280px) minmax(0, 1fr);
    gap: 32px;
  }
  .hero {
    font-size: 40px;
    font-weight: 600;
    letter-spacing: -0.02em;
    line-height: 1.1;
    font-variant-numeric: tabular-nums;
  }
  ul {
    margin: 18px 0 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: grid;
    grid-template-columns: 8px 16px auto 1fr auto;
    align-items: center;
    column-gap: 8px;
    padding: 6px 0;
    font-size: 13px;
  }
  li b {
    font-weight: 500;
    font-variant-numeric: tabular-nums;
  }
  li .share {
    grid-column: 3 / -1;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 2px;
  }
  .plot h3 {
    margin: 0 0 8px;
  }
  .totals {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 16px;
  }
  .totals div {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 12px;
  }
  .totals b {
    font-size: 18px;
    font-weight: 500;
    font-variant-numeric: tabular-nums;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 13px;
  }
  th {
    padding: 8px 0;
    border-bottom: 1px solid var(--border);
    color: var(--muted-foreground);
    font-weight: 400;
    text-align: right;
  }
  td {
    padding: 9px 0;
    border-bottom: 1px solid var(--border);
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  th:first-child,
  td:first-child {
    text-align: left;
  }
  tr.quiet td {
    color: var(--muted-foreground);
  }
  .cell {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }
  @container (max-width: 680px) {
    .overview {
      grid-template-columns: minmax(0, 1fr);
    }
    .totals {
      grid-template-columns: repeat(2, 1fr);
    }
  }
</style>
