//! Cell Grid (`solid.cell_grid`): the sketch's profiles to fill, the bodies to cut, the cell and
//! the grid. The command checks the values. A row for an option the command gains later is one
//! field here, one row in [`rows`] and one key in [`commands`] and [`for_feature`].
use crate::{
    SolveApp,
    dialogs::{Kind, check, shown_expr},
    selection::{BODIES, PROFILES, SelInput, SelectionExt as _},
    ui_kit::{choice_row, expr_row, row},
};
use serde_json::{Map, Value, json};
use solidcore_record::{
    Vec3,
    cell_grid::{CellShape, Clip, DotGrid},
};
use solidcraft_engine::{
    Sel,
    doc::{CellGridMode, Direction, FeatureKind, expr::Kind as ValueKind},
    geom::Vec3Geom as _,
};

/// The command's keywords, in the order of the choice rows.
const SHAPES: [&str; 5] = ["hexagon", "square", "triangle", "circle", "polygon"];
const DOT_GRIDS: [&str; 2] = ["hexagonal", "square"];
const CLIPS: [&str; 3] = ["whole", "snap", "free"];
/// The keep-outs' clip: the first choice leaves `keepout_clip` out (the same as the field's clip),
/// the others are [`CLIPS`] one place on.
const KEEPOUT_CLIPS: [Option<&str>; 4] = [None, Some("whole"), Some("snap"), Some("free")];
/// What the feature does with its cells; the first is the default.
const MODES: [&str; 2] = ["cut", "raised"];
const DIRECTIONS: [&str; 3] = ["positive", "negative", "symmetric"];
/// The inputs, in order: the profiles (their sketch and keep-outs), the bodies, the anchor.
const PROFILES_INPUT: usize = 0;
const BODIES_INPUT: usize = 1;
const ANCHOR_INPUT: usize = 2;
/// A corner, the origin or a snap point (an edge's middle or end, a hole's centre), each picked as
/// a vertex with its point; a sketch point; a construction point.
const ANCHOR_PICKS: crate::selection::Accept = crate::selection::VERTICES
    | crate::selection::ORIGIN
    | crate::selection::SNAP_POINTS
    | crate::selection::POINTS
    | crate::selection::CONSTRUCTION_POINTS;

#[derive(Clone, Debug)]
pub struct CellGrid {
    /// An index into [`MODES`].
    pub mode: usize,
    /// An index into [`SHAPES`].
    pub shape: usize,
    /// Sides of a polygon cell.
    pub sides: String,
    /// An index into [`DOT_GRIDS`]: where circle and polygon cells sit.
    pub dot_grid: usize,
    pub size: String,
    pub web: String,
    pub through_all: bool,
    pub depth: String,
    /// An index into [`DIRECTIONS`].
    pub direction: usize,
    /// An index into [`CLIPS`].
    pub clip: usize,
    /// An index into [`KEEPOUT_CLIPS`]: what a keep-out's edge does to the cells it crosses.
    pub keepout_clip: usize,
    /// With snapped cells, how many whole grid rows each straight field edge moves in.
    pub snap_inset: String,
    /// The clearance to the edges, with whole cells only.
    pub margin: String,
    pub angle: String,
}

impl Default for CellGrid {
    fn default() -> Self {
        Self {
            mode: 0,
            shape: 0,
            sides: "8".into(),
            dot_grid: 0,
            size: "6 mm".into(),
            web: "1.5 mm".into(),
            through_all: false,
            depth: "2 mm".into(),
            direction: 0,
            clip: 1,
            keepout_clip: 0,
            snap_inset: "0".into(),
            margin: "0 mm".into(),
            angle: "0 deg".into(),
        }
    }
}

impl CellGrid {
    /// Raised cells stand `depth` high: the command refuses them through all.
    fn raised(&self) -> bool {
        MODES.get(self.mode) == Some(&"raised")
    }

    /// Circle and polygon cells sit on a dot grid; the other shapes tile the plane.
    fn on_dots(&self) -> bool {
        matches!(SHAPES.get(self.shape), Some(&"circle" | &"polygon"))
    }

