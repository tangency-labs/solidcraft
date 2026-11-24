//! #3190's finding: two chamfers too large for the face they share are refused naming the
//! size that fits. Unequal ones name the scale of both (`FitKind::Scale`): the face's width
//! less the fit margin over their setbacks along it, an open bound (docs/generic-blend.md
//! section 3.5). One equal chamfer of one size names its radius (`FitKind::Radius`): the
//! fork's `BandsMeetAt`, where the bands meet.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_executive::FitKind;
use solidcore::curve::curve::Curve;
use solidcore::error::SolidError;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::math::CPoint;
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::{edge_closest_point, face_bounding_box};
use solidcore::surface::procedural_blend::chamfer::ChamferSize;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::{edge, Brep, EdgeId};

/// Every fix on, as ops runs.
fn kernel() -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    let mut f = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        *f.flag_mut(name).unwrap() = true;
    }
    k.fixes = f;
    k
}

fn pick(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
    let q = Point3::new(p[0], p[1], p[2]);
    let mut best = (f64::INFINITY, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance < best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.unwrap()
}

/// What chamfering the two top long edges of the 30 x 2 x 10 block by `d` (both) gives: the
/// result, the fit named and its kind, the first distance `chamfer_fit` reads from it, the
/// edges' tolerance, and (built) the width left of the top face.
struct Run {
    result: Result<(), SolidError>,
    fit: Option<(f64, FitKind)>,
    named: Option<f64>,
    tol: f64,
    top_width: Option<f64>,
    comment: String,
}

fn run(d: [f64; 2]) -> Run {
    let mut k = kernel();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 30.0, 2.0, 10.0).unwrap();
    let edges = [
        pick(&k, &b, [15.0, 0.0, 10.0]),
        pick(&k, &b, [15.0, 2.0, 10.0]),
    ];
    let tol = edge::tolerance(&b, edges[0])
        .unwrap()
        .max(edge::tolerance(&b, edges[1]).unwrap());
    let (result, fit, named, comment) = {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_chamfers_along(&k, &edges, &[ChamferSize::Distances(d); 2], &[])
            .unwrap();
        s.create_corners(&k, None).unwrap();
        for corner in s.fillet_corners().unwrap() {
            s.set_corner_bevel(corner, true).unwrap();
        }
        let result = s.do_filleting(&mut k, None);
        let fit = s.fit_size();
        let named = fit.and_then(|f| s.chamfer_fit(f));
        let comment = s.error_info().comment.clone();
        (result, fit.map(|f| (f.size, f.kind)), named, comment)
    };
    let top_width = result.is_ok().then(|| {
        // the top face: the face at z = 10 throughout
        b.faces()
            .ids()
            .filter_map(|f| face_bounding_box(&k, &b, f, true).ok())
            .find(|bx| (bx.min().z - 10.0).abs() < 1e-9 && (bx.max().z - 10.0).abs() < 1e-9)
            .map(|bx| bx.max().y - bx.min().y)
            .unwrap()
    });
    Run {
        result,
        fit,
        named,
        tol,
        top_width,
        comment,
    }
}

