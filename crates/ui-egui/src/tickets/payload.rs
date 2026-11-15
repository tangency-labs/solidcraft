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

/// One command in "What you were doing".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub command: String,
    pub params: String,
    pub ok: bool,
    pub result: String,
}

/// What the dialog collected, shown to the user before anything is sent.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Evidence {
    /// App version, OS, GPU and driver: the exact text sent.
    pub system: String,
    pub app_version: String,
    pub os: String,
    /// The last commands, oldest first.
    pub steps: Vec<Step>,
    /// The failing command and its error, when one failed.
    pub failing: Option<(Option<String>, String)>,
    /// The last lines of the log.
    pub log: Vec<String>,
    /// The 3D view as PNG.
    pub screenshot: Option<Vec<u8>>,
    /// The open design: its file name and bytes.
    pub design: Option<(String, Vec<u8>)>,
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

/// The text of "What you were doing", as the dialog shows it: one command a line, its settings
/// and its result under it.
pub fn steps_text(steps: &[Step], failing: Option<&(Option<String>, String)>) -> String {
    let mut out = String::new();
    for (i, s) in steps.iter().enumerate() {
        let outcome = if s.ok { "done" } else { "failed" };
        out.push_str(&format!("{}. {} ({outcome})\n    settings: {}\n    result: {}\n", i + 1, s.command, s.params, s.result));
    }
    if let Some((command, error)) = failing {
        out.push_str(&format!("\nFailing: {}{error}\n", command.as_deref().map(|c| format!("{c}: ")).unwrap_or_default()));
    }
    if out.is_empty() { "No commands recorded yet.".into() } else { out }
}

/// The report for `form` with the boxes in `ticks` and nothing more.
pub fn build(form: &Form, ticks: Ticks, evidence: &Evidence, client: &str) -> Outgoing {
    // A box with nothing behind it sends nothing, and the consent record says so.
    let ticks = Ticks {
        system: ticks.system && !evidence.system.is_empty(),
        repro: ticks.repro,
        log: ticks.log && !evidence.log.is_empty(),
        screenshot: ticks.screenshot && evidence.screenshot.is_some(),
        design: ticks.design && evidence.design.is_some(),
        crash: ticks.crash && evidence.crash.is_some(),
    };
    let mut ticket = json!({
        "app": if form.app.is_empty() { super::api::DEFAULT_APP } else { form.app.as_str() },
        "title": cut(form.title.trim(), MAX_TITLE),
        "body": cut(&form.description, MAX_DESCRIPTION),
        "type": form.kind.id(),
        "consent": {
            "system": ticks.system,
            "repro": ticks.repro,
            "log": ticks.log,
            "screenshot": ticks.screenshot,
            "design": ticks.design,
            "crash": ticks.crash,
            "client": client,
        },
    });
    if ticks.system {
        ticket["system"] = json!(evidence.system);
        ticket["appVersion"] = json!(cut(&evidence.app_version, 100));
        ticket["os"] = json!(cut(&evidence.os, 100));
    }
    if ticks.repro {
        let mut repro = json!({ "commands": evidence.steps });
        if let Some((command, error)) = &evidence.failing {
            repro["failing"] = json!({ "error": error });
            if let Some(command) = command {
                repro["failing"]["command"] = json!(command);
            }
        }
        ticket["repro"] = repro;
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
