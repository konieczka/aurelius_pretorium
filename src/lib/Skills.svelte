<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { ask, message } from "@tauri-apps/plugin-dialog";
  import { FileText, FolderOpen, Link2, Plus, RefreshCw, Search, Trash2, TriangleAlert, Unlink, X } from "@lucide/svelte";
  import ProviderIcon from "./ProviderIcon.svelte";
  import { app, openFile } from "./state.svelte";
  import { closeSkills, loadSkills, skills } from "./skillsView.svelte";
  import { agentsOf, clashes, listNames, lostWithout, matches, shortPath, validName, type Place, type Scope, type Skill } from "./skills";
  import { PROVIDERS, providerName, type Provider } from "./usageStats";

  const reload = () => loadSkills(app.workspace);
  $effect(() => {
    app.workspace; // rescan on open and when the sidebar switches folders
    reload();
  });

  const all = $derived(skills.scan?.skills ?? []);
  const locations = $derived(skills.scan?.locations ?? []);
  const folder = $derived(app.workspace.split("/").pop());
  const shown = $derived(all.filter((k) => matches(k, skills.query, skills.agent)));
  const groups = $derived([
    { scope: "project", title: `Project · ${folder}`, items: shown.filter((k) => k.scope === "project") },
    { scope: "global", title: "Global", items: shown.filter((k) => k.scope === "global") },
    { scope: "managed", title: "Built-in & synced", items: shown.filter((k) => k.scope === "managed") },
  ]);
  const selected = $derived(shown.find((k) => k.dir === skills.selected) ?? groups.flatMap((g) => g.items)[0]);

  const short = (p: string) => shortPath(p, skills.scan?.home ?? "", app.workspace);
  const names = (a: Provider[]) => listNames(a.map((p) => providerName[p]));
  const managedLabel = (k: Skill) => locations.find((l) => l.dir === k.places[0]?.location)?.label ?? "Built-in";
  const scopeLabel = (k: Skill) => (k.scope === "project" ? "Project" : k.scope === "global" ? "Global" : managedLabel(k));
  const flagged = (k: Skill) => k.problems.length > 0 || clashes(k, all).length > 0;

  async function run(f: () => Promise<unknown>) {
    try {
      await f();
    } catch (e) {
      message(String(e), { kind: "error" });
    }
    await reload();
  }

  /** The whole skill: links to it are removed, its own folder goes to the Trash. */
  async function remove(k: Skill) {
    const links = k.places.filter((p) => p.link).length;
    const own = k.places.some((p) => !p.link);
    const linkText = links === 1 ? "the link to it is removed" : `the ${links} links to it are removed`;
    const text = own
      ? `${names(k.agents)} will no longer see it. Its folder goes to the Trash${links ? `, and ${linkText}` : ""}.`
      : `This removes the ${links === 1 ? "link" : "links"}; the folder ${short(k.dir)} stays.`;
    if (!(await ask(text, { title: `Delete “${k.name}”?`, kind: "warning", okLabel: "Delete", cancelLabel: "Cancel" }))) return;
    await run(() => invoke("skill_delete", { project: app.workspace, paths: k.places.map((p) => p.path) }));
  }

  async function unlink(k: Skill, p: Place) {
    const lost = lostWithout(k, p, locations);
    const text = lost.length
      ? `${names(lost)} will no longer see “${k.name}”. The skill itself stays in ${short(k.dir)}.`
      : `Every agent still sees “${k.name}” through its other folders.`;
    if (!(await ask(text, { title: `Remove the link in ${short(p.location)}?`, kind: "warning", okLabel: "Remove", cancelLabel: "Cancel" }))) return;
    await run(() => invoke("skill_delete", { project: app.workspace, paths: [p.path] }));
  }

  // Add form
  let name = $state("");
  let description = $state("");
  let formError = $state("");
  const nameError = $derived(name && !validName(name) ? "Lowercase letters, digits and single hyphens, up to 64 characters" : "");
  const targets = $derived(locations.filter((l) => l.scope === skills.creating?.scope));
  const shared = $derived(skills.scan?.shared.find((s) => s.scope === skills.creating?.scope));

  function startCreate() {
    skills.creating = { scope: "project", dir: null };
    name = description = formError = "";
  }
  function setScope(scope: Scope) {
    if (skills.creating) skills.creating = { scope, dir: null };
  }

  async function create(e: SubmitEvent) {
    e.preventDefault();
    const c = skills.creating;
    if (!c || !validName(name)) return;
    try {
      const file = await invoke<string>("skill_create", { project: app.workspace, scope: c.scope, dir: c.dir, name, description });
      skills.creating = null;
      await reload();
      skills.selected = file.slice(0, file.lastIndexOf("/"));
      openFile(file); // a fresh skill is all template: on to writing it
    } catch (err) {
      formError = String(err);
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key !== "Escape" || e.defaultPrevented) return;
    if (skills.creating) skills.creating = null;
    else if (!(e.target as Element).closest?.("input, textarea")) closeSkills();
  }
