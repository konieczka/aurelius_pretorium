import { describe, expect, test } from "bun:test";
import { blockedByChanges, defaultBranch, mergeArgs, parseStashes, parseWorktrees, pushRejected, rewrittenOnly } from "./gitOps";

describe("gitOps", () => {
  test("worktrees: main first, branch / detached / bare", () => {
    const out = "worktree /r\nHEAD abc\nbranch refs/heads/main\n\nworktree /r-x\nHEAD def\ndetached\n\nworktree /b\nbare\n";
    expect(parseWorktrees(out)).toEqual([
      { path: "/r", branch: "main" },
      { path: "/r-x", branch: "detached" },
      { path: "/b", branch: "bare" },
    ]);
  });

  test("stashes drop git's branch prefix", () => {
    const out = "stash@{0}\taaa\t100\tOn main: before merge\nstash@{1}\tbbb\t50\tWIP on feat: 1234567 subject: with colon\n";
    expect(parseStashes(out)).toEqual([
      { ref: "stash@{0}", hash: "aaa", time: 100, message: "before merge" },
      { ref: "stash@{1}", hash: "bbb", time: 50, message: "1234567 subject: with colon" },
    ]);
  });

  test("reads blocked and rejected errors", () => {
    expect(blockedByChanges("error: Your local changes to the following files would be overwritten by checkout:\n\ta.ts")).toBe(true);
    expect(blockedByChanges("error: cannot rebase: You have unstaged changes.")).toBe(true);
    expect(blockedByChanges("fatal: not a git repository")).toBe(false);
    expect(pushRejected(" ! [rejected]        main -> main (non-fast-forward)")).toBe(true);
    expect(pushRejected("fatal: unable to access")).toBe(false);
  });

  test("rewritten history: every upstream-only commit has a copy here", () => {
    expect(rewrittenOnly("- abc\n- def\n")).toBe(true);
    expect(rewrittenOnly("- abc\n+ def\n")).toBe(false);
    expect(rewrittenOnly("")).toBe(false);
  });

  test("default branch: origin's HEAD, else a usual name", () => {
    expect(defaultBranch("origin/develop\n", ["main", "develop"])).toBe("develop");
    expect(defaultBranch("", ["feature", "master"])).toBe("master");
    expect(defaultBranch("origin/gone", ["feature"])).toBe(null);
  });

  test("merge args per strategy", () => {
    expect(mergeArgs("merge", "x")).toEqual(["merge", "--no-edit", "x"]);
    expect(mergeArgs("no-ff", "x")).toEqual(["merge", "--no-ff", "--no-edit", "x"]);
    expect(mergeArgs("squash", "x")).toEqual(["merge", "--squash", "x"]);
    expect(mergeArgs("rebase", "x")).toEqual(["merge", "--ff-only", "--no-edit", "x"]);
  });
});
