use serde_json::json;

use super::{CutList, CutListParams};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schema_keys_and_strict_options_match_the_cut_list_contract() {
    assert_keys(input_schema_for::<CutListParams>(), &["component", "format", "group_by", "include", "path"]);
    let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
    let reply = s.execute("cutlist.parts", &json!({"include":"bodies","group_by":["material","stock"]})).unwrap();
    let schema = output_schema_for::<CutList>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply));
    for p in [json!({"incldue":"bodies"}), json!({"include":false}), json!({"group_by":["name"]}), json!({"component":[]})] {
        assert!(s.execute("cutlist.parts", &p).is_err());
    }
}
