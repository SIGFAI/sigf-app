//! Player builds (docs/RECIPE-FORMAT.md section 4, `player_build`): files SIGF must not distribute (a native library
//! that compiles decompiled game code) are built once on the player's PC. Everything is pinned: the script is a release
//! asset of the mashup's own SIGFAI repo, its inputs commit-pinned GitHub sources, each by sha256 and size, and the
//! toolchain comes from the app's own table (`tools::TOOLS`). The app downloads all of it itself, through its
//! allowlist, before the script starts; the script then runs with no network need, in a work folder under
//! `<SIGF_HOME>/build/<slug>/`, which is deleted afterwards (inputs included: they are never kept in the cache).
//!
//! The script runs with the toolchain's `sh` and this environment (paths with forward slashes):
//! `SIGF_IN` (inputs: files by name, unpacked zips as folders), `SIGF_OUT` (where the declared outputs must be left),
//! `SIGF_WORK` (its scratch folder and working directory), `PATH` (the toolchain, then Windows' own folders), `HOME`,
//! `TEMP`/`TMP` (inside the work folder). No other variable of the app's environment is passed on.

use super::fetch::{fetch_pinned, Expected, FetchOpts};
use super::paths::{path_string, resolve_inside};
use super::recipe::PlayerBuild;
use super::{extract_zip, tools, InstallError};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Largest output a build may leave.
pub const OUTPUT_MAX_BYTES: u64 = 512 * 1024 * 1024;
/// Longest a build may run, whatever its `minutes`.
pub const MAX_RUN: Duration = Duration::from_secs(60 * 60);

/// One build's folders: `<root>/{in,out,work,tmp,dl}`, the script and `build.log` in `<root>`.
pub struct Dirs {
    pub root: PathBuf,
    pub input: PathBuf,
    pub out: PathBuf,
    pub work: PathBuf,
    pub tmp: PathBuf,
    pub dl: PathBuf,
}

impl Dirs {
    pub fn new(root: PathBuf) -> Self {
        Self { input: root.join("in"), out: root.join("out"), work: root.join("work"), tmp: root.join("tmp"), dl: root.join("dl"), root }
    }
}

fn failed(b: &PlayerBuild, message: impl Into<String>, log: Option<&Path>) -> InstallError {
    InstallError::BuildFailed { id: b.id.clone(), label: b.label.clone(), message: message.into(), log: log.map(path_string) }
}

/// The tools a build names, ready: downloaded and unpacked once (`tools::ensure`), or, in dev mode only, a folder
/// given in `overrides` (tool id -> a folder put on PATH as is, its `sh.exe` the shell).
pub fn toolchain(home: &Path, b: &PlayerBuild, overrides: &HashMap<String, PathBuf>, on_bytes: &mut dyn FnMut(u64, Option<u64>)) -> Result<Vec<tools::Ready>, InstallError> {
    let mut out = vec![];
    for id in &b.toolchain {
        let t = tools::find(id).ok_or_else(|| InstallError::recipe(format!("unknown tool {id}")))?;
        if let Some(dir) = overrides.get(id) {
            out.push(tools::Ready { path: vec![dir.clone()], shell: t.shell.map(|_| dir.join("sh.exe")) });
            continue;
        }
        out.push(tools::ensure(home, t, on_bytes)?);
    }
    Ok(out)
}

/// Fetches the script and the inputs into `dirs` (hash and size checked, through `FetchOpts::for_build`), the inputs
/// as `in/<name>` or, unpacked, `in/<name>/`.
pub fn stage(b: &PlayerBuild, dirs: &Dirs, allow_local: bool) -> Result<PathBuf, InstallError> {
    for d in [&dirs.input, &dirs.out, &dirs.work, &dirs.tmp, &dirs.dl] {
        std::fs::create_dir_all(d).map_err(|e| InstallError::io(d, e))?;
    }
    let get = |f: &super::recipe::BuildFile| {
        fetch_pinned(&dirs.dl, &f.url, &Expected::sha256(&f.sha256)?, &FetchOpts::for_build(allow_local, f.size), &mut |_, _| {}).map(|g| g.path)
    };
    let script = dirs.root.join(&b.script.name);
    std::fs::copy(get(&b.script)?, &script).map_err(|e| InstallError::io(&script, e))?;
    for i in &b.inputs {
        let got = get(i)?;
        let (dst, _) = resolve_inside(&dirs.input, "", &i.name)?;
        if !i.unpack {
            std::fs::copy(&got, &dst).map_err(|e| InstallError::io(&dst, e))?;
            continue;
        }
        let Some(root) = &i.root else {
            extract_zip(&got, &dst)?;
            continue;
        };
        // Unpack aside, then move only the root folder's contents into place.
        let aside = dirs.tmp.join(format!("unpack-{}", i.name));
        extract_zip(&got, &aside)?;
        let (inner, _) = resolve_inside(&aside, "", root)?;
        if !inner.is_dir() {
            return Err(failed(b, format!("{} has no folder {root}", i.name), None));
        }
        std::fs::rename(&inner, &dst).map_err(|e| InstallError::io(&dst, e))?;
        let _ = std::fs::remove_dir_all(&aside);
    }
    Ok(script)
}

