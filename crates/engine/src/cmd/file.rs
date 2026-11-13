//! Documents and files: new, open designs, CAD files and drawings, insert STEP, save, export.

#[path = "file_thumbnail.rs"]
mod thumbnail;

#[path = "file_easyprint.rs"]
mod easyprint;

#[path = "file_import.rs"]
pub(crate) mod import;

#[path = "file_open_steps.rs"]
pub(crate) mod open_steps;

#[cfg(not(target_arch = "wasm32"))]
#[path = "file_send.rs"]
mod send;

use serde_json::{Value, json};
use solidcraft_doc::Document;
use solidcraft_io::Format;

use super::CommandSpec;
use super::print_face;
use crate::params::{bad, bool_, num, str_, string_list};
use crate::schema::file::{
    ExportResult, NewerFileReport, OpenElsewhere, OpenResult, OpenedDesign, OpenedDrawing, OpenedFile, OpenedPointCloud, RecoverParams,
    RecoverResult, RecoveredDesign, RecoveryDiscardParams, RecoveryDiscardResult, RecoveryListParams, RecoveryListResult,
};
use crate::schema::insert_mesh::{InsertMeshParams, MeshInserted};
use crate::schema::insert_step::{InsertStepParams, StepInserted};
use crate::schema::new_design::{NewDesignParams, NewDesignResult};
use crate::schema::save::{SaveAsParams, SaveParams, SaveResult};
use crate::schema::save_mesh::{SaveMeshResult, SlicerDelivery};
use crate::schema::slicers::{SlicerInfo, SlicersParams, SlicersResult};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const INSERT_MESH_CONTRACT: TypedContract<InsertMeshParams> = TypedContract::new();
const INSERT_STEP_CONTRACT: TypedContract<InsertStepParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("file.new", "New Design", crate::checked!("file.new", new_doc))
        .icon("new")
        .key("Ctrl+N")
        .noundo()
        .contract(&NEW_DESIGN_CONTRACT)
        .output(output_schema_for::<NewDesignResult>)
        .params("name?"),
    CommandSpec::new("doc.open", "Open", crate::checked!("doc.open", open))
        .icon("open")
        .key("Ctrl+O")
        .noundo()
        .contract(&OPEN_CONTRACT)
        .output(output_schema_for::<OpenResult>)
        .params("path: .sc design (or a .solidcraft design from an earlier release), .dxf/.svg drawing (opens as a new, unsaved design with an XY sketch, using sketch.insert_dxf/sketch.insert_svg units and defaults), or a .step/.stp/.igs/.fcstd/.3dm/.3mf/.stl/.obj/.glb/.gltf file (STEP, IGES, FreeCAD, Rhino, 3MF, STL, OBJ or glTF; opens as a new design holding the file as a component named after it, as file.insert_step and file.insert_mesh make), or a .ply/.xyz/.pts point cloud (opens as a new design holding it, as pointcloud.insert makes; its `units` and `max_points` as there); up?: z|y for a STEP, IGES, FreeCAD, Rhino or glTF file (see file.insert_step and file.insert_mesh); names?: auto|solid|product|instance for a STEP, IGES, FreeCAD or Rhino file (see file.insert_step); units?: for an STL or OBJ file (see file.insert_mesh); origin?: bool for a mesh file (see file.insert_mesh); folder?: bool for a STEP, IGES, FreeCAD or Rhino file (see file.insert_step); reuse_import?: bool (STEP/IGES: reuse an unchanged import from the local saved-model cache, default true; false recomputes without reading or writing the cache; → import_reused: bool, content_sha256: exact source digest); read_only?: bool (a design: saving to its own file is refused, file.save_as keeps the changes elsewhere) → open_elsewhere: {host, user, app, opened} when another SolidCraft has the design open (its lock file is fresh); newer_file: {format, reads_up_to, unknown_changes, not_understood, notice} and warnings when a newer SolidCraft saved it: it opens all the same, and what this version can't read is named there and kept by saving"),
    CommandSpec::new("file.insert_step", "Insert STEP", crate::checked!("file.insert_step", insert_step))
        .at("SOLID", "INSERT")
        .icon("import")
        .contract(&INSERT_STEP_CONTRACT)
        .output(output_schema_for::<StepInserted>)
        .params("path: STEP, IGES, FreeCAD .FCStd or Rhino .3dm file (its bodies join the design as an Import base feature in a new component named after the file, placed in the active component; an assembly's products become nested components, a repeated part one component with an occurrence per placement, all held as one rigid group until component.unlock → component, component_name, occurrence, subcomponents?, rigid_joints?, group?: the folder); folder?: bool (default false; true: an import that brings more than one item into the history (its Import feature and the rigid joints holding its parts) goes into a new history folder named `Group #N`); name? (the feature's and the component's); up?: z|y (default z: the file's axes are kept; y: the file has Y up, so the import's component is turned a quarter turn about X, Y becoming Z → stand_up: its occurrence); names?: auto|solid|product|instance (default auto: keep meaningful solid names, replace placeholders with the nearest named product, else instance; solid: the solid's own name; product: the part's name; instance: the occurrence's name, else the product's); matrix?: [12 or 16 numbers] (where the new component's occurrence sits in the active component, after `up`: a rigid row-major 3x4 rotation-and-translation or 4x4 matrix, flat or as rows, as occurrence.move takes it; a scale, shear or mirror is refused before the file is read); tolerance? (see occurrence.move)"),
    CommandSpec::new("file.insert_mesh", "Insert Mesh", crate::checked!("file.insert_mesh", insert_mesh))
        .at("SOLID", "INSERT")
        .icon("import")
        .contract(&INSERT_MESH_CONTRACT)
        .output(output_schema_for::<MeshInserted>)
        .params("path: .3mf, .stl, .obj, .ply (with faces), .glb or .gltf file (its meshes join the design as mesh bodies in a new component named after the file → component, component_name, occurrence; a glTF file's node tree becomes nested components held as one rigid group until component.unlock, a mesh placed again another occurrence, its materials the bodies' appearances, and its metres millimetres); name?; up?: z|y for a glTF file (default y: glTF has Y up, so the import's component is turned a quarter turn about X, Y becoming Z → stand_up: its occurrence; z: the file's axes are kept); units?: mm|cm|m|in|ft for an STL or OBJ file, or a PLY file with faces (default mm): the unit its coordinates are in, scaled to millimetres as the file is read (a 3MF or glTF file states its own). The design keeps the millimetre mesh and not the unit, so insert the file again to try another; origin?: bool (default false: the mesh keeps its file's coordinates; true: once read, scaled and stood up, the import's occurrence is moved so the box around its bodies is centred on X and Y of the active component with its lowest point on Z = 0 → moved_to_origin: [x, y, z], the shift made); crease_angle_degrees?: 0..180 (0 clears imported shading normals for the previous flat-shading behavior; positive angles regenerate angle-weighted normals across manifold edges below this crease angle; omitted keeps authored OBJ normals/smoothing groups, or flat shading when absent; geometry stays unchanged)"),
    CommandSpec::new("file.save", "Save", crate::checked!("file.save", save))
        .icon("save")
        .key("Ctrl+S")
        .noundo()
        .contract(&SAVE_CONTRACT)
        .output(output_schema_for::<SaveResult>)
        .params("path? (default: current file; a name that opens as another kind of file, such as .step, .stl or .dxf, is refused: file.export writes those); format?: container|json (default: json for .json, otherwise container); compression?: fast|compact (default: fast); cache?: bool (the evaluated model, so the design opens without rebuilding; default: true); preview?: bool (saved display meshes, default: false); overwrite?: bool (replace the design's file even though another program or computer changed it since it was opened or saved; without it that save is refused)"),
    CommandSpec::new("file.save_as", "Save As", crate::checked!("file.save_as", save_as))
        .icon("save")
        .key("Ctrl+Shift+S")
        .noundo()
        .contract(&SAVE_AS_CONTRACT)
        .output(output_schema_for::<SaveResult>)
        .params("path (a design file name, .sc as a rule; one that opens as another kind of file, such as .step, .stl or .dxf, is refused: file.export writes those); format?: container|json (default: json for .json, otherwise container); compression?: fast|compact (default: fast); cache?: bool (the evaluated model, so the design opens without rebuilding; default: true); preview?: bool (saved display meshes, default: false); overwrite?: bool (as file.save, when `path` is the design's own file)"),
    CommandSpec::new("doc.recovery_list", "Recoverable Designs", crate::checked!("doc.recovery_list", recovery_list)).icon("restore")
        .noundo()
        .contract(&RECOVERY_LIST_CONTRACT)
        .output(output_schema_for::<RecoveryListResult>)
        .params("dir? (default: the recovery folder) → designs autosaved by apps that crashed or closed with unsaved changes"),
    CommandSpec::new("doc.recover", "Recover Design", crate::checked!("doc.recover", recover)).icon("restore")
        .noundo()
        .contract(&RECOVER_CONTRACT)
        .output(output_schema_for::<RecoverResult>)
        .params("id (from doc.recovery_list); dir?; discard?: bool (delete the entry once open) — opens it with its file path, unsaved"),
    CommandSpec::new("doc.recovery_discard", "Discard Recovered Design", crate::checked!("doc.recovery_discard", recovery_discard))
        .icon("delete")
        .noundo()
        .contract(&RECOVERY_DISCARD_CONTRACT)
        .output(output_schema_for::<RecoveryDiscardResult>)
        .params("id, or all: true; dir?"),
    CommandSpec::new("file.export", "Export", crate::checked!("file.export", export))
        .icon("export")
        .noundo()
        .contract(&EXPORT_CONTRACT)
        .output(crate::schema::output_schema_for::<crate::schema::file::FileExportResult>)
        .params("path; format?: stl|stla|obj|step|3mf|3mf-tagged|glb|urdf|json (default from extension; 3mf-tagged: 3MF whose triangles carry their B-rep face, at full precision, for meshing and analysis tools; docs/formats/3mf-face-tags.md; obj: the colours of the bodies go in a material library (`.mtl`) written next to the mesh file; glb: binary glTF in metres with Y up, each body placed and named, its colours, metallic and roughness as materials; a transparent body or face, or a body a render setup gives an image texture or optics, is refused; urdf: the whole assembly as a robot, with STL meshes written next to it; docs/urdf.md); bodies?: [names] (JSON and URDF export the complete design); urdf only: mesh_prefix? (before each mesh file name, e.g. package://robot/meshes/), effort? (default 1000), velocity? (default 10); quality?: standard|fine|finest (STL, OBJ, 3MF and GLB: how closely the mesh follows curved faces, a chord tolerance of 2e-4, 5e-5 or 1e-5 of each body's size; default standard); objects?: body|component (3MF only: one object per body, the default, or one object for each placed component with two or more bodies, with those bodies as its parts, so a slicer loads them as one object with parts); lay_on_print_faces?: bool (STL and 3MF only, default false: each body with a print face (body.print_face) is written laid on it, as a slicer export lays it → print_faces: the bodies laid, warnings: each body that has a print face but was written as it lies)"),
    // One command per format, so each File › Export item can have a key of its own; started
    // from a key or command search, the app opens the save picker (the mesh dialog for STL and
    // 3MF). Each takes `file.export`'s parameters for its format, without `format`.
    CommandSpec::new("file.export_step", "Export STEP", crate::checked!("file.export_step", export_step))
        .icon("export")
        .noundo()
        .contract(&EXPORT_STEP_CONTRACT)
        .output(crate::schema::output_schema_for::<crate::schema::file::ExportResult>)
        .params("path; bodies?: [names] (default: every body; a design with components then exports as a STEP assembly) → {path, bytes, format}: STEP, whatever the path's extension"),
    CommandSpec::new("file.export_stl", "Export STL", crate::checked!("file.export_stl", export_stl))
        .icon("mesh_export")
        .noundo()
        .contract(&EXPORT_STL_CONTRACT)
        .output(crate::schema::output_schema_for::<crate::schema::file::ExportResult>)
        .params("path; bodies?: [names] (default: every body); quality?: standard|fine|finest (default standard); lay_on_print_faces?: bool (default false: as file.export) → {path, bytes, format, print_faces?, warnings?}: binary STL, whatever the path's extension"),
    CommandSpec::new("file.export_3mf", "Export 3MF", crate::checked!("file.export_3mf", export_3mf))
        .icon("mesh_export")
        .noundo()
        .contract(&EXPORT_3MF_CONTRACT)
        .output(crate::schema::output_schema_for::<crate::schema::file::ExportResult>)
        .params("path; bodies?: [names] (default: every body); quality?: standard|fine|finest (default standard); objects?: body|component (as file.export); lay_on_print_faces?: bool (default false: as file.export) → {path, bytes, format, print_faces?, warnings?}: 3MF, whatever the path's extension"),
    CommandSpec::new("file.export_obj", "Export OBJ", crate::checked!("file.export_obj", export_obj_cmd))
        .icon("export")
        .noundo()
        .contract(&EXPORT_OBJ_CONTRACT)
        .output(crate::schema::output_schema_for::<crate::schema::file::ExportResult>)
        .params("path; bodies?: [names] (default: every body); quality?: standard|fine|finest (default standard) → {path, bytes, format, files}: OBJ, with the colours of the bodies in a material library (`.mtl`) next to it"),
    CommandSpec::new("file.export_urdf", "Export URDF", crate::checked!("file.export_urdf", export_urdf_cmd))
        .icon("export")
        .noundo()
        .contract(&EXPORT_URDF_CONTRACT)
        .output(crate::schema::output_schema_for::<crate::schema::file::UrdfExportResult>)
        .params("path; mesh_prefix?; effort? (default 1000); velocity? (default 10) → {path, format, files, links, joints, warnings}: the whole assembly as a robot, with STL meshes written next to it; docs/urdf.md"),
    CommandSpec::new("file.save_mesh", "Save As Mesh", crate::checked!("file.save_mesh", save_mesh))
        .at("INSERT", "FILES")
        .icon("mesh_export")
        .noundo()
        .contract(&SAVE_MESH_CONTRACT)
        .output(crate::schema::output_schema_for::<crate::schema::save_mesh::SaveMeshResult>)
        .params("path; format?: stl|3mf|3mf-tagged (default from the extension, else stl; 3mf-tagged: docs/formats/3mf-face-tags.md); bodies?: [names] (default: all); ascii?: bool (STL); slicer?: id or name from file.slicers, the program path it lists, or a program the user added under Custom… (send the saved file there, through the same steps as file.send_to_slicer: a slicer that has to be started first is waited for; may be deprecated later in favour of file.send_to_slicer or a Send to App workflow, kept for scripts); single_instance?: bool (default true; see file.send_to_slicer); quality?: standard|fine|finest (STL, OBJ and 3MF: how closely the mesh follows curved faces, a chord tolerance of 2e-4, 5e-5 or 1e-5 of each body's size; default standard); objects?: body|component (3MF only: one object per body, the default, or one object for each placed component with two or more bodies, with those bodies as its parts, so a slicer loads them as one object with parts); lay_on_print_faces?: bool (default false: each body with a print face (body.print_face) is saved laid on it; a file sent to `slicer` is laid on them whatever this says)"),
    CommandSpec::new("file.send_to_slicer", "Send to Slicer", send_to_slicer)
        .enabled(starts_programs)
        .at("INSERT", "FILES")
        .icon("mesh_export")
        .noundo()
        .params("slicer: id or name from file.slicers, or a program the user added under Custom…; bodies?: [names] (default: the selected bodies); wait?: block|return|no (default block: a slicer that drops a file at a cold start is started alone and the call waits up to 10 s for it to be ready, then sends; return: start it and answer {starting}; no: send now); single_instance?: bool (default true: PrusaSlicer, SuperSlicer and Cura load the file in the window that is open); format?: 3mf|stl|3mf-tagged (default 3mf, in millimetres; STL if that can't be written) → saves the bodies in this user's slicer folder and opens the file in the slicer. The file name stays the same for the same design and bodies, so sending again replaces it (use the slicer's Reload from disk)"),
    CommandSpec::new("file.open_in_easyprint", "Open with EasyPrint", easyprint::open_in_easyprint)
        .icon("mesh_export")
        .noundo()
        .params("consent: bool (required: true only once the user has agreed to upload the model to Printables' servers; it stays there for 30 minutes and opening it needs a Printables sign-in); bodies?: [names] (default: the selected bodies); format?: 3mf|stl (default 3mf, in millimetres) → uploads the bodies to EasyPrint, Printables' slicer in the browser, and opens it in the browser. 100 MB at most. Works in the desktop app and the browser build; the CLI has no way to upload yet"),
    CommandSpec::new("file.slicers", "Installed Slicers", crate::checked!("file.slicers", slicers))
        .icon("export")
        .noundo()
        .contract(&SLICERS_CONTRACT)
        .output(output_schema_for::<SlicersResult>)
        .params("→ the slicers found on this machine: [{id, name, program, icon?}]; pass an id, a name or the program path as file.send_to_slicer's or file.save_mesh's `slicer`"),
    CommandSpec::new("file.import_cache_clear", "Clear Reused Imports", crate::checked!("file.import_cache_clear", clear_import_cache))
        .icon("delete")
        .noundo()
        .contract(&CLEAR_IMPORT_CACHE_CONTRACT)
        .output(crate::schema::output_schema_for::<crate::schema::file::ImportCacheClearResult>)
        .params("→ removed: number of local cached imports deleted; leaves open designs and remembered stand-up answers unchanged"),
];

const CLEAR_IMPORT_CACHE_CONTRACT: crate::schema::TypedContract<crate::schema::file::ImportCacheClearParams> = crate::schema::TypedContract::new();

static IMPORT_CACHE_DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// Keep this process's reused imports in `dir` instead of the per-user data folder. For test
/// harnesses of other crates, where `cfg!(test)` is false in the engine; the first call wins.
pub fn use_import_cache_dir(dir: std::path::PathBuf) {
    let _ = IMPORT_CACHE_DIR.set(dir);
}

fn import_cache() -> Option<solidcraft_io::ImportCache> {
    if cfg!(target_arch = "wasm32") {
        return None;
    }
    if let Some(dir) = IMPORT_CACHE_DIR.get() {
        return Some(solidcraft_io::ImportCache::new(dir.clone()));
    }
    if let Some(dir) = solidcraft_geom::env::var_os("IMPORT_CACHE_DIR").filter(|dir| !dir.is_empty()) {
        return Some(solidcraft_io::ImportCache::new(dir.into()));
    }
    if cfg!(test) {
        return Some(solidcraft_io::ImportCache::new(std::env::temp_dir().join(format!("solidcraft-test-imports-{}", std::process::id()))));
    }
    // The stable per-user data folder is adopted across app-id updates with the other data.
    crate::recovery::data_dir().map(|dir| solidcraft_io::ImportCache::new(dir.join("imports")))
}

fn clear_import_cache(_: &mut Session, _: &Value) -> Result<Value> {
    let removed = import_cache().map(|cache| cache.clear()).transpose().map_err(|e| EngineError::Other(e.to_string()))?.unwrap_or(0);
    Ok(json!({"removed": removed}))
}

fn new_doc(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_(p, "name").unwrap_or("Untitled");
    s.replace_design(Session::new(Document::new(name)));
    // A name given here is the user's own, so Save As keeps it (#4907); "Untitled" is none.
    s.user_named = !name.trim().is_empty() && name.trim() != DEFAULT_NAME;
    let reply = NewDesignResult { name: name.to_string(), param_warnings: Vec::new() };
    serde_json::to_value(reply).map_err(|e| bad("file.new", e.to_string()))
}

const OPEN_CONTRACT: TypedContract<crate::schema::file::OpenParams> = TypedContract::new();
const RECOVERY_LIST_CONTRACT: TypedContract<RecoveryListParams> = TypedContract::new();
const RECOVER_CONTRACT: TypedContract<RecoverParams> = TypedContract::new();
const RECOVERY_DISCARD_CONTRACT: TypedContract<RecoveryDiscardParams> = TypedContract::new();
const NEW_DESIGN_CONTRACT: TypedContract<NewDesignParams> = TypedContract::new();
const EXPORT_CONTRACT: crate::schema::TypedContract<crate::schema::file::ExportParams> = crate::schema::TypedContract::new();
const SAVE_CONTRACT: TypedContract<SaveParams> = TypedContract::new();
const SAVE_AS_CONTRACT: TypedContract<SaveAsParams> = TypedContract::new();
const EXPORT_STEP_CONTRACT: crate::schema::TypedContract<crate::schema::file::ExportStepParams> = crate::schema::TypedContract::new();
const EXPORT_STL_CONTRACT: crate::schema::TypedContract<crate::schema::file::ExportStlParams> = crate::schema::TypedContract::new();
const EXPORT_3MF_CONTRACT: crate::schema::TypedContract<crate::schema::file::Export3mfParams> = crate::schema::TypedContract::new();
const EXPORT_OBJ_CONTRACT: crate::schema::TypedContract<crate::schema::file::ExportMeshParams> = crate::schema::TypedContract::new();
const SLICERS_CONTRACT: TypedContract<SlicersParams> = TypedContract::new();
const EXPORT_URDF_CONTRACT: crate::schema::TypedContract<crate::schema::file::ExportUrdfParams> = crate::schema::TypedContract::new();
const SAVE_MESH_CONTRACT: crate::schema::TypedContract<crate::schema::save_mesh::SaveMeshParams> = crate::schema::TypedContract::new();

fn path_arg<'a>(p: &'a Value, cmd: &str) -> Result<&'a str> {
    str_(p, "path").filter(|x| usable_path(x)).ok_or_else(|| bad(cmd, "`path` is required"))
}

/// Whether `path` is one a command takes: not blank, under 4096 bytes.
pub(crate) fn usable_path(path: &str) -> bool {
    !path.trim().is_empty() && path.len() < 4096
}

/// What `up` says when it is not `z` or `y`.
pub(crate) const UP_MESSAGE: &str = "`up` must be \"z\" (the file's axes are kept) or \"y\" (the file has Y up: its Y becomes Z)";
/// What `names` says when it is not one of its words.
pub(crate) const NAMES_MESSAGE: &str = "`names` must be \"auto\", \"solid\", \"product\" or \"instance\"";

/// `up` as a file is read with it: `Some(true)` for `y`, `Some(false)` for `z` (any case, spaces
/// around it ignored), `None` for anything else.
pub(crate) fn up_form(v: &Value) -> Option<bool> {
    match v.as_str().map(|u| u.trim().to_ascii_lowercase()).as_deref() {
        Some("z") => Some(false),
        Some("y") => Some(true),
        _ => None,
    }
}

/// `names` as a STEP or IGES file is read with it, `None` for anything that is not a word of it.
pub(crate) fn names_form(v: &Value) -> Option<solidcraft_io::StepNames> {
    v.as_str().and_then(solidcraft_io::StepNames::parse)
}

/// How a STEP or IGES file comes in beyond its path: which way is up in the file, and what its
/// bodies are named after.
struct StepOptions {
    /// `up: "y"`: the file has Y up, so the bodies it brings are turned a quarter turn about X
    /// (Y becomes Z), as the app's "Stand it up" offer does. `z` (the default) keeps the file's axes.
    y_up: bool,
    names: solidcraft_io::StepNames,
    /// `folder: true`: an import that brings more than one item gets a `Group #N` history folder
    /// (`import_folder`). `false` (the default) makes none, as before the option.
    folder: bool,
}

