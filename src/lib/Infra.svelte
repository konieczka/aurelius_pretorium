<script module lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { app } from "./state.svelte";

  export type Listener = { pid: number; ports: number[]; name: string; args: string; cwd: string; root: string; kind: "project" | "service" | "system"; tab: string | null };
  export type Container = { id: string; name: string; image: string; state: string; status: string; ports: { host: number; inner: number }[]; project: string; service: string; root: string };
  export type Recipe = { dir: string; name: string; command: string; kind: "script" | "compose" | "just" | "make" };
  type Scan = { listeners: Listener[]; containers: Container[]; docker: "ok" | "down" | "missing" };

  export const infra = $state({
    scan: null as Scan | null,
    recipes: [] as Recipe[], // ways to start the workspace's dev servers (package.json scripts, Compose, just, make)
    recipesFor: "", // the workspace they're for
    all: false, // also the system's and apps' own ports (AirPlay, Postman, Ollama...)
  });

  // Polls overlap (interval, focus, after an action): only the latest one started may write.
  let refreshes = 0;
  export async function refreshInfra() {
    const seq = ++refreshes;
    const ws = app.workspace;
    try {
      const [s, r] = await Promise.all([invoke<Scan>("infra_scan"), ws ? invoke<Recipe[]>("infra_recipes", { dir: ws }).catch(() => []) : []]);
      if (seq === refreshes) Object.assign(infra, { scan: s, recipes: r, recipesFor: ws });
    } catch {}
  }

  /** Keeps the scan fresh: every 3s while the Servers mode is up, every 15s otherwise (its badge). ponytail: polling. */
  export function watchInfra() {
    refreshInfra();
    let ticks = 0;
    const t = setInterval(() => {
      ticks++;
      if (document.hasFocus() && ((app.sidebar === "infra" && app.sidebarOpen) || ticks % 5 === 0)) refreshInfra();
    }, 3000);
    window.addEventListener("focus", refreshInfra);
    return () => (clearInterval(t), window.removeEventListener("focus", refreshInfra));
  }

  /** Is `root` the workspace's folder, one in it, or one it's in? */
  export const near = (root: string, ws = app.workspace) => !!root && (root === ws || ws.startsWith(root + "/") || root.startsWith(ws + "/"));

  /** Servers and running containers of the workspace's project (the mode's badge). */
  export const serverCount = () => {
    const s = infra.scan;
    if (!s) return 0;
    return s.listeners.filter((l) => l.kind === "project" && near(l.root)).length + s.containers.filter((c) => c.state === "running" && near(c.root)).length;
  };
</script>

