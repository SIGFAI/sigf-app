pub mod hosted;
pub mod install;
pub mod join;
pub mod launch;
pub mod privacy;
pub mod report;
pub mod scan;
pub mod update;
pub mod workshop;

use std::collections::HashMap;
use std::sync::Mutex;
use tauri::{Emitter, Manager};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_opener::OpenerExt;

/// Store URI schemes the app may hand to the system (Windows, macOS). Anything else is refused.
const LAUNCH_SCHEMES: &[&str] = &["steam://", "com.epicgames.launcher://", "uplay://", "goggalaxy://"];

/// One install/restore at a time: they share installed.json and may touch the same game folder.
static INSTALL_LOCK: Mutex<()> = Mutex::new(());

/// An install, restore or join install holds the lock right now (an update waits for it).
pub(crate) fn install_running() -> bool {
    matches!(INSTALL_LOCK.try_lock(), Err(std::sync::TryLockError::WouldBlock))
}

/// hosted.json is read and rewritten whole: one writer at a time.
static HOSTED_LOCK: Mutex<()> = Mutex::new(());

/// Links received (deep link, second instance) and not yet read by the UI: invite lobby ids, and library share links
/// in their `sigf://library/...` form. The UI drains it on start and on every `link://open` event, so a link that
/// arrives before the webview listens is never lost.
static PENDING_LINKS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// The player's own copies found or picked for a recipe (`own_copies`), by (recipe id, game): only the core ever holds
/// a path to them. Each is checked again by the engine before it is used, and never leaves the PC.
static OWN_PICKS: Mutex<Vec<(String, String, install::byo::OwnSource)>> = Mutex::new(Vec::new());

fn own_picks(recipe_id: &str) -> HashMap<String, install::byo::OwnSource> {
    OWN_PICKS.lock().unwrap_or_else(|p| p.into_inner()).iter().filter(|(r, _, _)| r == recipe_id).map(|(_, g, s)| (g.clone(), s.clone())).collect()
}

fn remember_pick(recipe_id: &str, game: &str, src: install::byo::OwnSource) {
    let mut picks = OWN_PICKS.lock().unwrap_or_else(|p| p.into_inner());
    picks.retain(|(r, g, _)| !(r == recipe_id && g == game));
    picks.push((recipe_id.to_string(), game.to_string(), src));
}

/// Install folders found by the last scan: the only folders an install or join may treat as `{game}`.
static SCANNED_DIRS: Mutex<Vec<std::path::PathBuf>> = Mutex::new(Vec::new());

fn remember_scan(s: &scan::Scan) {
    workshop::remember_scan(s);
    *SCANNED_DIRS.lock().unwrap_or_else(|p| p.into_inner()) = s.games.iter().filter_map(|g| g.install_dir.as_ref().map(Into::into)).collect();
}

/// Checks the `{game}` folders the UI sent against the core's own scan (scanning again once if one is unknown).
fn check_game_dirs(dirs: &HashMap<String, String>) -> Result<(), install::InstallError> {
    let known = SCANNED_DIRS.lock().unwrap_or_else(|p| p.into_inner()).clone();
    if install::check::game_dirs_ok(dirs, &known).is_ok() {
        return Ok(());
    }
    remember_scan(&scan::scan());
    let known = SCANNED_DIRS.lock().unwrap_or_else(|p| p.into_inner()).clone();
    install::check::game_dirs_ok(dirs, &known)
}

/// Dev builds only, with `SIGF_DEV_LOCAL_RECIPES=1`: recipes may name local files. A release build never allows it.
fn dev_local() -> bool {
    cfg!(debug_assertions) && install::check::dev_local_recipes()
}

/// The engine the commands use: `SIGF_HOME`, the scanned Prism, local files only in a dev build.
fn engine(prism: Option<install::Prism>) -> install::Engine {
    let mut e = install::Engine::from_env(prism);
    e.allow_local = dev_local();
    e
}

#[tauri::command]
async fn scan_games() -> Result<scan::Scan, String> {
    let s = tauri::async_runtime::spawn_blocking(scan::scan).await.map_err(|e| e.to_string())?;
    remember_scan(&s);
    Ok(s)
}

/// Steam art for a game its own store doesn't illustrate, matched by name (cached on disk).
#[tauri::command]
async fn steam_lookup(name: String) -> Option<scan::art::SteamArt> {
    // The search sends the game's name to Steam: only when the player allows it (docs/PRIVACY.md).
    if !privacy::art_search_ok(&privacy::current()) {
        return None;
    }
    tauri::async_runtime::spawn_blocking(move || scan::art::steam_lookup(&name)).await.ok().flatten()
}

#[tauri::command]
fn launch(app: tauri::AppHandle, uri: String) -> Result<(), String> {
    if !LAUNCH_SCHEMES.iter().any(|s| uri.starts_with(s)) {
        return Err(format!("refused launch uri: {uri}"));
    }
    app.opener().open_url(uri, None::<&str>).map_err(|e| e.to_string())
}

