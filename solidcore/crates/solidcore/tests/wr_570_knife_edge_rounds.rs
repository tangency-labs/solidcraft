//! Regression test for WR-570 (the fork's tangency-labs/smlib@b7ec55b, capi test
//! `knife_edge_rounds`): a knife edge (a cusp, two faces tangent along the edge that
//! fold back on each other) rounds to the ball touching both faces back from it.
//!
//! The part is a 62 1/6 x 21 plate whose corner is cut by a circle of radius 21 about
//! (TIP, -21), extruded 10: the arc wall meets the back face y = 0 tangentially at
//! x = TIP. Upstream `SmEdge::GetBlendEdgeuse` takes that edge for a smooth one and
//! the round fails. solidcore rounds it: the ball of radius r, 2r wide,
//! touches the back face sqrt(4 R r) back from the tip and takes off the
//! quadrilateral tip / wall centre / ball centre / back contact, less the wall's
//! sector and the ball's, all the way up. The mirrored part (its edge uses reversed)
//! rounds the same. The volume is asserted to 1e-9 relative, as the fork asserts it.
//!
//! The Nx1 corner's degeneracy test takes the knife edge's ends for ordinary corners
//! (its two faces' normals are anti-parallel there), and the edges of a sheet (the two
//! face uses of one face, anti-parallel at every point) for degenerate ones.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use std::f64::consts::PI;

use solidcore::blend::fillet_corner::p1::check_nx1_degeneracy;
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_surface_n_lib::OrientType;
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::primitive::api_primitives::{create_cylinder, create_plane};
use solidcore::sweep::api_brep::create_taper_extrude;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::edge;
use solidcore::topology::edgeuse::{evaluate_binormal, p1 as eu1, BinormalOptions};
use solidcore::topology::{EdgeId, EdgeuseId, FaceId, VertexId};
use solidcore::{Brep, Kernel};

const R: f64 = 21.0;
const TIP: f64 = 62.0 + 1.0 / 6.0;
const H: f64 = 10.0;

fn line(a: (f64, f64), b: (f64, f64)) -> Curve {
    Curve::from(create_line_segment(Vec3::new(a.0, a.1, 0.0), Vec3::new(b.0, b.1, 0.0)).unwrap())
}

/// The knife-edge part, its tip at x = `s * TIP` (`s` = 1, or -1 for the mirror image),
/// the profile chained head to tail.
fn part(k: &mut Kernel, s: f64) -> Brep {
    let curves = if s > 0.0 {
        vec![
            line((0.0, 0.0), (TIP, 0.0)),
            // (TIP, 0) to (TIP - R, -R)
            Curve::from(create_arc(Vec3::new(TIP, -R, 0.0), R, 90.0, 180.0).unwrap()),
            line((TIP - R, -R), (0.0, -R)),
            line((0.0, -R), (0.0, 0.0)),
        ]
    } else {
        vec![
            line((0.0, 0.0), (0.0, -R)),
            line((0.0, -R), (-TIP + R, -R)),
            // (-TIP + R, -R) to (-TIP, 0)
            Curve::from(create_arc(Vec3::new(-TIP, -R, 0.0), R, 0.0, 90.0).unwrap()),
            line((-TIP, 0.0), (0.0, 0.0)),
        ]
    };
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_taper_extrude(k, &refs, H, 0.0, 3, false).unwrap()
}

/// `SmApiBrepComputeVolume` at an integration accuracy well below the 1e-9 the volume
/// is checked to.
fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-11, &attrs).unwrap()
}

/// The knife edge: the vertical edge at (`s * TIP`, 0).
fn knife_edge(k: &Kernel, b: &Brep, s: f64) -> EdgeId {
    let at_tip = |p: Point3| (p.x - s * TIP).abs() < 1e-6 && p.y.abs() < 1e-6;
    b.edge_list()
        .iter()
        .copied()
        .find(|&e| {
            let curve = edge::curve(b, e).unwrap().unwrap();
            let ivl = edge::interval(b, e).unwrap();
            let p = curve.evaluate_point(k, ivl.min()).unwrap();
            let q = curve.evaluate_point(k, ivl.max()).unwrap();
            at_tip(p) && at_tip(q) && (p.z - q.z).abs() > H / 2.0
        })
        .expect("the knife edge")
}

