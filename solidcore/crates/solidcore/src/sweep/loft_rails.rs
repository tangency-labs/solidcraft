//! A loft guided by one or two rails: stations between its sections, each the plain loft there
//! moved so the points the rails guide land on them, skinned, with every guided edge certified
//! within the caller's tolerance of its rail.
//!
//! Upstream: none (not in SMLib). The station maps are SolidCraft's (`crates/kernel/src/
//! loft_rails.rs` at 52afe21: `railed`, `station_map`, `rail`, `centroid`), moved into the
//! kernel so no application builds them; the certificate and the placement of the stations are
//! new (tangency-labs/solidcore#3140, its hull case).
//!
//! | Rust                 | Upstream |
//! |----------------------|----------|
//! | [`loft_rails`]       | (none; SolidCraft's `railed`, then the loft) |
//! | [`sweep_two_rails`]  | (none; the two-rail sweep, The NURBS Book §10.4) |
//! | [`RailSection`]      | (none; SolidCraft's `Section`) |
//! | [`RailLoftOptions`]  | (none) |
//! | `station_map` (private) | (none; SolidCraft's `station_map`) |
//! | [`certificate`]      | (none) |
//!
//! **The stations (SolidCraft's construction, carried over).** The sections are at loft
//! parameters in proportion to the mean distance between their curves' starts. The plain loft
//! at a parameter `v` moves each control point of each section curve along the curve through
//! its counterparts in every section (a line between two sections; the kernel's interpolation
//! of degree `min(3, sections - 1)` at the sections' parameters across more). A rail meets each
//! section where it crosses the section's plane, at a point of the section's curves within the
//! body tolerance ([`crate::kernel::Tolerances::zone_tol_3d`]); that point, as a wire parameter
//! (the curve's index plus the fraction of its parameter range), is the point the rail guides.
//! A station at `v`, `f` of the way across its span, takes the plain loft there, the guided
//! point at the wire parameter `f` of the way between the span's two, and the rail's crossing
//! of the plane through that point square to the section planes' normals turned `f` of the
//! way, nudged so that at each section it is the section's own point. The station's map
//! ([`station_map`]) puts the guided points on those points: with one rail it turns about the
//! loft's direction and sizes about the station's middle (its curves' centroid by length),
//! shifted along the loft as far as the rail leads or lags; with two it moves with the first,
//! turns and sizes with the line between them, and leans to follow the second. The loft's
//! direction at a station is the central difference of the plain loft's middles over `1e-4`
//! of the loft parameter, SolidCraft's convention.
//!
//! **Where the stations go, and the certificate.** Each span between two sections starts with
//! one station at its middle. The stations and sections are skinned
//! ([`crate::sweep::api_surfaces::create_skin_primitive`]), and for each rail, between each two
//! neighbouring stations, the distance between the skin and the rail is bounded
//! ([`certificate::span_bound`]): every point of the rail's arc there is proved within the
//! bound of the skin, and a curve of the skin within it of the rail. A span whose bound is above
//! the tolerance gets a station at its middle, and the loft is skinned and bounded again. This
//! terminates by construction: each round adds a station, and past the caller's
//! [`RailLoftOptions::max_wires`] the loft is refused, naming the bound reached, the tolerance
//! asked and the stations used ([`crate::error::sweep::SweepFailure::RailPastStationCap`]). No
//! tolerance is loosened and no uncertified loft is returned. A span's rail ends are found on
//! the skin's surfaces by projection (not trusted: the bound holds for any correspondence), and
//! the span is bounded on the surface both lie nearest at once (a rail along an edge between two
//! faces lies on both).
//!
//! **Refused**, naming why: no section or one, no rail or more than two; sections whose curves
//! do not pair (counts, degrees, knots or weights differing by more than `1e-9` relative,
//! SolidCraft's convention); two sections on top of each other; a rail not crossing a section's
//! plane exactly once, or crossing it farther than the body tolerance from its curves; two
//! rails at one point of a section; a rail through the stations' middle, or two rails meeting
//! (the station shrinks to nothing, within `1e-6` of the loft's size, SolidCraft's convention);
//! a tolerance not finite and positive.
//!
//! **One profile along two rails ([`sweep_two_rails`]).** The two-rail sweep (Piegl and
//! Tiller, *The NURBS Book*, 2nd ed., §10.4: a sweep as a skin through placed sections): an open
//! profile whose ends lie on the two rails' starts (within the body tolerance) is placed at
//! stations of the rails' common parameter (each rail's natural range mapped linearly onto
//! `[0, 1]`): at `f` the rails' points `A`, `B`, the chord `c = B - A`, its unit `ĉ` and size
//! `k = |c| / |c0|`, and the direction of travel `d`, the mean of the rails' unit tangents made
//! square to the chord. Each control point `x` of the profile goes to `A + M (x - A0)`, `M` the
//! frame turning `d0` onto `d` unsized and `ĉ0` onto `ĉ` sized by `k`, the third way `d × ĉ`
//! sized by `k` ([`RailFit::Scale`]) or kept ([`RailFit::Stretch`]). The chord lands on the
//! chord, so the profile's ends lie on the rails at every station; at `f = 0` the map is the
//! identity, so the profile is swept as drawn. The stations are placed and certified by the
//! same loop as the loft's (`skin_and_certify`), each rail bounded against the skin's guided
//! edge. Refused, naming why: a closed profile or one asked to close or cap (two-rail sweeps
//! take open profiles), profile curves not end to end, the profile's ends not on the rails'
//! starts, rails starting the opposite ways, rails meeting (the chord within `1e-6` of the
//! size), the mean tangent along the chord.
//!
//! Deviations: SolidCraft crosses a rail's polyline from the previous crossing on; here a rail
//! is a kernel curve crossed exactly ([`crate::sweep::stations`]), so a rail crossing a station's
//! or section's plane more than once is refused instead of taking the next crossing. SolidCraft
//! places a fixed count of stations evenly and does not bound the skin between them; here they
//! are placed where the certificate asks. No recursion: the refinement is a loop.

