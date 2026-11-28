//! Dense ports of the common feature forms. Commands and parameter construction stay in dialogs.
use super::*;
use crate::dialogs_surface::continuity_row_explained;
use crate::ui_kit;
use crate::ui_kit::{choice_row, expr_row, expr_row_positive, hint_row, hint_row_positive};
use solidcraft_engine::kernel::Continuity;

/// The command whose icon and label head a dialog of `kind`, when it has one.
pub(super) fn command(kind: &Kind) -> Option<&'static str> {
    match kind {
        Kind::Material { .. } => Some("material.assign"),
        Kind::Appearance(_) => Some("appearance.assign"),
        Kind::Motion(k) => Some(match k {
            crate::dialogs_motion::Mo::ContactSet { .. } => "contact.create",
            crate::dialogs_motion::Mo::MotionStudy { .. } => "motion.study",
            crate::dialogs_motion::Mo::Explode { .. } => "explode.create",
            crate::dialogs_motion::Mo::Configurations { .. } => "config.table",
        }),
        Kind::Part(_) => Some("parts.insert"),
        Kind::PartSize(_) => Some("parts.resize"),
        Kind::SaveMesh(k) => Some(crate::dialogs_save_mesh::command_id(k)),
        Kind::UrdfExport(_) => Some("file.export"),
        Kind::ViewDxf { .. } => Some("file.export_view_dxf"),
        Kind::ViewNavigator(_) => Some("view.navigator"),
        Kind::InsertDrawing(p) => Some(if p.command.ends_with("dxf") { "sketch.insert_dxf" } else { "sketch.insert_svg" }),
        Kind::InsertAirfoil(_) => Some("sketch.insert_airfoil"),
        Kind::ShaftSpline(_) => Some(crate::dialogs_shaft_spline::COMMAND),
        Kind::Sketch => Some("sketch.create"),
        Kind::SketchIntersect { .. } => Some("sketch.intersect"),
        Kind::SketchProject { .. } => Some("sketch.project"),
        Kind::SketchModify(k) => Some(crate::sketch_modify_ui::command(k)),
        Kind::Text(_) => Some("sketch.edit_text"),
        Kind::TextMove(_) => Some("sketch.move"),
        Kind::EditParam { .. } => Some("parameters.change"),
        Kind::Measure { .. } => Some("inspect.measure"),
        Kind::Section { .. } => Some("inspect.section"),
        Kind::Rename { .. } => Some("timeline.rename"),
        Kind::RenameGroup { .. } => Some("timeline.rename_group"),
        Kind::ConfirmDelete { .. } => Some("timeline.delete"),
        Kind::Extrude { .. } => Some("solid.extrude"),
        Kind::Fillet { chamfer, .. } => Some(if *chamfer { "solid.chamfer" } else { "solid.fillet" }),
        Kind::Hole { .. } => Some("solid.hole"),
        Kind::PatternRect { .. } => Some("solid.pattern.rectangular"),
        Kind::PatternCirc { .. } => Some("solid.pattern.circular"),
        Kind::Revolve { .. } => Some("solid.revolve"),
        Kind::Mirror => Some("solid.mirror"),
        Kind::Loft { .. } => Some("solid.loft"),
        Kind::Sweep { .. } => Some("solid.sweep"),
        Kind::Midplane => Some("construct.plane.midplane"),
        Kind::MeshConvert { .. } => Some("mesh.convert"),
        Kind::MeshReduce { .. } => Some("mesh.reduce"),
        Kind::MeshSectionStack { .. } => Some("mesh.section_stack"),
        Kind::Construct { command, .. } => Some(command),
        Kind::ConstructAuto { command, .. } => Some(command),
        Kind::PathPlane { .. } => Some("construct.plane.along_path"),
        Kind::Split { .. } => Some("solid.split_body"),
        Kind::DeleteFaces { .. } => Some("face.delete"),
        Kind::Move { .. } => Some("solid.move"),
        Kind::TransformFace { .. } => Some("solid.transform_face"),
        Kind::Thread { .. } => Some("solid.thread"),
        Kind::ReplaceFace => Some("solid.replace_face"),
        Kind::BoundaryFill { .. } => Some("solid.boundary_fill"),
        Kind::Align { .. } => Some("solid.align"),
        Kind::ScanAlign(_) => Some("scan.align"),
        Kind::Remove => Some("solid.remove"),
        Kind::PathPattern { .. } => Some("solid.pattern.path"),
        Kind::Combine { .. } => Some("solid.combine"),
        Kind::FingerJoint { .. } => Some("solid.finger_joint"),
        Kind::Rib { web, .. } => Some(if *web { "solid.web" } else { "solid.rib" }),
        Kind::Primitive { cmd, .. } => Some(cmd),
        Kind::Surface(k) => Some(match k {
            crate::dialogs_surface::Surf::Extrude { .. } => "surface.extrude",
            crate::dialogs_surface::Surf::Revolve { .. } => "surface.revolve",
            crate::dialogs_surface::Surf::Sweep(_) => "surface.sweep",
            crate::dialogs_surface::Surf::Loft { .. } => "surface.loft",
            crate::dialogs_surface::Surf::Blend { .. } => "surface.blend",
            crate::dialogs_surface::Surf::Boundary { .. } => "surface.boundary",
            crate::dialogs_surface::Surf::Patch { .. } => "surface.patch",
            crate::dialogs_surface::Surf::Unstitch => "surface.unstitch",
            crate::dialogs_surface::Surf::ReverseNormal => "surface.reverse_normal",
            crate::dialogs_surface::Surf::Ruled { .. } => "surface.ruled",
            crate::dialogs_surface::Surf::Trim { .. } => "surface.trim",
            crate::dialogs_surface::Surf::Untrim { .. } => "surface.untrim",
            crate::dialogs_surface::Surf::SplitFace => "solid.split_face",
            _ => k.simple_metadata().map_or("", |(command, _)| command),
        }),
        Kind::Sheet(k) => match k {
            crate::dialogs_sheet::Sm::Flange { .. } => Some("sheet.flange"),
            crate::dialogs_sheet::Sm::CornerJoin { .. } => Some("sheet.corner_join"),
            crate::dialogs_sheet::Sm::Unfold { refold } => Some(if *refold { "sheet.refold" } else { "sheet.unfold" }),
            crate::dialogs_sheet::Sm::Convert { .. } => Some("sheet.convert"),
            crate::dialogs_sheet::Sm::Recognize { .. } => Some("sheet.recognize"),
            crate::dialogs_sheet::Sm::Rules { .. } => Some("sheet.manage_rules"),
            crate::dialogs_sheet::Sm::FlatPattern { .. } => Some("sheet.flat_pattern"),
            crate::dialogs_sheet::Sm::ExportDxf { .. } => Some("sheet.export_dxf"),
            crate::dialogs_sheet::Sm::Hardware { .. } => Some("sheet.hardware"),
            _ => k.simple_metadata().map(|(command, _)| command),
        },
        Kind::Frame(_) => Some("frame.member"),
        Kind::EndCap(_) => Some("frame.end_cap"),
        Kind::Dogbone(_) => Some("solid.dogbone"),
        Kind::CellGrid(_) => Some("solid.cell_grid"),
        Kind::Weld(_) => Some("weld.create"),
        Kind::RotateMember(_) => Some("frame.rotate_member"),
        Kind::FrameTrim(_) => Some("frame.trim"),
        Kind::Gusset(_) => Some("frame.gusset"),
        Kind::FramePlate(_) => Some("frame.plate"),
        Kind::CapturePosition(_) => Some("component.capture_position"),
        Kind::Pulley(_) => Some("pulley.insert"),
        Kind::Plastic(k) => match k {
            crate::dialogs_plastic::Pl::SnapFit { .. } => Some("plastic.snap_fit"),
            crate::dialogs_plastic::Pl::Assign { .. } => Some("plastic.assign_rule"),
            crate::dialogs_plastic::Pl::Rules { .. } => Some("plastic.manage_rules"),
            _ => k.simple_metadata().map(|(command, _)| command),
        },
        Kind::Assembly(k) => match k {
            crate::dialogs_assembly::Asm::FromBodies => Some("component.from_bodies"),
            crate::dialogs_assembly::Asm::NewComponent { .. } => Some("component.create"),
            crate::dialogs_assembly::Asm::Joint(f) => Some(if f.as_built { "joint.as_built" } else { "joint.create" }),
            crate::dialogs_assembly::Asm::JointOrigin { .. } => Some("joint.origin"),
            crate::dialogs_assembly::Asm::RigidGroup { .. } => Some("joint.rigid_group"),
            crate::dialogs_assembly::Asm::MotionLink { .. } => Some("joint.motion_link"),
            crate::dialogs_assembly::Asm::Coupling(f) => Some(f.command),
            crate::dialogs_assembly::Asm::Cam(_) => Some("joint.cam"),
            crate::dialogs_assembly::Asm::Constrain(_) => Some("assembly.constraint.create"),
            crate::dialogs_assembly::Asm::Trace(_) => Some("motion.trace"),
            crate::dialogs_assembly::Asm::Drive { .. } => Some("joint.drive"),
            crate::dialogs_assembly::Asm::Interference { .. } => Some("inspect.interference"),
        },
        _ => kind.simple_metadata().map(|(command, _)| command),
    }
}

const FILLET_TYPES: [(&str, &str); 5] = [
    ("Constant radius", "constant"),
    ("Chord length", "chord"),
    ("Variable radius", "variable"),
    ("ui.fillet_type_curvature", "curvature"),
    ("Spline", "spline"),
];

/// How a fillet's corners where three or more rounded edges meet continue the rounds, with the
/// hover line that says what each builds: tangent (a rolling ball, the `corner` default) or
/// curvature continuous (`corner: g2`, a smooth patch set back along each edge).
const FILLET_CORNERS: [(Continuity, &str); 2] =
    [(Continuity::G1, "ui.fillet_corner_tangent_tip"), (Continuity::G2, "ui.fillet_corner_curvature_tip")];

/// The default setback of a smooth-patch corner, as a multiple of the radius (the kernel's), as
/// the messages show it.
fn setback_factor() -> String {
    solidcraft_engine::doc::kernel::DEFAULT_SETBACK_FACTOR.to_string()
}

/// The Corners row's hover text: a line for each choice.
fn fillet_corner_tip() -> String {
    let factor = setback_factor();
    FILLET_CORNERS.map(|(_, tip)| solidcraft_ui_text::format(tip, &[("factor", &factor)])).join("\n")
}

/// What `solid.fillet` does with a radius too large for the faces beside an edge (`fit`): stop
/// and say so (the default), round every edge with the largest radius that fits, or keep the
/// radius and narrow it only where the faces run out.
const FILLET_FITS: [(&str, &str); 4] = [
    ("ui.fillet_fit_exact", "exact"),
    ("ui.fillet_fit_reduce", "reduce"),
    ("ui.fillet_fit_taper", "taper"),
    ("ui.fillet_fit_keep_edge", "keep_edge"),
];

/// How two rounds turn where they meet at a sharp corner edge (`turn`): swept round the edge
/// where the rolling ball can pass it and mitred elsewhere (the default, `auto`, not sent), in a
/// mitre, or swept round the edge.
const FILLET_TURNS: [(&str, &str); 3] = [("ui.fillet_turn_auto", "auto"), ("ui.fillet_turn_mitre", "mitre"), ("ui.fillet_turn_swept", "swept")];

/// Which fillet options apply together, exactly as `solid.fillet` takes them (`fillet` in
/// crates/engine/src/cmd/features.rs):
///
/// | Type          | Face limits (`fit`)     | Corners (`corner`)                  | Setback (`setbacks`) | Turns (`turn`)                                  |
/// |---------------|-------------------------|-------------------------------------|----------------------|-------------------------------------------------|
/// | Constant      | exact, reduce or taper  | tangent; curvature with exact only  | with curvature only  | auto (not sent), mitre; swept without taper, g2 |
/// | any other     | exact (not sent)        | tangent (not sent)                  | none                 | auto (not sent)                                 |
///
/// The dialog shows an option's row only where it applies and sends the option only then, and a
/// choice that rules another out puts that one back to its default ([`settle_fillet_options`]).
/// Where the Turns row is hidden, the default `auto` mitres every turn: a turn is swept only
/// between constant equal circular rounds, which a tapered radius, a curvature corner and the
/// other types are not.
pub(super) fn fillet_option_applies(extra: &serde_json::Map<String, Value>, key: &str) -> bool {
    let constant = extra.get("type").and_then(Value::as_str).is_none_or(|t| t == "constant");
    let exact = extra.get("fit").and_then(Value::as_str).is_none_or(|fit| fit == "exact");
    match key {
        "fit" => constant,
        "corner" | "setbacks" => constant && exact,
        // A swept turn takes an exact or reduced radius and rolling-ball corners.
        "turn" => constant && extra.get("fit").and_then(Value::as_str) != Some("taper") && extra.get("corner").and_then(Value::as_str) != Some("g2"),
        _ => true,
    }
}

/// After `changed` (`fit` or `corner`) took a new value, the options it rules out back to their
/// defaults: a fitted radius (`fit` reduce or taper) takes tangent corners, a curvature corner
/// takes the exact radius. A type other than Constant clears both ([`choose_fillet_type`]).
fn settle_fillet_options(extra: &mut serde_json::Map<String, Value>, changed: &str, setback: &mut Option<String>) {
    let fitted = extra.get("fit").and_then(Value::as_str).is_some_and(|fit| fit != "exact");
    let tapered = extra.get("fit").and_then(Value::as_str) == Some("taper");
    let patch = extra.get("corner").and_then(Value::as_str) == Some("g2");
    let swept = extra.get("turn").and_then(Value::as_str) == Some("swept");
    match changed {
        "fit" if fitted => {
            extra.remove("corner");
            extra.remove("setbacks");
            *setback = None;
            if tapered {
                extra.remove("turn");
            }
        }
        "corner" if patch => {
            extra.remove("fit");
            extra.remove("turn");
        }
        "turn" if swept => {
            extra.remove("corner");
            extra.remove("setbacks");
            *setback = None;
            if tapered {
                extra.remove("fit");
            }
        }
        _ => {}
    }
}

/// A fillet's Corners row (`corner`) and, with a smooth patch, its Setback (`setbacks`, one
/// distance for every corner), for a Constant radius fillet. Shown only while the edges have a
/// corner where three or more rounded edges meet, or the fillet already has a smooth patch.
fn fillet_corner_rows(
    ui: &mut egui::Ui,
    s: &solidcraft_engine::Session,
    extra: &mut serde_json::Map<String, Value>,
    inputs: &[selection::SelInput],
    radius: &str,
    setback: &mut Option<String>,
) -> bool {
    let items = selection::items(inputs, 0);
    let corners = fillet_corners(s, items);
    let patch = extra.get("corner").and_then(Value::as_str) == Some("g2");
    if !patch && corners.is_empty() {
        return false;
    }
    let mut enter = false;
    let mut corner = if patch { Continuity::G2 } else { Continuity::G1 };
    continuity_row_explained(ui, "ui.corners", "fi_corner", &mut corner, &FILLET_CORNERS.map(|(g, _)| g), &fillet_corner_tip());
    if (corner == Continuity::G2) != patch {
        if corner == Continuity::G2 {
            extra.insert("corner".into(), json!(Continuity::G2.as_str()));
        } else {
            extra.remove("corner");
        }
        settle_fillet_options(extra, "corner", setback);
    }
    if extra.get("corner").and_then(Value::as_str) == Some("g2") {
        let factor = setback_factor();
        let length = |text: &str| crate::length_input::eval(&s.doc, text, solidcraft_engine::doc::expr::Kind::Length).ok().filter(|v| v.is_finite());
        let r = length(radius).filter(|r| *r > 0.0);
        // Not typed, the field shows the command's default as a length, following the radius.
        let default = r.map(|r| crate::length_input::shown(&s.doc, solidcraft_engine::doc::kernel::DEFAULT_SETBACK_FACTOR * r));
        let shown = setback.clone().or(default).unwrap_or_default();
        // A setback at or below the least the kernel takes there is flagged before OK.
        let least = r.and_then(|r| crate::dialogs::fillet_least_setback(ui.ctx(), s, items, &corners, r));
        let warning = least
            .filter(|least| length(&shown).is_some_and(|v| v <= least * (1.0 + 1e-9)))
            .map(|least| solidcraft_ui_text::format("ui.fillet_setback_least", &[("least", &crate::length_input::shown(&s.doc, least))]));
        let mut text = shown.clone();
        let row = ui.scope(|ui| {
            let hint = solidcraft_ui_text::format("ui.fillet_setback_hint", &[("factor", &factor)]);
            ui_kit::checked_row(ui, "ui.fillet_setback", &mut text, &hint, warning.as_deref())
        });
        row.response.backing_on_hover_text(solidcraft_ui_text::format("ui.fillet_setback_tip", &[("factor", &factor)]));
        let (accepted, field) = row.inner;
        enter |= accepted;
        if text != shown {
            *setback = Some(text);
        }
        // Emptied and left, the field goes back to the default.
        if setback.as_deref().is_some_and(|t| t.trim().is_empty()) && !field.has_focus() {
            *setback = None;
        }
    }
    enter
}

/// A Constant radius fillet's "Face limits" row (`fit`), with the radius a reduced fillet used.
fn fillet_fit_rows(app: &SolveApp, ui: &mut egui::Ui, extra: &mut serde_json::Map<String, Value>, setback: &mut Option<String>) {
    let t = Tokens::get();
    let before = extra.get("fit").cloned();
    let row = ui.scope(|ui| form_row(ui, "ui.fillet_fit", |ui| extra_choice(ui, extra, "fi_fit", "fit", &FILLET_FITS)));
    row.response.backing_on_hover_text(format!("{}\n{}", tr("ui.fillet_fit_tip"), tr("ui.fillet_fit_keep_edge_tip")));
    let fit = row.inner;
    if extra.get("fit") != before.as_ref() {
        settle_fillet_options(extra, "fit", setback);
    }
    if fit == 1
        && let Some((lo, hi)) = app.preview.applied
    {
        form_row(ui, "Applied Radius", |ui| {
            let shown = if hi > lo {
                format!("{} – {}", crate::prefs::show_mm_bare(app, lo), crate::prefs::show_mm(app, hi, 1))
            } else {
                crate::prefs::show_mm(app, lo, 1)
            };
            ui.label(RichText::new(shown).color(t.warning));
        });
    }
}

/// A Constant radius fillet's Turns row (`turn`): how two rounds turn where they meet at a sharp
/// corner edge.
fn fillet_turn_row(ui: &mut egui::Ui, extra: &mut serde_json::Map<String, Value>, setback: &mut Option<String>) {
    let before = extra.get("turn").cloned();
    let row = ui.scope(|ui| form_row(ui, "ui.fillet_turn", |ui| extra_choice(ui, extra, "fi_turn", "turn", &FILLET_TURNS)));
    row.response.backing_on_hover_text(tr("ui.fillet_turn_tip"));
    if extra.get("turn") != before.as_ref() {
        settle_fillet_options(extra, "turn", setback);
    }
}

/// The most instances a solid pattern makes in one direction (the document's own limit).
const MAX_PATTERN_INSTANCES: i64 = 1000;

/// A length row that refuses zero and negative values when `positive` (a spacing that sets the
/// count with an extent), else any length (a spacing may run backwards).
fn expr_row_positive_if(ui: &mut egui::Ui, label: &str, value: &mut String, positive: bool) -> bool {
    if positive { expr_row_positive(ui, label, value) } else { expr_row(ui, label, value) }
}

/// A solid pattern's quantity row, with step arrows from 1 to `MAX_PATTERN_INSTANCES`. The
/// count goes to the command as typed, so it is evaluated as the document evaluates it.
fn quantity_row(ui: &mut egui::Ui, doc: &solidcraft_engine::doc::Document, label: &str, value: &mut String) -> bool {
    count_row_to(ui, doc, label, value, MAX_PATTERN_INSTANCES)
}

/// The most revolutions a coil makes (the helix's own limit, a whole number of turns).
const MAX_COIL_REVOLUTIONS: i64 = solidcraft_engine::kernel::MAX_HELIX_TURNS as i64;

/// A count row with step arrows by whole numbers from 1 to `max`, evaluated as the document
/// evaluates it. A fraction still types in (a coil's revolutions); an arrow steps it to the next
/// whole number.
fn count_row_to(ui: &mut egui::Ui, doc: &solidcraft_engine::doc::Document, label: &str, value: &mut String, max: i64) -> bool {
    let eval = |text: &str| doc.eval(text.trim(), solidcraft_engine::doc::expr::Kind::Unitless).ok();
    ui_kit::count_row(ui, label, value, &ui_kit::Count { min: 1, max, eval: &eval })
}

/// A finger count is stored as an integer on the feature, so this row follows the literal-integer
/// rule used when the dialog submits it instead of accepting expressions the command cannot keep.
fn finger_joint_count_row(ui: &mut egui::Ui, app: &SolveApp, value: &mut String) -> bool {
    let literal = |text: &str| text.trim().parse::<usize>().ok();
    let eval = |text: &str| literal(text).map(|count| count as f64);
    let enter = ui_kit::count_row(ui, "ui.finger_joint_count", value, &ui_kit::Count { min: 1, max: 64, eval: &eval });
    if !literal(value).is_some_and(|count| (1..=64).contains(&count)) {
        ui_kit::status_line(ui, app.tr("ui.finger_joint_invalid_count"), ui_kit::StatusTone::Error);
    }
    enter
}

/// The distance types a rectangular pattern direction offers, in the order of
/// [`PatternDistance`]'s words.
const PATTERN_DISTANCES: [(PatternDistance, &str); 4] = [
    (PatternDistance::CountSpacing, "ui.pattern_count_spacing"),
    (PatternDistance::CountExtent, "ui.pattern_count_extent"),
    (PatternDistance::SpacingExtent, "ui.pattern_spacing_extent"),
    (PatternDistance::CountList, "ui.pattern_count_list"),
];

