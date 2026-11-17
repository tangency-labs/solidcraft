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
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::{edge_closest_point, face_bounding_box};
use solidcore::surface::procedural_blend::chamfer::ChamferSize;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::{edge, Brep, EdgeId};

/// Every fix on but `geometric_g2_continuity`, as ops runs.
fn kernel() -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    let mut f = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        if name != "geometric_g2_continuity" {
            *f.flag_mut(name).unwrap() = true;
        }
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
