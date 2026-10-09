import { expect, test } from "bun:test";
import { agentBusy } from "./agentBusy";

// Titles as the CLIs actually set them, working then idle.
test("title spinners mean working", () => {
  for (const t of ["◐ Claude Code", "◑ Ok"]) expect(agentBusy("claude", t, 9999)).toBe(true);
  for (const t of ["✳ Claude Code", "✳ Ok"]) expect(agentBusy("claude", t, 0)).toBe(false);
  expect(agentBusy("codex", "⠦ fleet_command", null)).toBe(true);
  expect(agentBusy("codex", "fleet_command", 0)).toBe(false);
  expect(agentBusy("grok", "⠧ - Responding - grok", 9999)).toBe(true);
  expect(agentBusy("grok", "grok", 0)).toBe(false); // idle logo animation: output doesn't count
});

test("untitled agents go by recent output", () => {
  expect(agentBusy("agy", "", 200)).toBe(true);
  expect(agentBusy("agy", "", 5000)).toBe(false);
  expect(agentBusy("opencode", "", null)).toBe(false);
});
