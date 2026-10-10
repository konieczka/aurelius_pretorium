<script module lang="ts">
  import { Channel, invoke } from "@tauri-apps/api/core";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { WebglAddon } from "@xterm/addon-webgl";
  import "@xterm/xterm/css/xterm.css";
  import { app } from "./state.svelte";
  import { cssHex, theme } from "./theme.svelte";
  import { relaunchCommand } from "./agents.svelte";
  import * as L from "./layout";

  // ANSI colours per mode, tuned for the Aurelius surfaces (the alternate palettes keep them: same lightness).
  // xterm takes hex only; background, text, cursor and selection are read from the palette's CSS (termTheme).
  const palettes = {
    light: {
      background: "#fff9ee", foreground: "#14110d", cursor: "#966a00", cursorAccent: "#fff9ee", selectionBackground: "#ba890040",
      black: "#14110d", red: "#a03f3c", green: "#27762f", yellow: "#855b00", blue: "#1666aa", magenta: "#6f4fa1", cyan: "#007780", white: "#bdb6ad",
      brightBlack: "#68625a", brightRed: "#862726", brightGreen: "#045e17", brightYellow: "#6d4400", brightBlue: "#004e90", brightMagenta: "#593888", brightCyan: "#006069", brightWhite: "#e2ddd5",
    },
    dark: {
      background: "#110d07", foreground: "#f2eee7", cursor: "#e6b642", cursorAccent: "#110d07", selectionBackground: "#e6b64240",
      black: "#322d26", red: "#ed8c84", green: "#78bf7b", yellow: "#cda448", blue: "#6db0f4", magenta: "#b699eb", cyan: "#24c1c9", white: "#d1cdc7",
      brightBlack: "#75716a", brightRed: "#ffa59d", brightGreen: "#91d993", brightYellow: "#e8be62", brightBlue: "#86caff", brightMagenta: "#d0b2ff", brightCyan: "#4cdbe3", brightWhite: "#f8f5ef",
    },
  };

  function termTheme() {
    const gold = cssHex("--gold").slice(0, 7);
    const background = cssHex("--surface").slice(0, 7);
    return {
      ...palettes[theme.dark ? "dark" : "light"],
      background,
      foreground: cssHex("--foreground").slice(0, 7),
      cursor: gold,
      cursorAccent: background,
      selectionBackground: gold + "40",
    };
  }

  // Terminals outlive their component so moving a tab or re-splitting never kills the shell.
  // The shell itself lives in the backend under the tab id and outlives this window too (reload, workspace moved).
  type Entry = { term: Terminal; fit: FitAddon; host: HTMLDivElement; ready: boolean };
  const entries = new Map<string, Entry>();

  /** Drop the tab's terminal view; `kill` also ends its shell (else it keeps running for another window to attach). */
  export function disposeTerm(tabId: string, kill = true) {
    if (kill) invoke("pty_kill", { id: tabId });
    const e = entries.get(tabId);
    if (!e) return;
    e.term.dispose();
    e.host.remove();
    entries.delete(tabId);
  }

  // Input to type into a terminal as soon as its shell is attached (not persisted with the layout).
  const pending = new Map<string, string>();
  export const queueInput = (tabId: string, data: string) => pending.set(tabId, data);

  export const focusTerm = (tabId: string) => entries.get(tabId)?.term.focus();

  function create(tabId: string, cwd: string, container: HTMLElement): Entry {
    const host = document.createElement("div");
    host.className = "term-host";
    container.appendChild(host);
    const term = new Terminal({
      fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
      fontSize: 13,
      lineHeight: 1.15,
      theme: termTheme(),
      cursorBlink: true,
      allowProposedApi: true,
      macOptionIsMeta: true,
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

    const e: Entry = { term, fit, host, ready: false };
    const onData = new Channel<ArrayBuffer>();
    onData.onmessage = (buf) => {
      if (buf.byteLength) term.write(new Uint8Array(buf));
      else term.write("\r\n\x1b[2m[process exited]\x1b[0m\r\n");
    };
    const id = tabId;
    invoke<boolean>("pty_open", { id, cwd, cols: term.cols, rows: term.rows, onData })
      .then((spawned) => {
        e.ready = true;
        // A fresh shell for a tab that had an agent running: start it again.
        const tab = L.findTab(app.root, id);
        const agent = spawned && !pending.has(id) && tab && relaunchCommand(tab.pane.tabs[tab.index]);
        const data = pending.get(id) ?? (agent ? agent + "\r" : "");
        if (data) pending.delete(id), write(data);
      })
      .catch((err) => term.write(`\x1b[31m${err}\x1b[0m\r\n`));
    // Both fail once the shell has exited (the tab shows "[process exited]"): nothing to do about that here.
    const write = (data: string) => invoke("pty_write", { id, data }).catch(() => {});
    term.onData((data) => e.ready && write(data));
    term.onResize(({ cols, rows }) => e.ready && invoke("pty_resize", { id, cols, rows }).catch(() => {}));
    // Bells and notification escapes are read by the backend (status.rs), for every shell, shown or not.
    term.onTitleChange((t) => (app.titles[tabId] = t));
    entries.set(tabId, e);
    return e;
  }
</script>

<script lang="ts">
  import { untrack } from "svelte";

  let { tabId, cwd }: { tabId: string; cwd: string } = $props();
  let el: HTMLDivElement;

  $effect(() => {
    const existing = entries.get(tabId);
    if (existing) el.appendChild(existing.host);
    const e = existing ?? create(tabId, untrack(() => cwd), el); // cwd follows `cd`: that mustn't re-run this (and grab focus)
    e.term.focus();
    const ro = new ResizeObserver(() => el.clientWidth && e.fit.fit());
    ro.observe(el);
    return () => ro.disconnect();
  });

  $effect(() => {
    const e = entries.get(tabId);
    void theme.palette;
    if (e) e.term.options.theme = termTheme();
  });
</script>

<div class="term" bind:this={el}></div>

<style>
  .term {
    position: absolute;
    inset: 0;
    padding: 6px 0 0 10px;
    background: var(--surface);
  }
  .term :global(.term-host) {
    width: 100%;
    height: 100%;
  }
</style>