fn slashes(p: &Path) -> String {
    path_string(p).replace('\\', "/")
}

/// Runs the staged script with the toolchain's shell (see the module doc), its output into `build.log`, killed past
/// its time. Then every declared output must be a file in `out/`.
pub fn run(b: &PlayerBuild, dirs: &Dirs, script: &Path, tools: &[tools::Ready]) -> Result<(), InstallError> {
    let shell = tools.iter().find_map(|t| t.shell.clone()).ok_or_else(|| failed(b, "no shell in the toolchain", None))?;
    let log = dirs.root.join("build.log");
    let out = std::fs::File::create(&log).map_err(|e| InstallError::io(&log, e))?;
    let err = out.try_clone().map_err(|e| InstallError::io(&log, e))?;
    let mut path: Vec<PathBuf> = tools.iter().flat_map(|t| t.path.iter().cloned()).collect();
    let sysroot = std::env::var_os("SystemRoot").map(PathBuf::from);
    if let Some(r) = &sysroot {
        path.push(r.join("System32"));
        path.push(r.clone());
    }
    let path = std::env::join_paths(&path).map_err(|e| failed(b, e.to_string(), None))?;
    let mut cmd = std::process::Command::new(&shell);
    cmd.arg(slashes(script))
        .current_dir(&dirs.work)
        .env_clear()
        .env("PATH", path)
        .env("SIGF_IN", slashes(&dirs.input))
        .env("SIGF_OUT", slashes(&dirs.out))
        .env("SIGF_WORK", slashes(&dirs.work))
        .env("HOME", slashes(&dirs.work))
        .env("TEMP", path_string(&dirs.tmp))
        .env("TMP", path_string(&dirs.tmp))
        .stdin(std::process::Stdio::null())
        .stdout(out)
        .stderr(err);
    // What Windows programs (make, gcc, Python) expect to find.
    for k in ["SystemRoot", "windir", "SystemDrive", "OS", "NUMBER_OF_PROCESSORS", "PROCESSOR_ARCHITECTURE", "COMSPEC", "PATHEXT"] {
        if let Some(v) = std::env::var_os(k) {
            cmd.env(k, v);
        }
    }
    tools::no_window(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| failed(b, format!("could not start {}: {e}", shell.display()), Some(&log)))?;
    let limit = b.minutes.map(|m| Duration::from_secs(u64::from(m) * 60 * 4)).unwrap_or(MAX_RUN).clamp(Duration::from_secs(10 * 60), MAX_RUN);
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if started.elapsed() > limit => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(failed(b, format!("stopped after {} minutes", limit.as_secs() / 60), Some(&log)));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(250)),
            Err(e) => return Err(failed(b, e.to_string(), Some(&log))),
        }
    };
    if !status.success() {
        return Err(failed(b, format!("the build script failed ({status}): {}", tail(&log, 12)), Some(&log)));
    }
    for o in &b.outputs {
        let p = dirs.out.join(&o.name);
        let meta = std::fs::symlink_metadata(&p).ok().filter(|m| m.is_file());
        match meta {
            Some(m) if m.len() > 0 && m.len() <= OUTPUT_MAX_BYTES => {}
            Some(_) => return Err(failed(b, format!("{} is empty or too large", o.name), Some(&log))),
            None => return Err(failed(b, format!("the build did not produce {}", o.name), Some(&log))),
        }
    }
    Ok(())
}

/// The last `n` lines of a log, joined with " | ".
pub fn tail(log: &Path, n: usize) -> String {
    let text = std::fs::read(log).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let lines: Vec<&str> = text.lines().map(str::trim_end).filter(|l| !l.is_empty()).collect();
    lines[lines.len().saturating_sub(n)..].join(" | ")
}
