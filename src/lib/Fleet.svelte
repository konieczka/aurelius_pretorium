<script lang="ts">
  import { untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { GitBranch, MessageCircleQuestion, ShieldQuestion, X } from "@lucide/svelte";
  import ProviderIcon from "./ProviderIcon.svelte";
  import { agentProvider, shells } from "./agents.svelte";
  import { byProject, fleetRows, fmtSpan, STATUSES, type Row, type Status } from "./fleet";
  import { providerName } from "./usageStats";
  import { app, ownerOf, refreshRepo } from "./state.svelte";
  import { nameOf, ownerWindow, showTab, tabsIn, ws } from "./workspaces.svelte";

  // The sidebar's Fleet mode: every coding agent, in every workspace and window, by project.
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
  const when = (r: Row) => (r.since === null ? "" : fmtSpan(r.since));
  const state = (r: Row) => (r.since === null ? "not started" : `${{ attention: "waiting", busy: "working", idle: "idle", asleep: "" }[r.status]} ${fmtSpan(r.since)}`);
  // What it waits on you for: a prompt that's up, or (out of sight) a finished turn or a bell.
  const waitsFor = (r: Row) =>
    r.asking === "approval" ? { icon: ShieldQuestion, text: "Waiting for your approval" } :
    r.asking === "question" ? { icon: MessageCircleQuestion, text: "Asking you a question" } :
    r.status === "attention" ? { icon: null, text: "Your turn" } :
    null;

  function hint(r: Row) {
    const n = name(r);
    const w = where(r);
    const repo = app.repos[r.cwd];
    return [n.sub ? `${n.title} (${n.sub})` : n.title, state(r), `${w.text}: ${w.hint.toLowerCase()}`, r.cwd + (repo?.branch ? ` @ ${repo.branch}` : "")].join("\n");
  }

  async function end(r: Row) {
    if (await ask(`End the detached ${name(r).title} terminal? Its agent is stopped.`, { kind: "warning" })) invoke("pty_kill", { id: r.id });
  }
</script>

<div class="fleet">
  {#if rows.length}
    <p class="sum">
      {#each STATUSES.filter(count) as s, i (s)}{i ? " · " : ""}<span class:hot={s === "attention"}>{count(s)} {heading[s].toLowerCase()}</span>{/each}
    </p>
  {:else}
    <div class="empty">
      <p>No agents running.</p>
      <p class="small">Start <code>claude</code>, <code>codex</code>, <code>grok</code>, <code>agy</code> or <code>opencode</code> in a terminal and it shows up here, from any workspace or window.</p>
    </div>
  {/if}

  {#each groups as g (g.key)}
    <section>
      <h3 title={g.key}><span class="pname">{base(g.key)}</span><span class="n">{g.rows.length}</span></h3>
      <div class="cards">
        {#each g.rows as r (r.id)}
          {@const n = name(r)}
          {@const w = where(r)}
          {@const a = waitsFor(r)}
          {@const repo = app.repos[r.cwd]}
          <!-- The whole card jumps to the terminal: a button laid over it, under the End button -->
          <div class="card" class:waiting={r.status === "attention"} class:asleep={r.status === "asleep"} title={hint(r)}>
            {#if r.ws}<button class="main" onclick={() => r.ws && showTab(r.ws, r.id)} aria-label="Go to {n.title} in {w.text}"></button>{/if}
            <span class="agent" class:working={r.status === "busy" && !r.asking}>
              {#if agentProvider[r.agent]}<ProviderIcon provider={agentProvider[r.agent]} size={14} />{/if}
            </span>
            <span class="title">{n.title}</span>
            {#if r.ws}<span class="when">{when(r)}</span>
            {:else}<button class="end" onclick={() => end(r)} title="End this terminal" aria-label="End {n.title}"><X size={12} strokeWidth={1.5} /></button>{/if}
            {#if a}
              <span class="ask" class:prompt={r.asking}>
                {#if a.icon}<a.icon size={12} strokeWidth={2} />{:else}<span class="dot"></span>{/if}{a.text}
              </span>
            {/if}
            <span class="loc">
              <span class="ws">{w.text}</span>
              {#if (repo?.root || r.cwd) !== g.key}<span>·</span><span class="dir">{base(repo?.root || r.cwd)}</span>{/if}
              {#if repo?.branch}<span class="br"><GitBranch size={11} strokeWidth={1.5} /><span>{repo.branch}</span></span>{/if}
            </span>
          </div>
        {/each}
      </div>
    </section>
  {/each}
</div>

<style>
  .fleet {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 2px 0 12px;
    font-size: 13px;
  }
  .sum {
    margin: 0;
    padding: 0 6px;
    color: var(--muted-foreground);
    font-size: 12px;
  }
  .hot {
    color: var(--foreground);
    font-weight: 500;
  }
  .empty {
    padding: 16px 12px;
    color: var(--muted-foreground);
    text-align: center;
  }
  .empty p {
    margin: 0 0 8px;
  }
  .small {
    font-size: 12px;
    line-height: 1.45;
  }
  code {
    font-size: 11.5px;
  }
  /* Like Source Control's section heads */
  h3 {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    margin: 0;
    padding: 0 6px;
    color: var(--muted-foreground);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
  .pname {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .n {
    font-weight: 500;
  }
  .cards {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  /* A card per agent, as tall as it needs: mark · what it's on (all of it) · how long / what it waits on you for / where
   it is, and on which branch */
  .card {
    position: relative;
    display: grid;
    grid-template-columns: 14px minmax(0, 1fr) auto;
    align-items: start;
    column-gap: 8px;
    row-gap: 4px;
    padding: 8px 8px 8px 9px;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--card);
    color: var(--sidebar-foreground);
    transition: background-color var(--dur-quick) var(--ease-standard);
  }
  .card.waiting {
    border-color: oklch(0.7 0.17 50 / 45%);
  }
  .card.asleep > :not(.main) {
    opacity: 0.6;
  }
  .card > :not(.main, .end) {
    pointer-events: none;
  }
  .main {
    position: absolute;
    inset: 0;
    border: 0;
    border-radius: inherit;
    background: none;
    cursor: pointer;
  }
  .card:has(.main:hover),
  .card:has(.main:focus-visible) {
    background: var(--accent);
  }
  .card:has(.main:focus-visible) {
    box-shadow: 0 0 0 2px var(--gold-ring);
  }
  .main:focus-visible {
    outline: none;
  }
  .agent {
    display: flex;
    height: 18px;
    align-items: center;
  }
  .title {
    font-weight: 500;
    line-height: 18px;
    overflow-wrap: anywhere;
  }
  .when {
    color: var(--muted-foreground);
    font-size: 11.5px;
    line-height: 18px;
    font-variant-numeric: tabular-nums;
  }
  .card.waiting .when {
    color: var(--foreground);
  }
  .ask,
  .loc {
    grid-column: 2 / -1;
  }
  /* What it waits on you for: a prompt in the ember of the attention dots, a finished turn quieter */
  .ask {
    display: flex;
    align-items: center;
    gap: 5px;
    justify-self: start;
    color: var(--foreground);
    font-size: 12px;
    font-weight: 500;
  }
  .ask.prompt {
    padding: 2px 8px 2px 6px;
    border-radius: 9999px;
    background: oklch(0.7 0.17 50 / 16%);
    color: oklch(0.55 0.16 50);
  }
  :global(.dark) .ask.prompt {
    color: oklch(0.8 0.14 55);
  }
  .ask :global(svg) {
    flex: none;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 9999px;
    background: oklch(0.7 0.17 50);
    box-shadow: 0 0 0 2px oklch(0.7 0.17 50 / 25%);
  }
  .loc {
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
    color: var(--muted-foreground);
    font-size: 11.5px;
    white-space: nowrap;
  }
  .ws,
  .dir {
    flex: none;
    max-width: 50%;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .br {
    display: flex;
    align-items: center;
    gap: 3px;
    min-width: 0;
    margin-left: 4px;
  }
  .br :global(svg) {
    flex: none;
  }
  .br span {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .end {
    position: relative;
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    margin: -1px -2px 0 0;
    padding: 0;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--muted-foreground);
    cursor: pointer;
  }
  .end:hover {
    background: var(--hover);
    color: var(--foreground);
  }
  /* Same mark as the tabs: the icon breathes while working */
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
  @media (prefers-reduced-motion: reduce) {
    .agent.working :global(svg) {
      animation: none;
      filter: drop-shadow(0 0 2px color-mix(in oklch, var(--gold) 70%, transparent));
    }
  }
</style>
