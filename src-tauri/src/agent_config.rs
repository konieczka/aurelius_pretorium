//! Agent settings: each agent's config files (global, project, managed), read and edited in place. Edits go
//! through a parser that keeps the rest of the file as it was (comments, order, formatting): toml_edit for TOML,
//! jsonc-parser's CST for JSON. Agents rewrite these files themselves, so every write first checks the file still
//! holds what the page last read, and lands atomically.

use jsonc_parser::cst::{CstInputValue, CstObject, CstRootNode};
use jsonc_parser::ParseOptions;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item, TableLike};

use crate::usage::Provider::{self, Antigravity, Claude, Codex, Grok, OpenCode};
use crate::{err, Res};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Json,
    Jsonc,
    Toml,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// Set by an admin or device management: read-only here.
    Managed,
    /// Project, but only for you (Claude's settings.local.json).
    Local,
    Project,
    Global,
}

/// One config file an agent reads.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct File {
    path: String,
    scope: Scope,
    label: &'static str,
    format: Format,
    exists: bool,
    read_only: bool,
    /// What's on disk ("" when missing): the base the next write must still find.
    text: String,
    /// Parsed contents as JSON (TOML tables become objects); None when it doesn't parse.
    values: Option<Value>,
    error: Option<String>,
}

/// A model the agent offers, from its own model cache, with the reasoning efforts it takes.
#[derive(Debug, Serialize, PartialEq)]
pub struct Model {
    id: String,
    name: String,
    efforts: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Scan {
    agents: Vec<AgentConfig>,
    home: String,
}

#[derive(Debug, Serialize)]
pub struct AgentConfig {
    agent: Provider,
    installed: bool,
    /// Highest precedence first: the first file that sets a key decides it.
    files: Vec<File>,
    models: Vec<Model>,
}

struct Spec {
    path: PathBuf,
    scope: Scope,
    label: &'static str,
    format: Format,
}

fn spec(path: PathBuf, scope: Scope, label: &'static str) -> Spec {
    let format = match path.extension().and_then(|e| e.to_str()) {
        Some("toml") => Format::Toml,
        Some("jsonc") => Format::Jsonc,
        _ => Format::Json,
    };
    Spec { path, scope, label, format }
}

/// `dir/name.jsonc` if it's there, else `dir/name.json` (what a new file is called).
fn json_or_jsonc(dir: &Path, name: &str) -> PathBuf {
    let c = dir.join(format!("{name}.jsonc"));
    if c.exists() { c } else { dir.join(format!("{name}.json")) }
}

// ponytail: paths as of each CLI's current release (Oct 2026). Env overrides (CODEX_HOME, GROK_HOME, OPENCODE_CONFIG,
// GROK_CONFIG overlays) and Windows/Linux managed paths aren't followed.
/// The files each agent reads, highest precedence first. Managed files only when present.
fn specs(agent: Provider, home: &Path, project: &Path) -> Vec<Spec> {
    use Scope::*;
    let managed = |paths: &[&str], label| {
        paths.iter().map(PathBuf::from).filter(|p| p.exists()).map(|p| spec(p, Managed, label)).collect::<Vec<_>>()
    };
    let mut out = Vec::new();
    match agent {
        Claude => {
            out.extend(managed(&["/Library/Application Support/ClaudeCode/managed-settings.json"], "Managed"));
            out.push(spec(project.join(".claude/settings.local.json"), Local, "Project · just you"));
            out.push(spec(project.join(".claude/settings.json"), Project, "Project · shared"));
            out.push(spec(home.join(".claude/settings.json"), Global, "Global"));
        }
        Codex => {
            let home_managed = home.join(".codex/managed_config.toml");
            out.extend(managed(&["/etc/codex/managed_config.toml"], "Managed"));
            if home_managed.exists() {
                out.push(spec(home_managed, Managed, "Managed"));
            }
            out.push(spec(project.join(".codex/config.toml"), Project, "Project"));
            out.push(spec(home.join(".codex/config.toml"), Global, "Global"));
        }
        Grok => {
            // requirements.toml pins over everything; managed_config.toml is only a default under your own file.
            for p in [PathBuf::from("/etc/grok/requirements.toml"), home.join(".grok/requirements.toml")] {
                if p.exists() {
                    out.push(spec(p, Managed, "Requirements"));
                }
            }
            out.push(spec(project.join(".grok/config.toml"), Project, "Project"));
            out.push(spec(home.join(".grok/config.toml"), Global, "Global"));
            for p in [PathBuf::from("/etc/grok/managed_config.toml"), home.join(".grok/managed_config.toml")] {
                if p.exists() {
                    out.push(spec(p, Managed, "Managed defaults"));
                }
            }
        }
        Antigravity => {
            out.push(spec(home.join(".gemini/antigravity-cli/settings.json"), Global, "Global"));
            out.push(spec(home.join(".gemini/config/mcp_config.json"), Global, "MCP servers"));
        }
        OpenCode => {
            out.push(Spec { format: Format::Jsonc, ..spec(json_or_jsonc(project, "opencode"), Project, "Project") });
            out.push(Spec { format: Format::Jsonc, ..spec(json_or_jsonc(&home.join(".config/opencode"), "opencode"), Global, "Global") });
        }
    }
    out
}

const AGENTS: [Provider; 5] = [Claude, Codex, Grok, Antigravity, OpenCode];

fn command(agent: Provider) -> &'static str {
    match agent {
        Claude => "claude",
        Codex => "codex",
        Grok => "grok",
        Antigravity => "agy",
        OpenCode => "opencode",
    }
}

