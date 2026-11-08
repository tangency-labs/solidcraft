//! tangency-labs/solidcore#1795 under `Kernel::fixes.offset_extended_edges_mated` (WR-344):
//! the #1528 drilled plate shelled 2 inward, open at its top (z = 27.5), as SolidCraft's
//! `smlib_shell` calls it (extended convex edges, self-intersections handled, the offset
//! solid made). `EdgeuseOffset` cannot mate the offsets of the closed hole/arc edge (inset,
//! the hole's tube r 7 passes out through the arc's offset r 8, so `MateEdges` finds no
//! trimmable curve); WR-344 as the fork has it (tangency-labs/smlib@85bbfaf) still takes
//! that pair as mated, so the merge never intersects it and the shell fails with `SM_ERR`
//! (1001) in `make_faceuse`. The refinement leaves an edge whose mate failed to the merge,
//! as upstream does: the shell closes, as it does with the default fixes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::{planar_faces_at, report, Notifications};
use solidcore::boolean::api_brep as boolean;
use solidcore::curve::curve::Curve;
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::offset::offset_executive::p1::OffsetExecutive;
use solidcore::offset::offset_executive::p2::{OffsetOptions, ShellOptions};
use solidcore::primitive::api_curves as curves;
use solidcore::primitive::api_primitives as primitives;
use solidcore::sweep::api_brep as sweeps;
use solidcore::topology::proto_topology::p1::api_transform::{api_rotate, TransformObject};
use solidcore::{Brep, Kernel};

/// The #1528 operand (as `issue_1528_write_operand.rs` builds it).
fn drilled(k: &mut Kernel) -> Brep {
    let p = |x: f64, y: f64| Vec3::new(x, y, 0.0);
    let cs: Vec<Curve> = vec![
        Curve::from(curves::create_line_segment(p(0.0, 0.0), p(42.5, 0.0)).unwrap()),
        Curve::from(curves::create_line_segment(p(42.5, 0.0), p(42.5, 20.0)).unwrap()),
        Curve::from(curves::create_line_segment(p(42.5, 20.0), p(0.0, 20.0)).unwrap()),
        Curve::from(curves::create_arc(p(0.0, 10.0), 10.0, 90.0, 270.0).unwrap()),
    ];
    let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
    let body = sweeps::create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 27.5, true).unwrap();
    let mut drill =
        primitives::create_cylinder(k, Point3::new(-10.0, 12.5, 12.5), 5.0, 17.5).unwrap();
    api_rotate(
        k,
        Some(TransformObject::Brep(&mut drill)),
        Point3::new(-10.0, 12.5, 12.5),
        Vec3::new(0.0, 1.0, 0.0),
        90.0,
    )
    .unwrap();
    let mut b = boolean::boolean_difference(k, body, drill).unwrap();
    b.refresh_cached_topology();
    b
}

#[test]
fn shell_with_an_unmated_edge_closes_under_wr344() {
    let mut k = Kernel::default();
    k.fixes.offset_extended_edges_mated = true;
    let mut b = drilled(&mut k);
    let open = planar_faces_at(&k, &b, 2, 27.5);
    assert_eq!(open.len(), 1, "the top face");
    let mut notifications = Notifications::new(&mut b);
    let result = OffsetExecutive::shell_brep(
        &mut k,
        b,
        ShellOptions {
            offset: OffsetOptions {
                distance: -2.0,
                extend_convex_edges: true,
                merge_results: true,
            },
            create_offset_solid: true,
            merge_sheet_side_walls: false,
        },
        &open,
        &mut notifications.context(),
    )
    .result
    .map(|mut r| {
        r.refresh_cached_topology();
        r
    });
    // the default fixes' shell of this plate: 9655.150582612916 (the pinned C++ agrees)
    assert!(report(
        &k,
        "1795 WR-344 shell 2 open at z 27.5",
        result,
        Some(9655.150582612916),
        1e-6
    ));
}
