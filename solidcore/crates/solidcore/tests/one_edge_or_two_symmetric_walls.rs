//! "One edge or two" (docs/booleans.md) must not depend on the order of the merges: a 2 x 3
//! plane sheet of tolerance 1e-6 shelled by exactly two tolerances (-2e-6) has four side
//! walls 2e-6 wide, their top and bottom edges 2.00000000006e-6 apart (the rounding of
//! 1 - 0.999998), merged one wall after the other. The proof that a top edge is the bottom
//! face's edge read the edge's tolerance as the merges before had grown it (a vertex taking
//! in the other wall's corner grows its edges): the wall along y = 0, merged first, kept
//! two edges (1e-6 + 1e-6 do not reach), the wall along y = 3 lost one (its edge had grown
//! to 1.5e-6), and the result had 7 faces and 13 edges. The proof now reads an edge's
//! tolerance at most the B-rep's, the same for every edge: the four walls are decided alike,
//! 8 faces, 14 edges, 4 vertices (the goldens' `offset.offset_executive_p2` case
//! `shell_boundary_negative_exactly_two_tol`, as upstream).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::{original_key, Notifications};
use solidcore::geom::attribute::AttributeStore;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::tol;
use solidcore::math::{Point2, Point3, Vec3};
use solidcore::offset::offset_executive::p1::OffsetExecutive;
use solidcore::offset::offset_executive::p2::{OffsetOptions, ShellOptions};
use solidcore::surface::plane::p1::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p8::{create_face_from_surface, FaceBuildEnv};
use solidcore::topology::edge::NotifyEnv;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::topology_traverser::Marks;
use solidcore::{Brep, EntityRef, Kernel};

/// The plane z = 1 over [0, 2] x [0, 3], one face of tolerance 1e-6.
fn plane_sheet(k: &mut Kernel) -> Brep {
    let mut b =
        solidcore::topology::brep::p3::new_brep(k, tol::USE_DEFAULT, tol::USE_DEFAULT).unwrap();
    b.set_tolerance(1e-6);
    let domain = Extent2d::from_bounds(0.0, 0.0, 2.0, 3.0);
    let surface = Surface::from(Plane::new(
        Point3::new(0.0, 0.0, 1.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Point2::new(1.0, 1.0),
        &domain,
    ));
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let key_of = original_key;
    let mut env = NotifyEnv {
        records: &mut records,
        attributes: &mut attributes,
        context: None,
        key_of: &key_of,
    };
    let mut marks = Marks::default();
    let this_face = |f| original_key(EntityRef::Face(f));
    let mut fenv = FaceBuildEnv {
        env: &mut env,
        marks: &mut marks,
        this_face: &this_face,
    };
    create_face_from_surface(k, &mut b, Some(surface), &domain, &mut fenv).unwrap();
    b
}

/// The edges whose two ends lie on the line `coord(p) = value` (within 1e-5).
fn edges_along(b: &Brep, coord: fn(Point3) -> f64, value: f64) -> usize {
    b.edges()
        .ids()
        .filter(|&e| {
            let (Some(s), Some(t)) = solidcore::topology::edge::start_end_vertices(b, e).unwrap()
            else {
                return false;
            };
            [s, t].iter().all(|&v| {
                let p = b.vertex(v).unwrap().point;
                (coord(p) - value).abs() < 1e-5
            })
        })
        .count()
}

#[test]
#[ignore = "since upstream retired `brep_edit_statuses_pass_on` (#16956, WR-658) the shell is refused (1001, a relate conflict between the wall edges, then the 2e-6 wall's plane trim) as on main (golden `shell_boundary_negative_exactly_two_tol`); re-enable once the shell succeeds again"]
fn walls_two_tolerances_wide_are_decided_alike() {
    let mut k = Kernel::default();
    let mut sheet = plane_sheet(&mut k);
    let mut notifications = Notifications::new(&mut sheet);
    let shell = OffsetExecutive::shell_brep(
        &mut k,
        sheet,
        ShellOptions {
            offset: OffsetOptions {
                distance: -2e-6,
                extend_convex_edges: true,
                merge_results: true,
            },
            create_offset_solid: true,
            merge_sheet_side_walls: false,
        },
        &[],
        &mut notifications.context(),
    )
    .result
    .unwrap();
    let along = |coord: fn(Point3) -> f64, value| edges_along(&shell, coord, value);
    let (y0, y3) = (along(|p| p.y, 0.0), along(|p| p.y, 3.0));
    let (x0, x2) = (along(|p| p.x, 0.0), along(|p| p.x, 2.0));
    assert_eq!(y0, y3, "walls along y = 0 and y = 3: {y0} and {y3} edges");
    assert_eq!(x0, x2, "walls along x = 0 and x = 2: {x0} and {x2} edges");
    let counts = (
        shell.face_list().len(),
        shell.edges().ids().count(),
        shell.vertices().ids().count(),
    );
    assert_eq!(counts, (8, 14, 4), "faces, edges, vertices");
}
