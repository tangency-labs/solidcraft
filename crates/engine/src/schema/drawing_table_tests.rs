use serde_json::json;

use super::*;

/// What the commands take is read strictly, and a table's id is only that.
#[test]
fn table_params_are_read_strictly() {
    let id = solidcraft_doc::ids::encode(7);
    let read = |p| serde_json::from_value::<EditParams>(p);
    assert!(read(json!({"table": id, "set": {"heading": "top"}})).is_ok());
    assert!(read(json!({"table": 7, "set": {}})).is_err());
    assert!(read(json!({"table": id, "set": {"widht": 3}})).is_err());
    assert!(read(json!({"table": id})).is_err());
    assert!(serde_json::from_value::<AddParams>(json!({"sheet": "Sheet 1", "kind": "chart"})).is_err());
}
