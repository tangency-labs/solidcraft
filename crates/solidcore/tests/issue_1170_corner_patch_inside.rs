//! #1170 (WR-461): the setback corner patch of three rounds stays inside the faces it
//! meets. A box 60 x 40 x 20 with r5 rounds on the three edges at (60, 40, 20) and a
//! setback on that corner. Since solidcore#2933 the marked corner is the blend corner's
//! Setback, whose certified fill stays inside the box with or without
//! `corner_blend_stays_inside_neighbours` (the N x N patch's fix, which bulged 0.08 past the
//! faces at a setback of 13 without it).

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::math::Point3;
use solidcore::nurbs::knots::Side;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::{
    brep_bounding_box, brep_is_manifold_solid, surface_closest_point,
};
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::face::p2 as face;
use solidcore::topology::{edge, vertex, Brep};
use solidcore::{Kernel, VertexId};

const CORNER: [f64; 3] = [60.0, 40.0, 20.0];

fn at_corner(b: &Brep, v: VertexId) -> solidcore::Result<bool> {
    let p = vertex::point(b, v)?;
    Ok((p.x - CORNER[0]).abs() + (p.y - CORNER[1]).abs() + (p.z - CORNER[2]).abs() < 1e-9)
}

/// The box filleted with a setback at its far corner.
fn build(setback: f64, stay_inside: bool) -> solidcore::Result<(Kernel, Brep)> {
    let mut k = Kernel::default();
    k.fixes.corner_blend_stays_inside_neighbours = stay_inside;
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 20.0)?;
    let mut edges = Vec::new();
    for e in b.edges().ids() {
        let (s, t) = edge::start_end_vertices(&b, e)?;
        for v in [s, t].into_iter().flatten() {
            if at_corner(&b, v)? {
                edges.push(e);
            }
        }
    }
    assert_eq!(edges.len(), 3);
    let mut set_back = None;
    for v in b.vertices().ids() {
        if at_corner(&b, v)? {
            set_back = Some(v);
        }
    }
    {
        let mut session = FilletSession::new(&mut k, &mut b)?;
        session.prepare_constant_radius_edges(&mut k, &edges, 5.0, 1, 1, 1.0)?;
        // set back (`AddSetBackVertex`), so the corner is the Setback, not the Ball
        if let Some(v) = set_back {
            session.add_set_back_vertex(v);
        }
        session.create_corners(&k, None)?;
        for id in session.fillet_corners()? {
            if let Some(v) = session.corner(id)?.vertex {
                if at_corner(session.target(), v)? {
                    session.set_corner_setback(id, setback)?;
                }
            }
        }
        session.do_filleting(&mut k, None)?;
    }
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default())?;
    Ok((k, b))
}

/// The tight bounding box maximum, the face count (the B-rep's faces, not its face
/// records: deleted faces leave the arena) and the solidity of the box filleted with a
/// setback at its far corner.
fn fillet_with_setback(
    setback: f64,
    stay_inside: bool,
) -> solidcore::Result<([f64; 3], usize, bool)> {
    let (k, b) = build(setback, stay_inside)?;
    let x = brep_bounding_box(&k, &b, true)?;
    Ok((
        [x.max_at(0), x.max_at(1), x.max_at(2)],
        b.face_list().len(),
        brep_is_manifold_solid(&b)?,
    ))
}

/// The largest angle (degrees) between the surface normals of the two faces of an
/// edge, over the edges whose two faces are both B-spline patches (the corner patch's
/// inner curves and its seams to the rounds), sampled along the edge.
fn max_patch_normal_angle(setback: f64, stay_inside: bool) -> solidcore::Result<f64> {
    let (k, b) = build(setback, stay_inside)?;
    let mut worst: f64 = 0.0;
    for e in b.edges().ids() {
        let faces = edge::faces(&b, e, None)?;
        let [f1, f2] = faces[..] else { continue };
        let (Some(s1), Some(s2)) = (face::surface(&b, f1)?, face::surface(&b, f2)?) else {
            continue;
        };
        if !matches!(s1.kind(), SurfaceKind::BSpline(_))
            || !matches!(s2.kind(), SurfaceKind::BSpline(_))
        {
            continue;
        }
        let Some(c) = edge::curve(&b, e)? else {
            continue;
        };
        let iv = edge::interval(&b, e)?;
        for i in 0..=20 {
            let p = c.evaluate_point(&k, iv.min() + iv.length() * f64::from(i) / 20.0)?;
            let normal = |s: &solidcore::surface::surface::Surface| -> solidcore::Result<_> {
                let uv = surface_closest_point(&k, s, p)?
                    .uvs
                    .first()
                    .copied()
                    .ok_or(solidcore::SolidError::sm_err("issue_1170"))?;
                s.evaluate_normal(&k, uv, Side::Left, Side::Left)
            };
            // the normals' angle either way round, from the cross product, which resolves
            // it below acos's 2.1e-8 rad floor (tangency-labs/smlib#119)
            let (n1, n2) = (normal(s1)?, normal(s2)?);
            let ang = n1.cross(n2).length().atan2(n1.dot(n2).abs()).to_degrees();
            worst = worst.max(ang);
        }
    }
    Ok(worst)
}

#[test]
fn the_patch_keeps_g1_across_its_inner_curves() {
    // Without the fix the sampled normals differ by 0.0017 degrees (the fit of the
    // derivative curves); scaling the derivative curves must not add to that.
    for setback in [11.0, 12.0, 13.0, 14.0, 15.0] {
        let on = max_patch_normal_angle(setback, true).unwrap();
        assert!(on < 0.01, "setback {setback}: {on} degrees");
    }
}

#[test]
fn a_setback_patch_stays_inside_the_box_with_the_fix() {
    for setback in [8.0, 10.0, 11.0, 12.0, 13.0, 15.0] {
        let (max, faces, solid) = fillet_with_setback(setback, true).unwrap();
        for (m, limit) in max.iter().zip(CORNER) {
            assert!(*m <= limit + 1e-6, "setback {setback}: {max:?}");
        }
        assert_eq!(faces, 15, "setback {setback}");
        assert!(solid, "setback {setback}");
    }
}

#[test]
fn the_setback_stays_inside_without_the_n_by_n_fix() {
    // the N x N patch bulged 0.08 past the faces at 13 without the fix
    for setback in [10.0, 13.0] {
        let (max, faces, solid) = fillet_with_setback(setback, false).unwrap();
        for (m, limit) in max.iter().zip(CORNER) {
            assert!(*m <= limit + 1e-6, "setback {setback}: {max:?}");
        }
        assert_eq!(faces, 15, "setback {setback}");
        assert!(solid, "setback {setback}");
    }
}