/// The volume the round of radius `r` takes off.
fn removed(r: f64) -> f64 {
    let beta = ((R - r) / (R + r)).acos();
    let ox = TIP - (4.0 * R * r).sqrt();
    let quad = 0.5 * (TIP - ox) * (R + r);
    (quad - 0.5 * R * R * beta - 0.5 * r * r * (PI - beta)) * H
}

/// The knife edge of the part (mirrored when `s` is -1) rounded `r`: its face count and
/// volume, or the round's failure.
fn round(s: f64, r: f64) -> solidcore::Result<(usize, f64, bool)> {
    let mut k = Kernel::default();
    let mut b = part(&mut k, s);
    let e = knife_edge(&k, &b, s);
    fillet_edges(&mut k, &mut b, &[e], r, 1, 1, 1.0)?;
    let faces = b.face_list().len();
    let manifold = is_manifold_solid(&b).unwrap();
    Ok((faces, volume(&k, &mut b), manifold))
}

#[test]
fn wr_570_knife_edge_rounds_to_the_ball_back_from_it() {
    let before = (TIP - R) * R * H + R * R * (1.0 - PI / 4.0) * H;
    for r in [0.5, 2.0, 5.0] {
        for s in [1.0, -1.0] {
            let (faces, v, manifold) =
                round(s, r).unwrap_or_else(|e| panic!("r {r}, side {s}: the round failed: {e:?}"));
            let want = before - removed(r);
            assert_eq!(faces, 7, "r {r}, side {s}: faces");
            assert!(manifold, "r {r}, side {s}: not a manifold solid");
            assert!(
                (v - want).abs() <= 1e-9 * want,
                "r {r}, side {s}: volume {v}, expected {want}"
            );
        }
    }
}

/// The vertex an edge use starts at.
fn start(b: &Brep, eu: EdgeuseId) -> VertexId {
    let vu = eu1::vertexuse(b, eu).unwrap().unwrap();
    b.vertexuse(vu).unwrap().vertex.unwrap()
}

/// The two side edge uses the Nx1 degeneracy test reads next to `fil` at its start (its
/// clockwise neighbour and its radial's counter-clockwise one, the first's mate where
/// they coincide), each with its face and its face use normal at the corner end.
fn sides(k: &Kernel, b: &mut Brep, fil: EdgeuseId) -> [(FaceId, Vec3); 2] {
    let corner = start(b, fil);
    let s1 = eu1::cw_edgeuse(b, fil).unwrap().unwrap();
    let radial = eu1::radial(b, fil).unwrap().unwrap();
    let mut s2 = eu1::ccw_edgeuse(b, radial).unwrap().unwrap();
    if s1 == s2 {
        s2 = eu1::mate(b, s1).unwrap().unwrap();
    }
    [s1, s2].map(|eu| {
        let e = eu1::edge(b, eu).unwrap().unwrap();
        let ivl = edge::interval(b, e).unwrap();
        let same = eu1::orientation(b, eu).unwrap() == OrientType::Same;
        let t = if (start(b, eu) == corner) == same {
            ivl.min()
        } else {
            ivl.max()
        };
        let normal = evaluate_binormal(k, b, eu, t, false, &BinormalOptions::default())
            .unwrap()
            .face_normal;
        (eu1::face(b, eu).unwrap().unwrap(), normal)
    })
}

