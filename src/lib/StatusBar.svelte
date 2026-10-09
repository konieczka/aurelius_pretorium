<script lang="ts">
  import { ArrowDown, ArrowUp, CloudUpload, FolderGit2, GitBranch, LoaderCircle, RefreshCw, SquareTerminal } from "@lucide/svelte";
  import { popupMenu } from "./contextMenu";
  import { branchName, git, noHead, publish, switchBranch, switchWorktree, sync } from "./GitPanel.svelte";
  import { app, closeTab, restoreTab } from "./state.svelte";
  import { activity, agentProvider } from "./agents.svelte";
  import ProviderIcon from "./ProviderIcon.svelte";
  import { tabLabel } from "./Pane.svelte";
  import type { Tab } from "./layout";

  async function minMenu(e: MouseEvent, t: Tab) {
    e.preventDefault();
    await popupMenu([
      { text: "Restore", action: () => restoreTab(t.id) },
      { text: "Close", action: () => closeTab(t.id) },
    ]);
  }

  const s = $derived(git.status);
  const folder = $derived(git.root.slice(git.root.lastIndexOf("/") + 1));
</script>

<footer>
  {#if git.root && s}
    <button onclick={switchWorktree} title="Worktree: {git.root} (click to switch or create)">
      <FolderGit2 size={13} strokeWidth={1.5} />{folder}
    </button>
    <button onclick={switchBranch} title="Branch{s.upstream ? ` (tracking ${s.upstream})` : ''} (click to switch or create)">
      <GitBranch size={13} strokeWidth={1.5} />{branchName()}{#if s.files.length}*{/if}{#if git.op}<span class="op">{git.op} in progress</span>{/if}
    </button>
    {#if git.busy}
      <span class="item"><LoaderCircle class="spin" size={13} strokeWidth={1.5} />{git.busy}</span>
    {:else if git.op}
      <!-- no sync while a merge/rebase is stopped -->
    {:else if s.upstream}
      <button onclick={sync} title="Sync with {s.upstream} (pull, then push)">
        <RefreshCw size={13} strokeWidth={1.5} />
        {#if s.behind || s.ahead}
          <span class="ab">{s.behind}<ArrowDown size={11} strokeWidth={1.75} /> {s.ahead}<ArrowUp size={11} strokeWidth={1.75} /></span>
        {/if}
      </button>
    {:else if !noHead() && s.head !== "(detached)"}
      <button onclick={publish} title="Publish branch (git push -u origin HEAD)"><CloudUpload size={13} strokeWidth={1.5} />Publish</button>
    {/if}
  {/if}
  <!-- Minimized terminals: click to put one back where it was -->
  <span class="spacer"></span>
  {#each app.minimized as { tab: t } (t.id)}
    <button class="min" title="Restore {tabLabel(t)}" onclick={() => restoreTab(t.id)} oncontextmenu={(e) => minMenu(e, t)} onauxclick={(e) => e.button === 1 && closeTab(t.id)}>
      {#if t.kind === "term" && t.agent && agentProvider[t.agent]}
        <span class="agent" class:working={activity[t.id] === "busy"}><ProviderIcon provider={agentProvider[t.agent]} size={12} /></span>
      {:else}<SquareTerminal size={12} strokeWidth={1.5} />{/if}
      <span class="name">{tabLabel(t)}</span>
      {#if activity[t.id] === "attention"}<span class="attention" title="Needs your attention"></span>{/if}
    </button>
  {/each}
</footer>

<style>
  footer {
    grid-column: 1 / -1;
    display: flex;
    align-items: stretch;
    gap: 2px;
    height: 24px;
    padding: 0 6px;
    background: var(--sidebar);
    border-top: 1px solid var(--border-strong);
    font-size: 12px;
    color: var(--muted-foreground);
  }
  button,
  .item {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 0 7px;
    border: 0;
    background: none;
    color: inherit;
    font-size: 12px;
    white-space: nowrap;
  }
  button {
    cursor: pointer;
    transition: background-color var(--dur-quick) var(--ease-standard), color var(--dur-quick) var(--ease-standard);
  }
  button:hover {
    background: var(--accent);
    color: var(--foreground);
  }
  .spacer {
    flex: 1;
  }
  .min {
    max-width: 180px;
  }
  .min .name {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .agent {
    display: flex;
  }
  .agent.working :global(svg) {
    animation: breathe 2.4s ease-in-out infinite;
  }
  @keyframes breathe {
    0%,
    100% {
      opacity: 0.55;
    }
    50% {
      opacity: 1;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .agent.working :global(svg) {
      animation: none;
    }
  }
  .attention {
    flex: none;
    width: 6px;
    height: 6px;
    border-radius: 9999px;
    background: oklch(0.7 0.17 50);
    box-shadow: 0 0 0 2px oklch(0.7 0.17 50 / 25%);
  }
  .op {
    margin-left: 4px;
    color: var(--gold-ring);
  }
  .ab {
    display: inline-flex;
    align-items: center;
    font-variant-numeric: tabular-nums;
  }
  :global(.spin) {
    animation: spin 1s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