/// On the login shell's PATH, or in the agent's own bin folder (where its installer puts it).
fn installed(agent: Provider, home: &Path) -> bool {
    let own = match agent {
        Grok => Some(home.join(".grok/bin/grok")),
        OpenCode => Some(home.join(".opencode/bin/opencode")),
        _ => None,
    };
    own.is_some_and(|p| p.exists()) || crate::login_path().split(':').any(|d| Path::new(d).join(command(agent)).exists())
}

/// Models from the agent's own cache of its model list (Codex and Grok keep one); none for the others.
fn models(agent: Provider, home: &Path) -> Vec<Model> {
    let read = |p: PathBuf| std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
    let strs = |v: Option<&Value>, key: &str| -> Vec<String> {
        v.and_then(Value::as_array).into_iter().flatten().filter_map(|e| e.get(key)?.as_str().map(String::from)).collect()
    };
    match agent {
        Codex => {
            let Some(d) = read(home.join(".codex/models_cache.json")) else { return vec![] };
            d["models"].as_array().into_iter().flatten()
                .filter(|m| m["visibility"].as_str().is_none_or(|v| v == "list"))
                .filter_map(|m| {
                    let id = m["slug"].as_str()?.to_string();
                    let name = m["display_name"].as_str().unwrap_or(&id).to_string();
                    Some(Model { id, name, efforts: strs(m.get("supported_reasoning_levels"), "effort") })
                })
                .collect()
        }
        Grok => {
            let Some(d) = read(home.join(".grok/models_cache.json")) else { return vec![] };
            d["models"].as_object().into_iter().flatten()
                .filter(|(_, m)| m["info"]["hidden"].as_bool() != Some(true))
                .map(|(id, m)| Model {
                    id: id.clone(),
                    name: m["info"]["name"].as_str().unwrap_or(id).to_string(),
                    efforts: strs(m["info"].get("reasoning_efforts"), "value"),
                })
                .collect()
        }
        _ => vec![],
    }
}

/// Parse a config file's text into JSON values. Blank text is an empty file: no settings.
fn parse(text: &str, format: Format) -> Res<Value> {
    if text.trim().is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    match format {
        Format::Toml => Ok(toml_table(text.parse::<DocumentMut>().map_err(err)?.as_table())),
        Format::Json | Format::Jsonc => {
            let opts = if format == Format::Json { strict() } else { ParseOptions::default() };
            jsonc_parser::parse_to_serde_value::<Value>(text, &opts).map_err(err)
        }
    }
}

/// Plain JSON: what the agent's own parser will accept.
fn strict() -> ParseOptions {
    ParseOptions { allow_comments: false, allow_loose_object_property_names: false, allow_trailing_commas: false, allow_missing_commas: false, ..Default::default() }
}

fn toml_table(t: &dyn TableLike) -> Value {
    Value::Object(t.iter().filter_map(|(k, v)| Some((k.to_string(), toml_item(v)?))).collect())
}

fn toml_item(i: &Item) -> Option<Value> {
    match i {
        Item::None => None,
        Item::Value(v) => Some(toml_value(v)),
        Item::Table(t) => Some(toml_table(t)),
        Item::ArrayOfTables(a) => Some(Value::Array(a.iter().map(|t| toml_table(t)).collect())),
    }
}

