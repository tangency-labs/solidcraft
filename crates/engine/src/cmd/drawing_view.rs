//! `drawing.view.snapshot` (W07, #5866): one orthographic view of the design, worked out and
//! returned without saving anything. It reads the evaluated model and changes nothing.
//!
//! `drawing.view.inspect` and `drawing.view.edit` (W17c, #4816) read and change a saved view of a
//! drawing: each setting with where its value comes from ([`solidcraft_doc::drawing::Drawing::resolve_view`])
//! and where the view sits on its sheet ([`crate::drawing_view::on_sheet`]), and a change as one
//! undo step. Drawing or measuring a view of another configuration evaluates a copy of the design
//! ([`crate::drawing_view::copy_for`]); the design is unchanged.
//!
//! `drawing.view.move` (#4816 E9) drags a saved view across its sheet: a view lined up with its
//! parent keeps to its line, and the views lined up with it follow. One undo step, with no model
//! evaluated.
//!
//! `drawing.view.move_to_sheet` and `drawing.view.copy_to_sheet` (#4816) put a saved view on a
//! sheet: the views lined up with it go with it, and keep their places beside it. A copy is
//! independent of the views it was copied from (Cameron, 2026-10-26). Each lists the views it puts
//! outside the sheet's drawing space and moves none back in (#3875 S5).
//!
//! `drawing.view.base` and `drawing.view.projected` (#4816) make a saved view: a base view of the
//! drawing's design on a sheet, or a view projected from another, lined up with it a gap past its
//! boundary unless placed. Each is one undo step, refused whole by the record's rules, and lists
//! itself when it lies outside the sheet's drawing space.
//!
//! What a saved view shows, its `source` and `hide`, is set when it is made or edited and follows
//! the parent as any setting does. The occurrences are checked as they are chosen: one deleted later
//! leaves the view as it is, and it draws what is left ([`crate::drawing_view::measured`]).
//!
//! The view's settings are `doc::drawing_view::ViewSettings`; its basis, scale and map onto
//! paper are `geom::drawing_view`'s. Without a scale it takes the largest ISO 5455 scale at
//! which the view, centred where it goes, lies inside the sheet's drawing border.

use std::collections::BTreeSet;
use std::sync::Arc;

use serde::Serialize;
use serde_json::{Value, json};
use solidcore_record::Plane;
use solidcore_record::Vec2;
use solidcraft_doc::config::RowKey;
use solidcraft_doc::display_units::unit_scale;
use solidcraft_doc::drawing::Drawing;
use solidcraft_doc::drawing::balloon::Balloon;
use solidcraft_doc::drawing::dimension::{Dimension, LabelKind, MAX_DECIMALS, reading_label};
use solidcraft_doc::drawing::note::Note;
use solidcraft_doc::drawing::view::{ResolvedView, SetBy, Setting, View, ViewKind, ViewPlace, check_occurrence_paths, check_pose};
use solidcraft_doc::drawing_view::{
    Exploded, JointPose, Looking, MAX_POSE_JOINTS, OccurrencePaths, Pose, StudyStep, TangentEdges, ViewConfiguration, ViewDirection, ViewOverrides,
};
use solidcraft_doc::ids;
use solidcraft_geom::Aabb2;
use solidcraft_geom::drawing_measure::{self, AngleFrom, LinearDirection, Measured, Reading};
use solidcraft_geom::drawing_sheet::{DrawingBorderProfile, Sheet};
use solidcraft_geom::drawing_view::{Invalid, NamedView, PRESETS, ProjectedSide, Scale};
use solidcraft_geom::hlr::Visibility;
use solidcraft_geom::paper::{MAX_COORDINATE_MM, Picture};
use solidcraft_geom::prelude::*;
use solidcraft_geom::section::CurveAccuracy;
use solidcraft_io::paper::MAX_PICTURE_DPI;
use solidcraft_render::VisualStyle;

