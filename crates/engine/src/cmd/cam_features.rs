//! `cam.features`: the machinable features of a part, found on the exact B-rep of its bodies
//! (never on a mesh): round holes along the tool axis (through or blind, counterbores and
//! countersinks), closed pockets (a planar floor walled all round, its islands and the smallest
//! radius of its inside corners; or none, an opening through the part), planar faces facing the
//! tool, outer profiles, fillets and chamfers on top edges, other curved areas, and on a 4th
//! axis the A angle that brings each side up and the diameters turned about the A axis.
//!
//! Everything is read from the record: a face's analytic surface (`face_surface`), its loops of
//! oriented edges and the faces on the other side of each edge, the edges' exact curves (lines,
//! circles through `edge_circle`, the control nets that bound a spline) and the loops read in a
//! plane by `Body::planar_edges` (lines and arcs exactly). Whether a round face is a hole or a
//! boss is read from the planar faces at its ends (in an inner loop with the wall going into the
//! material, or a floor's outer loop), else by classifying a point beside it.
//!
//! Each feature gets an id (`hole:0`, `pocket:1`, `face:2`, `profile:0`…), numbered per kind in
//! a fixed order (setup, then from the top down, then by position), so the same part and the
//! same parameters give the same ids. `cam.job` operations name them through [`Lazy`], which
//! finds the features the way `cam.features` does with the job's bodies, origin, tolerance and
//! machine.

use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::{PI, TAU};
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

use serde_json::Value;
use solidcore_record::brep::{Brep, Curve, Loop as BrepLoop};
use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_cam::indexed::{frame_for_index, index_for_axis};
use solidcraft_cam::kinematics::Machine;
use solidcraft_cam::{Frame, Heights};
use solidcraft_geom::prelude::*;
use solidcraft_kernel::{Body, EdgeCircle, FaceSurface, PlanarPiece, PointClass, edge_circle, face_surface};

use super::{Ctx, MAX_BODY_NAMES, TOLERANCE_RANGE, finite_point, num, parse, v3};
use crate::cmd::CommandSpec;
use crate::params::bad;
use crate::schema::cam::{HeightsPartial, SideSpec};
use crate::schema::cam_features::{
    AxisSpec, CamFeaturesParams, CamFeaturesReply, Countersink, EdgeBreakReport, FaceReport, HoleGroup, HoleReport, HoleThread, IslandReport,
    PocketReport, ProfileReport, SetupReport, SurfaceReport, TurnedReport, WrappedReport,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const FEATURES_CONTRACT: TypedContract<CamFeaturesParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("cam.features", "CAM Features", crate::checked!("cam.features", features))
    .noundo()
    .icon("cam")
    .contract(&FEATURES_CONTRACT)
    .output(output_schema_for::<CamFeaturesReply>)
    .params(
        "bodies?: [names] (default every visible body); origin?: [x,y,z] (work zero, as a job's); axis?: \"z\" | \"a\" (a 4th axis about \
         work X: every side square to X, at the A angle that brings it up; default z); tolerance?: mm (default 0.01) → setups: [{a_deg, \
         tool_axis, top_z, bottom_z, bounds, swing_radius?, features}], holes: [{id: \"hole:k\", a_deg?, center, center_work, diameter, \
         top_z, bottom_z, depth, through, floor: through|flat|cone|round, tip_z?, counterbore?, counterbore_of?, countersink?}], hole_groups, \
         pockets: [{id: \"pocket:k\", floor_z, top_z, depth, outline, area, min_corner_radius, sharp_corners, walls_vertical, islands, \
         through? (an opening through the part: a slot, a window)}], faces: [{id: \"face:k\", z, area, role: \
         top|plateau|step|island_top|pocket_floor|hole_floor|wrapped_wall}], profiles: [{id: \"profile:k\", top_z, bottom_z, outline, \
         min_concave_radius?}], fillets, chamfers, surfaces, turned (axis a: [{id: \"turned:k\", diameter, x_min, x_max, lobe?: [ids of \
         the sides standing out of it], radius_max?, profile?: [[a_deg, radius]]}]), wrapped (axis a: [{id: \"wrapped:k\", radius, \
         floor_radius, depth, x_min, x_max, a_min_deg, a_max_deg, span_deg, outline unrolled at radius}]), notes. Read from the exact \
         B-rep, in each setup's frame (its Z the tool axis). A cam.job pocket or contour takes `feature: \"pocket:0\"` (heights default \
         from the feature); the ids are the same for the same bodies, origin and axis",
    )];

/// Most setups (A angles) looked at.
const MAX_SETUPS: usize = 360;
/// Most faces a body may have to be looked at.
const MAX_FACES: usize = 20_000;
/// Directions within this of each other (as 1 - cos) are the same.
const PARALLEL: f64 = 1e-9;
/// A face whose normal's Z is within this of 0 is a wall.
const SQUARE: f64 = 1e-7;
/// Faces sloping less than this from the tool axis, degrees, falling from a top face, are drafted
/// walls rather than chamfers.
const DRAFT_MAX_DEG: f64 = 20.0;
/// Turning more than this (radians) where two pieces of an outline meet is a corner.
const CORNER: f64 = 0.02;
/// Most points of a curve sampled into an outline.
const MAX_SAMPLES: usize = 720;
/// How far past the foot of its walls an opening through the part is cut, mm.
pub(crate) const BREAKTHROUGH: f64 = 0.5;

/// `cam.features`.
fn features(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.features";
    let params: CamFeaturesParams = parse(cmd, p, "")?;
    let tol = tolerance(cmd, params.tolerance)?;
    let origin = params.origin.unwrap_or([0.0; 3]);
    finite_point(&origin, "`origin`").map_err(|e| bad(cmd, e))?;
    let axis = params.axis.unwrap_or_default();
    let machine = match axis {
        AxisSpec::Z => Machine::three_axis(),
        AxisSpec::A => Machine::four_axis_a(),
    };
    let names = part_names(cmd, s, params.bodies.as_deref())?;
    let mut set = recognize(cmd, s, &names, v3(origin), axis, tol, &machine)?;
    if axis == AxisSpec::A {
        let st = s.world_state()?;
        lobes(&mut set.reply, &st, v3(origin), tol);
    }
    serde_json::to_value(&set.reply).map_err(|e| EngineError::Other(e.to_string()))
}

/// Degrees of A between the radii of a lobe's `profile`.
const LOBE_STEP_DEG: f64 = 15.0;

