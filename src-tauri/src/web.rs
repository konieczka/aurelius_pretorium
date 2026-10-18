//! Browser tabs: one native webview per web tab, laid over its pane's body (the frontend sends where) and kept, like a
//! terminal's shell, while the tab is hidden. They're children of the app's window, so pages render in WebKit with
//! the app's own cookies and storage store, and remote pages get no IPC (Tauri refuses remote origins).

use crate::{err, Res};
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Url, Webview, WebviewBuilder, WebviewUrl, Window};

/// Labels of browser webviews: "web-<tab id>".
pub const PREFIX: &str = "web-";
fn label(id: &str) -> String {
    format!("{PREFIX}{id}")
}

// WKWebView's own user agent names no browser, and some sites serve it a degraded page. Present as Safari.
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15";

/// Native view (address) -> tab id, to tell which browser has keyboard focus.
#[derive(Default)]
pub struct Views(Mutex<HashMap<usize, String>>);

/// What a page is doing, for the tab's label and address bar. Sent to every window; the one with the tab takes it.
#[derive(Clone, serde::Serialize)]
struct State {
    id: String,
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    loading: Option<bool>,
    /// Whether its history has somewhere to go back / forward to
    #[serde(skip_serializing_if = "Option::is_none")]
    back: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    forward: Option<bool>,
}

fn emit_state(wv: &Webview, url: &Url, title: Option<String>, loading: Option<bool>) {
    let id = wv.label().trim_start_matches(PREFIX).to_string();
    let _ = wv.app_handle().emit("web-state", State { id, url: url.to_string(), title, loading, back: None, forward: None });
}

/// Where a page is, watched on its WKWebView: page loads don't cover in-page navigation (history.pushState,
/// replaceState, #fragments), which single-page apps route with. Also whether Back and Forward have anywhere to go.
#[cfg(target_os = "macos")]
mod watch {
    use super::State;
    use objc2::{define_class, msg_send, rc::Retained, runtime::{AnyObject, NSObject}, AllocAnyThread, DefinedClass};
    use objc2_foundation::{NSString, NSURL};
    use std::{cell::RefCell, collections::HashMap, ffi::c_void};
    use tauri::{AppHandle, Emitter};

    const KEYS: [&str; 3] = ["URL", "canGoBack", "canGoForward"];

    pub struct Ivars {
        view: Retained<AnyObject>,
        app: AppHandle,
        id: String,
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[ivars = Ivars]
        pub struct Watcher;

        impl Watcher {
            #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
            fn observe(&self, _key: *mut NSString, _of: *mut AnyObject, _change: *mut AnyObject, _ctx: *mut c_void) {
                self.report();
            }
        }
    );

    impl Watcher {
        fn report(&self) {
            let Ivars { view, app, id } = self.ivars();
            let (url, back, forward) = unsafe {
                let url: Option<Retained<NSURL>> = msg_send![&**view, URL];
                let back: bool = msg_send![&**view, canGoBack];
                let forward: bool = msg_send![&**view, canGoForward];
                (url.and_then(|u| u.absoluteString()).map(|s| s.to_string()), back, forward)
            };
            let state = State { id: id.clone(), url: url.unwrap_or_default(), title: None, loading: None, back: Some(back), forward: Some(forward) };
            let _ = app.emit("web-state", state);
        }
    }

    impl Drop for Watcher {
        fn drop(&mut self) {
            let view = &*self.ivars().view;
            for key in KEYS {
                let _: () = unsafe { msg_send![view, removeObserver: &*self, forKeyPath: &*NSString::from_str(key)] };
            }
        }
    }

    thread_local! {
        /// Tab id -> its page's watcher. Main thread only, like the views.
        static WATCHERS: RefCell<HashMap<String, Retained<Watcher>>> = RefCell::default();
    }

