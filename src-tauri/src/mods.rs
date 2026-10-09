//! Mods from every source (docs/GAME-HUB.md sections 4-6): install plans from sigf.ai checked and installed through
//! the engine (`install::Engine`, `game-dir-snapshot`, registry id `mod/<ref>`, undone by the usual `restore`),
//! `nxm://` links from Nexus Mods' "Mod manager download" button, and the player's Nexus account
//! (`<SIGF_HOME>/nexus.json`, signed in through Nexus SSO).
//!
//! A plan comes from the webview: it is held to the whole rule here (refs, hosts per source, https, destinations
//! inside `{game}`, sizes, hashes) before anything is downloaded, whatever sigf.ai or the UI checked.

use crate::install::{self, check, fetch, InstallError, InstalledMod, ModFileRecord};
use crate::workshop::WorkshopError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};
use tauri::Emitter;
use tauri_plugin_opener::OpenerExt;

/// Ref prefixes (docs/GAME-HUB.md section 3).
pub const SOURCES: &[&str] = &["ts", "cf", "nx", "mio", "gb", "mr", "ws"];
const REF_MAX: usize = 120;
/// Files in one plan (the item and its dependencies).
pub const MAX_PLAN_FILES: usize = 200;
const PLAN_MAX_BYTES: usize = 512 * 1024;
const NAME_MAX: usize = 120;
const FILE_NAME_MAX: usize = 200;
const DST_MAX: usize = 300;
/// Share of the progress bar given to the downloads (the engine's install phase starts there).
const FETCH_PCT: f64 = 70.0;

fn err(code: &str, message: impl Into<String>) -> WorkshopError {
    WorkshopError::new(code, message)
}

fn bad_plan(message: impl Into<String>) -> WorkshopError {
    err("bad_plan", message)
}

// --- Refs --------------------------------------------------------------------------------------------------------------

/// A mod ref: `<source>:<id>` in `[A-Za-z0-9_.:-]`, at most 120 characters, a known source prefix and a non-empty id
/// (`ts:BepInEx-BepInExPack`, `nx:skyrimspecialedition:12604`); `ws:` takes a published file id.
pub fn valid_ref(s: &str) -> bool {
    if s.is_empty() || s.len() > REF_MAX || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b)) {
        return false;
    }
    let Some((source, id)) = s.split_once(':') else { return false };
    SOURCES.contains(&source) && !id.is_empty() && (source != "ws" || crate::workshop::valid_item_id(id))
}

/// The source prefix of a valid ref.
pub fn ref_source(s: &str) -> Option<&str> {
    valid_ref(s).then(|| s.split_once(':').map(|(p, _)| p)).flatten()
}

