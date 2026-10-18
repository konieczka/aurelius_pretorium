<script lang="ts">
  // A Redis tab: keys by pattern (SCAN), the picked key's value by its type, and a console below. The console runs
  // read commands only unless the tab allows writes.
  import { untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { Connected } from "./db";

  type Key = { key: string; kind: string; ttl: number };
  type Value = { kind: string; ttl: number; size: number; value: unknown };

  let { conn, write, version }: { conn: Connected; write: boolean; version: number } = $props();

  let db = $state(0);
  let pattern = $state("*");
  let keys = $state<Key[]>([]);
  let cursor = $state(0);
  let error = $state("");
  let picked = $state<string | null>(null);
  let value = $state<Value | null>(null);
  let valueError = $state("");
  let line = $state("");
  let log = $state<{ cmd: string; out: string; failed: boolean }[]>([]);

  async function scan(more = false) {
    error = "";
    try {
      const r = await invoke<{ cursor: number; keys: Key[] }>("db_keys", { id: conn.id, db, pattern, cursor: more ? cursor : 0 });
      keys = more ? [...keys, ...r.keys] : r.keys;
      cursor = r.cursor;
    } catch (e) {
      error = String(e);
    }
  }
  $effect(() => {
    void conn.id;
    void version;
    void db;
    untrack(() => {
      picked = null;
      value = null;
      scan();
    });
  });

  async function show(k: string) {
    picked = k;
    valueError = "";
    try {
      value = await invoke<Value>("db_value", { id: conn.id, db, key: k });
    } catch (e) {
      value = null;
      valueError = String(e);
    }
  }

  async function send() {
    const cmd = line.trim();
    if (!cmd) return;
    line = "";
    try {
      log = [...log, { cmd, out: await invoke<string>("db_command", { id: conn.id, db, line: cmd, write }), failed: false }];
      // It may have added, changed or removed keys.
      if (write) scan();
      if (picked) show(picked);
    } catch (e) {
      log = [...log, { cmd, out: String(e), failed: true }];
    }
  }

  const ttl = (ms: number) => (ms < 0 ? "" : ms < 120_000 ? `${Math.round(ms / 1000)}s` : ms < 7_200_000 ? `${Math.round(ms / 60_000)}m` : `${Math.round(ms / 3_600_000)}h`);
  const pairs = (v: unknown) => (v as [string, string | number][]) ?? [];
  const items = (v: unknown) => (v as string[]) ?? [];
  const entries = (v: unknown) => (v as [string, [string, string][]][]) ?? [];
</script>

<div class="redis">
  <div class="bar">
    <label>Database
      <select bind:value={db} aria-label="Database number">
        {#each conn.databases as d (d)}<option value={Number(d)}>{d}</option>{/each}
      </select>
    </label>
    <input aria-label="Key pattern" bind:value={pattern} onkeydown={(e) => e.key === "Enter" && scan()} placeholder="Pattern, e.g. user:*" />
    <button class="ghost" onclick={() => scan()}>Scan</button>
    <span class="count">{keys.length}{cursor ? "+" : ""} key{keys.length === 1 ? "" : "s"}</span>
  </div>
  {#if error}<p class="err" role="alert">{error}</p>{/if}
  <div class="body">
    <nav class="keys" aria-label="Keys">
      {#each keys as k (k.key)}
        <button class="key" class:on={picked === k.key} onclick={() => show(k.key)}>
          <span class="kind">{k.kind}</span><span class="kname">{k.key}</span>{#if k.ttl >= 0}<span class="ttl" title="Expires in">{ttl(k.ttl)}</span>{/if}
        </button>
      {:else}
        <p class="quiet">No keys match</p>
      {/each}
      {#if cursor}<button class="ghost more" onclick={() => scan(true)}>Load more</button>{/if}
    </nav>
    <section class="value" aria-label="Value">
      {#if valueError}
        <p class="err" role="alert">{valueError}</p>
      {:else if value && picked}
        <div class="vhead"><strong>{picked}</strong><span>{value.kind}</span><span>{value.size} {value.kind === "string" ? "bytes" : "items"}</span>{#if value.ttl >= 0}<span>expires in {ttl(value.ttl)}</span>{/if}</div>
        {#if value.kind === "string"}
          <pre class="selectable">{value.value}</pre>
        {:else if value.kind === "hash" || value.kind === "zset"}
          <table>
            <thead><tr><th scope="col">{value.kind === "hash" ? "Field" : "Member"}</th><th scope="col">{value.kind === "hash" ? "Value" : "Score"}</th></tr></thead>
            <tbody>{#each pairs(value.value) as [a, b], i (i)}<tr><td>{a}</td><td>{b}</td></tr>{/each}</tbody>
          </table>
        {:else if value.kind === "list" || value.kind === "set"}
          <ol class="list" start={value.kind === "list" ? 0 : 1} class:set={value.kind === "set"}>{#each items(value.value) as it, i (i)}<li>{it}</li>{/each}</ol>
        {:else if value.kind === "stream"}
          {#each entries(value.value) as [id, fields] (id)}<div class="entry"><code>{id}</code>{#each fields as [f, v], i (i)}<span>{f}={v}</span>{/each}</div>{/each}
        {:else}
          <pre>{JSON.stringify(value.value, null, 2)}</pre>
        {/if}
      {:else}
        <p class="quiet">Pick a key</p>
      {/if}
    </section>
  </div>
  <div class="console">
    <div class="log" role="log" aria-label="Console output">
      {#each log as l, i (i)}<div class="cmd">&gt; {l.cmd}</div><pre class:failed={l.failed}>{l.out}</pre>{/each}
    </div>
    <input aria-label="Redis command" bind:value={line} onkeydown={(e) => e.key === "Enter" && send()} placeholder={write ? "Command, e.g. SET greeting hi" : "Command, e.g. GET counter (read-only)"} />
  </div>
</div>

<style>
  .redis {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    font-size: 12.5px;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border-bottom: 1px solid var(--border);
  }
  .bar input {
    flex: 1;
    font-family: var(--font-mono);
  }
  .count {
    color: var(--muted-foreground);
  }
  .body {
    display: grid;
    grid-template-columns: 260px 1fr;
    flex: 1;
    min-height: 0;
  }
  .keys {
    overflow: auto;
    padding: 6px;
    border-right: 1px solid var(--border);
  }
  .key {
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
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .key:hover {
    background: color-mix(in oklch, var(--sidebar-accent) 60%, transparent);
  }
  .key.on {
    background: var(--accent);
  }
  .kind {
    flex: none;
    width: 44px;
    color: var(--muted-foreground);
    font-size: 10.5px;
    text-transform: uppercase;
  }
  .kname {
    flex: 1;
    overflow: hidden;
    font-family: var(--font-mono);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ttl {
    color: var(--muted-foreground);
    font-size: 11px;
  }
  .more {
    margin: 6px;
  }
  .value {
    overflow: auto;
    padding: 8px 12px;
  }
  .vhead {
    display: flex;
    gap: 10px;
    margin-bottom: 8px;
    color: var(--muted-foreground);
  }
  .vhead strong {
    color: var(--foreground);
    font-family: var(--font-mono);
  }
  pre {
    margin: 0;
    font-family: var(--font-mono);
    font-size: 12px;
    white-space: pre-wrap;
    word-break: break-word;
  }
  table {
    border-collapse: collapse;
  }
  th,
  td {
    padding: 3px 12px 3px 0;
    border-bottom: 1px solid var(--border);
    text-align: left;
    font-family: var(--font-mono);
  }
  .list {
    margin: 0;
    padding-left: 28px;
    font-family: var(--font-mono);
  }
  .list.set {
    list-style: disc;
  }
  .entry {
    display: flex;
    gap: 10px;
    padding: 2px 0;
    font-family: var(--font-mono);
  }
  .console {
    display: flex;
    flex-direction: column;
    height: 32%;
    min-height: 100px;
    border-top: 1px solid var(--border-strong);
  }
  .log {
    flex: 1;
    overflow: auto;
    padding: 6px 10px;
  }
  .cmd {
    color: var(--muted-foreground);
    font-family: var(--font-mono);
  }
  pre.failed {
    color: var(--diff-del-gutter);
  }
  .console input {
    margin: 6px 10px 8px;
    font-family: var(--font-mono);
  }
  .err {
    margin: 0;
    padding: 6px 10px;
    color: var(--diff-del-gutter);
  }
  .quiet {
    color: var(--muted-foreground);
  }
</style>
