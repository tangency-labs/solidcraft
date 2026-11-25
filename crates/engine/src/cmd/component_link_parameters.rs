//! Linked designs driven by this design's parameters (`component.link_parameters`).
//!
//! A link can set user parameters of its design, each an expression over this design's
//! parameters (`DesignLink::parameters`). The linked bodies are the design's with those values,
//! and the link keeps the values it last gave (`DesignLink::parameter_values`, each as text in the
//! parameter's own unit). After any value edit ([`Session::settle_value_edit`]), and after any
//! other edit that changes what a link is passed ([`follow_command`]: a feature input an
//! expression reads, a rename), [`drive_links`] works each expression out again; a link whose
//! values changed takes the design's bodies with the new ones, and their materials and looks, in
//! the same undo step. A value edit never takes in a newer version of the linked file:
//! when the file changed since this design last read it, the link keeps its bodies and says why
//! (Update Links is the one step that takes in a change to the file).
//!
//! Each value has to rebuild the linked design. So that dragging a value stays quick, the linked
//! designs evaluated last stay open ([`SOURCES`]): a new value then rebuilds only the features
//! that use it. A design whose rules switch features' suppression, or whose own links are driven,
//! goes back to the design its file holds before each drive ([`reset_source`]), so it gives what
//! a new read gives. An open
//! design is taken out of the list while it is used, so a linked design that
//! links another one (its own links driven by the values it was given) takes that one in turn.
//!
//! The first time a linked design is evaluated, its bodies go to this design as STEP text, read
//! back as a STEP import is: that read names the bodies and carries the design's face names,
//! materials and looks onto them. Reading the text costs far more than the rebuild, so a later
//! drive whose bodies and face names match that read one for one hands over the design's own
//! bodies as B-rep records instead ([`live_bodies`]): the Import feature keeps the records
//! (`FeatureKind::Import::records`), and the kernel makes the bodies of them
//! (`records_import_shared`) for the drive and again for every evaluation and every opening of
//! the saved design, so the bodies a drive gives are the ones the design reopens with. A drive
//! that doesn't match reads STEP text as the first did.
//!
//! A linked design's own value edit reports only its own links ([`with_own_report`]). A design
//! that links one that links it back (by file, at any depth) is a loop: the link that closes it
//! keeps its bodies and says so ([`DRIVING`]). Apart from loops, links inside links follow to
//! [`MAX_DEPTH`] levels, a limit that only guards the window thread's stack.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{Value, json};
use solidcraft_doc::expr::{self, Kind};
use solidcraft_doc::{DesignLink, FeatureKind};

use super::{Linked, SelectedConfiguration, component_key, digest, inherit_properties, read_linked, reply, selected_configuration, session_bodies};
use crate::params::bad;
use crate::schema::component_link::{DesignParameter, DesignParametersResult, LinkParametersResult, LinkedParameter};
use crate::{EngineError, Result, Session};

/// Linked designs kept open, each as last evaluated, oldest first out.
static SOURCES: Mutex<Vec<Source>> = Mutex::new(Vec::new());
const SOURCES_SIZE: usize = 6;

/// How deep linked designs may drive the designs they link in turn: each level is a design
/// evaluated inside the one above it, on this thread's stack.
const MAX_DEPTH: u32 = 32;

/// The number user parameters of linked designs, by file, the digest the link recorded and
/// configuration, for finding the names a by-name link could newly take ([`settable_names`]).
static NAMES: Mutex<Vec<(NamesKey, std::sync::Arc<Vec<SettableName>>)>> = Mutex::new(Vec::new());
const NAMES_SIZE: usize = 32;

type NamesKey = (PathBuf, Option<String>, Option<u64>);

/// A number user parameter of a linked design: its name, kind and unit.
type SettableName = (String, Kind, String);

thread_local! {
    /// How many linked designs this thread is evaluating, one inside another.
    static DEPTH: Cell<u32> = const { Cell::new(0) };
    /// The design files whose links this thread is driving, outermost first: a link to one of
    /// them closes a loop.
    static DRIVING: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
    /// What the last value edits on this thread drove: the components that took new bodies and
    /// the ones that kept theirs, with why. `parameters.change` reports and clears them.
    static REPORT: RefCell<(Vec<String>, Vec<String>)> = const { RefCell::new((Vec::new(), Vec::new())) };
    /// The links that kept their bodies though the values they take changed (a file gone or
    /// changed, a loop, or a design inside that could not follow), each said as in the report.
    /// A linked design's own value edit is read for them ([`collect_kept`]): its bodies then
    /// don't have the values it was given.
    static KEPT: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// While a plugin's command runs: the only folders the links it drives are read from
    /// ([`reading_links_in`]).
    static LINK_READS: RefCell<Option<solidcraft_io::vfs::PathScope>> = const { RefCell::new(None) };
}

/// A linked design kept open.
struct Source {
    file: PathBuf,
    /// [`digest`] of the file's bytes it was opened from.
    bytes: u64,
    configuration: Option<SelectedConfiguration>,
    validation: solidcraft_kernel::Validation,
    session: Session,
    /// The design's own expression of each parameter a link has set in this session.
    original: BTreeMap<String, String>,
    /// The first STEP read of the design's bodies, while they keep its bodies and face names one
    /// for one, for [`live_bodies`].
    template: Option<Template>,
    /// The design as its file holds it (before its configuration is activated), for a design
    /// whose rules switch features' suppression ([`switches_suppression`]) or whose own links are
    /// driven: each drive after the first starts from it ([`reset_source`]).
    saved: Option<std::sync::Arc<solidcraft_doc::Document>>,
    /// Whether a drive has used it since it was opened.
    driven: bool,
}

/// What driving a linked design gave.
enum Drove {
    /// Its bodies with the values given, the value each parameter was given, and the design's
    /// number user parameters with their units ([`DesignLink::parameter_units`]).
    Bodies(Box<(Linked, BTreeMap<String, String>, BTreeMap<String, String>)>),
    /// A link inside it kept its bodies, so its bodies don't have these values: each such link,
    /// as it said why. The link driving it keeps its own bodies and values too.
    Kept(Vec<String>),
}

/// A STEP read of a linked design's bodies that gave each of them back, in order, with its faces
/// in order: while the design keeps those bodies and face names, its live bodies are handed over
/// as records in the read's place, with the read's names, materials and looks.
struct Template {
    /// Each solid body's name and face names when it was read.
    faces: Vec<(String, std::sync::Arc<Vec<String>>)>,
    linked: Linked,
}

#[cfg(test)]
thread_local! {
    /// The times this thread's links handed over live bodies as records in place of a new read.
    static RECORD_READS: Cell<usize> = const { Cell::new(0) };
}

thread_local! {
    /// The next drive on this thread also gives the design's images as it leaves them
    /// ([`link_bodies_with_images`]); the drive takes the flag, so a design inside does not.
    static WANT_IMAGES: Cell<bool> = const { Cell::new(false) };
    /// Those images, from the evaluation that made the bodies.
    static DRIVEN_IMAGES: RefCell<Option<super::LinkedImages>> = const { RefCell::new(None) };
}

/// The components the value edits on this thread drove since the last call, and the warnings
/// of those that kept their bodies; both are cleared.
pub(crate) fn take_drive_report() -> (Vec<String>, Vec<String>) {
    REPORT.with(|r| std::mem::take(&mut *r.borrow_mut()))
}

/// Runs `f`, a linked design's own value edit, with this thread's report set aside: the edit
/// reports its own links only, and the edit that runs it keeps what it had noted. Without this,
/// the linked design's `parameters.change` would take the outer edit's report as its own.
fn with_own_report<T>(f: impl FnOnce() -> T) -> T {
    struct Restore(Option<(Vec<String>, Vec<String>)>);
    impl Drop for Restore {
        fn drop(&mut self) {
            if let Some(outer) = self.0.take() {
                REPORT.with(|r| *r.borrow_mut() = outer);
            }
        }
    }
    let _restore = Restore(Some(take_drive_report()));
    f()
}

