use serde_json::{Value, json};

use crate::{Effect, Info, PROTOCOL_VERSION, SUPPORTED_VERSIONS, Server, Tools, check_arguments, object, string, text_result, tool};

/// One app's tools: `echo` and `level`, nothing behind them (no engine, no design). `log` records
/// every listing and call in order.
#[derive(Default)]
struct Echo {
    calls: Vec<Value>,
    log: Vec<String>,
}

impl Tools for Echo {
    fn instructions(&self) -> String {
        "Echo test tools.".into()
    }
    fn list(&mut self) -> Vec<Value> {
        self.log.push("list".into());
        vec![
            tool("echo", "Echo", "Say `text` back.", object(json!({"text": string("What to say")}), &["text"]), Effect::ReadOnly),
            tool("level", "Level", "Set the level.", object(json!({"level": {"type": "string", "enum": ["low", "high"]}}), &[]), Effect::Edits),
        ]
    }
    fn call(&mut self, name: &str, args: &Value) -> Value {
        self.log.push(format!("call {name}"));
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

fn ping(id: usize) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "ping"})
}

fn notification() -> Value {
    json!({"jsonrpc": "2.0", "method": "notifications/progress"})
}

fn reply(s: &mut Server<Echo>, msg: &Value) -> Option<Value> {
    s.handle_line(&msg.to_string()).map(|r| serde_json::from_str(&r).unwrap())
}

/// A batch answers each request in it and none of its notifications; a batch of notifications
/// only gets no reply at all; a batch holds at most 1000 messages.
#[test]
fn batches_answer_their_requests_up_to_the_limit() {
    let mut s = Server::new(info(), Echo::default());
    let mixed =
        json!([ping(1), notification(), {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "echo", "arguments": {"text": "b"}}}]);
    let replies = reply(&mut s, &mixed).unwrap();
    assert_eq!(
        replies,
        json!([{"jsonrpc": "2.0", "id": 1, "result": {}}, {"jsonrpc": "2.0", "id": 2, "result": {"content": [{"type": "text", "text": "b"}]}}])
    );
    assert_eq!(reply(&mut s, &json!([notification(), notification()])), None);
    let full: Vec<Value> = (0..1000).map(ping).collect();
    assert_eq!(reply(&mut s, &Value::Array(full)).unwrap().as_array().map(Vec::len), Some(1000));
    let over: Vec<Value> = (0..1001).map(ping).collect();
    let r = reply(&mut s, &Value::Array(over)).unwrap();
    assert_eq!(r, json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32600, "message": "batch too large"}}));
}

/// We send no requests, so a response is never answered, alone or in a batch; a message with
/// neither a method nor a result, or one that is not an object, is refused; a blank line is skipped.
#[test]
fn a_response_we_never_asked_for_is_ignored() {
    let mut s = Server::new(info(), Echo::default());
    assert_eq!(reply(&mut s, &json!({"jsonrpc": "2.0", "id": 1, "result": {}})), None);
    assert_eq!(reply(&mut s, &json!({"jsonrpc": "2.0", "id": 2, "error": {"code": -1, "message": "no"}})), None);
    assert_eq!(
        reply(&mut s, &json!([{"jsonrpc": "2.0", "id": 3, "result": {}}, ping(4)])).unwrap(),
        json!([{"jsonrpc": "2.0", "id": 4, "result": {}}])
    );
    let r = reply(&mut s, &json!({"jsonrpc": "2.0", "id": 5})).unwrap();
    assert_eq!(r, json!({"jsonrpc": "2.0", "id": 5, "error": {"code": -32600, "message": "missing `method`"}}));
    let r = reply(&mut s, &json!(42)).unwrap();
    assert_eq!(r, json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32600, "message": "message must be an object"}}));
    assert_eq!(s.handle_line("   "), None);
    assert!(s.tools().log.is_empty());
}

/// A version we speak is echoed; one we don't, or none, gets ours.
#[test]
fn an_unknown_protocol_version_gets_ours() {
    let mut s = Server::new(info(), Echo::default());
    let version = |s: &mut Server<Echo>, params: Value| {
        reply(s, &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": params})).unwrap()["result"]["protocolVersion"].clone()
    };
    for v in SUPPORTED_VERSIONS {
        assert_eq!(version(&mut s, json!({"protocolVersion": v})), json!(v));
    }
    assert_eq!(version(&mut s, json!({"protocolVersion": "1999-01-01"})), json!(PROTOCOL_VERSION));
    assert_eq!(version(&mut s, json!({"protocolVersion": 7})), json!(PROTOCOL_VERSION));
    assert_eq!(version(&mut s, json!({})), json!(PROTOCOL_VERSION));
    assert!(!s.is_initialized(), "initialized only by the client's notification");
}

/// `arguments: null` is no arguments; a value outside a property's `enum` is refused before the
/// tool runs.
#[test]
fn null_arguments_are_none_and_enums_are_checked() {
    let mut s = Server::new(info(), Echo::default());
    let call = |s: &mut Server<Echo>, id: u64, args: Value| {
        reply(s, &json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": "level", "arguments": args}})).unwrap()
    };
    assert!(call(&mut s, 1, Value::Null).get("result").is_some());
    let none = reply(&mut s, &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "level"}})).unwrap();
    assert!(none.get("result").is_some(), "{none}");
    assert!(call(&mut s, 3, json!({"level": "high"})).get("result").is_some());
    let r = call(&mut s, 4, json!({"level": "loud"}));
    assert_eq!(r["error"], json!({"code": -32602, "message": "level: `level` must be one of [\"low\",\"high\"]"}));
    let r = call(&mut s, 5, json!([]));
    assert_eq!(r["error"], json!({"code": -32602, "message": "level: arguments must be an object"}));
    let args: Vec<&Value> = s.tools().calls.iter().map(|c| &c["args"]).collect();
    assert_eq!(args, [&json!({}), &json!({}), &json!({"level": "high"})]);
}

/// An app that serves no resources, templates or prompts lists none of each.
#[test]
fn the_default_lists_are_empty() {
    let mut s = Server::new(info(), Echo::default());
    let result = |s: &mut Server<Echo>, method: &str| reply(s, &json!({"jsonrpc": "2.0", "id": 1, "method": method})).unwrap()["result"].clone();
    assert_eq!(result(&mut s, "resources/list"), json!({"resources": []}));
    assert_eq!(result(&mut s, "resources/templates/list"), json!({"resourceTemplates": []}));
    assert_eq!(result(&mut s, "prompts/list"), json!({"prompts": []}));
}

/// `Tools::call` runs right after the listing its arguments were checked against, every time,
/// whatever was listed before it; a refused call is listed and never run.
#[test]
fn every_call_runs_right_after_its_listing() {
    let mut s = Server::new(info(), Echo::default());
    let call = |name: &str, args: Value| json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": args}});
    reply(&mut s, &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}));
    reply(&mut s, &call("echo", json!({"text": "a"})));
    reply(&mut s, &call("echo", json!({})));
    reply(&mut s, &json!([call("level", json!({})), call("echo", json!({"text": "b"}))]));
    assert_eq!(s.tools().log, ["list", "list", "call echo", "list", "list", "call level", "list", "call echo"]);
}
