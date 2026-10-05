//! Path rules. A recipe comes from the network, so every destination it names is checked to stay
//! inside the folder it targets (game dir or profile) before anything is written.

use super::InstallError;
use std::path::{Component, Path, PathBuf};

/// `sigf/gta5-blocky` -> `sigf-gta5-blocky`: safe as a single folder name.
pub fn slug(id: &str) -> String {
    let mut out = String::new();
    for c in id.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() || c == '.' || c == '_' {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches(['-', '.']).to_string()
}

/// Resolves a recipe destination under `root`. A leading `placeholder` (`{game}`, `{profile}`) is stripped and
/// the rest must be a plain relative path: no `..`, no root, no drive, no `:` (alternate data streams).
/// Returns (absolute path, relative path).
pub fn resolve_inside(root: &Path, placeholder: &str, dst: &str) -> Result<(PathBuf, PathBuf), InstallError> {
    let traversal = || InstallError::PathTraversal { dst: dst.to_string() };
    let s = dst.replace('\\', "/");
    // Only the slash right after the placeholder is ours; any other leading slash means an absolute path.
    let rest = match s.strip_prefix(placeholder).filter(|_| !placeholder.is_empty()) {
        Some(r) if r.is_empty() || r.starts_with('/') => r.trim_start_matches('/'),
        Some(_) => return Err(traversal()), // `{game}evil.dll`
        None => &s[..],
    };
    let mut rel = PathBuf::new();
    for comp in Path::new(rest).components() {
        match comp {
            Component::Normal(p) => {
                let p = p.to_string_lossy();
                if p.contains(':') || p.contains('{') {
                    return Err(traversal());
                }
                rel.push(p.as_ref());
            }
            Component::CurDir => {}
            _ => return Err(traversal()),
        }
    }
    if rel.as_os_str().is_empty() {
        return Err(traversal());
    }
    Ok((root.join(&rel), rel))
}

/// Plain relative segments joined by `/`: no empty, `.` or `..` segment, no `:`, `{`, `}` or backslash, so no
/// drive, root or leading/trailing `/` either. An install file's `root` (a folder inside its zip) must be one.
pub fn plain_rel(s: &str) -> bool {
    !s.is_empty() && s.split('/').all(|seg| !seg.is_empty() && seg != "." && seg != ".." && !seg.contains([':', '{', '}', '\\']))
}

/// The folder a recipe destination (or a launch arg) starts from. docs/RECIPE-FORMAT.md section 4, "Root placeholders".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Root {
    /// `{app}`: this mashup's own folder for that game, `<SIGF_HOME>/profiles/<slug>/<game>`. Deleted on restore.
    App,
    /// `{game}`: the game's install folder (from the scan). Snapshot-tracked.
    Game,
    /// `{docs}`: the player's Documents folder. Snapshot-tracked.
    Docs,
    /// `{fivem}`: the player's FiveM server data folder (the folder holding `resources/`). Snapshot-tracked.
    Fivem,
}

impl Root {
    pub const ALL: [Root; 4] = [Root::App, Root::Game, Root::Docs, Root::Fivem];

    pub fn token(self) -> &'static str {
        match self {
            Root::App => "{app}",
            Root::Game => "{game}",
            Root::Docs => "{docs}",
            Root::Fivem => "{fivem}",
        }
    }
}

/// `{game}/scripts/a.asi` -> (Game, "scripts/a.asi"); `{game}` -> (Game, ""). Anything not starting with a known
/// placeholder followed by `/` or the end is refused (a bare relative or absolute path names no root).
pub fn split_root(dst: &str) -> Result<(Root, String), InstallError> {
    let s = dst.replace('\\', "/");
    for r in Root::ALL {
        if let Some(rest) = s.strip_prefix(r.token()) {
            if rest.is_empty() || rest.starts_with('/') {
                return Ok((r, rest.trim_start_matches('/').to_string()));
            }
        }
    }
    Err(InstallError::PathTraversal { dst: dst.to_string() })
}

/// Second line of defence against junctions/symlinks inside the root: the real parent of `abs` must be under
/// the real `root`. Call after the parent folders exist.
pub fn ensure_real_parent_inside(root: &Path, abs: &Path, dst: &str) -> Result<(), InstallError> {
    let real_root = root.canonicalize().map_err(|e| InstallError::io(root, e))?;
    let parent = abs.parent().unwrap_or(abs);
    let real_parent = parent.canonicalize().map_err(|e| InstallError::io(parent, e))?;
    if real_parent.starts_with(&real_root) {
        Ok(())
    } else {
        Err(InstallError::PathTraversal { dst: dst.to_string() })
    }
}

/// Relative path as stored in manifests: forward slashes on every OS.
pub fn rel_string(rel: &Path) -> String {
    rel.to_string_lossy().replace('\\', "/")
}

pub fn path_string(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(slug("sigf/gta5-minecraft-blocky-los-santos"), "sigf-gta5-minecraft-blocky-los-santos");
        assert_eq!(slug("../../Windows"), "windows");
        assert_eq!(slug("A  B//C"), "a-b-c");
    }

    #[test]
    fn traversal_rejected() {
        let root = Path::new("C:/games/gta");
        for bad in [
            "{game}/../evil.dll",
            "{game}/a/../../evil.dll",
            "../evil.dll",
            "/abs.dll",
            "C:/Windows/evil.dll",
            "C:evil.dll",
            "{game}evil.dll",
            "{game}/file.txt:stream",
            "{game}",
            "{game}/",
            "{game}/{profile}/x",
        ] {
            assert!(resolve_inside(root, "{game}", bad).is_err(), "accepted {bad}");
        }
        let (abs, rel) = resolve_inside(root, "{game}", "{game}/scripts\\a.asi").unwrap();
        assert_eq!(rel, Path::new("scripts").join("a.asi"));
        assert_eq!(abs, root.join("scripts").join("a.asi"));
        assert!(resolve_inside(root, "{game}", "./plain.txt").is_ok());
    }

    #[test]
    fn plain_relative_paths() {
        for ok in ["Fusion", "Fusion/red4ext", "a b/c.d"] {
            assert!(plain_rel(ok), "refused {ok}");
        }
        for bad in ["", "/x", "x/", "a//b", ".", "./x", "x/..", "../x", "C:", "C:/x", "a\\b", "{game}/x", "a}", "x:y"] {
            assert!(!plain_rel(bad), "accepted {bad}");
        }
    }

    #[test]
    fn roots() {
        assert_eq!(split_root("{app}/mod.pk3").unwrap(), (Root::App, "mod.pk3".into()));
        assert_eq!(split_root("{game}").unwrap(), (Root::Game, String::new()));
        assert_eq!(split_root("{fivem}\\resources\\x").unwrap(), (Root::Fivem, "resources/x".into()));
        assert_eq!(split_root("{docs}/My Games/x").unwrap(), (Root::Docs, "My Games/x".into()));
        for bad in ["mod.pk3", "{profile}/x", "{game}evil", "C:/x", "/x", "{slug}/x"] {
            assert!(split_root(bad).is_err(), "accepted {bad}");
        }
    }
}
