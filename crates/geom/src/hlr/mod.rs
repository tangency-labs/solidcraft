//! Exact orthographic hidden lines for placed portable B-rep bodies (Drawing W06a, #2455;
//! plan #860). Edges and face silhouettes are projected from their exact curves, split where
//! their projections cross, and each piece is classified by rays toward the viewer against the
//! trimmed faces. Every piece is visible, hidden or explicitly uncertain; nothing is guessed.
//!
//! Supported: planes, and cylinders, cones, spheres and tori whether stored as analytic records,
//! rigidly placed revolutions of lines and circles or spline patches (recognised to 1e-9 relative
//! and then trimmed and intersected in their own chart). Other spline and rigidly placed revolution
//! faces are freeform:
//! unless [`Options::freeform`] is set they are listed in [`Drawing::unsupported`], their
//! silhouettes are not drawn and anything they could hide is reported uncertain.
//!
//! Work is done in a frame centred on the scene, so results do not drift for parts far from the
//! origin. Output curves are `Seg2` pieces in screen millimetres with the accuracy vocabulary of
//! [`crate::section`]. See `docs/drawing-hlr.md` for the contract and its measured limits.

pub(crate) mod curve;
mod silhouette;
pub(crate) mod surface;
mod trim;

use serde::Serialize;

use crate::brep::Brep;
use crate::section::CurveAccuracy;
use crate::{Seg2, Vec2, Vec3};
use curve::{C3, as_circle};
use surface::{Analytic, Hit, S3};
use trim::Trim;

#[derive(Debug, thiserror::Error)]
pub enum HlrError {
    #[error("not supported yet: {0}")]
    Unsupported(&'static str),
    #[error("hidden lines: {0} budget exceeded")]
    Budget(&'static str),
    #[error("hidden lines: cancelled")]
    Cancelled,
    #[error("hidden lines: invalid {0}")]
    Invalid(&'static str),
}

/// An orthographic view: `d` is the direction of sight; screen coordinates are `(p.x, p.y)`.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub x: Vec3,
    pub y: Vec3,
    pub d: Vec3,
}

impl View {
    pub fn new(d: Vec3, up: Vec3) -> Result<View, HlrError> {
        let d = d.normalized().ok_or(HlrError::Invalid("view direction"))?;
        let x = d.cross(up).normalized().ok_or(HlrError::Invalid("view up direction"))?;
        Ok(View { x, y: x.cross(d), d })
    }
    pub fn screen(&self, p: Vec3) -> [f64; 2] {
        [p.dot(self.x), p.dot(self.y)]
    }
}

/// One placed body: `world = rotation * local + translation`, rotation columns given.
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    pub body: usize,
    pub rotation: [Vec3; 3],
    pub translation: Vec3,
}

impl Placement {
    fn apply(&self, p: Vec3) -> Vec3 {
        let [a, b, c] = self.rotation;
        a * p.x + b * p.y + c * p.z + self.translation
    }
    fn local_dir(&self, v: Vec3) -> Vec3 {
        let [a, b, c] = self.rotation;
        Vec3::new(a.dot(v), b.dot(v), c.dot(v))
    }
    fn local(&self, p: Vec3) -> Vec3 {
        self.local_dir(p - self.translation)
    }
    fn rigid(&self) -> bool {
        let [a, b, c] = self.rotation;
        [a, b, c].iter().all(|v| v.is_finite() && (v.len() - 1.).abs() < 1e-9)
            && a.dot(b).abs() < 1e-9
            && a.dot(c).abs() < 1e-9
            && b.dot(c).abs() < 1e-9
            && (a.cross(b).dot(c) - 1.).abs() < 1e-9
            && self.translation.is_finite()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Sharp,
    /// Between two faces that meet tangentially (or lie on one surface).
    Tangent,
    /// Used twice by one face, or between two faces of one analytic surface (a patch boundary).
    Seam,
}

struct EdgeData {
    curve: C3,
    acc: CurveAccuracy,
    faces: Vec<usize>,
    kind: EdgeKind,
}

struct FaceData {
    surface: S3,
    analytic: Analytic,
    /// Faces of this body on the same quadric or torus (patches of one surface), itself included.
    group: Vec<usize>,
    trim: Trim,
    lo: Vec3,
    hi: Vec3,
    /// Neither a plane nor a recognised quadric or torus.
    freeform: bool,
}

/// A body prepared once and shared by all of its placements.
pub struct Prepared {
    edges: Vec<EdgeData>,
    faces: Vec<FaceData>,
    /// The body's records are kept moved by `-offset` (to its own centre), so its coordinates
    /// stay small wherever it was modelled; placements add it back.
    offset: Vec3,
    lo: Vec3,
    hi: Vec3,
    pub report: PrepareReport,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct PrepareReport {
    pub faces: usize,
    pub edges: usize,
    /// Face counts by recognised surface: plane, cylinder, cone, sphere, torus, other.
    pub surfaces: std::collections::BTreeMap<String, usize>,
    /// Edge counts by record kind and recognition, e.g. `rational_spline/circle`.
    pub curves: std::collections::BTreeMap<String, usize>,
    pub tangent_edges: usize,
    pub seam_edges: usize,
    /// Faces whose trim could not be classified, with the reason.
    pub unsupported_trims: Vec<(usize, String)>,
    /// Largest distance between a boundary curve and the chords used for trimming (mm).
    pub max_trim_chord: f64,
    /// Faces outside the supported subset (see [`Options::freeform`]).
    pub freeform_faces: Vec<usize>,
}

/// Whether two recognised surfaces are the same quadric or torus (planes never: coplanar
/// faces of a split are real edges in the record, and are drawn).
fn same_analytic(a: &Analytic, b: &Analytic, tol: f64) -> bool {
    let close = |x: Vec3, y: Vec3, scale: f64| x.dist(y) <= tol * (1. + scale);
    let on_axis = |p: Vec3, q: Vec3, a: Vec3, scale: f64| (q - p).cross(a).len() <= tol * (1. + scale);
    match (a, b) {
        (Analytic::Cylinder { p, a, r }, Analytic::Cylinder { p: q, a: b, r: s }) => {
            (r - s).abs() <= tol * r && a.cross(*b).len() <= tol && on_axis(*p, *q, *a, *r)
        }
        (Analytic::Cone { apex, a, half }, Analytic::Cone { apex: b_apex, a: b, half: h }) => {
            // The axes by their angle, as for cylinders and tori (a dot product would compare
            // its square).
            (half - h).abs() <= tol && a.cross(*b).len() <= tol && a.dot(*b) > 0. && close(*apex, *b_apex, apex.len())
        }
        (Analytic::Sphere { c, r }, Analytic::Sphere { c: d, r: s }) => (r - s).abs() <= tol * r && close(*c, *d, *r),
        (Analytic::Torus { c, a, major, minor }, Analytic::Torus { c: d, a: b, major: m, minor: n }) => {
            (major - m).abs() <= tol * major && (minor - n).abs() <= tol * major && a.cross(*b).len() <= tol && close(*c, *d, *major)
        }
        _ => false,
    }
}

fn analytic_name(a: &Analytic) -> &'static str {
    match a {
        Analytic::Plane => "plane",
        Analytic::Cylinder { .. } => "cylinder",
        Analytic::Cone { .. } => "cone",
        Analytic::Sphere { .. } => "sphere",
        Analytic::Torus { .. } => "torus",
        Analytic::Other => "other",
    }
}

pub fn prepare(b: &Brep, opts: &Options) -> Result<Prepared, HlrError> {
    prepare_cancellable(b, opts, &|| false)
}

/// [`prepare`], polling `cancel` between edges, faces and surface comparisons; when it returns
/// true the call ends with `HlrError::Cancelled`.
pub fn prepare_cancellable(b: &Brep, opts: &Options, cancel: &dyn Fn() -> bool) -> Result<Prepared, HlrError> {
    if !(1..=MAX_TRIM_SAMPLES).contains(&opts.trim_samples) {
        return Err(HlrError::Invalid("trim samples"));
    }
    b.validate().map_err(|_| HlrError::Invalid("portable B-rep"))?;
    // Work on the body moved to its own centre: rounding in ray and inversion formulas grows
    // with the coordinates, and a part may be modelled far from the origin (W05a finding 7).
    let offset = anchor(b);
    let moved = shifted(b, offset);
    let b = &moved;
    let mut edges = Vec::new();
    let mut faces = Vec::new();
    let mut report = PrepareReport::default();
    let mut curves = Vec::with_capacity(b.edges.len());
    for (ei, e) in b.edges.iter().enumerate() {
        if ei % 256 == 0 && cancel() {
            return Err(HlrError::Cancelled);
        }
        let (c, acc) = C3::of_edge(e)?;
        let record = match &e.curve {
            crate::brep::Curve::Line { .. } => "line",
            crate::brep::Curve::Circle { .. } => "circle_arc",
            crate::brep::Curve::Ellipse { .. } => "ellipse_arc",
            crate::brep::Curve::Polyline { .. } => "polyline",
            crate::brep::Curve::Spline { .. } => "spline",
            crate::brep::Curve::RationalSpline { .. } => "rational_spline",
            // Only one with a spline to follow gets here: what was an `intersection` record.
            crate::brep::Curve::SurfaceIntersection { .. } => "intersection",
        };
        let shape = if matches!(c, C3::Line { .. }) {
            "line"
        } else if as_circle(&c, 1e-9).is_some() {
            "circle"
        } else {
            "other"
        };
        *report.curves.entry(format!("{record}/{shape}")).or_insert(0) += 1;
        curves.push((c, acc));
    }
    let local_curves: Vec<C3> = curves.iter().map(|(c, _)| c.clone()).collect();
    let mut users: Vec<Vec<usize>> = vec![Vec::new(); b.edges.len()];
    // Faces and edges by their numbers in the record. Validation puts every face in a shell; its
    // loops are stored the same whichever side a shell uses, and hidden lines look at both, so the
    // shells' orientation does not come into it (a face two shells share is one face). An edge no
    // face uses (a wire) is drawn like any other.
    for (fi, f) in b.faces.iter().enumerate() {
        if cancel() {
            return Err(HlrError::Cancelled);
        }
        let mut surface = S3::of_face(f)?;
        // A flat spline patch is a plane: trimmed and intersected as one.
        if let Some(plane) = surface.flat(1e-9) {
            surface = plane;
        }
        let analytic = surface.analytic(1e-9);
        // Trim and intersect recognised spline surfaces in their own analytic chart.
        if matches!(surface, S3::Nurbs(_)) {
            let axis = match analytic {
                Analytic::Cylinder { p, a, .. } => Some((p, a)),
                Analytic::Cone { apex, a, .. } => Some((apex, a)),
                _ => None,
            };
            let mut h = [0., 0.];
            if let Some((p, a)) = axis {
                let along = trim::uses(f).filter_map(|u| local_curves.get(u.edge as usize)).flat_map(|c| {
                    let [t0, t1] = c.domain();
                    (0..=64).map(move |i| c.at(t0 + (t1 - t0) * i as f64 / 64.))
                });
                let (lo, hi) = along.fold((f64::MAX, f64::MIN), |(lo, hi), q| ((q - p).dot(a).min(lo), (q - p).dot(a).max(hi)));
                // Generous: the chart only has to contain the face; the trim bounds it exactly.
                let pad = 0.01 * (hi - lo).abs() + 1e-6;
                h = [lo - pad, hi + pad];
            }
            let toward = match analytic {
                Analytic::Sphere { c, .. } => {
                    let pts: Vec<Vec3> = trim::uses(f)
                        .filter_map(|u| local_curves.get(u.edge as usize))
                        .map(|c| c.at(0.5 * (c.domain()[0] + c.domain()[1])))
                        .collect();
                    let n = pts.len().max(1) as f64;
                    pts.iter().fold(Vec3::ZERO, |acc, p| acc + *p) / n - c
                }
                _ => Vec3::ZERO,
            };
            if let Some(chart) = S3::from_analytic(&analytic, h, toward) {
                surface = chart;
            }
        }
        *report.surfaces.entry(analytic_name(&analytic).to_string()).or_insert(0) += 1;
        let trim = trim::build(f, &local_curves, &surface, opts.trim_samples);
        if let Some(why) = trim.unsupported {
            report.unsupported_trims.push((fi, why.to_string()));
        }
        report.max_trim_chord = report.max_trim_chord.max(trim.chord_error);
        let (mut lo, mut hi) = (trim.lo, trim.hi);
        // Curved faces can bulge past their boundary: bound them by the whole surface's
        // guaranteed box (spline hull, turned profile), clipped to nothing tighter.
        if !matches!(surface, S3::Plane { .. })
            && let Some((blo, bhi)) = surface.bound()
        {
            lo = lo.min(blo);
            hi = hi.max(bhi);
        }
        // Every face is padded, planes too: a plane record and its edges may disagree by
        // rounding, and a box flat along the normal would cull the plane's own hit. Rounding
        // grows with where the body was modelled (its offset), not with the face's size.
        let pad = (hi - lo).len() * 1e-6 + 1e-12 * offset.x.abs().max(offset.y.abs()).max(offset.z.abs());
        lo -= Vec3::new(pad, pad, pad);
        hi += Vec3::new(pad, pad, pad);
        for u in trim::uses(f) {
            if let Some(list) = users.get_mut(u.edge as usize) {
                list.push(fi);
            }
        }
        let freeform = matches!(analytic, Analytic::Other);
        if freeform {
            report.freeform_faces.push(fi);
        }
        faces.push(FaceData { surface, analytic, group: Vec::new(), trim, lo, hi, freeform });
    }
    for (ei, (c, acc)) in curves.into_iter().enumerate() {
        let list = users.get(ei).cloned().unwrap_or_default();
        let mut distinct = list.clone();
        distinct.sort_unstable();
        distinct.dedup();
        let kind = if list.len() >= 2 && distinct.len() == 1 {
            EdgeKind::Seam
        } else if let [f1, f2] = distinct.as_slice() {
            let [t0, t1] = c.domain();
            let normals_parallel = [0.25, 0.5, 0.75].iter().all(|k| {
                let m = c.at(t0 + (t1 - t0) * k);
                let n = |f: usize| faces.get(f).and_then(|fd: &FaceData| fd.surface.inverse(m, None).and_then(|uv| fd.surface.normal(uv)));
                n(*f1).zip(n(*f2)).is_some_and(|(a, b)| a.cross(b).len() < 1e-6)
            });
            let same_surface = match (faces.get(*f1).map(|f: &FaceData| f.analytic), faces.get(*f2).map(|f: &FaceData| f.analytic)) {
                (Some(a), Some(b)) => same_analytic(&a, &b, 1e-9),
                _ => false,
            };
            if same_surface {
                EdgeKind::Seam
            } else if normals_parallel {
                EdgeKind::Tangent
            } else {
                EdgeKind::Sharp
            }
        } else {
            EdgeKind::Sharp
        };
        match kind {
            EdgeKind::Tangent => report.tangent_edges += 1,
            EdgeKind::Seam => report.seam_edges += 1,
            EdgeKind::Sharp => {}
        }
        edges.push(EdgeData { curve: c, acc, faces: list, kind });
    }
    // Faces on one quadric or torus. Only surfaces of one kind whose size (radius, half angle,
    // major radius) agrees to the tolerance can match, so each face is compared with the faces
    // in its window of the sorted sizes, not with every face.
    let size = |a: &Analytic| match a {
        Analytic::Cylinder { r, .. } => Some((0u8, *r)),
        Analytic::Cone { half, .. } => Some((1, *half)),
        Analytic::Sphere { r, .. } => Some((2, *r)),
        Analytic::Torus { major, .. } => Some((3, *major)),
        Analytic::Plane | Analytic::Other => None,
    };
    let analytics: Vec<Analytic> = faces.iter().map(|f| f.analytic).collect();
    let mut sorted: Vec<(u8, f64, usize)> = analytics.iter().enumerate().filter_map(|(j, a)| size(a).map(|(k, v)| (k, v, j))).collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let mut compared = 0usize;
    for (i, f) in faces.iter_mut().enumerate() {
        if i % 256 == 0 && cancel() {
            return Err(HlrError::Cancelled);
        }
        let mut group = vec![i];
        if let Some((kind, v)) = size(&f.analytic) {
            // Wider than any match allows: same_analytic takes sizes within 1e-9 of the size
            // (relative for lengths, absolute for the half angle).
            let reach = 2e-9 * v.abs().max(1.);
            let first = sorted.partition_point(|c| c.0 < kind || (c.0 == kind && c.1 < v - reach));
            for (_, _, j) in sorted.get(first..).unwrap_or_default().iter().take_while(|c| c.0 == kind && c.1 <= v + reach) {
                compared += 1;
                if compared > 50_000_000 {
                    return Err(HlrError::Budget("surface group"));
                }
                if compared.is_multiple_of(65536) && cancel() {
                    return Err(HlrError::Cancelled);
                }
                if *j != i && analytics.get(*j).is_some_and(|b| same_analytic(&f.analytic, b, 1e-9)) {
                    group.push(*j);
                }
            }
        }
        group.sort_unstable();
        f.group = group;
    }
    report.faces = faces.len();
    report.edges = edges.len();
    let (lo, hi) =
        faces.iter().fold((Vec3::new(f64::MAX, f64::MAX, f64::MAX), Vec3::new(f64::MIN, f64::MIN, f64::MIN)), |(a, b), f| (a.min(f.lo), b.max(f.hi)));
    if faces.is_empty() || !lo.is_finite() || !hi.is_finite() {
        return Err(HlrError::Invalid("empty or unbounded body"));
    }
    Ok(Prepared { edges, faces, offset, lo, hi, report })
}

/// The centre of the body's vertices' box (a validated body has vertices: every face has an edge
/// loop, every other shell an edge or a vertex).
fn anchor(b: &Brep) -> Vec3 {
    let mut pts = b.vertices.iter().map(|v| Vec3::new(v.point[0], v.point[1], v.point[2]));
    let Some(first) = pts.next() else { return Vec3::ZERO };
    let (lo, hi) = pts.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p)));
    let c = (lo + hi) * 0.5;
    if c.is_finite() { c } else { Vec3::ZERO }
}

