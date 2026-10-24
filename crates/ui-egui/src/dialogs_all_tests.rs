//! Each ribbon feature form keeps its parameter builder while sharing the dense layout.
use super::*;
use crate::scenario::{self, Harness};

fn fixture() -> Harness {
    let mut h = Harness::new();
    for (name, x) in [("A", 0), ("B", 30), ("C", 60), ("D", 90)] {
        h.app.run("component.activate", json!({"component":"root"})).unwrap();
        h.app.run("component.create", json!({"name":name})).unwrap();
        h.app.run("solid.cylinder", json!({"base":[x,0,0],"radius":10,"height":5})).unwrap();
    }
    h.app.run("component.activate", json!({"component":"root"})).unwrap();
    h.app.run("occurrence.ground", json!({"occurrence":"A:1","grounded":true})).unwrap();
    for (name, kind) in [("B", "revolute"), ("C", "cylindrical"), ("D", "slider")] {
        h.app.run("joint.as_built", json!({"name":name,"type":kind,"a":"A","b":name})).unwrap();
    }
    for (name, offset) in [("Base", 0), ("Top", 10)] {
        h.app.run("sketch.create", json!({"plane":"XY","offset":offset,"name":name})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[12,0],"p1":[16,4]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
    }
    h.app.run("sketch.create", json!({"plane":"XZ","name":"Path"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,0],[0,20]]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h
}

fn representative(h: &Harness, command: &str) -> Dialog {
    let mut d = Dialog::for_command(&h.app, command).unwrap();
    let s = &h.app.session;
    let profiles: Vec<_> = s.world_state().sketches.iter().take(2).map(|sk| Sel::Profile { sketch: sk.feature, index: 0 }).collect();
    let path = s.world_state().sketches.last().unwrap().sketch.curves.first().unwrap().id.clone();
    for (index, input) in d.inputs.iter_mut().enumerate() {
        if command == "joint.gear" {
            continue;
        }
        if matches!(input.label, "Rail" | "Second Rail" | "Pass through" | "Direction" | "Roller (optional)" | "Tip (optional)") {
            continue;
        }
        let name = if index == 1 && command != "surface.ruled" { "Body2" } else { "Body1" };
        let point = if index == 1 && command != "surface.ruled" { Vec3::new(30.0, 0.0, 5.0) } else { Vec3::new(0.0, 0.0, 5.0) };
        input.items = if command == "construct.point.along_path" {
            vec![edge_sel(s, Vec3::new(10.0, 0.0, 5.0)).unwrap()]
        } else if input.accept & PROFILES != 0 {
            if matches!(d.kind, Kind::Loft { .. } | Kind::Surface(crate::dialogs_surface::Surf::Loft { .. })) {
                profiles.clone()
            } else {
                vec![profiles[0].clone()]
            }
        } else if input.accept & BODIES != 0 {
            if matches!(command, "joint.as_built" | "joint.rigid_group") {
                vec![Sel::Body { name: "Body1".into() }, Sel::Body { name: "Body2".into() }]
            } else {
                vec![Sel::Body { name: name.into() }]
            }
        } else if input.accept & PLANES != 0 {
            vec![Sel::Plane { name: if index == 1 { "XZ" } else { "XY" }.into() }]
        } else if input.accept & crate::selection::SKETCH_LINES != 0 {
            vec![Sel::ExternalSketchCurve { sketch: s.world_state().sketches.last().unwrap().feature, id: path.clone() }]
        } else if input.accept & CURVES != 0 || command == "sheet.fold" && index == 0 {
            vec![Sel::SketchCurve { id: path.clone() }]
        } else if input.accept & AXES != 0 {
            vec![Sel::Axis { name: "Z".into() }]
        } else if input.accept & (FACES | PLANAR_FACES) != 0 {
            vec![face_sel(s, point).unwrap()]
        } else if input.accept & EDGES != 0 {
            vec![edge_sel(s, Vec3::new(10.0, 0.0, 5.0)).unwrap()]
        } else {
            Vec::new()
        };
    }
    if let Kind::PathPlane { sketch, .. } = &mut d.kind {
        *sketch = Some(s.world_state().sketches.last().unwrap().feature);
    }
    if let Kind::Mirror = &d.kind {
        d.extra.insert("combine".into(), json!(false));
    }
    if let Kind::BoundaryFill { cells, .. } = &mut d.kind {
        cells.push((Vec3::new(0.0, 0.0, 2.0), "Cell".into(), true));
    }
    if let Kind::Thread { designation, .. } = &mut d.kind {
        *designation = "M20x2.5".into();
    }
    if let Kind::Assembly(crate::dialogs_assembly::Asm::Cam(f)) = &mut d.kind {
        f.b = s.doc.assembly.joints[2].id;
    }
    d
}

fn check_form(command: &str, labels: &[&str]) {
    let mut h = fixture();
    let mut d = representative(&h, command);
    assert_eq!(super::command(&d.kind), Some(command));
    let spec = solidcraft_engine::find_command(command).unwrap();
    assert!(!spec.icon.is_empty());
    let expected = apply_commands(&h.app, &d).unwrap_or_else(|e| panic!("{command}: representative parameters: {e}"));
    // Rendering must never discard an optional/default value or add a new parameter.
    for _ in 0..3 {
        scenario::clear_handles();
        let mut out = h.ctx.run_ui(
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(320.0, 1600.0))), ..Default::default() },
            |ui| {
                ui.set_width(264.0);
                ui_kit::scope(ui, |ui| {
                    ui_kit::header(ui, spec.icon, tr(spec.label), || {});
                    rows(&mut h.app, &h.ctx, &mut d, ui);
                    ui_kit::footer(ui, "OK", "Cancel", true, ui_kit::Footer::default());
                });
            },
        );
        out.textures_delta.clear();
        h.output = out.platform_output;
    }
    let nodes = &h.output.accesskit_update.as_ref().unwrap().nodes;
    for input in &d.inputs {
        assert!(scenario::rect_of(&format!("dialog:field:{}", input.label)).is_some(), "{command}: selection field {}", input.label);
        assert!(
            nodes.iter().any(|(_, n)| n.role() == egui::accesskit::Role::Button && n.label() == Some(input.label)),
            "{command}: picker {} is a button",
            input.label
        );
    }
    for label in labels {
        if let Some(role) = control_role(command, label) {
            if *label == "Animate" || *label == "Radius" && command == "joint.rack_pinion" {
                let control = scenario::rect_of(&format!("dialog:control:{label}")).unwrap();
                assert!(
                    nodes.iter().any(|(_, n)| n.role() == role
                        && n.bounds().is_some_and(|b| control.contains(egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32)))),
                    "{command}: {label} uses {role:?}"
                );
                continue;
            }
            assert!(nodes.iter().any(|(_, n)| n.role() == role && n.label() == Some(tr(label))), "{command}: {label} uses {role:?}");
        }
    }
    let checks: &[&str] = match command {
        "solid.shell" => &["Tangent Chain"],
        "solid.rib" | "solid.web" | "solid.align" | "sheet.flange" | "sheet.hem" | "sheet.fold" | "joint.create" => &["Flip"],
        "inspect.section" => &["Flip", "Hatch"],
        "solid.move" => &["Create Copy"],
        "solid.combine" => &["Keep Tools", "New Component"],
        "solid.thread" => &["Modeled", "Full Length", "Automatic size"],
        "solid.boundary_fill" => &["Remove Tools"],
        "surface.offset" => &["Tangent chain"],
        "surface.trim" => &["Extend to meet", "Stitch the kept pieces"],
        "surface.ruled" => &["Flip"],
        "surface.untrim" => &["Restore outer boundary", "In place (surfaces)"],
        "component.create" => &["Activate"],
        "joint.gear" => &["Reverse", "Internal gear"],
        "joint.rack_pinion" | "joint.screw" => &["Reverse"],
        "joint.cam" => &["Other side"],
        "frame.member" => &["All Lines", "Mirror"],
        _ => &[],
    };
    for label in checks {
        assert!(nodes.iter().any(|(_, n)| n.role() == egui::accesskit::Role::CheckBox && n.label() == Some(*label)), "{command}: option {label}");
    }
    assert_eq!(scenario::count_of("dialog:ok:enabled"), Some(1.0), "{command}");
    assert_eq!(apply_commands(&h.app, &d).unwrap(), expected, "{command}: drawing is lossless");
    let mut label_width: Option<f32> = None;
    for label in d.inputs.iter().map(|i| i.label).chain(labels.iter().copied()) {
        let row = scenario::rect_of(&format!("dialog:row:{label}")).unwrap_or_else(|| panic!("{command}: missing dense row {label}"));
        let label_rect = scenario::rect_of(&format!("dialog:label:{label}")).unwrap();
        let control = scenario::rect_of(&format!("dialog:control:{label}")).unwrap();
        assert_eq!(row.height(), Tokens::get().control_height, "{command}: {label}");
        // The label column is the design's width, widened to the longest label, equally for every row.
        let t = Tokens::get();
        assert!(label_rect.width() >= t.form_label_width && label_rect.width() <= t.form_label_width * 1.6, "{command}: {label}");
        if let Some(first) = label_width {
            assert_eq!(label_rect.width(), first, "{command}: {label} lines up with the other rows");
        }
        label_width = Some(label_rect.width());
        assert!(row.contains_rect(control), "{command}: {label}");
        assert!(scenario::handle_at(&format!("dialog:{label}")).is_some() || scenario::rect_of(&format!("dialog:control:{label}")).is_some());
    }
    assert!(scenario::rect_of("dialog:footer").is_some());
    assert!(scenario::handle_at("dialog:ok").is_some());
    assert!(scenario::handle_at("dialog:cancel").is_some());
    let mut out = h.ctx.run_ui(egui::RawInput::default(), |ui| {
        ui_kit::scope(ui, |ui| {
            assert_eq!(ui_kit::footer(ui, "OK", "Cancel", false, ui_kit::Footer::default()), ui_kit::Action::None);
        });
    });
    out.textures_delta.clear();
    assert_eq!(scenario::count_of("dialog:ok:enabled"), Some(0.0), "{command}");
}

