//! The format registry: one table describing every file format SolidCraft reads or writes,
//! with the options each one takes. The UI's pickers and panels, the hosts' file-dialog
//! filters and the web `accept` list all derive from this table, so a new format (or a new
//! option) is added in one place. The writers and readers themselves stay where they are
//! ([`crate::Format`] and the `read_*`/`write_*` functions); a row here only *describes* a
//! format and names the engine command and parameters that drive it.

/// Which side of the save/export line a format sits on. Design serializations hold the whole
/// document and reopen losslessly (Save / Save As); artifacts are derived output (Export) or
/// foreign input (Import).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Design,
    Artifact,
}

/// What an imported file becomes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportTarget {
    /// A design file: opens as a tab, or joins a design as a linked component
    /// (`component.insert_design`).
    Design,
    /// B-rep CAD read as an Import base feature (`doc.open` / `file.insert_step`).
    Brep,
    /// A KiCad board read as a design of its own (`doc.open`).
    Kicad,
    /// Mesh read as a MeshImport feature (`doc.open` / `file.insert_mesh`).
    Mesh,
    /// A reference scan, separate from model bodies (`doc.open` / `pointcloud.insert`).
    PointCloud,
    /// A 2D drawing read into the active sketch (`sketch.insert_dxf` / `sketch.insert_svg`).
    Sketch,
}

/// What an export is of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportScope {
    /// Chosen bodies, or every body when none are chosen (the command's `bodies` parameter).
    Bodies,
    /// The whole design or assembly: no bodies parameter.
    Design,
    /// The current camera view: the command also needs a `camera`.
    View,
}

/// How a format imports.
pub struct ImportSpec {
    pub target: ImportTarget,
    /// Options offered at import time; each `param` is a parameter of `doc.open` and of the
    /// matching `file.insert_*` command.
    pub options: &'static [OptionSpec],
    /// Read through cadmpeg (FreeCAD `.FCStd`, Rhino `.3dm`). Every current host builds it;
    /// `native = false` leaves these rows out for a host that cannot.
    pub native_only: bool,
}

/// How a format exports.
pub struct ExportSpec {
    /// The engine command that writes it.
    pub command: &'static str,
    /// The command's `format` value, when the path's extension alone doesn't name it.
    pub format: Option<&'static str>,
    /// Options offered at export time; each `param` is a parameter of `command`.
    pub options: &'static [OptionSpec],
    pub scope: ExportScope,
    /// A lossless serialization of the whole document (reopens as the same design).
    pub lossless: bool,
}

/// One option of a format, mapped onto one parameter of the owning command.
pub struct OptionSpec {
    /// The command parameter this option fills.
    pub param: &'static str,
    /// English label; the UI translates it at paint time.
    pub label: &'static str,
    pub kind: OptionKind,
}

/// What an option's value looks like. `Choice` values are sent as strings; the labels are
/// English and translated by the UI.
pub enum OptionKind {
    Bool {
        default: bool,
    },
    /// (value as the command takes it, English label).
    Choice {
        choices: &'static [(&'static str, &'static str)],
        default: usize,
    },
    Text {
        default: &'static str,
        hint: &'static str,
    },
    Number {
        default: f64,
        min: f64,
        max: f64,
        unit: &'static str,
        /// The file's own value, read without importing it, when the command uses that value
        /// unless it is given another (a KiCad board's thickness): the option starts there, and
        /// at `default` when the file names none.
        from_file: Option<fn(&str) -> Option<f64>>,
    },
    /// An sRGB colour, sent as `"#rrggbb"`.
    Color {
        default: [u8; 3],
    },
}

/// One file format: how it is named, matched and driven.
pub struct FormatSpec {
    /// Stable id of ours (the command's `format` value where one exists).
    pub id: &'static str,
    /// English label; the UI translates it at paint time.
    pub label: &'static str,
    /// Extensions, lower-case, preferred first. Matching is case-insensitive; hosts add the
    /// case variants their file dialogs need.
    pub extensions: &'static [&'static str],
    pub role: Role,
    pub import: Option<ImportSpec>,
    pub export: Option<ExportSpec>,
}

/// Which way is up in a STEP, IGES, FreeCAD or Rhino file (see `doc.open`).
const UP_Z: OptionSpec = OptionSpec {
    param: "up",
    label: "Up axis",
    kind: OptionKind::Choice { choices: &[("z", "Keep the file's axes (Z up)"), ("y", "The file has Y up")], default: 0 },
};

