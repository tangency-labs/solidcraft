//! Agent chat: a side panel that talks to a coding agent the user already has (Claude Code,
//! Codex, Antigravity, OpenCode, …) over the Agent Client Protocol, and gives that agent
//! SolidCraft's MCP server, bridged to this window, as its way to read and change the open design
//! (docs/agent-chat.md, docs/design/in-app-agent-chat.md).
//!
//! Desktop only: the agent is a child process. Off unless turned on in Preferences. SolidCraft
//! never handles the agent's credentials: an agent that needs a login says so, and the panel
//! shows the agent's own login command for the user to run.
//!
//! - `backend.rs`: the [`backend::ChatBackend`] seam; `acp_client.rs` is the ACP backend.
//! - `gate.rs`: what the agent's tool requests may do (run, ask the user first, or never).
//! - `panel.rs`: the panel itself (docking, minimising, the picker, the conversation).
//! - Here: the conversation, turns (each one undoes as one step), and the requests waiting for
//!   Allow or Deny.

// The web build shows only a note: the process side is desktop only.
#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

pub mod acp;
#[cfg(not(target_arch = "wasm32"))]
mod acp_client;
pub mod agents;
#[cfg(not(target_arch = "wasm32"))]
mod api_client;
pub mod attach;
pub mod backend;
pub mod context;
#[cfg(not(target_arch = "wasm32"))]
pub mod discover;
pub mod gate;
mod markdown;
mod markdown_view;
mod panel;
#[cfg(not(target_arch = "wasm32"))]
pub mod paste;
#[cfg(not(target_arch = "wasm32"))]
mod process;
pub mod settings;
pub mod summary;

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};

use serde_json::{Value, json};

use context::Context;

use crate::i18n::{format as trf, tr};
use crate::{ControlRequest, SolveApp};
use backend::{ChatBackend, Event, PermissionOption, PromptPart, ToolStatus};

pub(crate) use panel::{left_inset, tool_space};
pub use panel::{show, tip as panel_tip, toggle};

/// Most entries kept in the conversation; older ones are dropped.
const MAX_ITEMS: usize = 2000;
/// Longest message sent.
const MAX_PROMPT: usize = 100_000;
/// Room kept in a message for the files' names and the words around their text.
const FILE_NAMES_ROOM: usize = 2_000;
/// How long a request waits for Allow or Deny before it is declined (the agent's MCP server gives
/// up on a request after five minutes).
const APPROVAL_TIMEOUT_MS: f64 = 240_000.0;
/// Size of the picture of the view sent with a message.
const VIEW_IMAGE: (u32, u32) = (960, 600);
/// A picture kept for the user to open later, at most this big.
const MAX_PICTURE_BYTES: usize = 4 << 20;
/// A tool call's arguments kept for its card, at most this big (as JSON text).
const MAX_TOOL_INPUT: usize = 64 << 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Off,
    Starting,
    Ready,
    Busy,
    NeedsAuth,
    Exited,
    /// The agent never got ready: none to start, it could not start, or it ended while starting.
    /// Connect reads Retry.
    Failed,
}

impl State {
    fn id(self) -> &'static str {
        match self {
            State::Off => "off",
            State::Starting => "starting",
            State::Ready => "ready",
            State::Busy => "busy",
            State::NeedsAuth => "auth",
            State::Exited => "exited",
            State::Failed => "failed",
        }
    }
    fn label(self) -> &'static str {
        tr(match self {
            State::Off => "ui.agent_chat_state_off",
            State::Starting => "ui.agent_chat_state_starting",
            State::Ready => "ui.agent_chat_state_ready",
            State::Busy => "ui.agent_chat_state_busy",
            State::NeedsAuth => "ui.agent_chat_state_auth",
            State::Exited => "ui.agent_chat_state_exited",
            State::Failed => "ui.agent_chat_state_failed",
        })
    }
    fn running(self) -> bool {
        matches!(self, State::Starting | State::Ready | State::Busy | State::NeedsAuth)
    }
}

/// A picture shown with a message: what the agent got (a picture of the view, or a file the user
/// attached) as the encoded bytes it was sent in (PNG, JPEG, GIF, WebP…), and a note shown with it
/// (the file's name). The thumbnail and the viewer share one texture, decoded off the window
/// thread the first time the picture is drawn.
#[derive(Clone)]
pub struct Picture {
    bytes: Arc<Vec<u8>>,
    note: String,
    /// The picture of the view that Attach view sends, not a file.
    view: bool,
    decode: Arc<Decode>,
}

/// A picture's decoding: started once, and the texture (`None`: not a picture) once it is in.
#[derive(Default)]
struct Decode {
    started: std::sync::atomic::AtomicBool,
    texture: std::sync::OnceLock<Option<egui::TextureHandle>>,
}

/// How a picture can be drawn now.
#[cfg(not(target_arch = "wasm32"))]
pub enum Shown {
    /// Still being decoded: a repaint comes when it is in.
    Loading,
    Ready(egui::TextureHandle),
    /// The bytes are not a picture the app can read.
    Failed,
}

impl Picture {
    pub fn new(bytes: Vec<u8>, note: impl Into<String>) -> Self {
        Self::shared(Arc::new(bytes), note, false)
    }

    fn shared(bytes: Arc<Vec<u8>>, note: impl Into<String>, view: bool) -> Self {
        Self { bytes, note: note.into(), view, decode: Arc::default() }
    }

    /// The picture as a texture, decoded in the background on first use.
    #[cfg(not(target_arch = "wasm32"))]
    fn texture(&self, ctx: &egui::Context) -> Shown {
        use std::sync::atomic::Ordering;
        if let Some(done) = self.decode.texture.get() {
            return done.clone().map_or(Shown::Failed, Shown::Ready);
        }
        if !self.decode.started.swap(true, Ordering::AcqRel) {
            let (bytes, decode, ctx) = (Arc::clone(&self.bytes), Arc::clone(&self.decode), ctx.clone());
            let spawned = std::thread::Builder::new().name("agent-chat-picture".into()).spawn(move || {
                let texture = decode_picture(&bytes).map(|image| ctx.load_texture("sc_agent_chat_picture", image, egui::TextureOptions::LINEAR));
                let _ = decode.texture.set(texture);
                ctx.request_repaint();
            });
            if spawned.is_err() {
                let _ = self.decode.texture.set(None);
                return Shown::Failed;
            }
        }
        Shown::Loading
    }
}

/// Decode a picture in any format the app reads, within limits (it came through the attachment
/// checks already; a hostile header still can't ask for gigabytes here).
#[cfg(not(target_arch = "wasm32"))]
fn decode_picture(bytes: &[u8]) -> Option<egui::ColorImage> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(20_000);
    limits.max_image_height = Some(20_000);
    limits.max_alloc = Some(1 << 30);
    reader.limits(limits);
    let rgba = reader.decode().ok()?.to_rgba8();
    let size = [usize::try_from(rgba.width()).ok()?, usize::try_from(rgba.height()).ok()?];
    Some(egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()))
}

impl std::fmt::Debug for Picture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Picture({} bytes, {:?})", self.bytes.len(), self.note)
    }
}

/// The viewer: the pictures of one message, and the one shown.
#[derive(Clone, Debug)]
pub struct Gallery {
    pictures: Vec<Picture>,
    at: usize,
}

#[derive(Clone, Debug)]
pub enum Item {
    User {
        text: String,
        image: bool,
        /// The view's picture, kept once it is taken (when the agent starts on the message).
        pictures: Vec<Picture>,
        /// What was selected or picked in the model when it was sent ("Attach selection", Pick context).
        context: Context,
        /// The names of the files that went with it.
        files: Vec<String>,
    },
    Agent {
        thought: bool,
        text: String,
    },
    Tool {
        id: String,
        title: String,
        kind: String,
        status: Option<ToolStatus>,
        /// The arguments it was called with, when the agent says (shown under Details, and read
        /// for the card's summary, #1457).
        input: Option<Value>,
        output: Option<String>,
    },
    Plan(Vec<(String, String)>),
    /// The agent asks before running something of its own (its options, its words).
    Permission {
        key: u64,
        title: String,
        options: Vec<PermissionOption>,
        answer: Option<String>,
    },
    /// A request of the agent's that SolidCraft holds for the user (files, replacing the design,
    /// or any change while the mode is "ask"); `tool`: what "Always allow" remembers.
    Approval {
        id: u64,
        title: String,
        tool: String,
        answer: Option<bool>,
    },
    /// A message's changes, merged into one undo step; `doc`: the design right after it.
    Changes {
        steps: usize,
        doc: Arc<solidcraft_engine::doc::Document>,
        undone: bool,
    },
    Notice {
        text: String,
        error: bool,
    },
}

/// The message being answered: what undo and the limits need.
struct Turn {
    /// Deferred requests stop being valid when this turn ends or its tab is left.
    active: Arc<std::sync::atomic::AtomicBool>,
    /// The design and the undo depth before it.
    start_doc: Arc<solidcraft_engine::doc::Document>,
    start_undo: usize,
    /// Requests that ran commands, and requests of any kind.
    changes: u32,
    requests: u32,
    limit_told: bool,
    tools_available: bool,
    tool_called: bool,
    replied: bool,
    edit_requested: bool,
    label: String,
}

/// A message typed while the agent works: shown in the transcript as "Queued" until it is sent.
#[derive(Clone, Debug, PartialEq)]
pub struct Pending {
    pub text: String,
    pub image: bool,
    pub context: Context,
    /// The files attached to it (they go with it).
    pub files: Vec<attach::Attachment>,
}

/// A request held for Allow or Deny.
struct Held {
    id: u64,
    req: ControlRequest,
    since: f64,
}

/// Starts the chat's own control endpoint for the agent's MCP server (the desktop host provides
/// it): requests arriving there go to the sender; the port it listens on.
pub type ControlStarter = Box<dyn FnMut(Sender<ControlRequest>) -> Option<u16>>;

/// A file not yet on the next message: a picture being read off the window thread (so a large one
/// doesn't stop the window), or a CAD file being imported into the design. Its chip shows
/// meanwhile; it keeps the id it was given when attached, so the files stay in the order they
/// were attached in.
pub(crate) struct Loading {
    pub id: u64,
    pub name: String,
    what: Reading,
}

enum Reading {
    /// A picture decoding, or a pasted one encoding: the file, or why not.
    File(Receiver<std::result::Result<attach::Attachment, String>>),
    /// A CAD file's import into the design: its answer, as a control reply.
    Import(Receiver<crate::control::ControlResponse>),
}

impl Loading {
    /// A CAD file being imported (its part lands in the design whatever the message does).
    pub fn importing(&self) -> bool {
        matches!(self.what, Reading::Import(_))
    }
}

/// The model before an ACP agent was sent another, for when it refuses that one.
struct ModelRevert {
    model: String,
    /// The one sent was not listed, so it leaves the list again.
    unlisted: bool,
    /// The model remembered for this agent.
    saved: Option<String>,
}