    /// Main thread: watch tab `id`'s WKWebView, reporting where it is right away.
    pub fn start(app: AppHandle, id: String, view: *mut c_void) {
        let Some(view) = (unsafe { Retained::retain(view as *mut AnyObject) }) else { return };
        let w = Watcher::alloc().set_ivars(Ivars { view: view.clone(), app, id: id.clone() });
        let w: Retained<Watcher> = unsafe { msg_send![super(w), init] };
        for key in KEYS {
            // NSKeyValueObservingOptionNew | Initial
            let _: () = unsafe { msg_send![&*view, addObserver: &*w, forKeyPath: &*NSString::from_str(key), options: 1usize | 4, context: std::ptr::null_mut::<c_void>()] };
        }
        WATCHERS.with(|m| m.borrow_mut().insert(id, w));
    }

    /// Main thread: stop watching (before the view goes: KVO mustn't outlive it).
    pub fn stop(id: &str) {
        WATCHERS.with(|m| m.borrow_mut().remove(id));
    }

    /// Main thread: show an error page when a load fails before the page commits (a dev server that's down, an
    /// unknown host). wry's navigation delegate doesn't handle that, so WebKit would just keep the old page (or
    /// nothing) and no event comes: reload looked like it did nothing. The method is added to wry's delegate class.
    pub fn catch_failures(view: *mut c_void) {
        use objc2::runtime::{Imp, Sel};
        use objc2::sel;
        let view = view as *mut AnyObject;
        unsafe {
            let delegate: *mut AnyObject = msg_send![view, navigationDelegate];
            let Some(cls) = delegate.as_ref().map(|d| d.class()) else { return };
            let imp: Imp = std::mem::transmute(failed as unsafe extern "C-unwind" fn(&AnyObject, Sel, &AnyObject, *mut AnyObject, &AnyObject));
            // A no-op once added (all tabs share the class)
            objc2::ffi::class_addMethod(cls as *const _ as *mut _, sel!(webView:didFailProvisionalNavigation:withError:), imp, c"v@:@@@".as_ptr());
            // WebKit looks at which methods the delegate has when it's set: set it again so this view sees the new one
            let _: () = msg_send![view, setNavigationDelegate: delegate];
        }
    }

    unsafe extern "C-unwind" fn failed(_this: &AnyObject, _cmd: objc2::runtime::Sel, view: &AnyObject, _nav: *mut AnyObject, error: &AnyObject) {
        let code: isize = msg_send![error, code];
        let domain: Retained<NSString> = msg_send![error, domain];
        // Not failures: another load or Stop cut it short, a link the tab won't open, a download
        match (&*domain.to_string(), code) {
            ("NSURLErrorDomain", -999) | ("WebKitErrorDomain", 102 | 204) => return,
            _ => {}
        }
        let info: *mut AnyObject = msg_send![error, userInfo];
        if info.is_null() {
            return;
        }
        let url: Option<Retained<NSURL>> = msg_send![info, objectForKey: &*NSString::from_str("NSErrorFailingURLKey")];
        let Some(url) = url else { return };
        let Some(href) = url.absoluteString().map(|s| s.to_string()) else { return };
        let message: Retained<NSString> = msg_send![error, localizedDescription];
        let html = NSString::from_str(&super::error_page(&href, &message.to_string(), code == -1004));
        // Safari's way: the page stands in for the address that failed, so it shows in the address bar and reload
        // tries it again. Private, so with a plain HTML load (where reload just redraws this page) if it's ever gone.
        let alternate = objc2::sel!(_loadAlternateHTMLString:baseURL:forUnreachableURL:);
        let has: bool = msg_send![view, respondsToSelector: alternate];
        if has {
            let _: () = msg_send![view, _loadAlternateHTMLString: &*html, baseURL: &*url, forUnreachableURL: &*url];
        } else {
            let _: *mut AnyObject = msg_send![view, loadHTMLString: &*html, baseURL: &*url];
        }
    }
}

