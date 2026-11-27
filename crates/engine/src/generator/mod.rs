//! Generators: a manufacturer's ordering code to a part, from a data-only package
//! (docs/formats/generator-package.md, RFC 0017, #22331).
//!
//! A package is JSON with a grammar, tables, rules and a command template. Nothing in it is
//! code: the pieces here parse the code, look rows up, evaluate tiny pure expressions and
//! produce a list of ordinary commands for the caller to run. Everything is bounded and every
//! failure is a [`GeneratorError`]; nothing panics, reads a file or reaches the network.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value as Json;

pub mod expr;
mod grammar;
pub mod installed;
mod table;
mod template;

use expr::{Budget, EvalError, Expr, Scope, Value};
use grammar::Tok;
use table::{Table, TableDef};
use template::Tmpl;

/// The package file.
pub const MAX_PACKAGE_BYTES: usize = 1 << 20;
/// An ordering code, in characters.
pub const MAX_CODE_CHARS: usize = 128;
/// Any one text value, in characters.
pub const MAX_TEXT_CHARS: usize = 4096;
pub const MAX_ROWS: usize = 5000;
pub const MAX_COLUMNS: usize = 64;
/// Points in one table cell.
pub const MAX_POINTS: usize = 512;
/// Items in one list cell.
pub const MAX_LIST_ITEMS: usize = 512;
pub const MAX_EXPR_CHARS: usize = 512;
pub const MAX_EXPR_NODES: usize = 256;
pub const MAX_EXPR_NESTING: usize = 32;
/// Evaluation steps in one build.
pub const MAX_BUILD_STEPS: u64 = 1_000_000;
/// Steps in a package.
pub const MAX_STEPS: usize = 256;
/// The most a repeat's `max` may be.
pub const MAX_REPEAT: usize = 1024;
/// Commands one build expands to, all repeats together.
pub const MAX_EXPANDED: usize = 512;
/// One command's rendered parameters, in bytes.
pub const MAX_PARAMS_BYTES: usize = 64 * 1024;
/// Matching steps for one ordering code.
pub const GRAMMAR_BUDGET: u32 = 20_000;

/// Why a package did not load, or a code did not build.
#[derive(Clone, Debug, PartialEq)]
pub enum GeneratorError {
    /// The package is malformed.
    Package(String),
    /// The package needs a newer SolidCraft.
    NeedsNewer { needs: String, have: String },
    /// A bound was exceeded.
    Limit { what: &'static str, limit: usize },
    /// An expression failed.
    Expr { expr: String, message: String },
    /// The code is not accepted; `message` is the package's own words.
    Refused { kind: RefusalKind, message: String },
}

/// Why a code is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefusalKind {
    /// Not an ordering code of this series, or a combination it does not offer.
    Invalid,
    /// A real code this generator does not model.
    NotModelled,
}

impl RefusalKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RefusalKind::Invalid => "invalid",
            RefusalKind::NotModelled => "not_modelled",
        }
    }
}

impl std::fmt::Display for GeneratorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GeneratorError::Package(m) => write!(f, "generator package: {m}"),
            GeneratorError::NeedsNewer { needs, have } => write!(f, "this generator needs SolidCraft {needs} or newer (this is {have})"),
            GeneratorError::Limit { what, limit } => write!(f, "generator limit: more than {limit} {what}"),
            GeneratorError::Expr { expr, message } => write!(f, "expression `{expr}`: {message}"),
            GeneratorError::Refused { message, .. } => f.write_str(message),
        }
    }
}

impl std::error::Error for GeneratorError {}

/// A plain name: letters, digits and underscores, not starting with a digit.
pub(crate) fn is_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && s.len() <= 64 && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

// ---- the package as written ----

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub pages: String,
}

#[derive(Clone, Debug, Deserialize)]
struct Package {
    format: String,
    #[serde(default)]
    min_solidcraft: Option<String>,
    id: String,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    license: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    guarantee: Option<String>,
    #[serde(default)]
    provenance: String,
    #[serde(default)]
    sources: Vec<Source>,
    #[serde(default)]
    declares: Option<Json>,
    grammar: GrammarDef,
    #[serde(default)]
    tables: BTreeMap<String, TableDef>,
    #[serde(default)]
    lookups: Vec<LookupDef>,
    #[serde(default)]
    defaults: BTreeMap<String, String>,
    #[serde(default)]
    rules: Vec<RuleDef>,
    #[serde(default)]
    vars: Vec<VarDef>,
    steps: Vec<StepDef>,
    #[serde(default)]
    insert: InsertDef,
    #[serde(default)]
    checks: Vec<Check>,
    #[serde(default, rename = "const")]
    consts: BTreeMap<String, Json>,
    #[serde(default)]
    review: Option<ReviewDef>,
    #[serde(flatten)]
    extra: BTreeMap<String, Json>,
}

