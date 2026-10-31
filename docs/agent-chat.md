# Agent chat

Agent chat is a side panel where you talk to a coding agent you already use (Claude Code,
Codex, Google Antigravity, OpenCode, Grok Build and others), or to a model behind an
OpenAI-compatible or Anthropic-compatible endpoint you choose (Ollama or LM Studio on your
computer, OpenAI, Anthropic, any other URL), and have it work on the design that's open. A
coding agent runs on your computer under **your own account** with that agent: SolidCraft
doesn't sell or proxy a model and needs no key for it. An endpoint's key, if it needs one, goes
into your system keychain, never into SolidCraft's settings.

Desktop app only.

## Turning it on and off

Everything about it is on one page: **Preferences → AI**.

- **Show AI features** (on by default) is the master switch. Turned off, SolidCraft shows nothing
  of it: no AI button, no shortcut, no menu entry, no panel, and nothing is looked for or
  started. The page stays, so you can turn it back on.
- **Show Assistant button in application bar** (on by default): the **Assistant** icon in the application bar
  row, which stays there however wide the window is. Turn it off to keep only the shortcut and
  the menu entry.
- **Connect the last-used agent when SolidCraft starts** (off by default): each launch starts
  the agent you connected last (the default one if you never connected), in the background,
  once SolidCraft has looked for the installed agents. The panel's header shows **Starting…**,
  then **Ready**, or **Connection failed** with **Retry** in place of **Connect** (the agent is
  missing, could not start, or ended while starting); the conversation says why. A design
  opened in place of the empty one SolidCraft starts with keeps the agent connected, as long
  as nothing was asked in that empty design's chat.
- **Open the Assistant panel when SolidCraft starts** (off by default): the panel (or its own
  window) is shown at each launch.

Neither applies when SolidCraft is started with `--control` for automation.

With AI features on, the panel opens from:

- the **Assistant** icon in the application bar;
- **Ctrl+Shift+A** (Cmd+Shift+A on a Mac), which also works from the chat's own text box;
- **View settings** (the last button of the navigation bar under the view) → **Agent chat**.

Nothing runs until you need it: SolidCraft looks for installed agents (a quick `--version`
each, in the background) when the panel or the AI page first opens, and starts an agent only
when you send a message or press **Connect** (or at launch, when you turned that on above).

## The panel

In the floating layout, Assistant fills the viewport height in the shared rounded panel, with
space around its edges. The compass and navigation bar move into the remaining space beside it. It overlays the model without changing the viewport projection;
clicks and scrolling over the panel stay in the chat. The docked layout retains its side panel.

- **Docking:** on the left, Assistant is the first panel, before vertical history and the browser.
  The drag highlight shows the actual destination bounds. Drag the panel's header (the dotted grip and the title) to the other half of the
  window and let go. The side it would land on lights up while you drag. You can also pick
  **Left** or **Right** under Preferences → AI → Panel. SolidCraft remembers the side.
- **Width:** drag the panel's inner edge. SolidCraft remembers it.
- **Its own window:** the pop-out button in the header (a window with an arrow) moves the chat
  into a window of its own, named after the design, which you can move anywhere, including
  another screen. The window's own close button closes the chat, which opens there again next
  time; **Dock** (the same place in its header) puts it back in the side panel. You can also
  pick **Own window** under Preferences → AI → Panel. SolidCraft remembers where the window
  was and how big.
- **Minimise** (the line in the header) folds the panel into a small floating strip (a side strip in the docked layout) that keeps the chat
  icon, the agent's state, and a count of anything waiting for your answer. Click the icon to
  unfold it. **Close** (×) hides the panel. Neither stops the agent; **Stop** does.
- **The agent switcher** under the title picks the agent for this conversation; switching stops
  the one running. When an ACP agent reports **configuration options** (including OpenCode V2),
  a compact inline strip below it shows the agent's model, reasoning effort, mode, and any other
  supported select options, wrapping when space is tight. Hover a selector for its full label and
  description. Their names and choices come from the agent: not every model offers adjustable
  reasoning effort. Changing the model can change or remove the effort choices. A selection is
  shown as applied only after the agent reports it; a rejected change leaves the confirmed
  selection in place and shows an error.
