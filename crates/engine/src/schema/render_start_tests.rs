use serde_json::{Value, json};

use crate::Session;

/// #13311: `engine.describe` gives Render Image and Check Render Request their exact parameters.
#[test]
fn both_commands_describe_their_parameters() {
    let mut s = Session::default();
    for id in ["render.start", "render.preflight"] {
        let described = s.execute("engine.describe", &json!({"command": id})).unwrap_or_else(|e| panic!("{e}"));
        assert!(described["input"].is_object(), "{id}: {described}");
        assert_eq!(described["input"].get("additionalProperties"), Some(&Value::Bool(false)), "{id}: {described}");
    }
}
