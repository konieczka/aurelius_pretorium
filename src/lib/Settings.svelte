<script lang="ts">
  import { Check, Monitor, Moon, Sun, X } from "@lucide/svelte";
  import { closeSettings } from "./settingsView.svelte";
  import { PALETTES, setMode, setPalette, theme, type Mode } from "./theme.svelte";

  const modes: { id: Mode; label: string; icon: typeof Sun }[] = [
    { id: "light", label: "Light", icon: Sun },
    { id: "dark", label: "Dark", icon: Moon },
    { id: "system", label: "System", icon: Monitor },
  ];

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape" && !e.defaultPrevented && !(e.target as Element).closest?.("input, textarea")) closeSettings();
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="settings slab">
  <div class="page">
    <header>
      <div>
        <h1>Settings</h1>
        <p class="sub">How Pretorium looks, in every window</p>
      </div>
      <button class="icon-btn" onclick={closeSettings} title="Close (Esc)" aria-label="Close settings"><X size={16} strokeWidth={1.5} /></button>
    </header>

    <section>
      <h2>Theme</h2>
      <div class="modes" role="radiogroup" aria-label="Theme">
        {#each modes as m (m.id)}
          <button class="mode" class:on={theme.mode === m.id} role="radio" aria-checked={theme.mode === m.id} onclick={() => setMode(m.id)}>
            <m.icon size={14} strokeWidth={1.5} />{m.label}
          </button>
        {/each}
      </div>
      <p class="hint">System follows macOS: light by day, dark by night if you have it set that way.</p>
    </section>

    <section>
      <h2>Palette</h2>
      <div class="palettes" role="radiogroup" aria-label="Palette">
        {#each PALETTES as p (p.id)}
          <button class="palette" class:on={theme.palette === p.id} role="radio" aria-checked={theme.palette === p.id} onclick={() => setPalette(p.id)}>
            <!-- A tiny Pretorium in this palette (and the current mode): its variables apply from data-palette down -->
            <div class="preview" data-palette={p.id} aria-hidden="true">
              <div class="mini-bar"><i class="lamp"></i><i class="chip"></i><i class="chip short"></i></div>
              <div class="mini-body">
                <div class="mini-side"><i></i><i class="sel"></i><i></i><i></i></div>
                <div class="mini-slab">
                  <div class="mini-head"><i class="tab"></i><i class="tab dim"></i></div>
                  <div class="mini-lines"><i style="width: 70%"></i><i class="acc" style="width: 38%"></i><i style="width: 55%"></i><i class="dim" style="width: 44%"></i></div>
                </div>
              </div>
            </div>
            <div class="label">
              <span class="name">{p.name}{#if p.id === "aurelius"}<span class="default">Default</span>{/if}</span>
              <span class="blurb">{p.blurb}</span>
            </div>
            {#if theme.palette === p.id}<span class="check"><Check size={12} strokeWidth={2.5} /></span>{/if}
          </button>
        {/each}
      </div>
    </section>
  </div>
</div>

<style>
  /* Plane 2: a slab, like the panes it stands in for */
  .settings {
    grid-column: 2;
    grid-row: 2;
    min-width: 0;
    overflow: auto;
    /* Room for the scrollbar on both sides, so the column stays centred (and as wide) whether or not it scrolls */
    scrollbar-gutter: stable both-edges;
  }
  .page {
    box-sizing: border-box; /* like Skills and Agents: --page-width includes the padding */
    max-width: var(--page-width);
    margin: 0 auto;
    padding: 28px 32px 48px;
  }
  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    margin-bottom: 8px;
  }
  h1 {
    margin: 0;
    font-size: 20px;
    font-weight: 600;
  }
  .sub {
    margin: 4px 0 0;
    color: var(--muted-foreground);
    font-size: 13px;
  }
  section {
    margin-top: 28px;
  }
  h2 {
    margin: 0 0 12px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--muted-foreground);
  }
  .hint {
    margin: 10px 0 0;
    color: var(--muted-foreground);
    font-size: 12px;
  }

  /* Segmented control, like the sidebar's Files / Changes */
  .modes {
    display: inline-flex;
    gap: 2px;
    padding: 3px;
    border-radius: 12px;
    background: oklch(var(--shadow-tint) / 0.07);
    box-shadow: inset 0 1px 2px oklch(var(--shadow-tint) / 0.1);
  }
  :global(.dark) .modes {
    background: oklch(var(--shadow-tint) / 0.3);
    box-shadow: inset 0 1px 2px oklch(var(--shadow-tint) / 0.4);
  }
  .mode {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    height: 30px;
    padding: 0 14px;
    border: 0;
    border-radius: 9px;
    background: none;
    color: var(--muted-foreground);
    font-size: 13px;
    cursor: pointer;
    transition: color var(--dur-quick) var(--ease-standard), background-color var(--dur-quick) var(--ease-standard);
  }
  .mode:hover {
    color: var(--foreground);
  }
  .mode.on {
    background: var(--chip-on);
    box-shadow: var(--sh-chip);
    color: var(--foreground);
  }

  .palettes {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    gap: 14px;
  }
  .palette {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 8px 8px 12px;
    border: 0;
    border-radius: 18px;
    background: var(--hover);
    box-shadow: inset 0 0 0 1px color-mix(in oklch, var(--border) 60%, transparent);
    text-align: left;
    cursor: pointer;
    transition: box-shadow var(--dur-base) var(--ease-standard), transform var(--dur-quick) var(--ease-standard);
  }
  .palette:hover {
    box-shadow: inset 0 0 0 1px var(--border-strong);
  }
  .palette:active {
    transform: scale(0.99);
  }
  /* Gold marks selection (the app's gold, not the swatch's) */
  .palette.on {
    box-shadow: inset 0 0 0 1.5px var(--gold), 0 0 24px -8px color-mix(in oklch, var(--gold) 50%, transparent);
  }
  .check {
    position: absolute;
    top: 14px;
    right: 14px;
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    border-radius: 9999px;
    background: var(--gold);
    color: var(--primary-foreground);
    box-shadow: 0 2px 6px oklch(var(--shadow-tint) / 0.3);
  }
  :global(.dark) .check {
    color: var(--surface);
  }
  .label {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 0 6px;
  }
  .name {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13.5px;
    font-weight: 550;
  }
  .default {
    padding: 1px 7px;
    border-radius: 9999px;
    background: var(--gold-soft);
    color: var(--foreground);
    font-size: 10.5px;
    font-weight: 500;
  }
  .blurb {
    color: var(--muted-foreground);
    font-size: 12px;
  }

  /* The swatch: field, glass bar, sidebar island and a work slab, all from the palette's own variables */
  .preview {
    display: flex;
    flex-direction: column;
    gap: 5px;
    height: 128px;
    padding: 6px;
    box-sizing: border-box;
    border-radius: 12px;
    overflow: hidden;
    background:
      radial-gradient(60% 70% at 15% 0%, var(--glow-a), transparent 70%),
      radial-gradient(70% 70% at 100% 100%, var(--glow-b), transparent 70%),
      linear-gradient(135deg, var(--gradient-from), var(--gradient-to));
    color: var(--foreground);
  }
  .preview i {
    display: block;
  }
  .mini-bar {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 12px;
  }
  .lamp {
    width: 8px;
    height: 8px;
    margin: 0 4px 0 2px;
    border-radius: 9999px;
    background: var(--gold);
    box-shadow: 0 0 6px var(--gold);
  }
  .chip {
    width: 34px;
    height: 9px;
    border-radius: 4px;
    background: var(--chip-on);
  }
  .chip.short {
    width: 22px;
    background: var(--chrome);
  }
  .mini-body {
    display: flex;
    flex: 1;
    gap: 5px;
    min-height: 0;
  }
  .mini-side {
    display: flex;
    flex-direction: column;
    gap: 4px;
    width: 26%;
    padding: 6px 5px;
    box-sizing: border-box;
    border-radius: 8px;
    background: var(--chrome);
    box-shadow: inset 0 1px 0 var(--hl), 0 0 0 1px oklch(var(--shadow-tint) / 0.12);
  }
  .mini-side i {
    height: 5px;
    border-radius: 3px;
    background: color-mix(in oklch, var(--muted-foreground) 35%, transparent);
  }
  .mini-side i.sel {
    background: var(--gold-soft);
    box-shadow: inset 0 0 0 1px color-mix(in oklch, var(--gold) 45%, transparent);
  }
  .mini-slab {
    flex: 1;
    display: flex;
    flex-direction: column;
    border-radius: 8px;
    overflow: hidden;
    background: var(--surface);
    box-shadow: inset 0 1px 0 var(--hl), 0 0 0 1px color-mix(in oklch, var(--gold) 50%, transparent), 0 6px 14px -6px oklch(var(--shadow-tint) / 0.5);
  }
  .mini-head {
    display: flex;
    gap: 3px;
    padding: 4px;
    background: var(--slab-head);
  }
  .tab {
    width: 28px;
    height: 7px;
    border-radius: 3px;
    background: var(--chip-on);
    box-shadow: 0 1px 2px oklch(var(--shadow-tint) / 0.25);
  }
  .tab.dim {
    width: 20px;
    background: none;
    box-shadow: inset 0 0 0 1px color-mix(in oklch, var(--muted-foreground) 25%, transparent);
  }
  .mini-lines {
    display: flex;
    flex-direction: column;
    gap: 5px;
    padding: 7px 8px;
  }
  .mini-lines i {
    height: 4px;
    border-radius: 2px;
    background: color-mix(in oklch, var(--foreground) 55%, transparent);
  }
  .mini-lines i.acc {
    background: var(--gold);
  }
  .mini-lines i.dim {
    background: color-mix(in oklch, var(--muted-foreground) 40%, transparent);
  }
</style>
