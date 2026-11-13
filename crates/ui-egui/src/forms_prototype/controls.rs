//! Forms editor fields and topology actions. Geometry changes go through engine commands.
use super::*;
use crate::ui_kit::BackingTooltipExt as _;
use solidcraft_engine::doc::expr::Kind;
use solidcraft_engine::schema::forms_prototype::FinishOptions;

const OPERATIONS: [&str; 4] = ["new", "join", "cut", "intersect"];

#[derive(Clone, Copy)]
struct Closure {
    source: (u64, u64, u64),
    closed: Option<bool>,
}

pub(super) struct Fields {
    closure: std::cell::Cell<Option<Closure>>,
    #[cfg(test)]
    closure_scans: std::cell::Cell<usize>,
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
            closure: Default::default(),
            #[cfg(test)]
            closure_scans: Default::default(),
            dimensions: ["40".into(), "40".into(), "40".into()],
            segments: ["2".into(), "2".into(), "2".into()],
            radii: ["20".into(), "7".into()],
            distance: "5".into(),
            sharpness: "10".into(),
            translation: std::array::from_fn(|_| "0".into()),
            rotation: std::array::from_fn(|_| "0".into()),
            scale: std::array::from_fn(|_| "1".into()),
            tolerance: "0.01 mm".into(),
            operation: 0,
            projection_direction: 0,
            projection_controls: false,
        }
    }
}
// Known topology eligibility is queried for several buttons each frame. Keep
// the exact boundary result until the live Form identity or source changes.
// An invalid source remains unknown and is qualified by the engine on entry.
pub(super) fn source_closed(app: &SolveApp) -> Option<bool> {
    let form = app.session.forms_prototype.as_ref()?;
    let (identity, revision) = form.source_revision();
    let source = (app.session.uid, identity, revision);
    let fields = &app.forms_prototype.controls;
    if let Some(cached) = fields.closure.get()
        && cached.source == source
    {
        return cached.closed;
    }
    let closed = form.cage.boundary_loops().ok().map(|loops| loops.is_empty());
    fields.closure.set(Some(Closure { source, closed }));
    #[cfg(test)]
    fields.closure_scans.set(fields.closure_scans.get().saturating_add(1));
    closed
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
    let Some(form) = app.session.forms_prototype.as_ref() else { return };
    if let Some(solidcraft_engine::doc::FeatureKind::Form { tolerance, operation, .. }) =
        form.editing_feature.and_then(|id| app.session.doc.feature(id)).map(|feature| &feature.kind)
    {
        app.forms_prototype.controls.tolerance = tolerance.as_str().to_owned();
        app.forms_prototype.controls.operation = match operation {
            solidcraft_engine::doc::Operation::NewBody => 0,
            solidcraft_engine::doc::Operation::Join => 1,
            solidcraft_engine::doc::Operation::Cut => 2,
            solidcraft_engine::doc::Operation::Intersect => 3,
        };
    }
    let fields = &mut app.forms_prototype.controls;
    if let Some(tolerance) = &form.finish_options.tolerance {
        fields.tolerance.clone_from(tolerance);
    }
    if let Some(operation) = form.finish_options.operation.as_deref().and_then(|name| OPERATIONS.iter().position(|&value| value == name)) {
        fields.operation = operation;
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
pub(super) fn transform_mode(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let response = crate::ui_kit::button(
            ui,
            app.tr("forms.prototype.move_mode"),
            if !app.forms_prototype.scale { crate::ui_kit::Button::SmallSelected } else { crate::ui_kit::Button::Small },
        );
        crate::scenario::publish_target("forms_move_mode", response.rect);
        if response.clicked() {
            app.forms_prototype.scale = false;
        }
        let response = crate::ui_kit::button(
            ui,
            app.tr("forms.prototype.scale_mode"),
            if app.forms_prototype.scale { crate::ui_kit::Button::SmallSelected } else { crate::ui_kit::Button::Small },
        );
        crate::scenario::publish_target("forms_scale_mode", response.rect);
        if response.clicked() {
            app.forms_prototype.scale = true;
        }
    });
}
pub(super) fn transform_frame(app: &mut SolveApp, ui: &mut egui::Ui) {
    let labels: [String; 3] = ["forms.prototype.global", "forms.prototype.view", "forms.prototype.selection"].map(|s| app.tr(s).to_owned());
    let name = app.tr("forms.prototype.orientation").to_owned();
    crate::ui_kit::choice_row(
        ui,
        &name,
        "forms_orientation",
        &mut app.forms_prototype.orientation,
        &labels.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    if app.forms_prototype.orientation == 2
        && !app.forms_prototype.elements.is_empty()
        && let Err(key) = basis(app)
    {
        ui.colored_label(Tokens::get().warning, app.tr(key));
    }
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
        if crate::ui_kit::button(ui, app.tr("forms.prototype.pivot"), crate::ui_kit::Button::Normal).clicked() {
            app.forms_prototype.setting_pivot = true;
        }
        if crate::ui_kit::button(ui, app.tr("forms.prototype.reset_pivot"), crate::ui_kit::Button::Normal).clicked() {
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
        let response = crate::ui_kit::button(ui, apply, crate::ui_kit::Button::Normal);
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
                if let Some(tolerance) = tolerance(app) {
                    finish::command(
                        app,
                        ui.ctx(),
                        "form.prototype.parameters",
                        json!({"dimensions":d,"segments":s.map(|v|v as usize),"radii":[r0,r1],"tolerance":tolerance}),
                    );
                } else {
                    app.set_status(app.tr("forms.prototype.invalid_number").to_owned(), true);
                }
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
    if surface::cap_selected(app) {
        ui.add(egui::Label::new(app.tr("forms.fill.cap_rim_editing")).wrap());
        return;
    }
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
    if crate::ui_kit::button(ui, apply, crate::ui_kit::Button::Normal).clicked() {
        transform(app);
    }
}
pub(super) fn output(app: &mut SolveApp, ui: &mut egui::Ui) {
    let previous = (app.forms_prototype.controls.tolerance.clone(), app.forms_prototype.controls.operation);
    let profile = app.session.forms_prototype.as_ref().is_some_and(|f| f.profile.is_some());
    let tolerance = app.tr("forms.prototype.tolerance").to_owned();
    let operation = app.tr("forms.prototype.operation").to_owned();
    let options: [String; 4] =
        ["forms.prototype.new_body", "forms.prototype.join", "forms.prototype.cut", "forms.prototype.intersect"].map(|s| app.tr(s).to_owned());
    let fields = &mut app.forms_prototype.controls;
    crate::ui_kit::expr_row(ui, &tolerance, &mut fields.tolerance);
    if !profile {
        crate::ui_kit::choice_row(ui, &operation, "forms_operation", &mut fields.operation, &options.iter().map(String::as_str).collect::<Vec<_>>());
        if fields.operation != 0 {
            let label = app.tr("forms.prototype.show_combination").to_owned();
            let response = crate::ui_kit::checkbox(ui, &label, &mut app.forms_prototype.show_combination);
            crate::scenario::publish_handle("forms_show_combination", response.rect.center());
            if let Some(Err(error)) = combine_preview::result(app) {
                let detail = solidcraft_ui_text::format("forms.prototype.combination_error", &[("error", error)]);
                crate::ui_kit::text(ui, &detail, crate::ui_kit::Text::Note("form_combination_error"));
            }
        }
    }
    let fields = &app.forms_prototype.controls;
    let update = FinishOptions {
        tolerance: (fields.tolerance != previous.0 && self::tolerance(app).is_some()).then(|| fields.tolerance.clone()),
        operation: if !profile && fields.operation != previous.1 { OPERATIONS.get(fields.operation).map(|name| (*name).to_owned()) } else { None },
    };
    if (update.tolerance.is_some() || update.operation.is_some())
        && let Err(error) = app.session.execute("form.prototype.finish_options", &json!(update))
    {
        app.set_status(error.to_string(), true);
    }
}
/// A cap removal rebuilds and validates CAD on the same worker as Finish.
pub(super) fn delete_selected_cap(app: &mut SolveApp, ctx: &egui::Context) -> bool {
    let Some(cap) = surface::selected_cap(app) else { return false };
    if let Some(reason) = tool_panel::topology_unavailable(app, "delete_faces") {
        app.set_status(app.tr(reason).to_owned(), true);
        return true;
    }
    let Some(tolerance) = tolerance(app) else {
        app.set_status(app.tr("forms.prototype.invalid_tolerance").to_owned(), true);
        return true;
    };
    finish::command(app, ctx, "form.prototype.topology", json!({"tool":"delete_faces","faces":[cap],"tolerance":tolerance}));
    true
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
            let reason = tool_panel::topology_unavailable(app, tool);
            // The button must take part in this row's wrap calculation.
            // An enabled child scope allocates only after drawing its content.
            let r = ui.add_enabled(reason.is_none(), |ui: &mut egui::Ui| crate::ui_kit::button(ui, app.tr(key), crate::ui_kit::Button::Normal));
            let r = if let Some(reason) = reason { r.backing_on_disabled_hover_text(app.tr(reason)) } else { r };
            crate::scenario::publish_target(&format!("forms_topology_{tool}"), r.rect);
            if r.clicked() {
                action = Some(tool);
            }
        }
    });
    if let Some(tool) = action {
        if let Some(reason) = tool_panel::topology_unavailable(app, tool) {
            app.set_status(app.tr(reason).to_owned(), true);
            return;
        }
        if let Some(mode) = tool_panel::topology_mode(tool) {
            tool_panel::start(app, mode);
            return;
        }
        if tool == "delete_faces" && delete_selected_cap(app, ui.ctx()) {
            return;
        }
        let faces: Vec<_> = surface::selected_faces(app).into_iter().collect();
        let edges: Vec<_> = app.forms_prototype.elements.iter().filter_map(|e| if let Element::Edge(v) = e { Some(*v) } else { None }).collect();
        let vertices: Vec<_> = if matches!(tool, "crease" | "uncrease") && !edges.is_empty() {
            Vec::new()
        } else {
            app.forms_prototype.selected.iter().copied().collect()
        };
        let distance = match tool {
            "uncrease" => Some(0.0),
            "crease" => number(app, &app.forms_prototype.controls.sharpness, Kind::Unitless),
            _ => number(app, &app.forms_prototype.controls.distance, Kind::Length),
        };
        if let Some(distance) = distance {
            if matches!(tool, "extrude_faces" | "extrude_edges") {
                let dirs = match basis(app) {
                    Ok(dirs) => dirs,
                    Err(key) => {
                        app.set_status(app.tr(key).to_owned(), true);
                        return;
                    }
                };
                let direction = app.forms_prototype.axis.vector(constraint_axes(app, dirs)).unwrap_or(dirs[2]);
                let d = solidcore_ops::form_transform::translate(Vec3::ZERO, direction, distance);
                // Numeric extrusion has the same evaluated-surface displacement
                // contract as dragging. Keep legacy raw topology commands intact.
                let (faces, edges) = if tool == "extrude_faces" { (faces, Vec::new()) } else { (Vec::new(), edges) };
                finish::command(
                    app,
                    ui.ctx(),
                    "form.prototype.extrude_drag",
                    json!({"faces":faces,"edges":edges,"displacement":[d.x,d.y,d.z],"surface":true,"checkpoint":true}),
                );
                return;
            }
            let params = json!({"tool":tool,"faces":faces,"edges":edges,"vertices":vertices,"distance":distance});
            if matches!(tool, "crease" | "uncrease") && app.session.forms_prototype.as_ref().is_some_and(|form| form.hierarchy.is_some()) {
                finish::command(app, ui.ctx(), "form.prototype.topology", params);
                return;
            }
            if run(app, "form.prototype.topology", params) {
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
    crate::ui_kit::checkbox(ui, &label, &mut app.forms_prototype.controls.projection_controls);
    let mut action = None;
    ui.horizontal(|ui| {
        let r = crate::ui_kit::button(ui, app.tr("forms.prototype.flatten"), crate::ui_kit::Button::Normal);
        crate::scenario::publish_handle("forms_flatten", r.rect.center());
        if r.clicked() {
            action = Some("flatten");
        }
        let r = crate::ui_kit::button(ui, app.tr("forms.prototype.straighten"), crate::ui_kit::Button::Normal);
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
    let operation = OPERATIONS.get(fields.operation).copied().unwrap_or("new");
    let params = json!({"tolerance":tolerance,"operation":operation});
    Ok(params)
}
pub(super) fn finish_params(app: &mut SolveApp) -> Result<serde_json::Value, String> {
    let mut params = commit_params(app)?;
    let tolerance = app.forms_prototype.controls.tolerance.clone();
    app.session.execute("form.prototype.finish_options", &json!({"tolerance":tolerance})).map_err(|error| error.to_string())?;
    // Preview/export need a numeric tolerance. Finish retains its validated
    // authored expression through the editor's durable finish options.
    if let Some(params) = params.as_object_mut() {
        params.remove("tolerance");
    }
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
    let dirs = match basis(app) {
        Ok(dirs) => dirs,
        Err(key) => {
            app.set_status(app.tr(key).to_owned(), true);
            return;
        }
    };
    let center = triad.center;
    let targets = match soft_selection::targets(app, false) {
        Ok(targets) => targets,
        Err(error) => {
            app.set_status(error, true);
            return;
        }
    };
    let positions: Option<Vec<_>> = targets
        .points
        .iter()
        .zip(&targets.weights)
        .map(|(&point, &weight)| solidcore_ops::form_transform::numeric(point, center, dirs, t, r, s, weight).map(|p| [p.x, p.y, p.z]))
        .collect();
    let Some(positions) = positions else {
        app.set_status(app.tr("forms.prototype.invalid_number").to_owned(), true);
        return;
    };
    run(
        app,
        if targets.control { "form.prototype.set_vertices" } else { "form.prototype.set_surface_vertices" },
        json!({"vertices":targets.indices,"positions":positions}),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click_topology(app: &mut SolveApp, ctx: &egui::Context, tool: &str) {
        let mut draw = |events| {
            let _ = crate::test_support::frame(
                ctx,
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0))), events, ..Default::default() },
                |ui| topology(app, ui),
            );
        };
        draw(Vec::new());
        let pos = crate::scenario::handle_at(&format!("forms_topology_{tool}")).unwrap();
        draw(vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE },
        ]);
        draw(vec![egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE }]);
    }

    #[test]
    fn uncrease_does_not_read_the_distance_field() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.run("form.prototype.create", json!({"primitive":"box","segments":[1,1,1],"levels":0})).unwrap();
        let edge = app.session.forms_prototype.as_ref().unwrap().cage.edges().unwrap()[0];
        app.run("form.prototype.topology", json!({"tool":"crease","edges":[edge],"distance":2.0})).unwrap();
        let form = app.session.forms_prototype.as_ref().unwrap();
        assert!(!form.cage.edge_creases.is_empty());
        app.forms_prototype.elements.insert(Element::Edge(edge));
        app.forms_prototype.controls.distance = "2 mm".into();

        click_topology(&mut app, &egui::Context::default(), "uncrease");

        assert!(app.session.forms_prototype.as_ref().unwrap().cage.edge_creases.is_empty());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn opening_topology_keeps_the_panel_width_and_the_refined_fill_button_clickable() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.preferences.language = "en".into();
        app.run("form.prototype.create", json!({"primitive":"face","dimensions":[9,9,1],"segments":[3,3,1],"levels":0})).unwrap();
        app.run("form.prototype.refine", json!({"faces":[4],"counts":[2,2]})).unwrap();
        let f = app.session.forms_prototype.as_ref().unwrap();
        let leaf = f.hierarchy.as_ref().unwrap().leaves.iter().position(|leaf| leaf.active && leaf.root == 4).unwrap();
        app.run("form.prototype.topology", json!({"tool":"delete_faces","faces":[leaf]})).unwrap();
        let hole = app.session.forms_prototype.as_ref().unwrap().definition();
        let rims = solidcraft_engine::doc::forms::match_openings(&hole.cage, hole.hierarchy.as_ref()).unwrap();
        let rim = rims.iter().find(|rim| rim.len() == 4).unwrap();
        let edge = [rim[0].min(rim[1]), rim[0].max(rim[1])];
        app.forms_prototype.elements.insert(Element::Edge(edge));
        selection::sync(&mut app);
        assert!(tool_panel::topology_unavailable(&app, "fill_hole").is_none());

        let ctx = egui::Context::default();
        crate::fonts::setup_bundled_fonts(&ctx);
        let draw = |app: &mut SolveApp, events| {
            crate::scenario::clear_handles();
            let mut viewport = egui::Rect::NOTHING;
            let _ = crate::test_support::frame(
                &ctx,
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1600.0, 1150.0))),
                    focused: true,
                    events,
                    ..Default::default()
                },
                |ui| {
                    panel::show(app, ui);
                    viewport = ui.available_rect_before_wrap();
                },
            );
            viewport
        };
        draw(&mut app, Vec::new());
        let closed = draw(&mut app, Vec::new());
        let header = crate::scenario::handle_at("forms.prototype.topology_section").unwrap();
        draw(&mut app, crate::test_support::pointer(header, true));
        draw(&mut app, crate::test_support::pointer(header, false));
        let opened = draw(&mut app, Vec::new());
        assert!((opened.right() - closed.right()).abs() < 1.0, "opening Topology must keep the allocated panel width");
        let actions: std::collections::BTreeMap<_, _> = crate::scenario::rects("forms_topology_").into_iter().collect();
        assert_eq!(actions.len(), 15);
        assert!(actions.values().all(|rect| rect.left() >= opened.right() - 1.0 && rect.right() <= 1600.0));
        let first_row = actions.values().next().unwrap().top();
        assert!(actions.values().any(|rect| (rect.top() - first_row).abs() > 1.0));

        let fill = crate::scenario::handle_at("forms_topology_fill_hole").unwrap();
        draw(&mut app, crate::test_support::pointer(fill, true));
        draw(&mut app, crate::test_support::pointer(fill, false));
        assert_eq!(tool_panel::inspect(&app)["mode"], "forms.prototype.fill_hole");
        tool_panel::cancel(&mut app);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), hole);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn numeric_extrude_face_buttons_target_evaluated_points_like_drag_with_one_undo() {
        for fixture in ["box", "face", "placed", "refined_mirror"] {
            let mut app = SolveApp::new(Default::default(), Default::default());
            let primitive = if fixture == "box" { "box" } else { "face" };
            app.session
                .execute("form.prototype.create", &json!({"primitive":primitive,"dimensions":[16,12,10],"segments":[4,3,3],"levels":1}))
                .unwrap();
            if fixture == "placed" {
                app.session
                    .execute(
                        "form.prototype.transform",
                        &json!({"matrix":solidcraft_engine::doc::rigid(Vec3::new(11.0,-7.0,3.0),Vec3::ZERO,Vec3::Y,0.4)}),
                    )
                    .unwrap();
            }
            if fixture == "refined_mirror" {
                app.session.execute("form.prototype.symmetry", &json!({"axis":0})).unwrap();
                let root = {
                    let form = app.session.forms_prototype.as_ref().unwrap();
                    form.logical.faces.iter().position(|face| face.iter().all(|&v| form.logical.points[v].x >= 4.0)).unwrap()
                };
                app.session.execute("form.prototype.refine", &json!({"faces":[root],"counts":[2,2]})).unwrap();
            }
            let form = app.session.forms_prototype.as_ref().unwrap();
            let source = form.definition();
            let face = if fixture == "refined_mirror" {
                form.logical
                    .faces
                    .iter()
                    .enumerate()
                    .skip(source.hierarchy.as_ref().unwrap().base.faces.len())
                    .find(|(_, face)| !face.is_empty() && face.iter().all(|&v| form.logical.points[v].x > 3.5))
                    .map(|(id, _)| id)
                    .unwrap()
            } else {
                form.logical
                    .faces
                    .iter()
                    .enumerate()
                    .filter(|(_, face)| face.len() == 4)
                    .max_by(|(_, a), (_, b)| {
                        let center = |face: &&Vec<usize>| face.iter().map(|&i| form.surface.vertices[i].z).sum::<f64>() / 4.0;
                        center(a).total_cmp(&center(b))
                    })
                    .map(|(id, _)| id)
                    .unwrap()
            };
            let original: Vec<_> = form.logical.faces[face].iter().map(|&id| form.surface.vertices[id]).collect();
            let undo = form.inspect()["undo"].as_u64().unwrap();
            let mut drag = app.session.scratch();
            drag.execute("form.prototype.extrude_drag", &json!({"faces":[face],"displacement":[0,0,2],"surface":true,"checkpoint":true})).unwrap();
            app.forms_prototype.elements.insert(Element::Face(face));
            selection::sync(&mut app);
            app.forms_prototype.pivot = Some(Vec3::new(3.0, 4.0, 5.0));
            app.forms_prototype.axis = Axis::Z;
            app.forms_prototype.controls.distance = "0.2 cm".into();
            let before = solidcraft_engine::recovery::capture_bytes(&app.session).unwrap();
            click_topology(&mut app, &egui::Context::default(), "extrude_faces");
            assert!(app.forms_prototype.pending.is_some(), "{fixture}: {:?}", app.status);
            assert_eq!(solidcraft_engine::recovery::capture_bytes(&app.session).unwrap(), before, "queued work cannot author a partial source");
            finish::flush_for_test(&mut app).unwrap();
            let form = app.session.forms_prototype.as_ref().unwrap();
            assert_eq!(form.definition(), drag.forms_prototype.as_ref().unwrap().definition(), "{fixture}: numeric/drag parity");
            assert!(!form.has_tool_preview(), "one-shot extrusion must not leave an Apply/Cancel preview open");
            assert_eq!(form.inspect()["undo"].as_u64(), Some(undo + 1));
            for (&id, original) in form.logical.faces[face].iter().zip(original) {
                assert!(form.surface.vertices[id].dist(original + Vec3::Z * 2.0) < 1e-6, "{fixture}: cap must move evaluated2mm");
            }
            if let Some(symmetry) = &form.symmetry {
                symmetry.validate_reflection(&form.cage).unwrap();
            }
            let extruded = form.definition();
            assert!(app.forms_prototype.elements.is_empty());
            assert!(app.forms_prototype.selected.is_empty());
            assert!(app.forms_prototype.pivot.is_none());
            app.session.execute("form.prototype.undo", &json!({})).unwrap();
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), source);
            app.session.execute("form.prototype.redo", &json!({})).unwrap();
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), extruded);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn numeric_edge_buttons_use_evaluated_boundary_and_trim_rim_targets() {
        for trimmed in [false, true] {
            let mut app = SolveApp::new(Default::default(), Default::default());
            app.session.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[20,20,1],"segments":[4,4,1],"levels":1})).unwrap();
            let edge = if trimmed {
                app.session.execute("form.prototype.refine", &json!({"faces":[5],"counts":[2,2]})).unwrap();
                let old = app.session.forms_prototype.as_ref().unwrap().logical.faces[16].clone();
                app.session.execute("form.prototype.topology", &json!({"tool":"delete_faces","faces":[16]})).unwrap();
                let form = app.session.forms_prototype.as_ref().unwrap();
                let h = form.hierarchy.as_ref().unwrap();
                old.iter()
                    .copied()
                    .zip(old.iter().copied().cycle().skip(1))
                    .take(old.len())
                    .find_map(|(a, b)| h.trim_attachments(&[[a, b]]).ok()?.into_iter().find(|t| t.root == 5).map(|t| t.edge))
                    .unwrap()
            } else {
                let ring = app.session.forms_prototype.as_ref().unwrap().cage.boundary_loops().unwrap().remove(0);
                [ring[0], ring[1]]
            };
            let edge = [edge[0].min(edge[1]), edge[0].max(edge[1])];
            let form = app.session.forms_prototype.as_ref().unwrap();
            let source = form.definition();
            let undo = form.inspect()["undo"].as_u64().unwrap();
            let original = form.surface.vertices.clone();
            let mut drag = app.session.scratch();
            drag.execute("form.prototype.extrude_drag", &json!({"edges":[edge],"displacement":[0,0,2],"surface":true})).unwrap();
            app.forms_prototype.elements.insert(Element::Edge(edge));
            selection::sync(&mut app);
            app.forms_prototype.axis = Axis::Z;
            app.forms_prototype.controls.distance = "2 mm".into();
            click_topology(&mut app, &egui::Context::default(), "extrude_edges");
            finish::flush_for_test(&mut app).unwrap();
            let form = app.session.forms_prototype.as_ref().unwrap();
            assert_eq!(form.definition(), drag.forms_prototype.as_ref().unwrap().definition(), "trimmed={trimmed}: {:?}", app.status);
            let wall = form.logical.faces.last().unwrap();
            for (old, new) in [(wall[0], wall[3]), (wall[1], wall[2])] {
                assert!(form.surface.vertices[new].dist(original[old] + Vec3::Z * 2.0) < 1e-6);
            }
            assert_eq!(form.inspect()["undo"].as_u64(), Some(undo + 1));
            let extruded = form.definition();
            app.session.execute("form.prototype.undo", &json!({})).unwrap();
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), source);
            app.session.execute("form.prototype.redo", &json!({})).unwrap();
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), extruded);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn numeric_extrusion_invalid_distance_and_interior_edges_keep_source_selection_and_pivot() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[20,20,1],"segments":[4,4,1],"levels":1})).unwrap();
        let face = app.session.forms_prototype.as_ref().unwrap().logical.faces[5].clone();
        app.forms_prototype.elements.insert(Element::Face(5));
        selection::sync(&mut app);
        app.forms_prototype.pivot = Some(Vec3::new(3.0, 4.0, 5.0));
        app.forms_prototype.controls.distance = "invalid distance".into();
        let source = solidcraft_engine::recovery::capture_bytes(&app.session).unwrap();
        let selected = app.forms_prototype.selected.clone();
        let elements = app.forms_prototype.elements.clone();
        let pivot = app.forms_prototype.pivot;
        click_topology(&mut app, &egui::Context::default(), "extrude_faces");
        assert!(app.forms_prototype.pending.is_none());
        assert!(app.status.as_ref().is_some_and(|(_, _, error)| *error));
        assert_eq!(solidcraft_engine::recovery::capture_bytes(&app.session).unwrap(), source);
        assert_eq!(app.forms_prototype.selected, selected);
        assert_eq!(app.forms_prototype.elements, elements);
        assert_eq!(app.forms_prototype.pivot, pivot);
        app.forms_prototype.elements = BTreeSet::from([Element::Edge([face[0].min(face[1]), face[0].max(face[1])])]);
        selection::sync(&mut app);
        app.forms_prototype.controls.distance = "2 mm".into();
        let selected = app.forms_prototype.selected.clone();
        let elements = app.forms_prototype.elements.clone();
        click_topology(&mut app, &egui::Context::default(), "extrude_edges");
        assert!(app.forms_prototype.pending.is_some());
        finish::flush_for_test(&mut app).unwrap();
        assert!(app.status.as_ref().is_some_and(|(_, _, error)| *error));
        assert_eq!(solidcraft_engine::recovery::capture_bytes(&app.session).unwrap(), source);
        assert_eq!(app.forms_prototype.selected, selected);
        assert_eq!(app.forms_prototype.elements, elements);
        assert_eq!(app.forms_prototype.pivot, pivot);
    }

    #[test]
    fn closure_cache_tracks_the_live_tool_candidate_and_cancel_without_changing_source_history() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","segments":[1,1,1],"levels":0})).unwrap();
        let before = app.session.forms_prototype.as_ref().unwrap().definition();
        assert_eq!(source_closed(&app), Some(false));
        assert_eq!(app.forms_prototype.controls.closure_scans.get(), 1);
        app.session.execute("form.prototype.tool.begin", &json!({})).unwrap();
        app.session.execute("form.prototype.tool.preview", &json!({"topology":{"tool":"thicken","distance":1}})).unwrap();
        assert_eq!(source_closed(&app), Some(true), "the accepted editor's open source cannot stand in for the live closed candidate");
        assert_eq!(app.forms_prototype.controls.closure_scans.get(), 2);
        app.session.execute("form.prototype.tool.cancel", &json!({})).unwrap();
        assert_eq!(source_closed(&app), Some(false));
        assert_eq!(app.forms_prototype.controls.closure_scans.get(), 3);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect()["undo"], 0);
    }
    #[test]
    fn repeated_topology_eligibility_reuses_exact_closure_and_invalidates_after_source_history_or_replacement() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"segments":[1,1,1],"levels":0})).unwrap();
        let before = app.session.forms_prototype.as_ref().unwrap().inspect();
        for _ in 0..20 {
            assert_eq!(tool_panel::topology_unavailable(&app, "thicken"), Some("forms.prototype.open_surface_required"));
            assert_eq!(tool_panel::topology_unavailable(&app, "unweld"), Some("forms.prototype.unweld_seam_required"));
        }
        assert_eq!(app.forms_prototype.controls.closure_scans.get(), 1);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), before);
        app.session.execute("form.prototype.close", &json!({})).unwrap();
        app.session.execute("form.prototype.create", &json!({"primitive":"face","segments":[1,1,1],"levels":0})).unwrap();
        assert!(tool_panel::topology_unavailable(&app, "thicken").is_none());
        assert_eq!(app.forms_prototype.controls.closure_scans.get(), 2, "a replacement Form can have the same public revision");
        let point = app.session.forms_prototype.as_ref().unwrap().cage.points[0] + Vec3::Z;
        app.session.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[point.x,point.y,point.z]]})).unwrap();
        assert!(tool_panel::topology_unavailable(&app, "thicken").is_none());
        assert_eq!(app.forms_prototype.controls.closure_scans.get(), 3);
        app.session.execute("form.prototype.undo", &json!({})).unwrap();
        assert!(tool_panel::topology_unavailable(&app, "thicken").is_none());
        assert_eq!(app.forms_prototype.controls.closure_scans.get(), 4);
        app.session.execute("form.prototype.close", &json!({})).unwrap();
        app.session.execute("form.prototype.create", &json!({"segments":[1,1,1],"levels":0})).unwrap();
        assert_eq!(tool_panel::topology_unavailable(&app, "thicken"), Some("forms.prototype.open_surface_required"));
        assert_eq!(app.forms_prototype.controls.closure_scans.get(), 5);
    }
    #[test]
    fn topology_entry_reasons_follow_the_selection_without_blocking_whole_form_tools() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        assert_eq!(tool_panel::topology_unavailable(&app, "thicken"), Some("forms.prototype.active_form_required"));
        app.session.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[4,6,1],"segments":[1,1,1],"levels":1})).unwrap();
        let before = app.session.forms_prototype.as_ref().unwrap().definition();
        for tool in ["extrude_faces", "delete_faces", "subdivide", "erase_and_fill"] {
            assert_eq!(tool_panel::topology_unavailable(&app, tool), Some("forms.prototype.select_faces_for_tool"), "{tool}");
        }
        assert_eq!(tool_panel::topology_unavailable(&app, "extrude_edges"), Some("forms.prototype.select_boundary_edges_for_tool"));
        for tool in ["thicken", "fill_hole", "match", "bridge"] {
            assert!(tool_panel::topology_unavailable(&app, tool).is_none(), "{tool} can choose its inputs after entry");
        }
        app.forms_prototype.elements.insert(Element::Face(0));
        selection::sync(&mut app);
        for tool in ["extrude_faces", "delete_faces", "subdivide", "erase_and_fill"] {
            assert!(tool_panel::topology_unavailable(&app, tool).is_none(), "{tool}");
        }
        let face = &before.cage.faces[0];
        app.forms_prototype.elements.clear();
        app.forms_prototype.elements.insert(Element::Edge([face[0].min(face[1]), face[0].max(face[1])]));
        selection::sync(&mut app);
        assert!(tool_panel::topology_unavailable(&app, "extrude_edges").is_none());
        assert_eq!(tool_panel::topology_unavailable(&app, "extrude_faces"), Some("forms.prototype.select_faces_for_tool"));
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect()["undo"], 0);
        assert!(app.session.doc.features.is_empty());
    }
    #[test]
    fn plane_alias_and_programmatic_parameters_keep_the_primitive_and_numeric_controls_in_sync() {
        for kind in ["plane", "face", "tube"] {
            let mut app = SolveApp::new(Default::default(), Default::default());
            app.run("form.prototype.create", json!({"primitive":kind,"dimensions":[8,8,6],"segments":[4,2,1],"radii":[4,2],"levels":1})).unwrap();
            assert_eq!(app.forms_prototype.primitive, if kind == "tube" { 7 } else { 5 }, "{kind} must select its own displayed primitive");
            let before = app.session.forms_prototype.as_ref().unwrap().inspect();
            let original = (
                app.forms_prototype.controls.dimensions.clone(),
                app.forms_prototype.controls.segments.clone(),
                app.forms_prototype.controls.radii.clone(),
            );
            app.run("form.prototype.parameters", json!({"dimensions":[12,10,9],"segments":[6,3,1],"radii":[6,3]})).unwrap();
            let form = app.session.forms_prototype.as_ref().unwrap();
            let updated = form.inspect();
            assert_eq!(form.parameters.kind, kind, "display synchronization must not rewrite the authored alias");
            let expected = (
                form.parameters.dimensions.map(|v| format!("{v:.4}")),
                form.parameters.segments.map(|v| v.to_string()),
                form.parameters.radii.map(|v| v.to_string()),
            );
            assert_ne!(expected, original);
            assert_eq!(
                (
                    app.forms_prototype.controls.dimensions.clone(),
                    app.forms_prototype.controls.segments.clone(),
                    app.forms_prototype.controls.radii.clone()
                ),
                expected
            );
            for (command, source, fields) in [("form.prototype.undo", &before, &original), ("form.prototype.redo", &updated, &expected)] {
                app.run(command, json!({})).unwrap();
                // History counters change; the authored primitive and its parameters must return exactly.
                let form = app.session.forms_prototype.as_ref().unwrap();
                assert_eq!(form.inspect()["parameters"], source["parameters"]);
                assert_eq!(form.parameters.kind, kind);
                assert_eq!(app.forms_prototype.primitive, if kind == "tube" { 7 } else { 5 });
                assert_eq!(
                    (&app.forms_prototype.controls.dimensions, &app.forms_prototype.controls.segments, &app.forms_prototype.controls.radii),
                    (&fields.0, &fields.1, &fields.2)
                );
            }
        }
    }
}

