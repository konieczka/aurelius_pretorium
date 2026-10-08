// Commit graph: parse `git log` output and assign lanes (pure, for GitGraph.svelte).

export type Commit = { hash: string; parents: string[]; author: string; time: number; refs: string[]; subject: string };
/** Line in a row's cell: from lane x1 at y1 to lane x2 at y2 (0 = top, 1 = middle/node, 2 = bottom). */
export type Seg = { x1: number; y1: 0 | 1; x2: number; y2: 1 | 2 };
export type Row = { commit: Commit; lane: number; segs: Seg[]; width: number };

// One record per commit: fields joined by \x1f, records terminated by \x1e.
export const LOG_FORMAT = "%H%x1f%P%x1f%an%x1f%at%x1f%D%x1f%s%x1e";

export function parseLog(out: string): Commit[] {
  return out
    .split("\x1e")
    .map((r) => r.replace(/^\n/, ""))
    .filter(Boolean)
    .map((r) => {
      const [hash, parents, author, time, refs, subject] = r.split("\x1f");
      return { hash, parents: parents ? parents.split(" ") : [], author, time: +time, refs: refs ? refs.split(", ") : [], subject };
    });
}

/**
 * Classic lane assignment over topo-ordered commits: `lanes[i]` holds the hash lane i is waiting for.
 * A commit takes the first lane waiting for it (others converge into it), its first parent continues
 * that lane, and extra parents (merges) branch into existing or free lanes.
 */
export function layout(commits: Commit[]): Row[] {
  const lanes: (string | null)[] = [];
  const free = () => {
    const i = lanes.indexOf(null);
    return i < 0 ? lanes.push(null) - 1 : i;
  };
  return commits.map((commit) => {
    const segs: Seg[] = [];
    let lane = lanes.indexOf(commit.hash);
    if (lane < 0) lane = free(); // branch tip: nothing above leads here
    lanes.forEach((h, i) => {
      if (h === commit.hash) segs.push({ x1: i, y1: 0, x2: lane, y2: 1 });
      else if (h) segs.push({ x1: i, y1: 0, x2: i, y2: 2 }); // passes through
    });
    lanes.forEach((h, i) => h === commit.hash && (lanes[i] = null));
    commit.parents.forEach((p, n) => {
      const existing = lanes.indexOf(p);
      if (existing >= 0 && !(n === 0 && existing > lane)) return segs.push({ x1: lane, y1: 1, x2: existing, y2: 2 }); // joins a lane already waiting for p
      if (existing >= 0) {
        // First parent already waited for further right: pull that lane into ours to keep the main line straight.
        lanes[existing] = null;
        const pass = segs.find((s) => s.x1 === existing && s.y1 === 0 && s.y2 === 2);
        if (pass) pass.x2 = lane;
      }
      const j = n === 0 ? lane : free();
      lanes[j] = p;
      segs.push({ x1: lane, y1: 1, x2: j, y2: 2 });
    });
    while (lanes.length && lanes[lanes.length - 1] === null) lanes.pop();
    const width = Math.max(lane + 1, ...segs.map((s) => Math.max(s.x1, s.x2) + 1));
    return { commit, lane, segs, width };
  });
}
