//! "Report a bug": a pre-filled GitHub issue the player reviews, then opens in their own browser. No telemetry: this
//! module only writes text on the PC. The app sends nothing; the browser opens `github.com/<repo>/issues/new` only when
//! the player clicks, and the player submits (or not) on GitHub. docs/PRIVACY.md, "Report a bug".
//!
//! - Where it goes ([`targets`]): the player picks the kind of problem.
//!   - "Bug in the mod itself" ([`mod_tracker`]): the card's `links.issues` (the sigf.ai catalog resolved it from the
//!     recipe's `issues`: the upstream author's tracker or the SIGFAI copy's own; absent for `issues: false`, and then
//!     this choice is not offered). A card without `links` (an older catalog): its repo's tracker.
//!   - "Install / app problem" ([`sigfai_copy`]): the SIGFAI copy's tracker, the repo the app installs from. The card's
//!     `repo` is that copy only for SIGF's own mashups: for an upstream fusion it credits the author's repo, so the copy
//!     comes from the mashup id (`sigf/<name>` is the SIGFAI repo `<name>`, library/publish.mjs `hosted`).
//!   - Preselected ([`preselect`]): "Install / app problem" when the last error was an install, player build or restore
//!     failure, else "Bug in the mod itself". No target at all: the UI offers "Copy report" only.
//!   App bugs go to [`APP_ISSUES`].
//! - What it holds: the app version, the system, the mashup and its install state, the games found, the last error the
//!   app showed and the last [`LOG_LINES`] lines of the mashup's kept build log, all through [`scrub`] (home folder ->
//!   `~`, user name, token-like strings).
//! - The link stays under [`URL_MAX`] bytes ([`issue_url`]): older log lines go first, then the body is cut.

use crate::install::{self, check, paths, InstalledMod};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// The SIGF app's own public tracker (library/export-sigf-app.mjs publishes the app to SIGFAI/sigf-app).
pub const APP_ISSUES: &str = "https://github.com/SIGFAI/sigf-app/issues";
/// How many log lines a report carries at most.
pub const LOG_LINES: usize = 40;
/// GitHub answers 414 past about 8 KB of URL: stay under it.
pub const URL_MAX: usize = 7800;