fn control_role(command: &str, label: &str) -> Option<egui::accesskit::Role> {
    use egui::accesskit::Role;
    Some(match label {
        "Type" if matches!(command, "sheet.flange" | "sheet.hem" | "solid.revolve" | "solid.emboss") => return None,
        "Direction" => return None, // segmented buttons, labelled by each choice
        "Continuity" | "Corners" | "Cells" | "Profile" => return None,
        "Operation" if command == "solid.mirror" => return None,
        "Operation" | "Type" | "Orientation" | "Move Type" | "Object Type" | "Rule" | "Rim Edge" | "Catch Direction" | "Shape" | "Holes" | "Size"
        | "Bend Line Position" | "From Snap" | "To Snap" | "Joint" | "Joint 1" | "Joint 2" | "Gear 1" | "Gear 2" | "Pinion" | "Rack" | "Screw"
        | "Nut" | "Cam" | "Follower" | "Anchor" => Role::ComboBox,
        "Radius" if command == "joint.rack_pinion" => Role::Label,
        "Pitch" if command == "solid.thread" => Role::ComboBox,
        "Automatic size" | "Reverse" | "Other side" | "All Lines" | "Mirror" => Role::CheckBox,
        "Animate" => Role::Button,
        "Ratio" if command.starts_with("joint.") && command != "joint.motion_link" => return None,
        _ => Role::TextInput,
    })
}

