//! Helpers to read hostile JSON parameters.

use serde_json::Value;
use solidcraft_doc::Expr;
use solidcraft_geom::{Vec2, Vec3};

use crate::{EngineError, Result, kernel};

pub fn bad(cmd: &str, msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: cmd.to_string(), msg: msg.into() }
}

pub fn finite(x: f64) -> Option<f64> {
    (x.is_finite() && x.abs() < 1e9).then_some(x)
}

pub fn num(p: &Value, k: &str) -> Option<f64> {
    p.get(k).and_then(Value::as_f64).and_then(finite)
}

pub fn str_<'a>(p: &'a Value, k: &str) -> Option<&'a str> {
    p.get(k).and_then(Value::as_str)
}

pub fn bool_(p: &Value, k: &str) -> Option<bool> {
    p.get(k).and_then(Value::as_bool)
}

pub fn vec2(v: &Value) -> Option<Vec2> {
    match v {
        Value::Array(a) if a.len() == 2 => Some(Vec2::new(finite(a.first()?.as_f64()?)?, finite(a.get(1)?.as_f64()?)?)),
        Value::Object(o) => Some(Vec2::new(finite(o.get("x")?.as_f64()?)?, finite(o.get("y")?.as_f64()?)?)),
        _ => None,
    }
}

pub fn vec3(v: &Value) -> Option<Vec3> {
    match v {
        Value::Array(a) if a.len() == 3 => Some(Vec3::new(finite(a.first()?.as_f64()?)?, finite(a.get(1)?.as_f64()?)?, finite(a.get(2)?.as_f64()?)?)),
        Value::Object(o) => Some(Vec3::new(finite(o.get("x")?.as_f64()?)?, finite(o.get("y")?.as_f64()?)?, finite(o.get("z")?.as_f64()?)?)),
        _ => None,
    }
}

pub fn req_vec2(cmd: &str, p: &Value, k: &str) -> Result<Vec2> {
    p.get(k).and_then(vec2).ok_or_else(|| bad(cmd, format!("`{k}` must be a point [x, y]")))
}

pub fn req_vec3(cmd: &str, p: &Value, k: &str) -> Result<Vec3> {
    p.get(k).and_then(vec3).ok_or_else(|| bad(cmd, format!("`{k}` must be a point [x, y, z]")))
}

pub fn vec2_list(cmd: &str, p: &Value, k: &str, max: usize) -> Result<Vec<Vec2>> {
    let a = p.get(k).and_then(Value::as_array).ok_or_else(|| bad(cmd, format!("`{k}` must be a list of points")))?;
    if a.len() > max {
        return Err(bad(cmd, format!("`{k}` has too many points")));
    }
    a.iter().map(|v| vec2(v).ok_or_else(|| bad(cmd, format!("`{k}` must contain [x, y] points")))).collect()
}

/// A finite number given for an expression, as the expression's text (`8` → `"8"`, which reads
/// as mm or degrees by context).
pub fn number_text(n: &serde_json::Number) -> Option<String> {
    n.as_f64().and_then(finite).map(|v| format!("{v}"))
}

/// An expression parameter: a number (mm or degrees by context) or an expression string.
pub fn expr(p: &Value, k: &str) -> Option<Expr> {
    match p.get(k)? {
        Value::Number(n) => number_text(n).map(Expr::new),
        Value::String(s) if !s.trim().is_empty() && s.len() <= 4096 => Some(Expr::new(s.trim())),
        _ => None,
    }
}

/// Why `v` isn't an expression parameter, for the message: a number out of range or a text too
/// long says so, rather than that it isn't a number or an expression at all.
pub fn expr_problem(v: Option<&Value>) -> &'static str {
    match v {
        Some(Value::Number(n)) if n.as_f64().is_some_and(f64::is_finite) => "must be a number between -1e9 and 1e9",
        Some(Value::String(s)) if s.len() > 4096 => "must be an expression of at most 4096 characters",
        _ => "must be a number or an expression",
    }
}

pub fn req_expr(cmd: &str, p: &Value, k: &str) -> Result<Expr> {
    expr(p, k).ok_or_else(|| bad(cmd, format!("`{k}` {}", expr_problem(p.get(k)))))
}

/// The most names a list parameter may carry.
const MAX_STRING_LIST: usize = 10_000;

/// The command name for a caller that does not know it: the engine names the command that ran
/// when the error reaches it.
pub const UNKNOWN_COMMAND: &str = "";

/// A short description of a value that is not a name, for the message that refuses it.
fn json_brief(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => format!("\"{s}\""),
        // A list or an object is named by its kind alone: it can be long.
        Value::Array(_) => "a list".to_string(),
        Value::Object(_) => "an object".to_string(),
    }
}

