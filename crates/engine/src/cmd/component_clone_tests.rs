#![allow(clippy::unwrap_used)]
use crate::Session;
use serde_json::{Value, json};
fn run(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap()
}
fn component(s: &mut Session, name: &str) -> u64 {
    run(s, "component.create", json!({"name":name,"grounded":false}))["component"].as_u64().unwrap()
}
fn volume(s: &mut Session, id: u64) -> f64 {
    run(s, "component.properties", json!({"component":id}))["total"]["volume_mm3"].as_f64().unwrap()
}
fn box2(s: &mut Session) -> u64 {
    run(s, "solid.box", json!({"length":2,"width":2,"height":2,"body_name":"Cube"}))["feature"].as_u64().unwrap()
}
fn copy(s: &mut Session, id: u64) -> Value {
    run(s, "component.paste_new", json!({"component":id,"translate":[20,0,0]}))
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-6, "{a} != {b}");
}

#[test]
fn make_independent_is_one_undoable_edit_and_keeps_pose_and_view_masks() {
    let mut s = Session::default();
    let original = component(&mut s, "Bolt");
    let feature = box2(&mut s);
    let first = s.doc.occurrence_of(original).unwrap().id;
    let second = run(&mut s, "occurrence.copy", json!({"occurrence":first,"translate":[20,0,0]}))["occurrence"].as_u64().unwrap();
    let before = s.occurrence_placement(&[second]).unwrap();
    let body = s.occurrence_bodies(&[second], true).unwrap()[0].clone();
    s.visibility.hidden_bodies.push(body.clone());
    s.visibility.locked_bodies.push(body);
    let result = run(&mut s, "component.make_independent", json!({"occurrence":second}));
    let independent = result["component"].as_u64().unwrap();
    assert_ne!(independent, original);
    assert_eq!(s.doc.occurrences.iter().find(|o| o.id == first).unwrap().component, original);
    assert_eq!(s.occurrence_placement(&[second]).unwrap().world, before.world);
    let new_body = s.occurrence_bodies(&[second], true).unwrap()[0].clone();
    assert!(s.visibility.hidden_bodies.contains(&new_body) && s.visibility.locked_bodies.contains(&new_body));
    let parameter = s.doc.feature(feature).unwrap().param_names[0].clone();
    run(&mut s, "parameters.change", json!({"name":parameter,"expression":"3 mm"}));
    near(volume(&mut s, original), 12.);
    near(volume(&mut s, independent), 8.);
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.occurrences.iter().find(|o| o.id == second).unwrap().component, original);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.occurrences.iter().find(|o| o.id == second).unwrap().component, independent);
}

#[test]
fn make_independent_copies_the_selected_parent_path_but_keeps_siblings_linked() {
    let mut s = Session::default();
    let parent = component(&mut s, "Assembly");
    let first_parent = s.doc.occurrence_of(parent).unwrap().id;
    let child = component(&mut s, "Bolt");
    box2(&mut s);
    let bolt = s.doc.occurrence_of(child).unwrap().id;
    run(&mut s, "component.activate", json!({"component":parent}));
    let sibling = run(&mut s, "occurrence.copy", json!({"component":child,"translate":[10,0,0]}))["occurrence"].as_u64().unwrap();
    let second_parent = run(&mut s, "occurrence.copy", json!({"component":parent,"translate":[0,20,0]}))["occurrence"].as_u64().unwrap();
    let before = s.occurrence_placement(&[second_parent, bolt]).unwrap().world;
    let count = s.doc.components.len();
    let result = run(&mut s, "component.make_independent", json!({"path":[second_parent,bolt]}));
    let path: Vec<u64> = serde_json::from_value(result["path"].clone()).unwrap();
    assert_eq!(s.doc.components.len(), count + 2);
    assert_eq!(s.occurrence_placement(&path).unwrap().world, before);
    assert_ne!(s.occurrence_placement(&path).unwrap().component, child);
    assert_eq!(s.occurrence_placement(&[first_parent, bolt]).unwrap().component, child);
    let copied_parent = s.doc.occurrences.iter().find(|o| o.id == second_parent).unwrap().component;
    assert!(s.doc.occurrences.iter().any(|o| o.parent == copied_parent && o.component == child && o.id != *path.last().unwrap()));
    assert!(s.doc.occurrences.iter().any(|o| o.id == sibling && o.parent == parent));
}

#[test]
fn make_independent_keeps_internal_relationships_on_both_subassembly_placements() {
    use solidcraft_doc::FeatureKind;
    for first_selected in [true, false] {
        let mut s = Session::default();
        let parent = component(&mut s, "Assembly");
        let first = s.doc.occurrence_of(parent).unwrap().id;
        let sketch = run(&mut s, "sketch.create", json!({"plane":"XY", "name":"Hidden sketch"}))["sketch"].as_u64().unwrap();
        run(&mut s, "sketch.finish", json!({}));
        let child = component(&mut s, "Child");
        box2(&mut s);
        let child_occurrence = s.doc.occurrence_of(child).unwrap().id;
        let joint = run(&mut s, "joint.as_built", json!({"type":"rigid", "a":first, "b":child_occurrence}))["joint"].as_u64().unwrap();
        run(&mut s, "contact.create", json!({"name":"Internal contact", "occurrences":[first, child_occurrence]}));
        run(&mut s, "explode.create", json!({"name":"Internal explode", "moves":[{"occurrence":child_occurrence, "translate":[0,0,10]}]}));
        run(&mut s, "component.activate", json!({"component":0}));
        let second = run(&mut s, "occurrence.copy", json!({"component":parent, "translate":[20,0,0]}))["occurrence"].as_u64().unwrap();
        let (selected, remaining) = if first_selected { (first, second) } else { (second, first) };
        let boundary = run(&mut s, "joint.as_built", json!({"type":"rigid", "a":0, "b":selected}))["joint"].as_u64().unwrap();
        s.visibility.hidden_joints.push(joint);
        s.visibility.hidden_sketches.push(sketch);
        s.visibility.hidden_profiles.push(sketch);
        s.visibility.shown_dimensions.push(sketch);
        let world = s.occurrence_placement(&[selected, child_occurrence]).unwrap().world;
        let result = run(&mut s, "component.make_independent", json!({"path":[selected]}));
        let copied = result["component"].as_u64().unwrap();
        let copied_child = s.doc.occurrences.iter().find(|o| o.parent == copied).unwrap().id;
        assert_eq!(s.occurrence_placement(&[selected, copied_child]).unwrap().world, world);
        assert_eq!(s.occurrence_placement(&[remaining, child_occurrence]).unwrap().component, child);
        let original = s.doc.assembly.joints.iter().find(|j| j.id == joint).unwrap();
        assert_eq!((original.a.occurrence, original.b.occurrence), (remaining, child_occurrence));
        let independent = s.doc.assembly.joints.iter().find(|j| j.a.occurrence == selected && j.b.occurrence == copied_child).unwrap();
        assert_ne!(independent.id, joint);
        assert!(s.visibility.hidden_joints.contains(&joint) && s.visibility.hidden_joints.contains(&independent.id));
        let external = s.doc.assembly.joints.iter().find(|j| j.id == boundary).unwrap();
        assert_eq!((external.a.occurrence, external.b.occurrence), (0, selected));
        assert_eq!(s.doc.assembly.contact_sets.len(), 2);
        assert_eq!(s.doc.assembly.contact_sets[0].occurrences, [remaining, child_occurrence]);
        assert_eq!(s.doc.assembly.contact_sets[1].occurrences, [selected, copied_child]);
        assert_eq!(s.doc.assembly.exploded[0].moves[0].0, child_occurrence);
        assert_eq!(s.doc.assembly.exploded[1].moves[0].0, copied_child);
        let copied_sketch = s.doc.features.iter().find(|f| f.component == copied && matches!(f.kind, FeatureKind::Sketch { .. })).unwrap().id;
        assert!(s.visibility.hidden_sketches.contains(&sketch) && s.visibility.hidden_sketches.contains(&copied_sketch));
        assert!(s.visibility.hidden_profiles.contains(&copied_sketch) && s.visibility.shown_dimensions.contains(&copied_sketch));
        let reopened = Session::new(solidcraft_doc::Document::from_json(&s.design().to_json()).unwrap());
        assert_eq!(reopened.doc.assembly, s.doc.assembly);
        assert_eq!(reopened.visibility, s.visibility);
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.doc.assembly.joints.len(), 2);
        assert_eq!(s.doc.assembly.joints.iter().find(|j| j.id == joint).unwrap().a.occurrence, first);
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(s.doc.assembly, reopened.doc.assembly);
    }
}