/// One rectangular pattern direction (`index` 0 or 1) as its own titled group: the direction's
/// pick, its Distance Type, the values that type takes (Quantity, Spacing, Extent, Spacing List,
/// Copy at the end) and (for features) Alignment, in the same order for both directions, so a direction's
/// settings sit together (#9634, #8523).
fn pattern_direction_group(app: &SolveApp, d: &mut Dialog, ui: &mut egui::Ui, index: usize) -> bool {
    let (key, title, quantity_label, spacing_label) = if index == 0 {
        ("pattern_direction1", "Direction 1", "Quantity", "Spacing")
    } else {
        ("pattern_direction2", "Direction 2", "Quantity 2", "Spacing 2")
    };
    let (type_label, extent_label, end_label, type_key, end_key) = if index == 0 {
        ("ui.pattern_distance_type", "ui.pattern_extent", "ui.pattern_copy_at_end", "distance1", "copy_at_end1")
    } else {
        ("ui.pattern_distance_type_2", "ui.pattern_extent_2", "ui.pattern_copy_at_end_2", "distance2", "copy_at_end2")
    };
    let list_label = if index == 0 { "ui.pattern_spacing_list" } else { "ui.pattern_spacing_list_2" };
    let features = d.inputs.first().is_some_and(|i| i.accept == selection::FEATURES);
    ui_kit::section(ui, key, title, true, |ui| {
        let mut enter = false;
        input_rows_range(d, ui, &app.session, app.preferences.origin_plane_names, index + 1..index + 2);
        let distance = pattern_distance(&d.extra, index);
        let mut chosen = PATTERN_DISTANCES.iter().position(|(kind, _)| *kind == distance).unwrap_or(0);
        let labels = PATTERN_DISTANCES.map(|(_, label)| label);
        choice_row(ui, type_label, type_key, &mut chosen, &labels);
        let distance = PATTERN_DISTANCES.get(chosen).map(|(kind, _)| *kind).unwrap_or_default();
        if distance.is_default() {
            d.extra.remove(type_key);
        } else {
            d.extra.insert(type_key.into(), json!(distance.as_str()));
        }
        if let Kind::PatternRect { count, spacing, count2, spacing2, extent, extent2, list, list2 } = &mut d.kind {
            let (count, spacing, extent, list) = if index == 0 { (count, spacing, extent, list) } else { (count2, spacing2, extent2, list2) };
            if distance.uses_count() {
                enter |= quantity_row(ui, &app.session.doc, quantity_label, count);
            }
            if distance.uses_spacing() {
                enter |= expr_row_positive_if(ui, spacing_label, spacing, distance.uses_extent());
            }
            if distance.uses_extent() {
                enter |= expr_row_positive(ui, extent_label, extent);
            }
            if distance.uses_list() {
                let row = ui.scope(|ui| ui_kit::row(ui, list_label, |ui| ui_kit::hint_field(ui, list, "10 mm; 20 mm; 30 mm")));
                row.response.backing_on_hover_text(tr("ui.pattern_spacing_list_tip"));
                enter |= row.inner;
            }
        }
        if distance == PatternDistance::SpacingExtent {
            form_row(ui, end_label, |ui| {
                extra_check(ui, &mut d.extra, end_key);
            });
        } else {
            d.extra.remove(end_key);
        }
        if features {
            solid_pattern_alignment_row(ui, &mut d.extra, index);
        }
        enter
    })
    .unwrap_or(false)
}

fn fillet_type(extra: &serde_json::Map<String, Value>) -> usize {
    let value = extra.get("type").and_then(Value::as_str).unwrap_or("constant");
    if value == "g2" { 3 } else { FILLET_TYPES.iter().position(|(_, key)| *key == value).unwrap_or(0) }
}

/// Defaults are introduced only for an explicit new type choice, never for a no-op edit.
fn choose_fillet_type(extra: &mut serde_json::Map<String, Value>, index: usize, radius: &str) {
    let Some((_, key)) = FILLET_TYPES.get(index) else { return };
    if index == 0 {
        extra.remove("type");
    } else {
        extra.insert("type".into(), json!(key));
    }
    if index != 0 {
        extra.remove("fit");
        extra.remove("corner");
        extra.remove("setbacks");
        extra.remove("turn");
    }
    if index == 2 {
        extra.entry("radius2").or_insert_with(|| json!("1 mm"));
        extra.entry("mid").or_insert_with(|| json!([]));
    }
    if index == 4 {
        let radii = spline_radii(extra, radius);
        extra.insert("radii".into(), json!(radii));
    }
}

/// The dialog's form: its hints, inputs and value rows. True when Enter was pressed in a field.
pub(super) fn rows(app: &mut SolveApp, ctx: &egui::Context, d: &mut Dialog, ui: &mut egui::Ui) -> bool {
    let t = Tokens::get();
    let mut enter = false;
    if matches!(d.kind, Kind::Appearance(_)) {
        ui.spacing_mut().item_spacing.y = t.space_10;
    }
    if matches!(d.kind, Kind::Sketch | Kind::SketchModify(_) | Kind::Text(_) | Kind::TextMove(_) | Kind::EditParam { .. }) {
        let prompt = if let Some(input) = d.active_input() {
            tr(hint(input)).to_owned()
        } else {
            let key = match d.kind {
                Kind::Text(_) => "sketch_panel.text_hint",
                Kind::TextMove(_) => "sketch_panel.move_hint",
                Kind::EditParam { .. } => "sketch_panel.dimension_hint",
                _ => "dialog.selection_guidance",
            };
            tr(key).to_owned()
        };
        ui_kit::text(ui, &prompt, ui_kit::Text::Hint("tool")).backing_on_hover_text(&prompt);
        if matches!(&d.kind, Kind::SketchModify(k) if k.previews()) {
            let label = d.active_input().map(|input| tr(input.label)).unwrap_or_else(|| tr("Objects"));
            let step = solidcraft_ui_text::format(
                "dialog.selection_step",
                &[("step", &(d.active + 1).to_string()), ("total", &d.inputs.len().to_string()), ("input", label)],
            );
            ui_kit::text(ui, &step, ui_kit::Text::Hint("step")).backing_on_hover_text(tr("dialog.selection_guidance"));
        }
    }
    // Trim explains incomplete picks before OK can be pressed, including a whole cutting sheet.
    if matches!(d.kind, Kind::Surface(crate::dialogs_surface::Surf::Trim { .. }))
        && d.error.is_none()
        && app.preview.error.is_none()
        && let Err(reason) = apply_commands(app, d)
    {
        let hint = crate::ui_kit::text(ui, &reason, crate::ui_kit::Text::Hint("trim_missing_input"));
        crate::scenario::publish_rect("dialog:trim_missing_input", hint.rect);
    }
    let manage_hint = match &d.kind {
        Kind::Material { .. } => Some("manage.material_hint"),
        Kind::Appearance(_) => Some("manage.appearance_hint"),
        Kind::Motion(crate::dialogs_motion::Mo::Configurations { .. }) => Some("manage.config_hint"),
        Kind::Sheet(crate::dialogs_sheet::Sm::Rules { .. }) | Kind::Plastic(crate::dialogs_plastic::Pl::Rules { .. }) => Some("manage.rules_hint"),
        Kind::Plastic(crate::dialogs_plastic::Pl::Assign { .. }) => Some("manage.assign_hint"),
        _ => None,
    };
    if let Some(key) = manage_hint {
        ui_kit::text(ui, tr(key), ui_kit::Text::Hint("manage"));
    }
    // Plane, Axis or Point: which method its picks chose, and that more picks choose again.
    if let Some(hint) = d.auto.as_ref().map(super::auto::hint) {
        ui_kit::text(ui, &hint, ui_kit::Text::Hint("auto"));
    }
    placement_row(d, ui, app);
    crate::dialogs_sweep::grow_loft_rails(d);
    if crate::hole_refs::takes(d) {
        // Each reference edge with its distance (#37, #6380).
        enter |= crate::hole_refs::input_rows(d, ui, &app.session, app.preferences.origin_plane_names, app.preferences.length_decimals);
    } else if matches!(d.kind, Kind::PatternRect { .. }) {
        // Each direction's pick shows in its own group, with its quantity and spacing (#9634).
        input_rows_range(d, ui, &app.session, app.preferences.origin_plane_names, 0..1);
    } else if matches!(d.kind, Kind::Move { .. }) {
        // Object (above), Selection, then the Move Type and its own picks.
        input_rows_range(d, ui, &app.session, app.preferences.origin_plane_names, 0..1);
        crate::dialogs_move::type_row(app, d, ui);
        let n = d.inputs.len();
        input_rows_range(d, ui, &app.session, app.preferences.origin_plane_names, 1..n);
    } else if matches!(d.kind, Kind::TransformFace { .. }) {
        input_rows_range(d, ui, &app.session, app.preferences.origin_plane_names, 0..1);
        let mode = if transform_face::pivot_input(d).is_some() {
            2
        } else {
            usize::from(matches!(d.kind, Kind::TransformFace { explicit_pivot: true, .. }))
        };
        let mut selected = mode;
        choice_row(ui, "Pivot", "transform_face_pivot", &mut selected, &["Picked point", "Custom", transform_face::PICK_ON_FACE]);
        if selected != mode {
            transform_face::set_pivot_mode(d, selected);
            if selected == 2 {
                surrender_keyboard_focus(ui);
                app.preview.clear();
            }
        }
        if selected == 2 && !sels(d, 0).is_empty() {
            input_rows_range(d, ui, &app.session, app.preferences.origin_plane_names, 1..2);
        }
    } else if matches!(d.kind, Kind::ShaftSpline(_)) {
        // The face a part's teeth start from shows once the part's round face is picked (#17693).
        for rows in crate::dialogs_shaft_spline::shown_picks(&d.inputs) {
            input_rows_range(d, ui, &app.session, app.preferences.origin_plane_names, rows);
        }
    } else if let Kind::CellGrid(c) = &d.kind {
        // The cell's profile shows with the Sketch shape only.
        let shown = crate::dialogs_cell_grid::shown_picks(c);
        input_rows_range(d, ui, &app.session, app.preferences.origin_plane_names, shown);
    } else if matches!(d.kind, Kind::Frame(_)) {
        // Corner Overrides shows among Frame Member's own rows, under the corner settings.
        input_rows_range(d, ui, &app.session, app.preferences.origin_plane_names, 0..2);
    } else if matches!(d.kind, Kind::Loft { .. } | Kind::Surface(crate::dialogs_surface::Surf::Loft { .. })) && d.inputs.len() > 5 {
        // More than four rails: the rail rows share one list that scrolls, about four in view (#12185).
        let rail_list_height = Tokens::get().control_height * 4.5;
        input_rows_range(d, ui, &app.session, app.preferences.origin_plane_names, 0..1);
        egui::ScrollArea::vertical().id_salt("loft_rails").max_height(rail_list_height).show(ui, |ui| {
            input_rows_range(d, ui, &app.session, app.preferences.origin_plane_names, 1..d.inputs.len());
        });
    } else {
        input_rows(d, ui, &app.session, app.preferences.origin_plane_names);
        crate::command_sketch::offer_row(app, d, ui);
    }
    // When a pick field is active (no widget holds keyboard focus), Tab enters the first
    // control after the pick inputs (e.g. Direction in Extrude) instead of jumping to the app bar.
    let prior_focus = d.had_keyboard_focus || ctx.memory(|m| m.focused().is_some_and(|id| m.had_focus_last_frame(id)));
    if !prior_focus
        && !ui.is_sizing_pass()
        && app.issue_report.draft.is_none()
        && ui.memory(|m| m.allows_interaction(ui.layer_id()))
        && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab))
    {
        ctx.memory_mut(|m| {
            if let Some(id) = m.focused() {
                m.surrender_focus(id);
            }
            m.move_focus(egui::FocusDirection::Next);
        });
        ctx.request_repaint();
    }
    d.sync_material(&app.session);
    // The Hole dialog's dropdowns preview each choice on a copy of the form as it stands.
    let hole_form = matches!(d.kind, Kind::Hole { .. }).then(|| (d.kind.clone(), to_object_on(&d.inputs)));
    match &mut d.kind {
        Kind::Material { index, editor } => crate::material_editor::rows(app, ui, index, editor),
        Kind::Appearance(k) => enter |= crate::dialogs_appearance::rows(app, ui, k, &mut d.inputs),
        Kind::Motion(k) => enter |= crate::dialogs_motion::rows(app, ui, k, &mut d.inputs),
        Kind::Part(k) => enter |= crate::dialogs_parts::rows(app, ui, k, &d.inputs),
        Kind::PartSize(k) => enter |= crate::dialogs_part_size::rows(app, ui, k),
        Kind::SaveMesh(k) => enter |= crate::dialogs_save_mesh::rows(app, ui, k, &mut d.inputs),
        Kind::UrdfExport(f) => enter |= crate::dialogs_urdf::rows(ui, f),
        Kind::ViewDxf { dashed, tolerance, zero_origin } => crate::dialogs_view_dxf::rows(app, ui, dashed, tolerance, zero_origin),
        Kind::ViewNavigator(form) => crate::view_gizmo::rows(app, ui, form, &d.inputs),
        Kind::InsertDrawing(p) => crate::dialogs_insert_drawing::rows(app, ui, p),
        Kind::InsertAirfoil(a) => crate::dialogs_insert_airfoil::rows(ui, a),
        Kind::ShaftSpline(k) => {
            // A face taken by a row it does not belong in goes to its own row (a bore out of Shaft, a shaft out
            // of Bore, a start face out of a round face's row). After any change of the picks the row that
            // waits next takes the next click: the face a picked part's teeth start from, before any other
            // free row.
            if crate::dialogs_shaft_spline::sort_picks(app, k, &mut d.inputs).is_some()
                && let Some(next) = crate::dialogs_shaft_spline::next_pick(&d.inputs)
            {
                d.active = next;
            }
            enter |= crate::dialogs_shaft_spline::rows(app, ui, k, &d.inputs)
        }
        Kind::Sketch => {
            let edge_on = edge_on_origin_planes(&app.cam);
            if !edge_on.is_empty() {
                let text = solidcraft_ui_text::format("sketch_panel.edge_on_hint", &[("planes", &edge_on.join(", "))]);
                ui_kit::text(ui, &text, ui_kit::Text::Hint("edge_on"));
            }
            if d.error.is_some() && matches!(d.inputs.first().and_then(|i| i.items.first()), Some(Sel::Face { .. })) {
                ui_kit::text(
                    ui,
                    tr("This face cannot provide a durable attachment. Choose another face or explicitly use a fixed plane."),
                    ui_kit::Text::Hint("attachment"),
                );
                // What the button does, said plainly: the sketch is made on the face's plane as it is
                // now, and does not follow later edits of the face (#16523).
                ui_kit::text(ui, solidcraft_ui_text::tr("ui.sketch_use_fixed_plane_explained"), ui_kit::Text::Hint("attachment"));
                if ui_kit::button(ui, tr("Use fixed plane"), ui_kit::Button::Normal).clicked() {
                    d.extra.insert("fixed_plane".into(), json!(true));
                }
            }
        }
        Kind::SketchIntersect { bodies, projection_link, construction } => {
            let mut selected = if *bodies { 1 } else { 0 };
            let changed = ui_kit::row(ui, tr("ui.sketch_intersect_selection_filter"), |ui| {
                ui_kit::segmented(ui, &mut selected, &[tr("Geometry"), tr("Bodies")], Default::default())
            });
            if changed && let Some(input) = d.inputs.first_mut() {
                *bodies = selected == 1;
                input.accept = if *bodies { BODIES } else { super::INTERSECT_GEOMETRY };
                input.items.clear();
            }
            ui_kit::row(ui, tr("ui.sketch_intersect_projection_link"), |ui| ui_kit::checkbox(ui, "", projection_link));
            ui_kit::row(ui, tr("ui.construction"), |ui| ui_kit::checkbox(ui, "", construction));
        }
        Kind::SketchProject { bodies, projection_link, construction } => {
            let response =
                ui_kit::row(ui, solidcraft_ui_text::tr("ui.project_link_tools"), |ui| ui_kit::checkbox(ui, "", &mut app.ui.project_link_tools));
            crate::scenario::publish_handle("project:link-tools", response.rect.center());
            response.backing_on_hover_text(solidcraft_ui_text::tr("ui.project_link_tools_note"));
            let mut selected = if *bodies { 1 } else { 0 };
            let changed = ui_kit::row(ui, tr("ui.sketch_intersect_selection_filter"), |ui| {
                ui_kit::segmented(ui, &mut selected, &[tr("Geometry"), tr("Bodies")], Default::default())
            });
            if changed && let Some(input) = d.inputs.first_mut() {
                *bodies = selected == 1;
                input.accept = if *bodies { BODIES } else { super::PROJECT_GEOMETRY };
                input.items.clear();
                // Geometry offers Tangent chain for edges; bodies have no edges to chain.
                input.chain = (!*bodies).then_some(false);
                input.chain_seeds.clear();
                input.chain_written.clear();
            }
            ui_kit::row(ui, tr("ui.sketch_intersect_projection_link"), |ui| ui_kit::checkbox(ui, "", projection_link));
            ui_kit::row(ui, tr("ui.construction"), |ui| ui_kit::checkbox(ui, "", construction));
        }
        Kind::SketchModify(k) => {
            let two_parallel_lines = crate::sketch_modify_ui::picks_two_parallel_lines(app, &d.inputs);
            enter |= crate::sketch_modify_ui::rows(ui, k, &app.session.doc, two_parallel_lines);
        }
        Kind::Text(k) => enter |= crate::sketch_text::rows(ui, k, &mut d.inputs, &mut d.active),
        Kind::TextMove(k) => enter |= crate::sketch_text_move::rows(ui, k),
        Kind::EditParam { name, expr } => {
            enter |= ui_kit::row(ui, name, |ui| ui_kit::focused_field(ui, expr));
        }
        Kind::Extrude {
            distance,
            direction,
            operation,
            auto_op,
            smart,
            distance2,
            taper,
            taper_present,
            taper2,
            to_offset,
            start,
            offset_start,
            symmetric_offset,
            all,
            all2,
            to_offset2,
            thin,
            wall,
            wall_side,
            wall2,
        } => {
            // The feature's name (#18668): what it is called, or will be. Blank is the default name.
            let mut name = d.extra.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
            let before = name.clone();
            form_row(ui, "Name", |ui| {
                enter |= ui_kit::text_field(ui, &mut name, "").0;
            });
            if name != before {
                d.extra.insert("name".into(), json!(name));
            }
            form_row(ui, "Direction", |ui| {
                // One Side, Two Sides, Symmetric (a flip is a negative distance).
                let order = [0usize, TWO_SIDES, 2];
                let reversed = *direction == 1 || *direction == TWO_SIDES_REVERSED;
                let shown = match *direction {
                    1 => 0,
                    TWO_SIDES_REVERSED => TWO_SIDES,
                    d => d,
                };
                let mut pos = order.iter().position(|o| *o == shown).unwrap_or(0);
                ui_kit::dropdown_with_keyboard_close(ui, "ex_dir", &mut pos, &["One Side", "Two Sides", "Symmetric"]);
                if pos != order.iter().position(|o| *o == shown).unwrap_or(0) {
                    *direction = match (order.get(pos).copied().unwrap_or(0), reversed) {
                        (0, true) => 1,
                        (TWO_SIDES, true) => TWO_SIDES_REVERSED,
                        (d, _) => d,
                    };
                    // Selecting Symmetric applies the current total-distance semantics.
                    d.extra.insert("convention".into(), json!(solidcraft_engine::doc::ExtrudeConvention::TotalDistance));
                    // An explicit direction ends the automatic choices (#1257).
                    smart.manual_dir = true;
                    *auto_op = false;
                }
            });
            let cur = if to_object_on(&d.inputs) { 1 } else { usize::from(*all) * 2 };
            let mut ext = cur;
            form_row(ui, "Extent Type", |ui| {
                // The dropdown says when the user picked an option, the one shown included: a
                // pick of To Object takes the pick, so the next click in the view picks the face
                // to run to (#19671).
                let picked = ui_kit::dropdown_picked(ui, "ex_ext", &mut ext, &["Distance", "To Object", "All"]);
                *all = ext == 2;
                set_to_object(&mut d.inputs, &mut d.active, ext == 1);
                if picked
                    && ext == 1
                    && let Some(at) = d.inputs.iter().position(|i| i.label == TO_OBJECT)
                {
                    d.active = at;
                }
            });
            if ext == 0 {
                enter |= expr_row(ui, "Distance", distance);
            }
            if ext == 1 {
                enter |= expr_row(ui, "To Offset", to_offset);
                // A profile wider than the To face ends on the face's surface carried on past it.
                let row = ui.scope(|ui| form_row(ui, "ui.extend_to_face", |ui| extra_check(ui, &mut d.extra, "extend_to_face")));
                row.response.backing_on_hover_text(tr("ui.extrude_extend_to_face_tip"));
            } else {
                d.extra.remove("extend_to_face");
            }
            if two_sided(*direction) {
                // The second side takes the extent types of the first (#18691).
                let cur2 = if object_on(&d.inputs, TO_OBJECT2) { 1 } else { usize::from(*all2) * 2 };
                let mut ext2 = cur2;
                form_row(ui, "ui.extrude_extent_type_2", |ui| {
                    combo(ui, "ex_ext2", &["Distance", "To Object", "All"], &mut ext2);
                    *all2 = ext2 == 2;
                    set_object(&mut d.inputs, &mut d.active, TO_OBJECT2, ext2 == 1);
                });
                match ext2 {
                    0 => enter |= expr_row(ui, "Distance 2", distance2),
                    1 => enter |= expr_row(ui, "ui.extrude_to_offset_2", to_offset2),
                    _ => {}
                }
            } else {
                *all2 = false;
                set_object(&mut d.inputs, &mut d.active, TO_OBJECT2, false);
            }
            // A choice made by hand sticks, the shown one included (#6901): Extrude's own choice
            // stops before the next frame makes it.
            if crate::op_auto::op_row(ui, "ex_op", operation) {
                *auto_op = false;
                // Cutting air does nothing: Cut points into the body unless the direction was set (#18243).
                smart.point_cut = crate::dialogs::OPS.get(*operation) == Some(&"cut") && !smart.manual_dir;
            }
            // Join and Cut take their bodies in Target Bodies. A body picked there is a
            // choice by hand, so Extrude's automatic operation stops changing it.
            set_extrude_targets(&mut d.inputs, &mut d.active, &mut d.extra, *operation);
            if d.inputs.iter().any(|i| i.label == EXTRUDE_TARGETS && !i.items.is_empty()) {
                *auto_op = false;
            }
            ui_kit::section(ui, "extrude_options", "Options", true, |ui| {
                let was_thin = *thin;
                let was_direction = *direction;
                let mut reverse = *direction == 1 || *direction == TWO_SIDES_REVERSED;
                if *direction < 2 || two_sided(*direction) {
                    ui_kit::checkbox_grid(ui, &mut [("Thin Extrude", thin), ("Reverse Direction", &mut reverse)], 2);
                    *direction = match (two_sided(*direction), reverse) {
                        (true, true) => TWO_SIDES_REVERSED,
                        (true, false) => TWO_SIDES,
                        (false, r) => usize::from(r),
                    };
                    if *direction != was_direction {
                        smart.manual_dir = true;
                        *auto_op = false;
                    }
                } else {
                    ui_kit::checkbox_grid(ui, &mut [("Thin Extrude", thin)], 2);
                }
                if *thin != was_thin {
                    set_thin(&mut d.inputs, *thin);
                }
                if *thin {
                    choice_row(ui, "Wall Location", "ex_wall", wall_side, &WALL_LABELS);
                    enter |= expr_row_positive(ui, "Wall Thickness", wall);
                    if *wall_side == 3 {
                        enter |= expr_row_positive(ui, "Thickness Outside", wall2);
                    }
                }
            });
            let advanced = *offset_start || object_on(&d.inputs, FROM_OBJECT) || *taper_present || !taper2.trim().is_empty();
            // Taper Angle 2 belongs to a Distance second side (All and To Object take no taper).
            let second_distance = !*all2 && !object_on(&d.inputs, TO_OBJECT2);
            ui_kit::section(ui, "extrude_advanced", "Advanced", advanced, |ui| {
                let old = if object_on(&d.inputs, FROM_OBJECT) { 2 } else { usize::from(*offset_start) };
                let mut st = old;
                form_row(ui, "Start", |ui| {
                    combo(ui, "ex_start", &["Profile Plane", "Offset", "From Object"], &mut st);
                    if st != old {
                        *offset_start = st == 1;
                        set_object(&mut d.inputs, &mut d.active, FROM_OBJECT, st == 2);
                        if st == 1 && start.is_empty() {
                            *start = length_default(&app.session, "5");
                        }
                        if st == 0 {
                            start.clear();
                        }
                    }
                });
                if st != 0 {
                    enter |= expr_row(ui, "Offset", start);
                    if *direction == 2 && st == 1 {
                        form_row(ui, "Symmetric Offset", |ui| check(ui, symmetric_offset));
                    }
                }
                // Every side is drafted by the taper; the second of two sides by its own when
                // Taper Angle 2 is filled in (empty: the same as Taper Angle).
                if crate::dialogs::extrude_taper_shown(*direction, *thin, st == 1 && *symmetric_offset) {
                    form_row(ui, "Taper Angle", |ui| {
                        let before = taper.clone();
                        enter |= ui_kit::stepped_field(ui, taper, "", "1 deg", |text, delta| angle_step(text, delta, 1.0, None));
                        if *taper != before {
                            *taper_present = !taper.trim().is_empty();
                        }
                    });
                    if two_sided(*direction) && second_distance {
                        form_row(ui, "ui.taper_angle_2", |ui| {
                            let first = taper.clone();
                            enter |= ui_kit::stepped_field(ui, taper2, &first, "1 deg", |text, delta| {
                                angle_step(if text.trim().is_empty() { &first } else { text }, delta, 1.0, None)
                            });
                        });
                    }
                }
            });
        }
        Kind::Fillet { radius, chamfer, chain, ctype, distance2, angle, flip, setback } => {
            if *chamfer {
                choice_row(ui, "Chamfer Type", "ch_type", ctype, &CHAMFER_TYPES);
            }
            enter |= expr_row_positive(ui, if *chamfer { if *ctype == 0 { "Distance" } else { "Distance 1" } } else { "Radius" }, radius);
            // A chamfer of two distances: the row the keyboard is in is the distance its arrow and
            // the canvas box follow.
            let mut editing = ui_kit::last_value_field(ctx).filter(|id| ctx.memory(|m| m.has_focus(*id))).map(|_| 0usize);
            let mut rtype = fillet_type(&d.extra);
            if !*chamfer {
                form_row(ui, "Type", |ui| {
                    let previous = rtype;
                    let response = ui_kit::dropdown(ui, "fi_type", &mut rtype, &FILLET_TYPES.map(|(label, _)| label));
                    if previous != rtype {
                        choose_fillet_type(&mut d.extra, rtype, radius);
                    }
                    response
                })
                .backing_on_hover_text(tr("ui.fillet_type_choices_tip"));
                if rtype == 0 {
                    enter |= fillet_corner_rows(ui, &app.session, &mut d.extra, &d.inputs, radius, setback);
                    fillet_fit_rows(app, ui, &mut d.extra, setback);
                    fillet_turn_row(ui, &mut d.extra, setback);
                }
            }
            if *chamfer && *ctype == 1 {
                enter |= expr_row_positive(ui, "Distance 2", distance2);
                editing = ui_kit::last_value_field(ctx).filter(|id| ctx.memory(|m| m.has_focus(*id))).map(|_| 1usize).or(editing);
            }
            if let Some(k) = editing.filter(|_| *chamfer && *ctype == 1) {
                crate::canvas::note_chamfer_editing(&mut d.extra, k);
            }
            if *chamfer && *ctype == 2 {
                enter |= expr_row(ui, "Angle", angle);
            }
            if *chamfer && *ctype > 0 {
                form_row(ui, "Flip", |ui| check(ui, flip));
            }
            ui_kit::section(ui, "fillet_options", "Options", true, |ui| {
                ui.scope(|ui| ui_kit::checkbox_grid(ui, &mut [("Tangent Chain", chain), ("Preview", &mut d.preview_enabled)], 2))
                    .response
                    .backing_on_hover_text(tr("ui.fillet_options_tip"));
                ui.scope(|ui| form_row(ui, "ui.fillet_color_edges", |ui| check(ui, &mut app.preferences.hover_edge_lit)))
                    .response
                    .backing_on_hover_text(tr("ui.fillet_color_edges_tip"));
            });
            // The radii along the edge, for the two types that take them.
            if !*chamfer && matches!(rtype, 2 | 4) {
                ui_kit::section(ui, "variable_radius", "Variable Radius", true, |ui| {
                    if rtype == 2 {
                        form_row(ui, "End Radius", |ui| {
                            enter |= extra_field(ui, &mut d.extra, "radius2", &length_default(&app.session, "1"));
                        });
                        // Which end of the first edge takes the start radius.
                        form_row(ui, "Swap Ends", |ui| {
                            extra_check(ui, &mut d.extra, "swap_ends");
                        });
                        // Radii between the ends: position along the edge (from the start end)
                        // and radius.
                        let mut mids = mid_points(&d.extra);
                        let mut remove = None;
                        for (k, (position, r)) in mids.iter_mut().enumerate() {
                            form_row(ui, "Mid Radius", |ui| {
                                ui.horizontal(|ui| {
                                    ui_kit::fraction_field(ui, position)
                                        .backing_on_hover_text(tr("Position along the edge, 0 at the start end, 1 at the other"));
                                    let width = (ui.available_width() - t.space_24 - t.space_4).max(0.0);
                                    ui.allocate_ui_with_layout(vec2(width, t.space_24), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        enter |= field(ui, r);
                                    });
                                    if ui_kit::button(ui, "×", ui_kit::Button::Small).backing_on_hover_text(tr("Remove this point")).clicked() {
                                        remove = Some(k);
                                    }
                                });
                            });
                        }
                        if let Some(k) = remove {
                            mids.remove(k);
                        }
                        form_row(ui, "", |ui| {
                            if ui_kit::button(ui, tr("Add Mid Radius"), ui_kit::Button::Normal).clicked() && mids.len() < 20 {
                                let t = if mids.is_empty() { 0.5 } else { (mids.iter().map(|m| m.0).fold(0.0, f64::max) + 1.0) * 0.5 };
                                mids.push((t.min(0.99), radius.clone()));
                            }
                        });
                        d.extra.insert("mid".into(), json!(mids.iter().map(|(t, r)| json!([t, r])).collect::<Vec<_>>()));
                        // How the radius runs between the radii: smoothly, or straight.
                        let was = d.extra.get("smooth").and_then(Value::as_bool).unwrap_or(false);
                        let mut smooth = was;
                        ui_kit::checkbox_grid(ui, &mut [("ui.fillet_smooth_radius", &mut smooth)], 1);
                        if smooth != was {
                            d.extra.insert("smooth".into(), json!(smooth));
                        }
                    }
                    if rtype == 4 {
                        // The radius along the edge as a B-spline: its control radii from the
                        // start end, also dragged by their handles on the edge.
                        form_row(ui, "Swap Ends", |ui| {
                            extra_check(ui, &mut d.extra, "swap_ends");
                        });
                        let mut radii = spline_radii(&mut d.extra, radius);
                        let n = radii.len();
                        for (k, r) in radii.iter_mut().enumerate() {
                            enter |= expr_row_positive(ui, if k + 1 == n { "End Radius" } else { "Control Radius" }, r);
                        }
                        d.extra.insert("radii".into(), json!(radii));
                    }
                });
            }
        }
        Kind::PatternRect { .. } => {
            enter |= pattern_direction_group(app, d, ui, 0);
            enter |= pattern_direction_group(app, d, ui, 1);
            pattern_join_option(ui, d);
        }
        Kind::PatternCirc { count, angle } => {
            enter |= quantity_row(ui, &app.session.doc, "Quantity", count);
            enter |= expr_row(ui, "Total angle", angle);
            solid_pattern_alignment_row(ui, &mut d.extra, 0);
            pattern_join_option(ui, d);
        }
        Kind::Hole { kind, opts, cb_diameter, cb_depth, cs_diameter, cs_angle, .. } => {
            let doc = &app.session.doc;
            let eval = |e: &str, k| doc.eval(e, k).ok();
            // Each option shows the hole it makes while hovered (dialogs_hole::preview).
            let preview = |ui: &mut egui::Ui, choice| {
                if let Some((form, to_object)) = &hole_form {
                    crate::dialogs_hole::preview(ui, form, *to_object, choice, &eval);
                }
            };
            use crate::dialogs_hole::Choice;
            form_row(ui, "Extents", |ui| {
                let cur = if to_object_on(&d.inputs) { 1 } else { usize::from(opts.all) * 2 };
                let mut ext = cur;
                ui_kit::dropdown_previews(ui, "hole_ext", &mut ext, &["Distance", "To Object", "All"], |ui, i| preview(ui, Choice::Extents(i)));
                opts.all = ext == 2;
                set_to_object(&mut d.inputs, &mut d.active, ext == 1);
            });
            if opts.multiple {
                // Every point of the picked points' sketches (standalone points and
                // circle centres), not just the picked ones.
                form_row(ui, "All Sketch Points", |ui| {
                    extra_check(ui, &mut d.extra, "all_points");
                });
                // The holes go into the part the sketch touches; Flip drills the
                // other way.
                form_row(ui, "Flip", |ui| {
                    extra_check(ui, &mut d.extra, "flip");
                });
            }
            form_row(ui, "Hole Type", |ui| {
                ui_kit::dropdown_previews(ui, "hole_kind", kind, &HOLE_LABELS, |ui, i| preview(ui, Choice::Type(i)));
            });
            form_row(ui, "Hole Tap Type", |ui| {
                let mut tap = if opts.tapped { 2 } else { usize::from(opts.clearance) };
                ui_kit::dropdown_previews(ui, "hole_tap", &mut tap, &["Simple", "ui.hole_clearance", "Tapped"], |ui, i| preview(ui, Choice::Tap(i)));
                opts.clearance = tap == 1;
                opts.tapped = tap == 2;
            });
            // A clearance hole for a bolt: its size and fit set the diameter (ISO 273).
            if opts.clearance {
                enter |= crate::dialogs_hole::clearance_rows(ui, opts);
                // A counterbore for a cap screw, a countersink for a countersunk one: the screw's
                // head sets the recess (#12321, #17728).
                match *kind {
                    1 => crate::dialogs_hole::head_row(ui, opts, 1, cb_diameter, cb_depth),
                    2 => crate::dialogs_hole::head_row(ui, opts, 2, cs_diameter, cs_angle),
                    _ => {}
                }
            }
            if opts.tapped {
                enter |= crate::dialogs_thread::hole_rows(ui, &app.session.doc, &mut opts.thread, &mut opts.custom_thread, &mut opts.drill);
                crate::dialogs_thread::hole_hand_row(ui, &mut opts.hand);
                ui_kit::checkbox_grid(ui, &mut [("Full thread depth", &mut opts.thread_full_depth), ("Modeled", &mut opts.modeled)], 1);
                // A modelled thread's inner end, towards the bottom, as the thread dialog's.
                if opts.modeled {
                    enter |= crate::dialogs_thread::end_rows(ui, &mut opts.ease_out);
                }
            }
            // Through all, the tip lies past the part.
            if !opts.all {
                form_row(ui, "Drill Point", |ui| {
                    let mut tip = usize::from(opts.angled);
                    ui_kit::dropdown_previews(ui, "hole_tip", &mut tip, &["Flat", "Angle"], |ui, i| preview(ui, Choice::Angled(i == 1)));
                    opts.angled = tip == 1;
                });
            }
            // Counterbore only: the thickness of the layers a 3D printer bridges across its
            // floor; empty for none.
            if *kind == 1 {
                enter |= hint_row(ui, "Bridge", &mut opts.bridge, tr("None"));
            }
            // The section through the hole, each size beside its dimension (#2171).
            let to_object = to_object_on(&d.inputs);
            enter |= crate::dialogs_hole::rows(ui, &mut d.kind, to_object, &eval);
        }
        _ => enter |= additional_rows(app, d, ui),
    }
    enter |= body_name_row(d, ui);
    error_rows(app, ctx, d, ui);
    enter
}

