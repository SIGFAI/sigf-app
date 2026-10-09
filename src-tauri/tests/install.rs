//! Install engine tests. Everything runs in temp dirs with local files: no network, no real game folder,
//! no game or Prism process is ever started (the Prism tests use a fake data dir and no exe).

use serde_json::json;
use sigf_app_lib::install::fetch::{self, hash_file, Algo, FetchOpts};
use sigf_app_lib::install::{Engine, InstallError, Phase, Prism, Recipe, Strategy};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

/// These tests install from local files: dev mode (`SIGF_DEV_LOCAL_RECIPES=1` in the example CLI).
const DEV: FetchOpts = FetchOpts { allow_local: true, max_bytes: 1 << 30, build: false, mod_hosts: None };

fn dev_engine(home: impl Into<PathBuf>, prism: Option<Prism>) -> Engine {
    let mut e = Engine::new(home, prism);
    e.allow_local = true;
    e
}

fn write(p: &Path, body: &[u8]) -> PathBuf {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
    p.to_path_buf()
}

fn sha(p: &Path) -> String {
    fetch::sha256_file(p).unwrap()
}

fn recipe(v: serde_json::Value) -> Recipe {
    Recipe::parse(&v.to_string()).unwrap()
}

fn no_progress() -> impl FnMut(sigf_app_lib::install::Progress) {
    |_| {}
}

fn dirs(game: &str, dir: &Path) -> HashMap<String, String> {
    HashMap::from([(game.to_string(), dir.to_string_lossy().into_owned())])
}

#[test]
fn spec_example_parses() {
    // A hand-written recipe with the optional fields the publisher does not write (builds, mode, a sourced requirement).
    let r = Recipe::parse(
        r#"{
  "id": "sigf/gta5-minecraft-blocky-los-santos", "version": "1.2.0", "name": "Blocky Los Santos", "kind": "passthrough",
  "games": [
    { "game": "gta5", "role": "host", "builds": { "steam": ["3889"] }, "mode": "story-offline" },
    { "game": "minecraft", "role": "guest", "mc": "1.21.1", "loader": "fabric@0.16.5" }
  ],
  "requires": [ { "id": "scripthookv", "version": "3889", "source": { "url": "...", "sha256": "..." } } ],
  "install": [
    { "game": "gta5", "strategy": "game-dir-snapshot", "files": [ { "src": "GTAxMC.asi", "dst": "{game}/GTAxMC.asi", "sha256": "..." } ] },
    { "game": "minecraft", "strategy": "mrpack", "pack": { "url": "...", "sha256": "..." } }
  ],
  "launch": [ { "game": "minecraft", "wait": "port:25599" }, { "game": "gta5", "args": [] } ],
  "source": { "repo": "https://github.com/SIGFAI/gta5-blocky-los-santos", "license": "MIT" },
  "media": { "cover": "...", "clip": "..." },
  "built_by": { "agent": "GRACEQUOT", "model": "claude-opus-5.5" }
}"#,
    )
    .unwrap();
    assert_eq!(r.slug(), "sigf-gta5-minecraft-blocky-los-santos");
    assert_eq!(r.install[0].strategy, Strategy::GameDirSnapshot);
    assert_eq!(r.install[1].strategy, Strategy::Mrpack);
    assert_eq!(r.games[0].builds["steam"], vec!["3889"]);
    assert!(Recipe::parse(r#"{"id":"x","version":"1","install":[{"game":"g","strategy":"nope"}]}"#).is_err());
}

#[test]
fn sha_mismatch_rejected() {
    let t = tempfile::tempdir().unwrap();
    let src = write(&t.path().join("src/mod.pk3"), b"real bytes");
    let wrong = "0".repeat(64);
    let cache = t.path().join("cache");
    let err = fetch::fetch(&cache, &src.to_string_lossy(), &wrong, &DEV, &mut |_, _| {}).err().unwrap();
    assert!(matches!(err, InstallError::ShaMismatch { ref actual, .. } if *actual == sha(&src)), "{err:?}");
    assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 0, "nothing left in the cache");

    // Through the engine too: the install fails and nothing is registered.
    let engine = dev_engine(t.path().join("home"), None);
    let r = recipe(json!({"id":"sigf/doom-x","version":"1","install":[
        {"game":"doom","strategy":"args","files":[{"src": src, "sha256": wrong}]}]}));
    assert!(matches!(engine.install(&r, &HashMap::new(), &mut no_progress()), Err(InstallError::ShaMismatch { .. })));
    assert!(engine.installed().is_empty());
    assert!(fetch::fetch(&cache, &src.to_string_lossy(), "not-a-hash", &DEV, &mut |_, _| {}).is_err());
}

