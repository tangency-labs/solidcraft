//! Documents and files: new, open designs, CAD files and drawings, insert STEP, save, export.

use crate::thumbnail;

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
use crate::{EngineError, Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("file.new", "New Design", new_doc).icon("new").key("Ctrl+N").noundo().params("name?"),
    CommandSpec::new("doc.open", "Open", open)
        .icon("open")
        .key("Ctrl+O")
        .noundo()
        .params("path: .sc design (or a .solidcraft design from an earlier release), .dxf/.svg drawing (opens as a new, unsaved design with an XY sketch, using sketch.insert_dxf/sketch.insert_svg units and defaults), or a .step/.stp/.igs/.fcstd/.3dm/.3mf/.stl/.obj/.glb/.gltf file (STEP, IGES, FreeCAD, Rhino, 3MF, STL, OBJ or glTF; opens as a new design holding the file as a component named after it, as file.insert_step and file.insert_mesh make), or a .ply/.xyz/.pts point cloud (opens as a new design holding it, as pointcloud.insert makes; its `units` and `max_points` as there); up?: z|y for a STEP, IGES, FreeCAD, Rhino or glTF file (see file.insert_step and file.insert_mesh); names?: auto|solid|product|instance for a STEP, IGES, FreeCAD or Rhino file (see file.insert_step); read_only?: bool (a design: saving to its own file is refused, file.save_as keeps the changes elsewhere) → open_elsewhere: {host, user, app, opened} when another SolidCraft has the design open (its lock file is fresh)"),
    CommandSpec::new("file.insert_step", "Insert STEP", insert_step)
        .at("SOLID", "INSERT")
        .icon("import")
        .params("path: STEP, IGES, FreeCAD .FCStd or Rhino .3dm file (its bodies join the design as an Import base feature in a new component named after the file, placed in the active component; an assembly's products become nested components, a repeated part one component with an occurrence per placement, all held as one rigid group until component.unlock → component, component_name, occurrence, subcomponents?, rigid_joints?); name? (the feature's and the component's); up?: z|y (default z: the file's axes are kept; y: the file has Y up, so the import's component is turned a quarter turn about X, Y becoming Z → stand_up: its occurrence); names?: auto|solid|product|instance (default auto: keep meaningful solid names, replace placeholders with the nearest named product, else instance; solid: the solid's own name; product: the part's name; instance: the occurrence's name, else the product's); matrix?: [12 or 16 numbers] (where the new component's occurrence sits in the active component, after `up`: a rigid row-major 3x4 rotation-and-translation or 4x4 matrix, flat or as rows, as occurrence.move takes it; a scale, shear or mirror is refused before the file is read); tolerance? (see occurrence.move)"),
    CommandSpec::new("file.insert_mesh", "Insert Mesh", insert_mesh)
        .at("SOLID", "INSERT")
        .icon("import")
        .params("path: .3mf, .stl, .obj, .ply (with faces), .glb or .gltf file (its meshes join the design as mesh bodies in a new component named after the file → component, component_name, occurrence; a glTF file's node tree becomes nested components held as one rigid group until component.unlock, a mesh placed again another occurrence, its materials the bodies' appearances, and its metres millimetres); name?; up?: z|y for a glTF file (default y: glTF has Y up, so the import's component is turned a quarter turn about X, Y becoming Z → stand_up: its occurrence; z: the file's axes are kept)"),
    CommandSpec::new("file.save", "Save", save)
        .icon("save")
        .key("Ctrl+S")
        .noundo()
        .params("path? (default: current file); format?: container|json (default: json for .json, otherwise container); compression?: fast|compact (default: fast); cache?: bool (the evaluated model, so the design opens without rebuilding; default: true); preview?: bool (saved display meshes, default: false); overwrite?: bool (replace the design's file even though another program or computer changed it since it was opened or saved; without it that save is refused)"),
    CommandSpec::new("file.save_as", "Save As", save_as)
        .icon("save")
        .key("Ctrl+Shift+S")
        .noundo()
        .params("path; format?: container|json (default: json for .json, otherwise container); compression?: fast|compact (default: fast); cache?: bool (the evaluated model, so the design opens without rebuilding; default: true); preview?: bool (saved display meshes, default: false); overwrite?: bool (as file.save, when `path` is the design's own file)"),
    CommandSpec::new("doc.recovery_list", "Recoverable Designs", recovery_list).icon("restore")
        .noundo()
        .params("dir? (default: the recovery folder) → designs autosaved by apps that crashed or closed with unsaved changes"),
    CommandSpec::new("doc.recover", "Recover Design", recover).icon("restore")
        .noundo()
        .params("id (from doc.recovery_list); dir?; discard?: bool (delete the entry once open) — opens it with its file path, unsaved"),
    CommandSpec::new("doc.recovery_discard", "Discard Recovered Design", recovery_discard).icon("delete").noundo().params("id, or all: true; dir?"),
    CommandSpec::new("file.export", "Export", export)
        .icon("export")
        .noundo()
        .params("path; format?: stl|stla|obj|step|3mf|3mf-tagged|urdf|json (default from extension; 3mf-tagged: 3MF whose triangles carry their B-rep face, at full precision, for meshing and analysis tools; docs/formats/3mf-face-tags.md; obj: the colours of the bodies go in a material library (`.mtl`) written next to the mesh file; urdf: the whole assembly as a robot, with STL meshes written next to it; docs/urdf.md); bodies?: [names] (JSON and URDF export the complete design); urdf only: mesh_prefix? (before each mesh file name, e.g. package://robot/meshes/), effort? (default 1000), velocity? (default 10); quality?: standard|fine|finest (STL, OBJ and 3MF: how closely the mesh follows curved faces, a chord tolerance of 2e-4, 5e-5 or 1e-5 of each body's size; default standard); objects?: body|component (3MF only: one object per body, the default, or one object for each placed component with two or more bodies, with those bodies as its parts, so a slicer loads them as one object with parts)"),
    CommandSpec::new("file.save_mesh", "Save As Mesh", save_mesh)
        .at("INSERT", "FILES")
        .icon("mesh_export")
        .noundo()
        .params("path; format?: stl|3mf|3mf-tagged (default from the extension, else stl; 3mf-tagged: docs/formats/3mf-face-tags.md); bodies?: [names] (default: all); ascii?: bool (STL); slicer?: id or name from file.slicers, the program path it lists, or a program the user added under Custom… (send the saved file there, through the same steps as file.send_to_slicer: a slicer that has to be started first is waited for; may be deprecated later in favour of file.send_to_slicer or a Send to App workflow, kept for scripts); single_instance?: bool (default true; see file.send_to_slicer); quality?: standard|fine|finest (STL, OBJ and 3MF: how closely the mesh follows curved faces, a chord tolerance of 2e-4, 5e-5 or 1e-5 of each body's size; default standard); objects?: body|component (3MF only: one object per body, the default, or one object for each placed component with two or more bodies, with those bodies as its parts, so a slicer loads them as one object with parts)"),
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
    CommandSpec::new("file.slicers", "Installed Slicers", slicers).icon("export")
        .noundo()
        .params("→ the slicers found on this machine: [{id, name, program, icon?}]; pass an id, a name or the program path as file.send_to_slicer's or file.save_mesh's `slicer`"),
];

