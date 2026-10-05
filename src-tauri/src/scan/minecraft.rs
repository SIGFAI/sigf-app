//! Minecraft runs through an existing launcher (Prism, Modrinth App, official), never our own:
//! they own Microsoft sign-in, Java and loaders. See docs/RECIPE-FORMAT.md section 3.

use super::{Game, Launcher};
use std::path::{Path, PathBuf};

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var(var).ok().map(PathBuf::from)
}

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

pub fn scan() -> (Vec<Launcher>, Option<Game>) {
    let appdata = env_dir("APPDATA");
    let local = env_dir("LOCALAPPDATA");
    let mut launchers = Vec::new();

    if let Some(data) = appdata.as_ref().map(|a| a.join("PrismLauncher")).filter(|p| p.exists()) {
        let exe = first_existing(
            [
                local.as_ref().map(|l| l.join("Programs").join("PrismLauncher").join("prismlauncher.exe")),
                Some(data.join("prismlauncher.exe")), // portable install
                Some(PathBuf::from("C:\\Program Files\\PrismLauncher\\prismlauncher.exe")),
            ]
            .into_iter()
            .flatten(),
        );
        launchers.push(Launcher {
            kind: "prism",
            exe: exe.map(|p| p.to_string_lossy().to_string()),
            instances: subdirs(&data.join("instances")),
            data_dir: data.to_string_lossy().to_string(),
        });
    }

    if let Some(data) = appdata.as_ref().map(|a| a.join("ModrinthApp")).filter(|p| p.exists()) {
        let exe = first_existing(
            [local.as_ref().map(|l| l.join("Modrinth App").join("Modrinth App.exe"))].into_iter().flatten(),
        );
        launchers.push(Launcher {
            kind: "modrinth",
            exe: exe.map(|p| p.to_string_lossy().to_string()),
            instances: subdirs(&data.join("profiles")),
            data_dir: data.to_string_lossy().to_string(),
        });
    }

    if let Some(data) = appdata.as_ref().map(|a| a.join(".minecraft")).filter(|p| p.exists()) {
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
