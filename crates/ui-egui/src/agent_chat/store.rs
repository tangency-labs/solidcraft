//! A design's agent context on disk (#1539): `bracket.sc` gets `bracket_context/` beside
//! it, holding what an agent needs to carry the design on: `context.json` (its id, the design, the
//! agent), `transcript.jsonl` (append-only, one record a line), `plan.json` (the agent's plan),
//! `references/` (pictures that went with messages) and `notes.md`. An unsaved design keeps its
//! context in the data folder until its first save.
//!
//! The folder may come with a design from someone else, so reading it is bounded (a line, the
//! transcript, `context.json`, a reference) and writing never follows a symbolic link out of it.

use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The most of the transcript a load reads: its latest records.
pub const TRANSCRIPT_LIMIT: u64 = 64 << 20;
/// A transcript line longer than this is skipped on load.
pub const RECORD_LIMIT: usize = 1 << 20;
/// The largest `context.json` or `plan.json` read.
pub const FILE_LIMIT: u64 = 1 << 20;
/// The largest reference picture kept.
pub const REFERENCE_LIMIT: usize = 32 << 20;

/// The context folder of a design saved at `design`: `bracket.sc` → `bracket_context`
/// beside it.
pub fn folder_for(design: &Path) -> PathBuf {
    let stem = design.file_stem().unwrap_or(design.as_os_str());
    let mut name = stem.to_os_string();
    name.push("_context");
    design.with_file_name(name)
}

/// Where an unsaved design's context lives under the data folder `data` until its first save.
pub fn unsaved_folder(data: &Path, id: &str) -> PathBuf {
    data.join("agent-chat").join("contexts").join(id)
}

/// Move a context folder out of the way when a design is saved over one that had its own
/// conversation: `c_context` becomes `c_context.old` (`c_context.old-2`, … when that is taken),
/// never merged or deleted. Where it went.
pub fn set_aside(dir: &Path) -> io::Result<PathBuf> {
    real_dir(dir)?;
    let name = dir.file_name().ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "context has no folder name"))?;
    let mut number = 1_u64;
    loop {
        let mut candidate = name.to_os_string();
        candidate.push(if number == 1 { ".old".to_owned() } else { format!(".old-{number}") });
        let target = dir.with_file_name(candidate);
        match fs::symlink_metadata(&target) {
            Ok(_) => {
                number = number.checked_add(1).ok_or_else(|| io::Error::new(ErrorKind::AlreadyExists, "no available context folder name"))?;
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                fs::rename(dir, &target)?;
                return Ok(target);
            }
            Err(error) => return Err(error),
        }
    }
}

/// `context.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Meta {
    pub id: String,
    /// The design's file name once saved (`bracket.sc`); empty before its first save.
    pub design: String,
    pub agent: String,
    /// Seconds since 1970.
    pub created: u64,
}

/// One line of `transcript.jsonl`. Lines of a kind this build doesn't know are skipped on load.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Record {
    User {
        text: String,
        /// Pictures that went with it, by their name in `references/`.
        #[serde(default)]
        pictures: Vec<String>,
        /// The names of the other files that went with it.
        #[serde(default)]
        files: Vec<String>,
    },
    Agent {
        text: String,
        #[serde(default)]
        thought: bool,
    },
    Tool {
        id: String,
        title: String,
        #[serde(default)]
        status: String,
        #[serde(default)]
        output: String,
    },
    /// A message's changes, merged into one undo step.
    Changes {
        steps: usize,
        #[serde(default)]
        undone: bool,
    },
    Notice {
        text: String,
        #[serde(default)]
        error: bool,
    },
}

/// An open context folder.
#[derive(Debug)]
pub struct Context {
    dir: PathBuf,
    meta: Meta,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// Give each context a process-unique id without keeping mutable global state on disk.
fn new_id() -> io::Result<String> {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|error| io::Error::new(ErrorKind::InvalidData, error))?.as_nanos();
    let mut hash = Sha256::new();
    hash.update(nanos.to_le_bytes());
    hash.update(std::process::id().to_le_bytes());
    hash.update(NEXT_ID.fetch_add(1, Ordering::Relaxed).to_le_bytes());
    let digest = hash.finalize();
    Ok(digest.iter().take(16).map(|byte| format!("{byte:02x}")).collect())
}

