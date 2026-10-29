//! Forms editor fields and topology actions. Geometry changes go through engine commands.
use super::*;
use solidcraft_engine::doc::expr::Kind;

pub(super) struct Fields {
    dimensions: [String; 3],
    segments: [String; 3],
    radii: [String; 2],
    distance: String,
    sharpness: String,
    translation: [String; 3],
    rotation: [String; 3],
    scale: [String; 3],
    pub(super) tolerance: String,
    operation: usize,
    projection_direction: usize,
    projection_controls: bool,
}
impl Default for Fields {
    fn default() -> Self {
        Self {
            dimensions: ["40".into(), "40".into(), "40".into()],
            segments: ["2".into(), "2".into(), "2".into()],
            radii: ["20".into(), "7".into()],
            distance: "5".into(),
            sharpness: "10".into(),
            translation: std::array::from_fn(|_| "0".into()),
            rotation: std::array::from_fn(|_| "0".into()),
            scale: std::array::from_fn(|_| "1".into()),
            tolerance: "0.01".into(),
            operation: 0,
            projection_direction: 0,
            projection_controls: false,
        }
    }
}
pub(super) fn number(app: &SolveApp, value: &str, kind: Kind) -> Option<f64> {
    let parsed = if let Ok(v) = value.parse::<f64>() {
        if matches!(kind, Kind::Angle) { v.to_radians() } else { v }
    } else {
        app.session.doc.eval(value, kind).ok()?
    };
    parsed.is_finite().then_some(parsed)
}
pub(super) fn tolerance(app: &SolveApp) -> Option<f64> {
    number(app, &app.forms_prototype.controls.tolerance, Kind::Length).filter(|v| (1e-6..=1.0).contains(v))
}
fn array(app: &SolveApp, values: &[String; 3], kind: Kind) -> Option<[f64; 3]> {
    Some([number(app, &values[0], kind)?, number(app, &values[1], kind)?, number(app, &values[2], kind)?])
}
pub(super) fn load(app: &mut SolveApp) {
    let Some(id) = app.session.forms_prototype.as_ref().and_then(|f| f.editing_feature) else {
        return;
    };
    if let Some(solidcraft_engine::doc::FeatureKind::Form { tolerance, operation, .. }) = app.session.doc.feature(id).map(|f| &f.kind) {
        app.forms_prototype.controls.tolerance =
            app.session.doc.eval(tolerance.as_str(), solidcraft_engine::doc::expr::Kind::Length).unwrap_or(0.01).to_string();
        app.forms_prototype.controls.operation = match operation {
            solidcraft_engine::doc::Operation::NewBody => 0,
            solidcraft_engine::doc::Operation::Join => 1,
            solidcraft_engine::doc::Operation::Cut => 2,
            solidcraft_engine::doc::Operation::Intersect => 3,
        };
    }
}
pub(super) fn sync(app: &mut SolveApp) {
    if let Some(f) = &app.session.forms_prototype {
        let fields = &mut app.forms_prototype.controls;
        fields.dimensions = f.parameters.dimensions.map(|v| format!("{v:.4}"));
        fields.segments = f.parameters.segments.map(|n| n.to_string());
        fields.radii = f.parameters.radii.map(|v| v.to_string());
    }
}
fn labels(app: &SolveApp) -> [String; 3] {
    ["forms.prototype.x", "forms.prototype.y", "forms.prototype.z"].map(|s| app.tr(s).to_owned())
}
fn inputs(ui: &mut egui::Ui, labels: &[String; 3], values: &mut [String; 3]) {
    for (label, value) in labels.iter().zip(values) {
        crate::ui_kit::expr_row(ui, label, value);
    }
}
pub(super) fn transform_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        if ui.selectable_label(!app.forms_prototype.scale, app.tr("forms.prototype.move_mode")).clicked() {
            app.forms_prototype.scale = false;
        }
        let response = ui.selectable_label(app.forms_prototype.scale, app.tr("forms.prototype.scale_mode"));
        crate::scenario::publish_handle("forms_scale_mode", response.rect.center());
        if response.clicked() {
            app.forms_prototype.scale = true;
        }
    });
    let labels: [String; 3] = ["forms.prototype.global", "forms.prototype.view", "forms.prototype.selection"].map(|s| app.tr(s).to_owned());
    let name = app.tr("forms.prototype.orientation").to_owned();
    crate::ui_kit::choice_row(
        ui,
        &name,
        "forms_orientation",
        &mut app.forms_prototype.orientation,
        &labels.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    let labels: [String; 4] =
        ["forms.prototype.view_plane", "forms.prototype.x", "forms.prototype.y", "forms.prototype.z"].map(|s| app.tr(s).to_owned());
    let name = app.tr("forms.prototype.drag_constraint").to_owned();
    let mut axis = match app.forms_prototype.axis {
        Axis::View => 0,
        Axis::X => 1,
        Axis::Y => 2,
        Axis::Z => 3,
    };
    crate::ui_kit::choice_row(ui, &name, "forms_drag_constraint", &mut axis, &labels.iter().map(String::as_str).collect::<Vec<_>>());
    app.forms_prototype.axis = match axis {
        1 => Axis::X,
        2 => Axis::Y,
        3 => Axis::Z,
        _ => Axis::View,
    };
}
pub(super) fn symmetry(app: &mut SolveApp, ui: &mut egui::Ui) {
    let current = app.session.forms_prototype.as_ref().and_then(|f| f.symmetry.as_ref().map(|s| s.axis));
    let mut selected = current.map_or(0, |n| n + 1);
    let labels: [String; 4] = ["forms.prototype.off", "forms.prototype.yz", "forms.prototype.xz", "forms.prototype.xy"].map(|s| app.tr(s).to_owned());
    let label = app.tr("forms.prototype.symmetry").to_owned();
    crate::ui_kit::choice_row(ui, &label, "forms_symmetry", &mut selected, &labels.iter().map(String::as_str).collect::<Vec<_>>());
    let axis = selected.checked_sub(1);
    if axis != current {
        run(app, "form.prototype.symmetry", json!({"axis":axis}));
    }
    ui.horizontal(|ui| {
        if ui.button(app.tr("forms.prototype.pivot")).clicked() {
            app.forms_prototype.setting_pivot = true;
        }
        if ui.button(app.tr("forms.prototype.reset_pivot")).clicked() {
            app.forms_prototype.pivot = None;
            app.forms_prototype.setting_pivot = false;
        }
    });
}
pub(super) fn parameters(app: &mut SolveApp, ui: &mut egui::Ui) {
    let dimensions = app.tr("forms.prototype.dimensions").to_owned();
    let segments = app.tr("forms.prototype.segments").to_owned();
    let apply = app.tr("forms.prototype.apply_parameters").to_owned();
    let kind = app.session.forms_prototype.as_ref().map_or("box", |f| f.parameters.kind.as_str());
    let rows = parameter_rows(kind);
    let translated = |rows: ParameterRows| rows.iter().map(|&(i, key)| (i, app.tr(key).to_owned())).collect::<Vec<_>>();
    let (dimension_rows, segment_rows, radius_rows) = (translated(rows.0), translated(rows.1), translated(rows.2));
    let locked = app.session.forms_prototype.as_ref().is_some_and(|f| f.hierarchy.is_some() || f.parameters.build().is_ok_and(|c| c != f.cage));
    let mut clicked = false;
    ui.add_enabled_ui(!locked, |ui| {
        let fields = &mut app.forms_prototype.controls;
        if !dimension_rows.is_empty() {
            ui.label(dimensions);
        }
        for (index, label) in &dimension_rows {
            if let Some(value) = fields.dimensions.get_mut(*index) {
                crate::ui_kit::expr_row(ui, label, value);
            }
        }
        for (index, label) in &radius_rows {
            if let Some(value) = fields.radii.get_mut(*index) {
                crate::ui_kit::expr_row(ui, label, value);
            }
        }
        ui.label(segments);
        let eval = |s: &str| s.parse::<f64>().ok();
        for (index, label) in &segment_rows {
            if let Some(value) = fields.segments.get_mut(*index) {
                crate::ui_kit::count_row(ui, label, value, &crate::ui_kit::Count { min: 1, max: 16, eval: &eval });
            }
        }
        let response = ui.button(apply);
        crate::scenario::publish_handle("forms_apply_parameters", response.rect.center());
        clicked = response.clicked();
    });
    if locked {
        ui.add(egui::Label::new(app.tr("forms.prototype.parameters_locked")).wrap());
    }
    if clicked {
        let f = &app.forms_prototype.controls;
        if let (Some(d), Some(s), Some(r0), Some(r1)) = (
            array(app, &f.dimensions, Kind::Length),
            array(app, &f.segments, Kind::Unitless),
            number(app, &f.radii[0], Kind::Length),
            number(app, &f.radii[1], Kind::Length),
        ) {
            if s.iter().all(|v| (1.0..=16.0).contains(v) && v.fract() == 0.0) && d.iter().all(|v| (0.1..=10_000.0).contains(v)) {
                run(app, "form.prototype.parameters", json!({"dimensions":d,"segments":s.map(|v|v as usize),"radii":[r0,r1]}));
            } else {
                app.set_status(app.tr("forms.prototype.invalid_number").to_owned(), true);
            }
        } else {
            app.set_status(app.tr("forms.prototype.invalid_number").to_owned(), true);
        }
    }
}
type ParameterRows = &'static [(usize, &'static str)];
fn parameter_rows(kind: &str) -> (ParameterRows, ParameterRows, ParameterRows) {
    const XYZ: ParameterRows = &[(0, "forms.prototype.x"), (1, "forms.prototype.y"), (2, "forms.prototype.z")];
    const DIAMETER: ParameterRows = &[(0, "forms.prototype.diameter")];
    const RADIAL_HEIGHT: ParameterRows = &[(0, "forms.prototype.radial"), (1, "forms.prototype.height")];
    match kind {
        "plane" | "face" => (&XYZ[..2], &XYZ[..2], &[]),
        "quadball" => (DIAMETER, &[(0, "forms.prototype.subdivisions")], &[]),
        "sphere" => (DIAMETER, &[(0, "forms.prototype.longitude"), (1, "forms.prototype.latitude")], &[]),
        "cylinder" | "pipe" => (&[(0, "forms.prototype.diameter"), (2, "forms.prototype.height")], RADIAL_HEIGHT, &[]),
        "torus" => (
            &[],
            &[(0, "forms.prototype.major_direction"), (1, "forms.prototype.minor_direction")],
            &[(0, "forms.prototype.major_radius"), (1, "forms.prototype.minor_radius")],
        ),
        "tube" => (&[(2, "forms.prototype.height")], RADIAL_HEIGHT, &[(0, "forms.prototype.outer_radius"), (1, "forms.prototype.inner_radius")]),
        _ => (XYZ, XYZ, &[]),
    }
}
pub(super) fn numeric(app: &mut SolveApp, ui: &mut egui::Ui) {
    let labels = labels(app);
    let titles: [String; 3] = ["forms.prototype.translation", "forms.prototype.rotation", "forms.prototype.scale"].map(|s| app.tr(s).to_owned());
    let apply = app.tr("forms.prototype.transform").to_owned();
    let fields = &mut app.forms_prototype.controls;
    ui.label(&titles[0]);
    inputs(ui, &labels, &mut fields.translation);
    ui.label(&titles[1]);
    inputs(ui, &labels, &mut fields.rotation);
    ui.label(&titles[2]);
    inputs(ui, &labels, &mut fields.scale);
    if ui.button(apply).clicked() {
        transform(app);
    }
}
pub(super) fn output(app: &mut SolveApp, ui: &mut egui::Ui) {
    let profile = app.session.forms_prototype.as_ref().is_some_and(|f| f.profile.is_some());
    let tolerance = app.tr("forms.prototype.tolerance").to_owned();
    let operation = app.tr("forms.prototype.operation").to_owned();
    let options: [String; 4] =
        ["forms.prototype.new_body", "forms.prototype.join", "forms.prototype.cut", "forms.prototype.intersect"].map(|s| app.tr(s).to_owned());
    let fields = &mut app.forms_prototype.controls;
    crate::ui_kit::expr_row(ui, &tolerance, &mut fields.tolerance);
    if !profile {
        crate::ui_kit::choice_row(ui, &operation, "forms_operation", &mut fields.operation, &options.iter().map(String::as_str).collect::<Vec<_>>());
    }
}
pub(super) fn topology(app: &mut SolveApp, ui: &mut egui::Ui) {
    let distance = app.tr("forms.prototype.distance").to_owned();
    let sharpness = app.tr("forms.prototype.sharpness").to_owned();
    crate::ui_kit::expr_row(ui, &distance, &mut app.forms_prototype.controls.distance);
    crate::ui_kit::expr_row(ui, &sharpness, &mut app.forms_prototype.controls.sharpness);
    let mut action = None;
    ui.horizontal_wrapped(|ui| {
        for (tool, key) in [
            ("extrude_faces", "forms.prototype.extrude_faces"),
            ("extrude_edges", "forms.prototype.extrude_edges"),
            ("delete_faces", "forms.prototype.delete_faces"),
            ("dissolve", "forms.prototype.dissolve"),
            ("fill_hole", "forms.prototype.fill_hole"),
            ("erase_and_fill", "forms.prototype.erase_and_fill"),
            ("bridge", "forms.prototype.bridge"),
            ("weld", "forms.prototype.weld"),
            ("collapse", "forms.prototype.collapse"),
            ("unweld", "forms.prototype.unweld"),
            ("thicken", "forms.prototype.thicken"),
            ("match", "forms.prototype.match"),
            ("subdivide", "forms.prototype.subdivide"),
            ("crease", "forms.prototype.crease"),
            ("uncrease", "forms.prototype.uncrease"),
        ] {
            let r = ui.button(app.tr(key));
            crate::scenario::publish_handle(&format!("forms_topology_{tool}"), r.rect.center());
            if r.clicked() {
                action = Some(tool);
            }
        }
    });
    if let Some(tool) = action {
        let preview = match tool {
            "thicken" => Some(tool_panel::Mode::Thicken),
            "match" => Some(tool_panel::Mode::Match),
            "fill_hole" => Some(tool_panel::Mode::Fill),
            "bridge" => Some(tool_panel::Mode::Bridge),
            "weld" => Some(tool_panel::Mode::Weld),
            "unweld" => Some(tool_panel::Mode::Unweld),
            "collapse" => Some(tool_panel::Mode::Collapse),
            "erase_and_fill" => Some(tool_panel::Mode::EraseFill),
            _ => None,
        };
        if let Some(mode) = preview {
            tool_panel::start(app, mode);
            return;
        }
        let faces: Vec<_> = surface::selected_faces(app).into_iter().collect();
        let edges: Vec<_> = app.forms_prototype.elements.iter().filter_map(|e| if let Element::Edge(v) = e { Some(*v) } else { None }).collect();
        let vertices: Vec<_> = if matches!(tool, "crease" | "uncrease") && !edges.is_empty() {
            Vec::new()
        } else {
            app.forms_prototype.selected.iter().copied().collect()
        };
        let value = if tool == "crease" { &app.forms_prototype.controls.sharpness } else { &app.forms_prototype.controls.distance };
        if let Some(distance) = number(app, value, if matches!(tool, "crease" | "uncrease") { Kind::Unitless } else { Kind::Length }) {
            let d = app.forms_prototype.axis.vector().unwrap_or(basis(app)[2]) * distance;
            if run(
                app,
                "form.prototype.topology",
                json!({"tool":tool,"faces":faces,"edges":edges,"vertices":vertices,"distance":distance,"displacement":[d.x,d.y,d.z]}),
            ) {
                app.forms_prototype.selected.clear();
                app.forms_prototype.elements.clear();
                app.forms_prototype.pivot = None;
            }
        } else {
            app.set_status(app.tr("forms.prototype.invalid_number").to_owned(), true);
        }
    }
}
pub(super) fn project(app: &mut SolveApp, ui: &mut egui::Ui) {
    let label = app.tr("forms.prototype.projection_direction").to_owned();
    let options: [String; 5] =
        ["forms.prototype.best_fit", "forms.prototype.x", "forms.prototype.y", "forms.prototype.z", "forms.prototype.oriented_normal"]
            .map(|s| app.tr(s).to_owned());
    crate::ui_kit::choice_row(
        ui,
        &label,
        "forms_projection_direction",
        &mut app.forms_prototype.controls.projection_direction,
        &options.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    let label = app.tr("forms.prototype.projection_controls").to_owned();
    ui.checkbox(&mut app.forms_prototype.controls.projection_controls, label);
    let mut action = None;
    ui.horizontal(|ui| {
        let r = ui.button(app.tr("forms.prototype.flatten"));
        crate::scenario::publish_handle("forms_flatten", r.rect.center());
        if r.clicked() {
            action = Some("flatten");
        }
        let r = ui.button(app.tr("forms.prototype.straighten"));
        crate::scenario::publish_handle("forms_straighten", r.rect.center());
        if r.clicked() {
            action = Some("straighten");
        }
    });
    if let Some(tool) = action {
        tool_panel::start(app, if tool == "flatten" { tool_panel::Mode::Flatten } else { tool_panel::Mode::Straighten });
    }
}
pub(super) fn projection_settings(app: &SolveApp) -> (usize, bool) {
    (app.forms_prototype.controls.projection_direction, app.forms_prototype.controls.projection_controls)
}
pub(super) fn operation(app: &SolveApp) -> usize {
    app.forms_prototype.controls.operation
}
pub(super) fn commit_params(app: &SolveApp) -> Result<serde_json::Value, String> {
    let fields = &app.forms_prototype.controls;
    let tolerance = tolerance(app).ok_or_else(|| app.tr("forms.prototype.invalid_tolerance").to_owned())?;
    if app.session.forms_prototype.as_ref().is_some_and(|f| f.profile.is_some()) {
        return Ok(json!({"tolerance":tolerance}));
    }
    let operation = ["new", "join", "cut", "intersect"][fields.operation.min(3)];
    let params = json!({"tolerance":tolerance,"operation":operation});
    Ok(params)
}
fn transform(app: &mut SolveApp) {
    let fields = &app.forms_prototype.controls;
    let (Some(t), Some(r), Some(s)) =
        (array(app, &fields.translation, Kind::Length), array(app, &fields.rotation, Kind::Angle), array(app, &fields.scale, Kind::Unitless))
    else {
        app.set_status(app.tr("forms.prototype.invalid_number").to_owned(), true);
        return;
    };
    if t.iter().chain(&r).chain(&s).any(|v| !v.is_finite()) || s.iter().any(|v| *v <= 0.0) {
        app.set_status(app.tr("forms.prototype.invalid_number").to_owned(), true);
        return;
    }
    let Some(triad) = triad(app) else {
        return;
    };
    let dirs = basis(app);
    let center = triad.center;
    let Some(form) = &app.session.forms_prototype else {
        return;
    };
    let indices: Vec<_> = app.forms_prototype.selected.iter().copied().collect();
    let control = form.hierarchy.is_none() && indices.len() == form.cage.points.len();
    let controls;
    let points = if control {
        controls = form.control_positions();
        &controls
    } else {
        &form.surface.vertices
    };
    let mut positions = Vec::new();
    for &i in &indices {
        let Some(p) = points.get(i) else {
            return;
        };
        let p = *p - center;
        let mut q = dirs.iter().zip(s).fold(Vec3::ZERO, |q, (&a, s)| q + a * (p.dot(a) * s));
        for (axis, angle) in dirs.iter().zip(r) {
            q = crate::gizmo::apply(&crate::gizmo::rotation(*axis, angle), q);
        }
        q += center + dirs.iter().zip(t).fold(Vec3::ZERO, |q, (&a, t)| q + a * t);
        positions.push([q.x, q.y, q.z]);
    }
    run(
        app,
        if control { "form.prototype.set_vertices" } else { "form.prototype.set_surface_vertices" },
        json!({"vertices":indices,"positions":positions}),
    );
}
