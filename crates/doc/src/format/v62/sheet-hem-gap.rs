//! A hem's `gap`, and a sheet metal rule's `hem_gap`, is the space between the hem's leg and the
//! face it folds over; the hem's bend's inner radius is half of it (#6748). Designs saved before
//! this read the same value as the inner radius itself: as one opens, each saved value is doubled
//! (`2 * (…)`), and a rule saved without `hem_gap` gets twice the old default, so every hem rebuilds
//! as it was until its value is edited. The last release refuses a design that has this change (it
//! refuses the newer format number).

use serde_json::Value;

use crate::Result;

/// The default `hem_gap` a rule had when it was the inner radius: twice it is the same hem.
const OLD_DEFAULT_DOUBLED: &str = "0.02 mm";

/// `expr` read as twice itself.
fn doubled(expr: &str) -> String {
    format!("2 * ({expr})")
}

pub fn step(v: &mut Value) -> Result<()> {
    for feature in v.get_mut("features").and_then(Value::as_array_mut).into_iter().flatten() {
        if feature.get("type").and_then(Value::as_str) != Some("sheet_hem") {
            continue;
        }
        if let Some(Value::String(gap)) = feature.get_mut("gap") {
            *gap = doubled(gap);
        }
    }
    for rule in v.pointer_mut("/sheet/rules").and_then(Value::as_array_mut).into_iter().flatten() {
        let Some(rule) = rule.as_object_mut() else { continue };
        match rule.get_mut("hem_gap") {
            Some(Value::String(gap)) => *gap = doubled(gap),
            _ => {
                rule.insert("hem_gap".into(), Value::String(OLD_DEFAULT_DOUBLED.into()));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use serde_json::json;

    #[test]
    fn saved_hem_gaps_double_and_a_rule_without_one_gets_twice_the_old_default() {
        let mut v = json!({
            "features": [
                {"type": "sheet_hem", "edges": [[0, 0, 0]], "length": "10 mm", "gap": "0.5 mm"},
                {"type": "sheet_hem", "edges": [[0, 0, 0]], "length": "10 mm"},
                {"type": "extrude", "gap": "1 mm"},
            ],
            "sheet": {"rules": [{"name": "A", "hem_gap": "T / 4"}, {"name": "B"}]},
        });
        super::step(&mut v).unwrap();
        assert_eq!(v["features"][0]["gap"], json!("2 * (0.5 mm)"));
        assert!(v["features"][1].get("gap").is_none(), "a hem with no gap of its own takes its rule's");
        assert_eq!(v["features"][2]["gap"], json!("1 mm"), "only hems");
        assert_eq!(v["sheet"]["rules"][0]["hem_gap"], json!("2 * (T / 4)"));
        assert_eq!(v["sheet"]["rules"][1]["hem_gap"], json!("0.02 mm"));
    }
}
