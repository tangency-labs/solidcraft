//! Native CAD import for FreeCAD `.FCStd`, Rhino `.3dm`, and supported SolidWorks
//! `.sldprt` parts and `.sldasm` assemblies. Imports produce STEP text, which the
//! Import feature embeds and reads through the existing STEP pipeline.
//!
//! cadmpeg handles the enabled open formats and the validated analytic part subset.
//! Assemblies use the restricted saved-Parasolid reader and source-preserving STEP
//! packer. Other commercial codecs and ShapeManager/ACIS remain disabled.
//!
//! FreeCAD and Rhino are available in browser builds too. SolidWorks import is
//! native-only because its saved-body reader and assembly references use native inputs.

/// File extensions handled by native CAD importers (lower case).
pub const NATIVE_CAD_EXTS: [&str; 4] = ["fcstd", "3dm", "sldprt", "sldasm"];

#[cfg(not(target_arch = "wasm32"))]
pub mod assembly;
#[cfg(not(target_arch = "wasm32"))]
mod parasolid;
#[cfg(not(target_arch = "wasm32"))]
mod selection;
#[cfg(not(target_arch = "wasm32"))]
mod sldprt;

/// Is this a supported native CAD file name (any case)?
pub fn is_native_cad_path(path: &str) -> bool {
    std::path::Path::new(path).extension().and_then(|e| e.to_str()).is_some_and(|e| NATIVE_CAD_EXTS.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// Restate a supported native CAD file (`file` is its name) as STEP text, with the losses
/// and warnings reported by the importer. Third-party panics become errors.
pub fn native_cad_to_step(bytes: &[u8], file: &str) -> Result<(String, Vec<String>), String> {
    #[cfg(target_arch = "wasm32")]
    if std::path::Path::new(file)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("sldprt") || e.eq_ignore_ascii_case("sldasm"))
    {
        return Err(format!("{file}: SolidWorks import is not supported in the browser yet (open it in the desktop app)"));
    }
    #[cfg(not(target_arch = "wasm32"))]
    if std::path::Path::new(file).extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("sldasm")) {
        return solidcraft_kernel::guard("SLDASM import", || assembly::from_files(bytes, file).map_err(solidcraft_kernel::KernelError::Invalid))
            .map_err(|e| e.to_string());
    }
    let name = std::path::Path::new(file).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| file.to_string());
    solidcraft_kernel::guard("cadmpeg import", || convert::to_step(&name, bytes).map_err(solidcraft_kernel::KernelError::Invalid)).map_err(
        |e| match e {
            solidcraft_kernel::KernelError::Invalid(m) => m,
            e => e.to_string(),
        },
    )
}

