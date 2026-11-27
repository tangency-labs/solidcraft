//! The command template (docs/formats/generator-package.md, section 5): command parameters with
//! `${expression}` holes, compiled once and rendered per build with the output bounded.

use serde_json::Value as Json;

use super::expr::{self, Budget, EvalError, Expr, Scope, Value};
use super::{GeneratorError, MAX_PARAMS_BYTES, MAX_TEXT_CHARS};

#[derive(Clone, Debug)]
pub enum Part {
    Lit(String),
    Hole(Expr),
}

#[derive(Clone, Debug)]
pub enum Tmpl {
    Const(Json),
    /// A string that is exactly one `${expr}`: the value keeps its type.
    Whole(Expr),
    Text(Vec<Part>),
    Array(Vec<Tmpl>),
    Object(Vec<(String, Tmpl)>),
}

fn bad<T>(msg: impl Into<String>) -> Result<T, GeneratorError> {
    Err(GeneratorError::Package(format!("template: {}", msg.into())))
}

/// Parameter names that would hand a command a file or an address.
const FORBIDDEN_KEYS: [&str; 6] = ["file", "filename", "url", "uri", "directory", "folder"];

fn parse_text(s: &str) -> Result<Tmpl, GeneratorError> {
    let chars: Vec<char> = s.chars().collect();
    let mut parts: Vec<Part> = Vec::new();
    let mut lit = String::new();
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        if c == '$' && chars.get(i + 1) == Some(&'$') && chars.get(i + 2) == Some(&'{') {
            lit.push_str("${");
            i += 3;
        } else if c == '$' && chars.get(i + 1) == Some(&'{') {
            let start = i + 2;
            let mut j = start;
            let mut in_text = false;
            while let Some(&d) = chars.get(j) {
                if d == '"' {
                    in_text = !in_text;
                } else if d == '\\' && in_text {
                    j += 1;
                } else if d == '}' && !in_text {
                    break;
                }
                j += 1;
            }
            if chars.get(j) != Some(&'}') {
                return bad(format!("a `${{` is not closed in `{s}`"));
            }
            let src: String = chars.get(start..j).unwrap_or_default().iter().collect();
            let e = expr::parse(&src).map_err(|m| GeneratorError::Expr { expr: src.clone(), message: m })?;
            if !lit.is_empty() {
                parts.push(Part::Lit(std::mem::take(&mut lit)));
            }
            parts.push(Part::Hole(e));
            i = j + 1;
        } else {
            lit.push(c);
            i += 1;
        }
    }
    if !lit.is_empty() {
        parts.push(Part::Lit(lit));
    }
    if parts.is_empty() {
        return Ok(Tmpl::Const(Json::String(String::new())));
    }
    if parts.len() == 1
        && let Some(only) = parts.pop()
    {
        return Ok(match only {
            Part::Lit(text) => Tmpl::Const(Json::String(text)),
            Part::Hole(e) => Tmpl::Whole(e),
        });
    }
    Ok(Tmpl::Text(parts))
}

/// Compile a JSON value with `${...}` holes in its strings.
pub fn compile(raw: &Json) -> Result<Tmpl, GeneratorError> {
    match raw {
        Json::String(s) => parse_text(s),
        Json::Array(items) => Ok(Tmpl::Array(items.iter().map(compile).collect::<Result<Vec<_>, _>>()?)),
        Json::Object(map) => {
            let mut out = Vec::with_capacity(map.len());
            for (k, v) in map {
                if FORBIDDEN_KEYS.contains(&k.to_ascii_lowercase().as_str()) {
                    return bad(format!("a command parameter named `{k}` is not allowed (a generator never names a file or address)"));
                }
                out.push((k.clone(), compile(v)?));
            }
            Ok(Tmpl::Object(out))
        }
        other => Ok(Tmpl::Const(other.clone())),
    }
}

fn run(e: &Expr, scope: &Scope, budget: &mut Budget) -> Result<Value, GeneratorError> {
    expr::eval(e, scope, budget).map_err(|err| match err {
        EvalError::Budget => GeneratorError::Limit { what: "evaluation steps", limit: super::MAX_BUILD_STEPS as usize },
        EvalError::Msg(message) => GeneratorError::Expr { expr: "a `${...}` in a command".into(), message },
    })
}

impl Tmpl {
    /// Fill the holes from `scope`; the result is at most 64 KiB of JSON.
    pub fn render(&self, scope: &Scope, budget: &mut Budget) -> Result<Json, GeneratorError> {
        let out = self.render_inner(scope, budget)?;
        let size = serde_json::to_vec(&out).map_or(usize::MAX, |v| v.len());
        if size > MAX_PARAMS_BYTES {
            return Err(GeneratorError::Limit { what: "bytes of one command's parameters", limit: MAX_PARAMS_BYTES });
        }
        Ok(out)
    }

    fn render_inner(&self, scope: &Scope, budget: &mut Budget) -> Result<Json, GeneratorError> {
        match self {
            Tmpl::Const(j) => Ok(j.clone()),
            Tmpl::Whole(e) => {
                let v = run(e, scope, budget)?;
                v.to_json().ok_or_else(|| GeneratorError::Package("template: a table row cannot be a command parameter".into()))
            }
            Tmpl::Text(parts) => {
                let mut s = String::new();
                for p in parts {
                    match p {
                        Part::Lit(t) => s.push_str(t),
                        Part::Hole(e) => match run(e, scope, budget)? {
                            Value::Record(_) => return bad("a table row cannot be part of text"),
                            v => s.push_str(&v.display()),
                        },
                    }
                    if s.chars().count() > MAX_TEXT_CHARS {
                        return Err(GeneratorError::Limit { what: "characters in one text", limit: MAX_TEXT_CHARS });
                    }
                }
                Ok(Json::String(s))
            }
            Tmpl::Array(items) => Ok(Json::Array(items.iter().map(|t| t.render_inner(scope, budget)).collect::<Result<Vec<_>, _>>()?)),
            Tmpl::Object(fields) => {
                let mut map = serde_json::Map::new();
                for (k, t) in fields {
                    map.insert(k.clone(), t.render_inner(scope, budget)?);
                }
                Ok(Json::Object(map))
            }
        }
    }
}