/// The Body Name row of a feature that makes a body of its own (New Body): empty names it as the
/// command always has, a typed name is sent as `body_name` (#20776).
fn body_name_row(d: &mut Dialog, ui: &mut egui::Ui) -> bool {
    // Shown for every feature that can make a body, and greyed out unless the operation is New
    // Body: a window whose operation is still being chosen keeps its size instead of jumping as
    // the row comes and goes (only New Body sends the name, see `apply_body_name`).
    let Some(new_body) = body_name_slot(&d.kind) else { return false };
    let mut name = d.extra.get("body_name").and_then(Value::as_str).unwrap_or_default().to_string();
    let mut enter = false;
    form_row(ui, "ui.body_name", |ui| {
        ui.add_enabled_ui(new_body, |ui| {
            enter = ui_kit::text_field(ui, &mut name, tr("ui.body_name_hint")).0;
        });
    });
    if !new_body {
        return false;
    }
    if name.trim().is_empty() {
        d.extra.remove("body_name");
    } else {
        d.extra.insert("body_name".into(), json!(name));
    }
    enter
}

fn extra_segments(ui: &mut egui::Ui, extra: &mut serde_json::Map<String, Value>, key: &str, choices: &[(&str, &str)]) {
    let current = extra.get(key).and_then(Value::as_str).and_then(|value| choices.iter().position(|(_, key)| *key == value)).unwrap_or(0);
    let mut selected = current;
    let labels: Vec<_> = choices.iter().map(|(label, _)| *label).collect();
    if ui_kit::segmented(ui, &mut selected, &labels, Default::default()) && selected != current {
        if selected == 0 {
            extra.remove(key);
        } else if let Some((_, value)) = choices.get(selected) {
            extra.insert(key.into(), json!(value));
        }
    }
}

/// Only user changes add optional JSON keys, preserving no-op feature edits.
/// A pattern of bodies or features: Join bodies if possible, which joins the copies with what they
/// copy where they touch (#21625). A pattern of components places occurrences, which never join.
fn pattern_join_option(ui: &mut egui::Ui, d: &mut Dialog) {
    if d.inputs.first().is_some_and(|i| i.items.iter().any(|x| matches!(x, Sel::Occurrence { .. }))) {
        d.extra.remove("join");
        return;
    }
    extra_options(ui, "pattern_join", &mut d.extra, &[("Join bodies if possible", "join")]);
}

fn extra_options(ui: &mut egui::Ui, id: &str, extra: &mut serde_json::Map<String, Value>, choices: &[(&str, &str)]) {
    let mut values: Vec<bool> = choices.iter().map(|(_, key)| extra.get(*key).and_then(Value::as_bool).unwrap_or(false)).collect();
    let before = values.clone();
    let mut items: Vec<_> = choices.iter().zip(values.iter_mut()).map(|((label, _), value)| (*label, value)).collect();
    ui_kit::options(ui, id, &mut items);
    for (((_, key), value), before) in choices.iter().zip(values).zip(before) {
        if value != before {
            extra.insert((*key).into(), json!(value));
        }
    }
}

/// The message a primitive's size field is labelled with, by its parameter name: "Section size",
/// not `section_size`.
pub(crate) fn primitive_label(key: &str) -> &str {
    match key {
        "length" => "ui.length",
        "width" => "ui.width",
        "height" => "ui.height",
        "diameter" => "ui.diameter",
        "radius" => "ui.radius",
        "top_diameter" => "ui.top_diameter",
        "top_radius" => "ui.top_radius",
        "major" => "ui.major_radius",
        "minor" => "ui.minor_radius",
        "revolutions" => "ui.revolutions",
        "pitch" => "ui.pitch",
        "section_size" => "ui.section_size",
        _ => key,
    }
}