#[test]
fn make_independent_refuses_unscoped_relationships_below_unchanged_children_atomically() {
    for first_selected in [true, false] {
        let mut s = Session::default();
        let child = component(&mut s, "Shared child");
        let outside_child = s.doc.occurrence_of(child).unwrap().id;
        let grandchild = component(&mut s, "Grandchild");
        box2(&mut s);
        let grandchild_occurrence = s.doc.occurrence_of(grandchild).unwrap().id;
        run(&mut s, "component.activate", json!({"component":0}));
        let parent = component(&mut s, "Assembly");
        let first = s.doc.occurrence_of(parent).unwrap().id;
        let child_occurrence = run(&mut s, "occurrence.copy", json!({"component":child, "parent":parent}))["occurrence"].as_u64().unwrap();
        assert_eq!(s.doc.first_path(child), [outside_child]);
        run(&mut s, "joint.as_built", json!({"type":"rigid", "a":first, "b":grandchild_occurrence}));
        run(&mut s, "component.activate", json!({"component":parent}));
        let sibling = component(&mut s, "Sibling");
        box2(&mut s);
        let sibling_occurrence = s.doc.occurrence_of(sibling).unwrap().id;
        run(&mut s, "component.activate", json!({"component":0}));
        let second = run(&mut s, "occurrence.copy", json!({"component":parent, "translate":[20,0,0]}))["occurrence"].as_u64().unwrap();
        let selected = if first_selected { first } else { second };
        let before = s.doc.clone();
        let visibility = s.visibility.clone();
        assert!(s.execute("component.make_independent", &json!({"path":[selected, sibling_occurrence]})).is_err());
        assert!(std::sync::Arc::ptr_eq(&s.doc, &before));
        assert_eq!(s.visibility, visibility);
        assert_eq!(s.occurrence_placement(&[selected, child_occurrence, grandchild_occurrence]).unwrap().component, grandchild);
    }
}

#[test]
fn source_duplicates_with_different_mapped_placements_keep_their_product_geometry() {
    use solidcraft_doc::IDENTITY;
    use solidcraft_geom::Vec3;
    use solidcraft_kernel::{ExportBody, ExportProduct, StepHeader, step_export_products};
    let body = solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(2., 3., 4.)).unwrap();
    let part =
        || ExportProduct { name: "Bolt".into(), bodies: vec![ExportBody { name: "Bolt".into(), body: &body, color: None }], children: Vec::new() };
    let products = [
        ExportProduct { name: "Assembly".into(), bodies: Vec::new(), children: vec![(1, IDENTITY, "First".into()), (2, IDENTITY, "Second".into())] },
        part(),
        part(),
    ];
    let text = step_export_products(&products, 0, &StepHeader::default()).unwrap();
    let mut mapped = text.clone();
    let mut extra = String::new();
    for (i, line) in text.lines().filter(|l| l.contains("ADVANCED_BREP_SHAPE_REPRESENTATION(")).enumerate() {
        let (representation, record) = line.split_once('=').unwrap();
        let (items, context) = record.split_once(",(").unwrap().1.split_once("),").unwrap();
        let origin = items.rsplit(',').next().unwrap();
        let context = context.trim_end_matches(");");
        let id = 1_000_000 + i * 10;
        let ending = format!(",{representation});");
        let relationship = text.lines().find(|l| l.contains("SHAPE_REPRESENTATION_RELATIONSHIP(") && l.ends_with(&ending)).unwrap();
        mapped = mapped.replace(relationship, &relationship.replace(&ending, &format!(",#{});", id + 4)));
        extra.push_str(&format!(
            "#{id}=REPRESENTATION_MAP({origin},{representation});\n#{}=CARTESIAN_POINT('',({}.,0.,0.));\n#{}=CARTESIAN_TRANSFORMATION_OPERATOR_3D('','',$,$,#{},1.,$);\n#{}=MAPPED_ITEM('',#{id},#{});\n#{}=SHAPE_REPRESENTATION('Bolt',(#{}),{context});\n",
            id + 1, i * 10, id + 2, id + 1, id + 3, id + 2, id + 4, id + 3
        ));
    }
    mapped.insert_str(mapped.rfind("ENDSEC;").unwrap(), &extra);
    let imported = solidcraft_io::step_import_feature(mapped.as_bytes(), "mapped.step").unwrap();
    assert_eq!(imported.import.bodies[0].geometry_item, imported.import.bodies[1].geometry_item);
    let mut s = Session::default();
    crate::cmd::file::import::add_in_component(
        &mut s,
        imported.kind,
        "Mapped bolts",
        imported.body_names,
        "Mapped bolts",
        crate::cmd::file::import::Tree::Step { import: &imported.import, share_parts: true },
    )
    .unwrap();
    let mut x: Vec<_> = s.world_state().bodies.iter().map(|b| solidcraft_kernel::measure(&b.body).unwrap().centroid.x).collect();
    x.sort_by(f64::total_cmp);
    assert_eq!(x.len(), 2);
    near(x[0], 1.);
    near(x[1], 11.);
}

