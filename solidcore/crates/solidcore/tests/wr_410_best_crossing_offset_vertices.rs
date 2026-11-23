//! WR-410 (tangency-labs/solidcore#1059): a boolean vertex first placed by one edge crossing
//! is placed again by a later crossing whose misplacement bound along the edge (gap over
//! sine) is smaller, only where a certified bound proves that the edge stretch the vertex
//! moves along stays within the vertex's tolerance of the other object and of the first
//! crossing's curve (`VertexDefinition::place_by_crossing`); otherwise the first placement
//! stays. Retiring `boolean_vertex_by_best_crossing` (tangency-labs/solidcraft#14246) moves
//! two `offset.offset_executive_p2` rows, both rounded solid offsets whose stitch merges the
//! offset faces with booleans:
//!
//! - `extrude_holes1_face0_inset24_7_rounded`: `Boolean/extrudeHoles1.smb` inset by 24.7
//!   keeping face 0 as a shell face (approximation tolerance 0.0059574935488984). Its vertex
//!   bounds move: lo.x, lo.y by about 1e-7, lo.z by 2.8e-13, hi.y by 1.0e-6, hi.z by 9.8e-7.
//! - `shell_file_problemShell10_face1_round`: `Shell/problemShell10.smb` inset by 12.7 keeping
//!   face 1 (approximation tolerance 0.001798855365225). Its hi.z moves by 1.03e-5.
//!
//! The counts stay. A vertex belongs on every face it bounds, and the faces' surfaces are the
//! exact offset surfaces while the edge curves are fits within the approximation tolerance, so
//! the measure is the vertex's largest gap to its faces' surfaces (`surface_closest_point`).
//! For each moved bound, measured with the first crossing (main before the retirement) and
//! with the certified move:
//!
//! | row, bound | vertex's largest face gap, first crossing | certified move |
//! |---|---|---|
//! | extrudeHoles1 lo.x, lo.y | 2.75e-7 | 2.1e-12 |
//! | extrudeHoles1 lo.z | 3.3e-13 | 1.5e-14 |
//! | extrudeHoles1 hi.y | 9.20e-7 | 3.66e-7 (to its cone) |
//! | extrudeHoles1 hi.z | 7.93e-7 | 7.93e-7 |
//! | problemShell10 hi.z | 9.73e-6 (its largest edge gap 1.03e-5, past the 1e-5 tolerance) | 1.9e-14 |
//!
//! The two extrudeHoles1 hi rows are also measured against an exact reference, the common
//! point of the surfaces the vertex belongs on (Newton's method, its own error bound under
//! 1e-10 given exact surface distances). For hi.y every distance is a closed form. For hi.z
//! the revolution's distance is `surface_closest_point` with its normal, a numerical
//! projection, not a closed form, and the error bound assumes that projection exact:
//!
//! - hi.y: its faces lie on a plane, a cone and a sphere (closed forms). Their common point is
//!   1.44e-6 from the first crossing's vertex (1.20e-6 in y) and 5.68e-7 from the moved one
//!   (1.65e-7 in y). The moved vertex is closer, not exact: it is still 3.66e-7 off the cone.
//!   Its gap to one edge grows from 2.43e-6 to 2.62e-6 (under the vertex tolerance 1e-5).
//! - hi.z: its faces lie on one sphere and one surface of revolution, and one of its edges is
//!   an exact ellipse, the sphere's section by the plane x = 27.5 (the trace of a consumed
//!   face). The common point of the sphere, that plane and the revolution is 1.88e-6 from the
//!   first crossing's z (4.72e-6 in all) and 9.03e-7 from the moved z (9.69e-7 in all). Its
//!   edge 125, a fit on the revolution, passes 3.63e-6 from that point, so that edge's gap to
//!   the vertex grows,
//!   6.83e-7 -> 2.85e-6, as the vertex moves toward it (the vertex's largest edge gap falls,
//!   4.70e-6 -> 2.85e-6).
//!
//! In problemShell10 the two top vertices, (-50.8, -28.575) and (34.925, -28.575), move onto
//! their faces by certified moves (each edge stretch within 9.73e-6 and 9.36e-6 of the other
//! object and of the first crossing's curve, under the vertex tolerance 1.8e-3): 9.73e-6 ->
//! 1.9e-14 and 9.35e-6 -> 1.6e-14 off their faces, their largest edge gaps 1.03e-5 -> 9.73e-6
//! and 1.02e-5 -> 9.35e-6. The vertex (-50.8, -114.3) would move against a face on an offset
//! surface, which has no certified bound, so it keeps its first placement, 7.49e-7 off a
//! face. The tests below assert the moved gaps under bounds the first-crossing vertices
//! exceed.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::Notifications;
use solidcore::curve::curve::CurveKind;
use solidcore::geom::attribute::AttributeStore;
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::knots::Side;
use solidcore::offset::offset_executive::p1::{OffsetExecutive, OffsetOperation};
use solidcore::offset::offset_executive::p2::SolidOffsetOptions;
use solidcore::query::api_queries::{edge_closest_point, surface_closest_point};
use solidcore::surface::implicit::AnalyticSupport;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{edge, vertex};
use solidcore::{Brep, Kernel, VertexId};

