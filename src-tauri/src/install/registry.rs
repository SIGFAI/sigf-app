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