/// A list of names given as a parameter: a list of names, or a single name as a string.
///
/// A value of any other kind is refused instead of read as an empty list: an empty list means
/// "every body" to most commands, so a wrong type would act on the whole model (#6651). An
/// entry that is not a name is refused too, and a list longer than [`MAX_STRING_LIST`] names is
/// refused rather than cut short.
///
/// A missing parameter is an empty list (it was not given), and so is `null`: the schema
/// contract declares these parameters optional and reads `null` as absent, as it does for
/// `str_`. A plain string is a list of one (the shorthand the repository allows on purpose).
///
/// The command's name is in the message. A caller that does not know it passes an empty
/// string: the engine names the command that ran when the error reaches it.
pub fn string_list(cmd: &str, p: &Value, k: &str) -> Result<Vec<String>> {
    match p.get(k) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(s)) => Ok(vec![s.clone()]),
        Some(Value::Array(a)) => {
            if a.len() > MAX_STRING_LIST {
                return Err(bad(cmd, format!("`{k}` must list at most {MAX_STRING_LIST} names")));
            }
            a.iter()
                .enumerate()
                .map(|(i, v)| {
                    v.as_str().map(str::to_string).ok_or_else(|| bad(cmd, format!("`{k}` must list names; the entry {} is {}", i + 1, json_brief(v))))
                })
                .collect()
        }
        Some(v) => Err(bad(cmd, format!("`{k}` must be a list of names; it is {}", json_brief(v)))),
    }
}

/// A list of keys, where an entry is a name or an id: a feature can be named or numbered, and
/// both kinds can sit in one list.
///
/// A value of the wrong kind is refused, and so is an entry that is neither a name nor an id,
/// named by its position as in [`string_list`]. The commands that take feature keys read them
/// here, because their list is not a list of names (#6651).
pub fn name_or_id_list(cmd: &str, p: &Value, k: &str) -> Result<Vec<String>> {
    Ok(name_or_id_values(cmd, p, k)?.iter().map(|v| v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string())).collect())
}

/// Keep the JSON types when a command distinguishes a numeric name from a numeric id.
pub(crate) fn name_or_id_values<'a>(cmd: &str, p: &'a Value, k: &str) -> Result<&'a [Value]> {
    match p.get(k) {
        None | Some(Value::Null) => Ok(&[]),
        Some(v @ Value::String(_)) => Ok(std::slice::from_ref(v)),
        Some(Value::Array(a)) => {
            if a.len() > MAX_STRING_LIST {
                return Err(bad(cmd, format!("`{k}` must list at most {MAX_STRING_LIST} keys")));
            }
            if let Some((i, v)) = a.iter().enumerate().find(|(_, v)| !v.is_string() && !v.is_u64()) {
                return Err(bad(cmd, format!("`{k}` must list names or ids; the entry {} is {}", i + 1, json_brief(v))));
            }
            Ok(a)
        }
        Some(v) => Err(bad(cmd, format!("`{k}` must be a list of names or ids; it is {}", json_brief(v)))),
    }
}