use crate::curve::curve::Curve;
use crate::error::{Result, SolidError};
use crate::geom::vector2d::Point2;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use crate::nurbs::certificate;
use crate::nurbs::crv_basic::Curve as NurbCurve;
use crate::nurbs::crv_fit::p1::{interp_given_params, interp_knot_vector};
use crate::nurbs::knots::{KnotVector, Side};
use crate::nurbs::math::{CPoint, NOW};
use crate::sweep::api_surfaces::{create_skin_primitive, SkinPrimitive};
use crate::sweep::stations::{apply, rail_crossing, rail_frame, Map, RailFit};

const OP: &str = "sweep::loft_rails";

/// The default most wires (sections and stations together) one loft is skinned through: a
/// resource bound, not derived.
pub const DEFAULT_MAX_WIRES: usize = 257;
/// Most sections.
pub const MAX_SECTIONS: usize = 100;
/// Most curves in a section.
pub const MAX_CURVES: usize = 1000;

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

fn unsupported(reason: &'static str) -> SolidError {
    SolidError::Unsupported {
        operation: OP,
        reason,
    }
}

/// A section of a railed loft: its plane and its curves in order.
#[derive(Debug, Clone)]
pub struct RailSection<'a> {
    /// A point of its plane.
    pub origin: Point3,
    /// Its plane's normal.
    pub normal: Vec3,
    /// Its curves, in order (each a B-spline; counterparts in every section pair).
    pub curves: Vec<&'a Curve>,
}

/// How a railed loft is built.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RailLoftOptions {
    /// How the rails size a station.
    pub fit: RailFit,
    /// Whether the sections' curves close.
    pub closed: bool,
    /// The skin's degree across the wires, 1 to 3.
    pub degree: usize,
    /// Whether the loft is capped at its ends.
    pub cap: bool,
    /// The most a guided edge may stray from its rail: the certificate's tolerance, the
    /// caller's (finite and positive).
    pub tolerance: f64,
    /// The most wires, sections and stations together, the loft is skinned through: a
    /// resource bound ([`DEFAULT_MAX_WIRES`]).
    pub max_wires: usize,
}

impl RailLoftOptions {
    /// A loft certified to `tolerance`, scaled by its rails, open, uncapped, skinned cubic,
    /// through at most [`DEFAULT_MAX_WIRES`] wires.
    #[must_use]
    pub const fn new(tolerance: f64) -> Self {
        Self {
            fit: RailFit::Scale,
            closed: false,
            degree: 3,
            cap: false,
            tolerance,
            max_wires: DEFAULT_MAX_WIRES,
        }
    }
}

/// A railed loft: its skin, the wires it was skinned through, and per rail the bound proved
/// on its guided edge (each at most the tolerance).
#[derive(Debug)]
pub struct RailedLoft {
    /// The skin.
    pub skin: SkinPrimitive,
    /// The wires skinned, sections and stations together.
    pub wires: usize,
    /// Per rail, the largest bound proved between it and the skin.
    pub bounds: Vec<f64>,
}

/// A section curve as plain numbers: degree, full knots, Euclidean control points, weights
/// (`None` for a polynomial curve).
#[derive(Debug, Clone)]
pub(crate) struct Plain {
    degree: usize,
    knots: Vec<f64>,
    points: Vec<Point3>,
    weights: Option<Vec<f64>>,
}

impl Plain {
    fn of(c: &Curve) -> Result<Self> {
        // a line keeps its B-spline part's NURB unmade until asked: made on a copy here
        let base = match c.kind() {
            crate::curve::curve::CurveKind::Line(l) => l.base_with_nurb()?,
            _ => std::borrow::Cow::Borrowed(
                c.b_spline_base()
                    .ok_or_else(|| unsupported("a section curve with no NURBS form"))?,
            ),
        };
        let degree = base.degree()?;
        let (unique, mult) = base.knots(None)?;
        let mut knots = Vec::new();
        for (&x, &n) in unique.iter().zip(&mult) {
            knots.extend(std::iter::repeat_n(x, n));
        }
        let (points, weights) = base.control_polygon()?;
        if !weights.is_empty() && weights.len() != points.len() {
            return Err(invalid("a curve's weights do not match its control points"));
        }
        Ok(Self {
            degree,
            knots,
            points,
            weights: (!weights.is_empty()).then_some(weights),
        })
    }

    /// The same curve with control points `points`.
    fn with(&self, points: Vec<Point3>) -> Self {
        Self {
            points,
            ..self.clone()
        }
    }

    /// Whether `other` pairs with it (SolidCraft's `pairs`): the same degree, counts, and
    /// knots and weights within `1e-9` relative.
    fn pairs(&self, other: &Self) -> bool {
        let near = |x: f64, y: f64| (x - y).abs() <= 1e-9 * (1.0 + x.abs());
        let weights = match (&self.weights, &other.weights) {
            (None, None) => true,
            (Some(a), Some(b)) => a.len() == b.len() && a.iter().zip(b).all(|(x, y)| near(*x, *y)),
            (Some(w), None) | (None, Some(w)) => w.iter().all(|x| near(*x, 1.0)),
        };
        self.degree == other.degree
            && self.points.len() == other.points.len()
            && self.knots.len() == other.knots.len()
            && self
                .knots
                .iter()
                .zip(&other.knots)
                .all(|(x, y)| near(*x, *y))
            && weights
    }

