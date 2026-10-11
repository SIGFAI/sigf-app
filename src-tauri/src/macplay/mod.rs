//! Play on Mac (docs/MAC-PLAY.md, engines in docs/MAC-ENGINES.md): Windows games in their own Wine prefix (Wine +
//! DXMT, Windows Steam inside), native ports fed with the player's data files, and native Mac builds started through
//! Steam. The real work runs on macOS only; elsewhere every command answers `unsupported`.
//!
//! Layout under `~/Library/Application Support/ai.sigf.app/macplay/` (`SIGF_MACPLAY_HOME` overrides it): `engines/`,
//! `prefixes/<game>/` (each a WINEPREFIX), `ports/<game>/`, `logs/<game>.log`, `recipes/<game>.json` (the last
//! checked recipe, for the commands that only take a game id) and `downloads/` (archives until unpacked).

pub mod engine;
pub mod port;
pub mod recipe;
pub mod wine;

use recipe::{Checked, DataFrom, Route};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::Mutex;
use tauri::Emitter;

/// A command's error for the UI: `unsupported`, `badRecipe`, `download`, `sha`, `io`, `unpack`, `wine`, `timeout`,
/// `notReady`, `notSetUp`, `dataMissing`, `macosTooOld`, `inUse`, `launch`, `steam_missing` (a native game and no
/// Steam for Mac).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MacError {
    pub code: String,
    pub message: String,
}

impl MacError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into() }
    }
    pub fn io(path: &Path, e: impl std::fmt::Display) -> Self {
        Self::new("io", format!("{}: {e}", path.display()))
    }
    pub fn from_install(e: crate::install::InstallError) -> Self {
        use crate::install::InstallError as E;
        let code = match &e {
            E::ShaMismatch { .. } => "sha",
            E::Download { .. } => "download",
            E::Recipe { .. } => "badRecipe",
            _ => "io",
        };
        Self::new(code, e.to_string())
    }
    fn unsupported() -> Self {
        Self::new("unsupported", "Play on Mac runs on macOS only")
    }
}

impl std::fmt::Display for MacError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

/// The work below runs on macOS only.
pub fn require_mac() -> Result<(), MacError> {
    if cfg!(target_os = "macos") {
        Ok(())
    } else {
        Err(MacError::unsupported())
    }
}

#[derive(Debug, Clone)]
pub struct Layout {
    root: PathBuf,
}

impl Layout {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
    /// `SIGF_MACPLAY_HOME`, else `<user data>/ai.sigf.app/macplay`.
    pub fn from_env() -> Self {
        if let Some(h) = std::env::var_os("SIGF_MACPLAY_HOME").filter(|h| !h.is_empty()) {
            return Self::new(h.into());
        }
        Self::new(crate::install::user_data_dir().unwrap_or_else(std::env::temp_dir).join("ai.sigf.app").join("macplay"))
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn engines(&self) -> PathBuf {
        self.root.join("engines")
    }
    pub fn prefixes(&self) -> PathBuf {
        self.root.join("prefixes")
    }
    pub fn prefix(&self, game: &str) -> PathBuf {
        self.prefixes().join(game)
    }
    pub fn port(&self, game: &str) -> PathBuf {
        self.root.join("ports").join(game)
    }
    pub fn log(&self, game: &str) -> PathBuf {
        self.root.join("logs").join(format!("{game}.log"))
    }
    pub fn recipe(&self, game: &str) -> PathBuf {
        self.root.join("recipes").join(format!("{game}.json"))
    }
    pub fn downloads(&self) -> PathBuf {
        self.root.join("downloads")
    }
}

/// A log grows to this, then starts again.
const LOG_MAX: u64 = 20 * 1024 * 1024;

/// The game's log, opened for appending (restarted past `LOG_MAX`).
pub fn log_file(path: &Path) -> Result<std::fs::File, MacError> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).map_err(|e| MacError::io(p, e))?;
    }
    let big = std::fs::metadata(path).is_ok_and(|m| m.len() > LOG_MAX);
    std::fs::OpenOptions::new().create(true).append(!big).write(true).truncate(big).open(path).map_err(|e| MacError::io(path, e))
}

/// One `[sigf <unix time>] <msg>` line in the game's log (best effort).
pub fn log_line(path: &Path, msg: &str) {
    use std::io::Write;
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    if let Ok(mut f) = log_file(path) {
        let _ = writeln!(f, "[sigf {t}] {msg}");
    }
}