#[derive(Default)]
pub struct AgentChat {
    /// The panel is shown (View menu, the application bar's chat button, Ctrl+Shift+A).
    pub open: bool,
    /// Only while dispatching this conversation's own admitted tool request. Window state:
    /// creating/opening a design may carry the conversation, but a person's tab switch may not.
    pub(crate) following_document: bool,
    pub items: Vec<Item>,
    pub input: String,
    /// The viewer, open on one message's pictures.
    gallery: Option<Gallery>,
    state: Option<State>,
    agent: String,
    images: bool,
    /// The agent takes documents (PDFs) in a message.
    documents: bool,
    /// Send a picture of the view with the next message.
    pub attach_view: bool,
    /// Send what is selected in the model with the next message.
    pub attach_selection: bool,
    /// How many things the current selection is by clicks, kept while the selection and the design
    /// stay as they are: (the selection, the design's address, the count).
    selected_count: Option<(Vec<solidcraft_engine::Sel>, usize, usize)>,
    /// What the user picked with Pick context for the next message, and how it reads.
    pub picked: Vec<solidcraft_engine::Sel>,
    pub picked_context: Context,
    auth: Vec<acp::AuthMethod>,
    auth_status: Option<String>,
    /// A message typed before the agent was ready: its text, whether the view goes with it, its
    /// place in the conversation, and its files.
    queued: Option<(String, bool, Option<usize>, Vec<attach::Attachment>)>,
    /// Messages typed while the agent works, in order. They go as one prompt when the turn ends
    /// (or at once with Send now); after Stop they wait for Send.
    pub pending: Vec<Pending>,
    /// The queued message being edited, and its text so far.
    pub pending_edit: Option<(usize, String)>,
    /// Send now was pressed: the queue goes as soon as the cancelled turn has ended.
    steering: bool,
    /// The turn was stopped: its queue is not sent by itself.
    stopped: bool,
    /// Why the last turn ended.
    ended: Option<String>,
    /// New chat while a message ran: its late end is not reported.
    discard_end: bool,
    /// The mode to put back when the fresh session is ready.
    restore_mode: Option<String>,
    /// New chat is waiting for the user's yes (a message is running or queued).
    pub confirm_new: bool,
    /// Where the panel was drawn last (for its shortcuts).
    pub panel_rect: Option<egui::Rect>,
    /// Unreserved sheet space, shared by the actual dock and its drag preview.
    pub(crate) dock_space: Option<egui::Rect>,
    /// The last message sent, kept to send again after a sign-in (its files too).
    last: Option<(String, bool, Option<usize>, Vec<attach::Attachment>)>,
    /// Files for the next message (pictures, text files, imported parts), shown as chips.
    pub(crate) attachments: Vec<attach::Attachment>,
    /// Pictures pasted so far, to name the next one.
    pasted: usize,
    /// When the last text paste arrived (input time), so the V key coming up after it isn't a second paste.
    pub(crate) text_pasted_at: Option<f64>,
    /// The chips' thumbnails, by the picture they show.
    thumbs: std::collections::HashMap<u64, egui::TextureHandle>,
    /// The last id given to a file added to a message.
    last_file_id: u64,
    /// Files still being read for the next message.
    pub(crate) loading: Vec<Loading>,
    /// Where the chat was drawn last frame (the panel, or its folded strip), for drops; `None`
    /// when it isn't drawn.
    pub(crate) drop_rect: Option<egui::Rect>,
    /// The command the running agent was started with.
    argv: Vec<String>,
    /// The agent picked for this conversation (the header's switcher); `None`: the default.
    pub agent_id: Option<String>,
    /// The agent the running backend is (for permissions and "always allow").
    running_id: String,
    /// The popped-out chat window was given its saved place (it is open); see `panel::window_builder`.
    pub(crate) window_placed: bool,
    /// The agent's own modes, and the current one.
    modes: Vec<(String, String)>,
    mode: String,
    /// The models the agent offers (none: no picker), and the current one.
    pub models: Vec<acp::ModelInfo>,
    pub model: String,
    /// The provider picked in the header when the models come from several providers (`None`: the
    /// current model's). It only narrows the model list; the agent keeps its model until one is chosen.
    pub model_provider: Option<String>,
    /// Why the running endpoint's models could not be listed, for the picker.
    pub models_note: Option<String>,
    /// What to put back if the running ACP agent refuses the model it was last sent.
    model_revert: Option<ModelRevert>,
    /// The typed model an agent's model option was sent, until it answers.
    typed_model: Option<String>,
    pub config_options: Option<Vec<acp::ConfigOption>>,
    pub config_pending: bool,
    /// An API endpoint's models, kept per endpoint (its base URL) for the next connection.
    models_cache: std::collections::HashMap<String, Vec<acp::ModelInfo>>,
    /// The running API agent's endpoint (empty for other agents).
    running_endpoint: String,
    backend: Option<Box<dyn ChatBackend>>,
    turn: Option<Turn>,
    tools_available: Option<bool>,
    /// The latest completed turn's notice; older notices remain in the transcript.
    turn_warning: Option<String>,
    held: Vec<Held>,
    next_approval: u64,
    /// The chat's control endpoint: the port, and its requests.
    pub tools_port: Option<u16>,
    tools_rx: Option<Receiver<ControlRequest>>,
    tools_tx: Option<Sender<ControlRequest>>,
    /// Settings → AI: the Add agent dialog and connection tests.
    pub settings: settings::State,
    /// Latest authentication evidence from a connection test or a chat, per agent; never saved.
    /// It belongs to the window, not to one tab's conversation.
    login_results: std::collections::HashMap<String, bool>,
    /// Desktop host hook: start the endpoint on demand.
    pub control_starter: Option<ControlStarter>,
    /// The picker is unfolded (it folds to one line once an agent is connected).
    picking: Option<bool>,
    /// This launch connects the agent started last once the installed agents are known ([`begin`]).
    connect_at_start: bool,
    #[cfg(not(target_arch = "wasm32"))]
    detection: Option<discover::Detection>,
    #[cfg(not(target_arch = "wasm32"))]
    detecting: Option<Receiver<discover::Detection>>,
}

impl Drop for AgentChat {
    fn drop(&mut self) {
        self.stop_backend(true);
    }
}

impl AgentChat {
    /// Exchange document state, keeping window services and preferences UI in the window.
    pub(crate) fn swap_document(&mut self, other: &mut Self) {
        std::mem::swap(self, other);
        std::mem::swap(&mut self.open, &mut other.open);
        std::mem::swap(&mut self.following_document, &mut other.following_document);
        std::mem::swap(&mut self.settings, &mut other.settings);
        std::mem::swap(&mut self.control_starter, &mut other.control_starter);
        std::mem::swap(&mut self.models_cache, &mut other.models_cache);
        std::mem::swap(&mut self.window_placed, &mut other.window_placed);
        std::mem::swap(&mut self.connect_at_start, &mut other.connect_at_start);
        // A sign-in proven in one tab holds for that agent in every tab (#9293).
        std::mem::swap(&mut self.login_results, &mut other.login_results);
        #[cfg(not(target_arch = "wasm32"))]
        {
            std::mem::swap(&mut self.detection, &mut other.detection);
            std::mem::swap(&mut self.detecting, &mut other.detecting);
        }
        // Hit regions must be measured again for the incoming conversation.
        self.panel_rect = None;
        self.dock_space = None;
        self.drop_rect = None;
    }

    pub(crate) fn disconnect_document(&mut self, session: &mut solidcraft_engine::Session, wait: bool) {
        finish_turn(self, session);
        self.clear_config();
        self.stop_backend(wait);
        if self.state.is_some() {
            self.state = Some(State::Off);
        }
    }

    fn clear_config(&mut self) {
        if self.config_options.take().is_some() {
            self.models.clear();
            self.model.clear();
            self.modes.clear();
            self.mode.clear();
            self.model_provider = None;
        }
        self.config_pending = false;
        self.typed_model = None;
    }

    fn apply_config_snapshot(&mut self, options: Vec<acp::ConfigOption>) {
        self.config_options = Some(options.clone());
        let model = options.iter().find(|o| o.category == "model");
        self.models = model
            .map(|o| {
                o.options.iter().map(|x| acp::ModelInfo { id: x.value.clone(), name: x.name.clone(), description: x.description.clone() }).collect()
            })
            .unwrap_or_default();
        let current = model.map_or_else(String::new, |o| o.current_value.clone());
        if current != self.model {
            self.model_provider = None;
        }
        self.model = current;
        let mode = options.iter().find(|o| o.category == "mode");
        self.modes = mode.map(|o| o.options.iter().map(|x| (x.value.clone(), x.name.clone())).collect()).unwrap_or_default();
        self.mode = mode.map_or_else(String::new, |o| o.current_value.clone());
    }
    /// Put a file on the next message with an id of its own; its name back.
    fn add_file(&mut self, file: attach::Attachment) -> String {
        let id = self.next_file_id();
        self.place(id, file)
    }
    fn next_file_id(&mut self) -> u64 {
        self.last_file_id += 1;
        self.last_file_id
    }
    /// A file goes where its id says: the order the files were attached in.
    fn place(&mut self, id: u64, mut file: attach::Attachment) -> String {
        file.id = id;
        let name = file.name.clone();
        let at = self.attachments.partition_point(|f| f.id < id);
        self.attachments.insert(at, file);
        name
    }
    /// Files on the next message, read or still being read.
    fn files(&self) -> usize {
        self.attachments.len() + self.loading.len()
    }
    /// Read a file for the next message on a worker thread, in the window's language.
    #[cfg(not(target_arch = "wasm32"))]
    fn load(
        &mut self,
        name: String,
        job: impl FnOnce() -> std::result::Result<attach::Attachment, String> + Send + 'static,
    ) -> std::result::Result<String, String> {
        let (tx, rx) = std::sync::mpsc::channel();
        let language = crate::i18n::language();
        std::thread::Builder::new()
            .name("chat-file".into())
            .spawn(move || {
                let _language = crate::i18n::scope(language);
                let _ = tx.send(job());
            })
            .map_err(|e| trf("ui.agent_chat_file_error", &[("name", &name), ("error", &e.to_string())]))?;
        let id = self.next_file_id();
        self.loading.push(Loading { id, name: name.clone(), what: Reading::File(rx) });
        Ok(name)
    }
    /// A CAD file whose import was asked for: it joins the message once the import is in.
    fn importing(&mut self, name: String, rx: Receiver<crate::control::ControlResponse>) -> String {
        let id = self.next_file_id();
        self.loading.push(Loading { id, name: name.clone(), what: Reading::Import(rx) });
        name
    }

    pub fn state(&self) -> State {
        self.state.unwrap_or(State::Off)
    }
    #[cfg(test)]
    pub(crate) fn set_state(&mut self, state: State) {
        self.state = Some(state);
    }
    fn push(&mut self, item: Item) {
        if self.items.len() >= MAX_ITEMS {
            self.items.drain(..MAX_ITEMS / 10);
        }
        self.items.push(item);
    }
    /// Keep the picture just sent with the message at `at` in the conversation (the user can open
    /// it). A message sent again (after a sign-in) has its picture already: nothing is added, and
    /// never to another message.
    fn keep_picture(&mut self, at: Option<usize>, png: Vec<u8>) {
        if let Some(Item::User { image: true, pictures, .. }) = at.and_then(|i| self.items.get_mut(i))
            && !pictures.iter().any(|p| p.view)
            && png.len() <= MAX_PICTURE_BYTES
        {
            pictures.push(Picture::shared(Arc::new(png), "", true));
        }
    }
    /// Say that the agent refused a model, and in its words why.
    fn model_refused(&mut self, model: &str, error: &str) {
        let text = trf("ui.agent_chat_model_rejected", &[("model", model), ("error", error)]);
        self.notice(text.chars().take(MODELS_NOTE_CAP).collect::<String>(), true);
    }
    fn notice(&mut self, text: impl Into<String>, error: bool) {
        self.push(Item::Notice { text: text.into(), error });
    }
    /// Requests waiting for Allow or Deny.
    pub fn waiting(&self) -> usize {
        self.held.len() + self.items.iter().filter(|i| matches!(i, Item::Permission { answer: None, .. })).count()
    }

    fn stop_backend(&mut self, wait: bool) {
        self.steering = false;
        if let Some(mut b) = self.backend.take() {
            b.cancel();
            b.shutdown(wait);
        }
        for h in self.held.drain(..) {
            let _ = h.req.reply.send(json!({"ok": false, "error": "the agent chat stopped"}));
        }
        for i in &mut self.items {
            if let Item::Approval { answer: a @ None, .. } = i {
                *a = Some(false);
            }
            if let Item::Permission { answer: a @ None, .. } = i {
                *a = Some(tr("ui.agent_chat_cancel").to_string());
            }
        }
    }

    /// Answer the agent's own permission question at `index` (`None`: cancelled).
    pub fn answer(&mut self, index: usize, option: Option<&str>) {
        let Some(Item::Permission { key, options, answer, .. }) = self.items.get_mut(index) else { return };
        if answer.is_some() {
            return;
        }
        let label =
            option.and_then(|o| options.iter().find(|x| x.id == o)).map_or_else(|| tr("ui.agent_chat_cancel").to_string(), |o| o.name.clone());
        *answer = Some(label);
        let key = *key;
        if let Some(b) = self.backend.as_mut() {
            b.answer_permission(key, option);
        }
    }

    /// Ask the agent to sign in by itself (an "agent" login method: it opens the browser).
    pub fn authenticate(&mut self, method: &str) {
        if let Some(b) = self.backend.as_mut() {
            b.authenticate(method);
            self.state = Some(State::Starting);
        }
    }

