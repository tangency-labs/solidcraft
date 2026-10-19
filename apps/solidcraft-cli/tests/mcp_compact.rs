//! `mcp --compact` and `SOLIDCRAFT_MCP_COMPACT` turn on compact tool output; without them the
//! server's tools are as they always were.

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

/// `inspect_design`'s input schema as `solidcraft-cli mcp` with `args` and `env` lists it, or
/// the server's stderr when it does not start.
fn inspect_schema(args: &[&str], env: Option<&str>) -> Result<Value, String> {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_solidcraft-cli"));
    cmd.args(["mcp", "--allow-any-path"]).args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    match env {
        Some(v) => cmd.env("SOLIDCRAFT_MCP_COMPACT", v),
        None => cmd.env_remove("SOLIDCRAFT_MCP_COMPACT"),
    };
    let mut child = cmd.spawn().unwrap();
    {
        let mut stdin = child.stdin.take().unwrap();
        // The server may refuse to start and close stdin first.
        let _ = writeln!(stdin, "{}", json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}));
    }
    let out = child.wait_with_output().unwrap();
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    let reply: Value = serde_json::from_slice(&out.stdout).unwrap();
    let tools = reply["result"]["tools"].as_array().cloned().unwrap_or_default();
    Ok(tools.into_iter().find(|t| t["name"] == "inspect_design").map(|t| t["inputSchema"].clone()).unwrap_or(Value::Null))
}

#[test]
fn compact_output_is_off_unless_asked() {
    let plain = inspect_schema(&[], None).unwrap();
    assert_eq!(plain["properties"]["sketches"]["type"], "boolean", "{plain}");
    assert!(plain["properties"].get("bodies").is_none(), "{plain}");
    assert_eq!(inspect_schema(&[], Some("0")).unwrap(), plain);

    for (args, env) in [(&["--compact"][..], None), (&[][..], Some("1")), (&[][..], Some("true"))] {
        let compact = inspect_schema(args, env).unwrap();
        assert!(compact["properties"]["bodies"].is_object(), "{args:?} {env:?}: {compact}");
    }

    let e = inspect_schema(&[], Some("maybe")).unwrap_err();
    assert!(e.contains("SOLIDCRAFT_MCP_COMPACT"), "{e}");
}