/// `macplay://progress`: `step` is `runtime`, `engine`, `renderer` (downloads: `done`/`total` in bytes), `unpack`, `prefix`,
/// `mono`, `vcrun2022`, `steam` (a download, then the install with 0/0), `data`, `ready`.
#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub game: String,
    pub step: String,
    pub done: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Disk {
    pub engine_bytes: u64,
    pub prefix_bytes: u64,
}

/// `mac_status`. A part a route does not have reads as done (`engine: ready` for `native`, `prefix: ready` for a port
/// without Windows Steam), so the UI walks the same steps for every route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MacStatus {
    pub route: Option<&'static str>,
    pub engine: &'static str,
    pub prefix: &'static str,
    pub store: &'static str,
    pub game: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<&'static str>,
    pub running: bool,
    pub disk: Disk,
}

/// Setups, removals and data copies: one at a time.
static MAC_LOCK: Mutex<()> = Mutex::new(());

/// Processes Play started, by game: `store` (Windows Steam), `game`.
static CHILDREN: Mutex<Vec<(String, &'static str, Child)>> = Mutex::new(Vec::new());

fn track(game: &str, kind: &'static str, child: Child) {
    CHILDREN.lock().unwrap_or_else(|p| p.into_inner()).push((game.to_string(), kind, child));
}

/// A tracked process of `game` (of `kind`, or any) still runs; ended ones are reaped.
fn alive(game: &str, kind: Option<&str>) -> bool {
    let mut c = CHILDREN.lock().unwrap_or_else(|p| p.into_inner());
    c.retain_mut(|(_, _, ch)| matches!(ch.try_wait(), Ok(None)));
    c.iter().any(|(g, k, _)| g == game && kind.is_none_or(|x| x == *k))
}

fn kill_tracked(game: &str) {
    let mut c = CHILDREN.lock().unwrap_or_else(|p| p.into_inner());
    for (_, _, ch) in c.iter_mut().filter(|(g, _, _)| g == game) {
        let _ = ch.kill();
        let _ = ch.wait();
    }
    c.retain(|(g, _, _)| g != game);
}

fn check_game(game: &str) -> Result<(), MacError> {
    if recipe::valid_game_id(game) {
        Ok(())
    } else {
        Err(MacError::new("badRecipe", format!("bad game id: {game}")))
    }
}

/// The recipe saved by the last setup (checked again: the file is on disk, not trusted more than the webview).
pub fn load_saved(layout: &Layout, game: &str) -> Result<Checked, MacError> {
    check_game(game)?;
    let s = std::fs::read_to_string(layout.recipe(game)).map_err(|_| MacError::new("notSetUp", format!("{game} is not set up on this Mac")))?;
    let c: Checked = serde_json::from_str(&s).map_err(|e| MacError::new("notSetUp", e.to_string()))?;
    let c = recipe::check_parsed(c.game, c.engines)?;
    if c.game.game != game {
        return Err(MacError::new("notSetUp", "saved recipe is for another game"));
    }
    Ok(c)
}

fn save(layout: &Layout, c: &Checked) -> Result<(), MacError> {
    let p = layout.recipe(&c.game.game);
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| MacError::io(d, e))?;
    }
    std::fs::write(&p, serde_json::to_string_pretty(c).unwrap_or_default()).map_err(|e| MacError::io(&p, e))
}

/// The recipe a command works on: the one the webview sent (checked, and for `game`), else the saved one.
fn resolve(layout: &Layout, game: &str, json: Option<&str>) -> Result<Checked, MacError> {
    match json {
        Some(j) => {
            let c = recipe::check(j)?;
            if c.game.game != game {
                return Err(MacError::new("badRecipe", format!("recipe is for {}, not {game}", c.game.game)));
            }
            Ok(c)
        }
        None => load_saved(layout, game),
    }
}

/// Bytes under `dir` (links not followed).
pub fn dir_size(dir: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let Ok(m) = e.path().symlink_metadata() else { continue };
            if m.is_dir() {
                stack.push(e.path());
            } else if m.is_file() {
                total += m.len();
            }
        }
    }
    total
}

/// This Mac's version (`sw_vers -productVersion`), as numbers.
fn macos_version() -> Option<Vec<u32>> {
    let out = std::process::Command::new("/usr/bin/sw_vers").arg("-productVersion").output().ok()?;
    parse_version(String::from_utf8_lossy(&out.stdout).trim())
}

fn parse_version(s: &str) -> Option<Vec<u32>> {
    s.split('.').map(|p| p.parse().ok()).collect()
}

