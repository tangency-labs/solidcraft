//! The drawing's Export panel (Drawing Review › Output › Export): the file format (PDF, SVG, PNG or
//! DXF), and for a PDF
//! whether it holds the sheet shown or every sheet, one page each, then a file dialog. The file is
//! written by `drawing.export` off the window thread, as an agent's export is
//! ([`crate::file_load::Kind::ExportSheet`]), so a large sheet doesn't stop the window.

use serde_json::{Value, json};
use solidcraft_engine::doc::ids;

use crate::SolveApp;
use crate::drawing_sheet_page::active_sheet;
use crate::ui_kit::{self, Button};

/// The formats offered, first the default: (label id, `format`, extension).
const FORMATS: [(&str, &str, &str); 4] = [
    ("ui.drawing_export.pdf", "pdf", "pdf"),
    ("ui.drawing_export.svg", "svg", "svg"),
    ("ui.drawing_export.png", "png", "png"),
    ("ui.drawing_export.dxf", "dxf", "dxf"),
];

/// The panel's choices, kept while it is open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ExportForm {
    /// Index into [`FORMATS`].
    pub(crate) format: usize,
    /// A PDF of every sheet rather than the sheet shown.
    pub(crate) all_sheets: bool,
}

fn tr(id: &'static str) -> &'static str {
    solidcraft_ui_text::tr(id)
}

/// The `drawing.export` parameters for `form` on the sheet `sheet`, without the path.
pub(crate) fn params(form: ExportForm, sheet: Option<u64>) -> Value {
    let (_, format, _) = FORMATS[form.format.min(FORMATS.len() - 1)];
    match (format, form.all_sheets, sheet) {
        // A PDF naming no sheet holds every sheet, in the drawing's order.
        ("pdf", true, _) | (_, _, None) => json!({"format": format}),
        (_, _, Some(sheet)) => json!({"format": format, "sheet": ids::encode(sheet)}),
    }
}

/// The file name offered: the drawing's name, and the sheet's when only one sheet is written.
fn file_name(app: &SolveApp, form: ExportForm, sheet: Option<u64>) -> String {
    let (_, _, extension) = FORMATS[form.format.min(FORMATS.len() - 1)];
    let drawing = app.session.drawing("drawing.inspect").ok();
    let name = drawing.map(|d| d.name.clone()).filter(|name| !name.is_empty()).unwrap_or_else(|| app.session.doc.name.clone());
    let one = !(FORMATS[form.format.min(FORMATS.len() - 1)].1 == "pdf" && form.all_sheets);
    let sheet_name = drawing.zip(sheet).and_then(|(d, id)| d.sheets.iter().find(|s| s.id == id).map(|s| s.name.clone()));
    match sheet_name {
        Some(sheet) if one && drawing.is_some_and(|d| d.sheets.len() > 1) => format!("{name} - {sheet}.{extension}"),
        _ => format!("{name}.{extension}"),
    }
}

/// Ask where to write it, then write it in the background.
pub(crate) fn export(app: &mut SolveApp, form: ExportForm) {
    let sheet = active_sheet(app);
    let params = params(form, sheet);
    let (_, _, extension) = FORMATS[form.format.min(FORMATS.len() - 1)];
    let name = file_name(app, form, sheet);
    app.pick_save(&name, &[extension], move |app, path| {
        let Some(path) = path else { return };
        let mut request = crate::file_load::Request::new(&path, crate::file_load::Kind::ExportSheet);
        request.options = params.as_object().cloned().unwrap_or_default();
        request.options.insert("path".into(), json!(path));
        if let Err(error) = crate::file_load::begin_request(app, request) {
            app.set_status(error, true);
        }
    });
}

/// The panel: its rows, then Cancel and Export. True when it closes.
pub(super) fn panel(app: &mut SolveApp, ui: &mut egui::Ui) -> bool {
    let mut form = app.drawing_workspace.export.unwrap_or_default();
    let sheets = app.session.drawing("drawing.inspect").map(|d| d.sheets.len()).unwrap_or_default();
    let (cancel, go) = super::inset(ui, |ui| {
        let labels = FORMATS.map(|(label, _, _)| tr(label));
        let r = ui_kit::choice_row(ui, tr("ui.drawing_export.format"), "drawing:export:format", &mut form.format, &labels);
        crate::scenario::publish_handle("drawing:export:format", r.rect.center());
        if FORMATS[form.format.min(FORMATS.len() - 1)].1 == "pdf" {
            let mut pages = usize::from(form.all_sheets);
            let options = [tr("ui.drawing_export.this_sheet"), tr("ui.drawing_export.all_sheets")];
            let r = ui_kit::choice_row(ui, tr("ui.drawing_export.sheets"), "drawing:export:sheets", &mut pages, &options);
            crate::scenario::publish_handle("drawing:export:sheets", r.rect.center());
            form.all_sheets = pages == 1;
            let note = if form.all_sheets && sheets > 1 { "ui.drawing_export.pdf_pages" } else { "ui.drawing_export.pdf_note" };
            ui_kit::text(ui, tr(note), ui_kit::Text::Note(note));
        } else {
            let note =
                if FORMATS[form.format.min(FORMATS.len() - 1)].1 == "dxf" { "ui.drawing_export.dxf_note" } else { "ui.drawing_export.one_sheet" };
            ui_kit::text(ui, tr(note), ui_kit::Text::Note(note));
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let go = ui_kit::button(ui, tr("ui.drawing_export.export"), Button::Normal);
                crate::scenario::publish_handle("drawing:export:go", go.rect.center());
                let cancel = ui_kit::button(ui, tr("ui.cancel"), Button::Normal).clicked();
                (cancel, go.clicked())
            })
            .inner
        })
        .inner
    });
    app.drawing_workspace.export = Some(form);
    if go {
        export(app, form);
    }
    cancel || go
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Harness;

    fn wait_for_export(h: &mut Harness) {
        let began = std::time::Instant::now();
        loop {
            h.frames(1);
            if h.app.file_load.idle() && !h.app.file_picker_busy() {
                break;
            }
            assert!(began.elapsed().as_secs() < 30, "export did not finish");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    /// Drawing Review › Output › Export opens its panel; PDF is the first format, and with All
    /// sheets the file is one PDF page per sheet. The file name offered is the drawing's.
    #[test]
    fn the_ribbons_export_writes_a_pdf_of_every_sheet() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", json!({"length":40,"width":20,"height":3})).unwrap();
        super::super::enter(&mut h.app, true).unwrap();
        h.frames(4);
        h.app.run("drawing.create", json!({})).unwrap();
        h.app.run("drawing.sheet.add", json!({})).unwrap();
        h.frames(4);
        h.app.ui.tab = "DRAWING REVIEW".into();
        h.frames(4);
        h.step(&json!({"click":{"handle":"button:drawing.export"}})).unwrap();
        h.frames(4);
        assert_eq!(h.app.drawing_workspace.options, Some(super::super::Tool::Export));
        h.app.drawing_workspace.export = Some(ExportForm { format: 0, all_sheets: true });
        h.frames(2);
        let dir = h.dir.clone();
        h.app.services.pick_save = Some(Box::new(move |name, extensions| {
            assert!(name.ends_with(".pdf"), "{name}");
            assert_eq!(extensions, &["pdf"]);
            Box::pin(std::future::ready(Some(dir.join("drawing.pdf").to_string_lossy().into_owned())))
        }));
        h.step(&json!({"click":{"handle":"drawing:export:go"}})).unwrap();
        wait_for_export(&mut h);
        let bytes = std::fs::read(h.dir.join("drawing.pdf")).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(bytes.starts_with(b"%PDF-") && text.contains("/Count 2"), "two pages");
        assert!(h.app.drawing_workspace.options.is_none(), "the panel closes");
    }

    /// The parameters each choice gives `drawing.export`.
    #[test]
    fn each_choice_names_its_sheets_and_format() {
        let sheet = Some(7);
        let id = ids::encode(7);
        assert_eq!(params(ExportForm { format: 0, all_sheets: true }, sheet), json!({"format": "pdf"}));
        assert_eq!(params(ExportForm { format: 0, all_sheets: false }, sheet), json!({"format": "pdf", "sheet": id}));
        assert_eq!(params(ExportForm { format: 1, all_sheets: true }, sheet), json!({"format": "svg", "sheet": id}), "svg is one sheet");
        assert_eq!(params(ExportForm { format: 2, all_sheets: false }, sheet), json!({"format": "png", "sheet": id}));
        assert_eq!(params(ExportForm { format: 3, all_sheets: true }, sheet), json!({"format": "dxf", "sheet": id}), "dxf is one sheet");
    }
}
