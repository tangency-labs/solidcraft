//! Dialogs for the surface tools and face splitting (#19): Surface Extrude, Revolve, Sweep and
//! Loft, Offset and Ruled Surface, Blend and Boundary Surface, Patch, Stitch, Thicken, Extend, Trim, Untrim
//! and Split Face. Each has a live preview and Edit Feature.

use crate::dialogs::put;
use crate::ui_kit::{choice_row, expr_row, hint_row};
use serde_json::{Map, Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::doc::{AxisRef, Direction, FaceAt, FeatureKind, Mat, PlaneRef, Position, ProfileSel, RuledCorners, RuledMode};
use solidcraft_engine::geom::Vec3;

use crate::SolveApp;
use crate::dialogs::{Kind, OP_LABELS, OPS, combo, form_row, op_index};
use crate::selection::{
    AXES, AXIS_PICKS, BODIES, CURVES, EDGES, FACES, PLANES, PROFILES, SelInput, SelectionExt as _, VERTICES, items, point_value as pt,
};
use crate::ui_kit;

crate::dialog_descriptor::dialog_descriptors! {
    /// A surface tool dialog's values.
    #[derive(Clone, Debug, PartialEq)]
    pub enum Surf {
        Offset("surface.offset", "OFFSET SURFACE", [("Faces", FACES, true)],
            FeatureKind::SurfaceOffset { distance, chain, .. }) {
            distance: String = "2 mm".into() => distance.to_string(),
            chain: bool = false => *chain,
        } => json!({"distance": distance});
        Stitch("surface.stitch", "STITCH", [("Surfaces", BODIES, true)],
            FeatureKind::Stitch { tolerance, .. }) {
            tolerance: String = String::new() => if tolerance.trim() == "0" { String::new() } else { tolerance.to_string() },
        } => json!({});
        Thicken("surface.thicken", "THICKEN", [("Surfaces", BODIES, true)],
            FeatureKind::Thicken { thickness, symmetric, operation, .. }) {
            thickness: String = "2 mm".into() => thickness.to_string(),
            symmetric: bool = false => *symmetric,
            operation: usize = 0 => op_index(operation),
        } => json!({"thickness": thickness, "symmetric": symmetric, "operation": OPS.get(*operation).copied().unwrap_or("new")});
        Extend("surface.extend", "EXTEND", [("Edges", EDGES, true)],
            FeatureKind::SurfaceExtend { distance, .. }) {
            distance: String = "10 mm".into() => distance.to_string(),
        } => json!({"distance": distance});
        ; custom {
            /// `direction`: an index into [`EXTRUDE_SIDES`].
            Extrude {
                distance: String,
                direction: usize,
            },
            /// `axis`: a stored {origin, dir} axis, used while no axis is picked.
            Revolve {
                angle: String,
                symmetric: bool,
                axis: Option<Value>,
            },
            Sweep(crate::dialogs_sweep::SweepOpts),
            /// `rail_fit`: how the guide rails reshape the sections (an index into the sweep dialog's
            /// rail fits).
            Loft {
                rail_fit: usize,
            },
            /// `continuity`: an index into [`BLEND_CONTINUITY`] for each end, [from, to]; `bulge`: how
            /// full each end leaves (0 is the plain bridge); `stitch`: sew the blend onto both bodies.
            Blend {
                continuity: [usize; 2],
                bulge: [f64; 2],
                stitch: bool,
            },
            /// `continuity`: an index into [`PATCH_CONTINUITY`] (for a loop of sketch curves, how much of
            /// the sketch's plane each side repeats; for picked edges, how the surface meets the faces
            /// beside them); `join`: sew the surface into the body of its edges.
            Boundary {
                continuity: usize,
                join: bool,
            },
            /// `continuity`: an index into [`PATCH_CONTINUITY`] (how a patch on edges meets the faces
            /// beside its loop).
            Patch {
                continuity: usize,
            },
            Unstitch,
            ReverseNormal,
            /// `mode`: an index into [`RULED_MODES`].
            Ruled {
                distance: String,
                mode: usize,
                angle: String,
                flip: bool,
                round: bool,
            },
            /// `direction`: the stored sweep direction of trimming curves (none: the sketch's normal).
            Trim {
                extend: bool,
                stitch: bool,
                direction: Option<Vec3>,
            },
            /// `holes`: an index into [`UNTRIM_HOLES`].
            Untrim {
                holes: usize,
                outer: bool,
                in_place: bool,
            },
            SplitFace,
        }
    }
}

impl Surf {
    pub fn title(&self) -> &'static str {
        match self {
            Surf::Extrude { .. } => "SURFACE EXTRUDE",
            Surf::Revolve { .. } => "SURFACE REVOLVE",
            Surf::Sweep(_) => "SURFACE SWEEP",
            Surf::Loft { .. } => "SURFACE LOFT",
            Surf::Blend { .. } => "BLEND SURFACE",
            Surf::Boundary { .. } => "BOUNDARY SURFACE",
            Surf::Patch { .. } => "PATCH",
            Surf::Unstitch => "UNSTITCH",
            Surf::ReverseNormal => "REVERSE NORMAL",
            Surf::Ruled { .. } => "RULED SURFACE",
            Surf::Trim { .. } => "TRIM",
            Surf::Untrim { .. } => "UNTRIM",
            Surf::SplitFace => "SPLIT FACE",
            _ => self.simple_metadata().map_or("", |(_, title)| title),
        }
    }

    /// The value on the canvas.
    pub fn primary(&mut self) -> Option<(&'static str, ValueKind, &mut String)> {
        Some(match self {
            Surf::Thicken { thickness, .. } => ("Thickness", ValueKind::Length, thickness),
            Surf::Extend { distance } | Surf::Extrude { distance, .. } | Surf::Offset { distance, .. } | Surf::Ruled { distance, .. } => {
                ("Distance", ValueKind::Length, distance)
            }
            Surf::Revolve { angle, .. } => ("Angle", ValueKind::Angle, angle),
            _ => return None,
        })
    }
}

/// How a patch on edges meets the faces beside its loop (labels, and the `continuity` parameter).
const PATCH_CONTINUITY: [(&str, &str); 3] = [("Position (G0)", "g0"), ("Tangent (G1)", "g1"), ("Curvature (G2)", "g2")];

/// How much of the surface at its edges each end of a blend repeats (labels, and the
/// `continuity` words).
const BLEND_CONTINUITY: [(&str, &str); 4] =
    [("Position (G0)", "g0"), ("Tangent (G1)", "g1"), ("Curvature (G2)", "g2"), ("Curvature change (G3)", "g3")];

/// What a boundary surface becomes: a body of its own, or part of the body of its edges.
const BOUNDARY_OPS: [&str; 2] = ["New Body", "Join"];

/// Which holes Untrim takes away: every one, the picked ones, or none (the outer boundary only).
const UNTRIM_HOLES: [&str; 3] = ["All", "Picked", "None"];