fn step_options(p: &Value, cmd: &str) -> Result<StepOptions> {
    let y_up = match p.get("up") {
        None | Some(Value::Null) => false,
        Some(v) => up_form(v).ok_or_else(|| bad(cmd, UP_MESSAGE))?,
    };
    let names = match p.get("names") {
        None | Some(Value::Null) => solidcraft_io::StepNames::default(),
        Some(v) => names_form(v).ok_or_else(|| bad(cmd, NAMES_MESSAGE))?,
    };
    let folder = match p.get("folder") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(folder)) => *folder,
        Some(_) => {
            return Err(bad(
                cmd,
                "`folder` must be true (an import that brings more than one item goes into a `Group #N` history folder) or false (no folder)",
            ));
        }
    };
    Ok(StepOptions { y_up, names, folder })
}

/// Read a STEP file as an Import feature.
fn read_step(path: &str, names: solidcraft_io::StepNames) -> Result<(solidcraft_io::StepFeature, Vec<u8>)> {
    let bytes = solidcraft_io::limits::read(path, solidcraft_io::limits::Kind::Step).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    let feature = solidcraft_io::step_import_feature_named(&bytes, path, names).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    Ok((feature, bytes))
}

/// Which way is up in a STEP or IGES file, from the program its header names: `("y", name)` for
/// an exporter whose files have Y up, `("z", name)` for SolidCraft's own (its STEP and IGES exports both name it), else `None`. Only
/// SolidWorks is listed as Y up: its STEP and IGES writers are well known to keep that program's
/// Y-up front view, and its name is in FILE_NAME (originating system, preprocessor version) and
/// often FILE_DESCRIPTION. Programs we aren't sure of stay unknown, so the file is only offered.
fn exporter_up(import: &solidcraft_kernel::StepImport) -> Option<(&'static str, &'static str)> {
    if import.system.trim().starts_with("SolidCraft") {
        return Some(("z", "SolidCraft"));
    }
    let named = [&import.system, &import.preprocessor, &import.description].iter().any(|h| h.to_ascii_lowercase().contains("solidworks"));
    named.then_some(("y", "SolidWorks"))
}

/// A quarter turn about X through the origin: a file's Y up becomes Z up.
fn quarter_turn_x() -> solidcraft_doc::Mat {
    solidcraft_doc::rigid(solidcore_record::Vec3::ZERO, solidcore_record::Vec3::ZERO, solidcore_record::Vec3::X, std::f64::consts::FRAC_PI_2)
}

/// Add an Import feature for a STEP file to the session's design, in a new component named
/// after the file (#1386).
///
/// While the profiler records, the reply's `profile` is the import's share of it (reading,
/// building the solids in SMLib, placing, meshing and evaluating them), as
/// `diagnostics.profile` reports it.
fn add_step(s: &mut Session, path: &str, name: Option<&str>, options: &StepOptions) -> Result<Value> {
    let profiled = solidcraft_profile::enabled().then(solidcraft_profile::mark);
    let (sf, bytes) = read_step(path, options.names)?;
    add_read_step(s, path, name, options, sf, &bytes, profiled)
}

fn add_read_step(
    s: &mut Session,
    path: &str,
    name: Option<&str>,
    options: &StepOptions,
    sf: solidcraft_io::StepFeature,
    bytes: &[u8],
    profiled: Option<solidcraft_profile::Mark>,
) -> Result<Value> {
    // The evaluator already certifies the STEP payload with SHA-256. Reuse that digest only
    // when it is exactly the source bytes (IGES conversion and legacy encodings differ).
    let unchanged = matches!(&sf.kind, solidcraft_doc::FeatureKind::Import { step, .. } if step.as_bytes() == bytes);
    let fname = name.filter(|n| !n.trim().is_empty()).unwrap_or(&sf.name).to_string();
    let cname = import::component_name(path, name);
    let (id, placed) = import::add_in_component(s, sf.kind, &fname, sf.body_names, &cname, import::Tree::Step(&sf.import))?;
    let content_sha256 = unchanged
        .then(|| {
            s.model.state().bodies.iter().filter(|b| b.feature == id).find_map(|b| {
                b.body.face_sources()?.iter().flatten().find_map(|source| match source {
                    solidcraft_kernel::FaceSource::Imported { payload_sha256: Some(digest), producer: Some(producer), .. } if *producer == id => {
                        Some(digest.clone())
                    }
                    _ => None,
                })
            })
        })
        .flatten()
        .unwrap_or_else(|| {
            use sha2::{Digest, Sha256};
            format!("{:x}", Sha256::digest(bytes))
        });
    // Y up: the import's component is turned a quarter turn about X (Y becomes Z), so an
    // assembly's parts and instances turn together.
    let turned = if options.y_up { turn_up(s, &placed)? } else { None };
    let created = created_bodies(s, id);
    let folder = if options.folder { import_folder(s, id, &placed)? } else { None };
    let fname = s.doc.feature(id).map(|f| f.name.clone()).unwrap_or_default();
    let known = exporter_up(&sf.import);
    let reply = StepInserted {
        path: path.to_string(),
        feature: id,
        name: fname,
        bodies: created.clone(),
        created_bodies: created,
        warnings: sf.warnings,
        system: sf.import.system.clone(),
        content_sha256,
        exporter: known.map(|(_, exporter)| exporter.to_string()),
        exporter_up: known.map(|(up, _)| up.to_string()),
        component: placed.component,
        component_name: placed.name.clone(),
        occurrence: placed.occurrence,
        subcomponents: (placed.components > 0).then_some(placed.components),
        rigid_joints: (placed.components > 0).then_some(placed.joints.len()),
        stand_up: turned,
        group: folder,
        transform: None,
        profile: profiled.map(|mark| crate::schema::diagnostics::ProfileReport::new(&mark.report(), 20)),
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(format!("{path}: {e}")))
}

/// An import that brings more than one item into the history (its Import feature and the rigid
/// joints holding its parts together) goes into a history folder of its own, named `Group #N`,
/// holding the feature and those joints, so it stands apart from the features before and after
/// it and the History shows it as one chip. The folder sits in the folder the feature was put
/// in, if any. Returns the folder.
fn import_folder(s: &mut Session, feature: u64, placed: &import::Placed) -> Result<Option<u64>> {
    if placed.joints.is_empty() {
        return Ok(None);
    }
    let parent = s.doc.timeline_groups.iter().find(|folder| folder.features.contains(&feature)).map(|folder| folder.id);
    let name = next_group_name(&s.doc);
    super::edit::new_folder(s, &name, vec![feature], placed.joints.clone(), parent, "file.insert_step").map(Some)
}

/// The first `Group #N` no history folder is named yet.
fn next_group_name(doc: &Document) -> String {
    let taken = |name: &str| doc.timeline_groups.iter().any(|folder| folder.name == name);
    (1..=doc.timeline_groups.len() + 1).map(|n| format!("Group #{n}")).find(|name| !taken(name)).unwrap_or_else(|| "Group".into())
}

/// Cache only a fresh open, never a live design, insert, or later edit. Read the source once:
/// the bytes hashed here are exactly the bytes imported on a miss.
fn open_step(
    path: &str,
    p: &Value,
    cache: Option<&solidcraft_io::ImportCache>,
    validation: solidcraft_kernel::Validation,
) -> Result<(Session, Value)> {
    let profiled = solidcraft_profile::enabled().then(solidcraft_profile::mark);
    let options = step_options(p, "doc.open")?;
    let reuse = match p.get("reuse_import") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(reuse)) => *reuse,
        _ => return Err(bad("doc.open", "`reuse_import` must be a boolean")),
    };
    let cache = cache.filter(|_| reuse && !solidcraft_io::is_native_cad_path(path) && solidcraft_geom::env::var_os("NO_MODEL_CACHE").is_none());
    let bytes = solidcraft_io::limits::read(path, solidcraft_io::limits::Kind::Step).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    let key = solidcraft_io::ImportCache::key(&bytes, path, &json!({"y_up": options.y_up, "names": format!("{:?}", options.names)}));
    let stem = std::path::Path::new(path).file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| "Untitled".into());
    if let Some((doc, saved, mut reply)) = cache.and_then(|cache| cache.read(&key)) {
        let mut fresh = Session::opened_validating(doc, Some(&saved), validation);
        // An import is still an unsaved design, with the same dirty state as a cold import.
        fresh.saved = std::sync::Arc::new(Document::new(&stem));
        reply["path"] = json!(path);
        reply["import_reused"] = json!(true);
        // A cached reply's timings belong to its original import, not this open.
        if let Some(reply) = reply.as_object_mut() {
            reply.remove("profile");
        }
        if let Some(mark) = profiled {
            let report = crate::schema::diagnostics::ProfileReport::new(&mark.report(), 20);
            reply["profile"] = serde_json::to_value(report).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
        }
        return Ok((fresh, reply));
    }
    let mut fresh = Session::new(Document::new(&stem));
    fresh.set_validation(validation);
    let sf = solidcraft_io::step_import_feature_named(&bytes, path, options.names).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    let mut reply = add_read_step(&mut fresh, path, None, &options, sf, &bytes, profiled)?;
    reply["import_reused"] = json!(false);
    if let Some(cache) = cache
        && let Some(saved) = saved_geometry(&fresh, &fresh.doc)
        && let Err(e) = cache.write(&key, &fresh.doc, &saved, &reply)
    {
        log::info!("import was not cached: {e}");
    }
    Ok((fresh, reply))
}

/// The bodies an import feature built, by name.
fn created_bodies(s: &Session, feature: u64) -> Vec<String> {
    s.model.state().bodies.iter().filter(|b| b.feature == feature).map(|b| b.name.clone()).collect()
}

/// Add a MeshImport feature for a 3MF, STL, OBJ or glTF file, in a new component named after the
/// file. A glTF file's nodes become components in it, and its materials the bodies' looks; it is
/// in metres with Y up, so unless `y_up` is false its component is turned to Z up (#1505).
fn add_mesh(
    s: &mut Session,
    path: &str,
    name: Option<&str>,
    y_up: Option<bool>,
    unit: Option<(&'static str, f64)>,
    origin: bool,
    crease_angle: Option<f64>,
) -> Result<Value> {
    let err = |e: solidcraft_io::IoError| EngineError::Other(format!("{path}: {e}"));
    let bytes = solidcraft_io::vfs::read(path, solidcraft_io::MAX_3MF_BYTES).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    let gltf = solidcraft_io::is_gltf_path(path);
    let (mut mf, plan, looks) = if gltf {
        let dir = std::path::Path::new(path).parent().map(std::path::Path::to_path_buf).unwrap_or_default();
        let mut load =
            |name: &str| solidcraft_io::vfs::read(&dir.join(name).to_string_lossy(), solidcraft_io::MAX_GLTF_BYTES).map_err(|e| e.to_string());
        let mut g = solidcraft_io::read_gltf(&bytes, path, &mut load).map_err(err)?;
        if !import::mesh_plan_fits(&g.plan) {
            g.flatten();
        }
        (g.feature, Some(g.plan), g.looks)
    } else {
        (solidcraft_io::mesh_import_feature_in(&bytes, path, unit.map_or(1.0, |(_, mm)| mm)).map_err(err)?, None, Vec::new())
    };
    if let Some(angle) = crease_angle
        && let solidcraft_doc::FeatureKind::MeshImport { meshes, .. } = &mut mf.kind
    {
        for mesh in meshes {
            if angle == 0.0 {
                // Keep the original flat display path, including its normalization
                // and storage behavior, when the caller opts out of imported shading.
                mesh.corner_normals = Default::default();
            } else {
                mesh.generate_crease_normals(angle).map_err(|e| bad("file.insert_mesh", e.to_string()))?;
            }
        }
    }
    let fname = name.filter(|n| !n.trim().is_empty()).unwrap_or(&mf.name).to_string();
    let cname = import::component_name(path, name);
    let tree = match &plan {
        Some(p) if !p.places.is_empty() => import::Tree::Mesh(p),
        _ => import::Tree::Flat,
    };
    let (id, placed) = import::add_in_component(s, mf.kind, &fname, mf.body_names, &cname, tree)?;
    let bodies = created_bodies(s, id);
    // The materials' looks, body by body (the bodies are built in the file's order).
    if looks.iter().any(Option::is_some) && bodies.len() == looks.len() {
        for (body, look) in bodies.iter().zip(looks) {
            if let Some(look) = look {
                s.doc_mut().appearances.bodies.insert(body.clone(), look);
            }
        }
        s.refresh();
    }
    let turned = if gltf && y_up.unwrap_or(true) { turn_up(s, &placed)? } else { None };
    // After the turn, so it is the stood-up mesh that sits on Z = 0.
    let moved = if origin { import::move_to_origin(s, &placed)? } else { None };
    let fname = s.doc.feature(id).map(|f| f.name.clone()).unwrap_or_default();
    let reply = MeshInserted {
        path: path.to_string(),
        feature: id,
        name: fname,
        bodies: bodies.clone(),
        created_bodies: bodies,
        warnings: mf.warnings,
        component: placed.component,
        component_name: placed.name.clone(),
        occurrence: placed.occurrence,
        subcomponents: (placed.components > 0).then_some(placed.components),
        rigid_joints: (placed.components > 0).then_some(placed.joints.len()),
        moved_to_origin: moved.map(|shift| [shift.x, shift.y, shift.z]),
        units: unit.map(|(name, _)| name.to_string()),
        stand_up: turned,
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(|e| bad("file.insert_mesh", e.to_string()))
}

/// Turn an import's component a quarter turn about X (the file's Y up becomes Z up), contents
/// and all. Returns its occurrence.
fn turn_up(s: &mut Session, placed: &import::Placed) -> Result<Option<u64>> {
    let occ = placed.occurrence.ok_or_else(|| EngineError::Other("the import has no placement to turn".into()))?;
    if let Some(o) = s.doc_mut().occurrences.iter_mut().find(|o| o.id == occ) {
        o.transform = solidcraft_doc::mat_mul(&quarter_turn_x(), &o.transform);
    }
    s.refresh();
    Ok(Some(occ))
}

/// `up` for a glTF file: `y` (the default: glTF is Y up) turns it to Z up, `z` keeps its axes.
/// Other mesh files take no `up`.
fn mesh_up(p: &Value, path: &str, cmd: &str) -> Result<Option<bool>> {
    if p.get("up").is_none_or(Value::is_null) {
        return Ok(None);
    }
    if !solidcraft_io::is_gltf_path(path) {
        return Err(bad(cmd, "`up` applies to STEP, IGES and glTF files only"));
    }
    Ok(Some(step_options(p, cmd)?.y_up))
}

/// `units` for an STL, OBJ or PLY file: the unit its coordinates are in (one of
/// `solidcraft_doc::point_cloud::UNITS`, the same as a point cloud's), which they are scaled from
/// to millimetres as the file is read. The design keeps the millimetre mesh and not the unit, so
/// importing again is how a different unit is tried. A 3MF or glTF file states its own.
fn mesh_units(p: &Value, path: &str, cmd: &str) -> Result<Option<(&'static str, f64)>> {
    let names = || solidcraft_doc::point_cloud::UNITS.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ");
    let unit = match p.get("units") {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::String(u)) => solidcraft_doc::point_cloud::UNITS
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(u))
            .map(|(name, mm)| (*name, *mm))
            .ok_or_else(|| bad(cmd, format!("`units` must be one of {} (not `{u}`)", names())))?,
        Some(_) => return Err(bad(cmd, format!("`units` must be one of {}", names()))),
    };
    if solidcraft_io::is_gltf_path(path) || path.to_ascii_lowercase().ends_with(".3mf") {
        return Err(bad(cmd, "`units` applies to STL, OBJ and PLY files only: a 3MF or glTF file states its own"));
    }
    Ok(Some(unit))
}

fn insert_mesh(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "file.insert_mesh";
    let path = path_arg(p, cmd)?;
    if !solidcraft_io::is_mesh_path(path) && !solidcraft_io::is_ply_path(path) {
        return Err(bad(cmd, "only 3MF, STL, OBJ, PLY and glTF files (.3mf, .stl, .obj, .ply, .glb, .gltf) can be inserted as meshes"));
    }
    let up = mesh_up(p, path, cmd)?;
    let unit = mesh_units(p, path, cmd)?;
    let crease_angle = mesh_crease_angle(p.get("crease_angle_degrees")).map_err(|message| bad(cmd, message))?;
    add_mesh(s, path, str_(p, "name"), up, unit, mesh_origin(p, cmd)?, crease_angle)
}

/// The shading-only crease reader, shared by the handler and its typed contract.
pub(crate) fn mesh_crease_angle(value: Option<&Value>) -> std::result::Result<Option<f64>, &'static str> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_f64()
            .filter(|v| v.is_finite() && (0.0..=180.0).contains(v))
            .map(Some)
            .ok_or("crease_angle_degrees must be between 0 and 180 degrees"),
    }
}

/// `origin`: whether a mesh import is moved to the origin (#12543). Off unless asked, so a mesh
/// keeps the coordinates its file gives it.
fn mesh_origin(p: &Value, cmd: &str) -> Result<bool> {
    match p.get("origin") {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(origin)) => Ok(*origin),
        Some(_) => Err(bad(
            cmd,
            "`origin` must be true (the mesh is moved so its box is centred on X and Y with its base on Z = 0) or false (the file's coordinates are kept)",
        )),
    }
}

fn insert_step(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "file.insert_step";
    let path = path_arg(p, cmd)?;
    if !solidcraft_io::is_step_path(path) {
        return Err(bad(cmd, "only STEP, IGES, FreeCAD and Rhino files (.step, .stp, .igs, .iges, .fcstd, .3dm) can be inserted"));
    }
    let options = step_options(p, cmd)?;
    let placement = match p.get("matrix") {
        None | Some(Value::Null) => None,
        Some(m) => Some(super::component::matrix::rigid(m, super::component::matrix::tolerance(p, cmd)?, cmd)?),
    };
    let mut out = add_step(s, path, str_(p, "name"), &options)?;
    if let Some(m) = placement {
        let occ = out["occurrence"].as_u64().ok_or_else(|| EngineError::Other("the import has no placement to move".into()))?;
        if let Some(o) = s.doc_mut().occurrences.iter_mut().find(|o| o.id == occ) {
            let placed = solidcraft_doc::mat_mul(&m, &o.transform);
            if !solidcraft_doc::is_placement(&placed) {
                return Err(bad(cmd, "`matrix` moves the import out of a rigid placement within 1e9 mm"));
            }
            o.transform = placed;
            out["transform"] = json!(o.transform);
        }
        s.refresh();
    }
    Ok(out)
}

fn open(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_arg(p, "doc.open")?;
    if !solidcraft_io::is_step_path(path) && p.get("names").is_some_and(|v| !v.is_null()) {
        return Err(bad("doc.open", "`names` applies to STEP and IGES files only"));
    }
    if !solidcraft_io::is_step_path(path) && !solidcraft_io::is_gltf_path(path) && p.get("up").is_some_and(|v| !v.is_null()) {
        return Err(bad("doc.open", "`up` applies to STEP, IGES and glTF files only"));
    }
    if !solidcraft_io::is_mesh_path(path) && p.get("origin").is_some_and(|v| !v.is_null()) {
        return Err(bad("doc.open", "`origin` applies to 3MF, STL, OBJ and glTF files only"));
    }
    if let Some(command) = super::drawing_import_command(path) {
        let stem = std::path::Path::new(path).file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| "Untitled".into());
        let mut fresh = Session::new(Document::new(&stem));
        fresh.set_validation(s.validation());
        let made: DrawingInserted = reply_from(fresh.execute(command, &json!({"path": path}))?, command)?;
        fresh.undo.clear();
        s.replace_design(fresh);
        return reply(OpenResult::Drawing(OpenedDrawing {
            sketch: made.sketch,
            created_sketch: made.created_sketch,
            curves: made.curves,
            texts: made.texts,
            info: made.info,
            path: path.to_string(),
            name: s.doc.name.clone(),
            features: s.doc.features.len(),
            errors: s.model.results.iter().filter(|r| r.error.is_some()).count(),
            param_warnings: Vec::new(),
        }));
    }
    if solidcraft_io::is_point_cloud_path(path) {
        // A new, unsaved design holding the points.
        let stem = std::path::Path::new(path).file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| "Untitled".into());
        let mut fresh = Session::new(Document::new(&stem));
        let mut q = serde_json::Map::new();
        q.insert("path".into(), json!(path));
        for k in ["units", "max_points"] {
            if let Some(v) = p.get(k) {
                q.insert(k.into(), v.clone());
            }
        }
        let r: CloudInserted = reply_from(fresh.execute("pointcloud.insert", &Value::Object(q))?, "pointcloud.insert")?;
        fresh.undo.clear();
        s.replace_design(fresh);
        return reply(OpenResult::PointCloud(OpenedPointCloud {
            path: path.to_string(),
            features: 0,
            errors: 0,
            cloud: r.cloud,
            points: r.points,
            source_points: r.source_points,
            bounds: r.bounds,
            warnings: r.warnings,
            param_warnings: Vec::new(),
        }));
    }
    if solidcraft_io::is_step_path(path) || solidcraft_io::is_mesh_path(path) {
        // A new, unsaved design holding the file's bodies.
        let stem = std::path::Path::new(path).file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| "Untitled".into());
        let (mut fresh, r) = if solidcraft_io::is_step_path(path) {
            open_step(path, p, import_cache().as_ref(), s.validation())?
        } else {
            let mut fresh = Session::new(Document::new(&stem));
            fresh.set_validation(s.validation());
            let r =
                add_mesh(&mut fresh, path, None, mesh_up(p, path, "doc.open")?, mesh_units(p, path, "doc.open")?, mesh_origin(p, "doc.open")?, None)?;
            (fresh, r)
        };
        // The design is the file: its component is the one being worked in.
        if let Some(c) = r["component"].as_u64() {
            fresh.active_component = c;
            fresh.active_occurrence_path = Some(fresh.doc.first_path(c));
        }
        fresh.undo.clear();
        s.replace_design(fresh);
        let errors = s.model.results.iter().filter(|r| r.error.is_some()).count();
        let r: ImportedFile = reply_from(r, "the import")?;
        return reply(OpenResult::File(OpenedFile {
            path: path.to_string(),
            features: s.doc.features.len(),
            errors,
            bodies: r.bodies,
            warnings: r.warnings,
            component: r.component,
            component_name: r.component_name,
            occurrence: r.occurrence,
            subcomponents: r.subcomponents,
            rigid_joints: r.rigid_joints,
            stand_up: r.stand_up,
            system: r.system,
            exporter: r.exporter,
            exporter_up: r.exporter_up,
            moved_to_origin: r.moved_to_origin,
            content_sha256: r.content_sha256,
            import_reused: r.import_reused,
            param_warnings: Vec::new(),
        }));
    }
    let read_only = bool_(p, "read_only").unwrap_or(false);
    // Stamped before it is read: a write while it is being read is a change too.
    let disk = solidcraft_io::design_lock::stamp(path);
    let (doc, saved) = read_design(s, path)?;
    let opened = Session::opened_at_validating(doc, saved.as_ref(), Some(path), s.validation());
    opened_design(s, opened, path, read_only, disk)
}

/// Read the design file at `path` for `doc.open`, under its file's name if it has none of its own.
fn read_design(s: &mut Session, path: &str) -> Result<(Document, Option<solidcraft_doc::SavedModel>)> {
    let (mut doc, saved) = solidcraft_io::read_design_file(path, s.open_preview.take()).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    if let Some(stem) = default_name_for(&doc.name, path) {
        doc.name = stem;
    }
    Ok((doc, saved))
}

