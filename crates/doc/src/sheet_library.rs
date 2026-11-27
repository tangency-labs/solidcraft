//! Sheet metal rule packs, and the standard library of materials and gauges (#18304).
//!
//! A rule pack is a small file of sheet metal rules that a person or a fabrication shop writes
//! to match their own stock and tooling: JSON, or CSV for the rules alone. The format is
//! `docs/formats/sheet-rule-packs.md`. The standard library is such a pack, shipped in
//! `assets/sheet-metal/standard-library.json` with the source of every value.
//!
//! What a pack row holds is what the stock and the shop decide. The thickness is the data; a
//! gauge is only a label, from a named chart. A material's published minimum inside radius is
//! material data, but the working radius comes from the tooling, so the library leaves it to the
//! rule (or a shop pack). The K-factor comes from the bend model ([`k_factor_estimate`]: the
//! bend method and R/T) unless the pack gives a measured one.
//!
//! A pack is read from a local file someone chose, so everything in it is checked before it is
//! used: its size, its row count and every string are bounded, each value must work out as a
//! rule's value does in a design, and a row may read no parameter but its own `Thickness` (a
//! pack goes into any design). What is wrong is refused with a [`PackError`] naming the row and
//! the value. A pack from a newer version is read for what this version knows, and the fields
//! it skipped are named.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::OnceLock;

use serde_json::{Map, Value, json};

use crate::expr;
use crate::sheet::{AngleDisplay, ReliefShape, SheetRule, UnfoldMethod};

/// The `format` a JSON pack names.
pub const FORMAT: &str = "solidcraft-sheet-rules";
/// The pack version this build writes and reads in full.
pub const VERSION: u64 = 1;
/// The largest pack file read.
pub const MAX_PACK_BYTES: usize = 1 << 20;
/// The most rules a pack holds.
pub const MAX_RULES: usize = 1000;
const MAX_NAME: usize = 128;
const MAX_LABEL: usize = 128;
const MAX_GAUGE: usize = 32;
const MAX_EXPRESSION: usize = 512;
const MAX_NOTE: usize = 2000;
const MAX_SOURCES: usize = 64;
const MAX_SKIPPED: usize = 32;

const REQUIRED: &[&str] = &["name", "thickness"];
/// The columns of a CSV pack, in the order a pack is written.
const COLUMNS: &[&str] = &[
    "name",
    "material",
    "gauge",
    "chart",
    "hardness",
    "min_bend_radius",
    "thickness",
    "k_factor",
    "bend_radius",
    "relief_width",
    "relief_depth",
    "corner_relief",
    "corner_relief_shape",
    "hem_gap",
    "gap",
    "angle_display",
];

/// Where a pack's values come from: a published table, a standard, a shop's own chart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub id: String,
    pub title: String,
    pub url: String,
}

/// How the sheet is bent: the bend model's method (the rows of the generic K-factor chart).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BendMethod {
    #[default]
    Air,
    Bottoming,
    Coining,
}

impl BendMethod {
    pub const ALL: [BendMethod; 3] = [BendMethod::Air, BendMethod::Bottoming, BendMethod::Coining];

    pub fn name(self) -> &'static str {
        match self {
            BendMethod::Air => "air",
            BendMethod::Bottoming => "bottoming",
            BendMethod::Coining => "coining",
        }
    }

    pub fn parse(s: &str) -> Option<BendMethod> {
        BendMethod::ALL.into_iter().find(|m| m.name().eq_ignore_ascii_case(s.trim()))
    }
}

/// Which column of the generic K-factor chart a material reads: soft (aluminium, and copper and
/// brass with it), medium (mild steel) or hard (stainless).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hardness {
    Soft,
    Medium,
    Hard,
}

impl Hardness {
    pub fn name(self) -> &'static str {
        match self {
            Hardness::Soft => "soft",
            Hardness::Medium => "medium",
            Hardness::Hard => "hard",
        }
    }

    pub fn parse(s: &str) -> Option<Hardness> {
        [Hardness::Soft, Hardness::Medium, Hardness::Hard].into_iter().find(|h| h.name().eq_ignore_ascii_case(s.trim()))
    }
}