fn toml_value(v: &toml_edit::Value) -> Value {
    use toml_edit::Value as T;
    match v {
        T::String(s) => Value::String(s.value().clone()),
        T::Integer(n) => Value::from(*n.value()),
        T::Float(f) => serde_json::Number::from_f64(*f.value()).map_or(Value::Null, Value::Number),
        T::Boolean(b) => Value::Bool(*b.value()),
        T::Datetime(d) => Value::String(d.value().to_string()),
        T::Array(a) => Value::Array(a.iter().map(toml_value).collect()),
        T::InlineTable(t) => toml_table(t),
    }
}

fn to_toml(v: &Value) -> Res<toml_edit::Value> {
    Ok(match v {
        Value::Null => return Err("TOML has no null".into()),
        Value::Bool(b) => (*b).into(),
        Value::Number(n) => match n.as_i64() {
            Some(i) => i.into(),
            None => n.as_f64().ok_or("Number out of range")?.into(),
        },
        Value::String(s) => s.as_str().into(),
        Value::Array(a) => toml_edit::Value::Array(a.iter().map(to_toml).collect::<Res<_>>()?),
        Value::Object(o) => {
            let mut t = toml_edit::InlineTable::new();
            for (k, v) in o {
                t.insert(k, to_toml(v)?);
            }
            toml_edit::Value::InlineTable(t)
        }
    })
}

fn to_cst(v: &Value) -> CstInputValue {
    match v {
        Value::Null => CstInputValue::Null,
        Value::Bool(b) => CstInputValue::Bool(*b),
        Value::Number(n) => CstInputValue::Number(n.to_string()),
        Value::String(s) => CstInputValue::String(s.clone()),
        Value::Array(a) => CstInputValue::Array(a.iter().map(to_cst).collect()),
        Value::Object(o) => CstInputValue::Object(o.iter().map(|(k, v)| (k.clone(), to_cst(v))).collect()),
    }
}

/// One setting to change: `key` is the path of names down to it; no value removes it.
#[derive(Debug, Deserialize)]
pub struct Edit {
    key: Vec<String>,
    value: Option<Value>,
}

/// Apply `edits` to a TOML text, keeping everything else (comments, order, a replaced value's trailing comment).
fn edit_toml(text: &str, edits: &[Edit]) -> Res<String> {
    let mut doc = text.parse::<DocumentMut>().map_err(err)?;
    for e in edits {
        let Some((last, parents)) = e.key.split_last() else { continue };
        let mut table: &mut dyn TableLike = doc.as_table_mut();
        let mut missing = false;
        for k in parents {
            if table.get(k).and_then(Item::as_table_like).is_none() {
                if e.value.is_none() {
                    missing = true; // nothing to remove
                    break;
                }
                table.insert(k, toml_edit::table());
            }
            table = table.get_mut(k).and_then(Item::as_table_like_mut).ok_or("Not a table")?;
        }
        if missing {
            continue;
        }
        match &e.value {
            None => {
                table.remove(last);
            }
            Some(v) => {
                let new = to_toml(v)?;
                match table.get_mut(last) {
                    Some(Item::Value(old)) => {
                        let decor = old.decor().clone();
                        *old = new;
                        *old.decor_mut() = decor;
                    }
                    _ => {
                        table.insert(last, Item::Value(new));
                    }
                }
            }
        }
    }
    Ok(doc.to_string())
}

/// Apply `edits` to a JSON(C) text through its syntax tree, so comments and layout stay.
fn edit_json(text: &str, edits: &[Edit]) -> Res<String> {
    let src = if text.trim().is_empty() { "{}\n" } else { text };
    let root = CstRootNode::parse(src, &ParseOptions::default()).map_err(err)?;
    for e in edits {
        let Some((last, parents)) = e.key.split_last() else { continue };
        let mut obj: CstObject = root.object_value_or_set();
        let mut missing = false;
        for k in parents {
            obj = match (&e.value, obj.object_value(k)) {
                (_, Some(o)) => o,
                (None, None) => {
                    missing = true;
                    break;
                }
                (Some(_), None) => obj.object_value_or_set(k),
            };
        }
        if missing {
            continue;
        }
        match (&e.value, obj.get(last)) {
            (Some(v), Some(p)) => p.set_value(to_cst(v)),
            (Some(v), None) => {
                obj.append(last, to_cst(v));
            }
            (None, Some(p)) => p.remove(),
            (None, None) => {}
        }
    }
    Ok(root.to_string())
}

