# solidcraft-mcp-server

The machinery of a Model Context Protocol server: JSON-RPC 2.0 over stdio (one message per
line), the MCP lifecycle (`initialize`, `ping`, `notifications/initialized`), the tools,
resources, prompts and completion methods, tool definitions with their annotations, the check of
a call's arguments against the schema it was listed with, and the content blocks a tool replies
with (text, images).
It holds no app types and depends on no engine or renderer, so any app can serve its own tools
through it.

An app implements `Tools`: its instructions, its tool definitions as they are listed now, how a
tool runs, and the resources, resource templates and prompts it serves, if any. `Server` speaks the
protocol around them, and its `initialize` reply names only what the app serves: the tools always,
`resources` when it has resources or templates, `prompts` when it has prompts, `completions` when
it has either prompts or templates. A resource is read as text or as bytes, each with its MIME
type (`Contents`); bytes go out base64-encoded. A `tools/call` lists the tools, checks the
arguments against that listing (an unknown tool, a missing or unknown argument, a type or enum
mismatch is a JSON-RPC invalid-params error and the tool never runs), then calls the tool.
`prompts/get` checks a prompt's arguments against its listing the same way before the app fills
it in. `completion/complete` refuses a prompt that is not listed before it asks the app, and its
reply holds the first 100 suggestions and how many there are. Hostile input is answered, never fatal: a line longer than `Info::max_line` is refused
in bounded space, bad UTF-8 and bad JSON are parse errors, and serving goes on.

## API

| Item | What it does |
|---|---|
| `Tools` | An app's tools: `instructions`, `list`, `call`, and optionally `resources`, `resource_templates`, `read_resource`, `prompts`, `get_prompt`, `complete` |
| `Contents` | What a resource read sends: `Text` or `Blob`, each with its MIME type; `Contents::json(value)` for JSON |
| `Reference` | What a completion asks about: a `Prompt` by name or a `Resource` by URI or URI template |
| `Server::new(info, tools)` | A server over `tools`; `serve(input, output)` until input closes, `handle_line`, `handle`, `tools()`, `is_initialized()` |
| `Info` | `name`, `title`, `version` for `serverInfo`, and `max_line`, the longest request line read |
| `PROTOCOL_VERSION`, `SUPPORTED_VERSIONS` | The protocol revisions spoken; the newest is offered for one not known |
| `tool(name, title, desc, schema, effect)` | A tool definition with annotations from its `Effect` |
| `Effect` | `ReadOnly`, `Edits`, `Replaces`, `Files`: the approval hints a client is given |
| `object(props, required)`, `string(desc)` | Input-schema pieces: a closed object, a described string |
| `check_arguments(defs, name, args)` | The top-level check `tools/call` makes |
| `text_result(text, is_error)` | A tool result of one text block |
| `image_result(data, mime, text)` | A tool result of an image (base64-encoded here) and its text |
| `base64(data)` | Standard base64 with padding (RFC 4648) |
