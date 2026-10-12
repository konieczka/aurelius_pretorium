//! What the agent in each shell is doing, judged here rather than in a window: a window only sees its own tabs, while
//! every shell (other windows', closed workspaces') feeds the Fleet view and the workspace tabs' dots.

use std::time::{Duration, Instant};

/// Terminal title, bell / notification escapes and prompts asking the user something, read off a shell's raw output.
/// Keeps its place across reads: a sequence can be split between two of them.
#[derive(Default)]
pub struct Signals {
    pub title: String,
    pub rang: bool,  // since last taken
    pub asked: bool, // since last taken
    pub asking: Option<Ask>, // a prompt is up that hasn't been typed into (lib.rs clears it on typing)
    state: Esc,
    osc: Vec<u8>,
    text: String,    // the last few letters and digits of the text shown, lowercased (see ASKS)
    answered: bool,  // no typing since the last prompt: redraws of it don't ask again
}

#[derive(Default, Clone, Copy)]
enum Esc {
    #[default]
    Ground,
    Esc,
    Osc,
    OscEsc, // ESC inside an OSC: the start of its ST terminator (ESC \)
    Csi,    // ESC [ ... up to its final byte: colours, cursor moves
}

const OSC_MAX: usize = 4096; // longer ones (OSC 52 clipboard pastes) are cut; titles and notifications are short

// An agent asking the user something: approval prompts (Claude Code's "Do you want to proceed?" / "make this edit",
// Codex's "Would you like to run the following command?", Antigravity's "Run this command?" / "Allow access to this
// file?"), the choices under them ("Yes, and don't ask again", "Yes, and always allow ...", "No, and tell ... what to
// do differently" in all three), Claude's question picker, or a question at the end of a reply. Matched on letters
// and digits only: TUIs that redraw by diff (Codex) move the cursor over unchanged blanks instead of writing spaces.
const ASKS: [&str; 10] = [
    "doyouwant",
    "wouldyoulike",
    "entertoselect",
    "yesanddont",
    "yesandalways",
    "noandtell",
    "runthiscommand",
    "allowaccessto",
    "allowcalling",
    "allowcreation",
];
// Of those, the ones only an approval prompt shows (the rest may be a question: Claude's picker, a reply ending in one).
const APPROVALS: [&str; 7] = ["yesanddont", "yesandalways", "noandtell", "runthiscommand", "allowaccessto", "allowcalling", "allowcreation"];
const TEXT_MAX: usize = 32;

/// What a prompt that's up asks for. An approval's choices are drawn after its question: they turn a question into one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Ask {
    Question,
    Approval,
}