/// What a tab shows when its page couldn't be loaded. No script: Try Again is a link to the address.
fn error_page(url: &str, message: &str, refused: bool) -> String {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
    let parsed = Url::parse(url).ok();
    let host = parsed.as_ref().and_then(|u| u.host_str().map(|h| match u.port() {
        Some(p) => format!("{h}:{p}"),
        None => h.to_string(),
    }));
    let host = host.unwrap_or_else(|| url.to_string());
    let local = parsed.as_ref().and_then(|u| u.host_str()).is_some_and(|h| matches!(h, "localhost" | "127.0.0.1" | "0.0.0.0" | "[::1]") || h.ends_with(".localhost"));
    let hint = if refused && local { "<p class=\"hint\">Nothing is answering on this port. Is the dev server running?</p>" } else { "" };
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>Can’t reach {host}</title><style>
:root {{ color-scheme: light dark; }}
body {{ margin: 0; height: 100vh; display: flex; align-items: center; justify-content: center; font: 13px -apple-system, system-ui, sans-serif; color: #6b6b6b; background: #fff; text-align: center; }}
@media (prefers-color-scheme: dark) {{ body {{ color: #a0a0a0; background: #1c1c1c; }} h1 {{ color: #eee !important; }} a {{ border-color: #444 !important; color: #eee !important; }} }}
main {{ max-width: 420px; padding: 24px; }}
h1 {{ margin: 0 0 8px; font-size: 17px; font-weight: 600; color: #1a1a1a; }}
p {{ margin: 0 0 6px; line-height: 1.5; word-break: break-word; }}
.hint {{ margin-top: 10px; }}
a {{ display: inline-block; margin-top: 18px; padding: 5px 14px; border: 1px solid #ccc; border-radius: 6px; color: #1a1a1a; text-decoration: none; }}
</style></head><body><main><h1>Can’t reach {host}</h1><p>{message}</p>{hint}<a href="{href}">Try Again</a></main></body></html>"#,
        host = esc(&host),
        message = esc(message),
        href = esc(url),
    )
}

// The page's best icon for a 13px tab: an SVG, else the smallest at least 32px (sharp at 2x), else the biggest; ones
// for the other color scheme are skipped. Without one, browsers try /favicon.ico.
const ICON_JS: &str = r#"(() => {
  const size = (l) => /svg/i.test(l.type) || /\.svg(\?|#|$)/i.test(l.href) || /any/i.test(l.sizes.value) ? 0
    : Math.max(16, ...[...l.sizes].map((s) => parseInt(s) || 0));
  const rank = (n) => (n === 0 ? 0 : n >= 32 ? n : 1e4 - n);
  const links = [...document.querySelectorAll('link[rel~="icon" i]')]
    .filter((l) => l.href && (!l.media || matchMedia(l.media).matches))
    .sort((a, b) => rank(size(a)) - rank(size(b)));
  return links.length ? links[0].href : location.origin + "/favicon.ico";
})()"#;

/// Icon address -> what it came to (a data: URL, or None: there was nothing usable there). Pages share icons.
static ICONS: Mutex<Option<HashMap<String, Option<String>>>> = Mutex::new(None);

#[derive(Clone, serde::Serialize)]
struct Icon {
    id: String,
    /// The page it's for: an answer that lands after the tab moved on is dropped.
    url: String,
    icon: Option<String>,
}

/// Ask the page for its icon, fetch it (off the main thread) and send it to the tab as a data: URL, which the app's
/// CSP allows where it wouldn't the site's own address.
fn find_icon(wv: &Webview) {
    let Ok(page) = wv.url() else { return };
    if !matches!(page.scheme(), "http" | "https") {
        return;
    }
    let (app, id) = (wv.app_handle().clone(), wv.label().trim_start_matches(PREFIX).to_string());
    let _ = wv.eval_with_callback(ICON_JS, move |json| {
        // What the page answers is the page's to make up: only an http(s) or data: image address is taken.
        let Some(href) = serde_json::from_str::<String>(&json).ok() else { return };
        let (app, id, page) = (app.clone(), id.clone(), page.to_string());
        std::thread::spawn(move || {
            let cached = ICONS.lock().unwrap().get_or_insert_default().get(&href).cloned();
            let icon = cached.unwrap_or_else(|| {
                let icon = fetch_icon(&href);
                let mut icons = ICONS.lock().unwrap();
                let icons = icons.get_or_insert_default();
                if icons.len() > 500 {
                    icons.clear();
                }
                icons.insert(href, icon.clone());
                icon
            });
            let _ = app.emit("web-icon", Icon { id, url: page, icon });
        });
    });
}

const ICON_MAX: u64 = 256 * 1024;

fn fetch_icon(href: &str) -> Option<String> {
    use base64::Engine;
    if let Some(data) = href.strip_prefix("data:") {
        return (data.starts_with("image/") && href.len() as u64 <= ICON_MAX).then(|| href.to_string());
    }
    let url: Url = href.parse().ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let mut res = ureq::get(url.as_str())
        .header("User-Agent", USER_AGENT)
        .config()
        .timeout_global(Some(std::time::Duration::from_secs(10)))
        .build()
        .call()
        .ok()?;
    let declared = res.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").split(';').next()?.trim().to_lowercase();
    let body = res.body_mut().with_config().limit(ICON_MAX).read_to_vec().ok()?;
    let kind = icon_type(&declared, &body)?;
    Some(format!("data:{kind};base64,{}", base64::engine::general_purpose::STANDARD.encode(&body)))
}

/// The image type to label it with, or None if it isn't one: servers send icons as octet-stream, or send an HTML page
/// (a 404 dressed as 200) where /favicon.ico should be.
fn icon_type(declared: &str, body: &[u8]) -> Option<&'static str> {
    let sniffed = match body {
        [0x89, b'P', b'N', b'G', ..] => Some("image/png"),
        [0, 0, 1, 0, ..] => Some("image/x-icon"),
        [b'G', b'I', b'F', b'8', ..] => Some("image/gif"),
        [0xFF, 0xD8, 0xFF, ..] => Some("image/jpeg"),
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Some("image/webp"),
        _ => None,
    };
    let svg = || {
        let head = String::from_utf8_lossy(&body[..body.len().min(1024)]).to_lowercase();
        (head.contains("<svg") && !head.contains("<html")).then_some("image/svg+xml")
    };
    sniffed.or_else(|| (declared == "image/svg+xml" || declared.starts_with("text/") || declared.is_empty()).then(svg).flatten())
}

/// The app's own pages (the dev server while developing) mustn't open in a tab: they'd count as the app, IPC included.
fn is_app(app: &AppHandle, url: &Url) -> bool {
    cfg!(dev) && app.config().build.dev_url.as_ref().is_some_and(|d| d.origin() == url.origin())
}

/// Web pages only. Not files, and not other apps' links (mailto:, zoommtg:…): a page could launch those unasked, and
/// this checks iframes too.
fn allow(app: &AppHandle, url: &Url) -> bool {
    match url.scheme() {
        "http" | "https" => !is_app(app, url),
        "about" | "data" | "blob" => true,
        _ => false,
    }
}

fn get(app: &AppHandle, id: &str) -> Res<Webview> {
    app.get_webview(&label(id)).ok_or_else(|| "no such browser tab".into())
}

/// Round the view's bottom corners to the pane's. Its top edge sits under the tab's address bar.
fn round(wv: &Webview, radius: f64) {
    #[cfg(target_os = "macos")]
    let _ = wv.with_webview(move |v| unsafe {
        use objc2::{msg_send, runtime::AnyObject};
        let v = v.inner() as *mut AnyObject;
        let _: () = msg_send![v, setWantsLayer: true];
        let layer: *mut AnyObject = msg_send![v, layer];
        if layer.is_null() {
            return;
        }
        // Which edge is "min y" depends on whether the view (or a superview) is flipped
        let flipped: bool = msg_send![layer, contentsAreFlipped];
        let bottom: usize = if flipped { 4 | 8 } else { 1 | 2 }; // kCALayer{Min,Max}X{Max,Min}YCorner
        let _: () = msg_send![layer, setCornerRadius: radius];
        let _: () = msg_send![layer, setMaskedCorners: bottom];
        let _: () = msg_send![layer, setMasksToBounds: true];
    });
    #[cfg(not(target_os = "macos"))]
    let _ = (wv, radius);
}

/// The project a tab opening `url` belongs to: on this Mac, the one serving it, else (the server isn't up yet) the folder
/// it was opened from; elsewhere none, so sites signed in to once are signed in everywhere. Blocking.
fn project_for(url: &Url, folder: String) -> String {
    match crate::logins::local_port(url) {
        Some(port) => crate::infra::project_on(port).unwrap_or(folder),
        None => String::new(),
    }
}

/// The cookies and storage for a project's tabs: a store of its own, so two projects on localhost (cookies don't go by
/// port) don't sign each other out. None: the app's. Stores need macOS 14; before it every tab has the app's.
fn data_store(project: &str) -> Option<[u8; 16]> {
    if project.is_empty() {
        return None;
    }
    // Two FNV-1a hashes of its path: the same project gets the same store across launches
    let fnv = |seed: u64| project.bytes().fold(seed, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    let mut id = [0; 16];
    id[..8].copy_from_slice(&fnv(0xcbf29ce484222325).to_le_bytes());
    id[8..].copy_from_slice(&fnv(0x6c62272e07bb0142).to_le_bytes());
    Some(id)
}

/// Show tab `id`'s browser at x, y, width, height (logical px in the window), opening `url` in it if it isn't open yet.
/// Opening it, the tab's `project` is settled (see project_for; `folder` is the one it was opened from) and returned;
/// it's passed back after, for the tab to keep its project, and so its store, across launches.
#[tauri::command]
pub async fn web_show(app: AppHandle, window: Window, id: String, url: String, project: Option<String>, folder: Option<String>, x: f64, y: f64, width: f64, height: f64, radius: f64) -> Res<Option<String>> {
    let (pos, size) = (LogicalPosition::new(x, y), LogicalSize::new(width.max(1.0), height.max(1.0)));
    if let Some(wv) = app.get_webview(&label(&id)) {
        if wv.window().label() != window.label() {
            wv.reparent(&window).map_err(err)?; // its workspace moved to this window
        }
        wv.set_bounds(tauri::Rect { position: pos.into(), size: size.into() }).map_err(err)?;
        wv.show().map_err(err)?;
        round(&wv, radius);
        return Ok(None);
    }
    let url: Url = url.parse().map_err(err)?;
    if !allow(&app, &url) {
        return Err(format!("can't open {url} here"));
    }
    let project = match project {
        Some(p) => p,
        None => {
            let (url, folder) = (url.clone(), folder.unwrap_or_default());
            crate::blocking(move || Ok(project_for(&url, folder))).await?
        }
    };
    if app.get_webview(&label(&id)).is_some() {
        return Ok(None); // made while that ran (calls for a tab go one at a time, so not by this page)
    }
    let (nav, opener) = (app.clone(), app.clone());
    let from = id.clone();
    let mut builder = WebviewBuilder::new(label(&id), WebviewUrl::External(url))
        .user_agent(USER_AGENT)
        .focused(false)
        .on_navigation(move |u| allow(&nav, u))
        // target=_blank and window.open: a new tab next to this one
        .on_new_window(move |u, _| {
            let _ = opener.emit("web-open", (&from, u.to_string()));
            tauri::webview::NewWindowResponse::Deny
        })
        .on_page_load(|wv, p| {
            let started = matches!(p.event(), tauri::webview::PageLoadEvent::Started);
            emit_state(&wv, p.url(), None, Some(started));
            if !started {
                find_icon(&wv);
                crate::logins::inject(&wv);
            }
        })
        .on_document_title_changed(|wv, title| {
            if let Ok(u) = wv.url() {
                emit_state(&wv, &u, Some(title), None);
            }
            find_icon(&wv); // single-page apps can swap it as they go
        });
    if let Some(store) = data_store(&project) {
        builder = builder.data_store_identifier(store);
    }
    crate::logins::set_project(&id, &project);
    let wv = window.add_child(builder, pos, size).map_err(err)?;
    #[cfg(target_os = "macos")]
    {
        let views = app.clone();
        let _ = wv.with_webview(move |v| {
            views.state::<Views>().0.lock().unwrap().insert(v.inner() as usize, id.clone());
            watch::catch_failures(v.inner());
            crate::logins::attach(views.clone(), id.clone(), v.inner());
            watch::start(views, id, v.inner());
        });
    }
    round(&wv, radius);
    Ok(Some(project))
}

#[tauri::command]
pub async fn web_hide(app: AppHandle, id: String) -> Res<()> {
    match app.get_webview(&label(&id)) {
        Some(wv) => wv.hide().map_err(err),
        None => Ok(()),
    }
}

/// Tab id -> its page zoom, when not 100%. The view keeps it across navigations; this is only to step from.
static ZOOMS: Mutex<Option<HashMap<String, f64>>> = Mutex::new(None);
// Safari's steps
const ZOOM_STEPS: [f64; 12] = [0.5, 0.75, 0.85, 1.0, 1.15, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0];

/// ⌘= / ⌘- / ⌘0 with a browser tab focused zoom its page only, not the app: in (1), out (-1) or back to 100% (0).
pub fn zoom(app: &AppHandle, id: &str, dir: i32) {
    let Ok(wv) = get(app, id) else { return };
    let mut zooms = ZOOMS.lock().unwrap();
    let zooms = zooms.get_or_insert_default();
    let cur = zooms.get(id).copied().unwrap_or(1.0);
    let level = match dir {
        0 => 1.0,
        1 => ZOOM_STEPS.into_iter().find(|&s| s > cur + 1e-6).unwrap_or(cur),
        _ => ZOOM_STEPS.into_iter().rev().find(|&s| s < cur - 1e-6).unwrap_or(cur),
    };
    if wv.set_zoom(level).is_ok() {
        if level == 1.0 {
            zooms.remove(id);
        } else {
            zooms.insert(id.to_string(), level);
        }
    }
}

/// The tab was closed (or its workspace left this window): the page goes.
#[tauri::command]
pub async fn web_close(app: AppHandle, id: String) -> Res<()> {
    app.state::<Views>().0.lock().unwrap().retain(|_, v| *v != id);
    ZOOMS.lock().unwrap().get_or_insert_default().remove(&id);
    crate::logins::forget_tab(&id);
    match app.get_webview(&label(&id)) {
        Some(wv) => {
            #[cfg(target_os = "macos")]
            let _ = wv.with_webview(move |_| watch::stop(&id));
            wv.close().map_err(err)
        }
        None => Ok(()),
    }
}

/// Hide all of this window's browsers: it reloaded, and its tabs show theirs again as they mount.
#[tauri::command]
pub async fn web_hide_all(window: Window) {
    for wv in window.webviews().into_iter().filter(|w| w.label().starts_with(PREFIX)) {
        let _ = wv.hide();
    }
}

#[tauri::command]
pub async fn web_navigate(app: AppHandle, id: String, url: String) -> Res<()> {
    let url: Url = url.parse().map_err(err)?;
    if !allow(&app, &url) {
        return Err(format!("can't open {url} here"));
    }
    get(&app, &id)?.navigate(url).map_err(err)
}

/// Back (-1) or forward (1) in the tab's history: the view's own, as Safari's buttons do, not the page's script.
#[tauri::command]
pub async fn web_go(app: AppHandle, id: String, delta: i32) -> Res<()> {
    let wv = get(&app, &id)?;
    #[cfg(target_os = "macos")]
    return wv
        .with_webview(move |v| unsafe {
            use objc2::{msg_send, runtime::AnyObject};
            let v = v.inner() as *mut AnyObject;
            let _: *mut AnyObject = if delta < 0 { msg_send![v, goBack] } else { msg_send![v, goForward] };
        })
        .map_err(err);
    #[cfg(not(target_os = "macos"))]
    wv.eval(format!("history.go({delta})")).map_err(err)
}

#[tauri::command]
pub async fn web_reload(app: AppHandle, id: String) -> Res<()> {
    get(&app, &id)?.reload().map_err(err)
}

#[tauri::command]
pub async fn web_stop(app: AppHandle, id: String) -> Res<()> {
    get(&app, &id)?.eval("window.stop()").map_err(err)
}

#[tauri::command]
pub async fn web_focus(app: AppHandle, id: String) -> Res<()> {
    get(&app, &id)?.set_focus().map_err(err)
}

/// The tab whose browser has keyboard focus in `window`, if one does.
#[tauri::command]
pub async fn web_focused(app: AppHandle, window: Window) -> Option<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let w = window.clone();
    window.run_on_main_thread(move || drop(tx.send(focused(&app, &w)))).ok()?;
    rx.recv().ok().flatten()
}

/// Main thread only (AppKit): which browser, if any, holds `window`'s first responder.
pub fn focused(app: &AppHandle, window: &Window) -> Option<String> {
    #[cfg(target_os = "macos")]
    unsafe {
        use objc2::{class, msg_send, runtime::AnyObject};
        let views = app.state::<Views>();
        let views = views.0.lock().unwrap();
        let mut v: *mut AnyObject = msg_send![window.ns_window().ok()? as *mut AnyObject, firstResponder];
        // The responder may be a view inside the WKWebView: walk up to it
        while !v.is_null() {
            if let Some(id) = views.get(&(v as usize)) {
                return Some(id.clone());
            }
            let is_view: bool = msg_send![v, isKindOfClass: class!(NSView)];
            if !is_view {
                break;
            }
            v = msg_send![v, superview];
        }
        None
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, window);
        None
    }
}

/// Give the keyboard back to `window`'s app page (a shortcut pressed in a browser acts on the app).
pub fn focus_app(app: &AppHandle, window: &str) {
    if let Some(wv) = app.get_webview(window) {
        let _ = wv.set_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::{data_store, error_page, icon_type};

    #[test]
    fn each_project_keeps_its_own_store() {
        assert_eq!(data_store(""), None);
        assert_eq!(data_store("/p/shop"), data_store("/p/shop"));
        assert_ne!(data_store("/p/shop"), data_store("/p/blog"));
    }

    #[test]
    fn error_pages_name_the_address_safely() {
        let page = error_page("http://localhost:5173/?q=<b>\"", "Could not connect to the server.", true);
        assert!(page.contains("<title>Can’t reach localhost:5173</title>"));
        assert!(page.contains(r#"href="http://localhost:5173/?q=&lt;b&gt;&quot;""#));
        assert!(page.contains("dev server"));
        assert!(!error_page("https://example.com/", "A server with the specified hostname could not be found.", false).contains("dev server"));
    }

    #[test]
    fn icons_are_told_by_their_bytes() {
        assert_eq!(icon_type("application/octet-stream", b"\x89PNG\r\n"), Some("image/png"));
        assert_eq!(icon_type("image/vnd.microsoft.icon", &[0, 0, 1, 0, 1]), Some("image/x-icon"));
        assert_eq!(icon_type("image/svg+xml", b"<?xml version=\"1.0\"?><svg xmlns=\"http://www.w3.org/2000/svg\"/>"), Some("image/svg+xml"));
        assert_eq!(icon_type("text/plain", b"<svg viewBox=\"0 0 1 1\"/>"), Some("image/svg+xml"));
        // A missing /favicon.ico answered with the site's HTML page
        assert_eq!(icon_type("text/html", b"<!doctype html><html><svg/></html>"), None);
        assert_eq!(icon_type("image/png", b"not an image"), None);
    }
}

