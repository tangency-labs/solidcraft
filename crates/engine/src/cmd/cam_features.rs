//! `cam.features`: the machinable features of a part, found on the exact B-rep of its bodies
//! (never on a mesh): round holes along the tool axis (through or blind, counterbores and
//! countersinks), closed pockets (a planar floor walled all round, its islands and the smallest
//! radius of its inside corners; or none, an opening through the part) and the planar faces
//! facing the tool, the pockets' floors among them.
//!
//! Everything is read from the record: a face's analytic surface (`face_surface`), its loops of
//! oriented edges and the faces on the other side of each edge, the edges' exact curves (lines,
//! circles through `edge_circle`, the control nets that bound a spline) and the loops read in a
//! plane by `Body::planar_edges` (lines and arcs exactly). Whether a round face is a hole or a
//! boss is read from the planar faces at its ends (in an inner loop with the wall going into the
//! material, or a floor's outer loop), else by classifying a point beside it.
//!
//! Each feature gets an id (`hole:0`, `pocket:1`, `face:2`…), numbered per kind in a fixed order
//! (from the top down, then by position), so the same part and the same parameters give the same
//! ids. `cam.job` operations name them through [`Lazy`], which finds the features the way
//! `cam.features` does with the job's bodies, origin and tolerance.

use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::{PI, TAU};
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

use serde_json::Value;
use solidcore_record::brep::{Brep, Curve, Loop as BrepLoop};
use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_cam::{Frame, Heights};
use solidcraft_geom::prelude::*;
use solidcraft_kernel::{Body, EdgeCircle, FaceSurface, PlanarPiece, PointClass, edge_circle, face_surface};

use super::{Ctx, MAX_BODY_NAMES, TOLERANCE_RANGE, finite_point, num, parse, v3};
use crate::cmd::CommandSpec;
use crate::params::bad;
use crate::schema::cam::{HeightsPartial, SideSpec};
use crate::schema::cam_features::{
    CamFeaturesParams, CamFeaturesReply, Countersink, FaceReport, HoleGroup, HoleReport, HoleThread, IslandReport, PocketReport, SetupReport,
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
        "bodies?: [names] (default every visible body); origin?: [x,y,z] (work zero, as a job's); tolerance?: mm (default 0.01) →          setups: [{tool_axis, top_z, bottom_z, bounds, features}], holes: [{id: \"hole:k\", center, center_work, diameter, top_z,          bottom_z, depth, through, floor: through|flat|cone|round, tip_z?, counterbore?, counterbore_of?, countersink?}], hole_groups,          pockets: [{id: \"pocket:k\", floor_z, top_z, depth, outline, area, min_corner_radius, sharp_corners, walls_vertical, islands,          through? (an opening through the part: a slot, a window)}], faces: [{id: \"face:k\", z, area, role:          top|plateau|step|island_top|pocket_floor|hole_floor}], notes. Read from the exact B-rep, in work coordinates (Z the tool          axis). A cam.job pocket or contour takes `feature: \"pocket:0\"` (heights default from the feature); the ids are the same          for the same bodies, origin and tolerance",
    )];

/// Most faces a body may have to be looked at.
const MAX_FACES: usize = 20_000;
/// Directions within this of each other (as 1 - cos) are the same.
const PARALLEL: f64 = 1e-9;
/// A face whose normal's Z is within this of 0 is a wall.
const SQUARE: f64 = 1e-7;
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
    let names = part_names(cmd, s, params.bodies.as_deref())?;
    let set = recognize(cmd, s, &names, v3(origin), tol)?;
    serde_json::to_value(&set.reply).map_err(|e| EngineError::Other(e.to_string()))
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
    pub frame: Frame,
    pub top: f64,
    pub bottom: f64,
}

/// What kind of outline a feature gives a 2D operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Hole,
    Pocket,
    Face,
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
        let shown: String = if id.chars().count() > 40 { format!("{}…", id.chars().take(40).collect::<String>()) } else { id.to_string() };
        Err(format!(
            "no feature `{shown}`: {known}; features are found along Z with the job's bodies, origin and tolerance (`cam.features` gives these ids)"
        ))
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
    cell: std::sync::OnceLock<std::result::Result<FeatureSet, String>>,
}

