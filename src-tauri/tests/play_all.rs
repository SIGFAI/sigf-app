//! The Play button for every mashup (docs/RECIPE-FORMAT.md section 4): `launch[].me3` (me3 with the recipe's `.me3`
//! profile, offline), `launch[].app_exe` (a program the recipe installs into `{app}`) and `requires_files` (a
//! prerequisite the player installs, checked before install and Play). Recipes are built here around stub files: no
//! game, me3, loader or store is ever started; me3 is only ever a command line.

use serde_json::{json, Value};
use sigf_app_lib::install::{check, fetch, Engine, InstallError, InstalledMod, Recipe};
use sigf_app_lib::launch;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

fn hex(b: &[u8]) -> String {
    fetch::hex(&<sha2::Sha256 as sha2::Digest>::digest(b))
}

/// A zip of `entries` at `path`, and its `contents` list.
fn zip(path: &Path, entries: &[(&str, &[u8])]) -> Vec<Value> {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for (name, body) in entries {
        z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(body).unwrap();
    }
    z.finish().unwrap();
    entries.iter().map(|(n, b)| json!({ "path": n, "sha256": hex(b) })).collect()
}

struct Env {
    _t: tempfile::TempDir,
    root: PathBuf,
    engine: Engine,
}

fn env() -> Env {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().to_path_buf();
    let mut engine = Engine::new(root.join("SIGF"), None);
    engine.allow_local = true; // local zips (dev mode)
    engine.docs = None;
    Env { _t: t, root, engine }
}

fn game_dir(e: &Env, name: &str, files: &[&str]) -> PathBuf {
    let d = e.root.join("games").join(name);
    std::fs::create_dir_all(&d).unwrap();
    for f in files {
        let p = d.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"stub").unwrap();
    }
    d
}

fn dirs(list: &[(&str, &Path)]) -> HashMap<String, String> {
    list.iter().map(|(g, p)| (g.to_string(), p.to_string_lossy().into_owned())).collect()
}

/// ER Mario's shape: the author's folder unpacked into `{app}` (root `ER-Mario`), started through the player's me3.
fn er_mario(e: &Env) -> Value {
    let z = e.root.join("src/er-mario-eldenring.zip");
    let contents = zip(&z, &[("ER-Mario/er_mario.dll", b"dll"), ("ER-Mario/er-mario.me3", b"profileVersion = \"v1\""), ("ER-Mario/README.md", b"readme")]);
    json!({ "id": "sigf/er-mario", "version": "0.4.0", "name": "ER Mario", "kind": "mashup",
        "games": [{ "game": "eldenring", "role": "host" }, { "game": "sm64", "role": "guest" }],
        "requires": [{ "id": "me3", "version": "0.13.0", "page": "https://github.com/garyttierney/me3/releases/tag/v0.13.0" }],
        "install": [{ "game": "eldenring", "strategy": "profile", "files": [{ "src": z.to_string_lossy(), "dst": "{app}", "root": "ER-Mario",
            "unpack": true, "contents": contents, "sha256": fetch::sha256_file(&z).unwrap() }] }],
        "launch": [{ "game": "eldenring", "me3": { "profile": "{app}/er-mario.me3" } }],
        "files": [] })
}

fn parse(v: &Value) -> Recipe {
    Recipe::parse(&v.to_string()).unwrap()
}

fn install(e: &Env, v: &Value, d: &HashMap<String, String>) -> Result<InstalledMod, InstallError> {
    e.engine.install(&parse(v), d, &mut |_| {})
}

#[test]
fn me3_launch_with_the_players_me3() {
    let e = env();
    let er = game_dir(&e, "ELDEN RING", &["Game/eldenring.exe"]);
    let d = dirs(&[("eldenring", &er)]);
    let m = install(&e, &er_mario(&e), &d).unwrap();
    let g = &m.games[0];
    let me3 = g.me3.as_ref().expect("me3 recorded in installed.json");
    assert!(g.exe.is_none());
    assert_eq!(me3.game, "eldenring");
    assert_eq!(me3.profile.path, "er-mario.me3");
    assert_eq!(Path::new(&me3.profile.dir), e.engine.home.join("profiles/sigf-er-mario/eldenring"));
    assert!(me3.exe.is_none());
    assert_eq!(me3.hint.as_deref(), Some("install ME3 from github.com/garyttierney/me3/releases/tag/v0.13.0"));
    assert_eq!(e.engine.installed()[0].games[0].me3.as_ref(), Some(me3), "kept in installed.json");

    // Play: the player's me3 (a stub file) with the installed profile, offline. Nothing is started here.
    let local = e.root.join("LocalAppData");
    let stub = launch::me3_installed_path(&local).unwrap();
    assert!(launch::me3_command(me3, Some(&stub)).unwrap_err().starts_with("me3 not found: install ME3 from "));
    std::fs::create_dir_all(stub.parent().unwrap()).unwrap();
    std::fs::write(&stub, b"stub me3").unwrap();
    let c = launch::me3_command(me3, Some(&stub)).unwrap();
    let profile = e.engine.home.join("profiles").join("sigf-er-mario").join("eldenring").join("er-mario.me3");
    assert_eq!(c.args, ["launch", "--game", "eldenring", "--profile", &profile.to_string_lossy(), "--online", "false"]);

    e.engine.restore("sigf/er-mario", false).unwrap();
    assert!(!profile.exists());
    assert!(launch::me3_command(me3, Some(&stub)).is_err(), "no profile after Restore");
}

