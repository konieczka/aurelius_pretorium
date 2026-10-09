//! Agent skills: folders holding a SKILL.md (YAML frontmatter with `name` and `description`, then instructions).
//! The agents share folders (Grok and OpenCode also read Claude's, Codex and OpenCode read `.agents/skills`), and
//! installers symlink one copy into several of them, so skills are listed by where they live, merged by real path,
//! each with the agents that read its folders.

use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

use crate::usage::Provider::{self, Antigravity, Claude, Codex, Grok, OpenCode};
use crate::{err, Res};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Project,
    Global,
    /// Owned by the agent itself (Codex's built-ins, Claude's claude.ai sync): read-only here.
    Managed,
}

/// A folder some agents look for skills in.
#[derive(Clone, Debug, Serialize)]
pub struct Loc {
    dir: PathBuf,
    scope: Scope,
    agents: Vec<Provider>,
    label: Option<&'static str>,
}

fn loc(dir: PathBuf, scope: Scope, agents: &[Provider]) -> Loc {
    Loc { dir, scope, agents: agents.to_vec(), label: None }
}

// ponytail: paths as of each CLI's current release (Oct 2026); extra dirs from Grok's/Codex's config.toml and
// Antigravity's .agents/skills.json, and plugin skills, aren't read yet.
/// Where each agent looks, for the project folder and the home folder.
fn locations(home: &Path, project: &Path) -> Vec<Loc> {
    use Scope::*;
    let mut out = vec![
        loc(project.join(".claude/skills"), Project, &[Claude, Grok, OpenCode]),
        loc(project.join(".agents/skills"), Project, &[Codex, Antigravity, OpenCode]),
        loc(project.join(".grok/skills"), Project, &[Grok]),
        loc(project.join(".opencode/skills"), Project, &[OpenCode]),
        loc(project.join(".opencode/skill"), Project, &[OpenCode]),
        loc(home.join(".claude/skills"), Global, &[Claude, Grok, OpenCode]),
        loc(home.join(".agents/skills"), Global, &[Codex, OpenCode]),
        loc(home.join(".codex/skills"), Global, &[Codex]),
        loc(home.join(".gemini/config/skills"), Global, &[Antigravity]),
        loc(home.join(".grok/skills"), Global, &[Grok]),
        loc(home.join(".config/opencode/skills"), Global, &[OpenCode]),
        loc(home.join(".config/opencode/skill"), Global, &[OpenCode]),
        Loc { label: Some("Codex built-in"), ..loc(home.join(".codex/skills/.system"), Managed, &[Codex]) },
    ];
    // Claude Code keeps claude.ai's skills in one folder per account.
    let synced = std::fs::read_dir(home.join(".claude/skills/synced")).into_iter().flatten().flatten();
    for e in synced.filter(|e| !e.file_name().to_string_lossy().starts_with('.')) {
        out.push(Loc { label: Some("Synced from claude.ai"), ..loc(e.path(), Managed, &[Claude]) });
    }
    out
}

/// Where "all agents" puts a new skill, and where it links it so the rest see it too.
fn shared(home: &Path, project: &Path, scope: Scope) -> Option<(PathBuf, Vec<PathBuf>)> {
    match scope {
        // .agents: Codex, Antigravity, OpenCode; .claude: Claude, Grok.
        Scope::Project => Some((project.join(".agents/skills"), vec![project.join(".claude/skills")])),
        // .agents: Codex, OpenCode; .claude: Claude, Grok; .gemini: Antigravity.
        Scope::Global => Some((home.join(".agents/skills"), vec![home.join(".claude/skills"), home.join(".gemini/config/skills")])),
        Scope::Managed => None,
    }
}

/// One place a skill shows up: its own folder, or a symlink to it.
#[derive(Debug, Serialize)]
pub struct Place {
    path: String,
    location: String,
    scope: Scope,
    link: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    name: String,
    description: String,
    /// Real folder (symlinks resolved).
    dir: String,
    /// The SKILL.md to open: in the real folder.
    file: String,
    scope: Scope,
    read_only: bool,
    agents: Vec<Provider>,
    places: Vec<Place>,
    /// Top-level files and folders beside SKILL.md (scripts/, references/...).
    files: Vec<String>,
    problems: Vec<String>,
}