/// Marks the file `s` is saved at as driving its links for as long as it lives.
struct Driving(bool);

impl Driving {
    fn enter(s: &Session) -> Driving {
        match s.path.as_deref() {
            Some(path) => {
                DRIVING.with(|d| d.borrow_mut().push(super::absolute(Path::new(path))));
                Driving(true)
            }
            None => Driving(false),
        }
    }
}

impl Drop for Driving {
    fn drop(&mut self) {
        if self.0 {
            DRIVING.with(|d| d.borrow_mut().pop());
        }
    }
}

/// The loop `file` would close, outermost design first, ending with `file`: `None` when no
/// design driving its links on this thread is `file`.
fn loop_through(file: &Path) -> Option<String> {
    DRIVING.with(|d| {
        let d = d.borrow();
        let at = d.iter().position(|x| x == file)?;
        let names: Vec<String> = d[at..]
            .iter()
            .chain([&file.to_path_buf()])
            .map(|x| x.file_name().map_or_else(|| x.display().to_string(), |n| n.to_string_lossy().to_string()))
            .collect();
        Some(names.join(" → "))
    })
}

fn note_driven(name: String) {
    REPORT.with(|r| r.borrow_mut().0.push(name));
}

fn note_warning(text: String) {
    REPORT.with(|r| r.borrow_mut().1.push(text));
}

/// A link that keeps its bodies though its values changed: said in the report, and noted for a
/// design this one is linked into ([`collect_kept`]).
fn note_kept(text: String) {
    KEPT.with(|k| k.borrow_mut().push(text.clone()));
    note_warning(text);
}

/// Forget what earlier value edits on this thread kept (a new value edit starts).
pub(crate) fn forget_kept() {
    KEPT.with(|k| k.borrow_mut().clear());
}

/// Runs `f`, a linked design's own value edit, and gives back with its result the links it
/// kept ([`note_kept`]); what the edit around it noted is left as it was.
fn collect_kept<T>(f: impl FnOnce() -> T) -> (T, Vec<String>) {
    let outer = KEPT.with(|k| std::mem::take(&mut *k.borrow_mut()));
    let out = f();
    let kept = KEPT.with(|k| std::mem::replace(&mut *k.borrow_mut(), outer));
    (out, kept)
}

/// The text a link gives a parameter of unit `unit` for `v` (mm, radians or a number): the
/// number in that unit, then the unit, so the design reads it back as the same value.
fn value_text(v: f64, unit: &str) -> String {
    match expr::unit_info(unit) {
        Some((Kind::Length | Kind::Angle, scale)) if scale > 0.0 => format!("{} {}", v / scale, unit.trim()),
        _ => format!("{v}"),
    }
}

/// The unit of a value's text (`12 mm`, `30 deg`, `3`), and its kind.
fn stored_unit(text: &str) -> (&str, Kind) {
    let unit = text.trim().rsplit_once(' ').map_or("", |(_, u)| u);
    (unit, expr::unit_info(unit).map_or(Kind::Unitless, |(k, _)| k))
}

/// The number a reply shows for a value's text: mm, degrees or the number.
fn reported(text: &str) -> Option<f64> {
    let v: f64 = text.split_whitespace().next()?.parse().ok()?;
    let (unit, _) = stored_unit(text);
    let v = match expr::unit_info(unit) {
        Some((Kind::Angle, scale)) => (v * scale).to_degrees(),
        Some((Kind::Length, scale)) => v * scale,
        _ => v,
    };
    Some(expr::round_reported(v))
}

/// This design's parameter values, to work out a link's expressions in.
struct Host {
    values: BTreeMap<String, expr::Value>,
    /// The names of this design's user parameters, for a link that sets parameters by name.
    user: std::collections::BTreeSet<String>,
    doc_len: f64,
}

impl Host {
    fn of(s: &Session) -> Host {
        let doc_len = expr::unit_info(s.doc.units.trim()).filter(|(k, _)| *k == Kind::Length).map(|(_, s)| s).unwrap_or(1.0);
        let user = s.doc.params.iter().filter(|p| !p.model).map(|p| p.name.clone()).collect();
        Host { values: s.doc.param_values().0, user, doc_len }
    }

    /// `e` worked out as a value of `kind` in a parameter of unit `unit`, as its text.
    fn value(&self, e: &str, kind: Kind, unit: &str) -> std::result::Result<String, String> {
        let defaults = expr::Defaults::for_unit(unit, self.doc_len);
        let lookup = |n: &str| self.values.get(n).copied().ok_or_else(|| solidcraft_doc::DocError::Expr(format!("unknown parameter `{n}`")));
        expr::eval_with_defaults(e, &lookup, defaults)
            .and_then(|v| v.to_kind_in(kind, defaults))
            .map(|v| value_text(v, unit))
            .map_err(|err| err.to_string().trim_start_matches("expression: ").to_string())
    }

    /// This design's parameter `name` as a value of `kind`, for a link that takes it by name.
    /// The two must be of one kind: a number here does not set a length or an angle there,
    /// where an expression's unit-less result is read in the parameter's unit ([`Host::value`]).
    fn named(&self, name: &str, kind: Kind, unit: &str) -> std::result::Result<String, String> {
        if let Some(v) = self.values.get(name)
            && v.kind() != Some(kind)
        {
            let wanted = match kind {
                Kind::Length => "a length",
                Kind::Angle => "an angle",
                Kind::Unitless => "a number",
            };
            return Err(format!("expected {wanted}, got {}", v.describe()));
        }
        self.value(name, kind, unit)
    }
}

/// Counts one more linked design being evaluated on this thread for as long as it lives.
struct Nested;

impl Nested {
    fn enter() -> Result<Nested> {
        let depth = DEPTH.with(Cell::get);
        if depth >= MAX_DEPTH {
            return Err(EngineError::Other(format!(
                "linked designs drive each other more than {MAX_DEPTH} deep: check that no design links one that links it back"
            )));
        }
        DEPTH.with(|d| d.set(depth + 1));
        Ok(Nested)
    }
}

