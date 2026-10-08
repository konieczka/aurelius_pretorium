import { expect, test } from "bun:test";
import { changes, conflicts, parseStatus, staged } from "./gitStatus";

// Real output (records joined by NUL), plus upstream/ab and a conflict record.
const out = [
  "# branch.oid 2f6741a6e4bfe48d99ddb50d48b6a5ff6d9788e6",
  "# branch.head main",
  "# branch.upstream origin/main",
  "# branch.ab +2 -1",
  "1 .D N... 100644 100644 000000 4bcfe98e 4bcfe98e del.txt",
  "1 MM N... 100644 100644 100644 422c2b7a 55dce135 mod.txt",
  "1 A. N... 000000 100644 100644 00000000 bca70f35 new file.ts",
  "2 R. N... 100644 100644 100644 587be6b4 587be6b4 R100 new.txt",
  "old.txt",
  "u UU N... 100644 100644 100644 100644 aaaa bbbb cccc both edited.txt",
  "? untracked.md",
  "",
].join("\0");

test("parseStatus: branch, records, groups", () => {
  const s = parseStatus(out);
  expect([s.head, s.upstream, s.ahead, s.behind]).toEqual(["main", "origin/main", 2, 1]);
  expect(s.files.find((f) => f.kind === "renamed")).toEqual({ path: "new.txt", orig: "old.txt", x: "R", y: ".", kind: "renamed" });
  expect(staged(s).map((f) => f.path)).toEqual(["mod.txt", "new file.ts", "new.txt"]);
  expect(changes(s).map((f) => f.path)).toEqual(["del.txt", "mod.txt", "untracked.md"]);
  expect(conflicts(s).map((f) => f.path)).toEqual(["both edited.txt"]);
});

test("parseStatus: fresh repo", () => {
  const s = parseStatus("# branch.oid (initial)\0# branch.head main\0");
  expect([s.oid, s.head, s.files.length, s.upstream]).toEqual(["(initial)", "main", 0, undefined]);
});