/// Each turned diameter that sides at A angles (faces, edge breaks, curved areas) stand out of
/// within its X range, as a cam's lobe stands out of its base circle: named with those sides, its
/// radius every [`LOBE_STEP_DEG`] of A at the middle of the sides along X and the largest of
/// them, each found by classifying points on the exact body along the ray up at that angle.
fn lobes(reply: &mut CamFeaturesReply, st: &solidcraft_doc::ModelState, origin: Vec3, tol: f64) {
    let swing = reply.setups.iter().filter_map(|su| su.swing_radius).fold(0.0, f64::max);
    let mut sides: Vec<(String, String, f64, f64)> = Vec::new();
    for f in reply.faces.iter().filter(|f| f.a_deg.is_some()) {
        sides.push((f.id.clone(), f.body.clone(), f.bounds[0][0], f.bounds[1][0]));
    }
    for r in reply.fillets.iter().chain(&reply.chamfers).filter(|r| r.a_deg.is_some()) {
        sides.push((r.id.clone(), r.body.clone(), r.bounds[0][0], r.bounds[1][0]));
    }
    for f in reply.surfaces.iter().filter(|f| f.a_deg.is_some()) {
        sides.push((f.id.clone(), f.body.clone(), f.bounds[0][0], f.bounds[1][0]));
    }
    // Each side on the narrowest turned diameter of its body whose X range holds it.
    let mut on: Vec<Vec<(String, f64, f64)>> = vec![Vec::new(); reply.turned.len()];
    for (id, body, x0, x1) in sides {
        let best = reply
            .turned
            .iter()
            .enumerate()
            .filter(|(_, t)| t.body == body && t.x_min <= x0 + tol && t.x_max >= x1 - tol)
            .min_by(|a, b| (a.1.x_max - a.1.x_min).total_cmp(&(b.1.x_max - b.1.x_min)));
        if let Some(list) = best.and_then(|(k, _)| on.get_mut(k)) {
            list.push((id, x0, x1));
        }
    }
    let steps = (360.0 / LOBE_STEP_DEG).round() as usize;
    for (t, list) in reply.turned.iter_mut().zip(on) {
        if list.is_empty() || swing <= 0.0 {
            continue;
        }
        let Some(b) = st.bodies.iter().find(|b| b.name == t.body).filter(|b| !b.body.is_mesh()) else { continue };
        let lo_x = list.iter().map(|s| s.1).fold(t.x_min, f64::max);
        let hi_x = list.iter().map(|s| s.2).fold(t.x_max, f64::min);
        let x = if lo_x <= hi_x { 0.5 * (lo_x + hi_x) } else { 0.5 * (t.x_min + t.x_max) };
        let r0 = t.diameter * 0.5;
        let mut profile = Vec::with_capacity(steps);
        for k in 0..steps {
            let a_deg = k as f64 * LOBE_STEP_DEG;
            let a = a_deg.to_radians();
            let inside = |r: f64| {
                let p = origin + Vec3::new(x, r * a.sin(), r * a.cos());
                matches!(b.body.classify_point(p), Ok(PointClass::Inside))
            };
            let (mut lo, mut hi) = (r0 + tol, swing + 1.0);
            let r = if !inside(lo) {
                r0
            } else if inside(hi) {
                continue;
            } else {
                // Halved down to a tenth of the tolerance: some 20 steps over a lobe's height.
                for _ in 0..64 {
                    if hi - lo <= tol * 0.1 {
                        break;
                    }
                    let mid = 0.5 * (lo + hi);
                    if inside(mid) {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                0.5 * (lo + hi)
            };
            profile.push([a_deg, r9((r / tol).round() * tol)]);
        }
        let top = profile.iter().map(|p| p[1]).fold(r0, f64::max);
        if top <= r0 + tol {
            // Sides cut into the diameter (a flat), not standing out of it.
            continue;
        }
        t.lobe = list.into_iter().map(|s| s.0).collect();
        t.radius_max = Some(top);
        t.profile = profile;
    }
}

fn tolerance(cmd: &str, t: Option<f64>) -> Result<f64> {
    let tol = t.unwrap_or(0.01);
    if tol.is_finite() && (TOLERANCE_RANGE.0..=TOLERANCE_RANGE.1).contains(&tol) {
        Ok(tol)
    } else {
        Err(bad(cmd, "the `tolerance` must be from 0.0001 to 1 mm"))
    }
}

/// The part's bodies: `given`, or every visible body, each one there.
pub(super) fn part_names(cmd: &str, s: &Session, given: Option<&[String]>) -> Result<Vec<String>> {
    let st = s.world_state()?;
    let names: Vec<String> = match given {
        Some(b) => b.to_vec(),
        None => st.bodies.iter().map(|b| b.name.clone()).filter(|n| s.body_visible(n)).collect(),
    };
    if names.len() > MAX_BODY_NAMES {
        return Err(bad(cmd, format!("name at most {MAX_BODY_NAMES} bodies")));
    }
    if names.is_empty() {
        return Err(bad(cmd, "there is no body to machine: the design has no visible body, or give `bodies`"));
    }
    for n in &names {
        if !st.bodies.iter().any(|b| &b.name == n) {
            return Err(bad(cmd, crate::cmd::no_body(n, st.bodies.iter().map(|b| b.name.as_str()))));
        }
    }
    let mut seen = BTreeSet::new();
    if let Some(twice) = names.iter().find(|n| !seen.insert(n.as_str())) {
        return Err(bad(cmd, format!("body `{twice}` is named twice")));
    }
    Ok(names)
}

/// One tool direction, as recognised.
#[derive(Clone, Debug)]
pub(crate) struct Setup {
    pub a_deg: f64,
    pub frame: Frame,
    pub top: f64,
    pub bottom: f64,
    pub swing: Option<f64>,
}

/// What kind of outline a feature gives a 2D operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Hole,
    Pocket,
    Face,
    Profile,
    /// No outline: fillets, chamfers, surfaces, turned diameters.
    Area,
}

/// A feature's exact geometry, in its setup's frame.
#[derive(Clone, Debug)]
pub(crate) struct Geo {
    pub kind: Kind,
    pub setup: usize,
    pub outer: Option<Loop2>,
    /// Islands with the top of their walls.
    pub islands: Vec<(Loop2, f64)>,
    pub top: f64,
    pub bottom: f64,
    pub bounds: [[f64; 2]; 2],
    /// A hole's centre, diameter and whether it goes through.
    pub hole: Option<(Vec2, f64, bool)>,
    /// What a 2D operation on it is told when its depth is zero.
    pub flat_top: bool,
    /// For a face, one per segment of `outer`: whether the part falls away below the face past
    /// that edge (an open side of a step or plateau) rather than a wall rising from it.
    pub open: Vec<bool>,
}

/// The features of a part.
#[derive(Clone)]
pub(crate) struct FeatureSet {
    pub reply: CamFeaturesReply,
    pub setups: Vec<Setup>,
    pub geo: BTreeMap<String, Geo>,
}

impl FeatureSet {
    /// The feature `id`, or a refusal saying which there are.
    pub fn get(&self, id: &str) -> std::result::Result<&Geo, String> {
        let id = id.trim();
        if let Some(g) = self.geo.get(id) {
            return Ok(g);
        }
        let kind = id.split(':').next().unwrap_or_default();
        let same: Vec<&str> = self.geo.keys().filter(|k| k.split(':').next() == Some(kind)).map(String::as_str).collect();
        let known = match (same.first(), same.last()) {
            (Some(a), Some(b)) if a == b => format!("this part has only `{a}`"),
            (Some(a), Some(b)) => format!("this part has `{a}` to `{b}`"),
            _ => {
                let kinds: BTreeSet<&str> = self.geo.keys().filter_map(|k| k.split(':').next()).collect();
                if kinds.is_empty() {
                    "this part has no machinable features".to_string()
                } else {
                    format!("ids are kind:number, of the kinds {}", kinds.into_iter().collect::<Vec<_>>().join(", "))
                }
            }
        };
        let along = match self.reply.axis {
            AxisSpec::Z => {
                "found along Z only, as the job's machine has no A axis about X (with `machine: \"4axis_a\"` the sides are found too; \
                 `cam.features {axis: \"z\"}` gives these ids)"
            }
            AxisSpec::A => "found on every side about X, as the job's machine has an A axis (`cam.features {axis: \"a\"}` gives these ids)",
        };
        let shown: String = if id.chars().count() > 40 { format!("{}…", id.chars().take(40).collect::<String>()) } else { id.to_string() };
        Err(format!("no feature `{shown}`: {known}; features are {along}, with the job's bodies and origin"))
    }

    pub fn setup(&self, g: &Geo) -> Option<&Setup> {
        self.setups.get(g.setup)
    }
}

/// The features of a job's part, found the first time an operation names one.
pub(crate) struct Lazy {
    bodies: Option<Vec<String>>,
    origin: Vec3,
    tol: f64,
    machine: Machine,
    cell: std::sync::OnceLock<std::result::Result<FeatureSet, String>>,
}

impl Lazy {
    pub fn new(bodies: Option<Vec<String>>, origin: Vec3, tol: f64, machine: &Machine) -> Lazy {
        Lazy { bodies, origin, tol, machine: machine.clone(), cell: std::sync::OnceLock::new() }
    }

    /// The axis features are found along for `machine`: `a` with one rotary about X, else `z`.
    pub fn axis_of(machine: &Machine) -> AxisSpec {
        match machine.rotaries.as_slice() {
            [one] if one.direction.normalized().is_some_and(|d| d.x.abs() > 1.0 - PARALLEL) => AxisSpec::A,
            _ => AxisSpec::Z,
        }
    }

    pub fn get(&self, cmd: &str, s: &Session) -> Result<&FeatureSet> {
        let found = self.cell.get_or_init(|| {
            let run = || -> Result<FeatureSet> {
                let names = part_names(cmd, s, self.bodies.as_deref())?;
                recognize(cmd, s, &names, self.origin, Lazy::axis_of(&self.machine), self.tol, &self.machine)
            };
            run().map_err(|e| match e {
                EngineError::BadParams { msg, .. } => msg,
                other => other.to_string(),
            })
        });
        found.as_ref().map_err(|e| bad(cmd, format!("finding the part's features: {e}")))
    }
}

/// What a 2D operation cuts for a feature: its regions, heights and the side a contour runs on.
pub(crate) struct FeatureCut {
    pub regions: Vec<Region2>,
    pub heights: Heights,
    pub side: SideSpec,
}

/// The heights of an operation on a feature of `setup` whose material runs from `top` down to
/// `bottom`, `given` laid over them. Those left out are worked out from those given: the retract
/// 3 mm above the material (the setup's top or the `top` given, the higher), the clearance 10 mm
/// above it, 5 mm above the retract given and 5 mm past the swing on a 4th axis. Out of order,
/// the refusal names each height and where it came from.
pub(super) fn feature_heights(setup: &Setup, top: f64, bottom: f64, given: Option<&HeightsPartial>) -> std::result::Result<Heights, String> {
    let g = given.copied().unwrap_or_default();
    let top_v = g.top.unwrap_or(top);
    let bottom_v = g.bottom.unwrap_or(bottom);
    let material = setup.top.max(top_v);
    let retract = g.retract.unwrap_or(material + 3.0);
    let mut clearance = (material + 10.0).max(retract + 5.0);
    if let Some(sw) = setup.swing {
        clearance = clearance.max(sw + 5.0);
    }
    let clearance = g.clearance.unwrap_or(clearance);
    let h = Heights { clearance, retract, top: top_v, bottom: bottom_v };
    if h.clearance < h.retract || h.retract < h.top || h.top < h.bottom {
        let from = |v: Option<f64>, x: f64, k: &str, d: &str| format!("{k} {} ({})", num(x), if v.is_some() { "given" } else { d });
        return Err(format!(
            "the heights must be ordered clearance ≥ retract ≥ top ≥ bottom; here {}, {}, {}, {}",
            from(g.clearance, h.clearance, "clearance", "the default"),
            from(g.retract, h.retract, "retract", "the default"),
            from(g.top, h.top, "top", "the feature's"),
            from(g.bottom, h.bottom, "bottom", "the feature's")
        ));
    }
    Ok(h)
}

/// The feature `id` checked against the frame the operation runs in.
fn in_frame<'s>(ctx: &Ctx<'_>, set: &'s FeatureSet, op: &str, id: &str, frame: &Frame) -> Result<&'s Geo> {
    let g = set.get(id).map_err(|e| ctx.bad(op, e))?;
    let Some(setup) = set.setup(g) else { return Err(ctx.bad(op, format!("feature `{id}` has no setup"))) };
    let same = frame.z.dot(setup.frame.z) > 1.0 - PARALLEL && frame.origin.dist(setup.frame.origin) <= 1e-6;
    if !same {
        let a = num(setup.a_deg);
        return Err(ctx.bad(
            op,
            format!(
                "feature `{id}` is machined at A = {a}° (its tool axis is work [{}, {}, {}]) but this operation runs with its tool along work \
                 [{}, {}, {}]: name a feature of the A = 0 setup",
                num(setup.frame.z.x),
                num(setup.frame.z.y),
                num(setup.frame.z.z),
                num(frame.z.x),
                num(frame.z.y),
                num(frame.z.z)
            ),
        ));
    }
    Ok(g)
}

/// The regions, heights and contour side of a 2D operation on feature `id`, in `frame`. A pocket
/// passes its tool's radius as `facing`: on a face at the setup's top (nothing of the part above
/// it) the pocket then covers the face's extent grown by the radius and 1 mm, so the tool's edge
/// passes the face's edges instead of stopping a radius inside them.
pub(super) fn feature_cut(
    ctx: &Ctx<'_>,
    op: &str,
    id: &str,
    frame: &Frame,
    given: Option<&HeightsPartial>,
    facing: Option<f64>,
) -> Result<FeatureCut> {
    let set = ctx.features.get(ctx.cmd, ctx.s)?;
    let g = in_frame(ctx, set, op, id, frame)?;
    let Some(setup) = set.setup(g) else { return Err(ctx.bad(op, format!("feature `{id}` has no setup"))) };
    let outer = match (g.kind, &g.outer) {
        (Kind::Hole, _) => {
            let Some((center, diameter, _)) = g.hole else { return Err(ctx.bad(op, format!("`{id}` is not a hole"))) };
            Loop2::circle(center, diameter * 0.5)
        }
        (Kind::Area, _) | (_, None) => {
            let [lo, hi] = g.bounds;
            return Err(ctx.bad(
                op,
                format!(
                    "`{id}` has no outline for a 2D operation: it is a curved area from X {} Y {} to X {} Y {}, down to Z {}",
                    num(lo[0]),
                    num(lo[1]),
                    num(hi[0]),
                    num(hi[1]),
                    num(g.bottom)
                ),
            ));
        }
        (_, Some(o)) => o.clone(),
    };
    let heights = feature_heights(setup, g.top, g.bottom, given).map_err(|e| ctx.bad(op, e))?;
    if !(heights.top > heights.bottom + 1e-9) {
        let why = if g.flat_top {
            format!(
                "`{id}` is a face at Z {} with nothing above it in the model: give `heights.top` (the stock's top) to face it, or use its \
                 pocket or the profile round it",
                num(g.bottom)
            )
        } else {
            format!("`heights.top` ({}) must be above `heights.bottom` ({})", num(heights.top), num(heights.bottom))
        };
        return Err(ctx.bad(op, why));
    }
    // An island is in the way only where the cut goes below its top.
    let holes: Vec<Loop2> = g.islands.iter().filter(|(_, top)| *top > heights.bottom + ctx.tol).map(|(l, _)| l.clone()).collect();
    let outer = match facing {
        Some(r) if g.kind == Kind::Face && g.flat_top && holes.is_empty() && (g.bottom - setup.top).abs() <= ctx.tol && r.is_finite() && r > 0.0 => {
            let m = r + 1.0;
            let [lo, hi] = g.bounds;
            Loop2::polygon(&[
                Vec2::new(lo[0] - m, lo[1] - m),
                Vec2::new(hi[0] + m, lo[1] - m),
                Vec2::new(hi[0] + m, hi[1] + m),
                Vec2::new(lo[0] - m, hi[1] + m),
            ])
        }
        _ => outer,
    };
    let side = match g.kind {
        Kind::Profile => SideSpec::Outside,
        _ => SideSpec::Inside,
    };
    Ok(FeatureCut { regions: vec![Region2 { outer, holes }], heights, side })
}

/// For a clearing on face `id` (a step or plateau the part falls away from on some sides): its
/// outline with each open side pushed out past the stock's box `stock` (else the face's own
/// extent) grown by `margin`, then cut back to that grown box, so the clearing takes the material
/// above the face out past the open sides as well as inside them. `None` for a face closed all
/// round (or not a face). A side pushed out across another (a notch sharper than a right angle
/// between two open sides) is refused, naming the face.
pub(super) fn open_region(ctx: &Ctx<'_>, op: &str, id: &str, frame: &Frame, stock: Option<[Vec2; 2]>, margin: f64) -> Result<Option<Region2>> {
    let set = ctx.features.get(ctx.cmd, ctx.s)?;
    let g = in_frame(ctx, set, op, id, frame)?;
    let [lo, hi] = stock.unwrap_or([Vec2::new(g.bounds[0][0], g.bounds[0][1]), Vec2::new(g.bounds[1][0], g.bounds[1][1])]);
    let Some(outer) = g.outer.as_ref() else { return Ok(None) };
    if g.kind != Kind::Face || g.open.len() != outer.segs.len() || !g.open.iter().any(|o| *o) {
        return Ok(None);
    }
    let tol = set.reply.tolerance.max(1e-4);
    // The outline as a polygon, each edge flagged open or not (counter-clockwise).
    let mut pts: Vec<Vec2> = Vec::new();
    let mut open: Vec<bool> = Vec::new();
    for (seg, o) in outer.segs.iter().zip(&g.open) {
        for p in loop_points(std::slice::from_ref(seg), tol) {
            if pts.last().is_none_or(|l: &Vec2| l.dist(p) > 1e-9) {
                pts.push(p);
                open.push(*o);
            }
        }
    }
    if pts.first().zip(pts.last()).is_some_and(|(a, b)| pts.len() > 1 && a.dist(*b) <= 1e-9) {
        pts.pop();
        open.pop();
    }
    let (pts, open, chords) = bridge_notches(pts, open, tol);
    let n = pts.len();
    if n < 3 || !open.iter().any(|o| *o) {
        return Ok(None);
    }
    let m = margin.max(0.0);
    let (blo, bhi) = (Vec2::new(lo.x - m, lo.y - m), Vec2::new(hi.x + m, hi.y + m));
    let [flo, fhi] = g.bounds;
    let reach = (Vec2::new(bhi.x.max(fhi[0]), bhi.y.max(fhi[1])) - Vec2::new(blo.x.min(flo[0]), blo.y.min(flo[1]))).len() + 1.0;
    let at = |i: usize| pts.get(i % n).copied().unwrap_or(Vec2::ZERO);
    let normal = |i: usize| {
        let d = at(i + 1) - at(i);
        Vec2::new(d.y, -d.x).normalized()
    };
    let refuse = || {
        ctx.bad(
            op,
            format!(
                "`{id}` is open on sides that meet at a notch sharper than a right angle, so its outline cannot be carried out past them to the stock's edge: give a `polygon` round the face and past its open sides instead"
            ),
        )
    };
    // The outline with the sides flagged in `open` pushed out and cut back to the grown box,
    // `None` where it cannot be (a notch sharper than a right angle between open sides). A side
    // flagged in `near` is pushed out by `margin` only (the tool over it, not on to the stock's
    // edge); the sides of one run of open sides are flagged alike.
    let grow = |open: &[bool], near: &[bool]| -> Option<Vec<Vec2>> {
        let reach_of = |k: usize| if near.get(k).copied().unwrap_or(false) { m.max(tol) } else { reach };
        let mut out: Vec<Vec2> = Vec::with_capacity(n + 8);
        for i in 0..n {
            let prev = (i + n - 1) % n;
            let (o_prev, o_here) = (open.get(prev).copied().unwrap_or(false), open.get(i).copied().unwrap_or(false));
            let v = at(i);
            match (o_prev, o_here) {
                (false, false) => out.push(v),
                (false, true) => {
                    let ni = normal(i)?;
                    out.push(v);
                    out.push(v + ni * reach_of(i));
                }
                (true, false) => {
                    let np = normal(prev)?;
                    out.push(v + np * reach_of(prev));
                    out.push(v);
                }
                (true, true) => {
                    let (Some(np), Some(ni)) = (normal(prev), normal(i)) else { return None };
                    let reach = reach_of(i);
                    let k = 1.0 + np.dot(ni);
                    if !(k > 1.0) {
                        // A right angle or sharper between two open sides (outside corner or notch):
                        // both pushed out separately, joined round the corner.
                        if np.cross(ni) < 0.0 {
                            return None;
                        }
                        out.push(v + np * reach);
                        out.push(v + (np + ni).normalized().unwrap_or(np) * (reach * std::f64::consts::SQRT_2));
                        out.push(v + ni * reach);
                    } else {
                        out.push(v + (np + ni) * (reach / k));
                    }
                }
            }
        }
        // Cut back to the grown box (a convex window: Sutherland-Hodgman).
        for (axis, bound, keep_above) in [(0, blo.x, true), (0, bhi.x, false), (1, blo.y, true), (1, bhi.y, false)] {
            let coord = |p: Vec2| if axis == 0 { p.x } else { p.y };
            let inside = |p: Vec2| if keep_above { coord(p) >= bound } else { coord(p) <= bound };
            let cross = |a: Vec2, b: Vec2| {
                let t = (bound - coord(a)) / (coord(b) - coord(a));
                a.lerp(b, t.clamp(0.0, 1.0))
            };
            let src = std::mem::take(&mut out);
            for (j, b) in src.iter().enumerate() {
                let a = src.get((j + src.len() - 1) % src.len().max(1)).copied().unwrap_or(*b);
                match (inside(a), inside(*b)) {
                    (true, true) => out.push(*b),
                    (true, false) => out.push(cross(a, *b)),
                    (false, true) => {
                        out.push(cross(a, *b));
                        out.push(*b);
                    }
                    (false, false) => {}
                }
            }
        }
        out.dedup_by(|a, b| a.dist(*b) <= 1e-9);
        (out.len() >= 3 && polygon_area(&out) > tol * tol && !self_crossing(&out)).then_some(out)
    };
    let far = vec![false; n];
    let mut out = grow(&open, &far).ok_or_else(refuse)?;
    // The sides pushed out.
    let mut keep = open.clone();
    // Past the open sides the part must not rise above the face (a boss beyond a lower area, the
    // far side of a pocket seen from a side setup): read from the exact solid at points over what
    // the open sides add, within the stock, from just above the face up to the setup's top.
    if part_above(ctx, ctx.features, set, g, frame, &pts, &out, (blo, bhi))?.is_some() {
        // Past some open sides only (a notch open to the stock's edge at one end and into a
        // deeper cut at the other, past which the part rises): each run of open sides pushed out
        // on its own, kept open where the part does not rise past it; the others are cut to as
        // the face's own edge, the clearing staying inside them.
        keep = vec![false; n];
        for run in open_runs(&open) {
            let mut one = vec![false; n];
            for &i in &run {
                if let Some(o) = one.get_mut(i) {
                    *o = true;
                }
            }
            let Some(grown) = grow(&one, &far) else { continue };
            if part_above(ctx, ctx.features, set, g, frame, &pts, &grown, (blo, bhi))?.is_none() {
                for &i in &run {
                    if let Some(o) = keep.get_mut(i) {
                        *o = true;
                    }
                }
            }
        }
        if keep.iter().any(|o| *o)
            && let Some(grown) = grow(&keep, &far)
        {
            out = grown;
        }
    }
    if let Some(at) = part_above(ctx, ctx.features, set, g, frame, &pts, &out, (blo, bhi))? {
        return Err(ctx.bad(
            op,
            format!(
                "`{id}` is open on some sides, but past them the part rises above it (at X {} Y {}): give a `polygon` round the face that stops short of it",
                num(at.x),
                num(at.y)
            ),
        ));
    }
    // A line bridging a round notch (a round hole that breaks into the face) is open into the hole
    // by `margin` where all that lies within the hole's circle and the part does not rise there:
    // the clearing goes over the hole and into the corners the line makes with the walls, which a
    // closed line would leave.
    let mut near = far.clone();
    for (c, circle) in chords.iter().enumerate() {
        let Some((centre, r)) = *circle else { continue };
        let Some(nc) = normal(c) else { continue };
        let (a, b) = (at(c), at(c + 1));
        if [a, b, a.lerp(b, 0.5)].iter().any(|p| (*p + nc * m).dist(centre) > r - tol) {
            continue;
        }
        let (mut k2, mut n2) = (keep.clone(), near.clone());
        if let (Some(k), Some(nr)) = (k2.get_mut(c), n2.get_mut(c)) {
            *k = true;
            *nr = true;
        }
        if let Some(grown) = grow(&k2, &n2)
            && part_above(ctx, ctx.features, set, g, frame, &pts, &grown, (blo, bhi))?.is_none()
        {
            (keep, near, out) = (k2, n2, grown);
        }
    }
    // An island is in the way only where the cut goes below its top: the face's own.
    let holes: Vec<Loop2> = g.islands.iter().filter(|(_, top)| *top > g.bottom + ctx.tol).map(|(l, _)| l.clone()).collect();
    Ok(Some(Region2 { outer: Loop2::polygon(&out), holes }))
}

/// For a contour inside face `id` with walls on some sides and open on others (a step, a slot's
/// floor): its outline with the open sides carried out by `margin` past the face ([`open_region`]
/// with no stock), so the pass along a wall goes on past the face's open end instead of turning
/// a tool's radius short of it and leaving the corner there. `None` for a face open all round or
/// closed all round, for any other feature, and where the outline cannot be carried out (the
/// part rising past an open side): the face's own outline then serves.
pub(super) fn walled_open_region(ctx: &Ctx<'_>, op: &str, id: &str, frame: &Frame, margin: f64) -> Option<Region2> {
    let set = ctx.features.get(ctx.cmd, ctx.s).ok()?;
    let g = in_frame(ctx, set, op, id, frame).ok()?;
    if g.kind != Kind::Face || !g.open.iter().any(|o| *o) || g.open.iter().all(|o| *o) {
        return None;
    }
    open_region(ctx, op, id, frame, None, margin).ok().flatten()
}

/// The runs of consecutive open sides of an outline (`open`, one flag per side, round the
/// outline): the indices of each, in order, a run across the end joined to the one at the start.
pub(super) fn open_runs(open: &[bool]) -> Vec<Vec<usize>> {
    let n = open.len();
    let is_open = |i: usize| open.get(i % n.max(1)).copied().unwrap_or(false);
    let Some(start) = (0..n).find(|&i| !is_open(i)) else {
        return if n > 0 { vec![(0..n).collect()] } else { Vec::new() };
    };
    let mut runs: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    for k in 1..=n {
        let i = (start + k) % n;
        if is_open(i) {
            cur.push(i);
        } else if !cur.is_empty() {
            runs.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        runs.push(cur);
    }
    runs
}

/// A face's outline `pts` (counter-clockwise, each edge flagged open where the part falls away
/// past it) with every notch where the part falls away into the face bridged: a run of open
/// edges between two closed ones, all of it on the face's side of the straight line between its
/// ends (a hole or a slot that breaks into the face, seen from above), becomes that line, closed.
/// Clearing over the notch cuts air above what falls away there, and the outline then has no
/// notch an open side could not be carried out past.
///
/// With each edge of the outline, where it is such a line over a round notch (a round hole that
/// breaks into the face), the circle of that notch: its centre and radius.
pub(super) fn bridge_notches(pts: Vec<Vec2>, open: Vec<bool>, tol: f64) -> (Vec<Vec2>, Vec<bool>, Vec<Option<(Vec2, f64)>>) {
    let n = pts.len();
    if n < 4 || open.len() != n || open.iter().all(|o| *o) || !open.iter().any(|o| *o) {
        let none = vec![None; n];
        return (pts, open, none);
    }
    let at = |i: usize| pts.get(i % n).copied().unwrap_or(Vec2::ZERO);
    let is_open = |i: usize| open.get(i % n).copied().unwrap_or(false);
    // Start after a closed edge, so that no run wraps round the end.
    let Some(start) = (0..n).find(|i| !is_open(*i) && is_open(i + 1)) else {
        let none = vec![None; n];
        return (pts, open, none);
    };
    let mut out_pts: Vec<Vec2> = Vec::with_capacity(n);
    let mut out_open: Vec<bool> = Vec::with_capacity(n);
    let mut out_chord: Vec<Option<(Vec2, f64)>> = Vec::with_capacity(n);
    let mut k = 0;
    while k < n {
        let i = start + 1 + k;
        if !is_open(i) {
            out_pts.push(at(i));
            out_open.push(false);
            out_chord.push(None);
            k += 1;
            continue;
        }
        // The run of open edges from vertex i to vertex j (edge i joins i and i + 1).
        let mut len = 0;
        while len < n && is_open(i + len) {
            len += 1;
        }
        let (a, b) = (at(i), at(i + len));
        let chord = b - a;
        let inward = len >= 2
            && chord.len() > tol
            && (1..len).all(|q| {
                let v = at(i + q) - a;
                chord.x * v.y - chord.y * v.x >= -tol * chord.len()
            })
            && (1..len).any(|q| {
                let v = at(i + q) - a;
                chord.x * v.y - chord.y * v.x > tol * chord.len()
            });
        if inward {
            // The circle through its ends and its middle point, where every point of it is on it.
            let circle = circle_through(a, at(i + len / 2), b).filter(|(c, r)| (1..len).all(|q| (at(i + q).dist(*c) - r).abs() <= 10.0 * tol));
            out_pts.push(a);
            out_open.push(false);
            out_chord.push(circle);
        } else {
            for q in 0..len {
                out_pts.push(at(i + q));
                out_open.push(true);
                out_chord.push(None);
            }
        }
        k += len;
    }
    if out_pts.len() < 3 {
        let none = vec![None; n];
        (pts, open, none)
    } else {
        (out_pts, out_open, out_chord)
    }
}

/// The circle through `a`, `b` and `c` (centre and radius), `None` when they are in a line.
fn circle_through(a: Vec2, b: Vec2, c: Vec2) -> Option<(Vec2, f64)> {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    if !(d.abs() > 1e-12) {
        return None;
    }
    let (a2, b2, c2) = (a.x * a.x + a.y * a.y, b.x * b.x + b.y * b.y, c.x * c.x + c.y * c.y);
    let centre =
        Vec2::new((a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d, (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d);
    let r = centre.dist(a);
    (centre.x.is_finite() && centre.y.is_finite() && r.is_finite()).then_some((centre, r))
}

/// A point over `grown` but outside `face` (both polygons in `frame`'s XY), within the stock's
/// box `stock`, where the job's part has material above face `g` (from just above it up to its
/// setup's top); `None` when there is none. Read from the exact solid: first its edges (every
/// straight edge exactly, arcs at points 0.05 mm apart), which a fin, a rib or a boss however
/// thin has over the area; then, for a face rising over it with its edges outside it, points
/// about 1 mm apart (at most some 4000) classified against the solid.
fn part_above(
    ctx: &Ctx<'_>,
    lazy: &Lazy,
    set: &FeatureSet,
    g: &Geo,
    frame: &Frame,
    face: &[Vec2],
    grown: &[Vec2],
    stock: (Vec2, Vec2),
) -> Result<Option<Vec2>> {
    let names = part_names(ctx.cmd, ctx.s, lazy.bodies.as_deref())?;
    let st = ctx.s.world_state()?;
    let bodies: Vec<&Body> = st.bodies.iter().filter(|b| names.contains(&b.name) && !b.body.is_mesh()).map(|b| &b.body).collect();
    let top = set.setup(g).map_or(g.top, |su| su.top.max(g.top));
    let eps = (set.reply.tolerance * 4.0).max(1e-3);
    let heights = [g.bottom + eps, 0.5 * (g.bottom + top), top - eps];
    let (lo, hi) = stock;
    let (blo, bhi) = (
        Vec2::new(
            grown.iter().map(|p| p.x).fold(f64::INFINITY, f64::min).max(lo.x),
            grown.iter().map(|p| p.y).fold(f64::INFINITY, f64::min).max(lo.y),
        ),
        Vec2::new(
            grown.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max).min(hi.x),
            grown.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max).min(hi.y),
        ),
    );
    if !(bhi.x > blo.x && bhi.y > blo.y) {
        return Ok(None);
    }
    Ok(part_above_read(&bodies, lazy.origin, frame, face, grown, (blo, bhi), heights, g.bottom, top, eps))
}

/// [`part_above`]'s reading of the exact solid, over box `(blo, bhi)`.
#[allow(clippy::too_many_arguments)]
fn part_above_read(
    bodies: &[&Body],
    origin: Vec3,
    frame: &Frame,
    face: &[Vec2],
    grown: &[Vec2],
    (blo, bhi): (Vec2, Vec2),
    heights: [f64; 3],
    bottom: f64,
    top: f64,
    eps: f64,
) -> Option<Vec2> {
    if let Some(q) = edges_above(bodies, origin, frame, face, grown, (bottom + eps, top), eps) {
        return Some(q);
    }
    // A point inside the part above the floor has a face of it straight above (where the
    // part ends going up), over the same point: only points over a face rising above the floor
    // are read, none when no such face is over the box.
    let floor = heights.iter().copied().filter(|z| *z > bottom).fold(f64::INFINITY, f64::min);
    let rising = faces_rising(bodies, origin, frame, (blo, bhi), floor);
    if rising.as_ref().is_some_and(Vec::is_empty) {
        return None;
    }
    let over = |q: Vec2| rising.as_ref().is_none_or(|r| r.iter().any(|(lo, hi)| !(q.x < lo.x || q.x > hi.x || q.y < lo.y || q.y > hi.y)));
    let step = ((bhi.x - blo.x) * (bhi.y - blo.y) / 4000.0).sqrt().max(1.0);
    let (nx, ny) = (((bhi.x - blo.x) / step).ceil() as usize + 1, ((bhi.y - blo.y) / step).ceil() as usize + 1);
    for j in 0..ny.min(4000) {
        for i in 0..nx.min(4000) {
            let q = Vec2::new((blo.x + i as f64 * step).min(bhi.x), (blo.y + j as f64 * step).min(bhi.y));
            if !inside_polygon(grown, q) || inside_polygon(face, q) || !over(q) {
                continue;
            }
            for z in heights.iter().filter(|z| **z > bottom) {
                let p = frame.to_world(Vec3::new(q.x, q.y, *z)) + origin;
                if bodies.iter().any(|b| matches!(b.classify_point(p), Ok(PointClass::Inside))) {
                    return Some(q);
                }
            }
        }
    }
    None
}

/// The boxes (in `frame`'s XY) of the faces of `bodies` that overlap box `(blo, bhi)` there and
/// may reach height `floor` or above it, each from the face's exact form ([`face_bound`]);
/// `None` when some face has no known bound (then every point is to be read).
fn faces_rising(bodies: &[&Body], origin: Vec3, frame: &Frame, (blo, bhi): (Vec2, Vec2), floor: f64) -> Option<Vec<(Vec2, Vec2)>> {
    // Wider by a little more than rounding, so a face meeting the floor or the box counts.
    let slack = 1e-9 * (1.0 + floor.abs() + blo.len() + bhi.len());
    let mut out = Vec::new();
    for b in bodies {
        let t = Topo::new("", b, origin);
        for f in 0..t.record.faces.len() {
            let range = |d: Vec3| face_bound(&t, f, d).map(|(lo, hi)| (lo - frame.origin.dot(d), hi - frame.origin.dot(d)));
            let (_, ztop) = range(frame.z)?;
            if ztop < floor - slack {
                continue;
            }
            let ((x0, x1), (y0, y1)) = (range(frame.x)?, range(frame.y)?);
            if x1 < blo.x - slack || x0 > bhi.x + slack || y1 < blo.y - slack || y0 > bhi.y + slack {
                continue;
            }
            out.push((Vec2::new(x0 - slack, y0 - slack), Vec2::new(x1 + slack, y1 + slack)));
        }
    }
    Some(out)
}

/// The range of `p · dir` (for a unit `dir`, `p` less `t`'s origin) over face `f` of `t`, from
/// its exact form, never narrower: a plane's over its edges (a linear function on a flat region
/// is extreme on its edge), a cylinder's or cone's over the piece of it between its edges' range
/// along its axis (its rims, and a cone's apex), a sphere's or torus's over the whole of it.
/// `None` when it has no known bound (a free-form face, a face with a lone vertex for a loop).
fn face_bound(t: &Topo<'_>, f: usize, dir: Vec3) -> Option<(f64, f64)> {
    let face = t.record.faces.get(f)?;
    if face.loops.is_empty() || face.loops.iter().any(|l| !matches!(l, BrepLoop::Edges { .. })) {
        return None;
    }
    // How far a circle round axis `k` reaches along `dir` per unit of its radius.
    let side = |k: Vec3| (1.0 - k.dot(dir).powi(2)).max(0.0).sqrt();
    let rim = |c: Vec3, r: f64, k: Vec3| (c.dot(dir) - r.abs() * side(k), c.dot(dir) + r.abs() * side(k));
    let out = match t.surface(f)? {
        FaceSurface::Plane { .. } => t.face_range(f, dir, false)?,
        FaceSurface::Cylinder { origin, axis, radius } => {
            let k = axis.vec();
            let (a0, a1) = t.face_range(f, k, false)?;
            let at = |a: f64| *origin + k * (a - origin.dot(k));
            let ((l0, h0), (l1, h1)) = (rim(at(a0), *radius, k), rim(at(a1), *radius, k));
            (l0.min(l1), h0.max(h1))
        }
        FaceSurface::Cone { apex, axis, half_angle } => {
            let k = axis.vec();
            let (a0, a1) = t.face_range(f, k, false)?;
            let tan = half_angle.tan();
            let at = |a: f64| rim(*apex + k * (a - apex.dot(k)), (a - apex.dot(k)) * tan, k);
            let ((l0, h0), (l1, h1)) = (at(a0), at(a1));
            let v = apex.dot(dir);
            (l0.min(l1).min(v), h0.max(h1).max(v))
        }
        FaceSurface::Sphere { center, radius } => (center.dot(dir) - radius.abs(), center.dot(dir) + radius.abs()),
        FaceSurface::Torus { center, axis, major, minor } => {
            let reach = major.abs() * side(axis.vec()) + minor.abs();
            (center.dot(dir) - reach, center.dot(dir) + reach)
        }
    };
    (out.0.is_finite() && out.1.is_finite()).then_some(out)
}

/// A point (in `frame`'s XY) of an edge of `bodies` that lies over `grown` but not over `face`,
/// each by more than `margin`, between heights `zs` (exclusive below, inclusive above): straight
/// edges exactly (split where they cross either outline, each piece's middle tested), circular
/// ones at points 0.05 mm apart (at most 20000 an edge). Other curves are left to the caller.
fn edges_above(bodies: &[&Body], origin: Vec3, frame: &Frame, face: &[Vec2], grown: &[Vec2], zs: (f64, f64), margin: f64) -> Option<Vec2> {
    let (z0, z1) = zs;
    // No height is above `z0` and at or below `z1` (a face at its setup's top, nothing of the
    // part rising past it): nothing to find, and no band to hold a point to.
    if !(z0 < z1) {
        return None;
    }
    let within = |p: Vec3| {
        let q = Vec2::new(p.x, p.y);
        p.z > z0
            && p.z <= z1
            && inside_polygon(grown, q)
            && boundary_dist(grown, q) > margin
            && !inside_polygon(face, q)
            && boundary_dist(face, q) > margin
    };
    let local = |p: Vec3| frame.to_local(p - origin);
    let glo = Vec2::new(grown.iter().map(|p| p.x).fold(f64::INFINITY, f64::min), grown.iter().map(|p| p.y).fold(f64::INFINITY, f64::min));
    let ghi = Vec2::new(grown.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max), grown.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max));
    for b in bodies {
        let rec = b.record();
        let vertex = |v: u32| rec.vertices.get(v as usize).map(|v| local(p3(v.point)));
        for (i, e) in rec.edges.iter().enumerate() {
            if let Some(c) = edge_circle(b, i) {
                let (center, start, k) = (local(c.center), local(c.start), frame.dir_to_local(c.axis.vec()));
                // An arc wholly below or above the heights, or beside the outline, is passed by.
                let r = c.radius;
                if center.z + r <= z0
                    || center.z - r > z1
                    || center.x + r < glo.x
                    || center.x - r > ghi.x
                    || center.y + r < glo.y
                    || center.y - r > ghi.y
                {
                    continue;
                }
                let v = start - center;
                let n = ((c.radius * c.angle.abs() / 0.05).ceil() as usize).clamp(16, 20_000);
                for j in 0..=n {
                    let t = c.angle * j as f64 / n as f64;
                    let p = center + v * t.cos() + k.cross(v) * t.sin() + k * (k.dot(v) * (1.0 - t.cos()));
                    if within(p) {
                        return Some(Vec2::new(p.x, p.y));
                    }
                }
                continue;
            }
            if !matches!(e.curve, Curve::Line { .. }) {
                continue;
            }
            let (Some(a), Some(b2)) = (vertex(e.vertices[0]), vertex(e.vertices[1])) else { continue };
            // The part of the segment between the heights.
            let d = b2 - a;
            let (mut lo, mut hi) = (0.0f64, 1.0f64);
            if d.z.abs() > 1e-12 {
                let (t0, t1) = ((z0 - a.z) / d.z, (z1 - a.z) / d.z);
                lo = lo.max(t0.min(t1));
                hi = hi.min(t0.max(t1));
            } else if !(a.z > z0 && a.z <= z1) {
                continue;
            }
            if !(hi >= lo) {
                continue;
            }
            // Split where it crosses either outline; the middle of each piece tells.
            let (a2, d2) = (Vec2::new(a.x, a.y), Vec2::new(d.x, d.y));
            let mut ts = vec![lo, hi];
            if d2.len() > 1e-12 {
                for poly in [grown, face] {
                    for (c, f) in poly.iter().zip(poly.iter().cycle().skip(1)) {
                        let w = *f - *c;
                        let den = d2.cross(w);
                        if den.abs() <= 1e-15 {
                            continue;
                        }
                        let t = (*c - a2).cross(w) / den;
                        let u = (*c - a2).cross(d2) / den;
                        if t > lo && t < hi && (-1e-12..=1.0 + 1e-12).contains(&u) {
                            ts.push(t);
                        }
                    }
                }
            }
            ts.sort_by(f64::total_cmp);
            for w in ts.windows(2) {
                let t = 0.5 * (w[0] + w[1]);
                let p = a + d * t;
                // Within the heights: the ends of the piece are its limits, not past them.
                let p = Vec3::new(p.x, p.y, p.z.clamp(z0 + 1e-12, z1));
                if within(p) {
                    return Some(Vec2::new(p.x, p.y));
                }
            }
        }
    }
    None
}

/// The distance from `q` to the outline of the closed polygon `pts`.
fn boundary_dist(pts: &[Vec2], q: Vec2) -> f64 {
    pts.iter()
        .zip(pts.iter().cycle().skip(1))
        .map(|(a, b)| {
            let d = *b - *a;
            let t = if d.len2() > 0.0 { ((q - *a).dot(d) / d.len2()).clamp(0.0, 1.0) } else { 0.0 };
            q.dist(*a + d * t)
        })
        .fold(f64::INFINITY, f64::min)
}

/// Whether `q` is inside the closed polygon `pts` (even-odd).
fn inside_polygon(pts: &[Vec2], q: Vec2) -> bool {
    let mut odd = false;
    for (a, b) in pts.iter().zip(pts.iter().cycle().skip(1)) {
        if (a.y > q.y) != (b.y > q.y) && q.x < a.x + (q.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            odd = !odd;
        }
    }
    odd
}

/// Whether two edges of the closed polygon `pts` that do not share a corner cross.
fn self_crossing(pts: &[Vec2]) -> bool {
    let n = pts.len();
    let edge = |i: usize| (pts.get(i).copied().unwrap_or(Vec2::ZERO), pts.get((i + 1) % n).copied().unwrap_or(Vec2::ZERO));
    let orient = |a: Vec2, b: Vec2, c: Vec2| (b - a).cross(c - a);
    for i in 0..n {
        let (a, b) = edge(i);
        for j in i + 2..n {
            if i == 0 && j == n - 1 {
                continue;
            }
            let (c, d) = edge(j);
            let (d1, d2, d3, d4) = (orient(a, b, c), orient(a, b, d), orient(c, d, a), orient(c, d, b));
            if d1 * d2 < -1e-12 && d3 * d4 < -1e-12 {
                return true;
            }
        }
    }
    false
}

