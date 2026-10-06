//! `mashup.json`: the recipe the SIGF publisher writes at release and the sigf.ai catalog serves
//! (docs/RECIPE-FORMAT.md section 4). Unknown fields are ignored so an older app still reads newer recipes.

use super::InstallError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recipe {
    /// `sigf/<slug>`; also the key in the installed registry.
    pub id: String,
    pub version: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub kind: Kind,
    #[serde(default)]
    pub games: Vec<GameReq>,
    /// Third-party pieces (ScriptHookV, loaders). Those with a `source` are fetched and verified with everything else.
    #[serde(default)]
    pub requires: Vec<Requirement>,
    #[serde(default)]
    pub install: Vec<Step>,
    #[serde(default)]
    pub launch: Vec<LaunchStep>,
    /// Every release asset of the mod (`name` = an install file's `src`). An install file without its own `url`
    /// is fetched from the asset of the same name.
    #[serde(default)]
    pub files: Vec<Asset>,
    #[serde(default)]
    pub source: Option<RepoSource>,
    #[serde(default)]
    pub media: Option<Media>,
    #[serde(default)]
    pub built_by: Option<BuiltBy>,
    /// Bring your own copy: game files the player already has (a ROM they dumped), found or picked on their PC,
    /// checked by SHA-1 and copied into the mashup's own folder. Never shipped, downloaded or uploaded by SIGF.
    #[serde(default)]
    pub own_copies: Vec<OwnCopy>,
    /// Files SIGF must not distribute, built once on the player's PC by a pinned script with a pinned toolchain.
    #[serde(default)]
    pub player_build: Vec<PlayerBuild>,
}

/// `own_copies[]`: one file of a game the player owns (docs/RECIPE-FORMAT.md section 4, "Bring your own copy").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnCopy {
    /// The `games[]` entry this copy comes from (`sm64`).
    pub game: String,
    /// What the player is asked for: "Super Mario 64 (USA)".
    #[serde(default)]
    pub label: String,
    pub rom: OwnRom,
    /// Lowercase hints for the search (`mario 64`): matching file names are checked first.
    #[serde(default)]
    pub names: Vec<String>,
    /// The install step (game id) whose folder receives the copy.
    pub step: String,
    /// The folder the copy goes to: `{instance}/...` (that step's Prism instance) or `{app}/...`.
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnRom {
    /// The file name it is saved as in `to` (`baserom.us.z64`).
    #[serde(rename = "as")]
    pub save_as: String,
    /// Accepted SHA-1s of the file (after `format` normalization), lowercase hex.
    pub sha1: Vec<String>,
    /// File extensions searched for (`.z64`), also inside `.zip` archives.
    pub extensions: Vec<String>,
    /// Exact size in bytes, when every accepted dump has the same size: speeds up the search.
    #[serde(default)]
    pub size: Option<u64>,
    /// `n64`: byte-swapped (`.v64`) and little-endian (`.n64`) dumps are normalized to big-endian `.z64` first.
    #[serde(default)]
    pub format: Option<String>,
}

/// `player_build[]`: files built once on the player's PC (docs/RECIPE-FORMAT.md section 4, "Player build").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerBuild {
    pub id: String,
    #[serde(default)]
    pub label: String,
    /// The install step (game id) whose folder receives the outputs.
    pub step: String,
    /// Ids of the app's own pinned toolchain table (`install::tools::TOOLS`), nothing else.
    pub toolchain: Vec<String>,
    /// The build script, a release asset of the mashup's own SIGFAI repo, run with the toolchain's `sh`.
    pub script: BuildFile,
    /// Pinned sources the script uses, fetched by the app (commit-pinned GitHub archives or raw files).
    #[serde(default)]
    pub inputs: Vec<BuildFile>,
    /// Files the script leaves in `$SIGF_OUT`, and where each goes.
    pub outputs: Vec<BuildOutput>,
    /// About how long the build takes, for the UI.
    #[serde(default)]
    pub minutes: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildFile {
    pub name: String,
    pub url: String,
    pub sha256: String,
    #[serde(default)]
    pub size: Option<u64>,
    /// Inputs only: a zip extracted into `$SIGF_IN/<name>/`.
    #[serde(default)]
    pub unpack: bool,
    /// With `unpack`: only this folder of the zip, its prefix stripped (GitHub archives wrap everything in one).
    #[serde(default)]
    pub root: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildOutput {
    pub name: String,
    pub to: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Mod,
    Mashup,
    Passthrough,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameReq {
    /// Canonical game id (`gta5`, `minecraft`, ...), see catalog/games.json.
    pub game: String,
    #[serde(default)]
    pub role: Option<String>,
    /// Store -> the game's id in that store (`{"steam": "440"}`).
    #[serde(default)]
    pub apps: HashMap<String, String>,
    /// Store -> build ids the mod is known to run on (Steam buildid, Epic AppVersionString).
    #[serde(default)]
    pub builds: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub mc: Option<String>,
    #[serde(default)]
    pub loader: Option<String>,
}

/// A third-party piece the mod needs. With a `source` the app fetches and verifies it like any install file;
/// without one it is a prerequisite the player (or the kit's own runtime layer) provides, listed for the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Requirement {
    pub id: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub source: Option<Source>,
    /// Official download page of a prerequisite that is not shipped (SKSE64, F4SE): named in errors that need it.
    #[serde(default)]
    pub page: Option<String>,
}

/// A downloadable blob pinned by hash. `url` must pass `check::UrlPolicy` (a release asset of the recipe's own SIGFAI
/// repo, or of its pinned upstream release); `file://` and local paths only in dev mode (`check::DEV_LOCAL_ENV`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub url: String,
    pub sha256: String,
    /// Bytes, when the recipe gives it: the download may not be larger.
    #[serde(default)]
    pub size: Option<u64>,
}

