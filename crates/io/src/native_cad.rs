//! Other CAD formats read through [cadmpeg](https://github.com/cadmpeg/cadmpeg): FreeCAD
//! `.FCStd` and Rhino `.3dm`. cadmpeg decodes the file into its neutral model and writes that as
//! STEP, which the Import feature then reads and keeps like any STEP file.
//!
//! Only codecs for open formats are compiled in (the workspace enables cadmpeg-registry's
//! `fcstd`, `rhino` and `step` features alone). cadmpeg's codecs for commercial formats, and its
//! `sat` codec (a ShapeManager stream decoder that reads ACIS as one dialect), are not built.
//!
//! This module is the importer seam: the one place cadmpeg is called, returning STEP text plus
//! warnings. A cadmpeg-based STEP reader could be added here later and chosen by a setting, to
//! compare with our own STEP reader, without changing that reader.

/// File extensions read through cadmpeg (lower case).
pub const NATIVE_CAD_EXTS: [&str; 2] = ["fcstd", "3dm"];

/// Is this a FreeCAD or Rhino file name (any case)?
pub fn is_native_cad_path(path: &str) -> bool {
    std::path::Path::new(path).extension().and_then(|e| e.to_str()).is_some_and(|e| NATIVE_CAD_EXTS.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// Restate a FreeCAD or Rhino file (`file` is its name) as STEP text, with the losses
/// and warnings cadmpeg reported. Panics inside cadmpeg become errors.
pub fn native_cad_to_step(bytes: &[u8], file: &str) -> Result<(String, Vec<String>), String> {
    let name = std::path::Path::new(file).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| file.to_string());
    solidcraft_kernel::guard("cadmpeg import", || convert::to_step(&name, bytes).map_err(solidcraft_kernel::KernelError::Invalid)).map_err(
        |e| match e {
            solidcraft_kernel::KernelError::Invalid(m) => m,
            e => e.to_string(),
        },
    )
}

/// A FreeCAD file's design history rebuilt as features (sketches, Pad and Pocket, Revolution and
/// Groove, Fillet and Chamfer), checked against the file's own solids: each rebuilt body has the
/// volume and extent of one of them. The error says why the history was not rebuilt; the file's
/// solids then import as before ([`native_cad_to_step`]). Rhino files carry no such history.
pub fn native_cad_design(bytes: &[u8], file: &str) -> Result<NativeDesign, String> {
    let name = std::path::Path::new(file).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| file.to_string());
    let design = solidcraft_kernel::guard("cadmpeg design", || convert::design(&name, bytes).map_err(solidcraft_kernel::KernelError::Invalid))
        .map_err(|e| match e {
            solidcraft_kernel::KernelError::Invalid(m) => m,
            e => e.to_string(),
        })?;
    let rebuilt = design::evaluate(&design.doc)?;
    let file_solids = crate::step_import_feature(bytes, file).map_err(|e| e.to_string())?;
    let skip = match &file_solids.kind {
        solidcraft_doc::FeatureKind::Import { skip, .. } => skip.clone(),
        _ => Vec::new(),
    };
    let theirs: Vec<_> = file_solids.import.bodies.iter().zip(0u64..).filter(|(_, i)| !skip.contains(i)).map(|(b, _)| b.body.clone()).collect();
    design::check_against(&rebuilt, &theirs)?;
    Ok(design)
}

mod design;
pub use design::NativeDesign;

/// The STEP items a cadmpeg restatement marks invisible: a FreeCAD body's intermediate shapes
/// (each Pad and Pocket, the tool shapes, sketch wires), which FreeCAD hides and only its body's
/// own shape shows. Their bodies are left out of the import ([`crate::StepFeature`]'s `skip`).
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

mod convert {
    use std::io::Cursor;

