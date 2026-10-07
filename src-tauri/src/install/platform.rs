//! Which systems a recipe runs on (docs/RECIPE-FORMAT.md section 4, `platforms`). The sigf.ai catalog applies the same
//! rule to its cards (`recipePlatforms` in the site's recipe module), so a Mac never lists a mashup it cannot play, and
//! the app's `install` and `join_lobby` commands refuse one all the same.
//!
//! - `platforms` given: those of the known ids it names (`windows`, `macos`; unknown ids are ignored).
//! - Not given: `windows`, plus `macos` when every install step is `mrpack` (a Minecraft pack in Prism Launcher, which
//!   runs on macOS). Anything that installs into another game is Windows only unless the recipe says otherwise.
//! - Never `macos`, whatever the recipe says, when it needs Windows to work: a player build (the pinned toolchain is
//!   w64devkit), a launch `exe`, `app_exe` or `me3`, or an install file that is a Windows binary (`.dll`, `.asi`, `.exe`).

use super::recipe::{Recipe, Strategy};
use super::InstallError;

pub const WINDOWS: &str = "windows";
pub const MACOS: &str = "macos";
/// The platform ids the app knows, in display order.
pub const KNOWN: &[&str] = &[WINDOWS, MACOS];

/// This build's platform id: `windows`, `macos`, else the OS name (no recipe targets it).
pub fn current() -> &'static str {
    if cfg!(target_os = "macos") {
        MACOS
    } else if cfg!(windows) {
        WINDOWS
    } else {
        std::env::consts::OS
    }
}

/// The name players read.
pub fn label(p: &str) -> &str {
    match p {
        WINDOWS => "Windows",
        MACOS => "macOS",
        other => other,
    }
}

/// A Windows binary by its name: `.dll`, `.asi` (ASI loader plugins), `.exe`.
pub fn windows_binary(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    [".dll", ".asi", ".exe"].iter().any(|x| n.ends_with(x))
}

/// The recipe needs Windows whatever it declares: a player build, a launch exe, or a Windows binary among its files.
pub fn needs_windows(r: &Recipe) -> bool {
    !r.player_build.is_empty()
        || r.launch.iter().any(|l| l.exe.is_some() || l.app_exe.is_some() || l.me3.is_some())
        || r.install.iter().flat_map(|s| &s.files).any(|f| {
            windows_binary(&f.src) || f.dst.as_deref().is_some_and(windows_binary) || f.contents.iter().any(|c| windows_binary(&c.path))
        })
}

/// The platforms the recipe runs on (see the module rule), in `KNOWN` order.
pub fn platforms(r: &Recipe) -> Vec<&'static str> {
    let base: Vec<&'static str> = match &r.platforms {
        Some(p) => KNOWN.iter().copied().filter(|k| p.iter().any(|x| x == k)).collect(),
        None if !r.install.is_empty() && r.install.iter().all(|s| s.strategy == Strategy::Mrpack) => vec![WINDOWS, MACOS],
        None => vec![WINDOWS],
    };
    base.into_iter().filter(|p| *p != MACOS || !needs_windows(r)).collect()
}

