//! Other CAD formats read through [cadmpeg](https://github.com/cadmpeg/cadmpeg): FreeCAD
//! `.FCStd` and Rhino `.3dm`. cadmpeg decodes the file into its neutral model (CADIR). By default
//! cadmpeg's STEP writer restates that as STEP, which the Import feature then reads and keeps like
//! any STEP file ([`native_cad_to_step`]); with [`crate::StepReader::Cadmpeg`] (the Preferences
//! switch "Import with cadmpeg", off by default) the model is read straight into B-rep records
//! ([`cadir_records`]) and the Import feature keeps those records, with no STEP text in between.
//!
//! Only codecs for open formats are compiled in (the workspace enables cadmpeg-registry's
//! `fcstd`, `rhino` and `step` features alone). cadmpeg's codecs for commercial formats, and its
//! `sat` codec (a ShapeManager stream decoder that reads ACIS as one dialect), are not built.
//!
//! This module is the importer seam: the one place cadmpeg is called, returning the STEP text or
//! the records plus warnings. A cadmpeg-based STEP reader could be added here later and chosen by a setting, to
//! compare with our own STEP reader, without changing that reader.

/// File extensions read through cadmpeg (lower case).
pub const NATIVE_CAD_EXTS: [&str; 2] = ["fcstd", "3dm"];

/// Is this a FreeCAD or Rhino file name (any case)?
pub fn is_native_cad_path(path: &str) -> bool {
    std::path::Path::new(path).extension().and_then(|e| e.to_str()).is_some_and(|e| NATIVE_CAD_EXTS.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// Is this a FreeCAD file name (any case)?
pub fn is_freecad_path(path: &str) -> bool {
    std::path::Path::new(path).extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("fcstd"))
}

/// Whether `bytes` are a FreeCAD design, from the contents alone: the file's name is not used
/// (a Rhino file can be called `.FCStd`, and a FreeCAD file `.3dm`). Panics inside cadmpeg
/// become `false`.
pub fn detects_freecad(bytes: &[u8]) -> bool {
    matches!(solidcraft_kernel::guard("cadmpeg detect", || Ok(convert::contents_are_freecad(bytes))), Ok(true))
}

/// Read a FreeCAD or Rhino file (`file` is its name) into an import kept as B-rep records,
/// with what cadmpeg could not bring over, and whether its contents are a FreeCAD design. Panics
/// inside cadmpeg or the reader become errors.
pub fn native_cad_records(bytes: &[u8], file: &str) -> Result<(solidcraft_kernel::ImportRecords, Vec<String>, bool), String> {
    let name = std::path::Path::new(file).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| file.to_string());
    solidcraft_kernel::guard("cadmpeg import", || convert::records(&name, bytes).map_err(solidcraft_kernel::KernelError::Invalid)).map_err(
        |e| match e {
            solidcraft_kernel::KernelError::Invalid(m) => m,
            e => e.to_string(),
        },
    )
}

/// Read a STEP file (`file` is its name) with cadmpeg's STEP decoder into an import kept as
/// B-rep records, as FreeCAD and Rhino files are read: an alternative to SolidCraft's own STEP
/// reader, chosen by the caller ([`crate::StepReader`]), to compare the two. Panics inside cadmpeg
/// or the reader become errors.
pub fn cadmpeg_step_records(bytes: &[u8], file: &str) -> Result<(solidcraft_kernel::ImportRecords, Vec<String>, bool), String> {
    let name = std::path::Path::new(file).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| file.to_string());
    solidcraft_kernel::guard("cadmpeg STEP import", || convert::step_records(&name, bytes).map_err(solidcraft_kernel::KernelError::Invalid)).map_err(
        |e| match e {
            solidcraft_kernel::KernelError::Invalid(m) => m,
            e => e.to_string(),
        },
    )
}

/// A FreeCAD file's design history rebuilt as features (sketches, Pad and Pocket, Revolution and
/// Groove, Fillet and Chamfer), checked against the file's own solids: each rebuilt body has the
/// volume and extent of one of them. The error says why the history was not rebuilt; the file's
/// solids then import as before ([`native_cad_to_step`], or [`native_cad_records`] with the
/// cadmpeg reader). Rhino files carry no such history.
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

