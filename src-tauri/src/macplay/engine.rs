//! Engines on disk: pinned downloads (through `install::fetch`, sha256-checked), unpacked with the system's own tools
//! (`/usr/bin/tar` reads .tar.xz, .tar.gz and .zip; `hdiutil` mounts a .dmg), checked for links that leave their
//! folder, freed of the quarantine flag, and for a Wine engine with a renderer, DXMT's files put in place
//! (docs/MAC-ENGINES.md section 3).

use super::recipe::{EngineKind, MacEngine};
use super::{Layout, MacError};
use crate::install::fetch;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

/// A file the core downloads itself, pinned here (not by the recipe): the webview can never swap these.
#[derive(Debug, Clone, Copy)]
pub struct Pinned {
    pub name: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub size: u64,
}

/// Valve's Windows Steam installer (unchanged since 2024-05-20). Never rehosted.
pub const STEAM_SETUP: Pinned = Pinned {
    name: "SteamSetup.exe",
    url: "https://cdn.cloudflare.steamstatic.com/client/installer/SteamSetup.exe",
    sha256: "7d3654531c32d941b8cae81c4137fc542172bfa9635f169cb392f245a0a12bcb",
    size: 2_380_800,
};
/// The same file from Valve's other CDN, tried when the first one fails.
pub const STEAM_SETUP_MIRROR: &str = "https://steamcdn-a.akamaihd.net/client/installer/SteamSetup.exe";

/// Wine Mono, for an engine that does not bundle one (`share/wine/mono`).
pub const WINE_MONO: Pinned = Pinned {
    name: "wine-mono-11.2.1-x86.msi",
    url: "https://github.com/wine-mono/wine-mono/releases/download/wine-mono-11.2.1/wine-mono-11.2.1-x86.msi",
    sha256: "e38810c017ccb64d450bbd6095d41c0b50e3fd967c306f6d2c64c3a48945cc92",
    size: 83_334_144,
};

/// Microsoft's Visual C++ 2015-2022 runtime 14.44.35211 (the `vcrun2022` verb), 64- and 32-bit.
pub const VCRUN_X64: Pinned = Pinned {
    name: "VC_redist.x64.exe",
    url: "https://download.visualstudio.microsoft.com/download/pr/bd1c8d9d-ba95-4eee-bc6e-df1fcc876373/CC0FF0EB1DC3F5188AE6300FAEF32BF5BEEBA4BDD6E8E445A9184072096B713B/VC_redist.x64.exe",
    sha256: "cc0ff0eb1dc3f5188ae6300faef32bf5beeba4bdd6e8e445a9184072096b713b",
    size: 25_635_768,
};
pub const VCRUN_X86: Pinned = Pinned {
    name: "VC_redist.x86.exe",
    url: "https://download.visualstudio.microsoft.com/download/pr/bd1c8d9d-ba95-4eee-bc6e-df1fcc876373/0C09F2611660441084CE0DF425C51C11E147E6447963C3690F97E0B25C55ED64/VC_redist.x86.exe",
    sha256: "0c09f2611660441084ce0df425c51c11e147e6447963c3690f97e0b25c55ed64",
    size: 13_953_392,
};

/// Where Play-on-Mac downloads may start and redirect: GitHub releases (and their asset hosts), Valve's installer
/// CDNs, Microsoft's download host (the pinned VC++ runtime).
pub const FETCH_HOSTS: &[&str] = &[
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
    "cdn.cloudflare.steamstatic.com",
    "steamcdn-a.akamaihd.net",
    "download.visualstudio.microsoft.com",
];

/// Written last into an engine folder: what was unpacked there, so a changed pin reads as `missing`.
pub const READY_FILE: &str = ".sigf-engine.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ready {
    pub engine_sha256: String,
    #[serde(default)]
    pub renderer_sha256: Option<String>,
    /// The Wine root inside the folder (the one holding `bin/wine` and `lib/wine`), `/`-separated; empty for a
    /// native engine.
    #[serde(default)]
    pub wine_root: String,
    /// A runtime's library folder (Sikarugir's `Contents/Frameworks`), `/`-separated.
    #[serde(default)]
    pub lib_dir: String,
}