/// The body's records with every position moved by `-d`: points, control points and NURBS forms,
/// a placed revolution through its matrix; regions, parameters and pcurves as they are.
fn shifted(b: &Brep, d: Vec3) -> Brep {
    b.moved(&crate::brep::placement::Rigid { rotation: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]], translation: [-d.x, -d.y, -d.z] })
}

#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Draw edges between tangent faces (seams are never drawn).
    pub tangent_edges: bool,
    /// Also handle freeform faces: contoured silhouettes and seeded ray tests, slow and
    /// ill-conditioned where a face is seen nearly edge-on (W05a finding 6). Off by default.
    pub freeform: bool,
    /// Boundary samples per curve piece when trimming faces, 1 to [`MAX_TRIM_SAMPLES`].
    pub trim_samples: usize,
    pub max_tracks: usize,
    pub max_rays: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options { tangent_edges: true, freeform: false, trim_samples: 256, max_tracks: 200_000, max_rays: 20_000_000 }
    }
}

/// Largest [`Options::trim_samples`].
pub const MAX_TRIM_SAMPLES: usize = 4096;
/// Largest coordinate of a placed body in the scene, in millimetres: the bound `geom::section`
/// and `geom::brep` put on stored numbers. Beyond it the scene's size, and every tolerance
/// taken from it, would lose its meaning or overflow.
pub const MAX_COORDINATE: f64 = 1e12;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Visible,
    Hidden,
    Uncertain,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    Edge {
        edge: usize,
        tangent: bool,
    },
    /// The silhouette of the faces on one surface; `face` is the first of them.
    Silhouette {
        face: usize,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct Piece {
    /// Parameter range on the source curve.
    pub t: [f64; 2],
    pub visibility: Visibility,
    /// Drawn by another piece: a hidden or uncertain piece lying on a visible one (hidden lines
    /// never show through visible ones), or a duplicate of a visible or hidden piece.
    pub covered: bool,
    /// The piece on screen (millimetres): lines and conic arcs where the projection is exact,
    /// chords otherwise.
    pub geometry: Vec<Seg2>,
    pub accuracy: CurveAccuracy,
    /// Exact points along the piece, joined by `geometry`'s chords where it is sampled.
    #[serde(skip)]
    points: Vec<[f64; 2]>,
    /// Largest distance between the projected curve and the chords joining `points`.
    #[serde(skip)]
    sag: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Line {
    /// Index into the scene's placements.
    pub placement: usize,
    pub source: Source,
    pub pieces: Vec<Piece>,
}

/// A face left out of the supported subset, and why.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Unsupported {
    pub placement: usize,
    pub face: usize,
    pub reason: &'static str,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Stats {
    pub tracks: usize,
    pub silhouettes: usize,
    pub crossings: usize,
    pub pieces: usize,
    pub uncertain_pieces: usize,
    /// Uncertain pieces not covered by a coincident visible piece.
    pub uncertain_uncovered: usize,
    pub covered_pieces: usize,
    /// Visibility changes found inside a piece by sampling, i.e. not at a computed crossing.
    pub unexpected_transitions: usize,
    pub rays: usize,
    pub surface_ray_tests: usize,
    /// Milliseconds per phase: curves and silhouettes, crossings, classification, coverage.
    /// Zero where the platform has no clock (`wasm32-unknown-unknown`).
    pub phase_ms: [f64; 4],
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Drawing {
    pub lines: Vec<Line>,
    /// Faces outside the supported subset: their silhouettes are missing and what they could
    /// hide is uncertain.
    pub unsupported: Vec<Unsupported>,
    pub stats: Stats,
}

enum TrackCurve<'a> {
    Edge(&'a C3),
    Owned(C3),
}

struct Track<'a> {
    placement: usize,
    curve: TrackCurve<'a>,
    range: [f64; 2],
    source: Source,
    acc: CurveAccuracy,
    own: Vec<usize>,
    /// Own-face hits closer than this are the curve's own surface: a sampled silhouette's
    /// chord lies up to its deviation inside the surface, and a ray tangent there leaves it
    /// again after about sqrt(2 * radius * deviation).
    own_eps: f64,
    samples: Vec<(f64, [f64; 2])>,
    /// Per sample chord: the distance of the exact projected curve from it at its middle.
    sags: Vec<f64>,
    splits: Vec<f64>,
}

impl Track<'_> {
    fn c3(&self) -> &C3 {
        match &self.curve {
            TrackCurve::Edge(c) => c,
            TrackCurve::Owned(c) => c,
        }
    }
}

