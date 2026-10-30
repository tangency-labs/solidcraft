//! Tool windows for sketch-space modify commands, on the `dialogs_surface` pattern (#19): one
//! `Mod` enum behind one `Kind::SketchModify` variant, grown by one variant and one arm per
//! command rather than by a new module each time. Scale came first, whose toolbar button
//! reached no tool and no dialog at all, so a press ran `sketch.scale` with no parameters and
//! the engine answered "`factor` must be a positive number". Offset joins it here (upstream
//! issue #162 item 1: "Offset does not create a constraint, the size of the offset seems
//! random"), and it now offers the same One Side, Two Sides and Symmetric directions
//! `solid.extrude` already names. Mirror joins them here, which ran as a bare click flow: entities until a
//! right-click, then one more click for the line, with nothing on screen saying what was
//! picked. Move joins them too, which had no toolbar placement, no tool and no dialog: its only
//! route into the UI was the sketch context menu, and that went through the same
//! empty-parameter fallback and reported an engine error. `Mod::Corner` joins them here,
//! serving `sketch.fillet` and the three `sketch.chamfer.*` commands together, the way
//! `solid.fillet` and `solid.chamfer` already share one window. Before it, a click committed a
//! size guessed at 20 percent of the shorter leg and then opened a bare parameter editor, so
//! the design changed before any number was asked for. `sketch.chamfer.distance_angle` sent a
//! hard-coded 45 degrees and `sketch.chamfer.two_distance` forced its second distance equal to
//! the first, leaving both values unreachable from the UI. The rectangular pattern joins them
//! too, whose click tool sent a hard-coded count of 3 and could reach neither the command's
//! second count nor either spacing, so a user who wanted five instances got three and deleted
//! two. The circular pattern is the same gap and joins this module later.
//!
//! The equation curve (#8352) has no picks: its window is the form for the formulas, the range and
//! the tolerance, and reopens on an existing equation curve to edit them.
//!
//! The Chamfer Type choice is a row of icon buttons rather than a drop-down: the modes differ
//! in shape, and a picture of each shape names it better than a line of text.

