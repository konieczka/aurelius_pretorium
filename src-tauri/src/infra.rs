//! Local infra for the sidebar's Servers mode: what's listening on TCP ports (dev servers, databases), and Docker's
//! containers. Ports come from `lsof`, processes from `ps`, containers from the `docker` CLI (it follows the user's
//! active context: Docker Desktop, OrbStack, Colima, and negotiates the API version).

use serde::Serialize;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;
use tauri::State;

use crate::{blocking, clean_env, cwd_of, login_path, usage::output_within, Ptys, Res};

#[derive(Serialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Project, // runs in a project folder: a dev server, a project's own database
    Service, // a Homebrew (or MacPorts) service: redis, postgres
    System,  // the OS's and apps' own (AirPlay, Postman, Ollama...): hidden unless asked for
}

#[derive(Serialize)]
pub struct Listener {
    pid: u32,
    ports: Vec<u16>,
    name: String,
    args: String,
    cwd: String,
    root: String, // the checkout it runs in, else its folder ("" for system ones)
    kind: Kind,
    tab: Option<String>, // the Pretorium terminal it was started from
}

#[derive(Serialize, PartialEq, Debug)]
struct Port {
    host: u16,
    inner: u16, // the container's own port
}

#[derive(Serialize)]
pub struct Container {
    id: String,
    name: String,
    image: String,
    state: String, // running, exited, paused, restarting, created...
    status: String, // "Up 3 hours (healthy)"
    ports: Vec<Port>,
    project: String, // Compose project ("" for a plain `docker run`)
    service: String,
    root: String, // the Compose project's checkout
}

#[derive(Serialize)]
pub struct Scan {
    listeners: Vec<Listener>,
    containers: Vec<Container>,
    docker: &'static str, // ok, down (the daemon isn't running), missing (no docker CLI)
}

/// Everything listening on TCP and every container, as of now.
#[tauri::command]
pub async fn infra_scan(ptys: State<'_, Ptys>) -> Res<Scan> {
    let shells: HashMap<u32, String> = ptys.sessions.lock().unwrap().iter().filter_map(|(id, s)| Some((s.child.process_id()?, id.clone()))).collect();
    blocking(move || {
        let (containers, docker) = containers();
        Ok(Scan { listeners: listeners(&shells), containers, docker })
    })
    .await
}

