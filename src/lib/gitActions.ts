// Git actions behind the menus: the status bar's branch (click to switch, right-click for the rest), the Git panel's
// "…" button, and right-clicking a commit or a branch in the graph. They run through GitPanel's `op` (busy label,
// errors, refresh; stopping on conflicts offers the ways out).
import { invoke } from "@tauri-apps/api/core";
import { ask, message } from "@tauri-apps/plugin-dialog";
import { askAgent, branchName, Cancelled, fail, fetch, git, op, publish, push, pushed, pushForce, run } from "./GitPanel.svelte";
import { pick, prompt, type Item } from "./QuickPick.svelte";
import { app, copyText, forgetFolder, openDiff, openWorkspace } from "./state.svelte";
import { shells, type Shell } from "./agents.svelte";
import { popupMenu, sep, type MenuEntry } from "./contextMenu";
import { blockedByChanges, branchTasks, commitTasks, conflictPrompt, defaultBranch, finishStrategies, historyPrompt, mergeArgs, parseWorktrees, strategies, type MergeStrategy } from "./gitOps";
import type { Commit } from "./graph";

const runIn = (cwd: string, args: string[]) => invoke<string>("git", { cwd, args });
const base = (p: string) => p.slice(p.lastIndexOf("/") + 1);
const short = (h: string) => (/^[0-9a-f]{40}$/.test(h) ? h.slice(0, 7) : h);
const under = (p: string, dir: string) => p === dir || p.startsWith(dir + "/");

/** Each worktree is its own workspace (own layout); the one we leave keeps running in the background. */
const goTo = (path: string) => openWorkspace(path);

export const worktrees = () => runIn(git.root, ["worktree", "list", "--porcelain"]).then(parseWorktrees, () => []);

