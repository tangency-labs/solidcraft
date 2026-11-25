//! The prism family: planar region booleans and extrusion, prism sections and equivalence.
//! Moved from SolidCraft's `crates/kernel/src/prism.rs` and `prism_measure.rs` tests (see
//! notes-f-prism/TESTS.md for each move). The oracles are closed forms; the tests compute them
//! with the maths the library itself may not use.
#![allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::panic,
    clippy::disallowed_methods,
    clippy::float_cmp
)]

use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2, FRAC_PI_3, FRAC_PI_4, PI, TAU};

use solidcore_ops::prism::{
    extrude_planar_regions, fitted_loop, loop_holds, planar_regions_boolean, prism_equivalent,
    prism_section, region_area, PrismSection,
};
use solidcore_ops::{make_box, make_cylinder, session, Body, BooleanOp, CheckLevel, Error};
use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};

fn all_fixes() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
}

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn rectangle(a: Vec2, b: Vec2) -> Region2 {
    Region2 {
        outer: Loop2::polygon(&[a, v(b.x, a.y), b, v(a.x, b.y)]),
        holes: vec![],
    }
}

fn area(r: &Region2) -> f64 {
    region_area(r).unwrap().value
}

fn loop_area(l: &Loop2) -> f64 {
    area(&Region2 {
        outer: l.clone(),
        holes: vec![],
    })
}

fn start(s: &Seg2) -> Vec2 {
    match *s {
        Seg2::Line { a, .. } | Seg2::Conic { a, .. } => a,
        Seg2::Arc {
            center,
            radius,
            start,
            ..
        } => v(
            center.x + radius * start.cos(),
            center.y + radius * start.sin(),
        ),
        Seg2::Cubic { p0, .. } => p0,
        Seg2::Bezier { p, .. } => p[0],
    }
}

fn end(s: &Seg2) -> Vec2 {
    match *s {
        Seg2::Line { b, .. } | Seg2::Conic { b, .. } => b,
        Seg2::Arc {
            center,
            radius,
            start,
            sweep,
        } => v(
            center.x + radius * (start + sweep).cos(),
            center.y + radius * (start + sweep).sin(),
        ),
        Seg2::Cubic { p3, .. } => p3,
        Seg2::Bezier { n, p } => p[usize::from(n)],
    }
}

fn dist(a: Vec2, b: Vec2) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

/// A line's or arc's length.
fn length(s: &Seg2) -> f64 {
    match *s {
        Seg2::Arc { radius, sweep, .. } => radius * sweep.abs(),
        _ => dist(start(s), end(s)),
    }
}

fn reversed(s: &Seg2) -> Seg2 {
    match *s {
        Seg2::Line { a, b } => Seg2::Line { a: b, b: a },
        Seg2::Arc {
            center,
            radius,
            start,
            sweep,
        } => Seg2::Arc {
            center,
            radius,
            start: start + sweep,
            sweep: -sweep,
        },
        Seg2::Conic { a, apex, b, w } => Seg2::Conic {
            a: b,
            apex,
            b: a,
            w,
        },
        other => panic!("{other:?}"),
    }
}

fn closed(l: &Loop2, tol: f64) -> bool {
    l.segs
        .iter()
        .zip(l.segs.iter().cycle().skip(1))
        .all(|(a, b)| dist(end(a), start(b)) <= tol)
}

fn is_line_or_arc(s: &Seg2) -> bool {
    matches!(s, Seg2::Line { .. } | Seg2::Arc { .. })
}

fn same_seg(a: &Seg2, b: &Seg2) -> bool {
    match (a, b) {
        (
            Seg2::Conic { a, apex, b, w },
            Seg2::Conic {
                a: c,
                apex: x,
                b: d,
                w: u,
            },
        ) => {
            dist(*a, *c) < 1e-9
                && dist(*apex, *x) < 1e-9
                && dist(*b, *d) < 1e-9
                && (w - u).abs() < 1e-12
        }
        _ => a == b,
    }
}