macro_rules! form_case {
    ($name:ident, $command:literal, [$($label:literal),*]) => {
        #[test] fn $name() { check_form($command, &[$($label),*]); }
    };
}
form_case!(revolve, "solid.revolve", ["Angle", "Direction", "Type"]);
form_case!(shell, "solid.shell", ["Thickness", "Direction"]);
form_case!(draft, "solid.draft", ["Angle"]);
form_case!(mirror, "solid.mirror", ["Operation"]);
form_case!(loft, "solid.loft", ["Operation"]);
form_case!(sweep, "solid.sweep", ["Orientation"]);
form_case!(offset_plane, "construct.plane.offset", ["Distance"]);
form_case!(midplane, "construct.plane.midplane", []);
form_case!(mesh_convert, "mesh.convert", ["Tolerance"]);
form_case!(axis_two_planes, "construct.axis.two_planes", []);
form_case!(point_along_path, "construct.point.along_path", ["Position (0 to 1)"]);
form_case!(angle_plane, "construct.plane.at_angle", ["Angle"]);
form_case!(path_plane, "construct.plane.along_path", ["Position (0 to 1)"]);
form_case!(split, "solid.split_body", []);
form_case!(scale, "solid.scale", ["Scale factor"]);
form_case!(offset_faces, "solid.offset_face", ["Distance"]);
form_case!(section, "inspect.section", ["Distance"]);
form_case!(move_copy, "solid.move", ["Move Type", "X Distance", "Y Distance", "Z Distance", "X Angle", "Y Angle", "Z Angle"]);
form_case!(rib, "solid.rib", ["Direction", "Thickness", "Depth"]);
form_case!(web, "solid.web", ["Direction", "Thickness", "Depth"]);
form_case!(emboss, "solid.emboss", ["Depth", "Type"]);
form_case!(replace_face, "solid.replace_face", []);
form_case!(boundary_fill, "solid.boundary_fill", ["Cells", "Operation"]);
form_case!(align, "solid.align", ["From Snap", "To Snap"]);
form_case!(remove, "solid.remove", []);
form_case!(path_pattern, "solid.pattern.path", ["Type", "Object Type", "Quantity", "Spacing"]);
form_case!(pipe, "solid.pipe", ["Diameter", "Wall"]);
form_case!(stock, "solid.bounding_solid", ["Margin"]);
form_case!(frame_member, "frame.member", ["All Lines", "Anchor", "Angle", "Mirror", "Name"]);
form_case!(thread, "solid.thread", ["Profile", "Size", "Pitch", "Designation"]);
form_case!(primitive_box, "solid.box", ["length", "width", "height", "Operation"]);
form_case!(cylinder, "solid.cylinder", ["diameter", "height", "Operation"]);
form_case!(sphere, "solid.sphere", ["diameter", "Operation"]);
form_case!(torus, "solid.torus", ["major", "minor", "Operation"]);
form_case!(coil, "solid.coil", ["diameter", "revolutions", "pitch", "section_size", "Operation"]);
form_case!(combine, "solid.combine", ["Operation"]);
form_case!(surface_extrude, "surface.extrude", ["Distance", "Direction"]);
form_case!(surface_revolve, "surface.revolve", ["Angle", "Direction"]);
form_case!(surface_sweep, "surface.sweep", ["Orientation"]);
form_case!(surface_loft, "surface.loft", []);
form_case!(surface_patch, "surface.patch", ["Continuity"]);
form_case!(surface_unstitch, "surface.unstitch", []);
form_case!(surface_reverse, "surface.reverse_normal", []);
form_case!(surface_offset, "surface.offset", ["Distance"]);
form_case!(surface_ruled, "surface.ruled", ["Distance", "Type", "Corners"]);
form_case!(surface_stitch, "surface.stitch", ["Tolerance"]);
form_case!(thicken, "surface.thicken", ["Thickness", "Direction", "Operation"]);
form_case!(surface_extend, "surface.extend", ["Distance"]);
form_case!(surface_trim, "surface.trim", []);
form_case!(surface_untrim, "surface.untrim", ["Holes"]);
form_case!(split_face, "solid.split_face", []);
form_case!(flange, "sheet.flange", ["Type", "Rule"]);
form_case!(hem, "sheet.hem", ["Type", "Length", "ui.sheet_gap"]);
form_case!(fold, "sheet.fold", ["Bend Line Position", "Angle", "Bend Radius"]);
form_case!(unfold, "sheet.unfold", []);
form_case!(refold, "sheet.refold", []);
form_case!(sheet_convert, "sheet.convert", ["Rule"]);
form_case!(boss, "plastic.boss", ["Diameter", "Height", "Hole Diameter", "Hole Depth"]);
form_case!(lip, "plastic.lip", ["Type", "Rim Edge", "Width", "Height"]);
form_case!(snap_fit, "plastic.snap_fit", ["Catch Direction", "Length", "Thickness", "Width", "Catch Depth", "Catch Length"]);
form_case!(rest, "plastic.rest", ["Shape", "Diameter", "Height"]);
form_case!(plastic_assign, "plastic.assign_rule", ["Rule"]);
form_case!(component, "component.create", ["Name"]);
form_case!(joint, "joint.create", ["Type", "Angle"]);
form_case!(as_built, "joint.as_built", ["Type"]);
form_case!(joint_origin, "joint.origin", ["Name"]);
form_case!(rigid_group, "joint.rigid_group", []);
form_case!(motion_link, "joint.motion_link", ["Joint 1", "Joint 2", "Ratio", "Offset"]);
form_case!(gear, "joint.gear", ["Gear 1", "Radius 1", "Gear 2", "Radius 2", "Ratio", "Animate"]);
form_case!(rack, "joint.rack_pinion", ["Pinion", "Radius", "Rack", "Ratio", "Animate"]);
form_case!(screw, "joint.screw", ["Screw", "Nut", "Pitch", "Ratio", "Animate"]);
form_case!(cam, "joint.cam", ["Cam", "Follower", "Type", "Animate"]);
form_case!(trace, "motion.trace", ["Joint", "From", "To", "Samples"]);

