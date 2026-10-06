//! Bring your own copy (docs/RECIPE-FORMAT.md section 4, `own_copies`): a mashup that needs a file of a game the player
//! owns (a cartridge ROM they dumped) gets it from the player's own PC, never from SIGF. The app looks for it in the
//! usual folders (loose or inside a `.zip`), or the player picks it; the file is checked against the recipe's SHA-1s
//! (after byte-order normalization for N64 dumps) and copied into the mashup's own folder, which Restore deletes.
//! The file never leaves the PC: nothing here opens a network connection.

use super::check::{OWN_MAX_BYTES, OWN_NORMALIZE_MAX_BYTES};
use super::fetch::hex;
use super::recipe::OwnCopy;
use super::InstallError;
use sha1::{Digest, Sha1};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Where the player's copy is: a file, or an entry of a zip file.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OwnSource {
    pub path: PathBuf,
    /// The entry's name inside the zip at `path`, when the copy is zipped.
    #[serde(default)]
    pub entry: Option<String>,
}

impl OwnSource {
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into(), entry: None }
    }

    /// How the UI names it: `C:\...\Super Mario 64.zip > Super Mario 64 (USA).z64`.
    pub fn display(&self) -> String {
        let p = self.path.to_string_lossy().into_owned();
        match &self.entry {
            Some(e) => format!("{p} > {e}"),
            None => p,
        }
    }
}

/// N64 dump byte orders, told apart by the first word of the header (0x80371240 in big-endian order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum N64Order {
    /// `.z64`: big-endian, the native order. Hashes are of this order.
    Z64,
    /// `.v64`: bytes swapped in pairs.
    V64,
    /// `.n64`: little-endian 32-bit words.
    N64,
}

pub fn n64_order(head: &[u8]) -> Option<N64Order> {
    match head.get(..4)? {
        [0x80, 0x37, 0x12, 0x40] => Some(N64Order::Z64),
        [0x37, 0x80, 0x40, 0x12] => Some(N64Order::V64),
        [0x40, 0x12, 0x37, 0x80] => Some(N64Order::N64),
        _ => None,
    }
}

/// Rewrites `buf` (a whole dump, length a multiple of 4) in `.z64` order.
pub fn n64_normalize(buf: &mut [u8], order: N64Order) {
    match order {
        N64Order::Z64 => {}
        N64Order::V64 => buf.as_chunks_mut::<2>().0.iter_mut().for_each(|c| c.swap(0, 1)),
        N64Order::N64 => buf.as_chunks_mut::<4>().0.iter_mut().for_each(|c| c.reverse()),
    }
}

/// What reading a candidate found: its SHA-1 (after normalization) and whether the recipe accepts it.
#[derive(Debug, Clone)]
pub struct Checked {
    pub sha1: String,
    pub ok: bool,
}

fn mismatch(c: &OwnCopy, src: &OwnSource, sha1: impl Into<String>) -> InstallError {
    InstallError::OwnCopyMismatch { game: c.game.clone(), label: c.label.clone(), file: src.display(), sha1: sha1.into() }
}

/// Opens the source for reading, with its size.
fn open(src: &OwnSource) -> Result<(Box<dyn Read>, u64), InstallError> {
    let f = std::fs::File::open(&src.path).map_err(|e| InstallError::io(&src.path, e))?;
    match &src.entry {
        None => {
            let len = f.metadata().map_err(|e| InstallError::io(&src.path, e))?.len();
            Ok((Box::new(f), len))
        }
        Some(name) => {
            let mut z = zip::ZipArchive::new(f).map_err(|e| InstallError::io(&src.path, e))?;
            let idx = z.index_for_name(name).ok_or_else(|| InstallError::io(&src.path, format!("no {name} in this zip")))?;
            let len = z.by_index(idx).map_err(|e| InstallError::io(&src.path, e))?.size();
            if len > OWN_NORMALIZE_MAX_BYTES {
                return Err(InstallError::io(&src.path, "a zipped copy may be at most 256 MiB: unzip it first"));
            }
            // Read the entry through an owned archive: ZipFile borrows the archive, so copy it out within the cap.
            let mut buf = vec![];
            z.by_index(idx).map_err(|e| InstallError::io(&src.path, e))?.take(len + 1).read_to_end(&mut buf).map_err(|e| InstallError::io(&src.path, e))?;
            Ok((Box::new(std::io::Cursor::new(buf)), len))
        }
    }
}