fn new_doc(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_(p, "name").unwrap_or("Untitled");
    s.replace_design(Session::new(Document::new(name)));
    // A name given here is the user's own, so Save As keeps it (#4907); "Untitled" is none.
    s.user_named = !name.trim().is_empty() && name.trim() != DEFAULT_NAME;
    Ok(json!({"name": name}))
}

fn path_arg<'a>(p: &'a Value, cmd: &str) -> Result<&'a str> {
    str_(p, "path").filter(|x| !x.trim().is_empty() && x.len() < 4096).ok_or_else(|| bad(cmd, "`path` is required"))
}

/// How a STEP or IGES file comes in beyond its path: which way is up in the file, and what its
/// bodies are named after.
struct StepOptions {
    /// `up: "y"`: the file has Y up, so the bodies it brings are turned a quarter turn about X
    /// (Y becomes Z), as the app's "Stand it up" offer does. `z` (the default) keeps the file's axes.
    y_up: bool,
    names: solidcraft_io::StepNames,
}

fn step_options(p: &Value, cmd: &str) -> Result<StepOptions> {
    let y_up = match p.get("up") {
        None | Some(Value::Null) => false,
        Some(v) => match v.as_str().map(|u| u.trim().to_ascii_lowercase()).as_deref() {
            Some("z") => false,
            Some("y") => true,
            _ => return Err(bad(cmd, "`up` must be \"z\" (the file's axes are kept) or \"y\" (the file has Y up: its Y becomes Z)")),
        },
    };
    let names = match p.get("names") {
        None | Some(Value::Null) => solidcraft_io::StepNames::default(),
        Some(v) => v
            .as_str()
            .and_then(solidcraft_io::StepNames::parse)
            .ok_or_else(|| bad(cmd, "`names` must be \"auto\", \"solid\", \"product\" or \"instance\""))?,
    };
    Ok(StepOptions { y_up, names })
}

