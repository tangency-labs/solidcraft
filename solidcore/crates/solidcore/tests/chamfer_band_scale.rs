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
use solidcore::error::SolidError;
use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::{edge_closest_point, face_bounding_box};
use solidcore::surface::procedural_blend::chamfer::ChamferSize;
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