#[test]
fn duplicate_step_definitions_share_geometry_and_survive_independence_and_source_only_reopen() {
    use solidcraft_doc::{FeatureKind, IDENTITY};
    use solidcraft_geom::Vec3;
    use solidcraft_kernel::{ExportBody, ExportProduct, StepHeader, step_export_products};
    use std::sync::Arc;
    let body = solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(2., 3., 4.)).unwrap();
    let at = |x| solidcraft_doc::rigid(Vec3::new(x, 0., 0.), Vec3::ZERO, Vec3::Z, 0.);
    let part = |name: &str, color| ExportProduct {
        name: name.into(),
        bodies: vec![ExportBody { name: "Bolt".into(), body: &body, color }],
        children: Vec::new(),
    };
    let products = vec![
        ExportProduct {
            name: "Assembly".into(),
            bodies: Vec::new(),
            children: vec![(1, IDENTITY, "Bolt:1".into()), (2, at(10.), "Bolt:2".into()), (3, at(20.), "Painted".into())],
        },
        part("Bolt", None),
        part("Bolt", None),
        part("Painted bolt", Some([1., 0., 0.])),
    ];
    let text = step_export_products(&products, 0, &StepHeader::default()).unwrap();
    let imported = solidcraft_io::step_import_feature(text.as_bytes(), "bolts.step").unwrap();
    let product = imported.import.tree[0].children[1].definition;
    let with_properties = text.replace("END-ISO-10303-21;", "");
    let end = with_properties.rfind("ENDSEC;").unwrap();
    let with_properties = format!(
        "{}#1000000=PRODUCT_DEFINITION_SHAPE('','',#{product});\n#1000001=PROPERTY_DEFINITION('material','brass',#1000000);\nENDSEC;\nEND-ISO-10303-21;\n",
        &with_properties[..end]
    );
    let separate = solidcraft_io::step_import_feature(with_properties.as_bytes(), "properties.step").unwrap();
    let mut properties = Session::default();
    crate::cmd::file::import::add_in_component(
        &mut properties,
        separate.kind,
        "Properties",
        separate.body_names,
        "Properties",
        crate::cmd::file::import::Tree::Step { import: &separate.import, share_parts: true },
    )
    .unwrap();
    properties.refresh();
    assert_eq!(properties.model.state().bodies.len(), 3, "an uninterpreted property through a shape definition keeps the part independent");
    let separate = solidcraft_io::step_import_feature(text.as_bytes(), "bolts.step").unwrap();
    let mut authored = Session::default();
    crate::cmd::file::import::add_in_component(
        &mut authored,
        separate.kind,
        "Bolts",
        separate.body_names,
        "Bolts",
        crate::cmd::file::import::Tree::Step { import: &separate.import, share_parts: false },
    )
    .unwrap();
    let first = authored.doc.occurrences.iter().find(|o| o.name == "Bolt:1").unwrap();
    let second = authored.doc.occurrences.iter().find(|o| o.name == "Bolt:2").unwrap();
    assert_ne!(first.component, second.component);
    let (first, second, component) = (authored.doc.first_path(first.component), authored.doc.first_path(second.component), first.component);
    let world = authored.world_state();
    assert_eq!(world.bodies.len(), 3);
    let meshes: Vec<_> = world.bodies.iter().map(|b| b.mesh_in_place().0).collect();
    assert!(meshes.windows(2).all(|m| Arc::ptr_eq(&m[0], &m[1])), "independent edit identities still share display geometry");
    run(&mut authored, "component.activate", json!({"component":component}));
    run(&mut authored, "solid.box", json!({"length":1,"width":1,"height":1,"corner":[50,0,0]}));
    assert_eq!(authored.occurrence_bodies(&first, false).unwrap().len(), 2);
    assert_eq!(authored.occurrence_bodies(&second, false).unwrap().len(), 1, "editing one definition leaves the other unchanged");
    let mut s = Session::default();
    let (_, placed) = crate::cmd::file::import::add_in_component(
        &mut s,
        imported.kind,
        "Bolts",
        imported.body_names,
        "Bolts",
        crate::cmd::file::import::Tree::Step { import: &imported.import, share_parts: true },
    )
    .unwrap();
    s.refresh();
    let parent = placed.component;
    let root_path = s.doc.first_path(parent);
    let bolts: Vec<_> = s.doc.occurrences.iter().filter(|o| o.parent == parent && o.name.starts_with("Bolt:")).cloned().collect();
    assert_eq!(bolts.len(), 2);
    assert_eq!(bolts[0].component, bolts[1].component);
    assert_eq!(s.model.state().bodies.len(), 2, "the coloured identity stays separate");
    let world = s.world_state();
    assert_eq!(world.bodies.len(), 3);
    let meshes: Vec<_> = world.bodies.iter().map(|b| b.mesh_in_place().0).collect();
    assert!(meshes.windows(2).all(|m| Arc::ptr_eq(&m[0], &m[1])), "all three appearances reuse one geometry resource");
    let mut path = root_path;
    path.push(bolts[1].id);
    let result = run(&mut s, "component.make_independent", json!({"path":path}));
    assert_ne!(result["component"], bolts[0].component);
    let sources: Vec<_> = s
        .doc
        .features
        .iter()
        .filter_map(|f| match &f.kind {
            FeatureKind::Import { step, .. } => Some(step),
            _ => None,
        })
        .collect();
    assert_eq!(sources.len(), 2);
    assert!(sources[0].same_source(sources[1]), "independence shares the source rather than re-exporting geometry");
    let bytes = solidcraft_io::write_design_container(&s.doc).unwrap();
    drop(s);
    let doc = solidcraft_io::read_design(&bytes).unwrap();
    let sources: Vec<_> = doc
        .features
        .iter()
        .filter_map(|f| match &f.kind {
            FeatureKind::Import { step, .. } => Some(step),
            _ => None,
        })
        .collect();
    assert!(sources[0].same_source(sources[1]), "reopen hydrates one authoritative source");
    let reopened = Session::new(doc);
    assert!(reopened.model.last_recomputed > 0, "source-only reopening evaluates the imports");
    let world = reopened.world_state();
    assert_eq!(world.bodies.len(), 3);
    let meshes: Vec<_> = world.bodies.iter().map(|b| b.mesh_in_place().0).collect();
    assert!(meshes.windows(2).all(|m| Arc::ptr_eq(&m[0], &m[1])));
    let mut volumes: Vec<_> = world.bodies.iter().map(|b| solidcraft_kernel::measure(&b.body).unwrap().volume).collect();
    volumes.sort_by(f64::total_cmp);
    for volume in volumes {
        near(volume, 24.);
    }
    let saved = reopened.model.saved(&reopened.doc).unwrap();
    let cached = solidcraft_io::write_design_container_with_cache(&reopened.doc, solidcraft_io::DesignCompression::Fast, None, Some(&saved)).unwrap();
    let mut reader = solidcraft_io::DesignContainerReader::new(std::io::Cursor::new(&cached)).unwrap();
    let geometry = reader.geometry().unwrap();
    let restored = Session::opened(reader.read_document().unwrap(), Some(&geometry));
    assert_eq!(restored.model.last_recomputed, 0, "the native cache avoids evaluating imports");
    let world = restored.world_state();
    let meshes: Vec<_> = world.bodies.iter().map(|b| b.mesh_in_place().0).collect();
    assert!(meshes.windows(2).all(|m| Arc::ptr_eq(&m[0], &m[1])), "cache adoption retains shared GPU sources");
}

#[test]
fn coincident_independent_primitives_keep_their_own_semantic_faces() {
    let mut s = Session::default();
    let first = box2(&mut s);
    let second = run(&mut s, "solid.box", json!({"length":2,"width":2,"height":2,"body_name":"Other"}))["feature"].as_u64().unwrap();
    let state = s.model.state();
    let a = state.body("Cube").unwrap();
    let b = state.body("Other").unwrap();
    let a_names = solidcraft_doc::naming::face_names(a);
    let b_names = solidcraft_doc::naming::face_names(b);
    assert!(a_names.contains(&format!("F{first}:+z")));
    assert!(b_names.contains(&format!("F{second}:+z")));
    assert!(!b_names.contains(&format!("F{first}:+z")));
}

#[test]
fn nested_definition_dag_and_histories_are_independent() {
    let mut s = Session::default();
    let parent = component(&mut s, "Assembly");
    let child = component(&mut s, "Child");
    let feature = box2(&mut s);
    run(&mut s, "occurrence.copy", json!({"component":child}));
    let copied = copy(&mut s, parent);
    let new = copied["component"].as_u64().unwrap();
    assert_eq!(copied["components"], 2);
    near(volume(&mut s, new), 16.0);
    let kids: Vec<_> = s.doc.occurrences.iter().filter(|o| o.parent == new).collect();
    assert_eq!(kids.len(), 2);
    assert_eq!(kids[0].component, kids[1].component);
    assert_ne!(kids[0].component, child);
    let parameter = s.doc.feature(feature).unwrap().param_names[0].clone();
    run(&mut s, "parameters.change", json!({"name":parameter,"expression":"3 mm"}));
    near(volume(&mut s, parent), 24.0);
    near(volume(&mut s, new), 16.0);
    let saved = s.doc.to_json();
    let reopened = solidcraft_doc::Document::from_json(&saved).unwrap();
    assert_eq!(reopened.occurrences.iter().filter(|o| o.parent == new).count(), 2);
}

