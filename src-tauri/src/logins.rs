//! Saved logins for browser tabs on this Mac's own servers (localhost and the like), remembered by project and port:
//! two projects that both serve :3000 keep their own. A script in a script world of its own (pages can't see it or its
//! message handler, see logins.js) asks for the site's logins when a password field shows up and fills one in, and
//! says what was typed when a login form is sent; the tab's toolbar then asks whether to save it. Kept in the Keychain.

use crate::Res;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, Url, Webview};

const KEYCHAIN: &str = "Pretorium sites";
/// The page's half.
const SCRIPT: &str = include_str!("logins.js");

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
pub struct Login {
    user: String,
    password: String,
}

/// What's remembered for a site: its logins (the latest saved first, the one filled in), and whether to stop asking.
#[derive(Default, Serialize, Deserialize, PartialEq, Debug)]
struct Site {
    #[serde(default)]
    logins: Vec<Login>,
    #[serde(default)]
    never: bool,
}

/// A project's site on a port. Project "" for a tab that has none.
#[derive(Clone, PartialEq, Debug)]
struct Key {
    project: String,
    port: u16,
}

impl Key {
    /// Its Keychain item's account.
    fn account(&self) -> String {
        format!("{}:{}", self.project, self.port)
    }
}

/// The port of an address on this Mac, else None: http(s) on loopback only, as a LAN address may be another machine's.
pub fn local_port(url: &Url) -> Option<u16> {
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?;
    let local = matches!(host, "localhost" | "0.0.0.0" | "[::1]") || host.ends_with(".localhost") || host.parse::<std::net::Ipv4Addr>().is_ok_and(|ip| ip.is_loopback());
    local.then(|| url.port_or_known_default()).flatten()
}

/// A frame's origin as WebKit has it (port 0: the scheme's default), as an address.
fn origin_url(scheme: &str, host: &str, port: isize) -> Option<Url> {
    let host = if host.contains(':') { format!("[{host}]") } else { host.to_string() };
    let port = if port > 0 { format!(":{port}") } else { String::new() };
    format!("{scheme}://{host}{port}/").parse().ok()
}

/// Tab id -> its project ("" none), from when its view was made.
static PROJECTS: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

pub fn set_project(id: &str, project: &str) {
    PROJECTS.lock().unwrap().get_or_insert_default().insert(id.into(), project.into());
}

fn key(id: &str, url: &Url) -> Option<Key> {
    let project = PROJECTS.lock().unwrap().get_or_insert_default().get(id).cloned().unwrap_or_default();
    Some(Key { project, port: local_port(url)? })
}

/// A login a page sent, while its tab's toolbar asks what to do with it.
struct Pending {
    key: Key,
    origin: String,
    login: Login,
}

/// Tab id -> what its toolbar is asking about.
static PENDING: Mutex<Option<HashMap<String, Pending>>> = Mutex::new(None);

/// The tab was closed.
pub fn forget_tab(id: &str) {
    PROJECTS.lock().unwrap().get_or_insert_default().remove(id);
    PENDING.lock().unwrap().get_or_insert_default().remove(id);
}

/// The E2E harness and tests keep logins out of the user's Keychain: in memory instead.
fn keychain_off() -> bool {
    std::env::var_os("PRETORIUM_NO_KEYCHAIN").is_some() || !cfg!(target_os = "macos")
}
static MEMORY: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

fn load(key: &Key) -> Site {
    let json = if keychain_off() {
        MEMORY.lock().unwrap().get_or_insert_default().get(&key.account()).cloned()
    } else {
        let out = std::process::Command::new("/usr/bin/security").args(["find-generic-password", "-s", KEYCHAIN, "-a", &key.account(), "-w"]).output();
        out.ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim_end().to_string())
    };
    json.and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default()
}

fn store(key: &Key, site: &Site) {
    let (account, gone) = (key.account(), site.logins.is_empty() && !site.never);
    if keychain_off() {
        let mut memory = MEMORY.lock().unwrap();
        let memory = memory.get_or_insert_default();
        if gone {
            memory.remove(&account);
        } else {
            memory.insert(account, ascii_json(site));
        }
        return;
    }
    let security = |args: &[&str]| drop(std::process::Command::new("/usr/bin/security").args(args).output());
    if gone {
        return security(&["delete-generic-password", "-s", KEYCHAIN, "-a", &account]);
    }
    // What Keychain Access lists it as: "localhost:3000 · shop"
    let name = key.project.rsplit('/').next().filter(|n| !n.is_empty()).unwrap_or("no project");
    let label = format!("localhost:{} · {name}", key.port);
    // ponytail: the secret is in `security`'s argv for a moment (only this user's processes can read it)
    security(&["add-generic-password", "-U", "-s", KEYCHAIN, "-a", &account, "-l", &label, "-w", &ascii_json(site)]);
}

