//! A variable or spline fillet with `tangent_chain` (the default) now rounds each picked edge's
//! open tangent chain in one blend, its radius law running along the whole chain from the end
//! nearest `start` (#6473). Builds before ran the law along the picked edge alone whatever
//! `tangent_chain` said, so this step saves `tangent_chain: false` on each older variable or
//! spline fillet, which rounds the picked edges alone as they did: every such fillet rebuilds as
//! it was. Fillets added later follow their chains. The last release opens a design with this
//! change with a warning about the newer format; it reads `tangent_chain: false` as it always did.
use serde_json::Value;

pub fn step(v: &mut Value) -> crate::Result<()> {
    for feature in v.get_mut("features").and_then(Value::as_array_mut).into_iter().flatten() {
        if feature.get("type").and_then(Value::as_str) != Some("fillet") {
            continue;
        }
        let law = feature.pointer("/style/type").and_then(Value::as_str);
        if !matches!(law, Some("variable" | "spline")) {
            continue;
        }
        if let Some(o) = feature.as_object_mut() {
            o.insert("tangent_chain".into(), Value::Bool(false));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn older_variable_and_spline_fillets_keep_rounding_their_picked_edges_alone() {
        let mut v = json!({"features": [
            {"type": "fillet", "edges": [[1, 0, 0]], "radius": "2", "style": {"type": "variable", "radius2": "1", "start": [0, 0, 0]}},
            {"type": "fillet", "edges": [[1, 0, 0]], "radius": "2", "style": {"type": "spline", "start": [0, 0, 0], "knots": [0, 0, 1, 1], "radii": ["1"]}, "tangent_chain": true},
            {"type": "fillet", "edges": [[1, 0, 0]], "radius": "2"},
            {"type": "fillet", "edges": [[1, 0, 0]], "radius": "2", "style": {"type": "chord"}},
            {"type": "chamfer", "edges": [[1, 0, 0]], "distance": "1"}
        ]});
        super::step(&mut v).unwrap();
        assert_eq!(v["features"][0]["tangent_chain"], false);
        assert_eq!(v["features"][1]["tangent_chain"], false);
        for i in 2..5 {
            assert!(v["features"][i].get("tangent_chain").is_none(), "{i}");
        }
        let once = v.clone();
        super::step(&mut v).unwrap();
        assert_eq!(v, once);
        let mut empty = json!({"name": "Untitled"});
        super::step(&mut empty).unwrap();
        assert_eq!(empty, json!({"name": "Untitled"}));
    }
}