/// glTF has Y up, so standing the import up is the default (see `file.insert_mesh`).
const UP_Y: OptionSpec = OptionSpec {
    param: "up",
    label: "Up axis",
    kind: OptionKind::Choice { choices: &[("y", "The file has Y up"), ("z", "Keep the file's axes (Z up)")], default: 0 },
};

/// What imported bodies are named after (see [`crate::StepNames`]).
const STEP_NAMES: OptionSpec = OptionSpec {
    param: "names",
    label: "Body names",
    kind: OptionKind::Choice {
        choices: &[("solid", "The solid's own name"), ("product", "The part that holds it"), ("instance", "The assembly occurrence")],
        default: 0,
    },
};

const BREP_IMPORT: &[OptionSpec] = &[UP_Z, STEP_NAMES];

/// What a FreeCAD file brings: its feature history rebuilt as features (sketches, Pad, Pocket,
/// Revolution, Groove, Fillet, Chamfer, primitives), or only its solids (see `doc.open`).
const HISTORY: OptionSpec = OptionSpec {
    param: "history",
    label: "FreeCAD history",
    kind: OptionKind::Choice { choices: &[("features", "Rebuild its sketches and features"), ("solids", "Solids only")], default: 0 },
};

const FREECAD_IMPORT: &[OptionSpec] = &[UP_Z, STEP_NAMES, HISTORY];

/// What an OpenUSD file brings: its meshes as mesh bodies (the default, as before the option), or
/// its exact B-rep where it has one (see `file.insert_mesh` and [`crate::UsdGeometry`]).
const USD_GEOMETRY: OptionSpec = OptionSpec {
    param: "geometry",
    label: "Geometry",
    kind: OptionKind::Choice { choices: &[("meshes", "Meshes"), ("exact", "Exact B-rep where the file has it")], default: 0 },
};

/// A KiCad PCB's import options (see `doc.open`); each default is what the command does without
/// the parameter: the copper as an overlay, the substrate in [`crate::DEFAULT_BOARD_COLOR`]
/// (rounded to 8-bit sRGB) and the board's own thickness, read from its file.
const KICAD_IMPORT: &[OptionSpec] = &[
    OptionSpec { param: "components", label: "Include components", kind: OptionKind::Bool { default: true } },
    OptionSpec {
        param: "copper",
        label: "Copper (traces, vias, zones, pads)",
        kind: OptionKind::Choice { choices: &[("none", "None"), ("overlay", "Overlay on the board"), ("mesh", "Modeled")], default: 1 },
    },
    OptionSpec { param: "silkscreen", label: "Include silkscreen", kind: OptionKind::Bool { default: true } },
    OptionSpec { param: "color", label: "Board color", kind: OptionKind::Color { default: [13, 84, 46] } },
    OptionSpec {
        param: "thickness",
        label: "Board thickness",
        kind: OptionKind::Number {
            default: 1.6,
            min: crate::kicad_in::BOARD_THICKNESS_MIN_MM,
            max: crate::kicad_in::BOARD_THICKNESS_MAX_MM,
            unit: "mm",
            from_file: Some(crate::kicad_file_thickness),
        },
    },
    OptionSpec { param: "representation", label: "Board", kind: OptionKind::Choice { choices: &[("brep", "Solid"), ("mesh", "Mesh")], default: 0 } },
];

/// Binary is the `stl` format value, ASCII is `stla` (see `file.export`).
const STL_ENCODING: OptionSpec =
    OptionSpec { param: "format", label: "Encoding", kind: OptionKind::Choice { choices: &[("stl", "Binary"), ("stla", "ASCII")], default: 0 } };

const URDF_OPTIONS: &[OptionSpec] = &[
    OptionSpec { param: "mesh_prefix", label: "Mesh file prefix", kind: OptionKind::Text { default: "", hint: "package://robot/meshes/" } },
    OptionSpec {
        param: "effort",
        label: "Joint effort",
        kind: OptionKind::Number { default: 1000.0, min: 0.0, max: 1e9, unit: "", from_file: None },
    },
    OptionSpec {
        param: "velocity",
        label: "Joint velocity",
        kind: OptionKind::Number { default: 10.0, min: 0.0, max: 1e9, unit: "", from_file: None },
    },
    OptionSpec { param: "images", label: "Pictures as textured visuals", kind: OptionKind::Bool { default: false } },
    OptionSpec { param: "mimic", label: "Couple motion links", kind: OptionKind::Bool { default: false } },
];

