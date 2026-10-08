//! sigf-steam: the SIGF app's Steam helper (docs/WORKSHOP.md section 3). Talks to the player's running Steam client
//! through the Steamworks SDK as the game's app id: subscribe, unsubscribe, item state. One process per call, one JSON
//! object per stdout line, exits when done, so Steam shows the game as running only for a few seconds.
//!
//! `sigf-steam <subscribe|unsubscribe|state> <appid> [ids...]`
// The app reads piped stdout: no console window in a release build.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use steamworks::{Client, DownloadItemResult, ItemState, PublishedFileId, SteamAPIInitError, UGC};

/// A subscribe call waits this long for its downloads, then reports what is left as failed.
const SUBSCRIBE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// Unsubscribe answers come back in seconds; past this the call gives up on the rest.
const UNSUBSCRIBE_TIMEOUT: Duration = Duration::from_secs(60);
const POLL: Duration = Duration::from_millis(100);
/// Download progress is reported at most this often per item (state changes always go out at once).
const PROGRESS_EVERY: Duration = Duration::from_millis(400);
const MAX_IDS: usize = 200;

fn emit(v: Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

/// Ends the process with a start failure (exit code 2).
fn fail_start(error: &str, message: impl Into<String>) -> ! {
    emit(json!({ "ev": "done", "ok": false, "error": error, "message": message.into() }));
    std::process::exit(2)
}

/// A decimal id of at most `max_len` digits, no sign, no leading zero.
fn number(s: &str, max_len: usize) -> Option<u64> {
    (!s.is_empty() && s.len() <= max_len && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse().ok()).flatten()
}

fn init_error(e: SteamAPIInitError) -> (&'static str, String) {
    match e {
        SteamAPIInitError::NoSteamClient(m) => ("steam_not_running", m),
        SteamAPIInitError::FailedGeneric(m) => {
            let l = m.to_ascii_lowercase();
            if l.contains("licen") || l.contains("subscri") || l.contains("not own") {
                ("not_owned", m)
            } else if l.contains("running") || l.contains("steamclient") {
                // Steam not started, or not installed at all (steamclient could not be loaded).
                ("steam_not_running", m)
            } else {
                ("init_failed", m)
            }
        }
        SteamAPIInitError::VersionMismatch(m) => ("init_failed", format!("Steam client out of date: {m}")),
    }
}

fn item_json(ugc: &UGC, id: PublishedFileId) -> Value {
    let s = ugc.item_state(id);
    let mut v = json!({
        "id": id.0.to_string(),
        "subscribed": s.contains(ItemState::SUBSCRIBED),
        "installed": s.contains(ItemState::INSTALLED),
        "downloading": s.intersects(ItemState::DOWNLOADING | ItemState::DOWNLOAD_PENDING),
        "needsUpdate": s.contains(ItemState::NEEDS_UPDATE),
    });
    if let Some(info) = ugc.item_install_info(id).filter(|_| s.contains(ItemState::INSTALLED)) {
        v["sizeBytes"] = json!(info.size_on_disk);
    }
    v
}

fn progress(id: PublishedFileId, state: &str, done: u64, total: u64, error: Option<&str>) {
    let mut v = json!({ "ev": "progress", "id": id.0.to_string(), "state": state, "done": done, "total": total });
    if let Some(e) = error {
        v["error"] = json!(e);
    }
    emit(v);
}

/// Up to date on disk: installed, nothing pending.
fn ready(s: ItemState) -> bool {
    s.contains(ItemState::INSTALLED) && !s.intersects(ItemState::NEEDS_UPDATE | ItemState::DOWNLOADING | ItemState::DOWNLOAD_PENDING)
}

#[derive(Clone, Copy, PartialEq)]
enum Step {
    /// SubscribeItem sent, waiting for its answer.
    Subscribing,
    /// Subscribed, DownloadItem sent, waiting for the files.
    Downloading,
    Finished,
}

fn subscribe(client: &Client, ids: &[PublishedFileId]) -> bool {
    let ugc = client.ugc();
    // Answers of SubscribeItem and DownloadItemResult callbacks, by id: Err(message) on failure.
    let answers: Arc<Mutex<HashMap<u64, Result<(), String>>>> = Arc::default();
    let dl_errors: Arc<Mutex<HashMap<u64, String>>> = Arc::default();
    let errs = dl_errors.clone();
    let _cb = client.register_callback(move |r: DownloadItemResult| {
        if let Some(e) = r.error {
            errs.lock().unwrap().insert(r.published_file_id.0, e.to_string());
        }
    });

    let mut steps: HashMap<u64, Step> = HashMap::new();
    // The last `downloading` line per item: (done, total, when).
    let mut last: HashMap<u64, (u64, u64, Instant)> = HashMap::new();
    let mut all_ok = true;
    for &id in ids {
        if ugc.item_state(id).contains(ItemState::SUBSCRIBED) {
            steps.insert(id.0, Step::Downloading);
            ugc.download_item(id, true);
        } else {
            steps.insert(id.0, Step::Subscribing);
            progress(id, "subscribing", 0, 0, None);
            let a = answers.clone();
            ugc.subscribe_item(id, move |r| {
                a.lock().unwrap().insert(id.0, r.map_err(|e| e.to_string()));
            });
        }
    }

    let start = Instant::now();
    loop {
        client.run_callbacks();
        for &id in ids {
            let step = steps[&id.0];
            if step == Step::Finished {
                continue;
            }
            if step == Step::Subscribing {
                match answers.lock().unwrap().remove(&id.0) {
                    None => continue,
                    Some(Err(e)) => {
                        all_ok = false;
                        progress(id, "failed", 0, 0, Some(&format!("subscribe: {e}")));
                        steps.insert(id.0, Step::Finished);
                        continue;
                    }
                    Some(Ok(())) => {
                        steps.insert(id.0, Step::Downloading);
                        ugc.download_item(id, true);
                    }
                }
            }
            if let Some(e) = dl_errors.lock().unwrap().remove(&id.0) {
                all_ok = false;
                progress(id, "failed", 0, 0, Some(&format!("download: {e}")));
                steps.insert(id.0, Step::Finished);
                continue;
            }
            let s = ugc.item_state(id);
            if ready(s) {
                let size = ugc.item_install_info(id).map(|i| i.size_on_disk).unwrap_or(0);
                progress(id, "installed", size, size, None);
                steps.insert(id.0, Step::Finished);
                continue;
            }
            let (done, total) = ugc.item_download_info(id).unwrap_or((0, 0));
            let now = Instant::now();
            let changed = match last.get(&id.0) {
                None => true,
                Some((d, t, at)) => (*d, *t) != (done, total) && now.duration_since(*at) >= PROGRESS_EVERY,
            };
            if changed {
                progress(id, "downloading", done, total, None);
                last.insert(id.0, (done, total, now));
            }
        }
        if steps.values().all(|s| *s == Step::Finished) {
            break;
        }
        if start.elapsed() >= SUBSCRIBE_TIMEOUT {
            for &id in ids {
                if steps[&id.0] != Step::Finished {
                    let (done, total) = ugc.item_download_info(id).unwrap_or((0, 0));
                    progress(id, "failed", done, total, Some("timeout: Steam is still downloading this item"));
                }
            }
            all_ok = false;
            break;
        }
        std::thread::sleep(POLL);
    }
    all_ok
}

fn unsubscribe(client: &Client, ids: &[PublishedFileId]) -> (bool, Vec<String>) {
    let ugc = client.ugc();
    let answers: Arc<Mutex<HashMap<u64, Result<(), String>>>> = Arc::default();
    let mut waiting = 0;
    for &id in ids {
        if !ugc.item_state(id).contains(ItemState::SUBSCRIBED) {
            continue;
        }
        waiting += 1;
        let a = answers.clone();
        ugc.unsubscribe_item(id, move |r| {
            a.lock().unwrap().insert(id.0, r.map_err(|e| e.to_string()));
        });
    }
    let start = Instant::now();
    while answers.lock().unwrap().len() < waiting && start.elapsed() < UNSUBSCRIBE_TIMEOUT {
        client.run_callbacks();
        std::thread::sleep(POLL);
    }
    let answers = answers.lock().unwrap();
    let mut errors: Vec<String> = answers.iter().filter_map(|(id, r)| r.as_ref().err().map(|e| format!("{id}: {e}"))).collect();
    if answers.len() < waiting {
        errors.push("timeout: Steam did not answer every unsubscribe".into());
    }
    (errors.is_empty(), errors)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(cmd), Some(appid)) = (args.first(), args.get(1)) else {
        fail_start("init_failed", "usage: sigf-steam <subscribe|unsubscribe|state> <appid> [ids...]");
    };
    if !["subscribe", "unsubscribe", "state"].contains(&cmd.as_str()) {
        fail_start("init_failed", format!("unknown command: {cmd}"));
    }
    let Some(appid) = number(appid, 10).and_then(|n| u32::try_from(n).ok()) else {
        fail_start("init_failed", "bad app id");
    };
    let raw = &args[2..];
    if raw.len() > MAX_IDS {
        fail_start("init_failed", format!("at most {MAX_IDS} items per call"));
    }
    let mut ids: Vec<PublishedFileId> = vec![];
    for s in raw {
        let Some(n) = number(s, 20) else { fail_start("init_failed", format!("bad item id: {s}")) };
        if !ids.contains(&PublishedFileId(n)) {
            ids.push(PublishedFileId(n));
        }
    }
    if ids.is_empty() && cmd != "state" {
        fail_start("init_failed", "no item ids");
    }

    let client = match Client::init_app(appid) {
        Ok(c) => c,
        Err(e) => {
            let (code, message) = init_error(e);
            fail_start(code, message)
        }
    };
    if !client.apps().is_subscribed() {
        fail_start("not_owned", format!("this Steam account does not own app {appid}"));
    }
    client.run_callbacks();

    match cmd.as_str() {
        "subscribe" => {
            let ok = subscribe(&client, &ids);
            if ok {
                emit(json!({ "ev": "done", "ok": true }));
            } else {
                emit(json!({ "ev": "done", "ok": false, "error": "failed", "message": "some items did not install" }));
            }
        }
        "unsubscribe" => {
            let (ok, errors) = unsubscribe(&client, &ids);
            let mut v = json!({ "ev": "done", "ok": ok });
            if !ok {
                v["error"] = json!("failed");
                v["message"] = json!(errors.join("; "));
            }
            emit(v);
        }
        _ => {
            let ugc = client.ugc();
            if ids.is_empty() {
                ids = ugc.subscribed_items(true);
            }
            let items: Vec<Value> = ids.iter().map(|&id| item_json(&ugc, id)).collect();
            emit(json!({ "ev": "state", "items": items }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(number("440", 10), Some(440));
        assert_eq!(number("18446744073709551615", 20), Some(u64::MAX));
        for bad in ["", "0", "012", "-1", "+1", "1e3", "18446744073709551616", "123456789012345678901"] {
            assert_eq!(number(bad, 20), None, "{bad}");
        }
    }
}
