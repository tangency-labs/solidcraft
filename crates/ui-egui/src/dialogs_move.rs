//! Move/Copy: the move types (Free Move, Translate, Rotate, Point to Point, Point to Position),
//! their rows and inputs, the triad on the canvas and the solid.move they make. Nothing
//! moves until a value is typed or the triad is dragged. Switching the type keeps the move made
//! so far, and the next type moves the bodies on from where it left them: one solid.move holds
//! the whole chain.
//!
//! The triad lies along the world's axes, and each ring turns the bodies about the world axis it
//! shows, on top of the turns already made. So the angles apply in the order they were dragged
//! (`order`): a drag on a ring changes its own angle alone and the angle reads what was dragged.
//! Where no sum of angles can make a turn, the turns so far go into the move made so far and the
//! angle starts over (`bake_turn`).

use crate::selection::SelectionExt as _;
use crate::ui_kit::{choice_row, expr_row};
use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_engine::Sel;
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::doc::{Mat, apply_point, apply_vector, is_identity, mat_inverse, mat_mul, rigid};
use solidcraft_engine::geom::Aabb3;
use solidcraft_engine::geom::prelude::*;

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind};
use crate::gizmo::{self, Change, Drag};
use crate::selection::{AXIS_PICKS, BODIES, EDGES, FACES, ORIGIN, SNAP_POINTS, SelInput, VERTICES};

pub const TYPES: [&str; 5] = ["Free Move", "Translate", "Rotate", "Point to Point", "Point to Position"];
pub const FREE: usize = 0;
pub const TRANSLATE: usize = 1;
pub const ROTATE: usize = 2;
pub const POINT_TO_POINT: usize = 3;
pub const POINT_TO_POSITION: usize = 4;
const POINTS: crate::selection::Accept = VERTICES | FACES | EDGES | ORIGIN | SNAP_POINTS;

/// What Move/Copy moves, one kind at a time (the Object row). More kinds can follow these.
pub const OBJECTS: [&str; 2] = ["Components", "Bodies"];
pub const OBJECT_COMPONENTS: usize = 0;
pub const OBJECT_BODIES: usize = 1;

/// A fresh Move dialog of a type (all values zero).
pub fn kind(mode: usize, doc: &solidcraft_engine::doc::Document) -> Kind {
    let z = || crate::length_input::default(doc, "0");
    let a = || "0 deg".to_string();
    Kind::Move { mode, x: z(), y: z(), z: z(), rx: a(), ry: a(), rz: a(), angle: a() }
}

/// The selection inputs of a move type (what moves first: bodies or components, see [`object`]).
pub fn inputs(mode: usize) -> Vec<SelInput> {
    let mut v = vec![SelInput::new("Selection", BODIES, true)];
    match mode {
        FREE => v.push(SelInput::new("Pivot", POINTS, false)),
        ROTATE => v.push(SelInput::new("Axis", AXIS_PICKS, false)),
        POINT_TO_POINT => {
            v.push(SelInput::new("From", POINTS, false));
            v.push(SelInput::new("To", POINTS, false));
        }
        POINT_TO_POSITION => v.push(SelInput::new("Point", POINTS, false)),
        _ => {}
    }
    v
}

/// The Object row (above Selection). Object follows what is picked, and choosing it turns the
/// picks into that kind ([`switch_object`]). An edited Move feature moves bodies alone.
pub fn object_row(app: &SolveApp, d: &mut Dialog, ui: &mut egui::Ui) {
    if !matches!(d.kind, Kind::Move { .. }) || d.editing.is_some() {
        return;
    }
    one_kind(app, d);
    let current = object(d);
    let mut o = current;
    choice_row(ui, "Object", "move_object", &mut o, &OBJECTS);
    if o != current {
        switch_object(app, d, o);
    }
}

/// The Move Type row (below Selection, above the type's own picks). Switching keeps the picks and
/// the move made so far (`chain`), and starts the new type's values at zero.
pub fn type_row(app: &SolveApp, d: &mut Dialog, ui: &mut egui::Ui) {
    let Kind::Move { mode, .. } = d.kind else { return };
    let mut m = mode;
    choice_row(ui, "Move Type", "move_type", &mut m, &TYPES);
    if m != mode {
        switch_type(app, d, m);
    }
}

/// Change the Move Type to `m`, keeping the move made so far.
fn switch_type(app: &SolveApp, d: &mut Dialog, m: usize) {
    // A type whose picks are missing or whose values don't evaluate adds nothing.
    if let Ok(Step::Made(p)) = step(app, d)
        && let Ok(made) = transform(app, &p)
    {
        let total = chain(d).map_or(made, |c| mat_mul(&made, &c));
        d.extra.insert("chain".into(), json!(total));
    }
    let bodies = d.inputs.first().map(|i| i.items.clone()).unwrap_or_default();
    d.kind = kind(m, &app.session.doc);
    d.inputs = inputs(m);
    if let Some(i) = d.inputs.first_mut() {
        i.items = bodies;
    }
    // A kept turn (`axis`, `angle`, `origin`) is in `chain` now; keeping it would turn twice.
    for key in ["pivot", "origin", "order", "axis", "angle", "about"] {
        d.extra.remove(key);
    }
    d.active = if bodies_empty(d) { 0 } else { 1.min(d.inputs.len() - 1) };
}

/// What the dialog moves ([`OBJECTS`]): an edited Move feature moves bodies.
pub(crate) fn object(d: &Dialog) -> usize {
    if d.editing.is_some() {
        return OBJECT_BODIES;
    }
    d.extra.get("object").and_then(Value::as_u64).map_or(OBJECT_BODIES, |o| (o as usize).min(OBJECTS.len() - 1))
}

fn set_object(d: &mut Dialog, o: usize) {
    d.extra.insert("object".into(), json!(o));
}

/// One kind moves at a time, and Object follows what is picked; with nothing picked yet it
/// starts from the view's pick mode. Opened on components and bodies together, the components
/// win: each body becomes the component it is in (so several bodies of one component are that
/// one component), and a body outside every component drops out. Once one kind is picked, the
/// other is refused ([`admit`]).
fn one_kind(app: &SolveApp, d: &mut Dialog) {
    let unset = !d.extra.contains_key("object");
    let picks = items(d, 0);
    let components = picks.iter().any(|s| matches!(s, Sel::Occurrence { .. }));
    let bodies = picks.iter().any(|s| matches!(s, Sel::Body { .. }));
    match (components, bodies) {
        (true, false) => set_object(d, OBJECT_COMPONENTS),
        (false, true) => set_object(d, OBJECT_BODIES),
        (true, true) if unset || object(d) == OBJECT_COMPONENTS => {
            to_components(&app.session, d);
            set_object(d, OBJECT_COMPONENTS);
        }
        (true, true) => {
            if let Some(input) = d.inputs.first_mut() {
                input.items.retain(|s| !matches!(s, Sel::Occurrence { .. }));
            }
        }
        (false, false) if unset => set_object(d, if app.picks_components() { OBJECT_COMPONENTS } else { OBJECT_BODIES }),
        (false, false) => {}
    }
}

