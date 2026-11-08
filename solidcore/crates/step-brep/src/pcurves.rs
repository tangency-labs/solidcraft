//! UV trim curves from a STEP file's own pcurves.
//!
//! Written from the ISO 10303-42 definitions only. A `PCURVE` gives an edge's curve in
//! the (u, v) parameters of one surface. The kernel's analytic surfaces evaluate those
//! parameters (angles in degrees) as their STEP parameterisation
//! (`Surface::evaluate_step_point`), and [`step_import`] maps each pcurve into those
//! units; the cylinder or cone frame this builder rotates about its axis
//! ([`crate::seam_frame`]) moves the kernel's u behind the file's by the rotation angle.
//! Their UV trim curves, though, live in the parameters of the surface's NURB, which
//! the kernel converts to and from the STEP ones (`ConvertUVFromSTEPToNURBS`; the
//! angle conversion is not linear). The checks below work in STEP parameters, and each
//! accepted piece is converted for its UV trim curve: exactly where it keeps one
//! parameter constant (a cylinder's circles and rulings), else as a polyline refined
//! until it lies within half the B-rep tolerance of the piece. A plane's parameters
//! need no conversion, and its pieces stay exact. Pcurves on B-spline surfaces,
//! surfaces of revolution and extrusions are not used.
//!
//! Each edge use's piece of its pcurve is found by projecting points of the edge onto
//! the pcurve lifted to the surface, then checked: the piece must run monotonically
//! along the edge, every sample must lie within the B-rep tolerance of the edge's 3D
//! curve, the loop's pieces must chain end to start (shifted by whole periods on a
//! periodic surface) and close, and the pieces must stay inside the face's UV domain.
//! A face gets the file's pcurves only when every edge use checks out; any other face
//! keeps the kernel's projected UV trim curves, and [`PcurveStats`] counts why.

use std::collections::BTreeMap;

use solidcore::curve::curve::Curve;
use solidcore::curve::line::{BoundedLineOptions, Line};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::global_solver::SolverOperation;
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::surface::surface::Surface;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::face::{p2 as fp2, p3 as fp3, p4 as fp4};
use solidcore::topology::{loopuse, Brep, FaceId};
use step_import::model::{self as sm, Body, Bound, Curve2d, FilePcurves};

/// What happened to a build's file pcurves ([`crate::BuildOptions::file_pcurves`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PcurveStats {
    /// Faces whose UV trim curves were offered the file's pcurves.
    pub faces: usize,
    /// Edge uses of those faces that took their file pcurve.
    pub uses: usize,
    /// Non-seam edge uses of those faces that the kernel projected instead: uses of
    /// edges the seam fix split, and uses it did not match to their pcurve.
    pub uses_projected: usize,
    /// Seam edge uses of those faces, offered no candidate (which periodic side a seam
    /// use's pcurve belongs to is ambiguous from the edge alone), so the kernel
    /// projected them.
    pub seam_uses: usize,
    /// Faces left to the kernel's projection, by reason.
    pub fallbacks: BTreeMap<&'static str, usize>,
    /// Of [`Self::faces`], those on a curved surface (not a plane or B-spline surface,
    /// whose STEP parameters are its NURB ones).
    pub curved_faces: usize,
    /// Of [`Self::uses`], those on a curved surface.
    pub curved_uses: usize,
    /// Of [`Self::fallbacks`], those on a curved surface.
    pub curved_fallbacks: BTreeMap<&'static str, usize>,
}

impl PcurveStats {
    /// Adds `other`'s counts to these.
    pub fn add(&mut self, other: &PcurveStats) {
        self.faces += other.faces;
        self.uses += other.uses;
        self.uses_projected += other.uses_projected;
        self.seam_uses += other.seam_uses;
        for (reason, count) in &other.fallbacks {
            *self.fallbacks.entry(*reason).or_default() += count;
        }
        self.curved_faces += other.curved_faces;
        self.curved_uses += other.curved_uses;
        for (reason, count) in &other.curved_fallbacks {
            *self.curved_fallbacks.entry(*reason).or_default() += count;
        }
    }

    fn fallback(&mut self, reason: &'static str, curved: bool) {
        *self.fallbacks.entry(reason).or_default() += 1;
        if curved {
            *self.curved_fallbacks.entry(reason).or_default() += 1;
        }
    }
}

/// Every face of `body` counted as left to the kernel for `reason` (a build that does
/// not use the file's pcurves).
pub(crate) fn not_used(body: &Body, reason: &'static str) -> PcurveStats {
    let mut stats = PcurveStats::default();
    for face in &body.faces {
        let curved = !matches!(
            face.surface,
            sm::Surface::Plane { .. } | sm::Surface::BSpline(_)
        );
        stats.fallback(reason, curved);
    }
    stats
}

/// The kernel face a model face became, with what its pcurves need.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BuiltFace {
    /// The face `MakeFaceWithCurves` made.
    pub face: Option<FaceId>,
    /// Degrees the kernel surface's u is behind the file's (the seam frame rotation).
    pub u_offset: f64,
    /// The face was split at a seam after stitching.
    pub split: bool,
}

/// Degrees from `file`'s x axis to `rotated`'s, about z: how far the kernel's u
/// (measured from `rotated`'s x) is behind the file's.
pub(crate) fn u_offset(file: &sm::Frame, rotated: &sm::Frame) -> f64 {
    let (Ok(x), Ok(y), Ok(new_x)) = (
        crate::v3(file.x).unitize(),
        crate::v3(file.y).unitize(),
        crate::v3(rotated.x).unitize(),
    ) else {
        return 0.0;
    };
    f64::atan2(new_x.dot(y), new_x.dot(x)).to_degrees()
}