/// Reject a link at this path, including a dangling link, without checking ancestors.
fn reject_link(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(io::Error::new(ErrorKind::InvalidInput, "symbolic link in context")),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Require an existing folder itself to be a real directory.
fn real_dir(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.is_dir() && !meta.file_type().is_symlink() {
        Ok(())
    } else {
        Err(io::Error::new(ErrorKind::InvalidInput, "context path is not a real directory"))
    }
}

/// Read a small JSON file with a cap that also holds if it grows after metadata is read.
fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> io::Result<T> {
    reject_link(path)?;
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > FILE_LIMIT {
        return Err(io::Error::new(ErrorKind::InvalidData, "context file is too large"));
    }
    let mut bytes = Vec::new();
    file.take(FILE_LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > FILE_LIMIT {
        return Err(io::Error::new(ErrorKind::InvalidData, "context file is too large"));
    }
    serde_json::from_slice(&bytes).map_err(|error| io::Error::new(ErrorKind::InvalidData, error))
}

/// Replace a whole context file atomically, after checking its own path for links.
fn write_whole(path: &Path, bytes: &[u8]) -> io::Result<()> {
    reject_link(path)?;
    solidcraft_engine::io::safe_write::write_atomic(path, bytes, false)
}

/// Copy regular files and folders, ignoring links in a context received from elsewhere.
fn copy_regular(source: &Path, target: &Path) -> io::Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let to = target.join(entry.file_name());
        if kind.is_dir() {
            fs::create_dir(&to)?;
            copy_regular(&entry.path(), &to)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), to)?;
        }
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct Plan {
    entries: Vec<PlanEntry>,
}

#[derive(Serialize, Deserialize)]
struct PlanEntry {
    content: String,
    status: String,
}

impl Context {
    /// Make a new context in `dir` (created with its parents) for `design` and `agent`, with a
    /// fresh id: `context.json`, an empty `transcript.jsonl` and `references/`. A folder already
    /// holding a `context.json` is left as it is (`AlreadyExists`).
    pub fn create(dir: &Path, design: &str, agent: &str) -> io::Result<Context> {
        Self::create_with_id(dir, design, agent, new_id()?)
    }

    fn create_with_id(dir: &Path, design: &str, agent: &str, id: String) -> io::Result<Context> {
        reject_link(dir)?;
        if fs::symlink_metadata(dir.join("context.json")).is_ok() {
            return Err(io::Error::new(ErrorKind::AlreadyExists, "context already exists"));
        }
        fs::create_dir_all(dir)?;
        real_dir(dir)?;
        let meta = Meta {
            id,
            design: design.to_owned(),
            agent: agent.to_owned(),
            created: SystemTime::now().duration_since(UNIX_EPOCH).map_err(|error| io::Error::new(ErrorKind::InvalidData, error))?.as_secs(),
        };
        let bytes = serde_json::to_vec(&meta).map_err(io::Error::other)?;
        write_whole(&dir.join("context.json"), &bytes)?;
        OpenOptions::new().write(true).create_new(true).open(dir.join("transcript.jsonl"))?;
        fs::create_dir(dir.join("references"))?;
        Ok(Context { dir: dir.to_path_buf(), meta })
    }

    /// Make a new context for a design that has not been saved yet, in
    /// [`unsaved_folder`]`(data, id)`: the folder is named by the context's own id, so either
    /// finds the other. Its design is empty until the first save moves it beside the file.
    pub fn create_unsaved(data: &Path, agent: &str) -> io::Result<Context> {
        let id = new_id()?;
        Self::create_with_id(&unsaved_folder(data, &id), "", agent, id)
    }