/// The K-factor the generic chart gives for `method`, `hardness` and an inside radius over
/// thickness of `r_over_t`: by R/T from 0 to T, from T to 3T, and past 3T. An estimate: a shop's
/// coupon measurement is better.
pub fn k_factor_estimate(method: BendMethod, hardness: Hardness, r_over_t: f64) -> f64 {
    let column = match hardness {
        Hardness::Soft => 0,
        Hardness::Medium => 1,
        Hardness::Hard => 2,
    };
    let rows: [[f64; 3]; 2] = match method {
        BendMethod::Air => [[0.33, 0.38, 0.40], [0.40, 0.43, 0.45]],
        BendMethod::Bottoming => [[0.42, 0.44, 0.46], [0.46, 0.47, 0.48]],
        BendMethod::Coining => [[0.38, 0.41, 0.44], [0.44, 0.46, 0.47]],
    };
    let band = if r_over_t < 1.0 {
        rows[0]
    } else if r_over_t <= 3.0 {
        rows[1]
    } else {
        [0.50, 0.50, 0.50]
    };
    band[column]
}

/// One rule of a pack: the rule's thickness and material, and its K-factor and radius when the
/// row gives them (`given`); the gauge it is (`16 ga`, a label, or a metric size such as `1.5 mm`)
/// and whose chart that is; the
/// material's hardness for the K-factor chart and its published minimum inside radius; and the
/// ids of the sources of its values.
#[derive(Clone, Debug, PartialEq)]
pub struct PackRule {
    pub rule: SheetRule,
    pub gauge: String,
    pub chart: String,
    pub hardness: Option<Hardness>,
    /// An expression that may use `Thickness`, or empty when none is published.
    pub min_bend_radius: String,
    pub given: Given,
    pub sources: Vec<String>,
}

/// Which of a rule's working values a pack row gave; the others are the default rule's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Given {
    pub k_factor: bool,
    pub bend_radius: bool,
}

impl PackRule {
    /// A design's or a person's rule as a pack row: every value is its own, K-factor and radius too.
    ///
    /// A pack row unfolds from a K-factor: it has no column for a 90° bend deduction or allowance.
    /// A rule that unfolds from one (#6794) goes in with the K-factor that value works out to, so
    /// the pack gives the same flat pattern as the rule. `None` when that K-factor cannot be worked
    /// out here: the rule's thickness, radius or 90° value reads a design's parameters (`BD90`), or
    /// the value is out of range. The rule's stored K-factor is not what it unfolds by, so it is
    /// never written in its place.
    pub fn of(mut rule: SheetRule) -> Option<PackRule> {
        if UnfoldMethod::parse(&rule.unfold_method) != Some(UnfoldMethod::KFactor) {
            let no_parameters = std::collections::BTreeMap::new();
            let values = crate::Document::default().rule_values(&no_parameters, &rule).ok()?;
            rule.k_factor = values.k_factor.to_string();
            rule.unfold_method = UnfoldMethod::KFactor.name().into();
        }
        rule.unfold_value.clear();
        Some(PackRule {
            rule,
            gauge: String::new(),
            chart: String::new(),
            hardness: None,
            min_bend_radius: String::new(),
            given: Given { k_factor: true, bend_radius: true },
            sources: Vec::new(),
        })
    }

    /// The minimum inside radius for a sheet `thickness` mm thick, when one is published.
    pub fn min_bend_radius_mm(&self, thickness: f64) -> Option<f64> {
        if self.min_bend_radius.is_empty() {
            return None;
        }
        let look = |n: &str| {
            if n == "Thickness" { Ok(expr::Value::length(thickness)) } else { Err(crate::DocError::Expr(format!("unknown parameter `{n}`"))) }
        };
        expr::eval_with(&self.min_bend_radius, &look).and_then(|v| v.to_kind(expr::Kind::Length)).ok()
    }

    /// The gauge as a label that says whose chart it is: `16 ga (Brown & Sharpe)`.
    pub fn gauge_label(&self) -> String {
        match (self.gauge.is_empty(), self.chart.is_empty()) {
            (true, _) => String::new(),
            (false, true) => self.gauge.clone(),
            (false, false) => format!("{} ({})", self.gauge, self.chart),
        }
    }
}