#[test]
fn reassigned_body_is_an_independent_base_feature() {
    let mut s = Session::default();
    let feature = box2(&mut s);
    let part = component(&mut s, "Part");
    run(&mut s, "component.move_bodies", json!({"component":part,"bodies":["Cube"]}));
    let copied = copy(&mut s, part);
    let new = copied["component"].as_u64().unwrap();
    assert_eq!(copied["frozen_bodies"], json!(["Cube"]));
    near(volume(&mut s, new), 8.0);
    let parameter = s.doc.feature(feature).unwrap().param_names[0].clone();
    run(&mut s, "parameters.change", json!({"name":parameter,"expression":"3 mm"}));
    near(volume(&mut s, part), 12.0);
    near(volume(&mut s, new), 8.0);
}

#[test]
fn deleting_an_independent_base_keeps_the_copied_component() {
    let mut s = Session::default();
    box2(&mut s);
    let part = component(&mut s, "Part");
    run(&mut s, "component.move_bodies", json!({"component":part,"bodies":["Cube"]}));
    let new = copy(&mut s, part)["component"].as_u64().unwrap();
    let base = s.doc.features.iter().find(|f| f.component == new && matches!(f.kind, solidcraft_doc::FeatureKind::Import { .. })).unwrap();
    assert!(!base.inserted && !s.doc.feature_owns_component(base));
    let (base, others) = (base.id, s.doc.features.iter().filter(|f| f.component == new).count() - 1);
    let (components, occurrences, joints) = (s.doc.components.clone(), s.doc.occurrences.clone(), s.doc.assembly.joints.clone());
    let reply = run(&mut s, "timeline.delete", json!({"feature": base.to_string()}));
    assert_eq!(reply["deleted"], json!([base]));
    assert_eq!(s.doc.components, components);
    assert_eq!(s.doc.occurrences, occurrences);
    assert_eq!(s.doc.assembly.joints, joints);
    assert_eq!(s.doc.features.iter().filter(|f| f.component == new).count(), others);
}

#[test]
fn clone_preserves_physical_material_and_independent_styles() {
    let mut s = Session::default();
    let part = component(&mut s, "Part");
    box2(&mut s);
    run(&mut s, "material.assign", json!({"bodies":["Cube"],"material":"Brass"}));
    run(&mut s, "appearance.assign", json!({"components":[part.to_string()],"color":"#123456","opacity":0.4}));
    run(&mut s, "appearance.assign", json!({"bodies":["Cube"],"color":"#abcdef"}));
    run(&mut s, "appearance.assign", json!({"body":"Cube","faces":[[1,1,2]],"color":"#00ff00"}));
    let copied = copy(&mut s, part);
    let new = copied["component"].as_u64().unwrap();
    let props = run(&mut s, "component.properties", json!({"component":new}));
    near(props["total"]["mass_g"].as_f64().unwrap(), 0.068);
    let copied_body = s.model.state().bodies.iter().find(|b| b.component == new).unwrap().name.clone();
    assert_eq!(s.doc.appearances.bodies[&copied_body], s.doc.appearances.bodies["Cube"]);
    let state = s.model.state();
    let body = state.body(&copied_body).unwrap();
    assert!(s.doc.face_colors(body).iter().any(|(_, look)| look.color == [0, 255, 0]));
    run(&mut s, "appearance.assign", json!({"bodies":[copied_body],"color":"#ff0000"}));
    assert_eq!(s.doc.appearances.bodies["Cube"].color, [0xab, 0xcd, 0xef]);
    assert_eq!(s.doc.appearances.components[&new].opacity, 0.4);
}

#[test]
fn reassigned_body_owned_move_replays_once_and_original_stays_unchanged() {
    let mut s = Session::default();
    box2(&mut s);
    let part = component(&mut s, "Part");
    run(&mut s, "component.move_bodies", json!({"component":part,"bodies":["Cube"]}));
    run(&mut s, "solid.move", json!({"bodies":["Cube"],"translate":[10,0,0]}));
    let before = run(&mut s, "component.properties", json!({"component":part}));
    let copied = copy(&mut s, part);
    let new = copied["component"].as_u64().unwrap();
    let after = run(&mut s, "component.properties", json!({"component":new}));
    near(after["total"]["center_of_mass"][0].as_f64().unwrap(), 31.0);
    assert_eq!(run(&mut s, "component.properties", json!({"component":part})), before);
}

#[test]
fn copied_external_destructive_target_is_atomic() {
    let mut s = Session::default();
    box2(&mut s);
    let part = component(&mut s, "Part");
    run(&mut s, "solid.move", json!({"bodies":["Cube"],"translate":[10,0,0]}));
    let saved = s.doc.to_json();
    let selection = s.selection.clone();
    let active = s.active_component;
    let pending = s.pending_moves.clone();
    assert!(s.execute("component.paste_new", &json!({"component":part})).is_err());
    assert_eq!(s.doc.to_json(), saved);
    assert_eq!(s.selection, selection);
    assert_eq!(s.active_component, active);
    assert_eq!(s.pending_moves, pending);
}

#[test]
fn generated_feature_parameter_dependencies_are_independent() {
    let mut s = Session::default();
    let part = component(&mut s, "Part");
    let source = box2(&mut s);
    let source_names = s.doc.feature(source).unwrap().param_names.clone();
    run(&mut s, "parameters.change", json!({"name":source_names[1],"expression":format!("{} * 2",source_names[0])}));
    let result = copy(&mut s, part);
    let clone = result["component"].as_u64().unwrap();
    near(volume(&mut s, clone), 16.0);
    let clone_names = s
        .doc
        .features
        .iter()
        .find(|f| f.component == clone && matches!(f.kind, solidcraft_doc::FeatureKind::Box { .. }))
        .unwrap()
        .param_names
        .clone();
    assert_ne!(clone_names, source_names);
    run(&mut s, "parameters.change", json!({"name":source_names[0],"expression":"3 mm"}));
    near(volume(&mut s, part), 36.0);
    near(volume(&mut s, clone), 16.0);
    run(&mut s, "parameters.change", json!({"name":clone_names[0],"expression":"4 mm"}));
    near(volume(&mut s, clone), 64.0);
    near(volume(&mut s, part), 36.0);
}

#[test]
fn selected_nested_pending_placement_survives_copy_and_undo() {
    let mut s = Session::default();
    let parent = component(&mut s, "Parent");
    let parent_first = s.doc.occurrence_of(parent).unwrap().id;
    let child = component(&mut s, "Child");
    let leaf = s.doc.occurrence_of(child).unwrap().id;
    box2(&mut s);
    let second = run(&mut s, "occurrence.copy", json!({"component":parent,"translate":[50,0,0]}))["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence":leaf,"capture":false,"translate":[5,0,0]}));
    run(&mut s, "occurrence.move", json!({"occurrence":second,"capture":false,"translate":[2,0,0]}));
    let pending = s.pending_moves.clone();
    let prior_context = (s.active_component, s.active_occurrence_path.clone(), s.active_sketch);
    let copied = run(&mut s, "component.paste_new", json!({"component":child,"occurrence":leaf,"path":[second,leaf],"translate":[10,0,0]}));
    let new_leaf = copied["occurrence"].as_u64().unwrap();
    assert_eq!((s.active_component, s.active_occurrence_path.clone(), s.active_sketch), prior_context);
    assert_eq!(s.selection, vec![crate::Sel::Occurrence { path: vec![second, new_leaf] }]);
    assert_eq!(s.pending_moves, pending);
    assert_eq!(s.occurrence_placement(&[parent_first, leaf]).unwrap().world[3][0], 5.0);
    let props = run(&mut s, "occurrence.properties", json!({"occurrence":new_leaf,"path":[second,new_leaf]}));
    near(props["total"]["center_of_mass"][0].as_f64().unwrap(), 68.0);
    run(&mut s, "edit.undo", json!({}));
    assert!(!s.doc.occurrences.iter().any(|o| o.id == new_leaf));
    assert_eq!(s.pending_moves, pending);
}

