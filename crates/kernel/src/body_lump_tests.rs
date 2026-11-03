use super::*;
use crate::{BoolOp, boolean, box_solid, measure, sphere, step_export, step_import};

fn cube(half: f64) -> Body {
    box_solid(Vec3::new(-half, -half, -half), Vec3::new(half, half, half)).unwrap()
}

fn nested_island() -> Body {
    let hollow = boolean(&cube(50.0), &cube(40.0), BoolOp::Cut).unwrap().unwrap();
    // This order puts the containing outer shell ahead of the disconnected island.
    let joined = boolean(&cube(20.0), &hollow, BoolOp::Union).unwrap().unwrap();
    boolean(&joined, &cube(10.0), BoolOp::Cut).unwrap().unwrap()
}

fn assert_material(body: &Body, expected: f64, probes: &[(Vec3, bool)]) {
    assert!(body.validity().is_empty(), "{:?}", body.validity());
    let mesh = body.mesh(&crate::MeshTolerance::chord(0.01)).unwrap();
    assert!((mesh.measure().volume - expected).abs() < expected * 1e-6, "{} vs {expected}", mesh.measure().volume);
    for (p, want) in probes {
        assert_eq!(mesh.contains(*p), *want, "material at {p:?}");
    }
}

fn check_nested(body: &Body) {
    let probes = [(Vec3::ZERO, false), (Vec3::X * 15.0, true), (Vec3::X * 30.0, false), (Vec3::X * 45.0, true)];
    assert_material(body, 544_000.0, &probes);
    let lumps = body.lumps().unwrap();
    assert_eq!(lumps.len(), 2);
    let mut volumes: Vec<_> = lumps.iter().map(|b| measure(b).unwrap().volume).collect();
    volumes.sort_by(f64::total_cmp);
    for (got, want) in volumes.iter().zip([56_000.0, 488_000.0]) {
        assert!((got - want).abs() < 1e-6, "{volumes:?}");
    }
    let meshes: Vec<_> = lumps.iter().map(|b| b.mesh(&crate::MeshTolerance::chord(0.01)).unwrap()).collect();
    for (p, want) in probes {
        assert_eq!(meshes.iter().filter(|m| m.contains(p)).count(), usize::from(want), "lump material at {p:?}");
    }
}

/// Robustness seed 15, shrunk: a sphere cut at a corner of a part with a sealed internal void far
/// from the sphere. The same cut on the part without the void works; with it, the boolean failed
/// ("no result", then "the exact fallback … takes planar faces only"). A cut from a box or a
/// cylinder on the voided part worked.
#[test]
fn sphere_cut_far_from_a_void_keeps_the_void() {
    let block = box_solid(Vec3::ZERO, Vec3::new(40.0, 40.0, 40.0)).unwrap();
    let cavity = box_solid(Vec3::new(25.0, 25.0, 25.0), Vec3::new(30.0, 30.0, 30.0)).unwrap();
    let hollow = boolean(&block, &cavity, BoolOp::Cut).unwrap().unwrap();
    assert_eq!(hollow.shell_count(), 2, "the cavity is a second shell");
    let ball = sphere(Vec3::new(0.0, 0.0, 40.0), 8.0).unwrap();
    let cut = boolean(&hollow, &ball, BoolOp::Cut).unwrap().unwrap();
    // The block, less the cavity, less the eighth of the ball inside the corner.
    let want = 64_000.0 - 125.0 - std::f64::consts::PI * 512.0 / 6.0;
    let got = measure(&cut).unwrap().volume;
    assert!((got - want).abs() < want * 1e-3, "volume {got}, want {want}");
    assert_eq!(cut.shell_count(), 2, "the cavity stays a sealed second shell");
    let mesh = cut.mesh(&crate::MeshTolerance::chord(0.01)).unwrap();
    for (p, material) in [(Vec3::new(27.5, 27.5, 27.5), false), (Vec3::new(1.0, 1.0, 39.0), false), (Vec3::new(20.0, 20.0, 20.0), true)] {
        assert_eq!(mesh.contains(p), material, "material at {p:?}");
    }
}

