//! `mrpack`: we unpack the Modrinth pack into a Prism instance folder ourselves. `prismlauncher --import` opens
//! an interactive dialog and picks its own folder name, while `--launch` needs that exact name; writing the
//! instance gives us a known name (`<slug>`) and a silent install. `--import` stays as a fallback.
//! Format: https://support.modrinth.com/en/articles/8802351-modrinth-modpack-format-mrpack

use super::check::UrlPolicy;
use super::fetch::{fetch_pinned, Algo, Expected, FetchOpts};
use super::paths::{path_string, resolve_inside};
use super::InstallError;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};

/// A detected Prism Launcher. `exe` can be missing (data dir found, binary elsewhere): the instance is still written.
#[derive(Debug, Clone)]
pub struct Prism {
    pub data_dir: PathBuf,
    pub exe: Option<PathBuf>,
}

/// Dropped in every instance we write, holding the recipe id: restore only ever deletes folders carrying it.
pub const MARKER: &str = ".sigf-instance";

#[derive(Debug, Deserialize)]
pub struct Index {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub files: Vec<IndexFile>,
    #[serde(default)]
    pub dependencies: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct IndexFile {
    pub path: String,
    #[serde(default)]
    pub hashes: HashMap<String, String>,
    #[serde(default)]
    pub env: Option<Env>,
    #[serde(default)]
    pub downloads: Vec<String>,
    #[serde(default, rename = "fileSize")]
    pub file_size: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct Env {
    #[serde(default)]
    pub client: Option<String>,
}

/// Most extra JVM arguments a recipe may give an instance.
pub const JVM_ARGS_MAX: usize = 16;

/// System property prefixes a recipe may not set: the JVM's and the libraries' own switches (security manager,
/// library paths, log4j config, Fabric's `addMods`/`gameJarPath`), which could load code from outside the pack.
const JVM_DENIED: &[&str] = &["java.", "javax.", "jdk.", "sun.", "com.sun.", "log4j", "fabric.", "org.lwjgl.", "jna.", "polyglot.", "mixin."];

/// The extra JVM args a recipe may set on its instance, nothing else (no `-javaagent`, no `-XX:`, no paths or URLs):
/// `-D<key>=<value>` with a plain dotted key (not under `JVM_DENIED`) and a value of `[A-Za-z0-9_.+-]`, or a heap /
/// stack size `-Xmx4G`, `-Xms512m`, `-Xss2m`. The sigf.ai catalog applies the same rule
/// (docs/RECIPE-FORMAT.md section 4, `jvm_args`).
pub fn jvm_arg_ok(a: &str) -> bool {
    if a.len() > 200 {
        return false;
    }
    if let Some(rest) = a.strip_prefix("-Xmx").or_else(|| a.strip_prefix("-Xms")).or_else(|| a.strip_prefix("-Xss")) {
        let digits = rest.strip_suffix(['k', 'K', 'm', 'M', 'g', 'G']).unwrap_or(rest);
        return !digits.is_empty() && digits.len() <= 6 && digits.bytes().all(|b| b.is_ascii_digit());
    }
    let Some((key, value)) = a.strip_prefix("-D").and_then(|p| p.split_once('=')) else { return false };
    let key_ok = key.bytes().next().is_some_and(|b| b.is_ascii_alphabetic())
        && key.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
        && !JVM_DENIED.iter().any(|d| key.to_ascii_lowercase().starts_with(d));
    key_ok && value.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.+-".contains(&b))
}

/// `instance.cfg` for a new instance. Extra JVM args go in Prism's per-instance override (`OverrideJavaArgs` gates
/// `JvmArgs`, MinecraftInstance.cpp); the value is quoted the way Qt's ini writer quotes one holding `=`.
pub fn instance_cfg(display: &str, jvm_args: &[String]) -> String {
    let mut cfg = format!("[General]\nConfigVersion=1.2\nInstanceType=OneSix\niconKey=default\nname={display}\n");
    if !jvm_args.is_empty() {
        cfg.push_str(&format!("OverrideJavaArgs=true\nJvmArgs=\"{}\"\n", jvm_args.join(" ")));
    }
    cfg
}

pub struct Written {
    /// Instance folder name, what `--launch` takes.
    pub instance: String,
    pub dir: PathBuf,
}

fn bad_pack(msg: impl std::fmt::Display) -> InstallError {
    InstallError::recipe(format!("bad mrpack: {msg}"))
}

fn open_zip(pack: &Path) -> Result<zip::ZipArchive<File>, InstallError> {
    let f = File::open(pack).map_err(|e| InstallError::io(pack, e))?;
    zip::ZipArchive::new(f).map_err(bad_pack)
}

pub fn read_index(pack: &Path) -> Result<Index, InstallError> {
    let mut z = open_zip(pack)?;
    let entry = z.by_name("modrinth.index.json").map_err(bad_pack)?;
    serde_json::from_reader(entry).map_err(bad_pack)
}

/// `modrinth.index.json` dependencies -> Prism `mmc-pack.json` components. Prism resolves the rest (LWJGL, libraries).
pub fn mmc_pack(deps: &HashMap<String, String>) -> Result<serde_json::Value, InstallError> {
    let mc = deps.get("minecraft").ok_or_else(|| bad_pack("no minecraft dependency"))?;
    let mut comps = vec![serde_json::json!({ "uid": "net.minecraft", "version": mc, "important": true })];
    let mut keys: Vec<_> = deps.keys().filter(|k| *k != "minecraft").collect();
    keys.sort();
    for k in keys {
        let v = &deps[k];
        match k.as_str() {
            "fabric-loader" | "quilt-loader" => {
                // Fabric and Quilt map against intermediary, versioned like Minecraft itself.
                comps.push(serde_json::json!({ "uid": "net.fabricmc.intermediary", "version": mc }));
                let uid = if k == "fabric-loader" { "net.fabricmc.fabric-loader" } else { "org.quiltmc.quilt-loader" };
                comps.push(serde_json::json!({ "uid": uid, "version": v }));
            }
            "forge" => comps.push(serde_json::json!({ "uid": "net.minecraftforge", "version": v })),
            "neoforge" => comps.push(serde_json::json!({ "uid": "net.neoforged", "version": v })),
            other => return Err(bad_pack(format!("unknown dependency {other}"))),
        }
    }
    Ok(serde_json::json!({ "components": comps, "formatVersion": 1 }))
}

/// Strongest hash the index gives for a file. The mrpack format requires sha1 and sha512.
fn expected(f: &IndexFile) -> Result<Expected, InstallError> {
    if let Some(h) = f.hashes.get("sha512") {
        Expected::new(Algo::Sha512, h)
    } else if let Some(h) = f.hashes.get("sha1") {
        Expected::new(Algo::Sha1, h)
    } else {
        Err(bad_pack(format!("no hash for {}", f.path)))
    }
}

fn write_file(dst: &Path, mut r: impl std::io::Read) -> Result<(), InstallError> {
    if let Some(p) = dst.parent() {
        std::fs::create_dir_all(p).map_err(|e| InstallError::io(p, e))?;
    }
    let mut out = File::create(dst).map_err(|e| InstallError::io(dst, e))?;
    std::io::copy(&mut r, &mut out).map(|_| ()).map_err(|e| InstallError::io(dst, e))
}

/// Writes `<prism>/instances/<slug>/` from `pack`. Built in a temp folder and renamed at the end, so a failure
/// never leaves a half instance in Prism's list. `on_file(done, total)` reports progress over the index files.
pub fn write_instance(
    prism_data: &Path,
    slug: &str,
    recipe_id: &str,
    pack: &Path,
    jvm_args: &[String],
    cache: &Path,
    policy: &UrlPolicy,
    on_file: &mut dyn FnMut(usize, usize),
) -> Result<Written, InstallError> {
    if let Some(a) = jvm_args.iter().find(|a| !jvm_arg_ok(a)) {
        return Err(InstallError::recipe(format!("jvm arg not allowed: {a}")));
    }
    let index = read_index(pack)?;
    let instances = prism_data.join("instances");
    let dir = instances.join(slug);
    if dir.exists() {
        // Ours from an earlier install that lost its registry entry: replace. Anyone else's: never touch.
        if owner(&dir).as_deref() == Some(recipe_id) {
            std::fs::remove_dir_all(&dir).map_err(|e| InstallError::io(&dir, e))?;
        } else {
            return Err(InstallError::io(&dir, "a Prism instance with this name already exists"));
        }
    }
    let tmp = instances.join(format!(".{slug}.sigf-tmp"));
    let _ = std::fs::remove_dir_all(&tmp);
    let built = build(&index, &tmp, slug, recipe_id, pack, jvm_args, cache, policy, on_file);
    if let Err(e) = built {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, &dir).map_err(|e| {
        let _ = std::fs::remove_dir_all(&tmp);
        InstallError::io(&dir, e)
    })?;
    Ok(Written { instance: slug.to_string(), dir })
}

fn build(
    index: &Index,
    tmp: &Path,
    slug: &str,
    recipe_id: &str,
    pack: &Path,
    jvm_args: &[String],
    cache: &Path,
    policy: &UrlPolicy,
    on_file: &mut dyn FnMut(usize, usize),
) -> Result<(), InstallError> {
    let mc = tmp.join(".minecraft");
    std::fs::create_dir_all(&mc).map_err(|e| InstallError::io(&mc, e))?;
    let mmc = serde_json::to_string_pretty(&mmc_pack(&index.dependencies)?).map_err(bad_pack)?;

    let files: Vec<_> = index
        .files
        .iter()
        .filter(|f| f.env.as_ref().and_then(|e| e.client.as_deref()) != Some("unsupported"))
        .collect();
    // Validate every path and every download before the first one: each file needs at least one allowed URL
    // (Modrinth's CDN, or the recipe's own release files); a mirror on any other host is never contacted.
    let mut plan = vec![];
    for f in &files {
        let (abs, _) = resolve_inside(&mc, "", &f.path)?;
        let urls: Vec<&String> = f.downloads.iter().filter(|u| policy.check(u, true).is_ok()).collect();
        if urls.is_empty() {
            return match f.downloads.first() {
                Some(u) => Err(policy.check(u, true).unwrap_err()),
                None => Err(bad_pack(format!("no download for {}", f.path))),
            };
        }
        if f.file_size.is_some_and(|n| n > super::check::MAX_FILE_BYTES) {
            return Err(bad_pack(format!("{} is too large", f.path)));
        }
        plan.push((abs, expected(f)?, urls));
    }
    for (i, (f, (abs, exp, urls))) in files.iter().zip(&plan).enumerate() {
        on_file(i, files.len());
        let mut last = bad_pack(format!("no download for {}", f.path));
        let mut got = None;
        let opts = FetchOpts::new(policy.allow_local, f.file_size);
        for url in urls {
            match fetch_pinned(cache, url, exp, &opts, &mut |_, _| {}) {
                Ok(x) => {
                    got = Some(x);
                    break;
                }
                Err(e) => last = e, // next mirror
            }
        }
        let got = got.ok_or(last)?;
        let src = File::open(&got.path).map_err(|e| InstallError::io(&got.path, e))?;
        write_file(abs, src)?;
    }
    on_file(files.len(), files.len());

    // overrides/ first, then client-overrides/ on top (the format's precedence), within the unpack budget.
    let mut budget = super::MAX_UNPACKED_BYTES;
    let mut z = open_zip(pack)?;
    for prefix in ["overrides/", "client-overrides/"] {
        for i in 0..z.len() {
            let entry = z.by_index(i).map_err(bad_pack)?;
            let name = entry.name().replace('\\', "/");
            let Some(rel) = name.strip_prefix(prefix) else { continue };
            if rel.is_empty() || entry.is_dir() {
                continue;
            }
            if entry.enclosed_name().is_none() {
                return Err(InstallError::PathTraversal { dst: name });
            }
            let (abs, _) = resolve_inside(&mc, "", rel)?;
            let size = entry.size();
            if size > budget {
                return Err(bad_pack(format!("overrides unpack to more than {} bytes", super::MAX_UNPACKED_BYTES)));
            }
            budget -= size;
            write_file(&abs, std::io::Read::take(entry, size))?;
        }
    }

    let w = |name: &str, body: &str| {
        let p = tmp.join(name);
        std::fs::write(&p, body).map_err(|e| InstallError::io(&p, e))
    };
    w("mmc-pack.json", &mmc)?;
    let display = if index.name.trim().is_empty() { slug } else { index.name.trim() };
    let display: String = display.chars().filter(|c| !c.is_control()).collect();
    w("instance.cfg", &instance_cfg(&display, jvm_args))?;
    w(MARKER, recipe_id)
}

/// Recipe id stored in the instance's marker, if we wrote it.
pub fn owner(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join(MARKER)).ok().map(|s| s.trim().to_string())
}

/// Deletes an instance we wrote. Refuses a folder without our marker for this recipe.
pub fn remove_instance(dir: &Path, recipe_id: &str) -> Result<(), InstallError> {
    if !dir.exists() {
        return Ok(());
    }
    if owner(dir).as_deref() != Some(recipe_id) {
        return Err(InstallError::io(dir, "not an instance installed by SIGF for this mod; left untouched"));
    }
    std::fs::remove_dir_all(dir).map_err(|e| InstallError::io(dir, e))
}

/// Fallback: hand the pack to Prism's own importer (interactive dialog; the player picks the name).
pub fn import(exe: &Path, pack: &Path) -> Result<(), InstallError> {
    std::process::Command::new(exe)
        .arg("--import")
        .arg(pack)
        .spawn()
        .map(|_| ())
        .map_err(|e| InstallError::io(exe, e))
}

pub fn launch_args(instance: &str) -> Vec<String> {
    vec!["--launch".into(), instance.into()]
}

pub fn describe(p: &Prism) -> String {
    p.exe.as_deref().map(path_string).unwrap_or_else(|| path_string(&p.data_dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jvm_args_whitelist() {
        for ok in ["-Dfusion.startHidden=true", "-Dfoo=", "-Da_b-c.d=1.2+x", "-Xmx4G", "-Xms512m", "-Xss2m", "-Xmx2048"] {
            assert!(jvm_arg_ok(ok), "refused {ok}");
        }
        for bad in [
            "-javaagent:evil.jar",
            "-XX:+UnlockDiagnosticVMOptions",
            "-XX:OnOutOfMemoryError=calc",
            "-Djava.library.path=C:/x",
            "-Djava.security.manager=allow",
            "-Dlog4j.configurationFile=http://x/y.xml",
            "-Dfabric.addMods=evil",
            "-DFabric.addMods=evil",
            "-Dx=a b",
            "-Dx=a,b",
            "-Dx=\"q\"",
            "-Dx=C:\\evil",
            "-Dx=http://e",
            "-Dnoequals",
            "-D=1",
            "-D1x=1",
            "-Xmx",
            "-XmxG",
            "-Xmx4GG",
            "-Xmx99999999G",
            "--add-opens=java.base/java.lang=ALL-UNNAMED",
            "-cp",
            "",
        ] {
            assert!(!jvm_arg_ok(bad), "accepted {bad}");
        }
    }

    #[test]
    fn instance_cfg_jvm_args() {
        let plain = instance_cfg("Pack", &[]);
        assert!(!plain.contains("JvmArgs") && !plain.contains("OverrideJavaArgs"));
        let cfg = instance_cfg("Pack", &["-Dfusion.startHidden=true".into(), "-Xmx4G".into()]);
        assert!(cfg.contains("\nOverrideJavaArgs=true\n"), "{cfg}");
        assert!(cfg.contains("\nJvmArgs=\"-Dfusion.startHidden=true -Xmx4G\"\n"), "{cfg}");
    }
}
