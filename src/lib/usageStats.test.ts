import { expect, test } from "bun:test";
import { fmtLeft, fmtShare, fmtTokens, pace, rangeStart, summarize, type Row } from "./usageStats";

const now = new Date(2026, 9, 9, 14, 30).getTime(); // local time
const at = (day: number, hour: number) => new Date(2026, 9, day, hour).getTime();
const row = (r: Partial<Row>): Row => ({ provider: "claude", model: "claude-opus-5-5", session: "s1", cwd: "", hour: at(9, 10), uncached: 0, cached: 0, output: 0, total: 0, ...r });

test("range starts at local midnight", () => {
  expect(rangeStart(1, now)).toBe(at(9, 0));
  expect(rangeStart(7, now)).toBe(at(3, 0));
});

test("summarize buckets by local day, provider and model", () => {
  const s = summarize(
    [
      row({ total: 100, cached: 80, uncached: 15, output: 5 }),
      row({ hour: at(8, 23), total: 50, cached: 40, uncached: 5, output: 5 }),
      row({ provider: "codex", model: "gpt-6-astra", session: "c1", hour: at(9, 1), total: 40, cached: 30, uncached: 6, output: 4 }),
      row({ provider: "grok", model: "grok-4.7-build", session: "g1", hour: at(8, 0), total: 10 }),
      row({ hour: at(2, 23), total: 999 }), // before the range
    ],
    7,
    now,
  );
  expect(s.totals).toEqual({ processed: 200, cached: 150, uncached: 26, output: 14 });
  expect(s.sessions).toBe(3);
  expect(s.providers.map((p) => [p.provider, p.tokens, p.sessions, p.share])).toEqual([
    ["claude", 150, 1, 75],
    ["codex", 40, 1, 20],
    ["grok", 10, 1, 5],
  ]);
  expect(s.models.map((m) => m.model)).toEqual(["claude-opus-5-5", "gpt-6-astra", "grok-4.7-build"]);
  expect(s.days).toHaveLength(7);
  expect(s.days[6]).toMatchObject({ start: at(9, 0), total: 140, sessions: 2, by: { claude: 100, codex: 40, grok: 0 } });
  expect(s.days[5]).toMatchObject({ total: 60, sessions: 2, by: { claude: 50, codex: 0, grok: 10 } });
});

test("formatters", () => {
  expect([999, 146_000, 925_400, 1_374_666, 17_800_000, 437_200_000, 1_234_000_000].map(fmtTokens)).toEqual(["999", "146K", "925K", "1.37M", "17.8M", "437M", "1.23B"]);
  expect(fmtTokens(18_000_000)).toBe("18M");
  expect([0, 0.04, 9, 73.62].map(fmtShare)).toEqual(["0%", "<0.1%", "9.0%", "73.6%"]);
  expect(fmtLeft(now + (5 * 1440 + 6 * 60 + 10) * 60_000, now)).toBe("5d 6h");
  expect(fmtLeft(now + 65 * 60_000, now)).toBe("1h 5m");
  expect(fmtLeft(now + 12 * 60_000, now)).toBe("12m");
  expect(fmtLeft(now - 1000, now)).toBe("now");
});

test("pace projects usage at reset", () => {
  const week = 7 * 1440;
  // Half the window gone, 30% used -> on course for 60%.
  expect(pace({ provider: "claude", label: "Weekly", used: 30, resetsAt: now + (week / 2) * 60_000, windowMinutes: week, note: null }, now)).toEqual({ projected: 60, over: false });
  expect(pace({ provider: "claude", label: "Session", used: 80, resetsAt: now + 150 * 60_000, windowMinutes: 300, note: null }, now)?.over).toBe(true);
  expect(pace({ provider: "claude", label: "Session", used: 1, resetsAt: now + 299 * 60_000, windowMinutes: 300, note: null }, now)).toBeNull();
});
