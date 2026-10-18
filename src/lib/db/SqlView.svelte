<script lang="ts">
  // A SQL database's tab (Postgres, MySQL/MariaDB, SQLite), and Mongo's: its tables on the left; on the right a table's
  // rows, a page at a time (Data), or the query editor (Query). Mongo has no query editor: its filter is JSON.
  import { untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { EditorView, basicSetup } from "codemirror";
  import { keymap } from "@codemirror/view";
  import { Compartment } from "@codemirror/state";
  import { sql, PostgreSQL, MySQL, SQLite, type SQLNamespace } from "@codemirror/lang-sql";
  import { Bot, ChevronLeft, ChevronRight, Play, Square, Table2, Eye } from "@lucide/svelte";
  import { aurelius } from "../Editor.svelte";
  import { agent, askAgent } from "../GitPanel.svelte";
  import { prompt } from "../QuickPick.svelte";
  import type { DbTarget } from "../layout";
  import Grid from "./Grid.svelte";
  import { engineName, range, type Completion, type Connected, type Grid as G, type Table } from "./db";

  let { conn, target, write, version }: { conn: Connected; target: DbTarget; write: boolean; version: number } = $props();

  const PAGE = 100;
  const mongo = $derived(conn.engine === "mongo");
  let tables = $state<Table[]>([]);
  let treeError = $state("");
  let current = $state<Table | null>(null);
  let mode = $state<"data" | "query">("data");

  // Data: a page of the current table.
  let page = $state<G | null>(null);
  let offset = $state(0);
  let sort = $state<string | null>(null);
  let desc = $state(false);
  let filter = $state("");
  let applied = $state(""); // the filter the page shows
  let dataError = $state("");
  let loading = $state(false);
  let pending = $state<{ sql: string; edit: unknown } | null>(null); // an edit waiting to be confirmed
  let editError = $state("");

  // Query: the editor and its last result.
  let result = $state<G | null>(null);
  let queryError = $state("");
  let running = $state(false);
  let history = $state<string[]>([]);

  const grouped = $derived.by(() => {
    const m = new Map<string, Table[]>();
    for (const t of tables) m.set(t.schema, [...(m.get(t.schema) ?? []), t]);
    return [...m];
  });

  async function loadTree() {
    treeError = "";
    try {
      tables = await invoke<Table[]>("db_tree", { id: conn.id, database: mongo ? conn.database : null });
    } catch (e) {
      treeError = String(e);
    }
  }
  // A new connection (another database), or Refresh.
  $effect(() => {
    void conn.id;
    void version;
    untrack(() => {
      loadTree();
      if (current) loadPage();
    });
  });

  function open(t: Table) {
    current = t;
    mode = "data";
    offset = 0;
    sort = null;
    desc = false;
    filter = applied = "";
    loadPage();
  }

  let loads = 0;
  async function loadPage() {
    if (!current) return;
    const seq = ++loads;
    loading = true;
    dataError = editError = "";
    pending = null;
    try {
      const g = await invoke<G>("db_rows", {
        id: conn.id,
        browse: { schema: current.schema, table: current.name, offset, limit: PAGE, sort, desc, filter: applied || null },
      });
      if (seq === loads) page = g;
    } catch (e) {
      if (seq === loads) (dataError = String(e)), (page = null);
    }
    if (seq === loads) loading = false;
  }

  function sortBy(column: string) {
    if (sort === column) desc = !desc;
    else (sort = column), (desc = false);
    offset = 0;
    loadPage();
  }
  function applyFilter() {
    applied = filter.trim();
    offset = 0;
    loadPage();
  }
  const turn = (by: number) => ((offset = Math.max(0, offset + by * PAGE)), loadPage());

  // Editing a cell: the UPDATE is shown, and runs once confirmed.
  const locked = $derived(
    mongo ? "Documents are read-only here" :
    !write ? "Allow writes (in the bar above) to edit cells" :
    current?.kind === "view" ? "A view's rows can't be edited" :
    !page?.key.length ? "This table has no primary key: its rows can't be edited one by one" :
    "",
  );
  async function edit(r: number, column: string, value: string | null) {
    if (!page || !current) return;
    const row = page.rows[r];
    const key = page.key.map((k) => [k, row[page!.columns.findIndex((c) => c.name === k)]]);
    const e = { schema: current.schema, table: current.name, key, column, value };
    editError = "";
    try {
      const preview = await invoke<{ sql: string }>("db_edit", { id: conn.id, edit: e, apply: false });
      pending = { sql: preview.sql, edit: e };
    } catch (err) {
      editError = String(err);
    }
  }
  async function confirmEdit() {
    if (!pending) return;
    try {
      await invoke("db_edit", { id: conn.id, edit: pending.edit, apply: true });
      pending = null;
      loadPage();
    } catch (err) {
      editError = String(err);
      pending = null;
    }
  }

  // The query editor, with completion from the database's tables.
  let host = $state<HTMLDivElement>();
  let view: EditorView | undefined;
  const language = new Compartment();
  const dialect = $derived(conn.engine === "mysql" ? MySQL : conn.engine === "sqlite" ? SQLite : PostgreSQL);
  $effect(() => {
    if (!host || mongo) return;
    const v = (view = new EditorView({
      parent: host,
      doc: "",
      extensions: [
        keymap.of([{ key: "Mod-Enter", run: () => (run(), true) }]),
        basicSetup,
        aurelius,
        EditorView.lineWrapping,
        untrack(() => language.of(sql({ dialect }))),
        EditorView.contentAttributes.of({ "aria-label": "SQL query" }),
      ],
    }));
    return () => (v.destroy(), (view = undefined));
  });
  $effect(() => {
    void conn.id;
    if (mongo) return;
    invoke<Completion[]>("db_completion", { id: conn.id })
      .then((list) => {
        const schema: SQLNamespace = {};
        for (const c of list) schema[conn.engine === "sqlite" || c.schema === "public" ? c.table : `${c.schema}.${c.table}`] = c.columns;
        view?.dispatch({ effects: language.reconfigure(sql({ dialect, schema })) });
      })
      .catch(() => {});
  });

  async function run() {
    const text = view?.state.doc.toString().trim() ?? "";
    if (!text || running) return;
    running = true;
    queryError = "";
    try {
      result = await invoke<G>("db_query", { id: conn.id, sql: text, write });
    } catch (e) {
      queryError = String(e);
      result = null;
    }
    running = false;
    history = [text, ...history.filter((h) => h !== text)].slice(0, 20);
    if (result?.affected != null && current) loadPage(); // it may have changed the rows on show
  }
  const cancel = () => invoke("db_cancel", { id: conn.id }).catch(() => {});
  function recall(q: string) {
    view?.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: q } });
    view?.focus();
  }
  function queryTable(t: Table) {
    mode = "query";
    const name = conn.engine === "mysql" ? `\`${t.schema}\`.\`${t.name}\`` : conn.engine === "sqlite" || t.schema === "public" ? `"${t.name}"` : `"${t.schema}"."${t.name}"`;
    queueMicrotask(() => recall(`SELECT * FROM ${name} LIMIT 100`));
  }

  /** Hand the question to the coding agent, with what it needs to write the query: the tables and their columns. */
  async function ask() {
    const q = await prompt(`Ask ${agent.cmd} about this database`);
    if (!q?.trim()) return;
    const cols = await invoke<Completion[]>("db_completion", { id: conn.id }).catch(() => [] as Completion[]);
    const schema = cols.map((c) => `- ${c.schema}.${c.table} (${c.columns.join(", ")})`).join("\n");
    askAgent(
      target.root,
      `${q.trim()}\n\nContext: a ${engineName[conn.engine]} database on this machine (${conn.route}, database ${conn.database}). ` +
        `Its tables:\n${schema}\n\nWrite the SQL for it; I'll run it in Pretorium's query editor.`,
    );
  }