/// What a pick into Selection becomes, or `None` when it is refused (it still lights on hover):
/// one kind moves at a time. With components picked, a body stands for the component it is in,
/// and a body outside every component is refused. With bodies picked, a component is refused,
/// unless it holds one of them: then the whole component moves, so the picks become components
/// and the ones inside it give way to it. Anything is taken while nothing is picked.
pub(crate) fn admit(s: &solidcraft_engine::Session, d: &mut Dialog, sel: Sel) -> Option<Sel> {
    if !matches!(d.kind, Kind::Move { .. }) || d.active != 0 || d.editing.is_some() {
        return Some(sel);
    }
    let picks = items(d, 0);
    if picks.is_empty() {
        return Some(sel);
    }
    let components = picks.iter().any(|p| matches!(p, Sel::Occurrence { .. }));
    match sel {
        Sel::Occurrence { path } if !components => {
            // The body's whole placement path: a pick of a nested component keeps only part of it.
            let holds_a_pick = picks.iter().any(|p| match p {
                Sel::Body { name } => crate::dialogs::body_placement_path(s, name).is_some_and(|owner| owner.starts_with(&path)),
                _ => false,
            });
            if !holds_a_pick {
                return None;
            }
            // The bodies it holds give way to it; the others become the components they are in.
            if let Some(input) = d.inputs.first_mut() {
                input.items.retain(|p| {
                    !matches!(p, Sel::Body { name } if crate::dialogs::body_placement_path(s, name).is_some_and(|owner| owner.starts_with(&path)))
                });
            }
            to_components(s, d);
            set_object(d, OBJECT_COMPONENTS);
            if let Some(input) = d.inputs.first_mut() {
                input.items.retain(|p| !matches!(p, Sel::Occurrence { path: inner } if inner.starts_with(&path)));
            }
            Some(Sel::Occurrence { path })
        }
        Sel::Body { name } if components => crate::dialogs::occurrence_path_of_body(s, &name).map(|path| Sel::Occurrence { path }),
        other => Some(other),
    }
}

/// Whether `s` is picked in the open Move/Copy dialog's Selection: the Browser shows it selected,
/// and a click on its row takes it out of the move.
pub(crate) fn picked(app: &SolveApp, s: &Sel) -> bool {
    app.dialog.as_ref().is_some_and(|d| matches!(d.kind, Kind::Move { .. }) && items(d, 0).contains(s))
}

/// Object chosen in its row: the picks become that kind.
fn switch_object(app: &SolveApp, d: &mut Dialog, o: usize) {
    if o == OBJECT_COMPONENTS {
        to_components(&app.session, d);
    } else {
        to_bodies(app, d);
    }
    set_object(d, o);
    d.error = None;
}

/// The picks as components: a body stands for the component it is in, once for all its bodies,
/// and a component inside another picked one moves with it. A body outside every component drops
/// out.
fn to_components(session: &solidcraft_engine::Session, d: &mut Dialog) {
    let Some(input) = d.inputs.first_mut() else { return };
    let mut paths: Vec<Vec<u64>> = Vec::new();
    for s in &input.items {
        let path = match s {
            Sel::Occurrence { path } => Some(path.clone()),
            Sel::Body { name } => crate::dialogs::occurrence_path_of_body(session, name),
            _ => None,
        };
        if let Some(path) = path.filter(|p| !p.is_empty() && !paths.contains(p)) {
            paths.push(path);
        }
    }
    let outer: Vec<Vec<u64>> = paths.iter().filter(|p| !paths.iter().any(|q| q.len() < p.len() && p.starts_with(q))).cloned().collect();
    input.items = outer.into_iter().map(|path| Sel::Occurrence { path }).collect();
}

/// The picks as bodies: a component stands for the bodies it places.
fn to_bodies(app: &SolveApp, d: &mut Dialog) {
    let Some(input) = d.inputs.first_mut() else { return };
    let mut out: Vec<Sel> = Vec::new();
    for s in &input.items {
        let names = match s {
            Sel::Body { name } => vec![name.clone()],
            Sel::Occurrence { path } => app.session.occurrence_bodies(path, true).unwrap_or_default(),
            _ => Vec::new(),
        };
        for name in names {
            let body = Sel::Body { name };
            if !out.contains(&body) {
                out.push(body);
            }
        }
    }
    input.items = out;
}

fn bodies_empty(d: &Dialog) -> bool {
    d.inputs.first().is_none_or(|i| i.items.is_empty())
}

/// The move made under earlier types, which the current type moves the bodies on from.
fn chain(d: &Dialog) -> Option<Mat> {
    let m: Mat = serde_json::from_value(d.extra.get("chain")?.clone()).ok()?;
    (!is_identity(&m)).then_some(m)
}

/// The moved bodies and where the move so far puts them (the earlier types' move, then the
/// current type's as far as it is made), while a Move dialog moves any: the picker finds them,
/// and their highlights and centres show, where the preview draws them, also mid-drag.
pub(crate) fn chained(app: &SolveApp) -> Option<(Vec<String>, Mat)> {
    let d = app.dialog.as_ref().filter(|d| matches!(d.kind, Kind::Move { .. }))?;
    let m = match (made(app, d), chain(d)) {
        (Some(s), Some(c)) => mat_mul(&s, &c),
        (Some(s), None) => s,
        (None, c) => c?,
    };
    (!is_identity(&m)).then(|| (bodies_of(app, d), m))
}

/// Whether the preview draws the bodies a Move dialog moves as a layer the GPU moves
/// (`move_preview`), and shows it: the layer then draws them, in their own look and outlined as
/// picked, in place of the highlight, so a change of the move builds no triangles (#17804).
pub(crate) fn drawn_as_layer(app: &SolveApp) -> bool {
    app.dialog.as_ref().is_some_and(|d| matches!(d.kind, Kind::Move { .. }))
        && app.preview.active
        && !app.preview.rolled_back
        && app.preview.layer_shown().is_some()
}

/// Whether the triad is being dragged in a Move dialog (`drag` keeps the values it began from).
pub(crate) fn dragging(app: &SolveApp) -> bool {
    app.dialog.as_ref().is_some_and(|d| matches!(d.kind, Kind::Move { .. }) && d.extra.contains_key("drag0"))
}

/// The current type's own move, when its picks and values make one.
fn made(app: &SolveApp, d: &Dialog) -> Option<Mat> {
    match step(app, d) {
        Ok(Step::Made(p)) => transform(app, &p).ok(),
        _ => None,
    }
}

/// Where a point a pick keeps on moved body `body` (see [`from_shown`]) shows: moved on by the
/// current type's own move.
pub(crate) fn shown_point(app: &SolveApp, body: &str, p: Vec3) -> Vec3 {
    let Some(d) = app.dialog.as_ref().filter(|d| matches!(d.kind, Kind::Move { .. }) && bodies_of(app, d).iter().any(|n| n == body)) else {
        return p;
    };
    made(app, d).map_or(p, |m| apply_point(&m, p))
}

/// A pick on a moved body is made where the move so far shows it; the current type measures
/// from where the earlier types leave the body, so the pick is taken back by the current type's
/// own move. Kept there, a pivot or From point picked mid-move doesn't move with the move it
/// sets.
pub(crate) fn from_shown(app: &SolveApp, sel: Sel) -> Sel {
    let Some(d) = app.dialog.as_ref().filter(|d| matches!(d.kind, Kind::Move { .. }) && on_moved(&bodies_of(app, d), &sel)) else { return sel };
    let Some(back) = made(app, d).and_then(|m| mat_inverse(&m)) else { return sel };
    match sel {
        Sel::Edge { body, index, point } => Sel::Edge { body, index, point: apply_point(&back, point) },
        Sel::Face { body, index, point } => Sel::Face { body, index, point: apply_point(&back, point) },
        Sel::Vertex { body, point } => Sel::Vertex { body, point: apply_point(&back, point) },
        other => other,
    }
}