/// `have` >= `need`, missing parts read as 0 (`26` = `26.0.0`).
fn version_at_least(have: &[u32], need: &[u32]) -> bool {
    let n = have.len().max(need.len());
    let at = |v: &[u32], i: usize| v.get(i).copied().unwrap_or(0);
    (0..n).map(|i| (at(have, i), at(need, i))).find(|(a, b)| a != b).is_none_or(|(a, b)| a > b)
}

/// Where a port's steam-win data is: the game folder in its own prefix's Steam, else in another prefix's.
fn steam_win_source(layout: &Layout, game: &str, appid: &str) -> Option<PathBuf> {
    let own = wine::steam_root(&layout.prefix(game));
    wine::steam_game_dir(&own, appid).or_else(|| wine::prefix_steams(layout).into_iter().find_map(|(_, root)| wine::steam_game_dir(&root, appid)))
}

/// A port's folders: its engine folder and the user's `~/Library` (where `{appsupport}` / `{prefs}` paths and markers
/// are).
fn port_dirs<'a>(layout: &Layout, c: &'a Checked) -> Option<(&'a recipe::PortBlock, port::Dirs)> {
    let p = c.game.port.as_ref()?;
    let e = c.engine(&p.engine)?;
    Some((p, port::Dirs::user(engine::engine_dir(layout, e, None))))
}

fn data_ready(layout: &Layout, c: &Checked) -> bool {
    port_dirs(layout, c).is_some_and(|(p, d)| port::data_ready(layout, &c.game.game, p, &d))
}

/// The prefix's Wine for a recipe with a `wine` block or a port's `via`, its engine (and runtime) ready, with the
/// recipe's env; `notReady` otherwise.
pub fn ready_wine(layout: &Layout, c: &Checked) -> Result<wine::Wine, MacError> {
    let renderer = c.wine_engines().is_some_and(|(_, r)| r.is_some());
    let wn = engine_wine(layout, c)?;
    if !wine::PrefixState::load(&wn.prefix).ready(renderer) {
        return Err(MacError::new("notReady", "the prefix is not prepared: run the setup"));
    }
    Ok(wn)
}

/// The recipe's Wine (engine and runtime ready, recipe env set), whatever state the prefix is in.
pub fn engine_wine(layout: &Layout, c: &Checked) -> Result<wine::Wine, MacError> {
    let (e, r) = c.wine_engines().ok_or_else(|| MacError::new("badRecipe", "this recipe has no Wine"))?;
    let dir = engine::engine_dir(layout, e, r);
    let ready = engine::ready(&dir, e, r).ok_or_else(|| MacError::new("notReady", "the engine is not downloaded: run the setup"))?;
    let libs = match c.runtime() {
        Some(rt) => vec![engine::runtime_lib(layout, rt).ok_or_else(|| MacError::new("notReady", "the engine's runtime is not downloaded: run the setup"))?],
        None => vec![],
    };
    let wn = wine::Wine::new(engine::wine_root(&dir, &ready), layout.prefix(&c.game.game), layout.log(&c.game.game)).with_libs(libs);
    Ok(match &c.game.wine {
        Some(w) => wn.with_recipe(w),
        None => wn,
    })
}

/// Mac Steam (the native client) has this app installed.
fn mac_steam_has(appid: &str) -> bool {
    crate::scan::steam::scan().unwrap_or_default().iter().any(|g| g.store_id == appid && g.prefix.is_none())
}

