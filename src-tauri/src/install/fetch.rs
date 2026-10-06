//! Content-addressed download cache: `cache/<sha256>` (mrpack entries, pinned by sha1/sha512, use
//! `cache/sha1-<hex>` / `cache/sha512-<hex>`). A file only lands under its hash name after the hash checked out;
//! a cache hit is re-hashed anyway to catch disk corruption.
//!
//! Network downloads: https only, starting on `check::DOWNLOAD_HOSTS` and following redirects only to
//! `check::REDIRECT_HOSTS`; capped in size (`FetchOpts::max_bytes`) and time (idle read timeout, total deadline).
//! Which URLs a recipe may name at all is `check::UrlPolicy`, applied by the callers before the first byte.
//! `file://` and local paths only with `FetchOpts::allow_local` (dev mode).

use super::check;
use super::InstallError;
use sha2::{Digest, Sha256, Sha512};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct Fetched {
    pub path: PathBuf,
    /// True when the cached copy already matched and nothing was read from the source.
    pub cached: bool,
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algo {
    Sha1,
    Sha256,
    Sha512,
}

/// A pinned hash: what a download must match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expected {
    pub algo: Algo,
    pub hex: String,
}

impl Expected {
    pub fn new(algo: Algo, hex: &str) -> Result<Self, InstallError> {
        let s = hex.trim().to_ascii_lowercase();
        let len = match algo {
            Algo::Sha1 => 40,
            Algo::Sha256 => 64,
            Algo::Sha512 => 128,
        };
        if s.len() == len && s.bytes().all(|b| b.is_ascii_hexdigit()) {
            Ok(Self { algo, hex: s })
        } else {
            Err(InstallError::recipe(format!("invalid {algo:?} hash: {hex}")))
        }
    }

    pub fn sha256(hex: &str) -> Result<Self, InstallError> {
        Self::new(Algo::Sha256, hex)
    }

    fn cache_name(&self) -> String {
        match self.algo {
            Algo::Sha256 => self.hex.clone(),
            Algo::Sha1 => format!("sha1-{}", self.hex),
            Algo::Sha512 => format!("sha512-{}", self.hex),
        }
    }
}

enum Hasher {
    S1(sha1::Sha1),
    S256(Sha256),
    S512(Sha512),
}

impl Hasher {
    fn new(algo: Algo) -> Self {
        match algo {
            Algo::Sha1 => Self::S1(sha1::Sha1::new()),
            Algo::Sha256 => Self::S256(Sha256::new()),
            Algo::Sha512 => Self::S512(Sha512::new()),
        }
    }
    fn update(&mut self, b: &[u8]) {
        match self {
            Self::S1(h) => h.update(b),
            Self::S256(h) => h.update(b),
            Self::S512(h) => h.update(b),
        }
    }
    fn finish(self) -> String {
        match self {
            Self::S1(h) => hex(&h.finalize()),
            Self::S256(h) => hex(&h.finalize()),
            Self::S512(h) => hex(&h.finalize()),
        }
    }
}

pub fn sha256_file(path: &Path) -> Result<String, InstallError> {
    hash_file(path, Algo::Sha256)
}

pub fn hash_file(path: &Path, algo: Algo) -> Result<String, InstallError> {
    let mut f = File::open(path).map_err(|e| InstallError::io(path, e))?;
    let mut h = Hasher::new(algo);
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf).map_err(|e| InstallError::io(path, e))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finish())
}

/// Limits for one download.
#[derive(Debug, Clone, Copy)]
pub struct FetchOpts {
    /// `file://` URLs and local paths are read (dev mode only, see `check::DEV_LOCAL_ENV`).
    pub allow_local: bool,
    /// The download is refused once it grows past this (the recipe's declared size, else `check::MAX_FILE_BYTES`).
    pub max_bytes: u64,
    /// A player build's download (toolchain, script, inputs): may also start on `check::BUILD_DOWNLOAD_HOSTS` and
    /// follow redirects to `check::BUILD_REDIRECT_HOSTS`. Which URLs are allowed at all is still the caller's rule.
    pub build: bool,
}

impl FetchOpts {
    /// Capped at `declared` bytes when the recipe gives a size, else at `check::MAX_FILE_BYTES`.
    pub fn new(allow_local: bool, declared: Option<u64>) -> Self {
        Self { allow_local, max_bytes: declared.filter(|d| *d > 0).unwrap_or(check::MAX_FILE_BYTES).min(check::MAX_FILE_BYTES), build: false }
    }