#[test]
fn me3_launch_with_a_me3_the_recipe_ships_and_eldenkill_flags() {
    let e = env();
    let er = game_dir(&e, "ELDEN RING", &["Game/eldenring.exe"]);
    let d = dirs(&[("eldenring", &er)]);
    let z = e.root.join("src/me3.zip");
    let me3_contents = zip(&z, &[("bin/me3.exe", b"me3 stub"), ("bin/me3_mod_host.dll", b"host")]);
    let p = e.root.join("src/mod.zip");
    let mod_contents = zip(&p, &[("EldenKill/eldenkill.me3", b"profile"), ("EldenKill/eldenkill.dll", b"dll")]);
    let v = json!({ "id": "sigf/eldenkill", "version": "0.1.0", "name": "EldenKill", "kind": "passthrough",
        "games": [{ "game": "eldenring", "role": "host" }],
        "install": [{ "game": "eldenring", "strategy": "game-dir-snapshot", "files": [
            { "src": z.to_string_lossy(), "dst": "{game}/SIGF/me3", "unpack": true, "contents": me3_contents, "sha256": fetch::sha256_file(&z).unwrap() },
            { "src": p.to_string_lossy(), "dst": "{game}", "unpack": true, "contents": mod_contents, "sha256": fetch::sha256_file(&p).unwrap() }] }],
        "launch": [{ "game": "eldenring", "me3": { "profile": "{game}/EldenKill/eldenkill.me3", "exe": "{game}/SIGF/me3/bin/me3.exe",
            "savefile": "EldenKill.sl2", "disable_arxan": true } }],
        "files": [] });
    let m = install(&e, &v, &d).unwrap();
    let me3 = m.games[0].me3.clone().unwrap();
    assert_eq!(me3.exe.as_ref().unwrap().sha256.as_deref(), Some(hex(b"me3 stub").as_str()));
    let c = launch::me3_command(&me3, None).unwrap();
    assert_eq!(c.exe, er.join("SIGF/me3/bin/me3.exe"));
    assert_eq!(&c.args[5..], ["--savefile", "EldenKill.sl2", "--disable-arxan", "--online", "false"]);
    assert_eq!(c.cwd, er.join("EldenKill"));
    std::fs::write(er.join("SIGF/me3/bin/me3.exe"), b"swapped").unwrap();
    assert!(launch::me3_command(&me3, None).unwrap_err().contains("changed since the install"));
    e.engine.restore("sigf/eldenkill", true).unwrap();
}

