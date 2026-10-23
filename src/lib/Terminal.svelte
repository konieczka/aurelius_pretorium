<script module lang="ts">
  import { Channel, invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { WebglAddon } from "@xterm/addon-webgl";
  import { WebLinksAddon } from "@xterm/addon-web-links";
  import "@xterm/xterm/css/xterm.css";
  import { app, copyText, droppedPaths, markDragTarget, openBrowserBeside, openFileBeside, takeDragTarget } from "./state.svelte";
  import { toast } from "./Toast.svelte";
  import { cssHex, theme } from "./theme.svelte";
  import { relaunchCommand } from "./agents.svelte";
  import { revealLine } from "./Editor.svelte";
  import { fileLinkProvider } from "./termLinks";
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
  type Entry = { term: Terminal; fit: FitAddon; host: HTMLDivElement; ready: boolean; themeReports: boolean; send: (data: string) => void };
  const entries = new Map<string, Entry>();

  // Mode 2031 (contour's dark/light reports): agents like Claude Code pick their colours from it, and follow it when the
  // app's theme changes. xterm.js has no support for it, so a running agent kept the colours of the theme it started in.
  const themeReport = () => `\x1b[?997;${theme.dark ? 1 : 2}n`;
  $effect.root(() => {
    $effect(() => {
      const report = themeReport();
      for (const e of entries.values()) if (e.themeReports) e.send(report);
    });
  });

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

  // Shell-quote a path the way Terminal.app does a dropped file: backslashes before the special characters.
  const shellEscape = (p: string) => p.replace(/[\s!"#$&'()*;<>?[\\\]`{|}^~]/g, "\\$&");

  /** Paths of what's dropped on a terminal: files from Finder etc., or one from the file tree. */
  const droppedOn = (dt: DataTransfer) => {
    const tree = dt.getData("text/x-path");
    return tree ? Promise.resolve([tree]) : droppedPaths(dt);
  };

  /** Type the dropped files' paths at the prompt, as a paste (agents like Claude Code pick up image paths from it). */
  async function dropInto(tabId: string, paths: string[] | Promise<string[]>) {
    const e = entries.get(tabId);
    if (!e) return;
    try {
      const ps = await paths;
      if (ps.length) e.term.paste(ps.map(shellEscape).join(" ") + " ");
    } catch (err) {
      e.term.write(`\r\n\x1b[31mCouldn't drop the file: ${err}\x1b[0m\r\n`);
    }
    e.term.focus();
  }

  // Screenshot thumbnails and other file promises get no `drop` here: the backend catches them (catch_promise_drops)
  // and sends their paths. They go to the terminal the drag was last over, if that was just now.
  listen<string[]>("promise-drop", ({ payload }) => {
    const tabId = takeDragTarget("term");
    if (tabId) dropInto(tabId, payload);
  });
  const isFileDrag = (ev: DragEvent) => !!ev.dataTransfer?.types.some((t) => t === "Files" || t === "text/x-path");

  // Click copies the link; Cmd+click opens it in a browser tab in a new pane to the right. Dragging across one still selects.
  // A link to a file (file://, or a path in the text: see termLinks.ts) opens it in a tab on click.
  // A file:// link to something else (a folder) is a link like the rest.
  function linkClicked(tabId: string, ev: MouseEvent, uri: string) {
    if (/^file:\/\//i.test(uri)) fileOf(tabId, uri).then((file) => {
        if (file) openFileBeside(file, tabId);
        else if (!ev.metaKey) linkCopied(ev, uri);
      });
    else if (!ev.metaKey) linkCopied(ev, uri);
    else if (/^https?:\/\//i.test(uri)) openBrowserBeside(uri, tabId); // OSC 8 links can be anything: web pages only
  }
  const linkCopied = (ev: MouseEvent, uri: string) => copyText(uri).then((ok) => ok && toast("Link copied", ev.clientX, ev.clientY));

  /** The files `paths` in tab `tabId`'s output name: relative ones are in its shell's folder, or else the workspace's. */
  function filesIn(tabId: string, paths: string[]) {
    const f = L.findTab(app.root, tabId);
    const t = f?.pane.tabs[f.index];
    const bases = [t?.kind === "term" ? t.cwd : "", app.workspace].filter(Boolean);
    return invoke<(string | null)[]>("existing_files", { bases, paths });
  }

  /** A file:// link's file, if it is one here (not a folder, nor missing). Its host is ignored: `ls --hyperlink` names
   * this Mac, which the page doesn't know. */
  async function fileOf(tabId: string, uri: string) {
    let path: string;
    try {
      path = decodeURIComponent(new URL(uri).pathname);
    } catch {
      return null; // not a URL, or a broken %-escape
    }
    return (await filesIn(tabId, [path]).catch(() => [null]))[0];
  }

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
      // OSC 8 hyperlinks (ls --hyperlink, gh, agents); plain-text URLs go through WebLinksAddon below.
      linkHandler: { activate: (ev, uri) => linkClicked(tabId, ev, uri) },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new WebLinksAddon((ev, uri) => linkClicked(tabId, ev, uri)));
    term.registerLinkProvider(
      fileLinkProvider(term, (paths) => filesIn(tabId, paths), (_ev, file, ref) => {
        const id = openFileBeside(file, tabId, !!ref.line);
        if (ref.line) revealLine(id, ref.line, ref.col);
      }),
    );
    term.open(host);
    try {
      const gl = new WebglAddon();
      gl.onContextLoss(() => gl.dispose());
      term.loadAddon(gl);
    } catch {} // falls back to DOM renderer
    // Leave Cmd+key to the app shortcuts / native menu.
    term.attachCustomKeyEventHandler((e) => !e.metaKey);
    fit.fit();

    const e: Entry = { term, fit, host, ready: false, themeReports: false, send: () => {} };
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
    e.send = (data) => e.ready && write(data);
    term.onData(e.send);
    // Handlers return false so xterm still handles the other modes in the same sequence.
    const has2031 = (params: (number | number[])[]) => params.includes(2031);
    term.parser.registerCsiHandler({ prefix: "?", final: "h" }, (params) => (has2031(params) && (e.themeReports = true), false));
    term.parser.registerCsiHandler({ prefix: "?", final: "l" }, (params) => (has2031(params) && (e.themeReports = false), false));
    term.parser.registerCsiHandler({ prefix: "?", final: "n" }, (params) => params[0] === 996 && (e.send(themeReport()), true));
    term.parser.registerEscHandler({ final: "c" }, () => ((e.themeReports = false), false)); // `reset`, after an agent that crashed with it on
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
    const animating = theme.transitioning;
    if (!e) return;
    let frame = 0;
    const update = () => {
      e.term.options.theme = termTheme();
      if (animating) frame = requestAnimationFrame(update);
    };
    update();
    return () => cancelAnimationFrame(frame);
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="term"
  bind:this={el}
  ondragover={(ev) => {
    if (!isFileDrag(ev)) return;
    ev.preventDefault();
    ev.dataTransfer!.dropEffect = "copy";
    markDragTarget("term", tabId);
  }}
  ondrop={(ev) => {
    if (!isFileDrag(ev)) return;
    ev.preventDefault();
    dropInto(tabId, droppedOn(ev.dataTransfer!));
  }}
></div>

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
