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

#[cfg(not(windows))]
pub fn scan() -> Option<Vec<Game>> {
    None
}
