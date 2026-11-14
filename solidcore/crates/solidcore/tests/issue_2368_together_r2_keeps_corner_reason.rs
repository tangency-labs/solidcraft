//! SolidCraft's `report-drafted-peg-front-and-concave-rounds-together-r2` (#2368's PR rig):
//! the drafted peg (a bar 40 x 16 whose far end is a half disc R8 about (40, 8), extruded
//! 10 with a 5 degree inward taper, its top's end circle extruded 10 more up with the same
//! taper and joined), its front top edge and the top's concave arc, which meet tangentially,
//! rounded r 2 in one fillet. The input is the body SolidCraft passed to the fillet
//! (`data/issue_2368_solidcraft_together_r2.smb`, the rig's dump of that call).
//!
//! The corner at the arc's vertex refuses itself in `MakeCornerTopology`, noting
//! `SM_FILERR_NO_ROOM`: the blends continue each other across more than one edge on a side.
//! The fork keeps a corner's noted `SM_FILERR_NO_ROOM` (or a `SM_FILERR_CORNER_UNSUPPORTED`
//! it noted) where `CreateFilletBrep` and `GetPreviewFilletSurfaces` call `MakeCornerTopology`
//! (`SmFilletExecutive.cpp` 3755-3759 and 1216-1219 at smlib#124, 63c7962), as it does after
//! `CalculateCornerEdges`. The port kept it only there, and returned
//! `SM_FILERR_VERTEX_PROBLEM` (3), "Unable to Make Topology". It then returned the fork's
//! refusal: `SM_FILERR_NO_ROOM` (8), fit 0.
//!
//! Since #2953 the Mixed corner takes this vertex's four edges, so the round builds.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::geom::attribute::AttributeStore;
use solidcore::kernel::{Kernel, UpstreamFixes};
use solidcore::math::Point3;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::api_files::read_brep_from_file;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{loopuse, EdgeId};
use solidcore::Brep;

const INPUT: &[u8] = include_bytes!("data/issue_2368_solidcraft_together_r2.smb");

/// SolidCraft's rig set: every fix on but the one it holds back
/// (`fillet_refuse_rounds_breaking_through`).
fn kernel() -> Kernel {
    let mut k = Kernel {
        fixes: UpstreamFixes::all(),
        ..Kernel::default()
    };
    k.fixes.fillet_refuse_rounds_breaking_through = false;
    k
}

/// A SolidCraft file, read under the notification naming as SolidCraft reads it.
fn read(k: &mut Kernel, bytes: &[u8]) -> Brep {
    let mut attributes = AttributeStore::default();
    let mut records = NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &loopuse::notify_ref,
        records: &mut records,
        next_key: 1 << 50,
    };
    let mut b = read_brep_from_file(k, Some(bytes.to_vec()), false, false, &mut env).unwrap();
    b.refresh_cached_topology();
    b
}

/// The edge nearest `p`.
fn pick(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
    let q = Point3::new(p[0], p[1], p[2]);
    let mut best = (f64::INFINITY, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance < best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.unwrap()
}

/// The two edges rounded r 2 together as SolidCraft drives the session: the kernel and the
/// body, or the error info's code, comment and fit on failure.
fn round() -> Result<(Kernel, Brep), (i32, String, Option<f64>)> {
    let mut k = kernel();
    let mut b = read(&mut k, INPUT);
    let edges = [
        pick(&k, &b, [20.0, 0.874_89, 10.0]),
        pick(&k, &b, [32.874_89, 8.0, 10.0]),
    ];
    let done = {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_per_edge_constant_radius(
            &mut k,
            &edges,
            &[2.0, 2.0],
            &[FilletSurfaceGeneratorType::Circular],
            1,
            1.0,
        )
        .unwrap();
        s.create_corners(&k, None).unwrap();
        s.do_filleting(&mut k, None).map_err(|_| {
            let info = s.error_info();
            (
                info.code,
                info.comment.clone(),
                s.fit_size().map(|f| f.size),
            )
        })
    };
    done.map(|()| (k, b))
}

/// The round builds one sound solid: the refusal this test pinned before #2953 held the body
/// to the fork's answer, so the body it now builds is held to being valid.
#[test]
fn the_mixed_corner_builds_the_round() {
    let (mut k, mut b) = round().unwrap();
    assert!(solidcore::topology::brep::p2::is_manifold_solid(&b).unwrap());
    let report = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    assert!(report.lines().is_empty(), "{report:?}");
}
