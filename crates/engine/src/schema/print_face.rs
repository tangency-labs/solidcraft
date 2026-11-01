//! Contracts and result types for the commands in `cmd/print_face.rs`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::types::Point3;

/// Mark the flat face of a body at a point as the body's print face, or clear it. A slicer
/// export (`file.send_to_slicer`, `file.save_mesh` with `slicer`, `file.open_in_easyprint`) lays
/// the body on its print face: turned so the face points down and lies at Z = 0, the centre of
/// mass over the origin. A body has one print face at most; marking another replaces it.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PrintFaceParams {
    /// The body, by name. Solid bodies only: a mesh body can't have a print face yet.
    pub body: String,
    /// A point on the flat face to mark, in mm. Default: none, which clears the body's print face.
    #[serde(default)]
    pub point: Option<Point3>,
}

/// What `body.print_face` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PrintFaceResult {
    /// The body.
    pub body: String,
    /// The persistent name of the face now marked. Absent when the print face was cleared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::PrintFaceParams;
    use crate::Session;
    use crate::schema::input_schema_for;
    use crate::schema::tests::assert_keys;

    #[test]
    fn schemas_and_the_key_check_agree() {
        assert_keys(input_schema_for::<PrintFaceParams>(), &["body", "point"]);
    }

    #[test]
    fn bad_calls_say_what_is_wrong_and_change_nothing() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 20, "width": 10, "height": 5, "body_name": "Block"})).unwrap_or_else(|e| panic!("{e}"));
        let undo = s.history.len();
        let e = s.execute("body.print_face", &json!({"point": [20, 5, 2.5]})).unwrap_err().to_string();
        assert!(e.contains("body"), "{e}");
        let e = s.execute("body.print_face", &json!({"body": "Bracket", "point": [20, 5, 2.5]})).unwrap_err().to_string();
        assert!(e.contains("Bracket"), "{e}");
        let e = s.execute("body.print_face", &json!({"body": "Block", "point": "top"})).unwrap_err().to_string();
        assert!(e.contains("point"), "{e}");
        assert!(s.doc.print_faces.is_empty() && s.history.len() == undo);
    }

    #[test]
    fn the_reply_names_the_marked_face_and_leaves_it_out_when_cleared() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 20, "width": 10, "height": 5, "body_name": "Block"})).unwrap_or_else(|e| panic!("{e}"));
        let r = s.execute("body.print_face", &json!({"body": "Block", "point": [20, 5, 2.5]})).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(r["body"], "Block");
        assert!(r["face"].as_str().is_some_and(|f| !f.is_empty()), "{r}");
        let r = s.execute("body.print_face", &json!({"body": "Block"})).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(r["body"], "Block");
        assert!(r.get("face").is_none(), "{r}");
    }
}
