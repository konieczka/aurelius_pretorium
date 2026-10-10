import { marked } from "marked";
import DOMPurify from "dompurify";

// Previewed markdown comes from any repo (READMEs, skills) and is shown in a window that can run shell commands
// through the backend: sanitizing is mandatory, and nothing in it may navigate, submit or embed anything.
const FORBID_TAGS = ["form", "button", "input", "select", "textarea", "iframe", "frame", "object", "embed", "base", "meta", "link", "style", "area", "map"];
const FORBID_ATTR = ["style", "action", "formaction", "target"];

export const renderMarkdown = (text: string) => DOMPurify.sanitize(marked.parse(text, { async: false }), { FORBID_TAGS, FORBID_ATTR });

/** Click handler for a preview: links don't navigate (the window only ever shows the app). */
export const blockLinks = (ev: MouseEvent) => (ev.target as Element).closest("a, area") && ev.preventDefault();
