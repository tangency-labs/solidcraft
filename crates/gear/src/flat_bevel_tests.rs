//! Bevel teeth end on their back and front cones (heel and toe), each member along its own
//! face: also on a nearly flat pitch cone (the bigger member of a set at a 100° to 150° shaft
//! angle, close to a crown gear), where a plane section square to the axis runs far past the
//! cones. Run with `--nocapture` for the numbers.

use crate::mesh3d::Solid;
use crate::mesh3d_tests::measure;
use crate::plan::{FLAT_CONE, Kind, Op, Spec, Step, faces_of, plan};

fn d(k: Kind) -> Spec {
    Spec::defaults_for(k)
}

/// Sets with one member (or both) flatter than 70°.
pub(crate) fn flat_sets() -> Vec<(&'static str, Spec)> {
    let m25 = |k: Kind, sigma: f64| Spec { module: 2.5, teeth: 17, teeth2: 34, shaft_angle: sigma, ..d(k) };
    vec![
        ("spiral 110deg 17:34 m2.5", m25(Kind::SpiralBevel, 110.0)),
        ("straight 110deg 17:34 m2.5", m25(Kind::Bevel, 110.0)),
        ("spiral 100deg 17:34 m2.5", m25(Kind::SpiralBevel, 100.0)),
        ("straight 100deg 17:34 m2.5", m25(Kind::Bevel, 100.0)),
        ("spiral 120deg 20:30", Spec { shaft_angle: 120.0, teeth: 20, teeth2: 30, ..d(Kind::SpiralBevel) }),
        ("straight 120deg 20:30 backlash", Spec { shaft_angle: 120.0, teeth: 20, teeth2: 30, backlash: 0.1, ..d(Kind::Bevel) }),
        (
            "spiral left 105deg 15:40 backlash",
            Spec { shaft_angle: 105.0, teeth: 15, teeth2: 40, spiral_angle: -25.0, backlash: 0.1, ..d(Kind::SpiralBevel) },
        ),
        ("straight 150deg 20:20 both flat", Spec { shaft_angle: 150.0, teeth: 20, teeth2: 20, ..d(Kind::Bevel) }),
        ("spiral 150deg 20:20 both flat", Spec { shaft_angle: 150.0, teeth: 20, teeth2: 20, spiral_angle: 30.0, ..d(Kind::SpiralBevel) }),
    ]
}

/// Each member's pitch cone angle (radians).
fn cones(s: &Spec) -> [f64; 2] {
    let sigma = s.shaft_angle.to_radians();
    let d1 = sigma.sin().atan2(f64::from(s.teeth2) / f64::from(s.teeth) + sigma.cos());
    [d1, sigma - d1]
}

#[test]
fn flat_bevels_mesh() {
    let rows: Vec<_> = flat_sets().iter().map(|(n, s)| measure(n, s)).collect();
    for r in &rows {
        r.show();
    }
    for r in &rows {
        r.check();
        assert!(!r.warnings.iter().any(|w| w.contains("80°")), "{}: {:?}", r.label, r.warnings);
    }
}

#[test]
fn bevels_with_their_own_faces_mesh() {
    let rows: Vec<_> = own_face_sets().iter().map(|(n, s)| measure(n, s)).collect();
    for r in &rows {
        r.show();
    }
    for r in &rows {
        r.check();
        assert!(!r.warnings.iter().any(|w| w.contains("80°")), "{}: {:?}", r.label, r.warnings);
    }
}

/// Sets whose members have faces of their own: another width, a heel in from the cone
/// distance (or out past it).
pub(crate) fn own_face_sets() -> Vec<(&'static str, Spec)> {
    vec![
        ("straight 20:30 gear 2 face 8", Spec { face_width2: 8.0, ..d(Kind::Bevel) }),
        ("straight 20:30 gear 2 face 16 heel in 2", Spec { face_width2: 16.0, face_offset2: 2.0, backlash: 0.1, ..d(Kind::Bevel) }),
        ("spiral 17:34 gear 2 face 9 heel in 1.5", Spec { face_width2: 9.0, face_offset2: 1.5, ..d(Kind::SpiralBevel) }),
        (
            "spiral 110deg 17:34 m2.5 gear 2 heel out 1",
            Spec { module: 2.5, teeth: 17, teeth2: 34, shaft_angle: 110.0, face_offset2: -1.0, backlash: 0.1, ..d(Kind::SpiralBevel) },
        ),
    ]
}

