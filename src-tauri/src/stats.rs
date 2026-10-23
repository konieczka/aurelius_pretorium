//! Project stats for the War Table: lines by language (or file format), files, and commits in the last week.
//! Files are the repo's (`git ls-files`: tracked plus untracked-but-not-ignored), else a walk that skips the usual
//! dependency and build folders. Lock files, minified files, big files (generated) and binaries aren't counted.

use serde::Serialize;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::{blocking, clean_env, login_path, Res};

#[derive(Serialize, Debug, PartialEq)]
pub struct Lang {
    name: String,
    lines: u64,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    lines: u64,
    files: u64,
    langs: Vec<Lang>, // most lines first
    commits7d: u64,
}

const MAX_FILE: u64 = 1 << 20; // bigger than 1 MB: generated or data, not code
const MAX_FILES: usize = 50_000;
const SKIP_DIRS: &[&str] = &["node_modules", "target", "dist", "build", "out", "vendor", "__pycache__", "venv", ".venv", "Pods", "DerivedData"];

/// Extension (or exact lowercase file name) -> language. Anything else that's text counts as "Other".
fn language(path: &Path) -> Option<&'static str> {
    let name = path.file_name()?.to_str()?.to_ascii_lowercase();
    if name.ends_with(".lock") || name.ends_with(".lockb") || name.ends_with(".min.js") || name.ends_with(".min.css") || name.ends_with(".map")
        || ["package-lock.json", "go.sum", "pnpm-lock.yaml", "npm-shrinkwrap.json"].contains(&name.as_str())
    {
        return None;
    }
    match name.as_str() {
        "dockerfile" => return Some("Dockerfile"),
        "makefile" | "justfile" => return Some("Makefile"),
        _ => {}
    }
    let ext = name.rsplit_once('.')?.1;
    Some(match ext {
        "ts" | "mts" | "cts" => "TypeScript",
        "tsx" => "TSX",
        "js" | "mjs" | "cjs" => "JavaScript",
        "jsx" => "JSX",
        "svelte" => "Svelte",
        "vue" => "Vue",
        "astro" => "Astro",
        "rs" => "Rust",
        "go" => "Go",
        "py" | "pyi" => "Python",
        "rb" => "Ruby",
        "php" => "PHP",
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "swift" => "Swift",
        "dart" => "Dart",
        "c" | "h" => "C",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" => "C++",
        "m" | "mm" => "Objective-C",
        "cs" => "C#",
        "zig" => "Zig",
        "lua" => "Lua",
        "ex" | "exs" => "Elixir",
        "hs" => "Haskell",
        "scala" => "Scala",
        "clj" | "cljs" => "Clojure",
        "sql" => "SQL",
        "sh" | "bash" | "zsh" | "fish" => "Shell",
        "html" | "htm" => "HTML",
        "css" => "CSS",
        "scss" | "sass" | "less" => "SCSS",
        "md" | "markdown" => "Markdown",
        "mdx" => "MDX",
        "json" | "jsonc" | "json5" => "JSON",
        "yaml" | "yml" => "YAML",
        "toml" => "TOML",
        "xml" | "plist" => "XML",
        "graphql" | "gql" => "GraphQL",
        "proto" => "Protobuf",
        "txt" | "rst" | "ini" | "cfg" | "conf" | "env" => "Other",
        _ => return None, // images, fonts, archives, and the unknown
    })
}

fn git(root: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new("git")
        .args(["-c", "core.fsmonitor=false"])
        .args(args)
        .current_dir(root)
        .env_clear()
        .envs(clean_env())
        .env("PATH", login_path())
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status.success().then_some(out.stdout)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        if out.len() >= MAX_FILES {
            return;
        }
        let name = e.file_name();
        let name = name.to_string_lossy();
        match e.file_type() {
            Ok(t) if t.is_dir() && !name.starts_with('.') && !SKIP_DIRS.contains(&name.as_ref()) => walk(&e.path(), out),
            Ok(t) if t.is_file() => out.push(e.path()),
            _ => {}
        }
    }
}

/// Lines in a text file; None for a binary (a NUL in its first 8 KB) or one that can't be read.
fn lines_of(path: &Path) -> Option<u64> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return None;
    }
    let mut buf = Vec::with_capacity(meta.len() as usize);
    std::fs::File::open(path).ok()?.read_to_end(&mut buf).ok()?;
    if buf[..buf.len().min(8192)].contains(&0) {
        return None;
    }
    let n = buf.iter().filter(|&&b| b == b'\n').count() as u64;
    Some(n + u64::from(buf.last().is_some_and(|&b| b != b'\n')))
}

fn tally(files: impl IntoIterator<Item = PathBuf>) -> (u64, u64, Vec<Lang>) {
    let mut by: HashMap<&'static str, u64> = HashMap::new();
    let (mut lines, mut count) = (0, 0);
    for f in files {
        let Some(lang) = language(&f) else { continue };
        let Some(n) = lines_of(&f) else { continue };
        *by.entry(lang).or_default() += n;
        lines += n;
        count += 1;
    }
    let mut langs: Vec<Lang> = by.into_iter().filter(|(_, n)| *n > 0).map(|(name, lines)| Lang { name: name.into(), lines }).collect();
    langs.sort_by(|a, b| b.lines.cmp(&a.lines).then(a.name.cmp(&b.name)));
    (lines, count, langs)
}

fn stats(root: &Path) -> Stats {
    let listed = git(root, &["ls-files", "-z", "--cached", "--others", "--exclude-standard"]);
    let files: Vec<PathBuf> = match &listed {
        Some(out) => out.split(|&b| b == 0).filter(|p| !p.is_empty()).take(MAX_FILES).map(|p| root.join(String::from_utf8_lossy(p).as_ref())).collect(),
        None => {
            let mut v = Vec::new();
            walk(root, &mut v);
            v
        }
    };
    let (lines, files, langs) = tally(files);
    let commits7d = git(root, &["rev-list", "--count", "--since=7.days.ago", "HEAD"])
        .and_then(|o| String::from_utf8_lossy(&o).trim().parse().ok())
        .unwrap_or(0);
    Stats { lines, files, langs, commits7d }
}

/// Stats of the project at `root` (a checkout's top level, or any folder).
#[tauri::command]
pub async fn project_stats(root: String) -> Res<Stats> {
    blocking(move || Ok(stats(Path::new(&root)))).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_lines_by_language_and_skips_what_isnt_code() {
        let dir = std::env::temp_dir().join(format!("pretorium-stats-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/a.ts"), "let a = 1;\nlet b = 2;\n").unwrap();
        std::fs::write(dir.join("src/b.ts"), "export {}").unwrap(); // no trailing newline: still a line
        std::fs::write(dir.join("README.md"), "# x\n\nhi\n").unwrap();
        std::fs::write(dir.join("bun.lock"), "lots\nof\nlines\n").unwrap();
        std::fs::write(dir.join("logo.png"), [0x89, b'P', b'N', b'G', 0, 1, 2]).unwrap();
        std::fs::write(dir.join("blob.json"), [b'{', 0, b'}']).unwrap(); // binary despite the name
        let (lines, files, langs) = tally(["src/a.ts", "src/b.ts", "README.md", "bun.lock", "logo.png", "blob.json"].map(|f| dir.join(f)));
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!((lines, files), (6, 3));
        assert_eq!(langs, vec![Lang { name: "Markdown".into(), lines: 3 }, Lang { name: "TypeScript".into(), lines: 3 }]);
    }
}
