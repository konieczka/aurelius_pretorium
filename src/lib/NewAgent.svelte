<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { Plus } from "@lucide/svelte";
  import ProviderIcon from "./ProviderIcon.svelte";
  import { PROVIDERS, providerName, type Provider } from "./usageStats";
  import { app, runInTerminal } from "./state.svelte";

  // The Fleet sidebar's New Agent button: a menu of every agent, those not installed greyed out; picking one starts it
  // in a new terminal in the sidebar's folder, in the rightmost pane.
  type Launch = { agent: Provider; command: string | null };

  let open = $state(false);
  let sel = $state(-1);
  // Looked up again on every opening (agents get installed meanwhile); the last answer shows until then.
  let launchers = $state<Launch[] | null>(null);
  let root: HTMLElement;

  const items = $derived(launchers ?? PROVIDERS.map((agent) => ({ agent, command: null })));
  const usable = $derived(items.flatMap((l, i) => (l.command ? [i] : [])));

  async function toggle() {
    open = !open;
    if (!open) return;
    sel = -1;
    try {
      launchers = await invoke<Launch[]>("agent_launchers");
    } catch {
      launchers ??= [];
    }
  }

  function start(l: Launch) {
    if (!l.command) return;
    open = false;
    runInTerminal(app.workspace, l.command, "rightmost");
  }

  function onKey(e: KeyboardEvent) {
    if (!open) return;
    if (e.key === "Escape") (e.preventDefault(), (open = false));
    else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!usable.length) return;
      const at = usable.indexOf(sel);
      sel = usable[(at + (e.key === "ArrowDown" ? 1 : at < 0 ? 0 : -1) + usable.length) % usable.length];
    } else if (e.key === "Enter" && sel >= 0) (e.preventDefault(), start(items[sel]));
  }
</script>

<svelte:window onkeydown={onKey} onpointerdown={(e) => open && !root.contains(e.target as Node) && (open = false)} />

<div class="new" bind:this={root}>
  <button class="icon-btn" class:on={open} onclick={toggle} title="New agent" aria-label="New agent" aria-haspopup="menu" aria-expanded={open}>
    <Plus size={15} strokeWidth={1.5} />
  </button>
  {#if open}
    <div class="menu glass-float" role="menu">
      <div class="head">Start an agent in <b>{app.workspace.split("/").pop()}</b></div>
      {#each items as l, i (l.agent)}
        <button class="item" class:sel={i === sel} role="menuitem" disabled={!l.command} onmouseenter={() => l.command && (sel = i)} onclick={() => start(l)} title={l.command ? `Runs ${l.command}` : undefined}>
          <span class="icon"><ProviderIcon provider={l.agent} size={15} /></span>
          <span class="name">{providerName[l.agent]}</span>
          {#if launchers && !l.command}<span class="note">Not installed</span>{/if}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .new {
    position: relative;
  }
  .icon-btn.on {
    background: var(--hover);
    color: var(--foreground);
  }
  .menu {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: 20;
    width: 220px;
    padding: 5px;
    border-radius: 14px;
    animation: drop var(--dur-quick) var(--ease-standard);
  }
  .head {
    padding: 5px 8px 6px;
    overflow: hidden;
    color: var(--muted-foreground);
    font-size: 11.5px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .head b {
    color: var(--foreground);
    font-weight: 500;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    height: 32px;
    padding: 0 8px;
    border: 0;
    border-radius: 9px;
    background: none;
    color: var(--foreground);
    font-size: 13px;
    text-align: left;
    cursor: pointer;
  }
  .item.sel {
    background: var(--chip-on);
    box-shadow: var(--sh-chip), inset 3px 0 0 var(--gold);
  }
  .item:disabled {
    cursor: default;
  }
  .item:disabled > :not(.note) {
    opacity: 0.45;
  }
  .icon {
    display: flex;
  }
  .name {
    flex: 1;
  }
  .note {
    color: var(--muted-foreground);
    font-size: 11px;
  }
  @keyframes drop {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .menu {
      animation: none;
    }
  }
</style>