/// The regions and heights of a contour or pocket, and the side a contour takes by default:
/// from the `feature` it names, or from its sketch or polygon and all four `heights`.
pub(super) fn op_cut(
    ctx: &Ctx<'_>,
    op: &str,
    sketch: Option<&crate::schema::SketchRef>,
    profiles: Option<&[usize]>,
    polygon: Option<&[[f64; 2]]>,
    feature: Option<&str>,
    heights: Option<&HeightsPartial>,
    frame: &Frame,
    facing: Option<f64>,
) -> Result<OpCut> {
    if let Some(id) = feature {
        if sketch.is_some() || polygon.is_some() || profiles.is_some() {
            return Err(ctx.bad(op, "give a `feature`, a `sketch` or a `polygon`, one of them"));
        }
        if ctx.place.axes.is_some() {
            return Err(
                ctx.bad(op, "a `feature` is found along the design's own Z: name it in a job without `orientation`, or give a `sketch` or `polygon`")
            );
        }
        let cut = feature_cut(ctx, op, id, frame, heights, facing)?;
        return Ok(OpCut { regions: cut.regions, heights: cut.heights, side: Some(cut.side) });
    }
    let regions = ctx.regions(op, sketch, profiles, polygon, frame)?;
    let h = heights.copied().unwrap_or_default();
    match (h.clearance, h.retract, h.top, h.bottom) {
        (Some(clearance), Some(retract), Some(top), Some(bottom)) => {
            Ok(OpCut { regions, heights: Heights { clearance, retract, top, bottom }, side: None })
        }
        _ => {
            let missing: Vec<String> = [("clearance", h.clearance), ("retract", h.retract), ("top", h.top), ("bottom", h.bottom)]
                .iter()
                .filter(|(_, v)| v.is_none())
                .map(|(k, _)| format!("`{k}`"))
                .collect();
            Err(ctx.bad(
                op,
                format!(
                    "give `heights` with {} (an operation on a sketch or polygon needs clearance, retract, top and bottom; one on a \
                     `feature` takes them from it)",
                    missing.join(", ")
                ),
            ))
        }
    }
}

/// What [`op_cut`] gives.
pub(crate) struct OpCut {
    pub regions: Vec<Region2>,
    pub heights: Heights,
    pub side: Option<SideSpec>,
}

// ---------------------------------------------------------------------------------------------
// Recognition.

/// An edge's exact shape, for its extent along a direction.
enum EdgeShape {
    Circle(EdgeCircle),
    /// Points whose convex hull holds the edge (its ends, a spline's control points).
    Hull(Vec<Vec3>),
    /// A spline's points read along it and how far it can stray from the polyline through them
    /// (mm): tighter than its control points, which stand off it.
    Sampled(Samples),
}

/// Points of a curve or a surface evaluated on its exact form, and how far it can stray from
/// them between (an upper bound read from their second differences, eight times the sag a
/// smooth curve has between them): every point of it lies within `margin` of their hull. Its
/// control points bound it too, and the tighter of the two is taken each way.
struct Samples {
    pts: Vec<Vec3>,
    margin: f64,
    /// Its control points (their hull holds it).
    hull: Vec<Vec3>,
    /// A surface's unit normal at each point, where it has one (empty for a curve).
    normals: Vec<Option<Vec3>>,
    /// A surface's points per row of its grid (0 for a curve).
    cols: usize,
}

impl Samples {
    /// The range of `p · dir` over what they hold, for a unit `dir`.
    fn range(&self, dir: Vec3) -> Option<(f64, f64)> {
        let (lo, hi) = self.pts.iter().map(|p| p.dot(dir)).fold(None, |acc, v| Some(widen(acc, v, v)))?;
        let (lo, hi) = (lo - self.margin, hi + self.margin);
        match self.hull.iter().map(|p| p.dot(dir)).fold(None, |acc, v| Some(widen(acc, v, v))) {
            Some((a, b)) if a.max(lo) <= b.min(hi) => Some((a.max(lo), b.min(hi))),
            _ => Some((lo, hi)),
        }
    }

    /// The most of `f` over their points, plus the margin, for `f` a distance (it changes by no
    /// more than a point moves).
    /// The hull's points bound it too where `f` is convex, as a distance from a line is.
    fn max_of(&self, f: impl Fn(Vec3) -> f64) -> f64 {
        let read = self.pts.iter().fold(0.0f64, |m, p| m.max(f(*p))) + self.margin;
        if self.hull.is_empty() { read } else { read.min(self.hull.iter().fold(0.0f64, |m, p| m.max(f(*p)))) }
    }
}

/// Parameters from `t0` to `t1` that hold every knot of `knots` between and split each span
/// between knots into equal steps (at least two, about `budget` in all), each with the span it
/// starts: `None` past [`MAX_SAMPLE_STEPS`] steps.
fn span_params(knots: &[f64], t0: f64, t1: f64, budget: usize) -> Option<Vec<(f64, usize)>> {
    let mut ends: Vec<f64> = vec![t0];
    for k in knots.iter().copied().filter(|k| k.is_finite() && *k > t0 && *k < t1) {
        if ends.last().is_some_and(|l| k > *l) {
            ends.push(k);
        }
    }
    ends.push(t1);
    let spans = ends.len() - 1;
    let each = (budget / spans.max(1)).max(2);
    if spans * each > MAX_SAMPLE_STEPS {
        return None;
    }
    let mut out = Vec::with_capacity(spans * each + 1);
    for (s, w) in ends.windows(2).enumerate() {
        for k in 0..each {
            out.push((w[0] + (w[1] - w[0]) * k as f64 / each as f64, s));
        }
    }
    out.push((t1, spans));
    Some(out)
}

/// Whether three samples in a row (by the spans they start, [`span_params`]) lie within one
/// span, its end knot included: the first two start the same span, the third that one or the
/// next (its first sample is the knot that ends the span).
fn within_span(a: usize, b: usize, c: usize) -> bool {
    a == b && (c == b || c == b.saturating_add(1))
}

/// Points read along a spline curve (moved by `-origin`), at its knots and between: `None` for
/// other curves (their ends, arcs or control points bound them exactly) and where it does not
/// evaluate.
fn curve_samples(curve: &Curve, origin: Vec3) -> Option<Samples> {
    let (knots, n) = match curve {
        Curve::Spline { knots, points } => (knots, points.len()),
        Curve::RationalSpline { knots, homogeneous } => (knots, homogeneous.len()),
        _ => return None,
    };
    let degree = knots.len().checked_sub(n + 1)?;
    let (t0, t1) = (*knots.get(degree)?, *knots.get(n)?);
    if !(t0.is_finite() && t1.is_finite() && t1 > t0) {
        return None;
    }
    let eval = solidcraft_geom::brep::eval::CurveEval::new(curve).ok()?;
    let params = span_params(knots, t0, t1, CURVE_SAMPLES)?;
    let pts: Vec<(Vec3, usize)> = params.iter().map(|(t, s)| eval.point(*t).map(|p| (p - origin, *s))).collect::<Option<Vec<_>>>()?;
    // The second difference of three in a row within one span, its end knot included (smooth
    // there); the middle one is never a knot.
    let margin =
        pts.windows(3).filter(|w| within_span(w[0].1, w[1].1, w[2].1)).map(|w| (w[0].0 - w[1].0 * 2.0 + w[2].0).len()).fold(0.0f64, f64::max) + 1e-9;
    let pts: Vec<Vec3> = pts.into_iter().map(|(p, _)| p).collect();
    (pts.iter().all(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite()) && margin.is_finite()).then_some(Samples {
        pts,
        margin,
        normals: Vec::new(),
        cols: 0,
        hull: {
            let mut h = Vec::new();
            hull_points(curve, origin, &mut h, 0);
            h
        },
    })
}

/// Points read over a free-form surface's whole parameter box (moved by `-origin`), which holds
/// any face on it, at its knots and between: `None` for the analytic kinds and where it does not
/// evaluate.
fn surface_samples(surface: &solidcore_record::brep::Surface, origin: Vec3) -> Option<Samples> {
    use solidcore_record::brep::Surface as S;
    let (uk, vk) = match surface {
        S::Spline { u_knots, v_knots, .. } | S::RationalSpline { u_knots, v_knots, .. } => (u_knots, v_knots),
        _ => return None,
    };
    let eval = solidcraft_geom::brep::eval::SurfaceEval::new(surface).ok()?;
    let [Some((u0, u1)), Some((v0, v1))] = eval.domain().range else { return None };
    if !(u0.is_finite() && u1.is_finite() && v0.is_finite() && v1.is_finite() && u1 > u0 && v1 > v0) {
        return None;
    }
    let (us, vs) = (span_params(uk, u0, u1, SURFACE_SAMPLES)?, span_params(vk, v0, v1, SURFACE_SAMPLES)?);
    let (nu, nv) = (us.len(), vs.len());
    if nu.saturating_mul(nv) > MAX_SURFACE_POINTS {
        return None;
    }
    let mut grid: Vec<Vec3> = Vec::with_capacity(nu * nv);
    let mut normals: Vec<Option<Vec3>> = Vec::with_capacity(nu * nv);
    for (u, _) in &us {
        for (v, _) in &vs {
            let (p, du, dv) = eval.derivs(*u, *v)?;
            grid.push(p - origin);
            normals.push(du.cross(dv).normalized());
        }
    }
    let g = |i: usize, j: usize| grid.get(i * nv + j).copied().unwrap_or(Vec3::ZERO);
    let span = |p: &[(f64, usize)], i: usize| p.get(i).map_or(usize::MAX, |x| x.1);
    let (mut uu, mut vv, mut uv) = (0.0f64, 0.0f64, 0.0f64);
    for i in 0..nu {
        for j in 0..nv {
            // Three in a row within one span each way; a cell is always within one.
            if i >= 1 && i + 1 < nu && within_span(span(&us, i - 1), span(&us, i), span(&us, i + 1)) {
                uu = uu.max((g(i - 1, j) - g(i, j) * 2.0 + g(i + 1, j)).len());
            }
            if j >= 1 && j + 1 < nv && within_span(span(&vs, j - 1), span(&vs, j), span(&vs, j + 1)) {
                vv = vv.max((g(i, j - 1) - g(i, j) * 2.0 + g(i, j + 1)).len());
            }
            if i + 1 < nu && j + 1 < nv {
                uv = uv.max((g(i + 1, j + 1) - g(i + 1, j) - g(i, j + 1) + g(i, j)).len());
            }
        }
    }
    let margin = uu + vv + 2.0 * uv + 1e-9;
    (grid.iter().all(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite()) && margin.is_finite()).then_some(Samples {
        pts: grid,
        margin,
        normals,
        cols: nv,
        hull: {
            let mut h = Vec::new();
            surface_hull(surface, origin, &mut h);
            h
        },
    })
}

/// Intervals a spline edge is read in ([`curve_samples`]).
const CURVE_SAMPLES: usize = 256;
/// Intervals each way a free-form surface is read in ([`surface_samples`]).
const SURFACE_SAMPLES: usize = 64;
/// Most steps a curve or a surface is read in each way; past it its control net bounds it.
const MAX_SAMPLE_STEPS: usize = 1024;
/// Most points a surface is read at; past it its control net bounds it.
const MAX_SURFACE_POINTS: usize = 40_000;
/// Most points of one body's free-form faces checked against the body for whether a face holds
/// them (each check classifies two points against the whole body; [`Topo::holds`]).
const MAX_TRIM_CHECKS: usize = 400;

/// Whether the window watching this work asked it to stop.
fn cancelled() -> bool {
    solidcraft_kernel::watch::current().is_some_and(|w| w.cancelled())
}

thread_local! {
    /// Points classified against a body to trim a face's surface samples ([`trim_reads`]).
    static TRIM_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many points this thread has classified against a body to tell which samples of a
/// free-form face's surface the face holds: the work count of reading free-form faces.
#[cfg(test)]
pub(crate) fn trim_reads() -> usize {
    TRIM_READS.with(std::cell::Cell::get)
}

/// How the faces round a loop of a face lie.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lie {
    Up,
    Down,
    Mixed,
}

/// A loop of a planar face read in a setup's plane.
struct LoopGeo {
    segs: Vec<Seg2>,
    /// Signed area of the outline.
    area: f64,
    /// The faces on the other side of its edges.
    beside: Vec<usize>,
    /// The faces on the other side of each of `segs`' edges, in `segs`' order.
    seg_beside: Vec<Vec<usize>>,
}

/// One body's topology, in work coordinates.
struct Topo<'a> {
    name: String,
    body: &'a Body,
    record: &'a Brep,
    origin: Vec3,
    surfaces: Vec<Option<FaceSurface>>,
    shapes: Vec<EdgeShape>,
    /// The faces each edge bounds.
    edge_faces: Vec<Vec<usize>>,
    /// The edges of each face, each loop as (edge, forward).
    loops: Vec<Vec<Vec<(usize, bool)>>>,
    /// Each free-form face's surface read on its exact form ([`surface_samples`]), read the
    /// first time the face is asked about ([`Topo::freeform`]): a pick reads the body's faces
    /// afresh, so a face of a plane or a cylinder (or one never asked about) is never read.
    freeform: Vec<std::sync::OnceLock<Option<Freeform>>>,
    /// Points of free-form faces read against the body ([`Topo::holds`]).
    trim_checks: AtomicUsize,
    /// The body's exact box (less `origin`), read when first asked ([`Topo::past_body`]).
    bounds: std::sync::OnceLock<Option<(Vec3, Vec3)>>,
}

/// A free-form face's surface read over its whole parameter box ([`surface_samples`]) and which
/// of those points the face holds, read when first asked ([`Topo::holds`]).
struct Freeform {
    s: Samples,
    /// The range of the face's edges along X, Y and Z.
    spans: [Option<(f64, f64)>; 3],
    /// Each point: 0 not read yet, 1 held, 2 not.
    held: Vec<AtomicU8>,
}

fn p3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

impl<'a> Topo<'a> {
    fn new(name: &str, body: &'a Body, origin: Vec3) -> Topo<'a> {
        let record = body.record();
        let vertex = |v: u32| record.vertices.get(v as usize).map(|v| p3(v.point) - origin);
        let shapes = record
            .edges
            .iter()
            .enumerate()
            .map(|(i, e)| {
                if let Some(c) = edge_circle(body, i) {
                    return EdgeShape::Circle(EdgeCircle { center: c.center - origin, start: c.start - origin, end: c.end - origin, ..c });
                }
                if let Some(s) = curve_samples(&e.curve, origin) {
                    return EdgeShape::Sampled(s);
                }
                let mut pts: Vec<Vec3> = e.vertices.iter().filter_map(|v| vertex(*v)).collect();
                hull_points(&e.curve, origin, &mut pts, 0);
                EdgeShape::Hull(pts)
            })
            .collect();
        let mut edge_faces: Vec<Vec<usize>> = vec![Vec::new(); record.edges.len()];
        let loops: Vec<Vec<Vec<(usize, bool)>>> = record
            .faces
            .iter()
            .enumerate()
            .map(|(fi, f)| {
                f.loops
                    .iter()
                    .filter_map(|l| match l {
                        BrepLoop::Edges { edges } => Some(
                            edges
                                .iter()
                                .map(|oe| {
                                    if let Some(list) = edge_faces.get_mut(oe.edge as usize)
                                        && !list.contains(&fi)
                                    {
                                        list.push(fi);
                                    }
                                    (oe.edge as usize, oe.forward)
                                })
                                .collect(),
                        ),
                        BrepLoop::Vertex { .. } => None,
                    })
                    .collect()
            })
            .collect();
        let surfaces = (0..record.faces.len())
            .map(|f| {
                face_surface(body, f).map(|s| match s {
                    FaceSurface::Plane { normal, point } => FaceSurface::Plane { normal, point: point - origin },
                    FaceSurface::Cylinder { origin: o, axis, radius } => FaceSurface::Cylinder { origin: o - origin, axis, radius },
                    FaceSurface::Cone { apex, axis, half_angle } => FaceSurface::Cone { apex: apex - origin, axis, half_angle },
                    FaceSurface::Sphere { center, radius } => FaceSurface::Sphere { center: center - origin, radius },
                    FaceSurface::Torus { center, axis, major, minor } => FaceSurface::Torus { center: center - origin, axis, major, minor },
                })
            })
            .collect();
        let freeform = record.faces.iter().map(|_| std::sync::OnceLock::new()).collect();
        Topo {
            name: name.to_string(),
            body,
            record,
            origin,
            surfaces,
            shapes,
            edge_faces,
            loops,
            freeform,
            trim_checks: AtomicUsize::new(0),
            bounds: std::sync::OnceLock::new(),
        }
    }

    /// Free-form face `f`'s surface read on its exact form ([`Topo::freeform_of`]), read once,
    /// when first asked for; `None` for an analytic face and one that does not evaluate.
    fn freeform(&self, f: usize) -> Option<&Freeform> {
        if self.surface(f).is_some() {
            return None;
        }
        self.freeform
            .get(f)?
            .get_or_init(|| self.record.faces.get(f).and_then(|face| surface_samples(face.parameters(), self.origin)).map(|s| self.freeform_of(f, s)))
            .as_ref()
    }

    /// Face `f`'s surface read over its whole parameter box, with the range of its edges
    /// along X, Y and Z: which points the face holds is read only when asked ([`Topo::holds`]).
    fn freeform_of(&self, f: usize, s: Samples) -> Freeform {
        let spans = [Vec3::X, Vec3::Y, Vec3::Z]
            .map(|a| self.face_edges(f).filter_map(|e| self.edge_range(e, a)).fold(None, |acc, (lo, hi)| Some(widen(acc, lo, hi))));
        let held = s.pts.iter().map(|_| AtomicU8::new(0)).collect();
        Freeform { s, spans, held }
    }

    /// Whether `p` (less `origin`) lies clearly outside the body's exact box: no face of the body
    /// holds it, so it need not be classified against the body.
    fn past_body(&self, p: Vec3) -> bool {
        let Some((lo, hi)) = *self.bounds.get_or_init(|| {
            let b = self.body.exact_bounds().ok()?;
            (b.min.is_finite() && b.max.is_finite()).then(|| (b.min - self.origin, b.max - self.origin))
        }) else {
            return false;
        };
        let pad = 1e-3_f64.max((hi - lo).len() * 1e-6);
        p.x < lo.x - pad || p.y < lo.y - pad || p.z < lo.z - pad || p.x > hi.x + pad || p.y > hi.y + pad || p.z > hi.z + pad
    }

    /// Whether free-form face `ff` holds its surface's point `k`: a surface's parameter box can
    /// run past the face trimmed from it. One inside the box the face's edges span is held (the
    /// face holds it, or its edges bound it there); one outside it is held where the body's
    /// boundary passes through it, where that cannot be told, and past [`MAX_TRIM_CHECKS`]
    /// points read for this body (its control net no tighter). Each answer is kept.
    fn holds(&self, ff: &Freeform, k: usize) -> bool {
        let (Some(p), Some(seen)) = (ff.s.pts.get(k).copied(), ff.held.get(k)) else { return true };
        match seen.load(Ordering::Relaxed) {
            1 => return true,
            2 => return false,
            _ => {}
        }
        let m = ff.s.margin;
        let inside = [Vec3::X, Vec3::Y, Vec3::Z].iter().zip(&ff.spans).all(|(a, r)| r.is_some_and(|(lo, hi)| (lo - m..=hi + m).contains(&p.dot(*a))));
        let held = inside
            || (!self.past_body(p) && {
                let read = self.trim_checks.fetch_add(1, Ordering::Relaxed);
                if let Some(w) = solidcraft_kernel::watch::current() {
                    w.progress("reading free-form faces", read.min(MAX_TRIM_CHECKS), MAX_TRIM_CHECKS);
                }
                read >= MAX_TRIM_CHECKS
                    || cancelled()
                    || match ff.s.normals.get(k).copied().flatten() {
                        None => true,
                        Some(n) => {
                            let eps = 1e-4 * (1.0 + p_scale(self));
                            let side = |q: Vec3| {
                                TRIM_READS.with(|c| c.set(c.get().saturating_add(1)));
                                self.body.classify_point(q + self.origin).ok()
                            };
                            match (side(p + n * eps), side(p - n * eps)) {
                                (Some(a), Some(b)) => a != b && a != PointClass::OnBoundary && b != PointClass::OnBoundary,
                                _ => true,
                            }
                        }
                    }
            });
        seen.store(if held { 1 } else { 2 }, Ordering::Relaxed);
        held
    }

    /// The most of `f` over the points of `ff`'s surface the face holds that read above
    /// `floor`: `None` where none does. Only points no lower than each neighbour in the grid
    /// are read against the body, highest first: a held point above `floor` that is not one
    /// climbs to one held, or crosses the face's edge, where `f` reads no more than the edges'
    /// most, so `floor` (that most plus the margin) bounds it.
    fn freeform_max(&self, ff: &Freeform, f: impl Fn(Vec3) -> f64, floor: f64) -> Option<f64> {
        let cols = ff.s.cols.max(1);
        let vals: Vec<f64> = ff.s.pts.iter().map(|p| f(*p)).collect();
        let rows = vals.len() / cols;
        let at = |i: usize, j: usize| vals.get(i * cols + j).copied();
        let peak = |k: usize, v: f64| {
            let (i, j) = (k / cols, k % cols);
            (i.saturating_sub(1)..=(i + 1).min(rows.saturating_sub(1)))
                .all(|a| (j.saturating_sub(1)..=(j + 1).min(cols - 1)).all(|b| at(a, b).is_none_or(|w| w <= v)))
        };
        let mut cands: Vec<(usize, f64)> = vals.iter().enumerate().filter(|(k, v)| **v > floor && peak(*k, **v)).map(|(k, v)| (k, *v)).collect();
        cands.sort_by(|a, b| b.1.total_cmp(&a.1));
        cands.into_iter().find(|(k, _)| self.holds(ff, *k)).map(|(_, v)| v)
    }

    /// The range of `p · dir` over free-form face `ff`, its edges reading `edges` there: the
    /// edges' range widened by the margin and to the points it holds past that
    /// ([`Topo::freeform_max`]), within its control net.
    fn freeform_range(&self, ff: &Freeform, dir: Vec3, edges: Option<(f64, f64)>) -> Option<(f64, f64)> {
        let Some((elo, ehi)) = edges else { return ff.s.range(dir) };
        let m = ff.s.margin;
        let hi = self.freeform_max(ff, |p| p.dot(dir), ehi + m).unwrap_or(ehi) + m;
        let lo = -(self.freeform_max(ff, |p| -p.dot(dir), m - elo).unwrap_or(-elo) + m);
        match ff.s.hull.iter().map(|p| p.dot(dir)).fold(None, |acc, v| Some(widen(acc, v, v))) {
            Some((a, b)) if a.max(lo) <= b.min(hi) => Some((a.max(lo), b.min(hi))),
            _ => Some((lo, hi)),
        }
    }

    fn surface(&self, f: usize) -> Option<&FaceSurface> {
        self.surfaces.get(f).and_then(Option::as_ref)
    }

    fn face_edges(&self, f: usize) -> impl Iterator<Item = usize> + '_ {
        self.loops.get(f).into_iter().flatten().flatten().map(|(e, _)| *e)
    }

    /// The range of `p · dir` over edge `e`, exactly for lines and circles.
    fn edge_range(&self, e: usize, dir: Vec3) -> Option<(f64, f64)> {
        match self.shapes.get(e)? {
            EdgeShape::Circle(c) => Some(circle_range(c, dir)),
            EdgeShape::Hull(pts) => pts.iter().map(|p| p.dot(dir)).fold(None, |acc, v| Some(widen(acc, v, v))),
            EdgeShape::Sampled(s) => s.range(dir),
        }
    }

    /// The range of `p · dir` over face `f`'s boundary, and with `whole` over the face: the
    /// boundary bounds a face of lines and circles on a plane, cylinder or cone; a sphere, torus
    /// or free-form face may rise past it, so its own extent is added.
    fn face_range(&self, f: usize, dir: Vec3, whole: bool) -> Option<(f64, f64)> {
        let mut r = self.face_edges(f).filter_map(|e| self.edge_range(e, dir)).fold(None, |acc, (lo, hi)| Some(widen(acc, lo, hi)));
        if whole {
            // Where the body's boundary passes through `p` (the side along `n` and the other
            // classify differently): a point of the surface the face holds.
            let eps = 1e-4 * (1.0 + p_scale(self));
            let on = |p: Vec3, n: Vec3| {
                if self.past_body(p) {
                    return false;
                }
                let side = |q: Vec3| self.body.classify_point(q + self.origin).ok();
                match (side(p + n * eps), side(p - n * eps)) {
                    (Some(a), Some(b)) => a != b && a != PointClass::OnBoundary && b != PointClass::OnBoundary,
                    _ => true,
                }
            };
            let extra = match self.surface(f).cloned() {
                // A sphere's or torus's farthest points along `dir` count where the face holds
                // them; else its edges bound it (a dome's cap, a fillet's quarter).
                Some(FaceSurface::Sphere { center, radius }) => {
                    let c = center.dot(dir);
                    let lo = on(center - dir * radius, -dir).then_some(c - radius);
                    let hi = on(center + dir * radius, dir).then_some(c + radius);
                    lo.into_iter().chain(hi).fold(None, |acc, v| Some(widen(acc, v, v)))
                }
                Some(FaceSurface::Torus { center, axis, major, minor }) => {
                    let a = axis.vec();
                    let along = a.dot(dir);
                    if along.abs() > 1.0 - PARALLEL {
                        // The rims of the tube farthest along the axis: circles of the major radius.
                        let u = a.cross(if a.x.abs() < 0.9 { Vec3::X } else { Vec3::Y }).normalized().unwrap_or(Vec3::X);
                        let w = a.cross(u);
                        let rim = |s: f64| {
                            (0..72).any(|k| {
                                let t = std::f64::consts::TAU * f64::from(k) / 72.0;
                                on(center + (u * t.cos() + w * t.sin()) * major + a * (s * minor), a * s)
                            })
                        };
                        let c = center.dot(dir);
                        let sign = along.signum();
                        let lo = rim(-sign).then_some(c - minor);
                        let hi = rim(sign).then_some(c + minor);
                        lo.into_iter().chain(hi).fold(None, |acc, v| Some(widen(acc, v, v)))
                    } else {
                        let c = center.dot(dir);
                        let reach = major * (1.0 - along * along).max(0.0).sqrt() + minor;
                        Some((c - reach, c + reach))
                    }
                }
                Some(_) => None,
                // A free-form face: its surface read on its exact form, else its control net.
                None => match self.freeform(f) {
                    Some(ff) => self.freeform_range(ff, dir, r),
                    None => self.record.faces.get(f).and_then(|face| {
                        let mut pts = Vec::new();
                        surface_hull(face.parameters(), self.origin, &mut pts);
                        pts.iter().map(|p| p.dot(dir)).fold(None, |acc, v| Some(widen(acc, v, v)))
                    }),
                },
            };
            if let Some((lo, hi)) = extra {
                r = Some(widen(r, lo, hi));
            }
        }
        r
    }

