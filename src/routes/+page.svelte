<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import Node from "$lib/Node.svelte";
  import FileTree from "$lib/FileTree.svelte";
  import { saveEditor } from "$lib/Editor.svelte";
  import {
    app, activeTab, closeTab, lastWorkspace, newTerminal, openWorkspace, saveLayout, splitFocused,
  } from "$lib/state.svelte";

  const last = lastWorkspace();
  if (last) openWorkspace(last);

  $effect(saveLayout);

  async function pickFolder() {
    const dir = await open({ directory: true });
    if (typeof dir === "string") openWorkspace(dir);
  }

  function onKey(e: KeyboardEvent) {
    if (!e.metaKey) return;
    const t = activeTab();
    const k = e.key.toLowerCase();
    const act =
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

{#if app.workspace}
  <div class="app">
    <aside>
      <header>
        <span class="ws" title={app.workspace}>{app.workspace.split("/").pop()}</span>
        <button onclick={() => app.treeVersion++} title="Refresh">↻</button>
        <button onclick={pickFolder} title="Open folder (⌘O)">⌘O</button>
      </header>
      <div class="tree">
        {#key app.workspace}<FileTree path={app.workspace} />{/key}
      </div>
    </aside>
    <main><Node bind:node={app.root} /></main>
  </div>
{:else}
  <div class="welcome">
    <h1>Fleet Command</h1>
    <button onclick={pickFolder}>Open Folder <kbd>⌘O</kbd></button>
  </div>
{/if}

<style>
  :global(html, body) {
    margin: 0;
    height: 100%;
    overflow: hidden;
    background: #1e2127;
    color: #abb2bf;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    -webkit-font-smoothing: antialiased;
  }
  .app {
    display: grid;
    grid-template-columns: 240px 1fr;
    height: 100vh;
  }
  aside {
    display: flex;
    flex-direction: column;
    min-height: 0;
    background: #21252b;
    border-right: 1px solid #181a1f;
  }
  header {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 30px;
    padding: 0 6px 0 12px;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .ws {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  header button {
    background: none;
    border: 0;
    color: #7f848e;
    cursor: pointer;
    font-size: 11px;
  }
  header button:hover {
    color: #fff;
  }
  .tree {
    flex: 1;
    overflow: auto;
    padding-bottom: 12px;
  }
  main {
    position: relative;
    min-width: 0;
  }
  .welcome {
    display: grid;
    place-content: center;
    gap: 20px;
    height: 100vh;
    text-align: center;
  }
  h1 {
    margin: 0;
    font-weight: 500;
    color: #e6e6e6;
  }
  .welcome button {
    padding: 10px 18px;
    border: 1px solid #3b4048;
    border-radius: 6px;
    background: #282c34;
    color: #e6e6e6;
    font-size: 14px;
    cursor: pointer;
  }
  .welcome button:hover {
    border-color: #61afef;
  }
  kbd {
    margin-left: 8px;
    color: #7f848e;
    font-family: inherit;
  }
</style>