#[test]
fn registry_dialog_inventory_has_no_unclassified_feature_forms() {
    let h = fixture();
    for spec in solidcraft_engine::command_specs() {
        let Some(d) = Dialog::for_command(&h.app, spec.id) else { continue };
        assert!(super::command(&d.kind).is_some(), "{}: {:?}", spec.id, d.kind);
    }
}

#[test]
fn new_cards_settle_before_the_first_click_in_both_layouts() {
    for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
        for command in ["solid.box", "solid.revolve", "solid.shell", "joint.create"] {
            let mut h = fixture();
            h.call("ui.set", json!({"layout":mode}));
            h.call("ui.resize", json!({"width":1400,"height":950}));
            h.app.dialog = Some(representative(&h, command));
            h.frames(2);
            let first = scenario::rect_of("dialog:footer").unwrap();
            h.frames(1);
            let settled = scenario::rect_of("dialog:footer").unwrap();
            assert_eq!(first, settled, "{command} {mode:?}: footer moved under the first click");
        }
    }
}

#[test]
fn remaining_dialogs_keep_kit_chrome_and_cancel_restores_preview() {
    for command in [
        "contact.create",
        "motion.study",
        "explode.create",
        "parts.insert",
        "joint.drive",
        "inspect.interference",
        "file.save_mesh",
        "file.export_view_dxf",
        "sheet.flat_pattern",
    ] {
        let mut h = fixture();
        h.call("ui.resize", json!({"width":1600,"height":1200}));
        let design = h.app.session.doc.to_json();
        let moves = h.app.session.pending_moves.clone();
        h.app.start(command);
        h.frames(4);
        for key in ["dialog:header", "dialog:footer", "dialog:outer"] {
            let rect = scenario::rect_of(key).unwrap_or_else(|| panic!("{command}: missing {key}"));
            assert!(rect.is_finite() && rect.is_positive() && h.ctx.content_rect().contains_rect(rect), "{command}: {key} {rect:?}");
        }
        let footer = scenario::rect_of("dialog:footer").unwrap();
        assert!(footer.contains(scenario::handle_at("dialog:cancel").unwrap()), "{command}: cancel remains in the pinned footer");
        h.step(&json!({"click":{"handle":"dialog:cancel"}})).unwrap();
        assert!(h.app.dialog.is_none(), "{command}");
        assert_eq!(h.app.session.doc.to_json(), design, "{command}: opening/cancelling keeps the design");
        assert_eq!(h.app.session.pending_moves, moves, "{command}: cancel restores display-only moves");
    }
}

