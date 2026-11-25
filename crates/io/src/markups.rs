//! Review markups: places in a design someone marked for change, each with a note, replies and
//! pictures, kept in a set of their own beside the design for a person or an agent to work through
//! ([review markups](../../../docs/review-markups.md)).
//!
//! A set is `<design>.markups.json`, with its lock in `<design>.markups/`. Its threads and anchors
//! are never part of the design: a markup names a face, an edge, a body, a feature or a component
//! and owns none of them, so writing them changes neither the design file nor its undo history,
//! and the file reads with no app running. A markup's pictures are the one part the design holds:
//! their bytes are entries of the design container ([`solidcraft_doc::markup_image`]), as a
//! canvas's image is, and the set names each by its entry.
//!
//! Every change is one step on the set as it stands: [`update`] takes the set's lock, reads the
//! file, makes the change, checks the whole set ([`Set::check`]) and puts the new file in place
//! whole ([`crate::safe_write`]). So two writers (the window and an agent) wait for each other and
//! neither loses the other's work, and a crash leaves the last whole file. In the browser there is
//! one writer and no disk: the file lives where the host keeps files ([`crate::vfs`]).
//!
//! This module holds the file and its rules only. Which face a markup is on, and where it is now,
//! is the engine's (`markup.*` commands in `solidcraft-engine`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::vfs;

/// The set's format. A change an older version would misread takes a new number; a set with a
/// higher number is refused, never rewritten.
pub const FORMAT: u32 = 1;
/// What a set's file name ends with: the design's file name and this.
pub const SUFFIX: &str = ".markups.json";
/// The most markups a set holds.
pub const MAX_MARKUPS: usize = 2000;
/// The longest note, reply or reason from the markup's maker, in characters.
pub const MAX_NOTE: usize = 4000;
/// The longest message from anyone but the markup's maker, in characters. A longer one is refused
/// whole, never cut.
pub const MAX_REPLY: usize = 280;
/// The most messages in a markup's thread, its note among them.
pub const MAX_MESSAGES: usize = 200;
/// The most pictures on a markup.
pub const MAX_IMAGES: usize = 8;
/// The largest picture, in bytes.
pub const MAX_IMAGE: usize = solidcraft_doc::markup_image::MAX_BYTES;
/// The most a set's pictures take together, in bytes.
pub const MAX_ALL_IMAGES: u64 = 1 << 30;
/// The longest name (of a person, a body, a face, an edge, a picture), in characters.
pub const MAX_NAME: usize = 4096;
/// The largest set file read.
pub const MAX_FILE: usize = 64 << 20;
/// The farthest a stored point or camera target may lie from the origin, in mm.
pub const MAX_COORD: f64 = 1e9;
/// The deepest component path an anchor names.
pub const MAX_PATH: usize = 64;

/// Where a markup stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Open,
    Resolved,
    Rejected,
}

impl Status {
    /// The word a list and a command use for it.
    pub fn word(self) -> &'static str {
        match self {
            Status::Open => "open",
            Status::Resolved => "resolved",
            Status::Rejected => "rejected",
        }
    }

    /// The status a word names (`None`: no status is called that).
    pub fn from_word(word: &str) -> Option<Status> {
        match word {
            "open" => Some(Status::Open),
            "resolved" => Some(Status::Resolved),
            "rejected" => Some(Status::Rejected),
            _ => None,
        }
    }
}

/// One message in a markup's thread: the note first, then each reply and closing reason.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub by: String,
    /// Unix seconds.
    pub at: u64,
    pub text: String,
}

/// The camera a markup was made from: the orbit camera's target (mm), yaw and pitch (degrees),
/// distance (mm) and field of view (degrees; 0 for orthographic).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct View {
    pub target: [f64; 3],
    pub yaw: f64,
    pub pitch: f64,
    pub distance: f64,
    #[serde(default)]
    pub fov: f64,
}