const SDF_OPTIONS: &[OptionSpec] = &[
    OptionSpec { param: "static", label: "Static model", kind: OptionKind::Bool { default: true } },
    OptionSpec { param: "model_uris", label: "Name files as model://", kind: OptionKind::Bool { default: false } },
];

const VIEW_DXF_OPTIONS: &[OptionSpec] = &[
    OptionSpec {
        param: "hidden",
        label: "Hidden edges",
        kind: OptionKind::Choice { choices: &[("omit", "Omitted"), ("dashed", "Dashed")], default: 0 },
    },
    OptionSpec {
        param: "tolerance",
        label: "Tolerance",
        kind: OptionKind::Number { default: 0.01, min: 0.0001, max: 1.0, unit: "mm", from_file: None },
    },
];

/// The options of `file.save` and `file.save_as` beyond the path (the design row itself; Save
/// As offers these).
pub static DESIGN_SAVE_OPTIONS: &[OptionSpec] = &[
    OptionSpec {
        param: "format",
        label: "Format",
        kind: OptionKind::Choice { choices: &[("container", "SolidCraft container"), ("json", "Plain JSON design")], default: 0 },
    },
    OptionSpec {
        param: "compression",
        label: "Compression",
        kind: OptionKind::Choice { choices: &[("fast", "Fast"), ("compact", "Compact")], default: 0 },
    },
    OptionSpec { param: "cache", label: "Geometry cache", kind: OptionKind::Bool { default: true } },
    OptionSpec { param: "preview", label: "Display preview", kind: OptionKind::Bool { default: false } },
];