/// Where "all agents" writes for a scope, for the add form to show.
#[derive(Serialize)]
pub struct Shared {
    scope: Scope,
    dir: String,
    links: Vec<String>,
}

#[derive(Serialize)]
pub struct Scan {
    skills: Vec<Skill>,
    locations: Vec<Loc>,
    shared: Vec<Shared>,
    home: String,
}

/// Top-level `key: value` pairs of a SKILL.md's frontmatter. Handles what skills use: plain, quoted, folded (`>`)
/// and literal (`|`) scalars. None when there's no frontmatter block.
fn frontmatter(text: &str) -> Option<Vec<(String, String)>> {
    let mut lines = text.trim_start_matches('\u{feff}').lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    let mut out: Vec<(String, String)> = Vec::new();
    let mut block: Option<char> = None; // style of the value being continued on indented lines
    let mut closed = false;
    for line in lines {
        let line = line.trim_end();
        if line == "---" {
            closed = true;
            break;
        }
        let indented = line.starts_with([' ', '\t']);
        if indented || line.is_empty() {
            if let (Some(style), Some((_, v))) = (block, out.last_mut()) {
                let part = line.trim();
                let sep = if style == '|' { "\n" } else { " " };
                if !v.is_empty() {
                    v.push_str(sep);
                }
                v.push_str(part);
            }
            continue;
        }
        let Some((k, v)) = line.split_once(':') else {
            block = None;
            continue;
        };
        let v = v.trim();
        let (value, style) = match v.chars().next() {
            Some(c @ ('>' | '|')) if v[1..].trim_matches(['-', '+']).is_empty() => (String::new(), Some(c)),
            Some('"') if v.len() > 1 && v.ends_with('"') => (v[1..v.len() - 1].replace("\\\"", "\"").replace("\\\\", "\\"), None),
            Some('\'') if v.len() > 1 && v.ends_with('\'') => (v[1..v.len() - 1].replace("''", "'"), None),
            _ => (v.to_string(), Some(' ')), // a plain scalar may go on over indented lines
        };
        out.push((k.trim().to_string(), value));
        block = style;
    }
    if !closed {
        return None;
    }
    for (_, v) in &mut out {
        *v = v.trim().to_string();
    }
    Some(out)
}

/// The Agent Skills naming rule: lowercase letters, digits and single hyphens, at most 64 characters.
pub fn valid_name(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !s.starts_with('-')
        && !s.ends_with('-')
        && !s.contains("--")
}

