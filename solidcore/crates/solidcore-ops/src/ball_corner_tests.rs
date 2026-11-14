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
//! The checks:
//! - The body is valid.
//! - The ball solve is reached. The session kernel's work counter
//!   ([`solidcore::kernel::WorkCounters::ball_certified_roots`]) grows across the blend. A
//!   panic in another thread's call makes a fresh session kernel, which would zero the count
//!   in between; nextest runs each test in its own process, so none can.
//! - Its volume estimate lies in its certified enclosure (`mass::enclosure::enclose_volume`,
//!   asked for at most 1e-6 V wide). The corner has no closed form to check it against. The
//!   enclosure refines until it is as narrow as asked, so its width bound holds by
//!   construction (asserted only so that a run out of cells shows), and the check is that V
//!   lies inside it.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)] // test

use solidcore::geom::vector3d::Point3;
use solidcore::kernel::Kernel;
use solidcore::mass::enclosure::enclose_volume;
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
    let sharp = body.volume(1e-8).unwrap();

    let before = session::work_counters(solidcore::kernel::WorkCounters::ball_certified_roots);
    blend(&body, Blend::Round, &edges, &[1.0], &[], Corners::Filled).unwrap();
    let after = session::work_counters(solidcore::kernel::WorkCounters::ball_certified_roots);
    assert!(
        after > before,
        "no ball corner's centre was solved ({before} -> {after})"
    );

    assert!(body.check(CheckLevel::All).unwrap().is_valid());
    let v = body.volume(1e-8).unwrap();
    // the rounds are concave: they add material
    assert!(v > sharp, "{sharp} -> {v}");
    let width = 1e-6 * v;
    let e = measure_body("enclose_volume", &body, |k, b| {
        Ok(Some(enclose_volume(k, b, width)?))
    })
    .unwrap()
    .unwrap();
    // met by construction unless the cells run out: the check is the next one
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