    /// The largest distance of face `f`'s edges from the line through `origin` along the unit
    /// `axis`: circles exactly, other edges by their control points (an upper bound).
    fn edges_reach(&self, f: usize, origin: Vec3, axis: Vec3) -> f64 {
        let off = |p: Vec3| {
            let d = p - origin;
            (d - axis * d.dot(axis)).len()
        };
        let mut out: f64 = 0.0;
        for e in self.face_edges(f) {
            match self.shapes.get(e) {
                Some(EdgeShape::Circle(c)) => out = out.max(off(c.center) + c.radius),
                Some(EdgeShape::Hull(pts)) => out = pts.iter().fold(out, |m, p| m.max(off(*p))),
                Some(EdgeShape::Sampled(s)) => out = out.max(s.max_of(off)),
                None => {}
            }
        }
        out
    }

    /// The largest distance of face `f` from the work X axis (an upper bound).
    fn face_swing(&self, f: usize) -> f64 {
        let yz = |p: Vec3| p.y.hypot(p.z);
        let mut out: f64 = 0.0;
        for e in self.face_edges(f) {
            match self.shapes.get(e) {
                Some(EdgeShape::Circle(c)) => out = out.max(yz(c.center) + c.radius),
                Some(EdgeShape::Hull(pts)) => out = pts.iter().fold(out, |m, p| m.max(yz(*p))),
                Some(EdgeShape::Sampled(s)) => out = out.max(s.max_of(yz)),
                None => {}
            }
        }
        match self.surface(f) {
            Some(FaceSurface::Sphere { center, radius }) => out = out.max(yz(*center) + radius),
            Some(FaceSurface::Torus { center, major, minor, .. }) => out = out.max(yz(*center) + major + minor),
            Some(_) => {}
            None => match self.freeform(f) {
                Some(ff) => {
                    let m = ff.s.margin;
                    let read = self.freeform_max(ff, yz, out + m).unwrap_or(out) + m;
                    let net = ff.s.hull.iter().map(|p| yz(*p)).fold(None, |acc: Option<f64>, v| Some(acc.map_or(v, |a| a.max(v))));
                    out = out.max(net.map_or(read, |h| read.min(h)));
                }
                None => {
                    if let Some(face) = self.record.faces.get(f) {
                        let mut pts = Vec::new();
                        surface_hull(face.parameters(), self.origin, &mut pts);
                        out = pts.iter().fold(out, |m, p| m.max(yz(*p)));
                    }
                }
            },
        }
        out
    }

    /// Face `f`'s loops read in the plane at height `z` of `frame` (square to its Z).
    fn face_loops(&self, f: usize, frame: &Frame, z: f64, tol: f64) -> Vec<LoopGeo> {
        let Some(loops) = self.loops.get(f) else { return Vec::new() };
        let plane = Plane { origin: frame.to_world(Vec3::new(0.0, 0.0, z)) + self.origin, x: frame.x, y: frame.y };
        let all: Vec<usize> = loops.iter().flatten().map(|(e, _)| *e).collect();
        let read = self.body.planar_edges(&plane, &all);
        let mut at = 0usize;
        let mut out = Vec::with_capacity(loops.len());
        for lp in loops {
            let mut segs: Vec<Seg2> = Vec::new();
            let mut beside: Vec<usize> = Vec::new();
            let mut seg_beside: Vec<Vec<usize>> = Vec::new();
            for (e, forward) in lp {
                let pieces = read.get(at).cloned().flatten();
                at += 1;
                let mut these: Vec<Seg2> = match pieces {
                    Some(ps) => ps.iter().flat_map(piece_segs).collect(),
                    None => self.own_segs(*e, frame),
                };
                if !forward {
                    these.reverse();
                    for s in &mut these {
                        *s = reversed(s);
                    }
                }
                let across: Vec<usize> = self.edge_faces.get(*e).into_iter().flatten().copied().filter(|g| *g != f).collect();
                seg_beside.extend(std::iter::repeat_n(across.clone(), these.len()));
                segs.extend(these);
                for g in across {
                    if !beside.contains(&g) {
                        beside.push(g);
                    }
                }
            }
            let area = loop_area(&segs, tol);
            out.push(LoopGeo { segs, area, beside, seg_beside });
        }
        out
    }

    /// Edge `e` in `frame`'s XY from its shape: a line between its ends, or its arc.
    fn own_segs(&self, e: usize, frame: &Frame) -> Vec<Seg2> {
        let local = |p: Vec3| {
            let l = frame.to_local(p);
            Vec2::new(l.x, l.y)
        };
        match self.shapes.get(e) {
            Some(EdgeShape::Circle(c)) => {
                let n = frame.dir_to_local(c.axis.vec());
                let center = local(c.center);
                if n.z.abs() > 1.0 - PARALLEL {
                    let s = local(c.start) - center;
                    let sweep = if n.z > 0.0 { c.angle } else { -c.angle };
                    vec![Seg2::Arc { center, radius: c.radius, start: s.y.atan2(s.x), sweep }]
                } else {
                    vec![Seg2::Line { a: local(c.start), b: local(c.end) }]
                }
            }
            _ => {
                let Some(edge) = self.record.edges.get(e) else { return Vec::new() };
                let at = |v: u32| self.record.vertices.get(v as usize).map(|v| local(p3(v.point) - self.origin));
                match (at(edge.vertices[0]), at(edge.vertices[1])) {
                    (Some(a), Some(b)) if a.dist(b) > 1e-12 => vec![Seg2::Line { a, b }],
                    _ => Vec::new(),
                }
            }
        }
    }
}

/// A length the size of the body's coordinates, for a step off its boundary.
fn p_scale(t: &Topo<'_>) -> f64 {
    t.origin.len()
}

fn widen(acc: Option<(f64, f64)>, lo: f64, hi: f64) -> (f64, f64) {
    match acc {
        Some((a, b)) => (a.min(lo), b.max(hi)),
        None => (lo, hi),
    }
}

/// Points whose hull holds `curve` (its control net), moved by `-origin`.
fn hull_points(curve: &Curve, origin: Vec3, out: &mut Vec<Vec3>, depth: usize) {
    match curve {
        Curve::Line { .. } | Curve::Circle { .. } => {}
        Curve::Ellipse { frame, radii } => {
            let (o, x, y) = (p3(frame.origin), p3(frame.x_axis), p3(frame.y_axis));
            for (sx, sy) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
                out.push(o + x * (radii[0] * sx) + y * (radii[1] * sy) - origin);
            }
        }
        Curve::Polyline { points } | Curve::Spline { points, .. } => out.extend(points.iter().take(100_000).map(|p| p3(*p) - origin)),
        Curve::RationalSpline { homogeneous, .. } => out.extend(
            homogeneous.iter().take(100_000).filter(|h| h[3].abs() > 1e-300).map(|h| Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]) - origin),
        ),
        Curve::SurfaceIntersection { leader, .. } if depth < 4 => hull_points(leader, origin, out, depth + 1),
        Curve::SurfaceIntersection { .. } => {}
    }
}

/// The control net of a free-form surface, moved by `-origin` (nothing for the analytic kinds).
fn surface_hull(surface: &solidcore_record::brep::Surface, origin: Vec3, out: &mut Vec<Vec3>) {
    use solidcore_record::brep::Surface as S;
    match surface {
        S::Spline { points, .. } => out.extend(points.iter().flatten().take(1_000_000).map(|p| p3(*p) - origin)),
        S::RationalSpline { homogeneous, .. } => out.extend(
            homogeneous
                .iter()
                .flatten()
                .take(1_000_000)
                .filter(|h| h[3].abs() > 1e-300)
                .map(|h| Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]) - origin),
        ),
        _ => {}
    }
}

/// The range of `p · dir` over the arc `c`.
fn circle_range(c: &EdgeCircle, dir: Vec3) -> (f64, f64) {
    let ends = (c.start.dot(dir), c.end.dot(dir));
    let mut r = (ends.0.min(ends.1), ends.0.max(ends.1));
    let Some(u) = (c.start - c.center).normalized() else { return r };
    let w = c.axis.vec().cross(u);
    let (a, b) = (u.dot(dir), w.dot(dir));
    let amp = a.hypot(b) * c.radius;
    let peak = b.atan2(a).rem_euclid(TAU);
    let within = |t: f64| t.rem_euclid(TAU) <= c.angle + 1e-12;
    let base = c.center.dot(dir);
    if within(peak) {
        r.1 = r.1.max(base + amp);
    }
    if within(peak + PI) {
        r.0 = r.0.min(base - amp);
    }
    r
}

// ---------------------------------------------------------------------------------------------
// 2D outlines.

fn reversed(s: &Seg2) -> Seg2 {
    match *s {
        Seg2::Line { a, b } => Seg2::Line { a: b, b: a },
        Seg2::Arc { center, radius, start, sweep } => Seg2::Arc { center, radius, start: start + sweep, sweep: -sweep },
        Seg2::Cubic { p0, p1, p2, p3 } => Seg2::Cubic { p0: p3, p1: p2, p2: p1, p3: p0 },
        Seg2::Conic { a, apex, b, w } => Seg2::Conic { a: b, apex, b: a, w },
        Seg2::Bezier { n, p } => {
            let mut q = p;
            let k = usize::from(n).min(q.len().saturating_sub(1));
            if let Some(head) = q.get_mut(..=k) {
                head.reverse();
            }
            Seg2::Bezier { n, p: q }
        }
    }
}

/// Where `a` and `b` run along the same line or circle the opposite ways over more than `tol`,
/// what is left of each once that shared stretch is taken out of both (the edge two faces seen
/// side by side share); `None` where they share no stretch. Lines and arcs are compared exactly;
/// other curves only when one is the other reversed.
fn cancel(a: &Seg2, b: &Seg2, tol: f64) -> Option<(Vec<Seg2>, Vec<Seg2>)> {
    match (*a, *b) {
        (Seg2::Line { a: p, b: q }, Seg2::Line { a: r, b: s }) => {
            let la = p.dist(q);
            let u = unit2(q - p)?;
            let v = unit2(s - r)?;
            if u.dot(v) > -1.0 + 1e-9 {
                return None;
            }
            let off = |x: Vec2| ((x - p).x * u.y - (x - p).y * u.x).abs();
            if off(r) > tol || off(s) > tol {
                return None;
            }
            let at = |x: Vec2| (x - p).dot(u);
            let (tr, ts) = (at(r), at(s));
            let (lo, hi) = (ts.max(0.0), tr.min(la));
            if hi - lo <= tol {
                return None;
            }
            let pt = |t: f64| p + u * t;
            let mut left_a = Vec::new();
            if lo > tol {
                left_a.push(Seg2::Line { a: p, b: pt(lo) });
            }
            if la - hi > tol {
                left_a.push(Seg2::Line { a: pt(hi), b: q });
            }
            let mut left_b = Vec::new();
            if tr - hi > tol {
                left_b.push(Seg2::Line { a: r, b: pt(hi) });
            }
            if lo - ts > tol {
                left_b.push(Seg2::Line { a: pt(lo), b: s });
            }
            Some((left_a, left_b))
        }
        (Seg2::Arc { center: ca, radius: ra, start: sa, sweep: wa }, Seg2::Arc { center: cb, radius: rb, start: sb, sweep: wb }) => {
            if ca.dist(cb) > tol || (ra - rb).abs() > tol || wa * wb >= 0.0 || !(ra > tol) {
                return None;
            }
            // Each as the angles it covers counter-clockwise.
            let span = |s: f64, w: f64| if w > 0.0 { (s, s + w) } else { (s + w, s) };
            let (a0, a1) = span(sa, wa);
            let (b0, b1) = span(sb, wb);
            let (k, lo, hi) = (-2..=2)
                .map(|k| {
                    let shift = std::f64::consts::TAU * f64::from(k);
                    (shift, a0.max(b0 + shift), a1.min(b1 + shift))
                })
                .max_by(|x, y| (x.2 - x.1).total_cmp(&(y.2 - y.1)))?;
            if (hi - lo) * ra <= tol {
                return None;
            }
            let arc = |center: Vec2, radius: f64, from: f64, to: f64, ccw: bool| {
                if ccw {
                    Seg2::Arc { center, radius, start: from, sweep: to - from }
                } else {
                    Seg2::Arc { center, radius, start: to, sweep: from - to }
                }
            };
            let mut left_a = Vec::new();
            if (lo - a0) * ra > tol {
                left_a.push(arc(ca, ra, a0, lo, wa > 0.0));
            }
            if (a1 - hi) * ra > tol {
                left_a.push(arc(ca, ra, hi, a1, wa > 0.0));
            }
            let mut left_b = Vec::new();
            if (lo - (b0 + k)) * rb > tol {
                left_b.push(arc(cb, rb, b0 + k, lo, wb > 0.0));
            }
            if ((b1 + k) - hi) * rb > tol {
                left_b.push(arc(cb, rb, hi, b1 + k, wb > 0.0));
            }
            // Each piece in the order its curve runs.
            if wa < 0.0 {
                left_a.reverse();
            }
            if wb < 0.0 {
                left_b.reverse();
            }
            Some((left_a, left_b))
        }
        (Seg2::Line { .. } | Seg2::Arc { .. }, _) | (_, Seg2::Line { .. } | Seg2::Arc { .. }) => None,
        _ => {
            let back = reversed(b);
            [0.0, 0.25, 0.5, 0.75, 1.0].iter().all(|t| seg_point(a, *t).dist(seg_point(&back, *t)) <= tol).then(|| (Vec::new(), Vec::new()))
        }
    }
}

/// Most upright faces [`wall_outline`] reads.
const MAX_WALL_FACES: usize = 2000;

/// The part's outline seen along the setup's axis (`fr.z`) from its outermost upright flat walls,
/// for a part whose top and bottom do not give it (a slot or notch splitting the top, chamfered or
/// rounded edges all round): each upright plane's footprint (its exact extent along the wall),
/// kept where nowhere along the setup's height (`lo` to `hi`, every level a vertex of the part is
/// at and between) the part is just outside it, chained into loops; the largest, counter-
/// clockwise, with the first wall's face. `None` where the footprints do not close up (an outline
/// with round corners or drafted walls).
fn wall_outline(t: &Topo<'_>, fr: &Frame, lo: f64, hi: f64, tol: f64) -> Option<(usize, Vec<Seg2>)> {
    if !(hi > lo + tol) {
        return None;
    }
    let eps = (tol * 5.0).max(1e-3);
    // The levels to read at: between every two heights a vertex of the part is at, and evenly.
    let mut zs: Vec<f64> = t.record.vertices.iter().map(|v| fr.to_local(p3(v.point) - t.origin).z).filter(|z| *z > lo && *z < hi).collect();
    zs.extend([lo, hi]);
    zs.sort_by(f64::total_cmp);
    zs.dedup_by(|a, b| (*a - *b).abs() <= tol);
    let mut levels: Vec<f64> = zs.windows(2).filter_map(|w| Some(0.5 * (w.first()? + w.get(1)?))).collect();
    levels.extend((1..16).map(|i| lo + (hi - lo) * f64::from(i) / 16.0));
    levels.truncate(512);
    let solid_at = |q: Vec2, z: f64| matches!(t.body.classify_point(fr.to_world(Vec3::new(q.x, q.y, z)) + t.origin), Ok(PointClass::Inside));
    let mut walls: Vec<(usize, Seg2)> = Vec::new();
    let mut read = 0usize;
    for f in 0..t.record.faces.len() {
        let Some(FaceSurface::Plane { normal, point }) = t.surface(f) else { continue };
        let n = normal.vec();
        if fr.dir_to_local(n).z.abs() > SQUARE {
            continue;
        }
        read += 1;
        if read > MAX_WALL_FACES {
            return None;
        }
        let d = n.cross(fr.z);
        let dl = d.len();
        if !(dl > 1e-9) {
            continue;
        }
        let d = d * (1.0 / dl);
        let Some((a, b)) = t.face_range(f, d, false) else { continue };
        let at = point.dot(d);
        let (pa, pb) = (fr.to_local(*point + d * (a - at)), fr.to_local(*point + d * (b - at)));
        let (pa, pb) = (Vec2::new(pa.x, pa.y), Vec2::new(pb.x, pb.y));
        if pa.dist(pb) <= tol {
            continue;
        }
        let nl = fr.dir_to_local(n);
        let Some(n2) = Vec2::new(nl.x, nl.y).normalized() else { continue };
        // Outermost: on one side of it the part is nowhere, at its middle and near both ends,
        // and that side is the outside.
        let along = pb - pa;
        let clear = |o: Vec2| [0.5, 0.05, 0.95].iter().all(|k| levels.iter().all(|z| !solid_at(pa + along * *k + o * eps, *z)));
        let Some(out) = [n2, n2 * -1.0].into_iter().find(|o| clear(*o)) else { continue };
        let seg = if along.cross(out) < 0.0 { Seg2::Line { a: pa, b: pb } } else { Seg2::Line { a: pb, b: pa } };
        walls.push((f, seg));
    }
    // Footprints along one line, one inside another, are one wall (a wall split by a slot above
    // its foot): the longest kept.
    let len = |s: &Seg2| seg_point(s, 0.0).dist(seg_point(s, 1.0));
    let mut keep: Vec<(usize, Seg2)> = Vec::new();
    let mut order: Vec<&(usize, Seg2)> = walls.iter().collect();
    order.sort_by(|x, y| len(&y.1).total_cmp(&len(&x.1)));
    for w in order {
        let (a, b) = (seg_point(&w.1, 0.0), seg_point(&w.1, 1.0));
        let inside = keep.iter().any(|(_, k)| {
            let (ka, kb) = (seg_point(k, 0.0), seg_point(k, 1.0));
            let dir = kb - ka;
            let l = dir.len();
            l > tol
                && [a, b]
                    .iter()
                    .all(|p| (dir.cross(*p - ka) / l).abs() <= tol && (*p - ka).dot(dir) >= -tol * l && (*p - ka).dot(dir) <= l * l + tol * l)
        });
        if !inside {
            keep.push(*w);
        }
    }
    let first = keep.iter().map(|(f, _)| *f).min()?;
    let segs: Vec<Seg2> = keep.iter().map(|(_, s)| *s).collect();
    let join = (tol * 10.0).max(1e-6);
    // Chained end to start; every footprint must be used in one loop or another.
    let mut used = vec![false; segs.len()];
    let mut loops: Vec<Vec<Seg2>> = Vec::new();
    while let Some(i0) = used.iter().position(|u| !*u) {
        let s0 = *segs.get(i0)?;
        if let Some(u) = used.get_mut(i0) {
            *u = true;
        }
        let start = seg_point(&s0, 0.0);
        let mut end = seg_point(&s0, 1.0);
        let mut chain = vec![s0];
        while end.dist(start) > join {
            let k = (0..segs.len())
                .filter(|k| !used.get(*k).copied().unwrap_or(true))
                .filter_map(|k| Some((k, seg_point(segs.get(k)?, 0.0).dist(end))))
                .filter(|(_, d)| *d <= join)
                .min_by(|x, y| x.1.total_cmp(&y.1))?
                .0;
            if let Some(u) = used.get_mut(k) {
                *u = true;
            }
            let s = *segs.get(k)?;
            end = seg_point(&s, 1.0);
            chain.push(s);
        }
        loops.push(chain);
    }
    let best = loops.into_iter().max_by(|a, b| loop_area(a, tol).abs().total_cmp(&loop_area(b, tol).abs()))?;
    let area = loop_area(&best, tol);
    (area.abs() > tol * tol).then(|| (first, oriented(&best, area, true)))
}

/// Most segments [`union_outline`] takes.
const MAX_UNION_SEGS: usize = 4000;

/// The outline of the faces whose outer loops (each counter-clockwise in one plane) are `loops`,
/// seen together: the stretches two of them share (running opposite ways) taken out, the rest
/// chained into loops. The largest loop is returned, counter-clockwise; `None` when the pieces
/// do not close up (faces that overlap rather than meet).
fn union_outline(loops: &[Vec<Seg2>], tol: f64) -> Option<Vec<Seg2>> {
    let mut segs: Vec<Seg2> = loops.iter().flatten().copied().collect();
    if segs.is_empty() || segs.len() > MAX_UNION_SEGS {
        return None;
    }
    // Take out every shared stretch.
    let mut i = 0;
    while i < segs.len() {
        let mut j = i + 1;
        let mut changed = false;
        while j < segs.len() {
            let (Some(a), Some(b)) = (segs.get(i), segs.get(j)) else { break };
            if let Some((left_a, left_b)) = cancel(a, b, tol) {
                segs.remove(j);
                segs.remove(i);
                segs.extend(left_a);
                segs.extend(left_b);
                changed = true;
                break;
            }
            j += 1;
        }
        if !changed {
            i += 1;
        } else {
            i = 0;
        }
        if segs.len() > MAX_UNION_SEGS {
            return None;
        }
    }
    // Chain what is left into loops.
    let join = (tol * 10.0).max(1e-6);
    let mut used = vec![false; segs.len()];
    let mut out: Vec<Vec<Seg2>> = Vec::new();
    while let Some(first) = used.iter().position(|u| !*u) {
        let Some(s0) = segs.get(first).copied() else { break };
        used[first] = true;
        let start = seg_point(&s0, 0.0);
        let mut chain = vec![s0];
        let mut end = seg_point(&s0, 1.0);
        while end.dist(start) > join {
            let next = segs
                .iter()
                .enumerate()
                .filter(|(k, _)| !used.get(*k).copied().unwrap_or(true))
                .map(|(k, s)| (k, seg_point(s, 0.0).dist(end)))
                .filter(|(_, d)| *d <= join)
                .min_by(|x, y| x.1.total_cmp(&y.1))?;
            used[next.0] = true;
            let s = segs.get(next.0).copied()?;
            end = seg_point(&s, 1.0);
            chain.push(s);
        }
        out.push(chain);
    }
    let best = out.into_iter().max_by(|a, b| loop_area(a, tol).abs().total_cmp(&loop_area(b, tol).abs()))?;
    let area = loop_area(&best, tol);
    Some(oriented(&best, area, true))
}

/// A piece read in a plane as segments: a spline as lines within `tol`.
fn piece_segs(p: &PlanarPiece) -> Vec<Seg2> {
    match p {
        PlanarPiece::Seg(s) => vec![*s],
        PlanarPiece::Spline(sp) => {
            let (Some(t0), Some(t1)) = (sp.knots.get(sp.degree), sp.knots.len().checked_sub(sp.degree + 1).and_then(|i| sp.knots.get(i))) else {
                return Vec::new();
            };
            let n = (sp.points.len() * 16).clamp(16, MAX_SAMPLES);
            let pts: Vec<Vec2> = (0..=n).filter_map(|i| spline_point(sp, t0 + (t1 - t0) * i as f64 / n as f64)).collect();
            pts.windows(2)
                .filter_map(|w| match w {
                    [a, b] if a.dist(*b) > 1e-12 => Some(Seg2::Line { a: *a, b: *b }),
                    _ => None,
                })
                .collect()
        }
    }
}

/// A point of a planar B-spline (de Boor, rational).
fn spline_point(sp: &solidcraft_kernel::PlanarSpline, t: f64) -> Option<Vec2> {
    let p = sp.degree;
    let n = sp.points.len();
    if n == 0 || sp.knots.len() != n + p + 1 || sp.weights.len() != n {
        return None;
    }
    let mut k = p;
    while k + 1 < n && *sp.knots.get(k + 1)? <= t {
        k += 1;
    }
    let mut d: Vec<[f64; 3]> = (0..=p)
        .map(|j| {
            let i = (j + k).checked_sub(p)?;
            let (q, w) = (sp.points.get(i)?, *sp.weights.get(i)?);
            Some([q.x * w, q.y * w, w])
        })
        .collect::<Option<_>>()?;
    for r in 1..=p {
        for j in (r..=p).rev() {
            let i = (j + k).checked_sub(p)?;
            let (lo, hi) = (*sp.knots.get(i)?, *sp.knots.get(i + p + 1 - r)?);
            let alpha = if (hi - lo).abs() > 1e-300 { (t - lo) / (hi - lo) } else { 0.0 };
            let (a, b) = (*d.get(j - 1)?, *d.get(j)?);
            *d.get_mut(j)? = [a[0] + (b[0] - a[0]) * alpha, a[1] + (b[1] - a[1]) * alpha, a[2] + (b[2] - a[2]) * alpha];
        }
    }
    let h = d.get(p)?;
    (h[2].abs() > 1e-300).then(|| Vec2::new(h[0] / h[2], h[1] / h[2]))
}