/// Whether a pick is on one of the bodies being moved (`moved`, see [`bodies_of`]).
fn on_moved(moved: &[String], s: &Sel) -> bool {
    match s {
        Sel::Edge { body, .. } | Sel::Face { body, .. } | Sel::Vertex { body, .. } => moved.contains(body),
        _ => false,
    }
}

/// The bodies the picks stand for: the bodies picked, and the bodies of each picked component
/// (the bodies its placement shows, as a Browser row lists them).
fn bodies_of(app: &SolveApp, d: &Dialog) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for sel in items(d, 0) {
        let names = match sel {
            Sel::Body { name } => vec![name.clone()],
            Sel::Occurrence { path } => app.session.occurrence_bodies(path, true).unwrap_or_default(),
            _ => Vec::new(),
        };
        for name in names {
            if !out.contains(&name) {
                out.push(name);
            }
        }
    }
    out
}

/// Whether Create Copy is ticked. A move opened on a pasted component never copies it.
fn copies(d: &Dialog) -> bool {
    !pasted(d) && d.extra.get("copy").and_then(Value::as_bool).unwrap_or(false)
}

/// Whether Move/Copy was opened to place a component just pasted: Create Copy is not offered.
pub(crate) fn pasted(d: &Dialog) -> bool {
    d.extra.get("pasted").and_then(Value::as_bool).unwrap_or(false)
}

/// The bodies the open Move/Copy dialog moves (those picked, and those of the components
/// picked): they keep their own look, outlined as selected, throughout the move.
pub(crate) fn moved_bodies(app: &SolveApp) -> Vec<String> {
    app.dialog.as_ref().filter(|d| matches!(d.kind, Kind::Move { .. })).map(|d| bodies_of(app, d)).unwrap_or_default()
}

/// Whether a Move dialog takes the components picked (not their bodies) into its bodies input: a
/// click on a component in the Parts selection mode, or a Browser row. Not when it edits a move
/// that is already a feature (its picks stay bodies). Without Create Copy the components still
/// move their bodies (`targets`).
pub(crate) fn picks_components(app: &SolveApp) -> bool {
    app.dialog.as_ref().is_some_and(|d| matches!(d.kind, Kind::Move { .. }) && d.active == 0 && d.editing.is_none())
}

/// What a click picks in a Move dialog: with Object on Components, the component a body
/// belongs to (copied as a component with Create Copy, see `targets`), otherwise the pick as it was.
pub(crate) fn component_pick(app: &SolveApp, hit: &crate::viewport::Hit, sel: Sel) -> Sel {
    let (crate::viewport::Hit::Face { body, .. }, Sel::Body { .. }) = (hit, &sel) else { return sel };
    if picks_components(app) && app.dialog.as_ref().is_some_and(|d| object(d) == OBJECT_COMPONENTS) {
        let occurrence = crate::viewport::component_selection(app, body);
        if matches!(occurrence, Sel::Occurrence { .. }) {
            return occurrence;
        }
    }
    sel
}

/// A Ctrl or Shift click (adding to the selection, as in the view) on a body once Point to
/// Point, Point to Position or Rotate has gone on to From, Point or Axis (#6170): the body joins
/// Bodies, or leaves it if picked already, and the window stays on the input it was on. So
/// several bodies are picked without clicking back into Bodies each time (#14205). With Shift
/// adding only (`allow_toggle` false, Preferences), a picked body stays picked. False when the
/// click is not such a pick, which then goes to the active input as usual, and while OK computes,
/// when the inputs hold.
pub(crate) fn add_body_click(app: &mut SolveApp, hits: &[crate::viewport::Hit], allow_toggle: bool) -> bool {
    if app.applying.is_some() {
        return false;
    }
    let Some(d) = app.dialog.as_mut() else { return false };
    let Kind::Move { mode, .. } = d.kind else { return false };
    if !matches!(mode, ROTATE | POINT_TO_POINT | POINT_TO_POSITION) || d.active == 0 || d.inputs.first().is_none_or(|i| i.accept & BODIES == 0) {
        return false;
    }
    // What the click picks for Bodies, made as a click with Bodies active makes it.
    let on = std::mem::replace(&mut d.active, 0);
    let body = crate::viewport::candidate(app, hits).map(|(_, sel)| sel);
    let Some(mut d) = app.dialog.take() else { return false };
    if let Some(body) = body.clone().filter(|b| allow_toggle || d.inputs.first().is_none_or(|i| !i.items.contains(b))) {
        d.pick(&app.session, body);
    }
    d.active = on;
    app.dialog = Some(d);
    body.is_some()
}

/// Whether Capture All Positions is ticked (it is at first): OK captures every component move
/// still pending, these and any made before, as the Capture Position command does.
fn captures(d: &Dialog) -> bool {
    d.extra.get("capture").and_then(Value::as_bool).unwrap_or(true)
}

/// The components picked without Create Copy, which move by their placement
/// (`occurrence.move`) rather than by a Move feature on their bodies, as their exact
/// placements. A component inside another picked one moves with it, so it is left out. An
/// edited Move feature moves bodies only.
pub(crate) fn placements(d: &Dialog) -> Vec<Vec<u64>> {
    placement_paths(items(d, 0), copies(d), d.editing.is_some())
}

/// [`placements`] from the Bodies picks, whether Create Copy is ticked, and whether a Move feature
/// is being edited. A shared child reached through two copies of its parent is one occurrence,
/// moved once: each `occurrence.move` would move it again.
pub(crate) fn placement_paths(picks: &[Sel], copy: bool, editing: bool) -> Vec<Vec<u64>> {
    if copy || editing {
        return Vec::new();
    }
    let paths: Vec<&Vec<u64>> = picks
        .iter()
        .filter_map(|s| match s {
            Sel::Occurrence { path } if !path.is_empty() => Some(path),
            _ => None,
        })
        .collect();
    let mut out: Vec<Vec<u64>> = Vec::new();
    for p in &paths {
        let inside_another = paths.iter().any(|q| q.len() < p.len() && p.starts_with(q));
        let same_occurrence = out.iter().any(|q| q.last() == p.last());
        if !inside_another && !same_occurrence {
            out.push((*p).clone());
        }
    }
    out
}

/// The row-major 3x4 matrix `occurrence.move` takes for the rigid move `m`.
fn matrix_rows(m: &Mat) -> Vec<f64> {
    (0..3).flat_map(|r| (0..4).map(move |c| m[c][r])).collect()
}