    fn nurb(&self) -> Result<NurbCurve> {
        let mut pw = Vec::with_capacity(self.points.len());
        for (i, q) in self.points.iter().enumerate() {
            match &self.weights {
                None => pw.push(CPoint::new(q.x, q.y, q.z, NOW)),
                Some(w) => {
                    let w = *w
                        .get(i)
                        .ok_or_else(|| invalid("a weight per control point"))?;
                    pw.push(CPoint::new(q.x * w, q.y * w, q.z * w, w));
                }
            }
        }
        NurbCurve::new(pw, self.degree, self.knots.clone())
    }

    fn curve(&self) -> Result<Curve> {
        let (unique, mult) = unique(&self.knots);
        let b = BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree: self.degree,
            control_points: &self.points,
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: &mult,
            knots: &unique,
            knot_type: KnotType::Unspecified,
            weights: self.weights.as_deref(),
            trim: None,
        })?;
        Ok(Curve::from(b))
    }

    /// Its point `f` (0 to 1) of the way along its parameter range.
    fn point_at(&self, nurb: &NurbCurve, f: f64) -> Result<Point3> {
        let (Some(&t0), Some(&t1)) = (
            self.knots.get(self.degree),
            self.knots.get(self.points.len()),
        ) else {
            return Err(invalid("a curve's knots"));
        };
        nurb.eval(t0 + (t1 - t0) * f.clamp(0.0, 1.0), Side::Left)
    }
}

/// A knot vector as its distinct knots and their multiplicities.
fn unique(knots: &[f64]) -> (Vec<f64>, Vec<usize>) {
    let mut values: Vec<f64> = Vec::new();
    let mut mults: Vec<usize> = Vec::new();
    for &u in knots {
        match (values.last(), mults.last_mut()) {
            (Some(&last), Some(m)) if last == u => *m += 1,
            _ => {
                values.push(u);
                mults.push(1);
            }
        }
    }
    (values, mults)
}

/// The eight-point Gauss-Legendre rule on `[-1, 1]` (SolidCraft's centroid's).
const GAUSS_8: [(f64, f64); 8] = [
    (-0.960_289_856_497_536_3, 0.101_228_536_290_376_26),
    (-0.796_666_477_413_626_7, 0.222_381_034_453_374_47),
    (-0.525_532_409_916_329, 0.313_706_645_877_887_3),
    (-0.183_434_642_495_649_8, 0.362_683_783_378_362),
    (0.183_434_642_495_649_8, 0.362_683_783_378_362),
    (0.525_532_409_916_329, 0.313_706_645_877_887_3),
    (0.796_666_477_413_626_7, 0.222_381_034_453_374_47),
    (0.960_289_856_497_536_3, 0.101_228_536_290_376_26),
];

/// The middle of a station (its curves): the centroid of its curves by length, each knot
/// span by eight-point Gauss-Legendre (SolidCraft's `centroid`).
fn centroid(curves: &[(Plain, NurbCurve)]) -> Result<Point3> {
    let (mut sum, mut length) = (Vec3::ZERO, 0.0);
    for (plain, nurb) in curves {
        let (Some(&lo), Some(&hi)) = (
            plain.knots.get(plain.degree),
            plain.knots.get(plain.points.len()),
        ) else {
            return Err(invalid("a curve's knots"));
        };
        for w in plain.knots.windows(2) {
            let [k0, k1] = *w else {
                continue;
            };
            let (a, b) = (k0.max(lo), k1.min(hi));
            if b <= a {
                continue;
            }
            let (mid, half) = (0.5 * (a + b), 0.5 * (b - a));
            for (x, wt) in GAUSS_8 {
                let d = nurb.derivs(mid + half * x, Side::Left, 1)?;
                let (Some(&p), Some(&dp)) = (d.first(), d.get(1)) else {
                    return Err(invalid("a curve's derivative"));
                };
                let ds = wt * half * dp.length();
                sum += p * ds;
                length += ds;
            }
        }
    }
    #[allow(clippy::neg_cmp_op_on_partial_ord)] // refuses NaN too
    if !(length > 0.0) {
        return Err(invalid("a section with no length"));
    }
    Ok(sum * (1.0 / length))
}

/// A wire parameter `f` of the way from `g0` to `g1`; round a closed wire of `n` curves the
/// shorter way (SolidCraft's `interpolate_g`).
fn interpolate_g(g0: f64, g1: f64, f: f64, n: usize, closed: bool) -> f64 {
    #[allow(clippy::cast_precision_loss)] // at most MAX_CURVES
    let n = n as f64;
    if !closed {
        return (g0 + f * (g1 - g0)).clamp(0.0, n);
    }
    let mut d = g1 - g0;
    if d > n / 2.0 {
        d -= n;
    } else if d < -n / 2.0 {
        d += n;
    }
    (g0 + f * d).rem_euclid(n)
}

/// The point at wire parameter `g` of a station's curves.
fn wire_point(curves: &[(Plain, NurbCurve)], g: f64) -> Result<Point3> {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // floored, clamped
    let k = (g.floor().max(0.0) as usize).min(curves.len().saturating_sub(1));
    let (plain, nurb) = curves.get(k).ok_or_else(|| invalid("a section's curves"))?;
    #[allow(clippy::cast_precision_loss)] // at most MAX_CURVES
    let f = g - k as f64;
    plain.point_at(nurb, f)
}

/// A station's curves, and per rail its parameter and point there.
pub(crate) type StationAt = (Vec<Plain>, Vec<(f64, Point3)>);

/// An affine move: `x` goes to `to + m (x - from)`.
struct Affine {
    from: Point3,
    to: Point3,
    m: Map,
}

impl Affine {
    fn apply(&self, x: Point3) -> Point3 {
        self.to + apply(&self.m, x - self.from)
    }
}

