//! Convert to Sheet Metal converts a body from its faces (#6748): a new feature type,
//! `sheet_surface_convert`, saves the body, its bend edges, the faces it skips as `skip` and the
//! material's side as `direction` (inside unless saved), with no fixed face. A conversion of
//! picked faces (`sheet_convert`) saves the faces picked after the first as `faces` and the
//! material's side as `direction`. Both save the faces picked to engrave, engravings of the panels
//! round them rather than panels, as `engrave`. A surface body converts too, its faces the sheet's
//! surface. The model cache keeps each sheet made from surfaces as `surface_sheets`. The last
//! release refuses the newer design format instead of dropping them.
//!
//! An older design's `sheet_convert` without bend edges converted a plate of even thickness, the
//! only body it took, as one sheet from the face it picked: as one opens, it is Recognize of that
//! plate from that face under its rule (`sheet_recognize`), which reads a plate as one sheet and
//! keeps its recesses. The sheet is the rule's thickness where the conversion took the plate's
//! own; the design does not say how thick the plate is, so where the two differ the feature says
//! so (`the body is … thick: the sheet is the rule's …`). One with bend edges stays a conversion of
//! the face it picked and every face a bend joins to it, the rule's thickness taken into the body,
//! as a solid's conversion was. Whether its body was a shell, whose walls kept their own
//! thickness, is the body's geometry, which the design does not keep either.

use serde_json::Value;

use crate::Result;

pub fn step(v: &mut Value) -> Result<()> {
    for feature in v.get_mut("features").and_then(Value::as_array_mut).into_iter().flatten() {
        let Some(f) = feature.as_object_mut() else { continue };
        if f.get("type").and_then(Value::as_str) != Some("sheet_convert") {
            continue;
        }
        // Bend edges, or what only this change saves (more faces, engravings, a side): no plate's.
        let listed = |key: &str| f.get(key).and_then(Value::as_array).is_some_and(|l| !l.is_empty());
        let side = f.get("direction").and_then(Value::as_str).is_some_and(|d| !d.is_empty());
        if listed("bends") || listed("faces") || listed("engrave") || side {
            continue;
        }
        f.remove("bends");
        f.insert("type".into(), Value::String("sheet_recognize".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use serde_json::json;
    use solidcore_record::Vec3;

    #[test]
    fn a_plate_conversion_is_recognize_of_the_plate_and_one_with_bend_edges_stays() {
        let tray = json!({
            "id": 6, "name": "C3", "type": "sheet_convert", "body": "Tray", "face": [50, 30, 0], "bends": [[50, 0, 0]], "face_names": ["F5:-z"],
        });
        let mut v = json!({
            "features": [
                {"id": 2, "name": "C1", "type": "sheet_convert", "body": "Plate", "face": [50, 30, 2], "rule": "R1"},
                {"id": 4, "name": "C2", "type": "sheet_convert", "body": "Slab", "face": [0, 0, 1], "bends": []},
                tray.clone(),
                {"id": 7, "name": "C4", "type": "sheet_convert", "body": "Part", "face": [0, 0, 1], "faces": [[0, 0, 0]]},
                {"id": 8, "name": "E1", "type": "extrude", "body": "Plate"},
            ],
        });
        super::step(&mut v).unwrap();
        // The plate's conversion is Recognize of the same plate, from the same face, under the same rule.
        assert_eq!(v["features"][0], json!({"id": 2, "name": "C1", "type": "sheet_recognize", "body": "Plate", "face": [50, 30, 2], "rule": "R1"}));
        let read: crate::Feature = serde_json::from_value(v["features"][0].clone()).unwrap();
        assert_eq!(
            read.kind,
            crate::FeatureKind::SheetRecognize {
                body: crate::BodyName::from("Plate"),
                face: Some(crate::FacePick(Vec3::new(50.0, 30.0, 2.0))),
                skip: Vec::new(),
                bends: Vec::new(),
                rule: Some("R1".into()),
            }
        );
        // An empty list of bend edges is none: a plate too, under the active rule.
        assert_eq!(v["features"][1], json!({"id": 4, "name": "C2", "type": "sheet_recognize", "body": "Slab", "face": [0, 0, 1]}));
        // With bend edges it stays as it was saved, as does one with faces only this change saves.
        assert_eq!(v["features"][2], tray);
        assert_eq!(v["features"][3]["type"], json!("sheet_convert"));
        assert_eq!(v["features"][4]["type"], json!("extrude"), "only conversions");
    }
}
