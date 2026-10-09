//! Usage insights: token stats from the AI CLIs' local session logs, and subscription limits.
//! Claude Code and Codex logs carry exact token counts; Grok's come from its CLI (`grok usage`); Antigravity and OpenCode
//! keep theirs in SQLite (read with macOS's sqlite3; Antigravity's records are protobuf). Limits: Codex logs a snapshot
//! with every response; Claude's come from Anthropic's usage endpoint, Grok's and Antigravity's from their CLIs.
//! OpenCode doesn't expose its limits.

use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::{err, Res};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    #[default]
    Claude,
    Codex,
    Grok,
    Antigravity,
    #[serde(rename = "opencode")]
    OpenCode,
}

/// Tokens one session spent on one model within one hour (epoch ms of the hour's start).
/// `total` is what was processed: uncached + cached input + output.
#[derive(Clone, Default, Debug, PartialEq, Serialize)]
pub struct Row {
    provider: Provider,
    model: String,
    session: String,
    hour: i64,
    uncached: u64,
    cached: u64,
    output: u64,
    total: u64,
}

const HOUR: i64 = 3_600_000;

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64
}

/// "2026-10-09T02:18:07.762Z" / "...+00:00" -> epoch ms (UTC offsets only, which is all these logs write).
fn parse_ts(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    if b.len() < 19 || b[4] != b'-' || b[10] != b'T' {
        return None;
    }
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hh, mm, ss) = (num(11..13)?, num(14..16)?, num(17..19)?);
    let mut ms = 0;
    if b.get(19) == Some(&b'.') {
        let frac: String = s[20..].chars().take_while(|c| c.is_ascii_digit()).take(3).collect();
        ms = format!("{frac:0<3}").parse().ok()?;
    }
    // Days from civil (Howard Hinnant).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(((days * 24 + hh) * 60 + mm) * 60_000 + ss * 1000 + ms)
}

/// What we know about one log file, so the next scan only reads what was appended.
#[derive(Default)]
struct FileScan {
    len: u64,
    offset: u64, // end of the last complete line read
    rows: HashMap<(String, String, i64), Row>, // (session, model, hour)
    session: String,
    model: String,
    seen: HashSet<String>, // Claude: message+request ids (one response is logged once per content block)
    last: [u64; 3],        // Codex: cumulative input, cached, output so far
}

impl FileScan {
    fn add(&mut self, provider: Provider, model: &str, at: i64, uncached: u64, cached: u64, output: u64, total: u64) {
        let hour = at.div_euclid(HOUR) * HOUR;
        let key = (self.session.clone(), model.to_string(), hour);
        let r = self.rows.entry(key).or_insert_with(|| Row {
            provider,
            model: model.to_string(),
            session: self.session.clone(),
            hour,
            ..Default::default()
        });
        r.uncached += uncached;
        r.cached += cached;
        r.output += output;
        r.total += total;
    }

    fn line(&mut self, provider: Provider, line: &str) {
        match provider {
            Provider::Claude => self.claude(line),
            Provider::Codex => self.codex(line),
            Provider::Grok | Provider::Antigravity | Provider::OpenCode => {} // not line logs: see `collect`
        }
    }

    fn claude(&mut self, line: &str) {
        if !line.contains("\"usage\"") {
            return;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { return };
        let msg = &v["message"];
        let (Some(model), Some(at)) = (msg["model"].as_str(), v["timestamp"].as_str().and_then(parse_ts)) else { return };
        if model.starts_with('<') {
            return; // "<synthetic>": locally generated, no tokens
        }
        let id = format!("{}:{}", msg["id"].as_str().unwrap_or(""), v["requestId"].as_str().unwrap_or(""));
        if id != ":" && !self.seen.insert(id) {
            return;
        }
        if let Some(s) = v["sessionId"].as_str() {
            self.session = s.to_string();
        }
        let u = &msg["usage"];
        let n = |k: &str| u[k].as_u64().unwrap_or(0);
        let (input, write, read, out) = (n("input_tokens"), n("cache_creation_input_tokens"), n("cache_read_input_tokens"), n("output_tokens"));
        self.add(Provider::Claude, model, at, input + write, read, out, input + write + read + out);
    }

    fn codex(&mut self, line: &str) {
        let kind = if line.contains("\"token_count\"") {
            "token_count"
        } else if line.contains("\"turn_context\"") || line.contains("\"session_meta\"") {
            "meta"
        } else {
            return;
        };
        let Ok(v) = serde_json::from_str::<Value>(line) else { return };
        let p = &v["payload"];
        if kind == "meta" {
            if let Some(m) = p["model"].as_str() {
                self.model = m.to_string();
            }
            if v["type"] == "session_meta" {
                if let Some(id) = p["id"].as_str() {
                    self.session = id.to_string();
                }
            }
            return;
        }
        let t = &p["info"]["total_token_usage"];
        let Some(at) = v["timestamp"].as_str().and_then(parse_ts) else { return };
        if t.is_null() {
            return;
        }
        let n = |k: &str| t[k].as_u64().unwrap_or(0);
        // Totals are cumulative and repeated when nothing changed: count the growth. (They restart if a session is
        // resumed into a new log, which is a new file here anyway.)
        let now = [n("input_tokens"), n("cached_input_tokens"), n("output_tokens")];
        let d: Vec<u64> = now.iter().zip(self.last).map(|(a, b)| a.saturating_sub(b)).collect();
        self.last = now;
        if d.iter().all(|&x| x == 0) {
            return;
        }
        // input_tokens includes the cached part.
        let model = if self.model.is_empty() { "codex".to_string() } else { self.model.clone() };
        self.add(Provider::Codex, &model, at, d[0].saturating_sub(d[1]), d[1], d[2], d[0] + d[2]);
    }
}

/// Read what was appended to `path` since the last scan (all of it if it shrank, i.e. was rewritten).
fn scan(provider: Provider, path: &Path, len: u64, f: &mut FileScan) -> std::io::Result<()> {
    if len < f.offset {
        *f = FileScan { session: std::mem::take(&mut f.session), model: std::mem::take(&mut f.model), ..Default::default() };
    }
    let mut file = std::fs::File::open(path)?;
    file.seek(SeekFrom::Start(f.offset))?;
    let mut buf = Vec::new();
    file.take(len - f.offset).read_to_end(&mut buf)?;
    let Some(end) = buf.iter().rposition(|&b| b == b'\n') else {
        f.len = len;
        return Ok(()); // no complete line yet
    };
    for line in buf[..end].split(|&b| b == b'\n') {
        if let Ok(s) = std::str::from_utf8(line) {
            f.line(provider, s);
        }
    }
    f.offset += end as u64 + 1;
    f.len = len;
    Ok(())
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>, keep: &dyn Fn(&Path) -> bool) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => walk(&p, out, keep),
            Ok(t) if t.is_file() && keep(&p) => out.push(p),
            _ => {}
        }
    }
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

