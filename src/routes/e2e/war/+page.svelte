<script lang="ts">
  // The War Table's E2E page (see e2e/README.md): the whole app, on three workspaces with a Claude tab each, its
  // commands answered here instead of by Tauri. Agents' states come from `window.__e2eShells` (by tab id), which a
  // test changes. Starts afresh unless opened with ?keep. Dev builds only: production renders nothing here.
  import { onMount, type Component } from "svelte";

  const FOLDERS = { t1: "/e2e/orchard", t2: "/e2e/quarry", t3: "/e2e/harbor" };

  let App = $state<Component | null>(null);
  onMount(async () => {
    if (!import.meta.env.DEV) return;
    if (!new URLSearchParams(location.search).has("keep")) {
      localStorage.clear();
      const ws = ["One", "Two", "Three"].map((name, i) => ({ id: `w${i + 1}`, name }));
      localStorage.setItem("workspaces", JSON.stringify(ws));
      for (const [i, [tab, folder]] of Object.entries(FOLDERS).entries()) {
        const pane = { id: `p${i + 1}`, type: "pane", tabs: [{ id: tab, kind: "term", cwd: folder, agent: "claude" }], active: 0 };
        const root = { id: `r${i + 1}`, type: "split", dir: "row", children: [pane], sizes: [1] };
        localStorage.setItem(`ws:w${i + 1}`, JSON.stringify({ folder, root, focused: pane.id }));
      }
      localStorage.setItem("win:main", JSON.stringify({ open: ws.map((w) => w.id), active: "w1" }));
    }
    const shells = ((window as unknown as { __e2eShells: unknown }).__e2eShells = Object.fromEntries(
      Object.entries(FOLDERS).map(([tab, cwd]) => [tab, { agent: "claude", quiet: null, title: "", cwd, busy: false, attention: false, asking: null, since: 60_000 }]),
    ));
    const calls: { cmd: string; args: unknown }[] = ((window as unknown as { __e2eCalls: unknown[] }).__e2eCalls = []) as never;

    const { mockIPC, mockWindows } = await import("@tauri-apps/api/mocks");
    mockWindows("main");
    mockIPC((cmd, args) => {
      if (cmd === "pty_status") return structuredClone(shells);
      calls.push({ cmd, args });
      return null;
    });
    // Only now: the app's modules read the workspaces, and may call Tauri, as they load.
    App = (await import("../../+page.svelte")).default;
  });
</script>

{#if App}<App />{/if}
