//! Minecraft runs through an existing launcher (Prism, Modrinth App, official), never our own:
//! they own Microsoft sign-in, Java and loaders. See docs/RECIPE-FORMAT.md section 3.

use super::{Game, Launcher};
use std::path::{Path, PathBuf};

fn first_existing(paths: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    paths.into_iter().find(|p| p.exists())
}

fn subdirs(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .map(|d| {
            d.flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| !n.starts_with('.') && !n.starts_with('_'))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// Where each launcher keeps its data, and where its program usually is.
struct Places {
    prism_data: Option<PathBuf>,
    prism_exe: Vec<PathBuf>,
    modrinth_data: Option<PathBuf>,
    modrinth_exe: Vec<PathBuf>,
    official_data: Option<PathBuf>,
}

/// Windows: `%APPDATA%\PrismLauncher`, `%APPDATA%\ModrinthApp`, `%APPDATA%\.minecraft`; the programs in
/// `%LOCALAPPDATA%\Programs` (Prism's per-user installer), Prism's portable folder, or Program Files.
#[cfg(windows)]
fn places() -> Places {
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from);
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let prism = appdata.as_ref().map(|a| a.join("PrismLauncher"));
    Places {
        prism_exe: [
            local.as_ref().map(|l| l.join("Programs").join("PrismLauncher").join("prismlauncher.exe")),
            prism.as_ref().map(|d| d.join("prismlauncher.exe")), // portable install
            Some(PathBuf::from("C:\\Program Files\\PrismLauncher\\prismlauncher.exe")),
        ]
        .into_iter()
        .flatten()
        .collect(),
        prism_data: prism,
        modrinth_data: appdata.as_ref().map(|a| a.join("ModrinthApp")),
        modrinth_exe: local.iter().map(|l| l.join("Modrinth App").join("Modrinth App.exe")).collect(),
        official_data: appdata.as_ref().map(|a| a.join(".minecraft")),
    }
}

/// macOS: data in `~/Library/Application Support` (`PrismLauncher`, `ModrinthApp`, `minecraft`); the programs are app
/// bundles in `/Applications` or `~/Applications`, started through the binary inside (`Contents/MacOS/`), which takes
/// the same command line as on Windows (`--launch <instance>`).
#[cfg(target_os = "macos")]
fn places() -> Places {
    let home = crate::install::user_home();
    let support = crate::install::user_data_dir();
    let apps: Vec<PathBuf> = std::iter::once(PathBuf::from("/Applications")).chain(home.iter().map(|h| h.join("Applications"))).collect();
    let bundle = |app: &str, bin: &str| apps.iter().map(|a| a.join(app).join("Contents").join("MacOS").join(bin)).collect::<Vec<_>>();
    Places {
        prism_data: support.as_ref().map(|s| s.join("PrismLauncher")),
        prism_exe: bundle("Prism Launcher.app", "prismlauncher"),
        modrinth_data: support.as_ref().map(|s| s.join("ModrinthApp")),
        modrinth_exe: bundle("Modrinth App.app", "Modrinth App"),
        official_data: support.as_ref().map(|s| s.join("minecraft")),
    }
}

/// Other systems: the XDG data folder (`~/.local/share/PrismLauncher`, `~/.minecraft`), programs on the usual paths.
#[cfg(not(any(windows, target_os = "macos")))]
fn places() -> Places {
    let home = crate::install::user_home();
    let data = crate::install::user_data_dir();
    Places {
        prism_data: data.as_ref().map(|d| d.join("PrismLauncher")),
        prism_exe: vec![PathBuf::from("/usr/bin/prismlauncher"), PathBuf::from("/usr/local/bin/prismlauncher")],
        modrinth_data: data.as_ref().map(|d| d.join("ModrinthApp")),
        modrinth_exe: vec![],
        official_data: home.as_ref().map(|h| h.join(".minecraft")),
    }
}

pub fn scan() -> (Vec<Launcher>, Option<Game>) {
    let p = places();
    let mut launchers = Vec::new();

    if let Some(data) = p.prism_data.filter(|d| d.exists()) {
        launchers.push(Launcher {
            kind: "prism",
            exe: first_existing(p.prism_exe).map(|p| p.to_string_lossy().to_string()),
            instances: subdirs(&data.join("instances")),
            data_dir: data.to_string_lossy().to_string(),
        });
    }

    if let Some(data) = p.modrinth_data.filter(|d| d.exists()) {
        launchers.push(Launcher {
            kind: "modrinth",
            exe: first_existing(p.modrinth_exe).map(|p| p.to_string_lossy().to_string()),
            instances: subdirs(&data.join("profiles")),
            data_dir: data.to_string_lossy().to_string(),
        });
    }

    if let Some(data) = p.official_data.filter(|d| d.exists()) {
        launchers.push(Launcher {
            kind: "official",
            exe: None,
            instances: subdirs(&data.join("versions")),
            data_dir: data.to_string_lossy().to_string(),
        });
    }

    let game = (!launchers.is_empty()).then(|| Game {
        key: "minecraft:java".into(),
        store: "minecraft",
        store_id: "java".into(),
        name: "Minecraft: Java Edition".into(),
        install_dir: launchers.first().map(|l| l.data_dir.clone()),
        build: None,
        size_bytes: None,
        launch: None,
        art: None,
        art_wide: None,
        art_local: None,
        hero_local: None,
        wide_local: None,
    });
    (launchers, game)
}