#[cfg(test)]
mod soft_transform_tests {
    use super::*;

    fn plane() -> SolveApp {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[20,20,1],"segments":[4,4,1],"levels":0})).unwrap();
        let form = app.session.forms_prototype.as_ref().unwrap();
        let seed = form.surface.vertices.iter().enumerate().min_by(|(_, a), (_, b)| a.len2().total_cmp(&b.len2())).unwrap().0;
        app.forms_prototype.selected.insert(seed);
        app.forms_prototype.elements.insert(Element::Vertex(seed));
        app.forms_prototype.soft.enabled = true;
        app.forms_prototype.soft.radius = "9 mm".into();
        app.forms_prototype.controls.translation = ["0".into(), "0".into(), "2 mm".into()];
        app
    }

    #[test]
    fn numeric_soft_translation_keeps_logical_selection_and_one_undo_checkpoint() {
        let mut app = plane();
        app.session.execute("form.prototype.refine", &json!({"faces":[0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15],"counts":[2,2]})).unwrap();
        let form = app.session.forms_prototype.as_ref().unwrap();
        let before = form.definition();
        let undo = form.inspect()["undo"].as_u64().unwrap();
        let selected = app.forms_prototype.selected.clone();
        let elements = app.forms_prototype.elements.clone();
        let targets = soft_selection::targets(&app, false).unwrap();
        assert!(targets.weights.iter().any(|&w| w > 0.0 && w < 1.0));
        assert!(targets.weights.contains(&1.0));
        transform(&mut app);
        let form = app.session.forms_prototype.as_ref().unwrap();
        for ((&id, &p), &weight) in targets.indices.iter().zip(&targets.points).zip(&targets.weights) {
            assert!(form.surface.vertices[id].dist(p + Vec3::Z * (2.0 * weight)) < 1e-6);
        }
        assert!(form.hierarchy.is_some());
        assert_eq!(form.hierarchy.as_ref().unwrap().leaves, before.hierarchy.as_ref().unwrap().leaves);
        assert_eq!(form.inspect()["undo"].as_u64(), Some(undo + 1));
        assert_eq!(app.forms_prototype.selected, selected);
        assert_eq!(app.forms_prototype.elements, elements);
        let after = form.definition();
        assert_ne!(before, after);
        assert!(app.session.doc.features.is_empty());
        app.session.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
        app.session.execute("form.prototype.redo", &json!({})).unwrap();
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), after);
    }

    #[test]
    fn invalid_soft_numeric_edit_is_atomic_and_disabled_soft_preserves_sharp_editing() {
        let mut app = plane();
        app.forms_prototype.soft.radius = "-1 mm".into();
        let before = app.session.forms_prototype.as_ref().unwrap().definition();
        let undo = app.session.forms_prototype.as_ref().unwrap().inspect()["undo"].clone();
        transform(&mut app);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect()["undo"], undo);
        app.forms_prototype.soft.enabled = false;
        let seed = *app.forms_prototype.selected.first().unwrap();
        let p = app.session.forms_prototype.as_ref().unwrap().surface.vertices[seed];
        transform(&mut app);
        assert!(app.session.forms_prototype.as_ref().unwrap().surface.vertices[seed].dist(p + Vec3::Z * 2.0) < 1e-6);
    }

    #[test]
    fn numeric_rotation_scale_translation_share_the_authored_pivot_and_falloff() {
        for soft in [true, false] {
            let mut app = plane();
            app.forms_prototype.soft.enabled = soft;
            if !soft {
                let n = app.session.forms_prototype.as_ref().unwrap().cage.points.len();
                app.forms_prototype.selected = (0..n).collect();
                app.forms_prototype.elements = (0..n).map(Element::Vertex).collect();
            }
            let center = Vec3::new(0.3, -0.7, 1.0);
            app.forms_prototype.pivot = Some(center);
            app.forms_prototype.orientation = 0;
            app.forms_prototype.controls.scale = ["2".into(), "0.5".into(), "1.5".into()];
            app.forms_prototype.controls.rotation = ["0".into(), "0".into(), "90".into()];
            app.forms_prototype.controls.translation = ["1 mm".into(), "2 mm".into(), "3 mm".into()];
            let targets = soft_selection::targets(&app, false).unwrap();
            assert_eq!(targets.control, !soft);
            let before = app.session.forms_prototype.as_ref().unwrap().definition();
            let expected: Vec<_> = targets
                .points
                .iter()
                .zip(&targets.weights)
                .map(|(&p, &w)| {
                    let d = p - center;
                    let (a, b, z) = (d.x * (1.0 + w), d.y * (1.0 - 0.5 * w), d.z * (1.0 + 0.5 * w));
                    let (sin, cos) = (std::f64::consts::FRAC_PI_2 * w).sin_cos();
                    center + Vec3::new(a * cos - b * sin, a * sin + b * cos, z) + Vec3::new(1.0, 2.0, 3.0) * w
                })
                .collect();
            transform(&mut app);
            let form = app.session.forms_prototype.as_ref().unwrap();
            let positions = if targets.control { form.control_positions() } else { form.surface.vertices.clone() };
            for (&id, expected) in targets.indices.iter().zip(expected) {
                assert!(positions[id].dist(expected) < 1e-6, "soft={soft}, logical vertex {id}");
            }
            assert_eq!(form.inspect()["undo"], 1);
            assert_eq!(app.forms_prototype.pivot, Some(center));
            app.session.execute("form.prototype.undo", &json!({})).unwrap();
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
        }
    }
}
