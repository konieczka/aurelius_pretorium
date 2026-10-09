// Files that open in a viewer instead of the text editor (formats WKWebView can decode).
const image = new Set(["png", "jpg", "jpeg", "gif", "webp", "avif", "bmp", "ico", "heic", "heif", "tif", "tiff"]);
const video = new Set(["mp4", "m4v", "mov", "webm"]);
const audio = new Set(["mp3", "wav", "m4a", "aac", "flac", "ogg", "oga", "opus", "aif", "aiff", "caf"]);
const model = new Set(["glb"]); // .gltf can reference side files by relative path, which asset URLs can't resolve

const ext = (path: string) => path.slice(path.lastIndexOf(".") + 1).toLowerCase();

export function mediaKind(path: string): "image" | "video" | "audio" | "model" | null {
  const e = ext(path);
  return image.has(e) ? "image" : video.has(e) ? "video" : audio.has(e) ? "audio" : model.has(e) ? "model" : null;
}

export const isSvg = (path: string) => ext(path) === "svg";
/** Text files with a rendered view next to the code (⌘E toggles). */
export const hasPreview = (path: string) => ["md", "markdown", "svg"].includes(ext(path));