/// Every log file, by provider.
fn sources() -> Vec<(Provider, PathBuf)> {
    let h = home();
    let jsonl = |p: &Path| p.extension().is_some_and(|e| e == "jsonl");
    let mut out = Vec::new();
    for (provider, dir, keep) in [
        (Provider::Claude, h.join(".claude/projects"), &jsonl as &dyn Fn(&Path) -> bool),
        (Provider::Codex, h.join(".codex/sessions"), &jsonl),
        (Provider::Codex, h.join(".codex/archived_sessions"), &jsonl),
        (Provider::Grok, h.join(".grok/sessions"), &|p: &Path| p.file_name().is_some_and(|n| n == "updates.jsonl")),
    ] {
        let mut files = Vec::new();
        walk(&dir, &mut files, keep);
        out.extend(files.into_iter().map(|f| (provider, f)));
    }
    // One SQLite file per Antigravity conversation; one for all of OpenCode.
    let convs = std::fs::read_dir(h.join(".gemini/antigravity-cli/conversations")).into_iter().flatten().flatten();
    out.extend(convs.map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "db")).map(|p| (Provider::Antigravity, p)));
    let oc = h.join(".local/share/opencode/opencode.db");
    if oc.exists() {
        out.push((Provider::OpenCode, oc));
    }
    out
}

/// Rows of a read-only query, via macOS's sqlite3. The apps use WAL mode: while one has the file open, a read-only
/// connection shares its -shm; once it has closed (and removed that), only an immutable open works, which is safe as
/// nobody is writing then.
fn sqlite(db: &Path, sql: &str) -> Option<Vec<Value>> {
    let run = |args: &[&std::ffi::OsStr]| {
        std::process::Command::new("/usr/bin/sqlite3")
            .args(args)
            .arg(sql)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
    };
    let immutable = format!("file:{}?immutable=1", db.display()); // ponytail: paths with ? or # would need escaping
    let out = run(&["-readonly".as_ref(), "-json".as_ref(), db.as_os_str()])
        .or_else(|| run(&["-json".as_ref(), immutable.as_ref()]))?;
    if out.stdout.iter().all(u8::is_ascii_whitespace) {
        return Some(vec![]); // no rows: sqlite3 prints nothing
    }
    serde_json::from_slice(&out.stdout).ok()
}

/// SQLite writes land in the -wal file first: a database changed when either file did.
fn db_stamp(db: &Path) -> (u64, i64) {
    let mut wal = db.as_os_str().to_owned();
    wal.push("-wal");
    [db, Path::new(&wal)].iter().filter_map(|p| std::fs::metadata(p).ok()).fold((0, 0), |(len, at), m| {
        let t = m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as i64);
        (len + m.len(), at.max(t))
    })
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).filter_map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()).collect()
}

/// A protobuf field: just enough of the wire format to read Antigravity's records.
enum Pb<'a> {
    Int(u64),
    Bytes(&'a [u8]),
}

fn varint(b: &mut &[u8]) -> Option<u64> {
    let mut v = 0u64;
    for i in 0..10 {
        let (&x, rest) = b.split_first()?;
        *b = rest;
        v |= ((x & 0x7f) as u64) << (7 * i);
        if x < 0x80 {
            return Some(v);
        }
    }
    None
}

