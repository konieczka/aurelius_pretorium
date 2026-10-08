// Git conflict-marker parsing and resolution (pure; positions are character offsets).

export type Side = { from: number; to: number; label: string }; // content range, excluding marker lines
export type Conflict = {
  from: number; // start of the <<<<<<< line
  to: number; // end of the >>>>>>> line, including its newline
  ours: Side;
  base?: Side; // diff3 / zdiff3 style
  theirs: Side;
  markers: number[]; // line-start offsets of each marker line (for styling)
};

export function findConflicts(text: string): Conflict[] {
  const out: Conflict[] = [];
  let cur: Partial<Conflict> & { stage?: "ours" | "base" | "theirs"; markers?: number[] } = {};
  let pos = 0;
  for (const line of text.split("\n")) {
    const start = pos;
    const end = Math.min(pos + line.length + 1, text.length); // includes "\n" when present
    pos += line.length + 1;
    const l = line.replace(/\r$/, "");
    const label = l.slice(8).trim();
    if (l.startsWith("<<<<<<<")) {
      cur = { from: start, stage: "ours", markers: [start], ours: { from: end, to: end, label } };
    } else if (!cur.stage) {
      continue;
    } else if (l.startsWith("|||||||") && cur.stage === "ours") {
      cur.ours!.to = start;
      cur.base = { from: end, to: end, label };
      cur.stage = "base";
      cur.markers!.push(start);
    } else if (l === "=======" && cur.stage !== "theirs") {
      (cur.stage === "base" ? cur.base! : cur.ours!).to = start;
      cur.theirs = { from: end, to: end, label: "" };
      cur.stage = "theirs";
      cur.markers!.push(start);
    } else if (l.startsWith(">>>>>>>") && cur.stage === "theirs") {
      cur.theirs = { ...cur.theirs!, to: start, label };
      cur.markers!.push(start);
      out.push({ from: cur.from!, to: end, ours: cur.ours!, base: cur.base, theirs: cur.theirs, markers: cur.markers! });
      cur = {};
    }
  }
  return out;
}

/** Replacement text for a conflict block. */
export function resolve(text: string, c: Conflict, pick: "ours" | "theirs" | "both") {
  const ours = text.slice(c.ours.from, c.ours.to);
  const theirs = text.slice(c.theirs.from, c.theirs.to);
  return pick === "ours" ? ours : pick === "theirs" ? theirs : ours + theirs;
}
