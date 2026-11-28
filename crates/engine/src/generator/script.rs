//! A generator's script: plain Rhai in a sandbox (docs/formats/generator.md, section 5). It can
//! compute and call three registered functions (`code_template`, `table`, `refuse`); it cannot
//! reach a file, the network, the clock, another module or `eval`, and it cannot change the
//! design: it only returns data. Every run is bounded, and every way it can fail is a typed error.

use std::collections::BTreeMap;
use std::sync::Arc;

use rhai::{AST, Array, Dynamic, Engine, EvalAltResult, ImmutableString, Map, Position, Scope};
use serde_json::Value as Json;

use super::code_template::{Params, Template, match_any};
use super::csv_table::{Table, key_text};

/// Operations one call may run.
pub const MAX_OPERATIONS: u64 = 100_000;
pub const MAX_CALL_LEVELS: usize = 16;
pub const MAX_EXPR_DEPTH: usize = 32;
pub const MAX_FN_EXPR_DEPTH: usize = 16;
pub const MAX_STRING_SIZE: usize = 4096;
pub const MAX_ARRAY_SIZE: usize = 1024;
pub const MAX_MAP_SIZE: usize = 256;
/// A script file, in bytes.
pub const MAX_SCRIPT_BYTES: usize = 32 * 1024;
/// Nesting of a value passed between SolidCraft and the script.
const MAX_VALUE_DEPTH: usize = 16;

/// Why a code is refused by the script.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefusalKind {
    /// Not a code of this series, or a combination it does not offer.
    Invalid,
    /// A real code this generator does not model.
    NotModelled,
    /// A bound was exceeded (reading the code, or the script's sandbox).
    Limit,
    /// The package or its script could not answer.
    Error,
}

impl RefusalKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RefusalKind::Invalid => "invalid",
            RefusalKind::NotModelled => "not_modelled",
            RefusalKind::Limit => "limit",
            RefusalKind::Error => "error",
        }
    }
}

/// How a script call ended badly.
#[derive(Clone, Debug, PartialEq)]
pub enum ScriptError {
    /// The script called `refuse`: an answer, in the generator's own words.
    Refused { kind: RefusalKind, message: String },
    /// A sandbox limit stopped it (operations, call depth, size, nesting).
    Limit(String),
    /// It did not compile, or failed while running.
    Script(String),
}

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScriptError::Refused { message, .. } => f.write_str(message),
            ScriptError::Limit(m) => write!(f, "generator script limit: {m}"),
            ScriptError::Script(m) => write!(f, "generator script: {m}"),
        }
    }
}

impl std::error::Error for ScriptError {}

/// A compiled script and the data it can see.
pub struct Script {
    engine: Engine,
    ast: AST,
    contextual: Vec<Engine>,
}

fn to_dynamic(v: &Json, depth: usize) -> Dynamic {
    if depth > MAX_VALUE_DEPTH {
        return Dynamic::UNIT;
    }
    match v {
        Json::Null => Dynamic::UNIT,
        Json::Bool(b) => Dynamic::from(*b),
        Json::Number(n) => match n.as_i64() {
            Some(i) => Dynamic::from(i),
            None => Dynamic::from(n.as_f64().unwrap_or(0.0)),
        },
        Json::String(s) => Dynamic::from(s.clone()),
        Json::Array(a) => Dynamic::from_array(a.iter().map(|x| to_dynamic(x, depth + 1)).collect::<Array>()),
        Json::Object(o) => {
            let mut m = Map::new();
            for (k, x) in o {
                m.insert(k.as_str().into(), to_dynamic(x, depth + 1));
            }
            Dynamic::from_map(m)
        }
    }
}

fn from_dynamic(d: &Dynamic, depth: usize) -> Result<Json, String> {
    if depth > MAX_VALUE_DEPTH {
        return Err("a value nests too deeply".into());
    }
    if d.is_unit() {
        return Ok(Json::Null);
    }
    if let Ok(b) = d.as_bool() {
        return Ok(Json::Bool(b));
    }
    if let Ok(i) = d.as_int() {
        return Ok(Json::from(i));
    }
    if let Ok(x) = d.as_float() {
        return serde_json::Number::from_f64(x).map(Json::Number).ok_or_else(|| "a number is not finite".to_string());
    }
    if d.is_string() {
        return d.clone().into_string().map(Json::String).map_err(|t| format!("expected text, found {t}"));
    }
    if let Some(a) = d.clone().try_cast::<Array>() {
        return a.iter().map(|x| from_dynamic(x, depth + 1)).collect::<Result<Vec<_>, _>>().map(Json::Array);
    }
    if let Some(m) = d.clone().try_cast::<Map>() {
        let mut out = serde_json::Map::new();
        for (k, x) in &m {
            out.insert(k.to_string(), from_dynamic(x, depth + 1)?);
        }
        return Ok(Json::Object(out));
    }
    Err(format!("a value of type {} cannot be used here", d.type_name()))
}

