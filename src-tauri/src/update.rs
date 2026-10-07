//! In-app updates (tauri-plugin-updater, `plugins.updater` in tauri.conf.json). The core does it all: the webview gets
//! no updater or process permission, only the three commands below. The check asks GitHub for the latest release's
//! `latest.json`, and only once the privacy choices are answered (docs/PRIVACY.md). Nothing is downloaded or installed
//! without the player's click, and never while a game SIGF installed a mashup into is running, or while an install
//! runs. The plugin verifies the installer against the minisign public key in the config, and with
//! `requireSignedVersion` also checks that the signature was made for the version `latest.json` announces.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::Emitter;
use tauri_plugin_updater::UpdaterExt;

/// What the UI shows in its banner.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Available {
    pub version: String,
    pub current: String,
    pub notes: Option<String>,
}

/// `update://progress`: bytes so far and the total when the server gives one; `installing` once it is all here.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    got: u64,
    total: Option<u64>,
    installing: bool,
}

/// Why the core refused to update now (the UI shows `message`; `kind` is `gameRunning`, `busy`, `privacy`, `none`,
/// or `failed`).
#[derive(Debug, serde::Serialize)]
pub struct UpdateError {
    pub kind: &'static str,
    pub message: String,
}

impl UpdateError {
    fn new(kind: &'static str, message: impl Into<String>) -> Self {
        UpdateError { kind, message: message.into() }
    }
}

/// The update the last check found, kept for the install (the same `latest.json` answer the player saw).
static FOUND: Mutex<Option<tauri_plugin_updater::Update>> = Mutex::new(None);

/// Asks for the latest version. `None`: this one is up to date.
#[tauri::command]
pub async fn update_check(app: tauri::AppHandle) -> Result<Option<Available>, UpdateError> {
    if !crate::privacy::current().asked {
        return Err(UpdateError::new("privacy", "answer the privacy choices first"));
    }
    let found = app
        .updater()
        .map_err(|e| UpdateError::new("failed", e.to_string()))?
        .check()
        .await
        .map_err(|e| UpdateError::new("failed", e.to_string()))?;
    let info = found.as_ref().map(|u| Available { version: u.version.clone(), current: u.current_version.clone(), notes: u.body.clone() });
    *FOUND.lock().unwrap_or_else(|p| p.into_inner()) = found;
    Ok(info)
}

/// Whether an update has to wait. `game_dirs` is the UI's scanned `{game}` map (game id -> install folder, as for an
/// install): only the games of installed mashups count, with the install records' own game folders and Prism.
#[tauri::command]
pub fn update_blocked(game_dirs: HashMap<String, String>) -> Option<UpdateError> {
    blocker(&game_dirs)
}

fn blocker(game_dirs: &HashMap<String, String>) -> Option<UpdateError> {
    if crate::install_running() {
        return Some(UpdateError::new("busy", "Wait for the install to finish"));
    }
    let (folders, exes) = watched(game_dirs);
    if running_in(&process_paths(), &folders, &exes) {
        return Some(UpdateError::new("gameRunning", "Close your game first"));
    }
    None
}

/// Downloads the update the last check found, with `update://progress`, then hands it to its installer: the NSIS
/// installer runs in passive mode (a progress window, no questions), closes this app and starts the new version.
#[tauri::command]
pub async fn update_install(app: tauri::AppHandle, game_dirs: HashMap<String, String>) -> Result<(), UpdateError> {
    if let Some(e) = blocker(&game_dirs) {
        return Err(e);
    }
    let update = FOUND.lock().unwrap_or_else(|p| p.into_inner()).clone().ok_or_else(|| UpdateError::new("none", "no update to install"))?;
    let mut got = 0u64;
    let a = app.clone();
    let bytes = update
        .download(
            |n, total| {
                got += n as u64;
                let _ = a.emit("update://progress", Progress { got, total, installing: false });
            },
            || {},
        )
        .await
        .map_err(|e| UpdateError::new("failed", e.to_string()))?;
    // The download is checked against the public key above before this point; the game check again, as it took a while.
    if let Some(e) = blocker(&game_dirs) {
        return Err(e);
    }
    let _ = app.emit("update://progress", Progress { got, total: Some(got), installing: true });
    // On Windows this starts the installer and exits the process; elsewhere it returns and the app restarts itself.
    update.install(bytes).map_err(|e| UpdateError::new("failed", e.to_string()))?;
    app.restart();
}