fn resized_box_editor(command: &str, params: Value) -> (Harness, u64) {
    let mut h = Harness::new();
    for (id, params) in [
        ("sketch.create", json!({"plane":"XY"})),
        ("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,20]})),
        ("sketch.dimension", json!({"entities":["l1"],"value":40})),
        ("sketch.dimension", json!({"entities":["l2"],"value":20})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"distance":20})),
        (command, params),
        ("parameters.change", json!({"name":"d2","expression":"30"})),
    ] {
        h.app.run(id, params).unwrap();
    }
    let id = h.app.session.doc.features.last().unwrap().id;
    assert!(h.app.session.model.results.iter().all(|r| r.error.is_none()));
    h.app.edit_feature(id);
    (h, id)
}

#[test]
fn blend_editor_refills_named_edges_after_sketch_resize() {
    for command in ["solid.fillet", "solid.chamfer"] {
        let (mut h, id) = resized_box_editor(command, json!({"edges":[[40,10,20],[20,20,20],[20,0,20],[0,10,20]],"radius":4,"distance":4}));
        let d = h.app.dialog.as_ref().unwrap();
        let picks = &d.inputs[0].items;
        assert_eq!(picks.len(), 4);
        assert_eq!(&d.picked_edges, picks);
        let s = &h.app.session;
        let body = s.model.state().bodies.first().unwrap().clone();
        let names = &s.doc.feature(id).unwrap().edge_names;
        for (pick, name) in picks.iter().zip(names) {
            let Sel::Edge { point, index, .. } = pick else { panic!("{pick:?}") };
            let hit = solidcraft_engine::doc::naming::locate_edge(&body, name, *point).unwrap();
            assert_eq!(*index, hit.edge);
            assert!(hit.on_point, "{command}: {point:?} must lie on the resized edge");
        }
        let commands = apply_commands(&h.app, d).unwrap();
        crate::dialogs::roll_marker(&mut h.app, None).unwrap();
        for (command, params) in commands {
            h.app.run(&command, params).unwrap();
        }
        assert_eq!(h.app.session.doc.features.last().unwrap().id, id);
        assert!(h.app.session.model.results.iter().all(|r| r.error.is_none()));
    }
}

