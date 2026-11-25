//! Other CAD formats read through [cadmpeg](https://github.com/cadmpeg/cadmpeg): FreeCAD
//! `.FCStd` files are read, and Rhino `.3dm` and OpenUSD files read and written
//! ([`usd`], [`cadmpeg_export`]); STEP and IGES files are read when the cadmpeg reader is
//! chosen ([`cadmpeg_step_records`], [`cadmpeg_iges_records`]) and written through it. cadmpeg decodes the file into its neutral model (CADIR). By default
//! cadmpeg's STEP writer restates that as STEP, which the Import feature then reads and keeps like
//! any STEP file ([`native_cad_to_step`]); with [`crate::StepReader::Cadmpeg`] (the Preferences
//! switch "Import with cadmpeg", off by default) the model is read straight into B-rep records
//! ([`cadir_records`]) and the Import feature keeps those records, with no STEP text in between.
//!
//! Only codecs for open formats are compiled in (the workspace enables cadmpeg-registry's
//! `fcstd`, `iges`, `rhino`, `step` and `usd` features alone). cadmpeg's codecs for commercial formats, and its
//! `sat` codec (a ShapeManager stream decoder that reads ACIS as one dialect), are not built.
//!
//! This module is the cadmpeg seam: the one place cadmpeg is called, returning the STEP text, the
//! records or the exported bytes plus warnings; an export writes the design into cadmpeg's model
//! directly (`cadir_out`) and cadmpeg's writer writes the file (`export`). A cadmpeg-based STEP reader could be added here later and chosen by a setting, to
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

/// A file read by cadmpeg into an import kept as B-rep records.
#[derive(Clone, Debug)]
pub struct RecordsRead {
    /// The records; their `system` is the program that wrote the file, named as SolidCraft's own
    /// reader names it: a STEP header's originating system, an IGES file's native system ID.
    pub records: solidcraft_kernel::ImportRecords,
    /// What cadmpeg could not bring over.
    pub warnings: Vec<String>,
    /// Whether the contents are a FreeCAD design.
    pub freecad: bool,
    /// A STEP file's FILE_NAME preprocessor version and FILE_DESCRIPTION text, as SolidCraft's
    /// own reader reads them (with the program, they say whether the file has Y up); empty for
    /// other files.
    pub preprocessor: String,
    pub description: String,
}

/// The program that wrote a file, as its header names it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Producer {
    system: String,
    preprocessor: String,
    description: String,
}

/// Bytes at the start of a STEP file read for its header, at most.
const STEP_HEADER_BYTES: usize = 256 << 10;
/// `ENDSEC` keywords tried as the end of a STEP file's header, at most.
const STEP_HEADER_ENDS: usize = 64;

/// A STEP file's FILE_NAME originating system and preprocessor version and its FILE_DESCRIPTION,
/// read by SolidCraft's own STEP parser from the file's HEADER section alone, so they are exactly
/// what a read of the whole file with SolidCraft's reader gives. cadmpeg's STEP decoder (0.6.12)
/// checks the header and keeps none of it. The header ends at an `ENDSEC`; one inside a string or
/// comment is not it, so each is tried in turn until the header parses. Empty when the start of
/// the file holds no header that parses.
fn step_producer(bytes: &[u8]) -> Producer {
    let head = match bytes.get(..STEP_HEADER_BYTES) {
        // The whole prefix, a header followed by a long line included; only a UTF-8 character
        // the limit splits is left out, so the prefix decodes as the whole file does.
        Some(head) => match std::str::from_utf8(head) {
            Err(e) if e.error_len().is_none() => head.get(..e.valid_up_to()).unwrap_or(head),
            _ => head,
        },
        None => bytes,
    };
    let text = crate::step_text(head);
    let mut from = 0;
    for _ in 0..STEP_HEADER_ENDS {
        let Some(end) = next_end_section(&text, from) else { break };
        let header = format!("{};\nEND-ISO-10303-21;\n", text.get(..end).unwrap_or_default());
        if let Ok(ex) = solidcraft_kernel::services::step::p21::parse(&header) {
            return Producer { system: ex.originating_system, preprocessor: ex.preprocessor, description: ex.description };
        }
        from = end;
    }
    Producer::default()
}

/// The offset just past the next `ENDSEC` keyword in `text` at or after `from` that is not
/// inside a quoted string or a comment, the keyword included. A header field's text or a comment
/// can hold the keyword, and one that ends nothing must not be tried as the header's end or use
/// up the tries.
fn next_end_section(text: &str, from: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let (mut at, mut quoted) = (from, false);
    while at < bytes.len() {
        match *bytes.get(at)? {
            // A quote is written doubled inside a string.
            b'\'' if quoted && bytes.get(at + 1) == Some(&b'\'') => at += 2,
            b'\'' => {
                quoted = !quoted;
                at += 1;
            }
            // A comment runs to its `*/` and may hold the keyword; one never closed leaves no
            // keyword after it.
            b'/' if !quoted && bytes.get(at + 1) == Some(&b'*') => {
                let comment = text.get(at + 2..)?;
                at = at + 2 + comment.find("*/")? + "*/".len();
            }
            b'E' if !quoted && text.get(at..).is_some_and(|rest| rest.starts_with("ENDSEC")) => return Some(at + "ENDSEC".len()),
            _ => at += 1,
        }
    }
    None
}

