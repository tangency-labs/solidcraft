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
    fn weld_named_settings_spans_and_type_sizes() {
        let mut s = Session::default();
        for typ in solidcraft_doc::weld::TYPES {
            let result = s.execute("weld.create", &json!({"weld_type":typ})).unwrap();
            assert!(result["id"].is_u64());
            s.doc.welds.last().unwrap().settings.validate().unwrap();
        }
        assert_eq!(s.doc.welds.len(), 9);
        for invalid in [
            json!({"weld_type":"groove"}),
            json!({"weld_type":"fillet","process":"MAG"}),
            json!({"weld_type":"fillet","process":"1234"}),
            json!({"weld_type":"fillet","process":"１２"}),
            json!({"weld_type":"fillet","side":"left"}),
            json!({"weld_type":"fillet","ends":"round"}),
            json!({"weld_type":"spot","spot_look":"unknown"}),
            json!({"weld_type":"fillet","reference_plane":"0"}),
            json!({"weld_type":"butt","size":{"a":3}}),
            json!({"weld_type":"fillet","size":{"d":5}}),
            json!({"weld_type":"fillet","spans":{"count":1.5,"length_mm":10,"pitch_mm":40}}),
            json!({"weld_type":"fillet","spans":{"count":0,"length_mm":10,"pitch_mm":40}}),
            json!({"weld_type":"fillet","spans":{"count":10001,"length_mm":10,"pitch_mm":40}}),
            json!({"weld_type":"fillet","spans":{"count":2,"length_mm":0,"pitch_mm":40}}),
            json!({"weld_type":"fillet","spans":{"count":2,"length_mm":10,"pitch_mm":-1}}),
        ] {
            let before = s.doc.clone();
            assert!(matches!(s.execute("weld.create", &invalid), Err(crate::EngineError::BadParams { .. })), "{invalid}");
            assert_eq!(s.doc, before);
        }
        s.execute("weld.create", &json!({"weld_type":"butt","size":{"h":"1 mm"},"process":"135","side":"other","ends":"rounded"})).unwrap();
        let record = s.doc.welds.last().unwrap();
        assert!(record.settings.density.is_none());
        assert_eq!(record.settings.size.len(), 1);
        assert_eq!(record.settings.side, "other");
        s.execute("weld.create", &json!({"weld_type":"fillet","spans":{"count":2,"length_mm":"1 cm","pitch_mm":"40 mm"}})).unwrap();
        let spans = s.doc.welds.last().unwrap().settings.spans.as_ref().unwrap();
        assert_eq!((spans.count, spans.length_mm, spans.pitch_mm), (2, 10., 40.));
    }
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
                "add_fillet",
                "all_around",
                "blend_toes",
                "both_sides",
                "density",
                "different_halves",
                "ends",
                "face",
                "field_weld",
                "groove",
                "merge_crossing",
                "overlap",
                "passes_shown",
                "penetration",
                "penetration_shown",
                "picks",
                "process",
                "reference_plane",
                "root",
                "root_gap",
                "sharp_mitre",
                "sheet_picked",
                "side",
                "size",
                "size_by_leg",
                "spans",
                "spot_look",
                "staggered",
                "symbol",
                "symbol_standard",
                "tack_mode",
                "tangent_chain",
                "weld_type",
            ],
        );
        let mut s = Session { param_check: ParamCheck::Warn, ..Default::default() };
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
    /// Type id: fillet, butt, edge, plug, spot, seam, stud, surfacing or tack.
    pub weld_type: String,
    /// Symbol convention: iso2553_2019_a (default), iso2553_2019_b or aws_a2_4_2020.
    pub symbol_standard: Option<String>,
    /// Butt groove: square, v (default), bevel, u, j, y, hy, x or k.
    pub groove: Option<String>,
    /// Butt penetration: full (default, no s) or partial (requires s).
    pub penetration: Option<String>,
    /// Butt root: as_welded (default), melt_through or backing_run.
    pub root: Option<String>,
    /// Tack placement: pitch (default), count or points. Size n is a positive integer.
    pub tack_mode: Option<String>,
    /// Optional root gap in mm (zero is allowed).
    pub root_gap: Option<LengthExpr>,
    /// Add a fillet to the butt weld. Default: false.
    pub add_fillet: Option<bool>,
    /// Pick groups: line, groove, point, hole, ring or area. Default: empty.
    #[serde(default)]
    pub picks: Option<BTreeMap<String, Vec<WeldPickParams>>>,
    /// Type-specific positive sizes: a,z,z1,z2,h,s,d,w,t,l,e (length expressions), n (unitless positive integer).
    #[serde(default)]
    pub size: Option<BTreeMap<String, LengthExpr>>,
    /// Face: flat, convex or concave. Default: flat for butt/plug, convex otherwise.
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
    /// Optional intermittent elements: count, length_mm and pitch_mm.
    #[serde(default)]
    pub spans: Option<WeldSpansParams>,
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
    /// Semantic side: arrow (default) or other.
    #[serde(default)]
    pub side: Option<String>,
    /// End treatment: square (default), tapered or rounded.
    #[serde(default)]
    pub ends: Option<String>,
    /// Optional ISO 4063 number as 1-3 ASCII digits, e.g. "135".
    #[serde(default)]
    pub process: Option<String>,
    /// Spot appearance: marks, ball, marks_ball, marks_pin, pin or dent.
    #[serde(default)]
    pub spot_look: Option<String>,
    /// Reference plane: sketch.
    #[serde(default)]
    pub reference_plane: Option<String>,
    /// Material density in kg/m³, as a number or unitless expression. Omitted: no mass.
    #[serde(default)]
    pub density: Option<NumberExpr>,
    /// Optional overlap (mm); omitted follows h.
    #[serde(default)]
    pub overlap: Option<LengthExpr>,
    /// Welding symbol built from the settings; validated using the geometry record's rules.
    #[serde(default)]
    #[schemars(with = "Option<SymbolSchema>")]
    pub symbol: Option<solidcraft_geom::weld::Weld>,
}

/// Intermittent elements; dimensions accept length expressions in mm.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WeldSpansParams {
    pub count: u32,
    pub length_mm: LengthExpr,
    pub pitch_mm: LengthExpr,
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