#[test]
fn cache_hit_skips_download() {
    let t = tempfile::tempdir().unwrap();
    let src = write(&t.path().join("src/mod.pk3"), b"cached bytes");
    let h = sha(&src);
    let cache = t.path().join("cache");
    let url = format!("file:///{}", src.to_string_lossy().replace('\\', "/").trim_start_matches('/'));
    let first = fetch::fetch(&cache, &url, &h.to_uppercase(), &DEV, &mut |_, _| {}).unwrap();
    assert!(!first.cached);
    assert_eq!(first.path, cache.join(&h));
    std::fs::remove_file(&src).unwrap(); // the source is gone: only a cache hit can succeed now
    let second = fetch::fetch(&cache, &url, &h, &DEV, &mut |_, _| {}).unwrap();
    assert!(second.cached);
    // A corrupted cache entry is not trusted.
    std::fs::write(&second.path, b"bit rot").unwrap();
    assert!(matches!(fetch::fetch(&cache, &url, &h, &DEV, &mut |_, _| {}), Err(InstallError::Download { .. })));
}

#[test]
fn args_strategy() {
    let t = tempfile::tempdir().unwrap();
    let src = write(&t.path().join("src/blocky.pk3"), b"wad");
    let home = t.path().join("home");
    let engine = dev_engine(&home, None);
    // `src` names a release asset; the file has no `url` of its own, so it comes from `files[]`.
    let r = recipe(json!({"id":"sigf/doom-blocky","version":"1.0.0","name":"Blocky Doom","install":[
        {"game":"doom","strategy":"args","files":[{"src":"blocky.pk3","dst":"{app}/blocky.pk3","sha256": sha(&src)}]}],
        "files":[{"name":"blocky.pk3","url": src,"sha256": sha(&src),"size":3}],
        "launch":[{"game":"doom","args":["-file","{app}/blocky.pk3","-skill","3"]}]}));
    let mut phases = vec![];
    let m = engine.install(&r, &HashMap::new(), &mut |p| phases.push((p.phase, p.pct))).unwrap();
    let placed = home.join("profiles").join("sigf-doom-blocky").join("doom").join("blocky.pk3");
    assert_eq!(std::fs::read(&placed).unwrap(), b"wad");
    let g = &m.games[0];
    assert_eq!(g.launch_args, vec!["-file".to_string(), placed.to_string_lossy().into_owned(), "-skill".into(), "3".into()]);
    assert_eq!(phases.last(), Some(&(Phase::Ready, 100)));
    for ph in [Phase::Download, Phase::Verify, Phase::Install] {
        assert!(phases.iter().any(|(p, _)| *p == ph), "missing {ph:?}");
    }
    assert!(phases.windows(2).all(|w| w[0].1 <= w[1].1), "pct never goes back");
    assert_eq!(engine.installed()[0].version, "1.0.0");

    engine.restore("sigf/doom-blocky", false).unwrap();
    assert!(!placed.exists());
    assert!(engine.installed().is_empty());
    assert!(matches!(engine.restore("sigf/doom-blocky", false), Err(InstallError::NotInstalled { .. })));
}

/// A fake game folder with one file the mod overwrites and one it leaves alone.
fn fake_game(t: &Path) -> PathBuf {
    let game = t.join("games/gta5");
    write(&game.join("dinput8.dll"), b"original dll");
    write(&game.join("GTA5.exe"), b"exe");
    game
}