/// Read a FreeCAD or Rhino file (`file` is its name) into an import kept as B-rep records,
/// with what cadmpeg could not bring over, and whether its contents are a FreeCAD design. Panics
/// inside cadmpeg or the reader become errors.
pub fn native_cad_records(bytes: &[u8], file: &str) -> Result<RecordsRead, String> {
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
pub fn cadmpeg_step_records(bytes: &[u8], file: &str) -> Result<RecordsRead, String> {
    let name = std::path::Path::new(file).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| file.to_string());
    solidcraft_kernel::guard("cadmpeg STEP import", || convert::step_records(&name, bytes).map_err(solidcraft_kernel::KernelError::Invalid)).map_err(
        |e| match e {
            solidcraft_kernel::KernelError::Invalid(m) => m,
            e => e.to_string(),
        },
    )
}

/// Read an IGES file (`file` is its name) with cadmpeg's IGES decoder into an import kept as
/// B-rep records, as a STEP file is read with [`cadmpeg_step_records`]: the alternative to
/// SolidCraft's own IGES reader, chosen by the caller ([`crate::StepReader`]). Panics inside
/// cadmpeg or the reader become errors.
pub fn cadmpeg_iges_records(bytes: &[u8], file: &str) -> Result<RecordsRead, String> {
    let name = std::path::Path::new(file).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| file.to_string());
    solidcraft_kernel::guard("cadmpeg IGES import", || convert::iges_records(&name, bytes).map_err(solidcraft_kernel::KernelError::Invalid)).map_err(
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

/// [`traced_records`]'s records alone.
#[cfg(test)]
pub(crate) fn cadir_records(ir: &cadmpeg_ir::document::CadIr, product: &str, system: &str, colours: bool) -> solidcraft_kernel::ImportRecords {
    traced_records(ir, product, system, colours, false).0
}

/// `ir`'s bodies as an import kept as records, written by `system`: each body region assembled
/// into a record by `solidcore_cadir`, its pcurves without a closed form made as a STEP import's
/// are ([`solidcraft_kernel::import_trims`]). Nothing passes through STEP text. CADIR's bodies
/// come with their own names, visibility and rigid placements, which the records keep. Where the
/// file has a product structure (occurrences), the tree is that structure and each body is placed
/// once for each use of its part ([`assembly`]); otherwise the bodies are one product called
/// `product`. With `colours`, each body takes the colours the file gives it and its faces
/// ([`Looks::paint_of`]). What could not be read (and what was repaired on the way) is in the
/// warnings, each naming its body; with `shown`, a body whose B-rep cannot be read comes in as the
/// mesh the file shows it with, where it has one. Also returns where each record body came from ([`validation::Origin`]), for checking the file's own
/// figures ([`validation::check`]).
fn traced_records(
    ir: &cadmpeg_ir::document::CadIr,
    product: &str,
    system: &str,
    colours: bool,
    shown: bool,
) -> (solidcraft_kernel::ImportRecords, Vec<validation::Origin>) {
    use solidcraft_kernel::{ImportRecords, RecordBody, StepNode};
    let _span = solidcraft_profile::span("import", "cadir.records");
    let read = solidcore_cadir::read_bodies(ir, Some(solidcraft_kernel::import_trims()));
    let by_id: std::collections::HashMap<&str, &cadmpeg_ir::topology::Body> = ir.model.bodies.iter().map(|b| (b.id.as_str(), b)).collect();
    let label = |id: &str| by_id.get(id).and_then(|b| b.name.clone()).filter(|n| !n.trim().is_empty()).unwrap_or_else(|| id.to_string());
    let mut warnings = Vec::new();
    let mut unplaced = Vec::with_capacity(read.read.len());
    let looks = colours.then(|| Looks::of(ir));
    let layers = HiddenLayers::of(ir);
    // With `shown`, a B-rep that cannot be read comes in as the mesh the file shows it with (an
    // OpenUSD `BrepArray`'s fallback mesh, a tessellation of its body), rather than not at all,
    // and rather than as an open surface of the faces that read: the mesh is all of the body.
    let has_triangles =
        |id: &str| ir.model.tessellations.iter().any(|t| t.body.as_ref().is_some_and(|b| b.as_str() == id) && !t.triangles().is_empty());
    let mut opened: Vec<(String, String)> = Vec::new();
    for r in read.read {
        if shown
            && has_triangles(&r.body)
            && let Some(why) = r.opened
        {
            opened.push((r.body, why));
            continue;
        }
        let body = by_id.get(r.body.as_str());
        let (color, paint) = looks.as_ref().map_or((None, None), |l| l.paint_of(ir, &r.body, &r.solid.face_origin));
        warnings.extend(r.solid.notes.iter().map(|n| format!("{}: {n}", label(&r.body))));
        let record = RecordBody {
            name: body.and_then(|b| b.name.as_deref()).map(str::trim).unwrap_or_default().to_string(),
            path: vec![product.to_string()],
            instance: None,
            // Only where asked: a Rhino object's colour is often its layer's default (black),
            // which is not how Rhino shades it.
            color,
            paint,
            hidden: body.is_some_and(|b| b.visible == Some(false)) || layers.bodies.contains(r.body.as_str()),
            placement: r.placement,
            brep: Some(r.solid.brep),
            face_origin: r.solid.face_origin,
            sheet: r.solid.sheet,
            copy_of: None,
            mesh: None,
        };
        unplaced.push(assembly::Unplaced { body: r.body, record });
    }
    let mut fallen: std::collections::HashSet<&str> =
        if shown { read.failures.iter().map(|f| f.body.as_str()).filter(|id| has_triangles(id)).collect() } else { Default::default() };
    fallen.extend(opened.iter().map(|(id, _)| id.as_str()));
    warnings.extend(
        opened.iter().map(|(id, why)| format!("{}: its B-rep was not imported ({why}): it came in as the mesh the file shows it with", label(id))),
    );
    warnings.extend(read.failures.iter().map(|f| {
        if fallen.contains(f.body.as_str()) {
            format!("{}: its B-rep was not imported ({}): it came in as the mesh the file shows it with", label(&f.body), f.message)
        } else {
            format!("{}: not imported: {}", label(&f.body), f.message)
        }
    }));
    // Parts the file gives as triangles alone (AP242 tessellated parts): a mesh body each, placed
    // as any other part. A tessellation of a body that has a B-rep is how the file shows that
    // body, not a part of its own: it is read only in place of a B-rep that could not be.
    for t in &ir.model.tessellations {
        // A tessellation the model ties to no body (cadmpeg reads a part given as triangles alone
        // so) is a part of its own, where the file defines it, under the root of bodies no
        // occurrence reaches; in an assembly that is not where the assembly puts it, which the
        // warning says.
        let (id, body) = match t.body.as_ref().map(|b| b.as_str()) {
            Some(id) => match by_id.get(id) {
                Some(body) if body.regions.is_empty() || fallen.contains(id) => (id, Some(*body)),
                _ => continue,
            },
            None => (t.id.as_str(), None),
        };
        let name = body
            .and_then(|b| b.name.clone())
            .or_else(|| t.source_object.as_ref().and_then(|s| s.name.clone()))
            .unwrap_or_default()
            .trim()
            .to_string();
        if body.is_none() && !ir.model.occurrences.is_empty() {
            let named = if name.trim().is_empty() { format!("triangles {id}") } else { name.clone() };
            warnings.push(format!("{named}: a part given as triangles comes in where the file defines it, not placed in the assembly: the file's reader does not say which part it is"));
        }
        let placement = match body.and_then(|b| b.transform) {
            None => None,
            Some(m) if m.is_proper_rigid() => Some(assembly::column_major(&m)),
            Some(_) => {
                warnings.push(format!("{}: not imported: not supported yet: a placement that mirrors or scales", label(id)));
                continue;
            }
        };
        let (color, paint) = looks.as_ref().map_or((None, None), |l| l.paint_of(ir, id, &[]));
        let record = RecordBody {
            name,
            path: vec![product.to_string()],
            instance: None,
            color,
            paint,
            // On a hidden layer: its body, or the tessellation itself (a layer names it by its id).
            hidden: body.is_some_and(|b| b.visible == Some(false)) || layers.bodies.contains(id) || layers.bodies.contains(t.id.as_str()),
            placement,
            brep: None,
            face_origin: Vec::new(),
            sheet: false,
            copy_of: None,
            mesh: Some(solidcraft_kernel::RecordMesh {
                positions: t.vertices().iter().map(|p| p.coordinates().map(|c| c.get())).collect(),
                triangles: t.triangles(),
            }),
        };
        unplaced.push(assembly::Unplaced { body: id.to_string(), record });
    }
    let (bodies, tree, origins) = if ir.model.occurrences.is_empty() {
        let identity = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        let origins = unplaced.iter().map(|u| validation::Origin { body: u.body.clone(), world: identity }).collect();
        let bodies: Vec<RecordBody> = unplaced.into_iter().map(|u| u.record).collect();
        let tree = vec![StepNode {
            name: product.to_string(),
            transform: identity,
            bodies: (0..bodies.len()).collect(),
            items: Vec::new(),
            left_out: Vec::new(),
            children: Vec::new(),
        }];
        (bodies, tree, origins)
    } else {
        assembly::placed(ir, product, unplaced, &layers, &mut warnings)
    };
    (ImportRecords { bodies, tree, warnings, system: system.to_string() }, origins)
}

/// `records` with each B-rep the kernel cannot build replaced by the mesh the file shows it with,
/// where it has one: an OpenUSD `BrepArray`'s fallback mesh, which cadmpeg makes a tessellation of
/// the B-rep's body. The body then comes in as a mesh body, with a warning saying why, rather
/// than not at all, as the mesh import would have brought it. A B-rep that builds keeps its
/// record and its mesh is not read. `origins` says which neutral-model body each record is.
fn meshes_for_refused(
    ir: &cadmpeg_ir::document::CadIr,
    mut records: solidcraft_kernel::ImportRecords,
    origins: &[validation::Origin],
) -> solidcraft_kernel::ImportRecords {
    let Ok(imp) = solidcraft_kernel::records_import_shared(&records) else { return records };
    let refused: std::collections::BTreeMap<usize, String> = imp
        .report
        .issues
        .iter()
        .filter(|i| i.kind == solidcraft_kernel::IssueKind::NotImported)
        .filter_map(|i| Some((usize::try_from(i.item?).ok()?, i.detail.clone())))
        .collect();
    for (k, why) in refused {
        let (Some(r), Some(origin)) = (records.bodies.get_mut(k), origins.get(k)) else { continue };
        if r.copy_of.is_some() || r.brep.is_none() {
            continue;
        }
        let Some(t) = ir.model.tessellations.iter().find(|t| t.body.as_ref().is_some_and(|b| b.as_str() == origin.body)) else { continue };
        let triangles = t.triangles();
        if triangles.is_empty() {
            continue;
        }
        r.mesh =
            Some(solidcraft_kernel::RecordMesh { positions: t.vertices().iter().map(|p| p.coordinates().map(|c| c.get())).collect(), triangles });
        r.brep = None;
        r.face_origin = Vec::new();
        r.sheet = false;
        r.paint = r.paint.take().map(|p| solidcraft_kernel::Paint { faces: Vec::new(), ..p });
        records.warnings.push(format!("{why}; it came in as the mesh the file shows it with"));
    }
    records
}

mod assembly;
mod validation;

/// What the file's hidden presentation layers hide (CADIR ids): bodies (with the bodies of the
/// products on them, and the parts given as triangles on them), and the occurrences and products
/// whose placements, with all they hold, start hidden.
#[derive(Default)]
pub(crate) struct HiddenLayers {
    pub bodies: std::collections::HashSet<String>,
    pub occurrences: std::collections::HashSet<String>,
    pub definitions: std::collections::HashSet<String>,
}

impl HiddenLayers {
    fn of(ir: &cadmpeg_ir::document::CadIr) -> Self {
        use cadmpeg_ir::presentation::PresentationItem;
        let mut out = HiddenLayers::default();
        for layer in ir.model.presentation_layers.iter().filter(|l| l.visible == Some(false)) {
            for item in &layer.items {
                match item {
                    PresentationItem::Body { body } => {
                        out.bodies.insert(body.as_str().to_string());
                    }
                    PresentationItem::Tessellation { tessellation } => {
                        out.bodies.insert(tessellation.clone());
                    }
                    PresentationItem::Occurrence { occurrence } => {
                        out.occurrences.insert(occurrence.as_str().to_string());
                    }
                    PresentationItem::Product { product } => {
                        out.definitions.insert(product.as_str().to_string());
                        if let Some(d) = ir.model.product_definitions.iter().find(|d| d.id == *product) {
                            out.bodies.extend(d.bodies.iter().map(|b| b.as_str().to_string()));
                        }
                    }
                    _ => {}
                }
            }
        }
        out
    }
}

/// A file's appearances by what they colour: whole bodies and single faces (CADIR ids).
struct Looks<'a> {
    bodies: std::collections::HashMap<&'a str, &'a cadmpeg_ir::appearance::Appearance>,
    faces: std::collections::HashMap<&'a str, &'a cadmpeg_ir::appearance::Appearance>,
}

