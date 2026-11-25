//! `sheet.check_shop` (#20113): whether a sheet metal part meets a fabrication shop's file and
//! design rules, as findings a person or a script acts on. The rules are data
//! (`solidcraft_doc::shop_rules`, `docs/formats/shop-rules.md`); the part is read and held against
//! them by `solidcraft_doc::shop_check`. Nothing is changed.

use serde_json::Value;
use solidcraft_doc::shop_check::{self, Severity};
use solidcraft_doc::shop_rules::{self, MAX_PROFILE_BYTES, Profile};

use super::CommandSpec;
use super::sheet::{DxfOptions, dxf_facts, flat_size, surfaces_named};
use crate::params::bad;
use crate::schema::sheet_check::{CheckShopParams, CheckShopResult, DxfCheckParams, ShopFinding, ShopInfo, ShopNotChecked, ShopParam, ShopSeverity};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const CHECK: &str = "sheet.check_shop";
const CONTRACT: TypedContract<CheckShopParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new(CHECK, "Check Against Shop Rules", crate::checked!("sheet.check_shop", check_shop))
    .at("SHEET METAL", "CHECK")
    .icon("inspect")
    .noundo()
    .read_only()
    .contract(&CONTRACT)
    .output(output_schema_for::<CheckShopResult>)
    .params("body?, dxf?: {layers?: roles|single, bend_lines?, bend_notes?, hardware_notes?}, shop: generic|pcbway|jlccnc|sendcutsend | a profile file's path (.json) | a profile object (docs/formats/shop-rules.md) → {shop: {id, name}, thickness, findings: [{rule, severity: error|warning, message, feature?, at?: [x, y], value?, limit?, sources: [url]}], checked: [rule], not_checked: [{rule, why}]}: whether a sheet metal part meets a fabrication shop's file and design rules. Rules: file.one_body, file.uniform_thickness, stock.thickness (by the sheet rule's material), size.max_flat, hole.min_diameter (round holes and perforations), hole.to_edge, hole.to_bend, flange.min (both sides of every bend: the least reach square to the bend, across the panel, to its far edge; a hole in the panel does not shorten it, a cut-out through to its edge does; from where the bend starts unless the profile says the bend line), thread.modelling, hardware.modelling, and for the flat DXF the part would export (with the options in `dxf`, default the export's defaults): file.dxf_single_layer, file.dxf_closed_contours, file.dxf_bend_lines, file.text_outlined; their findings name the `sheet.export_dxf` option that fixes them. A rule whose pages disagree uses the strictest value and gives warnings, never errors. A rule the shop gives no value for, and what could not be measured (cut-outs that are not round), is in not_checked with the reason. Holes are at (x, y) in the flat as read; distances are exact, to where a bend starts unless the profile says the bend line. The built-in values were read from the shops' pages on 2026-11-25 and need a spot check before you rely on them. Read-only: the same part always gives the same answer.")];

/// The profile a call names: a built-in id, a file, or the object itself.
fn profile_of(shop: &ShopParam) -> Result<Profile> {
    let refuse = |why: String| bad(CHECK, format!("`shop`: {why}"));
    match shop {
        ShopParam::Profile(object) => Profile::from_value(&Value::Object(object.clone())).map_err(|e| refuse(e.to_string())),
        ShopParam::Name(name) => {
            let lower = name.to_ascii_lowercase();
            if lower.ends_with(".json") || name.contains('/') || name.contains('\\') {
                let bytes = solidcraft_io::vfs::read(name, MAX_PROFILE_BYTES).map_err(|e| refuse(format!("{name}: {e}")))?;
                Profile::from_bytes(&bytes).map_err(|e| refuse(format!("{name}: {e}")))
            } else {
                shop_rules::profile(name).cloned().map_err(|e| refuse(e.to_string()))
            }
        }
    }
}

/// The options of the DXF the file rules are checked for: the export's defaults, and what the call
/// asks. As in the export, hardware notes have no layer of their own with `layers: single`.
fn dxf_options(asked: Option<&DxfCheckParams>) -> DxfOptions {
    let Some(a) = asked else { return DxfOptions::default() };
    let one_layer = a.layers.is_some_and(|l| l.0);
    let height = 3.0;
    DxfOptions {
        bend_notes: a.bend_notes.unwrap_or(false).then_some(height),
        no_bend_lines: a.bend_lines == Some(false),
        one_layer,
        hardware_notes: (a.hardware_notes.unwrap_or(false) && !one_layer).then_some(height),
        ..DxfOptions::default()
    }
}

fn check_shop(s: &mut Session, p: &Value) -> Result<Value> {
    let request: CheckShopParams = crate::schema::read(CHECK, p)?;
    let profile = profile_of(&request.shop)?;
    let sheet = surfaces_named(s, request.body.as_deref(), CHECK)?;
    sheet.flat_matches_body()?;
    let bodies: Vec<String> = s.model.state().bodies.iter().map(|b| b.name.clone()).collect();
    let size = flat_size(&sheet);
    let mut part = shop_check::part_of(&s.doc, &sheet, bodies, [size.x, size.y]);
    part.dxf = Some(dxf_facts(s, &sheet, &dxf_options(request.dxf.as_ref()))?);
    let report = shop_check::check(&profile, &part);
    let findings = report
        .findings
        .into_iter()
        .map(|f| ShopFinding {
            rule: f.rule,
            severity: if f.severity == Severity::Error { ShopSeverity::Error } else { ShopSeverity::Warning },
            message: f.message,
            feature: f.feature,
            at: f.at,
            value: f.value,
            limit: f.limit,
            sources: f.sources,
        })
        .collect();
    let result = CheckShopResult {
        shop: ShopInfo { id: profile.id.clone(), name: profile.name.clone() },
        thickness: part.thickness,
        findings,
        checked: report.checked,
        not_checked: report.not_checked.into_iter().map(|n| ShopNotChecked { rule: n.rule, why: n.why }).collect(),
        param_warnings: Vec::new(),
    };
    serde_json::to_value(result).map_err(|e| EngineError::Other(e.to_string()))
}
