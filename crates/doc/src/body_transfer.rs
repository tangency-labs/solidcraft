//! Recorded ownership changes are evaluated at their timeline position.
use crate::{DocError, Document, Mat, ModelBody, ModelState, Result};
use std::collections::BTreeSet;

/// A profile and references from its component are read through the recorded transfer frame
/// when that component receives the profile's body. Reading live placements would move the
/// source geometry when the component moves, then move the transferred body a second time.
/// All outputs of a constructor must reach the profile's component through the same frame.
/// Independent cross-component references retain their ordinary live placement.
pub(crate) fn profile_frame(doc: &Document, feature: &crate::Feature, sketch: u64) -> Option<Mat> {
    if feature.body_names.is_empty() {
        return None;
    }
    let profile = match &feature.kind {
        crate::FeatureKind::Extrude { sketch, operation: crate::Operation::NewBody, .. }
        | crate::FeatureKind::Revolve { sketch, operation: crate::Operation::NewBody, .. }
        | crate::FeatureKind::Coil { sketch: Some(sketch), operation: crate::Operation::NewBody, .. } => sketch,
        _ => return None,
    };
    let owner = doc.feature(profile.0)?.component;
    if owner == feature.component || (profile.0 != sketch && doc.feature(sketch)?.component != owner) {
        return None;
    }
    let mut frames: std::collections::BTreeMap<&str, (u64, Mat)> =
        feature.body_names.iter().map(|name| (name.as_str(), (feature.component, crate::IDENTITY))).collect();
    let start = doc.feature_index(feature.id)?.saturating_add(1);
    for step in doc.features.iter().take(doc.marker.unwrap_or(usize::MAX)).skip(start).filter(|step| !step.suppressed) {
        let crate::FeatureKind::BodyTransfer { bodies, from, to, transform } = &step.kind else { continue };
        for name in bodies {
            if let Some((component, frame)) = frames.get_mut(name.as_ref()) {
                if *component != *from || !valid_transfer_matrix(transform) {
                    return None;
                }
                *component = *to;
                *frame = crate::mat_mul(transform, frame);
            }
        }
        if frames.values().all(|(component, _)| *component == owner) {
            let (_, first) = frames.values().next()?;
            return frames.values().all(|(_, frame)| frame == first).then(|| crate::mat_inverse(first)).flatten();
        }
    }
    None
}

pub fn valid_transfer_matrix(m: &Mat) -> bool {
    crate::is_placement(m)
}

/// Validate the stored boundary before allocation, loading, or evaluation.
pub fn validate_body_transfer(doc: &Document, bodies: &[String], from: u64, to: u64, transform: &Mat) -> Result<()> {
    if bodies.is_empty()
        || bodies.len() > 1000
        || bodies.iter().any(|name| name.trim().is_empty() || name.len() > 512 || name.chars().any(char::is_control))
        || bodies.iter().collect::<BTreeSet<_>>().len() != bodies.len()
        || !valid_transfer_matrix(transform)
        || [from, to].iter().any(|id| *id != 0 && !doc.components.iter().any(|c| c.id == *id))
    {
        return Err(DocError::Invalid("invalid body transfer boundary".into()));
    }
    Ok(())
}