#[test]
fn me3_launch_is_strict() {
    let e = env();
    let er = game_dir(&e, "ELDEN RING", &["Game/eldenring.exe"]);
    let d = dirs(&[("eldenring", &er)]);
    let cases: Vec<(&str, Box<dyn Fn(&mut Value)>)> = vec![
        ("profile not installed", Box::new(|v| v["launch"][0]["me3"]["profile"] = json!("{app}/other.me3"))),
        ("profile escape", Box::new(|v| v["launch"][0]["me3"]["profile"] = json!("{app}/../x/er-mario.me3"))),
        ("profile not .me3", Box::new(|v| v["launch"][0]["me3"]["profile"] = json!("{app}/README.md"))),
        ("profile in docs", Box::new(|v| v["launch"][0]["me3"]["profile"] = json!("{docs}/er-mario.me3"))),
        ("profile absolute", Box::new(|v| v["launch"][0]["me3"]["profile"] = json!("C:/x/er-mario.me3"))),
        ("exe not installed", Box::new(|v| v["launch"][0]["me3"]["exe"] = json!("{game}/me3/bin/me3.exe"))),
        ("exe not me3", Box::new(|v| v["launch"][0]["me3"]["exe"] = json!("{app}/er_mario.dll"))),
        ("savefile path", Box::new(|v| v["launch"][0]["me3"]["savefile"] = json!("../ER0000.sl2"))),
        ("savefile kind", Box::new(|v| v["launch"][0]["me3"]["savefile"] = json!("x.exe"))),
        ("args", Box::new(|v| v["launch"][0]["args"] = json!(["--online", "true"]))),
        ("with exe", Box::new(|v| v["launch"][0]["exe"] = json!("Game/eldenring.exe"))),
        ("other game", Box::new(|v| {
            v["games"][0]["game"] = json!("skyrim");
            v["install"][0]["game"] = json!("skyrim");
            v["launch"][0]["game"] = json!("skyrim");
        })),
    ];
    for (name, f) in cases {
        let mut v = er_mario(&e);
        f(&mut v);
        let mut d = d.clone();
        d.insert("skyrim".into(), er.to_string_lossy().into_owned());
        let err = install(&e, &v, &d).err();
        assert!(matches!(err, Some(InstallError::Recipe { .. } | InstallError::PathTraversal { .. })), "{name}: {err:?}");
        assert!(e.engine.installed().is_empty(), "{name}");
        // The app's own recipe rule refuses it too (dev mode: local files).
        assert!(check::check_recipe(&sigf(&v), true).is_err(), "{name}: check_recipe");
    }
    assert!(check::check_recipe(&sigf(&er_mario(&e)), true).is_ok());
}

/// A local test recipe made to pass the catalog's shape rule (source, sizes, file names), dev mode.
fn sigf(v: &Value) -> String {
    let mut v = v.clone();
    let id = v["id"].as_str().unwrap().trim_start_matches("sigf/").to_string();
    v["source"] = json!({ "repo": format!("https://github.com/SIGFAI/{id}"), "license": "MIT" });
    let mut files = vec![];
    for s in v["install"].as_array_mut().unwrap() {
        for f in s["files"].as_array_mut().unwrap() {
            let src = f["src"].as_str().unwrap().replace('\\', "/");
            let url = format!("file:///{}", src.trim_start_matches('/'));
            let size = std::fs::metadata(f["src"].as_str().unwrap()).unwrap().len();
            f["url"] = json!(url);
            f["size"] = json!(size);
            files.push(json!({ "name": src.rsplit('/').next().unwrap(), "url": url, "sha256": f["sha256"], "size": size }));
        }
    }
    v["files"] = json!(files);
    v.to_string()
}

#[test]
fn app_exe_starts_a_program_the_recipe_installed() {
    let e = env();
    let mw2 = game_dir(&e, "Call of Duty Modern Warfare 2", &["iw4mp.exe"]);
    let d = dirs(&[("mw2", &mw2)]);
    let z = e.root.join("src/rewrite.zip");
    let contents = zip(&z, &[("Mashup/iw4l.exe", b"iw4l stub"), ("Mashup/skate/convert.exe", b"conv"), ("Mashup/LICENSE", b"Apache-2.0")]);
    let mut v = json!({ "id": "sigf/rewrite-2010", "version": "0.4.0", "name": "2010 Rust Rewrite Mashup", "kind": "mashup",
        "games": [{ "game": "mw2", "role": "host" }],
        "install": [{ "game": "mw2", "strategy": "profile", "files": [{ "src": z.to_string_lossy(), "dst": "{app}", "root": "Mashup",
            "unpack": true, "contents": contents, "sha256": fetch::sha256_file(&z).unwrap() }] }],
        "launch": [{ "game": "mw2", "app_exe": "iw4l.exe" }],
        "files": [] });
    assert!(check::check_recipe(&sigf(&v), true).is_ok());
    let m = install(&e, &v, &d).unwrap();
    let x = m.games[0].exe.clone().unwrap();
    assert!(x.own, "no Steam start for a program in the mashup's own folder");
    assert_eq!(x.sha256.as_deref(), Some(hex(b"iw4l stub").as_str()));
    let app = e.engine.home.join("profiles/sigf-rewrite-2010/mw2");
    assert_eq!(launch::resolve_exe(&x).unwrap(), app.join("iw4l.exe"));
    std::fs::write(app.join("iw4l.exe"), b"replaced").unwrap();
    assert!(launch::resolve_exe(&x).unwrap_err().contains("changed since the install"));
    e.engine.restore("sigf/rewrite-2010", false).unwrap();

    for bad in ["{app}/missing.exe", "{app}/LICENSE", "../iw4l.exe", "{game}/iw4mp.exe", "skate\\convert.exe", "C:/Windows/notepad.exe"] {
        v["launch"][0]["app_exe"] = json!(bad);
        assert!(install(&e, &v, &d).is_err(), "{bad}");
        assert!(check::check_recipe(&sigf(&v), true).is_err(), "{bad}: check_recipe");
    }
    v["launch"][0]["app_exe"] = json!("{app}/skate/convert.exe");
    assert!(install(&e, &v, &d).is_ok(), "{{app}}/ prefix and a sub-folder");
}

