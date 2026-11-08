//! The SolidCraft fork's tangency-labs/smlib@64e5632 and 8f145ee (#2273, smlib#79 step 7):
//! a variable round whose balls exist over the variable radius solver's stations is its
//! definition (`fillet_round_as_definition`, WR-698, on `fillet_gn_section_generator`).
//!
//! The capi's `variable_round_on_a_cone_is_its_definition` and
//! `variable_round_quadratic_law_is_its_definition`: the top back edge (y = 30, z = 20) of a
//! box 20 x 30 x 20 rounded by a B-spline law along x, graded as the fork's `section_blend`
//! grades them: the status, SMLib's validity check at level 2, the volume, every point of a
//! 17 x 17 grid of each blend face's surface (a face on none of the box's planes) by its
//! definition on the exact blend to rounding, its net on it within the fillet's tolerance and
//! within its certified bound of the definition, the net's spans, the definition's work as
//! counts, and at nine stations along each seam with a rail face the order of contact at
//! Continuity Analysis's tolerances (a fitted blend, G1 and not G2).
//!
//! How this test drives the kernel differs from the capi: the edge is picked as the nearest
//! within 0.1 of a point, and the round is `FilletSession::prepare_variable_radius_edges`
//! with the capi's law (`law_along`: the radius as x, the Greville abscissae as y, turned
//! with the edge), the capi's size (the larger end radius) and its circular section.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_precision_loss,
    clippy::needless_range_loop
)] // test helpers

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_standard_solver::p1::FilletLaw;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::vector2d::Point2;
use solidcore::kernel::{KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::p1::{
    BSplineCurve, BSplineCurveForm, CanonicalData, KnotType,
};
use solidcore::nurbs::b_spline_surface::SurfParamType;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::surface::procedural_blend::BlendCounts;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::edge;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::face::p2 as face;
use solidcore::topology::{EdgeId, FaceId};
use solidcore::{Brep, Kernel, Result};

/// A surface crossed by the plane through `p` square to `b`: the curve's height along `n`
/// over the line `p + sigma w`, at `sigma = j step` (j = 1..7), walked from `uv` by Newton.
/// Its first four derivatives at 0, from the polynomial of degree 7 through h(0) = 0 and
/// those seven.
#[allow(clippy::too_many_arguments, clippy::many_single_char_names)]
fn seam_graph(
    k: &Kernel,
    s: &Surface,
    mut uv: Point2,
    p: Point3,
    b: Vec3,
    w: Vec3,
    n: Vec3,
    step: f64,
) -> Option<[f64; 4]> {
    let mut m = [[0.0f64; 8]; 7];
    let along = |q: Point3, d: Vec3| (q.x - p.x) * d.x + (q.y - p.y) * d.y + (q.z - p.z) * d.z;
    for j in 1..=7 {
        let sigma = f64::from(j) * step;
        let mut converged = false;
        for _ in 0..40 {
            if converged {
                break;
            }
            let (q, du, dv) = s.evaluate_1st_derivatives(k, uv, true, true).ok()?;
            let (f0, f1) = (along(q, b), along(q, w) - sigma);
            let (a00, a01, a10, a11) = (du.dot(b), dv.dot(b), du.dot(w), dv.dot(w));
            let det = a00 * a11 - a01 * a10;
            if det == 0.0 {
                return None;
            }
            let (ddu, ddv) = ((f0 * a11 - a01 * f1) / det, (a00 * f1 - a10 * f0) / det);
            uv = Point2::new(uv.x - ddu, uv.y - ddv);
            converged = ddu.abs() + ddv.abs() <= 1e-15 * (1.0 + uv.x.abs() + uv.y.abs());
        }
        let q = s.evaluate_point(k, uv).ok()?;
        let mut x = 1.0;
        let row = &mut m[usize::try_from(j - 1).unwrap()];
        for cell in row.iter_mut().take(7) {
            x *= sigma;
            *cell = x;
        }
        row[7] = along(q, n);
    }
    for c in 0..7 {
        let mut piv = c;
        for r in c + 1..7 {
            if m[r][c].abs() > m[piv][c].abs() {
                piv = r;
            }
        }
        m.swap(c, piv);
        for r in c + 1..7 {
            let f = m[r][c] / m[c][c];
            for i in c..8 {
                let v = m[c][i];
                m[r][i] -= f * v;
            }
        }
    }
    let mut a = [0.0; 7];
    for c in (0..7).rev() {
        let mut v = m[c][7];
        for i in c + 1..7 {
            v -= m[c][i] * a[i];
        }
        a[c] = v / m[c][c];
    }
    Some([a[0], 2.0 * a[1], 6.0 * a[2], 24.0 * a[3]])
}

/// The order a seam reaches: 1 where the faces' graphs have slopes within `tol[0]`, 2 and
/// their second derivatives within `tol[1]`, and so on.
fn seam_order(diff: &[f64; 4], tol: &[f64; 4]) -> usize {
    let mut k = 0;
    while k < 4 && diff[k] <= tol[k] {
        k += 1;
    }
    k
}

/// The box's planes: a point and the unit normal.
const BOX_PLANES: [([f64; 3], [f64; 3]); 6] = [
    ([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
    ([20.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
    ([0.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    ([0.0, 30.0, 0.0], [0.0, 1.0, 0.0]),
    ([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
    ([0.0, 0.0, 20.0], [0.0, 0.0, 1.0]),
];

/// The two planes the round meets along its contact lines: the top and the back.
const RAILS: [usize; 2] = [5, 3];

fn plane_distance(i: usize, c: Point3) -> f64 {
    let (p, n) = BOX_PLANES[i];
    ((c.x - p[0]) * n[0] + (c.y - p[1]) * n[1] + (c.z - p[2]) * n[2]).abs()
}

/// A radius law as the capi's `smlib_fillet_variable` takes it: degree, the full knot
/// vector and the radii, from the edge's end nearest `START`.
struct Law {
    degree: usize,
    knots: &'static [f64],
    radii: &'static [f64],
}

/// The capi's `start`: the law runs from the edge's end at x = 0.
const START: [f64; 3] = [0.0, 30.0, 20.0];

/// r = 3 + x/10 (64e5632's `variable_round_on_a_cone_is_its_definition`).
const LINEAR: Law = Law {
    degree: 1,
    knots: &[0.0, 0.0, 1.0, 1.0],
    radii: &[3.0, 5.0],
};

/// The quadratic law of radii 3, 4, 3 (64e5632's
/// `variable_round_quadratic_law_is_its_definition`).
const QUADRATIC: Law = Law {
    degree: 2,
    knots: &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
    radii: &[3.0, 4.0, 3.0],
};

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

/// The capi's `law_along`: the law turned with the edge where its far end is nearer
/// `START`, the radius as x and as y the Greville abscissae (y(t) = t), each distinct knot
/// once with its multiplicity.
fn law_along(k: &Kernel, b: &Brep, e: EdgeId, law: &Law) -> Result<FilletLaw> {
    let ivl = edge::interval(b, e)?;
    let c = edge::curve(b, e)?.unwrap();
    let lo = c.evaluate(k, ivl.min(), 0, true, false)?[0];
    let hi = c.evaluate(k, ivl.max(), 0, true, false)?[0];
    let at = Point3::new(START[0], START[1], START[2]);
    let reverse = hi.distance(at) < lo.distance(at);
    let n = law.knots.len();
    let kv: Vec<f64> = (0..n)
        .map(|i| {
            if reverse {
                1.0 - law.knots[n - 1 - i]
            } else {
                law.knots[i]
            }
        })
        .collect();
    let m = law.radii.len();
    let control: Vec<Vec3> = (0..m)
        .map(|i| {
            let g: f64 = (1..=law.degree).map(|j| kv[i + j]).sum::<f64>() / law.degree as f64;
            Vec3::new(law.radii[if reverse { m - 1 - i } else { i }], g, 0.0)
        })
        .collect();
    let (mut knots, mut mults) = (Vec::<f64>::new(), Vec::<usize>::new());
    for &x in &kv {
        if knots.last() == Some(&x) {
            *mults.last_mut().unwrap() += 1;
        } else {
            knots.push(x);
            mults.push(1);
        }
    }
    let curve = BSplineCurve::create_canonical(&CanonicalData {
        dim: 3,
        degree: law.degree,
        control_points: &control,
        form: BSplineCurveForm::default(),
        knot_multiplicities: &mults,
        knots: &knots,
        knot_type: KnotType::default(),
        weights: None,
        trim: None,
    })?;
    Ok(FilletLaw::bspline(curve, &Extent1d::from_min_max(0.0, 1.0)))
}

/// The capi's `smlib_fillet_variable` on the box's top back edge: the circular section on
/// the variable radius solver, the size the larger end radius.
fn round(k: &mut Kernel, b: &mut Brep, law: &Law) -> Result<()> {
    let e = pick(k, b, [10.0, 30.0, 20.0]);
    let l = law_along(k, b, e, law)?;
    let size = law.radii[0].max(law.radii[law.radii.len() - 1]);
    {
        let mut s = FilletSession::new(k, b)?;
        s.prepare_variable_radius_edges(k, &[e], size, l, 1, 2)?;
        s.do_fillet(k)?;
    }
    stitch_and_orient(k, b, &StitchAndOrientOptions::default())?;
    b.refresh_cached_topology();
    Ok(())
}

fn volume_of(k: &Kernel, b: &mut Brep) -> Result<f64> {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-7, &attrs)
}

/// One blend face, graded.
#[derive(Debug)]
struct BlendFace {
    /// Its surface is its definition.
    definition: bool,
    /// The most a grid point of the definition is off the exact blend.
    off: f64,
    /// The most a grid point of the net (the surface itself, if not a definition) is off it.
    off_net: f64,
    /// The most a grid point of the net is off the definition, and the certified bound.
    gap: f64,
    bound: f64,
    /// The net's spans in all, u by v.
    spans: usize,
}

/// What grading a round found.
#[derive(Debug)]
struct Graded {
    counts: BlendCounts,
    volume: f64,
    /// SMLib's validity check at level 2: the failed tests.
    failures: usize,
    faces: Vec<BlendFace>,
    /// Each seam with a rail face: the order every station reaches at Continuity
    /// Analysis's tolerances, and the most any reaches.
    seams: Vec<(usize, usize)>,
}

/// The fork's `section_blend` with a fitted round of `size`, `off` the distance from the
/// exact blend (if it has a closed form).
fn grade(k: &mut Kernel, law: &Law, size: f64, off: Option<&dyn Fn(Point3) -> f64>) -> Graded {
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), 20.0, 30.0, 20.0).unwrap();
    k.blend_counts.set(BlendCounts::default());
    let made = round(k, &mut b, law);
    let counts = k.blend_counts.get();
    if let Err(err) = made {
        panic!("the round failed: {err:?}");
    }
    let mut g = Graded {
        counts,
        volume: volume_of(k, &mut b).unwrap(),
        failures: check_brep(k, &mut b, CheckLevel::Level2)
            .unwrap()
            .lines()
            .len(),
        faces: Vec::new(),
        seams: Vec::new(),
    };
    let off = |x: Point3| off.map_or(0.0, |f| f(x));
    let loose = [1f64.to_radians().tan(), 0.01, 0.001, 0.0001];
    let ids: Vec<FaceId> = b.faces().ids().collect();
    let edges: Vec<EdgeId> = b.edge_list().to_vec();
    let on_plane = |s: &Surface, i: usize| {
        let Ok(dom) = s.natural_uv_domain_with_nurb(k) else {
            return false;
        };
        let mid = Point2::new(
            0.5 * (dom.min().x + dom.max().x),
            0.5 * (dom.min().y + dom.max().y),
        );
        s.evaluate_point(k, mid)
            .is_ok_and(|m| plane_distance(i, m) <= 1e-9)
    };
    for f in ids {
        let Some(srf) = face::surface(&b, f).unwrap() else {
            continue;
        };
        if (0..BOX_PLANES.len()).any(|i| on_plane(srf, i)) {
            continue;
        }
        let dom = srf.natural_uv_domain_with_nurb(k).unwrap();
        let pb = match srf.kind() {
            SurfaceKind::ProceduralBlend(pb) => Some(pb),
            _ => None,
        };
        let mut bf = BlendFace {
            definition: pb.is_some(),
            off: 0.0,
            off_net: 0.0,
            gap: 0.0,
            bound: pb.map_or(0.0, |pb| pb.bound()),
            spans: 0,
        };
        for a in 0..=16 {
            for c in 0..=16 {
                let uv = Point2::new(
                    dom.min().x + (dom.max().x - dom.min().x) * f64::from(a) / 16.0,
                    dom.min().y + (dom.max().y - dom.min().y) * f64::from(c) / 16.0,
                );
                let x = srf.evaluate_point(k, uv).unwrap();
                if let Some(pb) = pb {
                    let y = pb.net().evaluate_point(uv).unwrap();
                    bf.off = bf.off.max(off(x));
                    bf.off_net = bf.off_net.max(off(y));
                    bf.gap = bf.gap.max(x.distance(y));
                } else {
                    bf.off_net = bf.off_net.max(off(x));
                }
            }
        }
        if let Some(pb) = pb {
            let u = pb.net().knots(SurfParamType::U, None).unwrap().0.len();
            let v = pb.net().knots(SurfParamType::V, None).unwrap().0.len();
            bf.spans = (u - 1) * (v - 1);
        }
        g.faces.push(bf);
        for &e in &edges {
            let uses = edge::edgeuses(&b, e, None, None).unwrap();
            let (mut mine, mut other) = (None, None);
            for &u in &uses {
                match eu1::face(&b, u).unwrap() {
                    Some(h) if h == f => mine = Some(u),
                    Some(_) => other = Some(u),
                    None => {}
                }
            }
            let (Some(mine), Some(other)) = (mine, other) else {
                continue;
            };
            let (Some(tm), Some(to)) = (
                eu1::uv_trim_curve(&b, mine).unwrap().0,
                eu1::uv_trim_curve(&b, other).unwrap().0,
            ) else {
                continue;
            };
            let of = eu1::face(&b, other).unwrap().unwrap();
            let osrf = face::surface(&b, of).unwrap().unwrap();
            if !on_plane(osrf, RAILS[0]) && !on_plane(osrf, RAILS[1]) {
                continue; // an end of the blend, not one of its contact lines
            }
            let ivl = edge::interval(&b, e).unwrap();
            let curve = edge::curve(&b, e).unwrap().unwrap();
            let (mut most, mut least) = ([0.0f64; 4], [f64::INFINITY; 4]);
            for st in 1..=9 {
                let t = ivl.min() + (ivl.max() - ivl.min()) * f64::from(st) / 10.0;
                let pt = curve.evaluate(k, t, 1, true, false).unwrap();
                let (p, along) = (pt[0], pt[1].unitize().unwrap());
                let w1 = tm.evaluate(k, t, 0, true, false).unwrap()[0];
                let w2 = to.evaluate(k, t, 0, true, false).unwrap()[0];
                let uv2 = Point2::new(w2.x, w2.y);
                let (_, du, dv) = osrf.evaluate_1st_derivatives(k, uv2, true, true).unwrap();
                let n = du.cross(dv).unitize().unwrap();
                let mut w = n.cross(along).unitize().unwrap();
                let uv = Point2::new(w1.x, w1.y);
                let inward = Point2::new(
                    uv.x + 1e-3 * (0.5 * (dom.min().x + dom.max().x) - uv.x),
                    uv.y + 1e-3 * (0.5 * (dom.min().y + dom.max().y) - uv.y),
                );
                let q = srf.evaluate_point(k, inward).unwrap();
                if (q.x - p.x) * w.x + (q.y - p.y) * w.y + (q.z - p.z) * w.z < 0.0 {
                    w = -w;
                }
                // a fitted blend's seam, over steps wide enough that the fit's gap does not
                // read as a difference of the derivatives
                let step = 0.02 * size;
                let hb = seam_graph(k, srf, uv, p, along, w, n, step);
                let ho = seam_graph(k, osrf, uv2, p, along, w, n, -step);
                for m in 0..4 {
                    let d = match (&hb, &ho) {
                        (Some(a), Some(c)) => (a[m] - c[m]).abs(),
                        _ => f64::INFINITY,
                    };
                    most[m] = most[m].max(d);
                    least[m] = least[m].min(d);
                }
            }
            g.seams
                .push((seam_order(&most, &loose), seam_order(&least, &loose)));
        }
    }
    g
}

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

/// The fork's behaviour these cases reach, and no other fix: the round as its definition
/// (`fillet_round_as_definition`) on the G^n section generator, whose circle is a definition
/// only where it is the ball's exact circle (`fillet_exact_arc_sections`); and the trace and
/// ends the fork's net is certified over, the variable round's spans tested against its law
/// (`fillet_variable_span_follows_fillet`, the fork's 4438859), with the blend corner's End at
/// each end of the edge.
fn fork_kernel() -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes.fillet_gn_section_generator = true;
    k.fixes.fillet_round_as_definition = true;
    k.fixes.fillet_exact_arc_sections = true;
    k.fixes.fillet_variable_span_follows_fillet = true;
    k
}

/// r = 3 + x/10: the balls' centres (x, 30 - r, 20 - r) run on a line, r proportional to
/// their distance from the apex (-30, 30, 20) where r would be 0, so the balls' envelope is
/// the circular cone about that line, sin(phi) = r / |c - apex| = 1 / sqrt(102). A point's
/// distance from it.
fn off_the_cone(x: Point3) -> f64 {
    off_a_cone(x, [-30.0, 30.0, 20.0], 0.1)
}

/// The cone of balls whose centres run on (x, 30 - r, 20 - r), r = slope (x - apex.x): a
/// point's distance from it.
fn off_a_cone(x: Point3, apex: [f64; 3], slope: f64) -> f64 {
    let l = (1.0 + 2.0 * slope * slope).sqrt();
    let ax = [1.0 / l, -slope / l, -slope / l];
    let sphi = slope / l;
    let cphi = (1.0 + slope * slope).sqrt() / l;
    let w = [x.x - apex[0], x.y - apex[1], x.z - apex[2]];
    let along = w[0] * ax[0] + w[1] * ax[1] + w[2] * ax[2];
    let squared = w[0] * w[0] + w[1] * w[1] + w[2] * w[2];
    let across = (squared - along * along).max(0.0).sqrt();
    (across * cphi - along * sphi).abs()
}

/// What the fork's grade of a variable round by its definition holds, on one kernel: its
/// net's `spans` exactly, and the definition's evaluations and station solves at most (the
/// fork's counts, about 5% over those measured on Linux; each solve seeded within the bound,
/// two Newton steps at most).
fn assert_definition(
    k: &mut Kernel,
    law: &Law,
    off: Option<&dyn Fn(Point3) -> f64>,
    spans: usize,
    counts: (u64, u64),
) -> Graded {
    let off_tol = 100.0
        * create_box(k, Point3::new(0.0, 0.0, 0.0), 20.0, 30.0, 20.0)
            .unwrap()
            .tolerance();
    let g = grade(k, law, 3.0, off);
    println!("{g:?}, volume {:.13}", g.volume);
    assert_eq!(g.failures, 0, "{g:?}");
    assert!(!g.faces.is_empty() && g.seams.len() >= 2, "{g:?}");
    for f in &g.faces {
        assert!(f.definition, "not its definition: {g:?}");
        // the definition on the exact blend to rounding, the net within the fillet's tolerance
        assert!(f.off <= 1e-11 && f.off_net <= off_tol, "{g:?}");
        assert!(f.gap <= f.bound, "the net off its definition: {g:?}");
        assert_eq!(f.spans, spans, "{g:?}");
    }
    // a fitted round meets both faces G1 and no more, at Continuity Analysis's tolerances
    assert!(g.seams.iter().all(|&s| s == (1, 1)), "{g:?}");
    assert!(
        g.counts.evaluations <= counts.0
            && g.counts.solves <= counts.1
            && g.counts.most_newton_steps <= 2,
        "{g:?}"
    );
    g
}

/// 64e5632 and 8f145ee: r = 3 + x/10 is its definition, the solver's stations running from
/// x = -10 to 30, where r stays from 2 to 6. The volume removed, 69.8731836896940, is the area
/// the cone's section in each plane x = const cuts off the corner, integrated over x (the
/// fork's figure): V = 11930.1268163103.
#[test]
fn the_variable_round_on_a_cone_is_its_definition() {
    for mut k in [fork_kernel(), solidcraft_kernel()] {
        let g = assert_definition(&mut k, &LINEAR, Some(&off_the_cone), 16, (9100, 5250));
        let want = 11_930.126_816_310_3;
        assert!(
            (g.volume - want).abs() <= 1e-7 * want,
            "volume {}, want {want}",
            g.volume
        );
    }
}

/// 64e5632: the quadratic law of radii 3, 4, 3 has no closed form, so the round is graded as
/// its definition only, its net within its certified bound of it.
#[test]
fn the_variable_round_of_a_quadratic_law_is_its_definition() {
    for mut k in [fork_kernel(), solidcraft_kernel()] {
        assert_definition(&mut k, &QUADRATIC, None, 22, (9600, 5450));
    }
}

/// 64e5632: r = 1 + x/5 is not its definition. The solver rolls the ball on past the edge's
/// ends (x from -10 to 30) with the law continued, and the law is 0 at x = -5, the cone's
/// apex: no ball of that radius exists there. The round is still the fitted one on its cone
/// (the fork's `variable_round_on_its_cone`, volume 11956.2224267359 to 1e-5).
#[test]
fn a_variable_round_whose_law_reaches_zero_over_the_stations_is_not_its_definition() {
    const STEEP: Law = Law {
        degree: 1,
        knots: &[0.0, 0.0, 1.0, 1.0],
        radii: &[1.0, 5.0],
    };
    for mut k in [fork_kernel(), solidcraft_kernel()] {
        let off_tol = 100.0
            * create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 20.0, 30.0, 20.0)
                .unwrap()
                .tolerance();
        let g = grade(
            &mut k,
            &STEEP,
            1.0,
            Some(&|x| off_a_cone(x, [-5.0, 30.0, 20.0], 0.2)),
        );
        println!("{g:?}, volume {:.13}", g.volume);
        assert_eq!(g.failures, 0, "{g:?}");
        assert!(!g.faces.is_empty() && g.seams.len() >= 2, "{g:?}");
        for f in &g.faces {
            assert!(!f.definition && f.off_net <= off_tol, "{g:?}");
        }
        assert!(g.seams.iter().all(|&s| s == (1, 1)), "{g:?}");
        let want = 11_956.222_426_735_9;
        assert!(
            (g.volume - want).abs() <= 1e-5 * want,
            "volume {}, want {want}",
            g.volume
        );
    }
}

/// Without `fillet_round_as_definition` the round on the cone is its net alone, a B-spline,
/// as before 0d43b4c (the fork's tests failed there).
#[test]
fn without_the_definition_fix_the_round_on_the_cone_is_not_its_definition() {
    let mut k = fork_kernel();
    k.fixes.fillet_round_as_definition = false;
    let g = grade(&mut k, &LINEAR, 3.0, Some(&off_the_cone));
    println!("{g:?}, volume {:.13}", g.volume);
    assert!(
        !g.faces.is_empty() && g.faces.iter().all(|f| !f.definition),
        "{g:?}"
    );
}
