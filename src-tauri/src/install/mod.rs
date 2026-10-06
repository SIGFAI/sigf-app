//! Install engine: recipe -> verified downloads -> one strategy per game -> registry entry that knows how to undo it.
//! Contract: docs/RECIPE-FORMAT.md sections 1, 3, 4. Everything lives under one base dir (`SIGF_HOME`, default
//! `%LOCALAPPDATA%\SIGF`): `cache/`, `profiles/`, `snapshots/`, `installed.json`.

pub mod build;
pub mod byo;
pub mod check;
pub mod fetch;
pub mod mrpack;
pub mod paths;
pub mod recipe;
pub mod registry;
pub mod snapshot;
pub mod tools;

pub use mrpack::Prism;
pub use recipe::{Recipe, Strategy};
pub use registry::{InstalledGame, InstalledMod, LaunchExe};

use paths::{ensure_real_parent_inside, path_string, resolve_inside, Root};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Typed so the UI can react (offer Prism, show tampered files) instead of parsing messages.
#[derive(Debug, Clone, Serialize, thiserror::Error)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum InstallError {
    #[error("bad recipe: {message}")]
    Recipe { message: String },
    #[error("download failed for {url}: {message}")]
    Download { url: String, message: String },
    #[error("sha256 mismatch for {file}: expected {expected}, got {actual}")]
    ShaMismatch { file: String, expected: String, actual: String },
    #[error("{game} needs the {launcher} launcher")]
    NeedsLauncher { game: String, launcher: String },
    #[error("no install folder known for {game}")]
    MissingGameDir { game: String },
    #[error("destination escapes its folder: {dst}")]
    PathTraversal { dst: String },
    #[error("files changed since install: {}", files.join(", "))]
    Tampered { files: Vec<String> },
    #[error("snapshot copies are damaged: {}", files.join(", "))]
    SnapshotCorrupt { files: Vec<String> },
    #[error("{id} is not installed")]
    NotInstalled { id: String },
    #[error("{path}: {message}")]
    Io { path: String, message: String },
    /// The recipe uses the player's own copy of a game file, and none was found or picked.
    #[error("uses your own copy of {label}: SIGF never ships or downloads it, pick your file first")]
    OwnCopyMissing { game: String, label: String },
    /// The file found or picked is not a dump the recipe accepts (another region, a modified or bad dump).
    #[error("{file} is not a {label} dump this mashup accepts ({sha1}): pick a clean, unmodified dump")]
    OwnCopyMismatch { game: String, label: String, file: String, sha1: String },
    /// A player build failed; `log` is the saved build log.
    #[error("building {label} failed: {message}")]
    BuildFailed { id: String, label: String, message: String, log: Option<String> },
}

impl InstallError {
    pub fn recipe(m: impl Into<String>) -> Self {
        Self::Recipe { message: m.into() }
    }
    pub fn io(path: &Path, e: impl std::fmt::Display) -> Self {
        Self::Io { path: path_string(path), message: e.to_string() }
    }
}

/// What a Tauri command returns on error: the typed error plus a readable `message`.
#[derive(Debug, Serialize)]
pub struct CommandError {
    #[serde(flatten)]
    pub error: InstallError,
    pub message: String,
}

impl From<InstallError> for CommandError {
    fn from(error: InstallError) -> Self {
        Self { message: error.to_string(), error }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Download,
    Verify,
    /// A player build runs (toolchain, then the script).
    Build,
    Install,
    Ready,
}

#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub id: String,
    pub phase: Phase,
    /// Overall 0..=100 across all phases, so the button can show one number.
    pub pct: u8,
}

/// `SIGF_HOME`, else `%LOCALAPPDATA%\SIGF`.
pub fn home_dir() -> PathBuf {
    if let Some(h) = std::env::var_os("SIGF_HOME").filter(|h| !h.is_empty()) {
        return PathBuf::from(h);
    }
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("SIGF")
}
/// The player's Documents folder (`{docs}`): `%USERPROFILE%\Documents`, else `$HOME/Documents`.
pub fn docs_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .filter(|h| !h.is_empty())
        .map(|h| PathBuf::from(h).join("Documents"))
}

pub struct Engine {
    pub home: PathBuf,
    pub prism: Option<Prism>,
    /// What `{docs}` resolves to (default: `docs_dir()`).
    pub docs: Option<PathBuf>,
    /// Dev mode: recipes may name `file://` URLs and local paths. Default: `check::dev_local_recipes()` (the
    /// `SIGF_DEV_LOCAL_RECIPES=1` flag of the example CLI and the tests); the app's own handlers set it false.
    pub allow_local: bool,
    /// The player's own copies for this install, by `own_copies[].game`: found by `byo::search` or picked by the
    /// player. Each one is checked against the recipe's SHA-1s before anything is downloaded.
    pub own: HashMap<String, byo::OwnSource>,
    /// Dev mode only (`allow_local`): toolchain ids -> a local folder used instead of the pinned download (tests).
    pub tool_dirs: HashMap<String, PathBuf>,
}

/// Share of the bar given to fetching; the rest is install.
const FETCH_PCT: f64 = 70.0;
/// Share of the bar given to player builds, after fetching (when the recipe has any).
const BUILD_PCT: f64 = 15.0;

struct Reporter<'a> {
    id: String,
    last: Option<(Phase, u8)>,
    sink: &'a mut dyn FnMut(Progress),
}

impl Reporter<'_> {
    fn emit(&mut self, phase: Phase, pct: f64) {
        let pct = pct.clamp(0.0, 100.0) as u8;
        if self.last != Some((phase, pct)) {
            self.last = Some((phase, pct));
            (self.sink)(Progress { id: self.id.clone(), phase, pct });
        }
    }
}

