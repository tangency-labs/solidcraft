//! Bodies through the exact exchange and back: the record they assemble into is the body they
//! were, in faces, edges, volume and area, and the history of the operation that made them
//! comes back in the record's indices.

use std::f64::consts::PI;

use solidcore_record::{Loop2, Plane, Region2, Vec2, Vec3};

use super::{assemble, body_from_exact, exact_body, exact_exported};
use crate::{Body, BoolOp, measure_mass};

fn v(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3::new(x, y, z)
}

/// The body back through the exchange, its face and edge counts, volume and area those of
/// `body` (to 1e-9 of them: the record's own pcurves are kept, and round faces' come from their
/// closed forms), every record face the exact face of the same place, and every edge followed.
fn round_trip(what: &str, body: &Body, volume: Option<f64>) -> Body {
    let exact = exact_body(body).unwrap_or_else(|e| panic!("{what}: out: {e}"));
    let (record, from) = assemble(&exact).unwrap_or_else(|e| panic!("{what}: in: {e}"));
    let back = Body::from_record(record).unwrap_or_else(|e| panic!("{what}: as a body: {e}"));
    assert_eq!((back.face_count(), back.edge_count()), (body.face_count(), body.edge_count()), "{what}: faces and edges");
    let mut faces = from.faces.clone();
    faces.sort_unstable();
    assert_eq!(faces, (0..body.face_count()).collect::<Vec<_>>(), "{what}: each face once");
    assert!(from.edges.iter().all(Option::is_some), "{what}: every edge followed: {:?}", from.edges);
    let (a, b) = (measure_mass(body).unwrap(), measure_mass(&back).unwrap());
    let close = |x: f64, y: f64| (x - y).abs() <= 1e-9 * x.abs().max(1.0);
    assert!(close(a.volume, b.volume), "{what}: volume {} back as {}", a.volume, b.volume);
    assert!(close(a.area, b.area), "{what}: area {} back as {}", a.area, b.area);
    if let Some(exact) = volume {
        assert!((b.volume - exact).abs() <= 1e-6 * exact, "{what}: volume {} against {exact}", b.volume);
    }
    back
}

#[test]
fn analytic_bodies_come_back_as_they_were() {
    let block = crate::box_solid(v(0.0, 0.0, 0.0), v(4.0, 3.0, 2.0)).unwrap();
    round_trip("a box", &block, Some(24.0));
    let rod = crate::cylinder(v(1.0, 2.0, 3.0), v(0.0, 0.0, 1.0), 2.0, 5.0).unwrap();
    round_trip("a cylinder", &rod, Some(PI * 4.0 * 5.0));
    let tilted = crate::cylinder(v(0.0, 0.0, 0.0), v(1.0, 1.0, 0.5), 1.5, 4.0).unwrap();
    round_trip("a tilted cylinder", &tilted, Some(PI * 2.25 * 4.0));
    let ball = crate::sphere(v(1.0, -1.0, 2.0), 3.0).unwrap();
    round_trip("a sphere", &ball, Some(4.0 / 3.0 * PI * 27.0));
    let ring = crate::torus(v(0.0, 0.0, 0.0), 5.0, 1.0).unwrap();
    round_trip("a torus", &ring, Some(2.0 * PI * PI * 5.0));
    // A cone: a right triangle (radius 2, height 3) turned about its side on the axis.
    let tri = Region2 { outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(2.0, 0.0), Vec2::new(0.0, 3.0)]), holes: Vec::new() };
    let cone = crate::revolve(&Plane::XY, &[tri], Vec2::ZERO, Vec2::new(0.0, 1.0), 2.0 * PI).unwrap().pop().unwrap();
    round_trip("a cone", &cone, Some(PI * 4.0 * 3.0 / 3.0));
}

#[test]
fn modelled_bodies_come_back_as_they_were() {
    let block = crate::box_solid(v(0.0, 0.0, 0.0), v(10.0, 10.0, 4.0)).unwrap();
    let hole = crate::cylinder(v(5.0, 5.0, -1.0), v(0.0, 0.0, 1.0), 2.0, 6.0).unwrap();
    let drilled = crate::boolean(&block, &hole, BoolOp::Cut).unwrap().unwrap();
    round_trip("a box with a hole", &drilled, Some(400.0 - PI * 4.0 * 4.0));
    let rounded = crate::fillet(&block, &[v(10.0, 5.0, 4.0)], 1.0).unwrap();
    // The round takes (1 - π/4) of a unit square off along the 10 mm edge.
    round_trip("a box with a round", &rounded, Some(400.0 - 10.0 * (1.0 - PI / 4.0)));
    let ball = crate::sphere(v(10.0, 10.0, 4.0), 3.0).unwrap();
    let dented = crate::boolean(&block, &ball, BoolOp::Cut).unwrap().unwrap();
    // An eighth of the ball is inside the corner.
    round_trip("a box with a ball cut from a corner", &dented, Some(400.0 - 4.0 / 3.0 * PI * 27.0 / 8.0));
    let thread = ridge_thread(6.0, 1.0, 3.0);
    round_trip("a thread", &thread, None);
}

