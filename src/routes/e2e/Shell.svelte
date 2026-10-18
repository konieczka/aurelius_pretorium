<script lang="ts">
  // The app's sidebar Servers panel and its panes, on the E2E fixture project (VITE_E2E_WORKSPACE).
  import "../../app.css";
  import * as L from "$lib/layout";
  import { app } from "$lib/state.svelte";
  import Infra, { watchInfra } from "$lib/Infra.svelte";
  import Node from "$lib/Node.svelte";
  import QuickPick from "$lib/QuickPick.svelte";

  app.workspace = import.meta.env.VITE_E2E_WORKSPACE ?? "";
  app.recent = [app.workspace];
  app.sidebar = "infra";
  app.root = L.rootWith(L.pane([]));
  $effect(watchInfra);
</script>

<div class="e2e">
  <aside class="glass" aria-label="Servers"><Infra /></aside>
  <main><Node bind:node={app.root} /></main>
</div>
<QuickPick />

<style>
  .e2e {
    display: grid;
    grid-template-columns: 300px 1fr;
    gap: 8px;
    height: 100vh;
    padding: 8px;
    box-sizing: border-box;
  }
  aside {
    overflow: auto;
    padding: 4px 8px;
    border-radius: var(--r-island);
  }
  main {
    display: flex;
    min-width: 0;
    min-height: 0;
  }
</style>