    /// Take in what the backend reported.
    fn on_event(&mut self, e: Event) -> Option<Outcome> {
        let signed_in = match &e {
            Event::Ready { .. } => Some(true),
            Event::AuthRequired { .. } => Some(false),
            _ => None,
        };
        if let Some(signed_in) = signed_in
            && !self.running_id.is_empty()
        {
            self.login_results.insert(self.running_id.clone(), signed_in);
            // A newer chat result supersedes the old test, including one still in flight.
            self.settings.tests.remove(&self.running_id);
        }
        match e {
            Event::Documents(documents) => self.documents = documents,
            Event::ToolsAvailable(available) => self.tools_available = Some(available),
            Event::Ready { agent, images } => {
                self.agent = agent;
                self.images = images;
                self.state = Some(State::Ready);
                self.picking = Some(false);
                // Keep the mode across a new chat.
                if self.config_options.is_none()
                    && let Some(m) = self.restore_mode.take()
                    && self.modes.iter().any(|(id, _)| *id == m)
                    && let Some(b) = self.backend.as_mut()
                {
                    b.set_mode(&m);
                    self.mode = m;
                }
            }
            Event::Text { thought, text } => {
                if !thought
                    && !text.trim().is_empty()
                    && let Some(turn) = self.turn.as_mut()
                {
                    turn.replied = true;
                }
                if let Some(Item::Agent { thought: t, text: last }) = self.items.last_mut()
                    && *t == thought
                {
                    last.push_str(&text);
                } else {
                    self.push(Item::Agent { thought, text });
                }
            }
            Event::Tool { id, title, kind, status, input, output } => {
                if let Some(turn) = self.turn.as_mut() {
                    turn.tool_called = true;
                }
                // Arguments too large to show or read are left with the agent.
                let input = input.filter(|v| serde_json::to_string(v).is_ok_and(|s| s.len() <= MAX_TOOL_INPUT));
                let existing = self.items.iter_mut().rev().find(|i| matches!(i, Item::Tool { id: x, .. } if *x == id));
                if let Some(Item::Tool { title: t, kind: k, status: s, input: i, output: o, .. }) = existing {
                    if let Some(v) = title {
                        *t = v;
                    }
                    if let Some(v) = kind {
                        *k = v;
                    }
                    if status.is_some() {
                        *s = status;
                    }
                    if input.is_some() {
                        *i = input;
                    }
                    if output.is_some() {
                        *o = output;
                    }
                } else {
                    self.push(Item::Tool { id, title: title.unwrap_or_default(), kind: kind.unwrap_or_default(), status, input, output });
                }
            }
            Event::Plan(entries) => {
                // A plan is replaced whole: update this turn's plan where it is.
                let turn = self.items.iter().rposition(|i| matches!(i, Item::User { .. })).unwrap_or(0);
                if let Some(Item::Plan(p)) = self.items.iter_mut().skip(turn).rev().find(|i| matches!(i, Item::Plan(_))) {
                    *p = entries;
                } else {
                    self.push(Item::Plan(entries));
                }
            }
            Event::Permission { key, title, options } => return Some(Outcome::Permission { key, title, options }),
            Event::Modes { current, available } => {
                if self.config_options.is_none() && !available.is_empty() {
                    self.modes = available;
                }
                if self.config_options.is_none() {
                    self.mode = current;
                }
            }
            Event::Models { current, available } => {
                self.model_revert = None;
                if self.config_options.is_none() && !available.is_empty() {
                    if self.models_note.is_some() {
                        // The listing failed: last time's list stays, with the configured model in it.
                        for model in available {
                            if !self.models.iter().any(|m| m.id == model.id) {
                                self.models.insert(0, model);
                            }
                        }
                    } else {
                        if !self.running_endpoint.is_empty() {
                            self.models_cache.insert(self.running_endpoint.clone(), available.clone());
                        }
                        self.models = available;
                    }
                }
                if self.config_options.is_none() {
                    if current != self.model {
                        self.model_provider = None;
                    }
                    self.model = current;
                }
            }
            // Formatted here: the listing's thread has no language of its own.
            Event::ModelsNote(error) => {
                self.models_note =
                    error.map(|error| trf("ui.agent_chat_models_failed", &[("error", &error)]).chars().take(MODELS_NOTE_CAP).collect());
            }
            Event::ModelRejected(message) => {
                let refused = self.model.clone();
                self.model_refused(&refused, &message);
                if let Some(revert) = self.model_revert.take() {
                    if revert.unlisted {
                        let refused = std::mem::take(&mut self.model);
                        self.models.retain(|m| m.id != refused);
                    }
                    self.model = revert.model;
                    self.model_provider = None;
                }
            }
            Event::ConfigOptions(options) => {
                self.apply_config_snapshot(options);
            }
            Event::ConfigOptionFinished { result, .. } => {
                self.config_pending = false;
                let typed = self.typed_model.take();
                match (result, typed) {
                    (Ok(options), _) => {
                        self.apply_config_snapshot(options);
                    }
                    // An agent's own words may not say what it refused ("Internal error").
                    (Err(message), Some(model)) => self.model_refused(&model, &message),
                    (Err(message), None) => self.notice(message, true),
                }
            }
            Event::AuthRequired { methods } => {
                self.clear_config();
                self.auth = methods;
                self.state = Some(State::NeedsAuth);
                self.picking = Some(false);
                // What was asked goes again once the login is done.
                if !self.stopped && self.queued.is_none() {
                    self.queued = self.last.take();
                }
            }
            Event::AuthStatus(s) => self.auth_status = Some(s),
            Event::TurnEnded { reason } => {
                if self.state() == State::Busy {
                    self.state = Some(State::Ready);
                }
                self.ended = Some(reason.clone());
                if std::mem::take(&mut self.discard_end) {
                    return Some(Outcome::TurnEnd);
                }
                match reason.as_str() {
                    "" | "auth" | "error" => {}
                    // Not a missing tool: the answer ran out of room, or the model declined (#9495).
                    "max_tokens" => self.notice(tr("ui.agent_chat_reply_cut_off").to_string(), false),
                    "refusal" => self.notice(tr("ui.agent_chat_reply_declined").to_string(), false),
                    _ => self.notice(format!("({reason})"), false),
                }
                return Some(Outcome::TurnEnd);
            }
            Event::Error(e) => {
                self.notice(e, true);
                match self.state() {
                    State::Starting | State::Failed => self.state = Some(State::Failed),
                    s if !s.running() => self.state = Some(State::Exited),
                    _ => {}
                }
            }
            Event::Exited { tail } => {
                self.clear_config();
                self.state = Some(if matches!(self.state(), State::Starting | State::Failed) { State::Failed } else { State::Exited });
                self.steering = false;
                let last: Vec<String> = tail.into_iter().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect();
                let detail = if last.is_empty() { String::new() } else { format!("\n{}", last.join("\n")) };
                self.notice(format!("{}{detail}", tr("ui.agent_chat_exited")), true);
                self.stop_backend(false);
                return Some(Outcome::TurnEnd);
            }
        }
        None
    }
}

/// What an event asks of the app.
enum Outcome {
    /// A message was answered (or the agent stopped): the turn's bookkeeping ends.
    TurnEnd,
    /// The agent asks before running something of its own: answered by the mode, or asked.
    Permission { key: u64, title: String, options: Vec<PermissionOption> },
}

/// The AI features are on (the master switch in Settings → AI) and this is the desktop app.
pub fn shown(app: &SolveApp) -> bool {
    app.preferences.ai_features && cfg!(not(target_arch = "wasm32"))
}

/// The Assistant button in the application bar is shown (honours the existing preference).
pub fn app_bar_button_shown(app: &SolveApp) -> bool {
    shown(app) && app.preferences.ai_toolbar_button
}

/// The application bar's status mark, independent of panel layout and painting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AssistantMark {
    Quiet,
    Ready,
    Busy,
    NeedsAuth,
    Stopped,
    Waiting(usize),
}

fn assistant_mark_for(state: State, login: &str, configured: bool, waiting: usize, stopped: bool, failed: bool) -> AssistantMark {
    if waiting > 0 {
        return AssistantMark::Waiting(waiting);
    }
    if state == State::NeedsAuth {
        return AssistantMark::NeedsAuth;
    }
    if matches!(state, State::Exited | State::Failed) || stopped || failed {
        return AssistantMark::Stopped;
    }
    match state {
        State::Busy | State::Starting => AssistantMark::Busy,
        // A live session is stronger evidence than an older discovery login probe.
        State::Ready => AssistantMark::Ready,
        State::Off if configured && matches!(login, "signed_in" | "key") => AssistantMark::Ready,
        State::Off if configured && login == "signed_out" => AssistantMark::NeedsAuth,
        _ => AssistantMark::Quiet,
    }
}

pub(crate) fn assistant_mark(app: &SolveApp) -> AssistantMark {
    let chat = &app.agent_chat;
    let current = agents::current(app);
    assistant_mark_for(
        chat.state(),
        current.as_ref().map_or("", |e| e.login),
        current.is_some(),
        chat.waiting(),
        chat.stopped,
        chat.ended.as_deref() == Some("error"),
    )
}

pub(crate) fn assistant_tip(mark: AssistantMark) -> String {
    let state = match mark {
        AssistantMark::Quiet => tr("ui.agent_chat_state_off").to_string(),
        AssistantMark::Ready => tr("ui.agent_chat_state_ready").to_string(),
        AssistantMark::Busy => tr("ui.agent_chat_state_busy").to_string(),
        AssistantMark::NeedsAuth => tr("ui.agent_chat_state_auth").to_string(),
        AssistantMark::Stopped => tr("ui.agent_chat_state_exited").to_string(),
        AssistantMark::Waiting(count) => trf("ui.assistant_waiting", &[("count", &count.to_string())]),
    };
    format!("{} (Ctrl+Shift+A) — {state}", tr("ui.assistant"))
}

/// Whether a permission question is about one of SolidCraft's own tools (which SolidCraft's own
/// gate checks): the agents name MCP tools after their server.
fn solidcraft_tool(title: &str) -> bool {
    title.to_ascii_lowercase().contains("solidcraft")
}

/// `solidcraft-cli` next to this program, else on the search path.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn find_cli() -> Option<std::path::PathBuf> {
    let name = if cfg!(windows) { "solidcraft-cli.exe" } else { "solidcraft-cli" };
    if let Some(p) = solidcraft_engine::geom::env::var("CLI").map(std::path::PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join(name)))
        .filter(|p| p.is_file())
        .or_else(|| discover::find_in("solidcraft-cli", &discover::search_dirs()))
}

/// The agent's working folder: an empty folder of SolidCraft's own, so an agent's own file tools
/// (where it has them) see nothing of the user's.
fn working_dir() -> std::path::PathBuf {
    let d = solidcraft_engine::recovery::data_dir()
        .map(|d| d.join("agent-chat").join("work"))
        .unwrap_or_else(|| std::env::temp_dir().join("solidcraft-agent-chat"));
    #[cfg(not(target_arch = "wasm32"))]
    let _ = std::fs::create_dir_all(&d);
    d
}

/// Start the agent: the chat's control endpoint for its MCP bridge if needed, the process.
#[cfg(not(target_arch = "wasm32"))]
pub fn connect(app: &mut SolveApp, ctx: &egui::Context) {
    disconnect(app);
    let Some(entry) = agents::current(app) else {
        let chat = &mut app.agent_chat;
        chat.notice(panel::no_agents_message(), true);
        chat.state = Some(State::Failed);
        // The picker lists what to install (and, in the Flatpak, the permissions to grant).
        chat.picking = Some(true);
        return;
    };
    // An API agent's key comes from the keychain for this connection only.
    let key = if entry.kind.is_api() && entry.key {
        match app.services.secrets.as_ref().map(|s| s.get(&entry.id)) {
            Some(Ok(k)) => k,
            Some(Err(e)) => {
                app.agent_chat.notice(trf("ui.ai_keychain_error", &[("error", &e)]), true);
                app.agent_chat.state = Some(State::Failed);
                return;
            }
            None => None,
        }
    } else {
        None
    };
    let http = app.services.agent_http.clone();
    let chat = &mut app.agent_chat;
    if chat.tools_port.is_none()
        && let Some(start) = chat.control_starter.as_mut()
    {
        let (tx, rx) = std::sync::mpsc::channel();
        if let Some(port) = start(tx.clone()) {
            chat.tools_port = Some(port);
            chat.tools_rx = Some(rx);
            chat.tools_tx = Some(tx);
        }
    }
    let cli = find_cli();
    if chat.tools_port.is_none() {
        chat.notice(tr("ui.agent_chat_no_control"), true);
    } else if cli.is_none() {
        chat.notice(tr("ui.agent_chat_no_cli"), true);
    }
    // Found, but it failed when asked its version (#9034): say why and how to reinstall it before
    // the system's own message arrives. It is still started, in case it only lacks `--version`.
    if let Some(hint) = entry.problem_hint() {
        chat.notice(hint, true);
    }
    let bridge = cli.zip(chat.tools_port).map(|(cli, port)| acp::McpBridge { cli, port });
    chat.tools_available = Some(bridge.is_some());
    let wake = ctx.clone();
    let start = backend::StartContext { cwd: working_dir(), bridge, tools: chat.tools_tx.clone(), wake: Box::new(move || wake.request_repaint()) };
    let started: Result<Box<dyn ChatBackend>, String> = match entry.kind {
        agents::Kind::Acp if entry.argv.is_empty() => Err(tr("ui.agent_chat_none_found").to_string()),
        agents::Kind::Acp => acp_client::AcpBackend::start(&entry.argv, start).map(|b| Box::new(b) as Box<dyn ChatBackend>),
        kind @ (agents::Kind::Api | agents::Kind::Anthropic) => match http {
            Some(http) => {
                let protocol = if kind == agents::Kind::Anthropic { agents::Protocol::Anthropic } else { agents::Protocol::OpenAi };
                api_client::ApiBackend::start(
                    api_client::ApiConfig {
                        name: entry.name.clone(),
                        protocol,
                        base_url: entry.base_url.clone(),
                        model: entry.model.clone(),
                        key,
                        http,
                        request_timeout_s: entry.request_timeout_s,
                        reasoning_effort: entry.reasoning_effort.clone(),
                    },
                    start,
                )
                .map(|b| Box::new(b) as Box<dyn ChatBackend>)
            }
            None => Err(tr("ui.ai_no_http").to_string()),
        },
    };
    match started {
        Ok(b) => {
            app.preferences.agent_last = entry.id.clone();
            chat.backend = Some(b);
            chat.argv = entry.argv.clone();
            chat.running_id = entry.id.clone();
            chat.modes.clear();
            chat.mode.clear();
            chat.models.clear();
            chat.model.clear();
            chat.models_note = None;
            chat.model_revert = None;
            chat.running_endpoint = if entry.kind.is_api() { entry.base_url.clone() } else { String::new() };
            // An endpoint's models from the last time, until the new list arrives.
            if let Some(list) = chat.models_cache.get(&chat.running_endpoint) {
                chat.models = list.clone();
                chat.model = entry.model.clone();
            }
            chat.state = Some(State::Starting);
            chat.auth.clear();
            chat.auth_status = None;
            chat.agent.clear();
        }
        Err(e) => {
            chat.notice(trf("ui.agent_chat_failed_start", &[("error", &e)]), true);
            chat.state = Some(State::Failed);
        }
    }
}