#[test]
fn shared_arc_line_endpoint_roundoff_does_not_open_a_union() {
    all_fixes();
    let a = Region2 {
        outer: Loop2::polygon(&[v(-2., 0.), v(2., 0.), v(2., 2.), v(-2., 2.)]),
        holes: vec![],
    };
    let line = |a: (f64, f64), b: (f64, f64)| Seg2::Line {
        a: v(a.0, a.1),
        b: v(b.0, b.1),
    };
    for epsilon in [-4e-14, 4e-14] {
        let b = Region2 {
            outer: Loop2 {
                segs: vec![
                    line((1.15, epsilon), (-1.15, -epsilon)),
                    Seg2::Arc {
                        center: v(-1.35, epsilon),
                        radius: 0.2,
                        start: 0.,
                        sweep: -FRAC_PI_2,
                    },
                    line((-1.35, -0.2), (-2.4, -0.2)),
                    line((-2.4, -0.2), (-2.4, -1.)),
                    line((-2.4, -1.), (2.4, -1.)),
                    line((2.4, -1.), (2.4, -0.2)),
                    line((2.4, -0.2), (1.35, -0.2)),
                    Seg2::Arc {
                        center: v(1.35, -epsilon),
                        radius: 0.2,
                        start: -FRAC_PI_2,
                        sweep: -FRAC_PI_2,
                    },
                ],
            },
            holes: vec![],
        };
        let result = planar_regions_boolean(
            std::slice::from_ref(&a),
            std::slice::from_ref(&b),
            BooleanOp::Union,
            1e-5,
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        assert!(closed(&result[0].outer, 1e-4));
        assert!((area(&result[0]) - area(&a) - area(&b)).abs() < 1e-6);
    }
}

#[test]
fn a_ledge_shorter_than_the_tolerance_reads_back_as_one_region() {
    all_fixes();
    let ledged = Region2 {
        outer: Loop2::polygon(
            &[
                (100.0, 3.0),
                (100.0, 3.0001),
                (97.0, 3.0001),
                (97.0, 60.0),
                (0.0, 60.0),
                (0.0, 3.0),
            ]
            .map(|(x, y)| v(x, y)),
        ),
        holes: vec![],
    };
    let below = rectangle(v(0.0, -12.0), v(100.0, 0.0));
    let out = planar_regions_boolean(
        std::slice::from_ref(&ledged),
        std::slice::from_ref(&below),
        BooleanOp::Union,
        1e-7,
    )
    .unwrap();
    assert_eq!(out.len(), 2, "{out:?}");
    let top = out
        .iter()
        .find(|r| loop_holds(&r.outer, v(50.0, 30.0)) == Some(true))
        .unwrap();
    assert!(
        top.outer.segs.len() == 6 && closed(&top.outer, 1e-9),
        "{top:?}"
    );
    assert!((area(top) - area(&ledged)).abs() < 1e-9, "{}", area(top));
}

/// The flange outline of SolidCraft's `a_cut_passing_a_hair_off_a_flange_corner_builds`, and
/// its corner V = (left, top) the cuts below pass near.
fn flange_outline() -> ([Vec2; 4], Vec2) {
    let x0 = 4.0 - 0.398_229_715_025_710_36;
    let (left, right, top) = (x0 - 30.0, x0 - 4.0, 98.0 - 2f64.sqrt());
    (
        [v(right, top), v(left, top), v(left, 0.0), v(right, 0.0)],
        v(left, top),
    )
}

/// The cut of the same test, whose taper runs from its foot on the flange's bottom edge to
/// its head: `along`, the head lies on the flange's side line x = left, `d` above the corner
/// (below for d < 0); otherwise the head is 1 above the flange's top and the taper passes
/// the corner `d` to its left along y = top (to its right for d < 0). Counter-clockwise,
/// foot first, head second.
fn flange_cut(d: f64, along: bool) -> [Vec2; 5] {
    let x0 = 4.0 - 0.398_229_715_025_710_36;
    let c = 98.0 - 2f64.sqrt();
    let to2 = |y: f64, z: f64| v(x0 - z, y);
    let (y, z) = if along {
        (c + d, 30.0)
    } else {
        (c + 1.0, 10.0 + (c + 1.0) * (20.0 + d) / c)
    };
    [
        to2(0.0, 10.0),
        to2(y, z),
        to2(y, 35.0),
        to2(-5.0, 35.0),
        to2(-5.0, 10.0),
    ]
}

/// A polygon's signed area (positive counter-clockwise), the shoelace formula.
fn polygon_area(p: &[Vec2]) -> f64 {
    let n = p.len();
    (0..n)
        .map(|i| p[i].x * p[(i + 1) % n].y - p[(i + 1) % n].x * p[i].y)
        .sum::<f64>()
        / 2.0
}

/// The part of polygon `p` on the left of the directed line through `a` and `b`
/// (Sutherland-Hodgman, one clip edge), points closer than 1e-12 to the last one dropped.
fn clip_left(p: &[Vec2], a: Vec2, b: Vec2) -> Vec<Vec2> {
    let side = |q: Vec2| (b.x - a.x) * (q.y - a.y) - (b.y - a.y) * (q.x - a.x);
    let mut out: Vec<Vec2> = Vec::new();
    let mut push = |q: Vec2| {
        if out.last().is_none_or(|&l| dist(l, q) > 1e-12) {
            out.push(q);
        }
    };
    for i in 0..p.len() {
        let (q, r) = (p[i], p[(i + 1) % p.len()]);
        let (sq, sr) = (side(q), side(r));
        if sq >= 0.0 {
            push(q);
        }
        if (sq >= 0.0) != (sr >= 0.0) {
            let t = sq / (sq - sr);
            push(v(q.x + t * (r.x - q.x), q.y + t * (r.y - q.y)));
        }
    }
    if out.len() > 1 && dist(out[0], *out.last().unwrap()) <= 1e-12 {
        out.pop();
    }
    out
}

/// Where the line through `a` and `b` meets the line through `p` with direction `u`.
fn meet(a: Vec2, b: Vec2, p: Vec2, u: Vec2) -> Vec2 {
    let w = v(b.x - a.x, b.y - a.y);
    let t = ((p.x - a.x) * u.y - (p.y - a.y) * u.x) / (w.x * u.y - w.y * u.x);
    v(a.x + t * w.x, a.y + t * w.y)
}

/// The exact flange-and-cut booleans (intersection, flange minus cut, union) as polygons:
/// the cut and the flange are convex, the cut's only edge inside the flange is the taper, so
/// the intersection is the cut clipped by the flange, the difference the flange clipped to
/// the right of the taper, and the union the cut with its taper replaced by the
/// difference's boundary from the foot round to where the taper leaves the flange.
fn flange_cut_exact(flange: &[Vec2; 4], cut: &[Vec2; 5]) -> [Vec<Vec2>; 3] {
    let (foot, head) = (cut[0], cut[1]);
    let mut inter = cut.to_vec();
    for i in 0..4 {
        inter = clip_left(&inter, flange[i], flange[(i + 1) % 4]);
    }
    let mut rest = clip_left(flange, head, foot);
    let at_foot = rest.iter().position(|&q| dist(q, foot) <= 1e-12).unwrap();
    rest.rotate_left(at_foot);
    let mut union = rest.clone();
    if dist(*union.last().unwrap(), head) > 1e-12 {
        union.push(head);
    }
    union.extend_from_slice(&cut[2..]);
    [inter, rest, union]
}

/// How far the area of a tolerant result may differ from the exact polygon `exact`: the
/// exact points near the corner V that one intersection tolerance `tx` cannot tell apart
/// (V, and where the taper meets the side line x = left or the top line y = top, or its
/// head, each within the stretch tx / sin(angle) of V along its line, when the taper passes
/// within tx of V) may become one vertex anywhere among them, so each exact vertex within
/// their diameter δ of V moves at most δ. Moving vertex `p_i` by δ changes the area by at
/// most `δ |p_(i+1) - p_(i-1)| / 2`.
fn sliver_bound(exact: &[Vec2], corner: Vec2, foot: Vec2, head: Vec2, tx: f64) -> f64 {
    let w = v(head.x - foot.x, head.y - foot.y);
    let len = w.x.hypot(w.y);
    let off = ((corner.x - foot.x) * w.y - (corner.y - foot.y) * w.x).abs() / len;
    if off > tx {
        return 0.0;
    }
    let mut near = vec![corner];
    for (u, p) in [
        (v(0.0, 1.0), meet(foot, head, corner, v(0.0, 1.0))),
        (v(1.0, 0.0), meet(foot, head, corner, v(1.0, 0.0))),
        (v(0.0, 0.0), head),
    ] {
        let sine = if u.x == 0.0 && u.y == 0.0 {
            1.0
        } else {
            (w.x * u.y - w.y * u.x).abs() / len
        };
        if dist(p, corner) <= tx / sine {
            near.push(p);
        }
    }
    let delta = near
        .iter()
        .flat_map(|&p| near.iter().map(move |&q| dist(p, q)))
        .fold(0.0, f64::max);
    let n = exact.len();
    (0..n)
        .filter(|&i| dist(exact[i], corner) <= delta)
        .map(|i| delta * dist(exact[(i + 1) % n], exact[(i + n - 1) % n]) / 2.0)
        .sum()
}

/// A flange's outline intersected with a cut whose taper passes 9.8e-6 off the flange's top
/// corner (SolidCraft's `a_cut_passing_a_hair_off_a_flange_corner_builds`, one of its calls,
/// on a kernel with no fixes). The taper meets the flange's side 4.83e-5 below the corner,
/// within the near-tangent stretch where the taper and the side lie within one intersection
/// tolerance of each other, so the side's piece between the crossing and the corner runs
/// along the taper. The intersection is one triangle under the taper, its apex on the side
/// between the taper's crossing and the corner, and its area that of the triangle with that
/// apex.
#[test]
fn a_taper_passing_a_hair_off_a_corner_intersects_to_one_triangle() {
    session::set_thread_fixes(Vec::<&str>::new()).unwrap();
    let ([right_top, _, left_bottom, _], corner) = flange_outline();
    let flange = Region2 {
        outer: Loop2::polygon(&flange_outline().0),
        holes: vec![],
    };
    let cut_pts = flange_cut(1e-5, false);
    let (foot, head) = (cut_pts[0], cut_pts[1]);
    let cut = Region2 {
        outer: Loop2::polygon(&cut_pts),
        holes: vec![],
    };
    // the taper meets x = left at y = crossing (computed here, as the oracle)
    let crossing = meet(foot, head, corner, v(0.0, 1.0)).y;
    assert!(
        crossing < corner.y && corner.y - crossing < 5e-5,
        "{crossing}"
    );
    let out = planar_regions_boolean(&[flange], &[cut], BooleanOp::Intersect, 1e-7).unwrap();
    assert_eq!(out.len(), 1, "{out:?}");
    let tri = &out[0];
    assert!(tri.holes.is_empty(), "{tri:?}");
    assert_eq!(tri.outer.segs.len(), 3, "{tri:?}");
    assert!(closed(&tri.outer, 1e-9), "{tri:?}");
    let corners: Vec<Vec2> = tri.outer.segs.iter().map(start).collect();
    let near = |p: Vec2| corners.iter().any(|&c| dist(c, p) <= 1e-9);
    assert!(near(foot) && near(left_bottom), "{corners:?}");
    let apex = corners
        .iter()
        .copied()
        .find(|c| c.y > 1.0)
        .unwrap_or_else(|| panic!("{corners:?}"));
    assert!(
        (apex.x - corner.x).abs() <= 1e-9 && apex.y >= crossing - 1e-9 && apex.y <= corner.y + 1e-9,
        "{apex:?}"
    );
    // the area of the triangle with that apex, within the bounds the apex's range gives it
    let half_base = (foot.x - corner.x) / 2.0;
    let got = area(tri);
    assert!((got - half_base * apex.y).abs() < 1e-9, "{got}");
    assert!(
        got >= half_base * crossing - 1e-9 && got <= half_base * corner.y + 1e-9,
        "{got}"
    );
    assert!(right_top.x > foot.x);
}

/// The flange and the cut of the test above, with the taper's head moved `d` along the
/// flange's side or past its corner (11 offsets, both ways, the three booleans: 66 planar
/// booleans on a kernel with no fixes). Each builds one region bounded by line segments that
/// end within one intersection tolerance of each other (a tolerant result's curve ends lie
/// within it of their shared vertex), and the polygon of its vertices has the exact polygon
/// boolean's area (computed here from the vertices: a convex clip) to rounding when the
/// taper passes the corner farther than one intersection tolerance, and within the sliver
/// the tolerance cannot resolve (`sliver_bound`) when it passes closer.
#[test]
fn a_taper_near_a_flange_corner_cuts_to_the_exact_area_or_within_its_sliver() {
    session::set_thread_fixes(Vec::<&str>::new()).unwrap();
    let tx = solidcore::kernel::Kernel::default().tolerances.xsect_tol_3d;
    let (flange_pts, corner) = flange_outline();
    let flange = Region2 {
        outer: Loop2::polygon(&flange_pts),
        holes: vec![],
    };
    let mut failures = Vec::new();
    for d in [
        0.0, 1e-5, -1e-5, 3e-5, -3e-5, 1e-4, -1e-4, 1.2e-4, -1.2e-4, 1e-3, -1e-3,
    ] {
        for along in [true, false] {
            let cut_pts = flange_cut(d, along);
            let cut = Region2 {
                outer: Loop2::polygon(&cut_pts),
                holes: vec![],
            };
            let exact = flange_cut_exact(&flange_pts, &cut_pts);
            for (op, exact) in [BooleanOp::Intersect, BooleanOp::Subtract, BooleanOp::Union]
                .into_iter()
                .zip(exact)
            {
                let want = polygon_area(&exact);
                let bound = sliver_bound(&exact, corner, cut_pts[0], cut_pts[1], tx) + 1e-9;
                let case = format!("d = {d:e}, along = {along}, {op:?}");
                match planar_regions_boolean(
                    std::slice::from_ref(&flange),
                    std::slice::from_ref(&cut),
                    op,
                    1e-7,
                ) {
                    Ok(out) => {
                        let corners: Vec<Vec2> = out
                            .iter()
                            .flat_map(|r| r.outer.segs.iter().map(start))
                            .collect();
                        let got = polygon_area(&corners);
                        let ok = out.len() == 1
                            && out[0].holes.is_empty()
                            && out[0]
                                .outer
                                .segs
                                .iter()
                                .all(|s| matches!(s, Seg2::Line { .. }))
                            && closed(&out[0].outer, tx)
                            && (got - want).abs() <= bound;
                        if !ok {
                            failures.push(format!(
                                "{case}: {} regions, area {got:.12}, exact {want:.12}, off {:e}, bound {bound:e}: {out:?}",
                                out.len(),
                                got - want
                            ));
                        }
                    }
                    Err(e) => failures.push(format!("{case}: refused: {e:?}")),
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn volume(b: &Body) -> f64 {
    b.volume(1e-8).unwrap()
}

#[test]
fn compound_planar_extrusion_normalizes_overlap_and_retains_islands() {
    all_fixes();
    let plane = Plane::XY;
    let a = rectangle(Vec2::ZERO, v(10.0, 10.0));
    let b = rectangle(v(5.0, 0.0), v(15.0, 10.0));
    let body = extrude_planar_regions(&plane, &[a.clone(), b], 0.0, 2.0).unwrap();
    assert!((volume(&body) - 300.0).abs() < 1e-6);
    let inner = Region2 {
        outer: Loop2::circle(v(5.0, 5.0), 2.0),
        holes: vec![],
    };
    let body = extrude_planar_regions(&plane, &[a.clone(), inner.clone()], 0.0, 2.0).unwrap();
    assert!((volume(&body) - 200.0).abs() < 1e-6);
    let mut hole = Loop2::circle(v(5.0, 5.0), 3.0);
    hole.segs = hole.segs.iter().map(reversed).collect();
    let annulus = Region2 {
        outer: a.outer,
        holes: vec![hole],
    };
    let body = extrude_planar_regions(&plane, &[annulus, inner], 0.0, 2.0).unwrap();
    let expected = (100.0 - PI * 5.0) * 2.0;
    assert!((volume(&body) - expected).abs() < 0.1);
    assert_eq!(body.solid_count().unwrap(), 2);
    let a = rectangle(Vec2::ZERO, v(10.0, 10.0));
    let b = rectangle(v(10.0, 10.0), v(20.0, 20.0));
    assert!(matches!(
        extrude_planar_regions(&plane, &[a, b], 0.0, 2.0),
        Err(Error::Unsupported(_))
    ));
    let circles = [Vec2::ZERO, v(10.0, 0.0)].map(|center| Region2 {
        outer: Loop2::circle(center, 5.0),
        holes: vec![],
    });
    assert!(extrude_planar_regions(&plane, &circles, 0.0, 2.0).is_err());
}

/// An ellipse about `c` with semi-axes `a` (along x) and `b`, as four exact conics.
fn ellipse(c: Vec2, a: f64, b: f64) -> Loop2 {
    let corners = [c + v(a, 0.0), c + v(0.0, b), c - v(a, 0.0), c - v(0.0, b)];
    Loop2 {
        segs: (0..4)
            .map(|i| Seg2::Conic {
                a: corners[i],
                apex: corners[i] + corners[(i + 1) % 4] - c,
                b: corners[(i + 1) % 4],
                w: FRAC_1_SQRT_2,
            })
            .collect(),
    }
}

#[test]
fn free_form_cut_outs_stay_exact_whole_or_cut() {
    all_fixes();
    let plate = rectangle(Vec2::ZERO, v(100.0, 60.0));
    let hole = Region2 {
        outer: ellipse(v(40.0, 30.0), 15.0, 8.0),
        holes: vec![],
    };
    let out = planar_regions_boolean(
        std::slice::from_ref(&plate),
        std::slice::from_ref(&hole),
        BooleanOp::Subtract,
        1e-7,
    )
    .unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].holes.len(), 1);
    // the whole ellipse comes back as its four exact conics, running as a hole (clockwise)
    let h = &out[0].holes[0];
    assert_eq!(h.segs.len(), 4);
    assert!(
        h.segs
            .iter()
            .all(|s| hole.outer.segs.iter().any(|t| same_seg(&reversed(s), t))),
        "{h:?}"
    );
    assert!(loop_area(h) < 0.0);
    let exact = 6000.0 - PI * 15.0 * 8.0;
    assert!(
        (area(&out[0]) - exact).abs() < 1e-9,
        "{} vs {exact}",
        area(&out[0])
    );
    // what the cut takes is the ellipse, whole and exact, counter-clockwise
    let taken = planar_regions_boolean(
        std::slice::from_ref(&plate),
        std::slice::from_ref(&hole),
        BooleanOp::Intersect,
        1e-7,
    )
    .unwrap();
    assert!(
        taken.len() == 1
            && taken[0]
                .outer
                .segs
                .iter()
                .all(|s| hole.outer.segs.iter().any(|t| same_seg(s, t))),
        "{taken:?}"
    );
    // an ellipse centred on the plate's edge makes a notch of exactly half the ellipse
    let over = Region2 {
        outer: ellipse(v(100.0, 30.0), 15.0, 8.0),
        holes: vec![],
    };
    let out = planar_regions_boolean(
        std::slice::from_ref(&plate),
        std::slice::from_ref(&over),
        BooleanOp::Subtract,
        1e-7,
    )
    .unwrap();
    assert_eq!(out.len(), 1);
    assert!(out[0].holes.is_empty() && !out[0].outer.segs.iter().all(is_line_or_arc));
    let notched = 6000.0 - PI * 15.0 * 8.0 / 2.0;
    assert!(
        (area(&out[0]) - notched).abs() < 1e-6,
        "{} vs {notched}",
        area(&out[0])
    );
    // two ellipses that overlap join into one outline of their conics; apart, both stay whole
    let beside = |dx: f64| Region2 {
        outer: ellipse(v(40.0 + dx, 30.0), 15.0, 8.0),
        holes: vec![],
    };
    let joined = planar_regions_boolean(
        std::slice::from_ref(&hole),
        &[beside(20.0)],
        BooleanOp::Union,
        1e-7,
    )
    .unwrap();
    assert!(
        joined.len() == 1 && joined[0].outer.segs.iter().all(|s| !is_line_or_arc(s)),
        "{joined:?}"
    );
    let apart = planar_regions_boolean(
        std::slice::from_ref(&hole),
        &[beside(40.0)],
        BooleanOp::Union,
        1e-7,
    )
    .unwrap();
    assert!(
        apart.len() == 2 && apart.iter().all(|r| r.outer.segs.len() == 4),
        "{apart:?}"
    );
    // the flat material extrudes with the exact elliptic wall: four conic faces, two caps
    let cut = planar_regions_boolean(&[plate], &[hole], BooleanOp::Subtract, 1e-7).unwrap();
    let body = extrude_planar_regions(&Plane::XY, &cut, 0.0, 2.0).unwrap();
    assert_eq!(body.counts().unwrap()[0], 4 + 4 + 2);
    let vol = volume(&body);
    assert!((vol - exact * 2.0).abs() < 1e-3 * exact, "{vol}");
}

#[test]
fn free_form_loops_that_are_not_finite_are_refused() {
    all_fixes();
    let plate = rectangle(Vec2::ZERO, v(100.0, 60.0));
    let mut bad = ellipse(v(40.0, 30.0), 15.0, 8.0);
    let (a, b) = (start(&bad.segs[1]), end(&bad.segs[1]));
    bad.segs[1] = Seg2::Conic {
        a,
        apex: v(f64::NAN, 0.0),
        b,
        w: 1.0,
    };
    let e = planar_regions_boolean(
        std::slice::from_ref(&plate),
        &[Region2 {
            outer: bad,
            holes: vec![],
        }],
        BooleanOp::Subtract,
        1e-7,
    )
    .unwrap_err();
    assert!(e.to_string().contains("free-form"), "{e}");
}

#[test]
fn many_small_cut_outs_far_apart_cut_as_before() {
    all_fixes();
    let plate = rectangle(Vec2::ZERO, v(2000.0, 20.0));
    let holes: Vec<Region2> = (0..400)
        .map(|i| Region2 {
            outer: Loop2::circle(v(2.5 + 5.0 * f64::from(i), 10.0), 1.8),
            holes: vec![],
        })
        .collect();
    let out = planar_regions_boolean(&[plate], &holes, BooleanOp::Subtract, 1e-7).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].holes.len(), 400);
    let exact = 40_000.0 - 400.0 * PI * 1.8 * 1.8;
    assert!((area(&out[0]) - exact).abs() < 1e-6, "{}", area(&out[0]));
}

/// Loops close only within the chaining tolerance: a neighbour with an edge level with a gap of
/// one joins the square instead of being dropped as if it lay inside it.
#[test]
fn ray_parity_closes_the_gaps_chaining_leaves() {
    all_fixes();
    let gap = 1e-13;
    let square = Loop2 {
        segs: vec![
            Seg2::Line {
                a: v(0.0, 0.0),
                b: v(10.0, 0.0),
            },
            Seg2::Line {
                a: v(10.0, 0.0),
                b: v(10.0, 10.0),
            },
            Seg2::Line {
                a: v(10.0, 10.0),
                b: v(0.0, 10.0),
            },
            Seg2::Line {
                a: v(0.0, 10.0),
                b: v(0.0, 5.0 + gap),
            },
            Seg2::Line {
                a: v(0.0, 5.0),
                b: v(0.0, 0.0),
            },
        ],
    };
    let a = Region2 {
        outer: square,
        holes: vec![],
    };
    let b = rectangle(v(-4.0, 2.0), v(0.0, 5.0));
    let union = planar_regions_boolean(&[a], &[b], BooleanOp::Union, 1e-7).unwrap();
    assert_eq!(union.len(), 1);
    assert!(
        (area(&union[0]).abs() - 112.0).abs() < 1e-9,
        "{}",
        area(&union[0])
    );
}

/// Edges on one line within the tolerance, a hair from parallel, are one edge of the result:
/// where they meet is no corner.
#[test]
fn overlapping_lines_a_hair_from_parallel_do_not_cross() {
    all_fixes();
    let square = rectangle(Vec2::ZERO, v(10.0, 10.0));
    let quad = |meet: f64, far: f64| {
        let on = |x: f64| v(x, (meet - x) * 1e-9);
        let mut corners = [on(4.0), on(14.0), v(14.0, far), v(4.0, far)];
        if far < 0.0 {
            corners.reverse();
        }
        Region2 {
            outer: Loop2::polygon(&corners),
            holes: vec![],
        }
    };
    let lengths = |r: &Region2| {
        let mut all: Vec<f64> = r.outer.segs.iter().map(length).collect();
        all.sort_by(f64::total_cmp);
        all
    };
    let same = |got: &[f64], want: &[f64]| {
        got.len() == want.len() && got.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-6)
    };
    for meet in [10.0 - 5e-8, 7.0] {
        let below = quad(meet, -5.0);
        let cut = planar_regions_boolean(
            std::slice::from_ref(&square),
            std::slice::from_ref(&below),
            BooleanOp::Subtract,
            1e-7,
        )
        .unwrap();
        assert_eq!(cut.len(), 1, "{meet}");
        assert!(
            same(&lengths(&cut[0]), &[10.0, 10.0, 10.0, 10.0]),
            "{meet}: cut edges {:?}",
            lengths(&cut[0])
        );
        assert!(
            (area(&cut[0]) - 100.0).abs() < 1e-6,
            "{meet}: {}",
            area(&cut[0])
        );
        let flush = quad(meet, 5.0);
        let union = planar_regions_boolean(
            std::slice::from_ref(&square),
            std::slice::from_ref(&flush),
            BooleanOp::Union,
            1e-7,
        )
        .unwrap_or_else(|e| panic!("{meet}: union {e}"));
        assert_eq!(union.len(), 1, "{meet}");
        assert!(
            same(&lengths(&union[0]), &[4.0, 5.0, 5.0, 10.0, 10.0, 14.0]),
            "{meet}: union edges {:?}",
            lengths(&union[0])
        );
        assert!(
            (area(&union[0]) - 120.0).abs() < 1e-6,
            "{meet}: {}",
            area(&union[0])
        );
    }
}

/// A tool smaller than a display chord's sagitta between the circle and the chord is cut, not
/// lost: the booleans never read the circle as a polyline.
#[test]
fn tiny_tool_between_circle_and_display_chord_is_not_lost() {
    all_fixes();
    let circle = Region2 {
        outer: Loop2::circle(Vec2::ZERO, 10.0),
        holes: vec![],
    };
    // a chord of 1e-3 sagitta spans 2 sqrt(2 r s) of arc: the square sits outside that chord
    let half_angle = (2.0 * 10.0 * 1e-3_f64).sqrt() / 10.0;
    let at = v(half_angle.cos(), half_angle.sin()) * 9.9998;
    let half = v(0.000025, 0.000025);
    let square = rectangle(at - half, at + half);
    assert!(9.9998 + 0.000025 * 2.0_f64.sqrt() < 10.0);
    assert!(
        9.9998 - 0.000025 * 2.0_f64.sqrt() > 10.0 * half_angle.cos(),
        "outside the chord"
    );
    let cut = planar_regions_boolean(
        std::slice::from_ref(&circle),
        std::slice::from_ref(&square),
        BooleanOp::Subtract,
        1e-7,
    )
    .unwrap();
    assert_eq!(cut.len(), 1);
    assert_eq!(cut[0].holes.len(), 1);
    assert!((loop_area(&cut[0].holes[0]).abs() - 2.5e-9).abs() < 1e-13);
    let intersect =
        planar_regions_boolean(&[circle], &[square], BooleanOp::Intersect, 1e-7).unwrap();
    assert_eq!(intersect.len(), 1);
    assert!((area(&intersect[0]) - 2.5e-9).abs() < 1e-13);
}

/// A 200 x 37 plate turned by `theta` about the origin, and the fillet region of one of its
/// corners: an arc of radius `r` tangent to both edges, closed by two lines meeting `reach` out.
fn plate_and_corner_fillet(theta: f64, r: f64, reach: f64) -> (Region2, Region2) {
    let (sn, cs) = theta.sin_cos();
    let rot = |x: f64, y: f64| v(x * cs - y * sn, x * sn + y * cs);
    let (w, lo, hi) = (200.0, -40.25, -3.25);
    let plate = Region2 {
        outer: Loop2::polygon(&[rot(0.0, hi), rot(0.0, lo), rot(w, lo), rot(w, hi)]),
        holes: vec![],
    };
    let arc = Seg2::Arc {
        center: rot(w - r, lo + r),
        radius: r,
        start: theta,
        sweep: -FRAC_PI_2,
    };
    let out = rot(w + reach, lo - reach);
    let cut = Region2 {
        outer: Loop2 {
            segs: vec![
                arc,
                Seg2::Line {
                    a: end(&arc),
                    b: out,
                },
                Seg2::Line {
                    a: out,
                    b: start(&arc),
                },
            ],
        },
        holes: vec![],
    };
    (plate, cut)
}

/// A line and an arc that touch meet at one point: the cut leaves no hairline edge along the
/// plate's edge past the arc's end, at every turn of the plate.
#[test]
fn an_arc_tangent_to_the_edges_it_ends_on_leaves_no_hairline_edges() {
    all_fixes();
    for k in 0..=360 {
        let theta = (f64::from(k) * 0.25).to_radians();
        for r in [2.0, 0.5, 25.0] {
            let (plate, cut) = plate_and_corner_fillet(theta, r, 0.4);
            let out = planar_regions_boolean(
                std::slice::from_ref(&plate),
                std::slice::from_ref(&cut),
                BooleanOp::Subtract,
                1e-7,
            )
            .unwrap_or_else(|e| panic!("{k} r {r}: {e}"));
            assert_eq!(out.len(), 1, "{k} r {r}");
            let lp = &out[0].outer;
            let shortest = lp.segs.iter().map(length).fold(f64::MAX, f64::min);
            let arcs = lp
                .segs
                .iter()
                .filter(|s| matches!(s, Seg2::Arc { .. }))
                .count();
            assert!(
                out[0].holes.is_empty() && lp.segs.len() == 5 && arcs == 1 && shortest > 0.5,
                "{k} r {r}: {} edges, shortest {shortest:e}",
                lp.segs.len()
            );
            let want = 200.0 * 37.0 - r * r * (1.0 - FRAC_PI_4);
            assert!(
                (area(&out[0]) - want).abs() < 1e-9,
                "{k} r {r}: {} vs {want}",
                area(&out[0])
            );
            let taken = planar_regions_boolean(
                std::slice::from_ref(&plate),
                std::slice::from_ref(&cut),
                BooleanOp::Intersect,
                1e-7,
            )
            .unwrap();
            assert!(
                taken.len() == 1 && taken[0].outer.segs.len() == 3,
                "{k} r {r}: {taken:?}"
            );
            assert!(
                (area(&taken[0]) - r * r * (1.0 - FRAC_PI_4)).abs() < 1e-9,
                "{k} r {r}"
            );
        }
    }
}

/// A plate whose bottom edge bends up by `slope` over its last 5 mm, and a strip below its
/// straight 50 mm, its corners on that edge rounded to the nanometre and moved by `off`; both
/// turned by `theta`.
fn plate_and_strip_by_a_bent_edge(theta: f64, slope: f64, off: Vec2) -> (Region2, Region2) {
    let (sn, cs) = theta.sin_cos();
    let rot = |x: f64, y: f64| v(x * cs - y * sn, x * sn + y * cs);
    let nm = |p: Vec2| v((p.x * 1e9).round() / 1e9, (p.y * 1e9).round() / 1e9) + off;
    let plate = [
        rot(95.0, 50.0 + 5.0 * slope),
        rot(100.0, 50.0),
        rot(150.0, 50.0),
        rot(150.0, 80.0),
        rot(95.0, 80.0),
    ];
    let strip = [
        nm(rot(100.0, 50.0)),
        rot(100.0, 47.0),
        rot(150.0, 47.0),
        nm(rot(150.0, 50.0)),
    ];
    (
        Region2 {
            outer: Loop2::polygon(&plate),
            holes: vec![],
        },
        Region2 {
            outer: Loop2::polygon(&strip),
            holes: vec![],
        },
    )
}

/// The bent piece of the plate's edge crosses the strip's edge short of their shared corner: the
/// union is the plate and the strip whole, six edges, no sliver.
#[test]
fn a_bent_edge_crossing_its_neighbour_by_their_corner_leaves_no_sliver() {
    all_fixes();
    let union_is_whole = |(plate, strip): (Region2, Region2), what: &str| {
        let want = area(&plate) + area(&strip);
        for (a, b) in [(&plate, &strip), (&strip, &plate)] {
            let out = planar_regions_boolean(
                std::slice::from_ref(a),
                std::slice::from_ref(b),
                BooleanOp::Union,
                1e-7,
            )
            .unwrap_or_else(|e| panic!("{what}: {e}"));
            assert_eq!(out.len(), 1, "{what}: {out:?}");
            let lp = &out[0].outer;
            let shortest = lp.segs.iter().map(length).fold(f64::MAX, f64::min);
            assert!(
                out[0].holes.is_empty() && lp.segs.len() == 6 && shortest > 2.9,
                "{what}: {} edges, shortest {shortest:e}",
                lp.segs.len()
            );
            assert!(
                (area(&out[0]) - want).abs() < 1e-6,
                "{what}: {} vs {want}",
                area(&out[0])
            );
        }
    };
    for k in 0..360 {
        for slope in [0.01, 0.001] {
            union_is_whole(
                plate_and_strip_by_a_bent_edge(
                    (f64::from(k) * 0.25).to_radians(),
                    slope,
                    Vec2::ZERO,
                ),
                &format!("{k} slope {slope}"),
            );
        }
    }
    for k in 0..16 {
        for d in [1e-9, 4e-9] {
            let a = f64::from(k) * TAU / 16.0;
            let off = v(a.cos() * d, a.sin() * d);
            union_is_whole(
                plate_and_strip_by_a_bent_edge(0.0, 0.01, off),
                &format!("off {off:?}"),
            );
        }
    }
}

/// Two disjoint squares intersect in nothing: the kernel's 2D boolean answers with a sheet of no
/// faces, which is an empty result, not a sheet to read back (that refused with a query failure).
#[test]
fn disjoint_squares_intersect_in_nothing() {
    let a = rectangle(v(0., 0.), v(10., 10.));
    let b = rectangle(v(20., 0.), v(30., 10.));
    for (x, y) in [(&a, &b), (&b, &a)] {
        let out = planar_regions_boolean(
            std::slice::from_ref(x),
            std::slice::from_ref(y),
            BooleanOp::Intersect,
            1e-7,
        );
        assert_eq!(out, Ok(vec![]));
    }
    let union = planar_regions_boolean(
        std::slice::from_ref(&a),
        std::slice::from_ref(&b),
        BooleanOp::Union,
        1e-7,
    )
    .unwrap();
    assert_eq!(union.len(), 2);
    assert!(union.iter().all(|r| (area(r) - 100.0).abs() < 1e-9));
}

/// Whether `a` and `b` are the same regions in the same order: loop for loop, segment for
/// segment of the same kind, every number within 1e-12.
fn same_regions(a: &[Region2], b: &[Region2]) -> bool {
    let near = |x: f64, y: f64| (x - y).abs() <= 1e-12;
    let pt = |p: Vec2, q: Vec2| near(p.x, q.x) && near(p.y, q.y);
    let seg = |s: &Seg2, t: &Seg2| match (*s, *t) {
        (Seg2::Line { a, b }, Seg2::Line { a: c, b: d }) => pt(a, c) && pt(b, d),
        (
            Seg2::Arc {
                center,
                radius,
                start,
                sweep,
            },
            Seg2::Arc {
                center: c,
                radius: r,
                start: s0,
                sweep: w,
            },
        ) => pt(center, c) && near(radius, r) && near(start, s0) && near(sweep, w),
        _ => same_seg(s, t),
    };
    let lp = |l: &Loop2, m: &Loop2| {
        l.segs.len() == m.segs.len() && l.segs.iter().zip(&m.segs).all(|(s, t)| seg(s, t))
    };
    a.len() == b.len()
        && a.iter().zip(b).all(|(r, q)| {
            lp(&r.outer, &q.outer)
                && r.holes.len() == q.holes.len()
                && r.holes.iter().zip(&q.holes).all(|(h, g)| lp(h, g))
        })
}

/// The result of a planar boolean comes in one canonical order whatever order its operands came
/// in: each loop starts at its smallest start point (x, then y), holes and regions sorted by that
/// point, and every arc starts at an angle in (-π, π].
#[test]
fn shuffled_operands_give_the_same_regions() {
    let circle = |c: Vec2, r: f64, start: f64| Loop2 {
        segs: vec![
            Seg2::Arc {
                center: c,
                radius: r,
                start,
                sweep: PI,
            },
            Seg2::Arc {
                center: c,
                radius: r,
                start: start + PI,
                sweep: PI,
            },
        ],
    };
    let mut ring = rectangle(v(0., 0.), v(20., 20.));
    ring.holes = vec![circle(v(6., 6.), 2., PI), circle(v(14., 14.), 3., 0.5)];
    let a = vec![
        ring,
        rectangle(v(30., 0.), v(40., 10.)),
        Region2 {
            outer: circle(v(60., 5.), 5., -PI),
            holes: vec![],
        },
    ];
    let b = vec![rectangle(v(15., -5.), v(65., 3.))];
    // the same regions: in another order, each loop turned to start elsewhere, holes swapped
    let turned = |l: &Loop2, n: usize| {
        let mut segs = l.segs.clone();
        let len = segs.len();
        segs.rotate_left(n % len);
        Loop2 { segs }
    };
    let shuffled: Vec<Region2> = a
        .iter()
        .rev()
        .map(|r| Region2 {
            outer: turned(&r.outer, 1),
            holes: r.holes.iter().rev().map(|h| turned(h, 1)).collect(),
        })
        .collect();
    for op in [BooleanOp::Union, BooleanOp::Subtract, BooleanOp::Intersect] {
        let one = planar_regions_boolean(&a, &b, op, 1e-7).unwrap();
        let two = planar_regions_boolean(&shuffled, &b, op, 1e-7).unwrap();
        let three = planar_regions_boolean(&b, &shuffled, op, 1e-7).unwrap();
        assert!(!one.is_empty());
        // the same regions, loops and segments in the same order; the numbers differ only by
        // the rounding of the operands' other order (well under 1e-12 here)
        assert!(same_regions(&one, &two), "{op:?}: {one:?} {two:?}");
        if op != BooleanOp::Subtract {
            assert!(same_regions(&one, &three), "{op:?}: {one:?} {three:?}");
        }
        let loops: Vec<&Loop2> = one
            .iter()
            .flat_map(|r| std::iter::once(&r.outer).chain(&r.holes))
            .collect();
        for l in &loops {
            let first = start(&l.segs[0]);
            assert!(l.segs.iter().all(|s| {
                let p = start(s);
                (first.x, first.y) <= (p.x, p.y)
            }));
            for s in &l.segs {
                if let Seg2::Arc { start, .. } = *s {
                    assert!(start > -PI && start <= PI, "{start}");
                }
            }
        }
        let firsts: Vec<(f64, f64)> = one
            .iter()
            .map(|r| {
                let p = start(&r.outer.segs[0]);
                (p.x, p.y)
            })
            .collect();
        assert!(firsts.windows(2).all(|w| w[0] <= w[1]), "{firsts:?}");
    }
}

/// The canonical order does not hang on rounding noise. Two regions whose leftmost edges are
/// vertical, at x values that differ by one ulp only, one way round and then the other: compared
/// exactly, the smaller x would pick each loop's start corner and the regions' order. On the grid
/// of the tolerance the x values are equal, so y decides: each loop starts at its lower left
/// corner and the lower region comes first, in both variants.
#[test]
fn leftmost_points_a_rounding_apart_keep_one_order() {
    let ulp = f64::EPSILON;
    let variant = |lo: f64, hi: f64| {
        // lower region: left edge from (lo, 0) to (hi, 4); upper one: from (hi, 10) to (lo, 14)
        let lower = Region2 {
            outer: Loop2::polygon(&[v(lo, 0.), v(5., 0.), v(5., 4.), v(hi, 4.)]),
            holes: vec![],
        };
        let upper = Region2 {
            outer: Loop2::polygon(&[v(hi, 10.), v(5., 10.), v(5., 14.), v(lo, 14.)]),
            holes: vec![],
        };
        let far = rectangle(v(100., 0.), v(110., 10.));
        planar_regions_boolean(&[upper, lower], &[far], BooleanOp::Union, 1e-7).unwrap()
    };
    let one = variant(1.0 + ulp, 1.0);
    let two = variant(1.0, 1.0 + ulp);
    for out in [&one, &two] {
        assert_eq!(out.len(), 3);
        let firsts: Vec<Vec2> = out.iter().map(|r| start(&r.outer.segs[0])).collect();
        for (p, y) in firsts.iter().zip([0., 10., 0.]) {
            assert_eq!(p.y, y, "{firsts:?}");
        }
        assert!((firsts[0].x - 1.0).abs() <= ulp && (firsts[1].x - 1.0).abs() <= ulp);
    }
    assert!(same_regions(&one, &two), "{one:?} {two:?}");
}

/// The canonical order does not split the grid cell at zero. Two regions whose leftmost edges are
/// vertical and straddle x = 0 by a hair (`-d` and `d`, `d` far under the tolerance), one way
/// round and then the other: `-d` rounds to the cell -0, which `total_cmp` orders before the
/// cell 0 of `d`, so the corner at `-d` would start each loop whatever its y. Both x values lie
/// in the one cell at zero, so y decides: each loop starts at its lower left corner and the
/// lower region comes first, in both variants.
#[test]
fn leftmost_points_straddling_zero_keep_one_order() {
    let d = 1e-15;
    let variant = |lo: f64, hi: f64| {
        // lower region: left edge from (lo, 0) to (hi, 4); upper one: from (hi, 10) to (lo, 14)
        let lower = Region2 {
            outer: Loop2::polygon(&[v(lo, 0.), v(5., 0.), v(5., 4.), v(hi, 4.)]),
            holes: vec![],
        };
        let upper = Region2 {
            outer: Loop2::polygon(&[v(hi, 10.), v(5., 10.), v(5., 14.), v(lo, 14.)]),
            holes: vec![],
        };
        let far = rectangle(v(100., 0.), v(110., 10.));
        planar_regions_boolean(&[upper, lower], &[far], BooleanOp::Union, 1e-7).unwrap()
    };
    let one = variant(-d, d);
    let two = variant(d, -d);
    for out in [&one, &two] {
        assert_eq!(out.len(), 3);
        let firsts: Vec<Vec2> = out.iter().map(|r| start(&r.outer.segs[0])).collect();
        for (p, y) in firsts.iter().zip([0., 10., 0.]) {
            assert_eq!(p.y, y, "{firsts:?}");
        }
        assert!(
            firsts[0].x.abs() <= d && firsts[1].x.abs() <= d,
            "{firsts:?}"
        );
    }
}

/// Planar booleans SolidCraft's sheet tests make (a channel engraved, a sheet with small bends
/// and holes, a sheet perforated with a thousand round holes: each operand pair recorded from the
/// call), where a face of one operand lies far from the other's plane face. The 2D boolean
/// rebuilds the faces on the first face's plane after growing it to cover them, and a plane grew
/// only 100 of its widths: the faces beyond were rebuilt on a plane that did not hold them
/// (WR-536), and the call failed (1001, 1006, 1007, or an edge off the plane). Each case's union,
/// intersection and both differences now build, closed, and their areas agree: the union is the
/// two differences and the intersection, which share no area, and it is also the operands' areas
/// (from the inputs) less the intersection's: area(A) + area(B) - area(A and B).
#[test]
fn far_faces_of_recorded_sheet_cuts_build_and_their_areas_agree() {
    use std::io::BufRead;
    #[derive(serde::Deserialize)]
    struct Case {
        test: String,
        tol: f64,
        a: Vec<Region2>,
        b: Vec<Region2>,
    }
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/planar_regions_far_faces.jsonl.gz"
    );
    let gz = flate2::read::GzDecoder::new(std::fs::File::open(path).unwrap());
    let mut n = 0;
    for line in std::io::BufReader::new(gz).lines() {
        let c: Case = serde_json::from_str(&line.unwrap()).unwrap();
        let run = |x: &[Region2], y: &[Region2], op| {
            let out = planar_regions_boolean(x, y, op, c.tol)
                .unwrap_or_else(|e| panic!("case {n} ({}), {op:?}: {e:?}", c.test));
            for l in out
                .iter()
                .flat_map(|r| std::iter::once(&r.outer).chain(&r.holes))
            {
                assert!(closed(l, c.tol * 10.0), "case {n} ({}), {op:?}", c.test);
            }
            out.iter().map(area).sum::<f64>()
        };
        let union = run(&c.a, &c.b, BooleanOp::Union);
        let common = run(&c.a, &c.b, BooleanOp::Intersect);
        let a_less_b = run(&c.a, &c.b, BooleanOp::Subtract);
        let b_less_a = run(&c.b, &c.a, BooleanOp::Subtract);
        let parts = a_less_b + b_less_a + common;
        assert!(
            (union - parts).abs() <= 1e-9 * union,
            "case {n} ({}): union {union}, parts {parts}",
            c.test
        );
        // the operands' own areas, from the inputs: a result that drops faces (far ones, say)
        // loses their area from the union and from the differences alike, which the check above
        // cannot see
        let area_a = c.a.iter().map(area).sum::<f64>();
        let area_b = c.b.iter().map(area).sum::<f64>();
        let whole = area_a + area_b - common;
        assert!(
            (union - whole).abs() <= 1e-9 * (area_a + area_b),
            "case {n} ({}): union {union}, area(A) + area(B) - area(A and B) {whole}",
            c.test
        );
        n += 1;
    }
    assert_eq!(n, 35);
}

