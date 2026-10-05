//! Finds the games already installed on this PC, store by store. Read-only: nothing here writes to disk.

pub mod appinfo;
pub mod art;
mod epic;
mod gog;
pub mod minecraft;
pub mod steam;
mod ubisoft;
pub mod vdf;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    /// `<store>:<store id>`, stable across scans.
    pub key: String,
    pub store: &'static str,
    pub store_id: String,
    pub name: String,
    pub install_dir: Option<String>,
    /// Steam buildid, Epic AppVersionString, GOG ver: what mods match against.
    pub build: Option<String>,
    pub size_bytes: Option<u64>,
    /// Store URI that launches the game through its own launcher (DRM and overlay intact).
    pub launch: Option<String>,
    /// Portrait art (600x900) when the store has a public one.
    pub art: Option<String>,
    /// Wide art (460x215), fallback for `art`.
    pub art_wide: Option<String>,
    /// Steam's own cached copies (appcache/librarycache): newer apps only serve art under hashed CDN paths.
    pub art_local: Option<String>,
    pub hero_local: Option<String>,
    pub wide_local: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Launcher {
    /// `prism` | `modrinth` | `official`
    pub kind: &'static str,
    pub exe: Option<String>,
    pub data_dir: String,
    pub instances: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Scan {
    pub games: Vec<Game>,
    pub launchers: Vec<Launcher>,
    /// Stores found on this PC, even with no games.
    pub stores: Vec<&'static str>,
    pub millis: u128,
}

pub fn scan() -> Scan {
    let t = std::time::Instant::now();
    let mut out = Scan::default();
    for (store, found) in [
        ("steam", steam::scan()),
        ("epic", epic::scan()),
        ("ubisoft", ubisoft::scan()),
        ("gog", gog::scan()),
    ] {
        if let Some(games) = found {
            out.stores.push(store);
            out.games.extend(games);
        }
    }
    let (launchers, mc) = minecraft::scan();
    if let Some(g) = mc {
        out.stores.push("minecraft");
        out.games.push(g);
    }
    out.launchers = launchers;
    let mut seen = std::collections::HashSet::new();
    out.games.retain(|g| seen.insert(g.key.clone()));
    out.games.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out.millis = t.elapsed().as_millis();
    out
}
