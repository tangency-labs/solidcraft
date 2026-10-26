# In-app agent chat over ACP

Status: desktop only, behind Preferences → AI → *Show AI features* (on by default; off hides and
stops everything). The user guide is
[docs/agent-chat.md](../agent-chat.md). This note records what I found about the Agent Client
Protocol and the agents that speak it, how the chat is built, and what's still open.

## Goal

A chat panel inside SolidCraft that drives a coding agent **the user already has and is already
signed in to** (Claude Code, Codex, Antigravity, …). SolidCraft holds no API keys and no
accounts: the agent runs as a child process under the user's own login, and SolidCraft gives
it one thing, its MCP server bridged to the open window, so the agent models on the design the
user is looking at.

## The protocol

[ACP](https://agentclientprotocol.com) is JSON-RPC 2.0 over the agent's stdin/stdout, one
message per line, protocol version `1`. The editor (here SolidCraft) is the *client*; the agent
is the *server*, started as a subprocess. Zed introduced it to embed Claude Code and Codex;
the Rust SDK (`agent-client-protocol`, Apache-2.0) and the schema crate
(`agent-client-protocol-schema`, Apache-2.0) are the reference.

What the prototype uses:

| Direction | Message | Use |
|---|---|---|
| → agent | `initialize {protocolVersion: 1, clientCapabilities, clientInfo}` | We declare no file system and no terminal (`fs.readTextFile/writeTextFile: false`, `terminal: false`), and terminal login (`auth.terminal: true`, plus the older `_meta["terminal-auth"]`). The reply carries `agentInfo` and `authMethods`. |
| → agent | `session/new {cwd, mcpServers}` | `mcpServers` holds one stdio server named `solidcraft-app`: `solidcraft-cli mcp --connect 127.0.0.1:PORT`. Its `env` contains ACP `name`/`value` entries for `XDG_DATA_HOME` (when set) and the effective `SOLIDCRAFT_CONTROL_DIR`. No token is sent to the agent. `cwd` is the open design's folder (else SolidCraft's own `agent` folder). |
| → agent | `session/prompt {sessionId, prompt: [content]}` | One text block. The reply comes at the end of the turn with `stopReason` (`end_turn`, `cancelled`, `refusal`, …). |
| → agent | `session/cancel {sessionId}` (notification) | Cancel button. Open permission prompts are answered `cancelled` first, as the spec asks. |
| → agent | `authenticate {methodId}` | Only for an "agent" login method the user clicks (the agent then opens the browser itself). |
| ← agent | `session/update` | `agent_message_chunk`, `agent_thought_chunk` (streamed text), `tool_call` / `tool_call_update` (`toolCallId`, `title`, `kind`, `status`: pending, in_progress, completed, failed, `content`), `plan` (entries with status). Mode, command and usage updates are ignored for now. |
| ← agent | `session/request_permission {toolCall, options: [{optionId, name, kind}]}` | Shown as a prompt in the conversation; the answer is `{outcome: {outcome: "selected", optionId}}` or `{outcome: "cancelled"}`. `kind` is `allow_once`, `allow_always`, `reject_once` or `reject_always`. |
| ← agent | `fs/*`, `terminal/*` | Not offered, answered "method not found". |
| ← agent | `_auth/status_update` | An extension both current adapters send ("Not logged in"); shown next to the agent's name. |

Errors: an agent that is not signed in answers `session/new` (Codex) or the first
`session/prompt` (Claude) with JSON-RPC error `-32000 "Authentication required"`.

## The agents

All checked on 2026-10-14 with the packages installed into a user prefix
(`npm i -g --prefix ~/…`). Licences are from each package's own metadata; nothing is bundled
with SolidCraft, the user installs the agent.

