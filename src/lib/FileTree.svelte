<script module lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { popupMenu, sep } from "./contextMenu";
  import { app, clipboardFiles, clipFile, copyText, deleteSelected, finishCreate, finishRename, moveInto, openFile, paste, startCreate } from "./state.svelte";

  /** VS Code-style right-click menu for a tree entry (or the workspace root: no cut/copy/rename/delete). */
  export async function treeMenu(ev: MouseEvent, path: string, isDir: boolean) {
    ev.preventDefault();
    ev.stopPropagation();
    const dir = isDir ? path : path.slice(0, path.lastIndexOf("/"));
    const root = path === app.workspace;
    app.selectedDir = dir;
    app.selectedPath = root ? "" : path;
    const canPaste = (await clipboardFiles()).length > 0;
    const create = (kind: "file" | "dir") => () => ((app.selectedDir = dir), startCreate(kind));
    await popupMenu([
      { text: "New File…", action: create("file") },
      { text: "New Folder…", action: create("dir") },
      sep,
      { text: "Reveal in Finder", action: () => invoke("reveal_path", { path }).catch(() => {}) },
      sep,
      ...(root ? [] : [
        { text: "Cut", accelerator: "CmdOrCtrl+X", action: () => clipFile(path, true) },
        { text: "Copy", accelerator: "CmdOrCtrl+C", action: () => clipFile(path, false) },
      ]),
      { text: "Paste", accelerator: "CmdOrCtrl+V", enabled: canPaste, action: () => paste(dir) },
      sep,
      { text: "Copy Path", action: () => copyText(path) },
      { text: "Copy Relative Path", action: () => copyText(path.slice(app.workspace.length + 1) || ".") },
      ...(root ? [] : [
        sep,
        { text: "Rename…", action: () => (app.renaming = path) },
        { text: "Delete", accelerator: "CmdOrCtrl+Backspace", action: deleteSelected },
      ]),
    ]);
  }
</script>

<script lang="ts">
  import { ChevronRight, FilePlus, FolderPlus } from "@lucide/svelte";
  import FileTree from "./FileTree.svelte";
  import { git } from "./GitPanel.svelte";
  import { decoFor } from "./gitStatus";
  import { fileIcon, folderIcon } from "./fileIcons";

  type Entry = { name: string; is_dir: boolean };
  const markTitle = { M: "Modified", A: "Added", U: "Untracked", R: "Renamed", D: "Deleted", C: "Conflict" };
  let { path, depth = 0 }: { path: string; depth?: number } = $props();
  let entries = $state<Entry[]>([]);

  $effect(() => {
    void app.treeVersion; // refresh button bumps this
    invoke<Entry[]>("list_dir", { path }).then((e) => (entries = e), () => (entries = []));
  });

  // Tree drag and drop: drop on a folder moves into it, on a file moves next to it.
  const isTreeDrag = (ev: DragEvent) => ev.dataTransfer?.types.includes("text/x-path");
  function over(ev: DragEvent, dir: string) {
    if (!isTreeDrag(ev)) return;
    ev.preventDefault();
    ev.stopPropagation(); // innermost row wins over the sidebar's root drop zone
    app.dropDir = dir;
  }
  function drop(ev: DragEvent, dir: string) {
    const from = ev.dataTransfer?.getData("text/x-path");
    if (!from) return;
    ev.preventDefault();
    ev.stopPropagation();
    app.dropDir = "";
    moveInto(from, dir);
  }
</script>