impl<'a> Looks<'a> {
    fn of(ir: &'a cadmpeg_ir::document::CadIr) -> Self {
        use cadmpeg_ir::appearance::AppearanceTarget;
        let by_id: std::collections::HashMap<&str, &cadmpeg_ir::appearance::Appearance> =
            ir.model.appearances.iter().map(|a| (a.id.as_str(), a)).collect();
        let (mut bodies, mut faces) = (std::collections::HashMap::new(), std::collections::HashMap::new());
        // A look bound to a body's triangles (an OpenUSD mesh's material) is the body's, unless
        // the body has one of its own; triangles of no body are a part of their own, by their id.
        let shown: std::collections::HashMap<&str, &str> =
            ir.model.tessellations.iter().map(|t| (t.id.as_str(), t.body.as_ref().map_or(t.id.as_str(), |b| b.as_str()))).collect();
        let mut by_triangles = Vec::new();
        for b in &ir.model.appearance_bindings {
            let Some(look) = by_id.get(b.appearance.as_str()).copied().filter(|a| a.base_color.is_some()) else { continue };
            match &b.target {
                AppearanceTarget::Body(id) => {
                    bodies.insert(id.as_str(), look);
                }
                AppearanceTarget::Tessellation(id) => {
                    if let Some(body) = shown.get(id.as_str()) {
                        by_triangles.push((*body, look));
                    }
                }
                AppearanceTarget::Face(id) => {
                    faces.insert(id.as_str(), look);
                }
                _ => {}
            }
        }
        for (body, look) in by_triangles {
            bodies.entry(body).or_insert(look);
        }
        Looks { bodies, faces }
    }

