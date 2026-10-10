// Pre-paint theme (external file: the CSP blocks inline scripts). Mirrors src/lib/theme.svelte.ts.
try {
  var stored = localStorage.getItem("theme");
  var dark = stored === "dark" || ((!stored || stored === "system") && matchMedia("(prefers-color-scheme: dark)").matches);
  document.documentElement.classList.toggle("dark", dark);
  var palette = localStorage.getItem("palette");
  if (palette && palette !== "aurelius") document.documentElement.dataset.palette = palette;
} catch (e) {}