/// The folder an engine (plus its renderer) lives in: `engines/<engine>` or `engines/<engine>+<renderer>`, since
/// DXMT replaces the engine's own d3d11/dxgi.
pub fn engine_dir(layout: &Layout, engine: &MacEngine, renderer: Option<&MacEngine>) -> PathBuf {
    match renderer {
        Some(r) => layout.engines().join(format!("{}+{}", engine.id, r.id)),
        None => layout.engines().join(&engine.id),
    }
}

/// The ready record of `dir` when it matches these pins.
pub fn ready(dir: &Path, engine: &MacEngine, renderer: Option<&MacEngine>) -> Option<Ready> {
    let r: Ready = serde_json::from_str(&std::fs::read_to_string(dir.join(READY_FILE)).ok()?).ok()?;
    (r.engine_sha256 == engine.sha256 && r.renderer_sha256.as_deref() == renderer.map(|r| r.sha256.as_str())).then_some(r)
}

/// The Wine root of a ready Wine engine folder.
pub fn wine_root(dir: &Path, r: &Ready) -> PathBuf {
    join_rel(dir, &r.wine_root)
}

fn join_rel(dir: &Path, rel: &str) -> PathBuf {
    rel.split('/').filter(|s| !s.is_empty() && *s != "..").fold(dir.to_path_buf(), |p, s| p.join(s))
}

fn rel_of(base: &Path, p: &Path) -> String {
    p.strip_prefix(base).unwrap_or(Path::new("")).components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
}

/// Downloads one pinned file into `downloads/` (sha256-checked, cached by hash). `on_bytes(done, total)`.
pub fn fetch_file(layout: &Layout, url: &str, sha256: &str, size: u64, on_bytes: &mut dyn FnMut(u64, Option<u64>)) -> Result<PathBuf, MacError> {
    let opts = fetch::FetchOpts::for_mod(false, Some(size), FETCH_HOSTS);
    fetch::fetch(&layout.downloads(), url, sha256, &opts, on_bytes).map(|f| f.path).map_err(MacError::from_install)
}

pub fn fetch_pinned(layout: &Layout, p: &Pinned, on_bytes: &mut dyn FnMut(u64, Option<u64>)) -> Result<PathBuf, MacError> {
    fetch_file(layout, p.url, p.sha256, p.size, on_bytes)
}

/// Runs a system tool with plain arguments (no shell), its output into `log`; an error names the tool.
pub fn tool(program: &str, args: &[&std::ffi::OsStr], log: &Path) -> Result<(), MacError> {
    let out = super::log_file(log)?;
    let err = out.try_clone().map_err(|e| MacError::io(log, e))?;
    let status = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err)
        .status()
        .map_err(|e| MacError::new("io", format!("{program}: {e}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(MacError::new("unpack", format!("{program} failed ({status})")))
    }
}

/// Unpacks `archive` into the empty folder `dest`: `/usr/bin/tar` (bsdtar: refuses `..` and absolute names, and
/// writing through links) for .tar.xz/.tar.gz/.tgz/.zip, `hdiutil` + `ditto` for a .dmg. Then drops links that
/// leave `dest` and the quarantine flag.
pub fn unpack(archive: &Path, url: &str, dest: &Path, exclude: &[&str], log: &Path) -> Result<(), MacError> {
    std::fs::create_dir_all(dest).map_err(|e| MacError::io(dest, e))?;
    if url.to_ascii_lowercase().ends_with(".dmg") {
        unpack_dmg(archive, dest, log)?;
    } else {
        let mut args: Vec<&std::ffi::OsStr> = vec!["-x".as_ref(), "-f".as_ref(), archive.as_os_str(), "-C".as_ref(), dest.as_os_str()];
        for x in exclude {
            args.push("--exclude".as_ref());
            args.push(x.as_ref());
        }
        tool("/usr/bin/tar", &args, log)?;
    }
    drop_escaping_links(dest)?;
    unquarantine(dest, log);
    Ok(())
}

/// Mounts the image read-only (no Finder window), copies its top-level entries with `ditto` (signatures kept), and
/// detaches it whatever happened. A license prompt gets `Y` on stdin.
fn unpack_dmg(dmg: &Path, dest: &Path, log: &Path) -> Result<(), MacError> {
    let mnt = dest.with_extension("mnt");
    let _ = std::fs::remove_dir_all(&mnt);
    std::fs::create_dir_all(&mnt).map_err(|e| MacError::io(&mnt, e))?;
    let out = super::log_file(log)?;
    let err = out.try_clone().map_err(|e| MacError::io(log, e))?;
    let mut child = Command::new("/usr/bin/hdiutil")
        .args(["attach", "-nobrowse", "-readonly", "-noautoopen", "-mountpoint"])
        .arg(&mnt)
        .arg(dmg)
        .stdin(Stdio::piped())
        .stdout(out)
        .stderr(err)
        .spawn()
        .map_err(|e| MacError::new("io", format!("hdiutil: {e}")))?;
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(b"Y\n");
    }
    let attached = child.wait().map_err(|e| MacError::new("io", format!("hdiutil: {e}")))?;
    if !attached.success() {
        let _ = std::fs::remove_dir(&mnt);
        return Err(MacError::new("unpack", format!("hdiutil attach failed ({attached})")));
    }
    let copied = (|| {
        for e in std::fs::read_dir(&mnt).map_err(|e| MacError::io(&mnt, e))?.flatten() {
            let name = e.file_name();
            // The usual `Applications` link and Finder's hidden files are not part of the engine.
            if name.to_string_lossy().starts_with('.') || e.file_type().map(|t| t.is_symlink()).unwrap_or(true) {
                continue;
            }
            tool("/usr/bin/ditto", &[e.path().as_os_str(), dest.join(&name).as_os_str()], log)?;
        }
        Ok(())
    })();
    let _ = tool("/usr/bin/hdiutil", &["detach".as_ref(), "-force".as_ref(), mnt.as_os_str()], log);
    let _ = std::fs::remove_dir(&mnt);
    copied
}