struct Ctx<'a> {
    defs: &'a [Prepared],
    scene: &'a [Placement],
    view: View,
    eps_own: f64,
    eps_other: f64,
    scale: f64,
    /// World boxes of placements, screen min/max and depth min.
    boxes: Vec<([f64; 2], [f64; 2], f64)>,
    rays: std::cell::Cell<usize>,
    surface_tests: std::cell::Cell<usize>,
    transitions: std::cell::Cell<usize>,
    max_rays: usize,
    freeform: bool,
    cancel: &'a dyn Fn() -> bool,
}

fn context<'a>(defs: &'a [Prepared], scene: &'a [Placement], view: &View, opts: &Options, cancel: &'a dyn Fn() -> bool) -> Result<Ctx<'a>, HlrError> {
    let mut boxes = Vec::with_capacity(scene.len());
    let (mut wlo, mut whi) = (Vec3::new(f64::MAX, f64::MAX, f64::MAX), Vec3::new(f64::MIN, f64::MIN, f64::MIN));
    for p in scene {
        if !p.rigid() {
            return Err(HlrError::Invalid("placement (not a rotation and translation)"));
        }
        let def = defs.get(p.body).ok_or(HlrError::Invalid("placement body index"))?;
        let (mut slo, mut shi, mut dmin) = ([f64::MAX; 2], [f64::MIN; 2], f64::MAX);
        for k in 0..8 {
            let c = Vec3::new(
                if k & 1 == 0 { def.lo.x } else { def.hi.x },
                if k & 2 == 0 { def.lo.y } else { def.hi.y },
                if k & 4 == 0 { def.lo.z } else { def.hi.z },
            );
            let w = p.apply(c);
            wlo = wlo.min(w);
            whi = whi.max(w);
            let s = view.screen(w);
            slo = [slo[0].min(s[0]), slo[1].min(s[1])];
            shi = [shi[0].max(s[0]), shi[1].max(s[1])];
            dmin = dmin.min(w.dot(view.d));
        }
        boxes.push((slo, shi, dmin));
    }
    let scale = if scene.is_empty() { 1. } else { (whi - wlo).len().max(1e-6) };
    Ok(Ctx {
        defs,
        scene,
        view: *view,
        eps_own: 1e-6 * scale,
        // Hits on other bodies closer than rounding are ignored, the rest up to eps_own are
        // uncertain: the work is done in a frame at the scene's centre, so rounding is about
        // 1e-16 of the coordinates, far below 1e-12 of the scene.
        eps_other: 1e-12 * scale,
        scale,
        boxes,
        rays: std::cell::Cell::new(0),
        surface_tests: std::cell::Cell::new(0),
        transitions: std::cell::Cell::new(0),
        max_rays: opts.max_rays,
        freeform: opts.freeform,
        cancel,
    })
}