/// What the UI knows and the core does not: the catalog card and the last scan.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportInput {
    /// None: a report about the app itself.
    pub mashup: Option<MashupInput>,
    /// The games of the mashup (or every store found, for an app report), as the last scan saw them.
    #[serde(default)]
    pub games: Vec<GameInput>,
    /// An install in progress, as the UI shows it (`download 40%`). The core reads the installed state itself.
    pub progress: Option<String>,
    /// The last error the app showed for this mashup (or for the app), as the player read it.
    pub last_error: Option<String>,
    /// What failed last: `install` (download, check, player build), `restore`, `play`.
    pub last_error_kind: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MashupInput {
    pub id: String,
    pub name: String,
    /// The catalog's current version.
    pub version: Option<String>,
    /// The card's `links`, when the catalog sent them.
    pub links: Option<CardLinks>,
    /// The card's `repo`.
    pub repo: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardLinks {
    pub issues: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameInput {
    /// Canonical id (`gta5`) or a store name for an app report.
    pub id: String,
    pub name: String,
    /// `steam`, `epic`...; None: not found on this PC.
    pub store: Option<String>,
    pub build: Option<String>,
    /// Content-only side (its look comes inside the mod): not needed on the PC.
    #[serde(default)]
    pub optional: bool,
}

/// The kinds of problem a report can be about (the player picks one in the sheet).
pub const KIND_MOD: &str = "mod";
pub const KIND_INSTALL: &str = "install";
pub const KIND_APP: &str = "app";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub title: String,
    /// The whole report, for "Copy report" (and the preview when there is no target).
    pub full: String,
    /// Where it can go, one per kind of problem (`mod`, `install`; `app` for the app). Empty: copy only.
    pub targets: Vec<Target>,
    /// The kind the sheet selects first.
    pub preselect: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub kind: String,
    /// `owner/repo` of the tracker.
    pub tracker: String,
    /// `https://github.com/<owner>/<repo>/issues/new?...`
    pub url: String,
    /// What the link carries (maybe shortened), shown in the preview.
    pub body: String,
    /// The link carries a shortened body.
    pub truncated: bool,
}

/// (owner, name) of `https://github.com/<owner>/<name>` (the recipe check's `repo_url_ok`), else None.
fn owner_name(url: &str) -> Option<(&str, &str)> {
    check::repo_url_ok(url).then(|| url.strip_prefix("https://github.com/")?.split_once('/')).flatten()
}

/// `owner/repo` of a GitHub repo URL whose name is a repo name (`check::repo_name_ok`), else None.
fn repo_of(url: &str) -> Option<String> {
    owner_name(url).filter(|(_, name)| check::repo_name_ok(name)).map(|(owner, name)| format!("{owner}/{name}"))
}

/// `owner/repo` of a GitHub issues URL we may open, else None.
fn issues_repo(url: &str) -> Option<String> {
    let url = url.trim();
    repo_of(url.strip_suffix("/issues/").or_else(|| url.strip_suffix("/issues"))?)
}

/// Where bugs in the mod itself go (`owner/repo`): the card's issues link; no `links` at all (older catalog): the card's
/// repo. `links.issues: null` is a recipe with `issues: false` (the author takes no reports): None.
pub fn mod_tracker(m: &MashupInput) -> Option<String> {
    match &m.links {
        Some(l) => l.issues.as_deref().and_then(issues_repo),
        None => repo_of(m.repo.as_deref()?.trim().trim_end_matches('/')),
    }
}

/// Where install and app problems go (`owner/repo`): the SIGFAI copy the app installs from. The card's repo when it is
/// a SIGFAI repo (SIGF's own mashups), else `SIGFAI/<name>` from the id `sigf/<name>` (an upstream fusion's card credits
/// the author's repo; its copy is the SIGFAI repo named like the id).
pub fn sigfai_copy(m: &MashupInput) -> Option<String> {
    if let Some(repo) = m.repo.as_deref().map(|r| r.trim().trim_end_matches('/')) {
        if owner_name(repo).is_some_and(|(owner, _)| owner.eq_ignore_ascii_case(check::OWNER)) {
            return repo_of(repo);
        }
    }
    repo_of(&check::hosted_repo(&m.id)?)
}

/// Where a report can go, one per kind: the mod's tracker and the SIGFAI copy's (a single `install` target when they
/// are the same repo); the app's tracker for an app report.
pub fn targets(m: Option<&MashupInput>) -> Vec<(&'static str, String)> {
    let Some(m) = m else { return issues_repo(APP_ISSUES).map(|t| vec![(KIND_APP, t)]).unwrap_or_default() };
    let (bug, copy) = (mod_tracker(m), sigfai_copy(m));
    let mut out = vec![];
    if let Some(b) = &bug {
        if copy.as_ref().is_none_or(|c| !c.eq_ignore_ascii_case(b)) {
            out.push((KIND_MOD, b.clone()));
        }
    }
    if let Some(c) = copy {
        out.push((KIND_INSTALL, c));
    }
    out
}

/// The kind the sheet selects first: an install, player build or restore failure is an install problem, anything else
/// a bug in the mod; the first target when that kind is not offered.
pub fn preselect(input: &ReportInput, kinds: &[&str]) -> Option<String> {
    let want = match input.last_error_kind.as_deref() {
        Some("install" | "restore") => KIND_INSTALL,
        _ => KIND_MOD,
    };
    kinds.iter().find(|k| **k == want).or(kinds.first()).map(|k| k.to_string())
}

// ---------- scrubbing ----------

static HOME_WIN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)\b[a-z]:[\\/]+(?:users|documents and settings)[\\/]+[^\\/\s:*?<>|"']+"#).unwrap());
static HOME_UNIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:/Users|/home)/[^/\s:'\x22]+").unwrap());
/// `token=...`, `"password": "..."`, `Authorization: Bearer ...`: the value goes.
static SECRET_KV: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\b((?:access_|refresh_|id_|auth_|api_|session_|private_)?(?:token|secret|password|passwd|pwd|api_?key|apikey|auth|authorization|cookie|sig|signature|credential)s?)(["']?\s*[:=]\s*["']?)(?:bearer\s+|basic\s+)?[^\s"'&,;]+"#).unwrap()
});
static BEARER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(bearer|basic)\s+[A-Za-z0-9._~+/=-]{8,}").unwrap());
/// Well-known token shapes (GitHub, Slack, OpenAI/Anthropic, AWS, Google, JWT) and URL credentials.
static KNOWN_TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|xox[abpors]-[A-Za-z0-9-]{10,}|sk-[A-Za-z0-9_-]{16,}|AKIA[0-9A-Z]{16}|AIza[0-9A-Za-z_-]{30,}|eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,})").unwrap()
});
static URL_CRED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b([a-z][a-z0-9+.-]*://)[^/\s:@]+:[^/\s@]+@").unwrap());
/// Long random-looking words (mixed case and digits): API keys, session ids. Plain hex (hashes, commits) stays.
static LONG_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Za-z0-9_+/=-]{32,}").unwrap());

fn random_looking(w: &str) -> bool {
    let lower = w.bytes().any(|b| b.is_ascii_lowercase());
    let upper = w.bytes().any(|b| b.is_ascii_uppercase());
    let digit = w.bytes().any(|b| b.is_ascii_digit());
    // Paths and names (`a/b/c`, `some-long-file-name`) have separators every few chars; keys do not.
    let longest_run = w.split(|c: char| matches!(c, '/' | '-' | '_' | '.')).map(str::len).max().unwrap_or(0);
    lower && upper && digit && longest_run >= 24
}

