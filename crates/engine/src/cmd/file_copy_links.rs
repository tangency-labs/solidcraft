//! Copy Design with Links: a saved design, the files it links all the way down, and the designs in
//! the same folders whose drawings draw any of them, copied into one folder under new names. Each
//! copied design's links and drawing sources point at the copies; a file not copied (a library part)
//! keeps being linked where it is. `file.copy_with_links.list` says what there is to copy, for the
//! dialog that picks and renames; `file.copy_with_links` writes the copies. Neither changes the
//! design that is open.

use super::CommandSpec;
use crate::schema::file_copy_links::{CopyLinksListParams, CopyLinksListResult, CopyLinksParams, CopyLinksResult};
use crate::schema::{TypedContract, output_schema_for};

const LIST_CONTRACT: TypedContract<CopyLinksListParams> = TypedContract::new();
const COPY_CONTRACT: TypedContract<CopyLinksParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("file.copy_with_links", "Copy Design with Links", crate::checked!("file.copy_with_links", copy))
        .icon("copy")
        .noundo()
        .doc_only()
        .contract(&COPY_CONTRACT)
        .output(output_schema_for::<CopyLinksResult>)
        .params("path?, drawings?, folder, copy: [{path, name}]; copies the design and the linked files picked into one folder under new names, linked to each other; a file left out stays linked where it is; never replaces a file"),
    CommandSpec::new("file.copy_with_links.list", "List Files to Copy with Design", crate::checked!("file.copy_with_links.list", list))
        .icon("copy")
        .noundo()
        .doc_only()
        .contract(&LIST_CONTRACT)
        .output(output_schema_for::<CopyLinksListResult>)
        .params("path?, drawings?; the saved design, every file it links all the way down, and the designs beside them whose drawings draw one"),
];

#[cfg(target_arch = "wasm32")]
fn list(_: &mut crate::Session, _: &serde_json::Value) -> crate::Result<serde_json::Value> {
    Err(crate::params::bad("file.copy_with_links.list", "Copying a design with its linked files needs the desktop app"))
}

#[cfg(target_arch = "wasm32")]
fn copy(_: &mut crate::Session, _: &serde_json::Value) -> crate::Result<serde_json::Value> {
    Err(crate::params::bad("file.copy_with_links", "Copying a design with its linked files needs the desktop app"))
}