/// A member flatter than [`FLAT_CONE`] ends on its back cone (nothing past its heel along the
/// pitch element) and its front cone (nothing of the teeth nearer the apex than its toe), its
/// outer tip diameter (r + ha·cos δ) at its heel; the others end on the planes through the
/// pitch circles at their heel and toe.
#[test]
fn bevels_end_on_their_back_and_front_cones() {
    let mut sets = flat_sets();
    sets.extend(own_face_sets());
    sets.push(("straight default", d(Kind::Bevel)));
    sets.push(("spiral default", d(Kind::SpiralBevel)));
    sets.push(("straight 60deg 16:40", Spec { teeth: 16, teeth2: 40, shaft_angle: 60.0, ..d(Kind::Bevel) }));
    for (label, s) in sets {
        let p = plan(&s).unwrap_or_else(|e| panic!("{label}: {e}"));
        let m = s.module;
        let ha = s.rack.addendum * m * if s.kind == Kind::SpiralBevel { s.spiral_angle.to_radians().cos() } else { 1.0 };
        for (i, (body, delta)) in p.bodies.iter().zip(cones(&s)).enumerate() {
            let z = if i == 0 { s.teeth } else { s.teeth2 };
            let r = m * f64::from(z) / 2.0;
            let re = r / delta.sin();
            let face = faces_of(&s, re)[i];
            let (heel, toe) = (face.heel, face.toe());
            let h = re * delta.cos();
            let trims = body.steps.iter().filter(|st| matches!(st, Step::Revolve { op: Op::Intersect, .. })).count();
            if delta <= FLAT_CONE {
                // Flat ends on the planes through the heel's and the toe's pitch circles.
                assert_eq!(trims, 0, "{label} {}", body.name);
                let Step::Taper { z0, height, .. } = body.steps[0] else { panic!("{label}") };
                assert!((z0 - h * (1.0 - heel / re)).abs() < 1e-9 && (z0 + height - h * (1.0 - toe / re)).abs() < 1e-9, "{label} {}", body.name);
                continue;
            }
            assert_eq!(trims, 1, "{label} {}: {:?}", body.name, body.steps.len());
            let pts = Solid::new(body, 0.03 * m).material_points(0.1 * m);
            assert!(pts.len() > 1000, "{label} {}: {} points", body.name, pts.len());
            let (sd, cd) = delta.sin_cos();
            // Along the pitch element at each point's own azimuth.
            let along = |q: &[f64; 3]| q[0].hypot(q[1]) * sd - (q[2] - h) * cd;
            let most = pts.iter().map(along).fold(f64::MIN, f64::max);
            let tip = pts.iter().map(|q| q[0].hypot(q[1])).fold(0.0, f64::max);
            let want = heel / re * (r + ha * delta.cos());
            eprintln!(
                "{label:<36} {:<12} δ {:5.1}°  tip Ø {:8.3} (r + ha·cos δ: {:8.3}, {:+.3}%)  most along the element {:8.3} of {:8.3}",
                body.name,
                delta.to_degrees(),
                2.0 * tip,
                2.0 * want,
                100.0 * (tip / want - 1.0),
                most,
                heel
            );
            assert!((tip / want - 1.0).abs() < 0.01, "{label} {}: tip Ø {} against {}", body.name, 2.0 * tip, 2.0 * want);
            assert!(most <= heel + 1e-3 * m, "{label} {}: a tooth reaches {} along the pitch element, past the heel at {heel}", body.name, most);
            // Nothing of the teeth is nearer the apex than the front cone: what is (the body's
            // front face and its hub) is under the roots, no higher than the front face.
            let Some(Step::Revolve { profile, .. }) = body.steps.iter().find(|st| matches!(st, Step::Revolve { op: Op::Intersect, .. })) else {
                unreachable!()
            };
            let face = profile.last().map_or(0.0, |q| q.y);
            let inside = pts.iter().filter(|q| along(q) < toe - 1e-3 * m).map(|q| q[2]).fold(f64::MIN, f64::max);
            assert!(inside <= face + 1e-6, "{label} {}: material inside the front cone at z {inside}, above the front face at {face}", body.name);
        }
    }
}

