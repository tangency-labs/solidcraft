# Assistant

Board: `Prefs2_Assistant` (1920x1339). Heading: Extend. Subtitle "AI agents in SolidCraft." No "In
profile" tag. Source today: the AI page (`crates/ui-egui/src/agent_chat/settings.rs`, page
`PAGE_AI`). The whole page dims when "Show AI features" is off (the hover says so).

## Availability

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Show AI features | Checkbox | `Prefs.ai_features` | `true` | "Off hides the Assistant everywhere and starts nothing. Everything below dims." | none | |
| Assistant button in the application bar | Checkbox | `Prefs.ai_toolbar_button` | `true` | none | none | |

## Permissions

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Ask first: "Asks for every change" / Make edits: "Edits freely, asks for the rest" / Do everything: "Never asks" | Three radio cards (204 x 65, name and one line) | `Prefs.ai_permissions` (`agent_chat::agents::Mode`) | Make edits (`Mode::Edits`) | "Use with care" on Do everything | none | |
| Always allowed | Collapsed disclosure with the real remembered-approval count and per-agent Reset | `Prefs.agent_always_allow` | empty | existing approval guidance | none | Below permission cards and warnings, before Agents. Retains actual remembered tools. |

## Agents (3)

One card per agent, with an "Add agent..." and a "Refresh" button at the end.

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Agent card (Claude Code "Default" "Signed in"; Codex CLI "Not signed in"; Local model "Key in environment") | Card with name, badges, "Test" (40 x 24), and for added agents "Edit" then "Remove" | `Prefs.agents` (`agent_chat::agents::AgentConfig`), `Prefs.agent_default` | none configured | none | none | Names and states are samples; build from `agents.rs`. Found agents cannot be edited or removed. |
| Permissions | Dropdown (220 wide, "Same as above") | `Prefs.agent_permissions[agent]` (`Mode`) | same as above | none | none | Per agent. |
| Request timeout | Number field (64 wide, aria "Seconds") with "seconds" | the agent's timeout in `AgentConfig` | per agent | none | none | Added API agents only, 30–3600 seconds in steps of 5. |
| Reasoning effort | Existing text field below Request timeout | `AgentConfig.reasoning_effort` | empty | existing reasoning guidance | none | Added API agents only; preserves normalization and next-request updates. |
| Add agent... | Button (109 x 28) | opens the add-agent form | n/a | none | none | |
| Refresh | Button (65 x 28) | refreshes agent states | n/a | none | none | |

## Panel

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Dock the chat: Left / Right / Own window | Segmented | `Prefs.agent_chat_side` (`"left"`, `"right"`) and `Prefs.agent_chat_popped` (Own window) | Right | none | none | Two fields behind one control. |
| Changes per message | Unlimited / Limited dropdown; number field when Limited | `Prefs.agent_chat_turn_limit` (aria "Limit") | `DEFAULT_TURN_LIMIT` = 0 (Unlimited) | none | none | Every value 1–200 remains reachable. Switching from Unlimited to Limited seeds 24; rendering never changes a value. The existing saved-value migration remains unchanged. |

## Connecting

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Let AI agents connect | Checkbox | `Prefs.agent_control` | `true` | "A local channel (MCP). Nothing leaves this computer." | none | |
| Add SolidCraft to AI apps | Checkbox | `Prefs.connect_ai_apps` | `true` | "Off takes the entries out again." | none | |
| Run slow commands in the background | Checkbox | `Prefs.agent_off_thread` | `false` | none | none | |
| Connect the last agent at start | Checkbox | `Prefs.agent_connect_at_start` | `false` | none | none | |
| Open the chat at start | Checkbox | `Prefs.agent_chat_open_at_start` | `false` | none | none | |

## Agent pointer

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Show the agent's pointer | Checkbox | `app.ui.agent_cursor.show` (`agent_cursor::Settings`) | per session | none | none | Session value; in the Snapshot. |
| Speed | Slider, "1.00x" readout, with the path preview (230x84) at the right | `app.ui.agent_cursor.speed` (`Speed`) | 1.0 | none | `agent-pointer-speed` | aria-label "Pointer speed". |
| Follow the camera | Checkbox | `app.ui.agent_cursor.follow_camera` | per session | "Look At sketches move the view with the pointer." | none | |

## Mismatches

* `agent_chat_width`, `agent_chat_minimized`, `agent_chat_window`, `agent_last` and `agent_models`
  have no control on the board. Always allowed, Edit and reasoning effort are retained newer controls.
* Agent names and states are sample content.
