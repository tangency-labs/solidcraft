//! Regression test for WR-643 (the SolidCraft fork's tangency-labs/smlib@7224896, branch
//! `replay/1.0.1`; #960). With WR-610's run-on (`fillet_trace_reaches_rails`),
//! `RailEdgeuseIntersect` widens a straight side edge's interval past both of its ends.
//! Behind the corner's vertex, the side edge's line lies on the far side of the filleted
//! edge, where no rail of the blend can be. Once the faces continue as themselves (WR-633,
//! `offset_extension_as_itself`), the mirror image of the ball lies there.
//!
//! The part is WR-570's knife edge (`wr_570_knife_edge_rounds.rs`: a 62 1/6 x 21 plate whose
//! corner is cut by a circle of radius 21 tangent to the back face at the tip, extruded 10).
//! Its arc wall continued as itself past the tip carries the mirror of the true ball. The
//! back face's bottom edge, run on behind the knife's vertex, is met there (the fork's gdb:
//! t = -0.104 against the true +0.104), and the end rib then finds no intersection (1001).
//! With `fillet_run_on_past_far_end_only`, the line runs on past the far end only, and each
//! round has the closed-form volume (WR-570's), to 1e-9 relative.
//!
//! The port's solve finds the true ball from its first guess even over the interval that
//! reaches behind the vertex (the same [-1.85, 2.85] as the fork's at r 0.5), so this part
//! rounds with the flag off too. The test that fails without the change is the
//! interval's own, `wr643_side_edge_runs_on_past_its_far_end_only` in
//! `blend/fillet_solver/p2.rs`; this one checks that the cut-off run-on still builds every
//! round of the knife edge.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use std::f64::consts::PI;

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::sweep::api_brep::create_taper_extrude;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::edge;
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel};

const R: f64 = 21.0;
const TIP: f64 = 62.0 + 1.0 / 6.0;
const H: f64 = 10.0;

fn line(a: (f64, f64), b: (f64, f64)) -> Curve {
    Curve::from(create_line_segment(Vec3::new(a.0, a.1, 0.0), Vec3::new(b.0, b.1, 0.0)).unwrap())
}

/// The knife-edge part, its tip at x = `s * TIP` (`s` = 1, or -1 for the mirror image),
/// the profile chained head to tail.
fn part(k: &mut Kernel, s: f64) -> Brep {
    let curves = if s > 0.0 {
        vec![
            line((0.0, 0.0), (TIP, 0.0)),
            // (TIP, 0) to (TIP - R, -R)
            Curve::from(create_arc(Vec3::new(TIP, -R, 0.0), R, 90.0, 180.0).unwrap()),
            line((TIP - R, -R), (0.0, -R)),
            line((0.0, -R), (0.0, 0.0)),
        ]
    } else {
        vec![
            line((0.0, 0.0), (0.0, -R)),
            line((0.0, -R), (-TIP + R, -R)),
            // (-TIP + R, -R) to (-TIP, 0)
            Curve::from(create_arc(Vec3::new(-TIP, -R, 0.0), R, 0.0, 90.0).unwrap()),
            line((-TIP, 0.0), (0.0, 0.0)),
        ]
    };
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_taper_extrude(k, &refs, H, 0.0, 3, false).unwrap()
}

/// `SmApiBrepComputeVolume` at an integration accuracy well below the 1e-9 the volume
/// is checked to.
fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-11, &attrs).unwrap()
}

/// The knife edge: the vertical edge at (`s * TIP`, 0).
fn knife_edge(k: &Kernel, b: &Brep, s: f64) -> EdgeId {
    let at_tip = |p: Point3| (p.x - s * TIP).abs() < 1e-6 && p.y.abs() < 1e-6;
    b.edge_list()
        .iter()
        .copied()
        .find(|&e| {
            let curve = edge::curve(b, e).unwrap().unwrap();
            let ivl = edge::interval(b, e).unwrap();
            let p = curve.evaluate_point(k, ivl.min()).unwrap();
            let q = curve.evaluate_point(k, ivl.max()).unwrap();
            at_tip(p) && at_tip(q) && (p.z - q.z).abs() > H / 2.0
        })
        .expect("the knife edge")
}

/// The volume the round of radius `r` takes off.
fn removed(r: f64) -> f64 {
    let beta = ((R - r) / (R + r)).acos();
    let ox = TIP - (4.0 * R * r).sqrt();
    let quad = 0.5 * (TIP - ox) * (R + r);
    (quad - 0.5 * R * R * beta - 0.5 * r * r * (PI - beta)) * H
}

/// The knife edge of the part (mirrored when `s` is -1) rounded `r` with the knife edge,
/// the run-on and the analytic continuation on: its face count, volume and manifoldness,
/// or the fillet error code.
fn round(far_end_only: bool, s: f64, r: f64) -> Result<(usize, f64, bool), i32> {
    let mut k = Kernel::default();
    k.fixes.fillet_knife_edge_rounds = true;
    k.fixes.fillet_trace_reaches_rails = true;
    k.fixes.fillet_rail_end_on_continued_surface = true;
    k.fixes.fillet_refuse_rails_leaving_faces = true;
    k.fixes.fillet_cliff_overflow = true;
    k.fixes.offset_extension_from_base = true;
    k.fixes.offset_extension_as_itself = true;
    k.fixes.fillet_run_on_past_far_end_only = far_end_only;
    let mut b = part(&mut k, s);
    let e = knife_edge(&k, &b, s);
    {
        let mut session = FilletSession::new(&mut k, &mut b).unwrap();
        session
            .prepare_constant_radius_edges(&mut k, &[e], r, 1, 1, 1.0)
            .unwrap();
        session
            .do_fillet(&mut k)
            .map_err(|_| session.error_info().code)?;
    }
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    let faces = b.face_list().len();
    let manifold = is_manifold_solid(&b).unwrap();
    Ok((faces, volume(&k, &mut b), manifold))
}

/// The rounds of the knife edge (r 0.5, 2, 5, each mirrored) with the run-on behind the
/// corner cut off: 7 faces, a manifold solid, and WR-570's closed-form volume to 1e-9
/// relative, as the fork's test asserts them.
#[test]
fn wr643_knife_edge_rounds_with_the_run_on_past_the_far_end_only() {
    let before = (TIP - R) * R * H + R * R * (1.0 - PI / 4.0) * H;
    for r in [0.5, 2.0, 5.0] {
        for s in [1.0, -1.0] {
            let (faces, v, manifold) = round(true, s, r)
                .unwrap_or_else(|code| panic!("r {r}, side {s}: the round failed: {code}"));
            let want = before - removed(r);
            assert_eq!(faces, 7, "r {r}, side {s}: faces");
            assert!(manifold, "r {r}, side {s}: not a manifold solid");
            assert!(
                (v - want).abs() <= 1e-9 * want,
                "r {r}, side {s}: volume {v}, expected {want}"
            );
        }
    }
}
