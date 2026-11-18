//! Export the camera at the instant Export is pressed, with a remembered hidden-edge choice
//! and a remembered zero point (the camera target or the model's origin).
use crate::dialogs::form_row;
use crate::{SolveApp, dialogs::Kind, selection::SelInput};
use serde_json::json;

pub fn start(app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    (id == "file.export_view_dxf").then(|| {
        let tolerance = app.preferences.view_dxf_tolerance;
        let tolerance = if tolerance.is_finite() && (0.0001..=1.0).contains(&tolerance) { tolerance } else { 0.01 };
        let zero_origin = app.preferences.view_dxf_zero == "origin";
        (Kind::ViewDxf { dashed: app.preferences.view_dxf_dashed, tolerance, zero_origin }, Vec::new())
    })
}

pub fn rows(app: &SolveApp, ui: &mut egui::Ui, dashed: &mut bool, tolerance: &mut f64, zero_origin: &mut bool) {
    crate::ui_kit::text(ui, app.tr_label("Hidden edges"), crate::ui_kit::Text::Note("hidden"));
    for (value, key, label) in [(false, "omit", "Do not export hidden edges"), (true, "dashed", "Export hidden edges as dashed lines")] {
        let r = crate::ui_kit::list_picker(ui, key, app.tr_label(label), *dashed == value);
        crate::scenario::publish_handle(&format!("dialog:dxf:{key}"), r.rect.center());
        if r.clicked() {
            *dashed = value;
        }
    }
    crate::ui_kit::text(ui, app.tr("dxf.zero"), crate::ui_kit::Text::Note("zero"));
    for (value, key, label) in [(false, "zero_target", "dxf.zero_target"), (true, "zero_origin", "dxf.zero_origin")] {
        let r = crate::ui_kit::list_picker(ui, key, app.tr(label), *zero_origin == value);
        crate::scenario::publish_handle(&format!("dialog:dxf:{key}"), r.rect.center());
        if r.clicked() {
            *zero_origin = value;
        }
    }
    crate::ui_kit::text(ui, app.tr("dxf.zero_help"), crate::ui_kit::Text::Note("zero help"));
    form_row(ui, "dxf.tolerance", |ui| {
        let r = crate::ui_kit::bounded_length_field(ui, tolerance, &app.session.doc.units, 8, 0.0001..=1.0, 0.001);
        crate::scenario::publish_handle("dialog:dxf:tolerance", r.rect.center());
    });
    crate::ui_kit::text(
        ui,
        app.tr_label(if app.cam.fov > 0.0 { "Current perspective view" } else { "Current orthographic view (1:1 mm)" }),
        crate::ui_kit::Text::Note("projection"),
    );
    crate::ui_kit::text(ui, app.tr_label("dxf.tolerance_help"), crate::ui_kit::Text::Note("tolerance"));
}

pub fn run(app: &mut SolveApp, dashed: bool, tolerance: f64, zero_origin: bool) -> Result<(), String> {
    if !crate::viewport::section_planes(app).is_empty() {
        return Err(app.tr_label("Turn off section analysis before exporting the view.").into());
    }
    if !solidcraft_engine::cmd::view_dxf_exportable(&app.session) {
        return Err(app.tr_label("No visible bodies or sketches to export.").into());
    }
    if !tolerance.is_finite() || !(0.0001..=1.0).contains(&tolerance) {
        return Err(app.tr_label("dxf.invalid_tolerance").into());
    }
    app.preferences.view_dxf_dashed = dashed;
    app.preferences.view_dxf_tolerance = tolerance;
    app.preferences.view_dxf_zero = if zero_origin { "origin" } else { "target" }.into();
    let camera = app.cam;
    app.pick_save(&format!("{}-view.dxf", app.session.doc.name), &["dxf"], move |app, path| {
        if let Some(path) = path {
            let params = json!({"camera":camera,"hidden":if dashed {"dashed"} else {"omit"},"tolerance":tolerance,"zero":if zero_origin {"origin"} else {"target"},"path":path});
            let mut request = crate::file_load::Request::new(&path, crate::file_load::Kind::ExportDxf);
            request.options = params.as_object().cloned().unwrap_or_default();
            let _ = crate::file_load::begin_request(app, request);
        }
    });
    Ok(())
}