/// `xattr -dr com.apple.quarantine`: files the app downloads carry no quarantine flag, but an unpacked archive may
/// hand its own down. Best effort.
pub fn unquarantine(dir: &Path, log: &Path) {
    if cfg!(target_os = "macos") {
        let _ = tool("/usr/bin/xattr", &["-dr".as_ref(), "com.apple.quarantine".as_ref(), dir.as_os_str()], log);
    }
}

/// An ad-hoc signature (`codesign --force --sign -`) for files the core drops into an engine unsigned (DXMT's
/// `winemetal.so`) and for native apps whose signature does not verify (GeneralsX ships unsigned).
pub fn adhoc_sign(path: &Path, deep: bool, log: &Path) -> Result<(), MacError> {
    let mut args: Vec<&std::ffi::OsStr> = vec!["--force".as_ref(), "--sign".as_ref(), "-".as_ref()];
    if deep {
        args.push("--deep".as_ref());
    }
    args.push(path.as_os_str());
    tool("/usr/bin/codesign", &args, log)
}

/// Every `.app` directly in `dir` (or one level down) whose signature does not verify gets an ad-hoc one: arm64
/// code must be signed to run.
pub fn sign_apps(dir: &Path, log: &Path) -> Result<(), MacError> {
    let mut apps = vec![];
    for e in std::fs::read_dir(dir).map_err(|e| MacError::io(dir, e))?.flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "app") {
            apps.push(p);
        } else if p.is_dir() && !p.is_symlink() {
            apps.extend(std::fs::read_dir(&p).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "app")));
        }
    }
    for app in apps {
        if tool("/usr/bin/codesign", &["--verify".as_ref(), "--deep".as_ref(), app.as_os_str()], log).is_err() {
            adhoc_sign(&app, true, log)?;
        }
    }
    Ok(())
}

/// Where a link at `link` (inside `root`) pointing to `target` ends up, worked out on the text (nothing is
/// followed); None when it leaves `root` or is absolute.
pub fn link_inside(root: &Path, link: &Path, target: &Path) -> Option<PathBuf> {
    if target.is_absolute() {
        return None;
    }
    let rel = link.strip_prefix(root).ok()?.parent()?;
    let mut parts: Vec<std::ffi::OsString> = rel.components().map(|c| c.as_os_str().to_os_string()).collect();
    for c in target.components() {
        match c {
            Component::Normal(s) => parts.push(s.to_os_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop()?;
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(parts.iter().fold(root.to_path_buf(), |p, s| p.join(s)))
}

/// Removes every symbolic link under `root` whose target is absolute or leaves `root` (an archive must not reach
/// outside its folder). Links inside (Wine builds have some) stay.
pub fn drop_escaping_links(root: &Path) -> Result<(), MacError> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).map_err(|e| MacError::io(&dir, e))?.flatten() {
            let p = e.path();
            let Ok(t) = e.file_type() else { continue };
            if t.is_symlink() {
                let target = std::fs::read_link(&p).map_err(|e| MacError::io(&p, e))?;
                if link_inside(root, &p, &target).is_none() {
                    std::fs::remove_file(&p).map_err(|e| MacError::io(&p, e))?;
                }
            } else if t.is_dir() {
                stack.push(p);
            }
        }
    }
    Ok(())
}

