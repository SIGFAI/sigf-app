//! Privacy choices (docs/PRIVACY.md): which automatic requests the app may make to services the player did not pick
//! themselves. Kept in `<SIGF_HOME>/privacy.json`. The installer may write the first answer (windows/hooks.nsh);
//! otherwise the app asks on its first start, and until then the core sends nothing but the catalog request.
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// How a host's join address is filled in: `ask` leaves it empty with a "use my LAN address" button, `auto` fills in
/// this PC's local network address.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LanShare {
    #[default]
    Ask,
    Auto,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Privacy {
    /// The first-start screen (or the installer's question) was answered.
    pub asked: bool,
    /// Game, mashup and creator pictures from Steam, Epic and Modrinth image servers.
    pub store_art: bool,
    /// The name of a game with no picture sent to the Steam store search.
    pub art_search: bool,
    /// The ids of the games you own sent with lobby lists (else the app filters the full public list itself).
    pub lobby_games: bool,
    pub lan_address: LanShare,
    /// The UI language the player picked (`en`, `zh-CN`, ...); absent: the system's. Only the UI reads it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

impl Default for Privacy {
    /// Features on, LAN address asked for, and nothing answered yet.
    fn default() -> Self {
        Privacy { asked: false, store_art: true, art_search: true, lobby_games: true, lan_address: LanShare::Ask, language: None }
    }
}

pub fn store_path() -> PathBuf {
    crate::install::home_dir().join("privacy.json")
}

/// The saved choices; a missing or unreadable file is the default (not answered).
pub fn load(path: &Path) -> Privacy {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// Writes the choices whole (tmp + rename).
pub fn save(path: &Path, p: &Privacy) -> Result<(), String> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

static CURRENT: Mutex<Option<Privacy>> = Mutex::new(None);

/// The choices in force (read once from disk, then kept in memory).
pub fn current() -> Privacy {
    let mut c = CURRENT.lock().unwrap_or_else(|p| p.into_inner());
    c.get_or_insert_with(|| load(&store_path())).clone()
}

/// Saves new choices and puts them in force.
pub fn set(p: Privacy) -> Result<Privacy, String> {
    save(&store_path(), &p)?;
    *CURRENT.lock().unwrap_or_else(|p| p.into_inner()) = Some(p.clone());
    Ok(p)
}

/// The only request allowed before the player answered: the catalog.
pub const CATALOG_PATH: &str = "/api/app/catalog";

/// Whether a sigf.ai GET may go out under these choices.
pub fn site_fetch_ok(p: &Privacy, path: &str) -> bool {
    p.asked || path == CATALOG_PATH
}

/// Whether a lobby API call may go out: nothing before the answer, and no `games=` list when that is off.
pub fn lobby_call_ok(p: &Privacy, path: &str) -> bool {
    if !p.asked {
        return false;
    }
    if p.lobby_games {
        return true;
    }
    match path.split_once('?') {
        Some((_, q)) => !q.split('&').any(|kv| kv.split('=').next() == Some("games")),
        None => true,
    }
}

/// Whether the Steam store search by name may run.
pub fn art_search_ok(p: &Privacy) -> bool {
    p.asked && p.art_search
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_on_but_unanswered() {
        let d = Privacy::default();
        assert!(!d.asked && d.store_art && d.art_search && d.lobby_games);
        assert_eq!(d.lan_address, LanShare::Ask);
    }

    #[test]
    fn reads_the_installer_file_and_partial_files() {
        // What windows/hooks.nsh writes.
        let p: Privacy = serde_json::from_str(r#"{"asked":true,"storeArt":false,"artSearch":false,"lobbyGames":false,"lanAddress":"ask"}"#).unwrap();
        assert_eq!(p, Privacy { asked: true, store_art: false, art_search: false, lobby_games: false, lan_address: LanShare::Ask, language: None });
        let p: Privacy = serde_json::from_str(r#"{"asked":true,"lanAddress":"auto"}"#).unwrap();
        assert!(p.asked && p.store_art && p.art_search && p.lobby_games);
        assert_eq!(p.lan_address, LanShare::Auto);
    }

    #[test]
    fn round_trip_and_bad_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("privacy.json");
        assert_eq!(load(&path), Privacy::default());
        let p = Privacy { asked: true, store_art: false, art_search: true, lobby_games: false, lan_address: LanShare::Auto, language: Some("zh-CN".into()) };
        save(&path, &p).unwrap();
        assert_eq!(load(&path), p);
        std::fs::write(&path, b"{not json").unwrap();
        assert_eq!(load(&path), Privacy::default());
    }

    #[test]
    fn nothing_but_the_catalog_before_the_answer() {
        let fresh = Privacy::default();
        assert!(site_fetch_ok(&fresh, "/api/app/catalog"));
        assert!(!site_fetch_ok(&fresh, "/api/studio/agents"));
        assert!(!site_fetch_ok(&fresh, "/api/app/recipe/sigf%2Fx@1.0.0"));
        assert!(!lobby_call_ok(&fresh, "/api/app/lobbies"));
        assert!(!lobby_call_ok(&fresh, "/api/app/lobbies/hosting"));
        assert!(!art_search_ok(&fresh));
        let yes = Privacy { asked: true, ..Privacy::default() };
        assert!(site_fetch_ok(&yes, "/api/studio/agents"));
        assert!(art_search_ok(&yes));
        assert!(!art_search_ok(&Privacy { art_search: false, ..yes.clone() }));
    }

    /// The installer's privacy text says word for word what docs/PRIVACY.md quotes, and names the policy's URL.
    #[test]
    fn installer_text_matches_the_policy() {
        let norm = |s: &str| s.replace("\r\n", "\n").trim().to_string();
        let txt = norm(include_str!("../windows/privacy.txt"));
        let doc = include_str!("../../docs/PRIVACY.md").replace("\r\n", "\n");
        let quoted = doc.split("```text\n").nth(1).and_then(|r| r.split("\n```").next()).expect("docs/PRIVACY.md quotes the installer text");
        assert_eq!(norm(quoted), txt, "docs/PRIVACY.md and windows/privacy.txt differ");
        assert!(txt.is_ascii(), "privacy.txt stays ASCII (the NSIS license page reads it in the ANSI code page)");
        assert!(txt.contains("https://sigf.ai/privacy"));
        for host in ["sigf.ai", "Steam", "Epic Games", "Modrinth", "GitHub", "Microsoft"] {
            assert!(txt.contains(host), "privacy.txt names {host}");
        }
    }

    /// What windows/hooks.nsh writes after its question parses, for either answer, and leaves the first-start screen on.
    #[test]
    fn installer_answer_parses() {
        let nsh = include_str!("../windows/hooks.nsh");
        let line = nsh.lines().find(|l| l.contains("FileWrite") && l.contains("storeArt")).expect("hooks.nsh writes privacy.json");
        let json = line.split('\'').nth(1).expect("quoted JSON");
        for (answer, on) in [("true", true), ("false", false)] {
            let p: Privacy = serde_json::from_str(&json.replace("$R7", answer)).unwrap();
            assert_eq!(p, Privacy { asked: false, store_art: on, art_search: on, lobby_games: on, lan_address: LanShare::Ask, language: None });
        }
    }

    #[test]
    fn owned_games_only_when_allowed() {
        let on = Privacy { asked: true, ..Privacy::default() };
        let off = Privacy { lobby_games: false, ..on.clone() };
        assert!(lobby_call_ok(&on, "/api/app/lobbies?games=tf2,minecraft"));
        assert!(!lobby_call_ok(&off, "/api/app/lobbies?games=tf2,minecraft"));
        assert!(!lobby_call_ok(&off, "/api/app/lobbies?mashup=sigf%2Fx&games=tf2"));
        assert!(lobby_call_ok(&off, "/api/app/lobbies?mashup=sigf%2Fx"));
        assert!(lobby_call_ok(&off, "/api/app/lobbies?"));
        assert!(lobby_call_ok(&off, "/api/app/lobbies/k3m9xq2wa7fd/heartbeat"));
    }
}