/// Gives each built face whose every edge use has a usable file pcurve its UV trim
/// curves from them. The other faces are counted by reason and left as they are.
pub(crate) fn apply(
    k: &Kernel,
    brep: &mut Brep,
    body: &Body,
    pcurves: &FilePcurves,
    built: &[BuiltFace],
    stats: &mut PcurveStats,
) {
    for (face, built) in body.faces.iter().zip(built) {
        let curved = !matches!(
            face.surface,
            sm::Surface::Plane { .. } | sm::Surface::BSpline(_)
        );
        match face_candidates(k, brep, body, pcurves, face, built) {
            Ok(None) => {}
            Ok(Some((id, curves, uses, seam_uses))) => match set_trims(k, brep, id, curves) {
                Ok(used) => {
                    stats.faces += 1;
                    stats.uses += used;
                    stats.uses_projected += uses.saturating_sub(used);
                    stats.seam_uses += seam_uses;
                    if curved {
                        stats.curved_faces += 1;
                        stats.curved_uses += used;
                    }
                }
                Err(_) => stats.fallback("kernel error setting the trims", curved),
            },
            Err(reason) => stats.fallback(reason, curved),
        }
    }
}

/// Replaces the UV trim curves of `face`'s edge uses (the stitch made some already)
/// with the best of `curves`; returns how many were taken. The kernel projects the
/// edge of any edge use no candidate matches.
fn set_trims(
    k: &Kernel,
    brep: &mut Brep,
    face: FaceId,
    curves: Vec<Curve>,
) -> solidcore::Result<usize> {
    for lu in fp4::upward_loopuses(brep, face, false, None)? {
        for eu in loopuse::edgeuses(brep, lu, None, None)? {
            eu1::set_uv_trim_curve(brep, eu, eu1::UvTrimCurveArg::Null, 0.0, true, None)?;
        }
    }
    let offered = curves.len();
    // A candidate running in its edge use's direction matches that use with
    // orientation `Same` whichever way the edge's curve runs (`sm_FindBestUVCurve`
    // skips a candidate whose lifted tangent opposes the use's).
    let orients = vec![OrientType::Same; offered];
    let mut slots: Vec<Option<Curve>> = curves.into_iter().map(Some).collect();
    fp3::create_uv_trim_curves(
        k,
        brep,
        face,
        false,
        Some((slots.as_mut_slice(), orients.as_slice())),
    )?;
    let left = slots.iter().flatten().count();
    Ok(offered - left)
}

/// The periods of a surface's (u, v) in the model's units, or `None` for a surface
/// whose pcurves are not used. A B-spline surface's are not: a closed one has two
/// parameter values on its seam, and its seam uses (left to the kernel) can take the
/// other side, leaving a parameter-range gap in the loop (seen on f1-db-214).
fn periods(surface: &sm::Surface) -> Option<[Option<f64>; 2]> {
    match surface {
        sm::Surface::Plane { .. } => Some([None, None]),
        sm::Surface::Cylinder { .. } | sm::Surface::Cone { .. } | sm::Surface::Sphere { .. } => {
            Some([Some(360.0), None])
        }
        sm::Surface::Torus { .. } => Some([Some(360.0), Some(360.0)]),
        sm::Surface::Revolution { .. }
        | sm::Surface::Extrusion { .. }
        | sm::Surface::BSpline(_) => None,
    }
}

/// A face's UV trim curve candidates: the kernel face, the candidates, and its
/// non-seam and seam edge use counts.
type FaceCandidates = (FaceId, Vec<Curve>, usize, usize);

/// The UV trim curve candidates of `face`, with its non-seam and its seam edge use
/// counts; `None` for a face without edge loops; the reason when a use's pcurve does not
/// check out.
///
/// Each non-seam use offers its piece both ways: the face's upward edge uses run the
/// loop's way or the other, and the kernel takes the one whose tangent agrees.
fn face_candidates(
    k: &Kernel,
    brep: &Brep,
    body: &Body,
    pcurves: &FilePcurves,
    face: &sm::Face,
    built: &BuiltFace,
) -> Result<Option<FaceCandidates>, &'static str> {
    let id = built.face.ok_or("face not built")?;
    if built.split {
        return Err("face split at a seam");
    }
    if !brep.face_list().contains(&id) {
        return Err("face not in the stitched body");
    }
    let surface_id = *pcurves
        .face_surfaces
        .get(&face.step_id)
        .ok_or("face without a surface entity")?;
    let periods = periods(&face.surface).ok_or("surface kind without pcurve support")?;
    let surface = fp2::surface(brep, id)
        .ok()
        .flatten()
        .ok_or("face without a surface")?;
    let step = surface
        .step_uv_domain_with_nurb(k)
        .map_err(|_| "surface without a STEP domain")?;
    let lift = Lift {
        surface,
        identity: matches!(
            face.surface,
            sm::Surface::Plane { .. } | sm::Surface::BSpline(_)
        ),
        lows: [step.u_interval().min(), step.v_interval().min()],
        periods,
    };
    let domain = fp2::uv_domain(brep, id).map_err(|_| "face without a UV domain")?;
    let domain = lift.step_domain(k, &domain)?;
    let tol = brep.tolerance();
    // Edges the face uses twice: seams.
    let mut seen = Vec::new();
    let mut seams = Vec::new();
    for bound in &face.loops {
        if let Bound::Edges(edges) = &bound.bound {
            for edge in edges {
                if seen.contains(&edge.edge) {
                    seams.push(edge.edge);
                } else {
                    seen.push(edge.edge);
                }
            }
        }
    }
    let mut out = Vec::new();
    let mut count = 0;
    let mut seam_count = 0;
    let mut any = false;
    for bound in &face.loops {
        let Bound::Edges(edges) = &bound.bound else {
            continue;
        };
        let oriented: Vec<(usize, bool)> = if bound.orientation {
            edges.iter().map(|edge| (edge.edge, edge.forward)).collect()
        } else {
            edges
                .iter()
                .rev()
                .map(|edge| (edge.edge, !edge.forward))
                .collect()
        };
        let mut uses = Vec::new();
        let mut on_seam = Vec::new();
        for (index, forward) in oriented {
            let edge = body.edges.get(index).ok_or("missing edge")?;
            let pcurve = pcurves
                .edges
                .get(&edge.step_id)
                .and_then(|list| list.iter().find(|p| p.surface == surface_id))
                .ok_or("edge use without a pcurve on the face's surface")?;
            let uv = Uv::new(&pcurve.curve, built.u_offset)?;
            let range = uv.range(pcurve.trim, &domain, periods)?;
            uses.push(edge_use(k, &lift, edge, forward, uv, range, tol)?);
            on_seam.push(seams.contains(&index));
        }
        let shifts = chain(&uses, &domain, periods)?;
        for ((piece, shift), seam) in uses.iter().zip(&shifts).zip(on_seam) {
            if !piece.inside(*shift, &domain) {
                return Err("pcurve outside the face's UV domain");
            }
            if seam {
                seam_count += 1;
            } else {
                out.push(piece.trim_curve(k, &lift, *shift, false, tol)?);
                out.push(piece.trim_curve(k, &lift, *shift, true, tol)?);
                count += 1;
            }
        }
        any = true;
    }
    Ok(any.then_some((id, out, count, seam_count)))
}

