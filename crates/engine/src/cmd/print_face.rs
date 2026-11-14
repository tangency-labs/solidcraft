//! Laying bodies on their print face for a slicer (#8429), or for an STL or 3MF export with
//! `lay_on_print_faces`. The bodies being sent are copied (the
//! state the export writes is already a copy), and each copy whose body has a print face is
//! turned so the face points down, its centre of mass moved onto the world origin in X and Y,
//! and the face moved onto Z = 0: one rigid transform, applied once. The design and its timeline
//! are not touched. Each body is placed on its own, so several land on the origin together and
//! the slicer arranges them.

use serde_json::Value;
use solidcore_record::Vec3;
use solidcraft_doc::{ModelState, PrintFace};
use solidcraft_geom::prelude::*;

use super::CommandSpec;
use crate::params::{bad, str_, string_list};
use crate::schema::print_face::{BodyPrintFace, PrintFaceResult, PrintFaceStatus, PrintFacesResult};
use crate::{EngineError, Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("body.print_face", "Print Face", crate::checked!("body.print_face", print_face))
        .icon("print_face")
        .key("Ctrl+B")
        .contract(&PRINT_FACE_CONTRACT)
        .output(crate::schema::output_schema_for::<PrintFaceResult>)
        .params("body: its name; point?: [x, y, z] on a flat face of it, in mm (omit to clear) → {body, face?}: marks the face a slicer export (file.send_to_slicer, file.save_mesh with slicer, file.open_in_easyprint), or an STL or 3MF export with lay_on_print_faces (file.export, file.export_stl, file.export_3mf, file.save_mesh), lays the body on: turned so the face points down at Z = 0, the centre of mass over the origin. One per body, replacing the last; flat faces of solid bodies only. A print face that is gone or no longer flat is reported in the export's warnings, and the body exported as it lies"),
    CommandSpec::new("body.print_faces", "Check Print Faces", crate::checked!("body.print_faces", print_faces))
        .icon("print_face")
        .noundo()
        .read_only()
        .contract(&PRINT_FACES_CONTRACT)
        .output(crate::schema::output_schema_for::<PrintFacesResult>)
        .params("bodies?: [names] (default every body) → {bodies: [{body, status: set | none | lost, face?, problem?}], all_set}: whether each body has a print face (body.print_face) that an export would lay it on; lost: one was marked but the face is gone or no longer flat (problem says why). all_set is true when at least one body was checked and every one is set. Read-only"),
];

const PRINT_FACE_CONTRACT: crate::schema::TypedContract<crate::schema::print_face::PrintFaceParams> = crate::schema::TypedContract::new();
const PRINT_FACES_CONTRACT: crate::schema::TypedContract<crate::schema::print_face::PrintFacesParams> = crate::schema::TypedContract::new();