/// Folders whose programs count as "your game", and single programs (Prism Launcher, which starts Minecraft).
fn watched(game_dirs: &HashMap<String, String>) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let (mut folders, mut exes) = (Vec::new(), Vec::new());
    for m in crate::install::Engine::from_env(None).installed() {
        for g in m.games {
            folders.extend(game_dirs.get(&g.game).map(PathBuf::from));
            folders.extend(g.game_dir.map(PathBuf::from));
            folders.extend(g.exe.map(|x| PathBuf::from(x.dir)));
            exes.extend(g.launcher.map(PathBuf::from));
        }
    }
    // A Steam library or game folder reached through a junction or symlink runs programs under its real path.
    let real = |v: &Vec<PathBuf>| v.iter().filter_map(|p| std::fs::canonicalize(p).ok()).collect::<Vec<_>>();
    let (rf, re) = (real(&folders), real(&exes));
    folders.extend(rf);
    exes.extend(re);
    (folders, exes)
}

/// Lower case, backslashes, no trailing separator: how Windows paths compare (macOS volumes are case-insensitive by
/// default too).
fn norm(p: &Path) -> String {
    let s = p.to_string_lossy().replace('/', "\\").to_lowercase();
    let s = s.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(s);
    s.trim_end_matches('\\').to_string()
}

/// Whether any running program is inside one of `folders` or is one of `exes`. A drive root or an empty folder never
/// counts (it would match everything).
pub fn running_in(processes: &[PathBuf], folders: &[PathBuf], exes: &[PathBuf]) -> bool {
    let folders: Vec<String> = folders.iter().map(|f| norm(f)).filter(|f| f.len() > 3 && f.contains('\\')).map(|f| f + "\\").collect();
    let exes: Vec<String> = exes.iter().map(|e| norm(e)).filter(|e| !e.is_empty()).collect();
    processes.iter().map(|p| norm(p)).any(|p| folders.iter().any(|f| p.starts_with(f.as_str())) || exes.contains(&p))
}

/// The full path of every program this user can see running. Windows: the process snapshot. macOS: `/bin/ps`, whose
/// `comm` column is the executable's full path there. Elsewhere none.
fn process_paths() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
        use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
        let mut out = Vec::new();
        // SAFETY: plain Win32 calls on handles this function opens and closes; buffers are sized and zeroed.
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE {
                return out;
            }
            let mut e: PROCESSENTRY32W = std::mem::zeroed();
            e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut ok = Process32FirstW(snap, &mut e) != 0;
            while ok {
                let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, e.th32ProcessID);
                if !h.is_null() {
                    let mut buf = [0u16; 1024];
                    let mut len = buf.len() as u32;
                    if QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len) != 0 {
                        out.push(PathBuf::from(std::ffi::OsString::from_wide(&buf[..len as usize])));
                    }
                    CloseHandle(h);
                }
                ok = Process32NextW(snap, &mut e) != 0;
            }
            CloseHandle(snap);
        }
        out
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("/bin/ps")
            .args(["-axo", "comm="])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).lines().map(str::trim).filter(|l| l.starts_with('/')).map(PathBuf::from).collect())
            .unwrap_or_default()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn game_folders_and_launchers() {
        let procs = [p(r"C:\Windows\explorer.exe"), p(r"D:\SteamLibrary\steamapps\common\Skyrim Special Edition\SkyrimSE.exe")];
        let skyrim = [p(r"d:/steamlibrary/steamapps/common/skyrim special edition/")];
        assert!(running_in(&procs, &skyrim, &[]));
        // A sibling folder with the same prefix is another game.
        assert!(!running_in(&procs, &[p(r"D:\SteamLibrary\steamapps\common\Skyrim")], &[]));
        assert!(!running_in(&procs, &[p(r"D:\SteamLibrary\steamapps\common\Fallout 4")], &[]));
        // Prism Launcher by its exe.
        let prism = [p(r"D:\Tools\PrismLauncher\prismlauncher.exe")];
        assert!(running_in(&[p(r"\\?\D:\Tools\PrismLauncher\PrismLauncher.exe")], &[], &prism));
        assert!(!running_in(&procs, &[], &prism));
    }

    #[test]
    fn roots_and_empty_folders_never_match() {
        let procs = [p(r"C:\Windows\explorer.exe")];
        for f in ["", r"C:\", "C:", "C:/", r"\"] {
            assert!(!running_in(&procs, &[p(f)], &[]), "{f:?}");
        }
        assert!(!running_in(&procs, &[], &[p("")]));
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn sees_this_test_process() {
        let me = std::env::current_exe().unwrap();
        let paths = process_paths();
        assert!(running_in(&paths, &[me.parent().unwrap().to_path_buf()], &[]), "{} not in the process list", me.display());
    }
}
