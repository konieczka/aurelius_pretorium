// E2E tests of the database browser (see e2e/README.md). They run the app's frontend in a browser against the real
// backend code (the e2e harness) and throwaway Docker databases; no AI model is involved.
import type { E2EConfig } from "e2e";
import { web } from "@e2e-dev/web";

export default {
  tests: ["e2e/tests/**/*.e2e.ts"],
  workers: 1, // the tests share the databases
  timeout: 120_000,
  targets: [
    {
      engine: web({ viewport: { width: 1440, height: 900 } }),
      app: {
        url: "http://127.0.0.1:1499",
        command: { executable: "bash", args: ["e2e/serve.sh"], startupTimeout: 300_000, log: ".e2e/logs/app.log", reuseExisting: true },
      },
    },
  ],
} satisfies E2EConfig;
