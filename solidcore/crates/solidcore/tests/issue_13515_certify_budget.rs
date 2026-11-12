//! #13515: a work budget for the G^n section certificate. A round whose certificate would
//! certify more spans than the budget allows is built as its approximation (the caller's
//! existing fallback) instead of as its definition, so the fillet still succeeds with the
//! same topology and, within the approximation's error, the same volume.
//!
//! The part is a tilted two-circle loft: circles of radius 4 at z = 0 (centred on the
//! origin) and at z = 10 (centred 0.25 along x), skinned linearly and capped. Its bottom
//! circle edge is rounded R1.5 with the kernel's definition flag on, and that round goes
//! through the certificate: 160 spans unbounded.
//!
//! Each test body runs on a thread with a 256 MB stack: the debug build overflows the
//! default test-thread stack on this round.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_curves::create_circle;
use solidcore::sweep::api_surfaces::create_skin_primitive;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, vertex, EdgeId};
use solidcore::{Brep, Kernel};

/// SolidCraft's fix list (`tests/kernel_cases/fixes.json`), and the definition flag: the
/// round is built as its definition only with it (`definition_surface`), and only a
/// definition goes through the certificate.
fn kernel() -> Kernel {
    let names: Vec<String> = serde_json::from_str(include_str!("kernel_cases/fixes.json")).unwrap();
    let mut f = UpstreamFixes::default();
    for name in &names {
        if let Some(flag) = f.flag_mut(name) {
            *flag = true;
        }
    }
    f.fillet_round_as_definition = true;
    Kernel {
        fixes: f,
        ..Kernel::default()
    }
}

/// Runs `body` on a thread with a 256 MB stack (the debug build's round needs it).
fn with_big_stack(body: fn()) {
    std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(body)
        .unwrap()
        .join()
        .unwrap();
}

/// The tilted loft: its bottom circle edge (both vertices at z = 0) is the round's edge.
fn tilted_loft(k: &mut Kernel) -> (Brep, Vec<EdgeId>) {
    let a = Curve::from(create_circle(Point3::new(0.0, 0.0, 0.0), 4.0).unwrap());
    let b = Curve::from(create_circle(Point3::new(0.25, 0.0, 10.0), 4.0).unwrap());
    let mut brep = create_skin_primitive(k, &[1, 1], &[Some(&a), Some(&b)], 1, true)
        .unwrap()
        .brep;
    brep.refresh_cached_topology();
    assert!(is_manifold_solid(&brep).unwrap());
    let bottom: Vec<EdgeId> = brep
        .edges()
        .ids()
        .filter(|&e| {
            let Some(v0) = edge::vertex(&brep, e).unwrap() else {
                return false;
            };
            let v1 = edge::other_vertex(&brep, e, Some(v0))
                .unwrap()
                .unwrap_or(v0);
            [v0, v1]
                .iter()
                .all(|&v| vertex::point(&brep, v).unwrap().z.abs() < 1e-9)
        })
        .collect();
    assert!(!bottom.is_empty());
    (brep, bottom)
}

/// The volume to a relative accuracy of 1e-6: ample for the 1e-3 comparison below, and the
/// definition's volume takes two minutes at 1e-11.
fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

/// The round of radius 1.5 on the loft's bottom edge with `budget` as the kernel's certify
/// budget: the volume it removes, the spans its certificate certified, the face, edge and
/// vertex counts after it, and the part.
fn round(budget: Option<u32>) -> (f64, u64, (usize, usize, usize), Brep) {
    let mut k = kernel();
    let (mut b, bottom) = tilted_loft(&mut k);
    let before = volume(&k, &mut b);
    k.fillet_certify_budget.set(budget);
    k.work_counters.reset();
    fillet_edges(&mut k, &mut b, &bottom, 1.5, 1, 1, 1.0).unwrap();
    let spans = k.work_counters.certify_spans();
    b.refresh_cached_topology();
    let after = volume(&k, &mut b);
    let counts = (
        b.faces().ids().count(),
        b.edges().ids().count(),
        b.vertices().ids().count(),
    );
    (before - after, spans, counts, b)
}

fn check_clean(b: &mut Brep) {
    let mut k = kernel();
    let lines = check_brep(&mut k, b, CheckLevel::Level2).unwrap().lines();
    assert!(lines.is_empty(), "{lines:?}");
    assert!(is_manifold_solid(b).unwrap());
}

/// Budget `None` (unbounded, as before): the round certifies past the budget the other test
/// uses, and gives a clean manifold solid.
#[test]
fn budget_none_is_unchanged() {
    with_big_stack(|| {
        let (_, spans, _, mut b) = round(None);
        assert!(
            spans > 8,
            "certified {spans} spans, want more than the budget of 8"
        );
        check_clean(&mut b);
    });
}

/// Budget `Some(8)`: the round is over budget, so it is built as its approximation. The
/// fillet still succeeds with the same topology as the unbounded round, the volume within
/// 1e-3 of it, the certificate's spans at most 8, and no face a procedural blend.
#[test]
fn a_round_over_budget_is_built_as_its_approximation() {
    with_big_stack(|| {
        let (removed, _, counts, _) = round(None);
        let (approx_removed, spans, approx_counts, mut approx) = round(Some(8));
        assert!(spans <= 8, "certified {spans} spans over a budget of 8");
        check_clean(&mut approx);
        assert_eq!(approx_counts, counts, "face, edge and vertex counts");
        assert!(
            (approx_removed - removed).abs() <= 1e-3 * removed,
            "approximation removed {approx_removed}, the definition {removed}"
        );
        for f in approx.faces().ids().collect::<Vec<_>>() {
            let s = approx.face(f).unwrap().surface.as_ref().unwrap();
            assert!(
                s.procedural_blend().is_none(),
                "face {f:?} is a procedural blend"
            );
        }
    });
}
