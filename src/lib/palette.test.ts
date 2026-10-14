import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { isPalette, paletteOverride, resolvePalette, shouldAnimatePaletteChange } from "./palette";

test("a workspace palette overrides the global palette", () => {
  expect(resolvePalette("mare", "tyrian")).toBe("mare");
  expect(resolvePalette("aurelius", "mare")).toBe("aurelius");
});

test("without a workspace palette, the global palette is the fallback", () => {
  expect(resolvePalette(undefined, "mare")).toBe("mare");
  expect(resolvePalette(null, "verdigris")).toBe("verdigris");
  // Clearing an override (null/undefined) falls back to whatever the global is now.
  expect(resolvePalette(null, "vesuvius")).toBe("vesuvius");
});

test("invalid stored values fall back instead of breaking the theme", () => {
  expect(resolvePalette("not-a-palette", "marmor")).toBe("marmor");
  expect(resolvePalette("not-a-palette", "also-bad")).toBe("aurelius");
  expect(resolvePalette(undefined, undefined)).toBe("aurelius");
  expect(resolvePalette(42, null)).toBe("aurelius");
});

test("only a real palette change after the first one animates", () => {
  expect(shouldAnimatePaletteChange("aurelius", "mare")).toBe(true);
  expect(shouldAnimatePaletteChange("mare", "mare")).toBe(false);
  expect(shouldAnimatePaletteChange("mare", "aurelius", { first: true })).toBe(false);
  expect(shouldAnimatePaletteChange("aurelius", "mare", { reducedMotion: true })).toBe(false);
  // Stored garbage resolves to Aurelius, so switching to it from Aurelius is not a change.
  expect(shouldAnimatePaletteChange("bogus", "aurelius")).toBe(false);
  expect(shouldAnimatePaletteChange("bogus", "tyrian")).toBe(true);
});

test("isPalette and paletteOverride accept only known palette ids", () => {
  expect(isPalette("mare")).toBe(true);
  expect(isPalette("aurelius")).toBe(true);
  expect(isPalette("Mare")).toBe(false);
  expect(isPalette("")).toBe(false);
  expect(isPalette(null)).toBe(false);
  expect(paletteOverride("tyrian")).toBe("tyrian");
  expect(paletteOverride("bogus")).toBe(undefined);
  expect(paletteOverride(undefined)).toBe(undefined);
});

const prepaint = readFileSync(new URL("../../static/theme.js", import.meta.url), "utf8");
const metas = JSON.stringify([
  { id: "one", name: "One", palette: "tyrian" },
  { id: "two", name: "Two", palette: "aurelius" },
  { id: "three", name: "Three", palette: "bad" },
]);

test.each([
  { label: "main", saved: { "win:main": '{"active":"one"}' }, expected: "tyrian" },
  { label: "main", saved: { "win:main": '{"active":"two"}' }, expected: "aurelius" },
  { label: "w-other", saved: { "win:w-other": '{"active":"one"}' }, expected: "tyrian" },
  { label: "main", saved: { "win:w-other": '{"active":"one"}' }, expected: "tyrian" },
  { label: "main", saved: { "win:main": '{"active":"three"}' }, expected: "mare" },
  { label: "main", saved: { "win:main": '{"active":"missing"}' }, expected: "mare" },
  { label: "main", saved: { "win:main": "malformed" }, expected: "mare" },
])("pre-paint theme restores $expected in $label", ({ label, saved, expected }) => {
  const storage: Record<string, string | undefined> = { palette: "mare", workspaces: metas, ...saved };
  Object.defineProperty(storage, "getItem", { value: (key: string) => storage[key] ?? null });
  const root = { dataset: {} as Record<string, string>, classList: { toggle() {} } };
  runInNewContext(prepaint, {
    localStorage: storage,
    document: { documentElement: root },
    window: { __TAURI_INTERNALS__: { metadata: { currentWindow: { label } } } },
    matchMedia: () => ({ matches: false }),
  });
  expect(root.dataset.palette).toBe(expected);
});
