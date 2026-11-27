//! `sheet.check_shop` (#20113): whether a sheet metal part meets a fabrication shop's file and
//! design rules, as findings a person or a script acts on. The rules are data
//! (`solidcraft_doc::shop_rules`, `docs/formats/shop-rules.md`); the part is read and held against
//! them by `solidcraft_doc::shop_check`. Nothing is changed.

use serde_json::Value;
use solidcraft_doc::shop_check::{self, Severity};
use solidcraft_doc::shop_presets::Preset;
use solidcraft_doc::shop_rules::{self, DxfLayers, MAX_PROFILE_BYTES, Profile};

use super::CommandSpec;
use super::sheet::{DxfOptions, dxf_facts, flat_size, surfaces_named};
use crate::params::bad;
use crate::schema::sheet_check::{
    CheckShopParams, CheckShopResult, DeleteShopPresetParams, DeleteShopPresetResult, DxfCheckParams, DxfChecked, DxfFrom, SaveShopPresetParams,
    SaveShopPresetResult, ShopFinding, ShopInfo, ShopNotChecked, ShopParam, ShopPresetsParams, ShopPresetsResult, ShopSeverity,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const CHECK: &str = "sheet.check_shop";
const CONTRACT: TypedContract<CheckShopParams> = TypedContract::new();
const PRESETS: &str = "sheet.shop_presets";
const SAVE: &str = "sheet.save_shop_preset";
const DELETE: &str = "sheet.delete_shop_preset";
const PRESETS_CONTRACT: TypedContract<ShopPresetsParams> = TypedContract::new();
const SAVE_CONTRACT: TypedContract<SaveShopPresetParams> = TypedContract::new();
const DELETE_CONTRACT: TypedContract<DeleteShopPresetParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new(CHECK, "Check Against Shop Rules", crate::checked!("sheet.check_shop", check_shop))
    .at("SHEET METAL", "CHECK")
    .icon("inspect")
    .noundo()
    .read_only()
    .contract(&CONTRACT)
    .output(output_schema_for::<CheckShopResult>)
    .params("body?, dxf?: {layers?: roles|single|cut, bend_lines?, bend_notes?, hardware_notes?}, shop: generic | the id of one of your presets (sheet.shop_presets) | a profile file's path (.json) | a profile object (docs/formats/shop-rules.md) → {shop: {id, name}, thickness, dxf: {layers, bend_lines, bend_notes, hardware_notes, from: asked|recommended|default}, findings: [{rule, severity: error|warning, message, feature?, at?: [x, y], value?, limit?, sources: [url]}], checked: [rule], not_checked: [{rule, why}]}: whether a sheet metal part meets a fabrication shop's file and design rules. Rules: file.one_body, file.uniform_thickness, stock.thickness (by the sheet rule's material), size.max_flat, hole.min_diameter (round holes and perforations), hole.to_edge, hole.to_bend, flange.min (both sides of every bend: the least reach square to the bend, across the panel, to its far edge; a hole in the panel does not shorten it, a cut-out through to its edge does; from where the bend starts unless the profile says the bend line), thread.modelling, hardware.modelling, and for the flat DXF the part would export (with the options in `dxf`; when it is left out, the export the shop's profile recommends, and the export's defaults for a profile that names none; the reply's `dxf` says which options were used): file.dxf_single_layer, file.dxf_closed_contours, file.dxf_bend_lines, file.text_outlined; their findings name the `sheet.export_dxf` option that fixes them. A rule whose pages disagree uses the strictest value and gives warnings, never errors. A rule the shop gives no value for, and what could not be measured (cut-outs that are not round), is in not_checked with the reason. Holes are at (x, y) in the flat as read; distances are exact, to where a bend starts unless the profile says the bend line. The built-in Generic profile is general sheet-metal design practice, not any shop's rules: save your shop's own numbers as a preset (sheet.save_shop_preset) and check against that before you order. Read-only: the same part always gives the same answer."),
    CommandSpec::new(PRESETS, "Shop Presets", crate::checked!("sheet.shop_presets", list_presets))
        .icon("list")
        .noundo()
        .doc_only()
        .read_only()
        .contract(&PRESETS_CONTRACT)
        .output(output_schema_for::<ShopPresetsResult>)
        .params("profile?: generic | a preset's id → {built_in: [{id, name}], presets: [{id, name}], profile?: the profile asked for, as an object}: the shop profiles sheet.check_shop takes by id: the built-in Generic and your own presets. Pass `profile` to get one as the object to edit and save as a preset."),
    CommandSpec::new(SAVE, "Save Shop Preset", crate::checked!("sheet.save_shop_preset", save_preset))
        .icon("save")
        .noundo()
        .doc_only()
        .contract(&SAVE_CONTRACT)
        .output(output_schema_for::<SaveShopPresetResult>)
        .params("name?: 1..64 characters, profile: a profile object (docs/formats/shop-rules.md) | path: a profile file (.json) → {id, name, replaced}: keeps a shop's rules as one of your presets, to check against by its id (`sheet.check_shop` `shop`). The id is the profile's `id`, or made from the name; the same id replaces the preset. The profile is checked as any profile is, and refused with the reason when it is not valid, when the name is `Generic`, or when 32 presets are saved. Presets are yours, kept with your preferences and in no design."),
    CommandSpec::new(DELETE, "Delete Shop Preset", crate::checked!("sheet.delete_shop_preset", delete_preset))
        .icon("delete")
        .noundo()
        .doc_only()
        .contract(&DELETE_CONTRACT)
        .output(output_schema_for::<DeleteShopPresetResult>)
        .params("id: a preset's id (sheet.shop_presets) → deleted: {id, name}: removes one of your shop presets; the built-in Generic cannot be deleted"),
];