/// Top-level fields of a message, in order (fixed-width ones skipped).
fn pb_fields(mut b: &[u8]) -> Vec<(u64, Pb<'_>)> {
    let mut out = Vec::new();
    while let Some(key) = varint(&mut b) {
        let v = match key & 7 {
            0 => match varint(&mut b) {
                Some(v) => Pb::Int(v),
                None => break,
            },
            2 => match varint(&mut b).map(|n| n as usize).filter(|&n| n <= b.len()) {
                Some(n) => {
                    let (v, rest) = b.split_at(n);
                    b = rest;
                    Pb::Bytes(v)
                }
                None => break,
            },
            1 | 5 => {
                let n = if key & 7 == 1 { 8 } else { 4 };
                match b.get(n..) {
                    Some(rest) => b = rest,
                    None => break,
                }
                continue;
            }
            _ => break,
        };
        out.push((key >> 3, v));
    }
    out
}

fn pb_msg(b: &[u8], field: u64) -> Option<&[u8]> {
    pb_fields(b).into_iter().find_map(|(f, v)| match v {
        Pb::Bytes(x) if f == field => Some(x),
        _ => None,
    })
}

fn pb_int(b: &[u8], field: u64) -> u64 {
    pb_fields(b).into_iter().find_map(|(f, v)| match v {
        Pb::Int(x) if f == field => Some(x),
        _ => None,
    }).unwrap_or(0)
}

/// An Antigravity conversation: one generation record per model call. In each, field 1 holds the call: its usage
/// (4: 2 input, 3 output, 4 cache writes, 5 cache reads; input excludes the cached part), model (19) and string
/// metadata (20) whose "last_step_index" points at the step it answered, which carries the time (1.1, epoch s).
fn antigravity_rows(f: &mut FileScan, db: &Path, fallback_at: i64) {
    let (Some(gens), Some(steps)) = (
        sqlite(db, "select hex(data) d from gen_metadata"),
        sqlite(db, "select idx, hex(metadata) m from steps"),
    ) else { return };
    let step_at: HashMap<u64, i64> = steps
        .iter()
        .filter_map(|s| {
            let m = unhex(s["m"].as_str()?);
            let secs = pb_int(pb_msg(&m, 1)?, 1);
            let idx = s["idx"].as_u64()?;
            (secs > 0).then_some((idx, secs as i64 * 1000))
        })
        .collect();
    f.rows.clear();
    for g in &gens {
        let d = unhex(g["d"].as_str().unwrap_or(""));
        let Some(call) = pb_msg(&d, 1) else { continue };
        let Some(u) = pb_msg(call, 4) else { continue };
        let model = pb_msg(call, 19).map_or("antigravity".into(), |m| String::from_utf8_lossy(m).into_owned());
        let step = pb_fields(call).into_iter().find_map(|(field, v)| match v {
            Pb::Bytes(kv) if field == 20 && pb_msg(kv, 1) == Some(b"last_step_index".as_slice()) => {
                std::str::from_utf8(pb_msg(kv, 2)?).ok()?.parse::<u64>().ok()
            }
            _ => None,
        });
        let at = step.and_then(|i| step_at.get(&i).copied()).unwrap_or(fallback_at);
        let (input, output, write, read) = (pb_int(u, 2), pb_int(u, 3), pb_int(u, 4), pb_int(u, 5));
        f.add(Provider::Antigravity, &model, at, input + write, read, output, input + write + read + output);
    }
}

/// OpenCode: every assistant message records its tokens (input excludes cache reads/writes; reasoning is on top of
/// output).
fn opencode_rows(f: &mut FileScan, db: &Path) {
    let Some(msgs) = sqlite(
        db,
        "select session_id s, time_created t, json_extract(data,'$.modelID') m, json_extract(data,'$.tokens') k \
         from message where json_extract(data,'$.role') = 'assistant'",
    ) else { return };
    f.rows.clear();
    for m in &msgs {
        let Some(k) = m["k"].as_str().and_then(|k| serde_json::from_str::<Value>(k).ok()) else { continue };
        let n = |v: &Value| v.as_u64().unwrap_or(0);
        let (input, output, write, read) = (n(&k["input"]), n(&k["output"]) + n(&k["reasoning"]), n(&k["cache"]["write"]), n(&k["cache"]["read"]));
        f.session = m["s"].as_str().unwrap_or("").to_string();
        f.add(Provider::OpenCode, m["m"].as_str().unwrap_or("opencode"), m["t"].as_i64().unwrap_or(0), input + write, read, output, input + write + read + output);
    }
}

static CACHE: Mutex<Option<HashMap<PathBuf, FileScan>>> = Mutex::new(None);

/// The Grok CLI: where its installer puts it, else whatever the login shell's PATH finds.
fn grok_cmd() -> std::process::Command {
    let local = home().join(".grok/bin/grok");
    let mut c = std::process::Command::new(if local.exists() { local.into_os_string() } else { "grok".into() });
    c.env_clear().envs(crate::clean_env()).env("PATH", crate::login_path()).stdin(std::process::Stdio::null());
    c
}

/// A session's exact per-turn tokens, from `grok usage <session>` (its documented way in; input includes the cached
/// part). Replaces what we had for it. A session with no turns yet prints an error, i.e. no rows.
fn grok_rows(f: &mut FileScan, usage: &Value) {
    f.rows.clear();
    for t in usage["turns"].as_array().into_iter().flatten() {
        let Some(at) = t["endedAt"].as_str().and_then(parse_ts) else { continue };
        let per_model: Vec<(&str, &Value)> = match t["modelUsage"].as_object() {
            Some(m) if !m.is_empty() => m.iter().map(|(k, v)| (k.as_str(), v)).collect(),
            _ => vec![(t["primaryModelId"].as_str().unwrap_or("grok"), t)],
        };
        for (model, u) in per_model {
            let n = |k: &str| u[k].as_u64().unwrap_or(0);
            let (input, cached, output) = (n("inputTokens"), n("cachedReadTokens"), n("outputTokens"));
            f.add(Provider::Grok, model, at, input.saturating_sub(cached), cached, output, input + output);
        }
    }
}

