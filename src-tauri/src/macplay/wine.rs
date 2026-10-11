//! Wine inside one game's prefix: the environment (the core's own keys, then the recipe's), processes started
//! without a shell with their output in the game's log, the prefix setup (wineboot, Mono, VC++ runtime, DXMT's
//! `winemetal.dll`, Windows Steam), and where Steam put the game.

use super::recipe::{Verb, WineBlock};

use super::{engine, Layout, MacError};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Steam's folder inside a wow64 prefix.
pub const STEAM_DIR: &[&str] = &["drive_c", "Program Files (x86)", "Steam"];
/// Flags that keep Windows Steam's web views alive under Wine on a Mac (GameToMac and Heroic use them).
pub const STEAM_FLAGS: &[&str] = &["-cef-disable-gpu", "-no-cef-sandbox"];

const BOOT_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const INSTALL_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const SERVER_TIMEOUT: Duration = Duration::from_secs(2 * 60);

pub fn steam_root(prefix: &Path) -> PathBuf {
    STEAM_DIR.iter().fold(prefix.to_path_buf(), |p, s| p.join(s))
}

pub fn steam_exe(prefix: &Path) -> PathBuf {
    steam_root(prefix).join("steam.exe")
}

/// A Unix path as Wine's `Z:` drive sees it (`/Users/a/x.msi` -> `Z:\Users\a\x.msi`).
pub fn dos_path(p: &Path) -> OsString {
    let mut s = OsString::from("Z:");
    s.push(p.as_os_str().to_string_lossy().replace('/', "\\"));
    s
}

/// Replaces `{prefix}`, `{data}` and `{engine}` in a recipe value (env or argument).
pub fn expand(v: &str, vars: &[(&str, &Path)]) -> String {
    vars.iter().fold(v.to_string(), |s, (k, p)| s.replace(&format!("{{{k}}}"), &p.to_string_lossy()))
}

/// One Wine engine pointed at one prefix.
#[derive(Debug, Clone)]
pub struct Wine {
    /// The folder holding `bin/wine` and `lib/wine`.
    pub root: PathBuf,
    pub prefix: PathBuf,
    pub log: PathBuf,
    /// Set after the core's own keys (recipe env, already checked: no WINE* key the core owns).
    pub env: Vec<(String, String)>,
    pub dll_overrides: String,
    /// The runtime's libraries (`@rpath` dylibs the engine does not carry), on `DYLD_FALLBACK_LIBRARY_PATH`.
    pub lib_dirs: Vec<PathBuf>,
}

impl Wine {
    pub fn new(root: PathBuf, prefix: PathBuf, log: PathBuf) -> Self {
        Self { root, prefix, log, env: vec![], dll_overrides: String::new(), lib_dirs: vec![] }
    }

    pub fn with_libs(mut self, dirs: Vec<PathBuf>) -> Self {
        self.lib_dirs = dirs;
        self
    }

    /// The recipe's env (`{prefix}` expanded) and overrides, for the store and the game.
    pub fn with_recipe(mut self, w: &WineBlock) -> Self {
        let vars = [("prefix", self.prefix.as_path())];
        self.env = w.env.iter().map(|(k, v)| (k.clone(), expand(v, &vars))).collect();
        self.dll_overrides = w.dll_overrides.clone();
        self
    }

    pub fn wine_bin(&self) -> PathBuf {
        self.root.join("bin").join("wine")
    }

    pub fn server_bin(&self) -> PathBuf {
        self.root.join("bin").join("wineserver")
    }

    /// The environment, in order: the core's keys, `WINEDEBUG=-all` unless the recipe sets it, then the recipe's.
    pub fn env_pairs(&self) -> Vec<(OsString, OsString)> {
        let mut out: Vec<(OsString, OsString)> = vec![
            ("WINEPREFIX".into(), self.prefix.clone().into()),
            ("WINESERVER".into(), self.server_bin().into()),
            ("WINELOADER".into(), self.wine_bin().into()),
            ("WINEDLLOVERRIDES".into(), self.dll_overrides.clone().into()),
        ];
        if !self.lib_dirs.is_empty() {
            // How Sikarugir's own launcher finds them; the system defaults stay after.
            let mut v = std::env::join_paths(self.lib_dirs.iter()).unwrap_or_default();
            v.push(":/usr/local/lib:/usr/lib");
            out.push(("DYLD_FALLBACK_LIBRARY_PATH".into(), v));
        }
        // `SIGF_MACPLAY_WINEDEBUG` (a support knob, e.g. `err+all`) wins over the quiet default and the recipe.
        match std::env::var_os("SIGF_MACPLAY_WINEDEBUG").filter(|v| !v.is_empty()) {
            Some(v) => out.push(("WINEDEBUG".into(), v)),
            None if !self.env.iter().any(|(k, _)| k == "WINEDEBUG") => out.push(("WINEDEBUG".into(), "-all".into())),
            None => {}
        }
        let debug_knob = std::env::var_os("SIGF_MACPLAY_WINEDEBUG").is_some_and(|v| !v.is_empty());
        out.extend(self.env.iter().filter(|(k, _)| !(debug_knob && k == "WINEDEBUG")).map(|(k, v)| (k.into(), v.into())));
        out
    }