</script>

<svelte:window onkeydown={onKey} onfocus={reload} />

<div class="skills">
  <div class="page">
    <header>
      <div>
        <h1>Skills</h1>
        <p class="sub">What your coding agents can load, in {folder} and your home folder</p>
      </div>
      <div class="tools">
        <button class="btn" onclick={startCreate}><Plus size={14} strokeWidth={1.75} />New skill</button>
        <button class="icon-btn" onclick={reload} title="Refresh" aria-label="Refresh"><RefreshCw size={15} strokeWidth={1.5} /></button>
        <button class="icon-btn" onclick={closeSkills} title="Close (Esc)" aria-label="Close skills"><X size={16} strokeWidth={1.5} /></button>
      </div>
    </header>

    <div class="filters">
      <label class="search">
        <Search size={14} strokeWidth={1.5} />
        <input placeholder="Search skills" bind:value={skills.query} spellcheck="false" />
      </label>
      <div class="seg" role="tablist" aria-label="Agent">
        <button role="tab" aria-selected={!skills.agent} class:on={!skills.agent} onclick={() => (skills.agent = null)}>All</button>
        {#each PROVIDERS as p (p)}
          <button role="tab" aria-selected={skills.agent === p} class:on={skills.agent === p} onclick={() => (skills.agent = p)} title="Skills {providerName[p]} sees">
            <ProviderIcon provider={p} size={13} />{providerName[p].split(" ")[0]}
            <span class="count">{all.filter((k) => k.agents.includes(p)).length}</span>
          </button>
        {/each}
      </div>
    </div>

    {#if skills.error}
      <div class="card error"><span>{skills.error}</span><button class="btn" onclick={reload}>Retry</button></div>
    {:else if !skills.scan}
      <p class="muted">Looking for skills…</p>
    {:else}
      <div class="body">
        <nav class="list" aria-label="Skills">
          {#each groups as g (g.scope)}
            {#if g.items.length || g.scope === "project"}
              <h2>{g.title}<span class="count">{g.items.length}</span></h2>
              {#each g.items as k (k.dir)}
                <button class="row" class:on={!skills.creating && selected === k} onclick={() => ((skills.selected = k.dir), (skills.creating = null))}>
                  <span class="row-top">
                    <span class="name">{k.name}</span>
                    {#if k.places.some((p) => p.link)}<span class="faint" title="Linked into more than one folder"><Link2 size={12} strokeWidth={1.75} /></span>{/if}
                    {#if flagged(k)}<span class="warn-icon" title="Has problems"><TriangleAlert size={12} strokeWidth={2} /></span>{/if}
                    <span class="icons">{#each k.agents as a (a)}<ProviderIcon provider={a} size={12} />{/each}</span>
                  </span>
                  <span class="row-desc">{k.description || "No description"}</span>
                </button>
              {:else}
                <p class="empty">{skills.query || skills.agent ? "None match." : "No skills in this project yet."}</p>
              {/each}
            {/if}
          {/each}
        </nav>

        <section class="detail">
          {#if skills.creating}
            {@const c = skills.creating}
            <form onsubmit={create}>
              <h2 class="form-title">New skill</h2>
              <div class="seg wide" role="tablist" aria-label="Scope">
                <button type="button" role="tab" aria-selected={c.scope === "project"} class:on={c.scope === "project"} onclick={() => setScope("project")}>Project · {folder}</button>
                <button type="button" role="tab" aria-selected={c.scope === "global"} class:on={c.scope === "global"} onclick={() => setScope("global")}>Global</button>
              </div>
              <p class="hint">{c.scope === "project" ? `Only in ${folder}; you can commit it with the project.` : "In every project on this Mac."}</p>

              <label class="field">
                <span>Name</span>
                <!-- svelte-ignore a11y_autofocus -->
                <input
                  value={name}
                  oninput={(e) => (name = e.currentTarget.value.toLowerCase().replace(/[\s_]+/g, "-"))}
                  placeholder="git-commit"
                  spellcheck="false"
                  autofocus
                />
                {#if nameError}<span class="field-error">{nameError}</span>{/if}
              </label>
              <label class="field">
                <span>Description</span>
                <textarea bind:value={description} rows="3" placeholder="What it does and when to use it, e.g. “Write commit messages in our format. Use when committing.”"></textarea>
                <span class="hint">Agents decide when to load a skill from this.</span>
              </label>

              <fieldset>
                <legend>Who sees it</legend>
                {#if shared}
                  <label class="target" class:on={c.dir === null}>
                    <input type="radio" name="target" checked={c.dir === null} onchange={() => (c.dir = null)} />
                    <span class="target-text">
                      <b>All agents</b>
                      <span class="hint">Saved in {short(shared.dir)}, linked from {shared.links.map(short).join(" and ")}</span>
                    </span>
                    <span class="icons">{#each agentsOf([shared.dir, ...shared.links], locations) as a (a)}<ProviderIcon provider={a} size={13} />{/each}</span>
                  </label>
                {/if}
                {#each targets as l (l.dir)}
                  <label class="target" class:on={c.dir === l.dir}>
                    <input type="radio" name="target" checked={c.dir === l.dir} onchange={() => (c.dir = l.dir)} />
                    <span class="target-text">
                      <span class="path">{short(l.dir)}</span>
                      <span class="hint">{names(l.agents)}</span>
                    </span>
                    <span class="icons">{#each l.agents as a (a)}<ProviderIcon provider={a} size={13} />{/each}</span>
                  </label>
                {/each}
              </fieldset>

              {#if formError}<div class="warn"><TriangleAlert size={13} strokeWidth={2} />{formError}</div>{/if}
              <div class="actions">
                <button class="btn primary" type="submit" disabled={!validName(name)}>Create and edit</button>
                <button class="btn" type="button" onclick={() => (skills.creating = null)}>Cancel</button>
              </div>
            </form>
          {:else if selected}
            {@const k = selected}
            <div class="title">
              <h2>{k.name}</h2>
              <span class="tag">{scopeLabel(k)}</span>
            </div>
            {#if k.description}<p class="desc selectable">{k.description}</p>{/if}

            {#each k.problems as p}
              <div class="warn"><TriangleAlert size={13} strokeWidth={2} /><span>{p}</span></div>
            {/each}
            {#each clashes(k, all) as c (c.other.dir)}
              <div class="warn">
                <TriangleAlert size={13} strokeWidth={2} />
                <span>Same name as the {scopeLabel(c.other).toLowerCase()} skill in {short(c.other.dir)}: {names(c.agents)} load only one of them.</span>
              </div>
            {/each}

            <h3>Used by</h3>
            <div class="agents">
              {#each k.agents as a (a)}<span class="agent"><ProviderIcon provider={a} />{providerName[a]}</span>{/each}
            </div>

            <h3>Folders</h3>
            <ul class="places">
              {#each k.places as p (p.path)}
                <li>
                  <span class="path selectable" title={p.path}>{short(p.path)}</span>
                  {#if p.link}<span class="tag small"><Link2 size={11} strokeWidth={1.75} />link</span>{/if}
                  <span class="icons">{#each agentsOf([p.location], locations) as a (a)}<ProviderIcon provider={a} size={13} />{/each}</span>
                  {#if p.link && p.scope !== "managed" && k.places.length > 1}
                    <button class="icon-btn" onclick={() => unlink(k, p)} title="Remove this link" aria-label="Remove link in {short(p.location)}"><Unlink size={13} strokeWidth={1.5} /></button>
                  {/if}
                </li>
              {/each}
            </ul>
            {#if k.places.every((p) => p.link) && k.file}<p class="note">Links to {short(k.dir)}</p>{/if}

            {#if k.file}
              <h3>Files</h3>
              <ul class="files">
                <li>SKILL.md</li>
                {#each k.files as f}<li>{f}</li>{/each}
              </ul>
            {/if}

            <div class="actions">
              <button class="btn primary" disabled={!k.file} onclick={() => openFile(k.file)}><FileText size={14} strokeWidth={1.5} />{k.readOnly ? "View" : "Edit"} SKILL.md</button>
              <button class="btn" onclick={() => invoke("reveal_path", { path: k.file || k.dir }).catch((e) => message(String(e), { kind: "error" }))}>
                <FolderOpen size={14} strokeWidth={1.5} />Reveal in Finder
              </button>
              {#if !k.readOnly}
                <button class="btn danger" onclick={() => remove(k)}><Trash2 size={14} strokeWidth={1.5} />Delete</button>
              {/if}
            </div>
            {#if k.readOnly}<p class="note">{managedLabel(k)}: managed by {names(k.agents)}, which may overwrite changes.</p>{/if}
          {:else}
            <p class="muted">{all.length ? "No skills match." : "No skills yet. Add one with New skill."}</p>
          {/if}
        </section>
      </div>
    {/if}
  </div>
</div>

<style>
  .skills {
    grid-column: 2;
    grid-row: 2;
    min-width: 0;
    overflow: auto;
    background: var(--background);
  }
  .page {
    max-width: 1120px;
    margin: 0 auto;
    padding: 28px 32px 48px;
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
    display: grid;
    grid-template-columns: minmax(260px, 360px) minmax(0, 1fr);
    gap: 24px;
    align-items: start;
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
  .faint {
    display: inline-flex;
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
    position: sticky;
    top: 0;
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
  .files {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .files li {
    padding: 2px 8px;
    border-radius: var(--radius-sm);
    background: var(--muted);
    font-family: var(--font-mono);
    font-size: 11.5px;
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
  .target-text b {
    font-weight: 500;
  }
  .target-text .hint {
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  @media (max-width: 860px) {
    .body {
      grid-template-columns: minmax(0, 1fr);
    }
    .detail {
      position: static;
    }
  }
</style>
