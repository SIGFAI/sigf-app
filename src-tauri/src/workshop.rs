//! Steam Workshop (docs/WORKSHOP.md): the commands that drive the Steam helper (`sigf-steam`, its own crate in
//! app/steam-helper, so this app never loads steam_api64.dll), the player's libraries in `<SIGF_HOME>/libraries.json`,
//! and `sigf://library/...` share links.
use serde::{Deserialize, Serialize};
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tauri::Emitter;

/// Item ids per helper call and per share link.
const MAX_CALL_IDS: usize = 200;
/// Items in one library.
const MAX_LIBRARY_ITEMS: usize = 500;
const MAX_LIBRARIES: usize = 200;
const MAX_NAME: usize = 80;
/// The helper gives up on downloads after 30 min; past this the core stops waiting and ends it.
const HELPER_DEADLINE: Duration = Duration::from_secs(35 * 60);

#[cfg(windows)]
const HELPER_EXE: &str = "sigf-steam.exe";
#[cfg(not(windows))]
const HELPER_EXE: &str = "sigf-steam";

/// One helper at a time: each one starts the Steam API as a game, and Steam shows one game running.
static HELPER_LOCK: Mutex<()> = Mutex::new(());
/// libraries.json is read and rewritten whole: one writer at a time.
static LIBRARIES_LOCK: Mutex<()> = Mutex::new(());
/// Steam app ids found by the last scan: the only ids the helper is started for.
static STEAM_APPIDS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// A command's error for the UI: `steam_not_running`, `not_owned`, `not_steam_game`, `helper_missing`, `init_failed`,
/// `library` (libraries.json refused or unwritable) or `failed`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WorkshopError {
    pub code: String,
    pub message: String,
}

impl WorkshopError {
    pub(crate) fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into() }
    }
    fn failed(message: impl Into<String>) -> Self {
        Self::new("failed", message)
    }
}

/// A decimal number of 1..=`max_len` digits, no sign, no leading zero, that fits a u64.
fn digits(s: &str, max_len: usize) -> Option<u64> {
    ((1..=max_len).contains(&s.len()) && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse().ok()).flatten()
}

/// A Steam app id: decimal, 1..=10 digits, no leading zero, fits a u32.
pub fn valid_appid(s: &str) -> bool {
    digits(s, 10).is_some_and(|n| u32::try_from(n).is_ok())
}

/// A published file id: decimal, 1..=20 digits, no leading zero, fits a u64 (decimal strings: they exceed 2^53).
pub fn valid_item_id(s: &str) -> bool {
    digits(s, 20).is_some()
}

/// The ids of one helper call: each valid, at most `MAX_CALL_IDS`, duplicates dropped (order kept).
fn check_ids(ids: &[String]) -> Result<Vec<String>, WorkshopError> {
    if ids.len() > MAX_CALL_IDS {
        return Err(WorkshopError::failed(format!("at most {MAX_CALL_IDS} items at a time")));
    }
    let mut out: Vec<String> = vec![];
    for id in ids {
        if !valid_item_id(id) {
            return Err(WorkshopError::failed(format!("bad item id: {id}")));
        }
        if !out.contains(id) {
            out.push(id.clone());
        }
    }
    Ok(out)
}

/// Keeps the Steam app ids of a scan (called with every scan the app makes).
pub fn remember_scan(s: &crate::scan::Scan) {
    *STEAM_APPIDS.lock().unwrap_or_else(|p| p.into_inner()) =
        s.games.iter().filter(|g| g.store == "steam" && valid_appid(&g.store_id)).map(|g| g.store_id.clone()).collect();
}

/// The app id must be a Steam game of the last scan (scanning again once if it is not).
fn check_steam_game(appid: &str) -> Result<(), WorkshopError> {
    if !valid_appid(appid) {
        return Err(WorkshopError::new("not_steam_game", format!("bad Steam app id: {appid}")));
    }
    let known = |a: &str| STEAM_APPIDS.lock().unwrap_or_else(|p| p.into_inner()).iter().any(|x| x == a);
    if known(appid) {
        return Ok(());
    }
    crate::remember_scan(&crate::scan::scan());
    if known(appid) {
        Ok(())
    } else {
        Err(WorkshopError::new("not_steam_game", format!("app {appid} is not a Steam game on this PC")))
    }
}