impl Drop for Nested {
    fn drop(&mut self) {
        DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

fn take_source(file: &Path, bytes: u64, configuration: Option<SelectedConfiguration>, validation: solidcraft_kernel::Validation) -> Option<Source> {
    let mut sources = SOURCES.lock().ok()?;
    let at = sources.iter().position(|x| x.file == file && x.bytes == bytes && x.configuration == configuration && x.validation == validation)?;
    Some(sources.remove(at))
}

fn put_source(source: Source) {
    let Ok(mut sources) = SOURCES.lock() else { return };
    sources.retain(|x| !(x.file == source.file && x.configuration == source.configuration && x.validation == source.validation));
    if sources.len() >= SOURCES_SIZE {
        sources.remove(0);
    }
    sources.push(source);
}

/// The design in `bytes` (the file at `file`), and with `geometry` the geometry it was saved
/// with (left unread otherwise).
fn read_document(file: &Path, bytes: &[u8], geometry: bool) -> Result<(solidcraft_doc::Document, Option<solidcraft_doc::SavedModel>)> {
    let shown = file.display().to_string();
    let fail = |e: solidcraft_io::IoError| EngineError::Other(format!("{shown}: {e}"));
    if solidcraft_io::is_design_container(bytes) {
        let mut reader = solidcraft_io::DesignContainerReader::new(std::io::Cursor::new(bytes)).map_err(fail)?;
        let saved = if geometry { reader.geometry() } else { None };
        Ok((reader.read_document().map_err(fail)?, saved))
    } else {
        Ok((solidcraft_io::read_design(bytes).map_err(fail)?, None))
    }
}

fn open_source(file: &Path, bytes: &[u8], configuration: Option<SelectedConfiguration>, validation: solidcraft_kernel::Validation) -> Result<Source> {
    let shown = file.display().to_string();
    let (doc, saved) = read_document(file, bytes, true)?;
    if let Some(selected) = configuration
        && doc.configs.row_identity(selected.row) != selected.identity
    {
        return Err(EngineError::Other(format!("{shown}: configuration `{}` was replaced; select a configuration again", selected.row)));
    }
    let mut session = Session::opened_at_validating(doc, saved.as_ref(), Some(&shown), validation);
    // The design as its file holds it, before a configuration is activated: activating one
    // drives the links inside, with their files as they are now ([`reset_source`]).
    let saved = (switches_suppression(&session.doc) || session.doc.links.values().any(drives)).then(|| session.doc.clone());
    if let Some(selected) = configuration {
        session.execute("config.activate", &json!({"row": selected.row})).map_err(|e| EngineError::Other(format!("{shown}: {e}")))?;
    }
    Ok(Source {
        file: file.to_path_buf(),
        bytes: digest(bytes),
        configuration,
        validation,
        session,
        original: BTreeMap::new(),
        template: None,
        saved,
        driven: false,
    })
}

/// The bodies of the design in `bytes` (the file at `file`) with the user parameters `link` sets
/// worked out in `host`, and the value each was given. `strict` (the link's settings are being
/// set): a kept parameter must be one the link could set.
fn driven_bodies(
    file: &Path,
    bytes: &[u8],
    configuration: Option<SelectedConfiguration>,
    link: &DesignLink,
    strict: bool,
    host: &Host,
    validation: solidcraft_kernel::Validation,
) -> Result<Drove> {
    let images = WANT_IMAGES.with(|w| w.replace(false));
    let _nested = Nested::enter()?;
    let file = super::absolute(file);
    let shown = file.display().to_string();
    let mut source = match take_source(&file, digest(bytes), configuration, validation) {
        Some(source) => source,
        None => open_source(&file, bytes, configuration, validation)?,
    };
    let reset = reset_source(&mut source, &shown)?;
    let result = drive_source(&mut source, &shown, link, strict, host, reset);
    // A design whose links could not follow its values is left half driven: it is not kept, so
    // the next drive starts again from the file (and finds whether the links can follow now).
    if matches!(result, Ok(Drove::Bodies(..))) {
        // The images of this evaluation, before the design goes back where another drive of the
        // same file can take it and give it other values.
        if images {
            DRIVEN_IMAGES.with(|i| *i.borrow_mut() = Some(super::design_images(&source.session, &shown)));
        }
        put_source(source);
    }
    result
}

/// Put a kept design back to the design its file holds (`Source::saved`) before a drive, when
/// what a drive gives would otherwise depend on the drives before it:
/// - its rules switch features' suppression: a rule switches a feature only when the value it
///   reads changes, and in one direction;
/// - it drives links of its own: the open copy holds what they gave another placement's values,
///   read when their files were as they were then, so a drive asking those values again would
///   reuse them without meeting a file now gone or changed.
///
/// The drive then sets its values on the file's design, as a new read of it does: the rules see
/// the same changes, and the links inside are driven from the file's values, reading their files
/// as they are now. The rebuild still redoes only the features that differ from the last one,
/// and the bodies are still handed over as records. A configured design takes its configuration
/// again (`config.activate`, which drives its links inside as on a new read). Returns whether
/// the design was put back and is not evaluated for it yet.
fn reset_source(source: &mut Source, shown: &str) -> Result<bool> {
    let first = !std::mem::replace(&mut source.driven, true);
    let Some(saved) = &source.saved else { return Ok(false) };
    if first {
        return Ok(false);
    }
    source.session.share_doc(saved.clone());
    let Some(selected) = source.configuration else { return Ok(true) };
    // Its configuration again, as a new read activates it. What its links inside keep is found
    // by the drive ([`stale_links`]), as after a new read.
    let (activated, _kept) = collect_kept(|| with_own_report(|| source.session.execute("config.activate", &json!({"row": selected.row}))));
    activated.map_err(|e| EngineError::Other(format!("{shown}: {e}")))?;
    Ok(false)
}

/// Whether `doc` has a configuration rule that switches a feature's suppression when a value
/// changes (`Document::apply_suppression_rules`).
fn switches_suppression(doc: &solidcraft_doc::Document) -> bool {
    doc.configs.rules.iter().any(|r| r.enabled && matches!(r.effect, solidcraft_doc::config::RuleEffect::DriveSuppression { .. }))
}

/// Whether `param` is one a link can set: a number user parameter.
fn settable(doc: &solidcraft_doc::Document, param: &solidcraft_doc::Param) -> bool {
    !param.model && !solidcraft_doc::text_params::is_text_unit(&param.unit) && doc.text_param(&param.name).is_none()
}

/// `reset`: the design was just put back to its file's ([`reset_source`]) and is not evaluated
/// for it yet.
fn drive_source(source: &mut Source, shown: &str, link: &DesignLink, strict: bool, host: &Host, reset: bool) -> Result<Drove> {
    let parameters = &link.parameters;
    let kept = &link.parameters_kept;
    if strict {
        let doc = &source.session.doc;
        if let Some(name) = kept.iter().find(|name| !doc.param(name).is_some_and(|p| settable(doc, p))) {
            return Err(EngineError::Other(format!("{shown} has no number user parameter `{name}` to keep")));
        }
    }
    // Each parameter to set: its name, its expression, and whether it is set by its name.
    let mut wanted: Vec<(String, String, bool)> = parameters.iter().map(|(n, e)| (n.clone(), e.clone(), false)).collect();
    if link.parameters_by_name {
        let doc = &source.session.doc;
        wanted.extend(
            doc.params
                .iter()
                .filter(|p| settable(doc, p) && !parameters.contains_key(&p.name) && !kept.contains(&p.name) && host.user.contains(&p.name))
                .map(|p| (p.name.clone(), p.name.clone(), true)),
        );
    }
    let mut applied = BTreeMap::new();
    // What the drive has to say: a parameter that keeps its own value, and what the linked
    // design's own links kept. It goes to the reply of the command that set the link going
    // (with the link's bodies) and to this thread's report, for a value edit's `link_warnings`.
    let mut said = Vec::new();
    for (name, e, named) in wanted {
        let doc = &source.session.doc;
        let param = doc.param(&name).ok_or_else(|| EngineError::Other(format!("{shown} has no user parameter `{name}`")))?;
        if param.model {
            return Err(EngineError::Other(format!("`{name}` in {shown} belongs to a dimension or feature input: a link sets user parameters only")));
        }
        if solidcraft_doc::text_params::is_text_unit(&param.unit) || doc.text_param(&name).is_some() {
            return Err(EngineError::Other(format!("`{name}` in {shown} is a text parameter: a link sets number parameters only")));
        }
        let value = if named { host.named(&e, param.kind(), &param.unit) } else { host.value(&e, param.kind(), &param.unit) };
        let text = match value {
            Ok(text) => text,
            // A parameter that only shares a name keeps its own value when the two don't agree
            // (an angle here, a length there).
            Err(why) if named => {
                said.push(format!("`{name}` in {shown} keeps its own value: {why}"));
                continue;
            }
            Err(why) => return Err(EngineError::Other(format!("`{name}` = `{e}`: {why}"))),
        };
        source.original.entry(name.clone()).or_insert_with(|| param.expr.clone());
        applied.insert(name, text);
    }
    // Every parameter a link set here before and this one doesn't goes back to its own value.
    let mut changes = serde_json::Map::new();
    for (name, original) in &source.original {
        let target = applied.get(name).unwrap_or(original);
        if source.session.doc.param(name).is_some_and(|p| p.expr != *target) {
            changes.insert(name.clone(), Value::String(target.clone()));
        }
    }
    if !changes.is_empty() {
        let (changed, kept) = collect_kept(|| with_own_report(|| source.session.execute("parameters.change", &json!({"changes": changes}))));
        let changed = changed.map_err(|e| EngineError::Other(format!("{shown}: {e}")))?;
        // A link inside that kept its bodies leaves this design's bodies without these values.
        if !kept.is_empty() {
            return Ok(Drove::Kept(kept));
        }
        // What else the linked design's own driven links said, said by this design.
        for warning in changed.get("link_warnings").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
            said.push(format!("{shown}: {warning}"));
        }
    } else if reset {
        source.session.refresh();
    }
    // A link inside that kept its bodies before (when its configuration was set, or when the
    // design was saved), and still gives other values than it was made with, leaves this design's
    // bodies without these values too, even when nothing here changed. Its design may follow now
    // (its file is back), so it is driven once more first, as a value edit would drive it.
    let mut stale = stale_links(&source.session);
    if !stale.is_empty() {
        let (followed, _) = collect_kept(|| with_own_report(|| follow_stale(&mut source.session)));
        followed.map_err(|e| EngineError::Other(format!("{shown}: {e}")))?;
        stale = stale_links(&source.session);
    }
    if !stale.is_empty() {
        return Ok(Drove::Kept(stale));
    }
    let mut linked = live_bodies(source, shown)?;
    for warning in said {
        note_warning(warning.clone());
        linked.1.warnings.push(warning);
    }
    let doc = &source.session.doc;
    let units = doc.params.iter().filter(|p| settable(doc, p)).map(|p| (p.name.clone(), p.unit.clone())).collect();
    Ok(Drove::Bodies(Box::new((linked, applied, units))))
}

/// The linked bodies of `source`'s design as it stands: its own bodies as records in the first
/// read's place when they still match it one for one, else read from a new STEP export (which
/// becomes the template when it matches).
fn live_bodies(source: &mut Source, shown: &str) -> Result<Linked> {
    let session = &source.session;
    if session.model.results.iter().any(|r| r.error.is_some()) {
        return session_bodies(session, shown);
    }
    let world = session.world_state()?;
    let solids: Vec<&solidcraft_doc::ModelBody> = world.bodies.iter().filter(|b| !b.body.is_mesh()).collect();
    let faces: Vec<(String, std::sync::Arc<Vec<String>>)> = solids.iter().map(|b| (b.name.clone(), solidcraft_doc::naming::face_names(b))).collect();
    if let Some(template) = source.template.as_ref().filter(|t| t.faces == faces)
        && let Some(linked) = as_records(template, &world, &solids, shown, {
            // Mesh bodies are not records: they come as the design has them now.
            world.bodies.iter().filter(|b| b.body.is_mesh()).map(|b| crate::cmd::file::mesh_data(&session.doc, b, &b.mesh())).collect()
        })?
    {
        #[cfg(test)]
        RECORD_READS.with(|n| n.set(n.get() + 1));
        return Ok(linked);
    }
    let linked = session_bodies(session, shown)?;
    // A read that gave each body back, in order, with its faces in order (the names carried onto
    // them are the body's own) can stand for the next ones.
    let one_for_one = linked.1.import.bodies.len() == solids.len()
        && linked.1.import.bodies.iter().zip(&solids).zip(&linked.2).zip(&faces).all(|(((read, live), carried), (_, names))| {
            read.name == live.name && read.body.face_count() == live.body.face_count() && carried.as_slice() == names.as_slice()
        });
    source.template = one_for_one.then(|| Template {
        faces,
        linked: (
            linked.0.clone(),
            solidcraft_io::StepFeature {
                name: linked.1.name.clone(),
                kind: linked.1.kind.clone(),
                body_names: linked.1.body_names.clone(),
                invisible: linked.1.invisible.clone(),
                warnings: linked.1.warnings.clone(),
                import: linked.1.import.clone(),
            },
            linked.2.clone(),
            linked.3.clone(),
            linked.4.clone(),
            linked.5.clone(),
        ),
    });
    Ok(linked)
}

/// `solids` (the design's, matching `template` one for one) handed over as B-rep records, with the
/// template's names, colours (face colours and opacity too), materials and looks, and the
/// cosmetic threads they have now. `None`
/// when the kernel doesn't make each of them back (the caller reads STEP text instead).
fn as_records(
    template: &Template,
    world: &solidcraft_doc::ModelState,
    solids: &[&solidcraft_doc::ModelBody],
    shown: &str,
    meshes: Vec<solidcraft_doc::MeshData>,
) -> Result<Option<Linked>> {
    let (title, read, carried, metadata, properties, _) = &template.linked;
    let FeatureKind::Import { file, skip, .. } = &read.kind else { return Ok(None) };
    let mut bodies = Vec::with_capacity(solids.len());
    for (live, was) in solids.iter().zip(&read.import.bodies) {
        let Ok(brep) = solidcraft_kernel::brep_record(&live.body) else { return Ok(None) };
        let faces = u64::try_from(live.body.face_count()).unwrap_or(u64::MAX);
        bodies.push(solidcraft_kernel::RecordBody {
            name: was.name.clone(),
            path: was.path.clone(),
            instance: was.instance.clone(),
            color: was.color,
            // The read's face colours and opacity, by the face they are on: the live body's faces
            // are the read's one for one (the template's test), and each is its own file face.
            paint: was.body.paint().cloned(),
            hidden: was.hidden,
            placement: None,
            brep: Some(brep),
            face_origin: (0..faces).collect(),
            mesh: None,
            copy_of: None,
            sheet: !was.closed,
        });
    }
    let tree = record_tree(&read.import.tree);
    let records = solidcraft_kernel::ImportRecords { bodies, tree, warnings: Vec::new(), system: read.import.system.clone() };
    let import = solidcraft_kernel::records_import_shared(&records).map_err(|e| EngineError::Other(format!("{shown}: {e}")))?;
    if import.bodies.len() != solids.len() {
        return Ok(None);
    }
    let kind = FeatureKind::Import {
        file: file.clone(),
        step: "".into(),
        components: import.tree.clone(),
        skip: skip.clone(),
        records: Some(std::sync::Arc::new(records)),
    };
    let feature = solidcraft_io::StepFeature {
        name: read.name.clone(),
        kind,
        body_names: read.body_names.clone(),
        invisible: read.invisible.clone(),
        warnings: read.warnings.clone(),
        import,
    };
    // Threads follow the design's values; the rest of what the read carried stays as it was.
    let mut properties = properties.clone();
    for (props, live) in properties.iter_mut().zip(solids) {
        props.threads = world.threads.iter().filter(|t| t.body == live.name).cloned().collect();
    }
    Ok(Some((title.clone(), feature, carried.clone(), metadata.clone(), properties, meshes)))
}

/// A STEP read's product tree for records: each node's bodies by their place among the records
/// (the read's own order), with no STEP items or solids left out.
fn record_tree(nodes: &[solidcraft_kernel::StepNode]) -> Vec<solidcraft_kernel::StepNode> {
    nodes
        .iter()
        .map(|n| solidcraft_kernel::StepNode {
            name: n.name.clone(),
            transform: n.transform,
            bodies: n.bodies.clone(),
            items: Vec::new(),
            left_out: Vec::new(),
            children: record_tree(&n.children),
        })
        .collect()
}

/// Whether `link` sets any of its design's parameters.
pub(super) fn drives(link: &DesignLink) -> bool {
    link.parameters_by_name || !link.parameters.is_empty()
}

/// The bodies `link` gives in `s` from the file's `bytes`: driven by its parameters when it sets
/// any, with the values given, else the design's own. `strict` (the link's settings are being
/// set): its kept parameters are checked against the design too.
pub(super) fn link_bodies(
    s: &Session,
    link: &DesignLink,
    file: &Path,
    bytes: &[u8],
    configuration: Option<SelectedConfiguration>,
    strict: bool,
) -> Result<(Linked, BTreeMap<String, String>, BTreeMap<String, String>)> {
    link_bodies_with_images(s, link, file, bytes, configuration, strict).map(|(linked, _, applied, units)| (linked, applied, units))
}

/// [`link_bodies`], with the design's canvases and decals where the design shows them with those
/// values (`design_images`), for Insert Design.
pub(super) fn link_bodies_with_images(
    s: &Session,
    link: &DesignLink,
    file: &Path,
    bytes: &[u8],
    configuration: Option<SelectedConfiguration>,
    strict: bool,
) -> Result<(Linked, super::LinkedImages, BTreeMap<String, String>, BTreeMap<String, String>)> {
    if !drives(link) && !(strict && !link.parameters_kept.is_empty()) {
        return super::design_bodies(&super::Source::File(file.to_path_buf()), bytes, configuration, s.validation())
            .map(|(l, images)| (l, images, BTreeMap::new(), BTreeMap::new()));
    }
    // The command that called this reports the drive's warnings with the bodies (each in its
    // `warnings`), so this thread's report, which is for value edits, is left as it was.
    let host = Host::of(s);
    // A command that sets the values refuses them when a design inside can't follow: the
    // bodies it would take don't have them.
    WANT_IMAGES.with(|w| w.set(true));
    let drove = with_own_report(|| driven_bodies(file, bytes, configuration, link, strict, &host, s.validation()));
    WANT_IMAGES.with(|w| w.set(false));
    let images = DRIVEN_IMAGES.with(|i| i.borrow_mut().take()).unwrap_or_default();
    match drove? {
        Drove::Bodies(driven) => {
            let (linked, applied, units) = *driven;
            Ok((linked, images, applied, units))
        }
        Drove::Kept(kept) => Err(EngineError::Other(format!("{} can't take these values: {}", file.display(), kept.join("; ")))),
    }
}

/// The STEP reads of the linked bodies a value edit took. Holding them until the design has
/// rebuilt lets the Import features reuse them instead of reading each STEP text again
/// (`StepFeature::import`).
/// With them, what each link that took new bodies inherits from its design (materials, looks,
/// face colours), applied once the design has rebuilt ([`HeldReads::inherit`]): a body the new
/// values bring in has its own material, as it has when the link is set or updated.
#[must_use = "hold the reads until the design has rebuilt"]
pub(crate) struct HeldReads {
    #[allow(dead_code)]
    reads: Vec<std::sync::Arc<solidcraft_kernel::StepImport>>,
    properties: Vec<(u64, Vec<super::SourceProperties>)>,
}

impl HeldReads {
    /// Whether any link took new bodies.
    pub(crate) fn drove(&self) -> bool {
        !self.properties.is_empty()
    }