/// The launch, once the window is up (the desktop host; not for runs driven over `--control`):
/// Settings → AI can open the panel and connect the agent started last (#7009).
pub fn begin(app: &mut SolveApp) {
    if !shown(app) {
        return;
    }
    if app.preferences.agent_chat_open_at_start {
        app.agent_chat.open = true;
    }
    app.agent_chat.connect_at_start = app.preferences.agent_connect_at_start;
}

/// Connect at start, once the installed agents are known (the one started last may be one of
/// them). A missing agent is not swapped for another: the chat says so and shows Connection
/// failed, whose Retry starts the agent the switcher shows. Nothing waits on it: discovery and
/// the agent run in the background, and an agent connected by hand meanwhile stays.
#[cfg(not(target_arch = "wasm32"))]
fn connect_at_start(app: &mut SolveApp, ctx: &egui::Context) {
    if !app.agent_chat.connect_at_start {
        return;
    }
    if app.agent_chat.detection.is_none() {
        refresh(app, ctx);
        return;
    }
    app.agent_chat.connect_at_start = false;
    if app.agent_chat.state.is_some() || !app.preferences.agent_connect_at_start {
        return;
    }
    let last = app.preferences.agent_last.clone();
    if !last.is_empty() {
        if !agents::entries(app).iter().any(|e| e.id == last) {
            app.agent_chat.notice(trf("ui.agent_chat_last_missing", &[("agent", &last)]), true);
            app.agent_chat.state = Some(State::Failed);
            return;
        }
        app.agent_chat.agent_id = Some(last);
    }
    connect(app, ctx);
}

/// Stop the agent (and everything it started); the conversation stays.
pub fn disconnect(app: &mut SolveApp) {
    crate::edit_in_place::forget_after_turn(app);
    app.agent_chat.disconnect_document(&mut app.session, false);
}

/// The app is quitting: end the agent and its helpers before the window goes.
pub fn shutdown(app: &mut SolveApp) {
    app.agent_chat.disconnect_document(&mut app.session, true);
    crate::documents::disconnect_background_chats(app, true);
}

/// Mark the origin only for this synchronous dispatch. Deferred work carries the same trusted
/// turn flag and enters here when it applies; JSON parameters cannot opt into continuation.
pub(crate) fn with_tool_request<T>(
    app: &mut SolveApp,
    active: Option<&Arc<std::sync::atomic::AtomicBool>>,
    run: impl FnOnce(&mut SolveApp) -> T,
) -> T {
    let own = active.is_some_and(|active| {
        active.load(std::sync::atomic::Ordering::Relaxed) && app.agent_chat.turn.as_ref().is_some_and(|turn| Arc::ptr_eq(active, &turn.active))
    });
    // A batch may replace the design and then roll back to its checkpoint. Keep the turn's
    // original undo boundary when that happens, including edits made before the batch.
    let before = app.agent_chat.turn.as_ref().filter(|_| own).map(|t| (app.session.doc.clone(), t.start_doc.clone(), t.start_undo));
    let previous = std::mem::replace(&mut app.agent_chat.following_document, own);
    let result = run(app);
    app.agent_chat.following_document = previous;
    if let Some((doc, start_doc, start_undo)) = before
        && Arc::ptr_eq(&doc, &app.session.doc)
        && let Some(turn) = app.agent_chat.turn.as_mut()
        && active.is_some_and(|active| Arc::ptr_eq(active, &turn.active))
    {
        turn.start_doc = start_doc;
        turn.start_undo = start_undo;
    }
    result
}

/// The agent's replacement succeeded. Keep its backend, transcript, queue, limits and validity
/// flag; subsequent edits undo in this design and picked geometry belongs to the old one.
pub(crate) fn continue_in_document(app: &mut SolveApp) {
    if let Some(turn) = app.agent_chat.turn.as_mut() {
        turn.start_doc = app.session.doc.clone();
        turn.start_undo = app.session.undo.len();
    }
    app.agent_chat.picked.clear();
    app.agent_chat.picked_context = Context::default();
    app.agent_chat.selected_count = None;
}

/// A newly opened tab has no conversation to displace. Move the initiating chat into it,
/// grouping the previous design's completed edits before its session is parked.
pub(crate) fn follow_new_tab(app: &mut SolveApp, switch: impl FnOnce(&mut SolveApp)) {
    if !app.agent_chat.following_document {
        switch(app);
        return;
    }
    if let Some(turn) = app.agent_chat.turn.as_ref() {
        let steps =
            merge_undo(&mut app.session.undo, &turn.start_doc, turn.start_undo, &trf("ui.agent_chat_undo_label", &[("message", &turn.label)]));
        if steps > 0 {
            app.agent_chat.push(Item::Changes { steps, doc: app.session.doc.clone(), undone: false });
        }
    }
    let mut chat = AgentChat::default();
    app.agent_chat.swap_document(&mut chat);
    switch(app);
    app.agent_chat.swap_document(&mut chat);
    continue_in_document(app);
}

/// A tab switch stops this turn before its session leaves the app. Keep the backend's own
/// conversation, drafts and queue, but do not send that queue automatically on returning.
pub(crate) fn leave_document(app: &mut SolveApp) {
    if app.agent_chat.state() == State::Busy {
        cancel(app);
    }
    end_turn(app);
    let chat = &mut app.agent_chat;
    chat.steering = false;
    chat.last = None;
    // A prompt waiting for startup or sign-in has not reached the backend yet. Keep it
    // with the explicit queue so a late Ready event cannot send it on returning to the tab.
    if let Some((text, image, at, files)) = chat.queued.take() {
        if let Some(at) = at.filter(|&at| matches!(chat.items.get(at), Some(Item::User { .. }))) {
            chat.items.remove(at);
        }
        // The text already carries the selection, so there is no context to show again.
        chat.pending.insert(0, Pending { text, image, context: Context::default(), files });
    }
    if !chat.pending.is_empty() {
        chat.stopped = true;
    }
}

/// Most messages waiting in the queue.
const MAX_PENDING: usize = 20;
/// Said to the agent before a message that interrupted it, so it knows its last step may be
/// incomplete.
const INTERRUPT_NOTE: &str = "[The user interrupted your previous step.]";

/// How many things the selection is, counting what was clicked (the two halves of a rim are one).
pub fn selected_count(app: &mut SolveApp) -> usize {
    let doc = Arc::as_ptr(&app.session.doc) as usize;
    if let Some((selection, at, count)) = &app.agent_chat.selected_count
        && *at == doc
        && *selection == app.session.selection
    {
        return *count;
    }
    let count = context::selection_count(&app.session, &app.session.selection);
    app.agent_chat.selected_count = Some((app.session.selection.clone(), doc, count));
    count
}

/// Open the picker for the next message's context (what is already picked stays in it). Nothing
/// happens while another tool or dialog is open.
pub fn start_pick(app: &mut SolveApp) {
    if app.dialog.is_none() {
        app.dialog = Some(crate::dialogs::Dialog::chat_context(&app.agent_chat.picked));
    }
}

/// The picker's OK: what its inputs hold is the next message's context.
pub(crate) fn pick_done(app: &mut SolveApp, d: &crate::dialogs::Dialog) -> Result<(), String> {
    let mut picked: Vec<solidcraft_engine::Sel> = Vec::new();
    for sel in d.inputs.iter().flat_map(|i| i.items.iter()) {
        if !picked.contains(sel) {
            picked.push(sel.clone());
        }
    }
    app.agent_chat.picked_context = context::describe(&mut app.session, &picked);
    // The tray lists what was described; more than the limit is only counted.
    picked.truncate(context::MAX_ITEMS);
    app.agent_chat.picked = picked;
    Ok(())
}

/// Take one picked item (by its place in the tray) out of what the next message carries.
pub fn remove_picked(app: &mut SolveApp, at: usize) {
    let chat = &mut app.agent_chat;
    if at < chat.picked_context.items.len() {
        chat.picked_context.items.remove(at);
        if at < chat.picked.len() {
            chat.picked.remove(at);
        }
    }
}

/// Send what the user typed: now if the agent is ready, queued behind the running message if it
/// is working, else once it is ready (starting it if needed).
pub fn send(app: &mut SolveApp, ctx: &egui::Context, text: String) {
    // A message waits for its files: the words go back in the box until they are read.
    if !app.agent_chat.loading.is_empty() {
        if app.agent_chat.input.trim().is_empty() {
            app.agent_chat.input = text;
        }
        app.agent_chat.notice(tr("ui.agent_chat_files_loading"), false);
        return;
    }
    let text: String = text.chars().take(MAX_PROMPT).collect();
    // Files alone make a message.
    if text.trim().is_empty() && app.agent_chat.attachments.is_empty() {
        return;
    }
    let text = if text.trim().is_empty() { String::new() } else { text };
    let image = app.agent_chat.attach_view && app.agent_chat.images;
    // The selection is read now, when the user sends, not when a queued message finally goes.
    // What was picked goes first, then what is selected (without what was picked twice).
    let mut context = std::mem::take(&mut app.agent_chat.picked_context);
    app.agent_chat.picked.clear();
    if app.agent_chat.attach_selection {
        let mut selected = context::describe_selection(&mut app.session);
        selected.items.retain(|i| !context.items.contains(i));
        context.merge(selected);
    }
    if app.agent_chat.state() == State::Busy {
        let chat = &mut app.agent_chat;
        if chat.pending.len() >= MAX_PENDING {
            // The files stay on the chips for when there is room.
            chat.notice(tr("ui.agent_chat_queue_full"), false);
        } else {
            let files = std::mem::take(&mut chat.attachments);
            chat.pending.push(Pending { text, image, context, files });
        }
        return;
    }
    let files = std::mem::take(&mut app.agent_chat.attachments);
    // Messages left waiting by Stop go first, in one prompt with this one.
    if !app.agent_chat.pending.is_empty() {
        app.agent_chat.pending.push(Pending { text, image, context, files });
        send_queue(app, ctx, false);
        return;
    }
    submit(app, ctx, text, image, context, files, false);
}

/// Put `text` in the conversation and send it with its files (`interrupted`: the agent is told it
/// cut in on its previous step).
fn submit(app: &mut SolveApp, ctx: &egui::Context, text: String, image: bool, context: Context, files: Vec<attach::Attachment>, interrupted: bool) {
    // Attached pictures show as thumbnails with the viewer, as the view's picture does; the other
    // files are listed by name.
    let pictures = files
        .iter()
        .filter_map(|f| match &f.content {
            attach::Content::Picture { bytes, .. } => Some(Picture::shared(Arc::clone(bytes), f.name.clone(), false)),
            _ => None,
        })
        .collect();
    app.agent_chat.push(Item::User {
        text: text.clone(),
        image,
        pictures,
        context: context.clone(),
        files: files.iter().filter(|f| !f.is_picture()).map(|f| f.name.clone()).collect(),
    });
    // The message's place in the conversation: its picture is kept there when it is taken.
    let at = app.agent_chat.items.len().checked_sub(1);
    // The agent gets the selection as text after the message; the conversation shows chips.
    let text = context::with_context(&text, &context);
    match app.agent_chat.state() {
        State::Ready => start_turn_with(app, text, image, at, files, interrupted.then_some(INTERRUPT_NOTE)),
        s => {
            app.agent_chat.queued = Some((text, image, at, files));
            #[cfg(not(target_arch = "wasm32"))]
            if !s.running() {
                connect(app, ctx);
            }
            #[cfg(target_arch = "wasm32")]
            let _ = (s, ctx);
        }
    }
}

