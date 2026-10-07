use super::Game;

#[cfg(windows)]
pub fn scan() -> Option<Vec<Game>> {
    use winreg::{enums::*, RegKey};
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let root = hklm.open_subkey("SOFTWARE\\WOW6432Node\\GOG.com\\Games").ok()?;
    let mut games = Vec::new();
    for id in root.enum_keys().flatten() {
        let Ok(k) = root.open_subkey(&id) else { continue };
        let name: String = k.get_value("gameName").unwrap_or_else(|_| id.clone());
        let dir: Option<String> = k.get_value("path").ok();
        // DLC entries carry a dependsOn pointing at the base game.
        if k.get_value::<String, _>("dependsOn").map(|d| !d.is_empty()).unwrap_or(false) {
            continue;
        }
        games.push(Game {
            key: format!("gog:{id}"),
            store: "gog",
            store_id: id.clone(),
            name,
            install_dir: dir,
            build: k.get_value("ver").ok(),
            size_bytes: None,
            launch: Some(format!("goggalaxy://openGameView/{id}")),
            art: None,
            art_wide: None,
            art_local: None,
            hero_local: None,
            wide_local: None,
        });
    }
    Some(games)
}

/// macOS: GOG Galaxy keeps its library in a SQLite database, but every GOG game carries its own `goggame-<id>.info`
/// (JSON: `gameId`, `rootGameId`, `name`, `buildId`) inside the app bundle (`Contents/Resources/`) or at the top of
/// its folder. Galaxy installs into `/Applications` by default; `~/Applications` and `~/GOG Games` are read too.
#[cfg(target_os = "macos")]
pub fn scan() -> Option<Vec<Game>> {
    let home = crate::install::user_home();
    let mut roots = vec![std::path::PathBuf::from("/Applications")];
    roots.extend(home.iter().flat_map(|h| [h.join("Applications"), h.join("GOG Games")]));
    let games = scan_info(&roots);
    let galaxy = roots.iter().any(|r| r.join("GOG Galaxy.app").is_dir());
    (galaxy || !games.is_empty()).then_some(games)
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn scan() -> Option<Vec<Game>> {
    None
}

/// The `goggame-*.info` files of each install in `roots` (one level down: `<root>/<Game>.app` or `<root>/<Game>/`,
/// and an app bundle inside such a folder). Base games only: a DLC's `rootGameId` names another game.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn scan_info(roots: &[std::path::PathBuf]) -> Vec<Game> {
    use std::path::{Path, PathBuf};
    /// What `dir` holds (nothing when it cannot be read).
    fn list(dir: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(dir).map(|d| d.flatten().map(|e| e.path()).collect()).unwrap_or_default()
    }
    fn infos(paths: &[PathBuf]) -> Vec<PathBuf> {
        paths.iter().filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("goggame-") && n.ends_with(".info"))).cloned().collect()
    }
    let is_app = |p: &Path| p.extension().is_some_and(|x| x == "app");
    let mut out: Vec<Game> = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else { continue };
        for e in entries.flatten() {
            let dir = e.path();
            if !dir.is_dir() {
                continue;
            }
            let listed = list(&dir);
            let mut found = infos(&listed);
            found.extend(infos(&list(&dir.join("Contents").join("Resources"))));
            if found.is_empty() && !is_app(&dir) {
                // A game folder holding the app bundle: `<root>/<Game>/<Game>.app`.
                for p in listed.iter().filter(|p| is_app(p)) {
                    found.extend(infos(&list(&p.join("Contents").join("Resources"))));
                }
            }
            for info in found {
                let Some(v) = std::fs::read(&info).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()) else { continue };
                let Some(id) = v["gameId"].as_str().filter(|i| !i.is_empty() && i.bytes().all(|b| b.is_ascii_digit())) else { continue };
                if v["rootGameId"].as_str().is_some_and(|r| r != id) || out.iter().any(|g| g.store_id == id) {
                    continue;
                }
                out.push(Game {
                    key: format!("gog:{id}"),
                    store: "gog",
                    store_id: id.to_string(),
                    name: v["name"].as_str().unwrap_or(id).to_string(),
                    install_dir: Some(dir.to_string_lossy().to_string()),
                    build: v["buildId"].as_str().map(String::from),
                    size_bytes: None,
                    launch: Some(format!("goggalaxy://openGameView/{id}")),
                    art: None,
                    art_wide: None,
                    art_local: None,
                    hero_local: None,
                    wide_local: None,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_info_files_in_bundles_and_folders() {
        let t = tempfile::tempdir().unwrap();
        let res = t.path().join("Witcher.app").join("Contents").join("Resources");
        std::fs::create_dir_all(&res).unwrap();
        std::fs::write(res.join("goggame-1207664643.info"), r#"{"gameId":"1207664643","rootGameId":"1207664643","name":"The Witcher","buildId":"5"}"#).unwrap();
        std::fs::write(res.join("goggame-1207664644.info"), r#"{"gameId":"1207664644","rootGameId":"1207664643","name":"DLC"}"#).unwrap();
        let folder = t.path().join("Doom").join("Doom.app").join("Contents").join("Resources");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("goggame-1440164514.info"), r#"{"gameId":"1440164514","name":"DOOM + DOOM II"}"#).unwrap();
        std::fs::create_dir_all(t.path().join("Other.app")).unwrap();
        let mut games = scan_info(&[t.path().to_path_buf(), t.path().join("missing")]);
        games.sort_by(|a, b| a.store_id.cmp(&b.store_id));
        let ids: Vec<_> = games.iter().map(|g| (g.key.as_str(), g.name.as_str())).collect();
        assert_eq!(ids, [("gog:1207664643", "The Witcher"), ("gog:1440164514", "DOOM + DOOM II")]);
        assert_eq!(games[0].build.as_deref(), Some("5"));
        assert!(games[1].install_dir.as_deref().unwrap().ends_with("Doom"));
    }
}