use super::CommandSpec;
use crate::drawing_view::{
    CHORD_RANGE, Extents, LeftOut, LineRequest, Lines, NOTHING_CHOSEN, OnSheet, Room, ShownSet, Styles, Unmeasured, ViewSessions,
};
use crate::params::bad;
use crate::schema::drawing_view::{
    AccuracyResult, AlongParam, AngleFromParam, BaseParams, BoxResult, ChoicesResult, ConfigurationParam, ConfigurationResult, CopyParams,
    CreateResult, DPI_RANGE, DrawnJointResult, DrawnRowResult, EditParams, EditResult, ExplodedParam, ExtentsResult, FromResult, InspectParams,
    InspectResult, LineResult, LinesStatusResult, MeasureParam, MeasurementResult, MoveParams, OutsideResult, PathSegResult, PictureResult,
    PieceResult, PlaceParam, PoseParam, PoseResult, ProjectedParams, ScaleResult, SceneResult, SettingResult, SettingsResult, SnapshotParams,
    SnapshotResult, SourceResult, StudyPoseParam, StudyStepResult, ToSheetResult, ViewLineParam, ViewMoveParams, ViewMoveResult, ViewResult, ViewSet,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const CMD: &str = "drawing.view.snapshot";
const CONTRACT: TypedContract<SnapshotParams> = TypedContract::new();
const INSPECT: &str = "drawing.view.inspect";
const INSPECT_CONTRACT: TypedContract<InspectParams> = TypedContract::new();
const EDIT: &str = "drawing.view.edit";
const EDIT_CONTRACT: TypedContract<EditParams> = TypedContract::new();
const DRAG: &str = "drawing.view.move";
const DRAG_CONTRACT: TypedContract<ViewMoveParams> = TypedContract::new();
const MOVE: &str = "drawing.view.move_to_sheet";
const MOVE_CONTRACT: TypedContract<MoveParams> = TypedContract::new();
const COPY: &str = "drawing.view.copy_to_sheet";
const COPY_CONTRACT: TypedContract<CopyParams> = TypedContract::new();
const BASE: &str = "drawing.view.base";
const BASE_CONTRACT: TypedContract<BaseParams> = TypedContract::new();
const PROJECTED: &str = "drawing.view.projected";
const PROJECTED_CONTRACT: TypedContract<ProjectedParams> = TypedContract::new();

/// How far a new projected view's boundary is from its parent's, in paper millimetres, until a
/// drawing style gives it.
const DEFAULT_GAP_MM: f64 = 20.0;

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new(CMD, "Drawing View Snapshot", crate::checked!("drawing.view.snapshot", snapshot))
    .icon("export")
    .noundo()
    .contract(&CONTRACT)
    .output(output_schema_for::<SnapshotResult>)
    .params("view?: front (default), back, top, bottom, left, right, iso (= iso_front_right), iso_front_left, iso_back_right, iso_back_left, or {direction, up, origin?}; scale?: \"paper:model\" such as \"1:2\" (default: the largest ISO 5455 scale that fits the sheet's drawing area); position_mm?: [x, y] for the view's centre on the sheet (default the sheet's centre); sheet?: {size: {standard: a3} or {custom: {sides_mm: [w, h]}}, orientation?: landscape|portrait, dpi?: 25..2400} (default A3 landscape, 300 dpi); visual_style?, reference_style?: visible (default), visible_hidden or shaded (drawn as a picture, without its own lines, and not with scene; shaded_visible and shaded_visible_hidden aren't supported yet); reference_hides_model?: false; reference_shown_percent?: 0..100, how far the box grows from the model's towards holding its reference parts, each side on its own (0, the default: the model's box; 100: holds them all); tangent_edges?: hide (default) or show; chord_mm?: 0.001..1 paper mm the lines are sampled at (default 0.01); max_error_mm?: the accuracy the lines must certify on paper (refused while they come from the mesh); scene?: true adds the lines as a paper scene of the sheet, hidden lines dashed under the visible ones; source?: {occurrences: [[ids…]]}, the occurrence subtrees drawn ([] the design's own bodies; default every body); hide?: [[ids…]], subtrees left out; at most 1000 paths in each and 4096 ids in all. measure?: up to 100 values to read on the view, each {linear: {along: x|y, from, to}}, {aligned: {from, to}} (model points), {angle: {edge: [a, b], from: horizontal|vertical|[c, d]}} or {radius: {edge}} (an edge as drawing.dimension.add takes it, which the view must see face on), with units?: mm|cm|m|in|ft (default the design's) and decimals?: 0..6 (default 2) for their labels; the reply then has measurements: [{kind, value_mm and paper_mm, or value_rad, label, meaning: projected, foreshortened, status: exact|approximate}]. Every body the design places, whatever the browser shows. Returns {view, scale: {text, factor, fitted}, position_mm, sheet, extents (model mm on the view's plane), boundary_mm (on the sheet), lines: [{occurrence, body, reference, source: {kind: edge, edge, tangent} or {kind: silhouette, face}, pieces: [{visibility, path: [paper mm lines, and arcs for circles facing the view], accuracy}]}], status: {lines: approximate, provenance: mesh_projection, chord_mm, model_chord_mm, uncertain_pieces, covered_pieces}, stats, scene (with scene: true: {size_mm, groups: [{object: 0, kind: view, items: [{kind: stroke, path, closed, width_mm}]}]}), picture? ({pixels: [w, h], dpi, rect_mm} when something is shaded: the boundary's pixel grid at sheet.dpi or 1200, the lower, which dpi gives), source}. Read-only: saves nothing."),
    CommandSpec::new(INSPECT, "Inspect Drawing View", crate::checked!("drawing.view.inspect", inspect))
        .icon("inspect")
        .in_drawing()
        .noundo()
        .doc_only()
        .contract(&INSPECT_CONTRACT)
        .output(output_schema_for::<InspectResult>)
        .params("view: a saved view's id → {view, sheet, kind: base|projected, source, parent?, side?, direction, place: {at: [x, y]} or {aligned: n}, settings: {scale, visual_style, reference_style, reference_hides_model, tangent_edges, occurrences (null: every body), exploded (\"assembled\" or {view: id}), pose (\"designed\", {joints: [...]} as saved, in radians and mm, or {study: {id, step}}; with drawn?, skipped? and study? for a position of its own), configuration (\"active\" or {row: id}, with drawn: {id, name} or null, and errors?), hide: {value, from: this|parent|default, parent?, set}}, reference_shown_percent, boundary_mm: {min, max} on the sheet (a 20 x 20 mm placeholder round the view's centre when there's nothing to measure), extents: {min, max, error_mm, status: approximate|placeholder, provenance (a placeholder's reason: past_walk_limit, no_bodies, not_placed, nothing_chosen, no_exploded_view, not_posed, no_configuration or not_configured)} in model mm on the view's plane, linked? (a projected view's display settings: true, false or \"partly\"), dimensions: [its dimensions' ids, in id order], balloons: [its balloons' ids, in id order], choices: {visual_style, scale}}. Read-only; a view of another configuration evaluates a copy of the design; the design is unchanged."),
    CommandSpec::new(EDIT, "Edit Drawing View", crate::checked!("drawing.view.edit", edit))
        .icon("edit")
        .in_drawing()
        .doc_only()
        .contract(&EDIT_CONTRACT)
        .output(output_schema_for::<EditResult>)
        .params("view: a saved view's id; set: {scale?, visual_style?, reference_style?, reference_hides_model?, tangent_edges?, line_group?: an ISO 128 line group by its wide line (0.25, 0.35, 0.5, 0.7, 1, 1.4 or 2), source?: {occurrences: [[ids…]]}, hide?: [[ids…]] as drawing.view.snapshot's, exploded?, pose?, configuration?: as drawing.view.base's: each a value, or null to follow the parent (a base view keeps a scale; a base view's line group follows its sheet's, then its drawing's); reference_shown_percent?; view?: a base view's direction; place?: {at: [x, y]} or {aligned: n}; linked?: a projected view's display settings all following its parent (true) or all its own, as they are now (false)} → {view, changed: [fields], followers: [the views projected from it whose settings changed with it]}. One undo step; no model is evaluated, but to check a new source or hide, a view of another configuration evaluates a copy of the design; the design is unchanged."),
    CommandSpec::new(DRAG, "Move View", crate::checked!("drawing.view.move", move_view))
        .icon("move")
        .in_drawing()
        .doc_only()
        .contract(&DRAG_CONTRACT)
        .output(output_schema_for::<ViewMoveResult>)
        .params("view: a saved view's id; to_mm: [x, y] for its centre; break_alignment?: place a view lined up with its parent apart (default false: it takes only its free axis) → {view, place, centre_mm, followers: the views lined up with it that went with it, outside: [{view, boundary_mm, unmeasured?}], those now outside the sheet's drawing space}. One undo step, no model evaluated; a move to where it is makes no step."),
    CommandSpec::new(MOVE, "Move View to Sheet", crate::checked!("drawing.view.move_to_sheet", move_to_sheet))
        .icon("move")
        .in_drawing()
        .doc_only()
        .contract(&MOVE_CONTRACT)
        .output(output_schema_for::<ToSheetResult>)
        .params("view: a saved view's id; sheet: id or name; position_mm?: [x, y] for its centre (default: where it is, or the sheet's centre for a view lined up with its parent); break_alignment?: true to move a view lined up with its parent, which is then placed apart → {view, sheet, views: [it, then the views lined up with it, which go with it], outside?: [those whose boundary leaves the sheet's drawing space or lies over its title block, none moved back in]}. One undo step. On the sheet it is on, a position_mm moves it as drawing.view.move does, and none changes nothing."),
    CommandSpec::new(COPY, "Copy View to Sheet", crate::checked!("drawing.view.copy_to_sheet", copy_to_sheet))
        .icon("copy")
        .in_drawing()
        .doc_only()
        .contract(&COPY_CONTRACT)
        .output(output_schema_for::<ToSheetResult>)
        .params("view: a saved view's id; sheet: id or name; position_mm?: [x, y] for the copy's centre (default the sheet's centre); with_linked?: the views lined up with it copied too, lined up with the copy (true), or not (false); needed when it has any → {view: the copy, sheet, views: [the copies, its first], outside?: [those whose boundary leaves the sheet's drawing space or lies over its title block]}. A copy follows none of the views it was copied from: a projected view copied alone becomes a base view, looking the way it did, with the settings it had. Each view's dimensions and balloons are copied with it, with ids of their own. One undo step."),
    CommandSpec::new(BASE, "Base View", crate::checked!("drawing.view.base", base))
        .icon("new")
        .in_drawing()
        .doc_only()
        .contract(&BASE_CONTRACT)
        .output(output_schema_for::<CreateResult>)
        .params("sheet: id or name; view?: as drawing.view.snapshot's (front by default); scale?: \"paper:model\" (default: the largest ISO 5455 preset at which it lies inside the sheet's drawing space and off its title block, centred where it goes, or 1:1 with nothing to measure; saved either way); position_mm?: [x, y] for its centre (default the sheet's centre); visual_style?, reference_style?, reference_hides_model?, reference_shown_percent?, tangent_edges?, source?: {occurrences: [[ids…]]}, hide?: [[ids…]] (as drawing.view.snapshot's), exploded?: \"assembled\" (default) or an exploded view of the design by its name or id, pose?: \"designed\" (default), [{joint: id or name, values: [as joint.drive takes them]}] or {study: id or name, step: a whole step of its timeline}, configuration?: \"active\" (default) or a row of the design's configuration table by its name or id: settings it sets itself (each left out follows the default) → {view, sheet, scale: {text, factor, fitted}, boundary_mm, outside?: [it, when its boundary leaves the sheet's drawing space or lies over its title block]}. A view of the drawing's design. One undo step; a view of another configuration evaluates a copy of the design; the design is unchanged."),
    CommandSpec::new(PROJECTED, "Projected View", crate::checked!("drawing.view.projected", projected))
        .icon("new")
        .in_drawing()
        .doc_only()
        .contract(&PROJECTED_CONTRACT)
        .output(output_schema_for::<CreateResult>)
        .params("parent: a saved view's id; side: left, right, above or below, where it sits (the drawing's projection says which side of the model it shows); place?: {at: [x, y]} or {aligned: n} (default: lined up with its parent, its boundary 20 mm past the parent's) → {view, sheet: its parent's, scale: its parent's, boundary_mm, outside?}. Its scale and display settings follow its parent; drawing.view.edit sets its own. One undo step; a view of another configuration evaluates a copy of the design; the design is unchanged."),
];

