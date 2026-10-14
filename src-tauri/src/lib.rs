use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::ipc::{Channel, InvokeResponseBody};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, State};

mod agent_config;
mod hooks;
mod mcp;
mod skills;
mod status;
mod usage;
mod web;
mod workdir;

struct Session {
    // Input for the shell, written by its own thread: a write blocks while the program in the terminal isn't
    // reading, and that must not hold up the main thread (every window) or the sessions lock.
    input: std::sync::mpsc::Sender<Vec<u8>>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    out: Arc<Mutex<Output>>,
    cwd: String, // where it is: starts as the tab's folder, follows `cd` (see `cwd_of`), or its agent's work
    watch: status::Watch,
    work: workdir::Work, // where its agent's commands run
    transcript: workdir::Transcript, // where its agent says it is (Claude Code)
    moved: Option<String>, // where its agent's own process last was, when not where the shell is
}

/// Where a shell's output goes, plus its recent output so a window that (re)attaches can replay it.
#[derive(Default)]
struct Output {
    chan: Option<Channel<InvokeResponseBody>>,
    recent: Vec<u8>,
    exited: bool,
    input: Option<Instant>,  // last write or resize from the window
    active: Option<Instant>, // last output that wasn't just answering that (echo, redraw)
    signals: status::Signals,
    asks: status::Asks,
}
const ECHO: std::time::Duration = std::time::Duration::from_millis(300);
const KEEP: usize = 512 * 1024; // ponytail: raw byte tail, not a parsed screen; replays the last ~512KB of output

/// Shells keyed by terminal tab id: they outlive the window that started them (reloads, workspaces moved between windows).
#[derive(Default)]
struct Ptys {
    sessions: Mutex<HashMap<String, Session>>,
}

type Res<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Whitelisted env for child processes: the app's own env (e.g. npm_*/bun vars from
/// `bun run tauri dev`) must not leak into user shells or git hooks.
fn clean_env() -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = ["HOME", "USER", "LOGNAME", "SHELL", "TMPDIR", "SSH_AUTH_SOCK", "__CF_USER_TEXT_ENCODING"]
        .iter()
        .filter_map(|k| std::env::var(k).ok().map(|v| (k.to_string(), v)))
        .collect();
    // Finder-launched apps get no LANG; without it shells mangle UTF-8.
    env.push(("LANG".into(), std::env::var("LANG").unwrap_or_else(|_| "en_US.UTF-8".into())));
    env
}

/// PATH from the user's login shell (Finder-launched apps lack Homebrew, nvm, ...), resolved once.
pub(crate) fn login_path() -> &'static str {
    static PATH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    PATH.get_or_init(|| {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        let mut cmd = std::process::Command::new(shell);
        cmd.args(["-ilc", r#"printf "__FC__%s__FC__" "$PATH""#])
            .env_clear()
            .envs(clean_env())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        // An rc file that hangs (waits on input, a slow network mount) mustn't hold up every git call forever.
        usage::output_within(cmd, std::time::Duration::from_secs(5))
            .and_then(|o| String::from_utf8_lossy(&o.stdout).split("__FC__").nth(1).map(String::from))
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin".into())
    })
}

/// Run blocking work (file system, child processes) on the blocking pool: sync commands run on the main thread,
/// where a big file or a slow disk would freeze every window.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Res<T> + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(err)?
}

/// Run git with an argument list (no shell). Async + blocking pool so pull/push never freeze the UI.
#[tauri::command]
async fn git(cwd: String, args: Vec<String>) -> Res<String> {
    blocking(move || {
        let out = std::process::Command::new("git")
            // Opening a folder runs status in the background: a repo's own config must not get to run commands
            // through it (a downloaded repo's core.fsmonitor runs on every `git status`).
            .args(["-c", "core.fsmonitor=false"])
            .args(&args)
            .current_dir(cwd)
            .env_clear()
            .envs(clean_env())
            .env("PATH", login_path())
            .env("GIT_TERMINAL_PROMPT", "0") // fail instead of waiting on a password prompt
            .env("GIT_OPTIONAL_LOCKS", "0") // background status must not fight a terminal's git
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(err)?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            let msg = if out.stderr.is_empty() { &out.stdout } else { &out.stderr };
            Err(String::from_utf8_lossy(msg).trim().to_string())
        }
    })
    .await
}

/// Attach `on_data` to the tab's shell, spawning it if it doesn't exist yet. Re-attaching replays recent output first.
#[tauri::command]
/// Returns whether it spawned a new shell (false = re-attached to a running one).
fn pty_open(
    ptys: State<Ptys>,
    id: String,
    cwd: String,
    cols: u16,
    rows: u16,
    on_data: Channel<InvokeResponseBody>,
) -> Res<bool> {
    let mut sessions = ptys.sessions.lock().unwrap();
    if let Some(s) = sessions.get(&id) {
        let mut out = s.out.lock().unwrap();
        if !out.recent.is_empty() {
            let _ = on_data.send(InvokeResponseBody::Raw(out.recent.clone()));
        }
        if out.exited {
            let _ = on_data.send(InvokeResponseBody::Raw(Vec::new()));
        }
        out.chan = Some(on_data);
        out.signals.resize(rows, cols);
        let _ = s.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        return Ok(false);
    }

    let pair = native_pty_system()
        .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .map_err(err)?;
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let mut cmd = CommandBuilder::new(shell);
    // Start from a clean env like Terminal.app: the app's own env (e.g. npm_*/bun vars and
    // node_modules/.bin from `bun run tauri dev`) must not leak into user shells. The login
    // shell (-l) then rebuilds PATH etc. from the user's profile.
    cmd.env_clear();
    for (k, v) in clean_env() {
        cmd.env(k, v);
    }
    cmd.env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
    cmd.arg("-l");
    cmd.cwd(&cwd);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    let (var, asks) = hooks::env(&id);
    cmd.env(var, asks);
    let child = pair.slave.spawn_command(cmd).map_err(err)?;
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().map_err(err)?;
    let mut writer = pair.master.take_writer().map_err(err)?;
    let (input, typed) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        // Ends when the session is dropped (its sender with it) or the shell is gone.
        for data in typed {
            if writer.write_all(&data).is_err() {
                break;
            }
        }
    });
    let out = Arc::new(Mutex::new(Output { chan: Some(on_data), ..Default::default() }));
    out.lock().unwrap().signals.resize(rows, cols);
    let sink = out.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 16 * 1024];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            let mut o = sink.lock().unwrap();
            let now = Instant::now();
            let echo = o.input.is_some_and(|t| now - t <= ECHO);
            if !echo {
                o.active = Some(now);
            }
            o.signals.feed(&buf[..n]);
            o.recent.extend_from_slice(&buf[..n]);
            if o.recent.len() > KEEP {
                // Cut at a line start so the replay doesn't begin mid escape sequence.
                let cut = o.recent.len() - KEEP;
                let cut = o.recent[cut..].iter().position(|&b| b == b'\n').map_or(cut, |i| cut + i + 1);
                o.recent.drain(..cut);
            }
            if let Some(c) = &o.chan {
                if c.send(InvokeResponseBody::Raw(buf[..n].to_vec())).is_err() {
                    o.chan = None; // window gone; output keeps accumulating until one re-attaches
                }
            }
        }
        // Empty message = shell exited.
        let mut o = sink.lock().unwrap();
        o.exited = true;
        if let Some(c) = &o.chan {
            let _ = c.send(InvokeResponseBody::Raw(Vec::new()));
        }
    });

    sessions.insert(id, Session { input, master: pair.master, child, out, cwd, watch: Default::default(), work: Default::default(), transcript: Default::default(), moved: None });
    Ok(true)
}

