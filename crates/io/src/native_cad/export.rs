//! Export through cadmpeg: the design is written into cadmpeg's neutral model
//! ([`super::cadir_out`], built for what each writer holds) and cadmpeg's writer for the format
//! writes the file. What the writer reports it could not carry, and what the model could not hold
//! for it, are the export's warnings; nothing is left out without one.
//!
//! - OpenUSD: the extension picks text, binary or a package ([`super::usd`]).
//! - Rhino `.3dm`: archive version 8 (Rhino 8's), cadmpeg's default. cadmpeg 0.6.12's Rhino
//!   writer writes a body's exact shape only when every face is a plane and every edge a straight
//!   line; any other body is written as its mesh, with a warning naming it. It writes no
//!   assemblies, no looks (one colour a body), and meshes without a name or colour, so each body
//!   is written where the design shows it, and each of those is a warning too.
//! - FreeCAD `.FCStd`: refused. cadmpeg 0.6.12's FreeCAD writer only writes back a FreeCAD file it
//!   read (it patches the file's own `Document.xml` and makes none), so it cannot write a design.

use cadmpeg_ir::codec::write::{EncodeInput, target::TargetRequest};
use cadmpeg_ir::document::CadIr;
use cadmpeg_registry::{Format as Codec, build_encoder};

use super::cadir_out::{self, Writes};
use crate::Format;

/// Why a FreeCAD file is not written.
pub(crate) const FREECAD_REFUSED: &str = "FreeCAD files cannot be exported yet: cadmpeg's FreeCAD writer only writes back a FreeCAD file it read, \
     and makes no new one from a design";

/// Why a Rhino file is not written in the browser.
pub(crate) const RHINO_IN_BROWSER: &str = "Rhino files cannot be exported in the browser yet: cadmpeg's Rhino writer needs a temporary file";

/// Rhino: no product records, a mesh only where the exact shape cannot be written, each mesh a
/// mesh of its own, one sRGB colour a body, and the pcurves of planar faces left to the writer,
/// which makes them from the edges and takes no other.
const RHINO: Writes = Writes {
    label: "Rhino",
    products: false,
    brep_meshes: false,
    bound_meshes: false,
    appearances: false,
    linear: false,
    planar_pcurves: false,
    named_bodies: false,
    admit: Some(rhino_admits),
};

/// The Rhino archive version written: Rhino 8's, cadmpeg's default.
const RHINO_TARGET: &str = "8";

/// Whether cadmpeg's Rhino writer writes `ir` (one body alone).
fn rhino_admits(ir: &CadIr) -> Result<(), String> {
    build_encoder(Codec::Rhino).plan(EncodeInput::new(ir, None), TargetRequest::Explicit(RHINO_TARGET)).map(|_| ()).map_err(|e| e.to_string())
}

/// Losses cadmpeg reports that say nothing to the person exporting: a USD mesh's points are
/// single precision, which every USD mesh is (the exact B-rep beside it keeps double precision;
/// a mesh that is a body's only geometry is said when the rounding can matter,
/// [`mesh_only_rounds`]); a Rhino mesh's normals are single precision, as every 3DM mesh's are.
const QUIET_LOSSES: [&str; 2] = [VERTEX_PRECISION, "rhino/mesh.normal-precision-reduced"];

/// cadmpeg's USD loss for points written in single precision.
const VERTEX_PRECISION: &str = "usd/export.vertex-precision";