/// Every format, designs first, then artifacts roughly as the Export list shows them. The
/// first row matching an extension wins in [`for_path`], so the design row stays above the
/// design-JSON row and the sketch DXF row above the view DXF row.
pub static FORMATS: &[FormatSpec] = &[
    FormatSpec {
        id: "design",
        label: "SolidCraft design",
        extensions: &[crate::DESIGN_EXTS[0], crate::DESIGN_EXTS[1], "json"],
        role: Role::Design,
        import: Some(ImportSpec { target: ImportTarget::Design, options: &[], native_only: false }),
        export: None,
    },
    FormatSpec {
        id: "step",
        label: "STEP",
        extensions: &["step", "stp"],
        role: Role::Artifact,
        import: Some(ImportSpec { target: ImportTarget::Brep, options: BREP_IMPORT, native_only: false }),
        export: Some(ExportSpec { command: "file.export", format: None, options: &[], scope: ExportScope::Bodies, lossless: false }),
    },
    FormatSpec {
        id: "iges",
        label: "IGES",
        extensions: &["igs", "iges"],
        role: Role::Artifact,
        import: Some(ImportSpec { target: ImportTarget::Brep, options: BREP_IMPORT, native_only: false }),
        export: Some(ExportSpec { command: "file.export", format: None, options: &[], scope: ExportScope::Bodies, lossless: false }),
    },
    FormatSpec {
        id: "fcstd",
        label: "FreeCAD",
        extensions: &["fcstd"],
        role: Role::Artifact,
        import: Some(ImportSpec { target: ImportTarget::Brep, options: FREECAD_IMPORT, native_only: true }),
        export: None,
    },
    FormatSpec {
        id: "3dm",
        label: "Rhino",
        extensions: &["3dm"],
        role: Role::Artifact,
        import: Some(ImportSpec { target: ImportTarget::Brep, options: BREP_IMPORT, native_only: true }),
        // Written by cadmpeg from the design (`file.export`); FreeCAD is not, as cadmpeg cannot
        // write a new FreeCAD file yet.
        export: Some(ExportSpec { command: "file.export", format: None, options: &[], scope: ExportScope::Bodies, lossless: false }),
    },
    FormatSpec {
        id: "kicad",
        label: "KiCad PCB",
        extensions: &["kicad_pcb", "kicad_pro"],
        role: Role::Artifact,
        import: Some(ImportSpec { target: ImportTarget::Kicad, options: KICAD_IMPORT, native_only: false }),
        export: None,
    },
    FormatSpec {
        id: "stl",
        label: "STL",
        extensions: &["stl"],
        role: Role::Artifact,
        import: Some(ImportSpec { target: ImportTarget::Mesh, options: &[], native_only: false }),
        export: Some(ExportSpec { command: "file.export", format: None, options: &[STL_ENCODING], scope: ExportScope::Bodies, lossless: false }),
    },
    FormatSpec {
        id: "3mf",
        label: "3MF",
        extensions: &["3mf"],
        role: Role::Artifact,
        import: Some(ImportSpec { target: ImportTarget::Mesh, options: &[], native_only: false }),
        export: Some(ExportSpec { command: "file.export", format: None, options: &[], scope: ExportScope::Bodies, lossless: false }),
    },
    FormatSpec {
        id: "3mf-tagged",
        label: "3MF with face tags",
        extensions: &["3mf"],
        role: Role::Artifact,
        import: None,
        export: Some(ExportSpec { command: "file.export", format: Some("3mf-tagged"), options: &[], scope: ExportScope::Bodies, lossless: false }),
    },
    FormatSpec {
        id: "obj",
        label: "OBJ",
        extensions: &["obj"],
        role: Role::Artifact,
        import: Some(ImportSpec { target: ImportTarget::Mesh, options: &[], native_only: false }),
        export: Some(ExportSpec { command: "file.export", format: None, options: &[], scope: ExportScope::Bodies, lossless: false }),
    },
    FormatSpec {
        id: "gltf",
        label: "glTF",
        extensions: &["glb", "gltf"],
        role: Role::Artifact,
        import: Some(ImportSpec { target: ImportTarget::Mesh, options: &[UP_Y], native_only: false }),
        export: None,
    },
    // Writing is GLB only (one file); the glTF row above reads both, and stays the row a
    // `.glb` path finds first.
    FormatSpec {
        id: "glb",
        label: "GLB (glTF binary)",
        extensions: &["glb"],
        role: Role::Artifact,
        import: None,
        export: Some(ExportSpec { command: "file.export", format: None, options: &[], scope: ExportScope::Bodies, lossless: false }),
    },
    // Read through cadmpeg: meshes as mesh bodies and the prim tree as components, turned to Z up
    // when the file says Y is up; with `geometry: exact`, its B-reps as B-rep bodies of an Import
    // feature instead. Written through cadmpeg from the STEP the bodies make: exact B-rep and
    // meshes, in metres with Z up; the extension picks the encoding.
    FormatSpec {
        id: "usd",
        label: "OpenUSD",
        extensions: &["usd", "usda", "usdc", "usdz"],
        role: Role::Artifact,
        import: Some(ImportSpec { target: ImportTarget::Mesh, options: &[USD_GEOMETRY], native_only: false }),
        export: Some(ExportSpec { command: "file.export", format: None, options: &[], scope: ExportScope::Bodies, lossless: false }),
    },
    FormatSpec {
        id: "ply",
        label: "PLY point cloud",
        extensions: &["ply"],
        role: Role::Artifact,
        import: Some(ImportSpec { target: ImportTarget::PointCloud, options: &[], native_only: false }),
        export: None,
    },
    // The Point Cloud Library's file: text, binary or compressed.
    FormatSpec {
        id: "pcd",
        label: "PCD point cloud",
        extensions: &["pcd"],
        role: Role::Artifact,
        import: Some(ImportSpec { target: ImportTarget::PointCloud, options: &[], native_only: false }),
        export: None,
    },
    // The open file for laser scans (ASTM E2807), often several scans of a site in one file.
    FormatSpec {
        id: "e57",
        label: "E57 point cloud",
        extensions: &["e57"],
        role: Role::Artifact,
        import: Some(ImportSpec { target: ImportTarget::PointCloud, options: &[], native_only: false }),
        export: None,
    },
    FormatSpec {
        id: "xyz",
        label: "XYZ point cloud",
        extensions: &["xyz"],
        role: Role::Artifact,
        import: Some(ImportSpec { target: ImportTarget::PointCloud, options: &[], native_only: false }),
        export: None,
    },
    FormatSpec {
        id: "pts",
        label: "PTS point cloud",
        extensions: &["pts"],
        role: Role::Artifact,
        import: Some(ImportSpec { target: ImportTarget::PointCloud, options: &[], native_only: false }),
        export: None,
    },
    // The same text as XYZ under another name.
    FormatSpec {
        id: "asc",
        label: "ASC point cloud",
        extensions: &["asc"],
        role: Role::Artifact,
        import: Some(ImportSpec { target: ImportTarget::PointCloud, options: &[], native_only: false }),
        export: None,
    },
    FormatSpec {
        id: "urdf",
        label: "URDF robot",
        extensions: &["urdf"],
        role: Role::Artifact,
        import: None,
        export: Some(ExportSpec { command: "file.export", format: None, options: URDF_OPTIONS, scope: ExportScope::Design, lossless: false }),
    },
    FormatSpec {
        id: "sdf",
        label: "Gazebo model (SDF)",
        extensions: &["sdf"],
        role: Role::Artifact,
        import: None,
        export: Some(ExportSpec { command: "file.export", format: None, options: SDF_OPTIONS, scope: ExportScope::Design, lossless: false }),
    },
    FormatSpec {
        id: "design-json",
        label: "Design as JSON",
        extensions: &["json"],
        role: Role::Design,
        import: None,
        export: Some(ExportSpec { command: "file.export", format: Some("json"), options: &[], scope: ExportScope::Design, lossless: true }),
    },
    FormatSpec {
        id: "dxf-sketch",
        label: "DXF drawing",
        extensions: &["dxf"],
        role: Role::Artifact,
        import: Some(ImportSpec { target: ImportTarget::Sketch, options: &[], native_only: false }),
        export: None,
    },
    FormatSpec {
        id: "view-dxf",
        label: "DXF of the current view",
        extensions: &["dxf"],
        role: Role::Artifact,
        import: None,
        export: Some(ExportSpec {
            command: "file.export_view_dxf",
            format: None,
            options: VIEW_DXF_OPTIONS,
            scope: ExportScope::View,
            lossless: false,
        }),
    },
    FormatSpec {
        id: "svg-sketch",
        label: "SVG drawing",
        extensions: &["svg"],
        role: Role::Artifact,
        import: Some(ImportSpec { target: ImportTarget::Sketch, options: &[], native_only: false }),
        export: None,
    },
];

