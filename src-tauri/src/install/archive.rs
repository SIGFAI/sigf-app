//! Archives the engine unpacks: zip (`zip`) and 7z (`sevenz-rust2`, Apache-2.0, pure Rust), told apart by their
//! first bytes, never by their name. Both go through the same rules: every entry name made a plain relative path
//! (absolute paths, drive letters and `..` abort the whole unpack: zip slip), written only inside `target`
//! (`resolve_inside` + `ensure_real_parent_inside`), at most `MAX_ENTRIES` entries and `MAX_UNPACKED_BYTES` bytes.
//!
//! 7z also has its header's memory bounded before anything is decoded (`MAX_HEADER_BYTES`, read raw and, when
//! packed, its unpacked size and its coders' dictionaries, `MAX_CODER_MEMORY`); encrypted 7z is refused. Known limit:
//! the data blocks' dictionaries are not bounded (`sevenz-rust2` keeps coder properties private), so a crafted 7z can
//! make the decoder ask for up to 4 GiB (zeroed, lazily committed) and fail the install or, at worst, the app.
//!
//! RAR is not unpacked: the only full decoder is UnRAR, whose license forbids using its code to rebuild the RAR
//! compressor (not an open-source license), and no permissively licensed pure-Rust decoder exists (2026-10).

use super::paths::{self, ensure_real_parent_inside, resolve_inside};
use super::{copy_capped, fetch, Extracted, InstallError, MAX_UNPACKED_BYTES};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Most entries (files and folders) one archive may hold.
pub const MAX_ENTRIES: usize = 100_000;
/// 7z: most bytes the header may take, raw or unpacked.
pub const MAX_HEADER_BYTES: u64 = 64 * 1024 * 1024;
/// 7z: most memory one coder may ask for (LZMA / LZMA2 dictionary, PPMd model). 7-Zip's "Ultra" preset uses 64 MiB.
pub const MAX_CODER_MEMORY: u64 = 1024 * 1024 * 1024;

const SEVEN_Z_MAGIC: [u8; 6] = [b'7', b'z', 0xBC, 0xAF, 0x27, 0x1C];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Zip,
    SevenZ,
    Rar,
}

/// What the file is, from its first bytes: a zip (local header, or the end record of an empty one), a 7z, a RAR
/// (4.x or 5), or None.
pub fn kind(path: &Path) -> Option<Kind> {
    let mut head = [0u8; 8];
    let mut f = File::open(path).ok()?;
    let n = f.read(&mut head).ok()?;
    let head = &head[..n];
    if head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06") {
        Some(Kind::Zip)
    } else if head.starts_with(&SEVEN_Z_MAGIC) {
        Some(Kind::SevenZ)
    } else if head.starts_with(b"Rar!\x1a\x07") {
        Some(Kind::Rar)
    } else {
        None
    }
}

/// An archive the engine unpacks (zip or 7z).
pub fn supported(path: &Path) -> bool {
    matches!(kind(path), Some(Kind::Zip | Kind::SevenZ))
}

fn not_supported(path: &Path) -> InstallError {
    match kind(path) {
        Some(Kind::Rar) => InstallError::recipe(format!(
            "{}: RAR archives are not unpacked (no open-source RAR decoder); only zip and 7z",
            path_name(path)
        )),
        _ => InstallError::recipe(format!("{}: not a zip or 7z archive", path_name(path))),
    }
}

fn path_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// A 7z entry name as a plain relative path (forward slashes, no empty or `.` segment), or None when it is absolute,
/// has a drive or stream (`:`), or climbs out (`..`).
fn enclosed(name: &str) -> Option<String> {
    let n = name.replace('\\', "/");
    if n.starts_with('/') || n.contains(':') || n.contains('\0') {
        return None;
    }
    let mut segs = vec![];
    for s in n.split('/') {
        match s {
            "" | "." => continue,
            ".." => return None,
            s => segs.push(s),
        }
    }
    (!segs.is_empty()).then(|| segs.join("/"))
}

