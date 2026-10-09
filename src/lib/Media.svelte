<script lang="ts">
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { FileWarning, Grid2x2, Maximize, Minus, Moon, Music, Plus, Sun } from "@lucide/svelte";
  import { app } from "./state.svelte";
  import { isSvg, mediaKind } from "./media";

  // `src` overrides the file (the SVG preview renders the editor's text, unsaved edits included).
  let { path, src: given }: { path: string; src?: string } = $props();
  const kind = $derived(given ? "image" : mediaKind(path));
  const name = $derived(path.slice(path.lastIndexOf("/") + 1));
  // The webview caches file URLs: the version param picks up images regenerated on disk. (Not for players: it would restart them.)
  const src = $derived(given ?? `${convertFileSrc(path)}?v=${kind === "image" ? app.gitVersion : 0}`);
  let failed = $state(false);

  // Backdrop behind images: dark artwork disappears on the dark checkerboard (and light on light).
  const backdrops = ["checker", "light", "dark"] as const;
  let backdrop = $state<(typeof backdrops)[number]>("checker");
  const cycleBackdrop = () => (backdrop = backdrops[(backdrops.indexOf(backdrop) + 1) % backdrops.length]);

  // Image zoom: null = fit the view. Vectors may scale up to fill it, bitmaps only down.
  let nat = $state({ w: 0, h: 0 });
  let box = $state({ w: 0, h: 0 });
  let zoom = $state<number | null>(null);
  const vector = $derived(!!given || isSvg(path));
  const fit = $derived(nat.w && box.w ? Math.min(vector ? 8 : 1, (box.w - 48) / nat.w, (box.h - 48) / nat.h) : 1);
  const scale = $derived(zoom ?? fit);
  const steps = [0.1, 0.25, 0.5, 0.75, 1, 1.5, 2, 3, 4, 6, 8, 12, 16];
  const clamp = (z: number) => Math.min(16, Math.max(0.05, z));
  const step = (dir: 1 | -1) =>
    (zoom = dir > 0 ? (steps.find((s) => s > scale + 1e-6) ?? 16) : (steps.findLast((s) => s < scale - 1e-6) ?? 0.05));

  /** Pinch / ⌘-scroll zooms the image (not the app). Needs a non-passive listener to cancel scrolling. */
  function wheelZoom(el: HTMLElement) {
    const on = (ev: WheelEvent) => {
      if (!ev.ctrlKey && !ev.metaKey) return;
      ev.preventDefault();
      zoom = clamp(scale * Math.exp(-ev.deltaY * 0.01));
    };
    el.addEventListener("wheel", on, { passive: false });
    return () => el.removeEventListener("wheel", on);
  }
</script>