/// A point of `s` at `t` in [0, 1].
fn seg_point(s: &Seg2, t: f64) -> Vec2 {
    match *s {
        Seg2::Line { a, b } => a.lerp(b, t),
        Seg2::Arc { center, radius, start, sweep } => center + Vec2::from_angle(start + sweep * t) * radius,
        Seg2::Cubic { p0, p1, p2, p3 } => {
            let u = 1.0 - t;
            p0 * (u * u * u) + p1 * (3.0 * u * u * t) + p2 * (3.0 * u * t * t) + p3 * (t * t * t)
        }
        Seg2::Conic { a, apex, b, w } => {
            let u = 1.0 - t;
            let (wa, wm, wb) = (u * u, 2.0 * u * t * w, t * t);
            let den = wa + wm + wb;
            if den.abs() > 1e-300 { (a * wa + apex * wm + b * wb) / den } else { a }
        }
        Seg2::Bezier { n, p } => {
            let k = usize::from(n).min(p.len().saturating_sub(1));
            let mut q: Vec<Vec2> = p.iter().take(k + 1).copied().collect();
            for r in 1..=k {
                for i in 0..=(k - r) {
                    if let (Some(a), Some(b)) = (q.get(i).copied(), q.get(i + 1).copied())
                        && let Some(slot) = q.get_mut(i)
                    {
                        *slot = a.lerp(b, t);
                    }
                }
            }
            q.first().copied().unwrap_or_default()
        }
    }
}

fn loop_points(segs: &[Seg2], tol: f64) -> Vec<Vec2> {
    let mut out = Vec::new();
    for s in segs {
        let n = match *s {
            Seg2::Line { .. } => 1,
            Seg2::Arc { radius, sweep, .. } => {
                let step = if tol < radius { 2.0 * (1.0 - tol / radius).acos() } else { PI / 4.0 };
                ((sweep.abs() / step.max(1e-3)).ceil() as usize).clamp(1, MAX_SAMPLES)
            }
            _ => 32,
        };
        for i in 0..n {
            let p = seg_point(s, i as f64 / n as f64);
            if out.last().is_none_or(|l: &Vec2| l.dist(p) > 1e-12) {
                out.push(p);
            }
        }
    }
    out
}

fn polygon_area(pts: &[Vec2]) -> f64 {
    pts.iter().zip(pts.iter().cycle().skip(1)).map(|(a, b)| a.cross(*b)).sum::<f64>() * 0.5
}

/// The loop turned so that its area is positive (`ccw`) or negative.
fn oriented(segs: &[Seg2], area: f64, ccw: bool) -> Vec<Seg2> {
    if (area > 0.0) == ccw { segs.to_vec() } else { segs.iter().rev().map(reversed).collect() }
}

fn unit2(v: Vec2) -> Option<Vec2> {
    v.normalized()
}

/// The directions `s` starts and ends in.
fn tangents(s: &Seg2) -> (Option<Vec2>, Option<Vec2>) {
    match *s {
        Seg2::Line { a, b } => (unit2(b - a), unit2(b - a)),
        Seg2::Arc { start, sweep, .. } => {
            let d = |t: f64| Vec2::new(-t.sin(), t.cos()) * sweep.signum();
            (Some(d(start)), Some(d(start + sweep)))
        }
        Seg2::Cubic { p0, p1, p2, p3 } => (unit2(p1 - p0).or(unit2(p2 - p0)), unit2(p3 - p2).or(unit2(p3 - p1))),
        Seg2::Conic { a, apex, b, .. } => (unit2(apex - a), unit2(b - apex)),
        Seg2::Bezier { n, p } => {
            let k = usize::from(n).min(p.len().saturating_sub(1));
            let at = |i: usize| p.get(i).copied().unwrap_or_default();
            (unit2(at(1) - at(0)), k.checked_sub(1).and_then(|j| unit2(at(k) - at(j))))
        }
    }
}

/// The inside corners of a loop with the free space on its left: the smallest radius of an arc
/// turning left (towards the free space), and how many sharp left turns it has.
fn corner_limits(segs: &[Seg2]) -> (Option<f64>, usize) {
    let mut radius: Option<f64> = None;
    let mut sharp = 0;
    for (s, next) in segs.iter().zip(segs.iter().cycle().skip(1)) {
        if let Seg2::Arc { radius: r, sweep, .. } = *s
            && sweep > 1e-9
        {
            radius = Some(radius.map_or(r, |m: f64| m.min(r)));
        }
        if let (Some(a), Some(b)) = (tangents(s).1, tangents(next).0) {
            let turn = a.cross(b).atan2(a.dot(b));
            if turn > CORNER {
                sharp += 1;
            }
        }
    }
    (radius, sharp)
}

fn bounds2(pts: &[Vec2]) -> [[f64; 2]; 2] {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for p in pts {
        lo = [lo[0].min(p.x), lo[1].min(p.y)];
        hi = [hi[0].max(p.x), hi[1].max(p.y)];
    }
    if lo[0].is_finite() { [lo, hi] } else { [[0.0; 2]; 2] }
}

/// A number as a reply gives it: rounded to 1e-9, no -0.
fn r9(v: f64) -> f64 {
    let r = (v * 1e9).round() / 1e9;
    if r == 0.0 { 0.0 } else { r }
}

fn r2(p: Vec2) -> [f64; 2] {
    [r9(p.x), r9(p.y)]
}

fn rb(b: [[f64; 2]; 2]) -> [[f64; 2]; 2] {
    [[r9(b[0][0]), r9(b[0][1])], [r9(b[1][0]), r9(b[1][1])]]
}

fn r3(p: Vec3) -> [f64; 3] {
    [r9(p.x), r9(p.y), r9(p.z)]
}

/// The signed area a loop encloses: exact for lines and arcs (each piece's share of
/// ½∮(x dy − y dx)), the other curves by points within `tol` along them.
fn loop_area(segs: &[Seg2], tol: f64) -> f64 {
    let mut twice = 0.0;
    for s in segs {
        match *s {
            Seg2::Line { a, b } => twice += a.cross(b),
            Seg2::Arc { center, radius, start, sweep } => {
                let (s0, c0) = start.sin_cos();
                let (s1, c1) = (start + sweep).sin_cos();
                twice += radius * radius * sweep + radius * (center.x * (s1 - s0) - center.y * (c1 - c0));
            }
            _ => {
                let mut pts = loop_points(std::slice::from_ref(s), tol);
                pts.push(seg_point(s, 1.0));
                twice += pts
                    .windows(2)
                    .map(|w| match w {
                        [a, b] => a.cross(*b),
                        _ => 0.0,
                    })
                    .sum::<f64>();
            }
        }
    }
    twice * 0.5
}

/// The extent of a loop: exact for lines and arcs (an arc's ends and the quarter points it
/// passes), the other curves by points within `tol` along them.
fn loop_bounds(segs: &[Seg2], tol: f64) -> [[f64; 2]; 2] {
    let mut pts: Vec<Vec2> = Vec::new();
    for s in segs {
        match *s {
            Seg2::Line { a, b } => pts.extend([a, b]),
            Seg2::Arc { center, radius, start, sweep } => {
                pts.extend([seg_point(s, 0.0), seg_point(s, 1.0)]);
                let (lo, hi) = if sweep >= 0.0 { (start, start + sweep) } else { (start + sweep, start) };
                let mut k = (lo / (PI * 0.5)).ceil();
                while k * PI * 0.5 <= hi + 1e-12 && pts.len() < 100_000 {
                    pts.push(center + Vec2::from_angle(k * PI * 0.5) * radius);
                    k += 1.0;
                }
            }
            _ => {
                pts.extend(loop_points(std::slice::from_ref(s), tol));
                pts.push(seg_point(s, 1.0));
            }
        }
    }
    bounds2(&pts)
}

/// The outline as a reply gives it: counter-clockwise, its exact area and extent.
fn outline(segs: &[Seg2], tol: f64) -> (Vec<[f64; 2]>, f64, [[f64; 2]; 2]) {
    let mut pts = loop_points(segs, tol);
    let area = loop_area(segs, tol);
    if polygon_area(&pts) < 0.0 {
        pts.reverse();
    }
    let b = loop_bounds(segs, tol);
    (pts.into_iter().map(r2).collect(), r9(area.abs()), rb(b))
}

// ---------------------------------------------------------------------------------------------
// The recognition itself.

/// A round face's cylinder: merged faces of one radius on one axis line.
struct Cyl {
    faces: Vec<usize>,
    origin: Vec3,
    axis: Vec3,
    radius: f64,
}

fn cylinders(t: &Topo<'_>, tol: f64) -> Vec<Cyl> {
    let mut out: Vec<Cyl> = Vec::new();
    for (f, s) in t.surfaces.iter().enumerate() {
        let Some(FaceSurface::Cylinder { origin, axis, radius }) = s else { continue };
        let a = axis.vec();
        let same = |c: &Cyl| {
            let off = (*origin - c.origin) - c.axis * (*origin - c.origin).dot(c.axis);
            (c.radius - radius).abs() <= tol && c.axis.dot(a).abs() > 1.0 - PARALLEL && off.len() <= tol
        };
        match out.iter_mut().find(|c| same(c)) {
            Some(c) => c.faces.push(f),
            None => out.push(Cyl { faces: vec![f], origin: *origin, axis: a, radius: *radius }),
        }
    }
    out
}

/// The circle edges of `c`'s faces that run round its axis, each with its height along `up`.
fn cyl_circles<'t>(t: &'t Topo<'_>, c: &Cyl, up: Vec3, tol: f64) -> Vec<(usize, f64, &'t EdgeCircle)> {
    let mut out: Vec<(usize, f64, &EdgeCircle)> = Vec::new();
    for f in &c.faces {
        for e in t.face_edges(*f) {
            if out.iter().any(|(x, ..)| *x == e) {
                continue;
            }
            let Some(EdgeShape::Circle(k)) = t.shapes.get(e) else { continue };
            let off = (k.center - c.origin) - c.axis * (k.center - c.origin).dot(c.axis);
            if (k.radius - c.radius).abs() <= tol && off.len() <= tol && k.axis.vec().dot(c.axis).abs() > 1.0 - PARALLEL {
                out.push((e, (k.center - c.origin).dot(up), k));
            }
        }
    }
    out
}

/// For a cylinder square to X on a 4th-axis part, entered from the turned surface (an edge of
/// its faces runs along a surface of revolution about X) with material just outside its wall
/// halfway along it: a hole drilled across the bar. Its extent along `up` from its origin.
fn cross_hole(t: &Topo<'_>, c: &Cyl, up: Vec3, tol: f64) -> Option<(f64, f64)> {
    if c.axis.x.abs() > SQUARE {
        return None;
    }
    let about_x = |a: Vec3| a.x.abs() > 1.0 - PARALLEL;
    let entered = c.faces.iter().flat_map(|f| t.face_edges(*f)).any(|e| {
        t.edge_faces.get(e).into_iter().flatten().any(|g| {
            !c.faces.contains(g)
                && matches!(t.surface(*g), Some(FaceSurface::Cylinder { axis, .. } | FaceSurface::Cone { axis, .. }) if about_x(axis.vec()))
        })
    });
    if !entered {
        return None;
    }
    let base = c.origin.dot(up);
    let (lo, hi) =
        c.faces.iter().flat_map(|f| t.face_edges(*f)).filter_map(|e| t.edge_range(e, up)).fold(None, |acc, (a, b)| Some(widen(acc, a, b)))?;
    let (lo, hi) = (lo - base, hi - base);
    if !(hi - lo > tol) {
        return None;
    }
    // Material just outside the wall halfway along it, all the way round: a hole's wall closes
    // round its axis, where a pocket's rounded end or corner (a part of a cylinder) has the
    // pocket's air on one side.
    let side = up.cross(Vec3::X).normalized()?;
    let other = up.cross(side).normalized()?;
    let mid = c.origin + up * ((lo + hi) * 0.5);
    let eps = (tol * 2.0).max(c.radius * 1e-3);
    let round = (0..CROSS_HOLE_SAMPLES).all(|k| {
        let a = std::f64::consts::TAU * k as f64 / CROSS_HOLE_SAMPLES as f64;
        let p = mid + (side * a.cos() + other * a.sin()) * (c.radius + eps) + t.origin;
        matches!(t.body.classify_point(p), Ok(PointClass::Inside))
    });
    round.then_some((lo, hi))
}

/// For a cylinder whose two ends are curves where it meets curved faces (a hole straight through
/// a domed or saddled part, its mouths on spheres or free-form faces), read from the exact B-rep:
/// its faces close round the axis (no straight edge along it borders another face, as a pocket's
/// rounded corner's do), no mouth is on a cylinder or cone, material is just outside its wall all
/// the way round halfway along it, and the axis just past each end is in air. Its extent along `up` from its origin.
fn curved_through(t: &Topo<'_>, c: &Cyl, up: Vec3, tol: f64) -> Option<(f64, f64)> {
    let edges: Vec<usize> = c.faces.iter().flat_map(|f| t.face_edges(*f)).collect();
    let partial = edges.iter().any(|e| {
        let along = match t.shapes.get(*e) {
            Some(EdgeShape::Hull(pts)) if pts.len() == 2 => (pts[1] - pts[0]).normalized().is_some_and(|d| d.dot(c.axis).abs() > 1.0 - PARALLEL),
            _ => false,
        };
        along && t.edge_faces.get(*e).into_iter().flatten().any(|g| !c.faces.contains(g))
    });
    // A mouth on a turned surface (a hole across a round bar) is the 4th axis's to find
    // ([`cross_hole`]); here the mouths are on spheres, tori or free-form faces.
    let turned = edges
        .iter()
        .flat_map(|e| t.edge_faces.get(*e).into_iter().flatten())
        .any(|g| !c.faces.contains(g) && matches!(t.surface(*g), Some(FaceSurface::Cylinder { .. } | FaceSurface::Cone { .. })));
    if partial || turned {
        return None;
    }
    let base = c.origin.dot(up);
    let (lo, hi) = edges.iter().filter_map(|e| t.edge_range(*e, up)).fold(None, |acc, (a, b)| Some(widen(acc, a, b)))?;
    let (lo, hi) = (lo - base, hi - base);
    if !(hi - lo > tol) {
        return None;
    }
    let across = if up.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    let side = up.cross(across).normalized()?;
    let other = up.cross(side).normalized()?;
    let eps = (tol * 2.0).max(c.radius * 1e-3);
    let air = |h: f64| matches!(t.body.classify_point(c.origin + up * h + t.origin), Ok(PointClass::Outside));
    if !air(lo - eps) || !air(hi + eps) {
        return None;
    }
    let mid = c.origin + up * ((lo + hi) * 0.5);
    let round = (0..CROSS_HOLE_SAMPLES).all(|k| {
        let a = std::f64::consts::TAU * k as f64 / CROSS_HOLE_SAMPLES as f64;
        let p = mid + (side * a.cos() + other * a.sin()) * (c.radius + eps) + t.origin;
        matches!(t.body.classify_point(p), Ok(PointClass::Inside))
    });
    round.then_some((lo, hi))
}

/// For a cylinder whose top end (along `up`) is a whole circle and whose wall runs on below its
/// lowest circle, ending on a curve where it meets another face (a hole down through a milled
/// flat that comes out on a bar's turned surface): its bottom along `up` from its origin, the
/// lowest point of its wall, when material is just outside the wall all the way round halfway
/// down it (a hole, not a boss).
fn down_past(t: &Topo<'_>, c: &Cyl, circles: &[(usize, f64, &EdgeCircle)], up: Vec3, tol: f64) -> Option<f64> {
    let (lo, hi) = (circles.iter().map(|x| x.1).reduce(f64::min)?, circles.iter().map(|x| x.1).reduce(f64::max)?);
    if !full_at(circles, hi, tol) {
        return None;
    }
    let base = c.origin.dot(up);
    let bottom = c.faces.iter().flat_map(|f| t.face_edges(*f)).filter_map(|e| t.edge_range(e, up)).map(|r| r.0).reduce(f64::min)? - base;
    if !(bottom < lo - tol) {
        return None;
    }
    let across = if up.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    let side = up.cross(across).normalized()?;
    let other = up.cross(side).normalized()?;
    let mid = c.origin + up * ((bottom + hi) * 0.5);
    let eps = (tol * 2.0).max(c.radius * 1e-3);
    let round = (0..CROSS_HOLE_SAMPLES).all(|k| {
        let a = std::f64::consts::TAU * k as f64 / CROSS_HOLE_SAMPLES as f64;
        let p = mid + (side * a.cos() + other * a.sin()) * (c.radius + eps) + t.origin;
        matches!(t.body.classify_point(p), Ok(PointClass::Inside))
    });
    round.then_some(bottom)
}

/// The angle round the A axis of a point in work coordinates: 0 at +Z, positive towards +Y,
/// as [`solidcraft_cam::rotary`] has it.
fn a_angle(p: Vec3) -> f64 {
    p.y.atan2(p.z)
}

/// The pockets wrapped round the A axis on body `bi` ([`WrappedReport`]): a face on a cylinder
/// about work X through work zero that covers part of a turn, with the void above it, material
/// under it and material round the rest of the turn at its radius, its plane walls radial or
/// square to X. Read from the exact faces: the floor's boundary (its vertices, and points along
/// curves), and points classified against the body.
fn wrapped_pockets(t: &Topo<'_>, bi: usize, tol: f64, out: &mut Vec<(Vec<i64>, WrappedReport, usize)>) {
    for f in 0..t.record.faces.len() {
        let Some(FaceSurface::Cylinder { origin, axis, radius }) = t.surface(f) else { continue };
        let off = *origin - Vec3::X * origin.x;
        if axis.vec().x.abs() < 1.0 - PARALLEL || off.len() > tol || *radius <= tol {
            continue;
        }
        let rf = *radius;
        // One loop only: a floor with islands is left to the other features.
        let Some([lp]) = t.loops.get(f).map(Vec::as_slice) else { continue };
        let vertex = |v: u32| t.record.vertices.get(v as usize).map(|v| p3(v.point) - t.origin);
        let mut pts: Vec<Vec3> = Vec::new();
        for (e, forward) in lp {
            let Some(edge) = t.record.edges.get(*e) else { continue };
            if let Some(p) = vertex(if *forward { edge.vertices[0] } else { edge.vertices[1] }) {
                pts.push(p);
            }
            if let Some(EdgeShape::Sampled(sm)) = t.shapes.get(*e)
                && sm.pts.len() > 2
            {
                let inner = &sm.pts[1..sm.pts.len() - 1];
                if *forward {
                    pts.extend_from_slice(inner);
                } else {
                    pts.extend(inner.iter().rev());
                }
            }
        }
        if pts.len() < 3 {
            continue;
        }
        // The walls: every face next to the floor; planes must be radial or square to X.
        let mut walls: Vec<usize> = Vec::new();
        let mut plain = true;
        for e in t.face_edges(f) {
            for g in t.edge_faces.get(e).into_iter().flatten() {
                if *g == f || walls.contains(g) {
                    continue;
                }
                match t.surface(*g) {
                    Some(FaceSurface::Plane { normal, point }) => {
                        let n = normal.vec();
                        let radial = n.x.abs() <= SQUARE && point.dot(n).abs() <= tol;
                        plain &= radial || n.x.abs() >= 1.0 - PARALLEL;
                    }
                    // Another diameter about X: a step or a groove, turned.
                    Some(FaceSurface::Cylinder { axis, .. }) if axis.vec().x.abs() >= 1.0 - PARALLEL => plain = false,
                    _ => {}
                }
                walls.push(*g);
            }
        }
        if !plain || walls.is_empty() {
            continue;
        }
        // The part's surface the pocket is cut into: the walls reach out to it.
        let outer = walls
            .iter()
            .flat_map(|g| t.face_edges(*g))
            .filter_map(|e| t.record.edges.get(e))
            .flat_map(|e| e.vertices)
            .filter_map(vertex)
            .map(|p| p.y.hypot(p.z))
            .fold(0.0f64, f64::max);
        if outer <= rf + tol {
            continue;
        }
        // The turn the floor leaves out: the widest gap between its points' angles.
        let mut angles: Vec<f64> = pts.iter().map(|p| a_angle(*p)).collect();
        angles.sort_by(f64::total_cmp);
        let Some(first) = angles.first().copied() else { continue };
        let Some((gap_start, gap)) =
            angles.iter().enumerate().map(|(i, a)| (*a, angles.get(i + 1).copied().unwrap_or(first + TAU) - a)).max_by(|a, b| a.1.total_cmp(&b.1))
        else {
            continue;
        };
        let span = TAU - gap;
        if gap <= tol / rf || span <= tol / rf {
            continue;
        }
        // Its start, within a half turn of A 0.
        let a0 = (gap_start + gap + PI).rem_euclid(TAU) - PI;
        let (x_min, x_max) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.x), hi.max(p.x)));
        let xm = (x_min + x_max) * 0.5;
        let eps = (tol * 2.0).max(rf * 1e-3);
        let class = |a: f64, r: f64| t.body.classify_point(Vec3::new(xm, r * a.sin(), r * a.cos()) + t.origin).ok();
        // Material round the rest of the turn, the void over the floor and material under it.
        if class(gap_start + gap * 0.5, rf + eps) != Some(PointClass::Inside)
            || class(a0 + span * 0.5, rf + eps) != Some(PointClass::Outside)
            || class(a0 + span * 0.5, rf - eps) != Some(PointClass::Inside)
        {
            continue;
        }
        // Unrolled at the outer radius, angles from the start of the pocket's turn.
        let unroll = |p: &Vec3| {
            let mut d = (a_angle(*p) - a0).rem_euclid(TAU);
            if d > span + gap * 0.5 {
                d -= TAU;
            }
            Vec2::new(p.x, outer * (a0 + d))
        };
        let mut outline: Vec<Vec2> = pts.iter().map(unroll).collect();
        if polygon_area(&outline) < 0.0 {
            outline.reverse();
        }
        walls.sort_unstable();
        let report = WrappedReport {
            id: String::new(),
            body: t.name.clone(),
            floor_face: f,
            walls,
            radius: r9(outer),
            floor_radius: r9(rf),
            depth: r9(outer - rf),
            x_min: r9(x_min),
            x_max: r9(x_max),
            a_min_deg: r9(a0.to_degrees()),
            a_max_deg: r9((a0 + span).to_degrees()),
            span_deg: r9(span.to_degrees()),
            outline: outline.into_iter().map(r2).collect(),
        };
        out.push((vec![q(x_min, tol), q(a0, tol), bi as i64, f as i64], report, bi));
    }
}

/// Points round a hole across a turned part, halfway along it, that must all be in material.
const CROSS_HOLE_SAMPLES: usize = 24;

/// Points across a flat face tested for material over them (a grid of this many a side).
const COVER_GRID: usize = 5;
/// Points up a column over a flat face classified against a curved face that may hang over it.
const COVER_COLUMN: usize = 64;