#[test]
fn nested_cavities_stay_with_their_material_islands() {
    let body = nested_island();
    check_nested(&body);
    let before = step_export(&[&body], "nested cavities").unwrap();
    assert_eq!(body.shell_count(), 4);
    assert_eq!(step_export(&[&body], "nested cavities").unwrap(), before);
}

#[test]
fn nested_voids_survive_step_round_trip_and_further_cuts() {
    let body = nested_island();
    let text = step_export(&[&body], "nested cavities").unwrap();
    let imported = step_import(&text).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let mut lumps = Vec::new();
    for imported in imported.bodies {
        lumps.extend(imported.body.lumps().unwrap());
    }
    assert_eq!(lumps.len(), 2);
    let mut volumes: Vec<_> = lumps.iter().map(|b| measure(b).unwrap().volume).collect();
    volumes.sort_by(f64::total_cmp);
    for (got, want) in volumes.iter().zip([56_000.0, 488_000.0]) {
        assert!((got - want).abs() < 1e-3, "{volumes:?}");
    }
    let opening = box_solid(Vec3::new(5.0, -2.0, -2.0), Vec3::new(25.0, 2.0, 2.0)).unwrap();
    let mut cut_volume = 0.0;
    for lump in &lumps {
        if let Some(cut) = boolean(lump, &opening, BoolOp::Cut).unwrap() {
            cut_volume += measure(&cut).unwrap().volume;
            assert!(!cut.mesh(&crate::MeshTolerance::chord(0.01)).unwrap().contains(Vec3::X * 15.0));
        }
    }
    // Only x=10..20 is material along this 4×4 opening: 160 mm³ removed.
    assert!((cut_volume - 543_840.0).abs() < 1e-3, "{cut_volume}");
}

#[test]
fn curved_internal_void_and_disconnected_material_keep_analytic_volume() {
    let hollow = boolean(&cube(20.0), &sphere(Vec3::ZERO, 10.0).unwrap(), BoolOp::Cut).unwrap().unwrap();
    let island = sphere(Vec3::ZERO, 4.0).unwrap();
    let joined = boolean(&island, &hollow, BoolOp::Union).unwrap().unwrap();
    let reverse = boolean(&hollow, &island, BoolOp::Union).unwrap().unwrap();
    assert!((measure(&joined).unwrap().volume - measure(&reverse).unwrap().volume).abs() < 1e-4);
    let body = boolean(&joined, &sphere(Vec3::ZERO, 2.0).unwrap(), BoolOp::Cut).unwrap().unwrap();
    let lumps = body.lumps().unwrap();
    assert_eq!(lumps.len(), 2);
    let mut volumes: Vec<_> = lumps.iter().map(|b| measure(b).unwrap().volume).collect();
    volumes.sort_by(f64::total_cmp);
    let pi = std::f64::consts::PI;
    let want = [4.0 / 3.0 * pi * (64.0 - 8.0), 64_000.0 - 4.0 / 3.0 * pi * 1000.0];
    for (got, want) in volumes.iter().zip(want) {
        assert!((got - want).abs() < want * 0.002, "{got} vs {want}");
    }
    let meshes: Vec<_> = lumps.iter().map(|b| b.mesh(&crate::MeshTolerance::chord(0.01)).unwrap()).collect();
    for (p, want) in [(Vec3::ZERO, 0), (Vec3::X * 3.0, 1), (Vec3::X * 7.0, 0), (Vec3::X * 15.0, 1)] {
        assert_eq!(meshes.iter().filter(|m| m.contains(p)).count(), want);
    }
    let imported = step_import(&step_export(&[&body], "curved nested cavities").unwrap()).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    // The two pieces are written as two solids (each with its own void), so they come back as
    // two bodies.
    let imported: Vec<_> = imported.bodies.into_iter().map(|b| b.body).collect();
    assert_eq!(imported.len(), 2);
    let imported_meshes: Vec<_> = imported.iter().map(|b| b.mesh(&crate::MeshTolerance::chord(0.01)).unwrap()).collect();
    for (p, want) in [(Vec3::ZERO, 0), (Vec3::X * 3.0, 1), (Vec3::X * 7.0, 0), (Vec3::X * 15.0, 1)] {
        assert_eq!(imported_meshes.iter().filter(|m| m.contains(p)).count(), want);
    }
}