/// One release asset: `recipe.files[]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub sha256: String,
    #[serde(default)]
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Strategy {
    Args,
    Mrpack,
    Profile,
    GameDirSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    pub game: String,
    pub strategy: Strategy,
    #[serde(default)]
    pub files: Vec<FileSpec>,
    /// `mrpack` only.
    #[serde(default)]
    pub pack: Option<Source>,
    /// `mrpack` only: extra JVM arguments for the Prism instance (`-Dkey=value`, `-Xmx4G`; see `mrpack::jvm_arg_ok`).
    #[serde(default)]
    pub jvm_args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSpec {
    /// The release asset's name (`recipe.files[].name`).
    pub src: String,
    /// Starts with a root placeholder: `{app}`, `{game}`, `{docs}` or `{fivem}` (see `paths::Root`). A plain file
    /// goes to that exact path; with `unpack` the zip is extracted into that folder (`{game}` alone = the game
    /// folder). Defaults to `{app}/<file name of src>`.
    #[serde(default)]
    pub dst: Option<String>,
    pub sha256: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    /// The file is a zip to extract into `dst` instead of a file to copy to `dst`.
    #[serde(default)]
    pub unpack: bool,
    /// `unpack` only: every entry of the zip with its sha256. When given, the extracted files must match it exactly.
    #[serde(default)]
    pub contents: Vec<Content>,
    /// `unpack` only: a folder inside the zip (`Fusion/red4ext`). Only the entries under it are placed, with
    /// that prefix stripped, into `dst`; the rest are verified against `contents` like any entry but not written.
    #[serde(default)]
    pub root: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Content {
    pub path: String,
    pub sha256: String,
}

impl FileSpec {
    /// Where to fetch this file from: its own `url`, else the recipe asset named `src`, else `src` as is
    /// (a local path or URL). Every location is checked by `check::UrlPolicy` before anything is fetched.
    pub fn location(&self, assets: &[Asset]) -> String {
        if let Some(u) = &self.url {
            return u.clone();
        }
        match assets.iter().find(|a| a.name == self.src) {
            Some(a) => a.url.clone(),
            None => self.src.clone(),
        }
    }

    /// The size the recipe declares for this file: its own `size`, else the asset's.
    pub fn declared_size(&self, assets: &[Asset]) -> Option<u64> {
        self.size.or_else(|| if self.url.is_none() { assets.iter().find(|a| a.name == self.src).and_then(|a| a.size) } else { None })
    }

    /// Last path segment of `src`, the default install name.
    pub fn file_name(&self) -> &str {
        self.src.rsplit(['/', '\\']).next().unwrap_or(&self.src)
    }

    /// `dst`, or `{app}/<file name>` when the recipe leaves it out.
    pub fn target(&self) -> String {
        self.dst.clone().unwrap_or_else(|| format!("{{app}}/{}", self.file_name()))
    }
}

/// How to start one game once installed. `args` may use the root placeholders (`-file {app}/mod.pk3`); the engine
/// returns them resolved to absolute paths in `InstalledGame::launch_args`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchStep {
    pub game: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// `port:<n>`: the next game starts once that local port answers (see `crate::launch`).
    #[serde(default)]
    pub wait: Option<String>,
    /// A program in `{game}` to start instead of the store's launch (`skse64_loader.exe`): games whose mod loads only
    /// through a script extender. Checked to stay inside the game folder.
    #[serde(default)]
    pub exe: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoSource {
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    /// Upstream fusions: the SIGFAI repo hosting the recipe (`repo` then credits the upstream repo).
    #[serde(default)]
    pub hosted: Option<String>,
    /// `"upstream"`: files may come from `<repo>/releases/download/<tag>/` (docs/RECIPE-FORMAT.md section 4, "Upstream fetch").
    #[serde(default)]
    pub fetch: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
    /// The upstream commit an upstream fusion pins.
    #[serde(default)]
    pub commit: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Media {
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(default)]
    pub clip: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuiltBy {
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    /// Upstream fusions: the upstream author credited.
    #[serde(default)]
    pub author: Option<String>,
}

impl Recipe {
    pub fn parse(json: &str) -> Result<Self, InstallError> {
        let r: Recipe = serde_json::from_str(json).map_err(|e| InstallError::recipe(e.to_string()))?;
        if r.id.trim().is_empty() || super::paths::slug(&r.id).is_empty() {
            return Err(InstallError::recipe("recipe id is empty"));
        }
        Ok(r)
    }

    pub fn slug(&self) -> String {
        super::paths::slug(&self.id)
    }

    /// Where to get a launch `exe` that is missing: the `requires` entry it belongs to (`skse64_loader.exe` ->
    /// `skse64`, by file name prefix) with its official page, as `install SKSE64 from skse.silverlock.org`.
    pub fn exe_hint(&self, exe: &str) -> Option<String> {
        let name = exe.rsplit(['/', '\\']).next().unwrap_or(exe).to_ascii_lowercase();
        let r = self
            .requires
            .iter()
            .filter(|r| r.page.is_some() && !r.id.is_empty())
            .find(|r| name.starts_with(&r.id.to_ascii_lowercase()))?;
        let page = r.page.as_deref()?;
        let page = page.trim_start_matches("https://").trim_start_matches("http://").trim_end_matches('/');
        Some(format!("install {} from {page}", r.id.to_ascii_uppercase()))
    }
}
