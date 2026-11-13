//! The Drawing workspace's Export window (decision D24, board `review-export.png` in
//! `docs/design/drawing-ui/`): a right-hand panel that writes the chosen sheets of the open drawing
//! through `drawing.export`, one file a sheet, each as a background job (`file_load`), so a large
//! PNG never blocks the window. It holds only the window's choices; the engine checks every value
//! again. PDF and DXF show as planned until their writers land on `drawings`.
//!
//! The paper is the export's own choice, never the sheet colour on screen; a blueprint theme only
//! starts it on the matching blue. A view that can't be drawn is named here, before the export and
//! after a refusal alike, from the drawing's status (`drawing.status` lists it as `failed` with its
//! reason), never from the refusal's text, so a sheet is never refused silently.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};

use serde_json::{Value, json};
use solidcraft_engine::doc::drawing::Drawing;
use solidcraft_engine::doc::drawing::view::ViewKind;
use solidcraft_engine::doc::ids;
use solidcraft_engine::geom::drawing_sheet::SheetSize;
use solidcraft_engine::geom::drawing_view::NamedView;

use crate::SolveApp;
use crate::ui_kit::{self, StatusTone};
use solidcraft_theme::Tokens;

/// The command the window builds: its ribbon button opens the window, and the window runs it.
pub const ID: &str = "drawing.export";
/// What a sheet export stopped by Cancel answers (`file_load`).
const CANCELLED: &str = "file operation cancelled";
/// The longest file name the window makes, in characters, before its extension.
const MAX_NAME: usize = 120;

/// The formats the window writes. PDF and DXF are shown, faded, as planned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Svg,
    Png,
}

impl Format {
    fn name(self) -> &'static str {
        match self {
            Format::Svg => "svg",
            Format::Png => "png",
        }
    }
}

/// The paper a sheet is exported on (`drawing.export {paper}`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paper {
    White,
    Blueprint,
    PrussianBlue,
}

impl Paper {
    const ALL: [Paper; 3] = [Paper::White, Paper::Blueprint, Paper::PrussianBlue];

    fn name(self) -> &'static str {
        match self {
            Paper::White => "white",
            Paper::Blueprint => "blueprint",
            Paper::PrussianBlue => "prussian_blue",
        }
    }
}

/// How a PNG on white paper stores its pixels (`drawing.export {colour}`). A blue paper is always
/// in colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Colour {
    Grey,
    BlackWhite,
    Rgb,
    Transparent,
}

impl Colour {
    const ALL: [Colour; 4] = [Colour::Grey, Colour::BlackWhite, Colour::Rgb, Colour::Transparent];

    fn name(self) -> &'static str {
        match self {
            Colour::Grey => "grey",
            Colour::BlackWhite => "black_white",
            Colour::Rgb => "rgb",
            Colour::Transparent => "transparent",
        }
    }
}

/// Where the files go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Folder {
    /// Beside the saved design, named after the design and each sheet.
    Design,
    /// A save dialog when Export is pressed.
    Choose,
}

/// One sheet being written to `path`: the background job answers on `reply`.
struct Job {
    sheet: u64,
    path: String,
    reply: Receiver<Value>,
}

/// The window's choices while it is open. Screen state only: nothing here is saved.
pub struct Export {
    pub format: Format,
    pub sheets: BTreeSet<u64>,
    pub paper: Paper,
    /// The PNG resolution the window sets; `None` keeps each sheet's own.
    pub dpi: Option<f64>,
    pub colour: Colour,
    pub folder: Folder,
    jobs: Vec<Job>,
    /// OK was pressed: the window closes once every sheet is written.
    close_when_done: bool,
    /// Cancel stopped the sheets being written: the window stays to say which were written.
    stopping: bool,
    /// What the last export said, sheet by sheet, translated.
    outcome: Vec<(StatusTone, String)>,
    /// The last `drawing.status` reply, and the revision and drawing cache it was read at.
    status: Option<Value>,
    status_read: Option<StatusKey>,
}

/// What `drawing.status` was read at: the session's revision and how far the drawing's views and
/// dimensions are drawn ([`solidcraft_engine::drawing_scene::drawn`]). It reads the same until
/// one of them moves.
type StatusKey = (u64, (Option<u64>, u64));

