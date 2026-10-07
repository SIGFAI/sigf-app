//! What the app itself enforces on a recipe, so it never relies on sigf.ai or its own UI alone
//! (docs/RECIPE-FORMAT.md section 4, "Download rule in the app"). Mirrors the sigf.ai catalog's own rule:
//!
//! - `UrlPolicy`: every download is a release asset of the recipe's own SIGFAI repo
//!   (`https://github.com/SIGFAI/<name>/releases/download/<tag>/<file>`, `<name>` from the id `sigf/<name>`), or, for an
//!   upstream fetch (`source.fetch: "upstream"`), a file of that one pinned upstream release
//!   (`<source.repo>/releases/download/<source.tag>/<file>`). Files listed inside an mrpack may also come from Modrinth's
//!   CDN. `file://` and local paths only in dev mode (`SIGF_DEV_LOCAL_RECIPES=1`, read by the example CLI and the tests;
//!   the shipped app never reads it).
//! - `check_recipe`: the whole catalog rule on a recipe's text (ids, games, steps, destinations, launch, sizes), run by
//!   the Tauri command handlers on whatever the webview hands them.
//! - `game_dirs_ok`: the `{game}` folders the UI names must be install folders the core's own scan found.
//! - Bring your own copy and player builds (`own_copies`, `player_build`): no download at all for an own copy (the
//!   file comes from the player's PC); a player build's script is a release asset of the recipe's own SIGFAI repo, its
//!   inputs are commit-pinned GitHub sources (`build_input_url_ok`), its toolchain one of the app's own pinned
//!   downloads (`install::tools::TOOLS`). Those hosts are allowed for build downloads only (`BUILD_DOWNLOAD_HOSTS`).