/// The folder holding `bin/wine` and `lib/wine` (Sikarugir: `wswine.bundle`; Gcenx: `Wine*.app/Contents/Resources/wine`),
/// at most five levels down.
pub fn find_wine_root(dir: &Path) -> Option<PathBuf> {
    find_dir(dir, 5, &|d| d.join("bin").join("wine").is_file() && d.join("lib").join("wine").is_dir())
}

/// The folder holding DXMT's `x86_64-windows` (`v0.80/` in the release), at most three levels down.
pub fn find_renderer_root(dir: &Path) -> Option<PathBuf> {
    find_dir(dir, 3, &|d| d.join("x86_64-windows").is_dir())
}

fn find_dir(dir: &Path, depth: usize, ok: &dyn Fn(&Path) -> bool) -> Option<PathBuf> {
    if ok(dir) {
        return Some(dir.to_path_buf());
    }
    if depth == 0 {
        return None;
    }
    let mut subs: Vec<PathBuf> = std::fs::read_dir(dir).ok()?.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.path()).collect();
    subs.sort();
    subs.into_iter().find_map(|s| find_dir(&s, depth - 1, ok))
}

/// The renderer's architecture folders, copied over the engine's `lib/wine/<same>`.
pub const RENDERER_ARCHES: &[&str] = &["x86_64-unix", "x86_64-windows", "i386-windows"];

/// A DLL or library name a renderer may install: plain characters, `.dll` or `.so`.
fn renderer_file_ok(name: &str) -> bool {
    let low = name.to_ascii_lowercase();
    (low.ends_with(".dll") || low.ends_with(".so")) && name.len() <= 64 && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

/// Copies the renderer's files into the Wine root (DXMT's "builtin" install: `winemetal.so` into
/// `lib/wine/x86_64-unix`, its DLLs into `lib/wine/x86_64-windows` and `i386-windows`) and ad-hoc signs the copied
/// `.so` files. Returns the files copied, as `<arch>/<name>`.
pub fn install_renderer(renderer_root: &Path, wine_root: &Path, log: &Path) -> Result<Vec<String>, MacError> {
    let mut done = vec![];
    for arch in RENDERER_ARCHES {
        let src = renderer_root.join(arch);
        if !src.is_dir() {
            continue;
        }
        let dst = wine_root.join("lib").join("wine").join(arch);
        std::fs::create_dir_all(&dst).map_err(|e| MacError::io(&dst, e))?;
        for e in std::fs::read_dir(&src).map_err(|e| MacError::io(&src, e))?.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !e.file_type().is_ok_and(|t| t.is_file()) || !renderer_file_ok(&name) {
                continue;
            }
            let to = dst.join(&name);
            let _ = std::fs::remove_file(&to);
            std::fs::copy(e.path(), &to).map_err(|e| MacError::io(&to, e))?;
            if name.ends_with(".so") && cfg!(target_os = "macos") {
                adhoc_sign(&to, false, log)?;
            }
            done.push(format!("{arch}/{name}"));
        }
    }
    if !done.iter().any(|f| f.starts_with("x86_64-windows/")) {
        return Err(MacError::new("unpack", "the renderer archive has no x86_64-windows DLLs"));
    }
    Ok(done)
}