use egui::Pos2;
use serde_json::{Map, Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::Document;
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::geom::{Vec2, Vec3};
use solidcraft_engine::sketch::PatternAlignment;

use crate::SolveApp;
use crate::dialogs::{CHAMFER_TYPES, Dialog, Kind};
use crate::selection::{AXES, CURVES, SKETCH_POINTS, SelInput};
use crate::tool_modifiers::{Audit, Scope, ToolModifiers};
use crate::ui_kit;
use crate::viewport::{Hit, Proj};

const SELECT_MANIP: &[Scope] = &[Scope::Selection, Scope::Manipulator, Scope::FeaturePreview];
const SELECT_ONLY: &[Scope] = &[Scope::Selection, Scope::FeaturePreview];

/// Per-tool modifier descriptors owned by this host file.
pub const MODIFIERS: &[ToolModifiers] = &[
    ToolModifiers { tool: "sketch.scale", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.offset", scopes: SELECT_MANIP, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.mirror", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    // Sketch Move: dialog/manipulator inherit only - no invented Alt-copy.
    ToolModifiers { tool: "sketch.move", scopes: SELECT_MANIP, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.transform", scopes: SELECT_MANIP, bindings: &[], audit: Audit::VerifiedNoOp },
    // Pattern spacing/count live in the dialog - no viewport modifier chords.
    ToolModifiers { tool: "sketch.pattern.rectangular", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.pattern.circular", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.fillet", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.chamfer.equal_distance", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.chamfer.two_distance", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.chamfer.distance_angle", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
    // Formulas only: no picks and no viewport modifier chords.
    ToolModifiers { tool: "sketch.equation_curve", scopes: SELECT_ONLY, bindings: &[], audit: Audit::VerifiedNoOp },
];

/// The equation curve's two forms: x(t) and y(t), or y = f(x).
pub(crate) const EQUATION_FORMS: [&str; 2] = ["Parametric", "Explicit"];

/// The three chamfer types, drawn as the cut each one makes, in `CHAMFER_TYPES` order.
const CHAMFER_TYPE_ICONS: [&str; 3] = ["sketch_chamfer_equal", "sketch_chamfer_two", "sketch_chamfer_angle"];

/// The engine's own cap on the copies one sketch pattern may make.
const MAX_COPIES: f64 = 500.0;

/// The offset directions, in the words and the order `solid.extrude` already uses for them.
/// One Side offsets the way the distance points. Two Sides adds an independent distance the
/// other way. Symmetric applies the one distance each way.
pub(crate) const OFFSET_MODES: [&str; 3] = ["One Side", "Two Sides", "Symmetric"];
const OFFSET_MODE_ICONS: [&str; 3] = ["offset_one_side", "offset_two_sides", "offset_symmetric"];
/// Index of Two Sides in [`OFFSET_MODES`] (the mode with a second distance).
const TWO_SIDES: usize = 1;
/// Index of Symmetric in [`OFFSET_MODES`].
const SYMMETRIC: usize = 2;

/// Whether an offset mode index is Symmetric (which hides Flip and offsets both ways).
pub(crate) fn offset_symmetric(mode: usize) -> bool {
    mode == SYMMETRIC
}

/// Whether an offset mode index is Two Sides (Distance and an independent Distance 2).
pub(crate) fn offset_two_sides(mode: usize) -> bool {
    mode == TWO_SIDES
}

/// A sketch modify tool window's values.
#[derive(Clone, Debug, PartialEq)]
pub enum Mod {
    /// `sketch.scale`: the factor the picked entities scale by. The base point is the second
    /// input; with none picked the command scales about the sketch origin, as its own default
    /// already does.
    Scale { factor: String },
    /// `sketch.offset`: the curves to offset, the distance typed and whether the connected
    /// chain follows. A positive distance is left of the chain, as the command defines it;
    /// `flip` reverses whatever sign the field holds. `mode` indexes [`OFFSET_MODES`], and
    /// `distance2` is the other side's distance that Two Sides asks for.
    Offset { distance: String, distance2: String, mode: usize, chain: bool, flip: bool },
    /// `sketch.mirror`: the entities and the line they reflect in. The command has no number of
    /// its own, so the window's whole job is to show what is picked and let a pick be changed
    /// before anything is committed.
    Mirror,
    /// `sketch.move`: how far the picked entities move along the sketch's own axes, how far they
    /// turn about the centre, and whether the originals stay behind. The command already
    /// carries the copy flag, so the Create Copy row needs no engine change.
    Move { x: String, y: String, angle: String, copy: bool },
    /// `sketch.transform`: the picked entities scaled, turned and moved as one, about the middle
    /// of their box. `x` and `y` move that middle, `factor` scales and `angle` turns about it.
    /// `frame` is the box (middle and size in the sketch, kept up to date from the picks by
    /// `sketch_transform_ui`) the window's size rows and the handles work on.
    Transform { x: String, y: String, factor: String, angle: String, frame: Option<([f64; 2], [f64; 2])> },
    /// `sketch.pattern.rectangular`: how many instances along the picked direction and how far
    /// apart, then the same along a second picked direction (perpendicular by default).
    PatternRect {
        count: String,
        spacing: String,
        count2: String,
        spacing2: String,
        alignment: PatternAlignment,
        alignment2: PatternAlignment,
        reversed: bool,
        reversed2: bool,
    },
    /// `sketch.pattern.circular`: how many instances around the picked centre, and the total
    /// angle they span.
    PatternCirc { count: String, angle: String, alignment: PatternAlignment, reversed: bool },
    /// `sketch.fillet` and the three `sketch.chamfer.*` commands. `chamfer` chooses between a
    /// fillet and a chamfer and never changes while the window is open; `ctype` chooses which
    /// of the three chamfer commands runs (0 equal distance, 1 two distances, 2 distance and
    /// angle). `size` is the radius or the first distance, `distance2` the second distance and
    /// `angle` the angle from the first line. `flip` names the two lines in the other order, so
    /// the first distance lands on the other leg of the corner.
    Corner { chamfer: bool, ctype: usize, size: String, distance2: String, angle: String, flip: bool },
    /// `sketch.equation_curve` (or `.edit` on the curve the window's `curve` extra names): the
    /// form (an index into [`EQUATION_FORMS`]), x(t) and y(t) (or y(x)), the variable's range and
    /// how far the curve may stray from the formulas.
    Equation { form: usize, x: String, y: String, start: String, end: String, tolerance: String },
}

impl Mod {
    pub fn previews(&self) -> bool {
        matches!(self, Self::Mirror | Self::PatternRect { .. } | Self::PatternCirc { .. } | Self::Offset { .. } | Self::Corner { .. })
    }

    pub fn title(&self) -> &'static str {
        match self {
            Mod::Scale { .. } => "SKETCH SCALE",
            Mod::Offset { .. } => "OFFSET",
            Mod::Mirror => "MIRROR",
            Mod::Move { .. } => "MOVE/COPY",
            Mod::Transform { .. } => "TRANSFORM",
            Mod::PatternRect { .. } => "RECTANGULAR PATTERN",
            Mod::PatternCirc { .. } => "CIRCULAR PATTERN",
            Mod::Corner { chamfer: false, .. } => "FILLET",
            Mod::Corner { .. } => "CHAMFER",
            Mod::Equation { .. } => "EQUATION CURVE",
        }
    }

    /// The value on the canvas.
    pub fn primary(&mut self) -> Option<(&'static str, ValueKind, &mut String)> {
        match self {
            Mod::Scale { factor } => Some(("Factor", ValueKind::Unitless, factor)),
            Mod::Offset { distance, .. } => Some(("Distance", ValueKind::Length, distance)),
            Mod::Move { x, .. } => Some(("X Distance", ValueKind::Length, x)),
            // The handles in the viewport are its manipulator; the window has the numbers.
            Mod::Transform { .. } => None,
            Mod::Mirror => None,
            Mod::PatternRect { spacing, .. } => Some(("Spacing", ValueKind::Length, spacing)),
            Mod::PatternCirc { angle, .. } => Some(("Angle", ValueKind::Angle, angle)),
            Mod::Corner { chamfer: false, size, .. } => Some(("Radius", ValueKind::Length, size)),
            Mod::Corner { size, .. } => Some(("Distance", ValueKind::Length, size)),
            Mod::Equation { .. } => None,
        }
    }
}

fn inputs(k: &Mod) -> Vec<SelInput> {
    match k {
        Mod::Scale { .. } => vec![SelInput::new("Entities", CURVES | SKETCH_POINTS, true), SelInput::new("Base point", SKETCH_POINTS, false)],
        Mod::Offset { .. } => vec![SelInput::new("Curves", CURVES, true)],
        // AXES is the mask that takes a straight sketch curve (`selection::SelInput::accepts`).
        Mod::Mirror => vec![SelInput::new("Entities", CURVES | SKETCH_POINTS, true), SelInput::new("Mirror line", AXES, false)],
        Mod::Move { .. } => vec![SelInput::new("Entities", CURVES | SKETCH_POINTS, true), SelInput::new("Center", SKETCH_POINTS, false)],
        Mod::Transform { .. } => vec![SelInput::new("Entities", CURVES | SKETCH_POINTS, true)],
        // The direction is optional: `sketch.pattern.rectangular` documents sketch x as its
        // default, the way the engine reads it.
        Mod::PatternRect { .. } => vec![
            SelInput::new("Entities", CURVES | SKETCH_POINTS, true),
            SelInput::new("Direction", AXES, false),
            SelInput::new("Direction 2", AXES, false),
        ],
        // The centre is required: `sketch.pattern.circular` has no default for it.
        Mod::PatternCirc { .. } => vec![SelInput::new("Entities", CURVES | SKETCH_POINTS, true), SelInput::new("Center", SKETCH_POINTS, false)],
        // A fillet rounds any number of corners to one radius, and a chamfer cuts any number
        // alike (`points`). Two lines name the corner between them where more lines meet at its
        // point (#1824), for both.
        Mod::Corner { .. } => vec![SelInput::new("Corners", SKETCH_POINTS | CURVES, true)],
        Mod::Equation { .. } => Vec::new(),
    }
}

/// The dialog for a sketch modify command.
pub fn start(app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    let corner = |chamfer: bool, ctype: usize| Mod::Corner {
        chamfer,
        ctype,
        size: crate::dialogs::length_default(&app.session, "2"),
        distance2: crate::dialogs::length_default(&app.session, "2"),
        angle: "45 deg".into(),
        flip: false,
    };
    let k = match id {
        "sketch.scale" => Mod::Scale { factor: "2".into() },
        "sketch.offset" => Mod::Offset {
            distance: crate::dialogs::length_default(&app.session, "2"),
            distance2: crate::dialogs::length_default(&app.session, "2"),
            mode: 0,
            chain: true,
            flip: false,
        },
        "sketch.mirror" => Mod::Mirror,
        "sketch.transform" => Mod::Transform {
            x: crate::dialogs::length_default(&app.session, "0"),
            y: crate::dialogs::length_default(&app.session, "0"),
            factor: "1".into(),
            angle: "0 deg".into(),
            frame: None,
        },
        "sketch.move" => Mod::Move {
            x: crate::dialogs::length_default(&app.session, "0"),
            y: crate::dialogs::length_default(&app.session, "0"),
            angle: "0 deg".into(),
            copy: false,
        },
        "sketch.pattern.rectangular" => Mod::PatternRect {
            count: "3".into(),
            spacing: crate::dialogs::length_default(&app.session, "20"),
            count2: "1".into(),
            spacing2: crate::dialogs::length_default(&app.session, "20"),
            alignment: PatternAlignment::Right,
            alignment2: PatternAlignment::Right,
            reversed: false,
            reversed2: false,
        },
        "sketch.pattern.circular" => {
            Mod::PatternCirc { count: "6".into(), angle: "360 deg".into(), alignment: PatternAlignment::Right, reversed: false }
        }
        "sketch.fillet" => corner(false, 0),
        "sketch.chamfer.equal_distance" => corner(true, 0),
        "sketch.chamfer.two_distance" => corner(true, 1),
        "sketch.chamfer.distance_angle" => corner(true, 2),
        // A sine wave to start from: one period, 40 wide and 10 high in the design's unit.
        "sketch.equation_curve" => Mod::Equation {
            form: 0,
            x: "20 * t / pi".into(),
            y: "5 * sin(t)".into(),
            start: "0".into(),
            end: "2 * pi".into(),
            tolerance: "0.01 mm".into(),
        },
        _ => return None,
    };
    let i = inputs(&k);
    Some((Kind::SketchModify(k), i))
}

/// Reopen the equation curve window on curve `id` of the sketch being edited, with its formulas.
/// OK runs `sketch.equation_curve.edit` on it.
pub fn edit_equation(app: &mut SolveApp, id: &str) -> bool {
    let Some(sid) = app.session.active_sketch else { return false };
    let state = app.session.world_state();
    let Some(eq) = state.sketch(sid).and_then(|s| s.sketch.equation_of(id)).cloned() else { return false };
    let kind = Mod::Equation {
        form: usize::from(eq.x.is_none()),
        x: eq.x.clone().unwrap_or_else(|| "t".into()),
        y: eq.y,
        start: eq.start,
        end: eq.end,
        tolerance: format!("{} mm", eq.tolerance),
    };
    let mut dialog = Dialog::new(Kind::SketchModify(kind), Vec::new());
    dialog.extra.insert("curve".into(), json!(id));
    app.tool = None;
    app.dialog = Some(dialog);
    true
}

/// The equation curve the selection is, when it is one curve that has formulas.
pub(crate) fn selected_equation(app: &SolveApp) -> Option<String> {
    let [Sel::SketchCurve { id, .. }] = app.session.selection.as_slice() else { return None };
    let sid = app.session.active_sketch?;
    let state = app.session.world_state();
    state.sketch(sid)?.sketch.equation_of(id).map(|e| e.curve.clone())
}

/// Sketch modify inputs accept only the sketch being edited. Origin axes remain valid
/// rectangular-pattern directions, while Hole continues using its separate POINTS mask.
pub fn candidate(d: &Dialog, session: &solidcraft_engine::Session, hit: &Hit) -> Option<Sel> {
    let active = session.active_sketch?;
    match hit {
        Hit::SketchCurve { sketch, .. } | Hit::SketchPoint { sketch, .. } if *sketch != active => None,
        Hit::SketchCurve { id, .. } if matches!(d.kind, Kind::SketchModify(Mod::Corner { .. })) => {
            let state = session.world_state();
            let sk = &state.sketch(active)?.sketch;
            if sk.is_spatial_corner_curve(id) {
                return None;
            }
            sk.curve_index(id)?;
            d.active_input()?.accepts(hit, Some(active), |_, _| false)
        }
        _ => d.active_input()?.accepts(hit, Some(active), |_, _| false),
    }
}

/// Reopen the same pattern window with its stored parameters. OK reaches the same engine
/// command with the pattern id, so previews and programmatic edits share the implementation.
pub fn edit_pattern(app: &mut SolveApp, id: &str) -> bool {
    use solidcraft_engine::sketch::SketchPatternKind;
    let Some(sid) = app.session.active_sketch else { return false };
    let state = app.session.world_state();
    let Some(sk) = state.sketch(sid).map(|s| &s.sketch) else { return false };
    let Some(pattern) = sk.patterns.iter().find(|p| p.id == id) else { return false };
    let input = |key: &str, fallback: String| pattern.expressions.get(key).cloned().unwrap_or(fallback);
    let kind = match &pattern.kind {
        SketchPatternKind::Circular { count, angle, alignment, reversed, .. } => Mod::PatternCirc {
            count: pattern.count_expression.clone().unwrap_or_else(|| input("count", count.to_string())),
            angle: input("angle", format!("{angle} deg")),
            alignment: if *alignment == PatternAlignment::Left { PatternAlignment::Right } else { *alignment },
            reversed: *reversed ^ (*alignment == PatternAlignment::Left),
        },
        SketchPatternKind::Rectangular { count, spacing, count2, spacing2, alignment, alignment2, reversed, reversed2, .. } => Mod::PatternRect {
            count: input("count", count.to_string()),
            spacing: input("spacing", format!("{spacing} mm")),
            count2: input("count2", count2.to_string()),
            spacing2: input("spacing2", format!("{spacing2} mm")),
            alignment: if *alignment == PatternAlignment::Left { PatternAlignment::Right } else { *alignment },
            alignment2: if *alignment2 == PatternAlignment::Left { PatternAlignment::Right } else { *alignment2 },
            reversed: *reversed ^ (*alignment == PatternAlignment::Left),
            reversed2: *reversed2 ^ (*alignment2 == PatternAlignment::Left),
        },
    };
    let mut dialog = Dialog::new(Kind::SketchModify(kind.clone()), inputs(&kind));
    if let Some(input) = dialog.inputs.first_mut() {
        input.items =
            pattern
                .entities
                .iter()
                .map(|id| {
                    if sk.curve_index(id).is_some() { Sel::SketchCurve { id: id.clone(), sketch: None } } else { Sel::SketchPoint { id: id.clone() } }
                })
                .collect();
    }
    match &pattern.kind {
        SketchPatternKind::Circular { center, .. } => {
            if let Some(input) = dialog.inputs.get_mut(1) {
                input.items = vec![Sel::SketchPoint { id: center.clone() }];
            }
        }
        SketchPatternKind::Rectangular { dir, dir2, direction, direction2, .. } => {
            dialog.extra.insert("dir".into(), json!(dir));
            dialog.extra.insert("dir2".into(), json!(dir2));
            for (index, id) in [(1, direction), (2, direction2)] {
                if let Some(id) = id
                    && let Some(input) = dialog.inputs.get_mut(index)
                {
                    input.items = vec![Sel::SketchCurve { id: id.clone(), sketch: None }];
                }
            }
        }
    }
    dialog.extra.insert("pattern".into(), json!(id));
    app.tool = None;
    app.dialog = Some(dialog);
    true
}

pub fn double_click_pattern(app: &mut SolveApp, pos: Pos2) -> bool {
    let Some(glyph) = crate::sketch_tools::glyph_at(pos) else { return false };
    let Some(sid) = app.session.active_sketch else { return false };
    let id = app
        .session
        .world_state()
        .sketch(sid)
        .and_then(|ss| ss.sketch.patterns.iter().find(|p| p.instances.iter().any(|i| i.constraints.contains(&glyph))).map(|p| p.id.clone()));
    id.is_some_and(|id| edit_pattern(app, &id))
}

/// Place a circular pattern centre in empty sketch space without creating a document point.
pub fn pick_position(d: &mut Dialog, session: &solidcraft_engine::Session, point: Vec3) -> bool {
    if !matches!(d.kind, Kind::SketchModify(Mod::PatternCirc { .. })) || d.active != 1 {
        return false;
    }
    d.pick(session, Sel::Vertex { body: String::new(), point });
    true
}

/// Command metadata for the active variant, including an explicitly chosen chamfer type.
pub(crate) fn command(k: &Mod) -> &'static str {
    match k {
        Mod::Scale { .. } => "sketch.scale",
        Mod::Offset { .. } => "sketch.offset",
        Mod::Mirror => "sketch.mirror",
        Mod::Move { .. } => "sketch.move",
        Mod::Transform { .. } => "sketch.transform",
        Mod::PatternRect { .. } => "sketch.pattern.rectangular",
        Mod::PatternCirc { .. } => "sketch.pattern.circular",
        Mod::Corner { chamfer: false, .. } => "sketch.fillet",
        Mod::Corner { ctype: 1, .. } => "sketch.chamfer.two_distance",
        Mod::Corner { ctype: 2, .. } => "sketch.chamfer.distance_angle",
        Mod::Corner { .. } => "sketch.chamfer.equal_distance",
        Mod::Equation { .. } => "sketch.equation_curve",
    }
}

/// Repeatable operations offer Apply for another selection, with the same typed values.
pub(crate) fn repeatable(k: &Mod) -> bool {
    matches!(k, Mod::Offset { .. } | Mod::Mirror | Mod::PatternRect { .. } | Mod::PatternCirc { .. })
}

/// A labelled value row. The value carries its own unit text ("2 mm", "45 deg").
fn number(ui: &mut egui::Ui, label: &str, value: &mut String) -> bool {
    ui_kit::row(ui, label, |ui| ui_kit::field(ui, value))
}

/// A pattern quantity row with step arrows. They keep the count from `min` up to `max`
/// copies, evaluated against the design's parameters as `whole` evaluates it on OK.
fn quantity(ui: &mut egui::Ui, doc: &Document, label: &str, value: &mut String, min: i64, max: i64) -> bool {
    let eval = |text: &str| doc.eval(text.trim(), ValueKind::Unitless).ok();
    ui_kit::count_row(ui, label, value, &ui_kit::Count { min, max: max.max(min), eval: &eval })
}

/// How many copies the other direction of a rectangular pattern leaves this one, so that
/// both together stay within `MAX_COPIES`. All of them while the other count is not a number.
fn copies_left(doc: &Document, other: &str) -> i64 {
    let other = doc.eval(other.trim(), ValueKind::Unitless).ok().filter(|v| v.is_finite() && *v >= 1.0).map_or(1.0, f64::round);
    (MAX_COPIES / other).floor() as i64
}

fn icons(ui: &mut egui::Ui, label: &str, labels: &[&'static str], icons: &[&'static str], selected: &mut usize) {
    ui_kit::row(ui, label, |ui| {
        let choices: Vec<_> =
            labels.iter().zip(icons).map(|(key, icon)| ui_kit::IconSegment { key, icon, tip: crate::i18n::tr_label(key).to_string() }).collect();
        ui_kit::segmented_icons(ui, selected, &choices, &[]);
    });
}

fn alignment(ui: &mut egui::Ui, label: &str, alignment: &mut PatternAlignment, reversed: &mut bool) {
    ui_kit::row(ui, label, |ui| {
        ui_kit::toggle_icon(ui, label, "Flip direction", reversed, "pattern_flip");
        let mut centered = *alignment == PatternAlignment::Center;
        ui_kit::toggle_icon(ui, label, "Center", &mut centered, "pattern_center");
        *alignment = if centered { PatternAlignment::Center } else { PatternAlignment::Right };
    });
}

fn pattern_hint(ui: &mut egui::Ui) {
    ui_kit::text(
        ui,
        crate::i18n::tr_label("Counts include the original. Center balances copies; Flip chooses the extra side for even totals."),
        ui_kit::Text::Note("pattern"),
    );
}

/// Dense rows after the selection inputs; true when an expression field accepts Enter.
/// `doc` evaluates the pattern counts the step arrows change.
pub fn rows(ui: &mut egui::Ui, k: &mut Mod, doc: &Document) -> bool {
    let mut enter = false;
    match k {
        Mod::Scale { factor } => enter |= number(ui, "Factor", factor),
        Mod::Offset { distance, distance2, mode, chain, flip } => {
            icons(ui, "Direction", &OFFSET_MODES, &OFFSET_MODE_ICONS, mode);
            enter |= number(ui, "Distance", distance);
            if *mode == TWO_SIDES {
                enter |= number(ui, "Distance 2", distance2);
            }
            ui_kit::section(ui, "offset_options", "Options", true, |ui| {
                if *mode != SYMMETRIC {
                    ui_kit::checkbox_grid(ui, &mut [("Flip", flip), ("Chain", chain)], 2);
                } else {
                    ui_kit::checkbox_grid(ui, &mut [("Chain", chain)], 2);
                }
            });
        }
        Mod::Mirror => {}
        Mod::Move { x, y, angle, copy } => {
            enter |= ui_kit::value_pair(ui, "Position", [x, y], ["X", "Y"], Some(["X Distance", "Y Distance"]));
            enter |= number(ui, "Angle", angle);
            ui_kit::options(ui, "move_options", &mut [("Create Copy", copy)]);
        }
        Mod::Transform { x, y, factor, angle, frame } => {
            enter |= ui_kit::value_pair(ui, "Position", [x, y], ["X", "Y"], Some(["X Distance", "Y Distance"]));
            // The size in millimetres follows the factor and the other way round; the aspect is kept.
            if let Some((_, size)) = frame {
                for (label, natural) in [("Width", size[0]), ("Height", size[1])] {
                    let k = crate::prefs::parse_number(factor.trim()).filter(|k| *k > 0.0).unwrap_or(1.0);
                    let mut shown = format!("{} mm", crate::sketch_transform_ui::plain(natural * k));
                    if number(ui, label, &mut shown)
                        && let Some(mm) = crate::prefs::parse_number(shown.trim_end_matches("mm").trim())
                        && mm > 0.0
                        && natural > 1e-9
                    {
                        *factor = format!("{}", ((mm / natural) * 1e6).round() / 1e6);
                    }
                }
            }
            enter |= number(ui, "Scale", factor);
            enter |= number(ui, "Angle", angle);
        }
        Mod::PatternRect { count, spacing, count2, spacing2, alignment: a, alignment2, reversed, reversed2 } => {
            let (left1, left2) = (copies_left(doc, count2), copies_left(doc, count));
            for (key, title, count, spacing, align, reverse, label, distance, align_label, left) in [
                ("direction1", "Direction 1", count, spacing, a, reversed, "Quantity", "Spacing", "Alignment", left1),
                ("direction2", "Direction 2", count2, spacing2, alignment2, reversed2, "Quantity 2", "Spacing 2", "Alignment 2", left2),
            ] {
                ui_kit::section(ui, key, title, true, |ui| {
                    enter |= quantity(ui, doc, label, count, 1, left);
                    enter |= number(ui, distance, spacing);
                    alignment(ui, align_label, align, reverse);
                });
            }
            pattern_hint(ui);
        }
        Mod::PatternCirc { count, angle, alignment: a, reversed } => {
            enter |= quantity(ui, doc, "Quantity", count, 2, MAX_COPIES as i64);
            enter |= number(ui, "Total angle", angle);
            alignment(ui, "Alignment", a, reversed);
            pattern_hint(ui);
        }
        Mod::Equation { form, x, y, start, end, tolerance } => {
            ui_kit::choice_row(ui, "Form", "equation_form", form, &EQUATION_FORMS);
            if *form == 0 {
                enter |= number(ui, "x(t)", x);
                enter |= number(ui, "y(t)", y);
            } else {
                enter |= number(ui, "y(x)", y);
            }
            enter |= number(ui, "Start", start);
            enter |= number(ui, "End", end);
            enter |= number(ui, "Tolerance", tolerance);
            ui_kit::text(ui, crate::i18n::tr("ui.equation_note"), ui_kit::Text::Note("equation"));
        }
        Mod::Corner { chamfer, ctype, size, distance2, angle, flip } => {
            if *chamfer {
                icons(ui, "Chamfer Type", &CHAMFER_TYPES, &CHAMFER_TYPE_ICONS, ctype);
            }
            enter |= number(ui, size_label(*chamfer, *ctype), size);
            if *chamfer && *ctype == 1 {
                enter |= number(ui, "Distance 2", distance2);
            }
            if *chamfer && *ctype == 2 {
                enter |= number(ui, "Angle", angle);
            }
            if *chamfer && *ctype > 0 {
                ui_kit::options(ui, "corner_options", &mut [("Flip", flip)]);
            }
        }
    }
    enter
}

/// The in-plane direction a picked origin axis or straight sketch curve gives the active
/// sketch, as `sketch.pattern.rectangular` reads its `dir`. None when there is no active
/// sketch, or when the axis stands perpendicular to the sketch plane and so has no direction
/// in it.
fn sketch_dir(app: &SolveApp, sel: &Sel) -> Option<Vec2> {
    let dir = crate::dialogs::axis_of(app, sel)?.1;
    let plane = app.session.world_state().sketch(app.session.active_sketch?)?.plane;
    Vec2::new(dir.dot(plane.x), dir.dot(plane.y)).normalized()
}

/// The world direction a sketch modify window's dragged value grows along, for the viewport's
/// value box and its drag arrow (the same machinery `dialogs_sheet::arrow` feeds). The
/// rectangular pattern's spacing follows the picked direction, or the sketch's own x axis, which
/// is what the command uses when nothing is picked. Every other variant keeps the arrow
/// `canvas::anchor` already works out for it.
pub fn arrow(app: &SolveApp, d: &Dialog) -> Option<Vec3> {
    let Kind::SketchModify(k) = &d.kind else { return None };
    match k {
        Mod::PatternRect { alignment, reversed, .. } => {
            let plane = app.session.world_state().sketch(app.session.active_sketch?)?.plane;
            let dir = pattern_dir(app, d)?;
            Some(plane.dir_to_world(if (*alignment == PatternAlignment::Left) ^ *reversed { -dir } else { dir }))
        }
        // A total angle is a rotation. The viewport draws a straight drag arrow and a rotation
        // handle only for Revolve, through machinery tied to its own extents, so the circular
        // pattern takes the value box alone until that manipulator is made general.
        _ => None,
    }
}

/// The rectangular pattern's first direction in the sketch: the picked one, or the sketch's x axis.
fn pattern_dir(app: &SolveApp, d: &Dialog) -> Option<Vec2> {
    match d.inputs.get(1).and_then(|i| i.items.first()) {
        Some(sel) => sketch_dir(app, sel),
        None => Some(Vec2::X),
    }
}

/// The world direction the rectangular pattern's Spacing 2 grows along, for its own drag arrow
/// (#2462): the picked second direction, the one an edited pattern was made with, or square to
/// the first, as the command does.
pub fn arrow2(app: &SolveApp, d: &Dialog) -> Option<Vec3> {
    let Kind::SketchModify(Mod::PatternRect { alignment2, reversed2, .. }) = &d.kind else { return None };
    let plane = app.session.world_state().sketch(app.session.active_sketch?)?.plane;
    let dir = match d.inputs.get(2).and_then(|i| i.items.first()) {
        Some(sel) => sketch_dir(app, sel)?,
        None => match d.extra.get("dir2").and_then(|v| serde_json::from_value::<Vec2>(v.clone()).ok()).and_then(Vec2::normalized) {
            Some(kept) => kept,
            None => pattern_dir(app, d)?.perp(),
        },
    };
    Some(plane.dir_to_world(if (*alignment2 == PatternAlignment::Left) ^ *reversed2 { -dir } else { dir }))
}

/// What the first value means for this corner type.
fn size_label(chamfer: bool, ctype: usize) -> &'static str {
    if !chamfer {
        "Radius"
    } else if ctype == 0 {
        "Distance"
    } else {
        "Distance 1"
    }
}

/// The corner under the cursor while the Fillet or Chamfer window is open: the hover hit and
/// the selection it stands for.
///
/// Corners are resolved through `sketch_tools::corner_point` rather than the ordinary
/// nearest-point test. A rectangle drawn from the origin leaves its own endpoint on top of the
/// fixed origin point, and the nearest-point test returns the origin, which ends no two curves.
/// That resolution chooses among the endpoints that actually make a corner instead.
pub(crate) fn corner_hit(app: &SolveApp, proj: &Proj, pos: Pos2) -> Option<(Hit, Sel)> {
    let Some(Kind::SketchModify(Mod::Corner { chamfer, .. })) = app.dialog.as_ref().map(|d| &d.kind) else { return None };
    let sketch = app.session.active_sketch?;
    let (at, id) = crate::sketch_tools::corner_point(app, proj, pos, !*chamfer)?;
    let sel = Sel::SketchPoint { id: id.clone() };
    Some((Hit::SketchPoint { sketch, id, at }, sel))
}

/// What input `i` holds.
fn items(inputs: &[SelInput], i: usize) -> &[Sel] {
    inputs.get(i).map(|x| x.items.as_slice()).unwrap_or(&[])
}

/// A local point id. Qualified ids supplied by scripts or older selections are also accepted;
/// commands check that their sketch prefix identifies the sketch being edited.
pub(crate) fn point_id<'a>(session: &solidcraft_engine::Session, sel: &'a Sel) -> Option<&'a str> {
    let Sel::SketchPoint { id } = sel else { return None };
    let Some((owner, point)) = id.split_once(':') else { return Some(id) };
    let active = session.active_sketch?;
    // Custom local ids may themselves contain ':'. Preserve them before interpreting a
    // legacy qualified selection, whose owner must match the sketch being edited.
    if session.world_state().sketch(active).is_some_and(|sk| sk.sketch.point_index(id).is_some()) {
        Some(id)
    } else {
        (owner.parse::<u64>().ok() == Some(active)).then_some(point)
    }
}