fn snapshot(s: &mut Session, p: &Value) -> Result<Value> {
    let request: SnapshotParams = crate::schema::read(CMD, p)?;
    let settings = request.settings();
    let basis = settings.validate().map_err(|e| bad(CMD, e.to_string()))?;
    if let Some(why) = crate::drawing_view::not_drawable(settings.visual_style, settings.reference_style) {
        return Err(bad(CMD, why));
    }
    if !(request.sheet.dpi.is_finite() && DPI_RANGE.contains(&request.sheet.dpi)) {
        return Err(bad(CMD, format!("sheet.dpi is a number from {} to {}", DPI_RANGE.start(), DPI_RANGE.end())));
    }
    let chord_mm = request.chord_mm.0;
    if !(chord_mm.is_finite() && CHORD_RANGE.contains(&chord_mm)) {
        return Err(bad(CMD, format!("chord_mm is a number from {} to {} (paper mm)", CHORD_RANGE.start(), CHORD_RANGE.end())));
    }
    if request.max_error_mm.is_some_and(|e| !(e.is_finite() && e > 0.0)) {
        return Err(bad(CMD, "max_error_mm is a number above 0 (paper mm)"));
    }
    let sheet = Sheet { size: request.sheet.size, orientation: request.sheet.orientation };
    let paper = sheet.size_mm().map_err(|e| bad(CMD, e.to_string()))?;
    let border = DrawingBorderProfile::iso().layout(&sheet).map_err(|e| bad(CMD, e.to_string()))?.drawing_border;

    let source = request.source.as_ref().map(|chosen| chosen.occurrences.as_slice());
    check_paths(s, source, &request.hide, CMD)?;
    let set = crate::drawing_view::shown(s).map_err(|why| bad(CMD, why.to_string()))?.keep(source, &request.hide);
    if set.bodies.is_empty() {
        return Err(bad(CMD, NOTHING_CHOSEN));
    }
    let extents = crate::drawing_view::measure(&set, &basis).map_err(|why| bad(CMD, why.to_string()))?;
    let shown = crate::drawing_view::shows(&set, &basis, &extents, settings.reference_shown_percent);
    let position = request.position_mm.map_or(paper * 0.5, |p| p.0);
    let (scale, fitted) = match settings.scale {
        Some(scale) => (scale, false),
        None => (fit(&extents, shown, position, Room { space: border, block: None }, CMD)?, true),
    };
    let frame = crate::drawing_view::frame(basis, scale, &extents.model, position, CMD)?;
    let boundary = frame.boundary(&shown).map_err(|e| bad(CMD, e.to_string()))?;
    let on_paper = [boundary.min, boundary.max].iter().all(|c| c.is_finite() && c.x.abs() <= MAX_COORDINATE_MM && c.y.abs() <= MAX_COORDINATE_MM);
    if !on_paper {
        return Err(bad(CMD, format!("at scale {scale} the view reaches past ±{MAX_COORDINATE_MM} mm on paper: choose a smaller scale")));
    }
    let styles =
        Styles { display: settings.visual_style, reference: settings.reference_style, reference_hides_model: settings.reference_hides_model };
    let measurements = measurements(s, &request, &basis, scale)?;
    let asked = LineRequest {
        shows: shown,
        hidden: settings.visual_style == VisualStyle::VisibleHidden,
        reference_hidden: settings.reference_style == VisualStyle::VisibleHidden,
        reference_hides_model: settings.reference_hides_model,
        tangent_edges: settings.tangent_edges == TangentEdges::Show,
        chord_mm,
    };
    let mut drawn = crate::drawing_view::lines(s, &set, &frame, &asked, CMD)?;
    styles.leave_shaded_to_pictures(&mut drawn);
    if let Some(max) = request.max_error_mm {
        certify(&drawn, max, scale)?;
    }
    let parts = crate::drawing_picture::view_parts(&set, styles);
    let scene = if request.scene {
        let mut group = crate::drawing_view::paper_group(&drawn, 0, solidcraft_geom::paper::line::LineGroup::DEFAULT);
        group.items.splice(0..0, crate::drawing_picture::picture_items(0, &parts, boundary));
        let scene = solidcraft_geom::paper::Scene::new(paper, vec![group]);
        scene.validate().map_err(|e| bad(CMD, format!("{e}: raise chord_mm, or choose a smaller view")))?;
        Some(SceneResult(scene))
    } else {
        None
    };
    let picture = if parts.is_empty() { None } else { Some(picture(boundary, request.sheet.dpi)?) };

    let named = match settings.view {
        ViewDirection::Named(view) => Some(snake_name(view)),
        ViewDirection::Looking(_) => None,
    };
    let reply = SnapshotResult {
        view: ViewResult {
            named,
            // The line of sight, -(x × y), written so no component reads -0.
            direction: basis.y.cross(basis.x).into(),
            x: basis.x.into(),
            y: basis.y.into(),
            origin: basis.origin.into(),
        },
        scale: ScaleResult { text: scale.to_string(), factor: scale.factor(), fitted },
        position_mm: position.into(),
        sheet: request.sheet,
        visual_style: snake_name(settings.visual_style),
        reference_style: snake_name(settings.reference_style),
        extents: extents_result(&extents, None),
        boundary_mm: BoxResult { min: boundary.min.into(), max: boundary.max.into() },
        lines: line_results(&set, &drawn),
        status: LinesStatusResult {
            lines: "approximate",
            provenance: "mesh_projection",
            chord_mm,
            model_chord_mm: drawn.model_chord_mm,
            uncertain_pieces: 0,
            covered_pieces: drawn.covered,
        },
        stats: drawn.stats.into(),
        scene,
        picture,
        measurements,
        source: SourceResult { bodies: set.bodies.len(), timeline_at_end: s.doc.marker.is_none() },
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(|e| crate::EngineError::Other(e.to_string()))
}

/// The most values one snapshot reads.
const MAX_MEASUREMENTS: usize = 100;
/// The decimals of a measurement's label unless told.
const DEFAULT_DECIMALS: u8 = 2;

/// `source` and `hide` checked as `cmd` asks for them: the record's rule
/// ([`check_occurrence_paths`]), and each a path `occurrence_placement` takes.
fn check_paths(s: &Session, source: Option<&[Vec<u64>]>, hide: &[Vec<u64>], cmd: &str) -> Result<()> {
    let source = source.unwrap_or_default();
    check_occurrence_paths(source, hide).map_err(|e| bad(cmd, e.to_string()))?;
    for (name, paths) in [("source.occurrences", source), ("hide", hide)] {
        for path in paths.iter().filter(|path| !path.is_empty()) {
            s.occurrence_placement(path).map_err(|e| bad(cmd, format!("{name} path {path:?}: {e}")))?;
        }
    }
    Ok(())
}

/// What `request.measure` reads on the view through `basis` at `scale`, in its order. The first that
/// can't be read refuses the call, by its place in the list.
fn measurements(s: &Session, request: &SnapshotParams, basis: &Plane, scale: Scale) -> Result<Vec<MeasurementResult>> {
    if request.measure.len() > MAX_MEASUREMENTS {
        return Err(bad(CMD, format!("at most {MAX_MEASUREMENTS} measurements in measure")));
    }
    let decimals = request.decimals.unwrap_or(DEFAULT_DECIMALS);
    if decimals > MAX_DECIMALS {
        return Err(bad(CMD, format!("decimals is 0 to {MAX_DECIMALS}")));
    }
    let units = request.units.as_deref().unwrap_or(s.doc.units.as_str());
    if unit_scale(units).1 != units {
        return Err(bad(CMD, "units is mm, cm, m, in or ft"));
    }
    let length = |kind, reading: Measured, label: LabelKind| MeasurementResult {
        kind,
        value_mm: Some(reading.value),
        paper_mm: Some(reading.value * scale.factor()),
        value_rad: None,
        label: reading_label(label, reading.value, units, decimals),
        meaning: "projected",
        foreshortened: reading.foreshortened,
        status: "exact",
    };
    let one = |m: &MeasureParam| -> std::result::Result<MeasurementResult, String> {
        let invalid = |e: Invalid| e.0.to_string();
        Ok(match m {
            MeasureParam::Linear { along, from, to } => {
                let direction = if *along == AlongParam::X { LinearDirection::Horizontal } else { LinearDirection::Vertical };
                let reading = drawing_measure::linear(basis, from.0, to.0, direction, Reading::Projected).map_err(invalid)?;
                length("linear", reading, LabelKind::Length)
            }
            MeasureParam::Aligned { from, to } => {
                let reading = drawing_measure::linear(basis, from.0, to.0, LinearDirection::Aligned, Reading::Projected).map_err(invalid)?;
                length("aligned", reading, LabelKind::Length)
            }
            MeasureParam::Angle { edge: [a, b], from } => {
                let line = match from {
                    AngleFromParam::View(ViewLineParam::Horizontal) => AngleFrom::Horizontal,
                    AngleFromParam::View(ViewLineParam::Vertical) => AngleFrom::Vertical,
                    AngleFromParam::Line([c, d]) => AngleFrom::Edge(c.0, d.0),
                };
                let reading = drawing_measure::angle(basis, (a.0, b.0), line, Reading::Projected).map_err(invalid)?;
                MeasurementResult { value_mm: None, paper_mm: None, value_rad: Some(reading.value), ..length("angle", reading, LabelKind::Angle) }
            }
            MeasureParam::Radius { edge } => {
                let named = s.doc.edge_reference(&s.model, &edge.pick, &edge.path).map_err(|e| e.to_string())?;
                let (radius, exact) = s.doc.radius_on_view(&s.model, &named, basis).map_err(|status| {
                    let (word, why, _) = super::drawing_dimension::status_words(status);
                    why.unwrap_or_else(|| word.to_string())
                })?;
                let reading = Measured { value: radius, foreshortened: false };
                MeasurementResult { status: if exact { "exact" } else { "approximate" }, ..length("radius", reading, LabelKind::Radius) }
            }
        })
    };
    let read = request.measure.iter().enumerate().map(|(at, m)| one(m).map_err(|why| bad(CMD, format!("measure[{at}]: {why}"))));
    read.collect()
}

/// Refused when saved view `view` of `record` doesn't draw a part that one of the occurrence paths
/// `paths` places: an annotation attaches only to what its view draws.
pub(super) fn check_drawn<'a>(record: &Drawing, view: u64, paths: impl IntoIterator<Item = &'a [u64]>, cmd: &str) -> Result<()> {
    match not_drawn(record, view, paths) {
        Some(why) => Err(bad(cmd, format!("view {} doesn't draw that part: {why}", ids::encode(view)))),
        None => Ok(()),
    }
}

/// Why view `view` of `record` doesn't draw the first of `paths` it leaves out, in words ([`left_out`]);
/// nothing when it draws them all, or for a view that can't be resolved, which its own reply reports.
/// The view is resolved once, however many paths there are.
fn not_drawn<'a>(record: &Drawing, view: u64, paths: impl IntoIterator<Item = &'a [u64]>) -> Option<String> {
    let resolved = record.resolve_view(view).ok()?;
    let left = paths.into_iter().find_map(|path| crate::drawing_view::left_out(&resolved, path))?;
    Some(match left {
        LeftOut::Hidden { by, set_by } => format!("its hide leaves out occurrence path {by:?}{}", set_on(set_by)),
        LeftOut::NotChosen { set_by } => format!("its source draws only the occurrences it chose{}", set_on(set_by)),
    })
}

/// Where a setting that leaves a part out was set, in words for [`not_drawn`]: nothing when the view
/// itself or the default set it. Hide and the chosen occurrences come from a view's chain today, but
/// a sheet and a drawing can set other settings (its line group), so each says where.
fn set_on(set_by: SetBy) -> String {
    match set_by {
        SetBy::Parent(parent) => format!(" (set on view {})", ids::encode(parent)),
        SetBy::Sheet => " (set on its sheet)".to_string(),
        SetBy::Drawing => " (set on the drawing)".to_string(),
        SetBy::This | SetBy::Default => String::new(),
    }
}