    /// A player build's download (see `build`).
    pub fn for_build(allow_local: bool, declared: Option<u64>) -> Self {
        Self { build: true, ..Self::new(allow_local, declared) }
    }
}

/// No byte for this long: the download is dropped.
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);
/// Whole download, however slow: 2 GiB at 200 kB/s still fits.
const TOTAL_TIMEOUT: Duration = Duration::from_secs(3 * 60 * 60);
const MAX_REDIRECTS: usize = 5;

/// The download client: https only, redirects only to `check::REDIRECT_HOSTS`, an idle timeout per read.
fn client(build: bool) -> Result<reqwest::blocking::Client, reqwest::Error> {
    let policy = reqwest::redirect::Policy::custom(move |a| {
        if a.previous().len() >= MAX_REDIRECTS {
            a.error("too many redirects")
        } else if if build { check::build_redirect_ok(a.url()) } else { check::redirect_ok(a.url()) } {
            a.follow()
        } else {
            let to = a.url().host_str().unwrap_or("?").to_string();
            a.error(format!("redirect to {to} refused"))
        }
    });
    reqwest::blocking::Client::builder()
        .user_agent(concat!("sigf-app/", env!("CARGO_PKG_VERSION")))
        .https_only(true)
        .redirect(policy)
        .connect_timeout(Duration::from_secs(30))
        // Blocking client: bounds the wait for the response headers and for every single read, so an idle timeout.
        .timeout(IDLE_TIMEOUT)
        .build()
}

enum Location {
    Local(PathBuf),
    Http(String),
}

