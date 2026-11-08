//! Seam pcurves come from the B-rep's own edge uses, one per use.
//!
//! `brep_to_model` samples each non-planar edge use's UV trim into one pcurve. A closed
//! edge is not a seam by itself: a cylinder's cap circle is closed but used once on the
//! wall, while the wall's seam edge is used twice. The exporter groups pcurves by `BRep`
//! edge and writes `SEAM_CURVE` only when one edge carries two pcurves on one surface,
//! so the count per (face, edge) must equal the number of actual uses: two for a seam,
//! one for every other trimmed use.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::BTreeMap;

use solidcore::kernel::Kernel;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives as api;
use solidcore::topology::Brep;
use step_brep::brep_to_model;
use step_import::model::{Bound, Model, PcurveKey};

/// One `BRep` edge as one face uses it: `(face step id, edge index in the body)`.
type FaceEdge = (u64, usize);

fn cylinder() -> Brep {
    let mut kernel = Kernel::default();
    api::create_cylinder(&mut kernel, Point3::new(0.0, 0.0, -2.0), 2.0, 4.0).unwrap()
}

fn torus() -> Brep {
    let mut kernel = Kernel::default();
    api::create_torus(&mut kernel, Point3::new(0.0, 0.0, 0.0), 5.0, 1.5).unwrap()
}

/// How many times each face's boundary uses each edge.
fn uses_per_face_edge(model: &Model) -> BTreeMap<FaceEdge, usize> {
    let mut counts = BTreeMap::new();
    for face in &body(model).faces {
        for loop_ in &face.loops {
            let Bound::Edges(edges) = &loop_.bound else {
                continue;
            };
            for oriented in edges {
                *counts.entry((face.step_id, oriented.edge)).or_default() += 1;
            }
        }
    }
    counts
}

/// How many sampled pcurve curves the model associates with each face edge: the sum
/// over its uses of the curves stored under that use's [`PcurveKey`].
fn curves_per_face_edge(model: &Model) -> BTreeMap<FaceEdge, usize> {
    let mut counts = BTreeMap::new();
    for face in &body(model).faces {
        for (loop_index, loop_) in face.loops.iter().enumerate() {
            let Bound::Edges(edges) = &loop_.bound else {
                continue;
            };
            for (use_index, oriented) in edges.iter().enumerate() {
                if let Some(curves) = model.pcurves.get(&(face.step_id, loop_index, use_index)) {
                    *counts.entry((face.step_id, oriented.edge)).or_default() += curves.len();
                }
            }
        }
    }
    counts
}

/// The pcurve keys of one face edge, in use order.
fn keys_of(model: &Model, face_edge: FaceEdge) -> Vec<PcurveKey> {
    let mut keys = Vec::new();
    for face in &body(model).faces {
        if face.step_id != face_edge.0 {
            continue;
        }
        for (loop_index, loop_) in face.loops.iter().enumerate() {
            let Bound::Edges(edges) = &loop_.bound else {
                continue;
            };
            for (use_index, oriented) in edges.iter().enumerate() {
                if oriented.edge == face_edge.1 {
                    keys.push((face.step_id, loop_index, use_index));
                }
            }
        }
    }
    keys
}

fn body(model: &Model) -> &step_import::model::Body {
    model.bodies.first().expect("the model has a body")
}

/// Each edge use carries exactly one sampled curve, so an edge used twice (a seam)
/// carries two and an edge used once carries one.
fn assert_one_curve_per_use(model: &Model, name: &str) {
    let uses = uses_per_face_edge(model);
    let curves = curves_per_face_edge(model);
    for (&(face, edge), &uses) in &uses {
        if !model
            .pcurves
            .keys()
            .any(|&(key_face, _, _)| key_face == face)
        {
            // A planar face writes no pcurves.
            continue;
        }
        assert_eq!(
            curves.get(&(face, edge)).copied().unwrap_or(0),
            uses,
            "{name}: face {face} edge {edge} has {uses} uses but {:?} curves: {:?}",
            curves.get(&(face, edge)),
            model.pcurves
        );
    }
    for (key, curves) in &model.pcurves {
        assert_eq!(
            curves.len(),
            1,
            "{name}: edge use {key:?} carries {} curves: {curves:?}",
            curves.len()
        );
    }
}

#[test]
fn cylinder_seam_has_two_curves_and_cap_trims_one() {
    let model = brep_to_model(&cylinder()).unwrap();
    assert_one_curve_per_use(&model, "cylinder");

    let uses = uses_per_face_edge(&model);
    let curves = curves_per_face_edge(&model);
    let seams: Vec<_> = uses
        .iter()
        .filter(|(&(_, _), &uses)| uses == 2)
        .map(|(&key, _)| key)
        .collect();
    assert_eq!(
        seams.len(),
        1,
        "the cylinder has one seam edge (its wall's): {uses:?}"
    );
    let seam = seams[0];
    assert_eq!(curves.get(&seam), Some(&2), "seam {seam:?}: {curves:?}");
    let singles: Vec<_> = uses
        .iter()
        .filter(|(&key, &uses)| uses == 1 && curves.contains_key(&key))
        .map(|(&key, _)| key)
        .collect();
    assert_eq!(
        singles.len(),
        2,
        "the wall trims two cap circles, one use each: {uses:?}"
    );
    for single in singles {
        assert_eq!(curves.get(&single), Some(&1), "cap trim {single:?}");
    }
}

