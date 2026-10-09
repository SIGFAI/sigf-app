//! Minecraft profiles (docs/GAME-HUB.md section 4, "Minecraft"): Modrinth and CurseForge mods install into a
//! SIGF-managed Prism instance per (Minecraft version, loader), "SIGF <version> <Loader>" in Prism's list, folder
//! `<Prism data>/instances/sigf-<version>-<loader>`. The core writes it the way the `mrpack` strategy writes a pack's
//! instance (`instance.cfg`, `mmc-pack.json` with the loader component, our marker), on the first install into it;
//! Prism fetches Minecraft, the loader and Java on the first launch. Mods go into its game folder through the engine's
//! snapshot (`{instance}/mods` in a plan is `{game}/mods` with that folder as `{game}`), so uninstalling one restores
//! exactly what it wrote. A folder of that name SIGF did not write is never touched, nor any other instance.

use crate::install::{mrpack, InstallError};
use crate::workshop::WorkshopError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The loaders a profile can run, as plans and the UI name them.
pub const LOADERS: &[&str] = &["fabric", "neoforge", "forge", "quilt"];
/// The folders of the instance's game folder a plan may write into (one plain file each).
pub const DST_FOLDERS: &[&str] = &["mods", "resourcepacks", "shaderpacks"];

/// A profile as a plan or the UI gives it. `loaderVersion` is only needed to create the instance.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct McProfile {
    pub mc: String,
    pub loader: String,
    #[serde(default, rename = "loaderVersion")]
    pub loader_version: String,
}

/// A Minecraft version: `1.21.1`, `26.3`, `24w14a`, `1.21-pre1`: a digit first, then `[A-Za-z0-9._+-]`, at most 32,
/// no `..`.
pub fn valid_mc_version(s: &str) -> bool {
    (1..=32).contains(&s.len())
        && s.as_bytes()[0].is_ascii_digit()
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
        && !s.contains("..")
}

/// A loader version (`0.16.5`, `52.0.0`, `21.1.77-beta`): `[A-Za-z0-9._+-]`, alphanumeric first, at most 64.
pub fn valid_loader_version(s: &str) -> bool {
    (1..=64).contains(&s.len()) && s.as_bytes()[0].is_ascii_alphanumeric() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
}

fn bad(message: impl Into<String>) -> InstallError {
    InstallError::recipe(message.into())
}

impl McProfile {
    pub fn new(mc: &str, loader: &str) -> Self {
        Self { mc: mc.into(), loader: loader.into(), loader_version: String::new() }
    }

    /// Version and loader well formed; with `create`, the loader version too.
    pub fn check(&self, create: bool) -> Result<(), InstallError> {
        if !valid_mc_version(&self.mc) {
            return Err(bad(format!("bad Minecraft version {:?}", self.mc.chars().take(40).collect::<String>())));
        }
        if !LOADERS.contains(&self.loader.as_str()) {
            return Err(bad("unknown Minecraft loader"));
        }
        if create && !valid_loader_version(&self.loader_version) {
            return Err(bad("bad loader version"));
        }
        Ok(())
    }

    /// The instance folder name, what `prismlauncher --launch` takes: `sigf-1.21.1-fabric`.
    pub fn folder(&self) -> String {
        format!("sigf-{}-{}", self.mc, self.loader)
    }

    /// The name in Prism's list: `SIGF 1.21.1 Fabric`.
    pub fn display(&self) -> String {
        let l = match self.loader.as_str() {
            "fabric" => "Fabric",
            "neoforge" => "NeoForge",
            "forge" => "Forge",
            "quilt" => "Quilt",
            other => other,
        };
        format!("SIGF {} {l}", self.mc)
    }

    /// What our marker file holds in a profile instance (never a recipe id: those are `sigf/<repo>`).
    pub fn marker(&self) -> String {
        format!("profile:{}-{}", self.mc, self.loader)
    }