/// Unequal chamfers, (1.5, 3) on both edges: refused by the band check naming the scale
/// `lambda` with `lambda (t_a + t_b) = 2 - margin`, `t_a`, `t_b` the two chamfers' setbacks
/// along the top face (at the right wedge, one of 1.5 and 3 each) and the margin a thousand of
/// the edges' tolerances: the open bound of docs/generic-blend.md section 3.5. `chamfer_fit`
/// reads it as the first distance, `1.5 lambda`. Chamfered `lambda (1.5, 3)` the block builds
/// and leaves the top face the margin wide. The check itself refuses only bands overlapping by
/// more than its `apart`, ten of the solvers' tolerances: just above `(2 + apart) / (t_a + t_b)`
/// it is refused (there by the solver, whose rails already cross). Between the two lie the
/// margin and `apart`.
#[test]
fn unequal_chamfers_too_large_name_the_scale_that_leaves_the_margin() {
    let r = run([1.5, 3.0]);
    assert!(r.result.is_err(), "built");
    let Some((lambda, FitKind::Scale)) = r.fit else {
        panic!("{:?}", r.fit);
    };
    let margin = 1000.0 * r.tol;
    let sums: Vec<f64> = [3.0, 4.5, 6.0]
        .into_iter()
        .filter(|s| (lambda * s - (2.0 - margin)).abs() <= 1e-9)
        .collect();
    assert_eq!(
        sums.len(),
        1,
        "lambda {lambda}: no setbacks sum to (2 - margin) / lambda"
    );
    let sum = sums[0];
    let named = r.named.unwrap();
    assert!(
        (named - 1.5 * lambda).abs() <= 1e-12,
        "{named} against {}",
        1.5 * lambda
    );
    // at the named scale it builds, the top face left the margin wide
    let at = run([1.5 * lambda, 3.0 * lambda]);
    at.result.as_ref().unwrap();
    let left = at.top_width.unwrap();
    assert!(
        (left - margin).abs() <= 1e-8,
        "the top face is {left} wide, not {margin}"
    );
    // just above the check's own threshold, refused. There the rails already cross: the
    // solver's refusal ("rounds of two edges cross on a face") comes before the band check's, and
    // names no size (a refusal without its fit, the finding's)
    let apart = 10.0 * Kernel::default().tolerances.approx_tol_3d;
    let check = (2.0 + apart) / sum * (1.0 + 1e-9);
    let above = run([1.5 * check, 3.0 * check]);
    assert!(
        above.result.is_err(),
        "built just above the check's threshold"
    );
    println!("RES unequal above: {:?}, {}", above.fit, above.comment);
    println!("RES unequal: lambda {lambda}, setbacks {sum}, margin {margin}, apart {apart}, check {check}, top face left {left}");
}

/// The same with the distances the other way round, (3, 1.5): a scale again.
#[test]
fn unequal_chamfers_the_other_way_name_a_scale() {
    let r = run([3.0, 1.5]);
    assert!(r.result.is_err(), "built");
    assert!(matches!(r.fit, Some((_, FitKind::Scale))), "{:?}", r.fit);
    let (lambda, _) = r.fit.unwrap();
    run([3.0 * lambda, 1.5 * lambda]).result.unwrap();
}

/// Parallel opposing rims have interior closest witnesses even when all endpoint
/// corners move. Their top widths meet at (2 - 1000 tol) / (2 d).
#[test]
fn unequal_chamfers_with_selected_endpoint_corners_name_the_interior_contact() {
    let run_top = |distance: f64| {
        let mut k = kernel();
        let mut b = create_box(&mut k, Point3::ZERO, 30.0, 2.0, 10.0).unwrap();
        let edges: Vec<_> = [
            [15.0, 0.0, 10.0],
            [15.0, 2.0, 10.0],
            [0.0, 1.0, 10.0],
            [30.0, 1.0, 10.0],
        ]
        .into_iter()
        .map(|p| pick(&k, &b, p))
        .collect();
        let top = edge::faces(&b, edges[0], None)
            .unwrap()
            .into_iter()
            .find(|&f| edge::faces(&b, edges[1], None).unwrap().contains(&f))
            .unwrap();
        let tol = edges
            .iter()
            .map(|&e| edge::tolerance(&b, e).unwrap())
            .fold(0.0_f64, f64::max);
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_chamfers_along(
            &k,
            &edges,
            &[ChamferSize::DistanceAngle {
                distance,
                angle: 80.0_f64.to_radians(),
            }],
            &[top; 4],
        )
        .unwrap();
        s.create_corners(&k, None).unwrap();
        for corner in s.fillet_corners().unwrap() {
            s.set_corner_bevel(corner, true).unwrap();
        }
        let result = s.do_filleting(&mut k, None);
        (result, s.fit_size(), tol)
    };
    let (result, fit, tol) = run_top(1.5);
    assert!(result.is_err());
    let fit = fit.unwrap();
    assert_eq!(fit.kind, FitKind::Scale);
    let want = (2.0 - 1000.0 * tol) / 3.0;
    assert!((fit.size - want).abs() < 1e-9, "{} vs {want}", fit.size);
    run_top(1.5 * fit.size).0.unwrap();
}