/// `sigf-steam` next to the app's own exe (where the installer puts it); a dev build also looks in the helper's
/// own target folders.
fn helper_path() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = vec![];
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        candidates.push(dir.join(HELPER_EXE));
    }
    #[cfg(debug_assertions)]
    {
        let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("steam-helper").join("target");
        // Release first: build.mjs puts the Steam API library next to that one.
        candidates.push(target.join("release").join(HELPER_EXE));
        candidates.push(target.join("debug").join(HELPER_EXE));
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// Codes the helper may hand back; anything else reads as `failed`.
fn known_code(code: &str) -> &str {
    match code {
        "steam_not_running" | "not_owned" | "init_failed" | "failed" => code,
        _ => "failed",
    }
}

/// The helper's final line as an error, when it says `ok: false`.
fn done_error(v: &serde_json::Value) -> Option<WorkshopError> {
    if v.get("ok").and_then(|o| o.as_bool()) == Some(true) {
        return None;
    }
    let code = v.get("error").and_then(|e| e.as_str()).unwrap_or("failed");
    let message = v.get("message").and_then(|m| m.as_str()).unwrap_or("the Steam helper failed");
    Some(WorkshopError::new(known_code(code), message))
}

/// Runs the helper once (`<command> <appid> [ids...]`), hands each progress line to `on_progress`, and returns its
/// final line (`done` or `state`). Holds `HELPER_LOCK` for the whole run. Blocking.
fn run_helper(command: &str, appid: &str, ids: &[String], on_progress: &mut dyn FnMut(serde_json::Value)) -> Result<serde_json::Value, WorkshopError> {
    let exe = helper_path().ok_or_else(|| WorkshopError::new("helper_missing", "the Steam helper (sigf-steam) is missing: reinstall SIGF"))?;
    let _g = HELPER_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut cmd = std::process::Command::new(&exe);
    cmd.arg(command)
        .arg(appid)
        .args(ids)
        .env("SteamAppId", appid)
        .env("SteamGameId", appid)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    if let Some(dir) = exe.parent() {
        cmd.current_dir(dir);
    }
    crate::install::tools::no_window(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| WorkshopError::new("helper_missing", format!("{}: {e}", exe.display())))?;
    let stdout = child.stdout.take().ok_or_else(|| WorkshopError::failed("no helper output"))?;
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let deadline = std::time::Instant::now() + HELPER_DEADLINE;
    let mut last: Option<serde_json::Value> = None;
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        match rx.recv_timeout(left) {
            Ok(line) => {
                // Only JSON objects count: the Steam API may print its own lines.
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
                match v.get("ev").and_then(|e| e.as_str()) {
                    Some("progress") => on_progress(v),
                    Some("done") | Some("state") => last = Some(v),
                    _ => {}
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(WorkshopError::failed("the Steam helper did not finish in time"));
            }
        }
    }
    let status = child.wait().map_err(|e| WorkshopError::failed(e.to_string()))?;
    last.ok_or_else(|| WorkshopError::failed(format!("the Steam helper ended without an answer ({status})")))
}

/// An item as Steam has it on this PC.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemState {
    pub id: String,
    pub subscribed: bool,
    pub installed: bool,
    pub downloading: bool,
    pub needs_update: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

/// `workshop://progress`: the helper's progress line with the app id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Progress {
    #[serde(default)]
    pub appid: String,
    pub id: String,
    pub state: String,
    #[serde(default)]
    pub done: u64,
    #[serde(default)]
    pub total: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The checks every helper call makes, then the call, off the async runtime. A helper that could not start the Steam
/// API (`init_failed`) while no Steam process runs reads as `steam_not_running`.
async fn helper_call(
    app: Option<tauri::AppHandle>,
    command: &'static str,
    appid: String,
    ids: Vec<String>,
) -> Result<serde_json::Value, WorkshopError> {
    tauri::async_runtime::spawn_blocking(move || {
        check_steam_game(&appid)?;
        let a = appid.clone();
        let v = run_helper(command, &appid, &ids, &mut |v| {
            let Some(app) = &app else { return };
            if let Ok(mut p) = serde_json::from_value::<Progress>(v) {
                if valid_item_id(&p.id) {
                    p.appid = a.clone();
                    let _ = app.emit("workshop://progress", &p);
                }
            }
        })?;
        if done_error(&v).is_some_and(|e| e.code == "init_failed") && !crate::launch::steam_process_running() {
            return Err(WorkshopError::new("steam_not_running", "Steam is not running: start Steam and sign in"));
        }
        Ok(v)
    })
    .await
    .map_err(|e| WorkshopError::failed(e.to_string()))?
}

/// `subscribe` or `unsubscribe` for these items; nothing to do on an empty list.
async fn change_subscriptions(app: Option<tauri::AppHandle>, command: &'static str, appid: String, ids: Vec<String>) -> Result<(), WorkshopError> {
    let ids = check_ids(&ids)?;
    if ids.is_empty() {
        return Ok(());
    }
    let done = helper_call(app, command, appid, ids).await?;
    done_error(&done).map_or(Ok(()), Err)
}

/// Subscribes to the items and waits until Steam has them on disk, with `workshop://progress` events on the way.
#[tauri::command]
pub async fn workshop_subscribe(app: tauri::AppHandle, appid: String, ids: Vec<String>) -> Result<(), WorkshopError> {
    change_subscriptions(Some(app), "subscribe", appid, ids).await
}

#[tauri::command]
pub async fn workshop_unsubscribe(appid: String, ids: Vec<String>) -> Result<(), WorkshopError> {
    change_subscriptions(None, "unsubscribe", appid, ids).await
}

/// The state of these items, or of every item the player subscribes to for that game (`ids: null`).
#[tauri::command]
pub async fn workshop_state(appid: String, ids: Option<Vec<String>>) -> Result<Vec<ItemState>, WorkshopError> {
    let ids = check_ids(&ids.unwrap_or_default())?;
    let v = helper_call(None, "state", appid, ids).await?;
    if v.get("ev").and_then(|e| e.as_str()) != Some("state") {
        return Err(done_error(&v).unwrap_or_else(|| WorkshopError::failed("no state from the Steam helper")));
    }
    let items: Vec<ItemState> = serde_json::from_value(v.get("items").cloned().unwrap_or_default()).map_err(|e| WorkshopError::failed(e.to_string()))?;
    Ok(items.into_iter().filter(|i| valid_item_id(&i.id)).collect())
}

// --- Libraries -------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibrarySource {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// A named, ordered group of mods for one game (docs/WORKSHOP.md section 4, docs/GAME-HUB.md section 6). Items are
/// refs (`ts:...`, `nx:...`); a bare number is a Steam Workshop id. `appid` (Steam) or `game` (canonical id, for games
/// not on Steam), at least one of them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game: Option<String>,
    pub name: String,
    pub items: Vec<String>,
    #[serde(default)]
    pub applied: bool,
    #[serde(default)]
    pub added_by_us: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<LibrarySource>,
    pub created: u64,
    pub updated: u64,
}

