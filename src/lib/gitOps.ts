// Pure helpers for the git actions (gitActions.ts): parsing git's output, reading its errors, building agent prompts.

export type Worktree = { path: string; branch: string }; // branch: name, "detached" or "bare"

/** `git worktree list --porcelain`; the first one is the main checkout. */
export function parseWorktrees(out: string): Worktree[] {
  return out
    .trim()
    .split("\n\n")
    .filter(Boolean)
    .map((b) => {
      const lines = b.split("\n");
      const branch = lines.find((l) => l.startsWith("branch "))?.slice("branch refs/heads/".length);
      return { path: lines[0].slice("worktree ".length), branch: branch ?? (lines.includes("bare") ? "bare" : "detached") };
    });
}

export type Stash = { ref: string; hash: string; time: number; message: string }; // ref: stash@{n}
export const STASH_FORMAT = "%gd%x09%H%x09%ct%x09%gs";

/** `git stash list --format=STASH_FORMAT`. The message drops git's "On <branch>: " / "WIP on <branch>: " prefix. */
export function parseStashes(out: string): Stash[] {
  return out
    .split("\n")
    .filter(Boolean)
    .map((l) => {
      const [ref, hash, time, ...msg] = l.split("\t");
      return { ref, hash, time: +time, message: msg.join("\t").replace(/^(WIP on|On) [^:]*: /, "") };
    });
}

/** The command refused to start: it would overwrite uncommitted changes. */
export const blockedByChanges = (e: unknown) =>
  /would be overwritten by|commit your changes or stash them|You have unstaged changes|Your index contains uncommitted changes|cannot (rebase|pull with rebase): You have/i.test(
    String(e),
  );

/** `git push` was refused because the remote branch has commits the local one doesn't. */
export const pushRejected = (e: unknown) => /\[rejected\]|non-fast-forward|fetch first|tip of your current branch is behind/i.test(String(e));

/** `git cherry HEAD @{u}`: every commit only on the upstream ("-" = an equivalent is already on HEAD) is a copy of
 * one of ours. Then the branch was rebased: pulling would bring the old copies back in, force-pushing is what's meant. */
export const rewrittenOnly = (cherry: string) => {
  const lines = cherry.split("\n").filter(Boolean);
  return lines.length > 0 && lines.every((l) => l.startsWith("-"));
};

/** The repo's main branch: origin's HEAD (`git symbolic-ref --short refs/remotes/origin/HEAD` → "origin/main"),
 * else the first usual name that exists locally. */
export function defaultBranch(originHead: string, locals: string[]): string | null {
  const fromOrigin = originHead.trim().replace(/^[^/]+\//, "");
  if (fromOrigin && locals.includes(fromOrigin)) return fromOrigin;
  return ["main", "master", "trunk", "develop"].find((b) => locals.includes(b)) ?? null;
}

export type MergeStrategy = "merge" | "no-ff" | "ff-only" | "squash" | "rebase";
export const strategies: { value: MergeStrategy; label: string; detail: string }[] = [
  { value: "merge", label: "Merge", detail: "fast-forward when possible, else a merge commit" },
  { value: "no-ff", label: "Merge with a merge commit", detail: "git merge --no-ff · always records the merge" },
  { value: "ff-only", label: "Fast-forward only", detail: "git merge --ff-only · fails if the branches have diverged" },
  { value: "squash", label: "Squash into one commit", detail: "git merge --squash, then one commit" },
];
export const finishStrategies = [...strategies, { value: "rebase" as const, label: "Rebase, then fast-forward", detail: "replays the branch on top, then a straight line" }];

/** `git merge` args for a strategy ("rebase" merges by fast-forward, after the rebase). Squash commits separately. */
export function mergeArgs(s: MergeStrategy, branch: string): string[] {
  const flag = { merge: [], "no-ff": ["--no-ff"], "ff-only": ["--ff-only"], rebase: ["--ff-only"], squash: ["--squash"] }[s];
  return ["merge", ...flag, ...(s === "squash" ? [] : ["--no-edit"]), branch];
}

// Agent hand-offs. Agents run without a terminal editor of ours, so they're told to stay non-interactive.

export function conflictPrompt(op: string, info: string, files: string[]) {
  return (
    `A git ${op || "merge"} is in progress in this repository (${info || op}) and stopped on conflicts in: ${files.join(", ")}. ` +
    `Resolve every conflict: work out what each side intended` +
    (op === "rebase" ? ` (during a rebase, HEAD/"current" is the upstream and the incoming side is the commit being replayed)` : "") +
    `, combine them so both intents are kept, and remove all conflict markers. If it's quick, check that the project still builds and its tests pass. ` +
    `Then stage each resolved file with git add. Do NOT commit and do NOT run --continue: I'll review and continue myself.`
  );
}

/** Ask an agent to rewrite history: `task` says what, the rest keeps it safe and non-interactive. */
export function historyPrompt(task: string, branch: string, head: string) {
  return (
    `${task} You're on branch ${branch}, at ${head.slice(0, 12)}. ` +
    `Rewrite history only with non-interactive git commands (for example git rebase -i with GIT_SEQUENCE_EDITOR and GIT_EDITOR set to scripts, ` +
    `or git reset --soft and new commits): never open an editor. Don't push. ` +
    `If anything goes wrong, git reset --hard ${head.slice(0, 12)} puts the branch back. When you're done, show the new history with git log --oneline.`
  );
}

export type HistoryTask = { label: string; task: string };
/** What an agent can do to the branch's own commits (those not on `base`). */
export const branchTasks = (base: string): HistoryTask[] => [
  { label: "Squash the branch into one commit", task: `Squash all commits on this branch that aren't on ${base} into one commit, with a message that sums them up. The final tree must stay identical.` },
  { label: "Improve the commit messages", task: `Reword the messages of this branch's commits that aren't on ${base}: a clear subject line, and a body explaining why where it helps. Don't change any content.` },
  { label: "Clean up the history", task: `Clean up this branch's commits that aren't on ${base}: squash fixup and WIP commits into the commits they belong to, and reorder where it reads better. The final tree must stay identical.` },
  { label: "Split the last commit", task: "Split the last commit into smaller commits, one per logical change, each with its own message. The final tree must stay identical." },
];
/** What an agent can do to one commit. */
export const commitTasks = (hash: string, subject: string): HistoryTask[] => {
  const c = `commit ${hash.slice(0, 12)} ("${subject}")`;
  return [
    { label: "Reword this commit", task: `Reword the message of ${c} so it says clearly what changed and why. Don't change any content.` },
    { label: "Squash into its parent", task: `Squash ${c} into its parent commit, combining their messages sensibly. The final tree must stay identical.` },
    { label: "Split this commit", task: `Split ${c} into smaller commits, one per logical change, each with its own message. The final tree must stay identical.` },
    { label: "Drop this commit", task: `Remove ${c} from the history, resolving any conflicts this causes in later commits so they keep their intent.` },
  ];
};