use super::recipe::Recipe;
use super::{mrpack, paths, InstallError};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The GitHub account whose repos host recipes and their files (the server's `APP_RECIPE_OWNERS` default).
pub const OWNER: &str = "SIGFAI";
/// Largest recipe text accepted (same as the server's `RECIPE_MAX_BYTES`).
pub const RECIPE_MAX_BYTES: usize = 256 * 1024;
/// Largest single download: a declared size above it is refused, and no download may grow past it.
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Dev flag: `1` lets recipes name `file://` URLs and local paths (example CLI, tests). Never set by the app.
pub const DEV_LOCAL_ENV: &str = "SIGF_DEV_LOCAL_RECIPES";
/// Where a download may start: GitHub release pages and Modrinth's CDN (mrpack entries).
pub const DOWNLOAD_HOSTS: &[&str] = &["github.com", "cdn.modrinth.com"];
/// Where a download may be redirected to: GitHub's release storage and Modrinth's CDN, https only.
pub const REDIRECT_HOSTS: &[&str] = &["objects.githubusercontent.com", "release-assets.githubusercontent.com", "cdn.modrinth.com"];
const MODRINTH_CDN: &str = "https://cdn.modrinth.com/data/";
/// Player builds only (`FetchOpts::build`): where their downloads may start besides `DOWNLOAD_HOSTS`. Commit-pinned
/// sources on GitHub (`raw.githubusercontent.com`), and python.org for the pinned Python of `install::tools::TOOLS`.
pub const BUILD_DOWNLOAD_HOSTS: &[&str] = &["raw.githubusercontent.com", "www.python.org"];
/// Player builds only: GitHub source archives redirect to `codeload.github.com`.
pub const BUILD_REDIRECT_HOSTS: &[&str] = &["codeload.github.com"];

/// `SIGF_DEV_LOCAL_RECIPES=1`.
pub fn dev_local_recipes() -> bool {
    std::env::var(DEV_LOCAL_ENV).is_ok_and(|v| v == "1")
}

fn bad(m: impl Into<String>) -> InstallError {
    InstallError::recipe(m)
}

/// `url` when it is a plain https URL whose path the HTTP client cannot fold into another one: ASCII with no
/// backslash, control or space, no `.` or `..` segment, no percent-encoded dot, slash or backslash (`%2e` is a dot
/// segment to URL parsers), and no user, port, query or fragment. Prefix checks then hold on the raw text.
pub fn canonical_https(url: &str) -> Option<reqwest::Url> {
    if url.len() > 2048 || !url.is_ascii() || url.bytes().any(|b| b <= b' ' || b == b'\\' || b == 0x7f) {
        return None;
    }
    let low = url.to_ascii_lowercase();
    if ["%2e", "%2f", "%5c", "%00"].iter().any(|p| low.contains(p)) {
        return None;
    }
    let path = low.strip_prefix("https://")?.split_once('/').map_or("", |(_, p)| p);
    if path.split('/').any(|seg| seg == "." || seg == "..") {
        return None;
    }
    let u = reqwest::Url::parse(url).ok()?;
    (u.scheme() == "https"
        && u.username().is_empty()
        && u.password().is_none()
        && u.port().is_none()
        && u.query().is_none()
        && u.fragment().is_none())
    .then_some(u)
}

/// A host a download may start from (`DOWNLOAD_HOSTS`), https only.
pub fn download_start_ok(u: &reqwest::Url) -> bool {
    u.scheme() == "https" && u.port().is_none() && u.username().is_empty() && u.host_str().is_some_and(|h| DOWNLOAD_HOSTS.contains(&h))
}

/// A redirect target a download may follow (`REDIRECT_HOSTS`), https only.
pub fn redirect_ok(u: &reqwest::Url) -> bool {
    u.scheme() == "https" && u.port().is_none() && u.username().is_empty() && u.host_str().is_some_and(|h| REDIRECT_HOSTS.contains(&h))
}

/// A player build's download start (`DOWNLOAD_HOSTS` + `BUILD_DOWNLOAD_HOSTS`), https only.
pub fn build_start_ok(u: &reqwest::Url) -> bool {
    download_start_ok(u)
        || (u.scheme() == "https" && u.port().is_none() && u.username().is_empty() && u.host_str().is_some_and(|h| BUILD_DOWNLOAD_HOSTS.contains(&h)))
}

/// A player build's redirect (`REDIRECT_HOSTS` + `BUILD_REDIRECT_HOSTS`), https only.
pub fn build_redirect_ok(u: &reqwest::Url) -> bool {
    redirect_ok(u)
        || (u.scheme() == "https" && u.port().is_none() && u.username().is_empty() && u.host_str().is_some_and(|h| BUILD_REDIRECT_HOSTS.contains(&h)))
}

fn hex_ok(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A GitHub owner and repo name pair (`owner/repo`).
fn owner_repo_ok(owner: &str, repo: &str) -> bool {
    !owner.is_empty() && owner.len() <= 39 && owner.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') && repo_name_ok(repo)
}

/// A player build input (docs/RECIPE-FORMAT.md section 4, "Player build"): a GitHub source pinned to a commit, either
/// the archive of that commit (`https://github.com/<owner>/<repo>/archive/<40 hex>.zip`) or one file of it
/// (`https://raw.githubusercontent.com/<owner>/<repo>/<40 hex>/<path>`). A branch or tag name is never accepted.
pub fn build_input_url_ok(url: &str) -> bool {
    if canonical_https(url).is_none() {
        return false;
    }
    if let Some(rest) = url.strip_prefix("https://github.com/") {
        let segs: Vec<&str> = rest.split('/').collect();
        return segs.len() == 4
            && owner_repo_ok(segs[0], segs[1])
            && segs[2] == "archive"
            && segs[3].strip_suffix(".zip").is_some_and(|c| hex_ok(c, 40));
    }
    if let Some(rest) = url.strip_prefix("https://raw.githubusercontent.com/") {
        let segs: Vec<&str> = rest.split('/').collect();
        return segs.len() >= 4 && segs.len() <= 16 && owner_repo_ok(segs[0], segs[1]) && hex_ok(segs[2], 40) && segs[3..].iter().all(|s| upstream_name_ok(s));
    }
    false
}

/// A location that is not a network URL: `file://` or a bare path.
pub fn is_local(location: &str) -> bool {
    location.to_ascii_lowercase().starts_with("file://") || !location.contains("://")
}

fn seg_ok(s: &str) -> bool {
    !s.is_empty() && s != "." && s != ".." && s.len() <= 200
}

/// `[A-Za-z0-9._+-]{1,200}` with no `..`: one upstream release file name (same rule as the server).
fn upstream_name_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= 200 && !s.contains("..") && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
}

/// Most mashups a recipe's `conflicts` may name.
pub const CONFLICTS_MAX: usize = 16;

/// `conflicts`: 1 to `CONFLICTS_MAX` distinct mashup ids (`sigf/<repo name>`), never the recipe's own. The ids need not
/// be published (yet). Same rule as the catalog's `conflictsOk`.
pub fn conflicts_ok(v: &Value, own_id: &str) -> bool {
    v.as_array().is_some_and(|a| {
        (1..=CONFLICTS_MAX).contains(&a.len())
            && a.iter().all(|c| c.as_str().is_some_and(|c| c != own_id && c.strip_prefix("sigf/").is_some_and(repo_name_ok)))
            && a.iter().enumerate().all(|(i, c)| !a[..i].contains(c))
    })
}

/// A GitHub repo name: `[A-Za-z0-9._-]{1,100}`, not `.`-led, not `.git`.
pub(crate) fn repo_name_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= 100 && !s.starts_with('.') && !s.ends_with(".git") && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

/// `https://github.com/<owner>/<repo>` (the server's `REPO_RE`).
pub(crate) fn repo_url_ok(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("https://github.com/") else { return false };
    let Some((owner, name)) = rest.split_once('/') else { return false };
    !owner.is_empty() && owner.len() <= 39 && owner.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') && !name.is_empty() && name.len() <= 100
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

/// The SIGFAI repo a recipe id `sigf/<name>` is published from: `https://github.com/SIGFAI/<name>`.
pub fn hosted_repo(id: &str) -> Option<String> {
    let name = id.strip_prefix("sigf/").filter(|n| repo_name_ok(n))?;
    Some(format!("https://github.com/{OWNER}/{name}"))
}

/// An upstream fusion (docs/RECIPE-FORMAT.md section 4, "Upstream fusions"): `source.hosted` is the recipe's own SIGFAI repo,
/// `source.repo` credits another GitHub repo, `source.commit` pins it, `built_by.author` names its author.
fn upstream_fusion(r: &Recipe, hosted: &str) -> bool {
    let Some(src) = &r.source else { return false };
    src.hosted.as_deref().map(|h| h.trim_end_matches('/')) == Some(hosted)
        && src.repo.as_deref().is_some_and(|repo| repo != hosted && repo_url_ok(repo))
        && src.commit.as_deref().is_some_and(|c| c.len() == 40 && c.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
        && r.built_by.as_ref().and_then(|b| b.author.as_deref()).is_some_and(|a| !a.is_empty() && a.chars().count() <= 60)
}

/// Where a recipe may download from (see the module doc).
#[derive(Debug, Clone)]
pub struct UrlPolicy {
    /// `https://github.com/SIGFAI/<name>/releases/download/`, or None when the id names no SIGFAI repo.
    hosted: Option<String>,
    /// `<source.repo>/releases/download/<source.tag>/` for an upstream fetch.
    upstream: Option<String>,
    /// Dev mode: `file://` and local paths allowed.
    pub allow_local: bool,
}

impl UrlPolicy {
    pub fn for_recipe(r: &Recipe, allow_local: bool) -> Self {
        let repo = hosted_repo(&r.id);
        let upstream = repo.as_deref().and_then(|hosted| {
            let src = r.source.as_ref()?;
            let tag = src.tag.as_deref().filter(|t| t.len() <= 100 && !t.is_empty() && t.bytes().all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b)))?;
            (src.fetch.as_deref() == Some("upstream") && upstream_fusion(r, hosted)).then(|| format!("{}/releases/download/{tag}/", src.repo.as_deref().unwrap_or_default()))
        });
        // An upstream fusion names its SIGFAI repo; a different one means the id and the source disagree: no host.
        let hosted_ok = match r.source.as_ref().and_then(|s| s.hosted.as_deref()) {
            Some(h) => repo.as_deref() == Some(h.trim_end_matches('/')),
            None => true,
        };
        Self { hosted: repo.filter(|_| hosted_ok).map(|h| format!("{h}/releases/download/")), upstream, allow_local }
    }

    /// A recipe download (install file, `requires[].source`, mrpack pack): a release asset of the recipe's own SIGFAI
    /// repo (`<tag>/<file>`), or one file of its pinned upstream release.
    pub fn recipe_url_ok(&self, url: &str) -> bool {
        if canonical_https(url).is_none() {
            return false;
        }
        let hosted = self.hosted.as_deref().and_then(|p| url.strip_prefix(p)).is_some_and(|tail| {
            let segs: Vec<&str> = tail.split('/').collect();
            segs.len() == 2 && segs.iter().all(|s| seg_ok(s))
        });
        hosted || self.upstream.as_deref().and_then(|p| url.strip_prefix(p)).is_some_and(upstream_name_ok)
    }

    /// A release asset of the recipe's own SIGFAI repo only (a player build's script: never an upstream file).
    pub fn hosted_url_ok(&self, url: &str) -> bool {
        canonical_https(url).is_some()
            && self.hosted.as_deref().and_then(|p| url.strip_prefix(p)).is_some_and(|tail| {
                let segs: Vec<&str> = tail.split('/').collect();
                segs.len() == 2 && segs.iter().all(|s| seg_ok(s))
            })
    }

    /// A player build download: its script must be `hosted_url_ok`, an input `build_input_url_ok` or a file of the
    /// recipe's own releases. Local files only in dev mode.
    pub fn check_build(&self, location: &str, script: bool) -> Result<(), InstallError> {
        if is_local(location) {
            return self.check(location, false);
        }
        let ok = if script { self.hosted_url_ok(location) } else { build_input_url_ok(location) || self.hosted_url_ok(location) };
        if ok {
            Ok(())
        } else {
            Err(InstallError::Download {
                url: location.into(),
                message: if script {
                    "a build script must be a release file of the mod's own SIGFAI repo".into()
                } else {
                    "a build input must be a commit-pinned GitHub source or a release file of the mod's own SIGFAI repo".into()
                },
            })
        }
    }

    /// A file listed inside an mrpack (`modrinth.index.json` downloads): Modrinth's CDN, or what `recipe_url_ok` allows.
    pub fn index_url_ok(&self, url: &str) -> bool {
        self.recipe_url_ok(url)
            || (canonical_https(url).is_some() && url.strip_prefix(MODRINTH_CDN).is_some_and(|t| t.split('/').all(seg_ok)))
    }

    /// `location` may be fetched: a local one only in dev mode, a URL only when the rule allows it.
    pub fn check(&self, location: &str, index: bool) -> Result<(), InstallError> {
        if is_local(location) {
            return if self.allow_local {
                Ok(())
            } else {
                Err(InstallError::Download { url: location.into(), message: format!("local files are refused (dev only: {DEV_LOCAL_ENV}=1)") })
            };
        }
        let ok = if index { self.index_url_ok(location) } else { self.recipe_url_ok(location) };
        if ok {
            Ok(())
        } else {
            Err(InstallError::Download {
                url: location.into(),
                message: "not an allowed download: only release files of the mod's own SIGFAI repo (or its pinned upstream release)".into(),
            })
        }
    }
}

// ---------- the catalog's recipe rule ----------

const KINDS: &[&str] = &["mod", "mashup", "passthrough"];
const STRATEGIES: &[&str] = &["args", "mrpack", "profile", "game-dir-snapshot"];
const ROOTS: &[&str] = &["{app}", "{game}", "{docs}", "{fivem}"];

fn game_ok(v: &Value) -> bool {
    v.as_str().is_some_and(|s| !s.is_empty() && s.len() <= 40 && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'))
}
/// `platforms`: 1 to 4 distinct ids of `[a-z0-9-]{1,20}`. Ids the app does not know (`linux`) are allowed and ignored
/// (`install::platform::platforms`), so a newer recipe still installs where it can.
fn platforms_ok(v: &Value) -> bool {
    v.as_array().is_some_and(|a| {
        (1..=4).contains(&a.len())
            && a.iter().all(|p| p.as_str().is_some_and(|s| !s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')))
            && a.iter().enumerate().all(|(i, p)| !a[..i].contains(p))
    })
}
fn sha_ok(v: &Value) -> bool {
    v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
}
fn text_ok(v: &Value, max: usize) -> bool {
    v.as_str().is_some_and(|s| !s.is_empty() && s.chars().count() <= max)
}
fn version_ok(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 3 && [4, 4, 6].iter().zip(&parts).all(|(max, p)| !p.is_empty() && p.len() <= *max && p.bytes().all(|b| b.is_ascii_digit()))
}
fn plain_segments(rest: &str) -> bool {
    rest.split('/').all(|seg| !seg.is_empty() && seg != "." && seg != ".." && !seg.contains(['{', '}', ':', '\\']))
}

/// An install destination: one root placeholder, then plain relative segments (empty only for an unpacked zip).
pub fn dst_ok(dst: &str, unpack: bool) -> bool {
    if dst.chars().count() > 300 {
        return false;
    }
    let Some(root) = ROOTS.iter().find(|r| dst == **r || dst.starts_with(&format!("{r}/"))) else { return false };
    let rest = dst.get(root.len() + 1..).unwrap_or("");
    if rest.is_empty() {
        return unpack;
    }
    plain_segments(rest)
}

/// Every `{name}` in a launch arg is a known root placeholder.
fn placeholders_known(s: &str) -> bool {
    let mut rest = s;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else { return true };
        let inner = &after[..end];
        if !inner.is_empty() && inner.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') && !ROOTS.contains(&format!("{{{inner}}}").as_str()) {
            return false;
        }
        rest = &after[end + 1..];
    }
    true
}

/// A launch `exe`: a `.exe` inside the game folder (`{game}/` prefix optional), plain relative segments.
fn exe_ok(exe: &str) -> bool {
    if exe.chars().count() > 200 {
        return false;
    }
    let rest = exe.strip_prefix("{game}/").unwrap_or(exe);
    rest.to_ascii_lowercase().ends_with(".exe") && plain_segments(rest)
}

/// A path a launch or prerequisite names inside its folder (after its `{app}/` or `{game}/` prefix): plain relative
/// segments, at most 200 chars, ending in `ext` (case-insensitive) when `ext` is given. Same rule as the catalog's
/// `launchRelOk`.
pub fn launch_rel_ok(rel: &str, ext: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    rel.chars().count() <= 200 && plain_segments(rel) && (ext.is_empty() || (name.len() > ext.len() && name.to_ascii_lowercase().ends_with(ext)))
}

/// me3's `--savefile`: a plain `.sl2` file name, `[A-Za-z0-9._-]{1,64}`, not dot-led, no `..`.
pub fn savefile_ok(s: &str) -> bool {
    (5..=64).contains(&s.len())
        && !s.starts_with('.')
        && !s.contains("..")
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        && s.to_ascii_lowercase().ends_with(".sl2")
}

/// A page the app may open for the player (`requires_files[].page`): a plain https URL (`canonical_https`: no user,
/// port, query, fragment or dot segment; no quote, `<`, `>` or `@`), at most 300 chars. Same rule as the catalog's `pageOk`.
pub fn page_ok(p: &str) -> bool {
    p.len() <= 300 && !p.contains(['"', '<', '>', '@']) && canonical_https(p).is_some_and(|u| u.host_str().is_some_and(|h| h.contains('.')))
}

/// What the player reads for a missing prerequisite: 1 to 120 chars, no control character.
pub fn message_ok(m: &str) -> bool {
    !m.trim().is_empty() && m.chars().count() <= 120 && !m.chars().any(char::is_control)
}

/// Most `requires_files` entries a recipe may list.
pub const REQUIRES_FILES_MAX: usize = 8;

/// `requires_files` (docs/RECIPE-FORMAT.md section 4): 1 to `REQUIRES_FILES_MAX` of `{ id, game, path, message, page }`,
/// `game` one of `games[]`, `path` `{game}/<plain segments>`. Same rule as the catalog's `requiresFilesOk`.
pub fn requires_files_ok(v: &Value, games: &[&str]) -> bool {
    v.as_array().is_some_and(|a| {
        (1..=REQUIRES_FILES_MAX).contains(&a.len())
            && a.iter().all(|f| {
                f.as_object().is_some_and(|f| {
                    game_ok(f.get("id").unwrap_or(&Value::Null))
                        && f.get("game").and_then(Value::as_str).is_some_and(|g| games.contains(&g))
                        && f.get("path").and_then(Value::as_str).and_then(|p| p.strip_prefix("{game}/")).is_some_and(|r| launch_rel_ok(r, ""))
                        && f.get("message").and_then(Value::as_str).is_some_and(message_ok)
                        && f.get("page").and_then(Value::as_str).is_some_and(page_ok)
                })
            })
    })
}

/// `{app}/<rel>` or `{game}/<rel>` ending in `ext`: a file the launch takes from the mashup's own folder or the game's.
fn app_or_game_ok(v: &Value, ext: &str) -> bool {
    v.as_str().is_some_and(|s| {
        let rest = s.strip_prefix("{app}/").or_else(|| s.strip_prefix("{game}/"));
        rest.is_some_and(|r| launch_rel_ok(r, ext))
    })
}

/// `launch[].me3` (docs/RECIPE-FORMAT.md section 4, "me3 launch"): `profile` a `.me3` in `{app}` or `{game}`, `exe`
/// (optional) a `me3.exe` there, `savefile` (optional) a `.sl2` name, `disable_arxan` (optional) a boolean.
fn me3_ok(v: &Value) -> bool {
    let Some(m) = v.as_object() else { return false };
    app_or_game_ok(m.get("profile").unwrap_or(&Value::Null), ".me3")
        && m.get("exe").is_none_or(|x| app_or_game_ok(x, ".exe") && x.as_str().is_some_and(|x| x.to_ascii_lowercase().ends_with("/me3.exe")))
        && m.get("savefile").is_none_or(|x| x.as_str().is_some_and(savefile_ok))
        && m.get("disable_arxan").is_none_or(Value::is_boolean)
}

/// An install file the app can place: `dst`, `unpack`, `contents`, `root` (see the server's `placeable`).
fn placeable(f: &Map<String, Value>) -> bool {
    let unpack = match f.get("unpack") {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return false,
    };
    if let Some(d) = f.get("dst") {
        if !d.as_str().is_some_and(|d| dst_ok(d, unpack)) {
            return false;
        }
    }
    let contents = match f.get("contents") {
        None => None,
        Some(Value::Array(a)) => Some(a),
        Some(_) => return false,
    };
    if let Some(c) = contents {
        let ok = c.iter().all(|c| {
            c.as_object().is_some_and(|c| sha_ok(c.get("sha256").unwrap_or(&Value::Null)) && c.get("path").and_then(Value::as_str).is_some_and(|p| dst_ok(&format!("{{app}}/{p}"), false)))
        });
        if !ok {
            return false;
        }
    }
    match f.get("root") {
        None => true,
        Some(r) => {
            let Some(r) = r.as_str() else { return false };
            if !unpack || r.chars().count() > 300 || !dst_ok(&format!("{{app}}/{r}"), false) {
                return false;
            }
            let prefix = format!("{r}/");
            contents.is_none_or(|c| c.iter().any(|c| c.get("path").and_then(Value::as_str).is_some_and(|p| p.starts_with(&prefix) && p.len() > prefix.len())))
        }
    }
}

/// The catalog's rule (the same checks the sigf.ai catalog runs) on a recipe's text, then the parsed recipe. Every
/// download must pass `UrlPolicy` with its sha256 and size; ids, games, steps, destinations and launch steps are held
/// to the same rules as on the server. `allow_local`: dev mode only (local recipes with `file://` URLs).
pub fn check_recipe(text: &str, allow_local: bool) -> Result<Recipe, InstallError> {
    if text.len() > RECIPE_MAX_BYTES {
        return Err(bad("recipe too large"));
    }
    let raw: Value = serde_json::from_str(text).map_err(|e| bad(e.to_string()))?;
    let r = raw.as_object().ok_or_else(|| bad("not an object"))?;
    let id = r.get("id").and_then(Value::as_str).unwrap_or_default();
    let hosted = hosted_repo(id).ok_or_else(|| bad(format!("id {id:?} is not sigf/<repo>")))?;
    if !r.get("version").and_then(Value::as_str).is_some_and(version_ok) {
        return Err(bad("bad version"));
    }
    if !text_ok(r.get("name").unwrap_or(&Value::Null), 120) {
        return Err(bad("bad name"));
    }
    if r.get("tagline").is_some_and(|t| !t.is_string()) {
        return Err(bad("bad tagline"));
    }
    if !r.get("kind").and_then(Value::as_str).is_some_and(|k| KINDS.contains(&k)) {
        return Err(bad("bad kind"));
    }
    let games = r.get("games").and_then(Value::as_array).filter(|g| (1..=4).contains(&g.len())).ok_or_else(|| bad("bad games"))?;
    let mut hosts = 0;
    for g in games {
        let role = g.get("role").and_then(Value::as_str);
        if !game_ok(g.get("game").unwrap_or(&Value::Null)) || !matches!(role, Some("host") | Some("guest")) {
            return Err(bad("bad games"));
        }
        hosts += usize::from(role == Some("host"));
    }
    if hosts != 1 {
        return Err(bad("bad games: exactly one host"));
    }

    let recipe = Recipe::parse(text)?;
    let policy = UrlPolicy::for_recipe(&recipe, allow_local);
    // A download: an allowed location, a sha256, and a size (optional for requires) no larger than MAX_FILE_BYTES.
    let asset = |f: &Value, size_required: bool| -> Result<(), InstallError> {
        let f = f.as_object().ok_or_else(|| bad("bad download"))?;
        let url = f.get("url").and_then(Value::as_str).ok_or_else(|| bad("download without url"))?;
        policy.check(url, false)?;
        if !sha_ok(f.get("sha256").unwrap_or(&Value::Null)) {
            return Err(bad(format!("bad sha256 for {url}")));
        }
        match f.get("size") {
            None if !size_required => Ok(()),
            Some(s) if s.as_u64().is_some_and(|n| n <= MAX_FILE_BYTES) => Ok(()),
            Some(s) if s.as_u64().is_some() => Err(bad(format!("{url} is larger than {} bytes", MAX_FILE_BYTES))),
            _ => Err(bad(format!("bad size for {url}"))),
        }
    };

    let install = r.get("install").and_then(Value::as_array).filter(|i| (1..=6).contains(&i.len())).ok_or_else(|| bad("bad install"))?;
    for e in install {
        let e = e.as_object().ok_or_else(|| bad("bad install"))?;
        let strategy = e.get("strategy").and_then(Value::as_str).unwrap_or_default();
        if !game_ok(e.get("game").unwrap_or(&Value::Null)) || !STRATEGIES.contains(&strategy) {
            return Err(bad("bad install step"));
        }
        if strategy == "mrpack" {
            asset(e.get("pack").unwrap_or(&Value::Null), true)?;
        } else {
            let files = e.get("files").and_then(Value::as_array).filter(|f| !f.is_empty()).ok_or_else(|| bad("install step without files"))?;
            for f in files {
                asset(f, true)?;
                if !f.as_object().is_some_and(placeable) {
                    return Err(bad(format!("bad install file {}", f.get("src").and_then(Value::as_str).unwrap_or("?"))));
                }
            }
        }
        if let Some(j) = e.get("jvm_args") {
            let ok = strategy == "mrpack"
                && j.as_array().is_some_and(|a| a.len() <= mrpack::JVM_ARGS_MAX && a.iter().all(|x| x.as_str().is_some_and(mrpack::jvm_arg_ok)));
            if !ok {
                return Err(bad("bad jvm_args"));
            }
        }
    }
    if let Some(req) = r.get("requires") {
        let a = req.as_array().ok_or_else(|| bad("bad requires"))?;
        for q in a {
            let q = q.as_object().ok_or_else(|| bad("bad requires"))?;
            if !game_ok(q.get("id").unwrap_or(&Value::Null)) || q.get("version").is_some_and(|v| !text_ok(v, 40)) {
                return Err(bad("bad requires"));
            }
            if let Some(s) = q.get("source") {
                asset(s, false)?;
            }
        }
    }
    if let Some(l) = r.get("launch") {
        let a = l.as_array().ok_or_else(|| bad("bad launch"))?;
        for l in a {
            let l = l.as_object().ok_or_else(|| bad("bad launch"))?;
            let args_ok = match l.get("args") {
                None => true,
                Some(a) => a.as_array().is_some_and(|a| a.len() <= 32 && a.iter().all(|x| x.as_str().is_some_and(|s| s.chars().count() <= 300 && placeholders_known(s)))),
            };
            let wait_ok = match l.get("wait") {
                None => true,
                Some(w) => w.as_str().and_then(|w| w.strip_prefix("port:")).is_some_and(|p| !p.is_empty() && p.len() <= 5 && p.bytes().all(|b| b.is_ascii_digit())),
            };
            let exe_good = l.get("exe").is_none_or(|x| x.as_str().is_some_and(exe_ok));
            let app_exe_good = l.get("app_exe").is_none_or(|x| x.as_str().is_some_and(|x| launch_rel_ok(x.strip_prefix("{app}/").unwrap_or(x), ".exe")));
            let me3_good = l.get("me3").is_none_or(|m| {
                me3_ok(m)
                    && l.get("game").and_then(Value::as_str).and_then(crate::launch::me3_game).is_some()
                    && l.get("args").is_none_or(|a| a.as_array().is_some_and(Vec::is_empty))
            });
            let kinds = ["exe", "app_exe", "me3"].iter().filter(|k| l.contains_key(**k)).count();
            if !game_ok(l.get("game").unwrap_or(&Value::Null)) || !args_ok || !wait_ok || !exe_good || !app_exe_good || !me3_good || kinds > 1 {
                return Err(bad("bad launch"));
            }
        }
    }
    let files = r.get("files").and_then(Value::as_array).ok_or_else(|| bad("bad files"))?;
    for f in files {
        asset(f, true)?;
        if !text_ok(f.get("name").unwrap_or(&Value::Null), 200) {
            return Err(bad("bad file name"));
        }
    }
    let steps: Vec<(&str, &str)> =
        install.iter().filter_map(|e| Some((e.get("game")?.as_str()?, e.get("strategy")?.as_str()?))).collect();
    let game_ids: Vec<&str> = games.iter().filter_map(|g| g.get("game").and_then(Value::as_str)).collect();
    if let Some(o) = r.get("own_copies") {
        own_copies_ok(o, &game_ids, &steps).map_err(|m| bad(format!("bad own_copies: {m}")))?;
    }
    if let Some(b) = r.get("player_build") {
        player_build_ok(b, &steps, &policy).map_err(|m| bad(format!("bad player_build: {m}")))?;
    }
    let src = r.get("source").and_then(Value::as_object).ok_or_else(|| bad("bad source"))?;
    let repo_matches = src.get("repo").and_then(Value::as_str).is_some_and(|s| s.trim_end_matches('/') == hosted);
    if !(repo_matches || upstream_fusion(&recipe, &hosted)) || !text_ok(src.get("license").unwrap_or(&Value::Null), 40) {
        return Err(bad("bad source"));
    }
    if r.get("media").is_some_and(|m| !m.is_object()) {
        return Err(bad("bad media"));
    }
    if r.get("platforms").is_some_and(|p| !platforms_ok(p)) {
        return Err(bad("bad platforms"));
    }
    if r.get("conflicts").is_some_and(|c| !conflicts_ok(c, id)) {
        return Err(bad("bad conflicts"));
    }
    if r.get("community").is_some_and(|c| !community_ok(c, &recipe, &hosted)) {
        return Err(bad("bad community"));
    }
    if r.get("requires_files").is_some_and(|f| !requires_files_ok(f, &game_ids)) {
        return Err(bad("bad requires_files"));
    }
    launch_files_installed(&recipe)?;
    Ok(recipe)
}

/// `community: true` (a mashup submitted on sigf.ai/submit): only `true`, only on an upstream fusion, credited to the
/// GitHub owner of `source.repo` (`built_by.author`, case aside). Display only (the card's "Community · by"); same rule
/// as the SIGF catalog (`communityOk`).
fn community_ok(v: &Value, r: &Recipe, hosted: &str) -> bool {
    if v != &Value::Bool(true) || !upstream_fusion(r, hosted) {
        return false;
    }
    let owner = r.source.as_ref().and_then(|s| s.repo.as_deref()).and_then(|u| u.strip_prefix("https://github.com/")).and_then(|p| p.split('/').next());
    let author = r.built_by.as_ref().and_then(|b| b.author.as_deref());
    matches!((owner, author), (Some(o), Some(a)) if o.eq_ignore_ascii_case(a))
}

/// Every file a launch takes from `{app}` or `{game}` (`app_exe`, me3's `profile` and `exe`) is one its game's install
/// step places, with a sha256: the app never starts a program the recipe did not pin.
fn launch_files_installed(r: &Recipe) -> Result<(), InstallError> {
    for l in &r.launch {
        let mut wanted: Vec<String> = vec![];
        if let Some(x) = &l.app_exe {
            wanted.push(format!("{{app}}/{}", x.strip_prefix("{app}/").unwrap_or(x)));
        }
        if let Some(m) = &l.me3 {
            wanted.push(m.profile.clone());
            wanted.extend(m.exe.clone());
        }
        if wanted.is_empty() {
            continue;
        }
        let step = r.install.iter().find(|s| s.game == l.game).ok_or_else(|| bad(format!("launch for {}: no install step", l.game)))?;
        for w in wanted {
            let (root, rest) = paths::split_root(&w).map_err(|_| bad(format!("bad launch file {w}")))?;
            if super::placed_sha(step, root, &rest).is_none() {
                return Err(bad(format!("launch for {}: the {} step does not install {w}", l.game, l.game)));
            }
        }
    }
    Ok(())
}

// ---------- bring your own copy, player builds ----------

/// Most own copies / player builds a recipe may ask for.
pub const OWN_COPIES_MAX: usize = 3;
pub const PLAYER_BUILDS_MAX: usize = 2;
/// Largest own copy (a ROM or a disc file), and the largest one normalized in memory (`format: "n64"`).
pub const OWN_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const OWN_NORMALIZE_MAX_BYTES: u64 = 256 * 1024 * 1024;
/// Largest build script and build input.
pub const BUILD_SCRIPT_MAX_BYTES: u64 = 1024 * 1024;
pub const BUILD_INPUT_MAX_BYTES: u64 = 512 * 1024 * 1024;
pub const BUILD_INPUTS_MAX: usize = 16;
pub const BUILD_OUTPUTS_MAX: usize = 8;
/// Own copy formats: `n64` normalizes byte order (`.v64`, `.n64` -> `.z64`) before hashing.
pub const OWN_FORMATS: &[&str] = &["n64"];

/// A plain file name: `[A-Za-z0-9._+-]{1,100}`, not dot-led, no `..`.
pub fn file_name_ok(s: &str) -> bool {
    upstream_name_ok(s) && s.len() <= 100 && !s.starts_with('.')
}

/// Where an own copy or a build output goes: `{instance}` (the step's Prism instance folder, `mrpack` steps only) or
/// `{app}` (the step's own SIGF folder), then plain relative segments. Both are deleted by Restore.
pub fn byo_to_ok(to: &str, mrpack: bool) -> bool {
    if to.chars().count() > 300 {
        return false;
    }
    let rest = if let Some(r) = to.strip_prefix("{instance}") {
        if !mrpack {
            return false;
        }
        r
    } else if let Some(r) = to.strip_prefix("{app}") {
        r
    } else {
        return false;
    };
    rest.is_empty() || rest.strip_prefix('/').is_some_and(plain_segments)
}

fn str_list(v: Option<&Value>, min: usize, max: usize, each: impl Fn(&str) -> bool) -> Result<Vec<&str>, ()> {
    let a = v.and_then(Value::as_array).filter(|a| a.len() >= min && a.len() <= max).ok_or(())?;
    let out: Vec<&str> = a.iter().filter_map(Value::as_str).filter(|s| each(s)).collect();
    if out.len() != a.len() {
        return Err(());
    }
    Ok(out)
}

/// `own_copies` (docs/RECIPE-FORMAT.md section 4, "Bring your own copy"). Same rule as the catalog's `ownCopiesOk`.
pub fn own_copies_ok(v: &Value, games: &[&str], steps: &[(&str, &str)]) -> Result<(), String> {
    let a = v.as_array().filter(|a| (1..=OWN_COPIES_MAX).contains(&a.len())).ok_or("1 to 3 copies")?;
    let mut seen = vec![];
    for c in a {
        let c = c.as_object().ok_or("not an object")?;
        let game = c.get("game").and_then(Value::as_str).filter(|g| games.contains(g)).ok_or("game must be one of games[]")?;
        if seen.contains(&game) {
            return Err(format!("{game} twice"));
        }
        seen.push(game);
        if !text_ok(c.get("label").unwrap_or(&Value::Null), 80) {
            return Err("label".into());
        }
        let rom = c.get("rom").and_then(Value::as_object).ok_or("rom")?;
        if !rom.get("as").and_then(Value::as_str).is_some_and(file_name_ok) {
            return Err("rom.as".into());
        }
        str_list(rom.get("sha1"), 1, 16, |h| hex_ok(h, 40)).map_err(|_| "rom.sha1: 1 to 16 lowercase SHA-1s")?;
        str_list(rom.get("extensions"), 1, 8, |e| {
            e.len() >= 2 && e.len() <= 9 && e.starts_with('.') && e[1..].bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
        .map_err(|_| "rom.extensions")?;
        if rom.get("size").is_some_and(|s| !s.as_u64().is_some_and(|n| n > 0 && n <= OWN_MAX_BYTES)) {
            return Err("rom.size".into());
        }
        if let Some(f) = rom.get("format") {
            if !f.as_str().is_some_and(|f| OWN_FORMATS.contains(&f)) {
                return Err("rom.format".into());
            }
            if !rom.get("size").and_then(Value::as_u64).is_some_and(|n| n <= OWN_NORMALIZE_MAX_BYTES && n % 4 == 0) {
                return Err("rom.format needs rom.size (a multiple of 4, at most 256 MiB)".into());
            }
        }
        if c.get("names").is_some() {
            str_list(c.get("names"), 0, 8, |n| !n.trim().is_empty() && n.chars().count() <= 40 && !n.contains(['\r', '\n', '\0']))
                .map_err(|_| "names")?;
        }
        let step = c.get("step").and_then(Value::as_str).ok_or("step")?;
        let strategy = steps.iter().find(|(g, _)| *g == step).map(|(_, s)| *s).ok_or("step must be an install step's game")?;
        if !c.get("to").and_then(Value::as_str).is_some_and(|t| byo_to_ok(t, strategy == "mrpack")) {
            return Err("to".into());
        }
    }
    Ok(())
}

/// `player_build` (docs/RECIPE-FORMAT.md section 4, "Player build"). Same rule as the catalog's `playerBuildOk`.
pub fn player_build_ok(v: &Value, steps: &[(&str, &str)], policy: &UrlPolicy) -> Result<(), String> {
    let a = v.as_array().filter(|a| (1..=PLAYER_BUILDS_MAX).contains(&a.len())).ok_or("1 or 2 builds")?;
    let mut ids = vec![];
    for b in a {
        let b = b.as_object().ok_or("not an object")?;
        let id = b.get("id").and_then(Value::as_str).filter(|i| game_ok(&Value::String(i.to_string()))).ok_or("id")?;
        if ids.contains(&id) {
            return Err(format!("{id} twice"));
        }
        ids.push(id);
        if !text_ok(b.get("label").unwrap_or(&Value::Null), 80) {
            return Err("label".into());
        }
        let step = b.get("step").and_then(Value::as_str).ok_or("step")?;
        let strategy = steps.iter().find(|(g, _)| *g == step).map(|(_, s)| *s).ok_or("step must be an install step's game")?;
        let tools = str_list(b.get("toolchain"), 1, 4, |t| super::tools::find(t).is_some()).map_err(|_| "toolchain: ids of the app's pinned tools only")?;
        if !tools.iter().any(|t| super::tools::find(t).is_some_and(|t| t.shell.is_some())) {
            return Err("toolchain needs a shell (w64devkit)".into());
        }
        if tools.iter().enumerate().any(|(i, t)| tools[..i].contains(t)) {
            return Err("toolchain twice".into());
        }
        let file = |f: &Value, script: bool| -> Result<String, String> {
            let f = f.as_object().ok_or("file")?;
            let name = f.get("name").and_then(Value::as_str).filter(|n| file_name_ok(n)).ok_or("file name")?;
            let url = f.get("url").and_then(Value::as_str).ok_or("url")?;
            policy.check_build(url, script).map_err(|e| e.to_string())?;
            if !sha_ok(f.get("sha256").unwrap_or(&Value::Null)) {
                return Err(format!("sha256 of {name}"));
            }
            let max = if script { BUILD_SCRIPT_MAX_BYTES } else { BUILD_INPUT_MAX_BYTES };
            if !f.get("size").and_then(Value::as_u64).is_some_and(|n| n <= max) {
                return Err(format!("size of {name}"));
            }
            let unpack = match f.get("unpack") {
                None => false,
                Some(Value::Bool(u)) if !script => *u,
                Some(_) => return Err(format!("unpack of {name}")),
            };
            if let Some(r) = f.get("root") {
                if !unpack || !r.as_str().is_some_and(|r| r.chars().count() <= 200 && paths::plain_rel(r)) {
                    return Err(format!("root of {name}"));
                }
            }
            Ok(name.to_string())
        };
        let script = file(b.get("script").unwrap_or(&Value::Null), true)?;
        if !script.ends_with(".sh") {
            return Err("script must be a .sh".into());
        }
        let mut names = vec![];
        if let Some(i) = b.get("inputs") {
            let i = i.as_array().filter(|i| i.len() <= BUILD_INPUTS_MAX).ok_or("inputs")?;
            for f in i {
                let n = file(f, false)?;
                if names.contains(&n) {
                    return Err(format!("input {n} twice"));
                }
                names.push(n);
            }
        }
        let outs = b.get("outputs").and_then(Value::as_array).filter(|o| (1..=BUILD_OUTPUTS_MAX).contains(&o.len())).ok_or("outputs")?;
        let mut out_names = vec![];
        for o in outs {
            let o = o.as_object().ok_or("output")?;
            let n = o.get("name").and_then(Value::as_str).filter(|n| file_name_ok(n)).ok_or("output name")?;
            if out_names.contains(&n) {
                return Err(format!("output {n} twice"));
            }
            out_names.push(n);
            if !o.get("to").and_then(Value::as_str).is_some_and(|t| byo_to_ok(t, strategy == "mrpack")) {
                return Err(format!("output {n}: to"));
            }
        }
        if b.get("minutes").is_some_and(|m| !m.as_u64().is_some_and(|m| (1..=60).contains(&m))) {
            return Err("minutes".into());
        }
    }
    Ok(())
}

/// A player build's downloads: (url, sha256, size, is the script).
pub fn planned_build_downloads(r: &Recipe) -> Vec<(String, String, Option<u64>, bool)> {
    let mut out = vec![];
    for b in &r.player_build {
        out.push((b.script.url.clone(), b.script.sha256.clone(), b.script.size, true));
        out.extend(b.inputs.iter().map(|i| (i.url.clone(), i.sha256.clone(), i.size, false)));
    }
    out
}

/// Every download a recipe plans (requires sources, install files, packs) with its declared size.
pub fn planned_downloads(r: &Recipe) -> Vec<(String, String, Option<u64>)> {
    let mut jobs: Vec<(String, String, Option<u64>)> =
        r.requires.iter().filter_map(|q| q.source.as_ref()).map(|s| (s.url.clone(), s.sha256.clone(), s.size)).collect();
    for step in &r.install {
        jobs.extend(step.files.iter().map(|f| (f.location(&r.files), f.sha256.clone(), f.declared_size(&r.files))));
        if let Some(p) = &step.pack {
            jobs.push((p.url.clone(), p.sha256.clone(), p.size));
        }
    }
    jobs
}

/// The `{game}` folders the UI names: each key a game id, each folder one the core's own scan found (`scanned`),
/// compared after resolving both, case-insensitively on Windows. Anything else is refused.
pub fn game_dirs_ok(dirs: &HashMap<String, String>, scanned: &[PathBuf]) -> Result<(), InstallError> {
    let norm = |p: &Path| -> Option<String> {
        let c = p.canonicalize().ok()?;
        let s = paths::path_string(&c);
        Some(if cfg!(windows) { s.to_lowercase() } else { s })
    };
    let known: Vec<String> = scanned.iter().filter_map(|p| norm(p)).collect();
    for (game, dir) in dirs {
        if !game_ok(&Value::String(game.clone())) {
            return Err(bad(format!("bad game id {game:?}")));
        }
        let p = Path::new(dir);
        if !p.is_absolute() || !norm(p).is_some_and(|d| known.contains(&d)) {
            return Err(InstallError::MissingGameDir { game: game.clone() });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SHA: &str = "0000000000000000000000000000000000000000000000000000000000000000";
    const COMMIT: &str = "1111111111111111111111111111111111111111";

    fn hosted(url: &str) -> Value {
        json!({ "id": "sigf/demo", "version": "1.0.0", "name": "Demo", "kind": "mod",
            "games": [{ "game": "doom", "role": "host" }],
            "install": [{ "game": "doom", "strategy": "args", "files": [{ "src": "demo.pk3", "url": url, "sha256": SHA, "size": 10, "dst": "{app}/demo.pk3" }] }],
            "launch": [{ "game": "doom", "args": ["-file", "{app}/demo.pk3"] }],
            "files": [{ "name": "demo.pk3", "url": url, "sha256": SHA, "size": 10 }],
            "source": { "repo": "https://github.com/SIGFAI/demo", "license": "MIT" } })
    }

    fn upstream(url: &str) -> Value {
        json!({ "id": "sigf/demo", "version": "1.0.0", "name": "Demo", "kind": "mashup",
            "games": [{ "game": "skyrim", "role": "host" }, { "game": "minecraft", "role": "guest" }],
            "install": [{ "game": "skyrim", "strategy": "game-dir-snapshot", "files": [{ "src": "Up.zip", "url": url, "sha256": SHA, "size": 10, "dst": "{game}", "unpack": true }] }],
            "files": [{ "name": "Up.zip", "url": url, "sha256": SHA, "size": 10 }],
            "built_by": { "author": "someone" },
            "source": { "repo": "https://github.com/Someone/Up", "hosted": "https://github.com/SIGFAI/demo", "fetch": "upstream", "tag": "v1.2",
                "commit": COMMIT, "license": "MIT" } })
    }

    fn ok(v: &Value) -> Result<Recipe, InstallError> {
        check_recipe(&v.to_string(), false)
    }

    #[test]
    fn community_field() {
        let up = "https://github.com/Someone/Up/releases/download/v1.2/Up.zip";
        let mut v = upstream(up);
        v["community"] = json!(true);
        assert!(ok(&v).is_ok(), "an upstream fusion credited to the repo's owner (case aside)");
        for bad in [json!(false), json!("true"), json!(1), json!(null)] {
            let mut v = upstream(up);
            v["community"] = bad.clone();
            assert!(ok(&v).is_err(), "{bad}");
        }
        let mut v = upstream(up);
        v["community"] = json!(true);
        v["built_by"]["author"] = json!("someone-else");
        assert!(ok(&v).is_err(), "credited to another name than the repo owner");
        let mut v = hosted("https://github.com/SIGFAI/demo/releases/download/v1.0.0/demo.pk3");
        v["community"] = json!(true);
        assert!(ok(&v).is_err(), "not an upstream fusion");
    }

    #[test]
    fn platforms_field() {
        let url = "https://github.com/SIGFAI/demo/releases/download/v1.0.0/demo.pk3";
        for good in [json!(["windows"]), json!(["windows", "macos"]), json!(["macos", "linux"])] {
            let mut v = hosted(url);
            v["platforms"] = good.clone();
            assert!(ok(&v).is_ok(), "{good}");
        }
        for bad in [json!([]), json!("macos"), json!(["windows", "windows"]), json!(["Mac OS"]), json!([1]), json!(["a", "b", "c", "d", "e"])] {
            let mut v = hosted(url);
            v["platforms"] = bad.clone();
            assert!(ok(&v).is_err(), "{bad}");
        }
    }

    #[test]
    fn conflicts_field() {
        let url = "https://github.com/SIGFAI/demo/releases/download/v1.0.0/demo.pk3";
        for good in [json!(["sigf/other"]), json!(["sigf/a", "sigf/b.c", "sigf/d_e"]), json!((0..16).map(|i| format!("sigf/m{i}")).collect::<Vec<_>>())] {
            let mut v = hosted(url);
            v["conflicts"] = good.clone();
            assert_eq!(ok(&v).unwrap().conflicts.len(), good.as_array().unwrap().len(), "{good}");
        }
        for bad in [
            json!([]),
            json!("sigf/a"),
            json!(["sigf/a", "sigf/a"]),
            json!(["sigf/demo"]),
            json!(["a"]),
            json!(["sigf/"]),
            json!(["sigf/a/b"]),
            json!(["SIGF/a"]),
            json!(["evil/a"]),
            json!(["sigf/.hidden"]),
            json!(["sigf/x.git"]),
            json!(["sigf/a b"]),
            json!([1]),
            json!([null]),
            json!((0..17).map(|i| format!("sigf/m{i}")).collect::<Vec<_>>()),
        ] {
            let mut v = hosted(url);
            v["conflicts"] = bad.clone();
            assert!(ok(&v).is_err(), "{bad}");
        }
        assert!(ok(&hosted(url)).unwrap().conflicts.is_empty(), "optional");
    }

    #[test]
    fn hosted_release_assets_pass() {
        ok(&hosted("https://github.com/SIGFAI/demo/releases/download/v1.0.0/demo.pk3")).unwrap();
    }

    #[test]
    fn foreign_and_tricky_urls_fail() {
        for url in [
            "http://github.com/SIGFAI/demo/releases/download/v1.0.0/demo.pk3",
            "https://github.com/SIGFAI/other/releases/download/v1.0.0/demo.pk3",
            "https://github.com/evil/demo/releases/download/v1.0.0/demo.pk3",
            "https://github.com/SIGFAI/demo/archive/refs/heads/main.zip",
            "https://github.com/SIGFAI/demo/releases/download/v1.0.0/../../../evil/x.zip",
            "https://github.com/SIGFAI/demo/releases/download/%2e%2e/%2e%2e/evil/x/y",
            "https://github.com/SIGFAI/demo/releases/download/v1.0.0%2fx/demo.pk3",
            "https://github.com/SIGFAI/demo/releases/download/v1.0.0/sub/demo.pk3",
            "https://github.com/SIGFAI/demo/releases/download/v1.0.0/demo.pk3?x=1",
            "https://github.com/SIGFAI/demo/releases/download/v1.0.0/demo.pk3#x",
            "https://github.com:8443/SIGFAI/demo/releases/download/v1.0.0/demo.pk3",
            "https://user@github.com/SIGFAI/demo/releases/download/v1.0.0/demo.pk3",
            "https://github.com.evil.example/SIGFAI/demo/releases/download/v1.0.0/demo.pk3",
            "https://evil.example/https://github.com/SIGFAI/demo/releases/download/v1.0.0/demo.pk3",
            "https://GITHUB.com/SIGFAI/demo/releases/download/v1.0.0/demo.pk3",
            "https://github.com/SIGFAI/demo/releases/download/v1.0.0/demo.pk3\\..\\x",
            "https://cdn.modrinth.com/data/AANobbMI/versions/x/a.jar",
            "file:///C:/Windows/System32/evil.dll",
            "C:/Users/x/evil.zip",
            "ftp://github.com/SIGFAI/demo/releases/download/v1.0.0/demo.pk3",
        ] {
            assert!(ok(&hosted(url)).is_err(), "{url} should be refused");
        }
    }

    #[test]
    fn upstream_fetch_allows_one_pinned_release() {
        ok(&upstream("https://github.com/Someone/Up/releases/download/v1.2/Up-1.2.zip")).unwrap();
        ok(&upstream("https://github.com/SIGFAI/demo/releases/download/v1.0.0/x.zip")).unwrap();
        for url in [
            "https://github.com/Someone/Up/releases/download/v1.3/Up-1.2.zip",
            "https://github.com/Someone/Other/releases/download/v1.2/Up-1.2.zip",
            "https://github.com/Someone/Up/releases/download/v1.2/sub/Up.zip",
            "https://github.com/Someone/Up/releases/download/v1.2/Up%20x.zip",
            "https://github.com/Someone/Up/releases/download/v1.2/..zip",
        ] {
            assert!(ok(&upstream(url)).is_err(), "{url} should be refused");
        }
        // Without fetch: "upstream" (or without a commit / author) the upstream release is not allowed.
        let url = "https://github.com/Someone/Up/releases/download/v1.2/Up-1.2.zip";
        for strip in ["fetch", "commit"] {
            let mut v = upstream(url);
            v["source"].as_object_mut().unwrap().remove(strip);
            assert!(ok(&v).is_err(), "without {strip}");
        }
        let mut v = upstream(url);
        v.as_object_mut().unwrap().remove("built_by");
        assert!(ok(&v).is_err(), "without an author");
        let mut v = upstream(url);
        v["source"]["hosted"] = json!("https://github.com/SIGFAI/other");
        assert!(ok(&v).is_err(), "hosted repo must be the id's");
    }

    #[test]
    fn mrpack_index_urls() {
        let r = ok(&hosted("https://github.com/SIGFAI/demo/releases/download/v1.0.0/demo.pk3")).unwrap();
        let p = UrlPolicy::for_recipe(&r, false);
        assert!(p.index_url_ok("https://cdn.modrinth.com/data/P7dR8mSH/versions/abc/fabric-api-0.1.jar"));
        assert!(p.index_url_ok("https://cdn.modrinth.com/data/AANobbMI/versions/abc/sodium-fabric-0.5.8%2Bmc1.20.1.jar"));
        assert!(p.index_url_ok("https://cdn.modrinth.com/data/AANobbMI/versions/abc/Some%20Mod%20[Fabric].jar"), "the client encodes the brackets");
        assert!(p.index_url_ok("https://github.com/SIGFAI/demo/releases/download/v1.0.0/mod.jar"));
        assert!(!p.index_url_ok("https://cdn.modrinth.com/../evil.jar"));
        assert!(!p.index_url_ok("https://cdn.modrinth.com/data/%2e%2e/evil.jar"));
        assert!(!p.index_url_ok("http://cdn.modrinth.com/data/a/b.jar"));
        assert!(!p.index_url_ok("https://evil.example/data/a/b.jar"));
        assert!(!p.index_url_ok("https://github.com/evil/x/releases/download/v1/mod.jar"));
        assert!(p.check("file:///C:/x.jar", true).is_err());
        assert!(UrlPolicy::for_recipe(&r, true).check("file:///C:/x.jar", true).is_ok());
    }

    #[test]
    fn local_files_only_in_dev_mode() {
        let v = hosted("file:///C:/mods/demo.pk3");
        assert!(check_recipe(&v.to_string(), false).is_err());
        check_recipe(&v.to_string(), true).unwrap();
        // Dev mode allows local files, not foreign hosts.
        assert!(check_recipe(&hosted("https://evil.example/demo.pk3").to_string(), true).is_err());
    }

    #[test]
    fn sizes_and_hashes() {
        let url = "https://github.com/SIGFAI/demo/releases/download/v1.0.0/demo.pk3";
        let mut v = hosted(url);
        v["files"][0]["size"] = json!(MAX_FILE_BYTES + 1);
        assert!(ok(&v).is_err(), "over the size cap");
        let mut v = hosted(url);
        v["install"][0]["files"][0].as_object_mut().unwrap().remove("size");
        assert!(ok(&v).is_err(), "size required");
        let mut v = hosted(url);
        v["files"][0]["sha256"] = json!("ABC");
        assert!(ok(&v).is_err(), "bad hash");
    }

    #[test]
    fn structure_rules() {
        let url = "https://github.com/SIGFAI/demo/releases/download/v1.0.0/demo.pk3";
        let cases: Vec<(&str, Box<dyn Fn(&mut Value)>)> = vec![
            ("id", Box::new(|v| v["id"] = json!("evil/demo"))),
            ("version", Box::new(|v| v["version"] = json!("1"))),
            ("kind", Box::new(|v| v["kind"] = json!("virus"))),
            ("two hosts", Box::new(|v| v["games"] = json!([{ "game": "doom", "role": "host" }, { "game": "tf2", "role": "host" }]))),
            ("dst escape", Box::new(|v| v["install"][0]["files"][0]["dst"] = json!("{game}/../../x.dll"))),
            ("dst absolute", Box::new(|v| v["install"][0]["files"][0]["dst"] = json!("C:/Windows/x.dll"))),
            ("dst ads", Box::new(|v| v["install"][0]["files"][0]["dst"] = json!("{game}/a.dll:evil"))),
            ("unknown placeholder", Box::new(|v| v["launch"][0]["args"] = json!(["{home}/x"]))),
            ("launch exe", Box::new(|v| v["launch"][0]["exe"] = json!("{game}/../cmd.exe"))),
            ("launch exe kind", Box::new(|v| v["launch"][0]["exe"] = json!("run.bat"))),
            ("wait", Box::new(|v| v["launch"][0]["wait"] = json!("http://x"))),
            ("jvm", Box::new(|v| v["install"][0]["jvm_args"] = json!(["-Xmx4G"]))),
            ("source", Box::new(|v| v["source"]["repo"] = json!("https://github.com/SIGFAI/other"))),
            ("strategy", Box::new(|v| v["install"][0]["strategy"] = json!("exec"))),
            ("requires", Box::new(|v| v["requires"] = json!([{ "id": "x", "source": { "url": "https://evil.example/x.dll", "sha256": SHA } }]))),
        ];
        for (name, f) in cases {
            let mut v = hosted(url);
            f(&mut v);
            assert!(ok(&v).is_err(), "{name} should be refused");
        }
        let mut v = hosted(url);
        v["launch"][0]["exe"] = json!("{game}/skse64_loader.exe");
        v["requires"] = json!([{ "id": "skse64", "page": "https://skse.silverlock.org/" }]);
        ok(&v).unwrap();
    }

    const SHA1: &str = "9bef1128717f958171a4afac3ed78ee2bb4e86ce";

    /// A Minecraft recipe with an own copy and a player build, as the Mario 64 in Minecraft recipe has them.
    fn byo() -> Value {
        let rel = "https://github.com/SIGFAI/demo/releases/download/v1.0.0/";
        json!({ "id": "sigf/demo", "version": "1.0.0", "name": "Demo", "kind": "mashup",
            "games": [{ "game": "minecraft", "role": "host" }, { "game": "sm64", "role": "guest" }],
            "install": [{ "game": "minecraft", "strategy": "mrpack", "pack": { "url": format!("{rel}demo.mrpack"), "sha256": SHA, "size": 10 } }],
            "files": [{ "name": "demo.mrpack", "url": format!("{rel}demo.mrpack"), "sha256": SHA, "size": 10 }],
            "own_copies": [{ "game": "sm64", "label": "Super Mario 64 (USA)", "names": ["mario 64"], "step": "minecraft",
                "rom": { "as": "baserom.us.z64", "sha1": [SHA1], "extensions": [".z64", ".v64", ".n64"], "size": 8388608, "format": "n64" },
                "to": "{instance}/.minecraft/config/mario64" }],
            "player_build": [{ "id": "sm64-dll", "label": "Mario library", "step": "minecraft", "toolchain": ["w64devkit-2.10.0", "python-3.12.10"],
                "script": { "name": "build.sh", "url": format!("{rel}build.sh"), "sha256": SHA, "size": 100 },
                "inputs": [
                    { "name": "libsm64", "url": format!("https://github.com/libsm64/libsm64/archive/{COMMIT}.zip"), "sha256": SHA, "size": 1000, "unpack": true, "root": format!("libsm64-{COMMIT}") },
                    { "name": "geo.inc.c", "url": format!("https://raw.githubusercontent.com/n64decomp/sm64/{COMMIT}/actors/mario/geo.inc.c"), "sha256": SHA, "size": 1000 }
                ],
                "outputs": [{ "name": "sm64.dll", "to": "{instance}/.minecraft/config/mario64" }], "minutes": 5 }],
            "source": { "repo": "https://github.com/SIGFAI/demo", "license": "MIT" } })
    }

    #[test]
    fn own_copies_and_player_build_pass() {
        let r = ok(&byo()).unwrap();
        assert_eq!(r.own_copies[0].rom.save_as, "baserom.us.z64");
        assert_eq!(planned_build_downloads(&r).len(), 3);
        // Never among the downloads the app plans for the mod itself.
        assert!(planned_downloads(&r).iter().all(|(u, _, _)| !u.contains("libsm64") && !u.contains("build.sh")));
    }

    #[test]
    fn own_copies_and_player_build_are_strict() {
        let cases: Vec<(&str, Box<dyn Fn(&mut Value)>)> = vec![
            ("copy game not in games", Box::new(|v| v["own_copies"][0]["game"] = json!("zelda"))),
            ("copy sha1 upper", Box::new(|v| v["own_copies"][0]["rom"]["sha1"] = json!([SHA1.to_uppercase()]))),
            ("copy sha1 sha256", Box::new(|v| v["own_copies"][0]["rom"]["sha1"] = json!([SHA]))),
            ("copy sha1 empty", Box::new(|v| v["own_copies"][0]["rom"]["sha1"] = json!([]))),
            ("copy as path", Box::new(|v| v["own_copies"][0]["rom"]["as"] = json!("../evil.dll"))),
            ("copy as sub", Box::new(|v| v["own_copies"][0]["rom"]["as"] = json!("a/b.z64"))),
            ("copy ext", Box::new(|v| v["own_copies"][0]["rom"]["extensions"] = json!(["z64"]))),
            ("copy format", Box::new(|v| v["own_copies"][0]["rom"]["format"] = json!("psx"))),
            ("copy format no size", Box::new(|v| {
                v["own_copies"][0]["rom"].as_object_mut().unwrap().remove("size");
            })),
            ("copy to game", Box::new(|v| v["own_copies"][0]["to"] = json!("{game}/x"))),
            ("copy to escape", Box::new(|v| v["own_copies"][0]["to"] = json!("{instance}/../x"))),
            ("copy to abs", Box::new(|v| v["own_copies"][0]["to"] = json!("C:/Windows"))),
            ("copy step", Box::new(|v| v["own_copies"][0]["step"] = json!("sm64"))),
            ("copy label", Box::new(|v| {
                v["own_copies"][0].as_object_mut().unwrap().remove("label");
            })),
            ("copies too many", Box::new(|v| {
                let c = v["own_copies"][0].clone();
                v["own_copies"] = json!([c.clone(), c.clone(), c.clone(), c]);
            })),
            ("copy twice", Box::new(|v| {
                let c = v["own_copies"][0].clone();
                v["own_copies"] = json!([c.clone(), c]);
            })),
            ("tool unknown", Box::new(|v| v["player_build"][0]["toolchain"] = json!(["w64devkit-2.10.0", "msys2"]))),
            ("tool no shell", Box::new(|v| v["player_build"][0]["toolchain"] = json!(["python-3.12.10"]))),
            ("tool twice", Box::new(|v| v["player_build"][0]["toolchain"] = json!(["w64devkit-2.10.0", "w64devkit-2.10.0"]))),
            ("script upstream", Box::new(|v| v["player_build"][0]["script"]["url"] = json!(format!("https://raw.githubusercontent.com/x/y/{COMMIT}/build.sh")))),
            ("script other repo", Box::new(|v| v["player_build"][0]["script"]["url"] = json!("https://github.com/SIGFAI/other/releases/download/v1/build.sh"))),
            ("script not sh", Box::new(|v| v["player_build"][0]["script"]["name"] = json!("build.ps1"))),
            ("script no size", Box::new(|v| {
                v["player_build"][0]["script"].as_object_mut().unwrap().remove("size");
            })),
            ("script big", Box::new(|v| v["player_build"][0]["script"]["size"] = json!(BUILD_SCRIPT_MAX_BYTES + 1))),
            ("input branch", Box::new(|v| v["player_build"][0]["inputs"][1]["url"] = json!("https://raw.githubusercontent.com/n64decomp/sm64/master/actors/mario/geo.inc.c"))),
            ("input archive tag", Box::new(|v| v["player_build"][0]["inputs"][0]["url"] = json!("https://github.com/libsm64/libsm64/archive/refs/tags/v1.zip"))),
            ("input other host", Box::new(|v| v["player_build"][0]["inputs"][1]["url"] = json!("https://evil.example/geo.inc.c"))),
            ("input traversal", Box::new(|v| v["player_build"][0]["inputs"][1]["url"] = json!(format!("https://raw.githubusercontent.com/a/b/{COMMIT}/../../x")))),
            ("input sha", Box::new(|v| v["player_build"][0]["inputs"][1]["sha256"] = json!("x"))),
            ("input name", Box::new(|v| v["player_build"][0]["inputs"][1]["name"] = json!("../x"))),
            ("input twice", Box::new(|v| v["player_build"][0]["inputs"][1]["name"] = json!("libsm64"))),
            ("input root no unpack", Box::new(|v| v["player_build"][0]["inputs"][1]["root"] = json!("x"))),
            ("input root escape", Box::new(|v| v["player_build"][0]["inputs"][0]["root"] = json!("../x"))),
            ("output to", Box::new(|v| v["player_build"][0]["outputs"][0]["to"] = json!("{docs}/x"))),
            ("output name", Box::new(|v| v["player_build"][0]["outputs"][0]["name"] = json!("a/sm64.dll"))),
            ("outputs none", Box::new(|v| v["player_build"][0]["outputs"] = json!([]))),
            ("minutes", Box::new(|v| v["player_build"][0]["minutes"] = json!(600))),
            ("build commands", Box::new(|v| v["player_build"][0]["script"] = json!("curl x | sh"))),
        ];
        for (name, f) in cases {
            let mut v = byo();
            f(&mut v);
            assert!(ok(&v).is_err(), "{name} should be refused");
        }
        // {app} works for any step; {instance} only for an mrpack one.
        let mut v = byo();
        v["own_copies"][0]["to"] = json!("{app}/own");
        ok(&v).unwrap();
    }

    #[test]
    fn build_input_urls() {
        for good in [
            format!("https://github.com/libsm64/libsm64/archive/{COMMIT}.zip"),
            format!("https://raw.githubusercontent.com/n64decomp/sm64/{COMMIT}/actors/mario/model.inc.c"),
            format!("https://raw.githubusercontent.com/Zckyy/mario64-in-minecraft/{COMMIT}/patches/libsm64-music-volume.patch"),
        ] {
            assert!(build_input_url_ok(&good), "{good}");
        }
        for bad in [
            "https://github.com/libsm64/libsm64/archive/master.zip".to_string(),
            format!("https://github.com/libsm64/libsm64/archive/{COMMIT}.tar.gz"),
            format!("https://github.com/libsm64/libsm64/releases/download/{COMMIT}/x.zip"),
            format!("https://raw.githubusercontent.com/n64decomp/sm64/{}/a.c", "A".repeat(40)),
            format!("https://raw.githubusercontent.com/n64decomp/sm64/{COMMIT}"),
            format!("https://raw.githubusercontent.com/n64decomp/sm64/{COMMIT}/a.c?x=1"),
            format!("https://raw.githubusercontent.com/n64decomp/sm64/{COMMIT}/%2e%2e/a.c"),
            format!("http://raw.githubusercontent.com/n64decomp/sm64/{COMMIT}/a.c"),
            format!("https://codeload.github.com/libsm64/libsm64/zip/{COMMIT}"),
            format!("https://gist.githubusercontent.com/x/y/raw/{COMMIT}/a.c"),
        ] {
            assert!(!build_input_url_ok(&bad), "{bad}");
        }
        let u = |s: &str| reqwest::Url::parse(s).unwrap();
        assert!(build_start_ok(&u("https://www.python.org/ftp/python/3.12.10/python-3.12.10-embed-amd64.zip")));
        assert!(!download_start_ok(&u("https://www.python.org/ftp/python/3.12.10/python-3.12.10-embed-amd64.zip")), "mods never start on python.org");
        assert!(build_redirect_ok(&u("https://codeload.github.com/a/b/zip/x")) && !redirect_ok(&u("https://codeload.github.com/a/b/zip/x")));
    }

    #[test]
    fn placeholders() {
        assert!(placeholders_known("-file {app}/a.pk3"));
        assert!(placeholders_known("+exec {game}/cfg {docs}"));
        assert!(!placeholders_known("{home}/x"));
        assert!(placeholders_known("{not a placeholder"));
    }

    #[test]
    fn redirect_hosts() {
        let u = |s: &str| reqwest::Url::parse(s).unwrap();
        assert!(redirect_ok(&u("https://release-assets.githubusercontent.com/github-production-release-asset/1/2?sp=r")));
        assert!(redirect_ok(&u("https://objects.githubusercontent.com/x")));
        assert!(redirect_ok(&u("https://cdn.modrinth.com/data/x")));
        assert!(!redirect_ok(&u("http://objects.githubusercontent.com/x")));
        assert!(!redirect_ok(&u("https://evil.example/x")));
        assert!(!redirect_ok(&u("https://objects.githubusercontent.com.evil.example/x")));
        assert!(!redirect_ok(&u("https://objects.githubusercontent.com:444/x")));
        assert!(download_start_ok(&u("https://github.com/SIGFAI/x/releases/download/v1/a")));
        assert!(!download_start_ok(&u("https://raw.githubusercontent.com/SIGFAI/x/main/a")));
    }

    #[test]
    fn game_dirs_must_be_scanned() {
        let t = tempfile::tempdir().unwrap();
        let game = t.path().join("Steam/steamapps/common/Doom");
        let other = t.path().join("Users/me/Desktop");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        let scanned = vec![game.clone()];
        let dirs = |d: &Path| HashMap::from([("doom".to_string(), d.to_string_lossy().into_owned())]);
        game_dirs_ok(&dirs(&game), &scanned).unwrap();
        assert!(matches!(game_dirs_ok(&dirs(&other), &scanned), Err(InstallError::MissingGameDir { .. })));
        assert!(game_dirs_ok(&dirs(&game.join("..").join("Doom")), &scanned).is_ok(), "same folder, other spelling");
        assert!(game_dirs_ok(&dirs(&game.join("sub")), &scanned).is_err());
        assert!(game_dirs_ok(&dirs(Path::new("relative/dir")), &scanned).is_err());
        let bad_key = HashMap::from([("../x".to_string(), game.to_string_lossy().into_owned())]);
        assert!(game_dirs_ok(&bad_key, &scanned).is_err());
    }
}
