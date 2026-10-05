// The install engine without the UI, for tests on a real PC:
//   cargo run --example install -- install <mashup.json> [game=<dir> ...]
//   cargo run --example install -- restore <id> [--force]
//   cargo run --example install -- list
//   cargo run --example install -- validate [--packs] <mashup.json> ...
// Uses the same SIGF_HOME (default %LOCALAPPDATA%\SIGF) and Prism detection as the app. Never launches anything.
// Local recipes (`file://` URLs, local paths) need SIGF_DEV_LOCAL_RECIPES=1; the app itself never allows them.
// `validate` runs the app's own recipe rule (install::check, not in dev mode) and prints every planned download;
// with --packs it also downloads each mrpack (hash-checked, into a temp folder) and checks the files it lists.

use sigf_app_lib::{install, scan};
use std::collections::HashMap;

fn prism() -> Option<install::Prism> {
    let (launchers, _) = scan::minecraft::scan();
    launchers.into_iter().find(|l| l.kind == "prism").map(|l| install::Prism { data_dir: l.data_dir.into(), exe: l.exe.map(Into::into) })
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let engine = install::Engine::from_env(prism());
    let res = match args.first().map(String::as_str) {
        Some("install") if args.len() >= 2 => {
            let recipe = install::Recipe::parse(&std::fs::read_to_string(&args[1]).expect("read recipe")).expect("parse recipe");
            let dirs: HashMap<String, String> =
                args[2..].iter().filter_map(|a| a.split_once('=')).map(|(k, v)| (k.to_string(), v.to_string())).collect();
            engine
                .install(&recipe, &dirs, &mut |p| eprintln!("{:?}", p))
                .map(|m| println!("{}", serde_json::to_string_pretty(&m).unwrap()))
        }
        Some("restore") if args.len() >= 2 => engine.restore(&args[1], args.iter().any(|a| a == "--force")).map(|_| println!("restored {}", args[1])),
        Some("validate") if args.len() >= 2 => validate(&args[1..]),
        Some("list") => {
            println!("{}", serde_json::to_string_pretty(&engine.installed()).unwrap());
            Ok(())
        }
        _ => {
            eprintln!("usage: install <mashup.json> [game=<dir> ...] | restore <id> [--force] | list | validate [--packs] <mashup.json> ...");
            std::process::exit(2);
        }
    };
    if let Err(e) = res {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

/// Every recipe through `install::check::check_recipe` (app mode: no local files); with `--packs`, every mrpack's
/// index entries through the same policy. Exits 1 when any recipe or pack entry is refused.
fn validate(args: &[String]) -> Result<(), install::InstallError> {
    let packs = args.iter().any(|a| a == "--packs");
    let tmp = std::env::temp_dir().join(format!("sigf-validate-{}", std::process::id()));
    let mut failed = 0;
    for path in args.iter().filter(|a| *a != "--packs") {
        let text = std::fs::read_to_string(path).map_err(|e| install::InstallError::io(std::path::Path::new(path), e))?;
        let r = match install::check::check_recipe(&text, false) {
            Ok(r) => r,
            Err(e) => {
                println!("REFUSED {path}: {e}");
                failed += 1;
                continue;
            }
        };
        println!("OK {} {} ({path})", r.id, r.version);
        for (url, _, size) in install::check::planned_downloads(&r) {
            println!("  {url} ({} bytes)", size.map(|s| s.to_string()).unwrap_or_else(|| "?".into()));
        }
        if !packs {
            continue;
        }
        let policy = install::check::UrlPolicy::for_recipe(&r, false);
        for step in &r.install {
            let Some(p) = &step.pack else { continue };
            let got = install::fetch::fetch(&tmp, &p.url, &p.sha256, &install::fetch::FetchOpts::new(false, p.size), &mut |_, _| {});
            let index = match got.and_then(|g| install::mrpack::read_index(&g.path)) {
                Ok(i) => i,
                Err(e) => {
                    println!("  PACK REFUSED {}: {e}", p.url);
                    failed += 1;
                    continue;
                }
            };
            let mut ok = 0;
            for f in &index.files {
                let allowed: Vec<&String> = f.downloads.iter().filter(|u| policy.index_url_ok(u)).collect();
                for u in f.downloads.iter().filter(|u| !policy.index_url_ok(u)) {
                    println!("  index mirror skipped for {}: {u}", f.path);
                }
                if allowed.is_empty() && f.env.as_ref().and_then(|e| e.client.as_deref()) != Some("unsupported") {
                    println!("  INDEX REFUSED {}: {:?}", f.path, f.downloads);
                    failed += 1;
                } else {
                    ok += 1;
                }
            }
            println!("  pack {}: {ok}/{} index files allowed", p.url.rsplit('/').next().unwrap_or(""), index.files.len());
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    if failed > 0 {
        eprintln!("{failed} refused");
        std::process::exit(1);
    }
    Ok(())
}
