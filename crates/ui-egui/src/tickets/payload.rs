//! What a report sends: built from the boxes the user ticked and nothing else. Each attachment's
//! text is the text the dialog shows; the design goes only when its box is ticked.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// The kind of ticket.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    #[default]
    Bug,
    Feature,
    Question,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Bug, Kind::Feature, Kind::Question];
    pub fn id(self) -> &'static str {
        match self {
            Self::Bug => "bug",
            Self::Feature => "feature",
            Self::Question => "question",
        }
    }
}

/// The attachment boxes. Everything about the design is off until ticked; the design file
/// is off by default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ticks {
    pub system: bool,
    pub repro: bool,
    pub log: bool,
    pub screenshot: bool,
    pub design: bool,
    pub crash: bool,
}

impl Default for Ticks {
    fn default() -> Self {
        Self { system: true, repro: true, log: true, screenshot: true, design: false, crash: false }
    }
}

/// The operation that failed, as the command API took it: what a triager replays on the
/// attached design (`solidcraft-cli run replay.json --in design.sc`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Failing {
    /// The command id (`solid.fillet`), when the failure came from one.
    pub command: Option<String>,
    /// Its parameters exactly as the command API takes them.
    pub params: Option<Value>,
    pub error: String,
    /// `solidcore-ops`, `solidcore-kernel` or `app`, from the error's type.
    pub origin: Option<String>,
    /// The kernel's error code or type name, when there is one.
    pub code: Option<String>,
    /// The stable code of the kernel call the failure was made from (`repro.failing.code`), by
    /// which the ticket server groups identical failures: present only when it was made from one.
    #[serde(default)]
    pub failure_code: Option<solidcraft_engine::kernel::ErrorCode>,
}

/// What the dialog collected, shown to the user before anything is sent.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Evidence {
    /// App version, commit, target, OS and GPU: the exact text shown.
    pub system: String,
    /// The same as fields.
    pub system_fields: serde_json::Map<String, Value>,
    pub app_version: String,
    pub os: String,
    /// How the session started and where the design is: `session_start`, the design's hash,
    /// its format version and the undo steps around it. Nothing of its content.
    pub session: serde_json::Map<String, Value>,
    /// The failed operation, when the report is about one.
    pub failing: Option<Failing>,
    /// The commands run before it, oldest first (`repro.steps`), opens and selections included.
    pub steps: Vec<Value>,
    /// The last lines of the log.
    pub log: Vec<String>,
    /// The 3D view as PNG.
    pub screenshot: Option<Vec<u8>>,
    /// The design the report is about: its file name and bytes.
    pub design: Option<(String, Vec<u8>)>,
    /// Where that design came from when it is not the one open (a crash report's copy, a design
    /// left for recovery), or why there is none: the message the dialog shows.
    pub design_from: Option<&'static str>,
    /// A crash report: its id and JSON.
    pub crash: Option<(String, String)>,
}

/// One file to upload.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct File {
    pub name: String,
    pub content_type: String,
    pub kind: String,
    #[serde(skip)]
    pub bytes: Vec<u8>,
    /// The upload the server opened for it, once it did (to resume).
    #[serde(default)]
    pub upload: Option<String>,
    #[serde(default)]
    pub done: bool,
}

/// A report ready to send, or waiting in the queue: the ticket and its files. Holds no token.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Outgoing {
    pub ticket: Value,
    pub files: Vec<File>,
    /// The ticket's number once the server filed it (a resumed report then only uploads).
    #[serde(default)]
    pub filed: Option<u64>,
}

/// What the user typed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Form {
    pub app: String,
    pub kind: Kind,
    pub title: String,
    pub description: String,
}

/// The longest title the server takes.
pub const MAX_TITLE: usize = 200;
pub const MAX_DESCRIPTION: usize = 20_000;

