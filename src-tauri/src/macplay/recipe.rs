//! Mac recipes (docs/MAC-PLAY.md section 1): the `MacGame` and the `MacEngine`s it names, as the webview hands them
//! over from `GET /api/app/mac/games`. The webview is not trusted: everything is checked here before a byte is
//! downloaded or a process started. Ids become folder names, paths stay inside their folder, env keys and DLL
//! overrides keep to plain characters, engines come only from GitHub releases with a sha256 and a size.

use super::MacError;
use serde::{Deserialize, Serialize};

/// Largest engine archive (a Wine build is about 170 MB, a native port a few hundred).
pub const MAX_ENGINE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_ENV: usize = 32;
const MAX_ENV_VALUE: usize = 1024;
const MAX_ARGS: usize = 32;
const MAX_ARG: usize = 512;
const MAX_PATH: usize = 260;
const MAX_DATA_FILES: usize = 64;
const MAX_ENGINES: usize = 64;

/// Hosts an engine URL may name: GitHub release assets (`github.com/<owner>/<repo>/releases/download/...`, SIGFAI's
/// included) and the hosts GitHub serves them from.
pub const ENGINE_HOSTS: &[&str] = &["github.com", "objects.githubusercontent.com", "release-assets.githubusercontent.com"];

/// Env keys a recipe may not set: the core owns where Wine and its prefix are, and nothing may change how processes
/// load code (`DYLD_*`, `LD_*` are refused by prefix too).
const RESERVED_ENV: &[&str] = &[
    "WINEPREFIX", "WINESERVER", "WINELOADER", "WINEDLLPATH", "WINEARCH", "WINEDLLOVERRIDES", "PATH", "HOME", "TMPDIR", "USER", "SHELL",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineKind {
    Wine,
    Renderer,
    Native,
    /// Libraries a Wine build loads through `@rpath` (Sikarugir's Template `Contents/Frameworks`), named by the wine
    /// engine's `needs`.
    Runtime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MacEngine {
    pub id: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub kind: EngineKind,
    /// Display only (the recipe list's name and version); never used to find or run anything.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// A wine engine's runtime (an engine id of kind runtime): its libraries are put on `DYLD_FALLBACK_LIBRARY_PATH`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub needs: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Route {
    Wine,
    NativePort,
    Native,
}

impl Route {
    pub fn as_str(self) -> &'static str {
        match self {
            Route::Wine => "wine",
            Route::NativePort => "native-port",
            Route::Native => "native",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Store {
    SteamWin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Online {
    Ok,
    OfflineOnly,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verb {
    Vcrun2022,
    Mono,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WineBlock {
    pub engine: String,
    /// A renderer engine id (DXMT), or empty for Wine's own d3d.
    #[serde(default)]
    pub renderer: String,
    pub store: Store,
    pub appid: String,
    pub exe: String,
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub dll_overrides: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub verbs: Vec<Verb>,
    pub online: Online,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DataFrom {
    SteamWin,
    GogInstaller,
    Folder,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortData {
    pub from: DataFrom,
    #[serde(default)]
    pub appid: Option<String>,
    pub files: Vec<String>,
    /// The data counts as ready when one of these exists, relative to `~/Library/Application Support` (VCMI imports
    /// a GOG installer itself into `vcmi/`: `vcmi/Data/H3bitmap.lod`).
    #[serde(default)]
    pub markers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortBlock {
    pub engine: String,
    pub data: PortData,
    /// `launch[0]`: a file inside the engine folder; the rest: its arguments, where `{data}` is the copied data
    /// folder and `{engine}` the engine folder.
    pub launch: Vec<String>,
    /// Set at launch (same rule as `wine.env`); values may use `{data}` and `{engine}` too: GeneralsX reads its data
    /// from `CNC_GENERALS_ZH_PATH`.
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
    /// steam-win data without a `wine` block: the Wine engine (kind wine) that hosts Windows Steam in the prefix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeBlock {
    pub steam_app_id: String,
}

/// One Mac recipe. Display fields the core does not use (`name`, `notes`, `tested`) are not kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacGame {
    pub game: String,
    #[serde(default)]
    pub name: String,
    pub route: Route,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wine: Option<WineBlock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<PortBlock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native: Option<NativeBlock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_macos: Option<String>,
}

/// A checked recipe and the engines it uses (only those: the others in the list are dropped).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checked {
    pub game: MacGame,
    pub engines: Vec<MacEngine>,
}

impl Checked {
    pub fn engine(&self, id: &str) -> Option<&MacEngine> {
        self.engines.iter().find(|e| e.id == id)
    }

    /// The Wine engine and its renderer (None for an empty renderer), for a recipe with a `wine` block.
    pub fn wine_engines(&self) -> Option<(&MacEngine, Option<&MacEngine>)> {
        if let Some(w) = self.game.wine.as_ref() {
            let e = self.engine(&w.engine)?;
            let r = (!w.renderer.is_empty()).then(|| self.engine(&w.renderer)).flatten();
            return Some((e, r));
        }
        // A port's `via`: Wine with no renderer, only to host Windows Steam.
        let via = self.game.port.as_ref()?.via.as_deref()?;
        Some((self.engine(via)?, None))
    }

    /// The runtime the recipe's Wine engine needs, if any.
    pub fn runtime(&self) -> Option<&MacEngine> {
        let (e, _) = self.wine_engines()?;
        self.engine(e.needs.as_deref()?)
    }

    /// The Steam app id Windows Steam installs in the prefix (the game, or a port's data).
    pub fn steam_appid(&self) -> Option<&str> {
        match &self.game.wine {
            Some(w) => Some(w.appid.as_str()),
            None => self.game.port.as_ref().and_then(|p| p.data.appid.as_deref()),
        }
    }

    /// The prefix verbs (only a `wine` block has some).
    pub fn verbs(&self) -> &[Verb] {
        self.game.wine.as_ref().map(|w| w.verbs.as_slice()).unwrap_or(&[])
    }
}

fn bad(m: impl Into<String>) -> MacError {
    MacError::new("badRecipe", m)
}

/// A game id (also a folder name): `[a-z0-9][a-z0-9_-]{0,63}`.
pub fn valid_game_id(s: &str) -> bool {
    let b = s.as_bytes();
    (1..=64).contains(&b.len()) && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit()) && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_' || *c == b'-')
}

/// An engine id (a folder name): `[a-z0-9][a-z0-9._-]{0,63}`, no `..`.
pub fn valid_engine_id(s: &str) -> bool {
    let b = s.as_bytes();
    (1..=64).contains(&b.len())
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"._-".contains(c))
        && !s.contains("..")
}

/// A path relative to its folder, `/`-separated: plain segments only (no `.`, `..`, empty, `\`, `:`, control
/// characters), not absolute, at most `MAX_PATH` bytes.
pub fn valid_rel_path(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_PATH
        && !s.starts_with('/')
        && !s.chars().any(|c| c.is_control() || c == '\\' || c == ':')
        && s.trim_end_matches('/').split('/').all(|seg| !seg.is_empty() && seg != "." && seg != ".." && seg.trim() == seg)
}

/// An env key: `[A-Z0-9_]+` starting with a letter, not one the core sets, not `DYLD_*` / `LD_*`.
pub fn valid_env_key(k: &str) -> bool {
    (1..=64).contains(&k.len())
        && k.as_bytes()[0].is_ascii_uppercase()
        && k.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        && !RESERVED_ENV.contains(&k)
        && !k.starts_with("DYLD_")
        && !k.starts_with("LD_")
}

/// An env value or a launch argument: no newline, NUL or other control character.
fn plain_text(s: &str, max: usize) -> bool {
    s.len() <= max && !s.chars().any(char::is_control)
}

/// `WINEDLLOVERRIDES`: DLL names, `,` `;` `=` and the load orders `n`, `b`, `d` (`dxgi,d3d11=b;nvapi64=`).
pub fn valid_dll_overrides(s: &str) -> bool {
    s.len() <= 512 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.,;=-".contains(&b))
}

fn valid_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// `13.0`, `26`, `26.0.1`.
fn valid_macos(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    (1..=3).contains(&parts.len()) && parts.iter().all(|p| (1..=3).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_digit()))
}

/// The archive types an engine of `kind` may be.
pub fn archive_ok(kind: EngineKind, url: &str) -> bool {
    let u = url.to_ascii_lowercase();
    let tar = [".tar.xz", ".tar.gz", ".tgz", ".zip"].iter().any(|e| u.ends_with(e));
    match kind {
        EngineKind::Wine | EngineKind::Renderer | EngineKind::Runtime => tar,
        EngineKind::Native => tar || u.ends_with(".dmg"),
    }
}

/// An engine URL: canonical https (no query, port, user, dot segments), on `ENGINE_HOSTS`, and on github.com a
/// release asset path (`/<owner>/<repo>/releases/download/<tag>/<file>`).
pub fn engine_url_ok(url: &str) -> bool {
    let Some(u) = crate::install::check::canonical_https(url) else { return false };
    let Some(host) = u.host_str() else { return false };
    if !ENGINE_HOSTS.contains(&host) {
        return false;
    }
    if host == "github.com" {
        let segs: Vec<&str> = u.path().trim_start_matches('/').split('/').collect();
        return segs.len() == 6 && segs[2] == "releases" && segs[3] == "download" && segs.iter().all(|s| !s.is_empty());
    }
    true
}

fn check_engine(e: &MacEngine) -> Result<(), MacError> {
    if !valid_engine_id(&e.id) {
        return Err(bad(format!("bad engine id: {}", e.id)));
    }
    if !engine_url_ok(&e.url) {
        return Err(bad(format!("engine {}: only GitHub release downloads are allowed: {}", e.id, e.url)));
    }
    if !archive_ok(e.kind, &e.url) {
        return Err(bad(format!("engine {}: unsupported archive type", e.id)));
    }
    if !valid_sha256(&e.sha256) {
        return Err(bad(format!("engine {}: sha256 must be 64 lowercase hex characters", e.id)));
    }
    if e.size == 0 || e.size > MAX_ENGINE_BYTES {
        return Err(bad(format!("engine {}: size must be 1..={MAX_ENGINE_BYTES} bytes", e.id)));
    }
    Ok(())
}

fn check_args(what: &str, args: &[String]) -> Result<(), MacError> {
    if args.len() > MAX_ARGS {
        return Err(bad(format!("{what}: at most {MAX_ARGS} arguments")));
    }
    match args.iter().find(|a| !plain_text(a, MAX_ARG)) {
        Some(a) => Err(bad(format!("{what}: bad argument {a:?}"))),
        None => Ok(()),
    }
}

fn check_env(what: &str, env: &std::collections::BTreeMap<String, String>) -> Result<(), MacError> {
    if env.len() > MAX_ENV {
        return Err(bad(format!("{what}: at most {MAX_ENV} variables")));
    }
    for (k, v) in env {
        if !valid_env_key(k) {
            return Err(bad(format!("{what}: refused key {k}")));
        }
        if !plain_text(v, MAX_ENV_VALUE) {
            return Err(bad(format!("{what}: bad value for {k}")));
        }
    }
    Ok(())
}

fn check_wine(w: &WineBlock, engines: &[MacEngine]) -> Result<(), MacError> {
    let kind_of = |id: &str| engines.iter().find(|e| e.id == id).map(|e| e.kind);
    if kind_of(&w.engine) != Some(EngineKind::Wine) {
        return Err(bad(format!("wine.engine {} is not a wine engine of the list", w.engine)));
    }
    if !w.renderer.is_empty() && kind_of(&w.renderer) != Some(EngineKind::Renderer) {
        return Err(bad(format!("wine.renderer {} is not a renderer engine of the list", w.renderer)));
    }
    if !crate::workshop::valid_appid(&w.appid) {
        return Err(bad(format!("bad Steam app id: {}", w.appid)));
    }
    if !valid_rel_path(&w.exe) || !w.exe.to_ascii_lowercase().ends_with(".exe") {
        return Err(bad(format!("wine.exe must be a .exe path inside the game folder: {}", w.exe)));
    }
    check_env("wine.env", &w.env)?;
    if !valid_dll_overrides(&w.dll_overrides) {
        return Err(bad("wine.dllOverrides: only DLL names and , ; = - . _"));
    }
    check_args("wine.args", &w.args)?;
    let mut seen = vec![];
    for v in &w.verbs {
        if seen.contains(v) {
            return Err(bad(format!("wine.verbs: {v:?} twice")));
        }
        seen.push(*v);
    }
    Ok(())
}

fn check_port(p: &PortBlock, engines: &[MacEngine]) -> Result<(), MacError> {
    if engines.iter().find(|e| e.id == p.engine).map(|e| e.kind) != Some(EngineKind::Native) {
        return Err(bad(format!("port.engine {} is not a native engine of the list", p.engine)));
    }
    let d = &p.data;
    match (d.from, d.appid.as_deref()) {
        (DataFrom::SteamWin, Some(a)) if crate::workshop::valid_appid(a) => {}
        (DataFrom::SteamWin, _) => return Err(bad("port.data: steam-win needs a valid appid")),
        (_, Some(a)) if !crate::workshop::valid_appid(a) => return Err(bad(format!("port.data: bad appid {a}"))),
        _ => {}
    }
    if d.files.is_empty() || d.files.len() > MAX_DATA_FILES {
        return Err(bad(format!("port.data.files: 1..={MAX_DATA_FILES} paths")));
    }
    // `["."]` alone: the whole game folder (GeneralsX needs all of it).
    let whole = d.files.len() == 1 && d.files[0] == ".";
    if let Some(f) = d.files.iter().find(|f| !whole && !valid_rel_path(f)) {
        return Err(bad(format!("port.data.files: bad path {f}")));
    }
    check_env("port.env", &p.env)?;
    if let Some(m) = p.data.markers.iter().find(|m| !valid_rel_path(m)) {
        return Err(bad(format!("port.data.markers: bad path {m}")));
    }
    if p.data.markers.len() > 8 {
        return Err(bad("port.data.markers: at most 8"));
    }
    if let Some(v) = &p.via {
        if engines.iter().find(|e| &e.id == v).map(|e| e.kind) != Some(EngineKind::Wine) {
            return Err(bad(format!("port.via {v} is not a wine engine of the list")));
        }
        if d.from != DataFrom::SteamWin {
            return Err(bad("port.via is only for data from steam-win"));
        }
    }
    let Some((exe, args)) = p.launch.split_first() else { return Err(bad("port.launch is empty")) };
    if !valid_rel_path(exe) {
        return Err(bad(format!("port.launch[0] must be a path inside the engine folder: {exe}")));
    }
    check_args("port.launch", args)
}

/// Parses and checks what the webview sent for `mac_setup` / `mac_play`: `{ "game": MacGame, "engines": MacEngine[] }`
/// (the API answer's engine list; the ones the game does not use are dropped), or a MacGame carrying its own
/// `engines` list.
pub fn check(json: &str) -> Result<Checked, MacError> {
    if json.len() > 256 * 1024 {
        return Err(bad("recipe too large"));
    }
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| bad(e.to_string()))?;
    let (game_v, engines_v) = if v.get("game").is_some_and(|g| g.is_object()) {
        (v["game"].clone(), v.get("engines").cloned())
    } else {
        let mut g = v.clone();
        let e = g.as_object_mut().and_then(|o| o.remove("engines"));
        (g, e)
    };
    let game: MacGame = serde_json::from_value(game_v).map_err(|e| bad(e.to_string()))?;
    let engines: Vec<MacEngine> = match engines_v {
        Some(e) => serde_json::from_value(e).map_err(|e| bad(format!("engines: {e}")))?,
        None => vec![],
    };
    check_parsed(game, engines)
}

pub fn check_parsed(game: MacGame, engines: Vec<MacEngine>) -> Result<Checked, MacError> {
    if !valid_game_id(&game.game) {
        return Err(bad(format!("bad game id: {}", game.game)));
    }
    if engines.len() > MAX_ENGINES {
        return Err(bad("too many engines"));
    }
    for (i, e) in engines.iter().enumerate() {
        check_engine(e)?;
        if let Some(n) = &e.needs {
            if e.kind != EngineKind::Wine || engines.iter().find(|r| &r.id == n).map(|r| r.kind) != Some(EngineKind::Runtime) {
                return Err(bad(format!("engine {}: needs {n} must be a runtime engine of the list, on a wine engine", e.id)));
            }
        }
        if engines[..i].iter().any(|o| o.id == e.id) {
            return Err(bad(format!("engine {} listed twice", e.id)));
        }
    }
    if let Some(m) = &game.min_macos {
        if !valid_macos(m) {
            return Err(bad(format!("bad minMacos: {m}")));
        }
    }
    match game.route {
        Route::Wine => {
            let w = game.wine.as_ref().ok_or_else(|| bad("route wine needs a wine block"))?;
            if game.port.is_some() || game.native.is_some() {
                return Err(bad("route wine takes only a wine block"));
            }
            check_wine(w, &engines)?;
        }
        Route::NativePort => {
            let p = game.port.as_ref().ok_or_else(|| bad("route native-port needs a port block"))?;
            if game.native.is_some() {
                return Err(bad("route native-port takes no native block"));
            }
            check_port(p, &engines)?;
            // A wine block (or `port.via`) on a port only hosts Windows Steam, to download the game's data files:
            // one of them is required for steam-win data, both refused otherwise.
            match (&game.wine, p.data.from) {
                (Some(_), DataFrom::SteamWin) if p.via.is_some() => return Err(bad("a native-port takes a wine block or port.via, not both")),
                (Some(w), DataFrom::SteamWin) => check_wine(w, &engines)?,
                (None, DataFrom::SteamWin) if p.via.is_none() => {
                    return Err(bad("a native-port with steam-win data needs port.via or a wine block (the Windows Steam that downloads it)"))
                }
                (Some(_), _) => return Err(bad("a native-port takes a wine block only for data from steam-win")),
                (None, _) => {}
            }
        }
        Route::Native => {
            let n = game.native.as_ref().ok_or_else(|| bad("route native needs a native block"))?;
            if game.wine.is_some() || game.port.is_some() {
                return Err(bad("route native takes only a native block"));
            }
            if !crate::workshop::valid_appid(&n.steam_app_id) {
                return Err(bad(format!("bad Steam app id: {}", n.steam_app_id)));
            }
        }
    }
    let used: Vec<&str> = [
        game.wine.as_ref().map(|w| w.engine.as_str()),
        game.wine.as_ref().map(|w| w.renderer.as_str()).filter(|r| !r.is_empty()),
        game.port.as_ref().map(|p| p.engine.as_str()),
        game.port.as_ref().and_then(|p| p.via.as_deref()),
    ]
    .into_iter()
    .flatten()
    .collect();
    let mut used: Vec<String> = used.into_iter().map(String::from).collect();
    // The runtime a used wine engine needs comes along.
    let needs: Vec<String> = engines.iter().filter(|e| used.contains(&e.id)).filter_map(|e| e.needs.clone()).collect();
    used.extend(needs);
    let engines = engines.into_iter().filter(|e| used.contains(&e.id)).collect();
    Ok(Checked { game, engines })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Every recipe sigf.ai serves (dumped by the server tests into this fixture) passes the core.
    #[test]
    fn served_recipes_pass() {
        let v: serde_json::Value = serde_json::from_str(include_str!("../../tests/fixtures/mac-games.json")).unwrap();
        for g in v["games"].as_array().unwrap() {
            let json = serde_json::json!({ "game": g, "engines": v["engines"] }).to_string();
            check(&json).unwrap_or_else(|e| panic!("{}: {e:?}", g["game"]));
        }
    }
    use serde_json::json;

    pub fn engines() -> serde_json::Value {
        json!([
            { "id": "wine-sikarugir-11.0-1", "kind": "wine", "size": 167322744,
              "url": "https://github.com/Sikarugir-App/Engines/releases/download/v1.0/WS12WineSikarugir11.0_1.tar.xz",
              "sha256": "67e29fb3d74f363af39c69ba11f9b13a79812c5db07cf4748672658e4a200a0e" },
            { "id": "dxmt-0.80", "kind": "renderer", "size": 18681669,
              "url": "https://github.com/3Shain/dxmt/releases/download/v0.80/dxmt-v0.80-builtin.tar.gz",
              "sha256": "8f260e36b5739e68f3bad613381441385c4dc7b85b78ba8de653d5a6a264529d" },
            { "id": "vcmi-1.6", "kind": "native", "size": 100,
              "url": "https://github.com/vcmi/vcmi/releases/download/1.6.8/VCMI-macOS-arm.dmg",
              "sha256": "0000000000000000000000000000000000000000000000000000000000000000" }
        ])
    }

    pub fn skyrim() -> serde_json::Value {
        json!({
            "game": "skyrim", "name": "Skyrim Special Edition", "route": "wine",
            "wine": {
                "engine": "wine-sikarugir-11.0-1", "renderer": "dxmt-0.80", "store": "steam-win", "appid": "489830",
                "exe": "SkyrimSE.exe", "env": { "WINEMSYNC": "1", "ROSETTA_ADVERTISE_AVX": "1" },
                "dllOverrides": "dxgi,d3d11,d3d10core,winemetal=b;nvapi64,nvngx=", "args": [], "verbs": ["vcrun2022"],
                "online": "offline-only"
            },
            "minMacos": "26.0", "notes": ["x"], "tested": []
        })
    }

    fn with(mut g: serde_json::Value, f: impl FnOnce(&mut serde_json::Value)) -> String {
        f(&mut g);
        json!({ "game": g, "engines": engines() }).to_string()
    }

    #[test]
    fn skyrim_passes_and_keeps_only_its_engines() {
        let c = check(&with(skyrim(), |_| {})).unwrap();
        assert_eq!(c.engines.len(), 2);
        let (w, r) = c.wine_engines().unwrap();
        assert_eq!(w.kind, EngineKind::Wine);
        assert_eq!(r.unwrap().id, "dxmt-0.80");
        // The flat form: a MacGame with its own engines.
        let mut flat = skyrim();
        flat["engines"] = engines();
        assert_eq!(check(&flat.to_string()).unwrap(), c);
    }

    #[test]
    fn refuses_bad_wine_blocks() {
        type Case = (&'static str, Box<dyn Fn(&mut serde_json::Value)>);
        let cases: Vec<Case> = vec![
            ("game id", Box::new(|g| g["game"] = json!("../x"))),
            ("game id case", Box::new(|g| g["game"] = json!("Skyrim"))),
            ("exe ..", Box::new(|g| g["wine"]["exe"] = json!("../../bin/sh.exe"))),
            ("exe abs", Box::new(|g| g["wine"]["exe"] = json!("/Applications/x.exe"))),
            ("exe drive", Box::new(|g| g["wine"]["exe"] = json!("C:/x.exe"))),
            ("exe backslash", Box::new(|g| g["wine"]["exe"] = json!("bin\\x.exe"))),
            ("exe not exe", Box::new(|g| g["wine"]["exe"] = json!("run.sh"))),
            ("env reserved", Box::new(|g| g["wine"]["env"] = json!({ "WINEPREFIX": "/tmp" }))),
            ("env dyld", Box::new(|g| g["wine"]["env"] = json!({ "DYLD_INSERT_LIBRARIES": "/tmp/x.dylib" }))),
            ("env lower", Box::new(|g| g["wine"]["env"] = json!({ "path": "x" }))),
            ("env charset", Box::new(|g| g["wine"]["env"] = json!({ "A-B": "x" }))),
            ("env newline", Box::new(|g| g["wine"]["env"] = json!({ "DXMT_X": "a\nb" }))),
            ("overrides", Box::new(|g| g["wine"]["dllOverrides"] = json!("dxgi=b $(rm -rf ~)"))),
            ("appid", Box::new(|g| g["wine"]["appid"] = json!("48a"))),
            ("store", Box::new(|g| g["wine"]["store"] = json!("epic"))),
            ("verb", Box::new(|g| g["wine"]["verbs"] = json!(["dotnet48"]))),
            ("verb twice", Box::new(|g| g["wine"]["verbs"] = json!(["mono", "mono"]))),
            ("arg nul", Box::new(|g| g["wine"]["args"] = json!(["a\u{0}b"]))),
            ("unknown wine field", Box::new(|g| g["wine"]["shell"] = json!("sh -c x"))),
            ("engine not listed", Box::new(|g| g["wine"]["engine"] = json!("wine-other"))),
            ("engine wrong kind", Box::new(|g| g["wine"]["engine"] = json!("dxmt-0.80"))),
            ("renderer wrong kind", Box::new(|g| g["wine"]["renderer"] = json!("wine-sikarugir-11.0-1"))),
            ("route mismatch", Box::new(|g| g["native"] = json!({ "steamAppId": "1" }))),
            ("no wine block", Box::new(|g| g["wine"] = serde_json::Value::Null)),
            ("macos", Box::new(|g| g["minMacos"] = json!("26.x"))),
        ];
        for (what, f) in cases {
            let e = check(&with(skyrim(), f)).err().unwrap_or_else(|| panic!("{what} passed"));
            assert_eq!(e.code, "badRecipe", "{what}");
        }
    }

    #[test]
    fn refuses_bad_engines() {
        let bad_engine = |f: &dyn Fn(&mut serde_json::Value)| {
            let mut e = engines();
            f(&mut e[0]);
            check(&json!({ "game": skyrim(), "engines": e }).to_string()).is_err()
        };
        assert!(bad_engine(&|e| e["url"] = json!("https://evil.example/WS12WineSikarugir11.0_1.tar.xz")));
        assert!(bad_engine(&|e| e["url"] = json!("http://github.com/a/b/releases/download/v1/x.tar.xz")));
        assert!(bad_engine(&|e| e["url"] = json!("https://github.com/a/b/raw/main/x.tar.xz")));
        assert!(bad_engine(&|e| e["url"] = json!("https://github.com/a/b/releases/download/v1/../../x.tar.xz")));
        assert!(bad_engine(&|e| e["url"] = json!("https://github.com/a/b/releases/download/v1/x.tar.xz?x=1")));
        assert!(bad_engine(&|e| e["url"] = json!("https://github.com:444/a/b/releases/download/v1/x.tar.xz")));
        assert!(bad_engine(&|e| e["url"] = json!("https://github.com/a/b/releases/download/v1/x.dmg")), "dmg only for native");
        assert!(bad_engine(&|e| e["url"] = json!("https://github.com/a/b/releases/download/v1/x.exe")));
        assert!(bad_engine(&|e| e["sha256"] = json!("")));
        assert!(bad_engine(&|e| e["sha256"] = json!("67E29FB3D74F363AF39C69BA11F9B13A79812C5DB07CF4748672658E4A200A0E")));
        assert!(bad_engine(&|e| e["size"] = json!(0)));
        assert!(bad_engine(&|e| e["size"] = json!(MAX_ENGINE_BYTES + 1)));
        assert!(bad_engine(&|e| e["id"] = json!("../x")));
        assert!(bad_engine(&|e| e["id"] = json!("a..b")));
        assert!(bad_engine(&|e| e["kind"] = json!("script")));
        assert!(bad_engine(&|e| e["extra"] = json!(1)));
        assert!(bad_engine(&|e| e["id"] = json!("dxmt-0.80")), "duplicate id");
        assert!(!bad_engine(&|e| e["url"] = json!("https://release-assets.githubusercontent.com/github-production-release-asset/1/x.tar.xz")));
    }

    #[test]
    fn ports_and_native() {
        let port = json!({
            "game": "heroes3", "route": "native-port",
            "port": { "engine": "vcmi-1.6", "data": { "from": "steam-win", "appid": "297000", "files": ["Data", "Maps", "MP3/"] },
                      "launch": ["VCMI.app/Contents/MacOS/vcmilauncher", "--data", "{data}"] }
        });
        // steam-win data needs the Wine that hosts Windows Steam: port.via or a wine block.
        assert!(check(&with(port.clone(), |_| {})).is_err());
        let via = check(&with(port.clone(), |g| g["port"]["via"] = json!("wine-sikarugir-11.0-1"))).unwrap();
        assert_eq!(via.engines.len(), 2);
        assert_eq!(via.wine_engines().map(|(e, r)| (e.id.as_str(), r.is_none())), Some(("wine-sikarugir-11.0-1", true)));
        assert_eq!(via.steam_appid(), Some("297000"));
        assert!(check(&with(port.clone(), |g| g["port"]["via"] = json!("dxmt-0.80"))).is_err());
        assert!(check(&with(port.clone(), |g| {
            g["port"]["via"] = json!("wine-sikarugir-11.0-1");
            g["port"]["data"]["from"] = json!("folder");
        }))
        .is_err());
        let gog = |g: &mut serde_json::Value| {
            g["port"]["data"]["from"] = json!("gog-installer");
            g["port"]["data"]["appid"] = serde_json::Value::Null;
            g["port"]["data"]["markers"] = json!(["vcmi/Data/H3bitmap.lod"]);
        };
        assert!(check(&with(port.clone(), gog)).is_ok());
        assert!(check(&with(port.clone(), |g| {
            gog(g);
            g["port"]["data"]["markers"] = json!(["../../.ssh/id_rsa"]);
        }))
        .is_err());
        assert!(check(&with(port.clone(), |g| g["port"]["data"]["appid"] = serde_json::Value::Null)).is_err());
        assert!(check(&with(port.clone(), |g| g["port"]["data"]["files"] = json!(["../Library"]))).is_err());
        assert!(check(&with(port.clone(), |g| g["port"]["data"]["files"] = json!([]))).is_err());
        assert!(check(&with(port.clone(), |g| g["port"]["launch"] = json!(["/bin/sh", "-c", "x"]))).is_err());
        assert!(check(&with(port.clone(), |g| g["port"]["launch"] = json!([]))).is_err());
        assert!(check(&with(port.clone(), |g| g["port"]["engine"] = json!("wine-sikarugir-11.0-1"))).is_err());
        let folder = |g: &mut serde_json::Value| g["port"]["data"]["from"] = json!("folder");
        assert!(check(&with(port.clone(), folder)).is_ok());
        assert!(check(&with(port.clone(), |g| {
            folder(g);
            g["port"]["data"]["files"] = json!(["."]);
            g["port"]["env"] = json!({ "CNC_GENERALS_ZH_PATH": "{data}" });
        }))
        .is_ok());
        assert!(check(&with(port.clone(), |g| {
            folder(g);
            g["port"]["data"]["files"] = json!([".", "Data"]);
        }))
        .is_err());
        assert!(check(&with(port.clone(), |g| {
            folder(g);
            g["port"]["env"] = json!({ "DYLD_LIBRARY_PATH": "{data}" });
        }))
        .is_err());
        // A wine block on a port: only to host Windows Steam for steam-win data.
        let mut hosted = port.clone();
        hosted["wine"] = skyrim()["wine"].clone();
        hosted["wine"]["appid"] = json!("297000");
        hosted["wine"]["exe"] = json!("Heroes3.exe");
        hosted["wine"]["renderer"] = json!("");
        let c = check(&with(hosted.clone(), |_| {})).unwrap();
        assert_eq!(c.engines.len(), 2);
        assert!(check(&with(hosted, |g| g["port"]["data"]["from"] = json!("folder"))).is_err());

        let native = json!({ "game": "aoe2de", "route": "native", "native": { "steamAppId": "813780" } });
        let c = check(&with(native.clone(), |_| {})).unwrap();
        assert!(c.engines.is_empty());
        assert!(check(&with(native, |g| g["native"]["steamAppId"] = json!("x"))).is_err());
    }

    #[test]
    fn runtimes() {
        let mut e = engines();
        e.as_array_mut().unwrap().push(json!({ "id": "sikarugir-template-1.0.21", "kind": "runtime", "size": 95883080,
            "url": "https://github.com/Sikarugir-App/Wrapper/releases/download/v1.0/Template-1.0.21.tar.xz",
            "sha256": "bbe996e4e4375318485953d0c7818b7b4b0a4dc1f13303bcc584f99f7602f78d" }));
        e[0]["needs"] = json!("sikarugir-template-1.0.21");
        let c = check(&json!({ "game": skyrim(), "engines": e }).to_string()).unwrap();
        assert_eq!(c.engines.len(), 3);
        assert_eq!(c.runtime().map(|r| r.id.as_str()), Some("sikarugir-template-1.0.21"));
        // needs: only on a wine engine, only a runtime.
        let mut bad1 = e.clone();
        bad1[0]["needs"] = json!("dxmt-0.80");
        assert!(check(&json!({ "game": skyrim(), "engines": bad1 }).to_string()).is_err());
        let mut bad2 = e.clone();
        bad2[1]["needs"] = json!("sikarugir-template-1.0.21");
        assert!(check(&json!({ "game": skyrim(), "engines": bad2 }).to_string()).is_err());
        let mut bad3 = e.clone();
        bad3[0]["needs"] = json!("missing");
        assert!(check(&json!({ "game": skyrim(), "engines": bad3 }).to_string()).is_err());
        // A runtime is never a recipe's wine engine.
        let mut g = skyrim();
        g["wine"]["engine"] = json!("sikarugir-template-1.0.21");
        assert!(check(&json!({ "game": g, "engines": e }).to_string()).is_err());
    }

    #[test]
    fn path_rules() {
        for ok in ["SkyrimSE.exe", "bin/x64/witcher3.exe", "Data", "MP3/", "Heroes 3/Data"] {
            assert!(valid_rel_path(ok), "{ok}");
        }
        for bad in ["", "/x", "../x", "a/../x", "a/./x", "a//x", "C:x", "a\\x", "a\nb", " a", "a /b", &"a".repeat(300)] {
            assert!(!valid_rel_path(bad), "{bad:?}");
        }
        assert!(valid_game_id("skyrim") && valid_game_id("mirrorsedge-catalyst") && !valid_game_id("-x") && !valid_game_id(""));
        assert!(valid_engine_id("wine-sikarugir-11.0-1") && !valid_engine_id(".x") && !valid_engine_id("x/y"));
        assert!(valid_dll_overrides("dxgi,d3d11,d3d10core,winemetal=b;nvapi64,nvngx=") && valid_dll_overrides(""));
        assert!(!valid_dll_overrides("a b") && !valid_dll_overrides("a\"b"));
    }
}
