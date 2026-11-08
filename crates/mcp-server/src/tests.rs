use serde_json::{Value, json};

use crate::{Effect, Info, Server, Tools, check_arguments, object, string, text_result, tool};

/// One app's tools: a single `echo` tool and nothing behind it (no engine, no design).
#[derive(Default)]
struct Echo {
    calls: Vec<Value>,
}

impl Tools for Echo {
    fn instructions(&self) -> String {
        "Echo test tools.".into()
    }
    fn list(&mut self) -> Vec<Value> {
        vec![tool("echo", "Echo", "Say `text` back.", object(json!({"text": string("What to say")}), &["text"]), Effect::ReadOnly)]
    }
    fn call(&mut self, name: &str, args: &Value) -> Value {
        self.calls.push(json!({"name": name, "args": args}));
        text_result(args["text"].as_str().unwrap_or_default(), false)
    }
}

fn info() -> Info {
    Info { name: "echo", title: "Echo", version: "1.0.0", max_line: 1 << 16 }
}

fn serve(lines: &[Value]) -> (Server<Echo>, Vec<Value>) {
    let input: String = lines.iter().map(|l| format!("{l}\n")).collect();
    let mut output = Vec::new();
    let mut s = Server::new(info(), Echo::default());
    s.serve(std::io::Cursor::new(input), &mut output).unwrap();
    let replies = String::from_utf8(output).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    (s, replies)
}

/// A registered tool is listed and called over the stdio transport, with no engine behind it.
#[test]
fn a_registered_tool_is_served_over_stdio() {
    let (mut s, replies) = serve(&[
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-03-26"}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "echo", "arguments": {"text": "hi"}}}),
    ]);
    assert_eq!(replies.len(), 3, "the notification gets no reply: {replies:?}");
    let init = &replies[0]["result"];
    assert_eq!(init["protocolVersion"], "2025-03-26");
    assert_eq!(init["serverInfo"], json!({"name": "echo", "title": "Echo", "version": "1.0.0"}));
    assert_eq!(init["instructions"], "Echo test tools.");
    assert!(s.is_initialized());
    assert_eq!(replies[1]["result"]["tools"][0]["name"], "echo");
    assert_eq!(replies[1]["result"]["tools"][0]["annotations"]["readOnlyHint"], true);
    assert_eq!(replies[2], json!({"jsonrpc": "2.0", "id": 3, "result": {"content": [{"type": "text", "text": "hi"}]}}));
    assert_eq!(s.tools().calls, vec![json!({"name": "echo", "args": {"text": "hi"}})]);
}

/// Arguments are checked against the tool's schema before the tool runs; unknown tools and
/// methods are JSON-RPC errors.
#[test]
fn arguments_break_the_schema_before_the_tool_runs() {
    let (mut s, replies) = serve(&[
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "echo", "arguments": {}}}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "echo", "arguments": {"text": 3}}}),
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "echo", "arguments": {"text": "a", "loud": true}}}),
        json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "shout", "arguments": {}}}),
        json!({"jsonrpc": "2.0", "id": 5, "method": "nope"}),
        json!({"jsonrpc": "2.0", "id": 6, "method": "resources/read", "params": {"uri": "x://y"}}),
    ]);
    let codes: Vec<&Value> = replies.iter().map(|r| &r["error"]["code"]).collect();
    assert_eq!(codes, [&json!(-32602), &json!(-32602), &json!(-32602), &json!(-32602), &json!(-32601), &json!(-32002)], "{replies:?}");
    assert_eq!(replies[0]["error"]["message"], "echo: missing required argument `text`");
    assert_eq!(replies[2]["error"]["message"], "echo: unknown argument `loud`; its arguments are: text");
    assert!(s.tools().calls.is_empty());
    let defs = s.tools().list();
    assert!(check_arguments(&defs, "echo", &json!({"text": "a"})).is_ok());
}

/// Hostile framing: oversized lines, bad UTF-8 and bad JSON are answered and the server goes on.
#[test]
fn hostile_lines_are_answered_and_serving_goes_on() {
    let mut input = vec![b'x'; (1 << 16) + 10];
    input.push(b'\n');
    input.extend_from_slice(b"\xff\xfe\n{not json\n[]\n");
    input.extend_from_slice(json!({"jsonrpc": "2.0", "id": 9, "method": "ping"}).to_string().as_bytes());
    let mut output = Vec::new();
    Server::new(info(), Echo::default()).serve(std::io::Cursor::new(input), &mut output).unwrap();
    let replies: Vec<Value> = String::from_utf8(output).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let codes: Vec<&Value> = replies.iter().map(|r| r.get("error").map_or(&r["result"], |e| &e["code"])).collect();
    assert_eq!(codes, [&json!(-32600), &json!(-32700), &json!(-32700), &json!(-32600), &json!({})], "{replies:?}");
}

#[test]
fn base64_matches_rfc4648() {
    for (i, o) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")] {
        assert_eq!(crate::base64(i.as_bytes()), o);
    }
    let r = crate::image_result(b"foo", "image/png", "info");
    assert_eq!(r, json!({"content": [{"type": "image", "data": "Zm9v", "mimeType": "image/png"}, {"type": "text", "text": "info"}]}));
}