/// A rule pack.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RulePack {
    pub name: String,
    pub note: String,
    pub sources: Vec<Source>,
    pub rules: Vec<PackRule>,
}

/// A pack as read, and what of it this version could not use.
#[derive(Clone, Debug, PartialEq)]
pub struct ReadPack {
    pub pack: RulePack,
    /// The version the pack says it is.
    pub version: u64,
    /// Fields and columns this version does not know, skipped (each named once).
    pub skipped: Vec<String>,
}

/// Why a pack is refused. Every message says what to change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackError {
    TooLarge { bytes: usize },
    NotText,
    Json(String),
    NotAPack,
    BadVersion,
    MissingColumn(&'static str),
    BadCsv { line: usize },
    Empty,
    TooManyRules,
    TooManySources,
    Field { what: &'static str },
    Row { row: usize, name: String, why: String },
    Duplicate { name: String },
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackError::TooLarge { bytes } => {
                write!(f, "the rule pack is {bytes} bytes; a pack is at most {MAX_PACK_BYTES} bytes")
            }
            PackError::NotText => f.write_str("the rule pack is not UTF-8 text: save it as UTF-8 JSON or CSV"),
            PackError::Json(why) => write!(f, "the rule pack is not valid JSON: {why}"),
            PackError::NotAPack => write!(f, "this JSON is not a sheet metal rule pack: a pack is an object with \"format\": \"{FORMAT}\""),
            PackError::BadVersion => f.write_str("the rule pack's \"version\" must be a whole number from 1"),
            PackError::MissingColumn(column) => write!(f, "the rule pack's CSV needs a `{column}` column"),
            PackError::BadCsv { line } => write!(f, "the rule pack's CSV has unmatched quotes on line {line}"),
            PackError::Empty => f.write_str("the rule pack has no rules"),
            PackError::TooManyRules => write!(f, "the rule pack has more than {MAX_RULES} rules"),
            PackError::TooManySources => write!(f, "the rule pack lists more than {MAX_SOURCES} sources"),
            PackError::Field { what } => write!(f, "the rule pack's {what}"),
            PackError::Row { row, name, why } if name.is_empty() => write!(f, "rule {row} of the pack: {why}"),
            PackError::Row { row, name, why } => write!(f, "rule {row} of the pack (`{name}`): {why}"),
            PackError::Duplicate { name } => write!(f, "the rule pack has two rules named `{name}`; each name must be its own"),
        }
    }
}

impl std::error::Error for PackError {}

impl From<PackError> for crate::DocError {
    fn from(e: PackError) -> Self {
        crate::DocError::Invalid(e.to_string())
    }
}

fn plain_text(text: &str, max: usize) -> bool {
    text.len() <= max && !text.chars().any(char::is_control)
}

/// A label of at most `max` characters (not bytes) on one line.
fn plain_label(text: &str, max: usize) -> bool {
    text.chars().count() <= max && !text.chars().any(char::is_control)
}

/// A field a row of this version reads.
fn known_field(key: &str) -> bool {
    COLUMNS.iter().any(|c| c.eq_ignore_ascii_case(key)) || key.eq_ignore_ascii_case("source")
}

/// Read a pack: JSON when it starts with `{`, CSV otherwise.
pub fn read(bytes: &[u8]) -> Result<ReadPack, PackError> {
    if bytes.len() > MAX_PACK_BYTES {
        return Err(PackError::TooLarge { bytes: bytes.len() });
    }
    let text = std::str::from_utf8(bytes).map_err(|_| PackError::NotText)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let trimmed = text.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') { read_json(trimmed) } else { read_csv(text) }
}

