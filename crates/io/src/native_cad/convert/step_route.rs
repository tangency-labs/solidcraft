//! The STEP route a FreeCAD or Rhino file takes by default: cadmpeg decodes the file into its
//! neutral model and writes that as STEP AP214, which SolidCraft's STEP reader then reads and the
//! Import feature keeps like any STEP file. The records route ([`crate::StepReader::Cadmpeg`],
//! the Preferences switch "Import with cadmpeg", off by default) reads the same model straight
//! into B-rep records instead.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeDepth};
use cadmpeg_ir::codec::write::{EncodeInput, target::TargetRequest};
use cadmpeg_ir::validate::validate_neutral_for_decode;
use cadmpeg_registry::{Format, build_encoder};

use super::{ACCEPTED, decode_at, hide_body_members, policy};

/// Restate a FreeCAD or Rhino file (`file` is its name) as STEP text, with the losses
/// and warnings cadmpeg reported. Panics inside cadmpeg become errors.
pub fn native_cad_to_step(bytes: &[u8], file: &str) -> Result<(String, Vec<String>), String> {
    let name = std::path::Path::new(file).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| file.to_string());
    solidcraft_kernel::guard("cadmpeg import", || to_step(&name, bytes).map_err(solidcraft_kernel::KernelError::Invalid)).map_err(|e| match e {
        solidcraft_kernel::KernelError::Invalid(m) => m,
        e => e.to_string(),
    })
}

/// The STEP items of a native CAD file that its solids import leaves out: a FreeCAD body's
/// hidden intermediate shapes. A Rhino file's hidden objects are its own visibility, which the
/// file's user asked to keep, so none are left out; the format is the one cadmpeg detects in
/// the bytes, not the file's name (a Rhino file can be called `.FCStd`).
pub(crate) fn hidden_items(file: &str, bytes: &[u8], step: &str) -> std::collections::HashSet<u64> {
    if crate::is_native_cad_path(file) && crate::detects_freecad(bytes) { invisible_items(step) } else { Default::default() }
}

/// The STEP items a FreeCAD restatement marks invisible: a body's intermediate shapes (each Pad
/// and Pocket, the tool shapes, sketch wires), which FreeCAD hides and only its body's own shape
/// shows. Their bodies are left out of the import ([`crate::StepFeature`]'s `skip`).
pub(crate) fn invisible_items(step: &str) -> std::collections::HashSet<u64> {
    let mut out = std::collections::HashSet::new();
    let mut rest = step;
    while let Some(at) = rest.find("INVISIBILITY(") {
        let after = rest.get(at + "INVISIBILITY(".len()..).unwrap_or_default();
        let list = after.split_once(';').map_or(after, |(l, _)| l);
        out.extend(list.split('#').skip(1).filter_map(|n| n.split(|c: char| !c.is_ascii_digit()).next()?.parse::<u64>().ok()));
        rest = after;
    }
    out
}

fn to_step(name: &str, bytes: &[u8]) -> Result<(String, Vec<String>), String> {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    if !crate::NATIVE_CAD_EXTS.contains(&ext.as_str()) {
        return Err(format!("{name}: not supported (FreeCAD .FCStd and Rhino .3dm only)"));
    }
    let (format, decoded) = decode_at(name, &ext, bytes, &ACCEPTED, DecodeDepth::Full)?;
    let policy = policy();
    let (mut ir, report, fidelity) = decoded.into_parts();
    hide_body_members(&mut ir);
    if !report.geometry_transferred() {
        return Err(format!("{name}: no geometry could be read from this {format} file"));
    }
    let mut warnings: Vec<String> = report.losses.iter().map(|l| format!("{}: {}", l.code, l.message)).collect();

    // Validate the neutral model, as cadmpeg's own conversion does.
    let arena = DecodeArena::new();
    let ctx = DecodeContext::for_loaded_input(&arena, &policy, bytes.len() as u64).map_err(|e| e.to_string())?;
    let validation = validate_neutral_for_decode(&ctx, &ir, Vec::new()).map_err(|e| e.to_string())?;
    ctx.finish_session().map_err(|e| e.to_string())?;
    warnings.extend(validation.findings.iter().map(|f| format!("{}: {}", f.severity, f.message)));
    if !validation.is_ok() {
        return Err(format!("{name}: the {format} model is invalid ({} errors)", validation.error_count()));
    }

    // Write it as STEP AP214.
    let plan = build_encoder(Format::Step)
        .plan(EncodeInput::new(&ir, Some(&fidelity)), TargetRequest::Explicit("ap214"))
        .map_err(|e| format!("{name}: could not restate as STEP: {e}"))?;
    let losses = &plan.report().losses;
    if losses.iter().any(|l| l.code.to_string() == "step/export.no-exportable-solids") {
        return Err(format!("{name}: this {format} file has no solids that can be imported"));
    }
    warnings.extend(losses.iter().map(|l| format!("{}: {}", l.code, l.message)));
    let mut out = Vec::new();
    plan.write_to(&mut out).map_err(|e| e.to_string())?;
    let step = String::from_utf8(out).map_err(|e| e.to_string())?;
    Ok((name_unnamed_products(step, name), warnings))
}

/// cadmpeg's STEP writer calls a product it has no name for `cadmpeg_model`; name it after the
/// file instead (as plain STEP text: letters, digits, spaces, `-`, `_` and `.` only).
fn name_unnamed_products(step: String, file: &str) -> String {
    const PLACEHOLDER: &str = "'cadmpeg_model'";
    let stem: String = std::path::Path::new(file)
        .file_stem()
        .map(|s| s.to_string_lossy().chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.')).collect())
        .unwrap_or_default();
    if stem.trim().is_empty() || !step.contains(PLACEHOLDER) { step } else { step.replace(PLACEHOLDER, &format!("'{stem}'")) }
}