fn cut(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// The failed operation's JSON as the ticket carries it.
pub fn failing_json(f: &Failing) -> Value {
    let mut v = json!({ "error": cut(&f.error, 20_000) });
    if let Some(c) = &f.command {
        v["command"] = json!(c);
    }
    if let Some(p) = &f.params {
        v["params"] = p.clone();
    }
    if let Some(o) = &f.origin {
        v["error_origin"] = json!(o);
    }
    if let Some(c) = &f.code {
        v["error_code"] = json!(c);
    }
    if let Some(c) = &f.failure_code {
        v["code"] = json!({"call": c.call, "kind": c.kind, "operation": c.operation});
    }
    v
}

/// The text of "The failed operation", as the dialog shows it: exactly what is sent, `design`
/// when the design's box is ticked.
pub fn repro_text(evidence: &Evidence, design: bool) -> String {
    serde_json::to_string_pretty(&repro_json(evidence, design)).unwrap_or_default()
}

/// What "The failed operation" sends: the session, the recent steps and the failed operation.
/// A step's parameters and file name (names of things in the design) go only with the design
/// (`design`), whose history holds them anyway.
pub fn repro_json(evidence: &Evidence, design: bool) -> Value {
    let mut v = Value::Object(evidence.session.clone());
    if !evidence.steps.is_empty() {
        let steps = evidence.steps.iter().map(|s| {
            let mut s = s.clone();
            if !design && let Some(o) = s.as_object_mut() {
                o.remove("params");
                o.remove("file");
            }
            s
        });
        v["steps"] = Value::Array(steps.collect());
    }
    if let Some(f) = &evidence.failing {
        v["failing"] = failing_json(f);
    }
    v
}

/// Who agreed to the attachments: the person at the dialog, or an agent's call (with whoever
/// confirmed the design or crash report, if anyone did).
#[derive(Clone, Debug, PartialEq)]
pub enum Via {
    Dialog,
    Agent { confirmed_by: Option<String> },
}

/// The report for `form` with the boxes in `ticks` and nothing more.
pub fn build(form: &Form, ticks: Ticks, evidence: &Evidence, client: &str) -> Outgoing {
    build_via(form, ticks, evidence, client, &Via::Dialog)
}

pub fn build_via(form: &Form, ticks: Ticks, evidence: &Evidence, client: &str, via: &Via) -> Outgoing {
    // A box with nothing behind it sends nothing, and the consent record says so.
    let ticks = Ticks {
        system: ticks.system && !evidence.system.is_empty(),
        repro: ticks.repro,
        log: ticks.log && !evidence.log.is_empty(),
        screenshot: ticks.screenshot && evidence.screenshot.is_some(),
        design: ticks.design && evidence.design.is_some(),
        crash: ticks.crash && evidence.crash.is_some(),
    };
    let kinds: Vec<&str> = [
        (ticks.system, "system"),
        (ticks.repro, "repro"),
        (ticks.log, "log"),
        (ticks.screenshot, "screenshot"),
        (ticks.design, "design"),
        (ticks.crash, "crash"),
    ]
    .into_iter()
    .filter_map(|(on, k)| on.then_some(k))
    .collect();
    let now = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let mut consent = json!({ "kinds": kinds, "at": solidcraft_engine::crash::utc(now), "client": client });
    match via {
        Via::Dialog => consent["via"] = json!("dialog"),
        Via::Agent { confirmed_by } => {
            consent["via"] = json!("agent");
            if let Some(who) = confirmed_by {
                consent["confirmed_by"] = json!(who);
            }
        }
    }
    let mut ticket = json!({
        "app": if form.app.is_empty() { super::api::DEFAULT_APP } else { form.app.as_str() },
        "title": cut(form.title.trim(), MAX_TITLE),
        "body": cut(&form.description, MAX_DESCRIPTION),
        "type": form.kind.id(),
        "consent": consent,
    });
    if ticks.system {
        let mut system = evidence.system_fields.clone();
        system.insert("text".into(), json!(evidence.system));
        ticket["system"] = Value::Object(system);
        ticket["appVersion"] = json!(cut(&evidence.app_version, 100));
        ticket["os"] = json!(cut(&evidence.os, 100));
    }
    if ticks.repro {
        ticket["repro"] = repro_json(evidence, ticks.design);
    }
    let mut files = Vec::new();
    let mut add = |name: String, content_type: &str, kind: &str, bytes: Vec<u8>| {
        // The server takes no empty file.
        if bytes.is_empty() {
            return;
        }
        files.push(File { name, content_type: content_type.into(), kind: kind.into(), bytes, upload: None, done: false });
    };
    if ticks.log {
        add("log.txt".into(), "text/plain", "log", evidence.log.join("\n").into_bytes());
    }
    if ticks.screenshot
        && let Some(png) = &evidence.screenshot
    {
        add("screenshot.png".into(), "image/png", "screenshot", png.clone());
    }
    if ticks.design
        && let Some((name, bytes)) = &evidence.design
    {
        add(name.clone(), "application/octet-stream", "design", bytes.clone());
    }
    if ticks.crash
        && let Some((id, json)) = &evidence.crash
    {
        add(format!("crash-{id}.json"), "application/json", "crash", json.clone().into_bytes());
    }
    Outgoing { ticket, files, filed: None }
}

/// A size as people read it.
pub fn size_text(bytes: usize) -> String {
    let b = bytes as f64;
    if b < 1024.0 {
        format!("{bytes} B")
    } else if b < 1024.0 * 1024.0 {
        format!("{:.0} KB", b / 1024.0)
    } else {
        format!("{:.1} MB", b / (1024.0 * 1024.0))
    }
}
