<script lang="ts">
  import "../app.css";
  import { open } from "@tauri-apps/plugin-dialog";
  import { ArrowDown, ArrowUp, ChevronsDownUp, ChevronsUpDown, FilePlus, Files, GitBranch, FolderOpen, FolderPlus, FolderRoot, RefreshCw } from "@lucide/svelte";
  import { zoom } from "$lib/zoom";
  import Node from "$lib/Node.svelte";
  import StatusBar from "$lib/StatusBar.svelte";
  import QuickPick, { pick } from "$lib/QuickPick.svelte";
  import FileTree, { treeMenu } from "$lib/FileTree.svelte";
  import { hasPreview } from "$lib/media";
  import WorkspaceBar from "$lib/WorkspaceBar.svelte";
  import { initWorkspaces, save } from "$lib/workspaces.svelte";
  import { watchAgents } from "$lib/agents.svelte";
  import GitPanel, { changeCount, git, pull, push, refreshGit, watchGit } from "$lib/GitPanel.svelte";
  import { saveEditor } from "$lib/Editor.svelte";
  import Usage from "$lib/Usage.svelte";
  import { closeUsage, toggleUsage, usage, warmUsage } from "$lib/usageView.svelte";
  import Skills from "$lib/Skills.svelte";
  import { closeSkills, saveSkill, skills, toggleSkillMode, toggleSkills } from "$lib/skillsView.svelte";
  import AgentSettings from "$lib/AgentSettings.svelte";
  import { agents, closeAgents, saveAgentFile, toggleAgents } from "$lib/agentsView.svelte";
  import Fleet from "$lib/Fleet.svelte";
  import { closeFleet, fleet, toggleFleet } from "$lib/fleetView.svelte";
  import {
    app, activeTab, closeTab, deleteSelected, clipFile, paste, moveInto, undoTree, newTerminal, newNote, openWorkspace, splitFocused, startCreate, toggleSidebar,
  } from "$lib/state.svelte";

  initWorkspaces();
  warmUsage();
  $effect(save);
  $effect(watchAgents);
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
    // Tree undo/delete/cut/copy/paste only when the sidebar has focus; otherwise ⌘Z belongs to the editor/terminal.
    if (app.treeFocused && app.sidebar === "files" && !(e.target as Element).closest?.("input, textarea") && (e.metaKey || e.ctrlKey) && !e.shiftKey && ["z", "x", "c", "v", "backspace"].includes(k)) {
      const p = app.selectedPath;
      const act =
        k === "z" ? undoTree :
        k === "backspace" ? deleteSelected :
        k === "v" ? () => paste(app.selectedDir || app.workspace) :
        p ? () => clipFile(p, k === "x") : null;
      if (!act) return;
      e.preventDefault();
      e.stopPropagation();
      return act();
    }
    if (!e.metaKey) return;
    const t = activeTab();
    const act =
      // ⌘= / ⌘- / ⌘0 are View menu shortcuts; ⌘+ (⇧=) doesn't match the menu's key equivalent.
      k === "+" ? () => zoom(0.1) :
      k === "o" ? pickFolder :
      k === "p" ? switchFolder :
      !app.workspace ? null :
      k === "e" && e.shiftKey ? () => (app.sidebar = "files") :
      k === "g" && e.shiftKey ? () => (app.sidebar = "git") :
      k === "b" && !e.shiftKey ? toggleSidebar :
      k === "u" && e.shiftKey ? toggleUsage :
      k === "w" && usage.open ? closeUsage :
      k === "k" && e.shiftKey ? toggleSkills :
      k === "w" && skills.open ? closeSkills :
      k === "s" && skills.open ? saveSkill :
      k === "e" && skills.open ? toggleSkillMode :
      k === "a" && e.shiftKey ? toggleAgents :
      k === "w" && agents.open ? closeAgents :
      k === "j" && e.shiftKey ? toggleFleet :
      k === "w" && fleet.open ? closeFleet :
      k === "s" && agents.open ? saveAgentFile :
      k === "t" ? newTerminal :
      k === "n" ? newNote :
      k === "d" ? () => splitFocused(e.shiftKey ? "col" : "row") :
      k === "w" ? () => t && closeTab(t.id) :
      k === "s" ? () => t?.kind === "file" && saveEditor(t.id) :
      k === "e" ? () => t?.kind === "file" && hasPreview(t.path) && (t.preview = !t.preview) :
      null;
    if (!act) return;
    e.preventDefault();
    e.stopPropagation();
    act();
  }
</script>

<svelte:window onkeydowncapture={onKey} />

