//! Issue 1289: the display mesh of a modelled screw thread (helical B-spline flanks).

use solidcraft_geom::tess::TessPlan;
use solidcraft_geom::{Mesh, Vec3};

use std::f64::consts::{PI, TAU};

use crate::{Body, DISPLAY_ANGLE};

/// A threaded shaft the way the thread feature makes one: an ISO metric ridge (60° flanks, P/8
/// crest) of `turns` turns wound on a core of the minor diameter, the ridge SMLib's screw sweep
/// of its section (exact helices, the flanks and crest its screw walls, flat ends in the planes
/// through the axis where it starts and stops) joined to the core cylinder, which the flanks then
/// trim along helices. The section reaches a tenth of a pitch into the core along its flanks, so
/// the join cuts it at the core rather than meeting it face to face.
fn ridge_thread(major: f64, pitch: f64, turns: f64) -> Body {
    use solidcraft_geom::{Loop2, Plane, Region2, Vec2};
    let h = pitch * 3f64.sqrt() / 2.0;
    let (rc, rm) = (major / 2.0 - 5.0 * h / 8.0, major / 2.0);
    let (foot, crest) = ((pitch - pitch / 4.0) / 2.0, pitch / 16.0);
    let z0 = pitch;
    let len = z0 + pitch * turns + pitch;
    // The ridge's half width at radius r, along its flanks.
    let half = |r: f64| foot + (crest - foot) * (r - rc) / (rm - rc);
    let r0 = rc - 0.1 * pitch;
    let section = Loop2::polygon(&[Vec2::new(r0, -half(r0)), Vec2::new(rm, -crest), Vec2::new(rm, crest), Vec2::new(r0, half(r0))]);
    let plane = Plane { origin: Vec3::new(0.0, 0.0, z0), x: Vec3::X, y: Vec3::Z };
    let ridge = crate::sweep_helix(&plane, &Region2 { outer: section, holes: Vec::new() }, Vec3::ZERO, Vec3::Z, pitch, turns).unwrap();
    let core = Region2 { outer: Loop2::circle(Vec2::ZERO, rc), holes: Vec::new() };
    let core = crate::extrude(&Plane::XY, &[core], 0.0, len).unwrap().pop().unwrap();
    crate::boolean(&core, &ridge, crate::BoolOp::Union).unwrap().unwrap()
}

fn tol(b: &Body) -> f64 {
    (b.size() * 1e-3).max(1e-3)
}

struct Quality {
    tris: usize,
    slivers: usize,
    flipped: usize,
    worst_sag: f64,
    min_angle: f64,
}

/// Triangles thinner than 1e-3 of their longest side; triangles whose winding disagrees with
/// their vertex normals; the largest gap between a triangle's centre and the surface, at the
/// surface point nearest the centre (a few Newton steps from a corner's parameters: a closed
/// surface's parameters are kept within one period, so a triangle across its seam has corners
/// whose parameters lie a period apart, and their mean is nowhere near the triangle).
fn quality(plan: &TessPlan, mesh: &Mesh) -> Quality {
    let mut q = Quality { tris: mesh.triangles.len(), slivers: 0, flipped: 0, worst_sag: 0.0, min_angle: 180.0 };
    for t in &mesh.triangles {
        let [a, b, c] = t.map(|i| mesh.positions[i as usize]);
        let n = (b - a).cross(c - a);
        let longest = (b - a).len().max((c - b).len()).max((a - c).len());
        let ang = |p: Vec3, q: Vec3, r: Vec3| {
            ((q - p).normalized().unwrap_or(Vec3::X)).dot((r - p).normalized().unwrap_or(Vec3::X)).clamp(-1.0, 1.0).acos()
        };
        let mina = ang(a, b, c).min(ang(b, c, a)).min(ang(c, a, b)).to_degrees();
        if mina < q.min_angle {
            q.min_angle = mina;
        }
        if n.len() < 1e-3 * longest * longest {
            q.slivers += 1;
            continue;
        }
        let vn = t.iter().fold(Vec3::ZERO, |s, &i| s + mesh.normals[i as usize]);
        if n.dot(vn) < 0.0 {
            q.flipped += 1;
        }
    }
    for f in &plan.faces {
        let Some(s) = f.surface.and_then(|i| plan.surfaces.get(i as usize)) else { continue };
        let pos = |k: u32| f.fixed.get(k as usize).copied().unwrap_or_else(|| s.eval(f.uv[k as usize][0], f.uv[k as usize][1]).0);
        for t in &f.triangles {
            let mid = (pos(t[0]) + pos(t[1]) + pos(t[2])) * (1.0 / 3.0);
            let mut uv = f.uv[t[0] as usize];
            for _ in 0..8 {
                let Some((p, su, sv)) = s.derivs(uv[0], uv[1]) else { break };
                let d = mid - p;
                let (a, b, c) = (su.dot(su), su.dot(sv), sv.dot(sv));
                let det = a * c - b * b;
                if det.abs() < 1e-300 {
                    break;
                }
                let (x, y) = (su.dot(d), sv.dot(d));
                uv = [uv[0] + (c * x - b * y) / det, uv[1] + (a * y - b * x) / det];
            }
            let (on, n) = s.eval(uv[0], uv[1]);
            let sag = (on - mid).dot(n).abs();
            q.worst_sag = q.worst_sag.max(sag);
        }
    }
    q
}

