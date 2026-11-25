//! Contracts and result types for the commands in `cmd/print_face.rs`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::types::{Point3, StringList};

/// Mark the flat face of a body at a point as the body's print face, or clear it. A slicer
/// export (`file.send_to_slicer`, `file.save_mesh` with `slicer`, `file.open_in_easyprint`), and an
/// STL or 3MF export with `lay_on_print_faces` (`file.export`, `file.export_stl`, `file.export_3mf`,
/// `file.save_mesh`), lays the body on its print face: turned so the face points down and lies at
/// Z = 0, the centre of mass over the origin. A body has one print face at most; marking another
/// replaces it.
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

/// Say whether bodies have a print face (`body.print_face`), without changing anything.
/// Read-only.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PrintFacesParams {
    /// The bodies to check: a name or a list of names. Default: every body of the design.
    #[serde(default)]
    pub bodies: Option<StringList>,
}

/// Whether a body has a print face.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PrintFaceStatus {
    /// It has a print face, and the face is still there and flat: an export lays the body on it.
    Set,
    /// It has no print face: an export writes it as it lies.
    None,
    /// It has one, but the face was cut away or is no longer flat: an export writes the body as
    /// it lies and warns.
    Lost,
}

/// One body's print face.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BodyPrintFace {
    /// The body.
    pub body: String,
    /// Whether it has a print face.
    pub status: PrintFaceStatus,
    /// The persistent name of the face marked as its print face. Absent for `none`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<String>,
    /// For `lost` only: why the face can't be found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

/// What `body.print_faces` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PrintFacesResult {
    /// Each body checked, in the order asked for (the design's order by default).
    pub bodies: Vec<BodyPrintFace>,
    /// True when at least one body was checked and every one has the status `set`.
    pub all_set: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{PrintFaceParams, PrintFacesParams};
    use crate::Session;
    use crate::schema::input_schema_for;
    use crate::schema::tests::assert_keys;

    #[test]
    fn schemas_and_the_key_check_agree() {
        assert_keys(input_schema_for::<PrintFaceParams>(), &["body", "point"]);
        assert_keys(input_schema_for::<PrintFacesParams>(), &["bodies"]);
    }

    #[test]
    fn bad_calls_say_what_is_wrong_and_change_nothing() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 20, "width": 10, "height": 5, "body_name": "Block"})).unwrap_or_else(|e| panic!("{e}"));
        let undo = s.undo.len();
        let e = s.execute("body.print_face", &json!({"point": [20, 5, 2.5]})).unwrap_err().to_string();
        assert!(e.contains("body"), "{e}");
        let e = s.execute("body.print_face", &json!({"body": "Bracket", "point": [20, 5, 2.5]})).unwrap_err().to_string();
        assert!(e.contains("Bracket"), "{e}");
        let e = s.execute("body.print_face", &json!({"body": "Block", "point": "top"})).unwrap_err().to_string();
        assert!(e.contains("point"), "{e}");
        assert!(s.doc.print_faces.is_empty() && s.undo.len() == undo);
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
