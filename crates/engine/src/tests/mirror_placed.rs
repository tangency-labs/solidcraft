//! `solid.mirror` of bodies in a placed component (a moved occurrence, a further instance, an
//! inserted design) mirrors each body where it is shown. Before, it mirrored the component's own
//! geometry and put the copy in the active component, so the copy lost the placement.

use serde_json::{Value, json};
use solidcraft_geom::Vec3;

use super::run;
use crate::*;

fn world_box(s: &Session, body: &str) -> (Vec3, Vec3) {
    let b = s.world_state().body(body).unwrap_or_else(|| panic!("no body `{body}`")).mesh().bounds();
    (b.min, b.max)
}

/// The box of `body` mirrored in the world YZ plane.
fn mirrored_yz(s: &Session, body: &str) -> (Vec3, Vec3) {
    let (lo, hi) = world_box(s, body);
    (Vec3::new(-hi.x, lo.y, lo.z), Vec3::new(-lo.x, hi.y, hi.z))
}

fn assert_box(got: (Vec3, Vec3), want: (Vec3, Vec3)) {
    assert!(got.0.dist(want.0) < 1e-6 && got.1.dist(want.1) < 1e-6, "got {got:?}, want {want:?}");
}

/// A component "Arm" holding "ArmBody", its occurrence turned and moved; the root is active.
fn moved_arm(s: &mut Session) -> (u64, u64) {
    let c = run(s, "component.create", json!({"name": "Arm", "grounded": false}));
    let (component, occurrence) = (c["component"].as_u64().unwrap(), c["occurrence"].as_u64().unwrap());
    run(s, "solid.box", json!({"corner": [10, 0, 0], "length": 20, "width": 4, "height": 6, "body_name": "ArmBody"}));
    run(s, "component.activate", json!({"component": 0}));
    run(s, "occurrence.move", json!({"occurrence": occurrence, "axis": [1, 0, 0], "angle": "90 deg", "translate": [30, 50, 0]}));
    (component, occurrence)
}

fn created(r: &Value) -> String {
    r["created_bodies"][0].as_str().unwrap().to_string()
}

#[test]
fn a_moved_components_body_is_mirrored_where_it_is_shown() {
    let mut s = Session::default();
    let (_, occurrence) = moved_arm(&mut s);
    let copy = created(&run(&mut s, "solid.mirror", json!({"bodies": ["ArmBody"], "plane": "YZ"})));
    assert_box(world_box(&s, &copy), mirrored_yz(&s, "ArmBody"));
    // The copy follows its source when the occurrence moves again.
    run(&mut s, "occurrence.move", json!({"occurrence": occurrence, "space": "world", "translate": [0, 200, 0]}));
    assert_box(world_box(&s, &copy), mirrored_yz(&s, "ArmBody"));
    // It survives a save and reopen.
    let t = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert_box(world_box(&t, &copy), mirrored_yz(&t, "ArmBody"));
}

#[test]
fn a_further_instance_is_mirrored_at_its_own_placement() {
    let mut s = Session::default();
    let (component, _) = moved_arm(&mut s);
    run(&mut s, "occurrence.copy", json!({"component": component, "translate": [0, 0, 40]}));
    let names: Vec<String> = s.world_state().bodies.iter().map(|b| b.name.clone()).collect();
    let instance = names.iter().find(|n| n.starts_with("ArmBody (")).cloned().unwrap();
    let r = run(&mut s, "solid.mirror", json!({"bodies": ["ArmBody", instance], "plane": "YZ"}));
    let copies: Vec<String> = r["created_bodies"].as_array().unwrap().iter().map(|n| n.as_str().unwrap().to_string()).collect();
    assert_eq!(copies.len(), 2, "{r}");
    assert_box(world_box(&s, &copies[0]), mirrored_yz(&s, "ArmBody"));
    assert_box(world_box(&s, &copies[1]), mirrored_yz(&s, &instance));
    // A component placed twice would show a copy at both placements: the copies join the
    // innermost component around it that is placed once, here the design itself.
    let st = s.model.state();
    for copy in &copies {
        assert_eq!(s.doc.model_body_component(st.body(copy).unwrap()), 0, "{copy}");
    }
}