/// Prism as found by the Minecraft scan; only its data dir is required to write an instance.
fn detect_prism() -> Option<install::Prism> {
    let (launchers, _) = scan::minecraft::scan();
    launchers.into_iter().find(|l| l.kind == "prism").map(|l| install::Prism {
        data_dir: l.data_dir.into(),
        exe: l.exe.map(Into::into),
    })
}

/// Runs engine work off the async runtime: downloads are blocking and can take minutes.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, install::InstallError> + Send + 'static,
) -> Result<T, install::CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = INSTALL_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        f()
    })
    .await
    .map_err(|e| install::InstallError::Io { path: String::new(), message: e.to_string() })?
    .map_err(Into::into)
}

#[tauri::command]
async fn install(
    app: tauri::AppHandle,
    recipe_json: String,
    game_dirs: HashMap<String, String>,
) -> Result<install::InstalledMod, install::CommandError> {
    blocking(move || {
        // The webview is not trusted: the recipe is held to the catalog's whole rule, the folders to the scan.
        let recipe = install::check::check_recipe(&recipe_json, dev_local())?;
        install::platform::require_here(&recipe)?;
        check_game_dirs(&game_dirs)?;
        let mut engine = engine(detect_prism());
        engine.own = own_picks(&recipe.id);
        engine.install(&recipe, &game_dirs, &mut |p| {
            let _ = app.emit("install://progress", &p);
        })
    })
    .await
}

/// Bring your own copy, step 1: for each `own_copies` entry of the recipe, the copy already picked (still valid), else a
/// search of the usual folders on this PC (`byo::search`). Nothing is sent anywhere. Found copies are kept for `install`.
#[tauri::command]
async fn own_copies_find(recipe_json: String) -> Result<Vec<install::byo::Found>, install::CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let recipe = install::check::check_recipe(&recipe_json, dev_local())?;
        let known = own_picks(&recipe.id);
        let roots = install::byo::default_roots();
        let mut out = vec![];
        for c in &recipe.own_copies {
            if let Some(src) = known.get(&c.game).filter(|s| install::byo::verify(c, s).is_ok()) {
                out.push(install::byo::Found { game: c.game.clone(), label: c.label.clone(), found: Some(src.clone()), rejected: vec![] });
                continue;
            }
            let f = install::byo::search(c, &roots, install::byo::SearchLimits::default());
            if let Some(src) = &f.found {
                remember_pick(&recipe.id, &c.game, src.clone());
            }
            out.push(f);
        }
        Ok::<_, install::InstallError>(out)
    })
    .await
    .map_err(|e| install::InstallError::Io { path: String::new(), message: e.to_string() })?
    .map_err(Into::into)
}

/// Bring your own copy, step 2: the player picks the file in a native dialog opened by the core (the webview never
/// names a path). A `.zip` is searched for a matching entry. The pick is checked by SHA-1: a wrong dump is an
/// `ownCopyMismatch` error; `found: null` means the player closed the dialog.
#[tauri::command]
async fn own_copy_pick(window: tauri::WebviewWindow, recipe_json: String, game: String) -> Result<install::byo::Found, install::CommandError> {
    let recipe = install::check::check_recipe(&recipe_json, dev_local())?;
    let c = recipe.own_copies.iter().find(|c| c.game == game).cloned().ok_or_else(|| install::InstallError::recipe(format!("no own copy for {game}")))?;
    let picked = pick_file(&window, &c).await;
    tauri::async_runtime::spawn_blocking(move || {
        let mut out = install::byo::Found { game: c.game.clone(), label: c.label.clone(), found: None, rejected: vec![] };
        let Some(path) = picked else { return Ok::<_, install::InstallError>(out) };
        let is_zip = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip"));
        let src = if is_zip {
            // Search the archive the same way the folder search does.
            let f = install::byo::search_zip(&c, &path);
            match f {
                Some(s) => s,
                None => {
                    return Err(install::InstallError::OwnCopyMismatch {
                        game: c.game.clone(),
                        label: c.label.clone(),
                        file: path.to_string_lossy().into_owned(),
                        sha1: "no matching file inside".into(),
                    })
                }
            }
        } else {
            install::byo::OwnSource::file(path)
        };
        install::byo::verify(&c, &src)?;
        remember_pick(&recipe.id, &c.game, src.clone());
        out.found = Some(src);
        Ok::<_, install::InstallError>(out)
    })
    .await
    .map_err(|e| install::InstallError::Io { path: String::new(), message: e.to_string() })?
    .map_err(Into::into)
}

