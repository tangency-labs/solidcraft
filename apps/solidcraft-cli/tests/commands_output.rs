//! The registry remains valid JSON and a closed pipeline reader must not cause a panic.

use std::process::{Command, Stdio};

#[test]
fn registry_output_is_complete_json() {
    let output = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli")).arg("commands").output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let registry: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let commands = registry.as_array().unwrap();
    assert!(commands.iter().any(|command| command["id"] == "solid.box"));
    assert!(commands.iter().any(|command| command["id"] == "engine.commands"));
    assert!(output.stdout.ends_with(b"\n"));
    assert!(output.stderr.is_empty());
}

#[test]
fn closed_registry_pipe_exits_without_panicking() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli")).arg("commands").stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    // Close our read end before waiting. No sleeps or timing threshold are needed:
    // the registry is larger than the pipe buffer, so it cannot all be written first.
    drop(child.stdout.take());
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "status: {}; stderr: {}", output.status, String::from_utf8_lossy(&output.stderr));
    assert!(output.stderr.is_empty(), "{}", String::from_utf8_lossy(&output.stderr));
}

fn cli_output(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_solidcraft-cli")).args(args).output().unwrap()
}

/// #20923: a word narrows the list to the commands whose id contains it; `--id` describes one.
#[test]
fn commands_filter_and_describe_one_command() {
    let all: serde_json::Value = serde_json::from_slice(&cli_output(&["commands"]).stdout).unwrap();
    let output = cli_output(&["commands", "CONFIG."]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let some: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let some = some.as_array().unwrap();
    assert!(!some.is_empty() && some.len() < all.as_array().unwrap().len());
    assert!(some.iter().all(|c| c["id"].as_str().unwrap().contains("config.")));
    assert!(some.iter().any(|c| c["id"] == "config.column"));
    let none: serde_json::Value = serde_json::from_slice(&cli_output(&["commands", "no-such-command-word"]).stdout).unwrap();
    assert_eq!(none, serde_json::json!([]));

    let output = cli_output(&["commands", "--id", "config.column"]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let one: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(one["id"], "config.column");
    assert!(one["params"].is_string());

    let output = cli_output(&["commands", "--id", "solid.bxo"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("did you mean `solid.box`"), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(cli_output(&["commands", "config", "--id", "config.column"]).status.code(), Some(1));
    assert_eq!(cli_output(&["commands", "--bogus"]).status.code(), Some(1));
    let two_words = cli_output(&["commands", "config", "solid"]);
    assert_eq!(two_words.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&two_words.stderr).contains("one filter word"), "{}", String::from_utf8_lossy(&two_words.stderr));
    let two_ids = cli_output(&["commands", "--id", "solid.box", "--id", "config.column"]);
    assert_eq!(two_ids.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&two_ids.stderr).contains("one --id"), "{}", String::from_utf8_lossy(&two_ids.stderr));
}
