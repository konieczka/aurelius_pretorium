import { expect, test } from "bun:test";
import { turnDone, type Turn } from "./agentTurn";

const idle = { busy: false, asking: null } as const;
const busy = { busy: true, asking: null } as const;

test("a requested review completes on its first idle poll even after less than three seconds", () => {
  const turn: Turn = { requested: true, started: null };
  expect(turnDone(turn, idle, 0)).toBe(false);
  expect(turnDone(turn, busy, 1000)).toBe(false);
  expect(turnDone(turn, idle, 2000)).toBe(true);
});

test("a short opening review retains completion evidence through the startup guard", () => {
  const turn: Turn = { requested: false, started: null };
  expect(turnDone(turn, busy, 1000)).toBe(false);
  expect(turnDone(turn, idle, 2000)).toBe(false);
  expect(turnDone(turn, idle, 3000)).toBe(false);
  expect(turnDone(turn, idle, 4000)).toBe(true);
});

test("approval and question pauses retain the review's progress without triggering fixes", () => {
  for (const requested of [false, true]) {
    for (const asking of ["approval", "question"] as const) {
      const turn: Turn = { requested, started: null };
      expect(turnDone(turn, busy, 1000)).toBe(false);
      expect(turnDone(turn, busy, 5000)).toBe(false);
      expect(turnDone(turn, { busy: false, asking }, 6000)).toBe(false);
      expect(turnDone(turn, { busy: true, asking }, 7000)).toBe(false);
      expect(turnDone(turn, { busy: false, asking }, 8000)).toBe(false);
      expect(turnDone(turn, busy, 9000)).toBe(false);
      expect(turnDone(turn, idle, 10_000)).toBe(true);
    }
  }
});

test("idle startup and active work cannot complete a review", () => {
  const turn: Turn = { requested: false, started: null };
  expect(turnDone(turn, idle, 20_000)).toBe(false);
  expect(turnDone(turn, busy, 21_000)).toBe(false);
  expect(turnDone(turn, busy, 30_000)).toBe(false);
  expect(turnDone(turn, idle, 31_000)).toBe(true);
});