/// The sides a surface extrude goes to (labels, and the `direction` parameter).
const EXTRUDE_SIDES: [(&str, &str); 3] = [("One Side", "positive"), ("Symmetric", "symmetric"), ("Flipped", "negative")];

/// The ways a ruled surface leaves its edges (labels, and the `mode` parameter).
const RULED_MODES: [(&str, &str); 4] = [("Normal", "normal"), ("Tangent", "tangent"), ("Angle", "angle"), ("Direction", "direction")];

fn inputs(k: &Surf) -> Vec<SelInput> {
    match k {
        Surf::Extrude { .. } => vec![SelInput::new("Profiles or curves", PROFILES | CURVES, true)],
        Surf::Revolve { .. } => vec![SelInput::new("Profiles or curves", PROFILES | CURVES, true), SelInput::new("Axis", AXIS_PICKS, false)],
        Surf::Sweep(_) => {
            let [rail, rail2] = crate::dialogs_sweep::rail_inputs();
            vec![
                SelInput::new("Profiles or curves", PROFILES | CURVES, true),
                SelInput::new("Path", CURVES, true),
                crate::dialogs_sweep::direction_input(),
                rail,
                rail2,
            ]
        }
        Surf::Loft { .. } => {
            let [rail, rail2] = crate::dialogs_sweep::rail_inputs();
            vec![SelInput::new("Sections", PROFILES | CURVES, true), rail, rail2]
        }
        Surf::Blend { .. } => vec![SelInput::new("From edges", EDGES, true), SelInput::new("To edges", EDGES, true)],
        Surf::Boundary { .. } => vec![SelInput::new("Curves", CURVES | EDGES, true), SelInput::new("Cross curves", CURVES | EDGES, true)],
        Surf::Patch { .. } => vec![SelInput::new("Boundary", PROFILES | EDGES, true), SelInput::new("Pass through", VERTICES | FACES, true)],
        Surf::Unstitch => vec![SelInput::new("Body, or faces to take out", BODIES | FACES, true)],
        Surf::ReverseNormal => vec![SelInput::new("Surface, or faces to turn over", BODIES | FACES, true)],
        Surf::Ruled { .. } => {
            vec![SelInput::new("Edges", EDGES, true), SelInput::new("Reference face", FACES, false), SelInput::new("Direction", AXES | EDGES, false)]
        }
        Surf::Trim { .. } => vec![
            SelInput::new("Surface", BODIES, false),
            SelInput::new("Trimming tool (plane, face or curves)", PLANES | FACES | CURVES, true),
            SelInput::new("Keep (points on it)", FACES, true).places(),
            SelInput::new("Other surface (mutual trim)", BODIES, false),
            SelInput::new("Keep of the other surface", FACES, true).places(),
        ],
        Surf::Untrim { .. } => vec![SelInput::new("Face", FACES, false), SelInput::new("Holes (edges on them)", EDGES, true)],
        Surf::SplitFace => vec![SelInput::new("Faces to split", FACES, true), SelInput::new("Splitting tool", PLANES | FACES, false)],
        _ => k.simple_inputs(),
    }
}

/// The dialog for a surface command.
pub fn start(app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    let k = match id {
        "surface.extrude" => Surf::Extrude { distance: crate::dialogs::length_default(&app.session, "10"), direction: 0 },
        "surface.revolve" => Surf::Revolve { angle: "360 deg".into(), symmetric: false, axis: None },
        "surface.sweep" => Surf::Sweep(crate::dialogs_sweep::SweepOpts::default()),
        "surface.loft" => Surf::Loft { rail_fit: 0 },
        "surface.blend" => Surf::Blend { continuity: [1, 1], bulge: [0.0, 0.0], stitch: false },
        "surface.boundary" => Surf::Boundary { continuity: 1, join: false },
        "surface.patch" => Surf::Patch { continuity: 0 },
        "surface.unstitch" => Surf::Unstitch,
        "surface.reverse_normal" => Surf::ReverseNormal,
        "surface.ruled" => {
            Surf::Ruled { distance: crate::dialogs::length_default(&app.session, "10"), mode: 0, angle: "5 deg".into(), flip: false, round: false }
        }
        "surface.trim" => Surf::Trim { extend: false, stitch: false, direction: None },
        "surface.untrim" => Surf::Untrim { holes: 0, outer: false, in_place: false },
        "solid.split_face" => Surf::SplitFace,
        _ => Surf::simple_start(id)?.0,
    };
    let i = inputs(&k);
    Some((crate::length_input::defaults(&app.session.doc, Kind::Surface(k)), i))
}

