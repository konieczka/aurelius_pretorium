import { expect, test } from "bun:test";
import { FIELDS, effective, fits, getPath, optionsFor, parseInput, secretRanges, type ConfigFile, type Field } from "./agentConfig";

const file = (f: Partial<ConfigFile>): ConfigFile => ({
  path: "/h/.codex/config.toml", scope: "global", label: "Global", format: "toml", exists: true, readOnly: false, text: "", values: {}, error: null, ...f,
});
const local = file({ path: "/p/.claude/settings.local.json", scope: "local", label: "Project · just you", values: { permissions: { defaultMode: "plan" } } });
const global = file({ path: "/h/.claude/settings.json", values: { model: "opus", permissions: { defaultMode: "acceptEdits", allow: [] } } });
const broken = file({ path: "/p/.claude/settings.json", scope: "project", values: null, error: "bad" });

test("getPath walks objects only", () => {
  expect(getPath(global.values, ["permissions", "defaultMode"])).toBe("acceptEdits");
  expect(getPath(global.values, ["permissions", "allow", "0"])).toBeUndefined();
  expect(getPath(null, ["model"])).toBeUndefined();
});

test("the first file that sets a key decides it, skipping files that don't parse", () => {
  const files = [local, broken, global];
  expect(effective(files, ["permissions", "defaultMode"])).toEqual({ file: local, value: "plan" });
  expect(effective(files, ["model"])).toEqual({ file: global, value: "opus" });
  expect(effective(files, ["effortLevel"])).toBeNull();
});

const effort = FIELDS.codex.find((f) => f.key[0] === "model_reasoning_effort")!;

test("effort options follow the chosen model, keeping a value set outside them", () => {
  const files = [file({ values: { model: "gpt-x" } })];
  const models = [{ id: "gpt-x", name: "X", efforts: ["low", "max"] }];
  expect(optionsFor(effort, files, models, undefined).map((o) => o.value)).toEqual(["low", "max"]);
  expect(optionsFor(effort, files, [], "ultra").map((o) => o.value)).toEqual(["low", "medium", "high", "xhigh", "ultra"]);
});

test("controls only show values of their own type", () => {
  const bool: Field = { key: ["autoupdate"], label: "", help: "", kind: "bool" };
  expect(fits(bool, undefined)).toBe(true);
  expect(fits(bool, false)).toBe(true);
  expect(fits(bool, "notify")).toBe(false);
  expect(fits(effort, { granular: {} })).toBe(false);
});

test("input becomes the stored value; blank unsets", () => {
  const num: Field = { key: ["n"], label: "", help: "", kind: "number" };
  expect(parseInput(num, " 80 ")).toBe(80);
  expect(parseInput(num, "")).toBeUndefined();
  expect(parseInput({ ...num, kind: "bool" }, "false")).toBe(false);
  expect(parseInput(effort, " high ")).toBe("high");
});

test("secret values are found in JSON and TOML lines", () => {
  const line = `  "ANTHROPIC_API_KEY": "sk-123", "model": "opus"`;
  expect(secretRanges(line).map(([a, z]) => line.slice(a, z))).toEqual(["sk-123"]);
  const toml = `Authorization = 'Bearer abc'`;
  expect(secretRanges(toml).map(([a, z]) => toml.slice(a, z))).toEqual(["Bearer abc"]);
  expect(secretRanges(`token_header = ""`)).toEqual([]);
  expect(secretRanges(`effortLevel = "high"`)).toEqual([]);
});

test("muse's form only edits keys its CLI accepts", () => {
  // Checked with `muse exec -c key=value`: unknown keys refuse the run, and so would its settings file.
  expect(FIELDS.muse.map((f) => f.key.join("."))).toEqual(["model", "reasoning_effort", "tui.terminal_background", "session_retention_days", "telemetry.enabled"]);
});