/// Ok when the recipe runs on this system, else a recipe error the UI shows as is.
pub fn require_here(r: &Recipe) -> Result<(), InstallError> {
    let ps = platforms(r);
    if ps.contains(&current()) {
        return Ok(());
    }
    let on = if ps.is_empty() { "no system this app supports".to_string() } else { ps.iter().map(|p| label(p)).collect::<Vec<_>>().join(" and ") };
    Err(InstallError::recipe(format!("{} runs on {on} only, not on {}", if r.name.is_empty() { &r.id } else { &r.name }, label(current()))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn recipe(v: serde_json::Value) -> Recipe {
        let mut base = json!({ "id": "sigf/x", "version": "1.0.0", "name": "X", "kind": "mod", "games": [{ "game": "minecraft", "role": "host" }],
            "install": [{ "game": "minecraft", "strategy": "mrpack", "pack": { "url": "https://github.com/SIGFAI/x/releases/download/v1.0.0/x.mrpack", "sha256": "0".repeat(64) } }],
            "files": [] });
        for (k, x) in v.as_object().unwrap() {
            base[k] = x.clone();
        }
        Recipe::parse(&base.to_string()).unwrap()
    }

    fn snapshot_step(src: &str, contents: &[&str]) -> serde_json::Value {
        json!({ "game": "skyrim", "strategy": "game-dir-snapshot", "files": [{ "src": src, "dst": "{game}/Data", "unpack": true, "sha256": "0".repeat(64),
            "contents": contents.iter().map(|p| json!({ "path": p, "sha256": "0".repeat(64) })).collect::<Vec<_>>() }] })
    }

    #[test]
    fn pure_minecraft_packs_run_on_both() {
        assert_eq!(platforms(&recipe(json!({}))), [WINDOWS, MACOS]);
    }

    #[test]
    fn anything_else_defaults_to_windows() {
        let r = recipe(json!({ "install": [snapshot_step("x.zip", &["SkyCraft.esp"])] }));
        assert_eq!(platforms(&r), [WINDOWS]);
        let args = recipe(json!({ "install": [{ "game": "doom", "strategy": "args", "files": [{ "src": "mod.pk3", "dst": "{app}/mod.pk3", "sha256": "0".repeat(64) }] }] }));
        assert_eq!(platforms(&args), [WINDOWS]);
    }

    #[test]
    fn declared_platforms_are_read_and_capped() {
        let pk3 = json!([{ "game": "doom", "strategy": "args", "files": [{ "src": "mod.pk3", "dst": "{app}/mod.pk3", "sha256": "0".repeat(64) }] }]);
        assert_eq!(platforms(&recipe(json!({ "install": pk3, "platforms": ["macos", "windows"] }))), [WINDOWS, MACOS]);
        assert_eq!(platforms(&recipe(json!({ "platforms": ["windows"] }))), [WINDOWS]);
        assert_eq!(platforms(&recipe(json!({ "platforms": ["windows", "linux"] }))), [WINDOWS]);
        // Windows binaries, a player build or a launch exe keep it off macOS whatever it says.
        for v in [
            json!({ "platforms": ["windows", "macos"], "install": [snapshot_step("x.zip", &["SKSE/Plugins/SkyCraft.dll"])] }),
            json!({ "platforms": ["windows", "macos"], "install": [snapshot_step("MCPassthrough.asi", &[])] }),
            json!({ "platforms": ["windows", "macos"], "launch": [{ "game": "minecraft", "exe": "skse64_loader.exe" }] }),
            json!({ "platforms": ["windows", "macos"], "player_build": [{ "id": "b", "step": "minecraft", "toolchain": ["w64devkit-2.10.0"],
                "script": { "name": "b.sh", "url": "https://github.com/SIGFAI/x/releases/download/v1.0.0/b.sh", "sha256": "0".repeat(64) },
                "outputs": [{ "name": "sm64.dll", "to": "{instance}/x" }] }] }),
        ] {
            assert_eq!(platforms(&recipe(v.clone())), [WINDOWS], "{v}");
        }
        assert!(platforms(&recipe(json!({ "platforms": ["macos"], "launch": [{ "game": "minecraft", "exe": "a.exe" }] }))).is_empty());
    }

    #[test]
    fn refusal_names_the_platforms() {
        let r = recipe(json!({ "platforms": ["windows"] }));
        let here = require_here(&r);
        if current() == WINDOWS {
            assert!(here.is_ok());
        } else {
            assert!(here.unwrap_err().to_string().contains("X runs on Windows only"));
        }
        assert!(require_here(&recipe(json!({}))).is_ok() || ![WINDOWS, MACOS].contains(&current()));
    }
}