/// A canonical game id: `[a-z0-9-]{1,40}`.
pub fn valid_game(s: &str) -> bool {
    !s.is_empty() && s.len() <= 40 && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

// --- Install plans (docs/GAME-HUB.md section 4) --------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct InstallPlan {
    #[serde(rename = "ref")]
    pub item: String,
    pub game: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub files: Vec<PlanFile>,
    #[serde(default)]
    pub deps: Vec<PlanDep>,
    #[serde(default)]
    pub link: Option<PlanLink>,
    #[serde(default)]
    pub needs: Option<String>,
    /// Minecraft plans (Modrinth, CurseForge): the SIGF profile instance the files go to (crate::mcprofile).
    #[serde(default)]
    pub instance: Option<crate::mcprofile::McProfile>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlanFile {
    pub url: String,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub hash: Option<PlanHash>,
    pub name: String,
    #[serde(default)]
    pub unpack: bool,
    pub dst: String,
    #[serde(default)]
    pub root: Option<String>,
    pub of: String,
    /// Layout rules applied to the archive's listing once downloaded (docs/GAME-HUB.md section 4); they replace
    /// `dst` / `root` when given.
    #[serde(default)]
    pub detect: Vec<PlanDetect>,
}

/// One layout rule of a plan file: `{ ifContains, dst, up? }` or `{ ifContains, refuse }`; `ifContains` is one pattern or
/// a list (any of them).
#[derive(Debug, Clone, Deserialize)]
pub struct PlanDetect {
    #[serde(rename = "ifContains")]
    pub if_contains: Patterns,
    #[serde(default)]
    pub dst: Option<String>,
    #[serde(default)]
    pub up: usize,
    #[serde(default)]
    pub refuse: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Patterns {
    One(String),
    Many(Vec<String>),
}

/// Rules in one plan file, patterns in one rule.
const MAX_DETECT: usize = 40;
const MAX_DETECT_PATTERNS: usize = 40;
const MAX_DETECT_UP: usize = 4;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PlanHash {
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub sha512: Option<String>,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub md5: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlanDep {
    #[serde(rename = "ref")]
    pub item: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlanLink {
    pub url: String,
    #[serde(default)]
    pub why: String,
}

/// One plan file once checked: where it comes from, what it must hash to, where it goes (an engine `dst`).
#[derive(Debug, Clone)]
pub struct CheckedFile {
    pub url: String,
    pub name: String,
    pub size: Option<u64>,
    /// The strongest hash the source gave, None when it gave none.
    pub expected: Option<fetch::Expected>,
    pub unpack: bool,
    /// `{game}/...` (or `{game}` alone for an archive unpacked into the game folder).
    pub dst: String,
    pub root: Option<String>,
    pub of: String,
    /// Layout rules (unpacked archives only): when given, `dst` / `root` come from the archive's listing.
    pub detect: Vec<install::archive::Layout>,
}

#[derive(Debug, Clone)]
pub struct CheckedPlan {
    /// `mod/<ref>`: the registry id.
    pub id: String,
    pub item: String,
    pub game: String,
    pub name: String,
    pub version: String,
    pub source: String,
    pub hosts: &'static [&'static str],
    pub files: Vec<CheckedFile>,
}

/// Text for the UI: control characters dropped, trimmed, at most `max` characters.
fn clean_text(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).collect::<String>().trim().chars().take(max).collect::<String>().trim().to_string()
}

/// `CON`, `nul.txt`, `COM1`...: names Windows keeps for devices, with or without an extension.
fn reserved_name(seg: &str) -> bool {
    let stem = seg.split('.').next().unwrap_or(seg).trim_end().to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
        || ((stem.starts_with("COM") || stem.starts_with("LPT")) && stem.len() == 4 && stem.as_bytes()[3].is_ascii_digit())
}

/// One path segment Windows writes as named: no `<>:"/\|?*{}` or control character, no leading space, no trailing dot
/// or space, not `.`/`..` or a device name, at most 255 characters.
fn segment_ok(seg: &str) -> bool {
    !seg.is_empty()
        && seg.chars().count() <= 255
        && seg != "."
        && seg != ".."
        && !seg.chars().any(|c| c.is_control() || "<>:\"/\\|?*{}".contains(c))
        && !seg.starts_with(' ')
        && !seg.ends_with(['.', ' '])
        && !reserved_name(seg)
}

/// Plain relative segments joined by `/`, each `segment_ok`.
fn rel_ok(s: &str) -> bool {
    !s.is_empty() && s.split('/').all(segment_ok)
}

/// The file name a plan gives, made safe as one segment: forbidden characters become `_`, trailing dots and spaces
/// go, at most 200 characters. None when nothing usable is left.
pub fn sanitize_file_name(name: &str) -> Option<String> {
    let mut s: String = name.chars().map(|c| if c.is_control() || "<>:\"/\\|?*{}".contains(c) { '_' } else { c }).collect();
    s = s.trim().chars().take(FILE_NAME_MAX).collect();
    let s = s.trim_end_matches(['.', ' ']).to_string();
    segment_ok(&s).then_some(s)
}

/// Archives the app does not unpack (zip and 7z only, `install::archive`): the UI links out to the mod's page instead.
/// RAR has no open-source decoder.
const OTHER_ARCHIVES: &[&str] = &[".rar", ".tar", ".gz", ".tgz", ".xz", ".bz2", ".zst", ".cab", ".exe", ".msi"];

fn other_archive(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    OTHER_ARCHIVES.iter().any(|e| n.ends_with(e))
}

/// The hex of one hash, checked for its algorithm's length (lowercased).
fn expected(algo: fetch::Algo, hex: &Option<String>) -> Result<Option<fetch::Expected>, WorkshopError> {
    match hex.as_deref() {
        None => Ok(None),
        Some(h) => fetch::Expected::new(algo, h).map(Some).map_err(|_| bad_plan(format!("bad {algo:?} hash"))),
    }
}

/// The strongest hash given (sha512 > sha256 > sha1 > md5); every one given must be well formed.
pub fn strongest_hash(h: &PlanHash) -> Result<Option<fetch::Expected>, WorkshopError> {
    use fetch::Algo::*;
    let all = [expected(Sha512, &h.sha512)?, expected(Sha256, &h.sha256)?, expected(Sha1, &h.sha1)?, expected(Md5, &h.md5)?];
    Ok(all.into_iter().flatten().next())
}

/// The whole rule on a plan's text (docs/GAME-HUB.md sections 4-5). Codes: `bad_plan`, `bad_host` (a URL off the
/// source's hosts), `unsupported_archive` (not a zip: link out), `link_only` / `needs_nxm` (a plan without files).
pub fn check_plan(text: &str) -> Result<CheckedPlan, WorkshopError> {
    check_plan_full(text).map(|(p, _)| p)
}

/// A Minecraft plan's `dst`: `{instance}/mods`, `{instance}/resourcepacks` or `{instance}/shaderpacks` exactly, a plain
/// file (no unpack, no root), as the engine's `{game}/<folder>` (the instance's game folder is `{game}` there).
fn instance_dst(f: &PlanFile) -> Result<String, WorkshopError> {
    let folder = f.dst.strip_prefix("{instance}/").filter(|d| crate::mcprofile::DST_FOLDERS.contains(d));
    match folder {
        Some(d) if !f.unpack && f.root.is_none() => Ok(format!("{{game}}/{d}")),
        _ => Err(bad_plan(format!("a Minecraft file goes into {{instance}}/mods, resourcepacks or shaderpacks, as is: {}", clean_text(&f.dst, DST_MAX)))),
    }
}

/// `check_plan`, with the Minecraft profile a plan installs into (None for every other plan). A plan with a profile is
/// a Minecraft plan from Modrinth or CurseForge, every file into the profile instance's folders (`instance_dst`).
pub fn check_plan_full(text: &str) -> Result<(CheckedPlan, Option<crate::mcprofile::McProfile>), WorkshopError> {
    if text.len() > PLAN_MAX_BYTES {
        return Err(bad_plan("plan too large"));
    }
    let p: InstallPlan = serde_json::from_str(text).map_err(|e| bad_plan(e.to_string()))?;
    let source = ref_source(&p.item).ok_or_else(|| bad_plan("bad ref"))?.to_string();
    if let Some(i) = &p.instance {
        if p.game != "minecraft" || !matches!(source.as_str(), "mr" | "cf") {
            return Err(bad_plan("only Minecraft plans from Modrinth or CurseForge go into a profile"));
        }
        i.check(true).map_err(|e| bad_plan(e.to_string()))?;
    }
    let hosts = check::mod_hosts(&source).ok_or_else(|| bad_plan(format!("{source} items do not install through a plan")))?;
    if !valid_game(&p.game) {
        return Err(bad_plan("bad game"));
    }
    if p.deps.len() > MAX_PLAN_FILES || p.deps.iter().any(|d| !valid_ref(&d.item)) {
        return Err(bad_plan("bad deps"));
    }
    if p.files.is_empty() {
        if p.needs.as_deref() == Some("nxm") {
            return Err(err("needs_nxm", "download it from the mod's files page on Nexus Mods (Mod manager download)"));
        }
        return match &p.link {
            Some(l) => Err(err("link_only", format!("this mod installs from its own page: {}", clean_text(&l.url, 300)))),
            None => Err(bad_plan("no files")),
        };
    }
    if p.files.len() > MAX_PLAN_FILES {
        return Err(bad_plan(format!("more than {MAX_PLAN_FILES} files")));
    }
    let mut files = vec![];
    for f in &p.files {
        // A Nexus plan for a free account comes with an empty url, filled by the UI after the nxm:// click.
        if f.url.is_empty() && p.needs.as_deref() == Some("nxm") {
            return Err(err("needs_nxm", "download it from the mod's files page on Nexus Mods (Mod manager download)"));
        }
        if !f.url.starts_with("https://") {
            return Err(bad_plan("https downloads only"));
        }
        if !check::mod_url_ok(hosts, &f.url) {
            return Err(err("bad_host", format!("not a download host of {source}: {}", host_of(&f.url))));
        }
        if ref_source(&f.of) != Some(source.as_str()) {
            return Err(bad_plan("a file belongs to another source or a bad ref"));
        }
        let name = sanitize_file_name(&f.name).ok_or_else(|| bad_plan("bad file name"))?;
        if f.size.is_some_and(|s| s > check::MOD_MAX_FILE_BYTES) {
            return Err(bad_plan(format!("{name} is larger than {} bytes", check::MOD_MAX_FILE_BYTES)));
        }
        let expected = strongest_hash(&f.hash.clone().unwrap_or_default())?;
        let fdst = if p.instance.is_some() { instance_dst(f)? } else { f.dst.clone() };
        let rest = match fdst.strip_prefix("{game}") {
            Some("") => "",
            Some(r) if r.starts_with('/') => &r[1..],
            _ => return Err(bad_plan("a destination starts with {game}")),
        };
        if !rest.is_empty() && !rel_ok(rest) {
            return Err(bad_plan(format!("bad destination {}", clean_text(&fdst, DST_MAX))));
        }
        if let Some(r) = &f.root {
            if !f.unpack || !rel_ok(r) {
                return Err(bad_plan("bad root"));
            }
        }
        if f.unpack && other_archive(&name) {
            return Err(err("unsupported_archive", format!("{name}: only zip and 7z archives install here")));
        }
        let detect = check_detect(&f.detect, f.unpack)?;
        // An unpacked archive goes into the folder `dst`; a plain file into the folder `dst` under its name, unless
        // `dst` already ends with that name.
        let dst = if f.unpack || rest.rsplit('/').next().is_some_and(|last| last.eq_ignore_ascii_case(&name)) {
            fdst.clone()
        } else if rest.is_empty() {
            format!("{{game}}/{name}")
        } else {
            format!("{{game}}/{rest}/{name}")
        };
        if dst.chars().count() > DST_MAX {
            return Err(bad_plan("destination too long"));
        }
        files.push(CheckedFile { url: f.url.clone(), name, size: f.size, expected, unpack: f.unpack, dst, root: f.root.clone(), of: f.of.clone(), detect });
    }
    let name = match clean_text(&p.name, NAME_MAX) {
        n if n.is_empty() => p.item.clone(),
        n => n,
    };
    let instance = p.instance;
    Ok((
        CheckedPlan {
            id: format!("mod/{}", p.item),
            item: p.item,
            game: p.game,
            name,
            version: clean_text(&p.version, 64),
            source,
            hosts,
            files,
        },
        instance,
    ))
}

/// A plan file's layout rules, checked: at most `MAX_DETECT`, on an unpacked archive only, each a pattern
/// `install::archive::parse_pattern` reads, `up` at most `MAX_DETECT_UP`, and either a `{game}` destination (as `dst`
/// is checked) or a refusal reason (`[a-z_]`, at most 32).
fn check_detect(rules: &[PlanDetect], unpack: bool) -> Result<Vec<install::archive::Layout>, WorkshopError> {
    use install::archive::{parse_pattern, Layout, Outcome};
    if rules.is_empty() {
        return Ok(vec![]);
    }
    if !unpack || rules.len() > MAX_DETECT {
        return Err(bad_plan("bad detect rules"));
    }
    rules
        .iter()
        .map(|r| {
            let given = match &r.if_contains {
                Patterns::One(p) => std::slice::from_ref(p),
                Patterns::Many(v) => v.as_slice(),
            };
            if given.is_empty() || given.len() > MAX_DETECT_PATTERNS {
                return Err(bad_plan("bad detect rule"));
            }
            let patterns = given
                .iter()
                .map(|p| parse_pattern(p).ok_or_else(|| bad_plan(format!("bad detect pattern {}", clean_text(p, 120)))))
                .collect::<Result<Vec<_>, _>>()?;
            if r.up > MAX_DETECT_UP {
                return Err(bad_plan("bad detect rule"));
            }
            let outcome = match (&r.dst, &r.refuse) {
                (Some(d), None) => {
                    let ok = match d.strip_prefix("{game}") {
                        Some("") => true,
                        Some(rest) => rest.strip_prefix('/').is_some_and(rel_ok) && d.chars().count() <= DST_MAX,
                        None => false,
                    };
                    if !ok {
                        return Err(bad_plan(format!("bad destination {}", clean_text(d, DST_MAX))));
                    }
                    Outcome::Place(d.clone())
                }
                (None, Some(why)) if !why.is_empty() && why.len() <= 32 && why.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') => Outcome::Refuse(why.clone()),
                _ => return Err(bad_plan("bad detect rule")),
            };
            Ok(Layout { patterns, up: r.up, outcome })
        })
        .collect()
}

/// The file's `dst` / `root` from its layout rules and the downloaded archive's listing. Refusals are
/// `unsupported_archive` (the UI links out to the mod's page), with what the archive holds in the message.
fn resolve_layout(f: &CheckedFile, archive: &Path, game: &str) -> Result<CheckedFile, WorkshopError> {
    use install::archive::Detected;
    let listing = install::archive::list(archive).map_err(from_install)?;
    let refused = |m: String| err("unsupported_archive", format!("{}: {m}", f.name));
    match install::archive::detect(&listing, &f.detect) {
        Detected::Place { dst, root } => {
            if root.as_deref().is_some_and(|r| !rel_ok(r)) {
                return Err(refused("its folder names cannot be installed as they are".into()));
            }
            Ok(CheckedFile { dst, root, detect: vec![], ..f.clone() })
        }
        Detected::Refuse(why) if why == "fomod" => {
            Err(refused("it has an installer with options (FOMOD): install it from its page with a mod manager".into()))
        }
        Detected::Refuse(why) => Err(refused(format!("it needs to be installed from its page ({why})"))),
        Detected::Ambiguous(roots) => Err(refused(format!(
            "it holds several variants to choose from ({}): install it from its page",
            roots.iter().map(|r| if r.is_empty() { "top folder".to_string() } else { clean_text(r, 60) }).collect::<Vec<_>>().join(", ")
        ))),
        Detected::NoMatch => Err(refused(format!("its files are not laid out the way {game} mods are: install it from its page"))),
    }
}

/// A URL's host for messages (never its path or query).
fn host_of(url: &str) -> String {
    reqwest::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_else(|| "?".into())
}

/// A URL without its query (what installed.json keeps: CDN tokens are not).
fn without_query(url: &str) -> String {
    url.split(['?', '#']).next().unwrap_or(url).to_string()
}

impl CheckedPlan {
    /// The engine's own recipe for the plan, once every file's sha256 is known: one `game-dir-snapshot` step.
    pub fn recipe(&self, shas: &[String]) -> Result<install::Recipe, InstallError> {
        let files: Vec<serde_json::Value> = self
            .files
            .iter()
            .zip(shas)
            .map(|(f, sha)| serde_json::json!({ "src": f.name, "dst": f.dst, "sha256": sha, "url": f.url, "unpack": f.unpack, "root": f.root }))
            .collect();
        let v = serde_json::json!({
            "id": self.id,
            "version": if self.version.is_empty() { "0" } else { self.version.as_str() },
            "name": self.name,
            "kind": "mod",
            "games": [ { "game": self.game, "role": "host" } ],
            "install": [ { "game": self.game, "strategy": "game-dir-snapshot", "files": files } ],
        });
        install::Recipe::parse(&v.to_string())
    }
}

/// Engine errors as `{ code, message }`: `bad_plan`, `download`, `hash_mismatch`, `missing_game_dir`, `tampered`,
/// `snapshot_corrupt`, `io`, else `failed`.
fn from_install(e: InstallError) -> WorkshopError {
    let code = match &e {
        InstallError::Recipe { .. } | InstallError::PathTraversal { .. } => "bad_plan",
        InstallError::Download { .. } => "download",
        InstallError::ShaMismatch { .. } => "hash_mismatch",
        InstallError::MissingGameDir { .. } => "missing_game_dir",
        InstallError::Tampered { .. } => "tampered",
        InstallError::SnapshotCorrupt { .. } => "snapshot_corrupt",
        InstallError::Io { .. } => "io",
        _ => "failed",
    };
    err(code, e.to_string())
}

/// Downloads and checks every file of the plan, then installs it through `engine` (its `mod_hosts` set to the plan's
/// source; `allow_local` as the caller set it). Holds no lock itself: the caller holds `INSTALL_LOCK`.
pub fn install_plan(
    engine: &mut install::Engine,
    plan: &CheckedPlan,
    game_dirs: &HashMap<String, String>,
    on_progress: &mut dyn FnMut(install::Progress),
) -> Result<InstalledMod, WorkshopError> {
    engine.mod_hosts = Some(plan.hosts);
    let allow_local = engine.allow_local;
    let cache = engine.cache_dir();
    let n = plan.files.len().max(1) as f64;
    let mut emit = |phase, pct: f64| on_progress(install::Progress { id: plan.id.clone(), phase, pct: pct.clamp(0.0, 100.0) as u8 });
    let mut shas = vec![];
    // The files as installed: layout rules (`detect`) resolved against each archive's listing.
    let mut placed = Vec::with_capacity(plan.files.len());
    for (i, f) in plan.files.iter().enumerate() {
        let base = i as f64 / n * FETCH_PCT;
        emit(install::Phase::Download, base);
        let opts = fetch::FetchOpts::for_mod(allow_local, f.size, plan.hosts);
        let mut last = u8::MAX;
        let (path, sha) = fetch::fetch_mod(&cache, &f.url, f.expected.as_ref(), &opts, &mut |done, total| {
            if let Some(t) = total.filter(|t| *t > 0) {
                let pct = base + done as f64 / t as f64 / n * FETCH_PCT * 0.95;
                if pct as u8 != last {
                    last = pct as u8;
                    emit(install::Phase::Download, pct);
                }
            }
        })
        .map_err(|e| match e {
            // The URL may carry a CDN token: name the file and host only.
            InstallError::Download { message, .. } => err("download", format!("{} from {}: {message}", f.name, host_of(&f.url))),
            InstallError::ShaMismatch { .. } => err("hash_mismatch", format!("{} does not match the hash its source gives", f.name)),
            other => from_install(other),
        })?;
        if f.unpack && !install::archive::supported(&path) {
            let what = match install::archive::kind(&path) {
                Some(install::archive::Kind::Rar) => "RAR archives are not unpacked (no open-source RAR decoder), only zip and 7z",
                _ => "not a zip or 7z archive",
            };
            return Err(err("unsupported_archive", format!("{}: {what}", f.name)));
        }
        placed.push(if f.detect.is_empty() { f.clone() } else { resolve_layout(f, &path, &plan.game)? });
        emit(install::Phase::Verify, (i + 1) as f64 / n * FETCH_PCT);
        shas.push(sha);
    }
    let recipe = CheckedPlan { files: placed, ..plan.clone() }.recipe(&shas).map_err(from_install)?;
    // The downloads above are the engine's cache hits now: only its install and ready steps reach the bar.
    let mut entry = engine
        .install(&recipe, game_dirs, &mut |p| {
            if !matches!(p.phase, install::Phase::Download | install::Phase::Verify) {
                on_progress(p);
            }
        })
        .map_err(from_install)?;
    entry.files = plan
        .files
        .iter()
        .zip(&shas)
        .map(|(f, sha)| ModFileRecord {
            name: f.name.clone(),
            of: f.of.clone(),
            url: without_query(&f.url),
            sha256: sha.clone(),
            verified: match f.expected.as_ref().map(|e| e.algo) {
                Some(fetch::Algo::Sha512) => "sha512",
                Some(fetch::Algo::Sha256) => "sha256",
                Some(fetch::Algo::Sha1) => "sha1",
                Some(fetch::Algo::Md5) => "md5",
                None => "none",
            }
            .into(),
        })
        .collect();
    let mut all = engine.installed();
    if let Some(m) = all.iter_mut().find(|m| m.id == entry.id) {
        m.files = entry.files.clone();
        install::registry::save(&engine.home, &all).map_err(from_install)?;
    }
    Ok(entry)
}

/// Installs a mod plan (docs/GAME-HUB.md section 5), with `install://progress` like `install`. Uninstall is
/// `restore("mod/<ref>")`. Errors are `{ code, message }` (see `check_plan` and `from_install`).
#[tauri::command]
pub async fn mods_install(app: tauri::AppHandle, plan_json: String, game_dirs: HashMap<String, String>) -> Result<InstalledMod, WorkshopError> {
    let (plan, instance) = check_plan_full(&plan_json)?;
    if let Some(profile) = instance {
        return profile_install(app, plan, profile).await;
    }
    // Only the plan's own game folder is used, and it must be one the scan found.
    let dirs: HashMap<String, String> = game_dirs.into_iter().filter(|(g, _)| *g == plan.game).collect();
    if dirs.is_empty() {
        return Err(err("missing_game_dir", format!("no install folder known for {}", plan.game)));
    }
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = crate::INSTALL_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        crate::check_game_dirs(&dirs).map_err(from_install)?;
        install_plan(&mut crate::engine(None), &plan, &dirs, &mut |p| {
            let _ = app.emit("install://progress", &p);
        })
    })
    .await
    .map_err(|e| err("failed", e.to_string()))?
}