pub fn hidden_lines(defs: &[Prepared], scene: &[Placement], view: &View, opts: &Options, cancel: &dyn Fn() -> bool) -> Result<Drawing, HlrError> {
    // Work in a frame centred on the scene: coordinates stay small, so rounding does not grow
    // with the distance from the origin (W05a finding 7). Results are shifted back at the end.
    let origin = scene_centre(defs, scene)?;
    // Each body's own offset (see `Prepared`) joins its placement here.
    let local: Vec<Placement> =
        scene.iter().map(|p| Placement { translation: p.apply(defs.get(p.body).map_or(Vec3::ZERO, |d| d.offset)) - origin, ..*p }).collect();
    let scene = local.as_slice();
    let shift = view.screen(origin);
    let ctx = context(defs, scene, view, opts, cancel)?;
    let scale = ctx.scale;
    let mut stats = Stats::default();
    let mut unsupported = Vec::new();
    let clock = Clock::start();
    // 1. Tracks: drawable edges and trimmed silhouettes of every placement.
    let mut tracks: Vec<Track> = Vec::new();
    for (pi, p) in scene.iter().enumerate() {
        if cancel() {
            return Err(HlrError::Cancelled);
        }
        let Some(def) = defs.get(p.body) else { continue };
        // A body's records were rounded where it was modelled: a plane may stand an ulp of
        // those coordinates off its edges, which matters for a part far smaller than its
        // distance from the origin.
        let far = 1e-14 * def.offset.x.abs().max(def.offset.y.abs()).max(def.offset.z.abs());
        for (ei, e) in def.edges.iter().enumerate() {
            if e.kind == EdgeKind::Seam || (e.kind == EdgeKind::Tangent && !opts.tangent_edges) {
                continue;
            }
            tracks.push(Track {
                placement: pi,
                curve: TrackCurve::Edge(&e.curve),
                range: e.curve.domain(),
                source: Source::Edge { edge: ei, tangent: e.kind == EdgeKind::Tangent },
                acc: e.acc,
                own: e.faces.clone(),
                // Skip the ray's own start on its faces: rounding grows with the coordinates
                // (1e-9 of the scene), the margin with the faces themselves (1e-6 of their
                // size), never with the whole scene, so small parts in large scenes keep their
                // self-occlusion.
                own_eps: {
                    let size = e.faces.iter().filter_map(|f| def.faces.get(*f)).map(|f| (f.hi - f.lo).len()).fold(0., f64::max);
                    let base = (1e-9 * ctx.scale).max(1e-6 * size).max(far);
                    match e.acc {
                        CurveAccuracy::Bounded { max_deviation_mm: tol } => base.max(10. * tol),
                        _ => base,
                    }
                },
                samples: Vec::new(),
                sags: Vec::new(),
                splits: Vec::new(),
            });
        }
        let dl = p.local_dir(view.d);
        let supported = |f: &FaceData| !(f.freeform && !opts.freeform) && f.trim.unsupported.is_none();
        // Faces on one surface (a sphere in two halves, say) share their silhouettes, keyed by
        // the set's first face: one curve along their seams, not one per face.
        let mut sets: std::collections::BTreeMap<usize, Vec<usize>> = std::collections::BTreeMap::new();
        for (fi, f) in def.faces.iter().enumerate() {
            if f.freeform && !opts.freeform {
                unsupported.push(Unsupported { placement: pi, face: fi, reason: "freeform face" });
                continue;
            }
            if let Some(reason) = f.trim.unsupported {
                unsupported.push(Unsupported { placement: pi, face: fi, reason });
                continue;
            }
            if matches!(f.analytic, Analytic::Plane) {
                continue;
            }
            // The set's silhouette is traced on its first face's surface, so a face joins only
            // when its recognised surface is that one to rounding (1e-13); recognition groups
            // surfaces more loosely (1e-9), and two surfaces that close can still differ inside
            // the faces (two spherical caps through one circle).
            let first = f.group.iter().copied().filter(|j| def.faces.get(*j).is_some_and(supported)).min().unwrap_or(fi);
            let fits = def.faces.get(first).is_some_and(|l| same_analytic(&l.analytic, &f.analytic, 1e-13));
            sets.entry(if fits { first } else { fi }).or_default().push(fi);
        }
        for (fi, members) in sets {
            let Some(f) = def.faces.get(fi) else { continue };
            let parts: Vec<(&S3, &Trim)> = members.iter().filter_map(|j| def.faces.get(*j)).map(|m| (&m.surface, &m.trim)).collect();
            let size = members.iter().filter_map(|j| def.faces.get(*j)).map(|m| (m.hi - m.lo).len()).fold(0., f64::max);
            let mut sils = Vec::new();
            silhouette::silhouettes(&parts, f.analytic, dl, &mut sils);
            for s in sils {
                for r in &s.ranges {
                    stats.silhouettes += 1;
                    tracks.push(Track {
                        placement: pi,
                        curve: TrackCurve::Owned(s.curve.clone()),
                        range: *r,
                        source: Source::Silhouette { face: fi },
                        acc: s.acc,
                        own: f.group.clone(),
                        own_eps: match s.acc {
                            CurveAccuracy::Sampled { max_sample_deviation_mm: dev } => {
                                (1e-9 * ctx.scale).max(1e-6 * size).max(far).max(2. * (2. * size * dev).sqrt())
                            }
                            _ => (1e-9 * ctx.scale).max(1e-6 * size).max(far),
                        },
                        samples: Vec::new(),
                        sags: Vec::new(),
                        splits: Vec::new(),
                    });
                }
            }
        }
        if tracks.len() > opts.max_tracks {
            return Err(HlrError::Budget("curve"));
        }
    }
    stats.tracks = tracks.len();
    // 2. Screen samples.
    for (ti, t) in tracks.iter_mut().enumerate() {
        if ti % 64 == 0 && cancel() {
            return Err(HlrError::Cancelled);
        }
        let Some(p) = scene.get(t.placement) else { continue };
        let c = t.c3();
        let mut ts: Vec<f64> = c.breaks().into_iter().filter(|x| *x > t.range[0] && *x < t.range[1]).collect();
        ts.push(t.range[0]);
        ts.push(t.range[1]);
        ts.sort_by(f64::total_cmp);
        let per = match c {
            C3::Line { .. } => 1,
            C3::Polyline(_) => 1,
            _ => 32,
        };
        let mut params = Vec::new();
        for w in ts.windows(2) {
            let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
            for i in 0..per {
                params.push(a + (b - a) * i as f64 / per as f64);
            }
        }
        params.push(t.range[1]);
        let screen = |x: f64| view.screen(p.apply(c.at(x)));
        let mut samples: Vec<(f64, [f64; 2])> = params.into_iter().map(|x| (x, screen(x))).collect();
        // Halve chords of exact curves until their sag on screen is within the target, so
        // chords stand for the curve closely enough for coincidence and crossing tests.
        if !matches!(c, C3::Line { .. } | C3::Polyline(_)) {
            let target = 1e-7 * scale.max(1.);
            for _ in 0..12 {
                let mut next = Vec::with_capacity(samples.len() * 2);
                let mut split = false;
                for w in samples.windows(2) {
                    let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
                    next.push(*a);
                    let m = 0.5 * (a.0 + b.0);
                    let q = screen(m);
                    // A spline's piece is held by its control points' hull: a parameterisation
                    // racing between samples cannot hide a swerve from the chord.
                    let hull =
                        c.hull([a.0, b.0]).map_or(0., |h| h.iter().map(|x| point_segment(view.screen(p.apply(*x)), a.1, b.1)).fold(0., f64::max));
                    if point_segment(q, a.1, b.1).max(hull) > target && next.len() < 1 << 16 {
                        next.push((m, q));
                        split = true;
                    }
                }
                if let Some(last) = samples.last() {
                    next.push(*last);
                }
                samples = next;
                if !split {
                    break;
                }
            }
        }
        // Chord sag on screen: exact for lines, measured at the middle for curves; a sampled
        // silhouette's chords stand for the true curve to within its measured deviation.
        // A fitted source record stands for the true curve to within its bound.
        let fitted = match t.acc {
            CurveAccuracy::Bounded { max_deviation_mm: tol } => tol,
            _ => 0.,
        };
        let sags = samples
            .windows(2)
            .map(|w| {
                fitted
                    + match (c, w.first(), w.get(1)) {
                        (C3::Line { .. }, _, _) => 0.,
                        (C3::Polyline(_), _, _) => match t.acc {
                            CurveAccuracy::Sampled { max_sample_deviation_mm: dev } => dev,
                            _ => 0.,
                        },
                        (_, Some(a), Some(b)) => {
                            let hull = c
                                .hull([a.0, b.0])
                                .map_or(0., |h| h.iter().map(|x| point_segment(view.screen(p.apply(*x)), a.1, b.1)).fold(0., f64::max));
                            point_segment(view.screen(p.apply(c.at(0.5 * (a.0 + b.0)))), a.1, b.1).max(hull)
                        }
                        _ => 0.,
                    }
            })
            .collect();
        t.samples = samples;
        t.sags = sags;
    }
    stats.phase_ms[0] = clock.lap();
    // 3. Crossings between projected curves (and each curve with itself).
    stats.crossings = crossings(&mut tracks, &ctx)?;
    // Faces left out (freeform or with an unsupported trim) draw no silhouettes to cut at, so
    // the curves are cut where they cross each such face's screen box instead, and below a
    // piece inside a box and behind its nearest point is uncertain: it may be hidden.
    let shadows = ShadowIndex::new(shadow_boxes(defs, scene, view, &unsupported))?;
    let mut near = Vec::new();
    let mut checks = 0usize;
    for (ti, t) in tracks.iter_mut().enumerate() {
        if shadows.list.is_empty() {
            break;
        }
        if ti % 64 == 0 && cancel() {
            return Err(HlrError::Cancelled);
        }
        let Some(pl) = scene.get(t.placement) else { continue };
        let mut cuts = Vec::new();
        for w in t.samples.windows(2) {
            let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
            // The curve's own screen box between the two samples: it can reach past the chord,
            // across it or along it (a view along an arc's plane folds it back on itself).
            let (lo, hi) = screen_box(t.c3(), [a.0, b.0], pl, view);
            shadows.query(lo, hi, &mut near);
            checks = checks.saturating_add(near.len());
            if checks > 500_000_000 {
                return Err(HlrError::Budget("shadow"));
            }
            for sh in near.iter().filter_map(|k| shadows.list.get(*k)) {
                for (k, edge) in [(0usize, sh.lo[0]), (0, sh.hi[0]), (1, sh.lo[1]), (1, sh.hi[1])] {
                    let (u, v) = (a.1[k], b.1[k]);
                    if (u - edge) * (v - edge) < 0. {
                        // Where the curve itself crosses the side, not its chord.
                        let side = |x: f64| view.screen(pl.apply(t.c3().at(x))).get(k).copied().unwrap_or(0.) - edge;
                        let (mut lo, mut hi) = (a.0, b.0);
                        for _ in 0..60 {
                            let mid = 0.5 * (lo + hi);
                            if (side(mid) < 0.) == (u - edge < 0.) {
                                lo = mid;
                            } else {
                                hi = mid;
                            }
                        }
                        cuts.push(0.5 * (lo + hi));
                    }
                }
                // A curve that enters the box where its chord does not: its stretch becomes a
                // piece of its own, judged below.
                if overlaps(lo, hi, sh) && box_gap(a.1, b.1, sh.lo, sh.hi) > 0. {
                    cuts.push(a.0);
                    cuts.push(b.0);
                }
            }
        }
        t.splits.extend(cuts);
    }
    stats.phase_ms[1] = clock.lap();
    // 4. Classify pieces between splits.
    let mut lines = Vec::with_capacity(tracks.len());
    for (ti, t) in tracks.iter().enumerate() {
        if ti % 64 == 0 && cancel() {
            return Err(HlrError::Cancelled);
        }
        let c = t.c3();
        let mut cuts = t.splits.clone();
        cuts.push(t.range[0]);
        cuts.push(t.range[1]);
        cuts.retain(|x| *x >= t.range[0] && *x <= t.range[1]);
        cuts.sort_by(f64::total_cmp);
        let span = t.range[1] - t.range[0];
        // Cuts merge when their parameters and their points are both close: nearby parameters
        // alone do not tell (a knot span narrower than 1e-10 of the range can carry most of a
        // circle), nor do equal points (a closed curve's two ends).
        cuts.dedup_by(|a, b| *a == *b || ((*a - *b).abs() <= 1e-10 * span && c.at(*a).dist(c.at(*b)) <= 1e-9 * scale.max(1.)));
        let mut pieces: Vec<Piece> = Vec::new();
        for w in cuts.windows(2) {
            let (Some(&a), Some(&b)) = (w.first(), w.get(1)) else { continue };
            for (r, vis) in classify_range(t, a, b, &ctx)? {
                let vis = if vis == Visibility::Visible && in_shadow(t, r, scene, view, &shadows) { Visibility::Uncertain } else { vis };
                match pieces.last_mut() {
                    Some(last) if last.visibility == vis && (last.t[1] - r[0]).abs() <= 1e-12 * span.max(1e-300) => last.t[1] = r[1],
                    _ => pieces.push(Piece {
                        t: r,
                        visibility: vis,
                        covered: false,
                        geometry: Vec::new(),
                        accuracy: t.acc,
                        points: Vec::new(),
                        sag: 0.,
                    }),
                }
            }
        }
        let p = scene.get(t.placement).ok_or(HlrError::Invalid("placement"))?;
        for piece in &mut pieces {
            let mut pts = vec![view.screen(p.apply(c.at(piece.t[0])))];
            pts.extend(t.samples.iter().filter(|(x, _)| *x > piece.t[0] && *x < piece.t[1]).map(|(_, s)| *s));
            pts.push(view.screen(p.apply(c.at(piece.t[1]))));
            piece.points = pts;
            piece.sag = t
                .samples
                .windows(2)
                .zip(&t.sags)
                .filter(|(w, _)| w.get(1).is_some_and(|b| b.0 > piece.t[0]) && w.first().is_some_and(|a| a.0 < piece.t[1]))
                .map(|(_, sag)| *sag)
                .fold(0., f64::max);
        }
        stats.pieces += pieces.len();
        stats.uncertain_pieces += pieces.iter().filter(|p| p.visibility == Visibility::Uncertain).count();
        lines.push(Line { placement: t.placement, source: t.source, pieces });
    }
    stats.phase_ms[2] = clock.lap();
    // Coverage is checked every 1e-4 of the scene (5 um to 0.5 mm): a gap narrower than that
    // in a coincident line is not seen.
    cover(&mut lines, 1e-7 + 1e-9 * scale, (1e-4 * scale).clamp(0.005, 0.5), cancel)?;
    stats.phase_ms[3] = clock.lap();
    for p in lines.iter().flat_map(|l| l.pieces.iter()) {
        stats.covered_pieces += usize::from(p.covered);
        stats.uncertain_uncovered += usize::from(p.visibility == Visibility::Uncertain && !p.covered);
    }
    stats.rays = ctx.rays.get();
    stats.surface_ray_tests = ctx.surface_tests.get();
    stats.unexpected_transitions = ctx.transitions.get();
    // Output geometry, back in the caller's frame.
    let shifted = |q: [f64; 2]| [q[0] + shift[0], q[1] + shift[1]];
    for (l, t) in lines.iter_mut().zip(&tracks) {
        let p = scene.get(t.placement).ok_or(HlrError::Invalid("placement"))?;
        for piece in &mut l.pieces {
            let (geometry, accuracy) = piece_geometry(t, piece, p, view);
            piece.geometry = geometry.into_iter().map(|g| shift_seg(g, shift)).collect();
            piece.accuracy = accuracy;
            piece.points = piece.points.iter().map(|q| shifted(*q)).collect();
        }
    }
    Ok(Drawing { lines, unsupported, stats })
}

/// The centre of the placed bodies' boxes, after checking the placements.
/// A left-out face's screen box and the depth of its nearest corner.
struct Shadow {
    lo: [f64; 2],
    hi: [f64; 2],
    near: f64,
}

fn shadow_boxes(defs: &[Prepared], scene: &[Placement], view: &View, left_out: &[Unsupported]) -> Vec<Shadow> {
    let mut out = Vec::new();
    for u in left_out {
        let Some(p) = scene.get(u.placement) else { continue };
        let Some(f) = defs.get(p.body).and_then(|d| d.faces.get(u.face)) else { continue };
        let (mut lo, mut hi, mut near) = ([f64::MAX; 2], [f64::MIN; 2], f64::MAX);
        for k in 0..8 {
            let c = Vec3::new(
                if k & 1 == 0 { f.lo.x } else { f.hi.x },
                if k & 2 == 0 { f.lo.y } else { f.hi.y },
                if k & 4 == 0 { f.lo.z } else { f.hi.z },
            );
            let w = p.apply(c);
            let s = view.screen(w);
            lo = [lo[0].min(s[0]), lo[1].min(s[1])];
            hi = [hi[0].max(s[0]), hi[1].max(s[1])];
            near = near.min(w.dot(view.d));
        }
        out.push(Shadow { lo, hi, near });
    }
    out
}

