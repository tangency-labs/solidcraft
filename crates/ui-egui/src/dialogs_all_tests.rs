//! Each ribbon feature form keeps its parameter builder while sharing the dense layout.
use super::*;
use crate::scenario::{self, Harness};

#[test]
fn measure_input_fits_external_sketch_geometry() {
    let mut h = Harness::new();
    let curve = Sel::ExternalSketchCurve { sketch: 17, id: "line1".into() };
    let point = Sel::ExternalSketchPoint { sketch: 17, id: "p1".into() };
    h.app.session.selection = vec![curve.clone(), point.clone()];
    let mut d = Dialog::for_command(&h.app, "inspect.measure").unwrap();
    let accept = d.inputs.first().unwrap().accept;
    assert!(fits(accept, &curve));
    assert!(fits(accept, &point));
    assert_eq!(d.items(), vec![curve.clone(), point.clone()]);
    assert_eq!(
        d.candidate(&h.app.session, &Hit::SketchCurve { sketch: 17, id: "line1".into(), straight: true, point: solidcraft_engine::geom::Vec3::ZERO }),
        Some(curve.clone())
    );
    assert_eq!(
        d.candidate(&h.app.session, &Hit::SketchPoint { sketch: 17, id: "p1".into(), at: solidcraft_engine::geom::Vec2::new(0.0, 0.0) }),
        Some(point.clone())
    );
    d.take_box(&h.app.session, vec![curve.clone(), point.clone()], false);
    assert_eq!(d.items(), vec![curve, point]);
}

#[test]
fn feature_inputs_reject_external_sketch_geometry() {
    let h = Harness::new();
    for (command, sel) in [
        ("solid.pipe", Sel::ExternalSketchCurve { sketch: 17, id: "line1".into() }),
        ("solid.hole", Sel::ExternalSketchPoint { sketch: 17, id: "p1".into() }),
    ] {
        let d = Dialog::for_command(&h.app, command).unwrap();
        assert!(!fits(d.inputs.first().unwrap().accept, &sel), "{command}");
    }
}

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
        if command == "solid.coil" {
            continue; // an empty Profile input keeps the primitive section, which this form checks
        }
        if command == "solid.cylinder" {
            continue; // its face and reference edges place it (cylinder reference tests); this form checks its fields
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
            vec![Sel::SketchCurve { id: path.clone(), sketch: None }]
        } else if input.accept & AXES != 0 {
            // The Revolve fixture's profile is in XY; its preview needs an axis in that plane.
            let axis = if command == "solid.revolve" { "Y" } else { "Z" };
            vec![Sel::Axis { name: axis.into() }]
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
            nodes.iter().any(|(_, n)| n.role() == egui::accesskit::Role::Button && n.label() == Some(crate::i18n::tr(input.label))),
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
        "Inset By" => Role::ComboBox,
        "Radius" if command == "joint.rack_pinion" => Role::Label,
        "Pitch" if command == "solid.thread" => Role::ComboBox,
        "Automatic size" | "Reverse" | "Other side" | "All Lines" | "Mirror" => Role::CheckBox,
        "Offset" if command == "frame.end_cap" => Role::CheckBox,
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
form_case!(end_cap, "frame.end_cap", ["Thickness", "Direction", "Offset", "Corner Chamfer"]);

/// The Motion Study's and Motion Trace's hints read in the user's language: the dialogs drew them
/// as they got them, in English.
#[test]
fn motion_dialog_hints_are_translated() {
    fn texts(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => out.push(text.galley.job.text.clone()),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| texts(shape, out)),
            _ => {}
        }
    }
    let mut h = fixture();
    h.app.preferences.language = "ja".into();
    let _japanese = crate::i18n::scope("ja");
    for (command, english) in [("motion.study", "add a key for a joint first"), ("motion.trace", "select a point on a component")] {
        let mut d = Dialog::for_command(&h.app, command).unwrap();
        let mut painted = Vec::new();
        for _ in 0..2 {
            let mut out = h.ctx.run_ui(
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(320.0, 1600.0))), ..Default::default() },
                |ui| {
                    ui.set_width(264.0);
                    ui_kit::scope(ui, |ui| {
                        rows(&mut h.app, &h.ctx, &mut d, ui);
                    });
                },
            );
            out.textures_delta.clear();
            painted.clear();
            out.shapes.iter().for_each(|shape| texts(&shape.shape, &mut painted));
        }
        assert!(!painted.iter().any(|p| p == english), "{command} paints {english:?} in Japanese");
        assert!(painted.iter().any(|p| p == crate::i18n::tr_label(english)), "{command} paints {painted:?}");
    }
}

