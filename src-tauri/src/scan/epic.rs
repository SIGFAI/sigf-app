use super::Game;
use base64::Engine as _;
use serde_json::Value;
use std::path::PathBuf;

fn launcher_data() -> PathBuf {
    let pd = std::env::var("ProgramData").unwrap_or_else(|_| "C:\\ProgramData".into());
    PathBuf::from(pd).join("Epic").join("EpicGamesLauncher").join("Data")
}

fn manifests_dir() -> PathBuf {
    launcher_data().join("Manifests")
}

/// The launcher's catalog cache: base64 of a JSON array of catalog items carrying `keyImages`.
/// Seen under ProgramData; LocalAppData kept as a fallback for other launcher versions.
fn catalog() -> Vec<Value> {
    let local = std::env::var("LOCALAPPDATA").map(|d| PathBuf::from(d).join("EpicGamesLauncher").join("Saved").join("Data"));
    for p in [Some(launcher_data()), local.ok()].into_iter().flatten() {
        let Ok(raw) = std::fs::read(p.join("Catalog").join("catcache.bin")) else { continue };
        let clean: Vec<u8> = raw.into_iter().filter(|c| !c.is_ascii_whitespace()).collect();
        let Ok(json) = base64::engine::general_purpose::STANDARD.decode(clean) else { continue };
        if let Ok(Value::Array(items)) = serde_json::from_slice(&json) {
            return items;
        }
    }
    Vec::new()
}

/// Catalog item for a manifest: by catalog item id, else by a release whose appId is the AppName.
fn find_item<'a>(items: &'a [Value], ns: &str, item: &str, app: &str) -> Option<&'a Value> {
    items
        .iter()
        .find(|i| !item.is_empty() && i["id"].as_str() == Some(item) && (ns.is_empty() || i["namespace"].as_str() == Some(ns)))
        .or_else(|| {
            items.iter().find(|i| i["releaseInfo"].as_array().is_some_and(|r| r.iter().any(|r| r["appId"].as_str() == Some(app))))
        })
}

/// First keyImage url of the given types, spaces escaped (some Epic file names carry them).
fn key_image(item: &Value, types: &[&str]) -> Option<String> {
    let imgs = item["keyImages"].as_array()?;
    types.iter().find_map(|t| {
        imgs.iter()
            .find(|i| i["type"].as_str() == Some(t))
            .and_then(|i| i["url"].as_str())
            .filter(|u| u.starts_with("https://"))
            .map(|u| u.replace(' ', "%20"))
    })
}

pub fn scan() -> Option<Vec<Game>> {
    let dir = manifests_dir();
    let entries = std::fs::read_dir(&dir).ok()?;
    let items = catalog();
    let mut games = Vec::new();
    for e in entries.flatten() {
        if e.path().extension().and_then(|x| x.to_str()) != Some("item") {
            continue;
        }
        let Ok(src) = std::fs::read_to_string(e.path()) else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&src) else { continue };
        // Engine builds, plugins and DLC-only entries are not games.
        let is_game = v["AppCategories"]
            .as_array()
            .map(|c| c.iter().any(|x| x.as_str() == Some("games")))
            .unwrap_or(true);
        if !is_game || v["bIsIncompleteInstall"].as_bool() == Some(true) {
            continue;
        }
        let Some(app) = v["AppName"].as_str() else { continue };
        if !v["InstallLocation"].as_str().is_some_and(|d| std::path::Path::new(d).is_dir()) {
            continue;
        }
        let ns = v["CatalogNamespace"].as_str().unwrap_or("");
        let item = v["CatalogItemId"].as_str().unwrap_or("");
        let launch = if ns.is_empty() {
            format!("com.epicgames.launcher://apps/{app}?action=launch&silent=true")
        } else {
            format!("com.epicgames.launcher://apps/{ns}%3A{item}%3A{app}?action=launch&silent=true")
        };
        let found = find_item(&items, ns, item, app);
        games.push(Game {
            key: format!("epic:{app}"),
            store: "epic",
            store_id: app.to_string(),
            name: v["DisplayName"].as_str().unwrap_or(app).to_string(),
            install_dir: v["InstallLocation"].as_str().map(String::from),
            build: v["AppVersionString"].as_str().map(String::from),
            size_bytes: v["InstallSize"].as_u64(),
            launch: Some(launch),
            art: found.and_then(|i| key_image(i, &["DieselGameBoxTall", "OfferImageTall", "Thumbnail"])),
            art_wide: found.and_then(|i| key_image(i, &["DieselGameBox", "OfferImageWide", "DieselStoreFrontWide"])),
            art_local: None,
            hero_local: None,
            wide_local: None,
        });
    }
    Some(games)
}