fn snapshot_recipe(t: &Path) -> Recipe {
    let asi = write(&t.join("src/GTAxMC.asi"), b"mod asi");
    let dll = write(&t.join("src/dinput8.dll"), b"modded dll");
    recipe(json!({"id":"sigf/gta5-blocky","version":"1","install":[{"game":"gta5","strategy":"game-dir-snapshot","files":[
        {"src": asi, "dst":"{game}/scripts/GTAxMC.asi", "sha256": sha(&asi)},
        {"src": dll, "dst":"{game}/dinput8.dll", "sha256": sha(&dll)}]}]}))
}

#[test]
fn snapshot_round_trip() {
    let t = tempfile::tempdir().unwrap();
    let game = fake_game(t.path());
    let engine = dev_engine(t.path().join("home"), None);
    let r = snapshot_recipe(t.path());

    let m = engine.install(&r, &dirs("gta5", &game), &mut no_progress()).unwrap();
    assert_eq!(std::fs::read(game.join("dinput8.dll")).unwrap(), b"modded dll");
    assert_eq!(std::fs::read(game.join("scripts/GTAxMC.asi")).unwrap(), b"mod asi");
    let snap = PathBuf::from(m.games[0].snapshot.as_ref().unwrap());
    let manifest: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(snap.join("manifest.json")).unwrap()).unwrap();
    let files = manifest["files"].as_array().unwrap();
    let asi = files.iter().find(|f| f["path"] == "scripts/GTAxMC.asi").unwrap();
    let dll = files.iter().find(|f| f["path"] == "dinput8.dll").unwrap();
    assert_eq!(asi["existedBefore"], false);
    assert!(asi["sha256Before"].is_null());
    assert_eq!(dll["existedBefore"], true);
    assert_eq!(dll["sha256Before"], fetch::hex(&<sha2::Sha256 as sha2::Digest>::digest(b"original dll")));
    assert_eq!(dll["sha256After"], sha(&game.join("dinput8.dll")));

    // Re-install of the same id restores first, so the snapshot keeps the true originals.
    engine.install(&r, &dirs("gta5", &game), &mut no_progress()).unwrap();
    assert_eq!(engine.installed().len(), 1);

    engine.restore("sigf/gta5-blocky", false).unwrap();
    assert_eq!(std::fs::read(game.join("dinput8.dll")).unwrap(), b"original dll");
    assert!(!game.join("scripts").exists(), "new file and the folder it needed are gone");
    assert_eq!(std::fs::read(game.join("GTA5.exe")).unwrap(), b"exe");
    assert!(!snap.exists());
    assert!(engine.installed().is_empty());
}

#[test]
fn restore_refuses_after_tampering() {
    let t = tempfile::tempdir().unwrap();
    let game = fake_game(t.path());
    let engine = dev_engine(t.path().join("home"), None);
    engine.install(&snapshot_recipe(t.path()), &dirs("gta5", &game), &mut no_progress()).unwrap();

    std::fs::write(game.join("dinput8.dll"), b"store update").unwrap();
    std::fs::write(game.join("scripts/player-notes.txt"), b"keep me").unwrap();
    match engine.restore("sigf/gta5-blocky", false) {
        Err(InstallError::Tampered { files }) => assert_eq!(files, vec!["gta5: dinput8.dll".to_string()]),
        other => panic!("expected Tampered, got {other:?}"),
    }
    // Refusal changed nothing.
    assert_eq!(std::fs::read(game.join("dinput8.dll")).unwrap(), b"store update");
    assert!(game.join("scripts/GTAxMC.asi").exists());
    assert_eq!(engine.installed().len(), 1);

    engine.restore("sigf/gta5-blocky", true).unwrap();
    assert_eq!(std::fs::read(game.join("dinput8.dll")).unwrap(), b"original dll");
    assert!(!game.join("scripts/GTAxMC.asi").exists());
    assert!(game.join("scripts/player-notes.txt").exists(), "a folder holding player files is kept");
    assert!(engine.installed().is_empty());
}

