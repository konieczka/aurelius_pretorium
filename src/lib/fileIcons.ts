// File tree icon theme, VS Code style: language files get a colored letter badge, everything else a tinted glyph.
// Colors are --fi-* variables (app.css) so they work in both themes.
import {
  Atom, Braces, CodeXml, Container, Database, File, FileArchive, FileCog, FileText, Film, Folder, FolderArchive,
  FolderCheck, FolderCode, FolderCog, FolderGit2, FolderKey, FolderOpen, FolderOutput, FolderPen, FolderRoot, FolderTree,
  GitBranch, Hammer, Hash, Image, Info, KeyRound, Lock, Music, Package, Scale, ScrollText, Settings, Sheet, Sparkles,
  SquareTerminal, Type, Zap, FolderHeart, FolderKanban, FolderInput, Sigma, Box, PenTool,
} from "@lucide/svelte";

type Glyph = typeof File;
export type FileIcon = { glyph: Glyph; color: string; badge?: undefined } | { badge: string; color: string; glyph?: undefined };

const c = (name: string) => `var(--fi-${name})`;
const g = (glyph: Glyph, color: string): FileIcon => ({ glyph, color: c(color) });
const b = (badge: string, color: string): FileIcon => ({ badge, color: c(color) });

const byExt: Record<string, FileIcon> = {
  ts: b("TS", "blue"), mts: b("TS", "blue"), cts: b("TS", "blue"),
  js: b("JS", "yellow"), mjs: b("JS", "yellow"), cjs: b("JS", "yellow"),
  tsx: g(Atom, "sky"), jsx: g(Atom, "sky"),
  svelte: b("S", "orange"), vue: b("V", "green"), astro: b("A", "orange"),
  rs: b("RS", "rust"), go: b("GO", "sky"), py: b("PY", "blue"), rb: b("RB", "red"), php: b("PHP", "violet"),
  java: b("JV", "red"), kt: b("KT", "violet"), kts: b("KT", "violet"), swift: b("SW", "orange"), dart: b("DT", "sky"),
  c: b("C", "blue"), h: b("H", "violet"), cpp: b("C++", "blue"), cc: b("C++", "blue"), hpp: b("H", "violet"),
  cs: b("C#", "green"), m: b("OC", "blue"), zig: b("ZG", "amber"), lua: b("LUA", "blue"), ex: b("EX", "violet"),
  exs: b("EX", "violet"), erl: b("ER", "red"), hs: b("HS", "violet"), ml: b("ML", "orange"), scala: b("SC", "red"),
  clj: b("CLJ", "green"), r: b("R", "blue"), jl: b("JL", "violet"), nim: b("NIM", "amber"), sol: b("SOL", "gray"),
  wasm: b("WA", "violet"), graphql: b("GQL", "pink"), gql: b("GQL", "pink"), proto: b("PB", "teal"),
  json: g(Braces, "yellow"), jsonc: g(Braces, "yellow"), json5: g(Braces, "yellow"),
  md: b("M↓", "sky"), mdx: b("M↓", "amber"), markdown: b("M↓", "sky"), rst: g(FileText, "gray"), tex: g(Sigma, "teal"),
  html: g(CodeXml, "orange"), htm: g(CodeXml, "orange"), xml: g(CodeXml, "amber"), plist: g(CodeXml, "gray"),
  css: g(Hash, "blue"), scss: g(Hash, "pink"), sass: g(Hash, "pink"), less: g(Hash, "violet"), styl: g(Hash, "green"),
  yaml: g(Settings, "red"), yml: g(Settings, "red"), toml: g(Settings, "rust"), ini: g(Settings, "gray"),
  conf: g(Settings, "gray"), cfg: g(Settings, "gray"), properties: g(Settings, "gray"),
  env: g(KeyRound, "yellow"), pem: g(KeyRound, "amber"), key: g(KeyRound, "amber"), crt: g(KeyRound, "amber"),
  lock: g(Lock, "gray"), lockb: g(Lock, "gray"),
  sh: g(SquareTerminal, "green"), bash: g(SquareTerminal, "green"), zsh: g(SquareTerminal, "green"),
  fish: g(SquareTerminal, "green"), ps1: g(SquareTerminal, "blue"), bat: g(SquareTerminal, "gray"),
  sql: g(Database, "pink"), db: g(Database, "pink"), sqlite: g(Database, "pink"), prisma: g(Database, "teal"),
  csv: g(Sheet, "green"), tsv: g(Sheet, "green"), xlsx: g(Sheet, "green"), xls: g(Sheet, "green"),
  pdf: g(FileText, "red"), txt: g(FileText, "gray"), log: g(ScrollText, "gray"), doc: g(FileText, "blue"), docx: g(FileText, "blue"),
  png: g(Image, "violet"), jpg: g(Image, "violet"), jpeg: g(Image, "violet"), gif: g(Image, "violet"),
  webp: g(Image, "violet"), avif: g(Image, "violet"), bmp: g(Image, "violet"), ico: g(Image, "violet"),
  icns: g(Image, "violet"), svg: g(Image, "amber"), heic: g(Image, "violet"),
  excalidraw: g(PenTool, "violet"),
  mp4: g(Film, "pink"), mov: g(Film, "pink"), webm: g(Film, "pink"), mkv: g(Film, "pink"),
  mp3: g(Music, "pink"), wav: g(Music, "pink"), ogg: g(Music, "pink"), flac: g(Music, "pink"), m4a: g(Music, "pink"),
  glb: g(Box, "teal"), gltf: g(Box, "teal"), obj: g(Box, "gray"), fbx: g(Box, "gray"), usdz: g(Box, "sky"),
  ttf: g(Type, "red"), otf: g(Type, "red"), woff: g(Type, "red"), woff2: g(Type, "red"),
  zip: g(FileArchive, "amber"), tar: g(FileArchive, "amber"), gz: g(FileArchive, "amber"), tgz: g(FileArchive, "amber"),
  "7z": g(FileArchive, "amber"), rar: g(FileArchive, "amber"), dmg: g(FileArchive, "gray"), xz: g(FileArchive, "amber"),
};

