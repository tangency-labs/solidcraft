//! Links from a design to other design files (Insert Design's linked components, and the kinds of
//! link to come): where a link keeps its file and how it finds it again. A link keeps the file
//! relative to the linking design's folder once that design is saved, so a folder of designs can
//! move together; before the design has a file the path is kept whole.
//!
//! Opening a linked design ([`open_linked`]) gives the design or its [`LinkState`] with the
//! reason, in one set of state words every kind of link reports.

use std::io::ErrorKind;
use std::path::{Component as PathPart, Path, PathBuf};

use serde::Serialize;
use solidcraft_doc::{DesignLink, DocError, Document};

/// The folder relative link paths are kept against: the design's own, once it has a file.
fn base_dir(path: Option<&str>) -> Option<PathBuf> {
    path.and_then(|p| absolute(Path::new(p)).parent().map(Path::to_path_buf))
}

/// A path as given, made absolute when the platform can, with `.` and `..` worked out.
pub(super) fn absolute(p: &Path) -> PathBuf {
    let full = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    let mut out = PathBuf::new();
    for c in full.components() {
        match c {
            PathPart::CurDir => {}
            PathPart::ParentDir if matches!(out.components().next_back(), Some(PathPart::Normal(_))) => {
                out.pop();
            }
            c => out.push(c.as_os_str()),
        }
    }
    out
}

/// `target` relative to the folder `base` (both absolute), when they share a root.
fn relative(base: &Path, target: &Path) -> Option<PathBuf> {
    let (b, t): (Vec<_>, Vec<_>) = (base.components().collect(), target.components().collect());
    let root = |c: &[PathPart]| c.iter().take_while(|x| matches!(x, PathPart::Prefix(_) | PathPart::RootDir)).count();
    if root(&b) == 0 || b.get(..root(&b)) != t.get(..root(&b)) {
        return None;
    }
    let common = b.iter().zip(&t).take_while(|(x, y)| x == y).count();
    let mut out = PathBuf::new();
    for _ in common..b.len() {
        out.push("..");
    }
    for c in t.iter().skip(common) {
        out.push(c.as_os_str());
    }
    Some(out)
}

/// How a link stores `file` for a design saved at `design` (`None`: not saved yet).
pub(super) fn stored(design: Option<&str>, file: &Path) -> String {
    let file = absolute(file);
    match base_dir(design).and_then(|d| relative(&d, &file)) {
        // Relative paths are written with `/`, so a design moves between systems with its parts.
        Some(rel) => rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/"),
        None => file.to_string_lossy().into_owned(),
    }
}

/// The file a stored link path names, for a design saved at `design`.
pub(super) fn resolve(design: Option<&str>, link: &str) -> PathBuf {
    let p = Path::new(link);
    match base_dir(design) {
        // Browser storage keys do not let the OS resolve `..` for us.
        Some(d) if p.is_relative() => absolute(&d.join(p)),
        _ => p.to_path_buf(),
    }
}

/// The links of `doc` stored for a design saved at `to` instead of at `from`.
pub(super) fn rebased(doc: &Document, from: Option<&str>, to: &str) -> std::collections::BTreeMap<u64, DesignLink> {
    doc.links.iter().map(|(c, l)| (*c, DesignLink { path: rebase(&l.path, from, to), feature: l.feature })).collect()
}

/// One link's path, kept for a design saved at `from`, as a design saved at `to` keeps it: the
/// one Save As rule every kind of link follows.
pub(super) fn rebase(link: &str, from: Option<&str>, to: &str) -> String {
    stored(Some(to), &resolve(from, link))
}

/// Whether two paths name one file: the same path, or (on native hosts) the same file through a
/// symlink or a path with different case. A design can't link itself under either.
pub(super) fn same_file(a: &Path, b: &Path) -> bool {
    absolute(a) == absolute(b) || matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y)
}

/// Where a link stands against its file: the one set of state words every kind of link reports.
/// Opening tells [`LinkState::Missing`] to [`LinkState::Newer`]; whether an open design is
/// [`LinkState::UpToDate`] or [`LinkState::OutOfDate`] is the link's own comparison.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkState {
    /// The file holds what the link last took from it.
    UpToDate,
    /// The file changed since the link last took from it.
    OutOfDate,
    /// There is no file at the link's path.
    Missing,
    /// The file is there but isn't a design this build can read.
    Unreadable,
    /// The file is outside the folders this agent may use.
    NotAllowed,
    /// The design was saved by a newer SolidCraft: updating SolidCraft opens it.
    Newer,
}

impl LinkState {
    /// The state word (`up_to_date`, `missing`…) that link listings report.
    pub fn word(self) -> &'static str {
        match self {
            LinkState::UpToDate => "up_to_date",
            LinkState::OutOfDate => "out_of_date",
            LinkState::Missing => "missing",
            LinkState::Unreadable => "unreadable",
            LinkState::NotAllowed => "not_allowed",
            LinkState::Newer => "newer",
        }
    }
}

