import { expect, test } from "bun:test";
import { layout, parseLog, type Commit } from "./graph";

const c = (hash: string, ...parents: string[]): Commit => ({ hash, parents, author: "", time: 0, refs: [], subject: hash });

test("merge splits into two lanes, branches converge back into lane 0", () => {
  // M merges feature (F-E) into main (C); both fork from B.
  const rows = layout([c("M", "C", "F"), c("F", "E"), c("E", "B"), c("C", "B"), c("B", "A"), c("A")]);
  expect(rows.map((r) => r.lane)).toEqual([0, 1, 1, 0, 0, 0]);
  expect(rows[0].segs).toContainEqual({ x1: 0, y1: 1, x2: 1, y2: 2 }); // merge edge out to the feature lane
  expect(rows[3].segs).toContainEqual({ x1: 1, y1: 0, x2: 0, y2: 2 }); // feature lane bends back into main
  expect(rows[5].segs).toEqual([{ x1: 0, y1: 0, x2: 0, y2: 1 }]); // root: line ends at the node
  expect(Math.max(...rows.map((r) => r.width))).toBe(2);
});

test("parseLog", () => {
  const out = "aaa\x1fbbb ccc\x1fMe\x1f1700000000\x1fHEAD -> main, origin/main, tag: v1\x1fMerge x\x1e\nbbb\x1f\x1fMe\x1f1690000000\x1f\x1fInit\x1e\n";
  const [m, r] = parseLog(out);
  expect(m).toEqual({ hash: "aaa", parents: ["bbb", "ccc"], author: "Me", time: 1700000000, refs: ["HEAD -> main", "origin/main", "tag: v1"], subject: "Merge x" });
  expect([r.parents, r.refs]).toEqual([[], []]);
});