/// Read a STEP file as an Import feature.
fn read_step(path: &str, names: solidcraft_io::StepNames) -> Result<solidcraft_io::StepFeature> {
    let bytes = solidcraft_io::limits::read(path, solidcraft_io::limits::Kind::Step).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    solidcraft_io::step_import_feature_named(&bytes, path, names).map_err(|e| EngineError::Other(format!("{path}: {e}")))
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
    solidcraft_doc::rigid(solidcraft_geom::Vec3::ZERO, solidcraft_geom::Vec3::ZERO, solidcraft_geom::Vec3::X, std::f64::consts::FRAC_PI_2)
}

/// Add an Import feature for a STEP file to the session's design, in a new component named
/// after the file (#1386).
fn add_step(s: &mut Session, path: &str, name: Option<&str>, options: &StepOptions) -> Result<Value> {
    let sf = read_step(path, options.names)?;
    let fname = name.filter(|n| !n.trim().is_empty()).unwrap_or(&sf.name).to_string();
    let cname = import::component_name(path, name);
    let (id, placed) = import::add_in_component(s, sf.kind, &fname, sf.body_names, &cname, import::Tree::Step(&sf.import))?;
    // Y up: the import's component is turned a quarter turn about X (Y becomes Z), so an
    // assembly's parts and instances turn together.
    let turned = if options.y_up { turn_up(s, &placed)? } else { None };
    let created = created_bodies(s, id);
    let fname = s.doc.feature(id).map(|f| f.name.clone()).unwrap_or_default();
    let mut out = json!({"path": path, "feature": id, "name": fname, "bodies": created, "created_bodies": created, "warnings": sf.warnings, "system": sf.import.system});
    if let Some((up, exporter)) = exporter_up(&sf.import) {
        out["exporter"] = json!(exporter);
        out["exporter_up"] = json!(up);
    }
    placed.report(&mut out);
    if let Some(turned) = turned {
        out["stand_up"] = json!(turned);
    }
    Ok(out)
}

/// The bodies an import feature built, by name.
fn created_bodies(s: &Session, feature: u64) -> Vec<String> {
    s.model.state().bodies.iter().filter(|b| b.feature == feature).map(|b| b.name.clone()).collect()
}