#[test]
fn planar_region_api_rejects_hostile_values_and_budgets() {
    all_fixes();
    let plate = rectangle(Vec2::ZERO, v(10.0, 10.0));
    for tolerance in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
        assert_eq!(
            planar_regions_boolean(
                std::slice::from_ref(&plate),
                &[],
                BooleanOp::Union,
                tolerance
            ),
            Err(Error::Argument)
        );
    }
    let mut bad = plate.clone();
    bad.outer.segs[0] = Seg2::Line {
        a: v(f64::NAN, 0.0),
        b: Vec2::X,
    };
    assert!(matches!(
        planar_regions_boolean(&[bad], &[], BooleanOp::Union, 1e-7),
        Err(Error::Profile(_))
    ));
    let many = vec![plate; solidcore::prism::profile::MAX_SEGMENTS / 4 + 1];
    assert!(matches!(
        planar_regions_boolean(&many, &[], BooleanOp::Union, 1e-7),
        Err(Error::Profile(_))
    ));
}

#[test]
fn a_prism_section_does_not_depend_on_the_query_frame_origin() {
    all_fixes();
    let body = make_cylinder([50.0, 50.0, -3.0], 5.0, 6.0).unwrap();
    let exact_volume = 150.0 * PI;
    for offset in [0.0, 300.0, 1000.0, 10000.0, -10000.0] {
        let plane = Plane {
            origin: Vec3::new(offset, offset, 7.0),
            ..Plane::XY
        };
        let section = prism_section(&body, &plane)
            .unwrap()
            .unwrap_or_else(|| panic!("the same cylinder is a prism at origin {offset}"));
        assert!((section.lo + 10.0).abs() <= section.tolerance);
        assert!((section.hi + 4.0).abs() <= section.tolerance);
        assert!(section.volume.lo <= exact_volume && exact_volume <= section.volume.hi);
        assert!(section.region.holes.is_empty());
        for segment in &section.region.outer.segs {
            let Seg2::Arc { center, radius, .. } = segment else {
                panic!("a cylinder's section is circular: {segment:?}");
            };
            assert!((center.x + offset - 50.0).abs() < 1e-8);
            assert!((center.y + offset - 50.0).abs() < 1e-8);
            assert!((radius - 5.0).abs() < 1e-8);
        }
    }
    for offset in [1e20, f64::MAX] {
        let plane = Plane {
            origin: Vec3::new(offset, offset, 7.0),
            ..Plane::XY
        };
        assert!(prism_section(&body, &plane).unwrap().is_none());
    }
    let moved = make_cylinder([500.0, 500.0, -3.0], 5.0, 6.0).unwrap();
    let plane = Plane {
        origin: Vec3::new(500.0, 500.0, 0.0),
        ..Plane::XY
    };
    let section = prism_section(&moved, &plane).unwrap().unwrap();
    assert!((section.lo + 3.0).abs() <= section.tolerance);
    assert!((section.hi - 3.0).abs() <= section.tolerance);
    assert!(section.volume.lo <= exact_volume && exact_volume <= section.volume.hi);
}

