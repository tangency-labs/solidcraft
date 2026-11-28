//! Split an imported STEP assembly into linked `.sc` files on disk: each leaf product once as a
//! part file, each assembly node as a design that `component.insert_design`s its children.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use solidcraft_doc::{Document, IDENTITY, Mat};
use solidcraft_kernel::{ExportBody, StepHeader, step_export_bodies};

use super::StepOptions;
use super::import;
use crate::params::bad;
use crate::{EngineError, Result, Session};

const CMD: &str = "file.split_step";

/// A STEP file the split option applies to (`.step` / `.stp`), not IGES or a native CAD path.
pub(super) fn is_step_file(path: &str) -> bool {
    Path::new(path).extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("step") || e.eq_ignore_ascii_case("stp"))
}

/// What [`write`] put on disk.
pub(super) struct Written {
    /// The top assembly or the single part file.
    pub root: PathBuf,
    /// Every `.sc` written, root included, in write order.
    pub files: Vec<PathBuf>,
    pub warnings: Vec<String>,
}

/// Import `path` into a temporary session, then write leaf parts and nested assemblies under
/// `folder`. `overwrite` replaces an existing file; without it an existing path is refused.
pub(super) fn write(path: &str, options: &StepOptions, folder: &Path, overwrite: bool, validation: solidcraft_kernel::Validation) -> Result<Written> {
    if cfg!(target_arch = "wasm32") {
        return Err(bad(CMD, "saving split files needs a disk; it is not available in the web app"));
    }
    if !is_step_file(path) {
        return Err(bad(CMD, "`split_to_files` is for a STEP file (`.step` / `.stp`)"));
    }
    if options.flatten {
        return Err(bad(CMD, "`flatten` and `split_to_files` cannot be used together: flattening removes the assembly tree to split"));
    }
    let folder = super::super::design_link::absolute(folder);
    std::fs::create_dir_all(&folder).map_err(|e| EngineError::Other(format!("{}: {e}", folder.display())))?;

    let mut temp = Session::new(Document::new("split"));
    temp.set_validation(validation);
    // Import without standing up: relative child transforms stay as in the file; `y_up` is baked
    // into the top assembly's child placements when that file is written.
    let mut import_options = options.clone();
    import_options.y_up = false;
    let imported = super::add_step(&mut temp, path, None, &import_options, false)?;
    let top = imported["component"].as_u64().ok_or_else(|| EngineError::Other(format!("{path}: the import has no component")))?;

    let mut warnings: Vec<String> =
        imported["warnings"].as_array().map(|w| w.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();

    let children_of = |doc: &Document, parent: u64| -> Vec<u64> { doc.components.iter().filter(|c| c.parent == parent).map(|c| c.id).collect() };
    let is_leaf = |doc: &Document, id: u64| children_of(doc, id).is_empty();

    // Unique components under `top`, leaves first (post-order).
    let mut order = Vec::new();
    let mut seen = HashSet::new();
    fn walk(doc: &Document, id: u64, children_of: &dyn Fn(&Document, u64) -> Vec<u64>, seen: &mut HashSet<u64>, order: &mut Vec<u64>) {
        if !seen.insert(id) {
            return;
        }
        for child in children_of(doc, id) {
            walk(doc, child, children_of, seen, order);
        }
        order.push(id);
    }
    walk(&temp.doc, top, &children_of, &mut seen, &mut order);

    if order.len() <= 1 && is_leaf(&temp.doc, top) {
        // One product (possibly multi-body): a single part file.
        let stem = file_stem(path, options);
        let out = unique_path(&folder, &stem, overwrite, &HashSet::new())?;
        save_leaf(&temp, top, &out)?;
        if imported.get("subcomponents").and_then(Value::as_u64).unwrap_or(0) == 0 {
            warnings.push("the STEP file is not an assembly; one part file was written".into());
        }
        return Ok(Written { root: out.clone(), files: vec![out], warnings });
    }

    let mut paths: HashMap<u64, PathBuf> = HashMap::new();
    let mut used_names: HashSet<String> = HashSet::new();
    let mut files = Vec::new();

    for id in order {
        let name = temp.doc.components.iter().find(|c| c.id == id).map(|c| c.name.as_str()).unwrap_or("Part");
        let stem = sanitize_stem(name);
        let out = unique_path(&folder, &stem, overwrite, &used_names)?;
        used_names.insert(out.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
        if is_leaf(&temp.doc, id) {
            save_leaf(&temp, id, &out)?;
        } else {
            let stand_up = options.y_up && id == top;
            save_assembly(&temp, id, &paths, &out, stand_up)?;
        }
        paths.insert(id, out.clone());
        files.push(out);
    }

    let root = paths.get(&top).cloned().ok_or_else(|| EngineError::Other("split: missing root file".into()))?;
    Ok(Written { root, files, warnings })
}

/// Open/insert parameter helpers: folder from `split_folder` or the STEP file's directory.
pub(super) fn split_folder_of(path: &str, options: &StepOptions) -> Result<PathBuf> {
    if let Some(folder) = options.split_folder.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(PathBuf::from(folder));
    }
    let parent = Path::new(path).parent().filter(|p| !p.as_os_str().is_empty()).map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    Ok(parent)
}

fn file_stem(path: &str, options: &StepOptions) -> String {
    if let Some(n) = options.split_name.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        return sanitize_stem(n);
    }
    Path::new(path).file_stem().map(|s| sanitize_stem(&s.to_string_lossy())).filter(|s| !s.is_empty()).unwrap_or_else(|| "Part".into())
}

