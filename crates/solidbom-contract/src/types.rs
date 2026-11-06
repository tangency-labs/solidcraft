//! The wire types the desktop app, the calculator and the service share.
//!
//! Identity is a UUID in a string, money and measurements are decimal strings, and a missing
//! value is `null` — never zero. A value here describes one exact source and analysis: editing
//! the source produces a new [`AnalysisKey`], and old measurements must not be attached to it.

use std::collections::BTreeSet;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{ContractError, Result, check_sha256};

/// The contract version this build reads and writes.
pub const SCHEMA_VERSION: u32 = 1;
/// The deepest occurrence path the importer keeps (`kernel::step_in` caps it at 32).
pub const MAX_SOURCE_PATH_DEPTH: usize = 32;
/// Caps on the vectors a hostile draft may carry, before anything is allocated per item.
pub const MAX_DEFINITIONS: usize = 10_000;
pub const MAX_OCCURRENCES: usize = 100_000;
pub const MAX_GEOMETRY_BINDINGS: usize = 100_000;
pub const MAX_PARTS: usize = 10_000;
pub const MAX_ANALYSIS_ISSUES: usize = 10_000;

/// A stored original file: its identity, digest, size and the name the user gave it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceRef {
    pub source_id: Uuid,
    pub sha256: String,
    pub byte_len: u64,
    pub original_name: String,
}

impl SourceRef {
    pub fn validate(&self) -> Result<()> {
        check_sha256(&self.sha256, "source.sha256")?;
        if self.byte_len == 0 {
            return Err(ContractError::invalid("source.byte_len", "an empty file is not a source"));
        }
        if self.original_name.trim().is_empty() {
            return Err(ContractError::invalid("source.original_name", "a source needs the name it was imported as"));
        }
        Ok(())
    }
}

/// The exact source and extractor that produced an analysis.
///
/// `engine_build` is the engine crate version; bump `extractor_version` whenever extraction
/// changes, so saved measurements are never silently attached to a different algorithm.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct AnalysisKey {
    pub source_sha256: String,
    pub engine_build: String,
    pub extractor_version: String,
}

impl AnalysisKey {
    pub fn validate(&self) -> Result<()> {
        check_sha256(&self.source_sha256, "analysis.source_sha256")?;
        check_text(&self.engine_build, "analysis.engine_build")?;
        check_text(&self.extractor_version, "analysis.extractor_version")
    }
}

/// One shared part definition in an analysis.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DefinitionRef {
    pub definition_id: Uuid,
    pub source_key: AnalysisKey,
    pub customer_part_number: Option<String>,
    pub customer_revision: Option<String>,
}

/// One placement of a definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OccurrenceRef {
    pub occurrence_id: Uuid,
    pub definition_id: Uuid,
    pub parent_occurrence_id: Option<Uuid>,
    /// Child indices in the imported tree, not session-local component ids.
    pub source_path: Vec<u64>,
    /// Column-major 4 × 4 placement (`transform[column][row]`), as the engine's `Mat` and the
    /// STEP importer keep it. Converting through a row-major reading would transpose it.
    pub transform: [[f64; 4]; 4],
}

/// A source-local body bound to a definition and occurrence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GeometryBinding {
    pub source_path: Vec<u64>,
    pub source_body: u32,
    pub definition_id: Uuid,
    pub occurrence_id: Uuid,
}

/// The units the prototype's contracts carry.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Unit {
    #[serde(rename = "mm3")]
    MillimetreCubed,
    #[serde(rename = "cm3")]
    CentimetreCubed,
    #[serde(rename = "g_cm3")]
    GramPerCentimetreCubed,
    #[serde(rename = "cny_kg")]
    CurrencyPerKilogram,
    #[serde(rename = "s")]
    Second,
    #[serde(rename = "h")]
    Hour,
    #[serde(rename = "cny_h")]
    CurrencyPerHour,
    #[serde(rename = "cny")]
    Currency,
}

/// Where a measurement came from.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MeasurementSource {
    Extracted,
    Manual,
}

/// Whether an engineer has reviewed a value.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewState {
    Pending,
    Confirmed,
}