- **Older agents and API endpoints** keep their model and mode drop-downs when they report those
  choices (an ACP agent's legacy model/mode lists, or an endpoint's `/models`). SolidCraft remembers
  the last model choice for each agent and puts it back on the next session when still available.
  An endpoint's agent also keeps it as its default model. An agent that reports no choices shows
  no drop-down. When these model lists include more than one provider (Hermes and OpenCode name
  each model `Provider · model`, or say `Provider: …` in its description), a provider drop-down
  comes first and the model drop-down lists only that provider's models, without the provider in
  front. It opens on the provider of the model in use; picking another provider changes nothing
  until one of its models is chosen. Models with no provider named are listed under *Other*.
- **Reasoning effort is not permissions.** The agent's thinking setting does not change
  SolidCraft's Ask / Edits / All approval policy. Reasoning options belong to the current session;
  changing them here does not rewrite OpenCode's configuration or another OpenCode session.

Messages use right-aligned user bubbles and plain assistant text. Consecutive tool calls share
an activity group: click a row to inspect its output; failed calls have an error tint. Approval
cards retain the agent’s permission choices, and SolidCraft’s own approvals offer Deny, Always
allow and Allow once. The composer’s permission menu uses the same Ask / Edits / All policy as
Preferences, for the selected agent, and shows the actual changes used against the configured
turn limit. Queue, Send now and Stop keep their existing behavior.

## Files and pictures

A message can carry up to eight files, to show the agent what you mean: a concept picture, a
sketch, a brief, a part to fit around.

- **+** next to **Send** has **Choose a file…** (a file dialog for the types below) and **Paste a
  picture** (the picture on the clipboard).
- **Drop files on the chat.** Files dropped on the panel (or its folded strip) attach to the
  next message, and the app doesn't open them too. While files are dragged over the chat, it says
  so. The window gives no position while something is dragged, so the drop is the chat's when the
  pointer was last seen over the chat; **+** always works.
- The files show as chips above the text box: a picture's thumbnail, the name, and × to take the
  file off. A picture is read off the window thread, so a large one never stops the window: its
  chip shows a spinner meanwhile, and **Send** waits for it. **Send** sends files with no text.
- **Pictures** (PNG, JPEG, GIF, WebP) go to the agent as they are when they are at most 2048 px
  on the long side and 3.75 MB (a picture's base64 then stays under 5 MB, so eight still fit in a
  prompt). Otherwise they are scaled down to 2048 px and re-encoded: as PNG when that fits, as
  JPEG when the picture has no transparency, halved until it fits; a GIF goes as its first frame.
  BMP and TIFF go as PNG. HEIC can't be read: export it as JPEG or PNG first. An agent that takes
  no pictures gets none, and the message says which were left out.
- **Text files** (UTF-8, up to 256 KB: Markdown, plain text, JSON, CSV, YAML, TOML, XML, SVG…)
  go in the message's text, whole as far as the message's 100,000-character cap allows: past it
  a file is cut (and says how much is missing) or named only.
- **CAD files** (STEP, IGES, FreeCAD, Rhino, STL, 3MF, OBJ, glTF) are imported into the design
  at once, as components of their own. A file joins the message once its import is in (its chip
  shows a spinner meanwhile, and **Send** waits), and the message tells the agent it was added; a
  failed import says why and the agent is told nothing of it. Taking a part's chip off the message
  leaves the part in the design (Undo removes it).
- Every file is named in the message. In the conversation the attached pictures show under it as
  thumbnails (hold the pointer over one for a bigger preview, click it to open the viewer, where
  the file's name is shown and ‹ › step through the message's pictures, together with the picture
  of the view); the other files are listed by name. Pictures in any of the formats above are
  decoded in the background, with a spinner until they are in. A file that can't be attached says
  why.

Files stay with their message: one queued while the agent works keeps its files, and one sent
again after a sign-in sends them again. When several queued messages go as one prompt, their
files go with it, at most eight; the rest are named in the chat and not sent. **New chat**
clears the files waiting on the chips too.

## Typing while the agent works

Each design tab has its own conversation, agent session, draft, attachments and queued messages.
A new tab starts with an empty chat. Switching away stops the current turn and keeps its
completed changes in that design's undo history. Returning restores the conversation; queued
messages wait for **Send**. Closing a design closes its conversation. Conversations are kept
only while the tabs remain open.

The input stays enabled during a turn. **Enter** queues the message: it shows in the conversation
as a **Queued** bubble that you can **Edit** or remove (×) until it is sent. Several queued
messages go as one prompt, in order, when the turn ends normally.

- **Send now** (on the bubble, or **Ctrl+Enter** / **Cmd+Enter** in the input) stops the current
  step, waits for the agent to say it stopped, then sends the queue as the next prompt, with a
  short note that the agent's previous step was interrupted. What the interrupted message changed
  undoes as one step, like any other message.
