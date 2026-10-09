import { invoke } from "@tauri-apps/api/core";
import type { Limit, Provider, Row } from "./usageStats";

// The Usage view stands in for this window's panes while open (they stay mounted underneath). Per window, not saved.
export const usage = $state({
  open: false,
  days: 7 as 7 | 30 | 90,
  breakdown: "model" as "model" | "day",
  rows: null as Row[] | null,
  limits: null as Limit[] | null,
  errors: [] as [Provider, string][],
  statsError: "",
  loadingLimits: false,
  now: Date.now(),
});

export const toggleUsage = () => (usage.open = !usage.open);
export const closeUsage = () => (usage.open = false);

export async function loadStats() {
  try {
    usage.rows = await invoke<Row[]>("usage_stats", { days: usage.days });
    usage.statsError = "";
  } catch (e) {
    usage.statsError = String(e);
  }
}

/** `force` skips the backend's one-minute cache of Claude's limits (Retry). */
export async function loadLimits(force = false) {
  usage.loadingLimits = true;
  try {
    const r = await invoke<{ limits: Limit[]; errors: [Provider, string][] }>("usage_limits", { force });
    usage.limits = r.limits;
    usage.errors = r.errors;
  } catch (e) {
    usage.errors = [["claude", String(e)], ["codex", String(e)]];
  } finally {
    usage.loadingLimits = false;
  }
}

/** While the view is open: keep stats (cheap, incremental) and limits fresh and countdowns ticking. */
export function poll() {
  loadLimits();
  usage.now = Date.now();
  const timers = [
    setInterval(loadStats, 60_000),
    setInterval(loadLimits, 120_000),
    setInterval(() => (usage.now = Date.now()), 30_000),
  ];
  return () => timers.forEach(clearInterval);
}
