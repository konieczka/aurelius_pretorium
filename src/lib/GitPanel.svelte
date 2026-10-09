<script module lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { ask, message } from "@tauri-apps/plugin-dialog";
  import { app, foreignOwner, openDiff, openFile, openWorkspace, runInTerminal } from "./state.svelte";
  import * as L from "./layout";
  import { pick, prompt } from "./QuickPick.svelte";
  import { changes, conflicts, emptyDeco, parseStatus, staged, treeDecorations, type GitFile, type GitStatus } from "./gitStatus";

  export const git = $state({
    root: "", // repo toplevel ("" = workspace is not in a repo)
    status: null as GitStatus | null,
    deco: emptyDeco(), // file tree colors / letters / muted ignored entries
    busy: "", // label of the running remote op (pull/push/sync)
    loaded: false,
    op: "" as "" | "merge" | "rebase" | "cherry-pick", // operation in progress (possibly stopped on conflicts)
    opInfo: "",
  });
  class Cancelled extends Error {}
  let lastRaw = "";

  const run = (args: string[]) => invoke<string>("git", { cwd: git.root || app.workspace, args });
  const fail = (e: unknown) => message(String(e), { kind: "error" });
  export const noHead = () => git.status?.oid === "(initial)";
  export const branchName = () => (git.status?.head === "(detached)" ? git.status.oid.slice(0, 7) : (git.status?.head ?? ""));
  export const changeCount = () => (git.status ? git.status.files.length : 0);

  /** Keeps status fresh (badge + panel). ponytail: polling, not a file watcher; fine for one repo. */
  export function watchGit() {
    refreshGit();
    const t = setInterval(() => document.hasFocus() && refreshGit(), 3000);
    window.addEventListener("focus", refreshGit);
    return () => (clearInterval(t), window.removeEventListener("focus", refreshGit));
  }

  export async function refreshGit() {
    if (!app.workspace) return;
    try {
      const [root, gitDir] = (await invoke<string>("git", { cwd: app.workspace, args: ["rev-parse", "--show-toplevel", "--absolute-git-dir"] })).trim().split("\n");
      git.root = root;
      const raw = await run(["status", "--porcelain=v2", "-z", "--branch", "--ignored=matching"]);
      git.status = parseStatus(raw);
      await detectOp(gitDir);
      // Status changed (e.g. an agent edited files): open diffs refetch, the tree picks up new files and recolors.
      if (raw !== lastRaw) {
        lastRaw = raw;
        git.deco = treeDecorations(git.status, root);
        app.gitVersion++;
        app.treeVersion++;
      }
    } catch {
      git.root = "";
      git.status = null;
      if (lastRaw) (lastRaw = ""), (git.deco = emptyDeco());
    }
    git.loaded = true;
    refreshBranches();
  }

  /** Branch of each other folder that has open tabs (tab hints); agents may switch them at any time. */
  async function refreshBranches() {
    const owners = new Set(L.panes(app.root).flatMap((p) => p.tabs).map(foreignOwner).filter(Boolean));
    for (const o of owners)
      app.branches[o] = await invoke<string>("git", { cwd: o, args: ["branch", "--show-current"] }).then((b) => b.trim(), () => "");
  }

  /** Merge / rebase / cherry-pick in progress? Read from the (per-worktree) git dir; missing files = not in progress. */
  async function detectOp(gitDir: string) {
    const read = (f: string) => invoke<string>("read_file", { path: `${gitDir}/${f}` }).then((t) => t.trim(), () => "");
    const [merge, rebaseHead, onto, msgnum, end, pick] = await Promise.all(
      ["MERGE_MSG", "rebase-merge/head-name", "rebase-merge/onto", "rebase-merge/msgnum", "rebase-merge/end", "CHERRY_PICK_HEAD"].map(read),
    );
    const mergeHead = merge && (await read("MERGE_HEAD"));
    [git.op, git.opInfo] = rebaseHead
      ? ["rebase", `Rebasing ${rebaseHead.replace("refs/heads/", "")} onto ${onto.slice(0, 7)}${end ? ` (${msgnum}/${end})` : ""}`]
      : mergeHead
        ? ["merge", merge.split("\n")[0]]
        : pick
          ? ["cherry-pick", `Cherry-picking ${pick.slice(0, 7)}`]
          : ["", ""];
  }

  /** Run a git op, report errors, refresh. */
  async function op(args: string[] | (() => Promise<unknown>), busy = "") {
    git.busy = busy;
    try {
      await (typeof args === "function" ? args() : run(args));
      return true;
    } catch (e) {
      await refreshGit();
      // Stopped on conflicts: the banner + Merge Changes list explain it better than git's raw output.
      if (!(e instanceof Cancelled) && !git.op) fail(e);
      return false;
    } finally {
      git.busy = "";
      await refreshGit();
      app.gitVersion++;
    }
  }

  /** `git pull`; if the branches diverged and no pull strategy is configured, ask merge vs rebase. */
  async function pullAsking() {
    try {
      await run(["pull"]);
    } catch (e) {
      if (!/divergent branches/i.test(String(e))) throw e;
      const v = await pick("Your branch and its upstream have diverged. How should they be combined?", [
        { label: "Merge", detail: "git pull --no-rebase · creates a merge commit", value: "merge" },
        { label: "Rebase", detail: "git pull --rebase · replays your commits on top of upstream", value: "rebase" },
        { label: "Merge, and remember for this repository", detail: "git config pull.rebase false", value: "merge!" },
        { label: "Rebase, and remember for this repository", detail: "git config pull.rebase true", value: "rebase!" },
      ]);
      if (!v) throw new Cancelled();
      const rebase = v.startsWith("rebase");
      if (v.endsWith("!")) await run(["config", "pull.rebase", String(rebase)]);
      await run(["pull", rebase ? "--rebase" : "--no-rebase"]);
    }
  }

  export const pull = () => op(pullAsking, "Pulling…");
  export const push = () => op(["push"], "Pushing…");
  export const sync = () => op(async () => (await pullAsking(), await run(["push"])), "Syncing…");
  /** Stage a resolved conflict file (absolute path). */
  export const markResolved = (path: string) => op(["add", "--", path]);

  // core.editor=true: continue must not wait for an editor (there's no tty).
  const continueOp = () =>
    op(git.op === "merge" ? ["commit", "--no-edit"] : ["-c", "core.editor=true", git.op, "--continue"]);
  async function abortOp() {
    if (await ask(`Abort the ${git.op}? Your branch goes back to how it was before it started.`, { kind: "warning" })) await op([git.op, "--abort"]);
  }

  // Agent hand-off: runs `<agent command> '<prompt>'` in a new terminal tab.
  const agent = $state({ cmd: "claude" });
  try {
    agent.cmd = localStorage.getItem("agentCommand") || "claude";
  } catch {}
  async function setAgentCmd() {
    const v = await prompt("Agent command for resolving conflicts (the prompt is appended as one argument)", agent.cmd);
    if (!v) return;
    agent.cmd = v;
    try {
      localStorage.setItem("agentCommand", v);
    } catch {}
  }
  const shq = (t: string) => `'${t.replace(/'/g, `'\\''`)}'`;
  function resolveWithAgent() {
    const files = git.status ? conflicts(git.status).map((f) => f.path) : [];
    const prompt =
      `A git ${git.op || "merge"} is in progress in this repository (${git.opInfo}) and stopped on conflicts in: ${files.join(", ")}. ` +
      `Resolve every conflict: work out what each side intended` +
      (git.op === "rebase" ? ` (during a rebase, HEAD/"current" is the upstream and the incoming side is the commit being replayed)` : "") +
      `, combine them so both intents are kept, and remove all conflict markers. If it's quick, check that the project still builds and its tests pass. ` +
      `Then stage each resolved file with git add. Do NOT commit and do NOT run --continue: I'll review and continue myself.`;
    runInTerminal(git.root, `${agent.cmd} ${shq(prompt)}`);
  }
  export const publish = () => op(["push", "-u", "origin", "HEAD"], "Publishing…");

  const stage = (paths: string[]) => op(["add", "-A", "--", ...paths]);
  const unstage = (paths: string[]) =>
    op(noHead() ? ["rm", "--cached", "-r", "-q", "--", ...paths] : ["restore", "--staged", "--", ...paths]);

  async function discard(files: GitFile[]) {
    const what = files.length === 1 ? files[0].path : `${files.length} files`;
    if (!(await ask(`Discard changes in ${what}? Changes to tracked files can't be undone.`, { kind: "warning" }))) return;
    const tracked = files.filter((f) => f.kind !== "untracked").map((f) => f.path);
    const untracked = files.filter((f) => f.kind === "untracked");
    await op(async () => {
      if (tracked.length) await run(["restore", "--worktree", "--", ...tracked]);
      // Untracked files: recoverable delete (holding area, then system Trash).
      for (const f of untracked) await invoke("trash_path", { path: `${git.root}/${f.path}` });
    });
  }

  async function commit(msg: string) {
    const s = git.status;
    if (!s || !msg.trim()) return false;
    if (!staged(s).length) {
      if (!changes(s).length) return void message("There are no changes to commit.");
      if (!(await ask("There are no staged changes. Stage all your changes and commit them directly?"))) return false;
    }
    return op(async () => {
      if (!staged(s).length) await run(["add", "-A"]);
      await run(["commit", "-m", msg]);
    });
  }

  export async function switchBranch() {
    const refs = (await run(["for-each-ref", "--sort=-committerdate", "--format=%(refname)%09%(objectname:short)%09%(contents:subject)", "refs/heads", "refs/remotes"]).catch(() => ""))
      .split("\n")
      .filter((l) => l && !l.split("\t")[0].endsWith("/HEAD"))
      .map((l) => {
        const [ref, sha, subject] = l.split("\t");
        return { ref, name: ref.replace(/^refs\/(heads|remotes)\//, ""), remote: ref.startsWith("refs/remotes/"), sha, subject };
      });
    const locals = new Set(refs.filter((r) => !r.remote).map((r) => r.name));
    const v = await pick("Select a branch to switch to", [
      { label: "+ Create new branch…", value: "\0new" },
      ...refs.map((r) => ({
        label: r.name,
        detail: `${r.sha} · ${r.subject}`,
        hint: r.name === git.status?.head ? "current" : r.remote ? "remote" : "",
        value: r.ref,
      })),
    ]);
    if (!v) return;
    if (v === "\0new") {
      const name = await prompt(`New branch name (from ${branchName()})`);
      if (name) await op(["switch", "-c", name]);
      return;
    }
    const r = refs.find((x) => x.ref === v)!;
    if (!r.remote) return op(["switch", r.name]);
    // Remote branch: switch to its local counterpart, creating a tracking branch if needed.
    const local = r.name.slice(r.name.indexOf("/") + 1);
    await op(locals.has(local) ? ["switch", local] : ["switch", "-c", local, "--track", r.name]);
  }

  /** Each worktree is its own workspace (own layout); the one we leave keeps running in the background. */
  function goTo(path: string) {
    git.loaded = false;
    openWorkspace(path);
  }

  export async function switchWorktree() {
    const wts = (await run(["worktree", "list", "--porcelain"]).catch(() => ""))
      .trim()
      .split("\n\n")
      .filter(Boolean)
      .map((b) => {
        const lines = b.split("\n");
        const branch = lines.find((l) => l.startsWith("branch "))?.slice("branch refs/heads/".length);
        return { path: lines[0].slice("worktree ".length), branch: branch ?? (lines.includes("bare") ? "bare" : "detached") };
      });
    const v = await pick("Select a worktree to open", [
      { label: "+ Create new worktree…", value: "\0new" },
      ...wts.map((w, i) => ({
        label: w.path.slice(w.path.lastIndexOf("/") + 1),
        detail: `${w.branch} · ${w.path}`,
        hint: w.path === git.root ? "current" : i === 0 ? "main" : "",
        value: w.path,
      })),
    ]);
    if (!v || v === git.root) return;
    if (v !== "\0new") return goTo(v);
    const name = await prompt("Branch for the new worktree (existing or new)");
    if (!name || !wts.length) return;
    // Sibling of the main worktree: <main>-<branch>
    const main = wts[0].path;
    const path = `${main}-${name.replace(/[^\w.-]+/g, "-")}`;
    const exists = await run(["rev-parse", "--verify", "--quiet", `refs/heads/${name}`]).then(() => true, () => false);
    if (await op(exists ? ["worktree", "add", path, name] : ["worktree", "add", "-b", name, path])) {
      if (await ask(`Worktree created at ${path}. Open it now?`)) goTo(path);
    }
  }

  function open(f: GitFile, isStaged: boolean) {
    const abs = `${git.root}/${f.path}`;
    if (f.kind === "conflict") openFile(abs); // conflict markers are best seen in the editor
    else openDiff(abs, isStaged, isStaged ? f.orig : undefined);
  }
</script>

<script lang="ts">
  import { Bot, ChevronRight, FileText, GitMerge, Minus, Pencil, Plus, Undo2 } from "@lucide/svelte";
  import { letter } from "./gitStatus";
  import GitGraph from "./GitGraph.svelte";

  let msg = $state("");
  let closed = $state<Record<string, boolean>>({});
  const s = $derived(git.status);
  const groups = $derived(
    s
      ? [
          { key: "conflicts", title: "Merge Changes", files: conflicts(s), side: "y" as const },
          { key: "staged", title: "Staged Changes", files: staged(s), side: "x" as const },
          { key: "changes", title: "Changes", files: changes(s), side: "y" as const },
        ].filter((g) => g.files.length)
      : [],
  );

  const name = (p: string) => p.slice(p.lastIndexOf("/") + 1);
  const dir = (p: string) => p.slice(0, Math.max(0, p.lastIndexOf("/")));
  async function doCommit() {
    if (await commit(msg)) msg = "";
  }
</script>

{#if !git.loaded}
  <div class="empty">Loading…</div>
{:else if !git.root || !s}
  <div class="empty">
    <p>The workspace folder is not a git repository.</p>
    <button class="primary" onclick={() => op(async () => invoke("git", { cwd: app.workspace, args: ["init"] }))}>Initialize Repository</button>
  </div>
{:else}
  <div class="panel">
    {#if git.op}
      {@const left = conflicts(s).length}
      <div class="banner">
        <div class="btitle selectable"><GitMerge size={14} strokeWidth={1.5} />{git.opInfo || git.op}</div>
        <div class="bsub">
          {left ? `${left} conflicted ${left === 1 ? "file" : "files"}: open each, accept changes or edit, then Mark as Resolved.` : "All conflicts resolved."}
        </div>
        <div class="bactions">
          <button class="primary" disabled={!!left || !!git.busy} onclick={continueOp}>Continue</button>
          <button class="ghost" disabled={!!git.busy} onclick={abortOp}>Abort</button>
        </div>
        {#if left}
          <div class="agent">
            <button class="ghost" onclick={resolveWithAgent} title="Open a terminal and ask the agent to resolve the conflicts"><Bot size={13} strokeWidth={1.5} />Resolve with {agent.cmd}</button>
            <button class="icon-btn" onclick={setAgentCmd} title="Change agent command"><Pencil size={12} strokeWidth={1.5} /></button>
          </div>
        {/if}
      </div>
    {/if}
    <div class="commit">
      <input
        bind:value={msg}
        placeholder="Commit message"
        title={`Enter to commit on "${branchName()}"`}
        onkeydown={(e) => e.key === "Enter" && (e.preventDefault(), doCommit())}
      />
      <button class="primary" disabled={!msg.trim() || !!git.busy} onclick={doCommit} title="Commit">Commit</button>
    </div>

    {#if !groups.length}
      <div class="clean">No changes</div>
    {/if}
    {#each groups as g (g.key)}
      <div class="section">
        <button class="shead" onclick={() => (closed[g.key] = !closed[g.key])}>
          <span class="chev" class:open={!closed[g.key]}><ChevronRight size={12} strokeWidth={1.5} /></span>
          <span class="stitle">{g.title}</span>
          <span class="acts">
            {#if g.key === "staged"}
              <!-- svelte-ignore a11y_click_events_have_key_events, a11y_interactive_supports_focus -->
              <span role="button" class="act" title="Unstage All" onclick={(e) => (e.stopPropagation(), unstage(g.files.map((f) => f.path)))}><Minus size={13} strokeWidth={1.5} /></span>
            {:else if g.key === "changes"}
              <!-- svelte-ignore a11y_click_events_have_key_events, a11y_interactive_supports_focus -->
              <span role="button" class="act" title="Discard All Changes" onclick={(e) => (e.stopPropagation(), discard(g.files))}><Undo2 size={13} strokeWidth={1.5} /></span>
              <!-- svelte-ignore a11y_click_events_have_key_events, a11y_interactive_supports_focus -->
              <span role="button" class="act" title="Stage All Changes" onclick={(e) => (e.stopPropagation(), op(["add", "-A"]))}><Plus size={13} strokeWidth={1.5} /></span>
            {/if}
          </span>
          <span class="count">{g.files.length}</span>
        </button>
        {#if !closed[g.key]}
          {#each g.files as f (f.path)}
            {@const l = letter(f, g.side)}
            <button class="row" title={f.orig ? `${f.orig} → ${f.path}` : f.path} onclick={() => open(f, g.key === "staged")}>
              <span class="name">{name(f.path)}</span>
              <span class="dir">{dir(f.path)}</span>
              <span class="acts">
                <!-- svelte-ignore a11y_click_events_have_key_events, a11y_interactive_supports_focus -->
                <span role="button" class="act" title="Open File" onclick={(e) => (e.stopPropagation(), openFile(`${git.root}/${f.path}`))}><FileText size={13} strokeWidth={1.5} /></span>
                {#if g.key === "staged"}
                  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_interactive_supports_focus -->
                  <span role="button" class="act" title="Unstage" onclick={(e) => (e.stopPropagation(), unstage([f.path]))}><Minus size={13} strokeWidth={1.5} /></span>
                {:else}
                  {#if g.key === "changes"}
                    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_interactive_supports_focus -->
                    <span role="button" class="act" title="Discard Changes" onclick={(e) => (e.stopPropagation(), discard([f]))}><Undo2 size={13} strokeWidth={1.5} /></span>
                  {/if}
                  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_interactive_supports_focus -->
                  <span role="button" class="act" title="Stage" onclick={(e) => (e.stopPropagation(), stage([f.path]))}><Plus size={13} strokeWidth={1.5} /></span>
                {/if}
              </span>
              <span class="letter {l}">{l}</span>
            </button>
          {/each}
        {/if}
      </div>
    {/each}
    <GitGraph />
  </div>
{/if}

<style>
  .panel {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 2px 0 12px;
  }
  .empty {
    padding: 16px 12px;
    color: var(--muted-foreground);
    font-size: 13px;
    text-align: center;
  }
  .empty p {
    margin: 0 0 12px;
  }
  .banner {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px 10px;
    border: 1px solid var(--border-strong);
    border-left: 2px solid var(--gold);
    border-radius: var(--radius-md);
    background: var(--card);
  }
  .btitle {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12.5px;
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .btitle :global(svg) {
    flex: none;
  }
  .bsub {
    color: var(--muted-foreground);
    font-size: 12px;
    line-height: 1.4;
  }
  .bactions,
  .agent {
    display: flex;
    gap: 6px;
  }
  .bactions .primary {
    flex: 1;
  }
  /* Aurelius Button variant="outline" */
  .ghost {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 5px;
    height: 30px;
    padding: 0 12px;
    border: 1px solid var(--input);
    border-radius: var(--radius-md);
    background: var(--background);
    font-size: 12.5px;
    cursor: pointer;
  }
  .ghost:hover {
    background: var(--accent);
  }
  .ghost:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .agent .ghost {
    flex: 1;
  }
  .agent .icon-btn {
    align-self: center;
  }
  .commit {
    display: flex;
    gap: 6px;
  }
  .commit input {
    flex: 1;
    min-width: 0;
    height: 30px;
    box-sizing: border-box;
    padding: 0 8px;
    border: 1px solid var(--input);
    border-radius: var(--radius-md);
    background: var(--background);
    color: var(--foreground);
    font: inherit;
    font-size: 13px;
    outline: none;
    transition: border-color var(--dur-quick) var(--ease-standard);
  }
  .commit input:focus {
    border-color: var(--gold-ring);
  }
  /* Aurelius Button variant="default" */
  .primary {
    height: 30px;
    border: 0;
    border-radius: var(--radius-md);
    background: var(--primary);
    color: var(--primary-foreground);
    box-shadow: var(--shadow-raised);
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    transition: opacity var(--dur-quick) var(--ease-standard), transform var(--dur-quick) var(--ease-standard);
  }
  .primary:hover {
    opacity: 0.9;
  }
  .primary:active {
    transform: scale(0.98);
  }
  .primary:disabled {
    opacity: 0.45;
    cursor: default;
    transform: none;
  }
  .primary {
    padding: 0 12px;
  }
  .clean {
    padding: 10px 6px;
    color: var(--muted-foreground);
    font-size: 12.5px;
  }
  .section {
    margin-top: 4px;
  }
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
  .name {
    flex: none;
    max-width: 60%;
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
  .row:hover .acts,
  .shead:hover .acts {
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
  .letter {
    width: 12px;
    flex: none;
    font-size: 11.5px;
    font-weight: 600;
    text-align: center;
    color: var(--muted-foreground);
  }
  .letter.M,
  .letter.R {
    color: var(--syn-number);
  }
  .letter.A,
  .letter.U {
    color: var(--diff-add-gutter);
  }
  .letter.D {
    color: var(--diff-del-gutter);
  }
</style>