/// Makes the engine folder for a recipe: downloads and unpacks the engine (and the renderer over it), into a
/// `.part` folder renamed when complete. `on(step, done, total)` reports download bytes.
pub fn ensure(
    layout: &Layout,
    engine: &MacEngine,
    renderer: Option<&MacEngine>,
    log: &Path,
    on: &mut dyn FnMut(&str, u64, u64),
) -> Result<(PathBuf, Ready), MacError> {
    let dir = engine_dir(layout, engine, renderer);
    if let Some(r) = ready(&dir, engine, renderer) {
        return Ok((dir, r));
    }
    super::log_line(log, &format!("engine {}: download", engine.id));
    let archive = fetch_file(layout, &engine.url, &engine.sha256, engine.size, &mut |d, t| on("engine", d, t.unwrap_or(engine.size)))?;
    let r_archive = match renderer {
        Some(r) => {
            super::log_line(log, &format!("renderer {}: download", r.id));
            Some(fetch_file(layout, &r.url, &r.sha256, r.size, &mut |d, t| on("renderer", d, t.unwrap_or(r.size)))?)
        }
        None => None,
    };
    on("unpack", 0, 0);
    let part = dir.with_file_name(format!("{}.part", dir.file_name().and_then(|n| n.to_str()).unwrap_or("engine")));
    let _ = std::fs::remove_dir_all(&part);
    unpack(&archive, &engine.url, &part, &[], log)?;
    let mut ready = Ready { engine_sha256: engine.sha256.clone(), renderer_sha256: None, wine_root: String::new(), lib_dir: String::new() };
    if engine.kind == EngineKind::Wine {
        let root = find_wine_root(&part).ok_or_else(|| MacError::new("unpack", format!("{}: no bin/wine in the archive", engine.id)))?;
        ready.wine_root = rel_of(&part, &root);
        if let (Some(r), Some(ra)) = (renderer, &r_archive) {
            let rdir = layout.downloads().join(format!("{}.unpack", r.id));
            let _ = std::fs::remove_dir_all(&rdir);
            unpack(ra, &r.url, &rdir, &[], log)?;
            let rroot = find_renderer_root(&rdir).ok_or_else(|| MacError::new("unpack", format!("{}: no x86_64-windows folder", r.id)))?;
            let files = install_renderer(&rroot, &root, log)?;
            super::log_line(log, &format!("renderer {}: {}", r.id, files.join(", ")));
            let _ = std::fs::remove_dir_all(&rdir);
            ready.renderer_sha256 = Some(r.sha256.clone());
        }
    } else if cfg!(target_os = "macos") {
        sign_apps(&part, log)?;
    }
    let json = serde_json::to_string_pretty(&ready).unwrap_or_default();
    std::fs::write(part.join(READY_FILE), json).map_err(|e| MacError::io(&part, e))?;
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::rename(&part, &dir).map_err(|e| MacError::io(&dir, e))?;
    // The archives are not needed once unpacked (a Wine build is ~170 MB).
    let _ = std::fs::remove_file(&archive);
    if let Some(ra) = r_archive {
        let _ = std::fs::remove_file(ra);
    }
    Ok((dir, ready))
}

/// Never unpacked from a runtime archive: Sikarugir's Template carries renderers, Apple's D3DMetal among them
/// (proprietary, docs/MAC-PLAY-RESEARCH.md section 5). DXMT comes from its own pinned release.
pub const RUNTIME_EXCLUDE: &[&str] = &["*/Frameworks/renderer"];
/// Folder names removed from a runtime after unpacking, in case an archive puts them elsewhere.
const RUNTIME_DROP: &[&str] = &["renderer", "d3dmetal", "apple_gptk", "D3DMetal.framework"];

/// Deletes every folder under `root` named one of `names` (not following links).
pub fn drop_dirs(root: &Path, names: &[&str]) -> Result<(), MacError> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).map_err(|e| MacError::io(&dir, e))?.flatten() {
            if !e.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            if names.iter().any(|n| e.file_name().to_string_lossy() == *n) {
                std::fs::remove_dir_all(e.path()).map_err(|x| MacError::io(&e.path(), x))?;
            } else {
                stack.push(e.path());
            }
        }
    }
    Ok(())
}

/// The runtime's library folder: a `Frameworks` folder holding `.dylib` files, at most four levels down.
pub fn find_lib_dir(dir: &Path) -> Option<PathBuf> {
    find_dir(dir, 4, &|d| {
        d.file_name().is_some_and(|n| n == "Frameworks")
            && std::fs::read_dir(d).is_ok_and(|rd| rd.flatten().any(|e| e.file_name().to_string_lossy().ends_with(".dylib")))
    })
}

