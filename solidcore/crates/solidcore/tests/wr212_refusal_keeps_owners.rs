//! `fixes.fillet_refuse_non_manifold_result` (WR-212) on a round it does not refuse: the
//! result is the flag-off result, curve owners included. The refusal's rollback copy
//! (`Brep::clone_for_rollback`) used to share the edge arenas copy-on-write, so the
//! round's first edge edit copied them for the caller's body with the copy
//! constructor, which resets every curve's owner. The first round of the WR-592 bar
//! (`wr592_torus_rail_on_axis.rs`) then failed `check_brep` with eight
//! `SmBrep::Test #61 ... Bad Edge fails pE->ValidateGeometry(FALSE) check [owner SmEdge]`
//! lines, while the flag-off round (and the pinned C++) checks clean.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, EntityRef, Kernel};

const PROFILE: [[f64; 2]; 4] = [
    [0.4893, 0.0579],
    [41.6497, 0.0579 + 3e-10],
    [41.6497 + 1e-10, 16.0579],
    [0.4893 + 2e-10, 16.0579 - 2e-10],
];

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

fn bar(k: &mut Kernel) -> Brep {
    let cs: Vec<Curve> = PROFILE
        .iter()
        .zip(PROFILE.iter().cycle().skip(1))
        .map(|(a, b)| {
            Curve::from(
                create_line_segment(Vec3::new(a[0], a[1], 0.0), Vec3::new(b[0], b[1], 0.0))
                    .unwrap(),
            )
        })
        .collect();
    let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
    create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 10.0, true).unwrap()
}

/// The WR-592 bar's first round (its front top edge, r 2) with the flag as `fix` says.
fn first_round(fix: bool) -> (Kernel, Brep) {
    let mut k = Kernel::default();
    k.fixes.fillet_refuse_non_manifold_result = fix;
    let mut b = bar(&mut k);
    let front_top = nearest_edge(&k, &b, Point3::new(20.0, 0.0579, 10.0));
    fillet_edges(&mut k, &mut b, &[front_top], 2.0, 1, 1, 1.0).unwrap();
    (k, b)
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

/// Per edge: whether its curve's owner is the edge's owner identity.
fn edge_owners(b: &Brep) -> Vec<(EdgeId, bool)> {
    b.edges()
        .ids()
        .map(|e| {
            let curve = edge::curve(b, e).unwrap().unwrap();
            (
                e,
                curve.owner() == Some(b.owner_identity(EntityRef::Edge(e))),
            )
        })
        .collect()
}

#[test]
fn wr212_flag_keeps_a_kept_round_as_without_it() {
    let (mut k_off, mut off) = first_round(false);
    let (mut k_on, mut on) = first_round(true);
    assert!(edge_owners(&off).iter().all(|&(_, owned)| owned));
    assert_eq!(edge_owners(&on), edge_owners(&off));
    assert_eq!(
        volume(&k_on, &mut on).to_bits(),
        volume(&k_off, &mut off).to_bits()
    );
    for (k, b) in [(&mut k_off, &mut off), (&mut k_on, &mut on)] {
        let lines = check_brep(k, b, CheckLevel::Level2).unwrap().lines();
        assert!(lines.is_empty(), "{lines:?}");
    }
}
