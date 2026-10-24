// Phase 6: agents in a terminal (Claude Code) follow the app's light and dark theme through mode 2031's reports, so a
// running one switches its colours with the app. Terminals have no shell here: the test writes their output itself.
import { test, type Browser } from "@e2e-dev/web";
import { expect } from "e2e";

type Screen = Parameters<Parameters<typeof test>[1]>[0]["screen"];
type Terms = Record<string, { onData: { onmessage: (buf: ArrayBuffer) => void } }>;
type Calls = { cmd: string; args: { data?: string } | null }[];

/** What the program in a terminal prints: the newest one's, or the `nth` opened. */
const print = (browser: Browser, text: string, nth = -1) =>
  browser.evaluate(([t, n]) => {
    const term = Object.values((window as unknown as { __e2eTerms: Terms }).__e2eTerms).at(n)!;
    term.onData.onmessage(new TextEncoder().encode(t).buffer as ArrayBuffer);
    return null;
  }, [text, nth] as const);

/** What the terminals answered their programs that starts with `prefix`, oldest first. */
const answers = (browser: Browser, prefix: string) =>
  browser.evaluate(
    (p) =>
      (window as unknown as { __e2eCalls: Calls }).__e2eCalls
        .map((c) => (c.cmd === "pty_write" ? c.args?.data : undefined))
        .filter((d) => d?.startsWith(p)),
    prefix,
  );

/** Whether an OSC 11 answer (rgb:rrrr/gggg/bbbb) is a dark background. */
const darkBg = (answer: string) => {
  const [r, g, b] = answer.match(/rgb:(\w+)\/(\w+)\/(\w+)/)!.slice(1).map((h) => parseInt(h.slice(0, 2), 16) / 255);
  return 0.2126 * r + 0.7152 * g + 0.0722 * b < 0.5;
};

/** Theme reports the terminal sent its program, oldest first. */
const reports = (browser: Browser) =>
  browser.evaluate(() =>
    (window as unknown as { __e2eCalls: Calls }).__e2eCalls
      .map((c) => (c.cmd === "pty_write" ? c.args?.data : undefined))
      .filter((d) => d?.startsWith("\x1b[?997;"))
      .map((d) => (d === "\x1b[?997;1n" ? "dark" : "light")),
  );

async function openTerminal(browser: Browser, screen: Screen, first = true) {
  if (first) await expect(screen.getByText("⌘T new terminal")).toBeVisible();
  const bar = await browser.evaluate(() => {
    const r = document.querySelector(".pane .bar")!.getBoundingClientRect();
    return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
  });
  await browser.mouse.move(bar.x, bar.y);
  await screen.getByRole("button", "New terminal (⌘T)").tap();
  await expect(browser.locator(".xterm-screen")).toBeVisible();
}

const settle = () => new Promise((r) => setTimeout(r, 300));

test("a program that asks for theme reports gets one when the theme changes", async ({ app, screen, browser }) => {
  await browser.emulateMedia({ colorScheme: "light" });
  await app.open("/e2e");
  await openTerminal(browser, screen);

  await print(browser, "\x1b[?996n"); // which theme now?
  await settle();
  expect(await reports(browser)).toEqual(["light"]);

  await browser.emulateMedia({ colorScheme: "dark" }); // not asked for reports yet: nothing
  await settle();
  expect(await reports(browser)).toEqual(["light"]);

  await print(browser, "\x1b[?2031h");
  await browser.emulateMedia({ colorScheme: "light" });
  await settle();
  await browser.emulateMedia({ colorScheme: "dark" });
  await settle();
  expect(await reports(browser)).toEqual(["light", "light", "dark"]);

  await print(browser, "\x1b[?2031l"); // the program quit
  await browser.emulateMedia({ colorScheme: "light" });
  await settle();
  expect(await reports(browser)).toEqual(["light", "light", "dark"]);
});

test("a program can tell the terminal does theme reports", async ({ app, screen, browser }) => {
  await app.open("/e2e");
  await openTerminal(browser, screen);

  await print(browser, "\x1b[?2031$p"); // OpenCode asks before turning them on
  await settle();
  await print(browser, "\x1b[?2031h\x1b[?2031$p");
  await settle();
  expect(await answers(browser, "\x1b[?2031;")).toEqual(["\x1b[?2031;2$y", "\x1b[?2031;1$y"]);
});

test("a terminal in a tab behind another answers with the theme's background", async ({ app, screen, browser }) => {
  await browser.emulateMedia({ colorScheme: "light" });
  await app.open("/e2e");
  await openTerminal(browser, screen);
  await openTerminal(browser, screen, false); // the first one's tab is now behind it

  // Claude Code, on a theme report, asks the background (OSC 11) to pick its colours from.
  await browser.emulateMedia({ colorScheme: "dark" });
  await settle();
  await print(browser, "\x1b]11;?\x07", 0);
  await settle();
  await browser.emulateMedia({ colorScheme: "light" });
  await settle();
  await print(browser, "\x1b]11;?\x07", 0);
  await settle();
  expect((await answers(browser, "\x1b]11;")).map(darkBg)).toEqual([true, false]);
});
