<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { ask, message } from "@tauri-apps/plugin-dialog";
  import { Eye, EyeOff, Pencil, Plus, Power, RefreshCw, Search, Trash2, TriangleAlert, X } from "@lucide/svelte";
  import ProviderIcon from "./ProviderIcon.svelte";
  import { app } from "./state.svelte";
  import { closeMcp, loadMcp, mcp, revealKey } from "./mcpView.svelte";
  import {
    argLines, blocksFor, clashes, copyPlan, drifted, flagged, group, matches, placeOf, readers, reportText, sameFile, summary, users, validName,
    type Entry, type Pair, type Place, type Report, type Server, type Source, type Target, type Transport,
  } from "./mcp";
  import { listNames, shortPath } from "./skills";
  import { PROVIDERS, providerName, type Provider } from "./usageStats";

  const reload = () => loadMcp(app.workspace);
  $effect(() => {
    app.workspace; // rescan on open and when the sidebar switches folders
    reload();
  });

  const all = $derived(group(mcp.scan?.entries ?? []));
  const sources = $derived(mcp.scan?.sources ?? []);
  const broken = $derived(sources.filter((s) => s.error));
  const folder = $derived(app.workspace.split("/").pop());
  const shown = $derived(all.filter((s) => matches(s, mcp.query, mcp.agent)));
  const groups = $derived([
    { scope: "project", title: `Project · ${folder}`, items: shown.filter((s) => s.scope === "project") },
    { scope: "local", title: "Just you · Claude Code", items: shown.filter((s) => s.scope === "local") },
    { scope: "global", title: "Global", items: shown.filter((s) => s.scope === "global") },
    { scope: "managed", title: "Managed", items: shown.filter((s) => s.scope === "managed") },
  ]);
  const selected = $derived(shown.find((s) => s.key === mcp.selected) ?? groups.flatMap((g) => g.items)[0]);
  // The definition whose details show: the one picked in "Defined in", else the first.
  let pickedPath = $state("");
  const place = $derived(selected?.places.find((e) => e.path + e.table.join(".") === pickedPath) ?? selected?.places[0]);

  const short = (p: string) => shortPath(p, mcp.scan?.home ?? "", app.workspace);
  const names = (a: Provider[]) => listNames(a.map((p) => providerName[p]));
  const fileName = (e: Entry | Source) => short(e.path) + (e.label ? ` · ${e.label.toLowerCase()}` : "");
  const scopeLabel = (s: Server) => ({ project: "Project", local: "Just you", global: "Global", managed: "Managed" })[s.scope];
  const transportLabel: Record<Transport, string> = { stdio: "stdio", http: "HTTP", sse: "SSE", ws: "WebSocket" };
  const writable = (s: Server) => s.places.filter((e) => !e.readOnly);

  function select(s: Server) {
    mcp.selected = s.key;
    mcp.form = null;
    pickedPath = "";
  }

  async function run(f: () => Promise<unknown>) {
    try {
      await f();
    } catch (e) {
      message(String(e), { kind: "error" });
    }
    await reload();
  }

  /** Dry-run `call`, confirm what the files can't hold, then do it. False when it stopped. */
  async function confirmed(call: (dryRun: boolean) => Promise<Report[]>, title: string, okLabel: string) {
    const dry = await call(true);
    const failed = dry.filter((r) => r.error);
    if (failed.length) {
      await message(failed.map((r) => `${short(r.path)}: ${r.error}`).join("\n"), { kind: "error", title });
      return false;
    }
    const text = reportText(dry, short);
    if (text && !(await ask(text, { title, kind: "warning", okLabel, cancelLabel: "Cancel" }))) return false;
    // A file can change while the dialog is open: then nothing was written.
    const failedNow = (await call(false)).filter((r) => r.error);
    if (failedNow.length) {
      await message(failedNow.map((r) => `${short(r.path)}: ${r.error}`).join("\n"), { kind: "error", title });
      return false;
    }
    return true;
  }

  /** The definition to copy: one that loads for someone, else the first. */
  const best = (s: Server) => s.places.find((e) => e.enabled && e.agents.some((a) => !e.blocked.some((b) => b.agent === a))) ?? s.places[0];

  /** Translate `s` into the files `agents` read (a file also reaches the other agents reading it). */
  async function give(s: Server, agents: Provider[]) {
    const targets: Target[] = copyPlan(s, agents, sources).map((f) => ({ path: f.path, table: f.table }));
    if (!targets.length) return;
    const from = placeOf(best(s));
    await run(() =>
      confirmed((dryRun) => invoke<Report[]>("mcp_copy", { project: app.workspace, from, targets, replace: false, dryRun }), `Copy “${s.name}”?`, "Copy"),
    );
  }
  /** "Copy it into ~/.claude.json (Grok Build reads it too)" */
  function giveTitle(s: Server, a: Provider) {
    const f = copyPlan(s, [a], sources)[0];
    const also = f.agents.filter((x) => x !== a && !readers(s).includes(x));
    return `Copy it into ${fileName(f)}` + (also.length ? ` (${names(also)} read it too)` : "");
  }

  /** Overwrite the other copies with `e`. */
  async function makeOthersMatch(s: Server, e: Entry) {
    const targets = s.places.filter((o) => o !== e && !o.readOnly).map((o) => ({ path: o.path, table: o.table }));
    await run(() =>
      confirmed(
        (dryRun) => invoke<Report[]>("mcp_copy", { project: app.workspace, from: placeOf(e), targets, replace: true, dryRun }),
        `Make ${targets.length === 1 ? "the other copy" : `the ${targets.length} other copies`} match ${fileName(e)}?`,
        "Overwrite",
      ),
    );
  }

  /** Agents that stop seeing `s` once `e` is removed. */
  const lostWithout = (s: Server, e: Entry) => e.agents.filter((a) => !s.places.some((o) => o !== e && o.agents.includes(a)));

  async function removeAll(s: Server) {
    const places = writable(s);
    const text = `${names(readers(s))} will no longer see it. It's removed from ${places.map(fileName).join(", ")}.`;
    if (!(await ask(text, { title: `Delete “${s.name}”?`, kind: "warning", okLabel: "Delete", cancelLabel: "Cancel" }))) return;
    await run(() => invoke("mcp_remove", { project: app.workspace, places: places.map(placeOf) }));
  }

  async function removePlace(s: Server, e: Entry) {
    const lost = lostWithout(s, e);
    const text = lost.length ? `${names(lost)} will no longer see “${s.name}”.` : `Every agent still sees “${s.name}” through its other files.`;
    if (!(await ask(text, { title: `Remove it from ${fileName(e)}?`, kind: "warning", okLabel: "Remove", cancelLabel: "Cancel" }))) return;
    await run(() => invoke("mcp_remove", { project: app.workspace, places: [placeOf(e)] }));
  }

  const setEnabled = (e: Entry, on: boolean) => run(() => invoke("mcp_set_enabled", { project: app.workspace, place: placeOf(e), on }));

  async function reveal(e: Entry, field: "env" | "headers", key: string) {
    const k = revealKey(placeOf(e), field, key);
    if (mcp.revealed[k] !== undefined) return void delete mcp.revealed[k];
    try {
      mcp.revealed[k] = await invoke<string>("mcp_reveal", { project: app.workspace, place: placeOf(e), field, key });
    } catch (err) {
      message(String(err), { kind: "error" });
    }
  }

  // ---- Form: a new server, or one definition edited

  type Row = { key: string; value: string; keep: boolean }; // keep: a hidden value left as it is
  let name = $state("");
  let transport = $state<Transport>("stdio");
  let command = $state("");
  let argsText = $state("");
  let cwd = $state("");
  let url = $state("");
  let env = $state<Row[]>([]);
  let headers = $state<Row[]>([]);
  let picked = $state<Target[]>([]); // new: files to write into
  let formError = $state("");
  const editing = $derived(mcp.form?.kind === "edit" ? mcp.form.place : null);
  const editEntry = $derived(editing && mcp.scan?.entries.find((e) => sameFile(e, editing) && e.name === editing.name));
  const formScope = $derived(mcp.form?.kind === "new" ? mcp.form.scope : null);
  // Project servers can also go in Claude's per-person list.
  const targets = $derived(sources.filter((f) => !f.readOnly && (formScope === "project" ? f.scope === "project" || f.scope === "local" : f.scope === formScope)));
  const preset = $derived(mcp.scan?.presets.find((p) => p.scope === formScope)?.targets ?? []);
  const pickedAgents = $derived(PROVIDERS.filter((a) => targets.some((f) => picked.some((t) => sameFile(t, f)) && f.agents.includes(a))));
  const nameError = $derived(name && !validName(name) ? "Start with a letter; then letters, digits, - and _ (up to 64)" : "");
  const isPicked = (f: Source) => picked.some((t) => sameFile(t, f));
  const toRows = (pairs: Pair[]): Row[] => pairs.map((p) => ({ key: p.key, value: p.value ?? "", keep: p.value === null }));

  function fill(spec: Entry["spec"] | null) {
    transport = spec?.transport ?? "stdio";
    command = spec?.command ?? "";
    argsText = spec?.args.join("\n") ?? "";
    cwd = spec?.cwd ?? "";
    url = spec?.url ?? "";
    env = toRows(spec?.env ?? []);
    headers = toRows(spec?.headers ?? []);
    formError = "";
  }
  function startNew() {
    mcp.form = { kind: "new", scope: "project" };
    name = "";
    fill(null);
    picked = (mcp.scan?.presets.find((p) => p.scope === "project")?.targets ?? []).slice();
  }
  function setScope(scope: "project" | "global") {
    mcp.form = { kind: "new", scope };
    picked = (mcp.scan?.presets.find((p) => p.scope === scope)?.targets ?? []).slice();
  }
  function startEdit(e: Entry) {
    mcp.form = { kind: "edit", place: placeOf(e), fingerprint: e.fingerprint };
    name = e.name;
    fill(e.spec);
  }
  function togglePick(f: Source) {
    picked = isPicked(f) ? picked.filter((t) => !sameFile(t, f)) : [...picked, { path: f.path, table: f.table }];
  }

  const input = () => {
    const pairs = (rows: Row[]) => rows.filter((r) => r.key.trim()).map((r) => ({ key: r.key.trim(), value: r.keep ? null : r.value }));
    return { transport, command, args: argLines(argsText), env: pairs(env), cwd, url, headers: pairs(headers) };
  };

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    const f = mcp.form;
    if (!f) return;
    formError = "";
    try {
      if (f.kind === "new") {
        if (!validName(name) || !picked.length) return;
        const spec = input();
        const call = (dryRun: boolean) => invoke<Report[]>("mcp_add", { project: app.workspace, targets: picked, name, spec, dryRun });
        if (!(await confirmed(call, `Add “${name}”?`, "Add"))) return;
        mcp.selected = `${f.scope === "project" && picked.every((t) => sources.find((s) => sameFile(s, t))?.scope === "local") ? "local" : f.scope}:${name}`;
      } else {
        const reports = await invoke<Report[]>("mcp_update", { project: app.workspace, place: f.place, spec: input(), fingerprint: f.fingerprint });
        const failed = reports.find((r) => r.error);
        if (failed) return void (formError = failed.error!);
        // Only what the file couldn't hold: warnings about values already in it would repeat on every save.
        const text = reportText(reports.map((r) => ({ ...r, warnings: [] })), short);
        if (text) message(text, { title: "Saved, with changes", kind: "info" });
      }
      mcp.form = null;
      await reload();
    } catch (err) {
      formError = String(err);
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key !== "Escape" || e.defaultPrevented) return;
    if (mcp.form) mcp.form = null;
    else if (!(e.target as Element).closest?.("input, textarea")) closeMcp();
  }