/// `doc.open` of a design: `opened`, the design read from `path` and evaluated, takes `s`'s
/// place, and the reply says what it is.
fn opened_design(
    s: &mut Session,
    opened: Session,
    path: &str,
    read_only: bool,
    disk: Option<solidcraft_io::design_lock::FileStamp>,
) -> Result<Value> {
    s.replace_design(opened);
    s.disk = disk;
    s.read_only = read_only;
    // A design saved with print faces (by an older version, or with them kept) opens without
    // them when they are not kept (#11512). The rest of it opens as saved, and dropping them is
    // neither an unsaved change nor a step to undo.
    if !s.keep_print_faces && !s.doc.print_faces.is_empty() {
        s.doc_mut().print_faces.clear();
        s.mark_saved();
    }
    let errors = s.model.results.iter().filter(|r| r.error.is_some()).count();
    let now = solidcraft_io::design_lock::now_secs();
    let open_elsewhere = solidcraft_io::design_lock::held_elsewhere(std::path::Path::new(path), now).map(|lock| OpenElsewhere {
        host: lock.host,
        user: lock.user,
        app: lock.app,
        opened: lock.opened,
    });
    reply(OpenResult::Design(OpenedDesign {
        path: path.to_string(),
        features: s.doc.features.len(),
        errors,
        name: s.doc.name.clone(),
        read_only: read_only.then_some(true),
        open_elsewhere,
        newer_file: s.doc.newer_file.as_ref().map(|file| NewerFileReport {
            format: file.format,
            reads_up_to: file.reads_up_to,
            unknown_changes: file.unknown_changes.clone(),
            not_understood: file.not_understood.clone(),
            notice: file.notice(),
        }),
        warnings: s.doc.newer_file.as_ref().map(|file| file.warnings()),
        param_warnings: Vec::new(),
    }))
}

/// `doc.open`'s reply, as JSON.
fn reply(result: OpenResult) -> Result<Value> {
    serde_json::to_value(result).map_err(|e| EngineError::Other(format!("doc.open: {e}")))
}

/// What `doc.open` takes from the reply of the command that did the work (`from`).
fn reply_from<T: serde::de::DeserializeOwned>(reply: Value, from: &str) -> Result<T> {
    serde_json::from_value(reply).map_err(|e| EngineError::Other(format!("doc.open: the reply of {from}: {e}")))
}

/// What `sketch.insert_dxf` and `sketch.insert_svg` reply, as far as `doc.open` passes it on.
#[derive(serde::Deserialize)]
struct DrawingInserted {
    sketch: u64,
    #[serde(default)]
    created_sketch: Option<u64>,
    curves: usize,
    texts: usize,
    #[serde(default)]
    info: Value,
}

/// What `pointcloud.insert` replies, as far as `doc.open` passes it on.
#[derive(serde::Deserialize)]
struct CloudInserted {
    cloud: u64,
    points: usize,
    source_points: u64,
    #[serde(default)]
    bounds: Option<[[f64; 3]; 2]>,
    warnings: Vec<String>,
}

/// What an import (`add_step`, `add_mesh`) replies, as far as `doc.open` passes it on.
#[derive(serde::Deserialize)]
struct ImportedFile {
    bodies: Vec<String>,
    warnings: Vec<String>,
    component: u64,
    component_name: String,
    #[serde(default)]
    occurrence: Option<u64>,
    #[serde(default)]
    subcomponents: Option<usize>,
    #[serde(default)]
    rigid_joints: Option<usize>,
    #[serde(default)]
    stand_up: Option<u64>,
    #[serde(default)]
    system: Option<String>,
    #[serde(default)]
    exporter: Option<String>,
    #[serde(default)]
    exporter_up: Option<String>,
    #[serde(default)]
    moved_to_origin: Option<Vec<f64>>,
    #[serde(default)]
    content_sha256: Option<String>,
    #[serde(default)]
    import_reused: Option<bool>,
}

/// The name a design never given one by its user is shown under.
const DEFAULT_NAME: &str = "Untitled";

/// A design still under the default name takes its file's name (`bracket.sc` →
/// "bracket"); a name the user chose is kept. `None` when nothing changes.
fn default_name_for(name: &str, path: &str) -> Option<String> {
    let name = name.trim();
    if !name.is_empty() && name != DEFAULT_NAME {
        return None;
    }
    let stem = std::path::Path::new(path).file_stem()?.to_string_lossy().trim().to_string();
    (!stem.is_empty()).then_some(stem)
}

/// The name a design takes when saved to `path`, if it changes. A design with no name of its
/// own takes the file's ([`default_name_for`]). Otherwise a name the user chose is kept, while
/// a borrowed one — taken from the file it was saved as before (#1710), from a sample, or from
/// a file that stored another design's name — follows a Save As to the new file's name, so
/// after Save As the open design *is* the new file (#4907). A plain Save to the same file
/// never renames.
fn saved_name(name: &str, old_path: Option<&str>, path: &str, user_named: bool) -> Option<String> {
    if let Some(stem) = default_name_for(name, path) {
        return Some(stem);
    }
    if user_named {
        return None;
    }
    let stem = |p: &str| std::path::Path::new(p).file_stem().map(|x| x.to_string_lossy().trim().to_string()).filter(|x| !x.is_empty());
    let new = stem(path)?;
    (old_path != Some(path) && name.trim() != new).then_some(new)
}

/// Rename the design, and the undo and redo states that still carry the old name, so undo
/// does not bring the default name back and leave the design looking changed.
fn rename_design(s: &mut Session, from: &str, to: &str) {
    // A prepared world is keyed by the document's Arc. Changing only its name
    // must not make an opted-in save lose warm geometry and display caches.
    // Retag only a cache that already matches: cold or stale placements stay so.
    let model = s.model.state();
    let keep_world = s.world_cache.lock().ok().is_some_and(|cache| cache.as_ref().is_some_and(|cache| cache.matches(s, &model)));
    s.doc_mut().name = to.to_string();
    // An open sketch's entry finds its place in the history by these states' addresses
    // (Finish folds the edit, Cancel checks the history): it follows the renamed copies.
    let mut renamed: Vec<(*const solidcraft_doc::Document, std::sync::Arc<solidcraft_doc::Document>)> = Vec::new();
    for snap in s.undo.iter_mut().chain(s.redo.iter_mut()) {
        if snap.doc.name == from {
            let old = std::sync::Arc::as_ptr(&snap.doc);
            std::sync::Arc::make_mut(&mut snap.doc).name = to.to_string();
            renamed.push((old, snap.doc.clone()));
        }
    }
    if let Some(entry) = s.sketch_entry.as_mut() {
        let follow = |doc: &mut std::sync::Arc<solidcraft_doc::Document>| match renamed.iter().find(|(old, _)| *old == std::sync::Arc::as_ptr(doc)) {
            Some((_, new)) => *doc = new.clone(),
            None if doc.name == from => std::sync::Arc::make_mut(doc).name = to.to_string(),
            None => {}
        };
        follow(&mut entry.doc);
        if let Some(top) = entry.undo_top.as_mut() {
            follow(top);
        }
    }
    if keep_world
        && let Ok(mut cache) = s.world_cache.lock()
        && let Some(cache) = cache.as_mut()
    {
        cache.doc = std::sync::Arc::clone(&s.doc);
    }
}

/// The kind of file `doc.open` reads `path` as when that is not a design, by its extension. A
/// design saved under such a name would not open again as the design (#9745).
fn other_file_kind(path: &str) -> Option<&'static str> {
    if solidcraft_io::is_step_path(path) {
        Some("a STEP or other CAD exchange file")
    } else if solidcraft_io::is_mesh_path(path) {
        Some("a mesh")
    } else if super::drawing_import_command(path).is_some() {
        Some("a 2D drawing")
    } else if solidcraft_io::is_point_cloud_path(path) {
        Some("a point cloud")
    } else if solidcraft_io::is_drawing_path(path) {
        Some("a SolidCraft drawing")
    } else {
        None
    }
}

/// Refuse to save a design under a name [`other_file_kind`] opens as another kind of file,
/// pointing to `file.export` when that writes the format.
fn check_design_name(cmd: &str, path: &str) -> Result<()> {
    let Some(kind) = other_file_kind(path) else { return Ok(()) };
    let ext = std::path::Path::new(path).extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
    let export = if solidcraft_io::Format::from_name(&ext).is_ok() { format!(", or write a .{ext} file with file.export") } else { String::new() };
    Err(bad(cmd, format!("{path}: a .{ext} file opens as {kind}, not as a design; save the design as .{}{export}", solidcraft_io::DESIGN_EXT)))
}

#[allow(clippy::too_many_arguments)]
fn write(s: &mut Session, cmd: &str, path: &str, format: &str, compression: &str, preview: bool, cache: bool, overwrite: bool) -> Result<Value> {
    // A path outside the agent's folders says so first, whatever its name.
    solidcraft_io::vfs::check(path).map_err(|e| EngineError::Other(e.to_string()))?;
    check_design_name(cmd, path)?;
    // A move not captured yet isn't in the document a file holds: refuse, rather than write the
    // file without it and say nothing (#9736). The app asks the Capture Position question first.
    // An exploded view shown saves as before: its placements aren't the design's.
    if s.moved_uncaptured() {
        let moved: Vec<&str> = s.doc.occurrences.iter().filter(|o| s.pending_moves.contains_key(&o.id)).map(|o| o.name.as_str()).collect();
        return Err(EngineError::Other(format!(
            "{path}: {} moved and not captured yet; component.capture_position keeps the move and component.revert_position puts it back, then save",
            moved.join(", ")
        )));
    }
    if s.path.as_deref() == Some(path) {
        if s.read_only {
            return Err(EngineError::Other(format!("{path}: the design was opened read-only; save it to another file with file.save_as")));
        }
        if !overwrite && s.disk_changed() {
            return Err(EngineError::Other(format!(
                "{path}: the file was changed by another program or computer since it was opened or last saved; save to another file with file.save_as, or pass `overwrite: true` to replace it"
            )));
        }
    }
    // Saving an unnamed or borrowed-named design to a new file names it after that file
    // (not an undoable edit); a name the user chose is kept (#4907).
    let renamed = saved_name(&s.doc.name, s.path.as_deref(), path, s.user_named).map(|stem| (s.doc.name.clone(), stem));
    if let Some((from, to)) = &renamed {
        rename_design(s, from, to);
    }
    let r = write_file(s, path, format, compression, preview, cache);
    if r.is_err()
        && let Some((from, to)) = &renamed
    {
        rename_design(s, to, from);
    }
    r
}

fn write_file(s: &mut Session, path: &str, format: &str, compression: &str, preview: bool, cache: bool) -> Result<Value> {
    let compression = match compression {
        "fast" => solidcraft_io::DesignCompression::Fast,
        "compact" => solidcraft_io::DesignCompression::Compact,
        _ => return Err(bad("file.save", "compression must be fast or compact")),
    };
    write_bytes(s, path, format, compression, preview, cache)
}

fn write_bytes(
    s: &mut Session,
    path: &str,
    format: &str,
    compression: solidcraft_io::DesignCompression,
    preview: bool,
    cache: bool,
) -> Result<Value> {
    let bytes = match format {
        "container" => s.design_container_at(Some(path), compression, preview, cache)?,
        "json" => solidcraft_io::write_design(&s.design_at(Some(path))),
        _ => return Err(bad("file.save", "format must be container or json")),
    };
    // Cancelled while the design was being prepared (a watched save): nothing is written.
    if solidcraft_kernel::watch::current().is_some_and(|w| w.cancelled()) {
        return Err(EngineError::Other(format!("{path}: save cancelled")));
    }
    // Atomic, keeping the version being replaced as `<file>.bak`; a watched save reports how far
    // the write is and stops on Cancel until the file is replaced.
    solidcraft_io::write_atomic(std::path::Path::new(path), &bytes, true).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    s.path = Some(path.to_string());
    s.disk = solidcraft_io::design_lock::stamp(path);
    s.read_only = false;
    s.mark_saved();
    let mut out = SaveResult {
        path: path.to_string(),
        bytes: bytes.len(),
        name: s.doc.name.clone(),
        kept_format: None,
        warnings: None,
        not_kept: None,
        param_warnings: Vec::new(),
    };
    // A newer SolidCraft's file keeps what this version can't read, and its format number. What
    // couldn't go back, because the design was changed around it, is said.
    if s.doc.newer_file.is_some() {
        let mut lost = solidcraft_doc::newer_file::not_kept(&s.doc);
        if format == "json" && s.doc.newer_file.as_ref().is_some_and(|file| file.needs_container()) {
            lost.push("the parts of the file beside the design, which plain JSON has no room for: save as .sc to keep them".to_string());
        }
        out.kept_format = Some(s.doc.format.clone());
        if !lost.is_empty() {
            out.warnings = Some(lost.iter().map(|what| format!("Not kept: {what}. The previous file is `{path}.bak`.")).collect());
            out.not_kept = Some(lost);
        }
    }
    serde_json::to_value(out).map_err(|e| bad("file.save", e.to_string()))
}

impl Session {
    /// Open a design with its file location, resolving component links before history keeps
    /// them. Absolute in-memory targets survive Save As, undo, redo and recovery unchanged.
    pub fn opened_at(doc: Document, saved: Option<&solidcraft_doc::SavedModel>, path: Option<&str>) -> Self {
        Self::opened_at_validating(doc, saved, path, solidcraft_kernel::Validation::default())
    }

    /// [`Self::opened_at`], its features evaluated under `validation` ([`Session::set_validation`]).
    pub fn opened_at_validating(
        mut doc: Document,
        saved: Option<&solidcraft_doc::SavedModel>,
        path: Option<&str>,
        validation: solidcraft_kernel::Validation,
    ) -> Self {
        super::component_link::resolve_design_links(&mut doc, path);
        Self::opened_validating(doc, saved, validation).located(path)
    }

    /// This session, just opened, as the design in the file at `path`.
    pub(crate) fn located(self, path: Option<&str>) -> Self {
        let mut session = self;
        session.path = path.map(str::to_owned);
        // A design saved before body names were pinned (#6403) is pinned now, as its first
        // command would pin it: that command pinned them as part of itself, so undoing it went
        // back to the unpinned design, whose fingerprints no evaluated result matched, and the
        // first undo after opening evaluated the whole timeline again (82-93 s in a
        // 5,484-feature design). The pinned names are the ones the file's bodies have, so
        // opening is no change: the pinned design is the saved one.
        let unpinned = session.doc.reserved_body_names.is_empty() && session.doc.features.iter().all(|f| f.body_names.is_empty());
        let before = session.doc.clone();
        session.pin_body_names();
        if unpinned || !std::sync::Arc::ptr_eq(&before, &session.doc) {
            session.pin_rolled_back_body_names();
        }
        session.saved = session.doc.clone();
        session
    }

    /// Pin the names of the bodies the features after the timeline marker make, in a design
    /// saved rolled back before names were pinned (#14008). Those features were never evaluated,
    /// so [`Session::pin_body_names`] left them unpinned: a body then made at the marker took the
    /// number of a later body, every later counted body shifted, and a combine that names them
    /// failed or joined another body. The names are the ones the whole timeline gives, which is
    /// what the design called those bodies when it was saved. The design stays rolled back.
    ///
    /// The whole timeline is evaluated on a scratch copy, and only the names it pins come back:
    /// refreshing this session there would also work out what the design reads from the later
    /// geometry (a gear's ratio from a face after the marker, joint placements) and keep it.
    fn pin_rolled_back_body_names(&mut self) {
        if !self.doc.marker.is_some_and(|m| m < self.doc.features.len()) {
            return;
        }
        let mut whole = self.scratch();
        whole.doc_mut().marker = None;
        whole.model.evaluate_shared(&whole.doc);
        whole.pin_body_names();
        let pins: Vec<(u64, Vec<String>)> = whole
            .doc
            .features
            .iter()
            .filter(|f| self.doc.feature(f.id).is_some_and(|mine| mine.body_names != f.body_names))
            .map(|f| (f.id, f.body_names.clone()))
            .collect();
        let reserve: Vec<String> = whole.doc.reserved_body_names.difference(&self.doc.reserved_body_names).cloned().collect();
        if pins.is_empty() && reserve.is_empty() {
            return;
        }
        // As `pin_body_names` stores names: the bodies keep the names they have, so no result
        // changes and none is evaluated again.
        let renamed: Vec<u64> = pins.iter().map(|(id, _)| *id).collect();
        let keep = self.model.pinned_prefix(&self.doc, &renamed);
        let doc = self.doc_mut();
        for (id, names) in pins {
            if let Some(f) = doc.feature_mut(id) {
                f.body_names = names;
            }
        }
        doc.reserved_body_names.extend(reserve);
        self.model.refingerprint_shared(&self.doc, keep);
    }

    /// Prepare only the file's copy with relative paths; saving never rewrites live history.
    pub(crate) fn design_at(&self, path: Option<&str>) -> std::borrow::Cow<'_, Document> {
        let mut design = std::borrow::Cow::Borrowed(&*self.doc);
        if let Some(path) = path {
            let links = super::design_link::rebased(&self.doc, self.path.as_deref(), path);
            if links != self.doc.links {
                design.to_mut().links = links;
            }
        }
        if !self.visibility.is_empty() {
            design.to_mut().visibility = self.visibility.clone();
        }
        // Print faces not kept stay with this session: the file is saved without them (#11512).
        if !self.keep_print_faces && !design.print_faces.is_empty() {
            design.to_mut().print_faces.clear();
        }
        // A sketch edited with the timeline rolled to it: the file holds the position the edit
        // returns to, not the temporary rollback (#14010).
        if let Some(marker) = self.sketch_restore_marker() {
            let marker = marker.filter(|m| *m < design.features.len());
            if design.marker != marker {
                design.to_mut().marker = marker;
            }
        }
        design
    }

    /// Take up a save that ran on `done`, a [`Session::scratch`] copy of this session that
    /// `file.save` or `file.save_as` ran on (on another thread, so the window keeps drawing):
    /// the name the save gave the design, its file, and its saved state, as running the command
    /// here would have left them. What is saved is the design the file holds, `done`'s, not this
    /// one's: a change made here while the save was written is not in the file, so it stays
    /// unsaved (#6003).
    pub fn adopt_save(&mut self, done: &Session) {
        if done.doc.name != self.doc.name {
            let (from, to) = (self.doc.name.clone(), done.doc.name.clone());
            rename_design(self, &from, &to);
        }
        self.path.clone_from(&done.path);
        self.disk = done.disk;
        self.read_only = done.read_only;
        self.saved = done.saved.clone();
        self.saved_visibility = done.saved_visibility.clone();
    }

    /// The design's file was renamed (browser OPFS rename, or an equivalent host move): keep the
    /// open session on the new path, and when the design name was borrowed from the file, follow
    /// it so the tab title matches the file list (#11375). A name the user chose is kept, as on
    /// Save As.
    pub fn follow_file_rename(&mut self, old_path: &str, new_path: &str) {
        if self.path.as_deref() != Some(old_path) || old_path == new_path {
            return;
        }
        if let Some(stem) = saved_name(&self.doc.name, Some(old_path), new_path, self.user_named) {
            let from = self.doc.name.clone();
            rename_design(self, &from, &stem);
        }
        self.path = Some(new_path.to_string());
    }

    /// Another program or computer wrote the design's file since it was opened or last saved:
    /// saving to it would replace that work (#5467). A file that is gone has not changed.
    pub fn disk_changed(&self) -> bool {
        match (&self.path, &self.disk) {
            (Some(path), Some(known)) => solidcraft_io::design_lock::changed_since(path, known),
            _ => false,
        }
    }

    /// Container bytes for desktop and browser saves. Only already evaluated bodies
    /// and warm meshes are eligible; saving never forces evaluation or tessellation.
    pub fn design_container(&self, compression: solidcraft_io::DesignCompression, preview: bool, cache: bool) -> solidcraft_io::Result<Vec<u8>> {
        self.design_container_at(self.path.as_deref(), compression, preview, cache)
    }

    fn design_container_at(
        &self,
        path: Option<&str>,
        compression: solidcraft_io::DesignCompression,
        preview: bool,
        cache: bool,
    ) -> solidcraft_io::Result<Vec<u8>> {
        let design = self.design_at(path);
        let geometry = cache.then(|| saved_geometry(self, &design)).flatten();
        let preview = preview.then(|| saved_preview(self)).flatten();
        let thumbnail = cache.then(|| thumbnail::saved(self)).flatten();
        solidcraft_io::write_design_container_with_thumbnail(&design, compression, preview.as_ref(), geometry.as_ref(), thumbnail.as_deref())
    }
}

/// The evaluated model a save keeps, so opening need not rebuild it: only the finished evaluation
/// of `design`, the document saved (no sketch being edited, no uncaptured moves, no feature errors).
fn saved_geometry(s: &Session, design: &Document) -> Option<solidcraft_doc::SavedModel> {
    if s.active_sketch.is_some() || !s.pending_moves.is_empty() || s.model.state().bodies.is_empty() {
        return None;
    }
    s.model.saved(design)
}

/// Cache only warm display geometry. A save never waits for an unevaluated mesh.
fn saved_preview(s: &Session) -> Option<solidcraft_io::DesignPreview> {
    if s.active_sketch.is_some() || !s.pending_moves.is_empty() || s.model.results.iter().any(|r| r.error.is_some()) {
        return None;
    }
    let world = s.cached_world_state()?;
    if world.bodies.is_empty() || world.bodies.len() > 1024 {
        return None;
    }
    let mut preview = solidcraft_io::DesignPreview::default();
    let mut bytes = 0usize;
    let mut triangles = 0usize;
    for body in &world.bodies {
        let mesh = body.cached_mesh()?;
        bytes =
            bytes.saturating_add(mesh.positions.len().saturating_mul(24)).saturating_add(mesh.triangles.len().saturating_mul(84)).saturating_add(32);
        triangles = triangles.saturating_add(mesh.triangles.len());
        if bytes > 8 << 20 || triangles > 100_000 {
            return None;
        }
        preview.meshes.push(solidcraft_doc::MeshData {
            name: body.name.clone(),
            positions: mesh.positions.iter().flat_map(|p| [p.x, p.y, p.z]).collect(),
            triangles: mesh.triangles.iter().flatten().copied().collect(),
            color: s.doc.body_color(body),
            corner_normals: mesh.triangles.iter().flatten().filter_map(|&i| mesh.normals.get(i as usize)).flat_map(|n| [n.x, n.y, n.z]).collect(),
        });
    }
    Some(preview)
}

fn save(s: &mut Session, p: &Value) -> Result<Value> {
    let path = match str_(p, "path") {
        Some(x) if !x.trim().is_empty() => x.to_string(),
        _ => s.path.clone().ok_or_else(|| bad("file.save", "the design has no file yet: give a `path`"))?,
    };
    write(
        s,
        "file.save",
        &path,
        str_(p, "format").unwrap_or(default_design_format(&path)),
        str_(p, "compression").unwrap_or("fast"),
        bool_(p, "preview").unwrap_or(false),
        bool_(p, "cache").unwrap_or(true),
        bool_(p, "overwrite").unwrap_or(false),
    )
}

fn default_design_format(path: &str) -> &'static str {
    if std::path::Path::new(path).extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("json")) { "json" } else { "container" }
}

fn save_as(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_arg(p, "file.save_as")?.to_string();
    write(
        s,
        "file.save_as",
        &path,
        str_(p, "format").unwrap_or(default_design_format(&path)),
        str_(p, "compression").unwrap_or("fast"),
        bool_(p, "preview").unwrap_or(false),
        bool_(p, "cache").unwrap_or(true),
        bool_(p, "overwrite").unwrap_or(false),
    )
}

/// A mesh export's `quality`: standard (as every export was) unless asked for (#4747).
fn mesh_quality(p: &Value, cmd: &str) -> Result<solidcraft_io::MeshQuality> {
    match p.get("quality") {
        None | Some(Value::Null) => Ok(solidcraft_io::MeshQuality::default()),
        Some(v) => v
            .as_str()
            .and_then(solidcraft_io::MeshQuality::from_name)
            .ok_or_else(|| bad(cmd, format!("`quality` must be standard, fine or finest, not {v}"))),
    }
}