/// The files of an archive (not its folders), as plain relative paths with forward slashes, without unpacking it.
/// An entry that escapes (zip slip) fails the listing like it fails the unpack.
pub fn list(path: &Path) -> Result<Vec<String>, InstallError> {
    match kind(path) {
        Some(Kind::Zip) => {
            let f = File::open(path).map_err(|e| InstallError::io(path, e))?;
            let mut z = zip::ZipArchive::new(f).map_err(|e| InstallError::io(path, e))?;
            if z.len() > MAX_ENTRIES {
                return Err(InstallError::recipe(format!("archive holds more than {MAX_ENTRIES} entries")));
            }
            let mut out = vec![];
            for i in 0..z.len() {
                let e = z.by_index_raw(i).map_err(|e| InstallError::io(path, e))?;
                let Some(rel) = e.enclosed_name() else {
                    return Err(InstallError::PathTraversal { dst: e.name().to_string() });
                };
                if !e.is_dir() {
                    out.push(paths::rel_string(&rel));
                }
            }
            Ok(out)
        }
        Some(Kind::SevenZ) => {
            let mut f = File::open(path).map_err(|e| InstallError::io(path, e))?;
            let a = open_7z(path, &mut f)?;
            let mut out = vec![];
            for e in &a.files {
                let rel = enclosed(e.name()).ok_or_else(|| InstallError::PathTraversal { dst: e.name().to_string() })?;
                if !e.is_directory() && !e.is_anti_item() {
                    out.push(rel);
                }
            }
            Ok(out)
        }
        _ => Err(not_supported(path)),
    }
}

/// Unpacks a zip or 7z under `target`; entries whose names escape it (zip slip) abort the install.
pub(super) fn extract(path: &Path, target: &Path) -> Result<Vec<Extracted>, InstallError> {
    match kind(path) {
        Some(Kind::Zip) => extract_zip(path, target),
        Some(Kind::SevenZ) => extract_7z(path, target),
        _ => Err(not_supported(path)),
    }
}

pub(super) fn extract_zip(zip_path: &Path, target: &Path) -> Result<Vec<Extracted>, InstallError> {
    let f = File::open(zip_path).map_err(|e| InstallError::io(zip_path, e))?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| InstallError::io(zip_path, e))?;
    if z.len() > MAX_ENTRIES {
        return Err(InstallError::recipe(format!("archive holds more than {MAX_ENTRIES} entries")));
    }
    std::fs::create_dir_all(target).map_err(|e| InstallError::io(target, e))?;
    let mut out = vec![];
    let mut budget = MAX_UNPACKED_BYTES;
    for i in 0..z.len() {
        let mut entry = z.by_index(i).map_err(|e| InstallError::io(zip_path, e))?;
        let name = entry.name().to_string();
        let Some(rel) = entry.enclosed_name() else {
            return Err(InstallError::PathTraversal { dst: name });
        };
        let rel = paths::rel_string(&rel);
        let is_dir = entry.is_dir();
        if let Some(x) = write_entry(target, &rel, &name, is_dir, &mut entry, &mut budget)? {
            out.push(x);
        }
    }
    Ok(out)
}

/// Writes one entry at `target/rel` (a folder, or a file read through the archive's byte budget).
fn write_entry(target: &Path, rel: &str, name: &str, is_dir: bool, mut r: &mut dyn Read, budget: &mut u64) -> Result<Option<Extracted>, InstallError> {
    let (abs, _) = resolve_inside(target, "", rel)?;
    if is_dir {
        std::fs::create_dir_all(&abs).map_err(|e| InstallError::io(&abs, e))?;
        return Ok(None);
    }
    if let Some(p) = abs.parent() {
        std::fs::create_dir_all(p).map_err(|e| InstallError::io(p, e))?;
    }
    ensure_real_parent_inside(target, &abs, name)?;
    let mut file = File::create(&abs).map_err(|e| InstallError::io(&abs, e))?;
    copy_capped(&mut r, &mut file, budget, &abs)?;
    drop(file);
    Ok(Some(Extracted { sha256: fetch::sha256_file(&abs)?, rel: rel.to_string(), abs }))
}

fn extract_7z(path: &Path, target: &Path) -> Result<Vec<Extracted>, InstallError> {
    let mut f = File::open(path).map_err(|e| InstallError::io(path, e))?;
    let archive = open_7z(path, &mut f)?;
    // Every name is checked before the first byte is written.
    for e in &archive.files {
        if enclosed(e.name()).is_none() {
            return Err(InstallError::PathTraversal { dst: e.name().to_string() });
        }
    }
    std::fs::create_dir_all(target).map_err(|e| InstallError::io(target, e))?;
    let mut reader = sevenz_rust2::ArchiveReader::from_archive(archive, f, sevenz_rust2::Password::empty());
    let mut out = vec![];
    let mut budget = MAX_UNPACKED_BYTES;
    let mut failed: Option<InstallError> = None;
    let walked = reader.for_each_entries(|entry, r| {
        if entry.is_anti_item() {
            return Ok(true);
        }
        let Some(rel) = enclosed(entry.name()) else {
            failed = Some(InstallError::PathTraversal { dst: entry.name().to_string() });
            return Ok(false);
        };
        match write_entry(target, &rel, entry.name(), entry.is_directory(), r, &mut budget) {
            Ok(x) => {
                out.extend(x);
                Ok(true)
            }
            Err(e) => {
                failed = Some(e);
                Ok(false)
            }
        }
    });
    if let Some(e) = failed {
        return Err(e);
    }
    walked.map_err(|e| InstallError::io(path, e))?;
    Ok(out)
}