/// The upstream test file, read as the difftest runner reads it; `None` (the test is
/// skipped) without the pinned upstream checkout.
fn upstream_part(k: &mut Kernel, name: &str) -> Option<Brep> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles")
        .join(name);
    let Ok(bytes) = std::fs::read(&path) else {
        assert!(
            std::env::var_os("SOLIDCORE_REQUIRE_UPSTREAM").is_none(),
            "SOLIDCORE_REQUIRE_UPSTREAM is set and the upstream test file is missing: {}",
            path.display()
        );
        eprintln!("unavailable upstream test file: {}", path.display());
        return None;
    };
    let mut b = Brep::new_with_infinite_region().unwrap();
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let key = solidcore::topology::loopuse::notify_ref;
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key,
        records: &mut records,
        next_key: 1 << 50,
    };
    read_from_file(k, &mut b, bytes, FileType::Ascii, false, &mut env).unwrap();
    b.use_arena_naming().unwrap();
    Some(b)
}

/// The rounded inset (`DoSolidOffset`, stitched and merged) of `name` keeping face `face`
/// as a shell face, as the goldens row runs it.
fn rounded_inset(k: &mut Kernel, name: &str, face: usize, distance: f64, tol: f64) -> Option<Brep> {
    let mut part = upstream_part(k, name)?;
    let f = bp1::faces(&part, None).unwrap()[face];
    let mut executive = OffsetExecutive::new(OffsetOperation::SolidOffset, distance, tol);
    executive.set_inset(true);
    executive.set_extend_convex_edges(false);
    executive.set_merge_results(true);
    executive.set_create_offset_solid(false);
    executive.set_shell_faces(&[f]);
    let mut notifications = Notifications::new(&mut part);
    Some(
        executive
            .do_solid_offset(
                k,
                Some(&mut part),
                SolidOffsetOptions {
                    skip_stitch_merge: false,
                },
                &mut notifications.context(),
            )
            .unwrap(),
    )
}

fn coordinate(p: Point3, axis: usize) -> f64 {
    [p.x, p.y, p.z][axis]
}

/// The vertex attaining the body's least (`high` false) or greatest coordinate on `axis`.
fn extreme_vertex(b: &Brep, axis: usize, high: bool) -> VertexId {
    let mut best: Option<(VertexId, f64)> = None;
    for &v in b.vertex_list() {
        let c = coordinate(vertex::point(b, v).unwrap(), axis);
        let better = match best {
            None => true,
            Some((_, b)) => (high && c > b) || (!high && c < b),
        };
        if better {
            best = Some((v, c));
        }
    }
    best.unwrap().0
}

