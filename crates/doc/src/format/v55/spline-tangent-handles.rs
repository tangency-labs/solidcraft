//! A fit point spline whose tangent handles were dragged saves the handle points and their
//! construction lines. A spline stores them only once one is dragged: older designs open
//! unchanged, their splines without handles. The last release warns about the newer format
//! number and opens the spline from its fit points.

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use solidcore_record::Vec2;
    use solidcraft_sketch::{CurveKind, Sketch};

    #[test]
    fn older_fit_splines_open_without_handles() {
        let mut sketch = Sketch::new();
        let points = [Vec2::new(0.0, 0.0), Vec2::new(12.0, 5.0), Vec2::new(24.0, 1.0), Vec2::new(37.0, 9.0)]
            .into_iter()
            .map(|point| sketch.add_point(point, None).unwrap())
            .collect::<Vec<_>>();
        sketch
            .add_curve(CurveKind::Spline { pts: points, control: false, degree: 3, periodic: false, nurbs: None, handles: Vec::new() }, None)
            .unwrap();
        let mut doc = crate::Document::new("Spline handles");
        doc.add_feature(crate::FeatureKind::Sketch { plane: crate::PlaneRef::Origin { name: "XY".into() }, sketch: sketch.clone() }, None)
            .unwrap();
        let mut old: Value = serde_json::from_str(&doc.to_json()).unwrap();
        old["format"] = json!(crate::format::format_string(crate::format::FORMAT - 1));
        old.as_object_mut().unwrap().remove("format_pending");

        let opened = crate::format::read(&old.to_string()).unwrap();
        let crate::FeatureKind::Sketch { sketch: opened, .. } = &opened.features[0].kind else { panic!("expected sketch feature") };
        assert_eq!(opened.curves, sketch.curves, "the splines open as they were saved");
        assert!(opened.curves.iter().all(|c| c.spline_handle_of.is_none()));
    }
}