/// Whether the piece `r` of a track may lie behind a left-out face, its own faces included (a
/// curled face can hide its own edge closer than the own-face margin).
fn in_shadow(t: &Track, r: [f64; 2], scene: &[Placement], view: &View, shadows: &ShadowIndex) -> bool {
    if shadows.list.is_empty() {
        return false;
    }
    let Some(p) = scene.get(t.placement) else { return false };
    let c = t.c3();
    // The piece is cut at the boxes' sides and where its curve enters a box between samples:
    // it is in a box when its own screen box overlaps it and, for a piece holding samples, its
    // middle or one of those samples lies in it (its ends may sit on the side it was cut at).
    // Then it is in the shadow when its depth can pass the face's nearest point anywhere along
    // it: the bound comes from the curve itself (its control points' hull, an arc's reach),
    // not from samples, which a dip in depth that the screen does not show could fall between.
    let (lo, hi) = screen_box(c, r, p, view);
    let mut near = Vec::new();
    shadows.query(lo, hi, &mut near);
    if near.is_empty() {
        return false;
    }
    let inner: Vec<[f64; 2]> = t.samples.iter().filter(|q| q.0 > r[0] && q.0 < r[1]).map(|q| q.1).collect();
    let mid = view.screen(p.apply(c.at(0.5 * (r[0] + r[1]))));
    let deepest = depth_bound(c, r, p, view.d);
    near.iter().filter_map(|k| shadows.list.get(*k)).any(|sh| {
        let within = |s: &[f64; 2]| s[0] >= sh.lo[0] && s[0] <= sh.hi[0] && s[1] >= sh.lo[1] && s[1] <= sh.hi[1];
        deepest > sh.near && overlaps(lo, hi, sh) && (inner.is_empty() || within(&mid) || inner.iter().any(within))
    })
}

/// Whether the box `lo hi` and a shadow's box share some area.
fn overlaps(lo: [f64; 2], hi: [f64; 2], sh: &Shadow) -> bool {
    lo[0] < sh.hi[0] && hi[0] > sh.lo[0] && lo[1] < sh.hi[1] && hi[1] > sh.lo[1]
}

/// The screen box of the curve's part `r` placed by `p`: a line's ends, an arc's ends and its
/// extremes along each screen axis, the projected control points a spline's part depends on
/// (they hold it), a polyline's vertices.
fn screen_box(c: &C3, r: [f64; 2], p: &Placement, view: &View) -> ([f64; 2], [f64; 2]) {
    let s = |q: Vec3| view.screen(p.apply(q));
    let mut pts = vec![s(c.at(r[0])), s(c.at(r[1]))];
    match c {
        C3::Line { .. } => {}
        C3::Arc { x, y, r: rad, .. } => {
            // On screen: centre + U cos t + V sin t; each axis peaks where tan t = V_k / U_k.
            let lin = |w: Vec3| view.screen(p.apply(w) - p.translation);
            let (u, v) = (lin(*x * rad[0]), lin(*y * rad[1]));
            for k in 0..2 {
                let (Some(&uk), Some(&vk)) = (u.get(k), v.get(k)) else { continue };
                // The peaks at base + k pi from the first one past the part's start, wherever
                // its angles are counted from.
                let (base, pi) = (vk.atan2(uk), std::f64::consts::PI);
                let first = ((r[0] - base) / pi).ceil();
                for k in 0..8 {
                    let t = base + (first + k as f64) * pi;
                    if t >= r[1] {
                        break;
                    }
                    if t > r[0] {
                        pts.push(s(c.at(t)));
                    }
                }
            }
        }
        C3::Spline { degree, knots, points, .. } => match c.hull(r) {
            Some(h) => pts.extend(h.into_iter().map(s)),
            None => pts.extend(
                points
                    .iter()
                    .enumerate()
                    .filter(|(i, h)| h[3] > 0. && knots.get(*i).zip(knots.get(i + degree + 1)).is_some_and(|(lo, hi)| *lo < r[1] && *hi > r[0]))
                    .map(|(_, h)| s(Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]))),
            ),
        },
        C3::Polyline(v) => pts.extend(v.iter().enumerate().filter(|(i, _)| (*i as f64) > r[0] && (*i as f64) < r[1]).map(|(_, q)| s(*q))),
    }
    let lo = pts.iter().fold([f64::MAX; 2], |m, q| [m[0].min(q[0]), m[1].min(q[1])]);
    let hi = pts.iter().fold([f64::MIN; 2], |m, q| [m[0].max(q[0]), m[1].max(q[1])]);
    (lo, hi)
}

/// Left-out faces' boxes on a uniform screen grid, so a curve meets only those near it.
struct ShadowIndex {
    list: Vec<Shadow>,
    lo: [f64; 2],
    hi: [f64; 2],
    cell: [f64; 2],
    n: usize,
    buckets: Vec<Vec<usize>>,
}

impl ShadowIndex {
    fn new(list: Vec<Shadow>) -> Result<ShadowIndex, HlrError> {
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for sh in &list {
            lo = [lo[0].min(sh.lo[0]), lo[1].min(sh.lo[1])];
            hi = [hi[0].max(sh.hi[0]), hi[1].max(sh.hi[1])];
        }
        let n = ((list.len() as f64).sqrt().ceil() as usize).clamp(1, 1024);
        let cell = [((hi[0] - lo[0]) / n as f64).max(1e-12), ((hi[1] - lo[1]) / n as f64).max(1e-12)];
        let mut index = ShadowIndex { list: Vec::new(), lo, hi, cell, n, buckets: vec![Vec::new(); n * n] };
        let mut entries = 0usize;
        for (k, sh) in list.iter().enumerate() {
            let (a, b) = (index.cell_of(sh.lo), index.cell_of(sh.hi));
            entries = entries.saturating_add((b[0] - a[0] + 1).saturating_mul(b[1] - a[1] + 1));
            if entries > 50_000_000 {
                return Err(HlrError::Budget("shadow grid"));
            }
            for y in a[1]..=b[1] {
                for x in a[0]..=b[0] {
                    if let Some(bucket) = index.buckets.get_mut(y * n + x) {
                        bucket.push(k);
                    }
                }
            }
        }
        index.list = list;
        Ok(index)
    }

    fn cell_of(&self, p: [f64; 2]) -> [usize; 2] {
        let f = |k: usize| (((p[k] - self.lo[k]) / self.cell[k]).floor().clamp(0., (self.n - 1) as f64)) as usize;
        [f(0), f(1)]
    }

    /// The boxes in the cells the box `lo hi` touches, each once.
    fn query(&self, lo: [f64; 2], hi: [f64; 2], out: &mut Vec<usize>) {
        out.clear();
        if self.list.is_empty() || hi[0] < self.lo[0] || hi[1] < self.lo[1] || lo[0] > self.hi[0] || lo[1] > self.hi[1] {
            return;
        }
        let (a, b) = (self.cell_of(lo), self.cell_of(hi));
        for y in a[1]..=b[1] {
            for x in a[0]..=b[0] {
                if let Some(bucket) = self.buckets.get(y * self.n + x) {
                    out.extend_from_slice(bucket);
                }
            }
        }
        out.sort_unstable();
        out.dedup();
    }
}

/// An upper bound of the depth along `d` of the curve's part `r` placed by `p`: a line's ends,
/// an arc's centre plus its reach, the projected control points a spline's part depends on
/// (its weights are positive, so the part lies in their hull), a polyline's vertices.
fn depth_bound(c: &C3, r: [f64; 2], p: &Placement, d: Vec3) -> f64 {
    let depth = |q: Vec3| p.apply(q).dot(d);
    match c {
        C3::Line { .. } => depth(c.at(r[0])).max(depth(c.at(r[1]))),
        C3::Arc { o, x, y, r: rad, .. } => {
            let dl = p.local_dir(d);
            depth(*o) + (rad[0] * x.dot(dl)).hypot(rad[1] * y.dot(dl))
        }
        C3::Spline { degree, knots, points, .. } => match c.hull(r) {
            Some(h) => h.into_iter().map(depth).fold(f64::MIN, f64::max),
            None => points
                .iter()
                .enumerate()
                .filter(|(i, h)| h[3] > 0. && knots.get(*i).zip(knots.get(i + degree + 1)).is_some_and(|(lo, hi)| *lo < r[1] && *hi > r[0]))
                .map(|(_, h)| depth(Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3])))
                .fold(f64::MIN, f64::max),
        }
        .max(depth(c.at(r[0])))
        .max(depth(c.at(r[1]))),
        C3::Polyline(pts) => pts
            .iter()
            .enumerate()
            .filter(|(i, _)| (*i as f64) > r[0] && (*i as f64) < r[1])
            .map(|(_, q)| depth(*q))
            .fold(depth(c.at(r[0])).max(depth(c.at(r[1]))), f64::max),
    }
}

/// Distance between the segment `a b` and the box `lo hi` (zero when they meet).
fn box_gap(a: [f64; 2], b: [f64; 2], lo: [f64; 2], hi: [f64; 2]) -> f64 {
    let inside = |p: [f64; 2]| p[0] >= lo[0] && p[0] <= hi[0] && p[1] >= lo[1] && p[1] <= hi[1];
    if inside(a) || inside(b) {
        return 0.;
    }
    let corners = [[lo[0], lo[1]], [hi[0], lo[1]], [hi[0], hi[1]], [lo[0], hi[1]]];
    let crosses = (0..4).any(|k| {
        let (Some(c), Some(d)) = (corners.get(k), corners.get((k + 1) % 4)) else { return false };
        let side = |p: [f64; 2], q: [f64; 2], r: [f64; 2]| (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]);
        side(a, b, *c) * side(a, b, *d) <= 0. && side(*c, *d, a) * side(*c, *d, b) <= 0.
    });
    if crosses {
        return 0.;
    }
    let to_box = |p: [f64; 2]| {
        let dx = (lo[0] - p[0]).max(p[0] - hi[0]).max(0.);
        let dy = (lo[1] - p[1]).max(p[1] - hi[1]).max(0.);
        dx.hypot(dy)
    };
    let corner_gap = corners.iter().map(|q| point_segment(*q, a, b)).fold(f64::MAX, f64::min);
    to_box(a).min(to_box(b)).min(corner_gap)
}

fn scene_centre(defs: &[Prepared], scene: &[Placement]) -> Result<Vec3, HlrError> {
    let (mut lo, mut hi) = (Vec3::new(f64::MAX, f64::MAX, f64::MAX), Vec3::new(f64::MIN, f64::MIN, f64::MIN));
    for p in scene {
        if !p.rigid() {
            return Err(HlrError::Invalid("placement (not a rotation and translation)"));
        }
        let def = defs.get(p.body).ok_or(HlrError::Invalid("placement body index"))?;
        for k in 0..8 {
            let c = Vec3::new(
                if k & 1 == 0 { def.lo.x } else { def.hi.x },
                if k & 2 == 0 { def.lo.y } else { def.hi.y },
                if k & 4 == 0 { def.lo.z } else { def.hi.z },
            );
            let w = p.apply(c + def.offset);
            if !(w.x.abs() <= MAX_COORDINATE && w.y.abs() <= MAX_COORDINATE && w.z.abs() <= MAX_COORDINATE) {
                return Err(HlrError::Invalid("scene extent"));
            }
            lo = lo.min(w);
            hi = hi.max(w);
        }
    }
    let c = if scene.is_empty() { Vec3::ZERO } else { (lo + hi) * 0.5 };
    if c.is_finite() { Ok(c) } else { Err(HlrError::Invalid("scene extent")) }
}

