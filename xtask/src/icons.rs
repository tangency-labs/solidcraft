//! Original UI icon standards and review-tool regression checks.
use std::path::Path;
use std::process::Command;

pub fn run(root: &Path) -> Result<(), String> {
    let checks = [
        vec!["-B", "crates/look/icons/design/tools/lint_icons.py"],
        vec!["-B", "crates/look/icons/design/tools/icon_status.py", "--check"],
        vec!["-B", "-m", "unittest", "discover", "-s", "tests/tools", "-p", "test_icon*.py"],
    ];
    // The three only read the tree (the tests write to temporary folders of their own), so they
    // run side by side; each one's output is printed, in this order, once all have ended.
    let outputs = crate::parallel::map(&checks, |args| {
        let mut command = Command::new("python3");
        command.args(args).current_dir(root);
        command.output()
    })?;
    let mut failed = Vec::new();
    for (args, output) in checks.iter().zip(outputs) {
        let what = format!("python3 {}", args.join(" "));
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
    if failed.is_empty() { Ok(()) } else { Err(format!("UI icon standards (Python 3.11+): {}", failed.join("; "))) }
}