/// Removes what identifies the player or could unlock something: the home folder (the real one, and any
/// `<drive>:\Users\<name>` / `/Users/<name>` / `/home/<name>`) becomes `~`, the user name `<user>`, secrets `[redacted]`.
/// The player's own patterns are compiled once, for every text of a report.
pub struct Scrubber {
    /// The home folder as written with `\`, `/` and doubled `\\`, in that order.
    home: Vec<Regex>,
    user: Option<Regex>,
}

impl Scrubber {
    pub fn new(home: Option<&Path>, user: Option<&str>) -> Self {
        let home = match home.map(|h| h.to_string_lossy().trim_end_matches(['\\', '/']).to_string()).filter(|h| h.len() > 3) {
            Some(h) => [h.clone(), h.replace('\\', "/"), h.replace('/', "\\"), h.replace('\\', "\\\\")]
                .iter()
                .map(|form| Regex::new(&format!("(?i){}", regex::escape(form))).unwrap())
                .collect(),
            None => vec![],
        };
        let user = user
            .map(str::trim)
            .filter(|u| u.chars().count() >= 3 && !matches!(u.to_ascii_lowercase().as_str(), "user" | "admin" | "player" | "public"))
            .map(|u| Regex::new(&format!(r"(?i)(^|[^A-Za-z0-9]){}($|[^A-Za-z0-9])", regex::escape(u))).unwrap());
        Self { home, user }
    }

    pub fn scrub(&self, text: &str) -> String {
        let mut s = text.to_string();
        for re in &self.home {
            s = re.replace_all(&s, "~").into_owned();
        }
        s = HOME_WIN.replace_all(&s, "~").into_owned();
        s = HOME_UNIX.replace_all(&s, "~").into_owned();
        s = URL_CRED.replace_all(&s, "${1}[redacted]@").into_owned();
        s = SECRET_KV.replace_all(&s, "${1}${2}[redacted]").into_owned();
        s = BEARER.replace_all(&s, "${1} [redacted]").into_owned();
        s = KNOWN_TOKEN.replace_all(&s, "[redacted]").into_owned();
        s = LONG_WORD.replace_all(&s, |c: &regex::Captures| if random_looking(&c[0]) { "[redacted]".to_string() } else { c[0].to_string() }).into_owned();
        if let Some(re) = &self.user {
            // Twice: adjacent matches share a separator.
            for _ in 0..2 {
                s = re.replace_all(&s, "${1}<user>${2}").into_owned();
            }
        }
        s
    }
}

/// One text through a [`Scrubber`] of its own.
pub fn scrub(text: &str, home: Option<&Path>, user: Option<&str>) -> String {
    Scrubber::new(home, user).scrub(text)
}

/// The player's home folder and user name, for [`scrub`].
fn identity() -> (Option<PathBuf>, Option<String>) {
    let user = ["USERNAME", "USER", "LOGNAME"].iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()));
    let home = install::user_home();
    let user = user.or_else(|| home.as_ref().and_then(|h| h.file_name().map(|n| n.to_string_lossy().into_owned())));
    (home, user)
}

// ---------- logs ----------

/// The last `n` non-empty lines of a text, without control characters (ANSI colors), each at most 300 chars.
pub fn tail_lines(text: &str, n: usize) -> Vec<String> {
    static ANSI: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\x1b\[[0-9;?]*[A-Za-z]").unwrap());
    let clean = ANSI.replace_all(text, "");
    let lines: Vec<String> = clean
        .lines()
        .map(|l| l.chars().filter(|c| !c.is_control() || *c == '\t').collect::<String>().trim_end().to_string())
        .filter(|l| !l.trim().is_empty())
        .map(|l| if l.chars().count() > 300 { format!("{}…", l.chars().take(299).collect::<String>()) } else { l })
        .collect();
    lines[lines.len().saturating_sub(n)..].to_vec()
}

/// The newest build log the engine kept for this mashup (`<home>/logs/<slug>-<build>-build.log`, kept when a player
/// build fails), as (file name, its last lines).
pub fn mashup_log(home: &Path, id: &str) -> Option<(String, Vec<String>)> {
    let prefix = format!("{}-", paths::slug(id));
    let newest = std::fs::read_dir(home.join("logs"))
        .ok()?
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.starts_with(&prefix) && n.ends_with("-build.log")
        })
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .max_by_key(|(t, _)| *t)?
        .1;
    let bytes = std::fs::read(&newest).ok()?;
    // Only the end matters: a runaway build log can be large.
    let start = bytes.len().saturating_sub(256 * 1024);
    let lines = tail_lines(&String::from_utf8_lossy(&bytes[start..]), LOG_LINES);
    Some((newest.file_name()?.to_string_lossy().into_owned(), lines))
}

// ---------- the system ----------