fn mirror_preselection() -> (Harness, Vec<Sel>) {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY","name":"Sketch1"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[2,2],"p1":[6,6]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("solid.extrude", json!({"sketch":"Sketch1","distance":5})).unwrap();
    let id = h.app.session.doc.features.iter().find(|f| f.name == "Extrude1").unwrap().id;
    let picked = vec![Sel::Feature { id }];
    h.app.session.selection = picked.clone();
    h.app.start("solid.mirror");
    h.frames(4);
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items, picked);
    (h, picked)
}

fn mirror_object_type(h: &mut Harness, choice: &str) {
    h.step(&json!({"click":{"handle":"dialog:Object Type"}})).unwrap();
    h.step(&json!({"click":{"handle":format!("dialog:Object Type:{choice}")}})).unwrap();
}

#[test]
fn mirror_object_type_switch_back_restores_preselected_feature() {
    let (mut h, picked) = mirror_preselection();
    mirror_object_type(&mut h, "Bodies");
    assert!(h.app.dialog.as_ref().unwrap().inputs[0].items.is_empty());
    mirror_object_type(&mut h, "Features");
    let d = h.app.dialog.as_mut().unwrap();
    assert_eq!(d.inputs[0].items, picked);
    d.inputs[1].items = vec![Sel::Plane { name: "YZ".into() }];
    let commands = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(commands[0].1["features"], json!(["Extrude1"]));
    for (command, params) in commands {
        h.app.run(&command, params).unwrap();
    }
    assert_eq!(h.app.session.world_state().bodies.len(), 2);
}

#[test]
fn mirror_timeline_click_keeps_picked_feature() {
    for dock in ["bottom", "left", "under_browser"] {
        mirror_history_click_keeps_picked_feature(dock);
    }
}

fn mirror_history_click_keeps_picked_feature(dock: &str) {
    let (mut h, picked) = mirror_preselection();
    h.app.run("view.history_layout", json!({"dock": dock})).unwrap();
    h.frames(4);
    // Mirror moves on to the plane; the objects input takes timeline clicks once it is active.
    h.step(&json!({"click":{"handle":"dialog:Features"}})).unwrap();
    assert_eq!(h.app.dialog.as_ref().unwrap().active, 0);
    h.step(&json!({"click":{"handle":"timeline:Extrude1"}})).unwrap();
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items, picked);
    // Wait out the double-click time: a double click opens the feature to edit.
    h.frames(30);
    h.step(&json!({"click":{"handle":"timeline:Extrude1"},"ctrl":true})).unwrap();
    let d = h.app.dialog.as_ref().unwrap();
    assert!(matches!(d.kind, Kind::Mirror));
    assert!(d.inputs[0].items.is_empty());
}

#[test]
fn rectangular_pattern_takes_features_from_every_history_layout() {
    for dock in ["bottom", "left", "under_browser"] {
        let mut h = Harness::new();
        h.app.run("view.history_layout", json!({"dock": dock})).unwrap();
        h.app.run("solid.box", json!({"length": 80, "width": 60, "height": 20})).unwrap();
        h.app.run("solid.cylinder", json!({"base": [20, 20, 20], "radius": 5, "height": 10, "operation": "join"})).unwrap();
        let id = h.app.session.doc.find_feature("Cylinder1").unwrap().id;
        h.app.run("select.clear", json!({})).unwrap();
        h.app.start("solid.pattern.rectangular");
        h.frames(4);
        mirror_object_type(&mut h, "Features");
        h.step(&json!({"click": {"handle": "timeline:Cylinder1"}})).unwrap();

        let d = h.app.dialog.as_ref().unwrap();
        assert_eq!(d.inputs[0].items, vec![Sel::Feature { id }], "{dock}: the dialog receives the History feature");
        assert_eq!(d.active, 0, "{dock}: Features stays active so more features can be picked");
        assert!(h.app.session.selection.is_empty(), "{dock}: a dialog pick does not replace the ordinary selection");
        h.app.dialog.as_mut().unwrap().inputs[1].items = vec![Sel::Axis { name: "X".into() }];
        let commands = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        assert_eq!(commands[0].1["features"], json!(["Cylinder1"]), "{dock}: the command receives the chosen feature");
    }
}

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
form_case!(align, "solid.align", ["Object Type", "From Snap", "To Snap"]);