    /// Body `body`'s colour and paint (its opacity, the colour's name, and each face of
    /// `face_origin`, indices into the model's faces, with a colour of its own), as a STEP import
    /// paints a solid: `None` for both where the file colours neither the body nor its faces.
    fn paint_of(&self, ir: &cadmpeg_ir::document::CadIr, body: &str, face_origin: &[u64]) -> (Option<[f32; 3]>, Option<solidcraft_kernel::Paint>) {
        let rgb = |a: &cadmpeg_ir::appearance::Appearance| a.base_color.map(|c| [c.r(), c.g(), c.b()]);
        let opacity = |a: &cadmpeg_ir::appearance::Appearance| a.base_color.map_or(1.0, |c| c.a());
        let named = |a: &cadmpeg_ir::appearance::Appearance| {
            a.name.clone().filter(|n| !n.trim().is_empty()).map(|n| n.chars().take(solidcraft_kernel::COLOUR_NAME_MAX).collect())
        };
        let own = self.bodies.get(body).copied();
        let faces: Vec<solidcraft_kernel::FacePaint> = face_origin
            .iter()
            .filter_map(|&f| {
                let face = ir.model.faces.get(usize::try_from(f).ok()?)?;
                let look = self.faces.get(face.id.as_str()).copied()?;
                Some(solidcraft_kernel::FacePaint { face: usize::try_from(f).ok()?, color: rgb(look)?, opacity: opacity(look), name: named(look) })
            })
            .collect();
        if own.is_none() && faces.is_empty() {
            return (None, None);
        }
        let color = own.and_then(rgb);
        (color, Some(solidcraft_kernel::Paint { opacity: own.map_or(1.0, opacity), faces, name: own.and_then(named) }))
    }
}
mod design;
pub use design::NativeDesign;

