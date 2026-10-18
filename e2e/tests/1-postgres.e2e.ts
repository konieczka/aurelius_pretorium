// Phase 1: Postgres. Over a published port and from inside a container, its tables a page at a time, the query
// editor (read-only by default), and finding the login on its own (else asking for it).
import { test } from "@e2e-dev/web";
import { expect } from "e2e";

test("a Postgres container opens on its tables, logged in from the container's settings", async ({ app, screen }) => {
  await app.open("/e2e");
  await screen.getByRole("button", /^pg postgres/).tap();

  await expect(screen.getByText("via container settings")).toBeVisible();
  await expect(screen.getByRole("combobox", "Database")).toHaveValue("shop");
  const tables = screen.getByRole("navigation", "Tables");
  for (const t of ["events", "big_spenders", "customers", "orders", "settings"]) {
    await expect(tables.getByRole("button", new RegExp(`^${t}`))).toBeVisible();
  }
  await expect(tables.getByRole("button", /^customers/)).toContainText("250");
});

test("pages through a table, sorts it by a column and filters it", async ({ app, screen }) => {
  await app.open("/e2e");
  await screen.getByRole("button", /^pg postgres/).tap();
  await screen.getByRole("navigation", "Tables").getByRole("button", /^customers/).tap();

  await expect(screen.getByText("1–100 of 250")).toBeVisible();
  await expect(screen.getByRole("cell", "Ada Lovelace")).toBeVisible();
  await expect(screen.getByRole("cell", "NULL").first()).toBeVisible(); // customer 2 has no email

  await screen.getByRole("button", "Next page").tap();
  await expect(screen.getByText("101–200 of 250")).toBeVisible();
  await expect(screen.getByRole("cell", "Customer 101")).toBeVisible();

  await screen.getByRole("columnheader", /^name/).tap();
  await screen.getByRole("columnheader", /^name/).tap();
  await expect(screen.getByRole("columnheader", /^name/)).toHaveAttribute("aria-sort", "descending");
  await expect(screen.getByText("1–100 of 250")).toBeVisible();
  await expect(screen.getByRole("row").nth(1)).toContainText("Customer 250");

  await screen.getByRole("textbox", "Filter").fill("vip AND meta->>'tier' = 'gold'");
  await screen.getByRole("textbox", "Filter").press("Enter");
  await expect(screen.getByText("1–25 of 25")).toBeVisible();
});

test("runs a query, and refuses one that writes while the tab is read-only", async ({ app, screen }) => {
  await app.open("/e2e");
  await screen.getByRole("button", /^pg postgres/).tap();
  await screen.getByRole("tab", "Query").tap();

  const editor = screen.getByRole("textbox", "SQL query");
  await editor.fill("SELECT name, email FROM customers WHERE id <= 2 ORDER BY id");
  await screen.getByRole("button", "Run").tap();
  await expect(screen.getByRole("status")).toContainText("2 rows");
  await expect(screen.getByRole("cell", "Ada Lovelace")).toBeVisible();
  await expect(screen.getByRole("cell", "ada@example.com")).toBeVisible();

  await editor.fill("UPDATE customers SET vip = true");
  await screen.getByRole("button", "Run").tap();
  await expect(screen.getByRole("alert")).toContainText("cannot execute UPDATE in a read-only transaction");
  await expect(screen.getByRole("button", "Read-only")).toBeVisible();
});

test("a container that publishes no port is browsed with psql inside it", async ({ app, screen }) => {
  await app.open("/e2e");
  await screen.getByRole("button", /^pg-hidden /).tap();

  await expect(screen.getByText("container pretorium-e2e-pg-hidden")).toBeVisible();
  await expect(screen.getByRole("combobox", "Database")).toHaveValue("warehouse");
  await screen.getByRole("navigation", "Tables").getByRole("button", /^bins/).tap();
  await expect(screen.getByText("1–3 of 3")).toBeVisible();
  for (const label of ["Bolts", "Nuts", "Washers"]) await expect(screen.getByRole("cell", label)).toBeVisible();
  await expect(screen.getByRole("cell", "NULL")).toBeVisible(); // Washers have no aisle
});

test("finds a login in the project's .env, and asks for one it can't find", async ({ app, screen }) => {
  await app.open("/e2e");
  await screen.getByRole("button", /^pg-env /).tap();
  await expect(screen.getByText("via .env")).toBeVisible();
  await screen.getByRole("navigation", "Tables").getByRole("button", /^greetings/).tap();
  await expect(screen.getByRole("cell", "found through .env")).toBeVisible();

  await screen.getByRole("button", /^pg-locked /).tap();
  const form = screen.getByRole("form", "Log in");
  await expect(form).toContainText("The server wants a password");
  await form.getByRole("textbox", "User").fill("keeper");
  await form.getByLabel("Password").fill("typed-in");
  await form.getByRole("textbox", "Database").fill("vault");
  await form.getByRole("button", "Connect").tap();
  await expect(screen.getByText("via login")).toBeVisible();
  await screen.getByRole("navigation", "Tables").getByRole("button", /^vault/).tap();
  await expect(screen.getByRole("cell", "unlocked")).toBeVisible();
});