/// Add a MeshImport feature for a 3MF, STL, OBJ or glTF file, in a new component named after the
/// file. A glTF file's nodes become components in it, and its materials the bodies' looks; it is
/// in metres with Y up, so unless `y_up` is false its component is turned to Z up (#1505).
fn add_mesh(s: &mut Session, path: &str, name: Option<&str>, y_up: Option<bool>) -> Result<Value> {
    let err = |e: solidcraft_io::IoError| EngineError::Other(format!("{path}: {e}"));
    let bytes = solidcraft_io::vfs::read(path, solidcraft_io::MAX_3MF_BYTES).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    let gltf = solidcraft_io::is_gltf_path(path);
    let (mf, plan, looks) = if gltf {
        let dir = std::path::Path::new(path).parent().map(std::path::Path::to_path_buf).unwrap_or_default();
        let mut load =
            |name: &str| solidcraft_io::vfs::read(&dir.join(name).to_string_lossy(), solidcraft_io::MAX_GLTF_BYTES).map_err(|e| e.to_string());
        let mut g = solidcraft_io::read_gltf(&bytes, path, &mut load).map_err(err)?;
        if !import::mesh_plan_fits(&g.plan) {
            g.flatten();
        }
        (g.feature, Some(g.plan), g.looks)
    } else {
        (solidcraft_io::mesh_import_feature(&bytes, path).map_err(err)?, None, Vec::new())
    };
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
    let fname = s.doc.feature(id).map(|f| f.name.clone()).unwrap_or_default();
    let mut out = json!({"path": path, "feature": id, "name": fname, "bodies": bodies, "created_bodies": bodies, "warnings": mf.warnings});
    placed.report(&mut out);
    if let Some(turned) = turned {
        out["stand_up"] = json!(turned);
    }
    Ok(out)
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

fn insert_mesh(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "file.insert_mesh";
    let path = path_arg(p, cmd)?;
    if !solidcraft_io::is_mesh_path(path) && !solidcraft_io::is_ply_path(path) {
        return Err(bad(cmd, "only 3MF, STL, OBJ, PLY and glTF files (.3mf, .stl, .obj, .ply, .glb, .gltf) can be inserted as meshes"));
    }
    let up = mesh_up(p, path, cmd)?;
    add_mesh(s, path, str_(p, "name"), up)
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
            o.transform = solidcraft_doc::mat_mul(&m, &o.transform);
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
    if let Some(command) = super::drawing_import_command(path) {
        let stem = std::path::Path::new(path).file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| "Untitled".into());
        let mut fresh = Session::new(Document::new(&stem));
        let mut out = fresh.execute(command, &json!({"path": path}))?;
        fresh.undo.clear();
        s.replace_design(fresh);
        out["path"] = json!(path);
        out["name"] = json!(s.doc.name);
        out["features"] = json!(s.doc.features.len());
        out["errors"] = json!(s.model.results.iter().filter(|r| r.error.is_some()).count());
        return Ok(out);
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
        let r = fresh.execute("pointcloud.insert", &Value::Object(q))?;
        fresh.undo.clear();
        s.replace_design(fresh);
        return Ok(
            json!({"path": path, "features": 0, "errors": 0, "cloud": r["cloud"], "points": r["points"], "source_points": r["source_points"], "bounds": r["bounds"], "warnings": r["warnings"]}),
        );
    }
    if solidcraft_io::is_step_path(path) || solidcraft_io::is_mesh_path(path) {
        // A new, unsaved design holding the file's bodies.
        let stem = std::path::Path::new(path).file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| "Untitled".into());
        let mut fresh = Session::new(Document::new(&stem));
        let r = if solidcraft_io::is_step_path(path) {
            add_step(&mut fresh, path, None, &step_options(p, "doc.open")?)?
        } else {
            add_mesh(&mut fresh, path, None, mesh_up(p, path, "doc.open")?)?
        };
        // The design is the file: its component is the one being worked in.
        if let Some(c) = r["component"].as_u64() {
            fresh.active_component = c;
            fresh.active_occurrence_path = Some(fresh.doc.first_path(c));
        }
        fresh.undo.clear();
        s.replace_design(fresh);
        let errors = s.model.results.iter().filter(|r| r.error.is_some()).count();
        let mut out = json!({"path": path, "features": s.doc.features.len(), "errors": errors, "bodies": r["bodies"], "warnings": r["warnings"]});
        for k in ["component", "component_name", "occurrence", "subcomponents", "rigid_joints", "stand_up", "system", "exporter", "exporter_up"] {
            if let Some(v) = r.get(k) {
                out[k] = v.clone();
            }
        }
        return Ok(out);
    }
    let read_only = bool_(p, "read_only").unwrap_or(false);
    // Stamped before it is read: a write while it is being read is a change too.
    let disk = solidcraft_io::design_lock::stamp(path);
    let (doc, saved) = read_design(s, path)?;
    opened_design(s, Session::opened_at(doc, saved.as_ref(), Some(path)), path, read_only, disk)
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
    let errors = s.model.results.iter().filter(|r| r.error.is_some()).count();
    let mut out = json!({"path": path, "features": s.doc.features.len(), "errors": errors, "name": s.doc.name});
    if read_only {
        out["read_only"] = json!(true);
    }
    let now = solidcraft_io::design_lock::now_secs();
    if let Some(lock) = solidcraft_io::design_lock::held_elsewhere(std::path::Path::new(path), now) {
        out["open_elsewhere"] = json!({"host": lock.host, "user": lock.user, "app": lock.app, "opened": lock.opened});
    }
    Ok(out)
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
pub(crate) fn rename_design(s: &mut Session, from: &str, to: &str) {
    // A prepared world is keyed by the document's Arc. Changing only its name
    // must not make an opted-in save lose warm geometry and display caches.
    // Retag only a cache that already matches: cold or stale placements stay so.
    let model = s.model.state();
    let keep_world = s.world_cache.lock().ok().is_some_and(|cache| cache.as_ref().is_some_and(|cache| cache.matches(s, &model)));
    s.doc_mut().name = to.to_string();
    for snap in s.undo.iter_mut().chain(s.redo.iter_mut()) {
        if snap.doc.name == from {
            std::sync::Arc::make_mut(&mut snap.doc).name = to.to_string();
        }
    }
    if keep_world
        && let Ok(mut cache) = s.world_cache.lock()
        && let Some(cache) = cache.as_mut()
    {
        cache.doc = std::sync::Arc::clone(&s.doc);
    }
}