pub fn status(layout: &Layout, c: &Checked) -> MacStatus {
    let game = c.game.game.as_str();
    let prefix = layout.prefix(game);
    let mut s = MacStatus {
        route: Some(c.game.route.as_str()),
        engine: "ready",
        prefix: "ready",
        store: "installed",
        game: "missing",
        data: None,
        running: alive(game, None),
        disk: Disk::default(),
    };
    if let (Some(appid), Some((e, r))) = (c.steam_appid(), c.wine_engines()) {
        let dir = engine::engine_dir(layout, e, r);
        let rt_ok = c.runtime().is_none_or(|rt| engine::runtime_lib(layout, rt).is_some());
        s.engine = if rt_ok && engine::ready(&dir, e, r).is_some() { "ready" } else { "missing" };
        s.disk.engine_bytes += dir_size(&dir);
        if let Some(rt) = c.runtime() {
            s.disk.engine_bytes += dir_size(&engine::engine_dir(layout, rt, None));
        }
        s.prefix = if wine::PrefixState::load(&prefix).ready(r.is_some()) { "ready" } else { "missing" };
        s.store = if alive(game, Some("store")) {
            "running"
        } else if wine::steam_exe(&prefix).is_file() {
            "installed"
        } else {
            "missing"
        };
        s.game = if wine::steam_game_dir(&wine::steam_root(&prefix), appid).is_some() { "installed" } else { "missing" };
        s.running |= wine::server_running(&prefix);
        s.disk.prefix_bytes += dir_size(&prefix);
    }
    match c.game.route {
        Route::Wine => {}
        Route::NativePort => {
            if let Some(p) = &c.game.port {
                if let Some(e) = c.engine(&p.engine) {
                    let dir = engine::engine_dir(layout, e, None);
                    if engine::ready(&dir, e, None).is_none() {
                        s.engine = "missing";
                    }
                    s.disk.engine_bytes += dir_size(&dir);
                }
                let ready = data_ready(layout, c);
                s.data = Some(if ready { "ready" } else { "missing" });
                if c.wine_engines().is_none() {
                    s.game = if ready { "installed" } else { "missing" };
                }
                s.disk.prefix_bytes += dir_size(&layout.port(game));
                if let Some((p, d)) = port_dirs(layout, c) {
                    s.disk.prefix_bytes += port::into_bytes(layout, game, p, &d);
                }
            }
        }
        Route::Native => {
            if let Some(n) = &c.game.native {
                s.store = if crate::scan::steam::steam_root().is_some() { "installed" } else { "missing" };
                s.game = if mac_steam_has(&n.steam_app_id) { "installed" } else { "missing" };
            }
        }
    }
    s
}

/// `mac_status` without a recipe at hand and none saved.
fn unknown_status() -> MacStatus {
    MacStatus { route: None, engine: "missing", prefix: "missing", store: "missing", game: "missing", data: None, running: false, disk: Disk::default() }
}

/// `mac_setup`: everything a route needs before Play, each step skipped when already done. `on(step, done, total)`.
pub fn setup(layout: &Layout, c: &Checked, on: &mut dyn FnMut(&str, u64, u64)) -> Result<MacStatus, MacError> {
    require_mac()?;
    let game = c.game.game.as_str();
    if let Some(min) = &c.game.min_macos {
        let need = parse_version(min).unwrap_or_default();
        if let Some(have) = macos_version() {
            if !version_at_least(&have, &need) {
                return Err(MacError::new("macosTooOld", format!("needs macOS {min} or later")));
            }
        }
    }
    let _g = MAC_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let log = layout.log(game);
    log_line(&log, &format!("setup {game} ({})", c.game.route.as_str()));
    save(layout, c)?;
    if let Some((e, r)) = c.wine_engines() {
        let libs = match c.runtime() {
            Some(rt) => vec![engine::ensure_runtime(layout, rt, &log, on)?],
            None => vec![],
        };
        let (dir, ready) = engine::ensure(layout, e, r, &log, on)?;
        let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        // The setup itself runs without the recipe's env and overrides: those are for Steam and the game.
        let wn = wine::Wine::new(engine::wine_root(&dir, &ready), layout.prefix(game), log.clone()).with_libs(libs);
        wine::setup_prefix(layout, &wn, &name, c.verbs(), r.is_some(), on)?;
    }
    if let Some(p) = &c.game.port {
        let e = c.engine(&p.engine).ok_or_else(|| MacError::new("badRecipe", "port engine missing"))?;
        engine::ensure(layout, e, None, &log, on)?;
        // steam-win data: copied as soon as Windows Steam has the game (mac_copy_data or the next setup otherwise).
        // gog-installer data: the engine imports it itself (VCMI's launcher). `from: none`: nothing to copy.
        copy_steam_data(layout, c, on)?;
    }
    on("ready", 0, 0);
    log_line(&log, "setup done");
    Ok(status(layout, c))
}

/// A steam-win port: copies the game folder from a prefix's Windows Steam into the data folder (`ports/<game>/data`
/// or `data.into`) when the game is there, then writes the config files; Ok(false) when it is not installed yet.
fn copy_steam_data(layout: &Layout, c: &Checked, on: &mut dyn FnMut(&str, u64, u64)) -> Result<bool, MacError> {
    let Some((p, dirs)) = port_dirs(layout, c) else { return Ok(false) };
    let (DataFrom::SteamWin, Some(appid)) = (p.data.from, p.data.appid.as_deref()) else { return Ok(false) };
    let Some(src) = steam_win_source(layout, &c.game.game, appid) else { return Ok(false) };
    on("data", 0, 0);
    port::copy_data(layout, &c.game.game, p, &dirs, &src)?;
    Ok(true)
}