</script>

<svelte:window onkeydown={onKey} onfocus={reload} />

{#snippet pairs(e: Entry, field: "env" | "headers", list: Pair[])}
  <ul class="kv">
    {#each list as p (p.key)}
      {@const k = revealKey(placeOf(e), field, p.key)}
      <li>
        <span class="path k">{p.key}</span>
        {#if p.value !== null}
          <span class="path v selectable">{p.value}</span>
        {:else if mcp.revealed[k] !== undefined}
          <span class="path v selectable">{mcp.revealed[k]}</span>
          <button class="icon-btn" onclick={() => reveal(e, field, p.key)} title="Hide" aria-label="Hide {p.key}"><EyeOff size={13} strokeWidth={1.5} /></button>
        {:else}
          <span class="path v masked">••••••••</span>
          <button class="icon-btn" onclick={() => reveal(e, field, p.key)} title="Show this value" aria-label="Show {p.key}"><Eye size={13} strokeWidth={1.5} /></button>
        {/if}
      </li>
    {/each}
  </ul>
{/snippet}

{#snippet rows(list: Row[], placeholder: string, set: (r: Row[]) => void)}
  {#each list as r, i (i)}
    <div class="pair-row">
      <input bind:value={r.key} placeholder={placeholder} spellcheck="false" />
      {#if r.keep}
        <span class="kept">•••••••• <button type="button" class="link-btn" onclick={() => ((r.keep = false), (r.value = ""))}>Change</button></span>
      {:else}
        <input bind:value={r.value} placeholder={"value or ${VAR}"} spellcheck="false" />
      {/if}
      <button type="button" class="icon-btn" onclick={() => set(list.filter((_, j) => j !== i))} aria-label="Remove {r.key}"><X size={13} strokeWidth={1.5} /></button>
    </div>
  {/each}
  <button type="button" class="link-btn add-row" onclick={() => set([...list, { key: "", value: "", keep: false }])}>Add</button>
{/snippet}

<div class="mcp slab">
  <div class="page">
    <header>
      <div>
        <h1>MCP servers</h1>
        <p class="sub">What your coding agents connect to, in {folder} and your home folder</p>
      </div>
      <div class="tools">
        <button class="btn" onclick={startNew}><Plus size={14} strokeWidth={1.75} />New server</button>
        <button class="icon-btn" onclick={reload} title="Refresh" aria-label="Refresh"><RefreshCw size={15} strokeWidth={1.5} /></button>
        <button class="icon-btn" onclick={closeMcp} title="Close (Esc)" aria-label="Close MCP servers"><X size={16} strokeWidth={1.5} /></button>
      </div>
    </header>

    <div class="filters">
      <label class="search">
        <Search size={14} strokeWidth={1.5} />
        <input placeholder="Search servers" bind:value={mcp.query} spellcheck="false" />
      </label>
      <div class="seg" role="tablist" aria-label="Agent">
        <button role="tab" aria-selected={!mcp.agent} class:on={!mcp.agent} onclick={() => (mcp.agent = null)}>All</button>
        {#each PROVIDERS as p (p)}
          <button role="tab" aria-selected={mcp.agent === p} class:on={mcp.agent === p} onclick={() => (mcp.agent = p)} title="Servers {providerName[p]} reads">
            <ProviderIcon provider={p} size={13} />{providerName[p].split(" ")[0]}
            <span class="count">{all.filter((s) => readers(s).includes(p)).length}</span>
          </button>
        {/each}
      </div>
    </div>

    {#each broken as f (f.path + f.table.join("."))}
      <div class="warn top"><TriangleAlert size={13} strokeWidth={2} /><span>{short(f.path)}: {f.error}</span></div>
    {/each}

    {#if mcp.error}
      <div class="card error"><span>{mcp.error}</span><button class="btn" onclick={reload}>Retry</button></div>
    {:else if !mcp.scan}
      <p class="muted">Looking for MCP servers…</p>
    {:else}
      <div class="body">
        <nav class="list" aria-label="MCP servers">
          {#each groups as g (g.scope)}
            {#if g.items.length || g.scope === "project"}
              <h2>{g.title}<span class="count">{g.items.length}</span></h2>
              {#each g.items as s (s.key)}
                {@const using = users(s)}
                <button class="row" class:on={!mcp.form && selected === s} onclick={() => select(s)}>
                  <span class="row-top">
                    <span class="name">{s.name}</span>
                    {#if s.places.length > 1}<span class="tag small">{s.places.length} copies</span>{/if}
                    {#if flagged(s)}<span class="warn-icon" title="Has problems"><TriangleAlert size={12} strokeWidth={2} /></span>{/if}
                    <span class="icons">
                      {#each readers(s) as a (a)}<span class:dim={!using.includes(a)}><ProviderIcon provider={a} size={12} /></span>{/each}
                    </span>
                  </span>
                  <span class="row-desc">{summary(s.places[0].spec) || "No command or URL"}</span>
                </button>
              {:else}
                <p class="empty">{mcp.query || mcp.agent ? "None match." : "No MCP servers in this project yet."}</p>
              {/each}
            {/if}
          {/each}
        </nav>

        <div class="detail-scroll">
          <section class="detail">
            {#if mcp.form}
              {@const f = mcp.form}
              <form onsubmit={submit}>
                <h2 class="form-title">{f.kind === "new" ? "New MCP server" : `Edit “${name}” in ${editEntry ? fileName(editEntry) : short(f.place.path)}`}</h2>
                {#if f.kind === "new"}
                  <div class="seg wide" role="tablist" aria-label="Scope">
                    <button type="button" role="tab" aria-selected={f.scope === "project"} class:on={f.scope === "project"} onclick={() => setScope("project")}>Project · {folder}</button>
                    <button type="button" role="tab" aria-selected={f.scope === "global"} class:on={f.scope === "global"} onclick={() => setScope("global")}>Global</button>
                  </div>
                  <p class="hint">{f.scope === "project" ? `Only in ${folder}; project files can be committed with it.` : "In every project on this Mac."}</p>
                  <label class="field">
                    <span>Name</span>
                    <!-- svelte-ignore a11y_autofocus -->
                    <input bind:value={name} placeholder="github" spellcheck="false" autofocus />
                    {#if nameError}<span class="field-error">{nameError}</span>{/if}
                  </label>
                {/if}

                <div class="seg wide gap" role="tablist" aria-label="Transport">
                  <button type="button" role="tab" aria-selected={transport === "stdio"} class:on={transport === "stdio"} onclick={() => (transport = "stdio")}>Local command (stdio)</button>
                  <button type="button" role="tab" aria-selected={transport !== "stdio"} class:on={transport !== "stdio"} onclick={() => transport === "stdio" && (transport = "http")}>Remote URL</button>
                </div>

                {#if transport === "stdio"}
                  <label class="field"><span>Command</span><input bind:value={command} placeholder="npx" spellcheck="false" /></label>
                  <label class="field">
                    <span>Arguments</span>
                    <textarea class="mono" bind:value={argsText} rows="3" placeholder={"-y\n@modelcontextprotocol/server-github"} spellcheck="false"></textarea>
                    <span class="hint">One per line.</span>
                  </label>
                  <label class="field"><span>Working folder</span><input bind:value={cwd} placeholder="Optional" spellcheck="false" /></label>
                  <div class="field">
                    <span>Environment</span>
                    {@render rows(env, "NAME", (r) => (env = r))}
                  </div>
                {:else}
                  <label class="field"><span>URL</span><input bind:value={url} placeholder="https://mcp.example.com/mcp" spellcheck="false" /></label>
                  <div class="seg small-seg" role="tablist" aria-label="Protocol">
                    {#each ["http", "sse"] as const as t (t)}
                      <button type="button" role="tab" aria-selected={transport === t} class:on={transport === t} onclick={() => (transport = t)}>{t === "http" ? "Streamable HTTP" : "SSE (older)"}</button>
                    {/each}
                  </div>
                  <div class="field">
                    <span>Headers</span>
                    {@render rows(headers, "Authorization", (r) => (headers = r))}
                  </div>
                {/if}
                <p class="hint">Write secrets as <code>{"${VAR}"}</code> so they stay in your environment, not the file; each agent's file gets its own spelling.</p>

                {#if f.kind === "new"}
                  <fieldset>
                    <legend>Who sees it</legend>
                    {#each targets as t (t.path + t.table.join("."))}
                      <label class="target" class:on={isPicked(t)}>
                        <input type="checkbox" checked={isPicked(t)} onchange={() => togglePick(t)} />
                        <span class="target-text">
                          <span class="path">{fileName(t)}</span>
                          <span class="hint">{names(t.agents)}</span>
                        </span>
                        <span class="icons">{#each t.agents as a (a)}<ProviderIcon provider={a} size={13} />{/each}</span>
                      </label>
                    {/each}
                    <p class="hint">{pickedAgents.length === PROVIDERS.length ? "Every agent will see it." : pickedAgents.length ? `${names(pickedAgents)} will see it.` : "Pick at least one file."}</p>
                  </fieldset>
                {/if}

                {#if formError}<div class="warn"><TriangleAlert size={13} strokeWidth={2} />{formError}</div>{/if}
                <div class="actions">
                  <button class="btn primary" type="submit" disabled={f.kind === "new" && (!validName(name) || !picked.length)}>{f.kind === "new" ? "Add server" : "Save"}</button>
                  <button class="btn" type="button" onclick={() => (mcp.form = null)}>Cancel</button>
                </div>
              </form>
            {:else if selected && place}
              {@const s = selected}
              {@const using = users(s)}
              <div class="title">
                <h2>{s.name}</h2>
                <span class="tag">{scopeLabel(s)}</span>
                <span class="tag">{transportLabel[place.spec.transport]}</span>
                {#if writable(s).length}
                  <button class="btn danger" onclick={() => removeAll(s)} title="Remove it from every file"><Trash2 size={13} strokeWidth={1.5} />Delete</button>
                {/if}
              </div>
              <p class="desc path selectable">{summary(place.spec) || "No command or URL"}</p>

              {#each [...new Set(s.places.flatMap((e) => e.problems.map((p) => `${fileName(e)}: ${p}`)))] as p}
                <div class="warn"><TriangleAlert size={13} strokeWidth={2} /><span>{p}</span></div>
              {/each}
              {#if drifted(s)}
                <div class="warn"><TriangleAlert size={13} strokeWidth={2} /><span>The copies differ, so agents get different servers. Pick the right one under Defined in and make the others match.</span></div>
              {/if}
              {#each clashes(s, all) as c (c.other.key)}
                <div class="warn">
                  <TriangleAlert size={13} strokeWidth={2} />
                  <span>Also defined in {scopeLabel(c.other).toLowerCase()} scope: {names(c.agents)} use only one of them.</span>
                </div>
              {/each}

              {@const missing = PROVIDERS.filter((a) => !readers(s).includes(a) && copyPlan(s, [a], sources).length)}
              <h3>Used by</h3>
              <div class="agents">
                {#each readers(s) as a (a)}
                  {@const why = blocksFor(s, a)}
                  {#if using.includes(a)}
                    <span class="agent"><ProviderIcon provider={a} />{providerName[a]}</span>
                  {:else}
                    <span class="agent dim" title={why.map((b) => (b.by ? `Uses the one in ${short(b.by)}` : b.reason)).join("\n")}><ProviderIcon provider={a} />{providerName[a]}</span>
                  {/if}
                {/each}
                {#each missing as a (a)}
                  <button class="agent add" onclick={() => give(s, [a])} title={giveTitle(s, a)}><Plus size={12} strokeWidth={2} /><ProviderIcon provider={a} />{providerName[a]}</button>
                {/each}
                {#if missing.length > 1}
                  <button class="link-btn" onclick={() => give(s, missing)}>Give all agents access</button>
                {/if}
              </div>
              {#each readers(s).filter((a) => !using.includes(a)) as a (a)}
                {#each blocksFor(s, a) as b}
                  <p class="note">{b.by ? `${providerName[a]} uses the definition in ${short(b.by)}` : b.reason}.</p>
                {/each}
              {/each}

              <h3>Defined in</h3>
              <ul class="places">
                {#each s.places as e (e.path + e.table.join("."))}
                  <li class:picked={e === place && s.places.length > 1}>
                    <button class="place-btn" onclick={() => (pickedPath = e.path + e.table.join("."))} title={e.path}>
                      <span class="path">{fileName(e)}</span>
                      {#if !e.enabled}<span class="tag small">off</span>{/if}
                    </button>
                    <span class="icons">{#each e.agents as a (a)}<ProviderIcon provider={a} size={13} />{/each}</span>
                    {#if !e.readOnly}
                      {#if drifted(s) && e === place}
                        <button class="link-btn" onclick={() => makeOthersMatch(s, e)}>Make the others match</button>
                      {/if}
                      {#if e.dialect !== "claude"}
                        <button class="icon-btn" class:on-power={e.enabled} onclick={() => setEnabled(e, !e.enabled)} title={e.enabled ? "Turn off" : "Turn on"} aria-label={e.enabled ? "Turn off" : "Turn on"}><Power size={13} strokeWidth={1.5} /></button>
                      {/if}
                      <button class="icon-btn" onclick={() => startEdit(e)} title="Edit this definition" aria-label="Edit in {fileName(e)}"><Pencil size={13} strokeWidth={1.5} /></button>
                      {#if s.places.length > 1}
                        <button class="icon-btn" onclick={() => removePlace(s, e)} title="Remove from this file" aria-label="Remove from {fileName(e)}"><X size={13} strokeWidth={1.5} /></button>
                      {/if}
                    {/if}
                  </li>
                {/each}
              </ul>

              {#if s.places.length > 1}<h3>Details · {fileName(place)}</h3>{:else}<h3>Details</h3>{/if}
              <dl class="spec">
                {#if place.spec.transport === "stdio"}
                  <dt>Command</dt><dd class="path selectable">{place.spec.command || "—"}</dd>
                  {#if place.spec.args.length}<dt>Arguments</dt><dd><ul class="args">{#each place.spec.args as a}<li class="path selectable">{a}</li>{/each}</ul></dd>{/if}
                  {#if place.spec.cwd}<dt>Working folder</dt><dd class="path selectable">{place.spec.cwd}</dd>{/if}
                  {#if place.spec.env.length}<dt>Environment</dt><dd>{@render pairs(place, "env", place.spec.env)}</dd>{/if}
                {:else}
                  <dt>URL</dt><dd class="path selectable">{place.spec.url || "—"}</dd>
                  {#if place.spec.headers.length}<dt>Headers</dt><dd>{@render pairs(place, "headers", place.spec.headers)}</dd>{/if}
                {/if}
                {#if place.extra.length}<dt>Also sets</dt><dd class="path">{place.extra.join(", ")}</dd>{/if}
              </dl>
              {#if place.extra.length}<p class="note">Settings of {names(place.agents)}'s own; editing here keeps them.</p>{/if}
              {#if place.dialect === "claude" && !place.readOnly}<p class="note">Claude Code turns servers on and off in <code>/mcp</code>.</p>{/if}
              {#if place.readOnly}<p class="note">Managed by an admin: read-only here.</p>{/if}
            {:else}
              <p class="muted">{all.length ? "No servers match." : "No MCP servers yet. Add one with New server."}</p>
            {/if}
          </section>
        </div>
      </div>
    {/if}
  </div>
</div>

<style>
  /* Header fixed; the list and the selected server scroll on their own. Shared look with Skills.svelte */
  .page {
    display: flex;
    flex-direction: column;
    box-sizing: border-box;
    height: 100%;
    max-width: var(--page-width);
    margin: 0 auto;
    padding: 28px 32px 0;
  }
  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
  }
  h1 {
    margin: 0;
    font-size: 20px;
    font-weight: 600;
  }
  .sub {
    margin: 4px 0 0;
    color: var(--muted-foreground);
    font-size: 13px;
  }
  .tools {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .tools .btn {
    margin-right: 6px;
  }
  .filters {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
    margin: 22px 0 18px;
  }
  .search {
    display: flex;
    align-items: center;
    gap: 6px;
    flex: 1;
    min-width: 180px;
    max-width: 320px;
    height: 30px;
    padding: 0 10px;
    border: 1px solid var(--input);
    border-radius: var(--radius-md);
    background: var(--background);
    color: var(--muted-foreground);
  }
  .search input {
    flex: 1;
    min-width: 0;
    border: 0;
    outline: 0;
    background: none;
    color: var(--foreground);
    font: inherit;
    font-size: 13px;
  }
  .seg {
    display: inline-flex;
    flex-wrap: wrap;
    padding: 2px;
    border-radius: var(--radius-md);
    background: var(--muted);
  }
  .seg button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--muted-foreground);
    font-size: 12px;
    cursor: pointer;
  }
  .seg button.on {
    background: var(--background);
    color: var(--foreground);
    box-shadow: var(--shadow-raised);
  }
  .seg.wide {
    display: flex;
  }
  .seg.wide button {
    flex: 1;
    justify-content: center;
  }
  .count {
    color: var(--muted-foreground);
    font-size: 11px;
    font-weight: 400;
    font-variant-numeric: tabular-nums;
  }
  .muted {
    color: var(--muted-foreground);
    font-size: 13px;
  }
  .body {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(260px, 360px) minmax(0, 1fr);
    grid-template-rows: minmax(0, 1fr);
    gap: 24px;
  }
  .list,
  .detail-scroll {
    min-height: 0;
    overflow: auto;
    padding-bottom: 32px;
  }
  h2 {
    display: flex;
    align-items: baseline;
    gap: 8px;
    margin: 18px 0 6px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--muted-foreground);
  }
  .list h2:first-child {
    margin-top: 0;
  }
  .row {
    display: flex;
    flex-direction: column;
    gap: 2px;
    width: 100%;
    padding: 7px 10px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    text-align: left;
    cursor: pointer;
  }
  .row:hover {
    background: var(--accent);
  }
  .row.on {
    background: var(--sidebar-accent);
    box-shadow: inset 2px 0 0 var(--gold);
  }
  .row-top {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 13px;
    font-weight: 500;
  }
  .row-desc {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--muted-foreground);
    font-size: 12px;
  }
  .icons {
    display: inline-flex;
    gap: 4px;
    margin-left: auto;
    color: var(--muted-foreground);
  }
  .warn-icon {
    display: inline-flex;
    color: var(--destructive);
  }
  .empty {
    margin: 4px 10px 8px;
    color: var(--muted-foreground);
    font-size: 12.5px;
  }
  .detail {
    padding: 18px 20px 20px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--card);
  }
  .title {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .title h2,
  .form-title {
    margin: 0;
    color: var(--foreground);
    font-size: 16px;
    font-weight: 600;
    letter-spacing: 0;
    text-transform: none;
  }
  .form-title {
    margin-bottom: 14px;
  }
  .tag {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 1px 8px;
    border: 1px solid var(--border);
    border-radius: 9999px;
    color: var(--muted-foreground);
    font-size: 11px;
    white-space: nowrap;
  }
  .tag.small {
    padding: 0 6px;
    font-size: 10.5px;
  }
  .desc {
    margin: 10px 0 0;
    font-size: 13px;
    line-height: 1.5;
  }
  h3 {
    margin: 20px 0 8px;
    color: var(--muted-foreground);
    font-size: 12px;
    font-weight: 500;
  }
  .warn {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    margin-top: 10px;
    padding: 8px 10px;
    border-radius: var(--radius-sm);
    background: color-mix(in oklch, var(--destructive) 9%, transparent);
    font-size: 12.5px;
    line-height: 1.4;
  }
  .warn :global(svg) {
    flex: none;
    margin-top: 2px;
    color: var(--destructive);
  }
  .agents {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .agent {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    padding: 0 10px;
    border-radius: 9999px;
    background: var(--muted);
    font-size: 12.5px;
  }
  .agent.add {
    gap: 5px;
    border: 1px dashed var(--border-strong);
    background: none;
    color: var(--muted-foreground);
    cursor: pointer;
  }
  .agent.add:hover {
    border-color: var(--gold-ring);
    background: var(--gold-soft);
    color: var(--foreground);
  }
  .link-btn {
    padding: 0 4px;
    border: 0;
    background: none;
    color: var(--muted-foreground);
    font-size: 12px;
    text-decoration: underline;
    text-underline-offset: 2px;
    cursor: pointer;
  }
  .link-btn:hover {
    color: var(--foreground);
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .places li {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 30px;
    border-bottom: 1px solid var(--border);
  }
  .places li:last-child {
    border-bottom: 0;
  }
  .path {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-mono);
    font-size: 12px;
  }
  .note,
  .hint {
    color: var(--muted-foreground);
    font-size: 12px;
  }
  .note {
    margin: 10px 0 0;
  }
  .hint {
    margin: 6px 0 0;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 22px;
  }
  .btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    flex: none;
    height: 30px;
    padding: 0 12px;
    border: 1px solid var(--input);
    border-radius: var(--radius-sm);
    background: var(--background);
    color: var(--foreground);
    font-size: 12.5px;
    cursor: pointer;
  }
  .btn:hover {
    background: var(--accent);
  }
  .btn:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .btn.primary {
    border-color: var(--primary);
    background: var(--primary);
    color: var(--primary-foreground);
  }
  .btn.primary:hover:not(:disabled) {
    opacity: 0.9;
  }
  .btn.danger {
    margin-left: auto;
    color: var(--destructive);
  }
  .card.error {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 16px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--card);
    color: var(--muted-foreground);
    font-size: 12.5px;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 16px;
    font-size: 12px;
    color: var(--muted-foreground);
  }
  .field input,
  .field textarea {
    padding: 7px 10px;
    border: 1px solid var(--input);
    border-radius: var(--radius-sm);
    background: var(--background);
    color: var(--foreground);
    font: inherit;
    font-size: 13px;
    resize: vertical;
  }
  .field input {
    font-family: var(--font-mono);
  }
  .field .hint {
    margin: 0;
  }
  .field-error {
    color: var(--destructive);
    font-size: 12px;
  }
  fieldset {
    margin: 18px 0 0;
    padding: 0;
    border: 0;
  }
  legend {
    margin-bottom: 6px;
    padding: 0;
    color: var(--muted-foreground);
    font-size: 12px;
  }
  .target {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: pointer;
    font-size: 13px;
  }
  .target + .target {
    margin-top: 6px;
  }
  .target.on {
    border-color: var(--gold-ring);
    background: var(--gold-soft);
  }
  .target input {
    margin: 0;
    accent-color: var(--gold-ring);
  }
  .target-text {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }
  .target-text .hint {
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .title .btn {
    height: 26px;
    padding: 0 10px;
    font-size: 12px;
  }
  /* Plane 2: a slab, like the panes it stands in for */
  .mcp {
    grid-column: 2;
    grid-row: 2;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }
  .dim {
    opacity: 0.4;
  }
  .agent.dim {
    opacity: 0.55;
    text-decoration: line-through;
    text-decoration-color: var(--muted-foreground);
  }
  .warn.top {
    margin: 0 0 12px;
  }
  .desc.path {
    white-space: normal;
    word-break: break-all;
  }
  .places li.picked {
    background: var(--accent);
  }
  .place-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    padding: 0 4px;
    border: 0;
    background: none;
    color: var(--foreground);
    text-align: left;
    cursor: pointer;
  }
  .icon-btn.on-power {
    color: var(--gold);
  }
  .spec {
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr);
    gap: 8px 16px;
    margin: 0;
    font-size: 12.5px;
  }
  .spec dt {
    color: var(--muted-foreground);
  }
  .spec dd {
    min-width: 0;
    margin: 0;
  }
  .spec dd.path,
  .args .path {
    white-space: normal;
    word-break: break-all;
  }
  .args {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .kv li {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 24px;
  }
  .kv .k {
    flex: none;
    max-width: 40%;
  }
  .kv .v {
    flex: 1;
    min-width: 0;
    color: var(--muted-foreground);
  }
  .kv .masked {
    letter-spacing: 0.1em;
  }
  code {
    font-family: var(--font-mono);
    font-size: 11.5px;
  }
  .seg.gap {
    margin-top: 18px;
  }
  .small-seg {
    margin-top: 10px;
  }
  .field textarea.mono {
    font-family: var(--font-mono);
  }
  .pair-row {
    display: grid;
    grid-template-columns: minmax(0, 2fr) minmax(0, 3fr) auto;
    align-items: center;
    gap: 6px;
  }
  .pair-row input {
    min-width: 0;
  }
  .kept {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--muted-foreground);
    letter-spacing: 0.05em;
  }
  .add-row {
    align-self: flex-start;
    padding: 0;
  }
</style>
