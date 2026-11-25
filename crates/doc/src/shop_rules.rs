//! Fabrication shop rule profiles (#20113): what a shop asks of a sheet metal part, as data. A
//! profile is a list of rules; each numeric rule keeps every value the shop's pages give, with the
//! page it came from, so a number that changes is a one-line edit and a rule whose pages disagree
//! says so. The built-in profiles are `assets/sheet-metal/shop-rules.json`; the format is
//! `docs/formats/shop-rules.md`.
//!
//! A profile may come from a file or an object a person gave, so everything in it is checked
//! before it is used: its size, its counts and every string are bounded, every number is finite
//! and in range, and what is wrong is refused with a [`ShopRulesError`] naming the rule.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::Value;

/// The `format` of a rules file.
pub const FORMAT: &str = "solidcraft-shop-rules";
/// The version this build reads.
pub const VERSION: u64 = 1;
/// The largest profile file read.
pub const MAX_PROFILE_BYTES: usize = 1 << 20;
const MAX_RULES: usize = 64;
const MAX_LIMITS: usize = 32;
const MAX_STEPS: usize = 32;
const MAX_STOCK: usize = 64;
const MAX_THICKNESSES: usize = 128;
const MAX_SOURCES: usize = 16;
const MAX_MATERIALS: usize = 16;
const MAX_TEXT: usize = 600;
const MAX_NAME: usize = 64;
/// The largest length a rule holds (mm): past it a value is a typo.
const MAX_MM: f64 = 100_000.0;
/// The largest multiple of the thickness or the radius a rule holds.
const MAX_FACTOR: f64 = 1000.0;

/// Why a profile was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShopRulesError(pub String);

impl fmt::Display for ShopRulesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ShopRulesError {}

fn refuse<T>(why: impl Into<String>) -> Result<T, ShopRulesError> {
    Err(ShopRulesError(why.into()))
}

/// What a rule is about: a closed list, so a script can rely on the set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum RuleId {
    /// One body in the file.
    #[serde(rename = "file.one_body")]
    OneBody,
    /// One thickness all over the part.
    #[serde(rename = "file.uniform_thickness")]
    UniformThickness,
    /// The thicknesses the shop stocks.
    #[serde(rename = "stock.thickness")]
    StockThickness,
    /// The largest flat pattern.
    #[serde(rename = "size.max_flat")]
    MaxFlat,
    /// The smallest hole.
    #[serde(rename = "hole.min_diameter")]
    HoleMinDiameter,
    /// A hole's least distance to an edge.
    #[serde(rename = "hole.to_edge")]
    HoleToEdge,
    /// A hole's least distance to a bend.
    #[serde(rename = "hole.to_bend")]
    HoleToBend,
    /// The shortest flange.
    #[serde(rename = "flange.min")]
    FlangeMin,
    /// Whether tapped threads are modelled or left a plain hole.
    #[serde(rename = "thread.modelling")]
    ThreadModelling,
    /// Whether press-in hardware is modelled or named on the order.
    #[serde(rename = "hardware.modelling")]
    HardwareModelling,
    /// The flat DXF has every cut contour on one layer.
    #[serde(rename = "file.dxf_single_layer")]
    DxfSingleLayer,
    /// The flat DXF has closed contours only, none cut twice.
    #[serde(rename = "file.dxf_closed_contours")]
    DxfClosedContours,
    /// The flat DXF draws the bend lines (or leaves them out).
    #[serde(rename = "file.dxf_bend_lines")]
    DxfBendLines,
    /// The flat DXF has its text as outlines, not text entities.
    #[serde(rename = "file.text_outlined")]
    TextOutlined,
}

impl RuleId {
    /// Every rule, in the order a report lists them.
    pub const ALL: &[RuleId] = &[
        RuleId::OneBody,
        RuleId::UniformThickness,
        RuleId::StockThickness,
        RuleId::MaxFlat,
        RuleId::HoleMinDiameter,
        RuleId::HoleToEdge,
        RuleId::HoleToBend,
        RuleId::FlangeMin,
        RuleId::ThreadModelling,
        RuleId::HardwareModelling,
        RuleId::DxfSingleLayer,
        RuleId::DxfClosedContours,
        RuleId::DxfBendLines,
        RuleId::TextOutlined,
    ];