/// A value with its unit, provenance and review state. Missing is `None`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Measurement {
    pub value: Option<Decimal>,
    pub unit: Unit,
    pub source: MeasurementSource,
    pub review: ReviewState,
    /// Why a manual value was entered; required for manual values.
    pub reason: Option<String>,
    pub analysis_key: AnalysisKey,
}

impl Measurement {
    pub fn validate(&self, field: &str) -> Result<()> {
        if let Some(value) = self.value
            && value.is_sign_negative()
        {
            return Err(ContractError::invalid(format!("{field}.value"), "a measurement cannot be negative"));
        }
        if self.source == MeasurementSource::Manual {
            match self.reason.as_deref() {
                Some(reason) if !reason.trim().is_empty() => {}
                _ => return Err(ContractError::invalid(format!("{field}.reason"), "a manual value needs a reason")),
            }
        }
        self.analysis_key.validate()
    }
}

/// The extracted tree, definitions, placements and geometry bindings for one exact source.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnalysisManifest {
    pub schema_version: u32,
    pub key: AnalysisKey,
    pub definitions: Vec<DefinitionRef>,
    pub occurrences: Vec<OccurrenceRef>,
    pub geometry_bindings: Vec<GeometryBinding>,
    pub issues: Vec<Issue>,
}

impl AnalysisManifest {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(ContractError::SchemaVersion { found: self.schema_version, expected: SCHEMA_VERSION });
        }
        self.key.validate()?;
        if self.definitions.len() > MAX_DEFINITIONS {
            return Err(ContractError::invalid("analysis.definitions", "too many definitions"));
        }
        if self.occurrences.len() > MAX_OCCURRENCES {
            return Err(ContractError::invalid("analysis.occurrences", "too many occurrences"));
        }
        if self.geometry_bindings.len() > MAX_GEOMETRY_BINDINGS {
            return Err(ContractError::invalid("analysis.geometry_bindings", "too many bindings"));
        }
        if self.issues.len() > MAX_ANALYSIS_ISSUES {
            return Err(ContractError::invalid("analysis.issues", "too many issues"));
        }

        let mut definition_ids = BTreeSet::new();
        for definition in &self.definitions {
            if !definition_ids.insert(definition.definition_id) {
                return Err(ContractError::invalid("analysis.definitions", "two definitions share one id"));
            }
            if definition.source_key != self.key {
                return Err(ContractError::invalid("analysis.definitions.source_key", "a definition belongs to a different analysis"));
            }
        }

        let mut occurrence_ids = BTreeSet::new();
        let mut source_paths = BTreeSet::new();
        for occurrence in &self.occurrences {
            if !occurrence_ids.insert(occurrence.occurrence_id) {
                return Err(ContractError::invalid("analysis.occurrences", "two occurrences share one id"));
            }
            if !definition_ids.contains(&occurrence.definition_id) {
                return Err(ContractError::invalid("analysis.occurrences.definition_id", "no such definition"));
            }
            if occurrence.source_path.len() > MAX_SOURCE_PATH_DEPTH {
                return Err(ContractError::invalid("analysis.occurrences.source_path", "the source path is too deep"));
            }
            if !source_paths.insert((occurrence.definition_id, occurrence.source_path.clone())) {
                return Err(ContractError::invalid("analysis.occurrences.source_path", "two placements share one path"));
            }
            check_transform(&occurrence.transform, "analysis.occurrences.transform")?;
        }
        for occurrence in &self.occurrences {
            if let Some(parent) = occurrence.parent_occurrence_id
                && !occurrence_ids.contains(&parent)
            {
                return Err(ContractError::invalid("analysis.occurrences.parent_occurrence_id", "no such parent occurrence"));
            }
        }

        let mut binding_keys = BTreeSet::new();
        for binding in &self.geometry_bindings {
            if !definition_ids.contains(&binding.definition_id) {
                return Err(ContractError::invalid("analysis.geometry_bindings.definition_id", "no such definition"));
            }
            if !occurrence_ids.contains(&binding.occurrence_id) {
                return Err(ContractError::invalid("analysis.geometry_bindings.occurrence_id", "no such occurrence"));
            }
            if binding.source_path.len() > MAX_SOURCE_PATH_DEPTH {
                return Err(ContractError::invalid("analysis.geometry_bindings.source_path", "the source path is too deep"));
            }
            if !binding_keys.insert((binding.occurrence_id, binding.source_body)) {
                return Err(ContractError::invalid("analysis.geometry_bindings", "two bindings share one body"));
            }
        }
        for issue in &self.issues {
            issue.validate()?;
        }
        Ok(())
    }
}