#[test]
fn a_plane_picked_on_a_moved_body_is_a_plane_of_the_active_component() {
    let mut s = Session::default();
    moved_arm(&mut s);
    // A world plane x = 30 through the moved body: the copy is mirrored in it, as shown.
    let copy = created(&run(&mut s, "solid.mirror", json!({"bodies": ["ArmBody"], "plane": {"origin": [30, 50, 0], "normal": [1, 0, 0]}})));
    let (lo, hi) = world_box(&s, "ArmBody");
    assert_box(world_box(&s, &copy), (Vec3::new(60.0 - hi.x, lo.y, lo.z), Vec3::new(60.0 - lo.x, hi.y, hi.z)));
}

#[test]
fn a_joined_mirror_of_a_moved_body_stays_in_its_component() {
    let mut s = Session::default();
    moved_arm(&mut s);
    // A world plane through the body's far end: the joined body doubles along x where it is.
    let (lo, hi) = world_box(&s, "ArmBody");
    run(&mut s, "solid.mirror", json!({"bodies": ["ArmBody"], "plane": {"origin": [hi.x, 0, 0], "normal": [1, 0, 0]}, "combine": true}));
    let len = hi.x - lo.x;
    assert_box(world_box(&s, "ArmBody"), (lo, Vec3::new(hi.x + len, hi.y, hi.z)));
}

#[test]
fn bodies_in_place_keep_no_placement() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Base"}));
    run(&mut s, "solid.mirror", json!({"bodies": ["Base"], "plane": "YZ"}));
    let f = s.doc.features.last().unwrap();
    assert!(matches!(&f.kind, solidcraft_doc::FeatureKind::Mirror { placements, .. } if placements.is_empty()), "{:?}", f.kind);
}

#[test]
fn a_position_not_captured_yet_is_refused() {
    let mut s = Session::default();
    let (_, occurrence) = moved_arm(&mut s);
    run(&mut s, "occurrence.move", json!({"occurrence": occurrence, "translate": [5, 0, 0], "capture": false}));
    let e = s.execute("solid.mirror", &json!({"bodies": ["ArmBody"], "plane": "YZ"})).unwrap_err().to_string();
    assert!(e.contains("Capture Position"), "{e}");
}

#[test]
fn a_copy_goes_beside_its_source_in_that_component() {
    // Moved or in place, a component's mirrored body is a body of that component.
    for moved in [true, false] {
        let mut s = Session::default();
        let (component, occurrence) = moved_arm(&mut s);
        if !moved {
            run(&mut s, "occurrence.move", json!({"occurrence": occurrence, "base_transform": solidcraft_doc::IDENTITY}));
        }
        let copy = created(&run(&mut s, "solid.mirror", json!({"bodies": ["ArmBody"], "plane": "YZ"})));
        let st = s.model.state();
        assert_eq!(s.doc.model_body_component(st.body(&copy).unwrap()), component, "moved: {moved}");
        assert_box(world_box(&s, &copy), mirrored_yz(&s, "ArmBody"));
        // It stays there through a later feature on it.
        run(&mut s, "solid.move", json!({"bodies": [copy.clone()], "translate": [0, 0, 5]}));
        assert_eq!(s.doc.model_body_component(s.model.state().body(&copy).unwrap()), component, "moved: {moved}");
    }
}