fn shift_seg(g: Seg2, d: [f64; 2]) -> Seg2 {
    let m = |v: Vec2| Vec2::new(v.x + d[0], v.y + d[1]);
    match g {
        Seg2::Line { a, b } => Seg2::Line { a: m(a), b: m(b) },
        Seg2::Arc { center, radius, start, sweep } => Seg2::Arc { center: m(center), radius, start, sweep },
        Seg2::Cubic { p0, p1, p2, p3 } => Seg2::Cubic { p0: m(p0), p1: m(p1), p2: m(p2), p3: m(p3) },
        Seg2::Conic { a, apex, b, w } => Seg2::Conic { a: m(a), apex: m(apex), b: m(b), w },
        Seg2::Bezier { n, p } => Seg2::Bezier { n, p: p.map(m) },
    }
}

/// A piece's screen geometry: a line for a straight source, exact conic arcs for a circle or
/// ellipse (an orthographic projection is affine, so it maps them to ellipses exactly), chords
/// between its exact points otherwise.
fn piece_geometry(t: &Track, piece: &Piece, p: &Placement, view: &View) -> (Vec<Seg2>, CurveAccuracy) {
    let c = t.c3();
    let s = |q: Vec3| {
        let v = view.screen(p.apply(q));
        Vec2::new(v[0], v[1])
    };
    let chords = || {
        let segs: Vec<Seg2> =
            piece.points.windows(2).filter_map(|w| Some(Seg2::Line { a: Vec2::from(*w.first()?), b: Vec2::from(*w.get(1)?) })).collect();
        (segs, CurveAccuracy::Sampled { max_sample_deviation_mm: piece.sag })
    };
    match c {
        C3::Line { .. } if t.acc == CurveAccuracy::Analytic => {
            (vec![Seg2::Line { a: s(c.at(piece.t[0])), b: s(c.at(piece.t[1])) }], CurveAccuracy::Analytic)
        }
        _ if t.acc == CurveAccuracy::Analytic => {
            // A circle or ellipse: o + u cos(a) + v sin(a), sampled at the piece's angles.
            let frame = match c {
                C3::Arc { o, x, y, r, .. } => Some((*o, *x * r[0], *y * r[1], true)),
                _ => as_circle(c, 1e-9).and_then(|(centre, normal, r)| {
                    let e1 = (c.at(t.range[0]) - centre).normalized()?;
                    Some((centre, e1 * r, normal.cross(e1) * r, false))
                }),
            };
            let Some((o, u, v, param_is_angle)) = frame else { return chords() };
            let angle = |x: f64| {
                if param_is_angle {
                    return x;
                }
                let w = c.at(x) - o;
                w.dot(v).atan2(w.dot(u))
            };
            // The angle turned from the piece's start to its end: the way it turns, certified
            // all along it (C3::turn_sign; a piece that turns back, or one that cannot be
            // certified, is drawn as chords), and its end angles fix the turn, however unevenly
            // it is parameterised. A whole closed edge turns a full circle.
            let (a0, a1) = if param_is_angle {
                (piece.t[0], piece.t[1])
            } else {
                let tau = std::f64::consts::TAU;
                let wrap = |d: f64| (d + std::f64::consts::PI).rem_euclid(tau) - std::f64::consts::PI;
                let (t0, t1) = (piece.t[0], piece.t[1]);
                let span = (t.range[1] - t.range[0]).abs();
                let a0 = angle(t0);
                let ends = angle(t1) - a0;
                let dir = match c.turn_sign([t0, t1], o, u, v) {
                    Some(d) if d != 0. => d,
                    // It does not turn at all: a point-like piece, unless its ends differ.
                    Some(_) if wrap(ends).abs() <= 1e-9 => 1.,
                    _ => return chords(),
                };
                let mut turn = (dir * ends).rem_euclid(tau);
                let whole =
                    t0 <= t.range[0] + 1e-12 * span && t1 >= t.range[1] - 1e-12 * span && c.at(t.range[0]).dist(c.at(t.range[1])) <= 1e-9 * u.len();
                if whole && (turn < 1e-9 || turn > tau - 1e-9) {
                    turn = tau;
                }
                (a0, a0 + dir * turn)
            };
            let pieces = ((a1 - a0).abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.) as usize;
            let at = |a: f64| o + u * a.cos() + v * a.sin();
            let mut out = Vec::with_capacity(pieces);
            for k in 0..pieces {
                let (b0, b1) = (a0 + (a1 - a0) * k as f64 / pieces as f64, a0 + (a1 - a0) * (k + 1) as f64 / pieces as f64);
                let (m, h) = (0.5 * (b0 + b1), 0.5 * (b1 - b0));
                let w = h.cos();
                if w <= 1e-12 {
                    return chords();
                }
                out.push(Seg2::Conic { a: s(at(b0)), apex: s(o + (u * m.cos() + v * m.sin()) / w), b: s(at(b1)), w });
            }
            (out, CurveAccuracy::Analytic)
        }
        _ => chords(),
    }
}

/// Finds where projected tracks cross or touch and records the split parameters on both.
fn crossings(tracks: &mut [Track], ctx: &Ctx) -> Result<usize, HlrError> {
    struct Seg {
        track: usize,
        i: usize,
        a: [f64; 2],
        b: [f64; 2],
        sag: f64,
        /// The grid cells it was put in: first and last column and row.
        cells: [usize; 4],
    }
    let mut segs = Vec::new();
    // Where each track's segments start in `segs`.
    let mut first_seg = Vec::with_capacity(tracks.len());
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for (ti, t) in tracks.iter().enumerate() {
        first_seg.push(segs.len());
        for (i, w) in t.samples.windows(2).enumerate() {
            let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
            for p in [a.1, b.1] {
                lo = [lo[0].min(p[0]), lo[1].min(p[1])];
                hi = [hi[0].max(p[0]), hi[1].max(p[1])];
            }
            segs.push(Seg { track: ti, i, a: a.1, b: b.1, sag: t.sags.get(i).copied().unwrap_or(0.), cells: [0; 4] });
        }
    }
    if segs.is_empty() {
        return Ok(0);
    }
    let n = ((segs.len() as f64).sqrt().ceil() as usize).clamp(1, 512);
    let cell = [((hi[0] - lo[0]) / n as f64).max(1e-12), ((hi[1] - lo[1]) / n as f64).max(1e-12)];
    let idx = |p: [f64; 2]| -> (usize, usize) {
        (((p[0] - lo[0]) / cell[0]).floor().clamp(0., (n - 1) as f64) as usize, ((p[1] - lo[1]) / cell[1]).floor().clamp(0., (n - 1) as f64) as usize)
    };
    let mut grid: Vec<Vec<usize>> = vec![Vec::new(); n * n];
    let mut entries = 0usize;
    for (si, s) in segs.iter_mut().enumerate() {
        if si % 65536 == 0 && (ctx.cancel)() {
            return Err(HlrError::Cancelled);
        }
        let g = s.sag + 1e-9 * ctx.scale;
        let (a, b) = (idx([s.a[0].min(s.b[0]) - g, s.a[1].min(s.b[1]) - g]), idx([s.a[0].max(s.b[0]) + g, s.a[1].max(s.b[1]) + g]));
        entries = entries.saturating_add((b.0 - a.0 + 1).saturating_mul(b.1 - a.1 + 1));
        if entries > 50_000_000 {
            return Err(HlrError::Budget("crossing grid"));
        }
        s.cells = [a.0, a.1, b.0, b.1];
        for y in a.1..=b.1 {
            for x in a.0..=b.0 {
                if let Some(c) = grid.get_mut(y * n + x) {
                    c.push(si);
                }
            }
        }
    }
    // A pair of segments is visited once per cell they share, so the number of visits is
    // known before the search starts, and no set of pairs already met has to be kept.
    let pairs = grid.iter().fold(0usize, |n, c| n.saturating_add(c.len().saturating_mul(c.len().saturating_sub(1)) / 2));
    if pairs > 500_000_000 {
        return Err(HlrError::Budget("crossing pair"));
    }
    let tol = 1e-9 * ctx.scale;
    let mut found = Vec::new();
    let mut visits = 0usize;
    // Two chords that lie on each other end to end, to rounding.
    let coincide = |s: &Seg, t: &Seg| {
        [point_segment(s.a, t.a, t.b), point_segment(s.b, t.a, t.b), point_segment(t.a, s.a, s.b), point_segment(t.b, s.a, s.b)]
            .iter()
            .all(|d| *d <= tol)
    };
    // The segment of the same track on the other side of a segment's end `f` (0 or 1).
    let across = |s: &Seg, f: f64| -> Option<&Seg> {
        let i = if f == 1. { s.i + 1 } else { s.i.checked_sub(1)? };
        segs.get(first_seg.get(s.track)? + i).filter(|n| n.track == s.track && n.i == i)
    };
    // Two curves that coincide meet at every sample point they share, chord ends touching. Such
    // a meeting is no crossing when the curves also coincide on both sides of it; where a
    // coincident run starts or ends, one side differs and the meeting is kept.
    // The end (0 or 1) of a segment that fraction `f` along it lies at, to rounding.
    let end_at = |s: &Seg, f: f64| {
        let len = (s.b[0] - s.a[0]).hypot(s.b[1] - s.a[1]);
        if f * len <= tol {
            Some(0.)
        } else if (1. - f) * len <= tol {
            Some(1.)
        } else {
            None
        }
    };
    let shared_point = |s: &Seg, t: &Seg, (u, v): (f64, f64)| {
        end_at(s, u)
            .zip(end_at(t, v))
            .and_then(|(fu, fv)| across(s, fu).zip(across(t, fv)))
            .is_some_and(|(s2, t2)| coincide(s, t2) && coincide(s2, t))
    };
    // Screen points of a segment's exact curve, `k` pieces (one for lines and polylines).
    let sub = |track: usize, i: usize, k: usize| -> Vec<[f64; 2]> {
        let Some(tr) = tracks.get(track) else { return Vec::new() };
        let (Some(a), Some(b)) = (tr.samples.get(i), tr.samples.get(i + 1)) else { return Vec::new() };
        let Some(p) = ctx.scene.get(tr.placement) else { return Vec::new() };
        let k = if matches!(tr.c3(), C3::Line { .. } | C3::Polyline(_)) { 1 } else { k };
        (0..=k).map(|j| ctx.view.screen(p.apply(tr.c3().at(a.0 + (b.0 - a.0) * j as f64 / k as f64)))).collect()
    };
    for (c, cellv) in grid.iter().enumerate() {
        let (x, y) = (c % n, c / n);
        for (k, &i) in cellv.iter().enumerate() {
            for &j in cellv.iter().skip(k + 1) {
                visits += 1;
                if visits.is_multiple_of(65536) && (ctx.cancel)() {
                    return Err(HlrError::Cancelled);
                }
                let (Some(s), Some(t)) = (segs.get(i), segs.get(j)) else { continue };
                // A pair sharing several cells is intersected in the first of them only.
                if s.cells[0].max(t.cells[0]) != x || s.cells[1].max(t.cells[1]) != y {
                    continue;
                }
                if s.track == t.track && s.i.abs_diff(t.i) <= 1 {
                    continue;
                }
                let straight = |k: usize| tracks.get(k).is_some_and(|tr| matches!(tr.c3(), C3::Line { .. }));
                let overlaps = straight(s.track) && straight(t.track);
                let hits = seg_hits(s.a, s.b, t.a, t.b, tol, overlaps);
                let reach = s.sag + t.sag + tol;
                // Chords of two curves that lie on each other end to end, to rounding (one curve
                // projected again, aligned holes seen along their axes), cross only within that
                // rounding, everywhere and nowhere in particular: visibility changes where such a
                // run starts or ends, which chords that only partly overlap find. Intersecting
                // their fine pieces is wasted work. Chords merely within each other's sag are not
                // skipped: the curves between them can still cross.
                let coincident = s.track != t.track && s.sag + t.sag > 0. && coincide(s, t);
                if coincident {
                    continue;
                }
                if hits.is_empty() && s.sag + t.sag > 0. && seg_gap(s.a, s.b, t.a, t.b) <= reach {
                    // The chords miss but the curves may not: two crossings close together can
                    // lie within one chord's sag. Intersect finer pieces of the exact curves.
                    let (ps, pt) = (sub(s.track, s.i, 16), sub(t.track, t.i, 16));
                    let (ns, nt) = (ps.len().saturating_sub(1).max(1) as f64, pt.len().saturating_sub(1).max(1) as f64);
                    for (x, w) in ps.windows(2).enumerate() {
                        for (y, z) in pt.windows(2).enumerate() {
                            let (Some(a), Some(b), Some(c), Some(d)) = (w.first(), w.get(1), z.first(), z.get(1)) else { continue };
                            for (u, v) in seg_hits(*a, *b, *c, *d, tol, false) {
                                found.push((s.track, s.i, (x as f64 + u) / ns, t.track, t.i, (y as f64 + v) / nt));
                            }
                        }
                    }
                } else {
                    let curved = s.track != t.track && s.sag + t.sag > 0.;
                    for (u, v) in hits {
                        if !(curved && shared_point(s, t, (u, v))) {
                            found.push((s.track, s.i, u, t.track, t.i, v));
                        }
                    }
                }
                if found.len() > 50_000_000 {
                    return Err(HlrError::Budget("crossing"));
                }
            }
        }
    }
    // Curves that nearly coincide in projection (two fitted copies of one intersection, say)
    // cross each other again and again within their tolerance; visibility changes only where
    // the coincidence starts or ends, so such a pair keeps only its outermost crossings.
    let mut by_pair: std::collections::BTreeMap<(usize, usize), Vec<(usize, usize, f64, usize, usize, f64)>> = std::collections::BTreeMap::new();
    for c in found {
        by_pair.entry((c.0.min(c.3), c.0.max(c.3))).or_default().push(c);
    }
    let mut found = Vec::new();
    for (_, mut list) in by_pair {
        if list.len() > 8 {
            list.sort_by(|a, b| (a.1 as f64 + a.2).total_cmp(&(b.1 as f64 + b.2)));
            // Collapse only when the curves stay within their sag of each other between every
            // pair of consecutive crossings (one curve seen twice); a curve that weaves across
            // another keeps all its crossings.
            let together = list.windows(2).all(|w| {
                let (Some(x), Some(y)) = (w.first(), w.get(1)) else { return false };
                let (Some(ta), Some(tb)) = (tracks.get(x.0), tracks.get(x.3)) else { return false };
                let mid = 0.5 * (sample_param(tracks, x.0, x.1, x.2) + sample_param(tracks, y.0, y.1, y.2));
                let Some(pa) = ctx.scene.get(ta.placement) else { return false };
                let q = ctx.view.screen(pa.apply(ta.c3().at(mid)));
                let sag = |t: &Track| t.sags.iter().fold(0., |m: f64, v| m.max(*v));
                let reach = 4. * (sag(ta) + sag(tb)) + tol;
                // Only the other curve's chords between its two crossings (and one either side).
                let (j0, j1) = (x.4.min(y.4).saturating_sub(1), x.4.max(y.4) + 2);
                tb.samples.get(j0..j1.min(tb.samples.len())).is_some_and(|run| {
                    run.windows(2).any(|w| match (w.first(), w.get(1)) {
                        (Some(a), Some(b)) => point_segment(q, a.1, b.1) <= reach,
                        _ => false,
                    })
                })
            });
            if together {
                let (Some(first), Some(last)) = (list.first().copied(), list.last().copied()) else { continue };
                list = vec![first, last];
            }
        }
        found.extend(list);
    }
    let count = found.len();
    for (ta, ia, u, tb, ib, v) in found {
        let pa = sample_param(tracks, ta, ia, u);
        let pb = sample_param(tracks, tb, ib, v);
        let (pa, pb) = refine(tracks, ctx, ta, pa, tb, pb);
        if let Some(t) = tracks.get_mut(ta) {
            t.splits.push(pa);
        }
        if let Some(t) = tracks.get_mut(tb) {
            t.splits.push(pb);
        }
    }
    Ok(count)
}