/// Whether a part is bought or made. The prototype calculates made parts only.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Sourcing {
    Purchased,
    Manufactured,
}

/// Which time a cycle figure describes.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CycleBasis {
    PerPiece,
}

/// One reviewed manufacturing operation. Every number stays optional until entered.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OperationInputs {
    pub cycle_seconds: Option<Decimal>,
    pub cycle_basis: CycleBasis,
    pub cavities: Option<u32>,
    pub production_rate_version_id: Option<Uuid>,
    pub setup_hours: Option<Decimal>,
    pub setup_rate_version_id: Option<Uuid>,
}

impl OperationInputs {
    pub fn validate(&self) -> Result<()> {
        if let Some(cycle) = self.cycle_seconds
            && cycle.is_sign_negative()
        {
            return Err(ContractError::invalid("operation.cycle_seconds", "a cycle time cannot be negative"));
        }
        if let Some(cavities) = self.cavities
            && cavities == 0
        {
            return Err(ContractError::invalid("operation.cavities", "at least one cavity is needed"));
        }
        if let Some(hours) = self.setup_hours
            && hours.is_sign_negative()
        {
            return Err(ContractError::invalid("operation.setup_hours", "setup time cannot be negative"));
        }
        Ok(())
    }
}

/// The reviewed engineering and manufacturing inputs for one part definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PartInputs {
    pub part_version_id: Uuid,
    pub classification: String,
    pub classification_confirmed: bool,
    pub sourcing: Sourcing,
    pub volume_mm3: Measurement,
    pub material_version_id: Option<Uuid>,
    pub allowance_rate: Option<Decimal>,
    pub operation: Option<OperationInputs>,
}

impl PartInputs {
    pub fn validate(&self) -> Result<()> {
        if self.classification_confirmed && self.classification.trim().is_empty() {
            return Err(ContractError::invalid("part.classification", "a confirmed classification needs a name"));
        }
        self.volume_mm3.validate("part.volume_mm3")?;
        if self.volume_mm3.unit != Unit::MillimetreCubed {
            return Err(ContractError::invalid("part.volume_mm3.unit", "the reviewed volume is in mm3"));
        }
        if let Some(allowance) = self.allowance_rate
            && allowance.is_sign_negative()
        {
            return Err(ContractError::invalid("part.allowance_rate", "an allowance cannot be negative"));
        }
        if let Some(operation) = &self.operation {
            operation.validate()?;
        }
        Ok(())
    }
}

/// A project draft exactly as the desktop saves it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DraftProject {
    pub project_id: Uuid,
    pub product_revision_id: Uuid,
    /// The saved version this draft is based on; 0 for a project that has never been saved.
    pub version: u64,
    pub source: SourceRef,
    pub analysis: AnalysisManifest,
    pub parts: Vec<PartInputs>,
    pub forecast_year: u16,
    pub forecast_quantity: u64,
    pub currency: String,
}

