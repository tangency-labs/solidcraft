//! Regression test for tangency-labs/solidcore#1945: with `fillet_through_extended_rail_end` and
//! `fillet_trace_reaches_rails` on, the round of a bar's top front edge after a round of the
//! bar/boss arc got a UV trim with coincident control points. Root cause (traced by tdamsma on
//! #1945): `SmLocalSolveNd::SolveIt` kept the unscaled point's Jacobian for a clamped-and-scaled
//! step, so a near-tangent SSI's end landed 1.6e-8 off and the rail trim kept a 3.1e-8 span. The
//! fork's tangency-labs/smlib@d2757e1 fixes it; the port carries it under WR-253's
//! `fixes.oscillating_skips_failed_guesses`, with the commit's 1-D part.
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
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::topology::brep::p2;
use solidcore::topology::brep::p3;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
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

fn run(reach: bool, nd_fix: bool) -> (f64, f64, Vec<String>) {
    let mut k = Kernel::default();
    k.fixes.fillet_through_extended_rail_end = true;
    k.fixes.fillet_trace_reaches_rails = reach;
    k.fixes.oscillating_skips_failed_guesses = nd_fix;
    let b = base(&mut k);
    let mut b1 = round(&mut k, &b, &ARC, 2.0).unwrap();
    let mut b2 = round(&mut k, &b1, &TOP, 2.0).unwrap();
    let (v1, v2) = (volume(&k, &mut b1), volume(&k, &mut b2));
    let f = failures(&mut k, &mut b2);
    (v1, v2, f)
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

/// With the N-D solve fix the round is valid. The fork's C++ builds it clean at V 49712.42808748
/// under SolidCraft's whole flag set; with only these flags the port gives 49712.4280860.
#[test]
fn top_edge_round_is_valid() {
    let (v1, v2, f) = run(true, true);
    let (lo, hi) = bracket();
    println!(
        "on: V={v2} removed={} (bracket {lo} .. {hi}) failures={}",
        v1 - v2,
        f.len()
    );
    for x in &f {
        println!("   {}", &x[..x.len().min(180)]);
    }
    assert!((lo..=hi).contains(&(v1 - v2)));
    assert!((v2 - 49712.42808748).abs() < 1e-5, "{v2}");
    assert!(f.is_empty(), "{f:#?}");
}

/// Without it (upstream's `SolveIt`) the trim keeps the short span: check tests #15, #3, #11.
#[test]
fn top_edge_round_upstream_solve_keeps_short_trim_span() {
    let (_, _, f) = run(true, false);
    assert!(
        f.iter().any(|x| x.contains("#15 Bad Repeated CPts")),
        "{f:#?}"
    );
}
