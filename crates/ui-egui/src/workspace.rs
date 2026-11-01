//! Ribbon structure is UI data; command registrations retain their original placements.
//! Explicit ids override the old placement table and can also share a command across sections.

use solidcraft_engine::CommandSpec;
use solidcraft_engine::doc::FeatureKind;

pub const RENDER_TABS: &[&str] = &["RENDER"];

pub fn render_tab(tab: &str) -> bool {
    RENDER_TABS.contains(&tab)
}
pub fn tab_label(tab: &str) -> &str {
    tab.strip_prefix("RENDER ").unwrap_or(tab)
}

/// A tab's registry name in normal word casing: "SHEET METAL" → "Sheet Metal".
pub fn word_cased(name: &str) -> String {
    name.split_whitespace()
        .map(|word| {
            let lower = word.to_lowercase();
            let mut chars = lower.chars();
            chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A ribbon tab's name as the ribbon shows it, in the current language: "SHEET METAL" reads
/// "Sheet Metal", or "Blech" in German. A partial catalog may translate only the word-cased name.
pub fn tab_name(tab: &str) -> String {
    let cased = word_cased(crate::i18n::tr_label(tab_label(tab)));
    crate::i18n::tr_label(&cased).to_string()
}

pub const TABS: &[&str] = &["MODEL", "SURFACE", "ASSEMBLE", "INSERT", "INSPECT", "MANAGE", "VIEW"];

/// Reading order of the compact, three-column constraint gallery. Icons come from each spec.
pub const CONSTRAINT_GALLERY: &[&str] = &[
    "sketch.constraint.horizontal_vertical",
    "sketch.constraint.coincident",
    "sketch.constraint.tangent",
    "sketch.constraint.equal",
    "sketch.constraint.parallel",
    "sketch.constraint.perpendicular",
];

pub struct CommandGroup {
    pub label: &'static str,
    pub ids: &'static [&'static str],
}

/// Optional headings for section lists. Unlisted commands retain their order at the end.
pub fn groups(tab: &str, panel: &str) -> &'static [CommandGroup] {
    match (tab, panel) {
        ("MODEL", "MODIFY") => &[
            CommandGroup { label: "Edges", ids: &["solid.fillet", "solid.chamfer"] },
            CommandGroup {
                label: "Faces",
                ids: &["solid.press_pull", "solid.draft", "solid.offset_face", "solid.replace_face", "solid.split_face", "face.delete"],
            },
            CommandGroup { label: "Bodies", ids: &["solid.shell", "solid.combine", "solid.split_body", "solid.scale", "solid.remove"] },
            CommandGroup { label: "Specialise", ids: &["sheet.convert"] },
        ],
        _ => &[],
    }
}

/// Menu order is independent of the ribbon's promoted-button order.
pub fn grouped_commands<'a>(tab: &str, panel: &str, commands: &[&'a CommandSpec]) -> Vec<(Option<&'static str>, &'a CommandSpec)> {
    let groups = groups(tab, panel);
    let mut out = Vec::new();
    for group in groups {
        let mut heading = Some(group.label);
        for id in group.ids {
            if let Some(command) = commands.iter().find(|c| c.id == *id) {
                out.push((heading.take(), *command));
            }
        }
    }
    out.extend(commands.iter().filter(|c| !groups.iter().any(|g| g.ids.contains(&c.id))).map(|c| (None, *c)));
    out
}

/// The ids of a panel's commands that are in none of its groups: [`grouped_commands`] puts them
/// last, without a heading.
pub fn ungrouped(tab: &str, panel: &str, commands: &[&CommandSpec]) -> Vec<&'static str> {
    let groups = groups(tab, panel);
    commands.iter().map(|c| c.id).filter(|id| !groups.iter().any(|g| g.ids.contains(id))).collect()
}

pub struct Section {
    pub tab: &'static str,
    pub panel: &'static str,
    pub promoted: usize,
    pub ids: &'static [&'static str],
}

/// Parameters and Configurations stay on every design tab they were on before the ribbon
/// redesign (MODEL and SHEET METAL), on SKETCH while editing a sketch, on WELDMENTS, and in
/// MANAGE, their canonical home.
const CONFIGURE: &[&str] = &["parameters.change", "config.table", "config.configure"];

/// Commands that stay for agents, MCP and scripts but are not offered to people: the sketch solves
/// itself after every edit, so a Solve button (and the search hit for it) only confuses; drawn 3D
/// points are moved with the triad that appears when one is clicked, not a Move button.
pub const AGENT_ONLY: &[&str] = &["sketch.solve", "sketch.move3d"];

pub const SECTIONS: &[Section] = &[
    Section {
        tab: "MODEL",
        panel: "SKETCH",
        promoted: 1,
        ids: &["sketch.create", "sketch.edit", "sketch.line3d", "sketch.arc3d", "sketch.spline3d", "sketch.point3d"],
    },
    Section { tab: "MODEL", panel: "CREATE", promoted: 3, ids: &["solid.extrude", "solid.revolve", "solid.hole"] },
    Section {
        tab: "MODEL",
        panel: "MODIFY",
        promoted: 4,
        ids: &["solid.fillet", "solid.chamfer", "solid.shell", "solid.press_pull", "sheet.convert"],
    },
    Section {
        tab: "MODEL",
        panel: "ARRANGE",
        promoted: 3,
        ids: &["solid.move", "solid.mirror", "solid.pattern.rectangular", "solid.align", "solid.pattern.circular", "solid.pattern.path"],
    },
    Section { tab: "MODEL", panel: "CONFIGURE", promoted: 2, ids: CONFIGURE },
    Section { tab: "MODEL", panel: "CONSTRUCT", promoted: 1, ids: &["construct.plane.offset"] },
    // Only while a linked design is edited in place, pinned at the right as FINISH SKETCH is.
    Section { tab: "MODEL", panel: "FINISH EDITING", promoted: 1, ids: &["component.finish_edit"] },
    // Sketches drive surfaces too: New Sketch leads the tab, as on MODEL and SHEET METAL.
    Section { tab: "SURFACE", panel: "SKETCH", promoted: 1, ids: &["sketch.create"] },
    Section { tab: "SURFACE", panel: "CREATE", promoted: 4, ids: &["surface.extrude", "surface.revolve", "surface.sweep", "surface.loft"] },
    Section { tab: "SURFACE", panel: "EDIT", promoted: 4, ids: &["surface.trim", "surface.untrim", "surface.extend", "surface.stitch"] },
    Section { tab: "SURFACE", panel: "CONVERT", promoted: 2, ids: &["surface.thicken", "mesh.convert"] },
    Section {
        tab: "ASSEMBLE",
        panel: "COMPONENTS",
        promoted: 4,
        ids: &["component.create", "component.from_bodies", "component.insert_design", "parts.library"],
    },
    Section {
        tab: "ASSEMBLE",
        panel: "JOINTS",
        promoted: 4,
        ids: &[
            "joint.create",
            "joint.as_built",
            "joint.origin",
            "joint.rigid_group",
            "joint.limits",
            "joint.gear",
            "joint.rack_pinion",
            "joint.screw",
            "joint.cam",
            "joint.reach",
        ],
    },
    Section {
        tab: "ASSEMBLE",
        panel: "POSITION",
        promoted: 4,
        ids: &["occurrence.move", "occurrence.ground", "component.capture_position", "component.revert_position"],
    },
    Section {
        tab: "ASSEMBLE",
        panel: "MOTION",
        promoted: 4,
        ids: &["joint.drive", "joint.motion_link", "motion.study", "motion.trace", "explode.create"],
    },
    Section {
        tab: "ASSEMBLE",
        panel: "CONTACT",
        promoted: 2,
        ids: &["contact.enable_sets", "contact.check", "contact.enable_all", "contact.disable_all", "contact.create", "contact.edit", "contact.list"],
    },
    Section {
        tab: "INSERT",
        panel: "FILES",
        promoted: 3,
        ids: &["file.insert_step", "file.insert_mesh", "pointcloud.insert", "component.insert_design", "file.save_mesh", "file.send_to_slicer"],
    },
    Section { tab: "INSERT", panel: "IMAGES", promoted: 3, ids: &["canvas.insert", "canvas.decal", "canvas.edit"] },
    Section { tab: "INSERT", panel: "2D", promoted: 2, ids: &["sketch.insert_dxf", "sketch.insert_svg"] },
    Section { tab: "INSERT", panel: "LIBRARY", promoted: 3, ids: &["parts.fastener", "parts.library", "parts.resize", "parts.insert"] },
    Section { tab: "INSERT", panel: "LINKS", promoted: 2, ids: &["component.update_links", "component.break_link"] },
    Section { tab: "INSPECT", panel: "MEASURE", promoted: 3, ids: &["inspect.measure", "inspect.center_of_mass", "component.properties"] },
    Section { tab: "INSPECT", panel: "SECTION", promoted: 1, ids: &["inspect.section"] },
    Section {
        tab: "INSPECT",
        panel: "CHECK",
        promoted: 4,
        ids: &["inspect.interference", "inspect.draft", "inspect.accessibility", "contact.check", "inspect.compare"],
    },
    Section {
        tab: "INSPECT",
        panel: "SURFACE QUALITY",
        promoted: 4,
        ids: &[
            "inspect.zebra",
            "inspect.curvature_map",
            "inspect.curvature_comb",
            "inspect.isocurve",
            "inspect.minimum_radius",
            "inspect.environment_map",
            "inspect.continuity",
        ],
    },
    Section { tab: "INSPECT", panel: "HISTORY", promoted: 3, ids: &["timeline.play", "timeline.dependents", "document.inspect"] },
    Section {
        tab: "MANAGE",
        panel: "PARAMETERS",
        promoted: 2,
        ids: &["parameters.change", "parameters.graph", "parameters.move", "parameters.import", "parameters.export"],
    },
    Section { tab: "MANAGE", panel: "CONFIGURATIONS", promoted: 2, ids: &["config.table", "config.configure", "config.rules", "config.preview"] },
    Section {
        tab: "MANAGE",
        panel: "MATERIALS",
        promoted: 2,
        ids: &["material.assign", "appearance.assign", "material.properties.validate", "library.inspect", "library.export", "library.apply"],
    },
    Section { tab: "MANAGE", panel: "RULES", promoted: 3, ids: &["sheet.manage_rules", "plastic.manage_rules", "plastic.assign_rule"] },
    Section {
        tab: "MANAGE",
        panel: "DESIGN",
        promoted: 2,
        ids: &["document.units", "timeline.compute_all", "edit.checkpoint", "edit.restore_checkpoint"],
    },
    Section {
        tab: "VIEW",
        panel: "VIEWS",
        promoted: 2,
        ids: &[
            "view.apply",
            "view.save",
            "view.rename",
            "view.delete",
            "view.list",
            "view.navigate",
            "view.fit",
            "view.home",
            "view.tool",
            "view.spin",
            "view.visual_style",
            "view.grid",
            "view.ground_shadow",
            "view.navigation_bar",
        ],
    },
    Section { tab: "VIEW", panel: "SELECT", promoted: 0, ids: &[] },
    Section {
        tab: "SKETCH",
        panel: "CREATE",
        promoted: 5,
        ids: &[
            "sketch.line",
            "sketch.rectangle.two_point",
            "sketch.circle.center",
            "sketch.arc.three_point",
            "sketch.construction_line",
            "sketch.infinite",
        ],
    },
    Section {
        tab: "SKETCH",
        panel: "MODIFY",
        promoted: 3,
        ids: &["sketch.trim", "sketch.offset", "sketch.fillet", "sketch.transform", "sketch.copy", "sketch.cut", "sketch.paste"],
    },
    Section { tab: "SKETCH", panel: "PATTERN", promoted: 3, ids: &["sketch.mirror", "sketch.pattern.circular", "sketch.pattern.rectangular"] },
    Section {
        tab: "SKETCH",
        panel: "CONSTRAIN",
        promoted: 7,
        ids: &[
            "sketch.dimension",
            "sketch.constraint.horizontal_vertical",
            "sketch.constraint.coincident",
            "sketch.constraint.tangent",
            "sketch.constraint.equal",
            "sketch.constraint.parallel",
            "sketch.constraint.perpendicular",
            "sketch.construction",
            "sketch.constrainer",
        ],
    },
    Section {
        tab: "SKETCH",
        panel: "REFERENCE",
        promoted: 1,
        ids: &[
            "sketch.project",
            "sketch.intersect",
            "sketch.include_3d",
            "sketch.project_to_surface",
            "sketch.intersection_curve",
            "sketch.spun_profile",
            "sketch.isoparametric_curve",
            "sketch.fit_curves_to_section",
            "sketch.insert_dxf",
            "sketch.insert_svg",
            "canvas.insert",
        ],
    },
    // Sketch dimensions are driven by parameters, so they are one click away while sketching.
    Section { tab: "SKETCH", panel: "INSPECT", promoted: 1, ids: &["sketch.inspect"] },
    Section { tab: "SKETCH", panel: "CONFIGURE", promoted: 2, ids: CONFIGURE },
    Section { tab: "SKETCH", panel: "FINISH SKETCH", promoted: 2, ids: &["sketch.finish", "sketch.cancel"] },
    // New Sketch leads the tab, as on MODEL: a sheet part starts from a sketch (#3250).
    Section { tab: "SHEET METAL", panel: "SKETCH", promoted: 1, ids: &["sketch.create"] },
    Section {
        tab: "SHEET METAL",
        panel: "CREATE",
        promoted: 4,
        ids: &["sheet.flange", "sheet.path_flange", "sheet.hem", "sheet.fold", "sheet.perforate", "sheet.convert"],
    },
    Section { tab: "SHEET METAL", panel: "FLAT", promoted: 3, ids: &["sheet.unfold", "sheet.refold", "sheet.flat_pattern"] },
    Section { tab: "SHEET METAL", panel: "OUTPUT", promoted: 2, ids: &["sheet.export_dxf", "sheet.export_bend_table"] },
    Section { tab: "SHEET METAL", panel: "RULES", promoted: 1, ids: &["sheet.manage_rules"] },
    Section { tab: "SHEET METAL", panel: "CONFIGURE", promoted: 2, ids: CONFIGURE },
    // A frame starts from a sketch of its lines; its profile sizes are often parameters.
    Section { tab: "WELDMENTS", panel: "SKETCH", promoted: 1, ids: &["sketch.create"] },
    Section { tab: "WELDMENTS", panel: "CREATE", promoted: 1, ids: &["frame.member"] },
    Section { tab: "WELDMENTS", panel: "MODIFY", promoted: 1, ids: &["frame.trim"] },
    Section { tab: "WELDMENTS", panel: "CONFIGURE", promoted: 2, ids: CONFIGURE },
];