/// The move putting a station's guided points on the rails (`pairs`: each guided point with
/// its rail's point), about the station's middle `middle`, the loft's unit direction there
/// `dir`, `size` the loft's size (SolidCraft's `station_map`; module docs).
fn station_map(
    pairs: &[(Point3, Point3)],
    middle: Point3,
    dir: Vec3,
    fit: RailFit,
    size: f64,
) -> Result<Affine> {
    let (from, to, a, r, two) = match pairs {
        [(p, q)] => (middle, middle, *p - middle, *q - middle, false),
        [(p1, q1), (p2, q2)] => (*p1, *q1, *p2 - *p1, *q2 - *q1, true),
        _ => return Err(invalid("a loft takes one or two guide rails")),
    };
    let flat = |v: Vec3| v - dir * v.dot(dir);
    let (ap, rp) = (flat(a), flat(r));
    let tiny = 1e-6 * size.max(1e-9);
    let (Ok(u0), Ok(u1)) = (ap.unitize(), rp.unitize()) else {
        return Err(shrinks(two));
    };
    #[allow(clippy::neg_cmp_op_on_partial_ord)] // refuses NaN too
    if !(ap.length() > tiny && rp.length() > tiny) {
        return Err(shrinks(two));
    }
    let mut m = rail_frame(dir, u0, dir, u1, rp.length() / ap.length(), fit);
    let lead = (r - a).dot(dir);
    if two {
        let lean = lead / ap.length();
        for (col, e) in m.iter_mut().zip([Vec3::X, Vec3::Y, Vec3::Z]) {
            *col += dir * (lean * e.dot(u0));
        }
    }
    let shift = if two { Vec3::ZERO } else { dir * lead };
    Ok(Affine {
        from,
        to: to + shift,
        m,
    })
}

fn shrinks(two: bool) -> SolidError {
    invalid(if two {
        "the two guide rails come together between the sections: the loft would shrink to nothing"
    } else {
        "a guide rail runs through the middle of the loft: it has to guide a point of the sections away from their middle"
    })
}

/// Where a rail meets a section: the guided point as a wire parameter, the point, and the
/// nudge onto it from the rail's crossing.
#[derive(Clone, Copy)]
struct Anchor {
    g: f64,
    nudge: Vec3,
}

/// The loft's sections as plain curves and the construction of its stations.
struct Loft<'a> {
    sections: Vec<Vec<(Plain, NurbCurve)>>,
    sources: &'a [RailSection<'a>],
    rails: &'a [&'a Curve],
    opts: &'a RailLoftOptions,
    along: Vec<f64>,
    /// Per (curve, control point), its curve across the sections (none for two sections).
    across: Vec<Vec<NurbCurve>>,
    normals: Vec<Vec3>,
    anchors: Vec<Vec<Anchor>>,
    size: f64,
}