/// What a markup is on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Anchor {
    /// A face, by its body and its lasting name (the name `model.faces` lists), and the point on
    /// it that was marked, where it was then (mm).
    Face {
        body: String,
        /// The feature that made the body: finds it again after a rename.
        maker: u64,
        face: String,
        at: [f64; 3],
        /// The configuration that was active when it was marked (`None`: the design had none).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        config: Option<String>,
    },
    /// An edge, by its body and its name (`<face>|<face>`, as `model.edges` lists it), and the
    /// point on it that was marked.
    Edge {
        body: String,
        maker: u64,
        edge: String,
        at: [f64; 3],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        config: Option<String>,
    },
    /// A body as a whole, and the point marked on it, if any.
    Body {
        body: String,
        maker: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        at: Option<[f64; 3]>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        config: Option<String>,
    },
    /// A feature of the timeline, by id, with its name then.
    Feature {
        feature: u64,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        at: Option<[f64; 3]>,
    },
    /// A component placed in the assembly, by its occurrence path (ids from the top), with its
    /// name then.
    Occurrence {
        path: Vec<u64>,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        at: Option<[f64; 3]>,
    },
    /// A point in space (mm).
    Point { at: [f64; 3] },
    /// The view itself: the markup's `view` is what it is about.
    View {},
    /// Nothing: what it was on is gone or was never given. `why` says so in words that name no id.
    Unanchored {
        why: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        given: Option<String>,
    },
}

impl Anchor {
    /// The point the anchor was marked at, if it has one.
    pub fn at(&self) -> Option<[f64; 3]> {
        match self {
            Anchor::Face { at, .. } | Anchor::Edge { at, .. } | Anchor::Point { at } => Some(*at),
            Anchor::Body { at, .. } | Anchor::Feature { at, .. } | Anchor::Occurrence { at, .. } => *at,
            Anchor::View {} | Anchor::Unanchored { .. } => None,
        }
    }

    /// The word for its kind (`face`, `edge`…).
    pub fn kind(&self) -> &'static str {
        match self {
            Anchor::Face { .. } => "face",
            Anchor::Edge { .. } => "edge",
            Anchor::Body { .. } => "body",
            Anchor::Feature { .. } => "feature",
            Anchor::Occurrence { .. } => "occurrence",
            Anchor::Point { .. } => "point",
            Anchor::View {} => "view",
            Anchor::Unanchored { .. } => "unanchored",
        }
    }

    fn check(&self) -> Result<(), String> {
        let names: Vec<&str> = match self {
            Anchor::Face { body, face, config, .. } => [Some(body.as_str()), Some(face.as_str()), config.as_deref()].into_iter().flatten().collect(),
            Anchor::Edge { body, edge, config, .. } => [Some(body.as_str()), Some(edge.as_str()), config.as_deref()].into_iter().flatten().collect(),
            Anchor::Body { body, config, .. } => [Some(body.as_str()), config.as_deref()].into_iter().flatten().collect(),
            Anchor::Feature { name, .. } | Anchor::Occurrence { name, .. } => vec![name.as_str()],
            Anchor::Unanchored { why, given } => [Some(why.as_str()), given.as_deref()].into_iter().flatten().collect(),
            Anchor::Point { .. } | Anchor::View {} => Vec::new(),
        };
        for n in names {
            check_name(n, "an anchor's name")?;
        }
        match self {
            Anchor::Face { body, face, .. } if body.trim().is_empty() || face.trim().is_empty() => {
                return Err("a face anchor needs its body and face".into());
            }
            Anchor::Edge { body, edge, .. } if body.trim().is_empty() || edge.trim().is_empty() => {
                return Err("an edge anchor needs its body and edge".into());
            }
            Anchor::Body { body, .. } if body.trim().is_empty() => return Err("a body anchor needs its body".into()),
            Anchor::Occurrence { path, .. } if path.is_empty() || path.len() > MAX_PATH || path.contains(&0) => {
                return Err(format!("an occurrence path holds 1 to {MAX_PATH} component ids, none 0"));
            }
            _ => {}
        }
        if let Some(at) = self.at() {
            check_point(&at)?;
        }
        Ok(())
    }
}

/// A picture on a markup: its bytes are the design's entry `file`
/// ([`solidcraft_doc::Document::markup_images`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Image {
    /// `<guid>.png` or `<guid>.jpg`: the entry's name in the design.
    pub file: String,
    /// The name it came with (a file's name, `screenshot`).
    pub name: String,
    pub by: String,
    pub at: u64,
    pub bytes: u64,
}