fn sanitize_stem(name: &str) -> String {
    let mut out = String::new();
    for ch in name.trim().chars() {
        match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => out.push('_'),
            c if c.is_control() => {}
            c => out.push(c),
        }
        if out.len() >= 120 {
            break;
        }
    }
    let out = out.trim().trim_end_matches('.');
    if out.is_empty() { "Part".into() } else { out.to_string() }
}

fn unique_path(folder: &Path, stem: &str, overwrite: bool, reserved: &HashSet<String>) -> Result<PathBuf> {
    let mut n = 0u32;
    loop {
        let name = if n == 0 { format!("{stem}.sc") } else { format!("{stem}_{n}.sc") };
        n = n.saturating_add(1);
        if reserved.contains(&name) {
            continue;
        }
        let path = folder.join(&name);
        if path.exists() && !overwrite {
            return Err(bad(CMD, format!("{} already exists; pass overwrite: true to replace it, or choose another folder", path.display())));
        }
        return Ok(path);
    }
}

/// Export the component's solids in its own frame and save them as a one-component design.
fn save_leaf(source: &Session, component: u64, out: &Path) -> Result<()> {
    let name = source.doc.components.iter().find(|c| c.id == component).map(|c| c.name.clone()).unwrap_or_else(|| "Part".into());
    // Prefer the ownership map; a single-part import may leave it empty and rely on the feature's component.
    let mut body_names: Vec<String> = source.doc.body_components.iter().filter(|(_, c)| **c == component).map(|(n, _)| n.clone()).collect();
    if body_names.is_empty() {
        body_names = source.model.state().bodies.iter().filter(|b| source.doc.model_body_component(b) == component).map(|b| b.name.clone()).collect();
    }
    if body_names.is_empty() {
        // An empty leaf (product with no solids): still write a named empty design so assemblies can link it.
        let mut part = Session::new(Document::new(&name));
        part.execute("file.save_as", &json!({"path": out.to_string_lossy(), "overwrite": true}))?;
        return Ok(());
    }
    let state = source.model.state();
    let mut locals = Vec::new();
    for bname in &body_names {
        let body = state.body(bname).ok_or_else(|| EngineError::Other(format!("split: missing body {bname}")))?;
        let offset = source.doc.body_offsets.get(bname).copied().unwrap_or(IDENTITY);
        // Bake the import offset into the solid so the part file's root holds it at identity.
        let local = if solidcraft_doc::is_identity(&offset) {
            body.body.clone()
        } else {
            solidcraft_kernel::place_matrix(&body.body, offset).map_err(|e| EngineError::Other(format!("split: {e}")))?
        };
        locals.push((bname.clone(), local));
    }
    let export: Vec<ExportBody<'_>> = locals.iter().map(|(n, b)| ExportBody { name: n.clone(), body: b, color: None }).collect();
    let header = StepHeader { file_name: format!("{name}.step"), ..Default::default() };
    let step = step_export_bodies(&export, &header).map_err(|e| EngineError::Other(format!("split: {e}")))?;
    let mut part = Session::new(Document::new(&name));
    let sf = solidcraft_io::step_import_feature(step.as_bytes(), &format!("{name}.step")).map_err(|e| EngineError::Other(format!("split: {e}")))?;
    let layers: Vec<Option<String>> = sf.import.bodies.iter().map(|b| b.layer.clone()).collect();
    let _ = import::add_in_component(&mut part, sf.kind, &name, sf.body_names, &sf.invisible, &layers, &name, import::Tree::Step(&sf.import))?;
    part.execute("file.save_as", &json!({"path": out.to_string_lossy(), "overwrite": true}))?;
    Ok(())
}