/// Reads the player's copy, normalizes it when the recipe says `format: "n64"`, and hashes it (SHA-1). With `out`, the
/// normalized bytes are also written there. Never refuses quietly: a wrong size or an unknown N64 header is a mismatch.
pub fn read_checked(c: &OwnCopy, src: &OwnSource, out: Option<&Path>) -> Result<Checked, InstallError> {
    let (mut r, len) = open(src)?;
    if c.rom.size.is_some_and(|s| s != len) || len > OWN_MAX_BYTES {
        return Err(mismatch(c, src, format!("size {len} bytes")));
    }
    let accepted = |h: &str| c.rom.sha1.iter().any(|a| a.eq_ignore_ascii_case(h));
    let mut sink: Option<std::fs::File> = match out {
        Some(p) => Some(std::fs::File::create(p).map_err(|e| InstallError::io(p, e))?),
        None => None,
    };
    let mut h = Sha1::new();
    if c.rom.format.as_deref() == Some("n64") {
        if len > OWN_NORMALIZE_MAX_BYTES || len % 4 != 0 {
            return Err(mismatch(c, src, format!("size {len} bytes")));
        }
        let mut buf = Vec::with_capacity(len as usize);
        r.take(len + 1).read_to_end(&mut buf).map_err(|e| InstallError::io(&src.path, e))?;
        if buf.len() as u64 != len {
            return Err(mismatch(c, src, "unreadable"));
        }
        let Some(order) = n64_order(&buf) else { return Err(mismatch(c, src, "not an N64 ROM header")) };
        n64_normalize(&mut buf, order);
        h.update(&buf);
        if let (Some(f), Some(p)) = (sink.as_mut(), out) {
            f.write_all(&buf).map_err(|e| InstallError::io(p, e))?;
        }
    } else {
        let mut buf = vec![0u8; 1 << 16];
        let mut done = 0u64;
        loop {
            let n = r.read(&mut buf).map_err(|e| InstallError::io(&src.path, e))?;
            if n == 0 {
                break;
            }
            done += n as u64;
            if done > len {
                return Err(mismatch(c, src, "file grew while read"));
            }
            h.update(&buf[..n]);
            if let (Some(f), Some(p)) = (sink.as_mut(), out) {
                f.write_all(&buf[..n]).map_err(|e| InstallError::io(p, e))?;
            }
        }
    }
    if let (Some(f), Some(p)) = (sink.as_mut(), out) {
        f.flush().map_err(|e| InstallError::io(p, e))?;
    }
    let sha1 = hex(&h.finalize());
    Ok(Checked { ok: accepted(&sha1), sha1 })
}

/// The player's copy, checked: Ok when its SHA-1 is one the recipe accepts, else `OwnCopyMismatch`.
pub fn verify(c: &OwnCopy, src: &OwnSource) -> Result<(), InstallError> {
    let got = read_checked(c, src, None)?;
    if got.ok {
        Ok(())
    } else {
        Err(mismatch(c, src, got.sha1))
    }
}

/// Copies the checked copy to `dir/<rom.as>` (temp file + rename; the bytes written are the ones hashed).
pub fn place(c: &OwnCopy, src: &OwnSource, dir: &Path) -> Result<PathBuf, InstallError> {
    std::fs::create_dir_all(dir).map_err(|e| InstallError::io(dir, e))?;
    let dst = dir.join(&c.rom.save_as);
    let tmp = dir.join(format!(".{}.sigf-tmp", c.rom.save_as));
    let got = read_checked(c, src, Some(&tmp));
    match got {
        Ok(g) if g.ok => std::fs::rename(&tmp, &dst).map(|_| dst.clone()).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            InstallError::io(&dst, e)
        }),
        Ok(g) => {
            let _ = std::fs::remove_file(&tmp);
            Err(mismatch(c, src, g.sha1))
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

// ---------- finding the player's copy ----------

/// The entries of a zip that could be the copy: one of the recipe's extensions, and its size when given.
fn zip_candidates(c: &OwnCopy, z: &Path) -> Vec<OwnSource> {
    let exts: Vec<String> = c.rom.extensions.iter().map(|e| e.to_ascii_lowercase()).collect();
    let Ok(f) = std::fs::File::open(z) else { return vec![] };
    let Ok(mut a) = zip::ZipArchive::new(f) else { return vec![] };
    let mut out = vec![];
    for i in 0..a.len().min(4096) {
        let Ok(e) = a.by_index(i) else { continue };
        let size_ok = c.rom.size.map_or(e.size() <= OWN_NORMALIZE_MAX_BYTES, |s| s == e.size());
        if e.is_file() && exts.contains(&ext_of(e.name())) && size_ok {
            out.push(OwnSource { path: z.to_path_buf(), entry: Some(e.name().to_string()) });
        }
    }
    out
}

/// The first entry of the zip the player picked that the recipe accepts.
pub fn search_zip(c: &OwnCopy, z: &Path) -> Option<OwnSource> {
    zip_candidates(c, z).into_iter().take(64).find(|s| read_checked(c, s, None).is_ok_and(|g| g.ok))
}

/// What the search found for one copy. `found` is a source the recipe accepts; `rejected` the candidates (right
/// extension and size) that did not match, by file name, so the UI can say "found X, but it is not the right dump".
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    pub game: String,
    pub label: String,
    pub found: Option<OwnSource>,
    pub rejected: Vec<String>,
}

