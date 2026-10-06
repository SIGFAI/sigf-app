//! Bring your own copy (`own_copies`) and player builds (`player_build`), end to end through the engine, in temp dirs.
//! The "ROM" is placeholder bytes with an N64 header: its SHA-1 is computed here and put in the test recipe, so no real
//! game file is ever involved. The build test runs a tiny script with Git for Windows' `sh` standing in for the pinned
//! w64devkit (dev mode only); it is skipped on a PC without Git. Nothing is downloaded, no game or Prism is started.

use serde_json::{json, Value};
use sigf_app_lib::install::byo::{self, OwnSource, SearchLimits};
use sigf_app_lib::install::check::check_recipe;
use sigf_app_lib::install::fetch::{self, hash_file, Algo};
use sigf_app_lib::install::{Engine, InstallError, Prism, Recipe};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

const ROM_SIZE: usize = 4096;

fn write(p: &Path, body: &[u8]) -> PathBuf {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
    p.to_path_buf()
}

fn zip(path: &Path, entries: &[(&str, &[u8])]) -> PathBuf {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for (name, body) in entries {
        z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(body).unwrap();
    }
    z.finish().unwrap();
    path.to_path_buf()
}

/// A placeholder N64 dump in `.z64` order: the header word, then a counter pattern. Not a game.
fn z64() -> Vec<u8> {
    let mut b: Vec<u8> = (0..ROM_SIZE).map(|i| (i * 7 % 251) as u8).collect();
    b[..4].copy_from_slice(&[0x80, 0x37, 0x12, 0x40]);
    b
}
fn v64(z: &[u8]) -> Vec<u8> {
    z.chunks(2).flat_map(|c| [c[1], c[0]]).collect()
}
fn n64(z: &[u8]) -> Vec<u8> {
    z.chunks(4).flat_map(|c| [c[3], c[2], c[1], c[0]]).collect()
}

fn sha1_of(t: &Path, bytes: &[u8]) -> String {
    let p = write(&t.join("hash-tmp"), bytes);
    let h = hash_file(&p, Algo::Sha1).unwrap();
    std::fs::remove_file(p).unwrap();
    h
}

fn synthetic_mrpack(t: &Path) -> PathBuf {
    let jar = write(&t.join("cdn/demo.jar"), b"jar bytes");
    let index = json!({
        "formatVersion": 1, "game": "minecraft", "versionId": "1.0.0", "name": "BYO Demo",
        "files": [{"path": "mods/demo.jar", "hashes": {"sha1": hash_file(&jar, Algo::Sha1).unwrap(), "sha512": hash_file(&jar, Algo::Sha512).unwrap()},
            "env": {"client": "required", "server": "required"}, "downloads": [jar], "fileSize": 9}],
        "dependencies": {"minecraft": "1.21.4", "fabric-loader": "0.16.10"}
    });
    zip(&t.join("src/byo-demo.mrpack"), &[("modrinth.index.json", index.to_string().as_bytes())])
}

fn file_url(p: &Path) -> String {
    format!("file:///{}", p.to_string_lossy().replace('\\', "/").trim_start_matches('/'))
}

/// A recipe with one own copy (into the Prism instance) and, when `build` is given, one player build.
fn recipe_json(t: &Path, sha1: &str, build: Option<Value>) -> Value {
    let pack = synthetic_mrpack(t);
    let url = file_url(&pack);
    let mut v = json!({
        "id": "sigf/byo-demo", "version": "0.1.0", "name": "BYO Demo", "kind": "mashup",
        "games": [{ "game": "minecraft", "role": "host" }, { "game": "demo64", "role": "guest", "label": "Demo 64" }],
        "install": [{ "game": "minecraft", "strategy": "mrpack", "pack": { "src": "byo-demo.mrpack", "url": url, "sha256": fetch::sha256_file(&pack).unwrap(), "size": std::fs::metadata(&pack).unwrap().len() } }],
        "own_copies": [{ "game": "demo64", "label": "Demo 64 (USA)", "names": ["demo 64"], "step": "minecraft",
            "rom": { "as": "baserom.us.z64", "sha1": [sha1], "extensions": [".z64", ".v64", ".n64"], "size": ROM_SIZE, "format": "n64" },
            "to": "{instance}/.minecraft/config/demo" }],
        "launch": [{ "game": "minecraft" }],
        "files": [{ "name": "byo-demo.mrpack", "url": url, "sha256": fetch::sha256_file(&pack).unwrap(), "size": std::fs::metadata(&pack).unwrap().len() }],
        "source": { "repo": "https://github.com/SIGFAI/byo-demo", "license": "MIT" }
    });
    if let Some(b) = build {
        v["player_build"] = json!([b]);
    }
    v
}

struct Env {
    _t: tempfile::TempDir,
    root: PathBuf,
    prism: PathBuf,
    engine: Engine,
}

