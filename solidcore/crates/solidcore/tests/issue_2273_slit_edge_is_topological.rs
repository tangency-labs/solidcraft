//! tangency-labs/solidcore#2273 (the fork's tangency-labs/smlib@8d0fbff, in smlib#79): an
//! edge with one face on both sides, its primary edge use and that use's radial partner in
//! the face's two loop uses (the radial pairing running the face's two sides). `SmEdge::
//! IsTopological` read edge uses 0 and 2, in one loop use, as a slit and called the edge
//! removable; `SmBrep::DeleteEdge` reads the primary and its radial, sees two loop uses and
//! merges the face into itself, deleting it with the surface it was to keep, and
//! `SmMerge::BooleanPostProcess` then sets the deleted face's domain (a SIGSEGV in the
//! fork, found by its capi test `middle_edge_of_an_arc_tangent_chain_r0_25`). The fork's
//! blend made that pairing; here it is made directly, by swapping the last two radial list
//! members of the slit that tangency-labs/solidcore#1120's union leaves in a cylinder wall.
//! `IsTopological` now reads the pair `DeleteEdge` reads: such an edge is not removable,
//! and the clean-up leaves the body as it was (UB-290). Without the fix `IsTopological`
//! returns true here and `boolean_post_process` fails with `NotFound` partway through
//! `DeleteEdge`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::boolean::api_brep::boolean_union;
use solidcore::boolean::merge::p1::{boolean_post_process, CleanupRelations};
use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p1;
use solidcore::topology::edge;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::{Brep, EdgeId, Kernel};

const R: f64 = 30.0;

/// The r 30 circle about the Z axis in z = 0 over `[start, end]` degrees.
fn half_arc(k: &Kernel, start: f64, end: f64) -> Curve {
    let mut frame = Axis2Placement::default();
    frame
        .set_canonical(
            Point3::ZERO,
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        )
        .unwrap();
    let interval = Extent1d::from_min_max(start, end);
    Curve::from(Ellipse::create_canonical_circle(k, &frame, R, Some(&interval), None).unwrap())
}

/// #1120's body: a cylinder r 30, z -6..0, united with a disc r 30, z 0..5 extruded from
/// two half arcs. The wall keeps the line between the disc's halves as a slit.
fn joined(k: &mut Kernel) -> Brep {
    let flange = create_cylinder(k, Point3::new(0.0, 0.0, -6.0), R, 6.0).unwrap();
    let a = half_arc(k, 0.0, 180.0);
    let b = half_arc(k, 180.0, 360.0);
    let disc = create_linear_sweep(
        k,
        &[Some(&a), Some(&b)],
        Vec3::new(0.0, 0.0, 1.0),
        5.0,
        true,
    )
    .unwrap();
    boolean_union(k, flange, disc).unwrap()
}

/// The removable edges with one face on both sides: the slits.
fn slits(k: &Kernel, b: &mut Brep) -> Vec<EdgeId> {
    let mut out = Vec::new();
    for e in p1::edges(b, None).unwrap() {
        if edge::faces(b, e, None).unwrap().len() == 1
            && edge::is_topological(k, b, e, 10.0).unwrap().is_topological
        {
            out.push(e);
        }
    }
    out
}

/// The loop use of the edge's primary edge use and of its radial partner.
fn primary_and_radial_loopuses(b: &Brep, e: EdgeId) -> (bool, bool) {
    let primary = eu1::primary_edgeuse(b, e).unwrap().unwrap();
    let radial = eu1::radial(b, primary).unwrap().unwrap();
    let third = edge::edgeuses(b, e, None, None).unwrap()[2];
    let lu = eu1::loopuse(b, primary).unwrap();
    (
        lu == eu1::loopuse(b, radial).unwrap(),
        lu == eu1::loopuse(b, third).unwrap(),
    )
}

/// The pairing the fork's blend left (smlib@8d0fbff): the face's loop use runs the edge
/// twice the same way, so the primary's radial partner is the use in the face's other loop
/// use. Made from a slit `[a, a', b', b]` (`a`, `b` in the loop use `L`, their mates `a'`,
/// `b'` in the other, `b` against the edge): `b` and `b'` exchange their places in the
/// radial list, their orientations and their trim curve, so `b` in `L` runs with the edge
/// as `a` does and `b'` against it: `[a, a', b, b']`.
fn pair_across_the_faces_two_sides(b: &mut Brep, e: EdgeId) {
    let list = edge::edgeuses(b, e, None, None).unwrap();
    let (mate, used) = (list[2], list[3]);
    let (o_mate, o_used) = (
        b.edgeuse(mate).unwrap().orientation,
        b.edgeuse(used).unwrap().orientation,
    );
    let (c_mate, c_used) = (
        b.edgeuse_mut(mate).unwrap().uv_trim_curve.take(),
        b.edgeuse_mut(used).unwrap().uv_trim_curve.take(),
    );
    b.edgeuse_mut(mate).unwrap().orientation = o_used;
    b.edgeuse_mut(mate).unwrap().uv_trim_curve = c_used;
    b.edgeuse_mut(used).unwrap().orientation = o_mate;
    b.edgeuse_mut(used).unwrap().uv_trim_curve = c_mate;
    b.edge_mut(e).unwrap().edgeuses.swap(2, 3);
}

#[test]
fn issue_2273_slit_paired_across_the_faces_two_sides_is_not_removed() {
    let mut k = Kernel::default();
    let mut b = joined(&mut k);
    let slit = slits(&k, &mut b);
    assert_eq!(slit.len(), 1, "#1120's union leaves one slit in the wall");
    let e = slit[0];
    // A slit: the primary edge use and its radial in one loop use, the third (the radial's
    // mate) in the other.
    assert_eq!(primary_and_radial_loopuses(&b, e), (true, false));

    // The radial pairing the other way round: the primary's radial partner in the face's
    // other loop use, the third use still in the primary's.
    pair_across_the_faces_two_sides(&mut b, e);
    assert_eq!(primary_and_radial_loopuses(&b, e), (false, true));
    let face = edge::faces(&b, e, None).unwrap()[0];
    let faces = p1::faces(&b, None).unwrap().len();

    assert!(
        !edge::is_topological(&k, &mut b, e, 10.0)
            .unwrap()
            .is_topological,
        "IsTopological reads the pair DeleteEdge reads: not removable"
    );
    b.set_editing_enabled(true);
    boolean_post_process(&mut k, &mut b, &CleanupRelations::default(), true).unwrap();
    assert!(b.face(face).is_ok(), "the wall is not deleted");
    assert!(b.edge(e).is_ok(), "the edge stays");
    assert_eq!(p1::faces(&b, None).unwrap().len(), faces);
}

#[test]
fn issue_2273_slit_in_one_loopuse_is_still_removed() {
    // The same slit, paired as made: `DeleteEdge` splits it off the loop, as before.
    let mut k = Kernel::default();
    let mut b = joined(&mut k);
    let e = slits(&k, &mut b)[0];
    let face = edge::faces(&b, e, None).unwrap()[0];
    b.set_editing_enabled(true);
    boolean_post_process(&mut k, &mut b, &CleanupRelations::default(), true).unwrap();
    assert!(b.face(face).is_ok());
    assert!(b.edge(e).is_err(), "the slit is removed");
}
