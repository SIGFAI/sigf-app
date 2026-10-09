//! Content-addressed download cache: `cache/<sha256>` (mrpack entries, pinned by sha1/sha512, use
//! `cache/sha1-<hex>` / `cache/sha512-<hex>`). A file only lands under its hash name after the hash checked out;
//! a cache hit is re-hashed anyway to catch disk corruption.
//!
//! Network downloads: https only, starting on `check::DOWNLOAD_HOSTS` and following redirects only to
//! `check::REDIRECT_HOSTS`; capped in size (`FetchOpts::max_bytes`) and time (idle read timeout, total deadline).
//! Which URLs a recipe may name at all is `check::UrlPolicy`, applied by the callers before the first byte.
//! `file://` and local paths only with `FetchOpts::allow_local` (dev mode).
//!
//! Mod plans (`crate::mods`) download through `fetch_mod` with `FetchOpts::for_mod`: their source's hosts
//! (`check::MOD_HOSTS`) instead of the lists above, the strongest hash the source gives (md5 included), or none.

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
    /// Mod plans only: some sources (Nexus, CurseForge, mod.io) give nothing stronger.
    Md5,
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
            Algo::Md5 => 32,
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
            Algo::Md5 => format!("md5-{}", self.hex),
            Algo::Sha1 => format!("sha1-{}", self.hex),
            Algo::Sha512 => format!("sha512-{}", self.hex),
        }
    }
}

enum Hasher {
    M5(md5::Md5),
    S1(sha1::Sha1),
    S256(Sha256),
    S512(Sha512),
}

impl Hasher {
    fn new(algo: Algo) -> Self {
        match algo {
            Algo::Md5 => Self::M5(<md5::Md5 as md5::Digest>::new()),
            Algo::Sha1 => Self::S1(sha1::Sha1::new()),
            Algo::Sha256 => Self::S256(Sha256::new()),
            Algo::Sha512 => Self::S512(Sha512::new()),
        }
    }
    fn update(&mut self, b: &[u8]) {
        match self {
            Self::M5(h) => md5::Digest::update(h, b),
            Self::S1(h) => h.update(b),
            Self::S256(h) => h.update(b),
            Self::S512(h) => h.update(b),
        }
    }
    fn finish(self) -> String {
        match self {
            Self::M5(h) => hex(&md5::Digest::finalize(h)),
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
    /// A mod plan's download: starts and redirects only on these hosts (one source's `check::MOD_HOSTS`), a query
    /// allowed (`check::mod_url_ok`). Replaces the recipe and build host lists.
    pub mod_hosts: Option<&'static [&'static str]>,
}

impl FetchOpts {
    /// Capped at `declared` bytes when the recipe gives a size, else at `check::MAX_FILE_BYTES`.
    pub fn new(allow_local: bool, declared: Option<u64>) -> Self {
        Self { allow_local, max_bytes: declared.filter(|d| *d > 0).unwrap_or(check::MAX_FILE_BYTES).min(check::MAX_FILE_BYTES), build: false, mod_hosts: None }
    }