/// `mac_copy_data`: a steam-win port's data, copied again from Windows Steam (after the player installed or updated
/// the game there).
pub fn copy_data(layout: &Layout, c: &Checked) -> Result<MacStatus, MacError> {
    require_mac()?;
    let _g = MAC_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    save(layout, c)?;
    if !copy_steam_data(layout, c, &mut |_, _, _| {})? {
        return Err(MacError::new("dataMissing", "the game is not installed in Windows Steam yet"));
    }
    Ok(status(layout, c))
}

/// `mac_store`: Windows Steam in the prefix, for the player to log in and install the game.
pub fn store(layout: &Layout, game: &str) -> Result<(), MacError> {
    require_mac()?;
    let c = load_saved(layout, game)?;
    let wn = ready_wine(layout, &c)?;
    let exe = wine::steam_exe(&wn.prefix);
    if !exe.is_file() {
        return Err(MacError::new("notReady", "Windows Steam is not installed: run the setup"));
    }
    if alive(game, Some("store")) {
        return Ok(());
    }
    let mut args: Vec<&std::ffi::OsStr> = vec![exe.as_os_str()];
    args.extend(wine::STEAM_FLAGS.iter().map(std::ffi::OsStr::new));
    track(game, "store", wn.spawn(&args, None)?);
    Ok(())
}

/// `mac_play`. Returns a store URI for the caller to open (`native`), else starts the game itself: Windows Steam's
/// `-applaunch` (or the exe directly when `direct` or no Steam), or the port's program.
pub fn play(layout: &Layout, c: &Checked, direct: bool) -> Result<Option<String>, MacError> {
    require_mac()?;
    let game = c.game.game.as_str();
    match c.game.route {
        Route::Native => {
            let n = c.game.native.as_ref().ok_or_else(|| MacError::new("badRecipe", "no native block"))?;
            if crate::scan::steam::steam_root().is_none() {
                return Err(MacError::new("steam_missing", "Steam for Mac is not installed"));
            }
            Ok(Some(format!("steam://rungameid/{}", n.steam_app_id)))
        }
        Route::Wine => {
            let w = c.game.wine.as_ref().ok_or_else(|| MacError::new("badRecipe", "no wine block"))?;
            let wn = ready_wine(layout, c)?;
            let dir = wine::steam_game_dir(&wine::steam_root(&wn.prefix), &w.appid)
                .ok_or_else(|| MacError::new("notReady", "the game is not installed in Windows Steam yet"))?;
            let steam = wine::steam_exe(&wn.prefix);
            let child = if !direct && steam.is_file() {
                let mut args: Vec<std::ffi::OsString> = vec![steam.into()];
                args.extend(wine::STEAM_FLAGS.iter().map(Into::into));
                args.push("-applaunch".into());
                args.push(w.appid.clone().into());
                args.extend(w.args.iter().map(Into::into));
                wn.spawn(&args.iter().map(|a| a.as_os_str()).collect::<Vec<_>>(), None)?
            } else {
                let exe = port::resolve_exe(&dir, &w.exe)?;
                let mut args: Vec<std::ffi::OsString> = vec![exe.clone().into()];
                args.extend(w.args.iter().map(Into::into));
                wn.spawn(&args.iter().map(|a| a.as_os_str()).collect::<Vec<_>>(), exe.parent())?
            };
            track(game, "game", child);
            Ok(None)
        }
        Route::NativePort => {
            let p = c.game.port.as_ref().ok_or_else(|| MacError::new("badRecipe", "no port block"))?;
            let e = c.engine(&p.engine).ok_or_else(|| MacError::new("badRecipe", "port engine missing"))?;
            let dir = engine::engine_dir(layout, e, None);
            if engine::ready(&dir, e, None).is_none() {
                return Err(MacError::new("notReady", "the engine is not downloaded: run the setup"));
            }
            // A gog-installer port starts without data: its launcher imports the installer itself.
            if !data_ready(layout, c) && p.data.from != DataFrom::GogInstaller {
                let _g = MAC_LOCK.lock().unwrap_or_else(|p| p.into_inner());
                if !copy_steam_data(layout, c, &mut |_, _, _| {})? {
                    return Err(MacError::new("dataMissing", "the game's data files are not copied yet"));
                }
            }
            let dirs = port::Dirs::user(dir);
            let data = port::data_target(layout, game, p, &dirs)?;
            port::write_config(p, &dirs, &data)?;
            let child = port::launch(&dirs, &data, p, &layout.log(game))?;
            track(game, "game", child);
            Ok(None)
        }
    }
}

