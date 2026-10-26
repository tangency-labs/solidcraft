//! Expressions know mass, volume and area units (#4811): `g`, `kg`, `lb`, `ml`, `mm³`, `cm³`, `m³`,
//! `mm²`, `cm²` and `m²`, and a parameter can be a mass, a volume or an area. A unit's name can't
//! be a parameter's, so this step lists the parameters, text parameters and feature inputs of an
//! older design called one of these words, and the design renames each as it opens (`g` to
//! `g_param`): every expression and text that uses it follows as the older build read it (`2 g`
//! was 2 × `g`), its comment says why, and no value changes. The words are this change's own,
//! kept here as they were made: a later unit word brings its own step. The last release refuses a
//! design saved with this (the newer format number) instead of failing to evaluate `50 g`.
use serde_json::Value;

/// The words this change made units (not `expr::UNITS`, which grows).
const WORDS: [&str; 10] = ["g", "kg", "lb", "ml", "mm³", "cm³", "m³", "mm²", "cm²", "m²"];

pub fn step(v: &mut Value) -> crate::Result<()> {
    super::list_unit_word_params(v, &WORDS)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn older_names_that_are_now_units_are_listed_for_renaming() {
        let mut v = json!({
            "params": [{"name": "g"}, {"name": "width"}, {"name": "kg"}, {"name": "g"}],
            "text_params": [{"name": "ml", "text": "A"}, {"name": "tag", "text": "B"}],
            "features": [{"type": "box", "param_names": ["d1", "lb"]}, {"param_names": ["mm³", "m²"]}, {"type": "sketch"}]
        });
        super::step(&mut v).unwrap();
        assert_eq!(v["unit_word_params"], json!(["g", "kg", "lb", "ml", "mm³", "m²"]));
        // Other words, other changes' units and ASCII powers aren't listed, and nothing changes.
        let mut free = json!({
            "params": [{"name": "l"}, {"name": "L"}, {"name": "t"}, {"name": "m3"}, {"name": "cm2"}, {"name": "oz"}, {"name": "mm"}],
            "features": [{"param_names": ["G", "gram", "kg_", ""]}]
        });
        let before = free.clone();
        super::step(&mut free).unwrap();
        assert_eq!(free, before);
        // A list an earlier step made is kept, and added to.
        let mut earlier = json!({"params": [{"name": "kg"}], "unit_word_params": ["oz"]});
        super::step(&mut earlier).unwrap();
        assert_eq!(earlier["unit_word_params"], json!(["oz", "kg"]));
        // Damaged shapes are passed over.
        for hostile in [
            json!(null),
            json!([]),
            json!("g"),
            json!({"params": "g"}),
            json!({"params": [5, null, {"name": 7}, {"name": ["g"]}]}),
            json!({"features": [{"param_names": "g"}, {"param_names": [null, 1, {"g": 1}]}, 3]}),
            json!({"features": {"param_names": ["g"]}}),
        ] {
            let mut v = hostile.clone();
            super::step(&mut v).unwrap();
            assert_eq!(v, hostile);
        }
    }

    /// A hostile file's size can't make the step slow: an earlier list over 1000 names is refused
    /// (as the design read refuses it), and names are read once each, so 100,000 parameters called
    /// `g` after a full earlier list are listed once, not compared with it 100,000 times.
    #[test]
    fn hostile_sizes_are_refused_or_read_once() {
        let names = |n: usize| (0..n).map(|i| format!("w{i}")).collect::<Vec<_>>();
        let g_params = || vec![json!({"name": "g"}); 100_000];
        let mut long = json!({"params": g_params(), "unit_word_params": names(1001)});
        let error = super::step(&mut long).unwrap_err().to_string();
        assert!(error.contains("damaged design: `unit_word_params` is not a short list of names"), "{error}");
        drop(long);
        let mut full = json!({"params": g_params(), "features": [{"param_names": vec!["kg"; 100_000]}], "unit_word_params": names(1000)});
        super::step(&mut full).unwrap();
        let listed = full["unit_word_params"].as_array().unwrap();
        assert_eq!(listed.len(), 1002);
        assert_eq!((&listed[1000], &listed[1001]), (&json!("g"), &json!("kg")));
    }
}
