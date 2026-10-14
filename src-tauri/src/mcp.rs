//! MCP servers: entries in each agent's own config file, spelled six ways across JSON, JSONC and TOML. Unlike skills
//! there's no folder to symlink, so giving another agent a server translates it into that agent's file, and copies
//! are compared for drift. Some files are read by more than one agent (Grok reads Claude's `~/.claude.json`, Muse and
//! Grok read `.mcp.json`), so servers are listed per file with the agents reading it. Literal env and header values
//! (often tokens) stay on this side: the page gets one only when it asks `mcp_reveal`, and copies happen here.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::agent_config::{self, Edit, Format};
use crate::usage::Provider::{self, Antigravity, Claude, Codex, Grok, Muse, OpenCode};
use crate::{err, Res};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Project,
    /// Claude's private per-project servers, kept in `~/.claude.json`.
    Local,
    Global,
    /// Set by an admin: read-only here.
    Managed,
}

/// How a file spells its servers. `.mcp.json` and Claude's own files share Claude's.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Dialect {
    Claude,
    Codex,
    Grok,
    OpenCode,
    Antigravity,
    Muse,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    #[default]
    Stdio,
    Http,
    Sse,
    Ws,
}

/// A server in no agent's spelling. Variable references are `${NAME}` (OpenCode's `{env:NAME}` is converted);
/// empty strings stand for "not set".
#[derive(Clone, Debug, Default, PartialEq, Hash)]
struct Spec {
    transport: Transport,
    command: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
    cwd: String,
    url: String,
    headers: Vec<(String, String)>,
}

/// A file some agents read servers from.
#[derive(Clone, Debug)]
struct Src {
    path: PathBuf,
    /// Names down to the servers table: `mcpServers`, `projects.<folder>.mcpServers`, `mcp_servers`, `mcp`.
    table: Vec<String>,
    scope: Scope,
    dialect: Dialect,
    format: Format,
    /// The agents reading it, each with the file's rank among that agent's own (lower wins when both name a server).
    readers: Vec<(Provider, u8)>,
    label: Option<&'static str>,
}

fn src(path: PathBuf, table: &[&str], scope: Scope, dialect: Dialect, readers: &[(Provider, u8)]) -> Src {
    let format = match path.extension().and_then(|e| e.to_str()) {
        Some("toml") => Format::Toml,
        Some("jsonc") => Format::Jsonc,
        _ => Format::Json,
    };
    Src { path, table: table.iter().map(|s| s.to_string()).collect(), scope, dialect, format, readers: readers.to_vec(), label: None }
}

/// A file's parsed contents; None when it's missing, unreadable or doesn't parse.
fn load(path: &Path, format: Format) -> Option<Value> {
    agent_config::read(path).ok().and_then(|t| agent_config::parse(&t, format).ok())
}

fn dig<'a>(v: &'a Value, keys: &[impl AsRef<str>]) -> Option<&'a Value> {
    keys.iter().try_fold(v, |v, k| v.get(k.as_ref()))
}

// ponytail: paths and precedence as of each CLI's current release (Oct 2026). Not followed: env overrides (CODEX_HOME,
// OPENCODE_CONFIG...), Grok's cwd-to-git-root walk and its Cursor source, Muse's deeper .mcp.json files, plugin
// servers, claude.ai connectors. Unconfirmed: how Antigravity merges its two files, and Grok skipping .mcp.json once
// its Claude-import marker is set.
/// Every file the agents read servers from, for the project folder and the home folder.
fn sources(home: &Path, project: &Path) -> Vec<Src> {
    use Dialect as D;
    use Scope::*;
    let grok_toml = home.join(".grok/config.toml");
    let grok_claude = !matches!(load(&grok_toml, Format::Toml).as_ref().and_then(|v| dig(v, &["compat", "claude", "mcps"])), Some(Value::Bool(false)));
    let claude_user: &[(Provider, u8)] = if grok_claude { &[(Claude, 2), (Grok, 2)] } else { &[(Claude, 2)] };
    let muse = home.join(".config/muse/settings.json");
    let legacy = load(&muse, Format::Json).is_some_and(|v| v.get("mcp_servers").is_some() && v.get("mcpServers").is_none());
    let claude_json = home.join(".claude.json");
    let folder = project.to_string_lossy().into_owned();
    let oc = |dir: &Path| agent_config::json_or_jsonc(dir, "opencode");
    let mut out = vec![
        src(project.join(".mcp.json"), &["mcpServers"], Project, D::Claude, &[(Claude, 1), (Grok, 3), (Muse, 0)]),
        src(project.join(".codex/config.toml"), &["mcp_servers"], Project, D::Codex, &[(Codex, 0)]),
        src(project.join(".grok/config.toml"), &["mcp_servers"], Project, D::Grok, &[(Grok, 0)]),
        Src { format: Format::Jsonc, ..src(oc(project), &["mcp"], Project, D::OpenCode, &[(OpenCode, 0)]) },
        src(project.join(".agents/mcp_config.json"), &["mcpServers"], Project, D::Antigravity, &[(Antigravity, 0)]),
        Src {
            table: vec!["projects".into(), folder, "mcpServers".into()],
            label: Some("Just you"),
            ..src(claude_json.clone(), &[], Local, D::Claude, &[(Claude, 0)])
        },
        src(claude_json, &["mcpServers"], Global, D::Claude, claude_user),
        src(home.join(".codex/config.toml"), &["mcp_servers"], Global, D::Codex, &[(Codex, 1)]),
        src(grok_toml, &["mcp_servers"], Global, D::Grok, &[(Grok, 1)]),
        Src { format: Format::Jsonc, ..src(oc(&home.join(".config/opencode")), &["mcp"], Global, D::OpenCode, &[(OpenCode, 1)]) },
        src(home.join(".gemini/config/mcp_config.json"), &["mcpServers"], Global, D::Antigravity, &[(Antigravity, 1)]),
        src(muse, &[if legacy { "mcp_servers" } else { "mcpServers" }], Global, D::Muse, &[(Muse, 1)]),
    ];
    let managed = [
        (PathBuf::from("/Library/Application Support/ClaudeCode/managed-mcp.json"), "mcpServers", D::Claude, Claude),
        (PathBuf::from("/etc/codex/managed_config.toml"), "mcp_servers", D::Codex, Codex),
        (home.join(".codex/managed_config.toml"), "mcp_servers", D::Codex, Codex),
        (PathBuf::from("/etc/grok/managed_config.toml"), "mcp_servers", D::Grok, Grok),
        (home.join(".grok/managed_config.toml"), "mcp_servers", D::Grok, Grok),
    ];
    for (path, table, dialect, agent) in managed {
        if path.exists() {
            // Claude's managed file takes over; Codex's and Grok's are defaults under your own.
            let rank = if agent == Claude { 0 } else { 2 };
            out.push(Src { label: Some("Managed"), ..src(path, &[table], Managed, dialect, &[(agent, rank)]) });
        }
    }
    out
}

/// Where "all agents" puts a new server for a scope: the fewest files that reach every agent.
fn presets(all: &[Src], scope: Scope) -> Vec<&Src> {
    let wanted = |s: &&Src| match scope {
        // .mcp.json: Claude, Grok, Muse.
        Scope::Project => matches!(s.dialect, Dialect::Claude | Dialect::Codex | Dialect::OpenCode | Dialect::Antigravity),
        // ~/.claude.json: Claude, Grok.
        Scope::Global => s.dialect != Dialect::Grok,
        _ => false,
    };
    all.iter().filter(|s| s.scope == scope).filter(wanted).collect()
}