// Migration data only: Render commands remain registered, but have no live ribbon sections.
const LEGACY_RENDER_SECTIONS: &[Section] = &[
    Section {
        tab: "RENDER APPEARANCE",
        panel: "APPEARANCE",
        promoted: 3,
        ids: &[
            "render.library.appearance.apply",
            "render.library.surface.bind",
            "render.aging.describe",
            "render.library.surface.prepare",
            "render.library.surface.unbind",
            "render.library.physical.prepare",
        ],
    },
    Section {
        tab: "RENDER SCENE",
        panel: "STUDIO",
        promoted: 3,
        ids: &[
            "render.preset.resolve",
            "render.library.studio.prepare",
            "render.library.studio.apply",
            "render.preset.apply",
            "render.library.environment.apply",
        ],
    },
    Section { tab: "RENDER SCENE", panel: "LIGHTS", promoted: 1, ids: &["render.lights.solo"] },
    Section {
        tab: "RENDER SCENE",
        panel: "STAGE",
        promoted: 2,
        ids: &["render.stage.ground", "render.stage.backdrop", "render.stage.clear", "render.hdr.inspect", "render.settings.capture_hdr"],
    },
    Section {
        tab: "RENDER CAMERA",
        panel: "SHOTS",
        promoted: 3,
        ids: &["render.settings.get", "render.settings.set", "render.shot.duplicate", "render.shot.rename", "render.shot.remove"],
    },
    Section { tab: "RENDER CAMERA", panel: "VIEW", promoted: 2, ids: &["render.settings.apply", "render.scene"] },
    Section { tab: "RENDER CAMERA", panel: "LENS", promoted: 1, ids: &["render.camera.get", "render.camera.set"] },
    Section { tab: "RENDER CAMERA", panel: "FRAME", promoted: 1, ids: &["render.camera.fit"] },
    Section { tab: "RENDER", panel: "IMAGE", promoted: 2, ids: &["render.quality.prepare", "render.effects.describe"] },
    Section { tab: "RENDER", panel: "RENDER", promoted: 2, ids: &["render.start", "render.preflight", "render.cancel"] },
    Section {
        tab: "RENDER",
        panel: "RESULT",
        promoted: 2,
        ids: &["render.export", "render.result.regrade", "render.compare", "render.status", "render.backends"],
    },
    Section { tab: "RENDER", panel: "BATCH", promoted: 1, ids: &["render.batch"] },
];

/// Compact source labels for painted big buttons only. Menus, search and tooltips
/// retain the engine label. Route these through `tr_label`, like other UI labels.
pub const RIBBON_LABELS: &[(&str, &str)] = &[
    ("render.batch", "All Shots"),
    ("render.cancel", "Cancel"),
    ("render.export", "Export Image"),
    ("render.result.regrade", "Regrade"),
    ("render.quality.prepare", "Quality"),
    ("render.settings.set", "Save Shot"),
    ("render.aging.describe", "Aging"),
    ("render.effects.describe", "Effects"),
    ("render.camera.fit", "Frame"),
    ("render.camera.get", "Camera"),
    ("render.preset.resolve", "Studios"),
    ("render.settings.get", "Shots"),
    ("render.library.appearance.apply", "Materials"),
    ("render.library.surface.bind", "Surface Maps"),
    ("render.library.studio.prepare", "Light Rigs"),
    ("render.library.studio.apply", "Edit Lights"),
    ("render.library.environment.apply", "HDR Library"),
    ("render.stage.backdrop", "Backgrounds"),
    ("render.stage.ground", "Ground"),
    ("render.settings.apply", "Saved Camera"),
    ("render.preflight", "Check"),
    ("appearance.assign", "Appearance"),
    ("component.capture_position", "Capture Pos."),
    ("component.create", "New Comp."),
    ("component.from_bodies", "From Bodies"),
    ("component.insert_design", "Insert"),
    ("component.properties", "Properties"),
    ("component.revert_position", "Revert Pos."),
    ("config.table", "Config Table"),
    ("contact.check", "Contact"),
    ("contact.enable_sets", "Contact Sets"),
    ("document.inspect", "Inspect"),
    ("document.units", "Units"),
    ("frame.trim", "Trim"),
    ("inspect.accessibility", "Access"),
    ("inspect.center_of_mass", "Mass Center"),
    ("inspect.curvature_comb", "Curv. Comb"),
    ("inspect.curvature_map", "Curvature"),
    ("inspect.draft", "Draft"),
    ("inspect.interference", "Interference"),
    ("inspect.isocurve", "Isocurve"),
    ("inspect.section", "Section"),
    ("inspect.zebra", "Zebra"),
    ("joint.as_built", "As-Built"),
    ("material.assign", "Material"),
    ("occurrence.move", "Move"),
    ("parameters.change", "Parameters"),
    ("parameters.graph", "Param. Graph"),
    ("parts.fastener", "Fastener"),
    ("parts.library", "Parts"),
    ("parts.resize", "Resize Part"),
    ("plastic.assign_rule", "Assign Rule"),
    ("plastic.manage_rules", "Plastic Rules"),
    ("pointcloud.insert", "Point Clouds"),
    ("render.scene", "Scene"),
    ("sheet.export_bend_table", "Bend Table"),
    ("sheet.export_dxf", "Export DXF"),
    ("sheet.flat_pattern", "Flat Pattern"),
    ("sheet.manage_rules", "Sheet Rules"),
    ("sheet.path_flange", "Path"),
    ("sketch.cancel", "Cancel"),
    ("sketch.circle.center", "Circle"),
    ("sketch.constraint.coincident", "Coincident"),
    ("sketch.constraint.horizontal_vertical", "Horiz./Vert."),
    ("sketch.construction_line", "X-Lines"),
    ("sketch.create", "New Sketch"),
    ("sketch.dimension", "Dimension"),
    ("sketch.finish", "Finish Sketch"),
    ("sketch.inspect", "Sketch Report"),
    ("sketch.pattern.circular", "Circular"),
    ("sketch.pattern.rectangular", "Rectangular"),
    ("sketch.rectangle.two_point", "Rectangle"),
    ("solid.move", "Move / Copy"),
    ("solid.pattern.rectangular", "Pattern"),
    ("surface.extrude", "Extrude"),
    ("surface.revolve", "Revolve"),
    ("surface.sweep", "Sweep"),
    ("timeline.dependents", "Dependents"),
    ("view.apply", "Restore View"),
    ("view.save", "Save View"),
];