#[test]
fn translated_and_scaled_nested_voids_keep_material_membership() {
    let body = nested_island();
    for scale in [0.01, 0.25, 2.0] {
        let origin = Vec3::new(1234.0, -987.0, 765.0);
        let transformed = crate::transform_matrix(
            &body,
            [[scale, 0.0, 0.0, 0.0], [0.0, scale, 0.0, 0.0], [0.0, 0.0, scale, 0.0], [origin.x, origin.y, origin.z, 1.0]],
        )
        .unwrap();
        let lumps = transformed
            .lumps()
            .unwrap_or_else(|e| panic!("scale={scale}: {e}; bounds={:?}", transformed.mesh(&crate::MeshTolerance::chord(1e-4)).unwrap().bounds()));
        assert_eq!(lumps.len(), 2);
        let meshes: Vec<_> = lumps.iter().map(|b| b.mesh(&crate::MeshTolerance::chord((scale * 0.01).max(1e-5))).unwrap()).collect();
        for (x, want) in [(0.0, 0), (15.0, 1), (30.0, 0), (45.0, 1)] {
            assert_eq!(meshes.iter().filter(|m| m.contains(origin + Vec3::X * (x * scale))).count(), want);
        }
    }
}

#[test]
fn spheres_apart_join_as_two_lumps() {
    let direction = Vec3::new(1.0, 2.0, 3.0).normalized().unwrap();
    let a = sphere(Vec3::ZERO, 10.0).unwrap();
    let far = sphere(direction * 21.0, 10.0).unwrap();
    let safe = crate::ops::join_pieces(vec![a, far]).unwrap().unwrap();
    assert_eq!(safe.lumps().unwrap().len(), 2);
}

fn certified_lump_witness() -> Body {
    use solidcraft_geom::{Loop2, Plane, Region2, Vec2};
    // The current trimmed lower prism: a rectangular tool with a circular stop
    // removed. Its cap footprint differs materially from the original sketch.
    let region = Region2 {
        outer: Loop2::polygon(&[Vec2::new(20., 20.), Vec2::new(30., 20.), Vec2::new(30., 30.), Vec2::new(20., 30.)]),
        holes: vec![Loop2::circle(Vec2::new(25., 25.), 2.)],
    };
    let lower = crate::extrude(&Plane::XY, &[region], 0., 1.).unwrap().pop().unwrap();
    let named = box_solid(Vec3::new(40., 20., 5.), Vec3::new(42., 23., 7.)).unwrap().with_box_certificate(202).unwrap();
    let unknown = box_solid(Vec3::new(50., 20., 5.), Vec3::new(51., 21., 6.)).unwrap().with_face_sources(None);
    // Deliberately put the larger named box first, then the trimmed tool. An
    // index-based copy of the source table would attach certificates wrongly.
    let inputs = [&named, &unknown, &lower];
    let sources: Vec<Option<FaceSource>> =
        inputs.iter().flat_map(|b| b.face_sources().map_or_else(|| vec![None; b.face_count()], |s| s.to_vec())).collect();
    let records: Vec<&solidcraft_geom::brep::Brep> = inputs.iter().map(|b| b.record()).collect();
    let part = solidcraft_geom::brep::part::compound(&records).unwrap();
    let body = Body::from_record(part.brep).unwrap();
    let table = part.faces.iter().map(|f| sources.get(*f as usize).cloned().flatten()).collect();
    body.with_face_sources(Some(std::sync::Arc::new(table)))
}