/// Name, description and what's wrong with a skill folder's SKILL.md.
fn read_skill(dir: &Path) -> (String, String, Vec<String>) {
    let folder = dir.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let text = match std::fs::read_to_string(dir.join("SKILL.md")) {
        Ok(t) => t,
        Err(e) => return (folder, String::new(), vec![format!("Can't read SKILL.md: {e}")]),
    };
    let Some(fm) = frontmatter(&text) else {
        return (folder, String::new(), vec!["SKILL.md has no frontmatter (a --- block with name and description)".into()]);
    };
    let get = |k: &str| fm.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone()).unwrap_or_default();
    let (name, description) = (get("name"), get("description"));
    let mut problems = Vec::new();
    if name.is_empty() {
        problems.push("No name in the frontmatter".into());
    } else if !valid_name(&name) {
        problems.push("Name should be lowercase letters, digits and hyphens, at most 64 characters".into());
    } else if name != folder {
        problems.push(format!("Name \u{201c}{name}\u{201d} differs from its folder \u{201c}{folder}\u{201d}"));
    }
    if description.is_empty() {
        problems.push("No description: agents won't know when to use it".into());
    } else if description.chars().count() > 1024 {
        problems.push("Description is over 1024 characters; some agents cut it off".into());
    }
    (if name.is_empty() { folder } else { name }, description, problems)
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn scan(home: &Path, project: &Path) -> Scan {
    let locations = locations(home, project);
    let mut skills: Vec<Skill> = Vec::new();
    for l in &locations {
        let mut entries: Vec<_> = std::fs::read_dir(&l.dir).into_iter().flatten().flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let path = e.path();
            if e.file_name().to_string_lossy().starts_with('.') {
                continue; // .system is a location of its own; .trash, .DS_Store...
            }
            let link = e.file_type().is_ok_and(|t| t.is_symlink());
            let place = Place { path: s(&path), location: s(&l.dir), scope: l.scope, link };
            let Ok(real) = std::fs::canonicalize(&path) else {
                if link {
                    let to = std::fs::read_link(&path).map(|t| s(&t)).unwrap_or_default();
                    skills.push(Skill {
                        name: e.file_name().to_string_lossy().into_owned(),
                        description: String::new(),
                        dir: s(&path),
                        file: String::new(),
                        scope: l.scope,
                        read_only: l.scope == Scope::Managed,
                        agents: l.agents.clone(),
                        places: vec![place],
                        files: vec![],
                        problems: vec![format!("Broken link: {to} doesn't exist")],
                    });
                }
                continue;
            };
            if !real.join("SKILL.md").is_file() {
                continue;
            }
            if let Some(k) = skills.iter_mut().find(|k| k.dir == s(&real)) {
                for a in &l.agents {
                    if !k.agents.contains(a) {
                        k.agents.push(*a);
                    }
                }
                if !link && k.places.iter().all(|p| p.link) {
                    k.scope = l.scope; // where the folder itself is decides
                }
                k.places.push(place);
                continue;
            }
            let (name, description, problems) = read_skill(&real);
            let mut files: Vec<String> = std::fs::read_dir(&real)
                .into_iter()
                .flatten()
                .flatten()
                .map(|f| f.file_name().to_string_lossy().into_owned())
                .filter(|f| f != "SKILL.md" && !f.starts_with('.'))
                .collect();
            files.sort();
            skills.push(Skill {
                name,
                description,
                file: s(&real.join("SKILL.md")),
                dir: s(&real),
                scope: l.scope,
                read_only: l.scope == Scope::Managed,
                agents: l.agents.clone(),
                places: vec![place],
                files,
                problems,
            });
        }
    }
    for k in &mut skills {
        k.read_only = k.scope == Scope::Managed;
        k.places.sort_by_key(|p| p.link); // its own folder first
        k.agents.sort_by_key(|a| *a as u8);
    }
    let shared = [Scope::Project, Scope::Global]
        .into_iter()
        .filter_map(|scope| shared(home, project, scope).map(|(d, links)| Shared { scope, dir: s(&d), links: links.iter().map(|l| s(l)).collect() }))
        .collect();
    Scan { skills, locations, shared, home: s(home) }
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

/// Every skill the agents would see in `project` (the folder in the sidebar) and the home folder.
#[tauri::command]
pub async fn skills_scan(project: String) -> Res<Scan> {
    tauri::async_runtime::spawn_blocking(move || scan(&home(), Path::new(&project))).await.map_err(err)
}

/// `to` relative to the folder `from` (both absolute), for symlinks that survive the tree moving (a cloned repo).
fn relative(from: &Path, to: &Path) -> PathBuf {
    let (f, t): (Vec<Component>, Vec<Component>) = (from.components().collect(), to.components().collect());
    let common = f.iter().zip(&t).take_while(|(a, b)| a == b).count();
    let mut out: PathBuf = f[common..].iter().map(|_| Component::ParentDir).collect();
    out.extend(&t[common..]);
    out
}