#[test]
fn copied_sheet_base_targets_its_own_sketch_history() {
    let mut s = Session::default();
    let part = component(&mut s, "Sheet");
    let sketch = run(&mut s, "sketch.create", json!({"plane":"XY","name":"Base"}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[10,10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"sketch":"Base"}));
    let source_volume = volume(&mut s, part);
    let copied = copy(&mut s, part);
    let new = copied["component"].as_u64().unwrap();
    near(volume(&mut s, new), source_volume);
    let clone_sketch = s.doc.features.iter().find(|f| f.component == new && matches!(f.kind, solidcraft_doc::FeatureKind::Sketch { .. })).unwrap().id;
    assert_ne!(clone_sketch, sketch);
    let base = s.doc.features.iter().find(|f| f.component == new && matches!(f.kind, solidcraft_doc::FeatureKind::SheetBase { .. })).unwrap();
    assert_eq!(base.kind.input_sketches(), vec![clone_sketch]);
    // Mutate only source geometry, then verify the copied base remains unchanged.
    if let solidcraft_doc::FeatureKind::Sketch { sketch, .. } = &mut s.doc_mut().feature_mut(sketch).unwrap().kind {
        for p in &mut sketch.points {
            p.pos.x *= 2.0;
        }
    }
    s.refresh();
    near(volume(&mut s, new), source_volume);
}

#[test]
fn renamed_pattern_outputs_and_later_body_operands_follow_clone() {
    let mut s = Session::default();
    let part = component(&mut s, "PatternPart");
    box2(&mut s);
    run(&mut s, "solid.pattern.rectangular", json!({"bodies":["Cube"],"dir1":[1,0,0],"count1":3,"spacing1":5}));
    let last = s.model.state().bodies.last().unwrap().name.clone();
    run(&mut s, "body.rename", json!({"body":last,"name":"Custom Output"}));
    run(&mut s, "solid.move", json!({"bodies":["Custom Output"],"translate":[0,5,0]}));
    let copied = copy(&mut s, part);
    let new = copied["component"].as_u64().unwrap();
    near(volume(&mut s, new), 24.0);
    let state = s.model.state();
    let target = state.bodies.iter().find(|b| b.name == "Custom Output (2)").unwrap();
    near(solidcraft_kernel::measure(&target.body).unwrap().centroid.y, 6.0);
    assert_eq!(s.doc.features.iter().filter(|f| f.component == new && matches!(f.kind, solidcraft_doc::FeatureKind::Move { .. })).count(), 1);
}

#[test]
fn configuration_rows_follow_copied_parameters_material_and_strict_face_lineage() {
    let mut s = Session::default();
    let part = component(&mut s, "Configured");
    let source = box2(&mut s);
    let length = s.doc.feature(source).unwrap().param_names[0].clone();
    let target = solidcraft_doc::config::ConfigFaceTarget {
        body: "Cube".into(),
        origin_feature: source,
        name: format!("F{source}:+z"),
        point: solidcraft_geom::Vec3::new(1.0, 1.0, 2.0),
        certificate: None,
    };
    run(&mut s, "config.configure", json!({"params":[length],"materials":["Cube"],"component_appearances":[part],"face_appearances":[target]}));
    let face_key = s.doc.config_key(&solidcraft_doc::config::Column::FaceAppearance(target.clone()));
    run(
        &mut s,
        "config.row",
        json!({"name":"Large","values":{length.clone():"4 mm","material:Cube":"Brass",face_key:{"name":"Custom","color":[0,255,0],"opacity":1.0}}}),
    );
    let result = copy(&mut s, part);
    let new = result["component"].as_u64().unwrap();
    assert_eq!(s.doc.configs.columns.len(), 8);
    run(&mut s, "config.activate", json!({"row":"Large"}));
    near(volume(&mut s, part), 16.0);
    near(volume(&mut s, new), 16.0);
    let props = run(&mut s, "component.properties", json!({"component":new}));
    near(props["total"]["mass_g"].as_f64().unwrap(), 0.136);
    let state = s.model.state();
    let body = state.bodies.iter().find(|b| b.component == new).unwrap();
    assert!(s.doc.face_colors(body).iter().any(|(_, look)| look.color == [0, 255, 0]));
    assert!(s.doc.configuration_style_status(&state).is_ok());
    assert!(s.doc.configuration_matches());
}

#[test]
fn configured_extrude_cap_copy_keeps_independent_semantic_lineage() {
    let mut s = Session::default();
    let part = component(&mut s, "Extruded");
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[2,2]}));
    run(&mut s, "sketch.finish", json!({}));
    let feature = run(&mut s, "solid.extrude", json!({"distance":2}))["feature"].as_u64().unwrap();
    let body_name = s.model.state().bodies.last().unwrap().name.clone();
    let target = solidcraft_doc::config::ConfigFaceTarget {
        body: body_name,
        origin_feature: feature,
        name: format!("F{feature}:top"),
        point: solidcraft_geom::Vec3::new(1.0, 1.0, 2.0),
        certificate: None,
    };
    run(&mut s, "config.configure", json!({"face_appearances":[target]}));
    let key = s.doc.config_key(&solidcraft_doc::config::Column::FaceAppearance(target));
    run(&mut s, "config.row", json!({"name":"Painted","values":{key:{"name":"Custom","color":[0,255,0],"opacity":1.0}}}));
    let copied = copy(&mut s, part)["component"].as_u64().unwrap();
    run(&mut s, "config.activate", json!({"row":"Painted"}));
    let state = s.model.state();
    let body = state.bodies.iter().find(|b| b.component == copied).unwrap();
    assert!(solidcraft_doc::naming::face_names(body).contains(&format!("F{}:top", body.feature)));
    assert!(s.doc.face_colors(body).iter().any(|(_, look)| look.color == [0, 255, 0]));
    assert!(s.doc.configuration_style_status(&state).is_ok());
    let saved = s.doc.to_json();
    let reopened = solidcraft_doc::Document::from_json(&saved).unwrap();
    let mut model = solidcraft_doc::Model::default();
    model.evaluate(&reopened);
    assert!(reopened.configuration_style_status(&model.state()).is_ok());
}