impl DraftProject {
    pub fn validate(&self) -> Result<()> {
        self.source.validate()?;
        self.analysis.validate()?;
        if self.source.sha256 != self.analysis.key.source_sha256 {
            return Err(ContractError::invalid("draft.source.sha256", "the analysis describes a different source"));
        }
        if !(1900..=9999).contains(&self.forecast_year) {
            return Err(ContractError::invalid("draft.forecast_year", "expected a four-digit calendar year"));
        }
        if self.forecast_quantity == 0 {
            return Err(ContractError::invalid("draft.forecast_quantity", "the fixture needs a positive forecast"));
        }
        if self.currency != "CNY" {
            return Err(ContractError::invalid("draft.currency", "this prototype records costs in CNY"));
        }
        if self.parts.len() > MAX_PARTS {
            return Err(ContractError::invalid("draft.parts", "too many parts"));
        }
        let mut part_ids = BTreeSet::new();
        for part in &self.parts {
            if !part_ids.insert(part.part_version_id) {
                return Err(ContractError::invalid("draft.parts", "two parts share one id"));
            }
            part.validate()?;
            if part.volume_mm3.analysis_key != self.analysis.key {
                return Err(ContractError::invalid("draft.parts.volume_mm3.analysis_key", "the measurement describes a different analysis"));
            }
        }
        Ok(())
    }
}

/// A versioned save request. Reusing `request_id` with the same draft is a safe retry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SaveDraft {
    pub request_id: String,
    pub expected_version: u64,
    pub draft: DraftProject,
}

impl SaveDraft {
    pub fn validate(&self) -> Result<()> {
        check_text(&self.request_id, "save.request_id")?;
        if self.draft.version != self.expected_version {
            return Err(ContractError::invalid("save.expected_version", "the draft describes a different version"));
        }
        self.draft.validate()
    }
}

/// The receipt a committed save returns.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SaveReceipt {
    pub request_id: String,
    pub project_id: Uuid,
    pub version: u64,
}

/// Ask the server to calculate one saved version.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CalculateRequest {
    pub request_id: String,
    pub project_id: Uuid,
    pub saved_version: u64,
}

impl CalculateRequest {
    pub fn validate(&self) -> Result<()> {
        check_text(&self.request_id, "calculation.request_id")?;
        if self.saved_version == 0 {
            return Err(ContractError::invalid("calculation.saved_version", "a calculation needs a saved version"));
        }
        Ok(())
    }
}

/// Server-resolved inputs: versions have become the values they name.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CalculationInputs {
    pub currency: String,
    pub material_overhead_rate: Decimal,
    pub parts: Vec<ResolvedPartInput>,
}

/// One part's values as the calculator receives them. `None` is missing, never zero.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResolvedPartInput {
    pub part_version_id: Uuid,
    pub quantity: u32,
    pub volume_mm3: Option<Decimal>,
    pub density_g_cm3: Option<Decimal>,
    pub material_price_per_kg: Option<Decimal>,
    pub allowance_rate: Option<Decimal>,
    pub cycle_seconds: Option<Decimal>,
    pub cavities: Option<u32>,
    pub production_rate_per_hour: Option<Decimal>,
    pub setup_hours: Option<Decimal>,
    pub setup_rate_per_hour: Option<Decimal>,
}

/// Whether every supported direct-cost line is known.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResultState {
    Incomplete,
    Complete,
}

/// One explained arithmetic line.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CostLine {
    pub code: String,
    pub label: String,
    pub amount: Option<Decimal>,
    pub basis: String,
}

/// One part's direct-cost result. A missing input leaves its affected amounts null.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PartCost {
    pub part_version_id: Uuid,
    pub quantity: u32,
    pub net_mass_kg: Option<Decimal>,
    pub allowed_material_cost: Option<Decimal>,
    pub material_overhead: Option<Decimal>,
    pub production_cost: Option<Decimal>,
    pub per_part_subtotal: Option<Decimal>,
    /// The per-part subtotal expanded by quantity.
    pub line_total: Option<Decimal>,
    pub explanations: Vec<CostLine>,
}

/// The calculator's answer: supported direct costs only, with the setup charge kept separate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CalculationOutcome {
    pub state: ResultState,
    pub parts: Vec<PartCost>,
    pub assembly_subtotal: Option<Decimal>,
    pub setup_total: Option<Decimal>,
    pub issues: Vec<Issue>,
}

/// An immutable calculation result for one saved version.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CalculationSnapshot {
    pub calculation_id: Uuid,
    pub project_id: Uuid,
    pub saved_version: u64,
    pub input_hash: String,
    pub calculator_version: String,
    pub inputs: CalculationInputs,
    pub outputs: CalculationOutcome,
    pub issues: Vec<Issue>,
}

