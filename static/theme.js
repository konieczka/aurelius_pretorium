// Pre-paint theme (external file: the CSP blocks inline scripts). Mirrors src/lib/theme.svelte.ts.
try {
  var stored = localStorage.getItem("theme");
  var dark = stored === "dark" || ((!stored || stored === "system") && matchMedia("(prefers-color-scheme: dark)").matches);
  document.documentElement.classList.toggle("dark", dark);
  var palettes = ["aurelius", "mare", "tyrian", "verdigris", "marmor", "vesuvius"];
  var palette = localStorage.getItem("palette");
  if (palettes.indexOf(palette) === -1) palette = "aurelius";
  // Tauri injects the window label before this script. Read its active workspace
  // before the first paint, including when main takes over a saved secondary window.
  var internals = window.__TAURI_INTERNALS__;
  var label = internals && internals.metadata ? internals.metadata.currentWindow.label : "main";
  try {
    var saved = localStorage.getItem("win:" + label);
    if (!saved && label === "main") {
      var key = Object.keys(localStorage).find(function (k) { return k.indexOf("win:") === 0; });
      if (key) saved = localStorage.getItem(key);
    }
    var win = JSON.parse(saved || "null");
    var workspaces = JSON.parse(localStorage.getItem("workspaces") || "[]");
    var workspace = Array.isArray(workspaces) && win && workspaces.find(function (m) { return m && m.id === win.active; });
    if (workspace && palettes.indexOf(workspace.palette) !== -1) palette = workspace.palette;
  } catch (e) {} // A malformed workspace must still get the global theme.
  document.documentElement.dataset.palette = palette;
} catch (e) {}