fn read_json(text: &str) -> Result<ReadPack, PackError> {
    // serde_json refuses nesting past its recursion limit, so a deep file is an error here.
    let value: Value = serde_json::from_str(text).map_err(|e| PackError::Json(e.to_string()))?;
    let Value::Object(top) = value else { return Err(PackError::NotAPack) };
    if top.get("format").and_then(Value::as_str) != Some(FORMAT) {
        return Err(PackError::NotAPack);
    }
    let version = match top.get("version") {
        None => VERSION,
        Some(v) => v.as_u64().filter(|v| *v >= 1).ok_or(PackError::BadVersion)?,
    };
    let mut skipped = Skipped::default();
    for key in top.keys().filter(|k| !matches!(k.as_str(), "format" | "version" | "name" | "note" | "sources" | "rules")) {
        skipped.add(key);
    }
    let name = optional_text(&top, "name", MAX_NAME).ok_or(PackError::Field { what: "name must be text of at most 128 characters on one line" })?;
    let note = match top.get("note") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) if s.len() <= MAX_NOTE && !s.chars().any(|c| c.is_control() && c != '\n') => s.clone(),
        Some(_) => return Err(PackError::Field { what: "note must be text of at most 2000 characters" }),
    };
    let sources = match top.get("sources") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(list)) => {
            if list.len() > MAX_SOURCES {
                return Err(PackError::TooManySources);
            }
            list.iter().map(source).collect::<Result<Vec<_>, _>>()?
        }
        Some(_) => return Err(PackError::Field { what: "sources must be a list" }),
    };
    let ids: BTreeSet<&str> = sources.iter().map(|s| s.id.as_str()).collect();
    if ids.len() != sources.len() {
        return Err(PackError::Field { what: "sources must each have their own id" });
    }
    let rows = match top.get("rules") {
        Some(Value::Array(rows)) => rows,
        _ => return Err(PackError::Field { what: "rules must be a list of rules" }),
    };
    if rows.len() > MAX_RULES {
        return Err(PackError::TooManyRules);
    }
    let mut rules = Vec::with_capacity(rows.len());
    for (i, row) in rows.iter().enumerate() {
        let n = i + 1;
        let Value::Object(row) = row else {
            return Err(PackError::Row { row: n, name: String::new(), why: "a rule is an object of named values".into() });
        };
        let mut fields = Vec::new();
        for (key, value) in row {
            if !known_field(key) {
                skipped.add(key);
                continue;
            }
            let text = match value {
                Value::String(s) => s.clone(),
                Value::Number(x) => x.to_string(),
                Value::Null => continue,
                Value::Array(_) if key == "source" => continue,
                _ => {
                    let name = row.get("name").and_then(Value::as_str).unwrap_or_default();
                    return Err(row_error(n, name, format!("`{key}` must be text or a number")));
                }
            };
            fields.push((key.as_str(), text));
        }
        let name = row.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
        let row_sources = match row.get("source") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::String(s)) => vec![s.clone()],
            Some(Value::Array(list)) if list.len() <= MAX_SOURCES => list
                .iter()
                .map(|s| s.as_str().map(str::to_string).ok_or_else(|| row_error(n, &name, "`source` is a source id or a list of them".into())))
                .collect::<Result<_, _>>()?,
            Some(_) => return Err(row_error(n, &name, "`source` is a source id or a list of them".into())),
        };
        if let Some(missing) = row_sources.iter().find(|s| !ids.contains(s.as_str())) {
            return Err(row_error(n, &name, format!("source `{missing}` is not in the pack's sources")));
        }
        let mut rule = pack_rule(n, &fields, &mut skipped)?;
        rule.sources = row_sources;
        rules.push(rule);
    }
    finish(RulePack { name, note, sources, rules }, version, skipped)
}

fn optional_text(map: &Map<String, Value>, key: &str, max: usize) -> Option<String> {
    match map.get(key) {
        None | Some(Value::Null) => Some(String::new()),
        Some(Value::String(s)) if plain_text(s, max) => Some(s.trim().to_string()),
        Some(_) => None,
    }
}

fn source(v: &Value) -> Result<Source, PackError> {
    const WHAT: &str = "sources must each be {\"id\", \"title\", \"url\"} with a short id and one-line text";
    let Value::Object(map) = v else { return Err(PackError::Field { what: WHAT }) };
    let id = optional_text(map, "id", 64).filter(|id| !id.is_empty()).ok_or(PackError::Field { what: WHAT })?;
    let title = optional_text(map, "title", MAX_NOTE).ok_or(PackError::Field { what: WHAT })?;
    let url = optional_text(map, "url", MAX_NOTE).ok_or(PackError::Field { what: WHAT })?;
    Ok(Source { id, title, url })
}