/// Reads a 7z's header within the memory bounds (see the module doc).
fn open_7z(path: &Path, f: &mut File) -> Result<sevenz_rust2::Archive, InstallError> {
    preflight_7z(f).map_err(|m| InstallError::recipe(format!("{}: {m}", path_name(path))))?;
    let a = sevenz_rust2::Archive::read(f, &sevenz_rust2::Password::empty()).map_err(|e| InstallError::io(path, e))?;
    if a.files.len() > MAX_ENTRIES {
        return Err(InstallError::recipe(format!("archive holds more than {MAX_ENTRIES} entries")));
    }
    // The data blocks' coder properties are private to the crate: only their method is checked here (encryption).
    for b in &a.blocks {
        for c in &b.coders {
            coder_ok(c.encoder_method_id(), &[]).map_err(|m| InstallError::recipe(format!("{}: {m}", path_name(path))))?;
        }
    }
    Ok(a)
}

// --- 7z header preflight -----------------------------------------------------------------------------------------------
// Only what is needed to bound the header's memory before `sevenz_rust2` reads it: the start header (32 bytes), the
// raw size of the next header and, when that header is itself packed (kEncodedHeader), the unpacked size and coders
// of the block holding it. Layout: 7-Zip's DOC/7zFormat.txt.

const K_END: u8 = 0x00;
const K_PACK_INFO: u8 = 0x06;
const K_UNPACK_INFO: u8 = 0x07;
const K_SIZE: u8 = 0x09;
const K_CRC: u8 = 0x0A;
const K_FOLDER: u8 = 0x0B;
const K_CODERS_UNPACK_SIZE: u8 = 0x0C;
const K_HEADER: u8 = 0x01;
const K_ENCODED_HEADER: u8 = 0x17;

struct Cur<'a> {
    b: &'a [u8],
    i: usize,
}

impl Cur<'_> {
    fn byte(&mut self) -> Result<u8, String> {
        let v = *self.b.get(self.i).ok_or("truncated 7z header")?;
        self.i += 1;
        Ok(v)
    }
    fn bytes(&mut self, n: usize) -> Result<&[u8], String> {
        let end = self.i.checked_add(n).filter(|e| *e <= self.b.len()).ok_or("truncated 7z header")?;
        let s = &self.b[self.i..end];
        self.i = end;
        Ok(s)
    }
    /// 7z's variable-length number (7zFormat.txt, REAL_UINT64).
    fn num(&mut self) -> Result<u64, String> {
        let first = self.byte()? as u64;
        let mut mask = 0x80u64;
        let mut value = 0u64;
        for i in 0..8 {
            if first & mask == 0 {
                return Ok(value | ((first & (mask - 1)) << (8 * i)));
            }
            value |= (self.byte()? as u64) << (8 * i);
            mask >>= 1;
        }
        Ok(value)
    }
    fn count(&mut self, max: u64) -> Result<usize, String> {
        let n = self.num()?;
        if n > max {
            return Err("7z header too large".into());
        }
        Ok(n as usize)
    }
    /// A bit vector of `n` bits, possibly "all defined" (one byte) first.
    fn skip_digests(&mut self, n: usize) -> Result<(), String> {
        let all = self.byte()?;
        let defined = if all != 0 {
            n
        } else {
            let bits = self.bytes(n.div_ceil(8))?;
            (0..n).filter(|i| bits[i / 8] & (0x80 >> (i % 8)) != 0).count()
        };
        self.bytes(defined.checked_mul(4).ok_or("7z header too large")?)?;
        Ok(())
    }
}