// Exact names (lowercase) win over extensions.
const byName: Record<string, FileIcon> = {
  "package.json": g(Package, "red"), "package-lock.json": g(Lock, "gray"), "bun.lock": g(Lock, "gray"),
  "bun.lockb": g(Lock, "gray"), "yarn.lock": g(Lock, "gray"), "pnpm-lock.yaml": g(Lock, "gray"),
  "cargo.toml": g(Package, "rust"), "cargo.lock": g(Lock, "gray"), "go.mod": g(Package, "sky"), "go.sum": g(Lock, "gray"),
  "pyproject.toml": g(Package, "blue"), "requirements.txt": g(Package, "blue"), gemfile: g(Package, "red"),
  "tsconfig.json": g(FileCog, "blue"), "jsconfig.json": g(FileCog, "yellow"), "deno.json": g(FileCog, "gray"),
  dockerfile: g(Container, "sky"), "docker-compose.yml": g(Container, "sky"), "docker-compose.yaml": g(Container, "sky"),
  "compose.yml": g(Container, "sky"), "compose.yaml": g(Container, "sky"), ".dockerignore": g(Container, "gray"),
  ".gitignore": g(GitBranch, "rust"), ".gitattributes": g(GitBranch, "rust"), ".gitmodules": g(GitBranch, "rust"),
  ".gitkeep": g(GitBranch, "gray"),
  makefile: g(Hammer, "rust"), justfile: g(Hammer, "amber"), "cmakelists.txt": g(Hammer, "blue"),
  license: g(Scale, "amber"), "license.md": g(Scale, "amber"), "license.txt": g(Scale, "amber"),
  "readme.md": g(Info, "sky"), readme: g(Info, "sky"), "changelog.md": g(ScrollText, "green"),
  "claude.md": g(Sparkles, "gold"), "agents.md": g(Sparkles, "gold"),
  ".env": g(KeyRound, "yellow"), ".npmrc": g(Settings, "red"), ".editorconfig": g(Settings, "gray"),
  ".prettierrc": g(Settings, "pink"), ".eslintrc": g(Settings, "violet"), ".nvmrc": g(Settings, "green"),
  ".ds_store": g(File, "gray"),
};

