//! Starting an installed mod's games beyond a store URI: a launch `exe` (script extender loader) from the game folder,
//! Steam running before a Steam game's loader, and `wait: "port:<n>"` between ordered launch steps
//! (docs/RECIPE-FORMAT.md section 4, `launch[]`). Everything here is testable without a game, store or Prism process:
//! Steam sits behind the `Steam` trait.

use crate::install::paths::resolve_inside;
use crate::install::LaunchExe;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How long the next game waits for the previous one's port.
pub const PORT_TIMEOUT: Duration = Duration::from_secs(120);
/// How long a loader waits for Steam to come up after `steam://open/main`.
pub const STEAM_TIMEOUT: Duration = Duration::from_secs(20);
pub const POLL: Duration = Duration::from_millis(500);

/// `port:<n>` -> n (1..=65535). Anything else is not a wait the app knows.
pub fn parse_wait(w: &str) -> Option<u16> {
    let n = w.strip_prefix("port:")?;
    if n.is_empty() || n.len() > 5 || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    n.parse::<u16>().ok().filter(|p| *p != 0)
}

/// The launch exe's absolute path: inside its game folder (same rule as install destinations, junctions included)
/// and present on disk. Missing: the error names the exe and where to get it (the recipe's `requires` page).
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
    let real_dir = dir.canonicalize().map_err(|e| format!("{}: {e}", x.dir))?;
    let real = abs.canonicalize().map_err(|e| format!("{}: {e}", abs.display()))?;
    if !real.starts_with(&real_dir) {
        return Err(format!("refused launch exe outside the game folder: {}", x.path));
    }
    Ok(abs)
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

/// Is `steam.exe` running (tasklist, no console window)? Always false off Windows.
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
    #[cfg(not(windows))]
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
        LaunchExe { path: path.into(), dir: dir.to_string_lossy().into_owned(), hint: hint.map(Into::into) }
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
}
