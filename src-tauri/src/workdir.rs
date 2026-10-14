//! Where an agent works. Agents keep their working directory to themselves: Claude Code's `cd` moves only its own
//! idea of it, not its process, and Codex or Gemini take a directory per command. But every command they run starts
//! there, or goes there (`cd ../wt && …`). So a sampler watches the processes each agent starts, often enough to see
//! most commands, short ones included: the last place one of them started or moved to is where the agent works.
//! Claude Code needs no guessing: its transcript records where it is with every message (see `Transcript`).

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const DEPTH: u8 = 3; // agent -> sandbox wrapper -> shell -> command

#[derive(Default)]
pub struct Work {
    agents: Vec<u32>,              // the agent's own processes (wrapper and real one), from `pty_status`
    seen: HashMap<u32, String>,    // its commands still running, and where each was at the last sample
    helpers: HashSet<u32>,         // its own helpers still running (see `helper`): they say nothing
    pub dir: Option<String>,       // where the latest of them started or moved to
}

impl Work {
    /// The agent's processes now (none: no agent). Another agent (none of the same processes) starts from scratch.
    pub fn set_agents(&mut self, pids: Vec<u32>) {
        if !pids.iter().any(|p| self.agents.contains(p)) {
            self.seen.clear();
            self.helpers.clear();
            self.dir = None;
        }
        self.agents = pids;
    }

    /// Look at the agent's commands: one that's new, or has moved since the last look, says where the agent is.
    /// Only checkouts count: a look into /tmp or ~ says nothing about the branch the agent works on.
    pub fn sample(&mut self) {
        if self.agents.is_empty() {
            return;
        }
        let mut live = HashMap::new();
        let mut todo: Vec<(u32, u8)> = self.agents.iter().map(|&p| (p, 1)).collect();
        let mut helpers = HashSet::new();
        while let Some((pid, depth)) = todo.pop() {
            for c in children(pid) {
                if self.agents.contains(&c) {
                    continue;
                }
                // Judged once, when first seen: it may exec something else later, but stays what it started as.
                if self.helpers.contains(&c) || (!self.seen.contains_key(&c) && helper(&args_of(c))) {
                    helpers.insert(c);
                    continue;
                }
                if let Some(dir) = crate::cwd_of(c) {
                    live.insert(c, dir);
                }
                if depth < DEPTH {
                    todo.push((c, depth + 1));
                }
            }
        }
        // Several news at once: the newest process (pids mostly grow) is the latest command.
        let news = live.iter().filter(|(pid, dir)| self.seen.get(pid) != Some(dir)).max_by_key(|(pid, _)| **pid);
        if let Some((_, dir)) = news.filter(|(_, dir)| in_checkout(dir)) {
            self.dir = Some(dir.clone());
        }
        self.seen = live;
        self.helpers = helpers;
    }
}

/// What an agent starts for itself rather than for its work, where it was launched whatever it works on now: Claude
/// Code keeps the Mac awake with a fresh `caffeinate` every few minutes, and agents (re)start their MCP servers.
/// Neither they nor what they run say where the agent is.
fn helper(args: &[String]) -> bool {
    const LAUNCHERS: [&str; 12] = ["node", "npx", "npm", "bun", "bunx", "deno", "uv", "uvx", "pipx", "python", "python3", "docker"];
    let Some(prog) = args.first().map(|a| a.rsplit('/').next().unwrap_or(a)) else { return false };
    prog == "caffeinate" || prog.contains("mcp") || (LAUNCHERS.contains(&prog) && args[1..].iter().any(|a| a.to_lowercase().contains("mcp")))
}

/// Where Claude Code works, from its own records: `sessions/<pid>.json` names its session, and every line of the
/// session's transcript carries its working directory then, following its `cd`s and worktrees.
#[derive(Default)]
pub struct Transcript {
    path: PathBuf,
    len: u64,            // the transcript's length when last read: unchanged, so is `cwd`
    cwd: Option<String>, // the last one it records
}

impl Transcript {
    /// None when Claude Code hasn't said yet (no message since it started): the caller falls back to sampling.
    pub fn claude(&mut self, pids: &[u32]) -> Option<String> {
        let dir = claude_dir();
        let (session, launched, started) = pids.iter().find_map(|p| {
            let v: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join(format!("sessions/{p}.json"))).ok()?).ok()?;
            Some((v["sessionId"].as_str()?.to_string(), v["cwd"].as_str().unwrap_or("").to_string(), v["startedAt"].as_u64().unwrap_or(0)))
        })?;
        let name = format!("{session}.jsonl");
        if self.path.file_name() != Some(name.as_ref()) {
            // Kept under its launch folder, every character but letters and digits a dash (long ones are shortened: look).
            let projects = dir.join("projects");
            let slug: String = launched.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
            let guess = projects.join(slug).join(&name);
            let path = if guess.exists() {
                guess
            } else {
                std::fs::read_dir(&projects).ok()?.flatten().map(|d| d.path().join(&name)).find(|p| p.exists())?
            };
            *self = Transcript { path, ..Default::default() };
        }
        let meta = std::fs::metadata(&self.path).ok()?;
        // A resumed session: what it recorded before is where the last process was, not where this one is.
        let at = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_millis() as u64;
        if at < started {
            return None;
        }
        if meta.len() != self.len {
            self.len = meta.len();
            self.cwd = last_cwd(&self.path, self.len).or(self.cwd.take());
        }
        self.cwd.clone()
    }
}

fn claude_dir() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".claude"))
}