impl Signals {
    /// `echo`: output answering the user's typing or a resize (redraws), which asks nothing new.
    pub fn feed(&mut self, bytes: &[u8], echo: bool) {
        for &b in bytes {
            self.state = match (self.state, b) {
                (Esc::Ground, 0x07) => {
                    self.rang = true;
                    Esc::Ground
                }
                (Esc::Ground | Esc::Esc | Esc::Csi, 0x1b) => Esc::Esc,
                (Esc::Ground, _) => {
                    if b.is_ascii_alphanumeric() {
                        self.letter(b.to_ascii_lowercase() as char, echo);
                    }
                    Esc::Ground
                }
                (Esc::Esc, b'[') => Esc::Csi,
                (Esc::Csi, 0x40..=0x7e) => Esc::Ground,
                (Esc::Csi, _) => Esc::Csi,
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

    fn letter(&mut self, c: char, echo: bool) {
        if self.text.len() >= TEXT_MAX {
            self.text.remove(0);
        }
        self.text.push(c);
        if echo {
            return;
        }
        let Some(a) = ASKS.iter().find(|a| self.text.ends_with(*a)) else { return };
        let kind = if APPROVALS.contains(a) { Ask::Approval } else { Ask::Question };
        self.asking = self.asking.max(Some(kind));
        if !self.answered {
            self.asked = true;
            self.answered = true;
        }
    }

    /// The user typed into the shell: the next prompt asks again.
    pub fn typed(&mut self) {
        self.answered = false;
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

/// Worth a sound and a notice: the agent asks something, or finished its turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Cue {
    Done,
    Ask,
}

// A finished turn waits this long to be cued: the prompt it stopped for may be drawn just after the title goes idle.
const DONE_DEFER: Duration = Duration::from_millis(1000);
const ASK_COVERS: Duration = Duration::from_millis(5000); // a turn ending this soon after asking needs no "done"

/// One shell's agent state between polls.
pub struct Watch {
    agent: Option<String>,
    busy_since: Option<Instant>,
    pub attention: bool, // it rang, or finished, while no window had it on screen
    seen: Option<Instant>,
    pub changed: Instant, // agent came or went, started or stopped working
    asked: Option<Instant>,
    done: Option<Instant>, // finished, not cued yet (DONE_DEFER)
    pub cue: Option<Cue>,  // from the last step, until taken; only while no window has the tab on screen
}

impl Default for Watch {
    fn default() -> Self {
        Watch { agent: None, busy_since: None, attention: false, seen: None, changed: Instant::now(), asked: None, done: None, cue: None }
    }
}

impl Watch {
    /// A window has the tab on screen (the active tab of a pane, in a focused window).
    pub fn seen(&mut self, now: Instant) {
        self.seen = Some(now);
    }

    /// Advance with what the shell shows now; returns whether the agent is working.
    pub fn step(&mut self, now: Instant, agent: Option<&str>, title: &str, quiet: Option<u64>, rang: bool, asked: bool) -> bool {
        if self.agent.as_deref() != agent {
            self.agent = agent.map(String::from);
            self.changed = now;
        }
        let Some(agent) = agent else {
            self.busy_since = None;
            self.attention = false;
            self.done = None;
            return false;
        };
        if asked {
            self.asked = Some(now);
            self.done = None;
            self.cue = Some(Cue::Ask);
        }
        let in_view = self.seen.is_some_and(|t| now.duration_since(t) < IN_VIEW);
        let working = busy(agent, title, quiet);
        if working {
            if self.busy_since.is_none() {
                self.busy_since = Some(now);
                self.changed = now;
            }
            self.attention = false;
            self.done = None;
        } else if let Some(since) = self.busy_since.take() {
            self.changed = now;
            let finished = TITLED.contains(&agent) || now.duration_since(since) >= SETTLED;
            self.attention = !in_view && finished;
            if finished && !self.asked.is_some_and(|t| now.duration_since(t) < ASK_COVERS) {
                self.done = Some(now);
            }
        } else if rang && !in_view {
            self.attention = true;
        }
        if self.done.is_some_and(|t| now.duration_since(t) >= DONE_DEFER) {
            self.done = None;
            self.cue = Some(Cue::Done);
        }
        if in_view {
            self.attention = false;
            self.cue = None; // you can see it ask or finish
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
        s.feed(b"hello \x1b]0;\xe2\x9c\xb3 Fix t", false);
        assert_eq!(s.title, "");
        s.feed(b"he bug\x07more", false);
        assert_eq!(s.title, "✳ Fix the bug");
        assert!(!s.rang, "a BEL ending an OSC isn't a bell");
        s.feed(b"\x1b]2;st-terminated\x1b", false);
        s.feed(b"\\", false);
        assert_eq!(s.title, "st-terminated");
        s.feed(b"\x1b[31mred\x1b[0m", false);
        assert!(!s.rang);
        s.feed(b"\x07", false);
        assert!(s.rang);
    }

    #[test]
    fn notification_escapes_ring() {
        let rings = |seq: &[u8]| {
            let mut s = Signals::default();
            s.feed(seq, false);
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
        assert!(w.step(at(0), Some("claude"), "◐ Work", None, false, false));
        assert!(!w.step(at(1000), Some("claude"), "✳ Work", None, false, false));
        assert!(w.attention);
        w.seen(at(1500));
        w.step(at(2000), Some("claude"), "✳ Work", None, false, false);
        assert!(!w.attention, "looked at");
        // Finishing on screen doesn't ask for attention; ringing while idle off screen does.
        w.step(at(2100), Some("claude"), "◐ More", None, false, false);
        w.seen(at(2200));
        w.step(at(2300), Some("claude"), "✳ More", None, false, false);
        assert!(!w.attention);
        w.step(at(9000), Some("claude"), "✳ More", None, true, false);
        assert!(w.attention);
        // The agent quits: nothing to attend to.
        w.step(at(9500), None, "", None, false, false);
        assert!(!w.attention);
    }

    #[test]
    fn short_bursts_of_output_arent_a_finished_turn() {
        let t0 = Instant::now();
        let at = |ms| t0 + Duration::from_millis(ms);
        let mut w = Watch::default();
        assert!(w.step(at(0), Some("agy"), "", Some(10), false, false));
        w.step(at(2000), Some("agy"), "", Some(1600), false, false);
        assert!(!w.attention, "startup banner");
        w.step(at(3000), Some("agy"), "", Some(10), false, false);
        w.step(at(7000), Some("agy"), "", Some(1600), false, false);
        assert!(w.attention);
    }

    #[test]
    fn prompts_ask_once_until_typed_into() {
        let mut s = Signals::default();
        s.feed(b"\x1b[1m Bash command\x1b[0m\r\n\x1b[2mDo you wa", false);
        assert!(!s.asked);
        s.feed(b"nt to proceed?\x1b[0m\r\n \xe2\x9d\xaf 1. Yes", false);
        assert!(std::mem::take(&mut s.asked));
        assert_eq!(s.asking, Some(Ask::Question), "until its choices are drawn");
        s.feed(b" 2. Yes, and don't ask again\r\n 3. No, and tell Claude what to do differently", false);
        assert!(!s.asked, "the same prompt");
        assert_eq!(s.asking, Some(Ask::Approval));
        // Redrawn without the user answering: the same prompt.
        s.feed(b"Do you want to proceed?", false);
        assert!(!s.asked);
        s.typed();
        s.feed(b"Do you want to proceed?", true);
        assert!(!s.asked, "echo of typing");
        // Codex draws by diff: cursor moves instead of spaces.
        s.feed(b"\x1b[5;3HWould\x1b[5;9Hyou\x1b[5;13Hlike to run", false);
        assert!(s.asked);
        let mut s = Signals::default();
        s.feed(b"Doing the thing you want to see", false);
        assert!(!s.asked);
        // Antigravity's prompts, and the choices under them.
        for prompt in [&b"\x1b[1mRun this command?\x1b[0m"[..], b"Allow access to this URL?", b" 2. Yes, and always allow 'git' in this conversation", b"No, and tell agy what to do differently"] {
            let mut s = Signals::default();
            s.feed(prompt, false);
            assert!(s.asked, "{}", String::from_utf8_lossy(prompt));
        }
    }

    #[test]
    fn cues_finishing_or_asking() {
        let t0 = Instant::now();
        let at = |ms| t0 + Duration::from_millis(ms);
        let mut w = Watch::default();
        w.step(at(0), Some("claude"), "◐ Work", None, false, false);
        w.step(at(1000), Some("claude"), "✳ Work", None, false, false);
        assert_eq!(w.cue.take(), None, "waits for a prompt drawn late");
        w.step(at(2000), Some("claude"), "✳ Work", None, false, false);
        assert_eq!(w.cue.take(), Some(Cue::Done));
        w.step(at(3000), Some("claude"), "✳ Work", None, false, false);
        assert_eq!(w.cue.take(), None, "once");
        // Stopping to ask: just the question, whichever comes first.
        w.step(at(4000), Some("claude"), "◐ Edit", None, false, false);
        w.step(at(5000), Some("claude"), "✳ Edit", None, false, false);
        w.step(at(5500), Some("claude"), "✳ Edit", None, false, true);
        assert_eq!(w.cue.take(), Some(Cue::Ask));
        w.step(at(7000), Some("claude"), "✳ Edit", None, false, false);
        assert_eq!(w.cue.take(), None);
        w.step(at(8000), Some("claude"), "◐ Edit", None, false, false);
        w.step(at(8500), Some("claude"), "◐ Edit", None, false, true);
        assert_eq!(w.cue.take(), Some(Cue::Ask));
        w.step(at(9000), Some("claude"), "✳ Edit", None, false, false);
        w.step(at(10000), Some("claude"), "✳ Edit", None, false, false);
        assert_eq!(w.cue.take(), None);
        // On screen: nothing to cue, though asking still covers the turn's end.
        w.step(at(11000), Some("claude"), "◐ Look", None, false, false);
        w.seen(at(11500));
        w.step(at(12000), Some("claude"), "◐ Look", None, false, true);
        assert_eq!(w.cue.take(), None);
        w.step(at(12500), Some("claude"), "✳ Look", None, false, false);
        w.step(at(16000), Some("claude"), "✳ Look", None, false, false);
        assert_eq!(w.cue.take(), None, "asked 4.5s before finishing");
        w.step(at(17000), Some("claude"), "◐ Look", None, false, false);
        w.seen(at(17500));
        w.step(at(18000), Some("claude"), "✳ Look", None, false, false);
        w.step(at(19000), Some("claude"), "✳ Look", None, false, false);
        assert_eq!(w.cue.take(), None, "finished in view");
        // Looked away before the deferred cue came due: it plays.
        w.step(at(30000), Some("claude"), "◐ Away", None, false, false);
        w.step(at(31000), Some("claude"), "✳ Away", None, false, false);
        w.step(at(32000), Some("claude"), "✳ Away", None, false, false);
        assert_eq!(w.cue.take(), Some(Cue::Done));
        // A startup banner isn't a finished turn.
        let mut w = Watch::default();
        w.step(at(0), Some("agy"), "", Some(10), false, false);
        w.step(at(2000), Some("agy"), "", Some(1600), false, false);
        w.step(at(4000), Some("agy"), "", Some(3600), false, false);
        assert_eq!(w.cue.take(), None);
    }
}
