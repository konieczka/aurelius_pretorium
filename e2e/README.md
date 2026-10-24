# E2E tests: the database browser

Scenarios for the Servers panel's database tabs, one file per phase:

| File | Covers |
| --- | --- |
| `tests/1-postgres.e2e.ts` | Postgres over a published port and with `psql` inside a container; paging, sorting, filtering; the query editor; logins from container settings, `.env`, or the login form |
| `tests/2-mysql-redis.e2e.ts` | MariaDB in the same SQL view; Redis keys by pattern, values by type, the console |
| `tests/3-writes.e2e.ts` | Write mode: the confirmation, queries that change data, cell edits that show their `UPDATE` first, why a cell can't be edited |
| `tests/4-mongo-sqlite-agent.e2e.ts` | Mongo through `mongosh` in its container; the project's SQLite files; asking the coding agent |
| `tests/5-terminal-links.e2e.ts` | File paths in a terminal's output as links: a file opens in a tab beside it, at its line if one is given; paths that aren't files stay text |
| `tests/6-terminal-theme.e2e.ts` | Theme reports to a terminal's program (mode 2031, which Claude Code follows): asked for, on each light/dark change while it's on, and whether they're on (DECRQM); a terminal in a hidden tab answers OSC 11 with the current theme's background |

They use the [e2e](https://e2e.tester.army/docs) framework with exact locators only: no AI model, no sign-in.

## Running

```sh
bun run test:e2e          # all of them
bun run test:e2e e2e/tests/3-writes.e2e.ts
```

Needs Docker running and Node 24.8 or newer for the runner (`nvm install 24`; `nvm use 24` in the shell that runs it).
The script turns off the runner's anonymous telemetry.

## How it works

Playwright can't drive Tauri's webview on macOS, so the tests run the app's frontend in Chromium against the real
backend code:

- `serve.sh` (started by the runner, see `e2e.config.ts`) brings up the databases, the **harness** and Vite on :1499.
- `services.sh up` starts throwaway containers (Postgres ×4, MariaDB, Redis, Mongo) with seed data from `seed/`,
  labelled as the Compose project of `fixtures/shop`, the workspace the tests open. `services.sh down` removes them;
  `serve.sh` does when it stops.
- The harness (`src-tauri/src/harness.rs`, built with `--features e2e`) answers the app's `infra_*` and `db_*`
  commands over HTTP on :7811, and `existing_files` and `open_file` for the terminal's file links (files in `fixtures/` only).
- `/e2e` (`src/routes/e2e`, dev builds only) mounts the Servers panel and the panes, with Tauri's IPC mocked to call the
  harness. Commands it doesn't serve (terminals) are recorded in `window.__e2eCalls` for the tests to read. There's
  no shell behind a terminal: a test writes its output through `window.__e2eTerms`, which also has its size.

The tests only touch the `pretorium-e2e-*` containers and `fixtures/shop`. With `serve.sh` already running, the runner
reuses it; the write tests put their rows back first (`tests/reset.ts`).
