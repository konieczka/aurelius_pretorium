import { expect, test } from "bun:test";
import { pathRefs } from "./termLinks";

const paths = (text: string) => pathRefs(text).map((r) => r.path);

test("paths with a folder or an extension are candidates, wherever they sit in a sentence", () => {
  expect(paths("- docs/stage-agent/feasibility.md — the research")).toEqual(["docs/stage-agent/feasibility.md"]);
  expect(paths("see README.md.")).toEqual(["README.md"]);
  expect(paths("Read(src/lib/Terminal.svelte) and `src-tauri/Cargo.toml`, then ./run.sh")).toEqual(["src/lib/Terminal.svelte", "src-tauri/Cargo.toml", "./run.sh"]);
  expect(paths("/Users/me/project/a.ts ~/notes/todo.txt ../up/b.rs")).toEqual(["/Users/me/project/a.ts", "~/notes/todo.txt", "../up/b.rs"]);
  expect(paths("a.ts, b.ts")).toEqual(["a.ts", "b.ts"]);
  expect(paths("a.ts,b.ts --out=dist/a.js config=src/app.json")).toEqual(["a.ts", "b.ts", "dist/a.js", "src/app.json"]);
});

test("names in any script are read whole", () => {
  expect(paths("docs/zażółć.md and src/café.md:3, 文档/说明.md")).toEqual(["docs/zażółć.md", "src/café.md", "文档/说明.md"]);
});

test("words, numbers, versions and folders on their own aren't", () => {
  expect(paths("It parses and has no duplicate ids; 75 scenarios, v1.2.3 or 1.5")).toEqual([]);
  expect(paths("in src/ and / and ..")).toEqual([]);
});

test("URLs are left to the web links", () => {
  expect(paths("https://github.com/a/b.md and file:///tmp/x.txt")).toEqual([]);
});

test("a line and column after the path are read off it", () => {
  const [r] = pathRefs("error at src/main.rs:12:5: oops");
  expect(r).toEqual({ start: 9, text: "src/main.rs:12:5", path: "src/main.rs", line: 12, col: 5 });
  expect(pathRefs("src/lib/x.ts:40.")[0]).toEqual({ start: 0, text: "src/lib/x.ts:40", path: "src/lib/x.ts", line: 40 });
});