#[test]
fn align_components_mode_switches_and_aligns_only_selected_instance() {
    let mut h = Harness::new();
    let comp = h.app.run("component.create", json!({"name": "Extrusion"})).unwrap()["component"].as_u64().unwrap();
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 60, "body_name": "Beam"})).unwrap();
    h.app.run("component.activate", json!({"component": "root"})).unwrap();
    let second = h.app.run("occurrence.copy", json!({"component": comp, "translate": [40, 0, 0]})).unwrap()["occurrence"].as_u64().unwrap();

    // Preselecting an occurrence opens Align in Components mode and advances to From.
    let picked = vec![Sel::Occurrence { path: vec![second] }];
    h.app.session.selection = picked.clone();
    h.app.start("solid.align");
    h.frames(4);
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].label, "Components");
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items, picked);
    assert_eq!(h.app.dialog.as_ref().unwrap().active, 1);

    // Switching Object Type to Bodies and back restores the preselected occurrence.
    mirror_object_type(&mut h, "Bodies");
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].label, "Bodies");
    assert!(h.app.dialog.as_ref().unwrap().inputs[0].items.is_empty());
    mirror_object_type(&mut h, "Components");
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].label, "Components");
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items, picked);

    let from = face_sel(&h.app.session, Vec3::new(45.0, 5.0, 0.0)).unwrap();
    let to = face_sel(&h.app.session, Vec3::new(10.0, 5.0, 30.0)).unwrap();
    {
        let d = h.app.dialog.as_mut().unwrap();
        d.inputs[1].items = vec![from];
        d.inputs[2].items = vec![to];
    }
    let commands = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(commands[0].0, "occurrence.align");
    assert_eq!(commands[0].1["paths"], json!([[second]]));
    for (command, params) in commands {
        h.app.run(&command, params).unwrap();
    }

    let st = h.app.session.world_state();
    let b1 = st.body("Beam").unwrap().mesh().bounds();
    assert!((b1.min.x - 0.0).abs() < 1e-5 && (b1.max.x - 10.0).abs() < 1e-5, "{b1:?}");
    let b2 = st.body("Beam (Extrusion:2)").unwrap().mesh().bounds();
    assert!((b2.min.x - 10.0).abs() < 1e-5 && (b2.max.x - 70.0).abs() < 1e-5, "{b2:?}");
}
form_case!(remove, "solid.remove", []);
form_case!(path_pattern, "solid.pattern.path", ["Type", "Object Type", "Quantity", "Spacing"]);
form_case!(pipe, "solid.pipe", ["Diameter", "Wall", "Operation"]);
form_case!(stock, "solid.bounding_solid", ["Margin"]);
form_case!(frame_member, "frame.member", ["All Lines", "Anchor", "Angle", "Mirror", "Name"]);
form_case!(thread, "solid.thread", ["Profile", "Size", "Pitch", "Designation"]);
form_case!(primitive_box, "solid.box", ["ui.length", "ui.width", "ui.height", "Operation"]);
form_case!(cylinder, "solid.cylinder", ["ui.diameter", "ui.height", "Operation"]);
form_case!(sphere, "solid.sphere", ["ui.diameter", "Operation"]);
form_case!(torus, "solid.torus", ["ui.major_radius", "ui.minor_radius", "Operation"]);
form_case!(cone, "solid.cone", ["ui.diameter", "ui.top_diameter", "ui.height", "Operation"]);
form_case!(coil, "solid.coil", ["ui.diameter", "ui.revolutions", "ui.pitch", "ui.section_size", "Operation"]);
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
form_case!(path_flange, "sheet.path_flange", ["Bend Radius"]);
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

/// Texts these dialogs draw themselves read in the user's language: the joints' Play button, the
/// Motion Study's Loop, the Stitch tolerance hint, the Flange type and the Emboss note.
#[test]
fn hand_drawn_dialog_texts_are_translated() {
    fn texts(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => out.push(text.galley.job.text.clone()),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| texts(shape, out)),
            _ => {}
        }
    }
    let mut h = fixture();
    h.app.preferences.language = "ja".into();
    let _japanese = crate::i18n::scope("ja");
    for (command, english) in [
        ("joint.gear", "▶ Play"),
        ("motion.study", "Loop"),
        ("surface.stitch", "tight"),
        ("sheet.flange", "Base"),
        ("solid.emboss", "Orthogonal line profiles tangent to one supported cylinder wrap automatically. Pick a wall to choose a target."),
    ] {
        let mut d = Dialog::for_command(&h.app, command).unwrap();
        let mut painted = Vec::new();
        for _ in 0..2 {
            let mut out = h.ctx.run_ui(
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(320.0, 1600.0))), ..Default::default() },
                |ui| {
                    ui.set_width(264.0);
                    ui_kit::scope(ui, |ui| {
                        rows(&mut h.app, &h.ctx, &mut d, ui);
                    });
                },
            );
            out.textures_delta.clear();
            painted.clear();
            out.shapes.iter().for_each(|shape| texts(&shape.shape, &mut painted));
        }
        assert!(!painted.iter().any(|p| p == english), "{command} paints {english:?} in Japanese: {painted:?}");
    }
}