/// `mac_stop`: the processes Play started, then `wineserver -k` for the prefix.
pub fn stop(layout: &Layout, game: &str) -> Result<(), MacError> {
    require_mac()?;
    check_game(game)?;
    kill_tracked(game);
    if let Ok(c) = load_saved(layout, game) {
        if c.wine_engines().is_some() {
            if let Ok(wn) = ready_wine(layout, &c) {
                wn.kill();
            }
        }
    }
    Ok(())
}

/// Engine folders a saved recipe uses.
fn engine_dirs(layout: &Layout, c: &Checked) -> Vec<PathBuf> {
    let mut out = vec![];
    if let Some((e, r)) = c.wine_engines() {
        out.push(engine::engine_dir(layout, e, r));
    }
    if let Some(rt) = c.runtime() {
        out.push(engine::engine_dir(layout, rt, None));
    }
    if let Some(e) = c.game.port.as_ref().and_then(|p| c.engine(&p.engine)) {
        out.push(engine::engine_dir(layout, e, None));
    }
    out
}

/// `mac_remove`: `prefix` (the prefix and port data: the Windows game goes with it), `engine` (refused while another
/// set-up game uses it), `all` (both, the saved recipe and the log).
pub fn remove(layout: &Layout, game: &str, what: &str) -> Result<(), MacError> {
    require_mac()?;
    check_game(game)?;
    if !["prefix", "engine", "all"].contains(&what) {
        return Err(MacError::new("badRecipe", format!("remove what? {what}")));
    }
    let saved = load_saved(layout, game).ok();
    let _ = stop(layout, game);
    let _g = MAC_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let rm = |p: &Path| match std::fs::remove_dir_all(p) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(MacError::io(p, e)),
        _ => Ok(()),
    };
    // What a port copied or wrote outside `ports/<game>` (a `data.into` folder, config under ~/Library), first: an
    // `{engine}` path is still resolvable before the engine goes.
    if what != "engine" {
        if let Some((p, d)) = saved.as_ref().and_then(|c| port_dirs(layout, c)) {
            let data = port::data_target(layout, game, p, &d).unwrap_or_else(|_| port::data_dir(layout, game));
            port::remove_config(p, &d, &data);
            port::remove_data(layout, game, p, &d);
        }
    }
    if what == "engine" || what == "all" {
        if let Some(c) = &saved {
            let mine = engine_dirs(layout, c);
            let others: Vec<PathBuf> = std::fs::read_dir(layout.root().join("recipes"))
                .into_iter()
                .flatten()
                .flatten()
                .filter_map(|e| e.path().file_stem().and_then(|s| s.to_str()).map(String::from))
                .filter(|g| g != game && (layout.prefix(g).is_dir() || layout.port(g).is_dir()))
                .filter_map(|g| load_saved(layout, &g).ok())
                .flat_map(|o| engine_dirs(layout, &o))
                .collect();
            if what == "engine" {
                if let Some(d) = mine.iter().find(|d| others.contains(d)) {
                    return Err(MacError::new("inUse", format!("{} is used by another game", d.display())));
                }
            }
            for d in mine.iter().filter(|d| !others.contains(d)) {
                rm(d)?;
            }
        }
    }
    if what == "prefix" || what == "all" {
        rm(&layout.prefix(game))?;
        rm(&layout.port(game))?;
    }
    if what == "all" {
        let _ = std::fs::remove_file(layout.recipe(game));
        let _ = std::fs::remove_file(layout.log(game));
    }
    Ok(())
}

/// Windows games installed in the prefixes' Steam, for the scan (macOS only): store `steam`, `prefix` set.
pub fn scan_prefix_games() -> Vec<crate::scan::Game> {
    if !cfg!(target_os = "macos") {
        return vec![];
    }
    prefix_games(&Layout::from_env())
}

pub fn prefix_games(layout: &Layout) -> Vec<crate::scan::Game> {
    wine::prefix_steams(layout).into_iter().flat_map(|(game, root)| crate::scan::steam::scan_root(&root, Some(&game))).collect()
}

// ---- Tauri commands -------------------------------------------------------------------------------------------

async fn off_thread<T: Send + 'static>(f: impl FnOnce() -> Result<T, MacError> + Send + 'static) -> Result<T, MacError> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| MacError::new("io", e.to_string()))?
}

