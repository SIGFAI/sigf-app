//! Native ports (VCMI, GeneralsX, OpenMW, DevilutionX...): the game's data files copied from the player's own copy
//! into `ports/<game>/data` (or the recipe's `data.into`), the config files the engine reads written next to them, and
//! the engine's Mac program started with them.
//!
//! Readiness: the default folder carries `.sigf-data.json` itself (written last, then the `.part` folder is renamed).
//! A `data.into` folder may be shared with the engine's own files (settings, saves), so its record is
//! `ports/<game>/.sigf-data.json`: the folder it was copied into and every file the copy wrote there, which is all
//! `mac_remove` deletes.

use super::recipe::{split_rooted, valid_rel_path, DataFrom, PortBlock, Root};
use super::{Layout, MacError};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// Written last into the data folder (or `ports/<game>/` for a `data.into` copy): the copy is complete.
pub const DATA_FILE: &str = ".sigf-data.json";
/// Suffix of a file or folder being written, renamed into place when complete.
const PART: &str = ".sigf-part";

/// The folders a port's paths start from: the engine folder and the user's `~/Library` (macOS only; elsewhere a
/// `{appsupport}` / `{prefs}` path is refused).
#[derive(Debug, Clone)]
pub struct Dirs {
    pub engine: PathBuf,
    pub library: Option<PathBuf>,
}

impl Dirs {
    pub fn user(engine: PathBuf) -> Self {
        let library = cfg!(target_os = "macos").then(crate::install::user_home).flatten().map(|h| h.join("Library"));
        Self { engine, library }
    }
    pub fn app_support(&self) -> Option<PathBuf> {
        self.library.as_ref().map(|l| l.join("Application Support"))
    }
    pub fn prefs(&self) -> Option<PathBuf> {
        self.library.as_ref().map(|l| l.join("Preferences"))
    }
    /// `{data}`, `{engine}`, `{appsupport}`, `{prefs}` for `wine::expand` (the last two only where they exist).
    fn vars(&self, data: &Path) -> Vec<(&'static str, PathBuf)> {
        let mut v = vec![("data", data.to_path_buf()), ("engine", self.engine.clone())];
        v.extend(self.app_support().map(|p| ("appsupport", p)));
        v.extend(self.prefs().map(|p| ("prefs", p)));
        v
    }
}