fn minenv(e: &Env) -> Value {
    let z = e.root.join("src/minenv-falloutnv.zip");
    let contents = zip(&z, &[("OSL/osl-launch.exe", b"osl"), ("Data/NVSE/Plugins/osl.dll", b"plugin")]);
    json!({ "id": "sigf/minenv", "version": "0.1.0", "name": "MineNV", "kind": "passthrough",
        "games": [{ "game": "falloutnv", "role": "host" }, { "game": "minecraft", "role": "guest" }],
        "requires_files": [{ "id": "xnvse", "game": "falloutnv", "path": "{game}/nvse_loader.exe",
            "message": "Install xNVSE 6.4.9+ first", "page": "https://github.com/xNVSE/NVSE/releases" }],
        "install": [{ "game": "falloutnv", "strategy": "game-dir-snapshot", "files": [{ "src": z.to_string_lossy(), "dst": "{game}",
            "unpack": true, "contents": contents, "sha256": fetch::sha256_file(&z).unwrap() }] }],
        "launch": [{ "game": "falloutnv", "exe": "OSL/osl-launch.exe" }],
        "files": [] })
}

#[test]
fn requires_files_stop_install_and_play_with_the_page() {
    let e = env();
    let nv = game_dir(&e, "Fallout New Vegas", &["FalloutNV.exe"]);
    let d = dirs(&[("falloutnv", &nv)]);
    let v = minenv(&e);
    assert!(check::check_recipe(&sigf(&v), true).is_ok());

    // Without xNVSE: refused before anything is downloaded or written, with the message and the page.
    let err = install(&e, &v, &d).unwrap_err();
    match &err {
        InstallError::MissingFile { id, game, path, message, page } => {
            assert_eq!((id.as_str(), game.as_str(), path.as_str()), ("xnvse", "falloutnv", "{game}/nvse_loader.exe"));
            assert_eq!(message, "Install xNVSE 6.4.9+ first");
            assert_eq!(page, "https://github.com/xNVSE/NVSE/releases");
        }
        other => panic!("expected missingFile, got {other:?}"),
    }
    let json = serde_json::to_value(&err).unwrap();
    assert_eq!(json["kind"], "missingFile");
    assert_eq!(json["page"], "https://github.com/xNVSE/NVSE/releases");
    // The page travels as `page`: the text is the message alone (a join error still names the page).
    assert_eq!(err.to_string(), "Install xNVSE 6.4.9+ first");
    assert_eq!(sigf_app_lib::join::JoinError::from(err.clone()).message, "Install xNVSE 6.4.9+ first: https://github.com/xNVSE/NVSE/releases");
    assert!(!nv.join("OSL").exists() && e.engine.installed().is_empty() && !e.engine.cache_dir().exists());

    // With it: installs, and Play checks it again before anything starts.
    std::fs::write(nv.join("nvse_loader.exe"), b"stub").unwrap();
    let m = install(&e, &v, &d).unwrap();
    assert_eq!(m.requires_files.len(), 1);
    assert_eq!(e.engine.installed()[0].requires_files, m.requires_files, "kept in installed.json");
    launch::check_required(&m.requires_files).unwrap();
    std::fs::remove_file(nv.join("nvse_loader.exe")).unwrap();
    let pe = launch::check_required(&m.requires_files).unwrap_err();
    assert_eq!((pe.kind, pe.message.as_str(), pe.page.as_deref()), ("missingFile", "Install xNVSE 6.4.9+ first", Some("https://github.com/xNVSE/NVSE/releases")));
    e.engine.restore("sigf/minenv", false).unwrap();
}