/// `rest` + `/` + `child`, either side possibly empty.
fn join_rel(rest: &str, child: &str) -> String {
    match (rest.is_empty(), child.is_empty()) {
        (true, _) => child.to_string(),
        (_, true) => rest.to_string(),
        _ => format!("{rest}/{child}"),
    }
}

/// `{name}` left in a launch arg once the known roots are replaced: a recipe error, never passed to a game.
fn leftover_placeholder(s: &str) -> Option<&str> {
    let start = s.find('{')?;
    let end = start + s[start..].find('}')?;
    let inner = &s[start + 1..end];
    (!inner.is_empty() && inner.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')).then(|| &s[start..=end])
}

/// One install file once its destination is parsed: root, and the path under it ("" = the root itself).
struct Placed<'a> {
    spec: &'a recipe::FileSpec,
    root: Root,
    rest: String,
}

/// What the checks before the download worked out for one step.
struct StepPlan<'a> {
    files: Vec<Placed<'a>>,
    /// The one snapshot-tracked root this step writes into, with its folder.
    outside: Option<(Root, PathBuf)>,
    launch_args: Vec<String>,
    exe: Option<LaunchExe>,
    wait: Option<String>,
}

impl Engine {
    pub fn new(home: impl Into<PathBuf>, prism: Option<Prism>) -> Self {
        Self { home: home.into(), prism, docs: docs_dir(), allow_local: check::dev_local_recipes(), own: HashMap::new(), tool_dirs: HashMap::new() }
    }

    pub fn from_env(prism: Option<Prism>) -> Self {
        Self::new(home_dir(), prism)
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.home.join("cache")
    }
    fn profile_dir(&self, slug: &str, game: &str) -> PathBuf {
        self.home.join("profiles").join(slug).join(paths::slug(game))
    }
    fn snapshot_dir(&self, slug: &str, game: &str) -> PathBuf {
        self.home.join("snapshots").join(slug).join(paths::slug(game))
    }
    fn staging_dir(&self, slug: &str) -> PathBuf {
        self.home.join("staging").join(slug)
    }
    fn build_dir(&self, slug: &str) -> PathBuf {
        self.home.join("build").join(slug)
    }

    pub fn installed(&self) -> Vec<InstalledMod> {
        registry::load(&self.home)
    }

    /// The folder a root placeholder stands for, for one game of one mashup.
    fn root_dir(&self, root: Root, slug: &str, game: &str, game_dirs: &HashMap<String, String>) -> Result<PathBuf, InstallError> {
        match root {
            Root::App => Ok(self.profile_dir(slug, game)),
            Root::Game => self.game_dir(game_dirs, game),
            Root::Docs => self.docs.clone().filter(|d| d.is_dir()).ok_or_else(|| InstallError::MissingGameDir { game: "docs".into() }),
            Root::Fivem => self.game_dir(game_dirs, "fivem"),
        }
    }

    /// Launch args with every root placeholder replaced by its absolute folder. An arg that starts with one is a
    /// path (`{app}/mod.pk3`), checked to stay inside that folder and written with the OS separators.
    fn resolve_args(&self, args: &[String], slug: &str, game: &str, game_dirs: &HashMap<String, String>) -> Result<Vec<String>, InstallError> {
        let mut out = vec![];
        for a in args {
            let resolved = if let Ok((root, rest)) = paths::split_root(a) {
                let dir = self.root_dir(root, slug, game, game_dirs)?;
                if rest.is_empty() {
                    path_string(&dir)
                } else {
                    path_string(&resolve_inside(&dir, "", &rest)?.0)
                }
            } else {
                let mut s = a.clone();
                for root in Root::ALL {
                    if s.contains(root.token()) {
                        s = s.replace(root.token(), &path_string(&self.root_dir(root, slug, game, game_dirs)?));
                    }
                }
                s
            };
            if let Some(p) = leftover_placeholder(&resolved) {
                return Err(InstallError::recipe(format!("unknown placeholder {p} in launch arg {a}")));
            }
            out.push(resolved);
        }
        Ok(out)
    }