impl Export {
    fn new(first_sheet: Option<u64>, paper: Paper, saved: bool) -> Self {
        Export {
            format: Format::Svg,
            sheets: first_sheet.into_iter().collect(),
            paper,
            dpi: None,
            colour: Colour::Grey,
            folder: if saved { Folder::Design } else { Folder::Choose },
            jobs: Vec::new(),
            close_when_done: false,
            stopping: false,
            outcome: Vec::new(),
            status: None,
            status_read: None,
        }
    }

    /// Sheets are being written.
    pub fn busy(&self) -> bool {
        !self.jobs.is_empty()
    }
}

/// The paper the window starts on: the matching blue in a blueprint theme, else white. The engine
/// stays explicit (`drawing.export` is white unless told otherwise); this is only the window's
/// first choice.
pub fn starting_paper(theme: &str) -> Paper {
    let theme = theme.to_ascii_lowercase();
    if theme.contains("prussian") {
        Paper::PrussianBlue
    } else if theme.contains("blueprint") {
        Paper::Blueprint
    } else {
        Paper::White
    }
}

/// Open the window for the drawing open (the ribbon's Export, `ui.start drawing.export`). With no
/// drawing open the engine's own rule says why, and nothing opens. A window writing sheets stays as
/// it is: a new one would drop the sheets under way, and what they say.
pub fn open(app: &mut SolveApp) {
    if app.drawing_workspace.export.as_ref().is_some_and(Export::busy) {
        app.drawing_workspace.settings_open = false;
        return;
    }
    if let Some(command) = solidcraft_engine::find_command(ID)
        && let Err(why) = command.allowed(&app.session)
    {
        app.set_status(crate::friendly_error(ID, &why), true);
        return;
    }
    let first = app.session.drawing(ID).ok().and_then(|d| d.sheets.first().map(|s| s.id));
    let paper = starting_paper(&app.preferences.theme);
    app.drawing_workspace.export = Some(Export::new(first, paper, app.session.path.is_some()));
    app.drawing_workspace.settings_open = false;
}

/// Close the window (Cancel, its close button, Esc, another panel). While sheets are being written
/// it stops them instead and stays open, so it can say which were written and which weren't.
pub fn close(app: &mut SolveApp) {
    if app.drawing_workspace.export.as_ref().is_some_and(Export::busy) {
        stop(app);
    } else {
        app.drawing_workspace.export = None;
    }
}

/// Another panel asks for the window's place: it goes, unless it is writing sheets, which it goes on
/// with and stays for (dropping it would leave them writing with nothing to say what they did).
pub fn give_way(app: &mut SolveApp) {
    if !app.drawing_workspace.export.as_ref().is_some_and(Export::busy) {
        app.drawing_workspace.export = None;
    }
}

/// Stop the sheets being written: those waiting are dropped, the one under way is told to stop, and
/// each answers in the window. A file already written is said as saved. Other file operations go on.
fn stop(app: &mut SolveApp) {
    let Some(export) = app.drawing_workspace.export.as_mut() else { return };
    export.stopping = true;
    export.close_when_done = false;
    let paths: Vec<String> = export.jobs.iter().map(|job| job.path.clone()).collect();
    app.file_load.cancel_sheet_exports(&paths);
}

/// A sheet's paper size as people name it: `A3`, `ANSI B`, `Letter`, or its sides for a custom one.
fn size_name(size: &SheetSize) -> String {
    match size {
        SheetSize::Custom { sides_mm: [a, b] } => format!("{a:.0} × {b:.0} mm"),
        SheetSize::Standard(paper) => {
            let code = serde_json::to_value(paper).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default();
            match code.split_once('_') {
                Some((family, letter)) => format!("{} {}", family.to_uppercase(), letter.to_uppercase()),
                None if code.starts_with('a') => code.to_uppercase(),
                None => {
                    let mut chars = code.chars();
                    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
                }
            }
        }
    }
}

