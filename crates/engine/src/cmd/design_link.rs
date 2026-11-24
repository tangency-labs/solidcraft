//! Links from a design to other design files (Insert Design's linked components, a drawing's
//! sources, and the kinds of link to come): where a link keeps its file and how it finds it again.
//! In memory every link path is whole ([`located`], on each open), so Save As, undo, redo,
//! checkpoints and recovery never change what it names; the file keeps it relative to the linking
//! design's folder ([`filed`], on each save), so a folder of designs can move together. Before the
//! design has a file the path is kept whole.
//!
//! Reading a linked design ([`read_linked_file`]) gives its bytes or its [`LinkState`] with the
//! reason, in one set of state words every kind of link reports.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::io::ErrorKind;
use std::path::{Component as PathPart, Path, PathBuf};
use std::sync::Arc;

use schemars::JsonSchema;
use serde::Serialize;
use solidcraft_doc::drawing::Drawing;
use solidcraft_doc::{DesignLink, Document, FeatureKind};
use solidcraft_sketch::LinkSource;

/// The folder relative link paths are kept against: the design's own, once it has a file.
fn base_dir(path: Option<&str>) -> Option<PathBuf> {
    path.and_then(|p| parent_dir(Path::new(p)))
}

/// Resolve directory aliases before comparing folders or working out `..`. Keep the final
/// filename: a missing source or a file symlink must keep the name the user linked.
fn parent_dir(path: &Path) -> Option<PathBuf> {
    #[cfg(unix)]
    if let Some(shown) = path.to_str()
        && !shown.contains("://")
        && solidcraft_io::vfs::remote(shown).is_none()
        && solidcraft_io::vfs::check_new(shown).is_ok()
        && let Ok(full) = std::path::absolute(path)
    {
        let mut parent = full.parent();
        let mut missing = PathBuf::new();
        while let Some(dir) = parent {
            match dir.canonicalize() {
                Ok(dir) => return Some(dir.join(missing)),
                // A missing source folder still moves with the project. Stop at a dangling
                // symlink or an inaccessible folder rather than guessing its destination.
                Err(e) if e.kind() == ErrorKind::NotFound && dir.symlink_metadata().is_err_and(|e| e.kind() == ErrorKind::NotFound) => {}
                _ => break,
            }
            let Some(name) = dir.file_name() else { break };
            missing = Path::new(name).join(missing);
            parent = dir.parent();
        }
    }
    // Browser keys, Windows paths and unavailable folders retain their lexical spelling.
    absolute(path).parent().map(Path::to_path_buf)
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
    let whole = absolute(file);
    let Some(base) = base_dir(design) else {
        return whole.to_string_lossy().into_owned();
    };
    match relative(&base, &link_target(file)) {
        // Relative paths are written with `/`, so a design moves between systems with its parts.
        Some(rel) => rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/"),
        None => whole.to_string_lossy().into_owned(),
    }
}

/// A link's absolute target, resolving directory aliases while keeping file symlinks distinct.
pub(super) fn link_target(file: &Path) -> PathBuf {
    parent_dir(file).zip(file.file_name()).map(|(dir, name)| dir.join(name)).unwrap_or_else(|| absolute(file))
}

/// The file a stored link path names, for a design saved at `design`.
pub(super) fn resolve(design: Option<&str>, link: &str) -> PathBuf {
    let p = Path::new(link);
    match p.is_relative().then(|| base_dir(design)).flatten() {
        // Browser storage keys do not let the OS resolve `..` for us.
        Some(d) => absolute(&d.join(p)),
        _ => p.to_path_buf(),
    }
}

/// One link's path, kept for a design saved at `from`, as a design saved at `to` keeps it: the
/// one Save As rule every kind of link follows.
pub(super) fn rebase(link: &str, from: Option<&str>, to: &str) -> String {
    stored(Some(to), &resolve(from, link))
}

/// Every link path of `doc`, a design saved at `from`, made whole ([`map_paths`]).
pub(super) fn located(doc: &mut Document, from: Option<&str>) {
    let whole = mapped(doc, &|path: &str| absolute(&resolve(from, path)).to_string_lossy().into_owned());
    put(doc, whole);
    // A sketch text's font file is read against the design's folder too (#20502).
    let fonts = {
        let mut seen = Cow::Borrowed(&*doc);
        font_paths_through(&mut seen, |font| whole_font(from, font));
        match seen {
            Cow::Owned(changed) => Some(changed),
            Cow::Borrowed(_) => None,
        }
    };
    if let Some(changed) = fonts {
        *doc = changed;
    }
}

