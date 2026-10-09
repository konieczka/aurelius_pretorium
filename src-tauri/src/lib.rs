use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::ipc::{Channel, InvokeResponseBody};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, State};

mod agent_config;
mod skills;
mod usage;

struct Session {
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    out: Arc<Mutex<Output>>,
}

/// Where a shell's output goes, plus its recent output so a window that (re)attaches can replay it.
#[derive(Default)]
struct Output {
    chan: Option<Channel<InvokeResponseBody>>,
    recent: Vec<u8>,
    exited: bool,
    input: Option<Instant>,  // last write or resize from the window
    active: Option<Instant>, // last output that wasn't just answering that (echo, redraw)
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
        // ponytail: an rc file that blocks on input would hang this once; no timeout.
        std::process::Command::new(shell)
            .args(["-ilc", r#"printf "__FC__%s__FC__" "$PATH""#])
            .env_clear()
            .envs(clean_env())
            .stdin(std::process::Stdio::null())
            .output()
            .ok()
            .and_then(|o| String::from_utf8_lossy(&o.stdout).split("__FC__").nth(1).map(String::from))
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin".into())
    })
}

/// Run git with an argument list (no shell). Async + blocking pool so pull/push never freeze the UI.
#[tauri::command]
async fn git(cwd: String, args: Vec<String>) -> Res<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let out = std::process::Command::new("git")
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
    .map_err(err)?
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
    cmd.cwd(cwd);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    let child = pair.slave.spawn_command(cmd).map_err(err)?;
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().map_err(err)?;
    let writer = pair.master.take_writer().map_err(err)?;
    let out = Arc::new(Mutex::new(Output { chan: Some(on_data), ..Default::default() }));
    let sink = out.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 16 * 1024];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            let mut o = sink.lock().unwrap();
            let now = Instant::now();
            if o.input.is_none_or(|t| now - t > ECHO) {
                o.active = Some(now);
            }
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

    sessions.insert(id, Session { writer, master: pair.master, child, out });
    Ok(true)
}

#[tauri::command]
fn pty_write(ptys: State<Ptys>, id: String, data: String) -> Res<()> {
    let mut map = ptys.sessions.lock().unwrap();
    let s = map.get_mut(&id).ok_or("no such pty")?;
    s.out.lock().unwrap().input = Some(Instant::now());
    s.writer.write_all(data.as_bytes()).map_err(err)
}

#[tauri::command]
fn pty_resize(ptys: State<Ptys>, id: String, cols: u16, rows: u16) -> Res<()> {
    let map = ptys.sessions.lock().unwrap();
    let s = map.get(&id).ok_or("no such pty")?;
    s.out.lock().unwrap().input = Some(Instant::now());
    s.master
        .resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .map_err(err)
}

#[tauri::command]
fn pty_kill(ptys: State<Ptys>, id: String) {
    if let Some(mut s) = ptys.sessions.lock().unwrap().remove(&id) {
        let _ = s.child.kill();
    }
}

/// Coding agent CLIs a terminal can run, by the command that starts them (Antigravity's is `agy`).
const AGENTS: [&str; 5] = ["claude", "codex", "agy", "grok", "opencode"];

/// The agent a process command line runs: its program, or the script given to an interpreter (npm installs such as
/// Codex's are `node .../bin/codex`). Names may carry an extension or a version: claude.exe, codex.js, grok-1.0.50.
fn agent_in(args: &str) -> Option<&'static str> {
    // ponytail: splits on whitespace, so an install path with spaces isn't recognized.
    let name = |w: &str| w.rsplit('/').next().unwrap_or(w).split('.').next().unwrap_or("").to_string();
    let mut words = args.split_whitespace();
    let mut prog = name(words.next()?);
    if ["node", "bun", "deno", "sh", "bash", "zsh", "python", "python3"].contains(&prog.as_str()) {
        prog = name(words.find(|w| !w.starts_with('-'))?);
    }
    AGENTS.iter().copied().find(|a| {
        prog == *a || prog.strip_prefix(a).and_then(|r| r.strip_prefix('-')).is_some_and(|v| v.starts_with(|c: char| c.is_ascii_digit()))
    })
}

#[derive(serde::Serialize)]
struct Shell {
    agent: Option<String>, // agent command in the foreground; None when it's the shell itself or another job
    quiet: Option<u64>,    // ms since its last output that wasn't an echo of input (None: none yet)
}