    /// After the design has rebuilt with the new bodies: each driven link takes its design's
    /// properties, onto the bodies as the rebuild named them; the design rebuilds again when
    /// one changed. Overrides made here stay, as with every link (`inherit_properties`).
    pub(crate) fn inherit(self, s: &mut Session) -> Result<()> {
        let mut changed = false;
        for (comp, properties) in &self.properties {
            changed |= inherit_properties(s, *comp, properties)?;
        }
        if changed {
            s.refresh();
        }
        Ok(())
    }
}

/// What a design's links are driven from: its units, its parameters (feature inputs included)
/// and what each link passes and last passed. [`drive_links`] keeps it on the session
/// (`Session::links_reconciled`); a command that leaves it different has changed what a link is
/// passed without the links following ([`follow_command`]).
fn link_inputs(doc: &solidcraft_doc::Document) -> u64 {
    let defs = doc.param_defs();
    let params: Vec<(&str, &str, &str)> = defs.iter().map(|d| (d.name.as_str(), d.expr.as_str(), d.unit.as_str())).collect();
    let links: Vec<_> = doc
        .links
        .iter()
        .map(|(c, l)| (*c, &l.path, &l.file_sha256, l.configuration, &l.parameters, l.parameters_by_name, &l.parameters_kept, &l.parameter_values))
        .collect();
    digest(&(doc.units.as_str(), params, links))
}

/// Put `linked` in the Import feature of `link`, the link of component `comp` (`name`) to the
/// design at `file`, and its mesh bodies in the link's mesh feature: whether its bodies changed,
/// and the read to hold until the design has rebuilt.
fn take_bodies(
    s: &mut Session,
    comp: u64,
    name: &str,
    file: &Path,
    link: &DesignLink,
    linked: Linked,
) -> Result<(bool, std::sync::Arc<solidcraft_kernel::StepImport>)> {
    let (_, sf, faces, _, properties, meshes) = linked;
    let read = sf.import.clone();
    let same = s.doc.feature(link.feature).is_some_and(|f| super::same_import_source(&f.kind, &sf.kind));
    if let Some(f) = s.doc_mut().feature_mut(link.feature) {
        if !same {
            if f.body_names.len() != sf.body_names.len() {
                f.body_names = sf.body_names;
            }
            f.kind = sf.kind;
        }
        f.link_faces = faces;
        // The cosmetic threads follow the design's values with its bodies, in the same step (a
        // thread the full length of a shaft made longer is longer too).
        f.link_threads = super::linked_threads(&properties);
    }
    let meshes_changed = super::set_link_meshes(s, comp, link.feature, name, &file.display().to_string(), meshes)?;
    Ok((!same || meshes_changed, read))
}

/// The number user parameters of `link`'s design, from the file it last read: kept in [`NAMES`]
/// while the file and the link's record of it agree. `None` when the file can't be read or has
/// changed since (the link can't be driven then anyway).
fn settable_names(s: &Session, comp: u64, link: &DesignLink) -> Option<std::sync::Arc<Vec<SettableName>>> {
    // The parameters the link recorded when it last took bodies: no file to read, and known
    // when the file is gone or changed (a match then keeps the link's bodies, saying why).
    if !link.parameter_units.is_empty() {
        let names = link.parameter_units.iter().map(|(name, unit)| (name.clone(), Kind::from_unit(unit), unit.clone())).collect();
        return Some(std::sync::Arc::new(names));
    }
    let file = super::absolute(&super::resolve(s.path.as_deref(), &link.path));
    let key: NamesKey = (file, link.file_sha256.clone(), link.configuration);
    if link.file_sha256.is_some()
        && let Some(names) = NAMES.lock().ok().and_then(|n| n.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone()))
    {
        return Some(names);
    }
    let (_, file, bytes) = read_linked(s, comp, link).ok()?;
    if link.file_sha256.as_ref().is_some_and(|kept| *kept != DesignLink::file_digest(&bytes)) {
        return None;
    }
    let (mut doc, _) = read_document(&file, &bytes, false).ok()?;
    if let Some(row) = link.configuration {
        doc.apply_configuration(row).ok()?;
    }
    let names: Vec<SettableName> = doc.params.iter().filter(|p| settable(&doc, p)).map(|p| (p.name.clone(), p.kind(), p.unit.clone())).collect();
    let names = std::sync::Arc::new(names);
    if link.file_sha256.is_some()
        && let Ok(mut cache) = NAMES.lock()
    {
        cache.retain(|(k, _)| *k != key);
        if cache.len() >= NAMES_SIZE {
            cache.remove(0);
        }
        cache.push((key, names.clone()));
    }
    Some(names)
}