fn row_error(row: usize, name: &str, why: String) -> PackError {
    let name = if plain_text(name, MAX_NAME) { name.trim().to_string() } else { String::new() };
    PackError::Row { row, name, why }
}

#[derive(Default)]
struct Skipped(BTreeSet<String>);

impl Skipped {
    fn add(&mut self, key: &str) {
        if self.0.len() < MAX_SKIPPED && plain_text(key, 64) {
            self.0.insert(key.to_string());
        }
    }
}

/// A row's values (`fields`: name and text) as a rule, checked as a design checks a rule.
fn pack_rule(row: usize, fields: &[(&str, String)], skipped: &mut Skipped) -> Result<PackRule, PackError> {
    let get = |key: &str| fields.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.trim()).filter(|v| !v.is_empty());
    let name = get("name").unwrap_or_default();
    let fail = |why: String| row_error(row, name, why);
    for key in REQUIRED {
        if get(key).is_none() {
            return Err(fail(format!("`{key}` is required")));
        }
    }
    if !plain_text(name, MAX_NAME) {
        return Err(fail("the name must be at most 128 characters on one line".into()));
    }
    for (key, _) in fields {
        if !known_field(key) {
            skipped.add(key);
        }
    }
    let mut rule = SheetRule::named(name);
    rule.material = get("material").unwrap_or_default().to_string();
    if !plain_label(&rule.material, MAX_LABEL) {
        return Err(fail("the material must be at most 128 characters on one line".into()));
    }
    let gauge = get("gauge").unwrap_or_default().to_string();
    if !plain_label(&gauge, MAX_GAUGE) {
        return Err(fail("the gauge must be at most 32 characters on one line".into()));
    }
    let chart = get("chart").unwrap_or_default().to_string();
    if !plain_label(&chart, MAX_LABEL) {
        return Err(fail("the chart must be at most 128 characters on one line".into()));
    }
    let hardness = match get("hardness") {
        None => None,
        Some(h) => Some(Hardness::parse(h).ok_or_else(|| fail("`hardness` must be soft, medium or hard".into()))?),
    };
    let given = Given { k_factor: get("k_factor").is_some(), bend_radius: get("bend_radius").is_some() };
    let mut min_bend_radius = String::new();
    // A pack has no 90° bend deduction or allowance (`unfold_value`): a row gives a K-factor.
    let pack_fields = rule.fields_mut().into_iter().filter(|(key, _)| *key != "unfold_value");
    let fields_and_min = pack_fields.chain(std::iter::once(("min_bend_radius", &mut min_bend_radius)));
    for (key, slot) in fields_and_min {
        let Some(text) = get(key) else { continue };
        if !plain_text(text, MAX_EXPRESSION) {
            return Err(fail(format!("`{key}` must be at most 512 characters on one line")));
        }
        if expr::definition(text).is_some() {
            return Err(fail(format!("`{key}` is `{text}`: give a value, not a parameter definition")));
        }
        if let Some(other) = crate::sheet::rule_references(key, text).into_iter().next() {
            return Err(fail(format!("`{key}` uses `{other}`: a pack's values may use only the rule's own `Thickness`")));
        }
        // A length with no unit would take the unit of whichever design the rule goes into.
        let with_thickness = |n: &str| {
            if n == "Thickness" { Ok(expr::Value::length(1.0)) } else { Err(crate::DocError::Expr(format!("unknown parameter `{n}`"))) }
        };
        if key != "k_factor" && expr::eval_with(text, &with_thickness).is_ok_and(|v| v.unitless()) {
            return Err(fail(format!(
                "`{key}` is `{text}` with no unit: write it in mm or in (`2 mm`, `0.0598 in`), since a pack goes into designs of any unit"
            )));
        }
        *slot = text.to_string();
    }
    if let Some(shape) = get("corner_relief_shape") {
        rule.corner_relief_shape =
            ReliefShape::parse(shape).ok_or_else(|| fail("`corner_relief_shape` must be notch, round, rectangular or tear".into()))?.name().into();
    }
    if let Some(display) = get("angle_display") {
        rule.angle_display = AngleDisplay::parse(display).ok_or_else(|| fail("`angle_display` must be bend or included".into()))?.name().into();
    }
    let no_parameters = std::collections::BTreeMap::new();
    let values = crate::Document::default().rule_values(&no_parameters, &rule).map_err(|e| fail(e.to_string()))?;
    let pack_rule = PackRule { rule, gauge, chart, hardness, min_bend_radius, given, sources: Vec::new() };
    if !pack_rule.min_bend_radius.is_empty() && !pack_rule.min_bend_radius_mm(values.thickness).is_some_and(|r| r > 0.0 && r.is_finite()) {
        return Err(fail("`min_bend_radius` must be a length more than 0".into()));
    }
    Ok(pack_rule)
}