fn listeners(shells: &HashMap<u32, String>) -> Vec<Listener> {
    let mut lsof = Command::new("/usr/sbin/lsof");
    lsof.args(["-nP", "-iTCP", "-sTCP:LISTEN", "-F", "pn"]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    let Some(out) = output_within(lsof, Duration::from_secs(3)) else { return Vec::new() };
    let ports = parse_lsof(&String::from_utf8_lossy(&out.stdout));
    let ps = Command::new("/bin/ps").args(["-A", "-ww", "-o", "pid=,ppid=,args="]).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    let procs = parse_ps(&ps);
    let home = std::env::var("HOME").unwrap_or_default();
    let mut list: Vec<Listener> = ports
        .into_iter()
        .filter_map(|(pid, ports)| {
            let args = procs.get(&pid).map(|p| p.1.clone()).unwrap_or_default();
            let exe = exe_of(pid).unwrap_or_else(|| args.split_whitespace().next().unwrap_or("").to_string());
            if exe.contains("/Docker.app/") {
                return None; // the ports Docker publishes: its containers stand for them
            }
            let cwd = cwd_of(pid).unwrap_or_default();
            let (kind, root) = classify(&exe, &cwd, &home);
            let mut tab = None;
            let mut at = pid;
            for _ in 0..64 {
                if let Some(id) = shells.get(&at) {
                    tab = Some(id.clone());
                    break;
                }
                match procs.get(&at) {
                    Some(&(parent, _)) if parent > 1 => at = parent,
                    _ => break,
                }
            }
            let mut name = program(if args.is_empty() { &exe } else { &args });
            // `node dist/index.js`, `bun run dev`: the folder says more (apps/api is "api").
            if ["index", "main", "server", "app", "cli", "start", "dev", "serve"].contains(&name.as_str()) && cwd.len() > 1 {
                name = cwd.rsplit('/').next().unwrap_or(&name).to_string();
            }
            Some(Listener { pid, ports: ports.into_iter().collect(), name, args, cwd, root, kind, tab })
        })
        .collect();
    list.sort_by_key(|l| l.ports.first().copied());
    list
}

/// Ports by pid, from `lsof -F pn`: a `p<pid>` line, then an `n<address>:<port>` line per socket.
fn parse_lsof(out: &str) -> HashMap<u32, BTreeSet<u16>> {
    let mut ports: HashMap<u32, BTreeSet<u16>> = HashMap::new();
    let mut pid = 0;
    for line in out.lines() {
        if let Some(p) = line.strip_prefix('p') {
            pid = p.parse().unwrap_or(0);
        } else if let Some(port) = line.strip_prefix('n').and_then(|n| n.rsplit(':').next()).and_then(|p| p.parse().ok()) {
            if pid > 0 {
                ports.entry(pid).or_default().insert(port); // IPv4 and IPv6 sockets on one port are one entry
            }
        }
    }
    ports
}

/// Parent pid and command line by pid, from `ps -o pid=,ppid=,args=`.
fn parse_ps(out: &str) -> HashMap<u32, (u32, String)> {
    out.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let pid = words.next()?.parse().ok()?;
            let ppid = words.next()?.parse().ok()?;
            Some((pid, (ppid, words.collect::<Vec<_>>().join(" "))))
        })
        .collect()
}

/// What a listener is, and the folder it belongs to.
fn classify(exe: &str, cwd: &str, home: &str) -> (Kind, String) {
    let system = ["/System/", "/usr/libexec/", "/usr/sbin/", "/Library/"].iter().any(|p| exe.starts_with(p)) || exe.contains(".app/");
    // Not ours to read (another user's), or started by launchd in `/`: apps and daemons, not something run in a project.
    if system || cwd.is_empty() || cwd == "/" {
        return (Kind::System, String::new());
    }
    // Homebrew's prefix is itself a checkout: a service running in its var/ isn't a project.
    let brew = |p: &str| ["/opt/homebrew/", "/usr/local/", "/opt/local/"].iter().any(|b| p.starts_with(b));
    if brew(cwd) {
        return (Kind::Service, String::new());
    }
    if let Some(root) = checkout(cwd) {
        return (Kind::Project, root); // a Homebrew postgres a project runs on its own data is the project's
    }
    if brew(exe) {
        return (Kind::Service, String::new());
    }
    if !home.is_empty() && cwd.starts_with(&format!("{home}/")) && !cwd.starts_with(&format!("{home}/Library/")) {
        return (Kind::Project, cwd.to_string());
    }
    (Kind::System, String::new())
}

/// The checkout (or worktree) `dir` is in.
fn checkout(dir: &str) -> Option<String> {
    Path::new(dir).ancestors().find(|d| d.join(".git").exists()).map(|d| d.to_string_lossy().into_owned())
}