    /// Everything about one step checkable without the network: destinations, roots present, launch args.
    fn plan_step<'a>(
        &self,
        recipe: &Recipe,
        slug: &str,
        step: &'a recipe::Step,
        game_dirs: &HashMap<String, String>,
    ) -> Result<StepPlan<'a>, InstallError> {
        let mut plan = StepPlan { files: vec![], outside: None, launch_args: vec![], exe: None, wait: None };
        if step.strategy == Strategy::Mrpack {
            if self.prism.is_none() {
                return Err(InstallError::NeedsLauncher { game: step.game.clone(), launcher: "prism".into() });
            }
            if step.pack.is_none() {
                return Err(InstallError::recipe(format!("mrpack step for {} has no pack", step.game)));
            }
            if step.jvm_args.len() > mrpack::JVM_ARGS_MAX {
                return Err(InstallError::recipe(format!("too many jvm_args for {}", step.game)));
            }
            if let Some(a) = step.jvm_args.iter().find(|a| !mrpack::jvm_arg_ok(a)) {
                return Err(InstallError::recipe(format!("jvm arg not allowed: {a}")));
            }
        } else {
            if !step.jvm_args.is_empty() {
                return Err(InstallError::recipe(format!("jvm_args on the {} step, which is not mrpack", step.game)));
            }
            if step.files.is_empty() {
                return Err(InstallError::recipe(format!("install step for {} has no files", step.game)));
            }
            for f in &step.files {
                let dst = f.target();
                let (root, rest) = paths::split_root(&dst)?;
                let dir = self.root_dir(root, slug, &step.game, game_dirs)?;
                if rest.is_empty() {
                    if !f.unpack {
                        return Err(InstallError::PathTraversal { dst });
                    }
                } else {
                    resolve_inside(&dir, "", &rest)?;
                }
                if let Some(r) = &f.root {
                    if !f.unpack {
                        return Err(InstallError::recipe(format!("{} has a root but is not unpacked", f.src)));
                    }
                    if !paths::plain_rel(r) {
                        return Err(InstallError::PathTraversal { dst: r.clone() });
                    }
                    if !f.contents.is_empty() && !f.contents.iter().any(|c| under_root(&c.path.replace('\\', "/"), r).is_some()) {
                        return Err(InstallError::recipe(format!("{} has nothing under {r}/", f.src)));
                    }
                }
                for c in &f.contents {
                    let path = c.path.replace('\\', "/");
                    let placed = match &f.root {
                        Some(r) => match under_root(&path, r) {
                            Some(p) => p,
                            None => continue, // verified, not written
                        },
                        None => &path,
                    };
                    resolve_inside(&dir, "", &join_rel(&rest, placed))?;
                }
                if root != Root::App {
                    match &plan.outside {
                        Some((r, _)) if *r != root => {
                            return Err(InstallError::recipe(format!(
                                "the {} step writes into both {} and {}",
                                step.game,
                                r.token(),
                                root.token()
                            )))
                        }
                        _ => plan.outside = Some((root, dir)),
                    }
                }
                plan.files.push(Placed { spec: f, root, rest });
            }
        }
        if let Some(l) = recipe.launch.iter().find(|l| l.game == step.game) {
            plan.launch_args = self.resolve_args(&l.args, slug, &step.game, game_dirs)?;
            if let Some(w) = &l.wait {
                crate::launch::parse_wait(w).ok_or_else(|| InstallError::recipe(format!("bad launch wait for {}: {w}", step.game)))?;
                plan.wait = Some(w.clone());
            }
            if let Some(x) = &l.exe {
                plan.exe = Some(self.launch_exe(recipe, step, x, game_dirs)?);
            }
        }
        Ok(plan)
    }

    /// A launch step's `exe`: a `.exe` inside the scanned game folder (`{game}/` prefix optional). It may be missing
    /// at install (the player adds SKSE64 later); `crate::launch::resolve_exe` checks it again at play.
    fn launch_exe(&self, recipe: &Recipe, step: &recipe::Step, exe: &str, game_dirs: &HashMap<String, String>) -> Result<LaunchExe, InstallError> {
        let game = &step.game;
        if step.strategy == Strategy::Mrpack {
            return Err(InstallError::recipe(format!("launch exe for {game}, which starts through Prism")));
        }
        let dir = self.game_dir(game_dirs, game)?;
        let (_, rel) = resolve_inside(&dir, Root::Game.token(), exe)?;
        if !rel.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")) {
            return Err(InstallError::recipe(format!("launch exe for {game} is not a .exe: {exe}")));
        }
        Ok(LaunchExe { path: paths::rel_string(&rel), dir: path_string(&dir), hint: recipe.exe_hint(exe) })
    }

    /// Installs `recipe`. `game_dirs` maps canonical game id -> scanned install folder (`{game}`), plus `fivem` ->
    /// the FiveM server data folder (`{fivem}`). Re-installing an installed id restores it first. Any failure rolls
    /// back the games already done, so the player is never left half-modded.
    pub fn install(
        &self,
        recipe: &Recipe,
        game_dirs: &HashMap<String, String>,
        on_progress: &mut dyn FnMut(Progress),
    ) -> Result<InstalledMod, InstallError> {
        let slug = recipe.slug();
        if slug.is_empty() {
            return Err(InstallError::recipe("recipe id is empty"));
        }
        let mut rep = Reporter { id: recipe.id.clone(), last: None, sink: on_progress };

        // Everything checkable without the network, before the first byte is downloaded.
        let mut seen_games = std::collections::HashSet::new();
        let mut plans = vec![];
        for step in &recipe.install {
            if !seen_games.insert(step.game.as_str()) {
                return Err(InstallError::recipe(format!("two install steps for {}", step.game)));
            }
            plans.push(self.plan_step(recipe, &slug, step, game_dirs)?);
        }

        // Bring your own copy: every copy the recipe needs is on this PC and is a dump it accepts, before any download.
        self.check_byo(recipe)?;

        // Every download must be allowed (check::UrlPolicy) and within the size cap before the first one starts.
        let policy = check::UrlPolicy::for_recipe(recipe, self.allow_local);
        let jobs = check::planned_downloads(recipe);
        for (url, _, size) in &jobs {
            policy.check(url, false)?;
            if size.is_some_and(|s| s > check::MAX_FILE_BYTES) {
                return Err(InstallError::recipe(format!("{url} is larger than {} bytes", check::MAX_FILE_BYTES)));
            }
        }
        for (url, _, size, script) in check::planned_build_downloads(recipe) {
            policy.check_build(&url, script)?;
            let max = if script { check::BUILD_SCRIPT_MAX_BYTES } else { check::BUILD_INPUT_MAX_BYTES };
            if !size.is_some_and(|s| s <= max) {
                return Err(InstallError::recipe(format!("{url}: a build file needs its size, at most {max} bytes")));
            }
        }

        // Fetch + verify every blob, keyed by sha256 so strategies can find their cache path.
        let n = jobs.len().max(1) as f64;
        let mut fetched: HashMap<String, PathBuf> = HashMap::new();
        for (i, (url, sha, size)) in jobs.iter().enumerate() {
            let base = i as f64 / n * FETCH_PCT;
            rep.emit(Phase::Download, base);
            let opts = fetch::FetchOpts::new(self.allow_local, *size);
            let got = fetch::fetch(&self.cache_dir(), url, sha, &opts, &mut |done, total| {
                if let Some(t) = total.filter(|t| *t > 0) {
                    rep.emit(Phase::Download, base + done as f64 / t as f64 / n * FETCH_PCT * 0.95);
                }
            })?;
            rep.emit(Phase::Verify, (i + 1) as f64 / n * FETCH_PCT);
            fetched.insert(sha.to_ascii_lowercase(), got.path);
        }
        let cached = |sha: &str| fetched.get(&sha.to_ascii_lowercase()).cloned().expect("fetched above");

        // Player builds run before anything is installed: a failed build leaves the PC as it was.
        let built = match self.run_builds(recipe, &slug, &mut rep) {
            Ok(b) => b,
            Err(e) => {
                let _ = std::fs::remove_dir_all(self.build_dir(&slug));
                return Err(e);
            }
        };

        if self.installed().iter().any(|m| m.id == recipe.id) {
            if let Err(e) = self.restore(&recipe.id, false) {
                let _ = std::fs::remove_dir_all(self.build_dir(&slug));
                return Err(e);
            }
        }

        let steps = recipe.install.len().max(1) as f64;
        let mut done: Vec<InstalledGame> = vec![];
        for (i, (step, plan)) in recipe.install.iter().zip(&plans).enumerate() {
            let built_pct = if recipe.player_build.is_empty() { 0.0 } else { BUILD_PCT };
            rep.emit(Phase::Install, FETCH_PCT + built_pct + i as f64 / steps * (99.0 - FETCH_PCT - built_pct));
            let r = match step.strategy {
                Strategy::Mrpack => self.install_mrpack(&slug, &recipe.id, step, &cached(&step.pack.as_ref().unwrap().sha256), &policy),
                _ => self.install_files(&slug, step, plan, &cached),
            };
            match r {
                Ok(mut g) => {
                    g.launch_args.extend(plan.launch_args.iter().cloned());
                    g.exe = plan.exe.clone();
                    g.wait = plan.wait.clone();
                    done.push(g);
                }
                Err(e) => {
                    for g in &done {
                        let _ = self.undo_game(&recipe.id, g, true);
                    }
                    let _ = std::fs::remove_dir_all(self.home.join("profiles").join(&slug));
                    let _ = std::fs::remove_dir_all(self.staging_dir(&slug));
                    let _ = std::fs::remove_dir_all(self.build_dir(&slug));
                    return Err(e);
                }
            }
        }
        // The player's own copies and the built files go into the folders just written ({instance}, {app}).
        let placed = self.place_byo(recipe, &slug, &done, &built);
        let _ = std::fs::remove_dir_all(self.build_dir(&slug));
        let _ = std::fs::remove_dir(self.home.join("build"));
        let placed = match placed {
            Ok(p) => p,
            Err(e) => {
                for g in &done {
                    let _ = self.undo_game(&recipe.id, g, true);
                }
                let _ = std::fs::remove_dir_all(self.home.join("profiles").join(&slug));
                let _ = std::fs::remove_dir_all(self.staging_dir(&slug));
                return Err(e);
            }
        };
        let _ = std::fs::remove_dir(self.staging_dir(&slug));
        let _ = std::fs::remove_dir(self.home.join("staging"));

        let entry = InstalledMod {
            id: recipe.id.clone(),
            version: recipe.version.clone(),
            name: recipe.name.clone(),
            installed_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            games: done,
            placed,
        };
        let mut all = self.installed();
        all.retain(|m| m.id != recipe.id);
        all.push(entry.clone());
        if let Err(e) = registry::save(&self.home, &all) {
            remove_placed(&entry.placed);
            for g in &entry.games {
                let _ = self.undo_game(&recipe.id, g, true);
            }
            return Err(e);
        }
        rep.emit(Phase::Ready, 100.0);
        Ok(entry)
    }

    fn game_dir(&self, game_dirs: &HashMap<String, String>, game: &str) -> Result<PathBuf, InstallError> {
        game_dirs
            .get(game)
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
            .ok_or_else(|| InstallError::MissingGameDir { game: game.to_string() })
    }

    /// `args`, `profile` and `game-dir-snapshot`: `{app}` files go into our own folder (deleted on restore); files
    /// under `{game}`, `{docs}` or `{fivem}` go through a snapshot of that folder, whatever the strategy, so every
    /// write outside the app is undone exactly. Within a step a later file wins over an earlier one on the same path.
    fn install_files(
        &self,
        slug: &str,
        step: &recipe::Step,
        plan: &StepPlan,
        cached: &dyn Fn(&str) -> PathBuf,
    ) -> Result<InstalledGame, InstallError> {
        let staging = self.staging_dir(slug).join(paths::slug(&step.game));
        let _ = std::fs::remove_dir_all(&staging);
        let r = self.install_files_in(slug, step, plan, cached, &staging);
        let _ = std::fs::remove_dir_all(&staging);
        r
    }

    fn install_files_in(
        &self,
        slug: &str,
        step: &recipe::Step,
        plan: &StepPlan,
        cached: &dyn Fn(&str) -> PathBuf,
        staging: &Path,
    ) -> Result<InstalledGame, InstallError> {
        let app = self.profile_dir(slug, &step.game);
        let has_app = plan.files.iter().any(|p| p.root == Root::App);
        if has_app {
            if app.exists() {
                std::fs::remove_dir_all(&app).map_err(|e| InstallError::io(&app, e))?;
            }
            std::fs::create_dir_all(&app).map_err(|e| InstallError::io(&app, e))?;
        }
        let mut outside: Vec<snapshot::Planned> = vec![];
        for (i, p) in plan.files.iter().enumerate() {
            let f = p.spec;
            let src = cached(&f.sha256);
            if p.root == Root::App {
                if f.unpack {
                    let target = if p.rest.is_empty() { app.clone() } else { resolve_inside(&app, "", &p.rest)?.0 };
                    if f.root.is_none() {
                        let got = extract_zip(&src, &target)?;
                        verify_contents(f, &got)?;
                    } else {
                        // The whole zip is extracted and verified aside, then only its root folder is copied in.
                        let got = extract_zip(&src, &staging.join(i.to_string()))?;
                        verify_contents(f, &got)?;
                        for (rel, x) in placed(f, &got)? {
                            let (abs, _) = resolve_inside(&target, "", &rel)?;
                            if let Some(parent) = abs.parent() {
                                std::fs::create_dir_all(parent).map_err(|e| InstallError::io(parent, e))?;
                            }
                            ensure_real_parent_inside(&app, &abs, &f.target())?;
                            std::fs::copy(&x.abs, &abs).map_err(|e| InstallError::io(&abs, e))?;
                        }
                    }
                } else {
                    let (abs, _) = resolve_inside(&app, "", &p.rest)?;
                    if let Some(parent) = abs.parent() {
                        std::fs::create_dir_all(parent).map_err(|e| InstallError::io(parent, e))?;
                    }
                    ensure_real_parent_inside(&app, &abs, &f.target())?;
                    std::fs::copy(&src, &abs).map_err(|e| InstallError::io(&abs, e))?;
                }
            } else if f.unpack {
                let got = extract_zip(&src, &staging.join(i.to_string()))?;
                verify_contents(f, &got)?;
                for (rel, x) in placed(f, &got)? {
                    outside.push(snapshot::Planned {
                        dst: format!("{}/{}", snapshot::PLACEHOLDER, join_rel(&p.rest, &rel)),
                        sha256: x.sha256.clone(),
                        src: x.abs.clone(),
                    });
                }
            } else {
                outside.push(snapshot::Planned { src, dst: format!("{}/{}", snapshot::PLACEHOLDER, p.rest), sha256: f.sha256.clone() });
            }
        }
        // Later files win: keep the last entry for every path (case-insensitive, like Windows).
        let mut seen = std::collections::HashSet::new();
        let mut kept: Vec<snapshot::Planned> = outside.into_iter().rev().filter(|p| seen.insert(p.dst.to_ascii_lowercase())).collect();
        kept.reverse();

        let mut g = InstalledGame {
            game: step.game.clone(),
            strategy: step.strategy,
            launch_args: vec![],
            instance: None,
            profile_dir: has_app.then(|| path_string(&app)),
            snapshot: None,
            game_dir: None,
            launcher: None,
            exe: None,
            wait: None,
        };
        if let (Some((_, dir)), false) = (&plan.outside, kept.is_empty()) {
            let snap = self.snapshot_dir(slug, &step.game);
            snapshot::apply(&snap, dir, &kept)?;
            g.snapshot = Some(path_string(&snap));
            g.game_dir = Some(path_string(dir));
        }
        Ok(g)
    }

    fn install_mrpack(
        &self,
        slug: &str,
        id: &str,
        step: &recipe::Step,
        pack: &Path,
        policy: &check::UrlPolicy,
    ) -> Result<InstalledGame, InstallError> {
        let prism = self.prism.as_ref().expect("checked before download");
        let launcher = prism.exe.as_deref().map(path_string);
        match mrpack::write_instance(&prism.data_dir, slug, id, pack, &step.jvm_args, &self.cache_dir(), policy, &mut |_, _| {}) {
            Ok(w) => Ok(InstalledGame {
                game: step.game.clone(),
                strategy: Strategy::Mrpack,
                launch_args: mrpack::launch_args(&w.instance),
                instance: Some(w.instance),
                profile_dir: Some(path_string(&w.dir)),
                snapshot: None,
                game_dir: None,
                launcher,
                exe: None,
                wait: None,
            }),
            // Only a folder we could not write falls back to Prism's importer; a bad pack or hash stays an error.
            Err(InstallError::Io { path, message }) => {
                let Some(exe) = prism.exe.as_deref() else {
                    return Err(InstallError::Io { path, message });
                };
                let dir = self.profile_dir(slug, &step.game);
                std::fs::create_dir_all(&dir).map_err(|e| InstallError::io(&dir, e))?;
                let named = dir.join(format!("{slug}.mrpack")); // Prism picks the importer by extension
                std::fs::copy(pack, &named).map_err(|e| InstallError::io(&named, e))?;
                mrpack::import(exe, &named)?;
                Ok(InstalledGame {
                    game: step.game.clone(),
                    strategy: Strategy::Mrpack,
                    launch_args: vec![],
                    instance: None,
                    profile_dir: None,
                    snapshot: None,
                    game_dir: None,
                    launcher,
                    exe: None,
                    wait: None,
                })
            }
            Err(e) => Err(e),
        }
    }

    fn undo_game(&self, id: &str, g: &InstalledGame, force: bool) -> Result<(), InstallError> {
        if let Some(s) = &g.snapshot {
            let s = Path::new(s);
            if s.join("manifest.json").exists() {
                snapshot::restore(s, force)?;
            }
        }
        if let Some(p) = &g.profile_dir {
            let p = Path::new(p);
            if g.strategy == Strategy::Mrpack {
                mrpack::remove_instance(p, id)?;
            } else if p.exists() {
                std::fs::remove_dir_all(p).map_err(|e| InstallError::io(p, e))?;
            }
        }
        Ok(())
    }

    /// "Restore vanilla". Checks every snapshotted game for changes before touching any of them, so it refuses
    /// cleanly (Tampered) instead of half-restoring. Also cleans up leftovers of a crashed install with no entry.
    pub fn restore(&self, id: &str, force: bool) -> Result<(), InstallError> {
        let slug = paths::slug(id);
        let mut all = self.installed();
        let games = match all.iter().find(|m| m.id == id) {
            Some(m) => m.games.clone(),
            None => self.orphans(id, &slug),
        };
        let placed = all.iter().find(|m| m.id == id).map(|m| m.placed.clone()).unwrap_or_default();
        if games.is_empty() && !all.iter().any(|m| m.id == id) {
            return Err(InstallError::NotInstalled { id: id.to_string() });
        }
        if !force {
            let mut t = vec![];
            for g in &games {
                if let Some(s) = g.snapshot.as_deref().map(Path::new).filter(|s| s.join("manifest.json").exists()) {
                    t.extend(snapshot::tampered(s)?.into_iter().map(|f| format!("{}: {f}", g.game)));
                }
            }
            if !t.is_empty() {
                return Err(InstallError::Tampered { files: t });
            }
        }
        // The player's own copies and the files built on this PC go first (they also live in folders deleted below).
        remove_placed(&placed);
        for g in &games {
            self.undo_game(id, g, true)?;
        }
        let _ = std::fs::remove_dir_all(self.home.join("profiles").join(&slug));
        let _ = std::fs::remove_dir_all(self.build_dir(&slug));
        let _ = std::fs::remove_dir(self.home.join("snapshots").join(&slug));
        let _ = std::fs::remove_dir_all(self.staging_dir(&slug));
        all.retain(|m| m.id != id);
        registry::save(&self.home, &all)
    }

    /// `own_copies` and `player_build` checks that need no network: every copy is on this PC (`self.own`) and accepted
    /// by its SHA-1, every step and tool named exists, every destination is one Restore deletes.
    fn check_byo(&self, recipe: &Recipe) -> Result<(), InstallError> {
        let step = |game: &str| recipe.install.iter().find(|s| s.game == game);
        let to_ok = |to: &str, game: &str| -> Result<(), InstallError> {
            let s = step(game).ok_or_else(|| InstallError::recipe(format!("no install step for {game}")))?;
            if check::byo_to_ok(to, s.strategy == Strategy::Mrpack) {
                Ok(())
            } else {
                Err(InstallError::PathTraversal { dst: to.to_string() })
            }
        };
        if recipe.own_copies.len() > check::OWN_COPIES_MAX || recipe.player_build.len() > check::PLAYER_BUILDS_MAX {
            return Err(InstallError::recipe("too many own copies or player builds"));
        }
        for c in &recipe.own_copies {
            to_ok(&c.to, &c.step)?;
            if !check::file_name_ok(&c.rom.save_as) {
                return Err(InstallError::PathTraversal { dst: c.rom.save_as.clone() });
            }
            let src = self.own.get(&c.game).ok_or_else(|| InstallError::OwnCopyMissing { game: c.game.clone(), label: c.label.clone() })?;
            byo::verify(c, src)?;
        }
        for b in &recipe.player_build {
            if b.toolchain.iter().any(|t| tools::find(t).is_none()) {
                return Err(InstallError::recipe(format!("unknown tool in {}", b.id)));
            }
            if !check::file_name_ok(&b.script.name) || b.inputs.iter().any(|i| !check::file_name_ok(&i.name)) {
                return Err(InstallError::recipe(format!("bad file name in {}", b.id)));
            }
            for o in &b.outputs {
                to_ok(&o.to, &b.step)?;
                if !check::file_name_ok(&o.name) {
                    return Err(InstallError::PathTraversal { dst: o.name.clone() });
                }
            }
        }
        Ok(())
    }

    /// Runs every player build (toolchain, then staged inputs, then the script). Returns each build's output folder.
    fn run_builds(&self, recipe: &Recipe, slug: &str, rep: &mut Reporter) -> Result<Vec<PathBuf>, InstallError> {
        let mut out = vec![];
        let n = recipe.player_build.len().max(1) as f64;
        for (i, b) in recipe.player_build.iter().enumerate() {
            let base = FETCH_PCT + i as f64 / n * BUILD_PCT;
            rep.emit(Phase::Build, base);
            let overrides = if self.allow_local { self.tool_dirs.clone() } else { HashMap::new() };
            let ready = build::toolchain(&self.home, b, &overrides, &mut |done, total| {
                if let Some(t) = total.filter(|t| *t > 0) {
                    rep.emit(Phase::Build, base + done as f64 / t as f64 / n * 5.0);
                }
            })?;
            let dirs = build::Dirs::new(self.build_dir(slug).join(paths::slug(&b.id)));
            let _ = std::fs::remove_dir_all(&dirs.root);
            let script = build::stage(b, &dirs, self.allow_local)?;
            rep.emit(Phase::Build, base + 7.0 / n);
            if let Err(e) = build::run(b, &dirs, &script, &ready) {
                // Keep the log where the player (or a bug report) can find it; the work folder goes.
                let logs = self.home.join("logs");
                let kept = logs.join(format!("{slug}-{}-build.log", paths::slug(&b.id)));
                let _ = std::fs::create_dir_all(&logs);
                let _ = std::fs::copy(dirs.root.join("build.log"), &kept);
                return Err(match e {
                    InstallError::BuildFailed { id, label, message, log: Some(_) } => InstallError::BuildFailed { id, label, message, log: Some(path_string(&kept)) },
                    other => other,
                });
            }
            out.push(dirs.out);
        }
        Ok(out)
    }

    /// The folder an own copy or a build output goes to: `{instance}/...` (the step's Prism instance, as written) or
    /// `{app}/...` (the step's own SIGF folder), created, and checked to stay inside it.
    fn byo_dir(&self, to: &str, slug: &str, game: &str, done: &[InstalledGame]) -> Result<PathBuf, InstallError> {
        let (root, rest) = if let Some(r) = to.strip_prefix("{instance}") {
            let inst = done
                .iter()
                .find(|g| g.game == game && g.strategy == Strategy::Mrpack)
                .and_then(|g| g.instance.as_ref().and(g.profile_dir.as_ref()))
                .ok_or_else(|| InstallError::recipe(format!("{to} needs the Prism instance SIGF writes for {game}")))?;
            (PathBuf::from(inst), r)
        } else if let Some(r) = to.strip_prefix("{app}") {
            (self.profile_dir(slug, game), r)
        } else {
            return Err(InstallError::PathTraversal { dst: to.to_string() });
        };
        std::fs::create_dir_all(&root).map_err(|e| InstallError::io(&root, e))?;
        let rest = rest.trim_start_matches('/');
        let dir = if rest.is_empty() { root.clone() } else { resolve_inside(&root, "", rest)?.0 };
        std::fs::create_dir_all(&dir).map_err(|e| InstallError::io(&dir, e))?;
        ensure_real_parent_inside(&root, &dir.join("x"), to)?;
        Ok(dir)
    }

    /// Copies the player's own copies (checked again while copied) and the built outputs into place. Returns every
    /// file written, for Restore.
    fn place_byo(&self, recipe: &Recipe, slug: &str, done: &[InstalledGame], built: &[PathBuf]) -> Result<Vec<String>, InstallError> {
        let mut placed = vec![];
        let r = (|| {
            for c in &recipe.own_copies {
                let src = self.own.get(&c.game).ok_or_else(|| InstallError::OwnCopyMissing { game: c.game.clone(), label: c.label.clone() })?;
                let dir = self.byo_dir(&c.to, slug, &c.step, done)?;
                placed.push(path_string(&byo::place(c, src, &dir)?));
            }
            for (b, out) in recipe.player_build.iter().zip(built) {
                for o in &b.outputs {
                    let dir = self.byo_dir(&o.to, slug, &b.step, done)?;
                    let dst = dir.join(&o.name);
                    std::fs::copy(out.join(&o.name), &dst).map_err(|e| InstallError::io(&dst, e))?;
                    placed.push(path_string(&dst));
                }
            }
            Ok(())
        })();
        match r {
            Ok(()) => Ok(placed),
            Err(e) => {
                remove_placed(&placed);
                Err(e)
            }
        }
    }

    /// Snapshots/instances left by an install that crashed before writing its registry entry.
    fn orphans(&self, id: &str, slug: &str) -> Vec<InstalledGame> {
        let mut out = vec![];
        let blank = |strategy| InstalledGame {
            game: String::new(),
            strategy,
            launch_args: vec![],
            instance: None,
            profile_dir: None,
            snapshot: None,
            game_dir: None,
            launcher: None,
            exe: None,
            wait: None,
        };
        if let Ok(rd) = std::fs::read_dir(self.home.join("snapshots").join(slug)) {
            for e in rd.flatten().filter(|e| e.path().join("manifest.json").exists()) {
                out.push(InstalledGame {
                    game: e.file_name().to_string_lossy().into_owned(),
                    snapshot: Some(path_string(&e.path())),
                    ..blank(Strategy::GameDirSnapshot)
                });
            }
        }
        if let Some(p) = &self.prism {
            let dir = p.data_dir.join("instances").join(slug);
            if mrpack::owner(&dir).as_deref() == Some(id) {
                out.push(InstalledGame { game: "minecraft".into(), profile_dir: Some(path_string(&dir)), ..blank(Strategy::Mrpack) });
            }
        }
        if self.home.join("profiles").join(slug).exists() {
            out.push(InstalledGame { profile_dir: Some(path_string(&self.home.join("profiles").join(slug))), ..blank(Strategy::Profile) });
        }
        out
    }
}