// ---- Reading each spelling

fn text_of(v: &Value) -> String {
    v.as_str().map(String::from).unwrap_or_else(|| if v.is_null() { String::new() } else { v.to_string() })
}
fn str_of(v: &Value, k: &str) -> String {
    v.get(k).map(text_of).unwrap_or_default()
}
fn strs(v: &Value, k: &str) -> Vec<String> {
    v.get(k).and_then(Value::as_array).into_iter().flatten().map(text_of).collect()
}
fn pairs(v: &Value, k: &str) -> Vec<(String, String)> {
    v.get(k).and_then(Value::as_object).into_iter().flatten().map(|(k, x)| (k.clone(), text_of(x))).collect()
}

/// The keys of an entry this page understands and rewrites; the rest are the agent's own and stay as they are.
fn modeled(d: Dialect) -> &'static [&'static str] {
    match d {
        Dialect::Claude => &["type", "command", "args", "env", "url", "headers"],
        Dialect::Muse => &["type", "transport", "command", "args", "env", "cwd", "url", "headers", "enabled"],
        Dialect::Codex => &["command", "args", "env", "env_vars", "cwd", "url", "bearer_token_env_var", "http_headers", "env_http_headers", "enabled"],
        Dialect::Grok => &["command", "args", "env", "url", "headers", "bearer_token_env_var", "transport", "type", "enabled"],
        Dialect::OpenCode => &["type", "command", "environment", "cwd", "url", "headers", "enabled"],
        Dialect::Antigravity => &["command", "args", "env", "cwd", "serverUrl", "url", "httpUrl", "headers", "disabled"],
    }
}

struct Read {
    spec: Spec,
    enabled: bool,
    problems: Vec<String>,
    /// Keys the page doesn't model (timeouts, OAuth, tool lists...).
    extra: Vec<String>,
}

fn read_entry(d: Dialect, v: &Value) -> Read {
    let mut s = Spec { command: str_of(v, "command"), args: strs(v, "args"), env: pairs(v, "env"), cwd: str_of(v, "cwd"), url: str_of(v, "url"), headers: pairs(v, "headers"), ..Default::default() };
    let mut problems = Vec::new();
    let mut enabled = v.get("enabled").and_then(Value::as_bool) != Some(false);
    let ty = str_of(v, "type");
    let remote = |s: &Spec| if s.url.is_empty() { Transport::Stdio } else { Transport::Http };
    match d {
        Dialect::Claude | Dialect::Muse => {
            let ty = if ty.is_empty() { str_of(v, "transport") } else { ty }; // Muse's legacy spelling
            s.transport = match ty.as_str() {
                "stdio" => Transport::Stdio,
                "http" | "streamable-http" | "streamable_http" => Transport::Http,
                "sse" => Transport::Sse,
                "ws" => Transport::Ws,
                "" => {
                    if d == Dialect::Claude && !s.url.is_empty() {
                        problems.push("Has a url but no type: Claude Code skips it (add \"type\": \"http\")".into());
                    }
                    remote(&s)
                }
                other => {
                    problems.push(format!("Unknown type \u{201c}{other}\u{201d}"));
                    remote(&s)
                }
            };
            if d == Dialect::Muse && matches!(s.transport, Transport::Sse | Transport::Ws) {
                problems.push("Muse Code connects only over stdio and streamable HTTP".into());
            }
        }
        Dialect::Codex | Dialect::Grok => {
            for e in v.get("env_vars").and_then(Value::as_array).into_iter().flatten() {
                let name = e.as_str().map(String::from).unwrap_or_else(|| str_of(e, "name"));
                s.env.push((name.clone(), format!("${{{name}}}")));
            }
            let bearer = str_of(v, "bearer_token_env_var");
            if !bearer.is_empty() {
                s.headers.push(("Authorization".into(), format!("Bearer ${{{bearer}}}")));
            }
            s.headers.extend(pairs(v, "http_headers"));
            s.headers.extend(pairs(v, "env_http_headers").into_iter().map(|(k, var)| (k, format!("${{{var}}}"))));
            let t = if ty.is_empty() { str_of(v, "transport") } else { ty };
            s.transport = if t == "sse" { Transport::Sse } else { remote(&s) };
        }
        Dialect::OpenCode => {
            let cmd = strs(v, "command");
            s.command = cmd.first().cloned().unwrap_or_default();
            s.args = cmd.into_iter().skip(1).collect();
            s.env = pairs(v, "environment");
            s.transport = match ty.as_str() {
                "local" => Transport::Stdio,
                "remote" => Transport::Http,
                _ => {
                    problems.push("OpenCode needs \"type\": \"local\" or \"remote\"".into());
                    remote(&s)
                }
            };
            let conv = |x: &mut String| *x = from_opencode(x);
            s.args.iter_mut().for_each(conv);
            s.env.iter_mut().chain(s.headers.iter_mut()).for_each(|(_, x)| conv(x));
            conv(&mut s.url);
        }
        Dialect::Antigravity => {
            s.url = str_of(v, "serverUrl");
            for old in ["url", "httpUrl"] {
                if v.get(old).is_some() {
                    problems.push(format!("Antigravity no longer reads \u{201c}{old}\u{201d}: rename it to serverUrl"));
                    if s.url.is_empty() {
                        s.url = str_of(v, old);
                    }
                }
            }
            s.transport = remote(&s);
            enabled = v.get("disabled").and_then(Value::as_bool) != Some(true);
        }
    }
    if s.command.is_empty() && s.url.is_empty() {
        problems.push("Has neither a command nor a URL".into());
    }
    let keys = modeled(d);
    let extra = v.as_object().into_iter().flatten().map(|(k, _)| k.clone()).filter(|k| !keys.contains(&k.as_str())).collect();
    Read { spec: s, enabled, problems, extra }
}

// ---- Variable references

/// The `${…}` references in `s`: (start, end, inside).
fn refs(s: &str) -> Vec<(usize, usize, &str)> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(a) = s[i..].find("${").map(|a| a + i) {
        let Some(b) = s[a..].find('}').map(|b| b + a) else { break };
        out.push((a, b + 1, &s[a + 2..b]));
        i = b + 1;
    }
    out
}

fn var_name(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// The variable when `s` is just `${NAME}`.
fn sole_ref(s: &str) -> Option<&str> {
    match refs(s)[..] {
        [(0, end, name)] if end == s.len() && var_name(name) => Some(name),
        _ => None,
    }
}

/// OpenCode's `{env:NAME}` as `${NAME}`.
fn from_opencode(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(a) = rest.find("{env:") {
        let Some(b) = rest[a..].find('}').map(|b| b + a) else { break };
        out.push_str(&rest[..a]);
        out.push_str(&format!("${{{}}}", &rest[a + 5..b]));
        rest = &rest[b + 1..];
    }
    out + rest
}

/// `${NAME}` as OpenCode's `{env:NAME}`; a `${NAME:-default}` loses its default.
fn to_opencode(s: &str, lost: &mut Vec<String>) -> String {
    let mut out = String::new();
    let mut at = 0;
    for (a, b, inner) in refs(s) {
        let name = inner.split_once(":-").map(|(n, _)| {
            lost.push(format!("the default in ${{{inner}}} (OpenCode has none)"));
            n
        });
        out.push_str(&s[at..a]);
        out.push_str(&format!("{{env:{}}}", name.unwrap_or(inner)));
        at = b;
    }
    out + &s[at..]
}

/// A value the page may show: a reference to a variable or file rather than a literal (which may be a token).
fn shown(v: &str) -> Option<String> {
    (v.is_empty() || !refs(v).is_empty() || v.contains("{file:")).then(|| v.to_string())
}

// ---- Writing each spelling

struct Built {
    fields: Vec<(String, Value)>,
    /// What the target can't hold.
    lost: Vec<String>,
    warnings: Vec<String>,
}

fn obj(pairs: &[(String, String)]) -> Value {
    Value::Object(pairs.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect())
}

fn strings(v: &[String]) -> Value {
    Value::Array(v.iter().cloned().map(Value::String).collect())
}

fn agent_name(d: Dialect) -> &'static str {
    match d {
        Dialect::Claude => "Claude Code",
        Dialect::Codex => "Codex",
        Dialect::Grok => "Grok Build",
        Dialect::OpenCode => "OpenCode",
        Dialect::Antigravity => "Antigravity",
        Dialect::Muse => "Muse Code",
    }
}

