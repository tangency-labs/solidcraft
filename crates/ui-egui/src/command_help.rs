//! Written help for the ribbon tooltips (#2758): two short sentences per command, shown under the
//! tool name when the pointer rests on its button. The names stay as they are; the help is a
//! separate message `help.<command id>` in the catalogs, so a tool without help keeps its plain
//! tooltip. Text only for now; pictures and animations are a later step. A new toolbar command
//! ships with its help: `every_toolbar_command_has_help` fails otherwise.

use crate::i18n;

/// The commands that have help text. Each one needs a `help.<id>` message in the catalogs.
pub const DOCUMENTED: &[&str] = &[
    "appearance.assign",
    "assembly.bom",
    "canvas.decal",
    "canvas.edit",
    "canvas.insert",
    "component.break_link",
    "component.capture_position",
    "component.create",
    "component.finish_edit",
    "component.from_bodies",
    "component.insert_design",
    "component.properties",
    "component.revert_position",
    "component.update_links",
    "config.configure",
    "config.preview",
    "config.rules",
    "config.table",
    "construct.axis.edge",
    "construct.axis.normal_to_face",
    "construct.axis.through_cylinder",
    "construct.axis.two_planes",
    "construct.axis.two_points",
    "construct.plane.along_path",
    "construct.plane.at_angle",
    "construct.plane.axis_point",
    "construct.plane.midplane",
    "construct.plane.offset",
    "construct.plane.perpendicular",
    "construct.plane.tangent",
    "construct.plane.three_points",
    "construct.plane.two_edges",
    "construct.point.along_path",
    "construct.point.center",
    "construct.point.edge_plane",
    "construct.point.three_planes",
    "construct.point.two_edges",
    "construct.point.vertex",
    "contact.check",
    "contact.create",
    "contact.disable_all",
    "contact.edit",
    "contact.enable_all",
    "contact.enable_sets",
    "contact.list",
    "document.inspect",
    "document.units",
    "edit.checkpoint",
    "edit.restore_checkpoint",
    "explode.create",
    "face.delete",
    "file.insert_mesh",
    "file.insert_step",
    "file.save_mesh",
    "file.send_to_slicer",
    "frame.end_cap",
    "frame.member",
    "inspect.accessibility",
    "inspect.center_of_mass",
    "inspect.colored_components",
    "inspect.compare",
    "inspect.continuity",
    "inspect.curvature_comb",
    "inspect.curvature_map",
    "inspect.draft",
    "inspect.environment_map",
    "inspect.interference",
    "inspect.isocurve",
    "inspect.measure",
    "inspect.minimum_radius",
    "inspect.section",
    "inspect.zebra",
    "joint.as_built",
    "joint.cam",
    "joint.create",
    "joint.drive",
    "joint.gear",
    "joint.limits",
    "joint.motion_link",
    "joint.origin",
    "joint.rack_pinion",
    "joint.reach",
    "joint.rigid_group",
    "joint.screw",
    "library.apply",
    "library.export",
    "library.inspect",
    "material.assign",
    "material.properties.validate",
    "mesh.convert",
    "motion.study",
    "motion.trace",
    "occurrence.ground",
    "occurrence.move",
    "parameters.change",
    "parameters.export",
    "parameters.graph",
    "parameters.import",
    "parameters.move",
    "parts.fastener",
    "parts.insert",
    "parts.library",
    "parts.resize",
    "plastic.assign_rule",
    "plastic.boss",
    "plastic.lip",
    "plastic.manage_rules",
    "plastic.rest",
    "plastic.snap_fit",
    "pointcloud.insert",
    "sheet.convert",
    "sheet.export_bend_table",
    "sheet.export_dxf",
    "sheet.flange",
    "sheet.flat_pattern",
    "sheet.fold",
    "sheet.hem",
    "sheet.manage_rules",
    "sheet.path_flange",
    "sheet.perforate",
    "sheet.refold",
    "sheet.unfold",
    "sketch.arc.center_point",
    "sketch.arc.tangent",
    "sketch.arc.three_point",
    "sketch.arc3d",
    "sketch.auto_constrain",
    "sketch.auto_constrain_from_datum",
    "sketch.blend_curve",
    "sketch.break",
    "sketch.cancel",
    "sketch.centerline",
    "sketch.chamfer.distance_angle",
    "sketch.chamfer.equal_distance",
    "sketch.chamfer.two_distance",
    "sketch.circle.center",
    "sketch.circle.three_point",
    "sketch.circle.three_tangent",
    "sketch.circle.two_point",
    "sketch.circle.two_tangent",
    "sketch.conic",
    "sketch.constrainer",
    "sketch.constraint.coincident",
    "sketch.constraint.collinear",
    "sketch.constraint.concentric",
    "sketch.constraint.curvature",
    "sketch.constraint.equal",
    "sketch.constraint.fix",
    "sketch.constraint.horizontal_vertical",
    "sketch.constraint.midpoint",
    "sketch.constraint.parallel",
    "sketch.constraint.perpendicular",
    "sketch.constraint.polygon",
    "sketch.constraint.symmetry",
    "sketch.constraint.tangent",
    "sketch.construction",
    "sketch.construction_line",
    "sketch.construction_line.angle",
    "sketch.construction_line.bisector",
    "sketch.construction_line.horizontal",
    "sketch.construction_line.parallel",
    "sketch.construction_line.perpendicular",
    "sketch.construction_line.tangent_at",
    "sketch.construction_line.tangent_external",
    "sketch.construction_line.tangent_two",
    "sketch.construction_line.vertical",
    "sketch.copy",
    "sketch.create",
    "sketch.cut",
    "sketch.dimension",
    "sketch.edit",
    "sketch.ellipse",
    "sketch.equation_curve",
    "sketch.extend",
    "sketch.fillet",
    "sketch.finish",
    "sketch.finish_auto_constrain",
    "sketch.fit_curves_to_section",
    "sketch.include_3d",
    "sketch.infinite",
    "sketch.insert_dxf",
    "sketch.insert_svg",
    "sketch.inspect",
    "sketch.intersect",
    "sketch.intersection_curve",
    "sketch.isoparametric_curve",
    "sketch.line",
    "sketch.line.midpoint",
    "sketch.line3d",
    "sketch.mirror",
    "sketch.move",
    "sketch.offset",
    "sketch.paste",
    "sketch.pattern.circular",
    "sketch.pattern.rectangular",
    "sketch.point",
    "sketch.point3d",
    "sketch.polygon.circumscribed",
    "sketch.polygon.edge",
    "sketch.polygon.inscribed",
    "sketch.project",
    "sketch.project_to_surface",
    "sketch.rectangle.center",
    "sketch.rectangle.rounded",
    "sketch.rectangle.three_point",
    "sketch.rectangle.two_point",
    "sketch.scale",
    "sketch.slot.arc_center",
    "sketch.slot.arc_three_point",
    "sketch.slot.center_point",
    "sketch.slot.center_to_center",
    "sketch.slot.overall",
    "sketch.spline.control_point",
    "sketch.spline.control_point_5",
    "sketch.spline.fit_point",
    "sketch.spline3d",
    "sketch.spun_profile",
    "sketch.text",
    "sketch.transform",
    "sketch.trim",
    "solid.align",
    "solid.boundary_fill",
    "solid.bounding_solid",
    "solid.box",
    "solid.chamfer",
    "solid.coil",
    "solid.combine",
    "solid.cone",
    "solid.cylinder",
    "solid.draft",
    "solid.emboss",
    "solid.extrude",
    "solid.fillet",
    "solid.hole",
    "solid.loft",
    "solid.mirror",
    "solid.move",
    "solid.offset_face",
    "solid.pattern.circular",
    "solid.pattern.path",
    "solid.pattern.rectangular",
    "solid.pipe",
    "solid.press_pull",
    "solid.remove",
    "solid.replace_face",
    "solid.revolve",
    "solid.rib",
    "solid.scale",
    "solid.shell",
    "solid.sphere",
    "solid.split_body",
    "solid.split_face",
    "solid.sweep",
    "solid.thread",
    "solid.torus",
    "solid.web",
    "surface.blend",
    "surface.boundary",
    "surface.extend",
    "surface.extrude",
    "surface.fair",
    "surface.loft",
    "surface.match",
    "surface.offset",
    "surface.patch",
    "surface.reverse_normal",
    "surface.revolve",
    "surface.ruled",
    "surface.stitch",
    "surface.sweep",
    "surface.thicken",
    "surface.trim",
    "surface.unstitch",
    "surface.untrim",
    "timeline.compute_all",
    "timeline.delete",
    "timeline.dependents",
    "timeline.play",
    "view.apply",
    "view.delete",
    "view.fit",
    "view.grid",
    "view.ground_shadow",
    "view.home",
    "view.list",
    "view.navigate",
    "view.navigation_bar",
    "view.rename",
    "view.save",
    "view.spin",
    "view.tool",
    "view.visual_style",
];