#[tauri::command]
fn pty_write(ptys: State<Ptys>, id: String, data: String) -> Res<()> {
    let map = ptys.sessions.lock().unwrap();
    let s = map.get(&id).ok_or("no such pty")?;
    let mut o = s.out.lock().unwrap();
    o.input = Some(Instant::now());
    // Answered (or dismissed). Focus reports the terminal sends by itself don't count.
    if !matches!(data.as_str(), "\x1b[I" | "\x1b[O") {
        o.asks.typed();
    }
    drop(o);
    s.input.send(data.into_bytes()).map_err(|_| "the shell has exited".into())
}

#[tauri::command]
fn pty_resize(ptys: State<Ptys>, id: String, cols: u16, rows: u16) -> Res<()> {
    let map = ptys.sessions.lock().unwrap();
    let s = map.get(&id).ok_or("no such pty")?;
    let mut o = s.out.lock().unwrap();
    o.input = Some(Instant::now());
    o.signals.resize(rows, cols);
    drop(o);
    s.master
        .resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .map_err(err)
}

#[tauri::command]
fn pty_kill(ptys: State<Ptys>, id: String) {
    if let Some(s) = ptys.sessions.lock().unwrap().remove(&id) {
        std::thread::spawn(move || hang_up(s)); // waits on the processes: off the main thread
    }
}

/// End a shell and whatever runs in its terminal, the way closing a terminal window does: SIGHUP to the shell's
/// process group and the foreground job's (an agent), SIGKILL to what's still there after a grace period, then reap.
/// Signalling the shell alone isn't enough: one killed before it passes the hangup on leaves the agent running
/// headless (the reader thread's copy of the terminal keeps it open, so the kernel never hangs it up).
fn hang_up(mut s: Session) {
    #[cfg(unix)]
    {
        let groups: Vec<i32> = [s.child.process_id().map(|p| p as i32), s.master.process_group_leader()]
            .into_iter()
            .flatten()
            .filter(|&g| g > 1)
            .collect();
        let signal = |sig: i32| groups.iter().filter(|&&g| unsafe { libc::killpg(g, sig) } == 0).count();
        signal(libc::SIGHUP);
        let grace = Instant::now();
        let gone = loop {
            if matches!(s.child.try_wait(), Ok(Some(_))) && signal(0) == 0 {
                break true;
            }
            if grace.elapsed() > std::time::Duration::from_secs(3) {
                break false;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        if !gone {
            signal(libc::SIGKILL);
        }
    }
    #[cfg(not(unix))]
    let _ = s.child.kill();
    let _ = s.child.wait();
}

/// Coding agent CLIs a terminal can run, by the command that starts them (Antigravity's is `agy`).
const AGENTS: [&str; 6] = ["claude", "codex", "agy", "grok", "opencode", "muse"];

/// The agent a process command line runs: its program, or the script given to an interpreter (npm installs such as
/// Codex's are `node .../bin/codex`). Names may carry an extension or a version: claude.exe, codex.js, grok-1.0.50, or
/// be the versioned binary a launcher script execs (muse-bin-1.4.7-R6101.1).
fn agent_in(args: &str) -> Option<&'static str> {
    // ponytail: splits on whitespace, so an install path with spaces isn't recognized.
    let name = |w: &str| w.rsplit('/').next().unwrap_or(w).split('.').next().unwrap_or("").to_string();
    let mut words = args.split_whitespace();
    let mut prog = name(words.next()?);
    if ["node", "bun", "deno", "sh", "bash", "zsh", "python", "python3"].contains(&prog.as_str()) {
        prog = name(words.find(|w| !w.starts_with('-'))?);
    }
    AGENTS.iter().copied().find(|a| {
        let rest = prog.strip_prefix(a).and_then(|r| r.strip_prefix('-'));
        let version = rest.map(|r| r.strip_prefix("bin-").unwrap_or(r));
        prog == *a || version.is_some_and(|v| v.starts_with(|c: char| c.is_ascii_digit()))
    })
}

#[derive(serde::Serialize)]
struct Shell {
    agent: Option<String>, // agent command in the foreground; None when it's the shell itself or another job
    quiet: Option<u64>,    // ms since its last output that wasn't an echo of input (None: none yet)
    title: String,
    cwd: String, // the agent's working directory now, or else the shell's
    busy: bool,
    attention: bool, // the agent rang, or finished, while no window had it on screen
    asking: Option<status::Ask>, // a prompt is up, waiting on the user
    since: u64,      // ms since the agent came or went, or started or stopped working
}

#[derive(Clone, serde::Serialize)]
struct Cued {
    id: String,
    cue: status::Cue,
}

/// What each shell is doing, by tab id. Only tabs with a shell are listed. Polled by every window, each passing the
/// tabs it has on screen (`seen`): what's in view in some window doesn't need attention. An agent out of view that
/// asks something or finishes is told to every window (`agent-cue`, for a notice) and makes a sound.
#[tauri::command]
async fn pty_status(ptys: State<'_, Ptys>, seen: Vec<String>, app: AppHandle) -> Res<HashMap<String, Shell>> {
    let mut jobs: Vec<(String, i32)> = Vec::new(); // tab id, foreground process group (other than the shell)
    #[cfg(unix)]
    for (id, s) in ptys.sessions.lock().unwrap().iter() {
        if let Some(pg) = s.master.process_group_leader() {
            if s.child.process_id() != Some(pg as u32) {
                jobs.push((id.clone(), pg));
            }
        }
    }
    let mut agents: HashMap<String, (&'static str, Vec<u32>)> = HashMap::new(); // tab id -> agent, its processes
    if !jobs.is_empty() {
        let ps = tauri::async_runtime::spawn_blocking(|| std::process::Command::new("/bin/ps").args(["-A", "-ww", "-o", "pid=,pgid=,args="]).output())
            .await
            .map_err(err)?
            .map_err(err)?;
        // Any process in the job counts: the agent may be a child of a wrapper.
        for line in String::from_utf8_lossy(&ps.stdout).lines() {
            let Some((pid, rest)) = line.trim_start().split_once(' ') else { continue };
            let Some((pg, args)) = rest.trim_start().split_once(' ') else { continue };
            let (Ok(pid), Some(agent)) = (pid.parse(), agent_in(args)) else { continue };
            for (id, _) in jobs.iter().filter(|(_, j)| pg.parse() == Ok(*j)) {
                agents.entry(id.clone()).or_insert((agent, vec![])).1.push(pid);
            }
        }
    }
    let now = Instant::now();
    let mut shells = HashMap::new();
    let mut cues = Vec::new();
    for (id, s) in ptys.sessions.lock().unwrap().iter_mut() {
        if seen.contains(id) {
            s.watch.seen(now);
        }
        // The agent's, while one runs: it may move itself into another worktree (Claude Code's EnterWorktree chdirs)
        // while the shell stays put. Any of its processes that moved counts: with a wrapper (Codex's `node codex.js`
        // runs a native `codex`) it may be the child. Gone shells keep the last one seen.
        let shell = s.child.process_id().and_then(cwd_of);
        let agent = agents.get(id);
        let moved = agent.and_then(|(_, pids)| pids.iter().filter_map(|&p| cwd_of(p)).find(|c| Some(c) != shell.as_ref()));
        // Claude Code records where it is: no guessing from its processes, nor sampling them.
        let told = agent.filter(|(a, _)| *a == "claude").and_then(|(_, pids)| s.transcript.claude(pids));
        s.work.set_agents(agent.filter(|_| told.is_none()).map(|(_, pids)| pids.clone()).unwrap_or_default());
        // The agent moving itself is newer than any command it ran before.
        if moved != s.moved {
            s.moved = moved.clone();
            s.work.dir = None;
        }
        // Then where its latest command ran: that's where it works, though the agent itself stays put.
        if let Some(cwd) = told.or(s.work.dir.clone()).or(moved).or(shell) {
            s.cwd = cwd;
        }
        let mut o = s.out.lock().unwrap();
        let quiet = o.active.map(|t| now.saturating_duration_since(t).as_millis() as u64);
        let rang = std::mem::take(&mut o.signals.rang);
        if let Some(at) = hooks::take(id) {
            o.asks.hooked(at);
        }
        let agent = agent.map(|(a, _)| *a);
        if agent.is_none() {
            o.asks.clear(); // whoever asked is gone
        }
        let screen = agent.and_then(|_| o.signals.prompt());
        let (asking, asked) = o.asks.check(screen);
        let busy = s.watch.step(now, agent, &o.signals.title, quiet, rang, asked);
        if let Some(cue) = s.watch.cue.take() {
            cues.push(Cued { id: id.clone(), cue });
        }
        shells.insert(id.clone(), Shell {
            agent: agent.map(String::from),
            quiet,
            title: o.signals.title.clone(),
            cwd: s.cwd.clone(),
            busy,
            attention: s.watch.attention,
            asking,
            since: now.saturating_duration_since(s.watch.changed).as_millis() as u64,
        });
    }
    // The most pressing of this poll's: several agents finishing together make one sound.
    if let Some(c) = cues.iter().map(|c| c.cue).max() {
        play(c);
    }
    for c in cues {
        let _ = app.emit("agent-cue", c);
    }
    Ok(shells)
}

/// Samples where each agent's commands run (see `workdir`), more often than `pty_status` is polled: most commands
/// are over in well under a second.
fn watch_work(app: &AppHandle) {
    loop {
        std::thread::sleep(std::time::Duration::from_millis(200));
        for s in app.state::<Ptys>().sessions.lock().unwrap().values_mut() {
            s.work.sample();
        }
    }
}

/// Sound Cues: a sound when an agent asks something (approval, a question) and another when it finishes its turn,
/// unless a window has its tab on screen.
/// Played here, not in a window: each window polls, and a shell may be in none of them.
static CUES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn play(cue: status::Cue) {
    if !CUES.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        // Told apart by rhythm as well as tone: a question pings twice, a finished turn chimes once.
        let (sound, times) = match cue {
            status::Cue::Ask => ("Ping", 2),
            status::Cue::Done => ("Glass", 1),
        };
        std::thread::spawn(move || {
            let mut played = Vec::new();
            for i in 0..times {
                if i > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(160));
                }
                played.extend(std::process::Command::new("/usr/bin/afplay").arg(format!("/System/Library/Sounds/{sound}.aiff")).spawn());
            }
            for mut c in played {
                let _ = c.wait();
            }
        });
    }
}