/// Whether a mesh of `ir` that is its body's only geometry (no body, or a body with no exact
/// regions) lies so far from the origin for its size that single precision can move its points
/// past the export's weld distance: a coordinate `x` rounds to within `|x| 2⁻²⁴` (relative, in
/// any unit), and the weld distance is the distance within which a mesh's points are one
/// (`crate::weld_distance`, of the mesh's diagonal).
pub(super) fn mesh_only_rounds(ir: &CadIr) -> bool {
    ir.model.tessellations.iter().any(|t| {
        let exact = t.body.as_ref().is_some_and(|b| ir.model.bodies.iter().any(|x| x.id == *b && !x.regions.is_empty()));
        if exact {
            return false;
        }
        let (mut lo, mut hi, mut far) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3], 0.0_f64);
        for p in t.vertices() {
            let p = p.get();
            for (k, v) in [p.x, p.y, p.z].into_iter().enumerate() {
                lo[k] = lo[k].min(v);
                hi[k] = hi[k].max(v);
                far = far.max(v.abs());
            }
        }
        let size = (0..3).map(|k| (hi[k] - lo[k]).powi(2)).sum::<f64>().sqrt();
        size.is_finite() && far * f64::from(f32::EPSILON) * 0.5 > crate::weld_distance(size)
    })
}

/// The bodies of `world` (all when `bodies` is empty) as a `format` file called `file`, written by
/// cadmpeg, with what the file could not carry: the design's components and placements when no
/// bodies are chosen and the format holds them ([`cadir_out::design_for`] says how), each mesh at
/// `quality`, and what `options` asks for.
#[allow(clippy::too_many_arguments)]
pub(super) fn from_design(
    doc: &solidcraft_doc::Document,
    model: &solidcraft_doc::ModelState,
    world: &solidcraft_doc::ModelState,
    bodies: &[String],
    options: &super::CadmpegOptions,
    file: &str,
    quality: crate::MeshQuality,
    format: Format,
) -> Result<(Vec<u8>, Vec<String>), String> {
    let name = std::path::Path::new(file).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| file.to_string());
    let (codec, target, writes) = match format {
        Format::Usd => {
            let target = super::usd::target(&name).ok_or_else(|| format!("{name}: not an OpenUSD file name (.usda, .usdc, .usd or .usdz)"))?;
            (Codec::Usd, target, Writes::USD)
        }
        // cadmpeg's Rhino writer stages the objects in a temporary file, and the browser has no
        // file system: asking it for one there would end the page, so it is refused first.
        Format::Rhino if cfg!(target_arch = "wasm32") => return Err(format!("{name}: {RHINO_IN_BROWSER}")),
        Format::Rhino => (Codec::Rhino, RHINO_TARGET, RHINO),
        Format::FreeCad => return Err(format!("{name}: {FREECAD_REFUSED}")),
        other => return Err(format!("{name}: {other:?} is not written through cadmpeg")),
    };
    let stem = std::path::Path::new(&name).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    // Images go inside a package; a layer alone would name files the export does not write.
    let mut early = Vec::new();
    let unpackaged;
    let options = if !options.surfaces.is_empty() && target != "usdz" {
        early.push(format!(
            "the images of render setup `{}` are written only in a .usdz package: the bodies are written with their looks",
            options.surfaces.first().map(|p| p.setup.as_str()).unwrap_or_default()
        ));
        unpackaged = super::CadmpegOptions { surfaces: Vec::new(), ..options.clone() };
        &unpackaged
    } else {
        options
    };
    let built = cadir_out::design_for(doc, model, world, bodies, options, &stem, quality, writes).map_err(|e| format!("{name}: {e}"))?;
    let label = writes.label;
    let plan = build_encoder(codec)
        .plan(EncodeInput::new(&built.ir, None), TargetRequest::Explicit(target))
        .map_err(|e| format!("{name}: could not write {label}: {e}"))?;
    let mut warnings = early;
    warnings.extend(built.warnings);
    let rounds = mesh_only_rounds(&built.ir);
    for l in &plan.report().losses {
        let code = l.code.to_string();
        if !QUIET_LOSSES.contains(&code.as_str()) || (code == VERTEX_PRECISION && rounds) {
            warnings.push(format!("{code}: {}", l.message));
        }
    }
    let mut out = Vec::new();
    plan.write_to(&mut out).map_err(|e| format!("{name}: could not write {label}: {e}"))?;
    Ok((out, warnings))
}
