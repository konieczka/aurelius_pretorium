// Parser for `git status --porcelain=v2 -z --branch [--ignored=matching]`.

export type GitFile = {
  path: string; // relative to the repo root
  orig?: string; // rename source
  x: string; // index status ("." = unchanged)
  y: string; // worktree status
  kind: "changed" | "renamed" | "conflict" | "untracked";
};
export type GitStatus = { head: string; oid: string; upstream?: string; ahead: number; behind: number; files: GitFile[]; ignored: string[] };

/** Path = everything after `n` space-separated fields (paths may contain spaces). */
const after = (rec: string, n: number) => rec.split(" ").slice(n).join(" ");

export function parseStatus(out: string): GitStatus {
  const s: GitStatus = { head: "", oid: "", ahead: 0, behind: 0, files: [], ignored: [] };
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
    else if (r.startsWith("! ")) s.ignored.push(r.slice(2)); // ignored dirs end in "/" and aren't listed inside
  }
  return s;
}

export const conflicts = (s: GitStatus) => s.files.filter((f) => f.kind === "conflict");
export const staged = (s: GitStatus) => s.files.filter((f) => f.kind !== "conflict" && f.x !== ".");
export const changes = (s: GitStatus) => s.files.filter((f) => f.kind !== "conflict" && f.y !== ".");

/** Letter shown in the list: the side's status, "U" for untracked/conflict. */
export const letter = (f: GitFile, side: "x" | "y") => (f.kind === "untracked" || f.kind === "conflict" ? "U" : f[side]);

// File tree decorations, keyed by absolute path.
// Mark: C = conflict, M = modified (also renamed, deleted below a folder), A = added, U = untracked, R = renamed.
export type Mark = "C" | "M" | "A" | "U" | "R" | "D";
export type TreeDeco = {
  marks: Map<string, Mark>; // files, and untracked folders (everything inside them is untracked)
  dirs: Map<string, Mark>; // folders containing changes: their most important mark
  ignored: Set<string>; // ignored files and folders (everything inside a folder is ignored)
};
export const emptyDeco = (): TreeDeco => ({ marks: new Map(), dirs: new Map(), ignored: new Set() });

const rank: Record<Mark, number> = { C: 4, M: 3, D: 3, R: 3, A: 2, U: 1 };
const dirMark = (m: Mark): Mark => (m === "D" || m === "R" ? "M" : m);

function markOf(f: GitFile): Mark {
  if (f.kind === "conflict") return "C";
  if (f.kind === "untracked") return "U";
  const c = f.y !== "." ? f.y : f.x; // the worktree side wins: it's what the file looks like now
  return c === "A" || c === "D" || c === "R" ? c : "M";
}

export function treeDecorations(s: GitStatus, root: string): TreeDeco {
  const d = emptyDeco();
  const abs = (p: string) => `${root}/${p.replace(/\/$/, "")}`;
  for (const f of s.files) {
    const path = abs(f.path);
    const m = markOf(f);
    d.marks.set(path, m);
    for (let dir = path.slice(0, path.lastIndexOf("/")); dir.length > root.length; dir = dir.slice(0, dir.lastIndexOf("/"))) {
      const old = d.dirs.get(dir);
      if (!old || rank[m] > rank[old]) d.dirs.set(dir, dirMark(m));
    }
  }
  for (const p of s.ignored) d.ignored.add(abs(p));
  return d;
}

/** `path` or one of its folders (below `root`) is in `set`. */
function within(path: string, root: string, has: (p: string) => boolean) {
  for (let p = path; p.length > root.length; p = p.slice(0, p.lastIndexOf("/"))) if (has(p)) return p;
  return "";
}

/** Decoration for one tree row. A folder without its own mark shows the summary of what's inside. */
export function decoFor(d: TreeDeco, root: string, path: string, isDir: boolean): { mark?: Mark; ignored: boolean } {
  const ignored = !!within(path, root, (p) => d.ignored.has(p));
  const own = within(path, root, (p) => d.marks.get(p) === "U" || p === path && d.marks.has(p));
  const mark = own ? d.marks.get(own) : isDir ? d.dirs.get(path) : undefined;
  return { mark, ignored };
}
