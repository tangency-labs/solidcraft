//! An explicit help request prints usage to stdout and succeeds; an unknown command still fails.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Every command the CLI takes, with the name its own usage names: `inspect` is `eval`, and
/// `licenses` is `licences`.
const COMMANDS: &[(&str, &str)] = &[
    ("run", "run"),
    ("eval", "eval"),
    ("inspect", "eval"),
    ("script", "script"),
    ("bench-open", "bench-open"),
    ("bench-agent", "bench-agent"),
    ("snapshot", "snapshot"),
    ("render", "render"),
    ("exec", "exec"),
    ("commands", "commands"),
    ("recipe", "recipe"),
    ("oracle", "oracle"),
    ("fuzz", "fuzz"),
    ("fuzz-one", "fuzz-one"),
    ("mcp", "mcp"),
    ("crashes", "crashes"),
    ("licences", "licences"),
    ("licenses", "licences"),
    ("gpu", "gpu"),
    ("icon", "icon"),
    ("bench-edit", "bench-edit"),
    ("save-stress", "save-stress"),
];

/// The commands the usage lists: the aliases answer to another command's name.
const LISTED: &[&str] = &[
    "run",
    "eval",
    "script",
    "bench-open",
    "bench-agent",
    "snapshot",
    "render",
    "exec",
    "commands",
    "recipe",
    "oracle",
    "fuzz",
    "mcp",
    "crashes",
    "licences",
    "gpu",
    "icon",
];

/// The command names a text mentions after `solidcraft-cli `, each one only once.
fn mentioned(text: &str) -> Vec<&str> {
    let mut names: Vec<&str> = text.split("solidcraft-cli ").skip(1).filter_map(|rest| rest.split_whitespace().next()).collect();
    names.sort_unstable();
    names.dedup();
    names
}

/// Runs the binary and waits at most `limit` for it to finish. `None` means it was still
/// running and was killed: the fuzz run `fuzz --help` used to start goes on for minutes.
fn run_within(limit: Duration, args: &[&str]) -> Option<(Option<i32>, String, String)> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli")).args(args).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let started = Instant::now();
    loop {
        match child.try_wait().unwrap() {
            Some(status) => {
                let out = child.wait_with_output().unwrap();
                return Some((status.code(), String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned()));
            }
            None if started.elapsed() >= limit => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            None => std::thread::sleep(Duration::from_millis(25)),
        }
    }
}

#[test]
fn help_prints_usage_and_succeeds() {
    for arg in ["--help", "-h", "help"] {
        let output = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli")).arg(arg).output().unwrap();
        assert!(output.status.success(), "{arg}: {}", String::from_utf8_lossy(&output.stderr));
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.starts_with("usage:"), "{arg}: {stdout}");
        assert!(stdout.contains("solidcraft-cli run"), "{arg}: {stdout}");
        assert!(output.stderr.is_empty(), "{arg}: {}", String::from_utf8_lossy(&output.stderr));
    }
}

#[test]
fn unknown_command_still_fails_with_usage() {
    let output = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli")).arg("--not-a-command").output().unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("error: usage:"));
}

#[test]
fn every_command_prints_its_own_usage() {
    for (cmd, named) in COMMANDS {
        for flag in ["--help", "-h"] {
            let output = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli")).args([*cmd, flag]).output().unwrap();
            assert!(output.status.success(), "{cmd} {flag}: {}", String::from_utf8_lossy(&output.stderr));
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(stdout.starts_with("usage:"), "{cmd} {flag}: {stdout}");
            assert_eq!(mentioned(&stdout), vec![*named], "{cmd} {flag} printed another command's usage: {stdout}");
            assert!(output.stderr.is_empty(), "{cmd} {flag}: {}", String::from_utf8_lossy(&output.stderr));
        }
    }
}

#[test]
fn the_top_level_usage_still_lists_every_command() {
    let output = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli")).arg("--help").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    for cmd in LISTED {
        assert!(stdout.contains(&format!("solidcraft-cli {cmd}")), "{cmd}: {stdout}");
    }
}

#[test]
fn fuzz_help_does_not_start_a_fuzz_run() {
    let Some((code, stdout, stderr)) = run_within(Duration::from_secs(20), &["fuzz", "--help"]) else {
        panic!("`fuzz --help` kept running: it started a fuzz run instead of printing the usage");
    };
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stdout.starts_with("usage:"), "{stdout}");
    assert_eq!(mentioned(&stdout), vec!["fuzz"], "{stdout}");
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn fuzz_refuses_an_argument_it_does_not_take() {
    for arg in ["--not-a-flag", "stray"] {
        let Some((code, _stdout, stderr)) = run_within(Duration::from_secs(20), &["fuzz", arg]) else {
            panic!("`fuzz {arg}` kept running: it started a fuzz run instead of refusing the argument");
        };
        assert_ne!(code, Some(0), "`fuzz {arg}` succeeded");
        assert!(stderr.contains(arg), "`fuzz {arg}`: {stderr}");
    }
}

#[test]
fn render_help_keeps_the_registered_gpu_request_controls() {
    for args in [vec!["--help"], vec!["render", "--help"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli")).args(&args).output().unwrap();
        assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
        assert!(output.stderr.is_empty());
        let stdout = String::from_utf8_lossy(&output.stdout);
        for flag in ["--progressive", "--samples", "--max-depth", "--seed", "--exposure", "--lighting", "--environment", "--tone-map"] {
            assert!(stdout.contains(flag), "{args:?} lost {flag}: {stdout}");
        }
    }
}