/// `design`'s link paths as its file at `to` keeps them, its paths taken against `from`
/// ([`map_paths`]).
pub(super) fn filed(design: &mut Cow<'_, Document>, from: Option<&str>, to: &str) {
    map_paths(design, |path| rebase(path, from, to));
    let base = base_dir(Some(to));
    font_paths_through(design, |font| kept_font(base.as_deref(), font));
}

/// A font file path of a sketch text as the design keeps it in memory: relative to the design's
/// folder, a path is made whole against it. `None`: a family name, a path that is already whole
/// or spelled with `~` or `%VAR%`, or a design with no folder yet (the path then stays as typed).
pub(super) fn whole_font(design: Option<&str>, font: &str) -> Option<String> {
    let name = font.trim();
    // A `~` or `%VAR%` spelling stays as typed, even where it does not expand (an unset variable).
    if name.starts_with('~') || name.contains('%') {
        return None;
    }
    let file = solidcraft_sketch::font_file_path(name)?;
    if file != Path::new(name) || !file.is_relative() {
        return None;
    }
    Some(absolute(&base_dir(design)?.join(&file)).to_string_lossy().into_owned())
}

/// A whole font file path as the design's file keeps it: relative to the design's folder `base`
/// when the font lies inside it, so the folder with its fonts moves as one. A font elsewhere
/// stays as written.
fn kept_font(base: Option<&Path>, font: &str) -> Option<String> {
    let file = Path::new(font.trim());
    if !file.is_absolute() {
        return None;
    }
    let base = base?;
    // The path as typed, else where it leads through folder aliases, as `base` is resolved
    // ([`link_target`]): a font typed through a linked folder is still inside the design's.
    let target = link_target(file);
    let inside = file.strip_prefix(base).or_else(|_| target.strip_prefix(base)).ok()?;
    Some(inside.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/"))
}

/// Every sketch text's font put through `map` (`None`: kept as it is). The design is copied only
/// when a font changes.
fn font_paths_through(design: &mut Cow<'_, Document>, map: impl Fn(&str) -> Option<String>) {
    let changes = design.features.iter().any(|f| match &f.kind {
        FeatureKind::Sketch { sketch, .. } => {
            sketch.links.iter().any(|l| matches!(&l.source, LinkSource::Text { font: Some(font), .. } if map(font).is_some()))
        }
        _ => false,
    });
    if !changes {
        return;
    }
    for feature in &mut design.to_mut().features {
        let FeatureKind::Sketch { sketch, .. } = &mut feature.kind else { continue };
        for link in &mut sketch.links {
            if let LinkSource::Text { font: Some(font), .. } = &mut link.source
                && let Some(new) = map(font)
            {
                *font = new;
            }
        }
    }
}

/// Every link path of `design` put through `map`, whatever its kind: its component links and its
/// drawings' sources. The design is copied only when a path changes, and each drawing only when one
/// of its own does. Opening ([`located`]) and saving ([`filed`]) a design take its paths through
/// here, and so does anything else that rewrites them (a pack's entry names).
pub(crate) fn map_paths(design: &mut Cow<'_, Document>, map: impl Fn(&str) -> String) {
    let changed = mapped(design, &map);
    if changed.0.is_some() || changed.1.is_some() {
        put(design.to_mut(), changed);
    }
}

/// Each whole file a link path of `doc`, a design saved at `from`, names, once, in the order first
/// named: its component links', then its drawings' sources'. The read-only side of [`map_paths`].
#[cfg_attr(not(test), expect(dead_code, reason = "a design pack (#9857) lists the files it packs here"))]
pub(crate) fn linked_files(doc: &Document, from: Option<&str>) -> Vec<PathBuf> {
    // A link to a design that came from the session names no file: it is not a file to pack.
    let links = doc.links.values().map(|link| link.path.as_str()).filter(|path| !path.is_empty());
    let sources = doc.drawings.iter().flat_map(|d| d.sources.iter().filter_map(|source| source.path.as_deref()));
    let mut seen = BTreeSet::new();
    links.chain(sources).map(|path| absolute(&resolve(from, path))).filter(|file| seen.insert(file.clone())).collect()
}

/// A design's component links and drawings after a map of its link paths, each `None` when none of
/// its paths changed.
type Mapped = (Option<BTreeMap<u64, DesignLink>>, Option<Vec<Arc<Drawing>>>);

/// What `map` makes of `doc`'s link paths ([`map_paths`]): the one list of the kinds of link path.
fn mapped(doc: &Document, map: &impl Fn(&str) -> String) -> Mapped {
    // A link to a design that came from the session names no file: it stays as it is.
    let links: BTreeMap<u64, DesignLink> =
        doc.links.iter().map(|(c, l)| (*c, DesignLink { path: if l.path.is_empty() { String::new() } else { map(&l.path) }, ..l.clone() })).collect();
    let mut drawings = doc.drawings.clone();
    let sources = sources_through(&mut drawings, map);
    ((links != doc.links).then_some(links), sources.then_some(drawings))
}

/// The links and drawings a map changed, put into `doc`.
fn put(doc: &mut Document, (links, drawings): Mapped) {
    if let Some(links) = links {
        doc.links = links;
    }
    if let Some(drawings) = drawings {
        doc.drawings = drawings;
    }
}

/// Each drawing source's path put through `map`, copying a drawing only when one of its paths
/// changes; whether any did.
fn sources_through(drawings: &mut [Arc<Drawing>], map: impl Fn(&str) -> String) -> bool {
    let mut changed = false;
    for drawing in drawings {
        let paths: Vec<Option<String>> = drawing.sources.iter().map(|s| s.path.as_deref().map(&map)).collect();
        if drawing.sources.iter().zip(&paths).any(|(s, path)| s.path != *path) {
            for (source, path) in Arc::make_mut(drawing).sources.iter_mut().zip(paths) {
                source.path = path;
            }
            changed = true;
        }
    }
    changed
}

/// Whether two paths name one file: the same path, or (on native hosts) the same file through a
/// symlink or a path with different case. A design can't link itself under either.
pub(super) fn same_file(a: &Path, b: &Path) -> bool {
    absolute(a) == absolute(b) || matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y)
}