impl Lazy {
    pub fn new(bodies: Option<Vec<String>>, origin: Vec3, tol: f64) -> Lazy {
        Lazy { bodies, origin, tol, cell: std::sync::OnceLock::new() }
    }

    pub fn get(&self, cmd: &str, s: &Session) -> Result<&FeatureSet> {
        let found = self.cell.get_or_init(|| {
            let run = || -> Result<FeatureSet> {
                let names = part_names(cmd, s, self.bodies.as_deref())?;
                recognize(cmd, s, &names, self.origin, self.tol)
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
/// above it and 5 mm above the retract given. Out of order, the refusal names each height and
/// where it came from.
pub(super) fn feature_heights(setup: &Setup, top: f64, bottom: f64, given: Option<&HeightsPartial>) -> std::result::Result<Heights, String> {
    let g = given.copied().unwrap_or_default();
    let top_v = g.top.unwrap_or(top);
    let bottom_v = g.bottom.unwrap_or(bottom);
    let material = setup.top.max(top_v);
    let retract = g.retract.unwrap_or(material + 3.0);
    let clearance = g.clearance.unwrap_or((material + 10.0).max(retract + 5.0));
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

/// The regions, heights and contour side of a 2D operation on feature `id`. A pocket
/// passes its tool's radius as `facing`: on a face at the setup's top (nothing of the part above
/// it) the pocket then covers the face's extent grown by the radius and 1 mm, so the tool's edge
/// passes the face's edges instead of stopping a radius inside them.
pub(super) fn feature_cut(ctx: &Ctx<'_>, op: &str, id: &str, given: Option<&HeightsPartial>, facing: Option<f64>) -> Result<FeatureCut> {
    let set = ctx.features.get(ctx.cmd, ctx.s)?;
    let g = set.get(id).map_err(|e| ctx.bad(op, e))?;
    let Some(setup) = set.setup(g) else { return Err(ctx.bad(op, format!("feature `{id}` has no setup"))) };
    let outer = match (g.kind, &g.outer, g.hole) {
        (Kind::Hole, _, Some((center, diameter, _))) => Loop2::circle(center, diameter * 0.5),
        (_, Some(o), _) => o.clone(),
        _ => return Err(ctx.bad(op, format!("`{id}` has no outline for a 2D operation"))),
    };
    let heights = feature_heights(setup, g.top, g.bottom, given).map_err(|e| ctx.bad(op, e))?;
    if !(heights.top > heights.bottom + 1e-9) {
        let why = if g.flat_top {
            format!(
                "`{id}` is a face at Z {} with nothing above it in the model: give `heights.top` (the stock's top) to face it, or use its \
                 pocket",
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
    Ok(FeatureCut { regions: vec![Region2 { outer, holes }], heights, side: SideSpec::Inside })
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
        let cut = feature_cut(ctx, op, id, heights, facing)?;
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

/// Points round a hole across a turned part, halfway along it, that must all be in material.
const CROSS_HOLE_SAMPLES: usize = 24;

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

/// Points across a flat face tested for material over them (a grid of this many a side).
const COVER_GRID: usize = 5;
/// Points up a column over a flat face classified against a curved face that may hang over it.
const COVER_COLUMN: usize = 64;

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

struct Found {
    reply: CamFeaturesReply,
    geo: Vec<(String, Geo)>,
}

#[derive(Default)]
struct Lists {
    holes: Vec<(Vec<i64>, HoleReport, Geo)>,
    pockets: Vec<(Vec<i64>, PocketReport, Geo, usize)>,
    faces: Vec<(Vec<i64>, FaceReport, Geo)>,
    /// Counterbore pairs: (larger hole, smaller hole) as indices into `holes`.
    counterbores: Vec<(usize, usize)>,
}

/// Every feature of the bodies `names` of the design, seen down work Z.
pub(crate) fn recognize(cmd: &str, s: &Session, names: &[String], origin: Vec3, tol: f64) -> Result<FeatureSet> {
    let st = s.world_state()?;
    recognize_in(cmd, &st, names, origin, tol)
}

/// [`recognize`] on the bodies of `st`.
pub(crate) fn recognize_in(cmd: &str, st: &solidcraft_doc::ModelState, names: &[String], origin: Vec3, tol: f64) -> Result<FeatureSet> {
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

    // The setup: the tool down work Z, and the part's extent in it.
    let mut setups = vec![Setup { frame: Frame::IDENTITY, top: 0.0, bottom: 0.0 }];
    let mut setup_bounds: Vec<[[f64; 2]; 2]> = Vec::with_capacity(setups.len());
    for su in &mut setups {
        let (mut zr, mut xr, mut yr) = (None, None, None);
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
            }
        }
        let (zlo, zhi) = zr.unwrap_or((0.0, 0.0));
        su.top = r9(zhi);
        su.bottom = r9(zlo);
        let ((xl, xh), (yl, yh)) = (xr.unwrap_or_default(), yr.unwrap_or_default());
        setup_bounds.push([[xl, yl], [xh, yh]]);
    }

    let found = find(&topos, &setups, tol);
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
    reply.origin = r3(origin);
    reply.bodies = names.to_vec();
    reply.tolerance = tol;
    reply.notes.splice(0..0, notes);
    reply.setups = setups
        .iter()
        .zip(&setup_bounds)
        .enumerate()
        .map(|(i, (su, b))| SetupReport {
            tool_axis: r3(su.frame.z),
            top_z: su.top,
            bottom_z: su.bottom,
            bounds: rb(*b),
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

fn find(topos: &[Topo<'_>], setups: &[Setup], tol: f64) -> Found {
    let mut l = Lists::default();
    // Faces claimed by a feature (body, face), so they are listed once.
    let mut claimed: BTreeSet<(usize, usize)> = BTreeSet::new();
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
                    continue;
                }
                let up = if along > 0.0 { c.axis } else { -c.axis };
                let circles = cyl_circles(t, c, up, tol);
                let across = if full(&circles, tol) || !circles.is_empty() { None } else { curved_through(t, c, up, tol) };
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
                let open_flags: Vec<bool> = if outer_lie == Lie::Up {
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
                };
                let face = FaceReport {
                    id: String::new(),
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
                l.faces.push((key.clone(), face, geo.clone()));
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
                    };
                    claimed.extend(lp.beside.iter().map(|g| (bi, *g)));
                    let (cx, cy) = ((bb[0][0] + bb[1][0]) * 0.5, (bb[0][1] + bb[1][1]) * 0.5);
                    // After the setup's pockets with a floor, so their ids stay as they were.
                    let key = vec![si as i64, i64::MAX, q(-z, tol), q(cy, tol), q(cx, tol), bi as i64, f as i64];
                    l.pockets.push((key, pocket, g, usize::MAX));
                }
                setup_faces.push((face_index, outer.beside.clone()));
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
        }
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
        let same = reply
            .hole_groups
            .iter_mut()
            .find(|g| (g.diameter - h.diameter).abs() <= tol.max(1e-9) && (g.depth - h.depth).abs() <= tol.max(1e-9) && g.through == h.through);
        match same {
            Some(g) => {
                g.count += 1;
                g.ids.push(h.id.clone());
            }
            None => reply.hole_groups.push(HoleGroup { diameter: h.diameter, depth: h.depth, through: h.through, count: 1, ids: vec![h.id.clone()] }),
        }
    }

    // Faces first, so pockets can name them.
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
    let mut faces: Vec<Option<(Vec<i64>, FaceReport, Geo)>> = l.faces.into_iter().map(Some).collect();
    for (k, i) in face_order.iter().enumerate() {
        let Some(Some((_, mut f, g))) = faces.get_mut(*i).map(Option::take) else { continue };
        f.id = format!("face:{k}");
        f.pocket = pocket_of_face.get(i).cloned();
        geo.push((f.id.clone(), g));
        reply.faces.push(f);
    }
    Found { reply, geo }
}