#[cfg(not(target_arch = "wasm32"))]
use native::{copy, list};

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::borrow::Cow;
    use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
    use std::path::{Path, PathBuf};

    use serde_json::Value;
    use solidcraft_doc::{DesignLink, Document};

    use super::super::design_link::{absolute, filed, located, map_paths, resolve};
    use crate::params::bad;
    use crate::schema::file_copy_links::{
        CopyLinksCopied, CopyLinksItem, CopyLinksListParams, CopyLinksListResult, CopyLinksParams, CopyLinksResult,
    };
    use crate::{EngineError, Result, Session};

    /// At most this many files are listed: a design linking more is refused.
    const MAX_FILES: usize = 2000;
    /// At most this many designs are read looking for drawings, over all the folders searched.
    const MAX_SCANNED: usize = 2000;

    const LIST: &str = "file.copy_with_links.list";
    const COPY: &str = "file.copy_with_links";

    const DESIGN: &str = "design";
    const DRAWING: &str = "drawing";
    const FILE: &str = "file";

    fn whole(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    /// The design to start from: `path`, or the open design's file when it has no unsaved changes.
    fn start(s: &Session, cmd: &str, path: Option<&str>) -> Result<PathBuf> {
        let path = match path {
            Some(p) => p,
            None => {
                let own = s.path.as_deref().ok_or_else(|| bad(cmd, "Save the design first: the copies are made from its file"))?;
                if s.is_dirty() {
                    return Err(bad(cmd, "Save the design's changes first: the copies are made from its file"));
                }
                own
            }
        };
        if path.trim().is_empty() || path.contains('\0') || !solidcraft_io::is_design_path(path) {
            return Err(bad(cmd, format!("`{path}` is not a design file")));
        }
        Ok(absolute(Path::new(path)))
    }

    /// A design file's bytes and its design, or why it can't be read.
    fn read_design(path: &Path) -> std::result::Result<(Vec<u8>, Document), String> {
        let shown = whole(path);
        let bytes = solidcraft_io::vfs::read_design_path(&shown, None, solidcraft_io::limits::max_design_bytes()).map_err(|e| e.to_string())?;
        let doc = solidcraft_io::read_design(&bytes).map_err(|e| e.to_string())?;
        Ok((bytes, doc))
    }

    /// The files `doc`, saved at `from`, names, each with its kind: its component links' (a linked
    /// STEP or importer's file is a `file`), then its drawings' sources (designs).
    fn named(doc: &Document, from: &Path) -> Vec<(PathBuf, &'static str)> {
        let from = whole(from);
        let file = |link: &DesignLink| link.step_names.is_some() || link.importer.is_some() || !solidcraft_io::is_design_path(&link.path);
        let links = doc.links.values().filter(|l| !l.path.is_empty()).map(|l| (l.path.as_str(), if file(l) { FILE } else { DESIGN }));
        let sources = doc.drawings.iter().flat_map(|d| d.sources.iter().filter_map(|s| s.path.as_deref())).map(|p| (p, DESIGN));
        links.chain(sources).map(|(p, kind)| (absolute(&resolve(Some(&from), p)), kind)).collect()
    }

    /// What there is to copy.
    struct Found {
        items: Vec<CopyLinksItem>,
        skipped: Vec<String>,
    }

    impl Found {
        fn add(&mut self, index: &mut HashMap<PathBuf, usize>, file: PathBuf, kind: &str, by: &Path) -> Result<Option<usize>> {
            let by = whole(by);
            if let Some(&i) = index.get(&file) {
                if !self.items[i].used_by.contains(&by) && self.items[i].path != by {
                    self.items[i].used_by.push(by);
                }
                return Ok(None);
            }
            if self.items.len() >= MAX_FILES {
                return Err(bad(LIST, format!("the design links more than {MAX_FILES} files")));
            }
            index.insert(file.clone(), self.items.len());
            let used_by = if by.is_empty() { Vec::new() } else { vec![by] };
            self.items.push(CopyLinksItem { path: whole(&file), kind: kind.into(), used_by, problem: None });
            Ok(Some(self.items.len() - 1))
        }
    }

    /// The design at `top`, every file it links all the way down, and (with `drawings`) the designs
    /// in the folders of the listed designs whose drawings draw one of them, with their own links.
    fn find(top: &Path, drawings: bool) -> Result<Found> {
        let mut found = Found { items: Vec::new(), skipped: Vec::new() };
        let mut index = HashMap::new();
        let mut queue = VecDeque::new();
        queue.extend(found.add(&mut index, top.to_path_buf(), DESIGN, Path::new(""))?);
        let mut searched = HashSet::new();
        let mut scanned = 0usize;
        loop {
            while let Some(i) = queue.pop_front() {
                if found.items[i].kind == FILE {
                    continue;
                }
                let at = PathBuf::from(&found.items[i].path);
                match read_design(&at) {
                    Err(why) => found.items[i].problem = Some(why),
                    Ok((_, doc)) => {
                        for (file, kind) in named(&doc, &at) {
                            queue.extend(found.add(&mut index, file, kind, &at)?);
                        }
                    }
                }
            }
            if !drawings {
                break;
            }
            // The folders of the designs listed so far that haven't been looked through.
            let folders: BTreeSet<PathBuf> = found
                .items
                .iter()
                .filter(|item| item.kind != FILE && item.problem.is_none())
                .filter_map(|item| Path::new(&item.path).parent().map(Path::to_path_buf))
                .filter(|folder| !searched.contains(folder))
                .collect();
            if folders.is_empty() {
                break;
            }
            for folder in folders {
                searched.insert(folder.clone());
                if solidcraft_io::vfs::check(&whole(&folder)).is_err() {
                    continue;
                }
                let Ok(entries) = std::fs::read_dir(&folder) else { continue };
                let mut files: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
                files.sort();
                for file in files {
                    let file = absolute(&file);
                    if index.contains_key(&file) || !solidcraft_io::is_design_path(&whole(&file)) || !file.is_file() {
                        continue;
                    }
                    if scanned >= MAX_SCANNED {
                        found.skipped.push(whole(&file));
                        continue;
                    }
                    scanned += 1;
                    let Ok((_, doc)) = read_design(&file) else {
                        found.skipped.push(whole(&file));
                        continue;
                    };
                    let drawn: Vec<PathBuf> = doc
                        .drawings
                        .iter()
                        .flat_map(|d| d.sources.iter().filter_map(|s| s.path.as_deref()))
                        .map(|p| absolute(&resolve(Some(&whole(&file)), p)))
                        .filter(|p| index.get(p).is_some_and(|&i| found.items[i].kind != FILE))
                        .collect();
                    let Some(first) = drawn.first() else { continue };
                    if let Some(i) = found.add(&mut index, file.clone(), DRAWING, first)? {
                        // It is listed as used by what it draws, so unticking a part shows its drawing.
                        for other in &drawn[1..] {
                            let other = whole(other);
                            if !found.items[i].used_by.contains(&other) {
                                found.items[i].used_by.push(other);
                            }
                        }
                        queue.push_back(i);
                    }
                }
            }
        }
        Ok(found)
    }

    pub(super) fn list(s: &mut Session, p: &Value) -> Result<Value> {
        let params: CopyLinksListParams = serde_json::from_value(p.clone()).map_err(|e| bad(LIST, e.to_string()))?;
        let top = start(s, LIST, params.path.as_deref())?;
        let found = find(&top, params.drawings.unwrap_or(true))?;
        if let Some(why) = found.items.first().and_then(|i| i.problem.clone()) {
            return Err(bad(LIST, format!("{}: {why}", whole(&top))));
        }
        let reply = CopyLinksListResult { design: whole(&top), items: found.items, skipped: found.skipped, param_warnings: Vec::new() };
        serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
    }

    /// Whether `name` can be a copy's file name: one name, no folder, nothing a file name can't hold.
    fn plain_name(name: &str) -> bool {
        let reserved = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
        !name.trim().is_empty()
            && name.len() <= 255
            && name != "."
            && name != ".."
            && !name.ends_with(['.', ' '])
            && !name.chars().any(|c| c.is_control() || reserved.contains(&c))
    }

    fn extension(path: &str) -> String {
        Path::new(path).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
    }

    /// One file to write: where from, where to, its bytes as read and as written.
    struct Write {
        from: PathBuf,
        to: PathBuf,
        original: Vec<u8>,
        contents: Vec<u8>,
    }

    pub(super) fn copy(s: &mut Session, p: &Value) -> Result<Value> {
        let params: CopyLinksParams = serde_json::from_value(p.clone()).map_err(|e| bad(COPY, e.to_string()))?;
        let top = start(s, COPY, params.path.as_deref())?;
        let folder = absolute(Path::new(&params.folder));
        if params.folder.trim().is_empty() || params.folder.contains('\0') || !folder.is_dir() {
            return Err(bad(COPY, format!("`{}` is not a folder", params.folder)));
        }
        solidcraft_io::vfs::check(&whole(&folder)).map_err(|e| bad(COPY, e.to_string()))?;
        let found = find(&top, params.drawings.unwrap_or(true))?;
        let listed: HashMap<PathBuf, &CopyLinksItem> = found.items.iter().map(|i| (PathBuf::from(&i.path), i)).collect();

        // Every pick checked, and every destination free, before anything is written.
        let mut dest: HashMap<PathBuf, PathBuf> = HashMap::new();
        let mut names = HashSet::new();
        for pick in &params.copy {
            let from = absolute(Path::new(&pick.path));
            let Some(item) = listed.get(&from) else {
                return Err(bad(COPY, format!("`{}` is not one of the design's files (file.copy_with_links.list)", pick.path)));
            };
            if let Some(why) = &item.problem {
                return Err(bad(COPY, format!("`{}` can't be copied: {why}", item.path)));
            }
            if !plain_name(&pick.name) {
                return Err(bad(COPY, format!("`{}` is not a file name", pick.name)));
            }
            let fits = if item.kind == FILE { extension(&pick.name) == extension(&item.path) } else { solidcraft_io::is_design_path(&pick.name) };
            if !fits {
                let keep = if item.kind == FILE { format!(".{}", extension(&item.path)) } else { format!(".{}", solidcraft_io::DESIGN_EXT) };
                return Err(bad(COPY, format!("`{}`: keep the {keep} extension", pick.name)));
            }
            if !names.insert(pick.name.to_lowercase()) {
                return Err(bad(COPY, format!("two copies are named `{}`", pick.name)));
            }
            let to = folder.join(&pick.name);
            if std::fs::symlink_metadata(&to).is_ok() {
                return Err(bad(COPY, format!("`{}` already exists: nothing is replaced", whole(&to))));
            }
            if dest.insert(from, to).is_some() {
                return Err(bad(COPY, format!("`{}` is picked twice", pick.path)));
            }
        }
        if !dest.contains_key(&top) {
            return Err(bad(COPY, format!("the design itself (`{}`) must be copied", whole(&top))));
        }

        // Each copied design after the copied files it names, so their new digests are known.
        let mut docs: HashMap<PathBuf, (Vec<u8>, Document)> = HashMap::new();
        for (from, _) in dest.iter().filter(|(from, _)| listed.get(*from).is_some_and(|i| i.kind != FILE)) {
            let read = read_design(from).map_err(|why| bad(COPY, format!("{}: {why}", whole(from))))?;
            docs.insert(from.clone(), read);
        }
        let mut order = Vec::new();
        let mut done = HashSet::new();
        let mut froms: Vec<&PathBuf> = dest.keys().collect();
        froms.sort();
        for from in froms {
            visit(from, &dest, &docs, &mut done, &mut Vec::new(), &mut order)?;
        }

        // Each copy's bytes, and each copied file's digest before and after.
        let mut digests: HashMap<PathBuf, (String, String)> = HashMap::new();
        let mut writes = Vec::new();
        for from in order {
            let to = dest.get(&from).cloned().unwrap_or_default();
            let (original, contents) = if let Some((bytes, doc)) = docs.get(&from) {
                let mut doc = doc.clone();
                located(&mut doc, Some(&whole(&from)));
                for link in doc.links.values_mut() {
                    if let Some((before, after)) = digests.get(&absolute(Path::new(&link.path)))
                        && link.file_sha256.as_deref() == Some(before.as_str())
                    {
                        link.file_sha256 = Some(after.clone());
                    }
                }
                let mut doc = Cow::Owned(doc);
                map_paths(&mut doc, |path| dest.get(&absolute(Path::new(path))).map(|to| whole(to)).unwrap_or_else(|| path.to_string()));
                let at = whole(&to);
                filed(&mut doc, Some(&at), &at);
                let contents =
                    if extension(&at) == "json" { solidcraft_io::write_design(&doc)? } else { solidcraft_io::write_design_container(&doc)? };
                (bytes.clone(), contents)
            } else {
                let bytes = solidcraft_io::vfs::read_design_path(&whole(&from), None, solidcraft_io::limits::max_step_bytes())
                    .map_err(|e| bad(COPY, format!("{}: {e}", whole(&from))))?;
                (bytes.clone(), bytes)
            };
            digests.insert(from.clone(), (DesignLink::file_digest(&original), DesignLink::file_digest(&contents)));
            writes.push(Write { from, to, original, contents });
        }

        let mut written: Vec<PathBuf> = Vec::new();
        for w in &writes {
            if let Err(e) = solidcraft_io::library_files::transfer(&whole(&w.from), &whole(&w.to), &w.original, &w.contents, false) {
                // Only copies this call made are removed: each was a new file.
                for made in &written {
                    let _ = std::fs::remove_file(made);
                }
                return Err(EngineError::Other(format!("{}: {e}; no copies were kept", whole(&w.to))));
            }
            written.push(w.to.clone());
        }
        let kept = found.items.iter().filter(|i| !dest.contains_key(Path::new(&i.path))).map(|i| i.path.clone()).collect();
        let copied = writes.iter().map(|w| CopyLinksCopied { from: whole(&w.from), to: whole(&w.to) }).collect();
        let reply = CopyLinksResult { design: dest.get(&top).map(|t| whole(t)).unwrap_or_default(), copied, kept, param_warnings: Vec::new() };
        serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
    }

    /// `from` into `order` after every copied file its design names; a loop of designs naming each
    /// other is refused.
    fn visit(
        from: &PathBuf,
        dest: &HashMap<PathBuf, PathBuf>,
        docs: &HashMap<PathBuf, (Vec<u8>, Document)>,
        done: &mut HashSet<PathBuf>,
        path: &mut Vec<PathBuf>,
        order: &mut Vec<PathBuf>,
    ) -> Result<()> {
        if done.contains(from) {
            return Ok(());
        }
        if path.contains(from) {
            return Err(bad(COPY, format!("`{}` links back to itself through the designs it links", whole(from))));
        }
        path.push(from.clone());
        if let Some((_, doc)) = docs.get(from) {
            let mut named: Vec<PathBuf> = named(doc, from).into_iter().map(|(file, _)| file).filter(|f| dest.contains_key(f) && f != from).collect();
            named.sort();
            named.dedup();
            for file in &named {
                visit(file, dest, docs, done, path, order)?;
            }
        }
        path.pop();
        done.insert(from.clone());
        order.push(from.clone());
        Ok(())
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "file_copy_links_tests.rs"]
mod tests;