fn parse_location(s: &str) -> Result<Location, InstallError> {
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("https://") {
        return Ok(Location::Http(s.to_string()));
    }
    if lower.starts_with("file://") {
        let mut p = percent_decode(&s["file://".len()..]);
        // file:///C:/x -> C:/x on Windows; file:///home/x stays absolute elsewhere.
        if cfg!(windows) && p.starts_with('/') && p.as_bytes().get(2) == Some(&b':') {
            p.remove(0);
        }
        return Ok(Location::Local(PathBuf::from(p)));
    }
    if s.contains("://") {
        return Err(InstallError::recipe(format!("unsupported url scheme: {s}")));
    }
    Ok(Location::Local(PathBuf::from(s)))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Makes sure `cache_dir/<sha256>` holds the file at `location`. `on_bytes(done, total)` reports download progress.
pub fn fetch(
    cache_dir: &Path,
    location: &str,
    sha256: &str,
    opts: &FetchOpts,
    on_bytes: &mut dyn FnMut(u64, Option<u64>),
) -> Result<Fetched, InstallError> {
    fetch_pinned(cache_dir, location, &Expected::sha256(sha256)?, opts, on_bytes)
}

pub fn fetch_pinned(
    cache_dir: &Path,
    location: &str,
    expected: &Expected,
    opts: &FetchOpts,
    on_bytes: &mut dyn FnMut(u64, Option<u64>),
) -> Result<Fetched, InstallError> {
    std::fs::create_dir_all(cache_dir).map_err(|e| InstallError::io(cache_dir, e))?;
    let name = expected.cache_name();
    let dest = cache_dir.join(&name);
    if dest.is_file() {
        if hash_file(&dest, expected.algo)? == expected.hex {
            return Ok(Fetched { path: dest, cached: true });
        }
        let _ = std::fs::remove_file(&dest); // corrupted on disk: fetch again
    }

    let refused = |m: String| InstallError::Download { url: location.into(), message: m };
    let (mut reader, total): (Box<dyn Read>, Option<u64>) = match parse_location(location)? {
        Location::Local(_) if !opts.allow_local => {
            return Err(refused(format!("local files are refused (dev only: {}=1)", check::DEV_LOCAL_ENV)));
        }
        Location::Local(p) => {
            let f = File::open(&p).map_err(|e| refused(e.to_string()))?;
            let len = f.metadata().ok().map(|m| m.len());
            (Box::new(f), len)
        }
        Location::Http(url) => {
            let start_ok = |u: &reqwest::Url| if opts.build { check::build_start_ok(u) } else { check::download_start_ok(u) };
            if !check::canonical_https(&url).is_some_and(|u| start_ok(&u)) {
                return Err(refused("only https downloads from GitHub releases or Modrinth's CDN".into()));
            }
            let dl = |e: reqwest::Error| InstallError::Download { url: url.clone(), message: e.to_string() };
            let resp = client(opts.build).map_err(dl)?.get(&url).send().and_then(|r| r.error_for_status()).map_err(dl)?;
            let len = resp.content_length();
            (Box::new(resp), len)
        }
    };
    if let Some(t) = total.filter(|t| *t > opts.max_bytes) {
        return Err(refused(format!("{t} bytes, more than the {} allowed", opts.max_bytes)));
    }
    let started = std::time::Instant::now();

    let part = cache_dir.join(format!("{name}.part"));
    let result = (|| {
        let mut out = File::create(&part).map_err(|e| InstallError::io(&part, e))?;
        let mut h = Hasher::new(expected.algo);
        let mut buf = vec![0u8; 1 << 16];
        let mut done = 0u64;
        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|e| InstallError::Download { url: location.into(), message: e.to_string() })?;
            if n == 0 {
                break;
            }
            done += n as u64;
            if done > opts.max_bytes {
                return Err(refused(format!("larger than the {} bytes allowed", opts.max_bytes)));
            }
            if started.elapsed() > TOTAL_TIMEOUT {
                return Err(refused("download took too long".into()));
            }
            h.update(&buf[..n]);
            out.write_all(&buf[..n]).map_err(|e| InstallError::io(&part, e))?;
            on_bytes(done, total);
        }
        out.flush().map_err(|e| InstallError::io(&part, e))?;
        let actual = h.finish();
        if actual != expected.hex {
            return Err(InstallError::ShaMismatch { file: location.into(), expected: expected.hex.clone(), actual });
        }
        Ok(())
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    std::fs::rename(&part, &dest).map_err(|e| InstallError::io(&dest, e))?;
    Ok(Fetched { path: dest, cached: false })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls() {
        assert!(matches!(parse_location("https://x/y").unwrap(), Location::Http(_)));
        assert!(parse_location("ftp://x/y").is_err());
        assert!(parse_location("http://github.com/SIGFAI/x/releases/download/v1/a").is_err(), "plain http refused");
        let Location::Local(p) = parse_location("file:///C:/My%20Mods/a.pk3").unwrap() else { panic!() };
        if cfg!(windows) {
            assert_eq!(p, PathBuf::from("C:/My Mods/a.pk3"));
        }
        assert_eq!(percent_decode("a%2"), "a%2");
    }

    fn fixture() -> (tempfile::TempDir, PathBuf, String) {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("a.bin");
        std::fs::write(&src, b"0123456789").unwrap();
        let sha = sha256_file(&src).unwrap();
        (t, src, sha)
    }

    #[test]
    fn local_files_need_dev_mode() {
        let (t, src, sha) = fixture();
        let cache = t.path().join("cache");
        let loc = src.to_string_lossy().into_owned();
        let e = fetch(&cache, &loc, &sha, &FetchOpts::new(false, None), &mut |_, _| {}).err().unwrap();
        assert!(matches!(e, InstallError::Download { .. }), "{e}");
        assert!(!cache.join(&sha).exists());
        fetch(&cache, &loc, &sha, &FetchOpts::new(true, None), &mut |_, _| {}).unwrap();
    }

    #[test]
    fn size_cap_stops_the_download() {
        let (t, src, sha) = fixture();
        let cache = t.path().join("cache");
        let loc = src.to_string_lossy().into_owned();
        let e = fetch(&cache, &loc, &sha, &FetchOpts::new(true, Some(4)), &mut |_, _| {}).err().unwrap();
        assert!(e.to_string().contains("allowed"), "{e}");
        assert!(!cache.join(format!("{sha}.part")).exists() && !cache.join(&sha).exists());
        fetch(&cache, &loc, &sha, &FetchOpts::new(true, Some(10)), &mut |_, _| {}).unwrap();
        assert_eq!(FetchOpts::new(false, Some(check::MAX_FILE_BYTES * 2)).max_bytes, check::MAX_FILE_BYTES);
        assert_eq!(FetchOpts::new(false, Some(0)).max_bytes, check::MAX_FILE_BYTES);
    }

    #[test]
    fn network_downloads_only_from_allowed_hosts() {
        let t = tempfile::tempdir().unwrap();
        let sha = "0".repeat(64);
        for url in ["https://evil.example/a.zip", "https://raw.githubusercontent.com/SIGFAI/x/main/a.zip", "https://github.com:444/SIGFAI/x/releases/download/v1/a"] {
            let e = fetch(t.path(), url, &sha, &FetchOpts::new(false, None), &mut |_, _| {}).err().unwrap();
            assert!(e.to_string().contains("only https downloads"), "{url}: {e}");
        }
    }
}