fn read(path: &Path) -> Res<String> {
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(err(e)),
    }
}

/// Write `content` over `path` if it still holds `base` (an agent may have rewritten it since the page read it).
/// Lands atomically through a temp file, through a symlink (dotfile managers) to its target, with the mode kept.
fn write_checked(path: &Path, base: &str, content: &str) -> Res<()> {
    if read(path)? != base {
        return Err(format!("{} changed on disk since it was loaded. Reloaded it; check it and save again.", path.display()));
    }
    let real = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    std::fs::create_dir_all(real.parent().ok_or("No parent folder")?).map_err(err)?;
    crate::write_atomic(&real, content.as_bytes()).map_err(err)
}

fn load(s: Spec) -> File {
    let (text, read_err) = match read(&s.path) {
        Ok(t) => (t, None),
        Err(e) => (String::new(), Some(e)),
    };
    let (values, error) = match read_err {
        Some(e) => (None, Some(e)),
        None => match parse(&text, s.format) {
            Ok(v) => (Some(v), None),
            Err(e) => (None, Some(e)),
        },
    };
    File {
        exists: s.path.exists(),
        read_only: s.scope == Scope::Managed,
        path: s.path.to_string_lossy().into_owned(),
        scope: s.scope,
        label: s.label,
        format: s.format,
        text,
        values,
        error,
    }
}

fn scan(home: &Path, project: &Path) -> Scan {
    let agents = AGENTS
        .iter()
        .map(|&agent| AgentConfig {
            agent,
            installed: installed(agent, home),
            files: specs(agent, home, project).into_iter().map(load).collect(),
            models: models(agent, home),
        })
        .collect();
    Scan { agents, home: home.to_string_lossy().into_owned() }
}

/// The format of `path` if it's a file some agent reads that may be written here: no writing anywhere else.
fn writable(home: &Path, project: &Path, path: &str) -> Res<Format> {
    AGENTS
        .iter()
        .flat_map(|&a| specs(a, home, project))
        .find(|s| s.scope != Scope::Managed && s.path == Path::new(path))
        .map(|s| s.format)
        .ok_or_else(|| format!("{path} isn't an agent settings file"))
}

fn save(home: &Path, project: &Path, path: &str, base: &str, content: &str) -> Res<()> {
    let format = writable(home, project, path)?;
    parse(content, format).map_err(|e| format!("Not saved: {e}"))?;
    write_checked(Path::new(path), base, content)
}