#[test]
fn path_traversal_rejected() {
    let t = tempfile::tempdir().unwrap();
    let game = fake_game(t.path());
    let evil = write(&t.path().join("src/evil.dll"), b"evil");
    let home = t.path().join("home");
    let engine = dev_engine(&home, None);
    for (strategy, dst) in [
        ("game-dir-snapshot", "{game}/../evil.dll"),
        ("game-dir-snapshot", "C:/Windows/evil.dll"),
        ("game-dir-snapshot", "{game}/sub/../../evil.dll"),
        ("args", "../../evil.dll"),
        ("args", "evil.dll"),
        ("profile", "{app}/../evil.dll"),
        ("profile", "{profile}/evil.dll"),
    ] {
        let r = recipe(json!({"id":"sigf/evil","version":"1","install":[{"game":"gta5","strategy":strategy,"files":[
            {"src": evil, "dst": dst, "sha256": sha(&evil)}]}]}));
        let err = engine.install(&r, &dirs("gta5", &game), &mut no_progress()).err();
        assert!(matches!(err, Some(InstallError::PathTraversal { .. })), "{strategy} {dst}: {err:?}");
    }
    assert!(!t.path().join("games/evil.dll").exists());
    assert!(!home.join("cache").exists(), "rejected before any download");
    assert!(engine.installed().is_empty());
}

