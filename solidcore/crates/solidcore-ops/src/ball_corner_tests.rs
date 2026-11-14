//! tangency-labs/solidcraft#15140: a ball corner of three equal rounds where fewer than two of
//! the faces are planes. Its centre has no closed form, so `blend::blend_corner::ball`'s
//! certified root proves it (`ball_certified_root`, the fork's `sm_BallCertifiedRoot`, its
//! Krawczyk test through the crate's one operator since #15502).
//!
//! Two cylinders, radius 5 and height 10, stand 6 apart on a 40 x 40 x 5 plate, and are
//! united with it. Their two base circles' outer arcs and the two lines where they meet are
//! rounded r 1, all concave. At each foot of a line, the two arcs' rounds and the line's round
//! meet in a corner of one plane and two cylinders.
//!
//! Three spheres, radius 5, centres 6 apart in a triangle, are united. Their three circles
//! meet at two points, top and bottom. Every piece of the circles (not the spheres' seams,
//! across which a sphere meets itself) is rounded r 1, all concave. At each of the two points
//! three rounds meet in a corner of three spheres and no plane, which only the certified root
//! solves.
//!
//! The checks, for each body:
//! - The body is valid.
//! - The ball solve is reached. The session kernel's work counter
//!   ([`solidcore::kernel::WorkCounters::ball_certified_roots`]) grows across the blend. A
//!   panic in another thread's call makes a fresh session kernel, which would zero the count
//!   in between; nextest runs each test in its own process, so none can.
//! - Its volume estimate lies in its certified enclosure (`mass::enclosure::enclose_volume`,
//!   asked for at most 1e-6 V wide for the cylinders, 1e-5 V for the spheres). The corner
//!   has no closed form to check it against. The enclosure narrows to the width asked unless
//!   its cell budget runs out, which is asserted not to happen (`cells < CELL_BUDGET`). So the
//!   width met is the one asked, and V lies inside it. The cylinders certify 1e-6 V in 2224
//!   cells (15140-corner3b). At 1e-6 V the spheres run out at 131073 cells, mostly strips at
//!   the gaps between faces: the sharp body at 1.05e-6 V, the rounded one at 5.57e-6 V. At
//!   1e-5 V they need 14761 and 22474 cells (measured in 15140-sphencl).
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::disallowed_methods
)] // test (it measures its fixture, as noisy_round_tests)

use solidcore::geom::vector3d::Point3;
use solidcore::kernel::Kernel;
use solidcore::mass::enclosure::{enclose_volume, CELL_BUDGET};
use solidcore::query::api_queries::edge_closest_point;

use crate::blend::{blend, Blend, Corners};
use crate::boolean::union_all;
use crate::session::{self, measure_body};
use crate::{make_box, make_cylinder, Body, CheckLevel};

/// The index of the edge of `body` through `p`, in its edge list (the order [`blend`]
/// reads without a record [`crate::Order`]).
fn edge_at(body: &Body, p: [f64; 3]) -> usize {
    let k = Kernel::default();
    let b = body.lock();
    let p = Point3::new(p[0], p[1], p[2]);
    b.edge_list()
        .iter()
        .position(|&e| edge_closest_point(&k, &b, e, p).is_ok_and(|c| c.closest.distance < 1e-9))
        .unwrap()
}

/// Rounds `edges` of `body` r 1 and checks the result (see the [module](self)): valid, the
/// ball solve reached, and its volume, larger than before, inside its certified enclosure
/// asked `rel_width` V wide, met inside the cell budget.
fn rounds_through_certified_ball_corners(body: &Body, edges: &[usize], rel_width: f64) {
    let sharp = body.volume(1e-8).unwrap();

    let before = session::work_counters(solidcore::kernel::WorkCounters::ball_certified_roots);
    blend(body, Blend::Round, edges, &[1.0], &[], Corners::Filled).unwrap();
    let after = session::work_counters(solidcore::kernel::WorkCounters::ball_certified_roots);
    assert!(
        after > before,
        "no ball corner's centre was solved ({before} -> {after})"
    );

    assert!(body.check(CheckLevel::All).unwrap().is_valid());
    let v = body.volume(1e-8).unwrap();
    // the rounds are concave: they add material
    assert!(v > sharp, "{sharp} -> {v}");
    let width = rel_width * v;
    let e = measure_body("enclose_volume", body, |k, b| {
        Ok(Some(enclose_volume(k, b, width)?))
    })
    .unwrap()
    .unwrap();
    // the width asked was met, not budgeted out
    assert!(
        e.counts.cells < CELL_BUDGET,
        "the enclosure ran out of cells ({} of {CELL_BUDGET})",
        e.counts.cells
    );
    assert!(
        e.width() <= width,
        "[{}, {}] is wider than {width}",
        e.lo,
        e.hi
    );
    assert!(
        e.lo <= v && v <= e.hi,
        "{v} is outside [{}, {}]",
        e.lo,
        e.hi
    );
}

#[test]
fn two_cylinders_on_a_plate_round_through_a_certified_ball_corner() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let plate = make_box([-20.0, -20.0, -5.0], [40.0, 40.0, 5.0]).unwrap();
    let a = make_cylinder([0.0, 0.0, 0.0], 5.0, 10.0).unwrap();
    let c = make_cylinder([6.0, 0.0, 0.0], 5.0, 10.0).unwrap();
    let body = union_all(vec![plate, a, c]).unwrap();
    // the base circles' outer arcs (through (-5, 0, 0) and (11, 0, 0)) and the lines x = 3,
    // y = +-4 where the cylinders meet
    let edges: Vec<usize> = [
        [-5.0, 0.0, 0.0],
        [11.0, 0.0, 0.0],
        [3.0, 4.0, 5.0],
        [3.0, -4.0, 5.0],
    ]
    .iter()
    .map(|&p| edge_at(&body, p))
    .collect();
    rounds_through_certified_ball_corners(&body, &edges, 1e-6);
}

#[test]
fn three_spheres_round_through_two_certified_ball_corners() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let h = 3.0 * 3.0_f64.sqrt();
    let centres = [[0.0, 0.0, 0.0], [6.0, 0.0, 0.0], [3.0, h, 0.0]];
    let body = union_all(
        centres
            .iter()
            .map(|&c| crate::make_sphere(c, 5.0).unwrap())
            .collect(),
    )
    .unwrap();
    // the circles' pieces: the edges whose middle lies on two spheres (a seam lies on one)
    let edges: Vec<usize> = {
        let k = Kernel::default();
        let b = body.lock();
        b.edge_list()
            .iter()
            .enumerate()
            .filter(|&(_, &e)| {
                let c = solidcore::topology::edge::curve(&b, e).unwrap().unwrap();
                let ivl = solidcore::topology::edge::interval(&b, e).unwrap();
                let m = c.evaluate_point(&k, ivl.evaluate(0.5)).unwrap();
                let on = centres
                    .iter()
                    .filter(|c| ((m - Point3::new(c[0], c[1], c[2])).length() - 5.0).abs() < 1e-7)
                    .count();
                on == 2
            })
            .map(|(n, _)| n)
            .collect()
    };
    // two pieces of each circle, and a third where sphere 0's seam crosses its circle with
    // sphere 1 (measured: 15140-sph3)
    assert_eq!(edges.len(), 7, "{edges:?}");
    rounds_through_certified_ball_corners(&body, &edges, 1e-5);
}