/// Whether the part has material over the whole of flat face `f` (its loops `loops` read at
/// height `z` in `fr`, `oi` the outer one) along `fr`'s Z: a tool coming down that axis cannot
/// reach it (the end wall of a pocket cut in a bar's side, under the bar). Read from the exact
/// solid at points across the face: material above a point starts at a face that faces down
/// over it, a flat one tested exactly (the point inside its outline), a curved one by
/// classifying points up the column between them.
fn covered(t: &Topo<'_>, fr: &Frame, f: usize, loops: &[LoopGeo], oi: usize, z: f64, tol: f64) -> bool {
    let lz = |n: Vec3| fr.dir_to_local(n).z;
    let local = |d: Vec3, r: (f64, f64)| (r.0 - fr.origin.dot(d), r.1 - fr.origin.dot(d));
    let Some(outer) = loops.get(oi) else { return false };
    let poly = loop_points(&outer.segs, tol);
    let holes: Vec<Vec<Vec2>> = loops.iter().enumerate().filter(|(i, _)| *i != oi).map(|(_, l)| loop_points(&l.segs, tol)).collect();
    if poly.len() < 3 {
        return false;
    }
    let b = bounds2(&poly);
    let eps = (tol * 4.0).max(1e-3);
    let mut pts: Vec<Vec2> = Vec::new();
    for i in 0..COVER_GRID {
        for j in 0..COVER_GRID {
            let u = (i as f64 + 0.5) / COVER_GRID as f64;
            let v = (j as f64 + 0.5) / COVER_GRID as f64;
            let q = Vec2::new(b[0][0] + (b[1][0] - b[0][0]) * u, b[0][1] + (b[1][1] - b[0][1]) * v);
            let clear_of = |p: &[Vec2]| boundary_dist(p, q) > eps;
            if inside_polygon(&poly, q) && clear_of(&poly) && holes.iter().all(|h| !inside_polygon(h, q) && clear_of(h)) {
                pts.push(q);
            }
        }
    }
    if pts.is_empty() {
        return false;
    }
    // The faces material over the face could start at: those facing down, at least in part,
    // reaching above it.
    let above: Vec<usize> = (0..t.record.faces.len())
        .filter(|g| *g != f)
        .filter(|g| match t.surface(*g) {
            Some(FaceSurface::Plane { normal, .. }) => lz(normal.vec()) < -SQUARE,
            Some(FaceSurface::Cylinder { axis, .. }) => lz(axis.vec()).abs() <= 1.0 - PARALLEL,
            _ => true,
        })
        .filter(|g| t.face_range(*g, fr.z, true).is_some_and(|r| local(fr.z, r).1 > z + tol))
        .collect();
    if above.is_empty() {
        return false;
    }
    // Flat faces facing straight down: their outlines, read once.
    let flats: Vec<(usize, Vec<Vec2>, Vec<Vec<Vec2>>)> = above
        .iter()
        .filter_map(|g| match t.surface(*g) {
            Some(FaceSurface::Plane { normal, point }) if lz(normal.vec()) < -1.0 + PARALLEL => {
                let zg = fr.to_local(*point).z;
                let ls = t.face_loops(*g, fr, zg, tol);
                let oi = ls.iter().enumerate().max_by(|a, b| a.1.area.abs().total_cmp(&b.1.area.abs())).map(|(i, _)| i)?;
                let outer = loop_points(&ls.get(oi)?.segs, tol);
                let inner = ls.iter().enumerate().filter(|(i, _)| *i != oi).map(|(_, l)| loop_points(&l.segs, tol)).collect();
                Some((*g, outer, inner))
            }
            _ => None,
        })
        .collect();
    // The others (curved, or flat and slanting down): their extents, read once.
    let others: Vec<[f64; 5]> = above
        .iter()
        .filter(|g| !flats.iter().any(|x| x.0 == **g))
        .filter_map(|g| {
            let (xr, yr, zr) = (t.face_range(*g, fr.x, true)?, t.face_range(*g, fr.y, true)?, t.face_range(*g, fr.z, true)?);
            let ((x0, x1), (y0, y1), (_, top)) = (local(fr.x, xr), local(fr.y, yr), local(fr.z, zr));
            Some([x0, x1, y0, y1, top])
        })
        .collect();
    let over = |q: Vec2| -> bool {
        if flats.iter().any(|(_, outer, inner)| inside_polygon(outer, q) && !inner.iter().any(|h| inside_polygon(h, q))) {
            return true;
        }
        // One column up to the highest of the others over the point: material anywhere in it
        // is over the face there.
        let top = others
            .iter()
            .filter(|b| q.x >= b[0] - tol && q.x <= b[1] + tol && q.y >= b[2] - tol && q.y <= b[3] + tol)
            .map(|b| b[4])
            .fold(f64::NEG_INFINITY, f64::max);
        if !(top > z + eps) {
            return false;
        }
        (1..=COVER_COLUMN).any(|k| {
            let h = z + eps + (top - z - eps) * k as f64 / COVER_COLUMN as f64;
            matches!(t.body.classify_point(fr.to_world(Vec3::new(q.x, q.y, h)) + t.origin), Ok(PointClass::Inside))
        })
    };
    pts.iter().all(|q| over(*q))
}

/// Points across a face read for [`undercut_faces`], along each side.
const UNDERCUT_GRID: usize = 8;

/// The faces of `faces` a tool coming down `fr`'s Z cannot see all of (undercuts), read on the
/// exact body: a flat face facing down with material under it (an overhanging wall: the tool
/// reaches it neither from above nor turned over), and a flat face facing up with a face facing
/// down over some of it (the part overhangs it there). Curved faces are not read.
fn undercut_faces(t: &Topo<'_>, fr: &Frame, faces: &[usize], bottom: f64, tol: f64) -> Vec<usize> {
    let lz = |n: Vec3| fr.dir_to_local(n).z;
    let eps = (tol * 4.0).max(1e-3);
    // A flat face's outline seen along Z and its height at a point of it.
    let shape = |g: usize| -> Option<(Vec<Vec2>, Vec3, Vec3)> {
        let Some(FaceSurface::Plane { normal, point }) = t.surface(g) else { return None };
        let (n, p) = (fr.dir_to_local(normal.vec()), fr.to_local(*point));
        let ls = t.face_loops(g, fr, p.z, tol);
        let oi = ls.iter().enumerate().max_by(|a, b| a.1.area.abs().total_cmp(&b.1.area.abs())).map(|(i, _)| i)?;
        let outer = loop_points(&ls.get(oi)?.segs, tol);
        (outer.len() >= 3).then_some((outer, n, p))
    };
    let z_at = |n: Vec3, p: Vec3, q: Vec2| p.z - (n.x * (q.x - p.x) + n.y * (q.y - p.y)) / n.z;
    let samples = |poly: &[Vec2]| -> Vec<Vec2> {
        let b = bounds2(poly);
        let mut out = Vec::new();
        for i in 0..UNDERCUT_GRID {
            for j in 0..UNDERCUT_GRID {
                let u = (i as f64 + 0.5) / UNDERCUT_GRID as f64;
                let v = (j as f64 + 0.5) / UNDERCUT_GRID as f64;
                let q = Vec2::new(b[0][0] + (b[1][0] - b[0][0]) * u, b[0][1] + (b[1][1] - b[0][1]) * v);
                if inside_polygon(poly, q) && boundary_dist(poly, q) > eps {
                    out.push(q);
                }
            }
        }
        out
    };
    let down: Vec<(usize, Vec<Vec2>, Vec3, Vec3)> = (0..t.record.faces.len())
        .filter(|g| matches!(t.surface(*g), Some(FaceSurface::Plane { normal, .. }) if lz(normal.vec()) < -SQUARE))
        .filter_map(|g| shape(g).map(|(o, n, p)| (g, o, n, p)))
        .collect();
    let mut out = Vec::new();
    for &f in faces {
        if cancelled() {
            break;
        }
        let Some(FaceSurface::Plane { normal, .. }) = t.surface(f) else { continue };
        let up = lz(normal.vec());
        if up < -SQUARE {
            let Some((_, poly, n, p)) = down.iter().find(|d| d.0 == f) else { continue };
            let under = samples(poly).into_iter().any(|q| {
                let z = z_at(*n, *p, q) - eps;
                (1..=COVER_COLUMN).any(|k| {
                    let h = z - (z - bottom) * k as f64 / COVER_COLUMN as f64;
                    matches!(t.body.classify_point(fr.to_world(Vec3::new(q.x, q.y, h)) + t.origin), Ok(PointClass::Inside))
                })
            });
            if under {
                out.push(f);
            }
        } else if up > SQUARE {
            let Some((poly, n, p)) = shape(f) else { continue };
            let over = samples(&poly).into_iter().any(|q| {
                let z = z_at(n, p, q);
                down.iter().any(|(g, o, dn, dp)| *g != f && inside_polygon(o, q) && boundary_dist(o, q) > eps && z_at(*dn, *dp, q) > z + eps)
            });
            if over {
                out.push(f);
            }
        }
    }
    out
}

/// Points round a cone face, at the middle of its height, tested for which side the material is.
const CONE_SAMPLES: usize = 16;

/// Whether cone face `f`, its axis along `fr`'s Z, faces down: read from the exact solid at
/// points round it at the middle of its height, material just above and none just below. Points
/// of the cone off the face (material on neither side, or both) say nothing; one facing up, or
/// none read, and it does not face down.
fn cone_faces_down(t: &Topo<'_>, fr: &Frame, f: usize, tol: f64) -> bool {
    let Some(FaceSurface::Cone { apex, axis, half_angle }) = t.surface(f) else { return false };
    let a = fr.dir_to_local(axis.vec());
    if !(a.z.abs() > 1.0 - PARALLEL) || !half_angle.is_finite() {
        return false;
    }
    let Some((lo, hi)) = t.face_range(f, fr.z, true) else { return false };
    let o = fr.origin.dot(fr.z);
    let mid = 0.5 * (lo + hi) - o;
    let c = fr.to_local(*apex);
    let along = (mid - c.z) / a.z;
    let r = along * half_angle.tan();
    if !(along > tol && r > tol && r.is_finite()) {
        return false;
    }
    let eps = (tol * 4.0).max(1e-3);
    let inside = |x: f64, y: f64, z: f64| matches!(t.body.classify_point(fr.to_world(Vec3::new(x, y, z)) + t.origin), Ok(PointClass::Inside));
    let (mut down, mut up) = (0, 0);
    for k in 0..CONE_SAMPLES {
        let th = std::f64::consts::TAU * k as f64 / CONE_SAMPLES as f64;
        let (x, y) = (c.x + r * th.cos(), c.y + r * th.sin());
        match (inside(x, y, mid + eps), inside(x, y, mid - eps)) {
            (true, false) => down += 1,
            (false, true) => up += 1,
            _ => {}
        }
    }
    down > 0 && up == 0
}

/// Whether the cylinder is a hole (material outside it): read from the planar faces at its
/// ends, else by classifying a point just outside it. Only the ends where the wall closes all the
/// way round are read: where a hole breaks into another feature (a slot across its foot) its
/// edge there is part of the outer loop of the face it breaks out of, as a boss's would be.
fn concave(t: &Topo<'_>, c: &Cyl, circles: &[(usize, f64, &EdgeCircle)], up: Vec3, tol: f64) -> Option<bool> {
    let (Some(lo), Some(hi)) = (circles.iter().map(|x| x.1).reduce(f64::min), circles.iter().map(|x| x.1).reduce(f64::max)) else {
        return None;
    };
    for (e, h, _) in circles {
        // From this end the wall runs towards the other.
        let into = if (h - hi).abs() <= tol { -up } else { up };
        if (hi - lo).abs() <= tol || !full_at(circles, *h, tol) {
            continue;
        }
        for g in t.edge_faces.get(*e).into_iter().flatten() {
            if c.faces.contains(g) {
                continue;
            }
            let Some(FaceSurface::Plane { normal, .. }) = t.surface(*g) else { continue };
            let n = normal.vec();
            if n.dot(c.axis).abs() < 1.0 - PARALLEL {
                continue;
            }
            // Which loop of the plane holds this edge: the largest is its outer loop.
            let Some(loops) = t.loops.get(*g) else { continue };
            let Ok(frame) = Frame::from_z(Vec3::ZERO, n) else { continue };
            let z = match t.surface(*g) {
                Some(FaceSurface::Plane { point, .. }) => point.dot(n),
                _ => continue,
            };
            let geo = t.face_loops(*g, &frame, z, tol);
            let Some(outer) = geo.iter().enumerate().max_by(|a, b| a.1.area.abs().total_cmp(&b.1.area.abs())).map(|(i, _)| i) else { continue };
            let Some(at) = loops.iter().position(|l| l.iter().any(|(x, _)| x == e)) else { continue };
            let inner = at != outer;
            return Some(if inner { into.dot(n) < 0.0 } else { into.dot(n) > 0.0 });
        }
    }
    // No planar end: a point just outside the wall, halfway along it.
    let (_, h0, k) = circles.first()?;
    let mid = (lo + hi) * 0.5;
    let p = k.start + up * (mid - h0);
    let radial = ((p - c.origin) - c.axis * (p - c.origin).dot(c.axis)).normalized()?;
    let eps = (tol * 2.0).max(c.radius * 1e-3);
    match t.body.classify_point(p + radial * eps + t.origin).ok()? {
        PointClass::Inside => Some(true),
        PointClass::Outside => Some(false),
        PointClass::OnBoundary => None,
    }
}

/// A face as one setup sees it.
#[derive(Clone)]
struct Seen {
    body: usize,
    face: usize,
    setup: usize,
}

struct Found {
    reply: CamFeaturesReply,
    geo: Vec<(String, Geo)>,
}

#[derive(Default)]
struct Lists {
    holes: Vec<(Vec<i64>, HoleReport, Geo)>,
    pockets: Vec<(Vec<i64>, PocketReport, Geo, usize)>,
    faces: Vec<(Vec<i64>, FaceReport, Geo, Seen)>,
    profiles: Vec<(Vec<i64>, ProfileReport, Geo, Seen)>,
    fillets: Vec<(Vec<i64>, EdgeBreakReport, Geo)>,
    chamfers: Vec<(Vec<i64>, EdgeBreakReport, Geo)>,
    surfaces: Vec<(Vec<i64>, SurfaceReport, Geo)>,
    turned: Vec<(Vec<i64>, TurnedReport)>,
    /// Wrapped pockets, each with its body.
    wrapped: Vec<(Vec<i64>, WrappedReport, usize)>,
    /// Counterbore pairs: (larger hole, smaller hole) as indices into `holes`.
    counterbores: Vec<(usize, usize)>,
}

/// Every feature of the bodies `names`, along `axis`, on `machine`.
pub(crate) fn recognize(cmd: &str, s: &Session, names: &[String], origin: Vec3, axis: AxisSpec, tol: f64, machine: &Machine) -> Result<FeatureSet> {
    let st = s.world_state()?;
    recognize_in(cmd, &st, names, origin, axis, tol, machine)
}

/// [`recognize`] on the bodies of `st`.
pub(crate) fn recognize_in(
    cmd: &str,
    st: &solidcraft_doc::ModelState,
    names: &[String],
    origin: Vec3,
    axis: AxisSpec,
    tol: f64,
    machine: &Machine,
) -> Result<FeatureSet> {
    let mut bodies: Vec<&solidcraft_doc::ModelBody> = Vec::with_capacity(names.len());
    let mut notes: Vec<String> = Vec::new();
    for n in names {
        let Some(b) = st.bodies.iter().find(|b| &b.name == n) else { return Err(bad(cmd, format!("no body `{n}`"))) };
        if b.body.is_mesh() {
            notes.push(format!("body `{n}` is a mesh: it has no exact faces, so no features are found on it"));
            continue;
        }
        if b.body.face_count() > MAX_FACES {
            return Err(bad(cmd, format!("body `{n}` has more than {MAX_FACES} faces")));
        }
        bodies.push(b);
    }
    let topos: Vec<Topo<'_>> = bodies.iter().map(|b| Topo::new(&b.name, &b.body, origin)).collect();

    // The setups: +Z, and with a 4th axis every direction square to X a face or hole faces.
    let mut dirs: Vec<Vec3> = vec![Vec3::Z];
    if axis == AxisSpec::A {
        for t in &topos {
            for s in t.surfaces.iter().flatten() {
                match s {
                    FaceSurface::Plane { normal, .. } if normal.vec().x.abs() <= SQUARE => dirs.push(normal.vec()),
                    FaceSurface::Cylinder { axis, .. } if axis.vec().x.abs() <= SQUARE => {
                        dirs.push(axis.vec());
                        dirs.push(-axis.vec());
                    }
                    _ => {}
                }
            }
        }
    }
    let mut setups: Vec<Setup> = Vec::new();
    let mut unreachable = 0usize;
    for d in dirs {
        let Some(d) = d.normalized() else { continue };
        if setups.iter().any(|s| s.frame.z.dot(d) > 1.0 - PARALLEL) {
            continue;
        }
        let Ok(rotary) = index_for_axis(machine, d) else {
            unreachable += 1;
            continue;
        };
        let Ok(frame) = frame_for_index(machine, &rotary) else { continue };
        if frame.z.dot(d) < 1.0 - 1e-6 {
            continue;
        }
        let a_deg = rotary.first().map_or(0.0, |a| r9(a.to_degrees()));
        if setups.len() >= MAX_SETUPS {
            notes.push(format!("more than {MAX_SETUPS} sides face square to X: only the first {MAX_SETUPS} are looked at"));
            break;
        }
        setups.push(Setup { a_deg, frame, top: 0.0, bottom: 0.0, swing: None });
    }
    if unreachable > 0 {
        notes.push(format!("{unreachable} directions faces look along cannot be reached by the machine's rotary axis"));
    }
    // A = 0 first, then going round.
    setups.sort_by(|a, b| {
        let key = |s: &Setup| (s.a_deg.rem_euclid(360.0) > 1e-9, s.a_deg.rem_euclid(360.0));
        key(a).partial_cmp(&key(b)).unwrap_or(std::cmp::Ordering::Equal)
    });

    // Each setup's extent.
    let mut setup_bounds: Vec<[[f64; 2]; 2]> = Vec::with_capacity(setups.len());
    for su in &mut setups {
        let (mut zr, mut xr, mut yr) = (None, None, None);
        let mut swing: f64 = 0.0;
        for t in &topos {
            for f in 0..t.record.faces.len() {
                let o = su.frame.origin;
                if let Some((lo, hi)) = t.face_range(f, su.frame.z, true) {
                    zr = Some(widen(zr, lo - o.dot(su.frame.z), hi - o.dot(su.frame.z)));
                }
                if let Some((lo, hi)) = t.face_range(f, su.frame.x, true) {
                    xr = Some(widen(xr, lo - o.dot(su.frame.x), hi - o.dot(su.frame.x)));
                }
                if let Some((lo, hi)) = t.face_range(f, su.frame.y, true) {
                    yr = Some(widen(yr, lo - o.dot(su.frame.y), hi - o.dot(su.frame.y)));
                }
                if axis == AxisSpec::A {
                    swing = swing.max(t.face_swing(f));
                }
            }
        }
        let (zlo, zhi) = zr.unwrap_or((0.0, 0.0));
        su.top = r9(zhi);
        su.bottom = r9(zlo);
        if axis == AxisSpec::A {
            su.swing = Some(r9(swing));
        }
        let ((xl, xh), (yl, yh)) = (xr.unwrap_or_default(), yr.unwrap_or_default());
        setup_bounds.push([[xl, yl], [xh, yh]]);
    }

    let found = find(&topos, &setups, axis, tol, &mut notes);
    if cancelled() {
        return Err(bad(cmd, "cancelled"));
    }
    // Whole holes square to every setup's Z: a 3-axis mill does not reach them from above.
    let side_holes: usize = topos
        .iter()
        .map(|t| {
            cylinders(t, tol)
                .iter()
                .filter(|c| setups.iter().all(|su| su.frame.dir_to_local(c.axis).z.abs() <= SQUARE))
                .filter(|c| {
                    let circles = cyl_circles(t, c, c.axis, tol);
                    full(&circles, tol) && concave(t, c, &circles, c.axis, tol) == Some(true)
                })
                .count()
        })
        .sum();
    if side_holes > 0 {
        notes.push(format!("{side_holes} holes run square to Z, not reachable on a 3-axis mill from above: another setup reaches them"));
    }
    if topos.iter().any(|t| t.trim_checks.load(Ordering::Relaxed) > MAX_TRIM_CHECKS) {
        notes.push(format!(
            "free-form faces took more than {MAX_TRIM_CHECKS} points read against the part, the most: the rest are taken at their surfaces' whole extent, so some heights and sizes may read larger than the part"
        ));
    }
    let mut reply = found.reply;
    tapped(&mut reply.holes, &st.threads, origin, tol);
    reply.axis = axis;
    reply.origin = r3(origin);
    reply.bodies = names.to_vec();
    reply.tolerance = tol;
    reply.notes.splice(0..0, notes);
    reply.setups = setups
        .iter()
        .zip(&setup_bounds)
        .enumerate()
        .map(|(i, (su, b))| SetupReport {
            a_deg: su.a_deg,
            tool_axis: r3(su.frame.z),
            top_z: su.top,
            bottom_z: su.bottom,
            bounds: rb(*b),
            swing_radius: su.swing,
            features: found.geo.iter().filter(|(_, g)| g.setup == i).map(|(id, _)| id.clone()).collect(),
        })
        .collect();
    Ok(FeatureSet { reply, setups, geo: found.geo.into_iter().collect() })
}

/// The threads the design taps in `holes` (a hole made with a thread is its tap drill): each
/// internal thread of the hole's body on the hole's own axis, its round wall the hole's, gives
/// the hole its designation and the threaded length in the setup's Z (within the wall).
fn tapped(holes: &mut [HoleReport], threads: &[solidcraft_doc::ThreadInfo], origin: Vec3, tol: f64) {
    for h in holes.iter_mut() {
        let top = Vec3::new(h.center_work[0], h.center_work[1], h.center_work[2]) + origin;
        let up = Vec3::new(h.axis_work[0], h.axis_work[1], h.axis_work[2]);
        let on_axis = |t: &&solidcraft_doc::ThreadInfo| {
            let a = t.axis.vec();
            let off = top - t.axis_point;
            t.internal
                && t.body == h.body
                && a.dot(up).abs() > 1.0 - PARALLEL
                && (off - a * off.dot(a)).len() <= tol
                && (t.radius - h.diameter * 0.5).abs() <= tol
        };
        let Some(t) = threads.iter().find(on_axis) else { continue };
        let a = t.axis.vec();
        let z = |s: f64| h.top_z + (t.axis_point + a * s - top).dot(up);
        let (za, zb) = (z(t.start), z(t.end));
        let hi = za.max(zb).min(h.top_z);
        let lo = za.min(zb).max(h.bottom_z);
        if !(hi - lo > tol) {
            continue;
        }
        let name = |v: serde_json::Result<Value>| v.ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
        h.thread = Some(HoleThread {
            designation: t.designation.clone(),
            form: name(serde_json::to_value(t.form)),
            major_diameter: r9(t.major_diameter),
            pitch: r9(t.pitch),
            starts: t.starts,
            hand: name(serde_json::to_value(t.hand)),
            top_z: r9(hi),
            bottom_z: r9(lo),
            depth: r9(hi - lo),
        });
    }
}

/// A quantised coordinate for ordering features.
fn q(v: f64, tol: f64) -> i64 {
    let k = (v / tol).round();
    if k.is_finite() { k.clamp(-9e15, 9e15) as i64 } else { 0 }
}