/// The over 80° warning is gone, and a cone at 85° and under still builds.
#[test]
fn nearly_crown_bevels_plan() {
    for sigma in [100.0, 110.0, 115.0, 120.0] {
        for k in [Kind::Bevel, Kind::SpiralBevel] {
            let s = Spec { module: 2.5, teeth: 17, teeth2: 34, shaft_angle: sigma, ..d(k) };
            let delta2 = cones(&s)[1].to_degrees();
            let p = plan(&s);
            if delta2 > 85.0 {
                assert!(p.is_err(), "{k:?} {sigma}°: δ {delta2} should be refused");
                continue;
            }
            let p = p.unwrap_or_else(|e| panic!("{k:?} {sigma}°: {e}"));
            assert_eq!(p.bodies.len(), 2);
            assert!(!p.warnings.iter().any(|w| w.contains("crown")), "{k:?} {sigma}°: {:?}", p.warnings);
        }
    }
}

/// Gear 2's face may not miss gear 1's, nor reach too near the apex.
#[test]
fn faces_that_miss_are_refused() {
    for (label, s) in [
        ("heel past gear 1's toe", Spec { face_offset2: 13.0, ..d(Kind::Bevel) }),
        ("toe past gear 1's heel", Spec { face_offset2: -13.0, ..d(Kind::Bevel) }),
        ("toe near the apex", Spec { face_width2: 12.0, face_offset2: 22.0, ..d(Kind::SpiralBevel) }),
    ] {
        assert!(plan(&s).is_err(), "{label}");
    }
}

#[test]
#[ignore]
fn probe_flat() {
    let name = std::env::var("FLAT_CASE").unwrap_or_default();
    let extra = vec![("mitre 24:24", Spec { teeth: 24, teeth2: 24, ..d(Kind::Bevel) })];
    for (label, s) in flat_sets().into_iter().chain(own_face_sets()).chain(extra) {
        if label.contains(name.as_str()) {
            measure(label, &s).show();
        }
    }
}

/// Bevel planetary sets with flat planets (near a crown) and planets with
/// faces of their own.
pub(crate) fn planetary_sets() -> Vec<(&'static str, Spec)> {
    use crate::plan::BevelPlanetMode::{Differential, Single};
    let bp = d(Kind::BevelPlanetary);
    let set = |mode, angle: f64, zs: u32, zp: u32, n: u32| Spec {
        bevel_planet_mode: mode,
        shaft_angle: angle,
        teeth: zs,
        teeth2: zp,
        planets: n,
        ..bp.clone()
    };
    vec![
        ("bp single 100 12:30 x1 flat planet", set(Single, 100.0, 12, 30, 1)),
        ("bp diff 90 10:30 x2 flat planets spiral 30", Spec { spiral_angle: 30.0, ..set(Differential, 90.0, 10, 30, 2) }),
        ("bp single 90 16:10 x3 planets face 8 heel in 1", Spec { face_width2: 8.0, face_offset2: 1.0, ..set(Single, 90.0, 16, 10, 3) }),
    ]
}

#[test]
fn bevel_planetary_flat_and_own_faces_mesh() {
    for (label, s) in planetary_sets() {
        crate::bevel_planet_tests::prove(label, &s);
    }
}