#[test]
fn grouping_lumps_preserves_only_unchanged_face_certificates_and_trimmed_caps() {
    let body = certified_lump_witness();
    // A face is known by its surface and parameter box (no two faces here share both).
    let key = |b: &Body, i: usize| {
        let f = &b.record().faces[i];
        format!("{:?} {:?}", f.surface, f.domain)
    };
    let expected: std::collections::HashMap<_, _> =
        (0..body.face_count()).zip(body.face_sources().unwrap()).map(|(i, source)| (key(&body, i), (body.face_forward(i), source.clone()))).collect();
    let before = step_export(&[&body], "source").unwrap();
    let lumps = body.lumps().unwrap();
    assert_eq!(lumps.len(), 3);
    let mut total_faces = 0;
    let mut raw_caps = 0;
    let mut named_faces = 0;
    let mut unknown_faces = 0;
    for lump in &lumps {
        assert_eq!(lump.face_sources().unwrap().len(), lump.face_count());
        for (i, source) in lump.face_sources().unwrap().iter().enumerate() {
            let (orientation, original) = expected.get(&key(lump, i)).expect("grouping must retain the original faces");
            assert_eq!(*orientation, lump.face_forward(i));
            assert_eq!(source, original);
            match source {
                Some(FaceSource::ExtrudeCap { region, high, plane }) => {
                    assert_eq!(region.holes.len(), 1);
                    assert!((region.area() - (100. - 4. * std::f64::consts::PI)).abs() < 1e-10);
                    assert!((plane.origin.z - if *high { 1. } else { 0. }).abs() < 1e-10);
                    raw_caps += 1;
                }
                Some(FaceSource::Box { producer, .. }) => {
                    assert_eq!(*producer, Some(202));
                    named_faces += 1;
                }
                None => unknown_faces += 1,
                _ => {}
            }
            total_faces += 1;
        }
        let mesh = lump.mesh(&crate::MeshTolerance::chord(1e-5)).unwrap();
        if mesh.contains(Vec3::new(21., 21., 0.5)) {
            assert!(!mesh.contains(Vec3::new(25., 25., 0.5)), "trimmed stop hole must not refill");
            assert!((mesh.measure().volume - (100. - 4. * std::f64::consts::PI)).abs() < 0.002);
        }
    }
    assert_eq!(total_faces, body.face_count());
    assert_eq!(raw_caps, 2);
    assert_eq!(named_faces, 6);
    assert_eq!(unknown_faces, 6);
    assert_eq!(step_export(&[&body], "source").unwrap(), before);
}

#[test]
fn grouping_lumps_does_not_infer_absent_or_misaligned_sources() {
    let body = certified_lump_witness();
    let absent = body.clone().with_face_sources(None);
    assert!(absent.lumps().unwrap().iter().all(|b| b.face_sources().is_none()));
    let malformed = body.with_face_sources(Some(std::sync::Arc::new(vec![None])));
    assert!(malformed.lumps().unwrap_err().to_string().contains("face source table"));
}

/// A long thin cylinder (a through hole's tool) far from the origin, its seam turned: its
/// vertices span only its length, so the coarse orientation mesh can give it a negative volume.
/// It must still come out outward, or a Cut with it removes the wrong side (#559).
#[test]
fn a_long_thin_cylinder_comes_out_outward() {
    use solidcraft_geom::{Loop2, Plane, Region2, Vec2, Vec3};
    let plane = Plane::from_normal(Vec3::new(0., 24., 30.), Vec3::Z).unwrap();
    for (lo, seam) in [(-181.0, 0.613), (-100.0, 0.613), (-10.0, 0.613), (-181.0, 0.0)] {
        let disc = Region2 { outer: Loop2::circle_from(Vec2::ZERO, 0.5, seam), holes: vec![] };
        let tool = crate::extrude(&plane, &[disc], lo, 0.9).unwrap().pop().unwrap();
        let volume = tool.mesh(&crate::MeshTolerance::chord(0.01)).unwrap().measure().volume;
        let want = std::f64::consts::PI * 0.25 * (0.9 - lo);
        assert!((volume - want).abs() < want * 0.02, "{lo} {seam}: {volume} vs {want}");
    }
}