/// What OK runs. Each component picked without Create Copy moves by its placement: captured
/// when Capture All Positions is ticked, which then captures the other pending moves too, and
/// pending when it is not. The bodies picked (or, with Create Copy, the bodies and components)
/// move by solid.move. Empty when only components are picked, nothing moved them and nothing is
/// left to capture: OK then just closes.
pub fn commands(app: &SolveApp, d: &Dialog) -> Result<Vec<(String, Value)>, String> {
    let p = params(app, d)?;
    let places = placements(d);
    let mut out = Vec::new();
    if !places.is_empty() {
        let m = transform(app, &p)?;
        let capture = captures(d);
        // Captured by the move itself, so one Undo takes it back to where it was. Capturing it
        // afterwards would leave Undo with it pending where it was moved to.
        let mut moved = Vec::new();
        if !is_identity(&m) {
            let matrix = matrix_rows(&m);
            for path in &places {
                let Some(&occurrence) = path.last() else { continue };
                moved.push(occurrence);
                out.push((
                    "occurrence.move".to_string(),
                    json!({ "occurrence": occurrence, "path": path, "space": "world", "matrix": matrix, "tolerance": 1e-6, "capture": capture }),
                ));
            }
        }
        // Capture All Positions: the components moved before, and not captured, too.
        if capture && app.session.pending_moves.keys().any(|id| !moved.contains(id)) {
            out.push(("component.capture_position".to_string(), json!({})));
        }
    }
    let moves_bodies = p.get("bodies").and_then(Value::as_array).is_some_and(|b| !b.is_empty()) || p.get("occurrences").is_some();
    if moves_bodies || places.is_empty() {
        out.push(("solid.move".to_string(), p));
    }
    Ok(out)
}

/// What the preview draws. The components that move by their placement are drawn as their
/// bodies moved, which is where the placement move will put them: one solid.move of every moved
/// body, which the fast move preview draws (`move_preview`) without evaluating the design. What
/// is hidden is left out of the preview (it still moves on OK), so hiding a body or a component
/// while it moves hides it; with everything hidden there is nothing to preview.
pub(crate) fn preview(app: &SolveApp, d: &Dialog, cmds: &mut Vec<(String, Value)>) {
    if !placements(d).is_empty() {
        let Some(m) = params(app, d).ok().and_then(|p| transform(app, &p).ok()) else { return };
        *cmds = vec![("solid.move".to_string(), with_transform(json!({ "bodies": bodies_of(app, d) }), &m))];
    }
    for (id, p) in cmds.iter_mut() {
        if id != "solid.move" {
            continue;
        }
        if let Some(bodies) = p.get_mut("bodies").and_then(Value::as_array_mut) {
            bodies.retain(|b| b.as_str().is_some_and(|name| app.body_visible(name)));
        }
        if let Some(paths) = p.get_mut("occurrences").and_then(Value::as_array_mut) {
            paths.retain(|path| {
                let path: Vec<u64> = path.as_array().map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
                app.session.occurrence_visible(&path)
            });
        }
    }
    cmds.retain(|(id, p)| {
        id != "solid.move"
            || p.get("bodies").and_then(Value::as_array).is_some_and(|b| !b.is_empty())
            || p.get("occurrences").and_then(Value::as_array).is_some_and(|o| !o.is_empty())
    });
}

/// The picked components whose positions are pending (moved, not captured yet): Revert Position
/// puts just these back.
pub(crate) fn revertible(app: &SolveApp, d: &Dialog) -> Vec<u64> {
    placements(d).iter().filter_map(|path| path.last().copied()).filter(|id| app.session.pending_moves.contains_key(id)).collect()
}

/// Whether OK has nothing to run (see [`commands`]).
pub(crate) fn nothing_to_run(app: &SolveApp, d: &Dialog) -> bool {
    matches!(d.kind, Kind::Move { .. }) && commands(app, d).is_ok_and(|c| c.is_empty())
}

/// Move/Copy opened on one component (from its Browser menu, the ribbon with it selected, or a
/// pasted design: `pasted`, which offers no Create Copy), which then moves by its placement.
pub(crate) fn open_on_occurrence(app: &mut SolveApp, path: Vec<u64>, pasted: bool) {
    if path.is_empty() {
        return;
    }
    crate::dialogs::cancel(app);
    app.tool = None;
    let Some(mut d) = crate::dialogs::Dialog::for_command(app, "solid.move") else { return };
    if let Some(input) = d.inputs.first_mut() {
        input.items = vec![Sel::Occurrence { path }];
    }
    // Selection has the focus, as when the dialog opens from the ribbon.
    d.active = 0;
    if pasted {
        d.extra.insert("pasted".into(), json!(true));
        d.extra.remove("copy");
    }
    set_object(&mut d, OBJECT_COMPONENTS);
    let _ = app.session.execute("select.clear", &json!({}));
    app.dialog = Some(d);
}

/// The component placement the open Move/Copy dialog moves, if it moves one.
#[cfg(test)]
pub(crate) fn moving(app: &SolveApp) -> Option<Vec<u64>> {
    app.dialog.as_ref().filter(|d| matches!(d.kind, Kind::Move { .. })).and_then(|d| placements(d).into_iter().next())
}

/// The exact placement of an occurrence when the caller names none: its first one.
pub(crate) fn first_path(app: &SolveApp, occurrence: u64) -> Vec<u64> {
    let doc = &app.session.doc;
    let Some(o) = doc.occurrences.iter().find(|o| o.id == occurrence) else { return Vec::new() };
    let mut path = doc.first_path(o.parent);
    path.push(occurrence);
    path
}

/// Whether a component that moves by its placement is a shared child: its definition is placed
/// in more than one copy of its parent, so the move applies to every copy.
pub(crate) fn moves_shared(app: &SolveApp, d: &Dialog) -> bool {
    placements(d).iter().any(|path| path.len() > 1 && app.session.occurrence_scope_count(path).is_ok_and(|n| n > 1))
}

/// What the move moves and copies, as solid.move takes them: the bodies, and with Create Copy the
/// components picked (copied as components, see `occurrences`). Without Create Copy a picked
/// component moves by its placement instead ([`placements`]), except in an edited Move feature,
/// which moves its bodies.
fn targets(app: &SolveApp, d: &Dialog) -> (Vec<String>, Vec<Vec<u64>>) {
    if !copies(d) {
        if d.editing.is_none() {
            return (items(d, 0).body_names(), Vec::new());
        }
        return (bodies_of(app, d), Vec::new());
    }
    let components = items(d, 0)
        .iter()
        .filter_map(|s| match s {
            Sel::Occurrence { path } => Some(path.clone()),
            _ => None,
        })
        .collect();
    (items(d, 0).body_names(), components)
}

/// The solid.move parameters that name what the move moves (`bodies`, and `occurrences` when
/// there are components to copy).
fn targets_json(app: &SolveApp, d: &Dialog) -> Value {
    let (bodies, components) = targets(app, d);
    let mut p = json!({ "bodies": bodies });
    if !components.is_empty() {
        p["occurrences"] = json!(components);
    }
    p
}

/// A point of the model before this move where the earlier types' move puts it: on a moved body
/// (`moved`) or, for the bodies' centre, always.
fn carried(d: &Dialog, moved: bool, p: Vec3) -> Vec3 {
    match chain(d) {
        Some(c) if moved => apply_point(&c, p),
        _ => p,
    }
}

/// The value rows after the inputs; true when Enter was pressed in one.
pub fn rows(ui: &mut egui::Ui, k: &mut Kind) -> bool {
    let Kind::Move { mode, x, y, z, rx, ry, rz, angle } = k else { return false };
    let mut enter = false;
    match *mode {
        FREE | TRANSLATE => {
            enter |= expr_row(ui, "X Distance", x);
            enter |= expr_row(ui, "Y Distance", y);
            enter |= expr_row(ui, "Z Distance", z);
            if *mode == FREE {
                enter |= expr_row(ui, "X Angle", rx);
                enter |= expr_row(ui, "Y Angle", ry);
                enter |= expr_row(ui, "Z Angle", rz);
            }
        }
        ROTATE => enter |= expr_row(ui, "Angle", angle),
        POINT_TO_POSITION => {
            enter |= expr_row(ui, "X", x);
            enter |= expr_row(ui, "Y", y);
            enter |= expr_row(ui, "Z", z);
        }
        _ => {}
    }
    enter
}