/// The curves and lone points picked in input `i`, as the engine's `entities` list.
fn entity_ids(session: &solidcraft_engine::Session, inputs: &[SelInput], i: usize) -> Vec<String> {
    items(inputs, i)
        .iter()
        .filter_map(|x| match x {
            Sel::SketchCurve { id, .. } => Some(id.clone()),
            _ => point_id(session, x).map(str::to_string),
        })
        .collect()
}

/// Sketch curve ids picked in input `i`.
fn curve_ids(inputs: &[SelInput], i: usize) -> Vec<String> {
    items(inputs, i).iter().filter_map(|x| if let Sel::SketchCurve { id, .. } = x { Some(id.clone()) } else { None }).collect()
}

/// A typed scale factor as the command wants it: the field evaluated against the document's
/// parameters, in the range the command accepts. Checking the range here lets the window say
/// so in its own error line instead of the status bar.
pub(crate) fn factor(app: &SolveApp, text: &str) -> Result<f64, String> {
    crate::length_input::no_definition(text, "Factor")?;
    let v = app.session.doc.eval(text.trim(), ValueKind::Unitless).map_err(|e| format!("Factor: {e}"))?;
    if !(v.is_finite() && v > 1e-9 && v < 1e6) {
        return Err("Factor must be a positive number".into());
    }
    Ok(v)
}

