//! Section Stack: the planes' spacing and the slabs' heights, and the command on mesh bodies
//! made from kernel solids, measured against values derived by hand.

use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_doc::{Document, FeatureKind, MeshData};
use solidcraft_sketch::CurveKind;

use super::{stack_extrudes, stack_offsets};
use crate::{Sel, Session};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn refusal(s: &mut Session, p: Value) -> String {
    match s.execute("mesh.section_stack", &p) {
        Ok(v) => panic!("{p} should be refused, got {v}"),
        Err(e) => e.to_string(),
    }
}

fn close(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
}

/// A design whose only feature imports `solids`, meshed at a 0.01 mm chord, as the one mesh body
/// `Mesh`.
fn mesh_session(solids: &[solidcraft_kernel::Body]) -> Session {
    let mut positions = Vec::new();
    let mut triangles = Vec::new();
    for solid in solids {
        let mesh = solid.mesh(&solidcraft_kernel::MeshTolerance::chord(0.01)).unwrap();
        let first = u32::try_from(positions.len() / 3).unwrap();
        positions.extend(mesh.positions.iter().flat_map(|p| [p.x, p.y, p.z]));
        triangles.extend(mesh.triangles.iter().flatten().map(|i| i + first));
    }
    let mut doc = Document::default();
    doc.add_feature(
        FeatureKind::MeshImport {
            file: "public-safe-shapes.stl".into(),
            meshes: vec![MeshData {
                name: "Mesh".into(),
                positions: positions.into(),
                triangles: triangles.into(),
                color: None,
                corner_normals: Default::default(),
            }],
        },
        Some("Imported mesh"),
    )
    .unwrap();
    Session::new(doc)
}

fn boxed(a: [f64; 3], b: [f64; 3]) -> solidcraft_kernel::Body {
    solidcraft_kernel::box_solid(Vec3::new(a[0], a[1], a[2]), Vec3::new(b[0], b[1], b[2])).unwrap()
}

/// The 10 x 20 x 30 mm box from the origin.
fn box_mesh() -> Session {
    mesh_session(&[boxed([0., 0., 0.], [10., 20., 30.])])
}

fn ids(v: &Value) -> Vec<u64> {
    v.as_array().unwrap().iter().map(|x| x.as_u64().unwrap()).collect()
}

/// A sketch's plane origin, its curves and its profile areas.
fn sketch_of(s: &Session, id: u64) -> (Vec3, Vec<CurveKind>, Vec<f64>) {
    let st = s.model.state();
    let sk = st.sketch(id).unwrap_or_else(|| panic!("no sketch {id}"));
    (sk.plane.origin, sk.sketch.curves.iter().map(|c| c.kind.clone()).collect(), sk.profiles.iter().map(|p| p.area).collect())
}

fn lines(curves: &[CurveKind]) -> usize {
    curves.iter().filter(|c| matches!(c, CurveKind::Line { .. })).count()
}

fn circles(curves: &[CurveKind]) -> Vec<f64> {
    curves.iter().filter_map(|c| if let CurveKind::Circle { r, .. } = c { Some(*r) } else { None }).collect()
}

fn body_volume(s: &mut Session, body: &str) -> f64 {
    run(s, "inspect.measure", json!({ "bodies": [body] }))["total"]["volume_mm3"].as_f64().unwrap()
}

fn slab_volume(s: &mut Session, r: &Value) -> f64 {
    let bodies: Vec<String> = r["bodies"].as_array().unwrap().iter().map(|b| b.as_str().unwrap().to_string()).collect();
    assert!(!bodies.is_empty(), "{r}");
    bodies.iter().map(|b| body_volume(s, b)).sum()
}

#[test]
fn stack_offsets_space_the_planes_from_the_lower_end_and_always_cut_the_upper_one() {
    let wide = [-100.0, 100.0];
    let cases: &[(f64, f64, f64, &[f64])] = &[
        (0.0, 10.0, 2.0, &[0.0, 2.0, 4.0, 6.0, 8.0, 10.0]),
        // The last multiple (9) falls short of 10: 10 is cut as well.
        (0.0, 10.0, 3.0, &[0.0, 3.0, 6.0, 9.0, 10.0]),
        // Either way round.
        (10.0, 0.0, 3.0, &[0.0, 3.0, 6.0, 9.0, 10.0]),
        // 6 is 0.005 short of the end, under 0.01: it moves onto the end.
        (0.0, 6.005, 2.0, &[0.0, 2.0, 4.0, 6.005]),
        // 0.02 short is past the hair: both planes.
        (0.0, 6.02, 2.0, &[0.0, 2.0, 4.0, 6.0, 6.02]),
        // A tenth of the step is the hair when the step is small: 0.00005 short of 0.00405
        // moves onto it, 0.0005 short of 0.0045 does not.
        (0.0, 0.00405, 0.001, &[0.0, 0.001, 0.002, 0.003, 0.00405]),
        (0.0, 0.0045, 0.001, &[0.0, 0.001, 0.002, 0.003, 0.004, 0.0045]),
        (3.0, 3.0, 1.0, &[3.0]),
        // Ends closer than the hair are the one plane at the lower end.
        (3.0, 3.004, 1.0, &[3.0]),
        (3.0, 3.00005, 0.001, &[3.0]),
        (3.0, 3.0004, 0.001, &[3.0, 3.0004]),
        // Not a running sum: 0.1 · k, never 0.1 + 0.1 + …
        (0.0, 0.3, 0.1, &[0.0, 0.1, 0.2, 0.30000000000000004]),
    ];
    for (from, to, step, want) in cases {
        let got = stack_offsets(*from, *to, *step, wide, 1000).unwrap();
        assert!(close(&got, want), "{from} {to} {step}: {got:?}");
    }
    // The ends stay 0.001 mm inside the body's reach.
    assert!(close(&stack_offsets(-50.0, 50.0, 5.0, [0.0, 12.0], 1000).unwrap(), &[0.001, 5.001, 10.001, 11.999]));
    assert!(close(&stack_offsets(-50.0, 0.0, 1.0, [0.0, 12.0], 1000).unwrap(), &[0.001]));
    // Ends wholly outside the reach cut none of the body: refused, naming it.
    assert!(stack_offsets(20.0, 30.0, 1.0, [0.0, 12.0], 1000).unwrap_err().contains("reaches from 0 mm to 12 mm"));
}

