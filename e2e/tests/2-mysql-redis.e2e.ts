// Phase 2: MySQL/MariaDB in the same SQL view, and Redis: keys by pattern, values by type, and a console.
import { test } from "@e2e-dev/web";
import { expect } from "e2e";

test("a MariaDB container opens with its tables, logged in as its app user", async ({ app, screen, browser }) => {
  await app.open("/e2e");
  await screen.getByRole("button", /^mariadb /).tap();

  await expect(screen.getByText("via container settings")).toBeVisible();
  await expect(browser.locator('[title="Logged in as"]')).toHaveText("shop");
  const tables = screen.getByRole("navigation", "Tables");
  await expect(tables.getByRole("button", /^products/)).toBeVisible();
  await expect(tables.getByRole("button", /^suppliers/)).toBeVisible();

  await tables.getByRole("button", /^products/).tap();
  await expect(screen.getByText("1–6 of 6")).toBeVisible();
  await expect(screen.getByRole("columnheader", /^price/)).toContainText("decimal(8,2)");
  await screen.getByRole("columnheader", /^price/).tap();
  await screen.getByRole("columnheader", /^price/).tap();
  await expect(screen.getByRole("row").nth(1)).toContainText("Grinder");
  await expect(screen.getByRole("row").nth(1)).toContainText("89.90");
});

test("runs MariaDB queries, and refuses a write while the tab is read-only", async ({ app, screen }) => {
  await app.open("/e2e");
  await screen.getByRole("button", /^mariadb /).tap();
  await screen.getByRole("tab", "Query").tap();

  const editor = screen.getByRole("textbox", "SQL query");
  await editor.fill("SELECT name, JSON_LENGTH(tags) AS tags FROM shop.products WHERE tags IS NOT NULL ORDER BY id");
  await screen.getByRole("button", "Run").tap();
  await expect(screen.getByRole("status")).toContainText("5 rows");
  await expect(screen.getByRole("row").filter({ hasText: "Moka pot" })).toContainText("2");

  await editor.fill("DELETE FROM shop.products");
  await screen.getByRole("button", "Run").tap();
  await expect(screen.getByRole("alert")).toContainText("Cannot execute statement in a READ ONLY transaction");
});

test("browses Redis keys by pattern and shows each value by its type", async ({ app, screen }) => {
  await app.open("/e2e");
  await screen.getByRole("button", /^redis /).tap();
  await expect(screen.getByText("via container settings")).toBeVisible(); // its --requirepass

  const keys = screen.getByRole("navigation", "Keys");
  for (const k of ["counter", "leaderboard", "queue:emails", "session:abc", "tags", "user:1"]) {
    await expect(keys.getByRole("button", new RegExp(k))).toBeVisible();
  }

  await screen.getByRole("textbox", "Key pattern").fill("queue:*");
  await screen.getByRole("textbox", "Key pattern").press("Enter");
  await expect(keys.getByRole("button")).toHaveCount(1);
  await keys.getByRole("button", /queue:emails/).tap();
  const value = screen.getByRole("region", "Value");
  await expect(value.getByRole("listitem")).toHaveText(["welcome", "reminder", "digest"]);

  await screen.getByRole("textbox", "Key pattern").fill("*");
  await screen.getByRole("textbox", "Key pattern").press("Enter");
  await keys.getByRole("button", /leaderboard/).tap();
  await expect(value.getByRole("row").filter({ hasText: "ada" })).toContainText("30");
  await keys.getByRole("button", /user:1/).tap();
  await expect(value.getByRole("cell", "ada@example.com")).toBeVisible();
  await keys.getByRole("button", /session:abc/).tap();
  await expect(value).toContainText("expires in");
  await expect(value).toContainText('{"user":1}');
});

test("the Redis console reads, refuses writes while read-only, and follows the picked database", async ({ app, screen }) => {
  await app.open("/e2e");
  await screen.getByRole("button", /^redis /).tap();
  const command = screen.getByRole("textbox", "Redis command");
  const log = screen.getByRole("log", "Console output");

  await command.fill("GET counter");
  await command.press("Enter");
  await expect(log).toContainText('"41"');
  await command.fill("SET counter 1");
  await command.press("Enter");
  await expect(log).toContainText("SET can change data: allow writes to run it");

  await screen.getByRole("combobox", "Database number").selectOption("2");
  await expect(screen.getByRole("navigation", "Keys").getByRole("button", /archive:2019/)).toBeVisible();
  await command.fill("DBSIZE");
  await command.press("Enter");
  await expect(log).toContainText("(integer) 1");
});