/// The 3MF parts a mesh export's `objects` asks for (#8139): none for `body` (the default, one
/// object per body), or for `component` one part per placement of a component with two or more
/// bodies ([`component_parts`]). Other formats have no parts, so `component` is refused there.
pub(super) fn mesh_parts(s: &Session, p: &Value, cmd: &str, format: Format) -> Result<Vec<(String, Vec<String>)>> {
    match p.get("objects") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(v) => match v.as_str() {
            Some("body") => Ok(Vec::new()),
            Some("component") if matches!(format, Format::ThreeMf | Format::ThreeMfTagged) => Ok(component_parts(s)),
            Some("component") => Err(bad(cmd, "`objects: component` is for 3MF: other formats have no parts")),
            _ => Err(bad(cmd, format!("`objects` must be body or component, not {v}"))),
        },
    }
}

/// Each placement of a component that holds two or more bodies, by its occurrence's name, with
/// the names its bodies have where they are placed (as the export names them). The design's own
/// bodies and a component's single body are left out: they stay objects of their own.
fn component_parts(s: &Session) -> Vec<(String, Vec<String>)> {
    let doc = &s.doc;
    if doc.occurrences.is_empty() {
        return Vec::new();
    }
    let st = s.model.state();
    let owners = doc.model_body_components(&st);
    let names = solidcraft_doc::WorldNames::new(doc, &st);
    doc.placements()
        .into_iter()
        .filter(|(comp, _, _)| *comp != 0)
        .filter_map(|(comp, path, _)| {
            let bodies: Vec<String> =
                st.bodies.iter().zip(&owners).filter(|(_, owner)| **owner == comp).map(|(b, _)| names.name(&b.name, comp, &path)).collect();
            let name = path.last().and_then(|id| doc.occurrences.iter().find(|o| o.id == *id)).map(|o| o.name.clone()).unwrap_or_default();
            (bodies.len() >= 2).then_some((name, bodies))
        })
        .collect()
}

fn export_to(
    s: &Session,
    cmd: &str,
    path: &str,
    format: Format,
    bodies: &[String],
    parts: &[(String, Vec<String>)],
    quality: solidcraft_io::MeshQuality,
) -> Result<Value> {
    if format == Format::Obj {
        // The colours of the bodies go in a material library beside the mesh file.
        return export_obj(s, path, bodies, quality);
    }
    if format == Format::Glb {
        return export_glb(s, cmd, path, bodies, quality);
    }
    let name = std::path::Path::new(path).file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| s.doc.name.clone());
    // A design with components exports as a STEP assembly; otherwise bodies as placed.
    let bytes = if format == Format::Design {
        if !bodies.is_empty() {
            return Err(bad(cmd, "design JSON exports the complete document; omit bodies"));
        }
        solidcraft_io::write_design(&s.design())
    } else if format == Format::Step && bodies.is_empty() && !s.doc.occurrences.is_empty() {
        solidcraft_io::step_assembly(&s.doc, &s.doc.painted(&s.model.state()), &name)?
    } else {
        // Bodies carry the colour they are shown in (appearance, material or imported), and the
        // file the design's properties where the format has a place for them.
        solidcraft_io::export_with_parts(
            &s.doc.painted(&*s.world_state()?),
            bodies,
            parts,
            format,
            &name,
            &s.doc.metadata,
            quality,
            export_thumbnail(s, format, bodies).as_deref(),
        )?
    };
    solidcraft_io::vfs::write(path, &bytes).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    let reply = ExportResult { path: path.to_string(), bytes: bytes.len(), format: format!("{format:?}"), ..ExportResult::default() };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

/// A picture of the part, drawn only for a format that has a place for it.
fn export_thumbnail(s: &Session, format: Format, bodies: &[String]) -> Option<Vec<u8>> {
    matches!(format, Format::ThreeMf | Format::ThreeMfTagged).then(|| thumbnail::exported(s, bodies)).flatten()
}

/// `lay_on_print_faces` as the mesh exports read it: true writes each body with a print face laid
/// on it, as a slicer export does. Only STL and 3MF take it; `false` is accepted for any format.
fn lay_on_print_faces(p: &Value, cmd: &str, format: Format) -> Result<bool> {
    match p.get("lay_on_print_faces") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => Ok(false),
        Some(Value::Bool(true)) if matches!(format, Format::StlBinary | Format::StlAscii | Format::ThreeMf | Format::ThreeMfTagged) => Ok(true),
        Some(Value::Bool(true)) => Err(bad(cmd, "`lay_on_print_faces` is for STL and 3MF: other formats are not printed")),
        Some(_) => Err(bad(cmd, "`lay_on_print_faces` must be true or false")),
    }
}

/// Write the bodies as a mesh file (STL or 3MF), each laid on its print face: the file's size, and
/// which bodies were laid and which were written as they lie.
fn write_laid(
    s: &Session,
    path: &str,
    format: Format,
    bodies: &[String],
    parts: &[(String, Vec<String>)],
    quality: solidcraft_io::MeshQuality,
) -> Result<(usize, print_face::Laid)> {
    let name = std::path::Path::new(path).file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| s.doc.name.clone());
    let mut state = s.doc.painted(&*s.world_state()?);
    let laid = print_face::lay_on_print_faces(s, &mut state, bodies, parts);
    let thumbnail = export_thumbnail(s, format, bodies);
    let bytes = solidcraft_io::export_with_parts(&state, bodies, parts, format, &name, &s.doc.metadata, quality, thumbnail.as_deref())?;
    solidcraft_io::vfs::write(path, &bytes).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    Ok((bytes.len(), laid))
}

/// [`export_to`] for `file.export`, `file.export_stl` and `file.export_3mf` with
/// `lay_on_print_faces`: each body laid on its print face.
fn export_laid(
    s: &Session,
    path: &str,
    format: Format,
    bodies: &[String],
    parts: &[(String, Vec<String>)],
    quality: solidcraft_io::MeshQuality,
) -> Result<Value> {
    let (bytes, laid) = write_laid(s, path, format, bodies, parts, quality)?;
    let reply = ExportResult {
        path: path.to_string(),
        bytes,
        format: format!("{format:?}"),
        print_faces: laid.bodies,
        warnings: laid.warnings,
        ..ExportResult::default()
    };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

/// [`export_to`] for a mesh file a slicer is sent, or `file.save_mesh` with `lay_on_print_faces`
/// (STL or 3MF): each body laid on its print face.
fn export_laid_to(
    s: &Session,
    path: &str,
    format: Format,
    bodies: &[String],
    parts: &[(String, Vec<String>)],
    quality: solidcraft_io::MeshQuality,
) -> Result<Value> {
    let (bytes, laid) = write_laid(s, path, format, bodies, parts, quality)?;
    let reply = SaveMeshResult {
        path: path.to_string(),
        bytes,
        format: format!("{format:?}"),
        print_faces: laid.bodies,
        warnings: laid.warnings,
        delivery: SlicerDelivery::default(),
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

fn export(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "file.export";
    let path = path_arg(p, cmd)?;
    let format = Format::from_name(str_(p, "format").unwrap_or(path))?;
    if format == Format::Urdf {
        if !string_list(cmd, p, "bodies")?.is_empty() {
            return Err(bad(cmd, "urdf exports the whole assembly; omit bodies"));
        }
        return export_urdf(s, cmd, path, p);
    }
    let quality = mesh_quality(p, cmd)?;
    let parts = mesh_parts(s, p, cmd, format)?;
    let bodies = string_list(cmd, p, "bodies")?;
    if lay_on_print_faces(p, cmd, format)? {
        return export_laid(s, path, format, &bodies, &parts, quality);
    }
    export_to(s, cmd, path, format, &bodies, &parts, quality)
}

fn export_step(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "file.export_step";
    let path = path_arg(p, cmd)?;
    export_to(s, cmd, path, Format::Step, &string_list(cmd, p, "bodies")?, &[], solidcraft_io::MeshQuality::default())
}

fn export_stl(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "file.export_stl";
    let path = path_arg(p, cmd)?;
    let (bodies, quality) = (string_list(cmd, p, "bodies")?, mesh_quality(p, cmd)?);
    if lay_on_print_faces(p, cmd, Format::StlBinary)? {
        return export_laid(s, path, Format::StlBinary, &bodies, &[], quality);
    }
    export_to(s, cmd, path, Format::StlBinary, &bodies, &[], quality)
}

fn export_3mf(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "file.export_3mf";
    let path = path_arg(p, cmd)?;
    let parts = mesh_parts(s, p, cmd, Format::ThreeMf)?;
    let (bodies, quality) = (string_list(cmd, p, "bodies")?, mesh_quality(p, cmd)?);
    if lay_on_print_faces(p, cmd, Format::ThreeMf)? {
        return export_laid(s, path, Format::ThreeMf, &bodies, &parts, quality);
    }
    export_to(s, cmd, path, Format::ThreeMf, &bodies, &parts, quality)
}

fn export_obj_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "file.export_obj";
    let path = path_arg(p, cmd)?;
    export_to(s, cmd, path, Format::Obj, &string_list(cmd, p, "bodies")?, &[], mesh_quality(p, cmd)?)
}

fn export_urdf_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "file.export_urdf";
    export_urdf(s, cmd, path_arg(p, cmd)?, p)
}

/// The OBJ export: the mesh file and the material library beside it, with the colours of the
/// bodies. Every path is checked before anything is written; the library goes first and the
/// mesh file last, so a mesh file never names a library that is not there.
fn export_obj(s: &Session, path: &str, bodies: &[String], quality: solidcraft_io::MeshQuality) -> Result<Value> {
    let mtl_path = std::path::Path::new(path).with_extension("mtl");
    let mtl_name = mtl_path.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| String::from("model.mtl"));
    let files = solidcraft_io::export_obj(&s.doc.painted(&*s.world_state()?), bodies, &mtl_name, quality)?;
    let mut targets: Vec<(String, &[u8])> = Vec::new();
    if !files.mtl.is_empty() {
        targets.push((mtl_path.to_string_lossy().to_string(), files.mtl.as_bytes()));
    }
    targets.push((path.to_string(), files.obj.as_bytes()));
    for (t, _) in &targets {
        solidcraft_io::vfs::check(t).map_err(|e| EngineError::Other(format!("{t}: {e}")))?;
    }
    let mut written: Vec<String> = Vec::new();
    for (t, bytes) in &targets {
        if let Err(e) = solidcraft_io::vfs::write(t, bytes) {
            let before = if written.is_empty() { "none".to_string() } else { written.join(", ") };
            return Err(EngineError::Other(format!("{t}: {e} (this file may be incomplete; written before it: {before})")));
        }
        written.push(t.clone());
    }
    Ok(json!({"path": path, "bytes": files.obj.len(), "format": format!("{:?}", Format::Obj), "files": written}))
}

/// The GLB export (#1553): the bodies as placed, with the looks the design shows them in.
/// Written atomically, so a refusal or a failed write leaves a file already there as it was. A
/// body a render setup gives an image texture or optics is refused: the file has no place for
/// them, and writing the body without them would show it wrongly.
fn export_glb(s: &Session, cmd: &str, path: &str, bodies: &[String], quality: solidcraft_io::MeshQuality) -> Result<Value> {
    solidcraft_io::vfs::check(path).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    let name = std::path::Path::new(path).file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| s.doc.name.clone());
    let world = s.world_state()?;
    let bound = super::render::bound_bodies(s, cmd)?;
    for b in world.bodies.iter().filter(|b| bodies.is_empty() || bodies.contains(&b.name)) {
        if let Some((setup, what)) = bound.get(&b.name) {
            return Err(bad(
                cmd,
                format!(
                    "{}: render setup `{setup}` gives it {what}, which GLB export does not write yet; remove it from the setup, or leave the body out with `bodies`",
                    b.name
                ),
            ));
        }
    }
    let bytes = solidcraft_io::export_glb(&s.doc, &s.model.state(), &world, bodies, &name, quality)?;
    solidcraft_io::write_atomic(std::path::Path::new(path), &bytes, false).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    Ok(json!({"path": path, "bytes": bytes.len(), "format": format!("{:?}", Format::Glb)}))
}

/// The assembly as a URDF and its meshes next to it (docs/urdf.md). Every path is checked before
/// anything is written; the meshes go first and the `.urdf` last.
fn export_urdf(s: &Session, cmd: &str, path: &str, p: &Value) -> Result<Value> {
    let positive = |k: &str| match p.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => num(p, k).filter(|x| *x > 0.0).map(Some).ok_or_else(|| bad(cmd, format!("`{k}` must be a positive number"))),
    };
    // `io::urdf` refuses a prefix XML cannot hold.
    let mesh_prefix = str_(p, "mesh_prefix").unwrap_or("").to_string();
    let options = solidcraft_io::UrdfOptions { mesh_prefix, effort: positive("effort")?, velocity: positive("velocity")? };
    let file = std::path::Path::new(path);
    let stem = file.file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| s.doc.name.clone());
    // Occurrences where they are shown, uncaptured moves included, except those the joint solver
    // places: it puts them back as soon as a joint moves, so the move must not become part of
    // their links.
    let st = s.model.state();
    let mut held = Vec::new();
    // The exploded view is shown only: the links are where the design places them.
    let moves = s.uncaptured_moves();
    let doc = if moves.is_empty() {
        std::borrow::Cow::Borrowed(&*s.doc)
    } else {
        let placed = solidcraft_doc::joints::solve(&s.doc, &st).world;
        let mut doc = (*s.doc).clone();
        for o in &mut doc.occurrences {
            match moves.get(&o.id) {
                Some(_) if placed.contains_key(&o.id) => held.push(o.name.clone()),
                Some(t) => o.transform = *t,
                None => {}
            }
        }
        std::borrow::Cow::Owned(doc)
    };
    let mut pkg = solidcraft_io::urdf(&doc, &doc.painted(&st), &stem, &options)?;
    for name in held {
        pkg.warnings.push(format!("`{name}` was moved but the move is not captured: exported where its joints place it"));
    }
    let dir = file.parent().unwrap_or(std::path::Path::new(""));
    let mut targets: Vec<(String, &[u8])> = pkg.meshes.iter().map(|(n, b)| (dir.join(n).to_string_lossy().to_string(), b.as_slice())).collect();
    targets.push((path.to_string(), pkg.xml.as_bytes()));
    for (t, _) in &targets {
        solidcraft_io::vfs::check(t).map_err(|e| EngineError::Other(format!("{t}: {e}")))?;
    }
    let mut written: Vec<String> = Vec::new();
    for (t, bytes) in &targets {
        if let Err(e) = solidcraft_io::vfs::write(t, bytes) {
            let before = if written.is_empty() { "none".to_string() } else { written.join(", ") };
            return Err(EngineError::Other(format!("{t}: {e} (this file may be incomplete; written before it: {before})")));
        }
        written.push(t.clone());
    }
    Ok(
        json!({"path": path, "format": "Urdf", "files": written, "links": pkg.model.links.len(), "joints": pkg.model.joints.len(), "warnings": pkg.warnings}),
    )
}

fn save_mesh(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "file.save_mesh";
    let path = path_arg(p, cmd)?;
    let format = match str_(p, "format").map(str::to_ascii_lowercase).as_deref() {
        None => match std::path::Path::new(path).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).as_deref() {
            Some("3mf") => Format::ThreeMf,
            _ => Format::StlBinary,
        },
        Some("stl") if bool_(p, "ascii").unwrap_or(false) => Format::StlAscii,
        Some("stl") => Format::StlBinary,
        Some("3mf") => Format::ThreeMf,
        Some("3mf-tagged") => Format::ThreeMfTagged,
        Some(other) => return Err(bad(cmd, format!("unknown mesh format `{other}` (stl, 3mf, 3mf-tagged)"))),
    };
    // `slicer` sends the saved file to a program, through the same pipeline as `file.send_to_slicer`
    // (a slicer that needs starting is waited for, `single_instance` applies). It is kept for scripts
    // and may be deprecated later in favour of `file.send_to_slicer` or a Send to App workflow; that is
    // a possibility, not a plan.
    let quality = mesh_quality(p, cmd)?;
    let parts = mesh_parts(s, p, cmd, format)?;
    // Read before `slicer`, so a wrong value is refused there too; a file sent to a slicer is laid
    // on its print faces whatever this says.
    let lay = lay_on_print_faces(p, cmd, format)?;
    if let Some(spec) = str_(p, "slicer") {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (spec, parts);
            return Err(bad(cmd, "a slicer can't be started from the browser"));
        }
        #[cfg(not(target_arch = "wasm32"))]
        return send::send(
            s,
            send::Send {
                cmd,
                kind: crate::slicers::TargetKind::Slicer,
                spec,
                bodies: string_list(cmd, p, "bodies")?,
                parts,
                format,
                quality,
                dest: send::Dest::Path(path),
                single_instance: bool_(p, "single_instance").unwrap_or(true),
                wait: send::Wait::Block,
            },
        );
    }
    let bodies = string_list(cmd, p, "bodies")?;
    if lay {
        return export_laid_to(s, path, format, &bodies, &parts, quality);
    }
    export_to(s, cmd, path, format, &bodies, &parts, quality)
}

/// The browser build can't start a program.
fn starts_programs(_: &Session) -> std::result::Result<(), String> {
    if cfg!(target_arch = "wasm32") { Err("a slicer can't be started from the browser".into()) } else { Ok(()) }
}

/// Save the bodies to this user's slicer folder and start the slicer with the file. The file
/// name stays the same for the same design and bodies, so sending again replaces it.
fn send_to_slicer(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "file.send_to_slicer";
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, p);
        Err(bad(cmd, "a slicer can't be started from the browser"))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let format = match str_(p, "format").map(str::to_ascii_lowercase).as_deref() {
            None | Some("3mf") => Format::ThreeMf,
            Some("stl") => Format::StlBinary,
            Some("3mf-tagged") => Format::ThreeMfTagged,
            Some(other) => return Err(bad(cmd, format!("unknown mesh format `{other}` (stl, 3mf, 3mf-tagged)"))),
        };
        let spec = str_(p, "slicer").ok_or_else(|| bad(cmd, "give `slicer` (see file.slicers)"))?;
        let wait = send::Wait::parse(str_(p, "wait")).ok_or_else(|| bad(cmd, "`wait` is block, return or no"))?;
        send::send(
            s,
            send::Send {
                cmd,
                kind: crate::slicers::TargetKind::Slicer,
                spec,
                bodies: string_list(cmd, p, "bodies")?,
                parts: Vec::new(),
                format,
                quality: Default::default(),
                dest: send::Dest::Folder,
                single_instance: bool_(p, "single_instance").unwrap_or(true),
                wait,
            },
        )
    }
}

fn slicers(_: &mut Session, _: &Value) -> Result<Value> {
    let slicers = crate::slicers::detect()
        .iter()
        .map(|d| SlicerInfo {
            id: d.id.clone(),
            name: d.name.clone(),
            program: d.command.first().cloned(),
            icon: d.icon.as_ref().map(|i| i.path().to_string_lossy().into_owned()),
        })
        .collect();
    serde_json::to_value(SlicersResult { slicers, param_warnings: Vec::new() }).map_err(|e| bad("file.slicers", e.to_string()))
}

fn recovery_dir(p: &Value, cmd: &str) -> Result<std::path::PathBuf> {
    let dir = match str_(p, "dir").filter(|d| !d.trim().is_empty() && d.len() < 4096) {
        Some(d) => d.into(),
        None => crate::recovery::default_dir().ok_or_else(|| bad(cmd, "no recovery folder on this system (give `dir`)"))?,
    };
    // An agent's commands reach the recovery folder only when it is one of their folders.
    solidcraft_io::vfs::check(&dir.to_string_lossy()).map_err(|e| EngineError::Other(e.to_string()))?;
    Ok(dir)
}

fn recovery_list(_s: &mut Session, p: &Value) -> Result<Value> {
    let dir = recovery_dir(p, "doc.recovery_list")?;
    let designs = crate::recovery::orphans(&dir).into_iter().map(RecoveredDesign::from).collect();
    serde_json::to_value(RecoveryListResult { dir, designs, param_warnings: Vec::new() })
        .map_err(|e| EngineError::Other(format!("doc.recovery_list: {e}")))
}

fn recover(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "doc.recover";
    let dir = recovery_dir(p, cmd)?;
    let id = str_(p, "id").ok_or_else(|| bad(cmd, "`id` is required (see doc.recovery_list)"))?;
    let (mut doc, entry) = crate::recovery::load(&dir, id)?;
    if let Some(stem) = entry.path.as_deref().and_then(|path| default_name_for(&doc.name, path)) {
        doc.name = stem;
    }
    s.replace_design(Session::opened_at_validating(doc, None, entry.path.as_deref(), s.validation()));
    s.mark_unsaved();
    if bool_(p, "discard").unwrap_or(false) {
        crate::recovery::remove(&dir, id)?;
    }
    let errors = s.model.results.iter().filter(|r| r.error.is_some()).count();
    let reply =
        RecoverResult { id: id.to_string(), path: entry.path, name: entry.name, features: s.doc.features.len(), errors, param_warnings: Vec::new() };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(format!("{cmd}: {e}")))
}

fn recovery_discard(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "doc.recovery_discard";
    let dir = recovery_dir(p, cmd)?;
    let ids: Vec<String> = if bool_(p, "all").unwrap_or(false) {
        crate::recovery::orphans(&dir).into_iter().map(|e| e.id).collect()
    } else {
        vec![str_(p, "id").ok_or_else(|| bad(cmd, "`id` or `all` is required"))?.to_string()]
    };
    for id in &ids {
        crate::recovery::remove(&dir, id)?;
    }
    serde_json::to_value(RecoveryDiscardResult { discarded: ids, param_warnings: Vec::new() }).map_err(|e| EngineError::Other(format!("{cmd}: {e}")))
}