#[test]
fn missing_game_dir_is_typed() {
    let t = tempfile::tempdir().unwrap();
    let engine = dev_engine(t.path().join("home"), None);
    let err = engine.install(&snapshot_recipe(t.path()), &HashMap::new(), &mut no_progress()).err();
    assert!(matches!(err, Some(InstallError::MissingGameDir { ref game }) if game == "gta5"));
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

#[test]
fn profile_strategy_unzips() {
    let t = tempfile::tempdir().unwrap();
    let pack = zip(&t.path().join("src/bepinex.zip"), &[("BepInEx/plugins/mod.dll", b"plugin"), ("doorstop_config.ini", b"ini")]);
    let home = t.path().join("home");
    let engine = dev_engine(&home, None);
    let r = recipe(json!({"id":"sigf/lc-x","version":"1","install":[{"game":"lethal-company","strategy":"profile",
        "files":[{"src": pack, "dst": "{app}", "unpack": true, "sha256": sha(&pack)}]}],
        "launch":[{"game":"lethal-company","args":["--doorstop-target","{app}/BepInEx"]}]}));
    let m = engine.install(&r, &HashMap::new(), &mut no_progress()).unwrap();
    let dir = home.join("profiles").join("sigf-lc-x").join("lethal-company");
    assert_eq!(std::fs::read(dir.join("BepInEx/plugins/mod.dll")).unwrap(), b"plugin");
    assert_eq!(m.games[0].launch_args[1], dir.join("BepInEx").to_string_lossy());

    let slip = zip(&t.path().join("src/slip.zip"), &[("../../escaped.txt", b"x")]);
    let r = recipe(json!({"id":"sigf/slip","version":"1","install":[{"game":"g","strategy":"profile",
        "files":[{"src": slip, "dst": "{app}", "unpack": true, "sha256": sha(&slip)}]}]}));
    assert!(matches!(engine.install(&r, &HashMap::new(), &mut no_progress()), Err(InstallError::PathTraversal { .. })));
    assert!(!home.join("escaped.txt").exists() && !home.join("profiles/escaped.txt").exists());
    assert!(!home.join("profiles/sigf-slip").exists(), "failed install leaves no profile");
}

/// A tiny Modrinth pack: one mod jar fetched by hash from a local "CDN", plus overrides.
fn synthetic_mrpack(t: &Path, mod_path: &str) -> PathBuf {
    let jar = write(&t.join("cdn/sodium.jar"), b"jar bytes");
    let index = json!({
        "formatVersion": 1, "game": "minecraft", "versionId": "1.0.0", "name": "Blocky Pack",
        "files": [
            {"path": mod_path, "hashes": {"sha1": hash_file(&jar, Algo::Sha1).unwrap(), "sha512": hash_file(&jar, Algo::Sha512).unwrap()},
             "env": {"client": "required", "server": "required"}, "downloads": [t.join("cdn/dead-mirror.jar"), jar], "fileSize": 9},
            {"path": "mods/server-only.jar", "hashes": {"sha1": "0".repeat(40)}, "env": {"client": "unsupported"}, "downloads": []}
        ],
        "dependencies": {"minecraft": "1.21.1", "fabric-loader": "0.16.5"}
    });
    zip(&t.join("src/blocky.mrpack"), &[
        ("modrinth.index.json", index.to_string().as_bytes()),
        ("overrides/config/blocky.json", b"{\"base\":true}"),
        ("overrides/options.txt", b"base"),
        ("client-overrides/options.txt", b"client"),
    ])
}

fn mc_recipe(pack: &Path) -> Recipe {
    recipe(json!({"id":"sigf/mc-blocky","version":"1","install":[{"game":"minecraft","strategy":"mrpack",
        "pack":{"url": pack, "sha256": sha(pack)}}]}))
}

#[test]
fn mrpack_needs_launcher() {
    let t = tempfile::tempdir().unwrap();
    let pack = synthetic_mrpack(t.path(), "mods/sodium.jar");
    let engine = dev_engine(t.path().join("home"), None);
    match engine.install(&mc_recipe(&pack), &HashMap::new(), &mut no_progress()) {
        Err(InstallError::NeedsLauncher { game, launcher }) => assert_eq!((game.as_str(), launcher.as_str()), ("minecraft", "prism")),
        other => panic!("{other:?}"),
    }
    // Serialized the way the UI receives it.
    let e = serde_json::to_value(sigf_app_lib::install::CommandError::from(InstallError::NeedsLauncher {
        game: "minecraft".into(),
        launcher: "prism".into(),
    }))
    .unwrap();
    assert_eq!(e["kind"], "needsLauncher");
    assert_eq!(e["launcher"], "prism");
    assert!(e["message"].as_str().unwrap().contains("prism"));
}

#[test]
fn mrpack_writes_prism_instance() {
    let t = tempfile::tempdir().unwrap();
    let prism_data = t.path().join("PrismLauncher");
    std::fs::create_dir_all(prism_data.join("instances")).unwrap();
    let pack = synthetic_mrpack(t.path(), "mods/sodium.jar");
    let engine = dev_engine(t.path().join("home"), Some(Prism { data_dir: prism_data.clone(), exe: None }));

    let m = engine.install(&mc_recipe(&pack), &HashMap::new(), &mut no_progress()).unwrap();
    let inst = prism_data.join("instances/sigf-mc-blocky");
    let g = &m.games[0];
    assert_eq!(g.instance.as_deref(), Some("sigf-mc-blocky"));
    assert_eq!(g.launch_args, vec!["--launch".to_string(), "sigf-mc-blocky".into()]);
    assert_eq!(std::fs::read(inst.join(".minecraft/mods/sodium.jar")).unwrap(), b"jar bytes");
    assert!(!inst.join(".minecraft/mods/server-only.jar").exists());
    assert_eq!(std::fs::read(inst.join(".minecraft/config/blocky.json")).unwrap(), b"{\"base\":true}");
    assert_eq!(std::fs::read(inst.join(".minecraft/options.txt")).unwrap(), b"client", "client-overrides win");
    let cfg = std::fs::read_to_string(inst.join("instance.cfg")).unwrap();
    assert!(cfg.contains("name=Blocky Pack") && cfg.contains("InstanceType=OneSix"));
    let mmc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(inst.join("mmc-pack.json")).unwrap()).unwrap();
    let uids: Vec<_> = mmc["components"].as_array().unwrap().iter().map(|c| (c["uid"].as_str().unwrap(), c["version"].as_str().unwrap())).collect();
    assert_eq!(uids, vec![("net.minecraft", "1.21.1"), ("net.fabricmc.intermediary", "1.21.1"), ("net.fabricmc.fabric-loader", "0.16.5")]);
    assert!(!prism_data.join("instances/.sigf-mc-blocky.sigf-tmp").exists());

    engine.restore("sigf/mc-blocky", false).unwrap();
    assert!(!inst.exists());
    assert!(engine.installed().is_empty());

    // A player's own instance with the same name is never overwritten.
    write(&inst.join("instance.cfg"), b"name=Mine");
    assert!(engine.install(&mc_recipe(&pack), &HashMap::new(), &mut no_progress()).is_err());
    assert_eq!(std::fs::read(inst.join("instance.cfg")).unwrap(), b"name=Mine");
}

