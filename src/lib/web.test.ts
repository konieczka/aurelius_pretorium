import { expect, test } from "bun:test";
import { hostOf, toUrl } from "./web";

test("addresses keep or gain a scheme; the rest is searched", () => {
  expect(toUrl("  ")).toBeNull();
  expect(toUrl("https://github.com/a/b")).toBe("https://github.com/a/b");
  expect(toUrl("about:blank")).toBe("about:blank");
  expect(toUrl("github.com")).toBe("https://github.com");
  expect(toUrl("docs.rs/tauri/latest")).toBe("https://docs.rs/tauri/latest");
  expect(toUrl("localhost:5173")).toBe("http://localhost:5173");
  expect(toUrl("localhost")).toBe("http://localhost");
  expect(toUrl("127.0.0.1:8080/api")).toBe("http://127.0.0.1:8080/api");
  expect(toUrl("192.168.1.20")).toBe("http://192.168.1.20");
  expect(toUrl("tauri child webview")).toBe("https://www.google.com/search?q=tauri%20child%20webview");
  expect(toUrl("svelte")).toBe("https://www.google.com/search?q=svelte");
  expect(toUrl("node.js vs deno")).toBe("https://www.google.com/search?q=node.js%20vs%20deno");
});

test("a page is named by its host until it has a title", () => {
  expect(hostOf("https://www.github.com/x")).toBe("github.com");
  expect(hostOf("http://localhost:5173/")).toBe("localhost:5173");
  expect(hostOf("nonsense")).toBe("nonsense");
});
