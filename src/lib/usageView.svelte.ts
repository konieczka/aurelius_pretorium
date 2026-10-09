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

/** Always the longest range: the page cuts out the one shown, so switching ranges needs no new scan. */
export async function loadStats() {
  try {
    usage.rows = await invoke<Row[]>("usage_stats", { days: 90 });
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

/**
 * Shortly after launch, fetch everything once in the background: the first scan of the logs and the CLI limit checks
 * take a few seconds, and then the page has data the moment it opens (and the backend's caches are warm).
 */
export function warmUsage() {
  setTimeout(() => {
    if (usage.rows || usage.open) return; // already opened (and loading)
    loadStats();
    loadLimits();
  }, 1500);
}

/** While the view is open: refresh now (showing what we have meanwhile), keep it fresh and countdowns ticking. */
export function poll() {
  loadStats();
  loadLimits();
  usage.now = Date.now();
  const timers = [
    setInterval(loadStats, 60_000),
    setInterval(loadLimits, 120_000),
    setInterval(() => (usage.now = Date.now()), 30_000),
  ];
  return () => timers.forEach(clearInterval);
}