/// The end strip reaches x = 16 at the long rims' closed scale 2/3, covering
/// their midpoint x = 15. The minimizing interval still has retained points in
/// (16, 30), or (0, 14) when the selected end is mirrored. Two selected end
/// strips also leave (16, 18), or a gap (2/3) (21 - f64(20.999999999)),
/// approximately 6.7e-10. Wall factors of two keep the top widths unchanged and
/// exercise the band event; equal setbacks on all four rims instead meet the
/// existing polygon-inradius event first. Neither depends on station spacing.
#[test]
fn parallel_chamfers_name_a_fit_beyond_a_selected_corner_strip() {
    for (end_widths, wall_factor) in [
        ([24.0, 0.0], 1.0),
        ([0.0, 24.0], 1.0),
        ([24.0, 18.0], 1.0),
        ([24.0, 20.999_999_999], 1.0),
        ([24.0, 18.0], 2.0),
        ([24.0, 20.999_999_999], 2.0),
    ] {
        let run_three = |scale: f64| {
            let mut k = kernel();
            let mut b = create_box(&mut k, Point3::ZERO, 30.0, 2.0, 100.0).unwrap();
            let mut edges = vec![
                pick(&k, &b, [15.0, 0.0, 100.0]),
                pick(&k, &b, [15.0, 2.0, 100.0]),
            ];
            let mut sizes =
                vec![ChamferSize::Distances([1.5 * scale, 1.5 * scale * wall_factor]); 2];
            for (end_x, width) in [0.0, 30.0].into_iter().zip(end_widths) {
                if width > 0.0 {
                    edges.push(pick(&k, &b, [end_x, 1.0, 100.0]));
                    sizes.push(ChamferSize::Distances([
                        width * scale,
                        width * scale * wall_factor,
                    ]));
                }
            }
            let tol = edges
                .iter()
                .map(|&e| edge::tolerance(&b, e).unwrap())
                .fold(0.0_f64, f64::max);
            let top = edge::faces(&b, edges[0], None)
                .unwrap()
                .into_iter()
                .find(|&f| edge::faces(&b, edges[1], None).unwrap().contains(&f))
                .unwrap();
            let references = vec![top; edges.len()];
            let mut s = FilletSession::new(&mut k, &mut b).unwrap();
            s.prepare_chamfers_along(&k, &edges, &sizes, &references)
                .unwrap();
            s.create_corners(&k, None).unwrap();
            for corner in s.fillet_corners().unwrap() {
                s.set_corner_bevel(corner, true).unwrap();
            }
            let result = s.do_filleting(&mut k, None);
            (result, s.fit_size(), tol)
        };
        let (result, fit, tol) = run_three(1.0);
        assert!(result.is_err());
        let fit = fit.expect("the long rims retain a contact beyond the end strip");
        let scaled = if wall_factor == 1.0 && end_widths.iter().all(|&width| width > 0.0) {
            // With all four equal-distance rims selected, the existing polygon
            // event precedes band contact: its inradius is 2/2=1, open by 1000 tol.
            assert_eq!(
                fit.kind,
                FitKind::Radius,
                "{end_widths:?}, factor {wall_factor}"
            );
            let want = 1.0 - 1000.0 * tol;
            assert!((fit.size - want).abs() < 1e-9, "{} vs {want}", fit.size);
            fit.size / 1.5
        } else {
            assert_eq!(
                fit.kind,
                FitKind::Scale,
                "{end_widths:?}, factor {wall_factor}"
            );
            let want = (2.0 - 1000.0 * tol) / 3.0;
            assert!((fit.size - want).abs() < 1e-9, "{} vs {want}", fit.size);
            fit.size
        };
        run_three(scaled).0.unwrap();
    }
}

/// Equal chamfers on a narrow top still need an open scale when another selected
/// edge has unequal setbacks. A closed radius converted afterward would leave no top.
#[test]
fn an_equal_pair_in_an_unequal_chamfer_call_keeps_the_scale_margin() {
    let run_three = |scale: f64| {
        let mut k = kernel();
        let mut b = create_box(&mut k, Point3::ZERO, 30.0, 2.0, 10.0).unwrap();
        let edges = [
            pick(&k, &b, [15.0, 0.0, 10.0]),
            pick(&k, &b, [15.0, 2.0, 10.0]),
            pick(&k, &b, [15.0, 0.0, 0.0]),
        ];
        let tol = edges
            .iter()
            .map(|&e| edge::tolerance(&b, e).unwrap())
            .fold(0.0_f64, f64::max);
        let sizes = [
            ChamferSize::Distances([1.5 * scale, 1.5 * scale]),
            ChamferSize::Distances([1.5 * scale, 1.5 * scale]),
            ChamferSize::Distances([1.5 * scale, 3.0 * scale]),
        ];
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_chamfers_along(&k, &edges, &sizes, &[]).unwrap();
        s.create_corners(&k, None).unwrap();
        for corner in s.fillet_corners().unwrap() {
            s.set_corner_bevel(corner, true).unwrap();
        }
        let result = s.do_filleting(&mut k, None);
        (result, s.fit_size(), tol)
    };
    let (result, fit, tol) = run_three(1.0);
    assert!(result.is_err());
    let fit = fit.unwrap();
    assert_eq!(fit.kind, FitKind::Scale);
    let want = (2.0 - 1000.0 * tol) / 3.0;
    assert!((fit.size - want).abs() < 1e-9, "{} vs {want}", fit.size);
    run_three(fit.size).0.unwrap();
}

