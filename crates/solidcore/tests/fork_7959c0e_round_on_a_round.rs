//! The SolidCraft fork's `round_on_a_definition_is_its_definition` (capi
//! `ops_kernel.cpp`, tangency-labs/smlib@7959c0e and c5b6bb7, smlib#101, merged at 852dd8c;
//! tangency-labs/solidcore#2298): a round on a round by its definition.
//!
//! The twisted wedge (the block 60 x 30 x 50 below the fork's `twisted_top`, a cubic
//! spline whose slope across y turns from -tan 30 to 0 to tan 30 along x) has its front
//! edge rounded r 3. The top is a cubic spline, so that round is its definition. The block
//! is then cut by the plane x + 0.6 y + 0.6 z = 55, which crosses that round at about
//! x 38 to 42, in the twist, and the cut's edges with the front, the round and the top (a
//! tangent chain) are rounded r 1. The round on the round rolls on a definition: it is its
//! own definition two deep, its sections the ball's circles to rounding (graded as the
//! fork's `circle_definitions` grades them), its net within the certified bound, the inner
//! round's derivatives enclosed over the boxes its certificate takes. Its rail on the inner
//! round runs across that round's stations, where the round is only as smooth as the faces
//! under it (C1 across: its curvature jumps), and a station of the outer round stands at
//! each crossing, as at a face's knot line.
//!
//! How this test drives the kernel differs from the capi: `smlib_split_body`'s back part
//! is a slice by a one-face sheet on the tool surface turned over (`SwapUV`), as the capi
//! makes it; an edge is picked as the nearest within 0.1 of a point; the rounds are
//! [`fillet_edges`] (circular, constant radius, then `StitchAndOrient`), on the kernel
//! SolidCraft runs (every fix on but `geometric_g2_continuity`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_precision_loss,
    clippy::neg_cmp_op_on_partial_ord
)] // test helpers

use solidcore::blend::fillet_executive::api::{create_face_from_surface, fillet_edges};
use solidcore::blend::fillet_section::definition_box::{
    definition_partials_over_box, gn_def_face_of,
};
use solidcore::blend::fillet_section::interval::Ivl;
use solidcore::blend::fillet_section::series::V3;
use solidcore::blend::fillet_section::station::{span, Nurbs};
use solidcore::boolean::api_brep::boolean_with_options;
use solidcore::boolean::merge::p1::MergeOptions;
use solidcore::geom::a_object::BooleanOperation;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::tol;
use solidcore::kernel::{KernelOptions, UpstreamFixes};
use solidcore::math::{Point2, Point3, Vec2, Vec3};
use solidcore::nurbs::b_spline_curve::KnotType;
use solidcore::nurbs::b_spline_surface::{
    BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData, SurfParamType,
};
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::surface::plane::Plane;
use solidcore::surface::procedural_blend::{BlendCounts, ProceduralBlend};
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::edge;
use solidcore::topology::face::p2 as face2;
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel};

/// `TWIST_TAN`: tan 30 degrees.
const TWIST_TAN: f64 = 0.577_350_269_189_625_76;

/// The kernel SolidCraft runs: every fix on but `geometric_g2_continuity`.
fn solidcraft_kernel() -> Kernel {
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

/// The fork's `twisted_top`: cubic in u over the knots -4 (4 times), 0, 4, .., 60, 64
/// (4 times), linear in v from y -1 to 31; the control points' x Greville's, their z
/// `25 + a y`, a = -tan 30 for the first seven columns, 0 for the next six, tan 30 after.
fn twisted_top() -> Surface {
    let mut ku = vec![-4.0; 4];
    let mut uk = vec![-4.0];
    let mut um = vec![4];
    for x in (0..=60).step_by(4) {
        ku.push(f64::from(x));
        uk.push(f64::from(x));
        um.push(1);
    }
    ku.extend_from_slice(&[64.0; 4]);
    uk.push(64.0);
    um.push(4);
    let nu = ku.len() - 4;
    let mut pts = Vec::new();
    for i in 0..nu {
        let x = (ku[i + 1] + ku[i + 2] + ku[i + 3]) / 3.0;
        let a = if i < 7 {
            -TWIST_TAN
        } else if i < 13 {
            0.0
        } else {
            TWIST_TAN
        };
        for y in [-1.0, 31.0] {
            pts.push(Point3::new(x, y, 25.0 + a * y));
        }
    }
    Surface::from(
        BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: 3,
            v_degree: 1,
            control_points: &pts,
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &um,
            v_multiplicities: &[2, 2],
            u_knots: &uk,
            v_knots: &[0.0, 1.0],
            knot_type: KnotType::Unspecified,
            weights: None,
            uv_domain: None,
        })
        .unwrap(),
    )
}