/// A Minecraft plan: its `{game}` is the game folder of the profile's SIGF Prism instance, written by the core in the
/// Prism data folder the scan finds (created on the first install, never another instance; crate::mcprofile), never a
/// folder from the webview. `needs_launcher` without Prism.
async fn profile_install(app: tauri::AppHandle, plan: CheckedPlan, profile: crate::mcprofile::McProfile) -> Result<InstalledMod, WorkshopError> {
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = crate::INSTALL_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let prism = crate::detect_prism().ok_or_else(|| err("needs_launcher", "install Prism Launcher to add Minecraft mods"))?;
        let root = crate::mcprofile::ensure(&prism.data_dir, &profile).map_err(from_install)?;
        let dirs = HashMap::from([(plan.game.clone(), install::paths::path_string(&root))]);
        install_plan(&mut crate::engine(None), &plan, &dirs, &mut |p| {
            let _ = app.emit("install://progress", &p);
        })
    })
    .await
    .map_err(|e| err("failed", e.to_string()))?
}

// --- nxm:// links ------------------------------------------------------------------------------------------------------

/// `nxm://<domain>/mods/<mod>/files/<file>?key=&expires=&user_id=`: Nexus Mods' "Mod manager download" link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NxmLink {
    pub domain: String,
    pub mod_id: u64,
    pub file_id: u64,
    pub key: String,
    pub expires: u64,
    pub user_id: Option<u64>,
}

impl NxmLink {
    /// The one form the UI receives (through `take_links`): lowercase domain, the known parameters in a fixed order.
    pub fn to_link(&self) -> String {
        let mut s = format!("nxm://{}/mods/{}/files/{}?key={}&expires={}", self.domain, self.mod_id, self.file_id, self.key, self.expires);
        if let Some(u) = self.user_id {
            s.push_str(&format!("&user_id={u}"));
        }
        s
    }
}

