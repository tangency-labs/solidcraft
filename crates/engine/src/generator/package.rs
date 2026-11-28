//! A generator package: a folder with `generator.toml`, a parametric design (`model`), an
//! optional Rhai script and optional CSV tables (docs/formats/generator.md, section 2). Loading
//! reads only inside the folder, and every part is bounded. Resolving a code is pure: it returns
//! data (a configuration row and parameter values) and never touches a design.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;
use serde_json::Value as Json;

use super::GeneratorError;
use super::code_template::{CharSet, Decl, Declared, FieldKind, MAX_CODE_CHARS, Params, Template, TemplateError, is_name, match_any};
use super::csv_table::{MAX_TABLE_BYTES, Table};
use super::script::{RefusalKind, Script, ScriptError};

/// The manifest, in bytes.
pub const MAX_MANIFEST_BYTES: usize = 256 * 1024;
const MAX_TABLES: usize = 32;
const MAX_TEMPLATES: usize = 32;
const MAX_ROWS_DESCRIBED: usize = 5000;
const MAX_EXAMPLES: usize = 512;
/// The format tag this build reads.
pub const FORMAT: &str = "solidcraft-generator/2";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: String,
    id: String,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    license: String,
    #[serde(default)]
    author: String,
    model: String,
    #[serde(default)]
    min_solidcraft: Option<String>,
    #[serde(default)]
    script: Option<String>,
    /// Parameter names whose values, joined with `-`, name the configuration row (no script).
    #[serde(default)]
    row_from: Vec<String>,
    #[serde(default)]
    parameters: BTreeMap<String, ParamDef>,
    #[serde(default)]
    template: Vec<TemplateDef>,
    #[serde(default)]
    rows: BTreeMap<String, RowDef>,
    #[serde(default)]
    example: Vec<ExampleDef>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ParamDef {
    #[serde(rename = "type")]
    ty: String,
    #[serde(default)]
    values: Vec<String>,
    /// Display text for choice values.
    #[serde(default)]
    labels: BTreeMap<String, String>,
    /// For a `map`: code letters to values.
    #[serde(default)]
    codes: BTreeMap<String, String>,
    #[serde(default)]
    min: Option<f64>,
    #[serde(default)]
    max: Option<f64>,
    #[serde(default)]
    step: Option<f64>,
    #[serde(default)]
    digits: Option<usize>,
    /// The unit written after a number sent to the design (`mm`).
    #[serde(default)]
    unit: Option<String>,
    #[serde(default)]
    default: Option<toml::Value>,
    /// For `text`: the extra characters it accepts, as `[A-Za-z0-9-]`.
    #[serde(default)]
    escape: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TemplateDef {
    pattern: String,
    /// Text for the component's description, with `{field}` filled in.
    #[serde(default)]
    description: Option<String>,
    /// Values this template adds to what it reads (`fields = { long = false }`).
    #[serde(default)]
    fields: BTreeMap<String, toml::Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RowDef {
    #[serde(default)]
    source: String,
    #[serde(default)]
    page: Option<toml::Value>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    note: String,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExampleDef {
    code: String,
    #[serde(default)]
    invalid: bool,
    #[serde(default)]
    message_contains: Option<String>,
    #[serde(default)]
    volume_mm3: Option<f64>,
    #[serde(default)]
    bbox_mm: Option<[f64; 3]>,
    #[serde(default)]
    faces: Option<u64>,
}

/// What a configuration row says about where its numbers come from.
#[derive(Clone, Debug, PartialEq)]
pub struct RowInfo {
    pub source: String,
    pub page: Option<String>,
    /// `verified`, `needs_review` or `not_modelled`.
    pub status: String,
    pub note: String,
    pub description: Option<String>,
}

/// A template with what goes with it.
#[derive(Clone, Debug)]
pub struct TemplateInfo {
    pub template: Template,
    pub description: Option<String>,
    pub set: Params,
}

/// A code a package declares, with what building it must give.
#[derive(Clone, Debug)]
pub struct Example {
    pub code: String,
    pub invalid: bool,
    pub message_contains: Option<String>,
    pub volume_mm3: Option<f64>,
    pub bbox_mm: Option<[f64; 3]>,
    pub faces: Option<u64>,
}

pub struct Package {
    pub dir: PathBuf,
    pub id: String,
    pub name: String,
    pub description: String,
    pub license: String,
    pub author: String,
    /// The design the package inserts, as a file inside the folder.
    pub model: PathBuf,
    pub templates: Vec<TemplateInfo>,
    pub declared: Declared,
    pub units: BTreeMap<String, String>,
    /// Display text for the values of choice parameters, by parameter.
    pub labels: BTreeMap<String, BTreeMap<String, String>>,
    pub rows: BTreeMap<String, RowInfo>,
    pub examples: Vec<Example>,
    row_from: Vec<String>,
    tables: Arc<BTreeMap<String, Table>>,
    script: Option<Script>,
}

/// Why a code was refused, in the generator's words.
#[derive(Clone, Debug, PartialEq)]
pub struct Refusal {
    pub kind: RefusalKind,
    pub message: String,
}

/// A code read all the way: what to insert, and what to say about it.
#[derive(Clone, Debug)]
pub struct Resolved {
    pub template: usize,
    pub fields: Params,
    /// The code in the template's own spelling.
    pub code: String,
    /// The configuration row to select, if any.
    pub configuration: Option<String>,
    /// Values for the design's parameters, as expressions.
    pub parameters: BTreeMap<String, String>,
    pub description: Option<String>,
    pub row: Option<RowInfo>,
    pub warnings: Vec<String>,
}

fn pkg<T>(msg: impl Into<String>) -> Result<T, GeneratorError> {
    Err(GeneratorError::Package(msg.into()))
}

fn version_tuple(v: &str) -> Option<(u64, u64, u64)> {
    let mut it = v.split('.');
    let a = it.next()?.parse().ok()?;
    let b = it.next()?.parse().ok()?;
    let c = it.next().unwrap_or("0").split(['-', '+']).next()?.parse().ok()?;
    Some((a, b, c))
}

/// Drop the `x-` keys, which a package may use for its own notes, at every level.
fn strip_extension_keys(t: &mut toml::Table) {
    t.retain(|k, _| !k.starts_with("x-"));
    for (_, v) in t.iter_mut() {
        match v {
            toml::Value::Table(inner) => strip_extension_keys(inner),
            toml::Value::Array(items) => {
                for item in items {
                    if let toml::Value::Table(inner) = item {
                        strip_extension_keys(inner);
                    }
                }
            }
            _ => {}
        }
    }
}

fn read_limited(path: &Path, limit: usize, what: &str) -> Result<String, GeneratorError> {
    let file = std::fs::File::open(path).map_err(|e| GeneratorError::Package(format!("cannot open {what} `{}`: {e}", path.display())))?;
    let mut text = String::new();
    file.take(limit as u64 + 1)
        .read_to_string(&mut text)
        .map_err(|e| GeneratorError::Package(format!("cannot read {what} `{}`: {e}", path.display())))?;
    if text.len() > limit {
        return Err(GeneratorError::Limit { what: "bytes in a package file", limit });
    }
    Ok(text)
}

/// A file inside the package folder: relative, no `..`, and still inside once links are followed.
fn inside(dir: &Path, relative: &str) -> Result<PathBuf, GeneratorError> {
    let rel = Path::new(relative);
    if relative.is_empty() || rel.is_absolute() || rel.components().any(|c| !matches!(c, std::path::Component::Normal(_))) || relative.contains('\\')
    {
        return pkg(format!("`{relative}` must be a plain relative path inside the package folder"));
    }
    let full = dir.join(rel);
    let (Ok(root), Ok(real)) = (std::fs::canonicalize(dir), std::fs::canonicalize(&full)) else {
        return pkg(format!("`{relative}` does not exist in the package folder"));
    };
    if !real.starts_with(&root) || !real.is_file() {
        return pkg(format!("`{relative}` is not a file inside the package folder"));
    }
    Ok(real)
}

fn json_of(v: &toml::Value, what: &str) -> Result<Json, GeneratorError> {
    match v {
        toml::Value::String(s) if s.chars().count() <= 64 => Ok(Json::String(s.clone())),
        toml::Value::Integer(i) => Ok(Json::from(*i)),
        toml::Value::Float(f) if f.is_finite() => Ok(Json::from(*f)),
        toml::Value::Boolean(b) => Ok(Json::Bool(*b)),
        _ => pkg(format!("{what} must be text (up to 64 characters), a number or true/false")),
    }
}

fn decl(name: &str, d: &ParamDef) -> Result<Decl, GeneratorError> {
    let kind = match d.ty.as_str() {
        "choice" => {
            if d.values.is_empty() || d.values.len() > 256 {
                return pkg(format!("parameter `{name}`: a choice lists 1 to 256 `values`"));
            }
            FieldKind::Choice(d.values.clone())
        }
        "map" => {
            if d.codes.is_empty() || d.codes.len() > 256 {
                return pkg(format!("parameter `{name}`: a map lists 1 to 256 `codes`"));
            }
            FieldKind::Map(d.codes.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        }
        "int" => {
            let (Some(min), Some(max)) = (d.min, d.max) else { return pkg(format!("parameter `{name}`: an int needs `min` and `max`")) };
            let step = d.step.unwrap_or(1.0);
            if !(min.fract() == 0.0 && max.fract() == 0.0 && step.fract() == 0.0) || min > max || step < 1.0 || min.abs() > 1e12 || max.abs() > 1e12 {
                return pkg(format!("parameter `{name}`: an int needs whole `min` <= `max` (within 10^12) and a whole `step` of 1 or more"));
            }
            FieldKind::Int { min: min as i64, max: max as i64, step: step as i64 }
        }
        "decimal" => {
            let (Some(min), Some(max)) = (d.min, d.max) else { return pkg(format!("parameter `{name}`: a decimal needs `min` and `max`")) };
            let step = d.step.unwrap_or(0.0);
            let digits = d.digits.unwrap_or(3);
            if !(min.is_finite() && max.is_finite() && step.is_finite()) || min > max || step < 0.0 || digits > 9 {
                return pkg(format!("parameter `{name}`: a decimal needs finite `min` <= `max`, a `step` of 0 or more and at most 9 `digits`"));
            }
            FieldKind::Decimal { min, max, step, digits }
        }
        "text" => {
            let (min, max) = (d.min.unwrap_or(1.0), d.max.unwrap_or(32.0));
            if min < 1.0 || min > max || max > MAX_CODE_CHARS as f64 {
                return pkg(format!("parameter `{name}`: text needs 1 <= `min` <= `max` <= {MAX_CODE_CHARS}"));
            }
            let extra = match &d.escape {
                Some(e) => CharSet::parse(e).map_err(|e| GeneratorError::Package(format!("parameter `{name}`: {e}")))?,
                None => CharSet::default(),
            };
            FieldKind::Text { min: min as usize, max: max as usize, extra }
        }
        other => return pkg(format!("parameter `{name}`: the type `{other}` is not choice, map, int, decimal or text")),
    };
    let default = match &d.default {
        Some(v) => Some(json_of(v, &format!("the default of `{name}`"))?),
        None => None,
    };
    Ok(Decl { kind, default })
}

impl Package {
    /// Load the package in `dir`.
    pub fn load(dir: &Path) -> Result<Package, GeneratorError> {
        let text = read_limited(&inside(dir, "generator.toml")?, MAX_MANIFEST_BYTES, "the manifest")?;
        let mut table: toml::Table = text.parse().map_err(|e| GeneratorError::Package(format!("generator.toml: {e}")))?;
        strip_extension_keys(&mut table);
        let m: Manifest = toml::Value::Table(table).try_into().map_err(|e| GeneratorError::Package(format!("generator.toml: {e}")))?;
        if m.format != FORMAT {
            return if m.format.starts_with("solidcraft-generator/") {
                Err(GeneratorError::NeedsNewer { needs: "a newer version".into(), have: env!("CARGO_PKG_VERSION").into() })
            } else {
                pkg(format!("`format` must be \"{FORMAT}\""))
            };
        }
        if let Some(need) = &m.min_solidcraft {
            let have = env!("CARGO_PKG_VERSION");
            match (version_tuple(need), version_tuple(have)) {
                (Some(n), Some(h)) if n > h => return Err(GeneratorError::NeedsNewer { needs: need.clone(), have: have.into() }),
                (Some(_), _) => {}
                _ => return pkg("`min_solidcraft` must look like 0.52.0"),
            }
        }
        if m.id.len() < 3 || m.id.len() > 64 || !m.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
            return pkg("`id` is 3 to 64 lower-case letters, digits and dashes");
        }
        if m.name.trim().is_empty() || m.name.chars().count() > 128 {
            return pkg("`name` is 1 to 128 characters");
        }
        let model = inside(dir, &m.model)?;
        if !solidcraft_io::is_design_path(&m.model) {
            return pkg("`model` must be a design file (.sc or .json)");
        }
        let mut declared = Declared::new();
        let mut units = BTreeMap::new();
        let mut labels = BTreeMap::new();
        for (name, d) in &m.parameters {
            if !is_name(name) {
                return pkg(format!("`{name}` is not a plain parameter name"));
            }
            declared.insert(name.clone(), decl(name, d)?);
            if let Some(u) = &d.unit {
                if u.is_empty() || u.len() > 16 || u.chars().any(|c| c.is_whitespace() || c.is_control()) {
                    return pkg(format!("parameter `{name}`: the unit is 1 to 16 characters without spaces"));
                }
                units.insert(name.clone(), u.clone());
            }
            if !d.labels.is_empty() {
                labels.insert(name.clone(), d.labels.clone());
            }
        }
        if m.template.is_empty() || m.template.len() > MAX_TEMPLATES {
            return pkg(format!("a generator has 1 to {MAX_TEMPLATES} [[template]] entries"));
        }
        let mut templates = Vec::new();
        for t in &m.template {
            let template = Template::parse(&t.pattern, &declared).map_err(|e| GeneratorError::Package(format!("template `{}`: {e}", t.pattern)))?;
            let mut set = Params::new();
            for (k, v) in &t.fields {
                if !is_name(k) {
                    return pkg(format!("`{k}` is not a plain field name"));
                }
                set.insert(k.clone(), json_of(v, &format!("`fields.{k}`"))?);
            }
            templates.push(TemplateInfo { template, description: t.description.clone(), set });
        }
        if m.rows.len() > MAX_ROWS_DESCRIBED {
            return Err(GeneratorError::Limit { what: "described rows", limit: MAX_ROWS_DESCRIBED });
        }
        let mut rows = BTreeMap::new();
        for (name, r) in &m.rows {
            let status = r.status.clone().unwrap_or_else(|| "verified".into());
            if !["verified", "needs_review", "not_modelled"].contains(&status.as_str()) {
                return pkg(format!("row `{name}`: `status` is verified, needs_review or not_modelled"));
            }
            let page = match &r.page {
                None => None,
                Some(toml::Value::Integer(n)) => Some(n.to_string()),
                Some(toml::Value::String(s)) if s.chars().count() <= 64 => Some(s.clone()),
                Some(_) => return pkg(format!("row `{name}`: `page` is a number or short text")),
            };
            rows.insert(name.clone(), RowInfo { source: r.source.clone(), page, status, note: r.note.clone(), description: r.description.clone() });
        }
        if m.example.len() > MAX_EXAMPLES {
            return Err(GeneratorError::Limit { what: "examples", limit: MAX_EXAMPLES });
        }
        let examples = m
            .example
            .iter()
            .map(|e| Example {
                code: e.code.clone(),
                invalid: e.invalid,
                message_contains: e.message_contains.clone(),
                volume_mm3: e.volume_mm3,
                bbox_mm: e.bbox_mm,
                faces: e.faces,
            })
            .collect();
        let tables = read_tables(dir)?;
        let templates_for_script: Arc<Vec<Template>> = Arc::new(templates.iter().map(|t| t.template.clone()).collect());
        let script_name = match &m.script {
            Some(s) => Some(s.clone()),
            None => dir.join("generator.rhai").is_file().then(|| "generator.rhai".to_string()),
        };
        let script = match script_name {
            Some(name) => {
                let path = inside(dir, &name)?;
                let source = read_limited(&path, super::script::MAX_SCRIPT_BYTES, "the script")?;
                Some(Script::compile(&source, templates_for_script, tables.clone()).map_err(GeneratorError::Script)?)
            }
            None => None,
        };
        for f in &m.row_from {
            if !declared.contains_key(f) && !templates.iter().any(|t| t.template.fields().contains(f)) {
                return pkg(format!("`row_from` names `{f}`, which no template sets"));
            }
        }
        Ok(Package {
            dir: dir.to_path_buf(),
            id: m.id,
            name: m.name,
            description: m.description,
            license: m.license,
            author: m.author,
            model,
            templates,
            declared,
            units,
            labels,
            rows,
            examples,
            row_from: m.row_from,
            tables,
            script,
        })
    }

    pub fn tables(&self) -> &BTreeMap<String, Table> {
        &self.tables
    }

    /// Read a code against the templates: which one fit, and its fields.
    pub fn read_code(&self, code: &str) -> Result<(usize, Params), Refusal> {
        let list: Vec<Template> = self.templates.iter().map(|t| t.template.clone()).collect();
        match match_any(&list, code) {
            Ok(Some(found)) => Ok(found),
            Ok(None) => Err(Refusal {
                kind: RefusalKind::Invalid,
                message: format!("`{}` is not a {} ordering code", code.trim().chars().take(40).collect::<String>(), self.name),
            }),
            Err(TemplateError::Limit { what, limit }) => Err(Refusal { kind: RefusalKind::Limit, message: format!("more than {limit} {what}") }),
            Err(e) => Err(Refusal { kind: RefusalKind::Error, message: e.to_string() }),
        }
    }

    fn with_unit(&self, name: &str, n: &serde_json::Number) -> String {
        match self.units.get(name) {
            Some(u) => format!("{n} {u}"),
            None => n.to_string(),
        }
    }

    fn script_refusal(e: ScriptError) -> Refusal {
        match e {
            ScriptError::Refused { kind, message } => Refusal { kind, message },
            ScriptError::Limit(m) => Refusal { kind: RefusalKind::Limit, message: m },
            ScriptError::Script(m) => Refusal { kind: RefusalKind::Error, message: m },
        }
    }

    /// Turn the fields of a code into what to insert: the configuration row and the parameter values.
    pub fn resolve_fields(&self, template: usize, fields: Params) -> Result<Resolved, Refusal> {
        let error = |message: String| Refusal { kind: RefusalKind::Error, message };
        let Some(info) = self.templates.get(template) else { return Err(error("no such template".into())) };
        let mut fields = fields;
        for (k, v) in &info.set {
            fields.insert(k.clone(), v.clone());
        }
        let mut configuration = None;
        let mut parameters = BTreeMap::new();
        let mut description = None;
        let scripted = match &self.script {
            Some(s) => s.resolve_in(&fields, template).map_err(Self::script_refusal)?,
            None => None,
        };
        match scripted {
            Some(Json::Object(out)) => {
                match out.get("configuration") {
                    None | Some(Json::Null) => {}
                    Some(Json::String(c)) => configuration = Some(c.clone()),
                    Some(_) => return Err(error("resolve() must give `configuration` as text or ()".into())),
                }
                match out.get("parameters") {
                    None | Some(Json::Null) => {}
                    Some(Json::Object(map)) => {
                        for (name, v) in map {
                            let expression = match v {
                                Json::String(s) => s.clone(),
                                Json::Number(n) => self.with_unit(name, n),
                                _ => return Err(error(format!("resolve() must give `{name}` as text or a number"))),
                            };
                            parameters.insert(name.clone(), expression);
                        }
                    }
                    Some(_) => return Err(error("resolve() must give `parameters` as a map".into())),
                }
                if let Some(Json::String(d)) = out.get("description") {
                    description = Some(d.clone());
                }
            }
            Some(_) => return Err(error("resolve() must return a map".into())),
            None => {
                if !self.row_from.is_empty() {
                    let parts: Vec<String> = self.row_from.iter().filter_map(|f| fields.get(f)).filter_map(super::csv_table::key_text).collect();
                    if parts.len() != self.row_from.len() {
                        return Err(error("the code does not give every field `row_from` names".into()));
                    }
                    configuration = Some(parts.join("-"));
                }
                for (name, v) in &fields {
                    if let Json::Number(n) = v {
                        parameters.insert(name.clone(), self.with_unit(name, n));
                    }
                }
            }
        }
        if description.is_none()
            && let Some(s) = &self.script
        {
            description = s.describe_in(&fields, template).map_err(Self::script_refusal)?;
        }
        let row = configuration.as_ref().and_then(|c| self.rows.get(c).cloned().or_else(|| self.table_row_info(c)));
        let mut warnings = Vec::new();
        if let Some(r) = &row {
            let name = configuration.clone().unwrap_or_default();
            match r.status.as_str() {
                "not_modelled" => {
                    let why = if r.note.is_empty() { format!("`{name}` is a real part this generator does not model") } else { r.note.clone() };
                    return Err(Refusal { kind: RefusalKind::NotModelled, message: why });
                }
                "needs_review" => warnings.push(format!("row `{name}` has not been checked against its source ({})", r.source)),
                _ => {}
            }
        }
        if description.is_none() {
            description = row.as_ref().and_then(|r| r.description.clone()).or_else(|| info.description.clone());
        }
        let description = description.map(|d| fill(&d, &fields));
        let code = info.template.print(&fields).map_err(|e| error(e.to_string()))?;
        if self.read_code(&code)?.0 != template {
            return Err(error("the printed code selects a different template".into()));
        }
        Ok(Resolved { template, fields, code, configuration, parameters, description, row, warnings })
    }

    /// Where a row's numbers come from, when a table has a row of that key with `source`, `page`,
    /// `status`, `note` or `description` columns: the table's own columns are the provenance.
    fn table_row_info(&self, key: &str) -> Option<RowInfo> {
        for table in self.tables.values() {
            let Some(row) = table.row(key) else { continue };
            let get = |name: &str| row.iter().find(|(n, _)| *n == name).and_then(|(_, v)| super::csv_table::key_text(v));
            let (source, status) = (get("source"), get("status"));
            if source.is_none() && status.is_none() {
                continue;
            }
            let status = match status.as_deref() {
                Some(s @ ("verified" | "needs_review" | "not_modelled")) => s.to_string(),
                Some(_) => "needs_review".to_string(),
                None => "verified".to_string(),
            };
            return Some(RowInfo {
                source: source.unwrap_or_default(),
                page: get("page"),
                status,
                note: get("note").unwrap_or_default(),
                description: get("description"),
            });
        }
        None
    }

    /// Read a code all the way.
    pub fn parse(&self, code: &str) -> Result<Resolved, Refusal> {
        self.parse_with(code, None)
    }

    /// Read a code with options that are not in the code (for example the caps a rail is ordered
    /// with): they reach `resolve` as `fields.options`.
    pub fn parse_with(&self, code: &str, options: Option<&serde_json::Map<String, Json>>) -> Result<Resolved, Refusal> {
        let (template, mut fields) = self.read_code(code)?;
        if let Some(o) = options {
            if fields.contains_key("options") {
                return Err(Refusal { kind: RefusalKind::Invalid, message: "options are supplied both in the code and outside it".into() });
            }
            fields.insert("options".to_string(), Json::Object(o.clone()));
        }
        self.resolve_fields(template, fields)
    }

    /// Print parameters as a code, with the first template that can say them (or the one whose
    /// pattern is `template`).
    pub fn code_for(&self, params: &Params, template: Option<&str>) -> Result<(usize, String), TemplateError> {
        let mut last = None;
        for (i, t) in self.templates.iter().enumerate() {
            if template.is_some_and(|p| p != t.template.source()) {
                continue;
            }
            match t.template.print(params) {
                Ok(code) => {
                    let (read_template, _) = self.read_code(&code).map_err(|e| TemplateError::NoMatch(e.message))?;
                    if read_template != i {
                        return Err(TemplateError::NoMatch("the printed code selects a different template".into()));
                    }
                    return Ok((i, code));
                }
                Err(e) => last = Some(e),
            }
        }
        Err(last.unwrap_or_else(|| TemplateError::NoMatch("no such template".into())))
    }
}

/// Replace `{field}` in `text` with the field's value; unknown names stay as written.
fn fill(text: &str, fields: &Params) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        out.push_str(rest.get(..start).unwrap_or_default());
        let after = rest.get(start + 1..).unwrap_or_default();
        match after.find('}') {
            Some(end) => {
                let name = after.get(..end).unwrap_or_default();
                match fields.get(name).and_then(|v| match v {
                    Json::Bool(b) => Some(b.to_string()),
                    other => super::csv_table::key_text(other),
                }) {
                    Some(v) => out.push_str(&v),
                    None => {
                        out.push('{');
                        out.push_str(name);
                        out.push('}');
                    }
                }
                rest = after.get(end + 1..).unwrap_or_default();
            }
            None => {
                out.push_str(rest.get(start..).unwrap_or_default());
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out.chars().take(256).collect()
}

fn read_tables(dir: &Path) -> Result<Arc<BTreeMap<String, Table>>, GeneratorError> {
    let mut out = BTreeMap::new();
    let folder = dir.join("tables");
    let entries = match std::fs::read_dir(&folder) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Arc::new(out)),
        Err(e) => return pkg(format!("cannot read `{}`: {e}", folder.display())),
    };
    let mut files = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| GeneratorError::Package(format!("cannot read `{}`: {e}", folder.display())))?.path();
        if path.extension().is_some_and(|x| x == "csv") {
            files.push(path);
        }
    }
    files.sort();
    if files.len() > MAX_TABLES {
        return Err(GeneratorError::Limit { what: "table files", limit: MAX_TABLES });
    }
    for path in files {
        let Some(name) = path.file_stem().and_then(|n| n.to_str()).filter(|n| is_name(n)).map(str::to_string) else {
            return pkg(format!("`{}` is not a plain table name", path.display()));
        };
        let relative = format!("tables/{}", path.file_name().and_then(|n| n.to_str()).unwrap_or_default());
        let real = inside(dir, &relative)?;
        let text = read_limited(&real, MAX_TABLE_BYTES, "a table")?;
        let table = Table::parse(&text).map_err(|e| GeneratorError::Package(format!("table `{name}`: {e}")))?;
        out.insert(name, table);
    }
    Ok(Arc::new(out))
}