/// Moves the bodies in `st`, which the evaluator discards when this fails (so it is changed in
/// place rather than copied: a copy of every body for each moved one was most of the cost).
pub(crate) fn evaluate(doc: &Document, st: &mut ModelState, bodies: &[String], from: u64, to: u64, transform: Mat) -> Result<()> {
    validate_body_transfer(doc, bodies, from, to, &transform)?;
    let mut moved = Vec::with_capacity(bodies.len());
    for name in bodies {
        let (slot, old) =
            st.bodies.iter().enumerate().find(|(_, b)| b.name == *name).ok_or_else(|| DocError::Unknown(format!("transferred body `{name}`")))?;
        if doc.model_body_component(old) != from {
            return Err(DocError::Invalid(format!("body `{name}` no longer belongs to transfer source component {from}")));
        }
        let shape = if crate::is_identity(&transform) { old.body.clone() } else { solidcraft_kernel::transform_matrix(&old.body, transform)? };
        let mut body = ModelBody::new(old.name.clone(), shape, old.feature);
        body.component = to;
        body.transferred = true;
        body.source_name = old.source_name.clone();
        body.source_transform = crate::mat_mul(&transform, &old.source_transform);
        body.imported = old.imported.clone();
        body.names = crate::naming::NameCell::same(old);
        body.drawn_as_moved(old, &transform);
        moved.push((slot, body));
    }
    for (slot, body) in moved {
        if let Some(b) = st.bodies.get_mut(slot) {
            *b = body;
        }
    }
    crate::body_transfer_sheet::remap_sheet_metadata(st, bodies, &transform)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_transferred_coil_keeps_its_recorded_profile_frame() {
        use crate::{AxisRef, BodyName, FeatureKind, Model, PlaneRef, rigid, world_state};
        use solidcore_record::{Vec2, Vec3};

        for use_sketch_axis in [false, true] {
            let mut doc = Document::new("transferred coil");
            let destination = doc.add_component(Some("Profile part"), 0).unwrap();
            let placement = |y, z| rigid(Vec3::new(0., y, z), Vec3::ZERO, Vec3::Z, 0.);
            doc.occurrences.iter_mut().find(|o| o.component == destination).unwrap().transform = placement(0., 7.);
            let mut profile = crate::sketch::Sketch::new();
            profile.add_circle(Vec2::new(20., 0.), 1., None, None).unwrap();
            let sketch = doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "YZ".into() }, sketch: profile }, None).unwrap();
            doc.feature_mut(sketch).unwrap().component = destination;
            let axis_ref = use_sketch_axis.then(|| AxisRef::SketchAxis { axis: "y".into() });
            let coil = serde_json::from_value(serde_json::json!({
                "type": "coil", "base": ["0", "0", "0"], "sketch": sketch, "axis_ref": axis_ref,
                "pitch": "8", "turns": "2", "operation": "new_body"
            }))
            .unwrap();
            let coil = doc.add_feature(coil, Some("Coil")).unwrap();
            doc.feature_mut(coil).unwrap().body_names = vec!["Coil".into()];
            let transfer = doc
                .add_feature(
                    FeatureKind::BodyTransfer { bodies: vec![BodyName("Coil".into())], from: 0, to: destination, transform: placement(0., -7.) },
                    None,
                )
                .unwrap();

            let mut model = Model::new();
            let check = |model: &mut Model, doc: &Document, owner, min_z: f64| {
                model.evaluate(doc);
                let errors: Vec<_> = model.results.iter().filter_map(|r| r.error.as_ref()).collect();
                assert!(errors.is_empty(), "{errors:?}");
                let world = world_state(doc, &model.state()).unwrap();
                let body = world.body("Coil").unwrap();
                assert_eq!(body.component, owner);
                let bounds = body.body.exact_bounds().unwrap();
                // The coil starts one section radius below its profile centre and rises by 16 mm.
                assert!((bounds.min.z - min_z).abs() < 1e-6 && (bounds.max.z - (min_z + 18.)).abs() < 1e-6, "{bounds:?}");
                bounds
            };
            let before = check(&mut model, &doc, destination, 6.);
            let dy = if use_sketch_axis { 40. } else { 0. };
            doc.occurrences.iter_mut().find(|o| o.component == destination).unwrap().transform = placement(dy, 47.);
            let after = check(&mut model, &doc, destination, 46.);
            assert!((after.min.y - before.min.y - dy).abs() < 1e-6 && (after.max.y - before.max.y - dy).abs() < 1e-6, "{before:?} -> {after:?}");

            // Without the transfer, the coil is back in the root and uses the live sketch frame.
            doc.feature_mut(transfer).unwrap().suppressed = true;
            let live = check(&mut model, &doc, 0, 46.);
            assert!((live.min.y - after.min.y).abs() < 1e-6 && (live.max.y - after.max.y).abs() < 1e-6, "{live:?}");
        }
    }

    #[test]
    fn a_transferred_coil_keeps_a_separate_axis_in_the_profile_component_frame() {
        use crate::{BodyName, FeatureKind, Model, PlaneRef, rigid, world_state};
        use solidcore_record::{Vec2, Vec3};

        for independent_axis in [false, true] {
            let mut doc = Document::new("transferred coil with a separate axis sketch");
            let profile_owner = doc.add_component(Some("Profile part"), 0).unwrap();
            let axis_owner = if independent_axis { doc.add_component(Some("Axis part"), 0).unwrap() } else { profile_owner };
            let plane = || PlaneRef::Origin { name: "YZ".into() };
            let mut profile = crate::sketch::Sketch::new();
            profile.add_circle(Vec2::new(20., 0.), 1., None, None).unwrap();
            let sketch = doc.add_feature(FeatureKind::Sketch { plane: plane(), sketch: profile }, None).unwrap();
            doc.feature_mut(sketch).unwrap().component = profile_owner;
            let mut line = crate::sketch::Sketch::new();
            line.add_line(Vec2::ZERO, Vec2::new(0., 10.), None, None, Some("l1")).unwrap();
            let axis_sketch = doc.add_feature(FeatureKind::Sketch { plane: plane(), sketch: line }, None).unwrap();
            doc.feature_mut(axis_sketch).unwrap().component = axis_owner;
            let coil = serde_json::from_value(serde_json::json!({
                "type": "coil", "base": ["0", "0", "0"], "sketch": sketch,
                "axis_ref": {"type": "sketch_line", "sketch": axis_sketch, "curve": "l1"},
                "pitch": "8", "turns": "2", "operation": "new_body"
            }))
            .unwrap();
            let coil = doc.add_feature(coil, Some("Coil")).unwrap();
            doc.feature_mut(coil).unwrap().body_names = vec!["Coil".into()];
            let transfer = |from, to| FeatureKind::BodyTransfer { bodies: vec![BodyName("Coil".into())], from, to, transform: crate::IDENTITY };
            doc.add_feature(transfer(0, profile_owner), None).unwrap();
            if independent_axis {
                doc.add_feature(transfer(profile_owner, axis_owner), None).unwrap();
            }

            let mut model = Model::new();
            model.evaluate(&doc);
            doc.occurrences.iter_mut().find(|o| o.component == axis_owner).unwrap().transform =
                rigid(Vec3::new(0., 10., 0.), Vec3::ZERO, Vec3::Z, 0.);
            model.evaluate(&doc);
            let errors: Vec<_> = model.results.iter().filter_map(|r| r.error.as_ref()).collect();
            assert!(errors.is_empty(), "{errors:?}");
            let world = world_state(&doc, &model.state()).unwrap();
            let body = world.body("Coil").unwrap();
            assert_eq!(body.component, axis_owner);
            // The section adds 1 mm to the axis radius. An axis in the profile's component
            // keeps its recorded 20 mm radius. An independent axis moves 10 mm toward the profile.
            let half_width = if independent_axis { 11. } else { 21. };
            let bounds = body.body.exact_bounds().unwrap();
            assert!((bounds.max.x - half_width).abs() < 1e-6, "independent axis: {independent_axis}, {bounds:?}");
        }
    }

    #[test]
    fn a_transferred_coil_keeps_a_construction_axis_in_the_profile_component_frame() {
        use crate::{BodyName, FeatureKind, Model, PlaneRef, rigid, world_state};
        use solidcore_record::{Vec2, Vec3};

        for independent_axis in [false, true] {
            let mut doc = Document::new("transferred coil on a construction axis");
            let destination = doc.add_component(Some("Profile part"), 0).unwrap();
            let axis_owner = if independent_axis { doc.add_component(Some("Axis part"), 0).unwrap() } else { destination };
            let placement = |y| rigid(Vec3::new(0., y, 0.), Vec3::ZERO, Vec3::Z, 0.);
            if independent_axis {
                doc.occurrences.iter_mut().find(|o| o.component == axis_owner).unwrap().transform = placement(7.);
            }
            let mut profile = crate::sketch::Sketch::new();
            profile.add_circle(Vec2::new(20., 0.), 1., None, None).unwrap();
            let sketch = doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "YZ".into() }, sketch: profile }, None).unwrap();
            doc.feature_mut(sketch).unwrap().component = destination;
            let axis = serde_json::from_value(serde_json::json!({
                "type": "construction_axis", "def": {"type": "two_points",
                    "a": {"type": "point", "p": [0, 0, 0]}, "b": {"type": "point", "p": [0, 0, 1]}}
            }))
            .unwrap();
            let axis = doc.add_feature(axis, Some("Winding axis")).unwrap();
            doc.feature_mut(axis).unwrap().component = axis_owner;
            let coil = serde_json::from_value(serde_json::json!({
                "type": "coil", "base": ["0", "0", "0"], "sketch": sketch,
                "axis_ref": {"type": "construction", "name": "Winding axis"},
                "pitch": "8", "turns": "2", "operation": "new_body"
            }))
            .unwrap();
            let coil = doc.add_feature(coil, Some("Coil")).unwrap();
            doc.feature_mut(coil).unwrap().body_names = vec!["Coil".into()];
            doc.add_feature(
                FeatureKind::BodyTransfer { bodies: vec![BodyName("Coil".into())], from: 0, to: destination, transform: placement(-7.) },
                None,
            )
            .unwrap();

            let mut model = Model::new();
            let half_width = |model: &mut Model, doc: &Document| {
                model.evaluate(doc);
                let errors: Vec<_> = model.results.iter().filter_map(|r| r.error.as_ref()).collect();
                assert!(errors.is_empty(), "{errors:?}");
                let world = world_state(doc, &model.state()).unwrap();
                let body = world.body("Coil").unwrap();
                assert_eq!(body.component, destination);
                body.body.exact_bounds().unwrap().max.x
            };
            assert!((half_width(&mut model, &doc) - 21.).abs() < 1e-6);
            doc.occurrences.iter_mut().find(|o| o.component == destination).unwrap().transform = placement(10.);
            if independent_axis {
                doc.occurrences.iter_mut().find(|o| o.component == axis_owner).unwrap().transform = placement(17.);
            }
            // The destination's axis retains the profile's recorded 20 mm radius.
            // A separate axis moves 10 mm toward the profile, reducing that radius to 10 mm.
            let expected = if independent_axis { 11. } else { 21. };
            let after = half_width(&mut model, &doc);
            assert!((after - expected).abs() < 1e-6, "independent axis: {independent_axis}, half width: {after}");
        }
    }

    #[test]
    fn invalid_transfer_constructor_preserves_document_and_allocator() {
        let mut doc = Document::default();
        let before = doc.to_json();
        let mut scaled = crate::IDENTITY;
        scaled[0][0] = 2.0;
        for (bodies, from, transform) in [
            (vec!["Block".into()], 42, crate::IDENTITY),
            (vec!["Block".into(), "Block".into()], 0, crate::IDENTITY),
            (vec![String::new()], 0, crate::IDENTITY),
            (vec!["   ".into()], 0, crate::IDENTITY),
            (vec!["bad\nname".into()], 0, crate::IDENTITY),
            (vec!["Block".into()], 0, scaled),
        ] {
            assert!(
                doc.add_feature(
                    crate::FeatureKind::BodyTransfer { bodies: bodies.into_iter().map(crate::BodyName).collect(), from, to: 0, transform },
                    None
                )
                .is_err()
            );
            assert_eq!(doc.to_json(), before);
        }
    }
}