#[test]
fn requires_files_are_strict() {
    let e = env();
    let nv = game_dir(&e, "Fallout New Vegas", &["FalloutNV.exe", "nvse_loader.exe"]);
    let d = dirs(&[("falloutnv", &nv)]);
    let cases: Vec<(&str, Box<dyn Fn(&mut Value)>)> = vec![
        ("escape", Box::new(|v| v["requires_files"][0]["path"] = json!("{game}/../nvse_loader.exe"))),
        ("no root", Box::new(|v| v["requires_files"][0]["path"] = json!("nvse_loader.exe"))),
        ("app root", Box::new(|v| v["requires_files"][0]["path"] = json!("{app}/nvse_loader.exe"))),
        ("absolute", Box::new(|v| v["requires_files"][0]["path"] = json!("{game}/C:/x.exe"))),
        ("game not in games", Box::new(|v| v["requires_files"][0]["game"] = json!("skyrim"))),
        ("http page", Box::new(|v| v["requires_files"][0]["page"] = json!("http://github.com/xNVSE/NVSE/releases"))),
        ("page with query", Box::new(|v| v["requires_files"][0]["page"] = json!("https://evil.example/?u=https://github.com"))),
        ("page script", Box::new(|v| v["requires_files"][0]["page"] = json!("javascript:alert(1)"))),
        ("no page", Box::new(|v| {
            v["requires_files"][0].as_object_mut().unwrap().remove("page");
        })),
        ("empty message", Box::new(|v| v["requires_files"][0]["message"] = json!(" "))),
        ("long message", Box::new(|v| v["requires_files"][0]["message"] = json!("x".repeat(121)))),
        ("control in message", Box::new(|v| v["requires_files"][0]["message"] = json!("Install\nxNVSE"))),
    ];
    for (name, f) in cases {
        let mut v = minenv(&e);
        f(&mut v);
        assert!(check::check_recipe(&sigf(&v), true).is_err(), "{name}: check_recipe");
        if !name.contains("message") {
            assert!(install(&e, &v, &d).is_err(), "{name}: engine");
        }
    }
    let mut v = minenv(&e);
    v["requires_files"] = json!([]);
    assert!(check::check_recipe(&sigf(&v), true).is_err(), "an empty list is refused, leave the field out");
    let many: Vec<Value> = (0..9).map(|_| minenv(&e)["requires_files"][0].clone()).collect();
    v["requires_files"] = json!(many);
    assert!(check::check_recipe(&sigf(&v), true).is_err(), "at most 8");
}

/// An app that does not know the new fields (0.1.1) reads the same recipes: unknown fields are ignored. Here: the
/// 0.1.2 parser on a recipe with fields it does not know either, and the new fields on a recipe for the old shape.
#[test]
fn new_fields_are_plain_extra_keys() {
    let e = env();
    let mut v = minenv(&e);
    v["launch"][0]["future_kind"] = json!({ "x": 1 });
    v["future_field"] = json!([1, 2]);
    assert!(check::check_recipe(&sigf(&v), true).is_ok());
    let r = parse(&v);
    assert_eq!(r.requires_files.len(), 1);
    // Without them it is the recipe as before: no me3, no app_exe, no prerequisite.
    let mut old = minenv(&e);
    old.as_object_mut().unwrap().remove("requires_files");
    assert!(parse(&old).requires_files.is_empty());
}

/// A game the recipe's launch[] leaves out is started by the mod itself (EldenKill's ULTRAKILL, UltraCraft's): Play
/// never starts it. A recipe with no launch[] at all keeps starting every game, as before.
#[test]
fn games_left_out_of_launch_are_not_started_by_play() {
    let e = env();
    let er = game_dir(&e, "ELDEN RING", &["Game/eldenring.exe"]);
    let uk = game_dir(&e, "ULTRAKILL", &["ULTRAKILL.exe"]);
    let d = dirs(&[("eldenring", &er), ("ultrakill", &uk)]);
    let mut v = er_mario(&e);
    let z = e.root.join("src/plugin.zip");
    let contents = zip(&z, &[("EldenKill.Guest.dll", b"guest")]);
    v["games"] = json!([{ "game": "eldenring", "role": "host" }, { "game": "ultrakill", "role": "guest" }]);
    v["install"].as_array_mut().unwrap().push(json!({ "game": "ultrakill", "strategy": "game-dir-snapshot", "files": [{ "src": z.to_string_lossy(),
        "dst": "{game}/BepInEx/plugins/EldenKill", "unpack": true, "contents": contents, "sha256": fetch::sha256_file(&z).unwrap() }] }));
    let m = install(&e, &v, &d).unwrap();
    let started: Vec<(&str, bool)> = m.games.iter().map(|g| (g.game.as_str(), g.no_start)).collect();
    assert_eq!(started, [("eldenring", false), ("ultrakill", true)]);
    assert!(serde_json::to_string(&m.games[0]).unwrap().find("noStart").is_none(), "written only when set");
    e.engine.restore("sigf/er-mario", false).unwrap();
    v["launch"] = json!([]);
    let m = install(&e, &v, &d).unwrap();
    assert!(m.games.iter().all(|g| !g.no_start));
    e.engine.restore("sigf/er-mario", false).unwrap();
}
