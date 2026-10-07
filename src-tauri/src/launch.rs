//! Starting an installed mod's games beyond a store URI: a launch `exe` (script extender loader) from the game folder,
//! an `app_exe` from the mashup's own folder, me3 with a `.me3` profile, Steam running before a Steam game's loader,
//! `wait: "port:<n>"` between ordered launch steps, and the `requires_files` checked before anything starts
//! (docs/RECIPE-FORMAT.md section 4, `launch[]` and `requires_files`). Everything here is testable without a game,
//! store or Prism process: Steam sits behind the `Steam` trait, and me3 is only ever a command line built here.

use crate::install::paths::resolve_inside;
use crate::install::{LaunchExe, Me3Launch, RequiredAt};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How long the next game waits for the previous one's port.
pub const PORT_TIMEOUT: Duration = Duration::from_secs(120);
/// How long a loader waits for Steam to come up after `steam://open/main`.
pub const STEAM_TIMEOUT: Duration = Duration::from_secs(20);
pub const POLL: Duration = Duration::from_millis(500);

/// Why Play did not start: `missingFile` (a `requires_files` prerequisite is not installed; `page` says where to get
/// it) or `launch` (anything else). With `missingFile` nothing was started.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayError {
    pub kind: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
}

impl PlayError {
    pub fn missing_file(message: &str, page: Option<&str>) -> Self {
        Self { kind: "missingFile", message: message.to_string(), page: page.map(String::from) }
    }
}

impl From<String> for PlayError {
    fn from(message: String) -> Self {
        Self { kind: "launch", message, page: None }
    }
}

impl std::fmt::Display for PlayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.page {
            Some(p) => write!(f, "{}: {p}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

/// `port:<n>` -> n (1..=65535). Anything else is not a wait the app knows.
pub fn parse_wait(w: &str) -> Option<u16> {
    let n = w.strip_prefix("port:")?;
    if n.is_empty() || n.len() > 5 || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    n.parse::<u16>().ok().filter(|p| *p != 0)
}

/// `rel` (after an optional leading `placeholder`) inside `dir` (junctions included) and an existing file: its absolute
/// path, else None.
fn file_inside(dir: &Path, placeholder: &str, rel: &str) -> Option<PathBuf> {
    let (abs, _) = resolve_inside(dir, placeholder, rel).ok()?;
    if !abs.is_file() {
        return None;
    }
    let real_dir = dir.canonicalize().ok()?;
    abs.canonicalize().ok().filter(|r| r.starts_with(&real_dir)).map(|_| abs)
}

/// A `requires_files` entry is present: the file exists inside its game folder.
pub fn required_present(r: &RequiredAt) -> bool {
    file_inside(Path::new(&r.dir), "", &r.path).is_some()
}

/// Every prerequisite file is there, else the first missing one's message and page. Play runs it before it starts
/// anything.
pub fn check_required(files: &[RequiredAt]) -> Result<(), PlayError> {
    match files.iter().find(|r| !required_present(r)) {
        Some(r) => Err(PlayError::missing_file(&r.message, r.page.as_deref())),
        None => Ok(()),
    }
}

/// The launch exe's absolute path: inside its folder (same rule as install destinations, junctions included), present
/// on disk, and with the sha256 the recipe pinned when the app installed it. Missing: the error names the exe and
/// where to get it (the recipe's `requires` page).
pub fn resolve_exe(x: &LaunchExe) -> Result<PathBuf, String> {
    let dir = Path::new(&x.dir);
    let (abs, _) = resolve_inside(dir, "{game}", &x.path).map_err(|e| e.to_string())?;
    if !abs.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")) {
        return Err(format!("refused launch exe: {}", x.path));
    }
    let name = abs.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| x.path.clone());
    if !abs.is_file() {
        return Err(match &x.hint {
            Some(h) => format!("{name} not found: {h}"),
            None => format!("{name} not found in {}", x.dir),
        });
    }
    // Present, but a junction or link may still lead out of the folder.
    let abs = file_inside(dir, "{game}", &x.path).ok_or_else(|| format!("refused launch exe outside its folder: {}", x.path))?;
    if let Some(want) = &x.sha256 {
        let got = crate::install::fetch::sha256_file(&abs).map_err(|e| e.to_string())?;
        if !got.eq_ignore_ascii_case(want) {
            return Err(format!("{name} changed since the install: Restore vanilla, then Get the mashup again"));
        }
    }
    Ok(abs)
}