/// Send the queued messages as one prompt, in order.
fn send_queue(app: &mut SolveApp, ctx: &egui::Context, interrupted: bool) {
    let chat = &mut app.agent_chat;
    chat.pending_edit = None;
    if chat.pending.is_empty() {
        return;
    }
    let all = std::mem::take(&mut chat.pending);
    let image = all.iter().any(|p| p.image);
    let mut files = Vec::new();
    let mut context = Context::default();
    let mut texts = Vec::new();
    for p in all {
        files.extend(p.files);
        context.merge(p.context);
        if !p.text.trim().is_empty() {
            texts.push(p.text);
        }
    }
    // One message carries at most MAX_FILES files; the rest are named, not sent.
    if files.len() > attach::MAX_FILES {
        let left: Vec<String> = files.split_off(attach::MAX_FILES).into_iter().map(|f| f.name).collect();
        let max = attach::MAX_FILES.to_string();
        chat.notice(trf("ui.agent_chat_files_left_out", &[("max", &max), ("files", &left.join(", "))]), true);
    }
    let text = texts.join("\n\n");
    let text: String = text.chars().take(MAX_PROMPT).collect();
    submit(app, ctx, text, image, context, files, interrupted);
}

/// Send now: stop the running step and send the queue as the next prompt once the agent has said
/// it stopped (its note tells the agent it was interrupted). With nothing running, sends the queue.
pub fn send_now(app: &mut SolveApp, ctx: &egui::Context) {
    if app.agent_chat.pending.is_empty() {
        return;
    }
    if app.agent_chat.state() == State::Busy {
        app.agent_chat.steering = true;
        app.agent_chat.stopped = false;
        interrupt(app);
    } else {
        send_queue(app, ctx, false);
    }
}

/// New chat: stop what runs (as Stop does), clear the conversation and the queue, and start a
/// fresh session on the same agent (no restart); the agent and its mode stay, the design's undo
/// history is untouched.
pub fn new_chat(app: &mut SolveApp) {
    let running = app.agent_chat.state() == State::Busy;
    if running {
        cancel(app);
    }
    decline_all(app, "the chat was cleared");
    end_turn(app);
    let chat = &mut app.agent_chat;
    chat.confirm_new = false;
    chat.items.clear();
    chat.turn_warning = None;
    chat.gallery = None;
    chat.picked.clear();
    chat.picked_context = Context::default();
    chat.pending.clear();
    chat.attachments.clear();
    chat.loading.clear();
    chat.pending_edit = None;
    chat.queued = None;
    chat.last = None;
    chat.steering = false;
    chat.stopped = false;
    chat.ended = None;
    chat.discard_end = running;
    chat.model_revert = None;
    if matches!(chat.state(), State::Ready | State::Busy) {
        chat.restore_mode = Some(chat.mode.clone()).filter(|m| !m.is_empty());
        chat.state = Some(State::Starting);
        if let Some(b) = chat.backend.as_mut() {
            b.new_session();
        }
    }
    chat.clear_config();
}

/// Whether New chat would throw something away that the user may want (so it asks first).
pub fn new_chat_needs_confirm(app: &SolveApp) -> bool {
    app.agent_chat.state() == State::Busy || !app.agent_chat.pending.is_empty() || app.agent_chat.files() > 0
}

/// Take a queued message out of the queue.
pub fn remove_pending(app: &mut SolveApp, index: usize) {
    let chat = &mut app.agent_chat;
    if index < chat.pending.len() {
        chat.pending.remove(index);
    }
    chat.pending_edit = None;
}

/// The turn ended: the queue goes on its own after a normal end, and after a Send now; not after
/// Stop, a sign-in request or an error.
fn after_turn(app: &mut SolveApp, ctx: &egui::Context) {
    let reason = app.agent_chat.ended.take().unwrap_or_default();
    let steering = std::mem::take(&mut app.agent_chat.steering);
    if app.agent_chat.state() != State::Ready || app.agent_chat.pending.is_empty() {
        return;
    }
    if steering && !matches!(reason.as_str(), "auth" | "error") {
        send_queue(app, ctx, reason == "cancelled");
    } else if !app.agent_chat.stopped && reason.is_empty() {
        send_queue(app, ctx, false);
    }
}

fn start_turn(app: &mut SolveApp, text: String, image: bool, at: Option<usize>, files: Vec<attach::Attachment>) {
    start_turn_with(app, text, image, at, files, None);
}

/// Start a message; `note` goes before the text for the agent, not into the conversation. The
/// files are named after the text (text files in full), and pictures go as images to an agent
/// that takes them. `at` is the message's place in the conversation, where its picture is kept.
fn start_turn_with(app: &mut SolveApp, text: String, image: bool, at: Option<usize>, files: Vec<attach::Attachment>, note: Option<&str>) {
    let (pictures, documents) = (app.agent_chat.images, app.agent_chat.documents);
    let lead = match note {
        Some(n) => format!("{n}\n\n{text}"),
        None => text.clone(),
    };
    // The whole message stays within the cap: the files' text gets what the words leave.
    let budget = MAX_PROMPT.saturating_sub(lead.chars().count() + FILE_NAMES_ROOM);
    let about_files = attach::note(&files, pictures, documents, budget);
    let sent: String = format!("{lead}{about_files}").chars().take(MAX_PROMPT).collect();
    let mut parts = vec![PromptPart::Text(sent)];
    if image {
        match crate::control::view_png(app, VIEW_IMAGE.0, VIEW_IMAGE.1) {
            Ok(png) => {
                parts.push(PromptPart::Image { mime: "image/png".into(), base64: backend::base64(&png) });
                app.agent_chat.keep_picture(at, png);
            }
            Err(e) => log::warn!("the view's picture is left out of the prompt: {e}"),
        }
    }
    parts.extend(attach::parts(&files, pictures, documents));
    // The undo step is named after the message, or its first file when it has no words.
    let first_line = text.lines().find(|l| !l.trim().is_empty()).or_else(|| files.first().map(|f| f.name.as_str())).unwrap_or("");
    let label: String = first_line.chars().take(48).collect();
    app.agent_chat.turn_warning = None;
    app.agent_chat.turn = Some(Turn {
        active: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        start_doc: app.session.doc.clone(),
        start_undo: app.session.undo.len(),
        changes: 0,
        requests: 0,
        limit_told: false,
        label,
        tools_available: app.agent_chat.tools_available.unwrap_or(app.agent_chat.tools_port.is_some()),
        tool_called: false,
        replied: false,
        edit_requested: requests_edit(&text),
    });
    app.agent_chat.last = Some((text, image, at, files));
    app.agent_chat.state = Some(State::Busy);
    app.agent_chat.stopped = false;
    app.agent_chat.steering = false;
    app.agent_chat.ended = None;
    if let Some(b) = app.agent_chat.backend.as_mut() {
        b.prompt(&parts);
    }
}

/// Add a file to the next message. A picture or text file is read and kept (see [`attach`]); a CAD
/// file is imported into the design at once, as its own component, and the message says so.
pub fn attach_path(app: &mut SolveApp, path: &str) -> std::result::Result<String, String> {
    if app.agent_chat.files() >= attach::MAX_FILES {
        return Err(trf("ui.agent_chat_files_max", &[("max", &attach::MAX_FILES.to_string())]));
    }
    let p = std::path::Path::new(path);
    let file = if attach::kind(p) == attach::Kind::Cad {
        if !p.is_file() {
            return Err(trf("ui.agent_chat_file_not_a_file", &[("name", path)]));
        }
        // Imported now, as a dropped file is; it joins the message once the import is in, so the
        // agent is told it was added only when it was. A full queue is refused at once.
        let name = attach::name_of(p);
        let (tx, rx) = std::sync::mpsc::channel();
        let mut request = crate::file_load::Request::new(path, crate::file_load::Kind::Insert);
        request.reply = Some(tx);
        crate::file_load::begin_request(app, request).map_err(|e| trf("ui.agent_chat_import_failed", &[("name", &name), ("error", &e)]))?;
        return Ok(app.agent_chat.importing(name, rx));
    } else {
        // A picture is decoded and a PDF read off the window thread: a large one takes long enough
        // to stop it.
        #[cfg(not(target_arch = "wasm32"))]
        if matches!(attach::kind(p), attach::Kind::Picture | attach::Kind::Pdf) {
            // A missing file is said at once; reading and decoding wait for the worker.
            if !p.is_file() {
                return Err(trf("ui.agent_chat_file_not_a_file", &[("name", &attach::name_of(p))]));
            }
            let path = p.to_path_buf();
            return app.agent_chat.load(attach::name_of(p), move || attach::read(&path));
        }
        attach::read(p)?
    };
    Ok(app.agent_chat.add_file(file))
}

/// Add a picture from the clipboard (RGBA rows, as the system gives them) to the next message.
pub fn attach_rgba(app: &mut SolveApp, width: usize, height: usize, rgba: &[u8]) -> std::result::Result<String, String> {
    if app.agent_chat.files() >= attach::MAX_FILES {
        return Err(trf("ui.agent_chat_files_max", &[("max", &attach::MAX_FILES.to_string())]));
    }
    let n = app.agent_chat.pasted + 1;
    let name = format!("pasted picture {n}.png");
    app.agent_chat.pasted = n;
    // Encoded off the window thread, as a decoded picture is.
    #[cfg(not(target_arch = "wasm32"))]
    {
        let rgba = rgba.to_vec();
        let named = name.clone();
        app.agent_chat.load(name, move || attach::from_rgba(named, width, height, &rgba))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let file = attach::from_rgba(name, width, height, rgba)?;
        Ok(app.agent_chat.add_file(file))
    }
}

/// Attach the picture on the clipboard, if it holds one.
#[cfg(not(target_arch = "wasm32"))]
pub fn paste_picture(app: &mut SolveApp) -> std::result::Result<String, String> {
    let image = arboard::Clipboard::new().and_then(|mut c| c.get_image()).map_err(|_| tr("ui.agent_chat_no_picture").to_string())?;
    attach_rgba(app, image.width, image.height, &image.bytes)
}

/// Files dropped on the chat attach to the next message; called before the app handles drops, so
/// those taken aren't opened too. A drop is the chat's when the pointer was last seen over it:
/// the platform gives no position during a drag (winit 0.30), so that is the best the chat can
/// know; the + button always works.
pub fn take_drops(app: &mut SolveApp, ctx: &egui::Context) {
    if !drop_is_ours(app, ctx) || !ctx.input(|i| !i.raw.dropped_files.is_empty()) {
        return;
    }
    let files = ctx.input_mut(|i| std::mem::take(&mut i.raw.dropped_files));
    attach_dropped(app, files.iter().map(|f| f.path().to_path_buf()));
}

/// Whether files dropped now would be the chat's: the pointer was last seen over it.
pub(crate) fn drop_is_ours(app: &SolveApp, ctx: &egui::Context) -> bool {
    let Some(rect) = app.agent_chat.drop_rect else { return false };
    ctx.input(|i| i.pointer.latest_pos()).is_some_and(|p| rect.contains(p))
}

/// Attach dropped files; a refused one says why in the conversation.
pub(crate) fn attach_dropped(app: &mut SolveApp, paths: impl Iterator<Item = std::path::PathBuf>) {
    for p in paths {
        if p.as_os_str().is_empty() {
            continue;
        }
        if let Err(e) = attach_path(app, &p.to_string_lossy()) {
            app.agent_chat.notice(e, true);
        }
    }
}

/// Stop: ask the agent to stop the message it is on; requests waiting for approval are declined.
/// Messages queued meanwhile stay queued (they wait for Send).
pub fn cancel(app: &mut SolveApp) {
    app.agent_chat.steering = false;
    app.agent_chat.stopped = true;
    crate::edit_in_place::forget_after_turn(app);
    interrupt(app);
}

/// Ask the agent to stop the message it is on and decline what waits for approval.
fn interrupt(app: &mut SolveApp) {
    decline_all(app, "the user stopped the message");
    if let Some(b) = app.agent_chat.backend.as_mut() {
        b.cancel();
    }
    for i in &mut app.agent_chat.items {
        if let Item::Permission { answer: a @ None, .. } = i {
            *a = Some(tr("ui.agent_chat_cancel").to_string());
        }
    }
}

fn decline_all(app: &mut SolveApp, why: &str) {
    let ids: Vec<u64> = app.agent_chat.held.iter().map(|h| h.id).collect();
    for id in ids {
        approve_with(app, id, false, why);
    }
}

/// Allow or deny a held request.
pub fn approve(app: &mut SolveApp, id: u64, allow: bool) {
    approve_with(app, id, allow, "the user declined it");
}