/// `smlib_split_body`'s back part: `body` sliced (`SM_BO_SLICE`, every option off) by a
/// one-face sheet of the body's tolerance on `tool` turned over.
fn split_back(k: &mut Kernel, body: Brep, tool: &Surface) -> Brep {
    let mut s = tool.clone();
    s.swap_uv(k).unwrap();
    let mut sheet =
        solidcore::topology::brep::p3::new_brep(k, tol::USE_DEFAULT, tol::USE_DEFAULT).unwrap();
    let t = body.tolerance();
    solidcore::topology::brep::p2::set_tolerance(k, &mut sheet, t, false, true).unwrap();
    create_face_from_surface(k, &mut sheet, &s).unwrap();
    boolean_with_options(
        k,
        body,
        sheet,
        BooleanOperation::Slice,
        MergeOptions::new(false, false, false, false, false),
    )
    .unwrap()
    .result
}

/// The edge nearest `p`, within 0.1.
fn pick(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
    let q = Point3::new(p[0], p[1], p[2]);
    let mut best = (0.1f64, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance <= best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.unwrap_or_else(|| panic!("no edge near {p:?}"))
}

/// SMLib's validity check at level 2 finds nothing wrong with `b`.
fn sound(k: &mut Kernel, b: &mut Brep) -> bool {
    check_brep(k, b, CheckLevel::Level2)
        .unwrap()
        .lines()
        .is_empty()
}

/// Whether u drives s on a definition (its surface not turned over).
fn u_is_s(pb: &ProceduralBlend) -> bool {
    pb.definition_parameters(Point2::new(1.0, 0.0)).x
        != pb.definition_parameters(Point2::new(0.0, 0.0)).x
}

/// The net's knots along s.
fn knots_along_s(pb: &ProceduralBlend) -> Vec<f64> {
    let param = if u_is_s(pb) {
        SurfParamType::U
    } else {
        SurfParamType::V
    };
    pb.net().knots(param, None).unwrap().0
}

/// The fork's `circle_definitions`: a round of the ball's circle by its definition, graded
/// on its own terms: at every station of a grid each section is the circle of the ball's
/// radius `r` (its points equidistant, by r, from the centre of the circle through three of
/// them, to rounding) and the net within the certified bound of the definition. Only the
/// definitions nested `depth` deep (0: every one). Returns how many faces were graded,
/// the worst section's distance off its circle, the worst net point's distance over the
/// bound, and the nets' spans along s in all.
fn circle_definitions(k: &Kernel, b: &Brep, r: f64, depth: u32) -> (usize, f64, f64, usize) {
    let (mut graded, mut worst_circle, mut worst_net, mut spans) = (0, 0.0_f64, 0.0_f64, 0);
    for &f in b.face_list() {
        let Some(s) = face2::surface(b, f).unwrap() else {
            continue;
        };
        let SurfaceKind::ProceduralBlend(pb) = s.kind() else {
            continue;
        };
        if pb.order() != 1 || (depth != 0 && pb.depth() != depth) {
            continue;
        }
        graded += 1;
        spans += knots_along_s(pb).len() - 1;
        let u_is_s = u_is_s(pb);
        let dom = pb.net().natural_uv_domain();
        let (lo, hi) = (dom.min(), dom.max());
        for a in 0..=32 {
            // a section: s fixed, t across (u and v the other way round on a face turned
            // over)
            let mut p = Vec::new();
            for c in 0..=8 {
                let (fs, ft) = (f64::from(a) / 32.0, f64::from(c) / 8.0);
                let uv = Point2::new(
                    lo.x + (hi.x - lo.x) * if u_is_s { fs } else { ft },
                    lo.y + (hi.y - lo.y) * if u_is_s { ft } else { fs },
                );
                let x = pb.evaluate_point(k, uv).unwrap();
                let y = pb.net().evaluate_point(uv).unwrap();
                p.push(x);
                worst_net = worst_net.max(x.distance(y) / pb.bound());
            }
            // the circle through the ends and the middle: its centre in their plane
            let (e1, e2) = (p[4] - p[0], p[8] - p[0]);
            let nrm = e1.cross(e2);
            let d = 2.0 * nrm.dot(nrm);
            if !(d > 0.0) {
                worst_circle = f64::INFINITY;
                continue;
            }
            let kk = nrm.cross(e1) * e2.dot(e2) + e2.cross(nrm) * e1.dot(e1);
            let centre = p[0] + kk * (1.0 / d);
            for x in &p {
                worst_circle = worst_circle.max((x.distance(centre) - r).abs());
            }
        }
    }
    (graded, worst_circle, worst_net, spans)
}

/// The fork's crossing count: for each round two deep and each support of it that is a
/// definition, the inner round's stations along s (its net's knots) that its rail on that
/// round crosses, and of those the ones without a station of its own (the rail's s on the
/// inner round at each of the outer round's stations, through its seed).
fn station_crossings(b: &Brep) -> (usize, usize) {
    let (mut crossings, mut missed) = (0, 0);
    for &f in b.face_list() {
        let Some(s) = face2::surface(b, f).unwrap() else {
            continue;
        };
        let SurfaceKind::ProceduralBlend(pb) = s.kind() else {
            continue;
        };
        if pb.depth() != 2 {
            continue;
        }
        for f in 0..2 {
            let Some(SurfaceKind::ProceduralBlend(inner)) = pb.support(f).map(Surface::kind) else {
                continue;
            };
            let inner_u_is_s = u_is_s(inner);
            let outer_u_is_s = u_is_s(pb);
            let ik = knots_along_s(inner);
            let ok = knots_along_s(pb);
            let seed = pb.seed(f).unwrap();
            let at: Vec<f64> = ok
                .iter()
                .map(|&kj| {
                    let uv = if outer_u_is_s {
                        Point2::new(kj, 0.0)
                    } else {
                        Point2::new(0.0, kj)
                    };
                    let s = pb.definition_parameters(uv).x;
                    let w = seed.evaluate_point(s).unwrap();
                    if inner_u_is_s {
                        w.x
                    } else {
                        w.y
                    }
                })
                .collect();
            let lo = at.iter().copied().fold(f64::INFINITY, f64::min);
            let hi = at.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            for (j, &kj) in ik.iter().enumerate() {
                if !(kj > lo + 1e-9 && kj < hi - 1e-9) || (j > 0 && kj == ik[j - 1]) {
                    continue;
                }
                crossings += 1;
                let found = at
                    .iter()
                    .any(|&a| (a - kj).abs() <= 1e-9 * (1.0 + kj.abs()));
                missed += usize::from(!found);
            }
        }
    }
    (crossings, missed)
}

/// What the case gives: the second round's grading, its counts and crossings.
#[derive(Debug)]
struct Outcome {
    graded: usize,
    circle: f64,
    net: f64,
    spans: usize,
    counts: BlendCounts,
    crossings: usize,
    missed: usize,
    sound: bool,
}

/// The twisted wedge with its front edge rounded r 3: that round is its definition.
fn rounded_wedge(k: &mut Kernel) -> Brep {
    let top = twisted_top();
    let block = create_box(k, Point3::new(0.0, 0.0, 0.0), 60.0, 30.0, 50.0).unwrap();
    let mut r = split_back(k, block, &top);
    let front = pick(k, &r, [30.0, 0.0, 25.0]);
    fillet_edges(k, &mut r, &[front], 3.0, 1, 1, 1.0).unwrap();
    r
}

/// The case, up to the second round (the fork's checks before it are asserted here).
fn round_on_a_round(k: &mut Kernel) -> Outcome {
    let r = rounded_wedge(k);
    // the cut: the plane's normal (2, 1.2, 1.2) (its axes' cross product), the part behind
    // it kept
    let plane = Surface::from(Plane::new(
        Point3::new(40.0, 0.0, 25.0),
        Vec3::new(0.0, 1.0, -1.0),
        Vec3::new(-1.2, 1.0, 1.0),
        Vec2::new(1.0, 1.0),
        &Extent2d::from_bounds(-100.0, -100.0, 100.0, 100.0),
    ));
    let mut kept = split_back(k, r, &plane);
    // the cut face's edges with the front, the round and the top: on the plane, off the
    // bottom and the back
    let nrm = Vec3::new(1.0, 0.6, 0.6);
    let mut picks = Vec::new();
    for &e in kept.edge_list() {
        let c = edge::curve(&kept, e).unwrap().unwrap().clone();
        let mid = c
            .evaluate_point(k, edge::interval(&kept, e).unwrap().evaluate(0.5))
            .unwrap();
        if (mid.dot(nrm) - 55.0).abs() < 1e-6 && mid.z > 1e-6 && mid.y < 30.0 - 1e-6 {
            picks.push([mid.x, mid.y, mid.z]);
        }
    }
    assert_eq!(picks.len(), 3, "{picks:?}");
    let edges: Vec<EdgeId> = picks.iter().map(|&p| pick(k, &kept, p)).collect();
    k.blend_counts.set(BlendCounts::default());
    fillet_edges(k, &mut kept, &edges, 1.0, 1, 1, 1.0).unwrap();
    let counts = k.blend_counts.get();
    let sound = sound(k, &mut kept);
    let (graded, circle, net, spans) = circle_definitions(k, &kept, 1.0, 2);
    let (crossings, missed) = station_crossings(&kept);
    Outcome {
        graded,
        circle,
        net,
        spans,
        counts,
        crossings,
        missed,
        sound,
    }
}

/// With WR-714 the round on the round is its own definition two deep, as the fork's: the
/// fork's checks at 852dd8c and its figures (45 spans, the rail crossing the inner round's
/// stations 11 times, each at a station of its own), and a count gate about 5% over the
/// counts here, as the fork gates its own.
///
/// The counts are not the fork's. Fork 852dd8c, built locally (macOS, clang, `-O3
/// -ffp-contract=off`), takes 135809 definition evaluations and 101687 station solves on
/// this case (its test's comment quotes 137720 and 102941 from its CI); here they are
/// 227503 and 146931. The gap is there before this change: without WR-714, solidcore takes
/// 180857 and 92825, where fork 7959c0e takes 88937 and 54775, all of it outside the round
/// on the round's `Build` (which evaluates no definition then). The change adds about the
/// same work on both sides: +46646 evaluations here, +46872 in the fork; the `Build` of the
/// round on the round took 35488 here and 36376 in the fork when this was ported. Main
/// after #2274 (step 7) raised both runs by the same amount, with or without WR-714: they
/// were 181076 and 122690 with it, 134718 and 71986 without, on #2274's earlier head.
#[test]
fn round_on_a_definition_is_its_definition() {
    let mut k = solidcraft_kernel();
    let o = round_on_a_round(&mut k);
    println!(
        "{} round(s) on a round by definition, {} spans: sections off the ball's circle by {:.3e}, the net at {:.3} of its bound; {} definition evaluations, {} station solves; the rail crosses the inner round's stations {} times, {} without a station of its own",
        o.graded,
        o.spans,
        o.circle,
        o.net,
        o.counts.evaluations,
        o.counts.solves,
        o.crossings,
        o.missed
    );
    assert!(o.sound, "{o:?}");
    assert!(
        o.graded == 1 && o.spans == 45 && o.circle <= 1e-11 && o.net <= 1.0,
        "{o:?}"
    );
    assert!(
        o.counts.evaluations <= 239_000 && o.counts.solves <= 154_000,
        "{o:?}"
    );
    assert!(o.crossings >= 4 && o.missed == 0, "{o:?}");
}

/// Without WR-714 the round on the round is built as before: not a definition (the
/// fork's known-failing mark at 7959c0e, "a round on a face that is a blend by its
/// definition is not built as its definition: the certificate does not enclose the inner
/// definition yet"), and the body still checks clean.
#[test]
fn without_wr714_the_round_on_the_round_is_built_as_before() {
    let mut k = solidcraft_kernel();
    k.fixes.fillet_round_on_round_as_definition = false;
    let o = round_on_a_round(&mut k);
    println!(
        "{} round(s) on a round by definition, {} spans: sections off the ball's circle by {:.3e}, the net at {:.3} of its bound; {} definition evaluations, {} station solves; the rail crosses the inner round's stations {} times, {} without a station of its own",
        o.graded,
        o.spans,
        o.circle,
        o.net,
        o.counts.evaluations,
        o.counts.solves,
        o.crossings,
        o.missed
    );
    assert!(o.sound, "{o:?}");
    assert_eq!(o.graded, 0, "{o:?}");
}

/// The net `net` with its distinct inner knot `kv` along s taken out (every copy of it),
/// one control row fewer per copy: the net a knot removal leaves. Only its knots along s
/// are read by the enclosure of its definition (they name the piece whose station seeds
/// the feet); its points are the rows kept.
fn without_knot(net: &Nurbs, along_u: bool, kv: f64) -> Nurbs {
    let mut n = net.clone();
    let knots = if along_u { &mut n.u } else { &mut n.v };
    let before = knots.len();
    knots.retain(|&x| x != kv);
    let gone = before - knots.len();
    assert!(gone > 0, "{kv} is not a knot");
    if along_u {
        n.nu -= gone;
        n.pw.truncate(4 * n.nu * n.nv);
    } else {
        let nv = n.nv - gone;
        let mut pw = Vec::with_capacity(4 * n.nu * nv);
        for i in 0..n.nu {
            pw.extend_from_slice(&net.pw[4 * i * net.nv..4 * (i * net.nv + nv)]);
        }
        n.nv = nv;
        n.pw = pw;
    }
    n
}

/// How far `p` is outside the enclosure `e` (0 inside), in the max norm.
fn outside(e: V3<Ivl>, p: Point3) -> f64 {
    let off = |i: Ivl, x: f64| (i.lo - x).max(x - i.hi).max(0.0);
    off(e.x, p.x).max(off(e.y, p.y)).max(off(e.z, p.z))
}

/// tangency-labs/solidcore#2570: the certificate of a round on the round encloses the
/// inner round's definition on the faces under it and its leader themselves, whatever its
/// net's stations. The inner round's net is taken with one knot along s removed (each in
/// turn), so a piece of it spans a station: where a foot crosses a knot line of the twisted
/// top, a C2 cubic, or the leader's knot. The enclosure of its definition at 21 points
/// across that piece must hold the definition there (its own evaluator, the station solved
/// at that s), and so must its enclosure over a box across the lost station. Before, the
/// enclosure took the faces' pieces at the station in the middle of the net's piece,
/// continued past the station, and every one was proved, up to 1.38e-3 off the definition.
/// Every point is proved but the round's two ends, where its feet lie on the edge of the
/// front face's knot range and Krawczyk's box reaches past it, which is refused.
#[test]
fn a_net_with_a_knot_removed_is_enclosed_on_its_definition() {
    let mut k = solidcraft_kernel();
    let r = rounded_wedge(&mut k);
    let (mut rounds, mut points, mut proved, mut worst) = (0, 0, 0, 0.0_f64);
    let (mut boxes, mut boxes_proved, mut ends_refused) = (0, 0, 0);
    for &f in r.face_list() {
        let Some(s) = face2::surface(&r, f).unwrap() else {
            continue;
        };
        let SurfaceKind::ProceduralBlend(pb) = s.kind() else {
            continue;
        };
        rounds += 1;
        let along_u = u_is_s(pb);
        let net = Nurbs::of(pb.net()).unwrap();
        let ks = knots_along_s(pb);
        let dom = pb.net().natural_uv_domain();
        let (lo, hi) = (dom.min(), dom.max());
        for j in 1..ks.len() - 1 {
            let coarse = without_knot(&net, along_u, ks[j]);
            let (a, b) = (ks[j - 1], ks[j + 1]);
            // a box across the station the coarse net lost, a quarter of the way to the
            // stations beside it along s and 2% of t across the middle line: its feet cross
            // the knot line or the leader's knot there, and the enclosure, cut at those,
            // must hold the definition at every point of the box
            let (sa, sb) = (ks[j] - 0.25 * (ks[j] - a), ks[j] + 0.25 * (b - ks[j]));
            let (c, w) = if along_u {
                (0.5 * (lo.y + hi.y), 0.01 * (hi.y - lo.y))
            } else {
                (0.5 * (lo.x + hi.x), 0.01 * (hi.x - lo.x))
            };
            let (ua, ub, va, vb) = if along_u {
                (sa, sb, c - w, c + w)
            } else {
                (c - w, c + w, sa, sb)
            };
            let mut d = gn_def_face_of(&k, s).unwrap();
            let ku = span(&coarse.u, coarse.p, coarse.nu, 0.5 * (ua + ub));
            let kv = span(&coarse.v, coarse.q, coarse.nv, 0.5 * (va + vb));
            boxes += 1;
            if let Ok(p) =
                definition_partials_over_box(&k, &mut d, &coarse, ku, kv, ua, ub, va, vb, 0)
            {
                boxes_proved += 1;
                for i in 0..=20 {
                    for q in [-1.0, 0.0, 1.0] {
                        let x = sa + (sb - sa) * f64::from(i) / 20.0;
                        let uv = if along_u {
                            Point2::new(x, c + q * w)
                        } else {
                            Point2::new(c + q * w, x)
                        };
                        let want = pb.evaluate_point(&k, uv).unwrap();
                        worst = worst.max(outside(p.at(0, 0), want));
                    }
                }
            }
            for i in 0..=20 {
                let x = a + (b - a) * f64::from(i) / 20.0;
                let uv = if along_u {
                    Point2::new(x, 0.5 * (lo.y + hi.y))
                } else {
                    Point2::new(0.5 * (lo.x + hi.x), x)
                };
                let mut d = gn_def_face_of(&k, s).unwrap();
                let ku = span(&coarse.u, coarse.p, coarse.nu, uv.x);
                let kv = span(&coarse.v, coarse.q, coarse.nv, uv.y);
                points += 1;
                let got = definition_partials_over_box(
                    &k, &mut d, &coarse, ku, kv, uv.x, uv.x, uv.y, uv.y, 0,
                );
                match got {
                    Ok(p) => {
                        proved += 1;
                        let want = pb.evaluate_point(&k, uv).unwrap();
                        worst = worst.max(outside(p.at(0, 0), want));
                    }
                    Err(_) if x == ks[0] || x == ks[ks.len() - 1] => ends_refused += 1,
                    Err(_) => {}
                }
            }
        }
    }
    println!("{rounds} round(s): {proved} of {points} points ({ends_refused} at the ends refused) and {boxes_proved} of {boxes} boxes proved, the worst {worst:.3e} off its definition");
    assert_eq!(rounds, 1);
    assert!(
        proved + ends_refused == points && boxes_proved == boxes && worst == 0.0,
        "{proved} of {points} ({ends_refused} ends refused), {boxes_proved} of {boxes} boxes, worst {worst:e}"
    );
}