/// Turn Sound Cues on/off (`None` just reads it); `preview` plays the "done" sound. Every window hears the change.
#[tauri::command]
fn sound_cues(on: Option<bool>, preview: Option<bool>, app: AppHandle) -> bool {
    if let Some(on) = on {
        CUES.store(on, std::sync::atomic::Ordering::Relaxed);
        let _ = app.emit("sound-cues", on);
    }
    if preview == Some(true) {
        play(status::Cue::Done);
    }
    CUES.load(std::sync::atomic::Ordering::Relaxed)
}

/// A process's current working directory: the user may have `cd`'d out of the folder the tab was opened in.
#[cfg(target_os = "macos")]
fn cwd_of(pid: u32) -> Option<String> {
    let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_vnodepathinfo>() as i32;
    let n = unsafe { libc::proc_pidinfo(pid as i32, libc::PROC_PIDVNODEPATHINFO, 0, &mut info as *mut _ as *mut libc::c_void, size) };
    if n != size {
        return None;
    }
    let path = unsafe { std::ffi::CStr::from_ptr(info.pvi_cdir.vip_path.as_ptr() as *const libc::c_char) };
    Some(path.to_string_lossy().into_owned()).filter(|p| !p.is_empty())
}
#[cfg(not(target_os = "macos"))]
fn cwd_of(pid: u32) -> Option<String> {
    std::fs::read_link(format!("/proc/{pid}/cwd")).ok().map(|p| p.to_string_lossy().into_owned())
}

/// Is the left mouse button physically held right now (regardless of which window has the events)?
#[cfg(target_os = "macos")]
fn left_button_down() -> bool {
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceButtonState(state: i32, button: u32) -> bool;
    }
    // kCGEventSourceStateCombinedSessionState, kCGMouseButtonLeft
    unsafe { CGEventSourceButtonState(0, 0) }
}
#[cfg(not(target_os = "macos"))]
fn left_button_down() -> bool {
    false // ponytail: tear-off follows the cursor on macOS only; elsewhere the window just lands at the cursor
}

/// For JS drag tracking that has to survive the cursor leaving the window.
#[tauri::command]
fn mouse_down() -> bool {
    left_button_down()
}

/// The window (other than `skip`) whose title bar or workspace bar (~40 logical px under the title bar) is under `c`.
fn bar_under(app: &AppHandle, c: PhysicalPosition<f64>, skip: &str) -> Option<String> {
    // Windows, not webview windows: one showing a browser tab has more than one webview, and Tauri lists it only here
    app.windows().into_iter().find_map(|(label, w)| {
        let (p, s, inner, k) = (w.outer_position().ok()?, w.outer_size().ok()?, w.inner_position().ok()?, w.scale_factor().ok()?);
        let hit = label != skip
            && c.x >= p.x as f64
            && c.x < (p.x + s.width as i32) as f64
            && c.y >= p.y as f64
            && c.y < inner.y as f64 + 44.0 * k; // the top bar (also the title bar: overlay style, so inner = outer)
        hit.then_some(label)
    })
}

