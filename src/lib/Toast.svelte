<script module lang="ts">
  // A short note that something happened (a link copied), shown where it happened and gone on its own.
  // One instance, rendered by the app shell; a new toast replaces the one showing.

  const shown = $state({ text: "", x: 0, y: 0, key: 0 });
  let timer: ReturnType<typeof setTimeout> | undefined;

  /** Show `text` just above x, y (CSS px in the window) for a moment. */
  export function toast(text: string, x: number, y: number) {
    clearTimeout(timer);
    Object.assign(shown, { text, x, y, key: shown.key + 1 });
    timer = setTimeout(() => (shown.text = ""), 1400);
  }
</script>

<script lang="ts">
  import { Check } from "@lucide/svelte";
</script>

{#if shown.text}
  {#key shown.key}
    <!-- Kept inside the window: a click near an edge would push it off -->
    <div class="toast glass-float" role="status" style="left: clamp(80px, {shown.x}px, calc(100vw - 80px)); top: max(44px, {shown.y}px)">
      <Check size={13} strokeWidth={2} />{shown.text}
    </div>
  {/key}
{/if}

<style>
  .toast {
    position: fixed;
    z-index: 110;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 5px 11px;
    border-radius: var(--r-control);
    color: var(--foreground);
    font-size: 12px;
    white-space: nowrap;
    pointer-events: none;
    transform: translate(-50%, calc(-100% - 10px));
    animation:
      rise var(--dur-base) var(--ease-out-quint),
      fade var(--dur-base) var(--ease-standard) 1180ms forwards;
  }
  .toast :global(svg) {
    color: var(--gold);
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translate(-50%, calc(-100% - 4px)) scale(0.96);
    }
  }
  @keyframes fade {
    to {
      opacity: 0;
    }
  }
</style>
