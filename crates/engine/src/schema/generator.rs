//! Contracts and result types for the generator commands (`generator.parse`, `generator.build`,
//! `generator.check`, `generator.list`): a manufacturer's ordering code to a part, from a
//! data-only generator package (docs/formats/generator-package.md).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Read an ordering code against a generator package and say what it means, without changing
/// the design. A code the package refuses is an answer (`ok` false), not an error.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct GeneratorParseParams {
    /// The package: the path of a `.generator.json` file, or the id of an installed package. Give
    /// this or `package_json`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    /// The package itself, as an object or as its JSON text, instead of `package`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_json: Option<serde_json::Value>,
    /// The ordering code, at most 128 characters.
    pub code: String,
}

/// Build a part from an ordering code: parse it, expand the package's command template and run
/// the commands in a new component, as one undo step. A code the package refuses is an error
/// that says why; a failing step leaves the design as it was.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct GeneratorBuildParams {
    /// The package: the path of a `.generator.json` file, or the id of an installed package. Give
    /// this or `package_json`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    /// The package itself, as an object or as its JSON text, instead of `package`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_json: Option<serde_json::Value>,
    /// The ordering code, at most 128 characters.
    pub code: String,
    /// Save the design to this path after building, as `file.save` does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save: Option<String>,
}

/// Run the checks a package carries (expected volume, bounding box, face count, dimensions and
/// refusals per code), each in a scratch design. The open design is not changed.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct GeneratorCheckParams {
    /// The package: the path of a `.generator.json` file, or the id of an installed package. Give
    /// this or `package_json`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    /// The package itself, as an object or as its JSON text, instead of `package`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_json: Option<serde_json::Value>,
}

/// List the generator packages installed for this user.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct GeneratorListParams {}

/// One source a package cites.
#[derive(Debug, Serialize, JsonSchema)]
pub struct GeneratorSource {
    pub id: String,
    pub title: String,
    /// Where it can be read; empty when the package gives none.
    pub url: String,
    /// The pages used; empty when the package gives none.
    pub pages: String,
}

/// What a package says about itself.
#[derive(Debug, Serialize, JsonSchema)]
pub struct GeneratorSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub license: String,
    pub author: String,
    /// The package's own claim: `checked`, `reviewed` or `drawing-matched`. Only a label until
    /// the Workshop index verifies it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guarantee: Option<String>,
    /// The provenance text the package shows beside its parts.
    pub provenance: String,
    pub sources: Vec<GeneratorSource>,
    /// Who checked the package and against what, when it says. Evidence for the guarantee, not
    /// proof: the Workshop index verifies the level.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review: Option<GeneratorReview>,
}

/// The package's `review` object.
#[derive(Debug, Serialize, JsonSchema)]
pub struct GeneratorReview {
    pub reviewer: String,
    /// `person`, `ai` or `automatic`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reviewer_kind: Option<String>,
    /// `YYYY-MM-DD`, or empty.
    pub date: String,
    /// A hash of the sources the review read, as the package gives it.
    pub sources_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    pub notes: String,
}

/// A named value: a field of the code or a variable.
#[derive(Debug, Serialize, JsonSchema)]
pub struct GeneratorValue {
    pub name: String,
    /// A number, text, true or false, or a list of text.
    pub value: serde_json::Value,
}

/// A table row a code used, with its provenance.
#[derive(Debug, Serialize, JsonSchema)]
pub struct GeneratorRow {
    /// The name the package gave the lookup.
    pub lookup: String,
    pub table: String,
    /// The key values.
    pub key: Vec<String>,
    /// The id of the source the row cites.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
    /// The status of the row's columns that have none of their own: `published`, `labelled`,
    /// `measured`, `derived` or `approximate`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// The columns that have a status of their own, by column name.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub column_status: std::collections::BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// What `generator.parse` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct GeneratorParseResult {
    /// Whether the package accepts the code.
    pub ok: bool,
    /// When refused: `invalid` (not a code of this series, or a combination it does not offer) or
    /// `not_modelled` (a real code this generator does not model).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// When refused: the package's own words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub generator: GeneratorSummary,
    /// The code as read (trimmed).
    pub code: String,
    /// The fields the grammar and defaults gave, by name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<GeneratorValue>,
    /// The package's variables, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<GeneratorValue>,
    /// The table rows the code used, with their provenance.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<GeneratorRow>,
    /// How many commands a build would run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commands: Option<usize>,
    /// The name of the component a build would make.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `generator.build` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct GeneratorBuildResult {
    pub generator: GeneratorSummary,
    pub code: String,
    /// The component the part is in; absent when the package builds in the active component.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
    /// How many commands ran.
    pub commands: usize,
    /// The bodies the build made.
    pub bodies: Vec<String>,
    /// Their total volume (mm³), when they enclose one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_mm3: Option<f64>,
    /// The size of their bounding box (mm), x, y, z.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox_mm: Option<[f64; 3]>,
    /// Their total face count.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub faces: Option<u64>,
    /// The fields and the table rows the build used, as `generator.parse` gives them.
    pub fields: Vec<GeneratorValue>,
    pub rows: Vec<GeneratorRow>,
    /// The path the design was saved to, when `save` was given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved: Option<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One check of a package, run.
#[derive(Debug, Serialize, JsonSchema)]
pub struct GeneratorCheckRow {
    pub code: String,
    pub ok: bool,
    /// What did not match, or empty.
    pub problems: Vec<String>,
}

/// What `generator.check` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct GeneratorCheckResult {
    pub generator: GeneratorSummary,
    /// Whether every check and every declared example held.
    pub passed: bool,
    pub results: Vec<GeneratorCheckRow>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One installed package.
#[derive(Debug, Serialize, JsonSchema)]
pub struct InstalledGenerator {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guarantee: Option<String>,
    /// Where the file is.
    pub path: String,
}

/// What `generator.list` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct GeneratorListResult {
    pub generators: Vec<InstalledGenerator>,
    /// Files in the folder that are not valid packages, with why.
    pub skipped: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