/// Say whether each body has a print face, and whether an export can still find it. Changes
/// nothing.
fn print_faces(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "body.print_faces";
    let names = string_list(cmd, p, "bodies")?;
    let world = s.world_state()?;
    let bodies: Vec<&solidcraft_doc::ModelBody> = if names.is_empty() {
        world.bodies.iter().collect()
    } else {
        names.iter().map(|n| world.body(n).ok_or_else(|| bad(cmd, format!("no body `{n}`")))).collect::<Result<_>>()?
    };
    let checked: Vec<BodyPrintFace> = bodies
        .into_iter()
        .map(|b| {
            let face = s.doc.print_faces.get(b.definition_name()).map(|t| t.name.clone());
            let (status, problem) = match s.doc.print_face(&world, b) {
                None => (PrintFaceStatus::None, None),
                Some(Ok(_)) => (PrintFaceStatus::Set, None),
                Some(Err(e)) => (PrintFaceStatus::Lost, Some(e.to_string())),
            };
            BodyPrintFace { body: b.name.clone(), status, face, problem }
        })
        .collect();
    let all_set = !checked.is_empty() && checked.iter().all(|b| b.status == PrintFaceStatus::Set);
    let reply = PrintFacesResult { bodies: checked, all_set, param_warnings: Vec::new() };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

/// Mark or clear a body's print face.
fn print_face(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "body.print_face";
    let name = str_(p, "body").ok_or_else(|| bad(cmd, "give `body`"))?;
    let point = match p.get("point") {
        None | Some(Value::Null) => None,
        Some(v) => Some(crate::params::vec3(v).ok_or_else(|| bad(cmd, "`point` must be [x, y, z] of numbers"))?),
    };
    let world = s.world_state()?;
    let body = world.body(name).ok_or_else(|| bad(cmd, format!("no body `{name}`")))?;
    let key = body.definition_name().to_string();
    let face = match point {
        None => {
            s.doc_mut().print_faces.remove(&key);
            None
        }
        Some(point) => {
            let index = solidcraft_doc::appearance::face_index_at(body, point).ok_or_else(|| bad(cmd, "no face of the body at `point`"))?;
            let target = s.doc.print_face_target(body, index).map_err(|e| bad(cmd, e.to_string()))?;
            let face = target.name.clone();
            s.doc_mut().print_faces.insert(key, target);
            Some(face)
        }
    };
    let reply = PrintFaceResult { body: name.to_string(), face, param_warnings: Vec::new() };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

/// Which bodies were laid on their print face, and a warning for each that has one but was
/// exported as it lies.
#[derive(Debug, Default)]
pub(super) struct Laid {
    pub bodies: Vec<String>,
    pub warnings: Vec<String>,
}

impl Laid {
    /// Add what happened to an export's reply.
    pub(super) fn report(self, out: &mut serde_json::Value) {
        let Some(obj) = out.as_object_mut() else { return };
        if !self.bodies.is_empty() {
            obj.insert("print_faces".into(), serde_json::json!(self.bodies));
        }
        if !self.warnings.is_empty() {
            obj.insert("warnings".into(), serde_json::json!(self.warnings));
        }
    }
}

/// Copies keep the print faces of the bodies they were made from (Move/Copy, Mirror), as they
/// keep their look: the copies are the bodies feature `reply` made, in the order of `sources`.
/// A copy whose face can't be matched is left without one.
pub(super) fn copies_keep_print_faces(s: &mut Session, reply: &Value, sources: &[String]) {
    let Some(id) = reply.get("feature").and_then(Value::as_u64) else { return };
    let state = s.model.state();
    let copies: Vec<_> = state.bodies.iter().filter(|b| b.feature == id).collect();
    if copies.len() != sources.len() {
        return;
    }
    let mut inherited = Vec::new();
    for (source, copy) in sources.iter().zip(copies) {
        let Some(source) = state.body(source) else { continue };
        if let Some(Ok(target)) = s.doc.copied_print_face(&state, source, copy) {
            inherited.push((copy.definition_name().to_string(), target));
        }
    }
    for (body, target) in inherited {
        s.doc_mut().print_faces.insert(body, target);
    }
}

/// Lay the bodies of `state` named in `bodies` (all of them when it is empty) on their print faces.
/// A body that is one of several parts of a 3MF object (`parts`, `objects: component`) stays as it
/// lies, so the object keeps its shape: turning one part on its own would pull it out of the rest.
pub(super) fn lay_on_print_faces(s: &Session, state: &mut ModelState, bodies: &[String], parts: &[(String, Vec<String>)]) -> Laid {
    let mut laid = Laid::default();
    let mut moves = Vec::new();
    for (i, b) in state.bodies.iter().enumerate() {
        if !bodies.is_empty() && !bodies.contains(&b.name) {
            continue;
        }
        if let Some((object, _)) = parts.iter().find(|(_, members)| members.contains(&b.name)) {
            if s.doc.print_faces.contains_key(b.definition_name()) {
                laid.warnings.push(format!("`{}` was exported as it lies: it is a part of the object `{object}`, which keeps its shape", b.name));
            }
            continue;
        }
        let face = match s.doc.print_face(state, b) {
            None => continue,
            Some(Ok(face)) => face,
            Some(Err(e)) => {
                laid.warnings.push(format!("`{}` was exported as it lies: {e}", b.name));
                continue;
            }
        };
        let centre = match solidcraft_kernel::measure_mass(&b.body) {
            Ok(m) => m.centroid,
            Err(e) => {
                laid.warnings.push(format!("`{}` was exported as it lies: no centre of mass ({e})", b.name));
                continue;
            }
        };
        moves.push((i, bed_matrix(face, centre)));
    }
    for (i, m) in moves {
        let Some(b) = state.bodies.get_mut(i) else { continue };
        match solidcraft_kernel::transform_matrix(&b.body, m) {
            Ok(body) => {
                b.body = body;
                laid.bodies.push(b.name.clone());
            }
            Err(e) => laid.warnings.push(format!("`{}` was exported as it lies: {e}", b.name)),
        }
    }
    laid
}

/// The rotation (rows) that turns unit vector `n` to −Z. A face that already points down is
/// left as it is; one that points straight up is turned half a turn about X.
fn turn_down(n: Vec3) -> [Vec3; 3] {
    let down = Vec3::new(0., 0., -1.);
    let c = n.dot(down);
    let v = n.cross(down);
    let s2 = v.len2();
    if s2 <= 1e-24 {
        return if c > 0. { [Vec3::X, Vec3::Y, Vec3::Z] } else { [Vec3::X, -Vec3::Y, -Vec3::Z] };
    }
    // Rodrigues: R e = e + v × e + v × (v × e) (1 − c) / |v|²; these are R's columns.
    let k = (1. - c) / s2;
    let [x, y, z] = [Vec3::X, Vec3::Y, Vec3::Z].map(|e| e + v.cross(e) + v.cross(v.cross(e)) * k);
    [Vec3::new(x.x, y.x, z.x), Vec3::new(x.y, y.y, z.y), Vec3::new(x.z, y.z, z.z)]
}

/// The column-major transform that lays `face` on the bed with `centre` above the origin.
fn bed_matrix(face: PrintFace, centre: Vec3) -> [[f64; 4]; 4] {
    let r = turn_down(face.normal);
    let apply = |p: Vec3| Vec3::new(r[0].dot(p), r[1].dot(p), r[2].dot(p));
    let c = apply(centre);
    let z = apply(face.point).z;
    let t = Vec3::new(-c.x, -c.y, -z);
    [[r[0].x, r[1].x, r[2].x, 0.], [r[0].y, r[1].y, r[2].y, 0.], [r[0].z, r[1].z, r[2].z, 0.], [t.x, t.y, t.z, 1.]]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(s: &mut Session, id: &str, p: Value) -> Value {
        match s.execute(id, &p) {
            Ok(v) => v,
            Err(e) => panic!("{id} {p}: {e}"),
        }
    }

    /// A box `L` x 10 x 5 (L = 20 mm) from the origin, called Block.
    fn block() -> Session {
        let mut s = Session::default();
        run(&mut s, "parameters.add", json!({"name": "L", "expression": "20 mm"}));
        run(&mut s, "solid.box", json!({"length": "L", "width": 10, "height": 5, "corner": [0, 0, 0], "body_name": "Block"}));
        s
    }

    /// Lay every body of `s` on its print face, as a slicer export does: Block's box and centre
    /// of mass afterwards, and what was reported.
    fn laid(s: &Session) -> (solidcraft_geom::Aabb3, Vec3, Laid) {
        let mut state = s.doc.painted(&s.world_state().unwrap());
        let laid = lay_on_print_faces(s, &mut state, &[], &[]);
        let block = state.bodies.iter().find(|b| b.name == "Block").expect("Block");
        let m = solidcraft_kernel::measure_mass(&block.body).expect("measure");
        (m.bbox, m.centroid, laid)
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn a_part_of_a_3mf_object_keeps_its_place_in_the_object() {
        // With `objects: component` the bodies of a component are the parts of one object:
        // turning one of them on its own would pull it out of the others, so it stays as it lies.
        let mut s = block();
        run(&mut s, "body.print_face", json!({"body": "Block", "point": [20, 5, 2.5]}));
        let mut state = s.doc.painted(&s.world_state().unwrap());
        let parts = [("Bracket".to_string(), vec!["Block".to_string(), "Other".to_string()])];
        let laid = lay_on_print_faces(&s, &mut state, &[], &parts);
        assert!(laid.bodies.is_empty(), "{:?}", laid.bodies);
        assert_eq!(laid.warnings.len(), 1, "{:?}", laid.warnings);
        assert!(laid.warnings.first().is_some_and(|w| w.contains("Block") && w.contains("Bracket")), "{:?}", laid.warnings);
        let block = state.bodies.iter().find(|b| b.name == "Block").expect("Block");
        let bbox = solidcraft_kernel::measure_mass(&block.body).expect("measure").bbox;
        assert!(near(bbox.min.x, 0.) && near(bbox.max.x, 20.) && near(bbox.max.z, 5.), "{bbox:?}");
    }

    #[test]
    fn a_side_face_is_laid_down_at_z_0_with_the_centre_of_mass_over_the_origin() {
        let mut s = block();
        let reply = run(&mut s, "body.print_face", json!({"body": "Block", "point": [20, 5, 2.5]}));
        assert!(reply["face"].is_string(), "{reply}");
        let (bbox, centre, laid) = laid(&s);
        assert_eq!(laid.bodies, ["Block"]);
        assert!(laid.warnings.is_empty(), "{:?}", laid.warnings);
        // The 10 x 5 end face is on the bed, so the 20 mm length now stands up.
        assert!(near(bbox.min.z, 0.) && near(bbox.max.z, 20.), "{bbox:?}");
        assert!(near(centre.x, 0.) && near(centre.y, 0.) && near(centre.z, 10.), "{centre:?}");
        assert!(near((bbox.max.x - bbox.min.x) * (bbox.max.y - bbox.min.y), 50.), "{bbox:?}");
        // The design itself has not moved.
        let m = solidcraft_kernel::measure_mass(&s.model.state().bodies[0].body).expect("measure");
        assert!(near(m.bbox.min.x, 0.) && near(m.bbox.max.x, 20.) && near(m.bbox.max.z, 5.), "{:?}", m.bbox);
    }

    #[test]
    fn a_face_that_points_up_is_turned_over() {
        let mut s = block();
        run(&mut s, "body.print_face", json!({"body": "Block", "point": [10, 5, 5]}));
        let (bbox, centre, laid) = laid(&s);
        assert_eq!(laid.bodies, ["Block"]);
        assert!(near(bbox.min.z, 0.) && near(bbox.max.z, 5.), "{bbox:?}");
        assert!(near(bbox.max.x - bbox.min.x, 20.) && near(bbox.max.y - bbox.min.y, 10.), "{bbox:?}");
        assert!(near(centre.x, 0.) && near(centre.y, 0.) && near(centre.z, 2.5), "{centre:?}");
    }

    #[test]
    fn the_print_face_follows_the_body_through_a_rebuild() {
        let mut s = block();
        run(&mut s, "body.print_face", json!({"body": "Block", "point": [20, 5, 2.5]}));
        run(&mut s, "parameters.change", json!({"name": "L", "expression": "30 mm"}));
        let (bbox, centre, laid) = laid(&s);
        assert!(laid.warnings.is_empty(), "{:?}", laid.warnings);
        assert!(near(bbox.min.z, 0.) && near(bbox.max.z, 30.), "{bbox:?}");
        assert!(near(centre.x, 0.) && near(centre.y, 0.) && near(centre.z, 15.), "{centre:?}");
    }

    #[test]
    fn without_a_print_face_the_body_is_exported_as_it_lies() {
        let mut s = block();
        run(&mut s, "body.print_face", json!({"body": "Block", "point": [10, 5, 5]}));
        let reply = run(&mut s, "body.print_face", json!({"body": "Block"}));
        assert!(reply.get("face").is_none(), "{reply}");
        assert!(s.doc.print_faces.is_empty());
        let (bbox, _, laid) = laid(&s);
        assert!(laid.bodies.is_empty() && laid.warnings.is_empty());
        assert!(near(bbox.min.x, 0.) && near(bbox.max.x, 20.) && near(bbox.min.z, 0.) && near(bbox.max.z, 5.), "{bbox:?}");
    }

    #[test]
    fn a_print_face_that_was_cut_away_is_reported_and_the_body_exported_as_it_lies() {
        let mut s = block();
        run(&mut s, "body.print_face", json!({"body": "Block", "point": [10, 5, 5]}));
        // A cut takes the top 2 mm off: the marked face is gone; the new top is the cut's.
        run(&mut s, "solid.box", json!({"length": 30, "width": 20, "height": 5, "corner": [-5, -5, 3], "operation": "cut"}));
        let (bbox, _, laid) = laid(&s);
        assert!(laid.bodies.is_empty());
        assert_eq!(laid.warnings.len(), 1, "{:?}", laid.warnings);
        assert!(laid.warnings[0].contains("Block"), "{:?}", laid.warnings);
        assert!(near(bbox.min.x, 0.) && near(bbox.max.z, 3.), "{bbox:?}");
    }

    #[test]
    fn print_faces_says_which_bodies_have_one_and_changes_nothing() {
        let mut s = block();
        run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [40, 0, 0], "body_name": "Spacer"}));
        let undo = s.undo.len();
        let r = run(&mut s, "body.print_faces", json!({}));
        assert_eq!(r["all_set"], false, "{r}");
        assert_eq!(r["bodies"], json!([{"body": "Block", "status": "none"}, {"body": "Spacer", "status": "none"}]), "{r}");

        let marked = run(&mut s, "body.print_face", json!({"body": "Block", "point": [10, 5, 5]}));
        let undo_after_mark = s.undo.len();
        let r = run(&mut s, "body.print_faces", json!({}));
        assert_eq!(r["bodies"][0]["status"], "set", "{r}");
        assert_eq!(r["bodies"][0]["face"], marked["face"], "{r}");
        assert_eq!(r["bodies"][1]["status"], "none", "{r}");
        assert_eq!(r["all_set"], false, "{r}");

        // Only the bodies asked for, in the order asked for.
        let r = run(&mut s, "body.print_faces", json!({"bodies": ["Block"]}));
        assert_eq!(r["all_set"], true, "{r}");
        assert_eq!(r["bodies"].as_array().map(Vec::len), Some(1), "{r}");
        run(&mut s, "body.print_face", json!({"body": "Spacer", "point": [42.5, 2.5, 0]}));
        let r = run(&mut s, "body.print_faces", json!({"bodies": ["Spacer", "Block"]}));
        assert_eq!(r["bodies"][0]["body"], "Spacer", "{r}");
        assert_eq!(r["all_set"], true, "{r}");

        // Checking is not an undo step and changes no print face.
        assert!(undo < undo_after_mark);
        let before = s.doc.print_faces.clone();
        let steps = s.undo.len();
        run(&mut s, "body.print_faces", json!({}));
        assert_eq!(s.undo.len(), steps);
        assert_eq!(s.doc.print_faces, before);
    }

    #[test]
    fn print_faces_reports_a_face_that_was_cut_away_as_lost() {
        let mut s = block();
        run(&mut s, "body.print_face", json!({"body": "Block", "point": [10, 5, 5]}));
        run(&mut s, "solid.box", json!({"length": 30, "width": 20, "height": 5, "corner": [-5, -5, 3], "operation": "cut"}));
        let r = run(&mut s, "body.print_faces", json!({"bodies": ["Block"]}));
        let b = &r["bodies"][0];
        assert_eq!(b["status"], "lost", "{r}");
        assert!(b["face"].is_string() && b["problem"].as_str().is_some_and(|p| !p.is_empty()), "{r}");
        assert_eq!(r["all_set"], false, "{r}");
    }

    #[test]
    fn print_faces_refuses_an_unknown_body_or_a_bad_list_and_is_false_with_no_bodies() {
        let mut s = block();
        let e = s.execute("body.print_faces", &json!({"bodies": ["Bracket"]})).unwrap_err().to_string();
        assert!(e.contains("Bracket"), "{e}");
        let e = s.execute("body.print_faces", &json!({"bodies": 3})).unwrap_err().to_string();
        assert!(e.contains("bodies"), "{e}");
        let e = s.execute("body.print_faces", &json!({"bodies": [1]})).unwrap_err().to_string();
        assert!(e.contains("bodies"), "{e}");
        let r = run(&mut Session::default(), "body.print_faces", json!({}));
        assert_eq!(r, json!({"bodies": [], "all_set": false}));
    }

    #[test]
    fn a_print_face_is_saved_with_the_design_and_an_older_design_has_none() {
        let mut s = block();
        run(&mut s, "body.print_face", json!({"body": "Block", "point": [10, 5, 5]}));
        let saved = serde_json::to_value(&*s.doc).expect("save");
        assert!(saved["print_faces"]["Block"].is_object(), "{}", saved["print_faces"]);
        let opened: solidcraft_doc::Document = serde_json::from_value(saved.clone()).expect("open");
        assert_eq!(opened.print_faces, s.doc.print_faces);
        let mut older = saved;
        older.as_object_mut().expect("object").remove("print_faces");
        let opened: solidcraft_doc::Document = serde_json::from_value(older).expect("open");
        assert!(opened.print_faces.is_empty());
    }

    /// A file in a fresh folder of the system's temporary folder, for `case`.
    fn temp_file(case: &str) -> String {
        let dir = std::env::temp_dir().join(format!("solidcraft-print-faces-{case}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temporary folder");
        dir.join("block.solidcraft").to_string_lossy().into_owned()
    }

    #[test]
    fn without_keep_print_faces_a_save_leaves_them_out_and_the_session_keeps_them() {
        let mut s = block();
        s.keep_print_faces = false;
        run(&mut s, "body.print_face", json!({"body": "Block", "point": [10, 5, 5]}));
        let path = temp_file("save");
        run(&mut s, "file.save", json!({"path": path}));
        // The design on screen still has it, and is not left with unsaved changes.
        assert!(s.doc.print_faces.contains_key("Block"));
        assert!(!s.is_dirty());
        let mut opened = Session::default();
        run(&mut opened, "doc.open", json!({"path": path}));
        assert!(opened.doc.print_faces.is_empty(), "the file has no print face");
        assert_eq!(opened.doc.features.len(), s.doc.features.len());
        assert_eq!(opened.doc.params, s.doc.params);
        // With keep_print_faces on (the engine's default), the file keeps it, as before.
        s.keep_print_faces = true;
        run(&mut s, "file.save", json!({"path": path}));
        let mut opened = Session::default();
        run(&mut opened, "doc.open", json!({"path": path}));
        assert_eq!(opened.doc.print_faces, s.doc.print_faces);
    }

    #[test]
    fn a_design_saved_with_print_faces_opens_without_them_unless_they_are_kept() {
        let mut s = block();
        run(&mut s, "material.assign", json!({"bodies": ["Block"], "material": "Brass"}));
        run(&mut s, "body.print_face", json!({"body": "Block", "point": [10, 5, 5]}));
        // Saved as an older version saved it: with the print face in the file.
        let path = temp_file("older");
        run(&mut s, "file.save", json!({"path": path}));
        let mut opened = Session { keep_print_faces: false, ..Session::default() };
        run(&mut opened, "doc.open", json!({"path": path}));
        assert!(opened.doc.print_faces.is_empty(), "the stored print face is dropped as the design opens");
        assert!(!opened.is_dirty(), "dropping it is not an unsaved change");
        assert!(opened.undo.is_empty(), "nor a step to undo");
        // Everything else the file holds is there.
        assert_eq!(opened.doc.features.len(), s.doc.features.len());
        assert_eq!(opened.doc.params, s.doc.params);
        assert_eq!(opened.doc.materials, s.doc.materials);
        assert_eq!(opened.model.state().bodies.len(), 1);
        // The setting is the user's: it survives the open.
        assert!(!opened.keep_print_faces);
        // Kept, the design opens with its print face.
        let mut kept = Session::default();
        run(&mut kept, "doc.open", json!({"path": path}));
        assert_eq!(kept.doc.print_faces, s.doc.print_faces);
    }

    /// `name` in a fresh folder of the system's temporary folder, for `case`.
    fn temp_path(case: &str, name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("solidcraft-lay-on-print-faces-{case}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temporary folder");
        dir.join(name).to_string_lossy().into_owned()
    }

    /// The box around every triangle of the STL file at `path`, in mm.
    fn stl_box(path: &str) -> ([f64; 3], [f64; 3]) {
        let triangles = solidcraft_io::read_stl(&std::fs::read(path).expect("the STL file")).expect("an STL file");
        assert!(!triangles.is_empty(), "{path} has no triangles");
        let (mut min, mut max) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for p in triangles.iter().flatten() {
            for k in 0..3 {
                min[k] = min[k].min(f64::from(p[k]));
                max[k] = max[k].max(f64::from(p[k]));
            }
        }
        (min, max)
    }

    /// Near enough for a coordinate an STL file holds as an f32.
    fn near_f32(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn an_stl_export_lays_the_body_on_its_print_face_only_when_asked() {
        // The 10 x 5 end at X = 20 is the print face, so laid the 20 mm length stands up at Z = 0,
        // centred on the origin in X and Y.
        let mut s = block();
        run(&mut s, "body.print_face", json!({"body": "Block", "point": [20, 5, 2.5]}));
        let undo = s.undo.len();
        let laid_box = |(min, max): ([f64; 3], [f64; 3])| {
            near_f32(min[2], 0.)
                && near_f32(max[2], 20.)
                && near_f32(min[0] + max[0], 0.)
                && near_f32(min[1] + max[1], 0.)
                && near_f32((max[0] - min[0]) * (max[1] - min[1]), 50.)
        };
        for (id, name, extra) in [
            ("file.export_stl", "a.stl", json!({})),
            ("file.export", "b.stl", json!({})),
            ("file.export", "c.stl", json!({"format": "stla"})),
            ("file.save_mesh", "d.stl", json!({})),
            ("file.save_mesh", "e.stl", json!({"format": "stl", "ascii": true})),
        ] {
            let path = temp_path("stl", name);
            let mut p = json!({"path": path, "lay_on_print_faces": true});
            p.as_object_mut().expect("object").extend(extra.as_object().expect("object").clone());
            let reply = run(&mut s, id, p);
            assert_eq!(reply["print_faces"], json!(["Block"]), "{id}: {reply}");
            assert!(reply.get("warnings").is_none(), "{id}: {reply}");
            let bbox = stl_box(&path);
            assert!(laid_box(bbox), "{id} {name}: {bbox:?}");
            // Without the key, or with it false, the body is written as it lies in the design.
            for lay in [json!(null), json!(false)] {
                let path = temp_path("stl-as-it-lies", name);
                let mut p = json!({"path": path, "lay_on_print_faces": lay});
                p.as_object_mut().expect("object").extend(extra.as_object().expect("object").clone());
                let reply = run(&mut s, id, p);
                assert!(reply.get("print_faces").is_none(), "{id}: {reply}");
                let (min, max) = stl_box(&path);
                assert!(near_f32(min[0], 0.) && near_f32(max[0], 20.) && near_f32(min[2], 0.) && near_f32(max[2], 5.), "{id}: {min:?} {max:?}");
            }
        }
        // The design itself has not moved, and nothing was added to undo.
        assert_eq!(s.undo.len(), undo);
        let m = solidcraft_kernel::measure_mass(&s.model.state().bodies[0].body).expect("measure");
        assert!(near(m.bbox.min.x, 0.) && near(m.bbox.max.x, 20.) && near(m.bbox.max.z, 5.), "{:?}", m.bbox);
    }

    #[test]
    fn a_3mf_export_reports_the_bodies_it_laid_on_their_print_face() {
        let mut s = block();
        run(&mut s, "body.print_face", json!({"body": "Block", "point": [20, 5, 2.5]}));
        for (id, name) in [("file.export_3mf", "a.3mf"), ("file.export", "b.3mf"), ("file.save_mesh", "c.3mf")] {
            let path = temp_path("3mf", name);
            let reply = run(&mut s, id, json!({"path": path, "lay_on_print_faces": true}));
            assert_eq!(reply["print_faces"], json!(["Block"]), "{id}: {reply}");
            assert!(std::fs::metadata(&path).is_ok_and(|m| m.len() > 0), "{id}: {path}");
        }
    }

    #[test]
    fn an_export_with_lay_on_print_faces_writes_a_body_without_one_as_it_lies() {
        // A second body with no print face lies where it is; a print face cut away is reported.
        let mut s = block();
        run(&mut s, "solid.box", json!({"length": 4, "width": 4, "height": 4, "corner": [50, 0, 0], "body_name": "Cube", "operation": "new"}));
        let path = temp_path("none", "cube.stl");
        let reply = run(&mut s, "file.export_stl", json!({"path": path, "bodies": ["Cube"], "lay_on_print_faces": true}));
        assert!(reply.get("print_faces").is_none() && reply.get("warnings").is_none(), "{reply}");
        let (min, max) = stl_box(&path);
        assert!(near_f32(min[0], 50.) && near_f32(max[0], 54.) && near_f32(max[2], 4.), "{min:?} {max:?}");
        run(&mut s, "body.print_face", json!({"body": "Block", "point": [10, 5, 5]}));
        run(&mut s, "solid.box", json!({"length": 30, "width": 20, "height": 5, "corner": [-5, -5, 3], "operation": "cut"}));
        let path = temp_path("cut", "block.stl");
        let reply = run(&mut s, "file.export_stl", json!({"path": path, "bodies": ["Block"], "lay_on_print_faces": true}));
        assert!(reply.get("print_faces").is_none(), "{reply}");
        assert!(reply["warnings"].as_array().is_some_and(|w| w.len() == 1 && w[0].as_str().is_some_and(|w| w.contains("Block"))), "{reply}");
        let (min, max) = stl_box(&path);
        assert!(near_f32(min[0], 0.) && near_f32(max[0], 20.) && near_f32(max[2], 3.), "{min:?} {max:?}");
    }

    #[test]
    fn lay_on_print_faces_is_refused_for_a_format_that_is_not_printed_or_a_value_that_is_not_true_or_false() {
        let mut s = block();
        run(&mut s, "body.print_face", json!({"body": "Block", "point": [20, 5, 2.5]}));
        for (id, name, lay) in [
            ("file.export", "a.step", json!(true)),
            ("file.export", "a.obj", json!(true)),
            ("file.export", "a.glb", json!(true)),
            ("file.export", "a.json", json!(true)),
            ("file.export", "b.stl", json!("yes")),
            ("file.export_stl", "c.stl", json!(1)),
            ("file.export_3mf", "d.3mf", json!([true])),
            ("file.save_mesh", "e.stl", json!({})),
        ] {
            let path = temp_path("refused", name);
            let e = s.execute(id, &json!({"path": path, "lay_on_print_faces": lay})).expect_err(id).to_string();
            assert!(e.contains("lay_on_print_faces"), "{id} {name}: {e}");
            assert!(std::fs::metadata(&path).is_err(), "{id} {name}: a refused export writes nothing");
        }
        // False is accepted for any format: the file is written as the design has it.
        let path = temp_path("false", "a.step");
        run(&mut s, "file.export", json!({"path": path, "lay_on_print_faces": false}));
        assert!(std::fs::metadata(&path).is_ok());
    }

    #[test]
    fn renaming_the_body_keeps_its_print_face() {
        let mut s = block();
        run(&mut s, "body.print_face", json!({"body": "Block", "point": [10, 5, 5]}));
        run(&mut s, "body.rename", json!({"body": "Block", "name": "Lid"}));
        assert!(s.doc.print_faces.contains_key("Lid") && !s.doc.print_faces.contains_key("Block"));
        assert_eq!(s.doc.print_faces["Lid"].body, "Lid");
    }

    /// Every body laid on its print face: (name, box, centre of mass) of those laid.
    fn all_laid(s: &Session) -> Vec<(String, solidcraft_geom::Aabb3, Vec3)> {
        let mut state = s.doc.painted(&s.world_state().unwrap());
        let laid = lay_on_print_faces(s, &mut state, &[], &[]);
        assert!(laid.warnings.is_empty(), "{:?}", laid.warnings);
        laid.bodies
            .iter()
            .map(|name| {
                let b = state.bodies.iter().find(|b| &b.name == name).expect("laid body");
                let m = solidcraft_kernel::measure_mass(&b.body).expect("measure");
                (name.clone(), m.bbox, m.centroid)
            })
            .collect()
    }

    #[test]
    fn a_copy_keeps_the_print_face_of_the_body_it_was_copied_from() {
        let mut s = block();
        run(&mut s, "body.print_face", json!({"body": "Block", "point": [20, 5, 2.5]}));
        run(&mut s, "solid.move", json!({"bodies": ["Block"], "translate": [0, 50, 0], "copy": true}));
        assert_eq!(s.doc.print_faces.len(), 2, "{:?}", s.doc.print_faces.keys().collect::<Vec<_>>());
        let laid = all_laid(&s);
        assert_eq!(laid.len(), 2, "{laid:?}");
        for (name, bbox, centre) in &laid {
            assert!(near(bbox.min.z, 0.) && near(bbox.max.z, 20.), "{name}: {bbox:?}");
            assert!(near(centre.x, 0.) && near(centre.y, 0.), "{name}: {centre:?}");
        }
    }

    #[test]
    fn a_mirrored_copy_keeps_the_mirrored_print_face() {
        let mut s = block();
        // The end at X = 20; its mirror image in YZ is the copy's end at X = -20.
        run(&mut s, "body.print_face", json!({"body": "Block", "point": [20, 5, 2.5]}));
        run(&mut s, "solid.mirror", json!({"bodies": ["Block"], "plane": "YZ"}));
        assert_eq!(s.doc.print_faces.len(), 2, "{:?}", s.doc.print_faces.keys().collect::<Vec<_>>());
        let copy = s.doc.print_faces.keys().find(|k| *k != "Block").expect("the copy's print face").clone();
        let world = s.world_state().unwrap();
        let body = world.body(&copy).expect("copy");
        let face = s.doc.print_face(&world, body).expect("marked").expect("found");
        assert!(near(face.point.x, -20.) && near(face.normal.x, -1.), "{face:?}");
        let laid = all_laid(&s);
        assert_eq!(laid.len(), 2, "{laid:?}");
        for (name, bbox, _) in &laid {
            assert!(near(bbox.min.z, 0.) && near(bbox.max.z, 20.), "{name}: {bbox:?}");
        }
    }

    #[test]
    fn a_copy_of_a_body_without_a_print_face_gets_none() {
        let mut s = block();
        run(&mut s, "solid.move", json!({"bodies": ["Block"], "translate": [0, 50, 0], "copy": true}));
        run(&mut s, "solid.mirror", json!({"bodies": ["Block"], "plane": "XZ"}));
        assert!(s.doc.print_faces.is_empty());
    }

    #[test]
    fn a_point_off_the_body_or_a_body_that_is_not_there_is_refused() {
        let mut s = block();
        assert!(s.execute("body.print_face", &json!({"body": "Block", "point": [100, 100, 100]})).is_err());
        assert!(s.execute("body.print_face", &json!({"body": "Nope", "point": [10, 5, 5]})).is_err());
        assert!(s.execute("body.print_face", &json!({"body": "Block", "point": "top"})).is_err());
        assert!(s.doc.print_faces.is_empty());
    }

    fn turned(n: Vec3) -> Vec3 {
        let r = turn_down(n);
        Vec3::new(r[0].dot(n), r[1].dot(n), r[2].dot(n))
    }

    #[test]
    fn every_normal_is_turned_to_point_down_by_a_rotation() {
        let down = Vec3::new(0., 0., -1.);
        for n in [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z, Vec3::new(1., 2., 3.), Vec3::new(-0.3, 0.1, 0.9), Vec3::new(1e-9, 0., 1.)]
        {
            let n = n.normalized().unwrap();
            assert!(turned(n).dist(down) < 1e-12, "{n:?} → {:?}", turned(n));
            let r = turn_down(n);
            // Orthonormal rows with a positive determinant: a rotation, never a mirror.
            for i in 0..3 {
                assert!((r[i].len() - 1.).abs() < 1e-12);
                for j in i + 1..3 {
                    assert!(r[i].dot(r[j]).abs() < 1e-12);
                }
            }
            assert!((r[0].cross(r[1]).dot(r[2]) - 1.).abs() < 1e-12);
        }
    }

    #[test]
    fn a_face_that_points_straight_up_is_turned_half_a_turn_about_x() {
        let r = turn_down(Vec3::Z);
        assert_eq!(r, [Vec3::X, -Vec3::Y, -Vec3::Z]);
    }

    #[test]
    fn the_bed_matrix_puts_the_face_at_z_0_and_the_centre_over_the_origin() {
        let face = PrintFace { index: 0, point: Vec3::new(3., 4., 10.), normal: Vec3::Z };
        let m = bed_matrix(face, Vec3::new(3., 4., 5.));
        let apply = |p: Vec3| {
            Vec3::new(
                m[0][0] * p.x + m[1][0] * p.y + m[2][0] * p.z + m[3][0],
                m[0][1] * p.x + m[1][1] * p.y + m[2][1] * p.z + m[3][1],
                m[0][2] * p.x + m[1][2] * p.y + m[2][2] * p.z + m[3][2],
            )
        };
        assert!(apply(face.point).z.abs() < 1e-12);
        let c = apply(Vec3::new(3., 4., 5.));
        assert!(c.x.abs() < 1e-12 && c.y.abs() < 1e-12 && c.z > 0., "{c:?}");
    }
}