fn expand(s: &str, vars: &[(&'static str, PathBuf)]) -> String {
    let v: Vec<(&str, &Path)> = vars.iter().map(|(k, p)| (*k, p.as_path())).collect();
    super::wine::expand(s, &v)
}

/// `a/b/` as a relative path.
fn rel_path(f: &str) -> PathBuf {
    f.trim_end_matches('/').split('/').fold(PathBuf::new(), |p, s| p.join(s))
}

/// `p` with `.sigf-part` added to its name.
fn part_of(p: &Path) -> PathBuf {
    let mut n = p.file_name().unwrap_or_default().to_os_string();
    n.push(PART);
    p.with_file_name(n)
}

/// `p` (which may not exist yet) stays under `root` once links are followed: its deepest existing ancestor resolves
/// inside `root`. A missing root has nothing under it, so anything created there stays inside.
pub fn stays_inside(root: &Path, p: &Path) -> bool {
    let Ok(base) = root.canonicalize() else { return std::fs::symlink_metadata(root).is_err() && p.starts_with(root) };
    let mut a = p;
    while std::fs::symlink_metadata(a).is_err() {
        match a.parent() {
            Some(x) => a = x,
            None => return false,
        }
    }
    a.canonicalize().is_ok_and(|r| r.starts_with(&base))
}

/// A recipe path (`{appsupport}/...`, `{engine}/...`, `{data}/...`) on disk: its root and the full path, refused when an
/// existing link on the way leads out of the root.
pub fn rooted(dirs: &Dirs, data: &Path, s: &str) -> Result<(PathBuf, PathBuf), MacError> {
    let (r, rel) = split_rooted(s).ok_or_else(|| MacError::new("badRecipe", format!("bad port path {s}")))?;
    let lib = || MacError::new("unsupported", format!("{s}: no user Library on this system"));
    let root = match r {
        Root::AppSupport => dirs.app_support().ok_or_else(lib)?,
        Root::Prefs => dirs.prefs().ok_or_else(lib)?,
        Root::Engine => dirs.engine.clone(),
        Root::Data => data.to_path_buf(),
    };
    let p = if rel.is_empty() { root.clone() } else { root.join(rel_path(rel)) };
    if !stays_inside(&root, &p) {
        return Err(MacError::new("badRecipe", format!("{s} leads out of its folder through a link")));
    }
    Ok((root, p))
}

/// The default data folder, `ports/<game>/data`.
pub fn data_dir(layout: &Layout, game: &str) -> PathBuf {
    layout.port(game).join("data")
}

/// Where the recipe's data goes: `data.into` resolved, else `ports/<game>/data`. `{data}` expands to it.
pub fn data_target(layout: &Layout, game: &str, port: &PortBlock, dirs: &Dirs) -> Result<PathBuf, MacError> {
    let default = data_dir(layout, game);
    match port.data.into.as_deref() {
        None => Ok(default),
        Some(i) => rooted(dirs, &default, i).map(|(_, p)| p),
    }
}

/// `.sigf-data.json`: what a copy did. `into` and `copied` only for a `data.into` copy.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Record {
    #[serde(default)]
    from: String,
    #[serde(default)]
    files: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    into: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    copied: Vec<String>,
    #[serde(default)]
    bytes: u64,
}

/// The record of the last `data.into` copy, when it was for `target` (the record is on disk, not trusted more than
/// that: its paths are checked again before use).
fn into_record(layout: &Layout, game: &str, target: &Path) -> Option<Record> {
    let r: Record = serde_json::from_str(&std::fs::read_to_string(layout.port(game).join(DATA_FILE)).ok()?).ok()?;
    (r.into.as_deref() == Some(&*target.to_string_lossy())).then_some(r)
}

/// The data is there: nothing to copy (`from: none`), copied by the core, or one of the recipe's markers exists in
/// the user's `Library/Application Support` (where VCMI keeps what it imported).
pub fn data_ready(layout: &Layout, game: &str, port: &PortBlock, dirs: &Dirs) -> bool {
    if port.data.from == DataFrom::None {
        return true;
    }
    let copied = match port.data.into {
        None => data_dir(layout, game).join(DATA_FILE).is_file(),
        Some(_) => data_target(layout, game, port, dirs).ok().is_some_and(|d| {
            into_record(layout, game, &d).is_some_and(|r| d.is_dir() && r.copied.first().filter(|f| valid_rel_path(f)).is_none_or(|f| d.join(rel_path(f)).exists()))
        }),
    };
    copied || dirs.app_support().is_some_and(|b| markers_present(&b, &port.data.markers))
}

/// Bytes a `data.into` copy put outside `ports/<game>` (0 otherwise), for the status's disk use.
pub fn into_bytes(layout: &Layout, game: &str, port: &PortBlock, dirs: &Dirs) -> u64 {
    if port.data.into.is_none() {
        return 0;
    }
    data_target(layout, game, port, dirs).ok().and_then(|d| into_record(layout, game, &d)).map_or(0, |r| r.bytes)
}

pub fn markers_present(base: &Path, markers: &[String]) -> bool {
    markers.iter().filter(|m| valid_rel_path(m)).any(|m| base.join(rel_path(m)).exists())
}

/// `rel` under `base`, each segment the folder's entry of that exact name, else the one equal ignoring case (Windows
/// data: `DIABDAT.MPQ` vs `diabdat.mpq`). The path on disk and `rel` spelled as on disk.
pub fn resolve_ci(base: &Path, rel: &str) -> Option<(PathBuf, String)> {
    let mut p = base.to_path_buf();
    let mut out: Vec<String> = vec![];
    for seg in rel.trim_end_matches('/').split('/') {
        let names: Vec<String> = std::fs::read_dir(&p).ok()?.flatten().filter_map(|e| e.file_name().into_string().ok()).collect();
        let want = seg.to_lowercase();
        let name = names.iter().find(|n| *n == seg).or_else(|| names.iter().filter(|n| n.to_lowercase() == want).min())?.clone();
        p.push(&name);
        out.push(name);
    }
    Some((p, out.join("/")))
}

/// Copies `src` into `dst` (files and folders; links are skipped, so nothing outside the copy is read).
pub fn copy_tree(src: &Path, dst: &Path) -> Result<u64, MacError> {
    copy_entry(src, dst, "", &mut vec![], None)
}

/// `copy_tree`, adding each file's path (`rel` and below, `/`-separated) to `copied`. `merge`: `dst` is in a folder
/// shared with other files, which every write must stay inside: each file is written to `<name>.sigf-part` and renamed
/// over its place.
fn copy_entry(src: &Path, dst: &Path, rel: &str, copied: &mut Vec<String>, merge: Option<&Path>) -> Result<u64, MacError> {
    let meta = std::fs::symlink_metadata(src).map_err(|e| MacError::io(src, e))?;
    if meta.file_type().is_symlink() {
        return Ok(0);
    }
    if let Some(root) = merge {
        if !stays_inside(root, dst) {
            return Err(MacError::new("io", format!("{} leads out of {} through a link", dst.display(), root.display())));
        }
    }
    if meta.is_file() {
        if let Some(p) = dst.parent() {
            std::fs::create_dir_all(p).map_err(|e| MacError::io(p, e))?;
        }
        let n = if merge.is_some() {
            if std::fs::symlink_metadata(dst).is_ok_and(|m| m.is_dir()) {
                return Err(MacError::new("io", format!("{} is a folder", dst.display())));
            }
            let tmp = part_of(dst);
            let n = std::fs::copy(src, &tmp).map_err(|e| MacError::io(&tmp, e))?;
            std::fs::rename(&tmp, dst).map_err(|e| MacError::io(dst, e))?;
            n
        } else {
            std::fs::copy(src, dst).map_err(|e| MacError::io(dst, e))?
        };
        if !rel.is_empty() {
            copied.push(rel.to_string());
        }
        return Ok(n);
    }
    std::fs::create_dir_all(dst).map_err(|e| MacError::io(dst, e))?;
    let mut n = 0;
    for e in std::fs::read_dir(src).map_err(|e| MacError::io(src, e))?.flatten() {
        let name = e.file_name();
        let sub = if rel.is_empty() { name.to_string_lossy().into_owned() } else { format!("{rel}/{}", name.to_string_lossy()) };
        n += copy_entry(&e.path(), &dst.join(&name), &sub, copied, merge)?;
    }
    Ok(n)
}

/// What a copy takes from the game folder `src`: each entry's path and its name relative to `src` (as on disk).
/// `["."]`: everything in it. A missing `files` entry is a `dataMissing` error naming them all; `optional` ones are
/// skipped.
fn sources(port: &PortBlock, src: &Path) -> Result<Vec<(PathBuf, String)>, MacError> {
    let d = &port.data;
    if d.files.len() == 1 && d.files[0] == "." {
        let rd = std::fs::read_dir(src).map_err(|e| MacError::new("dataMissing", format!("{}: {e}", src.display())))?;
        return Ok(rd.flatten().map(|e| (e.path(), e.file_name().to_string_lossy().into_owned())).collect());
    }
    let mut out: Vec<(PathBuf, String)> = vec![];
    let mut missing = vec![];
    for f in &d.files {
        match resolve_ci(src, f) {
            Some(x) => out.push(x),
            None => missing.push(f.as_str()),
        }
    }
    if !missing.is_empty() {
        return Err(MacError::new("dataMissing", format!("{} has no {}", src.display(), missing.join(", "))));
    }
    out.extend(d.optional.iter().filter_map(|f| resolve_ci(src, f)));
    let mut seen = std::collections::HashSet::new();
    out.retain(|(_, r)| seen.insert(r.clone()));
    Ok(out)
}

/// Copies the recipe's data from the game folder `src` to `data_target`, then writes the config files. The default
/// folder (and a `data.into` folder that is missing or empty) is filled through a `.sigf-part` folder renamed into
/// place; a `data.into` folder with other files in it gets each file renamed into place, the others left alone.
/// `from: none`: nothing to copy.
pub fn copy_data(layout: &Layout, game: &str, port: &PortBlock, dirs: &Dirs, src: &Path) -> Result<u64, MacError> {
    if port.data.from == DataFrom::None {
        return Ok(0);
    }
    // Into the engine folder: only once the engine is unpacked (its setup replaces a folder it did not finish).
    if port.data.into.as_deref().and_then(split_rooted).is_some_and(|(r, _)| r == Root::Engine) && !dirs.engine.join(super::engine::READY_FILE).is_file() {
        return Err(MacError::new("notReady", "the engine is not downloaded: run the setup"));
    }
    let entries = sources(port, src)?;
    let dst = data_target(layout, game, port, dirs)?;
    let mut copied = vec![];
    let rec = |copied: Vec<String>, bytes: u64, into: bool| Record {
        from: src.to_string_lossy().into_owned(),
        files: port.data.files.clone(),
        into: into.then(|| dst.to_string_lossy().into_owned()),
        copied,
        bytes,
    };
    let fill = |dir: &Path, copied: &mut Vec<String>, merge: Option<&Path>| -> Result<u64, MacError> {
        let mut n = 0;
        for (from, rel) in &entries {
            n += copy_entry(from, &dir.join(rel_path(rel)), rel, copied, merge)?;
        }
        Ok(n)
    };
    let bytes = if port.data.into.is_none() {
        let part = dst.with_extension("part");
        let _ = std::fs::remove_dir_all(&part);
        std::fs::create_dir_all(&part).map_err(|e| MacError::io(&part, e))?;
        let bytes = fill(&part, &mut copied, None)?;
        std::fs::write(part.join(DATA_FILE), serde_json::to_string(&rec(vec![], bytes, false)).unwrap_or_default()).map_err(|e| MacError::io(&part, e))?;
        let _ = std::fs::remove_dir_all(&dst);
        std::fs::rename(&part, &dst).map_err(|e| MacError::io(&dst, e))?;
        bytes
    } else {
        // The last copy's files go first (this one replaces it), and a default folder from an older recipe.
        remove_data(layout, game, port, dirs);
        let _ = std::fs::remove_dir_all(data_dir(layout, game));
        let empty = std::fs::read_dir(&dst).map_or(true, |mut rd| rd.next().is_none());
        let bytes = if empty && std::fs::symlink_metadata(&dst).map_or(true, |m| m.is_dir()) {
            let part = part_of(&dst);
            let _ = std::fs::remove_dir_all(&part);
            if let Some(p) = dst.parent() {
                std::fs::create_dir_all(p).map_err(|e| MacError::io(p, e))?;
            }
            std::fs::create_dir_all(&part).map_err(|e| MacError::io(&part, e))?;
            let bytes = fill(&part, &mut copied, None)?;
            let _ = std::fs::remove_dir(&dst);
            std::fs::rename(&part, &dst).map_err(|e| MacError::io(&dst, e))?;
            bytes
        } else {
            fill(&dst, &mut copied, Some(&dst))?
        };
        let p = layout.port(game);
        std::fs::create_dir_all(&p).map_err(|e| MacError::io(&p, e))?;
        std::fs::write(p.join(DATA_FILE), serde_json::to_string(&rec(copied, bytes, true)).unwrap_or_default()).map_err(|e| MacError::io(&p, e))?;
        bytes
    };
    write_config(port, dirs, &dst)?;
    Ok(bytes)
}

/// Removes what the last copy put in a `data.into` folder: the files its record lists (each checked again: a plain
/// path inside the folder, not a folder), the folders they leave empty, the folder itself when empty, and the record.
/// Nothing else in that folder is touched. The default folder goes with `ports/<game>` instead.
pub fn remove_data(layout: &Layout, game: &str, port: &PortBlock, dirs: &Dirs) {
    if port.data.into.is_none() {
        return;
    }
    let Ok(dst) = data_target(layout, game, port, dirs) else { return };
    let Some(r) = into_record(layout, game, &dst) else { return };
    for f in r.copied.iter().filter(|f| valid_rel_path(f)) {
        let p = dst.join(rel_path(f));
        if !stays_inside(&dst, &p) || !std::fs::symlink_metadata(&p).is_ok_and(|m| !m.is_dir()) {
            continue;
        }
        let _ = std::fs::remove_file(&p);
        let mut d = p.parent();
        while let Some(x) = d.filter(|x| *x != dst && x.starts_with(&dst)) {
            if std::fs::remove_dir(x).is_err() {
                break;
            }
            d = x.parent();
        }
    }
    let _ = std::fs::remove_dir(&dst);
    let _ = std::fs::remove_file(layout.port(game).join(DATA_FILE));
}

/// Writes the recipe's `config` files (after a data copy and before each launch), parent folders created, `{data}`,
/// `{engine}`, `{appsupport}` and `{prefs}` expanded in the text. `keep`: only when the file is missing (the player's
/// settings stay); otherwise replaced through `<name>.sigf-part`. An existing link or folder in the file's place is
/// refused.
pub fn write_config(port: &PortBlock, dirs: &Dirs, data: &Path) -> Result<(), MacError> {
    let vars = dirs.vars(data);
    for c in &port.config {
        let (_, path) = rooted(dirs, data, &c.path)?;
        match std::fs::symlink_metadata(&path) {
            Ok(m) if m.file_type().is_symlink() || m.is_dir() => {
                return Err(MacError::new("io", format!("{} is a link or a folder, not written", path.display())));
            }
            Ok(_) if c.keep => continue,
            _ => {}
        }
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).map_err(|e| MacError::io(p, e))?;
        }
        let tmp = part_of(&path);
        std::fs::write(&tmp, expand(&c.text, &vars)).map_err(|e| MacError::io(&tmp, e))?;
        std::fs::rename(&tmp, &path).map_err(|e| MacError::io(&path, e))?;
    }
    Ok(())
}