/// Cylinder and Sphere take their size as a diameter or a radius (#6609). Switching converts the
/// size, so the part stays as it is, and OK sends the one shown.
fn round_size_row(ui: &mut egui::Ui, fields: &mut [(&'static str, String)]) {
    let Some(field) = fields.iter_mut().find(|(k, _)| matches!(*k, "diameter" | "radius")) else { return };
    let current = usize::from(field.0 == "radius");
    let mut selected = current;
    form_row(ui, "Size", |ui| ui_kit::segmented(ui, &mut selected, &["Diameter", "Radius"], Default::default()));
    if selected != current {
        let diameter = selected == 0;
        field.1 = crate::length_input::scaled(&field.1, diameter);
        field.0 = if diameter { "diameter" } else { "radius" };
    }
}

/// Which two of a coil's revolutions, height and pitch the dialog takes: 0 revolutions and pitch
/// (the default, kept out of the parameters), 1 height and pitch, 2 height and revolutions (#20674).
fn coil_size_by(extra: &serde_json::Map<String, Value>) -> usize {
    match extra.get("size_by").and_then(Value::as_str) {
        Some("height_pitch") => 1,
        Some("height_revolutions") => 2,
        _ => 0,
    }
}

/// The "Size by" choice of the Coil dialog. Choosing a height gives it a starting value.
fn coil_size_row(ui: &mut egui::Ui, extra: &mut serde_json::Map<String, Value>, default_height: &str) -> bool {
    const KEYS: &[&str] = &["", "height_pitch", "height_revolutions"];
    let current = coil_size_by(extra);
    let mut selected = current;
    choice_row(
        ui,
        "ui.coil_size_by",
        "coil_size_by",
        &mut selected,
        &["ui.coil_by_revolutions_pitch", "ui.coil_by_height_pitch", "ui.coil_by_height_revolutions"],
    );
    if selected != current {
        match KEYS.get(selected) {
            Some(key) if !key.is_empty() => {
                extra.insert("size_by".into(), json!(key));
                extra.entry("coil_height").or_insert_with(|| json!(default_height));
            }
            _ => {
                extra.remove("size_by");
            }
        }
    }
    false
}

/// The coil's Height row, its text kept with the dialog's other options, and with a section of
/// its own, what it measures: the helix's centre line (the default, kept out of the parameters)
/// or the coil end to end (#20736).
fn coil_height_row(ui: &mut egui::Ui, extra: &mut serde_json::Map<String, Value>, from_sketch: bool) -> bool {
    let overall = !from_sketch && extra.get("height_measure").and_then(Value::as_str) == Some("overall");
    if !from_sketch {
        let mut selected = usize::from(overall);
        choice_row(ui, "ui.coil_height_measure", "coil_height_measure", &mut selected, &["ui.coil_center_line", "ui.coil_overall"]);
        if selected != usize::from(overall) {
            if selected == 1 {
                extra.insert("height_measure".into(), json!("overall"));
            } else {
                extra.remove("height_measure");
            }
        }
    }
    let mut height = extra.get("coil_height").and_then(Value::as_str).unwrap_or_default().to_string();
    let before = height.clone();
    // The helix's own height, centre line to centre line, which the coil's section reaches past;
    // or the coil's, end to end.
    let label = if overall { "ui.coil_overall_height" } else { "ui.coil_path_height" };
    let enter = expr_row(ui, label, &mut height);
    if height != before {
        extra.insert("coil_height".into(), json!(height));
    }
    enter
}

/// The coil's axis, section, where the section sits on the diameter, the way it turns and where
/// it starts. A default stays out of the parameters, so OK on an unchanged edit keeps the coil.
/// A sketch profile is the section, so the section rows are hidden then.
fn coil_rows(
    ui: &mut egui::Ui,
    extra: &mut serde_json::Map<String, Value>,
    placed_axis: Option<Vec3>,
    picked_axis: bool,
    unresolved_axis: bool,
    from_sketch: bool,
) -> bool {
    let placed_axis = placed_axis.map(pt);
    let mut axis = if unresolved_axis {
        String::new()
    } else {
        extra.get("axis").or(placed_axis.as_ref()).map(|v| v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string())).unwrap_or_default()
    };
    let before = axis.clone();
    let axis_enter = !picked_axis && ui_kit::text_row(ui, "ui.direction", &mut axis, "Z (X, Y, Z, [x, y, z])").0;
    if axis != before {
        if axis.trim().is_empty() {
            extra.remove("axis");
        } else {
            let value = serde_json::from_str::<Value>(&axis).ok().filter(|v| v.is_array() || v.is_object()).unwrap_or_else(|| json!(axis.trim()));
            extra.insert("axis".into(), value);
        }
    }
    let enter = !from_sketch && coil_section_rows(ui, extra);
    extra_options(ui, "coil_options", extra, &[("Clockwise", "clockwise"), ("Reverse Direction", "reversed")]);
    enter || axis_enter
}

/// The primitive coil's own section: its shape, where it sits on the diameter and where it starts.
fn coil_section_rows(ui: &mut egui::Ui, extra: &mut serde_json::Map<String, Value>) -> bool {
    const POSITIONS: [(&str, &str); 3] = [("Inside", "inside"), ("Center", "center"), ("Outside", "outside")];
    form_row(ui, "Section", |ui| extra_segments(ui, extra, "section", &[("Circular", "circular"), ("Square", "square"), ("Triangle", "triangle")]));
    form_row(ui, "Section position", |ui| {
        let current =
            extra.get("section_position").and_then(Value::as_str).and_then(|v| POSITIONS.iter().position(|(_, key)| *key == v)).unwrap_or(1);
        let mut selected = current;
        if ui_kit::segmented(ui, &mut selected, &POSITIONS.map(|(label, _)| label), Default::default()) && selected != current {
            match POSITIONS.get(selected) {
                Some((_, value)) if *value != "center" => {
                    extra.insert("section_position".into(), json!(value));
                }
                _ => {
                    extra.remove("section_position");
                }
            }
        }
    });
    let mut angle = extra.get("start_angle").and_then(Value::as_str).unwrap_or_default().to_string();
    let before = angle.clone();
    let enter = ui_kit::step_row(ui, "Start angle", &mut angle, "0 deg", "10 deg", |text, delta| angle_step(text, delta, 10.0, None));
    if angle != before {
        if angle.trim().is_empty() {
            extra.remove("start_angle");
        } else {
            extra.insert("start_angle".into(), json!(angle));
        }
    }
    enter
}

/// An angle text field stepped by `delta` arrows of `step_deg`: a plain number moves and keeps
/// its unit, an expression is left as typed, and `max_deg` stops the step past it.
pub(crate) fn angle_step(text: &str, delta: i64, step_deg: f64, max_deg: Option<f64>) -> Option<String> {
    let text = if text.trim().is_empty() { "0 deg" } else { text };
    let next = crate::dim_view::window::stepped(text, delta as f64, solidcraft_engine::doc::expr::Kind::Angle, "", 0.0, step_deg)?;
    match max_deg {
        Some(max) if delta > 0 && degrees(&next).is_some_and(|angle| angle > max + 1e-9) => None,
        _ => Some(next),
    }
}

/// The angle that stepped text writes, in degrees: a number with an optional unit, as the
/// stepping keeps it ("360 deg", "0,5 deg", "6.2 rad"); None for anything else.
fn degrees(text: &str) -> Option<f64> {
    let text = text.trim();
    let split = text.find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | ',' | '-' | '+' | ' '))).unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let value = ui_kit::parse_number(number)?;
    let (kind, per_unit) = solidcraft_engine::doc::expr::unit_info(if unit.trim().is_empty() { "deg" } else { unit.trim() })?;
    matches!(kind, solidcraft_engine::doc::expr::Kind::Angle).then(|| value * per_unit * 180.0 / std::f64::consts::PI)
}

fn additional_rows(app: &mut SolveApp, d: &mut Dialog, ui: &mut egui::Ui) -> bool {
    if matches!(d.kind, Kind::ScanAlign(_)) {
        crate::dialogs_scan_align::fields(ui, app, d);
        return false;
    }
    let mut enter = false;
    let through_point = offset_through_point(d).is_some();
    if crate::dialogs_move::several_destinations(d) {
        // Several To points copy to each (#22900): Create Copy ticks itself.
        d.extra.insert("copy".into(), json!(true));
    }
    // Move/Copy of a component without Create Copy moves its placement (`dialogs_move::placements`).
    let placing = !crate::dialogs_move::placements(d).is_empty();
    let shared = placing && crate::dialogs_move::moves_shared(app, d);
    // A component just pasted is placed, not copied.
    let pasted = crate::dialogs_move::pasted(d);
    // Picked components moved earlier and not captured: Revert Position puts just them back.
    let revertible = if placing { crate::dialogs_move::revertible(app, d) } else { Vec::new() };
    let unresolved_axis = unresolved_coil_axis(d, &app.session);
    match &mut d.kind {
        Kind::Revolve { angle, operation, .. } => {
            let cur = usize::from(to_object_on(&d.inputs));
            let mut ty = cur;
            form_row(ui, "Type", |ui| {
                ui_kit::segmented(ui, &mut ty, &["Angle", "To Object"], Default::default());
                set_to_object(&mut d.inputs, &mut d.active, ty == 1);
            });
            if ty == 0 {
                // One Side, Two Sides (a second angle), Symmetric (the angle split).
                let cur = if d.extra.contains_key("angle2") {
                    1
                } else if d.extra.get("direction").is_some() {
                    2
                } else {
                    0
                };
                let mut dir = cur;
                form_row(ui, "Direction", |ui| {
                    ui_kit::segmented(ui, &mut dir, &["One Side", "Two Sides", "Symmetric"], Default::default());
                    if dir != cur {
                        d.extra.remove("angle2");
                        d.extra.remove("direction");
                        match dir {
                            1 => {
                                d.extra.insert("angle2".into(), json!("90 deg"));
                            }
                            2 => {
                                d.extra.insert("direction".into(), json!("symmetric"));
                            }
                            _ => {}
                        }
                    }
                });
                enter |= ui_kit::step_row(ui, "Angle", angle, "", "10 deg", |text, delta| angle_step(text, delta, 10.0, Some(360.0)));
                if dir == 1 {
                    form_row(ui, "Angle 2", |ui| {
                        enter |= extra_field(ui, &mut d.extra, "angle2", "90 deg");
                    });
                }
            }
            crate::op_auto::op_row(ui, "rv_op", operation);
        }
        Kind::Shell { thickness } => {
            // The direction below says which side the wall grows to; the label stays short enough
            // for the label column.
            enter |= expr_row_positive(ui, "Thickness", thickness);
            form_row(ui, "Direction", |ui| {
                extra_segments(ui, &mut d.extra, "direction", &[("Inside", "inside"), ("Outside", "outside"), ("Both", "both")]);
            });
            extra_options(ui, "shell_options", &mut d.extra, &[("Tangent Chain", "tangent_chain")]);
        }
        Kind::Draft { angle } => {
            enter |= expr_row(ui, "Angle", angle);
        }
        Kind::Dome { spherical, reverse, .. } => {
            form_row(ui, "Shape", |ui| {
                let mut i = usize::from(*spherical);
                let labels = [solidcraft_ui_text::tr("ui.dome_elliptical"), solidcraft_ui_text::tr("ui.dome_spherical")];
                ui_kit::segmented(ui, &mut i, &labels, Default::default());
                *spherical = i == 1;
            });
            ui_kit::options(ui, "feature_options", &mut [(solidcraft_ui_text::tr("ui.dome_reverse"), reverse)]);
        }
        Kind::Mirror => {
            // Feature replay retains the source operation; Join is for body copies.
            if !d.inputs.first().is_some_and(|i| i.accept == selection::FEATURES) {
                let mut picked = false;
                form_row(ui, "Operation", |ui| {
                    let mut op = usize::from(d.extra.get("combine").and_then(Value::as_bool).unwrap_or(false));
                    picked = ui_kit::segmented(ui, &mut op, &["New Body", "Join"], Default::default());
                    d.extra.insert("combine".into(), json!(op == 1));
                });
                // Picked by hand: the preview's own choice, when it comes in, does not replace it.
                if picked {
                    crate::preview::mirror_operation_picked(app, d);
                }
            }
        }
        Kind::OffsetPlane { offset } => {
            if !through_point {
                enter |= expr_row(ui, "Distance", offset);
            }
        }
        Kind::AnglePlane { angle, direction_only, through_axis } => {
            enter |= expr_row(ui, "Angle", angle);
            let was = (*through_axis, *direction_only);
            // One column: both labels are long and would be cut short side by side.
            ui_kit::section(ui, "plane_angle_options", "Options", true, |ui| {
                ui_kit::checkbox_grid(ui, &mut [("Through the axis", through_axis), ("Use axis direction only", direction_only)], 1)
            });
            // The two exclude each other (#6180): the one just turned on wins.
            if *through_axis && !was.0 {
                *direction_only = false;
            } else if *direction_only && !was.1 {
                *through_axis = false;
            }
            const PIVOT: Accept = selection::VERTICES | selection::POINTS | selection::CONSTRUCTION_POINTS;
            set_object_accepting(&mut d.inputs, &mut d.active, "Pivot", PIVOT, *direction_only);
        }
        // A face tool cuts with that face extended, with the picked faces as they are, or with
        // the whole surface body (#9305). The choice shows once a face is the tool.
        Kind::Split { sheet } => {
            let face_tool = d.inputs.get(1).and_then(|inp| inp.items.first()).is_some_and(|sel| matches!(sel, Sel::Face { .. }));
            if face_tool {
                let options = SPLIT_SHEETS.map(solidcraft_ui_text::tr);
                choice_row(ui, solidcraft_ui_text::tr("ui.split_body.cut_with"), "split_sheet", sheet, &options);
            }
            let more = face_tool && *sheet == 1;
            set_object_accepting(&mut d.inputs, &mut d.active, SPLIT_TOOL_FACES, FACES, more);
            if let Some(inp) = d.inputs.iter_mut().find(|inp| inp.label == SPLIT_TOOL_FACES) {
                inp.multi = true;
            }
            // The whole surface body cuts with more surface bodies, picked several at once (#13303).
            let bodies = face_tool && *sheet == 2;
            set_object_accepting(&mut d.inputs, &mut d.active, SPLIT_TOOL_BODIES, BODIES, bodies);
            if let Some(inp) = d.inputs.iter_mut().find(|inp| inp.label == SPLIT_TOOL_BODIES) {
                inp.multi = true;
            }
        }
        // The angle turns the plane from its reference, so it shows once one is picked.
        Kind::TangentPlane { angle } => {
            if d.inputs.get(1).is_some_and(|inp| !inp.items.is_empty()) {
                enter |= expr_row(ui, "Angle", angle);
            }
        }
        Kind::PathPlane { t, .. } => {
            enter |= expr_row(ui, "Position (0 to 1)", t);
        }
        Kind::MeshConvert { tolerance } => {
            form_row(ui, "Tolerance", |ui| enter |= field(ui, tolerance));
        }
        Kind::MeshReduce { mode, count, percentage } => {
            form_row(ui, "Target", |ui| {
                ui_kit::segmented(ui, mode, &["Retained percentage", "Triangle count"], Default::default());
            });
            if *mode == 0 {
                form_row(ui, "Retained percentage", |ui| enter |= field(ui, percentage));
            } else {
                form_row(ui, "Triangle count", |ui| enter |= field(ui, count));
            }
        }
        Kind::MeshSectionStack { from, to, spacing, extrude } => {
            form_row(ui, "Spacing", |ui| enter |= field(ui, spacing));
            form_row(ui, "From", |ui| enter |= field(ui, from));
            form_row(ui, "To", |ui| enter |= field(ui, to));
            ui_kit::options(ui, "section_stack_options", &mut [("Extrude slabs", extrude)]);
        }
        Kind::Construct { t: Some(t), .. } => {
            form_row(ui, "Position (0 to 1)", |ui| enter |= field(ui, t));
        }
        Kind::ReplaceFace | Kind::Remove | Kind::Midplane | Kind::ChatContext | Kind::PluginPick { .. } | Kind::Construct { t: None, .. } => {}
        Kind::DeleteFaces { heal } => {
            ui_kit::options(ui, "delete_face_options", &mut [("ui.delete_face_heal", heal)]);
        }
        Kind::Rib { thickness, depth, flip, normal, extend, .. } => {
            form_row(ui, "Direction", |ui| {
                let mut i = usize::from(*normal);
                ui_kit::segmented(ui, &mut i, &["In plane", "Normal"], Default::default());
                *normal = i == 1;
            });
            enter |= expr_row_positive(ui, "Thickness", thickness);
            enter |= hint_row_positive(ui, "Depth", depth, tr("to the next face"));
            // A rib along the sketch normal has no ends to run on, so it has no Extend option.
            if *normal {
                ui_kit::options(ui, "feature_options", &mut [("Flip", flip)]);
            } else {
                let extend_label = solidcraft_ui_text::tr("ui.rib_extend_ends");
                ui_kit::options(ui, "feature_options", &mut [("Flip", flip), (extend_label, extend)]);
            }
        }
        Kind::Emboss { depth, deboss, offset_x, offset_y, angle } => {
            enter |= expr_row(ui, "Depth", depth);
            form_row(ui, "Type", |ui| {
                let mut i = usize::from(*deboss);
                ui_kit::segmented(ui, &mut i, &["Emboss", "Deboss"], Default::default());
                *deboss = i == 1;
            });
            // Where the profiles go on the face, apart from where the sketch has them (#20386):
            // along the sketch's x and y, and turned about their middle.
            enter |= hint_row(ui, "Offset X", offset_x, tr("none"));
            enter |= hint_row(ui, "Offset Y", offset_y, tr("none"));
            enter |= hint_row(ui, "Angle", angle, tr("none"));
            ui.add(egui::Label::new(solidcraft_ui_text::tr("ui.emboss_wrap_note")).wrap());
        }
        Kind::Align { flip, components, axes: [ax, ay, az], angle } => {
            // Where on the picked face or edge to snap: as picked (a face turns to meet
            // the other, an edge at its middle), its center, a round one's center, or, for bodies,
            // the joint origin the Joint dialog makes of it (found again as the part changes, #6257).
            let snaps: &[(&str, &str)] = if *components {
                &[("As Picked", "picked"), ("Center", "center"), ("Circle Center", "circle")]
            } else {
                &[("As Picked", "picked"), ("Center", "center"), ("Circle Center", "circle"), ("Joint Origin", "origin")]
            };
            // A planar To face as the plane it lies in: the moved face comes onto it along its
            // normal only, without sliding across to the picked point.
            let to_snaps: &[(&str, &str)] = if *components {
                snaps
            } else {
                &[("As Picked", "picked"), ("Center", "center"), ("Circle Center", "circle"), ("Joint Origin", "origin"), ("Onto Plane", "plane")]
            };
            for (label, key, choices) in [("From Snap", "from_snap", snaps), ("To Snap", "to_snap", to_snaps)] {
                form_row(ui, label, |ui| {
                    extra_choice(ui, &mut d.extra, key, key, choices);
                });
            }
            if *components {
                ui_kit::options(ui, "feature_options", &mut [("Flip", flip)]);
            } else {
                // A turn about the target's direction after the move (#6257).
                enter |= hint_row(ui, "Angle", angle, tr("none"));
                ui_kit::options(ui, "feature_options", &mut [("Flip", flip), ("X", ax), ("Y", ay), ("Z", az)]);
            }
            // A part inside a subassembly placed more than once sits where that shared
            // definition says: aligning it moves it in every placement.
            if *components && align_moves_shared(&app.session, d.inputs.first().map_or(&[], |i| i.items.as_slice())) {
                let t = Tokens::get();
                ui.add(egui::Label::new(RichText::new(tr(ALIGN_SHARED_WARNING)).color(t.warning).size(t.font_small)).wrap());
            }
        }
        Kind::BoundaryFill { operation, cells, .. } => {
            form_row(ui, "Cells", |_| {});
            if cells.is_empty() {
                ui_kit::status_line(ui, "Pick the tool bodies", ui_kit::StatusTone::Dim);
            }
            for (i, (_, label, on)) in cells.iter_mut().enumerate() {
                form_row(ui, &format!("Cell {}", i + 1), |ui| {
                    ui_kit::checkbox(ui, label.as_str(), on);
                });
            }
            choice_row(ui, "Operation", "bf_op", operation, &OP_LABELS);
            extra_options(ui, "boundary_options", &mut d.extra, &[("Remove Tools", "remove_tools")]);
        }
        Kind::PathPattern { count, spacing, start, fraction } => {
            enter |= quantity_row(ui, &app.session.doc, "Quantity", count);
            enter |= expr_row(ui, if d.extra.contains_key("distance") { "Distance" } else { "Spacing" }, spacing);
            // Where along the path the original sits: a distance, or a fraction of the path's
            // length that keeps its place as the path changes. Empty: the path's start.
            form_row(ui, "pattern.start_point", |ui| {
                let mut i = usize::from(*fraction);
                if ui_kit::segmented(ui, &mut i, &["pattern.start_distance", "pattern.start_fraction"], Default::default()) && (i == 1) != *fraction {
                    *fraction = i == 1;
                    start.clear();
                }
            });
            // Both labels about as wide as the dialog's others: switching keeps the label column
            // where it is, so the rows don't shift and Start point's choice stays two buttons.
            enter |= hint_row(ui, if *fraction { "  Position" } else { "  Distance" }, start, "0");
            // Identical: copies keep the original's orientation; Path direction: they turn with
            // the path, keeping the original's relation to it.
            form_row(ui, "Orientation", |ui| {
                extra_choice(
                    ui,
                    &mut d.extra,
                    "pp_orientation",
                    "orientation",
                    &[("pattern.orientation_identical", "identical"), ("pattern.orientation_path", "path")],
                );
            });
            extra_options(ui, "feature_options", &mut d.extra, &[("Flip direction", "flip")]);
        }
        Kind::Pipe { diameter, wall, hollow, operation } => {
            enter |= expr_row(ui, "Diameter", diameter);
            // A pipe is a solid rod or a hollow tube (#18344). The Type row says which one, and the
            // Wall row stays where it was: a thickness typed in it makes a tube, as it always did,
            // so nothing the user typed is ever dropped. Choosing Solid does not empty the Wall row:
            // the thickness the user typed stays there, and the command carries it.
            choice_row(ui, "Type", "pipe_type", hollow, &PIPE_TYPES);
            let was_wall = (*wall).clone();
            enter |= hint_row(ui, "Wall", wall, if *hollow == 1 { tr("ui.thickness") } else { tr("solid") });
            if *wall != was_wall {
                // A thickness in the field is a tube; an empty field is a rod (#18344).
                *hollow = usize::from(!wall.trim().is_empty());
            }
            choice_row(ui, "Operation", "pp_op", operation, &OP_LABELS);
        }
        Kind::Stock { margin } => {
            enter |= expr_row(ui, "Margin", margin);
        }
        Kind::Scale { factor, per_axis, factor_y, factor_z } => {
            form_row(ui, "Type", |ui| {
                let mut i = usize::from(*per_axis);
                ui_kit::segmented(ui, &mut i, &["scale.uniform", "scale.per_axis"], Default::default());
                if (i == 1) != *per_axis {
                    *per_axis = i == 1;
                    // Y and Z start at the factor, so nothing changes until one of them does.
                    if *per_axis {
                        factor_y.clone_from(factor);
                        factor_z.clone_from(factor);
                    }
                }
            });
            if *per_axis {
                enter |= expr_row_positive(ui, "X", factor);
                enter |= expr_row_positive(ui, "Y", factor_y);
                enter |= expr_row_positive(ui, "Z", factor_z);
            } else {
                enter |= expr_row_positive(ui, "Scale factor", factor);
            }
        }
        Kind::OffsetFaces { distance } => {
            enter |= expr_row(ui, "Distance", distance);
        }
        Kind::TransformFace { translate, axis, custom_axis, angle, explicit_pivot, origin } => {
            for (label, v) in ["X Distance", "Y Distance", "Z Distance"].into_iter().zip(translate) {
                enter |= expr_row(ui, label, v);
            }
            form_row(ui, "Axis", |ui| {
                ui_kit::segmented(ui, axis, &["X", "Y", "Z", "Custom"], Default::default());
            });
            if *axis == 3 {
                for (label, v) in ["X", "Y", "Z"].into_iter().zip(custom_axis) {
                    enter |= ui_kit::text_row(ui, label, v, "").0;
                }
            }
            enter |= expr_row(ui, "Angle", angle);
            if *explicit_pivot {
                for (label, v) in ["ui.transform_face.pivot_x", "ui.transform_face.pivot_y", "ui.transform_face.pivot_z"].into_iter().zip(origin) {
                    enter |= expr_row(ui, label, v);
                }
            }
        }
        Kind::Thread { designation, length, suggested, of, options, .. } => {
            let mut modeled = d.extra.get("modeled").and_then(Value::as_bool).unwrap_or(false);
            let before = modeled;
            let (submitted, reset) = crate::dialogs_thread::rows(ui, &app.session.doc, designation, suggested, length, options, &mut modeled);
            if modeled != before {
                d.extra.insert("modeled".into(), json!(modeled));
            }
            enter |= submitted;
            if reset {
                of.clear();
            }
            crate::dialogs_thread::sync_inputs(&mut d.inputs, &mut d.active, options, &app.session.doc);
        }
        Kind::Loft { operation, rail_fit } => {
            crate::op_auto::op_row(ui, "lf_op", operation);
            crate::dialogs_sweep::rail_fit_row(ui, "lf", rail_fit);
        }
        Kind::Sweep { operation, opts } => {
            enter |= crate::dialogs_sweep::rows_with(ui, opts, "sw", |ui| {
                crate::op_auto::op_row(ui, "sw_op", operation);
            });
        }
        k @ Kind::Move { .. } => {
            enter |= crate::dialogs_move::rows(ui, k);
            crate::dialogs_move::kept_axis_note(ui, &d.inputs, &d.extra);
            if shared {
                ui_kit::text(
                    ui,
                    solidcraft_ui_text::tr_label("This child is shared. Position changes apply to every copy of its parent."),
                    ui_kit::Text::Note("shared"),
                );
            }
            if placing {
                // Ticked at first, as the Browser's move captured.
                d.extra.entry("capture").or_insert(json!(true));
                let capture = ("Capture All Positions", "capture");
                let options: &[(&str, &str)] = if pasted { &[capture] } else { &[("Create Copy", "copy"), capture] };
                extra_options(ui, "move_options", &mut d.extra, options);
                if !revertible.is_empty() && ui_kit::button(ui, tr("Revert Position"), ui_kit::Button::Normal).clicked() {
                    let _ = app.run("component.revert_position", json!({ "occurrences": revertible }));
                }
            } else {
                // Keep Axes With Body takes along axes that run through the moved bodies (a
                // Center Pole); it shows on every move (not a copy) so it can be found before a
                // pole is added.
                let copying = d.extra.get("copy").and_then(Value::as_bool).unwrap_or(false);
                let mut choices = vec![("Create Copy", "copy")];
                if !copying {
                    let keep = crate::dialogs_move::keeps_axes(app, d);
                    d.extra.insert("keep_axes".into(), json!(keep));
                    choices.push((app.tr("move.keep_axes"), "keep_axes"));
                }
                extra_options(ui, "move_options", &mut d.extra, &choices);
            }
        }
        Kind::Primitive { cmd, fields, operation, at, .. } => {
            let from_sketch =
                *cmd == "solid.coil" && d.inputs.first().is_some_and(|i| i.items.iter().any(|item| matches!(item, Sel::Profile { .. })));
            if matches!(*cmd, "solid.cylinder" | "solid.sphere") {
                round_size_row(ui, fields);
            }
            let coil_by = if *cmd == "solid.coil" { coil_size_by(&d.extra) } else { 0 };
            if *cmd == "solid.coil" {
                enter |= coil_size_row(ui, &mut d.extra, &length_default(&app.session, "20"));
            }
            for (k, v) in fields.iter_mut() {
                if from_sketch && (*k == "diameter" || *k == "section_size") {
                    continue;
                }
                // Sized by height: the Height row takes the slot of the field it replaces.
                if (coil_by == 1 && *k == "revolutions") || (coil_by == 2 && *k == "pitch") {
                    enter |= coil_height_row(ui, &mut d.extra, from_sketch);
                    continue;
                }
                // Revolutions steps by whole turns like the other counts (#7481).
                enter |= if *k == "revolutions" {
                    count_row_to(ui, &app.session.doc, primitive_label(k), v, MAX_COIL_REVOLUTIONS)
                } else {
                    expr_row(ui, primitive_label(k), v)
                };
            }
            if *cmd == "solid.coil" {
                let picked_axis = d.inputs.get(1).is_some_and(|input| !input.items.is_empty());
                let placed_axis = d.inputs.first().and_then(|input| input.items.first()).and_then(|sel| match sel {
                    Sel::Plane { name } => crate::dialogs::plane_by_name(&app.session, name).map(|plane| plane.normal()),
                    _ => None,
                });
                enter |= coil_rows(ui, &mut d.extra, placed_axis.or(at.and_then(|(_, normal)| normal)), picked_axis, unresolved_axis, from_sketch);
            }
            if at.is_none() {
                enter |= crate::primitive_placement::rows(ui, &app.session, cmd, &mut d.extra);
            }
            if let Some((p, _)) = *at {
                form_row(ui, "Position", |ui| {
                    ui.horizontal(|ui| {
                        ui.label(format!("{:.2}, {:.2}, {:.2}", p.x, p.y, p.z));
                        // A cylinder's place is its Face pick, cleared there.
                        if *cmd != "solid.cylinder"
                            && ui_kit::button(ui, "×", ui_kit::Button::Small).backing_on_hover_text(tr("Place at the origin instead")).clicked()
                        {
                            *at = None;
                        }
                    });
                });
            }
            crate::op_auto::op_row(ui, "pr_op", operation);
        }
        Kind::Combine { operation, keep_tools, offset, round_edges, picked_faces, radius } => {
            form_row(ui, "Operation", |ui| {
                let mut op = operation.saturating_sub(1);
                combo(ui, "cb_op", &OP_LABELS[1..], &mut op);
                *operation = op + 1;
            });
            // A cut's clearance: the tools grow by the offset before they cut (#3774).
            if *operation == 2 {
                enter |= expr_row(ui, "ui.combine_offset", offset);
                let mut which = usize::from(*picked_faces);
                choice_row(
                    ui,
                    "ui.combine_offset_which",
                    "cb_offset_which",
                    &mut which,
                    &["ui.combine_offset_all_faces", "ui.combine_offset_picked_faces"],
                );
                *picked_faces = which == 1;
                ui_kit::checkbox_grid(ui, &mut [("ui.combine_offset_round_edges", round_edges)], 1);
                if *round_edges {
                    // Empty: the rounds take the offset's size, as the hint says.
                    enter |= hint_row_positive(ui, "ui.combine_offset_radius", radius, tr("ui.combine_offset_radius_hint"));
                }
            }
            crate::dialogs::set_combine_offset_faces(&mut d.inputs, &mut d.active, *operation == 2 && *picked_faces);
            ui_kit::section(ui, "combine_options", "Options", true, |ui| {
                let mut new_component = d.extra.get("new_component").and_then(Value::as_bool).unwrap_or(false);
                let before = new_component;
                ui_kit::checkbox_grid(ui, &mut [("New Component", &mut new_component), ("Keep Tools", keep_tools)], 2);
                if new_component != before {
                    d.extra.insert("new_component".into(), json!(new_component));
                }
            });
        }
        Kind::FingerJoint { count, clearance, inverted, edge_offset } => {
            enter |= finger_joint_count_row(ui, app, count);
            enter |= expr_row(ui, "ui.finger_joint_clearance", clearance);
            enter |= expr_row(ui, "ui.finger_joint_edge_offset", edge_offset);
            ui_kit::checkbox_grid(ui, &mut [("ui.finger_joint_inverted", inverted)], 1);
        }
        Kind::Surface(k) => {
            enter |= crate::dialogs_surface::rows(ui, k, &mut d.inputs, &mut d.active);
            // Extend's Distance drops a kept plane or face (one the dialog couldn't pick), so
            // going back to To Object needs a new pick; the null clears it from the feature.
            if matches!(k, crate::dialogs_surface::Surf::Extend { to_object: false, .. })
                && d.extra.get("up_to").is_some_and(|v| !v.is_null() && v != "bounds")
            {
                d.extra.insert("up_to".into(), Value::Null);
            }
        }
        Kind::Sheet(_) => {
            crate::dialogs_sheet::discover_flange_corners(app, d);
            if let Kind::Sheet(k) = &mut d.kind {
                enter |= crate::dialogs_sheet::rows(app, ui, k, &mut d.inputs, &d.extra);
            }
        }
        Kind::Plastic(k) => enter |= crate::dialogs_plastic::rows(app, ui, k, &d.inputs),
        Kind::Frame(_) => enter |= crate::dialogs_frame::rows(app, ui, d),
        Kind::EndCap(k) => enter |= crate::dialogs_end_cap::rows(app, ui, k, &d.inputs),
        Kind::Dogbone(k) => enter |= crate::dialogs_dogbone::rows(app, ui, k, &mut d.extra),
        Kind::Pulley(k) => enter |= crate::dialogs_pulley::rows(app, ui, k),
        Kind::CellGrid(k) => enter |= crate::dialogs_cell_grid::rows(app, ui, k, &d.inputs),
        Kind::RotateMember(k) => enter |= crate::dialogs_rotate_member::rows(app, ui, k, &mut d.inputs),
        Kind::FrameTrim(k) => enter |= crate::dialogs_frame::trim_rows(ui, k),
        Kind::Gusset(k) => enter |= crate::dialogs_gusset::rows(app, ui, k),
        Kind::FramePlate(k) => enter |= crate::dialogs_frame_plate::rows(app, ui, k),
        Kind::CapturePosition(k) => enter |= crate::dialogs_capture::rows(app, ui, k),
        Kind::Assembly(k) => enter |= crate::dialogs_assembly::rows(app, ui, k, &mut d.inputs, &mut d.active),
        Kind::Measure { result, show, dual_units, spacing, .. } => measure_rows(app, ui, result.as_ref(), show, dual_units, spacing),
        Kind::Section { offset, flip, hatch, add, edit } => {
            enter |= expr_row(ui, "Distance", offset);
            ui_kit::options(ui, "feature_options", &mut [("Flip", flip), ("Hatch", hatch)]);
            if edit.is_some() {
                ui_kit::text(ui, tr("section.edit_hint"), ui_kit::Text::Hint("section_edit"));
            } else if *add {
                ui_kit::text(ui, tr("section.add_hint"), ui_kit::Text::Hint("section_add"));
            } else if section_turns_off(&app.session, d.extra.contains_key("plane"), &d.inputs) {
                ui_kit::text(ui, tr("section.off_hint"), ui_kit::Text::Hint("section_off"));
            }
            saved_section_rows(app, d, ui);
        }
        Kind::Rename { name, .. } | Kind::RenameGroup { name, .. } => {
            form_row(ui, "Name", |ui| {
                enter |= ui_kit::focused_text_field(ui, name, "");
            });
        }
        _ => {}
    }
    // A Cut with bodies in several components lists the ones it may cut (#20647).
    crate::cut_bodies::rows(app, d, ui);
    enter
}

