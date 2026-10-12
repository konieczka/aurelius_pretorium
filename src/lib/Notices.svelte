<script module lang="ts">
  // Agents that asked something or finished out of sight (another workspace, minimized, a background tab): a note in
  // the corner that jumps to them. The backend cues it, for tabs no focused window has on screen (see `pty_status`);
  // every window hears it, and the one you're in shows it (else the one you were in last, for when you come back).
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import * as L from "./layout";
  import { agentProvider, shells } from "./agents.svelte";
  import { cleanTitle } from "./fleet";
  import { app } from "./state.svelte";
  import { tabsIn, ws } from "./workspaces.svelte";

  type Cue = "ask" | "done";
  type Notice = { key: number; tab: string; ws: string; cue: Cue; agent: string; label: string; left: number };

  const LIFE = 3000; // ms on screen, counted only while you're in the window and not pointing at it
  const MAX = 4;
  const notices = $state<Notice[]>([]);
  let keys = 0;

  const label = getCurrentWindow().label;
  const note = (k: string, v: string) => {
    try {
      localStorage.setItem(k, v);
    } catch {}
  };
  const read = (k: string) => {
    try {
      return localStorage.getItem(k);
    } catch {
      return null;
    }
  };
  // Which window you're in: "focusedWindow" is "" while you're in another app; "lastWindow" stays.
  window.addEventListener("focus", () => (note("focusedWindow", label), note("lastWindow", label)));
  window.addEventListener("blur", () => read("focusedWindow") === label && note("focusedWindow", ""));
  if (document.hasFocus()) note("focusedWindow", label), note("lastWindow", label);
  const mine = () => document.hasFocus() || (!read("focusedWindow") && read("lastWindow") === label);

  /** On screen here: the active tab of one of this window's panes. */
  const onScreen = (id: string) => L.panes(app.root).some((p) => p.tabs[p.active]?.id === id);

  listen<{ id: string; cue: Cue }>("agent-cue", ({ payload: { id, cue } }) => {
    if (!mine() || onScreen(id)) return;
    const home = ws.all.find((m) => tabsIn(m.id).some((t) => t.id === id));
    const t = home && tabsIn(home.id).find((t) => t.id === id);
    if (!home || t?.kind !== "term") return; // no workspace has its tab: nowhere to jump to
    const s = shells.all[id];
    const agent = s?.agent ?? t.agent ?? "";
    const n: Notice = { key: ++keys, tab: id, ws: home.id, cue, agent, label: t.name || cleanTitle(s?.title ?? "", s?.cwd || t.cwd, agent), left: LIFE };
    const i = notices.findIndex((x) => x.tab === id);
    if (i >= 0) notices.splice(i, 1);
    notices.push(n);
    if (notices.length > MAX) notices.splice(0, notices.length - MAX);
  }).catch(() => {});
</script>

<script lang="ts">
  import { Check, MessageCircleQuestion, X } from "@lucide/svelte";
  import ProviderIcon from "./ProviderIcon.svelte";
  import { providerName } from "./usageStats";
  import { nameOf, showTab } from "./workspaces.svelte";

  let hovered = $state(false);
  const TICK = 250;
  $effect(() => {
    const t = setInterval(() => {
      for (let i = notices.length - 1; i >= 0; i--) {
        const n = notices[i];
        if (onScreen(n.tab)) notices.splice(i, 1); // you went there yourself
        else if (document.hasFocus() && !hovered && (n.left -= TICK) <= 0) notices.splice(i, 1);
      }
    }, TICK);
    return () => clearInterval(t);
  });

  const dismiss = (key: number) => {
    const i = notices.findIndex((n) => n.key === key);
    if (i >= 0) notices.splice(i, 1);
  };
  function go(n: Notice) {
    dismiss(n.key);
    showTab(n.ws, n.tab);
  }
  const where = (n: Notice) =>
    n.ws !== ws.active ? nameOf(n.ws) : app.minimized.some((m) => m.tab.id === n.tab) ? "Minimized" : "Behind another tab";
  const who = (n: Notice) => (agentProvider[n.agent] ? providerName[agentProvider[n.agent]] : n.agent || "Terminal");
</script>

{#if notices.length}
  <!-- Newest at the bottom, nearest the corner -->
  <div class="notices" role="status" onpointerenter={() => (hovered = true)} onpointerleave={() => (hovered = false)}>
    {#each notices as n (n.key)}
      <div class="notice glass-float" class:ask={n.cue === "ask"}>
        <button class="body" onclick={() => go(n)} title="Go to {n.label || who(n)}">
          <span class="agent">
            {#if agentProvider[n.agent]}<ProviderIcon provider={agentProvider[n.agent]} size={16} />{/if}
            <span class="cue">
              {#if n.cue === "ask"}<MessageCircleQuestion size={10} strokeWidth={2.25} />{:else}<Check size={10} strokeWidth={2.75} />{/if}
            </span>
          </span>
          <span class="text">
            <span class="title"><b>{who(n)}</b> {n.cue === "ask" ? "needs your input" : "finished"}</span>
            <span class="sub">{n.label ? `${n.label} · ` : ""}{where(n)}</span>
          </span>
        </button>
        <button class="icon-btn close" onclick={() => dismiss(n.key)} aria-label="Dismiss"><X size={12} strokeWidth={1.75} /></button>
      </div>
    {/each}
  </div>
{/if}

<style>
  /* Above the status bar, in the corner. (A browser tab there is a native view drawn over the page: it covers these.) */
  .notices {
    position: fixed;
    right: 12px;
    bottom: 40px;
    z-index: 105;
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 6px;
  }
  .notice {
    display: flex;
    align-items: center;
    gap: 2px;
    width: 300px;
    padding: 4px;
    border-radius: 14px;
    animation: rise var(--dur-base) var(--ease-out-quint);
  }
  .body {
    display: flex;
    flex: 1;
    min-width: 0;
    align-items: center;
    gap: 10px;
    padding: 6px 4px 6px 8px;
    border: 0;
    border-radius: var(--r-control);
    background: none;
    color: var(--foreground);
    text-align: left;
    cursor: pointer;
    transition: background-color var(--dur-quick) var(--ease-standard);
  }
  .body:hover {
    background: var(--hover);
  }
  .agent {
    position: relative;
    display: flex;
    flex: none;
    color: var(--muted-foreground);
  }
  /* Badge on the agent's mark: a check when done, a question in gold when it waits on you */
  .cue {
    position: absolute;
    right: -6px;
    bottom: -5px;
    display: flex;
    padding: 1.5px;
    border-radius: 9999px;
    background: var(--muted-foreground);
    color: var(--surface);
    box-shadow: 0 0 0 1.5px var(--surface);
  }
  .ask .cue {
    background: var(--gold);
  }
  .text {
    display: flex;
    flex-direction: column;
    min-width: 0;
    gap: 1px;
  }
  .title,
  .sub {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .title {
    font-size: 12.5px;
  }
  .title b {
    font-weight: 600;
  }
  .sub {
    font-size: 11.5px;
    color: var(--muted-foreground);
  }
  .close {
    flex: none;
    align-self: flex-start;
    width: 22px;
    height: 22px;
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(6px) scale(0.98);
    }
  }
</style>