#[test]
fn revolve_layout_fixture_builds_a_valid_solid() {
    let mut h = fixture();
    let d = representative(&h, "solid.revolve");
    let before = h.app.session.model.results.len();
    let commands = crate::dialogs::preview_commands(&h.app, &d).unwrap();
    assert!(!commands.is_empty());
    for (command, params) in commands {
        h.app.run(&command, params).unwrap();
    }
    assert!(h.app.session.model.results.len() > before);
    assert!(h.app.session.model.results.iter().all(|result| result.error.is_none()));
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

/// Interface › Dialog buttons › Top (#6708): every card made by the shared panel moves its
/// actions into the header row, with the footer's enabled states, handles and close behaviour.
#[test]
fn top_dialog_buttons_keep_every_cards_states_handles_and_close() {
    for command in [
        "solid.revolve",
        "solid.shell",
        "solid.box",
        "joint.create",
        "solid.thread",
        "inspect.measure",
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
        let mut seen = Vec::new();
        for placement in ["bottom", "top"] {
            let mut h = fixture();
            h.call("ui.resize", json!({"width":1600,"height":1200}));
            h.call("ui.prefs", json!({"dialog_buttons": placement}));
            let design = h.app.session.doc.to_json();
            h.app.start(command);
            h.frames(4);
            let header = scenario::rect_of("dialog:header").unwrap_or_else(|| panic!("{command} {placement}: no header"));
            let cancel = scenario::handle_at("dialog:cancel").unwrap_or_else(|| panic!("{command} {placement}: no cancel"));
            seen.push((
                scenario::count_of("dialog:ok:enabled"),
                scenario::rect_of("dialog:ok").is_some(),
                scenario::rect_of("dialog:apply").is_some(),
            ));
            if placement == "top" {
                assert!(scenario::rect_of("dialog:footer").is_none(), "{command}: no footer row");
                assert!(header.contains(cancel), "{command}: cancel in the header {header:?}");
                assert!(scenario::rect_of("dialog:ok").is_none_or(|ok| header.contains_rect(ok)), "{command}: OK in the header");
                assert_eq!(scenario::handle_at("dialog:close"), Some(cancel), "{command}: Cancel is the close");
            } else {
                assert!(scenario::rect_of("dialog:footer").unwrap().contains(cancel), "{command}: cancel in the footer");
            }
            h.step(&json!({"click":{"handle":"dialog:close"}})).unwrap();
            assert!(h.app.dialog.is_none(), "{command} {placement}: closed");
            assert_eq!(h.app.session.doc.to_json(), design, "{command} {placement}: closing keeps the design");
        }
        assert_eq!(seen.first(), seen.get(1), "{command}: OK and Apply as at the bottom");
    }
}

/// With the buttons at the top, OK, Enter and Esc do what they do with them at the bottom.
#[test]
fn top_dialog_buttons_apply_and_cancel_like_the_footer() {
    for how in ["click", "enter", "escape"] {
        let mut h = fixture();
        h.call("ui.resize", json!({"width":1600,"height":1200}));
        h.call("ui.prefs", json!({"dialog_buttons": "top"}));
        h.app.dialog = Some(representative(&h, "solid.revolve"));
        h.settle();
        assert_eq!(scenario::count_of("dialog:ok:enabled"), Some(1.0));
        let (design, bodies) = (h.app.session.doc.to_json(), h.app.session.world_state().bodies.len());
        let step = match how {
            "click" => json!({"click":{"handle":"dialog:OK"}}),
            "enter" => json!({"key":"Enter"}),
            _ => json!({"key":"Escape"}),
        };
        h.step(&step).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none(), "{how}: the dialog closes");
        if how == "escape" {
            assert_eq!(h.app.session.doc.to_json(), design, "Esc leaves the design as it was");
        } else {
            assert_eq!(h.app.session.world_state().bodies.len(), bodies + 1, "{how}: the revolve is made");
        }
    }
}

/// OK and Cancel in the view's corner (#6709) do what the dialog's own do: the check applies and
/// closes, the cross cancels without a change, and the check is grey (and does nothing) whenever
/// the dialog's OK is.
#[test]
fn view_corner_ok_and_cancel_answer_the_dialog_like_its_own_buttons() {
    for how in ["ok", "cancel"] {
        let mut h = fixture();
        h.call("ui.resize", json!({"width":1600,"height":1200}));
        h.call("ui.prefs", json!({"view_confirm_corner": true}));
        h.app.dialog = Some(representative(&h, "solid.revolve"));
        h.settle();
        assert_eq!(scenario::count_of("view:confirm:ok:enabled"), scenario::count_of("dialog:ok:enabled"));
        assert_eq!(scenario::count_of("view:confirm:ok:enabled"), Some(1.0));
        let (design, bodies) = (h.app.session.doc.to_json(), h.app.session.world_state().bodies.len());
        h.step(&json!({"click":{"handle":format!("view:confirm:{how}")}})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none(), "{how}: the dialog closes");
        if how == "ok" {
            assert_eq!(h.app.session.world_state().bodies.len(), bodies + 1, "the check made the revolve");
        } else {
            assert_eq!(h.app.session.doc.to_json(), design, "the cross leaves the design as it was");
        }
        assert!(scenario::rect_of("view:confirm").is_none(), "{how}: no dialog and no sketch, no pair");
    }

    // A dialog whose OK is grey: so is the check, and clicking it changes nothing.
    let mut h = fixture();
    h.call("ui.resize", json!({"width":1600,"height":1200}));
    h.call("ui.prefs", json!({"view_confirm_corner": true}));
    h.app.start("solid.shell");
    h.settle();
    assert_eq!(scenario::count_of("dialog:ok:enabled"), Some(0.0));
    assert_eq!(scenario::count_of("view:confirm:ok:enabled"), Some(0.0));
    let design = h.app.session.doc.to_json();
    h.step(&json!({"click":{"handle":"view:confirm:ok"}})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_some(), "a grey check does nothing");
    assert_eq!(h.app.session.doc.to_json(), design);
    // Measure only closes: its check stays grey and the cross is its Close.
    h.app.start("inspect.measure");
    h.settle();
    assert_eq!(scenario::count_of("view:confirm:ok:enabled"), Some(0.0));
    h.step(&json!({"click":{"handle":"view:confirm:cancel"}})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none(), "the cross closes Measure");
}