/// Allow a held request and remember to allow its tool for this agent from now on.
pub fn approve_always(app: &mut SolveApp, id: u64) {
    let tool = app.agent_chat.items.iter().find_map(|i| match i {
        Item::Approval { id: x, tool, .. } if *x == id => Some(tool.clone()),
        _ => None,
    });
    if let Some(tool) = tool {
        let agent = app.agent_chat.running_id.clone();
        agents::remember(&mut app.preferences, &agent, &tool);
    }
    approve(app, id, true);
}

/// Answer the agent's own permission question at `index` with one of its options (`None`:
/// cancelled); an "always allow" answer is remembered for this agent and tool.
pub fn answer_permission(app: &mut SolveApp, index: usize, option: Option<&str>) {
    if let (Some(o), Some(Item::Permission { title, options, answer: None, .. })) = (option, app.agent_chat.items.get(index))
        && options.iter().any(|x| x.id == o && x.kind == "allow_always")
    {
        let (agent, title) = (app.agent_chat.running_id.clone(), title.clone());
        agents::remember(&mut app.preferences, &agent, &title);
    }
    app.agent_chat.answer(index, option);
}

/// The agent asks before running something of its own. SolidCraft's own tools are checked by
/// SolidCraft's gate, so they're allowed here (no double question); otherwise the mode or an
/// earlier "always allow" may answer, and the chat says so; otherwise the user is asked.
fn on_permission(app: &mut SolveApp, key: u64, title: String, options: Vec<PermissionOption>) {
    let agent = app.agent_chat.running_id.clone();
    let mode = agents::mode_for(&app.preferences, &agent);
    let remembered = agents::always_allowed(&app.preferences, &agent, &title);
    let why = if solidcraft_tool(&title) {
        Some("ui.ai_auto_ours")
    } else if remembered {
        Some("ui.ai_auto_remembered")
    } else if mode == agents::Mode::All {
        Some("ui.ai_auto_mode")
    } else {
        None
    };
    let once = options.iter().find(|o| o.kind == "allow_once");
    let always = options.iter().find(|o| o.kind == "allow_always");
    let pick = if remembered { always.or(once) } else { once.or(always) }.map(|o| o.id.clone());
    match (why, pick) {
        (Some(why), Some(option)) => {
            if let Some(b) = app.agent_chat.backend.as_mut() {
                b.answer_permission(key, Some(&option));
            }
            app.agent_chat.push(Item::Notice { text: trf("ui.ai_auto_allowed", &[("title", &title), ("why", tr(why))]), error: false });
        }
        _ => app.agent_chat.push(Item::Permission { key, title, options, answer: None }),
    }
}

fn approve_with(app: &mut SolveApp, id: u64, allow: bool, why: &str) {
    let chat = &mut app.agent_chat;
    let Some(i) = chat.held.iter().position(|h| h.id == id) else { return };
    let h = chat.held.remove(i);
    for item in &mut chat.items {
        if let Item::Approval { id: x, answer, .. } = item
            && *x == id
        {
            *answer = Some(allow);
        }
    }
    if allow {
        app.control_held.push_back(h.req);
    } else {
        let _ = h.req.reply.send(json!({"ok": false, "error": format!("not run: {why}")}));
    }
}

/// Recognise only explicit English edit requests; informational questions stay quiet.
/// This checks the user's opening words, never interprets the model's prose as a call.
fn requests_edit(text: &str) -> bool {
    let opening: String = text.trim().chars().take(160).collect::<String>().to_ascii_lowercase();
    let mut words = opening.as_str();
    for prefix in ["please ", "can you ", "could you ", "would you ", "please "] {
        if let Some(rest) = words.strip_prefix(prefix) {
            words = rest;
        }
    }
    [
        "add", "create", "make", "change", "rename", "move", "rotate", "delete", "remove", "sketch", "extrude", "round", "fillet", "chamfer",
        "scale", "cut", "drill",
    ]
    .iter()
    .any(|verb| words.strip_prefix(*verb).is_some_and(|rest| rest.starts_with(char::is_whitespace)))
}

/// The message is answered: its changes become one undo step, and what still waits is declined.
fn end_turn(app: &mut SolveApp) {
    decline_all(app, "the message ended");
    finish_turn(&mut app.agent_chat, &mut app.session);
}

fn finish_turn(chat: &mut AgentChat, session: &mut solidcraft_engine::Session) {
    let Some(turn) = chat.turn.take() else { return };
    turn.active.store(false, std::sync::atomic::Ordering::Relaxed);
    let steps = merge_undo(&mut session.undo, &turn.start_doc, turn.start_undo, &trf("ui.agent_chat_undo_label", &[("message", &turn.label)]));
    if chat.ended.as_deref() == Some("")
        && !chat.stopped
        && turn.tools_available
        && !turn.tool_called
        && turn.requests == 0
        && turn.changes == 0
        && steps == 0
        && Arc::ptr_eq(&session.doc, &turn.start_doc)
        && (!turn.replied || turn.edit_requested)
    {
        let warning = tr("ui.agent_chat_no_tool_call").to_string();
        chat.turn_warning = Some(warning.clone());
        chat.notice(warning, false);
    }
    if steps > 0 {
        let doc = session.doc.clone();
        chat.push(Item::Changes { steps, doc, undone: false });
    }
}

/// Merge the undo steps made since `start_doc` (at depth `start_undo`) into one, labelled `label`;
/// how many steps it took in (0: none were made, or the history no longer reaches that far).
fn merge_undo(
    undo: &mut Vec<solidcraft_engine::Snapshot>,
    start_doc: &Arc<solidcraft_engine::doc::Document>,
    start_undo: usize,
    label: &str,
) -> usize {
    // The first change of the turn saved the design as it was when the turn began. When the
    // history is full its oldest steps go, so look for it at or below where it was put.
    let Some(first) = (0..undo.len().min(start_undo + 1)).rev().find(|&i| undo.get(i).is_some_and(|s| Arc::ptr_eq(&s.doc, start_doc))) else {
        return 0;
    };
    let steps = undo.len() - first;
    undo.truncate(first + 1);
    if let Some(s) = undo.get_mut(first) {
        s.label = label.to_string();
    }
    steps
}

/// Undo a message's changes from the conversation (only while they are the last thing done).
pub fn undo_changes(app: &mut SolveApp, index: usize) {
    let Some(Item::Changes { doc, undone, .. }) = app.agent_chat.items.get(index) else { return };
    if *undone || !Arc::ptr_eq(doc, &app.session.doc) {
        return;
    }
    if app.run("edit.undo", json!({})).is_ok()
        && let Some(Item::Changes { undone, .. }) = app.agent_chat.items.get_mut(index)
    {
        *undone = true;
    }
}

/// The requests the agent's tools sent through the chat's endpoint: each one passes the gate.
fn pump_tools(app: &mut SolveApp) {
    let Some(rx) = app.agent_chat.tools_rx.take() else { return };
    let configured = app.preferences.agent_chat_turn_limit;
    let limit = if configured == 0 { u32::MAX } else { configured.clamp(1, 1000) };
    for mut req in rx.try_iter().take(64) {
        let refuse = |req: ControlRequest, e: String| {
            let _ = req.reply.send(json!({"ok": false, "error": e}));
        };
        let undo_len = app.session.undo.len();
        let Some(turn) = app.agent_chat.turn.as_mut() else {
            refuse(req, "the agent chat has no message in progress".into());
            continue;
        };
        req.chat_turn = Some(Arc::clone(&turn.active));
        turn.requests += 1;
        // Requests without folders of their own stay inside the chat's working folder.
        if let Some(o) = req.params.as_object_mut()
            && !o.contains_key("allow_paths")
        {
            o.insert("allow_paths".into(), json!([working_dir().to_string_lossy()]));
        }
        let undo_left = undo_len.saturating_sub(turn.start_undo);
        // The permission mode (the agent's own, else the global one) and "always allow" answers.
        let mode = agents::mode_for(&app.preferences, &app.agent_chat.running_id);
        let tool = gate::tool_key(&req.method, &req.params);
        let always = agents::always_allowed(&app.preferences, &app.agent_chat.running_id, &tool);
        let verdict = match gate::request(&req.method, &req.params, undo_left) {
            gate::Verdict::Ask(_) if mode == agents::Mode::All || always => gate::Verdict::Run { change: true },
            gate::Verdict::Run { change: true } if mode == agents::Mode::Ask && !always => {
                gate::Verdict::Ask(gate::describe(&req.method, &req.params))
            }
            v => v,
        };
        match verdict {
            gate::Verdict::Run { change } => {
                if change {
                    if turn.changes >= limit {
                        let first = !turn.limit_told;
                        turn.limit_told = true;
                        refuse(req, format!("the limit of {limit} changes for one message is reached: tell the user what is left to do"));
                        if first {
                            app.agent_chat.notice(trf("ui.agent_chat_limit", &[("limit", &limit.to_string())]), false);
                        }
                        continue;
                    }
                    turn.changes += 1;
                }
                app.control_held.push_back(req);
            }
            gate::Verdict::Ask(title) => {
                let chat = &mut app.agent_chat;
                chat.next_approval += 1;
                let id = chat.next_approval;
                chat.push(Item::Approval { id, title, tool, answer: None });
                chat.held.push(Held { id, req, since: crate::now_ms() });
            }
            gate::Verdict::Refuse(e) => {
                app.agent_chat.notice(trf("ui.agent_chat_refused", &[("reason", &e)]), false);
                refuse(req, e);
            }
        }
    }
    app.agent_chat.tools_rx = Some(rx);
    // Questions nobody answered: the agent stopped waiting long ago.
    let now = crate::now_ms();
    let stale: Vec<u64> = app.agent_chat.held.iter().filter(|h| now - h.since > APPROVAL_TIMEOUT_MS).map(|h| h.id).collect();
    for id in stale {
        approve_with(app, id, false, "nobody answered in time");
    }
}

/// Read what the agent sent since the last frame, its tool requests, and the agent search.
/// Files read off the window thread join the message as they are done, where their ids put them;
/// a refused one says why.
pub(crate) fn take_loaded(app: &mut SolveApp, ctx: &egui::Context) {
    use std::sync::mpsc::TryRecvError;
    let chat = &mut app.agent_chat;
    let mut i = 0;
    while i < chat.loading.len() {
        let name = chat.loading[i].name.clone();
        let done = match &chat.loading[i].what {
            Reading::File(rx) => match rx.try_recv() {
                Ok(read) => Some(read),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(Err(trf("ui.agent_chat_file_error", &[("name", &name), ("error", "stopped")]))),
            },
            Reading::Import(rx) => match rx.try_recv() {
                Ok(answer) if answer["ok"].as_bool() == Some(true) => Some(Ok(attach::Attachment::imported(name))),
                Ok(answer) => {
                    let why = answer["error"].as_str().unwrap_or_default().to_string();
                    Some(Err(trf("ui.agent_chat_import_failed", &[("name", &name), ("error", &why)])))
                }
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(Err(trf("ui.agent_chat_import_failed", &[("name", &name), ("error", "stopped")]))),
            },
        };
        match done {
            None => i += 1,
            Some(read) => {
                let l = chat.loading.remove(i);
                match read {
                    Ok(file) => {
                        chat.place(l.id, file);
                    }
                    Err(e) => chat.notice(e, true),
                }
            }
        }
    }
    if !chat.loading.is_empty() {
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }
}