/// A sheet's edge meets the next one between the two sides of one face: the two face
/// uses' normals are anti-parallel at every point and no ball touches both sides, so
/// the corner is degenerate (not a cusp).
#[test]
fn wr_570_a_lamina_edge_is_not_a_cusp() {
    let mut k = Kernel::default();
    let mut b = create_plane(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0).unwrap();
    let eus: Vec<EdgeuseId> = b.edgeuses().iter().map(|(id, _)| id).collect();
    assert_eq!(
        eus.len(),
        8,
        "a square sheet: four edges, each used by both sides"
    );
    for eu in eus {
        let [(f1, n1), (f2, n2)] = sides(&k, &mut b, eu);
        assert_eq!(f1, f2, "edge use {eu:?}: both sides lie on the one face");
        assert!(
            (n1.dot(n2) + 1.0).abs() <= 1e-12,
            "edge use {eu:?}: {n1:?}, {n2:?}"
        );
        let v = start(&b, eu);
        assert!(
            check_nx1_degeneracy(&k, &mut b, v, eu).unwrap(),
            "edge use {eu:?}: a sheet's corner is degenerate"
        );
    }
}

/// A closed face's seam read through the mate (the 2x1 case: the two side edge uses
/// are the seam's two uses on the one face use's loop, so the second is taken as the
/// first's mate): the two sides are one surface at one point, its normal continuing
/// across the seam, anti-parallel only by the face use's sign. The corner is
/// degenerate (not a cusp).
#[test]
fn wr_570_a_closed_face_seam_is_not_a_cusp() {
    let mut k = Kernel::default();
    let mut b = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 20.0).unwrap();
    assert!(is_manifold_solid(&b).unwrap(), "the cylinder is a solid");
    let eus: Vec<EdgeuseId> = b.edgeuses().iter().map(|(id, _)| id).collect();
    let mut seams = 0;
    for eu in eus {
        let s1 = eu1::cw_edgeuse(&b, eu).unwrap().unwrap();
        let radial = eu1::radial(&b, eu).unwrap().unwrap();
        if eu1::ccw_edgeuse(&b, radial).unwrap().unwrap() != s1 {
            continue;
        }
        let [(f1, n1), (f2, n2)] = sides(&k, &mut b, eu);
        if f1 != f2 {
            continue;
        }
        seams += 1;
        assert!(
            (n1.dot(n2) + 1.0).abs() <= 1e-12,
            "edge use {eu:?}: {n1:?}, {n2:?}"
        );
        let v = start(&b, eu);
        assert!(
            check_nx1_degeneracy(&k, &mut b, v, eu).unwrap(),
            "edge use {eu:?}: a closed face's seam is degenerate"
        );
    }
    assert!(seams > 0, "the cylinder has a corner read across its seam");
}

/// The knife edge's two faces are anti-parallel at its ends (the back face's normal
/// (0, 1, 0), the arc wall's (0, -1, 0): its centre (s TIP, -R) lies outside the part),
/// two faces folded back on each other: each end is an ordinary corner.
#[test]
fn wr_570_the_knife_edge_ends_at_ordinary_corners() {
    for s in [1.0, -1.0] {
        let mut k = Kernel::default();
        let mut b = part(&mut k, s);
        let e = knife_edge(&k, &b, s);
        let fil = edge::blend_edgeuse(&k, &mut b, e).unwrap().unwrap();
        let other = eu1::radial(&b, fil).unwrap().unwrap();
        for eu in [fil, other] {
            let [(f1, n1), (f2, n2)] = sides(&k, &mut b, eu);
            assert_ne!(f1, f2, "side {s}: the sides lie on two faces");
            let up = Vec3::new(0.0, 1.0, 0.0);
            assert!(
                ((n1 - up).length() <= 1e-9 && (n2 + up).length() <= 1e-9)
                    || ((n1 + up).length() <= 1e-9 && (n2 - up).length() <= 1e-9),
                "side {s}: normals {n1:?}, {n2:?}"
            );
            let v = start(&b, eu);
            assert!(
                !check_nx1_degeneracy(&k, &mut b, v, eu).unwrap(),
                "side {s}: the knife edge's end is an ordinary corner"
            );
        }
    }
}