/// The working directory the transcript's last line that has one records. Read from the end, a bigger piece each time:
/// a line (a long tool result) can be megabytes. Subagents' lines are theirs: they may work elsewhere.
fn last_cwd(path: &Path, len: u64) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut tail = 64 * 1024;
    loop {
        let start = len.saturating_sub(tail);
        f.seek(SeekFrom::Start(start)).ok()?;
        let mut buf = Vec::new();
        f.by_ref().take(len - start).read_to_end(&mut buf).ok()?;
        let mut lines: Vec<&[u8]> = buf.split(|&b| b == b'\n').collect();
        if start > 0 {
            lines.remove(0); // cut off: begins before the piece
        }
        let found = lines.iter().rev().filter(|l| l.windows(6).any(|w| w == b"\"cwd\":")).find_map(|l| {
            let v: serde_json::Value = serde_json::from_slice(l).ok()?;
            if v["isSidechain"] == true {
                return None;
            }
            v["cwd"].as_str().map(String::from)
        });
        if found.is_some() || start == 0 || tail >= 16 << 20 {
            return found;
        }
        tail *= 4;
    }
}

fn in_checkout(dir: &str) -> bool {
    Path::new(dir).ancestors().any(|d| d.join(".git").exists())
}

#[cfg(target_os = "macos")]
fn children(pid: u32) -> Vec<u32> {
    let mut buf = vec![0 as libc::pid_t; 256];
    let size = (buf.len() * std::mem::size_of::<libc::pid_t>()) as i32;
    let n = unsafe { libc::proc_listchildpids(pid as libc::pid_t, buf.as_mut_ptr() as *mut libc::c_void, size) };
    // Returns a count of pids (bytes on some older systems: then it's past the buffer, and the zeros are dropped).
    buf.truncate((n.max(0) as usize).min(buf.len()));
    buf.into_iter().filter(|&p| p > 0).map(|p| p as u32).collect()
}
#[cfg(not(target_os = "macos"))]
fn children(_pid: u32) -> Vec<u32> {
    Vec::new() // ponytail: macOS only for now; elsewhere the agent's own directory is all there is
}

/// A process's command line (empty if it's gone, or not ours to read).
#[cfg(target_os = "macos")]
fn args_of(pid: u32) -> Vec<String> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as libc::c_int];
    let mut size: libc::size_t = 0;
    let null = std::ptr::null_mut();
    if unsafe { libc::sysctl(mib.as_mut_ptr(), 3, null, &mut size, null, 0) } != 0 {
        return Vec::new();
    }
    let mut buf = vec![0u8; size];
    if unsafe { libc::sysctl(mib.as_mut_ptr(), 3, buf.as_mut_ptr() as *mut libc::c_void, &mut size, null, 0) } != 0 || size < 4 {
        return Vec::new();
    }
    // argc, then the executable's path, padding, and the arguments, each ending in a NUL.
    let argc = i32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    let mut parts = buf[4..size].split(|&b| b == 0).filter(|a| !a.is_empty());
    parts.next();
    parts.take(argc).map(|a| String::from_utf8_lossy(a).into_owned()).collect()
}
#[cfg(not(target_os = "macos"))]
fn args_of(_pid: u32) -> Vec<String> {
    Vec::new()
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn follows_the_agents_latest_command_into_a_checkout() {
        let repo = std::fs::canonicalize(env!("CARGO_MANIFEST_DIR")).unwrap();
        let src = repo.join("src");
        // The "agent": a shell that runs one command in the checkout, then another in a folder of it, then keeps the
        // Mac awake from where it was launched (a helper: no move), then looks into /tmp.
        let script = format!(
            "sleep 0.5 & wait; cd {} && sleep 0.5 & wait; (cd {} && exec caffeinate -t 1) & wait; cd /tmp && sleep 0.5 & wait",
            src.display(),
            repo.display()
        );
        let mut agent = std::process::Command::new("/bin/sh").args(["-c", &script]).current_dir(&repo).spawn().unwrap();
        let mut w = Work::default();
        w.set_agents(vec![agent.id()]);
        let mut dirs = Vec::new();
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(150));
            w.sample();
            if dirs.last() != w.dir.as_ref() {
                dirs.extend(w.dir.clone());
            }
        }
        agent.wait().unwrap();
        // /tmp isn't a checkout: the agent stays where it last was in one.
        assert_eq!(dirs, [repo.to_string_lossy(), src.to_string_lossy()]);
    }

    #[test]
    fn helpers_are_told_apart() {
        let a = |s: &str| s.split(' ').map(String::from).collect::<Vec<_>>();
        assert!(helper(&a("caffeinate -i -t 300")));
        assert!(helper(&a("npm exec @playwright/mcp@latest")));
        assert!(helper(&a("/opt/homebrew/bin/uvx mcp-server-fetch")));
        assert!(helper(&a("/usr/local/bin/github-mcp-server stdio")));
        assert!(!helper(&a("grep -rn mcp src")));
        assert!(!helper(&a("node scripts/build.js")));
        assert!(!helper(&[]));
    }

    #[test]
    fn reads_where_claude_last_was_from_the_end_of_its_transcript() {
        let dir = std::env::temp_dir().join(format!("fc-workdir-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.jsonl");
        // Then a subagent elsewhere, then a line too long for the first piece read, with no cwd of its own.
        let big = "x".repeat(200 * 1024);
        let text = format!(
            "{{\"cwd\":\"/a\"}}\n{{\"cwd\":\"/b\"}}\n{{\"cwd\":\"/wt\",\"isSidechain\":true}}\n{{\"type\":\"x\",\"r\":\"{big}\"}}\n"
        );
        std::fs::write(&path, &text).unwrap();
        assert_eq!(last_cwd(&path, text.len() as u64).as_deref(), Some("/b"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