/// Whether `link` sets parameters by name and its design has one it doesn't pass yet that this
/// design now has a user parameter of the same name and kind for (gained, or renamed to it).
fn new_match(s: &Session, comp: u64, link: &DesignLink, host: &Host) -> bool {
    link.parameters_by_name
        && settable_names(s, comp, link).is_some_and(|names| {
            names.iter().any(|(name, kind, unit)| {
                host.user.contains(name)
                    && !link.parameters.contains_key(name)
                    && !link.parameters_kept.contains(name)
                    && !link.parameter_values.contains_key(name)
                    && host.named(name, *kind, unit).is_ok()
            })
        })
}

fn component_name(s: &Session, comp: u64) -> String {
    s.doc.components.iter().find(|c| c.id == comp).map_or_else(|| comp.to_string(), |c| c.name.clone())
}

/// Whether `link`'s expressions give other values in `host` than the ones it last gave (in the
/// unit it gave each in): a parameter it sets that it gave no value yet, or one set by name that
/// this design no longer has, counts as changed. Why not, when one of its own expressions no
/// longer works out.
fn values_changed(host: &Host, link: &DesignLink) -> std::result::Result<bool, String> {
    if link.parameters.keys().any(|n| !link.parameter_values.contains_key(n)) {
        return Ok(true);
    }
    for (name, before) in &link.parameter_values {
        let (unit, kind) = stored_unit(before);
        let explicit = link.parameters.get(name);
        let e = match explicit {
            Some(e) => e.as_str(),
            None if link.parameters_by_name => name.as_str(),
            None => return Ok(true),
        };
        match host.value(e, kind, unit) {
            Ok(text) if text == *before => {}
            Ok(_) => return Ok(true),
            Err(why) if explicit.is_some() => return Err(format!("`{name}` = `{e}`: {why}")),
            // A parameter set by name that this design no longer has goes back to its own.
            Err(_) => return Ok(true),
        }
    }
    Ok(false)
}