/// The rows after the inputs; true when Enter was pressed in a field.
pub fn rows(ui: &mut egui::Ui, k: &mut Surf) -> bool {
    let mut enter = false;
    match k {
        Surf::Stitch { tolerance } => {
            enter |= hint_row(ui, "Tolerance", tolerance, crate::i18n::tr("ui.stitch_tolerance_hint"));
        }
        Surf::Thicken { thickness, symmetric, operation } => {
            enter |= expr_row(ui, "Thickness", thickness);
            form_row(ui, "Direction", |ui| {
                let mut dir = usize::from(*symmetric);
                ui_kit::segmented(ui, &mut dir, &["One Side", "Symmetric"], Default::default());
                *symmetric = dir == 1;
            });
            choice_row(ui, "Operation", "th_op", operation, &OP_LABELS);
        }
        Surf::Extend { distance } => {
            enter |= expr_row(ui, "Distance", distance);
        }
        Surf::Revolve { angle, symmetric, .. } => {
            enter |= expr_row(ui, "Angle", angle);
            form_row(ui, "Direction", |ui| {
                let mut dir = usize::from(*symmetric);
                ui_kit::segmented(ui, &mut dir, &["One Side", "Symmetric"], Default::default());
                *symmetric = dir == 1;
            });
        }
        Surf::Offset { distance, chain } => {
            enter |= expr_row(ui, "Distance", distance);
            ui_kit::options(ui, "offset_options", &mut [("Tangent chain", chain)]);
        }
        Surf::Ruled { distance, mode, angle, flip, round } => {
            enter |= expr_row(ui, "Distance", distance);
            choice_row(ui, "Type", "ru_mode", mode, &RULED_MODES.map(|(label, _)| label));
            if RULED_MODES.get(*mode).is_some_and(|(_, m)| *m == "angle") {
                enter |= expr_row(ui, "Angle", angle);
            }
            form_row(ui, "Corners", |ui| {
                let mut corner = usize::from(*round);
                ui_kit::segmented(ui, &mut corner, &["Mitre", "Round"], Default::default());
                *round = corner == 1;
            });
            ui_kit::options(ui, "ruled_options", &mut [("Flip", flip)]);
        }
        Surf::Extrude { distance, direction } => {
            enter |= expr_row(ui, "Distance", distance);
            form_row(ui, "Direction", |ui| {
                ui_kit::segmented(ui, direction, &EXTRUDE_SIDES.map(|(label, _)| label), Default::default());
            });
        }
        Surf::Untrim { holes, outer, in_place } => {
            choice_row(ui, "Holes", "ut_holes", holes, &UNTRIM_HOLES);
            ui_kit::options(ui, "untrim_options", &mut [("Restore outer boundary", outer), ("In place (surfaces)", in_place)]);
        }
        Surf::Patch { continuity } => {
            form_row(ui, "Continuity", |ui| {
                ui_kit::segmented(ui, continuity, &PATCH_CONTINUITY.map(|(label, _)| label), Default::default());
            });
        }
        Surf::Blend { continuity, bulge, stitch } => {
            let [from, to] = continuity;
            let [from_bulge, to_bulge] = bulge;
            form_row(ui, "Start continuity", |ui| {
                combo(ui, "bl_cont0", &BLEND_CONTINUITY.map(|(label, _)| label), from);
            });
            form_row(ui, "Start bulge", |ui| {
                ui_kit::scalar_field(ui, from_bulge, -0.9..=3.0);
            });
            form_row(ui, "End continuity", |ui| {
                combo(ui, "bl_cont1", &BLEND_CONTINUITY.map(|(label, _)| label), to);
            });
            form_row(ui, "End bulge", |ui| {
                ui_kit::scalar_field(ui, to_bulge, -0.9..=3.0);
            });
            ui_kit::options(ui, "blend_options", &mut [("Stitch to both bodies", stitch)]);
        }
        Surf::Boundary { continuity, join } => {
            form_row(ui, "Continuity", |ui| {
                ui_kit::segmented(ui, continuity, &PATCH_CONTINUITY.map(|(label, _)| label), Default::default());
            });
            form_row(ui, "Operation", |ui| {
                let mut op = usize::from(*join);
                combo(ui, "bo_op", &BOUNDARY_OPS, &mut op);
                *join = op == 1;
            });
        }
        Surf::Trim { extend, stitch, .. } => {
            ui_kit::options(ui, "trim_options", &mut [("Extend to meet", extend), ("Stitch the kept pieces", stitch)]);
        }
        Surf::Sweep(opts) => enter |= crate::dialogs_sweep::rows(ui, opts, "ss"),
        Surf::Loft { rail_fit } => crate::dialogs_sweep::rail_fit_row(ui, "sl", rail_fit),
        Surf::SplitFace | Surf::Unstitch | Surf::ReverseNormal => {}
    }
    enter
}

/// One body and points on the picked faces of it (none: a whole body picked).
fn body_and_faces(picks: &[Sel]) -> Result<(String, Vec<Value>), String> {
    let body_of = |x: &Sel| match x {
        Sel::Body { name } | Sel::Face { body: name, .. } => Some(name.clone()),
        _ => None,
    };
    let body = picks.iter().find_map(body_of).ok_or("pick a body or some of its faces")?;
    if picks.iter().any(|x| body_of(x).as_ref() != Some(&body)) {
        return Err("pick faces of one body".into());
    }
    let faces = picks.face_points().into_iter().map(pt).collect();
    Ok((body, faces))
}

/// The body of picked edges (all of one body) and a point on each edge.
fn body_and_edges(picks: &[Sel]) -> Result<(String, Vec<Value>), String> {
    let body_of = |x: &Sel| if let Sel::Edge { body, .. } = x { Some(body.clone()) } else { None };
    let body = picks.iter().find_map(body_of).ok_or("pick edges of a body")?;
    if picks.iter().any(|x| body_of(x).as_ref() != Some(&body)) {
        return Err("pick edges of one body".into());
    }
    let edges = picks.iter().filter_map(|x| if let Sel::Edge { point, .. } = x { Some(pt(*point)) } else { None }).collect();
    Ok((body, edges))
}

/// The sketch, profiles and curves of a surface builder's picks (all in one sketch).
fn sheet_picks(app: &SolveApp, picks: &[Sel]) -> Result<Value, String> {
    let profiles: Vec<(u64, usize)> = picks.profiles();
    let curves: Vec<String> = picks.curve_ids();
    let sketch = match profiles.first() {
        Some((sk, _)) => *sk,
        None => crate::dialogs::picks_sketch(app, picks, None).ok_or("the curves must be in one sketch")?,
    };
    if profiles.iter().any(|(sk, _)| *sk != sketch) {
        return Err("the profiles and curves must be in one sketch".into());
    }
    let mut p = json!({ "sketch": sketch });
    if !curves.is_empty() {
        p["curves"] = json!(curves);
    }
    if !profiles.is_empty() {
        p["profiles"] = json!(profiles.iter().map(|(_, i)| *i).collect::<Vec<_>>());
    }
    Ok(p)
}

/// A surface builder's stored picks as selections again: its profiles, then its curves.
fn sheet_sels(app: &SolveApp, sketch: u64, profiles: Option<&ProfileSel>, curves: &[String]) -> Vec<Sel> {
    let st = app.session.world_state();
    let mut sels: Vec<Sel> = match (profiles, st.sketch(sketch)) {
        (Some(sel), Some(ss)) => crate::dialogs::profile_indices(ss, sel).into_iter().map(|index| Sel::Profile { sketch, index }).collect(),
        _ => Vec::new(),
    };
    sels.extend(curves.iter().map(|id| crate::selection::curve_pick(id, sketch, app.session.active_sketch)));
    sels
}

/// A splitting or trimming tool: a plane by name, a planar face as its plane, or any other
/// face as {body, point} (its surface extended).
fn tool(app: &SolveApp, sel: Option<&Sel>) -> Result<(&'static str, Value), String> {
    match sel {
        Some(Sel::Plane { name }) => Ok(("plane", json!(name))),
        Some(Sel::Face { body, index, point }) => match crate::dialogs::planar_face(&app.session, body, *index) {
            Some((_, n)) => Ok(("plane", json!({"origin": pt(*point), "normal": pt(n)}))),
            None => Ok(("tool", json!({"body": body, "point": pt(*point)}))),
        },
        _ => Err("pick a plane or a face as the tool".into()),
    }
}

/// One curve of a boundary surface as the dialog picks it: sketch curves of one sketch, or edges
/// of one body, that meet end to end.
#[derive(Clone, Debug, PartialEq)]
enum Chain {
    Sketch { sketch: u64, ids: Vec<String> },
    Edges { body: String, points: Vec<Vec3> },
}