/// What a command line runs, by name: its program, or the script an interpreter runs
/// (`node .../node_modules/vite/bin/vite.js --host` is vite, `python -m uvicorn app:app` is uvicorn).
fn program(args: &str) -> String {
    // ponytail: splits on whitespace, so a path with spaces gets cut short.
    let base = |w: &str| {
        let b = w.rsplit('/').next().unwrap_or(w);
        let stem = b.rsplit_once('.').filter(|(_, ext)| ["js", "mjs", "cjs", "ts", "mts", "py", "rb"].contains(ext)).map_or(b, |(s, _)| s);
        stem.to_string()
    };
    let mut words = args.split_whitespace();
    let Some(first) = words.next() else { return String::new() };
    let prog = base(first);
    let interpreters = ["node", "bun", "deno", "tsx", "ts-node", "python", "python3", "ruby", "sh", "bash", "zsh"];
    if !interpreters.contains(&prog.as_str()) && !prog.starts_with("python3.") {
        return prog;
    }
    let takes_value = ["-r", "--require", "--import", "--loader", "--experimental-loader", "--env-file", "-c"];
    let mut skip = false;
    while let Some(w) = words.next() {
        if skip {
            skip = false;
        } else if w == "-m" {
            return words.next().map(base).unwrap_or(prog); // python -m <module>
        } else if takes_value.contains(&w) {
            skip = true;
        } else if w == "run" {
            continue; // bun run <script>, deno run <file>
        } else if !w.starts_with('-') {
            return base(w);
        }
    }
    prog
}

/// The program a process runs, as a path.
#[cfg(target_os = "macos")]
fn exe_of(pid: u32) -> Option<String> {
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let n = unsafe { libc::proc_pidpath(pid as i32, buf.as_mut_ptr() as *mut libc::c_void, buf.len() as u32) };
    (n > 0).then(|| String::from_utf8_lossy(&buf[..n as usize]).into_owned())
}
#[cfg(not(target_os = "macos"))]
fn exe_of(pid: u32) -> Option<String> {
    std::fs::read_link(format!("/proc/{pid}/exe")).ok().map(|p| p.to_string_lossy().into_owned())
}

/// `docker` with the user's PATH, or None when there's no docker CLI.
fn docker() -> Option<Command> {
    let path = login_path();
    let bin = std::env::split_paths(path).map(|d| d.join("docker")).find(|p| p.is_file())?;
    let mut cmd = Command::new(bin);
    cmd.env_clear().envs(clean_env()).env("PATH", path).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    Some(cmd)
}

fn containers() -> (Vec<Container>, &'static str) {
    let Some(mut cmd) = docker() else { return (Vec::new(), "missing") };
    // Tab-separated fields rather than `{{json .}}`: its Labels are one comma-joined string, and label values have commas.
    let fields = ["ID", "Names", "Image", "State", "Status", "Ports", "Label \"com.docker.compose.project\"", "Label \"com.docker.compose.service\"", "Label \"com.docker.compose.project.working_dir\""];
    let format = fields.map(|f| format!("{{{{.{f}}}}}")).join("\t");
    cmd.args(["ps", "-a", "--no-trunc", "--format", &format]);
    // A daemon that's starting up (or wedged) mustn't hold up the panel.
    match output_within(cmd, Duration::from_secs(3)) {
        Some(out) if out.status.success() => (String::from_utf8_lossy(&out.stdout).lines().filter_map(parse_container).collect(), "ok"),
        _ => (Vec::new(), "down"),
    }
}

fn parse_container(line: &str) -> Option<Container> {
    let f: Vec<&str> = line.split('\t').collect();
    let [id, name, image, state, status, ports, project, service, dir] = f[..] else { return None };
    Some(Container {
        id: id.into(),
        name: name.into(),
        image: image.into(),
        state: state.into(),
        status: status.into(),
        ports: parse_ports(ports),
        project: project.into(),
        service: service.into(),
        root: if dir.is_empty() { String::new() } else { checkout(dir).unwrap_or_else(|| dir.into()) },
    })
}

/// Published TCP ports from `docker ps`'s Ports column: `0.0.0.0:5433->5432/tcp, [::]:5433->5432/tcp, 6379/tcp`
/// (the last isn't published: nothing on this machine reaches it). Ranges are `0.0.0.0:8000-8001->8000-8001/tcp`.
fn parse_ports(s: &str) -> Vec<Port> {
    let mut out: Vec<Port> = Vec::new();
    for p in s.split(", ") {
        let Some((host, inner)) = p.strip_suffix("/tcp").and_then(|p| p.split_once("->")) else { continue };
        let host = host.rsplit(':').next().unwrap_or("");
        let range = |r: &str| -> Option<(u16, u16)> {
            let (a, b) = r.split_once('-').unwrap_or((r, r));
            Some((a.parse().ok()?, b.parse().ok()?))
        };
        let (Some((h0, h1)), Some((i0, _))) = (range(host), range(inner)) else { continue };
        for (k, h) in (h0..=h1).take(32).enumerate() {
            let port = Port { host: h, inner: i0 + k as u16 };
            if !out.contains(&port) {
                out.push(port);
            }
        }
    }
    out
}

