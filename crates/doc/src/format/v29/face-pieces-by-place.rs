//! The pieces of a split face are numbered by place (#3902): `#1`, `#2`, … follow the spread of
//! their centres, not the kernel's face order or the index of the operand face they came from,
//! which can change from one evaluation to the next. Older designs' stored `#k` names mean the
//! old numbering, so this step marks each of their features `kernel_piece_order`, which keeps
//! it for them. Features added later number by place. The last release refuses a design saved
//! with this (the newer format number).
use serde_json::Value;

pub fn step(v: &mut Value) -> crate::Result<()> {
    let Some(Value::Array(features)) = v.get_mut("features") else {
        return Ok(());
    };
    for feature in features {
        if let Value::Object(o) = feature {
            o.insert("kernel_piece_order".into(), Value::Bool(true));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn older_features_keep_their_piece_numbering() {
        let mut v = json!({"features": [{"type": "sketch"}, {"type": "extrude"}]});
        super::step(&mut v).unwrap();
        assert!(v["features"].as_array().unwrap().iter().all(|f| f["kernel_piece_order"] == true));
        let mut empty = json!({});
        super::step(&mut empty).unwrap();
        assert_eq!(empty, json!({}));
    }
}
