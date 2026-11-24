//! Contracts and replies for saved weld graphics.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Session,
        schema::{
            ParamCheck, input_schema_for, output_schema_for,
            tests::{assert_keys, conforms},
        },
    };
    use serde_json::json;
    #[test]
    fn weld_schemas_match_keys_and_replies() {
        crate::schema::tests::assert_strict_keeps_todays_errors(&[
            ("weld.create", json!({"weld_type":"unknown"})),
            ("weld.create", json!({"weld_type":"fillet","size":{"a":-1}})),
            ("weld.create", json!({"weld_type":"fillet","face":"unknown"})),
        ]);
        assert_keys(
            input_schema_for::<WeldCreateParams>(),
            &[
                "all_around",
                "blend_toes",
                "both_sides",
                "density",
                "different_halves",
                "edge_distance",
                "ends",
                "face",
                "field_weld",
                "flip_side",
                "merge_crossing",
                "overlap",
                "passes_shown",
                "pattern_count",
                "pattern_pitch",
                "penetration_shown",
                "picks",
                "process",
                "reference_plane",
                "root_side",
                "sharp_mitre",
                "sheet_picked",
                "size",
                "size_by_leg",
                "spans",
                "spot_look",
                "staggered",
                "symbol",
                "tack",
                "tangent_chain",
                "texture",
                "weld_type",
            ],
        );
        let mut s = Session::default();
        s.param_check = ParamCheck::Warn;
        let schema = serde_json::to_value(output_schema_for::<WeldCreated>()).unwrap();
        for p in [json!({"weld_type":"fillet"}), json!({"weld_type":"spot","size":{"d":"5 mm"},"picks":{"point":[{"point":{"x":0,"y":0,"z":0}}]}})] {
            let r = s.execute("weld.create", &p).unwrap();
            assert!(r.get("param_warnings").is_none(), "{r}");
            assert!(conforms(&schema, &schema, &r), "{r}");
        }
        let warned = s.execute("weld.create", &json!({"weld_type":"fillet","typo":true}));
        // The handler rejects unknown saved settings even when schema checking only warns.
        assert!(warned.is_err());
        s.param_check = ParamCheck::Strict;
        assert!(s.execute("weld.create", &json!({"weld_type":"fillet","typo":true})).is_err());
        let id = s.doc.welds[0].id;
        let reply = s.execute("weld.delete", &json!({"id":id})).unwrap();
        let schema = serde_json::to_value(output_schema_for::<WeldDeleted>()).unwrap();
        assert!(conforms(&schema, &schema, &reply));
    }
}
use super::types::{LengthExpr, NumberExpr, Point3};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Save one weld graphic, without adding timeline features or bodies.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct WeldCreateParams {
    /// Type id from the weld panel (fillet, groove, spot, plug, etc.).
    pub weld_type: String,
    /// Pick groups: line, groove, point, hole, ring or area. Default: empty.
    #[serde(default)]
    pub picks: Option<BTreeMap<String, Vec<WeldPickParams>>>,
    /// Named positive sizes in mm or length expressions: a,z,z1,h,s,d,fill,collar,t,w. Default: panel sizes.
    #[serde(default)]
    pub size: Option<BTreeMap<String, LengthExpr>>,
    /// Face: flat, convex or concave. Default: convex.
    #[serde(default)]
    pub face: Option<String>,
    /// size by leg. Default: false.
    #[serde(default)]
    pub size_by_leg: Option<bool>,
    /// blend toes. Default: false.
    #[serde(default)]
    pub blend_toes: Option<bool>,
    /// sheet picked. Default: false.
    #[serde(default)]
    pub sheet_picked: Option<bool>,
    /// tangent chain. Default: false.
    #[serde(default)]
    pub tangent_chain: Option<bool>,
    /// flip side. Default: false.
    #[serde(default)]
    pub flip_side: Option<bool>,
    /// spans. Default: false.
    #[serde(default)]
    pub spans: Option<bool>,
    /// both sides. Default: false.
    #[serde(default)]
    pub both_sides: Option<bool>,
    /// staggered. Default: false.
    #[serde(default)]
    pub staggered: Option<bool>,
    /// sharp mitre. Default: false.
    #[serde(default)]
    pub sharp_mitre: Option<bool>,
    /// merge crossing. Default: true.
    #[serde(default)]
    pub merge_crossing: Option<bool>,
    /// different halves. Default: false.
    #[serde(default)]
    pub different_halves: Option<bool>,
    /// tack. Default: false.
    #[serde(default)]
    pub tack: Option<bool>,
    /// field weld. Default: false.
    #[serde(default)]
    pub field_weld: Option<bool>,
    /// all around. Default: false.
    #[serde(default)]
    pub all_around: Option<bool>,
    /// passes shown. Default: false.
    #[serde(default)]
    pub passes_shown: Option<bool>,
    /// penetration shown. Default: false.
    #[serde(default)]
    pub penetration_shown: Option<bool>,
    /// Root side: 0 is arrow side, 1 is other side. Default: 0.
    #[serde(default)]
    pub root_side: Option<usize>,
    /// End treatment: 0 square, 1 tapered, 2 rounded. Default: 0.
    #[serde(default)]
    pub ends: Option<usize>,
    /// Process: 0 MIG, 1 TIG, 2 stick, 3 submerged arc, 4 laser, 5 friction stir, 6 unset. Default: 0.
    #[serde(default)]
    pub process: Option<usize>,
    /// Texture specimen index (0..=6), following the process choices. Default: 0.
    #[serde(default)]
    pub texture: Option<usize>,
    /// Spot appearance specimen index (0..=5). Default: 0.
    #[serde(default)]
    pub spot_look: Option<usize>,
    /// Reference plane choice index. Currently only 0 (off) is supported. Default: 0.
    #[serde(default)]
    pub reference_plane: Option<usize>,
    /// pattern count (unitless). Default: 4.
    #[serde(default)]
    pub pattern_count: Option<NumberExpr>,
    /// Centre-to-centre pattern pitch in mm. Default: 40.
    #[serde(default)]
    pub pattern_pitch: Option<LengthExpr>,
    /// edge distance (mm). Default: 12.
    #[serde(default)]
    pub edge_distance: Option<LengthExpr>,
    /// Material density in kg/m³, as a number or unitless expression. Default: 7850.
    #[serde(default)]
    pub density: Option<NumberExpr>,
    /// overlap (mm). Default: 1.5.
    #[serde(default)]
    pub overlap: Option<LengthExpr>,
    /// Optional manual welding symbol; validated using the geometry record's rules.
    #[serde(default)]
    #[schemars(with = "Option<SymbolSchema>")]
    pub symbol: Option<solidcraft_geom::weld::Weld>,
}

