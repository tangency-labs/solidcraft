//! Original UI icon standards and review-tool regression checks.
use std::path::Path;
use std::process::Command;

/// The oldest Python the icon tools support.
const MINIMUM: &str = "3.11";
/// Exits 0 only on a Python new enough for the icon tools.
const VERSION_CHECK: &str = "import sys; sys.exit(0 if sys.version_info >= (3, 11) else 1)";

/// The commands that may start Python, in the order they are tried. On Windows `python3` is often
/// only the Microsoft Store placeholder (it prints "Python was not found" and exits 9009), while a
/// normal install gives `python` and the `py` launcher (#6955).
fn candidates() -> &'static [&'static [&'static str]] {
    if cfg!(windows) { &[&["python3"], &["python"], &["py", "-3"]] } else { &[&["python3"]] }
}

/// The first candidate for which `works` is true.
fn choose<'a>(candidates: &[&'a [&'a str]], works: impl Fn(&[&str]) -> bool) -> Option<&'a [&'a str]> {
    candidates.iter().copied().find(|candidate| works(candidate))
}

/// True when `candidate` starts and is a Python new enough for the icon tools.
fn runs_new_enough(candidate: &[&str]) -> bool {
    let Some((program, prefix)) = candidate.split_first() else { return false };
    Command::new(program).args(prefix).args(["-c", VERSION_CHECK]).output().is_ok_and(|out| out.status.success())
}

pub fn run(root: &Path) -> Result<(), String> {
    let Some(python) = choose(candidates(), runs_new_enough) else {
        let tried: Vec<String> = candidates().iter().map(|c| c.join(" ")).collect();
        return Err(format!("UI icon standards need Python {MINIMUM}+, but none of these started one: {}", tried.join(", ")));
    };
    let checks = [
        vec!["-B", "crates/look/icons/design/tools/lint_icons.py"],
        vec!["-B", "crates/look/icons/design/tools/icon_status.py", "--check"],
        vec!["-B", "-m", "unittest", "discover", "-s", "tests/tools", "-p", "test_icon*.py"],
    ];
    let (program, prefix) = python.split_first().ok_or("no Python command")?;
    // The three only read the tree (the tests write to temporary folders of their own), so they
    // run side by side; each one's output is printed, in this order, once all have ended.
    let outputs = crate::parallel::map(&checks, |args| {
        let mut command = Command::new(program);
        command.args(prefix).args(args).current_dir(root);
        command.output()
    })?;
    let mut failed = Vec::new();
    for (args, output) in checks.iter().zip(outputs) {
        let what = format!("{} {}", python.join(" "), args.join(" "));
        eprintln!("$ {what}");
        match output {
            Ok(out) => {
                print!("{}", String::from_utf8_lossy(&out.stdout));
                eprint!("{}", String::from_utf8_lossy(&out.stderr));
                if !out.status.success() {
                    failed.push(format!("{what}: exited with {}", out.status));
                }
            }
            Err(e) => failed.push(format!("{what}: failed to spawn: {e}")),
        }
    }
    if failed.is_empty() { Ok(()) } else { Err(format!("UI icon standards (Python {MINIMUM}+): {}", failed.join("; "))) }
}

#[cfg(test)]
mod tests {
    use super::{candidates, choose};

    #[test]
    fn the_first_working_python_is_chosen() {
        let all: &[&[&str]] = &[&["python3"], &["python"], &["py", "-3"]];
        // Windows with only the Store placeholder as `python3` (#6955).
        assert_eq!(choose(all, |c| c[0] != "python3"), Some(&["python"][..]));
        // Only the launcher works.
        assert_eq!(choose(all, |c| c[0] == "py"), Some(&["py", "-3"][..]));
        // A working `python3` still wins.
        assert_eq!(choose(all, |_| true), Some(&["python3"][..]));
        assert_eq!(choose(all, |_| false), None);
    }

    #[test]
    fn python3_is_tried_first_everywhere() {
        assert_eq!(candidates().first(), Some(&&["python3"][..]));
        assert_eq!(candidates().len(), if cfg!(windows) { 3 } else { 1 });
    }
}