#[tauri::command]
pub async fn mac_status(game: String, game_json: Option<String>) -> Result<MacStatus, MacError> {
    require_mac()?;
    off_thread(move || {
        let layout = Layout::from_env();
        match resolve(&layout, &game, game_json.as_deref()) {
            Ok(c) => Ok(status(&layout, &c)),
            Err(e) if e.code == "notSetUp" => Ok(unknown_status()),
            Err(e) => Err(e),
        }
    })
    .await
}

#[tauri::command]
pub async fn mac_setup(app: tauri::AppHandle, game_json: String) -> Result<MacStatus, MacError> {
    require_mac()?;
    off_thread(move || {
        let c = recipe::check(&game_json)?;
        let game = c.game.game.clone();
        let mut last = (String::new(), 0u64);
        setup(&Layout::from_env(), &c, &mut |step, done, total| {
            // Byte counts arrive per read: send one event per step and per MiB.
            if last.0 == step && done.saturating_sub(last.1) < (1 << 20) && done != total {
                return;
            }
            last = (step.to_string(), done);
            let _ = app.emit("macplay://progress", Progress { game: game.clone(), step: step.into(), done, total });
        })
    })
    .await
}

#[tauri::command]
pub async fn mac_store(game: String) -> Result<(), MacError> {
    require_mac()?;
    off_thread(move || store(&Layout::from_env(), &game)).await
}

#[tauri::command]
pub async fn mac_play(app: tauri::AppHandle, game: String, game_json: Option<String>, direct: Option<bool>) -> Result<(), MacError> {
    require_mac()?;
    let uri = off_thread(move || {
        let layout = Layout::from_env();
        let c = resolve(&layout, &game, game_json.as_deref())?;
        play(&layout, &c, direct.unwrap_or(false))
    })
    .await?;
    if let Some(u) = uri {
        crate::open_store_uri(&app, u).map_err(|e| MacError::new("launch", e))?;
    }
    Ok(())
}

#[tauri::command]
pub async fn mac_copy_data(game_json: String) -> Result<MacStatus, MacError> {
    require_mac()?;
    off_thread(move || copy_data(&Layout::from_env(), &recipe::check(&game_json)?)).await
}

#[tauri::command]
pub async fn mac_stop(game: String) -> Result<(), MacError> {
    require_mac()?;
    off_thread(move || stop(&Layout::from_env(), &game)).await
}

#[tauri::command]
pub async fn mac_remove(game: String, what: String) -> Result<(), MacError> {
    require_mac()?;
    off_thread(move || remove(&Layout::from_env(), &game, &what)).await
}

/// A port whose data comes `from: folder`: the player picks the game's folder in a native dialog (the webview never
/// names a path), the listed files are copied from it.
#[tauri::command]
pub async fn mac_pick_data(window: tauri::WebviewWindow, game: String) -> Result<MacStatus, MacError> {
    require_mac()?;
    let layout = Layout::from_env();
    let c = load_saved(&layout, &game)?;
    let (p, dirs) = port_dirs(&layout, &c).map(|(p, d)| (p.clone(), d)).ok_or_else(|| MacError::new("badRecipe", "not a native port"))?;
    if p.data.from == DataFrom::None {
        return Err(MacError::new("badRecipe", "this port needs no game files"));
    }
    let Some(folder) = pick_folder(&window, &c.game.name).await else { return Ok(status(&layout, &c)) };
    off_thread(move || {
        let _g = MAC_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        port::copy_data(&layout, &game, &p, &dirs, &folder)?;
        Ok(status(&layout, &c))
    })
    .await
}

#[cfg(target_os = "macos")]
async fn pick_folder(window: &tauri::WebviewWindow, name: &str) -> Option<PathBuf> {
    let title = if name.is_empty() { "Pick the game's folder".to_string() } else { format!("Pick the folder of {name}") };
    rfd::AsyncFileDialog::new().set_title(title).set_parent(window).pick_folder().await.map(|f| f.path().to_path_buf())
}