/// `s` spelled for `d`, with what doesn't fit. An error when the server can't work there at all.
fn build(d: Dialect, s: &Spec) -> Res<Built> {
    let mut f: Vec<(String, Value)> = Vec::new();
    let (mut lost, mut warnings) = (Vec::new(), Vec::new());
    let mut put = |k: &str, v: Value| f.push((k.to_string(), v));
    let stdio = s.transport == Transport::Stdio;
    let name = agent_name(d);
    match (d, s.transport) {
        (Dialect::Codex, Transport::Sse | Transport::Ws) => return Err("Codex connects only over stdio and streamable HTTP".into()),
        (Dialect::Muse, Transport::Sse | Transport::Ws) => return Err("Muse Code connects only over stdio and streamable HTTP".into()),
        (Dialect::Grok | Dialect::OpenCode, Transport::Ws) => return Err(format!("{name} doesn't connect over WebSocket")),
        _ => {}
    }
    match d {
        Dialect::Claude | Dialect::Muse => {
            let ty = match (d, s.transport) {
                (_, Transport::Stdio) => "stdio",
                (Dialect::Muse, _) => "streamable-http",
                (_, Transport::Http) => "http",
                (_, Transport::Sse) => "sse",
                (_, Transport::Ws) => "ws",
            };
            put("type", ty.into());
            if stdio {
                put("command", s.command.clone().into());
                if !s.args.is_empty() {
                    put("args", strings(&s.args));
                }
                if !s.env.is_empty() {
                    put("env", obj(&s.env));
                }
                if !s.cwd.is_empty() {
                    if d == Dialect::Muse { put("cwd", s.cwd.clone().into()) } else { lost.push("the working folder (cwd)".into()) }
                }
            } else {
                put("url", s.url.clone().into());
                if !s.headers.is_empty() {
                    put("headers", obj(&s.headers));
                }
            }
        }
        Dialect::Codex => {
            if stdio {
                put("command", s.command.clone().into());
                if !s.args.is_empty() {
                    put("args", strings(&s.args));
                }
                let (mut env, mut pass) = (Vec::new(), Vec::new());
                for (k, v) in &s.env {
                    match sole_ref(v) {
                        Some(var) if var == k => pass.push(k.clone()),
                        _ if refs(v).is_empty() => env.push((k.clone(), v.clone())),
                        _ => lost.push(format!("env {k} (Codex passes a variable on only under its own name)")),
                    }
                }
                if !env.is_empty() {
                    put("env", obj(&env));
                }
                if !pass.is_empty() {
                    put("env_vars", strings(&pass));
                }
                if !s.cwd.is_empty() {
                    put("cwd", s.cwd.clone().into());
                }
            } else {
                put("url", s.url.clone().into());
                let (mut fixed, mut from_env) = (Vec::new(), Vec::new());
                for (k, v) in &s.headers {
                    let bearer = v.strip_prefix("Bearer ").and_then(sole_ref);
                    if let (true, Some(var)) = (k.eq_ignore_ascii_case("authorization"), bearer) {
                        put("bearer_token_env_var", var.into());
                    } else if let Some(var) = sole_ref(v) {
                        from_env.push((k.clone(), var.to_string()));
                    } else if refs(v).is_empty() {
                        fixed.push((k.clone(), v.clone()));
                    } else {
                        lost.push(format!("header {k} (Codex takes a header from a variable only whole)"));
                    }
                }
                if !fixed.is_empty() {
                    put("http_headers", obj(&fixed));
                }
                if !from_env.is_empty() {
                    put("env_http_headers", obj(&from_env));
                }
            }
        }
        Dialect::Grok => {
            if stdio {
                put("command", s.command.clone().into());
                if !s.args.is_empty() {
                    put("args", strings(&s.args));
                }
                if !s.env.is_empty() {
                    put("env", obj(&s.env));
                }
                if !s.cwd.is_empty() {
                    lost.push("the working folder (cwd)".into());
                }
            } else {
                put("url", s.url.clone().into());
                if s.transport == Transport::Sse {
                    put("transport", "sse".into());
                }
                if !s.headers.is_empty() {
                    put("headers", obj(&s.headers));
                }
            }
        }
        Dialect::OpenCode => {
            let mut conv = |v: &str| to_opencode(v, &mut lost);
            if stdio {
                put("type", "local".into());
                let cmd: Vec<String> = std::iter::once(&s.command).chain(&s.args).map(|a| conv(a)).collect();
                put("command", strings(&cmd));
                if !s.env.is_empty() {
                    let env: Vec<_> = s.env.iter().map(|(k, v)| (k.clone(), conv(v))).collect();
                    put("environment", obj(&env));
                }
                if !s.cwd.is_empty() {
                    put("cwd", s.cwd.clone().into());
                }
            } else {
                put("type", "remote".into());
                put("url", conv(&s.url).into());
                if !s.headers.is_empty() {
                    let h: Vec<_> = s.headers.iter().map(|(k, v)| (k.clone(), conv(v))).collect();
                    put("headers", obj(&h));
                }
            }
        }
        Dialect::Antigravity => {
            if stdio {
                put("command", s.command.clone().into());
                if !s.args.is_empty() {
                    put("args", strings(&s.args));
                }
                if !s.env.is_empty() {
                    put("env", obj(&s.env));
                }
                if !s.cwd.is_empty() {
                    put("cwd", s.cwd.clone().into());
                }
            } else {
                put("serverUrl", s.url.clone().into());
                if !s.headers.is_empty() {
                    put("headers", obj(&s.headers));
                }
            }
            let all = s.args.iter().chain(s.env.iter().chain(&s.headers).map(|(_, v)| v)).chain([&s.url]);
            if all.into_iter().any(|v| !refs(v).is_empty()) {
                warnings.push("Antigravity doesn't document ${VAR} expansion: check the server connects".into());
            }
        }
    }
    Ok(Built { fields: f, lost, warnings })
}

/// The literal (hidden) values of `s`, by name.
fn literals(s: &Spec) -> Vec<String> {
    let env = s.env.iter().map(|(k, v)| (k, v, "env"));
    let headers = s.headers.iter().map(|(k, v)| (k, v, "header"));
    env.chain(headers).filter(|(_, v, _)| shown(v).is_none()).map(|(k, _, what)| format!("{what} {k}")).collect()
}

