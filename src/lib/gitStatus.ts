// Parser for `git status --porcelain=v2 -z --branch`.

export type GitFile = {
  path: string; // relative to the repo root
  orig?: string; // rename source
  x: string; // index status ("." = unchanged)
  y: string; // worktree status
  kind: "changed" | "renamed" | "conflict" | "untracked";
};
export type GitStatus = { head: string; oid: string; upstream?: string; ahead: number; behind: number; files: GitFile[] };

/** Path = everything after `n` space-separated fields (paths may contain spaces). */
const after = (rec: string, n: number) => rec.split(" ").slice(n).join(" ");

export function parseStatus(out: string): GitStatus {
  const s: GitStatus = { head: "", oid: "", ahead: 0, behind: 0, files: [] };
  const recs = out.split("\0");
  for (let i = 0; i < recs.length; i++) {
    const r = recs[i];
    const xy = r.slice(2, 4);
    if (r.startsWith("# branch.oid ")) s.oid = r.slice(13);
    else if (r.startsWith("# branch.head ")) s.head = r.slice(14);
    else if (r.startsWith("# branch.upstream ")) s.upstream = r.slice(18);
    else if (r.startsWith("# branch.ab ")) [s.ahead, s.behind] = r.slice(12).split(" ").map((n) => Math.abs(parseInt(n)));
    else if (r.startsWith("1 ")) s.files.push({ path: after(r, 8), x: xy[0], y: xy[1], kind: "changed" });
    else if (r.startsWith("2 ")) s.files.push({ path: after(r, 9), orig: recs[++i], x: xy[0], y: xy[1], kind: "renamed" });
    else if (r.startsWith("u ")) s.files.push({ path: after(r, 10), x: xy[0], y: xy[1], kind: "conflict" });
    else if (r.startsWith("? ")) s.files.push({ path: r.slice(2), x: ".", y: "?", kind: "untracked" });
  }
  return s;
}

export const conflicts = (s: GitStatus) => s.files.filter((f) => f.kind === "conflict");
export const staged = (s: GitStatus) => s.files.filter((f) => f.kind !== "conflict" && f.x !== ".");
export const changes = (s: GitStatus) => s.files.filter((f) => f.kind !== "conflict" && f.y !== ".");

/** Letter shown in the list: the side's status, "U" for untracked/conflict. */
export const letter = (f: GitFile, side: "x" | "y") => (f.kind === "untracked" || f.kind === "conflict" ? "U" : f[side]);