/// Write an assembly design whose children are linked part/assembly files at the STEP placements.
fn save_assembly(source: &Session, component: u64, child_files: &HashMap<u64, PathBuf>, out: &Path, stand_up: bool) -> Result<()> {
    let name = source.doc.components.iter().find(|c| c.id == component).map(|c| c.name.clone()).unwrap_or_else(|| "Assembly".into());
    let mut asm = Session::new(Document::new(&name));
    // So insert_design stores paths relative to this file's folder.
    asm.path = Some(out.to_string_lossy().into_owned());

    let turn = if stand_up { Some(super::quarter_turn_x()) } else { None };
    // Direct child placements: occurrences whose parent component is `component`.
    let mut placements: Vec<(u64, Mat, Option<String>)> = source
        .doc
        .occurrences
        .iter()
        .filter(|o| o.parent == component)
        .map(|o| {
            let mut t = o.transform;
            if let Some(turn) = turn {
                t = solidcraft_doc::mat_mul(&turn, &t);
            }
            (o.component, t, Some(o.name.clone()))
        })
        .collect();
    // Stable order: by child component id, then transform bits.
    placements.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| format!("{:?}", a.1).cmp(&format!("{:?}", b.1))));

    for (child, transform, occ_name) in placements {
        let file = child_files.get(&child).ok_or_else(|| EngineError::Other(format!("split: no file for component {child}")))?;
        // Absolute path: `insert_design` resolves relative paths from the process cwd. Saving the
        // assembly rewrites links relative to this file's folder. `matrix` is the 3×4 form
        // insert_design takes (columns of the document Mat), not the Mat JSON shape.
        let mut params = json!({
            "path": file.to_string_lossy(),
            "reuse": true,
            "matrix": matrix_param(&transform),
        });
        if let Some(n) = occ_name.filter(|n| !n.is_empty()) {
            params["name"] = json!(n);
        }
        asm.execute("component.insert_design", &params)?;
    }
    asm.execute("file.save_as", &json!({"path": out.to_string_lossy(), "overwrite": true}))?;
    Ok(())
}

/// Parse split-related keys shared by `doc.open` and `file.insert_step`.
pub(super) fn read_split_flags(p: &Value, cmd: &str, options: &mut StepOptions) -> Result<bool> {
    options.split_to_files = match p.get("split_to_files") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(v)) => *v,
        Some(_) => return Err(bad(cmd, "`split_to_files` must be a boolean")),
    };
    options.split_folder = match p.get("split_folder") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => return Err(bad(cmd, "`split_folder` must be a folder path")),
    };
    if let Some(Value::String(n)) = p.get("name") {
        options.split_name = Some(n.clone());
    }
    if options.split_to_files && options.flatten {
        return Err(bad(cmd, "`flatten` and `split_to_files` cannot be used together: flattening removes the assembly tree to split"));
    }
    Ok(options.split_to_files)
}

pub(super) fn read_overwrite(p: &Value, cmd: &str) -> Result<bool> {
    match p.get("overwrite") {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(v)) => Ok(*v),
        Some(_) => Err(bad(cmd, "`overwrite` must be a boolean")),
    }
}

/// `component.insert_design`'s `matrix`: row-major 3×4 whose rows are the document Mat's columns
/// (translation from the last row).
fn matrix_param(m: &Mat) -> Value {
    json!([m[0][0], m[1][0], m[2][0], m[3][0], m[0][1], m[1][1], m[2][1], m[3][1], m[0][2], m[1][2], m[2][2], m[3][2],])
}

/// Enrich a command reply with the files written by a split.
pub(super) fn annotate(reply: &mut Value, written: &Written) {
    reply["split_root"] = json!(written.root.to_string_lossy());
    reply["split_files"] = json!(written.files.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>());
    if !written.warnings.is_empty() {
        match reply.get_mut("warnings").and_then(Value::as_array_mut) {
            Some(list) => list.extend(written.warnings.iter().cloned().map(Value::from)),
            None => reply["warnings"] = json!(written.warnings),
        }
    }
}

/// Insert the split root into `s` via `component.insert_design`.
pub(super) fn insert_root(s: &mut Session, written: &Written, name: Option<&str>, matrix: Option<Mat>) -> Result<Value> {
    let mut params = json!({
        "path": written.root.to_string_lossy(),
        "reuse": false,
    });
    if let Some(n) = name.map(str::trim).filter(|n| !n.is_empty()) {
        params["name"] = json!(n);
    }
    if let Some(m) = matrix {
        params["matrix"] = matrix_param(&m);
    }
    let mut out = s.execute("component.insert_design", &params)?;
    // `file.insert_step` callers and the envelope helper look for `component_name`.
    if out.get("component_name").is_none()
        && let Some(name) = out.get("name").cloned()
    {
        out["component_name"] = name;
    }
    annotate(&mut out, written);
    Ok(out)
}