/// A menu shortcut for the app's page to run (`browser`: the browser tab that had the keyboard).
#[derive(Clone, serde::Serialize)]
struct Shortcut<'a> {
    key: &'a str,
    shift: bool,
    browser: Option<String>,
}

#[derive(Clone, serde::Serialize)]
struct TearDrop {
    label: String,
    target: Option<String>,
}

/// Show a window the app opened hidden. Every new window is: one created visible while the app is in full screen
/// is put on screen inside tao's event handler, where AppKit's full-screen transition redraws synchronously and
/// re-enters that handler while it holds its lock, hanging the app (tearing off a workspace in full screen did).
/// Shown afterwards, it just appears, in the full-screen space.
#[tauri::command]
async fn show_window(app: AppHandle, label: String) -> Res<()> {
    app.get_window(&label).ok_or("no such window")?.show().map_err(err)
}

/// Tab tear-off: window `label` (holding workspace `id`) follows the cursor at offset dx, dy (physical px) until the
/// left button is released; then `source` gets "tear-dropped". Runs here, not in JS: the webview stops getting
/// pointer events once a window moves or appears under the cursor.
/// Over another window's bar the follower hides and that window gets "merge-hint" (the workspace id, then null
/// when the cursor leaves), like a browser tab sliding into another window's tab strip.
#[tauri::command]
fn follow_cursor(app: AppHandle, label: String, id: String, source: String, dx: f64, dy: f64) {
    std::thread::spawn(move || {
        let mut hover: Option<String> = None;
        let mut shown = false; // a new follower is opened hidden (see show_window): shown once it exists
        loop {
            if let (Ok(c), Some(w)) = (app.cursor_position(), app.get_window(&label)) {
                let _ = w.set_position(PhysicalPosition::new(c.x - dx, c.y - dy));
                if !shown {
                    shown = true;
                    let _ = w.show();
                }
                let target = bar_under(&app, c, &label);
                if target != hover {
                    if let Some(old) = &hover {
                        let _ = app.emit_to(old.as_str(), "merge-hint", None::<String>);
                    }
                    if let Some(new) = &target {
                        let _ = app.emit_to(new.as_str(), "merge-hint", Some(&id));
                    }
                    let _ = if target.is_some() { w.hide() } else { w.show() };
                    hover = target;
                }
            }
            if !left_button_down() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
        if let Some(t) = &hover {
            let _ = app.emit_to(t.as_str(), "merge-hint", None::<String>);
        }
        // Released before the new window existed (a quick flick): it still has to be seen, unless it's merging.
        if !shown && hover.is_none() {
            for _ in 0..250 {
                if let Some(w) = app.get_window(&label) {
                    let _ = w.show();
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
        let _ = app.emit_to(source.as_str(), "tear-dropped", TearDrop { label, target: hover });
    });
}

#[derive(serde::Serialize)]
struct Entry {
    name: String,
    is_dir: bool,
}

#[tauri::command]
async fn list_dir(path: String) -> Res<Vec<Entry>> {
    blocking(move || {
        let mut out: Vec<Entry> = std::fs::read_dir(path)
            .map_err(err)?
            .filter_map(|e| e.ok())
            .map(|e| Entry {
                name: e.file_name().to_string_lossy().into_owned(),
                // A link to a folder is one (skills linked between agents' folders): the tree opens it, not reads it.
                is_dir: e.file_type().is_ok_and(|t| t.is_dir() || (t.is_symlink() && e.path().is_dir())),
            })
            .collect();
        out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        Ok(out)
    })
    .await
}

/// Text files bigger than this aren't opened: the whole text crosses the IPC and lands in an editor.
const MAX_TEXT: u64 = 16 * 1024 * 1024;

fn read_text(path: &str) -> Res<String> {
    let len = std::fs::metadata(path).map_err(err)?.len();
    if len > MAX_TEXT {
        return Err(format!("This file is too large to open here ({} MB).", len / (1024 * 1024)));
    }
    std::fs::read_to_string(path).map_err(err)
}

/// A file's version on disk (modified time and size), to tell whether someone else changed it since it was read.
fn stamp_of(path: &str) -> Option<String> {
    let m = std::fs::metadata(path).ok()?;
    let t = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(format!("{}:{}", t.as_nanos(), m.len()))
}

#[tauri::command]
async fn read_file(path: String) -> Res<String> {
    blocking(move || read_text(&path)).await
}

#[derive(serde::Serialize)]
struct Opened {
    text: String,
    stamp: Option<String>,
}

/// A file for the editor: its text, and its version (passed back to `write_file`, which refuses to overwrite a newer one).
#[tauri::command]
async fn open_file(path: String) -> Res<Opened> {
    blocking(move || {
        let stamp = stamp_of(&path);
        Ok(Opened { text: read_text(&path)?, stamp })
    })
    .await
}

/// The file's version on disk now (None: it's gone), for the editor to notice changes made elsewhere.
#[tauri::command]
async fn file_stamp(path: String) -> Option<String> {
    blocking(move || Ok(stamp_of(&path))).await.ok().flatten()
}

/// What `write_file` reports when the file changed on disk since `expect` (the editor asks before overwriting).
const CHANGED_ON_DISK: &str = "changed on disk";

/// Write `content` to `path` and return its new version. With `expect` (the version the text was read at), a file
/// changed since then is left alone and this fails with CHANGED_ON_DISK. The text goes to a temporary file that
/// replaces the old one, so a crash mid-write can't leave it cut short.
#[tauri::command]
async fn write_file(path: String, content: String, expect: Option<String>) -> Res<Option<String>> {
    blocking(move || {
        if let Some(expect) = expect {
            if stamp_of(&path).is_some_and(|now| now != expect) {
                return Err(CHANGED_ON_DISK.into());
            }
        }
        write_atomic(Path::new(&path), content.as_bytes()).map_err(err)?;
        Ok(stamp_of(&path))
    })
    .await
}

/// Replace `path` with `bytes` via a temporary file beside it, keeping its permissions and following a symlink
/// (the link stays; its target is replaced).
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let dir = path.parent().ok_or_else(|| std::io::Error::other("no parent folder"))?;
    let name = path.file_name().ok_or_else(|| std::io::Error::other("no file name"))?.to_string_lossy();
    let tmp = dir.join(format!(".{name}.pretorium-{}", std::process::id()));
    let result = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        if let Ok(m) = std::fs::metadata(&path) {
            f.set_permissions(m.permissions())?;
        }
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, &path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[tauri::command]
fn create_file(path: String) -> Res<()> {
    // create_new: never clobber an existing file
    std::fs::OpenOptions::new().write(true).create_new(true).open(path).map(|_| ()).map_err(err)
}

#[tauri::command]
fn create_dir(path: String) -> Res<()> {
    std::fs::create_dir(path).map_err(err)
}

#[tauri::command]
async fn move_path(from: String, to: String) -> Res<()> {
    blocking(move || {
        // fs::rename silently replaces an existing file on Unix.
        // ponytail: exists-check then rename is racy; fine for a single-user editor.
        if std::path::Path::new(&to).exists() {
            return Err(format!("{to} already exists"));
        }
        std::fs::rename(from, to).map_err(err)
    })
    .await
}

#[tauri::command]
async fn copy_path(from: String, dir: String) -> Res<String> {
    blocking(move || copy_into(from, dir)).await
}

/// Copy a file or folder into `dir`, Finder-style: "a.txt", then "a copy.txt", "a copy 2.txt"... Returns the new path.
fn copy_into(from: String, dir: String) -> Res<String> {
    let src = Path::new(&from);
    let name = src.file_name().ok_or("nothing to copy")?.to_string_lossy().into_owned();
    // Folders and dotfiles have no extension to keep at the end.
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 && !src.is_dir() => (&name[..i], &name[i..]),
        _ => (name.as_str(), ""),
    };
    let to = (0..)
        .map(|n| Path::new(&dir).join(match n {
            0 => name.clone(),
            1 => format!("{stem} copy{ext}"),
            _ => format!("{stem} copy {n}{ext}"),
        }))
        .find(|p| !p.exists())
        .unwrap();
    copy_rec(src, &to).map_err(err)?;
    Ok(to.to_string_lossy().into_owned())
}

fn copy_rec(from: &Path, to: &Path) -> std::io::Result<()> {
    // ponytail: symlinks are copied as what they point to (a link to a folder fails); recreate links if that matters.
    if std::fs::symlink_metadata(from)?.is_dir() {
        std::fs::create_dir(to)?;
        for e in std::fs::read_dir(from)? {
            let e = e?;
            copy_rec(&e.path(), &to.join(e.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to).map(|_| ())
    }
}

/// Put a file on the system clipboard, the way Finder's Copy does (Finder and other apps can paste it).
#[tauri::command]
fn clipboard_set_file(path: String) -> Res<()> {
    #[cfg(target_os = "macos")]
    {
        use objc2::runtime::ProtocolObject;
        use objc2_app_kit::NSPasteboard;
        use objc2_foundation::{NSArray, NSString, NSURL};
        let pb = NSPasteboard::generalPasteboard();
        pb.clearContents();
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path));
        if !pb.writeObjects(&NSArray::from_retained_slice(&[ProtocolObject::from_retained(url)])) {
            return Err("Couldn't write to the clipboard".into());
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        // ponytail: other platforms keep it in-app only; wire up their file clipboards if we ship there.
        *CLIP.lock().unwrap() = vec![path];
    }
    Ok(())
}

/// Files on the system clipboard (copied here or in Finder etc.), if any.
#[tauri::command]
fn clipboard_files() -> Vec<String> {
    #[cfg(target_os = "macos")]
    return pasteboard_files(&objc2_app_kit::NSPasteboard::generalPasteboard());
    #[cfg(not(target_os = "macos"))]
    CLIP.lock().unwrap().clone()
}

/// Files of the drag just dropped on the page (from Finder etc.): the webview's drop event only has their contents.
#[tauri::command]
fn drag_files() -> Vec<String> {
    #[cfg(target_os = "macos")]
    return pasteboard_files(&objc2_app_kit::NSPasteboard::pasteboardWithName(unsafe { objc2_app_kit::NSPasteboardNameDrag }));
    #[cfg(not(target_os = "macos"))]
    vec![]
}

#[cfg(target_os = "macos")]
fn pasteboard_files(pb: &objc2_app_kit::NSPasteboard) -> Vec<String> {
    use objc2::ClassType;
    use objc2_app_kit::NSPasteboardURLReadingFileURLsOnlyKey;
    use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSURL};
    let classes = NSArray::from_slice(&[NSURL::class()]);
    let yes = NSNumber::new_bool(true);
    let opts = NSDictionary::from_slices(&[unsafe { NSPasteboardURLReadingFileURLsOnlyKey }], &[&*yes as &objc2::runtime::AnyObject]);
    let Some(objs) = (unsafe { pb.readObjectsForClasses_options(&classes, Some(&opts)) }) else { return vec![] };
    objs.iter()
        .filter_map(|o| o.downcast::<NSURL>().ok())
        .filter_map(|u| u.path().map(|p| p.to_string()))
        .collect()
}

/// A dropped file with no path to give (an image from a browser): saved to a temp folder.
/// Body = the bytes, header `name` = the URI-encoded file name. Returns the saved file's path.
#[tauri::command]
fn save_drop(request: tauri::ipc::Request) -> Res<String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else { return Err("Expected the file's bytes".into()) };
    let name = request.headers().get("name").and_then(|v| v.to_str().ok()).unwrap_or("");
    let path = drop_path(&percent_decode(name)).map_err(|e| e.to_string())?;
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

/// Where to keep a dropped file named `name`: a fresh folder under the temp dir, so names never clash.
fn drop_path(name: &str) -> std::io::Result<std::path::PathBuf> {
    // Only the last component: the name comes from the page
    let name = std::path::Path::new(name).file_name().map(|n| n.to_string_lossy().into_owned()).filter(|n| !n.is_empty()).unwrap_or_else(|| "dropped".into());
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let dir = std::env::temp_dir().join("pretorium-drops").join(stamp.to_string());
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join(name))
}

/// Screenshot thumbnails (and other file promises) never reach the page: WKWebView fails to receive the promise and
/// fires no `drop`. Their pasteboard also carries the file's URL though, so catch that drop natively instead: copy
/// the files (the source deletes its temp copy) and hand the paths to the page as "promise-drop", which types them
/// into the terminal the drag was over. Every other drop goes to the webview as before.
#[cfg(target_os = "macos")]
fn catch_promise_drops(webview: &tauri::Webview) {
    let app = webview.app_handle().clone();
    // Hooked on the webview's own class (wry's WKWebView subclass): its runtime name isn't stable
    let _ = webview.with_webview(move |wv| unsafe { hook_drops((*(wv.inner() as *const objc2::runtime::AnyObject)).class(), app) });
}

#[cfg(target_os = "macos")]
fn hook_drops(cls: &objc2::runtime::AnyClass, app: AppHandle) {
    use objc2::runtime::{AnyObject, Bool, Imp, Sel};
    use objc2::{msg_send, rc::Retained, sel};
    use objc2_app_kit::NSPasteboard;
    static APP: std::sync::OnceLock<AppHandle> = std::sync::OnceLock::new();
    static ORIGINAL: std::sync::OnceLock<Imp> = std::sync::OnceLock::new();
    if APP.set(app).is_err() {
        return;
    }
    unsafe extern "C-unwind" fn perform(this: &AnyObject, cmd: Sel, info: &AnyObject) -> Bool {
        let pb: Retained<NSPasteboard> = msg_send![info, draggingPasteboard];
        let promised = pb.types().is_some_and(|ts| {
            ts.iter().any(|t| ["com.apple.NSFilePromiseItemMetaData", "com.apple.pasteboard.promised-file-url"].contains(&&*t.to_string()))
        });
        let files: Vec<String> = if promised { pasteboard_files(&pb).into_iter().filter(|p| std::path::Path::new(p).is_file()).collect() } else { vec![] };
        if files.is_empty() {
            let Some(&original) = ORIGINAL.get() else { return Bool::NO };
            let original: unsafe extern "C-unwind" fn(&AnyObject, Sel, &AnyObject) -> Bool = std::mem::transmute(original);
            return original(this, cmd, info);
        }
        let _: () = msg_send![this, draggingExited: info]; // end the drag in the page (it never gets a drop)
        let copies: Vec<String> = files
            .iter()
            .filter_map(|f| {
                let to = drop_path(f.rsplit('/').next().unwrap_or("")).ok()?;
                std::fs::copy(f, &to).ok()?;
                Some(to.to_string_lossy().into_owned())
            })
            .collect();
        let _ = APP.get().unwrap().emit("promise-drop", copies);
        Bool::YES
    }
    // Past the KVO subclass macOS swaps in at runtime, to the class wry defines
    let mut cls = cls;
    while cls.name().to_string_lossy().contains("NSKVONotifying_") {
        let Some(sup) = cls.superclass() else { return };
        cls = sup;
    }
    let Some(original) = cls.instance_method(sel!(performDragOperation:)).map(|m| m.implementation()) else { return };
    let _ = ORIGINAL.set(original);
    let imp: Imp = unsafe { std::mem::transmute(perform as unsafe extern "C-unwind" fn(&AnyObject, Sel, &AnyObject) -> Bool) };
    unsafe { objc2::ffi::class_replaceMethod(cls as *const _ as *mut _, sel!(performDragOperation:), imp, c"B@:@".as_ptr()) };
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        match (b[i], b.get(i + 1).and_then(|&c| hex(c)), b.get(i + 2).and_then(|&c| hex(c))) {
            (b'%', Some(h), Some(l)) => (out.push((h * 16 + l) as u8), i += 3),
            (c, _, _) => (out.push(c), i += 1),
        };
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(not(target_os = "macos"))]
static CLIP: Mutex<Vec<String>> = Mutex::new(Vec::new());

#[tauri::command]
fn reveal_path(path: String) -> Res<()> {
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(target_os = "macos")]
    cmd.args(["-R", &path]);
    #[cfg(target_os = "windows")]
    let mut cmd = std::process::Command::new("explorer");
    #[cfg(target_os = "windows")]
    cmd.arg(format!("/select,{path}"));
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut cmd = std::process::Command::new("xdg-open");
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    cmd.arg(Path::new(&path).parent().unwrap_or(Path::new("/")));
    cmd.spawn().map(|_| ()).map_err(err)
}

/// Open a link from a terminal in the default browser. Web links only: OSC 8 lets programs put any URI there.
#[tauri::command]
fn open_url(url: String) -> Res<()> {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err(format!("not a web link: {url}"));
    }
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut cmd = std::process::Command::new("explorer");
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut cmd = std::process::Command::new("xdg-open");
    cmd.arg(&url).spawn().map(|_| ()).map_err(err)
}