#[test]
fn selected_second_occurrence_emboss_copy_remaps_target_and_author_paths_and_reopens() {
    let mut s = Session::default();
    let part = component(&mut s, "SleevePart");
    let cylinder = run(&mut s, "solid.cylinder", json!({"radius":10,"height":20,"body_name":"Sleeve"}))["feature"].as_u64().unwrap();
    let second = run(&mut s, "occurrence.copy", json!({"component":part,"translate":[80,0,0]}))["occurrence"].as_u64().unwrap();
    run(&mut s, "component.activate", json!({"component":part,"occurrence":second,"path":[second]}));
    run(&mut s, "sketch.create", json!({"name":"Badge","plane":{"origin":[80,10,0],"x_dir":[-1,0,0],"y_dir":[0,0,1]}}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[-2,5],"p1":[2,10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.emboss", json!({"sketch":"Badge","face":[80,10,7.5],"depth":1}));
    let original_volume = volume(&mut s, part);
    let copied = run(&mut s, "component.paste_new", json!({"component":part,"occurrence":second,"path":[second],"translate":[20,0,0]}));
    let new = copied["component"].as_u64().unwrap();
    let placement = copied["occurrence"].as_u64().unwrap();
    let emboss = s.doc.features.iter().find(|f| f.component == new && matches!(f.kind, solidcraft_doc::FeatureKind::Emboss { .. })).unwrap();
    let solidcraft_doc::FeatureKind::Emboss { face: Some(face), .. } = &emboss.kind else { panic!("missing selected face") };
    assert_eq!(face.body, "Sleeve (2)");
    assert_eq!(face.path, vec![placement]);
    assert_eq!(face.author_path, vec![placement]);
    near(volume(&mut s, new), original_volume);
    let clone_volume = volume(&mut s, new);
    let height = s.doc.feature(cylinder).unwrap().param_names[1].clone();
    run(&mut s, "parameters.change", json!({"name":height,"expression":"30 mm"}));
    near(volume(&mut s, new), clone_volume);
    assert!(volume(&mut s, part) > original_volume);
    let mut reopened = Session { doc: solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap().into(), ..Session::default() };
    reopened.refresh();
    assert!(reopened.doc.features.iter().all(|f| reopened.model.result(f.id).is_some_and(|r| r.error.is_none())));
    near(volume(&mut reopened, new), clone_volume);
}

#[test]
fn reassigned_sample_plate_paste_new_preserves_complex_frozen_geometry() {
    let mut s = Session::default();
    s.run_script(&crate::sample::script()).unwrap();
    let body = s.model.state().bodies.first().unwrap().name.clone();
    let created = run(&mut s, "component.from_bodies", json!({"bodies":[body]}));
    let part = created["components"][0]["component"].as_u64().unwrap();
    run(&mut s, "component.activate", json!({"component":part}));
    let occurrence = s.doc.occurrence_of(part).unwrap().id;
    run(&mut s, "occurrence.ground", json!({"occurrence":occurrence,"grounded":true}));
    let original_volume = volume(&mut s, part);
    let source_mesh = s.model.state().body(&body).unwrap().mesh();
    let boss_probe = solidcraft_geom::Vec3::new(40.0, 35.0, 12.0);
    let source_boss = source_mesh.contains(boss_probe);
    let copied = run(&mut s, "component.paste_new", json!({"component":part,"translate":[10,10,0]}));
    assert_eq!(s.doc.components.len(), 2);
    let new = copied["component"].as_u64().unwrap();
    // Imported curved faces may tessellate differently; keep the same geometric
    // accuracy required by copy, and independently probe plate and hole material.
    assert!((volume(&mut s, new) - original_volume).abs() < 1e-5 * original_volume);
    near(volume(&mut s, part), original_volume);
    let state = s.model.state();
    let copied_body = state.bodies.iter().find(|body| body.component == new).unwrap();
    let mesh = copied_body.mesh();
    for (point, expected) in
        [([5.0, 25.0, 4.0], true), ([12.0, 25.0, 4.0], false), ([68.0, 25.0, 4.0], false), ([40.0, 25.0, 12.0], false), ([0.0, 0.0, 4.0], false)]
    {
        assert_eq!(mesh.contains(solidcraft_geom::Vec3::new(point[0], point[1], point[2])), expected, "{point:?}");
    }
    assert_eq!(mesh.contains(boss_probe), source_boss);
    assert_eq!(state.body(&body).unwrap().mesh().contains(boss_probe), source_boss);
    let mut reopened = Session { doc: solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap().into(), ..Session::default() };
    reopened.refresh();
    assert!((volume(&mut reopened, new) - original_volume).abs() < 1e-5 * original_volume);
}

#[test]
fn generated_copy_parameters_cannot_heal_an_unrelated_missing_reference() {
    let mut s = Session::default();
    let part = component(&mut s, "Valid");
    let valid = box2(&mut s);
    run(&mut s, "component.activate", json!({"component":0}));
    let kind = s.doc.feature(valid).unwrap().kind.clone();
    let broken = s.doc_mut().add_feature(kind, Some("Broken reference")).unwrap();
    let mut probe = (*s.doc).clone();
    let missing = probe.new_model_param("2 mm", "mm");
    let feature = s.doc_mut().feature_mut(broken).unwrap();
    feature.component = 0;
    if let solidcraft_doc::FeatureKind::Box { width, .. } = &mut feature.kind {
        *width = missing.into();
    }
    s.refresh();
    let original_error = s.model.result(broken).unwrap().error.clone();
    assert!(original_error.is_some());
    let saved = s.doc.to_json();
    let context = (s.active_component, s.active_occurrence_path.clone(), s.active_sketch);
    let selection = s.selection.clone();
    let pending = s.pending_moves.clone();
    let error = s.execute("component.paste_new", &json!({"component":part})).unwrap_err();
    assert!(error.to_string().contains("original feature's error state"), "{error}");
    assert_eq!(s.doc.to_json(), saved);
    assert_eq!(s.model.result(broken).unwrap().error, original_error);
    assert_eq!((s.active_component, s.active_occurrence_path.clone(), s.active_sketch), context);
    assert_eq!(s.selection, selection);
    assert_eq!(s.pending_moves, pending);
}

#[test]
fn external_transfer_entry_and_internal_rotated_transfer_clone_once() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":4,"width":3,"height":2,"body_name":"Foreign"}));
    let parent = component(&mut s, "Assembly");
    let a = component(&mut s, "A");
    let ao = s.doc.occurrence_of(a).unwrap().id;
    run(&mut s, "occurrence.move", json!({"occurrence":ao,"translate":[10,0,0],"angle":90,"axis":[0,0,1],"capture":true}));
    run(&mut s, "component.move_bodies", json!({"component":a,"bodies":["Foreign"]}));
    run(&mut s, "component.activate", json!({"component":parent}));
    let b = component(&mut s, "B");
    let bo = s.doc.occurrence_of(b).unwrap().id;
    run(&mut s, "occurrence.move", json!({"occurrence":bo,"translate":[0,20,0],"angle":90,"axis":[1,0,0],"capture":true}));
    run(&mut s, "component.move_bodies", json!({"component":b,"bodies":["Foreign"]}));
    run(&mut s, "solid.move", json!({"bodies":["Foreign"],"translate":[0,0,5]}));
    let before = s.model.state().body("Foreign").unwrap().mesh().bounds();
    let copied = copy(&mut s, parent);
    assert_eq!(copied["frozen_bodies"], json!(["Foreign"]));
    let state = s.model.state();
    let body = state.body("Foreign (2)").unwrap();
    let after = body.mesh().bounds();
    assert!(before.min.dist(after.min) < 1e-6);
    assert!(before.max.dist(after.max) < 1e-6);
    assert!(body.transferred);
    let mut reopened = Session { doc: solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap().into(), ..Session::default() };
    reopened.refresh();
    assert!(reopened.model.state().body("Foreign (2)").unwrap().mesh().bounds().min.dist(before.min) < 1e-6);
}

#[test]
fn suppressed_external_entry_does_not_supply_clone_baseline() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":4,"width":3,"height":2,"body_name":"Foreign"}));
    let part = component(&mut s, "Part");
    let occurrence = s.doc.occurrence_of(part).unwrap().id;
    run(&mut s, "occurrence.move", json!({"occurrence":occurrence,"translate":[25,0,0],"angle":90,"axis":[0,0,1],"capture":true}));
    let skipped = s
        .doc_mut()
        .add_feature(
            solidcraft_doc::FeatureKind::BodyTransfer { bodies: vec!["Foreign".into()], from: 0, to: part, transform: solidcraft_doc::IDENTITY },
            None,
        )
        .unwrap();
    s.doc_mut().feature_mut(skipped).unwrap().suppressed = true;
    s.refresh();
    run(&mut s, "component.move_bodies", json!({"component":part,"bodies":["Foreign"]}));
    let before = s.model.state().body("Foreign").unwrap().mesh().bounds();
    copy(&mut s, part);
    let after = s.model.state().body("Foreign (2)").unwrap().mesh().bounds();
    assert!(before.min.dist(after.min) < 1e-6);
    assert!(before.max.dist(after.max) < 1e-6);
}

#[test]
fn transferred_frozen_face_paint_ignores_consumed_legacy_offset() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":2,"width":2,"height":2,"body_name":"Foreign"}));
    run(&mut s, "appearance.assign", json!({"body":"Foreign","faces":[[1,1,2]],"color":"#00ff00"}));
    s.doc_mut().body_offsets.insert(
        "Foreign".into(),
        solidcraft_doc::rigid(solidcraft_geom::Vec3::new(7.0, 0.0, 0.0), solidcraft_geom::Vec3::ZERO, solidcraft_geom::Vec3::Z, 0.0),
    );
    s.refresh();
    let part = component(&mut s, "Part");
    run(&mut s, "component.move_bodies", json!({"component":part,"bodies":["Foreign"]}));
    let original = s.doc.appearances.faces.iter().find(|face| face.body == "Foreign").unwrap();
    let expected = solidcraft_doc::naming::point_on_face(&s.model.state(), original.name.as_ref().unwrap(), original.point).unwrap().0;
    copy(&mut s, part);
    let paint = s.doc.appearances.faces.iter().find(|face| face.body == "Foreign (2)").unwrap();
    assert!(paint.point.dist(expected) < 1e-6, "{:?} versus {expected:?}", paint.point);
}

