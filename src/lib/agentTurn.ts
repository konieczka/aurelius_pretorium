import type { Shell } from "./agents.svelte";

export type Turn = { requested: boolean; started: number | null };

/** Keep completion evidence across idle polls and approval pauses. A typed request doesn't need the startup guard. */
export function turnDone(turn: Turn, shell: Pick<Shell, "busy" | "asking">, now: number): boolean {
  if (shell.busy) turn.started ??= now;
  if (shell.busy || shell.asking || turn.started === null) return false;
  // Opening prompts share their first output with startup redraws. Delay those, without losing a short turn.
  return turn.requested || now - turn.started >= 3000;
}