fn write(s: &mut Session, path: &str, format: &str, compression: &str, preview: bool, cache: bool, overwrite: bool) -> Result<Value> {
    solidcraft_io::vfs::check(path).map_err(|e| EngineError::Other(e.to_string()))?;
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
    // Atomic, keeping the version being replaced as `<file>.bak`.
    solidcraft_io::write_atomic(std::path::Path::new(path), &bytes, true).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    s.path = Some(path.to_string());
    s.disk = solidcraft_io::design_lock::stamp(path);
    s.read_only = false;
    s.cloud.binding = None;
    s.cloud.captures.clear();
    s.mark_saved();
    Ok(json!({"path": path, "bytes": bytes.len(), "name": s.doc.name}))
}

impl Session {
    /// Open a design with its file location, resolving component links before history keeps
    /// them. Absolute in-memory targets survive Save As, undo, redo and recovery unchanged.
    pub fn opened_at(mut doc: Document, saved: Option<&solidcraft_doc::SavedModel>, path: Option<&str>) -> Self {
        super::component_link::resolve_design_links(&mut doc, path);
        Self::opened(doc, saved).located(path)
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
        session.pin_body_names();
        session.saved = session.doc.clone();
        session
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
        self.cloud.binding.clone_from(&done.cloud.binding);
        self.cloud.captures.clear();
        self.saved = done.saved.clone();
        self.saved_visibility = done.saved_visibility.clone();
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
            bytes.saturating_add(mesh.positions.len().saturating_mul(24)).saturating_add(mesh.triangles.len().saturating_mul(12)).saturating_add(24);
        triangles = triangles.saturating_add(mesh.triangles.len());
        if bytes > 8 << 20 || triangles > 100_000 {
            return None;
        }
        preview.meshes.push(solidcraft_doc::MeshData {
            name: body.name.clone(),
            positions: mesh.positions.iter().flat_map(|p| [p.x, p.y, p.z]).collect(),
            triangles: mesh.triangles.iter().flatten().copied().collect(),
            color: s.doc.body_color(body),
        });
    }
    Some(preview)
}

