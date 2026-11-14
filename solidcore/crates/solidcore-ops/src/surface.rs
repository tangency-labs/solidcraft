//! Patches bounded by curves: the Coons patch of four curves meeting end to end round one loop
//! (three: its degenerate case, one side collapsed to a corner) and the ruled surface between
//! two curves, each a new one-face sheet record whose sides are the curves themselves. The
//! curves are the record's ([`Curve`]), each over its natural range; the geometry is the
//! kernel's (`make_curves_compatible`, `BSplineSurface::create_coons_patch`,
//! `create_ruled_surface`), and so is every distance compared. Ops checks the inputs, turns the
//! curves to run end to end, sequences the kernel's calls and converts.

use solidcore::curve::curve::CurveKind;
use solidcore::curve::from_parameters::from_parameters;
use solidcore::geom::certified::Bounded;
use solidcore::geom::tol::{context_zone_tol_3d, SmallTopoType, USE_DEFAULT};
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, Ends};
use solidcore::nurbs::b_spline_surface::{BSplineSurface, SurfParamType};
use solidcore::nurbs::n_sided::{n_sided as kernel_n_sided, NSidedRefusal, MAX_SIDES, MIN_SIDES};
use solidcore::primitive::api_curve_ops::make_curves_compatible;
use solidcore::query::api_queries::point_distance;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p3::new_brep;
use solidcore::topology::brep::p4::stitch_faces;
use solidcore::topology::brep::sheet::{coons_patch_of_loop, face_on};
use solidcore::topology::Brep;
use solidcore::SolidError;
use solidcore_record::brep::{Brep as Record, Curve, Natural};

use crate::body::{finish, Body, Expect};
use crate::error::status;
use crate::session::run;
use crate::sweep::{coord_ok, CURVE_MAX_POINTS};
use crate::{Error, Result};

/// A call's own refusal, else the value: as the kernel's calls answer inside [`run`].
pub(crate) type Out<T> = std::result::Result<T, Error>;

/// A call's own refusal inside a kernel call.
macro_rules! refused {
    ($e:expr) => {
        match $e {
            Ok(c) => c,
            Err(e) => return Ok(Err(e)),
        }
    };
}

fn point(p: [f64; 3]) -> Point3 {
    Vec3::new(p[0], p[1], p[2])
}

/// A kernel refusal of the arguments as [`Error::Argument`], any other error as its status.
fn argument_or_status(e: SolidError) -> Error {
    match e {
        SolidError::InvalidInput { .. } => Error::Argument,
        e => status(e),
    }
}

/// `curve` over its natural range as the kernel's B-spline: a line from its start to its end,
/// a polyline over all its segments, a spline over its knots' domain. A circle, an ellipse
/// (no ends of their own) and a traced intersection curve are refused.
fn spline(k: &Kernel, curve: &Curve) -> solidcore::Result<Out<BSplineCurve>> {
    let count = match curve {
        Curve::Line { start, end } => {
            if !coord_ok(start) || !coord_ok(end) {
                return Ok(Err(Error::Argument));
            }
            return Ok(
                BSplineCurve::create_line_segment(3, point(*start), point(*end))
                    .map_err(argument_or_status),
            );
        }
        Curve::Polyline { points } | Curve::Spline { points, .. } => points.len(),
        Curve::RationalSpline { homogeneous, .. } => homogeneous.len(),
        Curve::Circle { .. } | Curve::Ellipse { .. } | Curve::SurfaceIntersection { .. } => {
            return Ok(Err(Error::Argument));
        }
    };
    if count > CURVE_MAX_POINTS {
        return Ok(Err(Error::Argument));
    }
    let Natural::Range(lo, hi) = curve.natural() else {
        return Ok(Err(Error::Argument));
    };
    let Ok(record) = solidcore_convert::curve(curve) else {
        return Ok(Err(Error::Argument));
    };
    let made = match from_parameters(k, &record, [lo, hi]) {
        Ok(made) => made,
        Err(SolidError::InvalidInput { .. }) => return Ok(Err(Error::Argument)),
        Err(e) => return Err(e),
    };
    Ok(match made.curve.kind() {
        CurveKind::BSpline(b) => Ok(b.clone()),
        _ => Err(Error::Argument),
    })
}