/// The warning for an annotation on view `view` that points through `paths`, when the view's
/// settings were changed to leave one of them out after it was added: it stays, and says so.
pub(super) fn drawn_warning<'a>(record: &Drawing, view: u64, paths: impl IntoIterator<Item = &'a [u64]>) -> Vec<String> {
    not_drawn(record, view, paths).map(|why| format!("view {} doesn't draw this part now: {why}", ids::encode(view))).into_iter().collect()
}

/// Refused when `view` would show nothing of a design that has bodies to show. One with nothing to
/// measure anyway isn't the choice's fault, and gets its placeholder.
fn check_shows_bodies(shown: &std::result::Result<ShownSet, Unmeasured>, view: &ResolvedView, cmd: &str) -> Result<()> {
    match shown {
        Ok(set) if set.of_view(view).bodies.is_empty() => Err(bad(cmd, NOTHING_CHOSEN)),
        _ => Ok(()),
    }
}

/// Occurrence paths as the record keeps them.
fn occurrence_paths(paths: &[Vec<u64>]) -> OccurrencePaths {
    OccurrencePaths::from(paths)
}

/// Occurrence paths as inspect answers them.
fn path_lists(paths: Option<OccurrencePaths>) -> Option<Vec<Vec<u64>>> {
    paths.map(|paths| paths.to_vec())
}

/// Refused unless every piece of `drawn` certifies `max` paper mm at `scale` (the accuracy rule on
/// #5866): an analytic piece meets any, a bounded one its bound, a sampled one none.
fn certify(drawn: &Lines, max: f64, scale: Scale) -> Result<()> {
    let (mut bounded, mut sampled) = (0usize, 0usize);
    for piece in drawn.lines.iter().flat_map(|l| &l.pieces) {
        match piece.accuracy {
            CurveAccuracy::Analytic => {}
            CurveAccuracy::Bounded { max_deviation_mm } if max_deviation_mm <= max => {}
            CurveAccuracy::Bounded { .. } => bounded += 1,
            CurveAccuracy::Sampled { .. } => sampled += 1,
        }
    }
    if bounded + sampled == 0 {
        return Ok(());
    }
    Err(bad(
        CMD,
        format!(
            "accuracy not met: {bounded} bounded and {sampled} sampled pieces can't certify max_error_mm {max} at scale {scale}; the lines come from the mesh until the exact lines (W06) are used"
        ),
    ))
}

/// `drawn`'s lines as the reply gives them, each named by the shown body it is of.
fn line_results(shown: &ShownSet, drawn: &Lines) -> Vec<LineResult> {
    let line = |l: &crate::drawing_view::ViewLine| {
        let body = shown.bodies.get(l.shown);
        LineResult {
            occurrence: body.map(|b| b.path.clone()).unwrap_or_default(),
            body: body.map(|b| b.body.clone()).unwrap_or_default(),
            reference: l.reference,
            source: l.source.into(),
            pieces: l
                .pieces
                .iter()
                .map(|p| PieceResult {
                    visibility: if p.visibility == Visibility::Hidden { "hidden" } else { "visible" },
                    path: p.path.iter().copied().map(PathSegResult).collect(),
                    accuracy: AccuracyResult(p.accuracy),
                })
                .collect(),
        }
    };
    drawn.lines.iter().map(line).collect()
}

fn inspect(s: &mut Session, p: &Value) -> Result<Value> {
    let request: InspectParams = crate::schema::read(INSPECT, p)?;
    let d = s.drawing(INSPECT)?;
    let view = find(d, request.view.0, INSPECT)?;
    let resolved = d.resolve_view(view.id).map_err(|e| bad(INSPECT, e.to_string()))?;
    let mut sessions = ViewSessions::new(s);
    let placed = crate::drawing_view::on_sheet_from(&mut sessions, d, view.id, INSPECT)?;
    let pose = pose_result(s, &mut sessions, &resolved, view.settings.pose.clone());
    let configuration = configuration_result(&mut sessions, &resolved, view.settings.configuration);
    let (parent, side) = match view.kind {
        ViewKind::Base { .. } => (None, None),
        ViewKind::Projected { parent, side } => (Some(ids::encode(parent)), Some(snake_name(side))),
    };
    let own = &view.settings;
    let reply = InspectResult {
        view: ids::encode(view.id),
        sheet: ids::encode(view.sheet),
        kind: if parent.is_some() { "projected" } else { "base" },
        source: ids::encode(resolved.source),
        parent,
        side,
        direction: direction(&resolved.basis),
        place: view.place.into(),
        settings: SettingsResult {
            scale: setting(resolved.scale, own.scale, |s| s.to_string()),
            visual_style: setting(resolved.visual_style, own.visual_style, snake_name),
            reference_style: setting(resolved.reference_style, own.reference_style, snake_name),
            reference_hides_model: setting(resolved.reference_hides_model, own.reference_hides_model, |b| b),
            tangent_edges: setting(resolved.tangent_edges, own.tangent_edges, snake_name),
            line_group: setting(resolved.line_group, own.line_group, snake_name),
            occurrences: setting(resolved.occurrences, own.occurrences.clone().map(Some), path_lists),
            hide: setting(resolved.hide, own.hide.clone().map(Some), path_lists),
            exploded: setting(resolved.exploded, own.exploded, exploded_value),
            pose,
            configuration,
        },
        reference_shown_percent: view.reference_shown_percent,
        boundary_mm: BoxResult { min: placed.boundary.min.into(), max: placed.boundary.max.into() },
        extents: extents_result(&placed.extents, placed.unmeasured),
        linked: matches!(view.kind, ViewKind::Projected { .. }).then(|| match display_set(own) {
            0 => Value::Bool(true),
            DISPLAY_GROUP => Value::Bool(false),
            _ => Value::String("partly".into()),
        }),
        dimensions: d.dimensions.iter().filter(|dimension| dimension.view == view.id).map(|dimension| ids::encode(dimension.id)).collect(),
        balloons: d.balloons.iter().filter(|balloon| balloon.view == view.id).map(|balloon| ids::encode(balloon.id)).collect(),
        choices: ChoicesResult {
            visual_style: VisualStyle::ALL.iter().map(|s| snake_name(*s)).collect(),
            scale: PRESETS.iter().map(ToString::to_string).collect(),
        },
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(|e| crate::EngineError::Other(e.to_string()))
}

fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let request: EditParams = crate::schema::read(EDIT, p)?;
    let id = request.view.0;
    if request.set.source.is_some() || request.set.hide.is_some() {
        let source = request.set.source.as_ref().and_then(Option::as_ref).map(|chosen| chosen.occurrences.as_slice());
        check_paths(s, source, request.set.hide.as_ref().and_then(Option::as_deref).unwrap_or_default(), EDIT)?;
        let mut staged = Drawing::clone(s.drawing(EDIT)?);
        change(&mut staged, id, &request.set, &Found::default())?;
        let resolved = staged.resolve_view(id).map_err(|e| bad(EDIT, e.to_string()))?;
        check_shows_bodies(&crate::drawing_view::shown_by(&mut ViewSessions::new(s), &resolved), &resolved, EDIT)?;
    }
    let set = &request.set;
    let found = Found {
        exploded: set.exploded.as_ref().map(|chosen| chosen.as_ref().map(|chosen| exploded_of(s, chosen, EDIT)).transpose()).transpose()?,
        pose: set.pose.as_ref().map(|chosen| chosen.as_ref().map(|chosen| pose_of(s, chosen, EDIT)).transpose()).transpose()?,
        configuration: set
            .configuration
            .as_ref()
            .map(|chosen| chosen.as_ref().map(|chosen| configuration_of(s, chosen, EDIT)).transpose())
            .transpose()?,
    };
    let descendants = descendants(s.drawing(EDIT)?, id, |_| true);
    let apart = found.exploded.is_some() || found.pose.is_some() || found.configuration.is_some();
    if apart || set.visual_style.is_some() || set.reference_style.is_some() || set.linked.is_some() {
        let mut staged = Drawing::clone(s.drawing(EDIT)?);
        change(&mut staged, id, set, &found)?;
        check_apart(&staged, std::iter::once(id).chain(descendants.iter().copied()), EDIT)?;
    }
    let resolve = |d: &Drawing, ids: &[u64]| ids.iter().map(|id| d.resolve_view(*id).ok()).collect::<Vec<Option<ResolvedView>>>();
    let before = resolve(s.drawing(EDIT)?, &descendants);
    let changed = s.edit_drawing(EDIT, |d| change(d, id, &request.set, &found))?;
    let after = resolve(s.drawing(EDIT)?, &descendants);
    let followers = descendants.iter().zip(before.iter().zip(&after)).filter(|(_, (b, a))| b != a).map(|(id, _)| ids::encode(*id)).collect();
    let reply = EditResult { view: ids::encode(id), changed, followers, param_warnings: Vec::new() };
    serde_json::to_value(reply).map_err(|e| crate::EngineError::Other(e.to_string()))
}

/// The display settings a linked view takes from its parent: every inheritable one but the scale,
/// which keeps its own switch.
const DISPLAY_GROUP: usize = 5;

/// How many of the display settings `own` sets itself.
fn display_set(own: &ViewOverrides) -> usize {
    [
        own.visual_style.is_some(),
        own.reference_style.is_some(),
        own.reference_hides_model.is_some(),
        own.tangent_edges.is_some(),
        own.line_group.is_some(),
    ]
    .into_iter()
    .filter(|set| *set)
    .count()
}