- **Stop** (the square next to the input, or **Esc** in the input) stops the agent and keeps
  the queue: the bubbles wait for **Send**.
- **New chat** (the header button, **Ctrl+Shift+L** / **Cmd+Shift+L** with the chat focused)
  stops what runs, clears the conversation, the queue and the files on the chips, and starts a
  fresh session on the same agent, keeping the agent and its mode. It asks first when a message
  is running or queued, or files are waiting. The design's undo history is untouched.

## Agents

Preferences → AI → **Agents** lists every agent the chat can use, each with its kind (a program or
an endpoint), version or model, whether it looks signed in, **Test**, **Default** (the one a new
conversation starts with), its own permission setting, and **Remove** for the ones you added.

### Found by themselves

SolidCraft looks for agents that speak the [Agent Client Protocol](https://agentclientprotocol.com)
on your `PATH` and in the usual install folders a desktop app's `PATH` often misses (npm's
global folder, nvm, Volta, bun, `~/.local/bin`, `~/.grok/bin`, Homebrew, `%APPDATA%\npm`). For each one it
shows its **version** (from `--version`, given three seconds) and whether a **login was found**.
Version checks and agent sessions also use those folders on their `PATH`, so adapters can find
runtimes such as Node when SolidCraft is opened from Finder or the Dock. Existing absolute
`PATH` entries keep their order; no shell startup files are run.
SolidCraft only checks whether the agent's own login file (`~/.claude/.credentials.json`,
`~/.codex/auth.json`, `~/.gemini/oauth_creds.json`, …) or one of its key variables exists. It never
reads the file and never starts the agent to find out. On macOS Claude Code keeps its login in
the keychain, so SolidCraft doesn't guess there. A found login file may be expired: **Test** or a
chat connection verifies it. A successful ACP connection shows **signed in**; an authentication
rejection shows **not signed in**. These results apply to that agent until a newer connection
result or **Refresh**, which checks the installed agents again. An unrelated connection error
doesn't change the last known login state.

