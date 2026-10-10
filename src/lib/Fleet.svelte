<script lang="ts">
  import { untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { ArrowRight, Folder, FolderGit2, GitBranch, X } from "@lucide/svelte";
  import ProviderIcon from "./ProviderIcon.svelte";
  import { agentProvider, shells } from "./agents.svelte";
  import { closeFleet } from "./fleetView.svelte";
  import { byProject, fleetRows, fmtSpan, STATUSES, type Row, type Status } from "./fleet";
  import { providerName } from "./usageStats";
  import { app, ownerOf, refreshRepo } from "./state.svelte";
  import { nameOf, ownerWindow, showTab, tabsIn, ws } from "./workspaces.svelte";

  // Layouts are re-read on every poll (each second): other windows and moves between workspaces don't tell us.
  const rows = $derived.by(() => {
    void shells.polled;
    return fleetRows(ws.all.map((m) => ({ id: m.id, tabs: tabsIn(m.id) })), shells.all);
  });
  // A project is a repo, its worktrees included; else (or until the repo is looked up) the opened folder the agent is
  // in, or its own folder.
  const projectOf = (r: Row) => {
    const repo = app.repos[r.cwd];
    if (repo?.root) return repo.main || repo.root;
    const owner = ownerOf(r.cwd);
    return app.recent.includes(owner) ? owner : r.cwd;
  };
  const groups = $derived(byProject(rows, projectOf));
  const count = (s: Status) => rows.filter((r) => r.status === s).length;

  const heading: Record<Status, string> = { attention: "Needs you", busy: "Working", idle: "Idle", asleep: "Not started" };

  // Branch and worktree of each agent's folder: looked up once it shows up, and every 15s (agents switch branches).
  const asked = new Set<string>();
  $effect(() => {
    for (const r of rows) if (r.cwd && !asked.has(r.cwd)) asked.add(r.cwd), refreshRepo(r.cwd);
  });
  $effect(() => {
    const t = setInterval(() => untrack(() => new Set(rows.map((r) => r.cwd))).forEach((c) => c && refreshRepo(c)), 15_000);
    return () => clearInterval(t);
  });

  const base = (p: string) => p.slice(p.lastIndexOf("/") + 1);
  const name = (r: Row) => {
    const p = agentProvider[r.agent];
    return { title: r.label || (p ? providerName[p] : r.agent), sub: r.label && p ? providerName[p] : "" };
  };
  function where(r: Row) {
    if (!r.ws) return { text: "Detached", hint: "No workspace has this terminal's tab any more; its shell is still running" };
    const elsewhere = !ws.open.includes(r.ws) && ownerWindow(r.ws);
    const hint = r.ws === ws.active ? "This workspace" : ws.open.includes(r.ws) ? "Open in this window" : elsewhere ? "Open in another window" : "Not open: jumping opens it here";
    return { text: nameOf(r.ws), hint };
  }
  const when = (r: Row) =>
    r.since === null ? "not started" : `${{ attention: "waiting", busy: "working", idle: "idle", asleep: "" }[r.status]} ${fmtSpan(r.since)}`;

  async function end(r: Row) {
    if (await ask(`End the detached ${name(r).title} terminal? Its agent is stopped.`, { kind: "warning" })) invoke("pty_kill", { id: r.id });
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape" && !e.defaultPrevented && !(e.target as Element).closest?.("input, textarea")) closeFleet();
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="fleet slab">
  <div class="page">
    <header>
      <div>
        <h1>Fleet</h1>
        <p class="sub">
          {#if rows.length}
            {#each STATUSES.filter(count) as s, i (s)}{i ? " · " : ""}<span class:hot={s === "attention"}>{count(s)} {heading[s].toLowerCase()}</span>{/each}
          {:else}Every coding agent, in every workspace{/if}
        </p>
      </div>
      <button class="icon-btn" onclick={closeFleet} title="Close (Esc)" aria-label="Close fleet"><X size={16} strokeWidth={1.5} /></button>
    </header>

    {#if !rows.length}
      <div class="empty">
        <p>No agents running.</p>
        <p class="muted">Start <code>claude</code>, <code>codex</code>, <code>grok</code>, <code>agy</code> or <code>opencode</code> in a terminal and it shows up here, from any workspace or window.</p>
      </div>
    {/if}

    {#each groups as g (g.key)}
      <section>
        <h2 title={g.key}><FolderGit2 size={13} strokeWidth={1.5} />{base(g.key)} <span class="n">{g.rows.length}</span></h2>
        <div class="list">
          {#each g.rows as r (r.id)}
            {@const n = name(r)}
            {@const w = where(r)}
            {@const repo = app.repos[r.cwd]}
            <!-- The whole row jumps to the terminal: a button laid over it, under the End button -->
            <div class="row" class:waiting={r.status === "attention"} class:asleep={r.status === "asleep"} title={r.ws ? `Go to this terminal (${w.hint.toLowerCase()})` : w.hint}>
              {#if r.ws}<button class="main" onclick={() => r.ws && showTab(r.ws, r.id)} aria-label="Go to {n.title} in {w.text}"></button>{/if}
                <span class="state">
                  {#if agentProvider[r.agent]}<span class="agent" class:working={r.status === "busy"}><ProviderIcon provider={agentProvider[r.agent]} size={15} /></span>{/if}
                  {#if r.status === "attention"}<span class="attention"></span>{/if}
                </span>
                <span class="what">
                  <span class="title">{n.title}</span>
                  {#if n.sub}<span class="muted small">{n.sub}</span>{/if}
                </span>
                <span class="where">
                  <span class="ws" title={w.hint}>{w.text}</span>
                  <span class="loc" title={r.cwd}>
                    {#if (repo?.root || r.cwd) !== g.key}<span class="br"><Folder size={12} strokeWidth={1.5} />{base(repo?.root || r.cwd)}</span>{/if}
                    {#if repo?.branch}<span class="br"><GitBranch size={12} strokeWidth={1.5} />{repo.branch}</span>{/if}
                    {#if repo?.main}<span class="wt" title="Worktree of {repo.main}">worktree</span>{/if}
                  </span>
                </span>
                <span class="when muted small">{when(r)}</span>
                {#if r.ws}<span class="go"><ArrowRight size={14} strokeWidth={1.5} /></span>{:else}<button class="btn" onclick={() => end(r)}>End</button>{/if}
            </div>
          {/each}
        </div>
      </section>
    {/each}
  </div>
</div>

<style>
  /* Plane 2: a slab, like the panes it stands in for */
  .fleet {
    grid-column: 2;
    grid-row: 2;
    min-width: 0;
    overflow: auto;
    /* Room for the scrollbar on both sides, so the column stays centred (and as wide) whether or not it scrolls */
    scrollbar-gutter: stable both-edges;
  }
  .page {
    box-sizing: border-box; /* like Skills and Agents: --page-width includes the padding */
    max-width: var(--page-width);
    margin: 0 auto;
    padding: 28px 32px 48px;
  }
  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    margin-bottom: 8px;
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
  .hot {
    color: var(--foreground);
    font-weight: 500;
  }
  section {
    margin-top: 24px;
  }
  h2 {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0 0 8px;
    font-size: 13px;
    font-weight: 600;
  }
  h2 > :global(svg) {
    color: var(--muted-foreground);
  }
  .n {
    color: var(--muted-foreground);
    font-weight: 500;
  }
  .muted {
    color: var(--muted-foreground);
  }
  .small {
    font-size: 12px;
  }
  .empty {
    margin-top: 48px;
    text-align: center;
    font-size: 13px;
  }
  .empty p {
    margin: 0 0 6px;
  }
  code {
    font-size: 12px;
  }
  .list {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--card);
    overflow: hidden;
  }
  /* One line per agent: status · what it's on · where · how long · go */
  .row {
    position: relative;
    display: grid;
    grid-template-columns: 30px minmax(0, 1fr) minmax(0, 1.1fr) 92px 48px;
    align-items: center;
    gap: 12px;
    padding: 9px 10px 9px 14px;
    font-size: 13px;
  }
  .row + .row {
    border-top: 1px solid var(--border);
  }
  .row.asleep > :not(.main) {
    opacity: 0.6;
  }
  .row > :not(.main, .btn) {
    pointer-events: none;
  }
  .main {
    position: absolute;
    inset: 0;
    border: 0;
    background: none;
    cursor: pointer;
  }
  .row:has(.main:hover),
  .row:has(.main:focus-visible) {
    background: var(--accent);
  }
  .main:focus-visible {
    outline: none;
  }
  .state {
    display: flex;
    align-items: center;
    gap: 5px;
  }
  .what,
  .where {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .title,
  .ws,
  .loc {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .title {
    font-weight: 500;
  }
  .loc {
    display: flex;
    align-items: center;
    gap: 4px;
    color: var(--muted-foreground);
    font-size: 12px;
  }
  .loc :global(svg) {
    flex: none;
  }
  .br {
    display: contents;
  }
  .br + .br > :global(svg) {
    margin-left: 6px;
  }
  .wt {
    flex: none;
    margin-left: 4px;
    padding: 0 5px;
    border: 1px solid var(--border);
    border-radius: 9999px;
    font-size: 10.5px;
  }
  .when {
    text-align: right;
    white-space: nowrap;
  }
  .row.waiting .when {
    color: var(--foreground);
  }
  .go {
    display: flex;
    justify-content: flex-end;
    color: var(--muted-foreground);
    opacity: 0;
  }
  .row:has(.main:hover) .go,
  .row:has(.main:focus-visible) .go {
    opacity: 1;
  }
  .btn {
    position: relative;
    justify-self: end;
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--input);
    border-radius: var(--radius-sm);
    background: var(--background);
    color: var(--foreground);
    font-size: 12px;
    cursor: pointer;
  }
  .btn:hover {
    background: var(--accent);
  }
  /* Same marks as the tabs: the icon breathes while working; an ember dot when it needs you */
  .agent {
    display: flex;
  }
  .agent.working :global(svg) {
    animation: breathe 2.4s ease-in-out infinite;
  }
  @keyframes breathe {
    0%,
    100% {
      opacity: 0.55;
      filter: none;
    }
    50% {
      opacity: 1;
      filter: drop-shadow(0 0 3px color-mix(in oklch, var(--gold) 70%, transparent));
    }
  }
  .attention {
    width: 7px;
    height: 7px;
    border-radius: 9999px;
    background: oklch(0.7 0.17 50);
    box-shadow: 0 0 0 2px oklch(0.7 0.17 50 / 25%);
  }
  @media (prefers-reduced-motion: reduce) {
    .agent.working :global(svg) {
      animation: none;
      filter: drop-shadow(0 0 2px color-mix(in oklch, var(--gold) 70%, transparent));
    }
  }
</style>