/// A Nexus game domain (`skyrimspecialedition`): `[a-z0-9-]{1,60}`, not `-`-led.
pub fn valid_nexus_domain(s: &str) -> bool {
    !s.is_empty() && s.len() <= 60 && !s.starts_with('-') && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A Nexus download key from an nxm link: `[A-Za-z0-9_-]{1,128}`.
pub fn valid_nxm_key(s: &str) -> bool {
    !s.is_empty() && s.len() <= 128 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Decimal id: 1..=12 digits, no leading zero.
fn id_num(s: &str) -> Option<u64> {
    ((1..=12).contains(&s.len()) && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse().ok()).flatten()
}

/// Parses an nxm link strictly: exactly `<domain>/mods/<id>/files/<id>`, `key` and `expires` present once each,
/// `user_id` at most once, other parameters ignored but plain; no fragment. Collections links are refused.
pub fn parse_nxm(link: &str) -> Option<NxmLink> {
    let s = link.trim();
    if s.len() > 1024 || s.contains('#') {
        return None;
    }
    let rest = crate::join::strip_prefix_ci(s, "nxm://")?;
    let (path, query) = rest.split_once('?')?;
    let segs: Vec<&str> = path.split('/').collect();
    let [domain, "mods", mod_id, "files", file_id] = segs.as_slice() else { return None };
    let domain = domain.to_ascii_lowercase();
    if !valid_nexus_domain(&domain) {
        return None;
    }
    let (mut key, mut expires, mut user_id) = (None, None, None);
    for kv in query.split('&') {
        let (k, v) = kv.split_once('=')?;
        if !kv.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.-=%".contains(&b)) {
            return None;
        }
        let slot = match k {
            "key" => &mut key,
            "expires" => &mut expires,
            "user_id" => &mut user_id,
            _ => continue,
        };
        if slot.replace(v).is_some() {
            return None;
        }
    }
    let key = key.filter(|k| valid_nxm_key(k))?;
    let user_id = match user_id {
        Some(u) => Some(id_num(u)?),
        None => None,
    };
    Some(NxmLink { domain, mod_id: id_num(mod_id)?, file_id: id_num(file_id)?, key: key.to_string(), expires: id_num(expires?)?, user_id })
}

/// The nxm:// handler on this PC: `supported` (Windows only), `enabled` (it starts this app), `other` (another
/// manager, Vortex or Mod Organizer 2, has it).
#[derive(Debug, Clone, Serialize)]
pub struct NxmHandler {
    pub supported: bool,
    pub enabled: bool,
    pub other: bool,
}

#[cfg(windows)]
mod nxm_reg {
    use super::NxmHandler;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::RegKey;

    const BASE: &str = r"Software\Classes\nxm";
    /// What the key held before SIGF took it (another manager's command and icon), put back when the player turns it off.
    const PREVIOUS: &str = "SIGFPrevious";
    const PREVIOUS_ICON: &str = "SIGFPreviousIcon";

    fn exe() -> Result<String, String> {
        std::env::current_exe().map(|p| p.display().to_string()).map_err(|e| e.to_string())
    }

    fn ours() -> Result<String, String> {
        Ok(format!("\"{}\" \"%1\"", exe()?))
    }

    fn read(sub: &str) -> Option<String> {
        RegKey::predef(HKEY_CURRENT_USER).open_subkey(format!(r"{BASE}{sub}")).ok()?.get_value::<String, _>("").ok()
    }

    pub fn state() -> NxmHandler {
        let current = read(r"\shell\open\command");
        let enabled = ours().is_ok_and(|o| current.as_deref() == Some(o.as_str()));
        NxmHandler { supported: true, enabled, other: current.is_some() && !enabled }
    }

    pub fn set(enable: bool) -> Result<NxmHandler, String> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let ours = ours()?;
        let current = read(r"\shell\open\command");
        let e = |e: std::io::Error| e.to_string();
        if enable {
            let (key, _) = hkcu.create_subkey(BASE).map_err(e)?;
            if let Some(c) = current.filter(|c| *c != ours) {
                key.set_value(PREVIOUS, &c).map_err(e)?;
                if let Some(i) = read(r"\DefaultIcon") {
                    key.set_value(PREVIOUS_ICON, &i).map_err(e)?;
                }
            }
            key.set_value("", &"URL:NXM Protocol").map_err(e)?;
            key.set_value("URL Protocol", &"").map_err(e)?;
            let (icon, _) = hkcu.create_subkey(format!(r"{BASE}\DefaultIcon")).map_err(e)?;
            icon.set_value("", &format!("{},0", exe()?)).map_err(e)?;
            let (cmd, _) = hkcu.create_subkey(format!(r"{BASE}\shell\open\command")).map_err(e)?;
            cmd.set_value("", &ours).map_err(e)?;
        } else if current.as_deref() == Some(ours.as_str()) {
            // Only our own registration is undone: another manager's is left as it is.
            let key = hkcu.open_subkey_with_flags(BASE, KEY_READ | KEY_WRITE).map_err(e)?;
            match key.get_value::<String, _>(PREVIOUS).ok() {
                Some(prev) => {
                    let (cmd, _) = hkcu.create_subkey(format!(r"{BASE}\shell\open\command")).map_err(e)?;
                    cmd.set_value("", &prev).map_err(e)?;
                    if let Some(i) = key.get_value::<String, _>(PREVIOUS_ICON).ok() {
                        let (icon, _) = hkcu.create_subkey(format!(r"{BASE}\DefaultIcon")).map_err(e)?;
                        icon.set_value("", &i).map_err(e)?;
                    }
                    let _ = key.delete_value(PREVIOUS);
                    let _ = key.delete_value(PREVIOUS_ICON);
                }
                None => hkcu.delete_subkey_all(BASE).map_err(e)?,
            }
        }
        Ok(state())
    }
}

/// Whether nxm:// links start this app (Windows: `HKCU\Software\Classes\nxm`).
#[tauri::command]
pub fn nxm_handler_state() -> NxmHandler {
    #[cfg(windows)]
    return nxm_reg::state();
    #[cfg(not(windows))]
    NxmHandler { supported: false, enabled: false, other: false }
}

/// Makes this app the nxm:// handler (`enable`), or gives it back (a manager it replaced gets its registration back).
/// Only when the player asks: Vortex and Mod Organizer 2 claim nxm too, so it is never registered at install.
#[tauri::command]
pub fn nxm_handler(enable: bool) -> Result<NxmHandler, WorkshopError> {
    #[cfg(windows)]
    return nxm_reg::set(enable).map_err(|m| err("failed", m));
    #[cfg(not(windows))]
    {
        let _ = enable;
        Err(err("unsupported", "nxm links are handled on Windows only"))
    }
}

/// An nxm link in a start's arguments (`<exe> nxm://...`, the handler's command line), for `receive_links`.
pub fn nxm_arg(args: &[String]) -> Option<String> {
    match args {
        [_, link] if crate::join::strip_prefix_ci(link, "nxm://").is_some() => Some(link.clone()),
        _ => None,
    }
}

// --- Nexus account ------------------------------------------------------------------------------------------------------

const NEXUS_API: &str = "https://api.nexusmods.com/v1";
const SSO_URL: &str = "wss://sso.nexusmods.com";
const SSO_HOST: &str = "sso.nexusmods.com";
/// The player has this long to approve SIGF on nexusmods.com.
const SSO_DEADLINE: Duration = Duration::from_secs(10 * 60);
/// A read waits this long before the socket is pinged (keeps it open while the player approves).
const SSO_PING: Duration = Duration::from_secs(20);

/// The app's Nexus application slug, from the build (`SIGF_NEXUS_SLUG`); None until Nexus registers SIGF.
fn nexus_slug() -> Option<&'static str> {
    option_env!("SIGF_NEXUS_SLUG").filter(|s| !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'))
}

/// `<SIGF_HOME>/nexus.json`. The key never leaves this file except in the `apikey` header to api.nexusmods.com, and
/// is never logged or returned to the webview.
#[derive(Clone, Default, Serialize, Deserialize)]
struct NexusStore {
    api_key: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    premium: Option<bool>,
    #[serde(default)]
    user_id: Option<u64>,
}

impl std::fmt::Debug for NexusStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NexusStore").field("name", &self.name).field("premium", &self.premium).finish_non_exhaustive()
    }
}

/// One Nexus account call at a time (login, logout and validate rewrite nexus.json).
static NEXUS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
/// Bumped by every login and logout: a login still waiting on the socket stops when it changes.
static LOGIN_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn nexus_path() -> std::path::PathBuf {
    install::home_dir().join("nexus.json")
}

/// An API key as Nexus hands it: `[A-Za-z0-9+/=_-]{1,512}` (it goes into a header).
fn valid_api_key(s: &str) -> bool {
    !s.is_empty() && s.len() <= 512 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"+/=_-".contains(&b))
}

fn load_nexus(path: &Path) -> Option<NexusStore> {
    let s: NexusStore = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    valid_api_key(&s.api_key).then_some(s)
}

/// Written whole (temp file + rename), readable by the player's account only where the system allows it.
fn save_nexus(path: &Path, s: &NexusStore) -> Result<(), WorkshopError> {
    let fail = |e: std::io::Error| err("failed", format!("nexus.json: {e}"));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(fail)?;
    }
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_vec(s).map_err(|e| err("failed", e.to_string()))?;
    {
        use std::io::Write;
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
        let mut f = o.open(&tmp).map_err(fail)?;
        f.write_all(&body).map_err(fail)?;
    }
    std::fs::rename(&tmp, path).map_err(fail)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NexusStatus {
    pub connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub premium: Option<bool>,
}

fn status_of(s: Option<&NexusStore>) -> NexusStatus {
    match s {
        Some(s) => NexusStatus { connected: true, name: s.name.clone(), premium: s.premium },
        None => NexusStatus { connected: false, name: None, premium: None },
    }
}

