<script lang="ts">
  import { FileCode, Lock, RefreshCw, RotateCcw, TriangleAlert, X } from "@lucide/svelte";
  import ProviderIcon from "./ProviderIcon.svelte";
  import ConfigDoc from "./ConfigDoc.svelte";
  import * as L from "./layout";
  import { app } from "./state.svelte";
  import { agentProvider } from "./agents.svelte";
  import { agents, closeAgents, loadAgents, setValue } from "./agentsView.svelte";
  import { FIELDS, effective, fits, getPath, optionsFor, parseInput, scopeOrder, type ConfigFile, type Field } from "./agentConfig";
  import { shortPath } from "./skills";
  import { PROVIDERS, providerName, type Provider } from "./usageStats";

  const reload = () => loadAgents(app.workspace);
  $effect(() => {
    app.workspace; // reread on open and when the sidebar switches folders
    reload();
  });

  const folder = $derived(app.workspace.split("/").pop());
  const short = (p: string) => shortPath(p, agents.scan?.home ?? "", app.workspace);

  const cfg = $derived(agents.scan?.agents.find((a) => a.agent === agents.selected));
  const name = $derived(providerName[agents.selected]);
  const fields = $derived(FIELDS[agents.selected]);
  // The settings form writes one of the agent's own files: global by default.
  const writable = $derived((cfg?.files ?? []).filter((f) => !f.readOnly).sort((a, b) => scopeOrder.indexOf(a.scope) - scopeOrder.indexOf(b.scope)));
  const target = $derived(writable.find((f) => f.path === agents.target[agents.selected]) ?? writable[0]);
  const shown = $derived(cfg?.files.find((f) => f.path === agents.file[agents.selected]) ?? target);
  const blocked = $derived(!target ? "" : target.error ? "error" : agents.drafts[target.path] !== undefined ? "draft" : "");

  /** Terminal tabs running each agent right now: they keep the settings they started with. */
  const running = $derived.by(() => {
    const n: Partial<Record<Provider, number>> = {};
    for (const t of L.panes(app.root).flatMap((p) => p.tabs)) {
      const p = t.kind === "term" && t.agent ? agentProvider[t.agent] : undefined;
      if (p) n[p] = (n[p] ?? 0) + 1;
    }
    return n;
  });
  const unsaved = (p: Provider) => !!agents.scan?.agents.find((a) => a.agent === p)?.files.some((f) => agents.drafts[f.path] !== undefined);

  function scopeHint(f: ConfigFile) {
    if (f.scope === "global") return `Every project on this Mac. ${short(f.path)}`;
    if (f.scope === "local") return `Only you, only in ${folder}; Claude keeps it out of git. ${short(f.path)}`;
    const shared = agents.selected === "claude" ? "Committed with the project, so everyone on it gets these." : `Only in ${folder}.`;
    const trust = agents.selected === "codex" ? " Codex reads it only in projects you trust." : "";
    return `${shared}${trust} ${short(f.path)}`;
  }

  const show = (v: unknown) => (typeof v === "string" ? v : JSON.stringify(v));
  /** Where the value that applies comes from, when it isn't this file. */
  function note(field: Field, f: ConfigFile) {
    const files = cfg?.files ?? [];
    const eff = effective(files, field.key);
    const here = getPath(f.values, field.key);
    if (eff && eff.file !== f && files.indexOf(eff.file) < files.indexOf(f))
      return { locked: eff.file.readOnly, text: `${eff.file.readOnly ? "Locked" : "Overridden"} by ${eff.file.label}: ${show(eff.value)}` };
    if (here === undefined) return { locked: false, text: eff ? `Not set here; ${eff.file.label} sets ${show(eff.value)}` : "Not set; the agent's default applies" };
    return null;
  }

  /** Model IDs to offer in a model field: the ones the agent lists, for the field its efforts follow. */
  function suggestions(field: Field) {
    const models = cfg?.models ?? [];
    const isModel = models.length && fields.some((x) => x.effortOf?.join(".") === field.key.join("."));
    return isModel ? models.map((m) => m.id) : (field.suggest ?? []);
  }

  function change(field: Field, input: string) {
    if (!target || blocked) return;
    const value = parseInput(field, input);
    if (value === getPath(target.values, field.key)) return;
    setValue(target, field.key, value);
  }

  function onKey(e: KeyboardEvent) {
    if (e.key !== "Escape" || e.defaultPrevented) return;
    if (!(e.target as Element).closest?.("input, textarea, select, .cm-editor")) closeAgents();
  }
