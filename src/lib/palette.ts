/** Colour palettes (Settings). Aurelius is the default: app.css :root / .dark; the rest are blocks in palettes.css.
 * Pure (no DOM / Tauri) so the fallback rules can be unit-tested: a workspace may carry its own palette,
 * and everywhere else the global palette applies. */
export const PALETTES = [
  { id: "aurelius", name: "Aurelius", blurb: "The house gold on warm parchment" },
  { id: "mare", name: "Mare Nostrum", blurb: "Lapis sea under lamp light" },
  { id: "tyrian", name: "Tyrian", blurb: "Imperial murex purple" },
  { id: "verdigris", name: "Verdigris", blurb: "Weathered bronze, copper accents" },
  { id: "marmor", name: "Marmor", blurb: "Cool marble, a lapis accent" },
  { id: "vesuvius", name: "Vesuvius", blurb: "Volcanic ash and lava" },
] as const;
export type Palette = (typeof PALETTES)[number]["id"];

export const DEFAULT_PALETTE: Palette = "aurelius";

export const isPalette = (value: unknown): value is Palette =>
  typeof value === "string" && PALETTES.some((p) => p.id === value);

/** A stored workspace override, or undefined when it is absent or malformed. */
export const paletteOverride = (value: unknown): Palette | undefined => (isPalette(value) ? value : undefined);

/** The palette to actually show: the workspace's own when it has one, else the global one, else Aurelius. */
export function resolvePalette(override: unknown, global: unknown): Palette {
  if (isPalette(override)) return override;
  if (isPalette(global)) return global;
  return DEFAULT_PALETTE;
}

export const paletteName = (id: Palette): string => PALETTES.find((p) => p.id === id)?.name ?? id;

/** How long a palette switch cross-fades for (theme.svelte.ts / palettes.css / Terminal keep to this). */
export const PALETTE_TRANSITION_MS = 380;

/** Should a change of the shown palette animate? Not the first palette a window shows (launch must not fade
 * in from the global default), not when the effective palette does not actually change, and not for a user
 * who prefers reduced motion. */
export function shouldAnimatePaletteChange(
  previous: unknown,
  next: unknown,
  opts: { first?: boolean; reducedMotion?: boolean } = {},
): boolean {
  if (opts.first || opts.reducedMotion) return false;
  return resolvePalette(previous, DEFAULT_PALETTE) !== resolvePalette(next, DEFAULT_PALETTE);
}