    /// Open the context in `dir`: `NotFound` without a `context.json`, `InvalidData` when it
    /// can't be read as one (or is larger than [`FILE_LIMIT`]).
    pub fn open(dir: &Path) -> io::Result<Context> {
        real_dir(dir)?;
        let meta = read_json(&dir.join("context.json"))?;
        Ok(Context { dir: dir.to_path_buf(), meta })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn meta(&self) -> &Meta {
        &self.meta
    }

    /// Add one record at the end of the transcript.
    pub fn append(&self, record: &Record) -> io::Result<()> {
        real_dir(&self.dir)?;
        let path = self.dir.join("transcript.jsonl");
        reject_link(&path)?;
        let mut line = serde_json::to_vec(record).map_err(io::Error::other)?;
        if line.len() > RECORD_LIMIT {
            return Err(io::Error::new(ErrorKind::InvalidInput, "transcript record is too large"));
        }
        line.push(b'\n');
        OpenOptions::new().append(true).open(path)?.write_all(&line)
    }

    /// The transcript's records in order, reading at most [`TRANSCRIPT_LIMIT`] bytes.
    pub fn transcript(&self) -> std::io::Result<Vec<Record>> {
        self.transcript_within(TRANSCRIPT_LIMIT)
    }

    /// The transcript's records in order from its last `limit` bytes (whole lines only). Lines
    /// that don't parse, of an unknown kind, or longer than [`RECORD_LIMIT`] are skipped.
    pub fn transcript_within(&self, limit: u64) -> io::Result<Vec<Record>> {
        real_dir(&self.dir)?;
        let path = self.dir.join("transcript.jsonl");
        reject_link(&path)?;
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();
        let start = len.saturating_sub(limit.min(TRANSCRIPT_LIMIT));
        let cut_line = if start > 0 {
            file.seek(SeekFrom::Start(start - 1))?;
            let mut before = [0];
            file.read_exact(&mut before)?;
            before[0] != b'\n'
        } else {
            false
        };
        file.seek(SeekFrom::Start(start))?;
        let mut bytes = Vec::new();
        file.take(limit.min(TRANSCRIPT_LIMIT)).read_to_end(&mut bytes)?;
        let mut lines = bytes.split(|byte| *byte == b'\n');
        if cut_line {
            let _ = lines.next();
        }
        Ok(lines.filter(|line| !line.is_empty() && line.len() <= RECORD_LIMIT).filter_map(|line| serde_json::from_slice(line).ok()).collect())
    }

    /// Replace `plan.json` with the agent's plan: (step, status) pairs.
    pub fn write_plan(&self, plan: &[(String, String)]) -> io::Result<()> {
        real_dir(&self.dir)?;
        let entries = plan.iter().map(|(content, status)| PlanEntry { content: content.clone(), status: status.clone() }).collect();
        let bytes = serde_json::to_vec(&Plan { entries }).map_err(io::Error::other)?;
        write_whole(&self.dir.join("plan.json"), &bytes)
    }

    /// The saved plan; empty when there is none.
    pub fn plan(&self) -> io::Result<Vec<(String, String)>> {
        real_dir(&self.dir)?;
        match read_json::<Plan>(&self.dir.join("plan.json")) {
            Ok(plan) => Ok(plan.entries.into_iter().map(|entry| (entry.content, entry.status)).collect()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error),
        }
    }

    /// Keep a picture in `references/` under its own name made safe (no folders, no `..`), with
    /// `-2`, `-3`, … before the extension when the name is taken; the name it was kept under.
    /// Larger than [`REFERENCE_LIMIT`]: `InvalidInput`, nothing written.
    pub fn add_reference(&self, name: &str, bytes: &[u8]) -> io::Result<String> {
        if bytes.len() > REFERENCE_LIMIT {
            return Err(io::Error::new(ErrorKind::InvalidInput, "reference is too large"));
        }
        real_dir(&self.dir)?;
        let refs = self.dir.join("references");
        real_dir(&refs)?;
        let base = name.rsplit(['/', '\\']).next().filter(|part| !part.is_empty() && *part != "." && *part != "..").unwrap_or("reference");
        let base = base.replace("..", "_");
        let base = base.as_str();
        let (stem, ext) = match base.rsplit_once('.') {
            Some((stem, ext)) if !stem.is_empty() => (stem, format!(".{ext}")),
            _ => (base, String::new()),
        };
        for number in 1..=u32::MAX {
            let kept = if number == 1 { base.to_owned() } else { format!("{stem}-{number}{ext}") };
            let path = refs.join(&kept);
            match OpenOptions::new().write(true).create_new(true).open(path) {
                Ok(mut file) => {
                    file.write_all(bytes)?;
                    file.sync_all()?;
                    return Ok(kept);
                }
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(ErrorKind::AlreadyExists, "reference names exhausted"))
    }

    /// `notes.md`; empty when there is none.
    pub fn notes(&self) -> io::Result<String> {
        real_dir(&self.dir)?;
        let path = self.dir.join("notes.md");
        reject_link(&path)?;
        match File::open(path) {
            Ok(file) => {
                if file.metadata()?.len() > FILE_LIMIT {
                    return Err(io::Error::new(ErrorKind::InvalidData, "notes file is too large"));
                }
                let mut bytes = Vec::new();
                file.take(FILE_LIMIT + 1).read_to_end(&mut bytes)?;
                if bytes.len() as u64 > FILE_LIMIT {
                    return Err(io::Error::new(ErrorKind::InvalidData, "notes file is too large"));
                }
                String::from_utf8(bytes).map_err(|error| io::Error::new(ErrorKind::InvalidData, error))
            }
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(String::new()),
            Err(error) => Err(error),
        }
    }

    pub fn write_notes(&self, text: &str) -> io::Result<()> {
        real_dir(&self.dir)?;
        write_whole(&self.dir.join("notes.md"), text.as_bytes())
    }

    /// A design's first save: move the folder to `dir` and record the design's file name. The id
    /// stays. A `dir` already holding a context is left as it is (`AlreadyExists`), and so is this
    /// one.
    pub fn move_to(&mut self, dir: &Path, design: &str) -> io::Result<()> {
        real_dir(&self.dir)?;
        reject_link(dir)?;
        if fs::symlink_metadata(dir.join("context.json")).is_ok() {
            return Err(io::Error::new(ErrorKind::AlreadyExists, "context already exists"));
        }
        if let Some(parent) = dir.parent() {
            fs::create_dir_all(parent)?;
        }
        match fs::rename(&self.dir, dir) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::CrossesDevices => {
                fs::create_dir(dir)?;
                copy_regular(&self.dir, dir)?;
                fs::remove_dir_all(&self.dir)?;
            }
            Err(error) => return Err(error),
        }
        self.dir = dir.to_path_buf();
        self.meta.design = design.to_owned();
        let bytes = serde_json::to_vec(&self.meta).map_err(io::Error::other)?;
        write_whole(&dir.join("context.json"), &bytes)
    }

    /// Save As: a copy in `dir` for the design `design`, with a new id and the same transcript,
    /// plan, notes and references. This one is left as it is. A `dir` already holding a context is
    /// left as it is (`AlreadyExists`).
    pub fn copy_to(&self, dir: &Path, design: &str) -> io::Result<Context> {
        self.copy_with_id(dir, design, new_id()?)
    }

    /// The agent's own New: a copy for the new, unsaved design in [`unsaved_folder`]`(data, id)`,
    /// named by its new id, with an empty design until the first save. This one is left as it is.
    pub fn copy_unsaved(&self, data: &Path) -> io::Result<Context> {
        let id = new_id()?;
        self.copy_with_id(&unsaved_folder(data, &id), "", id)
    }

    fn copy_with_id(&self, dir: &Path, design: &str, id: String) -> io::Result<Context> {
        real_dir(&self.dir)?;
        reject_link(dir)?;
        if fs::symlink_metadata(dir.join("context.json")).is_ok() {
            return Err(io::Error::new(ErrorKind::AlreadyExists, "context already exists"));
        }
        if let Some(parent) = dir.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::create_dir(dir)?;
        copy_regular(&self.dir, dir)?;
        let mut meta = self.meta.clone();
        meta.id = id;
        meta.design = design.to_owned();
        let bytes = serde_json::to_vec(&meta).map_err(io::Error::other)?;
        write_whole(&dir.join("context.json"), &bytes)?;
        Ok(Context { dir: dir.to_path_buf(), meta })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::ErrorKind;

    /// A fresh folder for one test (each test runs in its own process).
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("solidcraft-context-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn pair(a: &str, b: &str) -> (String, String) {
        (a.to_string(), b.to_string())
    }

    fn user(text: &str) -> Record {
        Record::User { text: text.into(), pictures: vec![], files: vec![] }
    }

    #[test]
    fn context_folder_sits_beside_the_design() {
        let root = Path::new("/work/designs");
        assert_eq!(folder_for(&root.join("bracket.sc")), root.join("bracket_context"));
        assert_eq!(folder_for(&root.join("my.part.sc")), root.join("my.part_context"));
        // Designs saved before `.sc` (#7325) still open, and keep the same folder name.
        assert_eq!(folder_for(&root.join("bracket.solidcraft")), root.join("bracket_context"));
        assert_eq!(folder_for(&root.join("bracket")), root.join("bracket_context"));
        let data = Path::new("/data");
        assert_eq!(unsaved_folder(data, "abc123"), data.join("agent-chat").join("contexts").join("abc123"));
    }

    #[test]
    fn context_create_lays_out_the_folder() {
        let dir = scratch("create").join("bracket_context");
        let ctx = Context::create(&dir, "bracket.sc", "claude").unwrap();
        assert!(dir.join("context.json").is_file());
        assert!(dir.join("transcript.jsonl").is_file());
        assert!(dir.join("references").is_dir());
        assert!(!ctx.meta().id.is_empty());
        assert_eq!(ctx.meta().design, "bracket.sc");
        assert_eq!(ctx.meta().agent, "claude");
        assert!(ctx.meta().created > 1_700_000_000);
        assert_eq!(ctx.dir(), dir.as_path());
        // Two contexts never share an id.
        let other = Context::create(&scratch("create-2").join("x_context"), "", "claude").unwrap();
        assert_ne!(other.meta().id, ctx.meta().id);
    }

    #[test]
    fn context_create_leaves_an_existing_context_alone() {
        let dir = scratch("exists").join("bracket_context");
        let first = Context::create(&dir, "bracket.sc", "claude").unwrap();
        first.append(&user("keep me")).unwrap();
        let err = Context::create(&dir, "bracket.sc", "codex").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::AlreadyExists);
        let again = Context::open(&dir).unwrap();
        assert_eq!(again.meta(), first.meta());
        assert_eq!(again.transcript().unwrap(), vec![user("keep me")]);
    }

    #[test]
    fn context_create_unsaved_names_the_folder_by_its_id() {
        let data = scratch("unsaved");
        let ctx = Context::create_unsaved(&data, "claude").unwrap();
        // The folder and the context share one id, so either finds the other.
        assert_eq!(ctx.dir(), unsaved_folder(&data, &ctx.meta().id).as_path());
        assert_eq!(ctx.meta().design, "");
        assert_eq!(ctx.meta().agent, "claude");
        assert!(ctx.dir().join("transcript.jsonl").is_file());
        assert_eq!(Context::open(ctx.dir()).unwrap().meta(), ctx.meta());
        // Each unsaved design gets a folder of its own.
        let other = Context::create_unsaved(&data, "claude").unwrap();
        assert_ne!(other.dir(), ctx.dir());
        assert_ne!(other.meta().id, ctx.meta().id);
    }

    #[test]
    fn context_round_trip() {
        let dir = scratch("round").join("bracket_context");
        let ctx = Context::create(&dir, "bracket.sc", "claude").unwrap();
        let records = vec![
            Record::User { text: "a bracket, 40 mm arm\nwith two holes".into(), pictures: vec!["view.png".into()], files: vec!["spec.pdf".into()] },
            Record::Agent { text: "Planning it.".into(), thought: true },
            Record::Tool { id: "t1".into(), title: "execute".into(), status: "completed".into(), output: "{\"ok\":true}".into() },
            Record::Changes { steps: 3, undone: false },
            Record::Agent { text: "Done: 40 × 30 mm.".into(), thought: false },
            Record::Notice { text: "Stopped".into(), error: true },
        ];
        for r in &records {
            ctx.append(r).unwrap();
        }
        let plan = vec![pair("Sketch the profile", "completed"), pair("Extrude 6 mm", "in_progress")];
        ctx.write_plan(&plan).unwrap();
        ctx.write_notes("Purpose: a wall bracket.\n").unwrap();
        let kept = ctx.add_reference("view.png", b"\x89PNG fake").unwrap();
        assert_eq!(kept, "view.png");

        let back = Context::open(&dir).unwrap();
        assert_eq!(back.meta(), ctx.meta());
        assert_eq!(back.transcript().unwrap(), records);
        assert_eq!(back.plan().unwrap(), plan);
        assert_eq!(back.notes().unwrap(), "Purpose: a wall bracket.\n");
        assert_eq!(fs::read(dir.join("references").join("view.png")).unwrap(), b"\x89PNG fake");
        // One record a line: the newline in the first message stays inside its line.
        let text = fs::read_to_string(dir.join("transcript.jsonl")).unwrap();
        assert_eq!(text.lines().count(), records.len());

        // Appending after reopening keeps what was there.
        back.append(&user("and a third hole")).unwrap();
        let mut more = records.clone();
        more.push(user("and a third hole"));
        assert_eq!(Context::open(&dir).unwrap().transcript().unwrap(), more);

        // A new plan replaces the old one.
        back.write_plan(&[pair("Fillet the corner", "pending")]).unwrap();
        assert_eq!(back.plan().unwrap(), vec![pair("Fillet the corner", "pending")]);
    }

    #[test]
    fn context_without_plan_or_notes_reads_empty() {
        let dir = scratch("empty").join("c");
        let ctx = Context::create(&dir, "", "claude").unwrap();
        assert!(ctx.plan().unwrap().is_empty());
        assert_eq!(ctx.notes().unwrap(), "");
        assert!(ctx.transcript().unwrap().is_empty());
    }

    #[test]
    fn context_open_refuses_what_is_not_a_context() {
        let dir = scratch("open-bad");
        assert_eq!(Context::open(&dir.join("missing")).unwrap_err().kind(), ErrorKind::NotFound);
        let garbage = dir.join("garbage");
        fs::create_dir_all(&garbage).unwrap();
        fs::write(garbage.join("context.json"), b"{not json").unwrap();
        assert_eq!(Context::open(&garbage).unwrap_err().kind(), ErrorKind::InvalidData);
        let huge = dir.join("huge");
        fs::create_dir_all(&huge).unwrap();
        let padding = " ".repeat(FILE_LIMIT as usize + 1);
        fs::write(huge.join("context.json"), format!("{{\"id\":\"x\",\"design\":\"\",\"agent\":\"a\",\"created\":1{padding}}}")).unwrap();
        assert_eq!(Context::open(&huge).unwrap_err().kind(), ErrorKind::InvalidData);
    }

    #[test]
    fn context_transcript_skips_lines_it_cannot_read() {
        let dir = scratch("skip").join("c");
        let ctx = Context::create(&dir, "", "claude").unwrap();
        ctx.append(&user("first")).unwrap();
        let long = format!("{{\"kind\":\"user\",\"text\":\"{}\"}}\n", "x".repeat(RECORD_LIMIT + 1));
        let mut extra = String::new();
        extra.push_str("{broken\n");
        extra.push_str("{\"kind\":\"hologram\",\"text\":\"from a later build\"}\n");
        extra.push_str(&long);
        extra.push('\n');
        let mut file = fs::OpenOptions::new().append(true).open(dir.join("transcript.jsonl")).unwrap();
        std::io::Write::write_all(&mut file, extra.as_bytes()).unwrap();
        drop(file);
        ctx.append(&user("last")).unwrap();
        assert_eq!(ctx.transcript().unwrap(), vec![user("first"), user("last")]);
    }

    #[test]
    fn context_transcript_within_keeps_the_latest_whole_lines() {
        let dir = scratch("within").join("c");
        let ctx = Context::create(&dir, "", "claude").unwrap();
        for i in 0..100 {
            ctx.append(&user(&format!("message {i:03}"))).unwrap();
        }
        let line = fs::read_to_string(dir.join("transcript.jsonl")).unwrap().lines().next().unwrap().len() as u64 + 1;
        // Room for ten lines and a half: the ten latest come back, the cut one doesn't.
        let got = ctx.transcript_within(line * 10 + line / 2).unwrap();
        let want: Vec<Record> = (90..100).map(|i| user(&format!("message {i:03}"))).collect();
        assert_eq!(got, want);
    }

    #[test]
    fn context_references_stay_inside_and_never_overwrite() {
        let dir = scratch("refs").join("c");
        let ctx = Context::create(&dir, "", "claude").unwrap();
        assert_eq!(ctx.add_reference("../../evil.png", b"a").unwrap(), "evil.png");
        assert_eq!(ctx.add_reference("evil.png", b"b").unwrap(), "evil-2.png");
        assert_eq!(ctx.add_reference("/abs/evil.png", b"c").unwrap(), "evil-3.png");
        let odd = ctx.add_reference("", b"d").unwrap();
        assert!(!odd.is_empty() && !odd.contains('/') && !odd.contains('\\') && odd != "." && odd != "..");
        let refs = dir.join("references");
        assert_eq!(fs::read(refs.join("evil.png")).unwrap(), b"a");
        assert_eq!(fs::read(refs.join("evil-2.png")).unwrap(), b"b");
        assert_eq!(fs::read(refs.join("evil-3.png")).unwrap(), b"c");
        assert!(!dir.parent().unwrap().join("evil.png").exists());
        let big = vec![0u8; REFERENCE_LIMIT + 1];
        assert_eq!(ctx.add_reference("big.png", &big).unwrap_err().kind(), ErrorKind::InvalidInput);
        assert!(!refs.join("big.png").exists());
    }

    #[test]
    fn context_move_to_keeps_the_id() {
        let root = scratch("move");
        let data = unsaved_folder(&root.join("data"), "abc");
        let mut ctx = Context::create(&data, "", "claude").unwrap();
        ctx.append(&user("before saving")).unwrap();
        ctx.write_plan(&[pair("step", "pending")]).unwrap();
        ctx.add_reference("view.png", b"png").unwrap();
        let id = ctx.meta().id.clone();
        let beside = folder_for(&root.join("designs").join("bracket.sc"));
        fs::create_dir_all(beside.parent().unwrap()).unwrap();
        ctx.move_to(&beside, "bracket.sc").unwrap();
        assert_eq!(ctx.dir(), beside.as_path());
        assert!(!data.exists());
        let back = Context::open(&beside).unwrap();
        assert_eq!(back.meta().id, id);
        assert_eq!(back.meta().design, "bracket.sc");
        assert_eq!(back.transcript().unwrap(), vec![user("before saving")]);
        assert_eq!(back.plan().unwrap(), vec![pair("step", "pending")]);
        assert!(beside.join("references").join("view.png").is_file());
        // Writing goes on in the new place.
        ctx.append(&user("after saving")).unwrap();
        assert_eq!(Context::open(&beside).unwrap().transcript().unwrap().len(), 2);
    }

    #[test]
    fn context_move_to_an_existing_context_changes_nothing() {
        let root = scratch("move-clash");
        let mut ctx = Context::create(&root.join("a_context"), "", "claude").unwrap();
        ctx.append(&user("mine")).unwrap();
        let theirs = Context::create(&root.join("b_context"), "b.sc", "codex").unwrap();
        theirs.append(&user("theirs")).unwrap();
        let err = ctx.move_to(&root.join("b_context"), "b.sc").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::AlreadyExists);
        assert_eq!(ctx.dir(), root.join("a_context").as_path());
        assert_eq!(Context::open(&root.join("a_context")).unwrap().transcript().unwrap(), vec![user("mine")]);
        assert_eq!(Context::open(&root.join("b_context")).unwrap().transcript().unwrap(), vec![user("theirs")]);
    }