    /// Prism components: Minecraft, then the loader (Fabric and Quilt with intermediary), as `mrpack` writes them.
    pub fn mmc_pack(&self) -> Result<serde_json::Value, InstallError> {
        let key = match self.loader.as_str() {
            "fabric" => "fabric-loader",
            "quilt" => "quilt-loader",
            "forge" => "forge",
            "neoforge" => "neoforge",
            _ => return Err(bad("unknown Minecraft loader")),
        };
        let deps = HashMap::from([("minecraft".to_string(), self.mc.clone()), (key.to_string(), self.loader_version.clone())]);
        mrpack::mmc_pack(&deps)
    }
}

/// `<Prism data>/instances/<folder>`.
pub fn instance_dir(prism_data: &Path, p: &McProfile) -> PathBuf {
    prism_data.join("instances").join(p.folder())
}

/// The instance's game folder as Prism picks it (`MinecraftInstance::gameRoot`): `.minecraft` when it exists and
/// `minecraft` does not, else `minecraft`.
pub fn game_root(dir: &Path) -> PathBuf {
    let dot = dir.join(".minecraft");
    let plain = dir.join("minecraft");
    if dot.is_dir() && !plain.exists() {
        dot
    } else {
        plain
    }
}

/// Whether `dir` is a real folder (not a link) holding our marker for this profile.
pub fn is_ours(dir: &Path, p: &McProfile) -> bool {
    std::fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink()) && mrpack::owner(dir).as_deref() == Some(p.marker().as_str())
}

/// Writes a new profile instance: built in a hidden temp folder and renamed, so Prism never lists a half instance.
fn create(instances: &Path, p: &McProfile) -> Result<(), InstallError> {
    p.check(true)?;
    let dir = instances.join(p.folder());
    let tmp = instances.join(format!(".{}.sigf-tmp", p.folder()));
    let _ = std::fs::remove_dir_all(&tmp);
    let built = (|| {
        let mc = tmp.join(".minecraft");
        std::fs::create_dir_all(mc.join("mods")).map_err(|e| InstallError::io(&mc, e))?;
        let w = |name: &str, body: &str| {
            let f = tmp.join(name);
            std::fs::write(&f, body).map_err(|e| InstallError::io(&f, e))
        };
        w("mmc-pack.json", &serde_json::to_string_pretty(&p.mmc_pack()?).map_err(|e| bad(e.to_string()))?)?;
        w("instance.cfg", &mrpack::instance_cfg(&p.display(), &[]))?;
        w(mrpack::MARKER, &p.marker())
    })();
    if let Err(e) = built {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, &dir).map_err(|e| {
        let _ = std::fs::remove_dir_all(&tmp);
        InstallError::io(&dir, e)
    })
}

/// The profile's game folder, inside `<prism_data>/instances/<folder>`, the instance written first when it is
/// missing. Refuses a folder of that name that SIGF did not write (or a link), and a game folder that resolves
/// anywhere else (a link inside the instance).
pub fn ensure(prism_data: &Path, p: &McProfile) -> Result<PathBuf, InstallError> {
    p.check(false)?;
    let instances = prism_data.join("instances");
    std::fs::create_dir_all(&instances).map_err(|e| InstallError::io(&instances, e))?;
    let dir = instances.join(p.folder());
    if std::fs::symlink_metadata(&dir).is_ok() {
        if !is_ours(&dir, p) {
            return Err(InstallError::io(&dir, "a Prism instance with this name exists and SIGF did not create it; left untouched"));
        }
    } else {
        create(&instances, p)?;
    }
    let root = game_root(&dir);
    if std::fs::symlink_metadata(&root).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(InstallError::io(&root, "the instance's game folder is a link; left untouched"));
    }
    std::fs::create_dir_all(&root).map_err(|e| InstallError::io(&root, e))?;
    let canon = |x: &Path| x.canonicalize().map_err(|e| InstallError::io(x, e));
    let (real_instances, real_root) = (canon(&instances)?, canon(&root)?);
    if real_root.parent().and_then(Path::parent) != Some(real_instances.as_path())
        || real_root.parent().and_then(Path::file_name) != Some(std::ffi::OsStr::new(&p.folder()))
    {
        return Err(InstallError::io(&root, "the instance's game folder is outside the SIGF instance; left untouched"));
    }
    Ok(root)
}