fn env() -> Env {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().to_path_buf();
    let prism = root.join("PrismLauncher");
    std::fs::create_dir_all(prism.join("instances")).unwrap();
    let mut engine = Engine::new(root.join("home"), Some(Prism { data_dir: prism.clone(), exe: None }));
    engine.allow_local = true;
    Env { _t: t, root, prism, engine }
}

fn parse(v: &Value) -> Recipe {
    // The app's own rule (dev mode for the local files) accepts it, then the engine reads it.
    check_recipe(&v.to_string(), true).unwrap()
}

#[test]
fn own_copy_is_checked_normalized_placed_and_removed_by_restore() {
    let mut e = env();
    let z = z64();
    let sha1 = sha1_of(&e.root, &z);
    let r = parse(&recipe_json(&e.root, &sha1, None));
    let inst = e.prism.join("instances/sigf-byo-demo");
    let placed = inst.join(".minecraft/config/demo/baserom.us.z64");

    // No copy: refused before anything is downloaded or written.
    match e.engine.install(&r, &HashMap::new(), &mut |_| {}) {
        Err(InstallError::OwnCopyMissing { game, label }) => assert_eq!((game.as_str(), label.as_str()), ("demo64", "Demo 64 (USA)")),
        other => panic!("{other:?}"),
    }
    assert!(!e.engine.home.join("cache").exists() && !inst.exists());

    // A wrong dump (another file of the right size): a clear mismatch, nothing written.
    let mut other = z.clone();
    other[100] ^= 0xff;
    let bad = write(&e.root.join("roms/Demo 64 (Europe).z64"), &other);
    e.engine.own.insert("demo64".into(), OwnSource::file(&bad));
    match e.engine.install(&r, &HashMap::new(), &mut |_| {}) {
        Err(err @ InstallError::OwnCopyMismatch { .. }) => {
            let msg = err.to_string();
            assert!(msg.contains("Demo 64 (USA)") && msg.contains("Demo 64 (Europe).z64"), "{msg}");
        }
        other => panic!("{other:?}"),
    }
    assert!(!inst.exists());

    // A byte-swapped .v64 dump of the right game: normalized to .z64 order, then placed.
    let good = write(&e.root.join("roms/Demo 64 (USA).v64"), &v64(&z));
    e.engine.own.insert("demo64".into(), OwnSource::file(&good));
    let m = e.engine.install(&r, &HashMap::new(), &mut |_| {}).unwrap();
    assert_eq!(std::fs::read(&placed).unwrap(), z, "written in .z64 order");
    assert_eq!(m.placed.iter().map(PathBuf::from).collect::<Vec<_>>(), vec![placed.clone()]);
    assert_eq!(std::fs::read(inst.join(".minecraft/mods/demo.jar")).unwrap(), b"jar bytes");
    assert!(std::fs::read(&good).is_ok(), "the player's own file is left where it was");
    // The copy never reaches the download cache.
    for f in std::fs::read_dir(e.engine.home.join("cache")).unwrap().flatten() {
        assert_ne!(std::fs::read(f.path()).unwrap(), z);
    }

    e.engine.restore("sigf/byo-demo", false).unwrap();
    assert!(!placed.exists() && !inst.exists());
    assert!(e.engine.installed().is_empty());
    assert!(good.exists());
}

#[test]
fn search_finds_loose_and_zipped_dumps_and_names_the_wrong_ones() {
    let e = env();
    let z = z64();
    let sha1 = sha1_of(&e.root, &z);
    let r = parse(&recipe_json(&e.root, &sha1, None));
    let c = &r.own_copies[0];
    let lim = SearchLimits::default();

    let dl = e.root.join("Downloads");
    let mut other = n64(&z);
    other[64] ^= 1;
    write(&dl.join("hack/Demo 64 hack.n64"), &other);
    write(&dl.join("notes.txt"), b"not a rom");
    let f = byo::search(c, std::slice::from_ref(&dl), lim);
    assert!(f.found.is_none());
    assert_eq!(f.rejected, vec!["Demo 64 hack.n64".to_string()]);

    // A little-endian dump inside a zip, a few folders down.
    zip(&dl.join("emu/n64/Demo 64.zip"), &[("readme.txt", b"x"), ("Demo 64 (USA).n64", &n64(&z))]);
    let f = byo::search(c, std::slice::from_ref(&dl), lim);
    let found = f.found.expect("zipped dump found");
    assert_eq!(found.entry.as_deref(), Some("Demo 64 (USA).n64"));
    byo::verify(c, &found).unwrap();

    // Too deep or in a skipped folder: not searched.
    let deep = e.root.join("deep");
    write(&deep.join("a/b/c/d/e/f/Demo 64.z64"), &z);
    write(&deep.join("AppData/Demo 64.z64"), &z);
    assert!(byo::search(c, &[deep], lim).found.is_none());

    // The player picked a zip: its matching entry.
    let picked = zip(&e.root.join("pick.zip"), &[("Demo 64.v64", &v64(&z))]);
    assert_eq!(byo::search_zip(c, &picked).and_then(|s| s.entry).as_deref(), Some("Demo 64.v64"));
}

