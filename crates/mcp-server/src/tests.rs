use serde_json::{Value, json};

use crate::{
    Contents, Effect, Info, PROTOCOL_VERSION, Reference, SUPPORTED_VERSIONS, Server, Tools, check_arguments, object, string, text_result, tool,
};

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
    serve_with(Echo::default(), lines)
}

fn serve_with<T: Tools>(tools: T, lines: &[Value]) -> (Server<T>, Vec<Value>) {
    let input: String = lines.iter().map(|l| format!("{l}\n")).collect();
    let mut output = Vec::new();
    let mut s = Server::new(info(), tools);
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

fn reply<T: Tools>(s: &mut Server<T>, msg: &Value) -> Option<Value> {
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

/// An app that serves no resources, templates or prompts lists none of each, says it serves only
/// tools, and has no prompt to fill in or argument to complete.
#[test]
fn the_default_lists_are_empty() {
    let mut s = Server::new(info(), Echo::default());
    let result = |s: &mut Server<Echo>, method: &str| reply(s, &json!({"jsonrpc": "2.0", "id": 1, "method": method})).unwrap()["result"].clone();
    assert_eq!(result(&mut s, "resources/list"), json!({"resources": []}));
    assert_eq!(result(&mut s, "resources/templates/list"), json!({"resourceTemplates": []}));
    assert_eq!(result(&mut s, "prompts/list"), json!({"prompts": []}));
    assert_eq!(result(&mut s, "initialize")["capabilities"], json!({"tools": {"listChanged": false}}));
    for method in ["prompts/get", "completion/complete"] {
        let r = reply(&mut s, &json!({"jsonrpc": "2.0", "id": 2, "method": method, "params": {"name": "describe"}})).unwrap();
        assert_eq!(r["error"]["code"], -32601, "{method}: {r}");
    }
}

/// An app that serves more than tools: a prompt with a required and an optional argument, a
/// resource template, a Markdown resource and a PNG, and suggestions for any argument. `asked`
/// records every prompt filled in and every completion asked for.
#[derive(Default)]
struct Library {
    asked: Vec<String>,
}

impl Tools for Library {
    fn instructions(&self) -> String {
        "Library test.".into()
    }
    fn list(&mut self) -> Vec<Value> {
        Vec::new()
    }
    fn call(&mut self, name: &str, _: &Value) -> Value {
        text_result(format!("no tool `{name}`"), true)
    }
    fn resources(&self) -> Vec<Value> {
        vec![
            json!({"uri": "lib://readme", "name": "Readme", "mimeType": "text/markdown"}),
            json!({"uri": "lib://logo", "name": "Logo", "mimeType": "image/png"}),
        ]
    }
    fn resource_templates(&self) -> Vec<Value> {
        vec![json!({"uriTemplate": "lib://parts/{name}", "name": "Part", "mimeType": "text/markdown"})]
    }
    fn read_resource(&mut self, uri: &str) -> Option<Result<Contents, String>> {
        let markdown = |text: String| Contents::Text { mime_type: "text/markdown".into(), text };
        Some(Ok(match uri {
            "lib://readme" => markdown("# Parts\n\nA \"quoted\" line.".into()),
            "lib://logo" => Contents::Blob { mime_type: "image/png".into(), data: b"foo".to_vec() },
            "lib://parts/broken" => return Some(Err("the part is being rebuilt".into())),
            _ => markdown(format!("# {}", uri.strip_prefix("lib://parts/")?)),
        }))
    }
    fn prompts(&self) -> Vec<Value> {
        vec![json!({"name": "describe", "title": "Describe a part", "arguments": [{"name": "part", "required": true}, {"name": "tone"}]})]
    }
    fn get_prompt(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        self.asked.push(format!("{name} {args}"));
        let text = format!("Describe {} in a {} tone.", args["part"].as_str().unwrap_or_default(), args["tone"].as_str().unwrap_or("plain"));
        Ok(json!({"messages": [{"role": "user", "content": {"type": "text", "text": text}}]}))
    }
    fn complete(&mut self, reference: Reference<'_>, argument: &str, value: &str) -> Vec<String> {
        self.asked.push(format!("{reference:?} {argument}={value}"));
        (0..150).map(|i| format!("{value}{i}")).collect()
    }
}

/// The shared crate serves an app's prompt, resource template and Markdown and binary resources,
/// and tells the client at `initialize` that it does (#13336).
#[test]
fn an_app_serves_prompts_templates_and_markdown_or_binary_resources() {
    let request = |id: u64, method: &str, params: Value| json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
    let (mut s, replies) = serve_with(
        Library::default(),
        &[
            request(1, "initialize", json!({"protocolVersion": PROTOCOL_VERSION})),
            request(2, "prompts/list", json!({})),
            request(3, "prompts/get", json!({"name": "describe", "arguments": {"part": "bracket"}})),
            request(4, "resources/templates/list", json!({})),
            request(5, "resources/read", json!({"uri": "lib://readme"})),
            request(6, "resources/read", json!({"uri": "lib://logo"})),
            request(7, "resources/read", json!({"uri": "lib://parts/bracket"})),
            request(8, "resources/read", json!({"uri": "lib://parts/broken"})),
            request(9, "resources/read", json!({"uri": "lib://nope"})),
        ],
    );
    let result = |i: usize| &replies[i]["result"];
    assert_eq!(
        result(0)["capabilities"],
        json!({"tools": {"listChanged": false}, "resources": {"listChanged": false}, "prompts": {"listChanged": false}, "completions": {}})
    );
    assert_eq!(result(1)["prompts"][0]["name"], "describe");
    assert_eq!(result(2), &json!({"messages": [{"role": "user", "content": {"type": "text", "text": "Describe bracket in a plain tone."}}]}));
    assert_eq!(result(3), &json!({"resourceTemplates": [{"uriTemplate": "lib://parts/{name}", "name": "Part", "mimeType": "text/markdown"}]}));
    // The Markdown goes out as it is, not as a JSON string of it.
    assert_eq!(result(4), &json!({"contents": [{"uri": "lib://readme", "mimeType": "text/markdown", "text": "# Parts\n\nA \"quoted\" line."}]}));
    assert_eq!(result(5), &json!({"contents": [{"uri": "lib://logo", "mimeType": "image/png", "blob": "Zm9v"}]}));
    assert_eq!(result(6), &json!({"contents": [{"uri": "lib://parts/bracket", "mimeType": "text/markdown", "text": "# bracket"}]}));
    assert_eq!(replies[7]["error"], json!({"code": -32603, "message": "the part is being rebuilt"}));
    assert_eq!(replies[8]["error"]["code"], -32002);
    assert_eq!(s.tools().asked, [r#"describe {"part":"bracket"}"#]);
}

/// A prompt is filled in only with the arguments its listing declares, each a string, the
/// required ones present; anything else is invalid params and the app is never asked.
#[test]
fn prompt_arguments_are_checked_before_the_prompt_is_filled_in() {
    let mut s = Server::new(info(), Library::default());
    let get =
        |s: &mut Server<Library>, params: Value| reply(s, &json!({"jsonrpc": "2.0", "id": 1, "method": "prompts/get", "params": params})).unwrap();
    for (params, message) in [
        (json!({"arguments": {"part": "a"}}), "missing prompt `name`"),
        (json!({"name": "summarise"}), "unknown prompt `summarise`"),
        (json!({"name": "describe"}), "describe: missing required argument `part`"),
        (json!({"name": "describe", "arguments": ["a"]}), "describe: arguments must be an object"),
        (json!({"name": "describe", "arguments": {"part": 3}}), "describe: argument `part` must be a string"),
        (json!({"name": "describe", "arguments": {"part": null}}), "describe: argument `part` must be a string"),
        (json!({"name": "describe", "arguments": {"part": "a", "size": "big"}}), "describe: unknown argument `size`; its arguments are: part, tone"),
    ] {
        assert_eq!(get(&mut s, params.clone())["error"], json!({"code": -32602, "message": message}), "{params}");
    }
    assert!(s.tools().asked.is_empty());
    let filled = get(&mut s, json!({"name": "describe", "arguments": {"part": "hinge", "tone": "dry"}}));
    assert_eq!(filled["result"]["messages"][0]["content"]["text"], "Describe hinge in a dry tone.");
}

/// Completion asks the app about a listed prompt or a resource, and sends the first 100
/// suggestions with how many there are; a bad reference or argument is invalid params.
#[test]
fn completion_suggests_up_to_a_hundred_values() {
    let mut s = Server::new(info(), Library::default());
    let complete = |s: &mut Server<Library>, params: Value| {
        reply(s, &json!({"jsonrpc": "2.0", "id": 1, "method": "completion/complete", "params": params})).unwrap()
    };
    let r = complete(&mut s, json!({"ref": {"type": "ref/prompt", "name": "describe"}, "argument": {"name": "part", "value": "br"}}));
    let completion = &r["result"]["completion"];
    assert_eq!(completion["values"].as_array().map(Vec::len), Some(100), "{r}");
    assert_eq!((&completion["values"][0], &completion["total"], &completion["hasMore"]), (&json!("br0"), &json!(150), &json!(true)));
    let r = complete(&mut s, json!({"ref": {"type": "ref/resource", "uri": "lib://parts/{name}"}, "argument": {"name": "name", "value": ""}}));
    assert_eq!(r["result"]["completion"]["values"][99], "99");
    assert_eq!(s.tools().asked, [r#"Prompt("describe") part=br"#, r#"Resource("lib://parts/{name}") name="#]);
    let bad_ref = "`ref` must be a prompt's {type: ref/prompt, name} or a resource's {type: ref/resource, uri}";
    let bad_argument = "`argument` must have a `name` and a `value`, both strings";
    for (params, message) in [
        (json!({"ref": {"type": "ref/prompt", "name": "summarise"}, "argument": {"name": "a", "value": ""}}), "unknown prompt `summarise`"),
        (json!({"ref": {"type": "ref/tool", "name": "x"}, "argument": {"name": "a", "value": ""}}), bad_ref),
        (json!({"ref": {"type": "ref/resource"}, "argument": {"name": "a", "value": ""}}), bad_ref),
        (json!({"argument": {"name": "a", "value": ""}}), bad_ref),
        (json!({"ref": {"type": "ref/prompt", "name": "describe"}, "argument": {"name": "part"}}), bad_argument),
        (json!({"ref": {"type": "ref/prompt", "name": "describe"}}), bad_argument),
    ] {
        assert_eq!(complete(&mut s, params.clone())["error"], json!({"code": -32602, "message": message}), "{params}");
    }
    assert_eq!(s.tools().asked.len(), 2, "a refused completion never reaches the app");
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

/// Open schemas forward extra keys, but still check declared properties and required fields.
#[test]
fn additional_properties_are_forwarded_only_when_allowed() {
    let mut schema = object(json!({"theme": {"type": "string"}}), &["theme"]);
    for additional in [None, Some(false), Some(true)] {
        if let Some(allow) = additional {
            schema["additionalProperties"] = json!(allow);
        } else {
            schema.as_object_mut().unwrap().remove("additionalProperties");
        }
        let defs = vec![tool("prefs", "Preferences", "Set preferences", schema.clone(), Effect::Edits)];
        assert_eq!(check_arguments(&defs, "prefs", &json!({"theme": "dark", "other": 1})).is_ok(), additional == Some(true));
        assert!(check_arguments(&defs, "prefs", &json!({"theme": 1})).is_err());
        assert!(check_arguments(&defs, "prefs", &json!({"other": 1})).is_err());
    }
}