/// "Windows 11 24H2 (build 26100.4061)", "macOS 15.5": what players read in Settings / About This Mac.
pub fn os_version() -> String {
    #[cfg(windows)]
    {
        use winreg::{enums::HKEY_LOCAL_MACHINE, RegKey};
        if let Ok(k) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion") {
            let build: String = k.get_value("CurrentBuildNumber").or_else(|_| k.get_value("CurrentBuild")).unwrap_or_default();
            let ubr: Option<u32> = k.get_value("UBR").ok();
            let display: String = k.get_value("DisplayVersion").or_else(|_| k.get_value("ReleaseId")).unwrap_or_default();
            let edition: String = k.get_value("EditionID").unwrap_or_default();
            return windows_name(&build, ubr, &display, &edition);
        }
        "Windows".into()
    }
    #[cfg(target_os = "macos")]
    {
        let v = std::process::Command::new("/usr/bin/sw_vers").arg("-productVersion").output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
        format!("macOS {}", v.filter(|v| !v.is_empty()).unwrap_or_else(|| "(unknown version)".into()))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        std::env::consts::OS.to_string()
    }
}

/// Windows 11 still says "Windows 10" in ProductName: the build number tells them apart (22000 and up is 11).
pub fn windows_name(build: &str, ubr: Option<u32>, display: &str, edition: &str) -> String {
    let n: u32 = build.parse().unwrap_or(0);
    let major = if n >= 22000 { "11" } else { "10" };
    let mut s = format!("Windows {major}");
    // EditionID as players read it in Settings.
    let edition = match edition {
        "Core" | "CoreN" => "Home",
        "CoreSingleLanguage" => "Home Single Language",
        "Professional" | "ProfessionalN" => "Pro",
        "ProfessionalWorkstation" => "Pro for Workstations",
        "ProfessionalEducation" => "Pro Education",
        e => e,
    };
    if !edition.is_empty() {
        s.push_str(&format!(" {edition}"));
    }
    if !display.is_empty() {
        s.push_str(&format!(" {display}"));
    }
    if !build.is_empty() {
        s.push_str(&format!(" (build {build}{})", ubr.map(|u| format!(".{u}")).unwrap_or_default()));
    }
    s
}

// ---------- the report ----------

/// Everything the core adds to the UI's input.
pub struct Context {
    pub app_version: String,
    pub os: String,
    pub installed: Vec<InstalledMod>,
    /// The kept log: (file name, last lines).
    pub log: Option<(String, Vec<String>)>,
}