| Agent | What to install |
|---|---|
| Claude Code | `npm install -g @agentclientprotocol/claude-agent-acp` (the ACP adapter for Claude Code) |
| Codex | `npm install -g @agentclientprotocol/codex-acp` |
| Google Antigravity | Google's ACP server, a separate download from Google's `agy` command (which has no ACP mode): take the zip for your system from the [ACP registry](https://agentclientprotocol.com/registry), unzip it and put `agy_acp_server.par` (`agy_acp_server.exe` on Windows) on your `PATH`. The zip also holds `localharness_external`: leave the two together. It signs in itself (browser). |
| OpenCode | `npm install -g opencode-ai` (`opencode acp`) |
| XAI Grok | [Grok Build installer](https://docs.x.ai/build/overview), then `grok login` (`grok agent stdio` speaks ACP) |
| GitHub Copilot CLI, Qwen Code, goose | listed with their install command when missing |

The panel lists agents that aren't installed with their install command and a **Copy** button.
**Refresh** looks again after you install one. **Test** starts an agent briefly, asks it for a
session and ends it again.

### Flatpak

The Flatpak sandbox has no home-folder or network access by default. Agents installed
through nvm or `~/.local/bin` can therefore work in a terminal while SolidCraft reports
that no agent was found. For Claude Code and its ACP adapter, run:

```sh
flatpak override --user io.tangencylabs.solidcraft --filesystem=~/.nvm:ro --filesystem=~/.local/bin:ro --filesystem=~/.local/share/claude:ro --filesystem=~/.claude --filesystem=~/.claude.json --share=network
```

These permissions apply to SolidCraft for your user account:

- `~/.nvm:ro` exposes Node and globally installed npm adapters read-only.
- `~/.local/bin:ro` exposes locally installed executables read-only.
- `~/.local/share/claude:ro` exposes Claude Code's native installation read-only,
  including the targets of links in `~/.local/bin`.
- `~/.claude` and `~/.claude.json` allow Claude Code to read and update its sign-in
  and configuration files.
- `--share=network` lets the adapter reach its service. It also gives SolidCraft
  network access, including making an enabled control server reachable from the host.

Restart SolidCraft after applying the override. Other agents or installation folders
may need different filesystem permissions.

### XAI Grok

Install **Grok Build** and run `grok login` in a terminal to sign in with your Grok account.
In SolidCraft, open **AI → Refresh**, select
**XAI Grok**, then **Connect**. You can make it the default under
**Preferences → AI → Agents**.

SolidCraft reuses Grok Build's sign-in in `~/.grok/auth.json`; it checks only whether that file
exists and does not read or copy its tokens. If sign-in is needed, the panel offers Grok's own
sign-in methods. You can also run `grok login` again and reconnect. Grok controls model access
and usage limits for your account. This connection does not require an API key in SolidCraft.
Grok Build also supports `XAI_API_KEY` for users who choose API authentication.

### Add agent…

- **Installed agent:** your own entry for an installed agent, for example with other arguments.
- **Custom command:** any other program that speaks ACP on stdin and stdout, such as
  `my-agent --acp`. SolidCraft starts it directly, not through a shell.
- **OpenAI-compatible API:** a preset (Ollama `http://localhost:11434/v1`, LM Studio
  `http://localhost:1234/v1`, OpenAI, Mammouth AI `https://api.mammouth.ai/v1`, OpenCode Go `https://opencode.ai/zen/go/v1`) or a custom URL, the model name, and an optional key.
  OpenCode Go models are routed to their endpoint by themselves (`/responses`,
  `/messages` or Gemini for those families, `/chat/completions` for the rest),
  each conversation request carrying the session header Go asks for, so one
  preset covers every model.
  A model the list does not know is retried on the next protocol when Go names
  the mismatch.
- **Anthropic-compatible API:** a preset (Anthropic) or a custom URL, for an endpoint that
  speaks the Anthropic Messages API (`{base}/v1/messages`). This covers Anthropic itself and
  providers that offer an Anthropic-compatible endpoint, such as DeepSeek and Qwen.
- The key goes into the system keychain (Keychain on macOS, Credential Manager on Windows, the
  Secret Service on Linux) and is only ever sent over HTTPS or to this computer. **Test** asks an
  OpenAI-compatible endpoint for its models; an Anthropic-compatible one gets a short message,
  because not every such endpoint lists models (Qwen's has no model list). For these agents
  SolidCraft runs the conversation itself, with the same tools, limits and questions as for a
  coding agent, and no cap on model requests per message (Stop ends it). **Request timeout** is how long one model request may
  take: 300 s by default, from 30 s to an hour. A slow local model (a large model on modest
  hardware, or a long message with many tool results) may need more; raise it here or in the
  endpoint's entry under **Agents**, where a change applies from the next request. A request that
  runs out of time ends the message with the limit it hit and where to raise it.

For Ollama, choosing a model checks its reported capabilities in the background. If its
capabilities list lacks `tools`, a warning below the model picker explains that the model can
only chat and suggests choosing one with tool support. Results are cached in memory per base URL and
model, with a limit of 128 entries. SolidCraft recognises Ollama through `/api/version` at the
endpoint's origin or the Ollama preset. Failed checks and older servers without a capabilities
field show no warning. The check never blocks sending a message.

### Local models

Choose a model that supports native tool calling to read or change the design. With Ollama,
look for `tools` in the model's capabilities; a model that only supports chat cannot operate
SolidCraft. Other OpenAI-compatible endpoints also need a model with native tool support.

If tools were available but a completed turn made no tool calls or changes, the panel shows a
short notice when the reply was empty, or when an explicit edit request received only text.
Ordinary answers to questions do not show it. Edit requests are recognised conservatively from
English opening words such as “make”, “rename”, or “can you move”; other phrasing may stay quiet.
Cancelled and failed turns keep their existing messages. Text describing a call is never run.
A reply that stopped at its length limit, or that the model declined, says that instead of the
tool-calling notice: an endpoint reports it as `max_tokens` or `refusal` (Anthropic), or as
`length`, `content_filter` or a refusal (OpenAI-compatible). A tool call cut off at the length
limit is not run.

### An agent inside WSL (Windows)

An agent installed inside WSL keeps its own settings, skills and sign-in there. Add it as a
**Custom command** that starts it through `wsl.exe`, for example:

```
wsl.exe -d Ubuntu -e bash -lc claude-agent-acp
```

`bash -lc` runs your login shell's setup, so the agent is found where your `PATH` puts it (npm,
nvm, Volta). When the command's program is `wsl` or `wsl.exe`, SolidCraft gives the agent its
working folder and the `solidcraft-app` server's program as WSL shows them (`C:\Users\…` becomes
`/mnt/c/Users/…`). The server itself still runs on Windows and reaches the app at `127.0.0.1`, so
this works with WSL's default networking. If you moved WSL's drive mounts away from `/mnt`
(`automount.root` in `/etc/wsl.conf`), those paths won't match and the agent can't start the server.

## Signing in

If a coding agent isn't signed in, it says so and the panel shows **Sign in to the agent** with
the agent's own ways to sign in:

- **A command for a terminal**, e.g. `claude-agent-acp --cli auth login --claudeai`, with a **Copy**
  button. Run it in a terminal, sign in there, then press **I've signed in: connect again**.
- **A button** for agents that sign in by themselves (Codex's ChatGPT login opens your browser).
- **A variable to set** for agents that take a key from the environment. The panel has no field
  for a coding agent's key, on purpose.

Whatever you typed before signing in is sent once the agent is ready.

## What the agent can do

The agent reaches the open design only through SolidCraft's own tools (the
[MCP server](mcp.md)), which SolidCraft hands it when the chat starts. That server is named
`solidcraft-app`, so an MCP server you set up yourself under the name `solidcraft` (as in
[client configuration](mcp.md#client-configuration)) doesn't take its place:

- **Read the design:** list commands, inspect features, sketches and parameters, measure,
  take pictures of the view.
- **Change the design:** run any modelling command, alone or in a batch. Changes show in the
  window as they happen.
- **One undo per message:** everything the agent changes while answering one message becomes
  a single undo step, named after your message. The conversation shows *N changes, one undo
  step* with **Undo these changes** while nothing else has happened since.
- **Tool cards:** each tool call shows as a card with its state; click it to see what it returned.
- **Design feedback:** with **Preferences → AI › Tell the agent what each call did to the design**
  turned on (it is off by default; it takes effect when the agent next connects), a tool result can
  end with a *Design feedback* block: what the call broke or mended, and changes you made in the
  window between the agent's calls. The agent is told to act on it, and a long result is shortened
  before the block, never through it ([MCP](mcp.md#design-feedback)).
- **Formatted text:** the agent's answers show as Markdown: headings, lists, quotes, code blocks
  (with **Copy**), bold, italic, `code` and links (web links only; each link is a button that opens
  it in your browser). What you type, and the agent's thoughts, stay plain text.
- **Attach view:** tick it to send a picture of the current view with your message, for agents
  that take pictures. The picture is taken when the agent starts on the message and then shows as
  a small thumbnail under it. Hold the pointer over the thumbnail for a bigger preview that stays
  while you do; click it to open the viewer (the message's pictures one at a time, with **‹** and
  **›** or the arrow keys, closed with Esc or a click outside). A long message scrolls inside its
  box, so **Send** and the buttons beside it stay in view.
- **Attach selection:** tick it to send what is selected in the model with your message, so the
  agent knows what "this edge" or "that feature" is. The box shows how many things are selected, counted as you clicked them (the two halves of a rim you clicked once are one).
  Faces and edges go with the names and numbers the agent's tools take and with their size and
  place (so they can be found again after an edit), features with their name and kind, bodies with
  their name. The agent gets them as text after your message; the conversation shows your message
  as typed with the things as chips below it, grouped (Elements, Features, Bodies) with a count;
  hover a chip for what the agent was told about it. At most 64 things go with one message; the
  rest are counted ("+N more"), not dropped silently. The selection is read when you send, also for
  a message that waits in the queue.
- **Pick context:** press it to choose what the message is about on purpose. A picker opens with
  three lists, **Elements** (faces, edges, vertices), **Features** (steps of the timeline) and
  **Bodies**; click things in the model, the timeline or the browser to put them in the list that
  is active (click a list to make it active; click a picked thing again to take it out). **OK**
  puts them as chips above the message box, each with a **×**; they go with the next message like
  an attached selection (and are shown the same way), and the box is empty again afterwards.
  **Cancel** keeps what was picked before.

## Permissions

Preferences → AI → **Permissions** decides what the agent may do without asking, for every agent;
each agent can have its own setting in its row.

- **Ask every time:** every change to the design and every file action waits for **Allow**,
  **Always allow** or **Deny** in the panel.
- **Auto-allow model edits** (the default): modelling commands run at once; saving, opening, exporting,
  importing, starting a new design, any command given a file name, and undoing past the start of
  your message still ask.
- **Allow everything:** nothing asks. The page warns you: the agent can then save, overwrite and
  replace your design.

**Always allow** remembers that command for that agent. The page lists what you allowed this way,
per agent, with **Reset**.

These settings cover SolidCraft's tools. A coding agent's own tools (its shell, its file edits)
follow the agent's own mode, which you pick in the panel's header. When an agent asks before
using one of SolidCraft's tools, SolidCraft answers for you, since its own gate checks the call,
and notes it in the chat as *Auto-allowed*. With **Allow everything**, or after **Always allow**,
the agent's other questions are answered the same way and noted too.

What counts as "a command given a file name": a parameter that holds a file or folder (`file`,
`dir`, `output`, …), or `path` and `source` when the value reads like a file name (a separator, `~`,
a leading dot or a known extension), or any value of those in a command that exports, imports,
saves, opens or inserts. A sweep's or pattern's `path` (curve ids) and an occurrence's `path`
(ids) are geometry and run without a question.

## What it can't do

- **Files wait for you** unless you chose **Allow everything**. **Cancel** or **Stop** declines
  everything that's waiting, and a question nobody answers is declined after four minutes. The
  [MCP server's folder limits](mcp.md) still apply after you allow: the design's folder, the
  recovery folder and the server's own scratch folder.
- **An optional limit per message.** There's none by default. Untick **Unlimited** on the AI
  page to cap the changes the agent may make per message (it starts at 24). Reading the design
  doesn't count. At the limit, it's told to stop and report what's left; send another message to
  let it carry on. **Stop** always works.
- **No crash reports,** whatever the permissions. Crash reports can hold other designs and file
  paths.
- **Not the rest of the app.** It can't click the interface, change settings, quit, or drive the
  chat itself.
- **Its own tools stay off where we can turn them off.** SolidCraft declares no file system and
  no terminal to the agent, runs it in an empty working folder of its own, and asks the Claude
  Code adapter to turn off its shell, file and web tools for the session.

When the panel's agent is stopped, another agent is picked, or SolidCraft quits, the agent and
every program it started (the CLI it wraps, its MCP server) are stopped.

## Privacy

Nothing goes anywhere except to the agent you run, or the endpoint you added. SolidCraft starts
a coding agent as a program on your computer and talks to it over its standard input and
output. What the agent sends to its own service, under your account and that service's terms,
is between you and that service, exactly as when you use the agent in a terminal. SolidCraft
never sees, stores or passes on a coding agent's password, token or key. An endpoint's key is
kept in the system keychain and sent only to that endpoint. SolidCraft's own connection to the
agent's tools uses a random per-launch token in a file only you can read, which never passes
through the agent.

## Trying it without an account

A scripted test agent is in the source tree. It makes a block, rounds it and measures it, with
no model behind it:

```sh
cargo build --release -p solidcraft-ui-egui --example acp_test_agent
```

Then add it under Preferences → AI → **Add agent… → Custom command** with the path of
`target/release/examples/acp_test_agent`. Put "save" in your message to see an Allow / Deny card,
or "ask" to see the agent ask its own permission.

## Benchmarking the Assistant

`solidcraft-cli bench-assistant` runs the eleven tasks of the
[standard model test (v1, #4851)](https://github.com/tangency-labs/solidcraft/issues/4851) against a
desktop app that is already running with a control port, and checks each result on the design
itself. Use it to see how well a local model drives SolidCraft before you post its results.

```sh
solidcraft --control 9271
# In another terminal:
solidcraft-cli bench-assistant --connect 127.0.0.1:9271 \
  --base-url http://localhost:11434/v1 --model qwen2.5:7b \
  --json results.json --markdown results.md
```

Each task replaces the open design with a new one, so save your work first. The command adds the
endpoint as an API agent named "bench", starts a new chat with All permissions for every task,
sends the prompt once, and waits for the turn to end. It runs only the setup the test prescribes
(the box some tasks start from) and does none of the modelling itself. When it finishes it stops
the agent and puts your permission setting back.

- The control token comes from `--token`, `SOLIDCRAFT_CONTROL_TOKEN`, or the app's
  [token file](control-protocol.md#token), in that order.
- `--tasks 1,2,5` runs only those tasks, `--repeat N` runs the test N times, and `--timeout-s`
  (default 300) is each turn's deadline; a turn that runs past it is stopped and fails as "timeout".
- A failed task gets one of the issue's failure categories ("wrote the call as text", "wrong or
  unknown command", "wrong arguments", "the endpoint refused tools", "an empty reply", "only
  talked", "gave up after errors", or "hit the request limit"), or "wrong result" when the calls
  worked but the design is not what the task asks for.
- `--markdown` writes the issue's comment template filled in; add the quantization, download
  size, and runtime before you post it. `--json` keeps each task's failed rules, conversation, and
  design before and after. Both files are rewritten after every task.
