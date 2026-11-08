//! Regression test for tangency-labs/solidcore#1945: the round of a bar's top front edge after
//! a round of the bar/boss arc got a UV trim with coincident control points. Root cause (traced
//! by tdamsma on #1945): `SmLocalSolveNd::SolveIt` kept the unscaled point's Jacobian for a
//! clamped-and-scaled step, so a near-tangent SSI's end landed 1.6e-8 off and the rail trim kept
//! a 3.1e-8 span. The fork's tangency-labs/smlib@d2757e1 fixes it; the port carries it under
//! WR-253's `fixes.oscillating_skips_failed_guesses`, with the commit's 1-D part.
//!
//! The case reached that trim only with WR-641's `fillet_through_extended_rail_end`, removed
//! in #2500; it now runs under SolidCraft's fixes, the blend corner building both rounds,
//! where the upstream solve's short span does not arise.
#![allow(
    clippy::all,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    missing_docs
)]
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::boolean::api_brep as sm;
use solidcore::geom::a_object::BooleanOperation;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::geom::vector2d::Point2;
use solidcore::intersect::point_class::{PointClassType, PointClassification};
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::query::face::p1 as fq;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p1::regions;
use solidcore::topology::brep::p2;
use solidcore::topology::brep::p3;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::face::p2 as fp2;
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, EntityRef, Kernel};

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}
fn midpoint(k: &Kernel, b: &Brep, e: EdgeId) -> Point3 {
    let ivl = edge::interval(b, e).unwrap();
    let curve = edge::curve(b, e).unwrap().unwrap();
    curve
        .evaluate(k, 0.5 * (ivl.min() + ivl.max()), 0, true, false)
        .unwrap()[0]
}
fn nearest_edge(k: &Kernel, b: &Brep, p: Point3) -> EdgeId {
    b.edges()
        .ids()
        .min_by(|&x, &y| {
            midpoint(k, b, x)
                .distance(p)
                .total_cmp(&midpoint(k, b, y).distance(p))
        })
        .unwrap()
}
fn failures(k: &mut Kernel, b: &mut Brep) -> Vec<String> {
    let c = check_brep(k, b, CheckLevel::Level2).unwrap();
    c.reports
        .reports()
        .iter()
        .chain(c.pointer_reports.reports().iter())
        .filter(|r| !r.ok)
        .map(|r| {
            format!(
                "{} #{} {}: {}",
                r.reporting_type,
                r.test_index,
                r.name,
                r.format_log_message()
            )
        })
        .collect()
}
fn named(entity: EntityRef, domain: u64) -> ObjectRef {
    let r = p3::topology_ref(0, entity);
    ObjectRef {
        type_code: r.type_code,
        key: ObjectKey(domain + r.type_code as u64 * 1_000_000 + (r.key.0 & 0xffff_ffff)),
    }
}
fn copy_of(k: &mut Kernel, b: &Brep, n: u64) -> Brep {
    let target = move |e| named(e, n.saturating_mul(100_000_000));
    let mut state = p2::ArenaNotifyState::new(&Brep::new());
    let mut env = state.env(None);
    env.key_of = &target;
    p3::api_brep_copy(k, &mut env, Some(b), &|e| named(e, 0)).unwrap()
}
fn round(k: &mut Kernel, b: &Brep, edges_at: &[Point3], r: f64) -> Result<Brep, String> {
    let mut w = copy_of(k, b, 1);
    let edges: Vec<EdgeId> = edges_at.iter().map(|&p| nearest_edge(k, &w, p)).collect();
    let sizes = vec![r; edges.len()];
    {
        let mut s = FilletSession::new(k, &mut w).map_err(|e| format!("{e:?}"))?;
        s.prepare_per_edge_constant_radius(
            k,
            &edges,
            &sizes,
            &[FilletSurfaceGeneratorType::Circular],
            1,
            1.0,
        )
        .map_err(|e| format!("{e:?}"))?;
        s.create_corners(k, None).map_err(|e| format!("{e:?}"))?;
        if let Err(e) = s.do_filleting(k, None) {
            return Err(format!("{e:?} {:?}", s.error_info().comment));
        }
    }
    stitch_and_orient(k, &mut w, &StitchAndOrientOptions::default())
        .map_err(|e| format!("{e:?}"))?;
    w.refresh_cached_topology();
    Ok(w)
}
fn base(k: &mut Kernel) -> Brep {
    use solidcore::curve::curve::Curve;
    use solidcore::curve::ellipse::Ellipse;
    use solidcore::curve::line::Line;
    use solidcore::geom::axis2_placement::Axis2Placement;
    use solidcore::geom::extent1d::Extent1d;
    use solidcore::math::Vec3;
    use solidcore::sweep::api_brep::create_linear_sweep;
    let v = |x, y, z| Vec3::new(x, y, z);
    let pts = [
        v(0.0, -15.0, 0.0),
        v(100.0, -15.0, 0.0),
        v(100.0, 15.0, 0.0),
        v(0.0, 15.0, 0.0),
    ];
    let lines: Vec<Curve> = (0..4)
        .map(|i| Curve::from(Line::create_line_segment(3, pts[i], pts[(i + 1) % 4], None).unwrap()))
        .collect();
    let refs: Vec<Option<&Curve>> = lines.iter().map(Some).collect();
    let bar = create_linear_sweep(k, &refs, v(0.0, 0.0, 1.0), 10.0, true).unwrap();
    let a = 0.6131f64;
    let x0 = v(a.cos(), a.sin(), 0.0);
    let y0 = v(0.0, 0.0, 1.0).cross(x0);
    let frame = Axis2Placement::new(v(0.0, 0.0, 0.0), x0, y0);
    let degrees = Extent1d::from_min_max(0.0, 360.0);
    let c = Curve::from(
        Ellipse::create_canonical_circle(k, &frame, 20.0, Some(&degrees), None).unwrap(),
    );
    let boss = create_linear_sweep(k, &[Some(&c)], v(0.0, 0.0, 1.0), 20.0, true).unwrap();
    let ca = copy_of(k, &bar, 1);
    let cb = copy_of(k, &boss, 2);
    sm::boolean(k, ca, cb, BooleanOperation::Union)
        .unwrap()
        .result
}