/// Where a link stands against its file: the one set of state words every kind of link reports.
/// Reading tells [`LinkState::Missing`], [`LinkState::Unreadable`] and [`LinkState::NotAllowed`]
/// apart; whether a design that was read is [`LinkState::UpToDate`] or [`LinkState::OutOfDate`]
/// is the link's own comparison. A design a newer SolidCraft saved is not a state of its own: it
/// opens, and its `newer_file` says what this build couldn't read (#12166).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LinkState {
    /// The file holds what the link last took from it.
    UpToDate,
    /// The file changed since the link last took from it.
    OutOfDate,
    /// There is no file at the link's path.
    Missing,
    /// The file is there but isn't a design this build can read, or the path names a device.
    Unreadable,
    /// The file is on a network location that is not allowed, or outside the folders this agent
    /// may use.
    NotAllowed,
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
        }
    }
}

/// Why a linked design can't be read: its state, and the reason to show.
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

/// The state of an I/O error reading `shown`, with the reason naming the file once.
fn io_problem(shown: &str, e: &std::io::Error) -> LinkProblem {
    let named = |e: String| if e.starts_with(shown) { e } else { format!("{shown}: {e}") };
    match e.kind() {
        // The folder scope's refusal says which folders the agent may use.
        ErrorKind::PermissionDenied => problem(LinkState::NotAllowed, e.to_string()),
        ErrorKind::NotFound => problem(LinkState::Missing, format!("{shown}: not found")),
        _ => problem(LinkState::Unreadable, named(e.to_string())),
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

/// The bytes of the linked design at `file`, linked from the design saved at `design`, read
/// through the folders the caller may use, or why they can't be: a network location not allowed
/// or a device (judged from the path's text before anything is read), not a design or STEP file
/// name, missing, larger than the design (or STEP) size limit, or outside an agent's folders. Each with the
/// reason to show. Every kind of link reads its file here.
pub(super) fn read_linked_file(file: &Path, design: Option<&str>) -> Result<Vec<u8>, LinkProblem> {
    let shown = file.display().to_string();
    if let Some(why) = refusal(&shown, design) {
        return Err(why);
    }
    // A linked STEP file is read under the STEP size limit; every other link is a design.
    let step = super::component_link::is_step_source(&shown);
    if !step && !solidcraft_io::is_design_path(&shown) {
        return Err(problem(LinkState::Unreadable, format!("{shown}: not a .{} design", solidcraft_io::DESIGN_EXT)));
    }
    let (kind, limit) = if step {
        (solidcraft_io::limits::Kind::Step, solidcraft_io::limits::max_step_bytes())
    } else {
        (solidcraft_io::limits::Kind::Design, solidcraft_io::limits::max_design_bytes())
    };
    solidcraft_io::vfs::read_design_path(&shown, design, limit).map_err(|e| io_problem(&shown, &solidcraft_io::limits::explain(kind, e)))
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
        doc.links.insert(7, DesignLink { path: unsaved, feature: 3, ..Default::default() });
        let moved = links_filed(&doc, None, &design);
        assert_eq!(moved.get(&7).map(|l| l.path.as_str()), Some("../parts/plate.solidcraft"));
        let beside = root.join("parts").join("asm.solidcraft").to_string_lossy().into_owned();
        assert_eq!(
            links_filed(&Document { links: moved, ..Document::default() }, Some(&design), &beside).get(&7).map(|l| l.path.as_str()),
            Some("plate.solidcraft")
        );
    }

    /// The component links of `doc`, a design saved at `from`, as its file at `to` keeps them.
    fn links_filed(doc: &Document, from: Option<&str>, to: &str) -> std::collections::BTreeMap<u64, DesignLink> {
        let mut file = Cow::Borrowed(doc);
        filed(&mut file, from, to);
        file.links.clone()
    }

    /// Every link path a design holds goes through one walk, whatever its kind: its component links
    /// and its drawings' sources are made whole on open, kept relative to the folder of a save, and
    /// listed as the files they name, each file once. A design whose paths don't change is not
    /// copied.
    #[test]
    fn one_walk_takes_every_kind_of_link_path() {
        use solidcraft_doc::drawing::Source;
        use solidcraft_geom::drawing_sheet::{Orientation, PaperSize, Sheet};
        let root = absolute(Path::new("projects"));
        let design = root.join("asm").join("case.sc").to_string_lossy().into_owned();
        let mut doc = Document::default();
        doc.links.insert(7, DesignLink { path: "../parts/plate.sc".into(), feature: 3, ..Default::default() });
        let mut ids = solidcraft_doc::ids::IdSource::seeded(3890);
        let a4 = Sheet::standard(PaperSize::A4, Orientation::Landscape);
        let mut drawing = Drawing::new("Case", "en", "Sheet 1", a4, None, &Default::default(), &mut ids).unwrap();
        for path in [None, Some("../parts/plate.sc"), Some("../parts/block.sc")] {
            let id = drawing.allocate_id(&mut ids).unwrap();
            drawing.sources.push(Source { id, path: path.map(Into::into) });
        }
        drawing.sources.sort_by_key(|s| s.id);
        doc.drawings.push(drawing.into());
        let (plate, block) = (root.join("parts").join("plate.sc"), root.join("parts").join("block.sc"));
        assert_eq!(linked_files(&doc, Some(&design)), [plate.clone(), block.clone()], "as the file keeps them");
        located(&mut doc, Some(&design));
        assert_eq!(doc.links[&7].path, plate.to_string_lossy());
        assert_eq!(linked_files(&doc, None), [plate.clone(), block.clone()], "whole in memory, each file once");
        let to = root.join("asm").join("v2").join("case.sc").to_string_lossy().into_owned();
        let mut file = Cow::Borrowed(&doc);
        filed(&mut file, Some(&design), &to);
        assert_eq!(file.links[&7].path, "../../parts/plate.sc");
        let mut kept: Vec<Option<&str>> = file.drawings[0].sources.iter().map(|s| s.path.as_deref()).collect();
        kept.sort();
        assert_eq!(kept, [None, Some("../../parts/block.sc"), Some("../../parts/plate.sc")]);
        assert_eq!(linked_files(&file, Some(&to)), [plate, block], "the file names what memory does");
        let mut same = Cow::Borrowed(&doc);
        map_paths(&mut same, str::to_string);
        assert!(matches!(same, Cow::Borrowed(_)), "no path changed: nothing copied");
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

    /// A design deep in folders makes a drawing source's whole path longer than any parameter
    /// takes (4,096 bytes): the record has no cap on it, so the drawing keeps its rules, and Save As
    /// keeps the file it names, relative in the file, while the design in memory is never rewritten.
    #[test]
    fn a_deep_folder_resolves_to_a_long_absolute_path() {
        use solidcraft_doc::drawing::{Drawing, Source};
        use solidcraft_geom::drawing_sheet::{Orientation, PaperSize, Sheet};
        let deep = (0..25).fold(absolute(Path::new("projects")), |dir, i| dir.join(format!("{i:02}{}", "f".repeat(198))));
        let design = deep.join("asm").join("case.sc").to_string_lossy().into_owned();
        let mut ids = solidcraft_doc::ids::IdSource::seeded(3890);
        let a4 = Sheet::standard(PaperSize::A4, Orientation::Landscape);
        let mut drawing = Drawing::new("Case", "en", "Sheet 1", a4, None, &Default::default(), &mut ids).unwrap();
        let id = drawing.allocate_id(&mut ids).unwrap();
        drawing.sources.push(Source { id, path: Some("../parts/plate.sc".into()) });
        let mut doc = Document { drawings: vec![drawing.into()], ..Document::default() };
        located(&mut doc, Some(&design));
        let part = deep.join("parts").join("plate.sc");
        let whole = doc.drawings[0].sources[0].path.clone().unwrap();
        assert!(whole.len() > 4096, "{} bytes", whole.len());
        assert_eq!(Path::new(&whole), part);
        doc.drawings[0].validate().unwrap();
        let to = deep.join("asm").join("sub").join("case.sc").to_string_lossy().into_owned();
        let mut file = std::borrow::Cow::Borrowed(&doc);
        filed(&mut file, Some(&design), &to);
        let kept = file.drawings[0].sources[0].path.clone().unwrap();
        assert_eq!(kept, "../../parts/plate.sc");
        assert_eq!(resolve(Some(&to), &kept), part);
        assert_eq!(doc.drawings[0].sources[0].path.as_deref(), Some(whole.as_str()), "the design in memory is never rewritten");
    }

    #[cfg(unix)]
    #[test]
    fn directory_aliases_keep_relative_links_for_existing_and_missing_sources() {
        let dir = folder("directory-aliases").canonicalize().unwrap();
        let project = dir.join("projects/example");
        std::fs::create_dir_all(project.join("out")).unwrap();
        let output_alias = dir.join("output");
        let source_alias = dir.join("source");
        std::os::unix::fs::symlink(project.join("out"), &output_alias).unwrap();
        std::os::unix::fs::symlink(&project, &source_alias).unwrap();
        std::fs::write(project.join("part.sc"), b"{}").unwrap();
        for name in ["part.sc", "missing.sc", "missing-dir/part.sc"] {
            let expected = format!("../{name}");
            for design in [project.join("out/assembly.sc"), output_alias.join("assembly.sc"), output_alias.join("../out/assembly.sc")] {
                for source in [project.join(name), source_alias.join(name)] {
                    let design = design.to_str().unwrap();
                    assert_eq!(stored(Some(design), &source), expected);
                    assert_eq!(resolve(Some(design), &expected), project.join(name));
                }
            }
        }
        assert!(!project.join("missing.sc").exists());
        assert!(!project.join("missing-dir").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn source_file_symlinks_keep_their_names_through_directory_aliases() {
        let dir = folder("source-alias").canonicalize().unwrap();
        let project = dir.join("projects/example");
        std::fs::create_dir_all(project.join("out")).unwrap();
        let alias = dir.join("source");
        std::os::unix::fs::symlink(&project, &alias).unwrap();
        std::fs::write(project.join("first.sc"), b"first").unwrap();
        std::fs::write(project.join("second.sc"), b"second").unwrap();
        let current = project.join("current.sc");
        std::os::unix::fs::symlink("first.sc", &current).unwrap();
        let design = project.join("out/assembly.sc").to_string_lossy().into_owned();
        let link = stored(Some(&design), &alias.join("current.sc"));
        assert_eq!(link, "../current.sc");
        std::fs::remove_file(&current).unwrap();
        std::os::unix::fs::symlink("second.sc", &current).unwrap();
        assert_eq!(std::fs::read(resolve(Some(&design), &link)).unwrap(), b"second");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// One folder per test, with the files it needs.
    fn folder(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("solidcraft-design-link-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reading_a_linked_design_reports_its_state_and_reason() {
        let dir = folder("read");
        let good = dir.join("plate.sc");
        std::fs::write(&good, solidcraft_io::write_design(&Document::new("plate")).unwrap()).unwrap();
        let read = read_linked_file(&good, None).map(|bytes| solidcraft_io::read_design(&bytes).map(|d| d.name).ok());
        assert_eq!(read, Ok(Some("plate".to_string())));
        assert_eq!(link_presence(&good, None), None);
        let state = |file: &Path| read_linked_file(file, None).err().map(|e| (e.state, e.reason));
        // Missing: no file there (and the same without reading it).
        let missing = dir.join("gone.sc");
        assert!(matches!(state(&missing), Some((LinkState::Missing, r)) if r.contains("gone.sc")));
        assert_eq!(link_presence(&missing, None).map(|p| p.state), Some(LinkState::Missing));
        // Unreadable: neither a design nor a STEP file name, or a folder.
        std::fs::write(dir.join("part.txt"), b"ISO-10303-21;").unwrap();
        assert!(matches!(state(&dir.join("part.txt")), Some((LinkState::Unreadable, r)) if r.contains("not a .sc design")));
        // A STEP file is a link source too (Insert STEP keeps its link), read under the STEP limit.
        std::fs::write(dir.join("part.step"), b"ISO-10303-21;").unwrap();
        assert_eq!(read_linked_file(&dir.join("part.step"), None), Ok(b"ISO-10303-21;".to_vec()));
        std::fs::create_dir_all(dir.join("folder.sc")).unwrap();
        assert!(matches!(state(&dir.join("folder.sc")), Some((LinkState::Unreadable, _))));
        // A design a newer SolidCraft saved is read like any other, and opens saying so (#12166).
        let mut newer: serde_json::Value = serde_json::from_slice(&solidcraft_io::write_design(&Document::new("next")).unwrap()).unwrap();
        newer["format"] = serde_json::json!(solidcraft_doc::format::format_string(solidcraft_doc::format::FORMAT + 1));
        std::fs::write(dir.join("next.sc"), newer.to_string()).unwrap();
        let opened =
            read_linked_file(&dir.join("next.sc"), None).map(|bytes| solidcraft_io::read_design(&bytes).map(|d| d.newer_file.is_some()).ok());
        assert_eq!(opened, Ok(Some(true)));
        // Not allowed: outside the folders an agent may use, without saying whether it exists.
        let inside = dir.join("inside");
        std::fs::create_dir_all(&inside).unwrap();
        solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([inside])), || {
            assert_eq!(state(&good).map(|s| s.0), Some(LinkState::NotAllowed));
            assert_eq!(link_presence(&missing, None).map(|p| p.state), Some(LinkState::NotAllowed));
        });
        let words: Vec<&str> = [LinkState::UpToDate, LinkState::OutOfDate, LinkState::Missing, LinkState::Unreadable, LinkState::NotAllowed]
            .iter()
            .map(|s| s.word())
            .collect();
        assert_eq!(words, ["up_to_date", "out_of_date", "missing", "unreadable", "not_allowed"]);
        assert_eq!(serde_json::json!(LinkState::NotAllowed), serde_json::json!("not_allowed"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A network location not allowed, or a device, is refused from the path's text alone:
    /// reading it asks nothing of the disk or the network (#7372).
    #[test]
    fn network_and_device_paths_are_refused_before_anything_is_read() {
        let calls = solidcraft_io::vfs::file_calls();
        let network = Path::new(r"\\fileserver\share\standard.sc");
        let refused = read_linked_file(network, None).err();
        assert!(matches!(&refused, Some(p) if p.state == LinkState::NotAllowed && p.reason.contains("fileserver")), "{refused:?}");
        assert_eq!(link_presence(network, None).map(|p| p.state), Some(LinkState::NotAllowed));
        let device = Path::new(r"\\.\pipe\standard.sc");
        assert_eq!(read_linked_file(device, None).err().map(|p| p.state), Some(LinkState::Unreadable));
        assert_eq!(solidcraft_io::vfs::file_calls(), calls, "a refused path asked the file system");
    }

    #[test]
    fn a_path_and_its_alias_are_the_same_file() {
        let dir = folder("same");
        let file = dir.join("a.sc");
        std::fs::write(&file, b"{}").unwrap();
        assert!(same_file(&file, &dir.join(".").join("a.sc")));
        assert!(!same_file(&file, &dir.join("b.sc")));
        #[cfg(unix)]
        {
            let alias = dir.join("alias.sc");
            std::os::unix::fs::symlink(&file, &alias).unwrap();
            assert!(same_file(&file, &alias));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