/// The profile's state for the game page: Prism found (`prism`), its program found (`launcher`, Play needs it), the
/// instance written (`exists`), and its name in Prism.
#[derive(Debug, Clone, Serialize)]
pub struct ProfileState {
    pub prism: bool,
    pub launcher: bool,
    pub exists: bool,
    pub name: String,
}

fn cmd_err(e: InstallError) -> WorkshopError {
    WorkshopError::new("bad_profile", e.to_string())
}

#[tauri::command]
pub async fn mc_profile_state(mc: String, loader: String) -> Result<ProfileState, WorkshopError> {
    let p = McProfile::new(&mc, &loader);
    p.check(false).map_err(cmd_err)?;
    tauri::async_runtime::spawn_blocking(move || {
        let prism = crate::detect_prism();
        ProfileState {
            prism: prism.is_some(),
            launcher: prism.as_ref().is_some_and(|x| x.exe.is_some()),
            exists: prism.as_ref().is_some_and(|x| is_ours(&instance_dir(&x.data_dir, &p), &p)),
            name: p.display(),
        }
    })
    .await
    .map_err(|e| WorkshopError::new("failed", e.to_string()))
}

/// Play: Prism starts the profile's instance (`--launch sigf-<version>-<loader>`). `needs_launcher` without Prism's
/// program, `no_profile` before the first install into it.
#[tauri::command]
pub async fn mc_profile_play(mc: String, loader: String) -> Result<(), WorkshopError> {
    let p = McProfile::new(&mc, &loader);
    p.check(false).map_err(cmd_err)?;
    tauri::async_runtime::spawn_blocking(move || {
        let prism = crate::detect_prism().ok_or_else(|| WorkshopError::new("needs_launcher", "Prism Launcher not found"))?;
        let exe = prism.exe.clone().ok_or_else(|| WorkshopError::new("needs_launcher", "Prism Launcher's program not found"))?;
        if !is_ours(&instance_dir(&prism.data_dir, &p), &p) {
            return Err(WorkshopError::new("no_profile", format!("{} has no mods yet", p.display())));
        }
        std::process::Command::new(&exe)
            .args(mrpack::launch_args(&p.folder()))
            .spawn()
            .map(|_| ())
            .map_err(|e| WorkshopError::new("failed", format!("{}: {e}", exe.display())))
    })
    .await
    .map_err(|e| WorkshopError::new("failed", e.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sigf-mcprofile-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn fabric() -> McProfile {
        McProfile { mc: "1.21.1".into(), loader: "fabric".into(), loader_version: "0.16.5".into() }
    }

    #[test]
    fn names_and_checks() {
        let p = fabric();
        assert_eq!((p.folder().as_str(), p.display().as_str(), p.marker().as_str()), ("sigf-1.21.1-fabric", "SIGF 1.21.1 Fabric", "profile:1.21.1-fabric"));
        assert_eq!(McProfile::new("26.3", "neoforge").display(), "SIGF 26.3 NeoForge");
        assert!(p.check(true).is_ok());
        for v in ["1.21.1", "26.3", "24w14a", "1.21-pre1", "1.20.1+x"] {
            assert!(valid_mc_version(v), "{v}");
        }
        for v in ["", "..", "../x", "1/2", "1\\2", "a1.2", ".1", "1..2", "1 2", "1:2", &"1".repeat(33)] {
            assert!(!valid_mc_version(v), "{v}");
        }
        assert!(McProfile::new("1.21.1", "rift").check(false).is_err());
        assert!(McProfile::new("1.21.1", "fabric").check(true).is_err(), "creating needs the loader version");
        assert!(McProfile { loader_version: "0.16 5".into(), ..fabric() }.check(true).is_err());
        assert!(McProfile { loader_version: "../x".into(), ..fabric() }.check(true).is_err());
    }

    #[test]
    fn mmc_pack_per_loader() {
        let comps = |p: McProfile| p.mmc_pack().unwrap()["components"].as_array().unwrap().iter().map(|c| format!("{}={}", c["uid"].as_str().unwrap(), c["version"].as_str().unwrap())).collect::<Vec<_>>();
        assert_eq!(comps(fabric()), ["net.minecraft=1.21.1", "net.fabricmc.intermediary=1.21.1", "net.fabricmc.fabric-loader=0.16.5"]);
        assert_eq!(comps(McProfile { loader: "quilt".into(), loader_version: "0.29.1".into(), ..fabric() }), ["net.minecraft=1.21.1", "net.fabricmc.intermediary=1.21.1", "org.quiltmc.quilt-loader=0.29.1"]);
        assert_eq!(comps(McProfile { loader: "forge".into(), loader_version: "52.0.0".into(), ..fabric() }), ["net.minecraft=1.21.1", "net.minecraftforge=52.0.0"]);
        assert_eq!(comps(McProfile { loader: "neoforge".into(), loader_version: "21.1.77".into(), ..fabric() }), ["net.minecraft=1.21.1", "net.neoforged=21.1.77"]);
    }

    #[test]
    fn ensure_creates_the_instance_once() {
        let data = tmp("create");
        let root = ensure(&data, &fabric()).unwrap();
        let dir = data.join("instances").join("sigf-1.21.1-fabric");
        assert_eq!(root, dir.join(".minecraft"));
        assert!(root.join("mods").is_dir());
        let cfg = std::fs::read_to_string(dir.join("instance.cfg")).unwrap();
        assert!(cfg.contains("\nname=SIGF 1.21.1 Fabric\n") && cfg.contains("InstanceType=OneSix"), "{cfg}");
        let pack: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("mmc-pack.json")).unwrap()).unwrap();
        assert_eq!(pack["components"][2]["uid"], "net.fabricmc.fabric-loader");
        assert_eq!(std::fs::read_to_string(dir.join(mrpack::MARKER)).unwrap(), "profile:1.21.1-fabric");
        assert!(!data.join("instances").join(".sigf-1.21.1-fabric.sigf-tmp").exists());
        // A file the player or a mod put there survives the next install; the loader version is not needed again.
        std::fs::write(root.join("mods").join("a.jar"), b"x").unwrap();
        std::fs::write(dir.join("mmc-pack.json"), b"{\"kept\":true}").unwrap();
        assert_eq!(ensure(&data, &McProfile::new("1.21.1", "fabric")).unwrap(), root);
        assert!(root.join("mods").join("a.jar").exists());
        assert_eq!(std::fs::read_to_string(dir.join("mmc-pack.json")).unwrap(), "{\"kept\":true}");
        // Prism's newer layout (`minecraft` without the dot) is followed.
        let neo = McProfile { loader: "neoforge".into(), loader_version: "21.1.77".into(), ..fabric() };
        let ndir = data.join("instances").join(neo.folder());
        std::fs::create_dir_all(ndir.join("minecraft")).unwrap();
        std::fs::write(ndir.join(mrpack::MARKER), neo.marker()).unwrap();
        assert_eq!(ensure(&data, &neo).unwrap(), ndir.join("minecraft"));
        // Missing loader version for a new instance: refused, nothing left behind.
        assert!(ensure(&data, &McProfile::new("1.20.1", "forge")).is_err());
        assert!(!data.join("instances").join("sigf-1.20.1-forge").exists());
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn ensure_never_touches_other_instances() {
        let data = tmp("others");
        let dir = data.join("instances").join("sigf-1.21.1-fabric");
        // The player's own instance under that name, without our marker.
        std::fs::create_dir_all(dir.join(".minecraft").join("mods")).unwrap();
        std::fs::write(dir.join("instance.cfg"), "name=Mine").unwrap();
        assert!(ensure(&data, &fabric()).is_err());
        assert_eq!(std::fs::read_to_string(dir.join("instance.cfg")).unwrap(), "name=Mine");
        // Another profile's marker (or a pack's recipe id) is not ours either.
        std::fs::write(dir.join(mrpack::MARKER), "sigf/gta5-blocky").unwrap();
        assert!(ensure(&data, &fabric()).is_err());
        std::fs::write(dir.join(mrpack::MARKER), "profile:1.21.1-forge").unwrap();
        assert!(ensure(&data, &fabric()).is_err());
        // Bad names never reach the file system.
        assert!(ensure(&data, &McProfile { mc: "../../x".into(), ..fabric() }).is_err());
        assert!(!data.join("x").exists());
        let _ = std::fs::remove_dir_all(&data);
    }

    fn mc_plan(patch: impl FnOnce(&mut serde_json::Value)) -> String {
        let mut v = serde_json::json!({
            "ref": "mr:AANobbMI", "game": "minecraft", "name": "Sodium", "version": "0.6.0",
            "deps": [ { "ref": "mr:P7dR8mSH", "name": "Fabric API", "version": "0.100" } ],
            "instance": { "mc": "1.21.1", "loader": "fabric", "loaderVersion": "0.16.5" },
            "files": [
                { "url": "https://cdn.modrinth.com/data/P7dR8mSH/versions/x/fabric-api.jar", "name": "fabric-api.jar", "unpack": false,
                  "dst": "{instance}/mods", "of": "mr:P7dR8mSH", "hash": { "sha512": "a".repeat(128), "sha1": "b".repeat(40) } },
                { "url": "https://cdn.modrinth.com/data/AANobbMI/versions/y/sodium.jar", "name": "sodium.jar", "unpack": false,
                  "dst": "{instance}/mods", "of": "mr:AANobbMI" },
                { "url": "https://cdn.modrinth.com/data/Pack000A/versions/z/pack.zip", "name": "pack.zip", "unpack": false,
                  "dst": "{instance}/resourcepacks", "of": "mr:Pack000A" }
            ]
        });
        patch(&mut v);
        v.to_string()
    }

    #[test]
    fn plans_into_a_profile() {
        let (plan, inst) = crate::mods::check_plan_full(&mc_plan(|_| {})).unwrap();
        assert_eq!(inst, Some(fabric()));
        assert_eq!(plan.files.iter().map(|f| f.dst.as_str()).collect::<Vec<_>>(), ["{game}/mods/fabric-api.jar", "{game}/mods/sodium.jar", "{game}/resourcepacks/pack.zip"]);
        assert_eq!(plan.files[0].expected.as_ref().unwrap().algo, crate::install::fetch::Algo::Sha512);
        assert_eq!(plan.id, "mod/mr:AANobbMI");
        // CurseForge too, from its CDN.
        let cf = mc_plan(|v| {
            v["ref"] = "cf:394468".into();
            v["deps"] = serde_json::json!([]);
            v["files"] = serde_json::json!([{ "url": "https://edge.forgecdn.net/files/1/2/sodium.jar", "name": "sodium.jar", "unpack": false, "dst": "{instance}/mods", "of": "cf:394468" }]);
        });
        assert!(crate::mods::check_plan_full(&cf).unwrap().1.is_some());
        // Other plans have no profile.
        let code = |text: String| crate::mods::check_plan_full(&text).err().map(|e| e.code).unwrap_or_else(|| "ok".into());
        for (what, text) in [
            ("another game", mc_plan(|v| v["game"] = "skyrim".into())),
            ("another source", mc_plan(|v| { v["ref"] = "ts:a-b".into(); })),
            ("no loader version", mc_plan(|v| v["instance"]["loaderVersion"] = "".into())),
            ("bad version", mc_plan(|v| v["instance"]["mc"] = "../../x".into())),
            ("bad loader", mc_plan(|v| v["instance"]["loader"] = "rift".into())),
            ("config", mc_plan(|v| v["files"][1]["dst"] = "{instance}/config".into())),
            ("instance root", mc_plan(|v| v["files"][1]["dst"] = "{instance}".into())),
            ("deeper", mc_plan(|v| v["files"][1]["dst"] = "{instance}/mods/sub".into())),
            ("dotdot", mc_plan(|v| v["files"][1]["dst"] = "{instance}/mods/..".into())),
            ("game dst", mc_plan(|v| v["files"][1]["dst"] = "{game}/mods".into())),
            ("unpacked", mc_plan(|v| v["files"][1]["unpack"] = true.into())),
            ("root", mc_plan(|v| v["files"][1]["root"] = "x".into())),
            ("off cdn", mc_plan(|v| v["files"][1]["url"] = "https://evil.example/sodium.jar".into())),
            ("no profile", mc_plan(|v| { v.as_object_mut().unwrap().remove("instance"); })),
        ] {
            assert_ne!(code(text), "ok", "{what}");
        }
    }

    #[test]
    fn install_into_a_profile_and_restore() {
        let data = tmp("install");
        let jar = data.join("sodium.jar");
        std::fs::write(&jar, b"jar").unwrap();
        let root = ensure(&data, &fabric()).unwrap();
        std::fs::write(root.join("mods").join("mine.jar"), b"player's").unwrap();
        let (mut plan, _) = crate::mods::check_plan_full(&mc_plan(|v| {
            v["files"] = serde_json::json!([v["files"][1].clone()]);
            v["deps"] = serde_json::json!([]);
        }))
        .unwrap();
        // Local paths pass `fetch` only in dev mode.
        plan.files[0].url = jar.to_string_lossy().into_owned();
        let home = data.join("home");
        let mut engine = crate::install::Engine::new(&home, None);
        engine.allow_local = true;
        let dirs = HashMap::from([("minecraft".to_string(), crate::install::paths::path_string(&root))]);
        let m = crate::mods::install_plan(&mut engine, &plan, &dirs, &mut |_| {}).unwrap();
        assert_eq!(std::fs::read(root.join("mods").join("sodium.jar")).unwrap(), b"jar");
        assert_eq!(m.games[0].game_dir.as_deref(), Some(crate::install::paths::path_string(&root).as_str()), "the UI finds the profile from it");
        crate::install::Engine::new(&home, None).restore("mod/mr:AANobbMI", false).unwrap();
        assert!(!root.join("mods").join("sodium.jar").exists());
        assert_eq!(std::fs::read(root.join("mods").join("mine.jar")).unwrap(), b"player's", "the player's own mods stay");
        assert!(is_ours(&instance_dir(&data, &fabric()), &fabric()), "the instance stays for the next mod");
        let _ = std::fs::remove_dir_all(&data);
    }

    #[cfg(unix)]
    #[test]
    fn ensure_refuses_links() {
        let data = tmp("links");
        let elsewhere = tmp("links-elsewhere");
        let p = fabric();
        // The game folder linked out of the instance.
        let dir = data.join("instances").join(p.folder());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(mrpack::MARKER), p.marker()).unwrap();
        std::os::unix::fs::symlink(&elsewhere, dir.join(".minecraft")).unwrap();
        assert!(ensure(&data, &p).is_err());
        // The instance folder itself a link to a folder carrying our marker.
        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::write(elsewhere.join(mrpack::MARKER), p.marker()).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &dir).unwrap();
        assert!(ensure(&data, &p).is_err());
        assert!(!elsewhere.join(".minecraft").exists() && !elsewhere.join("minecraft").exists());
        let _ = std::fs::remove_dir_all(&data);
        let _ = std::fs::remove_dir_all(&elsewhere);
    }
}
