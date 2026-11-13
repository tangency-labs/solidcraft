//! `mcp --in script --param-check <level>`: the level applies to the script the session starts
//! from too, so a misspelled key in it is refused (strict) or reported on stderr (warn) instead of
//! silently ignored.

use std::process::{Command, Stdio};

#[test]
fn param_check_applies_to_the_starting_script() {
    let dir = std::env::temp_dir().join(format!("solidcraft-mcp-param-check-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("typo.json");
    let box_ = r#"{"command": "solid.box", "params": {"length": 5, "width": 5, "height": 5, "operaton": "cut"}}"#;
    std::fs::write(&script, format!(r#"{{"commands": [{box_}]}}"#)).unwrap();
    let run = |level: &str| {
        Command::new(env!("CARGO_BIN_EXE_solidcraft-cli"))
            .args(["mcp", "--in", script.to_str().unwrap(), "--param-check", level])
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let strict = run("strict");
    let stderr = String::from_utf8_lossy(&strict.stderr);
    assert!(!strict.status.success(), "{stderr}");
    assert!(stderr.contains("unknown key `operaton`; did you mean `operation`?"), "{stderr}");
    // Warn: the script runs, and its warnings go to stderr.
    let warn = run("warn");
    let stderr = String::from_utf8_lossy(&warn.stderr);
    assert!(warn.status.success(), "{stderr}");
    assert!(stderr.contains("step 1: unknown key `operaton`; did you mean `operation`?"), "{stderr}");
    // A warned step's warning is printed even when a later step fails.
    let failing = dir.join("typo_then_failure.json");
    std::fs::write(&failing, format!(r#"{{"commands": [{box_}, {{"command": "no.such_command"}}]}}"#)).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli"))
        .args(["mcp", "--in", failing.to_str().unwrap(), "--param-check", "warn"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{stderr}");
    assert!(stderr.contains("step 1: unknown key `operaton`") && stderr.contains("step 2"), "{stderr}");
    // Off: the script runs as before, and the server ends with its input.
    let off = run("off");
    assert!(off.status.success(), "{}", String::from_utf8_lossy(&off.stderr));
    assert!(!String::from_utf8_lossy(&off.stderr).contains("operaton"));
    // A bare flag is an error, not off.
    let bare = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli")).args(["mcp", "--param-check"]).stdin(Stdio::null()).output().unwrap();
    assert!(!bare.status.success());
    assert!(String::from_utf8_lossy(&bare.stderr).contains("--param-check wants off, warn or strict"));
    std::fs::remove_dir_all(&dir).ok();
}
