<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import FileTree from "./FileTree.svelte";
  import { app, openFile } from "./state.svelte";

  type Entry = { name: string; is_dir: boolean };
  let { path, depth = 0 }: { path: string; depth?: number } = $props();
  let entries = $state<Entry[]>([]);
  let open = $state<Record<string, boolean>>({});

  $effect(() => {
    void app.treeVersion; // refresh button bumps this
    invoke<Entry[]>("list_dir", { path }).then((e) => (entries = e), () => (entries = []));
  });
</script>

{#each entries as e (e.name)}
  {@const full = `${path}/${e.name}`}
  <button
    class="row"
    style="padding-left: {8 + depth * 12}px"
    title={full}
    onclick={() => (e.is_dir ? (open[e.name] = !open[e.name]) : openFile(full))}
  >
    <span class="icon">{e.is_dir ? (open[e.name] ? "▾" : "▸") : ""}</span>{e.name}
  </button>
  {#if e.is_dir && open[e.name]}
    <FileTree path={full} depth={depth + 1} />
  {/if}
{/each}

<style>
  .row {
    display: flex;
    width: 100%;
    padding: 2px 8px;
    border: 0;
    background: none;
    color: #abb2bf;
    font-size: 12.5px;
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
  }
  .row:hover {
    background: #2c313a;
  }
  .icon {
    display: inline-block;
    width: 14px;
    flex: none;
    color: #5c6370;
  }
</style>
