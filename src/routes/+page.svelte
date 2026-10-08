<script lang="ts">
  import "../app.css";
  import { open } from "@tauri-apps/plugin-dialog";
  import { ArrowDown, ArrowUp, ChevronsDownUp, FilePlus, Files, GitBranch, FolderOpen, FolderPlus, RefreshCw } from "@lucide/svelte";
  import { zoom } from "$lib/zoom";
  import Node from "$lib/Node.svelte";
  import StatusBar from "$lib/StatusBar.svelte";
  import QuickPick, { pick } from "$lib/QuickPick.svelte";
  import FileTree from "$lib/FileTree.svelte";
  import GitPanel, { changeCount, git, pull, push, refreshGit, watchGit } from "$lib/GitPanel.svelte";
  import { saveEditor } from "$lib/Editor.svelte";
  import {
    app, activeTab, closeTab, deleteSelected, lastWorkspace, moveInto, undoTree, newTerminal, openWorkspace, saveLayout, splitFocused, startCreate,
  } from "$lib/state.svelte";

  const last = lastWorkspace();
  if (last) openWorkspace(last);

  $effect(saveLayout);
  $effect(() => {
    if (app.workspace) return watchGit();
  });

  async function pickFolder() {
    const dir = await open({ directory: true });
    if (typeof dir === "string") openWorkspace(dir);
  }

  /** Switch the sidebar to another folder/project; open tabs stay. */
  async function switchFolder() {
    const v = await pick("Open a folder or project", [
      { label: "Open folder…", hint: "⌘O", value: "\0open" },
      ...app.recent.map((r) => ({ label: r.slice(r.lastIndexOf("/") + 1), detail: r, value: r })),
    ]);
    if (v === "\0open") pickFolder();
    else if (v) openWorkspace(v);
  }

  function onKey(e: KeyboardEvent) {
    const k = e.key.toLowerCase();
    // Typing into a pane hands ⌘Z back to it. Programmatic focus (a tab mounting) deliberately doesn't.
    if (!e.metaKey && !e.ctrlKey && (e.target as Element).closest?.("main")) app.treeFocused = false;
    // Tree undo/delete only when the sidebar has focus; otherwise ⌘Z belongs to the editor/terminal.
    if (app.treeFocused && app.sidebar === "files" && !(e.target as Element).closest?.("input, textarea") && (e.metaKey || e.ctrlKey) && !e.shiftKey && (k === "z" || k === "backspace")) {
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
      k === "p" ? switchFolder :
      !app.workspace ? null :
      k === "e" && e.shiftKey ? () => (app.sidebar = "files") :
      k === "g" && e.shiftKey ? () => (app.sidebar = "git") :
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

{#if app.workspace}
  <div class="app">
    <aside onpointerdown={() => (app.treeFocused = true)}>
      <header>
        <span class="mark"></span>
        <button class="ws" onclick={switchFolder} title="{app.workspace} (⌘P to switch)">{app.workspace.split("/").pop()}</button>
        <button class="icon-btn" onclick={pickFolder} title="Open folder (⌘O)"><FolderOpen size={15} strokeWidth={1.5} /></button>
      </header>
      <div class="toolbar">
        <div class="modes" role="tablist">
          <button class="icon-btn mode" class:on={app.sidebar === "files"} role="tab" aria-selected={app.sidebar === "files"} onclick={() => (app.sidebar = "files")} title="Files (⇧⌘E)"><Files size={15} strokeWidth={1.5} /></button>
          <button class="icon-btn mode" class:on={app.sidebar === "git"} role="tab" aria-selected={app.sidebar === "git"} onclick={() => (app.sidebar = "git")} title="Source Control (⇧⌘G)">
            <GitBranch size={15} strokeWidth={1.5} />
            {#if changeCount()}<span class="badge">{changeCount()}</span>{/if}
          </button>
        </div>
        <span class="spacer"></span>
        {#if app.sidebar === "files"}
          <button class="icon-btn" onclick={() => startCreate("file")} title="New file"><FilePlus size={14} strokeWidth={1.5} /></button>
          <button class="icon-btn" onclick={() => startCreate("dir")} title="New folder"><FolderPlus size={14} strokeWidth={1.5} /></button>
          <button class="icon-btn" onclick={() => app.treeVersion++} title="Refresh"><RefreshCw size={14} strokeWidth={1.5} /></button>
          <button class="icon-btn" onclick={() => (app.expanded = {})} title="Collapse all folders"><ChevronsDownUp size={14} strokeWidth={1.5} /></button>
        {:else if git.root}
          <button class="icon-btn" onclick={refreshGit} title="Refresh"><RefreshCw size={14} strokeWidth={1.5} /></button>
          <button class="icon-btn" onclick={pull} disabled={!!git.busy} title="Pull"><ArrowDown size={14} strokeWidth={1.5} /></button>
          <button class="icon-btn" onclick={push} disabled={!!git.busy} title="Push"><ArrowUp size={14} strokeWidth={1.5} /></button>
        {/if}
      </div>
      {#if app.sidebar === "files"}
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
      {:else}
        <div class="tree"><GitPanel /></div>
      {/if}
    </aside>
    <main onpointerdown={() => (app.treeFocused = false)}><Node bind:node={app.root} /></main>
    <StatusBar />
  </div>
  <QuickPick />
{:else}
  <div class="welcome">
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
    grid-template-rows: minmax(0, 1fr) auto;
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
    height: 34px;
    flex: none;
    padding: 0 6px 0 12px;
    border-bottom: 1px solid var(--sidebar-border);
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 2px;
    height: 32px;
    flex: none;
    padding: 0 6px;
    border-bottom: 1px solid var(--sidebar-border);
  }
  .spacer {
    flex: 1;
  }
  .modes {
    display: flex;
    gap: 2px;
  }
  /* Gold marks the active tool */
  .mode {
    position: relative;
  }
  .mode.on {
    background: var(--sidebar-accent);
    color: var(--sidebar-foreground);
  }
  .mode.on::after {
    content: "";
    position: absolute;
    left: 6px;
    right: 6px;
    bottom: -3px;
    height: 2px;
    border-radius: 9999px;
    background: var(--gold);
  }
  .badge {
    position: absolute;
    top: -3px;
    right: -5px;
    min-width: 14px;
    height: 14px;
    padding: 0 3px;
    border-radius: 9999px;
    background: var(--primary);
    color: var(--primary-foreground);
    font-size: 9.5px;
    font-weight: 600;
    line-height: 14px;
    text-align: center;
    box-sizing: border-box;
  }
  .ws {
    flex: 1;
    min-width: 0;
    padding: 0;
    border: 0;
    background: none;
    text-align: left;
    cursor: pointer;
    margin-left: 8px;
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