impl<'a> Loft<'a> {
    fn new(
        k: &Kernel,
        sources: &'a [RailSection<'a>],
        rails: &'a [&'a Curve],
        opts: &'a RailLoftOptions,
    ) -> Result<Self> {
        let count = sources.len();
        if !(2..=MAX_SECTIONS).contains(&count) {
            return Err(invalid("a railed loft runs through 2 to 100 sections"));
        }
        if !(1..=2).contains(&rails.len()) {
            return Err(invalid("a loft takes one or two guide rails"));
        }
        if !(opts.tolerance.is_finite() && opts.tolerance > 0.0) {
            return Err(invalid("a railed loft's tolerance is finite and positive"));
        }
        if !(1..=3).contains(&opts.degree) {
            return Err(invalid("a loft's skin degree is 1 to 3"));
        }
        let mut sections = Vec::with_capacity(count);
        for s in sources {
            if s.curves.is_empty() || s.curves.len() > MAX_CURVES {
                return Err(invalid("a section has 1 to 1000 curves"));
            }
            let mut curves = Vec::new();
            for c in &s.curves {
                let plain = Plain::of(c)?;
                let nurb = plain.nurb()?;
                curves.push((plain, nurb));
            }
            sections.push(curves);
        }
        let lead = sections.first().ok_or_else(|| invalid("sections"))?;
        let n = lead.len();
        if sections
            .iter()
            .any(|s| s.len() != n || s.iter().zip(lead).any(|(a, b)| !a.0.pairs(&b.0)))
        {
            return Err(unsupported(
                "a loft along guide rails between sections whose curves do not pair (their kinds, degrees or knots differ)",
            ));
        }
        let mut lo = Point3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
        let mut hi = Point3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for (plain, _) in sections.iter().flatten() {
            for p in &plain.points {
                if !(p.x.is_finite() && p.y.is_finite() && p.z.is_finite()) {
                    return Err(invalid("a section's control point is not finite"));
                }
                lo = Point3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
                hi = Point3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
            }
        }
        let size = lo.distance(hi);
        // the sections' parameters: the mean distance between their curves' starts
        let mut along = vec![0.0];
        for w in sections.windows(2) {
            let (Some(a), Some(b)) = (w.first(), w.get(1)) else {
                continue;
            };
            let mut step = 0.0;
            for ((pa, _), (pb, _)) in a.iter().zip(b) {
                if let (Some(x), Some(y)) = (pa.points.first(), pb.points.first()) {
                    step += x.distance(*y);
                }
            }
            #[allow(clippy::cast_precision_loss)] // at most MAX_CURVES
            let step = step / n as f64;
            along.push(along.last().copied().unwrap_or(0.0) + step);
        }
        let total = along.last().copied().unwrap_or(0.0);
        #[allow(clippy::neg_cmp_op_on_partial_ord)] // refuses NaN too
        let apart = total > 1e-9
            && along.windows(2).all(|w| match *w {
                [a, b] => b - a > 1e-9 * total,
                _ => true,
            });
        if !apart {
            return Err(invalid("two loft sections sit on top of each other"));
        }
        for u in &mut along {
            *u /= total;
        }
        // the plain loft across more than two sections: each control point's curve
        let mut across = Vec::new();
        if count > 2 {
            let degree = (count - 1).min(3);
            let knots = interp_knot_vector(&along, degree)?;
            let kv = KnotVector::from_slice(&knots)?;
            for (kk, (plain, _)) in lead.iter().enumerate() {
                let mut per = Vec::new();
                for j in 0..plain.points.len() {
                    let mut pts = Vec::with_capacity(count);
                    for s in &sections {
                        let q = s
                            .get(kk)
                            .and_then(|(c, _)| c.points.get(j))
                            .ok_or_else(|| invalid("a section's control point"))?;
                        pts.push(*q);
                    }
                    per.push(interp_given_params(&pts, &along, &kv, degree)?);
                }
                across.push(per);
            }
        }
        // the sections' normals, turned to agree with the first's
        let mut normals: Vec<Vec3> = Vec::with_capacity(count);
        for s in sources {
            let nn = s
                .normal
                .unitize()
                .map_err(|_| invalid("a section plane's normal of no length"))?;
            let prev = normals.last().copied().unwrap_or(nn);
            normals.push(if nn.dot(prev) < 0.0 { nn * -1.0 } else { nn });
        }
        let mut loft = Self {
            sections,
            sources,
            rails,
            opts,
            along,
            across,
            normals,
            anchors: Vec::new(),
            size,
        };
        loft.anchors = loft.anchor_rails(k)?;
        Ok(loft)
    }

    /// Where each rail meets each section (module docs).
    fn anchor_rails(&self, k: &Kernel) -> Result<Vec<Vec<Anchor>>> {
        let tol = k.tolerances.zone_tol_3d;
        let mut out = Vec::new();
        for rail in self.rails {
            let mut anchors = Vec::new();
            for (s, curves) in self.sources.iter().zip(&self.sections) {
                let (_, hit) = rail_crossing(k, rail, s.origin, s.normal)?;
                let mut best: Option<(f64, Point3, f64)> = None;
                for (i, c) in s.curves.iter().enumerate() {
                    let near = crate::query::api_queries::curve_closest_point(k, c, hit)?;
                    let (plain, _) = curves.get(i).ok_or_else(|| invalid("a section's curves"))?;
                    let (Some(&t0), Some(&t1)) = (
                        plain.knots.get(plain.degree),
                        plain.knots.get(plain.points.len()),
                    ) else {
                        return Err(invalid("a curve's knots"));
                    };
                    let f = ((near.parameter - t0) / (t1 - t0)).clamp(0.0, 1.0);
                    let d = near.closest.distance;
                    if best.is_none_or(|b| d < b.2) {
                        #[allow(clippy::cast_precision_loss)] // at most MAX_CURVES
                        let g = i as f64 + f;
                        best = Some((g, near.closest.point, d));
                    }
                }
                let (g, on, d) = best.ok_or_else(|| invalid("a section's curves"))?;
                if d > tol {
                    return Err(invalid(
                        "a guide rail crosses a section's plane away from its curves: a rail has to pass through every section",
                    ));
                }
                anchors.push(Anchor { g, nudge: on - hit });
            }
            out.push(anchors);
        }
        if let [a, b] = out.as_slice() {
            for (p, q) in a.iter().zip(b) {
                if (p.g - q.g).abs() <= 0.0 {
                    return Err(invalid(
                        "the two guide rails meet a section at the same point",
                    ));
                }
            }
        }
        Ok(out)
    }

    /// The plain loft's control points at `v`.
    fn plain(&self, v: f64) -> Result<Vec<(Plain, NurbCurve)>> {
        let lead = self.sections.first().ok_or_else(|| invalid("sections"))?;
        let mut out = Vec::with_capacity(lead.len());
        if self.sections.len() == 2 {
            let last = self.sections.last().ok_or_else(|| invalid("sections"))?;
            for ((a, _), (b, _)) in lead.iter().zip(last) {
                let pts = a
                    .points
                    .iter()
                    .zip(&b.points)
                    .map(|(p, q)| *p + (*q - *p) * v)
                    .collect();
                let plain = a.with(pts);
                let nurb = plain.nurb()?;
                out.push((plain, nurb));
            }
            return Ok(out);
        }
        for ((a, _), per) in lead.iter().zip(&self.across) {
            let mut pts = Vec::with_capacity(per.len());
            for c in per {
                pts.push(c.eval(v, Side::Left)?);
            }
            let plain = a.with(pts);
            let nurb = plain.nurb()?;
            out.push((plain, nurb));
        }
        Ok(out)
    }

    /// The plain loft's middle at `v` and its unit direction there (SolidCraft's central
    /// difference over `1e-4`).
    fn middle(&self, v: f64) -> Result<(Point3, Vec3)> {
        let h = 1e-4;
        let d =
            centroid(&self.plain((v + h).min(1.0))?)? - centroid(&self.plain((v - h).max(0.0))?)?;
        let d = d
            .unitize()
            .map_err(|_| invalid("two loft sections sit on top of each other"))?;
        Ok((centroid(&self.plain(v)?)?, d))
    }

    /// The station `f` of the way across span `i`, at loft parameter `v`, and per rail the
    /// point it lands on (the rail's crossing, nudged) and that crossing's rail parameter.
    fn station(&self, k: &Kernel, i: usize, f: f64) -> Result<StationAt> {
        let (Some(&v0), Some(&v1), Some(&n0), Some(&n1)) = (
            self.along.get(i),
            self.along.get(i + 1),
            self.normals.get(i),
            self.normals.get(i + 1),
        ) else {
            return Err(invalid("sections"));
        };
        let v = v0 + f * (v1 - v0);
        let station = self.plain(v)?;
        let (mid, dir) = self.middle(v)?;
        let normal = (n0 + (n1 - n0) * f)
            .unitize()
            .map_err(|_| invalid("section planes turning through each other"))?;
        let n = station.len();
        let mut pairs = Vec::new();
        let mut hits = Vec::new();
        for (rail, anchors) in self.rails.iter().zip(&self.anchors) {
            let (Some(a0), Some(a1)) = (anchors.get(i), anchors.get(i + 1)) else {
                return Err(invalid("a rail's anchors"));
            };
            let anchor = wire_point(&station, interpolate_g(a0.g, a1.g, f, n, self.opts.closed))?;
            let (t, hit) = rail_crossing(k, rail, anchor, normal)?;
            let target = hit + a0.nudge * (1.0 - f) + a1.nudge * f;
            pairs.push((anchor, target));
            hits.push((t, hit));
        }
        let map = station_map(&pairs, mid, dir, self.opts.fit, self.size)?;
        let moved = station
            .iter()
            .map(|(c, _)| c.with(c.points.iter().map(|p| map.apply(*p)).collect()))
            .collect();
        Ok((moved, hits))
    }
}