<script lang="ts">
  import { ask, message } from "@tauri-apps/plugin-dialog";
  import { ChevronRight, CircleStop, Ellipsis, Globe, Play, RotateCw, ScrollText, SquareTerminal } from "@lucide/svelte";
  import { openBrowser, runInTerminal } from "./state.svelte";
  import { pick } from "./QuickPick.svelte";
  import { showTab, tabsIn, ws } from "./workspaces.svelte";

  type Group = { key: string; title: string; hint: string; listeners: Listener[]; containers: Container[] };

  // Databases, caches and queues: there's no page to open on their ports.
  const NOT_WEB = new Set([5432, 3306, 6379, 27017, 11211, 5672, 1433, 1521, 9042, 2181, 9092, 26257, 4222, 6380, 33060]);
  const DATABASES = ["postgres", "redis-server", "valkey-server", "mysqld", "mariadbd", "mongod", "memcached", "rabbitmq-server", "clickhouse"];

  let closed = $state<Record<string, boolean>>({});
  $effect(() => void refreshInfra()); // switching to the mode shows what's up now, not 15s ago

  const base = (p: string) => p.slice(p.lastIndexOf("/") + 1);
  // The workspace's own servers and containers come first, on their own; then every other project's.
  const view = $derived.by(() => {
    const s = infra.scan;
    const here: Group = { key: "\0here", title: base(app.workspace), hint: app.workspace, listeners: [], containers: [] };
    if (!s) return { here, others: [] };
    const projects = new Map<string, Group>();
    const project = (root: string) => projects.get(root) ?? projects.set(root, { key: root, title: base(root), hint: root, listeners: [], containers: [] }).get(root)!;
    const services: Group = { key: "\0services", title: "Services", hint: "Homebrew services", listeners: [], containers: [] };
    const loose: Group = { key: "\0containers", title: "Containers", hint: "Containers outside a Compose project", listeners: [], containers: [] };
    const system: Group = { key: "\0system", title: "System and apps", hint: "Ports the OS and apps listen on", listeners: [], containers: [] };
    for (const l of s.listeners) {
      if (l.kind === "project") (near(l.root) ? here : project(l.root)).listeners.push(l);
      else if (l.kind === "service") services.listeners.push(l);
      else if (infra.all) system.listeners.push(l);
    }
    for (const c of s.containers) (!c.root ? loose : near(c.root) ? here : project(c.root)).containers.push(c);
    const sorted = [...projects.values()].sort((a, b) => a.title.localeCompare(b.title));
    return { here, others: [...sorted, services, loose, system].filter((g) => g.listeners.length || g.containers.length) };
  });

  // Recipes started from here, by `key`: until their server listens (it takes a moment) they count as running.
  const launched = $state<Record<string, { tab: string; at: number }>>({});
  const key = (r: Recipe) => `${r.dir}\0${r.command}`;
  const inside = (path: string, dir: string) => path === dir || path.startsWith(dir + "/");
  /** Is something it starts up already? A server in its folder, its Compose project's containers, or what it was just
   * started in. ponytail: by folder, so a root `dev` counts as running once any of the repo's servers is. */
  function running(r: Recipe, s: Scan) {
    const l = launched[key(r)];
    if (l && Date.now() - l.at < 30_000 && workspaceOf(l.tab)) return true;
    if (r.kind === "compose") return s.containers.some((c) => c.state === "running" && !!c.root && (inside(c.root, r.dir) || inside(r.dir, c.root)));
    return s.listeners.some((x) => x.kind === "project" && !DATABASES.includes(x.name) && (inside(x.cwd, r.dir) || (!!l && x.tab === l.tab)));
  }
  // A button for the best of each kind in the workspace's folder (a package's own only when the folder has none);
  // the rest are under More.
  const starts = $derived.by(() => {
    const s = infra.scan;
    if (!s || infra.recipesFor !== app.workspace) return { shown: [], more: [] };
    const idle = infra.recipes.filter((r) => !running(r, s));
    const rootScripts = idle.some((r) => r.kind === "script" && r.dir === app.workspace);
    const seen = new Set<string>();
    const shown: Recipe[] = [];
    const more: Recipe[] = [];
    for (const r of idle) {
      const k = `${r.dir}\0${r.kind}`;
      const best = !seen.has(k) && (r.kind !== "script" || r.dir === app.workspace || !rootScripts) && shown.length < 4;
      seen.add(k);
      (best ? shown : more).push(r);
    }
    return { shown, more };
  });
  const rel = (dir: string) => (dir === app.workspace ? "" : dir.startsWith(app.workspace + "/") ? dir.slice(app.workspace.length + 1) : dir);
  const recipeDetail = (r: Recipe) => [rel(r.dir), r.command].filter(Boolean).join(" · ");

  function start(r: Recipe) {
    launched[key(r)] = { tab: runInTerminal(r.dir, r.command, "rightmost"), at: Date.now() };
  }
  async function startMore() {
    const v = await pick("Start a dev server", starts.more.map((r) => ({ label: r.name, detail: recipeDetail(r), value: key(r) })));
    const r = v && starts.more.find((r) => key(r) === v);
    if (r) start(r);
  }

  const where = (l: Listener) => (l.root && l.cwd !== l.root && l.cwd.startsWith(l.root + "/") ? l.cwd.slice(l.root.length + 1) : "");
  const webPort = (l: Listener) => (DATABASES.includes(l.name) ? undefined : l.ports.find((p) => !NOT_WEB.has(p)));
  const webContainerPort = (c: Container) => (c.state === "running" ? c.ports.find((p) => !NOT_WEB.has(p.inner))?.host : undefined);
  const workspaceOf = (tab: string) => ws.all.find((m) => tabsIn(m.id).some((t) => t.id === tab))?.id;
  const fail = (e: unknown) => message(String(e), { kind: "error" });

  const open = (port: number) => openBrowser(`http://localhost:${port}`);
  function goTo(tab: string) {
    const w = workspaceOf(tab);
    if (w) showTab(w, tab);
  }
  async function stop(l: Listener) {
    const on = l.ports.map((p) => `:${p}`).join(", ");
    if (!(await ask(`Stop ${l.name} (pid ${l.pid}) on ${on}?`, { kind: "warning", okLabel: "Stop" }))) return;
    await invoke("infra_stop", { pid: l.pid }).catch(fail);
    refreshInfra();
  }
  async function docker(action: "start" | "stop" | "restart", c: Container) {
    await invoke("infra_docker", { action, id: c.id }).catch(fail);
    refreshInfra();
  }
  const logs = (c: Container) => runInTerminal(c.root || app.workspace, `docker logs -f --tail 200 ${c.name}`);

  // The row itself does the obvious thing: back to the terminal running it, else its page.
  function primary(l: Listener) {
    const p = webPort(l);
    if (l.tab && workspaceOf(l.tab)) goTo(l.tab);
    else if (p) open(p);
  }
  const listenerHint = (l: Listener) => [l.args, l.cwd, `pid ${l.pid}`, l.tab ? "Started in a Pretorium terminal" : ""].filter(Boolean).join("\n");
  const containerHint = (c: Container) => [c.name, c.image, c.status, c.ports.map((p) => `localhost:${p.host} → ${p.inner}`).join(", ")].filter(Boolean).join("\n");