pub fn ribbon_button_label(command: &CommandSpec) -> &str {
    RIBBON_LABELS.iter().find(|(id, _)| *id == command.id).map_or(command.label, |(_, label)| *label)
}

/// Families keep their full menu title while using the first command's compact ribbon name.
pub fn ribbon_family_label(family: &crate::sketch_families::Family) -> &'static str {
    family.commands.first().and_then(|first| RIBBON_LABELS.iter().find(|(id, _)| id == first)).map_or(family.label, |(_, label)| *label)
}

/// Registry placement → default ribbon destination, after explicit id overrides.
pub const PLACEMENTS: &[(&str, &str, &str, &str)] = &[
    ("SOLID", "CREATE", "MODEL", "CREATE"),
    ("SOLID", "MODIFY", "MODEL", "MODIFY"),
    ("SOLID", "CONSTRUCT", "MODEL", "CONSTRUCT"),
    ("SOLID", "ASSEMBLE", "ASSEMBLE", "JOINTS"),
    ("SOLID", "POSITION", "ASSEMBLE", "POSITION"),
    ("SOLID", "CONFIGURE", "MANAGE", "CONFIGURATIONS"),
    ("SOLID", "INSPECT", "INSPECT", "CHECK"),
    ("SOLID", "INSERT", "INSERT", "FILES"),
    ("SURFACE", "CREATE", "SURFACE", "CREATE"),
    ("SURFACE", "MODIFY", "SURFACE", "EDIT"),
    ("SURFACE", "INSPECT", "INSPECT", "SURFACE QUALITY"),
    ("SKETCH", "CREATE", "SKETCH", "CREATE"),
    ("SKETCH", "MODIFY", "SKETCH", "MODIFY"),
    ("SKETCH", "PATTERN", "SKETCH", "PATTERN"),
    ("SKETCH", "CONSTRAINTS", "SKETCH", "CONSTRAIN"),
    ("SKETCH", "INSERT", "INSERT", "IMAGES"),
    ("SKETCH", "INSPECT", "INSPECT", "SURFACE QUALITY"),
    ("SKETCH", "FINISH SKETCH", "SKETCH", "FINISH SKETCH"),
    ("SHEET METAL", "CREATE", "SHEET METAL", "CREATE"),
    ("SHEET METAL", "MODIFY", "SHEET METAL", "FLAT"),
    ("WELDMENTS", "CREATE", "WELDMENTS", "CREATE"),
    ("WELDMENTS", "MODIFY", "WELDMENTS", "MODIFY"),
    ("PLASTIC", "CREATE", "MODEL", "CREATE"),
    ("PLASTIC", "SETUP", "MANAGE", "RULES"),
    ("MESH", "MODIFY", "INSERT", "FILES"),
    ("RENDER", "SCENE", "RENDER CAMERA", "SHOTS"),
    ("RENDER", "IMAGE", "RENDER", "RENDER"),
];

pub fn panels(tab: &str) -> Vec<&'static str> {
    SECTIONS.iter().filter(|s| s.tab == tab).map(|s| s.panel).collect()
}

pub fn destination(c: &CommandSpec) -> Option<(&'static str, &'static str)> {
    if c.id.starts_with("render.") || c.tab.starts_with("RENDER") || c.id == "view.shots_strip" {
        return None;
    }
    legacy_render_destination(c).filter(|(tab, _)| !tab.starts_with("RENDER"))
}

pub(crate) fn legacy_render_destination(c: &CommandSpec) -> Option<(&'static str, &'static str)> {
    // A few shared commands have a canonical home that differs from their first occurrence.
    let primary = match c.id {
        "sheet.manage_rules" => Some(("MANAGE", "RULES")),
        "sheet.convert" => Some(("MODEL", "MODIFY")),
        "parameters.change" => Some(("MANAGE", "PARAMETERS")),
        "config.table" | "config.configure" => Some(("MANAGE", "CONFIGURATIONS")),
        _ => None,
    };
    primary
        .or_else(|| SECTIONS.iter().chain(LEGACY_RENDER_SECTIONS).find(|s| s.ids.contains(&c.id)).map(|s| (s.tab, s.panel)))
        .or_else(|| PLACEMENTS.iter().find(|(t, p, ..)| *t == c.tab && *p == c.panel).map(|(_, _, t, p)| (*t, *p)))
}

/// Ordered menu commands, with the promoted commands first. Empty sections are omitted by
/// panel_lists, except the selection filter, which has its own existing controls.
pub fn layout<'a>(tab: &str, panel: &str, specs: &[&'a CommandSpec]) -> Option<(Vec<&'a CommandSpec>, usize)> {
    let section = SECTIONS.iter().find(|s| s.tab == tab && s.panel == panel)?;
    let mut out: Vec<_> = section.ids.iter().filter_map(|id| specs.iter().find(|c| c.id == *id).copied()).collect();
    for c in specs.iter().filter(|c| destination(c) == Some((section.tab, section.panel))) {
        if !out.iter().any(|o| o.id == c.id) {
            out.push(*c);
        }
    }
    Some((out, section.promoted))
}

/// Old keys may be split across sections. Resolve each saved button by the same command map;
/// unknown command ids follow the old panel's default destination.
pub fn legacy_destination(tab: &str, panel: &str, id: &str) -> Option<(&'static str, &'static str)> {
    legacy_panel(tab, panel)?;
    solidcraft_engine::find_command(id).and_then(destination).or_else(|| legacy_panel(tab, panel))
}

pub fn legacy_panel(tab: &str, panel: &str) -> Option<(&'static str, &'static str)> {
    if panel == "SELECT" && matches!(tab, "SOLID" | "SURFACE" | "MESH" | "SHEET METAL" | "PLASTIC" | "UTILITIES" | "SKETCH") {
        return Some(("VIEW", "SELECT"));
    }
    PLACEMENTS.iter().find(|(t, p, ..)| *t == tab && *p == panel).map(|(_, _, t, p)| (*t, *p)).or(match (tab, panel) {
        ("PLASTIC" | "SHEET METAL", "MODIFY") => Some(("MODEL", "MODIFY")),
        ("PLASTIC" | "SHEET METAL" | "SURFACE" | "MESH", "ASSEMBLE") => Some(("ASSEMBLE", "JOINTS")),
        ("PLASTIC" | "SHEET METAL" | "SURFACE" | "MESH", "CONSTRUCT") => Some(("MODEL", "CONSTRUCT")),
        ("PLASTIC" | "SHEET METAL" | "SURFACE" | "MESH", "INSPECT") => Some(("INSPECT", "CHECK")),
        ("PLASTIC" | "SHEET METAL" | "SURFACE" | "MESH", "INSERT") => Some(("INSERT", "FILES")),
        ("MESH", "CREATE" | "PREPARE") => Some(("SURFACE", "CONVERT")),
        ("UTILITIES", "MAKE" | "ADD-INS" | "UTILITY") => Some(("MANAGE", "DESIGN")),
        ("UTILITIES", "INSPECT") => Some(("INSPECT", "CHECK")),
        _ => None,
    })
}

/// Timeline and browser icon of the sheet metal and plastic features (`None`: not one of them).
pub fn feature_icon(k: &FeatureKind) -> Option<&'static str> {
    Some(match k {
        FeatureKind::SheetBase { .. } | FeatureKind::SheetContour { .. } | FeatureKind::SheetFlange { .. } => "flange",
        FeatureKind::SheetHem { .. } => "hem",
        FeatureKind::SheetFold { .. } => "fold",
        FeatureKind::SheetPath { .. } => "path_flange",
        FeatureKind::SheetUnfold { refold: false, .. } => "unfold",
        FeatureKind::SheetUnfold { .. } => "refold",
        FeatureKind::SheetConvert { .. } => "convert_sheet",
        FeatureKind::Boss { .. } => "boss",
        FeatureKind::Lip { .. } => "lip",
        FeatureKind::SnapFit { .. } => "snap_fit",
        FeatureKind::Rest { .. } => "rest",
        FeatureKind::Rib { web: true, .. } => "web",
        FeatureKind::Rib { .. } => "rib",
        _ => return None,
    })
}

/// Selected geometry identifies the active body; without a body selection use the last body
/// of the active component, matching feature dialogs.
fn active_body<'a>(app: &'a crate::SolveApp, state: &'a solidcraft_engine::doc::ModelState) -> Option<&'a str> {
    use solidcraft_engine::Sel;
    let selected = app.session.selection.iter().find_map(|s| match s {
        Sel::Body { name } => Some(name.as_str()),
        Sel::Face { body, .. } | Sel::Edge { body, .. } => Some(body.as_str()),
        _ => None,
    });
    // The owners in one pass: asked body by body, each one searched the timeline, and the
    // ribbon asks this several times a frame.
    selected.or_else(|| {
        let owners = app.session.doc.model_body_components(state);
        state.bodies.iter().zip(owners).rev().find(|(_, c)| *c == app.session.active_component).map(|(b, _)| b.name.as_str())
    })
}

