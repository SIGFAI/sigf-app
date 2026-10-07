use super::{appinfo, vdf, Game};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Runtimes and tools Steam installs next to games (plus Wallpaper Engine): not games.
const SKIP_APPS: &[&str] = &["228980", "1070560", "1391110", "1628350", "1493710", "250820", "431960"];

/// Name endings of tools and extras, used only when appinfo.vdf can't tell the app type.
const SKIP_SUFFIXES: &[&str] = &["editor", "dedicated server", "sdk", "tools", "soundtrack", "benchmark"];

/// The Steam client's folder: the registry's `SteamPath` (else `C:\Program Files (x86)\Steam`) on Windows,
/// `~/Library/Application Support/Steam` on macOS.
pub fn steam_root() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use winreg::{enums::*, RegKey};
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(k) = hkcu.open_subkey("Software\\Valve\\Steam") {
            if let Ok(p) = k.get_value::<String, _>("SteamPath") {
                return Some(PathBuf::from(p));
            }
        }
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        if let Ok(k) = hklm.open_subkey("SOFTWARE\\WOW6432Node\\Valve\\Steam") {
            if let Ok(p) = k.get_value::<String, _>("InstallPath") {
                return Some(PathBuf::from(p));
            }
        }
    }
    #[cfg(windows)]
    let default = PathBuf::from("C:\\Program Files (x86)\\Steam");
    #[cfg(target_os = "macos")]
    let default = crate::install::user_data_dir()?.join("Steam");
    #[cfg(not(any(windows, target_os = "macos")))]
    let default = crate::install::user_home()?.join(".local").join("share").join("Steam");
    default.exists().then_some(default)
}

/// Steam writes the same library as `c:/program files (x86)/steam` and `C:\Program Files (x86)\Steam`.
fn same_dir(a: &Path, b: &Path) -> bool {
    let n = |p: &Path| p.to_string_lossy().replace('/', "\\").trim_end_matches('\\').to_lowercase();
    n(a) == n(b)
}

fn libraries(root: &Path) -> Vec<PathBuf> {
    let mut libs = vec![root.to_path_buf()];
    if let Ok(src) = std::fs::read_to_string(root.join("steamapps").join("libraryfolders.vdf")) {
        let v = vdf::parse(&src);
        if let Some(folders) = v.get("libraryfolders") {
            for (_, f) in folders.entries() {
                if let Some(p) = f.str("path") {
                    let p = PathBuf::from(p.replace("\\\\", "\\"));
                    if !libs.iter().any(|l| same_dir(l, &p)) {
                        libs.push(p);
                    }
                }
            }
        }
    }
    libs
}

/// Steam's own art cache, served to the webview through the asset protocol.
pub fn library_cache() -> Option<PathBuf> {
    let p = steam_root()?.join("appcache").join("librarycache");
    if !p.is_dir() {
        return None;
    }
    // Windows: one separator style, so the asset scope matches the paths the scan hands out.
    Some(if cfg!(windows) { PathBuf::from(p.to_string_lossy().replace('/', "\\")) } else { p })
}

/// Newest `name` in `<appid>/`, `<appid>/<hash>/` (current layout) or `<appid>_name` (old flat layout).
fn cached(cache: &Path, id: &str, names: &[&str]) -> Option<String> {
    let dir = cache.join(id);
    for name in names {
        let mut hits: Vec<PathBuf> = vec![dir.join(name)];
        if let Ok(subs) = std::fs::read_dir(&dir) {
            hits.extend(subs.flatten().filter(|e| e.path().is_dir()).map(|e| e.path().join(name)));
        }
        hits.push(cache.join(format!("{id}_{name}")));
        let newest = hits
            .into_iter()
            .filter_map(|p| Some((std::fs::metadata(&p).ok().filter(|m| m.is_file() && m.len() > 0)?.modified().ok()?, p)))
            .max_by_key(|(t, _)| *t);
        if let Some((_, p)) = newest {
            return Some(p.to_string_lossy().to_string());
        }
    }
    None
}

