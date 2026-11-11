//! Patches bounded by curves: the boundary patch of curves meeting end to end round one loop (a
//! Coons patch of four, an n-sided fill of three or of five and more) and the ruled surface
//! between two curves, each a new sheet record. The curves are the record's ([`Curve`]), each
//! over its natural range; the patches are the kernel's (`BSplineSurface::create_coons_patch`,
//! `create_n_sided_patch`, `create_ruled_surface`), and so is every distance compared. Ops
//! checks the inputs, turns the curves to run end to end, sequences the kernel's calls and
//! converts.

use solidcore::curve::curve::CurveKind;
use solidcore::curve::from_parameters::from_parameters;
use solidcore::geom::certified::Bounded;
use solidcore::geom::tol::{context_zone_tol_3d, SmallTopoType, USE_DEFAULT};
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, Ends};
use solidcore::nurbs::b_spline_surface::{BSplineSurface, SurfParamType};
use solidcore::query::api_queries::point_distance;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p3::new_brep;
use solidcore::topology::brep::p4::stitch_faces;
use solidcore::topology::brep::sheet::face_on;
use solidcore::topology::Brep;
use solidcore::SolidError;
use solidcore_record::brep::{Brep as Record, Curve, Natural};

use crate::body::{finish, Body, Expect};
use crate::error::status;
use crate::session::run;
use crate::sweep::{coord_ok, CURVE_MAX_POINTS};
use crate::{Error, Result};

/// A call's own refusal, else the value: as the kernel's calls answer inside [`run`].
type Out<T> = std::result::Result<T, Error>;

/// Most curves one boundary patch takes.
pub const MAX_SIDES: usize = 64;

