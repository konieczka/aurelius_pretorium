// Usage view math: rows from `usage_stats` (hourly, per session and model) -> what the page shows.

export type Provider = "claude" | "codex" | "grok" | "antigravity" | "opencode" | "muse";
export const PROVIDERS: Provider[] = ["claude", "codex", "grok", "antigravity", "opencode", "muse"]; // fixed order: it also fixes their colors
export const providerName: Record<Provider, string> = { claude: "Claude Code", codex: "Codex", grok: "Grok Build", antigravity: "Antigravity", opencode: "OpenCode", muse: "Muse Code" };

export type Row = { provider: Provider; model: string; session: string; cwd: string; hour: number; uncached: number; cached: number; output: number; total: number };
export type Limit = { provider: Provider; label: string; used: number; resetsAt: number | null; windowMinutes: number | null; note: string | null };

export type Totals = { processed: number; cached: number; uncached: number; output: number };
export type Day = { start: number; total: number; sessions: number; by: Record<Provider, number> };

const zero = (): Record<Provider, number> => ({ claude: 0, codex: 0, grok: 0, antigravity: 0, opencode: 0, muse: 0 });

/** Local midnight `days - 1` days before `now`'s day: the range is today plus the days before it. */
export function rangeStart(days: number, now = Date.now()) {
  const d = new Date(now);
  d.setHours(0, 0, 0, 0);
  d.setDate(d.getDate() - (days - 1));
  return d.getTime();
}

export function summarize(rows: Row[], days: number, now = Date.now()) {
  const start = rangeStart(days, now);
  const series: Day[] = [];
  for (let i = 0; i < days; i++) {
    const d = new Date(start);
    d.setDate(d.getDate() + i);
    series.push({ start: d.getTime(), total: 0, sessions: 0, by: zero() });
  }
  const dayOf = (t: number) => {
    let i = series.length - 1;
    while (i > 0 && series[i].start > t) i--;
    return series[i];
  };
  const totals: Totals = { processed: 0, cached: 0, uncached: 0, output: 0 };
  const providers = new Map<Provider, { tokens: number; sessions: Set<string> }>();
  const models = new Map<string, { provider: Provider; model: string; tokens: number }>();
  const daySessions = new Map<Day, Set<string>>();
  const all = new Set<string>();
  for (const r of rows) {
    if (r.hour < start) continue;
    totals.processed += r.total;
    totals.cached += r.cached;
    totals.uncached += r.uncached;
    totals.output += r.output;
    const p = providers.get(r.provider) ?? providers.set(r.provider, { tokens: 0, sessions: new Set() }).get(r.provider)!;
    p.tokens += r.total;
    const sid = `${r.provider}:${r.session}`;
    p.sessions.add(sid);
    all.add(sid);
    const key = `${r.provider}:${r.model}`;
    const m = models.get(key) ?? models.set(key, { provider: r.provider, model: r.model, tokens: 0 }).get(key)!;
    m.tokens += r.total;
    const day = dayOf(r.hour);
    day.total += r.total;
    day.by[r.provider] += r.total;
    (daySessions.get(day) ?? daySessions.set(day, new Set()).get(day)!).add(sid);
  }
  for (const [day, s] of daySessions) day.sessions = s.size;
  const share = (n: number) => (totals.processed ? (n / totals.processed) * 100 : 0);
  return {
    totals,
    sessions: all.size,
    providers: PROVIDERS.filter((p) => providers.has(p)).map((p) => {
      const v = providers.get(p)!;
      return { provider: p, tokens: v.tokens, sessions: v.sessions.size, share: share(v.tokens) };
    }),
    models: [...models.values()].sort((a, b) => b.tokens - a.tokens).map((m) => ({ ...m, share: share(m.tokens) })),
    days: series,
    share,
  };
}
export type Summary = ReturnType<typeof summarize>;

/** 3 significant digits: 146K, 1.41M, 17.8M, 437M. */
export function fmtTokens(n: number) {
  const [v, unit] = n >= 1e9 ? [n / 1e9, "B"] : n >= 1e6 ? [n / 1e6, "M"] : n >= 1e3 ? [n / 1e3, "K"] : [n, ""];
  if (!unit) return String(Math.round(v));
  return (v >= 100 ? Math.round(v).toString() : v.toFixed(v >= 10 ? 1 : 2).replace(/\.?0+$/, "")) + unit;
}

export const fmtShare = (p: number) => (p === 0 ? "0%" : p < 0.1 ? "<0.1%" : `${p.toFixed(1)}%`);

/** Time left until `at`: "5d 6h", "1h 5m", "12m". */
export function fmtLeft(at: number, now = Date.now()) {
  const m = Math.max(0, Math.round((at - now) / 60_000));
  const [d, h, min] = [Math.floor(m / 1440), Math.floor((m % 1440) / 60), m % 60];
  return d ? `${d}d ${h}h` : h ? `${h}h ${min}m` : m ? `${min}m` : "now";
}

/**
 * Where this window's usage is heading: `projected` = % used by reset at the current rate.
 * Null when the window just started (too little to go on) or its length isn't known.
 */
export function pace(l: Limit, now = Date.now()) {
  if (l.resetsAt == null || !l.windowMinutes) return null;
  const len = l.windowMinutes * 60_000;
  const elapsed = 1 - (l.resetsAt - now) / len;
  if (elapsed < 0.05 || elapsed > 1) return null;
  const projected = l.used / elapsed;
  return { projected, over: projected > 100 };
}
