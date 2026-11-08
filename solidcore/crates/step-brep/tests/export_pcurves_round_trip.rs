//! Primitives exported with their pcurves and imported again. The exporter writes the
//! pcurves in the file's STEP parameters (ISO 10303-42: angles in radians, a cylinder's
//! or cone's v in millimetres), converted from the kernel's NURB parameters, so the
//! importer's checks accept them.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test helpers

use solidcore::kernel::Kernel;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives as api;
use solidcore::surface::surface::p2::SurfaceKind;
use solidcore::topology::edgeuse::p1 as eu;
use solidcore::topology::trimming_tools::clear_uv_trim_curves;
use solidcore::topology::{Brep, FaceId, FaceuseId};
use step_brep::{
    brep_to_model, build_with_file_pcurves, validate_for_display, BuildMode, BuildOptions,
};
use step_import::model::Bound;

/// Stitched builds only (the file's pcurves are used on the stitched build; the
/// topology-first build would hide them), with or without the file's pcurves.
fn stitched(file_pcurves: bool) -> BuildOptions {
    BuildOptions {
        file_pcurves,
        mode: BuildMode::StitchOnly,
        ..BuildOptions::default()
    }
}

/// Faces that took their file pcurves, after checking none was found off its edge and
/// that every body the plain build makes display-ready still is.
fn round_trip(brep: &Brep) -> Result<usize, String> {
    let model = brep_to_model(brep).map_err(|e| e.to_string())?;
    let text = step_export::write(&model).map_err(|e| e.to_string())?;
    let model = step_import::import(text.as_bytes(), &step_import::Limits::default())
        .map_err(|e| e.to_string())?;
    assert!(
        model.file_pcurves.diagnostics.is_empty(),
        "{:?}",
        model.file_pcurves.diagnostics
    );
    assert!(!model.file_pcurves.edges.is_empty());
    let options = stitched(true);
    let mut faces = 0;
    for body in &model.bodies {
        // Only a body the plain build makes display-ready is compared.
        let plain = build_with_file_pcurves(body, &model.file_pcurves, stitched(false));
        if !plain.is_ok_and(|(built, _)| validate_for_display(&built).is_ok()) {
            continue;
        }
        let (built, stats) = build_with_file_pcurves(body, &model.file_pcurves, options)
            .map_err(|e| e.to_string())?;
        validate_for_display(&built).map_err(|e| e.to_string())?;
        for wrong in ["edge off its pcurve", "pcurve off the edge's 3D curve"] {
            assert!(!stats.fallbacks.contains_key(wrong), "{stats:?}");
        }
        faces += stats.faces;
    }
    Ok(faces)
}

#[test]
fn exported_cylinder_pcurves_check_out_on_import() {
    let mut kernel = Kernel::default();
    let cylinder =
        api::create_cylinder(&mut kernel, Point3::new(0.0, 0.0, -2.0), 2.0, 4.0).unwrap();
    // The side face takes its pcurves; the planar caps carry none.
    assert_eq!(round_trip(&cylinder), Ok(1));
}

#[test]
fn exported_cone_torus_and_sphere_pcurves_are_on_their_edges() {
    let mut kernel = Kernel::default();
    let cone = api::create_cone(&mut kernel, Point3::new(0.0, 0.0, 0.0), 3.0, 1.0, 4.0).unwrap();
    let torus = api::create_torus(&mut kernel, Point3::new(0.0, 0.0, 0.0), 5.0, 1.5).unwrap();
    let sphere = api::create_sphere(&mut kernel, Point3::new(0.0, 0.0, 0.0), 2.0).unwrap();
    for brep in [&cone, &torus, &sphere] {
        round_trip(brep).unwrap();
    }
}

// ---- a missing UV trim keeps the loop edge ----
//
// The wall of a cylinder is a non-planar face, so its loop edge uses carry pcurves.
// When one of those optional trims cannot be mapped the loop must keep the edge: the
// trim only enriches the face's boundary, it does not make the edge.