/// One equal chamfer of one size, (1.5, 1.5) on both: its offsets are one radius, and the
/// fork's `BandsMeetAt` names that radius where the bands meet (1, to its bisection), which
/// `chamfer_fit` reads as the distance; the scale is not asked for.
#[test]
fn an_equal_chamfer_too_large_names_its_radius() {
    let r = run([1.5, 1.5]);
    assert!(r.result.is_err(), "built");
    let Some((radius, FitKind::Radius)) = r.fit else {
        panic!("{:?}", r.fit);
    };
    assert!((radius - 1.0).abs() <= 1e-9, "{radius}");
    assert!((r.named.unwrap() - 1.0).abs() <= 1e-9, "{:?}", r.named);
}

/// Chamfering the two top long edges of a 30 x `width` x 10 block whose side is at `y0` by
/// `d` on both sides: the result, the fit named, and (built) the body's volume and the
/// width of what is left of its top face (0 when the chamfers use it up).
fn chamfer_block_top(
    y0: f64,
    width: f64,
    d: f64,
) -> (Result<(), SolidError>, Option<f64>, f64, f64) {
    let mut k = kernel();
    let mut b = create_box(&mut k, Point3::new(0.0, y0, 0.0), 30.0, width, 10.0).unwrap();
    let edges = [
        pick(&k, &b, [15.0, y0, 10.0]),
        pick(&k, &b, [15.0, y0 + width, 10.0]),
    ];
    let (result, fit) = {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_chamfers_along(&k, &edges, &[ChamferSize::Distances([d, d]); 2], &[])
            .unwrap();
        s.create_corners(&k, None).unwrap();
        for corner in s.fillet_corners().unwrap() {
            s.set_corner_bevel(corner, true).unwrap();
        }
        let result = s.do_filleting(&mut k, None);
        (result, s.fit_size().map(|f| f.size))
    };
    if result.is_err() {
        return (result, fit, 0.0, 0.0);
    }
    let store = AttributeStore::new();
    let key_of = |_| ObjectKey(0);
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    let volume = brep_volume(&k, &mut b, 1e-9, &attrs).unwrap();
    let top = b
        .faces()
        .ids()
        .filter_map(|f| face_bounding_box(&k, &b, f, true).ok())
        .find(|bx| (bx.min().z - 10.0).abs() < 1e-9 && (bx.max().z - 10.0).abs() < 1e-9)
        .map_or(0.0, |bx| bx.max().y - bx.min().y);
    (result, fit, volume, top)
}

/// #20450: two 1 mm chamfers on a face 2 wide meet exactly in its middle and build. So they
/// do where that width is 2 only to the rounding of the model's own coordinates: a block
/// from y = 6.000000000000001 to 8, as a cut leaves an edge, is 1.9999999999999991 wide
/// there, and the two bands, compared exactly, read as overlapping by 9e-16. That is no
/// overlap: the block builds with the volume of the exact one, 30 * 2 * 10 less two right
/// triangles of legs 1 along 30, 570, and its top face is used up. A face narrower than the
/// two chamfers by more than rounding (1e-9) is still refused, naming the size that fits.
#[test]
fn equal_chamfers_that_exactly_use_up_a_face_build_whatever_its_rounding() {
    for (y0, width) in [
        (0.0, 2.0_f64),
        (6.000_000_000_000_001, 1.999_999_999_999_999_1),
    ] {
        assert!(
            (2.0 - width).abs() < 1e-15,
            "{width}: 2 wide to its rounding"
        );
        let (result, _, volume, top) = chamfer_block_top(y0, width, 1.0);
        assert!(result.is_ok(), "{y0} {width}: {result:?}");
        assert!((volume - 570.0).abs() <= 1e-9 * 570.0, "{y0}: {volume}");
        assert!(top <= 1e-9, "{y0}: the top face is {top} wide");
    }
    let (result, fit, _, _) = chamfer_block_top(0.0, 2.0 - 1e-9, 1.0);
    assert!(result.is_err(), "built on a face 1e-9 too narrow");
    let fit = fit.expect("the refusal names a size");
    assert!(fit < 1.0 && fit > 0.99, "{fit}");
}