pub(crate) fn progress_text(work: Option<(&str, usize, usize)>) -> String {
    let (stage, done, total) = work.unwrap_or(("dxf_meshes", 0, 1));
    let label = match stage {
        "dxf_index" => "dxf.progress.index",
        "dxf_projection" => "dxf.progress.projection",
        "dxf_occlusion" => "dxf.progress.occlusion",
        "dxf_write" => "dxf.progress.write",
        _ => "dxf.progress.meshes",
    };
    format!("{}: {done}/{total}", solidcraft_ui_text::tr_label(label))
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_record::Vec3;
    use solidcraft_viewport::StandardView;

    #[test]
    fn view_dxf_file_menu_exports_with_the_current_camera_and_remembers_hidden_choice() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length":40,"width":30,"height":10})).unwrap();
        h.app.run("solid.cylinder", json!({"radius":3,"height":4,"base":[10,10,0],"operation":"cut"})).unwrap();
        h.app.cam.fov = 0.0;
        h.app.cam.set_view(StandardView::Top);
        h.frames(3);
        h.step(&json!({"click":{"handle":"appbar:folder"}})).unwrap();
        h.step(&json!({"widget":"Export Current View as DXF…"})).unwrap();
        assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::ViewDxf { dashed: false, .. })));
        h.step(&json!({"click":{"handle":"dialog:dxf:tolerance"},"double":true})).unwrap();
        h.step(&json!({"key":"A","cmd":true})).unwrap();
        h.step(&json!({"text":"0.1"})).unwrap();
        h.step(&json!({"key":"Tab"})).unwrap();
        h.step(&json!({"click":{"handle":"dialog:dxf:dashed"}})).unwrap();
        let dir = h.dir.clone();
        h.app.services.pick_save = Some(Box::new(move |name, extensions| {
            assert!(name.ends_with("-view.dxf"));
            assert_eq!(extensions, &["dxf"]);
            Box::pin(std::future::ready(Some(dir.join("view.dxf").to_string_lossy().into_owned())))
        }));
        h.step(&json!({"click":{"handle":"dialog:Export DXF…"}})).unwrap();
        wait_for_export(&mut h);
        let text = std::fs::read_to_string(h.dir.join("view.dxf")).unwrap();
        assert!(text.contains("LWPOLYLINE") && text.contains("HIDDEN"));
        assert!(h.app.dialog.is_none() && h.app.preferences.view_dxf_dashed);
        let saved = h.app.prefs();
        let mut restored = SolveApp::new(Default::default(), Default::default());
        restored.load_prefs(&saved);
        assert!(restored.preferences.view_dxf_dashed);
        assert_eq!(restored.preferences.view_dxf_tolerance, 0.1);
        h.app.start("file.export_view_dxf");
        h.frames(3);
        assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::ViewDxf { dashed: true, tolerance: 0.1, .. })));
        h.step(&json!({"click":{"handle":"dialog:dxf:omit"}})).unwrap();
        h.step(&json!({"click":{"handle":"dialog:Export DXF…"}})).unwrap();
        wait_for_export(&mut h);
        let text = std::fs::read_to_string(h.dir.join("view.dxf")).unwrap();
        assert!(!text.contains("LWPOLYLINE") && !text.contains("HIDDEN"));
        assert!(!h.app.preferences.view_dxf_dashed);
    }

    #[test]
    fn inch_dxf_tolerance_is_edited_in_inches_and_kept_in_millimetres() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("document.units", json!({"units":"in"})).unwrap();
        h.app.start("file.export_view_dxf");
        h.frames(3);
        h.step(&json!({"click":{"handle":"dialog:dxf:tolerance"},"double":true})).unwrap();
        h.step(&json!({"key":"A","cmd":true})).unwrap();
        h.step(&json!({"text":"0.001"})).unwrap();
        h.step(&json!({"key":"Tab"})).unwrap();
        let Some(Kind::ViewDxf { tolerance, .. }) = h.app.dialog.as_ref().map(|d| &d.kind) else { panic!("no DXF dialog") };
        assert!((*tolerance - 0.0254).abs() < 1e-10, "{tolerance}");
    }

    #[test]
    fn view_dxf_zero_point_choice_puts_the_model_origin_at_the_drawing_zero() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length":40,"width":30,"height":10})).unwrap();
        // The camera looks at the middle of the plate, so the two zero points differ.
        h.app.cam.fov = 0.0;
        h.app.cam.distance = 150.;
        h.app.cam.target = Vec3::new(20., 15., 5.);
        h.app.cam.set_view(StandardView::Top);
        h.frames(3);
        h.app.start("file.export_view_dxf");
        h.frames(3);
        // By default the drawing is centred on the camera target.
        assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::ViewDxf { zero_origin: false, .. })));
        export_to(&mut h, "target.dxf");
        let (lo, hi) = entity_bounds(&std::fs::read_to_string(h.dir.join("target.dxf")).unwrap());
        assert!(near(lo, -20., -15.) && near(hi, 20., 15.), "{lo:?} {hi:?}");
        // Choosing the model's origin puts the plate's corner at 0,0.
        h.app.start("file.export_view_dxf");
        h.frames(3);
        h.step(&json!({"click":{"handle":"dialog:dxf:zero_origin"}})).unwrap();
        assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::ViewDxf { zero_origin: true, .. })));
        export_to(&mut h, "origin.dxf");
        let (lo, hi) = entity_bounds(&std::fs::read_to_string(h.dir.join("origin.dxf")).unwrap());
        assert!(near(lo, 0., 0.) && near(hi, 40., 30.), "{lo:?} {hi:?}");
        // The choice is remembered, and the dialog opens on it next time.
        assert_eq!(h.app.preferences.view_dxf_zero, "origin");
        let saved = h.app.prefs();
        let mut restored = SolveApp::new(Default::default(), Default::default());
        restored.load_prefs(&saved);
        assert_eq!(restored.preferences.view_dxf_zero, "origin");
        h.app.start("file.export_view_dxf");
        h.frames(3);
        assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::ViewDxf { zero_origin: true, .. })));
    }

    /// Export the open dialog's drawing into the scenario's folder under this name.
    fn export_to(h: &mut crate::scenario::Harness, name: &str) {
        let dir = h.dir.clone();
        let file = name.to_string();
        h.app.services.pick_save = Some(Box::new(move |_name, extensions| {
            assert_eq!(extensions, &["dxf"]);
            Box::pin(std::future::ready(Some(dir.join(&file).to_string_lossy().into_owned())))
        }));
        h.step(&json!({"click":{"handle":"dialog:Export DXF…"}})).unwrap();
        wait_for_export(h);
    }

    /// The drawing's extents: the smallest and the largest x and y of every point the ENTITIES
    /// section draws.
    fn entity_bounds(text: &str) -> ((f64, f64), (f64, f64)) {
        let lines: Vec<&str> = text.lines().collect();
        // The section being read: "pending" is a marker waiting for its name.
        let mut section = "";
        let mut lo = (f64::MAX, f64::MAX);
        let mut hi = (f64::MIN, f64::MIN);
        let mut i = 0;
        while i + 1 < lines.len() {
            let code: i32 = lines[i].trim().parse().unwrap_or(-1);
            let value = lines[i + 1].trim();
            i += 2;
            if code == 0 {
                // A section marker, the end of one, or an entity's type: only the first two
                // change the section being read.
                if value == "SECTION" {
                    section = "pending";
                } else if value == "ENDSEC" {
                    section = "";
                }
                continue;
            }
            if code == 2 && section == "pending" {
                // The name that follows the section marker.
                section = value;
                continue;
            }
            if section != "ENTITIES" {
                continue;
            }
            let (x, y) = match code {
                10 | 11 => (value.parse::<f64>().ok(), None),
                20 | 21 => (None, value.parse::<f64>().ok()),
                _ => continue,
            };
            if let Some(x) = x {
                lo.0 = lo.0.min(x);
                hi.0 = hi.0.max(x);
            }
            if let Some(y) = y {
                lo.1 = lo.1.min(y);
                hi.1 = hi.1.max(y);
            }
        }
        (lo, hi)
    }

    fn near(p: (f64, f64), x: f64, y: f64) -> bool {
        (p.0 - x).abs() < 1e-3 && (p.1 - y).abs() < 1e-3
    }

    fn wait_for_export(h: &mut crate::scenario::Harness) {
        let began = std::time::Instant::now();
        loop {
            h.frames(1);
            if h.app.file_load.idle() && !h.app.file_picker_busy() {
                break;
            }
            assert!(began.elapsed().as_secs() < 10, "export did not finish");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}