/// The games me3 starts (its release ships a default profile for each): their canonical id is also me3's `--game` id.
pub const ME3_GAMES: &[&str] = &["eldenring", "nightreign", "sekiro"];

/// Canonical game id -> me3's `--game` id.
pub fn me3_game(game: &str) -> Option<&'static str> {
    ME3_GAMES.iter().find(|g| **g == game).copied()
}

/// Where me3's own installer (`me3_installer.exe`) puts me3: `<local app data>/Programs/garyttierney/me3/bin/me3.exe`
/// (`%LOCALAPPDATA%` on Windows). The only place a player's me3 is looked for: never PATH or the working folder.
pub fn me3_installed_path(local_app_data: &Path) -> Option<PathBuf> {
    local_app_data
        .is_absolute()
        .then(|| local_app_data.join("Programs").join("garyttierney").join("me3").join("bin").join("me3.exe"))
}

/// A me3 start, fully built: the program, its arguments, its working folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Me3Command {
    pub exe: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

/// The me3 command for an installed launch: me3 (the recipe's pinned copy, else the installer's, `installed`), then
/// `launch --game <id> --profile <profile> [--savefile <name>] [--disable-arxan] --online false`. Nothing from the
/// recipe reaches the command line but the checked profile path, savefile name and flag; always offline.
pub fn me3_command(m: &Me3Launch, installed: Option<&Path>) -> Result<Me3Command, String> {
    if !ME3_GAMES.contains(&m.game.as_str()) {
        return Err(format!("me3 does not start {}", m.game));
    }
    let exe = match &m.exe {
        Some(x) => resolve_exe(x)?,
        None => installed.filter(|p| p.is_file()).map(Path::to_path_buf).ok_or_else(|| match &m.hint {
            Some(h) => format!("me3 not found: {h}"),
            None => "me3 not found: install it with me3_installer.exe".to_string(),
        })?,
    };
    if !m.profile.path.to_ascii_lowercase().ends_with(".me3") {
        return Err(format!("refused me3 profile: {}", m.profile.path));
    }
    let profile = file_inside(Path::new(&m.profile.dir), "", &m.profile.path)
        .ok_or_else(|| format!("{} not found in {}: Restore vanilla, then Get the mashup again", m.profile.path, m.profile.dir))?;
    let mut args: Vec<String> = vec!["launch".into(), "--game".into(), m.game.clone(), "--profile".into(), profile.to_string_lossy().into_owned()];
    if let Some(s) = &m.savefile {
        if !crate::install::check::savefile_ok(s) {
            return Err(format!("refused me3 savefile: {s}"));
        }
        args.extend(["--savefile".into(), s.clone()]);
    }
    if m.disable_arxan {
        args.push("--disable-arxan".into());
    }
    args.extend(["--online".into(), "false".into()]);
    let cwd = profile.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(&m.profile.dir));
    Ok(Me3Command { exe, args, cwd })
}

/// Polls `127.0.0.1:<port>` until something accepts a connection, or fails after `timeout`.
pub fn wait_port(port: u16, timeout: Duration, poll: Duration) -> Result<(), String> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let end = Instant::now() + timeout;
    loop {
        if TcpStream::connect_timeout(&addr, poll.min(Duration::from_secs(1))).is_ok() {
            return Ok(());
        }
        if Instant::now() >= end {
            return Err(format!(
                "nothing answered on local port {port} after {} s: the game started before it did not open its link",
                timeout.as_secs()
            ));
        }
        std::thread::sleep(poll);
    }
}

/// The Steam client, as far as a launch needs it.
pub trait Steam {
    fn running(&self) -> bool;
    /// Asks Steam to start (`steam://open/main`); returns at once.
    fn start(&self) -> Result<(), String>;
}