type Ref = { ref: string; name: string; remote: boolean; sha: string; wt: string; subject: string };
async function refs(): Promise<Ref[]> {
  const fmt = "%(refname)%09%(objectname:short)%09%(worktreepath)%09%(contents:subject)";
  return (await run(["for-each-ref", "--sort=-committerdate", `--format=${fmt}`, "refs/heads", "refs/remotes"]).catch(() => ""))
    .split("\n")
    .filter((l) => l && !l.split("\t")[0].endsWith("/HEAD"))
    .map((l) => {
      const [ref, sha, wt, ...subject] = l.split("\t");
      return { ref, name: ref.replace(/^refs\/(heads|remotes)\//, ""), remote: ref.startsWith("refs/remotes/"), sha, wt, subject: subject.join("\t") };
    });
}
const refHint = (r: Ref) => (r.name === git.status?.head ? "current" : r.wt && r.wt !== git.root ? `in worktree ${base(r.wt)}` : r.remote ? "remote" : "");

/** Pick a branch (not the current one). Returns its name: "x", or "origin/x" for a remote one. */
async function pickBranch(title: string, remotes = true) {
  const list = (await refs()).filter((r) => r.name !== git.status?.head && (remotes || !r.remote));
  if (!list.length) return void (await message("There are no other branches."));
  return (await pick(title, list.map((r) => ({ label: r.name, detail: `${r.sha} · ${r.subject}`, hint: refHint(r), value: r.name })))) ?? undefined;
}

async function defaultBranchName() {
  const locals = (await refs()).filter((r) => !r.remote).map((r) => r.name);
  return defaultBranch(await run(["symbolic-ref", "--short", "refs/remotes/origin/HEAD"]).catch(() => ""), locals);
}

// Agents mid-turn in a checkout: changing its branch or files under them breaks what they're doing.

/** Agents working right now in checkout `dir` (by the checkout their folder is in, when known). */
function busyIn(dir: string): Shell[] {
  return Object.values(shells.all).filter((s) => s.agent && s.busy && (app.repos[s.cwd]?.root ? app.repos[s.cwd].root === dir : under(s.cwd, dir)));
}
async function agentsAllow(dir: string, what: string) {
  const busy = busyIn(dir);
  if (!busy.length) return true;
  const who = [...new Set(busy.map((s) => s.agent))].join(" and ");
  return ask(`${who} ${busy.length > 1 ? "are" : "is"} working in ${base(dir)} right now. ${what} changes the files under ${busy.length > 1 ? "them" : "it"}. Go ahead anyway?`, {
    kind: "warning",
    okLabel: "Go ahead",
    cancelLabel: "Cancel",
  });
}
/** `op`, once agents working in this checkout are out of the way. */
async function guarded(what: string, args: string[] | (() => Promise<unknown>), busy = "") {
  if (!(await agentsAllow(git.root, what))) return false;
  return op(Array.isArray(args) ? () => stashing(args) : args, busy);
}

/** A merge / rebase / cherry-pick / revert stopped part way in `cwd` (on conflicts). */
const inProgress = async (cwd: string) =>
  (await runIn(cwd, ["diff", "--name-only", "--diff-filter=U"]).catch(() => "")).trim() !== "" ||
  (await Promise.all(["MERGE_HEAD", "REBASE_HEAD", "CHERRY_PICK_HEAD", "REVERT_HEAD"].map((h) => runIn(cwd, ["rev-parse", "-q", "--verify", h]).then(() => true, () => false)))).some(Boolean);

/** Run `args` in `cwd`; if uncommitted changes are in its way, offer to stash them around it. */
async function stashing(args: string[], cwd = git.root) {
  try {
    return void (await runIn(cwd, args));
  } catch (e) {
    if (!blockedByChanges(e)) throw e;
    if (!(await ask(`Your uncommitted changes are in the way of git ${args[0]}. Stash them, run it, and put them back afterwards?`, { okLabel: "Stash and go on", cancelLabel: "Cancel" })))
      throw new Cancelled();
  }
  await runIn(cwd, ["stash", "push", "--include-untracked", "-m", `Before git ${args.join(" ")}`]);
  try {
    await runIn(cwd, args);
  } catch (e) {
    if (!(await inProgress(cwd))) {
      await runIn(cwd, ["stash", "pop"]).catch(() => {}); // it didn't start: put them back
      throw e;
    }
    await message("Your uncommitted changes wait in the stash meanwhile: pop them from the Stashes list once you're through.");
    throw e;
  }
  await runIn(cwd, ["stash", "pop"]).catch(() =>
    message("Your stashed changes conflict with the result. They're applied with conflict markers, and kept in the stash too."),
  );
}

// Switching

export async function switchBranch() {
  const list = await refs();
  const v = await pick("Select a branch to switch to", [
    { label: "+ Create new branch…", value: "\0new" },
    ...list.map((r) => ({ label: r.name, detail: `${r.sha} · ${r.subject}`, hint: refHint(r), value: r.ref })),
  ]);
  if (!v) return;
  if (v === "\0new") return createBranch("HEAD");
  const r = list.find((x) => x.ref === v)!;
  await checkout(r.name, r.remote);
}

/** Check out branch `name` (a remote one: its local counterpart, made to track it if new). A branch checked out in
 * another worktree can't be checked out here too: offer to open that worktree instead. */
export async function checkout(name: string, remote = false) {
  const list = await refs();
  const local = remote ? name.slice(name.indexOf("/") + 1) : name;
  const mine = list.find((r) => !r.remote && r.name === local);
  if (mine?.wt === git.root) return;
  if (mine?.wt) {
    if (await ask(`"${local}" is checked out in the worktree at ${mine.wt}. Open that worktree?`)) goTo(mine.wt);
    return;
  }
  await guarded("Switching branches", mine || !remote ? ["switch", local] : ["switch", "-c", local, "--track", name]);
}

async function createBranch(at: string) {
  const name = await prompt(`New branch name (from ${at === "HEAD" ? branchName() : short(at)})`);
  if (name) await guarded("Switching branches", ["switch", "-c", name, ...(at === "HEAD" ? [] : [at])]);
}

export async function switchWorktree() {
  const wts = await worktrees();
  const v = await pick("Select a worktree to open", [
    { label: "+ Create new worktree…", value: "\0new" },
    ...wts.map((w, i) => ({ label: base(w.path), detail: `${w.branch} · ${w.path}`, hint: w.path === git.root ? "current" : i === 0 ? "main" : "", value: w.path })),
  ]);
  if (!v || v === git.root) return;
  if (v !== "\0new") return goTo(v);
  const name = await prompt("Branch for the new worktree (existing or new)");
  if (!name || !wts.length) return;
  // Sibling of the main worktree: <main>-<branch>
  const path = `${wts[0].path}-${name.replace(/[^\w.-]+/g, "-")}`;
  const exists = await run(["rev-parse", "--verify", "--quiet", `refs/heads/${name}`]).then(() => true, () => false);
  if (await op(exists ? ["worktree", "add", path, name] : ["worktree", "add", "-b", name, path])) {
    if (await ask(`Worktree created at ${path}. Open it now?`)) goTo(path);
  }
}

// Merging and rebasing

async function pickStrategy(title: string, list = strategies) {
  return (await pick(title, list.map((s) => ({ label: s.label, detail: s.detail, value: s.value })))) as MergeStrategy | null;
}
const squashMessage = (from: string) => prompt("Message for the squashed commit", `Squash merge of ${short(from)}`);

/** Merge `from` (a branch, remote branch or commit) into the current branch. */
export async function mergeBranch(from?: string) {
  const cur = branchName();
  from ??= await pickBranch(`Merge which branch into ${cur}?`);
  if (!from) return;
  const s = await pickStrategy(`Merge ${short(from)} into ${cur}`);
  if (!s) return;
  const msg = s === "squash" ? await squashMessage(from) : "";
  if (s === "squash" && !msg) return;
  await guarded("Merging", async () => {
    await stashing(mergeArgs(s, from));
    if (s === "squash") await run(["commit", "-m", msg!]);
  }, "Merging…");
}

/** Replay the current branch's own commits on top of `onto`. */
export async function rebaseOnto(onto?: string) {
  const cur = branchName();
  onto ??= await pickBranch(`Rebase ${cur} onto which branch?`);
  if (!onto) return;
  if ((await pushed()) && !(await ask(`${cur} is already pushed. Rebasing rewrites its commits, so you'll need to force push afterwards (Sync offers it). Rebase anyway?`)))
    return;
  await guarded("Rebasing", ["rebase", onto], "Rebasing…");
}

/** Merge `from` into branch `into`, wherever `into` is checked out: git merges only into the branch checked out where
 * it runs, and a branch is checked out in one worktree at most. Conflicts in another worktree are left there to resolve. */
async function mergeAcross(from: string, into: string, s: MergeStrategy, msg = "") {
  let wt = (await worktrees()).find((w) => w.branch === into)?.path;
  if (!wt) {
    if (!(await ask(`${into} isn't checked out in any worktree. Switch ${base(git.root)} to it, to merge ${from} into it here?`))) throw new Cancelled();
    if (!(await agentsAllow(git.root, "Switching branches"))) throw new Cancelled();
    await stashing(["switch", into]);
    wt = git.root;
  } else if (wt !== git.root) {
    if ((await runIn(wt, ["status", "--porcelain", "--untracked-files=no"])).trim())
      throw new Error(`${base(wt)}, where ${into} is checked out, has uncommitted changes. Commit or stash them there first.`);
    if (!(await agentsAllow(wt, `Merging into ${into}`))) throw new Cancelled();
  }
  try {
    await runIn(wt, mergeArgs(s, from));
    if (s === "squash") await runIn(wt, ["commit", "-m", msg]);
  } catch (e) {
    if (wt === git.root || !(await inProgress(wt))) throw e; // here: the banner and conflict choices take over
    const files = (await runIn(wt, ["diff", "--name-only", "--diff-filter=U"]).catch(() => "")).split("\n").filter(Boolean);
    const v = await pick(`Merging ${from} into ${into} stopped on conflicts in ${base(wt)}`, [
      { label: `Open ${base(wt)}`, detail: "resolve them there: its Git panel shows the merge", value: "open" },
      { label: "Resolve them with an agent", detail: `opens a terminal in ${base(wt)}`, value: "agent" },
      { label: "Abort the merge", detail: `puts ${into} back as it was`, value: "abort" },
    ]);
    if (v === "open") goTo(wt);
    else if (v === "agent") askAgent(wt, conflictPrompt("merge", `merging ${from} into ${into}`, files));
    else if (v === "abort") await runIn(wt, ["reset", "--merge"]).catch(fail);
    throw new Cancelled();
  }
}

/** Merge the current branch into another one, wherever that one is checked out. */
export async function mergeInto(into?: string) {
  const cur = branchName();
  into ??= await pickBranch(`Merge ${cur} into which branch?`, false);
  if (!into) return;
  const s = await pickStrategy(`Merge ${cur} into ${into}`);
  if (!s) return;
  const msg = s === "squash" ? await squashMessage(cur) : "";
  if (s === "squash" && !msg) return;
  if (await op(() => mergeAcross(cur, into, s, msg ?? ""), "Merging…")) await message(`Merged ${cur} into ${into}.`);
}

/** Done with this branch: merge it into the main branch (or another), wherever that's checked out, then offer to
 * remove its worktree and delete it. */
export async function finishBranch() {
  const cur = git.status?.head;
  if (!cur || cur === "(detached)") return void (await message("Check out the branch to finish first."));
  if (git.status!.files.some((f) => f.kind !== "untracked")) return void (await message(`${cur} has uncommitted changes. Commit or stash them first.`));
  const def = await defaultBranchName();
  const locals = (await refs()).filter((r) => !r.remote && r.name !== cur);
  locals.sort((a, b) => +(b.name === def) - +(a.name === def));
  const into = await pick(`Finish ${cur}: merge it into…`, locals.map((r) => ({ label: r.name, detail: `${r.sha} · ${r.subject}`, hint: r.name === def ? "main branch" : refHint(r), value: r.name })));
  if (!into) return;
  const ahead = +(await run(["rev-list", "--count", `${into}..${cur}`]).catch(() => "1")).trim();
  if (ahead) {
    const s = await pickStrategy(`How should ${cur}'s ${ahead} commit${ahead === 1 ? "" : "s"} go into ${into}?`, finishStrategies);
    if (!s) return;
    const msg = s === "squash" ? await squashMessage(cur) : "";
    if (s === "squash" && !msg) return;
    const ok = await op(async () => {
      if (s === "rebase") {
        if (!(await agentsAllow(git.root, "Rebasing"))) throw new Cancelled();
        await run(["rebase", into]); // stops on conflicts here: resolve, continue, then Finish again
      }
      await mergeAcross(cur, into, s, msg ?? "");
    }, "Finishing…");
    if (!ok) return;
    await cleanUp(cur, into, s === "squash");
  } else await cleanUp(cur, into, false);
}

/** After `cur` went into `into`: remove its worktree, delete it, both, or neither. */
async function cleanUp(cur: string, into: string, squashed: boolean) {
  const wts = await worktrees();
  const main = wts[0]?.path ?? git.root;
  const mine = wts.find((w) => w.branch === cur)?.path; // none: it's not checked out anymore (we switched to `into`)
  const items: Item[] = [];
  if (mine && mine !== main)
    items.push(
      { label: `Remove the worktree ${base(mine)} and delete ${cur}`, value: "both" },
      { label: `Remove the worktree, keep ${cur}`, value: "wt" },
    );
  else if (!mine) items.push({ label: `Delete ${cur}`, detail: squashed ? "its commits are squashed into one on " + into : "", value: "branch" });
  if (!items.length) return void (await message(`${cur} is merged into ${into}.`));
  items.push({ label: "Keep everything", value: "keep" });
  const v = await pick(`${cur} is merged into ${into}. Clean up?`, items);
  if (!v || v === "keep") return;
  if (v !== "branch" && mine) {
    const shellsThere = Object.values(shells.all).filter((s) => under(s.cwd, mine));
    if (shellsThere.length) {
      const agents = [...new Set(shellsThere.flatMap((s) => s.agent ?? []))];
      const what = `${shellsThere.length} terminal${shellsThere.length === 1 ? " is" : "s are"} in ${base(mine)}${agents.length ? ` (${agents.join(", ")} running)` : ""}`;
      if (!(await ask(`${what}. They'll be left in a deleted folder. Remove it anyway?`, { kind: "warning" }))) return;
    }
    if (git.root === mine) goTo(wts.find((w) => w.branch === into)?.path ?? main);
    const removed = await op(async () => {
      try {
        await runIn(main, ["worktree", "remove", mine]);
      } catch (e) {
        if (!/untracked|modified/i.test(String(e))) throw e;
        if (!(await ask(`${base(mine)} has files git doesn't track. Delete them along with the worktree?`, { kind: "warning" }))) throw new Cancelled();
        await runIn(main, ["worktree", "remove", "--force", mine]);
      }
    });
    if (!removed) return;
    forgetFolder(mine);
  }
  if (v !== "wt") await op(() => runIn(main, ["branch", squashed ? "-D" : "-d", cur]));
}

// Branches

async function deleteBranch(name: string) {
  const r = (await refs()).find((x) => !x.remote && x.name === name);
  if (r?.wt) return void (await message(`${name} is checked out in ${base(r.wt)}. Switch that worktree to another branch first.`));
  if (!(await ask(`Delete the branch ${name}?`, { okLabel: "Delete", cancelLabel: "Cancel" }))) return;
  await op(async () => {
    try {
      await run(["branch", "-d", name]);
    } catch (e) {
      if (!/not fully merged/i.test(String(e))) throw e;
      if (!(await ask(`${name} has commits that aren't merged anywhere. Delete it anyway? They stay recoverable from git's reflog for a while.`, { kind: "warning" })))
        throw new Cancelled();
      await run(["branch", "-D", name]);
    }
  });
}

async function deleteRemoteBranch(name: string) {
  const [remote, ...rest] = name.split("/");
  if (!(await ask(`Delete ${rest.join("/")} from ${remote}? Everyone loses it there.`, { kind: "warning", okLabel: "Delete", cancelLabel: "Cancel" }))) return;
  await op(["push", remote, "--delete", rest.join("/")], "Pushing…");
}

async function renameBranch(name: string) {
  const to = await prompt(`Rename ${name} to`, name);
  if (to && to !== name) await op(["branch", "-m", name, to]);
}

/** Changes `to` made since it split off `from`: what merging it into `from` would bring. Opens the picked file's diff. */
export async function compare(from: string, to: string) {
  try {
    const [mb, tip, count] = (await Promise.all([run(["merge-base", from, to]), run(["rev-parse", to]), run(["rev-list", "--count", `${from}..${to}`])])).map((s) => s.trim());
    const files = (await run(["diff", "--name-status", "-M", mb, tip]))
      .split("\n")
      .filter(Boolean)
      .map((l) => {
        const [st, a, b] = l.split("\t");
        return { st: st[0], path: b ?? a, orig: b ? a : undefined };
      });
    const name = to === "HEAD" ? branchName() : short(to);
    if (!files.length) return void (await message(`${name} has no changes that aren't on ${from === "HEAD" ? branchName() : short(from)}.`));
    const v = await pick(
      `${name}: ${count} commit${count === "1" ? "" : "s"} and ${files.length} changed file${files.length === 1 ? "" : "s"} since it split off ${from === "HEAD" ? branchName() : short(from)}`,
      files.map((f, i) => {
        const at = f.path.lastIndexOf("/");
        return { label: f.path.slice(at + 1), detail: f.orig ? `${f.orig} → ${f.path}` : f.path.slice(0, Math.max(0, at)), hint: f.st, value: String(i) };
      }),
    );
    if (v === null) return;
    const f = files[+v];
    openDiff(`${git.root}/${f.path}`, false, f.orig, tip, mb);
  } catch (e) {
    fail(e);
  }
}

// Commits and the working tree

async function resetTo(hash: string, mode: "soft" | "mixed" | "hard") {
  const cur = branchName();
  if (mode === "hard" && !(await ask(`Reset ${cur} to ${short(hash)} and discard all uncommitted changes? Commits after it stay recoverable from git's reflog for a while; uncommitted changes don't.`, { kind: "warning", okLabel: "Reset", cancelLabel: "Cancel" })))
    return;
  await guarded("Resetting", () => run(["reset", `--${mode}`, hash]));
}

export async function undoCommit() {
  const msg = (await run(["log", "-1", "--format=%s"]).catch(() => "")).trim();
  if ((await pushed()) && !(await ask("The last commit is already pushed. Undoing it rewrites the branch, so you'll need to force push. Undo anyway?", { kind: "warning" })))
    return;
  const root = !(await run(["rev-parse", "-q", "--verify", "HEAD^"]).then(() => true, () => false));
  // Its changes stay staged, its message goes back in the box.
  if (await op(root ? ["update-ref", "-d", "HEAD"] : ["reset", "--soft", "HEAD~1"])) git.draft = msg;
}

/** Start amending: the box gets the last commit's message; committing amends (see GitPanel's `commit`). */
export async function startAmend() {
  const msg = (await run(["log", "-1", "--format=%s"]).catch(() => "")).trim();
  git.amend = msg;
  git.draft = msg;
}

export const stash = (untracked: boolean) => guarded("Stashing", ["stash", "push", ...(untracked ? ["--include-untracked"] : [])]);
export const stashApply = (ref: string, pop: boolean) => guarded("Applying the stash", ["stash", pop ? "pop" : "apply", ref]);
export async function stashDrop(ref: string, label: string) {
  if (await ask(`Delete the stash "${label}"? Its changes are gone with it.`, { kind: "warning", okLabel: "Delete", cancelLabel: "Cancel" })) await op(["stash", "drop", ref]);
}

/** Pick one of a stash's files and show its diff. Untracked files (stashed with -u) are in its third parent. */
export async function showStash(hash: string, label: string) {
  const [tracked, untracked] = await Promise.all([
    run(["diff", "--name-status", "-M", `${hash}^1`, hash]).catch(() => ""),
    run(["ls-tree", "-r", "--name-only", `${hash}^3`]).catch(() => ""),
  ]);
  const files = [
    ...tracked.split("\n").filter(Boolean).map((l) => {
      const [st, a, b] = l.split("\t");
      return { st: st[0], path: b ?? a, orig: b ? a : undefined, ref: hash };
    }),
    ...untracked.split("\n").filter(Boolean).map((path) => ({ st: "U", path, orig: undefined, ref: `${hash}^3` })),
  ];
  const v = await pick(label, files.map((f, i) => ({ label: f.path.slice(f.path.lastIndexOf("/") + 1), detail: f.path, hint: f.st, value: String(i) })));
  if (v === null) return;
  const f = files[+v];
  openDiff(`${git.root}/${f.path}`, false, f.orig, f.ref);
}

// Agents rewriting history

async function agentHistory(task: string) {
  const head = (await run(["rev-parse", "HEAD"]).catch(() => "")).trim();
  askAgent(git.root, historyPrompt(task, branchName(), head));
}
async function agentCustom() {
  const task = await prompt("What should the agent do with this branch's history?");
  if (task) await agentHistory(task);
}

// Menus

/** The current branch's menu: the Git panel's "…" button and right-clicking the branch in the status bar. */
export async function currentMenu() {
  const s = git.status;
  if (!git.root || !s) return;
  const cur = branchName();
  const onBranch = s.head !== "(detached)" && s.oid !== "(initial)";
  const free = !git.op && !git.busy; // no merge / rebase stopped part way, nothing running
  const dirty = s.files.length > 0;
  const def = (await defaultBranchName()) ?? "the main branch";
  await popupMenu([
    { text: "Fetch", action: fetch },
    s.upstream ? { text: "Push", action: push, enabled: free } : { text: "Publish Branch", action: publish, enabled: free && onBranch },
    { text: "Force Push (With Lease)", action: () => void ask(`Replace ${s.upstream ?? "the remote branch"} with ${cur}? Refused if someone pushed to it since your last fetch.`).then((y) => y && pushForce()), enabled: free && !!s.upstream },
    sep,
    { text: `Merge Branch into ${cur}…`, action: () => mergeBranch(), enabled: free && onBranch },
    { text: `Rebase ${cur} onto…`, action: () => rebaseOnto(), enabled: free && onBranch },
    { text: `Merge ${cur} into…`, action: () => mergeInto(), enabled: free && onBranch },
    { text: `Finish ${cur}…`, action: finishBranch, enabled: free && onBranch && cur !== def },
    { text: "Compare with Branch…", action: async () => { const b = await pickBranch(`Compare ${cur} with…`); if (b) await compare(b, "HEAD"); }, enabled: onBranch },
    sep,
    { text: "Stash Changes", action: () => stash(false), enabled: free && dirty },
    { text: "Stash Changes Including Untracked", action: () => stash(true), enabled: free && dirty },
    { text: "Amend Last Commit", action: startAmend, enabled: free && s.oid !== "(initial)" },
    { text: "Undo Last Commit", action: undoCommit, enabled: free && s.oid !== "(initial)" },
    sep,
    {
      text: "Ask Agent",
      items: [...branchTasks(def).map((t): MenuEntry => ({ text: t.label, action: () => agentHistory(t.task), enabled: onBranch })), sep, { text: "Something Else…", action: agentCustom }],
    },
    sep,
    { text: `Rename ${cur}…`, action: () => renameBranch(cur), enabled: onBranch },
    { text: "Create Branch…", action: () => createBranch("HEAD"), enabled: free },
  ]);
}

/** Right-click a branch in the graph. */
export async function branchMenu(name: string, kind: "head" | "branch" | "remote") {
  if (kind === "head") return currentMenu();
  const cur = branchName();
  const free = !git.op && !git.busy;
  const onBranch = git.status?.head !== "(detached)";
  const local = kind === "branch";
  await popupMenu([
    { text: "Check Out", action: () => checkout(name, !local), enabled: free },
    sep,
    { text: `Merge into ${cur}`, action: () => mergeBranch(name), enabled: free && onBranch },
    { text: `Rebase ${cur} onto ${name}`, action: () => rebaseOnto(name), enabled: free && onBranch },
    ...(local ? [{ text: `Merge ${cur} into ${name}`, action: () => mergeInto(name), enabled: free && onBranch }] : []),
    sep,
    { text: `Compare with ${cur}`, action: () => compare("HEAD", name) },
    sep,
    ...(local
      ? [
          { text: "Push", action: () => op(["push", "-u", "origin", name], "Pushing…"), enabled: free },
          { text: "Rename…", action: () => renameBranch(name) },
          { text: "Delete…", action: () => deleteBranch(name) },
        ]
      : [{ text: "Delete from Remote…", action: () => deleteRemoteBranch(name) }]),
    sep,
    { text: "Copy Branch Name", action: () => copyText(name) },
  ]);
}

/** Right-click a commit in the graph. */
export async function commitMenu(c: Commit, isHead: boolean) {
  const cur = branchName();
  const free = !git.op && !git.busy;
  const m = c.parents.length > 1 ? ["-m", "1"] : []; // a merge commit: against its first parent
  const mine = await run(["merge-base", "--is-ancestor", c.hash, "HEAD"]).then(() => true, () => false); // on the current branch
  await popupMenu([
    { text: "Check Out (Detached)", action: () => guarded("Switching", ["switch", "--detach", c.hash]), enabled: free && !isHead },
    { text: "Create Branch Here…", action: () => createBranch(c.hash), enabled: free },
    sep,
    { text: `Cherry-pick into ${cur}`, action: () => guarded("Cherry-picking", ["cherry-pick", ...m, c.hash]), enabled: free && !mine },
    { text: `Revert in ${cur}`, action: () => guarded("Reverting", ["revert", "--no-edit", ...m, c.hash]), enabled: free && mine },
    { text: `Merge into ${cur}`, action: () => mergeBranch(c.hash), enabled: free && !mine },
    { text: `Rebase ${cur} onto This`, action: () => rebaseOnto(c.hash), enabled: free && !isHead },
    {
      text: `Reset ${cur} to Here`,
      items: [
        { text: "Soft: Keep Changes Staged", action: () => resetTo(c.hash, "soft"), enabled: free && !isHead },
        { text: "Mixed: Keep Changes Unstaged", action: () => resetTo(c.hash, "mixed"), enabled: free && !isHead },
        { text: "Hard: Discard Changes…", action: () => resetTo(c.hash, "hard"), enabled: free },
      ],
    },
    sep,
    { text: "Ask Agent", items: commitTasks(c.hash, c.subject).map((t): MenuEntry => ({ text: t.label, action: () => agentHistory(t.task), enabled: mine })) },
    sep,
    { text: "Copy Commit Hash", action: () => copyText(c.hash) },
  ]);
}
