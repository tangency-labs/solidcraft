//! Helpers to read hostile JSON parameters.

use serde_json::Value;
use solidcraft_doc::Expr;
use solidcraft_geom::{Vec2, Vec3};

use crate::{EngineError, Result};

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

pub fn string_list(p: &Value, k: &str) -> Vec<String> {
    match p.get(k) {
        Some(Value::Array(a)) => a.iter().take(10_000).filter_map(|v| v.as_str().map(str::to_string)).collect(),
        Some(Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    }
}
