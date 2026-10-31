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

/// Binary is the `stl` format value, ASCII is `stla` (see `file.export`).
const STL_ENCODING: OptionSpec =
    OptionSpec { param: "format", label: "Encoding", kind: OptionKind::Choice { choices: &[("stl", "Binary"), ("stla", "ASCII")], default: 0 } };

const URDF_OPTIONS: &[OptionSpec] = &[
    OptionSpec { param: "mesh_prefix", label: "Mesh file prefix", kind: OptionKind::Text { default: "", hint: "package://robot/meshes/" } },
    OptionSpec { param: "effort", label: "Joint effort", kind: OptionKind::Number { default: 1000.0, min: 0.0, max: 1e9, unit: "" } },
    OptionSpec { param: "velocity", label: "Joint velocity", kind: OptionKind::Number { default: 10.0, min: 0.0, max: 1e9, unit: "" } },
];

const VIEW_DXF_OPTIONS: &[OptionSpec] = &[
    OptionSpec {
        param: "hidden",
        label: "Hidden edges",
        kind: OptionKind::Choice { choices: &[("omit", "Omitted"), ("dashed", "Dashed")], default: 0 },
    },
    OptionSpec { param: "tolerance", label: "Tolerance", kind: OptionKind::Number { default: 0.01, min: 0.0001, max: 1.0, unit: "mm" } },
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
        extensions: &[crate::DESIGN_EXTS[0], crate::DESIGN_EXTS[1], crate::DESIGN_EXTS[2], "json"],
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
        import: Some(ImportSpec { target: ImportTarget::Brep, options: BREP_IMPORT, native_only: true }),
        export: None,
    },
    FormatSpec {
        id: "3dm",
        label: "Rhino",
        extensions: &["3dm"],
        role: Role::Artifact,
        import: Some(ImportSpec { target: ImportTarget::Brep, options: BREP_IMPORT, native_only: true }),
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
        id: "osd",
        label: "OSD",
        extensions: &["osd"],
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
    FormatSpec {
        id: "ply",
        label: "PLY point cloud",
        extensions: &["ply"],
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
    FormatSpec {
        id: "urdf",
        label: "URDF robot",
        extensions: &["urdf"],
        role: Role::Artifact,
        import: None,
        export: Some(ExportSpec { command: "file.export", format: None, options: URDF_OPTIONS, scope: ExportScope::Design, lossless: false }),
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

/// Every extension `doc.open` reads (designs, CAD files and meshes; not sketch drawings),
/// without duplicates, in table order.
pub fn open_extensions(native: bool) -> Vec<&'static str> {
    let mut exts = Vec::new();
    for f in importable(native) {
        let importing = f.import.as_ref().map(|i| i.target);
        if importing == Some(ImportTarget::Sketch) {
            continue;
        }
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
            ImportTarget::Mesh => add("Mesh (3MF, STL, OBJ, OSD, glTF)", f.extensions),
            ImportTarget::PointCloud => add("Point cloud (PLY, XYZ, PTS)", f.extensions),
            ImportTarget::Sketch => add("Sketch drawings (DXF, SVG)", f.extensions),
        }
    }
    groups
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
        for e in with_target(ImportTarget::Mesh) {
            assert!(crate::is_mesh_path(&format!("part.{e}")), "is_mesh_path misses .{e}");
        }
        for e in ["3mf", "stl", "obj", "osd", "glb", "gltf"] {
            assert!(with_target(ImportTarget::Mesh).contains(&e), "registry misses mesh .{e}");
        }
        for e in ["ply", "xyz", "pts"] {
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
        assert!(for_path("a.png").is_none());
        assert!(for_path("nodot").is_none());
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
        assert_eq!(find("SolidCraft design (container or JSON)"), vec!["sc", "solidcraft", "solvecraft", "json"]);
        assert_eq!(find("STEP"), vec!["step", "stp"]);
        assert_eq!(find("IGES"), vec!["igs", "iges"]);
        assert_eq!(find("FreeCAD and Rhino"), vec!["fcstd", "3dm"]);
        assert_eq!(find("Mesh (3MF, STL, OBJ, OSD, glTF)"), vec!["stl", "3mf", "obj", "osd", "glb", "gltf"]);
        assert_eq!(find("Point cloud (PLY, XYZ, PTS)"), vec!["ply", "xyz", "pts"]);
        assert_eq!(find("Sketch drawings (DXF, SVG)"), vec!["dxf", "svg"]);
        assert!(import_filters(false).iter().all(|(l, _)| *l != "FreeCAD and Rhino"));
    }
}