fn save(s: &mut Session, p: &Value) -> Result<Value> {
    if s.cloud_binding().is_some() && str_(p, "path").is_none() {
        return super::cloud::save(s, &json!({}));
    }
    let path = match str_(p, "path") {
        Some(x) if !x.trim().is_empty() => x.to_string(),
        _ => s.path.clone().ok_or_else(|| bad("file.save", "the design has no file yet: give a `path`"))?,
    };
    write(
        s,
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
    let name = std::path::Path::new(path).file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| s.doc.name.clone());
    // A design with components exports as a STEP assembly; otherwise bodies as placed.
    let bytes = if format == Format::Design {
        if !bodies.is_empty() {
            return Err(bad("file.export", "design JSON exports the complete document; omit bodies"));
        }
        solidcraft_io::write_design(&s.design())
    } else if format == Format::Step && bodies.is_empty() && !s.doc.occurrences.is_empty() {
        solidcraft_io::step_assembly(&s.doc, &s.doc.painted(&s.model.state()), &name)?
    } else {
        // Bodies carry the colour they are shown in (appearance, material or imported), and the
        // file the design's properties where the format has a place for them.
        solidcraft_io::export_with_parts(&s.doc.painted(&s.world_state()), bodies, parts, format, &name, &s.doc.metadata, quality)?
    };
    solidcraft_io::vfs::write(path, &bytes).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    Ok(json!({"path": path, "bytes": bytes.len(), "format": format!("{format:?}")}))
}

/// [`export_to`] for a mesh file a slicer is sent (STL or 3MF): each body laid on its print face.
#[cfg(not(target_arch = "wasm32"))]
fn export_laid_to(
    s: &Session,
    path: &str,
    format: Format,
    bodies: &[String],
    parts: &[(String, Vec<String>)],
    quality: solidcraft_io::MeshQuality,
) -> Result<Value> {
    let name = std::path::Path::new(path).file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| s.doc.name.clone());
    let mut state = s.doc.painted(&s.world_state());
    let laid = print_face::lay_on_print_faces(s, &mut state, bodies, parts);
    let bytes = solidcraft_io::export_with_parts(&state, bodies, parts, format, &name, &s.doc.metadata, quality)?;
    solidcraft_io::vfs::write(path, &bytes).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    let mut out = json!({"path": path, "bytes": bytes.len(), "format": format!("{format:?}")});
    laid.report(&mut out);
    Ok(out)
}

fn export(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "file.export";
    let path = path_arg(p, cmd)?;
    let format = Format::from_name(str_(p, "format").unwrap_or(path))?;
    if format == Format::Urdf {
        return export_urdf(s, path, p);
    }
    let quality = mesh_quality(p, cmd)?;
    let parts = mesh_parts(s, p, cmd, format)?;
    export_to(s, path, format, &string_list(cmd, p, "bodies")?, &parts, quality)
}

/// The OBJ export: the mesh file and the material library beside it, with the colours of the
/// bodies. Every path is checked before anything is written; the library goes first and the
/// mesh file last, so a mesh file never names a library that is not there.
fn export_obj(s: &Session, path: &str, bodies: &[String], quality: solidcraft_io::MeshQuality) -> Result<Value> {
    let mtl_path = std::path::Path::new(path).with_extension("mtl");
    let mtl_name = mtl_path.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| String::from("model.mtl"));
    let files = solidcraft_io::export_obj(&s.doc.painted(&s.world_state()), bodies, &mtl_name, quality)?;
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