/// What each shell is doing, by tab id. Only tabs with a shell are listed. Polled by the windows.
#[tauri::command]
async fn pty_status(ptys: State<'_, Ptys>) -> Res<HashMap<String, Shell>> {
    let mut shells: HashMap<String, Shell> = HashMap::new();
    let mut jobs: Vec<(String, i32)> = Vec::new(); // tab id, foreground process group (other than the shell)
    for (id, s) in ptys.sessions.lock().unwrap().iter() {
        let quiet = s.out.lock().unwrap().active.map(|t| t.elapsed().as_millis() as u64);
        shells.insert(id.clone(), Shell { agent: None, quiet });
        #[cfg(unix)]
        if let Some(pg) = s.master.process_group_leader() {
            if s.child.process_id() != Some(pg as u32) {
                jobs.push((id.clone(), pg));
            }
        }
    }
    if jobs.is_empty() {
        return Ok(shells);
    }
    let ps = tauri::async_runtime::spawn_blocking(|| std::process::Command::new("/bin/ps").args(["-A", "-ww", "-o", "pgid=,args="]).output())
        .await
        .map_err(err)?
        .map_err(err)?;
    // Any process in the job counts: the agent may be a child of a wrapper.
    for line in String::from_utf8_lossy(&ps.stdout).lines() {
        let Some((pg, args)) = line.trim_start().split_once(' ') else { continue };
        let Some(agent) = agent_in(args) else { continue };
        for (id, _) in jobs.iter().filter(|(_, j)| pg.parse() == Ok(*j)) {
            shells.get_mut(id).unwrap().agent = Some(agent.to_string());
        }
    }
    Ok(shells)
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
    app.webview_windows().into_iter().find_map(|(label, w)| {
        let (p, s, inner, k) = (w.outer_position().ok()?, w.outer_size().ok()?, w.inner_position().ok()?, w.scale_factor().ok()?);
        let hit = label != skip
            && c.x >= p.x as f64
            && c.x < (p.x + s.width as i32) as f64
            && c.y >= p.y as f64
            && c.y < inner.y as f64 + 40.0 * k;
        hit.then_some(label)
    })
}

#[derive(Clone, serde::Serialize)]
struct TearDrop {
    label: String,
    target: Option<String>,
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
        loop {
            if let (Ok(c), Some(w)) = (app.cursor_position(), app.get_webview_window(&label)) {
                let _ = w.set_position(PhysicalPosition::new(c.x - dx, c.y - dy));
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
        let _ = app.emit_to(source.as_str(), "tear-dropped", TearDrop { label, target: hover });
    });
}

#[derive(serde::Serialize)]
struct Entry {
    name: String,
    is_dir: bool,
}

#[tauri::command]
fn list_dir(path: String) -> Res<Vec<Entry>> {
    let mut out: Vec<Entry> = std::fs::read_dir(path)
        .map_err(err)?
        .filter_map(|e| e.ok())
        .map(|e| Entry {
            name: e.file_name().to_string_lossy().into_owned(),
            is_dir: e.file_type().map(|t| t.is_dir()).unwrap_or(false),
        })
        .collect();
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(out)
}

#[tauri::command]
fn read_file(path: String) -> Res<String> {
    std::fs::read_to_string(path).map_err(err)
}