/// A list of names read leniently: a value of the wrong kind reads as an empty list, and an
/// entry that is not a name is dropped.
///
/// Only for the parameters where an empty list is harmless: a preview that cannot fail, and a
/// list of ids that names nothing to act on. A command that acts on bodies reads its lists
/// with [`string_list`], so a wrong type is refused there.
pub fn string_list_or_default(p: &Value, k: &str) -> Vec<String> {
    match p.get(k) {
        Some(Value::Array(a)) => a.iter().take(MAX_STRING_LIST).filter_map(|v| v.as_str().map(str::to_string)).collect(),
        Some(Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    }
}

/// A continuity word (`g0` … `g4`, [`kernel::Continuity::parse`]) for `what` (the key or entry
/// it was given as), refused when it is not a word and when `by` does not build that order: the
/// kernel's [`kernel::Construction`] table says which orders it does.
pub fn continuity(cmd: &str, v: Option<&Value>, what: &str, by: kernel::Construction) -> Result<kernel::Continuity> {
    let (lowest, highest) = by.orders();
    let g = v
        .and_then(Value::as_str)
        .and_then(kernel::Continuity::parse)
        .ok_or_else(|| bad(cmd, format!("{what} must be {}", kernel::Continuity::words(lowest, highest))))?;
    g.built_by(by).map_err(|e| match e {
        kernel::KernelError::Invalid(why) => bad(cmd, format!("{what}: {why}")),
        other => other.into(),
    })
}

/// Whether any text in `p` is shaped like a definition, `name = expression` (#4071), so the
/// design may hold one to store once the command has run (`Document::define_inline`). Looks at
/// most a bounded number of values deep into hostile input.
pub fn has_definition(p: &Value) -> bool {
    fn walk(v: &Value, budget: &mut usize) -> bool {
        if *budget == 0 {
            return false;
        }
        *budget -= 1;
        match v {
            Value::String(s) => s.len() <= 4096 && s.contains('=') && solidcraft_doc::expr::definition(s).is_some(),
            Value::Array(a) => a.iter().any(|x| walk(x, budget)),
            Value::Object(o) => o.values().any(|x| walk(x, budget)),
            _ => false,
        }
    }
    walk(p, &mut 100_000)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const CMD: &str = "solid.test";

    fn list(p: &Value) -> Result<Vec<String>> {
        string_list(CMD, p, "bodies")
    }

    fn refused(p: &Value) -> EngineError {
        list(p).expect_err("the parameter is refused")
    }

    fn keys(p: &Value) -> Result<Vec<String>> {
        name_or_id_list(CMD, p, "features")
    }

    #[test]
    fn a_missing_parameter_is_an_empty_list() {
        assert_eq!(list(&json!({})).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn null_is_an_empty_list_as_the_schema_contract_says() {
        // The contract declares these parameters optional: `null` is absent, as for `str_`.
        assert_eq!(list(&json!({"bodies": null})).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn a_list_of_names_is_read_as_it_is() {
        assert_eq!(list(&json!({"bodies": ["Base", "Other"]})).unwrap(), ["Base", "Other"]);
        assert_eq!(list(&json!({"bodies": []})).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn a_single_name_is_read_as_a_list_of_one() {
        assert_eq!(list(&json!({"bodies": "Base"})).unwrap(), ["Base"]);
    }

    #[test]
    fn a_value_of_the_wrong_kind_is_refused() {
        for value in [json!(42), json!(true), json!({"body": "Base"}), json!([["Base"]])] {
            let err = refused(&json!({"bodies": value.clone()}));
            assert!(matches!(&err, EngineError::BadParams { cmd, msg } if cmd == CMD && msg.contains("bodies")), "{value}: {err}");
        }
    }

    #[test]
    fn an_entry_that_is_not_a_name_is_refused() {
        for value in [json!(["Base", 7]), json!(["Base", null]), json!(["Base", {"body": "Base"}]), json!(["Base", ["Base"]])] {
            let err = refused(&json!({"bodies": value.clone()}));
            assert!(matches!(&err, EngineError::BadParams { cmd, msg } if cmd == CMD && msg.contains("bodies")), "{value}: {err}");
        }
    }

    #[test]
    fn the_entry_that_is_not_a_name_is_named_by_its_position() {
        let err = refused(&json!({"bodies": ["Base", "Other", 7]}));
        let EngineError::BadParams { msg, .. } = &err else {
            panic!("{err}");
        };
        assert!(msg.contains("bodies") && msg.contains("3") && msg.contains("7"), "{err}");
    }

    #[test]
    fn a_list_of_the_maximum_length_is_read_and_a_longer_one_is_refused() {
        let names: Vec<String> = (0..MAX_STRING_LIST).map(|i| format!("Body{i}")).collect();
        assert_eq!(list(&json!({"bodies": names.clone()})).unwrap().len(), MAX_STRING_LIST);
        let mut too_many = names;
        too_many.push("OneMore".to_string());
        let err = refused(&json!({"bodies": too_many}));
        assert!(matches!(&err, EngineError::BadParams { cmd, .. } if cmd == CMD), "{err}");
    }

    #[test]
    fn the_lenient_reader_drops_what_it_cannot_read() {
        assert_eq!(string_list_or_default(&json!({"bodies": 7}), "bodies"), Vec::<String>::new());
        assert_eq!(string_list_or_default(&json!({"bodies": ["Base", 7]}), "bodies"), ["Base"]);
        assert_eq!(string_list_or_default(&json!({"bodies": "Base"}), "bodies"), ["Base"]);
    }

    #[test]
    fn a_list_of_names_or_ids_is_read_in_the_order_given() {
        assert_eq!(keys(&json!({"features": ["Base", 3]})).unwrap(), ["Base", "3"]);
        assert_eq!(keys(&json!({"features": [3, "Base"]})).unwrap(), ["3", "Base"]);
        assert_eq!(keys(&json!({"features": "Base"})).unwrap(), ["Base"]);
        assert_eq!(keys(&json!({})).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn an_entry_that_is_neither_a_name_nor_an_id_is_refused() {
        for value in [json!(true), json!({"id": 3}), json!(["Base"]), json!(3.5), json!(-1)] {
            let err = keys(&json!({"features": [value.clone()]})).expect_err("the entry is refused");
            assert!(matches!(&err, EngineError::BadParams { cmd, msg } if cmd == CMD && msg.contains("features")), "{value}: {err}");
        }
    }

    #[test]
    fn a_list_of_keys_of_the_wrong_kind_is_refused() {
        for value in [json!(42), json!(true), json!({"body": "Base"})] {
            let err = keys(&json!({"features": value.clone()})).expect_err("the parameter is refused");
            assert!(matches!(&err, EngineError::BadParams { cmd, msg } if cmd == CMD && msg.contains("features")), "{value}: {err}");
        }
    }
}