#[test]
fn boundary_material_proof_requires_resolvable_stock_and_void() {
    use solidcraft_geom::Vec3;
    let a = solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(10., 10., 10.)).unwrap().tessellate(1e-3).unwrap();
    let b = solidcraft_kernel::box_solid(Vec3::new(1e-5, 0., 0.), Vec3::new(10.00001, 10., 10.)).unwrap().tessellate(1e-3).unwrap();
    let p = Vec3::new(5e-6, 5., 5.);
    assert_ne!(a.contains(p), b.contains(p));
    assert!((super::mesh_distance(&a, p) - 5e-6).abs() < 1e-12);
    assert!((super::mesh_distance(&b, p) - 5e-6).abs() < 1e-12);
    assert!(super::material_probe_matches(&a, &b, p, 1e-3, 1e-3));
    let shifted = solidcraft_kernel::box_solid(Vec3::new(1., 0., 0.), Vec3::new(11., 10., 10.)).unwrap().tessellate(1e-3).unwrap();
    assert!(!super::material_probe_matches(&a, &shifted, p, 1e-3, 1e-3));
    let thin = solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(10., 10., 1e-4)).unwrap().tessellate(1e-6).unwrap();
    let thin_shifted = solidcraft_kernel::box_solid(Vec3::new(0., 0., 1e-4), Vec3::new(10., 10., 2e-4)).unwrap().tessellate(1e-6).unwrap();
    // Every resolvable replacement is void: thin ambiguous material is refused.
    assert!(!super::material_probe_matches(&thin, &thin_shifted, Vec3::new(5., 5., 5e-5), 1e-3, 1e-3));
    assert!(!super::material_probe_matches(&a, &b, p, f64::INFINITY, 1e-3));
}

#[test]
fn frozen_bore_boundary_copy_preserves_stock_void_and_reopen() {
    use solidcraft_geom::Vec3;
    let bore = |center: f64| {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length":30,"width":20,"height":10,"body_name":"Drilled"}));
        run(&mut s, "solid.cylinder", json!({"base":[center,10,0],"radius":4,"height":10,"operation":"cut"}));
        s
    };
    let mut s = bore(10.);
    let source = s.model.state().body("Drilled").unwrap().body.tessellate(1e-3).unwrap();
    // The exact R4 wall passes through x6; a chord may end either side of that
    // cardinal angle, so its distance is bounded by the requested sag tolerance.
    assert!(super::mesh_distance(&source, Vec3::new(6., 10., 5.)) <= 1e-3);
    let shifted = bore(11.).model.state().body("Drilled").unwrap().body.tessellate(1e-3).unwrap();
    let lost = solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(30., 20., 10.)).unwrap().tessellate(1e-3).unwrap();
    assert!(!super::material_probe_matches(&source, &shifted, Vec3::new(6.5, 10., 5.), 1e-3, 1e-3));
    assert!(!super::material_probe_matches(&source, &lost, Vec3::new(10., 10., 5.), 1e-3, 1e-3));
    let created = run(&mut s, "component.from_bodies", json!({"bodies":["Drilled"]}));
    let part = created["components"][0]["component"].as_u64().unwrap();
    let copied = copy(&mut s, part)["component"].as_u64().unwrap();
    let check = |session: &Session| {
        let state = session.model.state();
        let body = state.bodies.iter().find(|body| session.doc.model_body_component(body) == copied).unwrap();
        let mesh = body.body.tessellate(1e-3).unwrap();
        for (point, stock) in
            [(Vec3::new(5.5, 10., 5.), true), (Vec3::new(6.5, 10., 5.), false), (Vec3::new(10., 10., 5.), false), (Vec3::new(18., 10., 5.), true)]
        {
            assert_eq!(mesh.contains(point), stock, "{point:?}");
        }
    };
    check(&s);
    check(&Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap()));
}

#[test]
fn frozen_r6_bore_copy_uses_current_geometry_despite_mesh_measure_bias() {
    use solidcraft_geom::Vec3;
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":20,"width":20,"height":10,"body_name":"Drilled"}));
    run(&mut s, "solid.cylinder", json!({"base":[10,10,0],"radius":6,"height":10,"operation":"cut"}));
    let source = s.model.state().body("Drilled").unwrap().body.clone();
    let original = solidcraft_kernel::step_export(&[&source], "source").unwrap();
    let created = run(&mut s, "component.from_bodies", json!({"bodies":["Drilled"]}));
    let part = created["components"][0]["component"].as_u64().unwrap();
    let before = s.doc.to_json();
    let copied = copy(&mut s, part)["component"].as_u64().unwrap();
    let check = |session: &Session| {
        let state = session.model.state();
        let body = state.bodies.iter().find(|b| session.doc.model_body_component(b) == copied).unwrap();
        assert_eq!(solidcraft_kernel::certified_prism_equivalent(&source, &body.body).unwrap(), Some(true));
        let mesh = body.body.tessellate(1e-3).unwrap();
        for (point, material) in
            [(Vec3::new(3., 10., 5.), true), (Vec3::new(5., 10., 5.), false), (Vec3::new(10., 10., 5.), false), (Vec3::new(18., 10., 5.), true)]
        {
            assert_eq!(mesh.contains(point), material, "{point:?}");
        }
        let original_body = state.body("Drilled").unwrap();
        assert_eq!(solidcraft_kernel::step_export(&[&original_body.body], "source").unwrap(), original);
    };
    check(&s);
    check(&Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap()));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), before);
    run(&mut s, "edit.redo", json!({}));
    check(&s);
}

/// A STEP file of three 10 mm boxes at x = 0, 30 and 60 (`Aa`, `Bb`, `Cc`). With `shared` the three
/// solids carry one label (`X`), as the parts of many assembly files do.
fn three_box_step(tag: &str, shared: bool) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("solidcraft-paste-new-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = Session::default();
    for (i, label) in ["Aa", "Bb", "Cc"].iter().enumerate() {
        let corner = [30.0 * i as f64, 0.0, 0.0];
        run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"corner":corner,"operation":"new_body","body_name":label}));
    }
    let path = dir.join("Three.step");
    run(&mut s, "file.export", json!({"path":path.to_string_lossy()}));
    if shared {
        let mut text = std::fs::read_to_string(&path).unwrap();
        for label in ["Aa", "Bb", "Cc"] {
            assert!(text.contains(&format!("MANIFOLD_SOLID_BREP('{label}'")), "the file names its solids: {label}");
            text = text.replace(&format!("MANIFOLD_SOLID_BREP('{label}'"), "MANIFOLD_SOLID_BREP('X'");
        }
        std::fs::write(&path, text).unwrap();
    }
    path
}