fn finish(pack: RulePack, version: u64, skipped: Skipped) -> Result<ReadPack, PackError> {
    if pack.rules.is_empty() {
        return Err(PackError::Empty);
    }
    let mut names = BTreeSet::new();
    if let Some(twice) = pack.rules.iter().find(|r| !names.insert(r.rule.name.to_lowercase())) {
        return Err(PackError::Duplicate { name: twice.rule.name.clone() });
    }
    Ok(ReadPack { pack, version, skipped: skipped.0.into_iter().collect() })
}

/// Split `text` at `separator` outside quotes. A doubled quote toggles twice, so it stays inside.
fn split_outside_quotes(text: &str, separator: char) -> impl Iterator<Item = &str> {
    let mut quoted = false;
    text.split(move |c| {
        if c == '"' {
            quoted = !quoted;
        }
        c == separator && !quoted
    })
}

fn csv_fields(record: &str, line: usize) -> Result<Vec<String>, PackError> {
    split_outside_quotes(record.trim_end_matches('\r'), ',')
        .map(|raw| {
            let field = raw.trim();
            if !field.contains('"') {
                return Ok(crate::csv::unguard(field).to_string());
            }
            match field.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
                Some(inner) if inner.split("\"\"").all(|part| !part.contains('"')) => {
                    Ok(crate::csv::unguard(&inner.replace("\"\"", "\"")).to_string())
                }
                _ => Err(PackError::BadCsv { line }),
            }
        })
        .collect()
}

fn read_csv(text: &str) -> Result<ReadPack, PackError> {
    let mut records = split_outside_quotes(text, '\n').enumerate().filter(|(_, r)| !r.trim().is_empty());
    let Some((line, header)) = records.next() else { return Err(PackError::Empty) };
    let header = csv_fields(header, line + 1)?;
    for column in REQUIRED {
        if !header.iter().any(|h| h.eq_ignore_ascii_case(column)) {
            return Err(PackError::MissingColumn(column));
        }
    }
    let mut skipped = Skipped::default();
    let mut rules = Vec::new();
    for (row, (line, record)) in records.enumerate() {
        if row >= MAX_RULES {
            return Err(PackError::TooManyRules);
        }
        let cells = csv_fields(record, line + 1)?;
        if cells.len() > header.len() {
            let name = cells.first().map_or("", String::as_str);
            return Err(row_error(
                row + 1,
                name,
                format!("it has {} values for {} columns: quote a value that has a comma in it", cells.len(), header.len()),
            ));
        }
        let fields: Vec<(&str, String)> = header.iter().map(String::as_str).zip(cells).collect();
        rules.push(pack_rule(row + 1, &fields, &mut skipped)?);
    }
    finish(RulePack { rules, ..RulePack::default() }, VERSION, skipped)
}

fn rule_json(r: &PackRule) -> Value {
    let rule = &r.rule;
    let mut row = Map::new();
    row.insert("name".into(), json!(rule.name));
    if !rule.material.is_empty() {
        row.insert("material".into(), json!(rule.material));
    }
    for (key, text) in [("gauge", &r.gauge), ("chart", &r.chart), ("min_bend_radius", &r.min_bend_radius)] {
        if !text.is_empty() {
            row.insert(key.into(), json!(text));
        }
    }
    if let Some(h) = r.hardness {
        row.insert("hardness".into(), json!(h.name()));
    }
    for (key, text, _) in rule.fields() {
        // A K-factor or radius the row didn't give stays out: it is the bend model's or the tooling's.
        // A pack has no 90° bend deduction or allowance either (see [`PackRule::of`]).
        if (key == "k_factor" && !r.given.k_factor) || (key == "bend_radius" && !r.given.bend_radius) || key == "unfold_value" {
            continue;
        }
        row.insert(key.into(), json!(text));
    }
    row.insert("corner_relief_shape".into(), json!(rule.corner_relief_shape));
    row.insert("angle_display".into(), json!(rule.angle_display));
    if !r.sources.is_empty() {
        row.insert("source".into(), json!(r.sources));
    }
    Value::Object(row)
}

