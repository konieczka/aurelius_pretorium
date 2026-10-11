<script module lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { allTabs, app, openBrowser } from "./state.svelte";
  import * as L from "./layout";

  // The page is a native webview over the tab's body (the backend's web.rs), not part of this page: it's moved to
  // wherever the body is on screen, and hidden whenever something of ours has to show on top of it (a dragged tab's
  // drop zones, the quick pick) or the tab isn't shown. Like a terminal's shell it outlives the component, so
  // switching tabs or moving it to another pane keeps the page as it was.

  type WebTab = Extract<L.Tab, { kind: "web" }>;
  const webTab = (id: string) => allTabs().find((t): t is WebTab => t.id === id && t.kind === "web");

  const loading = $state<Record<string, boolean>>({}); // tab id -> page loading
  /** Tab id -> its page's icon (a data: URL) and the origin it's for. Not saved: pages send it again as they load. */
  export const icons = $state<Record<string, { src: string; origin: string }>>({});
  const originOf = (url: string) => URL.parse(url)?.origin ?? "";
  const inputs = new Map<string, HTMLInputElement>(); // tab id -> its address bar, while shown

  // Calls that place, hide or close a tab's view go one at a time, latest wins: dragging a split sends one per frame,
  // and a show landing after the hide that followed it would leave a page over another tab.
  const queues = new Map<string, { busy: boolean; next: (() => Promise<unknown>) | null }>();
  async function send(id: string, call: () => Promise<unknown>) {
    let q = queues.get(id);
    if (!q) queues.set(id, (q = { busy: false, next: null }));
    q.next = call;
    if (q.busy) return;
    q.busy = true;
    while (q.next) {
      const c = q.next;
      q.next = null;
      await c().catch(() => {});
    }
    q.busy = false;
  }

  /** The tab is closed (`kill`, the page goes) or its workspace left this window (the page waits, hidden). */
  export function disposeBrowser(id: string, kill = true) {
    send(id, () => invoke(kill ? "web_close" : "web_hide", { id }));
    delete loading[id];
    delete icons[id];
  }

  export function focusAddress(id: string) {
    const el = inputs.get(id);
    el?.focus();
    el?.select();
  }

  /** Back (-1) or forward (1). */
  export const browserGo = (id: string, delta: number) => invoke("web_go", { id, delta }).catch(() => {});

  // Pages report where they are and what they're called, to every window: the one with the tab takes it.
  listen<{ id: string; url: string; title?: string; loading?: boolean }>("web-state", ({ payload: p }) => {
    const t = webTab(p.id);
    if (!t) return;
    if (p.url && p.url !== "about:blank") t.url = p.url;
    if (p.title !== undefined) t.title = p.title || undefined;
    if (p.loading !== undefined) loading[p.id] = p.loading;
    // Off to another site: its icon isn't this one (it sends its own once it has loaded)
    if (p.loading && icons[p.id] && originOf(p.url) !== icons[p.id].origin) delete icons[p.id];
  });
  listen<{ id: string; url: string; icon: string | null }>("web-icon", ({ payload: p }) => {
    const t = webTab(p.id);
    if (!t || originOf(p.url) !== originOf(t.url)) return; // late: the tab has moved on
    if (p.icon) icons[p.id] = { src: p.icon, origin: originOf(p.url) };
    else delete icons[p.id];
  });
  // A link that opens a new window (target=_blank, window.open): a tab right after its page's
  listen<[string, string]>("web-open", ({ payload: [from, url] }) => webTab(from) && openBrowser(url, from));

  // This page just (re)loaded; the backend's views didn't. Hide them until their tabs mount again.
  invoke("web_hide_all").catch(() => {});

  // Clicking into a page takes the keyboard from this one: the page's pane becomes the focused one.
  window.addEventListener("blur", () =>
    invoke<string | null>("web_focused").then((id) => {
      const f = id && L.findTab(app.root, id);
      if (f) app.focused = f.pane.id;
    }),
  );
</script>

