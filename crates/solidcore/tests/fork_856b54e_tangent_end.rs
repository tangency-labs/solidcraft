//! The SolidCraft fork's tangency-labs/smlib@856b54e, under WR-600's
//! `fillet_nx1_tangent_side_edge_same_convexity`: a blend whose side face runs tangent
//! into the end face across a side edge (a tangent seam T) ends where its rail crosses
//! T, and its end rib is solved from that tangent end outward.
//!
//! The part is the fork's tangent-end peg (de3a8c9): a bar 40 x 16 x 10 whose far end
//! is a half disc R 8 about (40, 8), with a disc R 8 standing 10 on its top about the
//! same axis. The front face y = 0 runs tangent into the cylinder along x = 40, where the
//! front top edge (convex) and the concave arc where the top meets the disc end. Without
//! the fork's change, `SmFilletNx1Corner::MakeCornerTopology` sends the rail end at T to
//! the extended-edge intersection and the end rib is marched into the point where the
//! fillet surface touches the end face: a chamfer of either edge fails with status 1001
//! (Corner Edge Computation Failure). With it, the four blends build, and each volume
//! agrees with its closed form (the blend's section swept along the edge or about the
//! disc's axis, cut by the cylinder or the front plane at the tangent end).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_precision_loss
)] // test helpers
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves as curves;
use solidcore::query::api_queries::get_edges;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

const PI: f64 = core::f64::consts::PI;

fn sweep(k: &mut Kernel, cs: &[Curve], h: f64) -> Brep {
    let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
    create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), h, true).unwrap()
}

fn peg(k: &mut Kernel) -> Brep {
    let p = Point3::new;
    let line = |a: Point3, b: Point3| Curve::from(curves::create_line_segment(a, b).unwrap());
    let bar = [
        line(p(0.0, 0.0, 0.0), p(40.0, 0.0, 0.0)),
        Curve::from(curves::create_arc(p(40.0, 8.0, 0.0), 8.0, -90.0, 90.0).unwrap()),
        line(p(40.0, 16.0, 0.0), p(0.0, 16.0, 0.0)),
        line(p(0.0, 16.0, 0.0), p(0.0, 0.0, 0.0)),
    ];
    let bar = sweep(k, &bar, 10.0);
    let disc = [Curve::from(
        curves::create_circle(p(40.0, 8.0, 10.0), 8.0).unwrap(),
    )];
    let disc = sweep(k, &disc, 10.0);
    boolean_union(k, bar, disc).unwrap()
}

fn find_edge(b: &Brep, pred: impl Fn(Point3, Point3) -> bool) -> EdgeId {
    let found: Vec<EdgeId> = get_edges(b)
        .unwrap()
        .into_iter()
        .filter(|&e| {
            let (s, t) = edge::start_end_vertices(b, e).unwrap();
            pred(
                b.vertex(s.unwrap()).unwrap().point,
                b.vertex(t.unwrap()).unwrap().point,
            )
        })
        .collect();
    assert_eq!(found.len(), 1);
    found[0]
}

/// The concave half circle where the top meets the disc, (40, 0, 10) to (40, 16, 10).
fn concave_arc(b: &Brep) -> EdgeId {
    find_edge(b, |s, t| {
        (s.z - 10.0).abs() < 1e-9
            && (t.z - 10.0).abs() < 1e-9
            && (s.x - 40.0).abs() < 1e-9
            && (t.x - 40.0).abs() < 1e-9
            && (s.y - t.y).abs() > 8.0
    })
}

/// The front top edge, (0, 0, 10) to (40, 0, 10).
fn front_top(b: &Brep) -> EdgeId {
    find_edge(b, |s, t| {
        (s.z - 10.0).abs() < 1e-9
            && (t.z - 10.0).abs() < 1e-9
            && s.y.abs() < 1e-9
            && t.y.abs() < 1e-9
            && (s.x - t.x).abs() > 30.0
    })
}

/// The fork's profile around the change: the tangent side-edge skips it builds on; the
/// trace run on past a side edge's end as the fork's `RailEdgeuseIntersect` runs it
/// (WR-610, WR-643; WR-654 for the B-spline trim on the cylinder, which an
/// `SmBSplineCurve` would clamp); and the fork's fixes the blends' other parts need (the
/// concave chamfer's cone and round's torus about the disc's axis, the cap's cover).
fn kernel(tangent_end: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_nx1_tangent_side_edge = true;
    k.fixes.fillet_nx1_opposite_continuation_side_edge = true;
    k.fixes.fillet_nx1_tangent_corner_edge = true;
    k.fixes.fillet_trace_reaches_rails = true;
    k.fixes.fillet_rail_end_on_continued_surface = true;
    k.fixes.fillet_run_on_past_far_end_only = true;
    k.fixes.fillet_run_on_bspline_trims = true;
    k.fixes.fillet_chamfer_cone_on_arc = true;
    k.fixes.fillet_torus_span_from_start_section = true;
    k.fixes.cover_wraps_across_closed_seam = true;
    k.fixes.fillet_nx1_tangent_side_edge_same_convexity = tangent_end;
    k
}