/// What the saved-sections list keeps between frames: the name being typed, the saved section
/// being renamed, and what the last action said when it failed.
#[derive(Clone, Default)]
struct SavedSectionDraft {
    name: String,
    renaming: Option<String>,
    error: String,
}

/// The design's saved Section Analyses (#20649): pick one to show it, save the section on show
/// under a name, rename or delete one. Each action is a command.
fn saved_section_rows(app: &mut SolveApp, d: &mut Dialog, ui: &mut egui::Ui) {
    let id = ui.scope_id().with("saved_sections");
    let mut draft: SavedSectionDraft = ui.data(|data| data.get_temp(id)).unwrap_or_default();
    let list = app.run("inspect.section.list", json!({})).unwrap_or(Value::Null);
    let shown = list.get("shown").and_then(Value::as_str).map(str::to_string);
    let names: Vec<String> = list
        .get("sections")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|x| x.get("name").and_then(Value::as_str).map(str::to_string))
        .collect();
    let live = !app.session.section.is_empty() || !app.session.section_hidden.is_empty();
    if names.is_empty() && !live {
        return;
    }
    ui_kit::text(ui, tr("section.saved_title"), ui_kit::Text::Hint("section_saved"));
    let mut act: Option<(&'static str, Value)> = None;
    for name in &names {
        // A long name or a long translation wraps to the next line instead of widening the card.
        ui.horizontal_wrapped(|ui| {
            let on = shown.as_deref() == Some(name.as_str());
            let look = if on { ui_kit::Button::SmallSelected } else { ui_kit::Button::Small };
            if ui_kit::button(ui, name.as_str(), look).clicked() {
                act = Some(("inspect.section.show", json!({"section": name})));
            }
            if ui_kit::button(ui, tr("section.saved_rename"), ui_kit::Button::Small).clicked() {
                draft.renaming = Some(name.clone());
                draft.name = name.clone();
            }
            if ui_kit::button(ui, "×", ui_kit::Button::Small).backing_on_hover_text(tr("section.saved_delete")).clicked() {
                act = Some(("inspect.section.delete", json!({"section": name})));
            }
        });
    }
    // The button takes its own width first and the name field the rest of the row, so a long
    // translation ("Speichern als…") cannot push the row, and with it the card, past the window.
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let renaming = draft.renaming.clone();
            let label = tr(if renaming.is_some() { "section.saved_rename" } else { "section.saved_save" });
            let ok = !draft.name.trim().is_empty() && (renaming.is_some() || live);
            let clicked = ui_kit::button(ui, label, ui_kit::Button::Enabled(ok)).clicked();
            let (enter, _) = ui_kit::text_field(ui, &mut draft.name, tr("section.saved_name"));
            if (clicked || enter) && ok {
                act = Some(match renaming {
                    Some(old) => ("inspect.section.rename", json!({"section": old, "name": draft.name})),
                    None => ("inspect.section.save", json!({"name": draft.name})),
                });
            }
        });
    });
    if let Some((cmd, params)) = act {
        match app.run(cmd, params) {
            Ok(_) => {
                draft.error.clear();
                if cmd == "inspect.section.show" {
                    // The dialog's preview and OK now stand for the section just shown.
                    crate::dialogs::load_shown_section(app, d);
                } else {
                    draft.name.clear();
                    draft.renaming = None;
                }
            }
            Err(e) => draft.error = e,
        }
    }
    if !draft.error.is_empty() {
        ui_kit::text(ui, &draft.error, ui_kit::Text::Error("section_saved_error"));
    }
    ui.data_mut(|data| data.insert_temp(id, draft));
}

/// The spacing of three or more round items in the Measure panel (#10170): what their centres
/// lie on, each neighbouring pair's gap in order (the items numbered as the panel lists them),
/// the span or the pitch circle, and whether the gaps are even.
fn measure_spacing_rows(app: &SolveApp, ui: &mut egui::Ui, sp: &Value, dual: bool) {
    let format = solidcraft_ui_text::format;
    let mm = |x: f64| crate::prefs::show_measure(app, x, 1, dual);
    let line = |ui: &mut egui::Ui, id: &str, text: String| ui_kit::text(ui, &text, ui_kit::Text::Report(&format!("measure_spacing_{id}"), false));
    let order: Vec<usize> = sp["order"].as_array().into_iter().flatten().filter_map(|v| v.as_u64()).map(|i| i as usize).collect();
    let circle = sp["pattern"].as_str() == Some("circle");
    let pattern = match sp["pattern"].as_str() {
        Some("line") => "measure.spacing_row",
        Some("circle") => "measure.spacing_circle",
        _ => "measure.spacing_none",
    };
    line(ui, "pattern", format(pattern, &[("count", &order.len().to_string())]));
    let odd = |g: usize| sp["odd_gaps"].as_array().is_some_and(|o| o.iter().any(|v| v.as_u64() == Some(g as u64)));
    for (g, gap) in sp["gaps"].as_array().into_iter().flatten().filter_map(Value::as_f64).enumerate() {
        // The panel numbers the items from 1, in the order they were picked.
        let number = |k: usize| order.get(k % order.len().max(1)).map(|i| (i + 1).to_string()).unwrap_or_default();
        let id = if odd(g) { "measure.spacing_gap_odd" } else { "measure.spacing_gap" };
        let mut text = format!("{}  {}", format(id, &[("from", &number(g)), ("to", &number(g + 1))]), mm(gap));
        if let Some(a) = sp["angle_deg"].get(g).and_then(Value::as_f64) {
            text.push_str(&format!("  {a:.3}°"));
        }
        line(ui, &format!("gap_{g}"), text);
    }
    if let (true, Some(d)) = (circle, sp["pcd"].as_f64()) {
        line(ui, "pcd", format!("{}  {}", tr("measure.spacing_pcd"), mm(d)));
    } else if let Some(span) = sp["span"].as_f64() {
        line(ui, "span", format!("{}  {}", tr("measure.spacing_span"), mm(span)));
    }
    let pitch = sp["pitch"].as_f64();
    match (sp["even"].as_bool() == Some(true), pitch) {
        (true, Some(p)) => line(ui, "even", format!("{}  {}", tr("measure.spacing_even"), mm(p))),
        (_, Some(p)) => line(ui, "even", format!("{}  {}", tr("measure.spacing_uneven_most"), mm(p))),
        _ => line(ui, "even", tr("measure.spacing_uneven").to_string()),
    };
    if let Some(a) = sp["angle_pitch_deg"].as_f64() {
        line(ui, "angle", format!("{}  {a:.3}°", tr("measure.spacing_angle")));
    }
}

/// The measurement in the dialog: each item's size, then the distance (a pair of round items
/// offers centre to centre, closest and farthest), its ΔX/Y/Z and the angle between two items;
/// for three or more round items, their spacing ([`measure_spacing_rows`]).
fn measure_rows(app: &SolveApp, ui: &mut egui::Ui, r: Option<&Value>, show: &mut Option<Shown>, dual: &mut bool, spacing: &mut bool) {
    let toggle = ui_kit::checkbox(ui, tr("measure.dual_units"), dual);
    crate::scenario::publish_handle("measure:dual_units", toggle.rect.center());
    let toggle = ui_kit::checkbox(ui, tr("measure.spacing_option"), spacing);
    crate::scenario::publish_handle("measure:spacing", toggle.rect.center());
    let Some(r) = r else {
        let hint = if *spacing { tr("measure.spacing_hint") } else { tr("pick one or two vertices, edges or faces") };
        ui_kit::text(ui, hint, ui_kit::Text::Hint("measure"));
        return;
    };
    if let Some(e) = r.get("error").and_then(Value::as_str) {
        ui_kit::text(ui, e, ui_kit::Text::Report("measure_error", true));
        return;
    }
    if let Some(sp) = r.get("spacing").filter(|sp| sp.is_object()) {
        // Each item's diameter only: a row's own figures follow.
        for (i, it) in r["items"].as_array().into_iter().flatten().enumerate() {
            let label = solidcraft_ui_text::format("measure.item", &[("number", &(i + 1).to_string())]);
            let size = crate::quick_measure::item_lines_with_units(app, it, *dual).into_iter().next().unwrap_or_default();
            ui_kit::text(ui, &format!("{label}  {size}"), ui_kit::Text::Report(&format!("measure_item_{i}_0"), false));
        }
        measure_spacing_rows(app, ui, sp, *dual);
        return;
    }
    let mm = |value: &Value| value.as_f64().map(|value| crate::prefs::show_measure(app, value, 1, *dual)).unwrap_or_default();
    let mut rows = Vec::new();
    for (i, item) in r["items"].as_array().into_iter().flatten().enumerate() {
        for (k, line) in crate::quick_measure::item_lines_with_units(app, item, *dual).into_iter().enumerate() {
            // Item descriptions include coordinate triples: keep the complete line wrapping
            // above the comparable results, without widening their value/action columns.
            let text = if k == 0 {
                let item = solidcraft_ui_text::format("measure.item", &[("number", &(i + 1).to_string())]);
                format!("{item}  {line}")
            } else {
                line
            };
            ui_kit::text(ui, &text, ui_kit::Text::Report(&format!("measure_item_{i}_{k}"), false));
        }
    }
    let distance = shown_distance(r, *show);
    let current = distance.as_ref().map(|(now, ..)| *now);
    if let Some((_, distance, a, b)) = distance {
        if r.get("center_distance").is_some() {
            for (which, label, key) in [
                (Shown::Centre, crate::quick_measure::centre_distance_label(r), "center_distance"),
                (Shown::Min, tr("Min"), "distance_mm"),
                (Shown::Max, tr("Max"), "max"),
            ] {
                if let Some(value) = r.get(key).filter(|value| value.is_number()) {
                    rows.push((key.to_owned(), label.to_owned(), mm(value), Some(which), false));
                }
            }
        } else {
            rows.push((
                "distance".to_owned(),
                solidcraft_ui_text::tr_label("Distance").to_owned(),
                crate::prefs::show_measure(app, distance, 1, *dual),
                None,
                true,
            ));
        }
        let delta = b - a;
        for (label, value) in [("ΔX", delta.x), ("ΔY", delta.y), ("ΔZ", delta.z)] {
            rows.push((label.to_owned(), label.to_owned(), crate::prefs::show_measure(app, value.abs(), 1, *dual), None, false));
        }
    }
    if let Some(angle) = r["angle_deg"].as_f64().filter(|_| r["items"].as_array().is_some_and(|items| items.len() == 2)) {
        rows.push(("angle".to_owned(), solidcraft_ui_text::tr_label("Angle").to_owned(), format!("{angle:.3}°"), None, false));
    }
    if rows.is_empty() {
        return;
    }
    // Dual-unit values are a wrapping report, not fixed-height cells: main keeps both units
    // and every coordinate visible even at high precision in a narrow tool panel.
    if *dual {
        for (key, name, value, choice, copy) in &rows {
            if let Some(choice) = choice {
                if ui_kit::list_picker(ui, key, name, Some(*choice) == current).clicked() {
                    *show = Some(*choice);
                }
                ui_kit::text(ui, value, ui_kit::Text::Report(&format!("measure_distance_{key}"), false));
            } else if *copy {
                ui.horizontal_top(|ui| {
                    let size = solidcraft_theme::Tokens::get().form_close_size;
                    let width = (ui.available_width() - size - ui.spacing().item_spacing.x).max(0.0);
                    ui.allocate_ui_with_layout(egui::vec2(width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                        let text = ui_kit::text(ui, &format!("{name}  {value}"), ui_kit::Text::Report("measure_distance", false));
                        crate::scenario::publish_rect(&format!("dialog:report:{key}"), text.rect);
                    });
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
                    let copy = ui_kit::icon_button_at(ui, rect, "copy", solidcraft_ui_text::tr_label("Copy"));
                    crate::scenario::publish_target("measure:copy:distance", copy.rect);
                    if copy.clicked() {
                        ui.ctx().copy_text(value.clone());
                    }
                });
            } else {
                let legacy = format!("measure_value_{name}");
                let text = ui_kit::text(ui, &format!("{name}  {value}"), ui_kit::Text::Report(&legacy, false));
                crate::scenario::publish_rect(&format!("dialog:report:{key}"), text.rect);
            }
        }
        return;
    }
    let width = rows.iter().map(|(_, _, value, _, _)| ui_kit::ContentWidth::label(ui, value)).fold(ui_kit::row_height(), f32::max);
    let columns = [
        ui_kit::Column::new(solidcraft_ui_text::tr_label("Name"), 1.0).flex(),
        ui_kit::Column::new(solidcraft_ui_text::tr_label("Value"), 2.0).flex().min_content_width(width),
        ui_kit::Column::new("", 0.0).icon_only(),
    ];
    ui_kit::report_list(ui, "measure_results", ui_kit::table_row_height() * 10.0, &columns, "", |ui, table| {
        for (key, name, value, choice, copy) in &rows {
            let mut clicked = false;
            let response = table.row(ui, key, choice.is_some() && *choice == current, |row| {
                clicked |= row.cell(|ui| ui_kit::text(ui, name, ui_kit::Text::Cell)).clicked();
                let cell = row.cell(|ui| ui_kit::text(ui, value, ui_kit::Text::Cell));
                clicked |= cell.clicked();
                crate::scenario::publish_rect(&format!("dialog:report:{key}"), cell.rect);
                row.cell(|ui| {
                    if *copy {
                        let button = ui_kit::StageBar::icon_action(ui, "copy", solidcraft_ui_text::tr_label("Copy"));
                        crate::scenario::publish_target("measure:copy:distance", button.rect);
                        if button.clicked() {
                            ui.ctx().copy_text(value.clone());
                        }
                    }
                });
            });
            crate::scenario::publish_target(&format!("dialog:row:{name}"), response.rect);
            if let Some(choice) = choice {
                crate::scenario::publish_target(&format!("dialog:list:{key}"), response.rect);
                if clicked || response.clicked() {
                    *show = Some(*choice);
                }
            }
        }
    });
}

