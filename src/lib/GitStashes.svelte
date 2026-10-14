<script lang="ts">
  // The Git panel's Stashes section (only when there are some): click one to see its files, hover for Apply / Pop / Delete.
  import { ArchiveRestore, ChevronRight, PackageOpen, Trash2 } from "@lucide/svelte";
  import { app } from "./state.svelte";
  import { git, run } from "./GitPanel.svelte";
  import { parseStashes, STASH_FORMAT, type Stash } from "./gitOps";
  import { showStash, stashApply, stashDrop } from "./gitActions";

  let open = $state(true);
  let list = $state<Stash[]>([]);

  $effect(() => {
    void git.status;
    void app.gitVersion; // refetch after git operations
    if (!git.root) return void (list = []);
    let dead = false;
    run(["stash", "list", `--format=${STASH_FORMAT}`]).then(
      (o) => !dead && (list = parseStashes(o)),
      () => !dead && (list = []),
    );
    return () => (dead = true);
  });

  function ago(t: number) {
    const s = Date.now() / 1000 - t;
    for (const [n, u] of [[86400, "d"], [3600, "h"], [60, "m"]] as const) if (s >= n) return `${Math.floor(s / n)}${u} ago`;
    return "just now";
  }
</script>

{#if list.length}
  <div class="section">
    <button class="shead" onclick={() => (open = !open)}>
      <span class="chev" class:open><ChevronRight size={12} strokeWidth={1.5} /></span>
      <span class="stitle">Stashes</span>
      <span class="count">{list.length}</span>
    </button>
    {#if open}
      {#each list as st (st.hash)}
        <button class="row" title="{st.ref} · {new Date(st.time * 1000).toLocaleString()}\n{st.message}" onclick={() => showStash(st.hash, st.message)}>
          <span class="name">{st.message}</span>
          <span class="dir">{ago(st.time)}</span>
          <span class="acts">
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_interactive_supports_focus -->
            <span role="button" class="act" title="Apply (keep the stash)" onclick={(e) => (e.stopPropagation(), stashApply(st.ref, false))}><ArchiveRestore size={13} strokeWidth={1.5} /></span>
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_interactive_supports_focus -->
            <span role="button" class="act" title="Pop (apply and delete the stash)" onclick={(e) => (e.stopPropagation(), stashApply(st.ref, true))}><PackageOpen size={13} strokeWidth={1.5} /></span>
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_interactive_supports_focus -->
            <span role="button" class="act" title="Delete" onclick={(e) => (e.stopPropagation(), stashDrop(st.ref, st.message))}><Trash2 size={13} strokeWidth={1.5} /></span>
          </span>
        </button>
      {/each}
    {/if}
  </div>
{/if}

<style>
  .section {
    margin-top: 4px;
  }
  .shead,
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    height: 26px;
    padding: 0 6px 0 2px;
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
  .shead:hover,
  .row:hover {
    background: color-mix(in oklch, var(--sidebar-accent) 60%, transparent);
  }
  .shead {
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--muted-foreground);
  }
  .stitle {
    flex: 1;
  }
  .count {
    min-width: 18px;
    padding: 0 5px;
    border-radius: 9999px;
    background: var(--muted);
    font-size: 11px;
    text-align: center;
    letter-spacing: 0;
  }
  .chev {
    display: grid;
    place-items: center;
    width: 12px;
    transition: transform var(--dur-base) var(--ease-out-quint);
  }
  .chev.open {
    transform: rotate(90deg);
  }
  .row {
    padding-left: 20px;
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .dir {
    flex: none;
    color: var(--muted-foreground);
    font-size: 11.5px;
  }
  .acts {
    display: none;
    gap: 1px;
  }
  .row:hover .acts {
    display: flex;
  }
  .row:hover .dir {
    display: none;
  }
  .act {
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    border-radius: var(--radius-sm);
    color: var(--muted-foreground);
  }
  .act:hover {
    background: var(--accent);
    color: var(--foreground);
  }
</style>