/// One file extracted from a zip: its path in the archive (forward slashes), where it landed, its sha256.
struct Extracted {
    rel: String,
    abs: PathBuf,
    sha256: String,
}

/// Most bytes one archive may unpack to (zip bombs): entries are read through this budget.
pub const MAX_UNPACKED_BYTES: u64 = 8 * 1024 * 1024 * 1024;

/// Copies `entry` to `file`, failing once the archive's unpacked total would pass `budget`.
fn copy_capped(entry: &mut impl std::io::Read, file: &mut std::fs::File, budget: &mut u64, abs: &Path) -> Result<(), InstallError> {
    use std::io::Read;
    let n = std::io::copy(&mut entry.take(*budget + 1), file).map_err(|e| InstallError::io(abs, e))?;
    if n > *budget {
        return Err(InstallError::recipe(format!("archive unpacks to more than {MAX_UNPACKED_BYTES} bytes")));
    }
    *budget -= n;
    Ok(())
}

/// Deletes the files an install placed outside its downloads (own copies, built outputs). Missing ones are fine.
fn remove_placed(placed: &[String]) {
    for p in placed {
        let _ = std::fs::remove_file(p);
    }
}

/// Extracts a zip under `target`; entries whose names escape it (zip slip) abort the install.
fn extract_zip(zip_path: &Path, target: &Path) -> Result<Vec<Extracted>, InstallError> {
    let f = std::fs::File::open(zip_path).map_err(|e| InstallError::io(zip_path, e))?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| InstallError::io(zip_path, e))?;
    std::fs::create_dir_all(target).map_err(|e| InstallError::io(target, e))?;
    let mut out = vec![];
    let mut budget = MAX_UNPACKED_BYTES;
    for i in 0..z.len() {
        let mut entry = z.by_index(i).map_err(|e| InstallError::io(zip_path, e))?;
        let name = entry.name().to_string();
        let Some(rel) = entry.enclosed_name() else {
            return Err(InstallError::PathTraversal { dst: name });
        };
        let rel = paths::rel_string(&rel);
        let (abs, _) = resolve_inside(target, "", &rel)?;
        if entry.is_dir() {
            std::fs::create_dir_all(&abs).map_err(|e| InstallError::io(&abs, e))?;
            continue;
        }
        if let Some(p) = abs.parent() {
            std::fs::create_dir_all(p).map_err(|e| InstallError::io(p, e))?;
        }
        ensure_real_parent_inside(target, &abs, &name)?;
        let mut file = std::fs::File::create(&abs).map_err(|e| InstallError::io(&abs, e))?;
        copy_capped(&mut entry, &mut file, &mut budget, &abs)?;
        drop(file);
        out.push(Extracted { sha256: fetch::sha256_file(&abs)?, rel, abs });
    }
    Ok(out)
}