#[derive(Serialize, Debug)]
pub struct Recipe {
    dir: String, // where it runs
    name: String,
    command: String,
    kind: &'static str, // script (package.json), compose, just, make
}

/// package.json scripts that start a dev server, best first.
const SCRIPTS: [&str; 6] = ["dev", "develop", "start", "serve", "preview", "storybook"];
/// justfile recipes and Makefile targets that do.
const TARGETS: [&str; 6] = ["dev", "develop", "serve", "start", "run", "up"];

/// Ways to start the folder's dev servers: its package.json scripts and its workspace packages', its Compose file,
/// its justfile recipes and Makefile targets.
#[tauri::command]
pub async fn infra_recipes(dir: String) -> Res<Vec<Recipe>> {
    blocking(move || Ok(recipes(Path::new(&dir)))).await
}

fn recipes(root: &Path) -> Vec<Recipe> {
    let mut out = Vec::new();
    let pkg = read_json(&root.join("package.json"));
    let pm = package_manager(root, pkg.as_ref());
    if let Some(p) = &pkg {
        scripts(root, p, pm, &mut out);
    }
    for dir in packages(root, pkg.as_ref()) {
        if let Some(p) = read_json(&dir.join("package.json")) {
            scripts(&dir, &p, pm, &mut out);
        }
    }
    let at = |name: &str, command: String, kind| Recipe { dir: root.to_string_lossy().into_owned(), name: name.into(), command, kind };
    if ["compose.yaml", "compose.yml", "docker-compose.yml", "docker-compose.yaml"].iter().any(|f| root.join(f).is_file()) {
        out.push(at("compose up", "docker compose up".into(), "compose"));
    }
    // The first of each tool's files only: on a case-insensitive disk justfile and Justfile are one.
    for (tool, files) in [("just", ["justfile", ".justfile", "Justfile"]), ("make", ["Makefile", "makefile", "GNUmakefile"])] {
        let Some(text) = files.iter().find_map(|f| std::fs::read_to_string(root.join(f)).ok()) else { continue };
        for t in targets(&text) {
            out.push(at(&format!("{tool} {t}"), format!("{tool} {t}"), tool));
        }
    }
    out
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// What runs the folder's scripts: told by its lockfile (in it or a folder above, up to the checkout), else by
/// package.json's packageManager.
fn package_manager(root: &Path, pkg: Option<&serde_json::Value>) -> &'static str {
    let locks = [("bun.lock", "bun"), ("bun.lockb", "bun"), ("pnpm-lock.yaml", "pnpm"), ("yarn.lock", "yarn"), ("package-lock.json", "npm")];
    for dir in root.ancestors() {
        if let Some((_, pm)) = locks.iter().find(|(f, _)| dir.join(f).is_file()) {
            return pm;
        }
        if dir.join(".git").exists() {
            break;
        }
    }
    let named = pkg.and_then(|p| p["packageManager"].as_str()).unwrap_or("");
    ["bun", "pnpm", "yarn"].into_iter().find(|pm| named.starts_with(&format!("{pm}@"))).unwrap_or("npm")
}