{#if app.workspace}
  <div class="app" class:collapsed={!app.sidebarOpen}>
    <WorkspaceBar />
    <aside class="glass" inert={!app.sidebarOpen} onpointerdown={() => (app.treeFocused = true)}>
      <header>
        <button class="ws" onclick={switchFolder} title="{app.workspace} (⌘P to switch)">
          <FolderRoot size={14} strokeWidth={1.5} /><span class="name">{app.workspace.split("/").pop()}</span><ChevronsUpDown size={13} strokeWidth={1.5} />
        </button>
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
          oncontextmenu={(e) => treeMenu(e, app.workspace, true)}
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
    <!-- The Usage, Skills, Agents and Fleet views stand in for the panes; they stay mounted (terminals keep running) -->
    <main class:away={usage.open || skills.open || agents.open || fleet.open} onpointerdown={() => (app.treeFocused = false)}><Node bind:node={app.root} /></main>
    {#if usage.open}<Usage />{:else if skills.open}<Skills />{:else if agents.open}<AgentSettings />{:else if fleet.open}<Fleet />{/if}
    <StatusBar />
  </div>
  <QuickPick />
{:else}
  <div class="welcome">
    <span class="mark big"></span>
    <h1>Pretorium</h1>
    <p>An Aurelius workspace</p>
    <button class="open" onclick={pickFolder}><FolderOpen size={16} strokeWidth={1.5} /> Open Folder <kbd>⌘O</kbd></button>
  </div>
{/if}

<style>
  /* Planes: the bar and status line sit on the field; the sidebar island and the work slabs float on it, a gutter apart */
  .app {
    display: grid;
    grid-template-columns: 248px 1fr;
    grid-template-rows: auto minmax(0, 1fr) auto;
    column-gap: var(--gap);
    box-sizing: border-box;
    height: 100vh;
    padding: 0 var(--gap);
    transition: grid-template-columns var(--dur-base) var(--ease-standard), column-gap var(--dur-base) var(--ease-standard);
  }
  /* The column animates to 0; the sidebar keeps its 248px inside so its contents don't reflow while sliding */
  .app.collapsed {
    grid-template-columns: 0 1fr;
    column-gap: 0;
  }
  aside {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    border-radius: var(--r-island);
    transition: opacity var(--dur-base) var(--ease-standard), visibility var(--dur-base);
  }
  aside > * {
    box-sizing: border-box;
    width: 248px;
    flex-shrink: 0;
  }
  .app.collapsed aside {
    opacity: 0;
    visibility: hidden;
  }
  header {
    display: flex;
    align-items: center;
    flex: none;
    padding: 10px 10px 6px;
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 2px;
    flex: none;
    padding: 0 10px 6px;
  }
  .spacer {
    flex: 1;
  }
  /* Segmented control: a recessed track, the active tool a raised chip with a gold tick */
  .modes {
    display: flex;
    gap: 2px;
    padding: 2px;
    border-radius: var(--r-control);
    background: oklch(var(--shadow-tint) / 0.07);
    box-shadow: inset 0 1px 2px oklch(var(--shadow-tint) / 0.1);
  }
  :global(.dark) .modes {
    background: oklch(var(--shadow-tint) / 0.3);
    box-shadow: inset 0 1px 2px oklch(var(--shadow-tint) / 0.4);
  }
  .mode {
    position: relative;
    width: 34px;
  }
  .mode.on {
    background: var(--chip-on);
    box-shadow: var(--sh-chip);
    color: var(--foreground);
  }
  .mode.on::after {
    content: "";
    position: absolute;
    left: 50%;
    bottom: 3px;
    width: 10px;
    height: 2px;
    margin-left: -5px;
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
    display: flex;
    align-items: center;
    gap: 6px;
    flex: 1;
    min-width: 0;
    height: 34px;
    padding: 0 10px;
    border: 0;
    border-radius: 12px;
    background: var(--hover);
    box-shadow: inset 0 1px 0 var(--hl);
    color: inherit;
    cursor: pointer;
    font-size: 13px;
    font-weight: 500;
    transition: background-color var(--dur-quick) var(--ease-standard);
  }
  .ws:hover {
    background: var(--accent);
  }
  .ws > :global(svg:last-child) {
    margin-left: auto;
  }
  .ws > :global(svg) {
    flex: none;
    color: var(--muted-foreground);
  }
  .ws .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tree {
    flex: 1;
    overflow: auto;
    padding: 2px 8px 12px;
  }
  .tree.drop {
    background: var(--gold-soft);
    box-shadow: inset 0 0 0 1px var(--gold-ring);
  }
  main {
    position: relative;
    min-width: 0;
    min-height: 0;
  }
  main.away {
    display: none;
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
    border-radius: 12px;
    background: var(--chrome);
    -webkit-backdrop-filter: var(--blur-chrome);
    backdrop-filter: var(--blur-chrome);
    box-shadow: var(--sh-chrome);
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
