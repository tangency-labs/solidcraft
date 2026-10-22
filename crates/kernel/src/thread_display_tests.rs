//! Issue 1289: the display mesh of a modelled screw thread (helical B-spline flanks).

use solidcraft_geom::tess::TessPlan;
use solidcraft_geom::{Mesh, Vec3};

use std::f64::consts::{PI, TAU};
use std::sync::Arc;

use crate::{Body, DISPLAY_ANGLE, EdgeSpec, FaceSpec, SurfSpec, sew};

/// A helix about Z of radius `r` from angle `a0` to `a1`, rising `pitch` per turn from `z0` at
/// angle 0, as cubic Bézier pieces of at most `TAU / 16`.
fn helix(r: f64, z0: f64, pitch: f64, a0: f64, a1: f64) -> EdgeSpec {
    let n = (((a1 - a0).abs() / (TAU / 16.0)).ceil() as usize).max(1);
    let h = (a1 - a0) / n as f64;
    let k = 4.0 / 3.0 * (h / 4.0).tan();
    let at = |a: f64| Vec3::new(r * a.cos(), r * a.sin(), z0 + pitch * a / TAU);
    let rise = Vec3::new(0.0, 0.0, pitch * h / TAU / 3.0);
    let turn = |a: f64| Vec3::new(-a.sin(), a.cos(), 0.0) * (r * k);
    let mut ctrl = vec![at(a0)];
    for i in 0..n {
        let (s, e) = (a0 + h * i as f64, a0 + h * (i + 1) as f64);
        ctrl.extend([at(s) + turn(s) + rise, at(e) - turn(e) - rise, at(e)]);
    }
    EdgeSpec::Chain { degree: 3, ctrl: ctrl.into() }
}

fn rev(e: &EdgeSpec) -> EdgeSpec {
    match e {
        EdgeSpec::Chain { degree, ctrl } => EdgeSpec::Chain { degree: *degree, ctrl: ctrl.iter().rev().copied().collect::<Arc<[Vec3]>>() },
        EdgeSpec::Line { a, b } => EdgeSpec::Line { a: *b, b: *a },
        EdgeSpec::Arc { a, b, mid } => EdgeSpec::Arc { a: *b, b: *a, mid: *mid },
    }
}

fn line(a: Vec3, b: Vec3) -> EdgeSpec {
    EdgeSpec::Line { a, b }
}

fn ends(e: &EdgeSpec) -> (Vec3, Vec3) {
    match e {
        EdgeSpec::Chain { ctrl, .. } => (ctrl[0], ctrl[ctrl.len() - 1]),
        EdgeSpec::Line { a, b } | EdgeSpec::Arc { a, b, .. } => (*a, *b),
    }
}

/// A full circle about Z at height `z`, as two arcs.
fn circle(r: f64, z: f64) -> Vec<EdgeSpec> {
    let p = |a: f64| Vec3::new(r * a.cos(), r * a.sin(), z);
    vec![EdgeSpec::Arc { a: p(0.3), b: p(0.3 + PI), mid: p(0.3 + PI / 2.0) }, EdgeSpec::Arc { a: p(0.3 + PI), b: p(0.3), mid: p(0.3 + 1.5 * PI) }]
}