fn scripts(dir: &Path, pkg: &serde_json::Value, pm: &str, out: &mut Vec<Recipe>) {
    let Some(s) = pkg["scripts"].as_object() else { return };
    let at = |name: &str, command: String| Recipe { dir: dir.to_string_lossy().into_owned(), name: name.into(), command, kind: "script" };
    // A Tauri app's dev server is `tauri dev`, through its `tauri` script.
    if s.get("tauri").and_then(|v| v.as_str()).is_some_and(|v| v.trim() == "tauri") {
        out.push(at("tauri dev", format!("{pm} run tauri dev")));
    }
    let mut names: Vec<&String> = s.keys().filter(|k| SCRIPTS.contains(&k.as_str()) || k.starts_with("dev:")).filter(|k| plain(k)).collect();
    names.sort_by_key(|k| SCRIPTS.iter().position(|s| s == k).unwrap_or(SCRIPTS.len()));
    for n in names {
        out.push(at(n, format!("{pm} run {n}")));
    }
}

/// Safe to put in a command line as it is.
fn plain(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || ":-_.".contains(c))
}

/// The workspace packages' folders: package.json's `workspaces` and pnpm-workspace.yaml's `packages` globs.
fn packages(root: &Path, pkg: Option<&serde_json::Value>) -> Vec<std::path::PathBuf> {
    let mut globs: Vec<String> = Vec::new();
    if let Some(w) = pkg.map(|p| &p["workspaces"]) {
        let list = w.as_array().or_else(|| w["packages"].as_array());
        globs.extend(list.into_iter().flatten().filter_map(|g| g.as_str().map(String::from)));
    }
    if let Ok(yaml) = std::fs::read_to_string(root.join("pnpm-workspace.yaml")) {
        // ponytail: reads the block list under `packages:`, not YAML at large (a flow list `[a, b]` is missed).
        let mut inside = false;
        for line in yaml.lines() {
            let t = line.trim();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            if !line.starts_with([' ', '\t', '-']) {
                inside = t == "packages:";
            } else if let Some(g) = t.strip_prefix('-').filter(|_| inside) {
                globs.push(g.trim().trim_matches(['"', '\'']).to_string());
            }
        }
    }
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    for g in globs.iter().filter(|g| !g.starts_with('!')) {
        let mut at = vec![root.to_path_buf()];
        for seg in g.trim_start_matches("./").split('/').filter(|s| !s.is_empty()) {
            at = if seg.contains('*') {
                // ponytail: `**` matches one level, like `*`
                at.iter()
                    .flat_map(|d| std::fs::read_dir(d).into_iter().flatten().flatten())
                    .filter(|e| e.path().is_dir())
                    .filter(|e| e.file_name().to_str().is_some_and(|n| n != "node_modules" && !n.starts_with('.') && wild(seg, n)))
                    .map(|e| e.path())
                    .collect()
            } else {
                at.into_iter().map(|d| d.join(seg)).filter(|d| d.is_dir()).collect()
            };
        }
        dirs.extend(at.into_iter().filter(|d| d.join("package.json").is_file()));
    }
    dirs.sort();
    dirs.dedup();
    dirs.truncate(64);
    dirs
}

/// Does `name` match a glob segment with one `*` (`*`, `app-*`, `*-web`)?
fn wild(seg: &str, name: &str) -> bool {
    let (pre, post) = seg.split_once('*').unwrap_or((seg, ""));
    let post = post.trim_start_matches('*');
    name.len() >= pre.len() + post.len() && name.starts_with(pre) && name.ends_with(post)
}

/// justfile recipes and Makefile targets that start something (`dev:`, `serve port="3000":`, `@up:`).
fn targets(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        if line.starts_with([' ', '\t', '#', '.']) {
            continue;
        }
        let Some((head, rest)) = line.split_once(':') else { continue };
        let words: Vec<&str> = head.split_whitespace().collect();
        let assigns = |w: &&str| w.ends_with('=') && w.len() <= 3; // =, :=, ?=, +=, !=
        if rest.starts_with('=') || words.first().is_none_or(|w| w.contains('=')) || words.get(1).is_some_and(assigns) {
            continue; // a variable (`x := 1`, `X ::= 1`, `X = a:b`); not a just recipe's parameter (`serve port="3000":`)
        }
        // A Makefile line may name several targets; a just recipe is one name, then its parameters.
        for name in words.iter().map(|w| w.trim_start_matches('@')).filter(|w| !w.contains('=')) {
            if TARGETS.contains(&name) && !out.iter().any(|o| o == name) {
                out.push(name.into());
            }
        }
    }
    out
}