| Agent | How it speaks ACP | Package / licence | Version probe | Login |
|---|---|---|---|---|
| Claude Code | `claude-agent-acp` adapter (renamed from `@zed-industries/claude-code-acp`, which is now deprecated) | `@agentclientprotocol/claude-agent-acp` 0.90.0, Apache-2.0. It depends on `@anthropic-ai/claude-agent-sdk`, which is under Anthropic's own terms ("SEE LICENSE IN README.md"): fine for the user to install, not something we would vendor. | `--version` prints `0.90.0` | Terminal method. Offered only when the client declares terminal login. On a desktop: "Claude Subscription" = `claude-agent-acp --cli auth login --claudeai`, "Anthropic Console" = `… --cli auth login --console`. Over SSH (no browser) it offers `claude-login` = `… --cli` (the TUI `/login`). The login is Claude Code's own: `~/.claude/.credentials.json` (Linux, Windows) or the macOS keychain. |
| Codex | `codex-acp` adapter (`@zed-industries/codex-acp` 0.16 moved to `@agentclientprotocol/codex-acp`) | `@agentclientprotocol/codex-acp` 2.2.2, Apache-2.0 | `--version` (0.16 rejected the flag; 2.x prints it) | Agent method `chat-gpt` (the adapter opens the browser for the ChatGPT login when the client calls `authenticate`), and `api-key` for a key the user configures in Codex. 0.16 also listed `env_var` methods for `CODEX_API_KEY` / `OPENAI_API_KEY`. The login is Codex's own: `~/.codex/auth.json`. |
| Google Antigravity | a separate server, `agy_acp_server.par` (`.exe` on Windows), from the ACP registry's zip (`agy-acp-server-1.4.0-…`, with `localharness_external` beside it, ~330 MB). The `agy` command (1.4.0) rejects `--acp`; its `--input-format stream-json` print mode is not ACP. Started without arguments (the registry's Linux entry passes an empty `--uid=`, not needed here). | proprietary (Google's terms) | none: `--version` prints a build date and takes about two seconds, so it is not asked | Agent methods `oauth-personal`, `oauth-business`, `gemini-api-key` (`GEMINI_API_KEY`), `agent-platform`; `session/new` answers "Authentication required" until one is chosen. Settings in `~/.gemini/antigravity-acp/settings.json`. |
| Gemini CLI (retired) | `gemini --acp` | `@google/gemini-cli` 0.64, Apache-2.0 | `--version` | Google ended it for consumer accounts on 2026-06-18 in favour of Antigravity. Still found when installed (enterprise licences may keep it working) but no longer offered for install. Its own login `~/.gemini/oauth_creds.json` or a key in the environment. |
| OpenCode | built in: `opencode acp` | `opencode-ai` 1.19, MIT | `--version` | Its own providers' logins (`~/.local/share/opencode/auth.json`) or keys in the environment. |
| XAI Grok | built in: `grok agent stdio` | [Grok Build](https://github.com/xai-org/grok-build), Apache-2.0; installed separately | `grok --version` prints `grok 1.0.58 (…) [alpha]` (checked 2026-10-15; in-app connection also checked with 1.0.60) | Its own `~/.grok/auth.json` or `XAI_API_KEY`. `grok login` signs in in the browser; ACP advertises `cached_token` and `grok.com` authentication methods. |
| Others in the ACP registry | `copilot --acp`, `qwen --acp`, `opencode acp`, `goose acp`, `cline --acp`, `kimi acp`, … | mixed; several are proprietary, one is GPL and one AGPL (we only start them as programs, never link them) | `--version` | each its own |

The registry (`https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json`) lists
about forty agents with their `npx`/binary distribution; a later version could read it to fill
the picker instead of the hard-coded table.

### The auth story

- **SolidCraft never sees a credential.** It doesn't pass API keys, doesn't store tokens and
  doesn't open login pages by itself. Each agent keeps its own login where its CLI always does
  (Claude Code: `~/.claude` or the keychain; Codex: `~/.codex`; Gemini and Antigravity: `~/.gemini`). A user
  who already uses the agent in a terminal is already signed in, and the chat simply works.
- **Not signed in:** the agent answers "Authentication required". The panel switches to
  *Sign-in needed* and lists the agent's own `authMethods`:
  - *terminal* methods are shown as the exact command to run in a terminal (with Copy), e.g.
    `claude-agent-acp --cli auth login --claudeai`; the user runs it, then presses Connect;
  - *agent* methods (Codex's ChatGPT login) get a button that calls `authenticate`; the agent
    opens the browser itself;
  - *env_var* methods say which variable to set in the user's environment. The panel has no
    field for a key, on purpose.
- Verified here, against both adapters, with an empty home folder (no credentials, so nobody's
  account was used): Claude's adapter returns `-32000 Authentication required` on the first
  prompt and offers the terminal login methods; Codex's returns it on `session/new` and offers
  `chat-gpt` and `api-key`. I stopped at that step.

## How it works

```
SolidCraft window ──(child process, own process group, ACP over stdio)──> agent (claude-agent-acp, codex-acp, agy_acp_server, opencode acp, …)
      ▲                                                                     │ starts, per session/new
      │ the chat's own endpoint (loopback, fresh token, gate)               ▼
      └──────────────────────────────────────────────────── solidcraft-cli mcp --connect 127.0.0.1:PORT
```

Code: `crates/ui-egui/src/agent_chat/`.

| File | What |
|---|---|
| `backend.rs` | The `ChatBackend` trait and its `Event`s: the only thing the panel talks to. |
| `acp.rs`, `acp_client.rs` | The protocol messages, and `AcpBackend`, the first backend. |
| `api_client.rs` | `ApiBackend`: an OpenAI-compatible or Anthropic-compatible endpoint, with SolidCraft running the tool loop. |
| `agents.rs` | The agents (found and added), permission modes, "always allow" answers. |
| `settings.rs` | Preferences → AI and the Add agent dialog. |
| `process.rs` | The agent as a child process in a process group of its own; stopping it ends the group. |
| `discover.rs` | Which agents are installed, their versions, whether they look signed in. |
| `markdown.rs`, `markdown_view.rs` | The agent's text as Markdown: a bounded reader, and its drawing (parsed once per item, again only when the text changes). |
| `gate.rs` | What the agent's tool requests may do: run, wait for Allow/Deny, or never. |
| `mod.rs` | The conversation, turns (undo as one step, the per-turn limit), held requests. |
| `panel.rs` | The panel: docking, minimising, picker, sign-in, conversation, input. |

### A pluggable backend

The panel doesn't know which protocol it's talking. A backend implements `ChatBackend` (`poll`
for events, `prompt`, `cancel`, `answer_permission`, `authenticate`, `set_mode`, `shutdown`) and
gets a `StartContext` with its working folder, the MCP bridge and `tools` (a sender for control
requests, for a backend that runs tools in process). Two backends exist:

- `AcpBackend` drives a coding agent the user already has (the default, and the point of the
  feature: the user's own account, no keys).
- `ApiBackend` talks to an OpenAI-compatible chat completions endpoint (Ollama, LM Studio,
  OpenAI, a custom URL) or an Anthropic-compatible Messages endpoint (Anthropic, or a provider
  that speaks the Messages API), and runs the tool loop itself: it starts the same
  `solidcraft-cli mcp --connect` server an ACP agent would get and is its MCP client
  (`tools/list` becomes the endpoint's tools, `crash_reports` left out), so its calls pass the
  same gate, limits, questions and undo as an ACP agent's. At most 24 model requests per
  message. The HTTP request is the desktop host's (`Services::agent_http`, ureq with no
  redirects); the key comes from the system keychain (`Services::secrets`, the keyring crate)
  for the connection only and is refused over plain HTTP to another computer. The two protocols
  differ only in their wire shape: OpenAI sends the system note as a message, function tools
  and one `tool` message per result; Anthropic sends it as `system`, `input_schema` tools and
  all of a round's `tool_result` blocks in one user message.

Another kind of backend slots in the same way without reshaping the panel.

### Design tools and the gate

On the first Connect the app starts **an endpoint of the chat's own**
(`control_server::listen_agent`): a free loopback port with a fresh random token, written to the
per-user token file where `solidcraft-cli mcp --connect` finds it. The token never passes
through the agent, `SOLIDCRAFT_CONTROL_TOKEN` is removed from the agent's environment, and
`--allow-any-path` is never passed, so the MCP server keeps its #205 folder limits (a request
that names no folders is confined to the chat's working folder).

Requests from that endpoint don't go to the app directly. `pump_tools` passes each one through
`gate::request`:

- **Methods:** only `engine.commands`, `document.inspect`, `engine.execute`, `engine.batch`,
  `ui.view`, `ui.render` and `ui.screenshot` (what the MCP server uses). No clicks, preferences,
  quitting, scripts or the chat itself.
- **Refused:** `diagnostics.*` (crash reports hold other designs and paths), `app.*`.
- **Allow/Deny in the panel:** `file.*`, `doc.*`, render export, any command given a file or
  folder, a picture saved outside the MCP server's scratch folder, and `edit.undo` past the
  turn's first change. The request waits (the endpoint is patient) until the user answers;
  Cancel/Stop declines everything waiting, and after four minutes it is declined (the MCP
  server stops waiting at five).
- **Permission modes** (Preferences → AI, global with a per-agent override): *Ask every time*
  turns every change into a question too; *Auto-allow model edits* (default) is the list above;
  *Allow everything* runs what would ask (never what is refused). "Always allow" on a card
  remembers the command per agent. The agent's own ACP permission questions about SolidCraft's
  tools are answered "allow once" by the client (the gate decides), and others follow the mode
  and the remembered answers; every automatic answer leaves an *Auto-allowed* note.
- **Per-turn limit:** 24 requests that run commands (a batch counts once; reads don't count),
  set in Preferences, and 400 requests of any kind. Past it the agent is told to stop and say
  what's left.
- **Outside a turn:** refused.

What runs goes to the same queue as the control channel's held requests, so file loads, the
agent cursor and the per-request folder scope all apply as for any other client.

### ACP/MCP control-token discovery

The original failure was `unauthorized: send the control token as "token"` when the ACP agent's
MCP child called the SolidCraft control endpoint. The endpoint creates a random token for its
port and writes it to `control_auth::token_path(port)`. The CLI's `Remote` parses the port from
`--connect` and looks up that port's token file for each control request; it does not rely on
`current.port` when the connection has an explicit port.

The token directory can be overridden by `SOLIDCRAFT_CONTROL_DIR`; otherwise it comes from the
platform data directory, which uses `XDG_DATA_HOME` on Linux. Forwarding `XDG_DATA_HOME` alone
does not communicate an explicit `SOLIDCRAFT_CONTROL_DIR` override to an MCP subprocess. The
bridge now derives the directory from `control_auth::token_path(self.port)` and supplies it as
`SOLIDCRAFT_CONTROL_DIR`, while retaining `XDG_DATA_HOME` for other data lookups. Both are
serialized as ACP `env` entries (`{name, value}`). The token itself remains in the private
per-port file and is not included in ACP session configuration, arguments, or logs. The test ACP
agent applies the received `env` entries when it starts the MCP child.

This diagnosis is consistent with the path-resolution code and the functional result below. A
new SolidCraft session authenticated successfully and ran the real `solidcraft-app.inspect_design`
tool against the open **Sample Plate** project. It returned `dirty: false` and geometry
information, made no design changes, and did not return `unauthorized`. This validates the tested
environment; it does not establish behavior for every environment override or for multiple
simultaneous instances.

The release app was built with `cargo build --release -p solidcraft --bin solidcraft -j 1`
(`CARGO_BUILD_JOBS=1` and `nice -n 10` were also applied). The build succeeded with exit code 0
in about 26 minutes and 6 seconds. A previous parallel/heavy build had coincided with a system
freeze, so keep app builds serialized and avoid concurrent Cargo builds. Do not use `cargo clean`
without explicit authorization; Cargo can reuse compatible artifacts from `target`.

### One undo step per message

When a message is sent the chat remembers the design (`Arc<Document>`) and the undo depth. When
the turn ends, the undo snapshots made since are merged into the first one (which holds the
design from before the turn), relabelled "Agent chat: <message>". If the history was trimmed in
the meantime the start is found by the design it holds; if it's gone, nothing is merged. The
conversation shows the merged step with an Undo button while it's still the latest thing done.

### Document ownership

Each open design tab owns an `AgentChat`, including its transcript, backend session, agent
selection, sign-in retry, draft, attachments, pending messages and approval state. `DocSlot`
parks that state alongside its `Session`; switching restores both together. Panel visibility,
placement, agent discovery, endpoint model cache, settings dialogs and the desktop endpoint
starter remain window state. A new tab gets a fresh conversation. Closing a tab drops its
backend; closing the last tab or replacing its design (including programmatic New, Open and
Recover commands or scripts) clears its conversation.
Conversation state is in memory only and does not change the design file format.

Leaving a tab cancels its running turn, declines outstanding approvals, and merges completed
changes into that design's undo history before swapping sessions. Its backend conversation
remains available when the tab is selected again. Pending messages stay with that tab and wait
for Send. Hidden backends are not polled and their tool endpoints never feed the active tab's
gate. Prompts waiting for startup or sign-in also move to the explicit Send queue on leaving.
Late events are received only when their owning tab is selected again; requests without a
running turn are refused, and late permission questions from a stopped turn are cancelled.
Turning off AI or quitting stops hidden backends as well.

Every admitted tool request also carries a shared turn validity flag. Ending the turn or
leaving the tab invalidates it. The app checks it before dispatch, after deferred computation,
before an animated command applies, and before returning or saving a window screenshot. A
cancelled computation or export releases its UI waiting slot without waiting for its worker;
already started file writes are not rolled back. Requests already past the gate cannot apply
commands to or return inspection or screenshot data from the next tab, even if tab indices
change through closing. Ordinary external control requests retain their existing active-design
behavior.

### The agent's own tools

ACP lets the client offer a file system and a terminal; the chat offers neither, and starts the
agent in an empty working folder of SolidCraft's own (`<data>/agent-chat/work`), not the
design's folder. The Claude Code adapter has shell, file and web tools of its own anyway: the
session asks it to turn them off (`_meta.claudeCode.options.disallowedTools`) and not to offer
"bypass permissions". Other agents' own permission requests appear in the panel with their
options.

### Processes

The agent starts in a process group of its own (Unix; on Windows `taskkill /T` ends its tree).
Stop, Close-and-Stop, turning the preference off, the agent chat being dropped and quitting all
end it the same way: its stdin closes (ACP agents end at end of input), then after a grace period
the group gets TERM, and KILL after another. Quitting waits for this (at most about three
seconds). `process.rs`'s tests start a fake agent that leaves helpers behind, once polite and
once ignoring both end of input and TERM, and check that none survive. If SolidCraft itself is
killed, the agent sees its stdin close and ends with its helpers.

### Discovery

As before, plus key variables (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `GEMINI_API_KEY`, …)
reported as *key set*, `CLAUDE_CONFIG_DIR` and `CODEX_HOME` respected, and OpenCode's
`~/.local/share/opencode/auth.json`. Nothing is started except `--version`.

### Panel

Opened from the **AI** button at the toolbar row's right end (the panels give it room and spill
into » as usual; Preferences → AI → *Show button in toolbar* hides it), Ctrl+Shift+A and the View
settings menu. Docked left or right (dragging the header across the window, or Preferences), resizable, folded
to a 30 px strip with Minimise (state dot and a count of open questions), closed with ×. Side,
width, fold and the per-turn limit are preferences, not design data. Ctrl+Shift+A is reserved in the shortcut editor and works from the chat's text box too. The
header has the agent switcher (per conversation) and, for agents with ACP `modes`, the agent's
own mode picker (`session/set_mode`).

### Control channel

`ui.agent_chat` (docs/control-protocol.md) sets the preferences, opens and docks the panel,
connects, sends, answers permission and approval prompts, cancels, undoes the last message and
returns the whole state, for tests and screenshots.

## Still open

| Area | Work |
|---|---|
| Conversation | Diffs, persisted conversation history, resume (`session/load`) |
| Session controls | Model and mode pickers from `configOptions` / `modes`, slash commands |
| Auth | An embedded terminal or "open a terminal here" for terminal logins on each OS; logout |
| Discovery | Read the ACP registry; keychain-aware login check on macOS |
| Packaging | Make sure every desktop package ships `solidcraft-cli` next to `solidcraft` (Flatpak, AppImage, macOS bundle, Windows zip), or serve MCP from the app binary itself |
| Windows | Job objects instead of `taskkill /T` once there's a safe API for them |
| API backend | Streaming answers; images in tool results |
