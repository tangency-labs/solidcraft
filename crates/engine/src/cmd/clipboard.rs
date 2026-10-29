//! Copy and paste of timeline features. Pasted features are independent copies: sketches get
//! their own dimension parameters, feature inputs their own names, and references inside the
//! copied set point at the copies.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use solidcraft_doc::{FeatureKind, PlaneRef};

use super::CommandSpec;
use crate::params::{bad, name_or_id_values, vec3};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("timeline.copy", "Copy Features", copy).icon("copy").noundo().params("features: [ids or names] (in timeline order on paste)"),
    CommandSpec::new("timeline.paste", "Paste Features", paste).icon("paste").params(
        "translate?: [x,y,z] (moves the copies' geometry); component?: where the copies go (default: the active one), \
         component_path?: its placement (default: the active one, else its only one); the copies go at the marker, each where \
         its original is in the world (copies into their own component keep its coordinates); body transfers are left out; \
         from_design?: true to paste sketch features from the host's cross-design clipboard snapshot",
    ),
];

fn copy(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "timeline.copy";
    // The features are named or numbered, and both kinds can sit in one list.
    let keys = name_or_id_values(cmd, p, "features")?;
    if keys.is_empty() || keys.len() > 1000 {
        return Err(bad(cmd, "`features` must list 1…1000 features"));
    }
    let ids = keys.iter().map(|key| super::edit::feature_id(s, Some(key), cmd)).collect::<Result<Vec<_>>>()?;
    // Keep timeline order so sketches come before the features that use them.
    let copied: Vec<solidcraft_doc::Feature> = s.doc.features.iter().filter(|f| ids.contains(&f.id)).cloned().collect();
    let names: Vec<String> = copied.iter().map(|f| f.name.clone()).collect();
    s.clipboard = copied;
    s.foreign_copy = Some(std::sync::Arc::new(crate::ForeignCopy::of_features(s)));
    Ok(json!({"copied": names}))
}

/// Origin planes become explicit planes so a translation can move them.
pub(in crate::cmd) fn explicit_planes(p: &mut PlaneRef) {
    match p {
        PlaneRef::Origin { name } => {
            if let Some(plane) = solidcraft_geom::Plane::named(name) {
                *p = PlaneRef::Custom { plane };
            }
        }
        PlaneRef::Offset { base, .. } | PlaneRef::ThroughPoint { base, .. } | PlaneRef::AtAngle { base, .. } | PlaneRef::Flipped { base, .. } => {
            explicit_planes(base)
        }
        _ => {}
    }
}