/// A fresh cylinder, independently built: the edge, face and edge-use ids of two
/// equal constructions agree.
fn cylinder_for_trim_test() -> Brep {
    let mut kernel = Kernel::default();
    api::create_cylinder(&mut kernel, Point3::new(0.0, 0.0, -2.0), 2.0, 4.0).unwrap()
}

/// The cylinder's curved wall: its face and material face use.
fn wall_face(brep: &Brep) -> (FaceId, FaceuseId) {
    for shell in brep.shells().ids() {
        for faceuse in brep.shell(shell).unwrap().get_faceuses().unwrap() {
            let Some(face) = brep.faceuse(faceuse).unwrap().face else {
                continue;
            };
            let planar = matches!(
                brep.face(face)
                    .unwrap()
                    .surface
                    .as_ref()
                    .map(solidcore::surface::surface::Surface::kind),
                Some(SurfaceKind::Plane(_))
            );
            if !planar {
                return (face, faceuse);
            }
        }
    }
    panic!("the cylinder has no wall face");
}

/// Every face's loops' bounds in file order, `Bound::Edges` and its oriented edges
/// included: the whole boundary, not just its size.
fn loop_bounds(model: &step_import::model::Model) -> Vec<(u64, Vec<Bound>)> {
    let body = model.bodies.first().expect("the model has a body");
    body.faces
        .iter()
        .map(|face| {
            (
                face.step_id,
                face.loops.iter().map(|loop_| loop_.bound.clone()).collect(),
            )
        })
        .collect()
}

/// A face's `Bound::Edges` lists, in file order.
fn face_edge_bounds(model: &step_import::model::Model, face_id: u64) -> Vec<Vec<usize>> {
    let body = model.bodies.first().unwrap();
    let face = body
        .faces
        .iter()
        .find(|face| face.step_id == face_id)
        .expect("the face is in the model");
    face.loops
        .iter()
        .map(|loop_| match &loop_.bound {
            Bound::Edges(edges) => edges.iter().map(|edge| edge.edge).collect(),
            Bound::Vertex(_) => panic!("the cylinder wall must have edge bounds"),
        })
        .collect()
}

#[test]
fn a_wall_without_uv_trims_keeps_every_loop_edge_and_its_order() {
    let cylinder = cylinder_for_trim_test();
    let before = brep_to_model(&cylinder).unwrap();
    let (wall, _) = wall_face(&cylinder);
    let wall_id = u64::from(wall.index()) + 1;
    assert!(
        before.pcurves.keys().any(|&(face, _, _)| face == wall_id),
        "the wall should carry pcurves: {:?}",
        before.pcurves
    );

    // The same cylinder with the wall's UV trims cleared: the exporter cannot write
    // its pcurves, but the loop still owns every edge.
    let mut stripped = cylinder_for_trim_test();
    let (stripped_wall, _) = wall_face(&stripped);
    assert_eq!(stripped_wall, wall);
    clear_uv_trim_curves(&mut stripped, stripped_wall).unwrap();
    let after = brep_to_model(&stripped).unwrap();

    assert!(
        after.pcurves.keys().all(|&(face, _, _)| face != wall_id),
        "the cleared wall should carry no pcurve: {:?}",
        after.pcurves
    );
    assert_eq!(
        loop_bounds(&before),
        loop_bounds(&after),
        "clearing the wall's optional UV trims dropped boundary edges"
    );
    let wall_edges = face_edge_bounds(&after, wall_id);
    assert!(
        wall_edges.iter().any(|edges| !edges.is_empty()),
        "the wall has no boundary edges at all"
    );

    // The file the stripped B-rep writes keeps that non-empty loop topology.
    let text = step_export::write(&after).unwrap();
    let imported = step_import::import(text.as_bytes(), &step_import::Limits::default()).unwrap();
    let mut walls = 0;
    for body in &imported.bodies {
        for face in &body.faces {
            let planar = matches!(face.surface, step_import::model::Surface::Plane { .. });
            if !planar {
                walls += 1;
                for loop_ in &face.loops {
                    match &loop_.bound {
                        Bound::Edges(edges) => assert!(
                            !edges.is_empty(),
                            "face {} wrote an empty edge loop",
                            face.step_id
                        ),
                        Bound::Vertex(_) => {}
                    }
                }
            }
        }
    }
    assert_eq!(walls, 1, "the round trip lost the cylinder wall");
}