/// A library id as the UI makes it: 10 characters of `[a-km-z2-9]`.
fn valid_library_id(s: &str) -> bool {
    s.len() == 10 && s.bytes().all(|b| matches!(b, b'a'..=b'k' | b'm'..=b'z' | b'2'..=b'9'))
}

fn name_ok(s: &str) -> bool {
    !s.trim().is_empty() && s.chars().count() <= MAX_NAME && !s.chars().any(char::is_control)
}

/// A library item: a Workshop id (bare digits) or a mod ref (`crate::mods::valid_ref`).
pub fn valid_library_item(s: &str) -> bool {
    valid_item_id(s) || crate::mods::valid_ref(s)
}

fn valid_library(l: &Library) -> bool {
    valid_library_id(&l.id)
        && (l.appid.is_some() || l.game.is_some())
        && l.appid.as_deref().is_none_or(valid_appid)
        && l.game.as_deref().is_none_or(crate::mods::valid_game)
        && name_ok(&l.name)
        && l.items.len() <= MAX_LIBRARY_ITEMS
        && l.items.iter().all(|i| valid_library_item(i))
        && l.added_by_us.len() <= MAX_LIBRARY_ITEMS
        && l.added_by_us.iter().all(|i| valid_library_item(i))
        && l.source.as_ref().is_none_or(|s| ["collection", "link"].contains(&s.kind.as_str()) && s.id.as_deref().is_none_or(valid_item_id))
}