fn sample_param(tracks: &[Track], t: usize, i: usize, f: f64) -> f64 {
    let Some(tr) = tracks.get(t) else { return 0. };
    match (tr.samples.get(i), tr.samples.get(i + 1)) {
        (Some(a), Some(b)) => a.0 + (b.0 - a.0) * f,
        _ => tr.range[0],
    }
}

/// Newton refinement of a crossing on the exact projected curves; keeps the linear estimate
/// when the curves are tangent or the iteration leaves the curves' ranges.
fn refine(tracks: &[Track], ctx: &Ctx, ta: usize, sa: f64, tb: usize, sb: f64) -> (f64, f64) {
    let (Some(a), Some(b)) = (tracks.get(ta), tracks.get(tb)) else { return (sa, sb) };
    let (Some(pa), Some(pb)) = (ctx.scene.get(a.placement), ctx.scene.get(b.placement)) else { return (sa, sb) };
    if matches!(a.c3(), C3::Polyline(_)) || matches!(b.c3(), C3::Polyline(_)) {
        return (sa, sb);
    }
    let fa = |s: f64| ctx.view.screen(pa.apply(a.c3().at(s)));
    let fb = |s: f64| ctx.view.screen(pb.apply(b.c3().at(s)));
    let da = |s: f64| ctx.view.screen(pa.apply(a.c3().d1(s)) - pa.translation);
    let db = |s: f64| ctx.view.screen(pb.apply(b.c3().d1(s)) - pb.translation);
    let (mut s, mut t) = (sa, sb);
    for _ in 0..30 {
        let (p, q) = (fa(s), fb(t));
        let f = [p[0] - q[0], p[1] - q[1]];
        let (ja, jb) = (da(s), db(t));
        // [ja, -jb] [ds, dt]^T = -f
        let det = ja[0] * (-jb[1]) - (-jb[0]) * ja[1];
        if det.abs() <= 1e-14 * (1. + ja[0].abs() + ja[1].abs()) * (1. + jb[0].abs() + jb[1].abs()) {
            return (sa, sb);
        }
        let ds = (-f[0] * (-jb[1]) - (-jb[0]) * (-f[1])) / det;
        let dt = (ja[0] * (-f[1]) - (-f[0]) * ja[1]) / det;
        s += ds;
        t += dt;
        if ds.abs() + dt.abs() <= 1e-15 * (1. + s.abs() + t.abs()) {
            break;
        }
    }
    let ok = |x: f64, r: [f64; 2]| x.is_finite() && x >= r[0] - 1e-12 * (r[1] - r[0]) && x <= r[1] + 1e-12 * (r[1] - r[0]);
    let close = (s - sa).abs() <= 0.5 * (a.range[1] - a.range[0]) && (t - sb).abs() <= 0.5 * (b.range[1] - b.range[0]);
    if ok(s, a.range) && ok(t, b.range) && close { (s.clamp(a.range[0], a.range[1]), t.clamp(b.range[0], b.range[1])) } else { (sa, sb) }
}