/// The assembly as a URDF and its meshes next to it (docs/urdf.md). Every path is checked before
/// anything is written; the meshes go first and the `.urdf` last.
fn export_urdf(s: &Session, path: &str, p: &Value) -> Result<Value> {
    let cmd = "file.export";
    if !string_list(cmd, p, "bodies")?.is_empty() {
        return Err(bad(cmd, "urdf exports the whole assembly; omit bodies"));
    }
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
    let doc = if s.pending_moves.is_empty() {
        std::borrow::Cow::Borrowed(&*s.doc)
    } else {
        let placed = solidcraft_doc::joints::solve(&s.doc, &st).world;
        let mut doc = (*s.doc).clone();
        for o in &mut doc.occurrences {
            match s.pending_moves.get(&o.id) {
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
    export_to(s, path, format, &string_list(cmd, p, "bodies")?, &parts, quality)
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
    let list: Vec<Value> = crate::slicers::detect()
        .iter()
        .map(|d| json!({"id": d.id, "name": d.name, "program": d.command.first(), "icon": d.icon.as_ref().map(|i| i.path().to_string_lossy().into_owned())}))
        .collect();
    Ok(json!({ "slicers": list }))
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
    Ok(json!({"dir": dir, "designs": crate::recovery::orphans(&dir)}))
}

fn recover(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "doc.recover";
    let dir = recovery_dir(p, cmd)?;
    let id = str_(p, "id").ok_or_else(|| bad(cmd, "`id` is required (see doc.recovery_list)"))?;
    let (mut doc, entry) = crate::recovery::load(&dir, id)?;
    if let Some(stem) = entry.path.as_deref().and_then(|path| default_name_for(&doc.name, path)) {
        doc.name = stem;
    }
    s.replace_design(Session::opened_at(doc, None, entry.path.as_deref()));
    s.mark_unsaved();
    if bool_(p, "discard").unwrap_or(false) {
        crate::recovery::remove(&dir, id)?;
    }
    let errors = s.model.results.iter().filter(|r| r.error.is_some()).count();
    Ok(json!({"id": id, "path": entry.path, "name": entry.name, "features": s.doc.features.len(), "errors": errors}))
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
    Ok(json!({"discarded": ids}))
}

#[cfg(test)]
#[path = "urdf_tests.rs"]
mod urdf_tests;

#[cfg(test)]
mod tests {
    use super::*;

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
            assert_eq!(s.model.state().sketch(id).unwrap().plane, solidcraft_geom::Plane::XY);
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

    #[test]
    #[ignore = "manual warmed engine save and provisional-open benchmark"]
    fn benchmark_warm_design_save_with_preview() {
        let target = std::env::var_os("CARGO_TARGET_DIR").map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from("target"));
        // The mesh storage fixture intentionally does not evaluate geometry. Use
        // original valid STEP boxes for the engine/display-cache measurement.
        let input = target.join("issue303-step.json");
        let doc = solidcraft_io::read_design(&std::fs::read(input).unwrap()).unwrap();
        let mut session = Session::new(doc);
        for body in &session.world_state().bodies {
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
        let state = s.world_state();
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
        for body in &s.world_state().bodies {
            body.mesh();
        }
        assert!(saved_preview(&s).is_some());
        s.execute("occurrence.move", &json!({"occurrence":occurrence,"translate":[10,0,0],"capture":false})).unwrap();
        for body in &s.world_state().bodies {
            body.mesh();
        }
        assert!(!s.pending_moves.is_empty());
        assert!(saved_preview(&s).is_none(), "uncaptured positions are not in the saved document");
        assert!(saved_geometry(&s, &s.doc).is_none());
        s.execute("component.capture_position", &json!({})).unwrap();
        for body in &s.world_state().bodies {
            body.mesh();
        }
        let preview = saved_preview(&s).unwrap();
        let min_x = preview.meshes.iter().flat_map(|mesh| mesh.positions.as_chunks::<3>().0.iter().map(|p| p[0])).fold(f64::INFINITY, f64::min);
        assert!((min_x - 10.0).abs() < 1e-9);
        let mut editing = s.scratch();
        editing.execute("sketch.create", &json!({"plane":"XY"})).unwrap();
        for body in &editing.world_state().bodies {
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
        let world = session.world_state();
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

    /// #5286: a saved model whose feature records the boolean retries it took (as the kernel
    /// writes them, with no `rejected` list when there were none) is taken on opening, not
    /// refused and evaluated again.
    #[test]
    fn a_saved_model_with_boolean_retries_is_adopted() {
        let mut session = Session::new(Document::new("Retried"));
        session.execute("solid.box", &json!({"length":10,"width":20,"height":30})).unwrap();
        let mut saved = saved_geometry(&session, &session.doc).unwrap();
        // What a coplanar-face retry leaves in the file (a user's design, #5286).
        let retry = json!({"events": [{"face_shift_mm": 0.6477, "rotation_rad": 0.0, "strategy": "coplanar_separation", "tolerance_mm": 0.0, "translation_mm": 0.0}], "truncated": false});
        saved.records["features"][0]["diagnostics"] = retry;
        let bytes =
            solidcraft_io::write_design_container_with_cache(&session.doc, solidcraft_io::DesignCompression::Fast, None, Some(&saved)).unwrap();
        let path = std::env::temp_dir().join(format!("solidcraft-saved-retries-{}.solidcraft", std::process::id()));
        std::fs::write(&path, &bytes).unwrap();
        let mut reopened = Session::default();
        reopened.execute("doc.open", &json!({"path": path})).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(reopened.model.results.iter().all(|r| r.ms == session.model.results[0].ms), "taken from the file, not evaluated");
        let d = &reopened.model.results[0].boolean_diagnostics;
        assert_eq!(d.events.len(), 1, "{d:?}");
        assert!(d.rejected.is_empty());
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
        let world = session.world_state();
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

    #[test]
    #[ignore = "manual native cache save benchmark; generate issue303-step.json first"]
    fn benchmark_native_cache_save() {
        let target = std::env::var_os("CARGO_TARGET_DIR").map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from("target"));
        let input = std::env::var_os("SOLIDCRAFT_BENCH_FILE").map(std::path::PathBuf::from).unwrap_or_else(|| target.join("issue303-step.json"));
        let mut session = Session::new(solidcraft_io::read_design(&std::fs::read(input).unwrap()).unwrap());
        for body in &session.world_state().bodies {
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
        let world = session.world_state();
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
        assert_eq!(opened.world_state().bodies.len(), 3);

        let progress = Arc::new(Progress::default());
        progress.cancel();
        let mut stopped = Session::default();
        watch(progress, || stopped.execute("doc.open", &json!({"path":path}))).unwrap();
        assert!(stopped.model.results.iter().all(|r| r.skipped), "no feature was rebuilt after Cancel");
        assert!(stopped.world_state().bodies.is_empty());
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

        let mut steps = OpenSteps::start(&json!({"path": path, "read_only": true})).unwrap().expect("a design is opened in steps");
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
        let bodies =
            |s: &Session| -> Vec<(String, usize)> { s.world_state().bodies.iter().map(|b| (b.name.clone(), b.mesh().triangles.len())).collect() };
        assert_eq!(bodies(&stepped), bodies(&whole));
        assert_eq!(stepped.world_state().bodies.len(), 3, "the box, the cylinder and the box before the marker");
        assert!(stepped.model.is_current(&stepped.doc));

        // Other files, and STEP-only options, are opened by `doc.open`.
        for p in [json!({"path": step}), json!({"path": "part.stl"}), json!({"path": path, "up": "y"}), json!({})] {
            assert!(OpenSteps::start(&p).unwrap().is_none(), "{p}");
        }
        assert!(OpenSteps::start(&json!({"path": dir.join("missing.sc").to_string_lossy()})).is_err());
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