/// The profile a call names: a built-in or preset id, a file, or the object itself.
fn profile_of(s: &Session, shop: &ShopParam) -> Result<Profile> {
    let refuse = |why: String| bad(CHECK, format!("`shop`: {why}"));
    match shop {
        ShopParam::Profile(object) => Profile::from_value(&Value::Object(object.clone())).map_err(|e| refuse(e.to_string())),
        ShopParam::Name(name) => {
            let lower = name.to_ascii_lowercase();
            if lower.ends_with(".json") || name.contains('/') || name.contains('\\') {
                let bytes = solidcraft_io::vfs::read(name, MAX_PROFILE_BYTES).map_err(|e| refuse(format!("{name}: {e}")))?;
                Profile::from_bytes(&bytes).map_err(|e| refuse(format!("{name}: {e}")))
            } else if let Ok(builtin) = shop_rules::profile(name) {
                Ok(builtin.clone())
            } else if let Some(preset) = s.shop_presets.get(name) {
                preset.parsed().map_err(|e| refuse(format!("the preset `{}`: {e}", preset.id)))
            } else {
                let why = shop_rules::profile(name).err().map_or_else(String::new, |e| e.to_string());
                Err(refuse(format!("{why}; {}", s.shop_presets.known())))
            }
        }
    }
}

fn info(id: &str, name: &str) -> ShopInfo {
    ShopInfo { id: id.to_string(), name: name.to_string() }
}

fn reply(result: impl serde::Serialize) -> Result<Value> {
    serde_json::to_value(result).map_err(|e| EngineError::Other(e.to_string()))
}

/// `sheet.shop_presets`: the built-in profile and the person's presets, and one of them as an
/// object when asked.
fn list_presets(s: &mut Session, p: &Value) -> Result<Value> {
    let request: ShopPresetsParams = crate::schema::read(PRESETS, p)?;
    let profile = match request.profile.as_deref() {
        None => None,
        Some(id) => Some(match shop_rules::profile_value(id) {
            Some(builtin) => builtin,
            None => match s.shop_presets.get(id) {
                Some(preset) => preset.profile.clone(),
                None => {
                    let why = shop_rules::profile(id).err().map_or_else(String::new, |e| e.to_string());
                    return Err(bad(PRESETS, format!("`profile`: {why}; {}", s.shop_presets.known())));
                }
            },
        }),
    };
    let built_in = shop_rules::profile_ids().into_iter().filter_map(|id| shop_rules::profile(id).ok().map(|p| info(id, &p.name))).collect();
    let presets = s.shop_presets.list().iter().map(|p| info(&p.id, &p.name)).collect();
    reply(ShopPresetsResult { built_in, presets, profile })
}

/// `sheet.save_shop_preset`: a profile object, or the one in a file, kept as a preset.
fn save_preset(s: &mut Session, p: &Value) -> Result<Value> {
    let request: SaveShopPresetParams = crate::schema::read(SAVE, p)?;
    let object = match (request.profile, request.path) {
        (Some(object), None) => Value::Object(object),
        (None, Some(path)) => {
            let bytes = solidcraft_io::vfs::read(&path, MAX_PROFILE_BYTES).map_err(|e| bad(SAVE, format!("`path`: {path}: {e}")))?;
            serde_json::from_slice(&bytes).map_err(|e| bad(SAVE, format!("`path`: {path} is not JSON: {e}")))?
        }
        _ => return Err(bad(SAVE, "give either `profile` (a profile object) or `path` (a profile file)")),
    };
    let preset = Preset::new(request.name.as_deref(), object).map_err(|e| bad(SAVE, e.to_string()))?;
    let (id, name) = (preset.id.clone(), preset.name.clone());
    let replaced = s.shop_presets.save(preset).map_err(|e| bad(SAVE, e.to_string()))?;
    reply(SaveShopPresetResult { id, name, replaced })
}