mod usd;
pub use usd::{USD_EXTS, is_usd_path};

mod cadir_out;
mod cadir_physics;
mod cadir_textures;
mod export;

/// What the design's view hides, for an export that writes it: bodies by their world name
/// (hidden themselves, by a folder or by a placement above them) and placements by their path
/// (hidden themselves).
#[derive(Clone, Debug, Default)]
pub struct Hidden {
    pub bodies: std::collections::BTreeSet<String>,
    pub placements: Vec<Vec<u64>>,
}

/// The choices of an export through cadmpeg beyond its bodies, file and quality.
#[derive(Clone, Debug, Default)]
pub struct CadmpegOptions {
    /// What the design's view hides, written hidden.
    pub hidden: Hidden,
    /// OpenUSD: the joints, grounding and each component's mass written as physics.
    pub physics: bool,
    /// OpenUSD: the image textures a render setup gives bodies, written into a `.usdz` package.
    pub surfaces: Vec<SurfacePlan>,
    /// OpenUSD: the pictures shown on the model (decals), written into a `.usdz` package.
    pub decals: Vec<DecalPlan>,
    /// OpenUSD: the optics a render setup gives bodies (glass).
    pub optics: Vec<OpticsPlan>,
    /// OpenUSD: each body as its mesh alone, without its exact shape (B-rep), for a file a
    /// viewer or a game engine shows rather than a CAD program edits.
    pub meshes_only: bool,
}

/// The optics a render setup gives one placement of a body: a dielectric (glass) of index `ior`
/// absorbing `absorption_per_mm` (red, green, blue, per mm) inside, its surface a thin sheet or
/// the boundary of a closed volume.
#[derive(Clone, Debug, PartialEq)]
pub struct OpticsPlan {
    /// The render setup's name.
    pub setup: String,
    /// The body as the design holds it: its definition name and the component holding it (0:
    /// the design itself). An export with the design's components matches these and `path`.
    pub definition: String,
    pub component: u64,
    /// The placement bound: its occurrence path from the design (empty for a body of the design
    /// itself).
    pub path: Vec<u64>,
    /// The body's world name, which an export of bodies where the design shows them (no
    /// components) matches.
    pub world: String,
    /// The index of refraction.
    pub ior: f64,
    /// How much of red, green and blue the inside absorbs, per mm.
    pub absorption_per_mm: [f64; 3],
    /// The surface is a thin sheet, not the boundary of a closed volume.
    pub thin_sheet: bool,
}

/// A picture shown on the model: its image on a quad.
#[derive(Clone, Debug)]
pub struct DecalPlan {
    pub name: String,
    /// The component it belongs to (0: the design itself).
    pub component: u64,
    /// Its corners (bottom-left, bottom-right, top-right, top-left) in its component's frame and
    /// in the world, mm.
    pub local: [[f64; 3]; 4],
    pub world: [[f64; 3]; 4],
    /// Its PNG or JPEG image, and the image's SHA-256 (hex), naming the file it is written as.
    pub image: SurfaceMap,
}

/// One image map of a [`SurfacePlan`]: its SHA-256 (hex), naming the file it is written as, and its
/// PNG or JPEG bytes.
#[derive(Clone, Debug)]
pub struct SurfaceMap {
    pub sha256: String,
    pub bytes: std::sync::Arc<[u8]>,
}

