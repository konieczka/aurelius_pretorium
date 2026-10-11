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
}

fn emit_state(wv: &Webview, url: &Url, title: Option<String>, loading: Option<bool>) {
    let id = wv.label().trim_start_matches(PREFIX).to_string();
    let _ = wv.app_handle().emit("web-state", State { id, url: url.to_string(), title, loading });
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

/// Show tab `id`'s browser at x, y, width, height (logical px in the window), opening `url` in it if it isn't open yet.
#[tauri::command]
pub async fn web_show(app: AppHandle, window: Window, id: String, url: String, x: f64, y: f64, width: f64, height: f64, radius: f64) -> Res<()> {
    let (pos, size) = (LogicalPosition::new(x, y), LogicalSize::new(width.max(1.0), height.max(1.0)));
    if let Some(wv) = app.get_webview(&label(&id)) {
        if wv.window().label() != window.label() {
            wv.reparent(&window).map_err(err)?; // its workspace moved to this window
        }
        wv.set_bounds(tauri::Rect { position: pos.into(), size: size.into() }).map_err(err)?;
        wv.show().map_err(err)?;
        round(&wv, radius);
        return Ok(());
    }
    let url: Url = url.parse().map_err(err)?;
    if !allow(&app, &url) {
        return Err(format!("can't open {url} here"));
    }
    let (nav, opener) = (app.clone(), app.clone());
    let from = id.clone();
    let builder = WebviewBuilder::new(label(&id), WebviewUrl::External(url))
        .user_agent(USER_AGENT)
        .focused(false)
        .on_navigation(move |u| allow(&nav, u))
        // target=_blank and window.open: a new tab next to this one
        .on_new_window(move |u, _| {
            let _ = opener.emit("web-open", (&from, u.to_string()));
            tauri::webview::NewWindowResponse::Deny
        })
        .on_page_load(|wv, p| emit_state(&wv, p.url(), None, Some(matches!(p.event(), tauri::webview::PageLoadEvent::Started))))
        // Also how in-page navigation (history.pushState) shows: single-page apps retitle on it
        .on_document_title_changed(|wv, title| {
            if let Ok(u) = wv.url() {
                emit_state(&wv, &u, Some(title), None);
            }
        });
    let wv = window.add_child(builder, pos, size).map_err(err)?;
    #[cfg(target_os = "macos")]
    {
        let views = app.clone();
        let _ = wv.with_webview(move |v| {
            views.state::<Views>().0.lock().unwrap().insert(v.inner() as usize, id);
        });
    }
    round(&wv, radius);
    Ok(())
}

#[tauri::command]
pub async fn web_hide(app: AppHandle, id: String) -> Res<()> {
    match app.get_webview(&label(&id)) {
        Some(wv) => wv.hide().map_err(err),
        None => Ok(()),
    }
}

/// The tab was closed (or its workspace left this window): the page goes.
#[tauri::command]
pub async fn web_close(app: AppHandle, id: String) -> Res<()> {
    app.state::<Views>().0.lock().unwrap().retain(|_, v| *v != id);
    match app.get_webview(&label(&id)) {
        Some(wv) => wv.close().map_err(err),
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

/// Back (-1) or forward (1) in the tab's history.
#[tauri::command]
pub async fn web_go(app: AppHandle, id: String, delta: i32) -> Res<()> {
    get(&app, &id)?.eval(format!("history.go({delta})")).map_err(err)
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