/// The row with this id.
pub fn find(id: &str) -> Option<&'static FormatSpec> {
    FORMATS.iter().find(|f| f.id == id)
}

fn ext_of(path: &str) -> Option<String> {
    std::path::Path::new(path).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase)
}

/// The first row matching the path's extension, any case.
pub fn for_path(path: &str) -> Option<&'static FormatSpec> {
    let ext = ext_of(path)?;
    FORMATS.iter().find(|f| f.extensions.contains(&ext.as_str()))
}

/// The first importing row matching the path's extension, any case.
pub fn import_for_path(path: &str) -> Option<&'static FormatSpec> {
    let ext = ext_of(path)?;
    FORMATS.iter().find(|f| f.import.is_some() && f.extensions.contains(&ext.as_str()))
}

/// The importing rows, without the cadmpeg ones when `native` is false.
pub fn importable(native: bool) -> impl Iterator<Item = &'static FormatSpec> {
    FORMATS.iter().filter(move |f| f.import.as_ref().is_some_and(|i| native || !i.native_only))
}

/// The exporting rows.
pub fn exportable() -> impl Iterator<Item = &'static FormatSpec> {
    FORMATS.iter().filter(|f| f.export.is_some())
}

/// Every extension `doc.open` reads (designs, CAD files, boards, meshes, scans and sketch
/// drawings, which it opens as a sketch), without duplicates, in table order.
pub fn open_extensions(native: bool) -> Vec<&'static str> {
    extensions_where(native, |_| true)
}

/// The extensions Start › Import offers: foreign CAD files, boards, meshes and scans. Designs open
/// with Open, and sketch drawings go into a sketch with Insert. Without duplicates, in table order.
pub fn import_extensions(native: bool) -> Vec<&'static str> {
    extensions_where(native, |target| matches!(target, ImportTarget::Brep | ImportTarget::Kicad | ImportTarget::Mesh | ImportTarget::PointCloud))
}

/// The extensions of the importing rows whose target `keep` takes, each once, in table order.
fn extensions_where(native: bool, keep: impl Fn(ImportTarget) -> bool) -> Vec<&'static str> {
    let mut exts = Vec::new();
    for f in importable(native).filter(|f| f.import.as_ref().is_some_and(|i| keep(i.target))) {
        for e in f.extensions {
            if !exts.contains(e) {
                exts.push(*e);
            }
        }
    }
    exts
}