/// A typed field as the command wants it: evaluated against the document's parameters, so an
/// expression works, with a bare number read in the design's own units. An empty field is zero,
/// the way an untouched row should leave the geometry where it is.
pub(crate) fn value(app: &SolveApp, text: &str, kind: ValueKind, label: &str) -> Result<f64, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(0.0);
    }
    crate::length_input::no_definition(text, label)?;
    let doc = &app.session.doc;
    let v = crate::length_input::eval(doc, text, kind).map_err(|e| format!("{label}: {e}"))?;
    if !v.is_finite() {
        return Err(format!("{label} must be a number"));
    }
    Ok(v)
}

/// A typed distance as the command wants it: the field evaluated against the document's
/// parameters, with the sign reversed when Flip is set. A bare number means the design's units,
/// as a typed sketch value does everywhere else.
fn signed_length(app: &SolveApp, label: &str, text: &str, flip: bool) -> Result<f64, String> {
    let doc = &app.session.doc;
    let v = crate::length_input::eval(doc, text.trim(), ValueKind::Length).map_err(|e| format!("{label}: {e}"))?;
    if !v.is_finite() || v.abs() <= 1e-9 {
        return Err(format!("{label} must be a non-zero number"));
    }
    Ok(if flip { -v } else { v })
}

/// Validate a typed count before sending its expression to the command.
fn whole(app: &SolveApp, label: &str, text: &str) -> Result<u64, String> {
    let v = app.session.doc.eval(text.trim(), ValueKind::Unitless).map_err(|e| format!("{label}: {e}"))?;
    if !v.is_finite() || !(1.0..=MAX_COPIES).contains(&v) || (v - v.round()).abs() > 1e-9 {
        return Err(format!("{label} must be a whole number from 1 to {MAX_COPIES:.0}"));
    }
    Ok(v.round() as u64)
}

