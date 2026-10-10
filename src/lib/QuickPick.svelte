<script module lang="ts">
  // VS Code-style quick pick: one instance rendered by the app shell, driven by pick() / prompt().
  export type Item = { label: string; detail?: string; hint?: string; value: string };

  const quick = $state({
    open: false,
    placeholder: "",
    items: null as Item[] | null, // null = free-text prompt
    value: "", // initial text
    resolve: (_: string | null) => {},
  });

  function show(placeholder: string, items: Item[] | null, value = "") {
    quick.resolve(null); // a newer picker replaces any open one
    return new Promise<string | null>((resolve) => Object.assign(quick, { open: true, placeholder, items, value, resolve }));
  }
  /** Resolves to the chosen item's value, or null if dismissed. */
  export const pick = (placeholder: string, items: Item[]) => show(placeholder, items);
  /** Resolves to the entered text (trimmed, non-empty), or null if dismissed. */
  export const prompt = (placeholder: string, value = "") => show(placeholder, null, value);
</script>

<script lang="ts">
  let query = $state("");
  $effect(() => {
    if (quick.open) query = quick.value;
  });
  let sel = $state(0);
  const shown = $derived(
    quick.items?.filter((i) => (i.label + " " + (i.detail ?? "")).toLowerCase().includes(query.toLowerCase())) ?? [],
  );

  function close(value: string | null) {
    const r = quick.resolve;
    Object.assign(quick, { open: false, resolve: () => {} });
    query = "";
    sel = 0;
    r(value);
  }
  function key(e: KeyboardEvent) {
    if (e.key === "Escape") close(null);
    else if (e.key === "ArrowDown") (sel = Math.min(sel + 1, shown.length - 1)), e.preventDefault();
    else if (e.key === "ArrowUp") (sel = Math.max(sel - 1, 0)), e.preventDefault();
    else if (e.key === "Enter") {
      if (!quick.items) close(query.trim() || null);
      else if (shown[sel]) close(shown[sel].value);
    }
  }
</script>

{#if quick.open}
  <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
  <div class="scrim" onclick={() => close(null)}>
    <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
    <div class="box glass-float" onclick={(e) => e.stopPropagation()}>
      <input
        {@attach (el) => el.focus()}
        bind:value={query}
        oninput={() => (sel = 0)}
        onkeydown={key}
        placeholder={quick.placeholder}
        spellcheck="false"
      />
      {#if quick.items}
        <div class="list">
          {#each shown as it, i (it.value)}
            <button class="item" class:sel={i === sel} onmouseenter={() => (sel = i)} onclick={() => close(it.value)}>
              <span class="label">{it.label}</span>
              {#if it.detail}<span class="detail">{it.detail}</span>{/if}
              {#if it.hint}<span class="hint">{it.hint}</span>{/if}
            </button>
          {:else}
            <div class="none">No matches</div>
          {/each}
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  /* Plane 3: the app dims and softens behind thick glass */
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 100;
    background: var(--scrim);
    -webkit-backdrop-filter: blur(3px) saturate(90%);
    backdrop-filter: blur(3px) saturate(90%);
    animation: fade var(--dur-quick) var(--ease-standard);
  }
  .box {
    position: absolute;
    top: 64px;
    left: 50%;
    box-sizing: border-box;
    width: min(600px, calc(100vw - 32px));
    transform: translateX(-50%);
    padding: 8px;
    border-radius: var(--r-float);
    animation: rise var(--dur-base) var(--ease-out-quint);
  }
  @keyframes fade {
    from {
      opacity: 0;
    }
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translateX(-50%) translateY(-6px) scale(0.98);
    }
  }
  input {
    box-sizing: border-box;
    width: 100%;
    height: 42px;
    padding: 0 14px;
    border: 0;
    border-radius: 14px;
    background: oklch(var(--shadow-tint) / 0.06);
    box-shadow: inset 0 0 0 1px color-mix(in oklch, var(--gold) 50%, transparent), 0 0 0 4px var(--gold-soft);
    color: var(--foreground);
    font: inherit;
    font-size: 14px;
    outline: none;
  }
  :global(.dark) input {
    background: oklch(0 0 0 / 0.2);
  }
  .list {
    max-height: 360px;
    margin-top: 6px;
    overflow: auto;
  }
  .item {
    display: flex;
    align-items: baseline;
    gap: 10px;
    width: 100%;
    padding: 8px 12px;
    border: 0;
    border-radius: 12px;
    background: none;
    font-size: 13px;
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
  }
  .item.sel {
    background: var(--chip-on);
    box-shadow: var(--sh-chip), inset 3px 0 0 var(--gold);
  }
  .detail {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--muted-foreground);
    font-size: 12px;
  }
  .hint {
    color: var(--muted-foreground);
    font-size: 11.5px;
  }
  .none {
    padding: 8px;
    color: var(--muted-foreground);
    font-size: 13px;
  }
</style>