/// View `id` of `d`, for `cmd`.
fn find<'a>(d: &'a Drawing, id: u64, cmd: &str) -> Result<&'a View> {
    d.view(id).ok_or_else(|| bad(cmd, format!("the drawing has no view {}", ids::encode(id))))
}

/// The views projected from view `id`, directly or through others, in the drawing's order: each
/// one `follows` takes, whose parent is `id` or one of them.
fn descendants(d: &Drawing, id: u64, follows: impl Fn(&View) -> bool) -> Vec<u64> {
    let mut found = vec![id];
    // Each pass adds the views whose parent is found; a chain is at most MAX_VIEW_DEPTH views.
    for _ in 0..solidcraft_doc::drawing::view::MAX_VIEW_DEPTH {
        let more: Vec<u64> = d
            .views
            .iter()
            .filter(|v| !found.contains(&v.id) && matches!(v.kind, ViewKind::Projected { parent, .. } if found.contains(&parent)) && follows(v))
            .map(|v| v.id)
            .collect();
        if more.is_empty() {
            break;
        }
        found.extend(more);
    }
    found.retain(|v| *v != id);
    d.views.iter().map(|v| v.id).filter(|v| found.contains(v)).collect()
}

/// What an edit's choices name in the design, found there before it changes anything: its exploded
/// view ([`exploded_of`]), its position ([`pose_of`]) and its configuration ([`configuration_of`]),
/// each `None` when the edit leaves it.
#[derive(Default)]
struct Found {
    exploded: Option<Option<Exploded>>,
    pose: Option<Option<Pose>>,
    configuration: Option<Option<ViewConfiguration>>,
}

/// `set` made on view `id` of `d`, with what it names in the design as `found`: the fields it changes,
/// by name.
fn change(d: &mut Drawing, id: u64, set: &ViewSet, found: &Found) -> Result<Vec<String>> {
    let resolved = d.resolve_view(id).map_err(|e| bad(EDIT, e.to_string()))?;
    let at = d.view_index(id).ok_or_else(|| bad(EDIT, format!("the drawing has no view {}", ids::encode(id))))?;
    let Some(view) = d.views.get_mut(at).map(std::sync::Arc::make_mut) else { return Ok(Vec::new()) };
    let before = view.clone();
    let base = matches!(view.kind, ViewKind::Base { .. });
    let own = &mut view.settings;
    if let Some(linked) = set.linked {
        if base {
            return Err(bad(EDIT, "a base view has no parent to follow: `linked` is a projected view's"));
        }
        // Following: the display settings cleared. Its own: each at the value it has now.
        own.visual_style = (!linked).then_some(resolved.visual_style.value);
        own.reference_style = (!linked).then_some(resolved.reference_style.value);
        own.reference_hides_model = (!linked).then_some(resolved.reference_hides_model.value);
        own.tangent_edges = (!linked).then_some(resolved.tangent_edges.value);
        own.line_group = (!linked).then_some(resolved.line_group.value);
    }
    if let Some(scale) = set.scale {
        if base && scale.is_none() {
            return Err(bad(EDIT, "a base view has no parent to follow: it keeps a scale"));
        }
        own.scale = scale.map(|s| s.0);
    }
    if let Some(style) = set.visual_style {
        own.visual_style = style.map(|s| s.0);
    }
    if let Some(style) = set.reference_style {
        own.reference_style = style.map(|s| s.0);
    }
    if let Some(hides) = set.reference_hides_model {
        own.reference_hides_model = hides;
    }
    if let Some(edges) = set.tangent_edges {
        own.tangent_edges = edges.map(|e| e.0);
    }
    if let Some(group) = set.line_group {
        own.line_group = group.map(|g| g.0);
    }
    if let Some(source) = &set.source {
        own.occurrences = source.as_ref().map(|chosen| occurrence_paths(&chosen.occurrences));
    }
    if let Some(hide) = &set.hide {
        own.hide = hide.as_deref().map(occurrence_paths);
    }
    if let Some(exploded) = found.exploded {
        own.exploded = exploded;
    }
    if let Some(pose) = &found.pose {
        own.pose = pose.clone();
    }
    if let Some(configuration) = found.configuration {
        own.configuration = configuration;
    }
    if let Some(percent) = set.reference_shown_percent {
        view.reference_shown_percent = percent;
    }
    if let Some(direction) = set.view {
        let ViewKind::Base { view: looking, .. } = &mut view.kind else {
            return Err(bad(EDIT, "a projected view looks from its side of its parent: only a base view's direction changes"));
        };
        *looking = direction.0;
    }
    if let Some(place) = set.place {
        view.place = match place {
            PlaceParam::At(at) => ViewPlace::At(at),
            PlaceParam::Aligned(along) => ViewPlace::Aligned(along),
        };
    }
    Ok(changed_fields(&before, view))
}

/// The names of the fields that differ between `a` and `b`, two states of one view.
fn changed_fields(a: &View, b: &View) -> Vec<String> {
    let (x, y) = (&a.settings, &b.settings);
    let fields = [
        ("scale", x.scale != y.scale),
        ("visual_style", x.visual_style != y.visual_style),
        ("reference_style", x.reference_style != y.reference_style),
        ("reference_hides_model", x.reference_hides_model != y.reference_hides_model),
        ("tangent_edges", x.tangent_edges != y.tangent_edges),
        ("line_group", x.line_group != y.line_group),
        ("source", x.occurrences != y.occurrences),
        ("hide", x.hide != y.hide),
        ("exploded", x.exploded != y.exploded),
        ("pose", x.pose != y.pose),
        ("configuration", x.configuration != y.configuration),
        ("reference_shown_percent", a.reference_shown_percent != b.reference_shown_percent),
        ("view", a.kind != b.kind),
        ("place", a.place != b.place),
    ];
    fields.into_iter().filter(|(_, changed)| *changed).map(|(name, _)| name.to_string()).collect()
}

/// Extents as the replies give them: measured on the display mesh, or a placeholder's and why.
/// The views [`crate::drawing_view::outside`] lists, as the commands answer them.
pub(super) fn outside_results(outside: Vec<(u64, OnSheet)>) -> Vec<OutsideResult> {
    let result = |(id, placed): (u64, OnSheet)| OutsideResult {
        view: ids::encode(id),
        boundary_mm: BoxResult { min: placed.boundary.min.into(), max: placed.boundary.max.into() },
        unmeasured: placed.unmeasured.map(Unmeasured::name),
    };
    outside.into_iter().map(result).collect()
}

fn extents_result(extents: &Extents, unmeasured: Option<Unmeasured>) -> ExtentsResult {
    ExtentsResult {
        min: extents.model.min.into(),
        max: extents.model.max.into(),
        error_mm: extents.error_mm,
        status: if unmeasured.is_some() { "placeholder" } else { "approximate" },
        provenance: unmeasured.map_or("display_mesh", Unmeasured::name),
    }
}

/// An exploded choice as inspect shows it and a view saves it: `"assembled"`, or `{"view": id}`.
fn exploded_value(exploded: Exploded) -> Value {
    serde_json::to_value(exploded).unwrap_or(Value::Null)
}

/// A configuration as inspect shows it and a view saves it: `"active"`, or `{"row": id}`.
fn configuration_value(configuration: ViewConfiguration) -> Value {
    serde_json::to_value(configuration).unwrap_or(Value::Null)
}

/// Saved view `view`'s configuration as inspect shows it, where `own` is what the view sets itself, and
/// what it draws from `sessions`: the row its copy is put in, or else the design's active row while the
/// design is still in it, and the features the row fails to build that the design builds
/// ([`crate::drawing_view::ViewCopy`]).
fn configuration_result(sessions: &mut ViewSessions, view: &ResolvedView, own: Option<ViewConfiguration>) -> ConfigurationResult {
    let setting = setting(view.configuration, own, configuration_value);
    let choice = sessions.choice(view);
    // The answer the key already holds for a view of the active row: the design isn't compared again.
    let in_active = choice.row().is_none() && sessions.matches();
    let (drawn, errors) = match sessions.drawing(&choice) {
        Ok((from, failing)) => {
            let row = match choice.row() {
                Some(row) => from.doc.configs.row(RowKey::Id(row)),
                None => from.doc.configs.active_row().filter(|_| in_active),
            };
            (row.map(|row| DrawnRowResult { id: row.id, name: row.name.clone() }), failing.to_vec())
        }
        Err(_) => (None, Vec::new()),
    };
    ConfigurationResult { setting, drawn, errors }
}

/// `chosen` as a view saves it, for `cmd`: active, or a row of the design's configuration table found
/// by its id or its name, as `config.activate` finds one, saved by its id.
fn configuration_of(s: &Session, chosen: &ConfigurationParam, cmd: &str) -> Result<ViewConfiguration> {
    let key = match chosen {
        ConfigurationParam::Active => return Ok(ViewConfiguration::Active),
        ConfigurationParam::Id(id) => RowKey::Id(*id),
        ConfigurationParam::Name(name) => RowKey::Name(name),
    };
    let row = s.doc.configs.row(key).ok_or_else(|| bad(cmd, format!("no configuration `{key}` in the design")))?;
    Ok(ViewConfiguration::Row(row.id))
}