/// A typed length as the command wants it, in millimetres. A bare number means the design's
/// units, as a typed sketch value does everywhere else. `value` above reads an empty row as
/// zero, which a Move window wants and a pattern spacing does not.
fn length(app: &SolveApp, label: &str, text: &str) -> Result<f64, String> {
    let doc = &app.session.doc;
    let v = crate::length_input::eval(doc, text.trim(), ValueKind::Length).map_err(|e| format!("{label}: {e}"))?;
    if !v.is_finite() {
        return Err(format!("{label} must be a number"));
    }
    Ok(v)
}
/// The curve ids picked in input `i` (a corner's two lines, #1824).
fn line_ids(inputs: &[SelInput], i: usize) -> Vec<String> {
    items(inputs, i)
        .iter()
        .filter_map(|x| match x {
            Sel::SketchCurve { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect()
}

/// Corner point ids within the active sketch, including qualified script selections.
fn corner_ids(app: &SolveApp, inputs: &[SelInput], i: usize) -> Vec<String> {
    items(inputs, i).iter().filter_map(|sel| point_id(&app.session, sel).map(str::to_string)).collect()
}

/// A corner size evaluated against the document's parameters, which every corner command needs
/// to be strictly positive. A bare number means the design's units for a length and degrees for
/// an angle, as every other typed sketch value does. `value` above is the Move window's reading
/// of a field, where an empty row means zero and a negative distance is a direction; neither is
/// a radius or a chamfer leg, so this one is its own.
fn corner_size(app: &SolveApp, label: &str, text: &str, kind: ValueKind) -> Result<f64, String> {
    crate::length_input::no_definition(text, label)?;
    let doc = &app.session.doc;
    let v = crate::length_input::eval(doc, text.trim(), kind).map_err(|e| format!("{label}: {e}"))?;
    if !v.is_finite() || v <= 1e-9 {
        return Err(format!("{label} must be a positive number"));
    }
    Ok(v)
}

/// The corner as a chamfer command takes it. Unflipped it is the point itself, and the command
/// resolves the two lines in curve order. Flipped it names those two lines in the other order,
/// so `distance` applies to the other leg.
fn corner_ref(app: &SolveApp, point: &str, flip: bool) -> Result<Value, String> {
    if !flip {
        return Ok(json!({ "point": point }));
    }
    let gone = || "that corner is no longer in the sketch".to_string();
    let sketch = app.session.active_sketch.ok_or("open a sketch first")?;
    let state = app.session.world_state();
    let sk = &state.sketch(sketch).ok_or_else(gone)?.sketch;
    let index = sk.resolve_point(point).ok_or_else(gone)?;
    let (a, b) = crate::sketch_tools::corner_curves(sk, index, false).ok_or("a chamfer needs a corner of exactly two lines")?;
    let id = |c: usize| sk.curves.get(c).map(|x| x.id.clone()).ok_or_else(gone);
    Ok(json!({"a": id(b)?, "b": id(a)?}))
}

/// A typed total angle as the command wants it, in degrees. A bare number means degrees, as a
/// typed angle does everywhere else. `sketch.pattern.circular` takes the full turn as 360 and
/// refuses anything outside that range.
fn total_angle(app: &SolveApp, label: &str, text: &str) -> Result<f64, String> {
    let v = app.session.doc.eval(text.trim(), ValueKind::Angle).map_err(|e| format!("{label}: {e}"))?.to_degrees();
    if !v.is_finite() || v.abs() <= 1e-9 || v.abs() > 360.0 {
        return Err(format!("{label} must be more than 0 and at most 360 degrees"));
    }
    Ok(v)
}

/// The bare sketch point id of a single point pick, as a command's `center` or `base` reads it.
/// `point_id` above takes one `Sel`; this one takes the first pick of an input.
fn picked_point(session: &solidcraft_engine::Session, inputs: &[SelInput], i: usize) -> Option<String> {
    items(inputs, i).first().and_then(|sel| point_id(session, sel)).map(str::to_string)
}

/// The command a sketch modify dialog makes. Scale, Move, and patterns send evaluated
/// numbers, except circular Quantity, which keeps its authored expression. Offset and Fillet
/// retain expressions in persistent driving dimensions.
pub fn commands(app: &SolveApp, k: &Mod, inputs: &[SelInput], extra: &Map<String, Value>) -> Result<Vec<(String, Value)>, String> {
    if inputs.iter().flat_map(|input| &input.items).any(|sel| matches!(sel, Sel::SketchPoint { .. }) && point_id(&app.session, sel).is_none()) {
        return Err("Select points of the sketch being edited".into());
    }
    let (cmd, mut params) = match k {
        Mod::Scale { factor: text } => {
            let entities = entity_ids(&app.session, inputs, 0);
            if entities.is_empty() {
                return Err("Select the sketch curves or points to scale first".into());
            }
            let f = factor(app, text)?;
            let mut p = json!({"entities": entities, "factor": f});
            if let Some(base) = items(inputs, 1).first().and_then(|sel| point_id(&app.session, sel)) {
                p["base"] = json!(base);
            }
            ("sketch.scale", p)
        }
        Mod::Transform { x, y, factor: text, angle, frame } => {
            let entities = entity_ids(&app.session, inputs, 0);
            if entities.is_empty() {
                return Err("Select the sketch curves or points to transform first".into());
            }
            let Some((middle, _)) = frame else { return Err("Select the sketch curves or points to transform first".into()) };
            let f = factor(app, text)?;
            let turn = value(app, angle, ValueKind::Angle, "Angle")?.to_degrees();
            let (dx, dy) = (value(app, x, ValueKind::Length, "X")?, value(app, y, ValueKind::Length, "Y")?);
            ("sketch.transform", json!({"entities": entities, "factor": f, "angle": turn, "center": middle, "translate": [dx, dy]}))
        }
        Mod::Offset { distance, distance2, mode, chain, flip } => {
            let curves = curve_ids(inputs, 0);
            if curves.is_empty() {
                return Err("Select the curves to offset first".into());
            }
            // Symmetric hides Flip, so a flip left over from another mode does not travel with it.
            signed_length(app, "Distance", distance, *flip && *mode != SYMMETRIC)?;
            let mut params = json!({"curves": curves, "distance": distance.trim(), "flip": *flip && *mode != SYMMETRIC, "chain": chain});
            if let Value::Object(m) = &mut params {
                if *mode == SYMMETRIC {
                    m.insert("symmetric".into(), json!(true));
                }
                if *mode == TWO_SIDES {
                    let d2 = signed_length(app, "Distance 2", distance2, false)?;
                    // The other way is the value negated; a definition keeps its name (#4071).
                    let negated = match solidcraft_engine::doc::expr::definition(distance2) {
                        Some((name, rhs)) => format!("{name} = -({rhs})"),
                        None => format!("-({})", distance2.trim()),
                    };
                    m.insert("distance2".into(), json!(if d2 > 0.0 { distance2.trim().to_string() } else { negated }));
                }
            }
            ("sketch.offset", params)
        }
        Mod::Mirror => {
            let entities = entity_ids(&app.session, inputs, 0);
            if entities.is_empty() {
                return Err("Select the sketch curves or points to mirror first".into());
            }
            let line = match items(inputs, 1).first() {
                Some(Sel::SketchCurve { id, .. }) => json!(id),
                Some(sel @ Sel::Axis { name }) if matches!(name.as_str(), "X" | "Y" | "Z") => {
                    if let Some(axis) = crate::tools::sketch_axis_of(app, name) {
                        json!(axis)
                    } else {
                        let (origin, dir) = crate::dialogs::axis_of(app, sel).ok_or("Select an origin axis")?;
                        let st = app.session.world_state();
                        let plane =
                            st.sketch(app.session.active_sketch.ok_or("No sketch is being edited")?).ok_or("The sketch no longer exists")?.plane;
                        if plane.normal().dot(dir).abs() > 1e-9 || plane.normal().dot(origin - plane.origin).abs() > 1e-6 {
                            return Err("The origin axis must lie in the sketch plane".into());
                        }
                        json!({"axis":name})
                    }
                }
                Some(_) => return Err("The mirror line must be a straight curve of this sketch or an origin axis in its plane".into()),
                None => return Err("Select the mirror line".into()),
            };
            if line.as_str().is_some_and(|line| entities.iter().any(|entity| entity == line)) {
                return Err("The mirror line cannot also be mirrored".into());
            }
            ("sketch.mirror", json!({"entities": entities, "line": line}))
        }
        Mod::Move { x, y, angle, copy } => {
            let entities = entity_ids(&app.session, inputs, 0);
            if entities.is_empty() {
                return Err("Select the sketch curves or points to move first".into());
            }
            let dx = value(app, x, ValueKind::Length, "X Distance")?;
            let dy = value(app, y, ValueKind::Length, "Y Distance")?;
            let deg = value(app, angle, ValueKind::Angle, "Angle")?.to_degrees();
            if dx.abs() < 1e-9 && dy.abs() < 1e-9 && deg.abs() < 1e-9 {
                return Err("Type a distance or an angle to move by".into());
            }
            let mut p = json!({"entities": entities, "translate": [dx, dy], "angle": deg, "copy": copy});
            if let Some(center) = items(inputs, 1).first().and_then(|sel| point_id(&app.session, sel)) {
                p["center"] = json!(center);
            }
            ("sketch.move", p)
        }
        Mod::PatternRect { count, spacing, count2, spacing2, alignment, alignment2, reversed, reversed2 } => {
            let entities = entity_ids(&app.session, inputs, 0);
            if entities.is_empty() {
                return Err("Select the sketch curves or points to pattern first".into());
            }
            let n1 = whole(app, "Quantity", count)?;
            let s1 = length(app, "Spacing", spacing)?;
            let n2 = whole(app, "Quantity 2", count2)?;
            let s2 = length(app, "Spacing 2", spacing2)?;
            if n1 > 1 && s1.abs() <= 1e-9 {
                return Err("Spacing must be a non-zero number".into());
            }
            if n2 > 1 && s2.abs() <= 1e-9 {
                return Err("Spacing 2 must be a non-zero number".into());
            }
            if n1.saturating_mul(n2) > MAX_COPIES as u64 {
                return Err(format!("Quantity and Quantity 2 together must make at most {MAX_COPIES:.0} instances"));
            }
            let mut p = json!({"entities": entities, "count": count, "spacing": spacing, "count2": count2, "spacing2": spacing2, "alignment": alignment, "alignment2": alignment2, "reversed": reversed, "reversed2": reversed2});
            // A picked direction that has no component in the sketch plane is reported rather
            // than quietly replaced by the default.
            for (index, vector_key, line_key) in [(1, "dir", "direction"), (2, "dir2", "direction2")] {
                if let Some(sel) = inputs.get(index).and_then(|i| i.items.first()) {
                    let d = sketch_dir(app, sel).ok_or("The direction must be an axis or a straight sketch curve that lies in the sketch plane")?;
                    p[vector_key] = json!([d.x, d.y]);
                    p[line_key] = match sel {
                        Sel::SketchCurve { id, .. } => json!(id),
                        _ => Value::Null,
                    };
                } else {
                    p[line_key] = Value::Null;
                }
            }
            ("sketch.pattern.rectangular", p)
        }
        Mod::PatternCirc { count, angle, alignment, reversed } => {
            let entities = entity_ids(&app.session, inputs, 0);
            if entities.is_empty() {
                return Err("Select the sketch curves or points to pattern first".into());
            }
            let center = match items(inputs, 1).first() {
                Some(Sel::Vertex { body, point }) if body.is_empty() => {
                    let plane =
                        app.session.world_state().sketch(app.session.active_sketch.ok_or("open a sketch first")?).ok_or("sketch is missing")?.plane;
                    let p = plane.to_local(*point);
                    json!([p.x, p.y])
                }
                _ => json!(picked_point(&app.session, inputs, 1).ok_or("Select the centre the pattern turns around first")?),
            };
            let n = whole(app, "Quantity", count)?;
            if n < 2 {
                return Err("Quantity must be at least 2".into());
            }
            total_angle(app, "Total angle", angle)?;
            (
                "sketch.pattern.circular",
                json!({"entities": entities, "center": center, "count": count, "angle": angle, "alignment": alignment, "reversed": reversed}),
            )
        }
        Mod::Corner { chamfer: false, size, .. } => {
            let corners = corner_ids(app, inputs, 0);
            let lines = line_ids(inputs, 0);
            // The engine keeps its millimetre API; bare UI values use the design unit.
            // Keep expressions intact so the radius remains a driving dimension.
            let radius = crate::length_input::expression(&app.session.doc, size, ValueKind::Length);
            match (corners.is_empty(), lines.as_slice()) {
                (true, []) => return Err("select the corners to round first".into()),
                (false, []) => ("sketch.fillet", json!({"points": corners, "radius": radius})),
                // Two lines name their corner even where other lines end at its point (#1824).
                (true, [a, b]) => ("sketch.fillet", json!({"a": a, "b": b, "radius": radius})),
                _ => return Err("select corner points, or the two lines of one corner".into()),
            }
        }
        Mod::Corner { ctype, size, distance2, angle, flip, .. } => {
            let flipped = *flip && *ctype > 0;
            let mut p = match (corner_ids(app, inputs, 0).as_slice(), line_ids(inputs, 0).as_slice()) {
                ([point], []) => corner_ref(app, point, flipped)?,
                // Several corners are cut alike, each flipped as one alone would be.
                (points, []) if !points.is_empty() => {
                    json!({"points": points.iter().map(|point| corner_ref(app, point, flipped)).collect::<Result<Vec<_>, _>>()?})
                }
                // Flip swaps the lines, so `distance` applies to the other leg.
                ([], [a, b]) if flipped => json!({"a": b, "b": a}),
                ([], [a, b]) => json!({"a": a, "b": b}),
                ([], []) => return Err("select the corner to cut first".into()),
                _ => return Err("select corner points, or the two lines of one corner".into()),
            };
            p["distance"] = json!(corner_size(app, size_label(true, *ctype), size, ValueKind::Length)?);
            match ctype {
                1 => {
                    p["distance2"] = json!(corner_size(app, "Distance 2", distance2, ValueKind::Length)?);
                    ("sketch.chamfer.two_distance", p)
                }
                2 => {
                    // The command takes degrees; an evaluated angle is in radians.
                    p["angle"] = json!(corner_size(app, "Angle", angle, ValueKind::Angle)?.to_degrees());
                    ("sketch.chamfer.distance_angle", p)
                }
                _ => ("sketch.chamfer.equal_distance", p),
            }
        }
        Mod::Equation { form, x, y, start, end, tolerance } => {
            let filled = |label: &str, text: &str| -> Result<String, String> {
                let text = text.trim();
                if text.is_empty() { Err(format!("{label} is empty")) } else { Ok(text.to_string()) }
            };
            let tol = crate::length_input::eval(&app.session.doc, tolerance.trim(), ValueKind::Length).map_err(|e| format!("Tolerance: {e}"))?;
            if !(tol.is_finite() && (1e-4..=10.0).contains(&tol)) {
                return Err("Tolerance must be from 0.0001 mm to 10 mm".into());
            }
            let mut p = json!({"y": filled(if *form == 0 { "y(t)" } else { "y(x)" }, y)?, "start": filled("Start", start)?, "end": filled("End", end)?, "tolerance": tol});
            if *form == 0 {
                p["x"] = json!(filled("x(t)", x)?);
            } else {
                p["explicit"] = json!(true);
            }
            (if extra.contains_key("curve") { "sketch.equation_curve.edit" } else { "sketch.equation_curve" }, p)
        }
    };
    if let Value::Object(m) = &mut params {
        for (key, value) in extra {
            m.entry(key.clone()).or_insert_with(|| value.clone());
        }
    }
    Ok(vec![(cmd.to_string(), params)])
}