/// Each record curve as the kernel's B-spline ([`spline`]).
pub(crate) fn splines(k: &Kernel, curves: &[Curve]) -> solidcore::Result<Out<Vec<BSplineCurve>>> {
    let mut out = Vec::new();
    for c in curves {
        out.push(refused!(spline(k, c)?));
    }
    Ok(Ok(out))
}

/// The kernel's distance between `a` and `b`, with the bound on its rounding.
fn apart(a: Point3, b: Point3) -> solidcore::Result<Bounded> {
    let d = point_distance(a, b)?;
    Ok(Bounded {
        value: d.value,
        bound: d.bound,
    })
}

/// Which end of a curve with `ends` is proved within `tol` of `p` (`false` its start, `true`
/// its end; the start where both are), else how far the nearer end is.
fn end_at(p: Point3, ends: &Ends, tol: f64) -> solidcore::Result<std::result::Result<bool, f64>> {
    let (start, end) = (apart(p, ends.start)?, apart(p, ends.end)?);
    Ok(if start.at_most(tol).is_yes() {
        Ok(false)
    } else if end.at_most(tol).is_yes() {
        Ok(true)
    } else if end.value < start.value {
        Err(end.value)
    } else {
        Err(start.value)
    })
}

/// `c` running the other way over the same interval (the kernel's reversal).
fn reverse(c: &mut BSplineCurve) -> solidcore::Result<()> {
    let interval = c.natural_interval();
    c.reverse_parameterization(&interval).map(|_| ())
}

/// `curves`, in their order, each turned to run from where the one before it ends, round one
/// loop: the first runs towards an end of the second, and every end meets the next curve's
/// start (the last's the first's) within `tol`, as the kernel's distance proves. A join not
/// proved is [`Error::BoundaryGap`].
fn looped(curves: &mut [BSplineCurve], tol: f64) -> solidcore::Result<Out<()>> {
    let (Some(first), Some(second)) = (curves.first(), curves.get(1)) else {
        return Ok(Err(Error::Argument));
    };
    let (a, b) = (first.ends()?, second.ends()?);
    match (end_at(a.end, &b, tol)?, end_at(a.start, &b, tol)?) {
        (Ok(_), _) => {}
        (Err(_), Ok(_)) => {
            if let Some(first) = curves.first_mut() {
                reverse(first)?;
            }
        }
        (Err(g), Err(h)) => {
            let gap = if h < g { h } else { g };
            return Ok(Err(Error::BoundaryGap { join: 0, gap }));
        }
    }
    let (mut end, start) = match curves.first() {
        Some(c) => {
            let e = c.ends()?;
            (e.end, e.start)
        }
        None => return Ok(Err(Error::Argument)),
    };
    for (i, c) in curves.iter_mut().enumerate().skip(1) {
        match end_at(end, &c.ends()?, tol)? {
            Ok(false) => {}
            Ok(true) => reverse(c)?,
            Err(gap) => {
                return Ok(Err(Error::BoundaryGap {
                    join: i.saturating_sub(1),
                    gap,
                }));
            }
        }
        end = c.ends()?.end;
    }
    let closing = apart(end, start)?;
    if !closing.at_most(tol).is_yes() {
        return Ok(Err(Error::BoundaryGap {
            join: curves.len().saturating_sub(1),
            gap: closing.value,
        }));
    }
    Ok(Ok(()))
}

/// A sheet of one face on `surface`, over its whole natural box.
pub(crate) fn sheet(k: &mut Kernel, surface: BSplineSurface) -> solidcore::Result<Out<Brep>> {
    sheet_starting(k, surface, None)
}

/// [`sheet`], its loop starting at the edge on side `first` of the box when given (the kernel's
/// `start_at_side`): a sheet made again on an edited surface lists its edges as the one it
/// replaces did.
pub(crate) fn sheet_starting(
    k: &mut Kernel,
    surface: BSplineSurface,
    first: Option<solidcore::surface::side::BoxSide>,
) -> solidcore::Result<Out<Brep>> {
    let mut b = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
    let surface = Surface::from(surface);
    let bx = surface.natural_uv_domain_with_nurb(k)?;
    let f = face_on(k, &mut b, surface, &bx)?;
    b.refresh_cached_topology();
    if let Some(first) = first {
        solidcore::topology::brep::sheet::start_at_side(k, &mut b, f, first)?;
    }
    finish(k, b, Expect::FACES)
}

/// A new body's record.
pub(crate) fn record(made: Result<Out<Brep>>) -> Result<Record> {
    Body::new(made??).to_record().map(|(r, _)| r)
}

