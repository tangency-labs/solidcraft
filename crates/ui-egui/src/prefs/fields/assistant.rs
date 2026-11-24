//! Stored preferences on the Assistant page (`PAGE_ASSISTANT`).
//!
//! Each entry is one field of `Prefs`, in `preference_settings!`'s form:
//! `field: Type = default, PAGE[, accept(range)][ => control(...)];`. Add a new preference
//! to the file of the page whose Defaults resets it, in alphabetical order by field name, so
//! two new preferences rarely land on the same line.

macro_rules! assistant_preferences {
    ($next:ident $(, $rest:ident)* ; $($fields:tt)*) => {
        $next! { $($rest),* ; $($fields)*
            agent_chat_open_at_start: bool = false, PAGE_ASSISTANT;
            /// Whether the chat is in its own window instead of the side panel, and that window's place
            /// and size as the system last reported them (`[x, y, width, height]`, points; `x` can be
            /// negative or past the main screen when the window sits on another one).
            agent_chat_popped: bool = false, PAGE_ASSISTANT;
            /// Changes the agent may make for one message; 0 is no limit.
            agent_chat_turn_limit: u32 = crate::agent_chat::gate::DEFAULT_TURN_LIMIT, PAGE_ASSISTANT;
            agent_chat_window: Option<[f32; 4]> = None, PAGE_ASSISTANT;
            /// When SolidCraft starts: connect the agent started last (`agent_last`, else the one a new
            /// conversation starts with), and open the panel (#7009). Off by default: an agent starts
            /// only when a message is sent or Connect is pressed, and the panel opens when asked for.
            agent_connect_at_start: bool = false, PAGE_ASSISTANT;
            /// Let AI agents connect: the localhost control channel (MCP) listens on
            /// `agent_control_port`, unless `--control`/`SOLIDCRAFT_CONTROL_PORT` name one for the run.
            /// Turning it on or off, or changing the port, takes effect at once ([`crate::control_switch`]).
            /// Off until the user turns it on, so a new install listens on no port.
            agent_control: bool = false, PAGE_ASSISTANT;
            /// The control channel's port. The default one takes the next free port when it is taken.
            agent_control_port: u16 = 27878, PAGE_ASSISTANT;
            /// An agent's slow commands over the control channel (MCP) compute off the window thread,
            /// so the window stays live while they run (#1989, [`crate::control_job`]), the default. Off:
            /// they run on the window thread, and the window waits for each.
            agent_off_thread: bool = true, PAGE_ASSISTANT;
            agent_permissions: std::collections::BTreeMap<String, crate::agent_chat::agents::Mode> = Default::default(), PAGE_ASSISTANT;
            /// Keep one Assistant conversation across open design tabs and Edit in Place instead of a
            /// separate chat per tab (#16282). Off by default so each tab keeps its own chat.
            agent_shared_session: bool = false, PAGE_ASSISTANT;
            /// Each reply's thinking (the dimmed text a model streams while it works) starts folded, with
            /// a control to open it (#17072). Off: it is shown open, as before.
            agent_thinking_folded: bool = true, PAGE_ASSISTANT;
            /// AI features (docs/agent-chat.md): the master switch. Off: no AI button, shortcut, menu
            /// entry or panel, and nothing is looked for or started.
            ai_features: bool = true, PAGE_ASSISTANT;
            /// How much the agent may do without asking (`ask`, `edits`, `all`), and per agent.
            ai_permissions: crate::agent_chat::agents::Mode = crate::agent_chat::agents::Mode::Edits, PAGE_ASSISTANT;
            /// The Assistant button in the application bar (the shortcut and View menu entry stay).
            ai_toolbar_button: bool = true, PAGE_ASSISTANT;
            /// Add SolidCraft's MCP server to the AI apps installed on this computer at start-up
            /// ([`crate::ai_connect`]); off takes the entries out again.
            connect_ai_apps: bool = true, PAGE_ASSISTANT;
            /// Compact output for agents connected through the MCP server (`solidcraft-cli mcp`, docs/mcp.md):
            /// `inspect_design` summarises sketches and tool JSON is not pretty-printed. Off: as always.
            /// The server asks once per agent session (`mcp.options`), so a change applies to the next one.
            mcp_compact: bool = false, PAGE_ASSISTANT;
        }
    };
}