    /// The rule's id as a profile and a report write it.
    pub fn as_str(self) -> &'static str {
        match self {
            RuleId::OneBody => "file.one_body",
            RuleId::UniformThickness => "file.uniform_thickness",
            RuleId::StockThickness => "stock.thickness",
            RuleId::MaxFlat => "size.max_flat",
            RuleId::HoleMinDiameter => "hole.min_diameter",
            RuleId::HoleToEdge => "hole.to_edge",
            RuleId::HoleToBend => "hole.to_bend",
            RuleId::FlangeMin => "flange.min",
            RuleId::ThreadModelling => "thread.modelling",
            RuleId::HardwareModelling => "hardware.modelling",
            RuleId::DxfSingleLayer => "file.dxf_single_layer",
            RuleId::DxfClosedContours => "file.dxf_closed_contours",
            RuleId::DxfBendLines => "file.dxf_bend_lines",
            RuleId::TextOutlined => "file.text_outlined",
        }
    }

    /// Whether the rule is a list of numeric limits.
    fn has_limits(self) -> bool {
        matches!(self, RuleId::MaxFlat | RuleId::HoleMinDiameter | RuleId::HoleToEdge | RuleId::HoleToBend | RuleId::FlangeMin)
    }
}

/// A page a value comes from.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub url: String,
    /// The day the page was read (`2026-11-25`).
    pub read: String,
    /// What the page says, in our own words.
    pub says: String,
}

/// What a value is a multiple of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Relative {
    /// The sheet's thickness.
    Thickness,
}

/// Where a distance to a bend is measured from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Measured {
    /// Where the bend starts, the edge of its zone where the flat sheet ends: the most generous
    /// reading, so a finding is a real one. The default.
    #[default]
    BendStart,
    /// The bend's centre line.
    BendLine,
}

/// The value for a sheet up to `up_to_mm` thick.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub up_to_mm: f64,
    pub value: f64,
}

/// One value a shop's pages give for a numeric rule. A rule with several limits means all of
/// them hold (a laser's `0.5 T` and `1 mm` floor), or, when the rule is marked `disagreement`,
/// that the shop's pages give different values and the strictest is used with a warning.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limit {
    /// The value, in mm, or a multiple of `relative_to`.
    #[serde(default)]
    pub value: Option<f64>,
    /// Or a value for each thickness range, in mm, in order of `up_to_mm`: the first step the
    /// sheet fits is the value; a thicker sheet takes the last.
    #[serde(default)]
    pub steps: Vec<Step>,
    /// `size.max_flat` only: the most the shorter side may be (mm); `value` is the longer side's.
    #[serde(default)]
    pub short_value: Option<f64>,
    #[serde(default)]
    pub relative_to: Option<Relative>,
    /// A multiple of the nearest bend's inside radius added to the value (`hole.to_bend`: `2 T + R`).
    #[serde(default)]
    pub plus_radius: Option<f64>,
    /// `hole.to_bend` and `flange.min`: where the distance is measured from.
    #[serde(default)]
    pub from: Option<Measured>,
    /// Material names (parts of the sheet rule's material, ignoring case) the value is for;
    /// none: any material.
    #[serde(default)]
    pub materials: Vec<String>,
    pub sources: Vec<Source>,
}

impl Limit {
    /// The limit for a sheet `thickness` thick whose nearest bend has inside radius `radius` (mm).
    pub fn needed(&self, thickness: f64, radius: f64) -> Option<f64> {
        let base = match (self.value, self.steps.as_slice()) {
            (Some(v), []) => v * if self.relative_to == Some(Relative::Thickness) { thickness } else { 1.0 },
            (None, steps) => steps.iter().find(|s| thickness <= s.up_to_mm + 1e-9).or(steps.last())?.value,
            _ => return None,
        };
        Some(base + self.plus_radius.unwrap_or(0.0) * radius)
    }

    /// Where a bend distance is measured from.
    pub fn measured_from(&self) -> Measured {
        self.from.unwrap_or_default()
    }
}

/// The thicknesses a shop stocks, for some materials.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stock {
    /// Material names (parts of the sheet rule's material, ignoring case); none: any material.
    #[serde(default)]
    pub materials: Vec<String>,
    /// The listed thicknesses (mm).
    #[serde(default)]
    pub thicknesses_mm: Vec<f64>,
    /// Or a range (mm), from and to.
    #[serde(default)]
    pub min_mm: Option<f64>,
    #[serde(default)]
    pub max_mm: Option<f64>,
    pub sources: Vec<Source>,
}

impl Stock {
    /// Whether a sheet `thickness` thick is stock: within 0.01 mm of a listed thickness, or in the range.
    pub fn admits(&self, thickness: f64) -> bool {
        let listed = self.thicknesses_mm.iter().any(|v| (v - thickness).abs() <= 0.01);
        let ranged = (self.min_mm.is_some() || self.max_mm.is_some())
            && self.min_mm.is_none_or(|m| thickness >= m - 1e-9)
            && self.max_mm.is_none_or(|m| thickness <= m + 1e-9);
        listed || ranged
    }
}