    /// The command takes a margin with whole cells only, at the field's edge and at the keep-outs:
    /// snapped and trimmed cells meet the edge.
    fn takes_margin(&self) -> bool {
        CLIPS.get(self.clip) == Some(&"whole") && matches!(KEEPOUT_CLIPS.get(self.keepout_clip), Some(None | Some("whole")))
    }

    /// The command moves the field's edges in by whole rows with snapped cells only.
    fn takes_snap_inset(&self) -> bool {
        CLIPS.get(self.clip) == Some(&"snap")
    }
}

fn inputs() -> Vec<SelInput> {
    vec![
        SelInput::new("ui.profiles", PROFILES, true),
        SelInput::new("ui.bodies", BODIES, true),
        // A short label keeps the form's label column narrow; the tip says what none means.
        SelInput::new("Anchor", ANCHOR_PICKS, false).tip("ui.cell_grid.anchor_tip"),
    ]
}

pub fn start(app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    (id == "solid.cell_grid").then(|| (crate::length_input::defaults(&app.session.doc, Kind::CellGrid(CellGrid::default())), inputs()))
}

pub fn rows(app: &SolveApp, ui: &mut egui::Ui, c: &mut CellGrid, _: &[SelInput]) -> bool {
    choice_row(ui, app.tr("ui.cell_grid.mode"), "cell_grid_mode", &mut c.mode, &[app.tr("ui.cell_grid.cut"), app.tr("ui.cell_grid.raised")]);
    choice_row(
        ui,
        app.tr("Shape"),
        "cell_grid_shape",
        &mut c.shape,
        &[app.tr("ui.cell_grid.hexagon"), app.tr("ui.perforate_square"), app.tr("ui.cell_grid.triangle"), app.tr("ui.circle"), app.tr("ui.polygon")],
    );
    let mut enter = false;
    if SHAPES.get(c.shape) == Some(&"polygon") {
        enter |= expr_row(ui, app.tr("ui.cell_grid.sides"), &mut c.sides);
    }
    if c.on_dots() {
        choice_row(
            ui,
            app.tr("ui.cell_grid.dot_grid"),
            "cell_grid_dot_grid",
            &mut c.dot_grid,
            &[app.tr("ui.cell_grid.hexagonal"), app.tr("ui.perforate_square")],
        );
    }
    enter |= expr_row(ui, app.tr("ui.cell_grid.size"), &mut c.size);
    enter |= expr_row(ui, app.tr("ui.cell_grid.web"), &mut c.web);
    if c.raised() {
        c.through_all = false;
    } else {
        row(ui, app.tr("ui.cell_grid.through_all"), |ui| check(ui, &mut c.through_all));
    }
    if !c.through_all {
        enter |= expr_row(ui, app.tr("ui.depth"), &mut c.depth);
    }
    choice_row(
        ui,
        app.tr("ui.direction"),
        "cell_grid_direction",
        &mut c.direction,
        &[app.tr("ui.cell_grid.positive"), app.tr("ui.cell_grid.negative"), app.tr("ui.symmetric")],
    );
    choice_row(
        ui,
        app.tr("ui.cell_grid.clip"),
        "cell_grid_clip",
        &mut c.clip,
        &[app.tr("ui.cell_grid.whole"), app.tr("ui.snap"), app.tr("ui.cell_grid.free")],
    );
    choice_row(
        ui,
        app.tr("ui.cell_grid.keepout_clip"),
        "cell_grid_keepout_clip",
        &mut c.keepout_clip,
        &[app.tr("ui.cell_grid.same_as_clip"), app.tr("ui.cell_grid.whole"), app.tr("ui.snap"), app.tr("ui.cell_grid.free")],
    );
    if c.takes_snap_inset() {
        enter |= expr_row(ui, app.tr("ui.cell_grid.snap_inset"), &mut c.snap_inset);
    }
    if c.takes_margin() {
        enter |= expr_row(ui, app.tr("Margin"), &mut c.margin);
    }
    enter |= expr_row(ui, app.tr("ui.angle"), &mut c.angle);
    enter
}