fn body_names(s: &Session) -> Vec<String> {
    s.model.state().bodies.iter().map(|b| b.name.clone()).collect()
}

/// Where the bodies of component `part` are: the lowest corner of each, rounded, in model order.
fn corners(s: &Session, part: u64) -> Vec<Vec<f64>> {
    let state = s.model.state();
    state
        .bodies
        .iter()
        .filter(|b| s.doc.model_body_component(b) == part)
        .map(|b| {
            let min = b.mesh().bounds().min;
            vec![min.x.round(), min.y.round(), min.z.round()]
        })
        .collect()
}

/// Lifts the first of the three bodies by 20 and the second by 50, as two features of the active
/// component.
fn lift_two(s: &mut Session, bodies: [&str; 3]) {
    run(s, "solid.move", json!({"bodies":[bodies[0]],"translate":[0,0,20],"name":"Lift first"}));
    run(s, "solid.move", json!({"bodies":[bodies[1]],"translate":[0,0,50],"name":"Lift second"}));
}

const LIFTED: [[f64; 3]; 3] = [[0.0, 0.0, 20.0], [30.0, 0.0, 50.0], [60.0, 0.0, 0.0]];

fn lifted() -> Vec<Vec<f64>> {
    LIFTED.iter().map(|c| c.to_vec()).collect()
}

/// A component whose Import feature holds the three solids of `path`. `file.insert_step` puts its
/// feature into the root even while a component is active, so the feature is moved into it afterwards.
fn component_with_import(s: &mut Session, path: &std::path::Path) -> u64 {
    let part = component(s, "Part");
    let import = run(s, "file.insert_step", json!({"path":path.to_string_lossy()}))["feature"].as_u64().unwrap();
    s.doc_mut().feature_mut(import).unwrap().component = part;
    s.refresh();
    part
}

/// A control: solids with labels of their own are built as they are labelled.
#[test]
fn paste_new_of_a_component_with_an_import_moves_the_copy_of_each_body() {
    let path = three_box_step("distinct", false);
    let mut s = Session::default();
    let part = component_with_import(&mut s, &path);
    assert_eq!(body_names(&s), ["Aa", "Bb", "Cc"]);
    lift_two(&mut s, ["Aa", "Bb", "Cc"]);
    let copied = run(&mut s, "component.paste_new", json!({"component":part}))["component"].as_u64().unwrap();
    assert_eq!(corners(&s, part), lifted());
    assert_eq!(corners(&s, copied), lifted());
}

/// Solids of a STEP file that share one label are stored as `X`, `X`, `X` and built as `X`,
/// `X (2)`, `X (3)`, and the features made on them name them so. The copy of the component has
/// those features act on its own bodies, each on its own.
#[test]
fn paste_new_of_an_import_with_one_label_on_all_solids_moves_the_copy_of_each_body() {
    let path = three_box_step("shared", true);
    let mut s = Session::default();
    let part = component_with_import(&mut s, &path);
    assert_eq!(body_names(&s), ["X", "X (2)", "X (3)"]);
    lift_two(&mut s, ["X", "X (2)", "X (3)"]);
    let copied = run(&mut s, "component.paste_new", json!({"component":part}))["component"].as_u64().unwrap();
    assert_eq!(corners(&s, part), lifted());
    assert_eq!(corners(&s, copied), lifted(), "{:?}", body_names(&s));
    // The copy keeps its bodies and their places when the design is saved and opened again.
    let names = body_names(&s);
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert_eq!(body_names(&reopened), names);
    assert_eq!(corners(&reopened, copied), lifted());
}

/// No shared label is needed: a design linked a second time builds `Aa (2)`, `Bb (2)`, `Cc (2)`
/// from the names `Aa`, `Bb`, `Cc`, and the features made on those bodies name them so.
#[test]
fn paste_new_of_a_design_linked_twice_moves_the_copy_of_each_body() {
    let dir = std::env::temp_dir().join(format!("solidcraft-paste-new-linked-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let design = dir.join("Three.solidcraft");
    let mut part = Session::default();
    for (i, label) in ["Aa", "Bb", "Cc"].iter().enumerate() {
        let corner = [30.0 * i as f64, 0.0, 0.0];
        run(&mut part, "solid.box", json!({"length":10,"width":10,"height":10,"corner":corner,"operation":"new_body","body_name":label}));
    }
    run(&mut part, "file.save_as", json!({"path":design.to_string_lossy()}));
    let mut s = Session::default();
    run(&mut s, "component.insert_design", json!({"path":design.to_string_lossy()}));
    let second = run(&mut s, "component.insert_design", json!({"path":design.to_string_lossy()}));
    let linked = second["component"].as_u64().unwrap();
    assert_eq!(body_names(&s), ["Aa", "Bb", "Cc", "Aa (2)", "Bb (2)", "Cc (2)"]);
    run(&mut s, "material.assign", json!({"bodies":["Aa"],"material":"Steel"}));
    run(&mut s, "material.assign", json!({"bodies":["Aa (2)"],"material":"Brass"}));
    run(&mut s, "component.activate", json!({"component":linked}));
    lift_two(&mut s, ["Aa (2)", "Bb (2)", "Cc (2)"]);
    let copied = run(&mut s, "component.paste_new", json!({"component":linked}))["component"].as_u64().unwrap();
    assert_eq!(corners(&s, linked), lifted());
    assert_eq!(corners(&s, copied), lifted(), "{:?}", body_names(&s));
    // The material of a body goes to its copy, and no material is kept for a body that does not exist.
    let names = body_names(&s);
    assert!(s.doc.materials.contains_key("Aa (2)(2)"));
    assert_eq!(s.doc.materials.get("Aa (2)(2)"), s.doc.materials.get("Aa (2)"));
    assert!(s.doc.materials.keys().all(|body| names.contains(body)), "{:?} for {names:?}", s.doc.materials.keys().collect::<Vec<_>>());
}

/// An OBJ file of three 10 mm cubes at x = 0, 30 and 60, every object called `X`.
fn three_cube_obj(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("solidcraft-paste-new-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let corners = [[0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0], [0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]];
    let faces = [[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [3, 7, 6], [3, 6, 2], [0, 4, 7], [0, 7, 3], [1, 2, 6], [1, 6, 5]];
    let mut text = String::new();
    for i in 0..3 {
        text += "o X\n";
        for c in corners {
            text += &format!("v {} {} {}\n", 30 * i + 10 * c[0], 10 * c[1], 10 * c[2]);
        }
        for f in faces {
            text += &format!("f {} {} {}\n", 8 * i + f[0] + 1, 8 * i + f[1] + 1, 8 * i + f[2] + 1);
        }
    }
    let path = dir.join("Three.obj");
    std::fs::write(&path, text).unwrap();
    path
}

/// The same for the objects of an OBJ file (3MF and STL files give the same kind of import).
#[test]
fn paste_new_of_a_mesh_import_with_one_label_on_all_objects_moves_the_copy_of_each_body() {
    let path = three_cube_obj("shared-obj");
    let mut s = Session::default();
    let part = component(&mut s, "Part");
    let import = run(&mut s, "file.insert_mesh", json!({"path":path.to_string_lossy()}))["feature"].as_u64().unwrap();
    s.doc_mut().feature_mut(import).unwrap().component = part;
    s.refresh();
    assert_eq!(body_names(&s), ["X", "X (2)", "X (3)"]);
    lift_two(&mut s, ["X", "X (2)", "X (3)"]);
    let copied = run(&mut s, "component.paste_new", json!({"component":part}))["component"].as_u64().unwrap();
    assert_eq!(corners(&s, part), lifted());
    assert_eq!(corners(&s, copied), lifted(), "{:?}", body_names(&s));
}