    /// A mod plan's download (see `mod_hosts`): capped at `declared` bytes, else at `check::MOD_MAX_FILE_BYTES`.
    pub fn for_mod(allow_local: bool, declared: Option<u64>, hosts: &'static [&'static str]) -> Self {
        let max = declared.filter(|d| *d > 0).unwrap_or(check::MOD_MAX_FILE_BYTES).min(check::MOD_MAX_FILE_BYTES);
        Self { allow_local, max_bytes: max, build: false, mod_hosts: Some(hosts) }
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
fn client(opts: &FetchOpts) -> Result<reqwest::blocking::Client, reqwest::Error> {
    let (build, mod_hosts) = (opts.build, opts.mod_hosts);
    let policy = reqwest::redirect::Policy::custom(move |a| {
        let ok = match mod_hosts {
            Some(hosts) => check::mod_redirect_ok(hosts, a.url()),
            None if build => check::build_redirect_ok(a.url()),
            None => check::redirect_ok(a.url()),
        };
        if a.previous().len() >= MAX_REDIRECTS {
            a.error("too many redirects")
        } else if ok {
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

    let part = cache_dir.join(format!("{name}.part"));
    let got = download(location, opts, &part, &[expected.algo], on_bytes)?;
    if got[0] != expected.hex {
        let _ = std::fs::remove_file(&part);
        return Err(InstallError::ShaMismatch { file: location.into(), expected: expected.hex.clone(), actual: got[0].clone() });
    }
    std::fs::rename(&part, &dest).map_err(|e| InstallError::io(&dest, e))?;
    Ok(Fetched { path: dest, cached: false })
}

/// A mod plan's file (`crate::mods`): checked against `expected` (the strongest hash its source gives) when there is
/// one, and always stored as `cache/<sha256>`, so the engine then installs it from the cache like any recipe file.
/// Returns the cached path and its sha256 (recorded in installed.json when the source gave no hash).
pub fn fetch_mod(
    cache_dir: &Path,
    location: &str,
    expected: Option<&Expected>,
    opts: &FetchOpts,
    on_bytes: &mut dyn FnMut(u64, Option<u64>),
) -> Result<(PathBuf, String), InstallError> {
    if opts.mod_hosts.is_none() {
        return Err(InstallError::recipe("a mod download needs its source's hosts"));
    }
    if let Some(e) = expected.filter(|e| e.algo == Algo::Sha256) {
        let f = fetch_pinned(cache_dir, location, e, opts, on_bytes)?;
        return Ok((f.path, e.hex.clone()));
    }
    std::fs::create_dir_all(cache_dir).map_err(|e| InstallError::io(cache_dir, e))?;
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let part = cache_dir.join(format!("mod-{}-{nonce}.part", std::process::id()));
    let algos: Vec<Algo> = expected.map(|e| e.algo).into_iter().chain([Algo::Sha256]).collect();
    let got = download(location, opts, &part, &algos, on_bytes)?;
    if let Some(e) = expected {
        if got[0] != e.hex {
            let _ = std::fs::remove_file(&part);
            return Err(InstallError::ShaMismatch { file: location.into(), expected: e.hex.clone(), actual: got[0].clone() });
        }
    }
    let sha = got.last().cloned().unwrap_or_default();
    let dest = cache_dir.join(&sha);
    std::fs::rename(&part, &dest).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        InstallError::io(&dest, e)
    })?;
    Ok((dest, sha))
}

/// Reads `location` into `part` (https on the allowed hosts, or a local file in dev mode) within `opts`' limits, and
/// returns the hex digest of each of `algos`, in order. `part` is removed on any failure.
fn download(location: &str, opts: &FetchOpts, part: &Path, algos: &[Algo], on_bytes: &mut dyn FnMut(u64, Option<u64>)) -> Result<Vec<String>, InstallError> {
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
            let allowed = match opts.mod_hosts {
                Some(hosts) => check::mod_url_ok(hosts, &url),
                None => {
                    let start_ok = |u: &reqwest::Url| if opts.build { check::build_start_ok(u) } else { check::download_start_ok(u) };
                    check::canonical_https(&url).is_some_and(|u| start_ok(&u))
                }
            };
            if !allowed {
                return Err(refused(if opts.mod_hosts.is_some() {
                    "only https downloads from the mod source's own hosts".into()
                } else {
                    "only https downloads from GitHub releases or Modrinth's CDN".into()
                }));
            }
            // The URL may carry a CDN token in its query: errors name the location without it.
            let dl = |e: reqwest::Error| InstallError::Download { url: location.into(), message: e.without_url().to_string() };
            let resp = client(opts).map_err(dl)?.get(&url).send().and_then(|r| r.error_for_status()).map_err(dl)?;
            // Every redirect was checked on the way; the host that answered must still be the source's own.
            if let Some(hosts) = opts.mod_hosts {
                if !check::mod_redirect_ok(hosts, resp.url()) {
                    return Err(refused(format!("answered from {}, not a host of the mod source", resp.url().host_str().unwrap_or("?"))));
                }
            }
            let len = resp.content_length();
            (Box::new(resp), len)
        }
    };
    if let Some(t) = total.filter(|t| *t > opts.max_bytes) {
        return Err(refused(format!("{t} bytes, more than the {} allowed", opts.max_bytes)));
    }
    let started = std::time::Instant::now();
    let result = (|| {
        let mut out = File::create(part).map_err(|e| InstallError::io(part, e))?;
        let mut hashers: Vec<Hasher> = algos.iter().map(|a| Hasher::new(*a)).collect();
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
            for h in &mut hashers {
                h.update(&buf[..n]);
            }
            out.write_all(&buf[..n]).map_err(|e| InstallError::io(part, e))?;
            on_bytes(done, total);
        }
        out.flush().map_err(|e| InstallError::io(part, e))?;
        Ok(hashers.into_iter().map(Hasher::finish).collect())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(part);
    }
    result
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
    fn mod_downloads_check_the_strongest_hash() {
        let (t, src, sha) = fixture();
        let cache = t.path().join("cache");
        let loc = src.to_string_lossy().into_owned();
        let hosts = check::mod_hosts("ts").unwrap();
        let opts = FetchOpts::for_mod(true, None, hosts);
        // b"0123456789"
        let md5 = "781e5e245d69b566979b86e28d23f2c7";
        let sha1 = "87acec17cd9dcd20a716cc2cf67417b71c8a7016";
        let sha512 = "bb96c2fc40d2d54617d6f276febe571f623a8dadf0b734855299b0e107fda32cf6b69f2da32b36445d73690b93cbd0f7bfc20e0f7f28553d2a4428f23b716e90";
        assert_eq!(hash_file(&src, Algo::Md5).unwrap(), md5);
        assert_eq!(hash_file(&src, Algo::Sha1).unwrap(), sha1);
        assert_eq!(hash_file(&src, Algo::Sha512).unwrap(), sha512);
        for (algo, hex) in [(Algo::Md5, md5), (Algo::Sha1, sha1), (Algo::Sha512, sha512), (Algo::Sha256, sha.as_str())] {
            let e = Expected::new(algo, hex).unwrap();
            let (path, got) = fetch_mod(&cache, &loc, Some(&e), &opts, &mut |_, _| {}).unwrap();
            assert_eq!(got, sha, "{algo:?}: stored by sha256");
            assert_eq!(path, cache.join(&sha));
            std::fs::remove_file(&path).unwrap();
        }
        for (algo, len) in [(Algo::Md5, 32), (Algo::Sha1, 40), (Algo::Sha512, 128)] {
            let e = Expected::new(algo, &"0".repeat(len)).unwrap();
            let err = fetch_mod(&cache, &loc, Some(&e), &opts, &mut |_, _| {}).err().unwrap();
            assert!(matches!(err, InstallError::ShaMismatch { .. }), "{algo:?}: {err}");
            assert!(!cache.join(&sha).exists());
        }
        assert!(Expected::new(Algo::Md5, "xyz").is_err());
        assert!(Expected::new(Algo::Md5, &"0".repeat(40)).is_err());
        // No hash from the source: allowed, its sha256 computed.
        let (_, got) = fetch_mod(&cache, &loc, None, &opts, &mut |_, _| {}).unwrap();
        assert_eq!(got, sha);
        assert!(std::fs::read_dir(&cache).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().ends_with(".part")));
        // Local files only in dev mode; a mod download always names its hosts.
        assert!(fetch_mod(&cache, &loc, None, &FetchOpts::for_mod(false, None, hosts), &mut |_, _| {}).is_err());
        assert!(fetch_mod(&cache, &loc, None, &FetchOpts::new(true, None), &mut |_, _| {}).is_err());
        assert!(fetch_mod(&cache, &loc, None, &FetchOpts::for_mod(true, Some(4), hosts), &mut |_, _| {}).is_err());
        assert_eq!(FetchOpts::for_mod(false, None, hosts).max_bytes, check::MOD_MAX_FILE_BYTES);
    }

    #[test]
    fn mod_downloads_only_from_their_source() {
        let t = tempfile::tempdir().unwrap();
        let opts = FetchOpts::for_mod(false, None, check::mod_hosts("ts").unwrap());
        for url in ["https://evil.example/a.zip", "https://github.com/SIGFAI/x/releases/download/v1/a", "http://thunderstore.io/a.zip"] {
            let e = fetch_mod(t.path(), url, None, &opts, &mut |_, _| {}).err().unwrap();
            assert!(matches!(e, InstallError::Download { .. } | InstallError::Recipe { .. }), "{url}: {e}");
        }
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