/// What a shop wants of a feature the file may model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Wanted {
    /// A plain hole in the file; the thread or hardware goes on the order.
    Plain,
    /// Modelled in the file.
    Modelled,
}

/// Whether a shop wants the bend lines in a flat DXF.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BendLines {
    /// A line at the centre of each bend, when the part has bends.
    Present,
    /// None: the shop would cut them.
    Absent,
}

/// One rule of a profile.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub id: RuleId,
    /// The unit of the numbers: `mm`, the only one.
    #[serde(default)]
    pub unit: Option<String>,
    /// The shop's process the numbers are for (`laser cutting`), for the message.
    #[serde(default)]
    pub process: Option<String>,
    /// A numeric rule's values.
    #[serde(default)]
    pub limits: Vec<Limit>,
    /// `stock.thickness`: the stock.
    #[serde(default)]
    pub stock: Vec<Stock>,
    /// `thread.modelling` and `hardware.modelling`: what the shop wants.
    #[serde(default)]
    pub wanted: Option<Wanted>,
    /// The pages of a rule without limits or stock.
    #[serde(default)]
    pub sources: Vec<Source>,
    /// `file.dxf_bend_lines`: whether the shop wants the bend lines.
    #[serde(default)]
    pub bend_lines: Option<BendLines>,
    /// The shop's pages give different values: the strictest is used, and a finding is a warning.
    #[serde(default)]
    pub disagreement: bool,
}

impl Rule {
    /// The pages the rule's values come from, once each, in the order they are given.
    pub fn urls(&self) -> Vec<String> {
        let all = self.sources.iter().chain(self.limits.iter().flat_map(|l| &l.sources)).chain(self.stock.iter().flat_map(|s| &s.sources));
        let mut out: Vec<String> = Vec::new();
        for s in all {
            if !out.contains(&s.url) {
                out.push(s.url.clone());
            }
        }
        out
    }
}

/// A shop's rules.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// The id the check is asked for by: lower case letters, digits, `_` and `-`. `custom` when a
    /// profile given as an object has none.
    #[serde(default = "custom")]
    pub id: String,
    pub name: String,
    /// What a person reading the profile should know (how it was read, what to confirm).
    #[serde(default)]
    pub note: Option<String>,
    pub rules: Vec<Rule>,
}

fn custom() -> String {
    "custom".into()
}

impl Profile {
    /// The rule `id`, when the profile has one.
    pub fn rule(&self, id: RuleId) -> Option<&Rule> {
        self.rules.iter().find(|r| r.id == id)
    }

    /// A profile object, checked.
    pub fn from_value(v: &Value) -> Result<Profile, ShopRulesError> {
        let profile: Profile = serde_json::from_value(v.clone()).map_err(|e| ShopRulesError(format!("the shop profile: {e}")))?;
        profile.validate()?;
        Ok(profile)
    }

    /// A profile file's bytes (a profile object), checked.
    pub fn from_bytes(bytes: &[u8]) -> Result<Profile, ShopRulesError> {
        if bytes.len() > MAX_PROFILE_BYTES {
            return refuse(format!("the shop profile is over {MAX_PROFILE_BYTES} bytes"));
        }
        let v: Value = serde_json::from_slice(bytes).map_err(|e| ShopRulesError(format!("the shop profile is not JSON: {e}")))?;
        Profile::from_value(&v)
    }

    fn validate(&self) -> Result<(), ShopRulesError> {
        let ok_id = !self.id.is_empty()
            && self.id.len() <= 32
            && self.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
        if !ok_id {
            return refuse(format!("the profile id `{}` must be 1 to 32 lower case letters, digits, `_` or `-`", clip(&self.id)));
        }
        text("name", &self.name, MAX_NAME)?;
        if let Some(n) = &self.note {
            text("note", n, MAX_TEXT)?;
        }
        if self.rules.is_empty() || self.rules.len() > MAX_RULES {
            return refuse(format!("a profile holds 1 to {MAX_RULES} rules"));
        }
        let mut seen = BTreeSet::new();
        for r in &self.rules {
            if !seen.insert(r.id.as_str()) {
                return refuse(format!("rule {} is given twice", r.id.as_str()));
            }
            r.validate()?;
        }
        Ok(())
    }
}

/// A string in a message, cut short.
fn clip(s: &str) -> String {
    s.chars().take(40).collect()
}