/// `a` and `b` made compatible, exactly: one degree (the higher, by degree elevation), one
/// parameter range (`[0, 1]` when theirs differ, by a linear change of parameter) and one knot
/// vector (by knot insertion), each curve's shape unchanged (the kernel's
/// `make_curves_compatible`).
fn compatible(a: BSplineCurve, b: BSplineCurve) -> solidcore::Result<[BSplineCurve; 2]> {
    let mut pair = [a, b];
    make_curves_compatible(&mut pair)?;
    Ok(pair)
}

/// The patch bounded by `curves`, which meet end to end round one loop in the order given
/// (each may run either way: it is turned to run on from the one before).
///
/// Four curves bound the bilinear Coons patch of its four sides, one face. Opposite sides are
/// first made compatible (one degree, parameter range and knot vector each pair, exactly), so
/// the patch's sides are the four curves themselves: the patch at the side of one curve is that
/// curve, point for point at the same fraction of its range (a rational curve, an arc, stays the
/// same curve but not the same parameter: the kernel gives the four common end weights). Three curves bound the same patch
/// with its fourth side collapsed to the point where the third curve meets the first (a
/// degenerate Coons patch: one side of its box is that corner). Five or more bound the
/// certified n-sided fill ([`n_sided`]), whose certificate this drops. Each curve is taken over its
/// natural range (a line from its start to its end, a spline over its knots' domain).
///
/// The ends must meet within the model's zone tolerance: the kernel's tolerance within which
/// two points are one point of the model (`context_zone_tol_3d`), compared with the kernel's
/// distance between the ends and the bound on its rounding, so a join is taken only where it is
/// proved. The patch's corners are the middles of the ends that meet there, within that
/// tolerance of each curve's end.
///
/// # Errors
/// [`Error::Argument`] for fewer than three curves, or a curve refused (a circle, an ellipse, a
/// traced intersection curve, numbers the kernel refuses); [`Error::BoundaryGap`] for curves
/// that do not meet; for five or more, those of [`n_sided`]; the kernel's status.
pub fn boundary(curves: &[Curve]) -> Result<Record> {
    if curves.len() < 3 {
        return Err(Error::Argument);
    }
    if curves.len() > 4 {
        return n_sided(curves).map(|(sheet, _)| sheet);
    }
    record(run("surface::boundary", |k| {
        let mut sides = refused!(splines(k, curves)?);
        let tol = context_zone_tol_3d(k, None, SmallTopoType::NormalSize);
        refused!(looped(&mut sides, tol)?);
        // c0 runs P0 to P1, c1 P1 to P2, c2 P2 to P3 and c3 P3 to P0 (three curves: c2 runs P2
        // to P0, and the fourth side stays at P0). The kernel's sides, in its order: P0 to P1 and
        // P3 to P2 (running in u, at v min and max), P0 to P3 and P1 to P2 (running in v, at u
        // min and max).
        match coons_patch_of_loop(sides, tol) {
            Ok(s) => sheet(k, s),
            Err(e) => Ok(Err(status(e))),
        }
    }))
}

/// The n-sided patch bounded by `curves` (three or more, meeting end to end round one loop in
/// the order given, each turned to run on from the one before, as [`boundary`] takes them): the
/// kernel's fill, N four-sided faces round a centre, each bounded by halves of two of the curves
/// and two inner curves from their middles to the centre, stitched into one sheet; and the
/// certified bound within which every curve is the sheet's boundary (`solidcore::nurbs::n_sided`).
/// The faces meet along the inner curves within the model's zone tolerance (G0, certified);
/// tangency across them is the fill's aim, within its angle tolerance, not certified.
///
/// # Errors
/// As [`boundary`] for the curves and their joins; [`Error::Unsupported`] for a rational curve
/// (an arc: the fill takes polynomial curves) or more than 64 curves; [`Error::PastTolerance`]
/// for a boundary or an inner join not proved within the zone tolerance; the kernel's status.
pub fn n_sided(curves: &[Curve]) -> Result<(Record, f64)> {
    if curves.len() < MIN_SIDES {
        return Err(Error::Argument);
    }
    if curves.len() > MAX_SIDES {
        return Err(Error::Unsupported(
            "an n-sided patch of more than 64 curves",
        ));
    }
    let mut bound = None;
    let made = run("surface::n_sided", |k| {
        let mut sides = refused!(splines(k, curves)?);
        let tol = context_zone_tol_3d(k, None, SmallTopoType::NormalSize);
        refused!(looped(&mut sides, tol)?);
        match kernel_n_sided(&sides, tol)? {
            Ok(fill) => {
                bound = Some(fill.bound);
                sheet_of(k, fill.patches)
            }
            Err(NSidedRefusal::Rational) => Ok(Err(Error::Unsupported(
                "a rational curve bounding an n-sided patch",
            ))),
            Err(NSidedRefusal::PastTolerance { bound }) => Ok(Err(Error::PastTolerance { bound })),
            Err(NSidedRefusal::Joins { gap }) => Ok(Err(Error::PastTolerance { bound: gap })),
        }
    });
    let sheet = record(made)?;
    Ok((sheet, bound.ok_or(Error::Exception)?))
}