fn collect(since: i64) -> Vec<Row> {
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = guard.get_or_insert_with(HashMap::new);
    let mut rows = Vec::new();
    for (provider, path) in sources() {
        let Ok(meta) = std::fs::metadata(&path) else { continue };
        // For the databases `len` is a stamp (both files' sizes and the newest mtime), so any write changes it.
        let (len, modified) = match provider {
            Provider::Antigravity | Provider::OpenCode => {
                let (len, at) = db_stamp(&path);
                (len ^ at as u64, at)
            }
            _ => (meta.len(), meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as i64)),
        };
        if modified < since {
            continue; // nothing in it can be in range
        }
        let f = cache.entry(path.clone()).or_default();
        if len != f.len {
            if provider == Provider::Antigravity {
                f.session = path.file_stem().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                antigravity_rows(f, &path, modified);
                f.len = len;
            } else if provider == Provider::OpenCode {
                opencode_rows(f, &path);
                f.len = len;
            } else if provider == Provider::Grok {
                // The session changed: ask the CLI for its totals again.
                f.session = path.parent().and_then(|p| p.file_name()).map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                let Ok(out) = grok_cmd().args(["usage", &f.session]).output() else { continue };
                grok_rows(f, &serde_json::from_slice(&out.stdout).unwrap_or(Value::Null));
                f.len = meta.len();
            } else {
                if provider == Provider::Codex && f.session.is_empty() {
                    f.session = path.file_stem().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                }
                if scan(provider, &path, meta.len(), f).is_err() {
                    continue;
                }
            }
        }
        rows.extend(f.rows.values().filter(|r| r.hour + HOUR > since).cloned());
    }
    rows
}

/// Hourly token rows for the last `days` days (the UI buckets them into local days).
#[tauri::command]
pub async fn usage_stats(days: u32) -> Res<Vec<Row>> {
    let since = now_ms() - (days as i64 + 1) * 24 * HOUR; // a day extra: the UI cuts at local midnight
    tauri::async_runtime::spawn_blocking(move || collect(since)).await.map_err(err)
}

/// One limit bar. `used` is a percentage; `resets_at` epoch ms.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit {
    provider: Provider,
    label: String,
    used: f64,
    resets_at: Option<i64>,
    window_minutes: Option<i64>,
    note: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Limits {
    limits: Vec<Limit>,
    errors: Vec<(Provider, String)>,
}

/// Claude's limits from Anthropic's usage endpoint (the one Claude Code's /usage shows), as the logged-in user.
fn claude_limits() -> Result<Vec<Limit>, String> {
    let out = std::process::Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", "Claude Code-credentials", "-w"])
        .output()
        .map_err(err)?;
    if !out.status.success() {
        return Err("Not logged in to Claude Code".into());
    }
    let creds: Value = serde_json::from_slice(&out.stdout).map_err(|_| "Couldn't read the Claude Code login".to_string())?;
    let oauth = &creds["claudeAiOauth"];
    let token = oauth["accessToken"].as_str().ok_or("Not logged in to Claude Code")?;
    // Refreshing it here would rotate the refresh token and log Claude Code out; let Claude Code do it.
    if oauth["expiresAt"].as_i64().is_some_and(|t| t < now_ms()) {
        return Err("Claude Code login expired. Run `claude` to refresh it".into());
    }
    let body = ureq::get("https://api.anthropic.com/api/oauth/usage")
        .header("Authorization", &format!("Bearer {token}"))
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("User-Agent", "fleet-command")
        .config()
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .call()
        .map_err(|e| match e {
            ureq::Error::StatusCode(401) => "Claude Code login expired. Run `claude` to refresh it".to_string(),
            e => format!("Couldn't reach Anthropic: {e}"),
        })?
        .body_mut()
        .read_to_string()
        .map_err(err)?;
    parse_claude(&serde_json::from_str(&body).map_err(err)?)
}

fn parse_claude(v: &Value) -> Result<Vec<Limit>, String> {
    let ts = |x: &Value| x.as_str().and_then(parse_ts);
    let limit = |label: String, used: f64, resets: &Value, window: i64| Limit {
        provider: Provider::Claude,
        label,
        used,
        resets_at: ts(resets),
        window_minutes: Some(window),
        note: None,
    };
    if let Some(list) = v["limits"].as_array() {
        return Ok(list
            .iter()
            .filter_map(|l| {
                let window = if l["group"] == "session" { 300 } else { 7 * 24 * 60 };
                let label = match (l["group"].as_str()?, l["scope"]["model"]["display_name"].as_str()) {
                    ("session", _) => "Session".to_string(),
                    ("weekly", Some(m)) => format!("Weekly · {m}"),
                    ("weekly", None) => "Weekly".to_string(),
                    (g, _) => g[..1].to_uppercase() + &g[1..],
                };
                Some(limit(label, l["percent"].as_f64()?, &l["resets_at"], window))
            })
            .collect());
    }
    // Older shape.
    let mut out = Vec::new();
    for (key, label, window) in [("five_hour", "Session", 300), ("seven_day", "Weekly", 10080), ("seven_day_opus", "Weekly · Opus", 10080), ("seven_day_sonnet", "Weekly · Sonnet", 10080)] {
        if let Some(used) = v[key]["utilization"].as_f64() {
            out.push(limit(label.into(), used, &v[key]["resets_at"], window));
        }
    }
    if out.is_empty() {
        return Err("Unexpected response from Anthropic".into());
    }
    Ok(out)
}