/// Stop a listener: SIGTERM to its process group (the whole job a terminal runs, the way ^C ends it), or to the
/// process alone when its group is a Pretorium shell's or the app's own.
#[tauri::command]
pub async fn infra_stop(ptys: State<'_, Ptys>, pid: u32) -> Res<()> {
    let shells: Vec<i32> = ptys.sessions.lock().unwrap().values().filter_map(|s| s.child.process_id()).map(|p| p as i32).collect();
    let home = std::env::var("HOME").unwrap_or_default();
    let exe = exe_of(pid).unwrap_or_default();
    // Only what the panel offers to stop: never the system's, nor something we can't even look at.
    if pid <= 1 || classify(&exe, &cwd_of(pid).unwrap_or_default(), &home).0 == Kind::System {
        return Err("Not a server Pretorium can stop".into());
    }
    #[cfg(unix)]
    unsafe {
        let group = libc::getpgid(pid as i32);
        let ok = if group > 1 && group != libc::getpgrp() && !shells.contains(&group) { libc::killpg(group, libc::SIGTERM) } else { libc::kill(pid as i32, libc::SIGTERM) };
        if ok != 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
    }
    Ok(())
}

/// Start, stop or restart a container.
#[tauri::command]
pub async fn infra_docker(action: String, id: String) -> Res<()> {
    if !["start", "stop", "restart"].contains(&action.as_str()) || id.is_empty() || id.starts_with('-') {
        return Err(format!("Can't {action} {id}"));
    }
    blocking(move || {
        let mut cmd = docker().ok_or("Docker isn't installed")?;
        cmd.args([&action, &id]);
        let out = output_within(cmd, Duration::from_secs(30)).ok_or("Docker didn't answer")?;
        if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_listening_ports_by_process() {
        let out = "p713\nf9\nn*:7000\nf10\nn*:7000\nf11\nn*:5000\np2108\nf6\nn127.0.0.1:6379\nf7\nn[::1]:6379\n";
        let ports = parse_lsof(out);
        assert_eq!(ports[&713].iter().copied().collect::<Vec<_>>(), [5000, 7000]);
        assert_eq!(ports[&2108].iter().copied().collect::<Vec<_>>(), [6379]);
    }

    #[test]
    fn names_servers_by_what_they_run() {
        assert_eq!(program("node /p/web/node_modules/.bin/../../node_modules/.pnpm/vite@7/node_modules/vite/bin/vite.js --host 127.0.0.1"), "vite");
        assert_eq!(program("node /p/musashi/node_modules/.bin/expo start"), "expo");
        assert_eq!(program("/opt/homebrew/opt/redis/bin/redis-server 127.0.0.1:6379"), "redis-server");
        assert_eq!(program("node --require /p/api/.pnp.cjs /p/api/dist/main.js"), "main");
        assert_eq!(program("python3 -m uvicorn app:app --reload"), "uvicorn");
        assert_eq!(program("bun run dev"), "dev");
    }

    #[test]
    fn tells_project_servers_from_services_and_the_system() {
        let home = "/Users/me";
        assert_eq!(classify("/System/Library/CoreServices/ControlCenter.app/Contents/MacOS/ControlCenter", "/", home).0, Kind::System);
        assert_eq!(classify("/Applications/Ollama.app/Contents/Resources/ollama", "/", home).0, Kind::System);
        assert_eq!(classify("/opt/homebrew/Cellar/redis/7/bin/redis-server", "/opt/homebrew/var/db/redis", home).0, Kind::Service);
        assert_eq!(classify("/Users/me/.nvm/node", "/Users/me/scratch/site", home), (Kind::Project, "/Users/me/scratch/site".into()));
        assert_eq!(classify("/Users/me/.nvm/node", "/Users/me/Library/Caches/x", home).0, Kind::System);
        let repo = std::fs::canonicalize(env!("CARGO_MANIFEST_DIR")).unwrap();
        let sub = repo.join("src");
        let (kind, root) = classify("/opt/homebrew/bin/node", sub.to_str().unwrap(), home);
        assert_eq!(kind, Kind::Project);
        assert!(sub.starts_with(&root) && Path::new(&root).join(".git").exists());
    }

    #[test]
    fn reads_published_container_ports() {
        let p = |h, i| Port { host: h, inner: i };
        assert_eq!(parse_ports("0.0.0.0:5433->5432/tcp, [::]:5433->5432/tcp"), [p(5433, 5432)]);
        assert_eq!(parse_ports("127.0.0.1:58000->8000/tcp, 5432/tcp, 0.0.0.0:53->53/udp"), [p(58000, 8000)]);
        assert_eq!(parse_ports("0.0.0.0:8000-8001->9000-9001/tcp"), [p(8000, 9000), p(8001, 9001)]);
        assert_eq!(parse_ports(""), []);
    }

    #[test]
    fn finds_ways_to_start_dev_servers() {
        let root = std::env::temp_dir().join(format!("pretorium-recipes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for d in ["apps/web", "apps/docs", "packages/ui", "tools"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        let write = |p: &str, s: &str| std::fs::write(root.join(p), s).unwrap();
        write("package.json", r#"{"scripts": {"build": "x", "preview": "vite preview", "dev": "turbo dev", "tauri": "tauri", "dev:docs": "x", "start": "a b; rm"}}"#);
        write("pnpm-lock.yaml", "");
        write("pnpm-workspace.yaml", "packages:\n  - 'apps/*'\n  - \"packages/*\"\n  - '!apps/docs'\nother:\n  - tools\n");
        write("apps/web/package.json", r#"{"scripts": {"dev": "vite"}}"#);
        write("packages/ui/package.json", r#"{"scripts": {"build": "tsc"}}"#);
        write("tools/package.json", r#"{"scripts": {"dev": "x"}}"#);
        write("compose.yaml", "services: {}\n");
        write("Makefile", "CC := cc\nX = a:b\n.PHONY: dev\ndev serve: deps\n\tnpm run dev\nbuild:\n\tcc\n");
        let r = recipes(&root);
        let names: Vec<(String, &str)> = r.iter().map(|r| (r.dir.strip_prefix(root.to_str().unwrap()).unwrap().to_string(), r.command.as_str())).collect();
        let at = |d: &str, c| (d.to_string(), c);
        assert_eq!(names, [
            at("", "pnpm run tauri dev"), at("", "pnpm run dev"), at("", "pnpm run start"), at("", "pnpm run preview"), at("", "pnpm run dev:docs"),
            at("/apps/web", "pnpm run dev"), // apps/docs has no package.json; tools isn't a workspace package
            at("", "docker compose up"), at("", "make dev"), at("", "make serve"),
        ]);
        assert_eq!(targets("set dotenv-load\nx := '1'\n@dev port=\"3000\":\n  vite\nup: dev\n"), ["dev", "up"]);
        assert!(wild("app-*", "app-web") && wild("*-web", "app-web") && !wild("app-*", "web"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn reads_compose_containers() {
        let c = parse_container("abc\tapp-db-1\tpostgres:17\trunning\tUp 3 hours\t0.0.0.0:5433->5432/tcp\tapp\tdb\t/nowhere/app").unwrap();
        assert_eq!((c.name.as_str(), c.project.as_str(), c.service.as_str(), c.root.as_str()), ("app-db-1", "app", "db", "/nowhere/app"));
        assert!(parse_container("too\tfew").is_none());
    }
}
