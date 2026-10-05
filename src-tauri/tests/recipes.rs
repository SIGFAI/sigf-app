//! Recipes as the SIGF publisher writes them, installed. tests/fixtures/<case>/ holds a mashup.json and its release
//! assets, written by tests/make-fixtures.mjs with placeholder bytes (no real mod, loader or game file); their
//! `file:///FIXTURE/<case>/<file>` URLs are re-pointed at this checkout. Each case: Recipe::parse, install into a temp SIGF home with temp
//! game folders (and a temp Prism data dir with no exe), check what landed and the launch args, then restore and check
//! the game folders are byte for byte what they were. No game, store or Prism process is ever started.

use serde_json::Value;
use sigf_app_lib::install::{Engine, InstallError, InstalledMod, Prism, Recipe, Strategy};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

/// Every `file://` URL in the recipe -> the asset of the same name in this checkout's fixture folder.
fn repoint(v: &mut Value, dir: &Path) {
    match v {
        Value::String(s) if s.starts_with("file://") => {
            let name = s.rsplit('/').next().unwrap().to_string();
            let p = dir.join(name).to_string_lossy().replace('\\', "/");
            *s = format!("file:///{}", p.trim_start_matches('/'));
        }
        Value::Array(a) => a.iter_mut().for_each(|x| repoint(x, dir)),
        Value::Object(o) => o.values_mut().for_each(|x| repoint(x, dir)),
        _ => {}
    }
}

fn load(case: &str) -> Recipe {
    let dir = fixtures().join(case);
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("mashup.json")).unwrap()).unwrap();
    repoint(&mut v, &dir);
    Recipe::parse(&v.to_string()).unwrap_or_else(|e| panic!("{case}: {e}"))
}

fn write(p: &Path, body: &[u8]) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

/// A folder's whole content: relative path -> bytes (folders as `None`), to compare before and after.
fn tree(root: &Path) -> BTreeMap<String, Option<Vec<u8>>> {
    let mut out = BTreeMap::new();
    fn go(root: &Path, d: &Path, out: &mut BTreeMap<String, Option<Vec<u8>>>) {
        for e in std::fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            let rel = p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            if p.is_dir() {
                out.insert(rel, None);
                go(root, &p, out);
            } else {
                out.insert(rel, Some(std::fs::read(&p).unwrap()));
            }
        }
    }
    go(root, root, &mut out);
    out
}

struct Env {
    _t: tempfile::TempDir,
    root: PathBuf,
    engine: Engine,
}

fn env(with_prism: bool) -> Env {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().to_path_buf();
    let prism = with_prism.then(|| {
        std::fs::create_dir_all(root.join("PrismLauncher/instances")).unwrap();
        Prism { data_dir: root.join("PrismLauncher"), exe: None } // no exe: nothing can be started
    });
    let mut engine = Engine::new(root.join("SIGF"), prism);
    engine.allow_local = true; // fixtures are local files (dev mode)
    std::fs::create_dir_all(root.join("Documents")).unwrap();
    engine.docs = Some(root.join("Documents"));
    Env { _t: t, root, engine }
}

fn install(e: &Env, r: &Recipe, dirs: &[(&str, &Path)]) -> InstalledMod {
    let dirs: HashMap<String, String> = dirs.iter().map(|(g, p)| (g.to_string(), p.to_string_lossy().into_owned())).collect();
    let m = e.engine.install(r, &dirs, &mut |_| {}).unwrap_or_else(|err| panic!("{}: {err}", r.id));
    for g in &m.games {
        for a in &g.launch_args {
            assert!(!a.contains('{'), "{}: unresolved launch arg {a}", r.id);
        }
    }
    assert_eq!(e.engine.installed().len(), 1);
    m
}

fn game<'a>(m: &'a InstalledMod, id: &str) -> &'a sigf_app_lib::install::InstalledGame {
    m.games.iter().find(|g| g.game == id).unwrap_or_else(|| panic!("no {id} in {:?}", m.games))
}