/// The vertex's largest gap to the surfaces of the faces it bounds.
fn face_gap(k: &Kernel, b: &Brep, v: VertexId) -> f64 {
    let p = vertex::point(b, v).unwrap();
    vertex::faces(b, v)
        .unwrap()
        .into_iter()
        .map(|f| {
            let s = b.face(f).unwrap().surface.as_ref().unwrap();
            surface_closest_point(k, s, p).unwrap().closest.distance
        })
        .fold(0.0, f64::max)
}

/// The vertex's largest gap to the curves of the edges it bounds.
fn edge_gap(k: &Kernel, b: &Brep, v: VertexId) -> f64 {
    let p = vertex::point(b, v).unwrap();
    vertex::edges(b, v, None)
        .unwrap()
        .into_iter()
        .map(|e| edge_closest_point(k, b, e, p).unwrap().closest.distance)
        .fold(0.0, f64::max)
}

/// The signed distance to an analytic support and its unit gradient at `x`: a plane, a
/// cylinder, a cone or a sphere (each a distance function, closed form).
fn analytic_distance(s: &AnalyticSupport, x: Point3) -> (f64, Vec3) {
    match *s {
        AnalyticSupport::Plane { placement, .. } => {
            let n = placement
                .x_axis()
                .cross(placement.y_axis())
                .unitize()
                .unwrap();
            ((x - placement.origin()).dot(n), n)
        }
        AnalyticSupport::Cylinder { placement, radius } => {
            let a = placement
                .x_axis()
                .cross(placement.y_axis())
                .unitize()
                .unwrap();
            let v = x - placement.origin();
            let radial = v - a * v.dot(a);
            let len = radial.length();
            (len - radius, radial * (1.0 / len))
        }
        AnalyticSupport::Cone {
            placement,
            base_radius,
            semi_angle_deg,
        } => {
            // the signed distance in the half plane through the axis to the cone's line
            // `rho = r0 + h tan(theta)`: `(rho - r0 - h tan(theta)) cos(theta)`
            let a = placement
                .x_axis()
                .cross(placement.y_axis())
                .unitize()
                .unwrap();
            let v = x - placement.origin();
            let h = v.dot(a);
            let radial = v - a * h;
            let rho = radial.length();
            let (sin, cos) = semi_angle_deg.to_radians().sin_cos();
            (
                (rho - base_radius) * cos - h * sin,
                radial * (cos / rho) - a * sin,
            )
        }
        AnalyticSupport::Sphere { placement, radius } => {
            let v = x - placement.origin();
            let len = v.length();
            (len - radius, v * (1.0 / len))
        }
        AnalyticSupport::Torus { .. } => panic!("not a plane, cylinder, cone or sphere: {s:?}"),
    }
}

fn det3(m: [Vec3; 3]) -> f64 {
    m[0].dot(m[1].cross(m[2]))
}

/// The common point of three surfaces near `start`, by Newton's method on their distance
/// functions (`dist` gives each signed distance and unit gradient), with a bound on its own
/// error: its residual `r` (the largest distance to the three) times the Frobenius norm of
/// the inverse of the matrix of the three unit normals there, `|x - x*| <= |J^-1| sqrt(3) r`
/// to first order. The second order term is the surfaces' curvature times that bound
/// squared, far below it here (the bound is asserted under 1e-10, the curvature radii are
/// over 1).
fn newton_corner(dist: impl Fn(Point3) -> [(f64, Vec3); 3], start: Point3) -> (Point3, f64) {
    let mut x = start;
    for _ in 0..50 {
        let rows = dist(x);
        let j = rows.map(|(_, n)| n);
        let f = rows.map(|(d, _)| d);
        let det = det3(j);
        // Cramer's rule for J dx = -f
        let col = |i: usize| -> f64 {
            let mut m = j;
            for (row, fv) in m.iter_mut().zip(f) {
                let mut c = [row.x, row.y, row.z];
                c[i] = -fv;
                *row = Vec3::new(c[0], c[1], c[2]);
            }
            det3(m) / det
        };
        let dx = Vec3::new(col(0), col(1), col(2));
        x += dx;
        if dx.length() < 1e-16 {
            break;
        }
    }
    let rows = dist(x);
    let residual = rows.iter().map(|(d, _)| d.abs()).fold(0.0, f64::max);
    let [a, b, c] = rows.map(|(_, n)| n);
    let det = det3([a, b, c]);
    // the inverse's columns are the rows' pairwise cross products over the determinant
    let inv_frobenius = ((b.cross(c)).length().powi(2)
        + (c.cross(a)).length().powi(2)
        + (a.cross(b)).length().powi(2))
    .sqrt()
        / det.abs();
    (x, inv_frobenius * 3.0_f64.sqrt() * residual)
}

