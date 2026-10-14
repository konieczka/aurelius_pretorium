<script lang="ts">
  import type { Provider } from "./usageStats";

  // Simplified marks for the AI CLIs, drawn in currentColor (Claude's in its clay, like its own UI).
  let { provider, size = 16 }: { provider: Provider; size?: number } = $props();
  // Claude's starburst: 12 rays of alternating length.
  const rays = Array.from({ length: 12 }, (_, i) => {
    const a = (i * Math.PI) / 6 + 0.12;
    const r = i % 2 ? 6.2 : 7.4;
    return { x: 8 + Math.cos(a) * r, y: 8 + Math.sin(a) * r };
  });
</script>

<svg class="pi {provider}" width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-linecap="round" aria-hidden="true">
  {#if provider === "claude"}
    {#each rays as r}<line x1="8" y1="8" x2={r.x} y2={r.y} stroke-width="1.6" />{/each}
  {:else if provider === "codex"}
    <!-- OpenAI's knot, as six interlaced petals -->
    {#each [0, 60, 120] as a}<ellipse cx="8" cy="8" rx="2.9" ry="6.6" transform="rotate({a} 8 8)" stroke-width="1.1" />{/each}
  {:else if provider === "grok"}
    <!-- xAI's slashed ring -->
    <path d="M12.4 4.6 A5.6 5.6 0 1 1 6 2.7" stroke-width="1.4" />
    <line x1="2.2" y1="13.8" x2="14" y2="2" stroke-width="1.4" />
  {:else if provider === "antigravity"}
    <!-- Antigravity's arch -->
    <path d="M2.5 13.5 C4.5 13.5 5.5 2.5 8 2.5 S11.5 13.5 13.5 13.5" stroke-width="1.6" />
  {:else if provider === "muse"}
    <!-- Muse Code's M, as one stroke -->
    <path d="M2.5 13 V3.5 L8 10 L13.5 3.5 V13" stroke-width="1.5" stroke-linejoin="round" />
  {:else}
    <!-- OpenCode's block -->
    <rect x="3" y="2.5" width="10" height="11" rx="1" stroke-width="1.4" />
    <rect x="6" y="6" width="4" height="4.5" fill="currentColor" stroke="none" />
  {/if}
</svg>

<style>
  .pi {
    flex: none;
  }
  .claude {
    color: var(--usage-claude);
  }
</style>