/// Git for Windows' sh, standing in for w64devkit's in dev mode.
fn git_sh_dir() -> Option<PathBuf> {
    ["C:/Program Files/Git/usr/bin", "C:/Program Files (x86)/Git/usr/bin"].iter().map(PathBuf::from).find(|d| d.join("sh.exe").is_file())
}

fn build_block(t: &Path, script: &[u8]) -> Value {
    let sh = write(&t.join("rel/build-demo.sh"), script);
    let hello = write(&t.join("up/hello.txt"), b"hello ");
    let src = zip(&t.join("up/src.zip"), &[("demo-src-0123/inner.txt", b"from the archive"), ("demo-src-0123/sub/x.c", b"int x;")]);
    let f = |name: &str, p: &Path| json!({ "name": name, "url": file_url(p), "sha256": fetch::sha256_file(p).unwrap(), "size": std::fs::metadata(p).unwrap().len() });
    let mut srcf = f("src", &src);
    srcf["unpack"] = json!(true);
    srcf["root"] = json!("demo-src-0123");
    json!({ "id": "demo-lib", "label": "Demo's library (demo.dll)", "step": "minecraft", "toolchain": ["w64devkit-2.10.0"],
        "script": f("build-demo.sh", &sh), "inputs": [f("hello.txt", &hello), srcf],
        "outputs": [{ "name": "demo.dll", "to": "{instance}/.minecraft/config/demo" }], "minutes": 1 })
}

#[test]
fn player_build_runs_offline_and_its_output_is_placed_then_restored() {
    let Some(sh) = git_sh_dir() else {
        eprintln!("skipped: no Git for Windows sh on this PC");
        return;
    };
    let mut e = env();
    let z = z64();
    let sha1 = sha1_of(&e.root, &z);
    let script = b"set -eu\ntest -d \"$SIGF_WORK\" && cd \"$SIGF_WORK\"\ncat \"$SIGF_IN/hello.txt\" \"$SIGF_IN/src/inner.txt\" > \"$SIGF_OUT/demo.dll\"\ntest -f \"$SIGF_IN/src/sub/x.c\"\necho built\n";
    let r = parse(&recipe_json(&e.root, &sha1, Some(build_block(&e.root, script))));
    e.engine.own.insert("demo64".into(), OwnSource::file(write(&e.root.join("roms/d.z64"), &z)));
    e.engine.tool_dirs.insert("w64devkit-2.10.0".into(), sh);

    let mut phases = vec![];
    let m = e.engine.install(&r, &HashMap::new(), &mut |p| phases.push(p.phase)).unwrap();
    let dll = e.prism.join("instances/sigf-byo-demo/.minecraft/config/demo/demo.dll");
    assert_eq!(std::fs::read(&dll).unwrap(), b"hello from the archive");
    assert_eq!(m.placed.len(), 2, "{:?}", m.placed);
    assert!(phases.contains(&sigf_app_lib::install::Phase::Build));
    assert!(!e.engine.home.join("build").exists(), "the work folder (sources included) is deleted");

    e.engine.restore("sigf/byo-demo", false).unwrap();
    assert!(!dll.exists());

    // A failing script: nothing installed, the log kept under logs/.
    let script = b"echo compiling; echo 'gcc: error: boom' >&2; exit 3\n";
    let r = parse(&recipe_json(&e.root, &sha1, Some(build_block(&e.root.join("v2"), script))));
    match e.engine.install(&r, &HashMap::new(), &mut |_| {}) {
        Err(InstallError::BuildFailed { id, message, log, .. }) => {
            assert_eq!(id, "demo-lib");
            assert!(message.contains("boom"), "{message}");
            assert!(std::fs::read_to_string(log.unwrap()).unwrap().contains("compiling"));
        }
        other => panic!("{other:?}"),
    }
    assert!(!e.prism.join("instances/sigf-byo-demo").exists());
    assert!(e.engine.installed().is_empty());
}

#[test]
fn tool_overrides_are_dev_mode_only() {
    let Some(sh) = git_sh_dir() else { return };
    let mut e = env();
    let z = z64();
    let sha1 = sha1_of(&e.root, &z);
    let r = parse(&recipe_json(&e.root, &sha1, Some(build_block(&e.root, b"exit 0\n"))));
    e.engine.own.insert("demo64".into(), OwnSource::file(write(&e.root.join("roms/d.z64"), &z)));
    e.engine.tool_dirs.insert("w64devkit-2.10.0".into(), sh);
    e.engine.allow_local = false;
    // Not in dev mode: the local recipe files are refused before the override could matter.
    assert!(matches!(e.engine.install(&r, &HashMap::new(), &mut |_| {}), Err(InstallError::Download { .. })));
}