/// The origin planes the view sees edge-on, as lines with nothing to click (#17606): in a top
/// view XZ and YZ, in a front view XY and YZ. Create Sketch then says where else to pick them.
pub(crate) fn edge_on_origin_planes(cam: &solidcraft_viewport::Camera) -> Vec<&'static str> {
    let back = cam.back();
    [("XY", back.z), ("XZ", back.y), ("YZ", back.x)].into_iter().filter(|(_, along_normal)| along_normal.abs() < 1e-2).map(|(name, _)| name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::{self, Harness};

    #[test]
    fn dual_measure_values_wrap_at_high_precision_in_a_narrow_panel() {
        let mut h = Harness::new();
        h.app.preferences.length_decimals = 8;
        for width in [200.0, 264.0] {
            for round_pair in [false, true] {
                let mut r = json!({"items":[],"distance_mm":13.0,"from":[0,0,0],"to":[3,4,12]});
                if round_pair {
                    r["center_distance"] = json!(13.0);
                    r["center_from"] = json!([0, 0, 0]);
                    r["center_to"] = json!([3, 4, 12]);
                    r["max"] = json!(25.4);
                    r["max_from"] = json!([0, 0, 0]);
                    r["max_to"] = json!([25.4, 0, 0]);
                }
                let ctx = egui::Context::default();
                crate::fonts::setup_bundled_fonts(&ctx);
                for frame in 0..3 {
                    let output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 700.0))),
                            time: Some(frame as f64 / 60.0),
                            ..Default::default()
                        },
                        |ui| measure_rows(&h.app, ui, Some(&r), &mut None, &mut true, &mut false),
                    );
                    if frame != 2 {
                        continue;
                    }
                    for text in [
                        "13.00000000 mm [0.51181102 in]",
                        "3.00000000 mm [0.11811024 in]",
                        "4.00000000 mm [0.15748031 in]",
                        "12.00000000 mm [0.47244094 in]",
                    ] {
                        let values: Vec<_> = output
                            .shapes
                            .iter()
                            .filter_map(|shape| match &shape.shape {
                                egui::Shape::Text(t) if t.galley.text().contains(text) => Some((shape.clip_rect, t)),
                                _ => None,
                            })
                            .collect();
                        assert!(!values.is_empty(), "{width}: {text}");
                        for (clip, t) in values {
                            assert!(!t.galley.elided, "{width}: {text}");
                            assert!(clip.expand(1.0).contains_rect(t.visual_bounding_rect()), "{width}: {text}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn measure_table_keeps_distance_selection_and_copy() {
        let h = Harness::new();
        let _language = solidcraft_ui_text::scope("en");
        let undo = h.app.session.undo.len();
        for dual in [false, true] {
            let mut shown = None;
            let result = json!({"items":[{"type":"point","position":[1234,5678,9012]}],
            "distance_mm":4,"from":[0,0,0],"to":[4,0,0],
            "center_distance":10,"center_from":[0,0,0],"center_to":[10,0,0],
            "max":16,"max_from":[0,0,0],"max_to":[16,0,0]});
            let draw = |shown: &mut Option<Shown>, result: &Value, events| {
                let mut dual = dual;
                solidcraft_ui_registry::clear_handles();
                solidcraft_ui_kit::test_support::raw_frame(
                    &h.ctx,
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 720.0))),
                        events,
                        focused: true,
                        ..Default::default()
                    },
                    |ui| {
                        ui.set_width(520.0);
                        ui_kit::scope(ui, |ui| measure_rows(&h.app, ui, Some(result), shown, &mut dual, &mut false));
                    },
                )
            };
            for _ in 0..3 {
                draw(&mut shown, &result, vec![]);
            }
            assert_eq!(scenario::rect_of("dialog:column:measure_results:Value").is_some(), !dual);
            assert!(scenario::rect_of("dialog:report:measure_item_0_0").is_some());
            if dual {
                for label in ["ΔX", "ΔY", "ΔZ"] {
                    assert!(scenario::rect_of(&format!("dialog:report:measure_value_{label}")).is_some());
                }
            }
            let pos = scenario::handle_at("dialog:list:max").unwrap();
            for pressed in [true, false] {
                draw(&mut shown, &result, solidcraft_ui_kit::test_support::pointer(pos, pressed));
            }
            assert!(shown == Some(Shown::Max));
            let plain = json!({"items":[], "distance_mm":4,"from":[0,0,0],"to":[4,0,0]});
            shown = None;
            for _ in 0..3 {
                draw(&mut shown, &plain, vec![]);
            }
            let pos = scenario::handle_at("measure:copy:distance").unwrap();
            draw(&mut shown, &plain, solidcraft_ui_kit::test_support::pointer(pos, true));
            let output = draw(&mut shown, &plain, solidcraft_ui_kit::test_support::pointer(pos, false));
            assert!(
                output.platform_output.commands.iter().any(
                    |command| matches!(command, egui::OutputCommand::CopyText(text) if text == &crate::prefs::show_measure(&h.app, 4.0, 1, dual))
                )
            );
        }
        assert_eq!(h.app.session.undo.len(), undo, "report interactions do not edit the design");
    }

    /// #17606: Create Sketch says how to pick a plane the view shows edge-on, and only then.
    #[test]
    fn create_sketch_hints_at_planes_seen_edge_on_only_when_there_are_some() {
        use solidcraft_viewport::StandardView;
        let mut cam = solidcraft_viewport::Camera::default();
        assert!(edge_on_origin_planes(&cam).is_empty(), "the iso view sees every origin plane");
        for (view, planes) in
            [(StandardView::Top, vec!["XZ", "YZ"]), (StandardView::Front, vec!["XY", "YZ"]), (StandardView::Right, vec!["XY", "XZ"])]
        {
            cam.set_view(view);
            assert_eq!(edge_on_origin_planes(&cam), planes, "{view:?}");
        }
        cam.yaw += 0.3;
        cam.pitch = 0.4;
        assert!(edge_on_origin_planes(&cam).is_empty(), "a turned view sees every plane again");

        fn has_text(shape: &egui::Shape, text: &str) -> bool {
            match shape {
                egui::Shape::Text(t) => t.galley.text() == text,
                egui::Shape::Vec(shapes) => shapes.iter().any(|s| has_text(s, text)),
                _ => false,
            }
        }
        let mut h = Harness::new();
        let hint = solidcraft_ui_text::format("sketch_panel.edge_on_hint", &[("planes", "XZ, YZ")]);
        assert!(hint.starts_with("XZ, YZ are seen edge-on"), "{hint}");
        h.step(&json!({"start":"sketch.create"})).unwrap();
        h.frames(2);
        assert!(!h.shapes.iter().any(|s| has_text(&s.shape, &hint)), "no hint while every plane can be clicked");
        h.app.cam.set_view(StandardView::Top);
        h.frames(2);
        assert!(h.shapes.iter().any(|s| has_text(&s.shape, &hint)), "the panel says how to pick XZ and YZ: {hint}");
    }

    const COMMANDS: [&str; 6] =
        ["solid.extrude", "solid.fillet", "solid.chamfer", "solid.hole", "solid.pattern.rectangular", "solid.pattern.circular"];

    fn sample(h: &mut Harness) {
        h.app.run("sketch.create", json!({"plane":"XY","name":"Base"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[10,10]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("solid.extrude", json!({"distance":"5 mm"})).unwrap();
    }

    fn finger_joint_editor() -> (Harness, u64) {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length":100,"width":3,"height":50,"body_name":"A","operation":"new"})).unwrap();
        h.app.run("solid.box", json!({"corner":[48.5,-48.5,0],"length":3,"width":100,"height":50,"body_name":"B","operation":"new"})).unwrap();
        let result = h.app.run("solid.finger_joint", json!({"bodies":["A","B"],"count":3,"clearance":"0.1 mm"})).unwrap();
        let id = result["feature"].as_u64().unwrap();
        h.app.edit_feature(id);
        h.frames(2);
        (h, id)
    }

    #[test]
    fn finger_joint_creation_uses_preselection_in_order() {
        let mut h = Harness::new();
        let selected = vec![Sel::Body { name: "C".into() }, Sel::Body { name: "A".into() }, Sel::Body { name: "B".into() }];
        h.app.session.selection = selected.clone();
        h.app.start("solid.finger_joint");
        let dialog = h.app.dialog.as_ref().unwrap();
        assert_eq!(dialog.inputs[0].items, selected);
        assert!(dialog.inputs[0].multi);
        assert!(dialog.editing.is_none());
        assert!(matches!(&dialog.kind, Kind::FingerJoint { edge_offset, .. } if edge_offset == "0 mm"));
        let commands = apply_commands(&h.app, dialog).unwrap();
        assert_eq!(commands[0].0, "solid.finger_joint");
        assert_eq!(commands[0].1["bodies"], json!(["C", "A", "B"]));
        assert_eq!(commands[0].1["count"], 3);
        assert_eq!(commands[0].1["edge_offset"], "0 mm");
    }

    #[test]
    fn finger_joint_creation_accepts_picks_after_opening() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length":100,"width":3,"height":50,"body_name":"A","operation":"new"})).unwrap();
        h.app.run("solid.box", json!({"corner":[48.5,-48.5,0],"length":3,"width":100,"height":50,"body_name":"B","operation":"new"})).unwrap();
        h.app.session.selection.clear();
        h.app.start("solid.finger_joint");
        let mut dialog = h.app.dialog.take().unwrap();
        assert!(dialog.inputs[0].items.is_empty());
        assert!(apply_commands(&h.app, &dialog).is_err());
        for name in ["B", "A"] {
            dialog.pick(&h.app.session, Sel::Body { name: name.into() });
        }
        let commands = apply_commands(&h.app, &dialog).unwrap();
        assert_eq!(commands[0].1["bodies"], json!(["B", "A"]));
        run_dialog(&mut h.app, &dialog).unwrap();
        assert!(matches!(
            &h.app.session.doc.features.last().unwrap().kind,
            FeatureKind::FingerJoint { bodies, count: 3, .. } if bodies.iter().map(|name| name.0.as_str()).collect::<Vec<_>>() == ["B", "A"]
        ));
    }

    #[test]
    fn finger_joint_preview_tracks_added_and_removed_bodies_without_committing() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length":100,"width":3,"height":50,"body_name":"A","operation":"new"})).unwrap();
        for (name, x) in [("B", 20), ("C", 70)] {
            h.app.run("solid.box", json!({"corner":[x,-48.5,0],"length":3,"width":100,"height":50,"body_name":name,"operation":"new"})).unwrap();
        }
        let before = serde_json::to_value(&h.app.session.doc).unwrap();
        h.app.session.selection.clear();
        h.app.start("solid.finger_joint");
        let mut dialog = h.app.dialog.take().unwrap();
        assert!(dialog.previews() && dialog.preview_enabled);
        for name in ["A", "B"] {
            dialog.pick(&h.app.session, Sel::Body { name: name.into() });
        }
        let colors = crate::preview::Colors::from_tokens(&solidcraft_theme::Tokens::dark());
        let commands = crate::dialogs::preview_commands(&h.app, &dialog).unwrap();
        let first = crate::preview::compute(&h.app.session, &commands, colors).unwrap();
        assert!(first.replaced.contains(&"B".to_string()));
        dialog.pick(&h.app.session, Sel::Body { name: "C".into() });
        let commands = crate::dialogs::preview_commands(&h.app, &dialog).unwrap();
        assert_eq!(commands[0].1["bodies"], json!(["A", "B", "C"]));
        let added = crate::preview::compute(&h.app.session, &commands, colors).unwrap();
        assert!(added.replaced.contains(&"B".to_string()) && added.replaced.contains(&"C".to_string()));
        dialog.remove_listed(&h.app.session, 0, 1, &Sel::Body { name: "B".into() });
        let commands = crate::dialogs::preview_commands(&h.app, &dialog).unwrap();
        assert_eq!(commands[0].1["bodies"], json!(["A", "C"]));
        let removed = crate::preview::compute(&h.app.session, &commands, colors).unwrap();
        assert!(!removed.replaced.contains(&"B".to_string()) && removed.replaced.contains(&"C".to_string()));
        h.app.dialog = Some(dialog);
        cancel(&mut h.app);
        assert_eq!(serde_json::to_value(&h.app.session.doc).unwrap(), before);
    }

    #[test]
    fn finger_joint_editor_refills_applies_and_undoes() {
        let (mut h, id) = finger_joint_editor();
        let dialog = h.app.dialog.as_mut().unwrap();
        assert_eq!(dialog.inputs[0].items, vec![Sel::Body { name: "A".into() }, Sel::Body { name: "B".into() }]);
        match &mut dialog.kind {
            Kind::FingerJoint { count, clearance, inverted, edge_offset } => {
                assert_eq!((count.as_str(), clearance.as_str(), *inverted, edge_offset.as_str()), ("3", "0.1 mm", false, "0 mm"));
                *count = "2".into();
                *clearance = "0.2 mm".into();
                *inverted = true;
                *edge_offset = "1 mm".into();
            }
            other => panic!("unexpected editor: {other:?}"),
        }
        let dialog = h.app.dialog.as_ref().unwrap().clone();
        let commands = apply_commands(&h.app, &dialog).unwrap();
        assert_eq!(commands[0].0, "timeline.redefine");
        assert_eq!(commands[0].1["params"]["bodies"], json!(["A", "B"]));
        run_dialog(&mut h.app, &dialog).unwrap();
        match &h.app.session.doc.feature(id).unwrap().kind {
            FeatureKind::FingerJoint { count, clearance, inverted, edge_offset, .. } => {
                assert_eq!((*count, clearance.as_str(), *inverted), (2, "0.2 mm", true));
                assert_eq!(edge_offset.as_ref().map(|value| value.as_str()), Some("1 mm"));
            }
            other => panic!("unexpected feature: {other:?}"),
        }
        h.app.run("edit.undo", json!({})).unwrap();
        assert!(matches!(
            &h.app.session.doc.feature(id).unwrap().kind,
            FeatureKind::FingerJoint { count: 3, inverted: false, edge_offset: None, .. }
        ));
    }

    #[test]
    fn finger_joint_editor_empty_edge_offset_uses_its_zero_default() {
        let (mut h, id) = finger_joint_editor();
        if let Kind::FingerJoint { edge_offset, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
            edge_offset.clear();
        } else {
            panic!("expected finger joint editor");
        }
        let dialog = h.app.dialog.as_ref().unwrap().clone();
        let commands = apply_commands(&h.app, &dialog).unwrap();
        assert_eq!(commands[0].1["params"]["edge_offset"], serde_json::Value::Null);
        run_dialog(&mut h.app, &dialog).unwrap();
        assert!(matches!(&h.app.session.doc.feature(id).unwrap().kind, FeatureKind::FingerJoint { edge_offset: None, .. }));
    }

    #[test]
    fn finger_joint_editor_rejects_bad_inputs_and_cancel_preserves_feature() {
        let (mut h, id) = finger_joint_editor();
        let before = serde_json::to_value(h.app.session.doc.feature(id).unwrap()).unwrap();
        let dialog = h.app.dialog.as_mut().unwrap();
        if let Kind::FingerJoint { count, .. } = &mut dialog.kind {
            *count = "65".into();
        } else {
            panic!("expected finger joint editor");
        }
        assert!(apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).is_err());
        let dialog = h.app.dialog.as_mut().unwrap();
        if let Kind::FingerJoint { count, clearance, .. } = &mut dialog.kind {
            *count = "3".into();
            clearance.clear();
        }
        assert!(apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).is_err());
        let dialog = h.app.dialog.as_mut().unwrap();
        if let Kind::FingerJoint { clearance, .. } = &mut dialog.kind {
            *clearance = "0.1 mm".into();
        }
        dialog.inputs[0].items = vec![Sel::Body { name: "A".into() }, Sel::Body { name: "A".into() }];
        assert!(apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).is_err());
        cancel(&mut h.app);
        assert_eq!(serde_json::to_value(h.app.session.doc.feature(id).unwrap()).unwrap(), before);
        assert_eq!(h.app.session.doc.marker, None);
    }

    #[test]
    fn common_forms_fit_both_layouts_and_scales_and_pin_footer_at_small_heights() {
        for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
            for scale in [1.0, 2.0] {
                for height in [950.0, 720.0] {
                    let mut h = Harness::new();
                    h.call("ui.set", json!({"layout": mode}));
                    h.app.preferences.ui_scale = scale;
                    h.call("ui.resize", json!({"width": 1400.0 / scale, "height": height / scale}));
                    sample(&mut h);
                    for command in COMMANDS {
                        h.step(&json!({"start":command})).unwrap();
                        // Force the long Hole variant too: counterbore, tapping and multiple picks.
                        if let Some(Dialog { kind: Kind::Hole { kind, opts, .. }, .. }) = h.app.dialog.as_mut() {
                            *kind = 1;
                            opts.tapped = true;
                            opts.multiple = true;
                        }
                        h.frames(8);
                        let card = scenario::rect_of("dialog:card").unwrap();
                        let viewport = h.app.viewport.rect.unwrap();
                        assert!(
                            viewport.expand(1.0).contains_rect(card),
                            "{command} {mode:?} scale={scale} height={height}: {card:?} outside {viewport:?}"
                        );
                        for action in ["dialog:ok", "dialog:cancel"] {
                            let rect = scenario::rect_of(action).unwrap();
                            assert!(
                                card.contains_rect(rect),
                                "{command} {mode:?} scale={scale} height={height}: clipped {action} {rect:?} card={card:?}"
                            );
                        }
                        assert!(crate::shell::covered_rects(&h.app).contains(&card));
                        assert!(card.width() <= 301.0, "{card:?}");
                        h.step(&json!({"widget":"Cancel"})).unwrap();
                        assert!(h.app.dialog.is_none(), "{command}");
                    }
                }
            }
        }
    }

    #[test]
    fn tiny_windows_do_not_panic_or_publish_non_finite_cards() {
        for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
            for size in [(1.0, 1.0), (40.0, 80.0), (180.0, 240.0)] {
                let mut h = Harness::new();
                h.call("ui.set", json!({"layout": mode}));
                sample(&mut h);
                h.call("ui.resize", json!({"width":size.0,"height":size.1}));
                for command in COMMANDS {
                    h.app.start(command);
                    assert!(h.app.dialog.is_some(), "{command} must actually open");
                    h.frames(3);
                    if let Some(rect) = h.app.viewport.dialog_rect {
                        assert!(rect.is_finite() && rect.width() >= 0.0 && rect.height() >= 0.0, "{command}: {rect:?}");
                    }
                    cancel(&mut h.app);
                }
            }
        }
    }

    #[test]
    fn fillet_options_keep_the_existing_command_parameters() {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1400,"height":1000}));
        sample(&mut h);
        h.step(&json!({"start":"solid.fillet"})).unwrap();
        h.frames(4);
        choose(&mut h, "Type", "ui.fillet_type_curvature");
        assert_eq!(h.app.dialog.as_ref().unwrap().extra.get("type"), Some(&json!("curvature")));
        h.step(&json!({"click":{"handle":"dialog:Tangent Chain"}})).unwrap();
        assert!(matches!(h.app.dialog.as_ref().unwrap().kind, Kind::Fillet { chain: false, .. }));
        // Color edges is the remembered preference, not a command parameter.
        assert!(!h.app.preferences.hover_edge_lit);
        h.step(&json!({"click":{"handle":"dialog:ui.fillet_color_edges"}})).unwrap();
        assert!(h.app.preferences.hover_edge_lit);
        assert_eq!(h.app.dialog.as_ref().unwrap().extra.get("type"), Some(&json!("curvature")));
        h.step(&json!({"click":{"handle":"dialog:section:Options"}})).unwrap();
        h.frames(2);
        assert!(scenario::handle_at("dialog:Tangent Chain").is_none());
        h.step(&json!({"click":{"handle":"dialog:section:Options"}})).unwrap();
        h.frames(2);
        assert!(scenario::handle_at("dialog:Tangent Chain").is_some());
    }
    /// #15939: with Tangent Chain unticked, one picked edge is rounded alone, not with the edges
    /// that run on from it smoothly. A 40 x 30 x 20 box with one upright edge rounded: its top
    /// back edge runs on into the round's top edge, and that into the top right edge.
    #[test]
    fn a_fillet_with_tangent_chain_off_rounds_the_picked_edge_alone() {
        use solidcraft_engine::geom::MeshGeom;
        let volume = |h: &Harness| crate::open_meshing::session_world(&h.app.session).body("Body1").unwrap().mesh().measure().volume;
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1400,"height":1000}));
        h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
        h.app.run("solid.fillet", json!({"edges": [[40, 30, 10]], "radius": "5"})).unwrap();
        let before = volume(&h);
        h.step(&json!({"start":"solid.fillet"})).unwrap();
        h.frames(4);
        h.step(&json!({"click":{"handle":"dialog:Tangent Chain"}})).unwrap();
        assert!(matches!(h.app.dialog.as_ref().unwrap().kind, Kind::Fillet { chain: false, .. }));
        let edge = edge_sel(&h.app.session, Vec3::new(17.5, 30.0, 20.0)).unwrap();
        let session = &h.app.session;
        h.app.dialog.as_mut().unwrap().pick(session, edge);
        assert_eq!(h.app.dialog.as_ref().unwrap().items().len(), 1);
        let p = parameters(&h);
        assert_eq!(p["tangent_chain"], json!(false), "{p}");
        h.app.run("solid.fillet", p).unwrap();
        // A 2 mm round along the 35 mm straight edge takes (1 - pi/4) r^2 L = 30.0 mm^3; with
        // the chain, the round's top edge and the 25 mm right edge go too (59 mm^3).
        let removed = before - volume(&h);
        assert!((27.0..33.0).contains(&removed), "removed {removed} mm^3");
        // Editing it keeps the chain off; a fillet a script saved with the chain on keeps it on.
        let saved = h.app.session.doc.features.last().unwrap().id;
        h.app.edit_feature(saved);
        assert_eq!(redefined(&h)["tangent_chain"], json!(false));
        h.step(&json!({"widget":"Cancel"})).unwrap();
        h.app.run("solid.fillet", json!({"edges": [[0, 15, 0]], "radius": "1"})).unwrap();
        let scripted = h.app.session.doc.features.last().unwrap().id;
        h.app.edit_feature(scripted);
        assert_ne!(redefined(&h)["tangent_chain"], json!(false));
    }

    fn fillet_harness() -> Harness {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1400,"height":1000}));
        sample(&mut h);
        let edge = edge_sel(&h.app.session, Vec3::new(5.0, 0.0, 5.0)).unwrap();
        h.app.session.selection = vec![edge];
        h.step(&json!({"start":"solid.fillet"})).unwrap();
        h.frames(4);
        h
    }

    /// #10863: a round's joins are smooth and Fillet refuses them, so picking the top face of a
    /// plate whose outer top edges are rounded rounds the hole's rim alone.
    #[test]
    fn a_face_pick_leaves_out_the_edges_a_round_already_meets() {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1400,"height":1000}));
        sample(&mut h);
        h.app.run("solid.hole", json!({"position":[5,5,5],"diameter":2})).unwrap();
        h.app.run("solid.fillet", json!({"edges":[[5,0,5],[5,10,5],[0,5,5],[10,5,5]],"radius":1})).unwrap();
        let face = face_sel(&h.app.session, Vec3::new(2.0, 2.0, 5.0)).unwrap();
        h.app.session.selection = vec![face];
        h.step(&json!({"start":"solid.fillet"})).unwrap();
        h.frames(4);
        let p = parameters(&h);
        let edges = p["edges"].as_array().unwrap();
        assert!(!edges.is_empty(), "{p}");
        for e in edges {
            let (x, y) = (e[0].as_f64().unwrap(), e[1].as_f64().unwrap());
            assert!(((x - 5.0).hypot(y - 5.0) - 1.0).abs() < 1e-3, "an edge off the hole's rim: {e}");
        }
        // The edges the dialog's corners and bodies are read from leave them out too.
        let items = h.app.dialog.as_ref().unwrap().items();
        let ids = super::super::fillet_edge_ids(&h.app.session, &items);
        let world = crate::open_meshing::session_world(&h.app.session);
        assert_eq!(ids.len(), edges.len(), "{ids:?}");
        for (body, i) in &ids {
            let mid = polyline_mid(&world.body(body).unwrap().mesh().edges[*i]);
            assert!(((mid.x - 5.0).hypot(mid.y - 5.0) - 1.0).abs() < 1e-3, "an edge off the hole's rim: {mid:?}");
        }
        h.app.run("solid.fillet", p).unwrap();
    }

    /// A 1° crease is not a smooth join: Fillet rounds it, so a face pick keeps it.
    #[test]
    fn a_face_pick_keeps_a_one_degree_crease() {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1400,"height":1000}));
        let rise = 10.0 * 1f64.to_radians().tan();
        h.app.run("sketch.create", json!({"plane":"XY","name":"Crease"})).unwrap();
        let outline = [[0.0, 0.0], [20.0, 0.0], [20.0, 5.0 + rise], [10.0, 5.0], [0.0, 5.0]];
        for (i, a) in outline.iter().enumerate() {
            h.app.run("sketch.line", json!({"points":[a, outline[(i + 1) % outline.len()]]})).unwrap();
        }
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("solid.extrude", json!({"distance":"5 mm"})).unwrap();
        let face = face_sel(&h.app.session, Vec3::new(5.0, 5.0, 2.5)).unwrap();
        h.app.session.selection = vec![face];
        h.step(&json!({"start":"solid.fillet"})).unwrap();
        h.frames(4);
        let p = parameters(&h);
        let edges = p["edges"].as_array().unwrap();
        let crease = |e: &Value| (e[0].as_f64().unwrap() - 10.0).abs() < 1e-6 && (e[1].as_f64().unwrap() - 5.0).abs() < 1e-6;
        assert!(edges.iter().any(crease), "the 1° crease at x = 10 left out: {p}");
    }

    fn parameters(h: &Harness) -> Value {
        let commands = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        assert_eq!(commands.first().unwrap().0, "solid.fillet");
        commands.first().unwrap().1.clone()
    }

    /// The parameters an edit dialog's OK gives its feature again.
    fn redefined(h: &Harness) -> Value {
        let commands = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        let (cmd, p) = commands.first().unwrap();
        assert_eq!(cmd, "timeline.redefine", "{p}");
        assert_eq!(p["command"], json!("solid.fillet"), "{p}");
        p["params"].clone()
    }

    fn choose(h: &mut Harness, row: &str, option: &str) {
        h.step(&json!({"click":{"handle":format!("dialog:{row}")}})).unwrap();
        h.step(&json!({"click":{"handle":format!("dialog:{row}:{option}")}})).unwrap();
        h.frames(2);
    }

    #[test]
    fn fillet_and_chamfer_name_the_body_of_their_edges() {
        // Named, the feature looks for its edges on that body alone instead of meshing every
        // body of the design on each preview and apply.
        let h = fillet_harness();
        let body = h.app.session.world_state().unwrap().bodies.first().unwrap().name.clone();
        let p = parameters(&h);
        assert_eq!(p["body"], json!(body), "{p}");
        // Edges of two bodies name none: each point finds its own.
        let mut h = Harness::new();
        sample(&mut h);
        h.app.run("solid.box", json!({"length":4,"width":4,"height":4,"corner":[20,0,0],"operation":"new"})).unwrap();
        let a = edge_sel(&h.app.session, Vec3::new(5.0, 0.0, 5.0)).unwrap();
        let b = edge_sel(&h.app.session, Vec3::new(22.0, 0.0, 4.0)).unwrap();
        h.app.session.selection = vec![a, b];
        h.step(&json!({"start":"solid.chamfer"})).unwrap();
        h.frames(4);
        // Picks on two bodies run one command per body, each with its own body and edge (#12453).
        let bodies: Vec<String> = h.app.session.world_state().unwrap().bodies.iter().map(|b| b.name.clone()).collect();
        let commands = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        assert_eq!(commands.len(), 2, "{commands:?}");
        for ((cmd, p), body) in commands.iter().zip(&bodies) {
            assert_eq!(*cmd, "solid.chamfer");
            assert_eq!(p["body"], json!(body), "{p}");
            assert_eq!(p["edges"].as_array().unwrap().len(), 1, "{p}");
        }
    }

    #[test]
    fn fillet_picks_on_a_split_body_round_both_halves() {
        // (#12453) The two halves of a split body share their section edges, so a pick on each
        // half names no single body: each half's edge goes in its own command, and both halves round.
        let mut h = Harness::new();
        sample(&mut h);
        h.app.run("solid.box", json!({"length":20,"width":10,"height":10,"corner":[0,0,0],"operation":"new","body_name":"Block"})).unwrap();
        h.app.run("solid.split_body", json!({"body":"Block","plane":{"origin":[10,0,0],"normal":[1,0,0]}})).unwrap();
        let halves: Vec<String> = h.app.session.world_state().unwrap().bodies.iter().skip(1).map(|b| b.name.clone()).collect();
        assert_eq!(halves.len(), 2, "{halves:?}");
        let left = edge_sel(&h.app.session, Vec3::new(5.0, 0.0, 10.0)).unwrap();
        let right = edge_sel(&h.app.session, Vec3::new(15.0, 0.0, 10.0)).unwrap();
        // The section edge both halves share: on main the engine refuses its point ("lies on edges of 2 bodies").
        let section = edge_sel(&h.app.session, Vec3::new(10.0, 5.0, 10.0)).unwrap();
        let section_body = match &section {
            Sel::Edge { body, .. } => body.clone(),
            other => panic!("{other:?}"),
        };
        h.app.session.selection = vec![left, right, section];
        h.step(&json!({"start":"solid.fillet"})).unwrap();
        h.frames(4);
        let before: Vec<f64> = {
            let st = h.app.session.world_state().unwrap();
            halves.iter().map(|name| st.body(name).unwrap().measure().unwrap().volume).collect()
        };
        let commands = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        assert_eq!(commands.len(), 2, "{commands:?}");
        // The section edge goes in the command of the body edge_sel gave it, and no other.
        let on_section = |p: &Value| p["edges"].as_array().unwrap().iter().any(|e| e[0] == json!(10.0) && e[2] == json!(10.0));
        for (_, p) in &commands {
            assert_eq!(on_section(p), p["body"] == json!(section_body), "{p}");
        }
        for (cmd, mut p) in commands {
            assert_eq!(cmd, "solid.fillet");
            p["radius"] = json!(1);
            h.app.run(&cmd, p).unwrap();
        }
        let st = h.app.session.world_state().unwrap();
        for (name, volume) in halves.iter().zip(before) {
            let after = st.body(name).unwrap().measure().unwrap().volume;
            assert!(after < volume, "{name} should lose volume from its rounded edge: {volume} -> {after}");
        }
    }

    #[test]
    fn fillet_picks_on_two_bodies_cannot_edit_a_feature() {
        // Editing keeps only one command (apply_commands), so picks on two bodies are refused, not dropped (#12453).
        let mut h = Harness::new();
        sample(&mut h);
        h.app.run("solid.box", json!({"length":20,"width":10,"height":10,"corner":[0,0,0],"operation":"new","body_name":"Block"})).unwrap();
        h.app.run("solid.split_body", json!({"body":"Block","plane":{"origin":[10,0,0],"normal":[1,0,0]}})).unwrap();
        let left = edge_sel(&h.app.session, Vec3::new(5.0, 0.0, 10.0)).unwrap();
        let right = edge_sel(&h.app.session, Vec3::new(15.0, 0.0, 10.0)).unwrap();
        h.app.session.selection = vec![left, right];
        h.step(&json!({"start":"solid.fillet"})).unwrap();
        h.frames(4);
        h.app.dialog.as_mut().unwrap().editing = Some((1, None));
        let err = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap_err();
        assert!(err.contains("one body"), "{err}");
    }

    #[test]
    fn press_pull_on_a_bore_inside_another_part_moves_that_bore() {
        // A pin standing inside a hollow housing that comes first: by its point alone the face
        // was looked for on the housing, whose box holds it, and the command failed with
        // "no face at …". The dialog now names the pin.
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length":100,"width":100,"height":100,"body_name":"Housing"})).unwrap();
        h.app.run("solid.box", json!({"length":80,"width":80,"height":100,"corner":[10,10,10],"operation":"cut","targets":["Housing"]})).unwrap();
        h.app.run("solid.cylinder", json!({"base":[50,50,20],"radius":5,"height":20,"operation":"new","body_name":"Pin"})).unwrap();
        let face = face_sel(&h.app.session, Vec3::new(55.0, 50.0, 30.0)).unwrap();
        assert!(matches!(&face, Sel::Face { body, .. } if body == "Pin"), "{face:?}");
        h.app.session.selection = vec![face];
        h.step(&json!({"start":"solid.press_pull"})).unwrap();
        h.frames(4);
        let commands = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        let (cmd, mut p) = commands.first().cloned().unwrap();
        assert_eq!(cmd, "solid.offset_face");
        assert_eq!(p["body"], json!("Pin"), "{p}");
        p["distance"] = json!(1);
        h.app.run(&cmd, p).unwrap();
        let st = h.app.session.world_state().unwrap();
        let pin = st.body("Pin").unwrap().measure().unwrap();
        let expected = std::f64::consts::PI * 36.0 * 20.0;
        assert!((pin.volume - expected).abs() < expected * 1e-3, "pin volume {} (expected {expected})", pin.volume);
        let housing = st.body("Housing").unwrap().measure().unwrap();
        assert!((housing.volume - (1e6 - 80.0 * 80.0 * 90.0)).abs() < 1.0, "the housing is unchanged: {}", housing.volume);
    }

    #[test]
    fn every_type_choice_produces_its_supported_command_without_changing_defaults() {
        for (label, key) in FILLET_TYPES {
            let mut h = fillet_harness();
            let original = parameters(&h);
            h.frames(3);
            assert_eq!(parameters(&h), original, "merely drawing the default form is lossless");
            choose(&mut h, "Type", label);
            let p = parameters(&h);
            assert_eq!(p.get("type").and_then(Value::as_str).unwrap_or("constant"), key, "{p}");
            assert_eq!(p["radius"], original["radius"]);
            assert_eq!(p["edges"], original["edges"]);
            assert!(p.get("preview").is_none() && p.get("preview_enabled").is_none());
            if key == "variable" {
                assert_eq!(p["radius2"], json!("1 mm"));
                assert!(p["start"].is_array());
            }
            if key == "spline" {
                assert_eq!(p["radii"].as_array().unwrap().len(), 4);
                assert_eq!(p["knots"].as_array().unwrap().len(), 9);
                assert!(p["start"].is_array());
            }
            // Type is the one place the section is chosen: no second Continuity control.
            assert!(scenario::handle_at("dialog:Continuity").is_none());
        }
    }

    #[test]
    fn if_too_large_maps_to_fit() {
        let mut h = fillet_harness();
        assert!(scenario::handle_at("dialog:section:Advanced").is_none(), "no Advanced section: its one row sits with Type");
        choose(&mut h, "ui.fillet_fit", "ui.fillet_fit_reduce");
        assert_eq!(parameters(&h)["fit"], json!("reduce"));
        choose(&mut h, "ui.fillet_fit", "ui.fillet_fit_taper");
        assert_eq!(parameters(&h)["fit"], json!("taper"));
        choose(&mut h, "ui.fillet_fit", "ui.fillet_fit_exact");
        assert!(parameters(&h).get("fit").is_none());
        choose(&mut h, "Type", "Chord length");
        assert!(scenario::handle_at("dialog:ui.fillet_fit").is_none(), "a fitted radius is for Constant radius only");
    }

    /// Turns sends `turn: swept` with a Constant radius only, and a choice it rules out (a
    /// narrowed radius, a curvature corner) puts the other back: Swept and those never go
    /// together.
    #[test]
    fn turns_maps_to_turn_and_rules_out_what_a_swept_corner_does_not_take() {
        let mut h = fillet_harness();
        set_edges(&mut h, 3);
        assert!(scenario::handle_at("dialog:ui.fillet_turn").is_some());
        assert!(parameters(&h).get("turn").is_none(), "Auto is the default, not sent");
        choose(&mut h, "ui.fillet_turn", "ui.fillet_turn_swept");
        assert_eq!(parameters(&h)["turn"], json!("swept"));
        choose(&mut h, "ui.fillet_fit", "ui.fillet_fit_reduce");
        assert_eq!((parameters(&h)["turn"].clone(), parameters(&h)["fit"].clone()), (json!("swept"), json!("reduce")));
        choose(&mut h, "ui.fillet_fit", "ui.fillet_fit_taper");
        assert!(parameters(&h).get("turn").is_none(), "a narrowed radius puts Turns back to the default");
        choose(&mut h, "ui.fillet_turn", "ui.fillet_turn_swept");
        assert!(parameters(&h).get("fit").is_none(), "Swept puts Face limits back to Exact radius");
        choose(&mut h, "ui.corners", Continuity::G2.label_id());
        assert!(parameters(&h).get("turn").is_none(), "a curvature corner puts Turns back to the default");
        choose(&mut h, "ui.fillet_turn", "ui.fillet_turn_swept");
        assert!(parameters(&h).get("corner").is_none(), "Swept puts Corners back to Tangent");
        choose(&mut h, "ui.fillet_turn", "ui.fillet_turn_mitre");
        assert_eq!(parameters(&h)["turn"], json!("mitre"), "Mitre is sent: it is not the default");
        choose(&mut h, "ui.fillet_turn", "ui.fillet_turn_auto");
        assert!(parameters(&h).get("turn").is_none(), "the default is not sent");
        choose(&mut h, "ui.fillet_turn", "ui.fillet_turn_swept");
        choose(&mut h, "Type", "Chord length");
        assert!(parameters(&h).get("turn").is_none() && scenario::handle_at("dialog:ui.fillet_turn").is_none(), "Turns is for Constant radius only");
    }

    /// A new fillet's Turns row starts at Auto, and an existing fillet keeps its own saved
    /// turn, so editing it never rewrites it.
    #[test]
    fn a_new_fillet_starts_at_auto_and_an_existing_one_keeps_its_turn() {
        // A new fillet's Turns row is Auto, which the command does not take (it is the
        // default): nothing is sent.
        let mut h = fillet_harness();
        set_edges(&mut h, 3);
        assert!(h.widget("Auto").is_some(), "a new fillet's Turns row starts at Auto");
        assert!(parameters(&h).get("turn").is_none(), "Auto is the default, not sent");
        // An existing fillet keeps its own saved turn: a mitred one stays mitred, and one saved
        // at Auto stays Auto.
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1400,"height":1000}));
        h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
        for (edge, turn, sent) in [([20, 0, 20], Some("mitre"), Some("mitre")), ([20, 30, 20], None, None)] {
            let mut p = json!({"edges": [edge], "radius": "1", "tangent_chain": false});
            if let Some(t) = turn {
                p["turn"] = json!(t);
            }
            h.app.run("solid.fillet", p).unwrap();
            let saved = h.app.session.doc.features.last().unwrap().id;
            h.app.edit_feature(saved);
            h.frames(4);
            assert_eq!(redefined(&h).get("turn").and_then(Value::as_str), sent, "{turn:?}");
            h.step(&json!({"widget":"Cancel"})).unwrap();
            h.frames(2);
        }
    }

    /// Editing a fillet keeps its turn. A mitred fillet, as every fillet saved before the
    /// rolling-ball default is, gives `turn: mitre` again, so it stays mitred rather than taking
    /// the new default; a fillet made with the default gives none (the default again); a swept
    /// one gives `turn: swept`.
    #[test]
    fn editing_a_fillet_keeps_its_turn() {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1400,"height":1000}));
        h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
        // a top edge of the box each: a front, the back and a side
        for (edge, turn, sent) in
            [([20, 0, 20], Some("mitre"), Some("mitre")), ([20, 30, 20], None, None), ([0, 15, 20], Some("swept"), Some("swept"))]
        {
            let mut p = json!({"edges": [edge], "radius": "1", "tangent_chain": false});
            if let Some(t) = turn {
                p["turn"] = json!(t);
            }
            h.app.run("solid.fillet", p).unwrap();
            let saved = h.app.session.doc.features.last().unwrap().id;
            h.app.edit_feature(saved);
            h.frames(4);
            assert_eq!(redefined(&h).get("turn").and_then(Value::as_str), sent, "{turn:?}");
            h.step(&json!({"widget":"Cancel"})).unwrap();
            h.frames(2);
        }
    }

    /// The dialog follows the option model (`fillet_option_applies`): through every type, fit and
    /// corner choice, in either order, each option it shows is one the others allow, and the
    /// command it builds is never refused for its options (`solid.fillet` runs it on a copy of
    /// the design). A choice that rules another out resets that one, visibly.
    #[test]
    fn no_reachable_fillet_option_state_is_refused_for_its_options() {
        let refused_for_options = |e: &str| {
            e.contains("take a constant radius")
                || e.contains("is built for")
                || e.contains("are built for")
                || e.contains(solidcraft_engine::doc::SWEPT_TAKES)
        };
        // Each distinct command is run once.
        let ran = std::cell::RefCell::new(std::collections::BTreeSet::new());
        let check = |h: &Harness, what: &str| {
            let p = parameters(h);
            let constant = p.get("type").is_none_or(|t| t == "constant");
            let exact = p.get("fit").is_none_or(|f| f == "exact");
            assert!(constant || p.get("fit").is_none(), "{what}: fit without Constant radius: {p}");
            assert!(constant && exact || (p.get("corner").is_none() && p.get("setbacks").is_none()), "{what}: a curvature corner with {p}");
            assert_eq!(scenario::handle_at("dialog:ui.fillet_fit").is_some(), constant, "{what}: Face limits shows exactly with Constant radius");
            assert_eq!(scenario::handle_at("dialog:ui.corners").is_some(), constant, "{what}: Corners shows exactly with Constant radius");
            if !ran.borrow_mut().insert(p.to_string()) {
                return;
            }
            let mut copy = solidcraft_engine::Session::new((*h.app.session.doc).clone());
            if let Err(e) = copy.execute("solid.fillet", &p) {
                assert!(!refused_for_options(&e.to_string()), "{what}: {p} refused for its options: {e}");
            }
        };
        for (label, _) in FILLET_TYPES {
            for (fit, _) in FILLET_FITS {
                for corner_first in [false, true] {
                    let mut h = fillet_harness();
                    set_edges(&mut h, 3);
                    let what = format!("{label}, {fit}, corner first: {corner_first}");
                    if corner_first {
                        choose(&mut h, "ui.corners", Continuity::G2.label_id());
                        check(&h, &what);
                        choose(&mut h, "ui.fillet_fit", fit);
                        if fit != "ui.fillet_fit_exact" {
                            assert!(parameters(&h).get("corner").is_none(), "{what}: a fitted radius puts Corners back to Tangent");
                        }
                    } else {
                        choose(&mut h, "ui.fillet_fit", fit);
                        check(&h, &what);
                        choose(&mut h, "ui.corners", Continuity::G2.label_id());
                        assert!(parameters(&h).get("fit").is_none(), "{what}: a curvature corner puts Face limits back to Exact radius");
                    }
                    check(&h, &what);
                    choose(&mut h, "Type", label);
                    check(&h, &what);
                }
            }
        }
    }

    /// The three edges of the sample's corner at (10, 0, 5): top front, top right and upright.
    fn corner_edges(h: &Harness, count: usize) -> Vec<Sel> {
        [Vec3::new(5.0, 0.0, 5.0), Vec3::new(10.0, 5.0, 5.0), Vec3::new(10.0, 0.0, 2.5)]
            .into_iter()
            .take(count)
            .map(|p| edge_sel(&h.app.session, p).unwrap())
            .collect()
    }

    fn set_edges(h: &mut Harness, count: usize) {
        let edges = corner_edges(h, count);
        h.app.dialog.as_mut().unwrap().inputs.first_mut().unwrap().items = edges;
        h.frames(3);
    }

    /// Corners shows only once three rounded edges meet at a corner; Curvature (a smooth patch) writes
    /// `corner: g2`, its Setback (untouched: the command's 1.5 × radius) goes to that corner as
    /// `setbacks`, and Tangent (a rolling ball) writes neither.
    #[test]
    fn corners_row_follows_the_selection_and_maps_to_corner_and_setbacks() {
        let mut h = fillet_harness();
        for count in [1, 2] {
            set_edges(&mut h, count);
            assert!(scenario::handle_at("dialog:ui.corners").is_none(), "{count} edges: no corner where three rounded edges meet");
        }
        set_edges(&mut h, 3);
        let corners = fillet_corners(&h.app.session, &h.app.dialog.as_ref().unwrap().inputs[0].items);
        assert_eq!(corners.len(), 1, "{corners:?}");
        assert!(corners[0].dist(Vec3::new(10.0, 0.0, 5.0)) < 1e-6, "{corners:?}");
        assert!(scenario::handle_at("dialog:ui.corners").is_some(), "three rounded edges meet at (10, 0, 5)");
        assert!(scenario::handle_at("dialog:ui.fillet_setback").is_none(), "Setback goes with a smooth patch");
        assert!(parameters(&h).get("corner").is_none(), "Ball is the default and writes nothing");
        choose(&mut h, "ui.corners", Continuity::G2.label_id());
        let p = parameters(&h);
        assert_eq!(p["corner"], json!("g2"));
        assert!(p.get("setbacks").is_none(), "an empty Setback leaves the command's default: {p}");
        assert!(scenario::handle_at("dialog:ui.fillet_setback").is_some());
        if let Some(Dialog { kind: Kind::Fillet { setback, .. }, .. }) = h.app.dialog.as_mut() {
            *setback = Some("3 mm".into());
        }
        assert_eq!(parameters(&h)["setbacks"], json!([{"vertex": [10.0, 0.0, 5.0], "distance": "3 mm"}]));
        choose(&mut h, "ui.corners", Continuity::G1.label_id());
        let p = parameters(&h);
        assert!(p.get("corner").is_none() && p.get("setbacks").is_none(), "{p}");
        assert!(scenario::handle_at("dialog:ui.fillet_setback").is_none());
        // Another type clears the patch and hides the row: its corners are the rolling ball's.
        choose(&mut h, "ui.corners", Continuity::G2.label_id());
        choose(&mut h, "Type", "Chord length");
        let p = parameters(&h);
        assert!(p.get("corner").is_none() && p.get("setbacks").is_none(), "{p}");
        assert!(scenario::handle_at("dialog:ui.corners").is_none());
    }

    /// Setback shows the command's default as an editable length that follows the radius until
    /// one is typed (and sends none), keeps a typed one through radius changes, and flags one at
    /// or below the least setback the kernel takes there (the radius, at this square corner)
    /// before OK; a setback past it builds.
    #[test]
    fn setback_shows_its_default_keeps_a_typed_value_and_flags_one_too_small() {
        let mut h = fillet_harness();
        set_edges(&mut h, 3);
        choose(&mut h, "ui.corners", Continuity::G2.label_id());
        let radius = |h: &mut Harness, r: &str| {
            if let Some(Dialog { kind: Kind::Fillet { radius, .. }, .. }) = h.app.dialog.as_mut() {
                *radius = r.into();
            }
            h.frames(3);
        };
        let shown = |h: &Harness| {
            let tree = h.output.accesskit_update.as_ref().unwrap();
            let label = solidcraft_ui_text::tr_label("ui.fillet_setback");
            tree.nodes.iter().find(|(_, n)| n.label() == Some(label)).and_then(|(_, n)| n.value()).unwrap_or_default().to_string()
        };
        let typed = |h: &Harness| match &h.app.dialog.as_ref().unwrap().kind {
            Kind::Fillet { setback, .. } => setback.clone(),
            other => panic!("{other:?}"),
        };
        let flagged = || scenario::rect_of("dialog:caption:ui.fillet_setback").is_some();
        let type_setback = |h: &mut Harness, text: &str| {
            h.step(&json!({"click":{"handle":"dialog:ui.fillet_setback"}})).unwrap();
            h.step(&json!({"key":"A","cmd":true})).unwrap();
            h.step(&json!({"text": text})).unwrap();
            h.frames(3);
        };
        radius(&mut h, "2 mm");
        assert_eq!((shown(&h), typed(&h)), ("3 mm".to_string(), None), "the default, 1.5 × the radius, as a length");
        assert!(!flagged() && parameters(&h).get("setbacks").is_none(), "an untouched default sends none");
        radius(&mut h, "3 mm");
        assert_eq!(shown(&h), "4.5 mm", "untouched, it follows the radius");
        radius(&mut h, "2 mm");
        type_setback(&mut h, "1.5 mm");
        assert_eq!(typed(&h).as_deref(), Some("1.5 mm"));
        radius(&mut h, "2.5 mm");
        assert_eq!(shown(&h), "1.5 mm", "typed, it stays as typed");
        radius(&mut h, "2 mm");
        assert!(flagged(), "1.5 mm is not past the least, the 2 mm radius");
        let mut copy = solidcraft_engine::Session::new((*h.app.session.doc).clone());
        let refused = copy.execute("solid.fillet", &parameters(&h)).unwrap_err().to_string();
        assert!(refused.contains("setback must be more than the radius (2 mm)"), "the kernel refuses what the field flags: {refused}");
        assert_eq!(
            crate::dialogs::fillet_plain_error(&refused, true),
            Some(crate::dialogs::FilletRefusal::InField),
            "the field says it, not again under the dialog"
        );
        let least = crate::dialogs::fillet_least_setback(
            &h.ctx,
            &h.app.session,
            &h.app.dialog.as_ref().unwrap().inputs[0].items,
            &[Vec3::new(10.0, 0.0, 5.0)],
            2.0,
        );
        assert!(least.is_some_and(|l| (l - 2.0).abs() < 1e-9), "{least:?}");
        assert_eq!(crate::length_input::shown(&h.app.session.doc, least.unwrap()), "2 mm", "the message's least");
        type_setback(&mut h, "2 mm");
        assert!(flagged(), "the least itself is refused too");
        type_setback(&mut h, "3 mm");
        assert!(!flagged(), "past the least");
        let p = parameters(&h);
        assert_eq!(p["setbacks"], json!([{"vertex": [10.0, 0.0, 5.0], "distance": "3 mm"}]));
        let mut copy = solidcraft_engine::Session::new((*h.app.session.doc).clone());
        copy.execute("solid.fillet", &p).unwrap();
        let st = crate::open_meshing::session_world(&copy);
        assert!(st.bodies.iter().all(|b| b.body.validity().is_empty()), "a 3 mm setback on 2 mm rounds builds");
    }

    /// No visible label of the fillet dialog names a continuity by its code (G0 … G3): the words
    /// are on the controls, the codes only in hover texts. Every section open, the corner row shown.
    #[test]
    fn fillet_labels_use_words_not_continuity_codes() {
        crate::i18n::install();
        let coded = |text: &str| ["G0", "G1", "G2", "G3"].iter().any(|code| text.contains(code));
        for label in FILLET_TYPES.map(|(label, _)| label).into_iter().chain(FILLET_CORNERS.map(|(g, _)| g.label_id())) {
            assert!(!coded(solidcraft_ui_text::tr_label(label)), "{label}: {}", solidcraft_ui_text::tr_label(label));
        }
        for (g, tip) in FILLET_CORNERS {
            assert!(solidcraft_ui_text::tr(tip).contains(g.code()), "the hover text keeps the code for those who know it: {tip}");
        }
        let tip = fillet_corner_tip();
        assert!(tip.contains("rolling") && tip.contains("1.5") && tip.contains("Setback"), "what each corner builds: {tip}");
        let mut h = fillet_harness();
        set_edges(&mut h, 3);
        choose(&mut h, "ui.corners", Continuity::G2.label_id());
        h.frames(3);
        assert!(scenario::handle_at("dialog:ui.fillet_setback").is_some() && scenario::handle_at("dialog:ui.fillet_fit").is_some());
        let tree = h.output.accesskit_update.as_ref().unwrap();
        let labels: Vec<String> = tree.nodes.iter().flat_map(|(_, node)| [node.label(), node.value()]).flatten().map(str::to_string).collect();
        assert!(labels.iter().any(|l| l == "Curvature"), "{labels:?}");
        let shown: Vec<&String> = labels.iter().filter(|l| coded(l)).collect();
        assert!(shown.is_empty(), "labels with a continuity code: {shown:?}");
    }

    /// A smooth-patch fillet reopens with its corner and setback, and OK writes them as saved; a
    /// setback list the dialog cannot show as one distance is kept as it is.
    /// A setback too short for SMLib's corner fill (past the least the field flags) is refused by
    /// the kernel naming the smallest that builds; the dialog says it in plain words, that length
    /// in them.
    #[test]
    fn a_setback_too_short_for_the_corner_says_the_least_that_builds() {
        crate::i18n::install();
        let refused = format!(
            "invalid parameters for `solid.fillet`: the setback is too short for its corner: {}5.9620 mm (the corner's patch is convex only from there)",
            solidcraft_engine::kernel::SETBACK_AT_LEAST
        );
        match crate::dialogs::fillet_plain_error(&refused, true) {
            Some(crate::dialogs::FilletRefusal::Plain(plain)) => assert!(plain.contains("5.9620 mm"), "{plain}"),
            other => panic!("{other:?}"),
        }
        // A blend too large for its faces keeps its own words.
        assert_eq!(
            crate::dialogs::fillet_plain_error("the fillet is larger than the faces beside its edges allow", true),
            Some(crate::dialogs::FilletRefusal::Plain(solidcraft_ui_text::tr("ui.fillet_error_too_large").to_string()))
        );
        // One that no smaller radius builds either is not told to try a smaller radius: the kernel
        // tried them down to its least.
        let none = "not supported yet: the fillet is larger than the faces beside its edges allow; no smaller radius builds either \
                    (down to 0.001 mm: the operation failed: not supported yet: the fillet is larger than the faces beside its edges allow)";
        assert_eq!(
            crate::dialogs::fillet_plain_error(none, false),
            Some(crate::dialogs::FilletRefusal::Plain(solidcraft_ui_text::tr("ui.fillet_error_no_radius").to_string()))
        );
    }

    /// #10849: an oversized fillet refusal carrying SMLib's largest fitting size names that size
    /// in the Fillet dialog and offers a button to use it; clicking the button updates the radius
    /// field, clears the refusal, and applying the resulting command builds cleanly.
    #[test]
    fn a_too_large_fillet_names_smlibs_fit_size_and_asks_whether_to_use_it() {
        crate::i18n::install();
        assert_eq!(
            crate::dialogs::fillet_plain_error(
                "not supported yet: the fillet is larger than the faces beside its edges allow; the largest radius that fits is 3 mm",
                false
            ),
            Some(crate::dialogs::FilletRefusal::FitSize { size_mm: 3.0 })
        );
        assert_eq!(
            crate::dialogs::fillet_plain_error(
                "not supported yet: the chord fillet is larger than the faces beside its edges allow; the largest chord width that fits is 1.5 mm",
                false
            ),
            Some(crate::dialogs::FilletRefusal::FitSize { size_mm: 1.5 })
        );
        assert_eq!(
            crate::dialogs::fillet_plain_error(
                "the round of radius 7 mm on the edge at (0.000, 32.000, 42.000) would cut through the material behind it into the hollow at (0.000, 30.000, 40.000) and open the part; the largest radius that fits is 6.828427 mm",
                false
            ),
            Some(crate::dialogs::FilletRefusal::FitSize { size_mm: 6.828427 })
        );

        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length": 14, "width": 6, "height": 10, "corner": [14, 0, 0], "body_name": "Bar"})).unwrap();
        let picks = [
            edge_sel(&h.app.session, Vec3::new(14.0, 0.0, 5.0)).unwrap(),
            edge_sel(&h.app.session, Vec3::new(28.0, 0.0, 5.0)).unwrap(),
            edge_sel(&h.app.session, Vec3::new(14.0, 6.0, 5.0)).unwrap(),
            edge_sel(&h.app.session, Vec3::new(28.0, 6.0, 5.0)).unwrap(),
        ];
        h.app.session.selection = picks.to_vec();
        h.step(&json!({"start": "solid.fillet"})).unwrap();
        h.frames(3);
        if let Some(Dialog { kind: Kind::Fillet { radius, .. }, .. }) = h.app.dialog.as_mut() {
            *radius = "3.5 mm".into();
        }
        let mut copy = solidcraft_engine::Session::new((*h.app.session.doc).clone());
        let refused = copy.execute("solid.fillet", &parameters(&h)).unwrap_err().to_string();
        assert!(refused.contains("the largest radius that fits is 3 mm"), "{refused}");
        let mut dialog = h.app.dialog.take().unwrap();
        dialog.refuse(&h.app, refused);
        h.app.dialog = Some(dialog);
        h.frames(3);

        assert!(scenario::handle_at("dialog:use_fit_size").is_some(), "Fillet dialog should offer a button to use SMLib's fit size");
        h.step(&json!({"click": {"handle": "dialog:use_fit_size"}})).unwrap();
        h.frames(3);

        let d = h.app.dialog.as_ref().unwrap();
        assert!(matches!(&d.kind, Kind::Fillet { radius, .. } if radius == "3 mm"), "{:?}", d.kind);
        assert!(d.error.is_none(), "clicking Use 3 mm should clear the refusal: {:?}", d.error);
        for (command, params) in apply_commands(&h.app, d).unwrap() {
            h.app.run(&command, params).unwrap();
        }
        let st = crate::open_meshing::session_world(&h.app.session);
        assert!(st.bodies.iter().all(|b| b.body.validity().is_empty()), "applying the offered fit size builds a valid body");
    }

    #[test]
    fn a_saved_smooth_patch_reopens_with_its_setback() {
        for (setbacks, field) in [
            (json!([{"vertex": [0, 0, 0], "distance": 7}]), Some("7")),
            // Named near the vertex, as the command allows, rather than at it.
            (json!([{"vertex": [0.1, 0.1, 0], "distance": 7}]), None),
        ] {
            let mut h = Harness::new();
            // The command's own smooth-patch corner: a 20 mm box, its three edges at the origin.
            h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20, "body_name": "B"})).unwrap();
            let edges = json!([[10, 0, 0], [0, 10, 0], [0, 0, 10]]);
            h.app.run("solid.fillet", json!({"edges": edges, "radius": 4, "corner": "g2", "setbacks": setbacks})).unwrap();
            let id = h.app.session.doc.features.last().unwrap().id;
            let corner_of = |h: &Harness| match &h.app.session.doc.feature(id).unwrap().kind {
                FeatureKind::Fillet { corner, setbacks, .. } => (*corner, setbacks.clone()),
                other => panic!("{other:?}"),
            };
            let saved = corner_of(&h);
            assert!(!saved.0.is_rolling() && saved.1.len() == 1, "{saved:?}");
            h.app.edit_feature(id);
            h.frames(3);
            let d = h.app.dialog.as_ref().unwrap();
            assert!(matches!(&d.kind, Kind::Fillet { setback, .. } if setback.as_deref() == field), "{:?}", d.kind);
            assert_eq!(d.extra.get("corner"), Some(&json!("g2")));
            for (command, params) in apply_commands(&h.app, d).unwrap() {
                crate::dialogs::roll_marker(&mut h.app, None).unwrap();
                h.app.run(&command, params).unwrap();
            }
            assert_eq!(corner_of(&h), saved, "OK keeps the corner and its setbacks");
        }
    }

    #[test]
    fn variable_controls_toggle_keep_mid_radii_and_preview_is_ui_only() {
        let mut h = fillet_harness();
        let original = parameters(&h);
        h.step(&json!({"click":{"handle":"dialog:Preview"}})).unwrap();
        h.frames(3);
        assert!(!h.app.dialog.as_ref().unwrap().preview_enabled);
        assert!(!h.app.preview.active && !h.app.preview.busy);
        assert_eq!(parameters(&h), original);
        h.step(&json!({"click":{"handle":"dialog:Preview"}})).unwrap();
        assert!(h.app.dialog.as_ref().unwrap().preview_enabled);
        assert_eq!(parameters(&h), original);
        assert!(scenario::handle_at("dialog:End Radius").is_none());
        choose(&mut h, "Type", "Variable radius");
        assert!(scenario::handle_at("dialog:End Radius").is_some(), "the section shows, open, once its type is chosen");
        assert!(
            scenario::handle_at("dialog:ui.fillet_smooth_radius").is_some(),
            "Smooth radius change is a supported variable-radius option even before adding a middle radius"
        );
        h.step(&json!({"widget":"Add Mid Radius"})).unwrap();
        h.step(&json!({"click":{"handle":"dialog:ui.fillet_smooth_radius"}})).unwrap();
        let saved = parameters(&h);
        assert_eq!(saved["mid"].as_array().unwrap().len(), 1);
        assert_eq!(saved["smooth"], json!(true));
        h.step(&json!({"click":{"handle":"dialog:section:Variable Radius"}})).unwrap();
        h.frames(2);
        assert!(scenario::handle_at("dialog:Mid Radius").is_none());
        assert_eq!(parameters(&h), saved, "folding keeps all values and commands");
        assert!(scenario::handle_at("dialog:section:Setbacks").is_none());
        assert!(scenario::handle_at("dialog:Result").is_none());
    }

    #[test]
    fn edges_field_paints_zero_one_and_many_and_footer_follows_validity() {
        fn contains_text(shape: &egui::Shape, expected: &str) -> bool {
            match shape {
                egui::Shape::Text(text) => text.galley.job.text == expected,
                egui::Shape::Vec(shapes) => shapes.iter().any(|shape| contains_text(shape, expected)),
                _ => false,
            }
        }
        let mut h = fillet_harness();
        let picks: Vec<_> = [Vec3::new(5.0, 0.0, 5.0), Vec3::new(0.0, 5.0, 5.0), Vec3::new(10.0, 5.0, 5.0)]
            .into_iter()
            .map(|p| edge_sel(&h.app.session, p).unwrap())
            .collect();
        for (count, text) in [(0, "Select edges"), (1, "1 selected"), (3, "3 selected")] {
            h.app.dialog.as_mut().unwrap().inputs.first_mut().unwrap().items = picks.iter().take(count).cloned().collect();
            h.frames(3);
            if count <= 1 {
                let tree = h.output.accesskit_update.as_ref().unwrap();
                let disabled = tree
                    .nodes
                    .iter()
                    .find_map(|(_, node)| (node.role() == egui::accesskit::Role::Button && node.label() == Some("OK")).then(|| node.is_disabled()))
                    .unwrap();
                assert_eq!(disabled, count == 0, "OK follows the actual required edge selection");
            }
            let mut output = h.ctx.run_ui(
                egui::RawInput {
                    time: Some(h.ctx.input(|input| input.time)),
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1400.0, 1000.0))),
                    ..Default::default()
                },
                |ui| h.app.ui(ui),
            );
            let painted = output.shapes.iter().any(|shape| contains_text(&shape.shape, text));
            output.textures_delta.clear();
            assert!(painted, "the actual field must paint {text}");
            let field = scenario::rect_of("dialog:field:Edges").unwrap();
            assert!(
                (field.width() - scenario::rect_of("dialog:control:Edges").unwrap().width()).abs() < 0.02,
                "clear is inside the full field (allow egui subpixel rounding)"
            );
            assert!(h.widget("OK").is_some());
            if count == 0 {
                h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
                assert!(h.app.dialog.is_some(), "invalid OK stays disabled");
            }
        }
    }

    #[test]
    fn degrees_reads_the_angle_of_stepped_text() {
        assert!((degrees("360 deg").unwrap() - 360.0).abs() < 1e-9);
        assert!((degrees("0,5 deg").unwrap() - 0.5).abs() < 1e-9);
        assert!((degrees("6.283185307179586 rad").unwrap() - 360.0).abs() < 1e-9);
        assert_eq!(degrees("a / 2"), None);
        assert_eq!(degrees(""), None);
    }
}

#[cfg(test)]
#[path = "dialogs_all_tests.rs"]
mod all_tests;

#[cfg(test)]
#[path = "dialogs_sketch_tests.rs"]
mod sketch_tests;
