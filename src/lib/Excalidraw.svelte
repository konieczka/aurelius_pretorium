<script module lang="ts">
  import type { Root } from "react-dom/client";

  // Excalidraw is a React component of several MB: loaded with React on the first drawing opened.
  async function importLib() {
    (window as { EXCALIDRAW_ASSET_PATH?: string }).EXCALIDRAW_ASSET_PATH = "/excalidraw/"; // fonts (vite.config.js)
    const [ex, react, dom] = await Promise.all([import("@excalidraw/excalidraw"), import("react"), import("react-dom/client"), import("@excalidraw/excalidraw/index.css")]);
    return { ex, react, dom };
  }
  type Lib = Awaited<ReturnType<typeof importLib>>;
  let lib: Promise<Lib> | undefined;
  const load = () => (lib ??= importLib());
</script>

<script lang="ts">
  import { untrack } from "svelte";
  import { FileWarning } from "@lucide/svelte";
  import { theme } from "./theme.svelte";

  // The drawing edits the file's text: `source` is read once (remount for a new one). Changes are marked at
  // once (`onedit`) and written back (`oncommit`) a moment later, or when `register`ed flush is called (save).
  let { source, onedit, oncommit, onlink, register }: {
    source: string;
    onedit: () => void;
    oncommit: (json: string) => void;
    onlink: (url: string) => void;
    register: (flush: (() => void) | undefined) => void;
  } = $props();

  const initial = untrack(() => {
    try {
      const data = source.trim() ? JSON.parse(source) : {}; // a new, empty file is a blank drawing
      if (typeof data !== "object" || !data || (data.elements !== undefined && !Array.isArray(data.elements))) throw 0;
      return { elements: data.elements ?? [], appState: data.appState ?? {}, files: data.files ?? {}, scrollToContent: true };
    } catch {
      return null;
    }
  });

  let host: HTMLDivElement;
  let ready = $state<Lib>();
  let failed = $state(initial ? "" : "This isn't a valid Excalidraw drawing.");
  let root: Root | undefined;

  // Scene changes: onChange also fires for scrolling, zooming and selecting, so compare what's saved.
  let saved: string | undefined;
  let pending: (() => string) | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  function flush() {
    clearTimeout(timer);
    const json = pending?.();
    pending = undefined;
    if (json !== undefined) oncommit(json);
  }

  $effect(() => {
    if (!initial) return;
    load().then((l) => (ready = l), (err) => (failed = `Couldn't load the drawing editor: ${err}`));
    register(flush);
    return () => {
      flush();
      register(undefined);
      root?.unmount();
    };
  });

  $effect(() => {
    if (!ready) return;
    const dark = theme.dark;
    const { ex, react } = ready;
    root ??= ready.dom.createRoot(host);
    root.render(
      react.createElement(
        ex.Excalidraw,
        {
          initialData: initial,
          theme: dark ? "dark" : "light",
          autoFocus: true,
          UIOptions: { canvasActions: { loadScene: false, saveToActiveFile: false, export: false, toggleTheme: false } },
          onChange(elements, appState, files) {
            const key = `${ex.hashElementsVersion(elements)}:${appState.viewBackgroundColor}:${Object.keys(files).length}`;
            if (saved === undefined) saved = key; // the first call: the drawing as loaded
            if (key === saved) return;
            saved = key;
            onedit();
            pending = () => ex.serializeAsJSON(elements, appState, files, "local");
            clearTimeout(timer);
            timer = setTimeout(flush, 400);
          },
          onLinkOpen(element, event) {
            const url = element.link ?? "";
            if (!/^https?:\/\//i.test(url)) return; // links to elements in the drawing: Excalidraw's own
            event.preventDefault();
            onlink(url);
          },
        },
        react.createElement(
          ex.MainMenu,
          null,
          react.createElement(ex.MainMenu.DefaultItems.SaveAsImage),
          react.createElement(ex.MainMenu.DefaultItems.SearchMenu),
          react.createElement(ex.MainMenu.DefaultItems.CommandPalette),
          react.createElement(ex.MainMenu.DefaultItems.Help),
          react.createElement(ex.MainMenu.DefaultItems.ClearCanvas),
          react.createElement(ex.MainMenu.Separator),
          react.createElement(ex.MainMenu.DefaultItems.ChangeCanvasBackground),
        ),
      ),
    );
  });
</script>

<div class="drawing" bind:this={host} class:hidden={failed}></div>
{#if failed}
  <div class="empty">
    <FileWarning size={28} strokeWidth={1.25} />
    <span>{failed}</span>
    <small>Edit its JSON with ⌘E.</small>
  </div>
{/if}

<style>
  .drawing,
  .empty {
    position: absolute;
    inset: 0;
    background: var(--surface);
  }
  .drawing :global(.excalidraw) {
    --ui-font: var(--font-sans);
  }
  .hidden {
    display: none;
  }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    color: var(--muted-foreground);
    font-size: 13px;
  }
  .empty small {
    font-size: 12px;
  }
</style>