/// A runtime (`kind: runtime`) in `engines/<id>/`, without its renderers; returns its library folder.
pub fn ensure_runtime(layout: &Layout, rt: &MacEngine, log: &Path, on: &mut dyn FnMut(&str, u64, u64)) -> Result<PathBuf, MacError> {
    let dir = engine_dir(layout, rt, None);
    if let Some(r) = ready(&dir, rt, None) {
        return Ok(join_rel(&dir, &r.lib_dir));
    }
    super::log_line(log, &format!("runtime {}: download", rt.id));
    let archive = fetch_file(layout, &rt.url, &rt.sha256, rt.size, &mut |d, t| on("runtime", d, t.unwrap_or(rt.size)))?;
    on("unpack", 0, 0);
    let part = dir.with_file_name(format!("{}.part", rt.id));
    let _ = std::fs::remove_dir_all(&part);
    unpack(&archive, &rt.url, &part, RUNTIME_EXCLUDE, log)?;
    drop_dirs(&part, RUNTIME_DROP)?;
    let lib = find_lib_dir(&part).ok_or_else(|| MacError::new("unpack", format!("{}: no Frameworks folder with libraries", rt.id)))?;
    let ready = Ready { engine_sha256: rt.sha256.clone(), renderer_sha256: None, wine_root: String::new(), lib_dir: rel_of(&part, &lib) };
    std::fs::write(part.join(READY_FILE), serde_json::to_string_pretty(&ready).unwrap_or_default()).map_err(|e| MacError::io(&part, e))?;
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::rename(&part, &dir).map_err(|e| MacError::io(&dir, e))?;
    let _ = std::fs::remove_file(&archive);
    Ok(join_rel(&dir, &ready.lib_dir))
}

