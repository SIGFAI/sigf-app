use super::Game;

#[cfg(windows)]
pub fn scan() -> Option<Vec<Game>> {
    use winreg::{enums::*, RegKey};
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let installs = hklm.open_subkey("SOFTWARE\\WOW6432Node\\Ubisoft\\Launcher\\Installs").ok()?;
    let mut games = Vec::new();
    for id in installs.enum_keys().flatten() {
        let Ok(k) = installs.open_subkey(&id) else { continue };
        let Ok(dir) = k.get_value::<String, _>("InstallDir") else { continue };
        let dir = dir.trim_end_matches(['/', '\\']).replace('/', "\\");
        if !std::path::Path::new(&dir).exists() {
            continue;
        }
        // Ubisoft keeps no title in the registry: the install folder carries it.
        let name = dir.rsplit('\\').next().unwrap_or(&id).to_string();
        games.push(Game {
            key: format!("ubisoft:{id}"),
            store: "ubisoft",
            store_id: id.clone(),
            name,
            install_dir: Some(dir),
            build: None,
            size_bytes: None,
            launch: Some(format!("uplay://launch/{id}/0")),
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
