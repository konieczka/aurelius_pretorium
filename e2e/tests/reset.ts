// Puts back the rows the write tests change, through the e2e harness: the suite can run again against databases that
// are still up (e2e/serve.sh reused) as well as fresh ones.
const harness = process.env.E2E_HARNESS ?? "http://127.0.0.1:7811";
const root = new URL("../fixtures/shop", import.meta.url).pathname;

async function call<T>(cmd: string, args: unknown): Promise<T> {
  const r = await fetch(`${harness}/invoke/${cmd}`, { method: "POST", body: JSON.stringify(args) });
  const body = await r.json();
  if (!r.ok) throw new Error(`${cmd}: ${body}`);
  return body as T;
}

async function run(target: Record<string, unknown>, statements: string[], redis = false) {
  const { id } = await call<{ id: string }>("db_connect", { target: { root, ...target } });
  for (const s of statements) {
    if (redis) await call("db_command", { id, db: 0, line: s, write: true });
    else await call("db_query", { id, sql: s, write: true });
  }
  await call("db_disconnect", { id });
}

export async function resetWrites() {
  await run({ engine: "postgres", host: "127.0.0.1", port: 54391, container: "pretorium-e2e-pg" }, [
    "DELETE FROM settings",
    "INSERT INTO settings VALUES ('theme', 'dark'), ('currency', 'EUR'), ('greeting', 'hello'), ('locale', 'en'), ('notes', 'none')",
  ]);
  await run({ engine: "mysql", host: "127.0.0.1", port: 33091, container: "pretorium-e2e-mariadb" }, ["UPDATE shop.products SET stock = 0 WHERE id = 4"]);
  await run({ engine: "redis", host: "127.0.0.1", port: 63791, container: "pretorium-e2e-redis" }, ["DEL greeting"], true);
}

export async function resetNotes() {
  await run({ engine: "sqlite", file: `${root}/data/notes.sqlite` }, ["UPDATE notes SET pinned = (title = 'Shopping')"]);
}
