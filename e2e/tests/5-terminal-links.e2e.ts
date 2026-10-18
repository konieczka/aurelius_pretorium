// Phase 5: file paths in a terminal's output (an agent naming the files it wrote, a compiler's "file:line:col") are
// links that open the file in a tab. Terminals have no shell here: the test writes their output itself.
import { test, type Browser } from "@e2e-dev/web";
import { expect } from "e2e";

type Screen = Parameters<Parameters<typeof test>[1]>[0]["screen"];
type Terms = Record<string, { onData: { onmessage: (buf: ArrayBuffer) => void }; cols: number; rows: number }>;

/** Show `text` in the terminal, from its top left corner. */
const print = (browser: Browser, text: string) =>
  browser.evaluate((t) => {
    const term = Object.values((window as unknown as { __e2eTerms: Terms }).__e2eTerms).at(-1)!;
    term.onData.onmessage(new TextEncoder().encode("\x1b[2J\x1b[H" + t).buffer as ArrayBuffer);
    return null;
  }, text);

/** Point at the middle of the terminal's cell in column `col` of row `row` (0-based), and optionally click it. */
async function pointAt(browser: Browser, col: number, row: number, click = false) {
  const { x, y } = await browser.evaluate(
    ([c, r]) => {
      const term = Object.values((window as unknown as { __e2eTerms: Terms }).__e2eTerms).at(-1)!;
      const box = document.querySelector(".xterm-screen")!.getBoundingClientRect();
      return { x: box.left + ((c + 0.5) * box.width) / term.cols, y: box.top + ((r + 0.5) * box.height) / term.rows };
    },
    [col, row],
  );
  await browser.mouse.move(x - 3, y);
  await browser.mouse.move(x, y);
  if (click) await browser.mouse.down(), await browser.mouse.up();
}

/** A new terminal from the pane's bar (its buttons show while the pointer is over it). */
async function openTerminal(browser: Browser, screen: Screen) {
  await expect(screen.getByText("⌘T new terminal")).toBeVisible(); // the empty pane's hint
  const bar = await browser.evaluate(() => {
    const r = document.querySelector(".pane .bar")!.getBoundingClientRect();
    return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
  });
  await browser.mouse.move(bar.x, bar.y);
  await screen.getByRole("button", "New terminal (⌘T)").tap();
  await expect(browser.locator(".xterm-screen")).toBeVisible();
}

const OUTPUT = [
  "I wrote up the opening hours as two new files, not committed:",
  "",
  "  - docs/hours.md — the hours.",
  "  - docs/holidays.md — not written yet.",
  "",
  "error[E0308]: src/orders.ts:3:14 expected number",
].join("\r\n");

test("a file the terminal names opens in a tab beside it", async ({ app, screen, browser }) => {
  await app.open("/e2e");
  await openTerminal(browser, screen);
  await print(browser, OUTPUT);

  const pointer = browser.locator(".xterm-cursor-pointer");
  await pointAt(browser, 6, 2); // on "docs/hours.md"
  await expect(pointer).toBeVisible();
  await pointAt(browser, 6, 2, true);

  await expect(screen.getByText("hours.md", { exact: true })).toBeVisible();
  await expect(screen.getByRole("heading", "Opening hours")).toBeVisible(); // Markdown opens as its preview
  await expect(screen.getByText("Terminal", { exact: true })).toBeVisible(); // still in view, in its own pane
});

test("a path that isn't a file stays text", async ({ app, screen, browser }) => {
  await app.open("/e2e");
  await openTerminal(browser, screen);
  await print(browser, OUTPUT);

  const pointer = browser.locator(".xterm-cursor-pointer");
  await pointAt(browser, 6, 2); // a file: the pointer shows it's a link
  await expect(pointer).toBeVisible();
  await pointAt(browser, 6, 3); // "docs/holidays.md" isn't there
  await expect(pointer).toBeHidden();
  await pointAt(browser, 6, 3, true);
  await pointAt(browser, 3, 0, true); // nor is "wrote"
  await expect(screen.getByText("holidays.md", { exact: true })).toBeHidden();
  await expect(screen.getByText("Terminal", { exact: true })).toBeVisible();
});

test("a file with a line and column opens its text there", async ({ app, screen, browser }) => {
  await app.open("/e2e");
  await openTerminal(browser, screen);
  await print(browser, OUTPUT);

  await pointAt(browser, 18, 5, true); // on "src/orders.ts:3:14"
  await expect(screen.getByText("orders.ts", { exact: true })).toBeVisible();
  await expect(browser.locator(".cm-activeLine")).toContainText("export const totalOf"); // the cursor's line
});