/// Codex logs its rate limits with every response: use the newest snapshot.
fn codex_limits() -> Result<Vec<Limit>, String> {
    let mut files = Vec::new();
    walk(&home().join(".codex/sessions"), &mut files, &|p| p.extension().is_some_and(|e| e == "jsonl"));
    let mut files: Vec<_> = files.into_iter().filter_map(|p| Some((std::fs::metadata(&p).ok()?.modified().ok()?, p))).collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    for (_, path) in files.iter().take(20) {
        let Ok(mut file) = std::fs::File::open(path) else { continue };
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        let _ = file.seek(SeekFrom::Start(len.saturating_sub(4 << 20)));
        let mut buf = Vec::new();
        if file.read_to_end(&mut buf).is_err() {
            continue;
        }
        let text = String::from_utf8_lossy(&buf);
        for line in text.lines().rev().filter(|l| l.contains("\"rate_limits\"")) {
            if let Some(l) = serde_json::from_str::<Value>(line).ok().and_then(|v| parse_codex(&v["payload"]["rate_limits"], now_ms())) {
                return Ok(l);
            }
        }
    }
    Err("No Codex sessions yet".into())
}

fn parse_codex(r: &Value, now: i64) -> Option<Vec<Limit>> {
    let mut out = Vec::new();
    for w in [&r["primary"], &r["secondary"]] {
        let Some(used) = w["used_percent"].as_f64() else { continue };
        let window = w["window_minutes"].as_i64();
        let label = match window {
            Some(300) => "Session".to_string(),
            Some(10080) => "Weekly".to_string(),
            Some(m) if m % 1440 == 0 => format!("{}d", m / 1440),
            Some(m) => format!("{}h", m / 60),
            None => "Limit".to_string(),
        };
        let resets_at = w["resets_at"].as_i64().map(|s| s * 1000);
        // The snapshot is from the last Codex response; a window that has rolled over since is back to full.
        let rolled = resets_at.is_some_and(|t| t <= now);
        let credits = &r["credits"];
        let note = if rolled {
            Some("Reset since last use".to_string())
        } else if credits["has_credits"] == true && credits["unlimited"] != true {
            credits["balance"].as_str().map(|b| format!("{b} credits"))
        } else {
            None
        };
        out.push(Limit {
            provider: Provider::Codex,
            label,
            used: if rolled { 0.0 } else { used },
            resets_at: if rolled { None } else { resets_at },
            window_minutes: window,
            note,
        });
    }
    (!out.is_empty()).then_some(out)
}

