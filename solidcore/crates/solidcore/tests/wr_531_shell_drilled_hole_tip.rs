//! Regression test for WR-531 (the fork's tangency-labs/smlib@d5dadaa, capi test
//! `shelled_box_with_a_drilled_hole`): a box with a blind drilled hole (r 3, a 118
//! degree tip) shelled 2 mm inward, open at a side and, separately, at the top. The
//! hole's tip is a cone's apex whose seam is its only edge: the extended offset builds
//! no offset there, so the cone's offset ends open on a circle. Pinned SMLib returns
//! that non-manifold offset as a success (`SmOffsetExecutive.cpp:255-262`). Here the
//! apex's offset is a sphere zone of radius 2 (WR-531's caps) and the shell is a closed
//! solid.
//!
//! The successful result must pass the level-2 check (`check_brep`) clean.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::math::det;

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::{planar_faces_at, shell};
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::Curve;
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::surface::surface::SurfaceKind;
use solidcore::sweep::api_brep::create_rotational_sweep;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::vertex::{edges, tolerance};
use solidcore::{Brep, Kernel};

/// A 40 x 40 x 20 box with a hole r 3 drilled 10 deep from the top at (20, 20), its
/// tip a 118 degree cone.
fn drilled_box(k: &mut Kernel) -> Brep {
    let (r, top, depth) = (3.0, 20.0, 10.0);
    let tip = r / det::tan(59.0_f64.to_radians());
    let bottom = top - depth;
    let at = |x: f64, z: f64| Point3::new(20.0 + x, 20.0, z);
    let profile = [
        (at(0.0, bottom - tip), at(r, bottom)),
        (at(r, bottom), at(r, top + 1.0)),
        (at(r, top + 1.0), at(0.0, top + 1.0)),
        (at(0.0, top + 1.0), at(0.0, bottom - tip)),
    ];
    let curves: Vec<Curve> = profile
        .iter()
        .map(|&(a, b)| Curve::from(create_line_segment(a, b).unwrap()))
        .collect();
    let curves: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let drill = create_rotational_sweep(
        k,
        &curves,
        at(0.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        360.0,
        true,
    )
    .unwrap();
    let block = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 40.0, 20.0).unwrap();
    boolean_difference(k, block, drill).unwrap()
}

/// The hole's tip (the cone's apex): the swept profile's end on the axis, exact.
fn tip() -> Point3 {
    Point3::new(20.0, 20.0, 10.0 - 3.0 / det::tan(59.0_f64.to_radians()))
}

/// The drilled box's vertex at the hole's tip (its one vertex with a single edge):
/// its point and tolerance.
fn tip_vertex(part: &Brep) -> (Point3, f64) {
    let tips: Vec<_> = part
        .vertex_list()
        .iter()
        .copied()
        .filter(|&v| edges(part, v, None).unwrap().len() == 1)
        .collect();
    assert_eq!(tips.len(), 1, "one vertex with a single edge");
    (
        part.vertex(tips[0]).unwrap().point,
        tolerance(part, tips[0]).unwrap(),
    )
}

/// The drilled box shelled 2 inward, open at the top or at x = 0, with the drilled box's
/// tip vertex ([`tip_vertex`]).
fn shelled(open_top: bool) -> solidcore::Result<(Kernel, Brep, (Point3, f64))> {
    let mut k = Kernel::default();
    let part = drilled_box(&mut k);
    let tip = tip_vertex(&part);
    let open = if open_top {
        planar_faces_at(&k, &part, 2, 20.0)
    } else {
        planar_faces_at(&k, &part, 0, 0.0)
    };
    assert_eq!(open.len(), 1, "the open face");
    let b = shell(&mut k, part, -2.0, &open)?;
    Ok((k, b, tip))
}

/// The level-2 check's failure count and report lines.
fn check(k: &mut Kernel, b: &mut Brep) -> (usize, Vec<String>) {
    let c = check_brep(k, b, CheckLevel::Level2).unwrap();
    (c.failure_count(), c.lines())
}

/// The radius and centre of each face's sphere.
fn spheres(b: &Brep) -> Vec<(f64, Point3)> {
    b.face_list()
        .iter()
        .filter_map(|&f| match b.face(f).unwrap().surface.as_ref()?.kind() {
            SurfaceKind::Sphere(s) => Some((s.radius(), s.base().position().origin())),
            _ => None,
        })
        .collect()
}

#[test]
fn wr_531_drilled_hole_shells_with_a_sphere_cap() {
    for open_top in [false, true] {
        let case = format!("open_top {open_top}");
        let (mut k, mut b, (tip_point, tip_tol)) = shelled(open_top).unwrap();
        assert!(
            brep_is_manifold_solid(&b).unwrap(),
            "{case}: not a closed solid"
        );
        let caps = spheres(&b);
        assert_eq!(caps.len(), 1, "{case}: one cap on a sphere");
        let (radius, centre) = caps[0];
        // The cap stands in for the tip vertex (`BuildOffsetOfVertex`): it is the exact
        // offset of that vertex, the sphere of the shell's distance 2 about the exact tip,
        // to within the vertex's tolerance, the accuracy the kernel declares there. Its
        // radius is not the distance copied: `create_sphere_from_arcs` fits it to the rim
        // arcs, which are evaluated on the offset cone at coordinates of about 30, so no
        // few-ulps-of-2 bound holds and the vertex's tolerance is the claim. `radius - 2`
        // is exact (Sterbenz: the radius is within a factor 2 of 2). The centre goes
        // through the sphere's placement arithmetic: within the vertex's tolerance of the
        // vertex's point, which lies within the same tolerance of the exact tip (the seam
        // edge, a line swept about the axis through its end, ends at the tip exactly), so
        // within twice it of the tip; the level-2 check below confirms the cap meets its rim.
        assert!((radius - 2.0).abs() <= tip_tol, "{case}: radius {radius}");
        assert!(
            centre.distance(tip_point) <= tip_tol,
            "{case}: centre {centre:?}, tip vertex {tip_point:?}, tolerance {tip_tol}"
        );
        assert!(
            centre.distance(tip()) <= 2.0 * tip_tol,
            "{case}: centre {centre:?} off the tip by more than {}",
            2.0 * tip_tol
        );
        let (failed, lines) = check(&mut k, &mut b);
        assert_eq!(failed, 0, "{case}: {lines:?}");
    }
}