/// The help for `command` in the current language, when it has any.
pub fn help(command: &str) -> Option<String> {
    if !DOCUMENTED.contains(&command) {
        return None;
    }
    let key = format!("help.{command}");
    let text = i18n::tr(&key);
    (text != key).then(|| text.to_owned())
}

/// Show `tip` (name, shortcut or the reason the tool is disabled) as the tooltip of `response`,
/// with the help of `command` below it when there is one.
pub fn tooltip(response: egui::Response, command: &str, tip: String) -> egui::Response {
    let help = help(command);
    response.on_hover_ui(move |ui| {
        ui.set_max_width(300.0);
        ui.label(tip);
        if let Some(help) = help {
            ui.add_space(2.0);
            ui.label(egui::RichText::new(help).weak());
        }
    })
}

/// The tooltip text of a panel drop-down row: the reason when the command is disabled (as before),
/// else its name and shortcut when it has help to show below, else nothing (no tooltip).
pub fn row_tip(command: &str, label: &str, shortcut: Option<&str>, disabled_reason: Option<&str>) -> Option<String> {
    match disabled_reason {
        Some(why) => Some(why.to_owned()),
        None => help(command).map(|_| format!("{label}{}", shortcut.map(|k| format!("  ({k})")).unwrap_or_default())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn catalog(source: &str) -> std::collections::BTreeMap<String, String> {
        serde_json::from_str(source).unwrap()
    }

    /// The `help.<command id>` messages; other `help.*` messages (About dialog texts) are not tool help.
    fn help_keys(source: &str) -> BTreeSet<String> {
        catalog(source)
            .into_keys()
            .filter(|k| k.starts_with("help.") && (k.starts_with("help.solid.") || DOCUMENTED.contains(&&k["help.".len()..])))
            .collect()
    }

    /// Every command the ribbon offers people, gathered the way the toolbar lays out its panels. A
    /// planned command only says "not supported yet"; it gets its help when it is built.
    fn toolbar_commands() -> BTreeSet<&'static str> {
        let specs = solidcraft_engine::command_specs();
        let mut ids = BTreeSet::new();
        for section in crate::workspace::SECTIONS {
            if let Some((commands, _)) = crate::workspace::layout(section.tab, section.panel, &specs) {
                ids.extend(commands.iter().filter(|c| c.planned.is_none() && !crate::workspace::AGENT_ONLY.contains(&c.id)).map(|c| c.id));
            }
        }
        ids
    }

    #[test]
    fn every_toolbar_command_has_help() {
        let missing: Vec<_> = toolbar_commands().into_iter().filter(|id| !DOCUMENTED.contains(id)).collect();
        assert!(
            missing.is_empty(),
            "toolbar commands without help: {missing:?}. Add `help.<id>` (two short sentences: what the tool does, \
             then when to use it) to a file in assets/locales/pending/ and the id to DOCUMENTED."
        );
    }

    #[test]
    fn every_documented_id_is_a_real_command() {
        for id in DOCUMENTED {
            assert!(solidcraft_engine::find_command(id).is_some(), "{id} is not a command");
        }
        assert!(DOCUMENTED.is_sorted());
    }

    #[test]
    fn every_help_message_is_for_a_documented_command() {
        let english = help_keys(include_str!(concat!(env!("OUT_DIR"), "/locales/en.json")));
        let wanted: BTreeSet<String> = DOCUMENTED.iter().map(|id| format!("help.{id}")).collect();
        assert_eq!(english, wanted);
        for key in &english {
            let id = key.trim_start_matches("help.");
            assert!(solidcraft_engine::find_command(id).is_some(), "{key} names no command");
        }
    }

    #[test]
    fn the_help_texts_are_written_in_every_complete_language() {
        let english = catalog(include_str!(concat!(env!("OUT_DIR"), "/locales/en.json")));
        for source in [include_str!(concat!(env!("OUT_DIR"), "/locales/ja.json")), include_str!(concat!(env!("OUT_DIR"), "/locales/zh.json"))] {
            let other = catalog(source);
            for (key, text) in english.iter().filter(|(k, _)| k.starts_with("help.")) {
                let translated = other.get(key).unwrap_or_else(|| panic!("{key} missing"));
                assert!(!translated.trim().is_empty());
                // The messages carry no placeholders; if one is added, both sides must agree.
                assert_eq!(text.matches('{').count(), translated.matches('{').count(), "{key}");
            }
        }
    }

    #[test]
    fn a_documented_command_has_help_and_others_do_not() {
        for id in DOCUMENTED {
            assert!(help(id).is_some_and(|text| !text.is_empty()), "{id}");
        }
        assert!(help("sketch.drag").is_none());
    }

    #[test]
    fn a_drop_down_row_has_a_tip_only_for_a_reason_or_help() {
        assert_eq!(row_tip("solid.shell", "Shell", Some("S"), None).as_deref(), Some("Shell  (S)"));
        assert_eq!(row_tip("solid.hole", "Hole", None, None).as_deref(), Some("Hole"));
        assert_eq!(row_tip("solid.hole", "Hole", None, Some("Select a face")).as_deref(), Some("Select a face"));
        assert_eq!(row_tip("sketch.drag", "Drag Sketch Geometry", None, None), None);
        assert_eq!(row_tip("sketch.drag", "Drag Sketch Geometry", None, Some("No body")).as_deref(), Some("No body"));
    }
}
