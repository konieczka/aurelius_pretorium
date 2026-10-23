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
  const tally = (rs: Row[]) => STATUSES.map((s) => [s, rs.filter((r) => r.status === s).length] as const).filter(([, n]) => n);

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
      {#each STATUSES.filter(count) as s (s)}<span class="tally" class:hot={s === "attention"}><span class="glyph {s}"></span>{count(s)} {heading[s].toLowerCase()}</span>{/each}
    </p>
  {:else}
    <div class="empty">
      <p>No agents running.</p>
      <p class="small">Start <code>claude</code>, <code>codex</code>, <code>grok</code>, <code>agy</code>, <code>opencode</code> or <code>muse</code> in a terminal and it shows up here, from any workspace or window.</p>
    </div>
  {/if}

  {#each groups as g (g.key)}
    <!-- A card per project; its head counts the agents by state, in the same marks as the rows -->
    <section class="project" class:waiting={g.rows.some((r) => r.status === "attention")}>
      <h3 title={g.key}>
        <span class="pname">{base(g.key)}</span>
        <span class="counts">
          {#each tally(g.rows) as [s, n] (s)}<span class="tally" title="{n} {heading[s].toLowerCase()}"><span class="glyph {s}"></span>{n}</span>{/each}
        </span>
      </h3>
      <div class="rows">
        {#each g.rows as r (r.id)}
          {@const n = name(r)}
          {@const w = where(r)}
          {@const a = waitsFor(r)}
          {@const repo = app.repos[r.cwd]}
          <!-- The whole row jumps to the terminal: a button laid over it, under the End button -->
          <div class="row {r.status}" class:prompt={r.asking} title={hint(r)}>
            {#if r.ws}<button class="main" onclick={() => r.ws && showTab(r.ws, r.id)} aria-label="Go to {n.title} in {w.text}"></button>{/if}
            <span class="mark">
              {#if agentProvider[r.agent]}<ProviderIcon provider={agentProvider[r.agent]} size={13} />{/if}
            </span>
            <span class="title">{n.title}</span>
            {#if r.ws}<span class="when">{when(r)}</span>
            {:else}<button class="end" onclick={() => end(r)} title="End this terminal" aria-label="End {n.title}"><X size={12} strokeWidth={1.5} /></button>{/if}
            {#if a}
              <span class="ask">
                {#if a.icon}<a.icon size={12} strokeWidth={2} />{/if}{a.text}
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
  /* Three states, three channels, so they read apart even without colour: waiting is a still ember (filled, the row
     tinted), working is gold and moves (an arc circles the icon), idle is a plain hairline and a dimmed icon. Not
     started is idle's dashed outline. */
  .fleet {
    --ember: oklch(0.68 0.18 52);
    --ember-ink: oklch(0.52 0.16 48);
    --ember-wash: oklch(0.7 0.17 50 / 9%);
    --ember-edge: oklch(0.7 0.17 50 / 42%);
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 2px 0 12px;
    font-size: 13px;
  }
  :global(.dark) .fleet {
    --ember: oklch(0.78 0.16 58);
    --ember-ink: oklch(0.82 0.13 60);
    --ember-wash: oklch(0.75 0.16 55 / 11%);
    --ember-edge: oklch(0.75 0.16 55 / 40%);
  }
  .sum {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 12px;
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

  /* The state marks, small (tallies) */
  .tally {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-variant-numeric: tabular-nums;
  }
  .glyph {
    flex: none;
    width: 7px;
    height: 7px;
    box-sizing: border-box;
    border-radius: 9999px;
  }
  .glyph.attention {
    background: var(--ember);
    box-shadow: 0 0 0 2px color-mix(in oklch, var(--ember) 22%, transparent);
  }
  .glyph.busy {
    border: 1.5px solid var(--gold);
    background: color-mix(in oklch, var(--gold) 30%, transparent);
  }
  .glyph.idle {
    border: 1.5px solid var(--faint);
  }
  .glyph.asleep {
    border: 1.5px dotted var(--faint);
    opacity: 0.7;
  }

  /* A card per project: rows sit 4px inside it, so their corners stay concentric with its own */
  .project {
    --r-card: 14px;
    padding: 0 4px 4px;
    border-radius: var(--r-card);
    background: var(--card);
    box-shadow: inset 0 1px 0 var(--hl), 0 0 0 1px oklch(var(--shadow-tint) / 0.09), 0 6px 16px -12px oklch(var(--shadow-tint) / 0.35);
    transition: box-shadow var(--dur-base) var(--ease-out-quint);
  }
  .project.waiting {
    box-shadow: inset 0 1px 0 var(--hl), 0 0 0 1px var(--ember-edge), 0 6px 18px -12px color-mix(in oklch, var(--ember) 55%, transparent);
  }
  h3 {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 32px;
    margin: 0;
    padding: 0 8px;
    color: var(--foreground);
    font-size: 12.5px;
    font-weight: 600;
  }
  .pname {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .counts {
    display: flex;
    flex: none;
    gap: 9px;
    color: var(--muted-foreground);
    font-size: 11.5px;
    font-weight: 500;
  }
  .rows {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  /* A row per agent, as tall as it needs: mark · what it's on (all of it) · how long / what it waits on you for / where
   it is, and on which branch */
  .row {
    position: relative;
    display: grid;
    grid-template-columns: 22px minmax(0, 1fr) auto;
    align-items: start;
    column-gap: 9px;
    row-gap: 3px;
    padding: 7px 8px 8px 6px;
    border-radius: calc(var(--r-card) - 4px);
    color: var(--sidebar-foreground);
    transition: background-color var(--dur-quick) var(--ease-out-quint);
  }
  /* Hairlines between rows, from the text column on; none next to a tinted or hovered row */
  .row + .row::before {
    content: "";
    position: absolute;
    top: -1.5px;
    left: 37px;
    right: 8px;
    height: 1px;
    background: var(--border);
    opacity: 0.55;
  }
  .row.attention::before,
  .row.attention + .row::before,
  .row:has(.main:hover)::before,
  .row:has(.main:hover) + .row::before {
    opacity: 0;
  }
  .row.attention {
    background: var(--ember-wash);
  }
  .row.asleep > :not(.main, .mark) {
    opacity: 0.6;
  }
  .row > :not(.main, .end) {
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
  .row:has(.main:hover),
  .row:has(.main:focus-visible) {
    background: var(--hover);
  }
  .row.attention:has(.main:hover) {
    background: color-mix(in oklch, var(--ember-wash), var(--ember) 7%);
  }
  .row:has(.main:focus-visible) {
    box-shadow: 0 0 0 2px var(--gold-ring);
  }
  .main:focus-visible {
    outline: none;
  }

  /* The state mark, large: the agent's icon in a round socket */
  .mark {
    position: relative;
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    margin-top: -2px;
    border-radius: 9999px;
  }
  .mark::before {
    content: "";
    position: absolute;
    inset: 0;
    box-sizing: border-box;
    border-radius: inherit;
  }
  .mark :global(svg) {
    position: relative;
  }
  .idle .mark::before {
    border: 1px solid var(--border-strong);
  }
  .idle .mark :global(svg) {
    opacity: 0.6;
    filter: saturate(0.4);
  }
  .asleep .mark::before {
    border: 1px dashed var(--border-strong);
  }
  .asleep .mark :global(svg) {
    opacity: 0.4;
    filter: grayscale(1);
  }
  .attention .mark {
    background: color-mix(in oklch, var(--ember) 16%, transparent);
  }
  .attention .mark::before {
    border: 1.5px solid var(--ember);
  }
  /* Working: a gold arc runs round the socket (a conic sweep, masked to a ring), over a faint full ring */
  .busy .mark {
    background: color-mix(in oklch, var(--gold) 10%, transparent);
  }
  .busy .mark::before {
    border: 1px solid color-mix(in oklch, var(--gold) 40%, transparent);
  }
  .busy .mark::after {
    content: "";
    position: absolute;
    inset: 0;
    padding: 2px;
    border-radius: inherit;
    background: conic-gradient(from 0deg, transparent 0 35%, color-mix(in oklch, var(--gold) 45%, transparent) 65%, var(--gold) 100%);
    -webkit-mask: linear-gradient(#000 0 0) content-box, linear-gradient(#000 0 0);
    mask: linear-gradient(#000 0 0) content-box, linear-gradient(#000 0 0);
    -webkit-mask-composite: xor;
    mask-composite: exclude;
    animation: orbit 1.6s cubic-bezier(0.45, 0.15, 0.35, 0.85) infinite;
  }
  @keyframes orbit {
    to {
      transform: rotate(1turn);
    }
  }

  .title {
    font-weight: 500;
    line-height: 18px;
    overflow-wrap: anywhere;
  }
  .idle .title {
    color: var(--muted-foreground);
  }
  .when {
    color: var(--faint);
    font-size: 11.5px;
    line-height: 18px;
    font-variant-numeric: tabular-nums;
  }
  .busy .when {
    color: var(--muted-foreground);
  }
  .attention .when {
    color: var(--ember-ink);
    font-weight: 500;
  }
  .ask,
  .loc {
    grid-column: 2 / -1;
  }
  /* What it waits on you for: a prompt as a pill in ember, a finished turn as plain ember text */
  .ask {
    display: flex;
    align-items: center;
    gap: 5px;
    justify-self: start;
    color: var(--ember-ink);
    font-size: 12px;
    font-weight: 500;
  }
  .prompt .ask {
    margin: 1px 0;
    padding: 2px 8px 2px 6px;
    border-radius: 9999px;
    background: color-mix(in oklch, var(--ember) 17%, transparent);
  }
  .ask :global(svg) {
    flex: none;
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
  .idle .loc {
    color: var(--faint);
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
  /* The global reduced-motion rule shortens animations to 1ms, which would make an infinite one strobe: the arc
     becomes a still, full gold ring */
  @media (prefers-reduced-motion: reduce) {
    .busy .mark::after {
      animation: none;
      background: var(--gold);
    }
  }
</style>