/// The Open dialog's filter groups beyond the leading "everything importable" one:
/// (English label, lower-case extensions). Hosts add the case variants their dialogs need.
pub fn import_filters(native: bool) -> Vec<(&'static str, Vec<&'static str>)> {
    let mut groups: Vec<(&'static str, Vec<&'static str>)> = Vec::new();
    let mut add = |label: &'static str, exts: &[&'static str]| {
        if let Some((_, g)) = groups.iter_mut().find(|(l, _)| *l == label) {
            for e in exts {
                if !g.contains(e) {
                    g.push(e);
                }
            }
        } else {
            groups.push((label, exts.to_vec()));
        }
    };
    for f in importable(native) {
        let Some(import) = f.import.as_ref() else { continue };
        match import.target {
            ImportTarget::Design => add("SolidCraft design (container or JSON)", f.extensions),
            ImportTarget::Brep if import.native_only => add("FreeCAD and Rhino", f.extensions),
            ImportTarget::Brep => add(f.label, f.extensions),
            ImportTarget::Kicad => add(f.label, f.extensions),
            // An OpenUSD scene is read as meshes, but it is its own kind of file to most people.
            ImportTarget::Mesh if f.id == "usd" => add(f.label, f.extensions),
            ImportTarget::Mesh => add("Mesh (3MF, STL, OBJ, glTF)", f.extensions),
            ImportTarget::PointCloud => add("Point cloud (PLY, PCD, E57, XYZ, PTS, ASC)", f.extensions),
            ImportTarget::Sketch => add("Sketch drawings (DXF, SVG)", f.extensions),
        }
    }
    groups
}

/// The Open dialog's groups for installed importer plugins: (plugin name, lower-case extensions).
/// The app sets them before it shows the dialog; hosts add them after the built-in groups.
static PLUGIN_OPEN_FILTERS: std::sync::RwLock<Vec<(String, Vec<String>)>> = std::sync::RwLock::new(Vec::new());

/// Set the importer plugins' Open dialog groups (see [`plugin_open_filters`]).
pub fn set_plugin_open_filters(filters: Vec<(String, Vec<String>)>) {
    if let Ok(mut current) = PLUGIN_OPEN_FILTERS.write() {
        *current = filters;
    }
}

