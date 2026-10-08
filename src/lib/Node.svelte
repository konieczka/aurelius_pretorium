<script lang="ts">
  import type { LNode } from "./layout";
  import Node from "./Node.svelte";
  import Pane from "./Pane.svelte";

  let { node = $bindable() }: { node: LNode } = $props();
  // Normalize: flex-grow weights summing below 1 would leave space unfilled.
  const total = $derived(node.type === "split" ? node.sizes.reduce((x, y) => x + y, 0) : 1);

  function drag(ev: PointerEvent, i: number) {
    if (node.type !== "split") return;
    const s = node;
    const handle = ev.currentTarget as HTMLElement;
    handle.setPointerCapture(ev.pointerId);
    const horiz = s.dir === "row";
    const start = horiz ? ev.clientX : ev.clientY;
    const [a, b] = [s.sizes[i], s.sizes[i + 1]];
    const total = s.sizes.reduce((x, y) => x + y, 0);
    const box = handle.parentElement!;
    const px = (horiz ? box.clientWidth : box.clientHeight) / total; // px per size unit
    const move = (e: PointerEvent) => {
      const d = ((horiz ? e.clientX : e.clientY) - start) / px;
      const min = 0.05 * total;
      const na = Math.min(Math.max(a + d, min), a + b - min);
      s.sizes[i] = na;
      s.sizes[i + 1] = a + b - na;
    };
    const up = () => {
      handle.removeEventListener("pointermove", move);
      handle.removeEventListener("pointerup", up);
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", up);
  }
</script>

{#if node.type === "pane"}
  <Pane bind:pane={node} />
{:else}
  <div class="split {node.dir}">
    {#each node.children as child, i (child.id)}
      {#if i > 0}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="divider" onpointerdown={(e) => drag(e, i - 1)}></div>
      {/if}
      <div class="cell" style="flex: {node.sizes[i] / total} 1 0">
        <Node bind:node={node.children[i]} />
      </div>
    {/each}
  </div>
{/if}

<style>
  .split {
    display: flex;
    width: 100%;
    height: 100%;
  }
  .col {
    flex-direction: column;
  }
  .cell {
    position: relative;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }
  .divider {
    flex: none;
    background: #181a1f;
  }
  .row > .divider {
    width: 4px;
    cursor: col-resize;
  }
  .col > .divider {
    height: 4px;
    cursor: row-resize;
  }
  .divider:hover {
    background: #3d4a63;
  }
</style>