/// Where a point pick is: a round edge stands for its centre, where the model is before this
/// move, in the world (a body of a placed component where its occurrence puts it, as it was
/// picked); any other pick is where it was picked.
fn sel_point(app: &SolveApp, d: &Dialog, s: Option<&Sel>) -> Option<Vec3> {
    let s = s?;
    match s {
        Sel::Edge { body, index, point } => {
            // As every pick reads the placed model: one that cannot be placed has no bodies here.
            let st = match d.editing {
                Some((id, _)) => std::sync::Arc::new(
                    solidcraft_engine::doc::world_state(&app.session.doc, &app.session.model.state_before(id)).unwrap_or_default(),
                ),
                None => crate::open_meshing::session_world(&app.session),
            };
            // The centre is found where the body was modelled; the picked point is where it stands.
            Some(crate::dialogs::edge_circle_in(&st, body, *index).map_or(*point, |(c, ..)| carried(d, on_moved(&bodies_of(app, d), s), c)))
        }
        Sel::Vertex { point, .. } | Sel::Face { point, .. } => Some(*point),
        _ => None,
    }
}

fn items(d: &Dialog, i: usize) -> &[Sel] {
    crate::selection::items(&d.inputs, i)
}

/// The box around the picked bodies (see [`bodies_of`]) in the world, where they are before this move.
fn bodies_bounds(app: &SolveApp, d: &Dialog) -> Aabb3 {
    let names = bodies_of(app, d);
    let mut bb = Aabb3::EMPTY;
    let Some((id, _)) = d.editing else {
        let st = crate::open_meshing::session_world(&app.session);
        for b in names.iter().filter_map(|n| st.body(n)) {
            bb = bb.union(&b.mesh().bounds());
        }
        return bb;
    };
    // This runs every frame: place the box rather than every body of the earlier model.
    let st = app.session.model.state_before(id);
    for (name, source, _, _, m) in app.session.doc.body_frames_with_source(&st) {
        if !names.contains(&name) {
            continue;
        }
        let Some(x) = st.body(&source).map(|b| b.mesh().bounds()) else { continue };
        for corner in 0..8 {
            let pick = |bit: u8, lo: f64, hi: f64| if corner >> bit & 1 == 0 { lo } else { hi };
            bb.add(apply_point(&m, Vec3::new(pick(0, x.min.x, x.max.x), pick(1, x.min.y, x.max.y), pick(2, x.min.z, x.max.z))));
        }
    }
    bb
}

/// The centre of the picked bodies where they are before this move (after the earlier types').
fn bodies_center(app: &SolveApp, d: &Dialog) -> Option<Vec3> {
    let bb = bodies_bounds(app, d);
    (!bb.is_empty()).then(|| carried(d, true, bb.center()))
}

/// What a Free Move turns about: the picked pivot, else the kept one (editing: the stored
/// origin, in the world), else the bodies' centre.
fn pivot(app: &SolveApp, d: &Dialog) -> Option<Vec3> {
    if let Some(p) = sel_point(app, d, items(d, 1).first()) {
        return Some(p);
    }
    if let Some(a) = d.extra.get("pivot").and_then(|v| serde_json::from_value::<[f64; 3]>(v.clone()).ok()) {
        return Some(Vec3::new(a[0], a[1], a[2]));
    }
    bodies_center(app, d)
}

/// Whether the move turns about the origin an edited move stored: its stored values, expressions
/// included, mean that. The triad still sits on the bodies, as that origin can be far from them.
fn kept_turn(d: &Dialog) -> bool {
    d.extra.contains_key("pivot") && items(d, 1).is_empty()
}

/// Where the bodies' centre is after the move as it stands.
fn moved_centre(app: &SolveApp, d: &Dialog) -> Option<Vec3> {
    Some(apply_point(&made(app, d)?, bodies_center(app, d)?))
}

/// Turn the same move about the bodies' centre instead of the kept point: the angles stay, the
/// distances become how far that centre goes. A ring needs this, as it turns about the triad.
fn turn_on_centre(app: &SolveApp, d: &mut Dialog) {
    // A kept turn that does not evaluate lives in `axis` and `angle`, about the kept point.
    if !kept_turn(d) || d.extra.contains_key("angle") {
        return;
    }
    let (Some(at), Some(centre)) = (moved_centre(app, d), bodies_center(app, d)) else { return };
    let t = at - centre;
    for key in ["pivot", "origin"] {
        d.extra.remove(key);
    }
    if let Kind::Move { x, y, z, .. } = &mut d.kind {
        (*x, *y, *z) = (length(app, t.x), length(app, t.y), length(app, t.z));
    }
}

/// The turn part of a placement, as rows.
pub(crate) fn rows_of(m: &Mat) -> [[f64; 3]; 3] {
    [0, 1, 2].map(|r| [m[0][r], m[1][r], m[2][r]])
}

/// A distance worked out from a turn, without its rounding noise.
fn length(app: &SolveApp, v: f64) -> String {
    crate::length_input::model(&app.session.doc, (v * 1e6).round() / 1e6 + 0.0)
}

fn len(app: &SolveApp, s: &str) -> Result<f64, String> {
    crate::length_input::eval(&app.session.doc, s, ValueKind::Length)
        .map_err(|e| e.to_string())
        .and_then(|v| if v.is_finite() { Ok(v) } else { Err("not a number".into()) })
}

fn ang(app: &SolveApp, s: &str) -> Result<f64, String> {
    crate::length_input::eval(&app.session.doc, s, ValueKind::Angle)
        .map_err(|e| e.to_string())
        .and_then(|v| if v.is_finite() { Ok(v) } else { Err("not a number".into()) })
}

fn mm(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{} mm", if s == "-0" || s.is_empty() { "0" } else { s })
}

/// The three angles of a Free Move.
fn angles(app: &SolveApp, d: &Dialog) -> Result<[f64; 3], String> {
    let Kind::Move { rx, ry, rz, .. } = &d.kind else { return Err("not a move".into()) };
    Ok([ang(app, rx)?, ang(app, ry)?, ang(app, rz)?])
}

/// The order the angles apply in (see [`gizmo::turn_of`]).
fn order(d: &Dialog) -> [usize; 3] {
    d.extra.get("order").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or(gizmo::ANGLE_ORDER)
}

/// Put the current turn into the move made so far (`chain`) and start the angles from zero; the
/// distances stay. Nothing moves.
fn bake_turn(app: &SolveApp, d: &mut Dialog) {
    let (Some(m), Kind::Move { x, y, z, .. }) = (made(app, d), &d.kind) else { return };
    let (Ok(tx), Ok(ty), Ok(tz)) = (len(app, x), len(app, y), len(app, z)) else { return };
    let keep = mat_mul(&rigid(Vec3::new(-tx, -ty, -tz), Vec3::ZERO, Vec3::Z, 0.0), &m);
    let total = chain(d).map_or(keep, |c| mat_mul(&keep, &c));
    d.extra.insert("chain".into(), json!(total));
    d.extra.remove("order");
    if let Kind::Move { rx, ry, rz, .. } = &mut d.kind {
        for a in [rx, ry, rz] {
            *a = "0 deg".into();
        }
    }
}

