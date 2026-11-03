//! tangency-labs/solidcore#2479: a pcurve's end points lie in its face surface's
//! parameter domain. `SmFace::SwapUV` mirrored each UV trim about `u = v` through a
//! rounded matrix, so every face `StitchAndOrient` (or any other reversal) swaps had trims
//! an ulp past its domain's bounds: a unit box's three swapped faces end at
//! `v = -2.2e-16`. With `Kernel::fixes.face_swap_uv_transposes_trims` (WR-729) the trims
//! are transposed exactly. The scan ([`support::pcurve_domain_scan`]) walks every face's
//! loops; the cases are the operations whose out-of-domain ends were all made there.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

#[path = "support/pcurve_domain_scan.rs"]
mod support;

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::curve::curve::Curve;
use solidcore::primitive::api_curves::{create_analytic_circle, create_line_segment};
use solidcore::primitive::api_primitives::{
    create_box, create_cone, create_cylinder, create_sphere, create_torus,
};
use solidcore::sweep::api_brep::{create_linear_sweep, create_rotational_sweep};
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel, Point3, Vec3};

fn kernel(fix: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.face_swap_uv_transposes_trims = fix;
    k
}

/// The edges whose midpoint satisfies `keep`.
fn edges_where(k: &Kernel, b: &Brep, keep: impl Fn(Point3) -> bool) -> Vec<EdgeId> {
    let edges: Vec<EdgeId> = b.edges().ids().collect();
    edges
        .into_iter()
        .filter(|&e| {
            let ivl = edge::interval(b, e).unwrap();
            let c = edge::curve(b, e).unwrap().unwrap();
            keep(c.evaluate_point(k, 0.5 * (ivl.min() + ivl.max())).unwrap())
        })
        .collect()
}

fn p(x: f64, y: f64, z: f64) -> Point3 {
    Point3::new(x, y, z)
}