/// A pcurve in the kernel surface's parameters (before any period shift).
#[derive(Debug, Clone)]
enum Uv {
    /// `origin + t * dir`.
    Line { origin: [f64; 2], dir: [f64; 2] },
    /// `centre + cos t * axis1 + sin t * axis2`, t in degrees.
    Conic {
        centre: [f64; 2],
        axis1: [f64; 2],
        axis2: [f64; 2],
    },
    /// A B-spline: the kernel curve evaluated, and the model data a shifted or trimmed
    /// copy is made from.
    Spline {
        curve: Box<Curve>,
        data: sm::BSplineCurve,
    },
}

/// `p` shifted by `s`.
fn add(p: [f64; 2], s: [f64; 2]) -> [f64; 2] {
    [p[0] + s[0], p[1] + s[1]]
}

/// A dimension-2 kernel B-spline of `data`'s first two coordinates shifted by `shift`,
/// trimmed to `trim`.
fn spline_2d(
    data: &sm::BSplineCurve,
    shift: [f64; 2],
    trim: Option<Extent1d>,
) -> Result<Curve, &'static str> {
    let points: Vec<Point3> = data
        .points
        .iter()
        .map(|p| Point3::new(p[0] + shift[0], p[1] + shift[1], 0.0))
        .collect();
    let mults = crate::usizes(&data.mults);
    BSplineCurve::create_canonical(&CanonicalData {
        dim: 2,
        degree: data.degree as usize,
        control_points: &points,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &mults,
        knots: &data.knots,
        knot_type: KnotType::Unspecified,
        weights: data.weights.as_deref(),
        trim,
    })
    .map(Curve::from)
    .map_err(|_| "pcurve B-spline rejected by the kernel")
}

impl Uv {
    /// `curve` with its u moved back by `u_offset` degrees.
    fn new(curve: &Curve2d, u_offset: f64) -> Result<Self, &'static str> {
        let back = |p: [f64; 2]| [p[0] - u_offset, p[1]];
        Ok(match curve {
            Curve2d::Line { origin, dir } => Uv::Line {
                origin: back(*origin),
                dir: *dir,
            },
            Curve2d::Conic {
                centre,
                axis1,
                axis2,
            } => Uv::Conic {
                centre: back(*centre),
                axis1: *axis1,
                axis2: *axis2,
            },
            Curve2d::BSpline(spline) => {
                let mut data = spline.clone();
                for p in &mut data.points {
                    p[0] -= u_offset;
                }
                let curve = spline_2d(&data, [0.0, 0.0], None)?;
                Uv::Spline {
                    curve: Box::new(curve),
                    data,
                }
            }
        })
    }

    /// The point at `t`.
    fn at(&self, k: &Kernel, t: f64) -> Result<[f64; 2], &'static str> {
        Ok(match self {
            Uv::Line { origin, dir } => [origin[0] + t * dir[0], origin[1] + t * dir[1]],
            Uv::Conic {
                centre,
                axis1,
                axis2,
            } => {
                let (sin, cos) = t.to_radians().sin_cos();
                [
                    centre[0] + cos * axis1[0] + sin * axis2[0],
                    centre[1] + cos * axis1[1] + sin * axis2[1],
                ]
            }
            Uv::Spline { curve, .. } => {
                let p = curve
                    .evaluate_point(k, t)
                    .map_err(|_| "pcurve evaluation failed")?;
                [p.x, p.y]
            }
        })
    }

    /// The parameter range searched for the edge: a conic's period, a B-spline's knot
    /// range, a line's trims (widened by 5%) or, untrimmed, the stretch inside the face
    /// domain (only where the line does not run along a periodic parameter).
    fn range(
        &self,
        trim: Option<(f64, f64)>,
        domain: &Extent2d,
        periods: [Option<f64>; 2],
    ) -> Result<(f64, f64), &'static str> {
        match self {
            Uv::Conic { .. } => Ok((0.0, 360.0)),
            Uv::Spline { curve, .. } => {
                let ivl = curve.natural_interval();
                Ok((ivl.min(), ivl.max()))
            }
            Uv::Line { origin, dir } => {
                if let Some((a, b)) = trim {
                    let (lo, hi) = (a.min(b), a.max(b));
                    let pad = 0.05 * (hi - lo);
                    return if hi > lo {
                        Ok((lo - pad, hi + pad))
                    } else {
                        Err("pcurve line with an empty trim")
                    };
                }
                let mut lo = f64::NEG_INFINITY;
                let mut hi = f64::INFINITY;
                let coordinates = [
                    (origin[0], dir[0], domain.u_interval(), periods[0]),
                    (origin[1], dir[1], domain.v_interval(), periods[1]),
                ];
                for (o, d, ivl, period) in coordinates {
                    if d == 0.0 {
                        continue;
                    }
                    if period.is_some() {
                        return Err("untrimmed pcurve line along a periodic parameter");
                    }
                    let (a, b) = ((ivl.min() - o) / d, (ivl.max() - o) / d);
                    lo = lo.max(a.min(b));
                    hi = hi.min(a.max(b));
                }
                if lo.is_finite() && hi.is_finite() && hi > lo {
                    Ok((lo, hi))
                } else {
                    Err("untrimmed pcurve line outside the face domain")
                }
            }
        }
    }

    /// Whether the parameter is periodic (a conic's).
    fn periodic(&self) -> bool {
        matches!(self, Uv::Conic { .. })
    }
}

/// One edge use's piece of its pcurve: from `t0` (the use's start) to `t1`.
#[derive(Debug, Clone)]
struct Piece {
    uv: Uv,
    t0: f64,
    t1: f64,
    /// UV samples from start to end (before any period shift), the ends first and last.
    samples: Vec<[f64; 2]>,
}

