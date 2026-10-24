//! Explicit native GPU registration must not change bridged MCP ownership.
use std::process::Command;
#[test]
fn connected_application_rejects_headless_gpu_installation_before_connecting() {
    let result = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli")).args(["mcp", "--render-gpu", "--connect", "127.0.0.1:1"]).output().unwrap();
    assert!(!result.status.success());
    let message = String::from_utf8_lossy(&result.stderr);
    assert!(message.contains("connected application owns its own device"), "{message}");
}