/// A loader exe started while Steam is down makes the game fail its Steam check: start Steam and wait for it.
pub fn ensure_steam(s: &dyn Steam, timeout: Duration, poll: Duration) -> Result<(), String> {
    if s.running() {
        return Ok(());
    }
    s.start()?;
    let end = Instant::now() + timeout;
    loop {
        std::thread::sleep(poll);
        if s.running() {
            return Ok(());
        }
        if Instant::now() >= end {
            return Err(format!("Steam did not start within {} s: open Steam and sign in, then press Play again", timeout.as_secs()));
        }
    }
}

/// `%SystemRoot%\System32\tasklist.exe` by absolute path, never whatever `tasklist` the PATH or the working folder
/// would resolve to.
#[cfg(windows)]
fn tasklist_exe() -> PathBuf {
    let root = std::env::var_os("SystemRoot").filter(|r| !r.is_empty()).map(PathBuf::from);
    let root = root.filter(|r| r.is_absolute()).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    root.join("System32").join("tasklist.exe")
}

/// Is the Steam client running? Windows: `steam.exe` in tasklist (no console window). macOS: a `steam_osx` process
/// (`/usr/bin/pgrep`). Elsewhere always false.
pub fn steam_process_running() -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new(tasklist_exe())
            .args(["/FI", "IMAGENAME eq steam.exe", "/FO", "CSV", "/NH"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_ascii_lowercase().contains("\"steam.exe\""))
            .unwrap_or(false)
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("/usr/bin/pgrep").args(["-x", "steam_osx"]).output().is_ok_and(|o| o.status.success())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::net::TcpListener;

    #[cfg(windows)]
    #[test]
    fn tasklist_by_absolute_path() {
        let p = tasklist_exe();
        assert!(p.is_absolute(), "{}", p.display());
        assert!(p.ends_with("System32/tasklist.exe") || p.ends_with(r"System32	asklist.exe"));
    }

    fn exe(dir: &Path, path: &str, hint: Option<&str>) -> LaunchExe {
        LaunchExe { path: path.into(), dir: dir.to_string_lossy().into_owned(), hint: hint.map(Into::into), sha256: None, own: false }
    }

    #[test]
    fn waits() {
        assert_eq!(parse_wait("port:25599"), Some(25599));
        for bad in ["port:", "port:0", "port:65536", "port:-1", "port:12a", "port:123456", "tcp:80", "", "port: 80"] {
            assert_eq!(parse_wait(bad), None, "{bad}");
        }
    }

    #[test]
    fn exe_inside_the_game_folder_only() {
        let t = tempfile::tempdir().unwrap();
        let game = t.path().join("Skyrim");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(game.join("skse64_loader.exe"), b"loader").unwrap();
        std::fs::write(t.path().join("evil.exe"), b"evil").unwrap();
        assert_eq!(resolve_exe(&exe(&game, "skse64_loader.exe", None)).unwrap(), game.join("skse64_loader.exe"));
        assert_eq!(resolve_exe(&exe(&game, "{game}/skse64_loader.exe", None)).unwrap(), game.join("skse64_loader.exe"));
        for bad in ["../evil.exe", "..\\evil.exe", "sub/../../evil.exe", "C:/Windows/notepad.exe", "/evil.exe", "{app}/x.exe", "a.exe:ads", "skse64_loader.bat"] {
            let e = resolve_exe(&exe(&game, bad, None)).unwrap_err();
            assert!(!e.contains("not found"), "{bad}: {e}");
        }
    }

    #[test]
    fn missing_exe_names_where_to_get_it() {
        let t = tempfile::tempdir().unwrap();
        let e = resolve_exe(&exe(t.path(), "skse64_loader.exe", Some("install SKSE64 from skse.silverlock.org"))).unwrap_err();
        assert_eq!(e, "skse64_loader.exe not found: install SKSE64 from skse.silverlock.org");
        let e = resolve_exe(&exe(t.path(), "f4se_loader.exe", None)).unwrap_err();
        assert!(e.starts_with("f4se_loader.exe not found in "), "{e}");
    }

    #[test]
    fn port_wait() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        wait_port(port, Duration::from_secs(5), Duration::from_millis(20)).unwrap();
        drop(l);
        // A port nobody listens on (just freed): a clear error after the timeout.
        let e = wait_port(port, Duration::from_millis(200), Duration::from_millis(20)).unwrap_err();
        assert!(e.contains(&format!("port {port}")), "{e}");
        // A listener that comes up while we poll.
        let free = TcpListener::bind("127.0.0.1:0").unwrap();
        let late = free.local_addr().unwrap().port();
        drop(free);
        let h = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            let l = TcpListener::bind(("127.0.0.1", late)).unwrap();
            std::thread::sleep(Duration::from_millis(1500));
            drop(l);
        });
        wait_port(late, Duration::from_secs(5), Duration::from_millis(20)).unwrap();
        h.join().unwrap();
    }

    struct FakeSteam {
        up_after: Option<u32>,
        polls: Cell<u32>,
        started: Cell<u32>,
    }

    impl Steam for FakeSteam {
        fn running(&self) -> bool {
            let n = self.polls.get();
            self.polls.set(n + 1);
            self.up_after.is_some_and(|k| n >= k)
        }
        fn start(&self) -> Result<(), String> {
            self.started.set(self.started.get() + 1);
            Ok(())
        }
    }

    fn fake(up_after: Option<u32>) -> FakeSteam {
        FakeSteam { up_after, polls: Cell::new(0), started: Cell::new(0) }
    }

    #[test]
    fn steam_started_only_when_down() {
        let up = fake(Some(0));
        ensure_steam(&up, Duration::from_secs(1), Duration::from_millis(1)).unwrap();
        assert_eq!(up.started.get(), 0);

        let later = fake(Some(3));
        ensure_steam(&later, Duration::from_secs(5), Duration::from_millis(1)).unwrap();
        assert_eq!(later.started.get(), 1);

        let never = fake(None);
        let e = ensure_steam(&never, Duration::from_millis(50), Duration::from_millis(5)).unwrap_err();
        assert!(e.contains("Steam did not start"), "{e}");
        assert_eq!(never.started.get(), 1);
    }

    fn sha(b: &[u8]) -> String {
        crate::install::fetch::hex(&<sha2::Sha256 as sha2::Digest>::digest(b))
    }

    #[test]
    fn pinned_exe_must_keep_its_sha256() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("iw4l.exe"), b"stub, never run").unwrap();
        let mut x = exe(t.path(), "iw4l.exe", None);
        x.sha256 = Some(sha(b"stub, never run"));
        assert_eq!(resolve_exe(&x).unwrap(), t.path().join("iw4l.exe"));
        x.sha256 = Some(sha(b"another build"));
        let e = resolve_exe(&x).unwrap_err();
        assert!(e.contains("changed since the install"), "{e}");
    }

    fn required(dir: &Path, path: &str) -> RequiredAt {
        RequiredAt {
            id: "xnvse".into(),
            path: path.into(),
            dir: dir.to_string_lossy().into_owned(),
            message: "Install xNVSE 6.4.9+ first".into(),
            page: Some("https://github.com/xNVSE/NVSE/releases".into()),
        }
    }

    #[test]
    fn required_files_block_play_with_their_message_and_page() {
        let t = tempfile::tempdir().unwrap();
        let game = t.path().join("FalloutNV");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(t.path().join("nvse_loader.exe"), b"outside").unwrap();
        let files = vec![required(&game, "nvse_loader.exe")];
        let e = check_required(&files).unwrap_err();
        assert_eq!(e, PlayError::missing_file("Install xNVSE 6.4.9+ first", Some("https://github.com/xNVSE/NVSE/releases")));
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["kind"], "missingFile");
        assert_eq!(json["page"], "https://github.com/xNVSE/NVSE/releases");
        assert_eq!(e.to_string(), "Install xNVSE 6.4.9+ first: https://github.com/xNVSE/NVSE/releases");
        // A folder of that name is not the file; paths never leave the game folder.
        std::fs::create_dir_all(game.join("nvse_loader.exe")).unwrap();
        assert!(check_required(&files).is_err());
        for bad in ["../nvse_loader.exe", "C:/nvse_loader.exe", "/nvse_loader.exe"] {
            assert!(!required_present(&required(&game, bad)), "{bad}");
        }
        std::fs::remove_dir(game.join("nvse_loader.exe")).unwrap();
        std::fs::write(game.join("nvse_loader.exe"), b"stub").unwrap();
        check_required(&files).unwrap();
        check_required(&[]).unwrap();
        assert_eq!(PlayError::from("x".to_string()).kind, "launch");
    }

    /// A me3 launch with stub files only: me3.exe and the profile are bytes on disk, nothing is ever started.
    fn me3_setup(t: &Path) -> (PathBuf, Me3Launch) {
        let app = t.join("SIGF/profiles/sigf-er-mario/eldenring");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join("er-mario.me3"), b"profileVersion = \"v1\"").unwrap();
        let local = t.join("LocalAppData");
        let me3 = me3_installed_path(&local).unwrap();
        std::fs::create_dir_all(me3.parent().unwrap()).unwrap();
        std::fs::write(&me3, b"stub me3, never run").unwrap();
        let m = Me3Launch {
            game: "eldenring".into(),
            profile: exe(&app, "er-mario.me3", None),
            exe: None,
            savefile: None,
            disable_arxan: false,
            hint: Some("install ME3 from github.com/garyttierney/me3/releases/tag/v0.13.0".into()),
        };
        (me3, m)
    }

    #[test]
    fn me3_command_is_built_by_the_app_and_always_offline() {
        let t = tempfile::tempdir().unwrap();
        let (me3, mut m) = me3_setup(t.path());
        assert!(me3.ends_with("Programs/garyttierney/me3/bin/me3.exe") || me3.ends_with(r"Programs\garyttierney\me3\bin\me3.exe"));
        let profile = Path::new(&m.profile.dir).join("er-mario.me3");
        let c = me3_command(&m, Some(&me3)).unwrap();
        assert_eq!(c.exe, me3);
        assert_eq!(c.args, ["launch", "--game", "eldenring", "--profile", &profile.to_string_lossy(), "--online", "false"]);
        assert_eq!(c.cwd, Path::new(&m.profile.dir));

        m.savefile = Some("EldenKill.sl2".into());
        m.disable_arxan = true;
        let c = me3_command(&m, Some(&me3)).unwrap();
        assert_eq!(&c.args[5..], ["--savefile", "EldenKill.sl2", "--disable-arxan", "--online", "false"]);

        // No me3 installed: where to get it, nothing started.
        let e = me3_command(&m, Some(&t.path().join("nowhere/me3.exe"))).unwrap_err();
        assert_eq!(e, "me3 not found: install ME3 from github.com/garyttierney/me3/releases/tag/v0.13.0");
        assert!(me3_command(&m, None).is_err());
        assert!(me3_installed_path(Path::new("relative")).is_none(), "never a relative folder");

        // Tampered or escaping values never reach the command line.
        for bad in ["EldenKill.sl2 --online true", "../x.sl2", "x.exe", ".sl2"] {
            let mut b = m.clone();
            b.savefile = Some(bad.into());
            assert!(me3_command(&b, Some(&me3)).is_err(), "{bad}");
        }
        for bad in ["../er-mario.me3", "er-mario.txt", "missing.me3", "C:/x.me3"] {
            let mut b = m.clone();
            b.profile.path = bad.into();
            assert!(me3_command(&b, Some(&me3)).is_err(), "{bad}");
        }
        let mut b = m.clone();
        b.game = "darksouls1".into();
        assert!(me3_command(&b, Some(&me3)).is_err());
    }

    #[test]
    fn me3_shipped_by_the_recipe_is_pinned() {
        let t = tempfile::tempdir().unwrap();
        let (_, mut m) = me3_setup(t.path());
        let game = t.path().join("ELDEN RING");
        std::fs::create_dir_all(game.join("EldenCraft/me3/bin")).unwrap();
        std::fs::write(game.join("EldenCraft/me3/bin/me3.exe"), b"shipped me3").unwrap();
        let mut x = exe(&game, "EldenCraft/me3/bin/me3.exe", None);
        x.sha256 = Some(sha(b"shipped me3"));
        m.exe = Some(x);
        let c = me3_command(&m, None).unwrap();
        assert_eq!(c.exe, game.join("EldenCraft/me3/bin/me3.exe"));
        std::fs::write(game.join("EldenCraft/me3/bin/me3.exe"), b"swapped").unwrap();
        assert!(me3_command(&m, None).unwrap_err().contains("changed since the install"));
        assert_eq!(me3_game("eldenring"), Some("eldenring"));
        assert_eq!(me3_game("skyrim"), None);
    }
}