/// `chosen` as a view saves it, for `cmd`: assembled, or an exploded view of the design found by its
/// id or name, as `explode.show` finds one.
fn exploded_of(s: &Session, chosen: &ExplodedParam, cmd: &str) -> Result<Exploded> {
    let views = &s.doc.assembly.exploded;
    let found = match chosen {
        ExplodedParam::Assembled => return Ok(Exploded::Assembled),
        ExplodedParam::Id(id) => views.iter().find(|v| v.id == *id).ok_or_else(|| id.to_string()),
        ExplodedParam::Name(name) => views.iter().find(|v| v.name == *name || v.id.to_string() == *name).ok_or_else(|| name.clone()),
    };
    found.map(|v| Exploded::View(v.id)).map_err(|key| bad(cmd, format!("no exploded view `{key}` in the design")))
}

/// `chosen` as a view saves it, for `cmd`: designed; each joint found by its id or name with its
/// values read as `joint.drive` reads them, then held to the record's limits ([`check_pose`]); or a
/// study found as `motion.play` finds it, at a whole step of its timeline.
fn pose_of(s: &Session, chosen: &PoseParam, cmd: &str) -> Result<Pose> {
    let given = match chosen {
        PoseParam::Designed => return Ok(Pose::Designed),
        PoseParam::Study(at) => return study_step_of(s, at, cmd).map(Pose::Study),
        PoseParam::Joints(given) => given,
    };
    // Bounded before any joint is looked up.
    if given.len() > MAX_POSE_JOINTS {
        return Err(bad(cmd, format!("a position sets at most {MAX_POSE_JOINTS} joints")));
    }
    let mut joints = Vec::with_capacity(given.len());
    for one in given {
        let id = super::joints::joint_id(s, Some(&one.joint), cmd)?;
        let joint = s.doc.assembly.joints.iter().find(|j| j.id == id).ok_or_else(|| bad(cmd, format!("no joint {id}")))?;
        let takes = joint.kind.dofs().len();
        if one.values.len() != takes {
            return Err(bad(cmd, format!("joint `{}` takes {takes} value{}", joint.name, if takes == 1 { "" } else { "s" })));
        }
        // A value's reason, said of its joint.
        let named = |e| match e {
            crate::EngineError::BadParams { msg, .. } => bad(cmd, format!("joint `{}`: {msg}", joint.name)),
            other => other,
        };
        let value = |(i, v)| {
            let x = super::joints::value(s, v, joint.kind.is_angle(i), cmd)?;
            if x.is_finite() { Ok(x) } else { Err(bad(cmd, "a value must be a finite number")) }
        };
        let values = one.values.iter().enumerate().map(|one| value(one).map_err(named));
        joints.push(JointPose { joint: id, values: values.collect::<Result<_>>()? });
    }
    let pose = Pose::Joints(joints.into());
    check_pose(&pose).map_err(|e| bad(cmd, e.to_string()))?;
    Ok(pose)
}

/// `at` as a view saves it, for `cmd`: the study by its id, and a whole step of its timeline.
fn study_step_of(s: &Session, at: &StudyPoseParam, cmd: &str) -> Result<StudyStep> {
    let index = super::motion::study_index(s, &json!({"study": at.study}), cmd)?;
    let study = s.doc.assembly.studies.get(index).ok_or_else(|| bad(cmd, "no such motion study"))?;
    let step = at.step.as_u64().and_then(|step| u32::try_from(step).ok()).filter(|step| *step <= study.steps);
    let step = step.ok_or_else(|| bad(cmd, format!("`step` is a whole step of motion study `{}`: 0 to {}", study.name, study.steps)))?;
    Ok(StudyStep { id: study.id, step })
}

/// A position as inspect shows it and a view saves it: `"designed"`, `{"joints": [...]}`, or
/// `{"study": {"id": .., "step": ..}}`.
fn pose_value(pose: Pose) -> Value {
    serde_json::to_value(pose).unwrap_or(Value::Null)
}

/// Saved view `view`'s position as inspect shows it, where `own` is what the view sets itself; for a
/// position of its own, also what it draws from `sessions`: the joints it sets, or a study keys, at
/// the values they take, the joints `s`'s design no longer has, and the study step it draws.
fn pose_result(s: &Session, sessions: &mut ViewSessions, view: &ResolvedView, own: Option<Pose>) -> PoseResult {
    let setting = setting(view.pose.clone(), own, pose_value);
    let (joints, study): (Vec<u64>, Option<StudyStepResult>) = match &view.pose.value {
        Pose::Designed => return PoseResult { setting, drawn: None, skipped: None, study: None },
        Pose::Joints(joints) => (joints.iter().map(|j| j.joint).collect(), None),
        Pose::Study(at) => match at.drawn(&s.doc.assembly.studies) {
            Some((m, step)) => {
                // Each joint it keys once, as `motion.play` lists them.
                let keyed: BTreeSet<u64> = m.keys.iter().map(|k| k.joint).collect();
                (keyed.into_iter().collect(), Some(StudyStepResult { id: m.id, name: m.name.clone(), step }))
            }
            None => (Vec::new(), None),
        },
    };
    let skipped = crate::drawing_view::gone_joints(s, view);
    let drawn = match sessions.of(view) {
        Ok(posed) => {
            // As the solver places them, after motion links: a screw's travel follows its turn.
            let placed = solidcraft_doc::joints::joint_values(&posed.doc.assembly);
            joints
                .iter()
                .filter_map(|joint| posed.doc.assembly.joints.iter().find(|driven| driven.id == *joint))
                .map(|driven| DrawnJointResult {
                    joint: driven.id,
                    name: driven.name.clone(),
                    values: placed
                        .get(&driven.id)
                        .unwrap_or(&driven.values)
                        .iter()
                        .enumerate()
                        .map(|(i, v)| if driven.kind.is_angle(i) { v.to_degrees() } else { *v })
                        .collect(),
                })
                .collect()
        }
        Err(_) => Vec::new(),
    };
    PoseResult { setting, drawn: Some(drawn), skipped: Some(skipped), study }
}

/// Refuses a drawing in which one of `views` draws its parts apart in a way it can't draw yet
/// ([`crate::drawing_view::not_drawable_apart`]).
fn check_apart(d: &Drawing, views: impl Iterator<Item = u64>, cmd: &str) -> Result<()> {
    for view in views {
        if let Some(why) = d.resolve_view(view).ok().as_ref().and_then(crate::drawing_view::not_drawable_apart) {
            return Err(bad(cmd, why));
        }
    }
    Ok(())
}

/// A resolved setting as inspect shows it: its value, where it comes from and what the view sets.
fn setting<T, U>(resolved: Setting<T>, own: Option<T>, show: impl Fn(T) -> U) -> SettingResult<U> {
    let (from, parent) = match resolved.set_by {
        SetBy::This => (FromResult::This, None),
        SetBy::Parent(id) => (FromResult::Parent, Some(ids::encode(id))),
        SetBy::Sheet => (FromResult::Sheet, None),
        SetBy::Drawing => (FromResult::Drawing, None),
        SetBy::Default => (FromResult::Default, None),
    };
    SettingResult { value: show(resolved.value), from, parent, set: own.map(&show) }
}

/// The views lined up with view `id`, directly or through others: the views that go where it goes.
fn lined_up(d: &Drawing, id: u64) -> Vec<u64> {
    descendants(d, id, |v| matches!(v.place, ViewPlace::Aligned(_)))
}

/// `view`'s place moved by `by`: a point moves, and a place lined up with a parent moves along
/// its free axis, so the views lined up with a moved view stay where they were beside it.
fn shifted(view: &View, by: Vec2) -> ViewPlace {
    match (view.place, view.kind) {
        (ViewPlace::At([x, y]), _) => ViewPlace::At([x + by.x, y + by.y]),
        (ViewPlace::Aligned(along), ViewKind::Projected { side, .. }) => ViewPlace::Aligned(along + side.along(by)),
        (ViewPlace::Aligned(along), ViewKind::Base { .. }) => ViewPlace::Aligned(along + by.y),
    }
}

/// The sheet `named` names in `d` (its id or its one name), and its centre.
fn sheet_named(d: &Drawing, named: &str, cmd: &str) -> Result<(u64, Vec2)> {
    let at = d.sheet_index(named).map_err(|e| bad(cmd, e.to_string()))?;
    let sheet = d.sheets.get(at).ok_or_else(|| bad(cmd, format!("no sheet `{named}`")))?;
    Ok((sheet.id, sheet.sheet.size_mm().map_err(|e| bad(cmd, e.to_string()))? * 0.5))
}

/// `d`'s view `id` placed where it goes dragged to `to` ([`Drawing::place_moved_to`]), and the views
/// that moved: it, then the views lined up with it whose centres changed, which follow by the
/// record's rule.
fn drag(d: &mut Drawing, id: u64, to: Vec2, break_alignment: bool, cmd: &str) -> Result<Vec<u64>> {
    let place = d.place_moved_to(id, to, break_alignment).map_err(|e| bad(cmd, e.to_string()))?;
    let lined = lined_up(d, id);
    let centres = |d: &Drawing| lined.iter().map(|v| d.view_centre(*v).ok()).collect::<Vec<_>>();
    let before = centres(d);
    if let Some(view) = d.view_index(id).and_then(|at| d.views.get_mut(at)) {
        Arc::make_mut(view).place = place;
    }
    let went = lined.iter().zip(before.into_iter().zip(centres(d))).filter(|(_, (b, a))| b != a).map(|(v, _)| *v);
    Ok(std::iter::once(id).chain(went).collect())
}

