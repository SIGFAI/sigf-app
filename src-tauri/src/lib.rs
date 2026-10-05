pub mod hosted;
pub mod install;
pub mod join;
pub mod launch;
pub mod privacy;
pub mod scan;

use std::collections::HashMap;
use std::sync::Mutex;
use tauri::{Emitter, Manager};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_opener::OpenerExt;

/// Store URI schemes the app may hand to Windows. Anything else is refused.
const LAUNCH_SCHEMES: &[&str] = &["steam://", "com.epicgames.launcher://", "uplay://", "goggalaxy://"];

/// One install/restore at a time: they share installed.json and may touch the same game folder.
static INSTALL_LOCK: Mutex<()> = Mutex::new(());

/// hosted.json is read and rewritten whole: one writer at a time.
static HOSTED_LOCK: Mutex<()> = Mutex::new(());

/// Invite links received (deep link, second instance) and not yet read by the UI. The UI drains it on start and on
/// every `link://open` event, so a link that arrives before the webview listens is never lost.
static PENDING_LINKS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Install folders found by the last scan: the only folders an install or join may treat as `{game}`.
static SCANNED_DIRS: Mutex<Vec<std::path::PathBuf>> = Mutex::new(Vec::new());

fn remember_scan(s: &scan::Scan) {
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
        check_game_dirs(&game_dirs)?;
        let engine = engine(detect_prism());
        engine.install(&recipe, &game_dirs, &mut |p| {
            let _ = app.emit("install://progress", &p);
        })
    })
    .await
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

/// Calls `/api/app/lobbies*` on sigf.ai: GET, POST or DELETE, an optional JSON body and host secret. Nothing else.
#[tauri::command]
async fn lobby_api(method: String, path: String, body: Option<String>, secret: Option<String>) -> Result<ApiAnswer, String> {
    if !lobby_path_ok(&path) {
        return Err(format!("refused lobby path: {path}"));
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

/// A lobby API path the UI may call: `/api/app/lobbies`, `/api/app/lobbies/hosting`, a lobby id (`[a-km-z2-9]{12}`, the
/// invite alphabet) with an optional `heartbeat` / `recipe` / `server` / `world`, or the list with a plain query. Checked
/// whole, before any URL is built, so no `..`, `%2e` or `%2f` can reach another sigf.ai path.
fn lobby_path_ok(path: &str) -> bool {
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

/// Invite links received since the last call (lobby ids, already checked).
#[tauri::command]
fn take_links() -> Vec<String> {
    std::mem::take(&mut *PENDING_LINKS.lock().unwrap_or_else(|p| p.into_inner()))
}

fn receive_links(app: &tauri::AppHandle, urls: impl IntoIterator<Item = String>) {
    let ids: Vec<String> = urls.into_iter().filter_map(|u| join::parse_link(&u)).collect();
    if ids.is_empty() {
        return;
    }
    {
        let mut pending = PENDING_LINKS.lock().unwrap_or_else(|p| p.into_inner());
        for id in ids {
            if !pending.contains(&id) {
                pending.push(id);
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
    joined
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
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

/// Starts an installed mashup, in the recipe's launch order. Minecraft sides go through Prism (`--launch <instance>`)
/// and start first, so a passthrough bridge is listening before the host game boots; a game with a launch `exe` (script
/// extender loader) is started from its folder, Steam first when it is a Steam game; any other through its store. A
/// game whose launch step has `wait: "port:<n>"` holds the next one until that local port answers.
/// With a join: Prism also gets `--server`, a connect engine `+connect`, any other game `{app}/sigf-join.json`.
/// Blocking (port and Steam waits): call it off the main thread.
fn play_mod(app: &tauri::AppHandle, id: &str, stores: &Stores, join: Option<JoinPlan>) -> Result<(), String> {
    let m = install::Engine::from_env(None)
        .installed()
        .into_iter()
        .find(|m| m.id == id)
        .ok_or_else(|| format!("{id} is not installed"))?;
    let mut games = m.games.clone();
    games.sort_by_key(|g| g.strategy != install::Strategy::Mrpack);
    for (i, g) in games.iter().enumerate() {
        start_game(app, id, g, stores, join.as_ref())?;
        if let (Some(w), true) = (g.wait.as_deref(), i + 1 < games.len()) {
            let port = launch::parse_wait(w).ok_or_else(|| format!("bad launch wait for {}: {w}", g.game))?;
            launch::wait_port(port, launch::PORT_TIMEOUT, launch::POLL).map_err(|e| format!("{}: {e}", g.game))?;
        }
    }
    Ok(())
}

fn start_game(app: &tauri::AppHandle, id: &str, g: &install::InstalledGame, stores: &Stores, join: Option<&JoinPlan>) -> Result<(), String> {
    let addr = join.and_then(|j| j.targets.get(&g.game)).map(String::as_str);
    if let Some(a) = addr {
        if !join::valid_address(a) {
            return Err(format!("refused join address for {}", g.game));
        }
    }
    let kind = join::join_kind(g.strategy, &g.game);
    if g.strategy == install::Strategy::Mrpack {
        let prism = detect_prism().and_then(|p| p.exe).ok_or("Prism Launcher not found")?;
        let inst = g.instance.clone().ok_or("instance missing from the install record")?;
        std::process::Command::new(prism).args(join::prism_args(&inst, addr)).spawn().map_err(|e| e.to_string())?;
        return Ok(());
    }
    if let (join::JoinKind::Mod, Some(a), Some(j)) = (kind, addr, join) {
        let dir = install::home_dir().join("profiles").join(install::paths::slug(id)).join(install::paths::slug(&g.game));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::write(dir.join("sigf-join.json"), join::join_file(j.lobby, a, j.host)).map_err(|e| e.to_string())?;
    }
    let store = stores.get(&g.game);
    let args = join::store_join_args(kind, &g.launch_args, addr);
    if let Some(x) = &g.exe {
        let exe = launch::resolve_exe(x)?;
        if store.is_some_and(|(s, _, _)| s == "steam") {
            launch::ensure_steam(&DesktopSteam(app), launch::STEAM_TIMEOUT, launch::POLL)?;
        }
        std::process::Command::new(&exe)
            .args(&args)
            .current_dir(&x.dir)
            .spawn()
            .map_err(|e| format!("{}: {e}", exe.display()))?;
        return Ok(());
    }
    let Some((store, store_id, uri)) = store else { return Ok(()) };
    if store == "steam" && !(!store_id.is_empty() && store_id.len() <= 10 && store_id.bytes().all(|b| b.is_ascii_digit())) {
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
async fn play(app: tauri::AppHandle, id: String, stores: Stores) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || play_mod(&app, &id, &stores, None)).await.map_err(|e| e.to_string())?
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
        if recipe.id != info.mashup.id || recipe.version != info.mashup.version {
            return Err(join::JoinError::new("badLobby", "the lobby's recipe is not its pinned version"));
        }
        let a = app.clone();
        blocking(move || {
            check_game_dirs(&game_dirs)?;
            engine(detect_prism()).install(&recipe, &game_dirs, &mut |p| {
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
    .map_err(|e| join::JoinError::new("launch", e))?;
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
            scan_games, steam_lookup, launch, install, restore, installed, fetch_text, play, join_lobby, lobby_api, lan_address,
            take_links, minecraft_players, hosted_list, hosted_save, hosted_forget, privacy_get, privacy_set
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
            assert!(lobby_path_ok(ok), "{ok}");
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
            assert!(!lobby_path_ok(bad), "{bad}");
        }
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
