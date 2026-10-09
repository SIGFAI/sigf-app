//! `installed.json`: what is installed, how, and how to undo it. Written atomically (temp file + rename)
//! so a crash never leaves the player with a registry that forgot a snapshot.

use super::recipe::Strategy;
use super::InstallError;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledMod {
    pub id: String,
    pub version: String,
    pub name: String,
    /// Unix seconds.
    pub installed_at: u64,
    pub games: Vec<InstalledGame>,
    /// Files placed besides the downloads: the player's own copies and the files built on this PC (absolute paths).
    /// Restore deletes them.
    #[serde(default)]
    pub placed: Vec<String>,
    /// The recipe's `conflicts` at install: a later install of one of them is refused even though its own recipe does
    /// not name this one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<String>,
    /// The recipe's `requires_files`, resolved at install: checked again before every Play.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires_files: Vec<RequiredAt>,
    /// Mod plans (`mod/<ref>`, `crate::mods`): every file downloaded, with the sha256 of what was installed and the
    /// hash the source gave that was checked (`none` when the source gives none).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<ModFileRecord>,
}

/// One downloaded file of a mod plan, as installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModFileRecord {
    pub name: String,
    /// The ref (item or dependency) the file belongs to.
    pub of: String,
    /// The download URL without its query (CDN tokens are not kept).
    pub url: String,
    pub sha256: String,
    /// `sha512`, `sha256`, `sha1`, `md5` or `none`.
    pub verified: String,
}

/// One `requires_files` entry with the game folder it was checked in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequiredAt {
    pub id: String,
    /// Relative to `dir`, forward slashes (`nvse_loader.exe`).
    pub path: String,
    /// The scanned game folder at install.
    pub dir: String,
    pub message: String,
    #[serde(default)]
    pub page: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledGame {
    pub game: String,
    pub strategy: Strategy,
    /// Arguments to pass on launch (store launch with args, or the launcher exe for Minecraft).
    pub launch_args: Vec<String>,
    /// Prism instance folder name (`mrpack`); None when the pack went through Prism's own import dialog.
    #[serde(default)]
    pub instance: Option<String>,
    /// Folder we own for this game: the profile dir (`args`/`profile`) or the Prism instance (`mrpack`).
    #[serde(default)]
    pub profile_dir: Option<String>,
    /// Snapshot folder (`game-dir-snapshot`).
    #[serde(default)]
    pub snapshot: Option<String>,
    #[serde(default)]
    pub game_dir: Option<String>,
    /// Launcher exe to start with `launch_args` (Prism), when the game is not launched through its store.
    #[serde(default)]
    pub launcher: Option<String>,
    /// The recipe's launch `exe` (script extender loader), started from the game folder instead of the store.
    #[serde(default)]
    pub exe: Option<LaunchExe>,
    /// The recipe's launch `wait` (`port:<n>`): what to wait for before starting the next game.
    #[serde(default)]
    pub wait: Option<String>,
    /// The recipe's launch `me3`: started through me3 with a `.me3` profile, offline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub me3: Option<Me3Launch>,
    /// The recipe's `launch[]` lists other games but not this one: Play does not start it (the mod starts it itself,
    /// hidden, or the player starts it from another tool). Entries written before this field start as before.
    #[serde(default, skip_serializing_if = "is_false")]
    pub no_start: bool,
}

impl InstalledGame {
    /// An entry with nothing set but its game and strategy: callers fill in what they own (`..InstalledGame::new(..)`).
    pub fn new(game: impl Into<String>, strategy: Strategy) -> Self {
        Self {
            game: game.into(),
            strategy,
            launch_args: vec![],
            instance: None,
            profile_dir: None,
            snapshot: None,
            game_dir: None,
            launcher: None,
            exe: None,
            wait: None,
            me3: None,
            no_start: false,
        }
    }
}

/// A me3 launch as installed: everything resolved and pinned at install, checked again at Play.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Me3Launch {
    /// me3's `--game` id (`eldenring`).
    pub game: String,
    /// The `.me3` profile: `path` relative to `dir` (the `{app}` or `{game}` folder it was installed into).
    pub profile: LaunchExe,
    /// A me3.exe the recipe installed (pinned by sha256); None: the player's me3 from its installer.
    #[serde(default)]
    pub exe: Option<LaunchExe>,
    #[serde(default)]
    pub savefile: Option<String>,
    #[serde(default)]
    pub disable_arxan: bool,
    /// Where to get me3 when it is missing (`install ME3 from github.com/...`), from the recipe's `requires`.
    #[serde(default)]
    pub hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchExe {
    /// As the recipe gives it, relative to `dir` (`skse64_loader.exe`); resolved again at play.
    pub path: String,
    /// The scanned game folder at install time: the exe's root and working directory.
    pub dir: String,
    /// Where to get it when missing (`install SKSE64 from skse.silverlock.org`), from the recipe's `requires`.
    #[serde(default)]
    pub hint: Option<String>,
    /// A file the recipe installed: its sha256, checked before it is started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// In the mashup's own `{app}` folder (`launch[].app_exe`): not a game, so Steam is not started first.
    #[serde(default, skip_serializing_if = "is_false")]
    pub own: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

pub fn load(home: &Path) -> Vec<InstalledMod> {
    std::fs::read_to_string(home.join("installed.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(home: &Path, mods: &[InstalledMod]) -> Result<(), InstallError> {
    std::fs::create_dir_all(home).map_err(|e| InstallError::io(home, e))?;
    let path = home.join("installed.json");
    let tmp = home.join("installed.json.tmp");
    let json = serde_json::to_string_pretty(mods).map_err(|e| InstallError::io(&path, e))?;
    std::fs::write(&tmp, json).map_err(|e| InstallError::io(&tmp, e))?;
    std::fs::rename(&tmp, &path).map_err(|e| InstallError::io(&path, e))
}
