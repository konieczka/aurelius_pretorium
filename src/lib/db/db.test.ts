import { expect, test } from "bun:test";
import { containerTarget, engineOfContainer, engineOfProcess, pretty, processTarget, range } from "./db";

test("databases are told by their program, image, or the port they serve inside", () => {
  expect(engineOfProcess("postgres")).toBe("postgres");
  expect(engineOfProcess("mariadbd")).toBe("mysql");
  expect(engineOfProcess("redis-server")).toBe("redis");
  expect(engineOfProcess("vite")).toBeNull();
  expect(engineOfContainer("postgres:16-alpine", [])).toBe("postgres");
  expect(engineOfContainer("docker.io/bitnami/redis:7.2", [])).toBe("redis");
  expect(engineOfContainer("mariadb:11.4@sha256:abc", [])).toBe("mysql");
  expect(engineOfContainer("mongo:7", [])).toBe("mongo");
  expect(engineOfContainer("company/billing-db", [5432])).toBe("postgres");
  expect(engineOfContainer("axllent/mailpit", [8025])).toBeNull();
});

test("a container is reached on its published port, else from inside it where that works", () => {
  const ports = [{ host: 8080, inner: 80 }, { host: 54391, inner: 5432 }];
  expect(containerTarget("postgres", "db", ports, "/p")).toEqual({ engine: "postgres", host: "127.0.0.1", port: 54391, container: "db", root: "/p" });
  expect(containerTarget("postgres", "db", [], "/p")).toEqual({ engine: "postgres", container: "db", root: "/p" });
  expect(containerTarget("mongo", "m", [], "/p")).toEqual({ engine: "mongo", container: "m", root: "/p" });
  expect(containerTarget("mysql", "my", [], "/p")).toBeNull();
  expect(processTarget("postgres", [8000, 5432], "/p").port).toBe(5432);
  expect(processTarget("redis", [6380], "/p").port).toBe(6380);
});

test("pages are counted and JSON values shown pretty", () => {
  expect(range(100, 100, 250)).toBe("101–200 of 250");
  expect(range(0, 0, 0)).toBe("No rows");
  expect(range(0, 3, null)).toBe("1–3");
  expect(pretty('{"a":1}')).toBe('{\n  "a": 1\n}');
  expect(pretty("{not json")).toBe("{not json");
  expect(pretty("plain")).toBe("plain");
});