pub fn poll(app: &mut SolveApp, ctx: &egui::Context) {
    settings::poll_tests(&mut app.agent_chat);
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(rx) = &app.agent_chat.detecting
        && let Ok(d) = rx.try_recv()
    {
        for found in &d.found {
            app.agent_chat.login_results.remove(found.id);
            app.agent_chat.settings.tests.remove(found.id);
        }
        app.agent_chat.detection = Some(d);
        app.agent_chat.detecting = None;
    }
    // AI features turned off: nothing keeps running, nothing is looked for.
    if !shown(app) {
        // Turned back on later, AI features don't connect as if SolidCraft were starting.
        app.agent_chat.connect_at_start = false;
        crate::documents::disconnect_background_chats(app, false);
        if app.agent_chat.backend.is_some() || app.agent_chat.open {
            disconnect(app);
            app.agent_chat.open = false;
        }
        return;
    }
    take_loaded(app, ctx);
    let events = app.agent_chat.backend.as_mut().map(|b| b.poll()).unwrap_or_default();
    for e in events {
        let ready = matches!(e, Event::Ready { .. });
        let configured = matches!(e, Event::ConfigOptionFinished { .. });
        let confirmed_model = match &e {
            Event::ConfigOptionFinished { id, result: Ok(options) } => {
                options.iter().find(|o| o.category == "model").filter(|o| o.id == *id).map(|o| o.current_value.clone())
            }
            _ => None,
        };
        let refused = if matches!(e, Event::ModelRejected(_)) { app.agent_chat.model_revert.as_ref().map(|r| r.saved.clone()) } else { None };
        match app.agent_chat.on_event(e) {
            Some(Outcome::TurnEnd) => {
                end_turn(app);
                after_turn(app, ctx);
            }
            Some(Outcome::Permission { key, .. }) if app.agent_chat.turn.is_none() => {
                if let Some(b) = app.agent_chat.backend.as_mut() {
                    b.answer_permission(key, None);
                }
            }
            Some(Outcome::Permission { key, title, options }) => on_permission(app, key, title, options),
            None => {}
        }
        if let Some(model) = confirmed_model {
            app.preferences.agent_models.insert(app.agent_chat.running_id.clone(), model.clone());
            if let Some(a) = app.preferences.agents.iter_mut().find(|a| a.id == app.agent_chat.running_id && a.kind.is_api()) {
                a.model = model;
            }
        }
        match refused {
            Some(Some(saved)) => {
                app.preferences.agent_models.insert(app.agent_chat.running_id.clone(), saved);
            }
            Some(None) => {
                app.preferences.agent_models.remove(&app.agent_chat.running_id);
            }
            None => {}
        }
        if ready {
            apply_saved_model(app);
        }
        if (ready || configured) && !app.agent_chat.config_pending {
            if app.agent_chat.config_options.is_some()
                && let Some(mode) = app.agent_chat.restore_mode.take()
                && mode != app.agent_chat.mode
            {
                set_mode(app, &mode);
            }
            if !app.agent_chat.config_pending
                && let Some((text, image, at, files)) = app.agent_chat.queued.take()
            {
                start_turn(app, text, image, at, files);
            }
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        settings::poll_capabilities(app, ctx);
        settings::poll_listing(app, ctx);
        connect_at_start(app, ctx);
    }
    pump_tools(app);
    if !app.agent_chat.held.is_empty() || app.agent_chat.state() == State::Busy {
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
}

/// Look for installed agents in the background (cached; Refresh looks again).
#[cfg(not(target_arch = "wasm32"))]
pub fn refresh(app: &mut SolveApp, ctx: &egui::Context) {
    if app.agent_chat.detecting.is_some() {
        return;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let _ = tx.send(discover::detect());
        ctx.request_repaint();
    });
    app.agent_chat.detecting = Some(rx);
}

/// Choose the model for this chat (the header's model picker): the agent switches to it, and it is
/// remembered for this agent; an API endpoint's agent also keeps it as its default model.
pub fn set_model(app: &mut SolveApp, model: &str) {
    if let Some(options) = app.agent_chat.config_options.as_ref() {
        let Some(option) = options.iter().find(|x| x.category == "model") else {
            return;
        };
        let id = option.id.clone();
        set_config_option(app, &id, model);
        return;
    }
    if app.agent_chat.models.iter().any(|m| m.id == model) {
        send_model(app, model);
    }
}

/// Whether a model the agent did not list can be sent: an endpoint, an ACP agent with a model
/// option, or a legacy ACP agent that listed models.
pub fn custom_models_accepted(app: &SolveApp) -> bool {
    let chat = &app.agent_chat;
    if chat.backend.is_none() || !chat.state().running() {
        return false;
    }
    match chat.config_options.as_ref() {
        Some(options) => options.iter().any(|o| o.category == "model"),
        None => !chat.running_endpoint.is_empty() || !chat.models.is_empty(),
    }
}

/// A typed model (the pickers' custom row). A listed id goes the usual way, others by the agent's
/// kind; an agent that refuses it keeps its model and the chat says so. Returns whether it was sent.
pub fn set_custom_model(app: &mut SolveApp, text: &str) -> bool {
    let model = text.trim();
    if !model_id_valid(model) || !custom_models_accepted(app) {
        return false;
    }
    let option = app.agent_chat.config_options.as_ref().and_then(|options| options.iter().find(|o| o.category == "model"));
    if let Some(id) = option.map(|o| o.id.clone()) {
        let sent = set_config_option_checked(app, &id, model, false);
        if sent {
            app.agent_chat.typed_model = Some(model.to_string());
        }
        return sent;
    }
    send_model(app, model);
    true
}

/// Switch a legacy ACP agent or an endpoint to `model`: it shows as current at once and is
/// remembered for this agent; an endpoint also keeps it as its default model.
fn send_model(app: &mut SolveApp, model: &str) {
    let agent = app.agent_chat.running_id.clone();
    let chat = &mut app.agent_chat;
    let unlisted = !chat.models.iter().any(|m| m.id == model);
    let entry = || acp::ModelInfo { id: model.to_string(), name: model.to_string(), description: String::new() };
    if unlisted {
        chat.models.insert(0, entry());
        chat.models.truncate(MODEL_LIST_CAP);
    }
    if chat.running_endpoint.is_empty() {
        chat.model_revert = Some(ModelRevert { model: chat.model.clone(), unlisted, saved: app.preferences.agent_models.get(&agent).cloned() });
    } else if unlisted {
        let cached = chat.models_cache.entry(chat.running_endpoint.clone()).or_default();
        cached.insert(0, entry());
        cached.truncate(MODEL_LIST_CAP);
    }
    if let Some(b) = chat.backend.as_mut() {
        b.set_model(model);
    }
    chat.model = model.to_string();
    chat.model_provider = None;
    if let Some(a) = app.preferences.agents.iter_mut().find(|a| a.id == agent && a.kind.is_api()) {
        a.model = model.to_string();
    }
    app.preferences.agent_models.insert(agent, model.to_string());
}

/// A new session is ready: put back the model last chosen for this agent, when it offers it.
fn apply_saved_model(app: &mut SolveApp) {
    let chat = &app.agent_chat;
    let Some(saved) = app.preferences.agent_models.get(&chat.running_id).cloned() else { return };
    if saved != chat.model && chat.models.iter().any(|m| m.id == saved) {
        set_model(app, &saved);
    }
}

/// Whether the model picker shows: only for an agent that reports models.
pub fn models_shown(app: &SolveApp) -> bool {
    !app.agent_chat.models.is_empty() && app.agent_chat.state().running()
}

/// Longest provider name kept from an agent's model list.
const PROVIDER_NAME_CAP: usize = 128;
/// Most models kept for a picker, whether listed or typed.
const MODEL_LIST_CAP: usize = 2000;
/// Longest model id kept from a listing or sent to an agent.
pub const MODEL_ID_CAP: usize = 256;
/// Longest note about a listing that failed or a model the agent refused.
const MODELS_NOTE_CAP: usize = 1024;

/// Whether a model id can go to an agent: not empty, not too long, no control characters.
fn model_id_valid(id: &str) -> bool {
    !id.is_empty() && id.chars().count() <= MODEL_ID_CAP && !id.chars().any(char::is_control)
}

/// A model's provider and its name at that provider, when the agent says which provider serves it:
/// a name `Provider · model` (Hermes, OpenCode) or a description `Provider: …` (up to a ` • `).
/// `None`: the agent names no provider.
pub fn model_provider(m: &acp::ModelInfo) -> Option<(String, String)> {
    let cap = |s: &str| s.trim().chars().take(PROVIDER_NAME_CAP).collect::<String>();
    if let Some((provider, name)) = m.name.split_once(" · ")
        && !provider.trim().is_empty()
        && !name.trim().is_empty()
    {
        return Some((cap(provider), name.trim().to_string()));
    }
    let rest = m.description.strip_prefix("Provider:")?;
    let provider = rest.lines().next().unwrap_or("").split(" • ").next().unwrap_or("");
    (!provider.trim().is_empty()).then(|| (cap(provider), m.name.clone()))
}

/// The provider a model is listed under: its own, or "Other" when the agent names none.
fn provider_of(m: &acp::ModelInfo) -> String {
    model_provider(m).map_or_else(|| tr("ui.agent_chat_provider_other").to_string(), |(p, _)| p)
}

/// The providers of the agent's models in the order the agent lists them, each with how many
/// models it serves. Empty when the models come from fewer than two providers: one list is enough.
pub fn model_providers(app: &SolveApp) -> Vec<(String, usize)> {
    let mut out: Vec<(String, usize)> = Vec::new();
    for m in &app.agent_chat.models {
        let p = provider_of(m);
        match out.iter_mut().find(|(n, _)| *n == p) {
            Some(e) => e.1 += 1,
            None => out.push((p, 1)),
        }
    }
    if out.len() < 2 {
        out.clear();
    }
    out
}

/// The provider whose models the header lists: the one picked while the agent still offers it,
/// else the current model's, else the first. `None` when there is no provider picker.
pub fn shown_provider(app: &SolveApp) -> Option<String> {
    let providers = model_providers(app);
    if let Some(p) = &app.agent_chat.model_provider
        && providers.iter().any(|(n, _)| n == p)
    {
        return Some(p.clone());
    }
    let current = app.agent_chat.models.iter().find(|m| m.id == app.agent_chat.model).map(provider_of);
    current.filter(|p| providers.iter().any(|(n, _)| n == p)).or_else(|| providers.first().map(|(n, _)| n.clone()))
}

/// The models the header's model picker lists, each with the name it shows: with a provider picker,
/// the shown provider's models without the provider in front; otherwise every model as it is named.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn listed_models(app: &SolveApp) -> Vec<(acp::ModelInfo, String)> {
    let Some(provider) = shown_provider(app) else {
        return app.agent_chat.models.iter().map(|m| (m.clone(), m.name.clone())).collect();
    };
    app.agent_chat
        .models
        .iter()
        .filter(|m| provider_of(m) == provider)
        .map(|m| (m.clone(), model_provider(m).map_or_else(|| m.name.clone(), |(_, n)| n)))
        .collect()
}

/// Show one provider's models in the model picker (the header's provider picker). The agent keeps
/// its model until one of them is chosen. A provider the agent does not offer is ignored.
pub fn set_model_provider(app: &mut SolveApp, provider: &str) {
    if model_providers(app).iter().any(|(n, _)| n == provider) {
        app.agent_chat.model_provider = Some(provider.to_string());
    }
}

/// Switch the agent to one of its own modes (the header's mode picker).
pub fn set_mode(app: &mut SolveApp, mode: &str) {
    if let Some(options) = app.agent_chat.config_options.as_ref() {
        let Some(option) = options.iter().find(|x| x.category == "mode") else {
            return;
        };
        let id = option.id.clone();
        set_config_option(app, &id, mode);
        return;
    }
    if app.agent_chat.modes.iter().any(|(id, _)| id == mode)
        && let Some(b) = app.agent_chat.backend.as_mut()
    {
        b.set_mode(mode);
        app.agent_chat.mode = mode.to_string();
    }
}

/// Change a reported modern ACP configuration option; only acknowledged snapshots change state.
pub fn set_config_option(app: &mut SolveApp, id: &str, value: &str) {
    set_config_option_checked(app, id, value, true);
}

/// [`set_config_option`], with a choice the agent did not list (`listed_only` false: a typed
/// model, which the agent may refuse). Returns whether it was sent.
fn set_config_option_checked(app: &mut SolveApp, id: &str, value: &str, listed_only: bool) -> bool {
    let Some(option) = app.agent_chat.config_options.as_ref().and_then(|xs| xs.iter().find(|x| x.id == id)) else {
        return false;
    };
    if app.agent_chat.config_pending
        || !matches!(app.agent_chat.state(), State::Ready | State::Busy)
        || (listed_only && !option.options.iter().any(|x| x.value == value))
    {
        return false;
    }
    let Some(b) = app.agent_chat.backend.as_mut() else { return false };
    b.set_config_option(id, value);
    app.agent_chat.config_pending = true;
    true
}