#[test]
fn a_prism_section_refuses_a_wall_displaced_from_its_caps() {
    use solidcore_record::brep::Surface;

    all_fixes();
    let body = make_cylinder([50.0, 50.0, -3.0], 5.0, 6.0).unwrap();
    let (mut record, _) = body.to_record().unwrap();
    let wall = record
        .faces
        .iter_mut()
        .find(|face| matches!(face.surface, Surface::Cylinder { .. }))
        .unwrap();
    let Surface::Cylinder { frame, .. } = &mut wall.surface else {
        unreachable!()
    };
    frame.origin[2] += 1000.0;
    let Some(Surface::RationalSpline { homogeneous, .. }) = &mut wall.nurbs else {
        panic!("the cylinder's wall has a rational net")
    };
    for p in homogeneous.iter_mut().flatten() {
        p[2] += 1000.0 * p[3];
    }
    let (displaced, _) = Body::from_record(&record, true).unwrap();
    assert!(prism_section(&displaced, &Plane::XY).unwrap().is_none());
}

/// A box cut by a cylinder through it, moved: a prism along its moved axis, and the same prism
/// as itself moved the same way; the drilled box against a box drilled wider is not.
#[test]
fn a_drilled_box_moved_is_the_prism_it_was() {
    all_fixes();
    let drilled = |r: f64| {
        let stock = make_box([0.0, 0.0, 0.0], [20.0, 20.0, 10.0]).unwrap();
        let cutter = make_cylinder([10.0, 10.0, -1.0], r, 12.0).unwrap();
        solidcore_ops::boolean::boolean(stock, cutter, BooleanOp::Subtract)
            .unwrap()
            .unwrap()
    };
    let source = drilled(6.0);
    let s: PrismSection = prism_section(&source, &Plane::XY)
        .unwrap()
        .expect("a drilled box is a prism");
    let exact = 4000.0 - 360.0 * PI;
    assert!(
        s.volume.lo <= exact && s.volume.hi >= exact,
        "{:?}",
        s.volume
    );
    assert!(s.lo.abs() <= s.tolerance && (s.hi - 10.0).abs() <= s.tolerance);
    let twin = drilled(6.0);
    assert_eq!(prism_equivalent(&source, &twin).unwrap(), Some(true));
    assert_eq!(prism_equivalent(&source, &source).unwrap(), Some(true));
    assert_eq!(
        prism_equivalent(&source, &drilled(6.0001)).unwrap(),
        Some(false)
    );
    // turned 0.7 rad about (1, 2, 3) through the origin, and moved: still the same prism
    let (c, sn) = (0.7_f64.cos(), 0.7_f64.sin());
    let n = {
        let l = 14.0_f64.sqrt();
        [1.0 / l, 2.0 / l, 3.0 / l]
    };
    let mut m = [[0.0; 4]; 4];
    for (i, row) in m.iter_mut().enumerate().take(3) {
        for (j, x) in row.iter_mut().enumerate().take(3) {
            let id = if i == j { 1.0 } else { 0.0 };
            let cross = match (i, j) {
                (0, 1) => n[2],
                (0, 2) => -n[1],
                (1, 0) => -n[2],
                (1, 2) => n[0],
                (2, 0) => n[1],
                (2, 1) => -n[0],
                _ => 0.0,
            };
            // column-major: m[column][row]
            *x = c * id + (1.0 - c) * n[i] * n[j] + sn * cross;
        }
    }
    m[3] = [30.0, 20.0, 10.0, 1.0];
    let moved = source.copy().unwrap();
    moved.transform(&m).unwrap();
    let twin_moved = twin.copy().unwrap();
    twin_moved.transform(&m).unwrap();
    assert_eq!(prism_equivalent(&moved, &twin_moved).unwrap(), Some(true));
    assert!(
        prism_section(&moved, &Plane::XY).unwrap().is_none(),
        "not along z once turned"
    );
}