/// Why a linked design can't be opened: its state, and the reason to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkProblem {
    pub state: LinkState,
    pub reason: String,
}

impl std::fmt::Display for LinkProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reason)
    }
}

fn problem(state: LinkState, reason: String) -> LinkProblem {
    LinkProblem { state, reason }
}

/// The state of an I/O error reading `shown`.
fn io_problem(shown: &str, e: &std::io::Error) -> LinkProblem {
    match e.kind() {
        // The folder scope's refusal says which folders the agent may use.
        ErrorKind::PermissionDenied => problem(LinkState::NotAllowed, e.to_string()),
        ErrorKind::NotFound => problem(LinkState::Missing, format!("{shown}: not found")),
        _ => problem(LinkState::Unreadable, format!("{shown}: {e}")),
    }
}

/// Why the path a design names may not be reached at all, judged from its text before anything
/// asks the disk or the network (#7372): a network location whose server isn't allowed
/// ([`LinkState::NotAllowed`]), or a device ([`LinkState::Unreadable`]). `design` is the linking
/// design's file, whose own server is allowed.
fn refusal(shown: &str, design: Option<&str>) -> Option<LinkProblem> {
    let remote = solidcraft_io::vfs::design_path_refusal(shown, design)?;
    let reason = solidcraft_io::vfs::check_design_path(shown, design).err().map(|e| e.to_string()).unwrap_or_else(|| shown.to_string());
    Some(match remote {
        solidcraft_io::vfs::Remote::Network(_) => problem(LinkState::NotAllowed, reason),
        solidcraft_io::vfs::Remote::Device => problem(LinkState::Unreadable, reason),
    })
}

/// Where the linked design at `file` stands without reading it: `None` when it can be read.
/// Cheap, for a design being opened, which must not read every file it links. A refused network
/// or device path is judged from its text alone, with nothing asked of the disk or the network.
pub(super) fn link_presence(file: &Path, design: Option<&str>) -> Option<LinkProblem> {
    let shown = file.display().to_string();
    refusal(&shown, design).or_else(|| solidcraft_io::vfs::len(&shown).err().map(|e| io_problem(&shown, &e)))
}

