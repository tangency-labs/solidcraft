//! Contracts and result types for the generator commands (`generator.list`, `generator.parse`,
//! `generator.code`, `generator.template`, `generator.insert`, `generator.check`): a
//! manufacturer's ordering code to a parametric part and back (docs/formats/generator.md).

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// List the generator packages installed for this user.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct GeneratorListParams {}

/// Read an ordering code with a generator and say what it means, without changing the design. A
/// code the generator refuses is an answer (`ok` false), not an error.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct GeneratorParseParams {
    /// The generator: an installed package's id, or the path of its folder (or its
    /// `generator.toml`).
    pub generator: String,
    /// The ordering code, at most 128 characters. Case and separators are ignored.
    pub code: String,
    /// Choices the code does not carry (the caps a rail is ordered with, say), as an object. They
    /// reach the generator's script as `fields.options`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Map<String, Value>>,
}

/// Print parameters as an ordering code.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct GeneratorCodeParams {
    /// The generator: an installed package's id, or the path of its folder.
    pub generator: String,
    /// The field values, by name (the `fields` a parse gives): text, numbers or true/false.
    pub parameters: Map<String, Value>,
    /// Print with this template (its pattern as written); default: the first that can say them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
}

/// Try a field template alone: read a code into fields, or print fields as a code. Every field
/// needs its own spec, as `{length: int 40..2000}`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct GeneratorTemplateParams {
    /// The template, at most 256 characters.
    pub template: String,
    /// A code to read. Give this or `parameters`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// Field values to print. Give this or `code`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Map<String, Value>>,
}

/// Insert a generator's part: a component linked to the package's parametric design, built for
/// an ordering code (or for field values) with its configuration row and parameters, as one undo
/// step. The part stays parametric: `component.set_link_parameters` changes its values later.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct GeneratorInsertParams {
    /// The generator: an installed package's id, or the path of its folder.
    pub generator: String,
    /// The ordering code. Give this or `parameters`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// Field values, as a parse gives them, instead of a code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Map<String, Value>>,
    /// With `code`: choices the code does not carry, as `generator.parse` takes them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Map<String, Value>>,
    /// Where to put it: any of `parent`, `translate`, `axis`, `angle`, `origin`, `matrix`,
    /// `to_matrix`, `tolerance`, as `component.insert_design` takes them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<Map<String, Value>>,
}

/// Run the checks a package declares (its `[[example]]` codes): each must read, print back, insert
/// in a scratch design and measure as declared; refusals must refuse. The open design is not
/// changed.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct GeneratorCheckParams {
    /// The generator: an installed package's id, or the path of its folder.
    pub generator: String,
}

/// Where a configuration row's numbers come from.
#[derive(Debug, Serialize, JsonSchema)]
pub struct GeneratorProvenance {
    /// The configuration row.
    pub row: String,
    /// The source document or catalogue.
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
    /// `verified`, `needs_review` or `not_modelled`.
    pub status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

/// One installed generator.
#[derive(Debug, Serialize, JsonSchema)]
pub struct GeneratorSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub license: String,
    pub author: String,
    /// The package folder.
    pub path: String,
    /// Its ordering-code templates, as written.
    pub templates: Vec<String>,
    /// What every code starts with, for an index.
    pub prefixes: Vec<String>,
}

/// What `generator.list` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct GeneratorListResult {
    pub generators: Vec<GeneratorSummary>,
    /// Folders that are not valid packages, with why.
    pub skipped: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `generator.parse` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct GeneratorParseResult {
    /// Whether the code was accepted.
    pub ok: bool,
    /// When refused: `invalid`, `not_modelled`, `limit` or `error`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// When refused: the generator's own words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// The generator's id.
    pub generator: String,
    /// The code in the template's own spelling (as typed when refused).
    pub code: String,
    /// The template that read it, as written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    /// What the code says, field by field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<Map<String, Value>>,
    /// The configuration row to select.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<String>,
    /// The values for the design's parameters, as expressions.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parameters: BTreeMap<String, String>,
    /// The part's description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<GeneratorProvenance>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `generator.code` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct GeneratorCodeResult {
    pub code: String,
    /// The template that printed it, as written.
    pub template: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `generator.template` returns: the fields of a code, or a code for fields.
#[derive(Debug, Serialize, JsonSchema)]
pub struct GeneratorTemplateResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// What every code of the template starts with.
    pub prefixes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `generator.insert` made.
#[derive(Debug, Serialize, JsonSchema)]
pub struct GeneratorInsertResult {
    pub generator: String,
    /// The ordering code, printed back; it is the component's part number.
    pub code: String,
    /// The new component, by id.
    pub component: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurrence: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<String>,
    pub parameters: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<GeneratorProvenance>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// The bodies the part brought in.
    pub bodies: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume_mm3: Option<f64>,
    /// The part's bounding-box size in mm, `[x, y, z]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bbox_mm: Option<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub faces: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One checked code.
#[derive(Debug, Serialize, JsonSchema)]
pub struct GeneratorCheckRow {
    pub code: String,
    pub ok: bool,
    /// What was wrong; empty when `ok`.
    pub problems: Vec<String>,
}

/// What `generator.check` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct GeneratorCheckResult {
    pub generator: String,
    pub passed: bool,
    pub results: Vec<GeneratorCheckRow>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
