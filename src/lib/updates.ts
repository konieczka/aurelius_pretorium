import { check } from "@tauri-apps/plugin-updater";
import { ask, message } from "@tauri-apps/plugin-dialog";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { restartApp } from "./workspaces.svelte";

// Updates come from the GitHub releases' latest.json (tauri.conf.json > plugins > updater); the updater checks the
// download's signature against the public key there before installing.

let checking = false;

/** Look for a newer release and offer to install it. `manual` (the menu item) also says when there's none, or why
 * the check failed; the background check stays quiet about both. */
export async function checkForUpdates(manual = false) {
  if (checking) return;
  checking = true;
  try {
    const update = await check();
    if (!update) {
      if (manual) await message("You're on the latest version of Pretorium.", { title: "No updates" });
      return;
    }
    const notes = update.body?.trim();
    const install = await ask(`Pretorium ${update.version} is available (you have ${update.currentVersion}).${notes ? `\n\n${notes}` : ""}`, {
      title: "Update available", okLabel: "Install", cancelLabel: "Later",
    });
    if (!install) return;
    await update.downloadAndInstall(); // replaces the app; it runs on from memory until restarted
    const now = await ask(`Pretorium ${update.version} is installed. Restart now to use it?`, {
      title: "Update installed", okLabel: "Restart", cancelLabel: "Later",
    });
    if (now) await restartApp();
  } catch (e) {
    if (manual) await message(`Couldn't check for updates:\n${e}`, { kind: "error" });
    else console.warn("update check failed", e);
  } finally {
    checking = false;
  }
}

/** "Check for Updates…" in the app menu (sent to one window), and, from the main window of a release build, a quiet
 * check soon after launch and twice a day after that. */
export function initUpdates() {
  const win = getCurrentWebviewWindow();
  win.listen("check-updates", () => checkForUpdates(true));
  if (win.label !== "main" || import.meta.env.DEV) return;
  setTimeout(() => checkForUpdates(), 15_000);
  setInterval(() => checkForUpdates(), 12 * 60 * 60 * 1000);
}