fn deg(v: f64) -> String {
    let s = format!("{:.4}", v.to_degrees());
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{} deg", if s == "-0" || s.is_empty() { "0" } else { s })
}

/// The solid.move parameters (bodies, translate, axis, angle): the current type's move after
/// the earlier types' (`chain`), which alone is made while the current type's picks are missing.
pub fn params(app: &SolveApp, d: &Dialog) -> Result<Value, String> {
    let mut p = match (step(app, d)?, chain(d)) {
        (Step::Made(p), None) => p,
        (Step::Needs(e), None) => return Err(e),
        (Step::Made(p), Some(c)) => {
            let made = transform(app, &p)?;
            with_transform(p, &mat_mul(&made, &c))
        }
        (Step::Needs(_), Some(c)) => with_transform(targets_json(app, d), &c),
    };
    if d.extra.get("copy").and_then(Value::as_bool).unwrap_or(false) {
        p["copy"] = json!(true);
    }
    Ok(p)
}

/// The current type's move, or what it still needs picked.
enum Step {
    Made(Value),
    Needs(String),
}

/// The rigid transform of solid.move parameters: the turn about the supplied origin, then the translation.
fn transform(app: &SolveApp, p: &Value) -> Result<Mat, String> {
    let value = |v: &Value, f: fn(&SolveApp, &str) -> Result<f64, String>| match v {
        Value::String(e) => f(app, e),
        Value::Number(n) => n.as_f64().ok_or_else(|| "not a number".to_string()),
        _ => Err("not a number".into()),
    };
    let t = match p.get("translate").and_then(Value::as_array) {
        Some(a) if a.len() == 3 => Vec3::new(value(&a[0], len)?, value(&a[1], len)?, value(&a[2], len)?),
        _ => Vec3::ZERO,
    };
    let Some(angle) = p.get("angle") else { return Ok(rigid(t, Vec3::ZERO, Vec3::Z, 0.0)) };
    let origin = match p.get("origin").and_then(Value::as_array) {
        Some(a) if a.len() == 3 => Vec3::new(value(&a[0], len)?, value(&a[1], len)?, value(&a[2], len)?),
        _ => Vec3::ZERO,
    };
    let axis = match p.get("axis").and_then(Value::as_array) {
        Some(a) if a.len() == 3 => Vec3::new(value(&a[0], len)?, value(&a[1], len)?, value(&a[2], len)?),
        _ => Vec3::Z,
    };
    Ok(rigid(t, origin, axis, value(angle, ang)?))
}

/// `p` with its translation and turn replaced by `m`'s. `m` turns about the world origin, so a
/// turn names that origin: an omitted one would be the moved body's own frame's origin.
fn with_transform(mut p: Value, m: &Mat) -> Value {
    let (axis, th) = gizmo::axis_angle(&rows_of(m));
    p["translate"] = json!([mm(m[3][0]), mm(m[3][1]), mm(m[3][2])]);
    if let Some(o) = p.as_object_mut() {
        o.remove("axis");
        o.remove("angle");
        o.remove("origin");
        o.remove("about");
    }
    if th.abs() > 1e-12 {
        p["origin"] = json!([mm(0.0), mm(0.0), mm(0.0)]);
        p["axis"] = json!([axis.x, axis.y, axis.z]);
        p["angle"] = json!(deg(th));
    }
    p
}

/// The current type's own move, from where the earlier types leave the bodies.
fn step(app: &SolveApp, d: &Dialog) -> Result<Step, String> {
    let Kind::Move { mode, x, y, z, rx, ry, rz, angle } = &d.kind else { return Err("not a move".into()) };
    let (bodies, components) = targets(app, d);
    if bodies.is_empty() && components.is_empty() && placements(d).is_empty() {
        return Err("select bodies first".into());
    }
    let need_point = |i: usize, what: &str| sel_point(app, d, items(d, i).first()).ok_or(format!("select {what} first"));
    let num = |t: Vec3| json!([mm(t.x), mm(t.y), mm(t.z)]);
    let mut p = targets_json(app, d);
    match *mode {
        TRANSLATE => p["translate"] = json!([x, y, z]),
        FREE => {
            let a = [ang(app, rx)?, ang(app, ry)?, ang(app, rz)?];
            let turned: Vec<usize> = (0..3).filter(|i| a[*i].abs() > 1e-12).collect();
            if turned.is_empty() {
                p["translate"] = json!([x, y, z]);
                // A rotation kept from an edited move about another axis.
                for k in ["axis", "angle", "origin"] {
                    if let Some(v) = d.extra.get(k) {
                        p[k] = v.clone();
                    }
                }
            } else {
                let piv = pivot(app, d).ok_or("select bodies first")?;
                let r = gizmo::turn_of(a, order(d));
                let (axis, th) = gizmo::axis_angle(&r);
                p["origin"] = if items(d, 1).is_empty() { d.extra.get("origin").cloned().unwrap_or_else(|| num(piv)) } else { num(piv) };
                p["translate"] = json!([x, y, z]);
                match turned.as_slice() {
                    // A single turn keeps its angle expression and pivot independently.
                    [i] => {
                        p["axis"] = json!([[1, 0, 0], [0, 1, 0], [0, 0, 1]][*i]);
                        p["angle"] = json!([rx, ry, rz][*i]);
                    }
                    _ => {
                        p["axis"] = json!([axis.x, axis.y, axis.z]);
                        p["angle"] = json!(deg(th));
                    }
                }
            }
        }
        ROTATE => {
            if let Some((axis, origin)) = kept_axis(&d.inputs, &d.extra) {
                // An edited turn's own axis and pivot, as stored (#17560).
                p["origin"] = origin.clone();
                p["axis"] = axis.clone();
                // And the edge or face it was picked on, so the turn still follows it (#17974).
                if let Some(about) = d.extra.get("about") {
                    p["about"] = about.clone();
                }
            } else {
                let Some((o, a)) = items(d, 1).first().and_then(|s| axis_line(app, d, s)) else {
                    return Ok(Step::Needs("select an axis, a straight or circular edge, or a cylinder, cone or torus face first".into()));
                };
                p["origin"] = num(o);
                p["axis"] = json!([a.x, a.y, a.z]);
            }
            ang(app, angle)?;
            p["angle"] = json!(angle);
            // A body's edge or round face is kept as a reference, so the turn follows it when an
            // earlier step moves the body (#17974).
            if let Some(about) = items(d, 1).first().and_then(turn_about) {
                p["about"] = about;
            }
        }
        POINT_TO_POINT => {
            let (from, to) = match (need_point(1, "the point to move from"), need_point(2, "the point to move to")) {
                (Ok(from), Ok(to)) => (from, to),
                (Err(e), _) | (_, Err(e)) => return Ok(Step::Needs(e)),
            };
            p["translate"] = num(to - from);
        }
        _ => {
            let from = match need_point(1, "the point to move") {
                Ok(from) => from,
                Err(e) => return Ok(Step::Needs(e)),
            };
            let at = Vec3::new(len(app, x)?, len(app, y)?, len(app, z)?);
            p["translate"] = num(at - from);
        }
    }
    Ok(Step::Made(p))
}