/// A view as people name it: by the way it looks when that is a named view ("Right view"), else by
/// its place among its sheet's views ("View 3"). Two views of one name on a sheet take their number.
pub fn view_name(d: &Drawing, view: u64) -> String {
    let on_sheet: Vec<u64> = d.view(view).map_or_else(Vec::new, |v| d.views.iter().filter(|o| o.sheet == v.sheet).map(|o| o.id).collect());
    let number = on_sheet.iter().position(|id| *id == view).map_or(0, |at| at + 1);
    let plain = |id: u64| -> Option<&'static str> {
        let looks = match d.view(id)?.kind {
            ViewKind::Base { view: solidcraft_engine::doc::drawing_view::ViewDirection::Named(named), .. } => Some(named),
            _ => d.resolve_view(id).ok().and_then(|r| NamedView::ALL.iter().copied().find(|n| n.basis() == r.basis)),
        }?;
        Some(match looks {
            NamedView::Front => "ui.drawing_export.view_front",
            NamedView::Back => "ui.drawing_export.view_back",
            NamedView::Top => "ui.drawing_export.view_top",
            NamedView::Bottom => "ui.drawing_export.view_bottom",
            NamedView::Left => "ui.drawing_export.view_left",
            NamedView::Right => "ui.drawing_export.view_right",
            NamedView::IsoFrontRight | NamedView::IsoFrontLeft | NamedView::IsoBackRight | NamedView::IsoBackLeft => "ui.drawing_export.view_iso",
        })
    };
    let numbered = solidcraft_ui_text::format("ui.drawing_export.view_number", &[("number", &number.to_string())]);
    match plain(view) {
        Some(key) if on_sheet.iter().filter(|id| plain(**id) == Some(key)).count() > 1 => format!("{} ({number})", solidcraft_ui_text::tr(key)),
        Some(key) => solidcraft_ui_text::tr(key).to_owned(),
        None => numbered,
    }
}

/// A file name made from the design's and a sheet's names: characters no file system takes become
/// dashes, and it is kept short.
pub fn file_stem(design: &str, sheet: &str) -> String {
    let joined = format!("{design} - {sheet}");
    let cleaned: String =
        joined.chars().map(|c| if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '-' } else { c }).collect();
    let short: String = cleaned.trim().chars().take(MAX_NAME).collect();
    let short = short.trim_end_matches(['.', ' ']).to_owned();
    if short.is_empty() { "drawing".into() } else { short }
}

/// The file each chosen sheet goes to: `base` for one sheet; for several, `base`'s name with each
/// sheet's after it. Names are made distinct, and each ends in the format's extension.
pub fn paths_for(base: &Path, sheets: &[(u64, String)], extension: &str) -> Vec<(u64, PathBuf)> {
    let folder = base.parent().map(Path::to_path_buf).unwrap_or_default();
    let stem = base.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "drawing".into());
    let mut taken: BTreeSet<String> = BTreeSet::new();
    let mut out = Vec::new();
    for (sheet, name) in sheets {
        let mut own = if sheets.len() == 1 { stem.clone() } else { file_stem(&stem, name) };
        let mut n = 2;
        while !taken.insert(own.to_lowercase()) {
            own = format!("{} ({n})", if sheets.len() == 1 { stem.clone() } else { file_stem(&stem, name) });
            n += 1;
        }
        out.push((*sheet, folder.join(format!("{own}.{extension}"))));
    }
    out
}

/// What the window sends for one sheet.
fn params(export: &Export, sheet: u64, path: &str) -> Value {
    let mut p = json!({"sheet": ids::encode(sheet), "format": export.format.name(), "path": path, "paper": export.paper.name()});
    if export.format == Format::Png {
        if let Some(dpi) = export.dpi {
            p["dpi"] = json!(dpi);
        }
        // A blue paper is always in colour: the engine refuses another.
        if export.paper == Paper::White {
            p["colour"] = json!(export.colour.name());
        }
    }
    p
}

/// The chosen sheets in drawing order, with their names.
fn chosen(d: &Drawing, export: &Export) -> Vec<(u64, String)> {
    d.sheets.iter().filter(|s| export.sheets.contains(&s.id)).map(|s| (s.id, s.name.clone())).collect()
}