/// `ir`'s bodies as an import kept as records, written by `system`: each body region assembled
/// into a record by `solidcore_cadir`, its pcurves without a closed form made as a STEP import's
/// are ([`solidcraft_kernel::import_trims`]). Nothing passes through STEP text. CADIR's bodies
/// come with their own names, visibility and rigid placements, which the records keep. Where the
/// file has a product structure (occurrences), the tree is that structure and each body is placed
/// once for each use of its part ([`assembly`]); otherwise the bodies are one product called
/// `product`. What could not be read (and what was repaired on the way) is in the warnings, each
/// naming its body.
pub(crate) fn cadir_records(ir: &cadmpeg_ir::document::CadIr, product: &str, system: &str) -> solidcraft_kernel::ImportRecords {
    use solidcraft_kernel::{ImportRecords, RecordBody, StepNode};
    let _span = solidcraft_profile::span("import", "cadir.records");
    let read = solidcore_cadir::read_bodies(ir, Some(solidcraft_kernel::import_trims()));
    let by_id: std::collections::HashMap<&str, &cadmpeg_ir::topology::Body> = ir.model.bodies.iter().map(|b| (b.id.as_str(), b)).collect();
    let label = |id: &str| by_id.get(id).and_then(|b| b.name.clone()).filter(|n| !n.trim().is_empty()).unwrap_or_else(|| id.to_string());
    let mut warnings = Vec::new();
    let mut unplaced = Vec::with_capacity(read.read.len());
    for r in read.read {
        let body = by_id.get(r.body.as_str());
        warnings.extend(r.solid.notes.iter().map(|n| format!("{}: {n}", label(&r.body))));
        let record = RecordBody {
            name: body.and_then(|b| b.name.clone()).unwrap_or_default(),
            path: vec![product.to_string()],
            instance: None,
            // Not yet: a Rhino object's colour is often its layer's default (black), which is not
            // how Rhino shades it; the STEP route brought no colour over either.
            color: None,
            hidden: body.is_some_and(|b| b.visible == Some(false)),
            placement: r.placement,
            brep: Some(r.solid.brep),
            face_origin: r.solid.face_origin,
            sheet: r.solid.sheet,
            copy_of: None,
        };
        unplaced.push(assembly::Unplaced { body: r.body, record });
    }
    warnings.extend(read.failures.iter().map(|f| format!("{}: not imported: {}", label(&f.body), f.message)));
    let (bodies, tree) = if ir.model.occurrences.is_empty() {
        let bodies: Vec<RecordBody> = unplaced.into_iter().map(|u| u.record).collect();
        let identity = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        let tree = vec![StepNode {
            name: product.to_string(),
            transform: identity,
            bodies: (0..bodies.len()).collect(),
            items: Vec::new(),
            left_out: Vec::new(),
            children: Vec::new(),
        }];
        (bodies, tree)
    } else {
        assembly::placed(ir, product, unplaced, &mut warnings)
    };
    ImportRecords { bodies, tree, warnings, system: system.to_string() }
}

mod assembly;
mod design;
pub use design::NativeDesign;

pub(crate) use convert::step_route::hidden_items;
pub use convert::step_route::native_cad_to_step;

mod convert {
    use std::io::Cursor;

