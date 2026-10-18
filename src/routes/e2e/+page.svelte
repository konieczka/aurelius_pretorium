<script lang="ts">
  // The E2E tests' page (see e2e/README.md): the Servers panel and the panes, with the app's commands answered by the
  // e2e harness over HTTP instead of Tauri. Dev builds only: production renders nothing here.
  import { onMount, type Component } from "svelte";

  let Shell = $state<Component | null>(null);
  onMount(async () => {
    if (!import.meta.env.DEV) return;
    const harness = import.meta.env.VITE_E2E_HARNESS ?? "http://127.0.0.1:7811";
    const { mockIPC, mockWindows } = await import("@tauri-apps/api/mocks");
    mockWindows("main");
    // What the harness doesn't serve (terminals, dialogs) is recorded for the tests to read, and answers null.
    const calls: { cmd: string; args: unknown }[] = ((window as unknown as { __e2eCalls: unknown[] }).__e2eCalls = []) as never;
    const served = /^(infra_scan|infra_recipes|db_[a-z]+)$/;
    mockIPC(async (cmd, args) => {
      if (!served.test(cmd)) {
        calls.push({ cmd, args: cmd === "pty_write" ? args : null });
        return null;
      }
      const r = await fetch(`${harness}/invoke/${cmd}`, { method: "POST", body: JSON.stringify(args ?? {}) });
      const body = await r.json();
      if (!r.ok) throw body;
      return body;
    });
    // Only now: the app's modules may call Tauri as they load.
    Shell = (await import("./Shell.svelte")).default;
  });
</script>

{#if Shell}<Shell />{/if}
