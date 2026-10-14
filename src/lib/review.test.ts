import { expect, test } from "bun:test";
import { FIX, reviewLaunch } from "./review";

test("claude reviews with its own command, and fixes with its flag", () => {
  expect(reviewLaunch("claude", "claude", false)).toEqual({ command: "claude '/code-review'", steps: [] });
  expect(reviewLaunch("claude", "claude", true)).toEqual({ command: "claude '/code-review --fix'", steps: [] });
});

test("grok gets /review as its opening prompt, and the fix once it's done", () => {
  expect(reviewLaunch("grok", "/h/.grok/bin/grok", false)).toEqual({ command: "/h/.grok/bin/grok '/review'", steps: [] });
  expect(reviewLaunch("grok", "grok", true).steps).toEqual([{ when: "done", keys: [FIX, "\r"] }]);
});

test("codex and opencode get /review typed in once they're up", () => {
  const codex = reviewLaunch("codex", "codex", true);
  expect(codex.command).toBe("codex");
  expect(codex.steps[0].when).toBe("ready");
  expect(codex.steps[0].keys[0]).toStartWith("/review Review the changes on this branch");
  expect(codex.steps[1]).toEqual({ when: "done", keys: [FIX, "\r"] });
  const opencode = reviewLaunch("opencode", "opencode", true);
  expect(opencode.command).toBe("opencode");
  expect(opencode.steps[0]).toEqual({ when: "ready", keys: ["/review", "\r", codex.steps[0].keys[0].slice("/review".length), "\r"] });
  expect(opencode.steps[0].keys[2]).toContain("uncommitted ones (staged, unstaged and untracked)");
  expect(opencode.steps[0].keys[2]).toContain("commits that aren't on the base branch yet");
  expect(opencode.steps[1]).toEqual({ when: "done", keys: [FIX, "\r"] });
  expect(reviewLaunch("opencode", "opencode", false).steps).toEqual([opencode.steps[0]]);
});

test("antigravity is asked in words, quoted for the shell", () => {
  const { command, steps } = reviewLaunch("antigravity", "agy", true);
  expect(command).toStartWith("agy -i 'Review the changes");
  expect(command).toEndWith("Then fix them all.'");
  expect(command).toContain("aren'\\''t"); // the shell's way to put ' in a quoted string
  expect(steps).toEqual([]);
});

test("muse is asked in words as its opening prompt", () => {
  const { command, steps } = reviewLaunch("muse", "/h/.local/bin/muse", false);
  expect(command).toStartWith("/h/.local/bin/muse 'Review the changes");
  expect(command).toEndWith("with file and line.'");
  expect(steps).toEqual([]);
  expect(reviewLaunch("muse", "muse", true).command).toEndWith("Then fix them all.'");
});