#[test]
fn loop_holds_a_point_on_an_edge_line_off_the_loop() {
    let sq = Loop2::polygon(&[v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0), v(0.0, 10.0)]);
    assert_eq!(loop_holds(&sq, v(-1.0, 0.0)), Some(false));
    assert_eq!(loop_holds(&sq, v(11.0, 10.0)), Some(false));
    assert_eq!(loop_holds(&sq, v(5.0, 5.0)), Some(true));
    assert_eq!(loop_holds(&sq, v(5.0, 0.0)), None);
}

#[test]
fn directed_planar_sweep_preserves_record_coordinates() {
    let center = v(10.0, 20.0);
    assert_eq!(
        solidcore_ops::prism::counterclockwise_sweep(center, v(12.0, 20.0), v(10.0, 23.0)),
        Some(FRAC_PI_2)
    );
    assert_eq!(
        solidcore_ops::prism::counterclockwise_sweep(center, center, v(10.0, 23.0)),
        None
    );
}

#[test]
fn hatch_record_admission_bounds_conversion_before_geometry() {
    use solidcore_ops::hatch::generate;
    use solidcore_record::hatch::{Invalid, Pattern};
    let pattern = Pattern::lines_45();
    let line = Seg2::Line {
        a: v(0.0, 0.0),
        b: v(1.0, 0.0),
    };
    let mut region = Region2 {
        outer: Loop2 {
            segs: vec![line; solidcore::hatch::MAX_INPUT_PIECES + 1],
        },
        holes: Vec::new(),
    };
    assert_eq!(
        generate(&pattern, &region),
        Err(Invalid("input piece budget exhausted"))
    );
    region.outer.segs.pop();
    assert_ne!(
        generate(&pattern, &region),
        Err(Invalid("input piece budget exhausted")),
        "the exact ceiling remains inclusive"
    );
    let circle = Loop2::circle(v(0.0, 0.0), 5.0);
    region = Region2 {
        outer: circle.clone(),
        holes: vec![circle.clone(), circle],
    };
    assert_eq!(
        generate(&pattern, &region),
        Err(Invalid("not supported yet: multiple circular hatch holes"))
    );
    region = Region2 {
        outer: Loop2 {
            segs: vec![line; 3],
        },
        holes: vec![Loop2::default(); solidcore::hatch::MAX_INPUT_PIECES / 3 + 1],
    };
    assert_eq!(
        generate(&pattern, &region),
        Err(Invalid("too many region holes"))
    );
}