#[test]
fn one_missing_middle_uv_trim_keeps_the_other_pcurve_keys() {
    let cylinder = cylinder_for_trim_test();
    let before = brep_to_model(&cylinder).unwrap();
    let (wall, wall_use) = wall_face(&cylinder);
    let wall_id = u64::from(wall.index()) + 1;
    let loops = eu::faceuse_loopuses(&cylinder, wall_use, false, None).unwrap();
    assert_eq!(loops.len(), 1, "the wall should have one loop");
    let uses = eu::loopuse_edgeuses(&cylinder, loops[0]).unwrap();
    assert!(
        uses.len() >= 3,
        "the wall loop has {} edge uses",
        uses.len()
    );

    // Select a non-seam middle use holding its own trim (its mate holds none), so
    // clearing its slot removes exactly one exported pcurve, not a seam mate's too.
    let sole_trim_holder = |index: usize| -> bool {
        let use_ = uses[index];
        let edge = eu::edge(&cylinder, use_).unwrap().unwrap();
        let occurrences = uses
            .iter()
            .filter(|&&other| eu::edge(&cylinder, other).unwrap() == Some(edge))
            .count();
        let own = eu::uv_trim_curve_pointer(&cylinder, use_)
            .unwrap()
            .is_some();
        let mate_clear = match eu::mate(&cylinder, use_).unwrap() {
            Some(mate) => eu::uv_trim_curve_pointer(&cylinder, mate)
                .unwrap()
                .is_none(),
            None => true,
        };
        occurrences == 1 && own && mate_clear
    };
    let cleared_index = (1..uses.len() - 1)
        .find(|&index| sole_trim_holder(index))
        .expect("the cylinder needs a non-seam, sole-holder middle edge use");
    let cleared_key = (wall_id, 0, cleared_index);
    assert!(
        before.pcurves.contains_key(&cleared_key),
        "the middle use should carry a pcurve: {:?}",
        before.pcurves
    );

    let mut stripped = cylinder_for_trim_test();
    let (stripped_wall, stripped_use) = wall_face(&stripped);
    assert_eq!(stripped_wall, wall);
    let stripped_loops = eu::faceuse_loopuses(&stripped, stripped_use, false, None).unwrap();
    let stripped_uses = eu::loopuse_edgeuses(&stripped, stripped_loops[0]).unwrap();
    assert_eq!(stripped_uses.len(), uses.len());
    eu::set_uv_trim_curve(
        &mut stripped,
        stripped_uses[cleared_index],
        eu::UvTrimCurveArg::Null,
        0.0,
        true,
        None,
    )
    .unwrap();
    let after = brep_to_model(&stripped).unwrap();

    assert_eq!(
        loop_bounds(&before),
        loop_bounds(&after),
        "a missing middle UV trim dropped boundary edges"
    );
    assert!(
        !after.pcurves.contains_key(&cleared_key),
        "the cleared use kept a pcurve"
    );
    // Every surviving pcurve keeps its key (face, loop and use index) and its samples:
    // no use after the missing one was renumbered onto another edge.
    for (key, curves) in &before.pcurves {
        if key == &cleared_key {
            continue;
        }
        assert_eq!(
            after.pcurves.get(key),
            Some(curves),
            "pcurve key {key:?} moved or changed"
        );
    }
    assert_eq!(
        after.pcurves.len() + 1,
        before.pcurves.len(),
        "unexpected pcurves after clearing one use: {:?}",
        after.pcurves
    );
    // The map comparison above checks this later key unconditionally; require it
    // in the fixture so the gap cannot be the last pcurve.
    assert!(
        before.pcurves.contains_key(&(wall_id, 0, uses.len() - 1)),
        "the cylinder needs a pcurve after the missing middle use"
    );
    let bounds_before = face_edge_bounds(&before, wall_id);
    let bounds_after = face_edge_bounds(&after, wall_id);
    assert_eq!(bounds_before, bounds_after);
}