/// Move `path` into a fresh subfolder of `dir` (the undo holding area) and return where it went.
fn hold(path: &Path, dir: &Path) -> std::io::Result<PathBuf> {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let slot = dir.join(nanos.to_string());
    std::fs::create_dir_all(&slot)?;
    let target = slot.join(path.file_name().ok_or_else(|| std::io::Error::other("no file name"))?);
    std::fs::rename(path, &target).inspect_err(|_| {
        let _ = std::fs::remove_dir(&slot);
    })?;
    Ok(target)
}

fn to_system_trash(path: &Path) -> Result<(), trash::Error> {
    #[allow(unused_mut)]
    let mut ctx = trash::TrashContext::default();
    #[cfg(target_os = "macos")]
    {
        // Default method scripts Finder (automation permission prompt); NSFileManager doesn't.
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        ctx.set_delete_method(DeleteMethod::NsFileManager);
    }
    ctx.delete(path)
}

fn holding_dir(app: &AppHandle) -> Res<PathBuf> {
    Ok(app.path().app_data_dir().map_err(err)?.join("trash"))
}

/// Recoverable delete. Returns the held path (undo = move_path back), or None when the item had to
/// go straight to the system Trash (different volume than the holding area) and can't be undone here.
#[tauri::command]
async fn trash_path(app: AppHandle, path: String) -> Res<Option<String>> {
    let dir = holding_dir(&app)?;
    blocking(move || match hold(Path::new(&path), &dir) {
        Ok(p) => Ok(Some(p.to_string_lossy().into_owned())),
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
            to_system_trash(Path::new(&path)).map_err(err)?;
            Ok(None)
        }
        Err(e) => Err(err(e)),
    })
    .await
}