fn fmt_date(secs: u64) -> String {
    // Civil date from Unix seconds (Howard Hinnant's days_from_civil, inverted).
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

const TEMPLATE: &str = "### What happened\n\n\n\n### What you expected\n\n\n\n### Steps to reproduce\n\n1. \n2. \n3. \n";

/// Keeps a log from closing the Markdown fence it sits in.
fn fence_safe(line: &str) -> String {
    line.replace("~~~", "~ ~ ~").replace("```", "'''")
}

/// Title and body (without the log), unscrubbed.
fn parts(input: &ReportInput, cx: &Context) -> (String, String) {
    let mut d = vec![format!("- SIGF app: {}", cx.app_version), format!("- System: {}", cx.os)];
    let title;
    match &input.mashup {
        Some(m) => {
            let v = m.version.as_deref().unwrap_or("unknown");
            title = format!("{} {v}: ", m.name);
            d.push(format!("- Mashup: {} ({}), catalog version {v}", m.name, m.id));
            let state = match cx.installed.iter().find(|x| x.id == m.id) {
                Some(x) => {
                    let how = x.games.iter().map(|g| format!("{} {}", g.game, serde_json::to_value(g.strategy).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default())).collect::<Vec<_>>().join(", ");
                    format!("installed, version {} on {} ({how})", x.version, fmt_date(x.installed_at))
                }
                None => match &input.progress {
                    Some(p) => format!("installing: {p}"),
                    None => "not installed".into(),
                },
            };
            d.push(format!("- Install: {state}"));
        }
        None => {
            title = format!("SIGF app {}: ", cx.app_version);
            let list = cx.installed.iter().map(|x| format!("{} {}", x.id, x.version)).collect::<Vec<_>>();
            d.push(format!("- Installed mashups: {}", if list.is_empty() { "none".into() } else { list.join(", ") }));
        }
    }
    if !input.games.is_empty() {
        d.push(if input.mashup.is_some() { "- Games:".into() } else { "- Stores found:".into() });
        for g in input.games.iter().take(12) {
            let at = match (&g.store, &g.build) {
                (Some(s), Some(b)) if !b.is_empty() => format!("{s}, build {b}"),
                (Some(s), _) => s.clone(),
                (None, _) if g.optional => "not needed (comes inside the mod)".into(),
                (None, _) => "not found on this PC".into(),
            };
            let name = if g.name.is_empty() || g.name == g.id { g.id.clone() } else { format!("{} ({})", g.name, g.id) };
            d.push(format!("  - {name}: {at}"));
        }
    }
    if let Some(e) = input.last_error.as_deref().map(str::trim).filter(|e| !e.is_empty()) {
        let e: String = e.chars().filter(|c| !c.is_control()).take(600).collect();
        d.push(format!("- Last error: {e}"));
    }
    let body = format!(
        "<!-- Written by the SIGF app on your PC. Nothing was sent by SIGF: check the text, fill in the first three parts, then submit. -->\n\n{TEMPLATE}\n### Details (filled in by the SIGF app)\n\n{}\n",
        d.join("\n")
    );
    (title, body)
}

/// What [`with_log`] puts before the log lines: `shown` lines kept, `skip` older ones left out.
fn log_head(name: &str, shown: usize, skip: usize) -> String {
    let note = if skip > 0 { format!(", {} older lines left out to fit the link", skip) } else { String::new() };
    format!("\n<details><summary>{name} (last {shown} lines{note})</summary>\n\n~~~text\n")
}

/// What [`with_log`] puts after the log lines.
const LOG_TAIL: &str = "\n~~~\n\n</details>\n";

fn with_log(body: &str, log: Option<&(String, Vec<String>)>, skip: usize) -> String {
    match log {
        Some((name, lines)) if skip < lines.len() => {
            let shown = &lines[skip..];
            let text = shown.iter().map(|l| fence_safe(l)).collect::<Vec<_>>().join("\n");
            format!("{body}{}{text}{LOG_TAIL}", log_head(name, shown.len(), skip))
        }
        Some((name, lines)) if !lines.is_empty() => format!("{body}\n_{name}: left out to fit the link. Use Copy report in the app for the full text._\n"),
        _ => body.to_string(),
    }
}

/// RFC 3986 unreserved: the bytes [`encode`] keeps as they are.
fn unreserved(b: u8) -> bool {
    matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~')
}

/// Percent-encodes a query value or URI part (RFC 3986 unreserved characters stay): issue links here, Steam launch
/// args in lib.rs.
pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if unreserved(b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `encode(s).len()`, without building it.
fn encoded_len(s: &str) -> usize {
    s.bytes().map(|b| if unreserved(b) { 1 } else { 3 }).sum()
}

/// `https://github.com/<repo>/issues/new?title=..&body=..`.
pub fn new_issue_url(repo: &str, title: &str, body: &str) -> String {
    format!("https://github.com/{repo}/issues/new?title={}&body={}", encode(title), encode(body))
}

/// The issue link and the body it carries, under `max` bytes: older log lines are left out first, then the body is cut
/// at a character boundary (never inside an escape) with a note. Returns (url, body, truncated).
pub fn issue_url(repo: &str, title: &str, body: &str, log: Option<&(String, Vec<String>)>, max: usize) -> (String, String, bool) {
    let title: String = title.chars().take(200).collect();
    let n = log.map(|l| l.1.len()).unwrap_or(0);
    if let Some((name, lines)) = log.filter(|l| !l.1.is_empty()) {
        // Encoded lengths add up: each line is encoded once, and a line left out takes its length (and the `%0A`
        // after it) off the link.
        let fixed = new_issue_url(repo, &title, "").len() + encoded_len(body) + encoded_len(LOG_TAIL);
        let lens: Vec<usize> = lines.iter().map(|l| encoded_len(&fence_safe(l))).collect();
        let mut shown = lens.iter().sum::<usize>() + 3 * (n - 1);
        for skip in 0..n {
            let len = fixed + encoded_len(&log_head(name, n - skip, skip)) + shown;
            if len <= max {
                let b = with_log(body, log, skip);
                let url = new_issue_url(repo, &title, &b);
                debug_assert_eq!(url.len(), len);
                return (url, b, skip > 0);
            }
            shown -= lens[skip] + if skip + 1 < n { 3 } else { 0 };
        }
    }
    // No log line left (or none to begin with).
    let b = with_log(body, log, n);
    let url = new_issue_url(repo, &title, &b);
    if url.len() <= max {
        return (url, b, n > 0);
    }
    // Still too long without any log line: cut the body itself.
    const CUT: &str = "\n\n[shortened to fit the link: use Copy report in the app for the full text]";
    let chars: Vec<char> = b.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let cand: String = chars[..mid].iter().collect::<String>() + CUT;
        if new_issue_url(repo, &title, &cand).len() <= max {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    let cut: String = chars[..lo].iter().collect::<String>() + CUT;
    (new_issue_url(repo, &title, &cut), cut, true)
}

/// The whole report, scrubbed, for this input. `tracker` None: no link (copy only).
pub fn build(input: &ReportInput, cx: &Context, home: Option<&Path>, user: Option<&str>) -> Report {
    let (title, body) = parts(input, cx);
    let s = Scrubber::new(home, user);
    let title = s.scrub(&title);
    let body = s.scrub(&body);
    let log = cx.log.as_ref().map(|(name, lines)| (s.scrub(name), lines.iter().map(|l| s.scrub(l)).collect::<Vec<_>>()));
    let full = with_log(&body, log.as_ref(), 0);
    let found = targets(input.mashup.as_ref());
    let kinds: Vec<&str> = found.iter().map(|(k, _)| *k).collect();
    let preselect = preselect(input, &kinds);
    let targets = found
        .into_iter()
        .map(|(kind, tracker)| {
            let (url, body, truncated) = issue_url(&tracker, &title, &body, log.as_ref(), URL_MAX);
            Target { kind: kind.into(), tracker, url, body, truncated }
        })
        .collect();
    Report { title, full, targets, preselect }
}

/// The report for the UI's input, with this PC's facts.
pub fn make(input: &ReportInput, app_version: &str) -> Report {
    let home = install::home_dir();
    let installed = install::Engine::from_env(None).installed();
    let log = input.mashup.as_ref().and_then(|m| mashup_log(&home, &m.id));
    let cx = Context { app_version: app_version.to_string(), os: os_version(), installed, log };
    let (h, u) = identity();
    build(input, &cx, h.as_deref(), u.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(issues: Option<&str>, links: bool, repo: Option<&str>) -> MashupInput {
        MashupInput {
            id: "sigf/bullyskate".into(),
            name: "BullySkate".into(),
            version: Some("1.0.3".into()),
            links: links.then(|| CardLinks { issues: issues.map(String::from) }),
            repo: repo.map(String::from),
        }
    }

    fn with_id(mut m: MashupInput, id: &str) -> MashupInput {
        m.id = id.into();
        m
    }

    #[test]
    fn mod_tracker_follows_the_card() {
        // Upstream author's tracker (the recipe's `issues` names it).
        assert_eq!(mod_tracker(&card(Some("https://github.com/Faiqie/BullySkate/issues"), true, Some("https://github.com/Faiqie/BullySkate"))).as_deref(), Some("Faiqie/BullySkate"));
        // The SIGFAI copy's own tracker.
        assert_eq!(mod_tracker(&card(Some("https://github.com/SIGFAI/devilutionx-d2/issues"), true, Some("https://github.com/ITSTDMCC/DevilutionX-D2-Movement"))).as_deref(), Some("SIGFAI/devilutionx-d2"));
        // `issues: false`: no tracker for the mod, even though the card has a repo.
        assert_eq!(mod_tracker(&card(None, true, Some("https://github.com/ITSTDMCC/DevilutionX-D2-Movement"))), None);
        // No links (older catalog): the card's repo.
        assert_eq!(mod_tracker(&card(None, false, Some("https://github.com/SIGFAI/mod-89/"))).as_deref(), Some("SIGFAI/mod-89"));
        assert_eq!(mod_tracker(&card(None, false, None)), None);
        // Never another site, a path that is not a tracker, or a bad repo name.
        for bad in ["http://github.com/a/b/issues", "https://evil.example/a/b/issues", "https://github.com/a/b/issues/new?x=1", "https://github.com/a/b", "https://github.com/a/.x/issues", "https://github.com/a/b.git/issues", "https://github.com.evil/a/b/issues"] {
            assert_eq!(mod_tracker(&card(Some(bad), true, None)), None, "{bad}");
        }
        assert_eq!(mod_tracker(&card(None, false, Some("https://gitlab.com/a/b"))), None);
        assert_eq!(issues_repo(APP_ISSUES).as_deref(), Some("SIGFAI/sigf-app"));
    }

    #[test]
    fn sigfai_copy_and_targets() {
        // SIGF's own mashup: the card's repo is the copy; mod and install go to the same tracker, offered once.
        let own = with_id(card(Some("https://github.com/SIGFAI/mod-89/issues"), true, Some("https://github.com/SIGFAI/mod-89")), "sigf/mod-89");
        assert_eq!(sigfai_copy(&own).as_deref(), Some("SIGFAI/mod-89"));
        assert_eq!(targets(Some(&own)), vec![(KIND_INSTALL, "SIGFAI/mod-89".to_string())]);
        // Upstream fusion: the card's repo is the author's, the copy comes from the id.
        let up = card(Some("https://github.com/Faiqie/BullySkate/issues"), true, Some("https://github.com/Faiqie/BullySkate"));
        assert_eq!(sigfai_copy(&up).as_deref(), Some("SIGFAI/bullyskate"));
        assert_eq!(targets(Some(&up)), vec![(KIND_MOD, "Faiqie/BullySkate".to_string()), (KIND_INSTALL, "SIGFAI/bullyskate".to_string())]);
        // `issues: false` (devilutionx-d2): no mod choice, the SIGFAI copy only.
        let d2 = with_id(card(None, true, Some("https://github.com/ITSTDMCC/DevilutionX-D2-Movement")), "sigf/devilutionx-d2");
        assert_eq!(targets(Some(&d2)), vec![(KIND_INSTALL, "SIGFAI/devilutionx-d2".to_string())]);
        // A copy name that is not a repo name: no copy (same URL rule as every link).
        for bad in ["sigf/.x", "sigf/a b", "other/x", "sigf/x.git", ""] {
            assert_eq!(sigfai_copy(&with_id(card(None, true, Some("https://github.com/a/b")), bad)), None, "{bad}");
        }
        // Nothing at all: copy only.
        assert!(targets(Some(&with_id(card(None, true, None), "local"))).is_empty());
        // The app.
        assert_eq!(targets(None), vec![(KIND_APP, "SIGFAI/sigf-app".to_string())]);
    }

    #[test]
    fn preselect_follows_the_last_error() {
        let both = [KIND_MOD, KIND_INSTALL];
        let with = |k: Option<&str>| ReportInput { last_error_kind: k.map(String::from), ..ReportInput::default() };
        assert_eq!(preselect(&with(Some("install")), &both).as_deref(), Some(KIND_INSTALL));
        assert_eq!(preselect(&with(Some("restore")), &both).as_deref(), Some(KIND_INSTALL));
        assert_eq!(preselect(&with(Some("play")), &both).as_deref(), Some(KIND_MOD));
        assert_eq!(preselect(&with(None), &both).as_deref(), Some(KIND_MOD));
        // The wanted kind is not offered: the one that is.
        assert_eq!(preselect(&with(None), &[KIND_INSTALL]).as_deref(), Some(KIND_INSTALL));
        assert_eq!(preselect(&with(None), &[]), None);
    }

    #[test]
    fn scrub_home_user_and_tokens() {
        // Fixture paths are put together with concat! so the public source tree holds no literal user path.
        let home = Path::new(concat!("C:", r"\Users\Jean Dupont"));
        let s = scrub(
            concat!("error at C:", r"\Users\Jean Dupont\AppData\Local\SIGF\build\x and c:", "/users/jean dupont/Documents, D:", r"\Users\other\x, /Users/jean/Library/x, /home/jean/.local"),
            Some(home),
            Some("Jean Dupont"),
        );
        assert_eq!(s, r"error at ~\AppData\Local\SIGF\build\x and ~/Documents, ~\x, ~/Library/x, ~/.local");
        // The user name alone, as a word, not inside other words.
        assert_eq!(scrub("hello jdupont, from JDUPONT-PC; jdupontx stays", None, Some("jdupont")), "hello <user>, from <user>-PC; jdupontx stays");
        // Short or generic names are left alone (they would eat ordinary words).
        assert_eq!(scrub("the user ran it", None, Some("user")), "the user ran it");
        // Tokens.
        let t = scrub(
            // Fake secrets, split with concat! so the public tree's secret scan does not flag the fixture.
            concat!("token=abc123 GH gh", "p_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 Authorization: Bearer eyJhbGciOi.eyJzdWIiOiIx.SflKxwRJSM \"password\": \"hunter2\" https://bob:pw", "@", "example.com/x key AbCdEfGhIjKlMnOpQrStUvWxYz0123456789"),
            None,
            None,
        );
        for leak in ["abc123", "ghp_", "eyJ", "hunter2", "bob:pw", "AbCdEf"] {
            assert!(!t.contains(leak), "{leak} leaked: {t}");
        }
        assert!(t.contains("token=[redacted]") && t.contains("https://[redacted]@example.com/x"), "{t}");
        // Hashes, commits, paths and long file names stay readable.
        let keep = "sha256 9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08 commit 1a5c3a7 ~/AppData/Local/SIGF/build/sigf-bullyskate/some-really-long-file-name-v1.2.3.zip";
        assert_eq!(scrub(keep, None, None), keep);
    }

    #[test]
    fn tail_keeps_the_end() {
        let text = (1..=100).map(|i| format!("line {i}\x1b[31m!\x1b[0m")).collect::<Vec<_>>().join("\r\n") + "\n\n";
        let t = tail_lines(&text, 40);
        assert_eq!(t.len(), 40);
        assert_eq!(t[0], "line 61!");
        assert_eq!(t[39], "line 100!");
        assert_eq!(tail_lines("a\nb", 40), vec!["a", "b"]);
    }

    #[test]
    fn finds_the_newest_kept_log() {
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(logs.join("sigf-other-x-build.log"), "other").unwrap();
        std::fs::write(logs.join("sigf-bullyskate-lib-build.log"), (1..=50).map(|i| format!("step {i}\n")).collect::<String>()).unwrap();
        let (name, lines) = mashup_log(dir.path(), "sigf/bullyskate").unwrap();
        assert_eq!(name, "sigf-bullyskate-lib-build.log");
        assert_eq!(lines.len(), LOG_LINES);
        assert_eq!(lines.last().unwrap(), "step 50");
        assert!(mashup_log(dir.path(), "sigf/nothing").is_none());
    }

    #[test]
    fn url_is_encoded_and_bounded() {
        let u = new_issue_url("SIGFAI/sigf-app", "a b&c", "x=1\n# y");
        assert_eq!(u, "https://github.com/SIGFAI/sigf-app/issues/new?title=a%20b%26c&body=x%3D1%0A%23%20y");
        // Fits: everything kept.
        let log = ("x-build.log".to_string(), (1..=40).map(|i| format!("line {i}")).collect::<Vec<_>>());
        let (url, body, cut) = issue_url("o/r", "t", "body", Some(&log), URL_MAX);
        assert!(!cut && url.len() <= URL_MAX && body.contains("line 1\n") && body.contains("line 40"));
        // Long lines: older lines go first, newest kept.
        let long = ("x-build.log".to_string(), (1..=40).map(|i| format!("line {i} {}", "é".repeat(60))).collect::<Vec<_>>());
        let (url, body, cut) = issue_url("o/r", "t", "body", Some(&long), 4000);
        assert!(cut && url.len() <= 4000, "{}", url.len());
        assert!(body.contains("line 40 ") && !body.contains("line 1 "), "{body}");
        assert!(body.contains("older lines left out"));
        // A huge body: cut at a character boundary, still a valid link under the limit.
        let huge = "é".repeat(5000);
        let (url, body, cut) = issue_url("o/r", "t", &huge, None, 3000);
        assert!(cut && url.len() <= 3000 && url.len() > 2500, "{}", url.len());
        assert!(body.ends_with("for the full text]"));
        // Every escape in the link is whole.
        let q = url.split("body=").nth(1).unwrap();
        let bytes = q.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' {
                assert!(i + 2 < bytes.len() && bytes[i + 1].is_ascii_hexdigit() && bytes[i + 2].is_ascii_hexdigit());
                i += 3;
            } else {
                i += 1;
            }
        }
    }

    #[test]
    fn windows_names() {
        assert_eq!(windows_name("26100", Some(4061), "24H2", "Professional"), "Windows 11 Pro 24H2 (build 26100.4061)");
        assert_eq!(windows_name("19045", None, "22H2", ""), "Windows 10 22H2 (build 19045)");
        assert_eq!(windows_name("26200", Some(9457), "25H2", "Core"), "Windows 11 Home 25H2 (build 26200.9457)");
        assert_eq!(fmt_date(1_791_331_200), "2026-10-07");
    }

    #[test]
    fn whole_report() {
        let input = ReportInput {
            mashup: Some(card(Some("https://github.com/Faiqie/BullySkate/issues"), true, None)),
            games: vec![
                GameInput { id: "bully".into(), name: "Bully: Scholarship Edition".into(), store: Some("steam".into()), build: Some("1234567".into()), optional: false },
                GameInput { id: "skate3".into(), name: "skate 3".into(), store: None, build: None, optional: true },
            ],
            progress: None,
            last_error: Some(concat!("Building lib failed (log: C:", r"\Users\Jean\AppData\Local\SIGF\logs\sigf-bullyskate-lib-build.log)").into()),
            last_error_kind: Some("install".into()),
        };
        let cx = Context {
            app_version: "0.1.2".into(),
            os: "Windows 11 Home 25H2 (build 26200.6899)".into(),
            installed: vec![],
            log: Some(("sigf-bullyskate-lib-build.log".into(), vec![concat!("cc: C:", r"\Users\Jean\AppData\Local\SIGF\build\a.c: error").into(), "token=s3cr3t".into()])),
        };
        let r = build(&input, &cx, Some(Path::new(concat!("C:", r"\Users\Jean"))), Some("Jean"));
        // A player build failed: the install problem is preselected, the author's tracker still offered.
        assert_eq!(r.preselect.as_deref(), Some(KIND_INSTALL));
        assert_eq!(r.targets.iter().map(|t| (t.kind.as_str(), t.tracker.as_str())).collect::<Vec<_>>(), vec![(KIND_MOD, "Faiqie/BullySkate"), (KIND_INSTALL, "SIGFAI/bullyskate")]);
        assert!(r.targets[0].url.starts_with("https://github.com/Faiqie/BullySkate/issues/new?title=BullySkate%201.0.3%3A%20&body="));
        assert!(r.targets[1].url.starts_with("https://github.com/SIGFAI/bullyskate/issues/new?title="));
        let t = &r.targets[1];
        assert!(!t.truncated && t.body == r.full);
        for want in ["- SIGF app: 0.1.2", "- Install: not installed", "Bully: Scholarship Edition (bully): steam, build 1234567", "skate 3 (skate3): not needed", "### What happened", r"~\AppData\Local\SIGF\logs", "token=[redacted]", "~~~text"] {
            assert!(t.body.contains(want), "missing {want}:\n{}", t.body);
        }
        assert!(!r.full.contains("Jean") && !r.full.contains("s3cr3t"));
        // No tracker and no copy: copy only.
        let none = ReportInput { mashup: Some(with_id(card(None, true, None), "local")), ..input.clone() };
        let r = build(&none, &cx, None, None);
        assert!(r.targets.is_empty() && r.preselect.is_none() && r.full.contains("### Steps to reproduce"));
        // The app itself.
        let app = build(&ReportInput::default(), &cx, None, None);
        assert_eq!(app.targets.len(), 1);
        assert_eq!(app.targets[0].tracker, "SIGFAI/sigf-app");
        assert!(app.title.starts_with("SIGF app 0.1.2") && app.full.contains("- Installed mashups: none"));
    }
}