type Case = (&'static str, fn(&mut Kernel) -> Brep);

/// The operations of the regression: primitives, booleans through seams, fillets and
/// chamfers on planar and curved faces, a sweep and a revolve.
const CASES: &[Case] = &[
    ("unit box", |k| {
        create_box(k, p(0.0, 0.0, 0.0), 1.0, 1.0, 1.0).unwrap()
    }),
    ("offset box", |k| {
        create_box(k, p(-5.0, 3.0, 7.0), 10.0, 20.0, 30.0).unwrap()
    }),
    ("cylinder", |k| {
        create_cylinder(k, p(1.0, 2.0, 3.0), 3.0, 10.0).unwrap()
    }),
    ("cone", |k| {
        create_cone(k, p(0.0, 0.0, 0.0), 4.0, 1.0, 6.0).unwrap()
    }),
    ("sphere", |k| {
        create_sphere(k, p(0.0, 0.0, 0.0), 5.0).unwrap()
    }),
    ("torus", |k| {
        create_torus(k, p(0.0, 0.0, 0.0), 6.0, 2.0).unwrap()
    }),
    ("box minus a cylinder through it", |k| {
        let block = create_box(k, p(0.0, 0.0, 0.0), 40.0, 40.0, 20.0).unwrap();
        let hole = create_cylinder(k, p(20.0, 20.0, -1.0), 3.0, 22.0).unwrap();
        boolean_difference(k, block, hole).unwrap()
    }),
    ("box minus a sphere across its seam", |k| {
        let block = create_box(k, p(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
        let ball = create_sphere(k, p(10.0, 5.0, 5.0), 3.0).unwrap();
        boolean_difference(k, block, ball).unwrap()
    }),
    ("box minus a cone", |k| {
        let block = create_box(k, p(0.0, 0.0, 0.0), 20.0, 20.0, 10.0).unwrap();
        let cone = create_cone(k, p(10.0, 10.0, -1.0), 4.0, 2.0, 12.0).unwrap();
        boolean_difference(k, block, cone).unwrap()
    }),
    ("box minus a torus", |k| {
        let block = create_box(k, p(-10.0, -10.0, 0.0), 20.0, 20.0, 5.0).unwrap();
        let ring = create_torus(k, p(0.0, 0.0, 5.0), 6.0, 2.0).unwrap();
        boolean_difference(k, block, ring).unwrap()
    }),
    ("cylinder union a crescent across its seam", |k| {
        let small = create_cylinder(k, p(10.0, 0.0, 0.0), 6.0, 10.0).unwrap();
        let big = create_cylinder(k, p(0.0, 0.0, 0.0), 10.0, 10.0).unwrap();
        let crescent = boolean_difference(k, small, big).unwrap();
        let cylinder = create_cylinder(k, p(0.0, 0.0, 0.0), 10.0, 10.0).unwrap();
        boolean_union(k, cylinder, crescent).unwrap()
    }),
    ("filleted box bottom", |k| {
        let mut tray = create_box(k, p(0.0, 0.0, 0.0), 25.0, 25.0, 10.0).unwrap();
        let bottom = edges_where(k, &tray, |m| m.z.abs() < 1e-6);
        fillet_edges(k, &mut tray, &bottom, 2.0, 1, 1, 1.0).unwrap();
        tray
    }),
    ("filleted cylinder rim", |k| {
        let mut c = create_cylinder(k, p(0.0, 0.0, 0.0), 5.0, 8.0).unwrap();
        let rim = edges_where(k, &c, |m| (m.z - 8.0).abs() < 1e-6);
        fillet_edges(k, &mut c, &rim, 1.0, 1, 1, 1.0).unwrap();
        c
    }),
    ("extruded circle", |k| {
        let circle = Curve::from(create_analytic_circle(k, p(0.0, 0.0, 0.0), 4.0).unwrap());
        create_linear_sweep(k, &[Some(&circle)], Vec3::Z, 7.0, true).unwrap()
    }),
    ("revolved rectangle", |k| {
        let corners = [
            p(2.0, 0.0, 0.0),
            p(5.0, 0.0, 0.0),
            p(5.0, 0.0, 4.0),
            p(2.0, 0.0, 4.0),
        ];
        let sides: Vec<Curve> = (0..4)
            .map(|i| Curve::from(create_line_segment(corners[i], corners[(i + 1) % 4]).unwrap()))
            .collect();
        let refs: Vec<Option<&Curve>> = sides.iter().map(Some).collect();
        create_rotational_sweep(k, &refs, Point3::ZERO, Vec3::Z, 360.0, true).unwrap()
    }),
];

/// Cases with another producer left: the old chamfer's faces (`fillet_edges` with
/// cross-section 0) end an ulp past `v = 1` with the fix too. That code is being replaced
/// by the generic blend (AGENTS.md §4b), so it is surveyed, not asserted.
const SURVEY_ONLY: &[Case] = &[("chamfered box top", |k| {
    let mut plate = create_box(k, p(0.0, 0.0, 0.0), 30.0, 20.0, 10.0).unwrap();
    let top = edges_where(k, &plate, |m| (m.z - 10.0).abs() < 1e-6);
    fillet_edges(k, &mut plate, &top, 1.5, 0, 1, 1.0).unwrap();
    plate
})];

/// The out-of-domain ends of every case of `cases`, built with or without the fix.
fn scan(cases: &[Case], fix: bool) -> Vec<(&'static str, Vec<String>)> {
    cases
        .iter()
        .map(|&(name, build)| {
            let mut k = kernel(fix);
            let b = build(&mut k);
            (name, support::report(&k, &b))
        })
        .collect()
}

#[test]
fn with_the_fix_every_pcurve_ends_in_its_face_domain() {
    let found: Vec<_> = scan(CASES, true)
        .into_iter()
        .filter(|(_, lines)| !lines.is_empty())
        .collect();
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn without_the_fix_the_swapped_faces_of_a_unit_box_end_past_their_domain() {
    let mut k = kernel(false);
    let b = create_box(&mut k, p(0.0, 0.0, 0.0), 1.0, 1.0, 1.0).unwrap();
    let found = support::scan(&k, &b);
    // the three faces StitchAndOrient swaps, two ends each at v = -2.2e-16 on [0, 1]
    let faces: std::collections::BTreeSet<_> = found.iter().map(|o| o.face).collect();
    assert_eq!(faces.len(), 3, "{found:#?}");
    assert!(found
        .iter()
        .all(|o| o.bound == 0.0 && o.value == -f64::EPSILON));
}

/// The counts the PR quotes (`--ignored --nocapture`).
#[test]
#[ignore = "survey output"]
fn survey() {
    let all: Vec<Case> = CASES.iter().chain(SURVEY_ONLY).copied().collect();
    for ((name, off), (_, on)) in scan(&all, false).into_iter().zip(scan(&all, true)) {
        println!("{name}: {} without, {} with", off.len(), on.len());
        for l in off.iter().chain(&on) {
            println!("  {l}");
        }
    }
}
