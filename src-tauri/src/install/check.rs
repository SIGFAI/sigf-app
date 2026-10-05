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

/// A GitHub repo name: `[A-Za-z0-9._-]{1,100}`, not `.`-led, not `.git`.
fn repo_name_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= 100 && !s.starts_with('.') && !s.ends_with(".git") && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

/// `https://github.com/<owner>/<repo>` (the server's `REPO_RE`).
fn repo_url_ok(s: &str) -> bool {
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
            if !game_ok(l.get("game").unwrap_or(&Value::Null)) || !args_ok || !wait_ok || !exe_good {
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
    let src = r.get("source").and_then(Value::as_object).ok_or_else(|| bad("bad source"))?;
    let repo_matches = src.get("repo").and_then(Value::as_str).is_some_and(|s| s.trim_end_matches('/') == hosted);
    if !(repo_matches || upstream_fusion(&recipe, &hosted)) || !text_ok(src.get("license").unwrap_or(&Value::Null), 40) {
        return Err(bad("bad source"));
    }
    if r.get("media").is_some_and(|m| !m.is_object()) {
        return Err(bad("bad media"));
    }
    Ok(recipe)
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
