<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { ArrowDown, ArrowUp, ChevronRight, Cloud, GitBranch, Network, Tag } from "@lucide/svelte";
  import { app, openDiff } from "./state.svelte";
  import { git } from "./GitPanel.svelte";
  import { pick } from "./QuickPick.svelte";
  import { layout, LOG_FORMAT, parseLog, type Row } from "./graph";
  import { branchMenu, commitMenu } from "./gitActions";

  let open = $state(true);
  let all = $state(false); // all branches vs current branch + upstream
  let limit = $state(150);
  let rows = $state<Row[]>([]);
  let incoming = $state(new Set<string>());
  let outgoing = $state(new Set<string>());

  const run = (args: string[]) => invoke<string>("git", { cwd: git.root, args });
  const revs = (range: string) => run(["rev-list", "-n500", range]).then((o) => new Set(o.split("\n").filter(Boolean)), () => new Set<string>());

  $effect(() => {
    const s = git.status;
    void app.gitVersion; // refetch after commits, pulls, checkouts
    if (!open || !git.root || !s || s.oid === "(initial)") return void (rows = []);
    const up = s.upstream;
    let dead = false;
    Promise.all([
      run(["log", "--topo-order", `-n${limit}`, "--decorate=full", `--format=${LOG_FORMAT}`, ...(all ? ["--all"] : ["HEAD", ...(up ? ["@{u}"] : [])])]),
      up ? revs("HEAD..@{u}") : new Set<string>(),
      up ? revs("@{u}..HEAD") : new Set<string>(),
    ]).then(
      ([out, inc, outg]) => {
        if (dead) return;
        rows = layout(parseLog(out));
        incoming = inc;
        outgoing = outg;
      },
      () => {},
    );
    return () => (dead = true);
  });

  // Lane geometry (px). The checked-out commit's lane is gold (gold = active); others cycle the syntax hues.
  const LW = 12, H = 24, MAXL = 10;
  const colors = ["var(--syn-title)", "var(--syn-keyword)", "var(--syn-string)", "var(--syn-number)", "var(--syn-attr)", "var(--syn-deletion)"];
  const isHead = (refs: string[]) => refs.some((x) => x === "HEAD" || x.startsWith("HEAD -> "));
  const headLane = $derived(rows.find((r) => isHead(r.commit.refs))?.lane ?? 0);
  const color = (lane: number) => (lane === headLane ? "var(--gold)" : colors[(lane > headLane ? lane - 1 : lane) % colors.length]);
  const x = (lane: number) => 7 + Math.min(lane, MAXL) * LW;
  const y = (v: number) => (v * H) / 2;
  const lanes = $derived(Math.min(MAXL + 1, Math.max(1, ...rows.map((r) => r.width))));
  function d(s: Row["segs"][number]) {
    const [x1, y1, x2, y2] = [x(s.x1), y(s.y1), x(s.x2), y(s.y2)];
    const m = (y1 + y2) / 2;
    return x1 === x2 ? `M${x1} ${y1}V${y2}` : `M${x1} ${y1}C${x1} ${m} ${x2} ${m} ${x2} ${y2}`;
  }

  // Refs from --decorate=full: "HEAD -> refs/heads/main", "refs/remotes/origin/main", "tag: refs/tags/v1", "HEAD".
  type Badge = { name: string; kind: "head" | "branch" | "remote" | "tag" };
  function badges(refs: string[]): Badge[] {
    return refs.flatMap((r): Badge[] => {
      if (r.startsWith("HEAD -> ")) return [{ name: r.slice(19), kind: "head" }];
      if (r === "HEAD") return [{ name: "HEAD", kind: "head" }];
      if (r.startsWith("tag: ")) return [{ name: r.slice(15), kind: "tag" }];
      if (r.startsWith("refs/remotes/")) return r.endsWith("/HEAD") ? [] : [{ name: r.slice(13), kind: "remote" }];
      if (r.startsWith("refs/heads/")) return [{ name: r.slice(11), kind: "branch" }];
      return [];
    });
  }

  function ago(t: number) {
    const s = Date.now() / 1000 - t;
    for (const [n, u] of [[31536000, "y"], [2592000, "mo"], [604800, "w"], [86400, "d"], [3600, "h"], [60, "m"]] as const)
      if (s >= n) return `${Math.floor(s / n)}${u} ago`;
    return "just now";
  }

  /** Click a commit: pick one of its changed files, open its diff (parent -> commit). */
  async function showCommit(r: Row) {
    const c = r.commit;
    const args = c.parents.length > 1 ? ["diff", "--name-status", "-M", `${c.hash}^1`, c.hash] : ["diff-tree", "-r", "--root", "--no-commit-id", "--name-status", "-M", c.hash];
    const files = (await run(args).catch(() => ""))
      .split("\n")
      .filter(Boolean)
      .map((l) => {
        const [st, a, b] = l.split("\t");
        return { st: st[0], path: b ?? a, orig: b ? a : undefined };
      });
    const v = await pick(
      `${c.hash.slice(0, 7)} · ${c.subject}`,
      files.map((f, i) => {
        const at = f.path.lastIndexOf("/");
        return { label: f.path.slice(at + 1), detail: f.orig ? `${f.orig} → ${f.path}` : f.path.slice(0, Math.max(0, at)), hint: f.st, value: String(i) };
      }),
    );
    if (v === null) return;
    const f = files[+v];
    openDiff(`${git.root}/${f.path}`, false, f.orig, c.hash);
  }