{#snippet icon(name: string, isDir: boolean, open = false)}
  {#if isDir}
    {@const f = folderIcon(name)}
    {@const Glyph = open ? f.open : f.glyph}
    <Glyph size={15} strokeWidth={1.6} style="color: {f.color}" />
  {:else}
    {@const i = fileIcon(name)}
    {#if i.glyph}
      <i.glyph size={15} strokeWidth={1.6} style="color: {i.color}" />
    {:else}
      <span class="badge" class:long={i.badge.length > 2} style:--c={i.color}>{i.badge}</span>
    {/if}
  {/if}
{/snippet}

<div class="level" class:nested={depth > 0}>
  {#if app.creating?.dir === path}
    <div class="row creating">
      <span class="chev"></span>
      {#if app.creating.kind === "dir"}<FolderPlus size={14} strokeWidth={1.5} />{:else}<FilePlus size={14} strokeWidth={1.5} />{/if}
      <input
        {@attach (el) => el.focus()}
        placeholder={app.creating.kind === "dir" ? "Folder name" : "File name"}
        onkeydown={(e) => {
          if (e.key === "Enter") finishCreate(e.currentTarget.value);
          if (e.key === "Escape") app.creating = null;
        }}
        onblur={(e) => finishCreate(e.currentTarget.value)}
      />
    </div>
  {/if}
  {#each entries as e (e.name)}
    {@const full = `${path}/${e.name}`}
    {@const dir = e.is_dir ? full : path}
    {#if app.renaming === full}
      <div class="row creating active">
        <span class="chev"></span>
        {@render icon(e.name, e.is_dir)}
        <input
          value={e.name}
          {@attach (el) => {
            el.focus();
            const dot = e.is_dir ? -1 : e.name.lastIndexOf(".");
            el.setSelectionRange(0, dot > 0 ? dot : e.name.length); // the name without its extension, like VS Code
          }}
          onkeydown={(ev) => {
            if (ev.key === "Enter") finishRename(full, ev.currentTarget.value);
            if (ev.key === "Escape") app.renaming = "";
          }}
          onblur={(ev) => finishRename(full, ev.currentTarget.value)}
        />
      </div>
    {:else}
      {@const d = decoFor(git.deco, git.root, full, e.is_dir)}
      <button
        class="row"
        class:ignored={d.ignored}
        data-mark={d.mark}
        class:active={full === app.selectedPath}
        class:cut={app.cut === full}
        class:drop={e.is_dir && app.dropDir === full}
        title={full}
        draggable="true"
        ondragstart={(ev) => ev.dataTransfer?.setData("text/x-path", full)}
        ondragend={() => (app.dropDir = "")}
        ondragover={(ev) => over(ev, dir)}
        ondrop={(ev) => drop(ev, dir)}
        oncontextmenu={(ev) => treeMenu(ev, full, e.is_dir)}
        onclick={() => {
          app.selectedDir = dir;
          app.selectedPath = full;
          if (e.is_dir) app.expanded[full] = !app.expanded[full];
          else openFile(full);
        }}
      >
        <span class="chev" class:open={app.expanded[full]}>
          {#if e.is_dir}<ChevronRight size={12} strokeWidth={1.5} />{/if}
        </span>
        {@render icon(e.name, e.is_dir, app.expanded[full])}
        <span class="name">{e.name}</span>
        {#if d.mark && e.is_dir}<span class="git-mark dot" title="Contains changes"></span>
        {:else if d.mark}<span class="git-mark" title={markTitle[d.mark]}>{d.mark === "C" ? "!" : d.mark}</span>{/if}
      </button>
    {/if}
    {#if e.is_dir && app.expanded[full]}
      <FileTree path={full} depth={depth + 1} />
    {/if}
  {/each}
</div>

<style>
  .level.nested {
    margin-left: 13px;
    padding-left: 4px;
    border-left: 1px solid transparent;
    transition: border-color var(--dur-base) var(--ease-standard);
  }
  /* Indent guides show while the tree is hovered; the selected entry's guide stays lit. */
  :global(.tree:hover) .level.nested {
    border-left-color: color-mix(in oklch, var(--muted-foreground) 22%, transparent);
  }
  .level.nested:has(> .row.active) {
    border-left-color: color-mix(in oklch, var(--gold) 45%, transparent);
  }
  .row {
    position: relative;
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    height: 26px;
    padding: 0 8px 0 2px;
    border: 0;
    border-radius: var(--r-chip);
    background: none;
    color: var(--sidebar-foreground);
    font-size: 13px;
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
    transition: background-color var(--dur-quick) var(--ease-standard);
  }
  .row:hover {
    background: var(--hover);
  }
  /* Gold marks selection */
  .row.active {
    background: var(--gold-soft);
    box-shadow: inset 0 0 0 1px color-mix(in oklch, var(--gold) 30%, transparent);
  }
  .row.drop {
    background: var(--gold-soft);
    box-shadow: inset 0 0 0 1px var(--gold-ring);
  }
  .creating {
    cursor: default;
  }
  input {
    flex: 1;
    min-width: 0;
    height: 20px;
    padding: 0 6px;
    border: 1px solid var(--gold-ring);
    border-radius: var(--radius-sm);
    background: var(--background);
    color: var(--foreground);
    font: inherit;
    outline: none;
  }
  .row :global(svg) {
    flex: none;
    color: var(--muted-foreground);
  }
  .chev :global(svg) {
    opacity: 0.7;
  }
  .badge {
    display: grid;
    place-items: center;
    flex: none;
    width: 15px;
    height: 13px;
    border-radius: 3px;
    background: color-mix(in oklch, var(--c) 16%, transparent);
    color: var(--c);
    font: 700 7.5px/1 var(--font-sans);
    letter-spacing: -0.02em;
  }
  .badge.long {
    font-size: 6px;
    letter-spacing: -0.04em;
  }
  /* Git status: VS Code colors — modified amber, new green, conflicts red. */
  .git-mark {
    flex: none;
    width: 12px;
    font-size: 11px;
    font-weight: 600;
    text-align: center;
    color: var(--c);
  }
  .git-mark.dot::before {
    content: "";
    display: block;
    width: 5px;
    height: 5px;
    margin: auto;
    border-radius: 9999px;
    background: currentColor;
    opacity: 0.8;
  }
  .row[data-mark="M"],
  .row[data-mark="R"] {
    --c: var(--syn-number);
  }
  .row[data-mark="A"],
  .row[data-mark="U"] {
    --c: var(--diff-add-gutter);
  }
  .row[data-mark="C"],
  .row[data-mark="D"] {
    --c: var(--diff-del-gutter);
  }
  .row[data-mark] .name {
    color: var(--c);
  }
  .row.ignored .name,
  .row.ignored > :global(svg),
  .row.ignored .badge {
    opacity: 0.45;
  }
  .chev {
    display: grid;
    place-items: center;
    width: 12px;
    flex: none;
    transition: transform var(--dur-base) var(--ease-out-quint);
  }
  .chev.open {
    transform: rotate(90deg);
  }
  .name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .row.cut {
    opacity: 0.5;
  }
</style>