/// The image maps a render setup gives one placement of a body: the textures of the body's look
/// for an OpenUSD export.
#[derive(Clone, Debug)]
pub struct SurfacePlan {
    /// The render setup's name.
    pub setup: String,
    /// The body as the design holds it: its definition name and component.
    pub definition: String,
    pub component: u64,
    /// The placement bound (empty for a body of the design itself) and the body's world name.
    pub path: Vec<u64>,
    pub world: String,
    /// The projection, in the body's definition frame (mm): a point `p` is at
    /// `((p − origin)·u / |u|², (p − origin)·v / |v|²)` in the images, repeating.
    pub origin: [f64; 3],
    pub u: [f64; 3],
    pub v: [f64; 3],
    /// The image's v runs the other way (the built-in surfaces' images are stored so).
    pub flip_v: bool,
    /// How strongly the normal map tilts the surface, 0 to 1.
    pub normal_strength: f64,
    pub base_color: Option<SurfaceMap>,
    pub normal: Option<SurfaceMap>,
    pub roughness: Option<SurfaceMap>,
    pub metallic: Option<SurfaceMap>,
}

impl CadmpegOptions {
    /// Whether an export of `bodies` (all when empty) of `doc` writes the physics
    /// [`Self::physics`] asks for: only the whole design, with its components, each placed
    /// rigidly. Otherwise each body is written where the world shows it, without physics.
    pub fn physics_fits(doc: &solidcraft_doc::Document, bodies: &[String]) -> bool {
        bodies.is_empty() && !doc.occurrences.is_empty() && doc.occurrences.iter().all(|o| solidcraft_doc::is_placement(&o.transform))
    }
}
pub(crate) use export::FREECAD_REFUSED;

/// The bodies of `world` (the design's model placed in the world; all of them when `bodies` is
/// empty) as a `format` file called `file`, written by cadmpeg: OpenUSD ([`crate::Format::Usd`],
/// the extension picking the encoding), Rhino ([`crate::Format::Rhino`]), or STEP and IGES
/// ([`crate::Format::Step`], [`crate::Format::Iges`]: SolidCraft's own writers are the default; this is
/// the choice of writing them with cadmpeg); FreeCAD is refused,
/// as cadmpeg cannot write a new FreeCAD file yet. With no bodies chosen, a design with components
/// keeps them where the format holds them: `model`, the model before placing, holds each
/// component's bodies. `options` says what else is written. Meshes are made at `quality`. Returns
/// the file's bytes and what it could not carry (`export` says what each format leaves out).
/// Panics inside cadmpeg become errors.
#[allow(clippy::too_many_arguments)]
pub fn cadmpeg_export(
    doc: &solidcraft_doc::Document,
    model: &solidcraft_doc::ModelState,
    world: &solidcraft_doc::ModelState,
    bodies: &[String],
    options: &CadmpegOptions,
    file: &str,
    quality: crate::MeshQuality,
    format: crate::Format,
) -> Result<(Vec<u8>, Vec<String>), String> {
    solidcraft_kernel::guard("cadmpeg export", || {
        export::from_design(doc, model, world, bodies, options, file, quality, format).map_err(solidcraft_kernel::KernelError::Invalid)
    })
    .map_err(|e| match e {
        solidcraft_kernel::KernelError::Invalid(m) => m,
        e => e.to_string(),
    })
}

/// [`cadmpeg_export`] as OpenUSD: `.usda` text, `.usdc` or `.usd` binary, or a `.usdz` package.
pub fn usd_from_design(
    doc: &solidcraft_doc::Document,
    model: &solidcraft_doc::ModelState,
    world: &solidcraft_doc::ModelState,
    bodies: &[String],
    options: &CadmpegOptions,
    file: &str,
    quality: crate::MeshQuality,
) -> Result<(Vec<u8>, Vec<String>), String> {
    cadmpeg_export(doc, model, world, bodies, options, file, quality, crate::Format::Usd)
}

/// An OpenUSD file (`file` is its name) read for an import of its exact geometry, kept as records
/// as a FreeCAD or Rhino file's are ([`native_cad_records`]): each B-rep (a `BrepArray`'s solids
/// and shells) a B-rep record, each mesh with no B-rep beside it a mesh
/// record, the prim tree the product tree, and the materials' colours (sRGB) the bodies'. A mesh
/// that only shows a B-rep (its fallback) is read only in place of a B-rep that cannot be. Also returns what cadmpeg
/// could not bring over, and whether the file says Y is up (OpenUSD's default when it says
/// nothing). Panics inside cadmpeg or the reader become errors.
pub fn usd_records(bytes: &[u8], file: &str) -> Result<(solidcraft_kernel::ImportRecords, Vec<String>, bool), String> {
    let name = std::path::Path::new(file).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| file.to_string());
    solidcraft_kernel::guard("cadmpeg USD exact import", || convert::usd_records(&name, bytes).map_err(solidcraft_kernel::KernelError::Invalid))
        .map_err(|e| match e {
            solidcraft_kernel::KernelError::Invalid(m) => m,
            e => e.to_string(),
        })
}

