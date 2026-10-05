//! Free hosted servers (docs/RECIPE-FORMAT.md section 9.5): the host secrets of the lobbies this app started a hosted
//! server for, kept in `<SIGF_HOME>/hosted.json` so "Download world" still works after a restart, for the 7 days the
//! world is kept. Only the lobby id, its secret, the mashup's name and the dates: nothing else about anyone.
use std::path::{Path, PathBuf};

/// Entries are dropped once their world is gone (`world_until`), or after this many days without one.
const KEEP_DAYS: i64 = 9;
const MAX_ENTRIES: usize = 50;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostedEntry {
    pub lobby: String,
    pub secret: String,
    pub mashup_id: String,
    pub name: String,
    /// Unix seconds.
    pub started_at: i64,
    /// Unix seconds: the world is downloadable until then (session end + 7 days), when known.
    #[serde(default)]
    pub world_until: Option<i64>,
}

pub fn store_path() -> PathBuf {
    crate::install::home_dir().join("hosted.json")
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// A secret as the API hands it out: base64url, 16..=200 characters.
fn secret_ok(s: &str) -> bool {
    (16..=200).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn valid(e: &HostedEntry) -> bool {
    crate::join::valid_lobby_id(&e.lobby)
        && secret_ok(&e.secret)
        && e.mashup_id.len() <= 120
        && e.name.chars().count() <= 120
        && !e.name.chars().any(|c| c.is_control())
}

fn alive(e: &HostedEntry, at: i64) -> bool {
    match e.world_until {
        Some(until) => until > at,
        None => e.started_at + KEEP_DAYS * 86_400 > at,
    }
}

/// The kept entries, newest first; unreadable files and stale or malformed entries are left out.
pub fn load(path: &Path) -> Vec<HostedEntry> {
    let at = now();
    let mut list: Vec<HostedEntry> = std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice::<Vec<HostedEntry>>(&b).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|e| valid(e) && alive(e, at))
        .collect();
    list.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    list
}

fn write(path: &Path, list: &[HostedEntry]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(list).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// Adds or updates the entry of that lobby (a later `world_until` replaces the earlier one).
pub fn save(path: &Path, entry: HostedEntry) -> Result<Vec<HostedEntry>, String> {
    if !valid(&entry) {
        return Err("refused hosted entry".into());
    }
    let mut list: Vec<HostedEntry> = load(path).into_iter().filter(|e| e.lobby != entry.lobby).collect();
    list.insert(0, entry);
    list.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    list.truncate(MAX_ENTRIES);
    write(path, &list)?;
    Ok(list)
}

pub fn forget(path: &Path, lobby: &str) -> Result<Vec<HostedEntry>, String> {
    let list: Vec<HostedEntry> = load(path).into_iter().filter(|e| e.lobby != lobby).collect();
    write(path, &list)?;
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(lobby: &str, started_at: i64, world_until: Option<i64>) -> HostedEntry {
        HostedEntry {
            lobby: lobby.into(),
            secret: "s3cr3t-base64url_value-0123456789".into(),
            mashup_id: "sigf/gta5-blocky".into(),
            name: "Blocky Los Santos".into(),
            started_at,
            world_until,
        }
    }

    #[test]
    fn keeps_secrets_until_the_world_is_gone() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("hosted.json");
        assert!(load(&p).is_empty(), "no file yet");
        let t = now();
        save(&p, entry("abcdefghjkmn", t - 100, None)).unwrap();
        save(&p, entry("bcdefghjkmnp", t - 50, Some(t + 7 * 86_400))).unwrap();
        save(&p, entry("cdefghjkmnpq", t - 30 * 86_400, Some(t - 1))).unwrap();
        let list = load(&p);
        assert_eq!(list.iter().map(|e| e.lobby.as_str()).collect::<Vec<_>>(), ["bcdefghjkmnp", "abcdefghjkmn"], "newest first, expired out");
        // An update of the same lobby replaces it.
        save(&p, entry("abcdefghjkmn", t - 100, Some(t + 3600))).unwrap();
        let list = load(&p);
        assert_eq!(list.len(), 2);
        assert_eq!(list.iter().find(|e| e.lobby == "abcdefghjkmn").unwrap().world_until, Some(t + 3600));
        forget(&p, "abcdefghjkmn").unwrap();
        assert_eq!(load(&p).len(), 1);
        // Stale without a known end: dropped after KEEP_DAYS.
        save(&p, entry("defghjkmnpqr", t - (KEEP_DAYS + 1) * 86_400, None)).unwrap();
        assert_eq!(load(&p).len(), 1);
    }

    #[test]
    fn refuses_what_the_api_never_hands_out() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("hosted.json");
        let t = now();
        for bad in [
            HostedEntry { lobby: "../../etc".into(), ..entry("abcdefghjkmn", t, None) },
            HostedEntry { secret: "short".into(), ..entry("abcdefghjkmn", t, None) },
            HostedEntry { secret: "has space in it 0123456789".into(), ..entry("abcdefghjkmn", t, None) },
            HostedEntry { name: "a\nb".into(), ..entry("abcdefghjkmn", t, None) },
        ] {
            assert!(save(&p, bad).is_err());
        }
        std::fs::write(&p, b"not json").unwrap();
        assert!(load(&p).is_empty(), "a broken file reads as empty");
    }
}
