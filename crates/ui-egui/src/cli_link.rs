//! `solidcraft-cli` on PATH for a macOS app bundle (a DMG install has no installer step): a
//! symlink in `/opt/homebrew/bin` or `/usr/local/bin` to the CLI inside the bundle, made quietly at
//! launch when one of them is writable, or from Help > Install solidcraft-cli command, which asks
//! for an administrator password when it must. A file that is not our symlink is never replaced.

use std::path::{Path, PathBuf};

/// What a link's place holds now.
#[derive(Debug, PartialEq)]
pub enum Slot {
    Empty,
    /// A symbolic link, to this path.
    Link(PathBuf),
    /// A file, a folder, or anything else: the user's.
    Other,
}

#[derive(Debug, PartialEq)]
pub enum Plan {
    /// The link is already right.
    Keep,
    /// Make it, or replace a stale link of ours.
    Write,
    /// Leave it: something else is there.
    Skip,
}

/// Whether `target` is a CLI inside some SolidCraft app bundle (an older or moved bundle's link
/// is ours to refresh).
fn ours(target: &Path) -> bool {
    let s = target.to_string_lossy();
    s.ends_with(".app/Contents/MacOS/solidcraft-cli")
}

/// What to do about the link slot for the CLI at `cli`.
pub fn plan(slot: &Slot, cli: &Path) -> Plan {
    match slot {
        Slot::Empty => Plan::Write,
        Slot::Link(t) if t == cli => Plan::Keep,
        Slot::Link(t) if ours(t) => Plan::Write,
        _ => Plan::Skip,
    }
}

/// The folders a link goes in, best first.
pub const DIRS: &[&str] = &["/opt/homebrew/bin", "/usr/local/bin"];

#[cfg(unix)]
fn slot(link: &Path) -> Slot {
    match std::fs::symlink_metadata(link) {
        Err(_) => Slot::Empty,
        Ok(m) if m.file_type().is_symlink() => std::fs::read_link(link).map_or(Slot::Other, Slot::Link),
        Ok(_) => Slot::Other,
    }
}

/// The CLI beside the running app, when the app runs from a bundle.
fn bundled_cli() -> Option<PathBuf> {
    let cli = std::env::current_exe().ok()?.parent()?.join("solidcraft-cli");
    (cli.is_file() && ours(&cli)).then_some(cli)
}

fn on_path_elsewhere(path_var: &std::ffi::OsStr, dirs: &[&str]) -> bool {
    std::env::split_paths(path_var).any(|d| !dirs.iter().any(|x| Path::new(x) == d) && d.join("solidcraft-cli").exists())
}

/// Make the link in `dirs` (the first that takes it). Returns the link made or kept.
#[cfg(unix)]
fn link_in(dirs: &[&str], cli: &Path) -> Option<PathBuf> {
    for dir in dirs {
        let link = Path::new(dir).join("solidcraft-cli");
        match plan(&slot(&link), cli) {
            Plan::Keep => return Some(link),
            Plan::Skip => continue,
            Plan::Write => {
                if !Path::new(dir).is_dir() {
                    continue;
                }
                let _ = std::fs::remove_file(&link);
                if std::os::unix::fs::symlink(cli, &link).is_ok() {
                    return Some(link);
                }
            }
        }
    }
    None
}

#[cfg(not(unix))]
fn link_in(_dirs: &[&str], _cli: &Path) -> Option<PathBuf> {
    None
}

/// At launch: best effort, silent. Does nothing off macOS, outside a bundle, or when
/// `solidcraft-cli` is already on PATH from somewhere else.
pub fn install_on_launch() {
    if !cfg!(target_os = "macos") {
        return;
    }
    let Some(cli) = bundled_cli() else { return };
    if std::env::var_os("PATH").is_some_and(|p| on_path_elsewhere(&p, DIRS)) {
        return;
    }
    link_in(DIRS, &cli);
}