/// Samples of an edge use checked against its pcurve.
const EDGE_SAMPLES: u32 = 4;
/// Samples of a pcurve piece checked against the edge's 3D curve.
const PIECE_SAMPLES: u32 = 8;
/// Coarse samples of a pcurve's range when projecting a point onto it.
const PROJECT_SAMPLES: u32 = 64;
/// Golden-section steps refining a projection.
const PROJECT_STEPS: u32 = 80;
/// The golden-section ratio.
const GOLDEN: f64 = 0.618_033_988_749_894_9;

/// A face's surface evaluated at STEP parameters.
struct Lift<'a> {
    surface: &'a Surface,
    /// The STEP parameters are the NURB's (a plane, a B-spline surface).
    identity: bool,
    /// The low ends of the surface's STEP domain.
    lows: [f64; 2],
    periods: [Option<f64>; 2],
}

/// `x` moved by whole periods into `[low, low + period)`.
fn wrap(x: f64, low: f64, period: Option<f64>) -> f64 {
    period.map_or(x, |period| low + (x - low).rem_euclid(period))
}

/// `x` clamped to `[low, low + period]`.
fn clamp(x: f64, low: f64, period: Option<f64>) -> f64 {
    period.map_or(x, |period| x.clamp(low, low + period))
}

impl Lift<'_> {
    /// The surface point at STEP parameters `uv` (periodic ones taken modulo their
    /// period: the STEP evaluation clamps to its domain).
    fn point(&self, k: &Kernel, [u, v]: [f64; 2]) -> Result<Point3, &'static str> {
        let uv = Point2::new(
            wrap(u, self.lows[0], self.periods[0]),
            wrap(v, self.lows[1], self.periods[1]),
        );
        self.surface
            .evaluate_step_point(k, uv)
            .map_err(|_| "surface evaluation failed")
    }

    /// The NURB parameters of STEP parameters `uv` inside the face's domain.
    fn to_nurbs(&self, k: &Kernel, [u, v]: [f64; 2]) -> Result<[f64; 2], &'static str> {
        if self.identity {
            return Ok([u, v]);
        }
        let uv = Point2::new(
            clamp(u, self.lows[0], self.periods[0]),
            clamp(v, self.lows[1], self.periods[1]),
        );
        let p = self
            .surface
            .convert_uv_from_step_to_nurbs(k, uv)
            .map_err(|_| "STEP to NURB parameter conversion failed")?;
        Ok([p.x, p.y])
    }

    /// The face's UV domain (NURB parameters) in STEP parameters.
    fn step_domain(&self, k: &Kernel, domain: &Extent2d) -> Result<Extent2d, &'static str> {
        if self.identity {
            return Ok(*domain);
        }
        let (u, v) = (domain.u_interval(), domain.v_interval());
        let mut lo = [f64::INFINITY; 2];
        let mut hi = [f64::NEG_INFINITY; 2];
        for (a, b) in [
            (u.min(), v.min()),
            (u.max(), v.min()),
            (u.min(), v.max()),
            (u.max(), v.max()),
        ] {
            let p = self
                .surface
                .convert_uv_from_nurbs_to_step(k, Point2::new(a, b))
                .map_err(|_| "NURB to STEP parameter conversion failed")?;
            lo = [lo[0].min(p.x), lo[1].min(p.y)];
            hi = [hi[0].max(p.x), hi[1].max(p.y)];
        }
        Ok(Extent2d::from_bounds(lo[0], lo[1], hi[0], hi[1]))
    }
}

/// The distance from `p` to `uv` at `t` lifted to the surface.
fn lifted_distance(
    k: &Kernel,
    lift: &Lift<'_>,
    uv: &Uv,
    t: f64,
    p: Point3,
) -> Result<f64, &'static str> {
    Ok(lift.point(k, uv.at(k, t)?)?.distance(p))
}

/// The parameter in `[lo, hi]` of `uv` lifted to `surface` nearest `p`, and the
/// distance: the nearest of evenly spaced samples, refined by golden-section search
/// between its neighbours.
fn project(
    k: &Kernel,
    lift: &Lift<'_>,
    uv: &Uv,
    (lo, hi): (f64, f64),
    p: Point3,
) -> Result<(f64, f64), &'static str> {
    let step = (hi - lo) / f64::from(PROJECT_SAMPLES);
    let mut best = (lo, f64::INFINITY);
    for i in 0..=PROJECT_SAMPLES {
        let t = lo + step * f64::from(i);
        let d = lifted_distance(k, lift, uv, t, p)?;
        if d < best.1 {
            best = (t, d);
        }
    }
    let (mut a, mut b) = ((best.0 - step).max(lo), (best.0 + step).min(hi));
    let mut c = b - GOLDEN * (b - a);
    let mut d = a + GOLDEN * (b - a);
    let mut fc = lifted_distance(k, lift, uv, c, p)?;
    let mut fd = lifted_distance(k, lift, uv, d, p)?;
    for _ in 0..PROJECT_STEPS {
        if fc < fd {
            b = d;
            d = c;
            fd = fc;
            c = b - GOLDEN * (b - a);
            fc = lifted_distance(k, lift, uv, c, p)?;
        } else {
            a = c;
            c = d;
            fc = fd;
            d = a + GOLDEN * (b - a);
            fd = lifted_distance(k, lift, uv, d, p)?;
        }
    }
    let t = 0.5 * (a + b);
    let dt = lifted_distance(k, lift, uv, t, p)?;
    Ok(if dt < best.1 { (t, dt) } else { best })
}

/// `t` moved by whole periods of 360 to within 180 of `near`.
fn unwrap_near(t: f64, near: f64) -> f64 {
    near + (t - near + 180.0).rem_euclid(360.0) - 180.0
}