fn move_view(s: &mut Session, p: &Value) -> Result<Value> {
    let request: ViewMoveParams = crate::schema::read(DRAG, p)?;
    let id = request.view.0;
    let moved = s.edit_drawing(DRAG, |d| drag(d, id, request.to_mm.0, request.break_alignment, DRAG))?;
    let d = s.drawing(DRAG)?;
    let place = find(d, id, DRAG)?.place.into();
    let centre = d.view_centre(id).map_err(|e| bad(DRAG, e.to_string()))?;
    let outside = outside_results(crate::drawing_view::outside(s, d, &moved, DRAG)?);
    let followers = moved.iter().skip(1).map(|v| ids::encode(*v)).collect();
    let reply = ViewMoveResult { view: ids::encode(id), place, centre_mm: [centre.x, centre.y], followers, outside, param_warnings: Vec::new() };
    serde_json::to_value(reply).map_err(|e| crate::EngineError::Other(e.to_string()))
}

fn move_to_sheet(s: &mut Session, p: &Value) -> Result<Value> {
    let request: MoveParams = crate::schema::read(MOVE, p)?;
    let id = request.view.0;
    let (sheet, views) = s.edit_drawing(MOVE, |d| {
        let (sheet, middle) = sheet_named(d, &request.sheet, MOVE)?;
        let view = find(d, id, MOVE)?;
        if view.sheet == sheet {
            // Already on it: a position is a move on its sheet, by the one rule.
            return match request.position_mm {
                Some(to) => Ok((sheet, drag(d, id, to.0, request.break_alignment, MOVE)?)),
                None => Ok((sheet, Vec::new())),
            };
        }
        let lined = matches!(view.place, ViewPlace::Aligned(_));
        if lined && !request.break_alignment {
            return Err(bad(MOVE, "it is lined up with its parent, which stays on its sheet: give `break_alignment: true` to place it apart"));
        }
        let from = d.view_centre(id).map_err(|e| bad(MOVE, e.to_string()))?;
        let to = match request.position_mm {
            Some(p) => p.0,
            None if lined => middle,
            None => from,
        };
        let moved: Vec<u64> = std::iter::once(id).chain(lined_up(d, id)).collect();
        for view in d.views.iter_mut().filter(|v| moved.contains(&v.id)) {
            let place = if view.id == id { ViewPlace::At([to.x, to.y]) } else { shifted(view, to - from) };
            let view = Arc::make_mut(view);
            view.sheet = sheet;
            view.place = place;
        }
        Ok((sheet, moved))
    })?;
    let outside = crate::drawing_view::outside(s, s.drawing(MOVE)?, &views, MOVE)?;
    to_sheet_answer(id, sheet, &views, outside)
}

fn copy_to_sheet(s: &mut Session, p: &Value) -> Result<Value> {
    let request: CopyParams = crate::schema::read(COPY, p)?;
    let id = request.view.0;
    let record = s.drawing(COPY)?.clone();
    find(&record, id, COPY)?;
    let linked = lined_up(&record, id);
    let with_linked = match request.with_linked {
        Some(with) => with,
        None if linked.is_empty() => false,
        None => {
            let names = linked.iter().map(|v| ids::encode(*v)).collect::<Vec<_>>().join(", ");
            return Err(bad(COPY, format!("views {names} are lined up with it: give `with_linked` (true to copy them too, false for it alone)")));
        }
    };
    let originals: Vec<u64> = std::iter::once(id).chain(linked.into_iter().filter(|_| with_linked)).collect();
    // Each view's dimensions, balloons and notes are part of what it shows, so they are copied with it.
    let measured: Vec<u64> = record.dimensions.iter().filter(|d| originals.contains(&d.view)).map(|d| d.id).collect();
    let ballooned: Vec<u64> = record.balloons.iter().filter(|b| originals.contains(&b.view)).map(|b| b.id).collect();
    let noted: Vec<u64> = record.notes.iter().filter(|n| n.anchor.view().is_some_and(|v| originals.contains(&v))).map(|n| n.id).collect();
    let mut copies = (0..originals.len() + measured.len() + ballooned.len() + noted.len())
        .map(|_| record.allocate_id(&mut s.ids))
        .collect::<solidcraft_doc::Result<Vec<u64>>>()
        .map_err(|e| bad(COPY, e.to_string()))?;
    let mut dimension_copies = copies.split_off(originals.len());
    let mut balloon_copies = dimension_copies.split_off(measured.len());
    let note_copies = balloon_copies.split_off(ballooned.len());
    let copy_of = |old: u64| originals.iter().position(|o| *o == old).and_then(|i| copies.get(i)).copied();
    let sheet = s.edit_drawing(COPY, |d| {
        let (sheet, middle) = sheet_named(d, &request.sheet, COPY)?;
        let from = d.view_centre(id).map_err(|e| bad(COPY, e.to_string()))?;
        let to = request.position_mm.map_or(middle, |p| p.0);
        let resolved = d.resolve_view(id).map_err(|e| bad(COPY, e.to_string()))?;
        let mut made = Vec::with_capacity(copies.len());
        for (old, new) in originals.iter().zip(&copies) {
            let original = find(d, *old, COPY)?;
            let mut view = View { id: *new, sheet, ..original.clone() };
            view.place = shifted(original, to - from);
            match original.kind {
                // The copy follows none of the views it came from: a projected view copied alone is
                // a base view, looking the way it did, with the settings it had as its own. Its line
                // group is its own only when its chain gave it: else it follows its new sheet, as a
                // base view copied does.
                ViewKind::Projected { .. } if *old == id => {
                    view.kind = ViewKind::Base { source: resolved.source, view: looking(&resolved.basis) };
                    view.settings = ViewOverrides {
                        scale: Some(resolved.scale.value),
                        visual_style: Some(resolved.visual_style.value),
                        reference_style: Some(resolved.reference_style.value),
                        reference_hides_model: Some(resolved.reference_hides_model.value),
                        tangent_edges: Some(resolved.tangent_edges.value),
                        occurrences: resolved.occurrences.value.clone(),
                        hide: resolved.hide.value.clone(),
                        line_group: matches!(resolved.line_group.set_by, SetBy::This | SetBy::Parent(_)).then_some(resolved.line_group.value),
                        exploded: Some(resolved.exploded.value).filter(|exploded| *exploded != Exploded::Assembled),
                        pose: Some(resolved.pose.value.clone()).filter(|pose| *pose != Pose::Designed),
                        configuration: Some(resolved.configuration.value).filter(|row| *row != ViewConfiguration::Active),
                    };
                    view.place = ViewPlace::At([to.x, to.y]);
                }
                ViewKind::Projected { parent, side } => view.kind = ViewKind::Projected { parent: copy_of(parent).unwrap_or(parent), side },
                ViewKind::Base { .. } => {}
            }
            made.push(Arc::new(view));
        }
        d.add_views(made);
        let copied: Vec<Arc<Dimension>> = measured
            .iter()
            .zip(&dimension_copies)
            .filter_map(|(old, new)| {
                let original = d.dimension(*old)?;
                // A copy shows no model dimension: its original still does, and each shows once.
                Some(Arc::new(Dimension { id: *new, view: copy_of(original.view)?, from_model: None, ..original.clone() }))
            })
            .collect();
        d.add_dimensions(copied);
        let copied: Vec<Arc<Balloon>> = ballooned
            .iter()
            .zip(&balloon_copies)
            .filter_map(|(old, new)| {
                let original = d.balloons.by_id(*old)?;
                Some(Arc::new(Balloon { id: *new, view: copy_of(original.view)?, ..original.clone() }))
            })
            .collect();
        d.balloons.add(copied);
        let copied: Vec<Arc<Note>> = noted
            .iter()
            .zip(&note_copies)
            .filter_map(|(old, new)| {
                let original = d.notes.by_id(*old)?;
                Some(Arc::new(Note { id: *new, anchor: original.anchor.on_view(copy_of(original.anchor.view()?)?), ..original.clone() }))
            })
            .collect();
        d.notes.add(copied);
        Ok(sheet)
    })?;
    let first = copies.first().copied().unwrap_or_default();
    let outside = crate::drawing_view::outside(s, s.drawing(COPY)?, &copies, COPY)?;
    to_sheet_answer(first, sheet, &copies, outside)
}