/// The links of `s`'s design whose bodies were not made with the values their expressions give
/// now, each said as a value edit says it: an edit could not drive them (their file is gone or
/// changed, a loop, a link inside that could not follow), whether this session made that edit or
/// the design was saved after it, or a configuration set the values. A design with any has
/// bodies without the values it was given, whatever values it is given next.
fn stale_links(s: &Session) -> Vec<String> {
    if !s.doc.links.values().any(drives) {
        return Vec::new();
    }
    let host = Host::of(s);
    let mut stale = Vec::new();
    for (comp, link) in s.doc.links.iter().filter(|(_, l)| drives(l)) {
        let cname = component_name(s, *comp);
        // A by-name match it doesn't pass yet counts too: its bodies don't have that value.
        let why = match values_changed(&host, link).map(|changed| changed || new_match(s, *comp, link, &host)) {
            Ok(false) => continue,
            Err(why) => why,
            Ok(true) => match read_linked(s, *comp, link) {
                Err(e) => e.to_string(),
                Ok((_, file, bytes)) if link.file_sha256.as_ref().is_some_and(|kept| *kept != DesignLink::file_digest(&bytes)) => {
                    format!("{} changed since this design last read it; Update Links takes in the change", file.display())
                }
                Ok((_, file, _)) => match loop_through(&file) {
                    Some(chain) => format!("its design links back to one that passes it values ({chain})"),
                    None => "its bodies were made with other values than this design passes it now".to_string(),
                },
            },
        };
        stale.push(format!("{cname} keeps its bodies: {why}"));
    }
    stale
}

/// The links of a linked design that kept their bodies, driven once more ([`drive_links`]): the
/// design rebuilt with the bodies of those that follow now, and their properties taken.
fn follow_stale(s: &mut Session) -> Result<()> {
    let reads = drive_links(s)?;
    if reads.drove() {
        s.refresh();
        reads.inherit(s)?;
    }
    Ok(())
}

/// After a value edit: each link whose parameters' expressions now give other values takes the
/// design's bodies with them. A link whose file is gone or changed since this design read it, or
/// whose design has a link inside that can't follow (at any depth), keeps its bodies and the
/// values they were made with (said in [`take_drive_report`], by the path from this design's
/// placement); one whose design fails with the new values, or whose expression no longer works
/// out, fails the edit.
pub(crate) fn drive_links(s: &mut Session) -> Result<HeldReads> {
    match LINK_READS.with(|r| r.borrow().clone()) {
        Some(scope) => solidcraft_io::vfs::scoped(Some(scope), || drive_links_here(s)),
        None => drive_links_here(s),
    }
}

/// Runs `f`, a plugin's command, with the linked designs its edits drive read only from
/// `folders` (the folders the user let the plugin read): the command's own file scope gives
/// them no authority, and the caller's never reaches them ([`drive_links`]).
pub(crate) fn reading_links_in<T>(folders: &[PathBuf], f: impl FnOnce() -> T) -> T {
    struct Restore(Option<solidcraft_io::vfs::PathScope>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let previous = self.0.take();
            LINK_READS.with(|r| *r.borrow_mut() = previous);
        }
    }
    let scope = solidcraft_io::vfs::PathScope::new(folders.iter().cloned());
    let _restore = Restore(LINK_READS.with(|r| r.replace(Some(scope))));
    f()
}

fn drive_links_here(s: &mut Session) -> Result<HeldReads> {
    let mut held = HeldReads { reads: Vec::new(), properties: Vec::new() };
    if !s.doc.links.values().any(drives) {
        return Ok(held);
    }
    let host = Host::of(s);
    let _driving = Driving::enter(s);
    let driven: Vec<(u64, DesignLink)> = s.doc.links.iter().filter(|(_, l)| drives(l)).map(|(c, l)| (*c, l.clone())).collect();
    for (comp, link) in driven {
        let cname = component_name(s, comp);
        // Whether an expression gives another value now than the link gave before, or a link that
        // sets parameters by name can take one it didn't (this design gained a parameter of that
        // name).
        let changed = values_changed(&host, &link).map_err(|why| EngineError::Other(format!("{cname}: {why}")))? || new_match(s, comp, &link, &host);
        if !changed {
            continue;
        }
        let (_, file, bytes) = match read_linked(s, comp, &link) {
            Ok(read) => read,
            Err(e) => {
                note_kept(format!("{cname} keeps its bodies: {e}"));
                continue;
            }
        };
        if let Some(chain) = loop_through(&file) {
            note_kept(format!("{cname} keeps its bodies: its design links back to one that passes it values ({chain})"));
            continue;
        }
        if link.file_sha256.as_ref().is_some_and(|kept| *kept != DesignLink::file_digest(&bytes)) {
            note_kept(format!(
                "{cname} keeps its bodies: {} changed since this design last read it; Update Links takes in the change",
                file.display()
            ));
            continue;
        }
        let drove = driven_bodies(&file, &bytes, selected_configuration(&link), &link, false, &host, s.validation())
            .map_err(|e| EngineError::Other(format!("{cname}: {e}")))?;
        let (linked, applied, units) = match drove {
            Drove::Bodies(driven) => *driven,
            // Its bodies and the values they were made with stay; each link inside that kept its
            // own is said by the path to it from this placement.
            Drove::Kept(kept) => {
                for inner in kept {
                    note_kept(format!("{cname} keeps its bodies: in {}, {inner}", file.display()));
                }
                continue;
            }
        };
        let properties = linked.4.clone();
        held.reads.push(take_bodies(s, comp, &cname, &file, &link, linked)?.1);
        held.properties.push((comp, properties));
        if let Some(kept) = s.doc_mut().links.get_mut(&comp) {
            kept.parameter_values = applied;
            kept.parameter_units = units;
        }
        note_driven(cname);
    }
    // The links have followed these inputs (or said why not): a later command that leaves them
    // so needs no drive of its own.
    s.links_reconciled = Some(link_inputs(&s.doc));
    Ok(held)
}

