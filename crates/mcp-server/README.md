# solidcraft-mcp-server

The machinery of a Model Context Protocol server: JSON-RPC 2.0 over stdio (one message per
line), the MCP lifecycle (`initialize`, `ping`, `notifications/initialized`), the tools and
resources methods, tool definitions with their annotations, the check of a call's arguments
against the schema it was listed with, and the content blocks a tool replies with (text, images).
It holds no app types and depends on no engine or renderer, so any app can serve its own tools
through it.

An app implements `Tools`: its instructions, its tool definitions as they are listed now, how a
tool runs, and its resources. `Server` speaks the protocol around them. A `tools/call` lists the
tools, checks the arguments against that listing (an unknown tool, a missing or unknown argument,
a type or enum mismatch is a JSON-RPC invalid-params error and the tool never runs), then calls
the tool. Hostile input is answered, never fatal: a line longer than `Info::max_line` is refused
in bounded space, bad UTF-8 and bad JSON are parse errors, and serving goes on.

## API

| Item | What it does |
|---|---|
| `Tools` | An app's tools: `instructions`, `list`, `call`, and optionally `resources`, `read_resource` |
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