#[test]
fn a_cut_keeps_its_history_through_the_exchange() {
    let block = crate::box_solid(v(0.0, 0.0, 0.0), v(10.0, 10.0, 4.0)).unwrap();
    let hole = crate::cylinder(v(5.0, 5.0, -1.0), v(0.0, 0.0, 1.0), 2.0, 6.0).unwrap();
    let drilled = crate::boolean(&block, &hole, BoolOp::Cut).unwrap().unwrap();
    let made = drilled.history().expect("the cut reports its history").clone();
    let exported = exact_exported(&drilled).unwrap();
    let (record, from) = assemble(&exported.body).unwrap();
    // Each record face and edge has the history of the face and edge it came from.
    let history = super::record_history(exported.history.as_ref().unwrap(), &from, &record).expect("the history follows");
    for (i, &f) in from.faces.iter().enumerate() {
        assert_eq!(history.faces[i], made.faces[f], "face {i}, from {f}");
    }
    for (i, e) in from.edges.iter().enumerate() {
        let e = e.expect("every edge followed");
        assert_eq!(history.edges[i].origins, made.edges[e].origins, "edge {i}, from {e}");
        let mut faces: Vec<usize> = history.edges[i].faces.iter().map(|&f| from.faces[f]).collect();
        let mut was = made.edges[e].faces.clone();
        faces.sort_unstable();
        was.sort_unstable();
        assert_eq!(faces, was, "edge {i}'s faces");
    }
    let back = body_from_exact(exported, &[&block, &hole]).unwrap();
    assert_eq!(back.history(), Some(&history), "the body carries it, checked against its operands");
}

#[test]
fn an_exact_body_with_no_pcurves_has_them_made() {
    let block = crate::box_solid(v(0.0, 0.0, 0.0), v(10.0, 10.0, 4.0)).unwrap();
    let hole = crate::cylinder(v(5.0, 5.0, -1.0), v(0.0, 0.0, 1.0), 2.0, 6.0).unwrap();
    let drilled = crate::boolean(&block, &hole, BoolOp::Cut).unwrap().unwrap();
    let mut exact = exact_body(&drilled).unwrap();
    for f in &mut exact.faces {
        for b in &mut f.bounds {
            if let solidcraft_kernel_api::ExactBound::Edges { uses } = b {
                for u in uses {
                    u.pcurve = None;
                }
            }
        }
    }
    let (record, _) = assemble(&exact).unwrap();
    let back = Body::from_record(record).unwrap();
    let b = measure_mass(&back).unwrap();
    let want = 400.0 - PI * 4.0 * 4.0;
    assert!((b.volume - want).abs() <= 1e-6 * want, "volume {} against {want}", b.volume);
}

#[test]
fn a_wrong_exact_body_is_refused_with_why() {
    let block = crate::box_solid(v(0.0, 0.0, 0.0), v(1.0, 1.0, 1.0)).unwrap();
    let mut exact = exact_body(&block).unwrap();
    if let Some(solidcraft_kernel_api::ExactBound::Edges { uses }) = exact.faces.first_mut().and_then(|f| f.bounds.first_mut()) {
        uses.pop();
    }
    assert!(assemble(&exact).is_err(), "a face whose bound does not close");
    let mut exact = exact_body(&block).unwrap();
    exact.shells = vec![vec![99]];
    assert!(assemble(&exact).is_err(), "a shell of a face that is not there");
}

/// A threaded shaft as the thread feature makes one (as `thread_display_tests.rs` builds it):
/// an ISO metric ridge of `turns` turns, screw swept, joined to a core of the minor diameter.
pub(crate) fn ridge_thread(major: f64, pitch: f64, turns: f64) -> Body {
    let h = pitch * 3f64.sqrt() / 2.0;
    let (rc, rm) = (major / 2.0 - 5.0 * h / 8.0, major / 2.0);
    let (foot, crest) = ((pitch - pitch / 4.0) / 2.0, pitch / 16.0);
    let z0 = pitch;
    let len = z0 + pitch * turns + pitch;
    let half = |r: f64| foot + (crest - foot) * (r - rc) / (rm - rc);
    let r0 = rc - 0.1 * pitch;
    let section = Loop2::polygon(&[Vec2::new(r0, -half(r0)), Vec2::new(rm, -crest), Vec2::new(rm, crest), Vec2::new(r0, half(r0))]);
    let plane = Plane { origin: Vec3::new(0.0, 0.0, z0), x: Vec3::X, y: Vec3::Z };
    let ridge = crate::sweep_helix(&plane, &Region2 { outer: section, holes: Vec::new() }, Vec3::ZERO, Vec3::Z, pitch, turns).unwrap();
    let core = Region2 { outer: Loop2::circle(Vec2::ZERO, rc), holes: Vec::new() };
    let core = crate::extrude(&Plane::XY, &[core], 0.0, len).unwrap().pop().unwrap();
    crate::boolean(&core, &ridge, BoolOp::Union).unwrap().unwrap()
}