/// A server-owned material version. The development service seeds explicit test values only; they
/// are not company defaults.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MaterialVersion {
    pub material_version_id: Uuid,
    pub name: String,
    pub density_g_cm3: Decimal,
    pub price_per_kg: Decimal,
    pub currency: String,
}

/// A server-owned hourly rate version for production or setup.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RateVersion {
    pub rate_version_id: Uuid,
    pub name: String,
    pub rate_per_hour: Decimal,
    pub currency: String,
}

/// The immutable reference data a project may select from.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReferenceData {
    pub materials: Vec<MaterialVersion>,
    pub rates: Vec<RateVersion>,
}

impl ReferenceData {
    pub fn validate(&self) -> Result<()> {
        let mut material_ids = BTreeSet::new();
        for material in &self.materials {
            if !material_ids.insert(material.material_version_id) {
                return Err(ContractError::invalid("reference.materials", "two materials share one id"));
            }
            check_text(&material.name, "reference.materials.name")?;
            if material.density_g_cm3 <= Decimal::ZERO {
                return Err(ContractError::invalid("reference.materials.density_g_cm3", "density must be positive"));
            }
            if material.price_per_kg.is_sign_negative() {
                return Err(ContractError::invalid("reference.materials.price_per_kg", "a price cannot be negative"));
            }
            if material.currency != "CNY" {
                return Err(ContractError::invalid("reference.materials.currency", "this prototype records costs in CNY"));
            }
        }
        let mut rate_ids = BTreeSet::new();
        for rate in &self.rates {
            if !rate_ids.insert(rate.rate_version_id) {
                return Err(ContractError::invalid("reference.rates", "two rates share one id"));
            }
            check_text(&rate.name, "reference.rates.name")?;
            if rate.rate_per_hour <= Decimal::ZERO {
                return Err(ContractError::invalid("reference.rates.rate_per_hour", "a rate must be positive"));
            }
            if rate.currency != "CNY" {
                return Err(ContractError::invalid("reference.rates.currency", "this prototype records costs in CNY"));
            }
        }
        Ok(())
    }

    pub fn material(&self, id: Uuid) -> Option<&MaterialVersion> {
        self.materials.iter().find(|material| material.material_version_id == id)
    }

    pub fn rate(&self, id: Uuid) -> Option<&RateVersion> {
        self.rates.iter().find(|rate| rate.rate_version_id == id)
    }
}

/// The compact reference a project view lists for each saved calculation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CalculationSummary {
    pub calculation_id: Uuid,
    pub saved_version: u64,
    pub calculator_version: String,
}

/// What reopening a project returns: the current draft and the immutable results saved for it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectView {
    pub draft: DraftProject,
    pub calculations: Vec<CalculationSummary>,
}

/// A named problem with the field it affects. A missing value is an issue, not a zero.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Issue {
    pub code: String,
    pub field_path: String,
    pub definition_id: Option<Uuid>,
    pub message: String,
}

impl Issue {
    pub fn new(code: impl Into<String>, field_path: impl Into<String>, message: impl Into<String>) -> Self {
        Self { code: code.into(), field_path: field_path.into(), definition_id: None, message: message.into() }
    }

    pub fn for_definition(mut self, definition_id: Uuid) -> Self {
        self.definition_id = Some(definition_id);
        self
    }

    pub fn validate(&self) -> Result<()> {
        check_text(&self.code, "issue.code")?;
        check_text(&self.field_path, "issue.field_path")?;
        check_text(&self.message, "issue.message")
    }
}

fn check_text(text: &str, field: &str) -> Result<()> {
    if text.trim().is_empty() { Err(ContractError::invalid(field, "must not be empty")) } else { Ok(()) }
}