</script>

{#snippet listener(l: Listener)}
  {@const p = webPort(l)}
  <button class="row" title={listenerHint(l)} onclick={() => primary(l)}>
    <span class="dot on"></span>
    <span class="name">{l.name}</span>
    <span class="dir">{where(l)}</span>
    <span class="acts">
      {#if p}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_interactive_supports_focus -->
        <span role="button" class="act" title="Open localhost:{p} in a browser tab" onclick={(e) => (e.stopPropagation(), open(p))}><Globe size={13} strokeWidth={1.5} /></span>
      {/if}
      {#if l.tab}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_interactive_supports_focus -->
        <span role="button" class="act" title="Go to its terminal" onclick={(e) => (e.stopPropagation(), goTo(l.tab!))}><SquareTerminal size={13} strokeWidth={1.5} /></span>
      {/if}
      {#if l.kind !== "system"}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_interactive_supports_focus -->
        <span role="button" class="act" title="Stop" onclick={(e) => (e.stopPropagation(), stop(l))}><CircleStop size={13} strokeWidth={1.5} /></span>
      {/if}
    </span>
    <span class="ports">{#each l.ports.slice(0, 2) as port (port)}<span class="port">:{port}</span>{/each}{#if l.ports.length > 2}<span class="port">+{l.ports.length - 2}</span>{/if}</span>
  </button>
{/snippet}

{#snippet container(c: Container)}
  {@const p = webContainerPort(c)}
  {@const up = c.state === "running"}
  <button class="row" class:off={!up} title={containerHint(c)} onclick={() => p && open(p)}>
    <span class="dot" class:on={up}></span>
    <span class="name">{c.service || c.name}</span>
    <span class="dir">{up ? c.image : c.status}</span>
    <span class="acts">
      {#if p}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_interactive_supports_focus -->
        <span role="button" class="act" title="Open localhost:{p} in a browser tab" onclick={(e) => (e.stopPropagation(), open(p))}><Globe size={13} strokeWidth={1.5} /></span>
      {/if}
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_interactive_supports_focus -->
      <span role="button" class="act" title="Follow its logs in a terminal" onclick={(e) => (e.stopPropagation(), logs(c))}><ScrollText size={13} strokeWidth={1.5} /></span>
      {#if up}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_interactive_supports_focus -->
        <span role="button" class="act" title="Restart" onclick={(e) => (e.stopPropagation(), docker("restart", c))}><RotateCw size={13} strokeWidth={1.5} /></span>
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_interactive_supports_focus -->
        <span role="button" class="act" title="Stop" onclick={(e) => (e.stopPropagation(), docker("stop", c))}><CircleStop size={13} strokeWidth={1.5} /></span>
      {:else}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_interactive_supports_focus -->
        <span role="button" class="act" title="Start" onclick={(e) => (e.stopPropagation(), docker("start", c))}><Play size={13} strokeWidth={1.5} /></span>
      {/if}
    </span>
    <span class="ports">{#each c.ports.slice(0, 2) as port (port.host)}<span class="port">:{port.host}</span>{/each}{#if c.ports.length > 2}<span class="port">+{c.ports.length - 2}</span>{/if}</span>
  </button>
{/snippet}

{#if !infra.scan}
  <div class="empty">Loading…</div>
{:else}
  {@const here = view.here}
  {@const herecount = here.listeners.length + here.containers.length}
  <div class="panel">
    <div class="section">
      <button class="shead" title={app.workspace} onclick={() => (closed[here.key] = !closed[here.key])}>
        <span class="chev" class:open={!closed[here.key]}><ChevronRight size={12} strokeWidth={1.5} /></span>
        <span class="stitle">{here.title}</span>
        {#if herecount}<span class="count">{herecount}</span>{/if}
      </button>
      {#if !closed[here.key]}
        {#each here.listeners as l (l.pid)}{@render listener(l)}{/each}
        {#each here.containers as c (c.id)}{@render container(c)}{/each}
        {#each starts.shown as r (key(r))}
          <button class="row start" title="Run {r.command} in a new terminal{rel(r.dir) ? ` in ${rel(r.dir)}` : ''}" onclick={() => start(r)}>
            <span class="play"><Play size={11} strokeWidth={2} /></span>
            <span class="name">{r.name}</span>
            <span class="dir">{recipeDetail(r)}</span>
          </button>
        {/each}
        {#if starts.more.length}
          <button class="row start" title="Other scripts and recipes that start a server" onclick={startMore}>
            <span class="play"><Ellipsis size={11} strokeWidth={2} /></span>
            <span class="name">More…</span>
            <span class="dir">{starts.more.length}</span>
          </button>
        {/if}
        {#if !herecount && !starts.shown.length && !starts.more.length}
          <p class="quiet">Nothing is running here.</p>
        {/if}
      {/if}
    </div>
    {#if view.others.length}<hr class="sep" />{/if}
    {#each view.others as g (g.key)}
      <div class="section">
        <button class="shead" title={g.hint} onclick={() => (closed[g.key] = !closed[g.key])}>
          <span class="chev" class:open={!closed[g.key]}><ChevronRight size={12} strokeWidth={1.5} /></span>
          <span class="stitle">{g.title}</span>
          <span class="count">{g.listeners.length + g.containers.length}</span>
        </button>
        {#if !closed[g.key]}
          {#each g.listeners as l (l.pid)}{@render listener(l)}{/each}
          {#each g.containers as c (c.id)}{@render container(c)}{/each}
        {/if}
      </div>
    {/each}
    {#if infra.scan.docker === "down"}
      <p class="note">Docker isn't running: start it to see your containers.</p>
    {/if}
  </div>
{/if}

<style>
  .panel {
    display: flex;
    flex-direction: column;
    padding: 2px 0 12px;
  }
  .empty {
    padding: 16px 12px;
    color: var(--muted-foreground);
    font-size: 13px;
    text-align: center;
  }
  .play {
    display: grid;
    flex: none;
    place-items: center;
    width: 12px;
    margin-left: -3px;
    color: var(--muted-foreground);
  }
  .start .name {
    color: var(--muted-foreground);
  }
  .start:hover .play,
  .start:hover .name {
    color: var(--foreground);
  }
  .quiet {
    margin: 0;
    padding: 2px 0 4px 20px;
    color: var(--muted-foreground);
    font-size: 12px;
  }
  /* Every other project's below a hairline, across the sidebar (out of the tree's 8px padding) */
  .sep {
    height: 1px;
    margin: 10px -8px 4px;
    border: 0;
    background: var(--border);
  }
  .note {
    margin: 10px 0 0;
    padding: 0 6px;
    color: var(--muted-foreground);
    font-size: 12px;
  }
  .section {
    margin-top: 4px;
  }
  /* Like Source Control's section heads and rows */
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
    overflow: hidden;
    text-overflow: ellipsis;
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
  .row.off .name {
    color: var(--muted-foreground);
  }
  .dot {
    flex: none;
    width: 6px;
    height: 6px;
    border-radius: 9999px;
    background: var(--muted-foreground);
    opacity: 0.5;
  }
  .dot.on {
    background: var(--diff-add-gutter);
    opacity: 1;
  }
  .name {
    flex: none;
    max-width: 50%;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .dir {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
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
  .ports {
    display: flex;
    flex: none;
    gap: 3px;
  }
  .port {
    color: var(--muted-foreground);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }
</style>