/// Inserting the same file again reuses its body names; a mirror of the first insert's bodies
/// keeps mirroring them (their copies vanished: the names led to the second insert's components).
#[test]
fn inserting_the_same_file_again_keeps_the_mirrored_copies() {
    let dir = std::env::temp_dir().join(format!("solidcraft-mirror-placed-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("nested.step").to_string_lossy().replace('\\', "/");
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Outer"}));
    run(&mut s, "solid.box", json!({"length": 30, "width": 30, "height": 5, "body_name": "Plate"}));
    let inner = run(&mut s, "component.create", json!({"name": "Inner"}))["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 10, "body_name": "Block"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "occurrence.move", json!({"occurrence": inner, "axis": [0, 0, 1], "angle": "90 deg", "translate": [100, 0, 0]}));
    run(&mut s, "file.export", json!({"path": file}));

    let mut s = Session::default();
    run(&mut s, "file.insert_step", json!({"path": file, "name": "A"}));
    let wanted: Vec<(Vec3, Vec3)> = ["Plate", "Block"].iter().map(|b| mirrored_yz(&s, b)).collect();
    let r = run(&mut s, "solid.mirror", json!({"bodies": ["Plate", "Block"], "plane": "YZ"}));
    let copies: Vec<String> = r["created_bodies"].as_array().unwrap().iter().map(|n| n.as_str().unwrap().to_string()).collect();
    run(&mut s, "solid.remove", json!({"bodies": ["Plate", "Block"]}));
    run(&mut s, "component.from_bodies", json!({"bodies": copies}));
    run(&mut s, "file.insert_step", json!({"path": file, "name": "B"}));
    let _ = std::fs::remove_dir_all(&dir);
    for r in &s.model.results {
        assert!(r.error.is_none(), "{}: {:?}", r.name, r.error);
    }
    for (copy, want) in copies.iter().zip(wanted) {
        assert_box(world_box(&s, copy), want);
    }
}

/// Moving an occurrence the mirror does not depend on leaves the mirror alone: before, any
/// occurrence move evaluated it again, and every later command paid for one more mirror.
#[test]
fn moving_an_unrelated_occurrence_does_not_evaluate_the_mirror_again() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 20, "width": 10, "height": 5, "corner": [10, 0, 0], "body_name": "A"}));
    run(&mut s, "solid.box", json!({"length": 8, "width": 8, "height": 8, "corner": [30, 0, 0], "body_name": "B"}));
    run(&mut s, "component.from_bodies", json!({"bodies": ["A", "B"]}));
    let occurrences: Vec<u64> = s.doc.occurrences.iter().map(|o| o.id).collect();
    for occurrence in &occurrences {
        run(&mut s, "occurrence.ground", json!({"occurrence": occurrence, "grounded": false}));
    }
    assert_eq!(occurrences.len(), 2, "{:?}", s.doc.occurrences);
    let a = s.doc.body_frames(&s.model.state()).into_iter().find(|(name, ..)| name == "A").map(|(_, _, path, _)| path).unwrap();
    let (mine, other) = if a.contains(&occurrences[0]) { (occurrences[0], occurrences[1]) } else { (occurrences[1], occurrences[0]) };
    run(&mut s, "occurrence.move", json!({"occurrence": mine, "translate": [0, 5, 0]}));
    let copy = created(&run(&mut s, "solid.mirror", json!({"bodies": ["A"], "plane": "YZ"})));
    for _ in 0..3 {
        run(&mut s, "occurrence.move", json!({"occurrence": other, "translate": [0, 0.1, 0]}));
        assert_eq!(s.model.last_recomputed, 0, "an unrelated move evaluated features again");
    }
    // Moving the mirrored body's own occurrence does evaluate it, and the copy follows.
    run(&mut s, "occurrence.move", json!({"occurrence": mine, "translate": [0, 1, 0]}));
    assert!(s.model.last_recomputed > 0);
    assert_box(world_box(&s, &copy), mirrored_yz(&s, "A"));
}