#[tauri::command]
fn write_file(path: String, content: String) -> Res<()> {
    std::fs::write(path, content).map_err(err)
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
fn move_path(from: String, to: String) -> Res<()> {
    // fs::rename silently replaces an existing file on Unix.
    // ponytail: exists-check then rename is racy; fine for a single-user editor.
    if std::path::Path::new(&to).exists() {
        return Err(format!("{to} already exists"));
    }
    std::fs::rename(from, to).map_err(err)
}

/// Copy a file or folder into `dir`, Finder-style: "a.txt", then "a copy.txt", "a copy 2.txt"... Returns the new path.
#[tauri::command]
fn copy_path(from: String, dir: String) -> Res<String> {
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
    {
        use objc2::ClassType;
        use objc2_app_kit::{NSPasteboard, NSPasteboardURLReadingFileURLsOnlyKey};
        use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSURL};
        let pb = NSPasteboard::generalPasteboard();
        let classes = NSArray::from_slice(&[NSURL::class()]);
        let yes = NSNumber::new_bool(true);
        let opts = NSDictionary::from_slices(&[unsafe { NSPasteboardURLReadingFileURLsOnlyKey }], &[&*yes as &objc2::runtime::AnyObject]);
        let Some(objs) = (unsafe { pb.readObjectsForClasses_options(&classes, Some(&opts)) }) else { return vec![] };
        objs.iter()
            .filter_map(|o| o.downcast::<NSURL>().ok())
            .filter_map(|u| u.path().map(|p| p.to_string()))
            .collect()
    }
    #[cfg(not(target_os = "macos"))]
    CLIP.lock().unwrap().clone()
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
fn trash_path(app: AppHandle, path: String) -> Res<Option<String>> {
    match hold(Path::new(&path), &holding_dir(&app)?) {
        Ok(p) => Ok(Some(p.to_string_lossy().into_owned())),
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
            to_system_trash(Path::new(&path)).map_err(err)?;
            Ok(None)
        }
        Err(e) => Err(err(e)),
    }
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        // Default macOS menu binds ⌘W to Close Window; we want it for tabs.
        .menu(|h| {
            use tauri::menu::{Menu, PredefinedMenuItem as P, Submenu};
            Menu::with_items(h, &[
                &Submenu::with_items(h, "Fleet Command", true, &[
                    &P::about(h, None, None)?, &P::separator(h)?, &P::hide(h, None)?, &P::quit(h, None)?,
                ])?,
                &Submenu::with_items(h, "Edit", true, &[
                    &P::undo(h, None)?, &P::redo(h, None)?, &P::separator(h)?,
                    &P::cut(h, None)?, &P::copy(h, None)?, &P::paste(h, None)?, &P::select_all(h, None)?,
                ])?,
                &Submenu::with_items(h, "Window", true, &[&P::minimize(h, None)?, &P::fullscreen(h, None)?])?,
            ])
        })
        .setup(|app| {
            let h = app.handle().clone();
            std::thread::spawn(move || flush_holding(&h));
            Ok(())
        })
        .manage(Ptys::default())
        .manage(Awake::default())
        .invoke_handler(tauri::generate_handler![
            follow_cursor, mouse_down, pty_open, pty_write, pty_resize, pty_kill, pty_status, list_dir, read_file, write_file, create_file, create_dir, move_path, copy_path, clipboard_set_file, clipboard_files, reveal_path, trash_path, git, stay_awake, usage::usage_stats, usage::usage_limits, skills::skills_scan, skills::skill_create, skills::skill_link, skills::skill_delete, agent_config::agent_configs, agent_config::agent_config_save, agent_config::agent_config_set
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    #[test]
    fn recognizes_agents_by_program_or_script() {
        use super::agent_in;
        assert_eq!(agent_in("claude --model opus"), Some("claude"));
        assert_eq!(agent_in("/opt/lib/node_modules/@anthropic-ai/claude-code/bin/claude.exe"), Some("claude"));
        assert_eq!(agent_in("node /Users/me/.bun/bin/codex resume"), Some("codex"));
        assert_eq!(agent_in("node --no-warnings /x/opencode.js"), Some("opencode"));
        assert_eq!(agent_in("/Users/me/.grok/bin/grok-1.0.50"), Some("grok"));
        assert_eq!(agent_in("agy"), Some("agy"));
        assert_eq!(agent_in("/bin/sh ./codex --flag"), Some("codex"));
        assert_eq!(agent_in("vim claude.md"), None);
        assert_eq!(agent_in("node server.js"), None);
        assert_eq!(agent_in("claude-monitor"), None);
        assert_eq!(agent_in("-zsh"), None);
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
    fn copy_picks_free_names_and_recurses() {
        let base = std::env::temp_dir().join(format!("fc-copy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("src/sub")).unwrap();
        std::fs::write(base.join("src/sub/a.txt"), "hi").unwrap();
        let dir = base.join("src").to_string_lossy().into_owned();
        let file = base.join("src/sub/a.txt").to_string_lossy().into_owned();
        let sub = base.join("src/sub").to_string_lossy().into_owned();
        let sub_dir = sub.clone();
        assert!(super::copy_path(file.clone(), sub_dir.clone()).unwrap().ends_with("a copy.txt"));
        assert!(super::copy_path(file, sub_dir).unwrap().ends_with("a copy 2.txt"));
        let copied = super::copy_path(sub, dir).unwrap();
        assert!(copied.ends_with("sub copy"));
        assert_eq!(std::fs::read_to_string(format!("{copied}/a copy 2.txt")).unwrap(), "hi");
        std::fs::remove_dir_all(&base).unwrap();
    }
}

