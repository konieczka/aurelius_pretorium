<script module lang="ts">
  // VS Code-style quick pick: one instance rendered by the app shell, driven by pick() / prompt().
  // `removable` items get an X (and ⌘⌫) that hands them to pick()'s `remove` and drops them from the list, unless
  // `remove` returns false (e.g. a confirmation was cancelled). A string means a delete: a trash can, with it as tooltip.
  export type Item = { label: string; detail?: string; hint?: string; value: string; removable?: boolean | string };
  type Remove = (value: string) => unknown;

  const quick = $state({
    open: false,
    placeholder: "",
    items: null as Item[] | null, // null = free-text prompt
    value: "", // initial text
    resolve: (_: string | null) => {},
    remove: (() => {}) as Remove,
  });

  function show(placeholder: string, items: Item[] | null, value = "", remove: Remove = () => {}) {
    quick.resolve(null); // a newer picker replaces any open one
    return new Promise<string | null>((resolve) => Object.assign(quick, { open: true, placeholder, items, value, resolve, remove }));
  }
  /** Resolves to the chosen item's value, or null if dismissed. */
  export const pick = (placeholder: string, items: Item[], remove?: Remove) => show(placeholder, items, "", remove);
  /** It's on screen (browser tabs hide their pages, which would cover it). */
  export const picking = () => quick.open;
  /** Resolves to the entered text (trimmed, non-empty), or null if dismissed. */
  export const prompt = (placeholder: string, value = "") => show(placeholder, null, value);
</script>

<script lang="ts">
  import { Trash2, X } from "@lucide/svelte";

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
    Object.assign(quick, { open: false, resolve: () => {}, remove: () => {} });
    query = "";
    sel = 0;
    r(value);
  }
  async function remove(it: Item) {
    if ((await quick.remove(it.value)) === false || !quick.items) return;
    quick.items = quick.items.filter((i) => i !== it);
    sel = Math.min(sel, Math.max(shown.length - 1, 0));
  }
  function key(e: KeyboardEvent) {
    if (e.key === "Escape") close(null);
    else if (e.key === "Backspace" && e.metaKey && shown[sel]?.removable) remove(shown[sel]), e.preventDefault();
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
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_interactive_supports_focus (keys go through the input) -->
            <div class="item" class:sel={i === sel} role="option" aria-selected={i === sel} onmouseenter={() => (sel = i)} onclick={() => close(it.value)}>
              <span class="label">{it.label}</span>
              {#if it.detail}<span class="detail">{it.detail}</span>{/if}
              {#if it.hint}<span class="hint">{it.hint}</span>{/if}
              {#if it.removable}
                <button class="icon-btn remove" class:delete={typeof it.removable === "string"} title="{typeof it.removable === 'string' ? it.removable : 'Remove from list'} (⌘⌫)" onclick={(e) => (e.stopPropagation(), remove(it))}>
                  {#if typeof it.removable === "string"}<Trash2 size={13} strokeWidth={1.5} />{:else}<X size={13} strokeWidth={1.5} />{/if}
                </button>
              {/if}
            </div>
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
    overflow: hidden auto;
  }
  .item {
    box-sizing: border-box;
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
  .remove {
    align-self: center;
    width: 22px;
    height: 22px;
    margin: -4px -6px -4px auto; /* right edge, also when there's no detail to push it there */
    visibility: hidden;
  }
  .remove.delete:hover {
    color: var(--destructive);
  }
  .item.sel .remove {
    visibility: visible;
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
