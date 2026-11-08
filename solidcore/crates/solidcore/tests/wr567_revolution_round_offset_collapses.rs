//! WR-567 (the fork's tangency-labs/smlib@8b2090f, smlib#64; SolidCraft
//! `shell_of_a_disc_rounded_smaller_than_the_wall`, solidcraft#319): a disc r 15, 5 thick,
//! one rim rounded r 1.5 and shelled 1.6 or 2 inward, open at one end. The round is a
//! fillet's surface of revolution of an arc; moved in past its radius it has no offset,
//! as a torus's, and the floor's and side's offsets meet across it in a circle. Upstream's
//! `SmSurfOfRevolution` never collapses, and its offset builds the arc turned inside out
//! (r 0.1 or 0.5 round its centre): the shell fails with 1001 open at the rounded end's
//! other side, and is 0.85% and 4.2% low open at the rounded end.
//!
//! Expected: the cavity is the cylinder r 15 - t, 5 - t deep; the round takes
//! r^2 (1 - pi/4) from the disc's section, its centroid r (10 - 3 pi) / (3 (4 - pi)) in
//! from the rim, swept round the axis (Pappus).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use core::f64::consts::PI;

use offset_fixture::{planar_faces_at, report, shell};
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::kernel::UpstreamFixes;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::{Brep, Kernel};

const RD: f64 = 15.0;
const H: f64 = 5.0;
const R: f64 = 1.5;

/// The disc from z = -5 to 0, its circular rim at `edge_z` rounded r 1.5
/// (`SmApiFilletEdges`, circular cross-section).
fn rounded_disc(k: &mut Kernel, edge_z: f64) -> Brep {
    let mut b = create_cylinder(k, Point3::new(0.0, 0.0, -H), RD, H).unwrap();
    let edges: Vec<_> = b
        .edges()
        .iter()
        .filter(|(_, e)| {
            let c = e.curve.as_ref().unwrap();
            [e.interval.min(), e.interval.mid(), e.interval.max()]
                .iter()
                .all(|&t| (c.evaluate_point(k, t).unwrap().z - edge_z).abs() < 1e-9)
        })
        .map(|(id, _)| id)
        .collect();
    assert_eq!(edges.len(), 1, "the rim at z {edge_z}");
    fillet_edges(k, &mut b, &edges, R, 1, 1, 1.0).unwrap();
    b.refresh_cached_topology();
    b
}

/// The shell's volume: the disc less the round's corner less the cavity.
fn expected(t: f64) -> f64 {
    let corner =
        R * R * (1.0 - PI / 4.0) * 2.0 * PI * (RD - R * (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI)));
    PI * RD * RD * H - corner - PI * (RD - t) * (RD - t) * (H - t)
}

/// The rounds' mating across collapsed faces (`offset_collapsed_rounds_mated`), with or
/// without the revolution's collapse test.
fn kernel(revolution_collapses: bool) -> Kernel {
    let mut k = Kernel::default();
    let mut f = UpstreamFixes::default();
    f.offset_collapsed_rounds_mated = true;
    f.revolution_round_offset_collapses = revolution_collapses;
    k.fixes = f;
    k
}

/// Shells the disc rounded at `edge_z` by `t`, open at `open_z`; whether the result is
/// an intact manifold solid of the expected volume to 1e-6.
fn shelled(k: &mut Kernel, edge_z: f64, open_z: f64, t: f64) -> bool {
    let b = rounded_disc(k, edge_z);
    let open = planar_faces_at(k, &b, 2, open_z);
    assert_eq!(open.len(), 1, "the end face at z {open_z}");
    let result = shell(k, b, -t, &open);
    report(
        k,
        &format!("WR-567 disc rounded at z {edge_z}, open at z {open_z}, shelled {t}"),
        result,
        Some(expected(t)),
        1e-6,
    )
}

#[test]
fn wr567_default_fails_open_at_the_other_end() {
    // upstream: the round's offset is the arc turned inside out
    let mut k = kernel(false);
    assert!(!shelled(&mut k, -H, 0.0, 1.6));
}

#[test]
fn wr567_round_thinner_than_the_wall_collapses() {
    for (edge_z, open_z) in [(-H, 0.0), (-H, -H), (0.0, -H)] {
        for t in [1.6, 2.0] {
            let mut k = kernel(true);
            assert!(
                shelled(&mut k, edge_z, open_z, t),
                "rounded at z {edge_z}, open at z {open_z}, shelled {t}"
            );
        }
    }
}