    /// `<root>/bin/<program> <args>`, the environment set, inherited WINE* keys removed, output into the log.
    fn command(&self, program: &Path, args: &[&OsStr]) -> Result<Command, MacError> {
        let mut c = Command::new(program);
        c.args(args);
        for (k, _) in std::env::vars_os() {
            if k.to_string_lossy().starts_with("WINE") {
                c.env_remove(k);
            }
        }
        c.envs(self.env_pairs());
        let out = super::log_file(&self.log)?;
        let err = out.try_clone().map_err(|e| MacError::io(&self.log, e))?;
        c.stdin(Stdio::null()).stdout(out).stderr(err);
        Ok(c)
    }

    /// `wine <args>` started and left running (the store, the game).
    pub fn spawn(&self, args: &[&OsStr], cwd: Option<&Path>) -> Result<Child, MacError> {
        let mut c = self.command(&self.wine_bin(), args)?;
        if let Some(d) = cwd {
            c.current_dir(d);
        }
        super::log_line(&self.log, &format!("wine {}", args.iter().map(|a| a.to_string_lossy()).collect::<Vec<_>>().join(" ")));
        c.spawn().map_err(|e| MacError::new("wine", format!("wine: {e}")))
    }

    /// `wine <args>`, waited for (killed past `timeout`), then the prefix's wineserver waited for too, so the
    /// registry is written before the next step.
    pub fn run(&self, args: &[&OsStr], timeout: Duration) -> Result<(), MacError> {
        let mut child = self.spawn(args, None)?;
        let status = wait(&mut child, timeout);
        self.wait_server();
        match status {
            Some(s) if s.success() => Ok(()),
            Some(s) => Err(MacError::new("wine", format!("wine {} failed ({s})", args.first().map(|a| a.to_string_lossy()).unwrap_or_default()))),
            None => {
                self.kill();
                Err(MacError::new("timeout", format!("wine {} took too long", args.first().map(|a| a.to_string_lossy()).unwrap_or_default())))
            }
        }
    }

    /// `wineserver -w`: waits until every process of the prefix ended (bounded).
    pub fn wait_server(&self) {
        if let Ok(mut c) = self.command(&self.server_bin(), &["-w".as_ref()]).and_then(|mut c| c.spawn().map_err(|e| MacError::new("wine", e.to_string()))) {
            if wait(&mut c, SERVER_TIMEOUT).is_none() {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
    }

    /// `wineserver -k`: ends every process of this prefix.
    pub fn kill(&self) {
        if let Ok(mut c) = self.command(&self.server_bin(), &["-k".as_ref()]) {
            if let Ok(mut ch) = c.spawn() {
                if wait(&mut ch, Duration::from_secs(30)).is_none() {
                    let _ = ch.kill();
                    let _ = ch.wait();
                }
            }
        }
    }
}

/// Waits for `child` up to `timeout`; None when it still runs.
fn wait(child: &mut Child, timeout: Duration) -> Option<std::process::ExitStatus> {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(s)) => return Some(s),
            Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_millis(250)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Err(_) => return None,
        }
    }
}

/// Wine's server socket for a prefix: `/tmp/.wine-<uid>/server-<dev>-<inode>/socket` (dlls/ntdll/unix/server.c).
/// It exists while the prefix's wineserver runs.
#[cfg(unix)]
pub fn server_socket(prefix: &Path) -> Option<PathBuf> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::metadata(prefix).ok()?;
    Some(PathBuf::from(format!("/tmp/.wine-{}", m.uid())).join(format!("server-{:x}-{:x}", m.dev(), m.ino())).join("socket"))
}

#[cfg(not(unix))]
pub fn server_socket(_prefix: &Path) -> Option<PathBuf> {
    None
}

pub fn server_running(prefix: &Path) -> bool {
    server_socket(prefix).is_some_and(|s| s.exists())
}