/// Writes CLI scripts that build a bevel body's teeth (its taper and trim) from primitive
/// commands with the kernel's check on every step: `FLAT_DUMP=dir`.
#[test]
#[ignore]
fn dump_trim_scripts() {
    use crate::geom::{Outline, Seg};
    use crate::plan::{taper_sections, taper_turn};
    let Ok(dir) = std::env::var("FLAT_DUMP") else { return };
    let v3 = |v: [f64; 3]| format!("[{},{},{}]", v[0], v[1], v[2]);
    let p2 = |q: crate::geom::V2| format!("[{},{}]", q.x, q.y);
    for (name, s, i) in [
        ("st100g2", Spec { module: 2.5, teeth: 17, teeth2: 34, shaft_angle: 100.0, ..d(Kind::Bevel) }, 1usize),
        ("st90g1", d(Kind::Bevel), 0),
        ("sp110g2", Spec { module: 2.5, teeth: 17, teeth2: 34, shaft_angle: 110.0, ..d(Kind::SpiralBevel) }, 1),
        ("sp90g2", d(Kind::SpiralBevel), 1),
        ("st90g2face", Spec { face_width2: 8.0, face_offset2: 1.5, ..d(Kind::Bevel) }, 1),
    ] {
        let p = plan(&s).unwrap();
        let b = &p.bodies[i];
        let f = b.frame;
        let (x, y, z) = (f.x, f.y(), f.z);
        let mut cmds = vec![r#"{"command":"diagnostics.validation","params":{"mode":"each_step"}}"#.to_string()];
        let mut sketch = 0;
        let outline_cmds = |cmds: &mut Vec<String>, o: &Outline, h: f64| {
            cmds.push(format!(
                r#"{{"command":"sketch.create","params":{{"plane":{{"origin":{},"x_dir":{},"y_dir":{}}},"project_edges":false}}}}"#,
                v3(f.world([0.0, 0.0, h])),
                v3(x),
                v3(y)
            ));
            for sg in &o.segs {
                cmds.push(match sg {
                    Seg::Line(a, c) => format!(r#"{{"command":"sketch.line","params":{{"points":[{},{}]}}}}"#, p2(*a), p2(*c)),
                    Seg::Arc { c, a, b, ccw } => {
                        let (a, b) = if *ccw { (*a, *b) } else { (*b, *a) };
                        format!(r#"{{"command":"sketch.arc.center_point","params":{{"center":{},"start":{},"end":{}}}}}"#, p2(*c), p2(a), p2(b))
                    }
                    Seg::Spline(pts) => format!(
                        r#"{{"command":"sketch.spline.fit_point","params":{{"points":[{}]}}}}"#,
                        pts.iter().map(|q| p2(*q)).collect::<Vec<_>>().join(",")
                    ),
                    Seg::Circle(..) => panic!(),
                });
            }
            cmds.push(r#"{"command":"sketch.finish","params":{}}"#.to_string());
        };
        let Step::Taper { outline, z0, height, twist, scale, .. } = &b.steps[0] else { panic!() };
        // Experiments: FLAT_DENSE=k refits every spline through k times as many points.
        let dense: usize = std::env::var("FLAT_DENSE").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
        let lines = std::env::var("FLAT_LINES").is_ok();
        let outline = &if lines {
            Outline {
                segs: outline
                    .segs
                    .iter()
                    .flat_map(|sg| match sg {
                        Seg::Spline(pts) => pts.windows(2).map(|w| Seg::Line(w[0], w[1])).collect::<Vec<_>>(),
                        other => vec![other.clone()],
                    })
                    .collect(),
            }
        } else if dense > 1 {
            Outline {
                segs: outline
                    .segs
                    .iter()
                    .map(|sg| match sg {
                        Seg::Spline(pts) => {
                            let mut out = Vec::new();
                            crate::geom::natural_spline(pts, dense, &mut out);
                            out.dedup_by(|a, b| a.dist(*b) < 1e-12);
                            Seg::Spline(out)
                        }
                        other => other.clone(),
                    })
                    .collect(),
            }
        } else {
            outline.clone()
        };
        let n = if twist.abs() < 1e-12 {
            std::env::var("FLAT_N").ok().and_then(|v| v.parse().ok()).unwrap_or(2)
        } else {
            taper_sections(outline, *twist, *scale)
        };
        let mut ids = Vec::new();
        for k in 0..n {
            let t = k as f64 / (n - 1) as f64;
            let sc = 1.0 + (scale - 1.0) * t;
            outline_cmds(&mut cmds, &outline.scaled(sc).rotated(taper_turn(*twist, *scale, sc)), z0 + height * t);
            sketch += 1;
            ids.push(format!(r#"{{"sketch":{sketch}}}"#));
        }
        cmds.push(format!(r#"{{"command":"solid.loft","params":{{"sections":[{}],"operation":"new","body_name":"G"}}}}"#, ids.join(",")));
        let Some(Step::Revolve { profile, .. }) = b.steps.get(1).filter(|st| matches!(st, Step::Revolve { op: Op::Intersect, .. })) else {
            std::fs::write(
                format!("{dir}/{name}.json"),
                format!(
                    "{{\"commands\":[
{}
]}}
",
                    cmds.join(
                        ",
"
                    )
                ),
            )
            .unwrap();
            eprintln!("{name}: {n} sections, {} commands, no trim", cmds.len());
            continue;
        };
        // FLAT_BLANK=back: only the back cone trims (the front of the blank far past the toe).
        let mut profile = profile.clone();
        if std::env::var("FLAT_BLANK").as_deref() == Ok("cyl") {
            // Cylinders through the pitch circles at the heel and the toe instead of the cones.
            let sigma = s.shaft_angle.to_radians();
            let d1 = sigma.sin().atan2(f64::from(s.teeth2) / f64::from(s.teeth) + sigma.cos());
            let delta = if i == 0 { d1 } else { sigma - d1 };
            let r = s.module * f64::from(if i == 0 { s.teeth } else { s.teeth2 }) / 2.0;
            let re = r / delta.sin();
            let b = s.face_width.min(0.45 * re);
            let (rh, rt) = (r, (re - b) * delta.sin());
            let (lo, hi, face) = (profile[0].y, profile[3].y, profile[profile.len() - 1].y);
            profile = [(0.0, lo), (rh, lo), (rh, hi), (rt, hi), (rt, face), (0.0, face)].iter().map(|q| crate::geom::V2::new(q.0, q.1)).collect();
        }
        if std::env::var("FLAT_BLANK").as_deref() == Ok("ring") {
            for q in profile.iter_mut() {
                if q.x == 0.0 {
                    q.x = 1.0;
                }
            }
        }
        if std::env::var("FLAT_BLANK").as_deref() == Ok("back") {
            let top = profile[3].y;
            profile = vec![profile[0], profile[1], profile[2], profile[3], crate::geom::V2::new(0.0, top)];
        }
        cmds.push(format!(
            r#"{{"command":"sketch.create","params":{{"plane":{{"origin":{},"x_dir":{},"y_dir":{}}},"project_edges":false}}}}"#,
            v3(f.origin),
            v3(x),
            v3(z)
        ));
        cmds.push(format!(
            r#"{{"command":"sketch.line","params":{{"points":[{}],"closed":true}}}}"#,
            profile.iter().map(|q| p2(*q)).collect::<Vec<_>>().join(",")
        ));
        cmds.push(r#"{"command":"sketch.finish","params":{}}"#.to_string());
        cmds.push(r#"{"command":"solid.revolve","params":{"axis":"y","operation":"intersect","targets":["G"]}}"#.to_string());
        std::fs::write(format!("{dir}/{name}.json"), format!("{{\"commands\":[\n{}\n]}}\n", cmds.join(",\n"))).unwrap();
        eprintln!("{name}: {n} sections, {} commands", cmds.len());
    }
}