/// Queue a background job for each sheet and its file.
fn begin(app: &mut SolveApp, files: Vec<(u64, PathBuf)>) {
    let Some(export) = app.drawing_workspace.export.as_mut() else { return };
    export.outcome.clear();
    let mut queued = Vec::new();
    for (sheet, path) in files {
        let path = path.to_string_lossy().into_owned();
        let (tx, rx) = std::sync::mpsc::channel();
        let mut request = crate::file_load::Request::new(&path, crate::file_load::Kind::ExportSheet);
        request.options = params(export, sheet, &path).as_object().cloned().unwrap_or_default();
        request.reply = Some(tx);
        queued.push(request);
        export.jobs.push(Job { sheet, path, reply: rx });
    }
    for request in queued {
        // A refusal to queue answers the job's reply, which the window then shows.
        let _ = crate::file_load::begin_request(app, request);
    }
}

/// Export the chosen sheets now (Apply, or OK with `close`).
fn export_now(app: &mut SolveApp, close: bool) {
    let Ok(d) = app.session.drawing(ID).cloned() else { return };
    let Some(export) = app.drawing_workspace.export.as_mut() else { return };
    let sheets = chosen(&d, export);
    if sheets.is_empty() || export.busy() {
        return;
    }
    export.close_when_done = close;
    let extension = export.format.name();
    let design = app.session.doc.name.clone();
    let first = sheets.first().map(|(_, name)| name.clone()).unwrap_or_default();
    let suggested = if sheets.len() == 1 { file_stem(&design, &first) } else { file_stem(&design, "").trim_end_matches([' ', '-']).to_owned() };
    match (export.folder, app.session.path.as_deref().map(Path::new).and_then(Path::parent)) {
        (Folder::Design, Some(folder)) => {
            let files = sheets.iter().map(|(id, name)| (*id, folder.join(format!("{}.{extension}", file_stem(&design, name))))).collect();
            begin(app, distinct(files));
        }
        _ => {
            app.pick_save(&format!("{suggested}.{extension}"), &[extension], move |app, path| {
                let Some(path) = path else {
                    if let Some(export) = app.drawing_workspace.export.as_mut() {
                        export.close_when_done = false;
                    }
                    return;
                };
                let mut base = PathBuf::from(path);
                if !base.extension().is_some_and(|e| e.eq_ignore_ascii_case(extension)) {
                    base.as_mut_os_string().push(format!(".{extension}"));
                }
                begin(app, paths_for(&base, &sheets, extension));
            });
        }
    }
}

/// Files made distinct where two sheets' names clean to the same file name.
fn distinct(files: Vec<(u64, PathBuf)>) -> Vec<(u64, PathBuf)> {
    let mut taken: BTreeSet<String> = BTreeSet::new();
    files
        .into_iter()
        .map(|(sheet, path)| {
            let mut candidate = path.clone();
            let mut n = 2;
            while !taken.insert(candidate.to_string_lossy().to_lowercase()) {
                let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let extension = path.extension().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                candidate = path.with_file_name(format!("{stem} ({n}).{extension}"));
                n += 1;
            }
            (sheet, candidate)
        })
        .collect()
}

