<script module lang="ts">
  import { Channel, invoke } from "@tauri-apps/api/core";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { WebglAddon } from "@xterm/addon-webgl";
  import "@xterm/xterm/css/xterm.css";
  import { app } from "./state.svelte";

  // Terminals outlive their component so moving a tab or re-splitting never kills the shell.
  type Entry = { term: Terminal; fit: FitAddon; host: HTMLDivElement; pty?: number };
  const entries = new Map<string, Entry>();

  export function disposeTerm(tabId: string) {
    const e = entries.get(tabId);
    if (!e) return;
    if (e.pty !== undefined) invoke("pty_kill", { id: e.pty });
    e.term.dispose();
    e.host.remove();
    entries.delete(tabId);
  }

  export const focusTerm = (tabId: string) => entries.get(tabId)?.term.focus();

  function create(tabId: string, cwd: string, container: HTMLElement): Entry {
    const host = document.createElement("div");
    host.className = "term-host";
    container.appendChild(host);
    const term = new Terminal({
      fontFamily: "Menlo, Monaco, monospace",
      fontSize: 13,
      cursorBlink: true,
      allowProposedApi: true,
      macOptionIsMeta: true,
      theme: { background: "#282c34", foreground: "#d7dae0" },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host);
    try {
      const gl = new WebglAddon();
      gl.onContextLoss(() => gl.dispose());
      term.loadAddon(gl);
    } catch {} // falls back to DOM renderer
    // Leave Cmd+key to the app shortcuts / native menu.
    term.attachCustomKeyEventHandler((e) => !e.metaKey);
    fit.fit();

    const e: Entry = { term, fit, host };
    const onData = new Channel<ArrayBuffer>();
    onData.onmessage = (buf) => {
      if (buf.byteLength) term.write(new Uint8Array(buf));
      else term.write("\r\n\x1b[2m[process exited]\x1b[0m\r\n");
    };
    invoke<number>("pty_spawn", { cwd, cols: term.cols, rows: term.rows, onData })
      .then((id) => (e.pty = id))
      .catch((err) => term.write(`\x1b[31m${err}\x1b[0m\r\n`));
    term.onData((data) => e.pty !== undefined && invoke("pty_write", { id: e.pty, data }));
    term.onResize(({ cols, rows }) => e.pty !== undefined && invoke("pty_resize", { id: e.pty, cols, rows }));
    term.onTitleChange((t) => (app.titles[tabId] = t));
    entries.set(tabId, e);
    return e;
  }
</script>

<script lang="ts">
  let { tabId, cwd }: { tabId: string; cwd: string } = $props();
  let el: HTMLDivElement;

  $effect(() => {
    const existing = entries.get(tabId);
    if (existing) el.appendChild(existing.host);
    const e = existing ?? create(tabId, cwd, el);
    e.term.focus();
    const ro = new ResizeObserver(() => el.clientWidth && e.fit.fit());
    ro.observe(el);
    return () => ro.disconnect();
  });
</script>

<div class="term" bind:this={el}></div>

<style>
  .term {
    position: absolute;
    inset: 0;
    padding: 4px 0 0 8px;
    background: #282c34;
  }
  .term :global(.term-host) {
    width: 100%;
    height: 100%;
  }
</style>