</script>

<div class="section">
  <button class="shead" onclick={() => (open = !open)}>
    <span class="chev" class:open><ChevronRight size={12} strokeWidth={1.5} /></span>
    <span class="stitle">Graph</span>
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_interactive_supports_focus -->
    <span role="button" class="act" class:on={all} title={all ? "Showing all branches" : "Showing current branch"} onclick={(e) => (e.stopPropagation(), (all = !all))}>
      <Network size={13} strokeWidth={1.5} />
    </span>
  </button>
  {#if open}
    {#each rows as r (r.commit.hash)}
      {@const c = r.commit}
      {@const inc = incoming.has(c.hash)}
      {@const head = isHead(c.refs)}
      {@const bs = badges(c.refs)}
      <button
        class="commit"
        class:inc
        title={`${c.hash.slice(0, 7)} · ${c.author} · ${ago(c.time)} (${new Date(c.time * 1000).toLocaleString()})\n${c.subject}${bs.length ? "\n" + bs.map((b) => b.name).join(", ") : ""}`}
        onclick={() => showCommit(r)}
        oncontextmenu={(e) => (e.preventDefault(), commitMenu(c, head))}
      >
        <svg width={x(lanes - 1) + 7} height={H} class="lanes">
          {#each r.segs as s, i (i)}
            <path d={d(s)} stroke={color(s.y1 === 0 ? s.x1 : s.x2)} />
          {/each}
          {#if head}
            <circle cx={x(r.lane)} cy={y(1)} r="5" fill="var(--surface)" stroke={color(r.lane)} stroke-width="2" />
            <circle cx={x(r.lane)} cy={y(1)} r="2" fill={color(r.lane)} />
          {:else}
            <circle cx={x(r.lane)} cy={y(1)} r={c.parents.length > 1 ? 3 : 3.5} fill={inc ? "var(--surface)" : color(r.lane)} stroke={color(r.lane)} stroke-width="1.5" />
          {/if}
        </svg>
        <span class="subject">{c.subject}</span>
        {#each bs.slice(0, 2) as b (b.kind + b.name)}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <span class="badge {b.kind}" oncontextmenu={(e) => b.kind !== "tag" && (e.preventDefault(), e.stopPropagation(), branchMenu(b.name, b.kind))}>
            {#if b.kind === "tag"}<Tag size={10} strokeWidth={1.75} />{:else if b.kind === "remote"}<Cloud size={10} strokeWidth={1.75} />{:else}<GitBranch size={10} strokeWidth={1.75} />{/if}{b.name}
          </span>
        {/each}
        {#if bs.length > 2}<span class="badge">+{bs.length - 2}</span>{/if}
        {#if inc}<span class="io" title="Incoming (not pulled yet)"><ArrowDown size={11} strokeWidth={1.75} /></span>{/if}
        {#if outgoing.has(c.hash)}<span class="io" title="Outgoing (not pushed yet)"><ArrowUp size={11} strokeWidth={1.75} /></span>{/if}
      </button>
    {/each}
    {#if rows.length >= limit}
      <button class="more" onclick={() => (limit += 150)}>Load more…</button>
    {/if}
  {/if}
</div>

<style>
  .section {
    margin-top: 4px;
  }
  .shead {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    height: 26px;
    padding: 0 6px 0 2px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--muted-foreground);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    cursor: pointer;
  }
  .shead:hover {
    background: color-mix(in oklch, var(--sidebar-accent) 60%, transparent);
  }
  .stitle {
    flex: 1;
    text-align: left;
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
  .act {
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    border-radius: var(--radius-sm);
  }
  .act:hover,
  .act.on {
    background: var(--accent);
    color: var(--foreground);
  }
  .commit {
    display: flex;
    align-items: center;
    gap: 5px;
    width: 100%;
    height: 24px;
    padding: 0 6px 0 4px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--sidebar-foreground);
    font-size: 12.5px;
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
  }
  .commit:hover {
    background: color-mix(in oklch, var(--sidebar-accent) 60%, transparent);
  }
  .commit.inc .subject {
    color: var(--muted-foreground);
    font-style: italic;
  }
  .lanes {
    flex: none;
    overflow: visible;
  }
  .lanes path {
    fill: none;
    stroke-width: 1.75;
    stroke-linecap: round;
  }
  .subject {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .badge {
    display: inline-flex;
    flex: none;
    align-items: center;
    gap: 3px;
    max-width: 84px;
    height: 16px;
    padding: 0 5px;
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: 9999px;
    background: var(--muted);
    color: var(--muted-foreground);
    font-size: 10.5px;
    text-overflow: ellipsis;
  }
  .badge :global(svg) {
    flex: none;
  }
  /* Gold = the checked-out branch */
  .badge.head {
    border-color: color-mix(in oklch, var(--gold) 45%, transparent);
    background: var(--gold-soft);
    color: var(--gold-ring);
    font-weight: 500;
  }
  .badge.remote {
    background: none;
  }
  .io {
    display: grid;
    flex: none;
    color: var(--muted-foreground);
  }
  .more {
    width: 100%;
    height: 24px;
    border: 0;
    background: none;
    color: var(--muted-foreground);
    font-size: 12px;
    cursor: pointer;
  }
  .more:hover {
    color: var(--foreground);
  }
</style>
