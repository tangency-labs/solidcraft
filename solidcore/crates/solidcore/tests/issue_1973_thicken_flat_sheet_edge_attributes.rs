//! tangency-labs/solidcore#1973: thickening a flat sheet keeps the attributes of the
//! sheet's own edges. Upstream's `SmBrep::StitchFaces` glues notify the shell's objects,
//! whose attributes `MergeBrep` copied from the sheet; the port's stitch ran on the
//! B-rep's own (empty) store, so the glued z = 0 edges lost their marks.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::cast_possible_truncation
)] // test helpers

use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{Attribute, Behavior};
use solidcore::math::{Point3, Vec3};
use solidcore::offset::offset_executive::p2::{shell_brep_simple, OffsetOptions, ShellOptions};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::Brep;
use solidcore::{EntityRef, Kernel};

const MARK: u64 = 20_000;

/// The z of both ends of each marked edge.
fn marked_edges(r: &Brep) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for &e in r.edge_list() {
        if r.entity_attributes(EntityRef::Edge(e))
            .into_iter()
            .any(|(_, a)| a.attribute_id() == MARK)
        {
            let [v0, v1] = r.edge(e).unwrap().vertices;
            out.push((r.vertex(v0).unwrap().point.z, r.vertex(v1).unwrap().point.z));
        }
    }
    out
}

fn thicken(merge_sheet_side_walls: bool) -> Brep {
    let mut k = Kernel::default();
    let c = Curve::from(
        create_line_segment(Point3::new(0.0, 0.0, 0.0), Point3::new(20.0, 0.0, 0.0)).unwrap(),
    );
    let mut s =
        create_linear_sweep(&mut k, &[Some(&c)], Vec3::new(0.0, 1.0, 0.0), 10.0, false).unwrap();
    for e in s.edge_list().to_vec() {
        s.add_attribute(EntityRef::Edge(e), Attribute::long(MARK, 2, Behavior::Copy))
            .unwrap();
    }
    let options = ShellOptions {
        offset: OffsetOptions {
            distance: 2.0,
            extend_convex_edges: true,
            merge_results: true,
        },
        create_offset_solid: true,
        merge_sheet_side_walls,
    };
    shell_brep_simple(&mut k, &s, options, &[]).unwrap()
}

#[test]
fn thicken_of_a_flat_sheet_keeps_the_sheet_edges_marks() {
    for merge_sheet_side_walls in [false, true] {
        let r = thicken(merge_sheet_side_walls);
        assert_eq!(r.edge_list().len(), 12);
        let marked = marked_edges(&r);
        // the sheet's 4 edges at z = 0 and their 4 offsets at z = 2; the uprights are new
        assert_eq!(marked.len(), 8, "{merge_sheet_side_walls}: {marked:?}");
        let at_zero = marked
            .iter()
            .filter(|(z0, z1)| z0.abs() < 1e-9 && z1.abs() < 1e-9)
            .count();
        assert_eq!(at_zero, 4, "{merge_sheet_side_walls}: {marked:?}");
    }
}