/// A sheet of one face on each of `patches`, over its whole natural box, stitched along the
/// sides they share.
fn sheet_of(k: &mut Kernel, patches: Vec<BSplineSurface>) -> solidcore::Result<Out<Brep>> {
    let mut b = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
    for s in patches {
        let s = Surface::from(s);
        let bx = s.natural_uv_domain_with_nurb(k)?;
        face_on(k, &mut b, s, &bx)?;
    }
    b.refresh_cached_topology();
    let tol = b.tolerance();
    if let Err(e) = stitch_faces(k, &mut b, tol, None, false) {
        return Ok(Err(status(e)));
    }
    b.refresh_cached_topology();
    finish(k, b, Expect::FACES)
}

/// The ruled surface between `a` and `b`, one face: `S(s, t) = (1 - t) a(s) + t b(s)`, `s` the
/// same fraction of each curve's natural range (the two first made compatible, exactly, as in
/// [`boundary`]), `t` from 0 on `a` to 1 on `b`. So the surface's sides are the two curves
/// themselves. With `reversed`, `b` runs the other way first (its end is joined to `a`'s start).
///
/// # Errors
/// [`Error::Argument`] for a curve refused (as [`boundary`]); the kernel's status.
pub fn ruled(a: &Curve, b: &Curve, reversed: bool) -> Result<Record> {
    record(run("surface::ruled", |k| {
        let a = refused!(spline(k, a)?);
        let mut b = refused!(spline(k, b)?);
        if reversed {
            reverse(&mut b)?;
        }
        let [a, b] = compatible(a, b)?;
        match BSplineSurface::create_ruled_surface(&a, &b, SurfParamType::V) {
            Ok(s) => sheet(k, s),
            Err(e) => Ok(Err(status(e))),
        }
    }))
}

/// `sheet` turned over ([`Body::turn_over`], `SmFace::SwapUV` on each face): each face's normal
/// is the other way, its surface's u and v swapped (a spline's net transposed, its box and
/// pcurves swapped to match), each face used in its shell as it was, the edges the same edges.
/// Returns the turned record and, for each record edge of `sheet`, its index in it: the record
/// numbers edges by its canonical walk, which a turned face's loops take the other way round, so
/// picks and history held by edge index go through this map. Turning a sheet over twice gives it
/// back.
///
/// # Errors
/// [`Error::Unsupported`] for a body with a closed shell (turned over, it would be inside out);
/// those of [`Body::from_record`] and [`Body::turn_over`].
pub fn reverse_sheet(sheet: &Record) -> Result<(Record, Vec<u32>)> {
    let (body, order) = Body::from_record(sheet, true)?;
    let turned = body.turn_over(&order)?;
    Ok((turned.record, turned.edges))
}