/// The library folder of a ready runtime.
pub fn runtime_lib(layout: &Layout, rt: &MacEngine) -> Option<PathBuf> {
    let dir = engine_dir(layout, rt, None);
    ready(&dir, rt, None).map(|r| join_rel(&dir, &r.lib_dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_judged_on_their_text() {
        let root = Path::new("/r");
        let at = Path::new("/r/a/b/link");
        assert_eq!(link_inside(root, at, Path::new("../c")), Some(PathBuf::from("/r/a/c")));
        assert_eq!(link_inside(root, at, Path::new("../../c")), Some(PathBuf::from("/r/c")));
        assert_eq!(link_inside(root, at, Path::new("./x/./y")), Some(PathBuf::from("/r/a/b/x/y")));
        assert_eq!(link_inside(root, at, Path::new("../../../etc/passwd")), None);
        assert_eq!(link_inside(root, at, Path::new("/etc/passwd")), None);
        assert_eq!(link_inside(root, Path::new("/r/link"), Path::new("..")), None);
    }

    #[cfg(unix)]
    #[test]
    fn escaping_links_are_dropped() {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        std::fs::create_dir_all(r.join("lib/wine")).unwrap();
        std::fs::write(r.join("lib/wine/real.so"), b"x").unwrap();
        std::os::unix::fs::symlink("real.so", r.join("lib/wine/inside.so")).unwrap();
        std::os::unix::fs::symlink("../../../outside", r.join("lib/wine/up")).unwrap();
        std::os::unix::fs::symlink("/Applications", r.join("Applications")).unwrap();
        drop_escaping_links(r).unwrap();
        assert!(r.join("lib/wine/inside.so").is_symlink());
        assert!(!r.join("lib/wine/up").is_symlink());
        assert!(!r.join("Applications").is_symlink());
        assert!(r.join("lib/wine/real.so").is_file());
    }

    #[test]
    fn finds_roots_and_installs_the_renderer() {
        let t = tempfile::tempdir().unwrap();
        let eng = t.path().join("engine");
        let root = eng.join("wswine.bundle");
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::create_dir_all(root.join("lib/wine/x86_64-windows")).unwrap();
        std::fs::write(root.join("bin/wine"), b"").unwrap();
        std::fs::write(root.join("lib/wine/x86_64-windows/d3d11.dll"), b"wined3d").unwrap();
        assert_eq!(find_wine_root(&eng), Some(root.clone()));
        assert_eq!(find_wine_root(&t.path().join("nothing")), None);

        let rd = t.path().join("dxmt");
        for (arch, f) in [("x86_64-windows", "d3d11.dll"), ("x86_64-windows", "winemetal.dll"), ("i386-windows", "dxgi.dll"), ("x86_64-windows", "evil name.dll"), ("x86_64-windows", "run.sh")] {
            std::fs::create_dir_all(rd.join("v0.80").join(arch)).unwrap();
            std::fs::write(rd.join("v0.80").join(arch).join(f), b"dxmt").unwrap();
        }
        let rroot = find_renderer_root(&rd).unwrap();
        assert_eq!(rroot, rd.join("v0.80"));
        let mut files = install_renderer(&rroot, &root, Path::new("/dev/null")).unwrap();
        files.sort();
        assert_eq!(files, ["i386-windows/dxgi.dll", "x86_64-windows/d3d11.dll", "x86_64-windows/winemetal.dll"]);
        assert_eq!(std::fs::read(root.join("lib/wine/x86_64-windows/d3d11.dll")).unwrap(), b"dxmt");
        assert!(!root.join("lib/wine/x86_64-windows/run.sh").exists());
        // A renderer with no 64-bit DLLs is refused.
        let empty = t.path().join("empty/x86_64-unix");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(install_renderer(&t.path().join("empty"), &root, Path::new("/dev/null")).is_err());
    }

    #[test]
    fn ready_matches_the_pins() {
        let t = tempfile::tempdir().unwrap();
        let e = MacEngine { id: "w".into(), url: String::new(), sha256: "a".repeat(64), size: 1, kind: EngineKind::Wine, name: None, version: None, needs: None };
        let r = MacEngine { id: "r".into(), url: String::new(), sha256: "b".repeat(64), size: 1, kind: EngineKind::Renderer, name: None, version: None, needs: None };
        let rec = Ready { engine_sha256: e.sha256.clone(), renderer_sha256: Some(r.sha256.clone()), wine_root: "wswine.bundle".into(), lib_dir: String::new() };
        std::fs::write(t.path().join(READY_FILE), serde_json::to_string(&rec).unwrap()).unwrap();
        assert_eq!(ready(t.path(), &e, Some(&r)), Some(rec.clone()));
        assert_eq!(ready(t.path(), &e, None), None);
        let e2 = MacEngine { sha256: "c".repeat(64), ..e.clone() };
        assert_eq!(ready(t.path(), &e2, Some(&r)), None);
        assert_eq!(wine_root(t.path(), &rec), t.path().join("wswine.bundle"));
    }

    #[test]
    fn runtime_without_renderers() {
        let t = tempfile::tempdir().unwrap();
        let c = t.path().join("Template-1.0.21.app/Contents");
        for d in ["Frameworks/renderer/d3dmetal/external", "Frameworks/renderer/dxmt", "Frameworks/GStreamer.framework", "Resources/apple_gptk"] {
            std::fs::create_dir_all(c.join(d)).unwrap();
        }
        std::fs::write(c.join("Frameworks/libinotify.0.dylib"), b"x").unwrap();
        std::fs::write(c.join("Frameworks/renderer/d3dmetal/external/libd3dshared.dylib"), b"x").unwrap();
        drop_dirs(t.path(), RUNTIME_DROP).unwrap();
        assert!(!c.join("Frameworks/renderer").exists() && !c.join("Resources/apple_gptk").exists());
        assert!(c.join("Frameworks/GStreamer.framework").is_dir());
        assert_eq!(find_lib_dir(t.path()), Some(c.join("Frameworks")));
        assert_eq!(rel_of(t.path(), &c.join("Frameworks")), "Template-1.0.21.app/Contents/Frameworks");
        assert_eq!(join_rel(t.path(), "a/../b"), t.path().join("a").join("b"));
    }

    #[test]
    fn pins_are_well_formed() {
        for p in [STEAM_SETUP, WINE_MONO, VCRUN_X64, VCRUN_X86] {
            assert!(crate::install::check::mod_url_ok(FETCH_HOSTS, p.url), "{}", p.url);
            assert!(fetch::Expected::sha256(p.sha256).is_ok());
            assert!(p.size > 0);
        }
        assert!(crate::install::check::mod_url_ok(FETCH_HOSTS, STEAM_SETUP_MIRROR));
        for h in super::super::recipe::ENGINE_HOSTS {
            assert!(FETCH_HOSTS.contains(h));
        }
    }
}
