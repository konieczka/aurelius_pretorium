// Working or not: these put a spinner at the start of the terminal title while working (Claude ◐◑, Codex and Grok
// braille) and drop it when done. Output is no use for them: Grok animates its logo while idle. The rest are judged by
// output that isn't an echo of input: idle agents' screens stay still.
export const TITLED = new Set(["claude", "codex", "grok"]);
const SPINNER = /^[⠀-⣿◐-◓]/;
const QUIET = 1500; // ms without output that counts as idle

/** `quiet` = ms since the shell's last output that wasn't an echo of input (null: none yet). */
export function agentBusy(agent: string, title: string, quiet: number | null) {
  return TITLED.has(agent) ? SPINNER.test(title) : quiet !== null && quiet < QUIET;
}
