//! What the agent in each shell is doing, judged here rather than in a window: a window only sees its own tabs, while
//! every shell (other windows', closed workspaces') feeds the Fleet view and the workspace tabs' dots.

use std::time::{Duration, Instant};

/// Terminal title and bell / notification escapes, read off a shell's raw output. Keeps its place across reads: a
/// sequence can be split between two of them.
#[derive(Default)]
pub struct Signals {
    pub title: String,
    pub rang: bool, // since last taken
    state: Esc,
    osc: Vec<u8>,
}

#[derive(Default, Clone, Copy)]
enum Esc {
    #[default]
    Ground,
    Esc,
    Osc,
    OscEsc, // ESC inside an OSC: the start of its ST terminator (ESC \)
}

const OSC_MAX: usize = 4096; // longer ones (OSC 52 clipboard pastes) are cut; titles and notifications are short

impl Signals {
    pub fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.state = match (self.state, b) {
                (Esc::Ground, 0x07) => {
                    self.rang = true;
                    Esc::Ground
                }
                (Esc::Ground | Esc::Esc, 0x1b) => Esc::Esc,
                (Esc::Esc | Esc::OscEsc, b']') => {
                    self.osc.clear();
                    Esc::Osc
                }
                (Esc::Osc, 0x07) | (Esc::OscEsc, b'\\') => {
                    self.osc_done();
                    Esc::Ground
                }
                (Esc::Osc, 0x1b) => Esc::OscEsc,
                (Esc::Osc, _) => {
                    if self.osc.len() < OSC_MAX {
                        self.osc.push(b);
                    }
                    Esc::Osc
                }
                (Esc::OscEsc, 0x1b) => Esc::Esc,
                _ => Esc::Ground,
            }
        }
    }

    /// OSC 0/2 set the title. Bell-likes are the notification escapes agents can be set to use: OSC 9 (iTerm2; with a
    /// number first it's ConEmu's progress instead), 777 notify (rxvt), 99 (kitty).
    fn osc_done(&mut self) {
        let s = String::from_utf8_lossy(&self.osc);
        let (code, rest) = s.split_once(';').unwrap_or((&s, ""));
        match code {
            "0" | "2" => self.title = rest.to_string(),
            "9" => {
                let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
                if digits == 0 || !(rest.len() == digits || rest[digits..].starts_with(';')) {
                    self.rang = true;
                }
            }
            "777" if rest.starts_with("notify;") => self.rang = true,
            "99" => self.rang = true,
            _ => {}
        }
    }
}

// Working or not: these put a spinner at the start of the terminal title while working (Claude ◐◑, Codex and Grok
// braille) and drop it when done. Output is no use for them: Grok animates its logo while idle. The rest are judged by
// output that isn't an echo of input: idle agents' screens stay still.
const TITLED: [&str; 3] = ["claude", "codex", "grok"];
const QUIET: u64 = 1500; // ms without output that counts as idle
const SETTLED: Duration = Duration::from_millis(3000); // output-judged agents: shorter bursts (startup banner) aren't a finished turn
const IN_VIEW: Duration = Duration::from_millis(2500); // a window reported the tab on screen this recently (they poll every second)

/// `quiet` = ms since the shell's last output that wasn't an echo of input (None: none yet).
pub fn busy(agent: &str, title: &str, quiet: Option<u64>) -> bool {
    if TITLED.contains(&agent) {
        title.starts_with(|c| ('\u{2800}'..='\u{28ff}').contains(&c) || ('◐'..='◓').contains(&c))
    } else {
        quiet.is_some_and(|q| q < QUIET)
    }
}

/// One shell's agent state between polls.
pub struct Watch {
    agent: Option<String>,
    busy_since: Option<Instant>,
    pub attention: bool, // it rang, or finished, while no window had it on screen
    seen: Option<Instant>,
    pub changed: Instant, // agent came or went, started or stopped working
}

impl Default for Watch {
    fn default() -> Self {
        Watch { agent: None, busy_since: None, attention: false, seen: None, changed: Instant::now() }
    }
}

impl Watch {
    /// A window has the tab on screen (the active tab of a pane, in a focused window).
    pub fn seen(&mut self, now: Instant) {
        self.seen = Some(now);
    }