/// Grok's allowance, as its TUI's /usage shows it: asked of the CLI's agent mode (ACP over stdio) with the
/// `_x.ai/billing` extension. Undocumented, so a CLI update may break it; then the card shows the error.
fn grok_limits() -> Result<Vec<Limit>, String> {
    if !home().join(".grok").exists() {
        return Ok(vec![]); // not installed: no card
    }
    let mut child = grok_cmd()
        .args(["agent", "stdio"])
        .current_dir(std::env::temp_dir())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Couldn't start grok: {e}"))?;
    let (mut stdin, stdout) = (child.stdin.take().unwrap(), child.stdout.take().unwrap());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            match serde_json::from_str::<Value>(&line) {
                Ok(v) if v["id"] == 2 => return drop(tx.send(v)),
                _ => {} // the initialize reply, notifications
            }
        }
    });
    let asked = writeln!(stdin, r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":1,"clientCapabilities":{{}}}}}}"#)
        .and_then(|_| writeln!(stdin, r#"{{"jsonrpc":"2.0","id":2,"method":"_x.ai/billing","params":{{}}}}"#));
    let reply = asked.ok().and_then(|_| rx.recv_timeout(Duration::from_secs(20)).ok());
    let _ = child.kill();
    let _ = child.wait();
    let v = reply.ok_or("Grok didn't answer")?;
    if let Some(e) = v["error"]["message"].as_str() {
        return Err(format!("Grok: {e}"));
    }
    parse_grok(&v["result"]).ok_or_else(|| "Unexpected reply from Grok".into())
}

fn parse_grok(r: &Value) -> Option<Vec<Limit>> {
    let c = &r["config"];
    let used = c["creditUsagePercent"].as_f64()?;
    let period = &c["currentPeriod"];
    let (start, end) = (period["start"].as_str().and_then(parse_ts), period["end"].as_str().and_then(parse_ts));
    let label = match period["type"].as_str().unwrap_or("") {
        t if t.ends_with("DAILY") => "Daily",
        t if t.ends_with("WEEKLY") => "Weekly",
        t if t.ends_with("MONTHLY") => "Monthly",
        _ => "Credits",
    };
    Some(vec![Limit {
        provider: Provider::Grok,
        label: label.into(),
        used,
        resets_at: end,
        window_minutes: start.zip(end).map(|(s, e)| (e - s) / 60_000),
        note: None,
    }])
}

/// Antigravity's quotas, from its CLI's read-only `/usage` (answered locally: no model turn, no conversation).
/// Model groups share a limit; buckets report what's *remaining*.
fn antigravity_limits() -> Result<Vec<Limit>, String> {
    if !home().join(".gemini/antigravity-cli").exists() {
        return Ok(vec![]); // not installed: no card
    }
    let local = home().join(".local/bin/agy");
    let mut cmd = std::process::Command::new(if local.exists() { local.into_os_string() } else { "agy".into() });
    cmd.args(["-p", "/usage", "--output-format", "json"])
        .env_clear()
        .envs(crate::clean_env())
        .env("PATH", crate::login_path())
        .current_dir(std::env::temp_dir())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let out = output_within(cmd, Duration::from_secs(30)).ok_or("Antigravity didn't answer")?;
    let v: Value = serde_json::from_slice(&out.stdout).map_err(|_| {
        let msg = if out.stderr.is_empty() { &out.stdout } else { &out.stderr };
        let msg = String::from_utf8_lossy(msg);
        format!("Antigravity: {}", msg.lines().next().unwrap_or("no reply").trim_start_matches("error: "))
    })?;
    parse_antigravity(&v).ok_or_else(|| "Unexpected reply from Antigravity".into())
}

fn parse_antigravity(v: &Value) -> Option<Vec<Limit>> {
    let groups = v["command"]["data"]["groups"].as_array()?;
    let mut out = Vec::new();
    for g in groups {
        // "Gemini Models" -> "Gemini", "Claude and GPT models" -> "Claude & GPT"
        let name = g["name"].as_str().unwrap_or("");
        let name = name.trim_end_matches(" Models").trim_end_matches(" models").replace(" and ", " & ");
        for b in g["buckets"].as_array().into_iter().flatten() {
            let Some(left) = b["remaining_fraction"].as_f64() else { continue };
            let (window, minutes) = match b["window"].as_str().unwrap_or("") {
                "daily" => ("Daily", Some(1440)),
                "weekly" => ("Weekly", Some(10080)),
                "monthly" => ("Monthly", None),
                w if !w.is_empty() => (w, None),
                _ => ("Limit", None),
            };
            let window = window[..1].to_uppercase() + &window[1..];
            out.push(Limit {
                provider: Provider::Antigravity,
                label: if name.is_empty() { window } else { format!("{window} · {name}") },
                used: ((1.0 - left) * 100.0).clamp(0.0, 100.0),
                resets_at: b["reset_time"].as_str().and_then(parse_ts),
                window_minutes: minutes,
                note: None,
            });
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Run `cmd` (stdout/stderr piped), giving up and killing it after `limit`.
fn output_within(mut cmd: std::process::Command, limit: Duration) -> Option<std::process::Output> {
    let child = cmd.spawn().ok()?;
    let pid = child.id();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || drop(tx.send(child.wait_with_output())));
    match rx.recv_timeout(limit) {
        Ok(out) => out.ok(),
        Err(_) => {
            let _ = std::process::Command::new("kill").arg(pid.to_string()).status();
            None
        }
    }
}

type Cached = Mutex<Option<(Instant, Result<Vec<Limit>, String>)>>;
static CLAUDE: Cached = Mutex::new(None);
static GROK: Cached = Mutex::new(None);
static ANTIGRAVITY: Cached = Mutex::new(None);

/// `fetch`'s last result if younger than `ttl` (and not `force`d), else a fresh one.
fn cached(slot: &Cached, ttl: u64, force: bool, fetch: fn() -> Result<Vec<Limit>, String>) -> Result<Vec<Limit>, String> {
    let mut c = slot.lock().unwrap_or_else(|e| e.into_inner());
    match &*c {
        Some((at, r)) if !force && at.elapsed() < Duration::from_secs(ttl) => r.clone(),
        _ => {
            let r = fetch();
            *c = Some((Instant::now(), r.clone()));
            r
        }
    }
}

/// Subscription limits. Claude's are fetched at most once a minute, Grok's and Antigravity's every 5 (they start a
/// process) unless `force`d; Codex's are read from its logs each time.
#[tauri::command]
pub async fn usage_limits(force: bool) -> Res<Limits> {
    tauri::async_runtime::spawn_blocking(move || {
        let failed = |_| Err("Check failed".to_string());
        let (claude, grok, antigravity) = std::thread::scope(|s| {
            let grok = s.spawn(|| cached(&GROK, 300, force, grok_limits));
            let antigravity = s.spawn(|| cached(&ANTIGRAVITY, 300, force, antigravity_limits));
            (cached(&CLAUDE, 60, force, claude_limits), grok.join().unwrap_or_else(failed), antigravity.join().unwrap_or_else(failed))
        });
        let mut out = Limits { limits: vec![], errors: vec![] };
        let all = [(Provider::Claude, claude), (Provider::Codex, codex_limits()), (Provider::Grok, grok), (Provider::Antigravity, antigravity)];
        for (p, r) in all {
            match r {
                Ok(l) => out.limits.extend(l),
                Err(e) => out.errors.push((p, e)),
            }
        }
        out
    })
    .await
    .map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_timestamps() {
        assert_eq!(parse_ts("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_ts("2026-10-09T02:18:07.762Z"), Some(1_791_512_287_762));
        assert_eq!(parse_ts("2026-10-14T03:00:00.451688+00:00"), Some(1_791_946_800_451));
        assert_eq!(parse_ts("2000-02-29T12:00:00Z"), Some(951_825_600_000));
        assert_eq!(parse_ts("nope"), None);
    }

    fn claude_line(id: &str, out: u64) -> String {
        format!(
            r#"{{"sessionId":"s1","requestId":"r","timestamp":"2026-10-09T02:18:07.762Z","message":{{"id":"{id}","model":"claude-opus-5-5","usage":{{"input_tokens":2,"cache_creation_input_tokens":10,"cache_read_input_tokens":100,"output_tokens":{out}}}}}}}"#
        )
    }

    #[test]
    fn claude_counts_each_response_once() {
        let mut f = FileScan::default();
        f.line(Provider::Claude, &claude_line("m1", 5));
        f.line(Provider::Claude, &claude_line("m1", 5)); // same response, next content block
        f.line(Provider::Claude, &claude_line("m2", 7));
        let rows: Vec<_> = f.rows.values().collect();
        assert_eq!(rows.len(), 1);
        let r = rows[0];
        assert_eq!((r.uncached, r.cached, r.output, r.total), (24, 200, 12, 236));
        assert_eq!(r.session, "s1");
        assert_eq!(r.hour, 1_791_511_200_000);
    }

    fn codex_count(ts: &str, input: u64, cached: u64, output: u64) -> String {
        format!(
            r#"{{"timestamp":"{ts}","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":{input},"cached_input_tokens":{cached},"output_tokens":{output}}}}}}}}}"#
        )
    }

    #[test]
    fn codex_counts_growth_of_cumulative_totals() {
        let mut f = FileScan::default();
        f.line(Provider::Codex, r#"{"timestamp":"2026-10-07T08:47:40.254Z","type":"turn_context","payload":{"model":"gpt-6-astra"}}"#);
        f.line(Provider::Codex, &codex_count("2026-10-07T08:50:00Z", 100, 60, 10));
        f.line(Provider::Codex, &codex_count("2026-10-07T08:51:00Z", 100, 60, 10)); // repeated, no change
        f.line(Provider::Codex, &codex_count("2026-10-07T08:52:00Z", 300, 200, 30));
        let rows: Vec<_> = f.rows.values().collect();
        assert_eq!(rows.len(), 1);
        let r = rows[0];
        assert_eq!(r.model, "gpt-6-astra");
        assert_eq!((r.uncached, r.cached, r.output, r.total), (100, 200, 30, 330));
    }

    #[test]
    fn grok_rows_from_usage_turns() {
        let usage: Value = serde_json::from_str(
            r#"{"turns":[
                {"endedAt":"2026-10-08T00:19:42.277793+00:00","primaryModelId":"grok-4.7-build","modelUsage":{
                    "grok-4.7-build":{"inputTokens":143941,"cachedReadTokens":84864,"outputTokens":1683},
                    "grok-4.7-fast":{"inputTokens":100,"cachedReadTokens":0,"outputTokens":10}}},
                {"endedAt":"2026-10-08T00:30:00Z","primaryModelId":"grok-4.7-build","inputTokens":1000,"cachedReadTokens":600,"outputTokens":50}
            ]}"#,
        )
        .unwrap();
        let mut f = FileScan { session: "g1".into(), ..Default::default() };
        grok_rows(&mut f, &usage);
        let build = f.rows.values().find(|r| r.model == "grok-4.7-build").unwrap();
        assert_eq!((build.uncached, build.cached, build.output, build.total), (59077 + 400, 84864 + 600, 1733, 145624 + 1050));
        assert_eq!(f.rows.values().find(|r| r.model == "grok-4.7-fast").unwrap().total, 110);
        grok_rows(&mut f, &Value::Null); // "No usage recorded"
        assert!(f.rows.is_empty());
    }

    #[test]
    fn parses_grok_limits() {
        let r: Value = serde_json::from_str(
            r#"{"config":{"creditUsagePercent":1.0,"currentPeriod":{"type":"USAGE_PERIOD_TYPE_WEEKLY","start":"2026-10-05T04:28:48.729013+00:00","end":"2026-10-12T04:28:48.729013+00:00"}},"subscription_tier":"SuperGrok"}"#,
        )
        .unwrap();
        let l = parse_grok(&r).unwrap();
        assert_eq!((l[0].label.as_str(), l[0].used, l[0].window_minutes), ("Weekly", 1.0, Some(10080)));
        assert_eq!(l[0].resets_at, parse_ts("2026-10-12T04:28:48.729Z"));
    }

    #[test]
    fn parses_antigravity_limits() {
        let v: Value = serde_json::from_str(
            r#"{"status":"SUCCESS","command":{"name":"usage","data":{"groups":[
                {"name":"Gemini Models","buckets":[{"id":"gemini-weekly","window":"weekly","remaining_fraction":0.95,"reset_time":"2026-10-16T02:52:33Z"}]},
                {"name":"Claude and GPT models","buckets":[{"id":"3p-weekly","window":"weekly","remaining_fraction":1,"reset_time":"2026-10-16T02:56:32Z"}]}
            ]}}}"#,
        )
        .unwrap();
        let l = parse_antigravity(&v).unwrap();
        assert_eq!(l.iter().map(|l| l.label.as_str()).collect::<Vec<_>>(), ["Weekly · Gemini", "Weekly · Claude & GPT"]);
        assert!((l[0].used - 5.0).abs() < 1e-9);
        assert_eq!((l[1].used, l[1].window_minutes), (0.0, Some(10080)));
        assert_eq!(l[0].resets_at, parse_ts("2026-10-16T02:52:33Z"));
    }

    /// Protobuf bytes for a varint field / a length-delimited field.
    fn vint(mut v: u64) -> Vec<u8> {
        let mut out = vec![];
        loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                out.push(b);
                return out;
            }
            out.push(b | 0x80);
        }
    }
    fn pb_int_field(field: u64, v: u64) -> Vec<u8> {
        [vint(field << 3), vint(v)].concat()
    }
    fn pb_bytes_field(field: u64, b: &[u8]) -> Vec<u8> {
        [vint(field << 3 | 2), vint(b.len() as u64), b.to_vec()].concat()
    }

    #[test]
    fn reads_protobuf_fields() {
        let usage = [pb_int_field(2, 19999), pb_int_field(3, 196), pb_int_field(5, 24486), pb_int_field(9, 36)].concat();
        let kv = [pb_bytes_field(1, b"last_step_index"), pb_bytes_field(2, b"13")].concat();
        let call = [pb_int_field(3, 1318), pb_bytes_field(4, &usage), pb_bytes_field(19, b"gemini-3.8-flash"), pb_bytes_field(20, &kv)].concat();
        let msg = [pb_bytes_field(4, b"id"), vec![0x0d, 1, 2, 3, 4], pb_bytes_field(1, &call)].concat(); // with a fixed32 to skip
        let call = pb_msg(&msg, 1).unwrap();
        let u = pb_msg(call, 4).unwrap();
        assert_eq!((pb_int(u, 2), pb_int(u, 3), pb_int(u, 4), pb_int(u, 5)), (19999, 196, 0, 24486));
        assert_eq!(pb_msg(call, 19), Some(b"gemini-3.8-flash".as_slice()));
        assert_eq!(pb_msg(pb_msg(call, 20).unwrap(), 2), Some(b"13".as_slice()));
        assert_eq!(unhex("0aFF"), [0x0a, 0xff]);
    }

    #[test]
    fn scan_reads_only_appended_lines() {
        let dir = std::env::temp_dir().join(format!("fc-usage-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.jsonl");
        std::fs::write(&path, format!("{}\n{}", claude_line("m1", 5), &claude_line("m2", 7)[..20])).unwrap();
        let mut f = FileScan::default();
        let len = |p: &Path| std::fs::metadata(p).unwrap().len();
        scan(Provider::Claude, &path, len(&path), &mut f).unwrap();
        assert_eq!(f.rows.values().map(|r| r.output).sum::<u64>(), 5); // the half-written line waits
        std::fs::write(&path, format!("{}\n{}\n", claude_line("m1", 5), claude_line("m2", 7))).unwrap();
        scan(Provider::Claude, &path, len(&path), &mut f).unwrap();
        assert_eq!(f.rows.values().map(|r| r.output).sum::<u64>(), 12);
        std::fs::write(&path, format!("{}\n", claude_line("m3", 1))).unwrap(); // rewritten shorter
        scan(Provider::Claude, &path, len(&path), &mut f).unwrap();
        assert_eq!(f.rows.values().map(|r| r.output).sum::<u64>(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn parses_claude_limits() {
        let v: Value = serde_json::from_str(
            r#"{"limits":[
                {"kind":"session","group":"session","percent":10,"resets_at":"2026-10-09T03:20:00.451668+00:00","scope":null},
                {"kind":"weekly_all","group":"weekly","percent":3,"resets_at":"2026-10-14T03:00:00+00:00","scope":null},
                {"kind":"weekly_scoped","group":"weekly","percent":0,"resets_at":"2026-10-14T03:00:00+00:00","scope":{"model":{"id":null,"display_name":"Fable"}}}
            ]}"#,
        )
        .unwrap();
        let l = parse_claude(&v).unwrap();
        assert_eq!(l.iter().map(|l| l.label.as_str()).collect::<Vec<_>>(), ["Session", "Weekly", "Weekly · Fable"]);
        assert_eq!((l[0].used, l[0].window_minutes), (10.0, Some(300)));
        assert_eq!(l[1].resets_at, Some(1_791_946_800_000));
    }

    #[test]
    fn codex_limits_roll_over() {
        let r: Value = serde_json::from_str(r#"{"primary":{"used_percent":3.0,"window_minutes":10080,"resets_at":1791967664},"secondary":null,"credits":{"has_credits":false}}"#).unwrap();
        let l = parse_codex(&r, 1_791_000_000_000).unwrap();
        assert_eq!((l[0].label.as_str(), l[0].used, l[0].resets_at), ("Weekly", 3.0, Some(1_791_967_664_000)));
        let l = parse_codex(&r, 1_792_000_000_000).unwrap();
        assert_eq!((l[0].used, l[0].resets_at, l[0].note.as_deref()), (0.0, None, Some("Reset since last use")));
    }
}