/// A fixed point, or an edge/face with a body name and exact topology index.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct WeldPickParams {
    /// Fixed point in mm; cannot have body/index or edge/face.
    #[serde(default)]
    pub point: Option<Point3>,
    /// Point on the edge in mm; requires body and index.
    #[serde(default)]
    pub edge: Option<Point3>,
    /// Point on the face in mm; requires body and index.
    #[serde(default)]
    pub face: Option<Point3>,
    /// Model body name for an edge or face. A body/index pair alone means a face in groove/area groups, otherwise an edge.
    #[serde(default)]
    pub body: Option<String>,
    /// Zero-based exact edge/face index.
    #[serde(default)]
    pub index: Option<usize>,
}

/// Optional manufacturing symbol, independent of bead geometry.
#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SymbolSchema {
    pub arrow_side: Option<SymbolSide>,
    pub other_side: Option<SymbolSide>,
    pub all_round: bool,
    pub field: bool,
    pub tail: Option<String>,
}
#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SymbolSide {
    pub kind: SymbolKind,
    pub size: Option<SymbolSize>,
    pub length_mm: Option<f64>,
    pub intermittent: Option<Intermittent>,
    pub contour: Option<Contour>,
}
#[derive(JsonSchema)]
#[schemars(rename_all = "snake_case")]
pub enum SymbolKind {
    Fillet,
    SquareButt,
    SingleVButt,
}
#[derive(JsonSchema)]
#[schemars(tag = "kind", content = "mm", rename_all = "snake_case", deny_unknown_fields)]
pub enum SymbolSize {
    Leg(f64),
    Throat(f64),
    DeepPenetrationThroat { total_mm: f64, nominal_mm: f64 },
    ButtPenetration(f64),
}
#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct Intermittent {
    pub count: Option<u16>,
    pub pitch_mm: f64,
}
#[derive(JsonSchema)]
#[schemars(rename_all = "snake_case")]
pub enum Contour {
    Flush,
    Convex,
    Concave,
}

/// Identity assigned to the new weld graphic.
#[derive(Debug, Serialize, JsonSchema)]
pub struct WeldCreated {
    /// Shared document identity.
    pub id: u64,
    /// Visible weld number, W1, W2, etc.
    pub number: String,
    /// Parameter diagnostics in warn mode.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
/// Remove one saved weld graphic.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct WeldDeleteParams {
    /// Weld identity returned by weld.create.
    pub id: u64,
}
/// Identity of the removed graphic.
#[derive(Debug, Serialize, JsonSchema)]
pub struct WeldDeleted {
    pub deleted: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