    /// Advance with what the shell shows now; returns whether the agent is working.
    pub fn step(&mut self, now: Instant, agent: Option<&str>, title: &str, quiet: Option<u64>, rang: bool) -> bool {
        if self.agent.as_deref() != agent {
            self.agent = agent.map(String::from);
            self.changed = now;
        }
        let Some(agent) = agent else {
            self.busy_since = None;
            self.attention = false;
            return false;
        };
        let in_view = self.seen.is_some_and(|t| now.duration_since(t) < IN_VIEW);
        let working = busy(agent, title, quiet);
        if working {
            if self.busy_since.is_none() {
                self.busy_since = Some(now);
                self.changed = now;
            }
            self.attention = false;
        } else if let Some(since) = self.busy_since.take() {
            self.changed = now;
            self.attention = !in_view && (TITLED.contains(&agent) || now.duration_since(since) >= SETTLED);
        } else if rang && !in_view {
            self.attention = true;
        }
        if in_view {
            self.attention = false;
        }
        working
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Titles as the CLIs actually set them, working then idle.
    #[test]
    fn title_spinners_mean_working() {
        for t in ["◐ Claude Code", "◑ Ok"] {
            assert!(busy("claude", t, Some(9999)));
        }
        for t in ["✳ Claude Code", "✳ Ok"] {
            assert!(!busy("claude", t, Some(0)));
        }
        assert!(busy("codex", "⠦ fleet_command", None));
        assert!(!busy("codex", "fleet_command", Some(0)));
        assert!(busy("grok", "⠧ - Responding - grok", Some(9999)));
        assert!(!busy("grok", "grok", Some(0))); // idle logo animation: output doesn't count
    }

    #[test]
    fn untitled_agents_go_by_recent_output() {
        assert!(busy("agy", "", Some(200)));
        assert!(!busy("agy", "", Some(5000)));
        assert!(!busy("opencode", "", None));
    }

    #[test]
    fn reads_titles_and_bells_across_reads() {
        let mut s = Signals::default();
        s.feed(b"hello \x1b]0;\xe2\x9c\xb3 Fix t");
        assert_eq!(s.title, "");
        s.feed(b"he bug\x07more");
        assert_eq!(s.title, "✳ Fix the bug");
        assert!(!s.rang, "a BEL ending an OSC isn't a bell");
        s.feed(b"\x1b]2;st-terminated\x1b");
        s.feed(b"\\");
        assert_eq!(s.title, "st-terminated");
        s.feed(b"\x1b[31mred\x1b[0m");
        assert!(!s.rang);
        s.feed(b"\x07");
        assert!(s.rang);
    }

    #[test]
    fn notification_escapes_ring() {
        let rings = |seq: &[u8]| {
            let mut s = Signals::default();
            s.feed(seq);
            s.rang
        };
        assert!(rings(b"\x1b]9;Done\x07"));
        assert!(!rings(b"\x1b]9;4;1;50\x07"), "ConEmu progress");
        assert!(!rings(b"\x1b]9;4\x07"));
        assert!(rings(b"\x1b]777;notify;Claude;Done\x07"));
        assert!(!rings(b"\x1b]777;other\x07"));
        assert!(rings(b"\x1b]99;;hi\x1b\\"));
        assert!(!rings(b"\x1b]7;file:///tmp\x07"));
    }

    #[test]
    fn finishing_unseen_needs_attention() {
        let t0 = Instant::now();
        let at = |ms| t0 + Duration::from_millis(ms);
        let mut w = Watch::default();
        assert!(w.step(at(0), Some("claude"), "◐ Work", None, false));
        assert!(!w.step(at(1000), Some("claude"), "✳ Work", None, false));
        assert!(w.attention);
        w.seen(at(1500));
        w.step(at(2000), Some("claude"), "✳ Work", None, false);
        assert!(!w.attention, "looked at");
        // Finishing on screen doesn't ask for attention; ringing while idle off screen does.
        w.step(at(2100), Some("claude"), "◐ More", None, false);
        w.seen(at(2200));
        w.step(at(2300), Some("claude"), "✳ More", None, false);
        assert!(!w.attention);
        w.step(at(9000), Some("claude"), "✳ More", None, true);
        assert!(w.attention);
        // The agent quits: nothing to attend to.
        w.step(at(9500), None, "", None, false);
        assert!(!w.attention);
    }

    #[test]
    fn short_bursts_of_output_arent_a_finished_turn() {
        let t0 = Instant::now();
        let at = |ms| t0 + Duration::from_millis(ms);
        let mut w = Watch::default();
        assert!(w.step(at(0), Some("agy"), "", Some(10), false));
        w.step(at(2000), Some("agy"), "", Some(1600), false);
        assert!(!w.attention, "startup banner");
        w.step(at(3000), Some("agy"), "", Some(10), false);
        w.step(at(7000), Some("agy"), "", Some(1600), false);
        assert!(w.attention);
    }
}