/// Who closed a markup, why and when.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    pub by: String,
    pub reason: String,
    pub at: u64,
}

/// One markup.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Markup {
    /// Its number in the set, never used again.
    pub id: u64,
    /// 32 hexadecimal digits, the same wherever the set is copied.
    pub guid: String,
    pub status: Status,
    /// What was said, oldest first; the first message is the note.
    pub thread: Vec<Message>,
    pub anchor: Anchor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<View>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<Image>,
    /// Who made it: the reviewer.
    pub by: String,
    /// Unix seconds.
    pub made: u64,
    pub changed: u64,
    /// Set while the markup is resolved or rejected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<Outcome>,
}

impl Markup {
    /// The note: the thread's first message.
    pub fn note(&self) -> &str {
        self.thread.first().map_or("", |m| m.text.as_str())
    }

    /// The longest message `by` may write on this markup: [`MAX_NOTE`] for its maker,
    /// [`MAX_REPLY`] for anyone else.
    pub fn limit_for(&self, by: &str) -> usize {
        if by == self.by { MAX_NOTE } else { MAX_REPLY }
    }

    fn check(&self) -> Result<(), String> {
        let id = self.id;
        if !is_guid(&self.guid) {
            return Err(format!("markup {id}: its guid must be 32 hexadecimal digits"));
        }
        check_person(&self.by).map_err(|e| format!("markup {id}: {e}"))?;
        if self.thread.is_empty() || self.thread.len() > MAX_MESSAGES {
            return Err(format!("markup {id}: a thread holds 1 to {MAX_MESSAGES} messages"));
        }
        for m in &self.thread {
            check_person(&m.by).map_err(|e| format!("markup {id}: {e}"))?;
            check_text(&m.text, self.limit_for(&m.by)).map_err(|e| format!("markup {id}: {e}"))?;
        }
        self.anchor.check().map_err(|e| format!("markup {id}: {e}"))?;
        if let Some(v) = &self.view {
            check_view(v).map_err(|e| format!("markup {id}: {e}"))?;
        }
        if self.images.len() > MAX_IMAGES {
            return Err(format!("markup {id}: at most {MAX_IMAGES} pictures"));
        }
        for i in &self.images {
            if !is_image_file(&i.file) {
                return Err(format!("markup {id}: a picture's file must be `<guid>.png` or `<guid>.jpg`"));
            }
            check_name(&i.name, "a picture's name").map_err(|e| format!("markup {id}: {e}"))?;
            check_person(&i.by).map_err(|e| format!("markup {id}: {e}"))?;
            if i.bytes > MAX_IMAGE as u64 {
                return Err(format!("markup {id}: a picture is at most {MAX_IMAGE} bytes"));
            }
        }
        match (&self.status, &self.outcome) {
            (Status::Open, Some(_)) => return Err(format!("markup {id}: an open markup has no outcome")),
            (Status::Resolved | Status::Rejected, None) => return Err(format!("markup {id}: a closed markup says who closed it and why")),
            (_, Some(o)) => {
                check_person(&o.by).map_err(|e| format!("markup {id}: {e}"))?;
                check_text(&o.reason, MAX_NOTE).map_err(|e| format!("markup {id}: {e}"))?;
            }
            (Status::Open, None) => {}
        }
        Ok(())
    }
}

/// A set of markups, as its file holds it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Set {
    pub format: u32,
    /// The number the next markup gets.
    pub next_id: u64,
    pub markups: Vec<Markup>,
}

impl Default for Set {
    fn default() -> Self {
        Set { format: FORMAT, next_id: 1, markups: Vec::new() }
    }
}