/// An axis line: origin axes and sketch lines, a straight or circular body edge, or a cylinder,
/// cone or torus face (`dialogs::axis_of`), or a straight edge of a body where it was modelled;
/// on a moved body, where the earlier types' move puts it.
fn axis_line(app: &SolveApp, d: &Dialog, s: &Sel) -> Option<(Vec3, Vec3)> {
    let (o, a) = modelled_axis(app, s)?;
    match chain(d) {
        Some(c) if on_moved(&bodies_of(app, d), s) => Some((apply_point(&c, o), apply_vector(&c, a))),
        _ => Some((o, a)),
    }
}

/// A picked body edge or face as the reference a Rotate's `about` keeps (#17974).
fn turn_about(s: &Sel) -> Option<Value> {
    match s {
        Sel::Edge { body, point, .. } if !body.is_empty() => Some(json!({"edge": [point.x, point.y, point.z], "body": body})),
        Sel::Face { body, point, .. } if !body.is_empty() => Some(json!({"face": [point.x, point.y, point.z], "body": body})),
        _ => None,
    }
}

fn modelled_axis(app: &SolveApp, s: &Sel) -> Option<(Vec3, Vec3)> {
    if let Some(a) = crate::dialogs::axis_of(app, s) {
        return Some(a);
    }
    let Sel::Edge { body, index, .. } = s else { return None };
    let st = app.session.model.state();
    let m = st.body(body)?.mesh();
    let e = m.edges.get(*index)?;
    let (a, b) = (*e.first()?, *e.last()?);
    let dir = (b - a).normalized()?;
    // Straight: every point on the chord.
    e.iter().all(|p| (*p - a).cross(dir).len() < 1e-6 * (1.0 + (b - a).len())).then_some((a, dir))
}

/// The axis an edited Rotate keeps while no other is picked (#17560): its direction and its
/// pivot as stored, the pivot's expressions included.
fn kept_axis<'a>(inputs: &[SelInput], extra: &'a serde_json::Map<String, Value>) -> Option<(&'a Value, &'a Value)> {
    if !crate::selection::items(inputs, 1).is_empty() || inputs.get(1).is_none_or(|i| i.accept != AXIS_PICKS) {
        return None;
    }
    Some((extra.get("axis")?, extra.get("origin")?))
}

/// [`kept_axis`] as a line in the world: a point on it and its direction.
fn kept_axis_line(app: &SolveApp, d: &Dialog) -> Option<(Vec3, Vec3)> {
    let (axis, origin) = kept_axis(&d.inputs, &d.extra)?;
    let [ax, ay, az]: [f64; 3] = serde_json::from_value(axis.clone()).ok()?;
    let [ox, oy, oz]: [String; 3] = serde_json::from_value(origin.clone()).ok()?;
    Some((Vec3::new(len(app, &ox).ok()?, len(app, &oy).ok()?, len(app, &oz).ok()?), Vec3::new(ax, ay, az)))
}

/// Under Rotate's Angle row, while an edited turn keeps its own axis: says so (#17560).
pub fn kept_axis_note(ui: &mut egui::Ui, inputs: &[SelInput], extra: &serde_json::Map<String, Value>) {
    if kept_axis(inputs, extra).is_some() {
        let key = if extra.contains_key("about") { "dialog.move_kept_edge" } else { "dialog.move_kept_axis" };
        let text = solidcraft_ui_text::tr(key);
        ui.add(egui::Label::new(egui::RichText::new(text).color(solidcraft_theme::Tokens::get().text_dim)).wrap());
    }
}

/// Where the triad sits: the pivot carried by the move so far, on the bodies for an edited
/// move's kept turn (Free Move and Translate only).
pub fn triad(app: &SolveApp, d: &Dialog) -> Option<gizmo::Triad> {
    let Kind::Move { mode, x, y, z, .. } = &d.kind else { return None };
    if !matches!(*mode, FREE | TRANSLATE) || bodies_empty(d) {
        return None;
    }
    let t = Vec3::new(len(app, x).unwrap_or(0.0), len(app, y).unwrap_or(0.0), len(app, z).unwrap_or(0.0));
    let on_body = if kept_turn(d) { moved_centre(app, d) } else { None };
    let center = match on_body {
        Some(at) => at,
        None => pivot(app, d)? + t,
    };
    Some(gizmo::Triad { center, translate: true, rotate: *mode == FREE })
}

/// Rotate's ring lies on the selected axis, level with the picked bodies' centre.
pub(crate) fn rotation_axis(app: &SolveApp, d: &Dialog) -> Option<(Vec3, Vec3)> {
    if !matches!(d.kind, Kind::Move { mode: ROTATE, .. }) {
        return None;
    }
    let centre = bodies_center(app, d)?;
    let (origin, axis) = match items(d, 1).first() {
        Some(s) => axis_line(app, d, s)?,
        None => kept_axis_line(app, d)?,
    };
    let axis = axis.normalized()?;
    Some((origin + axis * (centre - origin).dot(axis), axis))
}

/// Apply a triad drag to the dialog's values (added to the values when the drag began).
/// Which value the canvas box shows and types into, for the types that translate: X, Y or Z
/// (0 to 2), or, in Free Move, the angle about X, Y or Z (3 to 5). Kept in `extra` as
/// `value_field`; X at first.
pub fn value_field(d: &Dialog) -> usize {
    let Kind::Move { mode, .. } = &d.kind else { return 0 };
    let n = fields(*mode);
    d.extra.get("value_field").and_then(Value::as_u64).map_or(0, |f| f as usize).min(n.saturating_sub(1))
}

/// How many values the box can show for a move type (see [`value_field`]).
fn fields(mode: usize) -> usize {
    match mode {
        FREE => 6,
        TRANSLATE | POINT_TO_POSITION => 3,
        _ => 1,
    }
}

/// Show the next (`step` 1) or previous (`step` -1) value in the box, round the type's values.
pub fn step_value_field(d: &mut Dialog, step: i64) {
    let Kind::Move { mode, .. } = &d.kind else { return };
    let n = fields(*mode) as i64;
    let next = (value_field(d) as i64 + step).rem_euclid(n.max(1));
    d.extra.insert("value_field".into(), json!(next));
}

/// Show the value `handle` drives in the box: the distance along an arrow, the angle of a ring
/// (#4776). A plane square or the ball moves along two or three axes and leaves it as it is.
pub fn show_handle_value(d: &mut Dialog, handle: crate::gizmo::Handle) {
    let field = match handle {
        crate::gizmo::Handle::Axis(i) => Some(i.min(2)),
        crate::gizmo::Handle::Ring(i) => Some(3 + i.min(2)),
        _ => None,
    };
    if let (Some(f), Kind::Move { mode, .. }) = (field, &d.kind)
        && f < fields(*mode)
    {
        d.extra.insert("value_field".into(), json!(f));
    }
}