/// `ui.agent_chat` on the control channel (tests and screenshots): `enabled`, `command`,
/// `connect_at_start` and `open_at_start` (the launch preferences), `open`,
/// `side`, `minimized`, `width`, `refresh`, `connect`, `stop`, `attach` (a file's path, for the
/// next message), `remove_file` (index), `send`, `attach_view`, `attach_selection`, `pick`,
/// `permission` (an option id, or `"cancel"`), `approve` (`true`/`false`, the oldest held
/// request), `cancel` (Stop), `send_now` (steer: stop, then send the queue), `remove_pending` (index), `undo` (the newest message's changes); returns the panel's state.
pub fn control(app: &mut SolveApp, ctx: &egui::Context, p: &Value) -> Result<Value, String> {
    if let Some(e) = p.get("features").and_then(Value::as_bool) {
        app.preferences.ai_features = e;
    }
    if let Some(b) = p.get("toolbar_button").and_then(Value::as_bool) {
        app.preferences.ai_toolbar_button = b;
    }
    if let Some(b) = p.get("connect_at_start").and_then(Value::as_bool) {
        app.preferences.agent_connect_at_start = b;
    }
    if let Some(b) = p.get("open_at_start").and_then(Value::as_bool) {
        app.preferences.agent_chat_open_at_start = b;
    }
    if let Some(m) = p.get("permissions") {
        let Some(mode) = m.as_str().and_then(agents::Mode::parse) else { return Err("permissions must be ask, edits or all".into()) };
        app.preferences.ai_permissions = mode;
    }
    // Add an agent (as the Add agent dialog does), for tests and screenshots:
    // {id?, name, kind, command | base_url + model + request_timeout_s?}.
    if let Some(a) = p.get("add_agent") {
        let mut a = a.clone();
        // Seconds are rounded and clamped to 30 s ..= 1 h, not refused for being out of range.
        if let Some(t) = a.as_object_mut().and_then(|o| o.get_mut("request_timeout_s")) {
            let secs = t.as_f64().filter(|s| s.is_finite()).ok_or("add_agent: request_timeout_s must be a number of seconds")?;
            *t = json!(agents::request_timeout_s(secs));
        }
        let mut c: agents::AgentConfig = serde_json::from_value(a).map_err(|e| format!("add_agent: {e}"))?;
        if c.id.is_empty() {
            c.id = agents::new_id(&app.preferences, if c.kind.is_api() { c.kind.id() } else { "custom" });
        }
        c.key = false;
        app.preferences.agents.retain(|x| x.id != c.id);
        app.preferences.agents.push(c);
    }
    if let Some(id) = p.get("agent").and_then(Value::as_str) {
        if !agents::entries(app).iter().any(|e| e.id == id) {
            return Err(format!("no agent `{id}`"));
        }
        if app.agent_chat.agent_id.as_deref() != Some(id) {
            disconnect(app);
            app.agent_chat.agent_id = Some(id.to_string());
        }
    }
    // The Add agent dialog: open on a kind (`installed`, `custom`, `api`, `anthropic`), or `false`
    // to close. `anthropic` is an API endpoint on the Anthropic-compatible protocol.
    if let Some(d) = p.get("add_dialog") {
        match d.as_str() {
            Some(k) => {
                let (kind, protocol) = match k {
                    "installed" => (settings::AddKind::Installed, None),
                    "custom" => (settings::AddKind::Custom, None),
                    "api" => (settings::AddKind::Api, Some(agents::Protocol::OpenAi)),
                    "anthropic" => (settings::AddKind::Api, Some(agents::Protocol::Anthropic)),
                    _ => return Err("add_dialog must be installed, custom, api, anthropic or false".into()),
                };
                settings::open_add(app);
                if let Some(a) = app.agent_chat.settings.add.as_mut() {
                    a.kind = kind;
                    if kind != settings::AddKind::Api {
                        a.name.clear();
                    }
                    if let Some(protocol) = protocol {
                        settings::choose_preset(a, protocol, 0);
                    }
                }
            }
            None => app.agent_chat.settings.add = None,
        }
    }
    // The same dialog on an agent you added (by id), filled with what it holds; its key stays hidden.
    if let Some(id) = p.get("edit_dialog") {
        let id = id.as_str().ok_or("edit_dialog must be the id of an agent you added")?;
        if !settings::open_edit(app, id) {
            return Err(format!("edit_dialog: no added agent `{id}`"));
        }
    }
    if let Some(id) = p.get("default_agent").and_then(Value::as_str) {
        app.preferences.agent_default = id.to_string();
    }
    if let Some(m) = p.get("mode").and_then(Value::as_str) {
        set_mode(app, m);
    }
    if let Some(m) = p.get("provider").and_then(Value::as_str) {
        set_model_provider(app, m);
    }
    if let Some(m) = p.get("model").and_then(Value::as_str) {
        if app.agent_chat.models.iter().any(|x| x.id == m) {
            set_model(app, m);
        } else {
            set_custom_model(app, m);
        }
    }
    if let Some(option) = p.get("config_option") {
        let id = option.get("id").and_then(Value::as_str).ok_or("config_option.id must be a string")?;
        let value = option.get("value").and_then(Value::as_str).ok_or("config_option.value must be a string")?;
        set_config_option(app, id, value);
    }
    if !shown(app) {
        return Ok(status(app));
    }
    if let Some(o) = p.get("open").and_then(Value::as_bool) {
        app.agent_chat.open = o;
    }
    if let Some(s) = p.get("side").and_then(Value::as_str) {
        if !matches!(s, "left" | "right") {
            return Err("side must be left or right".into());
        }
        app.preferences.agent_chat_side = s.into();
    }
    if let Some(m) = p.get("minimized").and_then(Value::as_bool) {
        app.preferences.agent_chat_minimized = m;
    }
    if let Some(w) = p.get("popped").and_then(Value::as_bool) {
        app.preferences.agent_chat_popped = w;
    }
    if let Some(w) = p.get("width").and_then(Value::as_f64).filter(|w| w.is_finite()) {
        app.preferences.agent_chat_width = (w as f32).clamp(panel::MIN_WIDTH, panel::MAX_WIDTH);
        panel::forget_width(ctx);
    }
    if let Some(path) = p.get("attach").and_then(Value::as_str) {
        attach_path(app, path)?;
    }
    if let Some(n) = p.get("remove_file").and_then(Value::as_u64) {
        let n = usize::try_from(n).ok().filter(|n| *n < app.agent_chat.attachments.len()).ok_or("no such file on the next message")?;
        app.agent_chat.attachments.remove(n);
    }
    if let Some(a) = p.get("attach_view").and_then(Value::as_bool) {
        app.agent_chat.attach_view = a;
    }
    if let Some(a) = p.get("attach_selection").and_then(Value::as_bool) {
        app.agent_chat.attach_selection = a;
    }
    if p.get("pick").and_then(Value::as_bool) == Some(true) {
        start_pick(app);
    }
    if let Some(pick) = p.get("picker").and_then(Value::as_bool) {
        app.agent_chat.picking = Some(pick);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        if p.get("refresh").and_then(Value::as_bool) == Some(true) {
            refresh(app, ctx);
        }
        if p.get("stop").and_then(Value::as_bool) == Some(true) {
            disconnect(app);
        }
        if p.get("connect").and_then(Value::as_bool) == Some(true) {
            connect(app, ctx);
        }
    }
    if let Some(text) = p.get("send").and_then(Value::as_str) {
        send(app, ctx, text.to_string());
    }
    if let Some(o) = p.get("permission").and_then(Value::as_str) {
        let Some(n) = app.agent_chat.items.iter().rposition(|i| matches!(i, Item::Permission { answer: None, .. })) else {
            return Err("no permission request is waiting".into());
        };
        answer_permission(app, n, (o != "cancel").then_some(o));
    }
    if let Some(a) = p.get("approve") {
        let Some(id) = app.agent_chat.held.first().map(|h| h.id) else { return Err("no request is waiting for approval".into()) };
        match (a.as_bool(), a.as_str()) {
            (Some(allow), _) => approve(app, id, allow),
            (_, Some("always")) => approve_always(app, id),
            _ => return Err("approve must be true, false or \"always\"".into()),
        }
    }
    if p.get("cancel").and_then(Value::as_bool) == Some(true) {
        cancel(app);
    }
    if p.get("new_chat").and_then(Value::as_bool) == Some(true) {
        new_chat(app);
    }
    if p.get("send_now").and_then(Value::as_bool) == Some(true) {
        send_now(app, ctx);
    }
    if let Some(n) = p.get("remove_pending").and_then(Value::as_u64) {
        remove_pending(app, n as usize);
    }
    if p.get("undo").and_then(Value::as_bool) == Some(true) {
        let Some(n) = app.agent_chat.items.iter().rposition(|i| matches!(i, Item::Changes { .. })) else { return Err("no changes to undo".into()) };
        undo_changes(app, n);
    }
    Ok(status(app))
}

pub fn status(app: &SolveApp) -> Value {
    let c = &app.agent_chat;
    let items: Vec<Value> = c
        .items
        .iter()
        .map(|i| match i {
            Item::User { text, image, pictures, context, files } => json!({"user": text, "image": image, "pictures": pictures.len(), "files": files, "context": context.items.iter().map(|i| json!({"label": i.label, "detail": i.detail})).collect::<Vec<_>>(), "context_omitted": context.omitted}),
            Item::Agent { thought, text } => json!({"agent": text, "thought": thought}),
            Item::Tool { id, title, kind, status, input, output } => {
                json!({"tool": id, "title": title, "kind": kind, "status": status.map(|s| format!("{s:?}")), "input": input, "output": output})
            }
            Item::Plan(p) => json!({"plan": p}),
            Item::Permission { title, options, answer, .. } => {
                json!({"permission": title, "options": options.iter().map(|o| json!({"id": o.id, "name": o.name, "kind": o.kind})).collect::<Vec<_>>(), "answer": answer})
            }
            Item::Approval { title, answer, .. } => json!({"approval": title, "answer": answer}),
            Item::Changes { steps, undone, .. } => json!({"changes": steps, "undone": undone}),
            Item::Notice { text, error } => json!({"notice": text, "error": error}),
        })
        .collect();
    #[cfg(not(target_arch = "wasm32"))]
    let detection = c.detection.as_ref().map(|d| {
        json!({
            "found": d.found.iter().map(|f| json!({"id": f.id, "name": f.name, "argv": f.argv, "version": f.version, "login": f.login.id(), "problem": f.problem})).collect::<Vec<_>>(),
            "missing": d.missing.iter().map(|m| json!({"id": m.id, "install": m.install})).collect::<Vec<_>>(),
            "ms": d.took_ms,
        })
    });
    #[cfg(target_arch = "wasm32")]
    let detection: Option<Value> = None;
    let pid = c.backend.as_ref().and_then(|b| b.pid());
    let mut state = json!({
        "features": app.preferences.ai_features,
        "configOptions": c.config_options,
        "configPending": c.config_pending,
        "toolbarButton": app.preferences.ai_toolbar_button,
        "connectAtStart": app.preferences.agent_connect_at_start,
        "openAtStart": app.preferences.agent_chat_open_at_start,
        "lastAgent": app.preferences.agent_last,
        "permissions": app.preferences.ai_permissions.id(),
        "agents": agents::entries(app).iter().map(|e| json!({"id": e.id, "name": e.name, "kind": e.kind.id(), "found": e.found, "version": e.version, "login": e.login, "problem": e.problem, "request_timeout_s": e.kind.is_api().then_some(e.request_timeout_s)})).collect::<Vec<_>>(),
        "currentAgent": agents::current(app).map(|e| e.id),
        "models": c.models.iter().map(|m| json!({"id": m.id, "name": m.name, "description": m.description})).collect::<Vec<_>>(),
        "model": c.model,
        "providers": model_providers(app).iter().map(|(name, count)| json!({"name": name, "count": count})).collect::<Vec<_>>(),
        "provider": shown_provider(app),
        "modelWarning": settings::model_warning(app),
        "modes": c.modes.iter().map(|(id, name)| json!({"id": id, "name": name})).collect::<Vec<_>>(),
        "mode": c.mode,
        "open": c.open,
        "side": app.preferences.agent_chat_side,
        "minimized": app.preferences.agent_chat_minimized,
        "popped": app.preferences.agent_chat_popped,
        "window": app.preferences.agent_chat_window,
        "width": app.preferences.agent_chat_width,
        "state": c.state().id(),
        "agent": c.agent,
        "images": c.images,
        "documents": c.documents,
        "authStatus": c.auth_status,
        "authMethods": c.auth.iter().map(|m| json!({"id": m.id, "name": m.name, "kind": match &m.kind { acp::AuthKind::Agent => "agent", acp::AuthKind::Terminal { .. } => "terminal", acp::AuthKind::EnvVar { .. } => "env_var" }})).collect::<Vec<_>>(),
        "toolsPort": c.tools_port,
        "pid": pid,
        "waiting": c.waiting(),
        "picked": c.picked_context.items.iter().map(|i| json!({"label": i.label, "detail": i.detail})).collect::<Vec<_>>(),
        "pending": c.pending.iter().map(|p| json!({"text": p.text, "image": p.image, "context": p.context.total(), "files": p.files.iter().map(|f| f.name.clone()).collect::<Vec<_>>()})).collect::<Vec<_>>(),
        "attachments": c.attachments.iter().map(|f| f.name.clone()).collect::<Vec<_>>(),
        "loading": c.loading.iter().map(|f| f.name.clone()).collect::<Vec<_>>(),
        "stopped": c.stopped,
        "turn": c.turn.as_ref().map(|t| json!({"changes": t.changes, "requests": t.requests})),
        "turnWarning": c.turn_warning,
        "detection": detection,
        "items": items,
    });
    // Added apart: `json!` has reached the compiler's recursion limit above.
    if let Some(state) = state.as_object_mut() {
        state.insert("modelsNote".into(), json!(c.models_note));
        state.insert("customModels".into(), json!(custom_models_accepted(app)));
    }
    state
}

#[cfg(test)]
mod tests;
