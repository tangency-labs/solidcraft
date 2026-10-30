//! tangency-labs/solidcore#422 (WR-212): an R4 round over a through hole returns success with
//! a body that is not a closed manifold solid (SolidCraft rig case fillets/324). The round
//! would have to be trimmed by the hole's wall, which SMLib does not do. The opt-in
//! `Kernel::fixes.fillet_refuse_non_manifold_result` refuses the result instead, as the
//! issue's "Expected" allows ("a refusal naming the edge that can't be blended"). The
//! fork's trim by the hole's wall (tangency-labs/smlib@3dbe54a) is not ported.
//!
//! The case is the rig's: a 40 x 20 x 10 block minus a through hole of radius 3 about
//! (20, 14.5), rounded R4 along its top long edge at y = 20. The round covers y = 16..20
//! and the hole reaches y = 17.5.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::{edge, vertex, EdgeId};
use solidcore::{Brep, Kernel, Result, SolidError};

/// The block with its hole, and its top long edge at y = 20.
fn block(k: &mut Kernel) -> (Brep, EdgeId) {
    let b = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 20.0, 10.0).unwrap();
    let hole = create_cylinder(k, Point3::new(20.0, 14.5, -1.0), 3.0, 12.0).unwrap();
    let b = boolean_difference(k, b, hole).unwrap();
    let top: Vec<EdgeId> = b
        .edges()
        .ids()
        .filter(|&e| {
            let (s, t) = edge::start_end_vertices(&b, e).unwrap();
            let (Some(s), Some(t)) = (s, t) else {
                return false;
            };
            let (p, q) = (vertex::point(&b, s).unwrap(), vertex::point(&b, t).unwrap());
            [p, q]
                .iter()
                .all(|v| (v.y - 20.0).abs() < 1e-9 && (v.z - 10.0).abs() < 1e-9)
                && (p.x - q.x).abs() > 39.0
        })
        .collect();
    assert_eq!(top.len(), 1);
    (b, top[0])
}

/// The round of radius `r` with the fix as `fix` says: its status and whether the block is
/// a closed manifold solid after it.
fn round(fix: bool, r: f64) -> (Result<()>, bool) {
    let mut k = Kernel::default();
    k.fixes.fillet_refuse_non_manifold_result = fix;
    let (mut b, top) = block(&mut k);
    let result = fillet_edges(&mut k, &mut b, &[top], r, 1, 1, 1.0);
    let solid = is_manifold_solid(&b).unwrap();
    (result, solid)
}

#[test]
fn issue_422_flag_is_off_by_default() {
    assert!(!Kernel::default().fixes.fillet_refuse_non_manifold_result);
}

/// Upstream: the R4 round reports success and leaves a body that is not a closed solid.
#[test]
fn issue_422_upstream_returns_an_open_body_as_success() {
    let (result, solid) = round(false, 4.0);
    result.unwrap();
    assert!(!solid);
}

/// Volume of `b`.
fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

/// Face, edge and vertex counts of `b`.
fn counts(b: &Brep) -> (usize, usize, usize) {
    (
        b.faces().ids().count(),
        b.edges().ids().count(),
        b.vertices().ids().count(),
    )
}

/// The debug dump of `b` without the parts a copy never carries: notification
/// identities (issued to the objects themselves) and surface cache slots (caches only
/// speed up queries).
fn dump(b: &Brep) -> String {
    let s = format!("{b:?}");
    let mut out = String::new();
    let mut rest = s.as_str();
    loop {
        let next = ["IdentitySlot(", "CacheSlot("]
            .iter()
            .filter_map(|key| rest.find(key).map(|at| (at, key.len())))
            .min();
        let Some((at, len)) = next else {
            out.push_str(rest);
            return out;
        };
        out.push_str(&rest[..at]);
        out.push('_');
        let mut depth = 1;
        let mut end = at + len;
        for (i, c) in rest[at + len..].char_indices() {
            depth += match c {
                '(' => 1,
                ')' => -1,
                _ => 0,
            };
            if depth == 0 {
                end = at + len + i + 1;
                break;
            }
        }
        rest = &rest[end..];
    }
}

/// With the fix the R4 round is refused with `SM_FILERR_BAD_MERGE` (1001), and the
/// caller's body is exactly as it was before the call: same counts, volume (to the
/// bit), closed manifold solid, and the same B-rep down to its debug dump.
#[test]
fn issue_422_round_into_the_hole_is_refused_with_the_fix() {
    let mut k = Kernel::default();
    k.fixes.fillet_refuse_non_manifold_result = true;
    let (mut b, top) = block(&mut k);
    let before_counts = counts(&b);
    // (the volume makes the missing UV trims, so the dump is taken after it)
    let before_volume = volume(&k, &mut b);
    assert!(is_manifold_solid(&b).unwrap());
    let before_dump = dump(&b);
    let result = fillet_edges(&mut k, &mut b, &[top], 4.0, 1, 1, 1.0);
    match result {
        Err(SolidError::Upstream {
            code: 1001,
            entity: None,
            ..
        }) => {}
        other => panic!("not refused: {other:?}"),
    }
    assert_eq!(counts(&b), before_counts);
    assert!(is_manifold_solid(&b).unwrap());
    assert!(
        dump(&b) == before_dump,
        "the body differs from before the call"
    );
    assert_eq!(volume(&k, &mut b).to_bits(), before_volume.to_bits());
    // The restored body still takes a round that fits.
    fillet_edges(&mut k, &mut b, &[top], 2.0, 1, 1, 1.0).unwrap();
    assert!(is_manifold_solid(&b).unwrap());
}

/// A round clear of the hole (R2, the hole's wall at y = 17.5) still builds a solid.
#[test]
fn issue_422_round_clear_of_the_hole_is_kept() {
    for fix in [false, true] {
        let (result, solid) = round(fix, 2.0);
        result.unwrap();
        assert!(solid, "fix {fix}");
    }
}
