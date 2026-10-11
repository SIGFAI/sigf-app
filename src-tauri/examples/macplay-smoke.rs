// Play on Mac without the UI (CI: .github/workflows/app-macplay.yml), on a Mac only:
//   cargo run --example macplay-smoke [-- <recipe.json>]
// Runs the app's own mac_setup for a recipe (default: Skyrim's served Wine + DXMT recipe, no game
// download), then checks the prefix: wineboot done, `wine cmd /c ver` answers, DXMT's files in the engine and its
// winemetal.dll in the prefix, Windows Steam installed (steam.exe). Uses SIGF_MACPLAY_HOME when set. Never logs in
// to Steam and never starts a game.

use sigf_app_lib::macplay::{self, engine, recipe, wine};
use std::process::Command;

/// What sigf.ai serves (dumped into this fixture by the server tests): the smoke test runs the real recipe.
const SERVED: &str = include_str!("../tests/fixtures/mac-games.json");

/// `{ game, engines }` for one served game.
fn served(game: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(SERVED).unwrap_or_else(|e| fail(e));
    let g = v["games"].as_array().and_then(|a| a.iter().find(|g| g["game"] == game)).unwrap_or_else(|| fail(format!("{game} is not served")));
    serde_json::json!({ "game": g, "engines": v["engines"] }).to_string()
}

fn fail(msg: impl std::fmt::Display) -> ! {
    eprintln!("FAIL: {msg}");
    std::process::exit(1)
}

/// After a failed setup: the engine's own view, with Wine's error channels on.
fn diagnose(layout: &macplay::Layout, c: &recipe::Checked) {
    let Ok(w) = macplay::engine_wine(layout, c) else { return eprintln!("(engine not ready, no Wine to ask)") };
    eprintln!("--- diagnose: {} ---", w.wine_bin().display());
    let run = |args: &[&str], debug: &str| {
        let out = Command::new(w.wine_bin()).args(args).envs(w.env_pairs()).env("WINEDEBUG", debug).output();
        match out {
            Ok(o) => {
                eprintln!("$ wine {} -> {}", args.join(" "), o.status);
                let text = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
                for l in text.lines().take(120) {
                    eprintln!("  {l}");
                }
            }
            Err(e) => eprintln!("$ wine {}: {e}", args.join(" ")),
        }
    };
    for (k, v) in w.env_pairs() {
        eprintln!("  env {}={}", k.to_string_lossy(), v.to_string_lossy());
    }
    run(&["--version"], "-all");
    run(&["wineboot", "-u"], "err+all,fixme-all");
    let _ = Command::new("/usr/bin/file").arg(w.wine_bin()).status();
    let _ = Command::new("/usr/bin/codesign").args(["-dv"]).arg(w.wine_bin()).status();
}

fn main() {
    let json = match std::env::args().nth(1) {
        Some(p) => std::fs::read_to_string(&p).unwrap_or_else(|e| fail(format!("{p}: {e}"))),
        None => served("skyrim"),
    };
    let c = recipe::check(&json).unwrap_or_else(|e| fail(e));
    let layout = macplay::Layout::from_env();
    println!("macplay home: {}", layout.root().display());
    let mut last = String::new();
    let t = std::time::Instant::now();
    let st = macplay::setup(&layout, &c, &mut |step, done, total| {
        if step != last {
            println!("[{:>4}s] {step} {done}/{total}", t.elapsed().as_secs());
            last = step.to_string();
        }
    })
    .unwrap_or_else(|e| {
        let log = std::fs::read_to_string(layout.log(&c.game.game)).unwrap_or_default();
        eprintln!("--- log (last 80 lines) ---");
        for l in log.lines().rev().take(80).collect::<Vec<_>>().into_iter().rev() {
            eprintln!("{l}");
        }
        diagnose(&layout, &c);
        fail(e)
    });
    println!("status: {}", serde_json::to_string(&st).unwrap());
    if st.engine != "ready" || st.prefix != "ready" || st.store != "installed" {
        fail("setup ended without engine, prefix and Steam ready");
    }

    let Some((e, r)) = c.wine_engines() else { return println!("OK (no Wine in this recipe)") };
    let dir = engine::engine_dir(&layout, e, r);
    let ready = engine::ready(&dir, e, r).unwrap_or_else(|| fail("engine not marked ready"));
    let root = engine::wine_root(&dir, &ready);
    let prefix = layout.prefix(&c.game.game);

    let state = wine::PrefixState::load(&prefix);
    println!("prefix steps: {:?}", state.steps);
    if !state.has("wineboot") || !prefix.join("system.reg").is_file() {
        fail("wineboot did not run");
    }

    let w = macplay::ready_wine(&layout, &c).unwrap_or_else(|e| fail(e));
    let out = Command::new(w.wine_bin()).args(["cmd", "/c", "ver"]).envs(w.env_pairs()).output().unwrap_or_else(|e| fail(format!("wine: {e}")));
    let ver = String::from_utf8_lossy(&out.stdout);
    println!("wine cmd /c ver: {}", ver.trim());
    if !out.status.success() || !ver.contains("Windows") {
        fail(format!("wine cmd /c ver failed ({}): {}", out.status, String::from_utf8_lossy(&out.stderr)));
    }

    if r.is_some() {
        let lib = root.join("lib").join("wine");
        for f in ["x86_64-unix/winemetal.so", "x86_64-windows/winemetal.dll", "x86_64-windows/d3d11.dll", "x86_64-windows/dxgi.dll", "i386-windows/d3d11.dll"] {
            if !lib.join(f).is_file() {
                fail(format!("DXMT file missing: lib/wine/{f}"));
            }
        }
        let sys = prefix.join("drive_c").join("windows");
        for f in ["system32/winemetal.dll", "syswow64/winemetal.dll"] {
            if !sys.join(f).is_file() {
                fail(format!("winemetal.dll missing from the prefix: {f}"));
            }
        }
        println!("DXMT files in place");
    }

    let steam = wine::steam_exe(&prefix);
    if !steam.is_file() {
        fail("steam.exe missing");
    }
    println!("Windows Steam: {} ({} bytes)", steam.display(), std::fs::metadata(&steam).map(|m| m.len()).unwrap_or(0));
    w.kill();
    println!("OK in {}s", t.elapsed().as_secs());
}
