//! The toolchain a player build may use (docs/RECIPE-FORMAT.md section 4, "Player build"): a fixed table compiled into
//! the app, never named by a recipe beyond its ids. Each tool is one exact HTTPS download pinned by sha256 and size,
//! fetched once on the player's demand into `<SIGF_HOME>/tools/<id>/` and reused by every later build. Nothing is
//! installed system-wide and no installer runs: w64devkit is a self-extracting 7-Zip archive unpacked into that folder,
//! Python the official "embeddable package" zip.

use super::fetch::{fetch_pinned, Expected, FetchOpts};
use super::paths::path_string;
use super::{extract_zip, InstallError};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    /// A self-extracting 7-Zip archive (`7z.sfx`), unpacked by running it with `-y -o<dir>`.
    SevenZipSfx,
    /// A plain zip, unpacked by the app.
    Zip,
}

#[derive(Debug, Clone, Copy)]
pub struct Tool {
    /// What a recipe's `player_build[].toolchain` names.
    pub id: &'static str,
    pub label: &'static str,
    /// The one URL it is ever downloaded from.
    pub url: &'static str,
    pub sha256: &'static str,
    pub size: u64,
    pub kind: ToolKind,
    /// Folders (relative to the tool's folder) put on the build's PATH.
    pub path: &'static [&'static str],
    /// The shell that runs build scripts, when this tool provides one.
    pub shell: Option<&'static str>,
    /// A file that must exist once unpacked.
    pub check: &'static str,
    pub license: &'static str,
    pub page: &'static str,
}

/// The pinned toolchain. Same versions as other mashup apps use for player-side builds (w64devkit 2.10.0, Python
/// 3.12.10 embeddable). w64devkit's sha256 is GitHub's release digest; Python's matches python.org's published MD5.
pub const TOOLS: &[Tool] = &[
    Tool {
        id: "w64devkit-2.10.0",
        label: "w64devkit 2.10.0 (MinGW-w64 GCC, make, BusyBox sh)",
        url: "https://github.com/skeeto/w64devkit/releases/download/v2.10.0/w64devkit-x64-2.10.0.7z.exe",
        sha256: "18d0a4c71a166f8401ab6305781bec5882b40b5e06ba9807c61cb5f3b3c6325e",
        size: 67_127_496,
        kind: ToolKind::SevenZipSfx,
        path: &["w64devkit/bin"],
        shell: Some("w64devkit/bin/sh.exe"),
        check: "w64devkit/bin/gcc.exe",
        license: "Unlicense (w64devkit), GPL-3.0 with exceptions (GCC), GPL-2.0 (BusyBox)",
        page: "https://github.com/skeeto/w64devkit",
    },
    Tool {
        id: "python-3.12.10",
        label: "Python 3.12.10 (embeddable package)",
        url: "https://www.python.org/ftp/python/3.12.10/python-3.12.10-embed-amd64.zip",
        sha256: "4acbed6dd1c744b0376e3b1cf57ce906f9dc9e95e68824584c8099a63025a3c3",
        size: 11_133_606,
        kind: ToolKind::Zip,
        path: &[""],
        shell: None,
        check: "python.exe",
        license: "PSF-2.0",
        page: "https://www.python.org/downloads/release/python-31210/",
    },
];

pub fn find(id: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|t| t.id == id)
}

/// A tool ready to use: the folders for PATH and its shell, if any.
#[derive(Debug, Clone)]
pub struct Ready {
    pub path: Vec<PathBuf>,
    pub shell: Option<PathBuf>,
}

/// Marker written last into a tool's folder: the sha256 of the download it was unpacked from.
const MARKER: &str = ".sigf-tool";

pub fn tools_dir(home: &Path) -> PathBuf {
    home.join("tools")
}

/// The tool's folder when it is already unpacked from exactly its pinned download.
pub fn installed(home: &Path, t: &Tool) -> Option<Ready> {
    let dir = tools_dir(home).join(t.id);
    let ok = std::fs::read_to_string(dir.join(MARKER)).is_ok_and(|m| m.trim() == t.sha256) && dir.join(t.check).is_file();
    ok.then(|| ready(&dir, t))
}

fn ready(dir: &Path, t: &Tool) -> Ready {
    Ready {
        path: t.path.iter().map(|p| if p.is_empty() { dir.to_path_buf() } else { dir.join(p) }).collect(),
        shell: t.shell.map(|s| dir.join(s)),
    }
}

/// Makes sure the tool is unpacked under `<home>/tools/<id>/`: downloads its pinned file (hash and size checked),
/// unpacks it into a temp folder, checks it, then renames it into place. The download is deleted afterwards.
pub fn ensure(home: &Path, t: &Tool, on_bytes: &mut dyn FnMut(u64, Option<u64>)) -> Result<Ready, InstallError> {
    if let Some(r) = installed(home, t) {
        return Ok(r);
    }
    let root = tools_dir(home);
    let dir = root.join(t.id);
    let tmp = root.join(format!(".{}.tmp", t.id));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| InstallError::io(&tmp, e))?;
    let cache = home.join("cache");
    let got = fetch_pinned(&cache, t.url, &Expected::sha256(t.sha256)?, &FetchOpts::for_build(false, Some(t.size)), on_bytes)?;
    let r = unpack(&got.path, &tmp, t);
    let _ = std::fs::remove_file(&got.path);
    if let Err(e) = r {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(e);
    }
    if !tmp.join(t.check).is_file() {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(InstallError::io(&tmp.join(t.check), format!("{} did not unpack as expected", t.label)));
    }
    std::fs::write(tmp.join(MARKER), t.sha256).map_err(|e| InstallError::io(&tmp, e))?;
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::rename(&tmp, &dir).map_err(|e| {
        let _ = std::fs::remove_dir_all(&tmp);
        InstallError::io(&dir, e)
    })?;
    Ok(ready(&dir, t))
}

fn unpack(archive: &Path, into: &Path, t: &Tool) -> Result<(), InstallError> {
    match t.kind {
        ToolKind::Zip => extract_zip(archive, into).map(|_| ()),
        ToolKind::SevenZipSfx => {
            // The verified download is a PE file named by its hash: give it its extension to start it.
            let exe = into.with_extension("sfx.exe");
            std::fs::copy(archive, &exe).map_err(|e| InstallError::io(&exe, e))?;
            let mut cmd = std::process::Command::new(&exe);
            cmd.arg("-y").arg(format!("-o{}", path_string(into)));
            no_window(&mut cmd);
            let status = cmd.status().map_err(|e| InstallError::io(&exe, e));
            let _ = std::fs::remove_file(&exe);
            match status? {
                s if s.success() => Ok(()),
                s => Err(InstallError::io(into, format!("{} did not unpack (exit {s})", t.label))),
            }
        }
    }
}

/// Starts console programs without a console window (`CREATE_NO_WINDOW`).
pub fn no_window(cmd: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_pinned_and_allowlisted() {
        for t in TOOLS {
            assert_eq!(t.sha256.len(), 64, "{}", t.id);
            assert!(t.size > 0 && t.size <= super::super::check::MAX_FILE_BYTES);
            let u = super::super::check::canonical_https(t.url).expect(t.url);
            assert!(super::super::check::build_start_ok(&u), "{}", t.url);
            assert!(t.id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.'));
        }
        assert!(TOOLS.iter().any(|t| t.shell.is_some()));
        assert!(find("w64devkit-2.10.0").is_some() && find("gcc").is_none());
    }
}