#[test]
fn stack_offsets_refuse_what_cannot_be_cut_and_name_the_spacing_that_fits() {
    let e = stack_offsets(0.0, 100.0, 0.1, [-1.0, 101.0], 200).unwrap_err();
    // 199 gaps fit 200 planes: 100 / 199 = 0.50251…, rounded up to 0.503.
    assert!(e.contains("max_sections") && e.contains("0.503 mm"), "{e}");
    let fits = stack_offsets(0.0, 100.0, 0.503, [-1.0, 101.0], 200).unwrap();
    assert!(fits.len() <= 200, "{}", fits.len());
    // A spacing so small the count is past any number is refused, not allocated.
    assert!(stack_offsets(0.0, 1e300, 1e-300, [-1e301, 1e301], 1000).unwrap_err().contains("max_sections"));
    assert!(stack_offsets(0.0, 1.0, 0.0, [-1.0, 2.0], 10).unwrap_err().contains("more than 0"));
    assert!(stack_offsets(0.0, 1.0, -1.0, [-1.0, 2.0], 10).unwrap_err().contains("more than 0"));
    assert!(stack_offsets(f64::NAN, 1.0, 1.0, [-1.0, 2.0], 10).unwrap_err().contains("finite"));
    assert!(stack_offsets(0.0, 1.0, f64::INFINITY, [-1.0, 2.0], 10).unwrap_err().contains("finite"));
    // A body flatter than the two end margins.
    assert!(stack_offsets(0.0, 1.0, 0.1, [0.0, 0.001], 10).unwrap_err().contains("thick"));
    // One section allowed: only one plane does.
    assert!(stack_offsets(0.0, 1.0, 0.5, [-1.0, 2.0], 1).unwrap_err().contains("same value"));
    assert!(close(&stack_offsets(0.5, 0.5, 0.5, [-1.0, 2.0], 1).unwrap(), &[0.5]));
}

#[test]
fn stack_extrudes_run_each_slab_to_the_next_plane_and_never_past_the_body() {
    let cases: &[(&[f64], f64, f64, &[Option<f64>])] = &[
        (&[0.0, 2.0, 4.0], 2.0, 5.0, &[Some(2.0), Some(2.0), Some(1.0)]),
        (&[0.0, 2.0, 4.0], 2.0, 10.0, &[Some(2.0), Some(2.0), Some(2.0)]),
        // The end plane came 0.2 after 4: that slab runs on to 5 and the end is only drawn.
        (&[0.0, 2.0, 4.0, 4.2], 2.0, 5.0, &[Some(2.0), Some(2.0), Some(1.0), None]),
        // An end plane on the far face makes no slab.
        (&[0.0, 2.0, 4.0, 5.0], 2.0, 5.0, &[Some(2.0), Some(2.0), Some(1.0), None]),
        (&[0.0, 2.0, 4.0, 4.995], 2.0, 5.0, &[Some(2.0), Some(2.0), Some(1.0), None]),
        (&[3.0], 2.0, 5.0, &[None]),
        (&[], 2.0, 5.0, &[]),
        (&[0.0, 1.0], 1.0, 1.005, &[Some(1.0), None]),
    ];
    for (ats, step, far, want) in cases {
        let got = stack_extrudes(ats, *step, *far);
        let same = got.len() == want.len()
            && got.iter().zip(want.iter()).all(|(a, b)| match (a, b) {
                (Some(a), Some(b)) => (a - b).abs() < 1e-9,
                (None, None) => true,
                _ => false,
            });
        assert!(same, "{ats:?} {step} {far}: {got:?}");
    }
}

