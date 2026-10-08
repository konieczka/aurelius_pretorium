// Pre-paint theme (external file: the CSP blocks inline scripts). Mirrors src/lib/theme.svelte.ts.
try {
  var stored = localStorage.getItem("theme");
  var dark = stored === "dark" || ((!stored || stored === "system") && matchMedia("(prefers-color-scheme: dark)").matches);
  document.documentElement.classList.toggle("dark", dark);
} catch (e) {}
