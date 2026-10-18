// Database tabs: what the backend (src-tauri/src/db) answers, and which servers the Servers panel can browse.
import type { DbTarget } from "../layout";

export type Engine = DbTarget["engine"];
export type Connected = { id: string; engine: Engine; user: string; database: string; databases: string[]; via: string; route: string };
export type Col = { name: string; kind: string };
export type Grid = { columns: Col[]; rows: (string | null)[][]; key: string[]; total: number | null; affected: number | null; truncated: boolean; ms: number };
export type Table = { schema: string; name: string; kind: string; rows: number | null };
export type Completion = { schema: string; table: string; columns: string[] };

export const engineName: Record<Engine, string> = { postgres: "Postgres", mysql: "MySQL", redis: "Redis", mongo: "MongoDB", sqlite: "SQLite" };
const PORTS: Record<Exclude<Engine, "sqlite">, number> = { postgres: 5432, mysql: 3306, redis: 6379, mongo: 27017 };

/** The engine a server process runs, by its program's name. */
export function engineOfProcess(name: string): Engine | null {
  if (name === "postgres" || name === "postmaster") return "postgres";
  if (name === "mysqld" || name === "mariadbd") return "mysql";
  if (name === "redis-server" || name === "valkey-server") return "redis";
  if (name === "mongod") return "mongo";
  return null;
}

/** The engine a container runs: by its image (postgres:16, bitnami/redis, mariadb), else the port it serves inside. */
export function engineOfContainer(image: string, inner: number[]): Engine | null {
  const name = image.split("@")[0].split(":")[0].split("/").pop() ?? "";
  if (/^(postgres|postgis|timescaledb|pgvector|supabase-postgres)/.test(name)) return "postgres";
  if (/^(mysql|mariadb|percona)/.test(name)) return "mysql";
  if (/^(redis|valkey|keydb)/.test(name)) return "redis";
  if (/^mongo/.test(name)) return "mongo";
  const byPort = (Object.entries(PORTS) as [Engine, number][]).find(([, p]) => inner.includes(p));
  return byPort ? byPort[0] : null;
}

/** How to reach a container's database: the port it publishes for it, else from inside (psql or mongosh in it).
 * Null when neither works: MySQL and Redis need a published port. */
export function containerTarget(engine: Engine, id: string, ports: { host: number; inner: number }[], root: string): DbTarget | null {
  const published = engine === "sqlite" ? undefined : (ports.find((p) => p.inner === PORTS[engine]) ?? ports[0]);
  if (published) return { engine, host: "127.0.0.1", port: published.host, container: id, root };
  return engine === "postgres" || engine === "mongo" ? { engine, container: id, root } : null;
}

/** A server process's database: on its engine's usual port when it listens on several. */
export function processTarget(engine: Engine, ports: number[], root: string): DbTarget {
  const port = engine === "sqlite" ? ports[0] : (ports.find((p) => p === PORTS[engine]) ?? ports[0]);
  return { engine, host: "127.0.0.1", port, root };
}

/** Where a tab's database is, in a few words. */
export const describe = (t: DbTarget) => (t.file ? t.file : t.port ? `localhost:${t.port}` : `container ${t.container}`);

/** Values that are JSON, shown pretty. */
export function pretty(v: string) {
  const s = v.trim();
  if (!(s.startsWith("{") || s.startsWith("["))) return v;
  try {
    return JSON.stringify(JSON.parse(s), null, 2);
  } catch {
    return v;
  }
}

/** "1–100 of 250" */
export function range(offset: number, shown: number, total: number | null) {
  if (!shown) return total ? `0 of ${total}` : "No rows";
  const span = `${offset + 1}–${offset + shown}`;
  return total === null ? span : `${span} of ${total}`;
}