/// Over a fixed grid of ends, spacings and reaches, the planes always: ascend, stay inside the
/// reach, start and end on the (kept-inside) ends, leave no gap wider than the spacing (and a
/// hair, where the last multiple moved onto the end), are never
/// closer than the hair, and number at most the cap. The slabs are positive, at least 0.01 mm, and
/// reach no further than the body.
#[test]
fn the_stack_keeps_its_rules_over_a_grid_of_inputs() {
    let ends = [-40.0, -7.3, 0.0, 0.004, 2.5, 9.999, 13.0, 55.0];
    let steps = [0.003, 0.25, 0.7, 1.0, 3.3, 20.0];
    let reaches = [[0.0, 10.0], [-5.0, 5.0], [-30.0, 2.0]];
    let mut checked = 0;
    for from in ends {
        for to in ends {
            for step in steps {
                for reach in reaches {
                    let Ok(ats) = stack_offsets(from, to, step, reach, 1000) else { continue };
                    let (lo, hi) = (reach[0] + 1e-3, reach[1] - 1e-3);
                    let keep = |v: f64| v.max(lo).min(hi);
                    let (a, b) = if from <= to { (keep(from), keep(to)) } else { (keep(to), keep(from)) };
                    assert!(!ats.is_empty() && ats.len() <= 1000);
                    // The upper end is cut, unless the ends are closer than the hair: one plane.
                    let end = if b - a < 0.01f64.min(step / 10.0) { a } else { b };
                    assert!((ats[0] - a).abs() < 1e-9 && (ats[ats.len() - 1] - end).abs() < 1e-9, "{from} {to} {step} {reach:?}: {ats:?}");
                    for w in ats.windows(2) {
                        let gap = w[1] - w[0];
                        // At most a spacing, or a spacing and a hair where the last multiple
                        // moved onto the end.
                        assert!(gap > 0.0 && gap <= step + 0.01f64.min(step / 10.0) + 1e-9, "{from} {to} {step} {reach:?}: {ats:?}");
                        assert!(gap >= 0.01f64.min(step / 10.0) - 1e-9, "{from} {to} {step} {reach:?}: {ats:?}");
                    }
                    assert!(ats.iter().all(|v| *v >= lo - 1e-9 && *v <= hi + 1e-9));
                    let slabs = stack_extrudes(&ats, step, hi);
                    assert_eq!(slabs.len(), ats.len());
                    for (k, (at, slab)) in ats.iter().zip(&slabs).enumerate() {
                        if let Some(h) = slab {
                            assert!(*h > 0.0 && at + h <= hi + 1e-9, "{ats:?} {slabs:?}");
                            // Each slab reaches the next plane, or past it into the end slab's
                            // place; only the end slab may be cut short by the body.
                            if let Some(next) = ats.get(k + 1) {
                                assert!(at + h >= next - 1e-9, "{ats:?} {slabs:?}");
                            } else {
                                assert!(*h >= 0.01 - 1e-12, "{ats:?} {slabs:?}");
                            }
                        }
                    }
                    // Every plane but the end one makes a slab.
                    assert!(slabs.iter().rev().skip(1).all(Option::is_some) || ats.len() < 2, "{ats:?} {slabs:?}");
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 500, "{checked}");
}

#[test]
fn a_box_mesh_gets_one_rectangle_sketch_on_each_plane() {
    let mut s = box_mesh();
    let r = run(&mut s, "mesh.section_stack", json!({"body": "Mesh", "plane": "XY", "from": 5, "to": 25, "spacing": 5}));
    let sketches = ids(&r["sketches"]);
    assert_eq!((sketches.len(), r["planes"].as_u64(), r["empty"].as_u64()), (5, Some(5), Some(0)), "{r}");
    for (k, id) in sketches.iter().enumerate() {
        let (origin, curves, areas) = sketch_of(&s, *id);
        assert!((origin.z - 5.0 * (k as f64 + 1.0)).abs() < 1e-9, "{origin:?}");
        assert_eq!((curves.len(), lines(&curves)), (4, 4), "{curves:?}");
        assert_eq!(areas.len(), 1);
        assert!((areas[0] - 200.0).abs() < 1e-6, "{areas:?}");
    }
    assert!(r["slabs"].as_array().unwrap().is_empty() && r["bodies"].as_array().unwrap().is_empty());
    // One folder holds the stack; the mesh body is as it was.
    let group = r["group"].as_u64().unwrap();
    let folder = s.doc.timeline_groups.iter().find(|g| g.id == group).unwrap();
    assert_eq!(folder.features, sketches);
    assert_eq!(folder.name, "Section stack (Mesh)");
    assert!(s.model.state().body("Mesh").unwrap().body.is_mesh());
    assert!(s.model.results.iter().all(|x| x.error.is_none()), "{:?}", s.model.results);
}

#[test]
fn without_from_and_to_the_stack_spans_the_body_just_inside_its_ends() {
    let mut s = box_mesh();
    let r = run(&mut s, "mesh.section_stack", json!({"plane": "XY", "spacing": 5, "body": "Mesh"}));
    let zs: Vec<f64> = ids(&r["sketches"]).iter().map(|id| sketch_of(&s, *id).0.z).collect();
    assert!(close(&zs, &[0.001, 5.001, 10.001, 15.001, 20.001, 25.001, 29.999]), "{zs:?}");
}

/// XZ's normal is +Y (sketch x along +X, y along -Z), YZ's is +X: the sections are 10 x 30 and
/// 20 x 30.
#[test]
fn the_stack_runs_along_the_planes_normal() {
    for (plane, from, to, step, area, axis) in [("XZ", 5.0, 15.0, 5.0, 300.0, 1usize), ("YZ", 2.0, 8.0, 3.0, 600.0, 0usize)] {
        let mut s = box_mesh();
        let r = run(&mut s, "mesh.section_stack", json!({"body": "Mesh", "plane": plane, "from": from, "to": to, "spacing": step}));
        let sketches = ids(&r["sketches"]);
        assert_eq!(sketches.len(), 3, "{plane}: {r}");
        for (k, id) in sketches.iter().enumerate() {
            let (origin, curves, areas) = sketch_of(&s, *id);
            let at = [origin.x, origin.y, origin.z][axis];
            assert!((at - (from + step * k as f64)).abs() < 1e-9, "{plane}: {origin:?}");
            assert_eq!(lines(&curves), 4, "{plane}: {curves:?}");
            assert!((areas[0] - area).abs() < 1e-6, "{plane}: {areas:?}");
        }
    }
}

#[test]
fn a_cylinder_mesh_gets_one_circle_on_each_plane() {
    let cylinder = solidcraft_kernel::cylinder(Vec3::ZERO, Vec3::new(0., 0., 1.), 7.0, 30.0).unwrap();
    let mut s = mesh_session(&[cylinder]);
    let r = run(&mut s, "mesh.section_stack", json!({"body": "Mesh", "plane": "XY", "from": 3, "to": 27, "spacing": 8}));
    let sketches = ids(&r["sketches"]);
    assert_eq!(sketches.len(), 4, "{r}");
    // The fitter's tolerance is 0.002 of the mesh's diagonal (14 x 14 x 30 mm box).
    let tol = (14f64.powi(2) * 2.0 + 30f64.powi(2)).sqrt() * 2e-3;
    for id in sketches {
        let (_, curves, _) = sketch_of(&s, id);
        let radii = circles(&curves);
        assert_eq!((curves.len(), radii.len()), (1, 1), "{curves:?}");
        assert!((radii[0] - 7.0).abs() <= tol, "{radii:?}");
    }
}

/// A 60 x 40 x 6 mm plate with a through hole of radius 5: each section is the outline and the
/// hole, and the slabs keep the hole open. The planes at 1, 3 and 5 make slabs 2, 2 and 0.999 mm
/// (the last stops 0.001 mm under the top), 4.999 mm of plate in all.
#[test]
fn slabs_of_a_plate_keep_its_hole_open() {
    let plate = boxed([0., 0., 0.], [60., 40., 6.]);
    let bore = solidcraft_kernel::cylinder(Vec3::new(30., 20., -1.), Vec3::new(0., 0., 1.), 5.0, 8.0).unwrap();
    let holed = solidcraft_kernel::boolean(&plate, &bore, solidcraft_kernel::BoolOp::Cut).unwrap().unwrap();
    let mut s = mesh_session(&[holed]);
    let r = run(&mut s, "mesh.section_stack", json!({"body": "Mesh", "plane": "XY", "from": 1, "to": 5, "spacing": 2, "extrude": true}));
    let sketches = ids(&r["sketches"]);
    assert_eq!(sketches.len(), 3, "{r}");
    for id in &sketches {
        let (_, curves, _) = sketch_of(&s, *id);
        assert_eq!((lines(&curves), circles(&curves).len()), (4, 1), "{curves:?}");
    }
    assert_eq!(ids(&r["slabs"]).len(), 3, "{r}");
    let tol = (60f64.powi(2) + 40f64.powi(2) + 36.0).sqrt() * 2e-3;
    let height = 4.999;
    let want = (2400.0 - 25.0 * std::f64::consts::PI) * height;
    // A circle within `tol` of radius 5 has an area within 2π·5·tol (and tol²π) of 25π.
    let slack = (2.0 * std::f64::consts::PI * 5.0 * tol + std::f64::consts::PI * tol * tol) * height + 1e-6;
    let got = slab_volume(&mut s, &r);
    assert!((got - want).abs() <= slack, "{got} vs {want} ± {slack}");
    assert!(s.model.state().body("Mesh").unwrap().body.is_mesh());
}

#[test]
fn slabs_of_a_box_add_up_to_its_height_between_the_planes() {
    // 5, 7, 9, 11: four slabs of 2.
    let mut s = box_mesh();
    let r = run(&mut s, "mesh.section_stack", json!({"body": "Mesh", "plane": "XY", "from": 5, "to": 11, "spacing": 2, "extrude": true}));
    assert_eq!(ids(&r["slabs"]).len(), 4, "{r}");
    assert_eq!(r["bodies"], json!(["Mesh slab 1", "Mesh slab 2", "Mesh slab 3", "Mesh slab 4"]), "{r}");
    assert!((slab_volume(&mut s, &r) - 200.0 * 8.0).abs() < 1e-6, "{r}");
    // Upward from each plane along the normal: slab k fills z = 5 + 2k … 7 + 2k over the whole
    // section.
    for k in 0..4 {
        let b = s.model.state().body(&format!("Mesh slab {}", k + 1)).unwrap().body.exact_bounds().unwrap();
        let (lo, hi) = ([b.min.x, b.min.y, b.min.z], [b.max.x, b.max.y, b.max.z]);
        let z = 5.0 + 2.0 * k as f64;
        assert!(close(&lo, &[0.0, 0.0, z]) && close(&hi, &[10.0, 20.0, z + 2.0]), "{lo:?} {hi:?}");
    }
    assert!(s.model.state().body("Mesh").unwrap().body.is_mesh());

    // 24, 26, 28 and the end at 29.5: 2, 2, 1.5, then 0.499 up to 0.001 mm under the top.
    let mut s = box_mesh();
    let r = run(&mut s, "mesh.section_stack", json!({"body": "Mesh", "plane": "XY", "from": 24, "to": 29.5, "spacing": 2, "extrude": true}));
    assert_eq!(ids(&r["slabs"]).len(), 4, "{r}");
    assert!((slab_volume(&mut s, &r) - 200.0 * 5.999).abs() < 1e-6, "{r}");
    let first = s.model.state().body("Mesh slab 1").unwrap().body.exact_bounds().unwrap();
    let last = s.model.state().body("Mesh slab 4").unwrap().body.exact_bounds().unwrap();
    assert!((first.min.z - 24.0).abs() < 1e-9 && (last.max.z - 29.999).abs() < 1e-9, "{first:?} {last:?}");
}

#[test]
fn two_boxes_in_one_mesh_each_get_their_outline_and_slabs() {
    let mut s = mesh_session(&[boxed([0., 0., 0.], [10., 10., 10.]), boxed([20., 0., 0.], [30., 10., 10.])]);
    let r = run(&mut s, "mesh.section_stack", json!({"body": "Mesh", "plane": "XY", "from": 2, "to": 8, "spacing": 3, "extrude": true}));
    let sketches = ids(&r["sketches"]);
    assert_eq!(sketches.len(), 3, "{r}");
    for id in &sketches {
        let (_, curves, areas) = sketch_of(&s, *id);
        assert_eq!((lines(&curves), areas.len()), (8, 2), "{curves:?}");
    }
    // 2, 5, 8: slabs 3, 3, and 1.999 up to 0.001 mm under the tops.
    assert!((slab_volume(&mut s, &r) - 2.0 * 100.0 * 7.999).abs() < 1e-6, "{r}");
}

/// A cylinder of radius 7 about the vertical line through (7, 7), cut along (1, 1, 0)/√2: its box
/// reaches 0…19.799 mm along that normal but the cylinder only 9.8995 ± 7 = 2.8995…16.8995 mm. Of
/// the planes at 0.001, 1.001, …, 19.001 and 19.798, the three below and the four above miss it.
#[test]
fn planes_that_miss_the_mesh_make_no_sketch() {
    let cylinder = solidcraft_kernel::cylinder(Vec3::new(7., 7., 0.), Vec3::new(0., 0., 1.), 7.0, 10.0).unwrap();
    let mut s = mesh_session(&[cylinder]);
    let plane = json!({"origin": [0, 0, 0], "x_dir": [0, 0, 1], "y_dir": [1, -1, 0]});
    let r = run(&mut s, "mesh.section_stack", json!({"body": "Mesh", "plane": plane, "spacing": 1}));
    assert_eq!((r["planes"].as_u64(), r["empty"].as_u64(), ids(&r["sketches"]).len()), (Some(21), Some(7), 14), "{r}");
    let sketches = s.doc.features.iter().filter(|f| matches!(f.kind, FeatureKind::Sketch { .. })).count();
    assert_eq!(sketches, 14);
    // Planes at 0.001 and 1.001 only: both miss, so nothing is made.
    let before = s.doc.to_json();
    let e = refusal(&mut s, json!({"body": "Mesh", "plane": plane, "from": 0, "to": 1, "spacing": 1}));
    assert!(e.contains("none of the 2 planes"), "{e}");
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn the_stack_is_one_undo_step() {
    // The features (by id, kind and name) and the timeline folders.
    let timeline = |s: &Session| {
        let features: Vec<(u64, String, String)> =
            s.doc.features.iter().map(|f| (f.id, f.name.clone(), format!("{:?}", std::mem::discriminant(&f.kind)))).collect();
        let folders: Vec<(u64, Vec<u64>)> = s.doc.timeline_groups.iter().map(|g| (g.id, g.features.clone())).collect();
        (features, folders)
    };
    let mut s = box_mesh();
    let before = timeline(&s);
    run(&mut s, "mesh.section_stack", json!({"body": "Mesh", "plane": "XY", "from": 5, "to": 25, "spacing": 10, "extrude": true}));
    let after = timeline(&s);
    // Three sketches, two slabs (the end plane at 25 runs 2 mm on, so it is extruded too: three).
    assert_eq!(after.0.len(), before.0.len() + 6, "{after:?}");
    assert_eq!(after.1.len(), 1);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(timeline(&s), before);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(timeline(&s), after);
}

#[test]
fn bad_calls_are_refused_and_change_nothing() {
    let mut s = box_mesh();
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [50, 0, 0], "body_name": "Solid"}));
    let before = s.doc.to_json();
    let xy = |extra: Value| {
        let mut p = json!({"body": "Mesh", "plane": "XY", "spacing": 5});
        for (k, v) in extra.as_object().unwrap() {
            p[k] = v.clone();
        }
        p
    };
    for (p, says) in [
        (xy(json!({"body": "Solid"})), "select a mesh body"),
        (xy(json!({"body": "Nope"})), "no body `Nope`"),
        (json!({"plane": "XY", "spacing": 5}), "select a mesh body"),
        (json!({"body": "Mesh", "spacing": 5}), "`plane` is required"),
        (json!({"body": "Mesh", "plane": "XY"}), "`spacing` is required"),
        (xy(json!({"spacing": 0})), "more than 0"),
        (xy(json!({"spacing": -2})), "more than 0"),
        (xy(json!({"spacing": "30 deg"})), "spacing"),
        (xy(json!({"spacing": "nonsense"})), "spacing"),
        (xy(json!({"max_sections": 0})), "max_sections"),
        (xy(json!({"max_sections": 1001})), "max_sections"),
        (xy(json!({"max_sections": 2.5})), "max_sections"),
        // The default cap of 100: 29.998 mm between the kept-inside ends over 99 gaps is 0.30301…
        (xy(json!({"spacing": 0.01})), "0.304 mm fits"),
        (xy(json!({"spacing": 0.01, "max_sections": 200})), "0.151 mm fits"),
        (xy(json!({"spacing": 1e-300})), "max_sections"),
        (xy(json!({"from": "nonsense"})), "from"),
        (xy(json!({"extrude": "yes"})), "`extrude`"),
        (xy(json!({"name": ""})), "`name`"),
        (xy(json!({"name": "x".repeat(101)})), "`name`"),
        (xy(json!({"plane": "Nowhere"})), "Nowhere"),
    ] {
        let e = refusal(&mut s, p.clone());
        assert!(e.contains(says), "{p}: {e}");
        assert_eq!(s.doc.to_json(), before, "{p}");
    }
    // Two bodies selected and no `body`.
    s.selection = vec![Sel::Body { name: "Mesh".into() }, Sel::Body { name: "Solid".into() }];
    assert!(refusal(&mut s, json!({"plane": "XY", "spacing": 5})).contains("select one mesh body"));
    // Not while a sketch is open.
    s.selection.clear();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    assert!(s.execute("mesh.section_stack", &json!({"body": "Mesh", "plane": "XY", "spacing": 5})).is_err());
}

#[test]
fn the_selected_mesh_body_is_cut_when_body_is_left_out() {
    let mut s = box_mesh();
    s.selection = vec![Sel::Body { name: "Mesh".into() }];
    let r = run(&mut s, "mesh.section_stack", json!({"plane": "XY", "from": 10, "to": 20, "spacing": 10}));
    assert_eq!(ids(&r["sketches"]).len(), 2, "{r}");
}

/// Lengths are expressions in the design's units: half an inch apart is 12.7 mm, and a parameter
/// may set the spacing.
#[test]
fn spacing_and_ends_are_length_expressions() {
    let mut s = box_mesh();
    let r = run(&mut s, "mesh.section_stack", json!({"body": "Mesh", "plane": "XY", "spacing": "0.5 in"}));
    let zs: Vec<f64> = ids(&r["sketches"]).iter().map(|id| sketch_of(&s, *id).0.z).collect();
    assert!(close(&zs, &[0.001, 12.701, 25.401, 29.999]), "{zs:?}");

    let mut s = box_mesh();
    run(&mut s, "parameters.add", json!({"name": "pitch", "expression": "6", "unit": "mm"}));
    let r = run(&mut s, "mesh.section_stack", json!({"body": "Mesh", "plane": "XY", "from": "pitch", "to": "pitch * 3", "spacing": "pitch / 2"}));
    let zs: Vec<f64> = ids(&r["sketches"]).iter().map(|id| sketch_of(&s, *id).0.z).collect();
    assert!(close(&zs, &[6.0, 9.0, 12.0, 15.0, 18.0]), "{zs:?}");
}

/// The stack is ordinary sketches and extrudes: a design saved and opened cold has the same.
#[test]
fn a_saved_stack_reopens_the_same() {
    let mut s = box_mesh();
    let r = run(&mut s, "mesh.section_stack", json!({"body": "Mesh", "plane": "XY", "from": 5, "to": 11, "spacing": 2, "extrude": true}));
    let volume = slab_volume(&mut s, &r);
    let mut cold = Session::new(Document::from_json(&s.doc.to_json()).unwrap());
    assert!(cold.model.results.iter().all(|x| x.error.is_none()), "{:?}", cold.model.results);
    for id in ids(&r["sketches"]) {
        assert_eq!(lines(&sketch_of(&cold, id).1), 4);
    }
    assert!((slab_volume(&mut cold, &r) - volume).abs() < 1e-6);
    assert!(cold.doc.features.iter().skip(1).all(|f| matches!(f.kind, FeatureKind::Sketch { .. } | FeatureKind::Extrude { .. })));
}

#[test]
fn a_second_stack_adds_its_own_sketches_and_folder() {
    let mut s = box_mesh();
    let a = run(&mut s, "mesh.section_stack", json!({"body": "Mesh", "plane": "XY", "from": 5, "to": 15, "spacing": 5}));
    let b = run(&mut s, "mesh.section_stack", json!({"body": "Mesh", "plane": "XZ", "from": 5, "to": 15, "spacing": 5, "name": "Side"}));
    assert_eq!((ids(&a["sketches"]).len(), ids(&b["sketches"]).len()), (3, 3));
    assert_ne!(a["group"], b["group"]);
    assert_eq!(s.doc.timeline_groups.len(), 2);
    let names: Vec<String> = s.doc.features.iter().map(|f| f.name.clone()).collect();
    assert!(names.contains(&"Side 1 of 3 (5 mm)".to_string()), "{names:?}");
    // The same stack again takes the next free name.
    let c = run(&mut s, "mesh.section_stack", json!({"body": "Mesh", "plane": "XY", "from": 5, "to": 15, "spacing": 5}));
    assert_eq!(ids(&c["sketches"]).len(), 3, "{c}");
    let names: Vec<String> = s.doc.features.iter().map(|f| f.name.clone()).collect();
    assert!(names.contains(&"Section 1 of 3 (5 mm)".to_string()) && names.contains(&"Section 2 1 of 3 (5 mm)".to_string()), "{names:?}");
    assert!(s.model.results.iter().all(|x| x.error.is_none()), "{:?}", s.model.results);
}

/// Insert Mesh puts a scan in a component of its own. With the design's top level active, the
/// stack is sketched there and measured where the mesh is placed: the 10 x 20 x 30 mm box, then
/// moved by (100, 0, 50), so its planes run from z = 0.001 and then from z = 50.001.
#[test]
fn an_inserted_mesh_is_cut_where_it_is_placed() {
    let dir = crate::tests::Folder::new("section-stack-insert");
    let stl = dir.path("box.stl");
    let mut src = Session::default();
    run(&mut src, "solid.box", json!({"length": 10, "width": 20, "height": 30}));
    run(&mut src, "file.export_stl", json!({ "path": stl }));
    let mut s = Session::default();
    let inserted = run(&mut s, "file.insert_mesh", json!({ "path": stl }));
    let body = s.model.state().bodies[0].name.clone();
    let check = |s: &mut Session, bottom: f64| {
        let r = run(s, "mesh.section_stack", json!({"body": body, "plane": "XY", "spacing": 10}));
        let sketches = ids(&r["sketches"]);
        let zs: Vec<f64> = sketches.iter().map(|id| sketch_of(s, *id).0.z).collect();
        let want: Vec<f64> = [0.001, 10.001, 20.001, 29.999].iter().map(|z| z + bottom).collect();
        assert!(close(&zs, &want), "{zs:?}");
        for id in sketches {
            let (_, curves, areas) = sketch_of(s, id);
            assert_eq!(lines(&curves), 4, "{curves:?}");
            assert!((areas[0] - 200.0).abs() < 1e-6, "{areas:?}");
        }
    };
    check(&mut s, 0.0);
    let occurrence = inserted["occurrence"].clone();
    run(&mut s, "occurrence.ground", json!({"occurrence": occurrence, "grounded": false}));
    run(&mut s, "occurrence.move", json!({"occurrence": occurrence, "translate": [100, 0, 50]}));
    check(&mut s, 50.0);
}

/// Every section of a sphere is a different circle. Each slab is a body of its own, and their volumes add
/// up to the stepped sphere: a radius-10 sphere cut every 4 mm from -8 to 8, each slab the disc
/// of its plane (radius √(100 − z²)) run 4 mm up, the last 1.999 mm to just under the top.
#[test]
fn slabs_of_a_sphere_whose_sections_all_differ_are_built() {
    let sphere = solidcraft_kernel::sphere(Vec3::ZERO, 10.0).unwrap();
    let mut s = mesh_session(&[sphere]);
    let r = run(&mut s, "mesh.section_stack", json!({"body": "Mesh", "plane": "XY", "from": -8, "to": 8, "spacing": 4, "extrude": true}));
    let sketches = ids(&r["sketches"]);
    assert_eq!((sketches.len(), ids(&r["slabs"]).len()), (5, 5), "{r}");
    let zs = [-8.0, -4.0, 0.0, 4.0, 8.0];
    let heights = [4.0, 4.0, 4.0, 4.0, 1.999];
    let want: f64 = zs.iter().zip(heights).map(|(z, h)| std::f64::consts::PI * (100.0 - z * z) * h).sum();
    // Each fitted circle is within the fitter's tolerance (0.002 of the mesh's 34.6 mm diagonal)
    // of its section: its area within 2πr·tol + πtol².
    let tol = (3.0f64 * 400.0).sqrt() * 2e-3;
    let slack: f64 =
        zs.iter().zip(heights).map(|(z, h)| (2.0 * std::f64::consts::PI * (100.0 - z * z).sqrt() * tol + std::f64::consts::PI * tol * tol) * h).sum();
    let got = slab_volume(&mut s, &r);
    assert!((got - want).abs() <= slack, "{got} vs {want} ± {slack}");
    assert!(s.model.results.iter().all(|x| x.error.is_none()), "{:?}", s.model.results);
}

/// A 60 x 40 x 12 mm mount with rounded corners (radius 10), a boss of radius 12 up to z = 30,
/// a bore of radius 7 through it and two bolt holes of radius 3, inserted as a mesh and stacked
/// every 3 mm with slabs. Joining its slabs broke the boolean (neighbouring slabs meet on outlines
/// fitted section by section, which differ by a hair): each slab is a body of its own.
#[test]
fn slabs_of_a_mount_with_a_boss_are_built() {
    let dir = crate::tests::Folder::new("section-stack-mount");
    let stl = dir.path("mount.stl");
    let mut src = Session::default();
    for (id, p) in [
        ("solid.box", json!({"length": 60, "width": 40, "height": 12, "body_name": "Mount"})),
        ("solid.fillet", json!({"edges": [[0, 0, 6], [60, 0, 6], [0, 40, 6], [60, 40, 6]], "radius": 10})),
        ("solid.cylinder", json!({"base": [30, 20, 12], "radius": 12, "height": 18, "operation": "join"})),
        ("solid.cylinder", json!({"base": [30, 20, -1], "radius": 7, "height": 40, "operation": "cut"})),
        ("solid.cylinder", json!({"base": [10, 20, -1], "radius": 3, "height": 20, "operation": "cut"})),
        ("solid.cylinder", json!({"base": [50, 20, -1], "radius": 3, "height": 20, "operation": "cut"})),
    ] {
        run(&mut src, id, p);
    }
    run(&mut src, "file.export_stl", json!({ "path": stl }));
    let mut s = Session::default();
    run(&mut s, "file.insert_mesh", json!({ "path": stl }));
    let body = s.model.state().bodies[0].name.clone();
    let r = run(&mut s, "mesh.section_stack", json!({"body": body, "plane": "XY", "spacing": 3, "extrude": true}));
    assert_eq!(ids(&r["sketches"]).len(), 11, "{r}");
    assert!(s.model.results.iter().all(|x| x.error.is_none()), "{:?}", s.model.results);
    // The slabs step the mount: 0.001 to 29.999 high, so within a few percent of its volume.
    // Each slab is its section's material (the outlines whose holes stay open) run up to the next
    // plane: its volume is that area times its height, whatever the fit made of the section.
    let heights = [3.0, 3.0, 3.0, 3.0, 3.0, 3.0, 3.0, 3.0, 3.0, 2.998];
    let names: Vec<String> = r["bodies"].as_array().unwrap().iter().map(|b| b.as_str().unwrap().to_string()).collect();
    assert_eq!(names.len(), 10, "{r}");
    for ((id, h), name) in ids(&r["sketches"]).into_iter().zip(heights).zip(&names) {
        let profiles = crate::cmd::sketch_store::read(&s.doc, id).map(solidcraft_sketch::find_profiles).unwrap();
        let area: f64 =
            super::material_outlines(&profiles).iter().filter_map(|outer| profiles.iter().find(|p| &p.outer_curves == outer)).map(|p| p.area).sum();
        let got = body_volume(&mut s, name);
        assert!((got - area * h).abs() < 1e-6 * area * h + 1e-6, "{name}: {got} vs {area} × {h}");
    }
}

/// With the mesh's own component active, moved by (100, 0, 50), the stack is sketched in that
/// component, its plane given as Sketch takes it: XY is the component's own XY (as a new sketch
/// there), an explicit plane through the world point (100, 0, 50) the component's own origin, a
/// face picked where it is shown. The mesh's sections are at z = 0.001…29.999 of its frame.
#[test]
fn a_moved_components_mesh_is_cut_in_that_components_frame() {
    let dir = crate::tests::Folder::new("section-stack-moved");
    let stl = dir.path("box.stl");
    let mut src = Session::default();
    run(&mut src, "solid.box", json!({"length": 10, "width": 20, "height": 30}));
    run(&mut src, "file.export_stl", json!({ "path": stl }));
    let mut s = Session::default();
    let inserted = run(&mut s, "file.insert_mesh", json!({ "path": stl }));
    let occurrence = inserted["occurrence"].clone();
    run(&mut s, "occurrence.ground", json!({"occurrence": occurrence, "grounded": false}));
    run(&mut s, "occurrence.move", json!({"occurrence": occurrence, "translate": [100, 0, 50]}));
    let component = s.doc.components.iter().find(|c| c.id != 0).map(|c| c.id).unwrap();
    run(&mut s, "component.activate", json!({"component": component}));
    let body = s.model.state().bodies[0].name.clone();
    // A solid box beside the mesh in that component, its corner given where it is shown: its
    // bottom face, picked where it is shown, is a first plane too. Its outward normal points
    // down, so the stack runs from the mesh's top (offset -29.999) every 10 mm to its bottom.
    run(&mut s, "solid.box", json!({"corner": [120, 0, 50], "length": 10, "width": 20, "height": 30, "operation": "new"}));
    let up = [0.001, 10.001, 20.001, 29.999];
    let down = [29.999, 19.999, 9.999, 0.001];
    for (plane, want) in
        [(json!("XY"), up), (json!({"origin": [100, 0, 50], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}), up), (json!({"face": [125, 10, 50]}), down)]
    {
        let r = run(&mut s, "mesh.section_stack", json!({"body": body, "plane": plane, "spacing": 10}));
        let sketches = ids(&r["sketches"]);
        let zs: Vec<f64> = sketches.iter().map(|id| sketch_of(&s, *id).0.z).collect();
        assert!(close(&zs, &want), "{plane}: {zs:?} {r}");
        for id in sketches {
            let (_, curves, areas) = sketch_of(&s, id);
            assert!(lines(&curves) == 4 && (areas[0] - 200.0).abs() < 1e-6, "{plane}: {curves:?} {areas:?}");
        }
    }
}

/// A slab never runs through planes that missed the mesh: two boxes 20 mm apart along z
/// (0…10 and 30…40), cut every 5 mm with slabs, give slabs only where the mesh is. The planes at
/// 10.001…25.001 make no sketch, and the slab of the plane at 5.001 runs 5 mm (to 10.001, past
/// the lower box's top by 0.001), not on to the plane at 30.001.
#[test]
fn slabs_stop_where_the_next_plane_misses_the_mesh() {
    let mut s = mesh_session(&[boxed([0., 0., 0.], [10., 20., 10.]), boxed([0., 0., 30.], [10., 20., 40.])]);
    let r = run(&mut s, "mesh.section_stack", json!({"body": "Mesh", "plane": "XY", "spacing": 5, "extrude": true}));
    assert_eq!((r["planes"].as_u64(), r["empty"].as_u64()), (Some(9), Some(4)), "{r}");
    // Slabs of 5 mm from 0.001, 5.001, 30.001 and 35.001, the last to the top at 39.999 (4.998).
    let want = 200.0 * (5.0 + 5.0 + 5.0 + 4.998);
    let got = slab_volume(&mut s, &r);
    assert!((got - want).abs() < 1e-6 * want, "{got} vs {want}: {r}");
}

/// A plane in the middle of the stack can lie on a flat face inside the mesh: a stepped block
/// (20 x 20 up to z = 5, then 10 x 20 up to z = 10) cut every 2.5 mm from 2.5 has a plane on
/// its shoulder at z = 5. Each section is the block's outline at that height.
#[test]
fn a_plane_on_a_shoulder_inside_the_mesh_is_cut() {
    let mut s = mesh_session(&[boxed([0., 0., 0.], [20., 20., 5.]), boxed([0., 0., 5.], [10., 20., 10.])]);
    let r = run(&mut s, "mesh.section_stack", json!({"body": "Mesh", "plane": "XY", "from": 2.5, "to": 7.5, "spacing": 2.5}));
    let sketches = ids(&r["sketches"]);
    assert_eq!(sketches.len(), 3, "{r}");
    let areas: Vec<f64> = sketches.iter().map(|id| sketch_of(&s, *id).2.iter().sum()).collect();
    assert!((areas[0] - 400.0).abs() < 1e-6 && (areas[2] - 200.0).abs() < 1e-6, "{areas:?}");
    assert!(areas[1] > 199.0, "{areas:?}");
}

/// `from` and `to` wholly outside the body cut nothing of it: refused, naming its reach.
#[test]
fn a_range_outside_the_body_is_refused() {
    let mut s = box_mesh();
    let why = refusal(&mut s, json!({"body": "Mesh", "plane": "XY", "from": -100, "to": -50, "spacing": 5}));
    assert!(why.contains("0 mm to 30 mm"), "{why}");
    let why = refusal(&mut s, json!({"body": "Mesh", "plane": "XY", "from": 40, "to": 50, "spacing": 5}));
    assert!(why.contains("0 mm to 30 mm"), "{why}");
}

/// A thin last slab runs on through the end plane only where the mesh goes on there. Two boxes
/// (z 0…6.5 and 15…20) cut from 0.001 to 7 every 3 mm: the planes are 0.001, 3.001, 6.001 and 7,
/// and 7 misses the mesh (in the gap). The slab of 6.001 stops at 7 (0.999 mm), not 3.999 mm on
/// to 10 inside the gap.
#[test]
fn a_thin_last_slab_stops_at_a_plane_that_misses_the_mesh() {
    let mut s = mesh_session(&[boxed([0., 0., 0.], [10., 20., 6.5]), boxed([0., 0., 15.], [10., 20., 20.])]);
    let r = run(&mut s, "mesh.section_stack", json!({"body": "Mesh", "plane": "XY", "from": 0.001, "to": 7, "spacing": 3, "extrude": true}));
    assert_eq!((r["planes"].as_u64(), r["empty"].as_u64()), (Some(4), Some(1)), "{r}");
    let want = 200.0 * (3.0 + 3.0 + 0.999);
    let got = slab_volume(&mut s, &r);
    assert!((got - want).abs() < 1e-6 * want, "{got} vs {want}: {r}");
}

/// A second extruded stack on the same mesh names its slabs on from the first's (`Mesh slab 6`
/// after `Mesh slab 1`…`5`), and each name it reports is a body of the design.
#[test]
fn a_second_stack_of_slabs_numbers_its_slabs_on() {
    let mut s = box_mesh();
    let call = json!({"body": "Mesh", "plane": "XY", "from": 5, "to": 13, "spacing": 2, "extrude": true});
    let first = run(&mut s, "mesh.section_stack", call.clone());
    let second = run(&mut s, "mesh.section_stack", call);
    let names = |r: &Value| r["bodies"].as_array().unwrap().iter().map(|b| b.as_str().unwrap().to_string()).collect::<Vec<_>>();
    // Planes at 5, 7, 9, 11 and 13 mm, each slab 2 mm (the box goes on past 13): five slabs each.
    assert_eq!(names(&first), ["Mesh slab 1", "Mesh slab 2", "Mesh slab 3", "Mesh slab 4", "Mesh slab 5"], "{first}");
    assert_eq!(names(&second), ["Mesh slab 6", "Mesh slab 7", "Mesh slab 8", "Mesh slab 9", "Mesh slab 10"], "{second}");
    let st = s.model.state();
    assert!(names(&second).iter().all(|n| st.body(n).is_some()), "{second}");
}
