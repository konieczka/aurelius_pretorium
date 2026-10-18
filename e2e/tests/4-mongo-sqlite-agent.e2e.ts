// Phase 4: Mongo (through mongosh inside its container), the project's SQLite files, and handing a question about a
// database to the coding agent.
import { test } from "@e2e-dev/web";
import { beforeEach, expect } from "e2e";
import { resetNotes } from "./reset.ts";

beforeEach(resetNotes);

test("browses a Mongo container's collections and documents, filtered with JSON", async ({ app, screen }) => {
  await app.open("/e2e");
  await screen.getByRole("button", /^mongo /).tap();

  await expect(screen.getByText("container pretorium-e2e-mongo")).toBeVisible();
  await expect(screen.getByText("via container settings")).toBeVisible();
  await expect(screen.getByRole("combobox", "Database")).toHaveValue("shop");
  await expect(screen.getByRole("tab", "Query")).toBeHidden(); // documents are only read
  await expect(screen.getByRole("button", "Read-only")).toBeHidden();

  const collections = screen.getByRole("navigation", "Tables");
  await expect(collections.getByRole("button", /^customers/)).toContainText("1");
  await collections.getByRole("button", /^reviews/).tap();
  await expect(screen.getByText("1–30 of 30")).toBeVisible();

  await screen.getByRole("textbox", "Filter").fill('{ "product": "Moka pot", "stars": { "$gte": 4 } }');
  await screen.getByRole("textbox", "Filter").press("Enter");
  await expect(screen.getByText("1–4 of 4")).toBeVisible();
  await screen.getByRole("cell", /"text":"Review 4"/).tap();
  const detail = screen.getByRole("region", "Cell value");
  await expect(detail).toContainText('"product": "Moka pot"');
  await expect(detail).toContainText('"name": "User 3"');
});

test("a Mongo filter that isn't JSON says so", async ({ app, screen }) => {
  await app.open("/e2e");
  await screen.getByRole("button", /^mongo /).tap();
  await screen.getByRole("navigation", "Tables").getByRole("button", /^reviews/).tap();
  await screen.getByRole("textbox", "Filter").fill("{ product: Moka }");
  await screen.getByRole("textbox", "Filter").press("Enter");
  await expect(screen.getByRole("alert")).toContainText("The filter is not JSON");
});

test("the project's SQLite files are listed with its servers, and browsed like any database", async ({ app, screen }) => {
  await app.open("/e2e");
  await screen.getByRole("button", /^notes\.sqlite data/).tap();

  await expect(screen.getByText("SQLite", { exact: true })).toBeVisible();
  await screen.getByRole("navigation", "Tables").getByRole("button", /^notes/).tap();
  await expect(screen.getByText("1–3 of 3")).toBeVisible();
  await expect(screen.getByRole("cell", "milk, coffee")).toBeVisible();

  await screen.getByRole("tab", "Query").tap();
  const editor = screen.getByRole("textbox", "SQL query");
  await editor.fill("UPDATE notes SET pinned = 1 WHERE title <> 'Shopping'");
  await screen.getByRole("button", "Run").tap();
  await expect(screen.getByRole("alert")).toContainText("readonly");

  await screen.getByRole("button", "Read-only").tap();
  await screen.getByRole("button", "Allow writes").tap();
  await screen.getByRole("button", "Run").tap();
  await expect(screen.getByRole("status")).toContainText("2 rows changed");
  await editor.fill("SELECT count(*) AS pinned FROM notes WHERE pinned = 1");
  await screen.getByRole("button", "Run").tap();
  await expect(screen.getByRole("cell", "3")).toBeVisible();
});

test("asking the agent opens a terminal that hands it the question and the tables", async ({ app, screen, browser }) => {
  await app.open("/e2e");
  await screen.getByRole("button", /^pg postgres/).tap();
  await screen.getByRole("tab", "Query").tap();

  await screen.getByRole("button", /^Ask claude/).tap();
  const question = screen.getByPlaceholder("Ask claude about this database");
  await question.fill("Which customers spent the most?");
  await question.press("Enter");

  await expect(screen.getByText("Terminal", { exact: true })).toBeVisible(); // the agent's, next to the database's tab
  // What the terminal was told to run (terminals answer through the harness's recorder: no real shell here).
  const typed = () =>
    browser.evaluate(() =>
      ((window as unknown as { __e2eCalls: { cmd: string; args: { data?: string } | null }[] }).__e2eCalls ?? [])
        .filter((c) => c.cmd === "pty_write")
        .map((c) => c.args?.data ?? "")
        .join(""),
    );
  await expect.poll(typed).toContain("claude 'Which customers spent the most?");
  const sent = await typed();
  expect(sent).toContain("database shop");
  expect(sent).toContain("public.customers (id, name, email, vip, meta)");
  expect(sent).toContain("public.orders (id, customer_id, total, placed_at)");
});
