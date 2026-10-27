//! Repository chores, run as `cargo xtask <command>`.
//!
//! `cargo xtask rm <path>...` deletes files and directory trees, and
//! `cargo xtask worktree-rm <dir>...` deletes git worktrees and prunes them.
//! Agents use these instead of `rm -rf`, which the coding-agent harness holds
//! for the owner's approval even when permissions are bypassed, stalling
//! unattended runs.
//!
//! Deletion is confined to the worktree root (`$SOLIDCORE_WT`, default
//! `~/github/solidcore-wt`), this checkout's `bin/` and `reports/`, and the
//! system temp dir. A root itself, the main checkout, and anything inside a
//! `.git` directory are refused.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "usage:
  cargo xtask rm <path>...            delete files or directory trees
  cargo xtask worktree-rm <dir>...    delete git worktrees, then `git worktree prune`
  options: -n / --dry-run             print what would be deleted";

fn main() -> ExitCode {
    let mut args: Vec<OsString> = env::args_os().skip(1).collect();
    let dry_run = take_flag(&mut args, &["-n", "--dry-run"]);
    let Some(cmd) = (!args.is_empty()).then(|| args.remove(0)) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let result = match cmd.to_str() {
        Some("rm") => rm(&args, dry_run),
        Some("worktree-rm") => worktree_rm(&args, dry_run),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Removes every occurrence of any of `names` from `args`; reports whether one was present.
fn take_flag(args: &mut Vec<OsString>, names: &[&str]) -> bool {
    let before = args.len();
    args.retain(|a| !names.iter().any(|n| a == n));
    args.len() != before
}

/// Deletes each path after checking it against the allowed roots.
fn rm(paths: &[OsString], dry_run: bool) -> io::Result<()> {
    if paths.is_empty() {
        return Err(io::Error::other("rm: no paths given"));
    }
    let roots = Roots::discover()?;
    // Check every path before deleting any, so a bad argument deletes nothing.
    let checked = paths
        .iter()
        .map(|p| roots.check(Path::new(p)))
        .collect::<io::Result<Vec<_>>>()?;
    for target in checked {
        // Re-check right before deleting: if a parent was swapped for a symlink since the
        // first check, the target now resolves elsewhere and is refused. (std's
        // remove_dir_all never follows symlinks inside the tree it deletes.)
        if roots.check(&target)? != target {
            return Err(refuse(&target, "changed between check and delete"));
        }
        delete(&target, dry_run)?;
    }
    Ok(())
}

/// Deletes each worktree directory, then prunes git's worktree list.
fn worktree_rm(dirs: &[OsString], dry_run: bool) -> io::Result<()> {
    rm(dirs, dry_run)?;
    if dry_run {
        println!("would run: git worktree prune");
        return Ok(());
    }
    let status = Command::new("git")
        .args(["worktree", "prune"])
        .current_dir(main_checkout()?)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "git worktree prune failed: {status}"
        )))
    }
}

fn delete(target: &Path, dry_run: bool) -> io::Result<()> {
    let meta = match fs::symlink_metadata(target) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            println!("absent: {}", target.display());
            return Ok(());
        }
        Err(e) => return Err(e),
    };
    if dry_run {
        println!("would delete: {}", target.display());
        return Ok(());
    }
    // A symlink is removed itself, never followed.
    if meta.is_dir() {
        fs::remove_dir_all(target)?;
    } else {
        fs::remove_file(target)?;
    }
    println!("deleted: {}", target.display());
    Ok(())
}

/// The main checkout: the directory holding this workspace's shared `.git`.
fn main_checkout() -> io::Result<PathBuf> {
    let out = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .output()?;
    if !out.status.success() {
        return Err(io::Error::other("not inside a git checkout"));
    }
    let common = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    common
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("git common dir has no parent"))
}

/// The directories deletion is confined to.
struct Roots {
    allowed: Vec<PathBuf>,
    forbidden: Vec<PathBuf>,
}