/// The linked design at `file`, linked from the design saved at `design`, read through the
/// folders the caller may use, or why it can't be: a network location not allowed or a device
/// (judged from the path's text before anything is read), missing, not a design this build reads,
/// outside an agent's folders, or saved by a newer SolidCraft. Each with the reason to show.
pub(super) fn open_linked(file: &Path, design: Option<&str>) -> Result<Document, LinkProblem> {
    let shown = file.display().to_string();
    if let Some(why) = refusal(&shown, design) {
        return Err(why);
    }
    if !solidcraft_io::is_design_path(&shown) {
        return Err(problem(LinkState::Unreadable, format!("{shown}: not a .{} design", solidcraft_io::DESIGN_EXT)));
    }
    let bytes = solidcraft_io::vfs::read_design_path(&shown, design, solidcraft_io::MAX_DESIGN_BYTES).map_err(|e| io_problem(&shown, &e))?;
    solidcraft_io::read_design(&bytes).map_err(|e| match e {
        solidcraft_io::IoError::Doc(DocError::Newer(why)) => problem(LinkState::Newer, format!("{shown}: {why}")),
        e => problem(LinkState::Unreadable, format!("{shown}: {e}")),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn link_paths_are_kept_relative_to_the_design_and_follow_save_as() {
        let root = absolute(Path::new("projects"));
        let part = root.join("parts").join("plate.solidcraft");
        let design = root.join("assembly").join("case.solidcraft");
        let design = design.to_string_lossy().into_owned();
        assert_eq!(stored(Some(&design), &part), "../parts/plate.solidcraft");
        assert_eq!(resolve(Some(&design), "../parts/plate.solidcraft"), part);
        // A design path given relative to the working folder works the same.
        assert_eq!(stored(Some("case.solidcraft"), &absolute(Path::new("parts/plate.solidcraft"))), "parts/plate.solidcraft");
        // Before the design has a file the path is kept whole; saving it makes it relative.
        let unsaved = stored(None, &part);
        assert!(Path::new(&unsaved).is_absolute(), "{unsaved}");
        let mut doc = Document::default();
        doc.links.insert(7, DesignLink { path: unsaved, feature: 3 });
        let moved = rebased(&doc, None, &design);
        assert_eq!(moved.get(&7).map(|l| l.path.as_str()), Some("../parts/plate.solidcraft"));
        let beside = root.join("parts").join("asm.solidcraft").to_string_lossy().into_owned();
        assert_eq!(
            rebased(&Document { links: moved, ..Document::default() }, Some(&design), &beside).get(&7).map(|l| l.path.as_str()),
            Some("plate.solidcraft")
        );
    }

    #[test]
    fn one_rule_rebases_every_link_for_save_as() {
        let root = absolute(Path::new("projects"));
        let from = root.join("asm").join("case.solidcraft").to_string_lossy().into_owned();
        let to = root.join("asm").join("v2").join("case.solidcraft").to_string_lossy().into_owned();
        assert_eq!(rebase("../parts/plate.solidcraft", Some(&from), &to), "../../parts/plate.solidcraft");
        // A link kept whole (the design had no file yet) becomes relative too.
        let whole = root.join("parts").join("plate.solidcraft").to_string_lossy().into_owned();
        assert_eq!(rebase(&whole, None, &to), "../../parts/plate.solidcraft");
    }

    /// One folder per test, with the files it needs.
    fn folder(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("solidcraft-design-link-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn opening_a_linked_design_reports_its_state_and_reason() {
        let dir = folder("open");
        let good = dir.join("plate.solidcraft");
        std::fs::write(&good, solidcraft_io::write_design(&Document::new("plate"))).unwrap();
        assert_eq!(open_linked(&good, None).map(|d| d.name), Ok("plate".to_string()));
        assert_eq!(link_presence(&good, None), None);
        let state = |file: &Path| open_linked(file, None).err().map(|e| (e.state, e.reason));
        // Missing: no file there (and the same without reading it).
        let missing = dir.join("gone.solidcraft");
        assert!(matches!(state(&missing), Some((LinkState::Missing, r)) if r.contains("gone.solidcraft")));
        assert_eq!(link_presence(&missing, None).map(|p| p.state), Some(LinkState::Missing));
        // Unreadable: not a design file, not a design inside, or a folder.
        std::fs::write(dir.join("part.step"), b"ISO-10303-21;").unwrap();
        assert!(matches!(state(&dir.join("part.step")), Some((LinkState::Unreadable, r)) if r.contains("not a .solidcraft design")));
        std::fs::write(dir.join("junk.solidcraft"), b"not json").unwrap();
        assert!(matches!(state(&dir.join("junk.solidcraft")), Some((LinkState::Unreadable, _))));
        std::fs::create_dir_all(dir.join("folder.solidcraft")).unwrap();
        assert!(matches!(state(&dir.join("folder.solidcraft")), Some((LinkState::Unreadable, _))));
        // Newer: a format this build doesn't read says to update, not that it can't be read.
        let mut newer: serde_json::Value = serde_json::from_slice(&solidcraft_io::write_design(&Document::new("next"))).unwrap();
        newer["format"] = serde_json::json!(solidcraft_doc::format::format_string(solidcraft_doc::format::FORMAT + 1));
        std::fs::write(dir.join("next.solidcraft"), newer.to_string()).unwrap();
        assert!(matches!(state(&dir.join("next.solidcraft")), Some((LinkState::Newer, r)) if r.contains("update SolidCraft")));
        // Not allowed: outside the folders an agent may use, without saying whether it exists.
        let inside = dir.join("inside");
        std::fs::create_dir_all(&inside).unwrap();
        solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([inside])), || {
            assert_eq!(state(&good).map(|s| s.0), Some(LinkState::NotAllowed));
            assert_eq!(link_presence(&missing, None).map(|p| p.state), Some(LinkState::NotAllowed));
        });
        let words: Vec<&str> =
            [LinkState::UpToDate, LinkState::OutOfDate, LinkState::Missing, LinkState::Unreadable, LinkState::NotAllowed, LinkState::Newer]
                .iter()
                .map(|s| s.word())
                .collect();
        assert_eq!(words, ["up_to_date", "out_of_date", "missing", "unreadable", "not_allowed", "newer"]);
        assert_eq!(serde_json::json!(LinkState::NotAllowed), serde_json::json!("not_allowed"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A network location not allowed, or a device, is refused from the path's text alone:
    /// opening it asks nothing of the disk or the network (#7372).
    #[test]
    fn network_and_device_paths_are_refused_before_anything_is_read() {
        let calls = solidcraft_io::vfs::file_calls();
        let network = Path::new(r"\\fileserver\share\standard.solidcraft");
        let refused = open_linked(network, None).err();
        assert!(matches!(&refused, Some(p) if p.state == LinkState::NotAllowed && p.reason.contains("fileserver")), "{refused:?}");
        assert_eq!(link_presence(network, None).map(|p| p.state), Some(LinkState::NotAllowed));
        let device = Path::new(r"\\.\pipe\standard.solidcraft");
        assert_eq!(open_linked(device, None).err().map(|p| p.state), Some(LinkState::Unreadable));
        assert_eq!(solidcraft_io::vfs::file_calls(), calls, "a refused path asked the file system");
    }

    #[test]
    fn a_path_and_its_alias_are_the_same_file() {
        let dir = folder("same");
        let file = dir.join("a.solidcraft");
        std::fs::write(&file, b"{}").unwrap();
        assert!(same_file(&file, &dir.join(".").join("a.solidcraft")));
        assert!(!same_file(&file, &dir.join("b.solidcraft")));
        #[cfg(unix)]
        {
            let alias = dir.join("alias.solidcraft");
            std::os::unix::fs::symlink(&file, &alias).unwrap();
            assert!(same_file(&file, &alias));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