/// The Nexus API client: https only, no redirects, 20 s.
fn nexus_client() -> Result<reqwest::blocking::Client, WorkshopError> {
    reqwest::blocking::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("SIGF/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| err("network", e.without_url().to_string()))
}

/// GET `<NEXUS_API>/<path>` with the player's key. `path` is built from checked parts only. Nexus statuses become
/// codes: 401 `nexus_key_invalid`, 403 `nexus_forbidden`, 404 `not_found`, 410 `nxm_expired`, 429 `rate_limited`.
fn nexus_get(path: &str, api_key: &str) -> Result<serde_json::Value, WorkshopError> {
    let r = nexus_client()?
        .get(format!("{NEXUS_API}/{path}"))
        .header("apikey", api_key)
        .header("Application-Name", "SIGF")
        .header("Application-Version", env!("CARGO_PKG_VERSION"))
        .header("accept", "application/json")
        .send()
        .map_err(|e| err("network", e.without_url().to_string()))?;
    let status = r.status().as_u16();
    let code = match status {
        200 => None,
        401 => Some("nexus_key_invalid"),
        403 => Some("nexus_forbidden"),
        404 => Some("not_found"),
        410 => Some("nxm_expired"),
        429 => Some("rate_limited"),
        _ => Some("failed"),
    };
    if let Some(c) = code {
        return Err(err(c, format!("Nexus Mods answered HTTP {status}")));
    }
    r.json().map_err(|e| err("failed", e.without_url().to_string()))
}

/// `users/validate.json`: the key's account name and Premium flag into the store.
fn validate(store: &mut NexusStore) -> Result<(), WorkshopError> {
    let v = nexus_get("users/validate.json", &store.api_key)?;
    store.name = v.get("name").and_then(|n| n.as_str()).map(|n| clean_text(n, 64)).filter(|n| !n.is_empty());
    store.premium = v.get("is_premium").and_then(|p| p.as_bool());
    store.user_id = v.get("user_id").and_then(|u| u.as_u64());
    Ok(())
}

#[tauri::command]
pub fn nexus_status() -> NexusStatus {
    status_of(load_nexus(&nexus_path()).as_ref())
}

/// Checks the stored key with Nexus again (name, Premium). Not connected: `nexus_login_required`.
#[tauri::command]
pub async fn nexus_validate() -> Result<NexusStatus, WorkshopError> {
    tauri::async_runtime::spawn_blocking(|| {
        let _g = NEXUS_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let path = nexus_path();
        let mut s = load_nexus(&path).ok_or_else(|| err("nexus_login_required", "sign in to Nexus Mods first"))?;
        validate(&mut s)?;
        save_nexus(&path, &s)?;
        Ok(status_of(Some(&s)))
    })
    .await
    .map_err(|e| err("failed", e.to_string()))?
}

#[tauri::command]
pub fn nexus_logout() -> Result<NexusStatus, WorkshopError> {
    LOGIN_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let _g = NEXUS_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    match std::fs::remove_file(nexus_path()) {
        Ok(()) => Ok(status_of(None)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(status_of(None)),
        Err(e) => Err(err("failed", format!("nexus.json: {e}"))),
    }
}

/// A personal API key as the player pastes it (nexusmods.com/users/myaccount?tab=api): trimmed, then 16 to 512 chars
/// of `[A-Za-z0-9+/=_-]`. None for anything else (it goes into a header).
fn personal_key(raw: &str) -> Option<String> {
    let k = raw.trim();
    (k.len() >= 16 && valid_api_key(k)).then(|| k.to_string())
}

/// "Use my Nexus API key": the player's personal key instead of SSO. Checked with Nexus (`users/validate.json`), then
/// kept in nexus.json like an SSO key (never sent back to the webview, never logged). A bad or refused key:
/// `nexus_key_invalid`. Replaces a sign-in still waiting on the SSO socket.
#[tauri::command]
pub async fn nexus_set_key(key: String) -> Result<NexusStatus, WorkshopError> {
    let key = personal_key(&key).ok_or_else(|| err("nexus_key_invalid", "this is not a Nexus Mods API key"))?;
    let generation = LOGIN_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    tauri::async_runtime::spawn_blocking(move || {
        let mut s = NexusStore { api_key: key, ..Default::default() };
        validate(&mut s)?;
        let _g = NEXUS_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        if LOGIN_GEN.load(std::sync::atomic::Ordering::SeqCst) != generation {
            return Err(err("cancelled", "sign-in replaced"));
        }
        save_nexus(&nexus_path(), &s)?;
        Ok(status_of(Some(&s)))
    })
    .await
    .map_err(|e| err("failed", e.to_string()))?
}

/// Nexus SSO: opens nexusmods.com for the player to approve SIGF, waits for the key on the SSO socket (up to 10
/// minutes), checks it, keeps it in nexus.json. `nexus_unavailable` until the build has SIGF's application slug;
/// `cancelled` when a newer login or a logout replaced this one.
#[tauri::command]
pub async fn nexus_login(app: tauri::AppHandle) -> Result<NexusStatus, WorkshopError> {
    let slug = nexus_slug().ok_or_else(|| err("nexus_unavailable", "Nexus Mods sign-in is not available in this build yet"))?;
    let generation = LOGIN_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    tauri::async_runtime::spawn_blocking(move || {
        let open = |url: &str| app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string());
        let key = sso::api_key(slug, &open, generation)?;
        let mut s = NexusStore { api_key: key, ..Default::default() };
        validate(&mut s)?;
        let _g = NEXUS_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        if LOGIN_GEN.load(std::sync::atomic::Ordering::SeqCst) != generation {
            return Err(err("cancelled", "sign-in replaced"));
        }
        save_nexus(&nexus_path(), &s)?;
        Ok(status_of(Some(&s)))
    })
    .await
    .map_err(|e| err("failed", e.to_string()))?
}

mod sso {
    use super::*;
    use std::net::TcpStream;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;
    use tungstenite::{Message, WebSocket};

    type Socket = WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>;

    /// What the SSO socket says: `{ success, data: { connection_token? , api_key? }, error }`.
    #[derive(Debug, Default, Deserialize)]
    pub(super) struct Answer {
        #[serde(default)]
        pub success: bool,
        #[serde(default)]
        pub data: Option<AnswerData>,
        #[serde(default)]
        pub error: Option<String>,
    }

    #[derive(Debug, Default, Deserialize)]
    pub(super) struct AnswerData {
        #[serde(default)]
        pub connection_token: Option<String>,
        #[serde(default)]
        pub api_key: Option<String>,
    }

    /// The first request on the socket (protocol 2); `token` resumes a dropped connection.
    pub(super) fn hello(id: &str, token: Option<&str>) -> String {
        serde_json::json!({ "id": id, "token": token, "protocol": 2 }).to_string()
    }

    /// The page the player approves SIGF on.
    pub(super) fn approve_url(id: &str, slug: &str) -> String {
        format!("https://www.nexusmods.com/sso?id={id}&application={slug}")
    }

    /// TLS with the system's roots (as reqwest), on rustls with an explicit provider.
    fn connect() -> Result<Socket, String> {
        let tcp = TcpStream::connect((SSO_HOST, 443)).map_err(|e| e.to_string())?;
        tcp.set_read_timeout(Some(SSO_PING)).map_err(|e| e.to_string())?;
        tcp.set_write_timeout(Some(SSO_PING)).map_err(|e| e.to_string())?;
        use rustls_platform_verifier::BuilderVerifierExt;
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .with_platform_verifier()
            .map_err(|e| e.to_string())?
            .with_no_client_auth();
        let (ws, _) = tungstenite::client_tls_with_config(SSO_URL, tcp, None, Some(tungstenite::Connector::Rustls(Arc::new(config))))
            .map_err(|e| e.to_string())?;
        Ok(ws)
    }

    pub(super) fn api_key(slug: &str, open: &dyn Fn(&str) -> Result<(), String>, generation: u64) -> Result<String, WorkshopError> {
        let id = uuid::Uuid::new_v4().to_string();
        let deadline = Instant::now() + SSO_DEADLINE;
        let mut token: Option<String> = None;
        let mut opened = false;
        let mut last_error = String::from("no answer");
        for _ in 0..3 {
            let mut ws = match connect() {
                Ok(ws) => ws,
                Err(e) => {
                    last_error = e;
                    continue;
                }
            };
            if let Err(e) = ws.send(Message::text(hello(&id, token.as_deref()))) {
                last_error = e.to_string();
                continue;
            }
            if !opened {
                open(&approve_url(&id, slug)).map_err(|m| err("failed", m))?;
                opened = true;
            }
            loop {
                if LOGIN_GEN.load(Ordering::SeqCst) != generation {
                    let _ = ws.close(None);
                    return Err(err("cancelled", "sign-in replaced"));
                }
                if Instant::now() > deadline {
                    let _ = ws.close(None);
                    return Err(err("timeout", "Nexus Mods sign-in was not approved in time"));
                }
                match ws.read() {
                    Ok(Message::Text(t)) => {
                        let a: Answer = serde_json::from_str(t.as_str()).unwrap_or_default();
                        if !a.success {
                            let _ = ws.close(None);
                            return Err(err("nexus_sso", clean_text(a.error.as_deref().unwrap_or("Nexus Mods refused the sign-in"), 200)));
                        }
                        let data = a.data.unwrap_or_default();
                        if let Some(t) = data.connection_token.filter(|t| t.len() <= 512) {
                            token = Some(t);
                        }
                        if let Some(k) = data.api_key {
                            let _ = ws.close(None);
                            return if valid_api_key(&k) { Ok(k) } else { Err(err("nexus_sso", "Nexus Mods sent a key SIGF cannot use")) };
                        }
                    }
                    Ok(Message::Close(_)) => break,
                    Ok(_) => {}
                    Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                        if ws.send(Message::Ping(Vec::new().into())).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        last_error = e.to_string();
                        break;
                    }
                }
            }
        }
        Err(err("network", format!("Nexus Mods sign-in: {last_error}")))
    }
}

/// One download link Nexus offers for a file (one per CDN location).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NexusLink {
    pub name: String,
    pub short_name: String,
    /// https on `*.nexus-cdn.com` (`check::MOD_HOSTS` "nx"): the URL the UI puts into the plan's file.
    pub uri: String,
}