#[test]
fn mrpack_index_traversal_and_bad_hash() {
    let t = tempfile::tempdir().unwrap();
    let prism_data = t.path().join("PrismLauncher");
    let engine = dev_engine(t.path().join("home"), Some(Prism { data_dir: prism_data.clone(), exe: None }));
    let pack = synthetic_mrpack(t.path(), "../../escaped.jar");
    assert!(matches!(engine.install(&mc_recipe(&pack), &HashMap::new(), &mut no_progress()), Err(InstallError::PathTraversal { .. })));
    assert!(!prism_data.join("escaped.jar").exists() && !prism_data.join("instances/sigf-mc-blocky").exists());

    // A mod jar whose bytes no longer match the index hash: every mirror fails, no instance is left behind.
    let pack = synthetic_mrpack(t.path(), "mods/sodium.jar");
    std::fs::write(t.path().join("cdn/sodium.jar"), b"jar BYTES").unwrap(); // same size, other bytes
    let err = engine.install(&mc_recipe(&pack), &HashMap::new(), &mut no_progress()).err();
    assert!(matches!(err, Some(InstallError::ShaMismatch { .. })), "{err:?}");
    assert!(!prism_data.join("instances/sigf-mc-blocky").exists());
    assert!(engine.installed().is_empty());
}

#[test]
fn home_dir_honors_env() {
    let t = tempfile::tempdir().unwrap();
    // Only this test touches SIGF_HOME; every other test passes its home explicitly.
    std::env::set_var("SIGF_HOME", t.path());
    assert_eq!(sigf_app_lib::install::home_dir(), t.path());
    std::env::remove_var("SIGF_HOME");
    assert!(sigf_app_lib::install::home_dir().ends_with("SIGF"));
}

#[test]
fn launch_exe_recorded_and_checked_inside_the_game_folder() {
    let t = tempfile::tempdir().unwrap();
    let game = fake_game(t.path());
    let engine = dev_engine(t.path().join("home"), None);
    let with_launch = |launch: serde_json::Value| {
        let mut v = serde_json::to_value(snapshot_recipe(t.path())).unwrap();
        v["requires"] = json!([{"id": "skse64", "page": "https://skse.silverlock.org/"}]);
        v["launch"] = launch;
        recipe(v)
    };
    for bad in ["../evil.exe", "{game}/../evil.exe", "C:/Windows/notepad.exe", "{app}/x.exe"] {
        let err = engine.install(&with_launch(json!([{"game":"gta5","exe": bad}])), &dirs("gta5", &game), &mut no_progress()).err();
        assert!(matches!(err, Some(InstallError::PathTraversal { .. })), "{bad}: {err:?}");
    }
    let err = engine.install(&with_launch(json!([{"game":"gta5","exe":"loader.bat"}])), &dirs("gta5", &game), &mut no_progress()).err();
    assert!(matches!(err, Some(InstallError::Recipe { .. })), "{err:?}");
    let err = engine.install(&with_launch(json!([{"game":"gta5","wait":"soon"}])), &dirs("gta5", &game), &mut no_progress()).err();
    assert!(matches!(err, Some(InstallError::Recipe { .. })), "{err:?}");
    assert!(engine.installed().is_empty());

    // The loader may be missing at install (the player adds it later): recorded, checked again at play.
    let m = engine
        .install(&with_launch(json!([{"game":"gta5","exe":"skse64_loader.exe","wait":"port:25599"}])), &dirs("gta5", &game), &mut no_progress())
        .unwrap();
    let g = &m.games[0];
    let x = g.exe.as_ref().unwrap();
    assert_eq!((x.path.as_str(), Path::new(&x.dir)), ("skse64_loader.exe", game.as_path()));
    assert_eq!(x.hint.as_deref(), Some("install SKSE64 from skse.silverlock.org"));
    assert_eq!(g.wait.as_deref(), Some("port:25599"));
    let reloaded = engine.installed();
    assert_eq!(reloaded[0].games[0].exe.as_ref(), Some(x), "kept in installed.json");
    assert_eq!(
        sigf_app_lib::launch::resolve_exe(x).unwrap_err(),
        "skse64_loader.exe not found: install SKSE64 from skse.silverlock.org"
    );
    write(&game.join("skse64_loader.exe"), b"loader");
    assert_eq!(sigf_app_lib::launch::resolve_exe(x).unwrap(), game.join("skse64_loader.exe"));
}

