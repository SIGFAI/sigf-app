//! Art for games their own store doesn't illustrate (Ubisoft, Epic titles missing from the catalog cache):
//! the same title on Steam. Results, misses included, are cached in `%LOCALAPPDATA%\SIGF\artcache.json`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A miss is retried after a week: the game may land on Steam later.
const MISS_TTL: u64 = 7 * 24 * 3600;
/// Newer apps only serve their art under hashed paths, resolved through the store API.
const ASSET_BASE: &str = "https://shared.akamai.steamstatic.com/store_item_assets/";

/// One lookup at a time: a library of tiles asks at once, and they share the cache file.
static LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamArt {
    pub appid: String,
    /// Portrait 600x900.
    pub art: Option<String>,
    pub hero: Option<String>,
    /// Header 460x215.
    pub wide: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    at: u64,
    hit: Option<SteamArt>,
}

/// `Tom Clancy’s The Division® 2` and `Tom Clancy's The Division 2` meet as `tom clancy s the division 2`.
pub fn normalize(name: &str) -> String {
    let mut out = String::new();
    for c in name.to_lowercase().chars() {
        if matches!(c, '™' | '®' | '©') {
            continue;
        }
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with(' ') {
            out.push(' ');
        }
    }
    out.trim().to_string()
}

fn cache_path() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("SIGF").join("artcache.json"))
}

fn load() -> HashMap<String, Entry> {
    cache_path()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save(c: &HashMap<String, Entry>) {
    let Some(p) = cache_path() else { return };
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(b) = serde_json::to_vec_pretty(c) {
        let _ = std::fs::write(p, b);
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn get_json(client: &reqwest::blocking::Client, url: reqwest::Url) -> Result<Value, String> {
    let r = client.get(url).send().map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(format!("HTTP {}", r.status()));
    }
    serde_json::from_str(&r.text().map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

/// Store search, exact name match after normalizing. `Ok(None)` is a real miss, `Err` a network failure.
fn search(client: &reqwest::blocking::Client, name: &str) -> Result<Option<String>, String> {
    let url = reqwest::Url::parse_with_params(
        "https://store.steampowered.com/api/storesearch/",
        &[("term", name), ("cc", "us"), ("l", "en")],
    )
    .map_err(|e| e.to_string())?;
    let v = get_json(client, url)?;
    let want = normalize(name);
    let squash = |s: &str| s.replace(' ', "");
    Ok(v["items"].as_array().and_then(|items| {
        items
            .iter()
            .filter(|i| i["type"].as_str().is_none_or(|t| t == "app"))
            .find(|i| i["name"].as_str().is_some_and(|n| squash(&normalize(n)) == squash(&want)))
            .and_then(|i| i["id"].as_u64())
            .map(|id| id.to_string())
    }))
}

/// Hashed asset paths for an app; plain CDN paths (fine for older apps) when the API says nothing.
fn assets(client: &reqwest::blocking::Client, appid: &str) -> SteamArt {
    let plain = |f: &str| Some(format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}/{f}"));
    let mut art = SteamArt {
        appid: appid.to_string(),
        art: plain("library_600x900.jpg"),
        hero: plain("library_hero.jpg"),
        wide: plain("header.jpg"),
    };
    let input = format!(
        r#"{{"ids":[{{"appid":{appid}}}],"context":{{"language":"english","country_code":"US"}},"data_request":{{"include_assets":true}}}}"#
    );
    let Ok(url) = reqwest::Url::parse_with_params(
        "https://api.steampowered.com/IStoreBrowseService/GetItems/v1",
        &[("input_json", input.as_str())],
    ) else {
        return art;
    };
    let Ok(v) = get_json(client, url) else { return art };
    let a = &v["response"]["store_items"][0]["assets"];
    let Some(fmt) = a["asset_url_format"].as_str() else { return art };
    let url = |k: &str| a[k].as_str().map(|f| format!("{ASSET_BASE}{}", fmt.replace("${FILENAME}", f)));
    art.art = url("library_capsule").or(art.art);
    art.hero = url("library_hero").or(art.hero);
    art.wide = url("header").or(art.wide);
    art
}

/// Steam art for a game known only by name; `None` when Steam has no such title (or is unreachable).
pub fn steam_lookup(name: &str) -> Option<SteamArt> {
    let key = normalize(name);
    if key.is_empty() {
        return None;
    }
    let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut cache = load();
    if let Some(e) = cache.get(&key) {
        if e.hit.is_some() || now().saturating_sub(e.at) < MISS_TTL {
            return e.hit.clone();
        }
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent("sigf-app")
        .build()
        .ok()?;
    // Network errors are not cached: the next scan tries again.
    let hit = search(&client, name).ok()?.map(|id| assets(&client, &id));
    cache.insert(key, Entry { at: now(), hit: hit.clone() });
    save(&cache);
    hit
}

#[cfg(test)]
mod tests {
    use super::normalize;

    #[test]
    fn normalizes_store_names() {
        assert_eq!(normalize("Tom Clancy’s The Division® 2"), normalize("Tom Clancy's The Division 2"));
        assert_eq!(normalize("Kingdom: New Lands"), "kingdom new lands");
        assert_eq!(normalize("Kingdom New Lands"), "kingdom new lands");
    }
}
