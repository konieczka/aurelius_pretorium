// CodeMirror extension: highlights git conflict blocks and adds Accept Current / Incoming / Both actions above each.
import { EditorState, StateField, type Extension } from "@codemirror/state";
import { Decoration, EditorView, WidgetType, type DecorationSet } from "@codemirror/view";
import { findConflicts, resolve, type Conflict } from "./conflicts";

const conflictField = StateField.define<Conflict[]>({
  create: (s) => findConflicts(s.doc.toString()),
  update: (v, tr) => (tr.docChanged ? findConflicts(tr.state.doc.toString()) : v),
});

class Actions extends WidgetType {
  constructor(readonly c: Conflict) {
    super();
  }
  eq(o: Actions) {
    return o.c.from === this.c.from && o.c.to === this.c.to;
  }
  toDOM(view: EditorView) {
    const el = document.createElement("div");
    el.className = "cm-conflictActions";
    const items: [string, "ours" | "theirs" | "both"][] = [
      ["Accept Current Change", "ours"],
      ["Accept Incoming Change", "theirs"],
      ["Accept Both Changes", "both"],
    ];
    for (const [label, pick] of items) {
      const a = document.createElement("button");
      a.textContent = label;
      a.onmousedown = (e) => e.preventDefault(); // keep editor focus/selection
      a.onclick = () => {
        // Re-find by position: the doc may have changed since the widget was drawn.
        const c = view.state.field(conflictField).find((x) => x.from === this.c.from);
        if (c) view.dispatch({ changes: { from: c.from, to: c.to, insert: resolve(view.state.doc.toString(), c, pick) } });
      };
      el.append(a);
    }
    return el;
  }
  ignoreEvent() {
    return true;
  }
}

/** Dim " (Current Change)" style hint after a marker line, as in VS Code. */
class Hint extends WidgetType {
  constructor(readonly text: string) {
    super();
  }
  eq(o: Hint) {
    return o.text === this.text;
  }
  toDOM() {
    const el = document.createElement("span");
    el.className = "cm-conflictHint";
    el.textContent = this.text;
    return el;
  }
}

function lineDecos(state: EditorState, from: number, to: number, cls: string, b: [number, Decoration][]) {
  for (let pos = from; pos < to; ) {
    const line = state.doc.lineAt(pos);
    b.push([line.from, Decoration.line({ class: cls })]);
    pos = line.to + 1;
  }
}

const decorations = EditorView.decorations.compute([conflictField], (state): DecorationSet => {
  const all: [number, Decoration][] = [];
  for (const c of state.field(conflictField)) {
    all.push([c.from, Decoration.widget({ widget: new Actions(c), block: true, side: -1 })]);
    const [m0, ...rest] = c.markers;
    all.push([m0, Decoration.line({ class: "cm-conflictMarker cm-conflictOursMarker" })]);
    lineDecos(state, c.ours.from, c.ours.to, "cm-conflictOurs", all);
    if (c.base) lineDecos(state, c.base.from, c.base.to, "cm-conflictBase", all);
    lineDecos(state, c.theirs.from, c.theirs.to, "cm-conflictTheirs", all);
    all.push([state.doc.lineAt(m0).to, Decoration.widget({ widget: new Hint(" (Current Change)"), side: 1 })]);
    all.push([state.doc.lineAt(rest[rest.length - 1]).to, Decoration.widget({ widget: new Hint(" (Incoming Change)"), side: 1 })]);
    rest.forEach((m, i) => all.push([m, Decoration.line({ class: i === rest.length - 1 ? "cm-conflictMarker cm-conflictTheirsMarker" : "cm-conflictMarker" })]));
  }
  return Decoration.set(all.map(([pos, d]) => d.range(pos)), true); // true = sort (line vs block widget order)
});

const theme = EditorView.theme({
  ".cm-conflictOurs": { backgroundColor: "var(--merge-ours-bg)" },
  ".cm-conflictTheirs": { backgroundColor: "var(--merge-theirs-bg)" },
  ".cm-conflictBase": { backgroundColor: "var(--muted)", opacity: "0.75" },
  ".cm-conflictMarker": { color: "var(--muted-foreground)", fontStyle: "italic" },
  ".cm-conflictOursMarker": { backgroundColor: "var(--merge-ours-strong)" },
  ".cm-conflictTheirsMarker": { backgroundColor: "var(--merge-theirs-strong)" },
  ".cm-conflictHint": { color: "var(--muted-foreground)", fontFamily: "var(--font-sans)", fontSize: "11.5px", fontStyle: "normal" },
  ".cm-conflictActions": { display: "flex", gap: "2px", padding: "4px 0 2px", fontFamily: "var(--font-sans)" },
  ".cm-conflictActions button": {
    border: "0",
    background: "none",
    padding: "0 6px",
    color: "var(--muted-foreground)",
    fontSize: "11.5px",
    cursor: "pointer",
    borderRadius: "4px",
  },
  ".cm-conflictActions button:hover": { color: "var(--foreground)", backgroundColor: "var(--accent)" },
});

export const conflictMarkers: Extension = [conflictField, decorations, theme];
export const conflictsIn = (state: EditorState) => state.field(conflictField, false) ?? [];

/** Jump to the next/previous conflict (wrapping), centered. Returns its 1-based index, 0 if none. */
export function goToConflict(view: EditorView, dir: 1 | -1, from = view.state.selection.main.head) {
  const cs = conflictsIn(view.state);
  if (!cs.length) return 0;
  let i = dir > 0 ? cs.findIndex((c) => c.from > from) : cs.findLastIndex((c) => c.from < from);
  if (i < 0) i = dir > 0 ? 0 : cs.length - 1;
  view.dispatch({ selection: { anchor: cs[i].from }, effects: EditorView.scrollIntoView(cs[i].from, { y: "center" }) });
  return i + 1;
}