/// Written into the prefix as steps finish, so a setup that stopped half way picks up where it was.
pub const PREFIX_FILE: &str = ".sigf-prefix.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrefixState {
    /// The engine folder name the prefix was made with.
    pub engine: String,
    pub steps: Vec<String>,
}

impl PrefixState {
    pub fn load(prefix: &Path) -> Self {
        std::fs::read_to_string(prefix.join(PREFIX_FILE)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }
    pub fn has(&self, step: &str) -> bool {
        self.steps.iter().any(|s| s == step)
    }
    fn mark(&mut self, prefix: &Path, step: &str) -> Result<(), MacError> {
        if !self.has(step) {
            self.steps.push(step.to_string());
        }
        let p = prefix.join(PREFIX_FILE);
        std::fs::write(&p, serde_json::to_string_pretty(self).unwrap_or_default()).map_err(|e| MacError::io(&p, e))
    }
    /// Booted, and the renderer's DLL in place when the recipe has one.
    pub fn ready(&self, renderer: bool) -> bool {
        self.has("wineboot") && (!renderer || self.has("renderer"))
    }
}

/// The engine bundles Wine Mono (`share/wine/mono`): wineboot installs it, nothing to download.
pub fn bundled_mono(root: &Path) -> bool {
    std::fs::read_dir(root.join("share").join("wine").join("mono")).is_ok_and(|mut d| d.next().is_some())
}

/// The prefix steps of `mac_setup`, each skipped when done before: `wineboot -u` (wow64), the verbs, DXMT's
/// `winemetal.dll` into system32/syswow64, then Windows Steam (`SteamSetup.exe /S`). `on(step)` before each.
pub fn setup_prefix(layout: &Layout, wine: &Wine, engine_name: &str, verbs: &[Verb], renderer: bool, on: &mut dyn FnMut(&str, u64, u64)) -> Result<(), MacError> {
    let prefix = &wine.prefix;
    let mut st = PrefixState::load(prefix);
    if !st.engine.is_empty() && st.engine != engine_name {
        // Another engine made this prefix: start it again (a mixed prefix breaks in odd ways).
        super::log_line(&wine.log, &format!("prefix made with {}, now {engine_name}: recreating it", st.engine));
        wine.kill();
        std::fs::remove_dir_all(prefix).map_err(|e| MacError::io(prefix, e))?;
        st = PrefixState::default();
    }
    std::fs::create_dir_all(prefix).map_err(|e| MacError::io(prefix, e))?;
    st.engine = engine_name.to_string();
    if !st.has("wineboot") {
        on("prefix", 0, 0);
        wine.run(&["wineboot".as_ref(), "-u".as_ref()], BOOT_TIMEOUT)?;
        if !prefix.join("system.reg").is_file() || !prefix.join("drive_c").join("windows").is_dir() {
            return Err(MacError::new("wine", "wineboot did not create the prefix"));
        }
        st.mark(prefix, "wineboot")?;
    }
    for verb in verbs {
        match verb {
            Verb::Mono if !st.has("mono") => {
                on("mono", 0, 0);
                if !bundled_mono(&wine.root) {
                    let msi = engine::fetch_pinned(layout, &engine::WINE_MONO, &mut |d, t| on("mono", d, t.unwrap_or(engine::WINE_MONO.size)))?;
                    wine.run(&["msiexec".as_ref(), "/i".as_ref(), &dos_path(&msi), "/qn".as_ref()], INSTALL_TIMEOUT)?;
                }
                st.mark(prefix, "mono")?;
            }
            Verb::Vcrun2022 if !st.has("vcrun2022") => {
                for p in [engine::VCRUN_X64, engine::VCRUN_X86] {
                    let exe = engine::fetch_pinned(layout, &p, &mut |d, t| on("vcrun2022", d, t.unwrap_or(p.size)))?;
                    on("vcrun2022", 0, 0);
                    // The installer may end with 3010 (restart wanted), folded by Wine to 8 bits, and Wine's own
                    // builtin runtime stays as a fallback: only a hang stops the setup.
                    match wine.run(&[exe.as_os_str(), "/install".as_ref(), "/quiet".as_ref(), "/norestart".as_ref()], INSTALL_TIMEOUT) {
                        Err(e) if e.code == "timeout" => return Err(e),
                        Err(e) => super::log_line(&wine.log, &format!("{}: {} (kept going)", p.name, e.message)),
                        Ok(()) => {}
                    }
                }
                st.mark(prefix, "vcrun2022")?;
            }
            _ => {}
        }
    }
    if renderer && !st.has("renderer") {
        on("renderer", 0, 0);
        install_winemetal(&wine.root, prefix)?;
        st.mark(prefix, "renderer")?;
    }
    if !steam_exe(prefix).is_file() {
        on("steam", 0, 0);
        let setup = engine::fetch_pinned(layout, &engine::STEAM_SETUP, &mut |d, t| on("steam", d, t.unwrap_or(engine::STEAM_SETUP.size)))
            .or_else(|_| engine::fetch_file(layout, engine::STEAM_SETUP_MIRROR, engine::STEAM_SETUP.sha256, engine::STEAM_SETUP.size, &mut |_, _| {}))?;
        wine.run(&[setup.as_os_str(), "/S".as_ref()], INSTALL_TIMEOUT)?;
        if !steam_exe(prefix).is_file() {
            return Err(MacError::new("wine", "Steam's installer ended but steam.exe is missing"));
        }
        st.mark(prefix, "steam")?;
    }
    Ok(())
}

/// DXMT: `winemetal.dll` also goes into the prefix (`system32` 64-bit, `syswow64` 32-bit), from the engine's
/// `lib/wine/<arch>-windows` where `engine::install_renderer` put it.
pub fn install_winemetal(root: &Path, prefix: &Path) -> Result<(), MacError> {
    let win = prefix.join("drive_c").join("windows");
    let mut copied = 0;
    for (arch, sys) in [("x86_64-windows", "system32"), ("i386-windows", "syswow64")] {
        let src = root.join("lib").join("wine").join(arch).join("winemetal.dll");
        let dst_dir = win.join(sys);
        if src.is_file() && dst_dir.is_dir() {
            let dst = dst_dir.join("winemetal.dll");
            let _ = std::fs::remove_file(&dst);
            std::fs::copy(&src, &dst).map_err(|e| MacError::io(&dst, e))?;
            copied += 1;
        }
    }
    if copied == 0 {
        return Err(MacError::new("wine", "no winemetal.dll to install (renderer missing from the engine)"));
    }
    Ok(())
}

/// A Steam game in a prefix: its folder when Steam says it is fully installed (`StateFlags` 4) and the folder exists.
pub fn steam_game_dir(steam_root: &Path, appid: &str) -> Option<PathBuf> {
    let apps = steam_root.join("steamapps");
    let src = std::fs::read_to_string(apps.join(format!("appmanifest_{appid}.acf"))).ok()?;
    let v = crate::scan::vdf::parse(&src);
    let s = v.get("AppState")?;
    let flags: u32 = s.str("StateFlags")?.parse().ok()?;
    let dir = s.str("installdir").filter(|d| super::recipe::valid_rel_path(d) && !d.contains('/'))?;
    let dir = apps.join("common").join(dir);
    (flags & 4 != 0 && dir.is_dir()).then_some(dir)
}

/// The prefixes' Steam folders: `(game, <prefix>/drive_c/Program Files (x86)/Steam)` for each prefix that has one.
pub fn prefix_steams(layout: &Layout) -> Vec<(String, PathBuf)> {
    let Ok(dir) = std::fs::read_dir(layout.prefixes()) else { return vec![] };
    let mut out: Vec<(String, PathBuf)> = dir
        .flatten()
        .filter_map(|e| {
            let game = e.file_name().to_str()?.to_string();
            let root = steam_root(&e.path());
            (super::recipe::valid_game_id(&game) && root.join("steamapps").is_dir()).then_some((game, root))
        })
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn block() -> WineBlock {
        let mut env = BTreeMap::new();
        env.insert("DXMT_CONFIG_FILE".to_string(), "{prefix}/dxmt.conf".to_string());
        env.insert("WINEMSYNC".to_string(), "1".to_string());
        WineBlock {
            engine: "w".into(),
            renderer: "r".into(),
            store: super::super::recipe::Store::SteamWin,
            appid: "489830".into(),
            exe: "SkyrimSE.exe".into(),
            env,
            dll_overrides: "dxgi,d3d11=b".into(),
            args: vec![],
            verbs: vec![],
            online: super::super::recipe::Online::OfflineOnly,
        }
    }

    #[test]
    #[cfg(unix)] // Unix paths in the expected values; Play on Mac runs on macOS only
    fn env_has_the_core_keys_then_the_recipe() {
        let w = Wine::new("/e/wswine.bundle".into(), "/p/skyrim".into(), "/l/skyrim.log".into()).with_recipe(&block());
        let env: Vec<(String, String)> = w.env_pairs().into_iter().map(|(k, v)| (k.to_string_lossy().into(), v.to_string_lossy().into())).collect();
        let get = |k: &str| env.iter().find(|(x, _)| x == k).map(|(_, v)| v.as_str());
        assert_eq!(get("WINEPREFIX"), Some("/p/skyrim"));
        assert_eq!(get("WINESERVER"), Some("/e/wswine.bundle/bin/wineserver"));
        // The engines are wow64-only builds: no WINEARCH (Wine 10 refuses `wow64`, any value would be redundant).
        assert_eq!(get("WINEARCH"), None);
        assert_eq!(get("WINEDLLOVERRIDES"), Some("dxgi,d3d11=b"));
        assert_eq!(get("WINEDEBUG"), Some("-all"));
        assert_eq!(get("DXMT_CONFIG_FILE"), Some("/p/skyrim/dxmt.conf"));
        assert_eq!(get("WINEMSYNC"), Some("1"));
        assert_eq!(get("DYLD_FALLBACK_LIBRARY_PATH"), None);
        let w = w.with_libs(vec!["/e/T.app/Contents/Frameworks".into()]);
        let env: Vec<(String, String)> = w.env_pairs().into_iter().map(|(k, v)| (k.to_string_lossy().into(), v.to_string_lossy().into())).collect();
        let dyld = env.iter().find(|(k, _)| k == "DYLD_FALLBACK_LIBRARY_PATH").map(|(_, v)| v.as_str());
        assert_eq!(dyld, Some("/e/T.app/Contents/Frameworks:/usr/local/lib:/usr/lib"));
        let mut b = block();
        b.env.insert("WINEDEBUG".into(), "+loaddll".into());
        let w = Wine::new("/e".into(), "/p".into(), "/l".into()).with_recipe(&b);
        assert_eq!(w.env_pairs().iter().filter(|(k, _)| k == "WINEDEBUG").count(), 1);
    }

    #[test]
    fn paths() {
        assert_eq!(dos_path(Path::new("/Users/a/x y.msi")), OsString::from("Z:\\Users\\a\\x y.msi"));
        assert_eq!(steam_exe(Path::new("/p")), Path::new("/p/drive_c/Program Files (x86)/Steam/steam.exe"));
        assert_eq!(expand("{data}/x {engine}", &[("data", Path::new("/d")), ("engine", Path::new("/e"))]), "/d/x /e");
    }

    #[cfg(unix)]
    #[test]
    fn server_socket_path() {
        use std::os::unix::fs::MetadataExt;
        let t = tempfile::tempdir().unwrap();
        let m = std::fs::metadata(t.path()).unwrap();
        let s = server_socket(t.path()).unwrap();
        assert_eq!(s, PathBuf::from(format!("/tmp/.wine-{}/server-{:x}-{:x}/socket", m.uid(), m.dev(), m.ino())));
        assert!(!server_running(t.path()));
    }

    #[test]
    fn steam_games_in_a_prefix() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("Steam");
        let apps = root.join("steamapps");
        std::fs::create_dir_all(apps.join("common").join("Skyrim Special Edition")).unwrap();
        let acf = |flags: &str, dir: &str| format!("\"AppState\"\n{{\n\"appid\" \"489830\"\n\"StateFlags\" \"{flags}\"\n\"installdir\" \"{dir}\"\n}}\n");
        let m = apps.join("appmanifest_489830.acf");
        std::fs::write(&m, acf("4", "Skyrim Special Edition")).unwrap();
        assert_eq!(steam_game_dir(&root, "489830"), Some(apps.join("common").join("Skyrim Special Edition")));
        std::fs::write(&m, acf("1026", "Skyrim Special Edition")).unwrap();
        assert_eq!(steam_game_dir(&root, "489830"), None, "still downloading");
        std::fs::write(&m, acf("4", "../../..")).unwrap();
        assert_eq!(steam_game_dir(&root, "489830"), None);
        assert_eq!(steam_game_dir(&root, "1"), None);
    }

    #[test]
    fn prefix_state() {
        let t = tempfile::tempdir().unwrap();
        let mut st = PrefixState::load(t.path());
        assert!(!st.ready(false));
        st.mark(t.path(), "wineboot").unwrap();
        st.mark(t.path(), "wineboot").unwrap();
        let back = PrefixState::load(t.path());
        assert_eq!(back.steps, ["wineboot"]);
        assert!(back.ready(false) && !back.ready(true));
        install_winemetal(Path::new("/nonexistent"), t.path()).unwrap_err();
    }
}