/// The exact corner of three analytic surfaces near `start` (closed-form distances).
fn exact_corner(surfaces: &[AnalyticSupport; 3], start: Point3) -> (Point3, f64) {
    newton_corner(|x| surfaces.map(|s| analytic_distance(&s, x)), start)
}

fn counts(b: &Brep) -> (usize, usize, usize) {
    (
        b.face_list().len(),
        b.edge_list().len(),
        b.vertex_list().len(),
    )
}

#[test]
fn wr_410_extrude_holes_inset_vertices_meet_their_faces() {
    let mut k = Kernel::default();
    let Some(b) = rounded_inset(
        &mut k,
        "Boolean/extrudeHoles1.smb",
        0,
        24.7,
        0.005_957_493_548_898_4,
    ) else {
        return;
    };
    assert_eq!(counts(&b), (23, 45, 25), "faces, edges, vertices");
    // lo.x and lo.y: one vertex; first crossing 2.75e-7 off a face
    let v = extreme_vertex(&b, 0, false);
    assert_eq!(
        v,
        extreme_vertex(&b, 1, false),
        "lo.x and lo.y share a vertex"
    );
    let gap = face_gap(&k, &b, v);
    assert!(gap <= 1e-11, "lo.x/lo.y vertex {gap} off its faces");
    // lo.z: first crossing 3.3e-13
    let gap = face_gap(&k, &b, extreme_vertex(&b, 2, false));
    assert!(gap <= 1e-13, "lo.z vertex {gap} off its faces");
    // hi.y: its faces lie on a plane, a cone and a sphere, whose exact common point
    // (closed forms) is 1.44e-6 from the first crossing's vertex, 1.20e-6 in y
    let v = extreme_vertex(&b, 1, true);
    let p = vertex::point(&b, v).unwrap();
    let mut surfaces: Vec<AnalyticSupport> = Vec::new();
    for f in vertex::faces(&b, v).unwrap() {
        let s = b.face(f).unwrap().surface.as_ref().unwrap();
        let a = AnalyticSupport::of(s).unwrap();
        if !surfaces.contains(&a) {
            surfaces.push(a);
        }
    }
    let surfaces: [AnalyticSupport; 3] = surfaces.try_into().unwrap();
    let (corner, error) = exact_corner(&surfaces, p);
    assert!(error <= 1e-10, "the corner's own error bound {error}");
    let off = p.distance(corner);
    assert!(
        off <= 6e-7 - error,
        "hi.y vertex {off} from the exact corner"
    );
    let off_y = (p.y - corner.y).abs();
    assert!(
        off_y <= 2e-7 - error,
        "hi.y {off_y} from the exact corner's y"
    );
    // hi.z: its faces lie on one sphere and one surface of revolution, and one of its
    // edges is an exact ellipse, the sphere's section by a plane (the trace of a consumed
    // face), so the vertex's exact place is the common point of the sphere, that plane and
    // the revolution, 1.88e-6 below the first crossing's z (main -2.9391047867639335). Its
    // other edges are fits: edge 125, a fit on the revolution, passes 3.63e-6 from that
    // exact point, so its gap to the vertex grows, 6.83e-7 -> 2.85e-6, as the vertex moves
    // to that point (the fit, not the vertex, is off; the largest edge gap falls 4.70e-6 ->
    // 2.85e-6).
    let v = extreme_vertex(&b, 2, true);
    let p = vertex::point(&b, v).unwrap();
    let gap = face_gap(&k, &b, v);
    assert!(gap <= 8e-7, "hi.z vertex {gap} off its faces");
    let mut sphere = None;
    let mut revolution = None;
    for f in vertex::faces(&b, v).unwrap() {
        let s = b.face(f).unwrap().surface.as_ref().unwrap();
        match AnalyticSupport::of(s) {
            Some(a @ AnalyticSupport::Sphere { .. }) => sphere = Some(a),
            None => revolution = Some(s),
            Some(a) => panic!("hi.z face on {a:?}"),
        }
    }
    let (sphere, revolution) = (sphere.unwrap(), revolution.unwrap());
    let mut plane = None;
    for e in vertex::edges(&b, v, None).unwrap() {
        if let CurveKind::Ellipse(el) = edge::curve(&b, e).unwrap().unwrap().kind() {
            let c = el.canonical();
            let n = c
                .position
                .x_axis()
                .cross(c.position.y_axis())
                .unitize()
                .unwrap();
            plane = Some((c.position.origin(), n));
        }
    }
    let (origin, normal) = plane.unwrap();
    // the revolution's signed distance: to its closest point, along its unit normal there
    let revolution_distance = |x: Point3| -> (f64, Vec3) {
        let cp = surface_closest_point(&k, revolution, x).unwrap();
        let n = revolution
            .evaluate_normal(&k, cp.uvs[0], Side::Left, Side::Left)
            .unwrap()
            .unitize()
            .unwrap();
        ((x - cp.closest.point).dot(n), n)
    };
    let (corner, error) = newton_corner(
        |x| {
            [
                ((x - origin).dot(normal), normal),
                analytic_distance(&sphere, x),
                revolution_distance(x),
            ]
        },
        p,
    );
    assert!(error <= 1e-10, "the corner's own error bound {error}");
    let off_z = (p.z - corner.z).abs();
    assert!(
        off_z <= 1.2e-6 - error,
        "hi.z {off_z} from the exact corner's z"
    );
    let off = p.distance(corner);
    assert!(
        off <= 1.2e-6 - error,
        "hi.z vertex {off} from the exact corner"
    );
}