/// An unpacked archive must hold exactly the `contents` the recipe lists (when it lists them), byte for byte.
fn verify_contents(f: &recipe::FileSpec, got: &[Extracted]) -> Result<(), InstallError> {
    if f.contents.is_empty() {
        return Ok(());
    }
    let norm = |p: &str| p.replace('\\', "/");
    for c in &f.contents {
        let Some(x) = got.iter().find(|x| x.rel == norm(&c.path)) else {
            return Err(InstallError::recipe(format!("{} has no {}", f.src, c.path)));
        };
        if !x.sha256.eq_ignore_ascii_case(&c.sha256) {
            return Err(InstallError::ShaMismatch {
                file: format!("{}/{}", f.src, c.path),
                expected: c.sha256.to_ascii_lowercase(),
                actual: x.sha256.clone(),
            });
        }
    }
    if let Some(extra) = got.iter().find(|x| !f.contents.iter().any(|c| norm(&c.path) == x.rel)) {
        return Err(InstallError::recipe(format!("{} holds {}, not in its contents", f.src, extra.rel)));
    }
    Ok(())
}

/// `rel` with the zip folder `root` stripped, when `rel` is a file under it (`root/...`), else None.
fn under_root<'a>(rel: &'a str, root: &str) -> Option<&'a str> {
    rel.strip_prefix(root)?.strip_prefix('/').filter(|s| !s.is_empty())
}