/// A quadratic-ended outline whose two selected rims approach across a 0.6 mm
/// terminal strip. The short terminal rim stays sharp, as in #19108.
fn narrow_curved_tip(distance: f64, angle: f64) -> Run {
    let mut k = kernel();
    let p = |x, y| Point3::new(x, y, 0.0);
    let line = |a, b| Curve::from(create_line_segment(a, b).unwrap());
    let curved = BSplineCurve::from_data(
        3,
        2,
        5,
        2,
        &[
            CPoint::new(10.0, 0.6, 0.0, 1.0),
            CPoint::new(5.0, 0.6, 0.0, 1.0),
            CPoint::new(0.0, 5.0, 0.0, 1.0),
        ],
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
    )
    .unwrap();
    let curves = [
        line(p(0.0, 0.0), p(10.0, 0.0)),
        line(p(10.0, 0.0), p(10.0, 0.6)),
        Curve::from(curved),
        line(p(0.0, 5.0), p(0.0, 0.0)),
    ];
    let refs: Vec<_> = curves.iter().map(Some).collect();
    let mut b = create_linear_sweep(&mut k, &refs, Vec3::new(0.0, 0.0, 1.0), 10.0, true).unwrap();
    let edges: Vec<_> = [[5.0, 0.0, 10.0], [5.0, 1.7, 10.0]]
        .into_iter()
        .map(|p| pick(&k, &b, p))
        .collect();
    let top = edge::faces(&b, edges[0], None)
        .unwrap()
        .into_iter()
        .find(|&f| edge::faces(&b, edges[1], None).unwrap().contains(&f))
        .unwrap();
    let tol = edges
        .iter()
        .map(|&e| edge::tolerance(&b, e).unwrap())
        .fold(0.0_f64, f64::max);
    let mut s = FilletSession::new(&mut k, &mut b).unwrap();
    s.prepare_chamfers_along(
        &k,
        &edges,
        &[ChamferSize::DistanceAngle { distance, angle }],
        &[top; 2],
    )
    .unwrap();
    s.create_corners(&k, None).unwrap();
    for corner in s.fillet_corners().unwrap() {
        s.set_corner_bevel(corner, true).unwrap();
    }
    let result = s.do_filleting(&mut k, None);
    let fit = s.fit_size();
    let named = fit.and_then(|f| s.chamfer_fit(f));
    let comment = s.error_info().comment.clone();
    if fit.is_some() {
        assert_eq!(
            s.error_site().face.map(solidcore::error::TargetFace::id),
            Some(top),
            "limiting top face"
        );
    }
    Run {
        result,
        fit: fit.map(|f| (f.size, f.kind)),
        named,
        tol,
        top_width: None,
        comment,
    }
}

/// The two inward top offsets first touch across the terminal strip at 0.6 / 2.
/// For unequal chamfers the open fit leaves 1000 edge tolerances between their rails.
#[test]
fn a_distance_angle_chamfer_past_a_narrow_curved_tip_names_its_fit() {
    let angle = 80.0_f64.to_radians();
    for distance in [1.0, 0.5] {
        let refused = narrow_curved_tip(distance, angle);
        assert!(refused.result.is_err());
        let Some((scale, FitKind::Scale)) = refused.fit else {
            panic!("no scale: {}: {:?}", refused.comment, refused.result);
        };
        // The reference face may swap the canonical supports. Scale the caller's
        // first distance, rather than reading the canonical first support's setback.
        let named = distance * scale;
        let want = (0.6 - 1000.0 * refused.tol) / 2.0;
        assert!((named - want).abs() < 1e-9, "{named} vs {want}");
        assert!(
            narrow_curved_tip(named, angle).result.is_ok(),
            "the named distance builds"
        );
    }
    assert!(narrow_curved_tip(0.2, angle).result.is_ok());
}