pub fn drag(app: &SolveApp, d: &mut Dialog, g: Drag) {
    show_handle_value(d, g.handle);
    let snapshot = |d: &mut Dialog| {
        let Kind::Move { x, y, z, rx, ry, rz, .. } = &d.kind else { return };
        let v = [len(app, x), len(app, y), len(app, z), ang(app, rx), ang(app, ry), ang(app, rz)].map(|r| r.unwrap_or(0.0));
        d.extra.insert("drag0".into(), json!(v));
    };
    if g.started || !d.extra.contains_key("drag0") {
        snapshot(d);
        d.extra.remove("ring_set");
    }
    // Once a ring turns, not on the press: a click on a ring changes nothing.
    if let Change::Rotate { axis, angle } = g.change
        && angle != 0.0
        && !d.extra.contains_key("ring_set")
    {
        turn_on_centre(app, d);
        settle_ring(app, d, axis);
        d.extra.insert("ring_set".into(), json!(true));
        snapshot(d);
    }
    // Once the ring has turned, a drag back to where it started is a real zero.
    let turning = d.extra.contains_key("ring_set");
    let v0: [f64; 6] = d.extra.get("drag0").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    let Kind::Move { x, y, z, rx, ry, rz, .. } = &mut d.kind else { return };
    match g.change {
        Change::Translate(t) => {
            *x = crate::length_input::model(&app.session.doc, v0[0] + t.x);
            *y = crate::length_input::model(&app.session.doc, v0[1] + t.y);
            *z = crate::length_input::model(&app.session.doc, v0[2] + t.z);
        }
        Change::Rotate { axis, angle } => {
            let i = axis.min(2);
            if let Some(slot) = [rx, ry, rz].into_iter().nth(i)
                && turning
            {
                *slot = deg(gizmo::within_turn(v0[3 + i] + angle));
            }
        }
    }
    if g.done {
        for key in ["drag0", "ring_set"] {
            d.extra.remove(key);
        }
    }
}

/// Before a ring's first turn: let its angle add, or start it over on the turns so far.
fn settle_ring(app: &SolveApp, d: &mut Dialog, ring: usize) {
    let Ok(a) = angles(app, d) else { return };
    match gizmo::ring_order(a, order(d), ring) {
        Some(o) => {
            d.extra.insert("order".into(), json!(o));
        }
        None => bake_turn(app, d),
    }
}

/// The Free Move angles (X, Y, Z) that make the same turn as a turn of `angle` about `axis`,
/// in the dialog's default order (Z applied last, then Y, then X: [`gizmo::ANGLE_ORDER`]),
/// so an edited move shows angles that re-make the turn the feature stored.
pub(crate) fn euler_from_axis_angle(axis: Vec3, angle: f64) -> (f64, f64, f64) {
    let axis = axis.normalized().unwrap_or(Vec3::Z);
    let m = gizmo::rotation(axis, angle);
    // R = Rz(rz) * Ry(ry) * Rx(rx): row 2 is (-sin ry, cos ry sin rx, cos ry cos rx).
    let ry = (-m[2][0]).clamp(-1.0, 1.0).asin();
    if m[2][1].abs() + m[2][2].abs() < 1e-9 {
        // Half a turn about X or Y: only rx + rz shows; keep rz at zero.
        ((-m[1][2]).atan2(m[1][1]), ry, 0.0)
    } else {
        (m[2][1].atan2(m[2][2]), ry, m[1][0].atan2(m[0][0]))
    }
}

/// The dialog for editing a move: Free Move with its translation and, about a world axis
/// through its saved pivot, its angle (another axis is shown as the Free Move angles that make
/// the same turn, or kept as it is when the angle does not evaluate to a number).
pub fn for_feature(
    doc: &solidcraft_engine::doc::Document,
    d: &mut Dialog,
    feature: &solidcraft_engine::doc::FeatureKind,
    frame: &solidcraft_engine::doc::Mat,
) {
    let solidcraft_engine::doc::FeatureKind::Move { bodies, translate, rotate_axis, rotate_about, origin, angle, copy, .. } = feature else { return };
    let mut translate = translate.clone();
    let mut origin = origin.clone();
    let mut axis = rotate_axis.map(Vec3::from).unwrap_or(Vec3::Z);
    if !solidcraft_engine::doc::is_identity(frame) {
        solidcraft_engine::doc::map_vector_expr(frame, &mut translate);
        if angle.is_some() || origin.is_some() {
            let pivot = origin.get_or_insert_with(|| solidcraft_engine::doc::point_expr(Vec3::ZERO));
            solidcraft_engine::doc::map_point_expr(frame, pivot);
        }
        axis = solidcraft_engine::doc::apply_vector(frame, axis);
    }
    let world = [Vec3::X, Vec3::Y, Vec3::Z].iter().position(|w| axis.cross(*w).len() < 1e-9 && axis.dot(*w) > 0.0);
    // No translation, and none that a parameter could bring back: Rotate does not keep one.
    let still = translate
        .iter()
        .all(|e| solidcraft_engine::doc::expr::references(e).is_empty() && doc.eval(e, ValueKind::Length).is_ok_and(|v| v.abs() < 1e-12));
    // The edge or face a turn was picked about (#17974), kept as saved. In a placed component the
    // dialog works in world space while the reference stays as saved, so it is not carried there.
    let about = rotate_about.as_ref().filter(|_| solidcraft_engine::doc::is_identity(frame)).and_then(|g| serde_json::to_value(g).ok());
    let mode = if angle.is_some() && (world.is_none() || about.is_some()) && still { ROTATE } else { FREE };
    let [x, y, z] = translate.map(String::from);
    let mut k = kind(mode, doc);
    if let (Some(a), Kind::Move { angle: ka, .. }) = (angle, &mut k)
        && mode == ROTATE
    {
        // One turn about an axis that is not X, Y or Z, or about a picked edge or face: Rotate shows
        // its angle as typed, and keeps the stored axis, pivot and edge until another axis is
        // picked (#17560). Free Move could show it only as three angles that make the same turn,
        // losing an expression such as `-tent`, and would drop the edge the turn follows.
        *ka = a.to_string();
        d.extra.insert("axis".into(), json!([axis.x, axis.y, axis.z]));
        d.extra.insert("origin".into(), json!(origin.unwrap_or_else(|| solidcraft_engine::doc::point_expr(Vec3::ZERO))));
        if let Some(about) = about {
            d.extra.insert("about".into(), about);
        }
    } else if let Kind::Move { x: kx, y: ky, z: kz, rx, ry, rz, .. } = &mut k {
        (*kx, *ky, *kz) = (x, y, z);
        if let Some(a) = angle {
            let dir = axis;
            match world {
                Some(0) => *rx = a.to_string(),
                Some(1) => *ry = a.to_string(),
                Some(_) => *rz = a.to_string(),
                None => match doc.eval(a, ValueKind::Angle) {
                    Ok(th) if th.is_finite() => {
                        let (ax, ay, az) = euler_from_axis_angle(dir, th);
                        (*rx, *ry, *rz) = (deg(ax), deg(ay), deg(az));
                    }
                    _ => {
                        d.extra.insert("axis".into(), json!([dir.x, dir.y, dir.z]));
                        d.extra.insert("angle".into(), json!(a));
                    }
                },
            }
        }
        if angle.is_some() || origin.is_some() {
            let origin = origin.unwrap_or_else(|| solidcraft_engine::doc::point_expr(Vec3::ZERO));
            let pivot = origin.iter().map(|e| doc.eval(e, solidcraft_engine::doc::expr::Kind::Length)).collect::<Result<Vec<_>, _>>();
            if let Ok(pivot) = pivot {
                d.extra.insert("pivot".into(), json!(pivot));
            }
            d.extra.insert("origin".into(), json!(origin));
        }
    }
    d.kind = k;
    d.inputs = inputs(mode);
    if let Some(inp) = d.inputs.first_mut() {
        inp.items = bodies.iter().map(|n| Sel::Body { name: n.to_string() }).collect();
    }
    if *copy {
        d.extra.insert("copy".into(), json!(true));
    }
}

#[cfg(test)]
#[path = "dialogs_move_tests.rs"]
mod tests;