/// The angle (degrees) within which the kernel's n-sided fill matches the cross-boundary
/// derivatives it builds across its inner curves: the kernel's own tests' value.
const N_SIDED_ANGLE_DEG: f64 = 1.0;

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
fn splines(k: &Kernel, curves: &[Curve]) -> solidcore::Result<Out<Vec<BSplineCurve>>> {
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

/// A sheet of one face on each of `surfaces`, over its whole natural box, stitched along the
/// sides they share.
fn sheet(k: &mut Kernel, surfaces: Vec<BSplineSurface>) -> solidcore::Result<Out<Brep>> {
    let many = surfaces.len() > 1;
    let mut b = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
    for s in surfaces {
        let s = Surface::from(s);
        let bx = s.natural_uv_domain_with_nurb(k)?;
        face_on(k, &mut b, s, &bx)?;
    }
    b.refresh_cached_topology();
    if many {
        let tol = b.tolerance();
        if let Err(e) = stitch_faces(k, &mut b, tol, None, false) {
            return Ok(Err(status(e)));
        }
        b.refresh_cached_topology();
    }
    finish(k, b, Expect::FACES)
}

/// A new body's record.
fn record(made: Result<Out<Brep>>) -> Result<Record> {
    Body::new(made??).to_record().map(|(r, _)| r)
}

/// The patch bounded by `curves`, which meet end to end round one loop in the order given
/// (each may run either way: it is turned to run on from the one before). Four curves bound a
/// bilinear Coons patch, one face; three, or five and more, an n-sided fill: N four-sided faces
/// round a centre, each bounded by halves of two curves and two inner curves from their
/// middles to the centre, stitched into one sheet. Each curve is taken over its natural range
/// (a line from its start to its end, a spline over its knots' domain); the ends must meet
/// within the model's zone tolerance, as the kernel's distance proves.
///
/// # Errors
/// [`Error::Argument`] for fewer than three curves or more than [`MAX_SIDES`], or a curve
/// refused (a circle, an ellipse, a traced intersection curve, numbers the kernel refuses);
/// [`Error::BoundaryGap`] for curves that do not meet; [`Error::Unsupported`] for a rational
/// curve (an arc) bounding an n-sided fill, which the kernel's fill does not take; the
/// kernel's status.
pub fn boundary(curves: &[Curve]) -> Result<Record> {
    if curves.len() < 3 || curves.len() > MAX_SIDES {
        return Err(Error::Argument);
    }
    record(run("surface::boundary", |k| {
        let mut sides = refused!(splines(k, curves)?);
        let tol = context_zone_tol_3d(k, None, SmallTopoType::NormalSize);
        refused!(looped(&mut sides, tol)?);
        let surfaces = match <[BSplineCurve; 4]>::try_from(sides) {
            Ok([c0, c1, mut c2, mut c3]) => {
                // c0 runs P0 to P1, c1 P1 to P2, c2 P2 to P3, c3 P3 to P0. The kernel's sides,
                // in its order: P0 to P1 and P3 to P2 (running in u), P0 to P3 and P1 to P2
                // (running in v).
                reverse(&mut c2)?;
                reverse(&mut c3)?;
                let mut quad = [c0, c2, c3, c1];
                match BSplineSurface::create_coons_patch(tol, true, &mut quad, &mut []) {
                    Ok(s) => vec![s],
                    Err(e) => return Ok(Err(status(e))),
                }
            }
            Err(mut sides) => {
                for s in &sides {
                    if s.is_rational()? {
                        return Ok(Err(Error::Unsupported(
                            "a rational curve bounding an n-sided patch",
                        )));
                    }
                }
                let mut across: Vec<Option<BSplineCurve>> = sides.iter().map(|_| None).collect();
                match BSplineSurface::create_n_sided_patch(
                    &mut sides,
                    &mut across,
                    N_SIDED_ANGLE_DEG,
                    None,
                ) {
                    Ok(s) => s,
                    Err(e) => return Ok(Err(status(e))),
                }
            }
        };
        sheet(k, surfaces)
    }))
}

/// The ruled surface between `a` and `b`, one face: straight lines join the points at the
/// same fraction of each curve's natural range, `a` and `b` running as given.
///
/// # Errors
/// [`Error::Argument`] for a curve refused (as [`boundary`]); the kernel's status.
pub fn ruled(a: &Curve, b: &Curve) -> Result<Record> {
    record(run("surface::ruled", |k| {
        let a = refused!(spline(k, a)?);
        let b = refused!(spline(k, b)?);
        match BSplineSurface::create_ruled_surface(&a, &b, SurfParamType::V) {
            Ok(s) => sheet(k, vec![s]),
            Err(e) => Ok(Err(status(e))),
        }
    }))
}

#[cfg(test)]
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

    #[test]
    fn a_ruled_surface_joins_two_lines_straight() {
        let (a0, a1) = ([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]);
        let (b0, b1) = ([0.0, 5.0, 3.0], [10.0, 5.0, 8.0]);
        let sheet = ruled(&line(a0, a1), &line(b0, b1)).unwrap();
        assert_eq!(sheet.faces.len(), 1);
        let face = &sheet.faces[0];
        for s in [0.0, 0.3, 0.5, 1.0] {
            for t in [0.0, 0.4, 1.0] {
                let want = lerp(lerp(a0, a1, s), lerp(b0, b1, s), t);
                assert!(near(at(face, s, t), want, 1e-9), "at ({s}, {t})");
            }
        }
    }

    /// The quadratic spline from `a` to `b` whose middle control point stands 1.2 above the
    /// chord's middle, and the curve's middle (0.6 above it).
    fn bulge(a: [f64; 3], b: [f64; 3]) -> (Curve, [f64; 3]) {
        let m = lerp(a, b, 0.5);
        (
            Curve::Spline {
                knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                points: vec![a, [m[0], m[1], m[2] + 1.2], b],
            },
            [m[0], m[1], m[2] + 0.6],
        )
    }

    #[test]
    fn three_splines_bound_an_n_sided_sheet_on_them() {
        let p = [[0.0, 0.0, 0.0], [40.0, 0.0, 0.0], [20.0, 35.0, 0.0]];
        let sides: Vec<(Curve, [f64; 3])> = (0..3).map(|i| bulge(p[i], p[(i + 1) % 3])).collect();
        let curves: Vec<Curve> = sides.iter().map(|(c, _)| c.clone()).collect();
        let sheet = boundary(&curves).unwrap();
        // three four-sided faces round a centre, stitched along the three inner curves
        assert_eq!(sheet.faces.len(), 3);
        let shared = sheet.edges.iter().filter(|e| e.uses.len() == 2).count();
        assert_eq!(shared, 3);
        // the corners and the curves' middles are vertices of the sheet: its boundary runs
        // through them
        let on = |q: [f64; 3]| sheet.vertices.iter().any(|v| near(v.point, q, 1e-6));
        for (corner, (_, middle)) in p.iter().zip(&sides) {
            assert!(on(*corner), "corner {corner:?}");
            assert!(on(*middle), "middle {middle:?}");
        }
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
    fn too_few_curves_arcs_round_n_sides_and_circles_are_refused() {
        assert_eq!(
            boundary(&[line(P[0], P[1]), line(P[1], P[0])]),
            Err(Error::Argument)
        );
        // three quarter circles (rational) meeting round a loop
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
        assert!(matches!(boundary(&arcs), Err(Error::Unsupported(_))));
        let circle = Curve::Circle {
            frame: Frame {
                origin: [0.0; 3],
                x_axis: [1.0, 0.0, 0.0],
                y_axis: [0.0, 1.0, 0.0],
            },
            radius: 5.0,
        };
        assert_eq!(ruled(&circle, &line(P[0], P[1])), Err(Error::Argument));
    }
}