/// Volume of the ridge thread: the core, and per turn of the ridge 2π ∫ r · width(r) dr (a
/// screw motion of a section in an axial plane).
fn ridge_volume(major: f64, pitch: f64, turns: f64) -> f64 {
    let h = pitch * 3f64.sqrt() / 2.0;
    let (rc, rm) = (major / 2.0 - 5.0 * h / 8.0, major / 2.0);
    let (foot, crest) = ((pitch - pitch / 4.0) / 2.0, pitch / 16.0);
    let len = pitch * turns + 2.0 * pitch;
    let n = 1000;
    let ridge: f64 = (0..n)
        .map(|i| rc + (rm - rc) * (i as f64 + 0.5) / n as f64)
        .map(|r| TAU * r * 2.0 * (foot + (crest - foot) * (r - rc) / (rm - rc)) * (rm - rc) / n as f64)
        .sum();
    PI * rc * rc * len + ridge * turns
}

/// The thread, as made and after a STEP round trip (which is how such a thread reaches us).
fn threads() -> Vec<(String, Body, f64, f64, f64)> {
    let mut out = Vec::new();
    for (d, p, turns) in [(10.0, 1.5, 6.0), (12.0, 1.75, 8.0)] {
        let b = ridge_thread(d, p, turns);
        let step = crate::step_import(&crate::step_export(&[&b], "thread").unwrap()).unwrap().bodies.remove(0).body;
        out.push((format!("M{d}x{p}"), b, d, p, turns));
        out.push((format!("M{d}x{p} from STEP"), step, d, p, turns));
    }
    out
}

/// Issue 1289: a modelled thread's faces turn several times around the shaft. The core and the
/// crest cylinders, trimmed by helices, had no display plan, and the reference mesher drew them
/// with triangles across the solid. They are now cut along a seam: the display mesh stays on
/// the surfaces.
#[test]
fn thread_display_mesh_stays_on_its_surfaces() {
    for (name, b, d, p, turns) in threads() {
        let tol = tol(&b);
        let plan =
            b.mesh_plan(&crate::MeshTolerance { chord: tol, angle: Some(DISPLAY_ANGLE) }).unwrap_or_else(|e| panic!("{name}: no display plan: {e}"));
        let mesh = plan.evaluate();
        let q = quality(&plan, &mesh);
        assert!(q.worst_sag <= 1.5 * tol, "{name}: a triangle lies {} mm off its surface (tolerance {tol})", q.worst_sag);
        // Every vertex of the cylinders on its cylinder, every triangle centre within the
        // tolerance inside it.
        let h = p * 3f64.sqrt() / 2.0;
        let (rc, rm) = (d / 2.0 - 5.0 * h / 8.0, d / 2.0);
        for (ti, t) in mesh.triangles.iter().enumerate() {
            let r = |v: Vec3| (v.x * v.x + v.y * v.y).sqrt();
            let [a, b2, c] = t.map(|i| mesh.positions[i as usize]);
            let (ra, rb, rcn) = (r(a), r(b2), r(c));
            let centre = r((a + b2 + c) * (1.0 / 3.0));
            // An end disc is flat: its triangles are its boundary's, and one with its three
            // corners on the rim (at r = rc) lies on the disc, not on a cylinder.
            let end_disc = [a, b2, c].iter().all(|v| v.z.abs() < 1e-9) || [a, b2, c].iter().all(|v| (v.z - (turns + 2.0) * p).abs() < 1e-9);
            for want in [rc, rm] {
                if !end_disc && [ra, rb, rcn].iter().all(|x| (x - want).abs() < 1e-4) {
                    assert!(centre > want - 2.0 * tol, "{name}: triangle {ti} on the r = {want} cylinder cuts in to {centre}");
                }
            }
            assert!(end_disc || (centre > rc - 2.0 * tol && centre < rm + 1e-6), "{name}: triangle {ti} at radius {centre}, outside {rc}…{rm}");
        }
        // A mesh within the tolerance of the surfaces is within area × tolerance of the volume.
        let m = mesh.measure();
        let want = ridge_volume(d, p, turns);
        assert!((m.volume - want).abs() < m.area * tol, "{name}: display mesh volume {}, the thread's {want}", m.volume);
        // The reference mesh (picking, measuring, the viewport without GPU tessellation) is the same.
        let reference = b.mesh(&crate::MeshTolerance { chord: tol, angle: Some(DISPLAY_ANGLE) }).unwrap();
        assert_eq!(reference.triangles.len(), mesh.triangles.len(), "{name}");
        let measured = b.mesh(&crate::MeshTolerance::chord(tol * 0.1)).unwrap().measure().volume;
        assert!((measured - want).abs() < want * 1e-3, "{name}: measured volume {measured}, the thread's {want}");
    }
}

/// Issue 1289: no triangle of the thread's display mesh is degenerate, a sliver or turned
/// against its surface's normal.
#[test]
fn thread_display_mesh_has_no_degenerate_or_flipped_triangles() {
    for (name, b, ..) in threads() {
        let plan = b
            .mesh_plan(&crate::MeshTolerance { chord: tol(&b), angle: Some(DISPLAY_ANGLE) })
            .unwrap_or_else(|e| panic!("{name}: no display plan: {e}"));
        let mesh = plan.evaluate();
        let q = quality(&plan, &mesh);
        assert!(q.tris > 1000, "{name}: {} triangles", q.tris);
        assert_eq!(q.slivers, 0, "{name}: degenerate triangles");
        assert_eq!(q.flipped, 0, "{name}: triangles facing against their normals");
        assert!(q.min_angle > 1.0, "{name}: a triangle with a {:.3} degree corner", q.min_angle);
    }
}

/// Writes the M10 thread as STEP to `$THREAD_STEP` (for looking at it in the app).
#[test]
#[ignore]
fn write_thread_step() {
    if let Ok(path) = std::env::var("THREAD_STEP") {
        std::fs::write(path, crate::step_export(&[&ridge_thread(10.0, 1.5, 6.0)], "M10x1.5 thread").unwrap()).unwrap();
    }
}