/// A wire of the loft: a section's curves or a station's, at loft parameter `(span, f)`, and
/// per rail the rail's parameter and point there.
pub(crate) struct Wire {
    pub(crate) span: usize,
    pub(crate) f: f64,
    pub(crate) curves: Vec<Plain>,
    pub(crate) hits: Vec<(f64, Point3)>,
}

/// The loft through `sections` guided by `rails` (module docs).
///
/// # Errors
///
/// `InvalidInput` and `Unsupported` for the refusals the module names;
/// [`crate::error::sweep::SweepFailure::RailPastStationCap`] where the certificate is not met
/// within [`RailLoftOptions::max_wires`]; those of the crossings, projections, the skin and
/// the bound.
pub fn loft_rails(
    k: &mut Kernel,
    sections: &[RailSection<'_>],
    rails: &[&Curve],
    opts: &RailLoftOptions,
) -> Result<RailedLoft> {
    let loft = Loft::new(k, sections, rails, opts)?;
    let spans = sections.len() - 1;
    // the sections, with the rails' own crossings
    let mut wires: Vec<Wire> = Vec::new();
    for (i, s) in sections.iter().enumerate() {
        let curves = loft
            .sections
            .get(i)
            .ok_or_else(|| invalid("sections"))?
            .iter()
            .map(|(c, _)| c.clone())
            .collect();
        let mut hits = Vec::new();
        for rail in rails {
            hits.push(rail_crossing(k, rail, s.origin, s.normal)?);
        }
        let (span, f) = if i < spans {
            (i, 0.0)
        } else {
            (spans - 1, 1.0)
        };
        wires.push(Wire {
            span,
            f,
            curves,
            hits,
        });
    }
    skin_and_certify(
        k,
        wires,
        spans,
        rails,
        opts,
        &mut |k: &Kernel, i: usize, f: f64| loft.station(k, i, f),
    )
}

/// One open profile swept along two rails (module docs, "One profile along two rails"): the
/// profile's curves `profile`, end to end, its start on the start of `rails[0]` and its end on
/// the start of `rails[1]` (or the other way round), placed at stations of the rails' common
/// parameter and skinned, each rail certified within `opts.tolerance` of the skin's guided edge
/// (`skin_and_certify`). `opts.closed` and `opts.cap` must be false. The bounds are per rail,
/// in the order given.
///
/// # Errors
///
/// `InvalidInput` and `Unsupported` for the refusals the module names;
/// [`crate::error::sweep::SweepFailure::RailPastStationCap`] where the certificate is not met
/// within [`RailLoftOptions::max_wires`]; those of the evaluations, the skin and the bound.
pub fn sweep_two_rails(
    k: &mut Kernel,
    profile: &[&Curve],
    rails: [&Curve; 2],
    opts: &RailLoftOptions,
) -> Result<RailedLoft> {
    if !(opts.tolerance.is_finite() && opts.tolerance > 0.0) {
        return Err(invalid("a sweep's tolerance is finite and positive"));
    }
    if !(1..=3).contains(&opts.degree) {
        return Err(invalid("a sweep's skin degree is 1 to 3"));
    }
    if opts.closed || opts.cap {
        return Err(unsupported(
            "a closed or capped two-rail sweep: two-rail sweeps take open profiles",
        ));
    }
    if profile.is_empty() || profile.len() > MAX_CURVES {
        return Err(invalid("a profile has 1 to 1000 curves"));
    }
    let tol = k.tolerances.zone_tol_3d;
    let mut curves = Vec::new();
    for c in profile {
        let plain = Plain::of(c)?;
        let nurb = plain.nurb()?;
        curves.push((plain, nurb));
    }
    // end to end, open
    let mut ends = Vec::new();
    for (plain, nurb) in &curves {
        ends.push((plain.point_at(nurb, 0.0)?, plain.point_at(nurb, 1.0)?));
    }
    for w in ends.windows(2) {
        if let [(_, end), (start, _)] = w {
            if end.distance(*start) > tol {
                return Err(invalid("a profile's curves do not run end to end"));
            }
        }
    }
    let (Some(&(a0, _)), Some(&(_, b0))) = (ends.first(), ends.last()) else {
        return Err(invalid("a profile has 1 to 1000 curves"));
    };
    if a0.distance(b0) <= tol {
        return Err(unsupported(
            "a closed profile: two-rail sweeps take open profiles",
        ));
    }
    // which rail starts at the profile's start
    let [r0, r1] = rails;
    let (s0, s1) = (rail_at(k, r0, 0.0)?, rail_at(k, r1, 0.0)?);
    let swapped = if s0.1.distance(a0) <= tol && s1.1.distance(b0) <= tol {
        false
    } else if s1.1.distance(a0) <= tol && s0.1.distance(b0) <= tol {
        true
    } else {
        return Err(invalid(
            "a profile whose ends are not on the rails' starts: each rail starts at an end of the profile",
        ));
    };
    let ordered: [&Curve; 2] = if swapped { [r1, r0] } else { [r0, r1] };
    let mut lo = Point3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut hi = Point3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for (plain, _) in &curves {
        for q in &plain.points {
            if !(q.x.is_finite() && q.y.is_finite() && q.z.is_finite()) {
                return Err(invalid("a profile's control point is not finite"));
            }
            lo = Point3::new(lo.x.min(q.x), lo.y.min(q.y), lo.z.min(q.z));
            hi = Point3::new(hi.x.max(q.x), hi.y.max(q.y), hi.z.max(q.z));
        }
    }
    let (a, b) = if swapped { (s1, s0) } else { (s0, s1) };
    if a.2.dot(b.2) <= 0.0 {
        return Err(invalid(
            "two rails starting the opposite ways: they run from the profile's two ends the same way",
        ));
    }
    let chord = b.1 - a.1;
    let size = lo.distance(hi).max(chord.length());
    let (across, d) = chord_frame(chord, a.2, b.2, size)?;
    let sweep = Birail {
        profile: curves.iter().map(|(p, _)| p.clone()).collect(),
        rails: ordered,
        origin: a.1,
        along: d,
        across,
        length: chord.length(),
        size,
        fit: opts.fit,
    };
    let start = Wire {
        span: 0,
        f: 0.0,
        curves: sweep.profile.clone(),
        hits: vec![(a.0, a.1), (b.0, b.1)],
    };
    let (curves1, hits1) = sweep.station(k, 1.0)?;
    let end = Wire {
        span: 0,
        f: 1.0,
        curves: curves1,
        hits: hits1,
    };
    let rail_list: Vec<&Curve> = ordered.to_vec();
    let mut made = skin_and_certify(
        k,
        vec![start, end],
        1,
        &rail_list,
        opts,
        &mut |k: &Kernel, _span: usize, f: f64| sweep.station(k, f),
    )?;
    if swapped {
        made.bounds.reverse();
    }
    Ok(made)
}

/// Rail `c` at the common parameter `f` (its natural range mapped linearly onto `[0, 1]`): the
/// parameter, the point and the unit tangent.
fn rail_at(k: &Kernel, c: &Curve, f: f64) -> Result<(f64, Point3, Vec3)> {
    let iv = c.natural_interval();
    let t = iv.min() + (iv.max() - iv.min()) * f;
    let d = c.evaluate(k, t, 1, f >= 1.0, true)?;
    let (Some(&q), Some(&dq)) = (d.first(), d.get(1)) else {
        return Err(invalid("a rail's derivative"));
    };
    let dq = dq
        .unitize()
        .map_err(|_| invalid("a rail with no tangent"))?;
    Ok((t, q, dq))
}

/// The unit chord and the direction of travel at a station: the chord `chord` between the
/// rails' points, the mean of their unit tangents `t0`, `t1` made square to it.
fn chord_frame(chord: Vec3, t0: Vec3, t1: Vec3, size: f64) -> Result<(Vec3, Vec3)> {
    #[allow(clippy::neg_cmp_op_on_partial_ord)] // refuses NaN too
    if !(chord.length() > 1e-6 * size.max(1e-9)) {
        return Err(invalid(
            "the two rails come together: the sweep would shrink to nothing",
        ));
    }
    let across = chord
        .unitize()
        .map_err(|_| invalid("the two rails come together"))?;
    let mean = (t0 + t1) * 0.5;
    let along = mean - across * mean.dot(across);
    #[allow(clippy::neg_cmp_op_on_partial_ord)] // refuses NaN too
    if !(along.length() > 1e-6) {
        return Err(invalid(
            "the rails run along the line between them: the sweep has no direction",
        ));
    }
    let along = along
        .unitize()
        .map_err(|_| invalid("the rails run along the line between them"))?;
    Ok((across, along))
}

/// A two-rail sweep's construction: its profile and the frame it is drawn in at the start.
struct Birail<'a> {
    profile: Vec<Plain>,
    rails: [&'a Curve; 2],
    /// The first rail's start, where the profile's start lies.
    origin: Point3,
    /// The direction of travel at the start.
    along: Vec3,
    /// The unit chord at the start.
    across: Vec3,
    /// The chord's length at the start.
    length: f64,
    size: f64,
    fit: RailFit,
}

impl Birail<'_> {
    /// The profile placed at the common parameter `f`, and each rail's parameter and point there.
    fn station(&self, k: &Kernel, f: f64) -> Result<StationAt> {
        let [r0, r1] = self.rails;
        let (a, b) = (rail_at(k, r0, f)?, rail_at(k, r1, f)?);
        let chord = b.1 - a.1;
        let (across, along) = chord_frame(chord, a.2, b.2, self.size)?;
        let m = rail_frame(
            self.along,
            self.across,
            along,
            across,
            chord.length() / self.length,
            self.fit,
        );
        let placed = Affine {
            from: self.origin,
            to: a.1,
            m,
        };
        let curves = self
            .profile
            .iter()
            .map(|c| c.with(c.points.iter().map(|q| placed.apply(*q)).collect()))
            .collect();
        Ok((curves, vec![(a.0, a.1), (b.0, b.1)]))
    }
}