#[cfg(test)]
#[path = "urdf_tests.rs"]
mod urdf_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// #9449: a 3MF export carries a picture of the part as its package thumbnail.
    #[test]
    fn a_3mf_export_carries_the_design_thumbnail() {
        crate::capture::install_test_renderer();
        let mut s = Session::default();
        // Headless, as from the CLI: nothing has drawn the bodies, so their display meshes are cold.
        s.execute("solid.box", &json!({"length": 10, "width": 20, "height": 30})).unwrap();
        let dir = std::env::temp_dir();
        let has_thumbnail = |s: &mut Session, name: &str| {
            let path = dir.join(format!("solidcraft-3mf-thumbnail-{}-{name}", std::process::id()));
            s.execute("file.export", &json!({"path": path})).unwrap();
            let bytes = std::fs::read(&path).unwrap();
            std::fs::remove_file(&path).unwrap();
            bytes.windows(b"Metadata/thumbnail.png".len()).any(|entry| entry == b"Metadata/thumbnail.png")
        };
        assert!(has_thumbnail(&mut s, "part.3mf"));
        assert!(!has_thumbnail(&mut s, "part.stl"), "STL has no place for a picture");
    }

    /// While the profiler records, a STEP import's reply carries its own profile: the import's
    /// stages and the kernel's calls, as diagnostics.profile reports them.
    #[test]
    fn a_step_import_reports_its_profile_while_the_profiler_records() {
        let _serial = crate::schema::diagnostics::PROFILE_TESTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = std::env::temp_dir().join(format!("solidcraft-import-profile-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("block.step").to_string_lossy().to_string();
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 20, "width": 10, "height": 5})).unwrap();
        s.execute("file.export", &json!({"path": path})).unwrap();
        // The first read of the file (a second one finds it in the import cache).
        solidcraft_profile::set_enabled(true);
        let out = Session::default().execute("file.insert_step", &json!({"path": path})).unwrap();
        solidcraft_profile::set_enabled(false);
        let plain = Session::default().execute("file.insert_step", &json!({"path": path})).unwrap();
        assert!(plain.get("profile").is_none(), "nothing recorded, nothing reported: {plain}");
        let p = &out["profile"];
        let rows = p["rows"].as_array().unwrap();
        let has = |cat: &str| rows.iter().any(|r| r["category"] == cat);
        assert!(has("import") && has("solidcore"), "{p}");
        assert!(p["by_category"].as_array().is_some_and(|c| !c.is_empty()) && p["wall_ms"].as_f64().is_some_and(|w| w > 0.0), "{p}");
    }

    /// Renaming the open design's file updates its path and, when the name came from the file,
    /// the design name the tab shows (#11375).
    #[test]
    fn follow_file_rename_updates_path_and_borrowed_design_name() {
        let mut s = Session::default();
        s.doc_mut().name = "Plate".into();
        s.path = Some("opfs://Plate.solidcraft".into());
        s.user_named = false;
        s.follow_file_rename("opfs://Plate.solidcraft", "opfs://Bracket.solidcraft");
        assert_eq!(s.path.as_deref(), Some("opfs://Bracket.solidcraft"));
        assert_eq!(s.doc.name, "Bracket");

        let mut kept = Session::default();
        kept.doc_mut().name = "My part".into();
        kept.path = Some("opfs://Plate.solidcraft".into());
        kept.user_named = true;
        kept.follow_file_rename("opfs://Plate.solidcraft", "opfs://Bracket.solidcraft");
        assert_eq!(kept.path.as_deref(), Some("opfs://Bracket.solidcraft"));
        assert_eq!(kept.doc.name, "My part");

        let mut other = Session::default();
        other.doc_mut().name = "Other".into();
        other.path = Some("opfs://Other.solidcraft".into());
        other.follow_file_rename("opfs://Plate.solidcraft", "opfs://Bracket.solidcraft");
        assert_eq!(other.path.as_deref(), Some("opfs://Other.solidcraft"));
        assert_eq!(other.doc.name, "Other");
    }

    /// #8139: with `objects: "component"` a 3MF has one object for each placement of a component
    /// with two or more bodies, its bodies the object's parts; the design's own bodies stay objects
    /// of their own. The default (and `objects: "body"`) is one object per body, as before. Read
    /// back, an object with parts is one mesh with all its parts' triangles.
    #[test]
    fn a_components_bodies_export_to_3mf_as_one_object_with_parts() {
        let dir = std::env::temp_dir().join(format!("solidcraft-3mf-parts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = |n: &str| dir.join(n).to_string_lossy().to_string();
        let read = |p: &str| -> Vec<(String, usize)> {
            let (objects, warnings) = solidcraft_io::read_3mf(&std::fs::read(p).unwrap()).unwrap();
            assert!(warnings.is_empty(), "{warnings:?}");
            objects.iter().map(|o| (o.name.clone(), o.triangles.len())).collect()
        };
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "corner": [-30, 0, 0], "body_name": "Loose"})).unwrap();
        s.execute("component.create", &json!({"name": "Bracket"})).unwrap();
        s.execute("solid.box", &json!({"length": 20, "width": 20, "height": 10, "body_name": "Base"})).unwrap();
        s.execute("solid.cylinder", &json!({"radius": 5, "height": 10, "base": [10, 10, 10], "operation": "new", "body_name": "Boss"})).unwrap();
        s.execute("component.activate", &json!({"component": "root"})).unwrap();

        s.execute("file.export", &json!({"path": path("bodies.3mf")})).unwrap();
        let bodies = read(&path("bodies.3mf"));
        assert_eq!(bodies.len(), 3, "one object per body: {bodies:?}");
        s.execute("file.export", &json!({"path": path("body.3mf"), "objects": "body"})).unwrap();
        assert_eq!(read(&path("body.3mf")), bodies);
        let loose = bodies.iter().find(|(n, _)| n == "Loose").map(|(_, t)| *t).unwrap_or(0);
        let bracket: usize = bodies.iter().filter(|(n, _)| n != "Loose").map(|(_, t)| t).sum();

        s.execute("file.export", &json!({"path": path("parts.3mf"), "objects": "component"})).unwrap();
        let parts = read(&path("parts.3mf"));
        assert_eq!(parts, [("Loose".to_string(), loose), ("Bracket:1".to_string(), bracket)], "{bodies:?}");
        s.execute("file.save_mesh", &json!({"path": path("saved.3mf"), "objects": "component"})).unwrap();
        assert_eq!(read(&path("saved.3mf")), parts);
        s.execute("file.export", &json!({"path": path("tagged.3mf"), "format": "3mf-tagged", "objects": "component"})).unwrap();
        assert_eq!(read(&path("tagged.3mf")), parts);

        // Placed again, each placement is an object of its own.
        s.execute("occurrence.copy", &json!({"component": "Bracket", "translate": [0, 40, 0]})).unwrap();
        s.execute("file.export", &json!({"path": path("two.3mf"), "objects": "component"})).unwrap();
        let two = read(&path("two.3mf"));
        assert_eq!(two, [("Loose".to_string(), loose), ("Bracket:1".to_string(), bracket), ("Bracket:2".to_string(), bracket)]);

        // Other formats have no parts, and `objects` is body or component.
        let e = s.execute("file.export", &json!({"path": path("parts.stl"), "objects": "component"})).unwrap_err().to_string();
        assert!(e.contains("`objects: component` is for 3MF"), "{e}");
        let e = s.execute("file.save_mesh", &json!({"path": path("parts.stl"), "objects": "component"})).unwrap_err().to_string();
        assert!(e.contains("`objects: component` is for 3MF"), "{e}");
        for bad in [json!("parts"), json!(true), json!(2)] {
            let e = s.execute("file.export", &json!({"path": path("bad.3mf"), "objects": bad})).unwrap_err().to_string();
            assert!(e.contains("`objects` must be body or component"), "{bad}: {e}");
        }
        assert!(!std::path::Path::new(&path("parts.stl")).exists() && !std::path::Path::new(&path("bad.3mf")).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The JSON chunk of a GLB file, after checking its header and chunk lengths.
    fn glb_json(bytes: &[u8]) -> Value {
        let u32_at = |i: usize| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
        assert_eq!(&bytes[..4], b"glTF");
        assert_eq!((u32_at(4), u32_at(8)), (2, bytes.len()));
        let json_len = u32_at(12);
        assert_eq!(&bytes[16..20], b"JSON");
        assert_eq!(json_len % 4, 0);
        let bin = 20 + json_len;
        assert_eq!((&bytes[bin + 4..bin + 8], u32_at(bin) + bin + 8), (&b"BIN\0"[..], bytes.len()));
        serde_json::from_slice(&bytes[20..bin]).unwrap()
    }

    /// Each body node of a GLB with its mesh's bounds in millimetres (the POSITION accessor's),
    /// after checking the root node turns millimetres and Z up into metres and Y up.
    /// Each mesh node's name and the bounds of its mesh where the node tree below the root puts
    /// it (millimetres, Z up): occurrence nodes place their bodies by translation and rotation.
    fn glb_bodies(g: &Value) -> Vec<(String, [f64; 3], [f64; 3])> {
        let root = &g["nodes"][0];
        assert_eq!(g["scenes"][0]["nodes"], json!([0]));
        assert_eq!(root["scale"], json!([0.001, 0.001, 0.001]));
        let half = std::f64::consts::FRAC_1_SQRT_2;
        assert_eq!(root["rotation"], json!([-half, 0.0, 0.0, half]));
        let v3 = |v: &Value| -> [f64; 3] { [0, 1, 2].map(|i| v[i].as_f64().unwrap()) };
        // A node's placement as a matrix (`m[col][row]`) from its translation and rotation.
        let local = |n: &Value| -> solidcraft_doc::Mat {
            let t = n.get("translation").map_or([0.0; 3], v3);
            let [x, y, z, w] = n.get("rotation").map_or([0.0, 0.0, 0.0, 1.0], |q| [0, 1, 2, 3].map(|i| q[i].as_f64().unwrap()));
            [
                [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y + z * w), 2.0 * (x * z - y * w), 0.0],
                [2.0 * (x * y - z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + x * w), 0.0],
                [2.0 * (x * z + y * w), 2.0 * (y * z - x * w), 1.0 - 2.0 * (x * x + y * y), 0.0],
                [t[0], t[1], t[2], 1.0],
            ]
        };
        let kids = |n: &Value| -> Vec<usize> {
            n["children"].as_array().map(|c| c.iter().map(|i| i.as_u64().unwrap() as usize).collect()).unwrap_or_default()
        };
        let mut stack: Vec<(usize, solidcraft_doc::Mat)> = kids(root).into_iter().map(|c| (c, solidcraft_doc::IDENTITY)).collect();
        let mut out = Vec::new();
        while let Some((i, above)) = stack.pop() {
            let node = &g["nodes"][i];
            let m = solidcraft_doc::mat_mul(&above, &local(node));
            stack.extend(kids(node).into_iter().map(|c| (c, m)));
            let Some(mesh) = node.get("mesh") else { continue };
            let mesh = &g["meshes"][mesh.as_u64().unwrap() as usize];
            let accessor = &g["accessors"][mesh["primitives"][0]["attributes"]["POSITION"].as_u64().unwrap() as usize];
            let (lo, hi) = (v3(&accessor["min"]), v3(&accessor["max"]));
            let (mut min, mut max) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
            for k in 0..8 {
                let p = [0, 1, 2].map(|a| if k & (1 << a) == 0 { lo[a] } else { hi[a] });
                let q = solidcraft_doc::apply_point(&m, solidcore_record::Vec3::new(p[0], p[1], p[2]));
                for (a, v) in [q.x, q.y, q.z].into_iter().enumerate() {
                    min[a] = min[a].min(v);
                    max[a] = max[a].max(v);
                }
            }
            out.push((node["name"].as_str().unwrap().to_string(), min, max));
        }
        out
    }

    /// #1553: a GLB export holds every body where the assembly places it, each a node named after
    /// the body, under one root that makes millimetres metres and Z up Y up. A component placed
    /// twice is written twice, each copy where it stands (through its occurrence's node, #5296),
    /// and `bodies` exports only those named.
    #[test]
    fn a_glb_export_holds_each_body_where_the_assembly_places_it() {
        let dir = std::env::temp_dir().join(format!("solidcraft-glb-placed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = |n: &str| dir.join(n).to_string_lossy().to_string();
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "corner": [-30, 0, 0], "body_name": "Loose"})).unwrap();
        s.execute("component.create", &json!({"name": "Bracket"})).unwrap();
        s.execute("solid.box", &json!({"length": 20, "width": 20, "height": 10, "body_name": "Base"})).unwrap();
        s.execute("solid.cylinder", &json!({"radius": 5, "height": 10, "base": [10, 10, 10], "operation": "new", "body_name": "Boss"})).unwrap();
        s.execute("component.activate", &json!({"component": "root"})).unwrap();
        s.execute("occurrence.copy", &json!({"component": "Bracket", "translate": [0, 40, 0]})).unwrap();

        let out = s.execute("file.export", &json!({"path": path("assembly.glb")})).unwrap();
        assert_eq!(out["format"], "Glb");
        let bodies = glb_bodies(&glb_json(&std::fs::read(path("assembly.glb")).unwrap()));
        let near = |a: [f64; 3], b: [f64; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3);
        let mut found: Vec<([f64; 3], [f64; 3])> = bodies.iter().map(|(_, lo, hi)| (*lo, *hi)).collect();
        found.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut want = vec![
            ([-30.0, 0.0, 0.0], [-20.0, 10.0, 10.0]),
            ([0.0, 0.0, 0.0], [20.0, 20.0, 10.0]),
            ([0.0, 40.0, 0.0], [20.0, 60.0, 10.0]),
            ([5.0, 5.0, 10.0], [15.0, 15.0, 20.0]),
            ([5.0, 45.0, 10.0], [15.0, 55.0, 20.0]),
        ];
        want.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(found.len(), want.len(), "{bodies:?}");
        for (f, w) in found.iter().zip(&want) {
            assert!(near(f.0, w.0) && near(f.1, w.1), "{f:?} is not {w:?}: {bodies:?}");
        }
        let loose = bodies.iter().find(|(n, _, _)| n == "Loose").unwrap();
        assert!(near(loose.1, [-30.0, 0.0, 0.0]), "{loose:?}");

        s.execute("file.export", &json!({"path": path("loose.glb"), "bodies": ["Loose"]})).unwrap();
        let one = glb_bodies(&glb_json(&std::fs::read(path("loose.glb")).unwrap()));
        assert_eq!(one.iter().map(|(n, _, _)| n.as_str()).collect::<Vec<_>>(), ["Loose"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #1553: a GLB material has the look's colour (linear, as glTF's factors are), metallic and
    /// roughness, and a face's own look wins over its body's. A transparent body, or a body a
    /// render setup gives optics, is refused, and the file already at the path stays as it was.
    #[test]
    fn a_glb_export_writes_opaque_looks_and_refuses_what_it_cannot_hold() {
        let dir = std::env::temp_dir().join(format!("solidcraft-glb-looks-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("block.glb").to_string_lossy().to_string();
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 10, "width": 20, "height": 30, "body_name": "Block"})).unwrap();
        let pbr = json!({"metallic": 1.0, "roughness": 0.25, "dielectric_ior": 1.5});
        s.execute("appearance.assign", &json!({"bodies": ["Block"], "name": "Red metal", "color": [255, 0, 0], "opacity": 1.0, "pbr": pbr})).unwrap();
        s.execute("appearance.assign", &json!({"body": "Block", "faces": [[5, 10, 30]], "color": "#00ff00"})).unwrap();
        s.execute("file.export", &json!({"path": path})).unwrap();
        let written = std::fs::read(&path).unwrap();
        let g = glb_json(&written);
        let materials = g["materials"].as_array().unwrap();
        let factor = |name: &str| materials.iter().find(|m| m["name"] == name).map(|m| m["pbrMetallicRoughness"].clone());
        let red = factor("Red metal").unwrap_or_else(|| panic!("{materials:?}"));
        assert_eq!(red, json!({"baseColorFactor": [1.0, 0.0, 0.0, 1.0], "metallicFactor": 1.0, "roughnessFactor": 0.25}));
        let green = materials.iter().find(|m| m["pbrMetallicRoughness"]["baseColorFactor"] == json!([0.0, 1.0, 0.0, 1.0]));
        assert!(green.is_some_and(|m| m["pbrMetallicRoughness"]["metallicFactor"] == 0.0), "{materials:?}");
        assert_eq!(g["meshes"][0]["primitives"].as_array().map(Vec::len), Some(2), "the top face is a primitive of its own");
        assert!(g.get("extensionsUsed").is_none(), "IOR 1.5 is glTF's own: {g}");

        // Refused: the file written above stays as it was.
        s.execute("appearance.assign", &json!({"bodies": ["Block"], "color": "#2040c0", "opacity": 0.5})).unwrap();
        let e = s.execute("file.export", &json!({"path": path})).unwrap_err().to_string();
        assert!(e.contains("Block") && e.contains("transparent"), "{e}");
        assert_eq!(std::fs::read(&path).unwrap(), written);
        s.execute("appearance.assign", &json!({"bodies": ["Block"], "color": "#2040c0", "opacity": 1.0})).unwrap();

        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 4, "corner": [40, 0, 0], "body_name": "Diffuser"})).unwrap();
        s.execute("render.settings.set", &json!({"name": "Studio", "set": {"job": {"method": "gpu_path"}}})).unwrap();
        let index = crate::product_scene::extract(&s).unwrap().meshes.iter().position(|m| m.name == "Diffuser").unwrap();
        s.execute(
            "render.optics.bind",
            &json!({"name": "Studio", "body_index": index, "body_name": "Diffuser", "expected_revision": s.revision, "optics": {"ior": 1.4}}),
        )
        .unwrap();
        let e = s.execute("file.export", &json!({"path": path})).unwrap_err().to_string();
        assert!(e.contains("Diffuser") && e.contains("render setup `Studio`") && e.contains("optics"), "{e}");
        assert_eq!(std::fs::read(&path).unwrap(), written);
        s.execute("file.export", &json!({"path": path, "bodies": ["Block"]})).unwrap();
        assert_ne!(std::fs::read(&path).unwrap(), written, "the body without optics exports");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Each format's own export command (one key each in Keyboard Shortcuts) writes that format
    /// whatever the path's extension, as `file.export` does with `format`, and says so.
    #[test]
    fn each_format_has_an_export_command_of_its_own() {
        let dir = std::env::temp_dir().join(format!("solidcraft-export-per-format-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let at = |n: &str| dir.join(n).to_string_lossy().to_string();
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 20, "width": 10, "height": 5, "body_name": "Block"})).unwrap();
        for (cmd, format, signature) in [
            ("file.export_step", "Step", &b"ISO-10303-21;"[..]),
            ("file.export_3mf", "ThreeMf", &b"PK"[..]),
            ("file.export_obj", "Obj", &b""[..]),
            ("file.export_stl", "StlBinary", &b""[..]),
        ] {
            // A name with another format's extension: the command, not the name, picks the format.
            let path = at(&format!("{format}.txt"));
            let r = s.execute(cmd, &json!({"path": path})).unwrap();
            assert_eq!((r["path"].as_str(), r["format"].as_str()), (Some(path.as_str()), Some(format)), "{cmd}: {r}");
            let bytes = std::fs::read(&path).unwrap();
            assert_eq!(r["bytes"].as_u64(), Some(bytes.len() as u64), "{cmd}");
            assert!(bytes.starts_with(signature), "{cmd}");
        }
        // The triangles `file.export` writes (the 80-byte header holds the file's name), 84 + 50
        // bytes each. A planar face is exact for any triangulation of its outline, so the mesher
        // gives each of the box's six faces two triangles (kernel test `a_box_is_two_triangles_a_face`).
        s.execute("file.export", &json!({"path": at("box.stl")})).unwrap();
        assert_eq!(std::fs::read(at("StlBinary.txt")).unwrap()[80..], std::fs::read(at("box.stl")).unwrap()[80..]);
        let triangles = 6 * 2;
        let stl = std::fs::read(at("box.stl")).unwrap();
        assert_eq!(u32::from_le_bytes([stl[80], stl[81], stl[82], stl[83]]), u32::try_from(triangles).unwrap());
        assert_eq!(stl.len(), 84 + 50 * triangles);
        let obj = std::fs::read_to_string(at("Obj.txt")).unwrap();
        assert_eq!(obj.lines().filter(|l| l.starts_with("f ")).count(), triangles, "{obj}");
        let r = s.execute("file.export_urdf", &json!({"path": at("robot.urdf")})).unwrap();
        assert_eq!(r["format"], "Urdf");
        assert!(std::fs::read_to_string(at("robot.urdf")).unwrap().contains("<robot"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reopening_import_reuses_only_unchanged_untouched_results() {
        let dir = std::env::temp_dir().join(format!("solidcraft-reuse-import-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cache_dir = dir.join("imports");
        let cache = solidcraft_io::ImportCache::new(cache_dir.clone());
        let mut source = Session::default();
        source.execute("solid.box", &json!({"length": 10, "width": 40, "height": 5})).unwrap();
        let body = &source.model.state().bodies[0].body;
        let iges = solidcraft_kernel::iges_export_bodies(
            &[solidcraft_kernel::ExportBody { body, name: "Plate".into(), color: None }],
            &solidcraft_kernel::StepHeader::default(),
        )
        .unwrap();
        for extension in ["step", "igs"] {
            let path = dir.join(format!("plate.{extension}")).to_string_lossy().to_string();
            if extension == "step" {
                source.execute("file.export", &json!({"path": path})).unwrap();
            } else {
                std::fs::write(&path, &iges).unwrap();
            }
            let (mut first, cold) = open_step(&path, &json!({}), Some(&cache), solidcraft_kernel::Validation::default()).unwrap();
            assert_eq!(cold["import_reused"], false);
            let before = first.doc.to_json();
            let bounds = crate::view::bounds(&first).unwrap();
            assert!(first.is_dirty());
            assert!((solidcraft_kernel::measure_mass(&first.model.state().bodies[0].body).unwrap().volume - 2000.0).abs() < 1e-6);
            first.execute("solid.box", &json!({"length": 1, "width": 2, "height": 3})).unwrap();
            let (mut reopened, hit) = open_step(&path, &json!({}), Some(&cache), solidcraft_kernel::Validation::default()).unwrap();
            assert_eq!(hit["import_reused"], true);
            assert_eq!(hit["content_sha256"], cold["content_sha256"]);
            assert_eq!(hit["warnings"], cold["warnings"]);
            assert_eq!(reopened.doc.to_json(), before, "subsequent edits never enter the cache");
            assert_eq!(reopened.model.last_recomputed, 0, "cache hit adopts the model without importing");
            assert!(reopened.is_dirty());
            assert_eq!(crate::view::bounds(&reopened).unwrap(), bounds);
            assert!((solidcraft_kernel::measure_mass(&reopened.model.state().bodies[0].body).unwrap().volume - 2000.0).abs() < 1e-6);
            reopened.execute("solid.box", &json!({"length": 1, "width": 2, "height": 3})).unwrap();
            assert!(reopened.model.results.iter().all(|r| r.error.is_none()), "cached import remains editable");
            assert_eq!(
                open_step(&path, &json!({"reuse_import": false}), Some(&cache), solidcraft_kernel::Validation::default()).unwrap().1["import_reused"],
                false
            );
            assert_eq!(open_step(&path, &json!({}), Some(&cache), solidcraft_kernel::Validation::default()).unwrap().1["import_reused"], true);
            assert_eq!(
                open_step(&path, &json!({"up": "y", "names": "solid"}), Some(&cache), solidcraft_kernel::Validation::default()).unwrap().1["import_reused"],
                false
            );
            assert_eq!(
                open_step(&path, &json!({"up": "y", "names": "solid"}), Some(&cache), solidcraft_kernel::Validation::default()).unwrap().1["import_reused"],
                true
            );
            // Changed bytes, even unchanged geometry, must miss. IGES has fixed-width records,
            // so exercise this with a legal STEP comment.
            if extension == "step" {
                let mut bytes = std::fs::read(&path).unwrap();
                bytes.extend_from_slice(b"\n/* changed */\n");
                std::fs::write(&path, bytes).unwrap();
                let changed = open_step(&path, &json!({}), Some(&cache), solidcraft_kernel::Validation::default()).unwrap().1;
                assert_eq!(changed["import_reused"], false);
                assert_ne!(changed["content_sha256"], cold["content_sha256"]);
            }
        }
        // Corruption is disposable; every open can recompute without losing the source.
        for entry in std::fs::read_dir(&cache_dir).unwrap() {
            std::fs::write(entry.unwrap().path(), b"damaged").unwrap();
        }
        let path = dir.join("plate.step").to_string_lossy().to_string();
        assert_eq!(open_step(&path, &json!({}), Some(&cache), solidcraft_kernel::Validation::default()).unwrap().1["import_reused"], false);
        assert!(cache.clear().unwrap() > 0);
        assert_eq!(cache.clear().unwrap(), 0);
        assert_eq!(
            open_step(&path, &json!({"reuse_import": false}), Some(&cache), solidcraft_kernel::Validation::default()).unwrap().1["import_reused"],
            false
        );
        assert_eq!(std::fs::read_dir(&cache_dir).unwrap().count(), 0, "disabled reuse never stores an import");
        assert_eq!(open_step(&path, &json!({}), Some(&cache), solidcraft_kernel::Validation::default()).unwrap().1["import_reused"], false);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn import_digest_is_of_original_bytes_for_open_and_insert() {
        use sha2::{Digest, Sha256};
        let dir = std::env::temp_dir().join(format!("solidcraft-import-digest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("part.step").to_string_lossy().to_string();
        let mut source = Session::default();
        source.execute("solid.box", &json!({"length": 10, "width": 40, "height": 5})).unwrap();
        source.execute("file.export", &json!({"path": path})).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        for legacy in [false, true] {
            if legacy {
                bytes.extend_from_slice(b"\n/* legacy name: \xe9 */\n");
                std::fs::write(&path, &bytes).unwrap();
            }
            let expected = format!("{:x}", Sha256::digest(&bytes));
            for command in ["doc.open", "file.insert_step"] {
                let mut session = Session::default();
                let result = session.execute(command, &json!({"path": path})).unwrap();
                assert_eq!(result["content_sha256"], expected, "{command}: legacy={legacy}");
            }
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// #4747: a mesh export's `quality` follows curved faces more closely. A 3 mm hole in a
    /// 200 mm plate gets more triangles at fine and more again at finest, in STL and 3MF alike
    /// (Save As Mesh too); standard is what the export always gave; anything else is refused.
    #[test]
    fn a_mesh_export_follows_round_faces_more_closely_at_a_finer_quality() {
        let dir = std::env::temp_dir().join(format!("solidcraft-mesh-quality-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 200, "width": 100, "height": 5})).unwrap();
        s.execute("solid.cylinder", &json!({"base": [100, 50, -1], "radius": 1.5, "height": 7, "operation": "cut"})).unwrap();
        let stl_triangles = |b: &[u8]| u32::from_le_bytes([b[80], b[81], b[82], b[83]]);
        let mut counts = Vec::new();
        for quality in [None, Some("standard"), Some("fine"), Some("finest")] {
            let path = dir.join(format!("plate-{}.stl", quality.unwrap_or("default"))).to_string_lossy().to_string();
            let mut p = json!({"path": path});
            if let Some(q) = quality {
                p["quality"] = json!(q);
            }
            s.execute("file.export", &p).unwrap();
            counts.push(stl_triangles(&std::fs::read(&path).unwrap()));
        }
        assert_eq!(counts[0], counts[1], "standard is the default: {counts:?}");
        assert!(counts[2] > counts[1] && counts[3] > counts[2], "finer, more triangles: {counts:?}");
        let finest_3mf = dir.join("plate.3mf").to_string_lossy().to_string();
        let standard_3mf = dir.join("plate-standard.3mf").to_string_lossy().to_string();
        s.execute("file.save_mesh", &json!({"path": finest_3mf, "quality": "finest"})).unwrap();
        s.execute("file.save_mesh", &json!({"path": standard_3mf})).unwrap();
        assert!(std::fs::metadata(&finest_3mf).unwrap().len() > std::fs::metadata(&standard_3mf).unwrap().len());
        for bad in [json!("ultra"), json!(3), json!(true)] {
            let e = s.execute("file.export", &json!({"path": dir.join("bad.stl").to_string_lossy(), "quality": bad})).unwrap_err().to_string();
            assert!(e.contains("`quality` must be standard, fine or finest"), "{bad}: {e}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn opening_drawing_files_creates_an_unsaved_xy_sketch() {
        let dir = name_dir("drawing-open");
        let dxf = "0\nSECTION\n2\nENTITIES\n0\nLINE\n10\n0\n20\n0\n11\n10\n21\n0\n0\nLINE\n10\n10\n20\n0\n11\n10\n21\n10\n0\nARC\n10\n0\n20\n0\n40\n5\n50\n0\n51\n90\n0\nENDSEC\n0\nEOF\n";
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0 L10 0 L10 10"/><circle cx="5" cy="5" r="2"/></svg>"#;
        for (file, text) in [("outline.DXF", dxf), ("outline.SVG", svg)] {
            let path = dir.join(file);
            std::fs::write(&path, text).unwrap();
            let mut s = Session::default();
            s.execute("sketch.create", &json!({"plane":"YZ"})).unwrap();
            let result = s.execute("doc.open", &json!({"path":path})).unwrap();
            assert_eq!(result["errors"], 0);
            assert_eq!(s.doc.features.len(), 1);
            let id = s.active_sketch.unwrap();
            let sketch = s.doc.sketch(id).unwrap();
            assert_eq!(s.model.state().sketch(id).unwrap().plane, solidcore_record::Plane::XY);
            assert_eq!(sketch.curves.len(), 3);
            assert_eq!(s.doc.name, "outline");
            assert!(s.path.is_none(), "Save must ask for a design path");
            assert!(s.is_dirty());
            assert!(s.undo.is_empty());
            let before = s.doc.clone();
            std::fs::write(&path, "invalid drawing").unwrap();
            assert!(s.execute("doc.open", &json!({"path":path})).is_err());
            assert_eq!(s.doc, before, "a failed open preserves the current design");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// #3044: a save run on a scratch copy of the session (on a worker, so the window keeps
    /// drawing) and taken up with `adopt_save` leaves the session as saving it directly does:
    /// the design named after its first file, its file, and nothing unsaved; undo keeps the name.
    #[test]
    fn a_save_run_on_a_copy_and_adopted_matches_saving_directly() {
        let dir = std::env::temp_dir().join(format!("solidcraft-adopt-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut direct = Session::default();
        direct.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        let mut adopted = Session::new((*direct.doc).clone());
        adopted.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        let a = dir.join("Bracket.solidcraft");
        let b = dir.join("Bracket copy.solidcraft");
        direct.execute("file.save_as", &json!({"path": a.to_string_lossy()})).unwrap();
        let mut copy = adopted.scratch();
        copy.path.clone_from(&adopted.path);
        copy.execute("file.save_as", &json!({"path": b.to_string_lossy()})).unwrap();
        assert!(adopted.is_dirty(), "nothing taken up yet");
        adopted.adopt_save(&copy);
        assert!(!adopted.is_dirty());
        assert_eq!(adopted.path.as_deref(), b.to_str());
        assert_eq!(adopted.doc.name, "Bracket copy");
        assert_eq!(direct.doc.name, "Bracket");
        assert_eq!(adopted.undo.iter().map(|u| u.doc.name.clone()).collect::<Vec<_>>(), vec!["Bracket copy".to_string(); adopted.undo.len()]);
        assert_eq!(solidcraft_io::read_design(&std::fs::read(&b).unwrap()).unwrap(), *adopted.doc);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #6003: a session changed while its save ran on a copy takes up the save's file and name,
    /// but only what the file holds counts as saved: the change stays unsaved, and undoing it
    /// leaves nothing unsaved again. A hidden body is a change too.
    #[test]
    fn a_change_made_while_a_copy_saves_stays_unsaved() {
        let dir = std::env::temp_dir().join(format!("solidcraft-adopt-changed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Bracket.solidcraft");
        for hide in [false, true] {
            let mut s = Session::default();
            s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "body_name": "Block"})).unwrap();
            let mut copy = s.scratch();
            copy.execute("file.save_as", &json!({"path": path.to_string_lossy()})).unwrap();
            if hide {
                s.execute("browser.visibility", &json!({"items": ["Block"], "visible": false})).unwrap();
            } else {
                s.execute("solid.sphere", &json!({"diameter": 4})).unwrap();
            }
            s.adopt_save(&copy);
            assert_eq!((s.path.as_deref(), s.doc.name.as_str()), (path.to_str(), "Bracket"));
            assert!(s.is_dirty(), "hide {hide}: the change is not in the file");
            s.execute("edit.undo", &json!({})).unwrap();
            assert!(!s.is_dirty(), "hide {hide}: back to what the file holds");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A `.solidcraft` design from an earlier release opens, takes its file's name, and `file.save`
    /// writes it back to that same file (container format), never to a new `.sc` beside it.
    #[test]
    fn saving_an_opened_legacy_design_keeps_its_name() {
        let dir = std::env::temp_dir().join(format!("solidcraft-legacy-ext-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let old = dir.join("Bracket.solidcraft");
        let mut first = Session::default();
        first.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        first.execute("file.save_as", &json!({"path": old.to_string_lossy()})).unwrap();
        let mut s = Session::default();
        s.execute("doc.open", &json!({"path": old.to_string_lossy()})).unwrap();
        assert_eq!(s.doc.name, "Bracket");
        s.execute("solid.box", &json!({"length": 5, "width": 5, "height": 5})).unwrap();
        s.execute("file.save", &json!({})).unwrap();
        assert_eq!(s.path.as_deref(), old.to_str());
        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| !n.ends_with(".bak") && !n.ends_with(".lock"))
            .collect();
        assert_eq!(names, ["Bracket.solidcraft"]);
        let bytes = std::fs::read(&old).unwrap();
        assert!(solidcraft_io::is_design_container(&bytes));
        assert_eq!(solidcraft_io::read_design(&bytes).unwrap().features.len(), 2);
        // Save As to the new extension leaves the old file as it was.
        s.execute("file.save_as", &json!({"path": dir.join("Bracket.sc").to_string_lossy()})).unwrap();
        assert!(old.is_file() && dir.join("Bracket.sc").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A design a newer SolidCraft saved opens through the command agents and the app use: the
    /// reply warns, what it can't read shows as that feature's error, work goes on around it, and
    /// saving keeps the feature and the file's number in both encodings.
    #[test]
    fn a_newer_design_opens_and_saving_keeps_what_it_cant_read() {
        let dir = std::env::temp_dir().join(format!("sc_newer_design_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 2, "width": 3, "height": 4})).unwrap();
        let mut file: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
        let newer = solidcraft_doc::format::format_string(solidcraft_doc::format::FORMAT + 1);
        file["format"] = json!(newer);
        let future = json!({"id": 900, "name": "Lattice1", "type": "gyroid_lattice", "cell": 2.5});
        file["features"].as_array_mut().unwrap().push(future.clone());
        file["future_section"] = json!([1, 2]);
        for name in ["newer.json", "newer.sc"] {
            let path = dir.join(name);
            let source = solidcraft_doc::Document::from_json(&file.to_string()).unwrap();
            let bytes = if name.ends_with(".sc") { solidcraft_io::write_design_container(&source).unwrap() } else { file.to_string().into_bytes() };
            std::fs::write(&path, bytes).unwrap();
            let path = path.to_string_lossy().to_string();
            let mut opened = Session::default();
            let reply = opened.execute("doc.open", &json!({"path": path})).unwrap();
            assert_eq!(reply["errors"], 1, "{reply}");
            assert_eq!(reply["features"], 2, "{reply}");
            assert!(reply["newer_file"]["not_understood"].to_string().contains("gyroid_lattice"), "{reply}");
            let first = reply["warnings"][0].as_str().unwrap();
            assert_eq!(
                first,
                format!(
                    "Saved by a newer SolidCraft (format {}, this version reads up to {}). If something's missing, update.",
                    solidcraft_doc::format::FORMAT + 1,
                    solidcraft_doc::format::FORMAT
                )
            );
            assert_eq!(opened.model.state().bodies.len(), 1, "the box still builds");
            let lattice = opened.doc.features.iter().position(|f| f.name == "Lattice1").unwrap();
            assert!(opened.model.results[lattice].error.as_deref().unwrap().contains("gyroid_lattice"));
            opened.execute("solid.box", &json!({"length": 1, "width": 1, "height": 1})).unwrap();
            let saved = opened.execute("file.save", &json!({})).unwrap();
            assert_eq!(saved["kept_format"], json!(newer), "{saved}");
            assert!(saved.get("not_kept").is_none(), "{saved}");
            let mut again = Session::default();
            again.execute("doc.open", &json!({"path": path})).unwrap();
            assert_eq!(again.doc.features.len(), 3);
            assert_eq!(again.model.state().bodies.len(), 2);
            let written: Value = serde_json::from_str(&again.doc.to_json()).unwrap();
            assert_eq!(written["format"], json!(newer));
            assert_eq!(written["future_section"], json!([1, 2]));
            assert!(written["features"].as_array().unwrap().contains(&future));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #5467: a script, the CLI or the control channel never gets a prompt. A Save over a file
    /// another program or computer changed since it was opened is refused with an error naming
    /// `overwrite`, which replaces it; a design opened read-only only saves to another file.
    #[test]
    fn a_save_over_a_file_changed_on_disk_or_opened_read_only_is_refused() {
        let dir = std::env::temp_dir().join(format!("solidcraft-changed-on-disk-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("Bracket.solidcraft");
        let a_str = a.to_string_lossy().to_string();
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        s.execute("file.save_as", &json!({"path": a_str})).unwrap();
        assert!(!s.disk_changed());
        s.execute("file.save", &json!({})).unwrap();
        // Another computer writes the file.
        let mut other = Session::default();
        other.execute("doc.open", &json!({"path": a_str})).unwrap();
        other.execute("solid.sphere", &json!({"diameter": 4})).unwrap();
        other.execute("file.save", &json!({})).unwrap();
        assert!(s.disk_changed());
        s.execute("solid.sphere", &json!({"diameter": 6})).unwrap();
        let e = s.execute("file.save", &json!({})).unwrap_err().to_string();
        assert!(e.contains("changed by another program or computer") && e.contains("overwrite"), "{e}");
        assert!(s.is_dirty(), "nothing was saved");
        s.execute("file.save", &json!({"overwrite": true})).unwrap();
        assert!(!s.disk_changed() && !s.is_dirty());
        s.execute("file.save", &json!({})).unwrap();

        let mut ro = Session::default();
        let r = ro.execute("doc.open", &json!({"path": a_str, "read_only": true})).unwrap();
        assert_eq!(r["read_only"], json!(true));
        let e = ro.execute("file.save", &json!({"overwrite": true})).unwrap_err().to_string();
        assert!(e.contains("read-only"), "{e}");
        let b = dir.join("Bracket copy.solidcraft");
        ro.execute("file.save_as", &json!({"path": b.to_string_lossy()})).unwrap();
        assert!(!ro.read_only);
        ro.execute("file.save", &json!({})).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #9736: a component moved and not captured yet is a change the file doesn't hold. The
    /// design reads as unsaved, and Save and Save As refuse, naming the component, until the move
    /// is captured (then the file has it) or reverted (then the design is as saved).
    #[test]
    fn a_move_not_captured_is_unsaved_and_saving_waits_for_it() {
        let dir = std::env::temp_dir().join(format!("solidcraft-pending-move-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("two.sc").to_string_lossy().to_string();
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 40, "height": 10, "corner": [0, 0, 0], "body_name": "BaseB"})).unwrap();
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 30, "corner": [15, 15, 10], "body_name": "ArmB"})).unwrap();
        s.execute("component.from_bodies", &json!({"bodies": ["BaseB", "ArmB"]})).unwrap();
        s.execute("file.save_as", &json!({"path": path})).unwrap();
        assert!(!s.is_dirty());
        let arm = s.doc.occurrences.iter().find(|o| o.name.starts_with("ArmB")).unwrap().name.clone();
        let arm_x = |s: &Session| {
            let world = s.world_state().unwrap();
            let body = world.bodies.iter().find(|b| b.name.starts_with("ArmB")).unwrap();
            solidcraft_kernel::measure(&body.body).unwrap().centroid.x
        };
        for answer in ["component.revert_position", "component.capture_position"] {
            s.execute("occurrence.move", &json!({"occurrence": arm, "translate": [7, 0, 0], "capture": false})).unwrap();
            assert!(s.is_dirty(), "{answer}: the move is a change to save");
            for (cmd, p) in [("file.save", json!({})), ("file.save_as", json!({"path": dir.join("other.sc").to_string_lossy()}))] {
                let e = s.execute(cmd, &p).unwrap_err().to_string();
                assert!(e.contains(&arm) && e.contains("not captured") && e.contains("component.capture_position"), "{cmd}: {e}");
            }
            assert!(!dir.join("other.sc").exists(), "nothing written");
            s.execute(answer, &json!({})).unwrap();
            s.execute("file.save", &json!({})).unwrap();
            assert!(!s.is_dirty(), "{answer}");
        }
        let opened = Session::new(crate::io::read_design(&std::fs::read(&path).unwrap()).unwrap());
        assert!((arm_x(&opened) - 27.0).abs() < 1e-6, "the captured move is in the file: {}", arm_x(&opened));
        // An exploded view shown places the components only while it is shown: no change to
        // save, and the design saves as it is. A move on top of it is one.
        s.execute("explode.create", &json!({"name": "Apart", "moves": [{"occurrence": arm, "translate": [0, 0, 40]}]})).unwrap();
        s.execute("file.save", &json!({})).unwrap();
        s.execute("explode.show", &json!({"view": "Apart"})).unwrap();
        assert!(!s.pending_moves.is_empty() && !s.is_dirty(), "an exploded view shown");
        s.execute("file.save", &json!({})).unwrap();
        s.execute("occurrence.move", &json!({"occurrence": arm, "translate": [5, 0, 0], "capture": false})).unwrap();
        assert!(s.is_dirty());
        assert!(s.execute("file.save", &json!({})).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore = "manual warmed engine save and provisional-open benchmark"]
    fn benchmark_warm_design_save_with_preview() {
        let target = std::env::var_os("CARGO_TARGET_DIR").map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from("target"));
        // The mesh storage fixture intentionally does not evaluate geometry. Use
        // original valid STEP boxes for the engine/display-cache measurement.
        let input = target.join("issue303-step.json");
        let doc = solidcraft_io::read_design(&std::fs::read(input).unwrap()).unwrap();
        let mut session = Session::new(doc);
        for body in &session.world_state().unwrap().bodies {
            body.mesh();
        }
        let mut samples = [Vec::new(), Vec::new()];
        for round in 0..9 {
            for index in if round % 2 == 0 { [0, 1] } else { [1, 0] } {
                let path = target.join(if index == 0 { "issue303-warm.json" } else { "issue303-warm.solidcraft" });
                let began = std::time::Instant::now();
                session.execute("file.save", &json!({"path":path,"preview":true,"cache":false})).unwrap();
                samples[index].push(began.elapsed().as_secs_f64() * 1000.0);
            }
        }
        for s in &mut samples {
            s.sort_by(f64::total_cmp);
        }
        let bytes = std::fs::read(target.join("issue303-warm.solidcraft")).unwrap();
        assert!(solidcraft_io::read_design_preview(&bytes).is_some());
        assert_eq!(solidcraft_io::read_design(&bytes).unwrap(), *session.doc);
        let mut reads = Vec::new();
        for _ in 0..9 {
            let began = std::time::Instant::now();
            assert!(solidcraft_io::read_design_preview(&bytes).is_some());
            reads.push(began.elapsed().as_secs_f64() * 1000.0);
        }
        reads.sort_by(f64::total_cmp);
        eprintln!(
            "warmed engine median atomic save ms json={:.3} container_with_preview={:.3}; preview_decode_ms={:.3}; container_bytes={}",
            samples[0][4],
            samples[1][4],
            reads[4],
            bytes.len()
        );
    }

    #[test]
    fn save_uses_only_warm_meshes_and_open_preview_does_not_replace_evaluation() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length":10,"width":20,"height":30})).unwrap();
        assert!(saved_preview(&s).is_none(), "save must not triangulate a cold body");
        let state = s.world_state().unwrap();
        for body in &state.bodies {
            body.mesh();
        }
        let save_path = std::env::temp_dir().join(format!("solidcraft-preview-save-{}.solidcraft", std::process::id()));
        s.execute("file.save", &json!({"path":save_path})).unwrap();
        let saved = std::fs::read(&save_path).unwrap();
        assert!(solidcraft_io::read_design_geometry(&saved).is_some(), "ordinary saves keep the evaluated model");
        assert!(!saved.windows(b"cache/preview.bin".len()).any(|s| s == b"cache/preview.bin"), "ordinary saves omit redundant display meshes");
        s.execute("file.save", &json!({"path":save_path,"preview":true})).unwrap();
        assert!(solidcraft_io::read_design_preview(&std::fs::read(&save_path).unwrap()).is_some());
        let _ = std::fs::remove_file(&save_path);
        let _ = std::fs::remove_file(format!("{}.bak", save_path.display()));
        let mut preview = saved_preview(&s).unwrap();
        // A checksum-valid but forged cached shape must not become editable geometry.
        for mesh in &mut preview.meshes {
            mesh.positions = mesh.positions.iter().map(|c| c * 10.0).collect();
        }
        let bytes = solidcraft_io::write_design_container_with_preview(&s.doc, solidcraft_io::DesignCompression::Fast, Some(&preview)).unwrap();
        let path = std::env::temp_dir().join(format!("solidcraft-preview-{}.solidcraft", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        let seen = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let callback_seen = seen.clone();
        let mut reopened = Session {
            open_preview: Some(Box::new(move |preview| {
                callback_seen.fetch_add(preview.meshes.len(), std::sync::atomic::Ordering::Relaxed);
            })),
            ..Session::default()
        };
        reopened.execute("doc.open", &json!({"path":path})).unwrap();
        assert_eq!(seen.load(std::sync::atomic::Ordering::Relaxed), 1);
        assert!(reopened.open_preview.is_none());
        let measured = reopened.execute("inspect.measure", &json!({})).unwrap();
        assert!((measured["bodies"][0]["volume_mm3"].as_f64().unwrap() - 6000.0).abs() < 1e-6);
        assert_eq!(*reopened.doc, *s.doc);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn preview_saves_do_not_build_cold_placements_or_persist_transient_edits() {
        let mut s = Session::default();
        let created = s.execute("component.create", &json!({"name":"PreviewPart","grounded":false})).unwrap();
        let occurrence = created["occurrence"].as_u64().unwrap();
        s.execute("solid.box", &json!({"length":2,"width":3,"height":4,"body_name":"Block","operation":"new"})).unwrap();
        for body in &s.model.state().bodies {
            body.mesh();
        }
        *s.world_cache.lock().unwrap() = None;
        assert!(saved_preview(&s).is_none());
        assert!(saved_geometry(&s, &s.doc).is_some(), "the timeline's bodies need no placement");
        assert!(s.world_cache.lock().unwrap().is_none(), "saving must not prepare cold placements");
        for body in &s.world_state().unwrap().bodies {
            body.mesh();
        }
        assert!(saved_preview(&s).is_some());
        s.execute("occurrence.move", &json!({"occurrence":occurrence,"translate":[10,0,0],"capture":false})).unwrap();
        for body in &s.world_state().unwrap().bodies {
            body.mesh();
        }
        assert!(!s.pending_moves.is_empty());
        assert!(saved_preview(&s).is_none(), "uncaptured positions are not in the saved document");
        assert!(saved_geometry(&s, &s.doc).is_none());
        s.execute("component.capture_position", &json!({})).unwrap();
        for body in &s.world_state().unwrap().bodies {
            body.mesh();
        }
        let preview = saved_preview(&s).unwrap();
        let min_x = preview.meshes.iter().flat_map(|mesh| mesh.positions.as_chunks::<3>().0.iter().map(|p| p[0])).fold(f64::INFINITY, f64::min);
        assert!((min_x - 10.0).abs() < 1e-9);
        let mut editing = s.scratch();
        editing.execute("sketch.create", &json!({"plane":"XY"})).unwrap();
        for body in &editing.world_state().unwrap().bodies {
            body.mesh();
        }
        assert!(editing.active_sketch.is_some());
        assert!(saved_preview(&editing).is_none());
        assert!(saved_geometry(&editing, &editing.doc).is_none());
    }

    #[test]
    fn save_and_save_as_keep_the_model_unless_opted_out() {
        // Thumbnails are drawn by the renderer the host installs.
        crate::capture::install_test_renderer();
        let mut session = Session::new(Document::new("Cache opt-in"));
        session.execute("solid.box", &json!({"length":10,"width":20,"height":30})).unwrap();
        let world = session.world_state().unwrap();
        for body in &world.bodies {
            body.mesh();
        }
        let path = std::env::temp_dir().join(format!("solidcraft-cache-opt-in-{}.solidcraft", std::process::id()));
        for command in ["file.save", "file.save_as"] {
            for cache in [None, Some(true), Some(false), None] {
                let mut params = json!({"path":path});
                if let Some(cache) = cache {
                    params["cache"] = json!(cache);
                }
                session.execute(command, &params).unwrap();
                let bytes = std::fs::read(&path).unwrap();
                assert_eq!(solidcraft_io::read_design_geometry(&bytes).is_some(), cache != Some(false), "{command}: {cache:?}");
                assert_eq!(solidcraft_io::read_design_thumbnail(&bytes).is_some(), cache != Some(false), "{command}: {cache:?}");
                assert!(
                    !bytes.windows(b"cache/preview.bin".len()).any(|entry| entry == b"cache/preview.bin"),
                    "display meshes require preview: true"
                );
                assert_eq!(solidcraft_io::read_design(&bytes).unwrap(), *session.doc);
            }
        }
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_file(format!("{}.bak", path.display())).unwrap();
    }

    /// The model a save keeps writes each distinct sheet-metal body once, and every state names
    /// the records of exactly the sheets it holds (found by the sheet's text, not by building its
    /// record again for every state, which took most of a large sheet-metal design's save).
    #[test]
    fn a_saved_model_writes_each_sheet_once_for_the_states_that_hold_it() {
        let mut s = Session::new(Document::new("Sheets"));
        let sheet_rule = json!({"name": "AL2", "thickness": "2 mm", "k_factor": "0.5", "bend_radius": "1 mm", "active": true});
        s.execute("sheet.manage_rules", &sheet_rule).unwrap();
        for (i, x) in [0.0, 200.0, 400.0].into_iter().enumerate() {
            s.execute("sketch.create", &json!({"plane": "XY", "name": format!("Base{i}")})).unwrap();
            s.execute("sketch.rectangle.two_point", &json!({"p0": [x, 0], "p1": [x + 150.0, 100]})).unwrap();
            s.execute("sketch.finish", &json!({})).unwrap();
            s.execute("sheet.flange", &json!({"sketch": format!("Base{i}")})).unwrap();
            s.execute("sheet.flange", &json!({"edges": [[x + 75.0, 0, 2]], "height": 20})).unwrap();
        }
        let saved = saved_geometry(&s, &s.doc).unwrap();
        let records = &saved.records;
        let sheets = records["sheets"].as_array().unwrap();
        let texts: std::collections::HashSet<String> = sheets.iter().map(Value::to_string).collect();
        assert_eq!(texts.len(), sheets.len(), "each sheet once");
        let mut held = std::collections::HashSet::new();
        for (r, feature) in s.model.results.iter().zip(records["features"].as_array().unwrap()) {
            let state = &records["states"][feature["state"].as_u64().unwrap() as usize];
            let named = state["sheets"].as_array().unwrap();
            assert_eq!(named.len(), r.state.sheets.len());
            for (i, sheet) in named.iter().zip(&r.state.sheets) {
                let written = &sheets[i.as_u64().unwrap() as usize];
                assert_eq!(*written, serde_json::to_value(sheet).unwrap(), "{}: the record of its own sheet", r.name);
                held.insert(written.to_string());
            }
        }
        assert_eq!(held, texts, "every record written is one a state holds");
        assert!(sheets.len() >= 6, "each part's base, then with its flange: {}", sheets.len());
    }

    /// A design whose saved model would give a body back with other topology opens with the
    /// bodies it was saved with. A pattern copy cut by an automatic extrude came back from the
    /// native codec with two loose vertices more (10 faces, 18 edges, 12 vertices instead of 10);
    /// such a body is not kept, the design is saved without its model and evaluated on opening.
    #[test]
    fn a_design_opens_with_the_bodies_it_was_saved_with_when_one_does_not_decode_to_itself() {
        let mut session = Session::new(Document::new("Roundtrip"));
        for (cmd, p) in [
            ("solid.cylinder", json!({"axis": "Z", "base": [-2, -17, -2], "body_name": "B2", "height": 11, "radius": 6})),
            ("sketch.create", json!({"plane": "YZ"})),
            (
                "sketch.line",
                json!({"closed": true, "ids": ["w3a", "w3b", "w3c", "w3d"], "infer": true, "points": [[-3, -22], [1, -22], [1, -17], [-3, -17]]}),
            ),
            ("sketch.dimension", json!({"entities": ["w3a"], "type": "length", "value": 9})),
            ("solid.extrude", json!({"distance": -1, "operation": "cut"})),
            ("solid.box", json!({"corner": [27, -22, 0], "height": 12, "length": "d3", "operation": "new", "width": 3})),
            ("sketch.create", json!({"plane": "YZ"})),
            (
                "sketch.line",
                json!({"closed": true, "ids": ["w6a", "w6b", "w6c", "w6d"], "infer": true, "points": [[-16, -23], [2, -23], [2, -15], [-16, -15]]}),
            ),
            ("sketch.dimension", json!({"entities": ["w6a"], "type": "length", "value": 11})),
            ("solid.extrude", json!({"distance": 18, "operation": "new"})),
            ("solid.extrude", json!({"distance": -5, "operation": "new"})),
            ("sketch.create", json!({"plane": "YZ"})),
            ("sketch.circle.center", json!({"center": [15, -8], "radius": 9})),
            ("solid.extrude", json!({"distance": "d8", "operation": "new"})),
            ("solid.pattern.circular", json!({"axis": "Y", "bodies": ["Body4"], "count": 3})),
            ("solid.extrude", json!({"distance": -16, "operation": "auto"})),
        ] {
            session.execute(cmd, &p).unwrap();
        }
        let counts = |s: &Session| {
            s.model
                .state()
                .bodies
                .iter()
                .map(|b| (b.name.clone(), b.body.face_count(), b.body.edge_count(), b.body.vertex_count()))
                .collect::<Vec<_>>()
        };
        let bytes = session.design_container(solidcraft_io::DesignCompression::Fast, false, true).unwrap();
        let path = std::env::temp_dir().join(format!("solidcraft-roundtrip-{}.solidcraft", std::process::id()));
        std::fs::write(&path, &bytes).unwrap();
        let mut reopened = Session::default();
        reopened.execute("doc.open", &json!({"path": path})).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(counts(&reopened), counts(&session));
    }

    #[test]
    fn saved_model_needs_no_meshes_and_is_adopted_only_for_its_document() {
        let mut session = Session::new(Document::new("Saved model"));
        session.execute("solid.box", &json!({"length":10,"width":20,"height":30})).unwrap();
        let world = session.world_state().unwrap();
        assert!(world.bodies.iter().all(|b| b.cached_mesh().is_none()));
        let bytes = session.design_container(solidcraft_io::DesignCompression::Fast, false, true).unwrap();
        assert!(world.bodies.iter().all(|b| b.cached_mesh().is_none()), "saving the model must not triangulate");
        assert!(solidcraft_io::read_design_geometry(&bytes).is_some());
        assert!(
            solidcraft_io::read_design_geometry(&session.design_container(solidcraft_io::DesignCompression::Fast, false, false).unwrap()).is_none()
        );
        let path = std::env::temp_dir().join(format!("solidcraft-saved-model-{}.solidcraft", std::process::id()));
        std::fs::write(&path, &bytes).unwrap();
        let mut reopened = Session::default();
        reopened.execute("doc.open", &json!({"path":path})).unwrap();
        assert_eq!(reopened.doc, session.doc);
        assert!(reopened.model.results.iter().all(|r| r.ms == session.model.results[0].ms), "taken from the file, not evaluated");
        let measured = reopened.execute("inspect.measure", &json!({})).unwrap();
        assert!((measured["bodies"][0]["volume_mm3"].as_f64().unwrap() - 6000.).abs() < 1e-6);
        // The model of another document (a value edited in the file) is not taken: the design is
        // evaluated.
        let mut changed = (*session.doc).clone();
        if let Some(solidcraft_doc::FeatureKind::Box { length, .. }) = changed.features.last_mut().map(|f| &mut f.kind) {
            *length = "20".into();
        }
        let saved = saved_geometry(&session, &session.doc).unwrap();
        let other = solidcraft_io::write_design_container_with_cache(&changed, solidcraft_io::DesignCompression::Fast, None, Some(&saved)).unwrap();
        std::fs::write(&path, other).unwrap();
        reopened.execute("doc.open", &json!({"path":path})).unwrap();
        let measured = reopened.execute("inspect.measure", &json!({})).unwrap();
        assert!((measured["bodies"][0]["volume_mm3"].as_f64().unwrap() - 12000.).abs() < 1e-6);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn mesh_import_explicit_crease_normals_are_undoable_and_survive_cached_reopen() {
        let dir = std::env::temp_dir().join(format!("solidcraft-mesh-normals-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("fold.obj");
        std::fs::write(&path, "v 0 0 0\nv 1 0 0\nv 0 1 0\nv 0 -0.8660254037844386 0.5\nf 1 2 3\nf 2 1 4\n").unwrap();
        let mut session = Session { param_check: crate::schema::ParamCheck::Strict, ..Session::default() };
        let before = session.doc.to_json();
        for bad_angle in [json!(-1), json!(181), json!("smooth")] {
            let result = session.execute("file.insert_mesh", &json!({"path":path, "crease_angle_degrees":bad_angle}));
            assert!(result.unwrap_err().to_string().contains("crease_angle_degrees"));
            assert_eq!(session.doc.to_json(), before);
        }
        session.execute("file.insert_mesh", &json!({"path":path, "crease_angle_degrees":40})).unwrap();
        let normals = |s: &Session| s.model.state().bodies[0].mesh().normals.clone();
        let expected = normals(&session);
        assert!(expected[0].y > 0.2 && expected[0].z > 0.9);
        session.execute("edit.undo", &json!({})).unwrap();
        assert!(session.model.state().bodies.is_empty());
        session.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(normals(&session), expected);
        let bytes = session.design_container(solidcraft_io::DesignCompression::Fast, false, true).unwrap();
        let cached = solidcraft_io::read_design_geometry(&bytes).expect("shaded mesh retains native cache");
        assert!(!cached.bodies[0].body.triangle_corner_normals().unwrap().is_empty());
        let saved_path = dir.join("shaded.sc");
        std::fs::write(&saved_path, &bytes).unwrap();
        let mut reopened = Session::default();
        reopened.execute("doc.open", &json!({"path":saved_path})).unwrap();
        assert_eq!(normals(&reopened), expected);
        let doc = solidcraft_io::read_design(&bytes).unwrap();
        let cold = Session::new(doc);
        assert_eq!(normals(&cold), expected);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn mesh_import_zero_crease_restores_legacy_flat_shading_and_survives_reopen() {
        let dir = std::env::temp_dir().join(format!("solidcraft-obj-flat-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let vertices = "v 0 0 0\nv 1 0 0\nv 0 1 0\nv 0 -0.8660254037844386 0.5\n";
        let faces = "f 1 2 3\nf 2 1 4\n";
        let reference_path = dir.join("reference.obj");
        std::fs::write(&reference_path, format!("{vertices}{faces}")).unwrap();
        let mut reference = Session { param_check: crate::schema::ParamCheck::Strict, ..Session::default() };
        reference.execute("file.insert_mesh", &json!({"path":reference_path})).unwrap();
        let display = |s: &Session| {
            let mesh = s.model.state().bodies[0].mesh();
            (mesh.positions.clone(), mesh.triangles.clone(), mesh.normals.clone())
        };
        let expected = display(&reference);
        for (name, source) in
            [("groups", format!("{vertices}s 1\n{faces}")), ("authored", format!("{vertices}vn 0 1 1\nf 1//1 2//1 3//1\nf 2//1 1//1 4//1\n"))]
        {
            let path = dir.join(format!("{name}.obj"));
            std::fs::write(&path, source).unwrap();
            let mut annotated = Session { param_check: crate::schema::ParamCheck::Strict, ..Session::default() };
            annotated.execute("file.insert_mesh", &json!({"path":path})).unwrap();
            assert_ne!(display(&annotated).2, expected.2, "{name} must exercise imported shading");
            let mut session = Session { param_check: crate::schema::ParamCheck::Strict, ..Session::default() };
            session.execute("file.insert_mesh", &json!({"path":path, "crease_angle_degrees":0})).unwrap();
            assert_eq!(display(&session), expected, "{name} uses the exact former flat display path");
            let meshes = session
                .doc
                .features
                .iter()
                .find_map(|f| match &f.kind {
                    solidcraft_doc::FeatureKind::MeshImport { meshes, .. } => Some(meshes),
                    _ => None,
                })
                .unwrap();
            assert!(meshes.iter().all(|mesh| mesh.corner_normals.is_empty()));
            session.execute("edit.undo", &json!({})).unwrap();
            assert!(session.model.state().bodies.is_empty());
            session.execute("edit.redo", &json!({})).unwrap();
            assert_eq!(display(&session), expected);
            let bytes = session.design_container(solidcraft_io::DesignCompression::Fast, false, true).unwrap();
            let cached = solidcraft_io::read_design_geometry(&bytes).unwrap();
            assert!(cached.bodies[0].body.triangle_corner_normals().unwrap().is_empty());
            let saved_path = dir.join(format!("{name}.sc"));
            std::fs::write(&saved_path, &bytes).unwrap();
            let mut reopened = Session::default();
            reopened.execute("doc.open", &json!({"path":saved_path})).unwrap();
            assert_eq!(display(&reopened), expected);
            let cold = Session::new(solidcraft_io::read_design(&bytes).unwrap());
            assert_eq!(display(&cold), expected);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// #9170: a design holding a mesh body (an inserted STL) keeps its saved model and opens from
    /// it. One mesh body left the cache out for the whole model, so every feature, the boxes
    /// too, was evaluated again on opening.
    #[test]
    fn a_design_with_a_mesh_body_keeps_its_saved_model() {
        let dir = std::env::temp_dir().join(format!("solidcraft-mesh-cache-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let stl = dir.join("tetra.stl");
        let facets = [
            [[0, 0, 0], [0, 10, 0], [10, 0, 0]],
            [[0, 0, 0], [10, 0, 0], [0, 0, 10]],
            [[0, 0, 0], [0, 0, 10], [0, 10, 0]],
            [[10, 0, 0], [0, 10, 0], [0, 0, 10]],
        ];
        let mut text = String::from("solid tetra\n");
        for f in facets {
            text += "facet normal 0 0 0\nouter loop\n";
            for [x, y, z] in f {
                text += &format!("vertex {x} {y} {z}\n");
            }
            text += "endloop\nendfacet\n";
        }
        std::fs::write(&stl, text + "endsolid tetra\n").unwrap();
        let mut session = Session::new(Document::new("Boxes and a mesh"));
        for x in [0, 40, 80] {
            session.execute("solid.box", &json!({"length": 20, "width": 20, "height": 20, "corner": [x, 0, 0], "operation": "new_body"})).unwrap();
        }
        session.execute("file.insert_mesh", &json!({"path": stl})).unwrap();
        let triangles = |s: &Session| s.model.state().bodies.iter().filter_map(|b| b.body.triangle_mesh().map(|(_, t)| t.len())).collect::<Vec<_>>();
        assert_eq!(triangles(&session), [4], "one mesh body");
        let bytes = session.design_container(solidcraft_io::DesignCompression::Fast, false, true).unwrap();
        assert!(solidcraft_io::read_design_geometry(&bytes).is_some(), "the model is saved with the mesh body");
        let path = dir.join("boxes-mesh.solidcraft");
        std::fs::write(&path, &bytes).unwrap();
        let mut reopened = Session::default();
        reopened.execute("doc.open", &json!({"path": path})).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(reopened.model.results.len(), session.model.results.len());
        assert!(reopened.model.results.iter().zip(&session.model.results).all(|(a, b)| a.ms == b.ms), "taken from the file, not evaluated");
        assert_eq!(triangles(&reopened), [4]);
        let volume = |s: &mut Session| s.execute("inspect.measure", &json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap();
        let (saved, opened) = (volume(&mut session), volume(&mut reopened));
        assert!((opened - saved).abs() < 1e-9 && (saved - (3. * 8000. + 1000. / 6.)).abs() < 1e-6, "{opened} vs {saved}");
    }

    /// A sheet-metal design keeps each sheet body once in its saved model, not again with every
    /// later feature's state: a design of many features after its sheet metal wrote the sheets
    /// over and over, and past the cache's size limit kept only some of its states.
    #[test]
    fn a_saved_model_keeps_each_sheet_body_once() {
        let design = |boxes: usize| {
            let mut s = Session::new(Document::new("Sheet"));
            let run = |s: &mut Session, id: &str, p: Value| s.execute(id, &p).unwrap();
            run(
                &mut s,
                "sheet.manage_rules",
                json!({"name":"AL2","thickness":"2 mm","k_factor":0.5,"bend_radius":"1 mm","gap":"0.5 mm","active":true}),
            );
            run(&mut s, "sketch.create", json!({"plane":"XY","name":"Base"}));
            run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[200,150]}));
            run(&mut s, "sketch.finish", json!({}));
            run(&mut s, "sheet.flange", json!({"sketch":"Base"}));
            run(&mut s, "sheet.flange", json!({"edges":[[100,0,2],[200,75,2],[100,150,2],[0,75,2]],"height":"20 mm","angle":"90 deg"}));
            // A panel's holes: each one is a cut-out the sheet keeps.
            for i in 0..16 {
                run(&mut s, "solid.hole", json!({"position":[25 + 10 * i,40,2],"diameter":5}));
            }
            for i in 0..boxes {
                run(&mut s, "solid.box", json!({"length":5,"width":5,"height":5,"corner":[300 + 10 * i,0,0],"operation":"new"}));
            }
            s
        };
        let size = |s: &Session| serde_json::to_vec(&saved_geometry(s, &s.doc).unwrap().records).unwrap().len();
        let (few, many) = (design(2), design(12));
        let sheet = serde_json::to_vec(&many.model.state().sheets).unwrap().len();
        let per_feature = (size(&many) - size(&few)) / 10;
        assert!(per_feature * 2 < sheet, "each later feature adds {per_feature} bytes, the sheet is {sheet}");
        // Taken from the file as saved: the sheet is the one evaluated.
        let flat = many.model.state().sheets.clone();
        let path = std::env::temp_dir().join(format!("solidcraft-sheet-cache-{}.solidcraft", std::process::id()));
        std::fs::write(&path, many.design_container(solidcraft_io::DesignCompression::Fast, false, true).unwrap()).unwrap();
        let mut reopened = Session::default();
        reopened.execute("doc.open", &json!({"path":path})).unwrap();
        assert!(reopened.model.results.iter().zip(&many.model.results).all(|(a, b)| a.ms == b.ms), "taken from the file, not evaluated");
        assert_eq!(serde_json::to_value(&reopened.model.state().sheets).unwrap(), serde_json::to_value(&flat).unwrap());
        std::fs::remove_file(path).unwrap();
    }

    /// A design's saved model keeps each set of sketch keys once, not again with every state:
    /// every state wrote the key of each sketch before it, so a design of many sketches and many
    /// features after them (5,484 features) passed the cache's size limit and kept only some of
    /// its states. A set is written as the changes from the one before it; the model opens from
    /// the file with every state (the keys read back: `eval_cost_tests`).
    #[test]
    fn a_saved_model_keeps_each_set_of_sketch_keys_once() {
        let mut s = Session::new(Document::new("Sketches"));
        let run = |s: &mut Session, id: &str, p: Value| s.execute(id, &p).unwrap();
        for i in 0..6 {
            run(&mut s, "sketch.create", json!({"plane":"XY","name":format!("Pad{i}")}));
            run(&mut s, "sketch.rectangle.two_point", json!({"p0":[20 * i,0],"p1":[20 * i + 10,10]}));
            run(&mut s, "solid.extrude", json!({"distance":5,"operation":"new"}));
        }
        for i in 0..10 {
            run(&mut s, "solid.box", json!({"length":5,"width":5,"height":5,"corner":[20 * i,40,0],"operation":"new"}));
        }
        let saved = saved_geometry(&s, &s.doc).unwrap();
        let features = saved.records["features"].as_array().unwrap();
        assert_eq!(features.len(), s.doc.features.len());
        assert!(features.iter().all(|f| f.get("state").is_some()), "every state kept");
        let states = saved.records["states"].as_array().unwrap();
        let sets = saved.records["sketch_key_sets"].as_array().unwrap();
        // One set per sketch: each later one adds that sketch's key to the one before it.
        assert_eq!(sets.len(), 6, "{sets:?}");
        assert_eq!(sets[0]["set"].as_array().unwrap().len(), 1);
        for (i, set) in sets.iter().enumerate().skip(1) {
            assert_eq!(
                (set["base"].as_u64(), set["set"].as_array().unwrap().len(), set["drop"].as_array().unwrap().len()),
                (Some(i as u64 - 1), 1, 0)
            );
        }
        let last = sets.len() - 1;
        assert_eq!(
            states.iter().filter(|st| st["sketch_key_set"] == json!(last)).count(),
            2 + 10,
            "the last sketch, its extrude and every box share one set"
        );
        let path = std::env::temp_dir().join(format!("solidcraft-sketch-key-cache-{}.solidcraft", std::process::id()));
        let bytes = solidcraft_io::write_design_container_with_cache(&s.doc, solidcraft_io::DesignCompression::Fast, None, Some(&saved)).unwrap();
        std::fs::write(&path, bytes).unwrap();
        let mut reopened = Session::default();
        reopened.execute("doc.open", &json!({"path": path})).unwrap();
        assert_eq!(reopened.model.last_recomputed, 0, "taken from the file, not evaluated");
        assert!(reopened.model.results.iter().zip(&s.model.results).all(|(a, b)| a.ms == b.ms));
        std::fs::remove_file(path).unwrap();
    }

    /// A design's saved model writes each tap marking once, and every state names the markings
    /// it holds by place: every state listed the markings of all the features before it, so a
    /// design of many marked cylinders grew with the square of its features (400 cylinders: 34 of
    /// a 37 MB manifest), passed the cache's size limit and kept only some of its states. Opened
    /// from the file, every state holds the markings it was evaluated with, to the bit.
    #[test]
    fn a_saved_model_writes_each_tap_marking_once() {
        let design = |cylinders: usize| {
            let mut s = Session::new(Document::new("Marked"));
            let run = |s: &mut Session, id: &str, p: Value| s.execute(id, &p).unwrap();
            for i in 0..cylinders {
                let x = 10 * i;
                run(&mut s, "solid.cylinder", json!({"radius":3,"height":6,"base":[x,0,0]}));
                run(&mut s, "solid.thread", json!({"face":[x + 3,0,2],"designation":"M6","length":4}));
            }
            s
        };
        let size = |s: &Session| serde_json::to_vec(&saved_geometry(s, &s.doc).unwrap().records).unwrap().len();
        let (few, s) = (design(4), design(12));
        let markings = &s.model.state().threads;
        assert_eq!(markings.len(), 12, "one marking per cylinder");
        let per_cylinder = (size(&s) - size(&few)) / 8;
        let all = serde_json::to_vec(markings).unwrap().len();
        assert!(per_cylinder * 2 < all, "each later cylinder adds {per_cylinder} bytes, its markings are {all}");
        let saved = saved_geometry(&s, &s.doc).unwrap();
        let records = &saved.records;
        let written = records["threads"].as_array().unwrap();
        let texts: std::collections::HashSet<String> = written.iter().map(Value::to_string).collect();
        assert_eq!((texts.len(), written.len()), (12, 12), "each marking once in the manifest");
        for (r, feature) in s.model.results.iter().zip(records["features"].as_array().unwrap()) {
            let state = &records["states"][feature["state"].as_u64().unwrap() as usize];
            let named: Vec<Value> = state["threads"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|run| {
                    let first = run[0].as_u64().unwrap() as usize;
                    (first..first + run[1].as_u64().unwrap() as usize).map(|i| written[i].clone())
                })
                .collect();
            assert_eq!(Value::Array(named), serde_json::to_value(&r.state.threads).unwrap(), "{}: its own markings", r.name);
        }
        // Opened from the file as saved: nothing evaluated, every state as evaluated.
        let bytes = solidcraft_io::write_design_container_with_cache(&s.doc, solidcraft_io::DesignCompression::Fast, None, Some(&saved)).unwrap();
        let kept = solidcraft_io::DesignContainerReader::new(std::io::Cursor::new(&bytes)).unwrap().geometry().unwrap();
        let reopened = Session::opened(solidcraft_io::read_design(&bytes).unwrap(), Some(&kept));
        assert_eq!(reopened.model.last_recomputed, 0, "taken from the file, not evaluated");
        assert_eq!(reopened.model.results.len(), s.model.results.len());
        for (a, b) in reopened.model.results.iter().zip(&s.model.results) {
            assert_eq!(a.ms, b.ms, "{}: taken from the file", b.name);
            assert_eq!(serde_json::to_string(&a.state.threads).unwrap(), serde_json::to_string(&b.state.threads).unwrap(), "{}", b.name);
            let names = |r: &solidcraft_doc::FeatureResult| r.state.bodies.iter().map(|b| (b.name.clone(), b.feature)).collect::<Vec<_>>();
            assert_eq!(names(a), names(b), "{}", b.name);
        }
    }

    /// A saved model whose states name one large tap marking over and over is refused: every
    /// state holds its own copy of each marking it lists, so a marking counts by its text.
    #[test]
    fn a_saved_model_repeating_a_large_tap_marking_is_refused() {
        let mut s = Session::new(Document::new("Marked"));
        s.execute("solid.cylinder", &json!({"radius":3,"height":6,"base":[0,0,0]})).unwrap();
        s.execute("solid.thread", &json!({"face":[3,0,2],"designation":"M6","length":4})).unwrap();
        let saved = saved_geometry(&s, &s.doc).unwrap();
        assert!(solidcraft_doc::Model::adopt(&s.doc, &saved).is_ok());
        let mut hostile = solidcraft_doc::SavedModel { records: saved.records.clone(), bodies: saved.bodies.clone() };
        hostile.records["threads"][0]["designation"] = json!("M".repeat(1 << 20));
        let repeated: Vec<Value> = (0..4000).map(|_| json!([0, 1])).collect();
        let last = hostile.records["states"].as_array().unwrap().len() - 1;
        hostile.records["states"][last]["threads"] = Value::Array(repeated);
        assert!(solidcraft_doc::Model::adopt(&s.doc, &hostile).is_err());
    }

    #[test]
    #[ignore = "manual native cache save benchmark; generate issue303-step.json first"]
    fn benchmark_native_cache_save() {
        let target = std::env::var_os("CARGO_TARGET_DIR").map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from("target"));
        let input = std::env::var_os("SOLIDCRAFT_BENCH_FILE").map(std::path::PathBuf::from).unwrap_or_else(|| target.join("issue303-step.json"));
        let mut session = Session::new(solidcraft_io::read_design(&std::fs::read(input).unwrap()).unwrap());
        for body in &session.world_state().unwrap().bodies {
            body.mesh();
        }
        let modes = [("json", false, "fast"), ("container", false, "fast"), ("container", true, "fast"), ("container", true, "compact")];
        let mut samples = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
        for round in 0..9 {
            for i in if round % 2 == 0 { [0, 1, 2, 3] } else { [3, 2, 1, 0] } {
                let (format, cache, compression) = modes[i];
                let path = target.join(format!("issue303-native-{i}.solidcraft"));
                let start = std::time::Instant::now();
                session.execute("file.save", &json!({"path":path,"format":format,"cache":cache,"compression":compression})).unwrap();
                samples[i].push(start.elapsed().as_secs_f64() * 1000.);
            }
        }
        for (i, times) in samples.iter_mut().enumerate() {
            times.sort_by(f64::total_cmp);
            let bytes = std::fs::read(target.join(format!("issue303-native-{i}.solidcraft"))).unwrap();
            assert_eq!(solidcraft_io::read_design(&bytes).unwrap(), *session.doc);
            assert_eq!(solidcraft_io::read_design_geometry(&bytes).is_some(), modes[i].1);
            eprintln!("native_save mode={i} median_ms={:.3} bytes={}", times[4], bytes.len());
        }
    }

    #[test]
    #[ignore = "manual fresh-process opening benchmark; one encoding per process"]
    fn benchmark_native_cache_open() {
        let target = std::env::var_os("CARGO_TARGET_DIR").map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from("target"));
        let mode = std::env::var("SOLIDCRAFT_BENCH_ENCODING").unwrap_or_else(|_| "2".into());
        let path = target.join(format!("issue303-native-{mode}.solidcraft"));
        let display = std::sync::Arc::new(std::sync::Mutex::new(None));
        let notify = display.clone();
        let start = std::time::Instant::now();
        let mut session = Session {
            open_preview: Some(Box::new(move |_| {
                *notify.lock().unwrap() = Some(start.elapsed().as_secs_f64() * 1000.);
            })),
            ..Session::default()
        };
        let result = session.execute("doc.open", &json!({"path":path})).unwrap();
        assert_eq!(result["errors"], 0);
        let reconstructed = start.elapsed().as_secs_f64() * 1000.;
        let mut triangles = 0;
        let world = session.world_state().unwrap();
        for body in &world.bodies {
            triangles += body.mesh().triangles.len();
        }
        let ready = start.elapsed().as_secs_f64() * 1000.;
        eprintln!(
            "native_open mode={mode} provisional_ms={:?} rebuilt_ms={reconstructed:.3} meshes_ready_ms={ready:.3} bodies={} triangles={triangles}",
            *display.lock().unwrap(),
            world.bodies.len()
        );
    }

    fn name_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("solidcraft-name-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn save_as_names_an_untitled_design_after_its_file() {
        let dir = name_dir("untitled");
        let path = dir.join("bracket.solidcraft");
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length":10,"width":20,"height":30})).unwrap();
        assert_eq!(s.doc.name, "Untitled");
        let r = s.execute("file.save_as", &json!({"path":path})).unwrap();
        assert_eq!(r["name"], "bracket");
        assert_eq!(s.doc.name, "bracket");
        assert!(!s.is_dirty(), "taking the file's name is part of the save");
        assert_eq!(solidcraft_io::read_design(&std::fs::read(&path).unwrap()).unwrap().name, "bracket");
        // Undoing the box does not bring the old name back.
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc.name, "bracket");
        // Saved under another file, a name taken from the old file follows it (#1710), in the
        // file too, so reopening shows it.
        let copy = dir.join("copy.solidcraft");
        s.execute("file.save_as", &json!({"path":copy})).unwrap();
        assert_eq!(s.doc.name, "copy");
        let mut reopened = Session::default();
        reopened.execute("doc.open", &json!({"path":copy})).unwrap();
        assert_eq!(reopened.doc.name, "copy");
        // Saving again to the same file changes nothing.
        s.execute("file.save", &json!({})).unwrap();
        assert_eq!(s.doc.name, "copy");
        // As in the issue: a design opened from its file, then saved as another.
        reopened.execute("file.save_as", &json!({"path":dir.join("third.solidcraft")})).unwrap();
        assert_eq!(reopened.doc.name, "third");
        // A failed save leaves the name alone.
        let mut failed = Session::default();
        assert!(failed.execute("file.save_as", &json!({"path":dir.join("missing-dir").join("plate.solidcraft")})).is_err());
        assert_eq!(failed.doc.name, "Untitled");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_design_is_not_saved_under_a_name_that_opens_as_another_file() {
        // #9745: a design saved as `part.step` or `part.stl` was refused or opened as a broken mesh.
        let dir = name_dir("other-kind");
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length":10,"width":10,"height":10})).unwrap();
        for name in
            ["part.step", "part.STP", "part.igs", "part.stl", "part.obj", "part.3mf", "part.glb", "part.dxf", "part.svg", "part.ply", "part.scdwg"]
        {
            let path = dir.join(name);
            for command in ["file.save_as", "file.save"] {
                let e = s.execute(command, &json!({"path":path})).unwrap_err().to_string();
                assert!(e.contains("not as a design") && e.contains(".sc"), "{command} {name}: {e}");
                assert!(!path.exists(), "{command} {name} wrote the file");
            }
        }
        let e = s.execute("file.save_as", &json!({"path":dir.join("part.stl")})).unwrap_err().to_string();
        assert!(e.contains("file.export"), "STL is an export format: {e}");
        assert!(s.path.is_none() && s.is_dirty(), "a refused save changes nothing");
        // Names that open again as the design still save, and what they write opens as it.
        for name in ["part.sc", "part.json", "part.txt", "part"] {
            let path = dir.join(name);
            s.execute("file.save_as", &json!({"path":path})).unwrap();
            let mut reopened = Session::default();
            reopened.execute("doc.open", &json!({"path":path})).unwrap();
            assert_eq!(reopened.doc.features.len(), 1, "{name} reopens as the design");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn save_as_keeps_a_name_the_user_chose() {
        let dir = name_dir("named");
        let mut s = Session::default();
        s.execute("file.new", &json!({"name":"Gearbox lid"})).unwrap();
        s.execute("file.save_as", &json!({"path":dir.join("lid-v2.solidcraft")})).unwrap();
        assert_eq!(s.doc.name, "Gearbox lid");
        let mut reopened = Session::default();
        reopened.execute("doc.open", &json!({"path":dir.join("lid-v2.solidcraft")})).unwrap();
        assert_eq!(reopened.doc.name, "Gearbox lid");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// #4907: after Save As the open design *is* the new file — tab, browser and the name stored
    /// in the file — whenever its name didn't come from its current file: a sample's name, or a
    /// file renamed outside SolidCraft or named by someone else. A plain Save never renames.
    #[test]
    fn save_as_names_a_borrowed_name_after_the_new_file() {
        let dir = name_dir("borrowed");
        // The issue's steps: the sample "Sample Plate" saved as "Bracket v2.solidcraft".
        let mut s = Session::default();
        s.doc_mut().name = "Sample Plate".into();
        s.execute("solid.box", &json!({"length":10,"width":20,"height":30})).unwrap();
        let path = dir.join("Bracket v2.solidcraft");
        let r = s.execute("file.save_as", &json!({"path":path})).unwrap();
        assert_eq!(r["name"], "Bracket v2");
        assert_eq!(s.doc.name, "Bracket v2");
        assert!(!s.is_dirty(), "taking the new file's name is part of the save");
        assert_eq!(solidcraft_io::read_design(&std::fs::read(&path).unwrap()).unwrap().name, "Bracket v2");
        // Undoing the box does not bring the sample's name back.
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc.name, "Bracket v2");
        // Reopening shows the new name too.
        let mut reopened = Session::default();
        reopened.execute("doc.open", &json!({"path":path})).unwrap();
        assert_eq!(reopened.doc.name, "Bracket v2");
        // A file whose stored name is not its own (renamed outside SolidCraft, or saved by a
        // version before #1710): Save As names the design after the new file, while a plain
        // Save to the file it has leaves the name alone.
        let odd = dir.join("renamed.solidcraft");
        std::fs::write(&odd, solidcraft_io::write_design(&Document::new("Sample Plate"))).unwrap();
        let mut s = Session::default();
        s.execute("doc.open", &json!({"path":odd})).unwrap();
        assert_eq!(s.doc.name, "Sample Plate");
        s.execute("file.save", &json!({})).unwrap();
        assert_eq!(s.doc.name, "Sample Plate", "a plain Save does not rename behind the user's back");
        s.execute("file.save_as", &json!({"path":dir.join("Bracket v3.solidcraft")})).unwrap();
        assert_eq!(s.doc.name, "Bracket v3");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn opening_an_untitled_file_shows_its_file_name() {
        let dir = name_dir("open");
        for (stored, file) in [("Untitled", "hinge.solidcraft"), ("", "spacer.json")] {
            let path = dir.join(file);
            let bytes = solidcraft_io::write_design(&Document::new(stored));
            std::fs::write(&path, bytes).unwrap();
            let mut s = Session::default();
            let r = s.execute("doc.open", &json!({"path":path})).unwrap();
            let stem = file.split('.').next().unwrap();
            assert_eq!(s.doc.name, stem);
            assert_eq!(r["name"], stem);
            assert!(!s.is_dirty(), "showing the file's name is not a change to save");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn design_save_formats_reopen_and_preserve_atomic_backups() {
        let dir = std::env::temp_dir().join(format!("solidcraft-container-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let packaged = dir.join("design.solidcraft");
        let plain = dir.join("design.json");
        let export = dir.join("export.json");
        let mut session = Session::default();
        session.execute("solid.box", &json!({"length":10,"width":20,"height":30})).unwrap();
        session.execute("file.save_as", &json!({"path":packaged})).unwrap();
        let original = std::fs::read(&packaged).unwrap();
        assert!(solidcraft_io::is_design_container(&original));
        session.execute("file.save_as", &json!({"path":plain})).unwrap();
        assert!(std::fs::read_to_string(&plain).unwrap().contains("solidcraft/"));
        let path = session.path.clone();
        session.execute("file.export", &json!({"path":export,"format":"json"})).unwrap();
        assert_eq!(session.path, path);
        for file in [&packaged, &plain, &export] {
            let mut reopened = Session::default();
            reopened.execute("doc.open", &json!({"path":file})).unwrap();
            assert_eq!(reopened.doc, session.doc);
            let measure = reopened.execute("inspect.measure", &json!({})).unwrap();
            assert_eq!(measure["body_count"], 1);
            assert!((measure["bodies"][0]["volume_mm3"].as_f64().unwrap() - 6000.0).abs() < 1e-6);
        }
        session.execute("file.save", &json!({"path":packaged})).unwrap();
        assert_eq!(std::fs::read(solidcraft_io::backup_path(&packaged)).unwrap(), original);
        let before = std::fs::read(&packaged).unwrap();
        assert!(session.execute("file.save", &json!({"path":packaged,"format":"invalid"})).is_err());
        assert_eq!(std::fs::read(&packaged).unwrap(), before);
        session.execute("file.save", &json!({"path":packaged,"format":"json"})).unwrap();
        assert!(std::fs::read_to_string(&packaged).unwrap().contains("solidcraft/"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A watched open reports the timeline's progress; cancelled, it skips the rebuild.
    #[test]
    fn watched_open_reports_progress_and_stops_when_cancelled() {
        use solidcraft_doc::progress::{Progress, watch};
        use std::sync::Arc;
        let dir = std::env::temp_dir().join(format!("solidcraft-watched-open-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("three.solidcraft").to_string_lossy().to_string();
        let mut s = Session::default();
        for x in [0, 20, 40] {
            s.execute("solid.box", &json!({"length":10,"width":10,"height":10,"corner":[x,0,0]})).unwrap();
        }
        // Without its model: the timeline is evaluated (and watched) as it opens.
        s.execute("file.save", &json!({"path":path, "cache": false})).unwrap();

        let progress = Arc::new(Progress::default());
        let mut opened = Session::default();
        watch(progress.clone(), || opened.execute("doc.open", &json!({"path":path}))).unwrap();
        assert_eq!(progress.features(), (3, 3));
        assert_eq!(opened.world_state().unwrap().bodies.len(), 3);

        let progress = Arc::new(Progress::default());
        progress.cancel();
        let mut stopped = Session::default();
        watch(progress, || stopped.execute("doc.open", &json!({"path":path}))).unwrap();
        assert!(stopped.model.results.iter().all(|r| r.skipped), "no feature was rebuilt after Cancel");
        assert!(stopped.world_state().unwrap().bodies.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// #10198: a design opened a feature at a time ([`OpenSteps`]) is the design `doc.open`
    /// opens: the same features, results, bodies and history marker, with its file and name. Its
    /// import is offered before the step that evaluates it, and other files are left to `doc.open`.
    #[test]
    fn a_design_opened_a_feature_at_a_time_is_the_one_doc_open_opens() {
        use super::open_steps::OpenSteps;
        let dir = std::env::temp_dir().join(format!("solidcraft-open-steps-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("steps.sc").to_string_lossy().to_string();
        let step = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/step-conformance/cylinder_seam.step");
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length":10,"width":10,"height":10})).unwrap();
        s.execute("file.insert_step", &json!({"path": step})).unwrap();
        s.execute("component.activate", &json!({"component": "root"})).unwrap();
        s.execute("solid.box", &json!({"length":5,"width":5,"height":5,"corner":[40,0,0]})).unwrap();
        s.execute("solid.box", &json!({"length":5,"width":5,"height":5,"corner":[60,0,0]})).unwrap();
        let features = s.doc.features.len();
        s.execute("timeline.rollback", &json!({"position": features - 1})).unwrap();
        s.execute("file.save", &json!({"path": path, "cache": false})).unwrap();

        let mut whole = Session::default();
        let opened = whole.execute("doc.open", &json!({"path": path, "read_only": true})).unwrap();

        let mut steps =
            OpenSteps::start(&json!({"path": path, "read_only": true}), Default::default()).unwrap().expect("a design is opened in steps");
        assert_eq!(steps.path(), path);
        assert_eq!(steps.progress(), (0, features - 1), "the saved marker is where evaluation stops");
        let mut imports = 0;
        let mut n = 0;
        while !steps.done() {
            if let Some(text) = steps.next_import() {
                assert!(text.contains("ISO-10303-21"));
                imports += 1;
            }
            steps.step();
            n += 1;
            assert_eq!(steps.progress().0, n);
        }
        assert_eq!((imports, n), (1, features - 1));
        assert!(!steps.step(), "nothing is left to evaluate");
        let mut stepped = Session::default();
        let reply = steps.finish(&mut stepped).unwrap();

        assert_eq!(reply, opened);
        assert_eq!(stepped.doc, whole.doc);
        assert_eq!(stepped.doc.marker, Some(features - 1));
        assert_eq!((stepped.path.as_deref(), stepped.read_only), (Some(path.as_str()), true));
        let summary = |s: &Session| -> Vec<(u64, bool, bool)> { s.model.results.iter().map(|r| (r.id, r.skipped, r.error.is_some())).collect() };
        assert_eq!(summary(&stepped), summary(&whole));
        let bodies = |s: &Session| -> Vec<(String, usize)> {
            s.world_state().unwrap().bodies.iter().map(|b| (b.name.clone(), b.mesh().triangles.len())).collect()
        };
        assert_eq!(bodies(&stepped), bodies(&whole));
        assert_eq!(stepped.world_state().unwrap().bodies.len(), 3, "the box, the cylinder and the box before the marker");
        assert!(stepped.model.is_current(&stepped.doc));

        // Other files, and STEP-only options, are opened by `doc.open`.
        for p in [json!({"path": step}), json!({"path": "part.stl"}), json!({"path": path, "up": "y"}), json!({})] {
            assert!(OpenSteps::start(&p, Default::default()).unwrap().is_none(), "{p}");
        }
        assert!(OpenSteps::start(&json!({"path": dir.join("missing.sc").to_string_lossy()}), Default::default()).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A design opened a feature at a time is evaluated under the session's check mode, as
    /// `doc.open` is. Started under another mode than the session's (the mode changed meanwhile),
    /// nothing evaluated under it stands: every feature is evaluated again. The bar's last round
    /// is one the check refuses on solidcore (kernel case
    /// fillets/1207-fillet-bar-top-edge-after-boss-arc-round-r0-5), so there the modes differ in
    /// their errors too.
    #[test]
    fn a_design_opened_a_feature_at_a_time_is_checked_under_the_sessions_mode() {
        use super::open_steps::OpenSteps;
        use solidcraft_kernel::Validation;
        let dir = std::env::temp_dir().join(format!("solidcraft-open-steps-mode-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("steps.sc").to_string_lossy().to_string();
        let mut s = Session::default();
        s.set_validation(Validation::Off);
        s.execute("solid.box", &json!({"length": 100, "width": 30, "height": 10, "corner": [0, -15, 0], "body_name": "Arm"})).unwrap();
        s.execute("solid.cylinder", &json!({"diameter": 40, "height": 20, "base": [0, 0, 0], "operation": "join", "targets": ["Arm"]})).unwrap();
        s.execute("solid.fillet", &json!({"edges": [[16, 12, 10]], "radius": 2})).unwrap();
        s.execute("solid.fillet", &json!({"edges": [[60, -15, 10]], "radius": 0.5})).unwrap();
        s.execute("file.save", &json!({"path": path, "cache": false})).unwrap();
        let errors = |s: &Session| s.model.results.iter().filter(|r| r.error.is_some()).count();
        let open = |mode: Validation| {
            let mut whole = Session::default();
            whole.set_validation(mode);
            whole.execute("doc.open", &json!({"path": path})).unwrap();
            errors(&whole)
        };
        // Each evaluation of the design takes seconds (its last round), so each mode's reference
        // is opened once and each starting mode is tried once.
        for mode in Validation::ALL {
            let reference = open(mode);
            let mut starts = vec![mode];
            for other in [Validation::EachFeature, Validation::Off] {
                if !starts.contains(&other) {
                    starts.push(other);
                }
            }
            for started in starts {
                let mut steps = OpenSteps::start(&json!({"path": path}), started).unwrap().expect("a design is opened in steps");
                while steps.step() {}
                let mut stepped = Session::default();
                stepped.set_validation(mode);
                steps.finish(&mut stepped).unwrap();
                assert_eq!(stepped.validation(), mode);
                assert_eq!(errors(&stepped), reference, "started under {started:?}, the session's mode {mode:?}");
                let again = if started == mode { 0 } else { stepped.doc.features.len() };
                assert_eq!(stepped.model.last_recomputed, again, "started under {started:?}, the session's mode {mode:?}");
                assert!(stepped.model.is_current(&stepped.doc));
            }
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Every option the format registry offers is a parameter its owning command documents,
    /// so the registry and the commands can't drift apart.
    #[test]
    fn registry_options_name_documented_command_parameters() {
        use solidcraft_io::formats;
        let params_of = |id: &str| crate::find_command(id).map(|c| c.params).unwrap_or_else(|| panic!("registry names unknown command {id}"));
        for f in formats::FORMATS {
            if let Some(import) = &f.import {
                for o in import.options {
                    for id in ["doc.open", if import.target == formats::ImportTarget::Mesh { "file.insert_mesh" } else { "file.insert_step" }] {
                        assert!(params_of(id).contains(o.param), "{}: `{}` is not a documented parameter of {id}", f.id, o.param);
                    }
                }
            }
            if let Some(export) = &f.export {
                let params = params_of(export.command);
                if let Some(format) = export.format {
                    assert!(params.contains(format), "{}: `{format}` is not a documented format of {}", f.id, export.command);
                }
                for o in export.options {
                    assert!(params.contains(o.param), "{}: `{}` is not a documented parameter of {}", f.id, o.param, export.command);
                }
            }
        }
        for o in formats::DESIGN_SAVE_OPTIONS {
            assert!(params_of("file.save_as").contains(o.param), "`{}` is not a documented parameter of file.save_as", o.param);
        }
    }
}