<script lang="ts">
  import { ArrowLeft, ArrowRight, ExternalLink, Globe, RotateCw, X } from "@lucide/svelte";
  import { picking } from "./QuickPick.svelte";
  import { zoomLevel } from "./zoom";
  import { toUrl } from "./web";
  import { untrack } from "svelte";

  let { tab }: { tab: WebTab } = $props();
  const id = untrack(() => tab.id); // the pane remounts this for another tab

  let view: HTMLDivElement;
  let input: HTMLInputElement;
  let editing = $state(false);
  let address = $state("");
  $effect(() => {
    if (!editing) address = tab.url;
  });
  let error = $state("");
  let created = false; // the view exists (the first show made it, or it outlived a reload of this page)
  let focusPage = false; // give the page the keyboard once it's shown (an address was just entered)

  $effect(() => {
    inputs.set(id, input);
    return () => inputs.delete(id);
  });

  const radius = () => parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--r-pane")) || 16;

  // Follow the body every frame: splits, the sidebar and the window all move it, mostly without resizing it.
  $effect(() => {
    let last = "";
    let frame = requestAnimationFrame(tick);
    function tick() {
      frame = requestAnimationFrame(tick);
      const r = view.getBoundingClientRect();
      const shown = !!tab.url && !error && r.width > 0 && r.height > 0 && !app.dragging && !picking();
      const z = zoomLevel(); // CSS px × zoom = the window's logical px
      // Until the view exists a new address has to reach it too
      const key = shown ? [r.left, r.top, r.width, r.height, z, created ? "" : tab.url].join() : "";
      if (key === last) return;
      last = key;
      if (!shown) return void send(id, () => invoke("web_hide", { id }));
      const args = { id, url: tab.url, x: r.left * z, y: r.top * z, width: r.width * z, height: r.height * z, radius: radius() * z };
      send(id, () =>
        invoke("web_show", args).then(
          () => {
            created = true;
            if (focusPage) (focusPage = false), invoke("web_focus", { id });
          },
          (e) => (error = String(e)),
        ),
      );
    }
    return () => {
      cancelAnimationFrame(frame);
      send(id, () => invoke("web_hide", { id }));
    };
  });

  function go() {
    const url = toUrl(address);
    if (!url) return;
    editing = false;
    error = "";
    input.blur();
    if (created) {
      tab.url = url;
      invoke("web_navigate", { id, url }).then(() => invoke("web_focus", { id }), (e) => (error = String(e)));
    } else {
      focusPage = true;
      tab.url = url; // the next frame shows the view, which opens it
    }
  }

  function key(e: KeyboardEvent) {
    if (e.key === "Enter") go();
    else if (e.key === "Escape") {
      editing = false;
      address = tab.url;
      input.blur();
    }
  }

  // A load that fails may never report finishing: Stop also clears it
  const reload = () => (loading[id] ? ((loading[id] = false), invoke("web_stop", { id })) : invoke("web_reload", { id })).catch(() => {});
</script>

<div class="browser">
  <div class="toolbar">
    <button class="icon-btn" onclick={() => browserGo(id, -1)} disabled={!tab.url} title="Back (⌘[)"><ArrowLeft size={14} strokeWidth={1.5} /></button>
    <button class="icon-btn" onclick={() => browserGo(id, 1)} disabled={!tab.url} title="Forward (⌘])"><ArrowRight size={14} strokeWidth={1.5} /></button>
    <button class="icon-btn" onclick={reload} disabled={!tab.url} title={loading[id] ? "Stop" : "Reload (⌘R)"}>
      {#if loading[id]}<X size={14} strokeWidth={1.5} />{:else}<RotateCw size={13} strokeWidth={1.5} />{/if}
    </button>
    <input
      class="address"
      class:loading={loading[id]}
      bind:this={input}
      bind:value={address}
      placeholder="Search or enter an address"
      spellcheck="false"
      autocomplete="off"
      oninput={() => (editing = true)}
      onfocus={() => input.select()}
      onblur={() => (editing = false)}
      onkeydown={key}
    />
    <button class="icon-btn" onclick={() => invoke("open_url", { url: tab.url }).catch(() => {})} disabled={!/^https?:/.test(tab.url)} title="Open in your browser"><ExternalLink size={13} strokeWidth={1.5} /></button>
  </div>
  <div class="view" bind:this={view}>
    {#if error}
      <p class="note">{error}</p>
    {:else if !tab.url}
      <p class="note"><Globe size={28} strokeWidth={1.25} />Search or enter an address <kbd>⌘L</kbd></p>
    {/if}
  </div>
</div>

<style>
  .browser {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    background: var(--surface);
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 2px;
    height: 34px;
    flex: none;
    padding: 0 8px;
    border-bottom: 1px solid color-mix(in oklch, var(--border) 60%, transparent);
  }
  .toolbar > .icon-btn {
    width: 26px;
    height: 26px;
  }
  .address {
    flex: 1;
    min-width: 0;
    height: 24px;
    margin: 0 4px;
    padding: 0 10px;
    border: 1px solid var(--border);
    border-radius: var(--r-control);
    background: var(--background);
    color: var(--foreground);
    font: inherit;
    font-size: 12px;
    outline: none;
    transition: border-color var(--dur-quick) var(--ease-standard), box-shadow var(--dur-quick) var(--ease-standard);
  }
  .address:focus {
    border-color: var(--gold-ring);
    box-shadow: 0 0 0 3px var(--gold-soft);
  }
  .address.loading:not(:focus) {
    color: var(--muted-foreground);
  }
  .view {
    position: relative;
    flex: 1;
    min-height: 0;
  }
  .note {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
    height: 100%;
    margin: 0;
    padding: 0 24px;
    color: var(--muted-foreground);
    font-size: 13px;
    text-align: center;
  }
  kbd {
    padding: 1px 6px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--muted);
    font-family: inherit;
    font-size: 11px;
  }
</style>