/// The loft's skin-and-certificate loop (module docs, "Where the stations go, and the
/// certificate"): from the wires `wires` (the sections, each with its rails' crossings, `spans`
/// spans between them), a station placed by `station(k, span, f)` at each span's middle, the
/// wires skinned, each rail bounded against the skin on every span between neighbouring wires,
/// and a span whose bound is above the tolerance given a station at its middle, until every
/// span is certified; past [`RailLoftOptions::max_wires`] refused, naming the bound reached.
///
/// # Errors
///
/// [`crate::error::sweep::SweepFailure::RailPastStationCap`] past the wire cap; those of
/// `station`, the skin and the bound.
fn skin_and_certify(
    k: &mut Kernel,
    mut wires: Vec<Wire>,
    spans: usize,
    rails: &[&Curve],
    opts: &RailLoftOptions,
    station: &mut dyn FnMut(&Kernel, usize, f64) -> Result<StationAt>,
) -> Result<RailedLoft> {
    // one station at each span's middle
    let mut pending: Vec<(usize, f64)> = (0..spans).map(|i| (i, 0.5)).collect();
    // the largest bound the last skin proved (none before the first)
    let mut reached = f64::INFINITY;
    loop {
        if wires.len() + pending.len() > opts.max_wires {
            return Err(crate::error::Failure::new(
                OP,
                crate::error::sweep::SweepFailure::RailPastStationCap {
                    asked: opts.tolerance,
                    reached,
                    wires: wires.len(),
                },
            )
            .into_error());
        }
        for (i, f) in pending.drain(..) {
            let (curves, hits) = station(&*k, i, f)?;
            wires.push(Wire {
                span: i,
                f,
                curves,
                hits,
            });
        }
        wires.sort_by(|a, b| a.span.cmp(&b.span).then(a.f.total_cmp(&b.f)));
        let skin = skin_of(k, &wires, opts)?;
        let (bounds, failing) = certify(k, &skin, &wires, rails, opts.tolerance)?;
        reached = bounds.iter().copied().fold(0.0_f64, f64::max);
        if failing.is_empty() {
            return Ok(RailedLoft {
                skin,
                wires: wires.len(),
                bounds,
            });
        }
        for j in failing {
            let (Some(a), Some(b)) = (wires.get(j), wires.get(j + 1)) else {
                continue;
            };
            // the middle of the two wires' loft parameters: within one span (a section ends
            // a span at f 1 and starts the next at f 0)
            let (span, f) = if a.span == b.span {
                (a.span, 0.5 * (a.f + b.f))
            } else {
                (a.span, 0.5 * (a.f + 1.0))
            };
            pending.push((span, f));
        }
    }
}