fn restored(e: &Env, id: &str) {
    e.engine.restore(id, false).unwrap();
    assert!(e.engine.installed().is_empty());
    let slug = sigf_app_lib::install::paths::slug(id);
    for d in ["profiles", "snapshots", "staging"] {
        assert!(!e.engine.home.join(d).join(&slug).exists(), "{d}/{slug} left behind");
    }
}

#[test]
fn doom_pk3_goes_to_the_app_folder_and_launch_args_point_at_it() {
    let e = env(false);
    let r = load("doom");
    assert_eq!(r.install[0].strategy, Strategy::Args);
    let m = install(&e, &r, &[]);
    let pk3 = e.engine.home.join("profiles").join("sigf-doom-dragon-shout").join("doom").join("mod.pk3");
    assert_eq!(std::fs::read(&pk3).unwrap(), std::fs::read(fixtures().join("doom/doom-dragon-shout.pk3")).unwrap());
    let g = game(&m, "doom");
    assert_eq!(g.launch_args, vec!["-file".to_string(), pk3.to_string_lossy().into_owned()]);
    assert!(Path::new(&g.launch_args[1]).is_absolute() && Path::new(&g.launch_args[1]).is_file());
    assert!(g.snapshot.is_none(), "nothing written outside the app");
    restored(&e, &r.id);
    assert!(!pk3.exists());
}

#[test]
fn minecraft_mrpack_becomes_a_prism_instance() {
    let e = env(true);
    let r = load("minecraft");
    // Without Prism: the typed error the UI turns into "install Prism".
    let none = Engine::new(e.root.join("other"), None);
    assert!(matches!(none.install(&r, &HashMap::new(), &mut |_| {}), Err(InstallError::NeedsLauncher { .. })));

    let m = install(&e, &r, &[]);
    let inst = e.root.join("PrismLauncher/instances/sigf-minecraft-banana");
    let jar = std::fs::read(inst.join(".minecraft/mods/sigf-minecraft-banana.jar")).unwrap();
    assert_eq!(&jar[..2], b"PK");
    let mmc: Value = serde_json::from_str(&std::fs::read_to_string(inst.join("mmc-pack.json")).unwrap()).unwrap();
    let uids: Vec<_> = mmc["components"].as_array().unwrap().iter().map(|c| (c["uid"].as_str().unwrap(), c["version"].as_str().unwrap())).collect();
    assert_eq!(uids, vec![("net.minecraft", "26.3"), ("net.fabricmc.intermediary", "26.3"), ("net.fabricmc.fabric-loader", "0.19.5")]);
    assert!(std::fs::read_to_string(inst.join("instance.cfg")).unwrap().contains("name=Banana Blocks"));
    assert_eq!(game(&m, "minecraft").launch_args, vec!["--launch".to_string(), "sigf-minecraft-banana".into()]);
    restored(&e, &r.id);
    assert!(!inst.exists());
}

#[test]
fn tf2_layers_unpack_into_tf_custom_and_restore_to_vanilla() {
    let e = env(false);
    let r = load("tf2");
    let tf2 = e.root.join("games/Team Fortress 2");
    write(&tf2.join("tf/gameinfo.txt"), b"vanilla");
    write(&tf2.join("tf/custom/readme.txt"), b"custom content goes here");
    let before = tree(&tf2);

    assert!(matches!(e.engine.install(&r, &HashMap::new(), &mut |_| {}), Err(InstallError::MissingGameDir { ref game }) if game == "tf2"));
    let m = install(&e, &r, &[("tf2", &tf2)]);
    let custom = tf2.join("tf/custom");
    assert_eq!(std::fs::read(custom.join("sigf_dragon_shout/scripts/vscripts/sigf_mod.nut")).unwrap(), b"print(\"hi\")");
    assert_eq!(std::fs::read(custom.join("sigf_dragon_shout/cfg/sigf.cfg")).unwrap(), b"echo mod");
    assert_eq!(std::fs::read(custom.join("sigf_kit/cfg/sigf.cfg")).unwrap(), b"echo kit");
    assert_eq!(std::fs::read(custom.join("sigf_kit/scripts/vscripts/mapspawn.nut")).unwrap(), b"IncludeScript(\"sigf_mod\")");
    let g = game(&m, "tf2");
    assert_eq!(g.launch_args, vec!["-game", "tf", "-insecure"]);
    assert!(g.snapshot.is_some(), "game folder writes are snapshot-tracked even under `profile`");
    restored(&e, &r.id);
    assert_eq!(tree(&tf2), before);
}