/// Two mirrored copies of overlapping bodies of a placed component intersect as their sources do.
#[test]
fn mirrored_copies_of_a_placed_component_intersect() {
    let mut s = Session::default();
    let c = run(&mut s, "component.create", json!({"name": "Pair", "grounded": false}));
    let occurrence = c["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [10, 0, 0], "body_name": "P"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [15, 5, 5], "body_name": "Q"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "occurrence.move", json!({"occurrence": occurrence, "axis": [0, 0, 1], "angle": "30 deg", "translate": [5, 20, 0]}));
    let r = run(&mut s, "solid.mirror", json!({"bodies": ["P", "Q"], "plane": "YZ"}));
    let copies: Vec<String> = r["created_bodies"].as_array().unwrap().iter().map(|n| n.as_str().unwrap().to_string()).collect();
    let i = run(&mut s, "inspect.interference", json!({}));
    let pair = i["interferences"].as_array().unwrap().iter().find(|x| x["a"] == copies[0].as_str() || x["b"] == copies[0].as_str()).cloned();
    let pair = pair.unwrap_or_else(|| panic!("{i}"));
    assert!((pair["volume_mm3"].as_f64().unwrap() - 125.0).abs() < 1e-6, "{i}");
    run(&mut s, "solid.combine", json!({"target": copies[0], "tools": [copies[1]], "operation": "intersect", "keep_tools": true}));
    let v = run(&mut s, "inspect.measure", json!({"bodies": [copies[0]]}))["total"]["volume_mm3"].as_f64().unwrap();
    assert!((v - 125.0).abs() < 1e-6, "{v}");
}

/// A saved placement that leads to another component (a stale or edited path) no longer shows the
/// mirrored body: the mirror reports that, instead of mirroring it where the other one is placed.
#[test]
fn a_placement_leading_to_another_component_is_reported_lost() {
    let mut s = Session::default();
    moved_arm(&mut s);
    let other = run(&mut s, "component.create", json!({"name": "Other"}))["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "body_name": "OtherBody"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "occurrence.move", json!({"occurrence": other, "translate": [-500, 0, 0]}));
    let copy = created(&run(&mut s, "solid.mirror", json!({"bodies": ["ArmBody"], "plane": "YZ"})));
    let mirror = s.doc.features.last().unwrap().id;
    if let Some(f) = std::sync::Arc::make_mut(&mut s.doc).feature_mut(mirror)
        && let solidcraft_doc::FeatureKind::Mirror { placements, .. } = &mut f.kind
    {
        *placements = vec![vec![other]];
    }
    s.refresh();
    let e = s.model.result(mirror).and_then(|r| r.error.clone()).unwrap_or_else(|| panic!("mirrored at the wrong placement"));
    assert!(e.contains("no longer exists"), "{e}");
    assert!(s.world_state().body(&copy).is_none());
}

/// A carriage-like part (a bored block and a ring) placed once, free to move: its occurrence and
/// body names.
fn curved_part(s: &mut Session) -> (u64, Vec<String>) {
    let c = run(s, "component.create", json!({"name": "Carriage", "grounded": false}));
    run(s, "solid.box", json!({"length": 20, "width": 10, "height": 10, "corner": [10, 5, 0], "body_name": "Block"}));
    run(s, "solid.cylinder", json!({"base": [20, 10, -1], "radius": 3, "height": 12, "operation": "cut", "targets": ["Block"]}));
    run(s, "solid.torus", json!({"center": [20, 10, 20], "major": 6, "minor": 2, "operation": "new", "body_name": "Ring"}));
    run(s, "component.activate", json!({"component": 0}));
    run(s, "occurrence.move", json!({"occurrence": c["occurrence"], "translate": [0, 7, 3]}));
    (c["occurrence"].as_u64().unwrap(), vec!["Block".into(), "Ring".into()])
}

/// The shown bodies drawn, as a viewport does, and the tessellation that took.
fn drawn(s: &Session) -> f64 {
    let world = s.world_state();
    let (_, work) = solidcraft_kernel::profile::capture(|| {
        for b in &world.bodies {
            b.mesh_in_place();
            b.display_plan();
            b.mesh();
        }
    });
    work.tessellate_ms
}

/// What the viewport shows is what a fresh evaluation of the design shows: where the triangles
/// are, which way they face and which faces and edges they belong to.
fn assert_as_evaluated_afresh(s: &Session) {
    let mut fresh = solidcraft_doc::Model::new();
    fresh.no_cache = true;
    fresh.evaluate(&s.doc);
    assert!(fresh.results.iter().all(|r| r.error.is_none()));
    let (shown, want) = (s.world_state(), solidcraft_doc::world_state(&s.doc, &fresh.state()));
    assert_eq!(shown.bodies.len(), want.bodies.len());
    for (a, b) in shown.bodies.iter().zip(&want.bodies) {
        assert_eq!(a.name, b.name);
        let (x, y) = (a.mesh(), b.mesh());
        assert_eq!(x.triangles, y.triangles, "{}", a.name);
        assert_eq!(x.tri_face, y.tri_face, "{}", a.name);
        assert_eq!(x.edge_faces, y.edge_faces, "{}", a.name);
        assert_eq!(x.seams, y.seams, "{}", a.name);
        assert_eq!(x.positions.len(), y.positions.len(), "{}", a.name);
        assert!(x.edges.iter().flatten().zip(y.edges.iter().flatten()).all(|(p, q)| p.dist(*q) < 1e-7), "{}: edges", a.name);
        assert_same_surface(&x, &y, &a.name);
        // A plan's ties fall by the pose it was made at.
        let (x, y) = (a.display_plan().unwrap().evaluate(), b.display_plan().unwrap().evaluate());
        assert_eq!(x.triangles.len(), y.triangles.len(), "{}: plan", a.name);
        assert_same_surface(&x, &y, &a.name);
    }
}

/// Two meshes of one surface, facing the same way. Their points can differ: a revolved face is
/// turned by where it stands when it is made, which only moves where the surface is sampled.
fn assert_same_surface(x: &solidcraft_geom::Mesh, y: &solidcraft_geom::Mesh, name: &str) {
    let enclosed = |m: &solidcraft_geom::Mesh| {
        m.triangles.iter().fold((0.0, 0.0), |(v, s), t| {
            let [a, b, c] = t.map(|i| m.positions[i as usize]);
            (v + a.dot(b.cross(c)) / 6.0, s + (b - a).cross(c - a).len() / 2.0)
        })
    };
    let ((v, s), (w, t)) = (enclosed(x), enclosed(y));
    assert!(v > 0.0 && (v - w).abs() < 1e-4 * w && (s - t).abs() < 1e-4 * t, "{name}: encloses {v}, {s} against {w}, {t}");
    let (a, b) = (x.bounds(), y.bounds());
    assert!(a.min.dist(b.min) < 0.05 && a.max.dist(b.max) < 0.05, "{name}: {a:?} against {b:?}");
}

/// Dragging a part with a mirrored copy moved the copy too, and every frame meshed the copy from
/// nothing: a design with a few dozen curved mirrored bodies dragged at a frame or two a second.
#[test]
fn dragging_a_mirrored_part_meshes_its_copies_once() {
    let mut s = Session::default();
    let (occurrence, bodies) = curved_part(&mut s);
    let first = run(&mut s, "solid.mirror", json!({"bodies": bodies, "plane": "XZ"}));
    // Mirrored again, as four carriages from one are: the source of these copies moves as well.
    run(&mut s, "solid.mirror", json!({"bodies": first["created_bodies"], "plane": "YZ"}));
    drawn(&s);
    for k in 1..=4 {
        let at = f64::from(k) * 3.5;
        run(&mut s, "occurrence.drag", json!({"occurrence": occurrence, "at": [0, 0, 0], "point": [1, at, 2], "continuous": true}));
        assert!(s.model.last_recomputed > 0, "the copies follow the part");
        assert_eq!(drawn(&s), 0.0, "frame {k}: the copies are drawn from triangles already made");
    }
    assert_as_evaluated_afresh(&s);
    run(&mut s, "edit.undo", json!({}));
    assert_as_evaluated_afresh(&s);
}

/// What the viewport draws of a copy doesn't reach what the model reads: a later feature finds
/// the copy's own triangles, as it does in a design opened afresh.
#[test]
fn a_mirrored_copys_shared_triangles_are_display_only() {
    let mut s = Session::default();
    let (occurrence, bodies) = curved_part(&mut s);
    let r = run(&mut s, "solid.mirror", json!({"bodies": bodies, "plane": "XZ"}));
    run(&mut s, "occurrence.drag", json!({"occurrence": occurrence, "at": [0, 0, 0], "point": [0, 3, 0], "continuous": true}));
    drawn(&s);
    let (state, world) = (s.model.state(), s.world_state());
    for copy in r["created_bodies"].as_array().unwrap().iter().map(|n| n.as_str().unwrap()) {
        assert!(world.body(copy).is_some_and(|b| b.cached_mesh().is_some()), "{copy} was drawn");
        assert!(state.body(copy).unwrap().cached_mesh().is_none(), "{copy}: the model's own mesh is its own");
    }
}

/// A source changed is mirrored from what it is now, not from the triangles of what it was.
#[test]
fn a_changed_source_gives_its_copy_new_triangles() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "side", "expression": "10 mm"}));
    let c = run(&mut s, "component.create", json!({"name": "Part", "grounded": false}));
    run(&mut s, "solid.box", json!({"length": "side", "width": 10, "height": 10, "corner": [10, 0, 0], "body_name": "Block"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "occurrence.move", json!({"occurrence": c["occurrence"], "translate": [0, 20, 0]}));
    let copy = created(&run(&mut s, "solid.mirror", json!({"bodies": ["Block"], "plane": "XZ"})));
    assert_box(world_box(&s, &copy), (Vec3::new(10.0, -30.0, 0.0), Vec3::new(20.0, -20.0, 10.0)));
    run(&mut s, "parameters.change", json!({"name": "side", "expression": "15 mm"}));
    assert_box(world_box(&s, &copy), (Vec3::new(10.0, -30.0, 0.0), Vec3::new(25.0, -20.0, 10.0)));
    assert_as_evaluated_afresh(&s);
}

/// Copies nothing builds on skip the search that readies their revolved faces for a boolean.
/// A feature added later that does build on them gets them readied, as a design opened with that
/// feature has them, whether or not they were mirrored first.
#[test]
fn a_cut_added_after_a_mirror_finds_its_copies_readied() {
    let cut = |s: &mut Session, copy: &str| {
        run(s, "solid.cylinder", json!({"base": [20, -17, 15], "radius": 3, "height": 12, "operation": "cut", "targets": [copy]}));
    };
    let mut s = Session::default();
    let (_, bodies) = curved_part(&mut s);
    let copies = run(&mut s, "solid.mirror", json!({"bodies": bodies, "plane": "XZ"}));
    let ring = copies["created_bodies"][1].as_str().unwrap().to_string();
    let evaluated = s.model.recomputed;
    cut(&mut s, &ring);
    assert!(s.model.results.iter().all(|r| r.error.is_none()));
    assert!(s.model.recomputed - evaluated >= 2, "the mirror is evaluated again with the cut");
    let measured = |s: &Session| {
        let v = run(&mut s.scratch(), "inspect.measure", json!({"bodies": [ring]}));
        v["total"]["volume_mm3"].as_f64().unwrap()
    };
    let volume = measured(&s);
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert_eq!(measured(&reopened), volume, "what evaluates here is what opens");
}

/// A part inserted after the mirror keeps its meshes while the mirror moves: evaluating the
/// insert again gives the same solids, and meshing them was as slow as meshing the copies.
#[test]
fn an_insert_after_a_mirror_keeps_its_meshes_through_a_drag() {
    let dir = std::env::temp_dir().join(format!("solidcraft-mirror-insert-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("insert.step").to_string_lossy().replace('\\', "/");
    let mut part = Session::default();
    run(&mut part, "solid.sphere", json!({"radius": 5}));
    run(&mut part, "file.export", json!({"path": file}));
    let mut s = Session::default();
    let (occurrence, bodies) = curved_part(&mut s);
    run(&mut s, "solid.mirror", json!({"bodies": bodies, "plane": "XZ"}));
    run(&mut s, "file.insert_step", json!({"path": file, "name": "After"}));
    let _ = std::fs::remove_dir_all(&dir);
    run(&mut s, "component.activate", json!({"component": 0}));
    drawn(&s);
    for k in 1..=3 {
        run(&mut s, "occurrence.drag", json!({"occurrence": occurrence, "at": [0, 0, 0], "point": [0, f64::from(k), 0], "continuous": true}));
        assert_eq!(drawn(&s), 0.0, "frame {k}");
    }
    assert_as_evaluated_afresh(&s);
}