fn check_transform(transform: &[[f64; 4]; 4], field: &str) -> Result<()> {
    if transform.iter().flatten().any(|value| !value.is_finite()) {
        return Err(ContractError::invalid(field, "a placement must be finite"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use rust_decimal::Decimal;

    use super::*;
    use crate::error::decimal_from_f64;

    fn key() -> AnalysisKey {
        AnalysisKey { source_sha256: "a".repeat(64), engine_build: "0.40.0".into(), extractor_version: "1".into() }
    }

    fn source() -> SourceRef {
        SourceRef { source_id: Uuid::nil(), sha256: "a".repeat(64), byte_len: 10, original_name: "three_boxes.step".into() }
    }

    fn measured(value: Option<&str>) -> Measurement {
        Measurement {
            value: value.map(|v| v.parse().unwrap_or(Decimal::ZERO)),
            unit: Unit::MillimetreCubed,
            source: if value.is_some() { MeasurementSource::Extracted } else { MeasurementSource::Manual },
            review: ReviewState::Confirmed,
            reason: None,
            analysis_key: key(),
        }
    }

    #[test]
    fn a_decimal_is_written_as_a_string_and_a_missing_value_as_null() {
        let present = measured(Some("2400"));
        let absent = measured(None);
        assert_eq!(serde_json::to_value(&present).unwrap()["value"], serde_json::json!("2400"));
        assert_eq!(serde_json::to_value(&absent).unwrap()["value"], serde_json::Value::Null);
    }

    #[test]
    fn an_explicit_zero_stays_distinct_from_missing() {
        let zero = measured(Some("0"));
        let absent = measured(None);
        assert_eq!(serde_json::to_value(&zero).unwrap()["value"], serde_json::json!("0"));
        assert_ne!(serde_json::to_value(&zero).unwrap()["value"], serde_json::to_value(&absent).unwrap()["value"]);
    }

    #[test]
    fn unknown_fields_are_tolerated() {
        let json = serde_json::json!({
            "source_id": Uuid::nil(),
            "sha256": "a".repeat(64),
            "byte_len": 10,
            "original_name": "three_boxes.step",
            "written_by": "a later build",
        });
        assert!(serde_json::from_value::<SourceRef>(json).is_ok());
    }

    #[test]
    fn a_later_schema_version_is_refused() {
        let manifest = AnalysisManifest {
            schema_version: SCHEMA_VERSION + 1,
            key: key(),
            definitions: Vec::new(),
            occurrences: Vec::new(),
            geometry_bindings: Vec::new(),
            issues: Vec::new(),
        };
        assert!(matches!(manifest.validate(), Err(ContractError::SchemaVersion { .. })));
    }

    #[test]
    fn a_manual_measurement_needs_a_reason() {
        let mut measurement = measured(None);
        assert!(measurement.validate("volume").is_err());
        measurement.reason = Some("open surface measured in the workshop".into());
        assert!(measurement.validate("volume").is_ok());
    }

    #[test]
    fn a_definition_and_its_occurrences_must_share_the_analysis_key() {
        let definition_id = Uuid::nil();
        let manifest = AnalysisManifest {
            schema_version: SCHEMA_VERSION,
            key: key(),
            definitions: vec![DefinitionRef {
                definition_id,
                source_key: AnalysisKey { source_sha256: "b".repeat(64), ..key() },
                customer_part_number: None,
                customer_revision: None,
            }],
            occurrences: Vec::new(),
            geometry_bindings: Vec::new(),
            issues: Vec::new(),
        };
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn a_draft_must_describe_one_source_version_and_a_positive_forecast() {
        let draft = DraftProject {
            project_id: Uuid::nil(),
            product_revision_id: Uuid::nil(),
            version: 0,
            source: source(),
            analysis: AnalysisManifest {
                schema_version: SCHEMA_VERSION,
                key: key(),
                definitions: Vec::new(),
                occurrences: Vec::new(),
                geometry_bindings: Vec::new(),
                issues: Vec::new(),
            },
            parts: Vec::new(),
            forecast_year: 2027,
            forecast_quantity: 1,
            currency: "CNY".into(),
        };
        assert!(draft.validate().is_ok());
        assert!(DraftProject { forecast_quantity: 0, ..draft.clone() }.validate().is_err());
    }

    #[test]
    fn a_geometry_number_converts_through_its_round_trip_text() {
        assert_eq!(decimal_from_f64(2400.0, "volume").unwrap().to_string(), "2400");
    }
}