#[test]
fn gta5_fivem_resource_goes_to_the_fivem_server_folder() {
    let e = env(false);
    let r = load("gta5");
    let server = e.root.join("FXServer/server-data");
    write(&server.join("server.cfg"), b"ensure chat\n");
    write(&server.join("resources/[local]/chat/fxmanifest.lua"), b"chat");
    let before = tree(&server);

    // `{fivem}` is the FiveM server data folder, passed as game dir `fivem`; GTA's own folder is not needed.
    match e.engine.install(&r, &HashMap::new(), &mut |_| {}) {
        Err(InstallError::MissingGameDir { game }) => assert_eq!(game, "fivem"),
        other => panic!("{other:?}"),
    }
    let m = install(&e, &r, &[("fivem", &server)]);
    let res = server.join("resources/sigf_rain_of_cows");
    assert!(std::fs::read_to_string(res.join("fxmanifest.lua")).unwrap().contains("client_script 'client.lua'"));
    assert_eq!(std::fs::read(res.join("client.lua")).unwrap(), b"print(\"cows\")");
    assert_eq!(game(&m, "gta5").game_dir.as_deref(), Some(server.to_string_lossy().as_ref()));
    restored(&e, &r.id);
    assert_eq!(tree(&server), before);
}

#[test]
fn gta5_minecraft_passthrough_snapshot_plus_prism_instance() {
    let e = env(true);
    let r = load("gta5-minecraft");
    let gta = e.root.join("games/Grand Theft Auto V");
    write(&gta.join("GTA5.exe"), b"exe");
    write(&gta.join("MCPassthrough.asi"), b"older passthrough build");
    write(&gta.join("reshade-shaders/Shaders/Player.fx"), b"player shader");
    let before = tree(&gta);

    let m = install(&e, &r, &[("gta5", &gta)]);
    assert_eq!(std::fs::read(gta.join("MCPassthrough.asi")).unwrap(), b"asi");
    assert_eq!(std::fs::read(gta.join("reshade-shaders/Shaders/MCPassthrough.fx")).unwrap(), b"fx");
    assert!(!gta.join("LICENSE-passthrough.txt").exists() && !gta.join("sigf-crossover-minecraft.jar").exists());
    let inst = e.root.join("PrismLauncher/instances/sigf-gta5-blocky");
    assert!(inst.join(".minecraft/mods/sigf-crossover-minecraft.jar").is_file());
    assert_eq!(game(&m, "minecraft").launch_args, vec!["--launch".to_string(), "sigf-gta5-blocky".into()]);
    assert!(game(&m, "gta5").launch_args.is_empty());
    assert_eq!(r.launch[0].wait.as_deref(), Some("port:25599"));

    // A store update touched a file the mod wrote: restore refuses, then goes through when forced.
    write(&gta.join("MCPassthrough.asi"), b"changed");
    assert!(matches!(e.engine.restore(&r.id, false), Err(InstallError::Tampered { .. })));
    write(&gta.join("MCPassthrough.asi"), b"asi");
    restored(&e, &r.id);
    assert_eq!(tree(&gta), before);
    assert!(!inst.exists());
}