    use cadmpeg_core::decode::WorkBudget;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodeMode, DecodePolicy, ResourceLimits, View};
    use cadmpeg_ir::codec::write::{EncodeInput, target::TargetRequest};
    use cadmpeg_ir::codec::{DecodeFailure, DecodeOptions, DecodeResult};
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::eval::{admission::EvaluationAdmission, finite_or_refusal, model_curve_point_by_id};
    use cadmpeg_ir::index::ModelIndex;
    use cadmpeg_ir::validate::validate_neutral_for_decode;
    use cadmpeg_registry::{DETECTION_PREFIX_LEN, Format, InputCatalog, ResolveSourceError, ResolvedSource, build_encoder, forced_input};

    /// The codecs we accept (cadmpeg codec ids), whatever else the registry recognises.
    const ACCEPTED: [&str; 2] = ["fcstd", "rhino"];

    /// Caps for hostile input: cadmpeg's tight `service` profile, with our STEP size limit on
    /// the input. The service profile already admits a detailed part's 8 million entities:
    /// cadmpeg 0.6.3 dropped the collection-item budget that refused one at a million (#1783)
    /// and the work-unit budget that went with it.
    pub(super) fn policy() -> DecodePolicy {
        DecodePolicy {
            mode: DecodeMode::Salvage,
            limits: ResourceLimits { max_input_bytes: crate::MAX_STEP_BYTES as u64, ..ResourceLimits::service() },
        }
    }

    /// Decode `bytes` (a file called `name`, extension `ext`) with the codec cadmpeg detects from
    /// the bytes, else the one the extension names, if it is one of `accepted`. Returns the codec id
    /// and the decoded model.
    pub(super) fn decode_at(name: &str, ext: &str, bytes: &[u8], accepted: &[&str], depth: DecodeDepth) -> Result<(String, DecodeResult), String> {
        let policy = policy();
        let catalog = InputCatalog::with_builtins();
        let codec = {
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).map_err(|e| e.to_string())?;
            let prefix = bytes.get(..DETECTION_PREFIX_LEN).unwrap_or(bytes);
            let resolved = catalog.resolve_seekable_source(&ctx, View::over_retained(prefix), &mut Cursor::new(bytes), None);
            let resolved = match resolved {
                Ok(ResolvedSource::Unrecognized | ResolvedSource::Cadir) | Err(ResolveSourceError::Ambiguous(_)) => {
                    match forced_input(ext).map_err(|e| e.to_string())? {
                        Some(forced) => catalog.resolve_source(&ctx, View::over_retained(prefix), Some(forced)),
                        None => Ok(ResolvedSource::Unrecognized),
                    }
                }
                other => other,
            };
            ctx.finish_session().map_err(|e| e.to_string())?;
            match resolved.map_err(|e| format!("{name}: {e}"))? {
                ResolvedSource::Native { codec, .. } => codec,
                _ => return Err(format!("{name}: unrecognised file contents")),
            }
        };
        let format = codec.id().as_str().to_string();
        if !accepted.contains(&format.as_str()) {
            return Err(format!("{name}: {format} files are not supported (FreeCAD .FCStd and Rhino .3dm only)"));
        }

        // Decode in memory under the caps, as deep as the STEP writer reads: solids and products,
        // not the design history, which the import does not keep.
        let options = DecodeOptions { depth, policy };
        let decoded = codec.decode(&mut Cursor::new(bytes), &options).map_err(|e| match e {
            DecodeFailure::Codec(e) => format!("{name}: could not read this {format} file: {e}"),
            other => format!("{name}: {other}"),
        })?;
        Ok((format, decoded))
    }

    /// The design history of a FreeCAD file (`name`), rebuilt.
    pub(super) fn design(name: &str, bytes: &[u8]) -> Result<super::NativeDesign, String> {
        let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        let (format, decoded) = decode_at(name, &ext, bytes, &ACCEPTED, DecodeDepth::Full)?;
        if format != "fcstd" {
            return Err(format!("{format} files carry no feature history to rebuild"));
        }
        let (ir, _, _) = decoded.into_parts();
        let mut arenas = super::design::Arenas::of(&ir.model)?;
        // Each object's placement, from cadmpeg's record of FreeCAD's attachments.
        for r in ir.native.namespace("fcstd").and_then(|n| n.arenas().get("attachments")).into_iter().flatten() {
            let object = r.field("object").and_then(|o| o.as_str().and_then(|o| o.rsplit_once('#')).map(|(_, n)| n.to_string()));
            let placement = r.field("placement").and_then(|m| serde_json::from_value::<super::design::Mat4>(m).ok());
            if let (Some(object), Some(m)) = (object, placement)
                && m.iter().flatten().all(|v| v.is_finite())
            {
                arenas.placements.insert(object, m);
            }
        }
        // The origin axes patterns name: an `App::Line` runs along its placement's first column.
        let native = ir.native.namespace("fcstd").map(|n| n.arenas());
        let object_name = |v: &serde_json::Value| v.as_str().and_then(|o| o.rsplit_once('#')).map(|(_, n)| n.to_string());
        let lines: std::collections::HashSet<String> = native
            .and_then(|a| a.get("objects"))
            .into_iter()
            .flatten()
            .filter(|r| r.field("type_name").is_some_and(|t| t == "App::Line"))
            .filter_map(|r| r.field("name").and_then(|n| n.as_str().map(str::to_string)))
            .collect();
        for r in native.and_then(|a| a.get("properties")).into_iter().flatten() {
            if !r.field("name").is_some_and(|n| n == "Direction" || n == "Axis") {
                continue;
            }
            let (Some(owner), Some(links)) = (r.field("owner").as_ref().and_then(object_name), r.field("links")) else { continue };
            let Some([link]) = links.as_array().map(Vec::as_slice) else { continue };
            let whole = link.get("subelements").and_then(|s| s.as_array()).is_none_or(|s| s.iter().all(|e| e.as_str().is_none_or(str::is_empty)));
            let Some(line) = link.get("object").and_then(object_name).filter(|l| whole && lines.contains(l)) else { continue };
            if let Some(m) = arenas.placements.get(&line) {
                let at = solidcraft_geom::Vec3::new(m[0][3], m[1][3], m[2][3]);
                let along = solidcraft_geom::Vec3::new(m[0][0], m[1][0], m[2][0]);
                arenas.pattern_axes.insert(owner, (at, along));
            }
        }
        let index = std::cell::OnceCell::new();
        let work = WorkBudget::new(EDGE_WORK);
        let edge_point = |id: &str| edge_point(&ir, index.get_or_init(|| ModelIndex::build(&ir)), &work, id);
        super::design::rebuild(&arenas, &edge_point)
    }

    /// Mark hidden what FreeCAD shows only through its body: the shapes of the features inside a
    /// PartDesign body that has a shape of its own (a file saved from FreeCAD's window keeps the
    /// tip feature's shape visible beside the body's identical one), and binders, which are
    /// reference copies of other geometry. The STEP writer marks them invisible, and the import
    /// leaves them out.
    fn hide_body_members(ir: &mut CadIr) {
        use serde_json::Value;
        let Ok(Value::Array(features)) = serde_json::to_value(&ir.model.features) else { return };
        let operation = |f: &Value| {
            let d = f.get("definition")?;
            if d.get("definition").and_then(Value::as_str) == Some("post_process") { d.get("operation").cloned() } else { Some(d.clone()) }
        };
        let outputs = |f: &Value| -> Vec<String> {
            f.get("outputs").and_then(Value::as_array).into_iter().flatten().filter_map(|o| o.as_str().map(str::to_string)).collect()
        };
        let mut members = std::collections::HashSet::new();
        for f in &features {
            let Some(d) = operation(f) else { continue };
            let body = d.get("role").and_then(Value::as_str) == Some("solid_bodies") && !outputs(f).is_empty();
            if d.get("definition").and_then(Value::as_str) == Some("tree_node") && body {
                members.extend(
                    d.pointer("/children/children").and_then(Value::as_array).into_iter().flatten().filter_map(|c| c.as_str().map(str::to_string)),
                );
            }
        }
        let mut hidden = std::collections::HashSet::new();
        for f in &features {
            let member = f.get("id").and_then(Value::as_str).is_some_and(|id| members.contains(id));
            let binder = operation(f).is_some_and(|d| d.get("definition").and_then(Value::as_str) == Some("binder"));
            if member || binder {
                hidden.extend(outputs(f));
            }
        }
        for body in ir.model.bodies.iter_mut().filter(|b| hidden.contains(b.id.as_str())) {
            body.visible = Some(false);
        }
    }

    /// Curve evaluation steps spent placing fillet and chamfer edges, at most.
    const EDGE_WORK: usize = 1 << 22;

    /// A point on model edge `id`: its carrier at the middle of its range.
    fn edge_point(ir: &CadIr, index: &ModelIndex<'_>, work: &WorkBudget, id: &str) -> Option<solidcraft_geom::Vec3> {
        let edge = ir.model.edges.iter().find(|e| e.id.as_str() == id)?;
        let curve = edge.carrier.curve()?;
        let [a, b] = edge.param_range()?.get();
        let t = 0.5 * (a + b);
        let p =
            finite_or_refusal(EvaluationAdmission::Standard.within_work_slice(work, |adm| model_curve_point_by_id(adm, index, curve, t))).ok()??;
        Some(solidcraft_geom::Vec3::new(p.x, p.y, p.z))
    }

    pub(super) fn to_step(name: &str, bytes: &[u8]) -> Result<(String, Vec<String>), String> {
        let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        if !super::NATIVE_CAD_EXTS.contains(&ext.as_str()) {
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
        Ok((super::name_unnamed_products(step, name), warnings))
    }
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

#[cfg(test)]
mod tests;
