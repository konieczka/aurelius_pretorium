<script lang="ts">
  // Rows of a table page or a query result. A click picks a cell (its whole value shows below); a double-click edits
  // it where the tab allows (`editable`), else says why not (`locked`).
  import { ArrowDown, ArrowUp, Copy } from "@lucide/svelte";
  import { pretty, type Grid } from "./db";

  let {
    grid,
    sort = null,
    desc = false,
    onsort,
    editable = false,
    locked = "",
    onedit,
  }: {
    grid: Grid;
    sort?: string | null;
    desc?: boolean;
    onsort?: (column: string) => void;
    editable?: boolean;
    locked?: string; // why cells can't be edited
    onedit?: (row: number, column: string, value: string | null) => void;
  } = $props();

  let picked = $state<{ r: number; c: number } | null>(null);
  let editing = $state<{ r: number; c: number; value: string } | null>(null);
  let notice = $state("");
  $effect(() => {
    void grid;
    picked = editing = null; // another page or result
    notice = "";
  });

  const short = (v: string) => (v.length > 160 ? v.slice(0, 160) + "…" : v);
  const value = $derived(picked ? grid.rows[picked.r]?.[picked.c] : undefined);

  function edit(r: number, c: number) {
    if (!editable) return void (notice = locked);
    editing = { r, c, value: grid.rows[r][c] ?? "" };
  }
  function commit() {
    if (!editing) return;
    const { r, c, value } = editing;
    editing = null;
    if (value !== (grid.rows[r][c] ?? "")) onedit?.(r, grid.columns[c].name, value);
  }
  const focus = (el: HTMLInputElement) => (el.focus(), el.select());
</script>

<div class="grid">
  <div class="scroll">
    <table>
      <thead>
        <tr>
          {#each grid.columns as col (col.name)}
            <th scope="col" aria-sort={sort === col.name ? (desc ? "descending" : "ascending") : undefined}>
              <button class="head" disabled={!onsort} onclick={() => onsort?.(col.name)} title={onsort ? `Sort by ${col.name}` : col.name}>
                <span class="cname">{col.name}</span>
                {#if col.kind}<span class="kind">{col.kind}</span>{/if}
                {#if sort === col.name}{#if desc}<ArrowDown size={11} />{:else}<ArrowUp size={11} />{/if}{/if}
              </button>
            </th>
          {/each}
        </tr>
      </thead>
      <tbody>
        {#each grid.rows as row, r (r)}
          <tr>
            {#each row as v, c (c)}
              <td
                class:null={v === null}
                class:picked={picked?.r === r && picked?.c === c}
                onclick={() => (picked = { r, c })}
                ondblclick={() => edit(r, c)}
              >
                {#if editing?.r === r && editing?.c === c}
                  <input
                    aria-label="New value for {grid.columns[c].name}"
                    bind:value={editing.value}
                    use:focus
                    onkeydown={(e) => {
                      if (e.key === "Enter") commit();
                      else if (e.key === "Escape") editing = null;
                    }}
                    onblur={() => (editing = null)}
                  />
                {:else}{v === null ? "NULL" : short(v)}{/if}
              </td>
            {/each}
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
  {#if notice}<p class="notice" role="status">{notice}</p>{/if}
  {#if picked && value !== undefined}
    <div class="detail" role="region" aria-label="Cell value">
      <div class="dhead">
        <span class="dname">{grid.columns[picked.c]?.name}</span>
        {#if editable && value !== null}
          <button class="ghost" onclick={() => picked && onedit?.(picked.r, grid.columns[picked.c].name, null)}>Set to NULL</button>
        {/if}
        <button class="icon-btn" title="Copy" aria-label="Copy value" onclick={() => navigator.clipboard?.writeText(value ?? "")}><Copy size={12} /></button>
      </div>
      <pre class="selectable">{value === null ? "NULL" : pretty(value)}</pre>
    </div>
  {/if}
</div>

<style>
  .grid {
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1;
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
  table {
    border-collapse: separate;
    border-spacing: 0;
    font-size: 12.5px;
    font-variant-numeric: tabular-nums;
  }
  th {
    position: sticky;
    top: 0;
    z-index: 1;
    padding: 0;
    background: var(--card);
    border-bottom: 1px solid var(--border-strong);
    text-align: left;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 5px;
    width: 100%;
    padding: 5px 10px;
    border: 0;
    background: none;
    color: var(--foreground);
    font: inherit;
    font-weight: 600;
    white-space: nowrap;
    cursor: pointer;
  }
  .head:disabled {
    cursor: default;
  }
  .kind {
    color: var(--muted-foreground);
    font-size: 10.5px;
    font-weight: 400;
  }
  td {
    max-width: 320px;
    padding: 4px 10px;
    overflow: hidden;
    border-bottom: 1px solid var(--border);
    white-space: nowrap;
    text-overflow: ellipsis;
    cursor: default;
  }
  tr:hover td {
    background: color-mix(in oklch, var(--accent) 50%, transparent);
  }
  td.null {
    color: var(--muted-foreground);
    font-style: italic;
  }
  td.picked {
    outline: 1.5px solid var(--gold-ring);
    outline-offset: -1.5px;
  }
  td input {
    width: 100%;
    min-width: 120px;
    font: inherit;
  }
  .notice {
    margin: 0;
    padding: 6px 10px;
    border-top: 1px solid var(--border);
    color: var(--muted-foreground);
    font-size: 12px;
  }
  .detail {
    max-height: 40%;
    display: flex;
    flex-direction: column;
    border-top: 1px solid var(--border-strong);
  }
  .dhead {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
    font-size: 12px;
  }
  .dname {
    flex: 1;
    font-weight: 600;
  }
  pre {
    margin: 0;
    padding: 6px 10px 10px;
    overflow: auto;
    font-family: var(--font-mono);
    font-size: 12px;
    white-space: pre-wrap;
    word-break: break-word;
  }
</style>