/// The round of the edges nearest `edges_at`, driven as `SmApiFilletEdgesPerEdge` does.
const ARC: [Point3; 2] = [
    Point3::new(14.92183, 13.31687, 10.0),
    Point3::new(19.84481, -2.48666, 10.0),
];
const TOP: [Point3; 2] = [
    Point3::new(58.04674, -15.0, 10.0),
    Point3::new(14.25957, -15.0, 10.48333),
];

/// SolidCraft's fixes: every `UpstreamFixes` field on except `geometric_g2_continuity`, so
/// both rounds are built by the blend corner, the construction kept since WR-641's `fillet_through_extended_rail_end` was removed (#2500).
fn solidcraft_kernel() -> Kernel {
    let mut k = Kernel::default();
    for &name in UpstreamFixes::NAMES {
        if name != "geometric_g2_continuity" {
            *k.fixes.flag_mut(name).unwrap() = true;
        }
    }
    k
}

fn lumps(b: &Brep) -> usize {
    let infinite = b.infinite_region();
    regions(b, None)
        .unwrap()
        .into_iter()
        .filter(|r| Some(*r) != infinite)
        .count()
}

/// The first round's torus: the ball r 2 rolling on the bar's top z = 10 and the boss
/// rho = 20 has its centre on the circle rho = 22, z = 12.
fn torus_distance(p: Point3) -> f64 {
    let rho = (p.x * p.x + p.y * p.y).sqrt();
    ((rho - 22.0).powi(2) + (p.z - 12.0).powi(2)).sqrt()
}

/// Whether `p` lies on a face the second round does not make: the bar's planes, the boss's
/// cylinder or the first round's torus.
fn on_given_face(p: Point3) -> bool {
    let rho = (p.x * p.x + p.y * p.y).sqrt();
    [
        p.x,
        p.x - 100.0,
        p.y - 15.0,
        p.y + 15.0,
        p.z,
        p.z - 10.0,
        p.z - 20.0,
        rho - 20.0,
        torus_distance(p) - 2.0,
    ]
    .iter()
    .any(|d| d.abs() < 1e-7)
}

/// The distance from `p` to the envelope of the round's balls (r 2.0), less nothing: the
/// balls' centres lie on the side plane's offset `y = -13.0` and either on the top's offset
/// `z = 8.0` (a line) or at 2.0 + 2.0 from the first round's centre circle `rho = 22.0`,
/// `z = 12.0` (the offset torus's section by that plane, a closed curve, sampled over its
/// angle and refined by golden section); `p` is on the envelope where its nearest centre is
/// at `r`.
fn off_envelope(p: Point3) -> f64 {
    let r = 2.0_f64;
    let line = ((p.y - (-13.0_f64)).powi(2) + (p.z - (8.0_f64)).powi(2)).sqrt();
    // the centre at angle t round the offset torus's section in the plane y = -13.0
    let centre = |t: f64| {
        let rho = 22.0_f64 + (2.0_f64 + r) * t.cos();
        let z = 12.0_f64 + (2.0_f64 + r) * t.sin();
        let x2 = rho * rho - (-13.0_f64) * (-13.0_f64);
        (x2 >= 0.0).then(|| Point3::new(x2.sqrt(), -13.0_f64, z))
    };
    let dist = |t: f64| centre(t).map_or(f64::INFINITY, |c| c.distance(p));
    let n = 2000;
    let step = 2.0 * std::f64::consts::PI / f64::from(n);
    let best = (0..n)
        .map(|i| f64::from(i) * step)
        .min_by(|&a, &b| dist(a).total_cmp(&dist(b)))
        .unwrap();
    let (mut lo, mut hi) = (best - step, best + step);
    let g = (5f64.sqrt() - 1.0) / 2.0;
    for _ in 0..200 {
        let (m1, m2) = (hi - g * (hi - lo), lo + g * (hi - lo));
        if dist(m1) < dist(m2) {
            hi = m2;
        } else {
            lo = m1;
        }
    }
    let torus = dist(0.5 * (lo + hi));
    (line - r).abs().min((torus - r).abs())
}

