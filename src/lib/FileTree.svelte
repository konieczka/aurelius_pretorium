<script module lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { Menu } from "@tauri-apps/api/menu";
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
    const sep = { item: "Separator" } as const;
    const create = (kind: "file" | "dir") => () => ((app.selectedDir = dir), startCreate(kind));
    const menu = await Menu.new({
      items: [
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
      ],
    });
    await menu.popup();
  }
</script>

<script lang="ts">
  import { ChevronRight, File, FilePlus, Folder, FolderOpen, FolderPlus } from "@lucide/svelte";
  import FileTree from "./FileTree.svelte";

  type Entry = { name: string; is_dir: boolean };
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
        {#if e.is_dir}<Folder size={14} strokeWidth={1.5} />{:else}<File size={14} strokeWidth={1.5} />{/if}
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
      <button
        class="row"
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
        {#if e.is_dir}
          {#if app.expanded[full]}<FolderOpen size={14} strokeWidth={1.5} />{:else}<Folder size={14} strokeWidth={1.5} />{/if}
        {:else}
          <File size={14} strokeWidth={1.5} />
        {/if}
        <span class="name">{e.name}</span>
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
    border-left: 1px solid var(--sidebar-border);
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
    border-radius: var(--radius-sm);
    background: none;
    color: var(--sidebar-foreground);
    font-size: 13px;
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
    transition: background-color var(--dur-quick) var(--ease-standard);
  }
  .row:hover {
    background: color-mix(in oklch, var(--sidebar-accent) 60%, transparent);
  }
  .row.active {
    background: var(--sidebar-accent);
  }
  .row.drop {
    background: var(--gold-soft);
    box-shadow: inset 0 0 0 1px var(--gold-ring);
  }
  .row.active::before {
    content: "";
    position: absolute;
    left: 0;
    top: 6px;
    bottom: 6px;
    width: 2px;
    border-radius: 9999px;
    background: var(--gold);
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