/// The skin through the wires.
fn skin_of(k: &mut Kernel, wires: &[Wire], opts: &RailLoftOptions) -> Result<SkinPrimitive> {
    let mut curves = Vec::new();
    let mut counts = Vec::new();
    for w in wires {
        counts.push(w.curves.len());
        for c in &w.curves {
            curves.push(c.curve()?);
        }
    }
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_skin_primitive(k, &counts, &refs, opts.degree, opts.cap)
}

/// Per rail the largest bound between it and the skin over all spans between neighbouring
/// wires, and the spans (by their first wire) whose bound is above `tol`.
fn certify(
    k: &Kernel,
    skin: &SkinPrimitive,
    wires: &[Wire],
    rails: &[&Curve],
    tol: f64,
) -> Result<(Vec<f64>, Vec<usize>)> {
    let mut bounds = Vec::new();
    let mut failing: Vec<usize> = Vec::new();
    let surfaces: Vec<&crate::surface::surface::Surface> = skin
        .side_faces
        .iter()
        .filter_map(|&f| {
            skin.brep
                .face(f)
                .ok()
                .and_then(|face| face.surface.as_ref())
        })
        .collect();
    for (r, rail) in rails.iter().enumerate() {
        let nurb = Plain::of(rail)?.nurb()?;
        // each wire's rail point found on every skin surface: (surface, parameters, distance)
        let mut feet: Vec<Vec<Option<(Point2, f64)>>> = Vec::with_capacity(wires.len());
        let mut hits = Vec::with_capacity(wires.len());
        for w in wires {
            let (t, q) = *w
                .hits
                .get(r)
                .ok_or_else(|| invalid("a wire's rail crossing"))?;
            feet.push(
                surfaces
                    .iter()
                    .map(|s| certificate::node_on(k, s, q).ok())
                    .collect(),
            );
            hits.push((t, q));
        }
        let mut worst = 0.0_f64;
        for j in 0..wires.len().saturating_sub(1) {
            let (Some(&(ta, qa)), Some(&(tb, _)), Some(fa), Some(fb)) =
                (hits.get(j), hits.get(j + 1), feet.get(j), feet.get(j + 1))
            else {
                continue;
            };
            // the span's surface: the one both ends lie nearest at once (the least of the larger
            // of their two distances; a rail along an edge between two faces lies on both, and
            // either serves: the bound holds on any)
            let mut pick: Option<(usize, Point2, Point2, f64)> = None;
            for (si, (a, b)) in fa.iter().zip(fb).enumerate() {
                if let (Some((uva, da)), Some((uvb, db))) = (a, b) {
                    let far = da.max(*db);
                    if pick.is_none_or(|p| far < p.3) {
                        pick = Some((si, *uva, *uvb, far));
                    }
                }
            }
            let (si, uva, uvb, _) =
                pick.ok_or_else(|| unsupported("a rail point with no foot on the loft"))?;
            let surf = surfaces.get(si).ok_or_else(|| invalid("a skin surface"))?;
            let spans = certificate::RailSpans::about(&nurb, qa)?;
            let bound = certificate::span_bound(
                k,
                surf,
                rail,
                &spans,
                qa,
                certificate::Node { t: ta, uv: uva },
                certificate::Node { t: tb, uv: uvb },
                tol,
            )?;
            worst = worst.max(bound);
            if bound > tol && !failing.contains(&j) {
                failing.push(j);
            }
        }
        bounds.push(worst);
    }
    failing.sort_unstable();
    Ok((bounds, failing))
}