/// The importer plugins' Open dialog groups the app last set.
pub fn plugin_open_filters() -> Vec<(String, Vec<String>)> {
    PLUGIN_OPEN_FILTERS.read().map(|filters| filters.clone()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_extensions_lower_case() {
        for (i, f) in FORMATS.iter().enumerate() {
            assert!(FORMATS.iter().skip(i + 1).all(|g| g.id != f.id), "duplicate id {}", f.id);
            assert!(!f.extensions.is_empty(), "{} has no extensions", f.id);
            for e in f.extensions {
                assert_eq!(*e, e.to_ascii_lowercase(), "{} extension {e} is not lower-case", f.id);
            }
            assert!(f.import.is_some() || f.export.is_some(), "{} neither imports nor exports", f.id);
        }
    }

    /// The registry and the path predicates name the same extensions, so they can never
    /// drift apart.
    #[test]
    fn registry_extensions_match_the_path_predicates() {
        let with_target = |t: ImportTarget| {
            let mut exts: Vec<&str> =
                FORMATS.iter().filter(|f| f.import.as_ref().is_some_and(|i| i.target == t)).flat_map(|f| f.extensions.iter().copied()).collect();
            exts.sort_unstable();
            exts.dedup();
            exts
        };
        for e in with_target(ImportTarget::Brep) {
            assert!(crate::is_step_path(&format!("part.{e}")), "is_step_path misses .{e}");
        }
        for e in ["step", "stp", "igs", "iges", "fcstd", "3dm"] {
            assert!(with_target(ImportTarget::Brep).contains(&e), "registry misses B-rep .{e}");
        }
        for e in with_target(ImportTarget::Kicad) {
            assert!(crate::is_kicad_path(&format!("board.{e}")), "is_kicad_path misses .{e}");
        }
        for e in with_target(ImportTarget::Mesh) {
            assert!(crate::is_mesh_path(&format!("part.{e}")), "is_mesh_path misses .{e}");
        }
        for e in ["3mf", "stl", "obj", "glb", "gltf"] {
            assert!(with_target(ImportTarget::Mesh).contains(&e), "registry misses mesh .{e}");
        }
        for e in ["ply", "pcd", "e57", "xyz", "pts", "asc"] {
            assert!(with_target(ImportTarget::PointCloud).contains(&e), "registry misses point cloud .{e}");
            assert!(crate::is_point_cloud_path(&format!("scan.{e}")));
            assert!(open_extensions(true).contains(&e));
            assert!(open_extensions(false).contains(&e));
            assert_eq!(
                import_for_path(&format!("scan.{}", e.to_uppercase())).and_then(|f| f.import.as_ref()).map(|i| i.target),
                Some(ImportTarget::PointCloud)
            );
        }
        for e in with_target(ImportTarget::Design) {
            assert!(crate::is_design_path(&format!("part.{e}")), "is_design_path misses .{e}");
        }
        for e in crate::DESIGN_EXTS.iter().chain(["json"].iter()) {
            assert!(with_target(ImportTarget::Design).contains(e), "registry misses design .{e}");
        }
    }

    /// Every export row names a format `file.export` resolves: an explicit `format` value, a
    /// first extension [`crate::Format::from_name`] accepts, or its own command.
    #[test]
    fn export_rows_resolve_to_writers() {
        for f in exportable() {
            let Some(export) = f.export.as_ref() else { continue };
            if export.command != "file.export" {
                continue;
            }
            let name = export.format.unwrap_or(f.extensions[0]);
            assert!(crate::Format::from_name(name).is_ok(), "{}: file.export does not resolve {name}", f.id);
        }
    }

    #[test]
    fn paths_resolve_to_the_canonical_rows() {
        assert_eq!(for_path("a.SolidCraft").map(|f| f.id), Some("design"));
        assert_eq!(for_path("a.json").map(|f| f.id), Some("design"));
        assert_eq!(for_path("a.STEP").map(|f| f.id), Some("step"));
        assert_eq!(for_path("a.3mf").map(|f| f.id), Some("3mf"));
        assert_eq!(for_path("a.dxf").map(|f| f.id), Some("dxf-sketch"));
        assert_eq!(import_for_path("a.glb").map(|f| f.id), Some("gltf"));
        assert_eq!(for_path("a.glb").map(|f| f.id), Some("gltf"));
        let writes = |ext: &str| exportable().filter(|f| f.extensions.contains(&ext)).map(|f| f.id).collect::<Vec<_>>();
        assert_eq!(writes("glb"), vec!["glb"]);
        for e in ["usd", "usda", "usdc", "usdz"] {
            assert_eq!(import_for_path(&format!("a.{e}")).map(|f| f.id), Some("usd"));
            assert_eq!(writes(e), vec!["usd"]);
        }
        assert!(writes("gltf").is_empty(), "a .gltf file is never written");
        assert_eq!(writes("3dm"), vec!["3dm"]);
        assert_eq!(for_path("a.3DM").map(|f| f.id), Some("3dm"));
        assert!(writes("fcstd").is_empty(), "cadmpeg cannot write a new FreeCAD file yet");
        assert!(for_path("a.png").is_none());
        assert!(for_path("nodot").is_none());
    }

    /// The KiCad row starts where the import does without the parameter: the copper as an overlay,
    /// the default substrate colour and the board's own thickness from its file.
    #[test]
    fn kicad_defaults_are_the_import_defaults() {
        let options = find("kicad").and_then(|f| f.import.as_ref()).map(|i| i.options).expect("KiCad imports");
        let kind = |param: &str| &options.iter().find(|o| o.param == param).unwrap_or_else(|| panic!("{param} offered")).kind;
        let defaults = crate::KicadOptions::default();
        let OptionKind::Choice { choices, default } = kind("copper") else { panic!("copper is a choice") };
        assert_eq!(crate::KicadCopper::parse(choices[*default].0), Some(defaults.copper));
        let OptionKind::Color { default } = kind("color") else { panic!("color is a colour") };
        let expected = crate::DEFAULT_BOARD_COLOR.map(|c| (c * 255.0).round() as u8);
        assert_eq!(*default, expected);
        let OptionKind::Number { from_file: Some(read), .. } = kind("thickness") else { panic!("thickness comes from the file") };
        let dir = std::env::temp_dir().join(format!("solidcraft-formats-kicad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let board = dir.join("b.kicad_pcb");
        std::fs::write(&board, "(kicad_pcb (version 20240108) (general (thickness 2.2)))").unwrap();
        assert_eq!(read(&board.to_string_lossy()), Some(2.2));
        assert_eq!(read(&dir.join("b.kicad_pro").to_string_lossy()), Some(2.2), "a project reads its board");
        assert_eq!(read(&dir.join("missing.kicad_pcb").to_string_lossy()), None);
        // A board outside the row's range is not a board thickness, so the file names none: the
        // row starts at its default, 1.6 mm, which is the thickness the import uses for it too.
        let thin = dir.join("thin.kicad_pcb");
        std::fs::write(&thin, "(kicad_pcb (version 20240108) (general (thickness 0.005)))").unwrap();
        assert_eq!(read(&thin.to_string_lossy()), None, "a board thinner than the row offers");
        assert_eq!(crate::kicad_in::kicad_board_thickness(&std::fs::read(&thin).unwrap()), None);
        let thick = dir.join("thick.kicad_pcb");
        std::fs::write(&thick, "(kicad_pcb (version 20240108) (general (thickness 2000)))").unwrap();
        assert_eq!(read(&thick.to_string_lossy()), None, "a board thicker than a board");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// cadmpeg is pure Rust now, so the browser build offers its rows too; `native = false`
    /// stays for a host that cannot build it.
    #[test]
    fn a_host_without_cadmpeg_leaves_its_rows_out() {
        assert!(importable(true).any(|f| f.id == "fcstd"));
        assert!(open_extensions(true).contains(&"fcstd"));
        assert!(open_extensions(true).contains(&"3dm"));
        assert!(importable(false).all(|f| f.id != "fcstd" && f.id != "3dm"));
        assert!(!open_extensions(false).contains(&"fcstd"));
        assert!(open_extensions(false).contains(&"igs"));
    }

    /// The Open dialog's groups, as the hosts show them.
    #[test]
    fn import_filter_groups_hold_the_expected_extensions() {
        let native = import_filters(true);
        let find = |label: &str| native.iter().find(|(l, _)| *l == label).map(|(_, e)| e.clone()).unwrap_or_default();
        assert_eq!(find("SolidCraft design (container or JSON)"), vec!["sc", "solidcraft", "json"]);
        assert_eq!(find("STEP"), vec!["step", "stp"]);
        assert_eq!(find("IGES"), vec!["igs", "iges"]);
        assert_eq!(find("FreeCAD and Rhino"), vec!["fcstd", "3dm"]);
        assert_eq!(find("Mesh (3MF, STL, OBJ, glTF)"), vec!["stl", "3mf", "obj", "glb", "gltf"]);
        assert_eq!(find("OpenUSD"), vec!["usd", "usda", "usdc", "usdz"]);
        assert_eq!(find("Point cloud (PLY, PCD, E57, XYZ, PTS, ASC)"), vec!["ply", "pcd", "e57", "xyz", "pts", "asc"]);
        assert_eq!(find("Sketch drawings (DXF, SVG)"), vec!["dxf", "svg"]);
        assert!(import_filters(false).iter().all(|(l, _)| *l != "FreeCAD and Rhino"));
    }

    /// `doc.open` opens a DXF or SVG drawing as a sketch (#4635), so the Open lists offer them
    /// with no host adding them by hand (#7328).
    #[test]
    fn open_extensions_include_the_sketch_drawings() {
        let open = open_extensions(true);
        for e in ["dxf", "svg"] {
            assert_eq!(open.iter().filter(|x| **x == e).count(), 1, "{e} once in {open:?}");
        }
    }

    /// Start › Import offers every foreign CAD, board, mesh and scan file SolidCraft reads, and no
    /// design or sketch drawing (#7328).
    #[test]
    fn import_extensions_are_the_foreign_cad_mesh_and_scan_files() {
        let import = import_extensions(true);
        for e in ["step", "igs", "fcstd", "3dm", "kicad_pcb", "stl", "usdz", "ply"] {
            assert!(import.contains(&e), "{e} missing from {import:?}");
        }
        for e in ["sc", "solidcraft", "json", "dxf", "svg"] {
            assert!(!import.contains(&e), "{e} is not imported: {import:?}");
        }
        let foreign = [ImportTarget::Brep, ImportTarget::Kicad, ImportTarget::Mesh, ImportTarget::PointCloud];
        for f in importable(true).filter(|f| f.import.as_ref().is_some_and(|i| foreign.contains(&i.target))) {
            assert!(f.extensions.iter().all(|e| import.contains(e)), "{} missing", f.id);
        }
        let once: std::collections::BTreeSet<_> = import.iter().collect();
        assert_eq!(once.len(), import.len(), "no duplicates in {import:?}");
    }
}