/// An OpenUSD file (`file` is its name) read for a mesh import: each mesh a mesh body, the prim
/// tree a component plan and the materials the bodies' looks, as a glTF file is read
/// ([`crate::read_gltf`]); and whether the file says Y is up. Panics inside cadmpeg become errors.
pub fn read_usd(bytes: &[u8], file: &str) -> crate::Result<(crate::GltfImport, bool)> {
    solidcraft_kernel::guard("cadmpeg USD import", || usd::read(bytes, file).map_err(solidcraft_kernel::KernelError::Invalid)).map_err(|e| match e {
        solidcraft_kernel::KernelError::Invalid(m) => crate::IoError::Invalid(m),
        e => crate::IoError::Invalid(e.to_string()),
    })
}

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

    /// Caps for hostile input: cadmpeg's tight `service` profile, its caps scaled with our STEP
    /// size limit (never below `service`) and that limit on the input. The service profile already admits a detailed part's 8 million entities:
    /// cadmpeg 0.6.3 dropped the collection-item budget that refused one at a million (#1783)
    /// and the work-unit budget that went with it.
    pub(super) fn policy() -> DecodePolicy {
        let limit = crate::limits::Kind::Step.limit();
        let service = ResourceLimits::service();
        // Scaled with the limit, or a file it lets in is still refused by a default-sized cap.
        let scale = |cap: u64| {
            let scaled = u128::from(cap) * u128::from(limit) / crate::MAX_STEP_BYTES as u128;
            u64::try_from(scaled).unwrap_or(u64::MAX).max(cap)
        };
        DecodePolicy {
            mode: DecodeMode::Salvage,
            limits: ResourceLimits {
                max_input_bytes: limit,
                max_decompressed_bytes_total: scale(service.max_decompressed_bytes_total),
                max_decompressed_bytes_per_expand: scale(service.max_decompressed_bytes_per_expand),
                max_retained_bytes: scale(service.max_retained_bytes),
                max_entities: scale(service.max_entities),
                ..service
            },
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
            return Err(format!("{name}: {format} files are not supported here"));
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
        // Each MultiTransform's steps, in order.
        for r in native.and_then(|a| a.get("properties")).into_iter().flatten() {
            if !r.field("name").is_some_and(|n| n == "Transformations") {
                continue;
            }
            let (Some(owner), Some(links)) = (r.field("owner").as_ref().and_then(object_name), r.field("links")) else { continue };
            let steps: Option<Vec<String>> = links.as_array().map(|l| l.iter().filter_map(|link| link.get("object").and_then(object_name)).collect());
            if let Some(steps) = steps.filter(|s| !s.is_empty()) {
                arenas.multi_stages.insert(owner, steps);
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
    pub(super) fn records(name: &str, bytes: &[u8]) -> Result<super::RecordsRead, String> {
        let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        if !super::NATIVE_CAD_EXTS.contains(&ext.as_str()) {
            return Err(format!("{name}: not supported (FreeCAD .FCStd and Rhino .3dm only)"));
        }
        records_of(name, &ext, bytes, &ACCEPTED)
    }

    /// A STEP file (`name`) read by cadmpeg's STEP decoder into an import kept as records.
    pub(super) fn step_records(name: &str, bytes: &[u8]) -> Result<super::RecordsRead, String> {
        let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        if !matches!(ext.as_str(), "step" | "stp") {
            return Err(format!("{name}: not a STEP file (.step or .stp)"));
        }
        records_of(name, &ext, bytes, &["step"])
    }

    /// An IGES file (`name`) read by cadmpeg's IGES decoder into an import kept as records.
    pub(super) fn iges_records(name: &str, bytes: &[u8]) -> Result<super::RecordsRead, String> {
        let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        if !matches!(ext.as_str(), "igs" | "iges") {
            return Err(format!("{name}: not an IGES file (.igs or .iges)"));
        }
        records_of(name, &ext, bytes, &["iges"])
    }

    /// `bytes` decoded by the cadmpeg codec they are, if it is one of `accepted`, and read into
    /// records.
    fn records_of(name: &str, ext: &str, bytes: &[u8], accepted: &[&str]) -> Result<super::RecordsRead, String> {
        let (format, decoded) = decode_at(name, ext, bytes, accepted, DecodeDepth::Full)?;
        let (ir, report, _) = decoded.into_parts();
        records_decoded(name, &format, ir, &report, bytes)
    }

    /// An OpenUSD file (`name`) read by cadmpeg's USD codec into an import kept as records: each
    /// `BrepArray` B-rep a B-rep record (a `NurbsPatch` is refused: cadmpeg gives it no boundary), each mesh with no B-rep a mesh record,
    /// and a mesh that only shows a B-rep (its fallback, which cadmpeg makes a tessellation of
    /// that B-rep's body) not a second time. Also returns whether the file says Y is up
    /// (OpenUSD's default when it says nothing).
    pub(super) fn usd_records(name: &str, bytes: &[u8]) -> Result<(solidcraft_kernel::ImportRecords, Vec<String>, bool), String> {
        crate::limits::check(crate::limits::Kind::Step, bytes.len() as u64).map_err(|e| format!("{name}: {e}"))?;
        let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        // cadmpeg checks each layer's encoding from its first bytes; the extension only decides
        // when they say nothing.
        let ext = if ext == "usd" { String::from("usdc") } else { ext };
        let (format, decoded) = decode_at(name, &ext, bytes, &["usd"], DecodeDepth::Full)?;
        let (mut ir, report, _) = decoded.into_parts();
        let y_up = super::usd::says_y_up(&ir);
        super::usd::colours_to_srgb(&mut ir);
        let read = records_decoded(name, &format, ir, &report, bytes)?;
        Ok((read.records, read.warnings, y_up))
    }

    /// A model cadmpeg decoded (`format`, from `bytes`, the contents of file `name`) read into records.
    fn records_decoded(
        name: &str,
        format: &str,
        mut ir: CadIr,
        report: &cadmpeg_ir::report::decode::DecodeReport,
        bytes: &[u8],
    ) -> Result<super::RecordsRead, String> {
        let policy = policy();
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
        // The program that wrote the file, named as SolidCraft's own reader names it.
        let producer = match format {
            "step" => super::step_producer(bytes),
            "iges" => super::Producer { system: iges_system(&ir), ..Default::default() },
            "fcstd" => super::Producer { system: "FreeCAD".into(), ..Default::default() },
            "rhino" => super::Producer { system: "Rhino".into(), ..Default::default() },
            "usd" => super::Producer { system: "OpenUSD".into(), ..Default::default() },
            other => super::Producer { system: other.into(), ..Default::default() },
        };
        // A Rhino object's colour is often its layer's default (black), which is not how Rhino
        // shades it; STEP's, IGES's and OpenUSD's colours are how the part looks.
        let (mut records, origins) =
            super::traced_records(&ir, &product, &producer.system, matches!(format, "step" | "iges" | "usd"), format == "usd");
        if records.bodies.is_empty() {
            let why = records.warnings.first().map(|w| format!(" ({w})")).unwrap_or_default();
            return Err(format!("{name}: this {format} file has no solids that can be imported{why}"));
        }
        if format == "usd" {
            records = super::meshes_for_refused(&ir, records, &origins);
        }
        // The file's own figures for its parts, checked against the bodies (built once: the
        // design's evaluation finds them in the recent imports). A part that differs is a warning
        // about the file kept with its records, so the import's report (the History tooltip, the
        // status line, Copy Warning Message) has it, as SolidCraft's reader reports the file.
        if !ir.model.declared_measures.is_empty()
            && let Ok(imp) = solidcraft_kernel::records_import_shared(&records)
        {
            let notes = super::validation::check(&ir, &records, &origins, &imp);
            if !notes.is_empty() {
                records = solidcraft_kernel::records_noted(records, &imp, notes).map_err(|e| e.to_string())?.0;
            }
        }
        Ok(super::RecordsRead {
            records,
            warnings,
            freecad: format == "fcstd",
            preprocessor: producer.preprocessor,
            description: producer.description,
        })
    }

    /// The program that wrote an IGES file cadmpeg decoded into `ir`, as SolidCraft's own reader
    /// names it ([`solidcraft_kernel::iges_native_system`]): the global section's native system
    /// ID. cadmpeg (0.6.12) checks that field but keeps it nowhere in its model; it does keep every
    /// physical card of the file in its `iges` namespace (`cards`: each card's section, byte
    /// offset and bytes), so the global section's cards are read from there. Empty when they are
    /// not there.
    fn iges_system(ir: &CadIr) -> String {
        let cards = ir.native.namespace("iges").and_then(|n| n.arenas().get("cards"));
        // The arena is in the order of the cards' ids, not the file's: each global card is put
        // back at its place in the file (its byte offset).
        let mut lines: Vec<(u64, Vec<u8>)> = Vec::new();
        for card in cards.into_iter().flatten() {
            if card.field("section").as_ref().and_then(serde_json::Value::as_str) != Some("global") {
                continue;
            }
            let offset = card.field("offset").as_ref().and_then(serde_json::Value::as_u64);
            let payload = card.field("payload").as_ref().and_then(serde_json::Value::as_str).and_then(unhex);
            if let (Some(offset), Some(payload)) = (offset, payload) {
                lines.push((offset, payload));
            }
        }
        lines.sort_by_key(|(offset, _)| *offset);
        let mut global = Vec::new();
        for (_, line) in lines {
            global.extend_from_slice(&line);
            global.push(b'\n');
        }
        solidcraft_kernel::iges_native_system(&crate::step_text(&global))
    }

    /// Bytes written as two hexadecimal digits each (cadmpeg's native byte payloads).
    fn unhex(text: &str) -> Option<Vec<u8>> {
        let digit = |b: u8| (b as char).to_digit(16);
        text.as_bytes()
            .chunks(2)
            .map(|pair| match pair {
                [hi, lo] => Some((digit(*hi)? * 16 + digit(*lo)?) as u8),
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