/// Removes the config files the core replaces each launch under `~/Library` (those in the engine or data folder go
/// with them; `keep` ones are the player's settings and stay).
pub fn remove_config(port: &PortBlock, dirs: &Dirs, data: &Path) {
    for c in port.config.iter().filter(|c| !c.keep && split_rooted(&c.path).is_some_and(|(r, _)| matches!(r, Root::AppSupport | Root::Prefs))) {
        if let Ok((_, p)) = rooted(dirs, data, &c.path) {
            if std::fs::symlink_metadata(&p).is_ok_and(|m| m.is_file()) {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
}

/// `launch[0]` inside the engine folder, resolved (links followed) and still inside it, a file.
pub fn resolve_exe(engine_dir: &Path, rel: &str) -> Result<PathBuf, MacError> {
    if !super::recipe::valid_rel_path(rel) {
        return Err(MacError::new("badRecipe", format!("bad launch path {rel}")));
    }
    let base = engine_dir.canonicalize().map_err(|e| MacError::io(engine_dir, e))?;
    let p = rel.split('/').fold(engine_dir.to_path_buf(), |p, s| p.join(s));
    let real = p.canonicalize().map_err(|_| MacError::new("notReady", format!("{rel} is not in the engine")))?;
    if !real.starts_with(&base) || !real.is_file() {
        return Err(MacError::new("badRecipe", format!("{rel} leaves the engine folder")));
    }
    Ok(real)
}

/// Gives `exe` (a resolved `launch[0]`, inside the engine folder) the execute bit where it is readable, when it has
/// none: some release zips (OpenGothic, Descent 3) store their binaries without it.
pub fn ensure_executable(exe: &Path) -> Result<(), MacError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(exe).map_err(|e| MacError::io(exe, e))?.permissions().mode();
        if mode & 0o111 == 0 {
            let with_x = mode | ((mode & 0o444) >> 2) | 0o100;
            std::fs::set_permissions(exe, std::fs::Permissions::from_mode(with_x)).map_err(|e| MacError::io(exe, e))?;
        }
    }
    #[cfg(not(unix))]
    let _ = exe;
    Ok(())
}

/// Starts the port: `launch[0]` with the rest as arguments and `port.env`, `{data}` / `{engine}` / `{appsupport}` /
/// `{prefs}` expanded, from the program's folder, output into the log.
pub fn launch(dirs: &Dirs, data: &Path, port: &PortBlock, log: &Path) -> Result<Child, MacError> {
    let (exe, args) = port.launch.split_first().ok_or_else(|| MacError::new("badRecipe", "port.launch is empty"))?;
    let exe = resolve_exe(&dirs.engine, exe)?;
    ensure_executable(&exe)?;
    let vars = dirs.vars(data);
    let mut c = Command::new(&exe);
    c.args(args.iter().map(|a| expand(a, &vars)));
    c.envs(port.env.iter().map(|(k, v)| (k, expand(v, &vars))));
    if let Some(dir) = exe.parent() {
        c.current_dir(dir);
    }
    let out = super::log_file(log)?;
    let err = out.try_clone().map_err(|e| MacError::io(log, e))?;
    c.stdin(Stdio::null()).stdout(out).stderr(err);
    super::log_line(log, &format!("start {}", exe.display()));
    c.spawn().map_err(|e| MacError::new("launch", format!("{}: {e}", exe.display())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::recipe::{ConfigFile, PortData};

    fn port(files: &[&str]) -> PortBlock {
        PortBlock {
            engine: "vcmi".into(),
            data: PortData {
                from: DataFrom::Folder,
                appid: None,
                files: files.iter().map(|s| s.to_string()).collect(),
                optional: vec![],
                markers: vec![],
                into: None,
            },
            launch: vec!["VCMI.app/Contents/MacOS/vcmiclient".into(), "--data".into(), "{data}".into()],
            env: Default::default(),
            via: None,
            config: vec![],
        }
    }

    /// A layout, an engine folder and a `Library` in a temp folder.
    fn dirs(t: &Path) -> (Layout, Dirs) {
        let lib = t.join("Library");
        std::fs::create_dir_all(lib.join("Application Support")).unwrap();
        std::fs::create_dir_all(lib.join("Preferences")).unwrap();
        std::fs::create_dir_all(t.join("engine")).unwrap();
        std::fs::write(t.join("engine").join(super::super::engine::READY_FILE), "{}").unwrap();
        (Layout::new(t.join("macplay")), Dirs { engine: t.join("engine"), library: Some(lib) })
    }

    #[test]
    fn copies_the_listed_files() {
        let t = tempfile::tempdir().unwrap();
        let (layout, d) = dirs(t.path());
        let src = t.path().join("Heroes3");
        std::fs::create_dir_all(src.join("Data/sub")).unwrap();
        std::fs::create_dir_all(src.join("Maps")).unwrap();
        std::fs::write(src.join("Data/sub/h3bitmap.lod"), b"12345").unwrap();
        std::fs::write(src.join("Maps/a.h3m"), b"1").unwrap();
        std::fs::write(src.join("Heroes3.exe"), b"1").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc", src.join("Data/etc")).unwrap();

        let e = copy_data(&layout, "heroes3", &port(&["Data", "Maps", "MP3"]), &d, &src).unwrap_err();
        assert_eq!(e.code, "dataMissing");
        assert!(e.message.contains("MP3") && !data_ready(&layout, "heroes3", &port(&["Data"]), &d));

        assert_eq!(copy_data(&layout, "heroes3", &port(&["Data/", "Maps"]), &d, &src).unwrap(), 6);
        let dd = data_dir(&layout, "heroes3");
        assert!(dd.join("Data/sub/h3bitmap.lod").is_file() && dd.join("Maps/a.h3m").is_file());
        assert!(!dd.join("Heroes3.exe").exists() && !dd.join("Data/etc").exists());
        assert!(data_ready(&layout, "heroes3", &port(&["Data"]), &d));
        // Markers: what the engine imported itself.
        let lib = d.app_support().unwrap();
        let mut p = port(&["Data"]);
        p.data.markers = vec!["vcmi/Data/H3bitmap.lod".into()];
        assert!(!data_ready(&layout, "other", &p, &d));
        std::fs::create_dir_all(lib.join("vcmi/Data")).unwrap();
        std::fs::write(lib.join("vcmi/Data/H3bitmap.lod"), b"x").unwrap();
        assert!(data_ready(&layout, "other", &p, &d));
        p.data.markers = vec!["../Application Support/vcmi/Data/H3bitmap.lod".into()];
        assert!(!data_ready(&layout, "other", &p, &d));

        copy_data(&layout, "heroes3", &port(&["."]), &d, &src).unwrap();
        assert!(dd.join("Heroes3.exe").is_file());
    }

    #[test]
    fn files_match_ignoring_case_and_keep_the_disk_case() {
        let t = tempfile::tempdir().unwrap();
        let (layout, d) = dirs(t.path());
        let src = t.path().join("Diablo");
        std::fs::create_dir_all(src.join("Music")).unwrap();
        std::fs::write(src.join("diabdat.mpq"), b"123").unwrap();
        std::fs::write(src.join("Music/Town.wav"), b"1").unwrap();
        assert_eq!(resolve_ci(&src, "DIABDAT.MPQ").map(|(_, r)| r), Some("diabdat.mpq".to_string()));
        assert_eq!(resolve_ci(&src, "music/TOWN.WAV/").map(|(_, r)| r), Some("Music/Town.wav".to_string()));
        assert!(resolve_ci(&src, "hellfire.mpq").is_none());

        let mut p = port(&["DIABDAT.MPQ", "MUSIC"]);
        p.data.optional = vec!["hellfire.mpq".into(), "Diabdat.mpq".into()];
        assert_eq!(copy_data(&layout, "devilutionx", &p, &d, &src).unwrap(), 4);
        let names: Vec<String> = std::fs::read_dir(data_dir(&layout, "devilutionx")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert!(names.contains(&"diabdat.mpq".to_string()) && names.contains(&"Music".to_string()), "{names:?}");
        // An optional file is copied when present.
        std::fs::write(src.join("HellFire.MPQ"), b"12").unwrap();
        assert_eq!(copy_data(&layout, "devilutionx", &p, &d, &src).unwrap(), 6);
        assert!(data_dir(&layout, "devilutionx").join("HellFire.MPQ").is_file());
    }

    #[test]
    fn data_into_a_shared_folder() {
        let t = tempfile::tempdir().unwrap();
        let (layout, d) = dirs(t.path());
        let src = t.path().join("Diablo");
        std::fs::create_dir_all(src.join("Data")).unwrap();
        std::fs::write(src.join("DIABDAT.MPQ"), b"123").unwrap();
        std::fs::write(src.join("Data/x.bin"), b"1").unwrap();
        let mut p = port(&["diabdat.mpq", "Data"]);
        p.data.into = Some("{appsupport}/diasurgical/devilution".into());
        p.config = vec![ConfigFile { path: "{data}/diablo.ini".into(), text: "[Paths]\nData={data}\nEngine={engine}\n".into(), keep: false }];
        let into = d.app_support().unwrap().join("diasurgical").join("devilution");

        // Missing folder: filled through a .part folder, renamed into place.
        assert!(!data_ready(&layout, "devilutionx", &p, &d));
        assert_eq!(copy_data(&layout, "devilutionx", &p, &d, &src).unwrap(), 4);
        assert!(into.join("DIABDAT.MPQ").is_file() && into.join("Data/x.bin").is_file());
        assert!(!data_dir(&layout, "devilutionx").exists() && layout.port("devilutionx").join(DATA_FILE).is_file());
        assert!(data_ready(&layout, "devilutionx", &p, &d));
        assert_eq!(into_bytes(&layout, "devilutionx", &p, &d), 4);
        let ini = std::fs::read_to_string(into.join("diablo.ini")).unwrap();
        assert!(ini.contains(&format!("Data={}", into.display())) && ini.contains(&format!("Engine={}", d.engine.display())), "{ini}");

        // The engine's own files in that folder survive a new copy and the removal.
        std::fs::write(into.join("diablo.cfg"), b"settings").unwrap();
        std::fs::write(into.join("Data/save.sv"), b"save").unwrap();
        assert_eq!(copy_data(&layout, "devilutionx", &p, &d, &src).unwrap(), 4);
        assert!(into.join("diablo.cfg").is_file() && into.join("Data/save.sv").is_file());
        // Another recipe target is not ready with this record.
        let mut other = p.clone();
        other.data.into = Some("{appsupport}/elsewhere".into());
        assert!(!data_ready(&layout, "devilutionx", &other, &d));

        // A tampered record cannot name files outside the folder.
        let rec = layout.port("devilutionx").join(DATA_FILE);
        let mut r: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&rec).unwrap()).unwrap();
        std::fs::write(t.path().join("victim"), b"x").unwrap();
        r["copied"].as_array_mut().unwrap().push(serde_json::json!("../../../victim"));
        std::fs::write(&rec, r.to_string()).unwrap();
        remove_data(&layout, "devilutionx", &p, &d);
        assert!(t.path().join("victim").is_file());
        assert!(!into.join("DIABDAT.MPQ").exists() && !into.join("Data/x.bin").exists());
        assert!(into.join("diablo.cfg").is_file() && into.join("Data/save.sv").is_file() && into.join("diablo.ini").is_file());
        assert!(!rec.exists() && !data_ready(&layout, "devilutionx", &p, &d));

        // Only SIGF's files there: the folder goes too. Into the engine: once it is unpacked.
        let mut q = port(&["DIABDAT.MPQ"]);
        q.data.into = Some("{engine}/Diablo.app/Contents/Resources".into());
        let unpacked = Dirs { engine: t.path().join("not-yet"), library: d.library.clone() };
        assert_eq!(copy_data(&layout, "devilutionx", &q, &unpacked, &src).unwrap_err().code, "notReady");
        assert!(!unpacked.engine.exists());
        copy_data(&layout, "devilutionx", &q, &d, &src).unwrap();
        let res = d.engine.join("Diablo.app/Contents/Resources");
        assert!(res.join("DIABDAT.MPQ").is_file());
        remove_data(&layout, "devilutionx", &q, &d);
        assert!(!res.exists() && d.engine.is_dir());
    }

    #[test]
    fn config_keep_and_replace() {
        let t = tempfile::tempdir().unwrap();
        let (_, d) = dirs(t.path());
        let data = t.path().join("data");
        let mut p = port(&["x"]);
        p.config = vec![
            ConfigFile { path: "{prefs}/openmw/openmw.cfg".into(), text: "data=\"{data}\"\n".into(), keep: true },
            ConfigFile { path: "{appsupport}/OpenMW/launcher.cfg".into(), text: "{home} {prefs}".into(), keep: false },
        ];
        write_config(&p, &d, &data).unwrap();
        let cfg = d.prefs().unwrap().join("openmw/openmw.cfg");
        let launcher = d.app_support().unwrap().join("OpenMW/launcher.cfg");
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), format!("data=\"{}\"\n", data.display()));
        // Only the four roots are expanded.
        assert_eq!(std::fs::read_to_string(&launcher).unwrap(), format!("{{home}} {}", d.prefs().unwrap().display()));
        // keep: the player's edit stays; the other one is written again.
        std::fs::write(&cfg, "mine").unwrap();
        std::fs::write(&launcher, "old").unwrap();
        write_config(&p, &d, &data).unwrap();
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "mine");
        assert_ne!(std::fs::read_to_string(&launcher).unwrap(), "old");
        assert!(!part_of(&launcher).exists());
        // Removal: only the replaced-each-launch file.
        remove_config(&p, &d, &data);
        assert!(cfg.is_file() && !launcher.exists());
        // A folder in the file's place is refused.
        std::fs::create_dir_all(&launcher).unwrap();
        assert_eq!(write_config(&p, &d, &data).unwrap_err().code, "io");
        // No Library (off macOS): {appsupport} paths are refused.
        let bare = Dirs { engine: d.engine.clone(), library: None };
        assert_eq!(write_config(&p, &bare, &data).unwrap_err().code, "unsupported");
    }

    #[cfg(unix)]
    #[test]
    fn config_and_data_refuse_links_out() {
        let t = tempfile::tempdir().unwrap();
        let (layout, d) = dirs(t.path());
        let outside = t.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let sup = d.app_support().unwrap();
        std::os::unix::fs::symlink(&outside, sup.join("Evil")).unwrap();
        let mut p = port(&["x"]);
        p.config = vec![ConfigFile { path: "{appsupport}/Evil/sub/a.cfg".into(), text: "x".into(), keep: false }];
        assert_eq!(write_config(&p, &d, &t.path().join("data")).unwrap_err().code, "badRecipe");
        assert!(!outside.join("sub").exists());
        // An existing link as the file itself: refused whether it leads out or stays in.
        std::fs::create_dir_all(sup.join("Good")).unwrap();
        std::os::unix::fs::symlink(outside.join("target"), sup.join("Good/a.cfg")).unwrap();
        p.config[0].path = "{appsupport}/Good/a.cfg".into();
        assert!(write_config(&p, &d, &t.path().join("data")).is_err());
        assert!(!outside.join("target").exists());
        std::fs::write(sup.join("Good/real.cfg"), b"x").unwrap();
        std::os::unix::fs::symlink(sup.join("Good/real.cfg"), sup.join("Good/c.cfg")).unwrap();
        p.config[0].path = "{appsupport}/Good/c.cfg".into();
        assert_eq!(write_config(&p, &d, &t.path().join("data")).unwrap_err().code, "io");
        assert_eq!(std::fs::read(sup.join("Good/real.cfg")).unwrap(), b"x");
        // A link inside the root is fine.
        std::os::unix::fs::symlink(sup.join("Good"), sup.join("Alias")).unwrap();
        p.config[0].path = "{appsupport}/Alias/b.cfg".into();
        write_config(&p, &d, &t.path().join("data")).unwrap();
        assert!(sup.join("Good/b.cfg").is_file());
        // data.into through a link out.
        let src = t.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("x"), b"1").unwrap();
        let mut q = port(&["x"]);
        q.data.into = Some("{appsupport}/Evil/data".into());
        assert_eq!(copy_data(&layout, "g", &q, &d, &src).unwrap_err().code, "badRecipe");
        assert!(!outside.join("data").exists());
    }

    #[test]
    fn from_none_is_always_ready() {
        let t = tempfile::tempdir().unwrap();
        let (layout, d) = dirs(t.path());
        let mut p = port(&[]);
        p.data.from = DataFrom::None;
        assert!(data_ready(&layout, "openra", &p, &d));
        assert_eq!(copy_data(&layout, "openra", &p, &d, t.path()).unwrap(), 0);
        assert!(!layout.port("openra").exists());
    }

    #[cfg(unix)]
    #[test]
    fn launch_paths_stay_in_the_engine() {
        let t = tempfile::tempdir().unwrap();
        let eng = t.path().join("engine");
        std::fs::create_dir_all(eng.join("VCMI.app/Contents/MacOS")).unwrap();
        std::fs::write(eng.join("VCMI.app/Contents/MacOS/vcmiclient"), b"").unwrap();
        std::fs::write(t.path().join("outside"), b"").unwrap();
        std::os::unix::fs::symlink("../outside", eng.join("out")).unwrap();
        assert!(resolve_exe(&eng, "VCMI.app/Contents/MacOS/vcmiclient").is_ok());
        assert_eq!(resolve_exe(&eng, "out").unwrap_err().code, "badRecipe");
        assert_eq!(resolve_exe(&eng, "VCMI.app").unwrap_err().code, "badRecipe");
        assert_eq!(resolve_exe(&eng, "missing").unwrap_err().code, "notReady");
        assert_eq!(resolve_exe(&eng, "../outside").unwrap_err().code, "badRecipe");
    }

    #[cfg(unix)]
    #[test]
    fn launch_target_gets_the_execute_bit() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let bare = t.path().join("opengothic");
        std::fs::write(&bare, b"").unwrap();
        std::fs::set_permissions(&bare, std::fs::Permissions::from_mode(0o644)).unwrap();
        ensure_executable(&bare).unwrap();
        assert_eq!(mode(&bare), 0o755);
        // Owner-only files get it for the owner only; one that already has an x bit is left alone.
        let own = t.path().join("own");
        std::fs::write(&own, b"").unwrap();
        std::fs::set_permissions(&own, std::fs::Permissions::from_mode(0o600)).unwrap();
        ensure_executable(&own).unwrap();
        assert_eq!(mode(&own), 0o700);
        std::fs::set_permissions(&own, std::fs::Permissions::from_mode(0o740)).unwrap();
        ensure_executable(&own).unwrap();
        assert_eq!(mode(&own), 0o740);
    }
}
