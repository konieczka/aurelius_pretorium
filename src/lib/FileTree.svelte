<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { ChevronRight, File, Folder, FolderOpen } from "@lucide/svelte";
  import FileTree from "./FileTree.svelte";
  import { activeTab, app, openFile } from "./state.svelte";

  type Entry = { name: string; is_dir: boolean };
  let { path, depth = 0 }: { path: string; depth?: number } = $props();
  let entries = $state<Entry[]>([]);
  let open = $state<Record<string, boolean>>({});
  const current = $derived.by(() => {
    const t = activeTab();
    return t?.kind === "file" ? t.path : null;
  });

  $effect(() => {
    void app.treeVersion; // refresh button bumps this
    invoke<Entry[]>("list_dir", { path }).then((e) => (entries = e), () => (entries = []));
  });
</script>

<div class="level" class:nested={depth > 0}>
  {#each entries as e (e.name)}
    {@const full = `${path}/${e.name}`}
    <button
      class="row"
      class:active={full === current}
      title={full}
      onclick={() => (e.is_dir ? (open[e.name] = !open[e.name]) : openFile(full))}
    >
      <span class="chev" class:open={open[e.name]}>
        {#if e.is_dir}<ChevronRight size={12} strokeWidth={1.5} />{/if}
      </span>
      {#if e.is_dir}
        {#if open[e.name]}<FolderOpen size={14} strokeWidth={1.5} />{:else}<Folder size={14} strokeWidth={1.5} />{/if}
      {:else}
        <File size={14} strokeWidth={1.5} />
      {/if}
      <span class="name">{e.name}</span>
    </button>
    {#if e.is_dir && open[e.name]}
      <FileTree path={full} depth={depth + 1} />
    {/if}
  {/each}
</div>

<style>
  .level.nested {
    margin-left: 13px;
    padding-left: 4px;
    border-left: 1px solid var(--sidebar-border);
  }
  .row {
    position: relative;
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    height: 26px;
    padding: 0 8px 0 2px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--sidebar-foreground);
    font-size: 13px;
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
    transition: background-color var(--dur-quick) var(--ease-standard);
  }
  .row:hover {
    background: color-mix(in oklch, var(--sidebar-accent) 60%, transparent);
  }
  .row.active {
    background: var(--sidebar-accent);
  }
  .row.active::before {
    content: "";
    position: absolute;
    left: 0;
    top: 6px;
    bottom: 6px;
    width: 2px;
    border-radius: 9999px;
    background: var(--gold);
  }
  .row :global(svg) {
    flex: none;
    color: var(--muted-foreground);
  }
  .chev {
    display: grid;
    place-items: center;
    width: 12px;
    flex: none;
    transition: transform var(--dur-base) var(--ease-out-quint);
  }
  .chev.open {
    transform: rotate(90deg);
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