fn text(what: &str, s: &str, max: usize) -> Result<(), ShopRulesError> {
    if s.trim().is_empty() || s.chars().count() > max || s.chars().any(char::is_control) {
        return refuse(format!("`{what}` must be 1 to {max} characters of plain text"));
    }
    Ok(())
}

fn length(rule: &str, what: &str, v: f64) -> Result<(), ShopRulesError> {
    if v.is_finite() && v > 0.0 && v <= MAX_MM {
        Ok(())
    } else {
        refuse(format!("{rule}: {what} must be more than 0 and at most {MAX_MM} mm, not {v}"))
    }
}

fn factor(rule: &str, what: &str, v: f64) -> Result<(), ShopRulesError> {
    if v.is_finite() && (0.0..=MAX_FACTOR).contains(&v) { Ok(()) } else { refuse(format!("{rule}: {what} must be 0 to {MAX_FACTOR}, not {v}")) }
}

fn sources(rule: &str, list: &[Source]) -> Result<(), ShopRulesError> {
    if list.is_empty() || list.len() > MAX_SOURCES {
        return refuse(format!("{rule}: give 1 to {MAX_SOURCES} sources (the page each value comes from)"));
    }
    for s in list {
        if !s.url.starts_with("https://") || s.url.len() > 300 || s.url.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return refuse(format!("{rule}: a source url must be an https:// address, not `{}`", clip(&s.url)));
        }
        text(&format!("{rule}: read"), &s.read, 32)?;
        text(&format!("{rule}: says"), &s.says, MAX_TEXT)?;
    }
    Ok(())
}

fn materials(rule: &str, list: &[String]) -> Result<(), ShopRulesError> {
    if list.len() > MAX_MATERIALS {
        return refuse(format!("{rule}: at most {MAX_MATERIALS} materials"));
    }
    list.iter().try_for_each(|m| text(&format!("{rule}: a material"), m, MAX_NAME))
}

impl Rule {
    fn validate(&self) -> Result<(), ShopRulesError> {
        let name = self.id.as_str();
        if self.unit.as_deref().is_some_and(|u| u != "mm") {
            return refuse(format!("{name}: `unit` is mm, the only unit"));
        }
        if let Some(p) = &self.process {
            text(&format!("{name}: process"), p, MAX_NAME)?;
        }
        if (self.id == RuleId::DxfBendLines) != self.bend_lines.is_some() {
            return refuse(format!("{name}: `bend_lines` (present or absent) belongs to file.dxf_bend_lines, and only to it"));
        }
        let numeric = self.id.has_limits();
        if !numeric && !self.limits.is_empty() {
            return refuse(format!("{name} has no `limits`"));
        }
        if self.id != RuleId::StockThickness && !self.stock.is_empty() {
            return refuse(format!("{name} has no `stock`"));
        }
        let modelling = matches!(self.id, RuleId::ThreadModelling | RuleId::HardwareModelling);
        if modelling != self.wanted.is_some() {
            return refuse(format!("{name}: `wanted` (plain or modelled) belongs to thread.modelling and hardware.modelling, and only to them"));
        }
        if numeric {
            if self.limits.is_empty() || self.limits.len() > MAX_LIMITS {
                return refuse(format!("{name}: give 1 to {MAX_LIMITS} limits"));
            }
            for l in &self.limits {
                l.validate(self.id)?;
            }
        } else if self.id == RuleId::StockThickness {
            if self.stock.is_empty() || self.stock.len() > MAX_STOCK {
                return refuse(format!("{name}: give 1 to {MAX_STOCK} stock entries"));
            }
            for s in &self.stock {
                s.validate()?;
            }
        } else {
            sources(name, &self.sources)?;
        }
        if (numeric || self.id == RuleId::StockThickness) && !self.sources.is_empty() {
            return refuse(format!("{name}: its sources go in each limit or stock entry"));
        }
        Ok(())
    }
}