/// The same terminal event on equal offsets is a closed radius of exactly 0.3,
/// including the endpoint that the old station search excluded.
#[test]
fn an_equal_chamfer_names_the_endpoint_contact_of_a_curved_tip() {
    let refused = narrow_curved_tip(1.0, std::f64::consts::FRAC_PI_4);
    assert!(refused.result.is_err());
    let (size, kind) = refused.fit.unwrap();
    assert_eq!(kind, FitKind::Radius);
    assert!((size - 0.3).abs() <= 1e-12, "{size}");
    assert!((refused.named.unwrap() - 0.3).abs() <= 1e-12);
    assert!(
        narrow_curved_tip(size, std::f64::consts::FRAC_PI_4)
            .result
            .is_ok(),
        "the named equal distance builds"
    );
}

/// The polygon through `corners` swept `h` along z, and the midpoints of its top edges.
fn prism(k: &mut Kernel, corners: &[(f64, f64)], h: f64) -> (Brep, Vec<[f64; 3]>) {
    let n = corners.len();
    let line = |a: (f64, f64), b: (f64, f64)| {
        Curve::from(
            create_line_segment(Vec3::new(a.0, a.1, 0.0), Vec3::new(b.0, b.1, 0.0)).unwrap(),
        )
    };
    let curves: Vec<Curve> = (0..n)
        .map(|i| line(corners[i], corners[(i + 1) % n]))
        .collect();
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let b = create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), h, true).unwrap();
    let mids = (0..n)
        .map(|i| {
            let (a, c) = (corners[i], corners[(i + 1) % n]);
            [(a.0 + c.0) / 2.0, (a.1 + c.1) / 2.0, h]
        })
        .collect();
    (b, mids)
}

/// The corners of a regular `n`-gon of circumradius `r`.
fn regular(n: usize, r: f64) -> Vec<(f64, f64)> {
    (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            (r * a.cos(), r * a.sin())
        })
        .collect()
}

/// Chamfers the top edges of the prism over `corners` listed in `which` (all when `None`) by
/// `d`; the body and the result.
fn chamfer_top(
    corners: &[(f64, f64)],
    h: f64,
    d: f64,
    which: Option<&[usize]>,
) -> (Brep, Result<(), SolidError>) {
    let mut k = kernel();
    let (mut b, mids) = prism(&mut k, corners, h);
    let edges: Vec<EdgeId> = mids
        .iter()
        .enumerate()
        .filter(|(i, _)| which.is_none_or(|w| w.contains(i)))
        .map(|(_, &p)| pick(&k, &b, p))
        .collect();
    let result = {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_chamfers_along(
            &k,
            &edges,
            &vec![ChamferSize::Distances([d, d]); edges.len()],
            &[],
        )
        .unwrap();
        s.create_corners(&k, None).unwrap();
        for corner in s.fillet_corners().unwrap() {
            s.set_corner_bevel(corner, true).unwrap();
        }
        s.do_filleting(&mut k, None)
    };
    (b, result)
}

/// #16645: chamfering every edge of a long outline (128 edges) builds, as it did before the band
/// check compared every pair of bands. The result is a mitred chamfer all round: the 128 walls,
/// the top and the bottom and 128 chamfer faces, five edge rings of 128 (the bottom, the
/// verticals, the chamfers' feet, their tops and the mitres between them), three vertex rings of
/// 128; the volume is the prism less `P d^2 / 2 - K d^3 / 3` for the perimeter `P` and
/// `K = n tan(pi / n)` (the area `A(delta)` of the outline offset inwards by `delta` is
/// `A - P delta + K delta^2`, and the removed depth at `delta` is `d - delta`).
#[test]
fn chamfering_all_128_edges_of_an_outline_builds() {
    let (n, r, h, d) = (128_usize, 60.0_f64, 20.0_f64, 1.0_f64);
    let (b, result) = chamfer_top(&regular(n, r), h, d, None);
    result.unwrap();
    assert_eq!(b.faces().ids().count(), 2 + 2 * n);
    assert_eq!(b.edge_list().len(), 5 * n);
    assert_eq!(b.vertices().ids().count(), 3 * n);
    let nf = n as f64;
    let area = 0.5 * nf * r * r * (std::f64::consts::TAU / nf).sin();
    let perimeter = 2.0 * nf * r * (std::f64::consts::PI / nf).sin();
    let k_sum = nf * (std::f64::consts::PI / nf).tan();
    let want = area * h - (perimeter * d * d / 2.0 - k_sum * d * d * d / 3.0);
    let k = kernel();
    let mut b = b;
    let store = AttributeStore::new();
    let key_of = |_| ObjectKey(0);
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    let v = brep_volume(&k, &mut b, 1e-9, &attrs).unwrap();
    assert!(
        ((v - want) / want).abs() <= 1e-6,
        "volume {v} against {want}"
    );
}