/// The native open-file dialog for one own copy, filtered on its extensions (and `.zip`), over the app's window.
#[cfg(any(windows, target_os = "macos"))]
async fn pick_file(window: &tauri::WebviewWindow, c: &install::recipe::OwnCopy) -> Option<std::path::PathBuf> {
    let exts: Vec<String> = c.rom.extensions.iter().map(|e| e.trim_start_matches('.').to_string()).chain(["zip".to_string()]).collect();
    let mut d = rfd::AsyncFileDialog::new().set_title(format!("Pick your own copy of {}", c.label)).add_filter(&c.label, &exts);
    if let Some(dl) = install::user_home().map(|h| h.join("Downloads")).filter(|d| d.is_dir()) {
        d = d.set_directory(dl);
    }
    d = d.set_parent(window);
    d.pick_file().await.map(|f| f.path().to_path_buf())
}

#[cfg(not(any(windows, target_os = "macos")))]
async fn pick_file(_window: &tauri::WebviewWindow, _c: &install::recipe::OwnCopy) -> Option<std::path::PathBuf> {
    None
}

#[tauri::command]
async fn restore(id: String, force: bool) -> Result<(), install::CommandError> {
    blocking(move || engine(detect_prism()).restore(&id, force)).await
}

#[tauri::command]
fn installed() -> Vec<install::InstalledMod> {
    install::Engine::from_env(None).installed()
}

/// The one host the webview may read through the core (its own origin can't: no CORS on sigf.ai).
const SITE_HOST: &str = "sigf.ai";

/// An https URL on sigf.ai itself: no user, no port, no other host.
fn site_url_ok(u: &reqwest::Url) -> bool {
    u.scheme() == "https" && u.host_str() == Some(SITE_HOST) && u.port().is_none() && u.username().is_empty() && u.password().is_none()
}