</script>

<svelte:window onkeydown={onKey} onfocus={reload} />

<div class="agents slab">
  <div class="page">
    <header>
      <div>
        <h1>Agents</h1>
        <p class="sub">Settings for your coding agents, in {folder} and your home folder</p>
      </div>
      <div class="tools">
        <button class="icon-btn" onclick={reload} title="Refresh" aria-label="Refresh"><RefreshCw size={15} strokeWidth={1.5} /></button>
        <button class="icon-btn" onclick={closeAgents} title="Close (Esc)" aria-label="Close agents"><X size={16} strokeWidth={1.5} /></button>
      </div>
    </header>

    {#if agents.error}
      <div class="card error"><span>{agents.error}</span><button class="btn" onclick={reload}>Retry</button></div>
    {:else if !agents.scan}
      <p class="muted">Reading settings…</p>
    {:else}
      <div class="body">
        <nav class="list" aria-label="Agents">
          {#each PROVIDERS as p (p)}
            {@const a = agents.scan.agents.find((x) => x.agent === p)}
            <button class="row" class:on={agents.selected === p} onclick={() => ((agents.selected = p), (agents.formError = ""))}>
              <ProviderIcon provider={p} size={15} />
              <span class="name">{providerName[p]}</span>
              {#if unsaved(p)}<span class="unsaved" title="Unsaved changes"></span>{/if}
              {#if a?.files.some((f) => f.error)}<span class="warn-icon" title="A settings file doesn't parse"><TriangleAlert size={12} strokeWidth={2} /></span>{/if}
              {#if !a?.installed}<span class="faint">Not installed</span>{:else if running[p]}<span class="faint" title="Open tabs running it">{running[p]} running</span>{/if}
            </button>
          {/each}
        </nav>

        <div class="detail-scroll">
          <section class="detail">
            {#if cfg}
              <div class="title">
                <ProviderIcon provider={cfg.agent} size={18} />
                <h2>{name}</h2>
                {#if !cfg.installed}<span class="tag">Not installed</span>{/if}
                <div class="seg" role="tablist" aria-label="View">
                  <button role="tab" aria-selected={agents.tab === "settings"} class:on={agents.tab === "settings"} onclick={() => (agents.tab = "settings")}>Settings</button>
                  <button role="tab" aria-selected={agents.tab === "files"} class:on={agents.tab === "files"} onclick={() => (agents.tab = "files")}>Files<span class="count">{cfg.files.length}</span></button>
                </div>
              </div>
              <p class="note">
                Changes apply to new {name} sessions.
                {#if running[cfg.agent]}{running[cfg.agent]} open {running[cfg.agent] === 1 ? "tab runs" : "tabs run"} it; restart {running[cfg.agent] === 1 ? "it" : "them"} to pick changes up.{/if}
              </p>

              {#if agents.tab === "settings"}
                {#if !fields.length}
                  <p class="muted">{name} keeps no settings worth a form here. Its files are under <button class="link-btn" onclick={() => (agents.tab = "files")}>Files</button>.</p>
                {:else if target}
                  {#if writable.length > 1}
                    <div class="seg wide" role="tablist" aria-label="Where to save">
                      {#each writable as f (f.path)}
                        <button role="tab" aria-selected={target === f} class:on={target === f} onclick={() => ((agents.target[cfg.agent] = f.path), (agents.formError = ""))}>
                          {f.scope === "global" ? "Global" : f.label.replace("Project", `Project · ${folder}`)}
                        </button>
                      {/each}
                    </div>
                  {/if}
                  <p class="hint path-hint">{scopeHint(target)}</p>

                  {#if blocked === "error"}
                    <div class="warn"><TriangleAlert size={13} strokeWidth={2} /><span>Can't read {short(target.path)}: {target.error}. Fix it under Files first.</span></div>
                  {:else if blocked === "draft"}
                    <div class="warn"><TriangleAlert size={13} strokeWidth={2} /><span>This file has unsaved edits under Files. Save or discard them first.</span></div>
                  {/if}
                  {#if agents.formError}<div class="warn"><TriangleAlert size={13} strokeWidth={2} /><span>{agents.formError}</span></div>{/if}

                  <div class="fields" class:busy={agents.saving === target.path}>
                    {#each fields as field (field.key.join("."))}
                      {@const id = `f-${field.key.join("-")}`}
                      {@const here = getPath(target.values, field.key)}
                      {@const n = note(field, target)}
                      <div class="field-row">
                        <label class="label" for={id}>
                          <span>{field.label}</span>
                          <span class="hint">{field.help}</span>
                        </label>
                        <div class="control">
                          <div class="input-line">
                            {#if !fits(field, here)}
                              <code class="custom" title={show(here)}>{show(here)}</code>
                              <button class="btn small" onclick={() => ((agents.file[cfg.agent] = target.path), (agents.tab = "files"))}><FileCode size={12} strokeWidth={1.75} />Edit in file</button>
                            {:else if field.kind === "select" || field.kind === "bool"}
                              {@const options = field.kind === "bool" ? [{ value: "true", label: "On" }, { value: "false", label: "Off" }] : optionsFor(field, cfg.files, cfg.models, here)}
                              <select {id} disabled={!!blocked} value={here === undefined ? "" : String(here)} onchange={(e) => change(field, e.currentTarget.value)}>
                                <option value="">Not set</option>
                                {#each options as o (o.value)}<option value={o.value}>{o.label ?? o.value}</option>{/each}
                              </select>
                            {:else}
                              {@const list = suggestions(field)}
                              <input
                                {id}
                                type={field.kind === "number" ? "number" : "text"}
                                disabled={!!blocked}
                                value={here === undefined ? "" : String(here)}
                                placeholder={field.placeholder ?? "Not set"}
                                list={list.length ? `${id}-list` : undefined}
                                spellcheck="false"
                                autocomplete="off"
                                onchange={(e) => change(field, e.currentTarget.value)}
                                onkeydown={(e) => e.key === "Enter" && e.currentTarget.blur()}
                              />
                              {#if list.length}<datalist id="{id}-list">{#each list as s (s)}<option value={s}></option>{/each}</datalist>{/if}
                            {/if}
                            {#if here !== undefined && !blocked}
                              <button class="icon-btn" onclick={() => setValue(target, field.key, undefined)} title="Remove from this file" aria-label="Unset {field.label}"><RotateCcw size={13} strokeWidth={1.5} /></button>
                            {/if}
                          </div>
                          {#if n}<span class="source" class:locked={n.locked}>{#if n.locked}<Lock size={11} strokeWidth={1.75} />{/if}{n.text}</span>{/if}
                        </div>
                      </div>
                    {/each}
                  </div>
                {/if}
              {:else}
                <ul class="files">
                  {#each cfg.files as f (f.path)}
                    <li>
                      <button class="file-row" class:on={shown === f} onclick={() => (agents.file[cfg.agent] = f.path)}>
                        <span class="tag">{#if f.readOnly}<Lock size={10} strokeWidth={2} />{/if}{f.label}</span>
                        <span class="path" title={f.path}>{short(f.path)}</span>
                        {#if agents.drafts[f.path] !== undefined}<span class="unsaved" title="Unsaved changes"></span>{/if}
                        {#if f.error}<span class="warn-icon" title={f.error}><TriangleAlert size={12} strokeWidth={2} /></span>{/if}
                        <span class="state">{f.exists ? f.format.toUpperCase() : "Not created"}</span>
                      </button>
                    </li>
                  {/each}
                </ul>
                {#if shown}
                  {#if shown.error}<div class="warn"><TriangleAlert size={13} strokeWidth={2} /><span>{shown.error}</span></div>{/if}
                  {#if shown.readOnly}<p class="note">Set by your organization or device management: {name} applies it over your own settings.</p>{/if}
                  <div class="doc-wrap">{#key shown.path}<ConfigDoc file={shown} />{/key}</div>
                {/if}
              {/if}
            {/if}
          </section>
        </div>
      </div>
    {/if}
  </div>
</div>

<style>
  /* Header fixed; the list and the selected agent scroll on their own */
  /* Plane 2: a slab, like the panes it stands in for */
  .agents {
    grid-column: 2;
    grid-row: 2;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }
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
    margin-bottom: 22px;
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
  .muted {
    color: var(--muted-foreground);
    font-size: 13px;
  }
  .body {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(200px, 240px) minmax(0, 1fr);
    grid-template-rows: minmax(0, 1fr);
    gap: 24px;
  }
  .list,
  .detail-scroll {
    min-height: 0;
    overflow: auto;
    padding-bottom: 32px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 36px;
    padding: 0 10px;
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
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 13px;
    font-weight: 500;
  }
  .faint {
    margin-left: auto;
    color: var(--muted-foreground);
    font-size: 11.5px;
    white-space: nowrap;
  }
  .unsaved {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--gold);
  }
  .warn-icon {
    display: inline-flex;
    color: var(--destructive);
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
  .title h2 {
    margin: 0;
    font-size: 16px;
    font-weight: 600;
  }
  .title .seg {
    margin-left: auto;
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
    margin-top: 18px;
  }
  .seg.wide button {
    flex: 1;
    justify-content: center;
  }
  .count {
    color: var(--muted-foreground);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }
  .tag {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    flex: none;
    padding: 1px 8px;
    border: 1px solid var(--border);
    border-radius: 9999px;
    color: var(--muted-foreground);
    font-size: 11px;
    white-space: nowrap;
  }
  .note,
  .hint {
    color: var(--muted-foreground);
    font-size: 12px;
  }
  .note {
    margin: 10px 0 0;
  }
  .path-hint {
    margin: 8px 0 0;
  }
  .warn {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    margin-top: 12px;
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
  .fields {
    margin-top: 10px;
    transition: opacity var(--dur-quick) var(--ease-standard);
  }
  .fields.busy {
    opacity: 0.6;
    pointer-events: none;
  }
  .field-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1.1fr);
    gap: 20px;
    padding: 12px 0;
    border-bottom: 1px solid var(--border);
  }
  .field-row:last-child {
    border-bottom: 0;
  }
  .label {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: 13px;
    font-weight: 500;
  }
  .label .hint {
    font-weight: 400;
    line-height: 1.4;
  }
  .control {
    display: flex;
    flex-direction: column;
    gap: 5px;
    min-width: 0;
  }
  .input-line {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .input-line select,
  .input-line input {
    flex: 1;
    min-width: 0;
    height: 30px;
    padding: 0 10px;
    border: 1px solid var(--input);
    border-radius: var(--radius-sm);
    background: var(--background);
    color: var(--foreground);
    font: inherit;
    font-size: 13px;
  }
  .input-line input {
    font-family: var(--font-mono);
    font-size: 12.5px;
  }
  .input-line select:disabled,
  .input-line input:disabled {
    opacity: 0.5;
  }
  .custom {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    padding: 5px 8px;
    border-radius: var(--radius-sm);
    background: var(--muted);
    font-family: var(--font-mono);
    font-size: 12px;
  }
  .source {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--muted-foreground);
    font-size: 11.5px;
  }
  .source.locked {
    color: var(--gold-ring);
  }
  .files {
    margin: 16px 0 0;
    padding: 0;
    list-style: none;
  }
  .file-row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    min-height: 32px;
    padding: 0 8px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    text-align: left;
    cursor: pointer;
  }
  .file-row:hover {
    background: var(--accent);
  }
  .file-row.on {
    background: var(--gold-soft);
    box-shadow: inset 0 0 0 1px var(--gold-ring);
  }
  .path {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-mono);
    font-size: 12px;
  }
  .state {
    margin-left: auto;
    color: var(--muted-foreground);
    font-size: 11px;
    white-space: nowrap;
  }
  .doc-wrap {
    margin-top: 14px;
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
  .btn.small {
    height: 26px;
    padding: 0 8px;
    font-size: 12px;
  }
  .link-btn {
    padding: 0;
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    text-decoration: underline;
    text-underline-offset: 2px;
    cursor: pointer;
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
</style>