/// The section fit as Convert Mesh takes it (moved from SolidCraft's `mesh_convert.rs`
/// `planar_boxes_are_exact_and_coarse_polygons_are_not_rounded`): a hexagon keeps its six sides,
/// sixteen nodes too coarse for 0.001 keep sixteen, and within 0.2 they are the exact circle, one
/// arc of one full turn round the centre at the radius.
#[test]
fn coarse_section_polygons_are_not_rounded_and_a_fine_one_is_the_exact_circle() {
    let ring = |n: usize| -> Vec<Vec2> {
        (0..n)
            .map(|i| {
                let angle = i as f64 * TAU / n as f64;
                v(angle.cos() * 10., angle.sin() * 10.)
            })
            .collect()
    };
    assert_eq!(fitted_loop(&ring(6), 1., 1e-6).unwrap().segs.len(), 6);
    assert_eq!(fitted_loop(&ring(16), 0.001, 1e-6).unwrap().segs.len(), 16);
    let fitted = fitted_loop(&ring(16), 0.2, 1e-6).unwrap();
    let [Seg2::Arc {
        center,
        radius,
        sweep,
        ..
    }] = fitted.segs.as_slice()
    else {
        panic!("not one arc: {fitted:?}")
    };
    assert!(
        center.x.hypot(center.y) < 1e-9 && (radius - 10.).abs() < 1e-9,
        "centre {center:?}, radius {radius}"
    );
    assert!((sweep.abs() - TAU).abs() < 1e-9, "sweep {sweep}");
}

/// The point of the circle about `c` of radius `r` at angle `deg` (degrees).
fn on_circle(c: Vec2, r: f64, deg: f64) -> Vec2 {
    let a = deg.to_radians();
    v(c.x + r * a.cos(), c.y + r * a.sin())
}

/// A circle about `c` of radius `r` as two arcs, counter-clockwise, meeting at the angles `a`
/// and `b` (degrees, `a < b < a + 360`), the first starting at `start` (radians: `a` or `a`
/// plus whole turns).
fn circle_as_two_arcs(c: Vec2, r: f64, a: f64, b: f64, start: f64) -> Loop2 {
    let sweep = (b - a).to_radians();
    Loop2 {
        segs: vec![
            Seg2::Arc {
                center: c,
                radius: r,
                start,
                sweep,
            },
            Seg2::Arc {
                center: c,
                radius: r,
                start: start + sweep,
                sweep: TAU - sweep,
            },
        ],
    }
}

/// `l` with its segments' start points each within `1e-9` of one of `want`, every point of
/// `want` reached, and its arcs on the circle about `c` of radius `r` (within `1e-9`).
fn starts_at(l: &Loop2, want: &[Vec2], c: Vec2, r: f64) {
    assert_eq!(l.segs.len(), want.len(), "{l:?}");
    for p in want {
        assert!(
            l.segs.iter().any(|s| dist(start(s), *p) <= 1e-9),
            "no segment starts at {p:?}: {l:?}"
        );
    }
    for s in &l.segs {
        if let Seg2::Arc { center, radius, .. } = *s {
            assert!(
                dist(center, c) <= 1e-9 && (radius - r).abs() <= 1e-9,
                "{s:?}"
            );
        }
    }
    assert!(closed(l, 1e-9), "{l:?}");
}

/// A circle cut from a plate as two arcs of one circle keeps both points where they meet,
/// wherever they lie: each arc goes into the boolean as a curve of its own, so the vertex
/// between them is not taken for one inside a single edge (#15475 had both arcs on one circle
/// curve, and the boolean merged them).
#[test]
fn a_hole_drawn_as_two_arcs_keeps_both_points_where_they_meet() {
    let plate = rectangle(v(0.0, 0.0), v(100.0, 60.0));
    let c = v(40.0, 30.0);
    for (a, b) in [
        (30.0, 120.0),
        (-100.0, 10.0),
        (170.0, 260.0),
        (38.287, 218.287),
    ] {
        let hole = Region2 {
            outer: circle_as_two_arcs(c, 5.0, a, b, f64::to_radians(a)),
            holes: vec![],
        };
        let out = planar_regions_boolean(
            std::slice::from_ref(&plate),
            std::slice::from_ref(&hole),
            BooleanOp::Subtract,
            1e-7,
        )
        .unwrap();
        assert_eq!(out.len(), 1, "{a} {b}: {out:?}");
        let r = &out[0];
        // 4 lines and the 2 arcs: 6 edges
        assert_eq!(
            r.outer.segs.len() + r.holes.iter().map(|h| h.segs.len()).sum::<usize>(),
            6
        );
        assert_eq!(r.holes.len(), 1, "{a} {b}: {r:?}");
        starts_at(
            &r.holes[0],
            &[on_circle(c, 5.0, a), on_circle(c, 5.0, b)],
            c,
            5.0,
        );
        assert!(loop_area(&r.holes[0]) < 0.0, "a hole runs clockwise");
    }
}

/// The strip and holes the sheet's Recognize passes: four round holes of 10, 4, 2 and 1 mm,
/// each two clockwise half circles starting a whole turn past their angle (6.896 rad), the
/// strip intersected with itself. The 0.5 mm hole's arcs ran past 360° on #15475's circle and
/// the strip failed (`topology::brep::p7: loop does not close`); each half is now a circle of
/// its own trimmed from 0. The result is the strip as it was: the four holes meet where their
/// halves did, and the solid it extrudes to is valid with its closed-form volume.
#[test]
fn a_strip_with_small_holes_starting_past_a_whole_turn_intersects_to_itself() {
    let holes = [
        (v(198.0, 50.0), 5.000000000000001, 6.896185307179585),
        (
            v(395.9999999999999, 49.99999999999987),
            2.000000000000154,
            6.896185307179607,
        ),
        (v(594.0, 50.0), 1.0000000000000582, 6.896185307179572),
        (
            v(792.0000000000001, 50.00000000000005),
            0.49999999999981515,
            6.896185307179681,
        ),
    ];
    let loops = || -> Vec<Loop2> {
        holes
            .iter()
            .map(|&(c, r, s)| Loop2 {
                segs: vec![
                    Seg2::Arc {
                        center: c,
                        radius: r,
                        start: s,
                        sweep: -PI,
                    },
                    Seg2::Arc {
                        center: c,
                        radius: r,
                        start: s - PI,
                        sweep: -PI,
                    },
                ],
            })
            .collect()
    };
    let corners = [v(0.0, 0.0), v(990.0, 0.0), v(990.0, 100.0), v(0.0, 100.0)];
    let a = Region2 {
        outer: Loop2::polygon(&corners),
        holes: loops(),
    };
    let b = Region2 {
        outer: Loop2::polygon(&[corners[3], corners[0], corners[1], corners[2]]),
        holes: loops(),
    };
    for tol in [9.950381902218627e-4, 1e-7] {
        let out = planar_regions_boolean(
            std::slice::from_ref(&a),
            std::slice::from_ref(&b),
            BooleanOp::Intersect,
            tol,
        )
        .unwrap();
        assert_eq!(out.len(), 1, "{tol}: {out:?}");
        assert_eq!(out[0].holes.len(), 4, "{tol}: {out:?}");
        for &(c, r, s) in &holes {
            let deg = s.to_degrees();
            let h = out[0]
                .holes
                .iter()
                .find(|h| {
                    h.segs
                        .iter()
                        .all(|x| matches!(*x, Seg2::Arc { center, .. } if dist(center, c) <= 1e-9))
                })
                .unwrap_or_else(|| panic!("{tol}: no hole about {c:?}: {out:?}"));
            starts_at(
                h,
                &[on_circle(c, r, deg), on_circle(c, r, deg - 180.0)],
                c,
                r,
            );
        }
        let body = extrude_planar_regions(&Plane::XY, &out, 0.0, 1.0).unwrap();
        let report = body.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "{tol}: {report}");
        let want = 990.0 * 100.0 - PI * holes.iter().map(|h| h.1 * h.1).sum::<f64>();
        // the volume asked to a relative accuracy of 1e-8
        assert!(
            (volume(&body) - want).abs() <= 1e-8 * want,
            "{tol}: {} vs {want}",
            volume(&body)
        );
    }
}