// Config files named <tool>.config.<ext>.
const byConfig: Record<string, FileIcon> = {
  vite: g(Zap, "violet"), vitest: g(Zap, "green"), svelte: g(FileCog, "orange"), tailwind: g(FileCog, "sky"),
  next: g(FileCog, "gray"), eslint: g(FileCog, "violet"), prettier: g(FileCog, "pink"), postcss: g(FileCog, "red"),
  webpack: g(FileCog, "sky"), rollup: g(FileCog, "red"), jest: g(FileCog, "red"), playwright: g(FileCog, "green"),
  astro: g(FileCog, "orange"), nuxt: g(FileCog, "green"), drizzle: g(FileCog, "green"),
};

const plain = g(File, "gray");

export function fileIcon(name: string): FileIcon {
  const n = name.toLowerCase();
  if (byName[n]) return byName[n];
  if (n.startsWith(".env.")) return byName[".env"];
  if (n.startsWith("readme.")) return byName.readme;
  if (n.startsWith("dockerfile")) return byName.dockerfile;
  const cfg = n.match(/^([\w-]+)\.config\.\w+$/);
  if (cfg && byConfig[cfg[1]]) return byConfig[cfg[1]];
  if (n.endsWith(".d.ts")) return b("TS", "gray");
  const dot = n.lastIndexOf(".");
  return (dot >= 0 && byExt[n.slice(dot + 1)]) || plain;
}

// Folders: [closed, open] glyphs and a color. Special ones keep their glyph when open (the chevron shows that).
type FolderIcon = { glyph: Glyph; open: Glyph; color: string };
const f = (glyph: Glyph, color: string): FolderIcon => ({ glyph, open: glyph, color: c(color) });
const generic: FolderIcon = { glyph: Folder, open: FolderOpen, color: c("folder") };

const folders: Record<string, FolderIcon> = {
  src: f(FolderCode, "blue"), lib: f(FolderCode, "teal"), app: f(FolderCode, "blue"), source: f(FolderCode, "blue"),
  "src-tauri": f(FolderCode, "amber"), packages: f(FolderCode, "violet"), crates: f(FolderCode, "rust"),
  components: f(FolderKanban, "violet"), ui: f(FolderKanban, "violet"), widgets: f(FolderKanban, "violet"),
  routes: f(FolderTree, "green"), pages: f(FolderTree, "green"), api: f(FolderTree, "sky"), views: f(FolderTree, "green"),
  test: f(FolderCheck, "green"), tests: f(FolderCheck, "green"), __tests__: f(FolderCheck, "green"),
  spec: f(FolderCheck, "green"), e2e: f(FolderCheck, "green"), fixtures: f(FolderCheck, "teal"),
  node_modules: f(FolderArchive, "green"), vendor: f(FolderArchive, "gray"), ".venv": f(FolderArchive, "blue"),
  venv: f(FolderArchive, "blue"), target: f(FolderOutput, "rust"),
  dist: f(FolderOutput, "amber"), build: f(FolderOutput, "amber"), out: f(FolderOutput, "amber"),
  ".next": f(FolderOutput, "gray"), ".svelte-kit": f(FolderOutput, "orange"), gen: f(FolderOutput, "gray"),
  public: f(FolderHeart, "sky"), static: f(FolderHeart, "sky"), assets: f(FolderHeart, "pink"),
  images: f(FolderHeart, "violet"), img: f(FolderHeart, "violet"), icons: f(FolderHeart, "violet"), fonts: f(FolderHeart, "red"),
  docs: f(FolderPen, "sky"), doc: f(FolderPen, "sky"),
  config: f(FolderCog, "gray"), ".config": f(FolderCog, "gray"), ".vscode": f(FolderCog, "blue"),
  ".idea": f(FolderCog, "gray"), ".claude": f(FolderCog, "gold"), capabilities: f(FolderKey, "amber"),
  scripts: f(FolderInput, "green"), bin: f(FolderInput, "green"), tools: f(FolderInput, "gray"),
  ".git": f(FolderGit2, "rust"), ".github": f(FolderGit2, "gray"), ".husky": f(FolderGit2, "gray"),
  keys: f(FolderKey, "amber"), secrets: f(FolderKey, "amber"), certs: f(FolderKey, "amber"),
  migrations: f(FolderRoot, "pink"),
};

export const folderIcon = (name: string): FolderIcon => folders[name.toLowerCase()] ?? generic;
