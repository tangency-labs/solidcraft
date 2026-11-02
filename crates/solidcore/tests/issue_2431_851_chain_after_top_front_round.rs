//! Regression test for tangency-labs/solidcore#2431 on SolidCraft's own input
//! (`851-fillet-top-front-then-upright`, step 7): the body SolidCraft hands the blend after
//! the top front edge is rounded R2 (`data/issue_2431_851_before_front_upright_r2.smb`, the
//! rig's blend-call dump, sha256 3bb0c88f...), then the front upright at the arc R2 with the
//! tangent chain SolidCraft's propagation pulls in: the upright, the edge over the top front
//! round's end curve and the arc's top edge.
//!
//! The middle piece rolls on the top front round, a cylinder of the ball's radius: its offset
//! collapses onto the round's axis and the ball is fixed (#2401's `OffsetCollapses`). #2403
//! builds that piece as the ball's triangle between the sections the Joins on either side
//! place through the ball's centre, so an end of the edge where the chain runs on into
//! another blend closes the zone. #2431 at first judged those ends by the plane square to
//! the edge alone and refused the chain (naming 2 less the fit margin), which SolidCraft then
//! retried at that size, a trace of minutes. Now the chain builds at R2 and no size is noted.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    missing_docs
)]
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::api_files::read_brep_from_file;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p3::topology_ref;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{Brep, EdgeId};
use solidcore::{Kernel, Point3};

const RAD: f64 = 2.0;
const TIP: f64 = 62.0 + 1.0 / 6.0;
const WALL: f64 = 21.0;

/// The three edges rounded R2 together on the in-code part
/// (`issue_2403_chain_over_end_curve::three_edges_together_end_on_the_corner_ball`);
/// SolidCraft's grid gives 9524.342896 to 5e-3.
const THREE_TOGETHER: f64 = 9524.343664763;

/// Every fix but `geometric_g2_continuity`, with or without `fillet_blend_corner`.
fn kernel(blend: bool) -> Kernel {
    let mut k = Kernel::default();
    let mut f = UpstreamFixes::all();
    f.geometric_g2_continuity = false;
    f.fillet_blend_corner = blend;
    k.fixes = f;
    k
}

fn read(k: &mut Kernel) -> Brep {
    let bytes = include_bytes!("data/issue_2431_851_before_front_upright_r2.smb").to_vec();
    let mut attributes = AttributeStore::new();
    let mut records = NotifyRecords::new();
    let read_key = |e| topology_ref(1, e);
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &read_key,
        records: &mut records,
        next_key: 1 << 50,
    };
    let mut b = read_brep_from_file(k, Some(bytes), false, false, &mut env).unwrap();
    b.refresh_cached_topology();
    b
}

fn pick(k: &Kernel, b: &Brep, q: Point3) -> EdgeId {
    b.edges()
        .ids()
        .filter_map(|e| {
            edge_closest_point(k, b, e, q)
                .ok()
                .map(|c| (e, c.closest.distance))
        })
        .filter(|&(_, d)| d <= 0.05)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .expect("edge")
        .0
}

/// The upright, the edge over the top front round's end curve (at 45 degrees round the
/// round's axis) and the arc's top edge.
fn chain(k: &Kernel, b: &Brep) -> Vec<EdgeId> {
    let h = RAD / 2.0f64.sqrt();
    let (y, z) = (-19.0 - h, 8.0 + h);
    let over_end_curve = Point3::new(TIP - (WALL * WALL - (y + WALL) * (y + WALL)).sqrt(), y, z);
    [
        Point3::new(41.166666666666664, -21.0, 5.0),
        over_end_curve,
        Point3::new(47.3174, -6.1508, 10.0),
    ]
    .iter()
    .map(|&q| pick(k, b, q))
    .collect()
}

/// Rounds the chain R2 in one session: the session's error code and noted fit size.
fn round(k: &mut Kernel, b: &mut Brep) -> (bool, i32, Option<f64>) {
    let edges = chain(k, b);
    let xs = vec![FilletSurfaceGeneratorType::Circular; edges.len()];
    let radii = vec![RAD; edges.len()];
    let (ok, code, fit) = {
        let mut s = FilletSession::new(k, b).unwrap();
        s.prepare_per_edge_constant_radius(k, &edges, &radii, &xs, 1, 1.0)
            .unwrap();
        let ok = s.do_fillet(k).is_ok();
        (ok, s.error_info().code, s.fit_size().map(|f| f.size))
    };
    if ok {
        stitch_and_orient(k, b, &StitchAndOrientOptions::default()).unwrap();
        b.refresh_cached_topology();
    }
    (ok, code, fit)
}

/// The chain builds at R2 as #2403's corner, one clean solid of the three-together volume,
/// and the refusal sizing never runs: no fit is noted.
#[test]
fn the_chain_over_the_end_curve_builds_at_r2_with_no_size_search() {
    let mut k = kernel(true);
    let mut b = read(&mut k);
    let (ok, code, fit) = round(&mut k, &mut b);
    assert!(ok, "refused or failed: code {code}, fit {fit:?}");
    assert_eq!(fit, None, "a size was noted on success");
    assert_eq!(b.face_list().len(), 10);
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let v = brep_volume(&k, &mut b, 1e-8, &attrs).unwrap();
    assert!(
        (v - THREE_TOGETHER).abs() <= 1e-9 * THREE_TOGETHER,
        "{v} {THREE_TOGETHER}"
    );
}

/// Without `fillet_blend_corner` nothing builds the fixed ball's triangle, so the chain is
/// not built (the ends are judged by their planes alone, as #2401 does).
#[test]
fn without_the_blend_corner_the_chain_is_not_built() {
    let mut k = kernel(false);
    let mut b = read(&mut k);
    let (ok, _, _) = round(&mut k, &mut b);
    assert!(!ok);
}
