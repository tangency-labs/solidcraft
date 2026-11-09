//! WR-302 (`fillet_refuse_rails_leaving_faces`) and the fork's tangency-labs/smlib@d6a6d53:
//! a box's top edge ends on a boss's wall rising from the box. Its rail on the top crosses
//! the boss's base arc just past the edge's end and comes back onto the top beyond the
//! boss. The fork judged only the first and last off-face runs past an end, so it built
//! this blend as a cliff overflow and refused it. The port already skips a middle run
//! that leaves and comes back past an end (#1580). That rule is looser than d6a6d53's,
//! which also wants both crossings on the same end's side edge, and it keeps #1580's spur,
//! which returns across another edge. So this case builds with or without the fork's flags,
//! and both bodies are the same.
//!
//! The 40 x 20 x 10 box united with a cylinder of radius 5 and height 20 on (30, 0):
//! the front top edge runs (0, 0, 10) to (25, 0, 10), and a chamfer or round of 2 has
//! its top rail at y = 2, crossing the arc at x = 30 -+ sqrt(21).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::topology::edge;
use solidcore::{Brep, Kernel};

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

/// The material removed by a blend of 2 (`xsect` 0 chamfer, 1 round) on the front top
/// edge, with the fork's refusal and cliff-overflow flags or without.
fn removed(fork_flags: bool, xsect: u32) -> f64 {
    let mut k = Kernel::default();
    k.fixes.fillet_refuse_rails_leaving_faces = fork_flags;
    k.fixes.fillet_cliff_overflow = fork_flags;
    k.fixes.fillet_trace_reaches_rails = fork_flags;
    k.fixes.fillet_rail_end_on_continued_surface = fork_flags;
    let bx = create_box(&mut k, Vec3::ZERO, 40.0, 20.0, 10.0).unwrap();
    let boss = create_cylinder(&mut k, Point3::new(30.0, 0.0, 0.0), 5.0, 20.0).unwrap();
    let mut b = boolean_union(&mut k, bx, boss).unwrap();
    let before = volume(&k, &mut b);
    let front_top = |p: Point3| p.y.abs() < 1e-6 && (p.z - 10.0).abs() < 1e-6;
    let e = b
        .edge_list()
        .iter()
        .copied()
        .find(|&e| {
            let c = edge::curve(&b, e).unwrap().unwrap();
            let ivl = edge::interval(&b, e).unwrap();
            let s = c.evaluate_point(&k, ivl.min()).unwrap();
            let t = c.evaluate_point(&k, ivl.max()).unwrap();
            front_top(s) && front_top(t) && (s.x.abs() < 1e-6 || t.x.abs() < 1e-6)
        })
        .expect("front top edge");
    {
        let mut session = FilletSession::new(&mut k, &mut b).unwrap();
        session
            .prepare_constant_radius_edges(&mut k, &[e], 2.0, xsect, 1, 1.0)
            .unwrap();
        session.do_fillet(&mut k).unwrap();
    }
    before - volume(&k, &mut b)
}

#[test]
fn rail_back_on_its_face_beyond_the_boss_is_not_refused() {
    for xsect in [0, 1] {
        let fork = removed(true, xsect);
        let upstream = removed(false, xsect);
        // the blend along the 25 long edge: 2 x 2 / 2 (chamfer), (1 - pi/4) 2^2 (round)
        let section = if xsect == 0 {
            2.0
        } else {
            (1.0 - core::f64::consts::FRAC_PI_4) * 4.0
        };
        assert!(
            (fork - upstream).abs() <= 1e-9 * upstream && fork > 25.0 * section,
            "xsect {xsect}: fork flags {fork}, upstream {upstream}"
        );
    }
}