/// A plugin's run is taken in: its links stand as the plugin's own scope left them. Each edit it
/// made followed inside that scope (`plugin::SolidCraftHost::run_command`); a design it opened or
/// replaced keeps the bodies it was saved with, as a link whose file the plugin may not read
/// does. `plugin.run`'s own follow-up then finds nothing to drive, so no linked design is read
/// with the caller's authority because of what the plugin did. A later edit that changes what a
/// link is passed drives it as usual.
#[cfg_attr(not(all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64"))), allow(dead_code))]
pub(crate) fn mark_links_reconciled(s: &mut Session) {
    s.links_reconciled = Some(link_inputs(&s.doc));
}

/// After a command: when what the design's links are driven from is not what they last followed
/// (a feature input a link's expression reads, a rename that gives a link by name a match, or
/// a step of a command made of several that came after its value edit), the links follow in the
/// command's undo step, rebuilt and with their properties. Gives what the command's links did,
/// as a value edit reports it: the components that took new bodies, and the warnings of those
/// that kept theirs, from this drive and from the command's own value edit when its reply
/// doesn't carry them (`parameters.add`, `parameters.delete`; `parameters.change` takes its own).
pub(crate) fn follow_command(s: &mut Session) -> Result<(Vec<String>, Vec<String>)> {
    let (mut linked, mut warnings) = take_drive_report();
    if !s.doc.links.values().any(drives) || s.links_reconciled == Some(link_inputs(&s.doc)) {
        return Ok((linked, warnings));
    }
    forget_kept();
    let reads = drive_links(s)?;
    if reads.drove() {
        s.refresh();
        reads.inherit(s)?;
    }
    let (more_linked, more_warnings) = take_drive_report();
    linked.extend(more_linked);
    warnings.extend(more_warnings);
    Ok((linked, warnings))
}

/// What `link` sets, for a reply: its own expressions, and the parameters it set by name.
pub(super) fn listed(link: &DesignLink) -> BTreeMap<String, LinkedParameter> {
    let value = |name: &str| link.parameter_values.get(name).and_then(|t| reported(t));
    let own = link.parameters.iter().map(|(name, e)| (name.clone(), LinkedParameter { expression: e.clone(), value: value(name), by_name: false }));
    let named = link
        .parameter_values
        .keys()
        .filter(|name| link.parameters_by_name && !link.parameters.contains_key(*name))
        .map(|name| (name.clone(), LinkedParameter { expression: name.clone(), value: value(name), by_name: true }));
    own.chain(named).collect()
}

/// The most parameters one link may keep.
const MAX_KEPT: usize = 1000;

/// `p.by_name`: `None` when left out.
fn by_name_of(cmd: &str, p: &Value) -> Result<Option<bool>> {
    match p.get("by_name") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(bad(cmd, "`by_name` must be true or false")),
    }
}

/// `p.keep`, the names a link keeps at the design's own value: `None` when left out.
fn kept_of(cmd: &str, p: &Value) -> Result<Option<std::collections::BTreeSet<String>>> {
    let list = match p.get("keep") {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Array(list)) if list.len() <= MAX_KEPT => list,
        Some(_) => return Err(bad(cmd, format!("`keep` must be a list of up to {MAX_KEPT} parameter names"))),
    };
    let mut kept = std::collections::BTreeSet::new();
    for name in list {
        match name.as_str().map(str::trim) {
            Some(name) if !name.is_empty() => {
                kept.insert(name.to_string());
            }
            _ => return Err(bad(cmd, "each name in `keep` must be the name of a parameter")),
        }
    }
    Ok(Some(kept))
}

/// One expression given for parameter `name` in `field`: text or a number; `None` for null.
fn expression_of(cmd: &str, field: &str, name: &str, e: &Value) -> Result<Option<String>> {
    match e {
        Value::Null => Ok(None),
        Value::String(text) if !text.trim().is_empty() => Ok(Some(text.trim().to_string())),
        Value::Number(n) => Ok(Some(n.to_string())),
        _ => Err(bad(cmd, format!("`{field}.{name}` must be an expression (text or a number) or null"))),
    }
}

/// Refuses a parameter both kept and given an expression.
fn check_kept(cmd: &str, parameters: &BTreeMap<String, String>, kept: &std::collections::BTreeSet<String>) -> Result<()> {
    match kept.iter().find(|name| parameters.contains_key(*name)) {
        Some(name) => Err(bad(cmd, format!("`{name}` is both kept and given an expression: take it out of `keep` to set it"))),
        None => Ok(()),
    }
}

/// What `component.insert_design` asks the new link to pass (`parameters`, `by_name`, `keep`):
/// a link with them and nothing else.
pub(super) fn requested(cmd: &str, p: &Value) -> Result<DesignLink> {
    let mut parameters = BTreeMap::new();
    match p.get("parameters") {
        None | Some(Value::Null) => {}
        Some(Value::Object(given)) => {
            for (name, e) in given {
                let name = name.trim();
                if name.is_empty() {
                    return Err(bad(cmd, "a parameter name in `parameters` is blank"));
                }
                let e = expression_of(cmd, "parameters", name, e)?.ok_or_else(|| bad(cmd, format!("`parameters.{name}` must be an expression")))?;
                parameters.insert(name.to_string(), e);
            }
        }
        Some(_) => return Err(bad(cmd, "`parameters` must be an object of parameter name: expression")),
    }
    let parameters_kept = kept_of(cmd, p)?.unwrap_or_default();
    check_kept(cmd, &parameters, &parameters_kept)?;
    Ok(DesignLink { parameters, parameters_by_name: by_name_of(cmd, p)?.unwrap_or(false), parameters_kept, ..Default::default() })
}

/// Whether `a` and `b` pass their design the same values.
pub(super) fn same_drive(a: &DesignLink, b: &DesignLink) -> bool {
    a.parameters == b.parameters && a.parameters_by_name == b.parameters_by_name && a.parameters_kept == b.parameters_kept
}

/// Whether `link` can be passed values: only a link to a design file can. A linked STEP file has
/// no parameters, and a design inserted from the session has no file to build again.
fn takes_values(link: &DesignLink) -> std::result::Result<(), &'static str> {
    if link.step_names.is_some() {
        return Err("that component is linked to a STEP file, which has no parameters");
    }
    if link.path.is_empty() {
        return Err("that component's design has no file; save the design and insert it from its file to pass it values");
    }
    Ok(())
}

