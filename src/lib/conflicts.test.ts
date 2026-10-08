import { expect, test } from "bun:test";
import { findConflicts, resolve } from "./conflicts";

const apply = (text: string, pick: "ours" | "theirs" | "both", i = 0) => {
  const c = findConflicts(text)[i];
  return text.slice(0, c.from) + resolve(text, c, pick) + text.slice(c.to);
};

test("two-way conflicts", () => {
  const t = "a\n<<<<<<< HEAD\nmine\n=======\ntheirs\nmore\n>>>>>>> origin/main\nb\n<<<<<<< HEAD\nx\n=======\ny\n>>>>>>> 1a2b3c (Fix y)";
  const cs = findConflicts(t);
  expect(cs.length).toBe(2);
  expect([cs[0].ours.label, cs[0].theirs.label, cs[1].theirs.label]).toEqual(["HEAD", "origin/main", "1a2b3c (Fix y)"]);
  expect(apply(t, "ours")).toBe("a\nmine\nb\n" + t.slice(t.indexOf("<<<<<<< HEAD\nx")));
  expect(apply(t, "theirs")).toBe("a\ntheirs\nmore\nb\n" + t.slice(t.indexOf("<<<<<<< HEAD\nx")));
  expect(apply(t, "both", 1)).toBe(t.slice(0, t.indexOf("<<<<<<< HEAD\nx")) + "x\ny\n"); // no trailing newline at EOF
});

test("diff3 base section, empty sides, stray markers", () => {
  const t = "<<<<<<< ours\n=======\nnew\n>>>>>>> theirs\n=======\n<<<<<<< HEAD\nA\n||||||| base\nO\n=======\nB\n>>>>>>> br\n";
  const cs = findConflicts(t);
  expect(cs.length).toBe(2);
  expect(resolve(t, cs[0], "ours")).toBe(""); // deleted on our side
  expect(t.slice(cs[1].base!.from, cs[1].base!.to)).toBe("O\n");
  expect(resolve(t, cs[1], "both")).toBe("A\nB\n");
});