/// The piece of `uv` for the use of `edge` (`forward`: from its start vertex to its
/// end), searched in `range`; checked to run monotonically along the edge and to lie
/// within `tol` of its 3D curve.
fn edge_use(
    k: &Kernel,
    lift: &Lift<'_>,
    edge: &sm::Edge,
    forward: bool,
    uv: Uv,
    range: (f64, f64),
    tol: f64,
) -> Result<Piece, &'static str> {
    let (low, high) = if edge.same_sense {
        (edge.t_start, edge.t_end)
    } else {
        (edge.t_end, edge.t_start)
    };
    let curve = crate::make_curve(k, &edge.curve, Some((low, high)))
        .map_err(|_| "edge curve rejected by the kernel")?;
    let ivl = curve.natural_interval();
    let along = forward == edge.same_sense;
    let mut ts = Vec::new();
    let mut ends = Vec::new();
    for i in 0..=EDGE_SAMPLES {
        let f = f64::from(i) / f64::from(EDGE_SAMPLES);
        let q = curve
            .evaluate_point(k, ivl.evaluate(if along { f } else { 1.0 - f }))
            .map_err(|_| "edge curve evaluation failed")?;
        if i == 0 || i == EDGE_SAMPLES {
            ends.push(q);
        }
        let (t, d) = project(k, lift, &uv, range, q)?;
        if d > tol {
            return Err("edge off its pcurve");
        }
        let t = match ts.last() {
            Some(&prev) if uv.periodic() => unwrap_near(t, prev),
            _ => t,
        };
        ts.push(t);
    }
    // Where a B-spline or line pcurve is closed the edge's ends project onto either of
    // its ends: take the one that continues the interior samples.
    let interior = ts.get(1..ts.len().saturating_sub(1)).unwrap_or_default();
    let increasing = interior.windows(2).all(|w| matches!(w, [a, b] if a < b));
    let decreasing = interior.windows(2).all(|w| matches!(w, [a, b] if a > b));
    if !(increasing || decreasing) {
        return Err("pcurve not monotonic along the edge");
    }
    let (first_inner, last_inner) = (
        interior.first().copied().unwrap_or(range.0),
        interior.last().copied().unwrap_or(range.1),
    );
    let mut t0 = ts.first().copied().unwrap_or(range.0);
    let mut t1 = ts.last().copied().unwrap_or(range.1);
    let before = |a: f64, b: f64| if increasing { a < b } else { a > b };
    let (start_end, finish_end) = if increasing {
        (range.0, range.1)
    } else {
        (range.1, range.0)
    };
    if !before(t0, first_inner) {
        if uv.periodic() {
            return Err("pcurve not monotonic along the edge");
        }
        t0 = start_end;
    }
    if !before(last_inner, t1) {
        if uv.periodic() {
            return Err("pcurve not monotonic along the edge");
        }
        t1 = finish_end;
    }
    // The piece must start and end on the use's ends.
    for (t, q) in [t0, t1].into_iter().zip(ends) {
        if lifted_distance(k, lift, &uv, t, q)? > tol {
            return Err("pcurve ends off the edge's ends");
        }
    }
    // Every piece sample must lie on the edge's 3D curve.
    let mut samples = Vec::new();
    for j in 0..=PIECE_SAMPLES {
        let t = t0 + (t1 - t0) * f64::from(j) / f64::from(PIECE_SAMPLES);
        let [u, v] = uv.at(k, t)?;
        let p = lift.point(k, [u, v])?;
        let dropped = curve
            .drop_point(k, &ivl, p, None, tol, None, SolverOperation::Minimize)
            .map_err(|_| "edge curve projection failed")?;
        if !dropped.success || dropped.distance > tol {
            return Err("pcurve off the edge's 3D curve");
        }
        samples.push([u, v]);
    }
    Ok(Piece {
        uv,
        t0,
        t1,
        samples,
    })
}

/// The shift of a periodic coordinate `x` into `[low, low + period)`, or `None` when it
/// is within a millionth of a period of either end (on the seam).
fn anchor_shift(x: f64, low: f64, period: Option<f64>) -> Option<f64> {
    let Some(period) = period else {
        return Some(0.0);
    };
    let shift = -((x - low) / period).floor() * period;
    let r = x + shift - low;
    let margin = 1e-6 * period;
    (r > margin && r < period - margin).then_some(shift)
}

/// The whole periods moving `x` nearest `target`.
fn chain_shift(x: f64, target: f64, period: Option<f64>) -> f64 {
    period.map_or(0.0, |period| ((target - x) / period).round() * period)
}

/// Each piece's period shift: one piece whose middle is clear of the seam is moved into
/// the face domain, then each following piece starts where the one before ended. A
/// loop whose end is not back at its start (by less than a quarter period) is refused.
fn chain(
    pieces: &[Piece],
    domain: &Extent2d,
    periods: [Option<f64>; 2],
) -> Result<Vec<[f64; 2]>, &'static str> {
    let lows = [domain.u_interval().min(), domain.v_interval().min()];
    let (anchor, shift) = pieces
        .iter()
        .enumerate()
        .find_map(|(i, piece)| {
            let middle = piece.samples.get(piece.samples.len() / 2)?;
            let su = anchor_shift(middle[0], lows[0], periods[0])?;
            let sv = anchor_shift(middle[1], lows[1], periods[1])?;
            Some((i, [su, sv]))
        })
        .ok_or("no edge use clear of the seam")?;
    let mut shifts = vec![shift];
    let start_of = |piece: &Piece| piece.samples.first().copied().unwrap_or_default();
    let end_of = |piece: &Piece| piece.samples.last().copied().unwrap_or_default();
    let first = pieces.get(anchor).ok_or("no edge use")?;
    let mut end = add(end_of(first), shift);
    for piece in pieces
        .iter()
        .cycle()
        .skip(anchor + 1)
        .take(pieces.len().saturating_sub(1))
    {
        let start = start_of(piece);
        let s = [
            chain_shift(start[0], end[0], periods[0]),
            chain_shift(start[1], end[1], periods[1]),
        ];
        shifts.push(s);
        end = add(end_of(piece), s);
    }
    let start = add(start_of(first), shift);
    for (gap, period) in [
        (end[0] - start[0], periods[0]),
        (end[1] - start[1], periods[1]),
    ] {
        if period.is_some_and(|period| gap.abs() > 0.25 * period) {
            return Err("loop does not close in UV");
        }
    }
    // back to loop order
    shifts.rotate_right(anchor);
    Ok(shifts)
}