impl Limit {
    fn validate(&self, id: RuleId) -> Result<(), ShopRulesError> {
        let name = id.as_str();
        sources(name, &self.sources)?;
        materials(name, &self.materials)?;
        match (self.value, self.steps.is_empty()) {
            (Some(v), true) => {
                if self.relative_to.is_some() {
                    factor(name, "a value relative to the thickness", v)?;
                    if v <= 0.0 {
                        return refuse(format!("{name}: a value must be more than 0"));
                    }
                } else {
                    length(name, "a value", v)?;
                }
            }
            (None, false) => {
                if self.steps.len() > MAX_STEPS || self.relative_to.is_some() {
                    return refuse(format!("{name}: give up to {MAX_STEPS} steps, in mm, with no `relative_to`"));
                }
                let mut last = 0.0;
                for s in &self.steps {
                    length(name, "a step's up_to_mm", s.up_to_mm)?;
                    length(name, "a step's value", s.value)?;
                    if s.up_to_mm <= last {
                        return refuse(format!("{name}: the steps must go up in `up_to_mm`"));
                    }
                    last = s.up_to_mm;
                }
            }
            _ => return refuse(format!("{name}: a limit has a `value` or `steps`, one of them")),
        }
        if let Some(v) = self.short_value {
            if id != RuleId::MaxFlat {
                return refuse(format!("{name}: `short_value` belongs to size.max_flat"));
            }
            length(name, "short_value", v)?;
        }
        if let Some(v) = self.plus_radius {
            factor(name, "plus_radius", v)?;
        }
        if self.from.is_some() && !matches!(id, RuleId::HoleToBend | RuleId::FlangeMin) {
            return refuse(format!("{name}: `from` belongs to hole.to_bend and flange.min"));
        }
        if id == RuleId::MaxFlat && (self.relative_to.is_some() || self.plus_radius.is_some() || !self.steps.is_empty()) {
            return refuse(format!("{name}: a size is a plain number of mm"));
        }
        Ok(())
    }
}

impl Stock {
    fn validate(&self) -> Result<(), ShopRulesError> {
        let name = RuleId::StockThickness.as_str();
        sources(name, &self.sources)?;
        materials(name, &self.materials)?;
        if self.thicknesses_mm.len() > MAX_THICKNESSES {
            return refuse(format!("{name}: at most {MAX_THICKNESSES} thicknesses in an entry"));
        }
        for v in self.thicknesses_mm.iter().chain(&self.min_mm).chain(&self.max_mm) {
            length(name, "a thickness", *v)?;
        }
        if let (Some(lo), Some(hi)) = (self.min_mm, self.max_mm)
            && lo > hi
        {
            return refuse(format!("{name}: min_mm is over max_mm"));
        }
        if self.thicknesses_mm.is_empty() && self.min_mm.is_none() && self.max_mm.is_none() {
            return refuse(format!("{name}: an entry lists thicknesses or gives a range"));
        }
        Ok(())
    }
}

/// A rules file: its format, version and profiles.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    format: String,
    version: u64,
    /// For a person reading the file.
    #[allow(dead_code)]
    #[serde(default)]
    note: Option<String>,
    profiles: Vec<Profile>,
}

/// The profiles of a rules file's bytes, checked.
pub fn read_file(bytes: &[u8]) -> Result<Vec<Profile>, ShopRulesError> {
    if bytes.len() > MAX_PROFILE_BYTES {
        return refuse(format!("the shop rules file is over {MAX_PROFILE_BYTES} bytes"));
    }
    let file: File = serde_json::from_slice(bytes).map_err(|e| ShopRulesError(format!("the shop rules file: {e}")))?;
    if file.format != FORMAT {
        return refuse(format!("the shop rules file's format is `{}`, not `{FORMAT}`", clip(&file.format)));
    }
    if file.version != VERSION {
        return refuse(format!("the shop rules file is version {}, and this build reads version {VERSION}", file.version));
    }
    let mut ids = BTreeSet::new();
    for p in &file.profiles {
        p.validate()?;
        if !ids.insert(p.id.clone()) {
            return refuse(format!("profile `{}` is given twice", p.id));
        }
    }
    Ok(file.profiles)
}

fn builtin() -> &'static Result<Vec<Profile>, ShopRulesError> {
    static PROFILES: OnceLock<Result<Vec<Profile>, ShopRulesError>> = OnceLock::new();
    PROFILES.get_or_init(|| read_file(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/sheet-metal/shop-rules.json")).as_bytes()))
}

/// The ids of the built-in profiles, in the file's order.
pub fn profile_ids() -> Vec<&'static str> {
    match builtin() {
        Ok(list) => list.iter().map(|p| p.id.as_str()).collect(),
        Err(_) => Vec::new(),
    }
}

/// The built-in profile `id` (ignoring case).
pub fn profile(id: &str) -> Result<&'static Profile, ShopRulesError> {
    let list = builtin().as_ref().map_err(Clone::clone)?;
    let id = id.trim();
    list.iter().find(|p| p.id.eq_ignore_ascii_case(id)).ok_or_else(|| {
        let known: Vec<&str> = list.iter().map(|p| p.id.as_str()).collect();
        ShopRulesError(format!("`{}` is not a shop profile: the built-in ones are {}", clip(id), known.join(", ")))
    })
}

#[cfg(test)]
#[path = "shop_rules_tests.rs"]
mod tests;