fn fingerprint(s: &Spec) -> String {
    let mut s = s.clone();
    s.env.sort();
    s.headers.sort();
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// The strictest rule among the agents (Grok's): a letter or underscore, then letters, digits, `_` and `-`.
pub fn valid_name(s: &str) -> bool {
    s.len() <= 64
        && s.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && !s.ends_with('_')
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

// ---- Scan

/// Whether each agent will load a project's servers (trust and approvals live in each agent's own files).
struct Trust {
    codex: bool,
    grok: bool,
    muse: bool,
    claude_all: bool,
    claude_on: Vec<String>,
    claude_off: Vec<String>,
    grok_off: Vec<String>,
}

fn trust(home: &Path, project: &Path) -> Trust {
    let p = project.to_string_lossy();
    let json = |path: PathBuf| load(&path, Format::Json).unwrap_or(Value::Null);
    let codex = load(&home.join(".codex/config.toml"), Format::Toml).unwrap_or(Value::Null);
    let grok_trust = load(&home.join(".grok/trusted_folders.toml"), Format::Toml).unwrap_or(Value::Null);
    let grok = load(&home.join(".grok/config.toml"), Format::Toml).unwrap_or(Value::Null);
    let muse = json(home.join(".config/muse/trust.json"));
    let claude = json(home.join(".claude.json"));
    let entry = claude.get("projects").and_then(|ps| ps.get(p.as_ref())).cloned().unwrap_or(Value::Null);
    let settings = [project.join(".claude/settings.local.json"), project.join(".claude/settings.json"), home.join(".claude/settings.json")].map(json);
    let claude_files = || std::iter::once(&entry).chain(&settings);
    let list = |k: &str| claude_files().flat_map(|v| strs(v, k)).collect::<Vec<_>>();
    Trust {
        codex: dig(&codex, &["projects", &p, "trust_level"]).and_then(Value::as_str) == Some("trusted"),
        grok: dig(&grok_trust, &["folders", &p, "trusted"]).and_then(Value::as_bool) == Some(true),
        muse: dig(&muse, &["projects", &p, "decision"]).and_then(Value::as_str) == Some("trusted"),
        claude_all: claude_files().any(|v| v.get("enableAllProjectMcpServers").and_then(Value::as_bool) == Some(true)),
        claude_on: list("enabledMcpjsonServers"),
        claude_off: list("disabledMcpjsonServers"),
        grok_off: strs(&grok, "disabled_mcp_servers"),
    }
}

#[derive(Debug, Serialize)]
pub struct Pair {
    key: String,
    /// None: a literal, hidden until revealed.
    value: Option<String>,
}

/// A spec as the page sees it: literals hidden.
#[derive(Debug, Serialize)]
pub struct Shown {
    transport: Transport,
    command: String,
    args: Vec<String>,
    env: Vec<Pair>,
    cwd: String,
    url: String,
    headers: Vec<Pair>,
}

fn show(s: &Spec) -> Shown {
    let hide = |p: &[(String, String)]| p.iter().map(|(k, v)| Pair { key: k.clone(), value: shown(v) }).collect();
    Shown { transport: s.transport, command: s.command.clone(), args: s.args.clone(), env: hide(&s.env), cwd: s.cwd.clone(), url: s.url.clone(), headers: hide(&s.headers) }
}

/// An agent that reads a definition but won't use it.
#[derive(Debug, Serialize)]
pub struct Block {
    agent: Provider,
    reason: String,
    /// The file whose definition it uses instead.
    by: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    name: String,
    path: String,
    table: Vec<String>,
    scope: Scope,
    dialect: Dialect,
    label: Option<&'static str>,
    agents: Vec<Provider>,
    blocked: Vec<Block>,
    spec: Shown,
    enabled: bool,
    extra: Vec<String>,
    problems: Vec<String>,
    /// Same for copies that define the same server, literals included.
    fingerprint: String,
    read_only: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    path: String,
    table: Vec<String>,
    scope: Scope,
    dialect: Dialect,
    label: Option<&'static str>,
    agents: Vec<Provider>,
    exists: bool,
    read_only: bool,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Preset {
    scope: Scope,
    targets: Vec<Target>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Target {
    path: String,
    table: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Scan {
    entries: Vec<Entry>,
    sources: Vec<Source>,
    presets: Vec<Preset>,
    home: String,
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn target(s: &Src) -> Target {
    Target { path: self::s(&s.path), table: s.table.clone() }
}

fn blocks(src: &Src, name: &str, r: &Read, t: &Trust) -> Vec<Block> {
    let block = |agent, reason: &str| Block { agent, reason: reason.into(), by: None };
    let mut out = Vec::new();
    for &(a, _) in &src.readers {
        let why = if !r.enabled {
            Some("Turned off")
        } else if a == Grok && !valid_name(name) {
            Some("Grok skips servers whose name doesn't start with a letter or holds anything but letters, digits, _ and -")
        } else if a == Grok && t.grok_off.iter().any(|n| n == name) {
            Some("Turned off in Grok")
        } else if src.scope != Scope::Project {
            None
        } else {
            match (a, src.dialect) {
                (Codex, _) if !t.codex => Some("Codex reads project settings only in trusted projects"),
                (Grok, Dialect::Grok) if !t.grok => Some("Grok applies project settings once the folder is trusted"),
                (Muse, _) if !t.muse => Some("Muse Code loads .mcp.json only in trusted workspaces"),
                (Muse, _) if matches!(r.spec.transport, Transport::Sse | Transport::Ws) => Some("Muse Code connects only over stdio and streamable HTTP"),
                (Claude, _) if t.claude_off.iter().any(|n| n == name) => Some("Turned off in Claude Code (/mcp turns it back on)"),
                (Claude, _) if !t.claude_all && !t.claude_on.iter().any(|n| n == name) => Some("Claude Code asks you to approve it on its next start"),
                _ => None,
            }
        };
        if let Some(why) = why {
            out.push(block(a, why));
        }
    }
    out
}

fn scan(home: &Path, project: &Path) -> Scan {
    let all = sources(home, project);
    let t = trust(home, project);
    let mut entries: Vec<(usize, Entry)> = Vec::new();
    let mut sources = Vec::new();
    for (i, src) in all.iter().enumerate() {
        let doc = agent_config::read(&src.path).and_then(|text| agent_config::parse(&text, src.format));
        let mut error = doc.as_ref().err().cloned();
        if src.dialect == Dialect::Muse && doc.as_ref().is_ok_and(|v| v.get("mcp_servers").is_some() && v.get("mcpServers").is_some()) {
            error = Some("Has both mcpServers and mcp_servers: Muse Code turns MCP off".into());
        }
        sources.push(Source {
            path: s(&src.path),
            table: src.table.clone(),
            scope: src.scope,
            dialect: src.dialect,
            label: src.label,
            agents: src.readers.iter().map(|r| r.0).collect(),
            exists: src.path.exists(),
            read_only: src.scope == Scope::Managed,
            error,
        });
        let Ok(doc) = doc else { continue };
        for (name, raw) in dig(&doc, &src.table).and_then(Value::as_object).into_iter().flatten() {
            let r = read_entry(src.dialect, raw);
            entries.push((
                i,
                Entry {
                    name: name.clone(),
                    path: s(&src.path),
                    table: src.table.clone(),
                    scope: src.scope,
                    dialect: src.dialect,
                    label: src.label,
                    agents: src.readers.iter().map(|r| r.0).collect(),
                    blocked: blocks(src, name, &r, &t),
                    spec: show(&r.spec),
                    enabled: r.enabled,
                    fingerprint: fingerprint(&r.spec),
                    extra: r.extra,
                    problems: r.problems,
                    read_only: src.scope == Scope::Managed,
                },
            ));
        }
    }
    // An agent reading two files that name the same server uses its higher-ranked file's.
    let rank = |i: usize, a: Provider| all[i].readers.iter().find(|r| r.0 == a).map(|r| r.1);
    let mut shadows = Vec::new();
    for (n, (i, e)) in entries.iter().enumerate() {
        for &a in &e.agents {
            let mine = rank(*i, a);
            let better = entries.iter().filter(|(j, o)| o.name == e.name && j != i).find(|(j, _)| rank(*j, a).is_some_and(|r| Some(r) < mine));
            if let Some((_, o)) = better {
                shadows.push((n, Block { agent: a, reason: "Uses another definition".into(), by: Some(o.path.clone()) }));
            }
        }
    }
    for (n, b) in shadows {
        let e = &mut entries[n].1;
        e.blocked.retain(|x| x.agent != b.agent);
        e.blocked.push(b);
    }
    let presets = [Scope::Project, Scope::Global].map(|scope| Preset { scope, targets: presets(&all, scope).into_iter().map(target).collect() }).into();
    Scan { entries: entries.into_iter().map(|e| e.1).collect(), sources, presets, home: s(home) }
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

/// Every MCP server the agents would load in `project` (the folder in the sidebar) and the home folder.
#[tauri::command]
pub async fn mcp_scan(project: String) -> Res<Scan> {
    tauri::async_runtime::spawn_blocking(move || scan(&home(), Path::new(&project))).await.map_err(err)
}

// ---- Changing files

/// One server in one file.
#[derive(Debug, Clone, Deserialize)]
pub struct Place {
    path: String,
    table: Vec<String>,
    name: String,
}

/// A spec from the page's form. A pair without a value keeps the value it has now (the page never saw it).
#[derive(Debug, Clone, Deserialize)]
pub struct SpecInput {
    transport: Transport,
    command: String,
    args: Vec<String>,
    env: Vec<PairInput>,
    cwd: String,
    url: String,
    headers: Vec<PairInput>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PairInput {
    key: String,
    value: Option<String>,
}

/// What a write did, or would do, to one file.
#[derive(Debug, Serialize)]
pub struct Report {
    path: String,
    table: Vec<String>,
    lost: Vec<String>,
    warnings: Vec<String>,
    error: Option<String>,
}

fn find(home: &Path, project: &Path, path: &str, table: &[String]) -> Res<Src> {
    sources(home, project).into_iter().find(|s| s.path == Path::new(path) && s.table == table).ok_or_else(|| format!("{path} isn't a file agents read MCP servers from"))
}

fn writable(home: &Path, project: &Path, path: &str, table: &[String]) -> Res<Src> {
    let s = find(home, project, path, table)?;
    if s.scope == Scope::Managed {
        return Err(format!("{path} is managed by an admin"));
    }
    Ok(s)
}

fn entry_key(src: &Src, name: &str, field: Option<&str>) -> Vec<String> {
    src.table.iter().cloned().chain([name.to_string()]).chain(field.map(String::from)).collect()
}

/// The raw entry `name` of `src` in `text`.
fn raw(src: &Src, text: &str, name: &str) -> Res<Option<Value>> {
    let doc = agent_config::parse(text, src.format)?;
    Ok(dig(&doc, &entry_key(src, name, None)).cloned())
}

/// `text` with `edits`, kept parseable; a new Muse settings file starts with its schema version.
fn apply(src: &Src, text: &str, edits: Vec<Edit>) -> Res<String> {
    let seed = (src.dialect == Dialect::Muse && text.trim().is_empty()).then(|| Edit { key: vec!["schema_version".into()], value: Some(1.into()) });
    let edits: Vec<Edit> = seed.into_iter().chain(edits).collect();
    let out = match src.format {
        Format::Toml => agent_config::edit_toml(text, &edits)?,
        Format::Json | Format::Jsonc => agent_config::edit_json(text, &edits)?,
    };
    agent_config::parse(&out, src.format)?;
    Ok(out)
}

/// Edits writing `fields` as entry `name`, removing the modelled keys of `current` that aren't among them.
fn entry_edits(src: &Src, name: &str, fields: &[(String, Value)], current: Option<&Value>) -> Vec<Edit> {
    let mut out: Vec<Edit> = fields.iter().map(|(k, v)| Edit { key: entry_key(src, name, Some(k)), value: Some(v.clone()) }).collect();
    let on = ["enabled", "disabled"];
    for k in current.and_then(Value::as_object).into_iter().flat_map(|o| o.keys()) {
        if modeled(src.dialect).contains(&k.as_str()) && !on.contains(&k.as_str()) && !fields.iter().any(|(f, _)| f == k) {
            out.push(Edit { key: entry_key(src, name, Some(k)), value: None });
        }
    }
    out
}

/// Texts to write, one per file, each with the text it was made from.
#[derive(Default)]
struct Writes(Vec<(PathBuf, String, String)>);

impl Writes {
    /// The file's text with the writes planned so far.
    fn text(&mut self, path: &Path) -> Res<&mut String> {
        if !self.0.iter().any(|w| w.0 == path) {
            let base = agent_config::read(path)?;
            self.0.push((path.to_path_buf(), base.clone(), base));
        }
        Ok(&mut self.0.iter_mut().find(|w| w.0 == path).unwrap().2)
    }
    /// Each file over what it held when read: none if one has changed since (the rest may be written already).
    fn commit(self) -> Res<()> {
        self.0.into_iter().filter(|(_, base, text)| base != text).try_for_each(|(path, base, text)| agent_config::write_checked(&path, &base, &text))
    }
}

/// Plan writing `spec` as `name` into each target. `extra`: the source's own keys, copied when the spelling matches.
fn plan(home: &Path, project: &Path, targets: &[Target], name: &str, spec: &Spec, extra: Option<(Dialect, &Map<String, Value>)>, replace: bool) -> (Vec<Report>, Writes) {
    let mut writes = Writes::default();
    let reports = targets
        .iter()
        .map(|t| {
            let mut r = Report { path: t.path.clone(), table: t.table.clone(), lost: vec![], warnings: vec![], error: None };
            let result = (|| -> Res<()> {
                let src = writable(home, project, &t.path, &t.table)?;
                let text = writes.text(&src.path)?;
                let current = raw(&src, text, name)?;
                if current.is_some() && !replace {
                    return Err(format!("Already has a server named \u{201c}{name}\u{201d}"));
                }
                let mut b = build(src.dialect, spec)?;
                match extra {
                    Some((d, keys)) if d == src.dialect => b.fields.extend(keys.iter().map(|(k, v)| (k.clone(), v.clone()))),
                    Some((_, keys)) if !keys.is_empty() => b.lost.push(format!("{} (settings of its own agent)", keys.keys().cloned().collect::<Vec<_>>().join(", "))),
                    _ => {}
                }
                let lits = literals(spec);
                if src.scope == Scope::Project && !lits.is_empty() {
                    b.warnings.push(format!("Writes {} as written into a project file that may be committed; ${{VAR}} keeps it out", lits.join(", ")));
                }
                if src.dialect == Dialect::Muse && !spec.cwd.is_empty() {
                    b.warnings.push("Muse Code drops cwd when it rewrites settings.json; .mcp.json keeps it".into());
                }
                *text = apply(&src, text, entry_edits(&src, name, &b.fields, current.as_ref()))?;
                (r.lost, r.warnings) = (b.lost, b.warnings);
                Ok(())
            })();
            r.error = result.err();
            r
        })
        .collect();
    (reports, writes)
}

/// Write the plan unless it's a dry run or a file failed: then nothing is written.
fn finish(reports: Vec<Report>, writes: Writes, dry_run: bool) -> Res<Vec<Report>> {
    if !dry_run && reports.iter().all(|r| r.error.is_none()) {
        writes.commit()?;
    }
    Ok(reports)
}

fn resolve(input: &SpecInput, current: Option<&Spec>) -> Res<Spec> {
    let pairs = |given: &[PairInput], now: Option<&Vec<(String, String)>>| -> Res<Vec<(String, String)>> {
        given
            .iter()
            .filter(|p| !p.key.trim().is_empty())
            .map(|p| {
                let key = p.key.trim().to_string();
                let value = match &p.value {
                    Some(v) => v.clone(),
                    None => now.and_then(|n| n.iter().find(|(k, _)| *k == key)).map(|(_, v)| v.clone()).ok_or_else(|| format!("No value for {key}"))?,
                };
                Ok((key, value))
            })
            .collect()
    };
    let stdio = input.transport == Transport::Stdio;
    let spec = Spec {
        transport: input.transport,
        command: if stdio { input.command.trim().into() } else { String::new() },
        args: if stdio { input.args.clone() } else { vec![] },
        env: if stdio { pairs(&input.env, current.map(|c| &c.env))? } else { vec![] },
        cwd: if stdio { input.cwd.trim().into() } else { String::new() },
        url: if stdio { String::new() } else { input.url.trim().into() },
        headers: if stdio { vec![] } else { pairs(&input.headers, current.map(|c| &c.headers))? },
    };
    match stdio {
        true if spec.command.is_empty() => Err("A stdio server needs a command".into()),
        false if spec.url.is_empty() => Err("A remote server needs a URL".into()),
        _ => Ok(spec),
    }
}

/// The entry at `p`, read.
fn read_place(home: &Path, project: &Path, p: &Place) -> Res<(Src, Value)> {
    let src = find(home, project, &p.path, &p.table)?;
    let text = agent_config::read(&src.path)?;
    let v = raw(&src, &text, &p.name)?.ok_or_else(|| format!("{} has no server named \u{201c}{}\u{201d} any more", p.path, p.name))?;
    Ok((src, v))
}

fn reveal(home: &Path, project: &Path, p: &Place, field: &str, key: &str) -> Res<String> {
    let (src, v) = read_place(home, project, p)?;
    let spec = read_entry(src.dialect, &v).spec;
    let list = if field == "headers" { spec.headers } else { spec.env };
    list.into_iter().find(|(k, _)| k == key).map(|(_, v)| v).ok_or_else(|| format!("No {key}"))
}

/// One literal env or header (`field`) value of a server, for the page to show on request.
#[tauri::command]
pub fn mcp_reveal(project: String, place: Place, field: String, key: String) -> Res<String> {
    reveal(&home(), Path::new(&project), &place, &field, &key)
}

fn add(home: &Path, project: &Path, targets: &[Target], name: &str, input: &SpecInput, dry_run: bool) -> Res<Vec<Report>> {
    if !valid_name(name) {
        return Err("Start with a letter; then letters, digits, - and _ (at most 64)".into());
    }
    let spec = resolve(input, None)?;
    let (reports, writes) = plan(home, project, targets, name, &spec, None, false);
    finish(reports, writes, dry_run)
}

/// New server `name` in each target file, spelled for its agent. A dry run reports what each file can't hold.
#[tauri::command]
pub fn mcp_add(project: String, targets: Vec<Target>, name: String, spec: SpecInput, dry_run: bool) -> Res<Vec<Report>> {
    add(&home(), Path::new(&project), &targets, &name, &spec, dry_run)
}

fn copy(home: &Path, project: &Path, from: &Place, targets: &[Target], replace: bool, dry_run: bool) -> Res<Vec<Report>> {
    let (src, v) = read_place(home, project, from)?;
    let r = read_entry(src.dialect, &v);
    let keys: Map<String, Value> = v.as_object().into_iter().flatten().filter(|(k, _)| r.extra.contains(k)).map(|(k, v)| (k.clone(), v.clone())).collect();
    let (reports, writes) = plan(home, project, targets, &from.name, &r.spec, Some((src.dialect, &keys)), replace);
    finish(reports, writes, dry_run)
}

/// Give more agents a server: translate the one at `from` into each target file. `replace` overwrites a server of
/// the same name there (bringing drifted copies back in line).
#[tauri::command]
pub fn mcp_copy(project: String, from: Place, targets: Vec<Target>, replace: bool, dry_run: bool) -> Res<Vec<Report>> {
    copy(&home(), Path::new(&project), &from, &targets, replace, dry_run)
}

fn update(home: &Path, project: &Path, p: &Place, input: &SpecInput) -> Res<Vec<Report>> {
    let (src, v) = read_place(home, project, p)?;
    let spec = resolve(input, Some(&read_entry(src.dialect, &v).spec))?;
    let t = Target { path: p.path.clone(), table: p.table.clone() };
    let (reports, writes) = plan(home, project, &[t], &p.name, &spec, None, true);
    finish(reports, writes, false)
}

/// Change one definition from the form; the agent's own keys (timeouts, OAuth...) stay.
#[tauri::command]
pub fn mcp_update(project: String, place: Place, spec: SpecInput) -> Res<Vec<Report>> {
    update(&home(), Path::new(&project), &place, &spec)
}

fn remove(home: &Path, project: &Path, places: &[Place]) -> Res<()> {
    let mut writes = Writes::default();
    for p in places {
        let src = writable(home, project, &p.path, &p.table)?;
        let text = writes.text(&src.path)?;
        *text = apply(&src, text, vec![Edit { key: entry_key(&src, &p.name, None), value: None }])?;
    }
    writes.commit()
}

/// Remove servers from their files.
#[tauri::command]
pub fn mcp_remove(project: String, places: Vec<Place>) -> Res<()> {
    remove(&home(), Path::new(&project), &places)
}

fn set_enabled(home: &Path, project: &Path, p: &Place, on: bool) -> Res<()> {
    let src = writable(home, project, &p.path, &p.table)?;
    let (key, value) = match src.dialect {
        Dialect::Claude => return Err("Claude Code turns servers on and off in /mcp".into()),
        Dialect::Antigravity => ("disabled", (!on).then_some(Value::Bool(true))),
        _ => ("enabled", (!on).then_some(Value::Bool(false))),
    };
    let mut writes = Writes::default();
    let text = writes.text(&src.path)?;
    if raw(&src, text, &p.name)?.is_none() {
        return Err(format!("{} has no server named \u{201c}{}\u{201d} any more", p.path, p.name));
    }
    *text = apply(&src, text, vec![Edit { key: entry_key(&src, &p.name, Some(key)), value }])?;
    // Grok also keeps a list of servers you turned off, in your own settings.
    if on && src.readers.iter().any(|r| r.0 == Grok) {
        let g = writable(home, project, &s(&home.join(".grok/config.toml")), &["mcp_servers".into()])?;
        let text = writes.text(&g.path)?;
        let off = load_str(text, Format::Toml).map(|v| strs(&v, "disabled_mcp_servers")).unwrap_or_default();
        if off.contains(&p.name) {
            let kept: Vec<String> = off.into_iter().filter(|n| *n != p.name).collect();
            *text = agent_config::edit_toml(text, &[Edit { key: vec!["disabled_mcp_servers".into()], value: Some(strings(&kept)) }])?;
        }
    }
    writes.commit()
}

fn load_str(text: &str, format: Format) -> Option<Value> {
    agent_config::parse(text, format).ok()
}

/// Turn a server on or off in its file (agents that have a switch for it).
#[tauri::command]
pub fn mcp_set_enabled(project: String, place: Place, on: bool) -> Res<()> {
    set_enabled(&home(), Path::new(&project), &place, on)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn base(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
        let b = std::env::temp_dir().join(format!("fc-mcp-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&b);
        std::fs::create_dir_all(b.join("home")).unwrap();
        std::fs::create_dir_all(b.join("proj")).unwrap();
        let b = std::fs::canonicalize(b).unwrap();
        (b.join("home"), b.join("proj"), b)
    }

    fn write(p: PathBuf, text: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn stdio(cmd: &str, args: &[&str], env: &[(&str, &str)]) -> Spec {
        Spec {
            command: cmd.into(),
            args: args.iter().map(|a| a.to_string()).collect(),
            env: env.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            ..Default::default()
        }
    }

    fn http(url: &str, headers: &[(&str, &str)]) -> Spec {
        Spec { transport: Transport::Http, url: url.into(), headers: headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(), ..Default::default() }
    }

    fn roundtrip(d: Dialect, s: &Spec) -> Spec {
        let b = build(d, s).unwrap();
        let v = Value::Object(b.fields.into_iter().collect());
        let mut back = read_entry(d, &v).spec;
        back.env.sort();
        back.headers.sort();
        back
    }

    #[test]
    fn every_spelling_round_trips() {
        let local = stdio("npx", &["-y", "pkg"], &[("TOKEN", "${TOKEN}"), ("MODE", "dev")]);
        let remote = http("https://x.dev/mcp", &[("Authorization", "Bearer ${API}"), ("X-Team", "${TEAM}"), ("X-Fixed", "a")]);
        for d in [Dialect::Claude, Dialect::Codex, Dialect::Grok, Dialect::OpenCode, Dialect::Antigravity, Dialect::Muse] {
            for s in [&local, &remote] {
                let mut want = s.clone();
                want.env.sort();
                want.headers.sort();
                assert_eq!(roundtrip(d, s), want, "{d:?}");
            }
        }
    }

    #[test]
    fn reads_each_agents_fields() {
        let oc = read_entry(Dialect::OpenCode, &json!({ "type": "local", "command": ["bun", "x", "{env:DIR}"], "environment": { "K": "{env:K}" }, "enabled": false }));
        assert_eq!((oc.spec.command.as_str(), oc.spec.args.clone(), oc.enabled), ("bun", vec!["x".to_string(), "${DIR}".into()], false));
        assert_eq!(oc.spec.env, [("K".into(), "${K}".into())]);
        let ag = read_entry(Dialect::Antigravity, &json!({ "serverUrl": "https://a", "disabled": true, "disabledTools": ["x"] }));
        assert_eq!((ag.spec.transport, ag.enabled, ag.extra.clone()), (Transport::Http, false, vec!["disabledTools".to_string()]));
        assert!(read_entry(Dialect::Antigravity, &json!({ "url": "https://a" })).problems[0].contains("serverUrl"));
        assert!(read_entry(Dialect::Claude, &json!({ "url": "https://a" })).problems[0].contains("no type"));
        let muse = read_entry(Dialect::Muse, &json!({ "transport": "streamable_http", "url": "https://m" }));
        assert_eq!(muse.spec.transport, Transport::Http);
        let codex = read_entry(Dialect::Codex, &json!({ "url": "https://c", "bearer_token_env_var": "T", "startup_timeout_sec": 20 }));
        assert_eq!(codex.spec.headers, [("Authorization".into(), "Bearer ${T}".into())]);
        assert_eq!(codex.extra, ["startup_timeout_sec"]);
    }

    #[test]
    fn reports_what_a_spelling_cannot_hold() {
        let s = stdio("x", &[], &[("GITHUB_TOKEN", "${GH}"), ("A", "pre-${B}")]);
        let b = build(Dialect::Codex, &s).unwrap();
        assert_eq!(b.lost.len(), 2);
        let sse = Spec { transport: Transport::Sse, url: "https://s".into(), ..Default::default() };
        assert!(build(Dialect::Codex, &sse).is_err());
        assert!(build(Dialect::Muse, &sse).is_err());
        assert!(build(Dialect::OpenCode, &sse).is_ok());
        assert!(!build(Dialect::Claude, &Spec { cwd: "/w".into(), ..stdio("x", &[], &[]) }).unwrap().lost.is_empty());
        assert_eq!(to_opencode("${A:-b}/x", &mut vec![]), "{env:A}/x");
        assert!(!build(Dialect::Antigravity, &http("https://${HOST}", &[])).unwrap().warnings.is_empty());
    }

    #[test]
    fn literals_are_hidden() {
        let s = stdio("x", &[], &[("TOKEN", "sk-secret"), ("REF", "${REF}"), ("FILE", "{file:~/t}")]);
        let shown = serde_json::to_string(&show(&s)).unwrap();
        assert!(!shown.contains("sk-secret"));
        assert!(shown.contains("${REF}") && shown.contains("{file:~/t}"));
        assert_eq!(literals(&s), ["env TOKEN"]);
        assert_ne!(fingerprint(&s), fingerprint(&stdio("x", &[], &[("TOKEN", "sk-other"), ("REF", "${REF}"), ("FILE", "{file:~/t}")])));
        assert_eq!(fingerprint(&stdio("x", &[], &[("A", "1"), ("B", "2")])), fingerprint(&stdio("x", &[], &[("B", "2"), ("A", "1")])));
    }

    #[test]
    fn names() {
        assert!(valid_name("github") && valid_name("_x-1"));
        for bad in ["", "1x", "a_", "a.b", "a b", &"a".repeat(65)] {
            assert!(!valid_name(bad), "{bad}");
        }
    }

    #[test]
    fn scans_who_sees_what() {
        let (home, proj, b) = base("scan");
        let p = s(&proj);
        write(home.join(".claude.json"), &json!({ "mcpServers": { "sentry": { "type": "http", "url": "https://s" } }, "projects": { &p: { "mcpServers": { "mine": { "type": "stdio", "command": "x" } } } } }).to_string());
        write(proj.join(".mcp.json"), r#"{ "mcpServers": { "pw": { "command": "npx", "args": ["pw"], "env": { "KEY": "sk-1" } } } }"#);
        write(home.join(".grok/config.toml"), "[mcp_servers.sentry]\nurl = \"https://grok\"\n");
        write(home.join(".codex/config.toml"), "[mcp_servers.pw]\ncommand = \"npx\"\nargs = [\"pw\"]\nenv = { KEY = \"sk-2\" }\n");
        let r = scan(&home, &proj);
        let by = |n: &str, scope: Scope| r.entries.iter().find(|e| e.name == n && e.scope == scope).unwrap_or_else(|| panic!("{n}"));

        let pw = by("pw", Scope::Project);
        assert_eq!(pw.agents, [Claude, Grok, Muse]);
        assert!(pw.spec.env[0].value.is_none());
        // Project servers wait for approval and trust.
        assert!(pw.blocked.iter().any(|b| b.agent == Claude && b.reason.contains("approve")));
        assert!(pw.blocked.iter().any(|b| b.agent == Muse));
        assert_ne!(pw.fingerprint, by("pw", Scope::Global).fingerprint); // KEY differs

        // Grok reads ~/.claude.json, but its own file wins on the same name.
        let sentry = by("sentry", Scope::Global);
        assert_eq!(sentry.agents.len(), 2);
        assert!(sentry.blocked.iter().any(|b| b.agent == Grok && b.by.as_deref().is_some_and(|p| p.ends_with(".grok/config.toml"))));
        assert_eq!(by("mine", Scope::Local).agents, [Claude]);

        // Trusted and approved: nothing waits.
        write(home.join(".config/muse/trust.json"), &json!({ "projects": { &p: { "decision": "trusted" } } }).to_string());
        write(proj.join(".claude/settings.local.json"), r#"{ "enabledMcpjsonServers": ["pw"] }"#);
        let r = scan(&home, &proj);
        assert!(r.entries.iter().find(|e| e.name == "pw" && e.scope == Scope::Project).unwrap().blocked.is_empty());

        // Grok can be told not to read Claude's servers.
        write(home.join(".grok/config.toml"), "[compat.claude]\nmcps = false\n");
        assert_eq!(scan(&home, &proj).entries.iter().find(|e| e.name == "sentry").unwrap().agents, [Claude]);
        std::fs::remove_dir_all(&b).unwrap();
    }

    fn input(s: &Spec) -> SpecInput {
        let pairs = |p: &[(String, String)]| p.iter().map(|(k, v)| PairInput { key: k.clone(), value: Some(v.clone()) }).collect();
        SpecInput { transport: s.transport, command: s.command.clone(), args: s.args.clone(), env: pairs(&s.env), cwd: s.cwd.clone(), url: s.url.clone(), headers: pairs(&s.headers) }
    }

    fn t(path: PathBuf, table: &[&str]) -> Target {
        Target { path: s(&path), table: table.iter().map(|x| x.to_string()).collect() }
    }

    #[test]
    fn adds_copies_updates_and_removes() {
        let (home, proj, b) = base("write");
        write(home.join(".codex/config.toml"), "# mine\nmodel = \"gpt\"\n");
        let r = scan(&home, &proj);
        let all = &r.presets.iter().find(|p| p.scope == Scope::Global).unwrap().targets;
        let spec = stdio("npx", &["-y", "gh"], &[("GITHUB_TOKEN", "${GITHUB_TOKEN}"), ("MODE", "x")]);

        // A dry run writes nothing.
        let reports = add(&home, &proj, all, "github", &input(&spec), true).unwrap();
        assert!(reports.iter().all(|r| r.error.is_none()));
        assert!(!home.join(".claude.json").exists());

        add(&home, &proj, all, "github", &input(&spec), false).unwrap();
        let codex = std::fs::read_to_string(home.join(".codex/config.toml")).unwrap();
        assert!(codex.starts_with("# mine\nmodel = \"gpt\"\n"));
        assert!(codex.contains("[mcp_servers.github]") && !codex.contains("[mcp_servers]\n"));
        assert!(codex.contains("env_vars = [\"GITHUB_TOKEN\"]"));
        let muse: Value = serde_json::from_str(&std::fs::read_to_string(home.join(".config/muse/settings.json")).unwrap()).unwrap();
        assert_eq!(muse["schema_version"], 1);
        let oc = std::fs::read_to_string(home.join(".config/opencode/opencode.json")).unwrap();
        assert!(oc.contains("{env:GITHUB_TOKEN}"));
        let r = scan(&home, &proj);
        let gh: Vec<_> = r.entries.iter().filter(|e| e.name == "github").collect();
        assert_eq!(gh.iter().flat_map(|e| e.agents.clone()).collect::<std::collections::HashSet<_>>().len(), 6);
        assert!(gh.windows(2).all(|w| w[0].fingerprint == w[1].fingerprint));

        // Adding again fails before anything is written.
        assert!(add(&home, &proj, all, "github", &input(&spec), false).unwrap().iter().all(|r| r.error.is_some()));

        // Copying keeps the source's own keys when the spelling matches, and reports them otherwise.
        let claude = t(home.join(".claude.json"), &["mcpServers"]);
        let from = Place { path: claude.path.clone(), table: claude.table.clone(), name: "sentry".into() };
        add(&home, &proj, &[claude.clone()], "sentry", &input(&http("https://s", &[("Authorization", "sk-lit")])), false).unwrap();
        let cj = home.join(".claude.json");
        let text = std::fs::read_to_string(&cj).unwrap();
        std::fs::write(&cj, agent_config::edit_json(&text, &[Edit { key: vec!["mcpServers".into(), "sentry".into(), "timeout".into()], value: Some(json!(5)) }]).unwrap()).unwrap();
        let reports = copy(&home, &proj, &from, &[t(proj.join(".mcp.json"), &["mcpServers"]), t(home.join(".grok/config.toml"), &["mcp_servers"])], false, false).unwrap();
        assert!(reports[0].lost.is_empty() && reports[0].warnings[0].contains("header Authorization"));
        assert!(reports[1].lost[0].contains("timeout"));
        let mcp: Value = serde_json::from_str(&std::fs::read_to_string(proj.join(".mcp.json")).unwrap()).unwrap();
        assert_eq!((mcp["mcpServers"]["sentry"]["timeout"].clone(), mcp["mcpServers"]["sentry"]["headers"]["Authorization"].clone()), (json!(5), json!("sk-lit")));
        assert_eq!(reveal(&home, &proj, &from, "headers", "Authorization").unwrap(), "sk-lit");

        // Updating keeps unseen values and the agent's own keys.
        let mut edit = input(&http("https://s2", &[]));
        edit.headers = vec![PairInput { key: "Authorization".into(), value: None }];
        update(&home, &proj, &from, &edit).unwrap();
        let v = raw(&find(&home, &proj, &from.path, &from.table).unwrap(), &std::fs::read_to_string(&cj).unwrap(), "sentry").unwrap().unwrap();
        assert_eq!(v, json!({ "type": "http", "url": "https://s2", "headers": { "Authorization": "sk-lit" }, "timeout": 5 }));

        // On and off, then gone.
        let gp = Place { path: s(&home.join(".grok/config.toml")), table: vec!["mcp_servers".into()], name: "sentry".into() };
        set_enabled(&home, &proj, &gp, false).unwrap();
        assert!(std::fs::read_to_string(home.join(".grok/config.toml")).unwrap().contains("enabled = false"));
        assert!(set_enabled(&home, &proj, &from, false).is_err());
        remove(&home, &proj, &[gp, from.clone()]).unwrap();
        assert!(scan(&home, &proj).entries.iter().all(|e| e.name != "sentry" || e.scope == Scope::Project));

        // Only the files agents read.
        assert!(add(&home, &proj, &[t(proj.join("x.json"), &["mcpServers"])], "z", &input(&spec), false).unwrap()[0].error.is_some());
        assert!(remove(&home, &proj, &[Place { path: s(&proj.join("x.json")), table: vec!["mcpServers".into()], name: "z".into() }]).is_err());
        std::fs::remove_dir_all(&b).unwrap();
    }
}
