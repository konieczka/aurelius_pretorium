// Phase 3: write mode. A tab is read-only until writes are allowed (after a confirmation); then queries may change
// data, and a cell edit shows the UPDATE it runs (keyed on the primary key) before it runs it.
import { test } from "@e2e-dev/web";
import { beforeEach, expect } from "e2e";
import { resetWrites } from "./reset.ts";
import type { Screen } from "e2e";

beforeEach(resetWrites);

async function allowWrites(screen: Screen) {
  await screen.getByRole("button", "Read-only").tap();
  await expect(screen.getByRole("alertdialog", "Allow writes")).toBeVisible();
  await screen.getByRole("button", "Allow writes").tap();
  await expect(screen.getByRole("button", "Writes allowed")).toBeVisible();
}

test("allowing writes asks first; then a query can change data", async ({ app, screen }) => {
  await app.open("/e2e");
  await screen.getByRole("button", /^pg postgres/).tap();

  await screen.getByRole("button", "Read-only").tap();
  await expect(screen.getByRole("alertdialog", "Allow writes")).toContainText("change and delete data in shop");
  await screen.getByRole("button", "Cancel").tap();
  await expect(screen.getByRole("button", "Read-only")).toBeVisible();

  await allowWrites(screen);
  await screen.getByRole("tab", "Query").tap();
  const editor = screen.getByRole("textbox", "SQL query");
  await editor.fill("UPDATE settings SET value = 'PLN' WHERE key = 'currency'");
  await screen.getByRole("button", "Run").tap();
  await expect(screen.getByRole("status")).toContainText("1 row changed");
  await editor.fill("SELECT value FROM settings WHERE key = 'currency'");
  await screen.getByRole("button", "Run").tap();
  await expect(screen.getByRole("cell", "PLN")).toBeVisible();
});

test("editing a cell shows the UPDATE first, and runs it once applied", async ({ app, screen }) => {
  await app.open("/e2e");
  await screen.getByRole("button", /^pg postgres/).tap();
  await allowWrites(screen);
  await screen.getByRole("navigation", "Tables").getByRole("button", /^settings/).tap();

  await screen.getByRole("cell", "dark").doubleTap();
  await screen.getByRole("textbox", "New value for value").fill("light");
  await screen.getByRole("textbox", "New value for value").press("Enter");
  const confirm = screen.getByRole("alertdialog", "Confirm edit");
  await expect(confirm).toContainText(`UPDATE "public"."settings" SET "value" = 'light' WHERE "key" = 'theme'`);
  await confirm.getByRole("button", "Apply").tap();
  await expect(screen.getByRole("cell", "light")).toBeVisible();
  await expect(screen.getByRole("cell", "dark")).toBeHidden();
});

test("a cancelled edit changes nothing, and a value can be set to NULL", async ({ app, screen }) => {
  await app.open("/e2e");
  await screen.getByRole("button", /^pg postgres/).tap();
  await allowWrites(screen);
  await screen.getByRole("navigation", "Tables").getByRole("button", /^settings/).tap();

  await screen.getByRole("cell", "hello").doubleTap();
  await screen.getByRole("textbox", "New value for value").fill("bye");
  await screen.getByRole("textbox", "New value for value").press("Enter");
  await screen.getByRole("alertdialog", "Confirm edit").getByRole("button", "Cancel").tap();
  await screen.getByRole("button", "Refresh").tap();
  await expect(screen.getByRole("cell", "hello")).toBeVisible();

  await screen.getByRole("cell", "none").tap();
  await screen.getByRole("region", "Cell value").getByRole("button", "Set to NULL").tap();
  const confirm = screen.getByRole("alertdialog", "Confirm edit");
  await expect(confirm).toContainText(`SET "value" = NULL WHERE "key" = 'notes'`);
  await confirm.getByRole("button", "Apply").tap();
  await expect(screen.getByRole("row").filter({ hasText: "notes" })).toContainText("NULL");
});

test("says why a cell can't be edited: read-only, or no primary key", async ({ app, screen }) => {
  await app.open("/e2e");
  await screen.getByRole("button", /^pg postgres/).tap();
  const tables = screen.getByRole("navigation", "Tables");

  await tables.getByRole("button", /^settings/).tap();
  await screen.getByRole("cell", "en").doubleTap();
  await expect(screen.getByRole("status")).toHaveText("Allow writes (in the bar above) to edit cells");
  await expect(screen.getByRole("textbox", /^New value/)).toBeHidden();

  await allowWrites(screen);
  await tables.getByRole("button", /^events/).tap();
  await screen.getByRole("cell", "signup").doubleTap();
  await expect(screen.getByRole("status")).toContainText("no primary key");
});

test("MariaDB cells and Redis keys can be changed once writes are allowed", async ({ app, screen }) => {
  await app.open("/e2e");
  await screen.getByRole("button", /^mariadb /).tap();
  await allowWrites(screen);
  await screen.getByRole("navigation", "Tables").getByRole("button", /^products/).tap();
  const kettle = screen.getByRole("row").filter({ hasText: "Kettle" });
  await kettle.getByRole("cell", "0").doubleTap();
  await screen.getByRole("textbox", "New value for stock").fill("7");
  await screen.getByRole("textbox", "New value for stock").press("Enter");
  await expect(screen.getByRole("alertdialog", "Confirm edit")).toContainText("UPDATE `shop`.`products` SET `stock` = '7' WHERE `id` = '4'");
  await screen.getByRole("button", "Apply").tap();
  await expect(kettle.getByRole("cell", "7")).toBeVisible();

  await screen.getByRole("button", /^redis /).tap();
  await allowWrites(screen);
  const command = screen.getByRole("textbox", "Redis command");
  await command.fill('SET greeting "hi there"');
  await command.press("Enter");
  await command.fill("GET greeting");
  await command.press("Enter");
  await expect(screen.getByRole("log", "Console output")).toContainText('"hi there"');
  await expect(screen.getByRole("navigation", "Keys").getByRole("button", /greeting/)).toBeVisible();
});