fn preflight_7z(f: &mut File) -> Result<(), String> {
    let len = f.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;
    f.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut start = [0u8; 32];
    f.read_exact(&mut start).map_err(|_| "truncated 7z archive")?;
    if start[..6] != SEVEN_Z_MAGIC {
        return Err("not a 7z archive".into());
    }
    let u64_at = |i: usize| u64::from_le_bytes(start[i..i + 8].try_into().unwrap());
    let (offset, size) = (u64_at(12), u64_at(20));
    if size > MAX_HEADER_BYTES {
        return Err(format!("7z header larger than {MAX_HEADER_BYTES} bytes"));
    }
    if 32u64.checked_add(offset).and_then(|o| o.checked_add(size)).is_none_or(|end| end > len) {
        return Err("7z header past the end of the file".into());
    }
    if size == 0 {
        return Ok(()); // an empty archive
    }
    f.seek(SeekFrom::Start(32 + offset)).map_err(|e| e.to_string())?;
    let mut head = vec![0u8; size as usize];
    f.read_exact(&mut head).map_err(|e| e.to_string())?;
    f.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut c = Cur { b: &head, i: 0 };
    match c.byte()? {
        K_HEADER => Ok(()), // the raw header is already bounded by its size
        K_ENCODED_HEADER => encoded_header_ok(&mut c),
        _ => Err("broken 7z header".into()),
    }
}

/// The StreamsInfo of a packed header: every unpack size within `MAX_HEADER_BYTES`, every coder within its bounds.
fn encoded_header_ok(c: &mut Cur) -> Result<(), String> {
    loop {
        match c.byte()? {
            K_END => return Ok(()),
            K_PACK_INFO => {
                c.num()?; // pack position
                let n = c.count(1024)?;
                loop {
                    match c.byte()? {
                        K_END => break,
                        K_SIZE => {
                            for _ in 0..n {
                                c.num()?;
                            }
                        }
                        K_CRC => c.skip_digests(n)?,
                        _ => return Err("broken 7z header".into()),
                    }
                }
            }
            K_UNPACK_INFO => {
                if c.byte()? != K_FOLDER {
                    return Err("broken 7z header".into());
                }
                let folders = c.count(16)?;
                if c.byte()? != 0 {
                    return Err("unsupported 7z header (external folders)".into());
                }
                let mut outs = vec![];
                for _ in 0..folders {
                    let coders = c.count(32)?;
                    let (mut total_in, mut total_out) = (0usize, 0usize);
                    for _ in 0..coders {
                        let flag = c.byte()?;
                        let id = c.bytes((flag & 0x0F) as usize)?.to_vec();
                        let (ins, outs) = if flag & 0x10 != 0 { (c.count(32)?, c.count(32)?) } else { (1, 1) };
                        total_in += ins;
                        total_out += outs;
                        let props = if flag & 0x20 != 0 {
                            let n = c.count(256)?;
                            c.bytes(n)?.to_vec()
                        } else {
                            vec![]
                        };
                        coder_ok(&id, &props)?;
                    }
                    let pairs = total_out.saturating_sub(1);
                    for _ in 0..pairs {
                        c.num()?;
                        c.num()?;
                    }
                    let packed = total_in.saturating_sub(pairs);
                    if packed > 1 {
                        for _ in 0..packed {
                            c.num()?;
                        }
                    }
                    outs.push(total_out);
                }
                if c.byte()? != K_CODERS_UNPACK_SIZE {
                    return Err("broken 7z header".into());
                }
                for n in outs {
                    for _ in 0..n {
                        if c.num()? > MAX_HEADER_BYTES {
                            return Err(format!("7z header unpacks to more than {MAX_HEADER_BYTES} bytes"));
                        }
                    }
                }
                // The rest (folder CRCs, kEnd) is left to the real reader.
                return Ok(());
            }
            _ => return Err("broken 7z header".into()),
        }
    }
}

/// A coder the app decodes, within `MAX_CODER_MEMORY`: AES (encrypted archives) is refused; LZMA / LZMA2 dictionaries
/// and PPMd models are bounded. Other coders (copy, BCJ filters, deflate, bzip2, delta) use small fixed memory;
/// unknown ones are left to the reader, which refuses them.
fn coder_ok(id: &[u8], props: &[u8]) -> Result<(), String> {
    let mem = match id {
        [0x06, 0xF1, 0x07, 0x01] => return Err("encrypted 7z archives are not unpacked".into()),
        // LZMA: lc/lp/pb byte, then the dictionary size (u32 LE).
        [0x03, 0x01, 0x01] => props.get(1..5).map(|d| u32::from_le_bytes(d.try_into().unwrap()) as u64).unwrap_or(0),
        // LZMA2: one byte; 40 = 4 GiB - 1.
        [0x21] => match props.first() {
            Some(&b) if b > 40 => return Err("broken 7z coder".into()),
            Some(40) => u32::MAX as u64,
            Some(&b) => (2 | (b as u64 & 1)) << (b / 2 + 11),
            None => 0,
        },
        // PPMd: order byte, then the model size (u32 LE).
        [0x03, 0x04, 0x01] => props.get(1..5).map(|d| u32::from_le_bytes(d.try_into().unwrap()) as u64).unwrap_or(0),
        _ => 0,
    };
    if mem > MAX_CODER_MEMORY {
        return Err(format!("7z coder needs {mem} bytes of memory, more than the {MAX_CODER_MEMORY} allowed"));
    }
    Ok(())
}

