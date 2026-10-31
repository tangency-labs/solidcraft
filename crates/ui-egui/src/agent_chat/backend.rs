//! The seam between the chat panel and whatever answers it. The panel (conversation, sign-in,
//! approvals, turns, undo, the tool gate) only talks to a [`ChatBackend`]; the first backend is
//! [`crate::agent_chat::acp_client::AcpBackend`], which drives an agent the user already has over
//! the Agent Client Protocol. Another backend (one that calls a model API itself, say) slots in
//! by implementing the trait and sending its tool calls into [`StartContext::tools`]: they then
//! pass the same gate, count against the same per-turn limit and undo as one step, exactly like
//! the ACP agent's.

use serde_json::Value;

pub use super::acp::{AuthMethod, PermissionOption, ToolStatus};
pub use super::acp::{ConfigChoice, ConfigOption};

/// A connection test keeps authentication failures distinct from transport or protocol errors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TestError {
    AuthRequired,
    Other(String),
}

impl TestError {
    pub fn message(&self) -> &str {
        match self {
            Self::AuthRequired => crate::i18n::tr("ui.agent_chat_state_auth"),
            Self::Other(message) => message,
        }
    }
}

/// One part of a message to the agent.
#[derive(Clone, Debug, PartialEq)]
pub enum PromptPart {
    Text(String),
    /// A picture of the design (PNG), sent only to agents that take images.
    Image {
        mime: String,
        base64: String,
    },
    /// A file the agent reads as a document (a PDF), sent only to agents that take documents.
    Document {
        name: String,
        /// Where it is on this machine (a `file://` link).
        uri: String,
        mime: String,
        base64: String,
    },
}

/// What a backend reports to the panel.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Whether the agent takes documents (PDFs) in a message; sent before [`Event::Ready`].
    Documents(bool),
    /// Whether SolidCraft tools were offered by the backend.
    ToolsAvailable(bool),
    /// Ready for a message. `agent`: its name and version; `images`: it takes pictures.
    Ready {
        agent: String,
        images: bool,
    },
    /// Text it streams (`thought`: its reasoning, shown dimmed).
    Text {
        thought: bool,
        text: String,
    },
    /// A new tool call, or news about one (`input`: its arguments, when the agent says).
    Tool {
        id: String,
        title: Option<String>,
        kind: Option<String>,
        status: Option<ToolStatus>,
        input: Option<Value>,
        output: Option<String>,
    },
    /// Its plan, replaced whole: each entry with its status.
    Plan(Vec<(String, String)>),
    /// It asks before running something of its own; answered with [`ChatBackend::answer_permission`].
    Permission {
        key: u64,
        title: String,
        options: Vec<PermissionOption>,
    },
    /// It needs the user to sign in; `methods` are its own ways to do so.
    AuthRequired {
        methods: Vec<AuthMethod>,
    },
    /// What it says about its login.
    AuthStatus(String),
    /// Its own modes (an ACP agent's `modes`: ask, accept edits, plan, …) and the current one.
    Modes {
        current: String,
        available: Vec<(String, String)>,
    },
    /// Its models (empty `available`: only the current one changed).
    Models {
        current: String,
        available: Vec<super::acp::ModelInfo>,
    },
    /// Why an endpoint's models could not be listed (`None`: they could). It comes before the
    /// [`Event::Models`] of that listing, which then holds only the configured model.
    ModelsNote(Option<String>),
    ConfigOptions(Vec<ConfigOption>),
    ConfigOptionFinished {
        id: String,
        result: Result<Vec<ConfigOption>, String>,
    },
    /// The message is answered (`reason`: why it stopped, empty for a normal end).
    TurnEnded {
        reason: String,
    },
    /// Something failed; the backend is still running unless [`Event::Exited`] follows.
    Error(String),
    /// It ended; its last error output.
    Exited {
        tail: Vec<String>,
    },
}

/// What a backend gets to start.
pub struct StartContext {
    /// Its working folder: an empty folder of SolidCraft's own, not the user's files.
    pub cwd: std::path::PathBuf,
    /// The MCP server that reaches this window (`solidcraft-cli mcp --connect` to the chat's own
    /// endpoint), for backends that start MCP servers; `None` when it could not be set up.
    pub bridge: Option<super::acp::McpBridge>,
    /// Where tool calls run in process go, for backends that run their own tool loop.
    pub tools: Option<std::sync::mpsc::Sender<crate::ControlRequest>>,
    /// Wakes the window when something arrives.
    pub wake: Box<dyn Fn() + Send + Sync>,
}