/// Help > Install solidcraft-cli command: the same, then with an administrator prompt into
/// `/usr/local/bin` when no folder is writable. A message for the status line, and whether it failed.
pub fn install_from_menu() -> (String, bool) {
    if !cfg!(target_os = "macos") {
        return ("This is for the macOS app; the other packages put solidcraft-cli on PATH themselves.".into(), true);
    }
    let Some(cli) = bundled_cli() else {
        return ("solidcraft-cli was not found inside this app.".into(), true);
    };
    if let Some(l) = link_in(DIRS, &cli) {
        return (format!("solidcraft-cli is available as {}", l.display()), false);
    }
    let Some(script) = admin_script(&cli) else {
        return ("The app's path has characters the installer cannot quote; move SolidCraft to /Applications.".into(), true);
    };
    match std::process::Command::new("osascript").args(["-e", &script]).output() {
        Ok(o) if o.status.success() => ("solidcraft-cli is available as /usr/local/bin/solidcraft-cli".into(), false),
        Ok(o) => (format!("Not installed: {}", String::from_utf8_lossy(&o.stderr).trim()), true),
        Err(e) => (format!("Not installed: {e}"), true),
    }
}

/// The AppleScript that links `cli` into `/usr/local/bin` as an administrator. None when the path
/// holds a character that would need escaping (a quote or backslash), or an existing file is there.
pub fn admin_script(cli: &Path) -> Option<String> {
    let c = cli.to_str()?;
    if c.contains(['\'', '"', '\\', '\n']) {
        return None;
    }
    // `[ -e ] && ! [ -L ]`: never replace a file that is not a link.
    Some(format!(
        "do shell script \"mkdir -p /usr/local/bin && if [ -e /usr/local/bin/solidcraft-cli ] && [ ! -L /usr/local/bin/solidcraft-cli ]; then echo 'a file named solidcraft-cli is already in /usr/local/bin' >&2; exit 1; fi && ln -sfn '{c}' /usr/local/bin/solidcraft-cli\" with administrator privileges"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLI: &str = "/Applications/SolidCraft.app/Contents/MacOS/solidcraft-cli";

    #[test]
    fn plans() {
        let cli = Path::new(CLI);
        assert_eq!(plan(&Slot::Empty, cli), Plan::Write);
        assert_eq!(plan(&Slot::Link(cli.into()), cli), Plan::Keep);
        // A link to a moved or older bundle is ours to refresh; to anything else, not.
        assert_eq!(plan(&Slot::Link("/Users/me/Old.app/Contents/MacOS/solidcraft-cli".into()), cli), Plan::Write);
        assert_eq!(plan(&Slot::Link("/opt/other/solidcraft-cli".into()), cli), Plan::Skip);
        assert_eq!(plan(&Slot::Other, cli), Plan::Skip);
    }

    #[test]
    #[cfg(unix)]
    fn links_only_where_it_may() {
        let d = std::env::temp_dir().join(format!("solidcraft-cli-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let (a, b) = (d.join("a"), d.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let cli = Path::new(CLI);
        let dirs = [a.to_str().unwrap(), b.to_str().unwrap()];
        // A file of the user's blocks its folder; the next one gets the link.
        std::fs::write(a.join("solidcraft-cli"), "mine").unwrap();
        assert_eq!(link_in(&dirs, cli), Some(b.join("solidcraft-cli")));
        assert_eq!(std::fs::read_to_string(a.join("solidcraft-cli")).unwrap(), "mine");
        assert_eq!(std::fs::read_link(b.join("solidcraft-cli")).unwrap(), cli);
        // A stale link of ours is refreshed.
        std::fs::remove_file(b.join("solidcraft-cli")).unwrap();
        std::os::unix::fs::symlink("/x/Old.app/Contents/MacOS/solidcraft-cli", b.join("solidcraft-cli")).unwrap();
        assert_eq!(link_in(&dirs[1..], cli), Some(b.join("solidcraft-cli")));
        assert_eq!(std::fs::read_link(b.join("solidcraft-cli")).unwrap(), cli);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn path_check_ignores_our_own_folders() {
        let d = std::env::temp_dir().join(format!("solidcraft-cli-path-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("solidcraft-cli"), "").unwrap();
        let path = std::env::join_paths([d.as_path()]).unwrap();
        assert!(on_path_elsewhere(&path, &["/opt/homebrew/bin"]));
        assert!(!on_path_elsewhere(&path, &[d.to_str().unwrap()]));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn admin_script_refuses_paths_that_need_escaping() {
        assert!(admin_script(Path::new(CLI)).unwrap().contains("with administrator privileges"));
        assert!(admin_script(Path::new("/Apps/It's.app/Contents/MacOS/solidcraft-cli")).is_none());
        assert!(admin_script(Path::new("/Apps/a\"b.app/Contents/MacOS/solidcraft-cli")).is_none());
    }
}
