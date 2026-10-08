<script lang="ts">
  import "../app.css";
  import { open } from "@tauri-apps/plugin-dialog";
  import { FilePlus, FolderOpen, FolderPlus, Monitor, Moon, RefreshCw, Sun } from "@lucide/svelte";
  import { cycleTheme, theme } from "$lib/theme.svelte";
  import { zoom } from "$lib/zoom";
  import Node from "$lib/Node.svelte";
  import FileTree from "$lib/FileTree.svelte";
  import { saveEditor } from "$lib/Editor.svelte";
  import {
    app, activeTab, closeTab, deleteSelected, lastWorkspace, moveInto, undoTree, newTerminal, openWorkspace, saveLayout, splitFocused, startCreate,
  } from "$lib/state.svelte";

  const last = lastWorkspace();
  if (last) openWorkspace(last);

  $effect(saveLayout);
  const ThemeIcon = $derived({ light: Sun, dark: Moon, system: Monitor }[theme.mode]);

  async function pickFolder() {
    const dir = await open({ directory: true });
    if (typeof dir === "string") openWorkspace(dir);
  }

  function onKey(e: KeyboardEvent) {
    const k = e.key.toLowerCase();
    // Typing into a pane hands ⌘Z back to it. Programmatic focus (a tab mounting) deliberately doesn't.
    if (!e.metaKey && !e.ctrlKey && (e.target as Element).closest?.("main")) app.treeFocused = false;
    // Tree undo/delete only when the sidebar has focus; otherwise ⌘Z belongs to the editor/terminal.
    if (app.treeFocused && !app.creating && (e.metaKey || e.ctrlKey) && !e.shiftKey && (k === "z" || k === "backspace")) {
      e.preventDefault();
      e.stopPropagation();
      return k === "z" ? undoTree() : deleteSelected();
    }
    if (!e.metaKey) return;
    const t = activeTab();
    const act =
      k === "=" || k === "+" ? () => zoom(0.1) :
      k === "-" ? () => zoom(-0.1) :
      k === "0" ? () => zoom(null) :
      k === "o" ? pickFolder :
      !app.workspace ? null :
      k === "t" ? newTerminal :
      k === "d" ? () => splitFocused(e.shiftKey ? "col" : "row") :
      k === "w" ? () => t && closeTab(t.id) :
      k === "s" ? () => t?.kind === "file" && saveEditor(t.id) :
      k === "e" ? () => t?.kind === "file" && t.path.endsWith(".md") && (t.preview = !t.preview) :
      null;
    if (!act) return;
    e.preventDefault();
    e.stopPropagation();
    act();
  }
</script>

<svelte:window onkeydowncapture={onKey} />

{#snippet themeToggle()}
  <button class="icon-btn" onclick={cycleTheme} title="Theme: {theme.mode}" aria-label="Theme: {theme.mode}. Click to change.">
    <ThemeIcon size={15} strokeWidth={1.5} />
  </button>
{/snippet}

{#if app.workspace}
  <div class="app">
    <aside onpointerdown={() => (app.treeFocused = true)}>
      <header>
        <span class="mark"></span>
        <span class="ws" title={app.workspace}>{app.workspace.split("/").pop()}</span>
        <button class="icon-btn" onclick={() => startCreate("file")} title="New file"><FilePlus size={14} strokeWidth={1.5} /></button>
        <button class="icon-btn" onclick={() => startCreate("dir")} title="New folder"><FolderPlus size={14} strokeWidth={1.5} /></button>
        <button class="icon-btn" onclick={() => app.treeVersion++} title="Refresh"><RefreshCw size={14} strokeWidth={1.5} /></button>
        <button class="icon-btn" onclick={pickFolder} title="Open folder (⌘O)"><FolderOpen size={15} strokeWidth={1.5} /></button>
        {@render themeToggle()}
      </header>
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="tree"
        class:drop={app.dropDir === app.workspace}
        onclick={(e) => e.target === e.currentTarget && (app.selectedDir = app.workspace)}
        onkeydown={() => {}}
        ondragover={(e) => {
          if (!e.dataTransfer?.types.includes("text/x-path")) return;
          e.preventDefault();
          app.dropDir = app.workspace;
        }}
        ondragleave={(e) => e.target === e.currentTarget && (app.dropDir = "")}
        ondrop={(e) => {
          const from = e.dataTransfer?.getData("text/x-path");
          app.dropDir = "";
          if (from) moveInto(from, app.workspace);
        }}
      >
        {#key app.workspace}<FileTree path={app.workspace} />{/key}
      </div>
    </aside>
    <main onpointerdown={() => (app.treeFocused = false)}><Node bind:node={app.root} /></main>
  </div>
{:else}
  <div class="welcome">
    <div class="corner">{@render themeToggle()}</div>
    <span class="mark big"></span>
    <h1>Fleet Command</h1>
    <p>An Aurelius workspace</p>
    <button class="open" onclick={pickFolder}><FolderOpen size={16} strokeWidth={1.5} /> Open Folder <kbd>⌘O</kbd></button>
  </div>
{/if}

<style>
  .app {
    display: grid;
    grid-template-columns: 248px 1fr;
    height: 100vh;
  }
  aside {
    display: flex;
    flex-direction: column;
    min-height: 0;
    background: var(--sidebar);
    border-right: 1px solid var(--border-strong);
  }
  header {
    display: flex;
    align-items: center;
    gap: 2px;
    height: 34px;
    flex: none;
    padding: 0 6px 0 12px;
    border-bottom: 1px solid var(--sidebar-border);
  }
  .ws {
    flex: 1;
    margin-left: 6px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 13px;
    font-weight: 500;
  }
  .tree {
    flex: 1;
    overflow: auto;
    padding: 6px 6px 12px;
  }
  .tree.drop {
    background: var(--gold-soft);
    box-shadow: inset 0 0 0 1px var(--gold-ring);
  }
  main {
    position: relative;
    min-width: 0;
  }
  .welcome {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100vh;
  }
  .corner {
    position: absolute;
    top: 12px;
    right: 12px;
  }
  .mark.big {
    width: 88px;
    height: 88px;
    filter: drop-shadow(0 2px 4px rgba(0, 0, 0, 0.35));
  }
  h1 {
    margin: 20px 0 0;
    font-family: var(--font-cinzel);
    font-size: 2rem;
    font-weight: 600;
    letter-spacing: 0.02em;
    background: var(--wordmark-gradient);
    -webkit-background-clip: text;
    background-clip: text;
    color: transparent;
  }
  p {
    margin: 6px 0 28px;
    color: var(--muted-foreground);
    font-size: 11px;
    font-weight: 500;
    letter-spacing: 0.16em;
    text-transform: uppercase;
  }
  /* Aurelius Button variant="outline" */
  .open {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 36px;
    padding: 0 16px;
    border: 1px solid var(--input);
    border-radius: var(--radius-md);
    background: var(--background);
    box-shadow: var(--shadow-raised);
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: background-color var(--dur-quick) var(--ease-standard), transform var(--dur-quick) var(--ease-standard);
  }
  .open:hover {
    background: var(--accent);
  }
  .open:active {
    transform: scale(0.98);
  }
  kbd {
    margin-left: 4px;
    color: var(--muted-foreground);
    font-family: inherit;
    font-size: 12px;
  }
</style>