fn libraries_path() -> PathBuf {
    crate::install::home_dir().join("libraries.json")
}

/// The saved libraries, in their saved order; an unreadable file reads as empty, a malformed entry is left out.
fn load_libraries(path: &Path) -> Vec<Library> {
    let raw: Vec<serde_json::Value> = std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    raw.into_iter().filter_map(|v| serde_json::from_value::<Library>(v).ok()).filter(valid_library).take(MAX_LIBRARIES).collect()
}

fn write_libraries(path: &Path, list: &[Library]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(list).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// Adds the library, or replaces the one with its id in place; a new one goes first. An empty `appid` or `game` reads
/// as absent.
fn save_library(path: &Path, mut lib: Library) -> Result<Vec<Library>, String> {
    lib.appid = lib.appid.filter(|a| !a.is_empty());
    lib.game = lib.game.filter(|g| !g.is_empty());
    if !valid_library(&lib) {
        return Err("refused library".into());
    }
    let mut list = load_libraries(path);
    match list.iter_mut().find(|l| l.id == lib.id) {
        Some(l) => *l = lib,
        None => {
            if list.len() >= MAX_LIBRARIES {
                return Err(format!("at most {MAX_LIBRARIES} libraries"));
            }
            list.insert(0, lib);
        }
    }
    write_libraries(path, &list)?;
    Ok(list)
}

fn delete_library(path: &Path, id: &str) -> Result<Vec<Library>, String> {
    let list: Vec<Library> = load_libraries(path).into_iter().filter(|l| l.id != id).collect();
    write_libraries(path, &list)?;
    Ok(list)
}

/// Runs a libraries.json read or change under `LIBRARIES_LOCK`, off the async runtime; errors as code `library`.
async fn with_libraries<T: Send + 'static>(f: impl FnOnce(&Path) -> Result<T, String> + Send + 'static) -> Result<T, WorkshopError> {
    tauri::async_runtime::spawn_blocking(move || {
        let _g = LIBRARIES_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        f(&libraries_path())
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r)
    .map_err(|m| WorkshopError::new("library", m))
}

#[tauri::command]
pub async fn libraries_list() -> Result<Vec<Library>, WorkshopError> {
    with_libraries(|p| Ok(load_libraries(p))).await
}

#[tauri::command]
pub async fn libraries_save(lib: Library) -> Result<Vec<Library>, WorkshopError> {
    with_libraries(move |p| save_library(p, lib)).await
}

#[tauri::command]
pub async fn libraries_delete(id: String) -> Result<Vec<Library>, WorkshopError> {
    with_libraries(move |p| delete_library(p, &id)).await
}

// --- Share links -----------------------------------------------------------------------------------------------------

/// A library share link: `sigf://library/{appid}/<ids comma-separated>?name=<urlencoded>`.
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryLink {
    appid: String,
    items: Vec<String>,
    name: Option<String>,
}

impl LibraryLink {
    /// The link's one form, as the UI receives it.
    pub fn to_link(&self) -> String {
        let mut s = format!("sigf://library/{}/{}", self.appid, self.items.join(","));
        if let Some(n) = &self.name {
            s.push_str("?name=");
            s.push_str(&crate::report::encode(n));
        }
        s
    }
}

/// `%XX` and `+` decoded; None on a broken escape or bytes that are not UTF-8.
fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                let hex = s.get(i + 1..i + 3)?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// A library link: `sigf://library/...`, or the same after `https://sigf.ai/library/` (what gets pasted). Every item
/// id is checked, at most `MAX_CALL_IDS` (duplicates dropped); a `name` that is not a valid library name is dropped.
pub fn parse_library_link(link: &str) -> Option<LibraryLink> {
    let s = link.trim();
    let rest = crate::join::strip_prefix_ci(s, "sigf://library/")
        .or_else(|| crate::join::strip_prefix_ci(s, "https://sigf.ai/library/"))
        .or_else(|| crate::join::strip_prefix_ci(s, "https://www.sigf.ai/library/"))?;
    let rest = rest.split('#').next().unwrap_or("");
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let (appid, list) = path.trim_end_matches('/').split_once('/')?;
    // A Steam app id, or a game's canonical id for a library of a game outside Steam.
    let numeric = appid.bytes().all(|b| b.is_ascii_digit());
    if !(if numeric { valid_appid(appid) } else { crate::mods::valid_game(appid) }) {
        return None;
    }
    let mut items: Vec<String> = vec![];
    for raw in list.split(',').flat_map(|p| p.split("%2C")).flat_map(|p| p.split("%2c")) {
        // Workshop ids or mod refs (`ts:Ns-Name`, the `:` possibly sent as %3A).
        let item = percent_decode(raw)?;
        if !valid_library_item(&item) {
            return None;
        }
        if !items.contains(&item) {
            items.push(item);
        }
    }
    if items.is_empty() || items.len() > MAX_CALL_IDS {
        return None;
    }
    let name = query
        .split('&')
        .find_map(|kv| kv.strip_prefix("name="))
        .and_then(percent_decode)
        .map(|n| n.trim().to_string())
        .filter(|n| name_ok(n));
    Some(LibraryLink { appid: appid.to_string(), items, name })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_links_with_refs() {
        let l = parse_library_link("sigf://library/lethal/ts:notnotnotswipez-MoreCompany,gb%3AMod%3A123,456?name=Mix").unwrap();
        assert_eq!(l.to_link(), "sigf://library/lethal/ts:notnotnotswipez-MoreCompany,gb:Mod:123,456?name=Mix");
        assert!(parse_library_link("sigf://library/lethal/ts:../x").is_none());
        assert!(parse_library_link("sigf://library/Bad_Game/1").is_none());
    }

    #[test]
    fn ids() {
        assert!(valid_appid("440") && valid_appid("4294967295"));
        for bad in ["", "0", "0440", "-1", "4294967296", "44 0", "440a"] {
            assert!(!valid_appid(bad), "{bad}");
        }
        assert!(valid_item_id("3012345678") && valid_item_id("18446744073709551615"));
        for bad in ["", "0", "012", "18446744073709551616", "123456789012345678901", "1.5", "--help"] {
            assert!(!valid_item_id(bad), "{bad}");
        }
        assert_eq!(check_ids(&["2".into(), "1".into(), "2".into()]).unwrap(), ["2", "1"]);
        assert!(check_ids(&["/etc/passwd".into()]).is_err());
        let many: Vec<String> = (1..=201).map(|i| i.to_string()).collect();
        assert!(check_ids(&many).is_err());
        assert!(check_ids(&many[..200]).is_ok());
    }

    #[test]
    fn library_links() {
        let l = parse_library_link("sigf://library/440/3012345678,2987654321?name=Hats%20%26%20maps").unwrap();
        assert_eq!(l, LibraryLink { appid: "440".into(), items: vec!["3012345678".into(), "2987654321".into()], name: Some("Hats & maps".into()) });
        assert_eq!(l.to_link(), "sigf://library/440/3012345678,2987654321?name=Hats%20%26%20maps");
        // Pasted web form, encoded commas, a trailing slash, a fragment, a duplicate, a `+` space.
        let w = parse_library_link("  HTTPS://sigf.ai/library/4000/5%2C6,5/?x=1&name=My+mods#top ").unwrap();
        assert_eq!(w.to_link(), "sigf://library/4000/5,6?name=My%20mods");
        assert_eq!(parse_library_link("https://www.sigf.ai/library/4000/7").unwrap().to_link(), "sigf://library/4000/7");
        // A bad name is dropped, the link still imports.
        assert_eq!(parse_library_link("sigf://library/440/1?name=a%0Ab").unwrap().name, None);
        assert_eq!(parse_library_link("sigf://library/440/1?name=%ZZ").unwrap().name, None);
        assert_eq!(parse_library_link(&format!("sigf://library/440/1?name={}", "x".repeat(81))).unwrap().name, None);
        let max: Vec<String> = (1..=200).map(|i| i.to_string()).collect();
        assert!(parse_library_link(&format!("sigf://library/440/{}", max.join(","))).is_some());
        for bad in [
            "sigf://library/440",
            "sigf://library/440/",
            "sigf://library/0440/1",
            "sigf://library/440/1,,2",
            "sigf://library/440/1,abc",
            "sigf://library/440/1/2",
            "sigf://library/../1",
            "sigf://join/k3m9xq2wa7fd",
            "https://evil.example/library/440/1",
            "https://sigf.ai.evil.example/library/440/1",
            "http://sigf.ai/library/440/1",
        ] {
            assert!(parse_library_link(bad).is_none(), "{bad}");
        }
        let over: Vec<String> = (1..=201).map(|i| i.to_string()).collect();
        assert!(parse_library_link(&format!("sigf://library/440/{}", over.join(","))).is_none());
    }

    fn lib(id: &str, name: &str) -> Library {
        Library {
            id: id.into(),
            appid: Some("440".into()),
            game: None,
            name: name.into(),
            items: vec!["3012345678".into(), "2987654321".into()],
            applied: false,
            added_by_us: vec![],
            source: Some(LibrarySource { kind: "link".into(), id: None }),
            created: 1_760_000_000_000,
            updated: 1_760_000_000_000,
        }
    }

    #[test]
    fn libraries_store() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("libraries.json");
        assert!(load_libraries(&p).is_empty(), "no file yet");
        save_library(&p, lib("abcdefghjk", "First")).unwrap();
        let list = save_library(&p, lib("bcdefghjkm", "Second")).unwrap();
        assert_eq!(list.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(), ["bcdefghjkm", "abcdefghjk"], "new first");
        // Upsert keeps the place.
        let list = save_library(&p, Library { applied: true, ..lib("abcdefghjk", "First, renamed") }).unwrap();
        assert_eq!(list[1].name, "First, renamed");
        assert!(list[1].applied);
        assert_eq!(load_libraries(&p), list);
        let list = delete_library(&p, "bcdefghjkm").unwrap();
        assert_eq!(list.len(), 1);
        // The UI's JSON shape reads back.
        let ui = r#"[{"id":"cdefghjkmn","appid":"440","name":"UI","items":["1"],"applied":false,"addedByUs":["1"],"source":{"kind":"collection","id":"99"},"created":1,"updated":2}]"#;
        std::fs::write(&p, ui).unwrap();
        assert_eq!(load_libraries(&p)[0].added_by_us, ["1"]);
    }

    #[test]
    fn libraries_refused() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("libraries.json");
        for bad in [
            lib("short", "x"),
            lib("ABCDEFGHJK", "x"),
            lib("abcdefghjl", "x"),
            lib("abcdefghjk", ""),
            lib("abcdefghjk", "   "),
            lib("abcdefghjk", &"x".repeat(81)),
            lib("abcdefghjk", "a\nb"),
            Library { appid: Some("steam".into()), ..lib("abcdefghjk", "x") },
            Library { appid: None, game: None, ..lib("abcdefghjk", "x") },
            Library { appid: Some(String::new()), game: None, ..lib("abcdefghjk", "x") },
            Library { appid: None, game: Some("Lethal Company".into()), ..lib("abcdefghjk", "x") },
            Library { appid: None, game: Some("x".repeat(41)), ..lib("abcdefghjk", "x") },
            Library { items: vec!["xx:1".into()], ..lib("abcdefghjk", "x") },
            Library { items: vec!["ts:a b".into()], ..lib("abcdefghjk", "x") },
            Library { items: vec![format!("ts:{}", "a".repeat(118))], ..lib("abcdefghjk", "x") },
            Library { items: vec!["ws:abc".into()], ..lib("abcdefghjk", "x") },
            Library { added_by_us: vec!["ts:".into()], ..lib("abcdefghjk", "x") },
            Library { items: vec!["1".into(), "../x".into()], ..lib("abcdefghjk", "x") },
            Library { items: (1..=501).map(|i| i.to_string()).collect(), ..lib("abcdefghjk", "x") },
            Library { added_by_us: vec!["x".into()], ..lib("abcdefghjk", "x") },
            Library { source: Some(LibrarySource { kind: "web".into(), id: None }), ..lib("abcdefghjk", "x") },
            Library { source: Some(LibrarySource { kind: "collection".into(), id: Some("abc".into()) }), ..lib("abcdefghjk", "x") },
        ] {
            assert!(save_library(&p, bad).is_err());
        }
        assert!(save_library(&p, Library { items: (1..=500).map(|i| i.to_string()).collect(), ..lib("abcdefghjk", &"x".repeat(80)) }).is_ok());
        std::fs::write(&p, b"not json").unwrap();
        assert!(load_libraries(&p).is_empty(), "a broken file reads as empty");
        // One malformed entry does not take the others with it.
        std::fs::write(&p, r#"[{"id":"bad"},{"id":"cdefghjkmn","appid":"440","name":"ok","items":[],"created":1,"updated":2}]"#).unwrap();
        assert_eq!(load_libraries(&p).len(), 1);
    }

    #[test]
    fn libraries_with_refs() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("libraries.json");
        // Refs from every source next to bare Workshop ids, and a game that is not on Steam.
        let items: Vec<String> = ["3012345678", "ts:BepInEx-BepInExPack", "nx:skyrimspecialedition:12604", "cf:238222", "mio:1:2", "gb:Mod:3", "mr:AANobbMI", "ws:42"]
            .map(String::from)
            .to_vec();
        let l = Library { appid: None, game: Some("lethalcompany".into()), items: items.clone(), added_by_us: vec!["ts:BepInEx-BepInExPack".into()], ..lib("abcdefghjk", "Mixed") };
        let list = save_library(&p, l).unwrap();
        assert_eq!(list[0].items, items);
        assert_eq!(load_libraries(&p), list);
        let v = serde_json::to_value(&list[0]).unwrap();
        assert_eq!(v["game"], "lethalcompany");
        assert!(v.get("appid").is_none(), "absent, not null");
        // Both set, and an empty appid sent by the UI reads as absent.
        assert!(save_library(&p, Library { game: Some("tf2".into()), ..lib("bcdefghjkm", "Both") }).is_ok());
        let saved = save_library(&p, Library { appid: Some(String::new()), game: Some("valheim".into()), ..lib("cdefghjkmn", "Empty appid") }).unwrap();
        assert_eq!(saved[0].appid, None);
        // A library saved by an older app (appid only, Workshop ids) still reads.
        std::fs::write(&p, r#"[{"id":"cdefghjkmn","appid":"440","name":"Old","items":["1","2"],"created":1,"updated":2}]"#).unwrap();
        assert_eq!(load_libraries(&p)[0].appid.as_deref(), Some("440"));
        assert!(valid_library_item("1") && valid_library_item("ts:a-b") && !valid_library_item("0") && !valid_library_item("abc"));
    }

    #[test]
    fn helper_answers() {
        assert_eq!(done_error(&serde_json::json!({"ev":"done","ok":true})), None);
        let e = done_error(&serde_json::json!({"ev":"done","ok":false,"error":"not_owned","message":"m"})).unwrap();
        assert_eq!(e, WorkshopError::new("not_owned", "m"));
        assert_eq!(done_error(&serde_json::json!({"ev":"done","ok":false,"error":"weird"})).unwrap().code, "failed");
        let s: ItemState = serde_json::from_value(serde_json::json!({"id":"1","subscribed":true,"installed":true,"downloading":false,"needsUpdate":false,"folder":"C:\\x","sizeBytes":5})).unwrap();
        assert_eq!(s.size_bytes, Some(5));
        let p: Progress = serde_json::from_value(serde_json::json!({"ev":"progress","id":"1","state":"downloading","done":1,"total":2})).unwrap();
        assert_eq!((p.state.as_str(), p.done, p.total), ("downloading", 1, 2));
    }
}