fn paste(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "timeline.paste";
    match p.get("from_design") {
        Some(Value::Bool(true)) => return super::clipboard_foreign::paste(s, p),
        None | Some(Value::Bool(false)) => {}
        _ => return Err(bad(cmd, "`from_design` must be true or false")),
    }
    if s.clipboard.is_empty() {
        return Err(bad(cmd, "nothing copied (timeline.copy first)"));
    }
    let translate = match p.get("translate") {
        Some(v) => Some(vec3(v).ok_or_else(|| bad(cmd, "`translate` must be [x, y, z]"))?),
        None => None,
    };
    // A body transfer records which component bodies moved to, but the copies go into the
    // component they are pasted into: a copy of the transfer would move nothing that is there, and
    // it would belong to its destination rather than to the target. It is left out.
    let (transfers, feats): (Vec<_>, Vec<_>) = s.clipboard.iter().cloned().partition(|f| matches!(f.kind, FeatureKind::BodyTransfer { .. }));
    if feats.is_empty() {
        return Err(bad(cmd, "body transfers are not pasted: the copies go into the component they are pasted into"));
    }
    let comp = match p.get("component") {
        None => s.active_component,
        Some(v) => {
            let key = match v {
                Value::Number(n) => n.to_string(),
                Value::String(name) => name.clone(),
                _ => return Err(bad(cmd, "`component` must be a component id or name")),
            };
            s.doc.find_component(&key).ok_or_else(|| bad(cmd, format!("no component `{key}`")))?
        }
    };
    let given_path = match p.get("component_path") {
        None => None,
        Some(v) => Some(
            v.as_array()
                .filter(|a| a.len() <= 64)
                .and_then(|a| a.iter().map(Value::as_u64).collect::<Option<Vec<_>>>())
                .ok_or_else(|| bad(cmd, "`component_path` must list 0..64 occurrence ids"))?,
        ),
    };
    let mut sketches: BTreeMap<u64, u64> = BTreeMap::new();
    let mut bodies: BTreeMap<String, String> = BTreeMap::new();
    let mut used: std::collections::BTreeSet<String> = s.model.state().bodies.iter().map(|body| body.name.clone()).collect();
    let mut notices: Vec<String> = transfers.iter().map(|f| format!("{} is a body transfer and was not pasted", f.name)).collect();
    // The placement the copies are authored in: the one named, the active one (into the active
    // component, as before), else the target's only (or active) placement.
    let author_path = match given_path {
        Some(path) => path,
        None if comp == s.active_component => s.active_occurrence_path.clone().unwrap_or_else(|| s.doc.first_path(comp)),
        None => super::component::placing::placement_of(s, &s.placed_document(), comp, None, "component_path", cmd)?.0,
    };
    solidcraft_doc::EmbossFace::component_frame(&s.placed_document(), comp, &author_path)?;
    // A copy from another component goes where its original is in the world: its geometry moves
    // from its component's frame (its first placement, where its bodies have their own names)
    // into the target's.
    let placed = s.placed_document();
    let target_frame = placed.path_transform(comp, &author_path).ok_or_else(|| bad(cmd, "that path is not a placement of the target"))?;
    let to_target = solidcraft_doc::mat_inverse(&target_frame).ok_or_else(|| bad(cmd, "the target's frame is not invertible"))?;
    let frame_change: BTreeMap<u64, solidcraft_doc::Mat> = feats
        .iter()
        .map(|f| f.component)
        .filter(|from| *from != comp)
        .map(|from| {
            let source = placed.path_transform(from, &placed.first_path(from)).unwrap_or(solidcraft_doc::IDENTITY);
            (from, solidcraft_doc::mat_mul(&to_target, &source))
        })
        .collect();
    drop(placed);
    let mut shift = solidcraft_doc::IDENTITY;
    if let Some(t) = translate {
        shift[3][0] = t.x;
        shift[3][1] = t.y;
        shift[3][2] = t.z;
    }
    let mut names: BTreeMap<String, String> = BTreeMap::new();
    let mut made = Vec::new();
    for f in feats {
        let mut kind = f.kind.clone();
        kind.remap_refs(&sketches, &names);
        let m = solidcraft_doc::mat_mul(&shift, frame_change.get(&f.component).unwrap_or(&solidcraft_doc::IDENTITY));
        let moved = m.iter().enumerate().any(|(i, row)| row.iter().enumerate().any(|(j, v)| (v - if i == j { 1.0 } else { 0.0 }).abs() > 1e-12));
        // A body made by an earlier copy is the one this copy acts on (#613).
        kind.remap_bodies(&bodies);
        if let FeatureKind::Sketch { plane, .. } = &mut kind
            && let Some(anchor) = super::sketch::face::anchor_mut(plane)
        {
            let internal = bodies.contains_key(&anchor.target.body);
            if !internal && moved {
                return Err(bad(
                    cmd,
                    "a moved paste (translated, or into a component placed elsewhere) retains an external exact face attachment; \
                     explicitly convert it to a fixed plane first",
                ));
            }
            if !internal {
                notices.push(format!("pasted sketch retains external exact face attachment to {}", anchor.target.body));
            }
            if internal {
                let source = solidcraft_doc::mat_mul(&shift, frame_change.get(&anchor.component).unwrap_or(&solidcraft_doc::IDENTITY));
                anchor.reframe_placement(&m, &source)?;
            }
            anchor.remap_target(&sketches, &bodies)?;
            if internal {
                anchor.component = comp;
                anchor.path = author_path.clone();
            }
            anchor.author_path = author_path.clone();
            let target = anchor.target.clone();
            if let FeatureKind::Sketch { sketch, .. } = &mut kind {
                for link in &mut sketch.links {
                    if link.auto_face {
                        if let solidcraft_sketch::LinkSource::Face { body, .. } = &mut link.source
                            && let Some(mapped) = bodies.get(body)
                        {
                            *body = mapped.clone();
                        }
                        link.face_name = Some(target.name.clone());
                    }
                }
            }
        }
        let built: Vec<String> = s
            .model
            .result(f.id)
            .map(|result| result.state.bodies.iter().filter(|body| body.feature == f.id).map(|body| body.name.clone()).collect())
            .unwrap_or_default();
        let outputs = if f.body_names.is_empty() { built.clone() } else { f.body_names.clone() };
        // A later feature names a body as the model shows it, and that is not always the name stored
        // on the feature: solids that share a label are stored as `X`, `X`, `X` and shown as `X`,
        // `X (2)`, `X (3)`, and a second import of a file shows `Aa (2)` for the stored `Aa`. The
        // copies are found by the shown names; the stored ones only name the copies.
        let shown = if built.len() == outputs.len() { built } else { outputs.clone() };
        // Each copy is named after the body as shown, numbered: `Aa` → `Aa (2)`, `Aa (2)` → `Aa (2)(2)`.
        let copied_names: Vec<_> = shown
            .iter()
            .map(|shown| {
                let name = solidcraft_doc::copy_name(shown, |n| used.contains(n));
                used.insert(name.clone());
                bodies.insert(shown.clone(), name.clone());
                name
            })
            .collect();
        let doc = s.doc_mut();
        if let FeatureKind::Sketch { sketch, .. } = &mut kind {
            // Each dimension of the copy gets its own parameter, with the same expression.
            for c in &mut sketch.constraints {
                if let Some(pn) = c.param.clone() {
                    let (e, u) = doc.param(&pn).map(|x| (x.expr.clone(), x.unit.clone())).unwrap_or_else(|| (pn.clone(), "mm".into()));
                    c.param = Some(doc.new_model_param(&e, &u));
                }
            }
        }
        if moved {
            if let FeatureKind::Sketch { plane, .. } | FeatureKind::ConstructionPlane { plane } = &mut kind {
                explicit_planes(plane);
            }
            kind.transform_geometry(&m);
        }
        // A cylinder Emboss finds its face only by the persistent name recorded when it was made
        // (#1035): the copy keeps that name, pointed at the pasted features and bodies.
        let face_names: Vec<String> = if matches!(kind, FeatureKind::Emboss { face: Some(_), .. }) {
            f.face_names.iter().map(|n| solidcraft_doc::remap_topology(n, &sketches, &bodies)).collect()
        } else {
            Vec::new()
        };
        // A copy keeps its original's name, numbered: `Sketch1` → `Sketch1 (2)`.
        let name = solidcraft_doc::copy_name(&f.name, |n| doc.features.iter().any(|x| x.name == n));
        let id = doc.add_feature(kind, Some(&name))?;
        if let Some(nf) = doc.feature_mut(id) {
            nf.component = comp;
            nf.body_names = copied_names;
            nf.face_names = face_names;
        }
        let new_name = doc.feature(id).map(|x| x.name.clone()).unwrap_or_default();
        sketches.insert(f.id, id);
        names.insert(f.name.clone(), new_name.clone());
        made.push(json!({"id": id, "name": new_name, "from": f.name}));
    }
    s.refresh();
    let errors: Vec<Value> = made
        .iter()
        .filter_map(|m| m["id"].as_u64())
        .filter_map(|id| s.model.result(id).and_then(|r| r.error.clone()).map(|e| json!({"feature": id, "error": e})))
        .collect();
    Ok(json!({"pasted": made, "errors": errors, "notices":notices}))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::Session;

    fn run(s: &mut Session, id: &str, p: serde_json::Value) -> serde_json::Value {
        match s.execute(id, &p) {
            Ok(v) => v,
            Err(e) => panic!("{id} {p}: {e}"),
        }
    }

    fn volume(s: &mut Session) -> f64 {
        run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap_or(f64::NAN)
    }

    fn dimensioned_sketch() -> Session {
        let mut s = Session::default();
        run(&mut s, "parameters.change", json!({"name": "base", "expression": "5 mm"}));
        run(&mut s, "parameters.change", json!({"name": "width", "expression": "base * 4"}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 10]}));
        run(&mut s, "sketch.constraint.coincident", json!({"a": "p1", "b": "origin"}));
        let width = run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": "width"}))["param"].as_str().unwrap().to_string();
        run(&mut s, "sketch.dimension", json!({"entities": ["l2"], "value": format!("{width} / 2")}));
        run(&mut s, "sketch.finish", json!({}));
        s
    }

    fn sketch_area(s: &Session, id: u64) -> f64 {
        s.model.state().sketch(id).unwrap().profiles.iter().map(|p| p.area).sum()
    }

    #[test]
    fn foreign_sketch_paste_keeps_parameter_graph_snapshot_and_undo() {
        let mut source = dimensioned_sketch();
        run(&mut source, "timeline.copy", json!({"features": ["S"]}));
        let mut target = dimensioned_sketch();
        run(&mut target, "document.units", json!({"units": "in"}));
        run(&mut target, "parameters.change", json!({"name": "base", "expression": "7 mm"}));
        target.foreign_copy = source.foreign_copy.clone();
        run(&mut source, "parameters.change", json!({"name": "base", "expression": "30 mm"}));
        drop(source);
        let before = target.doc.to_json();
        let undo = target.history.len();
        let result = run(&mut target, "timeline.paste", json!({"from_design": true}));
        let id = result["pasted"][0]["id"].as_u64().unwrap();
        assert_eq!(result["pasted"][0]["name"], "S (2)");
        assert!((sketch_area(&target, id) - 200.).abs() < 1e-6);
        assert_eq!(target.doc.param("base").unwrap().expr, "7 mm");
        assert_eq!(target.doc.param("base_2").unwrap().expr, "5 mm");
        assert_eq!(target.doc.param("width_2").unwrap().expr, "base_2 * 4");
        assert_eq!(target.history.len(), undo + 1);
        let after = target.doc.to_json();
        run(&mut target, "edit.undo", json!({}));
        assert_eq!(target.doc.to_json(), before);
        run(&mut target, "edit.redo", json!({}));
        assert_eq!(target.doc.to_json(), after);
        let second = run(&mut target, "timeline.paste", json!({"from_design": true}))["pasted"][0]["id"].as_u64().unwrap();
        assert!((sketch_area(&target, second) - 200.).abs() < 1e-6);
        run(&mut target, "parameters.change", json!({"name": "base_2", "expression": "10 mm"}));
        assert!((sketch_area(&target, id) - 800.).abs() < 1e-6);
        assert!((sketch_area(&target, second) - 200.).abs() < 1e-6, "each paste owns its parameters");
        assert!(solidcraft_doc::Document::from_json(&target.doc.to_json()).is_ok());
    }

    #[test]
    fn foreign_sketch_paste_preserves_world_position_in_a_placed_component() {
        use solidcraft_geom::Vec3;

        let mut source = dimensioned_sketch();
        let arc = [Vec3::new(1., 0., 0.), Vec3::new(0., 0., 1.), Vec3::new(-1., 0., 0.)];
        for space in [solidcraft_sketch::WireSpace::World, solidcraft_sketch::WireSpace::Component] {
            source.doc_mut().sketch_mut(1).unwrap().add_drawn_arc_in(&arc, space).unwrap();
        }
        source.refresh();
        run(&mut source, "timeline.copy", json!({"features": ["S"]}));
        let mut target = Session::default();
        let made = run(&mut target, "component.create", json!({"name": "Target", "activate": false}));
        let component = made["component"].as_u64().unwrap();
        let occurrence = made["occurrence"].as_u64().unwrap();
        run(&mut target, "occurrence.ground", json!({"occurrence": occurrence, "grounded": false}));
        run(&mut target, "occurrence.move", json!({"occurrence": occurrence, "translate": [100, 20, 30], "axis": [0, 0, 1], "angle": "90 deg"}));
        target.foreign_copy = source.foreign_copy.clone();
        let id = run(&mut target, "timeline.paste", json!({"from_design": true, "component": component, "component_path": [occurrence]}))["pasted"]
            [0]["id"]
            .as_u64()
            .unwrap();
        let original = source.world_state().sketch(1).unwrap().plane;
        let pasted = target.world_state().sketch(id).unwrap().plane;
        assert!(pasted.origin.dist(original.origin) < 1e-6, "{original:?} vs {pasted:?}");
        assert!(pasted.x.dist(original.x) < 1e-6);
        assert!(pasted.y.dist(original.y) < 1e-6);
        assert!((sketch_area(&target, id) - 200.).abs() < 1e-6);
        assert_eq!(target.doc.feature(id).unwrap().component, component);
        for (original, pasted) in
            source.world_state().sketch(1).unwrap().sketch.wires.iter().zip(&target.world_state().sketch(id).unwrap().sketch.wires)
        {
            for (a, b) in original.pts.iter().chain(&original.fit).zip(pasted.pts.iter().chain(&pasted.fit)) {
                assert!(a.dist(*b) < 1e-6, "wire {:?}: {a:?} vs {b:?}", original.space);
            }
            let (a, b) = (original.arc.unwrap(), pasted.arc.unwrap());
            assert!(a.center.dist(b.center) < 1e-6);
            assert!(a.normal.dist(b.normal) < 1e-6);
            assert!((a.radius - b.radius).abs() < 1e-6);
        }
        let moved = run(
            &mut target,
            "timeline.paste",
            json!({"from_design": true, "component": component, "component_path": [occurrence], "translate": [2, 3, 4]}),
        )["pasted"][0]["id"]
            .as_u64()
            .unwrap();
        let frame = target.placed_document().path_transform(component, &[occurrence]).unwrap();
        let delta = solidcraft_doc::apply_vector(&frame, Vec3::new(2., 3., 4.));
        for (original, pasted) in
            source.world_state().sketch(1).unwrap().sketch.wires.iter().zip(&target.world_state().sketch(moved).unwrap().sketch.wires)
        {
            for (a, b) in original.pts.iter().chain(&original.fit).zip(pasted.pts.iter().chain(&pasted.fit)) {
                assert!((*a + delta).dist(*b) < 1e-6, "translated wire {:?}: {a:?} vs {b:?}", original.space);
            }
        }
    }

    #[test]
    fn foreign_sketch_paste_refuses_external_references_and_bad_parameters_atomically() {
        let mut source = dimensioned_sketch();
        let mut target = dimensioned_sketch();
        // The same numeric ID in the target must never become the sketch's missing plane.
        if let solidcraft_doc::FeatureKind::Sketch { plane, .. } = &mut source.doc_mut().feature_mut(1).unwrap().kind {
            *plane = solidcraft_doc::PlaneRef::Construction { feature: 1.into() };
        }
        run(&mut source, "timeline.copy", json!({"features": ["S"]}));
        target.foreign_copy = source.foreign_copy.clone();
        let before = target.doc.to_json();
        let undo = target.history.len();
        let error = target.execute("timeline.paste", &json!({"from_design": true})).unwrap_err().to_string();
        assert!(error.contains("fixed plane"), "{error}");
        assert_eq!(target.doc.to_json(), before);
        assert_eq!(target.history.len(), undo);
        for params in
            [json!({"from_design":"yes"}), json!({"from_design":true,"translate":"bad"}), json!({"from_design":true,"component_path":["bad"]})]
        {
            assert!(target.execute("timeline.paste", &params).is_err());
            assert_eq!(target.doc.to_json(), before);
        }
    }

    #[test]
    fn foreign_sketch_paste_remaps_links_and_rejects_missing_source_sketches() {
        let mut source = dimensioned_sketch();
        run(&mut source, "sketch.create", json!({"plane":"XY","offset":5,"name":"Projected"}));
        run(&mut source, "sketch.project", json!({"refs":[{"sketch":1,"curve":"l1"}]}));
        run(&mut source, "sketch.finish", json!({}));
        let mut target = dimensioned_sketch();
        run(&mut source, "timeline.copy", json!({"features":["Projected"]}));
        target.foreign_copy = source.foreign_copy.clone();
        let before = target.doc.to_json();
        let err = target.execute("timeline.paste", &json!({"from_design":true})).unwrap_err().to_string();
        assert!(err.contains("external projected geometry"), "{err}");
        assert_eq!(before, target.doc.to_json());
        run(&mut source, "timeline.copy", json!({"features":["S","Projected"]}));
        target.foreign_copy = source.foreign_copy.clone();
        let result = run(&mut target, "timeline.paste", json!({"from_design":true}));
        let first = result["pasted"][0]["id"].as_u64().unwrap();
        let second = result["pasted"][1]["id"].as_u64().unwrap();
        let copied = target.doc.sketch(second).unwrap();
        assert!(matches!(copied.links[0].source, solidcraft_sketch::LinkSource::SketchCurve { sketch, .. } if sketch == first));
        assert!(target.model.result(second).unwrap().error.is_none());
    }

    #[test]
    fn foreign_sketch_paste_keeps_plane_input_units_and_text_parameters() {
        let mut source = Session::default();
        run(&mut source, "document.units", json!({"units":"in"}));
        run(&mut source, "parameters.add", json!({"name":"height","expression":"2","unit":""}));
        run(&mut source, "parameters.add", json!({"name":"Label","expression":"ABC","unit":"text"}));
        run(&mut source, "sketch.create", json!({"plane":"XY","offset":"height","name":"Text"}));
        run(&mut source, "sketch.text", json!({"text_expression":"Label + '-X'","at":[0,0],"height":"5 mm"}));
        run(&mut source, "sketch.finish", json!({}));
        run(&mut source, "timeline.copy", json!({"features":["Text"]}));
        let mut target = Session::default();
        run(&mut target, "parameters.add", json!({"name":"Label","expression":"other","unit":"text"}));
        run(&mut target, "parameters.add", json!({"name":"d1","expression":"reserved","unit":"text"}));
        target.foreign_copy = source.foreign_copy.clone();
        let id = run(&mut target, "timeline.paste", json!({"from_design":true}))["pasted"][0]["id"].as_u64().unwrap();
        assert!((target.model.state().sketch(id).unwrap().plane.origin.z - 50.8).abs() < 1e-9);
        assert_eq!(target.doc.text_param("Label").unwrap().text, "other");
        assert_eq!(target.doc.text_param("Label_2").unwrap().text, "ABC");
        assert_eq!(target.doc.text_param("d1").unwrap().text, "reserved");
        assert!(target.doc.param("d1").is_none());
        let state = target.model.state();
        let text = &state.sketch(id).unwrap().sketch.links[0].source;
        assert!(matches!(text, solidcraft_sketch::LinkSource::Text { text, .. } if text == "ABC-X"));
        assert!(solidcraft_doc::Document::from_json(&target.doc.to_json()).is_ok());
    }

    #[test]
    fn foreign_sketch_paste_copies_equation_parameters_but_not_the_local_variable() {
        let mut source = Session::default();
        run(&mut source, "parameters.add", json!({"name":"height","expression":"3 mm"}));
        run(&mut source, "sketch.create", json!({"plane":"XY","name":"Curve"}));
        run(&mut source, "sketch.equation_curve", json!({"x":"t","y":"height + t * t","start":-2,"end":2}));
        run(&mut source, "sketch.finish", json!({}));
        run(&mut source, "timeline.copy", json!({"features":["Curve"]}));
        let mut target = Session::default();
        run(&mut target, "parameters.add", json!({"name":"height","expression":"99 mm"}));
        target.foreign_copy = source.foreign_copy.clone();
        let id = run(&mut target, "timeline.paste", json!({"from_design":true}))["pasted"][0]["id"].as_u64().unwrap();
        let state = target.model.state();
        let sketch = &state.sketch(id).unwrap().sketch;
        let equation = &sketch.equations[0];
        assert_eq!(equation.x.as_deref(), Some("t"));
        assert_eq!(equation.y, "height_2 + t * t");
        let curve = sketch.curves.iter().find(|c| c.id == equation.curve).unwrap();
        for point in curve.kind.point_ids() {
            let p = sketch.points[point].pos;
            assert!((p.y - 3. - p.x * p.x).abs() < 1e-8, "{p:?}");
        }
        assert!(target.doc.param("t").is_none());
    }

    /// Features pasted into a component placed elsewhere (offset and turned) are carried into its
    /// frame: the pasted body sits where the original is in the world, in that component, and the
    /// pasted sketch is named after the original.
    #[test]
    fn features_pasted_into_a_component_placed_elsewhere_stay_where_they_were() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"sketch": "S", "distance": 5, "body_name": "Block"}));
        let made = run(&mut s, "component.create", json!({"name": "A", "activate": false}));
        let (a, at) = (made["component"].as_u64().unwrap(), made["occurrence"].as_u64().unwrap());
        run(&mut s, "occurrence.ground", json!({"occurrence": at, "grounded": false}));
        run(&mut s, "occurrence.move", json!({"occurrence": at, "translate": [100, 0, 0], "axis": [0, 0, 1], "angle": "90 deg"}));
        run(&mut s, "timeline.copy", json!({"features": ["S", "Extrude1"]}));
        let r = run(&mut s, "timeline.paste", json!({"component": a, "component_path": [at]}));
        assert_eq!(r["errors"].as_array().map(Vec::len), Some(0), "{r}");
        assert_eq!(r["pasted"][0]["name"], "S (2)");
        let world = s.world_state();
        let (original, copy) = (world.body("Block").unwrap().mesh().bounds(), world.body("Block (2)").unwrap().mesh().bounds());
        assert!(copy.min.dist(original.min) < 1e-6 && copy.max.dist(original.max) < 1e-6, "{original:?} vs {copy:?}");
        let state = s.model.state();
        assert_eq!(s.doc.model_body_component(state.body("Block (2)").unwrap()), a);
    }

    /// A sketch attached to a face of a body that isn't copied with it can't be carried into a
    /// component placed elsewhere: the paste is refused, saying to make its plane fixed first.
    #[test]
    fn a_face_attached_sketch_is_not_moved_into_a_component_placed_elsewhere() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 20, "body_name": "Block"}));
        run(&mut s, "sketch.create", json!({"plane": {"face": [10, 10, 20]}, "name": "OnTop"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [2, 2], "p1": [6, 6]}));
        run(&mut s, "sketch.finish", json!({}));
        let made = run(&mut s, "component.create", json!({"name": "A", "activate": false}));
        let (a, at) = (made["component"].as_u64().unwrap(), made["occurrence"].as_u64().unwrap());
        run(&mut s, "occurrence.ground", json!({"occurrence": at, "grounded": false}));
        run(&mut s, "occurrence.move", json!({"occurrence": at, "translate": [100, 0, 0]}));
        run(&mut s, "timeline.copy", json!({"features": ["OnTop"]}));
        let err = s.execute("timeline.paste", &json!({"component": a, "component_path": [at]})).unwrap_err().to_string();
        assert!(err.contains("fixed plane"), "{err}");
        assert!(s.doc.find_feature("OnTop (2)").is_none(), "nothing was pasted");
    }

    #[test]
    fn copy_paste_sketch_and_extrude() {
        let mut s = Session::default();
        run(&mut s, "parameters.change", json!({"name": "w", "expression": "10 mm"}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
        run(&mut s, "sketch.constraint.coincident", json!({"a": "p1", "b": "origin"}));
        run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": "w"}));
        run(&mut s, "sketch.dimension", json!({"entities": ["l2"], "value": 10}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"sketch": "S", "distance": 5}));
        assert!((volume(&mut s) - 500.0).abs() < 1e-6);
        run(&mut s, "timeline.copy", json!({"features": ["S", "Extrude1"]}));
        let r = run(&mut s, "timeline.paste", json!({"translate": [30, 0, 0]}));
        assert_eq!(r["errors"].as_array().map(Vec::len), Some(0), "{r}");
        assert_eq!(run(&mut s, "inspect.measure", json!({}))["body_count"], 2);
        assert!((volume(&mut s) - 1000.0).abs() < 1e-6);
        // The copy's dimensions are its own, still driven by the user parameter.
        run(&mut s, "parameters.change", json!({"name": "w", "expression": "20 mm"}));
        assert!((volume(&mut s) - 2000.0).abs() < 1e-6);
        let pasted_extrude = r["pasted"][1]["name"].as_str().unwrap_or_default().to_string();
        let d = s.doc.find_feature(&pasted_extrude).and_then(|f| f.param_names.first().cloned()).unwrap_or_default();
        run(&mut s, "parameters.change", json!({"name": d, "expression": "10"}));
        assert!((volume(&mut s) - 3000.0).abs() < 1e-6, "only the copy got thicker");
        assert!(s.execute("timeline.paste", &json!({"translate": "x"})).is_err());
        let mut t = Session::default();
        assert!(t.execute("timeline.paste", &json!({})).is_err());
    }

    #[test]
    fn paste_at_a_marker_inside_a_timeline_folder_joins_the_folder() {
        let mut s = Session::default();
        for (name, body, x) in [("First", "FirstBody", 0), ("Second", "SecondBody", 20), ("Source", "SourceBody", 40)] {
            run(
                &mut s,
                "solid.box",
                json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0], "operation": "new_body", "name": name, "body_name": body}),
            );
        }
        let group = run(&mut s, "timeline.group", json!({"name": "Pair", "features": ["First", "Second"]}))["group"].as_u64().unwrap();
        run(&mut s, "timeline.copy", json!({"features": ["Source"]}));
        run(&mut s, "timeline.roll_to", json!({"position": 1}));

        let pasted = run(&mut s, "timeline.paste", json!({}))["pasted"][0]["id"].as_u64().unwrap();
        let folder = s.doc.timeline_groups.iter().find(|folder| folder.id == group).unwrap();
        let first = s.doc.find_feature("First").unwrap().id;
        let second = s.doc.find_feature("Second").unwrap().id;
        assert_eq!(folder.features, vec![first, pasted, second]);
        assert!(solidcraft_doc::Document::from_json(&s.doc.to_json()).is_ok(), "pasting inside a folder must leave a reopenable design");
    }

    /// A STEP file of three boxes at x = 0, 30 and 60. With `shared` the three solids carry one
    /// label (`X`), as the parts of many assembly files do; otherwise each has its own.
    fn three_box_step(tag: &str, shared: bool) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("solidcraft-paste-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = Session::default();
        for (i, label) in ["Aa", "Bb", "Cc"].iter().enumerate() {
            let corner = [30.0 * i as f64, 0.0, 0.0];
            run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": corner, "operation": "new_body", "body_name": label}));
        }
        let path = dir.join("Three.step");
        run(&mut s, "file.export", json!({"path": path.to_string_lossy()}));
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

    /// An OBJ file of three 10 mm cubes at x = 0, 30 and 60, every object called `X`.
    fn three_cube_obj(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("solidcraft-paste-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let corners = [[0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0], [0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]];
        let faces =
            [[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [3, 7, 6], [3, 6, 2], [0, 4, 7], [0, 7, 3], [1, 2, 6], [1, 6, 5]];
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

    fn body_names(s: &Session) -> Vec<String> {
        s.model.state().bodies.iter().map(|b| b.name.clone()).collect()
    }

    /// Where every body is: the lowest corner of its bounding box, rounded, in model order.
    fn corners(s: &mut Session) -> Vec<Vec<f64>> {
        let m = run(s, "inspect.measure", json!({}));
        let bodies = m["bodies"].as_array().cloned().unwrap_or_default();
        bodies
            .iter()
            .map(|b| b["bbox"]["min"].as_array().map(|a| a.iter().filter_map(|v| v.as_f64().map(f64::round)).collect()).unwrap_or_default())
            .collect()
    }

    /// Lifts the first of the three bodies of the import by 20 and the second by 50 (two features),
    /// copies the import with both and pastes them: the copies must be lifted, the originals once.
    fn paste_import_with_two_lifts(s: &mut Session, import: &Value, bodies: [&str; 3]) {
        run(s, "solid.move", json!({"bodies": [bodies[0]], "translate": [0, 0, 20], "name": "Lift first"}));
        run(s, "solid.move", json!({"bodies": [bodies[1]], "translate": [0, 0, 50], "name": "Lift second"}));
        run(s, "timeline.copy", json!({"features": [import, "Lift first", "Lift second"]}));
        let r = run(s, "timeline.paste", json!({}));
        assert_eq!(r["errors"].as_array().map(Vec::len), Some(0), "{r}");
    }

    const LIFTED: [[f64; 3]; 3] = [[0.0, 0.0, 20.0], [30.0, 0.0, 50.0], [60.0, 0.0, 0.0]];

    /// A control: three solids with labels of their own. The pasted features act on the pasted bodies.
    #[test]
    fn pasting_an_import_moves_the_copies_of_the_bodies_its_features_named() {
        let path = three_box_step("distinct", false);
        let mut s = Session::default();
        let import = run(&mut s, "file.insert_step", json!({"path": path.to_string_lossy()}))["feature"].clone();
        assert_eq!(body_names(&s), ["Aa", "Bb", "Cc"]);
        paste_import_with_two_lifts(&mut s, &import, ["Aa", "Bb", "Cc"]);
        assert_eq!(body_names(&s), ["Aa", "Bb", "Cc", "Aa (2)", "Bb (2)", "Cc (2)"]);
        let at: Vec<Vec<f64>> = LIFTED.iter().chain(&LIFTED).map(|c| c.to_vec()).collect();
        assert_eq!(corners(&mut s), at);
    }

    /// Solids of a STEP file that share one label are built as `X`, `X (2)`, `X (3)`, and the features
    /// made on them name them so. The pasted features act on the copies of those bodies, each on its
    /// own: `X (2)` is not left on the original, and `X` does not go to the last copy.
    #[test]
    fn pasting_an_import_with_one_label_on_all_solids_moves_the_copy_of_each_body() {
        let path = three_box_step("shared", true);
        let mut s = Session::default();
        let import = run(&mut s, "file.insert_step", json!({"path": path.to_string_lossy()}))["feature"].clone();
        assert_eq!(body_names(&s), ["X", "X (2)", "X (3)"]);
        paste_import_with_two_lifts(&mut s, &import, ["X", "X (2)", "X (3)"]);
        let names = body_names(&s);
        assert_eq!(names.len(), 6, "{names:?}");
        let at: Vec<Vec<f64>> = LIFTED.iter().chain(&LIFTED).map(|c| c.to_vec()).collect();
        assert_eq!(corners(&mut s), at, "{names:?}");
        // The copies keep their names and their places when the design is saved and opened again.
        let design = path.with_file_name("Three.solidcraft");
        run(&mut s, "file.save_as", json!({"path": design.to_string_lossy()}));
        let mut r = Session::default();
        run(&mut r, "doc.open", json!({"path": design.to_string_lossy()}));
        assert_eq!(body_names(&r), names);
        assert_eq!(corners(&mut r), at);
    }

    /// The same for the objects of an OBJ file (3MF and STL files give the same kind of import).
    #[test]
    fn pasting_a_mesh_import_with_one_label_on_all_objects_moves_the_copy_of_each_body() {
        let path = three_cube_obj("shared-obj");
        let mut s = Session::default();
        let import = run(&mut s, "file.insert_mesh", json!({"path": path.to_string_lossy()}))["feature"].clone();
        assert_eq!(body_names(&s), ["X", "X (2)", "X (3)"]);
        paste_import_with_two_lifts(&mut s, &import, ["X", "X (2)", "X (3)"]);
        let at: Vec<Vec<f64>> = LIFTED.iter().chain(&LIFTED).map(|c| c.to_vec()).collect();
        assert_eq!(corners(&mut s), at, "{:?}", body_names(&s));
    }

    /// No shared label is needed: a second import of the same file builds `Aa (2)`, `Bb (2)`, `Cc (2)`
    /// from the labels `Aa`, `Bb`, `Cc`, and the features made on those bodies name them so.
    #[test]
    fn pasting_a_second_import_of_a_file_moves_the_copies_of_its_bodies() {
        let path = three_box_step("twice", false);
        let mut s = Session::default();
        run(&mut s, "file.insert_step", json!({"path": path.to_string_lossy()}));
        let second = run(&mut s, "file.insert_step", json!({"path": path.to_string_lossy()}))["feature"].clone();
        assert_eq!(body_names(&s), ["Aa", "Bb", "Cc", "Aa (2)", "Bb (2)", "Cc (2)"]);
        paste_import_with_two_lifts(&mut s, &second, ["Aa (2)", "Bb (2)", "Cc (2)"]);
        // Each import is a component of its own (#1386), shown after the pasted copies: the first
        // import's bodies stay where they were, the second's and their copies are lifted.
        let plain = [[0.0, 0.0, 0.0], [30.0, 0.0, 0.0], [60.0, 0.0, 0.0]];
        let at: Vec<Vec<f64>> = LIFTED.iter().chain(&plain).chain(&LIFTED).map(|c| c.to_vec()).collect();
        assert_eq!(corners(&mut s), at, "{:?}", body_names(&s));
        let m = run(&mut s, "inspect.measure", json!({}));
        let names: Vec<&str> = m["bodies"].as_array().unwrap().iter().filter_map(|b| b["name"].as_str()).collect();
        assert_eq!(names, ["Aa (2)(2)", "Bb (2)(2)", "Cc (2)(2)", "Aa", "Bb", "Cc", "Aa (2)", "Bb (2)", "Cc (2)"]);
    }

    /// A cylinder Emboss finds its face by the persistent name it recorded when it was made. The
    /// pasted Emboss keeps that name, moved onto the pasted cylinder, so the copy gets its own
    /// badge, with or without a translate (#1035).
    #[test]
    fn pasted_cylinder_emboss_badges_the_pasted_cylinder() {
        for translate in [Some(json!([50, 0, 0])), None] {
            let mut s = Session::default();
            run(&mut s, "solid.cylinder", json!({"radius": "10 mm", "height": "20 mm", "body_name": "Sleeve", "name": "Tube"}));
            run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 10, 0], "x_dir": [-1, 0, 0], "y_dir": [0, 0, 1]}, "name": "Badge"}));
            run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-2, 5], "p1": [2, 10]}));
            run(&mut s, "sketch.finish", json!({}));
            run(&mut s, "solid.emboss", json!({"sketch": "Badge", "face": [0, 10, 7.5], "depth": 1, "targets": ["Sleeve"], "name": "Mark"}));
            let one = volume(&mut s);
            let plain = std::f64::consts::PI * 100.0 * 20.0;
            assert!(one > plain + 1.0, "the badge adds material: {one}");
            run(&mut s, "timeline.copy", json!({"features": ["Tube", "Badge", "Mark"]}));
            let p = translate.clone().map_or(json!({}), |t| json!({"translate": t}));
            let r = run(&mut s, "timeline.paste", p);
            assert_eq!(r["errors"].as_array().map(Vec::len), Some(0), "{translate:?}: {r}");
            let mark = s.doc.find_feature(r["pasted"][2]["name"].as_str().unwrap_or_default()).cloned().unwrap();
            let tube = r["pasted"][0]["id"].as_u64().unwrap();
            assert!(mark.face_names.first().is_some_and(|n| n.starts_with(&format!("F{tube}:"))), "{:?}", mark.face_names);
            assert!((volume(&mut s) - 2.0 * one).abs() < 1e-6, "{translate:?}: both cylinders carry a badge");
            if translate.is_some() {
                let m = run(&mut s, "inspect.measure", json!({"bodies": ["Sleeve (2)"]}));
                let body = &m["bodies"][0];
                let x = body["center_of_mass"][0].as_f64().unwrap_or(f64::NAN);
                let y = body["bbox"]["max"][1].as_f64().unwrap_or(f64::NAN);
                assert!((x - 50.0).abs() < 1e-3 && (y - 11.0).abs() < 1e-6, "the copy's badge sits on the moved cylinder: {m}");
            }
        }
    }

    #[test]
    fn pasted_fillet_rounds_the_copied_body_it_names() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 20]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"sketch": "S", "distance": 10, "operation": "new", "body_name": "Block", "name": "Block"}));
        run(&mut s, "solid.fillet", json!({"body": "Block", "radius": 2, "edges": [[20, 0, 10], [20, 20, 10]], "name": "Round"}));
        let one = volume(&mut s);
        assert!(one < 8000.0 - 1.0, "the fillet removes material: {one}");
        run(&mut s, "timeline.copy", json!({"features": ["S", "Block", "Round"]}));
        let r = run(&mut s, "timeline.paste", json!({"translate": [0, 40, 0]}));
        assert_eq!(r["errors"].as_array().map(Vec::len), Some(0), "{r}");
        let round = r["pasted"][2]["name"].as_str().unwrap_or_default().to_string();
        let def = run(&mut s, "timeline.inspect", json!({"feature": round}));
        assert_eq!(def["definition"]["body"], "Block (2)", "{def}");
        let m = run(&mut s, "inspect.measure", json!({}));
        assert_eq!(m["body_count"], 2, "{m}");
        assert!((volume(&mut s) - 2.0 * one).abs() < 1e-6, "both blocks are rounded: {m}");
    }

    /// One body's measures (bounding box, volume).
    fn measures(s: &mut Session, body: &str) -> serde_json::Value {
        run(s, "inspect.measure", json!({"bodies": [body]}))["bodies"][0].clone()
    }

    /// Pasting with a translate moves an extrude's `to` and `from` picks with it: the pasted posts
    /// run to the pasted roof and from the pasted shelf, not to and from the originals.
    #[test]
    fn a_pasted_extrude_runs_to_and_from_the_pasted_faces() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 2, "corner": [0, 0, 20], "name": "Roof", "body_name": "Roof"}));
        run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 2, "corner": [0, 0, 10], "name": "Shelf", "body_name": "Shelf"}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [5, 5], "p1": [10, 10]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"sketch": "S", "distance": 5, "to": [7, 7, 20], "operation": "new", "name": "Up", "body_name": "Up"}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "T"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [25, 25], "p1": [30, 30]}));
        run(&mut s, "sketch.finish", json!({}));
        run(
            &mut s,
            "solid.extrude",
            json!({"sketch": "T", "distance": 5, "from": [27, 27, 12], "operation": "new", "name": "On", "body_name": "On"}),
        );
        run(&mut s, "timeline.copy", json!({"features": ["Roof", "Shelf", "S", "Up", "T", "On"]}));
        let r = run(&mut s, "timeline.paste", json!({"translate": [100, 0, 10]}));
        assert_eq!(r["errors"].as_array().map(Vec::len), Some(0), "{r}");
        let up = measures(&mut s, "Up (2)");
        assert!((up["bbox"]["min"][2].as_f64().unwrap_or(0.0) - 10.0).abs() < 1e-6, "{up}");
        assert!((up["bbox"]["max"][2].as_f64().unwrap_or(0.0) - 30.0).abs() < 1e-6, "it ends on the pasted roof: {up}");
        let on = measures(&mut s, "On (2)");
        assert!((on["bbox"]["min"][2].as_f64().unwrap_or(0.0) - 22.0).abs() < 1e-6, "it starts on the pasted shelf: {on}");
        assert!((on["bbox"]["max"][2].as_f64().unwrap_or(0.0) - 27.0).abs() < 1e-6, "{on}");
    }

    /// Pasting with a translate moves a hole's `to` pick with it: the pasted through hole still goes
    /// through, rather than stopping at the original's depth.
    #[test]
    fn a_pasted_hole_drills_to_the_moved_point() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 10, "name": "Block", "body_name": "Block"}));
        run(&mut s, "solid.hole", json!({"position": [20, 20, 10], "direction": [0, 0, -1], "diameter": 10, "to": [30, 30, -2], "name": "Drill"}));
        run(&mut s, "timeline.copy", json!({"features": ["Block", "Drill"]}));
        let r = run(&mut s, "timeline.paste", json!({"translate": [60, 0, -6]}));
        assert_eq!(r["errors"].as_array().map(Vec::len), Some(0), "{r}");
        let (block, copy) = (measures(&mut s, "Block"), measures(&mut s, "Block (2)"));
        let v = |m: &serde_json::Value| m["volume_mm3"].as_f64().unwrap_or(f64::NAN);
        assert!((v(&block) - (16000.0 - std::f64::consts::PI * 25.0 * 10.0)).abs() < 1.0, "a through hole: {block}");
        assert!((v(&copy) - v(&block)).abs() < 1e-6, "the pasted hole goes through too: {copy}");
    }

    /// Pasting with a translate moves a revolve's `to` pick with it: the pasted turn ends at the same
    /// angle about its moved axis.
    #[test]
    fn a_pasted_revolve_turns_to_the_moved_point() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "S"}));
        run(&mut s, "sketch.line", json!({"points": [[10, 0], [15, 0], [15, 5], [10, 5]], "closed": true}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.revolve", json!({"sketch": "S", "axis": "y", "to": [0, 12, 2], "name": "Turn", "body_name": "Arc"}));
        run(&mut s, "timeline.copy", json!({"features": ["S", "Turn"]}));
        let r = run(&mut s, "timeline.paste", json!({"translate": [50, 0, 0]}));
        assert_eq!(r["errors"].as_array().map(Vec::len), Some(0), "{r}");
        let (arc, copy) = (measures(&mut s, "Arc"), measures(&mut s, "Arc (2)"));
        let v = |m: &serde_json::Value| m["volume_mm3"].as_f64().unwrap_or(f64::NAN);
        // A quarter turn of a 5 × 5 section whose centre is 12.5 from the axis.
        let quarter = 25.0 * std::f64::consts::FRAC_PI_2 * 12.5;
        assert!((v(&arc) - quarter).abs() < quarter * 1e-3, "{arc}");
        assert!((v(&copy) - v(&arc)).abs() < 1e-6, "the pasted arc turns a quarter too: {copy}");
    }

    /// A pasted coil that joins a body made by an earlier copy joins that copy, not the original.
    #[test]
    fn a_pasted_coil_joins_the_pasted_body() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 30, "width": 30, "height": 5, "name": "Base", "body_name": "Plate"}));
        run(
            &mut s,
            "solid.coil",
            json!({"diameter": 16, "pitch": 6, "revolutions": 3, "section_size": 2, "base": [15, 15, 5], "operation": "join", "targets": ["Plate"], "name": "Spring"}),
        );
        run(&mut s, "timeline.copy", json!({"features": ["Base", "Spring"]}));
        let r = run(&mut s, "timeline.paste", json!({"translate": [50, 0, 0]}));
        assert_eq!(r["errors"].as_array().map(Vec::len), Some(0), "{r}");
        let spring = r["pasted"][1]["name"].as_str().unwrap_or_default().to_string();
        let def = run(&mut s, "timeline.inspect", json!({"feature": spring}));
        assert_eq!(def["definition"]["targets"], json!(["Plate (2)"]), "{def}");
        let st = s.world_state();
        let v = |name: &str| solidcraft_kernel::measure(&st.body(name).unwrap().body).unwrap().volume;
        // A spring on the wrong plate is hundreds of mm³ off. The two plates are measured apart
        // (one moved 50 mm), which the boolean and the measure each round in their own way:
        // a few 1e-6 mm³ on 5000.
        assert!((v("Plate") - v("Plate (2)")).abs() < 1e-8 * v("Plate"), "each plate carries one spring: {} vs {}", v("Plate"), v("Plate (2)"));
    }

    /// A copied body transfer is left out of a paste: the copies go into the component they are
    /// pasted into. A pasted transfer belonged to that component and not to its destination, and
    /// the saved design no longer loaded.
    #[test]
    fn a_copied_body_transfer_is_left_out_of_the_paste() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "name": "Base", "body_name": "Block"}));
        let a = run(&mut s, "component.create", json!({"name": "A", "activate": false}))["component"].as_u64().unwrap_or_default();
        run(&mut s, "component.move_bodies", json!({"bodies": ["Block"], "component": "A"}));
        let transfer = s.doc.features.iter().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::BodyTransfer { .. })).map(|f| f.name.clone());
        let transfer = transfer.unwrap_or_default();
        run(&mut s, "timeline.copy", json!({"features": ["Base", transfer]}));
        let r = run(&mut s, "timeline.paste", json!({"translate": [20, 0, 0]}));
        assert_eq!(r["pasted"].as_array().map(Vec::len), Some(1), "{r}");
        assert_eq!(r["notices"], json!([format!("{transfer} is a body transfer and was not pasted")]), "{r}");
        let state = s.model.state();
        assert_eq!(state.body("Block").map(|b| s.doc.model_body_component(b)), Some(a));
        assert_eq!(state.body("Block (2)").map(|b| s.doc.model_body_component(b)), Some(0), "the copy is in the root, where it was pasted");
        if let Err(e) = solidcraft_doc::Document::from_json(&s.doc.to_json()) {
            panic!("the design loads back: {e}");
        }
        run(&mut s, "timeline.copy", json!({"features": [transfer]}));
        let err = s.execute("timeline.paste", &json!({})).map(|v| v.to_string()).unwrap_or_else(|e| e.to_string());
        assert!(err.contains("body transfers are not pasted"), "{err}");
    }
}
