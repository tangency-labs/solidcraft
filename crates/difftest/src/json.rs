//! Canonical JSON output: sorted keys, 2-space indent, shortest round-trip
//! float formatting, non-finite floats as strings. Independent of whether
//! `serde_json`'s `preserve_order` feature is enabled anywhere in the graph.

use serde_json::Value;

/// JSON value for an `f64`: a number if finite, else `"NaN"`, `"inf"`, `"-inf"`.
pub fn num(x: f64) -> Value {
    if x.is_finite() {
        serde_json::Number::from_f64(x).map_or(Value::Null, Value::Number)
    } else if x.is_nan() {
        Value::String("NaN".into())
    } else if x > 0.0 {
        Value::String("inf".into())
    } else {
        Value::String("-inf".into())
    }
}

/// Serialises `v` canonically (trailing newline included).
pub fn to_canonical_string(v: &Value) -> String {
    let mut s = String::new();
    write(&mut s, v, 0);
    s.push('\n');
    s
}

fn indent(s: &mut String, n: usize) {
    for _ in 0..n {
        s.push_str("  ");
    }
}

fn scalar(v: &Value) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "null".into())
}

fn write(s: &mut String, v: &Value, depth: usize) {
    match v {
        Value::Object(o) if !o.is_empty() => {
            let mut keys: Vec<&String> = o.keys().collect();
            keys.sort();
            s.push_str("{\n");
            for (i, k) in keys.iter().enumerate() {
                indent(s, depth + 1);
                s.push_str(&scalar(&Value::String((*k).clone())));
                s.push_str(": ");
                if let Some(x) = o.get(*k) {
                    write(s, x, depth + 1);
                }
                if i + 1 < keys.len() {
                    s.push(',');
                }
                s.push('\n');
            }
            indent(s, depth);
            s.push('}');
        }
        Value::Array(a) if !a.is_empty() => {
            // Arrays of scalars stay on one line.
            if a.iter().all(|x| !x.is_object() && !x.is_array()) {
                let parts: Vec<String> = a.iter().map(scalar).collect();
                s.push('[');
                s.push_str(&parts.join(", "));
                s.push(']');
                return;
            }
            s.push_str("[\n");
            for (i, x) in a.iter().enumerate() {
                indent(s, depth + 1);
                write(s, x, depth + 1);
                if i + 1 < a.len() {
                    s.push(',');
                }
                s.push('\n');
            }
            indent(s, depth);
            s.push(']');
        }
        other => s.push_str(&scalar(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorted_and_stable() {
        let v =
            serde_json::json!({"b": 1, "a": [1.5, 2], "c": {"z": num(f64::NAN), "y": num(0.1)}});
        let s = to_canonical_string(&v);
        assert_eq!(
            s,
            "{\n  \"a\": [1.5, 2],\n  \"b\": 1,\n  \"c\": {\n    \"y\": 0.1,\n    \"z\": \"NaN\"\n  }\n}\n"
        );
    }
}
