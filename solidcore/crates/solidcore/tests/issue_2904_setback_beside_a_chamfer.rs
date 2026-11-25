//! tangency-labs/solidcore#2904: SolidCraft's `34-chamfer-then-fillet`, a 40 x 30 x 20 box
//! with its four upright edges chamfered 5, then its eight top edges rounded r 2 and one
//! upright side/chamfer edge r 0.327 in one fillet (as SolidCraft blends a later round
//! together with the rounds it meets). At the top of that edge three rounds of unequal radii
//! meet, so the corner is a Setback, filled G1 between the front, the chamfer and the top.
//!
//! The fill lies on the material's side of its three planes, but the control net of the
//! small round's setback patch stands 2.49e-4 past the front and the chamfer, more than the
//! corner's tolerance 4e-5: bounded by that net alone the fill was refused ("the setback
//! corner's fill stands 0.000249035 past a face it meets"). Its distance from each plane is
//! bounded on the patch itself, halving it where its net passes the tolerance, and the round
//! builds.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test helpers

use solidcore::blend::blend_corner::BlendCornerEvent;
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{Brep, EdgeId};

/// Every opt-in fix on.
fn kernel() -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes = UpstreamFixes::all();
    k
}

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

/// The octagon's eight top edges.
const TOP: [[f64; 3]; 8] = [
    [2.5, 2.5, 20.0],
    [20.0, 0.0, 20.0],
    [37.5, 2.5, 20.0],
    [40.0, 15.0, 20.0],
    [37.5, 27.5, 20.0],
    [20.0, 30.0, 20.0],
    [2.5, 27.5, 20.0],
    [0.0, 15.0, 20.0],
];

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

/// The box chamfered 5 on its four upright edges, its top rounded r 2 in one fillet, with
/// the side/chamfer edge at (35, 0) rounded `small` in the same fillet when given: the event
/// at the corner (35, 0, 20), the volume (before `check_brep`), the face count.
fn rounded(small: Option<f64>) -> (Option<BlendCornerEvent>, f64, usize) {
    let mut k = kernel();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
    let upright: Vec<EdgeId> = [
        [0.0, 30.0, 10.0],
        [40.0, 30.0, 10.0],
        [40.0, 0.0, 10.0],
        [0.0, 0.0, 10.0],
    ]
    .iter()
    .map(|p| pick(&k, &b, *p))
    .collect();
    fillet_edges(&mut k, &mut b, &upright, 5.0, 0, 1, 1.0).unwrap();
    let mut es: Vec<EdgeId> = TOP.iter().map(|p| pick(&k, &b, *p)).collect();
    let mut radii = vec![2.0; TOP.len()];
    if let Some(r) = small {
        es.push(pick(&k, &b, [35.0, 0.0, 9.0]));
        radii.push(r);
    }
    let corner = Point3::new(35.0, 0.0, 20.0);
    let event = {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_per_edge_constant_radius(
            &mut k,
            &es,
            &radii,
            &vec![FilletSurfaceGeneratorType::Circular; es.len()],
            1,
            1.0,
        )
        .unwrap();
        s.create_corners(&k, None).unwrap();
        let mut event = None;
        for c in s.fillet_corners().unwrap() {
            let fc = s.corner(c).unwrap();
            let at = fc.vertex.map(|v| s.target().vertex(v).unwrap().point);
            if at.is_some_and(|p| p.distance(corner) < 1e-9) {
                event = fc.blend.as_ref().map(|b| b.event);
            }
        }
        s.do_filleting(&mut k, None).unwrap();
        event
    };
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    b.refresh_cached_topology();
    let v = volume(&k, &mut b);
    assert!(check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .is_valid());
    (event, v, b.faces().ids().count())
}

/// The issue's round builds: the corner is the Setback (three setback patches and three
/// interior parts, so 10 + 8 + 1 + 6 = 25 faces), valid at level 2. It takes material off the
/// top-rounded box: at least the small round's own section along the part of its edge the
/// setbacks leave it, r^2 (tan 22.5 deg - pi/8) over 20 less its setback of 2.26286 less the
/// top round's 2 (the 135 degree wedge's closed form). The volume is solidcore's own
/// construction, pinned as built.
#[test]
fn a_small_round_up_a_chamfer_into_a_rounded_top_is_set_back() {
    let (_, top_only, _) = rounded(None);
    let (event, v, faces) = rounded(Some(0.327));
    assert_eq!(event, Some(BlendCornerEvent::Setback));
    assert_eq!(faces, 25);
    let r = 0.327_f64;
    let section = r * r * ((std::f64::consts::PI / 8.0).tan() - std::f64::consts::PI / 8.0);
    let off = top_only - v;
    assert!(
        off > section * (20.0 - 2.262_862_25 - 2.0),
        "took {off}, the round's own section along its edge {}",
        section * (20.0 - 2.262_862_25 - 2.0)
    );
    let want = 22_891.747_547_942;
    assert!(
        (v - want).abs() <= 1e-9 * want,
        "volume {v:.9}, want {want:.9}"
    );
}