</script>

<div class="sql">
  <nav class="tree" aria-label="Tables">
    {#if treeError}<p class="err">{treeError}</p>{/if}
    {#each grouped as [schema, list] (schema)}
      <div class="schema">{schema}</div>
      {#each list as t (t.schema + "." + t.name)}
        <button class="tbl" class:on={current === t || (current?.schema === t.schema && current?.name === t.name)} onclick={() => open(t)} oncontextmenu={(e) => (e.preventDefault(), !mongo && queryTable(t))} title="{t.schema}.{t.name}">
          {#if t.kind === "view"}<Eye size={13} strokeWidth={1.5} />{:else}<Table2 size={13} strokeWidth={1.5} />{/if}
          <span class="tname">{t.name}</span>
          {#if t.rows !== null}<span class="n">{t.rows}</span>{/if}
        </button>
      {/each}
    {:else}
      {#if !treeError}<p class="quiet">No tables</p>{/if}
    {/each}
  </nav>

  <section class="main">
    {#if !mongo}
      <div class="modes" role="tablist">
        <button role="tab" aria-selected={mode === "data"} class:on={mode === "data"} onclick={() => (mode = "data")}>Data</button>
        <button role="tab" aria-selected={mode === "query"} class:on={mode === "query"} onclick={() => (mode = "query")}>Query</button>
      </div>
    {/if}

    <div class="pane" hidden={mode !== "data"}>
      {#if current}
        <div class="bar">
          <strong class="title">{current.schema}.{current.name}</strong>
          <input
            class="filter"
            aria-label="Filter"
            placeholder={mongo ? 'Filter: { "field": "value" }' : "WHERE …"}
            bind:value={filter}
            onkeydown={(e) => e.key === "Enter" && applyFilter()}
          />
          <span class="count" aria-live="polite">{page ? range(offset, page.rows.length, page.total) : ""}</span>
          <button class="icon-btn" aria-label="Previous page" disabled={!offset || loading} onclick={() => turn(-1)}><ChevronLeft size={14} /></button>
          <button class="icon-btn" aria-label="Next page" disabled={!page || loading || offset + page.rows.length >= (page.total ?? Infinity) || page.rows.length < PAGE} onclick={() => turn(1)}><ChevronRight size={14} /></button>
        </div>
        {#if pending}
          <div class="confirm" role="alertdialog" aria-label="Confirm edit">
            <span>Apply this change?</span>
            <code class="selectable">{pending.sql}</code>
            <button class="primary" onclick={confirmEdit}>Apply</button>
            <button class="ghost" onclick={() => (pending = null)}>Cancel</button>
          </div>
        {/if}
        {#if editError}<p class="err" role="alert">{editError}</p>{/if}
        {#if dataError}
          <p class="err" role="alert">{dataError}</p>
        {:else if page}
          <Grid grid={page} {sort} {desc} onsort={sortBy} editable={!locked} {locked} onedit={edit} />
        {/if}
      {:else}
        <p class="quiet center">Pick a {mongo ? "collection" : "table"} on the left{mongo ? "" : ", or write a query"}.</p>
      {/if}
    </div>

    {#if !mongo}
      <div class="pane" hidden={mode !== "query"}>
        <div class="editor" bind:this={host}></div>
        <div class="bar">
          <button class="primary" onclick={run} disabled={running} title="Run (⌘↵)"><Play size={12} />Run</button>
          {#if running && conn.engine === "postgres"}<button class="ghost" onclick={cancel}><Square size={11} />Cancel</button>{/if}
          {#if history.length}
            <select class="history" aria-label="History" onchange={(e) => (recall(e.currentTarget.value), (e.currentTarget.selectedIndex = 0))}>
              <option value="">History</option>
              {#each history as h (h)}<option value={h}>{h.length > 60 ? h.slice(0, 60) + "…" : h}</option>{/each}
            </select>
          {/if}
          <span class="spacer"></span>
          <button class="ghost" onclick={ask} title="Ask the coding agent to write a query: it gets this database's tables"><Bot size={13} strokeWidth={1.5} />Ask {agent.cmd}</button>
        </div>
        {#if queryError}
          <pre class="err" role="alert">{queryError}</pre>
        {:else if result}
          {#if result.columns.length}
            <p class="meta" role="status">{result.rows.length}{result.truncated ? "+" : ""} row{result.rows.length === 1 ? "" : "s"} · {result.ms} ms{result.truncated ? " (the first ones)" : ""}</p>
            <Grid grid={result} locked="Edit rows from a table's Data view" />
          {:else}
            <p class="meta" role="status">{result.affected ?? 0} row{result.affected === 1 ? "" : "s"} changed · {result.ms} ms</p>
          {/if}
        {/if}
      </div>
    {/if}
  </section>
</div>

<style>
  .sql {
    display: grid;
    grid-template-columns: 220px 1fr;
    height: 100%;
    min-height: 0;
  }
  .tree {
    overflow: auto;
    padding: 6px 6px 12px;
    border-right: 1px solid var(--border);
  }
  .schema {
    padding: 8px 6px 2px;
    color: var(--muted-foreground);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
  .tbl {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    height: 26px;
    padding: 0 6px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--foreground);
    font-size: 13px;
    text-align: left;
    cursor: pointer;
  }
  .tbl:hover {
    background: color-mix(in oklch, var(--sidebar-accent) 60%, transparent);
  }
  .tbl.on {
    background: var(--accent);
  }
  .tname {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .n {
    color: var(--muted-foreground);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }
  .main {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  .modes {
    display: flex;
    gap: 2px;
    padding: 6px 10px 0;
  }
  .modes button {
    padding: 4px 10px;
    border: 0;
    border-bottom: 2px solid transparent;
    background: none;
    color: var(--muted-foreground);
    font: inherit;
    font-size: 12.5px;
    cursor: pointer;
  }
  .modes button.on {
    border-bottom-color: var(--gold);
    color: var(--foreground);
  }
  .pane {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
  }
  .pane[hidden] {
    display: none;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 10px;
    border-bottom: 1px solid var(--border);
    font-size: 12.5px;
  }
  .title {
    white-space: nowrap;
  }
  .filter {
    flex: 1;
    min-width: 80px;
    font-family: var(--font-mono);
    font-size: 12px;
  }
  .count {
    color: var(--muted-foreground);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .spacer {
    flex: 1;
  }
  .history {
    max-width: 240px;
  }
  .editor {
    height: 34%;
    min-height: 90px;
    border-bottom: 1px solid var(--border);
    overflow: hidden;
  }
  .confirm {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    border-bottom: 1px solid var(--border);
    border-left: 2px solid var(--gold);
    background: var(--card);
    font-size: 12.5px;
  }
  .confirm code {
    flex: 1 1 100%;
    order: 1;
    font-size: 12px;
    white-space: pre-wrap;
  }
  .confirm span {
    flex: 1;
  }
  .meta {
    margin: 0;
    padding: 6px 10px;
    color: var(--muted-foreground);
    font-size: 12px;
  }
  .err {
    margin: 0;
    padding: 8px 10px;
    color: var(--diff-del-gutter);
    font-size: 12.5px;
    white-space: pre-wrap;
  }
  .quiet {
    padding: 8px 6px;
    color: var(--muted-foreground);
    font-size: 12.5px;
  }
  .center {
    margin: auto;
  }
</style>