/// Upstream fusions (fixtures `skse-fusion`, `f4se-fusion`): an upstream release's files, unchanged, as a
/// script-extender plugin under `{game}/Data` (snapshot) plus a Prism instance; the pack has no Modrinth downloads so
/// the test stays offline.
fn fusion(case: &str, game_id: &str, exe: &str, plugin: &str, loader: &str, hint: &str) {
    let e = env(true);
    let r = load(case);
    let game_dir = e.root.join("games").join(game_id);
    write(&game_dir.join(exe), b"exe");
    write(&game_dir.join(loader), b"script extender loader");
    write(&game_dir.join("Data/Skyrim.esm"), b"master");
    write(&game_dir.join("Data/F4SE/Plugins/version-1-11-240-0.bin"), b"address library");
    write(&game_dir.join("Data/SKSE/Plugins/other.dll"), b"another plugin");
    let before = tree(&game_dir);

    let m = install(&e, &r, &[(game_id, &game_dir)]);
    let dll = game_dir.join("Data").join(plugin);
    assert_eq!(&std::fs::read(&dll).unwrap()[..2], b"MZ", "{case}: the plugin DLL lands in Data");
    assert_eq!(std::fs::read(game_dir.join("Data/SKSE/Plugins/other.dll")).unwrap(), b"another plugin");
    assert!(game(&m, game_id).snapshot.is_some());
    let inst = e.root.join("PrismLauncher/instances").join(sigf_app_lib::install::paths::slug(&r.id));
    let jars: Vec<_> = std::fs::read_dir(inst.join(".minecraft/mods")).unwrap().flatten().map(|f| f.file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(jars.len(), 1, "{case}: {jars:?}");
    assert!(inst.join(".minecraft/licenses").read_dir().unwrap().count() == 1, "{case}: the upstream license travels with the jar");
    assert_eq!(r.launch[0].game, "minecraft", "{case}: Minecraft starts first");
    assert_eq!(r.launch[1].game, game_id);
    // The game starts through its script extender loader, found in the game folder at play.
    let x = game(&m, game_id).exe.clone().unwrap_or_else(|| panic!("{case}: no launch exe recorded"));
    assert_eq!((x.path.as_str(), Path::new(&x.dir)), (loader, game_dir.as_path()));
    assert_eq!(x.hint.as_deref(), Some(hint));
    assert_eq!(sigf_app_lib::launch::resolve_exe(&x).unwrap(), game_dir.join(loader));
    assert!(game(&m, "minecraft").exe.is_none());
    restored(&e, &r.id);
    assert_eq!(tree(&game_dir), before);
    assert!(!inst.exists());
}

#[test]
fn skse_plugin_into_data_and_prism_instance_its_ini_points_at() {
    fusion("skse-fusion", "skyrim", "SkyrimSE.exe", "SKSE/Plugins/Fusion.dll", "skse64_loader.exe", "install SKSE64 from skse.silverlock.org");
    // The plugin starts an installed Prism with sArguments: the app's instance for this recipe.
    let e = env(true);
    let r = load("skse-fusion");
    let dir = e.root.join("games/skyrim");
    write(&dir.join("SkyrimSE.exe"), b"exe");
    let m = install(&e, &r, &[("skyrim", &dir)]);
    // No SKSE64 in this folder: Play says where to get it instead of starting Skyrim without the plugin.
    assert_eq!(
        sigf_app_lib::launch::resolve_exe(game(&m, "skyrim").exe.as_ref().unwrap()).unwrap_err(),
        "skse64_loader.exe not found: install SKSE64 from skse.silverlock.org"
    );
    // Minecraft's window stays hidden: upstream's own launcher passes this property.
    let cfg = std::fs::read_to_string(e.root.join("PrismLauncher/instances/sigf-skse-fusion/instance.cfg")).unwrap();
    assert!(cfg.contains("\nOverrideJavaArgs=true\n") && cfg.contains("\nJvmArgs=\"-Dfusion.startHidden=true\"\n"), "{cfg}");
    let ini = std::fs::read_to_string(dir.join("Data/SKSE/Plugins/Fusion.ini")).unwrap();
    assert!(ini.contains(&format!("sArguments = --launch {}", sigf_app_lib::install::paths::slug(&r.id))), "{ini}");
    assert!(!dir.join("Data/SKSE/Plugins/Fusion/Fusion-Minecraft.zip").exists(), "upstream's bundled Prism is not shipped");
    assert!(dir.join("Data/SKSE/Plugins/Fusion/SOURCE.txt").is_file());
    restored(&e, &r.id);
}

#[test]
fn f4se_plugin_into_data_and_prism_instance() {
    fusion("f4se-fusion", "fallout4", "Fallout4.exe", "F4SE/Plugins/F4Fusion.dll", "f4se_loader.exe", "install F4SE from f4se.silverlock.org");
}

/// BepInEx fusions (fixtures `bepinex-fusion`, `bepinex-mrpack`): a BepInEx 5 zip unpacked into the game folder
/// before the plugin, both through the snapshot, so a player with an existing (edited) BepInEx gets it back.
fn bepinex_fusion(case: &str, game_id: &str, exe: &str, plugin: &str) -> (Env, Recipe, PathBuf, InstalledMod) {
    let e = env(true);
    let r = load(case);
    let dir = e.root.join("games").join(game_id);
    write(&dir.join(exe), b"exe");
    write(&dir.join("doorstop_config.ini"), b"the player's own BepInEx config");
    let before = tree(&dir);
    let src = &r.install[0].files;
    assert!(src[0].src.starts_with("BepInEx_win_x64_") && src[0].dst.as_deref() == Some("{game}"), "{case}: BepInEx first");
    assert_eq!(r.requires[0].source.as_ref().map(|s| &s.sha256), Some(&src[0].sha256), "{case}: the shipped BepInEx is the install file");

    let m = install(&e, &r, &[(game_id, &dir)]);
    assert_eq!(&std::fs::read(dir.join("winhttp.dll")).unwrap()[..2], b"MZ");
    assert_eq!(&std::fs::read(dir.join("BepInEx/core/BepInEx.dll")).unwrap()[..2], b"MZ");
    assert_eq!(&std::fs::read(dir.join("BepInEx/plugins").join(plugin)).unwrap()[..2], b"MZ", "{case}: plugin in place");
    assert!(game(&m, game_id).snapshot.is_some());
    let snapshot_dir = PathBuf::from(game(&m, game_id).snapshot.clone().unwrap());
    assert!(snapshot_dir.exists());
    restored(&e, &r.id);
    assert_eq!(tree(&dir), before, "{case}: the game folder is back, the player's doorstop_config.ini included");
    (e, r, dir, m)
}

#[test]
fn bepinex_and_plugin_into_the_game_folder() {
    let (_e, r, _dir, m) = bepinex_fusion("bepinex-fusion", "slimerancher", "SlimeRancher.exe", "Fusion/Fusion.dll");
    assert_eq!(m.games.len(), 1, "Minecraft is read, not installed");
    assert_eq!(r.launch.len(), 1);
    assert_eq!(r.launch[0].game, "slimerancher");
}

#[test]
fn bepinex_bridge_into_the_guest_and_minecraft_starts_alone() {
    let (_e, r, _dir, m) = bepinex_fusion("bepinex-mrpack", "ultrakill", "ULTRAKILL.exe", "Bridge/Bridge.dll");
    assert_eq!(game(&m, "minecraft").launch_args, vec!["--launch".to_string(), "sigf-bepinex-mrpack".into()]);
    // The Fabric mod starts the guest game through Steam itself: the app starts Minecraft only.
    assert_eq!(r.launch.iter().map(|l| l.game.as_str()).collect::<Vec<_>>(), vec!["minecraft"]);
    let e = env(true);
    let dir = e.root.join("games/ultrakill");
    write(&dir.join("ULTRAKILL.exe"), b"exe");
    install(&e, &r, &[("ultrakill", &dir)]);
    let mods = e.root.join("PrismLauncher/instances/sigf-bepinex-mrpack/.minecraft/mods");
    assert!(mods.join("bridge-fabric-0.1.0.jar").is_file());
    let mmc = std::fs::read_to_string(e.root.join("PrismLauncher/instances/sigf-bepinex-mrpack/mmc-pack.json")).unwrap();
    assert!(mmc.contains("\"1.21.11\"") && mmc.contains("\"0.19.5\""), "{mmc}");
    restored(&e, &r.id);
}

#[test]
fn every_fixture_parses_with_assets_on_disk() {
    for case in ["doom", "minecraft", "tf2", "gta5", "gta5-minecraft", "skse-fusion", "f4se-fusion", "bepinex-fusion", "bepinex-mrpack"] {
        let r = load(case);
        assert!(!r.files.is_empty(), "{case}");
        for a in &r.files {
            let p = fixtures().join(case).join(&a.name);
            assert!(p.is_file(), "{case}: {} missing", a.name);
            assert_eq!(a.size, Some(std::fs::metadata(&p).unwrap().len()));
        }
    }
}

/// An upstream zip wrapped in a top folder (`Fusion/...`), as some mods release them, built here (never repacked in a
/// real recipe: `root` places a sub-folder of the zip as released).
fn wrapped_zip(path: &Path, readme: &[u8]) -> (PathBuf, Vec<Value>) {
    use std::io::Write;
    let entries: [(&str, &[u8]); 3] = [
        ("Fusion/README.txt", readme),
        ("Fusion/red4ext/plugins/Fusion/Fusion.dll", b"plugin dll"),
        ("Fusion/Fusion-source.zip", b"source"),
    ];
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    z.add_directory("Fusion/", zip::write::SimpleFileOptions::default()).unwrap();
    for (name, body) in entries {
        z.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(body).unwrap();
    }
    z.finish().unwrap();
    let hex = |b: &[u8]| sigf_app_lib::install::fetch::hex(&<sha2::Sha256 as sha2::Digest>::digest(b));
    let contents = entries.iter().map(|(n, b)| serde_json::json!({"path": n, "sha256": hex(b)})).collect();
    (path.to_path_buf(), contents)
}

fn rooted(zip: &Path, contents: &[Value], root: Value, dst: &str, strategy: &str) -> Recipe {
    let sha = sigf_app_lib::install::fetch::sha256_file(zip).unwrap();
    let v = serde_json::json!({"id": "sigf/rooted", "version": "1.0.0", "install": [{"game": "cyberpunk", "strategy": strategy,
        "files": [{"src": zip.to_string_lossy(), "dst": dst, "root": root, "unpack": true, "contents": contents, "sha256": sha}]}]});
    Recipe::parse(&v.to_string()).unwrap()
}

#[test]
fn root_places_only_a_sub_folder_of_the_zip_and_restore_is_exact() {
    let e = env(false);
    let (zip, contents) = wrapped_zip(&e.root.join("src/Fusion.zip"), b"readme");
    let dir = e.root.join("games/cyberpunk");
    write(&dir.join("bin/x64/Cyberpunk2077.exe"), b"exe");
    write(&dir.join("red4ext/RED4ext.dll"), b"red4ext");
    write(&dir.join("red4ext/plugins/Other/Other.dll"), b"another plugin");
    let before = tree(&dir);

    let r = rooted(&zip, &contents, "Fusion/red4ext".into(), "{game}/red4ext", "game-dir-snapshot");
    install(&e, &r, &[("cyberpunk", &dir)]);
    assert_eq!(std::fs::read(dir.join("red4ext/plugins/Fusion/Fusion.dll")).unwrap(), b"plugin dll");
    let added: Vec<String> = tree(&dir).into_keys().filter(|k| !before.contains_key(k)).collect();
    assert_eq!(added, ["red4ext/plugins/Fusion", "red4ext/plugins/Fusion/Fusion.dll"], "nothing outside root is written");
    restored(&e, &r.id);
    assert_eq!(tree(&dir), before);

    // Same into the app folder.
    let r = rooted(&zip, &contents, "Fusion".into(), "{app}/mod", "profile");
    install(&e, &r, &[]);
    let app = e.engine.home.join("profiles/sigf-rooted/cyberpunk/mod");
    assert_eq!(std::fs::read(app.join("red4ext/plugins/Fusion/Fusion.dll")).unwrap(), b"plugin dll");
    assert_eq!(std::fs::read(app.join("README.txt")).unwrap(), b"readme");
    assert!(!app.join("Fusion").exists());
    restored(&e, &r.id);
}

#[test]
fn root_still_checks_every_entry_and_refuses_bad_roots() {
    let e = env(false);
    let (zip, contents) = wrapped_zip(&e.root.join("src/Fusion.zip"), b"readme");
    let dir = e.root.join("games/cyberpunk");
    write(&dir.join("bin/x64/Cyberpunk2077.exe"), b"exe");
    let before = tree(&dir);
    let dirs: HashMap<String, String> = HashMap::from([("cyberpunk".into(), dir.to_string_lossy().into_owned())]);
    let try_install = |r: &Recipe| e.engine.install(r, &dirs, &mut |_| {}).err();

    // An entry outside root that differs from contents still refuses the archive.
    let (other, _) = wrapped_zip(&e.root.join("src/changed/Fusion.zip"), b"changed readme");
    let err = try_install(&rooted(&other, &contents, "Fusion/red4ext".into(), "{game}/red4ext", "game-dir-snapshot"));
    assert!(matches!(err, Some(InstallError::ShaMismatch { ref file, .. }) if file.ends_with("Fusion/README.txt")), "{err:?}");

    for bad in ["../Fusion", "/Fusion", "Fusion/", "C:/x", "C:", "Fusion\\red4ext", "{game}", "a/./b", ""] {
        let err = try_install(&rooted(&zip, &contents, bad.into(), "{game}/red4ext", "game-dir-snapshot"));
        assert!(matches!(err, Some(InstallError::PathTraversal { .. })), "{bad}: {err:?}");
    }
    // Nothing under root, per contents and (without contents) per the archive itself.
    let err = try_install(&rooted(&zip, &contents, "red4ext".into(), "{game}/red4ext", "game-dir-snapshot"));
    assert!(matches!(err, Some(InstallError::Recipe { .. })), "{err:?}");
    let err = try_install(&rooted(&zip, &[], "Fusion/red".into(), "{game}/red4ext", "game-dir-snapshot"));
    assert!(matches!(err, Some(InstallError::Recipe { .. })), "{err:?}");
    // root without unpack.
    let sha = sigf_app_lib::install::fetch::sha256_file(&zip).unwrap();
    let v = serde_json::json!({"id": "sigf/rooted", "version": "1.0.0", "install": [{"game": "cyberpunk", "strategy": "game-dir-snapshot",
        "files": [{"src": zip.to_string_lossy(), "dst": "{game}/x.zip", "root": "Fusion", "sha256": sha}]}]});
    assert!(matches!(try_install(&Recipe::parse(&v.to_string()).unwrap()), Some(InstallError::Recipe { .. })));

    assert_eq!(tree(&dir), before);
    assert!(e.engine.installed().is_empty());
}

/// Published recipes (`<dir>/*/mashup.json`, `<dir>` from `SIGF_LIBRARY_DIR`, default `../../library` from this crate;
/// skipped when the folder does not exist) through the app's own rule
/// (`install::check::check_recipe`, not in dev mode): every planned download passes, and malicious variants of each
/// (another host, another owner, plain http, a `%2e` hop, a query) are refused.
#[test]
fn library_recipes_pass_the_app_url_rule_and_variants_fail() {
    use sigf_app_lib::install::check::{check_recipe, planned_downloads};
    let dir = std::env::var_os("SIGF_LIBRARY_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../library"));
    let Ok(entries) = std::fs::read_dir(&dir) else {
        eprintln!("no library at {}: skipped", dir.display());
        return;
    };
    let mut checked = 0;
    for e in entries.flatten() {
        let path = e.path().join("mashup.json");
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let r = check_recipe(&text, false).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
        let urls = planned_downloads(&r);
        assert!(!urls.is_empty(), "{}", path.display());
        for (url, _, _) in &urls {
            assert!(url.starts_with("https://github.com/") && url.contains("/releases/download/"), "{url}");
        }
        let first = &urls[0].0;
        let tail = first.rsplit('/').next().unwrap();
        for evil in [
            format!("https://evil.example/{tail}"),
            first.replacen("https://", "http://", 1),
            first.replacen("github.com/", "github.com.evil.example/", 1),
            first.replacen("/releases/download/", "/releases/download/%2e%2e/%2e%2e/%2e%2e/evil/x/releases/download/", 1),
            format!("{first}?redirect=https://evil.example/"),
            format!("https://github.com/SIGFAI-evil/x/releases/download/v1/{tail}"),
            format!("file:///C:/Users/Public/{tail}"),
        ] {
            let bad = text.replace(first.as_str(), &evil);
            assert!(check_recipe(&bad, false).is_err(), "{}: {evil} should be refused", path.display());
        }
        checked += 1;
    }
    eprintln!("{checked} library recipes checked in {}", dir.display());
}