/// JSON with everything past printable ASCII escaped: `security -w` prints a secret it finds unprintable as hex.
fn ascii_json(v: &impl Serialize) -> String {
    let mut out = String::new();
    for c in serde_json::to_string(v).unwrap_or_default().chars() {
        if c.is_ascii() && c != '\x7f' {
            out.push(c);
        } else {
            // Only ever inside a string: JSON's own syntax is ASCII
            for unit in c.encode_utf16(&mut [0; 2]) {
                out.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    out
}

/// What a sent form is to a site: Some(false) a new login to offer saving, Some(true) a saved user's new password, None
/// nothing to ask about (it's saved, or the site isn't to be asked about).
fn offer(site: &Site, login: &Login) -> Option<bool> {
    if login.password.is_empty() || site.logins.contains(login) {
        return None;
    }
    let update = site.logins.iter().any(|l| l.user == login.user);
    (update || !site.never).then_some(update)
}

/// `login` first: a new one, or a saved user's new password.
fn remember(logins: &mut Vec<Login>, login: Login) {
    logins.retain(|l| l.user != login.user);
    logins.insert(0, login);
}

/// A site's saved users, for the tab's toolbar.
#[derive(Clone, Serialize)]
struct Users {
    id: String,
    origin: String,
    users: Vec<String>,
}

/// A login a page sent, for the tab's toolbar to ask about.
#[derive(Clone, Serialize)]
struct Ask {
    id: String,
    origin: String,
    user: String,
    update: bool,
}

fn emit_users(app: &AppHandle, id: &str, origin: &str, site: &Site) {
    let users = site.logins.iter().map(|l| l.user.clone()).collect();
    let _ = app.emit("web-logins", Users { id: id.into(), origin: origin.into(), users });
}

#[derive(Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
enum Message {
    /// The page has a password field: its site's logins, please
    Logins,
    /// A login form was sent
    Sent { user: String, password: String },
}

/// A message from tab `id`'s page at `origin` (as WebKit has it, not as the page says). The reply, as JSON. Blocking.
fn handle(app: &AppHandle, id: &str, origin: &Url, body: &str) -> String {
    let Some(key) = key(id, origin) else { return "null".into() };
    let origin = origin.origin().ascii_serialization();
    match serde_json::from_str(body) {
        Ok(Message::Logins) => {
            let site = load(&key);
            emit_users(app, id, &origin, &site);
            serde_json::to_string(&site.logins).unwrap_or_default()
        }
        Ok(Message::Sent { user, password }) => {
            let login = Login { user: user.trim().into(), password };
            if let Some(update) = offer(&load(&key), &login) {
                let ask = Ask { id: id.into(), origin: origin.clone(), user: login.user.clone(), update };
                PENDING.lock().unwrap().get_or_insert_default().insert(id.into(), Pending { key, origin, login });
                let _ = app.emit("web-login-ask", ask);
            }
            "null".into()
        }
        Err(_) => "null".into(),
    }
}

/// The toolbar's answer about the login tab `id`'s page sent: "save" it, "never" ask about the site again, or anything
/// else to let it go.
#[tauri::command]
pub async fn web_login_answer(app: AppHandle, id: String, answer: String) -> Res<()> {
    let Some(Pending { key, origin, login }) = PENDING.lock().unwrap().get_or_insert_default().remove(&id) else { return Ok(()) };
    crate::blocking(move || {
        let mut site = load(&key);
        match answer.as_str() {
            "save" => remember(&mut site.logins, login),
            "never" => site.never = true,
            _ => return Ok(()),
        }
        store(&key, &site);
        emit_users(&app, &id, &origin, &site);
        Ok(())
    })
    .await
}

fn view(app: &AppHandle, id: &str) -> Res<(Webview, Key)> {
    let wv = app.get_webview(&format!("{}{id}", crate::web::PREFIX)).ok_or("no such browser tab")?;
    let key = wv.url().ok().and_then(|u| key(id, &u)).ok_or("logins are only kept for this Mac's own servers")?;
    Ok((wv, key))
}

/// Fill tab `id`'s login form with `user`'s saved login.
#[tauri::command]
pub async fn web_login_fill(app: AppHandle, id: String, user: String) -> Res<()> {
    let (wv, key) = view(&app, &id)?;
    let site = crate::blocking(move || Ok(load(&key))).await?;
    let login = site.logins.into_iter().find(|l| l.user == user).ok_or("no such login")?;
    let js = format!("window.__pretorium?.fill({})", serde_json::to_string(&login).unwrap_or_default());
    run(&wv, js);
    Ok(())
}

/// Forget `user`'s login for tab `id`'s site.
#[tauri::command]
pub async fn web_login_forget(app: AppHandle, id: String, user: String) -> Res<()> {
    let (wv, key) = view(&app, &id)?;
    let origin = wv.url().map(|u| u.origin().ascii_serialization()).unwrap_or_default();
    crate::blocking(move || {
        let mut site = load(&key);
        site.logins.retain(|l| l.user != user);
        store(&key, &site);
        emit_users(&app, &id, &origin, &site);
        Ok(())
    })
    .await
}

/// Run `js` in the script's world, on the tab's top frame.
fn run(wv: &Webview, js: String) {
    #[cfg(target_os = "macos")]
    let _ = wv.with_webview(move |v| bridge::run(v.inner(), &js));
    #[cfg(not(target_os = "macos"))]
    let _ = (wv, js);
}

/// A page finished loading: make sure it has the script. Its view runs it as each page loads, but the first page may
/// have started before the view had it. It's a no-op where it has run.
pub fn inject(wv: &Webview) {
    run(wv, SCRIPT.to_string());
}

#[cfg(target_os = "macos")]
pub use bridge::attach;

#[cfg(target_os = "macos")]
mod bridge {
    use block2::{DynBlock, RcBlock};
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
    use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
    use objc2_foundation::NSString;
    use objc2_web_kit::{WKContentWorld, WKScriptMessage, WKScriptMessageHandlerWithReply, WKUserContentController, WKUserScript, WKUserScriptInjectionTime, WKWebView};
    use std::ffi::c_void;
    use tauri::AppHandle;

    /// The script's world, and its message handler's name (window.webkit.messageHandlers.logins, there only).
    const WORLD: &str = "pretorium";
    const HANDLER: &str = "logins";

    pub struct Ivars {
        app: AppHandle,
        id: String,
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[ivars = Ivars]
        struct Handler;

        unsafe impl NSObjectProtocol for Handler {}

        unsafe impl WKScriptMessageHandlerWithReply for Handler {
            #[unsafe(method(userContentController:didReceiveScriptMessage:replyHandler:))]
            fn received(&self, _: &WKUserContentController, message: &WKScriptMessage, reply: &DynBlock<dyn Fn(*mut AnyObject, *mut NSString)>) {
                let (body, origin) = unsafe {
                    let body = message.body().downcast::<NSString>().map(|s| s.to_string()).unwrap_or_default();
                    let o = message.frameInfo().securityOrigin();
                    (body, super::origin_url(&o.protocol().to_string(), &o.host().to_string(), o.port()))
                };
                let reply = Reply(reply.copy());
                let Ivars { app, id } = self.ivars();
                let (app, id) = (app.clone(), id.clone());
                // The Keychain is a process away: off the main thread, answering back on it
                std::thread::spawn(move || {
                    let answer = origin.map_or_else(|| "null".into(), |o| super::handle(&app, &id, &o, &body));
                    let _ = app.run_on_main_thread(move || reply.send(&answer));
                });
            }
        }
    );

    /// A page's promise, waiting on an answer.
    struct Reply(RcBlock<dyn Fn(*mut AnyObject, *mut NSString)>);
    // Taken off the main thread only to be sent back to it, and called there.
    unsafe impl Send for Reply {}

    impl Reply {
        fn send(self, json: &str) {
            let s = NSString::from_str(json);
            self.0.call((Retained::as_ptr(&s) as *mut AnyObject, std::ptr::null_mut()));
        }
    }

    fn world(mtm: MainThreadMarker) -> Retained<WKContentWorld> {
        unsafe { WKContentWorld::worldWithName(&NSString::from_str(WORLD), mtm) }
    }

    /// Main thread: give tab `id`'s new view the script, as each page loads, and its message handler.
    pub fn attach(app: AppHandle, id: String, view: *mut c_void) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let view = unsafe { &*(view as *const WKWebView) };
        let world = world(mtm);
        let handler = mtm.alloc::<Handler>().set_ivars(Ivars { app, id });
        let handler: Retained<Handler> = unsafe { msg_send![super(handler), init] };
        unsafe {
            let controller = view.configuration().userContentController();
            controller.addScriptMessageHandlerWithReply_contentWorld_name(ProtocolObject::from_ref(&*handler), &world, &NSString::from_str(HANDLER));
            let source = NSString::from_str(super::SCRIPT);
            let script = WKUserScript::initWithSource_injectionTime_forMainFrameOnly_inContentWorld(mtm.alloc(), &source, WKUserScriptInjectionTime::AtDocumentEnd, true, &world);
            controller.addUserScript(&script);
        }
    }

    /// Main thread: run `js` in the script's world on the view's top frame.
    pub fn run(view: *mut c_void, js: &str) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let view = unsafe { &*(view as *const WKWebView) };
        unsafe { view.evaluateJavaScript_inFrame_inContentWorld_completionHandler(&NSString::from_str(js), None, &world(mtm), None) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn login(user: &str, password: &str) -> Login {
        Login { user: user.into(), password: password.into() }
    }

    #[test]
    fn only_this_macs_own_servers() {
        let port = |s: &str| local_port(&s.parse().unwrap());
        assert_eq!(port("http://localhost:3000/login"), Some(3000));
        assert_eq!(port("http://127.0.0.1:5173"), Some(5173));
        assert_eq!(port("http://[::1]:8080"), Some(8080));
        assert_eq!(port("http://app.localhost"), Some(80));
        assert_eq!(port("https://localhost"), Some(443));
        assert_eq!(port("http://192.168.1.4:3000"), None);
        assert_eq!(port("https://github.com"), None);
        assert_eq!(port("http://localhost.example.com:3000"), None);
        assert_eq!(port("file:///etc/hosts"), None);
        assert_eq!(origin_url("http", "::1", 3000).as_ref().and_then(local_port), Some(3000));
        assert_eq!(origin_url("http", "localhost", 0).as_ref().and_then(local_port), Some(80));
    }

    #[test]
    fn a_project_and_port_name_an_item() {
        assert_eq!(Key { project: "/Users/me/shop".into(), port: 3000 }.account(), "/Users/me/shop:3000");
        assert_eq!(Key { project: "".into(), port: 3000 }.account(), ":3000");
    }

    #[test]
    fn secrets_are_stored_as_printable_json() {
        let site = Site { logins: vec![login("zoë", "pässwörd 🔑\n\"x\"")], never: false };
        let json = ascii_json(&site);
        assert!(json.bytes().all(|b| (0x20..0x7f).contains(&b)), "{json}");
        assert_eq!(serde_json::from_str::<Site>(&json).unwrap(), site);
    }

    #[test]
    fn sent_forms_are_offered_unless_saved_or_never() {
        let site = Site { logins: vec![login("admin", "a")], never: false };
        assert_eq!(offer(&site, &login("admin", "a")), None);
        assert_eq!(offer(&site, &login("admin", "b")), Some(true));
        assert_eq!(offer(&site, &login("user", "u")), Some(false));
        assert_eq!(offer(&site, &login("user", "")), None);
        let never = Site { never: true, ..site };
        assert_eq!(offer(&never, &login("user", "u")), None);
        assert_eq!(offer(&never, &login("admin", "b")), Some(true)); // a saved login still gets its new password
    }

    #[test]
    fn the_latest_saved_comes_first_once() {
        let mut logins = vec![login("a", "1"), login("b", "2")];
        remember(&mut logins, login("b", "3"));
        assert_eq!(logins, [login("b", "3"), login("a", "1")]);
    }

    #[test]
    fn sites_are_kept_by_project_and_port() {
        std::env::set_var("PRETORIUM_NO_KEYCHAIN", "1");
        let (shop, blog) = (Key { project: "/p/shop".into(), port: 3000 }, Key { project: "/p/blog".into(), port: 3000 });
        store(&shop, &Site { logins: vec![login("admin", "s")], never: false });
        assert_eq!(load(&shop).logins, [login("admin", "s")]);
        assert_eq!(load(&blog), Site::default());
        store(&shop, &Site::default());
        assert!(MEMORY.lock().unwrap().get_or_insert_default().get(&shop.account()).is_none());
    }
}