<div class="media {kind === 'image' && !failed ? `bg-${backdrop}` : ''}">
  {#if failed}
    <div class="empty">
      <FileWarning size={28} strokeWidth={1.25} />
      <span>Can't display <b>{name}</b></span>
      <small>The file is missing or in a format this viewer doesn't support.</small>
    </div>
  {:else if kind === "image"}
    <div class="stage" bind:clientWidth={box.w} bind:clientHeight={box.h} {@attach wheelZoom}>
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions, a11y_click_events_have_key_events -->
      <img
        {src}
        alt={name}
        draggable="false"
        style:width="{nat.w * scale}px"
        style:height="{nat.h * scale}px"
        class:framed={!vector}
        class:pixelated={!vector && scale > 2}
        class:hidden={!nat.w}
        ondblclick={() => (zoom = zoom === null ? 1 : null)}
        onload={(e) => {
          const i = e.currentTarget as HTMLImageElement;
          // An SVG without width/height reports 0: give it a sensible box.
          nat = { w: i.naturalWidth || 512, h: i.naturalHeight || 512 };
        }}
        onerror={() => (failed = true)}
      />
    </div>
    {#if nat.w}
      <div class="bar">
        <button class="icon-btn" onclick={() => step(-1)} title="Zoom out (⌘-scroll)"><Minus size={13} strokeWidth={1.5} /></button>
        <span class="pct">{Math.round(scale * 100)}%</span>
        <button class="icon-btn" onclick={() => step(1)} title="Zoom in (⌘-scroll)"><Plus size={13} strokeWidth={1.5} /></button>
        <button class="icon-btn" class:on={zoom === null} onclick={() => (zoom = null)} title="Fit (double-click toggles 100%)"><Maximize size={13} strokeWidth={1.5} /></button>
        <button class="icon-btn" onclick={cycleBackdrop} title="Background: {backdrop}">
          {#if backdrop === "checker"}<Grid2x2 size={13} strokeWidth={1.5} />{:else if backdrop === "light"}<Sun size={13} strokeWidth={1.5} />{:else}<Moon size={13} strokeWidth={1.5} />{/if}
        </button>
        {#if !given}<span class="dims">{nat.w} × {nat.h}</span>{/if}
      </div>
    {/if}
  {:else if kind === "video"}
    <div class="stage">
      <!-- svelte-ignore a11y_media_has_caption -->
      <video {src} controls preload="metadata" onerror={() => (failed = true)}></video>
    </div>
  {:else if kind === "audio"}
    <div class="stage">
      <div class="track">
        <div class="art"><Music size={30} strokeWidth={1.25} /></div>
        <span class="title" title={path}>{name}</span>
        <audio {src} controls preload="metadata" onerror={() => (failed = true)}></audio>
      </div>
    </div>
  {/if}
</div>

<style>
  .media {
    position: absolute;
    inset: 0;
    background: var(--surface);
  }
  /* Transparency checkerboard, subtle in both themes */
  .bg-light {
    background: oklch(0.98 0.004 80);
  }
  .bg-dark {
    background: oklch(0.17 0.006 78);
  }
  .bg-checker {
    background:
      repeating-conic-gradient(color-mix(in oklch, var(--muted-foreground) 9%, transparent) 0 25%, transparent 0 50%) 0 0 / 18px 18px,
      var(--surface);
  }
  .stage {
    position: absolute;
    inset: 0;
    display: flex;
    overflow: auto;
    padding: 24px;
    box-sizing: border-box;
  }
  /* margin: auto centers without clipping the top/left once it overflows (unlike place-items) */
  img,
  video,
  .track {
    flex: none;
    margin: auto;
  }
  img {
    cursor: zoom-in;
  }
  img.framed {
    box-shadow: 0 0 0 1px var(--border), var(--shadow-raised);
  }
  img.pixelated {
    image-rendering: pixelated;
  }
  img.hidden {
    visibility: hidden;
  }
  video {
    max-width: 100%;
    max-height: 100%;
    border-radius: var(--radius-md);
    background: black;
    box-shadow: var(--shadow-raised);
  }
  .track {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 14px;
    width: min(420px, 100%);
    padding: 28px 24px 22px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--card);
    box-shadow: var(--shadow-raised);
    box-sizing: border-box;
  }
  .art {
    display: grid;
    place-items: center;
    width: 84px;
    height: 84px;
    border-radius: var(--radius);
    background: var(--gold-soft);
    color: var(--gold-ring);
  }
  .title {
    max-width: 100%;
    overflow: hidden;
    font-size: 13px;
    font-weight: 500;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  audio {
    width: 100%;
  }
  .bar {
    position: absolute;
    bottom: 14px;
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 2px;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background: var(--popover);
    box-shadow: var(--shadow-raised);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    color: var(--muted-foreground);
  }
  .pct {
    min-width: 40px;
    text-align: center;
  }
  .icon-btn.on {
    color: var(--foreground);
    background: var(--accent);
  }
  .dims {
    padding: 0 8px 0 6px;
    border-left: 1px solid var(--border);
  }
  .empty {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    color: var(--muted-foreground);
    font-size: 13px;
  }
  .empty b {
    color: var(--foreground);
    font-weight: 500;
  }
</style>