/// Who checked the package and against what (the `review` object).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewDef {
    #[serde(default)]
    reviewer: String,
    #[serde(default)]
    reviewer_kind: Option<String>,
    #[serde(default)]
    date: String,
    #[serde(default)]
    sources_hash: String,
    #[serde(default)]
    level: Option<String>,
    #[serde(default)]
    notes: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GrammarDef {
    alternatives: Vec<Vec<Json>>,
    #[serde(default)]
    refuse: Option<String>,
    /// `upper` or `lower`: the code is folded before it is matched.
    #[serde(default)]
    normalize: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LookupDef {
    table: String,
    r#as: String,
    key: BTreeMap<String, String>,
    #[serde(default)]
    missing: Option<String>,
    /// `invalid` (the default) or `not_modelled`.
    #[serde(default)]
    missing_kind: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleDef {
    when: String,
    refuse: String,
    #[serde(default)]
    kind: Option<String>,
    /// `early` (before the lookups) or `late` (after the variables, the default).
    #[serde(default)]
    phase: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VarDef {
    name: String,
    expr: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RepeatDef {
    var: String,
    count: String,
    max: usize,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StepDef {
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    params: Option<Json>,
    #[serde(default)]
    when: Option<String>,
    #[serde(default)]
    repeat: Option<RepeatDef>,
    /// A block: the commands one `repeat` or `when` covers, in order.
    #[serde(default)]
    steps: Option<Vec<LeafDef>>,
}

/// One command inside a block.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LeafDef {
    command: String,
    #[serde(default)]
    params: Option<Json>,
    #[serde(default)]
    when: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct InsertDef {
    /// A name template for the new component (default `${code}`), or `false` to build in the
    /// active component without making one.
    #[serde(default)]
    component: Option<Json>,
}

/// An expected result the package carries for one code (section 7 of the format).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Check {
    pub code: String,
    #[serde(default)]
    pub invalid: bool,
    #[serde(default)]
    pub message_contains: Option<String>,
    #[serde(default)]
    pub volume_mm3: Option<f64>,
    #[serde(default)]
    pub rel_tol: Option<f64>,
    #[serde(default)]
    pub abs_tol: Option<f64>,
    #[serde(default)]
    pub bbox_mm: Option<[f64; 3]>,
    #[serde(default)]
    pub faces: Option<u64>,
    #[serde(default)]
    pub dimensions: BTreeMap<String, f64>,
}

// ---- the loaded generator ----

/// What a package says about itself, for lists and the dialog.
#[derive(Clone, Debug, PartialEq)]
pub struct Meta {
    pub id: String,
    pub name: String,
    pub description: String,
    pub license: String,
    pub author: String,
    /// The package's own claim: `checked`, `reviewed` or `drawing-matched`. Only a label until
    /// the Workshop index verifies it.
    pub guarantee: Option<String>,
    pub provenance: String,
    pub sources: Vec<(String, String, String, String)>,
    /// Who checked the package and against what, when it says.
    pub review: Option<Review>,
}

/// The package's `review` object: evidence for its `guarantee`, shown with it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Review {
    pub reviewer: String,
    /// `person`, `ai` or `automatic`.
    pub reviewer_kind: Option<String>,
    pub date: String,
    pub sources_hash: String,
    pub level: Option<String>,
    pub notes: String,
}

#[derive(Debug)]
struct Lookup {
    table: String,
    name: String,
    key: Vec<(String, Expr)>,
    missing: Option<String>,
    missing_kind: RefusalKind,
}

#[derive(Debug)]
struct Rule {
    when: Expr,
    refuse: String,
    kind: RefusalKind,
    early: bool,
}

#[derive(Debug)]
struct Repeat {
    var: String,
    count: Expr,
    max: usize,
}

/// One command of a step, with its own condition.
#[derive(Debug)]
struct Leaf {
    command: String,
    params: Tmpl,
    when: Option<Expr>,
}

/// A step: one command or a block of them, run when its condition holds, once or repeated.
#[derive(Debug)]
struct Step {
    when: Option<Expr>,
    repeat: Option<Repeat>,
    body: Vec<Leaf>,
}

/// A package, loaded and checked, ready to turn codes into command lists.
#[derive(Debug)]
pub struct Generator {
    pub meta: Meta,
    pub checks: Vec<Check>,
    /// Every `declares.matches` example and negative, for the package check.
    pub declared: Vec<(String, bool)>,
    alternatives: Vec<Vec<Tok>>,
    refuse: Option<String>,
    tables: BTreeMap<String, Table>,
    lookups: Vec<Lookup>,
    defaults: Vec<(String, Expr)>,
    rules: Vec<Rule>,
    vars: Vec<(String, Expr)>,
    steps: Vec<Step>,
    /// The component's name template; none: build in the active component.
    component: Option<Tmpl>,
    consts: Scope,
    normalize: Option<bool>,
}

/// The row a lookup found, with its provenance.
#[derive(Clone, Debug, PartialEq)]
pub struct RowUsed {
    pub lookup: String,
    pub table: String,
    pub key: Vec<String>,
    pub source: Option<String>,
    pub page: Option<String>,
    /// The status of the row's columns that do not have one of their own.
    pub status: Option<String>,
    /// The columns that have a status of their own.
    pub column_status: BTreeMap<String, String>,
    pub note: Option<String>,
}

/// A code parsed against a package.
#[derive(Clone, Debug)]
pub struct Parsed {
    pub code: String,
    /// The grammar's fields and defaults, then the variables, in order.
    pub fields: Vec<(String, Value)>,
    pub vars: Vec<(String, Value)>,
    pub rows: Vec<RowUsed>,
    scope: Scope,
}

/// One command to run.
#[derive(Clone, Debug, PartialEq)]
pub struct PlannedCommand {
    /// The package step it came from, counting from 0.
    pub step: usize,
    pub command: String,
    pub params: Json,
}

/// A code, ready to run.
#[derive(Clone, Debug)]
pub struct Plan {
    pub parsed: Parsed,
    /// The component a build makes; none when the package builds in the active component.
    pub component: Option<String>,
    pub commands: Vec<PlannedCommand>,
}

fn pkg<T>(msg: impl Into<String>) -> Result<T, GeneratorError> {
    Err(GeneratorError::Package(msg.into()))
}

fn expr_err(src: &str, e: EvalError) -> GeneratorError {
    match e {
        EvalError::Budget => GeneratorError::Limit { what: "evaluation steps", limit: MAX_BUILD_STEPS as usize },
        EvalError::Msg(message) => GeneratorError::Expr { expr: src.chars().take(80).collect(), message },
    }
}

fn compile_expr(src: &str) -> Result<Expr, GeneratorError> {
    expr::parse(src).map_err(|message| GeneratorError::Expr { expr: src.chars().take(80).collect(), message })
}

/// The command ids a generator may run: modelling commands only.
fn command_allowed(id: &str) -> bool {
    const PREFIXES: [&str; 7] = ["sketch.", "solid.", "component.", "construct.", "surface.", "parameters.", "sheet."];
    const WORDS: [&str; 9] = ["import", "link", "open", "export", "save", "load", "insert_", "paste", "from_file"];
    const EXACT: [&str; 2] = ["browser.visibility", "document.metadata.set"];
    EXACT.contains(&id) || (PREFIXES.iter().any(|p| id.starts_with(p)) && !WORDS.iter().any(|w| id.contains(w)))
}

fn refusal_kind(kind: Option<&str>, what: &str) -> Result<RefusalKind, GeneratorError> {
    match kind {
        None | Some("invalid") => Ok(RefusalKind::Invalid),
        Some("not_modelled") => Ok(RefusalKind::NotModelled),
        Some(other) => pkg(format!("{what} is invalid or not_modelled, not `{other}`")),
    }
}

/// `x-` keys are the package's own: allowed on every structural object (not inside command
/// parameters or `declares`, whose keys are the package's data), kept by nobody and ignored.
fn strip_extension_keys(raw: &mut Json) {
    fn strip(o: &mut serde_json::Map<String, Json>) {
        o.retain(|k, _| !k.starts_with("x-"));
    }
    fn each(v: Option<&mut Json>, f: &mut dyn FnMut(&mut serde_json::Map<String, Json>)) {
        match v {
            Some(Json::Array(items)) => items.iter_mut().for_each(|i| {
                if let Json::Object(o) = i {
                    f(o);
                }
            }),
            Some(Json::Object(o)) => f(o),
            _ => {}
        }
    }
    let Some(top) = raw.as_object_mut() else { return };
    for key in ["grammar", "insert", "review"] {
        each(top.get_mut(key), &mut |o| strip(o));
    }
    for key in ["sources", "lookups", "rules", "vars", "checks"] {
        each(top.get_mut(key), &mut |o| strip(o));
    }
    if let Some(Json::Object(tables)) = top.get_mut("tables") {
        for t in tables.values_mut() {
            each(Some(t), &mut |o| strip(o));
        }
    }
    if let Some(Json::Array(steps)) = top.get_mut("steps") {
        for st in steps {
            if let Json::Object(o) = st {
                strip(o);
                each(o.get_mut("steps"), &mut |l| strip(l));
                each(o.get_mut("repeat"), &mut |r| strip(r));
            }
        }
    }
}

fn version_tuple(v: &str) -> Option<(u64, u64, u64)> {
    let mut it = v.trim().split('.');
    let a = it.next()?.parse().ok()?;
    let b = it.next()?.parse().ok()?;
    let c = it.next()?.split(['-', '+']).next()?.parse().ok()?;
    if it.next().is_some() { None } else { Some((a, b, c)) }
}

impl Generator {
    /// Load a package from its JSON text. `current` is this build's version, `"0.52.0"`.
    pub fn load(text: &str, current: &str) -> Result<Generator, GeneratorError> {
        if text.len() > MAX_PACKAGE_BYTES {
            return Err(GeneratorError::Limit { what: "bytes in a package", limit: MAX_PACKAGE_BYTES });
        }
        let mut raw: Json = serde_json::from_str(text).map_err(|e| GeneratorError::Package(format!("not a generator package: {e}")))?;
        strip_extension_keys(&mut raw);
        let p: Package = serde_json::from_value(raw).map_err(|e| GeneratorError::Package(format!("not a generator package: {e}")))?;
        if let Some(k) = p.extra.keys().find(|k| !k.starts_with("x-")) {
            return pkg(format!("unknown key `{k}`"));
        }
        match p.format.strip_prefix("solidcraft-generator/") {
            Some("1") => {}
            Some(other) => {
                return Err(GeneratorError::NeedsNewer { needs: format!("a version that reads generator format {other}"), have: current.into() });
            }
            None => return pkg(format!("`format` must be solidcraft-generator/1, not `{}`", p.format.chars().take(40).collect::<String>())),
        }
        if let Some(min) = &p.min_solidcraft {
            let (Some(need), Some(have)) = (version_tuple(min), version_tuple(current)) else {
                return pkg("`min_solidcraft` must look like 0.52.0");
            };
            if need > have {
                return Err(GeneratorError::NeedsNewer { needs: min.clone(), have: current.into() });
            }
        }
        if !(3..=64).contains(&p.id.len()) || !p.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
            return pkg("`id` must be 3 to 64 lower-case letters, digits and dashes");
        }
        for (what, text, max) in [
            ("name", &p.name, 200),
            ("description", &p.description, 2000),
            ("provenance", &p.provenance, 2000),
            ("license", &p.license, 100),
            ("author", &p.author, 200),
        ] {
            if text.chars().count() > max {
                return pkg(format!("`{what}` is longer than {max} characters"));
            }
        }
        if let Some(g) = &p.guarantee
            && !["checked", "reviewed", "drawing-matched"].contains(&g.as_str())
        {
            return pkg("`guarantee` must be checked, reviewed or drawing-matched");
        }
        if p.sources.len() > 64 {
            return Err(GeneratorError::Limit { what: "sources", limit: 64 });
        }
        if p.steps.is_empty() {
            return pkg("`steps` must have at least one command");
        }
        if p.steps.len() > MAX_STEPS {
            return Err(GeneratorError::Limit { what: "steps", limit: MAX_STEPS });
        }

        // The grammar.
        if p.grammar.alternatives.is_empty() || p.grammar.alternatives.len() > 16 {
            return pkg("`grammar.alternatives` must have 1 to 16 alternatives");
        }
        let mut alternatives = Vec::new();
        for alt in &p.grammar.alternatives {
            if alt.is_empty() || alt.len() > 32 {
                return pkg("an alternative must have 1 to 32 tokens");
            }
            alternatives.push(alt.iter().map(|t| grammar::compile_token(t, 0)).collect::<Result<Vec<_>, _>>()?);
        }

        // Tables and lookups.
        if p.tables.len() > 32 {
            return Err(GeneratorError::Limit { what: "tables", limit: 32 });
        }
        let mut tables = BTreeMap::new();
        for (name, def) in &p.tables {
            if !is_name(name) {
                return pkg(format!("`{name}` is not a plain table name"));
            }
            tables.insert(name.clone(), Table::load(name, def)?);
        }
        let mut lookups = Vec::new();
        let mut names: Vec<&str> = Vec::new();
        for l in &p.lookups {
            let Some(t) = tables.get(&l.table) else { return pkg(format!("lookup `{}` names the unknown table `{}`", l.r#as, l.table)) };
            if !is_name(&l.r#as) || names.contains(&l.r#as.as_str()) {
                return pkg(format!("`{}` is not a unique plain name for a lookup", l.r#as));
            }
            names.push(&l.r#as);
            let mut key = Vec::new();
            for col in t.key_columns() {
                let Some(src) = l.key.get(col) else { return pkg(format!("lookup `{}` gives no value for the key column `{col}`", l.r#as)) };
                key.push((col.clone(), compile_expr(src)?));
            }
            if l.key.len() != key.len() {
                return pkg(format!("lookup `{}` names a column that is not in the table's key", l.r#as));
            }
            let missing_kind = refusal_kind(l.missing_kind.as_deref(), "a lookup's `missing_kind`")?;
            lookups.push(Lookup { table: l.table.clone(), name: l.r#as.clone(), key, missing: l.missing.clone(), missing_kind });
        }
        if lookups.len() > 32 {
            return Err(GeneratorError::Limit { what: "lookups", limit: 32 });
        }

        // Defaults, variables and rules.
        let mut defaults = Vec::new();
        for (k, v) in &p.defaults {
            if !is_name(k) {
                return pkg(format!("`{k}` is not a plain field name"));
            }
            defaults.push((k.clone(), compile_expr(v)?));
        }
        let mut vars = Vec::new();
        for v in &p.vars {
            if !is_name(&v.name) || vars.iter().any(|(n, _): &(String, Expr)| *n == v.name) {
                return pkg(format!("`{}` is not a unique plain name for a variable", v.name));
            }
            vars.push((v.name.clone(), compile_expr(&v.expr)?));
        }
        if vars.len() > 128 || p.rules.len() > 128 {
            return Err(GeneratorError::Limit { what: "variables or rules", limit: 128 });
        }
        let mut rules = Vec::new();
        for r in &p.rules {
            let kind = refusal_kind(r.kind.as_deref(), "a rule's `kind`")?;
            let early = match r.phase.as_deref() {
                None | Some("late") => false,
                Some("early") => true,
                Some(other) => return pkg(format!("a rule's `phase` is early or late, not `{other}`")),
            };
            rules.push(Rule { when: compile_expr(&r.when)?, refuse: r.refuse.clone(), kind, early });
        }

        // The command template.
        let mut steps = Vec::new();
        let mut leaves = 0usize;
        for (i, st) in p.steps.iter().enumerate() {
            let n = i + 1;
            let repeat = match &st.repeat {
                None => None,
                Some(r) => {
                    if !is_name(&r.var) || r.max > MAX_REPEAT {
                        return pkg(format!("step {n}: a repeat needs a plain `var` and a `max` of at most {MAX_REPEAT}"));
                    }
                    Some(Repeat { var: r.var.clone(), count: compile_expr(&r.count)?, max: r.max })
                }
            };
            let when = st.when.as_deref().map(compile_expr).transpose()?;
            let leaf = |command: &str, params: Option<&Json>, when: Option<&str>| -> Result<Leaf, GeneratorError> {
                if !command_allowed(command) {
                    return pkg(format!("step {n} runs `{command}`, which a generator may not run"));
                }
                let params = template::compile(params.unwrap_or(&Json::Object(Default::default())))?;
                Ok(Leaf { command: command.to_string(), params, when: when.map(compile_expr).transpose()? })
            };
            let body = match (&st.command, &st.steps) {
                (Some(command), None) => vec![leaf(command, st.params.as_ref(), None)?],
                (None, Some(block)) => {
                    if st.params.is_some() {
                        return pkg(format!("step {n}: a block has no `params`; each command in it has its own"));
                    }
                    if block.is_empty() || block.len() > 64 {
                        return pkg(format!("step {n}: a block holds 1 to 64 commands"));
                    }
                    block.iter().map(|l| leaf(&l.command, l.params.as_ref(), l.when.as_deref())).collect::<Result<Vec<_>, _>>()?
                }
                _ => return pkg(format!("step {n} needs either a `command` or a block of `steps`, not both or neither")),
            };
            leaves += body.len();
            steps.push(Step { when, repeat, body });
        }
        if leaves > MAX_STEPS {
            return Err(GeneratorError::Limit { what: "commands in the template", limit: MAX_STEPS });
        }
        let component = match &p.insert.component {
            None => Some(template::compile(&Json::String("${code}".into()))?),
            Some(Json::Bool(false)) => None,
            Some(Json::String(t)) => Some(template::compile(&Json::String(t.clone()))?),
            Some(_) => return pkg("`insert.component` must be a name template or false"),
        };

        // Constants, the code's case and the review.
        if p.consts.len() > 64 {
            return Err(GeneratorError::Limit { what: "constants", limit: 64 });
        }
        let mut consts = Scope::new();
        for (k, v) in &p.consts {
            let value = match v {
                Json::Number(n) => n.as_f64().filter(|x| x.is_finite()).map(Value::Num),
                Json::String(t) if t.chars().count() <= MAX_TEXT_CHARS => Some(Value::Text(t.clone())),
                Json::Bool(b) => Some(Value::Bool(*b)),
                _ => None,
            };
            match value {
                Some(value) if is_name(k) && k != "code" => {
                    consts.insert(k.clone(), value);
                }
                _ => return pkg(format!("`const` `{k}` must be a plain name (not `code`) with a finite number, text or true/false")),
            }
        }
        let mut taken: Vec<String> = Vec::new();
        for alt in &alternatives {
            grammar::field_names(alt, &mut taken);
        }
        taken.extend(lookups.iter().map(|l: &Lookup| l.name.clone()));
        taken.extend(defaults.iter().map(|(n, _): &(String, Expr)| n.clone()));
        taken.extend(vars.iter().map(|(n, _): &(String, Expr)| n.clone()));
        if let Some(clash) = taken.iter().find(|n| consts.contains_key(*n) || n.as_str() == "code") {
            return pkg(format!(
                "`{clash}` is used twice: as a constant, a field, a default, a lookup or a variable (and `code` is the code itself)"
            ));
        }
        let normalize = match p.grammar.normalize.as_deref() {
            None => None,
            Some("upper") => Some(true),
            Some("lower") => Some(false),
            Some(other) => return pkg(format!("`grammar.normalize` is upper or lower, not `{other}`")),
        };
        let review = match &p.review {
            None => None,
            Some(r) => {
                if let Some(kind) = &r.reviewer_kind
                    && !["person", "ai", "automatic"].contains(&kind.as_str())
                {
                    return pkg("`review.reviewer_kind` must be person, ai or automatic");
                }
                if let Some(level) = &r.level
                    && !["checked", "reviewed", "drawing-matched"].contains(&level.as_str())
                {
                    return pkg("`review.level` must be checked, reviewed or drawing-matched");
                }
                let day = r.date.as_bytes();
                let date_ok = r.date.is_empty()
                    || (day.len() == 10 && day.iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() }));
                if !date_ok {
                    return pkg("`review.date` must look like 2026-11-27");
                }
                if r.reviewer.chars().count() > 200 || r.sources_hash.chars().count() > 128 || r.notes.chars().count() > 2000 {
                    return pkg("`review` has a text that is too long");
                }
                Some(Review {
                    reviewer: r.reviewer.clone(),
                    reviewer_kind: r.reviewer_kind.clone(),
                    date: r.date.clone(),
                    sources_hash: r.sources_hash.clone(),
                    level: r.level.clone(),
                    notes: r.notes.clone(),
                })
            }
        };

        // What the package declares for search.
        let mut declared = Vec::new();
        if let Some(Json::Object(d)) = &p.declares
            && let Some(Json::Array(matches)) = d.get("matches")
        {
            for m in matches.iter().take(64) {
                for (key, positive) in [("examples", true), ("negative", false)] {
                    if let Some(Json::Array(list)) = m.get(key) {
                        declared.extend(list.iter().take(64).filter_map(Json::as_str).map(|c| (c.to_string(), positive)));
                    }
                }
            }
        }
        if p.checks.len() > 512 {
            return Err(GeneratorError::Limit { what: "checks", limit: 512 });
        }

        let meta = Meta {
            id: p.id,
            name: p.name,
            description: p.description,
            license: p.license,
            author: p.author,
            guarantee: p.guarantee,
            provenance: p.provenance,
            sources: p.sources.into_iter().map(|s| (s.id, s.title, s.url, s.pages)).collect(),
            review,
        };
        Ok(Generator {
            meta,
            checks: p.checks,
            declared,
            alternatives,
            refuse: p.grammar.refuse,
            tables,
            lookups,
            defaults,
            rules,
            vars,
            steps,
            component,
            consts,
            normalize,
        })
    }

    /// Parse a code: the grammar, defaults, lookups, variables and rules. Changes nothing.
    pub fn parse(&self, code: &str) -> Result<Parsed, GeneratorError> {
        self.parse_with(code, &mut Budget { remaining: MAX_BUILD_STEPS })
    }

    fn parse_with(&self, code: &str, budget: &mut Budget) -> Result<Parsed, GeneratorError> {
        let typed = code.trim();
        let shown = || typed.chars().take(40).collect::<String>();
        let folded;
        let code = match self.normalize {
            Some(true) => {
                folded = typed.to_ascii_uppercase();
                folded.as_str()
            }
            Some(false) => {
                folded = typed.to_ascii_lowercase();
                folded.as_str()
            }
            None => typed,
        };
        let Some(fields) = grammar::parse_code(&self.alternatives, code)? else {
            let message = match &self.refuse {
                Some(m) => m.replace("{code}", &shown()),
                None => format!("`{}` is not an ordering code this generator understands", shown()),
            };
            return Err(GeneratorError::Refused { kind: RefusalKind::Invalid, message });
        };
        let mut scope: Scope = self.consts.clone();
        scope.extend(fields);
        scope.insert("code".into(), Value::Text(code.to_string()));
        let mut order: Vec<String> = scope.keys().filter(|k| *k != "code" && !self.consts.contains_key(*k)).cloned().collect();

        // Defaults, each worked out from the fields and constants alone.
        let mut computed = Vec::new();
        for (name, e) in &self.defaults {
            if !scope.contains_key(name) {
                computed.push((name.clone(), expr::eval(e, &scope, budget).map_err(|err| expr_err(name, err))?));
            }
        }
        for (name, v) in computed {
            scope.insert(name.clone(), v);
            order.push(name);
        }

        self.run_rules(true, &scope, budget)?;

        let mut rows = Vec::new();
        for l in &self.lookups {
            let mut key = Vec::new();
            for (col, e) in &l.key {
                let v = expr::eval(e, &scope, budget).map_err(|err| expr_err(col, err))?;
                key.push(
                    v.key_string()
                        .ok_or_else(|| GeneratorError::Expr { expr: col.clone(), message: "a lookup key must be a number or text".into() })?,
                );
            }
            let Some(table) = self.tables.get(&l.table) else { return pkg(format!("table `{}` is missing", l.table)) };
            let Some(row) = table.find(&key) else {
                let message = match &l.missing {
                    Some(m) => format_message(m, &scope),
                    None => format!("`{}` has no entry for {}", l.table, key.join(", ")),
                };
                return Err(GeneratorError::Refused { kind: l.missing_kind, message });
            };
            let text = |c: &str| match row.values.get(c) {
                Some(Value::Text(t)) if !t.is_empty() => Some(t.clone()),
                _ => None,
            };
            rows.push(RowUsed {
                lookup: l.name.clone(),
                table: l.table.clone(),
                key,
                source: text("source"),
                page: text("page"),
                status: row.status.clone(),
                column_status: row.column_status.clone(),
                note: text("note"),
            });
            scope.insert(l.name.clone(), Value::Record(row.values.clone()));
        }

        let mut vars = Vec::new();
        for (name, e) in &self.vars {
            let v = expr::eval(e, &scope, budget).map_err(|err| expr_err(name, err))?;
            scope.insert(name.clone(), v.clone());
            vars.push((name.clone(), v));
        }

        self.run_rules(false, &scope, budget)?;

        let fields = order.iter().filter_map(|k| scope.get(k).map(|v| (k.clone(), v.clone()))).collect();
        Ok(Parsed { code: code.to_string(), fields, vars, rows, scope })
    }

    /// The rules of one phase, in order; the first whose `when` holds refuses the code.
    fn run_rules(&self, early: bool, scope: &Scope, budget: &mut Budget) -> Result<(), GeneratorError> {
        for r in self.rules.iter().filter(|r| r.early == early) {
            let hit = expr::eval(&r.when, scope, budget).map_err(|err| expr_err("a rule", err))?;
            match hit {
                Value::Bool(true) => return Err(GeneratorError::Refused { kind: r.kind, message: format_message(&r.refuse, scope) }),
                Value::Bool(false) => {}
                _ => return Err(GeneratorError::Expr { expr: "a rule".into(), message: "`when` must give true or false".into() }),
            }
        }
        Ok(())
    }

    /// Parse a code and expand the template into the commands to run.
    pub fn plan(&self, code: &str) -> Result<Plan, GeneratorError> {
        let mut budget = Budget { remaining: MAX_BUILD_STEPS };
        let parsed = self.parse_with(code, &mut budget)?;
        let component = match &self.component {
            None => None,
            Some(t) => match t.render(&parsed.scope, &mut budget)? {
                Json::String(s) if !s.trim().is_empty() => Some(s),
                _ => Some(parsed.code.clone()),
            },
        };
        let holds = |e: &Expr, scope: &Scope, budget: &mut Budget, what: &str| -> Result<bool, GeneratorError> {
            match expr::eval(e, scope, budget).map_err(|err| expr_err(what, err))? {
                Value::Bool(b) => Ok(b),
                _ => Err(GeneratorError::Expr { expr: what.into(), message: "must give true or false".into() }),
            }
        };
        let mut commands: Vec<PlannedCommand> = Vec::new();
        for (index, step) in self.steps.iter().enumerate() {
            if let Some(w) = &step.when
                && !holds(w, &parsed.scope, &mut budget, "a step's `when`")?
            {
                continue;
            }
            // How many times the step runs, checked before anything is expanded.
            let (times, var) = match &step.repeat {
                None => (1usize, None),
                Some(r) => {
                    let count = match expr::eval(&r.count, &parsed.scope, &mut budget).map_err(|e| expr_err("a repeat's `count`", e))? {
                        Value::Num(n) => n,
                        _ => return Err(GeneratorError::Expr { expr: "a repeat's `count`".into(), message: "must be a number".into() }),
                    };
                    if !count.is_finite() || count.fract() != 0.0 || count < 0.0 {
                        return Err(GeneratorError::Expr {
                            expr: "a repeat's `count`".into(),
                            message: format!("must be a whole number of 0 or more, not {}", expr::fmt_num(count)),
                        });
                    }
                    if count > r.max as f64 {
                        return Err(GeneratorError::Limit { what: "repeats in one step (its `max`)", limit: r.max });
                    }
                    (count as usize, Some(&r.var))
                }
            };
            if commands.len().saturating_add(times.saturating_mul(step.body.len())) > MAX_EXPANDED {
                return Err(GeneratorError::Limit { what: "commands in one build", limit: MAX_EXPANDED });
            }
            let mut scope = parsed.scope.clone();
            for i in 0..times {
                if let Some(v) = var {
                    scope.insert(v.clone(), Value::Num(i as f64));
                }
                for leaf in &step.body {
                    if let Some(w) = &leaf.when
                        && !holds(w, &scope, &mut budget, "a command's `when`")?
                    {
                        continue;
                    }
                    let params = leaf.params.render(&scope, &mut budget)?;
                    commands.push(PlannedCommand { step: index, command: leaf.command.clone(), params });
                }
            }
        }
        Ok(Plan { parsed, component, commands })
    }

    /// The value of a variable or field of a parsed code, for a package's `dimensions` checks.
    pub fn named_value(parsed: &Parsed, name: &str) -> Option<f64> {
        match parsed.scope.get(name) {
            Some(Value::Num(n)) => Some(*n),
            _ => None,
        }
    }
}

/// Replace `{name}` and `{lookup.column}` in a message with their values; anything that is not a
/// known name stays as written.
fn format_message(msg: &str, scope: &Scope) -> String {
    let mut out = String::new();
    let mut rest = msg;
    while let Some(open) = rest.find('{') {
        out.push_str(rest.get(..open).unwrap_or_default());
        let after = rest.get(open + 1..).unwrap_or_default();
        let Some(close) = after.find('}') else {
            out.push_str(rest.get(open..).unwrap_or_default());
            return out;
        };
        let name = after.get(..close).unwrap_or_default();
        let parts: Vec<String> = name.split('.').map(str::to_string).collect();
        let mut budget = Budget { remaining: 1000 };
        match expr::eval(&Expr::Path(parts), scope, &mut budget) {
            Ok(v) => out.push_str(&v.display()),
            Err(_) => {
                out.push('{');
                out.push_str(name);
                out.push('}');
            }
        }
        rest = after.get(close + 1..).unwrap_or_default();
    }
    out.push_str(rest);
    out.chars().take(MAX_TEXT_CHARS).collect()
}

#[cfg(test)]
mod tests;