impl Piece {
    /// Whether every sample, shifted by `shift`, is inside `domain` (to a ten-millionth
    /// of its size).
    fn inside(&self, shift: [f64; 2], domain: &Extent2d) -> bool {
        let (u, v) = (domain.u_interval(), domain.v_interval());
        let (mu, mv) = (1e-7 * (1.0 + u.length()), 1e-7 * (1.0 + v.length()));
        self.samples.iter().all(|&sample| {
            let [a, b] = add(sample, shift);
            a >= u.min() - mu && a <= u.max() + mu && b >= v.min() - mv && b <= v.max() + mv
        })
    }

    /// The UV trim curve candidate in the surface's NURB parameters: [`Self::candidate`]
    /// where they are the STEP ones; else the piece converted, exactly as a line where
    /// it keeps one parameter constant, otherwise as a polyline refined until each
    /// segment's middle lies within half of `tol` of the piece's span over it.
    fn trim_curve(
        &self,
        k: &Kernel,
        lift: &Lift<'_>,
        shift: [f64; 2],
        reversed: bool,
        tol: f64,
    ) -> Result<Curve, &'static str> {
        if lift.identity {
            return self.candidate(k, shift, reversed);
        }
        let (t0, t1) = if reversed {
            (self.t1, self.t0)
        } else {
            (self.t0, self.t1)
        };
        let constant = |c: fn(&[f64; 2]) -> f64| {
            let first = self.samples.first().map_or(0.0, c);
            self.samples
                .iter()
                .all(|p| (c(p) - first).abs() <= 1e-12 * (1.0 + first.abs()))
        };
        let at = |t: f64| -> Result<[f64; 2], &'static str> { Ok(add(self.uv.at(k, t)?, shift)) };
        if constant(|p| p[0]) || constant(|p| p[1]) {
            return nurbs_polyline(&[lift.to_nurbs(k, at(t0)?)?, lift.to_nurbs(k, at(t1)?)?]);
        }
        let mut segments = 16u32;
        while segments <= 512 {
            let mut points = Vec::new();
            for i in 0..=segments {
                let t = t0 + (t1 - t0) * f64::from(i) / f64::from(segments);
                points.push(lift.to_nurbs(k, at(t)?)?);
            }
            let mut close = true;
            for (i, pair) in points.windows(2).enumerate() {
                let [a, b] = pair else {
                    continue;
                };
                let middle = Point2::new(0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1]));
                let q = lift
                    .surface
                    .evaluate_point(k, middle)
                    .map_err(|_| "surface evaluation failed")?;
                // (the shift is whole periods: the lifted piece is the same without it)
                let ta = t0 + (t1 - t0) * i as f64 / f64::from(segments);
                let tb = t0 + (t1 - t0) * (i as f64 + 1.0) / f64::from(segments);
                let (_, gap) = project(k, lift, &self.uv, (ta.min(tb), ta.max(tb)), q)?;
                if gap > 0.5 * tol {
                    close = false;
                    break;
                }
            }
            if close {
                return nurbs_polyline(&points);
            }
            segments *= 2;
        }
        Err("pcurve piece too curved for a polyline in NURB parameters")
    }

    /// The UV trim curve candidate: the piece shifted by `shift`, running from its
    /// use's start to its end (`reversed`: the other way). A line is an exact
    /// dimension-2 line, a conic exact rational quadratic arcs (at most 90 degrees
    /// each), a B-spline the trimmed spline.
    fn candidate(
        &self,
        k: &Kernel,
        shift: [f64; 2],
        reversed: bool,
    ) -> Result<Curve, &'static str> {
        let (t0, t1) = if reversed {
            (self.t1, self.t0)
        } else {
            (self.t0, self.t1)
        };
        match &self.uv {
            Uv::Line { .. } => {
                let start = add(self.uv.at(k, t0)?, shift);
                let end = add(self.uv.at(k, t1)?, shift);
                let chord = Vec3::new(end[0] - start[0], end[1] - start[1], 0.0);
                let scale = chord.length();
                let dir = chord.unitize().map_err(|_| "degenerate pcurve line")?;
                Ok(Curve::from(Line::bounded(
                    Vec3::new(start[0], start[1], 0.0),
                    dir,
                    &Extent1d::from_min_max(0.0, 1.0),
                    &BoundedLineOptions {
                        scale,
                        dim: 2,
                        ..BoundedLineOptions::default()
                    },
                )))
            }
            Uv::Conic {
                centre,
                axis1,
                axis2,
            } => conic_arcs(add(*centre, shift), *axis1, *axis2, t0, t1),
            Uv::Spline { data, .. } => {
                let (lo, hi) = (t0.min(t1), t0.max(t1));
                let mut curve = spline_2d(data, shift, Some(Extent1d::from_min_max(lo, hi)))?;
                if t0 > t1 {
                    let ivl = curve.natural_interval();
                    curve
                        .reverse_parameterization(k, &ivl)
                        .map_err(|_| "pcurve B-spline reversal failed")?;
                }
                Ok(curve)
            }
        }
    }
}

/// The dimension-2 degree-1 B-spline through `points`, parameter 0 to the segment
/// count.
fn nurbs_polyline(points: &[[f64; 2]]) -> Result<Curve, &'static str> {
    let control: Vec<Point3> = points
        .iter()
        .map(|p| Point3::new(p[0], p[1], 0.0))
        .collect();
    let count = control.len().saturating_sub(1);
    if count == 0 {
        return Err("degenerate pcurve piece");
    }
    let knots: Vec<f64> = (0..=count).map(|i| i as f64).collect();
    let mut mults = vec![1usize; count + 1];
    if let Some(m) = mults.first_mut() {
        *m = 2;
    }
    if let Some(m) = mults.last_mut() {
        *m = 2;
    }
    BSplineCurve::create_canonical(&CanonicalData {
        dim: 2,
        degree: 1,
        control_points: &control,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &mults,
        knots: &knots,
        knot_type: KnotType::Unspecified,
        weights: None,
        trim: None,
    })
    .map(Curve::from)
    .map_err(|_| "pcurve polyline rejected by the kernel")
}

