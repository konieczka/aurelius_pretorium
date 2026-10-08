use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use tauri::ipc::{Channel, InvokeResponseBody};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, State};

struct Session {
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
}

#[derive(Default)]
struct Ptys {
    next: AtomicU32,
    sessions: Mutex<HashMap<u32, Session>>,
}

type Res<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command]
fn pty_spawn(
    ptys: State<Ptys>,
    cwd: String,
    cols: u16,
    rows: u16,
    on_data: Channel<InvokeResponseBody>,
) -> Res<u32> {
    let pair = native_pty_system()
        .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .map_err(err)?;
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let mut cmd = CommandBuilder::new(shell);
    // Start from a clean env like Terminal.app: the app's own env (e.g. npm_*/bun vars and
    // node_modules/.bin from `bun run tauri dev`) must not leak into user shells. The login
    // shell (-l) then rebuilds PATH etc. from the user's profile.
    cmd.env_clear();
    for key in ["HOME", "USER", "LOGNAME", "SHELL", "TMPDIR", "SSH_AUTH_SOCK", "__CF_USER_TEXT_ENCODING"] {
        if let Ok(v) = std::env::var(key) {
            cmd.env(key, v);
        }
    }
    cmd.env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
    // Finder-launched apps get no LANG; without it shells mangle UTF-8.
    cmd.env("LANG", std::env::var("LANG").unwrap_or_else(|_| "en_US.UTF-8".into()));
    cmd.arg("-l");
    cmd.cwd(cwd);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    let child = pair.slave.spawn_command(cmd).map_err(err)?;
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().map_err(err)?;
    let writer = pair.master.take_writer().map_err(err)?;
    std::thread::spawn(move || {
        let mut buf = [0u8; 16 * 1024];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || on_data.send(InvokeResponseBody::Raw(buf[..n].to_vec())).is_err() {
                break;
            }
        }
        // Empty message = shell exited.
        let _ = on_data.send(InvokeResponseBody::Raw(Vec::new()));
    });

    let id = ptys.next.fetch_add(1, Ordering::Relaxed);
    ptys.sessions
        .lock()
        .unwrap()
        .insert(id, Session { writer, master: pair.master, child });
    Ok(id)
}

#[tauri::command]
fn pty_write(ptys: State<Ptys>, id: u32, data: String) -> Res<()> {
    let mut map = ptys.sessions.lock().unwrap();
    let s = map.get_mut(&id).ok_or("no such pty")?;
    s.writer.write_all(data.as_bytes()).map_err(err)
}

#[tauri::command]
fn pty_resize(ptys: State<Ptys>, id: u32, cols: u16, rows: u16) -> Res<()> {
    let map = ptys.sessions.lock().unwrap();
    let s = map.get(&id).ok_or("no such pty")?;
    s.master
        .resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .map_err(err)
}

#[tauri::command]
fn pty_kill(ptys: State<Ptys>, id: u32) {
    if let Some(mut s) = ptys.sessions.lock().unwrap().remove(&id) {
        let _ = s.child.kill();
    }
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
        .invoke_handler(tauri::generate_handler![
            pty_spawn, pty_write, pty_resize, pty_kill, list_dir, read_file, write_file, create_file, create_dir, move_path, trash_path
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
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
}
