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
use solidcore_record::cell_grid::{CellShape, Clip, DotGrid};
use solidcraft_engine::{
    Sel,
    doc::{Direction, FeatureKind, expr::Kind as ValueKind},
};

/// The command's keywords, in the order of the choice rows.
const SHAPES: [&str; 5] = ["hexagon", "square", "triangle", "circle", "polygon"];
const DOT_GRIDS: [&str; 2] = ["hexagonal", "square"];
const CLIPS: [&str; 3] = ["whole", "snap", "free"];
const DIRECTIONS: [&str; 3] = ["positive", "negative", "symmetric"];
/// The inputs, in order: the profiles (their sketch and keep-outs), the bodies, the anchor.
const PROFILES_INPUT: usize = 0;
const BODIES_INPUT: usize = 1;
const ANCHOR_INPUT: usize = 2;
const ANCHOR_PICKS: crate::selection::Accept = crate::selection::VERTICES | crate::selection::SNAP_POINTS;

#[derive(Clone, Debug)]
pub struct CellGrid {
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
    /// The clearance to the edges, with whole cells only.
    pub margin: String,
    pub angle: String,
}

impl Default for CellGrid {
    fn default() -> Self {
        Self {
            shape: 0,
            sides: "8".into(),
            dot_grid: 0,
            size: "6 mm".into(),
            web: "1.5 mm".into(),
            through_all: false,
            depth: "2 mm".into(),
            direction: 0,
            clip: 1,
            margin: "0 mm".into(),
            angle: "0 deg".into(),
        }
    }
}

impl CellGrid {
    /// Circle and polygon cells sit on a dot grid; the other shapes tile the plane.
    fn on_dots(&self) -> bool {
        matches!(SHAPES.get(self.shape), Some(&"circle" | &"polygon"))
    }
}

fn inputs() -> Vec<SelInput> {
    vec![
        SelInput::new("ui.cell_grid.profiles", PROFILES, true),
        SelInput::new("ui.cell_grid.bodies", BODIES, true),
        SelInput::new("ui.cell_grid.anchor", ANCHOR_PICKS, false),
    ]
}

pub fn start(app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    (id == "solid.cell_grid").then(|| (crate::length_input::defaults(&app.session.doc, Kind::CellGrid(CellGrid::default())), inputs()))
}

pub fn rows(app: &SolveApp, ui: &mut egui::Ui, c: &mut CellGrid, _: &[SelInput]) -> bool {
    choice_row(
        ui,
        app.tr("ui.cell_grid.shape"),
        "cell_grid_shape",
        &mut c.shape,
        &[app.tr("ui.cell_grid.hexagon"), app.tr("ui.cell_grid.square"), app.tr("ui.cell_grid.triangle"), app.tr("ui.circle"), app.tr("ui.polygon")],
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
            &[app.tr("ui.cell_grid.hexagonal"), app.tr("ui.cell_grid.square")],
        );
    }
    enter |= expr_row(ui, app.tr("ui.cell_grid.size"), &mut c.size);
    enter |= expr_row(ui, app.tr("ui.cell_grid.web"), &mut c.web);
    row(ui, app.tr("ui.cell_grid.through_all"), |ui| check(ui, &mut c.through_all));
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
    // The command takes a margin with whole cells only: snapped and trimmed cells meet the edge.
    if CLIPS.get(c.clip) == Some(&"whole") {
        enter |= expr_row(ui, app.tr("ui.cell_grid.margin"), &mut c.margin);
    }
    enter |= expr_row(ui, app.tr("ui.angle"), &mut c.angle);
    enter
}

pub fn commands(app: &SolveApp, c: &CellGrid, inputs: &[SelInput], extra: &Map<String, Value>) -> Result<Vec<(String, Value)>, String> {
    let picked = crate::selection::items(inputs, PROFILES_INPUT).profiles();
    let Some(sketch) = picked.first().map(|(sketch, _)| *sketch) else { return Err(app.tr("error.cell_grid_profiles").into()) };
    let profiles: Vec<usize> = picked.iter().filter(|(s, _)| *s == sketch).map(|(_, index)| *index).collect();
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
        if c.through_all {
            args.insert("through_all".into(), json!(true));
        } else {
            args.insert("depth".into(), json!(c.depth));
        }
        if CLIPS.get(c.clip) == Some(&"whole") && !c.margin.trim().is_empty() {
            args.insert("margin".into(), json!(c.margin));
        }
        let bodies = crate::selection::items(inputs, BODIES_INPUT).body_names();
        if !bodies.is_empty() {
            args.insert("targets".into(), json!(bodies));
        }
        if let Some(Sel::Vertex { point, .. }) = crate::selection::items(inputs, ANCHOR_INPUT).first() {
            args.insert("anchor".into(), crate::selection::point_value(*point));
        }
        // Editing: the stored anchor and targets stay unless new ones are picked.
        for (k, v) in extra {
            args.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    Ok(vec![("solid.cell_grid".into(), p)])
}

pub fn for_feature(app: &SolveApp, kind: &FeatureKind) -> Option<(Kind, Vec<SelInput>, Map<String, Value>)> {
    let FeatureKind::CellGrid {
        sketch,
        profiles,
        shape,
        sides,
        dot_grid,
        size,
        web,
        angle,
        anchor,
        clip,
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
        margin: margin.as_ref().map_or(defaults.margin, length),
        angle: angle.as_ref().map_or(defaults.angle, |e| shown_expr(e, ValueKind::Angle)),
    };
    let mut inputs = inputs();
    let state = app.session.model.state();
    if let Some(input) = inputs.get_mut(PROFILES_INPUT) {
        let items = state.sketch(sketch.0).map(|ss| crate::dialogs::profile_indices(ss, profiles)).unwrap_or_default();
        input.items = items.into_iter().map(|index| Sel::Profile { sketch: sketch.0, index }).collect();
    }
    if let Some(input) = inputs.get_mut(BODIES_INPUT) {
        input.items = targets.iter().map(|name| Sel::Body { name: name.to_string() }).collect();
    }
    let mut extra = Map::new();
    // The anchor may follow parameters: it is kept as typed, not as a picked point.
    if let Some(anchor) = anchor {
        extra.insert("anchor".into(), json!(anchor.iter().map(length).collect::<Vec<_>>()));
    }
    Some((Kind::CellGrid(c), inputs, extra))
}