/// Marks hidden and uncertain pieces lying within `tol` of a visible piece of another line.
fn cover(lines: &mut [Line], tol: f64, step: f64, cancel: &dyn Fn() -> bool) -> Result<(), HlrError> {
    // Chords of visible and hidden pieces, keyed by (line, piece) so a piece never covers
    // itself; another piece of the same line may (a circle seen edge-on folds over itself).
    // `tol` is the base; each chord adds its own sag, each query point the sag of its chord.
    let mut segs: Vec<((usize, usize), Visibility, [f64; 2], [f64; 2], f64)> = Vec::new();
    for (li, l) in lines.iter().enumerate() {
        for (pi, p) in l.pieces.iter().enumerate().filter(|(_, p)| p.visibility != Visibility::Uncertain) {
            segs.extend(p.points.windows(2).filter_map(|w| Some(((li, pi), p.visibility, *w.first()?, *w.get(1)?, p.sag))));
        }
    }
    let reach = segs.iter().map(|s| s.4).fold(0., f64::max) * 2. + tol;
    if segs.is_empty() {
        return Ok(());
    }
    // Bucket the chords on a grid of cells at least `tol` wide.
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for (_, _, a, b, _) in &segs {
        for p in [a, b] {
            lo = [lo[0].min(p[0]), lo[1].min(p[1])];
            hi = [hi[0].max(p[0]), hi[1].max(p[1])];
        }
    }
    let n = ((segs.len() as f64).sqrt().ceil() as usize).clamp(1, 1024);
    let cell = [((hi[0] - lo[0]) / n as f64).max(reach), ((hi[1] - lo[1]) / n as f64).max(reach)];
    let at = |x: f64, k: usize| (((x - lo[k]) / cell[k]).floor().max(0.) as usize).min(n - 1);
    let mut grid: Vec<Vec<usize>> = vec![Vec::new(); n * n];
    for (i, (_, _, a, b, _)) in segs.iter().enumerate() {
        for y in at(a[1].min(b[1]) - reach, 1)..=at(a[1].max(b[1]) + reach, 1) {
            for x in at(a[0].min(b[0]) - reach, 0)..=at(a[0].max(b[0]) + reach, 0) {
                if let Some(c) = grid.get_mut(y * n + x) {
                    c.push(i);
                }
            }
        }
    }
    // Which coincident pieces hide a piece: visible ones hide hidden and uncertain pieces;
    // a duplicate of a visible or hidden piece on an earlier line is dropped.
    let hides = |key: (usize, usize), own: Visibility, other: (usize, usize), state: Visibility| {
        other != key
            && match (own, state) {
                (Visibility::Hidden | Visibility::Uncertain, Visibility::Visible) => true,
                (Visibility::Visible, Visibility::Visible) | (Visibility::Hidden, Visibility::Hidden) => other.0 < key.0,
                _ => false,
            }
    };
    let checks = std::cell::Cell::new(0usize);
    let near = |key: (usize, usize), own: Visibility, q: [f64; 2], slack: f64| {
        checks.set(checks.get() + 1);
        let Some(bucket) = grid.get(at(q[1], 1) * n + at(q[0], 0)) else { return false };
        bucket
            .iter()
            .filter_map(|i| segs.get(*i))
            .any(|(k, state, a, b, sag)| hides(key, own, *k, *state) && point_segment(q, *a, *b) <= tol + sag + slack)
    };
    for (li, l) in lines.iter_mut().enumerate() {
        for (pi, p) in l.pieces.iter_mut().enumerate() {
            // Points along every chord at most `step` apart (a coincident line may cover a long
            // chord's ends and middle yet leave gaps between them). Points between the exact
            // ones lie on the chord, up to the piece's sag from its curve.
            let mut covered = !p.points.is_empty();
            if checks.get() > 200_000_000 {
                return Err(HlrError::Budget("coverage"));
            }
            if pi % 16 == 0 && cancel() {
                return Err(HlrError::Cancelled);
            }
            for w in p.points.windows(2) {
                let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
                let k = ((a[0] - b[0]).hypot(a[1] - b[1]) / step).ceil().clamp(1., 1e6) as usize;
                covered = covered
                    && (0..=k).all(|j| {
                        let f = j as f64 / k as f64;
                        // The piece's own representation error (chord sag, fitted bound) applies
                        // to every point it is checked at.
                        let slack = p.sag;
                        near((li, pi), p.visibility, [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f], slack)
                    });
                if !covered {
                    break;
                }
            }
            if let [q] = p.points.as_slice() {
                covered = near((li, pi), p.visibility, *q, p.sag);
            }
            p.covered = covered;
        }
    }
    Ok(())
}

fn point_segment(q: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (ex, ey) = (b[0] - a[0], b[1] - a[1]);
    let l2 = ex * ex + ey * ey;
    let t = if l2 > 0. { (((q[0] - a[0]) * ex + (q[1] - a[1]) * ey) / l2).clamp(0., 1.) } else { 0. };
    (a[0] + t * ex - q[0]).hypot(a[1] + t * ey - q[1])
}

/// Smallest distance between two screen segments that do not cross.
fn seg_gap(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> f64 {
    point_segment(a, c, d).min(point_segment(b, c, d)).min(point_segment(c, a, b)).min(point_segment(d, a, b))
}

/// Intersections of two screen segments, touching included: fractions along each.
fn seg_hits(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2], tol: f64, overlaps: bool) -> Vec<(f64, f64)> {
    let r = [b[0] - a[0], b[1] - a[1]];
    let s = [d[0] - c[0], d[1] - c[1]];
    let den = r[0] * s[1] - r[1] * s[0];
    let w = [c[0] - a[0], c[1] - a[1]];
    let (rl, sl) = ((r[0] * r[0] + r[1] * r[1]).sqrt(), (s[0] * s[0] + s[1] * s[1]).sqrt());
    if rl <= 0. || sl <= 0. {
        return Vec::new();
    }
    if den.abs() > 1e-12 * rl * sl {
        let u = (w[0] * s[1] - w[1] * s[0]) / den;
        let v = (w[0] * r[1] - w[1] * r[0]) / den;
        let (tu, tv) = (tol / rl, tol / sl);
        if u >= -tu && u <= 1. + tu && v >= -tv && v <= 1. + tv {
            return vec![(u.clamp(0., 1.), v.clamp(0., 1.))];
        }
        return Vec::new();
    }
    // Parallel: overlapping collinear lines split at each other's endpoints. Sampled curves
    // that coincide are skipped: their chords overlap everywhere and would split at every sample.
    let dist = (w[0] * r[1] - w[1] * r[0]).abs() / rl;
    if dist > tol || !overlaps {
        return Vec::new();
    }
    let along = |p: [f64; 2]| ((p[0] - a[0]) * r[0] + (p[1] - a[1]) * r[1]) / (rl * rl);
    let back = |p: [f64; 2]| ((p[0] - c[0]) * s[0] + (p[1] - c[1]) * s[1]) / (sl * sl);
    let mut out = Vec::new();
    for (p, on_cd) in [(c, true), (d, true), (a, false), (b, false)] {
        let (u, v) = if on_cd { (along(p), if p == c { 0. } else { 1. }) } else { (if p == a { 0. } else { 1. }, back(p)) };
        if (0. ..=1.).contains(&u) && (0. ..=1.).contains(&v) {
            out.push((u, v));
        }
    }
    out
}

/// States at a few interior points, with transitions located by bisection.
fn classify_range(t: &Track, a: f64, b: f64, ctx: &Ctx) -> Result<Vec<([f64; 2], Visibility)>, HlrError> {
    let fr = [0.1, 0.3, 0.5, 0.7, 0.9];
    let xs: Vec<f64> = fr.iter().map(|f| a + (b - a) * f).collect();
    let mut states = Vec::with_capacity(xs.len());
    for x in &xs {
        states.push(classify(t, *x, ctx)?);
    }
    let mut out: Vec<([f64; 2], Visibility)> = Vec::new();
    let mut start = a;
    for k in 0..xs.len() {
        let (Some(&s), Some(&x)) = (states.get(k), xs.get(k)) else { continue };
        if let (Some(&next), Some(&nx)) = (states.get(k + 1), xs.get(k + 1)) {
            if next != s {
                ctx.transitions.set(ctx.transitions.get() + 1);
                let (mut lo, mut hi) = (x, nx);
                for _ in 0..40 {
                    let mid = 0.5 * (lo + hi);
                    if classify(t, mid, ctx)? == s {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                let cut = 0.5 * (lo + hi);
                out.push(([start, cut], s));
                start = cut;
            }
        } else {
            out.push(([start, b], s));
        }
        let _ = x;
    }
    Ok(out)
}

fn classify(t: &Track, x: f64, ctx: &Ctx) -> Result<Visibility, HlrError> {
    let n = ctx.rays.get() + 1;
    ctx.rays.set(n);
    if n > ctx.max_rays {
        return Err(HlrError::Budget("ray"));
    }
    if n.is_multiple_of(1024) && (ctx.cancel)() {
        return Err(HlrError::Cancelled);
    }
    let own = ctx.scene.get(t.placement).ok_or(HlrError::Invalid("placement"))?;
    Ok(classify_point(ctx, t.placement, &t.own, t.own_eps, own.apply(t.c3().at(x))))
}

/// Visibility of a world point on placement `placement` lying on its faces `own`.
fn classify_point(ctx: &Ctx, placement: usize, own: &[usize], own_eps: f64, world: Vec3) -> Visibility {
    let screen = ctx.view.screen(world);
    let depth = world.dot(ctx.view.d);
    let toward = -ctx.view.d;
    let mut uncertain = false;
    let mut hits: Vec<Hit> = Vec::new();
    for (qi, q) in ctx.scene.iter().enumerate() {
        let Some((slo, shi, dmin)) = ctx.boxes.get(qi) else { continue };
        let pad = ctx.eps_other;
        if screen[0] < slo[0] - pad || screen[0] > shi[0] + pad || screen[1] < slo[1] - pad || screen[1] > shi[1] + pad || *dmin > depth {
            continue;
        }
        let Some(def) = ctx.defs.get(q.body) else { continue };
        let (p0, dl) = (q.local(world), q.local_dir(toward));
        for (fi, f) in def.faces.iter().enumerate() {
            let is_own = qi == placement && own.contains(&fi);
            let Some([l0, l1]) = slab(p0, dl, f.lo, f.hi) else { continue };
            let eps = if is_own { own_eps } else { ctx.eps_other };
            let range = [l0.max(eps), l1];
            if !(range[1] >= range[0]) {
                continue;
            }
            ctx.surface_tests.set(ctx.surface_tests.get() + 1);
            hits.clear();
            f.surface.ray_as(&f.analytic, p0, dl, range, &mut hits);
            for h in &hits {
                let p = f.surface.point(h.uv);
                let tol = 2. * f.trim.chord_error + 1e-7 * (f.hi - f.lo).len();
                // A hit on the boundary is uncertain on either side of it, so the parity test is
                // left out there: its cost grows with the boundary around the hit, all of a
                // circle when bodies in line along the view meet each other on their circles.
                if f.trim.near(p, tol) {
                    uncertain = true;
                    continue;
                }
                match f.trim.contains(h.uv) {
                    None => uncertain = true,
                    Some(inside) => {
                        // An unsupported freeform face never hides anything for certain.
                        let unsure = f.freeform && !ctx.freeform;
                        if inside && (unsure || h.grazing || (!is_own && h.lam < ctx.eps_own)) {
                            uncertain = true;
                        } else if inside {
                            return Visibility::Hidden;
                        }
                    }
                }
            }
        }
    }
    if uncertain { Visibility::Uncertain } else { Visibility::Visible }
}

/// Phase timing where a clock exists.
struct Clock {
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    last: std::cell::Cell<std::time::Instant>,
}

impl Clock {
    fn start() -> Clock {
        Clock {
            #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
            last: std::cell::Cell::new(std::time::Instant::now()),
        }
    }
    fn lap(&self) -> f64 {
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        {
            let now = std::time::Instant::now();
            let ms = now.duration_since(self.last.get()).as_secs_f64() * 1e3;
            self.last.set(now);
            ms
        }
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        0.
    }
}

/// Ray parameter range inside an axis-aligned box.
fn slab(p: Vec3, d: Vec3, lo: Vec3, hi: Vec3) -> Option<[f64; 2]> {
    let (mut t0, mut t1) = (f64::NEG_INFINITY, f64::INFINITY);
    for (o, dd, a, b) in [(p.x, d.x, lo.x, hi.x), (p.y, d.y, lo.y, hi.y), (p.z, d.z, lo.z, hi.z)] {
        if dd.abs() < 1e-300 {
            if o < a || o > b {
                return None;
            }
        } else {
            let (x, y) = ((a - o) / dd, (b - o) / dd);
            t0 = t0.max(x.min(y));
            t1 = t1.min(x.max(y));
        }
    }
    (t1 >= t0.max(0.)).then_some([t0.max(0.), t1])
}

#[cfg(test)]
#[path = "../hlr_tests.rs"]
mod tests;
