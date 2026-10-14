//! What the agent in each shell is doing, judged here rather than in a window: a window only sees its own tabs, while
//! every shell (other windows', closed workspaces') feeds the Fleet view and the workspace tabs' dots.

use std::time::{Duration, Instant};

/// Terminal title and bell / notification escapes, read off a shell's raw output, and the screen it draws (to see a
/// prompt that's up). Keeps its place across reads: a sequence can be split between two of them.
pub struct Signals {
    pub title: String,
    pub rang: bool, // since last taken
    state: Esc,
    osc: Vec<u8>,
    screen: vt100::Parser,
}

impl Default for Signals {
    fn default() -> Self {
        Signals { title: String::new(), rang: false, state: Esc::default(), osc: Vec::new(), screen: vt100::Parser::new(24, 80, 0) }
    }
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

/// What a prompt that's up asks for. An approval's choices are drawn after its question: they turn a question into one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Ask {
    Question,
    Approval,
}

impl Signals {
    pub fn feed(&mut self, bytes: &[u8]) {
        self.screen.process(bytes);
        for &b in bytes {
            self.state = match (self.state, b) {
                (Esc::Ground, 0x07) => {
                    self.rang = true;
                    Esc::Ground
                }
                (Esc::Ground | Esc::Esc | Esc::Csi, 0x1b) => Esc::Esc,
                (Esc::Ground, _) => Esc::Ground,
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

    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.screen.screen_mut().set_size(rows, cols);
    }

    /// The prompt on screen now, if one is.
    pub fn prompt(&self) -> Option<Ask> {
        let s = self.screen.screen();
        prompt_in(&s.rows(0, s.size().1).collect::<Vec<_>>())
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

// A prompt is read off the screen, not the output: what agents print (replies, files, diffs, grep hits) is full of
// "do you want" and "allow access to", while a prompt that's up is a list of numbered choices at the bottom of the
// screen, under its question. Approvals: Claude Code's "Do you want to proceed?" / "make this edit to ...?", Codex's
// "Would you like to run the following command?", Antigravity's "Run this command?" / "Allow access to this file?", and
// the choices under them ("Yes, and don't ask again", "Yes, and always allow ...", "No, and tell ... what to do
// differently"; Muse Code's "Allow for this session" / "Block for this session"). Questions: a picker with the cursor
// on a choice (Claude's questions, Muse Code's "Do you trust this workspace?"). A question at the end of a reply asks
// nothing a finished turn doesn't.
const HEADS: [&str; 7] = ["do you want", "would you like", "run this command", "allow access to", "allow calling", "allow creation", "do you trust"];
const APPROVALS: [&str; 5] = ["yes, and don't ask again", "yes, and always allow", "no, and tell", "allow for this session", "block for this session"];
const FOOTERS: [&str; 3] = ["enter to select", "enter to confirm", "to navigate"];
const CURSORS: [char; 4] = ['❯', '›', '▶', '>'];
const FRAME: [char; 9] = ['│', '┃', '║', '|', '╭', '╮', '╰', '╯', '─'];
const BOTTOM: usize = 20; // non-blank rows from the bottom a prompt's choices are looked for in
const ABOVE: usize = 12; // rows above its first choice its question is looked for in (a command or diff in between)

fn prompt_in(rows: &[String]) -> Option<Ask> {
    let lines: Vec<String> = rows
        .iter()
        .map(|r| r.trim_matches(|c: char| c.is_whitespace() || FRAME.contains(&c)).replace('\u{2019}', "'").to_lowercase())
        .filter(|l| !l.is_empty())
        .collect();
    let from = lines.len().saturating_sub(BOTTOM);
    let choices: Vec<(usize, bool, &str)> = lines.iter().enumerate().skip(from).filter_map(|(i, l)| choice(l).map(|(c, t)| (i, c, t))).collect();
    let &(first, _, _) = choices.first()?;
    if choices.iter().any(|(_, _, t)| APPROVALS.iter().any(|a| t.starts_with(a))) {
        return Some(Ask::Approval);
    }
    let head = lines[first.saturating_sub(ABOVE)..first].iter().rev().find_map(|l| HEADS.iter().find(|h| l.starts_with(*h)));
    if head.is_some_and(|h| *h != "do you trust") && choices.iter().any(|(_, _, t)| t.starts_with("yes")) {
        return Some(Ask::Approval);
    }
    let footer = lines[first..].iter().any(|l| FOOTERS.iter().any(|f| l.contains(f)));
    (choices.iter().any(|(_, c, _)| *c) && (head.is_some() || footer)).then_some(Ask::Question)
}

/// A numbered choice ("❯ 1. Yes", "2) No", Muse's "2  Allow ..."): whether the cursor is on it, and its text.
fn choice(line: &str) -> Option<(bool, &str)> {
    let rest = line.trim_start_matches(CURSORS);
    let cursor = rest.len() != line.len();
    let rest = rest.trim_start();
    let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if !(1..=2).contains(&digits) {
        return None;
    }
    let rest = &rest[digits..];
    let rest = rest.strip_prefix(['.', ')']).unwrap_or(rest);
    rest.starts_with(' ').then(|| (cursor, rest.trim_start())).filter(|(_, t)| !t.is_empty())
}

/// Whether a prompt is up, from the screen and the agent's own word (a hook), and when one comes up: a cue once per
/// prompt, though TUIs redraw it (and may be caught mid-redraw).
#[derive(Default)]
pub struct Asks {
    up: Option<Ask>,
    gone: u8,               // polls in a row without one on screen
    hook: Option<Ask>,      // the agent said it asks; until typed into
    typed: Option<std::time::SystemTime>,
}

const GONE: u8 = 2; // polls without the prompt before it counts as answered (not a redraw)

impl Asks {
    /// The user typed into the shell: whatever asked is answered (or dismissed).
    pub fn typed(&mut self) {
        self.hook = None;
        self.typed = Some(std::time::SystemTime::now());
    }

    /// The agent's hook said, at `at`, a prompt went up. Older than the last typing: already answered.
    pub fn hooked(&mut self, at: std::time::SystemTime) {
        if self.typed.is_none_or(|t| at > t) {
            self.hook = Some(Ask::Approval);
        }
    }

    /// The agent is gone: so is what it asked.
    pub fn clear(&mut self) {
        self.up = None;
        self.hook = None;
    }

    /// With the prompt on screen now: what's being asked, and whether it just started asking.
    pub fn check(&mut self, screen: Option<Ask>) -> (Option<Ask>, bool) {
        let asking = screen.max(self.hook);
        let asked = asking.is_some() && self.up.is_none();
        if asking.is_some() {
            self.up = self.up.max(asking);
            self.gone = 0;
        } else if self.up.is_some() {
            self.gone += 1;
            if self.gone >= GONE {
                self.up = None;
            }
        }
        (asking, asked)
    }
}

// Working or not: these put a spinner at the start of the terminal title while working (Claude ◐◑, Codex, Grok and
// Muse braille) and drop it when done. Output is no use for them: Grok animates its logo while idle. The rest are judged by
// output that isn't an echo of input: idle agents' screens stay still.
const TITLED: [&str; 4] = ["claude", "codex", "grok", "muse"];
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
        assert!(busy("muse", "⠹ ws", Some(9999)));
        assert!(!busy("muse", "ws", Some(0))); // its redrawn status rows while idle don't count
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

    fn screen(text: &[u8]) -> Option<Ask> {
        let mut s = Signals::default();
        s.feed(text);
        s.prompt()
    }

    // Prompts as the CLIs draw them, at the bottom of the screen.
    #[test]
    fn prompts_on_screen_ask() {
        let claude = b"\x1b[1m Bash command\x1b[0m\r\n   ls -la\r\n\r\n Do you want to proceed?\r\n \xe2\x9d\xaf 1. Yes\r\n";
        assert_eq!(screen(claude), Some(Ask::Approval), "the question and a yes are enough");
        let claude = [&claude[..], b"   2. Yes, and don\xe2\x80\x99t ask again for ls commands\r\n   3. No, and tell Claude what to do differently (esc)\r\n"].concat();
        assert_eq!(screen(&claude), Some(Ask::Approval));
        // Codex draws by diff: cursor moves instead of spaces.
        let codex = b"\x1b[3;3HWould\x1b[3;9Hyou\x1b[3;13Hlike to run the following command?\x1b[5;5H$ cargo test\x1b[7;3H\xe2\x80\xba 1. Yes, proceed (y)\x1b[8;5H2. No, and tell Codex what to do differently (esc)";
        assert_eq!(screen(codex), Some(Ask::Approval));
        for prompt in [&b"\x1b[1mRun this command?\x1b[0m\r\n  1. Yes\r\n  2. No"[..], b"Allow access to this URL?\r\n\xe2\x9d\xaf 1. Yes\r\n 2. Yes, and always allow 'git' in this conversation", b"\x1b[9;3H1  Allow once\x1b[10;3H2  Allow for this session"] {
            assert_eq!(screen(prompt), Some(Ask::Approval), "{}", String::from_utf8_lossy(prompt));
        }
        // Pickers: Claude's questions, Muse's workspace trust.
        let picker = b" \xe2\x98\x90 Scope\r\n\r\nWhich files should change?\r\n\r\n\xe2\x9d\xaf 1. Just src\r\n  2. Everything\r\n\r\nEnter to select \xc2\xb7 \xe2\x86\x91/\xe2\x86\x93 to navigate \xc2\xb7 Esc to cancel";
        assert_eq!(screen(picker), Some(Ask::Question));
        assert_eq!(screen(b"\x1b[1mDo you trust this workspace?\r\n\xe2\x9d\xaf 1. Trust\r\n  2. Quit"), Some(Ask::Question));
    }

    // What agents print while they work: the phrases prompts use, but no prompt.
    #[test]
    fn output_that_only_mentions_asking_doesnt() {
        for out in [
            &b"Done. Would you like me to also add tests?"[..],
            b"Nothing left to do. You want the fix in main.",
            b"src/auth.rs:12: // allow access to admins\r\n+ /// Allow calling this from JS",
            b"say no and tell them\r\npress enter to select a file",
            // A reply ending in a question with its options listed: the turn ends, that's cue enough.
            b"Would you like me to:\r\n1. Add tests\r\n2. Ship it",
            // A file shown with the prompts quoted in it.
            b"   12  // Codex's \"Would you like to run the following command?\"\r\n   13  s.feed(b\" 2. Yes, and don't ask again\")",
        ] {
            assert_eq!(screen(out), None, "{}", String::from_utf8_lossy(out));
        }
        // An answered prompt scrolls away: what's at the bottom now is the work.
        let mut s = Signals::default();
        s.feed(b"Do you want to proceed?\r\n\xe2\x9d\xaf 1. Yes\r\n  2. No\r\n");
        assert_eq!(s.prompt(), Some(Ask::Approval));
        s.feed("output line\r\n".repeat(30).as_bytes());
        assert_eq!(s.prompt(), None);
        // Claude erases it in place.
        s.feed(b"Do you want to proceed?\r\n\xe2\x9d\xaf 1. Yes\r\n  2. No\r\n\x1b[3A\x1b[J\xe2\x8f\xba Running");
        assert_eq!(s.prompt(), None);
    }

    #[test]
    fn asks_once_per_prompt() {
        let mut a = Asks::default();
        assert_eq!(a.check(Some(Ask::Question)), (Some(Ask::Question), true));
        assert_eq!(a.check(Some(Ask::Approval)), (Some(Ask::Approval), false), "its choices drawn");
        assert_eq!(a.check(None), (None, false), "caught mid-redraw");
        assert_eq!(a.check(Some(Ask::Approval)), (Some(Ask::Approval), false));
        a.check(None);
        a.check(None);
        assert_eq!(a.check(Some(Ask::Approval)), (Some(Ask::Approval), true), "the next prompt");
        // The hook: up until typed into, unless it came after the typing it would be answered by.
        let mut a = Asks::default();
        let t = std::time::SystemTime::now();
        a.hooked(t);
        assert_eq!(a.check(None), (Some(Ask::Approval), true));
        assert_eq!(a.check(None), (Some(Ask::Approval), false));
        a.typed();
        assert_eq!(a.check(None), (None, false));
        a.hooked(t);
        assert_eq!(a.check(None).0, None, "answered already");
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
