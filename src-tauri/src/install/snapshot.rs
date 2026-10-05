//! `game-dir-snapshot`: the only strategy that writes into the player's game folder, so it keeps enough to undo
//! itself exactly. The manifest is written before the first game file, so even a crash mid-install is restorable.

use super::fetch::sha256_file;
use super::paths::{ensure_real_parent_inside, rel_string, resolve_inside};
use super::InstallError;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub const PLACEHOLDER: &str = "{game}";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub game_dir: String,
    pub files: Vec<Entry>,
    /// Folders the install created, shallow first; removed on restore when empty.
    #[serde(default)]
    pub created_dirs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// Relative to the game dir, forward slashes.
    pub path: String,
    pub existed_before: bool,
    pub sha256_before: Option<String>,
    pub sha256_after: String,
}

/// One file to write: `src` (verified cache file) to `dst` (recipe destination, `{game}/...`).
pub struct Planned {
    pub src: PathBuf,
    pub dst: String,
    pub sha256: String,
}

fn manifest_path(snap: &Path) -> PathBuf {
    snap.join("manifest.json")
}

fn originals(snap: &Path) -> PathBuf {
    snap.join("files")
}

pub fn read_manifest(snap: &Path) -> Result<Manifest, InstallError> {
    let p = manifest_path(snap);
    let s = std::fs::read_to_string(&p).map_err(|e| InstallError::io(&p, e))?;
    serde_json::from_str(&s).map_err(|e| InstallError::io(&p, e))
}

fn copy(src: &Path, dst: &Path) -> Result<(), InstallError> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| InstallError::io(parent, e))?;
    }
    std::fs::copy(src, dst).map(|_| ()).map_err(|e| InstallError::io(dst, e))
}

/// Snapshots every file `files` will overwrite into `snap`, then writes them into `game_dir`.
/// On a write error the files already written are rolled back before returning.
pub fn apply(snap: &Path, game_dir: &Path, files: &[Planned]) -> Result<Manifest, InstallError> {
    if !game_dir.is_dir() {
        return Err(InstallError::io(game_dir, "game folder not found"));
    }
    if manifest_path(snap).exists() {
        return Err(InstallError::io(snap, "a snapshot already exists here; restore it first"));
    }
    let mut seen = HashSet::new();
    let mut resolved = Vec::new();
    for f in files {
        let (abs, rel) = resolve_inside(game_dir, PLACEHOLDER, &f.dst)?;
        if !seen.insert(rel_string(&rel).to_ascii_lowercase()) {
            return Err(InstallError::recipe(format!("two files target {}", f.dst)));
        }
        if abs.is_dir() {
            return Err(InstallError::io(&abs, "destination is a folder"));
        }
        resolved.push((f, abs, rel));
    }

    let mut manifest = Manifest { game_dir: game_dir.to_string_lossy().into_owned(), files: vec![], created_dirs: vec![] };
    let mut created = HashSet::new();
    for (f, abs, rel) in &resolved {
        // Folders that do not exist yet, shallow first, so restore can remove them deepest first.
        let mut missing = vec![];
        let mut cur = rel.parent();
        while let Some(p) = cur.filter(|p| !p.as_os_str().is_empty()) {
            if !game_dir.join(p).exists() {
                missing.push(rel_string(p));
            }
            cur = p.parent();
        }
        for d in missing.into_iter().rev() {
            if created.insert(d.clone()) {
                manifest.created_dirs.push(d);
            }
        }
        let existed = abs.is_file();
        let before = if existed {
            copy(abs, &originals(snap).join(rel))?;
            Some(sha256_file(abs)?)
        } else {
            None
        };
        manifest.files.push(Entry {
            path: rel_string(rel),
            existed_before: existed,
            sha256_before: before,
            sha256_after: f.sha256.to_ascii_lowercase(),
        });
    }
    std::fs::create_dir_all(snap).map_err(|e| InstallError::io(snap, e))?;
    let mp = manifest_path(snap);
    let json = serde_json::to_string_pretty(&manifest).map_err(|e| InstallError::io(&mp, e))?;
    std::fs::write(&mp, json).map_err(|e| InstallError::io(&mp, e))?;

    let written = (|| {
        for (f, abs, _) in &resolved {
            if let Some(parent) = abs.parent() {
                std::fs::create_dir_all(parent).map_err(|e| InstallError::io(parent, e))?;
            }
            ensure_real_parent_inside(game_dir, abs, &f.dst)?;
            copy(&f.src, abs)?;
        }
        Ok(())
    })();
    if let Err(e) = written {
        let _ = restore(snap, true);
        return Err(e);
    }
    Ok(manifest)
}

/// Game files that no longer match what the install wrote (edited, updated by the store, deleted).
pub fn tampered(snap: &Path) -> Result<Vec<String>, InstallError> {
    let m = read_manifest(snap)?;
    let game_dir = PathBuf::from(&m.game_dir);
    let mut out = vec![];
    for e in &m.files {
        let (abs, _) = resolve_inside(&game_dir, PLACEHOLDER, &e.path)?;
        let now = if abs.is_file() { Some(sha256_file(&abs)?) } else { None };
        if now.as_deref() != Some(e.sha256_after.as_str()) {
            out.push(e.path.clone());
        }
    }
    Ok(out)
}

/// Puts the originals back, deletes files the install added, removes folders it created, then drops the snapshot.
/// Refuses when a game file changed since install, unless `force`.
pub fn restore(snap: &Path, force: bool) -> Result<(), InstallError> {
    let m = read_manifest(snap)?;
    let game_dir = PathBuf::from(&m.game_dir);
    if !force {
        let t = tampered(snap)?;
        if !t.is_empty() {
            return Err(InstallError::Tampered { files: t });
        }
    }
    // Check the saved originals before touching anything: a half restore is worse than none.
    let mut corrupt = vec![];
    for e in m.files.iter().filter(|e| e.existed_before) {
        let (_, rel) = resolve_inside(&game_dir, PLACEHOLDER, &e.path)?;
        let saved = originals(snap).join(&rel);
        let ok = saved.is_file() && Some(sha256_file(&saved)?) == e.sha256_before;
        if !ok {
            corrupt.push(e.path.clone());
        }
    }
    if !corrupt.is_empty() {
        return Err(InstallError::SnapshotCorrupt { files: corrupt });
    }
    for e in &m.files {
        let (abs, rel) = resolve_inside(&game_dir, PLACEHOLDER, &e.path)?;
        if e.existed_before {
            copy(&originals(snap).join(&rel), &abs)?;
        } else if abs.is_file() {
            std::fs::remove_file(&abs).map_err(|err| InstallError::io(&abs, err))?;
        }
    }
    for d in m.created_dirs.iter().rev() {
        if let Ok((abs, _)) = resolve_inside(&game_dir, PLACEHOLDER, d) {
            let _ = std::fs::remove_dir(abs); // only succeeds when empty: player files are never removed
        }
    }
    std::fs::remove_dir_all(snap).map_err(|e| InstallError::io(snap, e))
}
