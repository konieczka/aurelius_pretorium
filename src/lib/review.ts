// The Fleet sidebar's Review button: how each agent is started to review the folder's branch, and (Auto-fix) made to
// fix what it found. Claude and Grok run their review command given as the opening prompt; Codex and OpenCode only
// when it's typed in (as an opening prompt it's sent as plain text); Antigravity and Muse Code have none, so they're
// asked in words.
import type { Step } from "./agents.svelte";
import type { Provider } from "./usageStats";

const SCOPE = "the changes on this branch: uncommitted ones (staged, unstaged and untracked) and its commits that aren't on the base branch yet";
const ASK = `Review ${SCOPE}. Look for bugs, regressions and risky code, confirm each one before reporting it, and list them most severe first, with file and line.`;
export const FIX = "Fix every issue you found in the review.";

/** Quoted for the shell: 'it'\''s' */
const quote = (s: string) => `'${s.replaceAll("'", `'\\''`)}'`;

/** What to type in a shell to start a review with `agent` (started by `command`), and what to type into it after. */
export function reviewLaunch(agent: Provider, command: string, fix: boolean): { command: string; steps: Step[] } {
  const fixAfter: Step[] = fix ? [{ when: "done", keys: [FIX, "\r"] }] : [];
  switch (agent) {
    case "claude":
      return { command: `${command} ${quote(fix ? "/code-review --fix" : "/code-review")}`, steps: [] };
    case "grok":
      return { command: `${command} ${quote("/review")}`, steps: fixAfter };
    case "codex": // with instructions it skips its menu of what to review
      return { command, steps: [{ when: "ready", keys: [`/review Review ${SCOPE}.`, "\r"] }, ...fixAfter] };
    case "opencode": // accept the command completion, then add its scope before sending
      return { command, steps: [{ when: "ready", keys: ["/review", "\r", ` Review ${SCOPE}.`, "\r"] }, ...fixAfter] };
    case "antigravity":
      return { command: `${command} -i ${quote(fix ? `${ASK} Then fix them all.` : ASK)}`, steps: [] };
    case "muse": // a prompt after its options starts an interactive session with it
      return { command: `${command} ${quote(fix ? `${ASK} Then fix them all.` : ASK)}`, steps: [] };
  }
}
