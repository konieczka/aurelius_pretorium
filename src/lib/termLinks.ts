// File paths in a terminal's output (an agent's "docs/plan.md", a compiler's "src/main.rs:12:5") as links. The text
// only says what might be a path: the backend says which are files (existing_files), and only those become links.
import type { IBufferCellPosition, ILink, ILinkProvider, Terminal } from "@xterm/xterm";

export type PathRef = { start: number; text: string; path: string; line?: number; col?: number };

// A run of path characters, with a ":line" or ":line:col" after it. Spaces end a path: one with spaces isn't told
// apart from the words around it.
const candidate = /(?:~|\.{1,2})?\/?[\w@%+=.,-]+(?:\/[\w@%+=.,-]+)*\/?(?::(\d+)(?::(\d+))?)?/g;

/** What in `text` may be a file's path: one with a folder in it or a file extension. URLs are left to the web links. */
export function pathRefs(text: string): PathRef[] {
  const out: PathRef[] = [];
  for (const m of text.matchAll(candidate)) {
    let s = m[0];
    let path = s.replace(/(?::\d+){1,2}$/, "");
    // Sentence punctuation after a path isn't part of it ("see docs/plan.md." or "a.ts, b.ts").
    const trail = path.match(/[.,]+$/)?.[0].length ?? 0;
    if (trail && path.length < s.length) continue; // "a.ts.:3" isn't a path and a line
    if (trail) (path = path.slice(0, -trail)), (s = s.slice(0, -trail));
    const start = m.index;
    if (!path || path.endsWith("/") || /^[.~/]*$/.test(path)) continue;
    if (/[\w.+-]:\/\/?$/.test(text.slice(0, start))) continue; // in a URL
    if (!path.includes("/") && !/[^.]\.[A-Za-z][\w-]*$/.test(path)) continue; // a word, a number, a version
    const line = m[1] ? Number(m[1]) : undefined;
    const col = m[2] ? Number(m[2]) : undefined;
    out.push({ start, text: s, path, ...(line ? { line } : {}), ...(col ? { col } : {}) });
  }
  return out;
}

type Row = { text: string; cells: number[]; y: number }; // cells[i]: the column the text's i-th UTF-16 unit is in

/** The buffer line `y` (0-based) is part of, as text: the rows a long line wrapped onto joined, with where each
 * character is on screen (wide characters take two columns, emoji two units). */
function logicalLine(term: Terminal, y: number): Row[] {
  const buf = term.buffer.active;
  let first = y;
  while (first > 0 && buf.getLine(first)?.isWrapped) first--;
  const rows: Row[] = [];
  const cell = buf.getNullCell();
  for (let r = first; r === first || buf.getLine(r)?.isWrapped; r++) {
    const line = buf.getLine(r);
    if (!line) break;
    let text = "";
    const cells: number[] = [];
    for (let x = 0; x < line.length; x++) {
      line.getCell(x, cell);
      if (!cell.getWidth()) continue; // the second half of a wide character
      const ch = cell.getChars() || " ";
      text += ch;
      for (let i = 0; i < ch.length; i++) cells.push(x);
    }
    rows.push({ text, cells, y: r });
  }
  return rows;
}

/** Links for the file paths on the terminal's row `y` (1-based, as xterm asks). `resolve` gives each path's file
 * (absolute) or null; `open` is called with the file on click. */
export function fileLinkProvider(
  term: Terminal,
  resolve: (paths: string[]) => Promise<(string | null)[]>,
  open: (ev: MouseEvent, file: string, ref: PathRef) => void,
): ILinkProvider {
  return {
    provideLinks(y, callback) {
      const rows = logicalLine(term, y - 1);
      const text = rows.map((r) => r.text).join("");
      const at = (i: number): IBufferCellPosition => {
        for (const r of rows) {
          if (i < r.cells.length) return { x: r.cells[i] + 1, y: r.y + 1 };
          i -= r.cells.length;
        }
        const last = rows[rows.length - 1];
        return { x: (last?.cells.at(-1) ?? 0) + 1, y: (last?.y ?? 0) + 1 };
      };
      const refs = pathRefs(text).filter((r) => {
        const s = at(r.start).y, e = at(r.start + r.text.length - 1).y;
        return s <= y && y <= e; // the rows that wrap a line ask for it too: answer only for the links on this one
      });
      if (!refs.length) return callback(undefined);
      resolve(refs.map((r) => r.path)).then(
        (files) => {
          const links: ILink[] = [];
          refs.forEach((r, i) => {
            const file = files[i];
            if (file) links.push({ range: { start: at(r.start), end: at(r.start + r.text.length - 1) }, text: r.text, activate: (ev) => open(ev, file, r) });
          });
          callback(links.length ? links : undefined);
        },
        () => callback(undefined),
      );
    },
  };
}