/// Folders searched by default: the player's Downloads, Desktop and Documents (OneDrive ones too), and common ROM
/// folders. Only these, a few levels deep: never the whole disk.
pub fn default_roots() -> Vec<PathBuf> {
    let mut out = vec![];
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).filter(|h| !h.is_empty()).map(PathBuf::from) {
        for sub in ["Downloads", "Desktop", "Documents", "ROMs", "Roms", "roms", "Games", "OneDrive/Desktop", "OneDrive/Documents", "OneDrive/Downloads"] {
            out.push(home.join(sub));
        }
        out.push(home.clone());
    }
    for drive in ["C:", "D:", "E:"] {
        out.push(PathBuf::from(format!("{drive}/ROMs")));
        out.push(PathBuf::from(format!("{drive}/Emulation")));
    }
    out.retain(|p| p.is_dir());
    let mut seen = vec![];
    out.retain(|p| {
        let k = p.to_string_lossy().to_lowercase();
        !seen.contains(&k) && {
            seen.push(k);
            true
        }
    });
    out
}

/// Limits of one search.
#[derive(Debug, Clone, Copy)]
pub struct SearchLimits {
    pub depth: usize,
    pub entries: usize,
    pub candidates: usize,
    pub time: Duration,
}

impl Default for SearchLimits {
    fn default() -> Self {
        Self { depth: 4, entries: 60_000, candidates: 40, time: Duration::from_secs(15) }
    }
}

/// Folders never walked into: system, app data, version control and package folders.
const SKIP_DIRS: &[&str] = &["appdata", "node_modules", "$recycle.bin", "windows", "program files", "program files (x86)", "programdata", "system volume information"];

/// Largest zip opened by the search.
const SEARCH_ZIP_MAX: u64 = 1024 * 1024 * 1024;

fn ext_of(name: &str) -> String {
    name.rsplit_once('.').map(|(_, e)| format!(".{}", e.to_ascii_lowercase())).unwrap_or_default()
}

/// Looks for the player's copy under `roots` (depth-limited, no symlinks). Candidates are files with one of the
/// recipe's extensions (and its size, when given), loose or inside a `.zip`; those whose name holds one of `names`
/// are checked first. Returns the first one whose SHA-1 the recipe accepts.
pub fn search(c: &OwnCopy, roots: &[PathBuf], lim: SearchLimits) -> Found {
    let started = Instant::now();
    let exts: Vec<String> = c.rom.extensions.iter().map(|e| e.to_ascii_lowercase()).collect();
    let size_ok = |n: u64| c.rom.size.map_or(n <= OWN_MAX_BYTES, |s| s == n);
    let mut cands: Vec<OwnSource> = vec![];
    let mut zips: Vec<PathBuf> = vec![];
    let mut seen = 0usize;
    let mut stack: Vec<(PathBuf, usize)> = roots.iter().rev().map(|r| (r.clone(), 0)).collect();
    while let Some((dir, depth)) = stack.pop() {
        if seen >= lim.entries || started.elapsed() > lim.time {
            break;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            seen += 1;
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_symlink() {
                continue;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            if ft.is_dir() {
                let low = name.to_ascii_lowercase();
                if depth < lim.depth && !name.starts_with('.') && !SKIP_DIRS.contains(&low.as_str()) {
                    stack.push((e.path(), depth + 1));
                }
                continue;
            }
            let ext = ext_of(&name);
            let Ok(len) = e.metadata().map(|m| m.len()) else { continue };
            if exts.contains(&ext) && size_ok(len) {
                cands.push(OwnSource::file(e.path()));
            } else if ext == ".zip" && len <= SEARCH_ZIP_MAX {
                zips.push(e.path());
            }
        }
    }
    for z in zips {
        cands.extend(zip_candidates(c, &z));
    }
    // Likely names first (case-insensitive), then by path for a stable order.
    let names: Vec<String> = c.names.iter().map(|n| n.to_lowercase()).collect();
    let rank = |s: &OwnSource| {
        let n = s.display().to_lowercase();
        usize::from(!names.iter().any(|x| n.contains(x.as_str())))
    };
    cands.sort_by_key(|s| (rank(s), s.display()));
    let mut out = Found { game: c.game.clone(), label: c.label.clone(), ..Default::default() };
    for s in cands.into_iter().take(lim.candidates) {
        match read_checked(c, &s, None) {
            Ok(g) if g.ok => {
                out.found = Some(s);
                return out;
            }
            _ => out.rejected.push(s.entry.clone().unwrap_or_else(|| s.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn n64_orders_normalize_to_z64() {
        let z = [0x80u8, 0x37, 0x12, 0x40, 1, 2, 3, 4];
        let mut v = [0x37u8, 0x80, 0x40, 0x12, 2, 1, 4, 3];
        let mut n = [0x40u8, 0x12, 0x37, 0x80, 4, 3, 2, 1];
        assert_eq!(n64_order(&z), Some(N64Order::Z64));
        assert_eq!(n64_order(&v), Some(N64Order::V64));
        assert_eq!(n64_order(&n), Some(N64Order::N64));
        assert_eq!(n64_order(b"PK\x03\x04"), None);
        n64_normalize(&mut v, N64Order::V64);
        n64_normalize(&mut n, N64Order::N64);
        assert_eq!(v, z);
        assert_eq!(n, z);
    }
}