fn refusal(kind: &str, message: &str) -> Box<EvalAltResult> {
    let mut m = Map::new();
    m.insert("refusal".into(), Dynamic::from(kind.to_string()));
    m.insert("message".into(), Dynamic::from(message.to_string()));
    Box::new(EvalAltResult::ErrorRuntime(Dynamic::from_map(m), Position::NONE))
}

fn failure(message: impl Into<String>) -> Box<EvalAltResult> {
    Box::new(EvalAltResult::ErrorRuntime(Dynamic::from(message.into()), Position::NONE))
}

fn map_error(e: EvalAltResult) -> ScriptError {
    match e {
        EvalAltResult::ErrorInFunctionCall(_, _, inner, _) => map_error(*inner),
        EvalAltResult::ErrorTooManyOperations(..) => ScriptError::Limit(format!("it ran more than {MAX_OPERATIONS} operations")),
        EvalAltResult::ErrorStackOverflow(..) => ScriptError::Limit(format!("it called functions more than {MAX_CALL_LEVELS} deep")),
        EvalAltResult::ErrorDataTooLarge(what, _) => ScriptError::Limit(format!("{what} grew too large")),
        EvalAltResult::ErrorTerminated(..) => ScriptError::Limit("it was stopped".into()),
        EvalAltResult::ErrorRuntime(d, _) => {
            if let Some(m) = d.clone().try_cast::<Map>()
                && let Some(kind) = m.get("refusal").and_then(|k| k.clone().into_string().ok())
            {
                let message = m.get("message").and_then(|k| k.clone().into_string().ok()).unwrap_or_default();
                let kind = if kind == "not_modelled" { RefusalKind::NotModelled } else { RefusalKind::Invalid };
                return ScriptError::Refused { kind, message };
            }
            ScriptError::Script(d.clone().into_string().unwrap_or_else(|_| d.to_string()))
        }
        other => ScriptError::Script(other.to_string()),
    }
}

fn params_of(map: &Map) -> Result<Params, Box<EvalAltResult>> {
    let mut out = Params::new();
    for (k, v) in map {
        out.insert(k.to_string(), from_dynamic(v, 0).map_err(failure)?);
    }
    Ok(out)
}

impl Script {
    /// Compile `source`. `templates` and `tables` are what `code_template` and `table` see.
    pub fn compile(source: &str, templates: Arc<Vec<Template>>, tables: Arc<BTreeMap<String, Table>>) -> Result<Script, ScriptError> {
        if source.len() > MAX_SCRIPT_BYTES {
            return Err(ScriptError::Limit(format!("the script is larger than {} KiB", MAX_SCRIPT_BYTES / 1024)));
        }
        let engine = Self::engine(templates.clone(), tables.clone());
        let contextual = templates.iter().map(|t| Self::engine(Arc::new(vec![t.clone()]), tables.clone())).collect();
        let ast = engine.compile(source).map_err(|e| ScriptError::Script(e.to_string()))?;
        Ok(Script { engine, ast: ast.clone_functions_only(), contextual })
    }