/// Both options at once (#6708 with #6709): the check in the view's corner greys out with the OK
/// in the title row, the pair keeps clear of the dialog, and either OK applies it.
#[test]
fn top_dialog_buttons_and_the_view_corner_agree() {
    let open = |command: &str| {
        let mut h = fixture();
        h.call("ui.resize", json!({"width":1600,"height":1200}));
        h.call("ui.prefs", json!({"dialog_buttons": "top", "view_confirm_corner": true}));
        if command == "solid.revolve" {
            h.app.dialog = Some(representative(&h, command));
        } else {
            h.app.start(command);
        }
        h.settle();
        h
    };
    for (command, enabled) in [("solid.revolve", 1.0), ("solid.shell", 0.0)] {
        let _h = open(command);
        let header = scenario::rect_of("dialog:header").unwrap();
        assert!(scenario::rect_of("dialog:footer").is_none(), "{command}");
        assert!(header.contains_rect(scenario::rect_of("dialog:ok").unwrap()), "{command}: OK in the title row");
        assert_eq!(scenario::count_of("dialog:ok:enabled"), Some(enabled), "{command}");
        assert_eq!(scenario::count_of("view:confirm:ok:enabled"), Some(enabled), "{command}: the check follows the title row's OK");
        assert!(!scenario::rect_of("view:confirm").unwrap().intersects(header), "{command}: the pair keeps clear of the dialog");
    }
    for handle in ["dialog:ok", "view:confirm:ok"] {
        let mut h = open("solid.revolve");
        let bodies = h.app.session.world_state().bodies.len();
        h.step(&json!({"click":{"handle":handle}})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none(), "{handle}: the dialog closes");
        assert_eq!(h.app.session.world_state().bodies.len(), bodies + 1, "{handle}: the revolve is made");
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
fn draft_uses_the_picked_solid_beside_a_hidden_overlapping_mesh() {
    use solidcraft_engine::doc::{Document, MeshData, naming};

    let mesh = solidcraft_engine::kernel::box_solid(Vec3::ZERO, Vec3::new(20.0, 20.0, 10.0)).unwrap().tessellate(0.01).unwrap();
    let mut doc = Document::default();
    doc.add_feature(
        FeatureKind::MeshImport {
            file: "overlapping-box.stl".into(),
            meshes: vec![MeshData {
                name: "Hidden mesh".into(),
                positions: mesh.positions.iter().flat_map(|p| [p.x, p.y, p.z]).collect(),
                triangles: mesh.triangles.iter().flatten().copied().collect(),
                color: None,
            }],
        },
        Some("Imported mesh"),
    )
    .unwrap();
    let mut h = Harness::new();
    h.app.session = Session::new(doc);
    h.app.run("solid.box", json!({"length":20,"width":20,"height":10})).unwrap();
    h.app.run("browser.visibility", json!({"items":["Hidden mesh"],"visible":false})).unwrap();
    let state = h.app.session.world_state();
    let hidden_mesh = state.body("Hidden mesh").unwrap().mesh();
    let body = state.bodies.iter().find(|body| !body.body.is_mesh()).unwrap();
    let target = body.name.clone();
    let point = Vec3::new(20.0, 10.0, 5.0);
    let name = naming::face_name_at_body(body, point).unwrap();
    let (index, _, _) = naming::face_on_body(body, &name, point).unwrap();
    h.app.session.selection = vec![Sel::Face { body: target.clone(), index, point }];
    let mut d = Dialog::for_command(&h.app, "solid.draft").unwrap();
    d.inputs[1].items = vec![Sel::Plane { name: "XY".into() }];
    d.kind = Kind::Draft { angle: "5 deg".into() };
    let commands = apply_commands(&h.app, &d).unwrap();
    assert_eq!(preview_commands(&h.app, &d).unwrap(), commands);
    let (command, params) = &commands[0];
    h.app.run(command, params.clone()).unwrap();
    assert_eq!(params["body"], target);
    let state = h.app.session.model.state();
    assert_eq!(state.bodies.len(), 2);
    assert!(state.body("Hidden mesh").unwrap().body.is_mesh());
    assert_eq!(state.body("Hidden mesh").unwrap().mesh().positions, hidden_mesh.positions);
    assert_eq!(state.body("Hidden mesh").unwrap().mesh().triangles, hidden_mesh.triangles);
    let volume = state.body(&target).unwrap().body.volume_at(0.01).unwrap();
    assert!((volume - 4000.0).abs() > 50.0, "the picked wall should taper: {volume}");
    assert!(matches!(
        &h.app.session.doc.features.last().unwrap().kind,
        FeatureKind::Draft { body: Some(body), .. } if *body == target
    ));
}

#[test]
fn draft_rejects_faces_from_different_bodies() {
    let h = Harness::new();
    let mut d = Dialog::for_command(&h.app, "solid.draft").unwrap();
    d.inputs[0].items =
        vec![Sel::Face { body: "First".into(), index: 0, point: Vec3::ZERO }, Sel::Face { body: "Second".into(), index: 0, point: Vec3::X }];
    d.inputs[1].items = vec![Sel::Plane { name: "XY".into() }];
    assert_eq!(apply_commands(&h.app, &d).unwrap_err(), "pick faces of one body");
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

#[test]
fn coil_from_a_profile_hides_the_primitive_section() {
    let h = fixture();
    let mut d = Dialog::for_command(&h.app, "solid.coil").unwrap();
    let sketch = h.app.session.world_state().sketches[0].feature;
    d.inputs[0].items = vec![Sel::Profile { sketch, index: 0 }];
    let commands = apply_commands(&h.app, &d).unwrap();
    let (id, p) = &commands[0];
    assert_eq!(id, "solid.coil");
    assert_eq!(p["sketch"], json!(sketch));
    assert_eq!(p["profiles"], json!([0]));
    assert!(p.get("diameter").is_none(), "{p}");
    assert!(p.get("section_size").is_none(), "{p}");
    assert!(p.get("revolutions").is_some() && p.get("pitch").is_some(), "{p}");
    assert!(p.get("operation").is_some(), "{p}");
}

fn extrude_focus_fixture() -> Harness {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[50,30]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.call("ui.view", json!({"view":"top"}));
    h.call("ui.view", json!({"view":"fit"}));
    h.step(&json!({"start":"solid.extrude"})).unwrap();
    h.frames(8);
    h
}

#[test]
fn viewport_profile_click_returns_extrude_value_focus() {
    let mut h = extrude_focus_fixture();
    h.step(&json!({"click":{"world":[15,10,0]}})).unwrap();
    h.step(&json!({"text":"30"})).unwrap();
    assert!(matches!(&h.app.dialog.as_ref().unwrap().kind, Kind::Extrude { distance, .. } if distance == "30"));
    // Clicking an already selected profile removes it; pick it again before building.
    h.step(&json!({"click":{"world":[15,10,0]}})).unwrap();
    h.step(&json!({"text":"30"})).unwrap();
    h.step(&json!({"key":"Enter"})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none());
    let bounds = h.app.session.world_state().bodies.first().unwrap().mesh().bounds();
    assert!((bounds.max.z - bounds.min.z - 30.0).abs() < 1e-7);
}

/// Typing `depth = 30` in the extrude dialog's distance defines the user parameter `depth` and
/// the extrude uses it, in the dialog's one undo step; the preview made nothing (#4071).
#[test]
fn a_dialog_field_typed_as_a_definition_defines_the_parameter() {
    let mut h = extrude_focus_fixture();
    let undo = h.app.session.undo.len();
    // As above: the first click returns focus to the distance, the second picks the profile.
    h.step(&json!({"click":{"world":[15,10,0]}})).unwrap();
    h.step(&json!({"text":"30"})).unwrap();
    h.step(&json!({"click":{"world":[15,10,0]}})).unwrap();
    h.step(&json!({"text":"depth = 30"})).unwrap();
    assert!(matches!(&h.app.dialog.as_ref().unwrap().kind, Kind::Extrude { distance, .. } if distance == "depth = 30"));
    h.settle();
    assert!(h.app.session.doc.param("depth").is_none(), "the preview defined nothing");
    h.step(&json!({"key":"Enter"})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none(), "{:?} {:?}", h.app.status, h.app.dialog.as_ref().map(|d| (d.error.clone(), format!("{:?}", d.kind))));
    let doc = &h.app.session.doc;
    let depth = doc.param("depth").expect("the user parameter was added");
    assert!(!depth.model && depth.expr == "30" && depth.unit == "mm", "{depth:?}");
    let extrude = doc.features.iter().rev().find(|f| f.name.starts_with("Extrude")).unwrap();
    assert!(extrude.kind.inputs().iter().any(|(label, e, _)| *label == "Distance" && e == "depth"), "{:?}", extrude.kind.inputs());
    assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step");
    let bounds = h.app.session.world_state().bodies.first().unwrap().mesh().bounds();
    assert!((bounds.max.z - bounds.min.z - 30.0).abs() < 1e-7);
}

#[test]
fn viewport_empty_click_falls_back_to_first_numeric_dialog_field() {
    let mut h = extrude_focus_fixture();
    if let Some(id) = h.ctx.memory(|m| m.focused()) {
        h.ctx.memory_mut(|m| m.surrender_focus(id));
    }
    h.frames(2);
    let viewport = h.app.viewport.rect.unwrap();
    h.step(&json!({"click":[viewport.left()+80.0, viewport.bottom()-80.0]})).unwrap();
    h.step(&json!({"text":"30"})).unwrap();
    assert!(matches!(&h.app.dialog.as_ref().unwrap().kind, Kind::Extrude { distance, .. } if distance == "30"));
}

#[test]
fn viewport_click_preserves_active_secondary_dialog_value() {
    let mut h = extrude_focus_fixture();
    if let Kind::Extrude { direction, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
        *direction = crate::dialogs::TWO_SIDES;
    }
    h.frames(3);
    crate::test_support::click(&mut h, "dialog:Distance 2");
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"7"})).unwrap();
    assert!(matches!(&h.app.dialog.as_ref().unwrap().kind, Kind::Extrude { distance2, .. } if distance2 == "7"));
    h.step(&json!({"click":{"world":[15,10,0]}})).unwrap();
    h.step(&json!({"text":"30"})).unwrap();
    assert!(matches!(&h.app.dialog.as_ref().unwrap().kind, Kind::Extrude { distance, distance2, .. } if distance == "10 mm" && distance2 == "30"));
}

