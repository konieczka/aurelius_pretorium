//! Where an agent works. Agents keep their working directory to themselves: Claude Code's `cd` moves only its own
//! idea of it, not its process, and Codex or Gemini take a directory per command. But every command they run starts
//! there, or goes there (`cd ../wt && …`). So a sampler watches the processes each agent starts, often enough to see
//! most commands, short ones included: the last place one of them started or moved to is where the agent works.

use std::collections::HashMap;
use std::path::Path;

const DEPTH: u8 = 3; // agent -> sandbox wrapper -> shell -> command

#[derive(Default)]
pub struct Work {
    agents: Vec<u32>,              // the agent's own processes (wrapper and real one), from `pty_status`
    seen: HashMap<u32, String>,    // its commands still running, and where each was at the last sample
    pub dir: Option<String>,       // where the latest of them started or moved to
}

impl Work {
    /// The agent's processes now (none: no agent). Another agent (none of the same processes) starts from scratch.
    pub fn set_agents(&mut self, pids: Vec<u32>) {
        if !pids.iter().any(|p| self.agents.contains(p)) {
            self.seen.clear();
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
        while let Some((pid, depth)) = todo.pop() {
            for c in children(pid) {
                if self.agents.contains(&c) {
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

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn follows_the_agents_latest_command_into_a_checkout() {
        let repo = std::fs::canonicalize(env!("CARGO_MANIFEST_DIR")).unwrap();
        let src = repo.join("src");
        // The "agent": a shell that runs one command in the checkout, then another in a folder of it.
        let script = format!("sleep 0.5 & wait; cd {} && sleep 0.5 & wait; cd /tmp && sleep 0.5 & wait", src.display());
        let mut agent = std::process::Command::new("/bin/sh").args(["-c", &script]).current_dir(&repo).spawn().unwrap();
        let mut w = Work::default();
        w.set_agents(vec![agent.id()]);
        let mut dirs = Vec::new();
        for _ in 0..12 {
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
}