/// Last session's undo history is gone, so its held items go to the system Trash (never deleted outright).
fn flush_holding(app: &AppHandle) {
    let Ok(dir) = holding_dir(app) else { return };
    let Ok(slots) = std::fs::read_dir(&dir) else { return };
    for slot in slots.flatten() {
        let items: Vec<_> = std::fs::read_dir(slot.path()).into_iter().flatten().flatten().collect();
        if items.iter().all(|i| to_system_trash(&i.path()).is_ok()) {
            let _ = std::fs::remove_dir(slot.path());
        }
    }
}

/// Stay Awake: a `caffeinate` child while on. `-w <our pid>` makes it exit with the app, even after a crash.
#[derive(Default)]
struct Awake(Mutex<Option<std::process::Child>>);

/// Turn Stay Awake on/off (`None` just reads it). Every window hears the change, so their buttons agree.
#[tauri::command]
fn stay_awake(on: Option<bool>, awake: State<Awake>, app: AppHandle) -> Res<bool> {
    let mut child = awake.0.lock().unwrap();
    match on {
        Some(true) if child.is_none() => {
            // -d: display stays on, -i: no idle system sleep.
            let c = std::process::Command::new("/usr/bin/caffeinate")
                .args(["-d", "-i", "-w", &std::process::id().to_string()])
                .spawn()
                .map_err(err)?;
            *child = Some(c);
        }
        Some(false) => {
            if let Some(mut c) = child.take() {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
        _ => return Ok(child.is_some()),
    }
    let _ = app.emit("stay-awake", child.is_some());
    Ok(child.is_some())
}

/// Reload bypassing the cache (WKWebView's reloadFromOrigin); plain reload elsewhere.
fn force_reload(w: &tauri::Webview) {
    #[cfg(target_os = "macos")]
    let _ = w.with_webview(|wv| unsafe {
        let wv = &*(wv.inner() as *const objc2::runtime::AnyObject);
        let _: *mut objc2::runtime::AnyObject = objc2::msg_send![wv, reloadFromOrigin: std::ptr::null::<objc2::runtime::AnyObject>()];
    });
    #[cfg(not(target_os = "macos"))]
    let _ = w.reload();
}

/// Let the webview load `path` over the asset protocol (images, video, 3D models), whose scope starts empty: the app
/// shows files it was asked to, not anything on disk a script might fetch. A .gltf's textures and buffers sit beside it.
#[tauri::command]
fn allow_asset(app: AppHandle, path: String) -> Res<()> {
    let p = Path::new(&path);
    let scope = app.asset_protocol_scope();
    if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("gltf")) {
        scope.allow_directory(p.parent().ok_or("no folder")?, true).map_err(err)
    } else {
        scope.allow_file(p).map_err(err)
    }
}

/// Quit for real, once a window has dealt with "quit-requested" (asked about working agents, saved unsaved edits).
/// Every shell is ended the way closing its tab does it, so no agent outlives the app running headless.
/// `restart`: start again afterwards (an update was just installed).
#[tauri::command]
async fn quit(app: AppHandle, ptys: State<'_, Ptys>, restart: Option<bool>) -> Res<()> {
    let sessions: Vec<Session> = ptys.sessions.lock().unwrap().drain().map(|(_, s)| s).collect();
    blocking(move || {
        for t in sessions.into_iter().map(|s| std::thread::spawn(move || hang_up(s))).collect::<Vec<_>>() {
            let _ = t.join();
        }
        Ok(())
    })
    .await?;
    if restart == Some(true) {
        app.restart();
    }
    app.exit(0);
    Ok(())
}

/// Windows only ever show the app itself. A link or form in previewed markdown (a repo's README) must not be able to
/// navigate one to a remote page, which would then run inside the app's window. (Browser tabs have their own rules: web.rs.)
fn app_url(url: &tauri::Url) -> bool {
    match url.scheme() {
        "tauri" | "about" => true,
        "http" | "https" => matches!(url.host_str(), Some("tauri.localhost")) || (cfg!(dev) && url.host_str() == Some("localhost")),
        _ => false,
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri::plugin::Builder::<tauri::Wry>::new("navigation-guard").on_navigation(|w, url| w.label().starts_with(web::PREFIX) || app_url(url)).build())
        // Default macOS menu binds ⌘W to Close Window; we want it for tabs.
        .menu(|h| {
            use tauri::menu::{Menu, MenuItem, PredefinedMenuItem as P, Submenu};
            let item = |id: &str, text: &str, accel: &str| MenuItem::with_id(h, id, text, true, Some(accel));
            Menu::with_items(h, &[
                &Submenu::with_items(h, "Pretorium", true, &[
                    // Our own Quit: the windows get to ask about agents still working and keep unsaved edits first.
                    &P::about(h, None, None)?, &MenuItem::with_id(h, "check-updates", "Check for Updates…", true, None::<&str>)?,
                    &P::separator(h)?, &P::hide(h, None)?, &item("quit", "Quit Pretorium", "CmdOrCtrl+Q")?,
                ])?,
                // The app's own shortcuts, handled by its page (+page.svelte). Here so they also work while a browser tab
                // has the keyboard: a page sees a key first and only what it leaves reaches the menu.
                &Submenu::with_items(h, "File", true, &[
                    &item("key:t", "New Terminal", "CmdOrCtrl+T")?, &item("key:shift+t", "New Browser Tab", "CmdOrCtrl+Shift+T")?,
                    &item("key:n", "New Note", "CmdOrCtrl+N")?, &P::separator(h)?,
                    &item("key:o", "Open Folder…", "CmdOrCtrl+O")?, &item("key:p", "Switch Folder…", "CmdOrCtrl+P")?, &P::separator(h)?,
                    &item("key:w", "Close Tab", "CmdOrCtrl+W")?,
                ])?,
                &Submenu::with_items(h, "Edit", true, &[
                    &P::undo(h, None)?, &P::redo(h, None)?, &P::separator(h)?,
                    &P::cut(h, None)?, &P::copy(h, None)?, &P::paste(h, None)?, &P::select_all(h, None)?,
                ])?,
                &Submenu::with_items(h, "View", true, &[
                    &item("reload", "Reload", "CmdOrCtrl+R")?, &item("force-reload", "Force Reload", "CmdOrCtrl+Shift+R")?, &P::separator(h)?,
                    &item("key:b", "Toggle Sidebar", "CmdOrCtrl+B")?, &item("key:d", "Split Right", "CmdOrCtrl+D")?, &item("key:shift+d", "Split Down", "CmdOrCtrl+Shift+D")?, &P::separator(h)?,
                    &item("key:l", "Open Location", "CmdOrCtrl+L")?, &item("key:[", "Back", "CmdOrCtrl+[")?, &item("key:]", "Forward", "CmdOrCtrl+]")?, &P::separator(h)?,
                    &item("zoom-reset", "Actual Size", "CmdOrCtrl+0")?, &item("zoom-in", "Zoom In", "CmdOrCtrl+=")?, &item("zoom-out", "Zoom Out", "CmdOrCtrl+-")?,
                ])?,
                &Submenu::with_items(h, "Window", true, &[&P::minimize(h, None)?, &P::fullscreen(h, None)?])?,
            ])
        })
        .on_menu_event(|h, e| {
            if let id @ ("quit" | "check-updates") = e.id().as_ref() {
                // One window runs these (the focused one, else any); with none left there's nothing to ask about.
                let windows = h.windows();
                let w = windows.values().find(|w| w.is_focused().unwrap_or(false)).or_else(|| windows.values().next());
                let event = if id == "quit" { "quit-requested" } else { "check-updates" };
                match w {
                    Some(w) => { let _ = h.emit_to(tauri::EventTarget::webview_window(w.label()), event, ()); }
                    None if id == "quit" => h.exit(0),
                    None => {}
                }
                return;
            }
            let Some(w) = h.windows().into_values().find(|w| w.is_focused().unwrap_or(false)) else { return };
            let browser = web::focused(h, &w); // a browser tab has the keyboard (menu events come on the main thread)
            if let Some(key) = e.id().as_ref().strip_prefix("key:") {
                // Back/Forward stay in the browser; anything else acts on the app, which takes the keyboard back
                if browser.is_some() && !matches!(key, "[" | "]") {
                    web::focus_app(h, w.label());
                }
                let (shift, key) = key.strip_prefix("shift+").map_or((false, key), |k| (true, k));
                let _ = h.emit_to(tauri::EventTarget::webview_window(w.label()), "shortcut", Shortcut { key, shift, browser });
                return;
            }
            let Some(page) = h.get_webview(w.label()) else { return };
            match e.id().as_ref() {
                "reload" => match browser.and_then(|id| h.get_webview(&format!("{}{id}", web::PREFIX))) {
                    Some(b) => { let _ = b.reload(); }
                    None => { let _ = page.reload(); }
                },
                "force-reload" => force_reload(&page),
                // A focused browser tab zooms just its page, which keeps the keyboard
                id @ ("zoom-in" | "zoom-out" | "zoom-reset") if browser.is_some() => {
                    web::zoom(h, browser.as_deref().unwrap_or_default(), match id { "zoom-in" => 1, "zoom-out" => -1, _ => 0 });
                }
                // The app's zoom level lives in the frontend (persisted, and read back for drag math).
                id @ ("zoom-in" | "zoom-out" | "zoom-reset") => { let _ = h.emit_to(tauri::EventTarget::webview_window(page.label()), "zoom", id); }
                _ => {}
            }
        })
        .on_page_load(|webview, _| {
            // The webview class exists once a page loads; installed once
            #[cfg(target_os = "macos")]
            catch_promise_drops(webview);
        })
        .setup(|app| {
            std::thread::spawn(hooks::install);
            let h = app.handle().clone();
            std::thread::spawn(move || flush_holding(&h));
            let h = app.handle().clone();
            std::thread::spawn(move || watch_work(&h));
            Ok(())
        })
        .manage(Ptys::default())
        .manage(web::Views::default())
        .manage(Awake::default())
        .invoke_handler(tauri::generate_handler![
            follow_cursor, mouse_down, pty_open, pty_write, pty_resize, pty_kill, pty_status, list_dir, read_file, open_file, file_stamp, write_file, quit, allow_asset, show_window, create_file, create_dir, move_path, copy_path, clipboard_set_file, clipboard_files, drag_files, save_drop, reveal_path, open_url, trash_path, git, stay_awake, sound_cues, usage::usage_stats, usage::usage_limits, skills::skills_scan, skills::skill_create, skills::skill_link, skills::skill_delete, mcp::mcp_scan, mcp::mcp_reveal, mcp::mcp_add, mcp::mcp_copy, mcp::mcp_update, mcp::mcp_remove, mcp::mcp_set_enabled, agent_config::agent_configs, agent_config::agent_launchers, agent_config::agent_config_save, agent_config::agent_config_set,
            web::web_show, web::web_hide, web::web_close, web::web_hide_all, web::web_navigate, web::web_go, web::web_reload, web::web_stop, web::web_focus, web::web_focused
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_a_processs_working_directory() {
        let dir = std::fs::canonicalize(std::env::temp_dir()).unwrap();
        let mut child = std::process::Command::new("/bin/sleep").arg("5").current_dir(&dir).spawn().unwrap();
        let cwd = super::cwd_of(child.id());
        child.kill().unwrap();
        assert_eq!(cwd.as_deref(), dir.to_str());
        assert_eq!(super::cwd_of(u32::MAX >> 1), None);
    }

    #[test]
    fn recognizes_agents_by_program_or_script() {
        use super::agent_in;
        assert_eq!(agent_in("claude --model opus"), Some("claude"));
        assert_eq!(agent_in("/opt/lib/node_modules/@anthropic-ai/claude-code/bin/claude.exe"), Some("claude"));
        assert_eq!(agent_in("node /Users/me/.bun/bin/codex resume"), Some("codex"));
        assert_eq!(agent_in("node --no-warnings /x/opencode.js"), Some("opencode"));
        assert_eq!(agent_in("/Users/me/.grok/bin/grok-1.0.50"), Some("grok"));
        assert_eq!(agent_in("agy"), Some("agy"));
        assert_eq!(agent_in("/Users/me/.local/bin/muse-bin-1.4.7-R6101.1 --model muse-spark-1.3"), Some("muse"));
        assert_eq!(agent_in("bash /Users/me/.local/bin/muse resume --last"), Some("muse"));
        assert_eq!(agent_in("musescore"), None);
        assert_eq!(agent_in("muse-bin"), None);
        assert_eq!(agent_in("/bin/sh ./codex --flag"), Some("codex"));
        assert_eq!(agent_in("vim claude.md"), None);
        assert_eq!(agent_in("node server.js"), None);
        assert_eq!(agent_in("claude-monitor"), None);
        assert_eq!(agent_in("-zsh"), None);
    }

    #[test]
    fn decodes_dropped_file_names() {
        use super::percent_decode;
        assert_eq!(percent_decode("Screenshot%202026-10-11%20at%2010.00.00%E2%80%AFAM.png"), "Screenshot 2026-10-11 at 10.00.00\u{202f}AM.png");
        assert_eq!(percent_decode("100%.txt"), "100%.txt");
        assert_eq!(percent_decode("a%2"), "a%2");
    }

    #[test]
    fn hold_moves_into_holding_and_back() {
        let base = std::env::temp_dir().join(format!("fc-hold-{}", std::process::id()));
        let src = base.join("ws/notes.md");
        std::fs::create_dir_all(src.parent().unwrap()).unwrap();
        std::fs::write(&src, "hi").unwrap();
        let held = super::hold(&src, &base.join("trash")).unwrap();
        assert!(!src.exists());
        assert_eq!(std::fs::read_to_string(&held).unwrap(), "hi");
        assert_eq!(held.file_name().unwrap(), "notes.md");
        std::fs::rename(&held, &src).unwrap(); // what undo does
        assert!(src.exists());
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn atomic_writes_keep_mode_and_links_and_stamps_see_changes() {
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::temp_dir().join(format!("fc-write-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let file = base.join("run.sh");
        std::fs::write(&file, "old").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
        let link = base.join("link.sh");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        let path = file.to_str().unwrap();
        let before = super::stamp_of(path).unwrap();

        super::write_atomic(&link, b"new text").unwrap();
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "new text");
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o755);
        assert_ne!(super::stamp_of(path).unwrap(), before);
        assert_eq!(std::fs::read_dir(&base).unwrap().count(), 2); // no temp file left behind
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn windows_only_navigate_within_the_app() {
        let ok = |u: &str| super::app_url(&u.parse().unwrap());
        assert!(ok("tauri://localhost/"));
        assert!(ok("http://tauri.localhost/index.html"));
        assert!(!ok("https://evil.example/"));
        assert!(!ok("http://tauri.localhost.evil.example/"));
        assert!(!ok("file:///etc/passwd"));
        assert!(!ok("data:text/html,hi"));
    }

    #[test]
    fn copy_picks_free_names_and_recurses() {
        let base = std::env::temp_dir().join(format!("fc-copy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("src/sub")).unwrap();
        std::fs::write(base.join("src/sub/a.txt"), "hi").unwrap();
        let dir = base.join("src").to_string_lossy().into_owned();
        let file = base.join("src/sub/a.txt").to_string_lossy().into_owned();
        let sub = base.join("src/sub").to_string_lossy().into_owned();
        let sub_dir = sub.clone();
        assert!(super::copy_into(file.clone(), sub_dir.clone()).unwrap().ends_with("a copy.txt"));
        assert!(super::copy_into(file, sub_dir).unwrap().ends_with("a copy 2.txt"));
        let copied = super::copy_into(sub, dir).unwrap();
        assert!(copied.ends_with("sub copy"));
        assert_eq!(std::fs::read_to_string(format!("{copied}/a copy 2.txt")).unwrap(), "hi");
        std::fs::remove_dir_all(&base).unwrap();
    }
}