#[test]
fn torus_seam_has_two_curves() {
    let model = brep_to_model(&torus()).unwrap();
    assert_one_curve_per_use(&model, "torus");

    let uses = uses_per_face_edge(&model);
    let curves = curves_per_face_edge(&model);
    let seams: Vec<_> = uses
        .iter()
        .filter(|(&(_, _), &uses)| uses == 2)
        .map(|(&key, _)| key)
        .collect();
    assert_eq!(
        seams.len(),
        2,
        "a torus is periodic in u and v, so both its seam edges are used twice: {uses:?}"
    );
    for seam in &seams {
        assert_eq!(
            curves.get(seam),
            Some(&2),
            "torus seam {seam:?} must carry two curves: {curves:?}"
        );
    }
    assert!(
        uses.values().all(|&uses| uses <= 2),
        "unexpected use count: {uses:?}"
    );
}

/// The written file marks exactly the seams, and the importer recovers one pcurve per
/// actual use from it.
#[test]
fn written_seam_curves_match_the_actual_seams() {
    for (name, brep) in [("cylinder", cylinder()), ("torus", torus())] {
        let model = brep_to_model(&brep).unwrap();
        let uses = uses_per_face_edge(&model);
        let curves = curves_per_face_edge(&model);
        let seams: Vec<_> = uses
            .iter()
            .filter(|(&(_, _), &uses)| uses == 2)
            .map(|(&key, _)| key)
            .collect();
        let expected_seams = if name == "cylinder" { 1 } else { 2 };
        assert_eq!(seams.len(), expected_seams, "{name}: seam edges: {uses:?}");

        let text = step_export::write(&model).unwrap();
        assert_eq!(
            text.matches("SEAM_CURVE(").count(),
            expected_seams,
            "{name}: the file must mark exactly its {expected_seams} seam edge(s)"
        );

        let imported =
            step_import::import(text.as_bytes(), &step_import::Limits::default()).unwrap();
        let imported_uses = uses_per_face_edge(&imported);
        let imported_body = body(&imported);
        let mut file_counts = Vec::new();
        for (&(face, edge), &uses) in &imported_uses {
            if !pcurve_bearing_face(&imported, face) {
                // A planar face writes no pcurves.
                continue;
            }
            let step_id = imported_body.edges[edge].step_id;
            let file_count = imported
                .file_pcurves
                .edges
                .get(&step_id)
                .map_or(0, Vec::len);
            assert_eq!(
                file_count, uses,
                "{name}: imported face {face} edge {edge} (#{step_id}) has {uses} uses \
                 but {file_count} pcurves"
            );
            file_counts.push(file_count);
        }
        assert_eq!(
            file_counts.iter().filter(|&&count| count == 2).count(),
            expected_seams,
            "{name}: each imported seam edge carries two pcurves: {file_counts:?}"
        );
        if name == "cylinder" {
            assert!(
                file_counts.contains(&1),
                "{name}: a non-seam trimmed use carries one pcurve: {file_counts:?}"
            );
        }
        assert!(
            file_counts.iter().all(|&count| count <= 2),
            "{name}: no edge may carry more than its two uses: {file_counts:?}"
        );

        // Each seam edge's two real trims are the two sides of the periodic cut: the
        // surface line one full period apart, not a synthetic copy added to one use.
        for &seam in &seams {
            assert_eq!(
                curves.get(&seam),
                Some(&2),
                "{name}: seam {seam:?} curves: {curves:?}"
            );
            let mut means = Vec::new();
            for key in keys_of(&model, seam) {
                let uv = &model.pcurves[&key][0];
                let mean_u = uv.iter().map(|p| p[0]).sum::<f64>() / uv.len() as f64;
                let mean_v = uv.iter().map(|p| p[1]).sum::<f64>() / uv.len() as f64;
                means.push([mean_u, mean_v]);
            }
            assert_eq!(means.len(), 2, "{name}: seam {seam:?} keys: {means:?}");
            let turn = std::f64::consts::TAU;
            let du = (means[1][0] - means[0][0]).abs();
            let dv = (means[1][1] - means[0][1]).abs();
            assert!(
                ((du - turn).abs() < 1e-9 && dv < 1e-9) || ((dv - turn).abs() < 1e-9 && du < 1e-9),
                "{name}: seam {seam:?} uses must be one full period apart: {means:?}"
            );
        }
    }
}

/// Whether `face` carries pcurves: only a non-planar face does.
fn pcurve_bearing_face(model: &Model, face_id: u64) -> bool {
    body(model)
        .faces
        .iter()
        .find(|face| face.step_id == face_id)
        .is_some_and(|face| !matches!(face.surface, step_import::model::Surface::Plane { .. }))
}