/// The picks grouped into curves: picks of one sketch (or one body) whose ends meet run on into
/// one chain, in the order they were first picked; anything else is a curve of its own.
fn chains(app: &SolveApp, picks: &[Sel]) -> Result<Vec<Chain>, String> {
    let st = app.session.world_state();
    // Each pick: what it belongs to (a sketch or a body), its ends, and how it joins a chain.
    let mut parts: Vec<(Chain, [Vec3; 2])> = Vec::with_capacity(picks.len());
    for x in picks {
        match x {
            Sel::SketchCurve { id, .. } => {
                let sketch = crate::dialogs::picks_sketch(app, std::slice::from_ref(x), None).ok_or("a picked curve is no longer in a sketch")?;
                let ss = st.sketch(sketch).ok_or("a picked curve's sketch has not been solved")?;
                let ends = if let Some(ci) = ss.sketch.curve_index(id) {
                    let segs = ss.sketch.segs(ci);
                    match (segs.first(), segs.last()) {
                        (Some(a), Some(b)) => [ss.plane.to_world(a.start()), ss.plane.to_world(b.end())],
                        _ => return Err(format!("curve `{id}` has no geometry")),
                    }
                } else {
                    let w = ss.sketch.wire_index(id).and_then(|wi| ss.sketch.wires.get(wi)).ok_or("a picked curve is no longer in a sketch")?;
                    match (w.pts.first(), w.pts.last()) {
                        (Some(a), Some(b)) => [*a, *b],
                        _ => return Err(format!("curve `{id}` has no geometry")),
                    }
                };
                parts.push((Chain::Sketch { sketch, ids: vec![id.clone()] }, ends));
            }
            Sel::Edge { body, index, point } => {
                let b = st.body(body).ok_or("a picked edge's body is gone")?;
                let m = b.mesh();
                let e = m.edges.get(*index).ok_or("a picked edge is gone")?;
                let (Some(a), Some(z)) = (e.first(), e.last()) else { return Err("a picked edge has no points".into()) };
                parts.push((Chain::Edges { body: body.clone(), points: vec![*point] }, [*a, *z]));
            }
            _ => return Err("pick sketch curves or edges".into()),
        }
    }
    let same_source = |a: &Chain, b: &Chain| match (a, b) {
        (Chain::Sketch { sketch: x, .. }, Chain::Sketch { sketch: y, .. }) => x == y,
        (Chain::Edges { body: x, .. }, Chain::Edges { body: y, .. }) => x == y,
        _ => false,
    };
    let meet = |a: &[Vec3; 2], b: &[Vec3; 2]| a.iter().any(|p| b.iter().any(|q| p.dist(*q) <= 1e-6 * (1.0 + p.len())));
    // Union the picks that meet, keeping the first pick of each chain in front.
    let mut group: Vec<usize> = (0..parts.len()).collect();
    fn root(group: &[usize], mut i: usize) -> usize {
        while let Some(&g) = group.get(i) {
            if g == i {
                break;
            }
            i = g;
        }
        i
    }
    for i in 0..parts.len() {
        for j in 0..i {
            let (Some((ci, ei)), Some((cj, ej))) = (parts.get(i), parts.get(j)) else { continue };
            if same_source(ci, cj) && meet(ei, ej) {
                let (ri, rj) = (root(&group, i), root(&group, j));
                if let Some(g) = group.get_mut(ri.max(rj)) {
                    *g = ri.min(rj);
                }
            }
        }
    }
    let mut out: Vec<(usize, Chain)> = Vec::new();
    for (i, (part, _)) in parts.into_iter().enumerate() {
        let r = root(&group, i);
        match out.iter_mut().find(|(k, _)| *k == r) {
            Some((_, Chain::Sketch { ids, .. })) => {
                if let Chain::Sketch { ids: more, .. } = part {
                    ids.extend(more);
                }
            }
            Some((_, Chain::Edges { points, .. })) => {
                if let Chain::Edges { points: more, .. } = part {
                    points.extend(more);
                }
            }
            None => out.push((r, part)),
        }
    }
    Ok(out.into_iter().map(|(_, c)| c).collect())
}