/// A round notch over a plate's edge, its circle drawn as two arcs meeting twice inside the
/// plate (at -120° and -60°): the notch's outline is the circle's lower half from where it
/// crosses the edge (0° and 180°), through both meeting points, as three arcs.
#[test]
fn a_round_notch_drawn_as_two_arcs_keeps_their_meeting_points_on_its_outline() {
    let plate = rectangle(v(0.0, 0.0), v(100.0, 60.0));
    let c = v(50.0, 60.0);
    let notch = Region2 {
        outer: circle_as_two_arcs(c, 8.0, -120.0, -60.0, f64::to_radians(-120.0)),
        holes: vec![],
    };
    let out = planar_regions_boolean(
        std::slice::from_ref(&plate),
        std::slice::from_ref(&notch),
        BooleanOp::Subtract,
        1e-7,
    )
    .unwrap();
    assert_eq!(out.len(), 1, "{out:?}");
    assert!(out[0].holes.is_empty(), "{out:?}");
    let outer = &out[0].outer;
    let arcs: Vec<&Seg2> = outer
        .segs
        .iter()
        .filter(|s| matches!(s, Seg2::Arc { .. }))
        .collect();
    assert_eq!(arcs.len(), 3, "{outer:?}");
    // 5 lines (the plate's bottom and sides, its top on either side of the notch) and 3 arcs
    assert_eq!(outer.segs.len(), 8, "{outer:?}");
    for deg in [0.0, -60.0, -120.0] {
        let p = on_circle(c, 8.0, deg);
        let s = arcs
            .iter()
            .find(|s| dist(start(s), p) <= 1e-9)
            .unwrap_or_else(|| panic!("no arc starts at {deg}°: {outer:?}"));
        let Seg2::Arc {
            center,
            radius,
            sweep,
            ..
        } = **s
        else {
            panic!("{s:?}")
        };
        assert!(
            dist(center, c) <= 1e-9 && (radius - 8.0).abs() <= 1e-9,
            "{s:?}"
        );
        assert!((sweep + FRAC_PI_3).abs() <= 1e-9, "{deg}°: {s:?}");
    }
    assert!(closed(outer, 1e-9), "{outer:?}");
    let want = 6000.0 - PI * 64.0 / 2.0;
    assert!(
        (area(&out[0]) - want).abs() <= 1e-9 * want,
        "{}",
        area(&out[0])
    );
}

/// A face strays across a direction by its tilt over its own size (the diagonal of its tight
/// box), in closed form: on a Y-groove plate swept along z the walls not at all, each cap by
/// twice its diagonal, the 30° bevel across x by √3 times its own diagonal, the top not at all;
/// a cylinder's side along its axis not at all and across it by four times its box diagonal; a
/// wide thin disc's caps by their whole diameter (a cap's only vertex is its seam's, brett19 on
/// #21614); a ruled spline net along its rulings not at all, short or long, and across a tilt by
/// twice its height times the tilt's sine; any finite scale of a direction as the unit one; zero
/// no bound; NaN and a face past the faces refused.
#[test]
fn a_face_strays_across_a_direction_by_its_tilt_over_its_own_size() {
    let zone = solidcore_ops::sweep::zone_tolerance().unwrap();
    let close = |got: f64, want: f64, what: &str| {
        assert!((got - want).abs() <= 1e-6 * want, "{what}: {got} != {want}");
    };
    let t = 30.0_f64.to_radians().tan();
    let top = 0.5 + 7.0 * t;
    let corners = [
        v(-40.0, 0.0),
        v(-40.0, -8.0),
        v(-0.5, -8.0),
        v(-0.5, -7.0),
        v(-top, 0.0),
    ];
    let region = Region2 {
        outer: Loop2::polygon(&corners),
        holes: vec![],
    };
    let plate = extrude_planar_regions(&Plane::XY, &[region], 0.0, 120.0).unwrap();
    let (_, order) = plate.to_record().unwrap();
    let normal = |f: usize| plate.face_plane_normal(&order, f).unwrap().unwrap().value;
    let stray = |f: usize, d: [f64; 3]| plate.face_straying(&order, f, d).unwrap().unwrap();
    let cap = (39.5_f64.powi(2) + 8.0_f64.powi(2)).sqrt();
    let bevel = ((7.0 * t).powi(2) + 7.0_f64.powi(2) + 120.0_f64.powi(2)).sqrt();
    let (mut caps, mut bevels, mut tops) = (0, 0, 0);
    for f in 0..order.faces.len() {
        let n = normal(f);
        let along = stray(f, [0.0, 0.0, 1.0]);
        if n[2].abs() > 0.99 {
            caps += 1;
            close(along, 2.0 * cap, "cap");
            continue;
        }
        assert!(along <= zone, "face {f}: {along}");
        let across = stray(f, [1.0, 0.0, 0.0]);
        if n[1] > 0.99 {
            tops += 1;
            assert!(across <= zone, "top: {across}");
        } else if n[0] > 0.1 && n[1] > 0.1 {
            bevels += 1;
            close(across, 3.0_f64.sqrt() * bevel, "bevel");
        }
    }
    assert_eq!((caps, bevels, tops), (2, 1, 1));
    // any finite scale of a direction gives the unit direction's bound
    let f = (0..order.faces.len())
        .find(|&f| normal(f)[0] > 0.1 && normal(f)[1] > 0.1)
        .unwrap();
    let unit = stray(f, [1.0, 0.0, 0.0]);
    for scale in [7.0, 1e200, 1e-200] {
        let scaled = stray(f, [scale, 0.0, 0.0]);
        assert!(
            (scaled - unit).abs() <= 1e-12 * unit,
            "{scale}: {scaled} != {unit}"
        );
    }
    assert_eq!(plate.face_straying(&order, f, [0.0; 3]).unwrap(), None);
    assert!(matches!(
        plate.face_straying(&order, f, [f64::NAN, 0.0, 1.0]),
        Err(Error::Argument)
    ));
    assert!(matches!(
        plate.face_straying(&order, order.faces.len(), [0.0, 0.0, 1.0]),
        Err(Error::Argument)
    ));

    // a cylinder of radius 5 and height 20: its side's box diagonal is √600
    let cylinder = make_cylinder([0.0; 3], 5.0, 20.0).unwrap();
    let (_, order) = cylinder.to_record().unwrap();
    let side = (0..order.faces.len())
        .find(|&f| cylinder.face_plane_normal(&order, f).unwrap().is_none())
        .unwrap();
    let along = cylinder
        .face_straying(&order, side, [0.0, 0.0, 1.0])
        .unwrap()
        .unwrap();
    assert!(along <= zone, "{along}");
    let across = cylinder
        .face_straying(&order, side, [1.0, 0.0, 0.0])
        .unwrap()
        .unwrap();
    close(across, 4.0 * 600.0_f64.sqrt(), "side across x");

    // a disc of radius 1000 and height 1, d 4e-6 off its plane: a line along d across a cap
    // leaves it by 8e-3, so each cap is far from straight (its vertex box would have said 1)
    let disc = make_cylinder([0.0; 3], 1000.0, 1.0).unwrap();
    let (_, order) = disc.to_record().unwrap();
    let d = [1.0, 0.0, 4e-6];
    let mut flat = 0;
    for f in 0..order.faces.len() {
        if disc.face_plane_normal(&order, f).unwrap().is_some() {
            flat += 1;
            let got = disc.face_straying(&order, f, d).unwrap().unwrap();
            close(got, 2.0 * 8.0_f64.sqrt() * 1000.0 * 4e-6, "disc cap");
        }
    }
    assert_eq!(flat, 2);

    // a plain ruled spline net (brett19 on #21614): a degree-1 loft between a cubic at z = 0 and
    // the same cubic at z = h runs straight along its rulings, however short they are against
    // its width, and strays across a direction tilted θ off them by 2·h·sin θ
    let ruled = |width: f64, h: f64| {
        let cubic = |z: f64| solidcore_ops::sweep::Curve::Nurbs {
            degree: 3,
            points: vec![
                [0.0, 0.0, z, 1.0],
                [0.3 * width, 0.2 * width, z, 1.0],
                [0.7 * width, -0.2 * width, z, 1.0],
                [width, 0.0, z, 1.0],
            ],
            knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        };
        let sheet =
            solidcore_ops::sweep::loft(&[vec![cubic(0.0)], vec![cubic(h)]], 1, false).unwrap();
        let (_, order) = sheet.to_record().unwrap();
        assert_eq!(order.faces.len(), 1);
        (sheet, order)
    };
    for (width, h) in [(10.0, 10.0), (10.0, 1.0), (1000.0, 1.0)] {
        let (sheet, order) = ruled(width, h);
        let along = sheet
            .face_straying(&order, 0, [0.0, 0.0, 1.0])
            .unwrap()
            .unwrap();
        assert!(along <= zone, "ruled {width} x {h}: {along}");
    }
    let (sheet, order) = ruled(10.0, 10.0);
    let tilted = sheet
        .face_straying(&order, 0, [1e-3, 0.0, 1.0])
        .unwrap()
        .unwrap();
    let sin = 1e-3 / (1.0 + 1e-6_f64).sqrt();
    close(tilted, 2.0 * 10.0 * sin, "ruled, tilted");
}

/// The record faces of `body` whose outward plane normal is `n` (within 1e-9 per component).
fn faces_facing(body: &Body, order: &solidcore_ops::Order, n: [f64; 3]) -> Vec<usize> {
    (0..order.faces.len())
        .filter(|&f| {
            body.face_plane_normal(order, f)
                .unwrap()
                .is_some_and(|m| m.value.iter().zip(n).all(|(a, b)| (a - b).abs() < 1e-9))
        })
        .collect()
}

/// The edges faces `a` and `b` of `body` share.
fn shared_edges(body: &Body, order: &solidcore_ops::Order, a: usize, b: usize) -> Vec<usize> {
    let ea = body.rim_edges(order, &[a]).unwrap();
    let eb = body.rim_edges(order, &[b]).unwrap();
    ea.into_iter().filter(|e| eb.contains(e)).collect()
}