/// A YAML scalar for a one-line value: plain when that's unambiguous, else double-quoted.
fn yaml_str(v: &str) -> String {
    let v = v.split_whitespace().collect::<Vec<_>>().join(" ");
    let plain = !v.is_empty()
        && !v.contains(": ")
        && !v.contains(" #")
        && !v.ends_with(':')
        && !v.starts_with(|c: char| "-?:,[]{}#&*!|>'\"%@`".contains(c));
    if plain {
        v
    } else {
        format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

fn template(name: &str, description: &str) -> String {
    let title: String = name.split('-').map(|w| w[..1].to_uppercase() + &w[1..]).collect::<Vec<_>>().join(" ");
    format!(
        "---\nname: {name}\ndescription: {}\n---\n\n# {title}\n\nWhat the agent should do when this skill applies: the steps, conventions and checks.\n",
        yaml_str(description)
    )
}

fn create(home: &Path, project: &Path, scope: Scope, dir: Option<&Path>, name: &str, description: &str) -> Res<String> {
    if !valid_name(name) {
        return Err("Use lowercase letters, digits and hyphens (at most 64 characters).".into());
    }
    let (primary, links) = match dir {
        Some(d) => {
            let known = locations(home, project).into_iter().any(|l| l.dir == d && l.scope == scope && scope != Scope::Managed);
            if !known {
                return Err(format!("{} isn't a skills folder", d.display()));
            }
            (d.to_path_buf(), vec![])
        }
        None => shared(home, project, scope).ok_or("Can't add managed skills")?,
    };
    // Every target must be free before anything is written.
    for d in std::iter::once(&primary).chain(&links) {
        if std::fs::symlink_metadata(d.join(name)).is_ok() {
            return Err(format!("{} already exists", d.join(name).display()));
        }
    }
    let skill = primary.join(name);
    std::fs::create_dir_all(&primary).map_err(err)?;
    std::fs::create_dir(&skill).map_err(err)?;
    std::fs::write(skill.join("SKILL.md"), template(name, description)).map_err(err)?;
    for d in &links {
        symlink_into(d, &skill)?;
    }
    Ok(s(&skill.join("SKILL.md")))
}

/// A relative link to `skill` from the skills folder `dir`, under the skill's folder name.
fn symlink_into(dir: &Path, skill: &Path) -> Res<()> {
    std::fs::create_dir_all(dir).map_err(err)?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(relative(dir, skill), dir.join(skill.file_name().ok_or("no folder name")?)).map_err(err)?;
    Ok(())
}

fn editable(home: &Path, project: &Path) -> Vec<PathBuf> {
    locations(home, project).into_iter().filter(|l| l.scope != Scope::Managed).map(|l| l.dir).collect()
}

fn link(home: &Path, project: &Path, skill: &Path, dirs: &[String]) -> Res<()> {
    if !skill.join("SKILL.md").is_file() {
        return Err(format!("{} isn't a skill", skill.display()));
    }
    let name = skill.file_name().ok_or("no folder name")?;
    let editable = editable(home, project);
    // Every target must be a skills folder and free before anything is linked.
    for d in dirs.iter().map(Path::new) {
        if !editable.iter().any(|e| e == d) {
            return Err(format!("{} isn't a skills folder", d.display()));
        }
        if std::fs::symlink_metadata(d.join(name)).is_ok() {
            return Err(format!("{} already exists", d.join(name).display()));
        }
    }
    dirs.iter().try_for_each(|d| symlink_into(Path::new(d), skill))
}

/// Give more agents a skill: link its folder `dir` from the skills folders `dirs` they read.
#[tauri::command]
pub fn skill_link(project: String, dir: String, dirs: Vec<String>) -> Res<()> {
    link(&home(), Path::new(&project), Path::new(&dir), &dirs)
}

/// New skill `name` in the skills folder `dir`, or with no `dir` in the shared folder of `scope` plus links from
/// the others, so every agent sees it. Returns its SKILL.md.
#[tauri::command]
pub fn skill_create(project: String, scope: Scope, dir: Option<String>, name: String, description: String) -> Res<String> {
    create(&home(), Path::new(&project), scope, dir.as_deref().map(Path::new), &name, &description)
}

fn delete(home: &Path, project: &Path, paths: &[String]) -> Res<()> {
    let editable = editable(home, project);
    for p in paths {
        let p = Path::new(p);
        if !p.parent().is_some_and(|d| editable.iter().any(|e| e == d)) {
            return Err(format!("{} isn't in a skills folder", p.display()));
        }
    }
    // Links first: once the folder is gone they'd dangle.
    let (links, dirs): (Vec<&Path>, Vec<&Path>) =
        paths.iter().map(Path::new).partition(|p| std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink()));
    for l in links {
        std::fs::remove_file(l).map_err(err)?;
    }
    for d in dirs {
        crate::to_system_trash(d).map_err(err)?;
    }
    Ok(())
}

/// Remove skill folders and links. Links are just unlinked; folders go to the system Trash.
#[tauri::command]
pub fn skill_delete(project: String, paths: Vec<String>) -> Res<()> {
    delete(&home(), Path::new(&project), &paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fm(t: &str) -> Vec<(String, String)> {
        frontmatter(t).unwrap()
    }

    #[test]
    fn reads_frontmatter_styles() {
        assert_eq!(fm("---\nname: a\ndescription: Does b: c\n---\n# A"), [("name".into(), "a".into()), ("description".into(), "Does b: c".into())]);
        assert_eq!(fm("---\nname: \"imagegen\"\ndescription: 'It''s'\n---\n")[1].1, "It's");
        assert_eq!(fm("---\nname: \"imagegen\"\n---\n")[0].1, "imagegen");
        assert_eq!(fm("---\nname: x\ndescription: >-\n  Generate images\n  from text.\nlicense: MIT\n---\n")[1].1, "Generate images from text.");
        assert_eq!(fm("---\ndescription: |\n  one\n  two\n---\n")[0].1, "one\ntwo");
        assert_eq!(fm("---\r\ndescription: long\r\n  wrapped line\r\n---\r\n")[0].1, "long wrapped line");
        assert!(frontmatter("# no frontmatter").is_none());
        assert!(frontmatter("---\nname: x\n").is_none());
    }

    #[test]
    fn checks_names() {
        assert!(valid_name("git-commit-2"));
        for bad in ["", "Git", "a_b", "-a", "a-", "a--b", &"a".repeat(65)] {
            assert!(!valid_name(bad), "{bad}");
        }
    }

    #[test]
    fn quotes_yaml_only_when_needed() {
        assert_eq!(yaml_str("Commit with  conventions"), "Commit with conventions");
        assert_eq!(yaml_str("Use when: committing"), "\"Use when: committing\"");
        assert_eq!(yaml_str("\"quoted\""), "\"\\\"quoted\\\"\"");
        assert_eq!(yaml_str(""), "\"\"");
        let t = template("git-commit", "Use when: x");
        assert_eq!(fm(&t), [("name".into(), "git-commit".into()), ("description".into(), "Use when: x".into())]);
        assert!(t.contains("# Git Commit"));
    }

    #[test]
    fn relative_paths() {
        assert_eq!(relative(Path::new("/h/.claude/skills"), Path::new("/h/.agents/skills/x")), PathBuf::from("../../.agents/skills/x"));
        assert_eq!(relative(Path::new("/p/.claude/skills"), Path::new("/p/.claude/skills/x")), PathBuf::from("x"));
    }

    fn base(tag: &str) -> PathBuf {
        let b = std::env::temp_dir().join(format!("fc-skills-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&b);
        std::fs::create_dir_all(b.join("home")).unwrap();
        std::fs::create_dir_all(b.join("proj")).unwrap();
        std::fs::canonicalize(b).unwrap() // macOS: /var -> /private/var, as canonicalize reports real dirs
    }

    #[test]
    fn scans_merges_links_and_flags_problems() {
        let b = base("scan");
        let (home, proj) = (b.join("home"), b.join("proj"));
        let write = |p: PathBuf, text: &str| {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        };
        write(home.join(".agents/skills/wrangler/SKILL.md"), "---\nname: wrangler\ndescription: Workers CLI\n---\n");
        write(home.join(".agents/skills/wrangler/scripts/run.sh"), "");
        std::fs::create_dir_all(home.join(".claude/skills/.trash/x")).unwrap();
        std::os::unix::fs::symlink("../../.agents/skills/wrangler", home.join(".claude/skills/wrangler")).unwrap();
        std::os::unix::fs::symlink("../../.agents/skills/gone", home.join(".claude/skills/gone")).unwrap();
        write(home.join(".codex/skills/.system/imagegen/SKILL.md"), "---\nname: \"imagegen\"\ndescription: \"Images\"\n---\n");
        write(home.join(".claude/skills/synced/acct/docs/SKILL.md"), "---\nname: docs\ndescription: Docs\n---\n");
        write(proj.join(".grok/skills/Commit/SKILL.md"), "---\nname: commit\n---\n");
        std::fs::create_dir_all(proj.join(".claude/skills/not-a-skill")).unwrap();

        let r = scan(&home, &proj);
        let by = |n: &str| r.skills.iter().find(|k| k.name == n).unwrap_or_else(|| panic!("{n}"));
        assert_eq!(r.skills.len(), 5);

        let w = by("wrangler");
        assert_eq!(w.scope, Scope::Global);
        assert_eq!(w.agents, [Claude, Codex, Grok, OpenCode]);
        assert_eq!(w.places.len(), 2);
        assert!(w.places.iter().any(|p| p.link && p.path.ends_with(".claude/skills/wrangler")));
        assert_eq!(w.files, ["scripts"]);
        assert!(w.problems.is_empty());

        assert!(by("gone").problems[0].starts_with("Broken link"));
        assert!(by("imagegen").read_only);
        assert_eq!(by("docs").agents, [Claude]);
        assert!(by("docs").read_only);

        let c = by("commit");
        assert_eq!((c.scope, c.agents.as_slice()), (Scope::Project, [Grok].as_slice()));
        assert_eq!(c.problems.len(), 2); // name vs folder, no description
        std::fs::remove_dir_all(&b).unwrap();
    }

    fn link_path(dir: &str) -> PathBuf {
        Path::new(dir).join("git-commit")
    }

    #[test]
    fn creates_for_all_agents_and_deletes() {
        let b = base("create");
        let (home, proj) = (b.join("home"), b.join("proj"));
        let file = create(&home, &proj, Scope::Global, None, "git-commit", "Commits").unwrap();
        assert!(file.ends_with(".agents/skills/git-commit/SKILL.md"));
        let r = scan(&home, &proj);
        let k = &r.skills[0];
        assert_eq!(r.skills.len(), 1);
        assert_eq!(k.agents, [Claude, Codex, Grok, Antigravity, OpenCode]);
        assert_eq!(std::fs::read_link(home.join(".claude/skills/git-commit")).unwrap(), PathBuf::from("../../.agents/skills/git-commit"));
        assert!(create(&home, &proj, Scope::Global, None, "git-commit", "").unwrap_err().contains("already exists"));

        // Project, all agents: everyone sees it through .agents + .claude.
        create(&home, &proj, Scope::Project, None, "deploy", "Deploys").unwrap();
        let d = scan(&home, &proj).skills.into_iter().find(|k| k.name == "deploy").unwrap();
        assert_eq!((d.scope, d.agents.len()), (Scope::Project, 5));

        // One folder only, which must be a known one.
        create(&home, &proj, Scope::Project, Some(&proj.join(".grok/skills")), "lint", "Lints").unwrap();
        assert!(create(&home, &proj, Scope::Project, Some(&proj.join("src")), "x", "").is_err());
        assert!(create(&home, &proj, Scope::Global, None, "Bad Name", "").is_err());

        // Deleting unlinks links and refuses paths outside skills folders.
        assert!(delete(&home, &proj, &[s(&proj.join("src/x"))]).is_err());
        let gone = s(&home.join(".gemini/config/skills/git-commit"));
        delete(&home, &proj, &[gone.clone()]).unwrap();
        assert!(std::fs::symlink_metadata(&gone).is_err());
        assert!(home.join(".agents/skills/git-commit/SKILL.md").exists());

        // Giving access back: a relative link from the folder Antigravity reads.
        let gemini = s(&home.join(".gemini/config/skills"));
        let skill = home.join(".agents/skills/git-commit");
        link(&home, &proj, &skill, &[gemini.clone()]).unwrap();
        assert_eq!(std::fs::read_link(&link_path(&gemini)).unwrap(), PathBuf::from("../../../.agents/skills/git-commit"));
        assert!(link(&home, &proj, &skill, &[gemini]).unwrap_err().contains("already exists"));
        assert!(link(&home, &proj, &skill, &[s(&proj.join("src"))]).is_err());
        assert!(link(&home, &proj, &proj.join("nope"), &[s(&home.join(".grok/skills"))]).is_err());
        std::fs::remove_dir_all(&b).unwrap();
    }
}