impl Set {
    /// The one validation: a set that fails it is refused, never repaired or written.
    pub fn check(&self) -> Result<(), String> {
        if self.format != FORMAT {
            return Err(format!("a markup set of format {} (this SolidCraft reads format {FORMAT})", self.format));
        }
        if self.markups.len() > MAX_MARKUPS {
            return Err(format!("a set holds at most {MAX_MARKUPS} markups"));
        }
        let mut ids = std::collections::BTreeSet::new();
        let mut guids = std::collections::BTreeSet::new();
        let mut files = std::collections::BTreeSet::new();
        let mut all: u64 = 0;
        for m in &self.markups {
            if m.id == 0 || m.id >= self.next_id {
                return Err(format!("markup {}: its number must be at least 1 and below the set's next number {}", m.id, self.next_id));
            }
            if !ids.insert(m.id) {
                return Err(format!("markup {} is in the set twice", m.id));
            }
            if !guids.insert(m.guid.to_ascii_lowercase()) {
                return Err(format!("markup {}: another markup has its guid", m.id));
            }
            m.check()?;
            for i in &m.images {
                if !files.insert(i.file.to_ascii_lowercase()) {
                    return Err(format!("markup {}: a picture's file is named twice in the set", m.id));
                }
                all = all.saturating_add(i.bytes);
            }
        }
        if all > MAX_ALL_IMAGES {
            return Err(format!("a set's pictures take at most {MAX_ALL_IMAGES} bytes together"));
        }
        Ok(())
    }

    /// The markup with this number.
    pub fn get(&self, id: u64) -> Option<&Markup> {
        self.markups.iter().find(|m| m.id == id)
    }

    /// The markup with this number, to change.
    pub fn get_mut(&mut self, id: u64) -> Option<&mut Markup> {
        self.markups.iter_mut().find(|m| m.id == id)
    }

    /// The number of the markup with this guid (any case).
    pub fn by_guid(&self, guid: &str) -> Option<u64> {
        self.markups.iter().find(|m| m.guid.eq_ignore_ascii_case(guid)).map(|m| m.id)
    }
}

/// Whether `s` is a markup's or a picture's guid: 32 hexadecimal digits.
pub fn is_guid(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Whether `s` is a picture's entry name: a guid and `.png` or `.jpg`.
pub fn is_image_file(s: &str) -> bool {
    solidcraft_doc::markup_image::valid_name(s)
}

/// The kind of picture `bytes` hold by their first bytes: `png` or `jpg` (`None`: neither).
pub fn image_kind(bytes: &[u8]) -> Option<&'static str> {
    solidcraft_doc::markup_image::kind(bytes)
}

fn check_point(p: &[f64; 3]) -> Result<(), String> {
    if p.iter().all(|v| v.is_finite() && v.abs() <= MAX_COORD) {
        Ok(())
    } else {
        Err(format!("a point's coordinates must be finite and within {MAX_COORD} mm"))
    }
}

/// Whether a camera is one a list can give and the window can fly to.
pub fn check_view(v: &View) -> Result<(), String> {
    check_point(&v.target)?;
    let angles = v.yaw.is_finite() && v.pitch.is_finite() && v.yaw.abs() <= 1e6 && v.pitch.abs() <= 1e6;
    if !angles || !(v.distance.is_finite() && v.distance > 0.0 && v.distance <= MAX_COORD) || !(v.fov.is_finite() && (0.0..180.0).contains(&v.fov)) {
        return Err("a view needs finite angles, a distance above 0 and a field of view from 0 to under 180 degrees".into());
    }
    Ok(())
}

fn check_name(s: &str, what: &str) -> Result<(), String> {
    if s.chars().count() > MAX_NAME || s.chars().any(|c| c.is_control()) {
        return Err(format!("{what} is at most {MAX_NAME} characters, with no control characters"));
    }
    Ok(())
}

/// Whether `by` can name who wrote something: 1 to 200 characters, no control characters.
pub fn check_person(by: &str) -> Result<(), String> {
    let n = by.chars().count();
    if by.trim().is_empty() || n > 200 || by.chars().any(|c| c.is_control()) {
        return Err("a name is 1 to 200 characters, with no control characters".into());
    }
    Ok(())
}

/// Whether `text` can be a message of at most `limit` characters: not blank, no control characters
/// but line breaks and tabs. The error says the limit, the length and how far over it is.
pub fn check_text(text: &str, limit: usize) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("a message must not be empty".into());
    }
    if text.chars().any(|c| c.is_control() && c != '\n' && c != '\t' && c != '\r') {
        return Err("a message has no control characters but line breaks and tabs".into());
    }
    let n = text.chars().count();
    if n > limit {
        let whose = if limit == MAX_REPLY { "a message from anyone but the markup's maker" } else { "a message" };
        return Err(format!("{whose} is at most {limit} characters; this one is {n}, {} over: say it shorter", n - limit));
    }
    Ok(())
}