/// An L-shaped plate seen from above (brett19's probe on #22033): its top face runs from x = -40
/// to -1 for z in 0..120 and on to x = 200 for z in 120..240, so the face's centroid (x about 66)
/// lies past its edge at x = -1. The direction into the top face from that edge is -x all the
/// same, and into the wall below it is down; an edge that does not bound the face is refused.
#[test]
fn the_direction_into_a_face_comes_from_its_loop_not_its_centroid() {
    all_fixes();
    let above = Plane {
        origin: Vec3::new(0.0, 0.0, 0.0),
        x: Vec3::new(1.0, 0.0, 0.0),
        y: Vec3::new(0.0, 0.0, 1.0),
    };
    let outline = [
        v(-40.0, 0.0),
        v(-1.0, 0.0),
        v(-1.0, 120.0),
        v(200.0, 120.0),
        v(200.0, 240.0),
        v(-40.0, 240.0),
    ];
    let region = Region2 {
        outer: Loop2::polygon(&outline),
        holes: vec![],
    };
    let plate = extrude_planar_regions(&above, &[region], 0.0, 8.0).unwrap();
    let (_, order) = plate.to_record().unwrap();
    let top = faces_facing(&plate, &order, [0.0, 1.0, 0.0]);
    assert_eq!(top.len(), 1, "{top:?}");
    let top = top[0];
    let centroid = plate.face_centroid(&order, top, 1e-8).unwrap().centroid;
    assert!(centroid[0] > 60.0, "{centroid:?}");
    let wall = faces_facing(&plate, &order, [1.0, 0.0, 0.0])
        .into_iter()
        .find(|&f| {
            shared_edges(&plate, &order, f, top)
                .iter()
                .any(|&e| (plate.edge_middle(&order, e).unwrap()[0] + 1.0).abs() < 1e-9)
        })
        .unwrap();
    let toe = shared_edges(&plate, &order, wall, top);
    assert_eq!(toe.len(), 1, "{toe:?}");
    let near = |got: [f64; 3], want: [f64; 3]| {
        assert!(
            got.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-9),
            "{got:?} != {want:?}"
        );
    };
    near(
        plate.edge_inward(&order, toe[0], top).unwrap(),
        [-1.0, 0.0, 0.0],
    );
    near(
        plate.edge_inward(&order, toe[0], wall).unwrap(),
        [0.0, -1.0, 0.0],
    );
    let bottom = faces_facing(&plate, &order, [0.0, -1.0, 0.0])[0];
    assert!(matches!(
        plate.edge_inward(&order, toe[0], bottom),
        Err(Error::Argument)
    ));
}

/// The direction faces run straight along: a plane pair gives it (on one body or two, in any
/// order); a cylinder gives its axis where the planes are parallel or there are none; a face
/// across the only direction is not straight; parallel planes alone give none.
#[test]
fn faces_run_straight_along_a_plane_pair_or_else_a_cylinder_axis() {
    use solidcore_ops::measure::{straight_direction, Straight, StraightBy};
    all_fixes();
    let a = make_box([0.0, 0.0, 0.0], [10.0, 20.0, 30.0]).unwrap();
    let (_, ao) = a.to_record().unwrap();
    let b = make_box([20.0, 0.0, 0.0], [10.0, 20.0, 30.0]).unwrap();
    let (_, bo) = b.to_record().unwrap();
    let top = faces_facing(&a, &ao, [0.0, 0.0, 1.0])[0];
    let side = faces_facing(&b, &bo, [-1.0, 0.0, 0.0])[0];
    // +z and -x: their cross product is along y
    let along = |got: Straight, want: [f64; 3]| match got {
        Straight::Along { direction, .. } => assert!(
            direction
                .iter()
                .zip(want)
                .all(|(p, q)| (p - q).abs() < 1e-12),
            "{direction:?}"
        ),
        other => panic!("not straight along {want:?}: {other:?}"),
    };
    let one = straight_direction(&[(&a, &ao, top), (&b, &bo, side)]).unwrap();
    along(one, [0.0, 1.0, 0.0]);
    assert!(matches!(
        one,
        Straight::Along {
            by: StraightBy::Planes(..),
            ..
        }
    ));
    assert_eq!(
        straight_direction(&[(&b, &bo, side), (&a, &ao, top)]).unwrap(),
        match one {
            Straight::Along { direction, .. } => Straight::Along {
                direction,
                by: StraightBy::Planes(1, 0),
            },
            other => other,
        }
    );
    // two parallel faces give no direction
    let bottom = faces_facing(&a, &ao, [0.0, 0.0, -1.0])[0];
    assert_eq!(
        straight_direction(&[(&a, &ao, top), (&a, &ao, bottom)]).unwrap(),
        Straight::Undetermined
    );
    // a cylinder's side: along its axis; with its cap, the cap is across it
    let c = make_cylinder([0.0, 0.0, 0.0], 5.0, 20.0).unwrap();
    let (_, co) = c.to_record().unwrap();
    let round = (0..co.faces.len())
        .find(|&f| c.face_plane_normal(&co, f).unwrap().is_none())
        .unwrap();
    let cap = faces_facing(&c, &co, [0.0, 0.0, 1.0])[0];
    let got = straight_direction(&[(&c, &co, round)]).unwrap();
    along(got, [0.0, 0.0, 1.0]);
    assert!(matches!(
        got,
        Straight::Along {
            by: StraightBy::Axis(0),
            ..
        }
    ));
    assert_eq!(
        straight_direction(&[(&c, &co, round), (&c, &co, cap)]).unwrap(),
        Straight::NotStraight { face: 1 }
    );
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn unit3(a: [f64; 3]) -> [f64; 3] {
    let n = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    a.map(|c| c / n)
}

fn vec3(a: [f64; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}

/// A 10 by 10 plate 1 thick whose top faces `n`, its sides along `x` (square to `n`) and `n × x`,
/// and the record index of its top face.
fn plate_facing(n: [f64; 3], x: [f64; 3]) -> (Body, solidcore_ops::Order, usize) {
    let (n, x) = (unit3(n), unit3(x));
    let plane = Plane {
        origin: Vec3::new(0.0, 0.0, 0.0),
        x: vec3(x),
        y: vec3(cross(n, x)),
    };
    let body =
        extrude_planar_regions(&plane, &[rectangle(v(0.0, 0.0), v(10.0, 10.0))], 0.0, 1.0).unwrap();
    let (_, order) = body.to_record().unwrap();
    let top = faces_facing(&body, &order, n);
    assert_eq!(top.len(), 1, "{top:?}");
    (body, order, top[0])
}

/// brett19's probe on #22066: three 10 mm faces off the world axes, each tilted off square to y by
/// 0.9 of what the zone allows. y proves all three straight; the cross product of the two furthest
/// from parallel does not prove the third, so the answer is a direction inside every face's band.
#[test]
fn faces_tilted_within_the_zone_run_along_a_direction_inside_every_band() {
    use solidcore_ops::measure::{straight_direction, Straight, StraightBy};
    all_fixes();
    let zone = solidcore_ops::sweep::zone_tolerance().unwrap();
    let alpha = 0.9 * zone / (2.0 * 10.0 * 2.0_f64.sqrt());
    let a = plate_facing([0.0, alpha, 1.0], [1.0, 0.0, 0.0]);
    let b = plate_facing([1.0, alpha, 0.0], [0.0, 0.0, 1.0]);
    let c = plate_facing([1.0, -alpha, 1.0], [1.0, 0.0, -1.0]);
    let faces = [(&a.0, &a.1, a.2), (&b.0, &b.1, b.2), (&c.0, &c.1, c.2)];
    for (body, order, face) in faces {
        let e = body
            .face_straying(order, face, [0.0, 1.0, 0.0])
            .unwrap()
            .unwrap();
        assert!(e <= zone, "{e}");
    }
    let got = straight_direction(&faces).unwrap();
    let Straight::Along { direction, by } = got else {
        panic!("{got:?}");
    };
    assert_eq!(by, StraightBy::Bands, "{got:?}");
    assert!((direction[1] - 1.0).abs() < 1e-12, "{direction:?}");
    for (body, order, face) in faces {
        let e = body.face_straying(order, face, direction).unwrap().unwrap();
        assert!(e <= zone, "{e}");
    }
}

/// Flipped senses on #22066: the faces +x, +y and -x of a box and the same with the first and last
/// swapped name the same pair as well as the same direction.
#[test]
fn flipped_senses_name_the_same_faces() {
    use solidcore_ops::measure::{straight_direction, Straight, StraightBy};
    all_fixes();
    let a = make_box([0.0, 0.0, 0.0], [10.0, 20.0, 30.0]).unwrap();
    let (_, ao) = a.to_record().unwrap();
    let face = |n: [f64; 3]| faces_facing(&a, &ao, n)[0];
    let (px, py, nx) = (
        face([1.0, 0.0, 0.0]),
        face([0.0, 1.0, 0.0]),
        face([-1.0, 0.0, 0.0]),
    );
    let one = straight_direction(&[(&a, &ao, px), (&a, &ao, py), (&a, &ao, nx)]).unwrap();
    let two = straight_direction(&[(&a, &ao, nx), (&a, &ao, py), (&a, &ao, px)]).unwrap();
    assert_eq!(one, two);
    assert_eq!(
        one,
        Straight::Along {
            direction: [0.0, 0.0, 1.0],
            by: StraightBy::Planes(1, 0)
        }
    );
}

/// A plate off the world axes with a square hole: from the edge where its top meets a wall of the
/// hole (a face whose use runs the other way round), the way into the top is away from the hole
/// and into the wall is down. A cylinder's seam, with its face on both sides, is refused.
#[test]
fn the_direction_into_a_face_off_the_axes_and_at_a_hole_and_a_seam_refused() {
    all_fixes();
    let (ex, ey) = ([2.0 / 3.0, 1.0 / 3.0, 2.0 / 3.0], unit3([-1.0, 2.0, 0.0]));
    let n = cross(ex, ey);
    let plane = Plane {
        origin: Vec3::new(3.0, -2.0, 7.0),
        x: vec3(ex),
        y: vec3(ey),
    };
    let region = Region2 {
        outer: Loop2::polygon(&[
            v(-20.0, -20.0),
            v(20.0, -20.0),
            v(20.0, 20.0),
            v(-20.0, 20.0),
        ]),
        holes: vec![Loop2::polygon(&[
            v(-5.0, -5.0),
            v(-5.0, 5.0),
            v(5.0, 5.0),
            v(5.0, -5.0),
        ])],
    };
    let plate = extrude_planar_regions(&plane, &[region], 0.0, 8.0).unwrap();
    let (_, order) = plate.to_record().unwrap();
    let top = faces_facing(&plate, &order, n);
    assert_eq!(top.len(), 1, "{top:?}");
    let top = top[0];
    // the toe of the hole's wall at local x = 5, which faces -ex into the hole
    let at = [0, 1, 2].map(|i| [3.0, -2.0, 7.0][i] + 5.0 * ex[i] + 8.0 * n[i]);
    let (wall, edge) = faces_facing(&plate, &order, ex.map(|c| -c))
        .into_iter()
        .find_map(|f| {
            shared_edges(&plate, &order, f, top)
                .into_iter()
                .find(|&e| {
                    let m = plate.edge_middle(&order, e).unwrap();
                    (0..3).all(|i| (m[i] - at[i]).abs() < 1e-9)
                })
                .map(|e| (f, e))
        })
        .unwrap();
    let near = |got: [f64; 3], want: [f64; 3]| {
        assert!(
            got.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-9),
            "{got:?} != {want:?}"
        );
    };
    near(plate.edge_inward(&order, edge, top).unwrap(), ex);
    near(
        plate.edge_inward(&order, edge, wall).unwrap(),
        n.map(|c| -c),
    );
    // a cylinder's seam: its round face lies on both sides
    let c = make_cylinder([0.0, 0.0, 0.0], 5.0, 20.0).unwrap();
    let (_, co) = c.to_record().unwrap();
    let round = (0..co.faces.len())
        .find(|&f| c.face_plane_normal(&co, f).unwrap().is_none())
        .unwrap();
    let seam = (0..co.edges.len())
        .find(|&e| {
            let m = c.edge_middle(&co, e).unwrap();
            (m[0] - 5.0).abs() < 1e-9 && m[1].abs() < 1e-9 && (m[2] - 10.0).abs() < 1e-9
        })
        .unwrap();
    assert!(c.edge_inward(&co, seam, round).is_err());
}
