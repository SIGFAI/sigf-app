//! Browser games: a third-party game site played inside the main window, as a child webview over the frame the
//! Browser view leaves for it. No capability names the `web` webview, so the remote page cannot call any app command
//! (capabilities/default.json covers `main` only). Fullscreen follows the page's own Fullscreen API (the game's button,
//! Esc) and F11, signalled through the document title by a small script, since the page has no IPC.

use std::sync::atomic::{AtomicBool, Ordering};
use tauri::webview::{NewWindowResponse, WebviewBuilder};
use tauri::{Emitter, LogicalPosition, LogicalSize, Manager, Rect, WebviewUrl, WindowEvent};
use tauri_plugin_opener::OpenerExt;

const LABEL: &str = "web";
const SIGNAL: &str = "\u{2063}sigf:";

/// The game fills the whole screen; the frame bounds the UI sends are ignored until it leaves.
static FULLSCREEN: AtomicBool = AtomicBool::new(false);
static RESIZE_HOOK: AtomicBool = AtomicBool::new(false);

const SCRIPT: &str = r#"(() => {
  const P = '⁣sigf:';
  const say = (m) => {
    const t = document.title;
    document.title = P + m;
    setTimeout(() => { if (document.title === P + m) document.title = t; }, 80);
  };
  document.addEventListener('fullscreenchange', () => say(document.fullscreenElement ? 'fs-on' : 'fs-off'));
  addEventListener('keydown', (e) => {
    if (e.key !== 'F11') return;
    e.preventDefault();
    if (document.fullscreenElement) document.exitFullscreen(); else say('fs-toggle');
  }, true);
})();"#;

/// `x, y, w, h`: the frame in the main window, in CSS (logical) pixels.
#[derive(serde::Deserialize, Clone, Copy)]
pub struct Frame {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

fn rect(f: Frame) -> Rect {
    Rect { position: LogicalPosition::new(f.x, f.y).into(), size: LogicalSize::new(f.w.max(1.0), f.h.max(1.0)).into() }
}

fn https(url: &str) -> Option<tauri::Url> {
    tauri::Url::parse(url).ok().filter(|u| u.scheme() == "https" && u.username().is_empty() && u.password().is_none())
}

fn fill_window(app: &tauri::AppHandle) {
    let (Some(win), Some(wv)) = (app.get_window("main"), app.get_webview(LABEL)) else { return };
    if let (Ok(size), Ok(scale)) = (win.inner_size(), win.scale_factor()) {
        let s = size.to_logical::<f64>(scale);
        let _ = wv.set_bounds(rect(Frame { x: 0.0, y: 0.0, w: s.width, h: s.height }));
    }
}

fn set_fullscreen(app: &tauri::AppHandle, on: bool) {
    if FULLSCREEN.swap(on, Ordering::SeqCst) == on {
        return;
    }
    if let Some(win) = app.get_window("main") {
        let _ = win.set_fullscreen(on);
    }
    if on {
        fill_window(app);
    }
    if let Some(wv) = app.get_webview(LABEL) {
        let _ = wv.set_focus();
    }
    // Leaving: the view sends its frame again.
    let _ = app.emit("web://fullscreen", on);
}

/// Opens (or switches) the game in the frame.
#[tauri::command]
pub async fn web_open(app: tauri::AppHandle, url: String, frame: Frame) -> Result<(), String> {
    let u = https(&url).ok_or_else(|| format!("refused url: {url}"))?;
    if let Some(wv) = app.get_webview(LABEL) {
        wv.navigate(u).map_err(|e| e.to_string())?;
        let _ = wv.set_bounds(rect(frame));
        return wv.set_focus().map_err(|e| e.to_string());
    }
    let win = app.get_window("main").ok_or("no main window")?;
    if !RESIZE_HOOK.swap(true, Ordering::SeqCst) {
        let a = app.clone();
        win.on_window_event(move |e| {
            if matches!(e, WindowEvent::Resized(_)) && FULLSCREEN.load(Ordering::SeqCst) {
                fill_window(&a);
            }
        });
    }
    let opener = app.clone();
    let signals = app.clone();
    let builder = WebviewBuilder::new(LABEL, WebviewUrl::External(u))
        .initialization_script(SCRIPT)
        // Pop-ups (ads, "join our Discord") go to the system browser, never a new app window.
        .on_new_window(move |u, _| {
            if u.scheme() == "https" {
                let _ = opener.opener().open_url(u.as_str(), None::<&str>);
            }
            NewWindowResponse::Deny
        })
        .on_document_title_changed(move |_, title| match title.strip_prefix(SIGNAL) {
            Some("fs-on") => set_fullscreen(&signals, true),
            Some("fs-off") => set_fullscreen(&signals, false),
            Some("fs-toggle") => set_fullscreen(&signals, !FULLSCREEN.load(Ordering::SeqCst)),
            _ => {}
        });
    let r = rect(frame);
    let wv = win.add_child(builder, r.position, r.size).map_err(|e| e.to_string())?;
    wv.set_focus().map_err(|e| e.to_string())
}

/// The frame moved or resized (window resize, layout).
#[tauri::command]
pub fn web_frame(app: tauri::AppHandle, frame: Frame) {
    if FULLSCREEN.load(Ordering::SeqCst) {
        return;
    }
    if let Some(wv) = app.get_webview(LABEL) {
        let _ = wv.set_bounds(rect(frame));
    }
}

#[tauri::command]
pub fn web_fullscreen(app: tauri::AppHandle, on: bool) {
    set_fullscreen(&app, on);
}

/// Back to the list: the game stops.
#[tauri::command]
pub fn web_close(app: tauri::AppHandle) {
    set_fullscreen(&app, false);
    if let Some(wv) = app.get_webview(LABEL) {
        let _ = wv.close();
    }
}
