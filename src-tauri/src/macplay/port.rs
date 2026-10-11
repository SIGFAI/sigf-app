//! Native ports (VCMI, GeneralsX): the game's data files copied from the player's own copy into
//! `ports/<game>/data`, and the engine's Mac program started with them.

use super::recipe::PortBlock;
use super::{Layout, MacError};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// Written last into the data folder: the copy is complete.
pub const DATA_FILE: &str = ".sigf-data.json";

pub fn data_dir(layout: &Layout, game: &str) -> PathBuf {
    layout.port(game).join("data")
}

/// The data is there: copied by the core, or one of the recipe's markers exists under `base` (the user's
/// `Library/Application Support`, where VCMI keeps what it imported).
pub fn data_ready(layout: &Layout, game: &str, port: &PortBlock, base: Option<&Path>) -> bool {
    data_dir(layout, game).join(DATA_FILE).is_file() || base.is_some_and(|b| markers_present(b, &port.data.markers))
}

pub fn markers_present(base: &Path, markers: &[String]) -> bool {
    markers.iter().filter(|m| super::recipe::valid_rel_path(m)).any(|m| m.trim_end_matches('/').split('/').fold(base.to_path_buf(), |p, s| p.join(s)).exists())
}

/// Copies `src` into `dst` (files and folders; links are skipped, so nothing outside the copy is read).
pub fn copy_tree(src: &Path, dst: &Path) -> Result<u64, MacError> {
    let meta = std::fs::symlink_metadata(src).map_err(|e| MacError::io(src, e))?;
    if meta.file_type().is_symlink() {
        return Ok(0);
    }
    if meta.is_file() {
        if let Some(p) = dst.parent() {
            std::fs::create_dir_all(p).map_err(|e| MacError::io(p, e))?;
        }
        return std::fs::copy(src, dst).map_err(|e| MacError::io(dst, e));
    }
    std::fs::create_dir_all(dst).map_err(|e| MacError::io(dst, e))?;
    let mut n = 0;
    for e in std::fs::read_dir(src).map_err(|e| MacError::io(src, e))?.flatten() {
        n += copy_tree(&e.path(), &dst.join(e.file_name()))?;
    }
    Ok(n)
}

/// Copies the recipe's `data.files` from the game folder `src` (`["."]`: all of it) into `ports/<game>/data`, through
/// a `.part` folder. A missing entry is a `dataMissing` error naming them all.
pub fn copy_data(layout: &Layout, game: &str, port: &PortBlock, src: &Path) -> Result<u64, MacError> {
    let whole = port.data.files.len() == 1 && port.data.files[0] == ".";
    let rel = |f: &str| f.trim_end_matches('/').split('/').fold(PathBuf::new(), |p, s| p.join(s));
    if !whole {
        let missing: Vec<&str> = port.data.files.iter().map(String::as_str).filter(|f| !src.join(rel(f)).exists()).collect();
        if !missing.is_empty() {
            return Err(MacError::new("dataMissing", format!("{} has no {}", src.display(), missing.join(", "))));
        }
    }
    let dst = data_dir(layout, game);
    let part = dst.with_extension("part");
    let _ = std::fs::remove_dir_all(&part);
    let bytes = if whole {
        copy_tree(src, &part)?
    } else {
        let mut n = 0;
        for f in &port.data.files {
            n += copy_tree(&src.join(rel(f)), &part.join(rel(f)))?;
        }
        n
    };
    let rec = serde_json::json!({ "from": src.to_string_lossy(), "files": port.data.files, "bytes": bytes });
    std::fs::write(part.join(DATA_FILE), rec.to_string()).map_err(|e| MacError::io(&part, e))?;
    let _ = std::fs::remove_dir_all(&dst);
    std::fs::rename(&part, &dst).map_err(|e| MacError::io(&dst, e))?;
    Ok(bytes)
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

/// Starts the port: `launch[0]` with the rest as arguments and `port.env`, `{data}` / `{engine}` expanded, from the
/// program's folder, output into the log.
pub fn launch(engine_dir: &Path, data: &Path, port: &PortBlock, log: &Path) -> Result<Child, MacError> {
    let (exe, args) = port.launch.split_first().ok_or_else(|| MacError::new("badRecipe", "port.launch is empty"))?;
    let exe = resolve_exe(engine_dir, exe)?;
    let vars = [("data", data), ("engine", engine_dir)];
    let mut c = Command::new(&exe);
    c.args(args.iter().map(|a| super::wine::expand(a, &vars)));
    c.envs(port.env.iter().map(|(k, v)| (k, super::wine::expand(v, &vars))));
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
    use super::super::recipe::{DataFrom, PortData};

    fn port(files: &[&str]) -> PortBlock {
        PortBlock {
            engine: "vcmi".into(),
            data: PortData { from: DataFrom::Folder, appid: None, files: files.iter().map(|s| s.to_string()).collect(), markers: vec![] },
            launch: vec!["VCMI.app/Contents/MacOS/vcmiclient".into(), "--data".into(), "{data}".into()],
            env: Default::default(),
            via: None,
        }
    }

    #[test]
    fn copies_the_listed_files() {
        let t = tempfile::tempdir().unwrap();
        let layout = Layout::new(t.path().join("macplay"));
        let src = t.path().join("Heroes3");
        std::fs::create_dir_all(src.join("Data/sub")).unwrap();
        std::fs::create_dir_all(src.join("Maps")).unwrap();
        std::fs::write(src.join("Data/sub/h3bitmap.lod"), b"12345").unwrap();
        std::fs::write(src.join("Maps/a.h3m"), b"1").unwrap();
        std::fs::write(src.join("Heroes3.exe"), b"1").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc", src.join("Data/etc")).unwrap();

        let e = copy_data(&layout, "heroes3", &port(&["Data", "Maps", "MP3"]), &src).unwrap_err();
        assert_eq!(e.code, "dataMissing");
        assert!(e.message.contains("MP3") && !data_ready(&layout, "heroes3", &port(&["Data"]), None));

        assert_eq!(copy_data(&layout, "heroes3", &port(&["Data/", "Maps"]), &src).unwrap(), 6);
        let d = data_dir(&layout, "heroes3");
        assert!(d.join("Data/sub/h3bitmap.lod").is_file() && d.join("Maps/a.h3m").is_file());
        assert!(!d.join("Heroes3.exe").exists() && !d.join("Data/etc").exists());
        assert!(data_ready(&layout, "heroes3", &port(&["Data"]), None));
        // Markers: what the engine imported itself.
        let lib = t.path().join("Application Support");
        let mut p = port(&["Data"]);
        p.data.markers = vec!["vcmi/Data/H3bitmap.lod".into()];
        assert!(!data_ready(&layout, "other", &p, Some(&lib)));
        std::fs::create_dir_all(lib.join("vcmi/Data")).unwrap();
        std::fs::write(lib.join("vcmi/Data/H3bitmap.lod"), b"x").unwrap();
        assert!(data_ready(&layout, "other", &p, Some(&lib)));
        p.data.markers = vec!["../Application Support/vcmi/Data/H3bitmap.lod".into()];
        assert!(!data_ready(&layout, "other", &p, Some(&lib)));

        copy_data(&layout, "heroes3", &port(&["."]), &src).unwrap();
        assert!(d.join("Heroes3.exe").is_file());
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
}