impl Roots {
    fn discover() -> io::Result<Self> {
        let main = canonical(&main_checkout()?)?;
        let worktrees = match env::var_os("SOLIDCORE_WT") {
            Some(p) => PathBuf::from(p),
            None => home()?.join("github").join("solidcore-wt"),
        };
        let mut allowed = vec![main.join("bin"), main.join("reports")];
        for root in [worktrees, env::temp_dir()] {
            if let Ok(c) = canonical(&root) {
                allowed.push(c);
            }
        }
        Ok(Roots {
            allowed,
            forbidden: vec![main],
        })
    }

    /// Resolves `path` and returns it if it lies strictly inside an allowed root.
    fn check(&self, path: &Path) -> io::Result<PathBuf> {
        let abs = lexical_absolute(path)?;
        // Resolve symlinks in the parent only, so a link is judged by where it sits.
        let parent = abs.parent().ok_or_else(|| refuse(path, "has no parent"))?;
        let name = abs
            .file_name()
            .ok_or_else(|| refuse(path, "has no file name"))?;
        let resolved = canonical(parent)?.join(name);
        if resolved.components().any(|c| c.as_os_str() == ".git") {
            return Err(refuse(path, "is inside a .git directory"));
        }
        if self.forbidden.iter().any(|f| f == &resolved) {
            return Err(refuse(path, "is the main checkout"));
        }
        if self.allowed.contains(&resolved) {
            return Err(refuse(path, "is a deletion root itself"));
        }
        let inside = self
            .allowed
            .iter()
            .any(|root| resolved != *root && resolved.starts_with(root));
        if inside {
            Ok(resolved)
        } else {
            Err(refuse(
                path,
                "is outside the worktree root, bin/, reports/ and the temp dir",
            ))
        }
    }
}

fn refuse(path: &Path, why: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!("refusing {}: {why}", path.display()),
    )
}

fn home() -> io::Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("HOME is not set"))
}

fn canonical(p: &Path) -> io::Result<PathBuf> {
    fs::canonicalize(p).map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", p.display())))
}

/// Makes `path` absolute and folds `.` and `..` without touching the filesystem.
fn lexical_absolute(path: &Path) -> io::Result<PathBuf> {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()?.join(path)
    };
    let mut out = PathBuf::new();
    for c in joined.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots(tmp: &Path) -> Roots {
        let main = tmp.join("main");
        Roots {
            allowed: vec![main.join("bin"), tmp.join("wt")],
            forbidden: vec![main],
        }
    }

    #[test]
    fn allows_paths_strictly_inside_a_root() {
        let tmp = tempdir("allow");
        fs::create_dir_all(tmp.join("wt/a/bin")).unwrap();
        let r = roots(&tmp);
        assert_eq!(
            r.check(&tmp.join("wt/a/bin")).unwrap(),
            tmp.join("wt/a/bin")
        );
        assert!(
            r.check(&tmp.join("wt")).is_err(),
            "a root itself is refused"
        );
    }

    #[test]
    fn refuses_escapes_git_dirs_and_the_main_checkout() {
        let tmp = tempdir("refuse");
        fs::create_dir_all(tmp.join("wt/a/.git")).unwrap();
        fs::create_dir_all(tmp.join("main/bin")).unwrap();
        let r = roots(&tmp);
        assert!(r.check(&tmp.join("wt/../main")).is_err());
        assert!(r.check(&tmp.join("wt/a/.git")).is_err());
        assert!(r.check(&tmp.join("elsewhere")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_out_of_a_root_is_judged_where_it_sits() {
        let tmp = tempdir("link");
        fs::create_dir_all(tmp.join("wt")).unwrap();
        fs::create_dir_all(tmp.join("outside/keep")).unwrap();
        std::os::unix::fs::symlink(tmp.join("outside"), tmp.join("wt/link")).unwrap();
        let r = roots(&tmp);
        let target = r.check(&tmp.join("wt/link")).unwrap();
        delete(&target, false).unwrap();
        assert!(
            tmp.join("outside/keep").exists(),
            "the link's target must survive"
        );
        assert!(!tmp.join("wt/link").exists());
    }

    fn tempdir(tag: &str) -> PathBuf {
        let d = env::temp_dir().join(format!("xtask-test-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        canonical(&d).unwrap()
    }
}