/// The conic `centre + cos t * axis1 + sin t * axis2` from `t0` to `t1` (degrees,
/// either way) as a dimension-2 rational quadratic B-spline: one exact arc per at most
/// 90 degrees, parameter 0 to the arc count.
fn conic_arcs(
    centre: [f64; 2],
    axis1: [f64; 2],
    axis2: [f64; 2],
    t0: f64,
    t1: f64,
) -> Result<Curve, &'static str> {
    let sweep = t1 - t0;
    let arcs = (sweep.abs() / 90.0).ceil().max(1.0);
    if !arcs.is_finite() || arcs > 4.0 {
        return Err("pcurve conic sweep over a full turn");
    }
    let count = arcs as usize;
    let at = |t: f64, scale: f64| {
        let (sin, cos) = t.to_radians().sin_cos();
        Point3::new(
            centre[0] + scale * (cos * axis1[0] + sin * axis2[0]),
            centre[1] + scale * (cos * axis1[1] + sin * axis2[1]),
            0.0,
        )
    };
    let step = sweep / arcs;
    let half = (0.5 * step).to_radians().cos();
    let mut points = vec![at(t0, 1.0)];
    let mut weights = vec![1.0];
    for i in 0..count {
        let a = t0 + step * i as f64;
        points.push(at(a + 0.5 * step, 1.0 / half));
        weights.push(half);
        points.push(at(a + step, 1.0));
        weights.push(1.0);
    }
    let knots: Vec<f64> = (0..=count).map(|i| i as f64).collect();
    let mut mults = vec![2usize; count + 1];
    if let Some(m) = mults.first_mut() {
        *m = 3;
    }
    if let Some(m) = mults.last_mut() {
        *m = 3;
    }
    BSplineCurve::create_canonical(&CanonicalData {
        dim: 2,
        degree: 2,
        control_points: &points,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &mults,
        knots: &knots,
        knot_type: KnotType::Unspecified,
        weights: Some(&weights),
        trim: None,
    })
    .map(Curve::from)
    .map_err(|_| "pcurve conic rejected by the kernel")
}

#[cfg(test)]
mod tests {
    use super::*;
    use step_import::model::{Frame, Surface as ModelSurface};

    fn frame() -> Frame {
        Frame {
            origin: [1.0, 2.0, 3.0],
            x: [0.0, 1.0, 0.0],
            y: [0.0, 0.0, 1.0],
            z: [1.0, 0.0, 0.0],
        }
    }

    fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }

    fn plus(p: [f64; 3], s: f64, d: [f64; 3]) -> [f64; 3] {
        [p[0] + s * d[0], p[1] + s * d[1], p[2] + s * d[2]]
    }

    /// The ISO 10303-42 point of `surface` at (u, v), angles in degrees.
    fn iso_point(surface: &ModelSurface, u: f64, v: f64) -> [f64; 3] {
        let (su, cu) = u.to_radians().sin_cos();
        let (sv, cv) = v.to_radians().sin_cos();
        match surface {
            ModelSurface::Plane { frame } => plus(plus(frame.origin, u, frame.x), v, frame.y),
            ModelSurface::Cylinder { frame, radius } => {
                let p = plus(frame.origin, radius * cu, frame.x);
                plus(plus(p, radius * su, frame.y), v, frame.z)
            }
            ModelSurface::Cone {
                frame,
                radius,
                semi_angle_deg,
            } => {
                let r = radius + v * semi_angle_deg.to_radians().tan();
                let p = plus(frame.origin, r * cu, frame.x);
                plus(plus(p, r * su, frame.y), v, frame.z)
            }
            ModelSurface::Sphere { frame, radius } => {
                let p = plus(frame.origin, radius * cv * cu, frame.x);
                plus(plus(p, radius * cv * su, frame.y), radius * sv, frame.z)
            }
            ModelSurface::Torus {
                frame,
                major,
                minor,
            } => {
                let r = major + minor * cv;
                let p = plus(frame.origin, r * cu, frame.x);
                plus(plus(p, r * su, frame.y), minor * sv, frame.z)
            }
            _ => unreachable!(),
        }
    }

    /// The kernel surfaces this builder makes take the model's pcurve parameters
    /// unchanged (ISO 10303-42, angles in degrees); a rotated cylinder or cone frame
    /// moves u back by [`u_offset`].
    #[test]
    fn kernel_surfaces_use_the_iso_parameters() {
        let k = Kernel::default();
        let f = frame();
        let surfaces = [
            ModelSurface::Plane { frame: f },
            ModelSurface::Cylinder {
                frame: f,
                radius: 2.5,
            },
            ModelSurface::Cone {
                frame: f,
                radius: 2.5,
                semi_angle_deg: 20.0,
            },
            ModelSurface::Sphere {
                frame: f,
                radius: 2.5,
            },
            ModelSurface::Torus {
                frame: f,
                major: 4.0,
                minor: 1.5,
            },
        ];
        let params = [(0.0, 0.0), (37.0, 0.5), (200.0, -1.25), (300.0, 60.0)];
        for model in &surfaces {
            let surface = crate::make_surface(&k, model, None).expect("surface");
            for &(u, v) in &params {
                // (a torus's STEP v runs over 0..360; the STEP evaluation clamps to it)
                let v = if matches!(model, ModelSurface::Torus { .. }) && v < 0.0 {
                    v + 360.0
                } else {
                    v
                };
                let p = surface
                    .evaluate_step_point(&k, Point2::new(u, v))
                    .expect("evaluate");
                let q = iso_point(model, u, v);
                let gap = sub([p.x, p.y, p.z], q);
                let gap = (gap[0] * gap[0] + gap[1] * gap[1] + gap[2] * gap[2]).sqrt();
                assert!(gap < 1e-9, "{model:?} at ({u}, {v}): {p:?} vs {q:?}");
            }
        }
        // The seam frame rotated by 30 degrees: the file's u = 70 is the kernel's 40.
        let (s, c) = 30f64.to_radians().sin_cos();
        let rotated = Frame {
            x: plus(plus([0.0; 3], c, f.x), s, f.y),
            y: plus(plus([0.0; 3], c, f.y), -s, f.x),
            ..f
        };
        let offset = u_offset(&f, &rotated);
        assert!((offset - 30.0).abs() < 1e-12, "{offset}");
        let model = ModelSurface::Cylinder {
            frame: f,
            radius: 2.5,
        };
        let surface = crate::make_surface(&k, &model, Some(&rotated)).expect("surface");
        let p = surface
            .evaluate_step_point(&k, Point2::new(70.0 - offset, 1.0))
            .expect("evaluate");
        let q = iso_point(&model, 70.0, 1.0);
        let gap = sub([p.x, p.y, p.z], q);
        assert!(gap.iter().all(|g| g.abs() < 1e-9), "{p:?} vs {q:?}");
    }

    /// On a cylinder the trim curves are in the NURB parameters: a circle stays an
    /// exact line, a helix becomes a polyline within the tolerance, and both convert
    /// back to the STEP piece.
    #[test]
    fn trim_curves_are_in_nurb_parameters() {
        let k = Kernel::default();
        let model = ModelSurface::Cylinder {
            frame: frame(),
            radius: 2.5,
        };
        let surface = crate::make_surface(&k, &model, None).expect("surface");
        let step = surface.step_uv_domain_with_nurb(&k).expect("domain");
        let lift = Lift {
            surface: &surface,
            identity: false,
            lows: [step.u_interval().min(), step.v_interval().min()],
            periods: [Some(360.0), None],
        };
        for (dir, segments) in [([1.0, 0.0], 1), ([1.0, 0.01], 2)] {
            let uv = Uv::Line {
                origin: [30.0, 3.0],
                dir,
            };
            let samples = (0..=PIECE_SAMPLES)
                .map(|j| {
                    uv.at(&k, 220.0 * f64::from(j) / f64::from(PIECE_SAMPLES))
                        .expect("at")
                })
                .collect();
            let piece = Piece {
                uv,
                t0: 0.0,
                t1: 220.0,
                samples,
            };
            let curve = piece
                .trim_curve(&k, &lift, [0.0, 0.0], false, 1e-5)
                .expect("trim curve");
            let ivl = curve.natural_interval();
            for i in 0..=50 {
                let p = curve
                    .evaluate_point(&k, ivl.evaluate(f64::from(i) / 50.0))
                    .expect("uv");
                let back = surface
                    .convert_uv_from_nurbs_to_step(&k, Point2::new(p.x, p.y))
                    .expect("step");
                let v = 3.0 + dir[1] * (back.x - 30.0);
                assert!((back.y - v).abs() < 1e-5, "{dir:?} {i}: {back:?}");
                if i == 0 || i == 50 {
                    let u = if i == 0 { 30.0 } else { 250.0 };
                    assert!((back.x - u).abs() < 1e-7, "{dir:?} {i}: {back:?}");
                }
            }
            // The circle is one segment; the helix needs more.
            assert_eq!(ivl.max() - ivl.min() > 1.0, segments > 1, "{dir:?} {ivl:?}");
        }
    }

    #[test]
    fn conic_arcs_are_exact() {
        let k = Kernel::default();
        // An ellipse scaled unevenly, swept backwards over 250 degrees.
        let (centre, axis1, axis2) = ([1.0, -2.0], [3.0, 0.5], [-0.25, 1.5]);
        let curve = conic_arcs(centre, axis1, axis2, 100.0, -150.0).expect("arcs");
        let ivl = curve.natural_interval();
        for i in 0..=40 {
            let p = curve
                .evaluate_point(&k, ivl.evaluate(f64::from(i) / 40.0))
                .expect("evaluate");
            // Solve centre + cos t a1 + sin t a2 = p for (cos t, sin t).
            let (x, y) = (p.x - centre[0], p.y - centre[1]);
            let det = axis1[0] * axis2[1] - axis2[0] * axis1[1];
            let c = (x * axis2[1] - axis2[0] * y) / det;
            let s = (axis1[0] * y - x * axis1[1]) / det;
            assert!((c * c + s * s - 1.0).abs() < 1e-12, "{i}: {c} {s}");
        }
        let start = curve.evaluate_point(&k, ivl.min()).expect("start");
        let end = curve.evaluate_point(&k, ivl.max()).expect("end");
        let point = |t: f64| {
            let (s, c) = f64::to_radians(t).sin_cos();
            [
                centre[0] + c * axis1[0] + s * axis2[0],
                centre[1] + c * axis1[1] + s * axis2[1],
            ]
        };
        let (a, b) = (point(100.0), point(-150.0));
        assert!((start.x - a[0]).abs() < 1e-12 && (start.y - a[1]).abs() < 1e-12);
        assert!((end.x - b[0]).abs() < 1e-12 && (end.y - b[1]).abs() < 1e-12);
    }

    #[test]
    fn chain_shifts_seam_uses_by_whole_periods() {
        let k = Kernel::default();
        let line = |origin: [f64; 2], dir: [f64; 2]| Uv::Line { origin, dir };
        // A full cylinder band in kernel u: bottom 0 -> 360 at v = 0, the seam up at
        // u = 360, the top back 360 -> 0 at v = 5, the seam down at u = 0. The file
        // gives every piece 720 degrees off, and both seam pieces at the same u.
        let piece = |uv: Uv, t0: f64, t1: f64| {
            let samples = (0..=PIECE_SAMPLES)
                .map(|j| {
                    uv.at(&k, t0 + (t1 - t0) * f64::from(j) / f64::from(PIECE_SAMPLES))
                        .expect("at")
                })
                .collect();
            Piece {
                uv,
                t0,
                t1,
                samples,
            }
        };
        let pieces = [
            piece(line([720.0, 0.0], [1.0, 0.0]), 0.0, 360.0),
            piece(line([720.0, 0.0], [0.0, 1.0]), 0.0, 5.0),
            piece(line([720.0, 5.0], [1.0, 0.0]), 360.0, 0.0),
            piece(line([720.0, 0.0], [0.0, 1.0]), 5.0, 0.0),
        ];
        let domain = Extent2d::from_bounds(0.0, -1.0, 360.0, 6.0);
        let periods = [Some(360.0), None];
        let shifts = chain(&pieces, &domain, periods).expect("chain");
        assert_eq!(
            shifts,
            vec![[-720.0, 0.0], [-360.0, 0.0], [-720.0, 0.0], [-720.0, 0.0]]
        );
        for (piece, shift) in pieces.iter().zip(&shifts) {
            assert!(piece.inside(*shift, &domain));
        }
        // Without the seam pieces the band's loops do not close.
        assert_eq!(
            chain(&pieces[..1], &domain, periods),
            Err("loop does not close in UV")
        );
    }
}