#[test]
fn mrpack_jvm_args_go_to_instance_cfg_and_only_whitelisted() {
    let t = tempfile::tempdir().unwrap();
    let prism_data = t.path().join("PrismLauncher");
    std::fs::create_dir_all(prism_data.join("instances")).unwrap();
    let pack = synthetic_mrpack(t.path(), "mods/sodium.jar");
    let engine = dev_engine(t.path().join("home"), Some(Prism { data_dir: prism_data.clone(), exe: None }));
    let with_args = |args: serde_json::Value| {
        let mut v = serde_json::to_value(mc_recipe(&pack)).unwrap();
        v["install"][0]["jvm_args"] = args;
        recipe(v)
    };
    for bad in ["-javaagent:evil.jar", "-Dfabric.addMods=evil", "-XX:OnOutOfMemoryError=calc", "-Dx=C:/evil"] {
        let err = engine.install(&with_args(json!([bad])), &HashMap::new(), &mut no_progress()).err();
        assert!(matches!(err, Some(InstallError::Recipe { .. })), "{bad}: {err:?}");
    }
    assert!(!t.path().join("home/cache").exists(), "rejected before any download");
    engine.install(&with_args(json!(["-Dfusion.startHidden=true", "-Xmx4G"])), &HashMap::new(), &mut no_progress()).unwrap();
    let cfg = std::fs::read_to_string(prism_data.join("instances/sigf-mc-blocky/instance.cfg")).unwrap();
    assert!(cfg.contains("\nOverrideJavaArgs=true\n") && cfg.contains("\nJvmArgs=\"-Dfusion.startHidden=true -Xmx4G\"\n"), "{cfg}");
}

/// A pack whose one mod jar lists exactly `downloads`.
fn pack_with_downloads(t: &Path, downloads: serde_json::Value) -> PathBuf {
    let jar = write(&t.join("cdn/sodium.jar"), b"jar bytes");
    let index = json!({
        "formatVersion": 1, "game": "minecraft", "versionId": "1.0.0", "name": "Pack",
        "files": [{"path": "mods/sodium.jar", "hashes": {"sha512": hash_file(&jar, Algo::Sha512).unwrap()}, "downloads": downloads, "fileSize": 9}],
        "dependencies": {"minecraft": "1.21.1"}
    });
    zip(&t.join("src/pack.mrpack"), &[("modrinth.index.json", index.to_string().as_bytes())])
}

#[test]
fn engine_refuses_local_files_without_dev_mode() {
    let t = tempfile::tempdir().unwrap();
    let src = write(&t.path().join("src/mod.pk3"), b"bytes");
    let mut engine = Engine::new(t.path().join("home"), None);
    engine.allow_local = false; // what the app's handlers use
    let url = format!("file:///{}", src.to_string_lossy().replace('\\', "/").trim_start_matches('/'));
    for loc in [url, src.to_string_lossy().into_owned()] {
        let r = recipe(json!({"id":"sigf/doom-x","version":"1","install":[{"game":"doom","strategy":"args","files":[{"src": loc, "sha256": sha(&src)}]}]}));
        let err = engine.install(&r, &HashMap::new(), &mut no_progress()).err();
        assert!(matches!(err, Some(InstallError::Download { .. })), "{err:?}");
    }
    assert!(!t.path().join("home/cache").exists() || std::fs::read_dir(t.path().join("home/cache")).unwrap().count() == 0);
    assert!(engine.installed().is_empty());
}