/// A threaded shaft the way a modelled thread usually comes out of a CAD system: an ISO metric
/// ridge (60° flanks, P/8 crest) of `turns` turns wound on a core of the minor diameter, the
/// flanks ruled B-spline surfaces between helices, the crest and the core cylinders trimmed by
/// those helices.
fn ridge_thread(major: f64, pitch: f64, turns: f64) -> Body {
    let h = pitch * 3f64.sqrt() / 2.0;
    let (rc, rm) = (major / 2.0 - 5.0 * h / 8.0, major / 2.0);
    let (foot, crest) = ((pitch - pitch / 4.0) / 2.0, pitch / 16.0);
    let z0 = pitch;
    let len = z0 + pitch * turns + pitch;
    let end = TAU * turns;
    let fl = helix(rc, z0 - foot, pitch, 0.0, end);
    let fu = helix(rc, z0 + foot, pitch, 0.0, end);
    let cl = helix(rm, z0 - crest, pitch, 0.0, end);
    let cu = helix(rm, z0 + crest, pitch, 0.0, end);
    let (fl0, fl1) = ends(&fl);
    let (fu0, fu1) = ends(&fu);
    let (cl0, cl1) = ends(&cl);
    let (cu0, cu1) = ends(&cu);
    let at = |r: f64, a: f64, z: f64| Vec3::new(r * a.cos(), r * a.sin(), z);
    let mid = z0 + pitch / 2.0;
    let out = |a: f64| Vec3::new(a.cos(), a.sin(), 0.0);
    let flank = 0.5f64;
    let faces = vec![
        FaceSpec { loops: vec![circle(rc, 0.0).iter().rev().map(rev).collect()], surface: SurfSpec::Plane, probe: None },
        FaceSpec { loops: vec![circle(rc, len)], surface: SurfSpec::Plane, probe: None },
        FaceSpec {
            loops: vec![circle(rc, 0.0), circle(rc, len).iter().rev().map(rev).collect(), vec![line(fl0, fu0), fu.clone(), line(fu1, fl1), rev(&fl)]],
            surface: SurfSpec::Revolved {
                a: Vec3::new(rc, 0.0, -1.0),
                b: Vec3::new(rc, 0.0, len + 1.0),
                axis_origin: Vec3::ZERO,
                axis: Vec3::Z,
                angle: TAU,
            },
            probe: Some((at(rc, PI, z0 * 0.5), out(PI))),
        },
        FaceSpec {
            loops: vec![vec![fl.clone(), line(fl1, cl1), rev(&cl), line(cl0, fl0)]],
            surface: SurfSpec::Ruled { a: fl.clone(), b: cl.clone() },
            probe: Some((at((rc + rm) / 2.0, PI, mid - (foot + crest) / 2.0), out(PI) * flank - Vec3::Z * 0.866)),
        },
        FaceSpec {
            loops: vec![vec![cu.clone(), line(cu1, fu1), rev(&fu), line(fu0, cu0)]],
            surface: SurfSpec::Ruled { a: fu.clone(), b: cu.clone() },
            probe: Some((at((rc + rm) / 2.0, PI, mid + (foot + crest) / 2.0), out(PI) * flank + Vec3::Z * 0.866)),
        },
        FaceSpec {
            loops: vec![vec![cl.clone(), line(cl1, cu1), rev(&cu), line(cu0, cl0)]],
            surface: SurfSpec::Revolved {
                a: Vec3::new(rm, 0.0, -1.0),
                b: Vec3::new(rm, 0.0, len + 1.0),
                axis_origin: Vec3::ZERO,
                axis: Vec3::Z,
                angle: TAU,
            },
            probe: Some((at(rm, PI, mid), out(PI))),
        },
        FaceSpec { loops: vec![vec![line(fl0, cl0), line(cl0, cu0), line(cu0, fu0), line(fu0, fl0)]], surface: SurfSpec::Plane, probe: None },
        FaceSpec { loops: vec![vec![line(fl1, fu1), line(fu1, cu1), line(cu1, cl1), line(cl1, fl1)]], surface: SurfSpec::Plane, probe: None },
    ];
    sew(&faces).unwrap()
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
/// their vertex normals; the largest gap between a triangle's centre and the surface at the
/// centre's parameters.
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
            let uv = t.iter().fold([0.0, 0.0], |m, &k| [m[0] + f.uv[k as usize][0] / 3.0, m[1] + f.uv[k as usize][1] / 3.0]);
            let mid = (pos(t[0]) + pos(t[1]) + pos(t[2])) * (1.0 / 3.0);
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
/// crest cylinders, trimmed by helices (and the core bounded by two circles without a seam
/// edge), had no display plan, and the reference mesher drew them with triangles across the
/// solid. They are now cut along a seam: the display mesh stays on the surfaces.
#[test]
fn thread_display_mesh_stays_on_its_surfaces() {
    for (name, b, d, p, turns) in threads() {
        let tol = tol(&b);
        let plan = b.mesh_plan(&crate::MeshTolerance { chord: tol, angle: Some(DISPLAY_ANGLE) }).unwrap_or_else(|e| panic!("{name}: no display plan: {e}"));
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
            for want in [rc, rm] {
                if [ra, rb, rcn].iter().all(|x| (x - want).abs() < 1e-4) {
                    assert!(centre > want - 2.0 * tol, "{name}: triangle {ti} on the r = {want} cylinder cuts in to {centre}");
                }
            }
            let end_disc = [a, b2, c].iter().all(|v| v.z.abs() < 1e-9) || [a, b2, c].iter().all(|v| (v.z - (turns + 2.0) * p).abs() < 1e-9);
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
        let plan = b.mesh_plan(&crate::MeshTolerance { chord: tol(&b), angle: Some(DISPLAY_ANGLE) }).unwrap_or_else(|e| panic!("{name}: no display plan: {e}"));
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
