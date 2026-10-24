// Phase 7: the War Table, every project's agents and servers on one canvas, opened over the whole window. Its page
// (/e2e/war) is the whole app on three workspaces, One, Two and Three, with a Claude tab each in orchard, quarry and
// harbor. Their states come from `window.__e2eShells`, which the tests change.
import { test, type Browser } from "@e2e-dev/web";
import { expect } from "e2e";

type Screen = Parameters<Parameters<typeof test>[1]>[0]["screen"];
type Shell = { busy: boolean; asking: "question" | "approval" | null };
const TABS = { orchard: "t1", quarry: "t2", harbor: "t3" };
const WORKSPACE_ORDER = ["orchard", "quarry", "harbor"]; // by name it would be harbor, orchard, quarry

/** Projects as they stand on the table, by their camps (Castra) or column heads (Front line): rows top to bottom,
 * then left to right. */
const order = (browser: Browser, view: "castra" | "front") =>
  browser.evaluate(
    (sel) =>
      [...document.querySelectorAll<HTMLElement>(sel)]
        .map((el) => ({ name: el.dataset.project!.split("/").pop()!, r: el.getBoundingClientRect() }))
        .sort((a, b) => Math.round(a.r.top) - Math.round(b.r.top) || a.r.left - b.r.left)
        .map((x) => x.name),
    view === "castra" ? ".war .camp" : ".war .head",
  );

/** The agent in `project` starts working, or asks for approval (it's polled every second). */
const setAgent = (browser: Browser, project: keyof typeof TABS, state: Partial<Shell>) =>
  browser.evaluate(
    ([tab, s]) => {
      Object.assign((window as unknown as { __e2eShells: Record<string, Shell> }).__e2eShells[tab], s);
      return null;
    },
    [TABS[project], state] as const,
  );

/** Drag from the middle of `from` to the middle of `to`, in small steps, as a hand would. */
async function drag(browser: Browser, from: string, to: string) {
  const [a, b] = await browser.evaluate(
    (sels) =>
      sels.map((s) => {
        const r = document.querySelector(s)!.getBoundingClientRect();
        return { x: r.left + r.width / 2, y: r.top + Math.min(r.height / 2, 40) };
      }),
    [from, to],
  );
  await browser.mouse.move(a.x, a.y);
  await browser.mouse.down();
  for (let i = 1; i <= 12; i++) await browser.mouse.move(a.x + ((b.x - a.x) * i) / 12, a.y + ((b.y - a.y) * i) / 12);
  await browser.mouse.up();
}

type App = Parameters<Parameters<typeof test>[1]>[0]["app"];

async function openWarTable(app: App, browser: Browser, screen: Screen) {
  await app.open("/e2e/war");
  await screen.getByRole("button", "War Table").tap();
  await expect(browser.locator(".war")).toBeVisible();
  await expect.poll(() => order(browser, "castra")).toEqual(WORKSPACE_ORDER);
}

const frontLine = (screen: Screen) => screen.getByRole("button", "Front line").tap();

test("the War Table stands in for the panes, from the top bar, until it's closed", async ({ app, browser, screen }) => {
  await openWarTable(app, browser, screen);
  await expect(screen.getByRole("button", "War Table")).toHaveAttribute("aria-pressed", "true");
  await expect(browser.locator("main")).toBeHidden(); // the panes are away
  await screen.getByRole("button", "War Table").tap();
  await expect(browser.locator(".war")).toBeHidden();
  await expect(browser.locator("main")).toBeVisible();

  await screen.getByRole("button", "War Table").tap();
  await screen.getByRole("button", "Usage").tap(); // the views take turns
  await expect(browser.locator(".war")).toBeHidden();
});

test("projects keep their places when agents start working or need you", async ({ app, browser, screen }) => {
  await openWarTable(app, browser, screen);
  await setAgent(browser, "harbor", { asking: "approval" });
  await setAgent(browser, "quarry", { busy: true });
  await expect(browser.locator('.war [data-agent="t3"]')).toContainText("Waiting for your approval");
  expect(await order(browser, "castra")).toEqual(WORKSPACE_ORDER);

  await frontLine(screen);
  await expect(browser.locator('.war .card[data-agent="t3"][data-status="attention"]')).toBeVisible();
  await expect.poll(() => order(browser, "front")).toEqual(WORKSPACE_ORDER);
});

test("a Front line column moves where its head is dragged, and stays there", async ({ app, browser, screen }) => {
  await openWarTable(app, browser, screen);
  await frontLine(screen);
  await expect.poll(() => order(browser, "front")).toEqual(WORKSPACE_ORDER);

  await drag(browser, '.war .head[data-project$="/harbor"]', '.war .head[data-project$="/orchard"]');
  await expect.poll(() => order(browser, "front")).toEqual(["harbor", "orchard", "quarry"]);
  await expect(browser.locator(".war .head.lit")).toBeHidden(); // the drag didn't also focus the column

  // Kept for next time, and for Castra too
  await app.open("/e2e/war?keep");
  await screen.getByRole("button", "War Table").tap();
  await expect.poll(() => order(browser, "front")).toEqual(["harbor", "orchard", "quarry"]);
  await screen.getByRole("button", "Castra").tap();
  await expect.poll(() => order(browser, "castra")).toEqual(["harbor", "orchard", "quarry"]);

  await screen.getByRole("button", "Workspace order").tap();
  await expect.poll(() => order(browser, "castra")).toEqual(WORKSPACE_ORDER);
  await expect(screen.getByRole("button", "Workspace order")).toBeHidden();
});

test("a camp dropped on another takes its place", async ({ app, browser, screen }) => {
  await openWarTable(app, browser, screen);
  await drag(browser, '.war .camp[data-project$="/harbor"] .plate', '.war .camp[data-project$="/quarry"] .plate');
  await expect.poll(() => order(browser, "castra")).toEqual(["orchard", "harbor", "quarry"]);
});

test("a click on an agent in another workspace opens its terminal there", async ({ app, browser, screen }) => {
  await openWarTable(app, browser, screen);
  await expect(screen.getByRole("tab", /^One/)).toHaveAttribute("aria-selected", "true");

  await browser.locator('.war [data-agent="t2"]').tap(); // quarry's, in Two
  await expect(browser.locator(".war")).toBeHidden();
  await expect(screen.getByRole("tab", /^Two/)).toHaveAttribute("aria-selected", "true");
  await expect(browser.locator(".pane .where")).toContainText("quarry"); // its pane shows the agent's folder
  // Its terminal came up: a shell was opened for its tab
  await expect
    .poll(() => browser.evaluate(() => (window as unknown as { __e2eCalls: { cmd: string; args: { id?: string } }[] }).__e2eCalls.some((c) => c.cmd === "pty_open" && c.args?.id === "t2")))
    .toEqual(true);
});