/// Nexus's `download_link.json` answer, keeping only links on the Nexus CDN.
fn parse_links(v: &serde_json::Value) -> Vec<NexusLink> {
    let hosts = check::mod_hosts("nx").unwrap_or(&[]);
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|l| {
                    let uri = l.get("URI").and_then(|u| u.as_str())?;
                    check::mod_url_ok(hosts, uri).then(|| NexusLink {
                        name: clean_text(l.get("name").and_then(|n| n.as_str()).unwrap_or(""), 80),
                        short_name: clean_text(l.get("short_name").and_then(|n| n.as_str()).unwrap_or(""), 40),
                        uri: uri.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Download links for one Nexus file with the player's key: Premium accounts directly, free accounts with the `key`
/// and `expires` of the nxm:// click. Not signed in: `nexus_login_required`; an old click: `nxm_expired`.
#[tauri::command]
pub async fn nexus_download_links(domain: String, mod_id: u64, file_id: u64, key: Option<String>, expires: Option<u64>) -> Result<Vec<NexusLink>, WorkshopError> {
    if !valid_nexus_domain(&domain) || !(1..1_000_000_000_000).contains(&mod_id) || !(1..1_000_000_000_000).contains(&file_id) {
        return Err(err("failed", "bad Nexus file"));
    }
    let query = match (key, expires) {
        (Some(k), Some(e)) if valid_nxm_key(&k) && e < 1_000_000_000_000 => {
            if e <= now_secs() {
                return Err(err("nxm_expired", "this download link has expired: click Mod manager download again"));
            }
            format!("?key={k}&expires={e}")
        }
        (None, None) => String::new(),
        _ => return Err(err("failed", "bad nxm key")),
    };
    tauri::async_runtime::spawn_blocking(move || {
        let s = load_nexus(&nexus_path()).ok_or_else(|| err("nexus_login_required", "sign in to Nexus Mods first"))?;
        let v = nexus_get(&format!("games/{domain}/mods/{mod_id}/files/{file_id}/download_link.json{query}"), &s.api_key)?;
        let links = parse_links(&v);
        if links.is_empty() {
            return Err(err("failed", "Nexus Mods gave no download link"));
        }
        Ok(links)
    })
    .await
    .map_err(|e| err("failed", e.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SHA: &str = "ab00000000000000000000000000000000000000000000000000000000000000";

    fn plan() -> serde_json::Value {
        json!({
            "ref": "ts:notnotnotswipez-MoreCompany",
            "game": "lethalcompany",
            "name": "MoreCompany",
            "version": "1.11.0",
            "deps": [ { "ref": "ts:BepInEx-BepInExPack", "name": "BepInExPack", "version": "5.4.2100" } ],
            "files": [
                { "url": "https://thunderstore.io/package/download/BepInEx/BepInExPack/5.4.2100/", "size": 700000, "name": "BepInEx-BepInExPack-5.4.2100.zip",
                  "unpack": true, "dst": "{game}", "root": "BepInExPack", "of": "ts:BepInEx-BepInExPack" },
                { "url": "https://gcdn.thunderstore.io/live/repository/packages/notnotnotswipez-MoreCompany-1.11.0.zip", "hash": { "sha256": SHA.to_uppercase() },
                  "name": "notnotnotswipez-MoreCompany-1.11.0.zip", "unpack": true, "dst": "{game}/BepInEx/plugins/notnotnotswipez-MoreCompany", "of": "ts:notnotnotswipez-MoreCompany" }
            ]
        })
    }

    fn with_file(f: serde_json::Value) -> String {
        let mut p = plan();
        p["files"][1] = f;
        p.to_string()
    }

    fn file(patch: serde_json::Value) -> serde_json::Value {
        let mut f = plan()["files"][1].clone();
        for (k, v) in patch.as_object().unwrap() {
            f[k] = v.clone();
        }
        f
    }

    fn code(text: &str) -> String {
        check_plan(text).err().map(|e| e.code).unwrap_or_else(|| "ok".into())
    }

    #[test]
    fn good_plan() {
        let p = check_plan(&plan().to_string()).unwrap();
        assert_eq!(p.id, "mod/ts:notnotnotswipez-MoreCompany");
        assert_eq!((p.source.as_str(), p.files.len()), ("ts", 2));
        assert_eq!(p.files[0].dst, "{game}");
        assert!(p.files[0].expected.is_none());
        assert_eq!(p.files[1].expected.as_ref().unwrap().hex, SHA, "lowercased");
        let r = p.recipe(&[SHA.into(), SHA.into()]).unwrap();
        assert_eq!(r.id, "mod/ts:notnotnotswipez-MoreCompany");
        assert_eq!(r.slug(), "mod-ts-notnotnotswipez-morecompany");
        assert_eq!(r.install[0].strategy, install::Strategy::GameDirSnapshot);
        assert_eq!(r.install[0].files[0].root.as_deref(), Some("BepInExPack"));
        assert_eq!(r.install[0].files[1].location(&r.files), plan()["files"][1]["url"]);
        // A plain file goes into the folder `dst` under its name, or to `dst` when it already names it.
        let f = file(json!({ "unpack": false, "name": "MoreCompany.dll", "dst": "{game}/BepInEx/plugins" }));
        assert_eq!(check_plan(&with_file(f)).unwrap().files[1].dst, "{game}/BepInEx/plugins/MoreCompany.dll");
        let f = file(json!({ "unpack": false, "name": "MoreCompany.dll", "dst": "{game}/BepInEx/plugins/morecompany.dll" }));
        assert_eq!(check_plan(&with_file(f)).unwrap().files[1].dst, "{game}/BepInEx/plugins/morecompany.dll");
        let f = file(json!({ "unpack": false, "name": "x.pak", "dst": "{game}" }));
        assert_eq!(check_plan(&with_file(f)).unwrap().files[1].dst, "{game}/x.pak");
        // Names are cleaned, not refused.
        let f = file(json!({ "unpack": false, "name": "a:b?.dll.", "dst": "{game}/Mods" }));
        assert_eq!(check_plan(&with_file(f)).unwrap().files[1].name, "a_b_.dll");
        let mut p = plan();
        p["name"] = json!("  More\u{7}Company ");
        assert_eq!(check_plan(&p.to_string()).unwrap().name, "MoreCompany");
    }

    #[test]
    fn bad_hosts_and_schemes() {
        assert_eq!(code(&with_file(file(json!({ "url": "https://evil.example/a.zip" })))), "bad_host");
        assert_eq!(code(&with_file(file(json!({ "url": "https://cdn.modrinth.com/data/a/b.zip" })))), "bad_host", "another source's host");
        assert_eq!(code(&with_file(file(json!({ "url": "https://thunderstore.io.evil.example/a.zip" })))), "bad_host");
        assert_eq!(code(&with_file(file(json!({ "url": "http://thunderstore.io/package/download/a/b/1/" })))), "bad_plan");
        assert_eq!(code(&with_file(file(json!({ "url": "file:///C:/a.zip" })))), "bad_plan");
        assert_eq!(code(&with_file(file(json!({ "url": "https://thunderstore.io/a/../b.zip" })))), "bad_host");
        // Nexus CDN links carry their token in the query.
        let mut p = plan();
        p["ref"] = json!("nx:skyrimspecialedition:12604");
        p["files"] = json!([ { "url": "https://supporter-files.nexus-cdn.com/1704/12604/SkyUI.zip?md5=aB-1&expires=1760000000&user_id=1", "name": "SkyUI.zip",
            "unpack": true, "dst": "{game}/Data", "of": "nx:skyrimspecialedition:12604", "hash": { "md5": "781e5e245d69b566979b86e28d23f2c7" } } ]);
        let c = check_plan(&p.to_string()).unwrap();
        assert_eq!(c.files[0].expected.as_ref().unwrap().algo, fetch::Algo::Md5);
        assert_eq!(without_query(&c.files[0].url), "https://supporter-files.nexus-cdn.com/1704/12604/SkyUI.zip");
        // The server's Nexus plan for a free account: one file with an empty url and an `nx` block; installable once the
        // UI put the CDN link from nexus_download_links into it.
        p["needs"] = json!("nxm");
        p["nx"] = json!({ "domain": "skyrimspecialedition", "modId": 12604, "fileId": 35407, "filesUrl": "https://www.nexusmods.com/x", "files": [] });
        p["files"][0]["url"] = json!("");
        assert_eq!(code(&p.to_string()), "needs_nxm");
        p["files"][0]["url"] = json!("https://cf-files.nexus-cdn.com/1704/12604/SkyUI.zip?md5=a&expires=1&user_id=2");
        assert_eq!(code(&p.to_string()), "ok");
        // A file of another source, or a Workshop item, is refused.
        assert_eq!(code(&with_file(file(json!({ "of": "cf:1234" })))), "bad_plan");
        let mut p = plan();
        p["ref"] = json!("ws:3012345678");
        assert_eq!(code(&p.to_string()), "bad_plan");
    }

    #[test]
    fn destinations_stay_in_the_game() {
        for dst in [
            "{game}/../x",
            "{game}/BepInEx/../../x",
            "../x",
            "/etc/x",
            "C:/Windows",
            "C:\\Windows",
            "{game}\\..\\x",
            "{app}/x",
            "{docs}/x",
            "{game}x",
            "{game}/a//b",
            "{game}/a/./b",
            "{game}/a:b",
            "{game}/con",
            "{game}/NUL.txt",
            "{game}/a./b",
            "{game}/{game}",
            "{game}/a/",
            "",
        ] {
            assert_eq!(code(&with_file(file(json!({ "dst": dst })))), "bad_plan", "{dst}");
        }
        assert_eq!(code(&with_file(file(json!({ "dst": format!("{{game}}/{}", "a/".repeat(150)) + "b" })))), "bad_plan", "too long");
        assert_eq!(code(&with_file(file(json!({ "root": "../x" })))), "bad_plan");
        assert_eq!(code(&with_file(file(json!({ "root": "a", "unpack": false })))), "bad_plan");
        for name in ["", "..", ".", "  ", "CON", "aux.dll", "..."] {
            assert_eq!(code(&with_file(file(json!({ "name": name })))), "bad_plan", "{name:?}");
        }
    }

    #[test]
    fn limits_hashes_and_archives() {
        let mut p = plan();
        let f = p["files"][1].clone();
        p["files"] = json!(vec![f.clone(); MAX_PLAN_FILES]);
        assert!(check_plan(&p.to_string()).is_ok());
        p["files"] = json!(vec![f; MAX_PLAN_FILES + 1]);
        assert_eq!(code(&p.to_string()), "bad_plan", "too many files");
        assert_eq!(code(&with_file(file(json!({ "size": check::MOD_MAX_FILE_BYTES + 1 })))), "bad_plan");
        assert_eq!(code(&with_file(file(json!({ "size": check::MOD_MAX_FILE_BYTES })))), "ok");
        assert_eq!(code(&with_file(file(json!({ "hash": { "sha256": "abc" } })))), "bad_plan");
        assert_eq!(code(&with_file(file(json!({ "hash": { "sha256": SHA, "md5": "zz" } })))), "bad_plan", "every hash given is well formed");
        let strongest = |h: serde_json::Value| strongest_hash(&serde_json::from_value(h).unwrap()).unwrap().map(|e| e.algo);
        let (md5, sha1, sha512) = ("0".repeat(32), "0".repeat(40), "0".repeat(128));
        assert_eq!(strongest(json!({ "md5": md5, "sha1": sha1 })), Some(fetch::Algo::Sha1));
        assert_eq!(strongest(json!({ "md5": md5, "sha1": sha1, "sha256": SHA, "sha512": sha512 })), Some(fetch::Algo::Sha512));
        assert_eq!(strongest(json!({ "md5": md5, "sha256": SHA })), Some(fetch::Algo::Sha256));
        assert_eq!(strongest(json!({ "md5": md5 })), Some(fetch::Algo::Md5));
        assert_eq!(strongest(json!({})), None);
        assert_eq!(code(&with_file(file(json!({ "name": "SkyUI.7z" })))), "ok", "7z unpacks like zip");
        assert_eq!(code(&with_file(file(json!({ "name": "x.RAR" })))), "unsupported_archive");
        assert_eq!(code(&with_file(file(json!({ "name": "x.7z", "unpack": false, "dst": "{game}/Mods" })))), "ok", "kept as a file");
        let mut p = plan();
        p["files"] = json!([]);
        p["needs"] = json!("nxm");
        assert_eq!(code(&p.to_string()), "needs_nxm");
        p["needs"] = serde_json::Value::Null;
        p["link"] = json!({ "url": "https://www.nexusmods.com/x", "why": "fomod" });
        assert_eq!(code(&p.to_string()), "link_only");
        assert_eq!(code("{"), "bad_plan");
        assert_eq!(code(&json!({ "ref": "ts:x", "game": "Bad Game", "files": [] }).to_string()), "bad_plan");
    }

    #[test]
    fn detect_rules_are_checked() {
        let d = |rules: serde_json::Value| code(&with_file(file(json!({ "detect": rules }))));
        assert_eq!(d(json!([{ "ifContains": "fomod/ModuleConfig.xml", "refuse": "fomod" }, { "ifContains": "*.pak", "dst": "{game}/G/Content/Paks/~mods" },
            { "ifContains": "manifest.json", "up": 1, "dst": "{game}/Mods" }, { "ifContains": ["*.esp", "textures/"], "dst": "{game}/Data" },
            { "ifContains": "*", "dst": "{game}" }])), "ok");
        let p = check_plan(&with_file(file(json!({ "detect": [{ "ifContains": "Data/", "dst": "{game}" }] })))).unwrap();
        assert_eq!(p.files[1].detect.len(), 1);
        for bad in [
            json!([{ "ifContains": "*.pak", "dst": "{game}/../x" }]),
            json!([{ "ifContains": "*.pak", "dst": "{docs}/x" }]),
            json!([{ "ifContains": "*.pak", "dst": "{game}/a/", }]),
            json!([{ "ifContains": "../x/", "dst": "{game}" }]),
            json!([{ "ifContains": "*.pak" }]),
            json!([{ "ifContains": "*.pak", "dst": "{game}", "refuse": "fomod" }]),
            json!([{ "ifContains": "*.pak", "refuse": "Bad Reason" }]),
            json!([{ "ifContains": "*.pak", "dst": "{game}", "up": 5 }]),
            json!([{ "ifContains": [], "dst": "{game}" }]),
            json!([{ "ifContains": ["*.esp", "a/../b/"], "dst": "{game}" }]),
            json!([{ "ifContains": vec!["*"; MAX_DETECT_PATTERNS + 1], "dst": "{game}" }]),
            json!(vec![json!({ "ifContains": "*", "dst": "{game}" }); MAX_DETECT + 1]),
        ] {
            assert_eq!(d(bad.clone()), "bad_plan", "{bad}");
        }
        assert_eq!(code(&with_file(file(json!({ "unpack": false, "name": "a.pak", "dst": "{game}/x", "detect": [{ "ifContains": "*", "dst": "{game}" }] })))), "bad_plan", "rules need an unpacked archive");
    }

    #[test]
    fn layouts_through_the_engine() {
        // A 7z laid out under a wrapper folder, placed by its rules; a FOMOD archive and a variant archive refused.
        let t = tempfile::tempdir().unwrap();
        let game = t.path().join("Game");
        std::fs::create_dir_all(&game).unwrap();
        let seven = |name: &str, entries: &[(&str, &[u8])]| {
            let p = t.path().join(name);
            let mut w = sevenz_rust2::ArchiveWriter::create(&p).unwrap();
            for (n, data) in entries {
                w.push_archive_entry(sevenz_rust2::ArchiveEntry::new_file(n), Some(*data)).unwrap();
            }
            w.finish().unwrap();
            p
        };
        let good = seven("good.7z", &[("Cool Mod v2/Cool_P.pak", b"pak"), ("Cool Mod v2/Cool_P.utoc", b"toc"), ("readme.txt", b"hi")]);
        let fomod = seven("fomod.7z", &[("fomod/ModuleConfig.xml", b"<x/>"), ("A/a.pak", b"a")]);
        let variants = seven("variants.7z", &[("Red/a.pak", b"r"), ("Blue/a.pak", b"b")]);
        let lua = seven("lua.7z", &[("Mod/Scripts/main.lua", b"lua")]);
        let rules: Vec<install::archive::Layout> = check_detect(
            &serde_json::from_value::<Vec<PlanDetect>>(json!([
                { "ifContains": "fomod/ModuleConfig.xml", "refuse": "fomod" },
                { "ifContains": "*.pak", "dst": "{game}/G/Content/Paks/~mods" }
            ]))
            .unwrap(),
            true,
        )
        .unwrap();
        let f = |p: &Path, name: &str| CheckedFile {
            url: p.to_string_lossy().into_owned(),
            name: name.into(),
            size: None,
            expected: None,
            unpack: true,
            dst: "{game}".into(),
            root: None,
            of: "nx:x:1".into(),
            detect: rules.clone(),
        };
        let plan = CheckedPlan {
            id: "mod/nx:x:1".into(),
            item: "nx:x:1".into(),
            game: "testgame".into(),
            name: "Cool".into(),
            version: "2".into(),
            source: "nx".into(),
            hosts: check::mod_hosts("nx").unwrap(),
            files: vec![f(&good, "Cool.7z")],
        };
        let home = t.path().join("home");
        let engine = || {
            let mut e = install::Engine::new(&home, None);
            e.allow_local = true;
            e
        };
        let dirs = HashMap::from([("testgame".to_string(), game.to_string_lossy().into_owned())]);
        install_plan(&mut engine(), &plan, &dirs, &mut |_| {}).unwrap();
        let mods = game.join("G/Content/Paks/~mods");
        assert_eq!(std::fs::read(mods.join("Cool_P.pak")).unwrap(), b"pak");
        assert_eq!(std::fs::read(mods.join("Cool_P.utoc")).unwrap(), b"toc");
        assert!(!game.join("readme.txt").exists() && !mods.join("readme.txt").exists(), "outside the detected root");
        install::Engine::new(&home, None).restore("mod/nx:x:1", false).unwrap();
        assert!(!mods.join("Cool_P.pak").exists());
        for (p, want) in [(&fomod, "FOMOD"), (&variants, "variants"), (&lua, "laid out")] {
            let e = install_plan(&mut engine(), &CheckedPlan { files: vec![f(p, "x.7z")], ..plan.clone() }, &dirs, &mut |_| {}).unwrap_err();
            assert_eq!(e.code, "unsupported_archive", "{p:?}");
            assert!(e.message.contains(want), "{p:?}: {}", e.message);
        }
        assert!(!game.join("G").join("Content").join("Paks").join("~mods").join("a.pak").exists());
        // A RAR is named as such.
        let rar = t.path().join("x.rar");
        std::fs::write(&rar, b"Rar!\x1a\x07\x00rest").unwrap();
        let e = install_plan(&mut engine(), &CheckedPlan { files: vec![CheckedFile { detect: vec![], ..f(&rar, "x.rar") }], ..plan.clone() }, &dirs, &mut |_| {}).unwrap_err();
        assert_eq!(e.code, "unsupported_archive");
        assert!(e.message.contains("RAR"), "{}", e.message);
    }

    #[test]
    fn refs() {
        for ok in ["ts:BepInEx-BepInExPack", "cf:238222", "nx:skyrimspecialedition:12604", "mio:1:2", "gb:Mod:123", "mr:AANobbMI", "ws:3012345678"] {
            assert!(valid_ref(ok), "{ok}");
        }
        for bad in ["", "ts", "ts:", "xx:1", "ts:a b", "ts:a/b", "ws:abc", "ws:0", "TS:x", &format!("ts:{}", "a".repeat(118))] {
            assert!(!valid_ref(bad), "{bad}");
        }
        assert!(valid_ref(&format!("ts:{}", "a".repeat(117))));
        assert_eq!(ref_source("nx:skyrimspecialedition:1"), Some("nx"));
    }

    #[test]
    fn install_through_the_engine() {
        // A zip and a plain file from local paths (dev mode), installed into a game folder and restored.
        let t = tempfile::tempdir().unwrap();
        let game = t.path().join("Game");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(game.join("keep.txt"), b"original").unwrap();
        let zip_path = t.path().join("pack.zip");
        {
            let mut z = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
            let o = zip::write::SimpleFileOptions::default();
            z.start_file("BepInExPack/winhttp.dll", o).unwrap();
            std::io::Write::write_all(&mut z, b"dll").unwrap();
            z.start_file("BepInExPack/keep.txt", o).unwrap();
            std::io::Write::write_all(&mut z, b"modded").unwrap();
            z.start_file("icon.png", o).unwrap();
            z.finish().unwrap();
        }
        let plain = t.path().join("Plugin.dll");
        std::fs::write(&plain, b"plugin").unwrap();
        let md5 = fetch::hash_file(&plain, fetch::Algo::Md5).unwrap();
        // Local paths pass `fetch` only in dev mode; the plan rule wants https, so build the checked plan directly.
        let hosts = check::mod_hosts("ts").unwrap();
        let f = |url: &Path, name: &str, unpack: bool, dst: &str, root: Option<&str>, expected| CheckedFile {
            url: url.to_string_lossy().into_owned(),
            name: name.into(),
            size: None,
            expected,
            unpack,
            dst: dst.into(),
            root: root.map(Into::into),
            of: "ts:a-b".into(),
            detect: vec![],
        };
        let plan = CheckedPlan {
            id: "mod/ts:a-b".into(),
            item: "ts:a-b".into(),
            game: "testgame".into(),
            name: "A".into(),
            version: "1.0.0".into(),
            source: "ts".into(),
            hosts,
            files: vec![
                f(&zip_path, "pack.zip", true, "{game}", Some("BepInExPack"), None),
                f(&plain, "Plugin.dll", false, "{game}/BepInEx/plugins/a-b/Plugin.dll", None, Some(fetch::Expected::new(fetch::Algo::Md5, &md5).unwrap())),
            ],
        };
        let home = t.path().join("home");
        let engine = || {
            let mut e = install::Engine::new(&home, None);
            e.allow_local = true;
            e
        };
        let dirs = HashMap::from([("testgame".to_string(), game.to_string_lossy().into_owned())]);
        let mut events = vec![];
        let m = install_plan(&mut engine(), &plan, &dirs, &mut |p| events.push((p.phase, p.pct))).unwrap();
        assert_eq!(std::fs::read(game.join("winhttp.dll")).unwrap(), b"dll");
        assert_eq!(std::fs::read(game.join("keep.txt")).unwrap(), b"modded");
        assert!(!game.join("icon.png").exists(), "outside the root");
        assert_eq!(std::fs::read(game.join("BepInEx/plugins/a-b/Plugin.dll")).unwrap(), b"plugin");
        assert_eq!(m.files.iter().map(|f| f.verified.as_str()).collect::<Vec<_>>(), ["none", "md5"]);
        assert_eq!(m.files[0].sha256, fetch::sha256_file(&zip_path).unwrap());
        let saved = install::registry::load(&home);
        assert_eq!(saved[0].files, m.files, "recorded in installed.json");
        assert!(events.windows(2).all(|w| w[0].1 <= w[1].1), "the bar never goes back: {events:?}");
        assert_eq!(events.last().unwrap().0, install::Phase::Ready);
        // Uninstall is the usual restore.
        install::Engine::new(&home, None).restore("mod/ts:a-b", false).unwrap();
        assert_eq!(std::fs::read(game.join("keep.txt")).unwrap(), b"original");
        assert!(!game.join("winhttp.dll").exists() && !game.join("BepInEx").exists());
        // Not a zip where a zip is unpacked: unsupported_archive, nothing written.
        let mut p7 = plan.clone();
        p7.files = vec![f(&plain, "x.zip", true, "{game}", None, None)];
        assert_eq!(install_plan(&mut engine(), &p7, &dirs, &mut |_| {}).unwrap_err().code, "unsupported_archive");
        // A wrong hash stops it before anything is written.
        let mut bad = plan.clone();
        bad.files[1].expected = Some(fetch::Expected::new(fetch::Algo::Sha1, &"0".repeat(40)).unwrap());
        assert_eq!(install_plan(&mut engine(), &bad, &dirs, &mut |_| {}).unwrap_err().code, "hash_mismatch");
        assert!(!game.join("winhttp.dll").exists());
    }

    #[test]
    fn nxm_links() {
        let l = parse_nxm("nxm://SkyrimSpecialEdition/mods/12604/files/35407?key=AbC_12-x&expires=1760000000&user_id=123").unwrap();
        assert_eq!(
            l,
            NxmLink { domain: "skyrimspecialedition".into(), mod_id: 12604, file_id: 35407, key: "AbC_12-x".into(), expires: 1760000000, user_id: Some(123) }
        );
        assert_eq!(l.to_link(), "nxm://skyrimspecialedition/mods/12604/files/35407?key=AbC_12-x&expires=1760000000&user_id=123");
        assert_eq!(parse_nxm(&l.to_link()), Some(l.clone()), "the normalized form parses back");
        // Order, unknown parameters and a missing user_id are fine.
        let l2 = parse_nxm(" NXM://stardewvalley/mods/1/files/2?expires=99&campaign=x&key=k ").unwrap();
        assert_eq!(l2.to_link(), "nxm://stardewvalley/mods/1/files/2?key=k&expires=99");
        for bad in [
            "nxm://skyrimspecialedition/mods/12604/files/35407",
            "nxm://skyrimspecialedition/mods/12604/files/35407?key=a",
            "nxm://skyrimspecialedition/mods/12604/files/35407?expires=1",
            "nxm://skyrimspecialedition/mods/12604/files/35407?key=a&key=b&expires=1",
            "nxm://skyrimspecialedition/mods/12604/files/35407?key=a b&expires=1",
            "nxm://skyrimspecialedition/mods/12604/files/35407?key=a%22&expires=1",
            "nxm://skyrimspecialedition/mods/12604/files/35407?key=a&expires=1x",
            "nxm://skyrimspecialedition/mods/12604/files/35407?key=a&expires=1&user_id=-1",
            "nxm://skyrimspecialedition/mods/012604/files/35407?key=a&expires=1",
            "nxm://skyrimspecialedition/mods/12604/files/35407/?key=a&expires=1",
            "nxm://skyrimspecialedition/mods/12604/files/35407?key=a&expires=1#x",
            "nxm://skyrim_se/mods/1/files/2?key=a&expires=1",
            "nxm://../mods/1/files/2?key=a&expires=1",
            "nxm://skyrimspecialedition/collections/abc/revisions/3?key=a&expires=1",
            "nxm://skyrimspecialedition/mods/1/files?key=a&expires=1",
            "nxm:/skyrimspecialedition/mods/1/files/2?key=a&expires=1",
            "https://www.nexusmods.com/skyrimspecialedition/mods/1",
            "sigf://join/k3m9xq2wa7fd",
        ] {
            assert!(parse_nxm(bad).is_none(), "{bad}");
        }
        assert!(parse_nxm(&format!("nxm://a/mods/1/files/2?key={}&expires=1", "k".repeat(129))).is_none());
        assert_eq!(nxm_arg(&["SIGF.exe".into(), "nxm://a/mods/1/files/2?key=k&expires=1".into()]).as_deref(), Some("nxm://a/mods/1/files/2?key=k&expires=1"));
        assert!(nxm_arg(&["SIGF.exe".into(), "sigf://join/x".into()]).is_none());
        assert!(nxm_arg(&["SIGF.exe".into(), "nxm://a".into(), "--x".into()]).is_none());
    }

    #[test]
    fn nexus_store_and_answers() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("nexus.json");
        assert!(load_nexus(&p).is_none());
        let s = NexusStore { api_key: "abc+/=_-123".into(), name: Some("player".into()), premium: Some(false), user_id: Some(5) };
        save_nexus(&p, &s).unwrap();
        let back = load_nexus(&p).unwrap();
        assert_eq!(status_of(Some(&back)), NexusStatus { connected: true, name: Some("player".into()), premium: Some(false) });
        assert!(!format!("{back:?}").contains("abc+"), "the key never shows in Debug");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        }
        std::fs::write(&p, r#"{"api_key":"bad key\n"}"#).unwrap();
        assert!(load_nexus(&p).is_none(), "a key that cannot go into a header reads as signed out");
        assert_eq!(serde_json::to_value(status_of(None)).unwrap(), json!({ "connected": false }));
        // SSO messages.
        assert_eq!(serde_json::from_str::<serde_json::Value>(&sso::hello("u-1", None)).unwrap(), json!({ "id": "u-1", "token": null, "protocol": 2 }));
        assert_eq!(serde_json::from_str::<serde_json::Value>(&sso::hello("u-1", Some("t"))).unwrap()["token"], "t");
        assert_eq!(sso::approve_url("u-1", "sigf"), "https://www.nexusmods.com/sso?id=u-1&application=sigf");
        let a: sso::Answer = serde_json::from_str(r#"{"success":true,"data":{"api_key":"K"},"error":null}"#).unwrap();
        assert_eq!(a.data.unwrap().api_key.as_deref(), Some("K"));
        // Download links: only the Nexus CDN.
        let v = json!([
            { "name": "Nexus Global Content Delivery Network", "short_name": "Nexus CDN", "URI": "https://cf-files.nexus-cdn.com/1704/12604/SkyUI.7z?md5=x&expires=1&user_id=2" },
            { "name": "Evil", "short_name": "Evil", "URI": "https://evil.example/SkyUI.7z" },
            { "name": "Plain", "short_name": "Plain", "URI": "http://cf-files.nexus-cdn.com/a.zip" }
        ]);
        let links = parse_links(&v);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].short_name, "Nexus CDN");
        assert!(parse_links(&json!({ "error": "x" })).is_empty());
        assert!(valid_nexus_domain("baldursgate3") && !valid_nexus_domain("Skyrim") && !valid_nexus_domain("-x") && !valid_nexus_domain(""));
    }

    #[test]
    fn personal_keys() {
        let k = "aBcD1234eFgH5678+/==--Zz09_-xyz";
        assert_eq!(personal_key(&format!("  {k}\n")).as_deref(), Some(k), "pasted with spaces and a newline");
        assert!(personal_key("short-key").is_none());
        assert!(personal_key("").is_none());
        assert!(personal_key("aBcD1234eFgH5678 inner space").is_none());
        assert!(personal_key("aBcD1234eFgH5678\r\nX-Evil: 1").is_none(), "no header injection");
        assert!(personal_key(&"a".repeat(513)).is_none());
        assert!(personal_key(&"a".repeat(512)).is_some());
    }
}