    use cadmpeg_core::decode::WorkBudget;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodeMode, DecodePolicy, ResourceLimits, View};
    use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions, DecodeResult};
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::eval::{admission::EvaluationAdmission, finite_or_refusal, model_curve_point_by_id};
    use cadmpeg_ir::index::ModelIndex;
    use cadmpeg_ir::validate::validate_neutral_for_decode;
    use cadmpeg_registry::{DETECTION_PREFIX_LEN, InputCatalog, ResolveSourceError, ResolvedSource, forced_input};

    /// The codecs we accept (cadmpeg codec ids), whatever else the registry recognises.
    const ACCEPTED: [&str; 2] = ["fcstd", "rhino"];

    pub(super) mod step_route;

    /// Caps for hostile input: cadmpeg's tight `service` profile, with our STEP size limit on
    /// the input. The service profile already admits a detailed part's 8 million entities:
    /// cadmpeg 0.6.3 dropped the collection-item budget that refused one at a million (#1783)
    /// and the work-unit budget that went with it.
    pub(super) fn policy() -> DecodePolicy {
        DecodePolicy {
            mode: DecodeMode::Salvage,
            limits: ResourceLimits { max_input_bytes: crate::limits::Kind::Step.limit(), ..ResourceLimits::service() },
        }
    }

    /// The native codec that reads `bytes`: the format the contents state, else the one `ext`
    /// names when the contents are not recognised (an empty `ext` asks the contents alone).
    /// `None` when neither names a native codec.
    fn detect<'a>(catalog: &'a InputCatalog, policy: &DecodePolicy, ext: &str, bytes: &[u8]) -> Result<Option<&'a dyn Codec>, String> {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy).map_err(|e| e.to_string())?;
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
        match resolved.map_err(|e| e.to_string())? {
            ResolvedSource::Native { codec, .. } => Ok(Some(codec)),
            _ => Ok(None),
        }
    }

    /// Whether `bytes` are a FreeCAD design, from the contents alone (no extension is consulted).
    pub(super) fn contents_are_freecad(bytes: &[u8]) -> bool {
        let catalog = InputCatalog::with_builtins();
        let policy = policy();
        matches!(detect(&catalog, &policy, "", bytes), Ok(Some(codec)) if codec.id().as_str() == "fcstd")
    }

    /// Decode `bytes` (a file called `name`, extension `ext`) with the codec cadmpeg detects from
    /// the bytes, else the one the extension names, if it is one of `accepted`. Returns the codec id
    /// and the decoded model.
    pub(super) fn decode_at(name: &str, ext: &str, bytes: &[u8], accepted: &[&str], depth: DecodeDepth) -> Result<(String, DecodeResult), String> {
        let policy = policy();
        let catalog = InputCatalog::with_builtins();
        let codec = detect(&catalog, &policy, ext, bytes)
            .map_err(|e| format!("{name}: {e}"))?
            .ok_or_else(|| format!("{name}: unrecognised file contents"))?;
        let format = codec.id().as_str().to_string();
        if !accepted.contains(&format.as_str()) {
            return Err(format!("{name}: {format} files are not supported (FreeCAD .FCStd and Rhino .3dm only)"));
        }

        // Decode in memory under the caps, at the depth the caller chose: the records read the
        // bodies; a FreeCAD history rebuild reads the design too.
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
                let at = solidcore_record::Vec3::new(m[0][3], m[1][3], m[2][3]);
                let along = solidcore_record::Vec3::new(m[0][0], m[1][0], m[2][0]);
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
    /// reference copies of other geometry. The import leaves them out (on the STEP route the STEP
    /// writer marks them invisible).
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
    fn edge_point(ir: &CadIr, index: &ModelIndex<'_>, work: &WorkBudget, id: &str) -> Option<solidcore_record::Vec3> {
        let edge = ir.model.edges.iter().find(|e| e.id.as_str() == id)?;
        let curve = edge.carrier.curve()?;
        let [a, b] = edge.param_range()?.get();
        let t = 0.5 * (a + b);
        let p =
            finite_or_refusal(EvaluationAdmission::Standard.within_work_slice(work, |adm| model_curve_point_by_id(adm, index, curve, t))).ok()??;
        Some(solidcore_record::Vec3::new(p.x, p.y, p.z))
    }

    /// A FreeCAD or Rhino file (`name`) read into an import kept as records, with what cadmpeg
    /// could not bring over, and whether the contents are a FreeCAD design.
    pub(super) fn records(name: &str, bytes: &[u8]) -> Result<(solidcraft_kernel::ImportRecords, Vec<String>, bool), String> {
        let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        if !super::NATIVE_CAD_EXTS.contains(&ext.as_str()) {
            return Err(format!("{name}: not supported (FreeCAD .FCStd and Rhino .3dm only)"));
        }
        records_of(name, &ext, bytes, &ACCEPTED)
    }

    /// A STEP file (`name`) read by cadmpeg's STEP decoder into an import kept as records.
    pub(super) fn step_records(name: &str, bytes: &[u8]) -> Result<(solidcraft_kernel::ImportRecords, Vec<String>, bool), String> {
        let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        if !matches!(ext.as_str(), "step" | "stp") {
            return Err(format!("{name}: not a STEP file (.step or .stp)"));
        }
        records_of(name, &ext, bytes, &["step"])
    }

    /// `bytes` decoded by the cadmpeg codec they are, if it is one of `accepted`, and read into
    /// records.
    fn records_of(name: &str, ext: &str, bytes: &[u8], accepted: &[&str]) -> Result<(solidcraft_kernel::ImportRecords, Vec<String>, bool), String> {
        let (format, decoded) = decode_at(name, ext, bytes, accepted, DecodeDepth::Full)?;
        let policy = policy();
        let (mut ir, report, _) = decoded.into_parts();
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

        let product = std::path::Path::new(name).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| name.to_string());
        let system = match format.as_str() {
            "fcstd" => "FreeCAD",
            "rhino" => "Rhino",
            "step" => "",
            other => other,
        };
        let records = super::cadir_records(&ir, &product, system);
        if records.bodies.is_empty() {
            let why = records.warnings.first().map(|w| format!(" ({w})")).unwrap_or_default();
            return Err(format!("{name}: this {format} file has no solids that can be imported{why}"));
        }
        Ok((records, warnings, format == "fcstd"))
    }
}

#[cfg(test)]
mod tests;