/// What one finished sheet says in the window.
fn outcome_lines(d: Option<&Drawing>, views: &[(u64, u64, String, String)], sheet: u64, reply: &Value) -> Vec<(StatusTone, String)> {
    let sheet_name = d.and_then(|d| d.sheets.iter().find(|s| s.id == sheet)).map_or_else(|| ids::encode(sheet), |s| s.name.clone());
    if reply.get("ok").and_then(Value::as_bool) == Some(true) {
        let result = reply.get("result").unwrap_or(&Value::Null);
        // The file's name: the folder is the one the window shows or the one just chosen.
        let path = result.get("path").and_then(Value::as_str).unwrap_or_default();
        let file = Path::new(path).file_name().map_or_else(|| path.to_owned(), |f| f.to_string_lossy().into_owned());
        let mut lines = vec![(StatusTone::Dim, solidcraft_ui_text::format("ui.drawing_export.saved", &[("sheet", &sheet_name), ("path", &file)]))];
        let stale = result.get("out_of_date").and_then(Value::as_array).map_or(0, Vec::len);
        if stale > 0 {
            let key = if stale == 1 { "ui.drawing_export.as_last_were_one" } else { "ui.drawing_export.as_last_were_other" };
            lines.push((StatusTone::Warning, solidcraft_ui_text::format(key, &[("sheet", &sheet_name), ("count", &stale.to_string())])));
        }
        let missing: Vec<&Value> = result
            .get("dimensions")
            .and_then(Value::as_array)
            .map_or_else(Vec::new, |all| all.iter().filter(|d| d.get("drawn") == Some(&json!(false))).collect());
        if let Some(first) = missing.first() {
            let why = first.get("why").and_then(Value::as_str).unwrap_or_default();
            let key = if missing.len() == 1 { "ui.drawing_export.dimensions_one" } else { "ui.drawing_export.dimensions_other" };
            lines.push((
                StatusTone::Warning,
                solidcraft_ui_text::format(key, &[("sheet", &sheet_name), ("count", &missing.len().to_string()), ("why", why)]),
            ));
        }
        return lines;
    }
    let error = reply.get("error").and_then(Value::as_str).unwrap_or_default();
    if error == CANCELLED {
        let stopped = solidcraft_ui_text::tr("ui.drawing_export.stopped");
        return vec![(StatusTone::Warning, solidcraft_ui_text::format("ui.drawing_export.failed", &[("sheet", &sheet_name), ("error", stopped)]))];
    }
    // The status lists each view of the sheet that can't be drawn, with its reason: name it as people know it.
    let named: Vec<(StatusTone, String)> = d.map_or_else(Vec::new, |d| {
        views
            .iter()
            .filter(|(_, s, status, _)| *s == sheet && status == "failed")
            .map(|(view, _, _, reason)| {
                let text = solidcraft_ui_text::format(
                    "ui.drawing_export.refused",
                    &[("sheet", &sheet_name), ("view", &view_name(d, *view)), ("reason", reason)],
                );
                (StatusTone::Error, text)
            })
            .collect()
    });
    if named.is_empty() {
        let text = solidcraft_ui_text::format("ui.drawing_export.failed", &[("sheet", &sheet_name), ("error", &crate::friendly_error(ID, error))]);
        vec![(StatusTone::Error, text)]
    } else {
        named
    }
}

/// Take the answers of finished sheets, and close the window after OK once every sheet is written.
fn collect(app: &mut SolveApp) {
    let d = app.session.drawing(ID).ok().cloned();
    let Some(export) = app.drawing_workspace.export.as_mut() else { return };
    let mut finished = false;
    let mut stopped = false;
    let mut lines = Vec::new();
    let views = listed(export);
    export.jobs.retain(|job| match job.reply.try_recv() {
        Ok(reply) => {
            stopped |= reply.get("error").and_then(Value::as_str) == Some(CANCELLED);
            lines.extend(outcome_lines(d.as_deref(), &views, job.sheet, &reply));
            finished = true;
            false
        }
        Err(TryRecvError::Empty) => true,
        Err(TryRecvError::Disconnected) => {
            lines.extend(outcome_lines(
                d.as_deref(),
                &views,
                job.sheet,
                &json!({"ok": false, "error": solidcraft_ui_text::tr("ui.drawing_export.stopped")}),
            ));
            finished = true;
            false
        }
    });
    export.outcome.extend(lines);
    // Cancel here or on the progress card: the window stays to say which sheets were written.
    export.stopping |= stopped;
    if finished && export.jobs.is_empty() {
        let failed = export.outcome.iter().any(|(tone, _)| matches!(tone, StatusTone::Error));
        let stopped = std::mem::take(&mut export.stopping);
        if export.close_when_done && !failed && !stopped {
            app.drawing_workspace.export = None;
        } else {
            export.close_when_done = false;
        }
    }
}

/// Read the drawing's status (`drawing.status`, read-only) when the window opens, after a change to
/// the design and after a view or its dimensions are drawn, so a view that can't be drawn is named
/// before Export is pressed. Nothing else changes it, so it isn't read again until then: each read
/// measures every dimension of the drawing.
fn refresh_status(app: &mut SolveApp) {
    let key = |app: &SolveApp| (app.session.revision, solidcraft_engine::drawing_scene::drawn(&app.session));
    let seen = key(app);
    if app.drawing_workspace.export.as_ref().is_none_or(|e| e.status_read == Some(seen)) {
        return;
    }
    let status = app.session.execute("drawing.status", &json!({})).ok();
    // As read: the read itself may bring the cache to the drawing open.
    let read = key(app);
    if let Some(export) = app.drawing_workspace.export.as_mut() {
        export.status = status;
        export.status_read = Some(read);
    }
}