#[cfg(test)]
// The tests check against a sphere's radius; ops itself computes none.
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use solidcore::eval::SurfaceEval;
    use solidcore_record::brep::{Face, Frame};

    fn line(start: [f64; 3], end: [f64; 3]) -> Curve {
        Curve::Line { start, end }
    }

    /// The point of `face` at the fractions `s`, `t` of its domain.
    fn at(face: &Face, s: f64, t: f64) -> [f64; 3] {
        let surface = face.nurbs.as_ref().unwrap_or(&face.surface);
        let eval = SurfaceEval::new(&solidcore_convert::surface(surface).unwrap()).unwrap();
        let [[u0, u1], [v0, v1]] = face.domain;
        let p = eval
            .point([u0 + s * (u1 - u0), v0 + t * (v1 - v0)])
            .unwrap();
        [p.x, p.y, p.z]
    }

    fn near(a: [f64; 3], b: [f64; 3], tol: f64) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < tol)
    }

    fn lerp(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
        [
            a[0] + t * (b[0] - a[0]),
            a[1] + t * (b[1] - a[1]),
            a[2] + t * (b[2] - a[2]),
        ]
    }

    /// A twisted quadrilateral's corners.
    const P: [[f64; 3]; 4] = [
        [0.0, 0.0, 0.0],
        [10.0, 0.0, 0.0],
        [10.0, 10.0, 5.0],
        [0.0, 10.0, 0.0],
    ];

    #[test]
    fn four_lines_bound_the_bilinear_patch() {
        // The second line runs backwards: it is turned to run on from the first.
        let curves = [
            line(P[0], P[1]),
            line(P[2], P[1]),
            line(P[2], P[3]),
            line(P[3], P[0]),
        ];
        let sheet = boundary(&curves).unwrap();
        assert_eq!(sheet.faces.len(), 1);
        let face = &sheet.faces[0];
        let corners = [
            at(face, 0.0, 0.0),
            at(face, 1.0, 0.0),
            at(face, 0.0, 1.0),
            at(face, 1.0, 1.0),
        ];
        // its corners are the quadrilateral's
        for p in P {
            assert!(
                corners.iter().any(|c| near(*c, p, 1e-9)),
                "{p:?} in {corners:?}"
            );
        }
        // the middle of each line is on it, at the middle of a side
        let sides = [
            at(face, 0.5, 0.0),
            at(face, 0.5, 1.0),
            at(face, 0.0, 0.5),
            at(face, 1.0, 0.5),
        ];
        for i in 0..4 {
            let m = lerp(P[i], P[(i + 1) % 4], 0.5);
            assert!(
                sides.iter().any(|c| near(*c, m, 1e-9)),
                "{m:?} in {sides:?}"
            );
        }
        // and every point is the bilinear blend of its corners
        for s in [0.1, 0.25, 0.5, 0.8] {
            for t in [0.2, 0.5, 0.7] {
                let blend = lerp(
                    lerp(corners[0], corners[1], s),
                    lerp(corners[2], corners[3], s),
                    t,
                );
                assert!(near(at(face, s, t), blend, 1e-9), "at ({s}, {t})");
            }
        }
    }

    /// The quadratic spline from `a` to `b` whose middle control point stands `up` above the
    /// chord's middle.
    fn bulge(a: [f64; 3], b: [f64; 3], up: f64) -> Curve {
        let m = lerp(a, b, 0.5);
        Curve::Spline {
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            points: vec![a, [m[0], m[1], m[2] + up], b],
        }
    }

    /// The point of a quadratic Bézier spline at `s`.
    fn on(c: &Curve, s: f64) -> [f64; 3] {
        let Curve::Spline { points, .. } = c else {
            panic!("a spline")
        };
        lerp(
            lerp(points[0], points[1], s),
            lerp(points[1], points[2], s),
            s,
        )
    }

    /// The four sides of `face`, each at the fraction `s` of its range.
    fn sides_at(face: &Face, s: f64) -> [[f64; 3]; 4] {
        [
            at(face, s, 0.0),
            at(face, s, 1.0),
            at(face, 0.0, s),
            at(face, 1.0, s),
        ]
    }

    /// Whether the point of `c` at every sampled fraction is a side of `face` at that fraction
    /// or at its complement (a side may run either way).
    fn a_side_of(face: &Face, c: &Curve) -> bool {
        (0..=8).all(|i| {
            let s = f64::from(i) / 8.0;
            let p = on(c, s);
            sides_at(face, s)
                .iter()
                .chain(&sides_at(face, 1.0 - s))
                .any(|q| near(*q, p, 1e-9))
        })
    }

    #[test]
    fn four_splines_are_the_sides_of_their_patch() {
        let curves: Vec<Curve> = (0..4)
            .map(|i| bulge(P[i], P[(i + 1) % 4], 1.0 + i as f64))
            .collect();
        let sheet = boundary(&curves).unwrap();
        assert_eq!(sheet.faces.len(), 1);
        for c in &curves {
            assert!(a_side_of(&sheet.faces[0], c), "{c:?}");
        }
    }

    #[test]
    fn three_splines_bound_a_patch_with_a_side_at_a_corner() {
        let p = [[0.0, 0.0, 0.0], [40.0, 0.0, 0.0], [20.0, 35.0, 0.0]];
        let curves: Vec<Curve> = (0..3).map(|i| bulge(p[i], p[(i + 1) % 3], 1.2)).collect();
        let sheet = boundary(&curves).unwrap();
        assert_eq!(sheet.faces.len(), 1);
        let face = &sheet.faces[0];
        for c in &curves {
            assert!(a_side_of(face, c), "{c:?}");
        }
        // the fourth side is the corner where the third curve meets the first
        let collapsed = [0.0, 0.25, 0.5, 1.0].map(|s| sides_at(face, s));
        assert!(
            (0..4).any(|k| collapsed.iter().all(|sides| near(sides[k], p[0], 1e-9))),
            "{collapsed:?}"
        );
    }

    #[test]
    fn three_arcs_bound_an_octant_of_a_sphere() {
        // three quarter circles of radius 10 (rational) meeting round a loop
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let quarter = |a: [f64; 3], corner: [f64; 3], b: [f64; 3]| Curve::RationalSpline {
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            homogeneous: vec![
                [a[0], a[1], a[2], 1.0],
                [corner[0] * w, corner[1] * w, corner[2] * w, w],
                [b[0], b[1], b[2], 1.0],
            ],
        };
        let (x, y, z) = ([10.0, 0.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 10.0]);
        let arcs = [
            quarter(x, [10.0, 10.0, 0.0], y),
            quarter(y, [0.0, 10.0, 10.0], z),
            quarter(z, [10.0, 0.0, 10.0], x),
        ];
        let sheet = boundary(&arcs).unwrap();
        assert_eq!(sheet.faces.len(), 1);
        // every side's point is on the sphere the arcs lie on
        for i in 0..=8 {
            for q in sides_at(&sheet.faces[0], f64::from(i) / 8.0) {
                let r = q.iter().map(|x| x * x).sum::<f64>().sqrt();
                assert!((r - 10.0).abs() < 1e-9, "{q:?} at {r}");
            }
        }
    }

    #[test]
    fn a_ruled_surface_has_its_two_curves_as_sides() {
        let a = bulge([0.0, 0.0, 0.0], [10.0, 0.0, 0.0], 2.0);
        let (b0, b1) = ([0.0, 5.0, 3.0], [10.0, 5.0, 8.0]);
        let check = |sheet: &Record, from: [f64; 3], to: [f64; 3]| {
            assert_eq!(sheet.faces.len(), 1);
            let face = &sheet.faces[0];
            for i in 0..=8 {
                let s = f64::from(i) / 8.0;
                // a at t = 0, b at t = 1, at the same fraction s
                assert!(near(at(face, s, 0.0), on(&a, s), 1e-9), "a at {s}");
                assert!(near(at(face, s, 1.0), lerp(from, to, s), 1e-9), "b at {s}");
                // straight between them
                let mid = lerp(on(&a, s), lerp(from, to, s), 0.5);
                assert!(near(at(face, s, 0.5), mid, 1e-9), "ruling at {s}");
            }
        };
        check(&ruled(&a, &line(b0, b1), false).unwrap(), b0, b1);
        // reversed: b's end is joined to a's start
        check(&ruled(&a, &line(b0, b1), true).unwrap(), b1, b0);
    }

    #[test]
    fn curves_that_do_not_meet_are_refused_with_the_gap() {
        let curves = [
            line(P[0], P[1]),
            line(P[1], P[2]),
            line(P[2], [0.0, 10.0, 1.0]),
            line(P[3], P[0]),
        ];
        match boundary(&curves) {
            Err(Error::BoundaryGap { join, gap }) => {
                assert_eq!(join, 2);
                assert!((gap - 1.0).abs() < 1e-12, "{gap}");
            }
            other => panic!("{other:?}"),
        }
        // the last does not come back to the first
        let open = [
            line(P[0], P[1]),
            line(P[1], P[2]),
            line(P[2], P[3]),
            line(P[3], [0.0, 2.0, 0.0]),
        ];
        match boundary(&open) {
            Err(Error::BoundaryGap { join, gap }) => {
                assert_eq!(join, 3);
                assert!((gap - 2.0).abs() < 1e-12, "{gap}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn too_few_curves_and_circles_are_refused_and_five_fill() {
        assert_eq!(
            boundary(&[line(P[0], P[1]), line(P[1], P[0])]),
            Err(Error::Argument)
        );
        let corners = [
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [12.0, 8.0, 0.0],
            [5.0, 12.0, 0.0],
            [-2.0, 8.0, 0.0],
        ];
        let pentagon: Vec<Curve> = (0..5)
            .map(|i| line(corners[i], corners[(i + 1) % 5]))
            .collect();
        // five curves: the n-sided fill, five faces
        assert_eq!(boundary(&pentagon).unwrap().faces.len(), 5);
        let circle = Curve::Circle {
            frame: Frame {
                origin: [0.0; 3],
                x_axis: [1.0, 0.0, 0.0],
                y_axis: [0.0, 1.0, 0.0],
            },
            radius: 5.0,
        };
        assert_eq!(
            ruled(&circle, &line(P[0], P[1]), false),
            Err(Error::Argument)
        );
    }

    /// The corners of a regular pentagon of radius 10 about the z axis, at height `z(x, y)`.
    fn pentagon(z: impl Fn(f64, f64) -> f64) -> Vec<[f64; 3]> {
        (0..5)
            .map(|i| {
                let a = std::f64::consts::TAU * f64::from(i) / 5.0;
                let (x, y) = (10.0 * a.cos(), 10.0 * a.sin());
                [x, y, z(x, y)]
            })
            .collect()
    }

    #[test]
    fn a_regular_pentagon_of_lines_fills_flat() {
        let p = pentagon(|_, _| 0.0);
        let sides: Vec<Curve> = (0..5).map(|i| line(p[i], p[(i + 1) % 5])).collect();
        let (sheet, bound) = n_sided(&sides).unwrap();
        assert!(bound <= 1e-9, "{bound}");
        assert_eq!(sheet.faces.len(), 5);
        // five faces round a centre, stitched along the five inner curves
        assert_eq!(sheet.edges.iter().filter(|e| e.uses.len() == 2).count(), 5);
        // planar data, planar faces: every point of every face in z = 0
        for face in &sheet.faces {
            for (s, t) in [(0.0, 0.0), (0.3, 0.6), (1.0, 0.5), (0.5, 1.0)] {
                assert!(at(face, s, t)[2].abs() < 1e-9);
            }
        }
    }

    #[test]
    fn five_splines_on_a_saddle_bound_their_patch() {
        // each side a cubic whose control points lie on z = x y / 50 at thirds of the chord
        let z = |x: f64, y: f64| x * y / 50.0;
        let p = pentagon(z);
        let sides: Vec<Curve> = (0..5)
            .map(|i| {
                let (a, b) = (p[i], p[(i + 1) % 5]);
                let third = |t: f64| {
                    let (x, y) = (a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1]));
                    [x, y, z(x, y)]
                };
                Curve::Spline {
                    knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
                    points: vec![a, third(1.0 / 3.0), third(2.0 / 3.0), b],
                }
            })
            .collect();
        let (sheet, bound) = n_sided(&sides).unwrap();
        assert!(bound <= 1e-6, "{bound}");
        assert_eq!(sheet.faces.len(), 5);
        // the corners are vertices of the sheet: its boundary runs through them
        for corner in &p {
            assert!(
                sheet.vertices.iter().any(|v| near(v.point, *corner, 1e-6)),
                "{corner:?}"
            );
        }
    }

    #[test]
    fn five_arcs_on_a_sphere_are_refused() {
        // five arcs of the circle z = 6 on the sphere of radius 10 (radius 8), as rational
        // quadratics
        let half = std::f64::consts::PI / 5.0;
        let w = half.cos();
        let arcs: Vec<Curve> = (0..5)
            .map(|i| {
                let a = 2.0 * half * f64::from(i);
                let on = |t: f64, r: f64| [r * t.cos(), r * t.sin(), 6.0];
                let (s, m, e) = (on(a, 8.0), on(a + half, 8.0 / w), on(a + 2.0 * half, 8.0));
                Curve::RationalSpline {
                    knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                    homogeneous: vec![
                        [s[0], s[1], s[2], 1.0],
                        [m[0] * w, m[1] * w, m[2] * w, w],
                        [e[0], e[1], e[2], 1.0],
                    ],
                }
            })
            .collect();
        assert!(matches!(n_sided(&arcs), Err(Error::Unsupported(_))));
    }
}
