<script lang="ts">
  // The Fleet sidebar tab's mark: a Roman galley (ram at the bow, curled stern post, square sail, oars), drawn like a
  // Lucide icon: 24-unit grid, stroked in currentColor.
  let { size = 16, strokeWidth = 1.5 }: { size?: number; strokeWidth?: number } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width={strokeWidth} stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
  <path d="M2.6 13.5H20" />
  <path d="M2.6 13.5c.3 1.8 1 3.1 1.9 4h10.8c2.3 0 3.9-1.3 4.7-4" />
  <path d="M4.5 17.5H1.8" />
  <path d="M20 13.5c1.4-.9 2-2.5 1.6-3.9-.3-1.1-1.5-1.4-2.1-.6-.4.6 0 1.4.7 1.3" />
  <path d="M10.5 11v2.5M10.5 4.5V2.5" />
  <path d="M7 4.5h7" />
  <path d="M7 4.5 6 11c3-1 6-1 9 0l-1-6.5" />
  <path d="M7.3 17.5l-1.2 3M10.6 17.5l-1.2 3M13.9 17.5l-1.2 3" />
</svg>