/// Lowercased app types from appinfo.vdf; empty when the file is missing or in an unknown format.
fn app_types(root: &Path, ids: &HashSet<u32>) -> HashMap<u32, String> {
    std::fs::read(root.join("appcache").join("appinfo.vdf"))
        .ok()
        .and_then(|b| appinfo::app_types(&b, ids))
        .unwrap_or_default()
}

fn looks_like_tool(name: &str) -> bool {
    let n = name.to_lowercase();
    SKIP_SUFFIXES.iter().any(|s| n.ends_with(s))
}

pub fn scan() -> Option<Vec<Game>> {
    let root = steam_root()?;
    let cache = library_cache();
    let mut games = Vec::new();
    for lib in libraries(&root) {
        let apps = lib.join("steamapps");
        let Ok(dir) = std::fs::read_dir(&apps) else { continue };
        for e in dir.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !(name.starts_with("appmanifest_") && name.ends_with(".acf")) {
                continue;
            }
            let Ok(src) = std::fs::read_to_string(e.path()) else { continue };
            let v = vdf::parse(&src);
            let Some(s) = v.get("AppState") else { continue };
            let Some(id) = s.str("appid") else { continue };
            let title = s.str("name").unwrap_or(id).to_string();
            if SKIP_APPS.contains(&id) || title.contains("Redistributable") || title.starts_with("Proton ") {
                continue;
            }
            let dir = s.str("installdir").map(|d| apps.join("common").join(d));
            // Stale manifests survive a deleted or moved library folder: no folder, no game.
            if !dir.as_ref().is_some_and(|d| d.is_dir()) {
                continue;
            }
            games.push(Game {
                key: format!("steam:{id}"),
                store: "steam",
                store_id: id.to_string(),
                name: title,
                install_dir: dir.map(|d| d.to_string_lossy().to_string()),
                build: s.str("buildid").map(String::from),
                size_bytes: s.str("SizeOnDisk").and_then(|x| x.parse().ok()),
                launch: Some(format!("steam://rungameid/{id}")),
                art: Some(format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{id}/library_600x900.jpg")),
                art_wide: Some(format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{id}/header.jpg")),
                art_local: cache.as_deref().and_then(|c| cached(c, id, &["library_600x900.jpg", "library_capsule.jpg"])),
                hero_local: cache.as_deref().and_then(|c| cached(c, id, &["library_hero.jpg"])),
                wide_local: cache.as_deref().and_then(|c| cached(c, id, &["header.jpg", "library_header.jpg"])),
            });
        }
    }
    // Tools, software and editors install like games: keep what Steam itself calls a game.
    let ids: HashSet<u32> = games.iter().filter_map(|g| g.store_id.parse().ok()).collect();
    let types = app_types(&root, &ids);
    games.retain(|g| match g.store_id.parse().ok().and_then(|id: u32| types.get(&id)) {
        Some(t) => t == "game",
        None => !looks_like_tool(&g.name),
    });
    Some(games)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_cached_art_in_both_layouts() {
        let d = tempfile::tempdir().unwrap();
        let c = d.path();
        std::fs::create_dir_all(c.join("10").join("abc")).unwrap();
        std::fs::write(c.join("10").join("abc").join("library_600x900.jpg"), b"x").unwrap();
        std::fs::write(c.join("20_library_hero.jpg"), b"x").unwrap();
        std::fs::create_dir_all(c.join("30")).unwrap();
        std::fs::write(c.join("30").join("library_capsule.jpg"), b"x").unwrap();
        assert!(cached(c, "10", &["library_600x900.jpg"]).unwrap().ends_with("library_600x900.jpg"));
        assert!(cached(c, "20", &["library_hero.jpg"]).unwrap().ends_with("20_library_hero.jpg"));
        assert!(cached(c, "30", &["library_600x900.jpg", "library_capsule.jpg"]).unwrap().ends_with("library_capsule.jpg"));
        assert!(cached(c, "40", &["library_600x900.jpg"]).is_none());
    }

    #[test]
    fn tool_names() {
        assert!(looks_like_tool("Infection Free Zone - Map Editor"));
        assert!(!looks_like_tool("Infection Free Zone"));
    }
}