/// The client for sigf.ai: https only, and a redirect is followed only when it stays on sigf.ai.
fn http() -> Result<reqwest::blocking::Client, String> {
    let policy = reqwest::redirect::Policy::custom(|a| {
        if a.previous().len() >= 5 || !site_url_ok(a.url()) {
            a.error("redirect refused")
        } else {
            a.follow()
        }
    });
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent("sigf-app")
        .https_only(true)
        .redirect(policy)
        .build()
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn fetch_text(url: String) -> Result<String, String> {
    let Some(u) = reqwest::Url::parse(&url).ok().filter(site_url_ok) else {
        return Err(format!("refused fetch: {url}"));
    };
    if !privacy::site_fetch_ok(&privacy::current(), u.path()) {
        return Err("privacy: waiting for the first-start choices".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let r = http()?.get(&url).send().map_err(|e| e.to_string())?;
        if !r.status().is_success() {
            return Err(format!("HTTP {} for {url}", r.status()));
        }
        r.text().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The lobby API's answer: status and body, whatever the status (the UI reads `410`, `409`, `429` itself).
#[derive(serde::Serialize)]
struct ApiAnswer {
    status: u16,
    body: String,
}

/// Calls the sigf.ai app API: `/api/app/lobbies*` (GET, POST or DELETE, an optional JSON body and host secret) and
/// the Workshop proxy `/api/app/workshop/*` (GET only). Nothing else.
#[tauri::command]
async fn lobby_api(method: String, path: String, body: Option<String>, secret: Option<String>) -> Result<ApiAnswer, String> {
    if !lobby_path_ok(&method, &path) {
        return Err(format!("refused app API path: {method} {path}"));
    }
    if !privacy::lobby_call_ok(&privacy::current(), &path) {
        return Err("privacy: this lobby request is turned off".into());
    }
    let m = match method.as_str() {
        "GET" => reqwest::Method::GET,
        "POST" => reqwest::Method::POST,
        "DELETE" => reqwest::Method::DELETE,
        _ => return Err(format!("refused method: {method}")),
    };
    tauri::async_runtime::spawn_blocking(move || {
        let mut rq = http()?.request(m, format!("{}{path}", join::SITE));
        if let Some(b) = body {
            rq = rq.header("content-type", "application/json").body(b);
        }
        if let Some(s) = secret.filter(|s| !s.is_empty()) {
            rq = rq.header("authorization", format!("Bearer {s}"));
        }
        let r = rq.send().map_err(|e| e.to_string())?;
        let status = r.status().as_u16();
        Ok(ApiAnswer { status, body: r.text().map_err(|e| e.to_string())? })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A sigf.ai app API path the UI may call (lobbies and workshop): `/api/app/lobbies`, `/api/app/lobbies/hosting`, a lobby
/// id (`[a-km-z2-9]{12}`, the invite alphabet) with an optional `heartbeat` / `recipe` / `server` / `world`, or the list
/// with a plain query; with GET also `/api/app/workshop/browse` and `/items` with a plain query and
/// `/api/app/workshop/collection/<id>`. Checked whole, before any URL is built, so no `..`, `%2e` or `%2f` can reach
/// another sigf.ai path.
fn lobby_path_ok(method: &str, path: &str) -> bool {
    if let Some(rest) = path.strip_prefix("/api/app/workshop/") {
        return method == "GET" && workshop_path_ok(rest);
    }
    const BASE: &str = "/api/app/lobbies";
    let Some(rest) = path.strip_prefix(BASE) else { return false };
    if rest.is_empty() || rest == "/hosting" {
        return true;
    }
    if let Some(q) = rest.strip_prefix('?') {
        // The query cannot change the path; keep it to the characters URLSearchParams writes for ids and lists.
        return q.len() <= 2000 && q.bytes().all(|b| b.is_ascii_alphanumeric() || b"=&,._-%".contains(&b));
    }
    let Some(rest) = rest.strip_prefix('/') else { return false };
    let (id, tail) = rest.split_once('/').unwrap_or((rest, ""));
    join::parse_link(id).as_deref() == Some(id) && ["", "heartbeat", "recipe", "server", "world"].contains(&tail)
}

/// The part after `/api/app/workshop/`: `browse` or `items` with an optional query in the characters the UI's
/// URLSearchParams and id lists write, or `collection/<published file id>`.
fn workshop_path_ok(rest: &str) -> bool {
    if let Some(id) = rest.strip_prefix("collection/") {
        return workshop::valid_item_id(id);
    }
    let (route, query) = rest.split_once('?').unwrap_or((rest, ""));
    ["browse", "items"].contains(&route)
        && query.len() <= 2500
        && query.bytes().all(|b| b.is_ascii_alphanumeric() || b"=&%,*+/._-".contains(&b))
}

/// This PC's LAN address (the route to the internet's interface), for a host's default join address. No packet is
/// sent: connecting a UDP socket only picks the interface.
#[tauri::command]
fn lan_address() -> Option<String> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("1.1.1.1:80").ok()?;
    let ip = s.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then(|| ip.to_string())
}

/// Players on the host's own Minecraft world ([online, max]), for the lobby's live count; None when it does not answer.
#[tauri::command]
async fn minecraft_players(address: String) -> Option<(u32, u32)> {
    tauri::async_runtime::spawn_blocking(move || join::minecraft_players(&address)).await.ok().flatten()
}

/// The lobbies this app runs a free hosted server for, with their host secrets (world downloads for 7 days).
#[tauri::command]
fn hosted_list() -> Vec<hosted::HostedEntry> {
    hosted::load(&hosted::store_path())
}

#[tauri::command]
fn hosted_save(entry: hosted::HostedEntry) -> Result<Vec<hosted::HostedEntry>, String> {
    let _g = HOSTED_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    hosted::save(&hosted::store_path(), entry)
}

#[tauri::command]
fn hosted_forget(lobby: String) -> Result<Vec<hosted::HostedEntry>, String> {
    let _g = HOSTED_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    hosted::forget(&hosted::store_path(), &lobby)
}

/// The system this build runs on (`windows`, `macos`), for the catalog's `platforms` and the platform's own links.
#[tauri::command]
fn app_platform() -> &'static str {
    install::platform::current()
}

/// The privacy choices in force (docs/PRIVACY.md).
#[tauri::command]
fn privacy_get() -> privacy::Privacy {
    privacy::current()
}

/// Saves the player's privacy choices (the first-start screen sets `asked`).
#[tauri::command]
fn privacy_set(choices: privacy::Privacy) -> Result<privacy::Privacy, String> {
    privacy::set(choices)
}

/// "Report a bug": the scrubbed report and its GitHub link, for the player to review. Nothing is sent: the UI shows
/// it, and opens the link in the browser only when the player clicks (src/report.rs).
#[tauri::command]
async fn bug_report(app: tauri::AppHandle, input: report::ReportInput) -> Result<report::Report, String> {
    let version = app.package_info().version.to_string();
    tauri::async_runtime::spawn_blocking(move || report::make(&input, &version)).await.map_err(|e| e.to_string())
}

/// Links received since the last call, already checked: lobby ids, and library links (`sigf://library/...`).
#[tauri::command]
fn take_links() -> Vec<String> {
    std::mem::take(&mut *PENDING_LINKS.lock().unwrap_or_else(|p| p.into_inner()))
}

fn receive_links(app: &tauri::AppHandle, urls: impl IntoIterator<Item = String>) {
    let links: Vec<String> = urls.into_iter().filter_map(|u| join::parse_link(&u).or_else(|| workshop::parse_library_link(&u).map(|l| l.to_link()))).collect();
    if links.is_empty() {
        return;
    }
    {
        let mut pending = PENDING_LINKS.lock().unwrap_or_else(|p| p.into_inner());
        for link in links {
            if !pending.contains(&link) {
                pending.push(link);
            }
        }
    }
    let _ = app.emit("link://open", ());
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Percent-encodes launch args for `steam://run/<appid>//<args>/`.
fn steam_args(args: &[String]) -> String {
    let joined = args
        .iter()
        .map(|a| if a.contains(' ') { format!("\"{a}\"") } else { a.clone() })
        .collect::<Vec<_>>()
        .join(" ");
    report::encode(&joined)
}

/// `stores` maps canonical game id -> (store, store id, store launch uri) from the last scan.
type Stores = HashMap<String, (String, String, Option<String>)>;

/// A join for `play_mod`: the lobby and its address per game (each checked by `join::valid_address`).
struct JoinPlan<'a> {
    lobby: &'a str,
    host: &'a str,
    targets: &'a HashMap<String, String>,
}

/// Steam on this PC, for `launch::ensure_steam`: `steam.exe` in the process list, started through its URI.
struct DesktopSteam<'a>(&'a tauri::AppHandle);

impl launch::Steam for DesktopSteam<'_> {
    fn running(&self) -> bool {
        launch::steam_process_running()
    }
    fn start(&self) -> Result<(), String> {
        launch(self.0.clone(), "steam://open/main".into())
    }
}

/// How one installed game starts, worked out before anything is started: a missing loader, me3 or profile fails Play
/// while nothing runs yet (a passthrough never leaves a hidden Minecraft behind).
enum Start {
    Prism(std::path::PathBuf, String),
    /// The checked exe, the folder it starts in, and whether it is the mashup's own `app_exe` (no Steam wait).
    Exe { exe: std::path::PathBuf, dir: String, own: bool },
    Me3(launch::Me3Command),
    Store,
}

/// The player's me3 from its installer (`launch::me3_installed_path`), on Windows only.
fn me3_installed() -> Option<std::path::PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    install::user_data_dir().and_then(|d| launch::me3_installed_path(&d))
}

fn prepare(g: &install::InstalledGame) -> Result<Start, launch::PlayError> {
    if g.strategy == install::Strategy::Mrpack {
        let prism = detect_prism().and_then(|p| p.exe).ok_or("Prism Launcher not found".to_string())?;
        let inst = g.instance.clone().ok_or("instance missing from the install record".to_string())?;
        return Ok(Start::Prism(prism, inst));
    }
    if let Some(m) = &g.me3 {
        return Ok(Start::Me3(launch::me3_command(m, me3_installed().as_deref())?));
    }
    if let Some(x) = &g.exe {
        return Ok(Start::Exe { exe: launch::resolve_exe(x)?, dir: x.dir.clone(), own: x.own });
    }
    Ok(Start::Store)
}

/// Starts an installed mashup, in the recipe's launch order; a game its `launch[]` leaves out (`no_start`) is never
/// started (the mod starts it, hidden). First, with nothing started yet: the recipe's
/// `requires_files` (a missing prerequisite is a `missingFile` error with its page) and every game's loader, me3 and
/// profile. Then Minecraft sides go through Prism (`--launch <instance>`) and start first, so a passthrough bridge is
/// listening before the host game boots; a game with a launch `exe` or `app_exe` is started from its folder (Steam
/// first when it is a Steam game's loader), a `me3` one through me3 with its profile, offline; any other through its
/// store. A game whose launch step has `wait: "port:<n>"` holds the next one until that local port answers.
/// With a join: Prism also gets `--server`, a connect engine `+connect`, any other game `{app}/sigf-join.json`.
/// Blocking (port and Steam waits): call it off the main thread.
fn play_mod(app: &tauri::AppHandle, id: &str, stores: &Stores, join: Option<JoinPlan>) -> Result<(), launch::PlayError> {
    let m = install::Engine::from_env(None)
        .installed()
        .into_iter()
        .find(|m| m.id == id)
        .ok_or_else(|| format!("{id} is not installed"))?;
    launch::check_required(&m.requires_files)?;
    // A game the recipe's launch[] leaves out is started by the mod itself (or by another tool): never by Play.
    let mut games: Vec<install::InstalledGame> = m.games.iter().filter(|g| !g.no_start).cloned().collect();
    games.sort_by_key(|g| g.strategy != install::Strategy::Mrpack);
    let starts = games.iter().map(prepare).collect::<Result<Vec<_>, _>>()?;
    for (i, (g, s)) in games.iter().zip(starts).enumerate() {
        start_game(app, id, g, s, stores, join.as_ref())?;
        if let (Some(w), true) = (g.wait.as_deref(), i + 1 < games.len()) {
            let port = launch::parse_wait(w).ok_or_else(|| format!("bad launch wait for {}: {w}", g.game))?;
            launch::wait_port(port, launch::PORT_TIMEOUT, launch::POLL).map_err(|e| format!("{}: {e}", g.game))?;
        }
    }
    Ok(())
}

fn start_game(app: &tauri::AppHandle, id: &str, g: &install::InstalledGame, start: Start, stores: &Stores, join: Option<&JoinPlan>) -> Result<(), String> {
    let addr = join.and_then(|j| j.targets.get(&g.game)).map(String::as_str);
    if let Some(a) = addr {
        if !join::valid_address(a) {
            return Err(format!("refused join address for {}", g.game));
        }
    }
    let kind = join::join_kind(g.strategy, &g.game);
    if let Start::Prism(prism, inst) = &start {
        std::process::Command::new(prism).args(join::prism_args(inst, addr)).spawn().map_err(|e| e.to_string())?;
        return Ok(());
    }
    if let (join::JoinKind::Mod, Some(a), Some(j)) = (kind, addr, join) {
        let dir = install::home_dir().join("profiles").join(install::paths::slug(id)).join(install::paths::slug(&g.game));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::write(dir.join("sigf-join.json"), join::join_file(j.lobby, a, j.host)).map_err(|e| e.to_string())?;
    }
    let store = stores.get(&g.game);
    let args = join::store_join_args(kind, &g.launch_args, addr);
    // A Steam game's loader or me3 needs Steam up first; the mashup's own `app_exe` is not a game and never waits.
    let needs_steam = match &start {
        Start::Me3(_) => true,
        Start::Exe { own, .. } => !own,
        Start::Prism(..) | Start::Store => false,
    };
    if needs_steam && store.is_some_and(|(s, _, _)| s == "steam") {
        launch::ensure_steam(&DesktopSteam(app), launch::STEAM_TIMEOUT, launch::POLL)?;
    }
    match start {
        Start::Me3(c) => {
            std::process::Command::new(&c.exe).args(&c.args).current_dir(&c.cwd).spawn().map_err(|e| format!("{}: {e}", c.exe.display()))?;
            return Ok(());
        }
        Start::Exe { exe, dir, .. } => {
            std::process::Command::new(&exe)
                .args(&args)
                .current_dir(&dir)
                .spawn()
                .map_err(|e| format!("{}: {e}", exe.display()))?;
            return Ok(());
        }
        Start::Prism(..) | Start::Store => {}
    }
    let Some((store, store_id, uri)) = store else { return Ok(()) };
    if store == "steam" && !workshop::valid_appid(store_id) {
        return Err(format!("bad Steam app id for {}", g.game));
    }
    let target = if store == "steam" && !args.is_empty() {
        format!("steam://run/{store_id}//{}/", steam_args(&args))
    } else {
        uri.clone().ok_or_else(|| format!("no launch for {}", g.game))?
    };
    launch(app.clone(), target)
}

#[tauri::command]
async fn play(app: tauri::AppHandle, id: String, stores: Stores) -> Result<(), launch::PlayError> {
    tauri::async_runtime::spawn_blocking(move || play_mod(&app, &id, &stores, None)).await.map_err(|e| launch::PlayError::from(e.to_string()))?
}

/// Where a join is, for the UI's one button: `lobby` -> `install` (with `install://progress`) -> `launch` -> `done`.
#[derive(Clone, serde::Serialize)]
struct JoinProgress {
    lobby: String,
    step: &'static str,
}

/// Joins a lobby the player confirmed (`confirmed`, from the UI's join sheet): reads it, installs its pinned version when this PC has another one (or none) through the normal
/// engine, then launches with the join args. `game_dirs` and `stores` come from the last scan, as for install/play.
#[tauri::command]
async fn join_lobby(
    app: tauri::AppHandle,
    lobby: String,
    game_dirs: HashMap<String, String>,
    stores: Stores,
    confirmed: join::Confirmed,
) -> Result<join::Lobby, join::JoinError> {
    let id = join::parse_link(&lobby).ok_or_else(|| join::JoinError::new("badLobby", "not an invite link"))?;
    if !privacy::current().asked {
        return Err(join::JoinError::new("privacy", "answer the privacy choices first"));
    }
    let step = |s: &'static str| {
        let _ = app.emit("join://progress", JoinProgress { lobby: id.clone(), step: s });
    };
    step("lobby");
    let get = |path: String| -> Result<(u16, String), join::JoinError> {
        let r = http().map_err(|e| join::JoinError::new("network", e))?.get(format!("{}{path}", join::SITE)).send().map_err(|e| join::JoinError::new("network", e.to_string()))?;
        let status = r.status().as_u16();
        Ok((status, r.text().map_err(|e| join::JoinError::new("network", e.to_string()))?))
    };
    let lid = id.clone();
    let (status, body) = tauri::async_runtime::spawn_blocking(move || get(format!("/api/app/lobbies/{lid}")))
        .await
        .map_err(|e| join::JoinError::new("network", e.to_string()))??;
    match status {
        200 => {}
        404 => return Err(join::JoinError::new("lobbyNotFound", "no such lobby")),
        410 => return Err(join::JoinError::new("lobbyClosed", "this lobby has ended")),
        s => return Err(join::JoinError::new("network", format!("lobby: HTTP {s}"))),
    }
    let info: join::Lobby = serde_json::from_str(&body).map_err(|e| join::JoinError::new("badLobby", e.to_string()))?;
    if info.state == "full" {
        return Err(join::JoinError::new("lobbyFull", "this lobby is full"));
    }
    if info.targets.is_empty() {
        return Err(join::JoinError::new("notReady", "the host has not opened the game yet"));
    }
    if let Some(t) = info.targets.iter().find(|t| !join::valid_address(&t.address)) {
        return Err(join::JoinError::new("badLobby", format!("bad address for {}", t.game)));
    }
    // Only what the player saw and accepted in the join sheet: same mashup, version and server addresses.
    if !confirmed.matches(&info) {
        return Err(join::JoinError::new("lobbyChanged", "The lobby changed since you confirmed it: check it again"));
    }

    // Version pinning: exactly the lobby's version, from the recipe kept with the lobby.
    let have = install::Engine::from_env(None).installed().into_iter().any(|m| m.id == info.mashup.id && m.version == info.mashup.version);
    if !have {
        step("install");
        let lid = id.clone();
        let (status, recipe_json) = tauri::async_runtime::spawn_blocking(move || get(format!("/api/app/lobbies/{lid}/recipe")))
            .await
            .map_err(|e| join::JoinError::new("network", e.to_string()))??;
        if status != 200 {
            return Err(join::JoinError::new(if status == 410 { "lobbyClosed" } else { "network" }, format!("recipe: HTTP {status}")));
        }
        let recipe = install::check::check_recipe(&recipe_json, false)?;
        install::platform::require_here(&recipe)?;
        if recipe.id != info.mashup.id || recipe.version != info.mashup.version {
            return Err(join::JoinError::new("badLobby", "the lobby's recipe is not its pinned version"));
        }
        let a = app.clone();
        blocking(move || {
            check_game_dirs(&game_dirs)?;
            // A mashup on the player's own copy: look for it on this PC if it was not picked yet (nothing is sent).
            for c in &recipe.own_copies {
                if !own_picks(&recipe.id).contains_key(&c.game) {
                    let roots = install::byo::default_roots();
                    if let Some(src) = install::byo::search(c, &roots, install::byo::SearchLimits::default()).found {
                        remember_pick(&recipe.id, &c.game, src);
                    }
                }
            }
            let mut engine = engine(detect_prism());
            engine.own = own_picks(&recipe.id);
            engine.install(&recipe, &game_dirs, &mut |p| {
                let _ = a.emit("install://progress", &p);
            })
        })
        .await
        .map_err(|e| join::JoinError::from(e.error))?;
    }

    step("launch");
    let targets: HashMap<String, String> = info.targets.iter().map(|t| (t.game.clone(), t.address.clone())).collect();
    let (a, mashup, lobby_id, host) = (app.clone(), info.mashup.id.clone(), id.clone(), info.host.clone());
    tauri::async_runtime::spawn_blocking(move || {
        play_mod(&a, &mashup, &stores, Some(JoinPlan { lobby: &lobby_id, host: &host, targets: &targets }))
    })
    .await
    .map_err(|e| join::JoinError::new("launch", e.to_string()))?
    .map_err(|e| join::JoinError::new(e.kind, e.to_string()))?;
    step("done");
    Ok(info)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // First: a second launch (a sigf:// click while the app runs) hands its arguments here and exits. With the
        // deep-link feature its link reaches on_open_url below; this callback only brings the window forward.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        // Driven from the core only (src/update.rs): the webview has no updater permission.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            // The config glob covers any Steam root; this adds the real one in case the glob misses it.
            if let Some(dir) = scan::steam::library_cache() {
                let _ = app.asset_protocol_scope().allow_directory(dir, true);
            }
            // Installed builds get the scheme from the NSIS installer (tauri.conf.json plugins.deep-link); a dev
            // build registers it for the current user at start.
            #[cfg(all(debug_assertions, windows))]
            let _ = app.deep_link().register_all();
            let handle = app.handle().clone();
            app.deep_link().on_open_url(move |event| receive_links(&handle, event.urls().into_iter().map(|u| u.to_string())));
            // Cold start from a link: it is in the start's own arguments.
            if let Ok(Some(urls)) = app.deep_link().get_current() {
                receive_links(app.handle(), urls.into_iter().map(|u| u.to_string()));
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            scan_games, steam_lookup, launch, install, own_copies_find, own_copy_pick, restore, installed, fetch_text, play, join_lobby, lobby_api, lan_address,
            take_links, minecraft_players, hosted_list, hosted_save, hosted_forget, privacy_get, privacy_set, app_platform, bug_report,
            update::update_check, update::update_blocked, update::update_install,
            workshop::workshop_subscribe, workshop::workshop_unsubscribe, workshop::workshop_state, workshop::libraries_list, workshop::libraries_save,
            workshop::libraries_delete
        ])
        .run(tauri::generate_context!())
        .expect("error while running the SIGF app");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lobby_paths() {
        for ok in [
            "/api/app/lobbies",
            "/api/app/lobbies/hosting",
            "/api/app/lobbies?games=tf2,minecraft",
            "/api/app/lobbies?mashup=sigf%2Fexample-mashup&games=minecraft",
            "/api/app/lobbies/k3m9xq2wa7fd",
            "/api/app/lobbies/k3m9xq2wa7fd/heartbeat",
            "/api/app/lobbies/k3m9xq2wa7fd/recipe",
            "/api/app/lobbies/k3m9xq2wa7fd/server",
            "/api/app/lobbies/k3m9xq2wa7fd/world",
        ] {
            assert!(lobby_path_ok("GET", ok), "{ok}");
        }
        for bad in [
            "/api/app/lobbies/%2e%2e/catalog",
            "/api/app/lobbies/%2E%2E/%2E%2E/admin",
            "/api/app/lobbies/../admin",
            "/api/app/lobbies/k3m9xq2wa7fd/../../admin",
            "/api/app/lobbies/k3m9xq2wa7fd%2f..%2fadmin",
            "/api/app/lobbies/k3m9xq2wa7fd/heartbeat/x",
            "/api/app/lobbies/k3m9xq2wa7fd/other",
            "/api/app/lobbies/K3M9XQ2WA7FD",
            "/api/app/lobbies/short",
            "/api/app/lobbiesx",
            "/api/app/lobbies/",
            "/api/app/lobbies?x=1#frag",
            "/api/app/lobbies?x=1/../admin",
            "/api/admin",
            "https://evil.example/api/app/lobbies",
            "//evil.example/api/app/lobbies",
        ] {
            assert!(!lobby_path_ok("GET", bad), "{bad}");
        }
    }

    #[test]
    fn workshop_paths() {
        for ok in [
            "/api/app/workshop/browse",
            "/api/app/workshop/browse?appid=440&sort=trend&q=red+hat%20x&cursor=AoJ4*%2Fx%2B%3D&tag=Maps",
            "/api/app/workshop/browse?appid=440&cursor=AoJ4/x+=",
            "/api/app/workshop/items?ids=3012345678,2987654321",
            "/api/app/workshop/collection/3012345678",
        ] {
            assert!(lobby_path_ok("GET", ok), "{ok}");
            assert!(!lobby_path_ok("POST", ok), "POST {ok}");
            assert!(!lobby_path_ok("DELETE", ok), "DELETE {ok}");
        }
        assert!(lobby_path_ok("GET", &format!("/api/app/workshop/items?ids={}", "1".repeat(2496))));
        for bad in [
            "/api/app/workshop",
            "/api/app/workshop/",
            "/api/app/workshop/other",
            "/api/app/workshop/browsex",
            "/api/app/workshop/browse/",
            "/api/app/workshop/browse/../../admin",
            "/api/app/workshop/%2e%2e/admin",
            "/api/app/workshop/browse?q=a b",
            "/api/app/workshop/browse?q=a#frag",
            "/api/app/workshop/browse?q=a?b",
            "/api/app/workshop/browse?q=<script>",
            "/api/app/workshop/items?ids=1;2",
            "/api/app/workshop/collection/",
            "/api/app/workshop/collection/0123",
            "/api/app/workshop/collection/12a",
            "/api/app/workshop/collection/1/2",
            "/api/app/workshop/collection/1?x=1",
            "/api/app/workshop/collection/../../admin",
            "/api/app/workshopx/browse",
        ] {
            assert!(!lobby_path_ok("GET", bad), "{bad}");
        }
        assert!(!lobby_path_ok("GET", &format!("/api/app/workshop/items?ids={}", "1".repeat(2501))));
    }

    #[test]
    fn fetch_text_hosts() {
        let ok = |s: &str| reqwest::Url::parse(s).is_ok_and(|u| site_url_ok(&u));
        assert!(ok("https://sigf.ai/api/app/catalog"));
        assert!(ok("https://sigf.ai/api/app/recipe/sigf%2Fx@1.0.0"));
        assert!(!ok("http://sigf.ai/api/app/catalog"));
        assert!(!ok("https://sigf.ai.evil.example/x"));
        assert!(!ok("https://sigf.ai@evil.example/x"));
        assert!(!ok("https://user@sigf.ai/x"));
        assert!(!ok("https://sigf.ai:8443/x"));
        assert!(!ok("https://github.com/SIGFAI/x"));
        assert!(!ok("https://api.modrinth.com/v2/project/x"));
    }
}