    fn engine(templates: Arc<Vec<Template>>, tables: Arc<BTreeMap<String, Table>>) -> Engine {
        let mut engine = Engine::new();
        engine.set_max_operations(MAX_OPERATIONS);
        engine.set_max_call_levels(MAX_CALL_LEVELS);
        engine.set_max_expr_depths(MAX_EXPR_DEPTH, MAX_FN_EXPR_DEPTH);
        engine.set_max_string_size(MAX_STRING_SIZE);
        engine.set_max_array_size(MAX_ARRAY_SIZE);
        engine.set_max_map_size(MAX_MAP_SIZE);
        engine.set_max_variables(256);
        engine.disable_symbol("eval");
        engine.on_print(|_| {});
        engine.on_debug(|_, _, _| {});

        engine.register_fn("refuse", |kind: ImmutableString, message: ImmutableString| -> Result<Dynamic, Box<EvalAltResult>> {
            match kind.as_str() {
                "invalid" | "not_modelled" => Err(refusal(kind.as_str(), message.as_str())),
                other => Err(failure(format!("refuse() takes \"invalid\" or \"not_modelled\", not \"{other}\""))),
            }
        });

        let t = tables.clone();
        engine.register_fn("table", move |name: ImmutableString, key: Dynamic| -> Result<Dynamic, Box<EvalAltResult>> {
            let Some(table) = t.get(name.as_str()) else { return Err(failure(format!("there is no table `{name}`"))) };
            let Some(key) = from_dynamic(&key, 0).ok().as_ref().and_then(key_text) else { return Ok(Dynamic::UNIT) };
            let Some(row) = table.row(&key) else { return Ok(Dynamic::UNIT) };
            let mut m = Map::new();
            for (col, v) in row {
                m.insert(col.into(), to_dynamic(v, 0));
            }
            Ok(Dynamic::from_map(m))
        });
        let t = tables;
        engine.register_fn("table", move |name: ImmutableString, key: Dynamic, column: ImmutableString| -> Result<Dynamic, Box<EvalAltResult>> {
            let Some(table) = t.get(name.as_str()) else { return Err(failure(format!("there is no table `{name}`"))) };
            if !table.columns.iter().any(|c| c == column.as_str()) {
                return Err(failure(format!("the table `{name}` has no column `{column}`")));
            }
            let Some(key) = from_dynamic(&key, 0).ok().as_ref().and_then(key_text) else { return Ok(Dynamic::UNIT) };
            let Some(row) = table.row(&key) else { return Ok(Dynamic::UNIT) };
            Ok(row.into_iter().find(|(c, _)| *c == column.as_str()).map_or(Dynamic::UNIT, |(_, v)| to_dynamic(v, 0)))
        });

        engine.register_fn("code_template", move |values: Map| -> Result<String, Box<EvalAltResult>> {
            let params = params_of(&values)?;
            let mut last = None;
            for t in templates.iter() {
                match t.print(&params) {
                    Ok(code) => return Ok(code),
                    Err(e) => last = Some(e.to_string()),
                }
            }
            Err(failure(last.unwrap_or_else(|| "the generator has no template".into())))
        });

        engine
    }

    /// Whether the script defines `name` with `arity` parameters.
    pub fn defines(&self, name: &str, arity: usize) -> bool {
        self.ast.iter_functions().any(|f| f.name == name && f.params.len() == arity)
    }

    fn call(&self, name: &str, arg: Json) -> Result<Json, ScriptError> {
        self.call_in(&self.engine, name, arg)
    }

    fn call_in(&self, engine: &Engine, name: &str, arg: Json) -> Result<Json, ScriptError> {
        let mut scope = Scope::new();
        let out = engine.call_fn::<Dynamic>(&mut scope, &self.ast, name, (to_dynamic(&arg, 0),)).map_err(|e| map_error(*e))?;
        from_dynamic(&out, 0).map_err(ScriptError::Script)
    }

    /// `resolve(fields)`: `{configuration, parameters}` for a code's fields, or a refusal.
    /// `None` when the script defines no `resolve`.
    pub fn resolve(&self, fields: &Params) -> Result<Option<Json>, ScriptError> {
        if !self.defines("resolve", 1) {
            return Ok(None);
        }
        self.call("resolve", Json::Object(fields.iter().map(|(k, v)| (k.clone(), v.clone())).collect())).map(Some)
    }

    /// Resolve using the matched template for the script's `code_template` helper.
    pub fn resolve_in(&self, fields: &Params, template: usize) -> Result<Option<Json>, ScriptError> {
        if !self.defines("resolve", 1) {
            return Ok(None);
        }
        let engine = self.contextual.get(template).ok_or_else(|| ScriptError::Script("no such template".into()))?;
        self.call_in(engine, "resolve", Json::Object(fields.iter().map(|(k, v)| (k.clone(), v.clone())).collect())).map(Some)
    }

    /// Describe using the matched template for the script's `code_template` helper.
    pub fn describe_in(&self, fields: &Params, template: usize) -> Result<Option<String>, ScriptError> {
        if !self.defines("describe", 1) {
            return Ok(None);
        }
        let engine = self.contextual.get(template).ok_or_else(|| ScriptError::Script("no such template".into()))?;
        match self.call_in(engine, "describe", Json::Object(fields.iter().map(|(k, v)| (k.clone(), v.clone())).collect()))? {
            Json::String(s) => Ok(Some(s)),
            Json::Null => Ok(None),
            other => Err(ScriptError::Script(format!("describe() must return text, not {other}"))),
        }
    }

    /// `describe(parameters)`: a line of text beside the code. `None` when not defined.
    pub fn describe(&self, params: &Params) -> Result<Option<String>, ScriptError> {
        if !self.defines("describe", 1) {
            return Ok(None);
        }
        match self.call("describe", Json::Object(params.iter().map(|(k, v)| (k.clone(), v.clone())).collect()))? {
            Json::String(s) => Ok(Some(s)),
            Json::Null => Ok(None),
            other => Err(ScriptError::Script(format!("describe() must return text, not {other}"))),
        }
    }
}

/// Read a code against `templates` for a script that wants to (shared with the commands).
pub fn read_code(templates: &[Template], code: &str) -> Result<Option<(usize, Params)>, super::code_template::TemplateError> {
    match_any(templates, code)
}
