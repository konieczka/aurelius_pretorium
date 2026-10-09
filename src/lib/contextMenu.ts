import { Menu, MenuItem, Submenu } from "@tauri-apps/api/menu";

export type MenuEntry =
  | { item: "Separator" }
  | { text: string; items: MenuEntry[] }
  | { text: string; action: () => void; enabled?: boolean; accelerator?: string };

export const sep: MenuEntry = { item: "Separator" };

// Items written inline in Menu.new({ items }) lose their actions: Tauri builds them as temporaries, and dropping one
// unregisters its handler, so clicking does nothing. Items made with MenuItem.new / Submenu.new are held in the
// resource table instead. The last menu's resources are freed when the next one opens (its action may still be in flight).
let live: { close(): Promise<void> }[] = [];

/** Native right-click menu at the cursor. */
export async function popupMenu(entries: MenuEntry[]) {
  for (const r of live.splice(0)) r.close().catch(() => {});
  const build = (list: MenuEntry[]): Promise<(MenuItem | Submenu | { item: "Separator" })[]> =>
    Promise.all(
      list.map(async (e) => {
        if ("item" in e) return e;
        const r = "items" in e ? await Submenu.new({ text: e.text, items: await build(e.items) }) : await MenuItem.new(e);
        live.push(r);
        return r;
      }),
    );
  const menu = await Menu.new({ items: await build(entries) });
  live.push(menu);
  await menu.popup();
}