#[test]
fn engine_checks_every_url_before_the_first_download() {
    let t = tempfile::tempdir().unwrap();
    let src = write(&t.path().join("src/mod.pk3"), b"bytes");
    let engine = dev_engine(t.path().join("home"), None);
    let ok_sha = sha(&src);
    for bad in [
        "https://evil.example/x.dll",
        "http://github.com/SIGFAI/doom-x/releases/download/v1/x.dll",
        "https://github.com/SIGFAI/other/releases/download/v1/x.dll",
        "https://github.com/SIGFAI/doom-x/releases/download/%2e%2e/%2e%2e/evil/releases/download/v1/x.dll",
    ] {
        // The local file comes first; it must not be fetched either, since the second URL is refused.
        let r = recipe(json!({"id":"sigf/doom-x","version":"1","install":[{"game":"doom","strategy":"args","files":[
            {"src": src, "sha256": ok_sha},
            {"src": "x.dll", "url": bad, "sha256": "0".repeat(64)}]}]}));
        let err = engine.install(&r, &HashMap::new(), &mut no_progress()).err();
        assert!(matches!(err, Some(InstallError::Download { ref url, .. }) if url == bad), "{bad}: {err:?}");
        assert!(!t.path().join("home/cache").join(&ok_sha).exists(), "nothing fetched for {bad}");
    }
    // requires[] sources and packs are held to the same rule.
    let r = recipe(json!({"id":"sigf/doom-x","version":"1","requires":[{"id":"loader","source":{"url":"https://evil.example/l.dll","sha256":"0".repeat(64)}}],
        "install":[{"game":"doom","strategy":"args","files":[{"src": src, "sha256": ok_sha}]}]}));
    assert!(matches!(engine.install(&r, &HashMap::new(), &mut no_progress()), Err(InstallError::Download { .. })));
    // A declared size above the cap is refused up front.
    let r = recipe(json!({"id":"sigf/doom-x","version":"1","install":[{"game":"doom","strategy":"args","files":[{"src": src, "sha256": ok_sha, "size": 3u64 << 30}]}]}));
    assert!(matches!(engine.install(&r, &HashMap::new(), &mut no_progress()), Err(InstallError::Recipe { .. })));
    // A file larger than its declared size is refused while downloading.
    let r = recipe(json!({"id":"sigf/doom-x","version":"1","install":[{"game":"doom","strategy":"args","files":[{"src": src, "sha256": ok_sha, "size": 2}]}]}));
    assert!(matches!(engine.install(&r, &HashMap::new(), &mut no_progress()), Err(InstallError::Download { .. })));
    assert!(engine.installed().is_empty());
}

#[test]
fn mrpack_index_downloads_allowlisted() {
    let t = tempfile::tempdir().unwrap();
    let prism_data = t.path().join("PrismLauncher");
    std::fs::create_dir_all(prism_data.join("instances")).unwrap();
    let engine = dev_engine(t.path().join("home"), Some(Prism { data_dir: prism_data.clone(), exe: None }));
    let jar = t.path().join("cdn/sodium.jar");
    for bad in [
        json!(["https://evil.example/sodium.jar"]),
        json!(["http://cdn.modrinth.com/data/AANobbMI/versions/x/sodium.jar"]),
        json!(["https://github.com/evil/x/releases/download/v1/sodium.jar"]),
        json!(["https://cdn.modrinth.com/data/%2e%2e/sodium.jar"]),
    ] {
        let pack = pack_with_downloads(t.path(), bad.clone());
        let err = engine.install(&mc_recipe(&pack), &HashMap::new(), &mut no_progress()).err();
        assert!(matches!(err, Some(InstallError::Download { .. })), "{bad}: {err:?}");
        assert!(!prism_data.join("instances/sigf-mc-blocky").exists());
    }
    // A refused mirror is skipped, never contacted, when an allowed one is listed.
    let pack = pack_with_downloads(t.path(), json!(["https://evil.example/sodium.jar", jar]));
    engine.install(&mc_recipe(&pack), &HashMap::new(), &mut no_progress()).unwrap();
    assert_eq!(std::fs::read(prism_data.join("instances/sigf-mc-blocky/.minecraft/mods/sodium.jar")).unwrap(), b"jar bytes");
    engine.restore("sigf/mc-blocky", false).unwrap();
    // Not in dev mode, a local mirror is refused like any other.
    let mut strict = Engine::new(t.path().join("home2"), Some(Prism { data_dir: prism_data.clone(), exe: None }));
    strict.allow_local = false;
    assert!(matches!(strict.install(&mc_recipe(&pack), &HashMap::new(), &mut no_progress()), Err(InstallError::Download { .. })));
}