/// The views of the chosen sheets the status lists, as (view, sheet, status, reason).
fn listed(export: &Export) -> Vec<(u64, u64, String, String)> {
    let views = export.status.as_ref().and_then(|s| s.get("views")).and_then(Value::as_array);
    views
        .into_iter()
        .flatten()
        .filter_map(|v| {
            let view = ids::decode(v.get("view")?.as_str()?)?;
            let sheet = ids::decode(v.get("sheet")?.as_str()?)?;
            let status = v.get("status")?.as_str()?.to_owned();
            let reason = v.get("reason").and_then(Value::as_str).unwrap_or_default().to_owned();
            Some((view, sheet, status, reason))
        })
        .collect()
}

fn close_button(ui: &mut egui::Ui) -> bool {
    let t = Tokens::get();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(t.space_24, t.space_24), egui::Sense::hover());
    let r = ui_kit::icon_button_at(ui, rect, "close", solidcraft_ui_text::tr("ui.drawing_export.close"));
    crate::scenario::publish_target("drawing:export:close", rect);
    r.clicked()
}

/// The window's contents, in the Drawing workspace's right-hand panel host.
pub fn body(app: &mut SolveApp, ui: &mut egui::Ui) {
    collect(app);
    if app.session.open_drawing.is_none() {
        // Its sheets are gone with it: nothing is left to write them for.
        stop(app);
        app.drawing_workspace.export = None;
        return;
    }
    let Ok(d) = app.session.drawing(ID).cloned() else { return };
    refresh_status(app);
    let saved = app.session.path.is_some();
    let design = app.session.doc.name.clone();
    let Some(export) = app.drawing_workspace.export.as_mut() else { return };
    // Sheets deleted while the window is open leave its choice.
    export.sheets.retain(|id| d.sheets.iter().any(|s| s.id == *id));
    let t = Tokens::get();
    let mut close = false;
    ui.horizontal(|ui| {
        ui.heading(solidcraft_ui_text::tr("ui.drawing_export.title"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| close = close_button(ui));
    });
    ui.add_space(t.space_8);

    ui_kit::section_heading(ui, solidcraft_ui_text::tr("ui.drawing_export.format"));
    let planned = solidcraft_ui_text::tr("ui.drawing_export.planned");
    let mut format = match export.format {
        Format::Svg => 1,
        Format::Png => 2,
    };
    let formats = ["PDF", "SVG", "PNG", "DXF"];
    let reasons = [Some(planned), None, None, Some(planned)];
    let targets = ["drawing:export:format:pdf", "drawing:export:format:svg", "drawing:export:format:png", "drawing:export:format:dxf"];
    // Each row of segments under its own id: segments are numbered within their parent.
    let picked = ui
        .push_id("drawing_export_format", |ui| {
            ui_kit::segmented(ui, &mut format, &formats, ui_kit::Segments { disabled_reasons: &reasons, targets: &targets, ..Default::default() })
        })
        .inner;
    if picked {
        export.format = if format == 2 { Format::Png } else { Format::Svg };
    }
    ui.add_space(t.space_8);

    ui_kit::section_heading(ui, solidcraft_ui_text::tr("ui.drawing_export.sheets"));
    let views = listed(export);
    for sheet in &d.sheets {
        let mut on = export.sheets.contains(&sheet.id);
        let label = format!("{} · {}", sheet.name, size_name(&sheet.sheet.size));
        let r = ui_kit::checkbox(ui, &label, &mut on);
        crate::scenario::publish_target(&format!("drawing:export:sheet:{}", sheet.name), r.rect);
        if r.changed() {
            if on {
                export.sheets.insert(sheet.id);
            } else {
                export.sheets.remove(&sheet.id);
            }
        }
        for (view, _, _, reason) in views.iter().filter(|(_, s, status, _)| *s == sheet.id && status == "failed") {
            let text = solidcraft_ui_text::format(
                "ui.drawing_export.view_failed",
                &[("view", &view_name(&d, *view)), ("sheet", &sheet.name), ("reason", reason)],
            );
            ui_kit::status_line(ui, &text, StatusTone::Warning);
        }
    }
    if export.sheets.is_empty() {
        ui_kit::status_line(ui, solidcraft_ui_text::tr("ui.drawing_export.no_sheet"), StatusTone::Warning);
    }
    ui.add_space(t.space_8);

    ui_kit::section_heading(ui, solidcraft_ui_text::tr("ui.drawing_export.paper"));
    let papers = [
        solidcraft_ui_text::tr("ui.drawing_export.paper_white"),
        solidcraft_ui_text::tr("ui.drawing_export.paper_blueprint"),
        solidcraft_ui_text::tr("ui.drawing_export.paper_prussian_blue"),
    ];
    let mut paper = Paper::ALL.iter().position(|p| *p == export.paper).unwrap_or(0);
    let paper_targets = ["drawing:export:paper:white", "drawing:export:paper:blueprint", "drawing:export:paper:prussian_blue"];
    let picked = ui
        .push_id("drawing_export_paper", |ui| {
            ui_kit::segmented(ui, &mut paper, &papers, ui_kit::Segments { targets: &paper_targets, ..Default::default() })
        })
        .inner;
    if picked {
        export.paper = Paper::ALL.get(paper).copied().unwrap_or(Paper::White);
    }
    ui_kit::text(ui, solidcraft_ui_text::tr("ui.drawing_export.paper_note"), ui_kit::Text::Note("drawing_export_paper"));
    ui.add_space(t.space_8);

    ui_kit::section_heading(ui, solidcraft_ui_text::tr("ui.drawing_export.output"));
    if export.format == Format::Png {
        let own = d.sheets.iter().find(|s| export.sheets.contains(&s.id)).map_or(solidcraft_engine::doc::drawing::DEFAULT_DPI, |s| s.dpi);
        let resolution = solidcraft_ui_text::tr("ui.drawing_export.resolution");
        let mut dpi = export.dpi.unwrap_or(own);
        ui_kit::row(ui, resolution, |ui| {
            let r = ui_kit::number_field(ui, &mut dpi, 25.0..=2400.0, 1.0, " dpi");
            crate::scenario::publish_target("drawing:export:dpi", r.rect);
            if r.changed() && dpi.is_finite() {
                export.dpi = Some(dpi.clamp(25.0, 2400.0));
            }
        });
        if export.dpi.is_some() {
            ui_kit::text(ui, solidcraft_ui_text::tr("ui.drawing_export.dpi_set"), ui_kit::Text::Note("drawing_export_dpi"));
            if ui_kit::button(ui, solidcraft_ui_text::tr("ui.drawing_export.dpi_reset"), ui_kit::Button::Normal).clicked() {
                export.dpi = None;
            }
        } else {
            ui_kit::text(ui, solidcraft_ui_text::tr("ui.drawing_export.dpi_note"), ui_kit::Text::Note("drawing_export_dpi"));
        }
        let colour_label = solidcraft_ui_text::tr("ui.drawing_export.colour");
        let colours = [
            solidcraft_ui_text::tr("ui.drawing_export.colour_grey"),
            solidcraft_ui_text::tr("ui.drawing_export.colour_black_white"),
            solidcraft_ui_text::tr("ui.drawing_export.colour_rgb"),
            solidcraft_ui_text::tr("ui.drawing_export.colour_transparent"),
        ];
        let blue = export.paper != Paper::White;
        let only_rgb = solidcraft_ui_text::tr("ui.drawing_export.colour_blue");
        let shown_colour = if blue { Colour::Rgb } else { export.colour };
        let current = Colour::ALL.iter().position(|c| *c == shown_colour).and_then(|at| colours.get(at)).copied().unwrap_or_default();
        ui_kit::row(ui, colour_label, |ui| {
            // A blue paper is always in colour: the choice is shown, and held, until the paper is white.
            ui.add_enabled_ui(!blue, |ui| {
                let r = ui_kit::dropdown_menu(ui, "drawing_export_colour", current, |ui| {
                    for (colour, label) in Colour::ALL.iter().zip(colours) {
                        let r = ui.selectable_label(export.colour == *colour, label);
                        crate::scenario::publish_target(&format!("drawing:export:colour:{}", colour.name()), r.rect);
                        if r.clicked() {
                            export.colour = *colour;
                        }
                    }
                });
                crate::scenario::publish_target("drawing:export:colour", r.rect);
                if blue {
                    r.on_disabled_hover_text(only_rgb);
                }
            });
        });
        if blue {
            ui_kit::text(ui, only_rgb, ui_kit::Text::Note("drawing_export_colour"));
        }
    }
    let folder_label = solidcraft_ui_text::tr("ui.drawing_export.folder");
    if !saved {
        export.folder = Folder::Choose;
    }
    let shown = solidcraft_ui_text::tr(match export.folder {
        Folder::Design => "ui.drawing_export.folder_design",
        Folder::Choose => "ui.drawing_export.folder_choose",
    });
    ui_kit::row(ui, folder_label, |ui| {
        let r = ui_kit::dropdown_menu(ui, "drawing_export_folder", shown, |ui| {
            for (folder, key, enabled) in
                [(Folder::Design, "ui.drawing_export.folder_design", saved), (Folder::Choose, "ui.drawing_export.folder_choose", true)]
            {
                let r = ui.add_enabled_ui(enabled, |ui| ui.selectable_label(export.folder == folder, solidcraft_ui_text::tr(key))).inner;
                crate::scenario::publish_target(
                    &format!("drawing:export:folder:{}", if folder == Folder::Design { "design" } else { "choose" }),
                    r.rect,
                );
                if r.clicked() {
                    export.folder = folder;
                }
            }
        });
        crate::scenario::publish_target("drawing:export:folder", r.rect);
    });
    if !saved {
        ui_kit::text(ui, solidcraft_ui_text::tr("ui.drawing_export.folder_unsaved"), ui_kit::Text::Note("drawing_export_unsaved"));
    }
    let sheets = chosen(&d, export);
    if export.folder == Folder::Design && !sheets.is_empty() {
        let names: Vec<String> = sheets.iter().map(|(_, name)| format!("{}.{}", file_stem(&design, name), export.format.name())).collect();
        let files = solidcraft_ui_text::format("ui.drawing_export.files", &[("files", &names.join(", "))]);
        ui_kit::text(ui, files.as_str(), ui_kit::Text::Note("drawing_export_files"));
    } else if sheets.len() > 1 {
        ui_kit::text(ui, solidcraft_ui_text::tr("ui.drawing_export.several"), ui_kit::Text::Note("drawing_export_files"));
    }
    ui.add_space(t.space_8);
    ui_kit::text(ui, solidcraft_ui_text::tr("ui.drawing_export.paper_info"), ui_kit::Text::Hint("drawing_export_paper_info"));
    for (tone, line) in &export.outcome {
        ui_kit::status_line(ui, line, *tone);
    }
    ui.add_space(t.space_8);

    // The footer says what a held drawing will write as it last was, as the board's does.
    let held = export.status.as_ref().and_then(|s| s.get("held")).and_then(Value::as_bool) == Some(true);
    let stale = views.iter().filter(|(_, sheet, status, _)| export.sheets.contains(sheet) && status == "out_of_date").count();
    let footer_status = if export.busy() {
        Some(solidcraft_ui_text::tr(if export.stopping { "ui.drawing_export.stopping" } else { "ui.drawing_export.exporting" }).to_owned())
    } else if held && stale > 0 {
        let key = if stale == 1 { "ui.drawing_export.out_of_date_one" } else { "ui.drawing_export.out_of_date_other" };
        Some(solidcraft_ui_text::format(key, &[("count", &stale.to_string())]))
    } else {
        None
    };
    let ready = !sheets.is_empty() && !export.busy();
    let action =
        ui_kit::footer(ui, "OK", "Cancel", ready, ui_kit::Footer { apply: Some("Apply"), status: footer_status.as_deref(), ..Default::default() });
    match action {
        ui_kit::Action::Ok if ready => export_now(app, true),
        ui_kit::Action::Apply if ready => export_now(app, false),
        ui_kit::Action::Cancel => close = true,
        _ => {}
    }
    if close {
        crate::drawing_export_panel::close(app);
    }
}

#[cfg(test)]
#[path = "drawing_export_panel_tests.rs"]
mod tests;