/// `sheet.delete_shop_preset`.
fn delete_preset(s: &mut Session, p: &Value) -> Result<Value> {
    let request: DeleteShopPresetParams = crate::schema::read(DELETE, p)?;
    if shop_rules::profile(&request.id).is_ok() {
        return Err(bad(DELETE, format!("`id`: `{}` is the built-in profile, which stays", request.id.trim())));
    }
    let gone = s.shop_presets.delete(&request.id).map_err(|e| bad(DELETE, format!("`id`: {e}")))?;
    reply(DeleteShopPresetResult { deleted: info(&gone.id, &gone.name) })
}

/// The options of the DXF the file rules are checked for, as the export reads them. As in the
/// export, hardware notes have no layer of their own with `layers: single`.
fn dxf_options(layers: DxfLayers, bend_lines: bool, bend_notes: bool, hardware_notes: bool) -> DxfOptions {
    let height = 3.0;
    DxfOptions {
        bend_notes: bend_notes.then_some(height),
        no_bend_lines: !bend_lines,
        one_layer: layers == DxfLayers::Single,
        cut_layer: layers == DxfLayers::Cut,
        hardware_notes: (hardware_notes && layers != DxfLayers::Single).then_some(height),
        ..DxfOptions::default()
    }
}

/// What the DXF is checked with: what the call asks (the export's defaults for what it leaves
/// out), or, when it asks nothing, what the shop's profile recommends (the export's defaults when
/// it names none). The options come back as the reply's `dxf`.
fn dxf_checked(asked: Option<&DxfCheckParams>, profile: &Profile) -> (DxfOptions, DxfChecked) {
    let (layers, bend_lines, bend_notes, hardware_notes, from) = match (asked, &profile.dxf_export) {
        (Some(a), _) => (
            a.layers.map_or(DxfLayers::Roles, |l| l.0),
            a.bend_lines.unwrap_or(true),
            a.bend_notes.unwrap_or(false),
            a.hardware_notes.unwrap_or(false),
            DxfFrom::Asked,
        ),
        (None, Some(r)) => (r.layers, r.bend_lines, r.bend_notes, r.hardware_notes, DxfFrom::Recommended),
        (None, None) => (DxfLayers::Roles, true, false, false, DxfFrom::Default),
    };
    let options = dxf_options(layers, bend_lines, bend_notes, hardware_notes);
    let echo = DxfChecked {
        layers: layers.as_str().to_string(),
        bend_lines,
        bend_notes,
        hardware_notes: hardware_notes && layers != DxfLayers::Single,
        from,
    };
    (options, echo)
}

fn check_shop(s: &mut Session, p: &Value) -> Result<Value> {
    let request: CheckShopParams = crate::schema::read(CHECK, p)?;
    let profile = profile_of(s, &request.shop)?;
    let sheet = surfaces_named(s, request.body.as_deref(), CHECK)?;
    sheet.flat_matches_body()?;
    let bodies: Vec<String> = s.model.state().bodies.iter().map(|b| b.name.clone()).collect();
    let size = flat_size(&sheet);
    // The features on this sheet's body: those that built or changed it, and the cosmetic threads
    // listed on it (a Thread that changes no geometry).
    let mut owned = s.body_features().remove(&sheet.body).unwrap_or_default();
    owned.extend(s.model.state().threads.iter().filter(|t| t.body == sheet.body).map(|t| t.feature));
    let mut part = shop_check::part_of(&s.doc, &sheet, bodies, [size.x, size.y], &owned);
    let (options, dxf) = dxf_checked(request.dxf.as_ref(), &profile);
    part.dxf = Some(dxf_facts(s, &sheet, &options)?);
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
        dxf,
        findings,
        checked: report.checked,
        not_checked: report.not_checked.into_iter().map(|n| ShopNotChecked { rule: n.rule, why: n.why }).collect(),
        param_warnings: Vec::new(),
    };
    serde_json::to_value(result).map_err(|e| EngineError::Other(e.to_string()))
}
