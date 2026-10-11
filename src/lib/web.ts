// Browser tabs: what's typed in the address bar, and what the tab is called.

const LOCAL = /^(localhost|127(\.\d+){3}|0\.0\.0\.0|\[::1\]|10(\.\d+){3}|192\.168(\.\d+){2}|[\w-]+\.local)(:\d+)?([/?#]|$)/i;

/** The address to open for what was typed: a URL as is, a host or local server with the scheme added, anything else
 * searched for. null for nothing typed. */
export function toUrl(text: string): string | null {
  const s = text.trim();
  if (!s) return null;
  if (/^[a-z][a-z\d+.-]*:\/\//i.test(s) || /^about:/i.test(s)) return s;
  if (LOCAL.test(s)) return "http://" + s; // dev servers rarely speak https
  if (!/\s/.test(s) && /^[^/?#]+\.[a-z]{2,}(:\d+)?([/?#]|$)/i.test(s)) return "https://" + s;
  return "https://www.google.com/search?q=" + encodeURIComponent(s);
}

/** Short name for a page without a title yet: its host. */
export function hostOf(url: string): string {
  try {
    return new URL(url).host.replace(/^www\./, "") || url;
  } catch {
    return url;
  }
}