/// The active body when it is sheet metal. Rules on unrelated bodies do not count.
pub fn sheet_body(app: &crate::SolveApp) -> Option<String> {
    let state = app.session.model.state();
    let body = active_body(app, &state)?;
    state.sheets.iter().find(|s| s.body == body).map(|s| s.body.clone())
}

/// The active body when a Frame Member feature made it.
pub fn frame_body(app: &crate::SolveApp) -> Option<String> {
    let state = app.session.model.state();
    let name = active_body(app, &state)?;
    let body = state.bodies.iter().find(|b| b.name == name)?;
    let made_by = app.session.doc.feature(body.feature)?;
    matches!(made_by.kind, FeatureKind::FrameMember { .. }).then(|| body.name.clone())
}

/// The design tabs following the general-purpose tabs in the ribbon.
pub struct SpecialtyTab {
    pub tab: &'static str,
    pub icon: &'static str,
}

pub const SPECIALTY_TABS: &[SpecialtyTab] =
    &[SpecialtyTab { tab: "SHEET METAL", icon: "sheet" }, SpecialtyTab { tab: "WELDMENTS", icon: "frame_member" }];

/// A tab's row in the toolbar menu: the icon of the command that leads the tab (Model › Create ›
/// Extrude, Inspect › Measure, View › Views and so on).
pub fn tab_icon(tab: &str) -> &'static str {
    match tab {
        "MODEL" => "extrude",
        "SURFACE" => "surface_extrude",
        "ASSEMBLE" => "joint",
        "INSERT" => "import",
        "INSPECT" => "measure",
        "MANAGE" => "params",
        "VIEW" => "camera",
        _ => SPECIALTY_TABS.iter().find(|o| o.tab == tab).map_or("", |o| o.icon),
    }
}

/// Tabs shown by default, in row order. Sketch has its own visibility preference and the Render
/// workspace owns its controls, so neither is on this list.
pub fn choosable_tabs() -> impl Iterator<Item = &'static str> {
    TABS.iter().copied().chain(SPECIALTY_TABS.iter().map(|o| o.tab))
}

/// The person's choice. Every tab is on until explicitly hidden.
pub fn tab_turned_on(app: &crate::SolveApp, tab: &str) -> bool {
    choosable_tabs().any(|t| t == tab) && !app.preferences.hidden_toolbar_tabs.iter().any(|t| t == tab)
}

/// Show or hide a tab (kept with the preferences); names not on the list are left alone.
pub fn set_tab_shown(app: &mut crate::SolveApp, tab: &str, shown: bool) {
    if !choosable_tabs().any(|t| t == tab) {
        return;
    }
    app.preferences.hidden_toolbar_tabs.retain(|t| t != tab);
    if !shown {
        app.preferences.hidden_toolbar_tabs.push(tab.to_string());
    }
}

/// Every design tab stays hidden until the person chooses to show it again.
pub fn visible_tabs(app: &crate::SolveApp) -> Vec<&'static str> {
    if crate::render_workspace::active(app) {
        return Vec::new();
    }
    let mut tabs: Vec<_> = TABS.iter().copied().filter(|tab| tab_turned_on(app, tab)).collect();
    if app.preferences.sketch_tab_always {
        // Right after MODEL, whether a sketch is open or not (#7904).
        let at = tabs.iter().position(|t| *t == "MODEL").map_or(0, |i| i + 1);
        tabs.insert(at, "SKETCH");
    } else if app.session.active_sketch.is_some() {
        tabs.push("SKETCH");
    }
    tabs.extend(SPECIALTY_TABS.iter().map(|o| o.tab).filter(|tab| tab_turned_on(app, tab)));
    tabs
}

/// Where the ribbon goes when the selected tab is not in the row: Model, or the first tab shown
/// while Model is hidden; none when every tab is hidden.
fn home_tab(app: &crate::SolveApp) -> &'static str {
    let tabs = visible_tabs(app);
    if tabs.contains(&"MODEL") { "MODEL" } else { tabs.first().copied().unwrap_or_default() }
}

/// With the SKETCH tab kept shown (#7904) and no sketch open: whether `id` is one of the tools in
/// its CREATE or REFERENCE sections, which then start a sketch first (Create Sketch, then the
/// tool). Tools that work on a sketch's geometry (modify, pattern, constrain) wait for one.
pub fn starts_a_sketch(app: &crate::SolveApp, id: &str) -> bool {
    app.preferences.sketch_tab_always
        && app.session.active_sketch.is_none()
        && solidcraft_engine::find_command(id)
            .and_then(destination)
            .is_some_and(|(tab, panel)| tab == "SKETCH" && matches!(panel, "CREATE" | "REFERENCE"))
}

/// Select contextual tabs on entry (or a different sheet body or frame member), allowing the
/// other tabs to remain usable for the rest of that context. Sketch takes precedence over sheet
/// metal, and sheet metal over weldments.
pub fn sync_context(app: &mut crate::SolveApp) {
    if crate::render_workspace::active(app) {
        return;
    }
    let context = app
        .session
        .active_sketch
        .map(|id| format!("sketch:{id}"))
        .or_else(|| sheet_body(app).map(|body| format!("sheet:{body}")))
        .or_else(|| frame_body(app).map(|body| format!("frame:{body}")));
    if app.ui.ribbon_context != context {
        let was_sketch = app.ui.ribbon_context.as_deref().is_some_and(|c| c.starts_with("sketch:"));
        let back = if app.session.active_sketch.is_some() {
            // Remember the tab the sketch was started from (#3250).
            if !was_sketch && app.ui.tab != "SKETCH" {
                app.ui.sketch_return_tab = Some(app.ui.tab.clone());
            }
            None
        } else if was_sketch {
            app.ui.sketch_return_tab.take()
        } else {
            None
        };
        app.ui.tab = if app.session.active_sketch.is_some() {
            "SKETCH"
        } else if let Some(back) = back.as_deref().filter(|t| *t != "SKETCH" && visible_tabs(app).contains(t)) {
            // Finishing a sketch goes back to the tab it came from (#3250).
            visible_tabs(app).into_iter().find(|t| t == &back).unwrap_or_else(|| home_tab(app))
        } else if context.as_deref().is_some_and(|c| c.starts_with("frame:")) {
            "WELDMENTS"
        } else if context.is_some() {
            "SHEET METAL"
        } else {
            home_tab(app)
        }
        .into();
        app.ui.ribbon_context = context;
    }
    if !visible_tabs(app).contains(&app.ui.tab.as_str()) {
        app.ui.tab = home_tab(app).into();
    }
}

pub fn select_command_tab(app: &mut crate::SolveApp, id: &str) {
    sync_context(app);
    if let Some(tab) = command_tab(app, id) {
        app.ui.tab = tab.into();
    }
}

/// The tab to switch to so the ribbon shows command `id`, or `None` to stay: the current tab
/// already lists it, or its home tab can't be shown.
pub fn command_tab(app: &crate::SolveApp, id: &str) -> Option<&'static str> {
    let c = solidcraft_engine::find_command(id)?;
    // Keep the tab a toolbar click came from when it already lists this shared command.
    if crate::toolbar_custom::panel_lists(&app.ui.tab).iter().any(|(_, cmds, _)| cmds.iter().any(|c| c.id == id)) {
        return None;
    }
    let (tab, _) = destination(c)?;
    // The Render workspace owns its entry/exit lifecycle.
    let render_closed = render_tab(tab) && !crate::render_workspace::active(app);
    (!render_closed && visible_tabs(app).contains(&tab)).then_some(tab)
}

