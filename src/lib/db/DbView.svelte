<script lang="ts">
  // A database tab: connects (finding the login on its own, else asking for one), then shows the engine's view.
  // Read-only until "Allow writes" is confirmed; the tab remembers that choice.
  import { onDestroy, untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { Database, KeyRound, Lock, LockOpen, RefreshCw } from "@lucide/svelte";
  import type { Tab } from "../layout";
  import SqlView from "./SqlView.svelte";
  import RedisView from "./RedisView.svelte";
  import { describe, engineName, type Connected } from "./db";
  import "./controls.css";

  let { tab }: { tab: Extract<Tab, { kind: "db" }> } = $props();

  let conn = $state<Connected | null>(null);
  let error = $state("");
  let connecting = $state(false);
  let login = $state<{ user: string; password: string; database: string; remember: boolean; why: string } | null>(null);
  let asking = $state(false); // "Allow writes" waits for a confirmation
  let version = $state(0); // bumped by Refresh
  let typed: Record<string, unknown> = {}; // the login typed in: switching databases reconnects with it

  const target = $derived(tab.target);
  const canWrite = $derived(target.engine !== "mongo");

  async function connect(extra: Record<string, unknown> = {}) {
    const old = conn;
    connecting = true;
    error = "";
    try {
      const c = await invoke<Connected>("db_connect", { target: { ...target, ...extra } });
      if (old) invoke("db_disconnect", { id: old.id }).catch(() => {});
      conn = c;
      login = null;
    } catch (e) {
      const m = String(e);
      if (m.startsWith("auth: ")) login = { user: (extra.user as string) ?? login?.user ?? "", password: "", database: target.database ?? "", remember: false, why: m.slice(6) };
      else error = m;
    }
    connecting = false;
  }
  $effect(() => {
    untrack(() => connect());
  });
  onDestroy(() => conn && invoke("db_disconnect", { id: conn.id }).catch(() => {}));

  function submit(e: SubmitEvent) {
    e.preventDefault();
    if (!login) return;
    const { user, password, database, remember } = login;
    typed = { user: user || null, password };
    connect({ ...typed, database: database || null, remember });
  }

  // Another database on the same server: a connection of its own (Postgres binds one to a database).
  function useDatabase(d: string) {
    if (!conn || d === conn.database) return;
    tab.target = { ...target, database: d };
    if (conn.engine === "mongo") conn = { ...conn, database: d };
    else connect({ ...typed, database: d });
  }
</script>

<div class="db">
  <header class="head">
    <Database size={14} strokeWidth={1.5} />
    <span class="engine">{engineName[target.engine]}</span>
    <span class="route" title={describe(target)}>{conn?.route ?? describe(target)}</span>
    {#if conn}
      {#if conn.user}<span class="who" title="Logged in as">{conn.user}</span>{/if}
      <span class="via" title="Where the login came from">via {conn.via}</span>
      {#if conn.engine !== "redis" && conn.engine !== "sqlite" && conn.databases.length}
        <select aria-label="Database" value={conn.database} onchange={(e) => useDatabase(e.currentTarget.value)}>
          {#each conn.databases as d (d)}<option value={d}>{d}</option>{/each}
        </select>
      {/if}
      <span class="spacer"></span>
      {#if canWrite}
        {#if tab.write}
          <button class="mode open" onclick={() => (tab.write = false)} title="Statements in this tab may change data. Click to make it read-only again">
            <LockOpen size={13} strokeWidth={1.5} />Writes allowed
          </button>
        {:else}
          <button class="mode" onclick={() => (asking = true)} title="Statements in this tab can only read"><Lock size={13} strokeWidth={1.5} />Read-only</button>
        {/if}
      {/if}
      <button class="icon-btn" aria-label="Refresh" title="Refresh" onclick={() => version++}><RefreshCw size={13} strokeWidth={1.5} /></button>
    {/if}
  </header>
  {#if asking}
    <div class="confirm" role="alertdialog" aria-label="Allow writes">
      <span>Statements in this tab will be able to change and delete data in <strong>{conn?.database || describe(target)}</strong>.</span>
      <button class="primary" onclick={() => ((tab.write = true), (asking = false))}>Allow writes</button>
      <button class="ghost" onclick={() => (asking = false)}>Cancel</button>
    </div>
  {/if}

  <div class="content">
    {#if conn}
      {#key conn.id}
        {#if conn.engine === "redis"}
          <RedisView {conn} write={!!tab.write} {version} />
        {:else}
          <SqlView {conn} {target} write={!!tab.write} {version} />
        {/if}
      {/key}
    {:else if login}
      <form class="login" onsubmit={submit} aria-label="Log in">
        <h3><KeyRound size={15} strokeWidth={1.5} />Log in to {engineName[target.engine]} at {describe(target)}</h3>
        <p class="why">{login.why}</p>
        <p class="hint">No login for it was found in the container's settings, the project's .env files or the Keychain.</p>
        {#if target.engine !== "redis"}
          <label>User <input name="user" autocomplete="off" bind:value={login.user} /></label>
        {/if}
        <label>Password <input name="password" type="password" bind:value={login.password} /></label>
        {#if target.engine === "postgres" || target.engine === "mysql" || target.engine === "mongo"}
          <label>Database <input name="database" autocomplete="off" bind:value={login.database} /></label>
        {/if}
        <label class="check"><input type="checkbox" bind:checked={login.remember} /> Remember in the Keychain</label>
        <button class="primary" type="submit" disabled={connecting}>{connecting ? "Connecting…" : "Connect"}</button>
      </form>
    {:else if error}
      <div class="failed">
        <p role="alert">{error}</p>
        <button class="ghost" onclick={() => connect()}>Try again</button>
      </div>
    {:else}
      <p class="quiet">Connecting…</p>
    {/if}
  </div>
</div>

<style>
  .db {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    background: var(--surface);
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border-bottom: 1px solid var(--border);
    font-size: 12.5px;
    white-space: nowrap;
  }
  .engine {
    font-weight: 600;
  }
  .route {
    overflow: hidden;
    font-family: var(--font-mono);
    font-size: 12px;
    text-overflow: ellipsis;
  }
  .who,
  .via {
    color: var(--muted-foreground);
  }
  .spacer {
    flex: 1;
  }
  .mode {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 3px 8px;
    border: 1px solid var(--border);
    border-radius: 9999px;
    background: none;
    color: var(--muted-foreground);
    font: inherit;
    cursor: pointer;
  }
  .mode.open {
    border-color: var(--gold-ring);
    color: var(--foreground);
  }
  .confirm {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    border-bottom: 1px solid var(--border);
    border-left: 2px solid var(--gold);
    background: var(--card);
    font-size: 12.5px;
  }
  .confirm span {
    flex: 1;
  }
  .content {
    flex: 1;
    min-height: 0;
  }
  .login {
    display: flex;
    flex-direction: column;
    gap: 10px;
    width: min(360px, 90%);
    margin: 40px auto;
    font-size: 13px;
  }
  .login h3 {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0;
    font-size: 14px;
  }
  .login label {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .login label.check {
    flex-direction: row;
    align-items: center;
  }
  .why {
    margin: 0;
    color: var(--diff-del-gutter);
  }
  .hint {
    margin: 0;
    color: var(--muted-foreground);
    font-size: 12px;
  }
  .failed {
    padding: 24px;
    font-size: 13px;
  }
  .failed p {
    color: var(--diff-del-gutter);
    white-space: pre-wrap;
  }
  .quiet {
    padding: 24px;
    color: var(--muted-foreground);
  }
</style>