#[test]
fn face_editor_refills_named_faces_after_sketch_resize() {
    for (command, params) in
        [("solid.shell", json!({"faces":[[20,20,10]],"thickness":2})), ("solid.draft", json!({"faces":[[20,20,10]],"neutral":"XY","angle":3}))]
    {
        let (mut h, id) = resized_box_editor(command, params);
        let d = h.app.dialog.as_ref().unwrap();
        let picks = &d.inputs[0].items;
        assert_eq!(picks.len(), 1, "{command}: the moved face must refill");
        let Sel::Face { point, .. } = picks.first().unwrap() else { panic!("{picks:?}") };
        assert!((point.y - 30.0).abs() < 1e-6, "{command}: {point:?}");
        let commands = apply_commands(&h.app, d).unwrap();
        crate::dialogs::roll_marker(&mut h.app, None).unwrap();
        for (command, params) in commands {
            h.app.run(&command, params).unwrap();
        }
        assert_eq!(h.app.session.doc.features.last().unwrap().id, id);
        assert!(h.app.session.model.results.iter().all(|r| r.error.is_none()));
    }
}

#[test]
fn solid_pattern_quantity_arrows_step_the_count_the_command_receives() {
    let mut h = fixture();
    h.app.run("parameters.add", json!({"name":"copies","expression":"3","unit":""})).unwrap();
    for (command, key, label) in [
        ("solid.pattern.circular", "count", "Quantity"),
        ("solid.pattern.rectangular", "count1", "Quantity"),
        ("solid.pattern.path", "count", "Quantity"),
    ] {
        let mut d = representative(&h, command);
        if command == "solid.pattern.rectangular" {
            d.inputs[1].items = vec![Sel::Axis { name: "X".into() }];
        }
        h.app.dialog = Some(d);
        h.settle();
        let sent = |h: &Harness| {
            let cmds = crate::dialogs::preview_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap_or_else(|e| panic!("{command}: {e}"));
            cmds.iter().find(|(id, _)| id == command).map(|(_, p)| p[key].clone()).unwrap_or_else(|| panic!("{command}: {cmds:?}"))
        };
        let before = sent(&h).as_str().unwrap().parse::<i64>().unwrap();
        h.step(&json!({"click":{"handle":format!("dialog:{label} up")}})).unwrap();
        assert_eq!(sent(&h), json!((before + 1).to_string()), "{command}");
        let set = |h: &mut Harness, text: &str| {
            if let Some(Kind::PatternRect { count, .. } | Kind::PatternCirc { count, .. } | Kind::PathPattern { count, .. }) =
                h.app.dialog.as_mut().map(|d| &mut d.kind)
            {
                *count = text.into();
            }
            h.frames(2);
        };
        // From 1 to the document's 1000 instances.
        set(&mut h, "1");
        h.step(&json!({"click":{"handle":format!("dialog:{label} down")}})).unwrap();
        assert_eq!(sent(&h), json!("1"), "{command}");
        set(&mut h, "1000");
        h.step(&json!({"click":{"handle":format!("dialog:{label} up")}})).unwrap();
        assert_eq!(sent(&h), json!("1000"), "{command}");
        set(&mut h, "copies");
        h.step(&json!({"click":{"handle":format!("dialog:{label} up")}})).unwrap();
        assert_eq!(sent(&h), json!("copies + 1"), "{command}");
        h.step(&json!({"click":{"handle":"dialog:cancel"}})).unwrap();
        h.settle();
    }
}