/// A circle drawn partly past the top face its sketch is on is cut by the face's projected outline
/// into a lens on the face and a crescent past it. Clicking one of them in Extrude builds that
/// region alone, the one highlighted and counted, not the whole circle (#6724).
#[test]
fn extrude_builds_the_one_region_clicked_where_the_face_outline_cuts_a_profile() {
    for x in [7.0, 14.0] {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.app.run("sketch.circle.center", json!({"center": [0, 0], "radius": 10})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("solid.extrude", json!({"distance": 10, "body_name": "Base"})).unwrap();
        let sketch = h.app.run("sketch.create", json!({"plane": {"face": [0, 0, 10]}})).unwrap()["sketch"].as_u64().unwrap();
        h.app.run("sketch.circle.center", json!({"center": [10, 0], "radius": 6})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        let state = h.app.session.world_state();
        let ss = state.sketch(sketch).unwrap();
        assert_eq!(ss.profiles.len(), 3, "the lens, the crescent past the face and the rest of the face");
        let index = ss.profiles.iter().position(|p| p.region.contains(solidcraft_engine::geom::Vec2::new(x, 0.0))).unwrap();
        let area = ss.profiles[index].area;
        let mut d = Dialog::for_command(&h.app, "solid.extrude").unwrap();
        if let Kind::Extrude { distance, .. } = &mut d.kind {
            *distance = "5".into();
        }
        d.pick(&h.app.session, Sel::Profile { sketch, index });
        assert_eq!(d.items(), vec![Sel::Profile { sketch, index }], "one region picked at x = {x}");
        let commands = apply_commands(&h.app, &d).unwrap();
        assert_eq!(commands.len(), 1, "{commands:?}");
        let (id, p) = &commands[0];
        assert_eq!(p["exact_regions"], json!(true), "{p}");
        let before = h.app.run("inspect.measure", json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap();
        let made = h.app.run(id, p.clone()).unwrap()["feature"].as_u64().unwrap();
        let Some(FeatureKind::Extrude { profiles, .. }) = h.app.session.doc.feature(made).map(|f| f.kind.clone()) else { panic!("an extrude") };
        let after = h.app.session.world_state();
        let used = solidcraft_engine::doc::profile_selection_indices(after.sketch(sketch).unwrap(), &profiles).unwrap();
        assert_eq!(used, vec![index], "the region clicked at x = {x}, and only it");
        let volume = h.app.run("inspect.measure", json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap();
        assert!(((volume - before) - area * 5.0).abs() < 5e-3 * area * 5.0, "x = {x}: {volume} - {before} vs {}", area * 5.0);
    }
}

/// Every selection field of every command's dialog is labelled with a message the catalogs carry,
/// so it reads in the user's language ("First edge", "Second point" read English everywhere when
/// they were missing).
#[test]
fn every_selection_field_label_is_translated() {
    let h = fixture();
    let _japanese = crate::i18n::scope("ja");
    let mut missing = std::collections::BTreeSet::new();
    for spec in solidcraft_engine::command_specs() {
        let Some(d) = Dialog::for_command(&h.app, spec.id) else { continue };
        for input in &d.inputs {
            let label = input.label.trim();
            if label.chars().any(|c| c.is_ascii_alphabetic()) && crate::i18n::tr_label(label) == label {
                missing.insert(format!("{}: {label}", spec.id));
            }
        }
    }
    assert!(missing.is_empty(), "selection fields without a message: {missing:#?}");
}

#[test]
fn align_components_picks_the_placement_under_the_active_component_and_warns_when_shared() {
    let mut h = Harness::new();
    let made = h.app.run("component.create", json!({"name": "Frame", "grounded": false})).unwrap();
    let (frame, a) = (made["component"].as_u64().unwrap(), made["occurrence"].as_u64().unwrap());
    let bracket = h.app.run("component.create", json!({"name": "Bracket", "grounded": false})).unwrap()["occurrence"].as_u64().unwrap();
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 4, "body_name": "Plate"})).unwrap();
    h.app.run("component.activate", json!({"component": "root"})).unwrap();
    let plate = h.app.session.world_state().bodies.first().map(|b| b.name.clone()).unwrap();

    // With the design active, a click on the part inside Frame picks the Frame placement.
    assert_eq!(occurrence_path_of_body(&h.app.session, &plate), Some(vec![a]));
    // With that Frame placement active, the same click picks the part inside it.
    h.app.run("component.activate", json!({"component": frame, "occurrence": a, "path": [a]})).unwrap();
    assert_eq!(occurrence_path_of_body(&h.app.session, &plate), Some(vec![a, bracket]));
    assert!(!align_moves_shared(&h.app.session, &[Sel::Occurrence { path: vec![a, bracket] }]), "Frame is placed once");

    // Once Frame is placed twice, the part's placement inside it is shared: the dialog says so.
    h.app.run("component.activate", json!({"component": "root"})).unwrap();
    h.app.run("occurrence.copy", json!({"component": frame, "translate": [0, 50, 0]})).unwrap();
    assert!(align_moves_shared(&h.app.session, &[Sel::Occurrence { path: vec![a, bracket] }]));
    assert!(!align_moves_shared(&h.app.session, &[Sel::Occurrence { path: vec![a] }]), "a top-level placement is its own");

    h.app.session.selection = vec![Sel::Occurrence { path: vec![a] }];
    h.app.start("solid.align");
    h.frames(4);
    assert!(h.widget(ALIGN_SHARED_WARNING).is_none(), "no warning for a top-level pick");
    h.app.dialog.as_mut().unwrap().inputs[0].items = vec![Sel::Occurrence { path: vec![a, bracket] }];
    h.frames(4);
    assert!(h.widget(ALIGN_SHARED_WARNING).is_some(), "the shared pick is called out");
}

/// OK on the editor of a pattern behind the history marker leaves it rolled back, so the view
/// does not change: the status line says why, where the edit used to look as if it had silently
/// failed (#8100).
#[test]
fn ok_on_a_rolled_back_pattern_editor_says_it_is_not_built() {
    let mut h = Harness::new();
    h.app.home.open = false;
    for (id, params) in [
        ("solid.box", json!({"length":40,"width":30,"height":10,"corner":[0,0,0],"body_name":"Plate"})),
        ("solid.cylinder", json!({"radius":2,"height":20,"base":[10,15,-5],"operation":"cut","targets":["Plate"],"name":"Cut"})),
        ("solid.pattern.circular", json!({"features":["Cut"],"axis":{"origin":[20,15,0],"dir":[0,0,1]},"count":2,"name":"Pattern"})),
        ("timeline.roll_to", json!({"position":2})),
    ] {
        h.app.run(id, params).unwrap();
    }
    let pattern = h.app.session.doc.features.last().unwrap().id;
    h.app.edit_feature(pattern);
    h.settle();
    assert!(h.app.dialog.is_some(), "the pattern's editor opens");
    h.step(&json!({"click":{"handle":"dialog:OK"}})).unwrap();
    for _ in 0..500 {
        if h.app.dialog.is_none() {
            break;
        }
        h.frames(1);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(h.app.dialog.is_none(), "the edit is applied");
    assert_eq!(h.app.session.doc.marker, Some(2), "the pattern stays behind the marker");
    let (text, _, error) = h.app.status.clone().unwrap();
    assert!(!error, "{text}");
    assert!(text.starts_with("Pattern changed, but it is after the history marker"), "{text}");
}