// --- Layout detection (docs/GAME-HUB.md section 4, `detect`) --------------------------------------------------------------
// A mod plan file may carry layout rules: the server only knows the file's name, the core knows its listing. The
// first rule one of whose patterns occurs in the listing decides where the archive goes (`dst`) and which folder of
// it is the mod's root (everything above is stripped, everything outside is left out), or refuses it. A rule with
// several patterns works like Vortex's `stopPatterns`: the shallowest occurrence of any of them sets the root.

/// What a rule looks for in the listing (case-insensitive, whole path segments).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pattern {
    /// `*`: any file; the root is the archive's top.
    Any,
    /// `*.pak`: a file with this ending (lowercase, dot included); the root is the folder holding it.
    Ext(String),
    /// `Data/`, `archive/pc/mod/`: these folders in a row; the root is the folder holding the first one.
    Folder(Vec<String>),
    /// `manifest.json`, `fomod/ModuleConfig.xml`: a file (with the folders before it); the root is the folder holding
    /// the first segment.
    File(Vec<String>),
}

const PATTERN_MAX: usize = 120;

/// A rule's pattern, or None when it is not one of the forms above (segments are plain names: no `..`, `:`, `\`,
/// wildcard inside).
pub fn parse_pattern(s: &str) -> Option<Pattern> {
    if s.is_empty() || s.len() > PATTERN_MAX {
        return None;
    }
    if s == "*" {
        return Some(Pattern::Any);
    }
    let seg_ok = |g: &str| {
        !g.is_empty() && g != "." && g != ".." && !g.chars().any(|c| c.is_control() || "<>:\"\\|?*{}".contains(c))
    };
    if let Some(ext) = s.strip_prefix('*') {
        let ok = ext.len() >= 2 && ext.starts_with('.') && ext[1..].split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || "_-~".contains(c)));
        return ok.then(|| Pattern::Ext(ext.to_ascii_lowercase()));
    }
    let (body, folder) = match s.strip_suffix('/') {
        Some(b) => (b, true),
        None => (s, false),
    };
    let segs: Vec<String> = body.split('/').map(str::to_lowercase).collect();
    if !segs.iter().all(|g| seg_ok(g)) {
        return None;
    }
    Some(if folder { Pattern::Folder(segs) } else { Pattern::File(segs) })
}

/// One rule: its patterns (any of them), how many folders to go up from where one matched (`up`), and the outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub patterns: Vec<Pattern>,
    pub up: usize,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Install into this `{game}/...` destination.
    Place(String),
    /// Refuse with this reason (`fomod`, ...).
    Refuse(String),
}

/// What the rules make of a listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detected {
    /// Install into `dst`, keeping only what is under `root` (None: the whole archive).
    Place { dst: String, root: Option<String> },
    Refuse(String),
    /// The first matching rule finds its root in several places at the same depth (variants, options): which one
    /// the player wants is not something to guess. A few of them, for the message.
    Ambiguous(Vec<String>),
    /// No rule matches.
    NoMatch,
}

/// Where `pattern` occurs in `file` (a listing path): the root folder each occurrence implies, as segments.
fn anchors<'a>(pattern: &Pattern, file: &'a str) -> Vec<Vec<&'a str>> {
    let segs: Vec<&str> = file.split('/').collect();
    let lower: Vec<String> = segs.iter().map(|s| s.to_lowercase()).collect();
    let n = segs.len();
    match pattern {
        Pattern::Any => vec![vec![]],
        Pattern::Ext(e) => {
            let last = &lower[n - 1];
            if last.len() > e.len() && last.ends_with(e.as_str()) {
                vec![segs[..n - 1].to_vec()]
            } else {
                vec![]
            }
        }
        Pattern::File(p) => {
            let k = p.len();
            if n >= k && lower[n - k..] == p[..] {
                vec![segs[..n - k].to_vec()]
            } else {
                vec![]
            }
        }
        Pattern::Folder(p) => {
            let k = p.len();
            // Folders only: the file name itself is not one.
            (0..n.saturating_sub(k)).filter(|i| lower[*i..i + k] == p[..]).map(|i| segs[..i].to_vec()).collect()
        }
    }
}