fn find(topos: &[Topo<'_>], setups: &[Setup], axis: AxisSpec, tol: f64, notes: &mut Vec<String>) -> Found {
    let mut l = Lists::default();
    let a_of = |s: usize| if axis == AxisSpec::A { setups.get(s).map(|x| x.a_deg) } else { None };
    // Faces claimed by a feature (body, face), so a later setup does not list them again.
    let mut claimed: BTreeSet<(usize, usize)> = BTreeSet::new();
    let mut side_holes = 0usize;
    // Bores on the A axis: (diameter, x_min, x_max, chamfers at its ends, the most its faces
    // stand off the axis).
    let mut a_bores: Vec<(f64, f64, f64, usize, f64)> = Vec::new();
    for (bi, t) in topos.iter().enumerate() {
        let cyls = cylinders(t, tol);
        let mut cyl_done: BTreeSet<usize> = BTreeSet::new();
        for (si, su) in setups.iter().enumerate() {
            let fr = &su.frame;
            let z_of = |p: Vec3| fr.to_local(p).z;
            let lz = |n: Vec3| fr.dir_to_local(n).z;
            let zrange = |f: usize| t.face_range(f, fr.z, false).map(|(lo, hi)| (lo - fr.origin.dot(fr.z), hi - fr.origin.dot(fr.z)));
            let vertical = |g: usize| match t.surface(g) {
                Some(FaceSurface::Plane { normal, .. }) => lz(normal.vec()).abs() <= SQUARE,
                Some(FaceSurface::Cylinder { axis, .. }) => lz(axis.vec()).abs() > 1.0 - PARALLEL,
                _ => false,
            };
            let lie = |lp: &LoopGeo, z: f64| -> Lie {
                let mut up = false;
                let mut down = false;
                for g in &lp.beside {
                    match zrange(*g) {
                        Some((lo, hi)) if hi > z + tol && lo > z - tol => up = true,
                        Some((lo, hi)) if lo < z - tol && hi < z + tol => down = true,
                        _ => return Lie::Mixed,
                    }
                }
                match (up, down) {
                    (true, false) => Lie::Up,
                    (false, true) => Lie::Down,
                    _ => Lie::Mixed,
                }
            };
            // The top of the walls rising from a loop at `z`: up its faces and on through every
            // face joined to them that rises from `z` and is not flat (a round or chamfer at the
            // foot, the upright wall above it, a round at its top), up to the flat faces round
            // them. A wall that starts on a round is as tall as the wall above the round.
            let flat = |g: usize| matches!(t.surface(g), Some(FaceSurface::Plane { normal, .. }) if lz(normal.vec()).abs() > 1.0 - PARALLEL);
            let wall_top = |lp: &LoopGeo, z: f64| -> f64 {
                let mut seen: BTreeSet<usize> = lp.beside.iter().copied().collect();
                let mut todo: Vec<usize> = lp.beside.iter().copied().filter(|g| !flat(*g)).collect();
                let mut top = lp.beside.iter().filter_map(|g| zrange(*g)).map(|r| r.1).fold(f64::NEG_INFINITY, f64::max);
                while let Some(g) = todo.pop() {
                    for e in t.face_edges(g) {
                        for h in t.edge_faces.get(e).into_iter().flatten().copied() {
                            if !seen.insert(h) || flat(h) {
                                continue;
                            }
                            if let Some((lo, hi)) = zrange(h)
                                && lo >= z - tol
                            {
                                // Turned to an index, the part's own faces slope: one sloping
                                // up from a wall's top is no wall, and the wall ends where it
                                // meets it.
                                if si != 0 && matches!(t.surface(h), Some(FaceSurface::Plane { normal, .. }) if lz(normal.vec()) > SQUARE) {
                                    if let Some((_, eh)) = t.edge_range(e, fr.z) {
                                        top = top.max(eh - fr.origin.dot(fr.z));
                                    }
                                    continue;
                                }
                                top = top.max(hi);
                                todo.push(h);
                            }
                        }
                    }
                }
                top
            };
            let wall_foot = |lp: &LoopGeo| lp.beside.iter().filter_map(|g| zrange(*g)).map(|r| r.0).fold(f64::NEG_INFINITY, f64::max);

            // Holes along this setup's axis.
            let mut setup_holes: Vec<usize> = Vec::new();
            for (ci, c) in cyls.iter().enumerate() {
                if cyl_done.contains(&ci) {
                    continue;
                }
                let along = lz(c.axis);
                if along.abs() < 1.0 - PARALLEL {
                    if si == 0 && axis == AxisSpec::Z && along.abs() <= SQUARE {
                        // Counted once below, if it is a whole hole.
                        let circles = cyl_circles(t, c, c.axis, tol);
                        if full(&circles, tol) && concave(t, c, &circles, c.axis, tol) == Some(true) {
                            side_holes += 1;
                        }
                    }
                    continue;
                }
                let up = if along > 0.0 { c.axis } else { -c.axis };
                let circles = cyl_circles(t, c, up, tol);
                // A hole drilled across a turned part (a 4th-axis bar): its ends are where it
                // meets the turned surface, curves rather than circles. Its extent along its axis
                // is its faces', and it is a hole when material is just outside its wall.
                let across = if full(&circles, tol) {
                    None
                } else if axis == AxisSpec::A {
                    cross_hole(t, c, up, tol)
                } else if circles.is_empty() {
                    curved_through(t, c, up, tol)
                } else {
                    None
                };
                if !full(&circles, tol) && across.is_none() {
                    continue;
                }
                let (lo, hi) = match across {
                    Some(range) => range,
                    None => {
                        let (Some(lo), Some(hi)) = (circles.iter().map(|x| x.1).reduce(f64::min), circles.iter().map(|x| x.1).reduce(f64::max))
                        else {
                            continue;
                        };
                        match down_past(t, c, &circles, up, tol) {
                            // Down from a circle on a flat to a curve where it comes out (a
                            // hole through a milled flat out of a bar's turned surface).
                            Some(bottom) => (bottom, hi),
                            None if concave(t, c, &circles, up, tol) == Some(true) => (lo, hi),
                            None => continue,
                        }
                    }
                };
                let at = |h: f64| circles.iter().filter(move |x| (x.1 - h).abs() <= tol).map(|x| x.0);
                let beside_at = |h: f64| -> Vec<usize> {
                    let mut v: Vec<usize> = Vec::new();
                    for e in at(h) {
                        for g in t.edge_faces.get(e).into_iter().flatten() {
                            if !c.faces.contains(g) && !v.contains(g) {
                                v.push(*g);
                            }
                        }
                    }
                    v
                };
                // Open at the top: a face there faces up (a top face, a counterbore's step) or a
                // countersink widens up from it.
                let mut countersink: Option<(usize, Countersink)> = None;
                let mut open = false;
                for g in beside_at(hi) {
                    match t.surface(g) {
                        Some(FaceSurface::Plane { normal, .. }) if lz(normal.vec()) > 1.0 - PARALLEL => open = true,
                        Some(FaceSurface::Cone { axis: ca, half_angle, .. }) if ca.vec().dot(c.axis).abs() > 1.0 - PARALLEL => {
                            if let Some((_, top)) = zrange(g)
                                && top > z_of(c.origin + up * hi) + tol
                            {
                                let widest = t
                                    .face_edges(g)
                                    .filter_map(|e| match t.shapes.get(e) {
                                        Some(EdgeShape::Circle(k)) => Some(k.radius),
                                        _ => None,
                                    })
                                    .fold(0.0, f64::max);
                                open = true;
                                countersink = Some((g, Countersink { diameter: r9(widest * 2.0), angle_deg: r9(half_angle.to_degrees() * 2.0) }));
                            }
                        }
                        _ => {}
                    }
                }
                if !open && across.is_none() {
                    continue;
                }
                // How it ends at the bottom.
                let mut floor = "through";
                let mut tip: Option<f64> = None;
                let mut through = false;
                let mut decided = false;
                for g in beside_at(lo) {
                    match t.surface(g) {
                        Some(FaceSurface::Plane { normal, .. }) if lz(normal.vec()) < -1.0 + PARALLEL => {
                            through = true;
                            decided = true;
                        }
                        Some(FaceSurface::Plane { normal, .. }) if lz(normal.vec()) > 1.0 - PARALLEL => {
                            floor = "flat";
                            decided = true;
                        }
                        Some(FaceSurface::Cone { apex, axis: ca, .. }) if ca.vec().dot(c.axis).abs() > 1.0 - PARALLEL => {
                            let az = z_of(*apex);
                            if az < z_of(c.origin + up * lo) - tol {
                                floor = "cone";
                                tip = Some(r9(az));
                            } else {
                                through = true;
                            }
                            decided = true;
                        }
                        _ => {}
                    }
                }
                // A curved face below the bottom circle, its edges within the wall's radius, with
                // material just outside the wall under its end, closes the hole (a round at its
                // foot, a ball end): it is blind, its round floor under `bottom_z`. A point below
                // on the axis would be in the hole's air.
                let closes = beside_at(lo).iter().any(|g| {
                    !matches!(t.surface(*g), Some(FaceSurface::Plane { .. } | FaceSurface::Cone { .. }))
                        && zrange(*g).is_some_and(|(_, top)| top <= z_of(c.origin + up * lo) + tol)
                        && t.edges_reach(*g, c.origin, c.axis) <= c.radius + tol
                }) && circles.iter().find(|x| (x.1 - lo).abs() <= tol).is_some_and(|(_, _, k)| {
                    let eps = (tol * 2.0).max(c.radius * 1e-3);
                    let radial = ((k.start - c.origin) - c.axis * (k.start - c.origin).dot(c.axis)).normalized();
                    radial.is_some_and(|r| matches!(t.body.classify_point(k.start + r * eps - up * eps + t.origin), Ok(PointClass::Inside)))
                });
                if !decided && closes {
                    floor = "round";
                    decided = true;
                }
                if !decided {
                    // Just under the bottom on the axis: air means it comes out.
                    let below = c.origin + up * (lo - (tol * 2.0).max(1e-3)) + t.origin;
                    through = matches!(t.body.classify_point(below), Ok(PointClass::Outside));
                    floor = if through { "through" } else { "flat" };
                }
                if through {
                    floor = "through";
                }
                cyl_done.insert(ci);
                for f in &c.faces {
                    claimed.insert((bi, *f));
                }
                if let Some((g, _)) = &countersink {
                    claimed.insert((bi, *g));
                }
                let top_pt = c.origin + up * hi;
                let lt = fr.to_local(top_pt);
                let (top_z, bottom_z) = (r9(lt.z), r9(z_of(c.origin + up * lo)));
                let center = Vec2::new(lt.x, lt.y);
                let d = c.radius * 2.0;
                let hole = HoleReport {
                    id: String::new(),
                    a_deg: a_of(si),
                    body: t.name.clone(),
                    center: r2(center),
                    center_work: r3(top_pt),
                    axis_work: r3(fr.z),
                    diameter: r9(d),
                    top_z,
                    bottom_z,
                    depth: r9(top_z - bottom_z),
                    through,
                    floor: floor.into(),
                    tip_z: tip,
                    counterbore: None,
                    counterbore_of: None,
                    countersink: countersink.map(|(_, c)| c),
                    breaks_out: if across.is_some() {
                        None
                    } else {
                        broken_span(&circles, tol).map(|(a, b)| {
                            let (za, zb) = (z_of(c.origin + up * a), z_of(c.origin + up * b));
                            [r9(za.min(zb)), r9(za.max(zb))]
                        })
                    },
                    faces: c.faces.clone(),
                    thread: None,
                };
                let r = c.radius;
                let geo = Geo {
                    kind: Kind::Hole,
                    setup: si,
                    outer: Some(Loop2::circle(center, r)),
                    islands: Vec::new(),
                    top: top_z,
                    bottom: bottom_z,
                    bounds: [[center.x - r, center.y - r], [center.x + r, center.y + r]],
                    hole: Some((center, d, through)),
                    flat_top: false,
                    open: Vec::new(),
                };
                let key = vec![si as i64, q(-top_z, tol), q(center.y, tol), q(center.x, tol), q(d, tol), bi as i64];
                setup_holes.push(l.holes.len());
                l.holes.push((key, hole, geo));
            }
            // Counterbores: a larger hole on the same axis whose bottom is a smaller one's top.
            for big in &setup_holes {
                for small in &setup_holes {
                    let (Some(b), Some(s)) = (l.holes.get(*big), l.holes.get(*small)) else { continue };
                    let same_axis = b.1.center.iter().zip(&s.1.center).all(|(x, y)| (x - y).abs() <= tol);
                    if same_axis && b.1.diameter > s.1.diameter + tol && (b.1.bottom_z - s.1.top_z).abs() <= tol {
                        l.counterbores.push((*big, *small));
                    }
                }
            }

            // Planar faces facing the tool.
            let mut island_walls: BTreeSet<usize> = BTreeSet::new();
            let mut setup_faces: Vec<(usize, Vec<usize>)> = Vec::new();
            let mut candidates: Vec<(Vec<i64>, ProfileReport, Geo, Seen, Vec<usize>)> = Vec::new();
            for f in 0..t.record.faces.len() {
                let Some(FaceSurface::Plane { normal, point }) = t.surface(f) else { continue };
                if lz(normal.vec()) < 1.0 - PARALLEL {
                    continue;
                }
                let z = z_of(*point);
                let loops = t.face_loops(f, fr, z, tol);
                let Some(oi) = loops.iter().enumerate().max_by(|a, b| a.1.area.abs().total_cmp(&b.1.area.abs())).map(|(i, _)| i) else { continue };
                let Some(outer) = loops.get(oi) else { continue };
                if !(outer.area.abs() > tol * tol) {
                    continue;
                }
                claimed.insert((bi, f));
                // Under material all over (an end wall of a pocket in a bar's side, seen through
                // the bar): no tool of this setup reaches it.
                if covered(t, fr, f, &loops, oi, z, tol) {
                    continue;
                }
                let outer_lie = lie(outer, z);
                let hole_floor = outer_lie == Lie::Up
                    && outer
                        .beside
                        .iter()
                        .all(|g| matches!(t.surface(*g), Some(FaceSurface::Cylinder { axis, .. }) if lz(axis.vec()).abs() > 1.0 - PARALLEL))
                    && {
                        let r = outer.beside.iter().find_map(|g| match t.surface(*g) {
                            Some(FaceSurface::Cylinder { radius, .. }) => Some(*radius),
                            _ => None,
                        });
                        r.is_some_and(|r| (outer.area.abs() - PI * r * r).abs() <= (PI * r * r) * 1e-3 + tol)
                    };
                let role = match outer_lie {
                    Lie::Up if hole_floor => "hole_floor",
                    Lie::Up => "pocket_floor",
                    Lie::Down if (z - su.top).abs() <= tol => "top",
                    Lie::Down => "plateau",
                    Lie::Mixed => "step",
                };
                // Islands: inner loops whose walls rise from this face.
                let islands: Vec<(Loop2, f64, &LoopGeo)> = loops
                    .iter()
                    .enumerate()
                    .filter(|(i, lp)| *i != oi && lie(lp, z) == Lie::Up)
                    .map(|(_, lp)| (Loop2 { segs: oriented(&lp.segs, lp.area, false) }, wall_top(lp, z), lp))
                    .collect();
                let (pts, area, b) = outline(&outer.segs, tol);
                let net_area = r9(area - islands.iter().map(|(_, _, lp)| lp.area.abs()).sum::<f64>());
                let outer_ccw = oriented(&outer.segs, outer.area, true);
                // Which of its edges have no wall rising from them (open sides): air just past the
                // edge and just above the face, by the exact body. In `outer_ccw`'s order. A side
                // a face rises from (by its exact range: from the face's height up) is closed,
                // however gently it starts: a fillet running tangent out of the face and up a
                // wall leaves air just past the edge and just above the face.
                let eps = (tol * 4.0).max(1e-3);
                let rises = |k: usize| {
                    outer.seg_beside.get(k).is_some_and(|gs| gs.iter().any(|g| matches!(zrange(*g), Some((lo, hi)) if hi > z + tol && lo > z - tol)))
                };
                let mut open_flags: Vec<bool> = if outer_lie == Lie::Up {
                    Vec::new()
                } else {
                    outer
                        .segs
                        .iter()
                        .enumerate()
                        .map(|(k, sg)| {
                            if rises(k) {
                                return false;
                            }
                            let (a, b) = (seg_point(sg, 0.499), seg_point(sg, 0.501));
                            let Some(d) = (b - a).normalized() else { return false };
                            let out = if outer.area > 0.0 { Vec2::new(d.y, -d.x) } else { Vec2::new(-d.y, d.x) };
                            let m = seg_point(sg, 0.5) + out * eps;
                            let p = fr.to_world(Vec3::new(m.x, m.y, z + eps)) + t.origin;
                            matches!(t.body.classify_point(p), Ok(PointClass::Outside))
                        })
                        .collect()
                };
                // The faces rising from a side of it that is not open (in `outer.segs`' order).
                let closed_side: BTreeSet<usize> =
                    outer.seg_beside.iter().zip(&open_flags).filter(|(_, open)| !**open).flat_map(|(g, _)| g.iter().copied()).collect();
                if !(outer.area > 0.0) {
                    open_flags.reverse();
                }
                if open_flags.len() != outer_ccw.len() || outer_lie != Lie::Mixed && outer_lie != Lie::Down {
                    open_flags.clear();
                }
                let top = match outer_lie {
                    Lie::Up => wall_top(outer, z),
                    // Only the walls rising from it: a face that also reaches below it past an
                    // open side (the part's end where a slot runs out) is no wall over it, unless
                    // it also rises from a side that is not open (a shoulder round a bar's flat,
                    // which reaches below the flat on the bar's far side).
                    Lie::Mixed => {
                        let rising = LoopGeo {
                            segs: Vec::new(),
                            area: 0.0,
                            beside: outer
                                .beside
                                .iter()
                                .copied()
                                .filter(|g| zrange(*g).is_none_or(|(lo, _)| lo > z - tol) || closed_side.contains(g))
                                .collect(),
                            seg_beside: Vec::new(),
                        };
                        wall_top(&rising, z).max(z)
                    }
                    Lie::Down => z,
                };
                let geo = Geo {
                    kind: Kind::Face,
                    setup: si,
                    outer: Some(Loop2 { segs: outer_ccw.clone() }),
                    islands: islands.iter().map(|(lp, top, _)| (lp.clone(), *top)).collect(),
                    top: r9(top),
                    bottom: r9(z),
                    bounds: b,
                    hole: None,
                    flat_top: outer_lie == Lie::Down,
                    open: open_flags,
                };
                let face = FaceReport {
                    id: String::new(),
                    a_deg: a_of(si),
                    body: t.name.clone(),
                    face: f,
                    z: r9(z),
                    area: net_area,
                    bounds: b,
                    role: role.into(),
                    pocket: None,
                    normal_work: r3(normal.vec()),
                };
                let cx = (b[0][0] + b[1][0]) * 0.5;
                let cy = (b[0][1] + b[1][1]) * 0.5;
                let key = vec![si as i64, q(-z, tol), q(cy, tol), q(cx, tol), bi as i64, f as i64];
                let face_index = l.faces.len();
                l.faces.push((key.clone(), face, geo.clone(), Seen { body: bi, face: f, setup: si }));
                if role == "pocket_floor" {
                    // The walls' inside corners: free space (the pocket) on the left.
                    let (mut radius, mut sharp) = corner_limits(&outer_ccw);
                    for (lp, ..) in &islands {
                        let (r, s) = corner_limits(&lp.segs);
                        radius = match (radius, r) {
                            (Some(a), Some(b)) => Some(a.min(b)),
                            (a, b) => a.or(b),
                        };
                        sharp += s;
                    }
                    let half = ((b[1][0] - b[0][0]).min(b[1][1] - b[0][1]) * 0.5).max(0.0);
                    let min_corner = if sharp > 0 { 0.0 } else { radius.unwrap_or(half) };
                    let walls_vertical = outer.beside.iter().all(|g| vertical(*g));
                    let pocket = PocketReport {
                        id: String::new(),
                        a_deg: a_of(si),
                        body: t.name.clone(),
                        floor_face: f,
                        face_id: String::new(),
                        floor_z: r9(z),
                        top_z: r9(top),
                        depth: r9(top - z),
                        outline: pts.clone(),
                        area: net_area,
                        bounds: b,
                        min_corner_radius: r9(min_corner),
                        sharp_corners: sharp,
                        walls_vertical,
                        islands: islands
                            .iter()
                            .map(|(lp, top, _)| {
                                let (pts, area, _) = outline(&lp.segs, tol);
                                IslandReport { outline: pts, top_z: r9(*top), area }
                            })
                            .collect(),
                        through: false,
                    };
                    let mut g = geo.clone();
                    g.kind = Kind::Pocket;
                    g.flat_top = false;
                    l.pockets.push((key.clone(), pocket, g, face_index));
                }
                if role == "pocket_floor" {
                    for (_, _, lp) in &islands {
                        island_walls.extend(lp.beside.iter().copied());
                    }
                }
                // Openings through the part from this face (a slot, an obround, a window with
                // rounded corners): an inner loop whose upright walls fall away from it down to
                // air, not a round hole's (its walls are the hole's). A pocket cut through the
                // part, a little past its bottom.
                for (i, lp) in loops.iter().enumerate() {
                    if i == oi
                        || lp.beside.is_empty()
                        || lie(lp, z) != Lie::Down
                        || !lp.beside.iter().all(|g| vertical(*g) && !claimed.contains(&(bi, *g)))
                    {
                        continue;
                    }
                    let foot = wall_foot(lp);
                    if !(foot.is_finite() && foot < z - tol) {
                        continue;
                    }
                    let ccw = oriented(&lp.segs, lp.area, true);
                    // Air just inside the opening under the face and under its walls' foot, by
                    // the exact body.
                    let Some(sg) = ccw.first() else { continue };
                    let Some(d) = (seg_point(sg, 0.501) - seg_point(sg, 0.499)).normalized() else { continue };
                    let m = seg_point(sg, 0.5) + Vec2::new(-d.y, d.x) * eps;
                    let air = |h: f64| matches!(t.body.classify_point(fr.to_world(Vec3::new(m.x, m.y, h)) + t.origin), Ok(PointClass::Outside));
                    if !(air(z - eps) && air(foot - eps)) {
                        continue;
                    }
                    // Through the part only where nothing of it is under the walls' foot, just
                    // inside every side, down to the breakthrough: else the opening leads into
                    // more of the part (a fillet or a slope running on from the walls' foot, a
                    // deeper pocket), and a cut past the foot would go into it: cut to the foot.
                    let fine = (tol * 0.5).clamp(1e-4, eps);
                    let step = ccw.len().div_ceil(64).max(1);
                    let clear_below = ccw.iter().step_by(step).all(|sg| {
                        let Some(d) = (seg_point(sg, 0.501) - seg_point(sg, 0.499)).normalized() else { return true };
                        let m = seg_point(sg, 0.5) + Vec2::new(-d.y, d.x) * fine;
                        let h = foot - BREAKTHROUGH;
                        matches!(t.body.classify_point(fr.to_world(Vec3::new(m.x, m.y, h)) + t.origin), Ok(PointClass::Outside))
                    });
                    let (pts, area, bb) = outline(&ccw, tol);
                    let (radius, sharp) = corner_limits(&ccw);
                    let half = ((bb[1][0] - bb[0][0]).min(bb[1][1] - bb[0][1]) * 0.5).max(0.0);
                    let min_corner = if sharp > 0 { 0.0 } else { radius.unwrap_or(half) };
                    let pocket = PocketReport {
                        id: String::new(),
                        a_deg: a_of(si),
                        body: t.name.clone(),
                        floor_face: lp.beside.first().copied().unwrap_or(f),
                        face_id: String::new(),
                        floor_z: r9(foot),
                        top_z: r9(z),
                        depth: r9(z - foot),
                        outline: pts,
                        area: r9(area.abs()),
                        bounds: bb,
                        min_corner_radius: r9(min_corner),
                        sharp_corners: sharp,
                        walls_vertical: true,
                        islands: Vec::new(),
                        through: clear_below,
                    };
                    let g = Geo {
                        kind: Kind::Pocket,
                        setup: si,
                        outer: Some(Loop2 { segs: ccw }),
                        islands: Vec::new(),
                        top: r9(z),
                        bottom: r9(if clear_below { foot - BREAKTHROUGH } else { foot }),
                        bounds: bb,
                        hole: None,
                        flat_top: false,
                        open: Vec::new(),
                    };
                    claimed.extend(lp.beside.iter().map(|g| (bi, *g)));
                    let (cx, cy) = ((bb[0][0] + bb[1][0]) * 0.5, (bb[0][1] + bb[1][1]) * 0.5);
                    // After the setup's pockets with a floor, so their ids stay as they were.
                    let key = vec![si as i64, i64::MAX, q(-z, tol), q(cy, tol), q(cx, tol), bi as i64, f as i64];
                    l.pockets.push((key, pocket, g, usize::MAX));
                }
                setup_faces.push((face_index, outer.beside.clone()));
                // An outline whose walls fall straight away all round is a profile (unless it is
                // an island's top, found once every pocket of the setup is).
                if outer_lie == Lie::Down && outer.beside.iter().all(|g| vertical(*g)) {
                    let foot = wall_foot(outer);
                    if foot.is_finite() && foot < z - tol {
                        let (rad, sharp) = corner_limits(&oriented(&outer.segs, outer.area, false));
                        let notch = if sharp > 0 { Some(0.0) } else { rad.map(r9) };
                        let profile = ProfileReport {
                            id: String::new(),
                            a_deg: a_of(si),
                            body: t.name.clone(),
                            top_z: r9(z),
                            bottom_z: r9(foot),
                            depth: r9(z - foot),
                            outline: pts.clone(),
                            area,
                            bounds: b,
                            min_concave_radius: notch,
                            face_id: String::new(),
                        };
                        let pg = Geo { kind: Kind::Profile, islands: Vec::new(), top: r9(z), bottom: r9(foot), flat_top: false, ..geo.clone() };
                        candidates.push((
                            vec![si as i64, q(-z, tol), q(-area, tol), bi as i64, f as i64],
                            profile,
                            pg,
                            Seen { body: bi, face: f, setup: si },
                            outer.beside.clone(),
                        ));
                    }
                }
                // Faces falling away from its edges that are not walls: rounds and chamfers.
                for lp in &loops {
                    for g in &lp.beside {
                        let g = *g;
                        if claimed.contains(&(bi, g)) || vertical(g) {
                            continue;
                        }
                        let Some((lo, hi)) = zrange(g) else { continue };
                        if !(lo < z - tol && hi < z + tol) {
                            continue;
                        }
                        let kind = match t.surface(g) {
                            Some(FaceSurface::Cylinder { axis, radius, .. }) if lz(axis.vec()).abs() <= SQUARE => Some((true, *radius)),
                            Some(FaceSurface::Torus { minor, .. }) => Some((true, *minor)),
                            Some(FaceSurface::Sphere { radius, .. }) => Some((true, *radius)),
                            // The face's angle from the tool axis, as a cone's half angle is.
                            Some(FaceSurface::Plane { normal, .. }) if lz(normal.vec()) > SQUARE => {
                                Some((false, lz(normal.vec()).clamp(-1.0, 1.0).asin().to_degrees()))
                            }
                            Some(FaceSurface::Cone { half_angle, axis, .. }) if lz(axis.vec()).abs() > 1.0 - PARALLEL => {
                                Some((false, half_angle.to_degrees()))
                            }
                            _ => None,
                        };
                        let Some((round, value)) = kind else { continue };
                        // A face this close to upright is a drafted wall (a pocket's or a
                        // profile's), not a narrow bevel along an edge: the walls are finished
                        // with them.
                        if !round && value < DRAFT_MAX_DEG {
                            continue;
                        }
                        // A flat facing another setup's tool (a hex bar's next flat, sloping
                        // from this index) is that setup's flat, cut there: not a chamfer here.
                        let facing_other = |n: Vec3| setups.iter().enumerate().any(|(k, o)| k != si && o.frame.dir_to_local(n).z > 1.0 - PARALLEL);
                        if matches!(t.surface(g), Some(FaceSurface::Plane { normal, .. }) if facing_other(normal.vec())) {
                            continue;
                        }
                        claimed.insert((bi, g));
                        let (xl, xh) = t.face_range(g, fr.x, true).unwrap_or_default();
                        let (yl, yh) = t.face_range(g, fr.y, true).unwrap_or_default();
                        let (ox, oy) = (fr.origin.dot(fr.x), fr.origin.dot(fr.y));
                        let b = [[xl - ox, yl - oy], [xh - ox, yh - oy]];
                        let report = EdgeBreakReport {
                            id: String::new(),
                            a_deg: a_of(si),
                            body: t.name.clone(),
                            faces: vec![g],
                            radius: round.then_some(r9(value)),
                            angle_deg: (!round).then_some(r9(value)),
                            top_z: r9(hi),
                            bottom_z: r9(lo),
                            bounds: rb(b),
                        };
                        let geo = Geo {
                            kind: Kind::Area,
                            setup: si,
                            outer: None,
                            islands: Vec::new(),
                            top: r9(hi),
                            bottom: r9(lo),
                            bounds: rb(b),
                            hole: None,
                            flat_top: false,
                            open: Vec::new(),
                        };
                        let key =
                            vec![si as i64, q(-hi, tol), q((b[0][1] + b[1][1]) * 0.5, tol), q((b[0][0] + b[1][0]) * 0.5, tol), bi as i64, g as i64];
                        if round {
                            merge_break(&mut l.fillets, t, key, report, geo, tol);
                        } else {
                            merge_break(&mut l.chamfers, t, key, report, geo, tol);
                        }
                    }
                }
            }
            // An island's top: every wall round it rises from a pocket's floor.
            let island = |beside: &[usize]| !beside.is_empty() && beside.iter().all(|g| island_walls.contains(g));
            for (fi, beside) in &setup_faces {
                if island(beside)
                    && let Some(entry) = l.faces.get_mut(*fi)
                {
                    entry.1.role = "island_top".into();
                }
            }
            // The part's silhouette seen from below is a profile too, when every face facing down
            // (the bottom, and the ceiling of a slot or recess cut from below) has walls rising
            // straight all round it: the outline of those faces seen together, so a slot across
            // the underside that splits the bottom face does not split the outline. Unless a
            // face above gave the same.
            let mut tiles: Vec<Vec<Seg2>> = Vec::new();
            let mut first_bottom: Option<usize> = None;
            let mut straight = true;
            for f in 0..t.record.faces.len() {
                let Some(FaceSurface::Plane { normal, point }) = t.surface(f) else { continue };
                if lz(normal.vec()) > -1.0 + PARALLEL {
                    continue;
                }
                let z = z_of(*point);
                let loops = t.face_loops(f, fr, z, tol);
                let Some(outer) = loops.iter().max_by(|a, b| a.area.abs().total_cmp(&b.area.abs())) else { continue };
                let walls = outer.beside.iter().all(|g| vertical(*g));
                if (z - su.bottom).abs() <= tol {
                    straight &= walls && lie(outer, z) == Lie::Up;
                    first_bottom = first_bottom.or(Some(f));
                }
                // A face whose walls are not upright (a recess rounded at its ceiling) is left out:
                // the outline is then checked against the part's extent below.
                if walls {
                    tiles.push(oriented(&outer.segs, outer.area, true));
                }
            }
            // The part's extent across this setup, from its faces.
            let extent = |d: Vec3| {
                (0..t.record.faces.len())
                    .filter_map(|g| t.face_range(g, d, false))
                    .fold(None, |acc, (lo, hi)| Some(widen(acc, lo - fr.origin.dot(d), hi - fr.origin.dot(d))))
            };
            let whole = |b: [[f64; 2]; 2]| match (extent(fr.x), extent(fr.y)) {
                (Some((x0, x1)), Some((y0, y1))) => {
                    let near = |a: f64, c: f64| (a - c).abs() <= tol * 10.0;
                    near(b[0][0], x0) && near(b[1][0], x1) && near(b[0][1], y0) && near(b[1][1], y1)
                }
                _ => false,
            };
            // A face's outline that is the rectangle round the whole part (its bounds the part's
            // extent, its area theirs) has every step of the part inside it: on a part with a flat
            // bottom whose walls rise straight from it, its walls are cut down to that bottom,
            // past the foot of its own walls (the side of an L turned on its back, its foot
            // standing below it), where nothing of the part stands out. (A part rounded below
            // keeps its outline at its walls' foot: what is under it a later setup cuts.)
            let flat_bottom = first_bottom.is_some() && straight;
            for c in candidates.iter_mut().filter(|_| flat_bottom) {
                let (p, g) = (&mut c.1, &mut c.2);
                let span = (p.bounds[1][0] - p.bounds[0][0]) * (p.bounds[1][1] - p.bounds[0][1]);
                let rim = 2.0 * ((p.bounds[1][0] - p.bounds[0][0]) + (p.bounds[1][1] - p.bounds[0][1]));
                if p.bottom_z > su.bottom + tol && (span - p.area.abs()).abs() <= rim * tol && whole(p.bounds) {
                    p.bottom_z = r9(su.bottom);
                    p.depth = r9(p.top_z - su.bottom);
                    g.bottom = r9(su.bottom);
                }
            }
            l.profiles.extend(candidates.into_iter().filter(|c| !island(&c.4)).map(|(k, p, g, seen, _)| (k, p, g, seen)));
            // The outline from below only where it is the whole part's: a bottom narrower than a
            // step above it (the foot of an L turned on its side) has the step standing out over
            // its walls, and a contour round it from the setup's top cuts through the step.
            let silhouette = match first_bottom {
                Some(f) if straight => union_outline(&tiles, tol).filter(|segs| whole(loop_bounds(segs, tol))).map(|segs| (f, segs)),
                _ => None,
            };
            // Else, where no profile was found for this body in this setup (a slot or notch
            // splitting its top, its edges chamfered all round): its outermost upright walls.
            let silhouette = silhouette.or_else(|| {
                let found = l.profiles.iter().any(|(k, ..)| k.first() == Some(&(si as i64)) && k.get(3) == Some(&(bi as i64)));
                if found {
                    return None;
                }
                wall_outline(t, fr, su.bottom, su.top, tol).filter(|(_, segs)| whole(loop_bounds(segs, tol)))
            });
            if let Some((f, segs)) = silhouette {
                let z = su.bottom;
                let (pts, area, b) = outline(&segs, tol);
                let dup = l.profiles.iter().any(|(_, p, ..)| {
                    p.a_deg == a_of(si)
                        && (p.bottom_z - r9(z)).abs() <= tol
                        && (p.area - area).abs() <= area * 1e-6 + tol * tol
                        && p.bounds.iter().flatten().zip(b.iter().flatten()).all(|(x, y)| (x - y).abs() <= tol)
                });
                let (rad, sharp) = corner_limits(&oriented(&segs, area, false));
                let profile = ProfileReport {
                    id: String::new(),
                    a_deg: a_of(si),
                    body: t.name.clone(),
                    top_z: su.top,
                    bottom_z: r9(z),
                    depth: r9(su.top - z),
                    outline: pts,
                    area,
                    bounds: b,
                    min_concave_radius: if sharp > 0 { Some(0.0) } else { rad.map(r9) },
                    face_id: String::new(),
                };
                let pg = Geo {
                    kind: Kind::Profile,
                    setup: si,
                    outer: Some(Loop2 { segs: segs.clone() }),
                    islands: Vec::new(),
                    top: su.top,
                    bottom: r9(z),
                    bounds: b,
                    hole: None,
                    flat_top: false,
                    open: Vec::new(),
                };
                if !dup {
                    l.profiles.push((
                        vec![si as i64, q(-su.top, tol), q(-area, tol), bi as i64, f as i64],
                        profile,
                        pg,
                        Seen { body: bi, face: f, setup: si },
                    ));
                }
            }
            for f in 0..t.record.faces.len() {
                if let Some(FaceSurface::Plane { normal, .. }) = t.surface(f)
                    && lz(normal.vec()) < -1.0 + PARALLEL
                {
                    claimed.insert((bi, f));
                }
            }
        }
        // Diameters turned about the A axis.
        if axis == AxisSpec::A {
            for (ci, c) in cyls.iter().enumerate() {
                if cyl_done.contains(&ci) || c.axis.x.abs() < 1.0 - PARALLEL {
                    continue;
                }
                let off = c.origin - Vec3::X * c.origin.x;
                if off.len() > tol {
                    continue;
                }
                let circles = cyl_circles(t, c, Vec3::X, tol);
                match concave(t, c, &circles, Vec3::X, tol) {
                    Some(false) => {}
                    // A bore on the A axis (a ring's or a sleeve's): no tool along Z on this
                    // setup reaches it. Said, with its extent and the chamfers at its ends.
                    Some(true) if full(&circles, tol) => {
                        let ends: Vec<usize> = (0..t.record.faces.len())
                            .filter(|g| matches!(t.surface(*g), Some(FaceSurface::Cone { axis, .. }) if axis.vec().x.abs() > 1.0 - PARALLEL))
                            .filter(|g| c.faces.iter().any(|h| shares_edge(t, *h, *g)))
                            .collect();
                        // Its extent with the chamfers at its ends, from their exact faces.
                        let Some((x0, x1)) = c
                            .faces
                            .iter()
                            .chain(&ends)
                            .filter_map(|g| t.face_range(*g, Vec3::X, false))
                            .fold(None, |acc, (lo, hi)| Some(widen(acc, lo, hi)))
                        else {
                            continue;
                        };
                        // The most its faces stand off the axis (the chamfers' wide ends), from
                        // their exact faces: the axis is work X.
                        let off = c
                            .faces
                            .iter()
                            .chain(&ends)
                            .flat_map(|g| [t.face_range(*g, Vec3::Y, false), t.face_range(*g, Vec3::Z, false)])
                            .flatten()
                            .fold(c.radius, |r, (lo, hi)| r.max(lo.abs()).max(hi.abs()));
                        a_bores.push((r9(c.radius * 2.0), r9(x0), r9(x1), ends.len(), r9(off)));
                        continue;
                    }
                    _ => continue,
                }
                let xs: Vec<f64> = circles.iter().map(|(_, _, k)| k.center.x).collect();
                let (Some(x0), Some(x1)) = (xs.iter().copied().reduce(f64::min), xs.iter().copied().reduce(f64::max)) else { continue };
                for f in &c.faces {
                    claimed.insert((bi, *f));
                }
                let report = TurnedReport {
                    id: String::new(),
                    body: t.name.clone(),
                    diameter: r9(c.radius * 2.0),
                    x_min: r9(x0),
                    x_max: r9(x1),
                    faces: c.faces.clone(),
                    ..TurnedReport::default()
                };
                l.turned.push((vec![q(x0, tol), q(c.radius, tol), bi as i64], report));
            }
        }
        // Pockets wrapped round the A axis.
        if axis == AxisSpec::A {
            wrapped_pockets(t, bi, tol, &mut l.wrapped);
        }
        // What is left that is not a wall: curved areas, reached from the first setup.
        if let Some(su) = setups.first() {
            let fr = &su.frame;
            let lz = |n: Vec3| fr.dir_to_local(n).z;
            // A cylinder about any setup's tool axis is a wall there (a hole's, a pocket's corner
            // on an indexed side), cut by that setup's features: not a curved area, and on a 4th
            // axis not a turned face (its own axis is not A's).
            // A hole running across the setups' axes (a side port, a cross passage) is a hole
            // for another setup: its wall, closed all the way round with material outside it, is
            // hidden from the tool here, and so is its drill point (a cone on its axis touching
            // it). A drill point at the foot of a hole along a setup's axis is the drill's too.
            // Read from the exact faces, not as a curved area.
            let mut side_hole: BTreeSet<usize> = BTreeSet::new();
            // Each whole hole with the direction its tool comes from (`None` across every setup).
            let mut holes: Vec<(&Cyl, Option<Vec3>)> = Vec::new();
            for c in &cyls {
                let circles = cyl_circles(t, c, c.axis, tol);
                if !(full(&circles, tol) && concave(t, c, &circles, c.axis, tol) == Some(true)) {
                    continue;
                }
                let from = setups.iter().find(|x| x.frame.dir_to_local(c.axis).z.abs() > 1.0 - PARALLEL).map(|x| x.frame.z);
                if from.is_none() {
                    side_hole.extend(c.faces.iter().copied());
                }
                holes.push((c, from));
            }
            for g in 0..t.record.faces.len() {
                let Some(FaceSurface::Cone { apex, axis, .. }) = t.surface(g) else { continue };
                let tip = |(c, from): &(&Cyl, Option<Vec3>)| {
                    let off = (*apex - c.origin) - c.axis * (*apex - c.origin).dot(c.axis);
                    if axis.vec().dot(c.axis).abs() <= 1.0 - PARALLEL || off.len() > tol || !c.faces.iter().any(|h| shares_edge(t, *h, g)) {
                        return false;
                    }
                    let Some(up) = from else { return true };
                    let wall_lo = c.faces.iter().filter_map(|h| t.face_range(*h, *up, false)).map(|r| r.0).fold(f64::INFINITY, f64::min);
                    t.face_range(g, *up, false).is_some_and(|(_, hi)| hi <= wall_lo + tol)
                };
                if holes.iter().any(tip) {
                    side_hole.insert(g);
                }
            }
            let wall = |g: usize| match t.surface(g) {
                _ if side_hole.contains(&g) => true,
                Some(FaceSurface::Plane { normal, .. }) => lz(normal.vec()) <= SQUARE,
                Some(FaceSurface::Cylinder { axis, .. }) => setups.iter().any(|x| x.frame.dir_to_local(axis.vec()).z.abs() > 1.0 - PARALLEL),
                // A cone facing down (a chamfer the other side's setup cut, seen after the part is
                // turned over): no tool coming down reaches it, as a flat face facing down.
                Some(FaceSurface::Cone { .. }) => cone_faces_down(t, fr, g, tol),
                _ => false,
            };
            let left: Vec<usize> = (0..t.record.faces.len()).filter(|f| !claimed.contains(&(bi, *f)) && !wall(*f)).collect();
            let mut groups: Vec<Vec<usize>> = Vec::new();
            for f in left {
                let touching: Vec<usize> =
                    groups.iter().enumerate().filter(|(_, g)| g.iter().any(|h| shares_edge(t, *h, f))).map(|(i, _)| i).collect();
                let mut merged = vec![f];
                for i in touching.iter().rev() {
                    if *i < groups.len() {
                        merged.extend(groups.remove(*i));
                    }
                }
                merged.sort_unstable();
                groups.push(merged);
            }
            groups.sort();
            for faces in groups {
                let o = fr.origin;
                let range = |d: Vec3| {
                    faces.iter().filter_map(|f| t.face_range(*f, d, true)).fold(None, |acc, (a, b)| Some(widen(acc, a - o.dot(d), b - o.dot(d))))
                };
                let (Some((zl, zh)), Some((xl, xh)), Some((yl, yh))) = (range(fr.z), range(fr.x), range(fr.y)) else { continue };
                if zh <= su.bottom + tol {
                    continue;
                }
                let mut kinds: BTreeSet<&str> = BTreeSet::new();
                let mut min_radius: Option<f64> = None;
                for f in &faces {
                    let (k, r) = match t.surface(*f) {
                        Some(FaceSurface::Sphere { radius, .. }) => ("sphere", Some(*radius)),
                        Some(FaceSurface::Torus { minor, .. }) => ("torus", Some(*minor)),
                        Some(FaceSurface::Cylinder { radius, .. }) => ("cylinder", Some(*radius)),
                        Some(FaceSurface::Cone { .. }) => ("cone", None),
                        Some(FaceSurface::Plane { .. }) => ("plane", None),
                        None => ("freeform", None),
                    };
                    kinds.insert(k);
                    if let Some(r) = r {
                        min_radius = Some(min_radius.map_or(r, |m: f64| m.min(r)));
                    }
                }
                let b = rb([[xl, yl], [xh, yh]]);
                let report = SurfaceReport {
                    id: String::new(),
                    a_deg: a_of(0),
                    body: t.name.clone(),
                    faces: faces.clone(),
                    kinds: kinds.into_iter().map(str::to_string).collect(),
                    min_radius: min_radius.map(r9),
                    top_z: r9(zh),
                    bottom_z: r9(zl.max(su.bottom)),
                    bounds: b,
                    undercut_faces: undercut_faces(t, fr, &faces, su.bottom, tol),
                };
                let geo = Geo {
                    kind: Kind::Area,
                    setup: 0,
                    outer: None,
                    islands: Vec::new(),
                    top: r9(zh),
                    bottom: r9(zl.max(su.bottom)),
                    bounds: b,
                    hole: None,
                    flat_top: false,
                    open: Vec::new(),
                };
                let key =
                    vec![0, q(-zh, tol), q((yl + yh) * 0.5, tol), q((xl + xh) * 0.5, tol), bi as i64, faces.first().copied().unwrap_or(0) as i64];
                l.surfaces.push((key, report, geo));
            }
        }
    }
    if side_holes > 0 {
        notes.push(format!("{side_holes} holes run square to Z, not reachable on a 3-axis mill: find them with `axis: \"a\"` or another setup"));
    }
    a_bores.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.total_cmp(&b.0)));
    for (k, (d, x0, x1, ends, _)) in a_bores.iter().enumerate() {
        let id = format!("bore:{k}");
        let chamfers = match ends {
            0 => String::new(),
            1 => " and the chamfer at its end".to_string(),
            n => format!(" and the {n} chamfers at its ends"),
        };
        notes.push(format!(
            "{id}: a {} mm bore along the A axis, X {} to {}{chamfers}: no tool along Z reaches it on the A axis; cut it in another setup, with the part standing on its end",
            num(*d),
            num(*x0),
            num(*x1)
        ));
    }
    number(l, tol)
}