fn set(home: &Path, project: &Path, path: &str, base: &str, edits: &[Edit]) -> Res<String> {
    let format = writable(home, project, path)?;
    let text = match format {
        Format::Toml => edit_toml(base, edits)?,
        Format::Json | Format::Jsonc => edit_json(base, edits)?,
    };
    parse(&text, format)?;
    write_checked(Path::new(path), base, &text)?;
    Ok(text)
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

#[tauri::command]
pub async fn agent_configs(project: String) -> Res<Scan> {
    tauri::async_runtime::spawn_blocking(move || scan(&home(), Path::new(&project))).await.map_err(err)
}

/// Save a whole file from the editor, if it parses.
#[tauri::command]
pub fn agent_config_save(project: String, path: String, base: String, content: String) -> Res<()> {
    save(&home(), Path::new(&project), &path, &base, &content)
}

/// Change single settings in a file; returns its new text.
#[tauri::command]
pub fn agent_config_set(project: String, path: String, base: String, edits: Vec<Edit>) -> Res<String> {
    set(&home(), Path::new(&project), &path, &base, &edits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn edit(key: &str, value: Option<Value>) -> Edit {
        Edit { key: key.split('.').map(String::from).collect(), value }
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fc-agent-config-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn toml_edits_keep_comments_and_layout() {
        let text = "# my config\nmodel = \"a\" # pinned\n\n[projects.\"/x\"]\ntrust_level = \"trusted\"\n";
        let out = edit_toml(text, &[edit("model", Some(json!("b"))), edit("model_reasoning_effort", Some(json!("high")))]).unwrap();
        assert_eq!(out, "# my config\nmodel = \"b\" # pinned\nmodel_reasoning_effort = \"high\"\n\n[projects.\"/x\"]\ntrust_level = \"trusted\"\n");
    }

    #[test]
    fn toml_edits_make_and_leave_tables() {
        let text = "[ui]\nyolo = false\n";
        let out = edit_toml(text, &[edit("models.default", Some(json!("grok-4.7"))), edit("ui.yolo", None), edit("nope.x", None)]).unwrap();
        assert_eq!(parse(&out, Format::Toml).unwrap(), json!({ "ui": {}, "models": { "default": "grok-4.7" } }));
        assert!(!out.contains("nope"));
        let out = edit_toml("", &[edit("session.auto_compact_threshold_percent", Some(json!(80)))]).unwrap();
        assert_eq!(out, "[session]\nauto_compact_threshold_percent = 80\n");
    }

    #[test]
    fn json_edits_keep_comments_and_order() {
        let text = "{\n  // keep me\n  \"model\": \"opus\",\n  \"permissions\": {\n    \"allow\": []\n  }\n}\n";
        let out = edit_json(text, &[edit("model", Some(json!("sonnet"))), edit("permissions.defaultMode", Some(json!("plan"))), edit("effortLevel", Some(json!("high")))]).unwrap();
        assert!(out.contains("// keep me"));
        assert!(out.find("\"model\"").unwrap() < out.find("\"permissions\"").unwrap());
        assert_eq!(
            parse(&out, Format::Jsonc).unwrap(),
            json!({ "model": "sonnet", "permissions": { "allow": [], "defaultMode": "plan" }, "effortLevel": "high" })
        );
        let out = edit_json(&out, &[edit("permissions.defaultMode", None), edit("missing.key", None)]).unwrap();
        assert_eq!(parse(&out, Format::Jsonc).unwrap(), json!({ "model": "sonnet", "permissions": { "allow": [] }, "effortLevel": "high" }));
    }

    #[test]
    fn json_edits_start_blank_files() {
        let out = edit_json("", &[edit("alwaysThinkingEnabled", Some(json!(true)))]).unwrap();
        assert_eq!(parse(&out, Format::Json).unwrap(), json!({ "alwaysThinkingEnabled": true }));
    }

    #[test]
    fn blank_files_are_empty_and_strict_json_rejects_comments() {
        assert_eq!(parse("  \n", Format::Json).unwrap(), json!({}));
        assert!(parse("{ // no\n }", Format::Json).is_err());
        assert!(parse("{ // fine\n }", Format::Jsonc).is_ok());
        assert_eq!(parse("a = 1979-05-27\n", Format::Toml).unwrap(), json!({ "a": "1979-05-27" }));
    }

    #[test]
    fn writes_only_known_files_and_only_over_what_was_read() {
        let home = tmp("home");
        let project = tmp("project");
        let p = home.join(".codex/config.toml");
        let path = p.to_str().unwrap();
        // A missing file is created, folders and all.
        let text = set(&home, &project, path, "", &[edit("model", Some(json!("gpt")))]).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "model = \"gpt\"\n");
        // Changed on disk since: refused, file untouched.
        std::fs::write(&p, "model = \"other\"\n").unwrap();
        assert!(set(&home, &project, path, &text, &[edit("model", Some(json!("x")))]).unwrap_err().contains("changed on disk"));
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "model = \"other\"\n");
        // Broken TOML from the editor isn't written.
        assert!(save(&home, &project, path, "model = \"other\"\n", "model = ").is_err());
        // Not an agent settings file.
        assert!(save(&home, &project, home.join(".codex/auth.json").to_str().unwrap(), "", "{}").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn writes_through_symlinks_keeping_the_mode() {
        use std::os::unix::fs::PermissionsExt;
        let home = tmp("links");
        let project = tmp("links-project");
        let real = home.join("dotfiles/settings.json");
        std::fs::create_dir_all(real.parent().unwrap()).unwrap();
        std::fs::write(&real, "{}\n").unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        let link = home.join(".claude/settings.json");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        set(&home, &project, link.to_str().unwrap(), "{}\n", &[edit("model", Some(json!("opus")))]).unwrap();
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert!(std::fs::read_to_string(&real).unwrap().contains("\"model\": \"opus\""));
        assert_eq!(std::fs::metadata(&real).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn precedence_puts_claude_local_over_project_over_global() {
        let s = specs(Claude, Path::new("/h"), Path::new("/p"));
        let scopes: Vec<_> = s.iter().filter(|s| s.scope != Scope::Managed).map(|s| s.scope).collect();
        assert_eq!(scopes, [Scope::Local, Scope::Project, Scope::Global]);
        assert_eq!(specs(OpenCode, Path::new("/h"), Path::new("/p"))[1].path, Path::new("/h/.config/opencode/opencode.json"));
    }
}