/// The first rule that matches `files` (an archive listing, `list`), and what it decides.
pub fn detect(files: &[String], rules: &[Layout]) -> Detected {
    for r in rules {
        let mut found: Vec<Vec<&str>> = files
            .iter()
            .flat_map(|f| r.patterns.iter().flat_map(move |p| anchors(p, f)))
            .filter(|a| a.len() >= r.up)
            .map(|mut a| {
                a.truncate(a.len() - r.up);
                a
            })
            .collect();
        if found.is_empty() {
            continue;
        }
        if let Outcome::Refuse(why) = &r.outcome {
            return Detected::Refuse(why.clone());
        }
        let Outcome::Place(dst) = &r.outcome else { unreachable!() };
        let depth = found.iter().map(Vec::len).min().unwrap_or(0);
        found.retain(|a| a.len() == depth);
        found.sort();
        found.dedup();
        if found.len() > 1 {
            return Detected::Ambiguous(found.iter().take(3).map(|a| a.join("/")).collect());
        }
        let root = found[0].join("/");
        return Detected::Place { dst: dst.clone(), root: (!root.is_empty()).then_some(root) };
    }
    Detected::NoMatch
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(list: &[(&str, usize, &str)]) -> Vec<Layout> {
        list.iter()
            .map(|(p, up, dst)| Layout {
                patterns: p.split(' ').map(|p| parse_pattern(p).unwrap_or_else(|| panic!("{p}"))).collect(),
                up: *up,
                outcome: match dst.strip_prefix("refuse:") {
                    Some(why) => Outcome::Refuse(why.into()),
                    None => Outcome::Place(dst.to_string()),
                },
            })
            .collect()
    }
    fn files(l: &[&str]) -> Vec<String> {
        l.iter().map(|s| s.to_string()).collect()
    }
    fn place(dst: &str, root: Option<&str>) -> Detected {
        Detected::Place { dst: dst.into(), root: root.map(Into::into) }
    }

    #[test]
    fn patterns() {
        assert_eq!(parse_pattern("*"), Some(Pattern::Any));
        assert_eq!(parse_pattern("*.PAK"), Some(Pattern::Ext(".pak".into())));
        assert_eq!(parse_pattern("*.partsbnd.dcx"), Some(Pattern::Ext(".partsbnd.dcx".into())));
        assert_eq!(parse_pattern("Data/"), Some(Pattern::Folder(vec!["data".into()])));
        assert_eq!(parse_pattern("archive/pc/mod/"), Some(Pattern::Folder(vec!["archive".into(), "pc".into(), "mod".into()])));
        assert_eq!(parse_pattern("fomod/ModuleConfig.xml"), Some(Pattern::File(vec!["fomod".into(), "moduleconfig.xml".into()])));
        for bad in ["", "*.", "*pak", "*.p*k", "../x", "a/../b", "a//b", "/a", "C:/x", "a\\b", "a/*/b", "*.pa k", &"a".repeat(121)] {
            assert_eq!(parse_pattern(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn detect_unreal_paks() {
        // vortex-games-like UE layout: LogicMods first, then any .pak (with its .ucas/.utoc beside it).
        let r = rules(&[("fomod/ModuleConfig.xml", 0, "refuse:fomod"), ("LogicMods/", 0, "{game}/G/Content/Paks"), ("*.pak", 0, "{game}/G/Content/Paks/~mods")]);
        assert_eq!(detect(&files(&["MyMod_P.pak", "MyMod_P.ucas", "MyMod_P.utoc"]), &r), place("{game}/G/Content/Paks/~mods", None));
        assert_eq!(detect(&files(&["MyMod/G/Content/Paks/~mods/MyMod_P.pak", "MyMod/readme.txt"]), &r), place("{game}/G/Content/Paks/~mods", Some("MyMod/G/Content/Paks/~mods")));
        assert_eq!(detect(&files(&["X/logicmods/Bp.pak"]), &r), place("{game}/G/Content/Paks", Some("X")));
        assert_eq!(detect(&files(&["Option A/a.pak", "Option B/b.pak"]), &r), Detected::Ambiguous(vec!["Option A".into(), "Option B".into()]));
        assert_eq!(detect(&files(&["Main/a.pak", "Main/Optional/b.pak"]), &r), place("{game}/G/Content/Paks/~mods", Some("Main")), "shallowest wins");
        assert_eq!(detect(&files(&["fomod/ModuleConfig.xml", "a/x.pak"]), &r), Detected::Refuse("fomod".into()));
        assert_eq!(detect(&files(&["Scripts/main.lua"]), &r), Detected::NoMatch);
        assert_eq!(detect(&files(&["notapak"]), &r), Detected::NoMatch);
        assert_eq!(detect(&files(&[".pak"]), &r), Detected::NoMatch, "a bare extension is not a file name");
    }

    #[test]
    fn detect_data_roots_and_up() {
        let r = rules(&[("Data/", 0, "{game}"), ("*.esp", 0, "{game}/Data"), ("textures/", 0, "{game}/Data"), ("*", 0, "{game}/Data")]);
        let stop = rules(&[("*.esp *.bsa textures/ meshes/ skse/", 0, "{game}/Data"), ("*", 0, "{game}/Data")]);
        // Vortex-like stop patterns: the shallowest occurrence of any of them is the root.
        assert_eq!(detect(&files(&["Mod/Data/textures/a.dds", "Mod/Data/Deep/x.esp", "Mod/readme.txt"]), &stop), place("{game}/Data", Some("Mod/Data")));
        assert_eq!(detect(&files(&["Data/SKSE/Plugins/a.dll"]), &stop), place("{game}/Data", Some("Data")));
        assert_eq!(detect(&files(&["Main/x.esp", "Optional/textures/a.dds"]), &stop), Detected::Ambiguous(vec!["Main".into(), "Optional".into()]));
        assert_eq!(detect(&files(&["Mod 1.0/Data/Mod.esp", "Mod 1.0/Data/textures/a.dds"]), &r), place("{game}", Some("Mod 1.0")));
        assert_eq!(detect(&files(&["Mod/Mod.esp", "Mod/textures/a.dds"]), &r), place("{game}/Data", Some("Mod")));
        assert_eq!(detect(&files(&["Textures/a.dds"]), &r), place("{game}/Data", None), "case-insensitive");
        assert_eq!(detect(&files(&["textures"]), &r), place("{game}/Data", None), "a file named like a folder is not one");
        assert_eq!(detect(&files(&["readme.txt"]), &r), place("{game}/Data", None));
        // Stardew-like: a manifest one folder down goes into Mods as is; at the top, under the mod's name.
        let r = rules(&[("manifest.json", 1, "{game}/Mods"), ("manifest.json", 0, "{game}/Mods/Name")]);
        assert_eq!(detect(&files(&["[CP] A/manifest.json", "[CP] A/assets/x.png", "[JA] A/manifest.json"]), &r), place("{game}/Mods", None));
        assert_eq!(detect(&files(&["Pack/A/manifest.json", "Pack/B/manifest.json"]), &r), place("{game}/Mods", Some("Pack")));
        assert_eq!(detect(&files(&["manifest.json", "A.dll"]), &r), place("{game}/Mods/Name", None));
        assert_eq!(detect(&files(&[]), &r), Detected::NoMatch);
    }

    fn write_7z(path: &Path, entries: &[(&str, &[u8])]) {
        let mut w = sevenz_rust2::ArchiveWriter::create(path).unwrap();
        for (name, data) in entries {
            if name.ends_with('/') {
                w.push_archive_entry::<&[u8]>(sevenz_rust2::ArchiveEntry::new_directory(name.trim_end_matches('/')), None).unwrap();
            } else {
                w.push_archive_entry(sevenz_rust2::ArchiveEntry::new_file(name), Some(*data)).unwrap();
            }
        }
        w.finish().unwrap();
    }

    #[test]
    fn kinds_by_first_bytes() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("x.zip"); // the name says zip, the bytes say 7z
        write_7z(&p, &[("a.txt", b"a")]);
        assert_eq!(kind(&p), Some(Kind::SevenZ));
        let r = t.path().join("x.rar");
        std::fs::write(&r, b"Rar!\x1a\x07\x01\x00rest").unwrap();
        assert_eq!(kind(&r), Some(Kind::Rar));
        let e = extract(&r, &t.path().join("out")).unwrap_err();
        assert!(e.to_string().contains("RAR"), "{e}");
        assert!(list(&r).is_err());
        let txt = t.path().join("a.txt");
        std::fs::write(&txt, b"PK").unwrap();
        assert_eq!(kind(&txt), None);
        assert!(!supported(&txt));
    }

    #[test]
    fn unpacks_7z_like_zip() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("mod.7z");
        write_7z(&p, &[("Mod/", b""), ("Mod/Content/Paks/~mods/a.pak", b"pak"), ("Mod/readme.txt", b"hi"), ("Mod/empty.txt", b"")]);
        let mut names = list(&p).unwrap();
        names.sort();
        assert_eq!(names, ["Mod/Content/Paks/~mods/a.pak", "Mod/empty.txt", "Mod/readme.txt"]);
        let out = t.path().join("out");
        let got = extract(&p, &out).unwrap();
        assert_eq!(std::fs::read(out.join("Mod/Content/Paks/~mods/a.pak")).unwrap(), b"pak");
        assert_eq!(std::fs::read(out.join("Mod/empty.txt")).unwrap(), b"");
        let pak = got.iter().find(|x| x.rel == "Mod/Content/Paks/~mods/a.pak").unwrap();
        assert_eq!(pak.sha256, fetch::sha256_file(&out.join("Mod/Content/Paks/~mods/a.pak")).unwrap());
        assert_eq!(got.len(), 3, "folders are not files");
    }

    #[test]
    fn seven_z_slip_is_refused() {
        for bad in ["../evil.txt", "a/../../evil.txt", "/abs.txt", "C:/x.txt", "a\\..\\..\\evil.txt", "x:stream"] {
            let t = tempfile::tempdir().unwrap();
            let p = t.path().join("slip.7z");
            write_7z(&p, &[("ok.txt", b"ok"), (bad, b"evil")]);
            let out = t.path().join("out");
            let e = extract(&p, &out).unwrap_err();
            assert!(matches!(e, InstallError::PathTraversal { .. }), "{bad}: {e}");
            assert!(!out.join("ok.txt").exists(), "{bad}: nothing written before the names are checked");
            assert!(!t.path().join("evil.txt").exists());
            assert!(matches!(list(&p).unwrap_err(), InstallError::PathTraversal { .. }), "{bad}");
        }
        assert_eq!(enclosed("./a//b/./c"), Some("a/b/c".into()));
        assert_eq!(enclosed("a\\b"), Some("a/b".into()));
        assert_eq!(enclosed(".."), None);
        assert_eq!(enclosed("./"), None);
    }

    #[test]
    fn zip_slip_is_refused_in_listings_too() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("slip.zip");
        let mut z = zip::ZipWriter::new(File::create(&p).unwrap());
        z.start_file("../evil.txt", zip::write::SimpleFileOptions::default()).unwrap();
        std::io::Write::write_all(&mut z, b"evil").unwrap();
        z.finish().unwrap();
        assert!(matches!(list(&p).unwrap_err(), InstallError::PathTraversal { .. }));
        assert!(matches!(extract(&p, &t.path().join("out")).unwrap_err(), InstallError::PathTraversal { .. }));
    }

    #[test]
    fn seven_z_memory_bounds() {
        assert!(coder_ok(&[0x21], &[24]).is_ok(), "LZMA2 64 MiB");
        assert!(coder_ok(&[0x21], &[40]).is_err(), "LZMA2 4 GiB");
        assert!(coder_ok(&[0x21], &[41]).is_err());
        let mut lzma = vec![0x5D];
        lzma.extend((2u32 << 30).to_le_bytes());
        assert!(coder_ok(&[0x03, 0x01, 0x01], &lzma).is_err(), "LZMA 2 GiB");
        assert!(coder_ok(&[0x06, 0xF1, 0x07, 0x01], &[]).is_err(), "AES");
        // A header claiming more than the file holds.
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("big.7z");
        let mut b = SEVEN_Z_MAGIC.to_vec();
        b.extend([0, 4, 0, 0, 0, 0]);
        b.extend(0u64.to_le_bytes());
        b.extend((MAX_HEADER_BYTES + 1).to_le_bytes());
        b.extend([0u8; 4]);
        std::fs::write(&p, &b).unwrap();
        assert!(extract(&p, &t.path().join("out")).unwrap_err().to_string().contains("header"));
        // A real archive with a packed header passes the preflight.
        let ok = t.path().join("ok.7z");
        write_7z(&ok, &[("a/b.txt", b"x"), ("c.txt", b"y")]);
        preflight_7z(&mut File::open(&ok).unwrap()).unwrap();
    }
}
