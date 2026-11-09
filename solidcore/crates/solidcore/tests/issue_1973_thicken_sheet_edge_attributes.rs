//! tangency-labs/solidcore#1973: thickening a flat sheet (`shell_brep_simple` with
//! `create_offset_solid`) lost the attributes of the edges on the sheet's side. After
//! `MergeBrep` the sheet copy's attributes are in the caller's notification stores, but
//! the stitch that closes the solid glued on a store of its own, so where a glue kept
//! the side wall's edge and deleted the copy's, the copy's attribute was gone. The stitch
//! now notifies through the caller's stores, as upstream's objects carry their own: the
//! sheet's edges keep their attributes on the sheet side, as their offsets do on the
//! other, and the side walls' new uprights have none (the fork's C++ gives the same
//! counts).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{Attribute, Behavior};
use solidcore::math::{Point3, Vec3};
use solidcore::offset::offset_executive::p2::{shell_brep_simple, OffsetOptions, ShellOptions};
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::{Brep, EntityRef, Kernel};

const MARK: u64 = 20_000;

fn line(a: (f64, f64, f64), b: (f64, f64, f64)) -> Curve {
    Curve::from(
        create_line_segment(Point3::new(a.0, a.1, a.2), Point3::new(b.0, b.1, b.2)).unwrap(),
    )
}

/// The sheet swept from `curves` 10 along `along`, every edge marked, thickened 2: the
/// result's edge count and how many of its edges carry the mark.
fn thickened(curves: &[Curve], along: Vec3) -> (usize, usize) {
    let mut k = Kernel::default();
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let mut sheet: Brep = create_linear_sweep(&mut k, &refs, along, 10.0, false).unwrap();
    for e in sheet.edge_list().to_vec() {
        sheet
            .add_attribute(EntityRef::Edge(e), Attribute::long(MARK, 2, Behavior::Copy))
            .unwrap();
    }
    let options = ShellOptions {
        offset: OffsetOptions {
            distance: 2.0,
            extend_convex_edges: true,
            merge_results: true,
        },
        create_offset_solid: true,
        merge_sheet_side_walls: true,
    };
    let solid = shell_brep_simple(&mut k, &sheet, options, &[]).unwrap();
    let marked = solid
        .edge_list()
        .iter()
        .filter(|&&e| {
            solid
                .entity_attributes(EntityRef::Edge(e))
                .into_iter()
                .any(|(_, a)| a.attribute_id() == MARK)
        })
        .count();
    (solid.edge_list().len(), marked)
}

#[test]
fn issue_1973_a_thickened_flat_sheet_keeps_its_edges_marks_on_both_sides() {
    // a 20 x 10 sheet at z = 0: its 4 edges and their 4 offsets are marked, the 4 new
    // uprights are not (before the fix: 4, all at z = 2)
    let flat = [line((0.0, 0.0, 0.0), (20.0, 0.0, 0.0))];
    assert_eq!(thickened(&flat, Vec3::new(0.0, 1.0, 0.0)), (12, 8));
}

#[test]
fn issue_1973_a_thickened_l_of_two_flat_faces_keeps_its_edges_marks() {
    // an L of two flat faces (7 edges): its 7 edges and their 7 offsets are marked, the
    // 4 new uprights are not (14 marked, as the fork's C++ marks on the issue's L; before
    // the fix: 8)
    let l = [
        line((0.0, 0.0, 0.0), (20.0, 0.0, 0.0)),
        line((20.0, 0.0, 0.0), (20.0, 0.0, 10.0)),
    ];
    assert_eq!(thickened(&l, Vec3::new(0.0, 1.0, 0.0)), (18, 14));
}

#[test]
fn issue_1973_a_thickened_curved_sheet_keeps_its_edges_marks() {
    // a half cylinder of radius 10 (the arc swept 10 along its axis): its 4 edges and
    // their 4 offsets (before the fix: 6)
    let arc = [Curve::from(
        create_arc(Point3::new(0.0, 0.0, 0.0), 10.0, 0.0, 180.0).unwrap(),
    )];
    assert_eq!(thickened(&arc, Vec3::new(0.0, 0.0, 1.0)), (12, 8));
}