/// `component.link_parameters`.
pub(super) fn link_parameters(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.link_parameters";
    let comp = component_key(s, p, "component", cmd)?;
    let link = s.doc.links.get(&comp).cloned().ok_or_else(|| bad(cmd, "that component is not linked to a design"))?;
    takes_values(&link).map_err(|why| bad(cmd, why))?;
    let by_name = by_name_of(cmd, p)?;
    let kept = kept_of(cmd, p)?;
    let update = match p.get("update") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(bad(cmd, "`update` must be true or false")),
    };
    if p.get("set").is_none() && by_name.is_none() && kept.is_none() {
        if update {
            return Err(bad(cmd, "`update` takes in the file with new settings: give `set`, `by_name` or `keep` too, or run Update Links"));
        }
        return reply(
            cmd,
            LinkParametersResult {
                component: comp,
                feature: link.feature,
                parameters: listed(&link),
                by_name: link.parameters_by_name,
                keep: link.parameters_kept.iter().cloned().collect(),
                geometry_changed: false,
                warnings: Vec::new(),
                param_warnings: Vec::new(),
            },
        );
    }
    let empty = serde_json::Map::new();
    let set = match p.get("set") {
        None | Some(Value::Null) => &empty,
        Some(set) => set.as_object().ok_or_else(|| bad(cmd, "`set` must be an object of parameter name: expression or null"))?,
    };
    let mut parameters = link.parameters.clone();
    for (name, e) in set {
        let name = name.trim();
        if name.is_empty() {
            return Err(bad(cmd, "a parameter name in `set` is blank"));
        }
        match expression_of(cmd, "set", name, e)? {
            None => parameters.remove(name),
            Some(e) => parameters.insert(name.to_string(), e),
        };
    }
    let kept = kept.unwrap_or_else(|| link.parameters_kept.clone());
    check_kept(cmd, &parameters, &kept)?;
    let cname = s.doc.components.iter().find(|c| c.id == comp).map_or_else(|| comp.to_string(), |c| c.name.clone());
    let (_, file, bytes) = read_linked(s, comp, &link).map_err(|e| bad(cmd, format!("{cname}: {e}")))?;
    let read = DesignLink::file_digest(&bytes);
    let changed = link.file_sha256.as_ref().is_some_and(|kept| *kept != read);
    // Setting values never takes in a newer version of the file on its own: that is Update
    // Links' step, or this one's with `update` (a file whose parameters were renamed can't be
    // updated with the settings the link has, nor set without taking in the file).
    if changed && !update {
        return Err(bad(
            cmd,
            format!(
                "{cname}: {} changed since this design last read it; run Update Links first, then set its parameters, or set them with `update` to take in the change in the same step",
                file.display()
            ),
        ));
    }
    let next = DesignLink { parameters, parameters_by_name: by_name.unwrap_or(link.parameters_by_name), parameters_kept: kept, ..link.clone() };
    let (linked, applied, units) = link_bodies(s, &next, &file, &bytes, selected_configuration(&link), true).map_err(|e| bad(cmd, e.to_string()))?;
    if changed {
        // The settings work with the file as it is now: Update Links takes it in with them, as
        // it takes in any change to the file (the drive it makes again is the one just made,
        // kept open). Its warnings come with this reply.
        if let Some(kept) = s.doc_mut().links.get_mut(&comp) {
            kept.parameters = next.parameters;
            kept.parameters_by_name = next.parameters_by_name;
            kept.parameters_kept = next.parameters_kept;
            kept.parameter_values = applied;
            kept.parameter_units = units;
        }
        let updated = super::update_links(s, &json!({"component": comp})).map_err(|e| bad(cmd, format!("{cname}: {e}")))?;
        let mut warnings = linked.1.warnings.clone();
        warnings.extend(updated["warnings"].as_array().into_iter().flatten().filter_map(|w| w.as_str().map(str::to_string)));
        warnings.dedup();
        let kept = s.doc.links.get(&comp).cloned().unwrap_or_default();
        return reply(
            cmd,
            LinkParametersResult {
                component: comp,
                feature: link.feature,
                parameters: listed(&kept),
                by_name: kept.parameters_by_name,
                keep: kept.parameters_kept.iter().cloned().collect(),
                geometry_changed: updated["updated"].as_array().is_some_and(|u| !u.is_empty()),
                warnings,
                param_warnings: Vec::new(),
            },
        );
    }
    let warnings = linked.1.warnings.clone();
    let properties = linked.4.clone();
    let (geometry_changed, _read) = take_bodies(s, comp, &cname, &file, &link, linked)?;
    if let Some(kept) = s.doc_mut().links.get_mut(&comp) {
        kept.parameters = next.parameters;
        kept.parameters_by_name = next.parameters_by_name;
        kept.parameters_kept = next.parameters_kept;
        kept.parameter_values = applied;
        kept.parameter_units = units;
        // The bytes these bodies came from: a link made before links kept them (none recorded)
        // knows from now on when its file changes, so no later edit takes in a change unasked.
        kept.file_sha256 = Some(read);
    }
    s.refresh();
    if let Some(e) = s.model.result(link.feature).and_then(|r| r.error.clone()) {
        return Err(bad(cmd, e));
    }
    inherit_properties(s, comp, &properties)?;
    s.refresh();
    let kept = s.doc.links.get(&comp).cloned().unwrap_or_default();
    reply(
        cmd,
        LinkParametersResult {
            component: comp,
            feature: link.feature,
            parameters: listed(&kept),
            by_name: kept.parameters_by_name,
            keep: kept.parameters_kept.iter().cloned().collect(),
            geometry_changed,
            warnings,
            param_warnings: Vec::new(),
        },
    )
}

/// `component.design_parameters`.
pub(super) fn design_parameters(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.design_parameters";
    let (file, bytes, link) = match (p.get("path"), p.get("component")) {
        (Some(_), Some(_)) => return Err(bad(cmd, "give `path` or `component`, not both")),
        (Some(path), None) => {
            let given =
                path.as_str().filter(|x| !x.trim().is_empty() && x.len() < 4096).ok_or_else(|| bad(cmd, "`path` must be the path of a design"))?;
            // A network or device path is judged from its text, as Insert Design judges it.
            super::link_refused(s.path.as_deref(), given).map_err(|e| bad(cmd, e.to_string()))?;
            let file = super::absolute(Path::new(given));
            let bytes = super::read_link(&file, s.path.as_deref())?;
            (file, bytes, None)
        }
        (None, Some(_)) => {
            let comp = component_key(s, p, "component", cmd)?;
            let link = s.doc.links.get(&comp).cloned().ok_or_else(|| bad(cmd, "that component is not linked to a design"))?;
            takes_values(&link).map_err(|why| bad(cmd, why))?;
            let (_, file, bytes) = read_linked(s, comp, &link).map_err(|e| bad(cmd, e.to_string()))?;
            (file, bytes, Some(link))
        }
        (None, None) => return Err(bad(cmd, "give `path` (a design file) or `component` (a linked component)")),
    };
    // The parameters only: the geometry saved with the design stays unread.
    let (mut doc, _) = read_document(&file, &bytes, false)?;
    // A link to a configuration shows that configuration's values, which the link starts from.
    if let Some(selected) = link.as_ref().and_then(selected_configuration) {
        if doc.configs.row_identity(selected.row) != selected.identity {
            return Err(bad(cmd, format!("{}: configuration `{}` was replaced; select a configuration again", file.display(), selected.row)));
        }
        doc.apply_configuration(selected.row).map_err(|e| bad(cmd, format!("{}: {e}", file.display())))?;
    }
    let host = Host::of(s);
    let parameters = doc
        .param_rows()
        .into_iter()
        .filter(|r| r.source == "user" && r.text.is_none() && doc.param(&r.name).is_some_and(|p| settable(&doc, p)))
        .map(|r| {
            // What taking it by name would give: this design's parameter of that name, as a
            // value of this one's kind.
            let here = host.user.contains(&r.name).then(|| host.named(&r.name, Kind::from_unit(&r.unit), &r.unit).ok()).flatten();
            DesignParameter {
                here: here.as_deref().and_then(reported),
                name: r.name,
                expression: r.expression,
                unit: r.unit,
                value: r.value,
                display_value: r.display_value,
                comment: r.comment,
            }
        })
        .collect();
    let link = link.unwrap_or_default();
    let changed = link.file_sha256.as_ref().is_some_and(|kept| *kept != DesignLink::file_digest(&bytes));
    reply(
        cmd,
        DesignParametersResult {
            file: file.display().to_string(),
            parameters,
            by_name: link.parameters_by_name,
            set: link.parameters,
            keep: link.parameters_kept.into_iter().collect(),
            changed,
            param_warnings: Vec::new(),
        },
    )
}

#[cfg(test)]
#[path = "component_link_parameters_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;
