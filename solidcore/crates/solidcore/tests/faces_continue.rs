//! `query::faces_continue` (tangency-labs/solidcore#3138): two faces across an edge are proved
//! to continue one another only on identical surface definitions with the same outward side;
//! anything else is undecided, never a guess.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use solidcore::boolean::api_brep::boolean_difference;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::faces_continue::{faces_continue, Continues};
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::{edge, face, Brep, FaceId};
use solidcore::Kernel;

fn on_cylinder(b: &Brep, f: FaceId) -> bool {
    matches!(
        face::surface(b, f).unwrap().map(Surface::kind),
        Some(SurfaceKind::Cone(_))
    )
}

/// A plate drilled through: across every edge, the two faces continue one another exactly
/// when both are the hole's (its wall stored in pieces on one cylinder); a plane beside a
/// plane or the cylinder is undecided. Measured: how many hole edges are proved.
#[test]
fn a_hole_s_pieces_continue_and_nothing_else_does() {
    let mut k = Kernel {
        fixes: solidcore::kernel::UpstreamFixes::all(),
        ..Kernel::default()
    };
    let plate = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 30.0, 20.0, 6.0).unwrap();
    let hole = create_cylinder(&mut k, Point3::new(15.0, 10.0, -1.0), 3.0, 8.0).unwrap();
    let b = boolean_difference(&mut k, plate, hole).unwrap();
    let (mut proved, mut undecided) = (0, 0);
    for e in b.edge_list().to_vec() {
        let faces = edge::faces(&b, e, None).unwrap();
        let [f, g] = faces[..] else { continue };
        if f == g {
            continue;
        }
        let r = faces_continue(&b, f, g, e).unwrap();
        let both = on_cylinder(&b, f) && on_cylinder(&b, g);
        assert_eq!(r == Continues::Proved, both, "edge {e:?}: {r:?}");
        if both {
            proved += 1;
        } else {
            undecided += 1;
        }
    }
    eprintln!("faces_continue: {proved} hole edges proved, {undecided} edges undecided");
    assert!(undecided >= 12, "the plate's own edges and the hole's rims");
    // an edge that bounds neither face, and one face twice, are the caller's error
    let f = b.face_list()[0];
    assert!(faces_continue(&b, f, f, b.edge_list()[0]).is_err());
}