#[test]
fn wr_410_problem_shell_inset_vertices_meet_their_faces() {
    let mut k = Kernel::default();
    let Some(b) = rounded_inset(
        &mut k,
        "Shell/problemShell10.smb",
        1,
        12.7,
        0.001_798_855_365_225,
    ) else {
        return;
    };
    assert_eq!(counts(&b), (6, 12, 8), "faces, edges, vertices");
    // the top vertex (first crossing 9.73e-6 off a face, 1.03e-5 off an edge)
    let v = extreme_vertex(&b, 2, true);
    let p = vertex::point(&b, v).unwrap();
    assert!(
        (p.x + 50.8).abs() < 1e-9 && (p.y + 28.575).abs() < 1e-9,
        "top vertex {p:?}"
    );
    let gap = face_gap(&k, &b, v);
    assert!(gap <= 1e-12, "top vertex {gap} off its faces");
    let gap = edge_gap(&k, &b, v);
    assert!(gap < 1e-5, "top vertex {gap} off its edges");
    // every vertex lies within its tolerance of its faces; the one at (-50.8, -114.3)
    // keeps its first placement, 7.49e-7 off a face (its move is not certified)
    for &v in b.vertex_list() {
        let gap = face_gap(&k, &b, v);
        assert!(gap <= 1e-6, "vertex {v:?} {gap} off its faces");
    }
}