    #[test]
    fn context_copy_to_gets_a_new_id() {
        let root = scratch("copy");
        let src = Context::create(&root.join("bracket_context"), "bracket.sc", "claude").unwrap();
        src.append(&user("hello")).unwrap();
        src.write_plan(&[pair("step", "completed")]).unwrap();
        src.write_notes("notes").unwrap();
        src.add_reference("view.png", b"png").unwrap();
        let copy = src.copy_to(&root.join("copy_context"), "copy.sc").unwrap();
        assert_ne!(copy.meta().id, src.meta().id);
        assert_eq!(copy.meta().design, "copy.sc");
        assert_eq!(copy.meta().agent, "claude");
        assert_eq!(copy.transcript().unwrap(), vec![user("hello")]);
        assert_eq!(copy.plan().unwrap(), vec![pair("step", "completed")]);
        assert_eq!(copy.notes().unwrap(), "notes");
        assert_eq!(fs::read(root.join("copy_context").join("references").join("view.png")).unwrap(), b"png");
        // The source is as it was, and the two grow apart from here.
        copy.append(&user("only in the copy")).unwrap();
        let src_again = Context::open(&root.join("bracket_context")).unwrap();
        assert_eq!(src_again.meta(), src.meta());
        assert_eq!(src_again.transcript().unwrap(), vec![user("hello")]);
        // Onto an existing context: refused, both untouched.
        let err = src.copy_to(&root.join("copy_context"), "copy.sc").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::AlreadyExists);
        assert_eq!(Context::open(&root.join("copy_context")).unwrap().transcript().unwrap().len(), 2);
    }

    #[test]
    fn a_copy_for_an_unsaved_design_is_named_by_its_own_id() {
        let root = scratch("copy-unsaved");
        let src = Context::create(&root.join("bracket_context"), "bracket.sc", "claude").unwrap();
        src.append(&user("hello")).unwrap();
        let data = root.join("data");
        let copy = src.copy_unsaved(&data).unwrap();
        assert_ne!(copy.meta().id, src.meta().id);
        assert_eq!(copy.meta().design, "", "unsaved until its first save");
        assert_eq!(copy.dir(), unsaved_folder(&data, &copy.meta().id).as_path());
        assert_eq!(Context::open(copy.dir()).unwrap().transcript().unwrap(), vec![user("hello")]);
        assert_eq!(Context::open(&root.join("bracket_context")).unwrap().meta(), src.meta(), "the source is as it was");
    }

    #[test]
    fn context_set_aside_keeps_every_earlier_folder() {
        let root = scratch("aside");
        let dir = root.join("c_context");
        let first = Context::create(&dir, "c.sc", "claude").unwrap();
        first.append(&user("first")).unwrap();
        assert_eq!(set_aside(&dir).unwrap(), root.join("c_context.old"));
        assert!(!dir.exists());
        let second = Context::create(&dir, "c.sc", "claude").unwrap();
        second.append(&user("second")).unwrap();
        assert_eq!(set_aside(&dir).unwrap(), root.join("c_context.old-2"));
        assert_eq!(Context::open(&root.join("c_context.old")).unwrap().transcript().unwrap(), vec![user("first")]);
        assert_eq!(Context::open(&root.join("c_context.old-2")).unwrap().transcript().unwrap(), vec![user("second")]);
        assert_eq!(set_aside(&root.join("missing_context")).unwrap_err().kind(), ErrorKind::NotFound);
    }

    #[cfg(unix)]
    #[test]
    fn context_never_writes_through_a_link() {
        use std::os::unix::fs::symlink;
        let root = scratch("links");
        let outside = root.join("outside.txt");
        fs::write(&outside, b"precious").unwrap();
        let ctx = Context::create(&root.join("c"), "", "claude").unwrap();
        // A folder from someone else whose files point out of it.
        for name in ["transcript.jsonl", "plan.json", "notes.md"] {
            let p = ctx.dir().join(name);
            let _ = fs::remove_file(&p);
            symlink(&outside, &p).unwrap();
        }
        // Appending refuses; a whole-file write may refuse or replace the link itself, but never
        // writes through it.
        assert!(ctx.append(&user("x")).is_err());
        let _ = ctx.write_plan(&[pair("s", "pending")]);
        let _ = ctx.write_notes("x");
        assert_eq!(fs::read(&outside).unwrap(), b"precious");
        // A references folder that is a link elsewhere.
        let elsewhere = root.join("elsewhere");
        fs::create_dir_all(&elsewhere).unwrap();
        fs::remove_dir_all(ctx.dir().join("references")).unwrap();
        symlink(&elsewhere, ctx.dir().join("references")).unwrap();
        assert!(ctx.add_reference("view.png", b"png").is_err());
        assert!(!elsewhere.join("view.png").exists());
        // A context folder that is itself a link elsewhere (only the folder itself is checked: on
        // macOS the temporary folder's own path goes through one).
        let target = root.join("target");
        fs::create_dir_all(&target).unwrap();
        symlink(&target, root.join("linked_context")).unwrap();
        assert!(Context::create(&root.join("linked_context"), "", "claude").is_err());
        assert!(Context::open(&root.join("linked_context")).is_err());
        assert_eq!(fs::read_dir(&target).unwrap().count(), 0);
    }
}
