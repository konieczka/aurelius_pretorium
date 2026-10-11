import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";
import process from "node:process";
import { createReadStream, readdirSync, readFileSync, statSync } from "node:fs";
import { join, normalize } from "node:path";
const host = process.env.TAURI_DEV_HOST;

/** Excalidraw loads its fonts from window.EXCALIDRAW_ASSET_PATH (else a CDN, which the CSP blocks): serve them
 * at /excalidraw/fonts in dev and copy them into the build.
 * @returns {import("vite").Plugin} */
function excalidrawFonts() {
  const dir = "node_modules/@excalidraw/excalidraw/dist/prod/fonts";
  let ssr = false;
  return {
    name: "excalidraw-fonts",
    configResolved: (config) => void (ssr = !!config.build.ssr),
    configureServer(server) {
      server.middlewares.use("/excalidraw/fonts", (req, res, next) => {
        const path = join(dir, normalize(decodeURIComponent((req.url ?? "").split("?")[0])).replace(/^(\.\.[/\\])+/, ""));
        if (!path.endsWith(".woff2") || !statSync(path, { throwIfNoEntry: false })?.isFile()) return next();
        res.setHeader("Content-Type", "font/woff2");
        createReadStream(path).pipe(res);
      });
    },
    generateBundle() {
      if (ssr) return;
      for (const f of readdirSync(dir, { recursive: true, encoding: "utf8" }))
        if (f.endsWith(".woff2")) this.emitFile({ type: "asset", fileName: `excalidraw/fonts/${f}`, source: readFileSync(join(dir, f)) });
    },
  };
}

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [sveltekit(), excalidrawFonts()],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || "127.0.0.1",
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
