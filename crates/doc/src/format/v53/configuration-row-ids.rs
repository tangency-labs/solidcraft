//! Configuration rows save a fixed id (`id`, #14750), from the design's id counter: a rename keeps
//! it and no other row is ever given it, so what points at a row still finds it after a rename.
//! The table still saves its active row by name. An older design's rows get their ids as it opens,
//! in row order from its saved `next_id` (past any id a row already has), and `next_id` moves past
//! them. The last release warns about the newer format number, opens the design with its whole
//! table and keeps the row ids it can't read.

use serde_json::Value;

use crate::{DocError, Result};

pub fn step(v: &mut Value) -> Result<()> {
    let saved = v.get("next_id").and_then(Value::as_u64).unwrap_or(0);
    let Some(rows) = v.pointer_mut("/configs/rows").and_then(Value::as_array_mut) else { return Ok(()) };
    let past = |id: u64| id.checked_add(1).ok_or_else(|| DocError::Invalid("damaged design: the id space is exhausted".into()));
    let mut next = saved.max(1);
    if let Some(largest) = rows.iter().filter_map(|row| row.get("id")?.as_u64()).max() {
        next = next.max(past(largest)?);
    }
    let first = next;
    for row in rows.iter_mut().filter_map(Value::as_object_mut) {
        if !row.contains_key("id") {
            row.insert("id".into(), next.into());
            next = past(next)?;
        }
    }
    if next != first
        && let Some(design) = v.as_object_mut()
    {
        design.insert("next_id".into(), next.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::step;
    use serde_json::{Value, json};

    fn design(next_id: Option<u64>, rows: Value) -> Value {
        let mut v = json!({"name": "Plate", "configs": {"columns": [], "rows": rows, "active": "Long"}});
        if let Some(next) = next_id {
            v["next_id"] = next.into();
        }
        v
    }

    fn ids(v: &Value) -> Vec<Option<u64>> {
        v["configs"]["rows"].as_array().map_or(Vec::new(), |rows| rows.iter().map(|row| row["id"].as_u64()).collect())
    }

    /// Rows get ids in order from the saved counter, past any id a row has, and the counter moves
    /// past them; the active row stays named.
    #[test]
    fn an_older_designs_rows_get_ids_in_order_from_its_counter() {
        let rows = json!([{"name": "Short", "cells": []}, {"name": "Long", "cells": []}]);
        let mut v = design(Some(7), rows.clone());
        step(&mut v).unwrap();
        assert_eq!((ids(&v), &v["next_id"], &v["configs"]["active"]), (vec![Some(7), Some(8)], &json!(9), &json!("Long")));
        // No counter: from 1.
        let mut v = design(None, rows);
        step(&mut v).unwrap();
        assert_eq!((ids(&v), &v["next_id"]), (vec![Some(1), Some(2)], &json!(3)));
        // A row that has an id keeps it, and the others go past it.
        let mut v = design(Some(2), json!([{"name": "Short", "cells": []}, {"id": 40, "name": "Long", "cells": []}]));
        step(&mut v).unwrap();
        assert_eq!((ids(&v), &v["next_id"]), (vec![Some(41), Some(40)], &json!(42)));
    }

    /// A design with no table, or whose rows all have ids, is left as it is.
    #[test]
    fn a_design_with_nothing_to_give_is_left_as_it_is() {
        for mut v in
            [json!({"name": "Plate", "next_id": 3}), design(Some(3), json!([{"id": 1, "name": "Long", "cells": []}])), design(None, json!([]))]
        {
            let before = v.clone();
            step(&mut v).unwrap();
            assert_eq!(v, before);
        }
    }

    /// A counter at the end of the id space can give no id: the design is refused, not given a
    /// repeated one.
    #[test]
    fn a_counter_at_the_end_is_refused() {
        let mut v = design(Some(u64::MAX), json!([{"name": "Long", "cells": []}]));
        assert!(step(&mut v).unwrap_err().to_string().contains("id space is exhausted"));
        let mut v = design(Some(1), json!([{"id": u64::MAX, "name": "Short", "cells": []}, {"name": "Long", "cells": []}]));
        assert!(step(&mut v).is_err());
    }

    /// The largest table a design may hold gets one id per row, in order, in one pass.
    #[test]
    fn the_largest_table_gets_an_id_for_every_row() {
        let rows: Vec<Value> = (0..crate::config::MAX_ROWS).map(|i| json!({"name": format!("Row {i}"), "cells": []})).collect();
        let mut v = design(Some(5), Value::Array(rows));
        step(&mut v).unwrap();
        let expected: Vec<Option<u64>> = (5..5 + crate::config::MAX_ROWS as u64).map(Some).collect();
        assert_eq!(ids(&v), expected);
        assert_eq!(v["next_id"], json!(5 + crate::config::MAX_ROWS as u64));
    }

    /// An older design opens with an id for every row, its active row still matching, and the
    /// design's next id past them; rows that repeat an id are a damaged design.
    #[test]
    fn an_older_design_opens_with_row_ids_and_its_active_row() {
        let mut d = crate::Document::new("Plate");
        d.set_param("L", "25 mm", None, None).unwrap();
        d.configs.columns = vec![crate::config::Column::Param("L".into())];
        let ids = [d.allocate_id().unwrap(), d.allocate_id().unwrap()];
        d.configs.rows = vec![
            crate::config::ConfigRow { id: ids[0], name: "Short".into(), cells: vec!["25 mm".into()] },
            crate::config::ConfigRow { id: ids[1], name: "Long".into(), cells: vec!["30 mm".into()] },
        ];
        d.apply_configuration("Long").unwrap();
        let mut old: Value = serde_json::from_str(&d.to_json()).unwrap();
        old["format"] = Value::from("solidcraft/6");
        for row in old["configs"]["rows"].as_array_mut().unwrap() {
            row.as_object_mut().unwrap().remove("id");
        }
        let opened = crate::format::read(&old.to_string()).unwrap();
        let given: Vec<u64> = opened.configs.rows.iter().map(|row| row.id).collect();
        assert!(given[0] != given[1] && given.iter().all(|id| *id > 0), "{given:?}");
        assert!(opened.next_id > given[0].max(given[1]));
        assert_eq!(opened.configs.active_row().map(|row| row.name.as_str()), Some("Long"));
        assert!(opened.configuration_matches());
        let mut twice: Value = serde_json::from_str(&d.to_json()).unwrap();
        twice["configs"]["rows"][1]["id"] = twice["configs"]["rows"][0]["id"].clone();
        let refused = crate::format::read(&twice.to_string()).unwrap_err().to_string();
        assert!(refused.contains("configuration row id"), "{refused}");
    }

    /// What the last release does with a row field it doesn't know, as it does with `id`: the
    /// table opens whole and the field goes back into the file when it saves.
    #[test]
    fn a_newer_designs_unknown_row_field_keeps_the_table_whole() {
        let mut d = crate::Document::new("Plate");
        d.set_param("L", "25 mm", None, None).unwrap();
        d.configs.columns = vec![crate::config::Column::Param("L".into())];
        d.configs.rows = vec![crate::config::ConfigRow { id: d.allocate_id().unwrap(), name: "Long".into(), cells: vec!["30 mm".into()] }];
        d.configs.active = Some("Long".into());
        let mut newer: Value = serde_json::from_str(&d.to_json()).unwrap();
        newer["format"] = Value::from(crate::format::format_string(crate::format::FORMAT + 1));
        newer["configs"]["rows"][0]["revision"] = json!(3);
        let opened = crate::format::read(&newer.to_string()).unwrap();
        assert_eq!(opened.configs.rows.len(), 1);
        assert_eq!(opened.configs.active.as_deref(), Some("Long"));
        let saved: Value = serde_json::from_str(&opened.to_json()).unwrap();
        assert_eq!(saved["configs"], newer["configs"], "kept for the build that reads it");
    }
}