/// Defaults from the previous ribbon, used only to carry removals into the new layout.
pub fn legacy_buttons(tab: &str, panel: &str) -> Vec<&'static str> {
    let (ids, n, source): (&[&str], usize, &str) = match (tab, panel) {
        ("SKETCH", "CREATE") => (
            &[
                "sketch.line",
                "sketch.rectangle.two_point",
                "sketch.circle.center",
                "sketch.arc.three_point",
                "sketch.slot.center_to_center",
                "sketch.polygon.inscribed",
                "sketch.spline.fit_point",
                "sketch.point",
            ],
            8,
            tab,
        ),
        ("SKETCH", "PATTERN") => (&["sketch.mirror", "sketch.pattern.rectangular", "sketch.pattern.circular"], 3, tab),
        ("SKETCH", "CONSTRAINTS") => (&[], 6, tab),
        ("SOLID", "CONFIGURE") => (&["parameters.change", "config.table"], 2, tab),
        ("SOLID" | "SHEET METAL" | "PLASTIC", "ASSEMBLE") => (&["component.create", "joint.create", "joint.as_built"], 3, tab),
        ("SHEET METAL", "CREATE") => (&["sketch.create", "sheet.flange", "sheet.flat_pattern"], 3, tab),
        ("SHEET METAL", "MODIFY") => (&["sheet.manage_rules", "sheet.unfold"], 2, tab),
        ("PLASTIC", "SETUP") => (&["plastic.manage_rules", "plastic.assign_rule"], 2, tab),
        ("PLASTIC", "CREATE") => (&["sketch.create", "plastic.boss", "plastic.snap_fit", "plastic.lip", "plastic.rest"], 5, tab),
        ("SOLID" | "PLASTIC", "MODIFY") => (&[], 4, "SOLID"),
        ("SOLID", "CREATE") => (&[], 4, tab),
        ("SHEET METAL" | "PLASTIC", "CONSTRUCT" | "INSPECT" | "INSERT") => (&[], 2, "SOLID"),
        (_, "FINISH SKETCH") => (&[], 1, tab),
        _ => (&[], 2, tab),
    };
    let mut out = ids.to_vec();
    for c in solidcraft_engine::command_specs().into_iter().filter(|c| c.tab == source && c.panel == panel) {
        if !out.contains(&c.id) {
            out.push(c.id);
        }
    }
    out.truncate(n);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn modify_groups_order_commands_and_keep_unlisted_commands_at_the_end() {
        let specs = solidcraft_engine::command_specs();
        let (commands, _) = layout("MODEL", "MODIFY", &specs).unwrap();
        let grouped = grouped_commands("MODEL", "MODIFY", &commands);
        assert_eq!(grouped.iter().filter_map(|(h, _)| *h).collect::<Vec<_>>(), ["Edges", "Faces", "Bodies", "Specialise"]);
        assert_eq!(grouped.len(), commands.len());
        assert_eq!(
            grouped.iter().map(|(_, c)| c.id).collect::<Vec<_>>(),
            [
                "solid.fillet",
                "solid.chamfer",
                "solid.press_pull",
                "solid.draft",
                "solid.offset_face",
                "solid.replace_face",
                "solid.split_face",
                "face.delete",
                "solid.shell",
                "solid.combine",
                "solid.split_body",
                "solid.scale",
                "solid.remove",
                "sheet.convert",
                "timeline.delete",
            ]
        );
        let plain = grouped_commands("SURFACE", "CREATE", &commands);
        assert!(plain.iter().all(|(h, _)| h.is_none()));
        assert_eq!(plain.iter().map(|(_, c)| c.id).collect::<Vec<_>>(), commands.iter().map(|c| c.id).collect::<Vec<_>>());
    }

    #[test]
    fn promoted_long_labels_have_compact_ribbon_names() {
        let specs = solidcraft_engine::command_specs();
        let english: std::collections::BTreeMap<String, String> =
            serde_json::from_str(include_str!(concat!(env!("OUT_DIR"), "/locales/en.json"))).unwrap();
        for section in SECTIONS {
            let (commands, count) = layout(section.tab, section.panel, &specs).unwrap();
            for command in commands.iter().take(count) {
                // Gallery cells are icon-only; they retain full names in their tooltips.
                if section.tab == "SKETCH" && section.panel == "CONSTRAIN" && CONSTRAINT_GALLERY.contains(&command.id) {
                    continue;
                }
                if command.label.chars().count() > 12 || command.label.split_whitespace().any(|word| word.chars().count() > 9) {
                    assert!(RIBBON_LABELS.iter().any(|(id, _)| *id == command.id), "missing compact label for {}", command.id);
                }
            }
        }
        for (id, label) in RIBBON_LABELS {
            assert!(solidcraft_engine::find_command(id).is_some(), "unknown command {id}");
            assert!(label.chars().count() <= 13, "{id}: {label}");
            assert!(english.values().any(|value| value == label), "untranslated ribbon label {label}");
        }
        assert_eq!(ribbon_button_label(solidcraft_engine::find_command("sketch.create").unwrap()), "New Sketch");
        assert_eq!(solidcraft_engine::find_command("sketch.create").unwrap().label, "Create Sketch");
        let construction = crate::sketch_families::family("sketch.construction_line").unwrap();
        assert_eq!(ribbon_family_label(construction), "X-Lines");
        assert_eq!(construction.label, "Construction Line");
    }

    #[test]
    fn every_placed_command_is_reachable_and_every_layout_id_exists() {
        let specs = solidcraft_engine::command_specs();
        let lists: Vec<_> = SECTIONS.iter().map(|s| (s, layout(s.tab, s.panel, &specs).unwrap())).collect();
        for c in specs.iter().filter(|c| !c.tab.is_empty() && !c.tab.starts_with("RENDER")) {
            assert!(lists.iter().any(|(_, (cmds, _))| cmds.iter().any(|x| x.id == c.id)), "lost toolbar command {} ({}/{})", c.id, c.tab, c.panel);
        }
        for (s, (cmds, n)) in lists {
            assert!(
                n <= 4 || (s.tab == "SKETCH" && ((s.panel == "CONSTRAIN" && n == 7) || (s.panel == "CREATE" && n == 5))),
                "{}/{} promotes {n} commands",
                s.tab,
                s.panel
            );
            for id in s.ids {
                assert!(specs.iter().any(|c| c.id == *id), "unknown layout id {id}");
            }
            let unique: std::collections::BTreeSet<_> = cmds.iter().map(|c| c.id).collect();
            assert_eq!(unique.len(), cmds.len());
        }
    }

    #[test]
    fn model_defaults_and_shared_commands_follow_the_map() {
        let specs = solidcraft_engine::command_specs();
        for (panel, expected) in [
            ("SKETCH", vec!["sketch.create"]),
            ("CREATE", vec!["solid.extrude", "solid.revolve", "solid.hole"]),
            ("MODIFY", vec!["solid.fillet", "solid.chamfer", "solid.shell", "solid.press_pull"]),
            ("ARRANGE", vec!["solid.move", "solid.mirror", "solid.pattern.rectangular"]),
            ("CONSTRUCT", vec!["construct.plane.offset"]),
        ] {
            let (cmds, n) = layout("MODEL", panel, &specs).unwrap();
            assert_eq!(cmds.iter().take(n).map(|c| c.id).collect::<Vec<_>>(), expected);
        }
        for (tab, panel, id) in [
            ("MODEL", "CREATE", "plastic.boss"),
            ("INSERT", "FILES", "component.insert_design"),
            ("ASSEMBLE", "COMPONENTS", "component.insert_design"),
            ("SHEET METAL", "CREATE", "sheet.convert"),
            ("SHEET METAL", "SKETCH", "sketch.create"),
            ("SURFACE", "SKETCH", "sketch.create"),
            ("SKETCH", "REFERENCE", "canvas.insert"),
        ] {
            assert!(layout(tab, panel, &specs).unwrap().0.iter().any(|c| c.id == id));
        }
    }

    /// The ribbon redesign (#2176) removed the MESH and UTILITIES tabs. A command that still
    /// declares one is advertised at a place the ribbon cannot show, because the generated command
    /// inventory prints the placement a spec declares (#133).
    #[test]
    fn no_command_is_advertised_at_a_tab_the_ribbon_removed() {
        for c in solidcraft_engine::command_specs() {
            assert!(!matches!(c.tab, "MESH" | "UTILITIES"), "{} declares the removed {} tab", c.id, c.tab);
        }
        let spec = solidcraft_engine::find_command("file.save_mesh").expect("file.save_mesh is registered");
        assert_eq!((spec.tab, spec.panel), ("INSERT", "FILES"), "Save As Mesh declares the panel that shows it");
        assert_eq!(destination(spec), Some(("INSERT", "FILES")));
    }

    /// Without a body picked, the active body is the last one the active component owns, whatever
    /// the other components hold after it.
    #[test]
    fn without_a_selection_the_active_body_is_the_active_components_last() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"body_name":"Top1", "operation":"new", "length":10, "width":10, "height":10})).unwrap();
        let sub = h.app.run("component.create", json!({"name":"Sub"})).unwrap()["component"].as_u64().unwrap();
        h.app.run("component.activate", json!({"component":sub})).unwrap();
        h.app.run("solid.box", json!({"body_name":"Sub1", "operation":"new", "length":5, "width":5, "height":5})).unwrap();
        h.app.run("component.activate", json!({"component":0})).unwrap();
        h.app.run("solid.box", json!({"body_name":"Top2", "operation":"new", "length":4, "width":4, "height":4})).unwrap();
        h.app.run("component.activate", json!({"component":sub})).unwrap();
        h.app.run("solid.box", json!({"body_name":"Sub2", "operation":"new", "length":3, "width":3, "height":3})).unwrap();
        h.app.run("select.set", json!({"items":[]})).unwrap();
        let active = |h: &crate::scenario::Harness| active_body(&h.app, &h.app.session.model.state()).map(str::to_owned);
        assert_eq!(active(&h).as_deref(), Some("Sub2"));
        h.app.run("component.activate", json!({"component":0})).unwrap();
        h.app.run("select.set", json!({"items":[]})).unwrap();
        assert_eq!(active(&h).as_deref(), Some("Top2"), "the top level's last body, not a later one in Sub");
        h.app.run("select.set", json!({"items":[{"type":"body","name":"Sub1"}]})).unwrap();
        assert_eq!(active(&h).as_deref(), Some("Sub1"), "a picked body wins");
    }

    #[test]
    fn contextual_tabs_follow_the_active_body_and_allow_other_tabs() {
        let mut h = crate::scenario::Harness::new();
        let mut fresh = TABS.to_vec();
        fresh.extend(["SHEET METAL", "WELDMENTS"]);
        assert_eq!(visible_tabs(&h.app), fresh, "SHEET METAL and WELDMENTS are reachable before any sheet body or member exists");
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        assert_eq!(h.app.ui.tab, "SKETCH");
        h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,30]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("sheet.flange", json!({"name":"Sheet", "body_name":"Sheet"})).unwrap();
        assert_eq!(h.app.ui.tab, "SHEET METAL");
        assert!(visible_tabs(&h.app).contains(&"SHEET METAL"));
        let path = h.dir.join("sheet.solidcraft");
        h.app.run("file.save", json!({"path":path})).unwrap();
        h.app.ui.tab = "INSPECT".into();
        h.app.run("doc.open", json!({"path":path})).unwrap();
        assert_eq!(h.app.ui.tab, "SHEET METAL", "opening a document re-enters its sheet context");

        h.app.ui.tab = "INSPECT".into();
        sync_context(&mut h.app);
        assert_eq!(h.app.ui.tab, "INSPECT");
        h.app.run("solid.box", json!({"body_name":"Solid", "operation":"new", "length":10, "width":10, "height":10})).unwrap();
        h.app.run("select.set", json!({"items":[{"type":"body","name":"Solid"}]})).unwrap();
        sync_context(&mut h.app);
        assert!(visible_tabs(&h.app).contains(&"SHEET METAL"), "the tab stays reachable off a sheet body by default");
        assert!(sheet_body(&h.app).is_none());
        h.app.run("select.set", json!({"items":[{"type":"body","name":"Sheet"}]})).unwrap();
        sync_context(&mut h.app);
        assert_eq!(h.app.ui.tab, "SHEET METAL");
    }

    /// Weldments is there in a fresh document (a frame starts from a sketch of its lines), holds
    /// Frame Member, and selects itself while a member is the active body, as Sheet Metal does for
    /// a sheet body, but keeps the ordinary tab look.
    #[test]
    fn weldments_tab_holds_frame_member_and_follows_a_member_body() {
        let specs = solidcraft_engine::command_specs();
        assert_eq!(panels("WELDMENTS"), ["SKETCH", "CREATE", "MODIFY", "CONFIGURE"]);
        let (create, n) = layout("WELDMENTS", "CREATE", &specs).unwrap();
        assert_eq!(create.iter().take(n).map(|c| c.id).collect::<Vec<_>>(), ["frame.member"]);
        assert!(!panel_ids("MODEL", &specs).contains(&"frame.member"), "Frame Member's home is Weldments");

        let mut h = crate::scenario::Harness::new();
        h.step(&json!({"click":{"handle":"tab:WELDMENTS"}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.ui.tab, "WELDMENTS");
        for id in ["sketch.create", "frame.member", "parameters.change"] {
            assert!(crate::scenario::handle_at(&format!("button:{id}")).is_some(), "{id} is a button on the Weldments tab");
        }
        h.step(&json!({"click":{"handle":"button:frame.member"}})).unwrap();
        assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(crate::dialogs::Kind::Frame(_))));
        assert_eq!(h.app.ui.tab, "WELDMENTS", "Frame Member opens from the tab without leaving it");
        h.step(&json!({"key":"Escape"})).unwrap();

        h.app.run("sketch.create", json!({"plane":"XZ", "name":"Frame"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[600,400]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("solid.box", json!({"body_name":"Plate", "operation":"new", "length":100, "width":100, "height":10})).unwrap();
        assert!(frame_body(&h.app).is_none(), "a plain solid is not a member");
        h.app.ui.tab = "INSPECT".into();
        let tube = json!({"kind":"rect_tube", "width":40, "height":40, "wall":3});
        h.app.run("frame.member", json!({"sketch":"Frame", "profile":tube, "body_name":"Rail"})).unwrap();
        assert_eq!(frame_body(&h.app).as_deref(), Some("Rail (4)"));
        assert_eq!(h.app.ui.tab, "WELDMENTS", "a new member selects the tab");
        assert!(!crate::toolbar::contextual_tab(&h.app, "WELDMENTS"), "selected on a member, it keeps the ordinary tab look");

        h.app.run("select.set", json!({"items":[{"type":"body","name":"Plate"}]})).unwrap();
        sync_context(&mut h.app);
        assert!(frame_body(&h.app).is_none());
        assert!(visible_tabs(&h.app).contains(&"WELDMENTS"), "the tab stays reachable off a member");
        h.app.run("select.set", json!({"items":[{"type":"body","name":"Rail (2)"}]})).unwrap();
        sync_context(&mut h.app);
        assert_eq!(h.app.ui.tab, "WELDMENTS");
    }

    /// Tabs show until the person hides them, from the same menu and Settings, down
    /// to no tab at all; the plus stays, so they can come back (#5717).
    #[test]
    fn standard_tabs_show_until_hidden_and_every_tab_can_be_hidden() {
        let mut h = crate::scenario::Harness::new();
        h.frames(2);
        assert!(h.app.preferences.hidden_toolbar_tabs.is_empty());
        assert_eq!(h.app.ui.tab, "MODEL");

        // Hide the selected tab from its own menu: it leaves the row and the next tab is selected.
        h.step(&json!({"click":{"handle":"tab:MODEL"},"button":"right"})).unwrap();
        h.call("ui.menuPick", json!({"item":"Model"}));
        h.frames(2);
        assert_eq!(h.app.preferences.hidden_toolbar_tabs, ["MODEL"]);
        assert!(crate::scenario::handle_at("tab:MODEL").is_none());
        assert_eq!(h.app.ui.tab, "SURFACE", "the first tab still shown is selected");

        // Settings › Layout lists the standard tabs too.
        h.call("ui.prefs", json!({"open":"Layout"}));
        crate::prefs::reveal_preference(&mut h, "prefs:toolbar_tab:MODEL");
        h.step(&json!({"click":{"handle":"prefs:toolbar_tab:MODEL"}})).unwrap();
        assert!(h.app.preferences.hidden_toolbar_tabs.is_empty());
        crate::prefs::reveal_preference(&mut h, "prefs:toolbar_tab:VIEW");
        h.step(&json!({"click":{"handle":"prefs:toolbar_tab:VIEW"}})).unwrap();
        assert_eq!(h.app.preferences.hidden_toolbar_tabs, ["VIEW"]);
        h.call("ui.prefs", json!({"button":"defaults"}));
        assert!(h.app.preferences.hidden_toolbar_tabs.is_empty(), "the Interface page's defaults show every standard tab");
        h.call("ui.prefs", json!({"button":"ok"}));

        // The control channel checks the list, and it is kept with the preferences.
        assert!(h.call("ui.prefs", json!({"hidden_toolbar_tabs": ["SKETCH"]}))["error"].is_string(), "only choosable tabs");
        assert!(h.call("ui.prefs", json!({"hidden_toolbar_tabs": ["VIEW", "VIEW"]}))["error"].is_string(), "each once");
        assert!(h.call("ui.prefs", json!({"hidden_toolbar_tabs": choosable_tabs().collect::<Vec<_>>()}))["error"].is_null());
        let mut other = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        other.load_prefs(&h.app.prefs());
        assert!(visible_tabs(&other).is_empty());

        // Every tab hidden: only the plus is left in the row, the ribbon says how to bring the
        // tabs back, and the menu has no tab to customise.
        h.frames(2);
        assert!(visible_tabs(&h.app).is_empty());
        assert_eq!(h.app.ui.tab, "");
        assert!(choosable_tabs().all(|tab| crate::scenario::handle_at(&format!("tab:{tab}")).is_none()));
        assert!(crate::scenario::handle_at("ribbon:tabs_hidden").is_some());
        let items = crate::toolbar_custom::toolbar_items(&h.app);
        assert!(!items.iter().any(|i| matches!(i.id.as_str(), "ui.toolbarCustomize" | "ui.toolbarReset")));
        // A sketch still brings its tab, and finishing it leaves the row empty again.
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.frames(2);
        assert_eq!(visible_tabs(&h.app), ["SKETCH"]);
        assert_eq!(h.app.ui.tab, "SKETCH");
        h.app.finish_sketch();
        h.frames(2);
        assert_eq!(h.app.ui.tab, "");
        // The plus brings a tab back.
        h.step(&json!({"click":{"handle":"ribbon:add_tab"}})).unwrap();
        h.call("ui.menuPick", json!({"item":"Inspect"}));
        h.frames(2);
        assert_eq!(visible_tabs(&h.app), ["INSPECT"]);
        assert_eq!(h.app.ui.tab, "INSPECT");
        assert!(crate::scenario::handle_at("ribbon:tabs_hidden").is_none());
        // A hidden tab's commands are still found by command search, and start from there.
        let found = crate::shortcut_box::search(&h.app, "Change Parameters");
        assert_eq!(found.first().map(|row| row.0), Some("parameters.change"), "Parameters is on the hidden Model and Manage tabs");
        h.step(&json!({"start":"parameters.change"})).unwrap();
        assert!(h.app.params.is_some());
        assert_eq!(h.app.ui.tab, "INSPECT", "its tabs stay hidden");
        h.call("ui.prefs", json!({"hidden_toolbar_tabs": []}));
    }

    #[test]
    fn specialty_tabs_show_by_default_and_stay_hidden_for_features() {
        let mut h = crate::scenario::Harness::new();
        h.frames(2);
        assert_eq!(visible_tabs(&h.app), choosable_tabs().collect::<Vec<_>>());
        h.step(&json!({"click":{"handle":"ribbon:add_tab"}})).unwrap();
        let rows: Vec<_> = crate::toolbar_custom::toolbar_items(&h.app).into_iter().filter(|i| i.id == "ui.toolbarTab").collect();
        assert_eq!(
            rows.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(),
            ["Model", "Surface", "Assemble", "Insert", "Inspect", "Manage", "View", "Sheet Metal", "Weldments"]
        );
        assert!(rows.iter().all(|i| i.shortcut == "current"), "all existing tabs stay on by default");
        h.step(&json!({"click":{"handle":"ribbon:add_tab"}})).unwrap();
        assert!(h.app.viewport.context_menu.is_none(), "a second click closes the menu");
        h.step(&json!({"key":"S"})).unwrap();
        assert!(h.app.ui.palette_open, "the plus leaves single-key shortcuts available");
        h.step(&json!({"key":"Escape"})).unwrap();
        h.step(&json!({"click":{"handle":"ribbon:add_tab"}})).unwrap();
        h.call("ui.menuPick", json!({"item":"Sheet Metal"}));
        h.frames(2);
        assert_eq!(h.app.preferences.hidden_toolbar_tabs, ["SHEET METAL"]);
        assert!(crate::scenario::handle_at("tab:SHEET METAL").is_none());
        assert!(visible_tabs(&h.app).contains(&"WELDMENTS"));
        let mut other = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        other.load_prefs(&h.app.prefs());
        assert!(!visible_tabs(&other).contains(&"SHEET METAL"));

        h.app.run("sketch.create", json!({"plane":"XY", "name":"Tab path"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,30]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("sheet.flange", json!({"name":"Sheet", "body_name":"Sheet"})).unwrap();
        sync_context(&mut h.app);
        h.frames(2);
        assert_eq!(h.app.ui.tab, "MODEL", "a sheet context cannot select its hidden tab");
        assert!(!visible_tabs(&h.app).contains(&"SHEET METAL"));
        assert!(crate::scenario::handle_at("tab:SHEET METAL").is_none());
        assert_eq!(h.app.preferences.hidden_toolbar_tabs, ["SHEET METAL"], "features do not change the saved choice");
        let path = h.dir.join("hidden-sheet.solidcraft");
        h.app.run("file.save", json!({"path":path})).unwrap();
        other.run("doc.open", json!({"path":path})).unwrap();
        assert!(!visible_tabs(&other).contains(&"SHEET METAL"), "opening a sheet design respects the saved hiding choice");
        sync_context(&mut other);
        assert_eq!(other.ui.tab, "MODEL");
        h.app.run("edit.undo", json!({})).unwrap();
        sync_context(&mut h.app);
        assert!(!visible_tabs(&h.app).contains(&"SHEET METAL"));

        h.call("ui.prefs", json!({"hidden_toolbar_tabs": ["SHEET METAL", "WELDMENTS"]}));
        h.app.run("frame.member", json!({"sketch":"Tab path", "profile":{"kind":"rect_tube","width":10,"height":10,"wall":1}})).unwrap();
        sync_context(&mut h.app);
        h.frames(2);
        assert_eq!(h.app.ui.tab, "MODEL", "a frame context cannot select its hidden tab");
        assert!(!visible_tabs(&h.app).contains(&"WELDMENTS"));
        assert!(crate::scenario::handle_at("tab:WELDMENTS").is_none());
        h.app.run("file.save", json!({"path":path})).unwrap();
        set_tab_shown(&mut other, "WELDMENTS", false);
        other.run("doc.open", json!({"path":path})).unwrap();
        assert!(!visible_tabs(&other).contains(&"WELDMENTS"), "opening a frame design respects the saved hiding choice");
        sync_context(&mut other);
        assert_eq!(other.ui.tab, "MODEL");
        h.call("ui.prefs", json!({"hidden_toolbar_tabs":choosable_tabs().collect::<Vec<_>>()}));
        h.frames(2);
        assert!(visible_tabs(&h.app).is_empty(), "a design with a frame member can still hide every tab");
        assert_eq!(h.app.ui.tab, "");
        h.app.run("edit.undo", json!({})).unwrap();
        sync_context(&mut h.app);
        assert!(!visible_tabs(&h.app).contains(&"WELDMENTS"));

        // The Sketch visibility setting still works when every other tab is hidden.
        h.call("ui.prefs", json!({"hidden_toolbar_tabs":choosable_tabs().collect::<Vec<_>>(), "sketch_tab_always":true}));
        h.frames(2);
        assert_eq!(visible_tabs(&h.app), ["SKETCH"]);
        assert_eq!(h.app.ui.tab, "SKETCH");

        h.call("ui.prefs", json!({"open":"Layout"}));
        h.call("ui.prefs", json!({"button":"defaults"}));
        h.call("ui.prefs", json!({"button":"ok"}));
        assert!(h.app.preferences.hidden_toolbar_tabs.is_empty());
        assert_eq!(visible_tabs(&h.app), choosable_tabs().collect::<Vec<_>>(), "defaults restore Sheet Metal and Weldments too");
    }

    /// Parameters and Configurations are on the design tabs they were on before the redesign, and a
    /// fresh document reaches Sheet Metal (to flange a sketch or convert a solid) by its tab.
    #[test]
    fn parameters_and_configurations_are_on_model_and_sheet_metal_and_the_sheet_tab_opens_fresh() {
        let specs = solidcraft_engine::command_specs();
        for tab in ["MODEL", "SHEET METAL", "SKETCH", "MANAGE"] {
            let ids = panel_ids(tab, &specs);
            assert!(ids.contains(&"parameters.change"), "{tab}: Parameters");
            assert!(ids.contains(&"config.table"), "{tab}: Configurations");
        }
        for tab in ["MODEL", "SHEET METAL", "SKETCH"] {
            let (commands, n) = layout(tab, "CONFIGURE", &specs).unwrap();
            assert_eq!(commands.iter().take(n).map(|c| c.id).collect::<Vec<_>>(), ["parameters.change", "config.table"]);
        }
        let mut h = crate::scenario::Harness::new();
        assert!(sheet_body(&h.app).is_none());
        h.frames(2);
        h.step(&json!({"click":{"handle":"tab:SHEET METAL"}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.ui.tab, "SHEET METAL");
        let specs = solidcraft_engine::command_specs();
        let (create, _) = layout("SHEET METAL", "CREATE", &specs).unwrap();
        assert!(create.iter().any(|c| c.id == "sheet.convert"), "Convert to Sheet Metal is in the Sheet Metal tab's Create panel");
        for id in ["sheet.flange", "parameters.change", "config.table"] {
            assert!(crate::scenario::handle_at(&format!("button:{id}")).is_some(), "{id} is a button on the fresh Sheet Metal tab");
        }
        h.step(&json!({"click":{"handle":"button:parameters.change"}})).unwrap();
        assert!(h.app.params.is_some(), "Parameters opens from the Sheet Metal tab");
        assert_eq!(h.app.ui.tab, "SHEET METAL", "and the tab stays");
        h.step(&json!({"click":{"handle":"params:close"}})).unwrap();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.frames(2);
        assert_eq!(h.app.ui.tab, "SKETCH");
        for id in ["parameters.change", "config.table"] {
            assert!(crate::scenario::handle_at(&format!("button:{id}")).is_some(), "{id} is a button on the Sketch tab");
        }
        h.app.finish_sketch();
        h.step(&json!({"click":{"handle":"tab:MODEL"}})).unwrap();
        h.frames(2);
        h.step(&json!({"click":{"handle":"button:config.table"}})).unwrap();
        assert!(matches!(
            h.app.dialog.as_ref().map(|d| &d.kind),
            Some(crate::dialogs::Kind::Motion(crate::dialogs_motion::Mo::Configurations { .. }))
        ));
        assert_eq!(h.app.ui.tab, "MODEL", "Configurations opens from the Model tab without leaving it");
    }

    fn panel_ids(tab: &str, specs: &[&CommandSpec]) -> Vec<&'static str> {
        SECTIONS.iter().filter(|s| s.tab == tab).flat_map(|s| layout(tab, s.panel, specs).unwrap().0).map(|c| c.id).collect()
    }

    #[test]
    fn starts_palette_and_control_use_the_new_destinations() {
        let mut h = crate::scenario::Harness::new();
        // Parameters is also on MODEL, so a start from there stays put; from SURFACE it goes home.
        h.app.ui.tab = "SURFACE".into();
        h.frames(2);
        for (id, tab) in [("parameters.change", "MANAGE"), ("solid.box", "MODEL"), ("surface.patch", "SURFACE")] {
            h.step(&json!({"start":id})).unwrap();
            assert_eq!(h.app.ui.tab, tab, "{id}");
            h.step(&json!({"key":"Escape"})).unwrap();
        }
        h.step(&json!({"key":"S"})).unwrap();
        h.step(&json!({"text":"Change Parameters"})).unwrap();
        h.step(&json!({"key":"Enter"})).unwrap();
        assert_eq!(h.app.ui.tab, "MANAGE");
        h.step(&json!({"key":"Escape"})).unwrap();
        h.app.start("parameters.change");
        assert_eq!(h.app.ui.tab, "MANAGE");
        h.frames(2);
        for tab in TABS {
            assert!(crate::scenario::handle_at(&format!("tab:{tab}")).is_some(), "{tab}");
        }
    }

    #[test]
    fn parameters_and_configurations_open_separately() {
        use serde_json::json;
        let mut h = crate::scenario::Harness::new();
        let specs = solidcraft_engine::command_specs();
        let (commands, _) = layout("MANAGE", "CONFIGURATIONS", &specs).unwrap();
        assert!(commands.iter().any(|c| c.id == "config.configure"));
        h.step(&json!({"click":{"handle":"tab:MANAGE"}})).unwrap();
        h.step(&json!({"click":{"handle":"button:parameters.change"}})).unwrap();
        assert!(h.app.params.is_some(), "the ordinary Parameters button opens the editor");
        assert!(h.app.dialog.is_none());
        assert!(h.widget("User Parameters").is_some());
        assert!(h.widget("Model Parameters").is_some());
        // Exercise the inline draft row through the public add prompt and Enter.
        h.step(&json!({"click":{"handle":"dialog:table:user:add"}})).unwrap();
        let parameters = h.app.params.as_mut().unwrap();
        parameters.new_name = "width".into();
        parameters.new_expr = "25 mm".into();
        h.frames(3);
        h.step(&json!({"key":"Enter"})).unwrap();
        let values = h.app.session.execute("parameters.list", &json!({})).unwrap();
        assert!(values["parameters"].as_array().unwrap().iter().any(|p| p["name"] == "width" && p["value"] == 25.0));
        h.step(&json!({"click":{"handle":"params:close"}})).unwrap();
        assert!(h.app.params.is_none());
        h.step(&json!({"click":{"handle":"button:config.table"}})).unwrap();
        assert!(h.app.params.is_none());
        assert!(matches!(
            h.app.dialog.as_ref().map(|d| &d.kind),
            Some(crate::dialogs::Kind::Motion(crate::dialogs_motion::Mo::Configurations { .. }))
        ));
        assert!(h.widget("Add Column").is_some());
        assert!(h.widget("Add configuration").is_some());
    }

    /// Shared assembly, sheet and plastic commands remain reachable in their ribbon sections.
    #[test]
    fn ribbon_sections_keep_shared_commands_reachable() {
        let specs = solidcraft_engine::command_specs();
        let ids = |tab, panel| layout(tab, panel, &specs).unwrap().0.iter().map(|c| c.id).collect::<Vec<_>>();
        let joints = ids("ASSEMBLE", "JOINTS");
        assert_eq!(joints.first().copied(), Some("joint.create"));
        assert!(ids("ASSEMBLE", "MOTION").contains(&"joint.drive"));
        assert!(ids("ASSEMBLE", "COMPONENTS").contains(&"component.create"));
        let sheet = ids("SHEET METAL", "CREATE");
        assert_eq!(sheet.first().copied(), Some("sheet.flange"));
        assert!(ids("MODEL", "CREATE").contains(&"solid.extrude"));
        assert!(ids("SHEET METAL", "FLAT").contains(&"sheet.flat_pattern"));
        assert!(ids("SHEET METAL", "FLAT").contains(&"sheet.unfold"));
        assert!(ids("MANAGE", "RULES").contains(&"plastic.manage_rules"));
        let create = ids("MODEL", "CREATE");
        assert!(create.contains(&"plastic.boss") && create.contains(&"solid.rib") && create.contains(&"plastic.lip"));
        assert!(ids("MODEL", "MODIFY").contains(&"solid.shell"));
        assert_eq!(panels("MANAGE"), ["PARAMETERS", "CONFIGURATIONS", "MATERIALS", "RULES", "DESIGN"]);
        // The former sketch/solid INSPECT panels now share a global INSPECT tab.
        for (panel, commands) in [
            ("CHECK", vec!["inspect.interference", "inspect.draft", "inspect.accessibility"]),
            ("MEASURE", vec!["inspect.center_of_mass"]),
            (
                "SURFACE QUALITY",
                vec![
                    "inspect.zebra",
                    "inspect.curvature_map",
                    "inspect.environment_map",
                    "inspect.minimum_radius",
                    "inspect.isocurve",
                    "inspect.curvature_comb",
                ],
            ),
        ] {
            let actual = ids("INSPECT", panel);
            for id in commands {
                assert!(actual.contains(&id), "{panel}: {id}");
            }
        }
    }

    /// #1240: a saved part reaches an assembly from the ASSEMBLE panel (and the File menu) as a
    /// linked component, and a STEP file picked there is inserted as Insert STEP would.
    #[test]
    fn insert_design_is_on_the_assemble_panel_and_inserts_a_saved_part() {
        use serde_json::json;
        let specs = solidcraft_engine::command_specs();
        assert!(layout("ASSEMBLE", "COMPONENTS", &specs).unwrap().0.iter().any(|c| c.id == "component.insert_design"));
        let mut h = crate::scenario::Harness::new();
        let part = h.dir.join("part1.solidcraft");
        let mut s = solidcraft_engine::Session::default();
        s.execute("solid.box", &json!({"length": 10, "width": 20, "height": 30})).unwrap();
        s.execute("file.save", &json!({"path": part})).unwrap();
        s.execute("file.export", &json!({"path": h.dir.join("part2.step")})).unwrap();
        h.step(&json!({"choose_file": "part1.solidcraft"})).unwrap();
        h.step(&json!({"start": "component.insert_design"})).unwrap();
        h.frames(3);
        let links = h.app.session.execute("component.links", &json!({})).unwrap();
        assert_eq!(links["links"][0]["name"], "part1", "{links}");
        assert_eq!(h.app.session.world_state().bodies.len(), 1);
        h.step(&json!({"choose_file": "part2.step"})).unwrap();
        h.step(&json!({"start": "component.insert_design"})).unwrap();
        for _ in 0..600 {
            if h.app.session.world_state().bodies.len() == 2 {
                break;
            }
            h.frames(1);
        }
        assert_eq!(h.app.session.world_state().bodies.len(), 2, "the STEP file's body is inserted");
    }

    /// Every way into a sketch (New Sketch on the ribbon then a plane, a double-click in the
    /// timeline or the browser, Edit Sketch in the browser's menu) opens the SKETCH tab and looks
    /// at the sketch; Finish Sketch (palette or ribbon) lands on MODEL with the view left where it is
    /// and the sketch still listed, as before the ribbon redesign.
    #[test]
    fn entering_and_finishing_a_sketch_keeps_the_tab_view_and_history() {
        let top = json!({"camera": {"yaw_deg": 0, "pitch_deg": 90}});
        let expect = |h: &mut crate::scenario::Harness, e: &serde_json::Value, what: &str| {
            h.frames(3);
            crate::scenario::check(h, e).unwrap_or_else(|err| panic!("{what}: {err}"));
        };
        let mut h = crate::scenario::Harness::new();
        h.call("ui.view", json!({"view": "iso"}));
        h.frames(3);
        h.step(&json!({"click": {"handle": "button:sketch.create"}})).unwrap();
        h.step(&json!({"click": {"plane": "XY"}})).unwrap();
        expect(&mut h, &top, "New Sketch on XY");
        assert_eq!(h.app.ui.tab, "SKETCH");
        h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]})).unwrap();
        h.step(&json!({"click": {"handle": "sketch_palette:finish"}})).unwrap();
        expect(&mut h, &json!({"sketching": false, "errors": 0}), "Finish");
        expect(&mut h, &top, "Finish leaves the view where it is");
        assert_eq!(h.app.ui.tab, "MODEL");
        assert!(crate::scenario::handle_at("timeline:Sketch1").is_some() && crate::scenario::handle_at("row:Sketch1").is_some());

        let entries = [
            ("timeline double-click", vec![json!({"click": {"handle": "timeline:Sketch1"}, "double": true})]),
            ("browser double-click", vec![json!({"click": {"handle": "row:Sketch1"}, "double": true})]),
            (
                "browser Edit Sketch",
                vec![json!({"click": {"handle": "row:Sketch1"}, "button": "right"}), json!({"click": {"handle": "menu:ui.editSketch"}})],
            ),
        ];
        for (how, steps) in entries {
            for s in steps {
                h.frames(3);
                h.step(&s).unwrap_or_else(|e| panic!("{how}: {e}"));
            }
            expect(&mut h, &json!({"sketching": true}), how);
            expect(&mut h, &top, how);
            assert_eq!(h.app.ui.tab, "SKETCH", "{how}");
            h.step(&json!({"click": {"handle": "button:sketch.finish"}})).unwrap();
            expect(&mut h, &json!({"sketching": false}), how);
            expect(&mut h, &top, how);
            assert_eq!(h.app.ui.tab, "MODEL", "{how}");
        }
    }

    /// Escape and Enter, pressed one at a time, never leave a sketch (#1804); Undo right after
    /// Finish reopens it, and a second Undo removes it and leaves the SKETCH tab.
    #[test]
    fn keys_keep_a_sketch_open_and_undo_after_finish_reopens_it() {
        let mut h = crate::scenario::Harness::new();
        h.frames(3);
        h.step(&json!({"start": "sketch.create"})).unwrap();
        h.step(&json!({"click": {"plane": "XY"}})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]})).unwrap();
        for key in [json!({"key": "Escape"}), json!({"key": "Escape"}), json!({"key": "Enter"}), json!({"key": "Enter", "cmd": true})] {
            h.step(&key).unwrap();
            // Well apart, so two presses are never a quick double press.
            h.frames(60);
            assert!(h.app.session.active_sketch.is_some(), "{key}");
            assert_eq!(h.app.ui.tab, "SKETCH", "{key}");
        }
        h.step(&json!({"click": {"handle": "sketch_palette:finish"}})).unwrap();
        h.frames(2);
        h.step(&json!({"key": "Z", "cmd": true})).unwrap();
        h.frames(2);
        assert!(h.app.session.active_sketch.is_some(), "Undo after Finish reopens the sketch");
        assert_eq!(h.app.ui.tab, "SKETCH");
        h.step(&json!({"key": "Z", "cmd": true})).unwrap();
        h.frames(2);
        assert!(h.app.session.active_sketch.is_none());
        assert_eq!(h.app.ui.tab, "MODEL");
    }

    /// New Sketch is on the SHEET METAL tab, as it was before the ribbon redesign, and a sketch
    /// started there stays on its tab until it opens.
    #[test]
    fn new_sketch_starts_from_the_sheet_metal_tab() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("sheet.flange", json!({"name": "Sheet", "body_name": "Sheet"})).unwrap();
        h.frames(3);
        assert_eq!(h.app.ui.tab, "SHEET METAL");
        h.step(&json!({"click": {"handle": "button:sketch.create"}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.ui.tab, "SHEET METAL", "New Sketch is on this tab, so it stays");
        h.step(&json!({"click": {"plane": "XZ"}})).unwrap();
        h.frames(2);
        assert!(h.app.session.active_sketch.is_some());
        assert_eq!(h.app.ui.tab, "SKETCH");
    }

    /// New Sketch is on the SURFACE tab too, and a sketch started there stays on that tab until
    /// it opens.
    #[test]
    fn new_sketch_starts_from_the_surface_tab() {
        let mut h = crate::scenario::Harness::new();
        h.step(&json!({"click": {"handle": "tab:SURFACE"}})).unwrap();
        h.frames(2);
        h.step(&json!({"click": {"handle": "button:sketch.create"}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.ui.tab, "SURFACE", "New Sketch is on this tab, so it stays");
        h.step(&json!({"click": {"plane": "XY"}})).unwrap();
        h.frames(2);
        assert!(h.app.session.active_sketch.is_some());
        assert_eq!(h.app.ui.tab, "SKETCH");
    }

    /// Move 3D Point stays for agents, MCP and scripts; people move a drawn 3D point with the
    /// triad that appears when it is clicked, not a ribbon button.
    #[test]
    fn move3d_is_agent_only() {
        assert!(AGENT_ONLY.contains(&"sketch.move3d"), "sketch.move3d must stay available to agents");
        for section in SECTIONS.iter().filter(|s| s.panel == "SKETCH") {
            assert!(!section.ids.contains(&"sketch.move3d"), "{}/{} must not offer sketch.move3d on the ribbon", section.tab, section.panel);
        }
    }
}