/// The extracted files to place and the path each goes to under `dst`: all of them, or with a `root` only those
/// under it, prefix stripped (an archive with nothing there is refused).
fn placed<'a>(f: &recipe::FileSpec, got: &'a [Extracted]) -> Result<Vec<(String, &'a Extracted)>, InstallError> {
    let Some(root) = &f.root else {
        return Ok(got.iter().map(|x| (x.rel.clone(), x)).collect());
    };
    let out: Vec<_> = got.iter().filter_map(|x| under_root(&x.rel, root).map(|r| (r.to_string(), x))).collect();
    if out.is_empty() {
        return Err(InstallError::recipe(format!("{} has nothing under {root}/", f.src)));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn under_root_strips_only_whole_segments() {
        assert_eq!(under_root("Fusion/red4ext/plugins/x.dll", "Fusion/red4ext"), Some("plugins/x.dll"));
        assert_eq!(under_root("Fusion/README.txt", "Fusion"), Some("README.txt"));
        assert_eq!(under_root("Fusion/README.txt", "Fusion/red4ext"), None);
        assert_eq!(under_root("FusionX/a.dll", "Fusion"), None, "prefix of a segment is not a folder");
        assert_eq!(under_root("Fusion", "Fusion"), None);
        assert_eq!(under_root("Fusion/", "Fusion"), None);
        assert_eq!(under_root("fusion/a.dll", "Fusion"), None, "zip paths are case-sensitive");
    }

    fn spec(root: Option<&str>) -> recipe::FileSpec {
        serde_json::from_value(serde_json::json!({"src": "m.zip", "sha256": "00", "unpack": true, "root": root})).unwrap()
    }

    fn x(rel: &str) -> Extracted {
        Extracted { rel: rel.into(), abs: PathBuf::from(rel), sha256: String::new() }
    }

    #[test]
    fn placed_filters_and_strips() {
        let got = [x("M/red4ext/plugins/M/M.dll"), x("M/README.txt"), x("M/src.zip")];
        let all: Vec<String> = placed(&spec(None), &got).unwrap().into_iter().map(|(r, _)| r).collect();
        assert_eq!(all, ["M/red4ext/plugins/M/M.dll", "M/README.txt", "M/src.zip"]);
        let sub: Vec<String> = placed(&spec(Some("M/red4ext")), &got).unwrap().into_iter().map(|(r, _)| r).collect();
        assert_eq!(sub, ["plugins/M/M.dll"]);
        assert!(matches!(placed(&spec(Some("Other")), &got), Err(InstallError::Recipe { .. })));
    }
}