/// The band check still refuses bands that overlap, where the box test prunes nothing: the two
/// long top edges of a 100 x 4 plate chamfered by 3 each leave bands that cross on the 4-wide top
/// face. The pair shares no vertex, so the check reaches it; its boxes overlap.
#[test]
fn chamfering_two_edges_of_a_plate_past_its_width_is_refused() {
    let plate = [(0.0, 0.0), (100.0, 0.0), (100.0, 4.0), (0.0, 4.0)];
    let (_, result) = chamfer_top(&plate, 10.0, 3.0, Some(&[0, 2]));
    assert!(result.is_err(), "built");
}

/// #21417: opposing straight chamfers on a slightly skewed or tapered planar face meet along
/// their weighted bisector continuously down to arbitrarily small angles, naming both equal
/// radii (`FitKind::Radius`) and unequal setback scales (`FitKind::Scale`), including when a
/// selected end chamfer trims the narrow end of the top face.
#[test]
fn opposing_chamfers_on_a_skewed_or_tapered_face_name_their_fit() {
    for skew in [1e-14_f64, 1e-6, 1e-3, 0.5] {
        let corners = [(0.0, 0.0), (30.0, 0.0), (30.0, 2.0 + skew), (0.0, 2.0)];
        let cos_theta = 30.0 / (900.0 + skew * skew).sqrt();
        // Equal setbacks [1.5, 1.5] meet on the x = 0 cap at d = 2 cos(theta) / (1 + cos(theta)).
        let want_radius = 2.0 * cos_theta / (1.0 + cos_theta);
        let run_pair = |size: ChamferSize| {
            let mut k = kernel();
            let (mut b, mids) = prism(&mut k, &corners, 10.0);
            let edges = [pick(&k, &b, mids[0]), pick(&k, &b, mids[2])];
            let top = edge::faces(&b, edges[0], None)
                .unwrap()
                .into_iter()
                .find(|&f| edge::faces(&b, edges[1], None).unwrap().contains(&f))
                .unwrap();
            let tol = edge::tolerance(&b, edges[0])
                .unwrap()
                .max(edge::tolerance(&b, edges[1]).unwrap());
            let mut s = FilletSession::new(&mut k, &mut b).unwrap();
            s.prepare_chamfers_along(&k, &edges, &[size, size], &[top, top])
                .unwrap();
            s.create_corners(&k, None).unwrap();
            for corner in s.fillet_corners().unwrap() {
                s.set_corner_bevel(corner, true).unwrap();
            }
            let result = s.do_filleting(&mut k, None);
            let fit = s.fit_size();
            drop(s);
            let valid = result.is_ok().then(|| {
                let store = AttributeStore::new();
                let key_of = |_| ObjectKey(0);
                let attrs = PropertyAttributes {
                    store: &store,
                    key_of: &key_of,
                };
                brep_volume(&k, &mut b, 1e-9, &attrs).is_ok()
            });
            (result, fit, tol, valid)
        };
        let (result, fit, _, _) = run_pair(ChamferSize::Distances([1.5, 1.5]));
        assert!(result.is_err(), "skew {skew:e}: equal 1.5 built");
        let fit = fit.expect("equal skewed refusal names a fit");
        assert_eq!(fit.kind, FitKind::Radius, "skew {skew:e}");
        assert!(
            (fit.size - want_radius).abs() <= 1e-9,
            "skew {skew:e}: {} vs {want_radius}",
            fit.size
        );
        // Slightly below the closed bound builds cleanly.
        let (below, _, _, below_valid) = run_pair(ChamferSize::Distances([
            want_radius - 0.01,
            want_radius - 0.01,
        ]));
        assert!(below.is_ok(), "skew {skew:e}: control below fit failed");
        assert_eq!(below_valid, Some(true), "skew {skew:e}: below fit invalid");
        // Replaying the reported closed fit builds a valid closed solid.
        let (at_fit, _, _, at_fit_valid) = run_pair(ChamferSize::Distances([fit.size, fit.size]));
        assert!(
            at_fit.is_ok(),
            "skew {skew:e}: reported closed fit {} failed: {at_fit:?}",
            fit.size
        );
        assert_eq!(
            at_fit_valid,
            Some(true),
            "skew {skew:e}: reported closed fit invalid"
        );

        // Distance-angle (80 deg) chamfers name an open common scale that builds cleanly.
        let angle = 80.0_f64.to_radians();
        let (result, fit, tol, _) = run_pair(ChamferSize::DistanceAngle {
            distance: 1.5,
            angle,
        });
        assert!(result.is_err(), "skew {skew:e}: 80 deg 1.5 built");
        let fit = fit.expect("distance-angle skewed refusal names a fit");
        assert_eq!(fit.kind, FitKind::Scale, "skew {skew:e}");
        let want_scale = (2.0 * want_radius - 1000.0 * tol) / 3.0;
        assert!(
            (fit.size - want_scale).abs() <= 1e-8,
            "skew {skew:e}: scale {} vs {want_scale}",
            fit.size
        );
        assert!(
            run_pair(ChamferSize::DistanceAngle {
                distance: 1.5 * fit.size,
                angle,
            })
            .0
            .is_ok(),
            "skew {skew:e}: named distance-angle fit failed to build"
        );
    }

    // Selected end strip covering x in [0, 16] on a slightly skewed 30 x 2 top leaves (16, 30),
    // naming the open scale past the corner strip, which builds cleanly.
    let corners = [(0.0, 0.0), (30.0, 0.0), (30.0, 2.0 + 1e-6), (0.0, 2.0)];
    let run_three = |scale: f64| {
        let mut k = kernel();
        let (mut b, mids) = prism(&mut k, &corners, 100.0);
        let edges = [
            pick(&k, &b, mids[0]),
            pick(&k, &b, mids[2]),
            pick(&k, &b, mids[3]),
        ];
        let top = edge::faces(&b, edges[0], None)
            .unwrap()
            .into_iter()
            .find(|&f| edge::faces(&b, edges[1], None).unwrap().contains(&f))
            .unwrap();
        let tol = edges
            .iter()
            .map(|&e| edge::tolerance(&b, e).unwrap())
            .fold(0.0_f64, f64::max);
        let sizes = [
            ChamferSize::Distances([1.5 * scale, 3.0 * scale]),
            ChamferSize::Distances([1.5 * scale, 3.0 * scale]),
            ChamferSize::Distances([24.0 * scale, 48.0 * scale]),
        ];
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_chamfers_along(&k, &edges, &sizes, &[top; 3])
            .unwrap();
        s.create_corners(&k, None).unwrap();
        for corner in s.fillet_corners().unwrap() {
            s.set_corner_bevel(corner, true).unwrap();
        }
        let result = s.do_filleting(&mut k, None);
        (result, s.fit_size(), tol)
    };
    let (result, fit, tol) = run_three(1.0);
    assert!(result.is_err(), "skewed three-edge built");
    let fit = fit.expect("skewed three-edge refusal names a fit");
    assert_eq!(fit.kind, FitKind::Scale);
    let want = (2.0 + (16.0 / 30.0) * 1e-6 - 1000.0 * tol) / 3.0;
    assert!((fit.size - want).abs() <= 1e-7, "{} vs {want}", fit.size);
    run_three(fit.size).0.unwrap();

    // #20450 rounding gate holds for a 9e-16-skewed 2 mm face too: two 1 mm chamfers still build.
    let almost_two = [
        (0.0, 0.0),
        (30.0, 0.0),
        (30.0, 1.999_999_999_999_999_1),
        (0.0, 2.0),
    ];
    let (mut b, result) = chamfer_top(&almost_two, 10.0, 1.0, Some(&[0, 2]));
    assert!(result.is_ok(), "almost-2 skewed face refused: {result:?}");
    let k = kernel();
    let store = AttributeStore::new();
    let key_of = |_| ObjectKey(0);
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    let volume = brep_volume(&k, &mut b, 1e-9, &attrs).unwrap();
    assert!((volume - 570.0).abs() <= 1e-9 * 570.0, "volume {volume}");
}