/// The set beside the design at `design`: `<design>.markups.json`.
pub fn beside(design: &str) -> String {
    format!("{design}{SUFFIX}")
}

/// The folder of the set at `set` (its lock): the set's path without `.json`.
/// `Err` for a path that is not a set's (`…markups.json`).
pub fn folder(set: &str) -> Result<String, String> {
    match set.strip_suffix(".json") {
        Some(stem) if set.ends_with(SUFFIX) && !stem.ends_with('/') && !stem.ends_with('\\') => Ok(stem.to_string()),
        _ => Err(format!("a markup set's file name ends with `{SUFFIX}`")),
    }
}

/// Read a set's text: a set of [`FORMAT`] as it is, checked. A newer format is refused with a
/// message; so is a file that is not a set, or one that fails [`Set::check`].
pub fn parse(text: &str) -> Result<Set, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("not a markup set: {e}"))?;
    let format = v.get("format").and_then(Value::as_u64).ok_or("not a markup set: it has no `format`")?;
    if format > u64::from(FORMAT) {
        return Err(format!("this markup set was made by a newer SolidCraft (format {format}; this one reads format {FORMAT}): open it there"));
    }
    let set: Set = serde_json::from_value(v).map_err(|e| format!("not a markup set: {e}"))?;
    set.check()?;
    Ok(set)
}

/// The text a set is written as: plain JSON without indents.
pub fn to_text(set: &Set) -> Result<String, String> {
    serde_json::to_string(set).map_err(|e| e.to_string())
}

/// The set at `set` as it stands (an empty set where there is no file yet).
pub fn read(set: &str) -> Result<Set, String> {
    folder(set)?;
    if !vfs::exists(set) {
        return Ok(Set::default());
    }
    let bytes = vfs::read(set, MAX_FILE).map_err(|e| format!("{set}: {e}"))?;
    let text = String::from_utf8(bytes).map_err(|_| format!("{set}: not a markup set: it is not text"))?;
    parse(&text).map_err(|e| format!("{set}: {e}"))
}

/// The set's lock, held while it lives (nothing in the browser, which has one writer).
struct Held {
    #[cfg(not(target_arch = "wasm32"))]
    _file: std::fs::File,
}

#[cfg(not(target_arch = "wasm32"))]
fn lock(folder: &str) -> Result<Held, String> {
    let path = format!("{folder}/lock");
    vfs::check_new(&path).map_err(|e| format!("{path}: {e}"))?;
    if std::fs::symlink_metadata(&path).is_ok_and(|m| !m.is_file()) {
        return Err(format!("{path}: not a regular file"));
    }
    let file = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&path).map_err(|e| format!("{path}: {e}"))?;
    file.lock().map_err(|e| format!("{path}: {e}"))?;
    Ok(Held { _file: file })
}

#[cfg(target_arch = "wasm32")]
fn lock(_folder: &str) -> Result<Held, String> {
    Ok(Held {})
}

/// Make the set's folder, inside what the caller may write.
fn make_folder(set: &str) -> Result<String, String> {
    let dir = folder(set)?;
    vfs::check_new(&dir).map_err(|e| format!("{dir}: {e}"))?;
    vfs::create_dir_all(&dir).map_err(|e| format!("{dir}: {e}"))?;
    Ok(dir)
}

/// One change to the set at `set`, under its lock: read it as it stands, run `step`, check the
/// result and write it whole. When `step` or the check refuses, the file stays as it was.
pub fn update<T>(set: &str, step: impl FnOnce(&mut Set) -> Result<T, String>) -> Result<T, String> {
    vfs::check_new(set).map_err(|e| format!("{set}: {e}"))?;
    let dir = make_folder(set)?;
    let _held = lock(&dir)?;
    let mut s = read(set)?;
    let out = step(&mut s)?;
    s.check()?;
    let text = to_text(&s)?;
    crate::safe_write::write_atomic_no_follow(std::path::Path::new(set), text.as_bytes()).map_err(|e| format!("{set}: {e}"))?;
    Ok(out)
}

#[cfg(test)]
#[path = "markups_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
