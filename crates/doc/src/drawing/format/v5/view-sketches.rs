//! A drawing's sketches save their owner (`owner`, #5425). A sheet owns a sketch drawn in paper
//! millimetres (`{"sheet": id}`). A view owns a sketch drawn in model millimetres in the view's
//! plane, placed on paper at the view's scale (`{"view": id}`). An older drawing saves a sheet
//! sketch's sheet as `sheet`. Here each such record gets `owner.sheet` instead, with the same id and
//! geometry. The last release has no sketches: it warns about the newer format number, opens the
//! rest of the drawing and keeps the sketches as they were.
use serde_json::{Value, json};

pub fn step(v: &mut Value) -> crate::Result<()> {
    let Some(sketches) = v.get_mut("sketches").and_then(Value::as_array_mut) else { return Ok(()) };
    // A record that already has an owner, or is not a record, is left for the reader to judge:
    // the step moves a field, it never repairs a damaged file.
    for record in sketches.iter_mut().filter_map(Value::as_object_mut).filter(|record| !record.contains_key("owner")) {
        if let Some(sheet) = record.remove("sheet") {
            record.insert("owner".into(), json!({ "sheet": sheet }));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    /// Each older sheet sketch keeps its id, name, geometry and sheet, now saved as its owner. A
    /// record that already has an owner, or has both fields, or is not a record, is left as it was,
    /// and a drawing with no sketches is not touched.
    #[test]
    fn an_older_drawings_sheet_sketches_save_their_sheet_as_their_owner() {
        let sketch = json!({"points": [{"pos": [15.0, 10.0]}], "curves": [], "constraints": []});
        let old = |id: &str, sheet: &str| json!({"id": id, "sheet": sheet, "name": "Outline", "sketch": sketch});
        let both = json!({"id": "000000000000c", "sheet": "0000000000001", "owner": {"view": "0000000000003"}, "name": "Both", "sketch": sketch});
        let owned = json!({"id": "000000000000d", "owner": {"view": "0000000000003"}, "name": "On a view", "sketch": sketch});
        let mut v = json!({"sketches": [old("000000000000a", "0000000000001"), old("000000000000b", "0000000000002"), both, owned, null]});
        let before = v.clone();
        super::step(&mut v).unwrap();
        assert_eq!(v["sketches"][0], json!({"id": "000000000000a", "owner": {"sheet": "0000000000001"}, "name": "Outline", "sketch": sketch}));
        assert_eq!(v["sketches"][1]["owner"], json!({"sheet": "0000000000002"}));
        assert_eq!(v["sketches"][1].get("sheet"), None);
        assert_eq!(v["sketches"].as_array().unwrap()[2..], before["sketches"].as_array().unwrap()[2..]);
        let mut empty = json!({"name": "Untitled"});
        super::step(&mut empty).unwrap();
        assert_eq!(empty, json!({"name": "Untitled"}));
    }
}
