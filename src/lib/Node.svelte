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
  /* No overflow clip here: the slab's shadow and focus halo spill into the gutter */
  .cell {
    position: relative;
    min-width: 0;
    min-height: 0;
  }
  /* The gutter between slabs is the handle; a pill shows on hover */
  .divider {
    position: relative;
    z-index: 5;
    flex: none;
  }
  .row > .divider {
    width: var(--gap);
    cursor: col-resize;
  }
  .col > .divider {
    height: var(--gap);
    cursor: row-resize;
  }
  .divider::after {
    content: "";
    position: absolute;
    top: 50%;
    left: 50%;
    border-radius: 9999px;
    background: var(--gold);
    opacity: 0;
    transform: translate(-50%, -50%);
    transition: opacity var(--dur-quick) var(--ease-standard);
  }
  .row > .divider::after {
    width: 3px;
    height: 40px;
  }
  .col > .divider::after {
    width: 40px;
    height: 3px;
  }
  .divider:hover::after,
  .divider:active::after {
    opacity: 1;
  }
</style>