/// Whether the circles at one height of a cylinder go all the way round.
fn full(circles: &[(usize, f64, &EdgeCircle)], tol: f64) -> bool {
    circles.iter().any(|x| full_at(circles, x.1, tol))
}

/// Whether the circles at height `h` go all the way round.
fn full_at(circles: &[(usize, f64, &EdgeCircle)], h: f64, tol: f64) -> bool {
    circles.iter().filter(|x| (x.1 - h).abs() <= tol).map(|x| x.2.angle).sum::<f64>() >= TAU - 1e-6
}

/// [lowest, highest] height of the arcs that do not go all the way round: where the wall breaks
/// into another feature. `None` when every circle is whole.
fn broken_span(circles: &[(usize, f64, &EdgeCircle)], tol: f64) -> Option<(f64, f64)> {
    let open = circles.iter().filter(|x| !full_at(circles, x.1, tol)).map(|x| x.1);
    open.fold(None, |acc, h| Some(widen(acc, h, h)))
}

fn shares_edge(t: &Topo<'_>, a: usize, b: usize) -> bool {
    t.face_edges(a).any(|e| t.edge_faces.get(e).is_some_and(|fs| fs.contains(&b)))
}

/// A round or chamfer face added to the feature it touches with the same size, else a new one.
fn merge_break(list: &mut Vec<(Vec<i64>, EdgeBreakReport, Geo)>, t: &Topo<'_>, key: Vec<i64>, report: EdgeBreakReport, geo: Geo, tol: f64) {
    let size = report.radius.or(report.angle_deg).unwrap_or(0.0);
    let Some(face) = report.faces.first().copied() else { return };
    let near = list.iter_mut().find(|(_, r, g)| {
        r.body == report.body
            && g.setup == geo.setup
            && (r.radius.or(r.angle_deg).unwrap_or(0.0) - size).abs() <= tol
            && r.faces.iter().any(|h| shares_edge(t, *h, face))
    });
    match near {
        Some((k, r, g)) => {
            r.faces.push(face);
            r.faces.sort_unstable();
            r.top_z = r.top_z.max(report.top_z);
            r.bottom_z = r.bottom_z.min(report.bottom_z);
            let b = [
                [r.bounds[0][0].min(report.bounds[0][0]), r.bounds[0][1].min(report.bounds[0][1])],
                [r.bounds[1][0].max(report.bounds[1][0]), r.bounds[1][1].max(report.bounds[1][1])],
            ];
            r.bounds = b;
            g.bounds = b;
            g.top = r.top_z;
            g.bottom = r.bottom_z;
            if key < *k {
                *k = key;
            }
        }
        None => list.push((key, report, geo)),
    }
}

/// Sorts every list and numbers the features.
fn number(mut l: Lists, tol: f64) -> Found {
    let mut reply = CamFeaturesReply::default();
    let mut geo: Vec<(String, Geo)> = Vec::new();

    // Holes, with their counterbores.
    let mut order: Vec<usize> = (0..l.holes.len()).collect();
    order.sort_by(|a, b| l.holes.get(*a).map(|x| &x.0).cmp(&l.holes.get(*b).map(|x| &x.0)));
    let mut hole_id: BTreeMap<usize, String> = BTreeMap::new();
    for (k, i) in order.iter().enumerate() {
        hole_id.insert(*i, format!("hole:{k}"));
    }
    for (big, small) in &l.counterbores {
        let (Some(b), Some(s)) = (hole_id.get(big).cloned(), hole_id.get(small).cloned()) else { continue };
        if let Some(h) = l.holes.get_mut(*big) {
            h.1.counterbore_of = Some(s);
        }
        if let Some(h) = l.holes.get_mut(*small) {
            h.1.counterbore = Some(b);
        }
    }
    let mut holes: Vec<Option<(Vec<i64>, HoleReport, Geo)>> = l.holes.into_iter().map(Some).collect();
    for (k, i) in order.iter().enumerate() {
        let Some(Some((_, mut h, g))) = holes.get_mut(*i).map(Option::take) else { continue };
        h.id = format!("hole:{k}");
        geo.push((h.id.clone(), g));
        reply.holes.push(h);
    }
    // Groups of equal holes.
    for h in &reply.holes {
        let same = reply.hole_groups.iter_mut().find(|g| {
            (g.diameter - h.diameter).abs() <= tol.max(1e-9)
                && (g.depth - h.depth).abs() <= tol.max(1e-9)
                && g.through == h.through
                && g.a_deg == h.a_deg
        });
        match same {
            Some(g) => {
                g.count += 1;
                g.ids.push(h.id.clone());
            }
            None => reply.hole_groups.push(HoleGroup {
                diameter: h.diameter,
                depth: h.depth,
                through: h.through,
                a_deg: h.a_deg,
                count: 1,
                ids: vec![h.id.clone()],
            }),
        }
    }

    // Faces first, so pockets and profiles can name them.
    let mut face_order: Vec<usize> = (0..l.faces.len()).collect();
    face_order.sort_by(|a, b| l.faces.get(*a).map(|x| &x.0).cmp(&l.faces.get(*b).map(|x| &x.0)));
    let mut face_id: BTreeMap<usize, String> = BTreeMap::new();
    for (k, i) in face_order.iter().enumerate() {
        face_id.insert(*i, format!("face:{k}"));
    }
    l.pockets.sort_by(|a, b| a.0.cmp(&b.0));
    let mut pocket_of_face: BTreeMap<usize, String> = BTreeMap::new();
    for (k, (_, p, g, fi)) in l.pockets.iter_mut().enumerate() {
        p.id = format!("pocket:{k}");
        p.face_id = face_id.get(fi).cloned().unwrap_or_default();
        pocket_of_face.insert(*fi, p.id.clone());
        geo.push((p.id.clone(), g.clone()));
    }
    reply.pockets = l.pockets.into_iter().map(|(_, p, ..)| p).collect();
    // Wrapped pockets, so their walls' faces can name them.
    l.wrapped.sort_by(|a, b| a.0.cmp(&b.0));
    let mut wall_of: BTreeMap<(usize, usize), String> = BTreeMap::new();
    for (k, (_, r, bi)) in l.wrapped.iter_mut().enumerate() {
        r.id = format!("wrapped:{k}");
        for w in &r.walls {
            wall_of.insert((*bi, *w), r.id.clone());
        }
    }
    let mut faces: Vec<Option<(Vec<i64>, FaceReport, Geo, Seen)>> = l.faces.into_iter().map(Some).collect();
    let mut seen_face: BTreeMap<(usize, usize, usize), String> = BTreeMap::new();
    for (k, i) in face_order.iter().enumerate() {
        let Some(Some((_, mut f, g, seen))) = faces.get_mut(*i).map(Option::take) else { continue };
        f.id = format!("face:{k}");
        f.pocket = pocket_of_face.get(i).cloned();
        if let Some(w) = wall_of.get(&(seen.body, seen.face)) {
            f.role = "wrapped_wall".into();
            f.pocket = Some(w.clone());
        }
        seen_face.insert((seen.body, seen.face, seen.setup), f.id.clone());
        geo.push((f.id.clone(), g));
        reply.faces.push(f);
    }
    l.profiles.sort_by(|a, b| a.0.cmp(&b.0));
    for (k, (_, mut p, g, seen)) in l.profiles.into_iter().enumerate() {
        p.id = format!("profile:{k}");
        p.face_id = seen_face.get(&(seen.body, seen.face, seen.setup)).cloned().unwrap_or_default();
        geo.push((p.id.clone(), g));
        reply.profiles.push(p);
    }
    for (list, kind, out) in [(l.fillets, "fillet", &mut reply.fillets), (l.chamfers, "chamfer", &mut reply.chamfers)] {
        let mut list = list;
        list.sort_by(|a, b| a.0.cmp(&b.0));
        for (k, (_, mut r, g)) in list.into_iter().enumerate() {
            r.id = format!("{kind}:{k}");
            geo.push((r.id.clone(), g));
            out.push(r);
        }
    }
    l.surfaces.sort_by(|a, b| a.0.cmp(&b.0));
    for (k, (_, mut r, g)) in l.surfaces.into_iter().enumerate() {
        r.id = format!("surface:{k}");
        geo.push((r.id.clone(), g));
        reply.surfaces.push(r);
    }
    l.turned.sort_by(|a, b| a.0.cmp(&b.0));
    for (k, (_, mut r)) in l.turned.into_iter().enumerate() {
        r.id = format!("turned:{k}");
        let g = Geo {
            kind: Kind::Area,
            setup: 0,
            outer: None,
            islands: Vec::new(),
            top: r.diameter * 0.5,
            bottom: 0.0,
            bounds: [[r.x_min, -r.diameter * 0.5], [r.x_max, r.diameter * 0.5]],
            hole: None,
            flat_top: false,
            open: Vec::new(),
        };
        geo.push((r.id.clone(), g));
        reply.turned.push(r);
    }
    for (_, r, _) in l.wrapped {
        let g = Geo {
            kind: Kind::Area,
            setup: 0,
            outer: None,
            islands: Vec::new(),
            top: r.radius,
            bottom: r.floor_radius,
            bounds: [[r.x_min, -r.radius], [r.x_max, r.radius]],
            hole: None,
            flat_top: false,
            open: Vec::new(),
        };
        geo.push((r.id.clone(), g));
        reply.wrapped.push(r);
    }
    Found { reply, geo }
}