mod convert {
    use std::io::Cursor;

    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodeMode, DecodePolicy, ResourceLimits, View};
    use cadmpeg_ir::codec::write::{EncodeInput, target::TargetRequest};
    use cadmpeg_ir::codec::{DecodeFailure, DecodeOptions, DecodeResult};
    use cadmpeg_ir::validate::validate_neutral_for_decode;
    use cadmpeg_registry::{DETECTION_PREFIX_LEN, Format, InputCatalog, ResolveSourceError, ResolvedSource, build_encoder, forced_input};

    /// The codecs we accept (cadmpeg codec ids), whatever else the registry recognises.
    #[cfg(target_arch = "wasm32")]
    const ACCEPTED: [&str; 2] = ["fcstd", "rhino"];
    #[cfg(not(target_arch = "wasm32"))]
    const ACCEPTED: [&str; 3] = ["fcstd", "rhino", "sldprt"];

    /// Caps for hostile input: cadmpeg's tight `service` profile, with our STEP size limit.
    pub(super) fn policy() -> DecodePolicy {
        DecodePolicy {
            mode: DecodeMode::Salvage,
            // In 0.6.3 the service profile admits eight million entities, retaining the
            // decompression, retained-byte and recursion caps for detailed parts/assemblies.
            limits: ResourceLimits { max_input_bytes: crate::MAX_STEP_BYTES as u64, ..ResourceLimits::service() },
        }
    }

    /// Decode `bytes` (a file called `name`, extension `ext`) with the codec cadmpeg detects from
    /// the bytes, else the one the extension names, if it is one of `accepted`. Returns the codec id
    /// and the decoded model.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn decode(name: &str, ext: &str, bytes: &[u8], accepted: &[&str]) -> Result<(String, DecodeResult), String> {
        decode_at_depth(name, ext, bytes, accepted, DecodeDepth::Full)
    }

    fn decode_at_depth(name: &str, ext: &str, bytes: &[u8], accepted: &[&str], depth: DecodeDepth) -> Result<(String, DecodeResult), String> {
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
            return Err(format!("{name}: {format} files are not supported (FreeCAD .FCStd, Rhino .3dm and supported SolidWorks .sldprt parts)"));
        }

        // Decode in memory under the caps.
        let options = DecodeOptions { depth, policy };
        let decoded = codec.decode(&mut Cursor::new(bytes), &options).map_err(|e| match e {
            DecodeFailure::Codec(e) => format!("{name}: could not read this {format} file: {e}"),
            other => format!("{name}: {other}"),
        })?;
        Ok((format, decoded))
    }

    pub(super) fn to_step(name: &str, bytes: &[u8]) -> Result<(String, Vec<String>), String> {
        to_step_inner(name, bytes)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn to_step_configuration(name: &str, bytes: &[u8], configuration: &str) -> Result<(String, Vec<String>), String> {
        let selected = super::selection::select(bytes, configuration)?;
        let (step, mut warnings) = super::parasolid::part(bytes, name, &selected)?;
        warnings.push(format!(
            "SLDPRT: imported stored configuration {configuration:?} (source index {}); feature history is not imported",
            selected.source_index
        ));
        Ok((step, warnings))
    }

    fn to_step_inner(name: &str, bytes: &[u8]) -> Result<(String, Vec<String>), String> {
        let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        if !super::NATIVE_CAD_EXTS.contains(&ext.as_str()) {
            return Err(format!("{name}: not supported (FreeCAD .FCStd, Rhino .3dm and supported SolidWorks .sldprt parts)"));
        }
        let depth = if ext == "sldprt" { DecodeDepth::Full } else { DecodeDepth::Geometry };
        let (format, decoded) = decode_at_depth(name, &ext, bytes, &ACCEPTED, depth)?;
        if ext == "sldprt" && format != "sldprt" {
            return Err(format!("{name}: the contents are not an SLDPRT part"));
        }
        let policy = policy();
        let (ir, report, fidelity) = decoded.into_parts();
        let mut warnings: Vec<String> = report.losses.iter().map(|l| format!("{}: {}", l.code, l.message)).collect();
        #[cfg(not(target_arch = "wasm32"))]
        let (ir, report, fidelity, reference) = {
            let (mut ir, mut report, mut fidelity) = (ir, report, fidelity);
            let reference = if format == "sldprt" {
                let reference = super::sldprt::reference_points(&ir)?;
                let reference_volume = super::sldprt::reference_volume(&ir)?;

                if let Some(cached) = super::sldprt::cached_part(&ir, &fidelity)? {
                    let (_, recovered) = decode(name, "sldprt", &cached.input, &["sldprt"])?;
                    (ir, report, fidelity) = recovered.into_parts();
                    let painted = super::sldprt::restore_colors(&mut ir, &cached.geometry)?;
                    if painted > 0 {
                        warnings.push(format!("SLDPRT: restored source base colors on {painted} cached faces"));
                    }
                    warnings
                        .push("SLDPRT: restored the active imported part from its matching local body cache; feature history is not imported".into());
                }
                super::sldprt::normalize_cached(&mut ir, &reference)?;
                Some((reference, reference_volume))
            } else {
                None
            };
            (ir, report, fidelity, reference)
        };
        if !report.geometry_transferred() {
            return Err(format!("{name}: no geometry could be read from this {format} file"));
        }
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
        #[cfg(not(target_arch = "wasm32"))]
        if let Some((reference, reference_volume)) = reference {
            super::sldprt::validate_step(&step, &reference, reference_volume)?;
        }
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
