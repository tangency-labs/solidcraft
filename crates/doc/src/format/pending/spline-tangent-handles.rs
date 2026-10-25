//! Fit splines save tangent-handle points and construction lines. Older designs receive natural
//! tangent handles when opened, preserving their original curve. The last release refuses the
//! newer format number rather than silently dropping editable tangent geometry.
use serde_json::Value;

pub fn step(value: &mut Value) -> crate::Result<()> {
    let Some(features) = value.get_mut("features").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    for feature in features {
        if feature.get("type").and_then(Value::as_str) != Some("sketch") {
            continue;
        }
        let Some(sketch_json) = feature.get_mut("sketch") else { continue };
        let mut sketch: solidcraft_sketch::Sketch = serde_json::from_value(sketch_json.clone())
            .map_err(|error| crate::DocError::Invalid(format!("design file: {error}")))?;
        sketch
            .add_missing_spline_handles()
            .map_err(|error| crate::DocError::Invalid(format!("cannot add tangent handles to an older spline: {error}")))?;
        *sketch_json = serde_json::to_value(sketch).map_err(|error| crate::DocError::Invalid(format!("design file: {error}")))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use solidcraft_geom::Vec2;
    use solidcraft_sketch::{CurveKind, Sketch};

    #[test]
    fn older_fit_splines_open_with_natural_handles_and_keep_their_shape() {
        let mut sketch = Sketch::new();
        let points = [
            Vec2::new(0.0, 0.0),
            Vec2::new(12.0, 5.0),
            Vec2::new(24.0, 1.0),
            Vec2::new(37.0, 9.0),
        ]
        .into_iter()
        .map(|point| sketch.add_point(point, None).unwrap())
        .collect::<Vec<_>>();
        let curve = sketch.add_curve(CurveKind::Spline { pts: points, control: false, degree: 3, periodic: false, handles: Vec::new() }, None).unwrap();
        let before = sketch.exact_segs(curve).unwrap();
        let output = sketch.add_freeform_offset(curve, true, 2.0).unwrap();
        let output_id = sketch.curves[output].id.clone();
        let output_before = sketch.exact_segs(output).unwrap();
        let mut doc = crate::Document::new("Spline handles");
        doc.add_feature(
            crate::FeatureKind::Sketch { plane: crate::PlaneRef::Origin { name: "XY".into() }, sketch },
            None,
        )
        .unwrap();
        let mut old: Value = serde_json::from_str(&doc.to_json()).unwrap();
        old["format"] = json!(crate::format::format_string(crate::format::FORMAT - 1));
        old.as_object_mut().unwrap().remove("format_pending");

        let mut opened = crate::format::read(&old.to_string()).unwrap();
        let crate::FeatureKind::Sketch { sketch, .. } = &mut opened.features[0].kind else { panic!("expected sketch feature") };
        let spline = sketch.curves.iter().position(|curve| matches!(curve.kind, CurveKind::Spline { .. })).unwrap();
        let CurveKind::Spline { handles, .. } = &sketch.curves[spline].kind else { panic!("expected spline") };

        assert_eq!(handles.len(), 4);
        let output = sketch.curve_index(&output_id).unwrap();
        let CurveKind::Spline { handles: output_handles, .. } = &sketch.curves[output].kind else { panic!("expected offset spline") };
        assert!(output_handles.is_empty());
        let after = sketch.exact_segs(spline).unwrap();
        assert_eq!(after.len(), before.len());
        for (actual, expected) in after.iter().zip(&before) {
            for step in 0..=16 {
                let t = step as f64 / 16.0;
                assert!(actual.point_at(t).dist(expected.point_at(t)) < 1e-12);
            }
        }
        assert_eq!(sketch.curves.iter().filter(|curve| curve.spline_handle_of.is_some()).count(), 4);
        let output_after = sketch.exact_segs(output).unwrap();
        assert_eq!(output_after.len(), output_before.len());
        for (actual, expected) in output_after.iter().zip(&output_before) {
            for step in 0..=16 {
                let t = step as f64 / 16.0;
                assert!(actual.point_at(t).dist(expected.point_at(t)) < 1e-12);
            }
        }
        sketch.validate_spline_handles().unwrap();
        sketch.refresh_offsets().unwrap();
    }
}