/// A chat backend. Methods never block the frame; replies arrive through [`ChatBackend::poll`].
pub trait ChatBackend {
    /// What arrived since the last call.
    fn poll(&mut self) -> Vec<Event>;
    /// Send a message (the panel only sends one after [`Event::Ready`] and while no turn runs).
    fn prompt(&mut self, parts: &[PromptPart]);
    /// Stop the message being answered; open permission questions count as cancelled.
    fn cancel(&mut self);
    /// Start a fresh conversation on the running backend (no restart); [`Event::Ready`] follows.
    /// A message being answered is cancelled first.
    fn new_session(&mut self);
    /// Answer an [`Event::Permission`] with one of its options (`None`: cancelled).
    fn answer_permission(&mut self, key: u64, option: Option<&str>);
    /// Ask the backend to sign in by itself with one of its methods (it opens the browser).
    fn authenticate(&mut self, method: &str);
    /// Use one of the models it reported in [`Event::Models`].
    fn set_model(&mut self, _model: &str) {}
    /// Switch to one of the modes it reported in [`Event::Modes`].
    fn set_mode(&mut self, _mode: &str) {}
    /// An endpoint's new request timeout, in seconds, from the next request on.
    fn set_request_timeout(&mut self, _secs: u64) {}
    /// An endpoint's new reasoning effort (empty: send nothing), from the next request on.
    fn set_reasoning_effort(&mut self, _effort: &str) {}
    fn set_config_option(&mut self, _id: &str, _value: &str) {}
    /// Its process, for status and tests.
    fn pid(&self) -> Option<u32> {
        None
    }
    /// End it and everything it started. `wait`: block until done (quitting).
    fn shutdown(&mut self, wait: bool);
}

/// One HTTP request of the API backend (the desktop host sends it; nothing else in the chat
/// reaches the network).
#[derive(Clone, Debug, PartialEq)]
pub struct HttpRequest {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub timeout_s: u64,
}

/// Sends an [`HttpRequest`]: the status and the body, or why it failed.
pub type HttpFn = std::sync::Arc<dyn Fn(&HttpRequest) -> Result<(u16, Vec<u8>), String> + Send + Sync>;

/// The system keychain (the desktop host's), for API keys: never in the preferences.
pub trait Secrets: Send + Sync {
    fn get(&self, id: &str) -> Result<Option<String>, String>;
    fn set(&self, id: &str, secret: &str) -> Result<(), String>;
    fn delete(&self, id: &str) -> Result<(), String>;
}

/// Standard base64 with padding (RFC 4648), for pictures in messages.
pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk.first().copied().unwrap_or(0), chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18u32, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(T.get(((n >> shift) & 63) as usize).copied().unwrap_or(b'A')));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A message's parts as ACP content blocks.
pub fn content_blocks(parts: &[PromptPart]) -> Vec<Value> {
    parts
        .iter()
        .map(|p| match p {
            PromptPart::Text(t) => serde_json::json!({"type": "text", "text": t}),
            PromptPart::Image { mime, base64 } => serde_json::json!({"type": "image", "mimeType": mime, "data": base64}),
            PromptPart::Document { uri, mime, base64, .. } => {
                serde_json::json!({"type": "resource", "resource": {"uri": uri, "mimeType": mime, "blob": base64}})
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_rfc_examples() {
        for (i, o) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")]
        {
            assert_eq!(base64(i.as_bytes()), o);
        }
    }

    #[test]
    fn pictures_go_as_image_blocks() {
        let b = content_blocks(&[PromptPart::Text("hi".into()), PromptPart::Image { mime: "image/png".into(), base64: "AA==".into() }]);
        assert_eq!(b[1]["type"], "image");
        assert_eq!(b[1]["mimeType"], "image/png");
    }

    #[test]
    fn documents_go_as_embedded_resources() {
        let doc = PromptPart::Document {
            name: "brief.pdf".into(),
            uri: "file:///a/brief.pdf".into(),
            mime: "application/pdf".into(),
            base64: "AA==".into(),
        };
        let b = content_blocks(&[doc]);
        assert_eq!(b[0]["type"], "resource");
        assert_eq!(b[0]["resource"]["uri"], "file:///a/brief.pdf");
        assert_eq!(b[0]["resource"]["mimeType"], "application/pdf");
        assert_eq!(b[0]["resource"]["blob"], "AA==");
    }
}