/// The command a surface dialog makes.
pub fn commands(app: &SolveApp, k: &Surf, inputs: &[SelInput], extra: &Map<String, Value>) -> Result<Vec<(String, Value)>, String> {
    let need = |i: usize, what: &str| if items(inputs, i).is_empty() { Err(format!("select {what} first")) } else { Ok(()) };
    let (cmd, mut p) = match k {
        Surf::Extrude { distance, direction } => {
            need(0, "profiles or curves to extrude")?;
            let mut p = sheet_picks(app, items(inputs, 0))?;
            p["distance"] = json!(distance);
            p["direction"] = json!(EXTRUDE_SIDES.get(*direction).map_or("positive", |(_, d)| *d));
            ("surface.extrude", p)
        }
        Surf::Revolve { angle, symmetric, axis } => {
            need(0, "profiles or curves to revolve")?;
            let mut p = sheet_picks(app, items(inputs, 0))?;
            // Same as a solid revolve: a bare curve id is resolved on this profile sketch, where
            // it can name a different line than the one the pick highlights (#7452, #3726).
            let profile_sketch = p.get("sketch").and_then(Value::as_u64);
            p["axis"] = match items(inputs, 1).first() {
                Some(Sel::Axis { name }) => json!(name),
                Some(curve @ Sel::SketchCurve { .. }) => crate::dialogs::sketch_curve_axis(app, curve, profile_sketch)?,
                Some(x) => crate::dialogs::axis_param(app, x)?,
                None => axis.clone().ok_or("select an axis")?,
            };
            p["angle"] = json!(angle);
            if *symmetric {
                p["symmetric"] = json!(true);
            }
            ("surface.revolve", p)
        }
        Surf::Sweep(opts) => {
            need(0, "profiles or curves to sweep")?;
            need(1, "the path")?;
            let mut p = sheet_picks(app, items(inputs, 0))?;
            let path: Vec<String> = items(inputs, 1).curve_ids();
            p["path_sketch"] = json!(
                crate::dialogs::picks_sketch(app, items(inputs, 1), p["sketch"].as_u64()).ok_or("the path curves must be in one other sketch")?
            );
            p["path"] = json!(path);
            crate::dialogs_sweep::params(app, opts, items(inputs, 2).first(), [items(inputs, 3), items(inputs, 4)], &mut p)?;
            ("surface.sweep", p)
        }
        Surf::Loft { rail_fit } => {
            need(0, "the sections")?;
            // Each picked profile is a section, and so is each chain of picked curves (those of
            // one sketch that meet end to end), in the order of their first pick: two sections
            // can come from one sketch (#8221).
            let picks = items(inputs, 0);
            let curves: Vec<Sel> = picks.iter().filter(|x| matches!(x, Sel::SketchCurve { .. })).cloned().collect();
            let mut chained: Vec<Option<Chain>> = chains(app, &curves)?.into_iter().map(Some).collect();
            let mut sections: Vec<Value> = Vec::new();
            let mut sketches: Vec<u64> = Vec::new();
            for x in picks {
                match x {
                    Sel::Profile { sketch, index } => {
                        sections.push(json!({"sketch": sketch, "profiles": [index]}));
                        sketches.push(*sketch);
                    }
                    Sel::SketchCurve { id, .. } => {
                        let sk = crate::dialogs::picks_sketch(app, std::slice::from_ref(x), None).ok_or("a picked curve is no longer in a sketch")?;
                        let holds = |c: &Option<Chain>| matches!(c, Some(Chain::Sketch { sketch, ids }) if *sketch == sk && ids.contains(id));
                        if let Some(Some(Chain::Sketch { sketch, ids })) = chained.iter_mut().find(|c| holds(c)).map(Option::take) {
                            sections.push(json!({"sketch": sketch, "curves": ids}));
                            sketches.push(sketch);
                        }
                    }
                    _ => {}
                }
            }
            sketches.sort_unstable();
            sketches.dedup();
            if sketches.len() < 2 {
                return Err("pick sections in two or more sketches".into());
            }
            let mut p = json!({ "sections": sections });
            crate::dialogs_sweep::rail_params(app, *rail_fit, [items(inputs, 1), items(inputs, 2)], &mut p)?;
            ("surface.loft", p)
        }
        Surf::Blend { continuity, bulge, stitch } => {
            need(0, "the edges the blend leaves")?;
            need(1, "the edges the blend arrives at")?;
            let side = |i: usize| -> Result<Value, String> {
                let (body, edges) = body_and_edges(items(inputs, i))?;
                Ok(json!({"body": body, "edges": edges}))
            };
            let word = |i: usize| BLEND_CONTINUITY.get(i).map_or("g1", |(_, w)| *w);
            let mut p = json!({
                "from": side(0)?,
                "to": side(1)?,
                "continuity": [word(continuity[0]), word(continuity[1])],
                "bulge": bulge,
            });
            if *stitch {
                p["stitch"] = json!(true);
            }
            ("surface.blend", p)
        }
        Surf::Boundary { continuity, join } => {
            need(0, "the curves (sketch curves or edges)")?;
            let word = PATCH_CONTINUITY.get(*continuity).map_or("g0", |(_, w)| *w);
            let across = items(inputs, 1);
            let picks = items(inputs, 0);
            let sketch_ids: Vec<String> = picks.curve_ids();
            // Two to four sketch curves of one sketch and nothing across: the loop patch.
            let looped = across.is_empty() && !*join && sketch_ids.len() == picks.len() && (2..=4).contains(&picks.len());
            match looped.then(|| crate::dialogs::picks_sketch(app, picks, None)).flatten() {
                Some(sk) => {
                    let mut p = json!({"sketch": sk, "curves": sketch_ids});
                    // Orders that differ from side to side (kept from the feature) stand as they are.
                    if !extra.contains_key("continuity") {
                        p["continuity"] = json!(word);
                    }
                    ("surface.boundary", p)
                }
                None => {
                    let curve = |c: &Chain| match c {
                        Chain::Sketch { sketch, ids } => json!({"sketch": sketch, "curves": ids}),
                        Chain::Edges { body, points } => {
                            let mut v = json!({"body": body, "edges": points.iter().map(|p| pt(*p)).collect::<Vec<_>>()});
                            if *continuity > 0 {
                                v["continuity"] = json!(word);
                            }
                            v
                        }
                    };
                    let u: Vec<Value> = chains(app, picks)?.iter().map(curve).collect();
                    let v: Vec<Value> = chains(app, across)?.iter().map(curve).collect();
                    let mut p = json!({"u": u});
                    if !v.is_empty() {
                        p["v"] = json!(v);
                    }
                    if *join {
                        p["operation"] = json!("join");
                    }
                    ("surface.boundary", p)
                }
            }
        }
        Surf::Patch { continuity } => {
            need(0, "a boundary (sketch profiles or a loop of edges)")?;
            let profiles: Vec<(u64, usize)> = items(inputs, 0).profiles();
            if let Some((sk, _)) = profiles.first() {
                let idx: Vec<usize> = profiles.iter().filter(|(s, _)| s == sk).map(|(_, i)| *i).collect();
                ("surface.patch", json!({"sketch": sk, "profiles": idx}))
            } else {
                let edges: Vec<Value> = items(inputs, 0).edge_points().into_iter().map(pt).collect();
                let body = items(inputs, 0).iter().find_map(|x| if let Sel::Edge { body, .. } = x { Some(body.clone()) } else { None });
                let mut p = json!({"edges": edges, "body": body});
                if let Some((_, word)) = PATCH_CONTINUITY.get(*continuity).filter(|_| *continuity > 0) {
                    p["continuity"] = json!(word);
                }
                let points: Vec<Value> = items(inputs, 1)
                    .iter()
                    .filter_map(|x| match x {
                        Sel::Vertex { point, .. } | Sel::Face { point, .. } => Some(pt(*point)),
                        _ => None,
                    })
                    .collect();
                if !points.is_empty() {
                    p["points"] = json!(points);
                }
                ("surface.patch", p)
            }
        }
        Surf::Offset { chain, .. } => {
            need(0, "the faces to offset")?;
            let (body, faces) = body_and_faces(items(inputs, 0))?;
            if faces.is_empty() {
                return Err("pick faces to offset".into());
            }
            let mut p = json!({"body": body, "faces": faces});
            if *chain {
                p["chain"] = json!(true);
            }
            ("surface.offset", p)
        }
        Surf::Ruled { distance, mode, angle, flip, round } => {
            need(0, "the edges to rule from")?;
            let (body, edges) = body_and_edges(items(inputs, 0))?;
            let mode = RULED_MODES.get(*mode).map_or("normal", |(_, m)| *m);
            let mut p = json!({"body": body, "edges": edges, "distance": distance, "mode": mode});
            if mode == "angle" {
                p["angle"] = json!(angle);
            }
            if let Some(Sel::Face { body: fb, point, .. }) = items(inputs, 1).first() {
                if *fb != body {
                    return Err("the reference face must be on the edges' body".into());
                }
                p["face"] = pt(*point);
            }
            // A picked direction, else the one the feature was made with (Edit Feature).
            let kept = extra.get("direction").and_then(|v| serde_json::from_value::<[f64; 3]>(v.clone()).ok()).map(|a| Vec3::new(a[0], a[1], a[2]));
            let dir = match items(inputs, 2).first() {
                Some(x) => Some(crate::dialogs::axis_of(app, x).ok_or("the direction must be an axis, a sketch line or a straight edge")?.1),
                None => kept,
            };
            match (mode, dir) {
                ("angle" | "direction", Some(d)) => p["direction"] = pt(d),
                ("direction", None) => return Err("select a direction (an axis, a sketch line or a straight edge) first".into()),
                _ => {}
            }
            if *flip {
                p["flip"] = json!(true);
            }
            if *round {
                p["corners"] = json!("round");
            }
            ("surface.ruled", p)
        }
        Surf::Unstitch | Surf::ReverseNormal => {
            need(0, "a body or some of its faces")?;
            let (body, faces) = body_and_faces(items(inputs, 0))?;
            let cmd = if matches!(k, Surf::Unstitch) { "surface.unstitch" } else { "surface.reverse_normal" };
            (cmd, json!({"body": body, "faces": faces}))
        }
        Surf::Stitch { tolerance } => {
            need(0, "the surfaces to stitch")?;
            let mut p = json!({ "bodies": items(inputs, 0).body_names() });
            put(&mut p, &[("tolerance", tolerance)]);
            ("surface.stitch", p)
        }
        Surf::Thicken { .. } => {
            need(0, "the surfaces to thicken")?;
            ("surface.thicken", json!({"bodies": items(inputs, 0).body_names()}))
        }
        Surf::Extend { .. } => {
            need(0, "the edges to extend")?;
            let body =
                items(inputs, 0).iter().find_map(|x| if let Sel::Edge { body, .. } = x { Some(body.clone()) } else { None }).unwrap_or_default();
            let edges: Vec<Value> = items(inputs, 0).edge_points().into_iter().map(pt).collect();
            ("surface.extend", json!({"body": body, "edges": edges}))
        }
        Surf::Trim { extend, stitch, direction } => {
            need(0, "the surface to trim")?;
            need(2, "what to keep (points on it)")?;
            let body = items(inputs, 0).body_names().into_iter().next().unwrap_or_default();
            // One point keeps its side, several the regions they pick.
            let points = |i: usize| -> Option<Value> {
                let pts: Vec<Value> =
                    items(inputs, i).iter().filter_map(|x| if let Sel::Face { point, .. } = x { Some(pt(*point)) } else { None }).collect();
                match pts.as_slice() {
                    [] => None,
                    [one] => Some(one.clone()),
                    _ => Some(json!(pts)),
                }
            };
            let mut p = json!({"body": body, "keep": points(2).ok_or("pick what to keep")?});
            if let Some(other) = items(inputs, 3).body_names().into_iter().next() {
                need(4, "what to keep of the other surface")?;
                p["with"] = json!(other);
                p["keep_with"] = points(4).ok_or("pick what to keep of the other surface")?;
                if *extend {
                    p["extend"] = json!(true);
                }
                if *stitch {
                    p["stitch"] = json!(true);
                }
            } else {
                need(1, "the trimming tool")?;
                let picks = items(inputs, 1);
                let curves: Vec<String> =
                    picks.iter().filter_map(|x| if let Sel::SketchCurve { id, .. } = x { Some(id.clone()) } else { None }).collect();
                if curves.is_empty() {
                    let [one] = picks else { return Err("pick one plane or face as the tool, or sketch curves".into()) };
                    let (key, t) = tool(app, Some(one))?;
                    p[key] = t;
                } else {
                    if curves.len() != picks.len() {
                        return Err("trim by sketch curves, or by one plane or face".into());
                    }
                    let sketch = crate::dialogs::picks_sketch(app, picks, None).ok_or("the curves must be in one sketch")?;
                    let mut c = json!({"sketch": sketch, "curves": curves});
                    if let Some(d) = direction {
                        c["direction"] = pt(*d);
                    }
                    p["curves"] = c;
                }
            }
            ("surface.trim", p)
        }
        Surf::Untrim { holes, outer, in_place } => {
            need(0, "the face to untrim")?;
            let (body, point) = items(inputs, 0)
                .first()
                .and_then(|x| if let Sel::Face { body, point, .. } = x { Some((body.clone(), pt(*point))) } else { None })
                .ok_or("pick the face to untrim")?;
            let mut p = json!({"face": {"body": body, "point": point}});
            match holes {
                0 => p["holes"] = json!("all"),
                1 => {
                    let picked: Vec<Value> = items(inputs, 1).edge_points().into_iter().map(pt).collect();
                    if picked.is_empty() {
                        return Err("pick edges of the holes to remove".into());
                    }
                    p["holes"] = json!(picked);
                }
                _ => p["holes"] = json!([]),
            }
            if *outer {
                p["outer"] = json!(true);
            }
            if *in_place {
                p["in_place"] = json!(true);
            }
            ("surface.untrim", p)
        }
        Surf::SplitFace => {
            need(0, "the faces to split")?;
            need(1, "the splitting tool")?;
            let faces: Vec<Value> = items(inputs, 0).face_points().into_iter().map(pt).collect();
            let body = items(inputs, 0).iter().find_map(|x| if let Sel::Face { body, .. } = x { Some(body.clone()) } else { None });
            let (key, t) = tool(app, items(inputs, 1).first())?;
            let mut p = json!({"faces": faces, "body": body});
            p[key] = t;
            ("solid.split_face", p)
        }
    };
    k.fill_simple_params(&mut p);
    if let Value::Object(m) = &mut p {
        // A ruled surface's kept direction is used above, or dropped with its mode.
        for (k, v) in extra.iter().filter(|(k, _)| !(cmd == "surface.ruled" && k.as_str() == "direction")) {
            m.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    Ok(vec![(cmd.to_string(), p)])
}

/// A tool as a selection again: a plane, else the face at the stored point.
fn tool_sel(app: &SolveApp, plane: &PlaneRef, tool: Option<&FaceAt>) -> Option<Sel> {
    match tool {
        Some(t) => crate::dialogs::face_sel(&app.session, t.point.0),
        None => crate::dialogs::plane_sel(&app.session, plane),
    }
}

/// The dialog for editing a surface feature.
/// `frame` maps the feature's component frame to the world, for an axis stored as a line.
pub fn for_feature(app: &SolveApp, kind: &FeatureKind, frame: &Mat) -> Option<(Kind, Vec<SelInput>, Map<String, Value>)> {
    let s = &app.session;
    let edge = |p: &solidcraft_engine::doc::Position| crate::dialogs::edge_sel(s, p.0);
    let (k, fill): (Surf, Vec<Vec<Sel>>) = match kind {
        FeatureKind::SurfaceRevolve { sketch, profiles, curves, axis, angle, angle2, symmetric } => {
            let (pick, stored) = match axis {
                AxisRef::World { axis } => (Some(Sel::Axis { name: axis.to_ascii_uppercase() }), None),
                AxisRef::SketchAxis { axis } => (Some(Sel::Axis { name: axis.to_ascii_lowercase() }), None),
                AxisRef::SketchLine { curve } => (Some(crate::selection::curve_pick(curve, sketch.0, s.active_sketch)), None),
                AxisRef::Construction { name } => (Some(Sel::Axis { name: name.to_string() }), None),
                AxisRef::Line { origin, dir } => (
                    None,
                    Some(json!({
                        "origin": pt(solidcraft_engine::doc::apply_point(frame, origin.0)),
                        "dir": pt(solidcraft_engine::doc::apply_vector(frame, dir.0))
                    })),
                ),
                // An edge or face that is gone stays the reference until another is picked.
                AxisRef::Model { geo } => match crate::dialogs::model_axis_sel(s, geo) {
                    Some(pick) => (Some(pick), None),
                    None => (None, serde_json::to_value(geo).ok()),
                },
            };
            let mut extra = Map::new();
            if let Some(a2) = angle2 {
                extra.insert("angle2".into(), json!(a2));
            }
            let k = Surf::Revolve { angle: angle.to_string(), symmetric: *symmetric, axis: stored };
            let mut ins = inputs(&k);
            for (inp, items) in ins.iter_mut().zip([sheet_sels(app, sketch.0, profiles.as_ref(), curves), pick.into_iter().collect()]) {
                inp.items = items;
            }
            return Some((Kind::Surface(k), ins, extra));
        }
        FeatureKind::SurfaceSweep { sketch, profiles, curves, path_sketch, path, orientation, twist, scale, rails, rail_fit, .. } => {
            let (opts, direction) = crate::dialogs_sweep::SweepOpts::of(orientation, twist, scale, *rail_fit, frame);
            let [rail, rail2] = crate::dialogs_sweep::rail_sels(rails, s.active_sketch);
            (
                Surf::Sweep(opts),
                vec![
                    sheet_sels(app, sketch.0, profiles.as_ref(), curves),
                    path.iter().map(|id| crate::selection::curve_pick(id, path_sketch.0, s.active_sketch)).collect(),
                    direction.into_iter().collect(),
                    rail,
                    rail2,
                ],
            )
        }
        FeatureKind::SurfaceBlend { from, to, continuity, bulge, align, seam, stitch } => {
            let index = |c: &solidcraft_engine::doc::kernel::Continuity| BLEND_CONTINUITY.iter().position(|(_, w)| *w == c.as_str()).unwrap_or(1);
            let k = Surf::Blend { continuity: continuity.each_ref().map(index), bulge: *bulge, stitch: *stitch };
            // What the dialog does not show is kept as it was: the leave directions and the seams.
            let mut extra = Map::new();
            for (key, points) in [("align", align), ("seam", seam)] {
                if let Some([p, q]) = points {
                    extra.insert(key.into(), json!([pt(p.0), pt(q.0)]));
                }
            }
            let mut ins = inputs(&k);
            for (inp, side) in ins.iter_mut().zip([from, to]) {
                inp.items = side.edges.iter().filter_map(edge).collect();
            }
            return Some((Kind::Surface(k), ins, extra));
        }
        FeatureKind::SurfaceBoundary { sketch, curves, orders, crown, samples, .. } => {
            let first = orders.first().copied().unwrap_or(solidcraft_engine::doc::kernel::Continuity::G1);
            let index = PATCH_CONTINUITY.iter().position(|(_, w)| *w == first.as_str());
            // What the dialog does not show is kept as it was: a crown, the stations, and orders
            // that differ from side to side or that the list does not offer (G3).
            let mut extra = Map::new();
            if *crown != 0.0 {
                extra.insert("crown".into(), json!(crown));
            }
            if *samples != solidcraft_engine::doc::BOUNDARY_SAMPLES {
                extra.insert("samples".into(), json!(samples));
            }
            if index.is_none() || orders.iter().any(|o| *o != first) {
                extra.insert("continuity".into(), json!(orders.iter().map(|o| o.as_str()).collect::<Vec<_>>()));
            }
            let k = Surf::Boundary { continuity: index.unwrap_or(1), join: false };
            let mut ins = inputs(&k);
            if let Some(inp) = ins.first_mut() {
                inp.items = curves.iter().map(|id| crate::selection::curve_pick(id, sketch.0, s.active_sketch)).collect();
            }
            return Some((Kind::Surface(k), ins, extra));
        }
        FeatureKind::SurfaceNetwork { u, v, operation } => {
            // A network with a point for a side has nothing to pick it with here.
            if u.iter().chain(v).any(|c| c.point.is_some()) {
                return None;
            }
            let sels = |list: &[solidcraft_engine::doc::NetworkCurve]| -> Vec<Sel> {
                list.iter()
                    .flat_map(|c| {
                        let mut out: Vec<Sel> = c
                            .curves
                            .iter()
                            .map(|id| match c.sketch {
                                Some(k) => crate::selection::curve_pick(id, k.0, s.active_sketch),
                                None => Sel::SketchCurve { id: id.clone(), sketch: None },
                            })
                            .collect();
                        out.extend(c.edges.iter().filter_map(edge));
                        out
                    })
                    .collect()
            };
            let g = u.iter().chain(v).find(|c| c.body.is_some()).map_or(solidcraft_engine::doc::kernel::Continuity::G0, |c| c.continuity);
            let index = PATCH_CONTINUITY.iter().position(|(_, w)| *w == g.as_str()).unwrap_or(0);
            (Surf::Boundary { continuity: index, join: *operation == solidcraft_engine::doc::Operation::Join }, vec![sels(u), sels(v)])
        }
        FeatureKind::SurfaceLoft { sections, rails, rail_fit, .. } => {
            let [rail, rail2] = crate::dialogs_sweep::rail_sels(rails, s.active_sketch);
            let picked = sections.iter().flat_map(|sec| sheet_sels(app, sec.sketch.0, sec.profiles.as_ref(), &sec.curves)).collect();
            (Surf::Loft { rail_fit: crate::dialogs_sweep::rail_fit_index(*rail_fit) }, vec![picked, rail, rail2])
        }
        FeatureKind::SurfaceExtrude { sketch, profiles, curves, extent } => {
            let sels = sheet_sels(app, sketch.0, profiles.as_ref(), curves);
            let direction = match extent.direction {
                Direction::Positive => 0,
                Direction::Symmetric => 1,
                Direction::Negative => 2,
            };
            let k = Surf::Extrude { distance: extent.distance.to_string(), direction };
            let mut ins = inputs(&k);
            if let Some(input) = ins.first_mut() {
                input.items = sels;
            }
            let mut extra = Map::new();
            extra.insert("convention".into(), json!(extent.convention));
            return Some((Kind::Surface(k), ins, extra));
        }
        FeatureKind::Patch { sketch, edges, crown, continuity, points, .. } => {
            let mut sels: Vec<Sel> = edges.iter().filter_map(edge).collect();
            if let Some(sk) = sketch {
                let st = s.world_state();
                if let Some(ss) = st.sketch(sk.0) {
                    sels.extend((0..ss.profiles.len()).map(|index| Sel::Profile { sketch: sk.0, index }));
                }
            }
            let index = PATCH_CONTINUITY.iter().position(|(_, w)| *w == continuity.all.as_str()).unwrap_or(0);
            // What the dialog does not show is kept as it was: the crown, edges with a
            // continuity of their own, and points that are not on a face any more.
            let through: Vec<Sel> = points.iter().filter_map(|p| crate::dialogs::face_sel(s, p.0)).collect();
            let mut extra = Map::new();
            if *crown != 0.0 {
                extra.insert("crown".into(), json!(crown));
            }
            if !continuity.edges.is_empty() {
                extra.insert("continuity".into(), serde_json::to_value(continuity).unwrap_or(Value::Null));
            }
            if through.len() != points.len() {
                extra.insert("points".into(), json!(points.iter().map(|p| pt(p.0)).collect::<Vec<_>>()));
            }
            let k = Surf::Patch { continuity: index };
            let mut ins = inputs(&k);
            for (input, picks) in ins.iter_mut().zip([sels, through]) {
                input.items = picks;
            }
            return Some((Kind::Surface(k), ins, extra));
        }
        FeatureKind::SurfaceOffset { faces, .. } => {
            (Surf::simple_restore(kind)?, vec![faces.iter().filter_map(|p| crate::dialogs::face_sel(s, p.0)).collect()])
        }
        FeatureKind::SurfaceRuled { edges, distance, mode, angle, direction, flip, corners, face, .. } => {
            let k = Surf::Ruled {
                distance: distance.to_string(),
                mode: match mode {
                    RuledMode::Normal => 0,
                    RuledMode::Tangent => 1,
                    RuledMode::Angle => 2,
                    RuledMode::Direction => 3,
                },
                angle: angle.as_deref().map(str::to_string).unwrap_or_else(|| "5 deg".into()),
                flip: *flip,
                round: *corners == RuledCorners::Round,
            };
            let mut ins = inputs(&k);
            let fill = [edges.iter().filter_map(edge).collect(), face.and_then(|p| crate::dialogs::face_sel(s, p.0)).into_iter().collect()];
            for (inp, items) in ins.iter_mut().zip(fill) {
                inp.items = items;
            }
            let mut extra = Map::new();
            if let Some(d) = direction {
                extra.insert("direction".into(), pt(d.0));
            }
            return Some((Kind::Surface(k), ins, extra));
        }
        FeatureKind::Unstitch { body, faces } | FeatureKind::ReverseNormal { body, faces } => {
            let sels: Vec<Sel> = if faces.is_empty() {
                vec![Sel::Body { name: body.to_string() }]
            } else {
                faces.iter().filter_map(|p| crate::dialogs::face_sel(s, p.0)).collect()
            };
            (if matches!(kind, FeatureKind::Unstitch { .. }) { Surf::Unstitch } else { Surf::ReverseNormal }, vec![sels])
        }
        FeatureKind::Stitch { bodies, .. } => (Surf::simple_restore(kind)?, vec![bodies.iter().map(|n| Sel::Body { name: n.to_string() }).collect()]),
        FeatureKind::Thicken { bodies, .. } => {
            (Surf::simple_restore(kind)?, vec![bodies.iter().map(|n| Sel::Body { name: n.to_string() }).collect()])
        }
        FeatureKind::SurfaceExtend { edges, mode, up_to, neighbours, .. } => {
            // The dialog edits the edges and the distance; the rest is kept as it was.
            let mut extra = Map::new();
            if !mode.is_natural() {
                extra.insert("mode".into(), json!(mode));
            }
            if !*neighbours {
                extra.insert("neighbours".into(), json!(false));
            }
            match up_to {
                Some(solidcraft_engine::doc::ExtendTo::Bounds) => {
                    extra.insert("up_to".into(), json!("bounds"));
                }
                Some(solidcraft_engine::doc::ExtendTo::Face(t)) => {
                    extra.insert("up_to".into(), json!({"body": t.body, "point": pt(t.point.0)}));
                }
                Some(solidcraft_engine::doc::ExtendTo::Plane(PlaneRef::Origin { name })) => {
                    extra.insert("up_to".into(), json!(name));
                }
                Some(solidcraft_engine::doc::ExtendTo::Plane(PlaneRef::Construction { feature })) => {
                    let shown = s.doc.feature(feature.0).map(|f| json!(f.name)).unwrap_or_else(|| json!(feature.0));
                    extra.insert("up_to".into(), shown);
                }
                _ => {}
            }
            let k = Surf::simple_restore(kind)?;
            let mut ins = inputs(&k);
            if let Some(inp) = ins.first_mut() {
                inp.items = edges.iter().filter_map(edge).collect();
            }
            return Some((Kind::Surface(k), ins, extra));
        }
        FeatureKind::SurfaceTrim { body, plane, tool, keep, keep_more, curves, with, keep_with, stitch, extend } => {
            let faces = |pts: &mut dyn Iterator<Item = &Position>| -> Vec<Sel> { pts.filter_map(|p| crate::dialogs::face_sel(s, p.0)).collect() };
            let tools: Vec<Sel> = match (with, curves) {
                (Some(_), _) => Vec::new(),
                (None, Some(c)) if c.curves.is_empty() => {
                    let st = s.world_state();
                    st.sketch(c.sketch.0)
                        .map(|ss| {
                            ss.sketch
                                .curves
                                .iter()
                                .filter(|k| !k.construction)
                                .map(|k| crate::selection::curve_pick(&k.id, c.sketch.0, s.active_sketch))
                                .collect()
                        })
                        .unwrap_or_default()
                }
                (None, Some(c)) => c.curves.iter().map(|id| crate::selection::curve_pick(id, c.sketch.0, s.active_sketch)).collect(),
                (None, None) => tool_sel(app, plane, tool.as_ref()).into_iter().collect(),
            };
            (
                Surf::Trim { extend: *extend, stitch: *stitch, direction: curves.as_ref().and_then(|c| c.direction.map(|d| d.0)) },
                vec![
                    vec![Sel::Body { name: body.to_string() }],
                    tools,
                    faces(&mut std::iter::once(keep).chain(keep_more)),
                    with.iter().map(|n| Sel::Body { name: n.to_string() }).collect(),
                    faces(&mut keep_with.iter()),
                ],
            )
        }
        FeatureKind::SurfaceUntrim { face, holes, outer, in_place } => {
            let (index, picked) = match holes {
                None => (0, Vec::new()),
                Some(points) if points.is_empty() => (2, Vec::new()),
                Some(points) => (1, points.iter().filter_map(edge).collect()),
            };
            (
                Surf::Untrim { holes: index, outer: *outer, in_place: *in_place },
                vec![crate::dialogs::face_sel(s, face.point.0).into_iter().collect(), picked],
            )
        }
        FeatureKind::SplitFace { faces, plane, tool, .. } => (
            Surf::SplitFace,
            vec![faces.iter().filter_map(|p| crate::dialogs::face_sel(s, p.0)).collect(), tool_sel(app, plane, tool.as_ref()).into_iter().collect()],
        ),
        _ => return None,
    };
    let mut ins = inputs(&k);
    for (inp, items) in ins.iter_mut().zip(fill) {
        inp.items = items;
    }
    Some((Kind::Surface(k), ins, Map::new()))
}