/// The pack as JSON, in this version's format.
pub fn write_json(pack: &RulePack) -> String {
    let mut top = Map::new();
    top.insert("format".into(), json!(FORMAT));
    top.insert("version".into(), json!(VERSION));
    if !pack.name.is_empty() {
        top.insert("name".into(), json!(pack.name));
    }
    if !pack.note.is_empty() {
        top.insert("note".into(), json!(pack.note));
    }
    if !pack.sources.is_empty() {
        top.insert("sources".into(), pack.sources.iter().map(|s| json!({"id": s.id, "title": s.title, "url": s.url})).collect());
    }
    top.insert("rules".into(), pack.rules.iter().map(rule_json).collect());
    let mut text = serde_json::to_string_pretty(&Value::Object(top)).unwrap_or_default();
    text.push('\n');
    text
}

/// The pack's rules as CSV (a CSV pack holds no name, note or sources).
pub fn write_csv(pack: &RulePack) -> String {
    let mut out = COLUMNS.join(",");
    out.push('\n');
    for r in &pack.rules {
        let rule = &r.rule;
        let value = |column: &str| -> &str {
            match column {
                "name" => &rule.name,
                "material" => &rule.material,
                "gauge" => &r.gauge,
                "chart" => &r.chart,
                "hardness" => r.hardness.map_or("", Hardness::name),
                "min_bend_radius" => &r.min_bend_radius,
                "k_factor" if !r.given.k_factor => "",
                "bend_radius" if !r.given.bend_radius => "",
                "corner_relief_shape" => &rule.corner_relief_shape,
                "angle_display" => &rule.angle_display,
                key => rule.fields().into_iter().find(|(k, _, _)| *k == key).map_or("", |(_, text, _)| text.as_str()),
            }
        };
        out.push_str(&COLUMNS.iter().map(|c| crate::csv::field(value(c))).collect::<Vec<_>>().join(","));
        out.push('\n');
    }
    out
}

/// The standard library: common materials in standard gauges, each value with its source
/// (`assets/sheet-metal/standard-library.json`). The build's tests read it; were it ever
/// unreadable, the library is empty rather than the app failing.
pub fn standard() -> &'static RulePack {
    static LIBRARY: OnceLock<RulePack> = OnceLock::new();
    LIBRARY.get_or_init(|| {
        read(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sheet-metal/standard-library.json")).as_bytes())
            .map(|r| r.pack)
            .unwrap_or_default()
    })
}

/// The standard library's materials, in its order.
pub fn standard_materials() -> Vec<&'static str> {
    let mut out: Vec<&str> = Vec::new();
    for r in &standard().rules {
        if !out.contains(&r.rule.material.as_str()) {
            out.push(&r.rule.material);
        }
    }
    out
}

/// `16`, `16ga`, `16 ga` and `16 gauge` are one gauge, and `1.5 mm` and `1.5mm` one metric size.
fn gauge_key(gauge: &str) -> String {
    let g: String = gauge.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect();
    g.strip_suffix("gauge").or_else(|| g.strip_suffix("ga")).unwrap_or(&g).to_string()
}

/// The standard library's rule for `material` (ignoring case) in `gauge`.
pub fn standard_rule(material: &str, gauge: &str) -> Option<&'static PackRule> {
    let material = material.trim();
    let gauge = gauge_key(gauge);
    standard().rules.iter().find(|r| r.rule.material.eq_ignore_ascii_case(material) && gauge_key(&r.gauge) == gauge)
}

/// The gauges the standard library has of `material`.
pub fn standard_gauges(material: &str) -> Vec<&'static PackRule> {
    standard().rules.iter().filter(|r| r.rule.material.eq_ignore_ascii_case(material.trim())).collect()
}

#[cfg(test)]
#[path = "sheet_library_tests.rs"]
mod tests;