/// Fillet the front top edge or the concave arc with `xsect` (0 chamfer, 1 round) of
/// size 2; the volume of the result, which must check clean.
fn blend(tangent_end: bool, front: bool, xsect: u32) -> solidcore::Result<f64> {
    let mut k = kernel(tangent_end);
    let mut b = peg(&mut k);
    let e = if front {
        front_top(&b)
    } else {
        concave_arc(&b)
    };
    fillet_edges(&mut k, &mut b, &[e], 2.0, xsect, 1, 1.0)?;
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:#?}");
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    Ok(brep_volume(&k, &mut b, 1e-9, &attrs).unwrap())
}

fn simpson(f: impl Fn(f64) -> f64, a: f64, b: f64) -> f64 {
    let n = 20_000;
    let h = (b - a) / f64::from(n);
    let mut sum = f(a) + f(b);
    for i in 1..n {
        sum += f(a + f64::from(i) * h) * if i % 2 == 1 { 4.0 } else { 2.0 };
    }
    sum * h / 3.0
}

/// The peg: the bar, its half-disc end and the disc.
fn peg_volume() -> f64 {
    6400.0 + 32.0 * PI * 10.0 + 64.0 * PI * 10.0
}

/// The front chamfer: its triangle (legs 2) along the edge, cut past
/// `x = 40 - sqrt(28)` by the disc's cylinder, outside which it lies `c(u) = 8 -
/// sqrt(64 - u^2)` from the front at `u = 40 - x`.
fn front_chamfer_volume() -> f64 {
    let s = 28.0_f64.sqrt();
    let c = |u: f64| 8.0 - (64.0 - u * u).sqrt();
    peg_volume() - (2.0 * (40.0 - s) + simpson(|u| 2.0 * c(u) - c(u) * c(u) / 2.0, 0.0, s))
}

/// A concave blend of the arc whose section stands `h(r)` above the top at radius `r`
/// from the axis: swept about the axis over the part of the top it lies on,
/// `2 asin(8 / r)` of a turn (the front and back planes cut it at both tangent ends).
fn concave_volume(h: impl Fn(f64) -> f64) -> f64 {
    // r = 10 - 2 sin(s): smooth in s where the round's section meets the wall
    peg_volume()
        + simpson(
            |s| {
                let r = 10.0 - 2.0 * s.sin();
                h(r) * r * 2.0 * (8.0 / r).asin() * 2.0 * s.cos()
            },
            0.0,
            PI / 2.0,
        )
}

#[test]
fn front_chamfer_ends_at_its_tangent_end() {
    // without the change, status 1001 (Corner Edge Computation Failure)
    let err = blend(false, true, 0).unwrap_err();
    assert!(err.to_string().contains("1001"), "{err}");
    let v = blend(true, true, 0).unwrap();
    let want = front_chamfer_volume();
    assert!((v - want).abs() < 1e-6, "V {v} vs {want}");
}

#[test]
fn concave_chamfer_ends_at_its_tangent_ends() {
    let err = blend(false, false, 0).unwrap_err();
    assert!(err.to_string().contains("1001"), "{err}");
    let v = blend(true, false, 0).unwrap();
    let want = concave_volume(|r| 10.0 - r);
    // the fork: 9457.585735987 against its quadrature's 9457.585731695
    assert!((v - want).abs() < 1e-5, "V {v} vs {want}");
}

#[test]
fn concave_round_ends_at_its_tangent_ends() {
    let want = concave_volume(|r| 2.0 - (4.0 - (10.0 - r) * (10.0 - r)).max(0.0).sqrt());
    // WR-600's round builds either way; its end ribs built from the tangent ends
    // outward are nearer the exact volume than the ones marched into them
    let before = blend(false, false, 1).unwrap();
    let after = blend(true, false, 1).unwrap();
    assert!((after - want).abs() < 1e-5, "V {after} vs {want}");
    assert!(
        (after - want).abs() < (before - want).abs(),
        "{after} {before} {want}"
    );
}

#[test]
fn front_round_ends_at_its_tangent_end() {
    let v = blend(true, true, 1).unwrap();
    let before = blend(false, true, 1).unwrap();
    assert!((v - before).abs() < 1e-5, "V {v} vs {before}");
}