pub fn commands(app: &SolveApp, c: &CellGrid, inputs: &[SelInput], extra: &Map<String, Value>) -> Result<Vec<(String, Value)>, String> {
    let picked = crate::selection::items(inputs, PROFILES_INPUT).profiles();
    let Some(sketch) = picked.first().map(|(sketch, _)| *sketch) else { return Err(app.tr("error.cell_grid_profiles").into()) };
    // The command fills one sketch: profiles of a second sketch are refused, not dropped.
    if picked.iter().any(|(s, _)| *s != sketch) {
        return Err(app.tr("error.cell_grid_one_sketch").into());
    }
    let profiles: Vec<usize> = picked.iter().map(|(_, index)| *index).collect();
    let mut p = json!({
        "sketch": sketch,
        "profiles": profiles,
        "shape": SHAPES.get(c.shape).copied().unwrap_or("hexagon"),
        "size": c.size,
        "web": c.web,
        "angle": c.angle,
        "clip": CLIPS.get(c.clip).copied().unwrap_or("snap"),
        "direction": DIRECTIONS.get(c.direction).copied().unwrap_or("positive"),
    });
    if let Value::Object(args) = &mut p {
        if SHAPES.get(c.shape) == Some(&"polygon") {
            args.insert("sides".into(), json!(c.sides));
        }
        if c.on_dots() {
            args.insert("dot_grid".into(), json!(DOT_GRIDS.get(c.dot_grid).copied().unwrap_or("hexagonal")));
        }
        // Cut is the command's default: left out, as before the choice existed.
        if c.raised() {
            args.insert("mode".into(), json!("raised"));
        }
        if c.through_all && !c.raised() {
            args.insert("through_all".into(), json!(true));
        } else {
            args.insert("depth".into(), json!(c.depth));
        }
        if let Some(Some(keepout_clip)) = KEEPOUT_CLIPS.get(c.keepout_clip) {
            args.insert("keepout_clip".into(), json!(keepout_clip));
        }
        // 0 is the command's default: left out, a feature without an inset stays as it was.
        if c.takes_snap_inset() && !matches!(c.snap_inset.trim(), "" | "0") {
            args.insert("snap_inset".into(), json!(c.snap_inset));
        }
        if c.takes_margin() && !c.margin.trim().is_empty() {
            args.insert("margin".into(), json!(c.margin));
        }
        let bodies = crate::selection::items(inputs, BODIES_INPUT).body_names();
        if !bodies.is_empty() {
            args.insert("targets".into(), json!(bodies));
        }
        // Editing: the stored anchor stays as typed (it may follow parameters) while its point is
        // still the one in the field; a new pick replaces it, an empty field is the sketch origin.
        let kept = extra.get("anchor").filter(|kept| stored_point(app, kept).is_some());
        match crate::selection::items(inputs, ANCHOR_INPUT).first() {
            Some(Sel::Vertex { body, point })
                if body.is_empty() && kept.and_then(|k| stored_point(app, k)).is_some_and(|k| k.dist(*point) < 1e-9) =>
            {
                args.insert("anchor".into(), kept.cloned().unwrap_or_default());
            }
            Some(pick) => {
                let point = anchor_point(app, pick).ok_or_else(|| app.tr("error.cell_grid_anchor").to_string())?;
                args.insert("anchor".into(), crate::selection::point_value(point));
            }
            None => {}
        }
        for (k, v) in extra.iter().filter(|(k, _)| *k != "anchor") {
            args.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    Ok(vec![("solid.cell_grid".into(), p)])
}

pub fn for_feature(app: &SolveApp, kind: &FeatureKind) -> Option<(Kind, Vec<SelInput>, Map<String, Value>)> {
    let FeatureKind::CellGrid {
        sketch,
        profiles,
        mode,
        shape,
        sides,
        dot_grid,
        size,
        web,
        angle,
        anchor,
        clip,
        keepout_clip,
        snap_inset,
        margin,
        depth,
        through_all,
        direction,
        targets,
        ..
    } = kind
    else {
        return None;
    };
    let length = |e: &solidcraft_engine::doc::Expr| shown_expr(e, ValueKind::Length);
    let defaults = CellGrid::default();
    let c = CellGrid {
        mode: match mode {
            CellGridMode::Cut => 0,
            CellGridMode::Raised => 1,
        },
        shape: match shape {
            CellShape::Hexagon => 0,
            CellShape::Square => 1,
            CellShape::Triangle => 2,
            CellShape::Circle => 3,
            CellShape::Polygon => 4,
        },
        sides: sides.as_ref().map_or(defaults.sides, |e| shown_expr(e, ValueKind::Unitless)),
        dot_grid: usize::from(*dot_grid == DotGrid::Square),
        size: length(size),
        web: length(web),
        through_all: *through_all,
        depth: depth.as_ref().map_or(defaults.depth, length),
        direction: match direction {
            Direction::Positive => 0,
            Direction::Negative => 1,
            Direction::Symmetric => 2,
        },
        clip: match clip {
            Clip::Whole => 0,
            Clip::Snap => 1,
            Clip::Free => 2,
        },
        keepout_clip: match keepout_clip {
            None => 0,
            Some(Clip::Whole) => 1,
            Some(Clip::Snap) => 2,
            Some(Clip::Free) => 3,
        },
        snap_inset: snap_inset.as_ref().map_or(defaults.snap_inset, |e| shown_expr(e, ValueKind::Unitless)),
        margin: margin.as_ref().map_or(defaults.margin, length),
        angle: angle.as_ref().map_or(defaults.angle, |e| shown_expr(e, ValueKind::Angle)),
    };
    let mut inputs = inputs();
    let state = crate::open_meshing::session_world(&app.session);
    if let Some(input) = inputs.get_mut(PROFILES_INPUT) {
        let items = state.sketch(sketch.0).map(|ss| crate::dialogs::profile_indices(ss, profiles)).unwrap_or_default();
        input.items = items.into_iter().map(|index| Sel::Profile { sketch: sketch.0, index }).collect();
    }
    if let Some(input) = inputs.get_mut(BODIES_INPUT) {
        input.items = targets.iter().map(|name| Sel::Body { name: name.to_string() }).collect();
    }
    let mut extra = Map::new();
    // The anchor may follow parameters: it is kept as typed, and the field shows its point, so
    // that clearing the field goes back to the sketch origin.
    if let Some(anchor) = anchor {
        let typed = json!(anchor.iter().map(length).collect::<Vec<_>>());
        if let (Some(input), Some(point)) = (inputs.get_mut(ANCHOR_INPUT), stored_point(app, &typed)) {
            input.items = vec![Sel::Vertex { body: String::new(), point }];
        }
        extra.insert("anchor".into(), typed);
    }
    Some((Kind::CellGrid(c), inputs, extra))
}

/// Where an anchor pick is: a vertex comes with its point; a sketch point and a construction
/// point are looked up in the model.
fn anchor_point(app: &SolveApp, pick: &Sel) -> Option<Vec3> {
    let world = crate::open_meshing::session_world(&app.session);
    match pick {
        Sel::Vertex { point, .. } => Some(*point),
        Sel::SketchPoint { id } => crate::canvas::sketch_point_at(app, &world, id),
        Sel::ConstructionPoint { name } => {
            let c = world.construct.iter().find(|c| c.name == *name)?;
            let solidcraft_engine::doc::construct::ConstructGeom::Point(p) = c.geom else { return None };
            // A point of a component's construction sits where that component's occurrence is.
            let component = app.session.doc.feature(c.feature).map_or(0, |f| f.component);
            let occurrence = if component == 0 { None } else { app.session.doc.occurrence_of(component) };
            Some(
                occurrence
                    .map_or(p, |o| solidcraft_engine::doc::apply_point(&solidcraft_engine::doc::joints::occurrence_world(&app.session.doc, o.id), p)),
            )
        }
        _ => None,
    }
}

/// The point of a stored anchor, `[x, y, z]` as typed.
fn stored_point(app: &SolveApp, typed: &Value) -> Option<Vec3> {
    let coord = |i: usize| crate::length_input::eval(&app.session.doc, typed.get(i)?.as_str()?, ValueKind::Length).ok();
    Some(Vec3::new(coord(0)?, coord(1)?, coord(2)?))
}