fn base(s: &mut Session, p: &Value) -> Result<Value> {
    let request: BaseParams = crate::schema::read(BASE, p)?;
    let record = s.drawing(BASE)?.clone();
    let (sheet, middle) = sheet_named(&record, &request.sheet, BASE)?;
    let source = record.sources.first().map(|source| source.id).ok_or_else(|| bad(BASE, "the drawing draws no design"))?;
    let chosen = request.source.as_ref().map(|chosen| chosen.occurrences.as_slice());
    check_paths(s, chosen, request.hide.as_deref().unwrap_or_default(), BASE)?;
    let direction = request.view.unwrap_or_default().0;
    let position = request.position_mm.map_or(middle, |p| p.0);
    let id = record.allocate_id(&mut s.ids).map_err(|e| bad(BASE, e.to_string()))?;
    let settings = ViewOverrides {
        scale: request.scale.map(|scale| scale.0),
        visual_style: request.visual_style.map(|style| style.0),
        reference_style: request.reference_style.map(|style| style.0),
        reference_hides_model: request.reference_hides_model,
        tangent_edges: request.tangent_edges.map(|edges| edges.0),
        occurrences: chosen.map(occurrence_paths),
        hide: request.hide.as_deref().map(occurrence_paths),
        line_group: None,
        exploded: request.exploded.as_ref().map(|chosen| exploded_of(s, chosen, BASE)).transpose()?,
        pose: request.pose.as_ref().map(|chosen| pose_of(s, chosen, BASE)).transpose()?,
        configuration: request.configuration.as_ref().map(|chosen| configuration_of(s, chosen, BASE)).transpose()?,
    };
    let place = ViewPlace::At([position.x, position.y]);
    let kind = ViewKind::Base { source, view: direction };
    let mut view = View { id, sheet, kind, place, settings, reference_shown_percent: request.reference_shown_percent.unwrap_or(0.0) };
    // The record's rules first, at 1:1 unless a scale is given, so a bad direction, percent or place
    // is refused as the record says; then what it shows; then the fitted scale, which is saved. With
    // nothing to measure, 1:1.
    let one = Scale::new(1.0, 1.0).map_err(|e| bad(BASE, e.to_string()))?;
    let mut staged = Drawing::clone(&record);
    staged.add_views([Arc::new(View {
        settings: ViewOverrides { scale: view.settings.scale.or(Some(one)), ..view.settings.clone() },
        ..view.clone()
    })]);
    staged.validate().map_err(|e| bad(BASE, e.to_string()))?;
    check_apart(&staged, std::iter::once(id), BASE)?;
    let resolved = staged.resolve_view(id).map_err(|e| bad(BASE, e.to_string()))?;
    let shown = crate::drawing_view::shown_by(&mut ViewSessions::new(s), &resolved);
    check_shows_bodies(&shown, &resolved, BASE)?;
    let mut fitted = false;
    if view.settings.scale.is_none() {
        let room = Room::on(crate::drawing_view::sheet(&staged, sheet, BASE)?, BASE)?;
        let measured = crate::drawing_view::measured(&shown, &resolved).ok();
        fitted = measured.is_some();
        view.settings.scale = Some(match measured {
            Some((set, extents)) => {
                fit(&extents, crate::drawing_view::shows(&set, &resolved.basis, &extents, view.reference_shown_percent), position, room, BASE)?
            }
            None => one,
        });
    }
    s.edit_drawing(BASE, |d| {
        d.add_views([Arc::new(view)]);
        Ok(())
    })?;
    created(s, BASE, id, fitted)
}

fn projected(s: &mut Session, p: &Value) -> Result<Value> {
    let request: ProjectedParams = crate::schema::read(PROJECTED, p)?;
    let record = s.drawing(PROJECTED)?.clone();
    let (parent, sheet) = find(&record, request.parent.0, PROJECTED).map(|v| (v.id, v.sheet))?;
    let side = request.side.0;
    let id = record.allocate_id(&mut s.ids).map_err(|e| bad(PROJECTED, e.to_string()))?;
    let parent_centre = record.view_centre(parent).map_err(|e| bad(PROJECTED, e.to_string()))?;
    let place = match request.place {
        Some(PlaceParam::At(at)) => ViewPlace::At(at),
        Some(PlaceParam::Aligned(along)) => ViewPlace::Aligned(along),
        None => ViewPlace::Aligned(side.along(parent_centre)),
    };
    let kind = ViewKind::Projected { parent, side };
    let mut view = View { id, sheet, kind, place, settings: ViewOverrides::default(), reference_shown_percent: 0.0 };
    if request.place.is_none() {
        // Staged on its parent's centre to measure it, then moved along the free axis until its
        // boundary is the gap past its parent's.
        let mut staged = Drawing::clone(&record);
        staged.add_views([Arc::new(view.clone())]);
        staged.validate().map_err(|e| bad(PROJECTED, e.to_string()))?;
        let theirs = crate::drawing_view::on_sheet(s, &staged, parent, PROJECTED)?.boundary;
        let ours = crate::drawing_view::on_sheet(s, &staged, id, PROJECTED)?.boundary;
        let half = (side.along(ours.max) - side.along(ours.min)) / 2.0;
        view.place = ViewPlace::Aligned(match side {
            ProjectedSide::Right | ProjectedSide::Above => side.along(theirs.max) + DEFAULT_GAP_MM + half,
            ProjectedSide::Left | ProjectedSide::Below => side.along(theirs.min) - DEFAULT_GAP_MM - half,
        });
    }
    s.edit_drawing(PROJECTED, |d| {
        d.add_views([Arc::new(view)]);
        Ok(())
    })?;
    created(s, PROJECTED, id, false)
}

/// The reply for view `id`, just made by `cmd`: its sheet, its scale (`fitted` when worked out),
/// its boundary, and itself when it leaves its sheet's drawing space.
fn created(s: &Session, cmd: &str, id: u64, fitted: bool) -> Result<Value> {
    let d = s.drawing(cmd)?;
    let sheet_id = find(d, id, cmd)?.sheet;
    let sheet = crate::drawing_view::sheet(d, sheet_id, cmd)?;
    let scale = d.resolve_view(id).map_err(|e| bad(cmd, e.to_string()))?.scale.value;
    let placed = crate::drawing_view::on_sheet(s, d, id, cmd)?;
    let outside = if crate::drawing_view::leaves(sheet, &placed, cmd)? { vec![(id, placed)] } else { Vec::new() };
    let reply = CreateResult {
        view: ids::encode(id),
        sheet: ids::encode(sheet_id),
        scale: ScaleResult { text: scale.to_string(), factor: scale.factor(), fitted },
        boundary_mm: BoxResult { min: placed.boundary.min.into(), max: placed.boundary.max.into() },
        outside: outside_results(outside),
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(|e| crate::EngineError::Other(e.to_string()))
}

fn to_sheet_answer(view: u64, sheet: u64, views: &[u64], outside: Vec<(u64, OnSheet)>) -> Result<Value> {
    let reply = ToSheetResult {
        view: ids::encode(view),
        sheet: ids::encode(sheet),
        views: views.iter().map(|v| ids::encode(*v)).collect(),
        outside: outside_results(outside),
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(|e| crate::EngineError::Other(e.to_string()))
}

/// The named view that is exactly `basis`, if one is.
fn named(basis: &Plane) -> Option<NamedView> {
    NamedView::ALL.iter().copied().find(|v| v.basis() == *basis)
}

/// `basis` as a base view saves it: by name when it is exactly a named view's, so nothing rounds;
/// else its line of sight and up, which give the same basis back to within rounding.
fn looking(basis: &Plane) -> ViewDirection {
    named(basis).map_or_else(
        || ViewDirection::Looking(Looking { direction: basis.y.cross(basis.x).into(), up: basis.y.into(), origin: basis.origin.into() }),
        ViewDirection::Named,
    )
}

/// A basis as the replies give it: its name when it is exactly a named view's.
fn direction(basis: &Plane) -> ViewResult {
    ViewResult {
        named: named(basis).map(snake_name),
        // The line of sight, -(x × y), written so no component reads -0.
        direction: basis.y.cross(basis.x).into(),
        x: basis.x.into(),
        y: basis.y.into(),
        origin: basis.origin.into(),
    }
}

/// The largest preset scale at which what a view shows (model mm), placed by the middle of its model at
/// `position`, lies where `room` says: the room inside the drawing border is twice the distance from
/// the position to the nearer side, each way, for the box round the model's middle that holds what
/// shows; that box at a smaller preset, if need be, is off the title block.
fn fit(extents: &Extents, shown: Aabb2, position: Vec2, room: Room, cmd: &str) -> Result<Scale> {
    let border = room.space;
    let off = shown.center() - extents.model.center();
    let size = shown.size() + Vec2::new(off.x.abs(), off.y.abs()) * 2.0;
    let within =
        Vec2::new(2.0 * (position.x - border.min.x).min(border.max.x - position.x), 2.0 * (position.y - border.min.y).min(border.max.y - position.y));
    let clear = |scale: &Scale| {
        let half = size * scale.factor() / 2.0;
        room.clear_of_block(&Aabb2 { min: position - half, max: position + half })
    };
    let largest = Scale::fit(size, within);
    largest.and_then(|largest| PRESETS.iter().copied().filter(|s| s.factor() <= largest.factor()).find(clear)).ok_or_else(|| {
        let name = |s: Option<&Scale>| s.map(|s| s.to_string()).unwrap_or_default();
        let presets = format!("from {} to {}", name(PRESETS.first()), name(PRESETS.last()));
        let off_block = if room.block.is_some() { ", off its title block," } else { "" };
        bad(
            cmd,
            format!(
                "the view fits the sheet's drawing area{off_block} at no preset scale {presets} here: give a scale, a position or a larger sheet"
            ),
        )
    })
}

/// The picture of a view whose boundary is `rect`, on its pixel grid at the sheet's `dpi` or the
/// most a picture is drawn at, the lower (lines may export finer); refused past the grid's side
/// limit.
fn picture(rect: Aabb2, dpi: f64) -> Result<PictureResult> {
    let dpi = dpi.min(MAX_PICTURE_DPI);
    let grid = Picture { source: 0, rect, opacity: 1.0 }.grid(dpi).map_err(|e| bad(CMD, e.to_string()))?;
    Ok(PictureResult { pixels: [grid.width, grid.height], dpi, rect_mm: BoxResult { min: rect.min.into(), max: rect.max.into() } })
}

/// A unit enum's name as it reads and writes (`iso_front_right`, `visible_hidden`).
pub(super) fn snake_name(value: impl Serialize) -> String {
    serde_json::to_value(value).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

#[cfg(test)]
#[path = "drawing_view_tests.rs"]
pub(super) mod tests;