#[cfg(not(target_os = "macos"))]
async fn pick_folder(_window: &tauri::WebviewWindow, _name: &str) -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        let v = |s: &str| parse_version(s).unwrap();
        assert!(version_at_least(&v("26.0"), &v("26.0")));
        assert!(version_at_least(&v("26.0.1"), &v("26")));
        assert!(version_at_least(&v("26.5"), &v("26.0")));
        assert!(!version_at_least(&v("15.7.2"), &v("26.0")));
        assert!(!version_at_least(&v("26"), &v("26.0.1")));
        assert!(parse_version("26.x").is_none());
    }

    #[test]
    fn layout_and_saved_recipes() {
        let t = tempfile::tempdir().unwrap();
        let layout = Layout::new(t.path().to_path_buf());
        assert_eq!(layout.prefix("skyrim"), t.path().join("prefixes/skyrim"));
        assert_eq!(load_saved(&layout, "skyrim").unwrap_err().code, "notSetUp");
        assert_eq!(load_saved(&layout, "../x").unwrap_err().code, "badRecipe");
        let json = serde_json::json!({ "game": recipe::tests::skyrim(), "engines": recipe::tests::engines() }).to_string();
        let c = recipe::check(&json).unwrap();
        save(&layout, &c).unwrap();
        assert_eq!(load_saved(&layout, "skyrim").unwrap(), c);
        assert_eq!(resolve(&layout, "skyrim", None).unwrap(), c);
        assert_eq!(resolve(&layout, "witcher3", Some(&json)).unwrap_err().code, "badRecipe");
        // A tampered file on disk is checked like the webview's.
        let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(layout.recipe("skyrim")).unwrap()).unwrap();
        v["game"]["wine"]["exe"] = serde_json::json!("../../x.exe");
        std::fs::write(layout.recipe("skyrim"), v.to_string()).unwrap();
        assert_eq!(load_saved(&layout, "skyrim").unwrap_err().code, "badRecipe");

        let s = status(&layout, &c);
        assert_eq!((s.route, s.engine, s.prefix, s.store, s.game, s.running), (Some("wine"), "missing", "missing", "missing", "missing", false));
    }

    #[test]
    fn prefix_games_are_steam_games_with_a_marker() {
        let t = tempfile::tempdir().unwrap();
        let layout = Layout::new(t.path().to_path_buf());
        let apps = wine::steam_root(&layout.prefix("skyrim")).join("steamapps");
        std::fs::create_dir_all(apps.join("common/Skyrim Special Edition")).unwrap();
        std::fs::write(
            apps.join("appmanifest_489830.acf"),
            "\"AppState\"\n{\n\"appid\" \"489830\"\n\"name\" \"The Elder Scrolls V: Skyrim Special Edition\"\n\"StateFlags\" \"4\"\n\"installdir\" \"Skyrim Special Edition\"\n\"buildid\" \"123\"\n}\n",
        )
        .unwrap();
        // A folder that is not a game id is ignored.
        std::fs::create_dir_all(wine::steam_root(&layout.prefixes().join("Bad Name")).join("steamapps")).unwrap();
        let games = prefix_games(&layout);
        assert_eq!(games.len(), 1);
        let g = &games[0];
        assert_eq!((g.store, g.store_id.as_str(), g.prefix.as_deref()), ("steam", "489830", Some("skyrim")));
        assert!(g.install_dir.as_deref().unwrap().ends_with("Skyrim Special Edition"));
        assert_eq!(g.launch, None, "started through mac_play, not steam://");
    }

    #[test]
    fn unsupported_off_macos() {
        if cfg!(target_os = "macos") {
            return;
        }
        let t = tempfile::tempdir().unwrap();
        let layout = Layout::new(t.path().to_path_buf());
        assert_eq!(stop(&layout, "skyrim").unwrap_err().code, "unsupported");
        assert_eq!(store(&layout, "skyrim").unwrap_err().code, "unsupported");
        assert!(scan_prefix_games().is_empty());
    }

    #[test]
    fn port_without_data_reads_ready() {
        let t = tempfile::tempdir().unwrap();
        let layout = Layout::new(t.path().to_path_buf());
        let g = serde_json::json!({
            "game": "openra", "name": "OpenRA", "route": "native-port",
            "port": { "engine": "vcmi-1.6", "data": { "from": "none" }, "launch": ["OpenRA.app/Contents/MacOS/OpenRA"] }
        });
        let c = recipe::check(&serde_json::json!({ "game": g, "engines": recipe::tests::engines() }).to_string()).unwrap();
        assert_eq!(c.engines.len(), 1);
        let s = status(&layout, &c);
        assert_eq!((s.route, s.engine, s.prefix, s.store, s.game, s.data), (Some("native-port"), "missing", "ready", "installed", "installed", Some("ready")));
        assert!(data_ready(&layout, &c));
    }

    #[test]
    fn dir_sizes() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("a/b")).unwrap();
        std::fs::write(t.path().join("a/b/x"), b"12345").unwrap();
        std::fs::write(t.path().join("y"), b"12").unwrap();
        assert_eq!(dir_size(t.path()), 7);
        assert_eq!(dir_size(&t.path().join("missing")), 0);
    }
}