/// The round r 2 of the bar's top front edge on its definition: every point of a face the
/// round makes (sampled 9 x 9 over its surface's domain, the samples on the face) lies on
/// the envelope of its balls ([`off_envelope`]). The largest miss over the faces that are
/// their definition (`Surface::procedural_blend`) and over the others (B-splines traced
/// within the round's tolerance), with the count of each.
fn off_definition(k: &Kernel, b: &mut Brep) -> [(f64, usize); 2] {
    let mut out = [(0.0f64, 0usize); 2];
    for f in p1::faces(b, None).unwrap() {
        let s = fp2::surface(b, f).unwrap().unwrap().clone();
        let d = s.natural_uv_domain_with_nurb(k).unwrap();
        let at = |i: i32, j: i32| {
            Point2::new(
                d.min().x + (d.max().x - d.min().x) * f64::from(i) / 8.0,
                d.min().y + (d.max().y - d.min().y) * f64::from(j) / 8.0,
            )
        };
        if on_given_face(s.evaluate_point(k, at(4, 4)).unwrap()) {
            continue;
        }
        let slot = &mut out[usize::from(s.procedural_blend().is_none())];
        slot.1 += 1;
        for i in 0..=8 {
            for j in 0..=8 {
                let uv = at(i, j);
                let p = s.evaluate_point(k, uv).unwrap();
                // only the face's own points: past its trims the surface may be the
                // trace's extension, on extended faces
                let mut pc = PointClassification::new(1e-6);
                fq::point_3d_classify(
                    k,
                    b,
                    f,
                    p,
                    1e-6,
                    true,
                    &mut pc,
                    fq::Point3dClassifyOptions::default(),
                )
                .unwrap();
                if pc.point_class() == PointClassType::Unknown {
                    continue;
                }
                let miss = off_envelope(p);
                slot.0 = slot.0.max(miss);
            }
        }
    }
    out
}

/// Both rounds under SolidCraft's fixes: the volumes after the first and the second, and
/// the second body's `check_brep` failures, lump count and round on its definition.
fn run() -> (f64, f64, Vec<String>, usize, [(f64, usize); 2]) {
    let mut k = solidcraft_kernel();
    let b = base(&mut k);
    let mut b1 = round(&mut k, &b, &ARC, 2.0).unwrap();
    let mut b2 = round(&mut k, &b1, &TOP, 2.0).unwrap();
    let (v1, v2) = (volume(&k, &mut b1), volume(&k, &mut b2));
    let f = failures(&mut k, &mut b2);
    let definition = off_definition(&k, &mut b2);
    (v1, v2, f, lumps(&b2), definition)
}

/// Analytic bracket for what the r2 round of the bar's top front edge removes: a straight convex
/// round takes (1 - pi/4) r^2 per length. Its straight run starts where the first round's rail
/// meets the front face (x = sqrt(22^2 - 15^2)) and the removal stops at the cylinder wall's line
/// on the front face (x = sqrt(20^2 - 15^2)) at most: 72.0260 <= removed <= 74.4851.
fn bracket() -> (f64, f64) {
    let per_mm = (1.0 - std::f64::consts::PI / 4.0) * 4.0;
    (
        per_mm * (100.0 - 259f64.sqrt()),
        per_mm * (100.0 - 175f64.sqrt()),
    )
}

/// The second round, the bar's top front edge and its run over the first round up to the
/// boss, ends at an End on the boss where the first round's torus touches the boss: one
/// lump, `check_brep` clean, the volume it removes within the closed-form bracket, and its
/// faces on the rolling ball's definition.
///
/// Without WR-641's flag this failed under SolidCraft's fixes (#2500), 1001
/// `CalcFilletIntSideFace` at the End on the boss: the trace's extension before the End's
/// section, on the torus's offset extended round its tube (`offset_extension_as_itself`), ran
/// round the offsets' closed oval past its own span (the end-point and loop checks compare UV,
/// which the wrapped extension does not repeat), the fillet surface covered that span twice,
/// and the End's rail ends dropped onto the second sheet. The extension now stops where it
/// comes back onto the traced curve in 3D and keeps half of that gap.
#[test]
fn top_edge_round_is_valid() {
    let (v1, v2, f, lumps, [(exact_miss, exact), (traced_miss, traced)]) = run();
    eprintln!(
        "VALS removed {} exact {exact} {exact_miss:e} traced {traced} {traced_miss:e}",
        v1 - v2
    );
    let (lo, hi) = bracket();
    assert_eq!(lumps, 1);
    assert!(f.is_empty(), "{f:#?}");
    assert!((lo..=hi).contains(&(v1 - v2)), "removed {}", v1 - v2);
    // both runs are their definitions, exactly: the straight one between two planes, and
    // the one over the first round's torus between the front plane and the torus, whose
    // ball rolls on past the face onto the torus extended as itself
    assert_eq!((exact, traced), (2, 0));
    assert!(exact_miss < 1e-9, "{exact_miss}");
    assert_eq!(traced_miss, 0.0);
}