/// Exact point classification: a box's inside, outside, faces, edges and corners; a cylinder's
/// curved wall within its tolerance (on it), 5 µm inside it (inside: the old mesh test, at a
/// chord of 2e-3 of the size, missed this band) and just beyond; a cavity is outside.
#[test]
fn points_classify_exactly_against_the_body() {
    use crate::{PointClass::*, cylinder};
    let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
    for (p, want) in [
        (Vec3::new(5.0, 10.0, 15.0), Inside),
        (Vec3::new(5.0, 10.0, 31.0), Outside),
        (Vec3::new(5.0, 10.0, 30.0 + 1e-4), Outside),
        (Vec3::new(5.0, 10.0, 30.0 - 1e-4), Inside),
        (Vec3::new(5.0, 10.0, 30.0), OnBoundary),
        (Vec3::new(10.0, 10.0, 30.0), OnBoundary),
        (Vec3::new(10.0, 20.0, 30.0), OnBoundary),
        (Vec3::ZERO, OnBoundary),
    ] {
        assert_eq!(b.classify_point(p).unwrap(), want, "{p:?}");
    }
    assert!(b.classify_point(Vec3::new(f64::NAN, 0.0, 0.0)).is_err());

    let c = cylinder(Vec3::ZERO, Vec3::Z, 5.0, 10.0).unwrap();
    let tol = c.record().tolerance;
    let at = |r: f64| Vec3::new(r * std::f64::consts::FRAC_1_SQRT_2, r * std::f64::consts::FRAC_1_SQRT_2, 5.0);
    assert_eq!(c.classify_point(at(5.0 + 0.5 * tol)).unwrap(), OnBoundary);
    assert_eq!(c.classify_point(at(5.0 - 0.5 * tol)).unwrap(), OnBoundary);
    assert_eq!(c.classify_point(at(5.0 - 5e-3)).unwrap(), Inside);
    assert_eq!(c.classify_point(at(5.0 + 1e-4)).unwrap(), Outside);
    assert_eq!(c.classify_point(Vec3::new(0.0, 0.0, 10.0)).unwrap(), OnBoundary);

    let hollow = boolean(&cube(50.0), &cube(40.0), BoolOp::Cut).unwrap().unwrap();
    assert_eq!(hollow.classify_point(Vec3::ZERO).unwrap(), Outside, "the cavity");
    assert_eq!(hollow.classify_point(Vec3::X * 45.0).unwrap(), Inside);
    assert_eq!(hollow.classify_point(Vec3::X * 40.0).unwrap(), OnBoundary, "the cavity's wall");
}

/// A mesh body's triangles are its shape: on them within the record's tolerance, else inside or
/// outside by the rays' crossings.
#[test]
fn points_classify_against_a_mesh_body() {
    use crate::PointClass::*;
    let p = [Vec3::ZERO, Vec3::X * 10.0, Vec3::Y * 10.0, Vec3::Z * 10.0];
    let m = crate::mesh_body(&p, &[[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]]).unwrap();
    assert_eq!(m.classify_point(Vec3::new(1.0, 1.0, 1.0)).unwrap(), Inside);
    assert_eq!(m.classify_point(Vec3::new(5.0, 5.0, 5.0)).unwrap(), Outside);
    assert_eq!(m.classify_point(Vec3::new(1.0, 1.0, 0.0)).unwrap(), OnBoundary);
    assert_eq!(m.classify_point(Vec3::X * 10.0).unwrap(), OnBoundary);
}
