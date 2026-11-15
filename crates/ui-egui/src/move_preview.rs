//! Move feedback from the moved bodies' own display triangles, without a scratch design. A move
//! changes no shape, so its preview needs no evaluation: the bodies' meshes are drawn moved by
//! the move's transform. The preview from a scratch design evaluated the move and meshed the
//! bodies again for every change, so while the triad was dragged the part trailed the arrow by
//! one evaluation, longer for a larger part.
use solidcraft_engine::geom::prelude::*;
use std::sync::Arc;

use serde_json::Value;
use solidcore_record::Vec3;
use solidcraft_engine::doc::{Document, IDENTITY, Mat, apply_point, apply_vector, expr::Kind, mat_inverse, mat_mul, rigid};
use solidcraft_engine::geom::{Aabb3, Mesh};

use crate::{
    SolveApp,
    gpu::GpuScene,
    preview::{Built, Colors},
};

/// Most triangles moved this way each frame; beyond them the scratch-design preview runs.
const MAX_TRIANGLES: usize = 2_000_000;

#[derive(Default)]
pub(crate) struct MovePreview {
    cached: Option<Cached>,
}

/// The world meshes of the bodies drawn (the listed bodies, and the bodies of the components
/// copied) and the frames that place them, for the design and revision they were taken from.
struct Cached {
    document: Arc<Document>,
    revision: u64,
    names: Vec<String>,
    meshes: Vec<Arc<Mesh>>,
    frames: Vec<Mat>,
}

/// Where the components of `bodies` place them, the world for a body outside one.
fn placed_frames(app: &SolveApp, bodies: &[String]) -> Vec<Mat> {
    if app.session.doc.occurrences.is_empty() {
        return vec![IDENTITY; bodies.len()];
    }
    let frames = app.session.placed_document().body_frames(&app.session.model.state());
    bodies.iter().map(|name| frames.iter().find(|f| f.0 == *name).map_or(IDENTITY, |f| f.3)).collect()
}

impl MovePreview {
    pub(crate) fn clear(&mut self) {
        self.cached = None;
    }

    /// The preview of a `solid.move` (or its edit), or `None` to leave it to the scratch design:
    /// another command, values that don't evaluate, a body not shown, or too many triangles.
    pub(crate) fn compute(&mut self, app: &SolveApp, commands: &[(String, Value)], colors: Colors) -> Option<Result<Built, String>> {
        let [(id, params)] = commands else { return None };
        let params = match id.as_str() {
            "solid.move" => params,
            "timeline.redefine" if params.get("command").and_then(Value::as_str) == Some("solid.move") => params.get("params")?,
            _ => return None,
        };
        // A matrix is the engine's to read (`move_matrix_params`).
        if params.get("matrix").is_some() {
            return None;
        }
        let doc = &app.session.doc;
        let length = |v: &Value| match v {
            Value::Number(n) => n.as_f64(),
            Value::String(e) => doc.eval(e, Kind::Length).ok(),
            _ => None,
        };
        let translate = match params.get("translate") {
            Some(Value::Array(a)) if a.len() == 3 => Vec3::new(length(&a[0])?, length(&a[1])?, length(&a[2])?),
            None => Vec3::ZERO,
            _ => return None,
        };
        let origin = match params.get("origin") {
            Some(Value::Array(a)) if a.len() == 3 => Some(Vec3::new(length(&a[0])?, length(&a[1])?, length(&a[2])?)),
            None => None,
            _ => return None,
        };
        let given_axis = match params.get("axis") {
            Some(Value::Array(a)) if a.len() == 3 => Some(Vec3::new(a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?)),
            None => None,
            _ => return None,
        };
        let axis = given_axis.unwrap_or(Vec3::Z);
        let angle = match params.get("angle") {
            None | Some(Value::Null) => 0.0,
            Some(Value::String(e)) => doc.eval(e, Kind::Angle).ok()?,
            Some(Value::Number(n)) => doc.eval(&n.to_string(), Kind::Angle).ok()?,
            _ => return None,
        };
        if !(translate.is_finite() && axis.is_finite() && angle.is_finite() && origin.is_none_or(|o| o.is_finite()))
            || (angle != 0.0 && axis.normalized().is_none())
        {
            return None;
        }
        let bodies: Vec<String> = params.get("bodies")?.as_array()?.iter().map(|b| b.as_str().map(str::to_string)).collect::<Option<_>>()?;
        // The components copied (`occurrences`): their bodies are drawn moved, as copies of them.
        let components: Vec<Vec<u64>> = match params.get("occurrences") {
            None => Vec::new(),
            Some(Value::Array(list)) => {
                list.iter().map(|path| path.as_array()?.iter().map(Value::as_u64).collect::<Option<Vec<u64>>>()).collect::<Option<_>>()?
            }
            Some(_) => return None,
        };
        if bodies.is_empty() && components.is_empty() {
            return None;
        }
        let copy = params.get("copy").and_then(Value::as_bool) == Some(true);
        // Only a copy takes components (the engine refuses them otherwise).
        if !components.is_empty() && !copy {
            return None;
        }
        let mut names = bodies.clone();
        for path in &components {
            names.extend(app.session.occurrence_bodies(path, true).ok()?);
        }
        if self.cached.as_ref().is_none_or(|c| c.revision != app.session.revision || !Arc::ptr_eq(&c.document, &app.session.doc) || c.names != names)
        {
            let state = crate::open_meshing::session_world(&app.session);
            let meshes: Vec<Arc<Mesh>> = names.iter().map(|n| state.body(n).map(|b| b.mesh())).collect::<Option<_>>()?;
            if meshes.iter().map(|m| m.triangles.len()).sum::<usize>() > MAX_TRIANGLES {
                return None;
            }
            // A component's copy moves in the world (`occurrence.move`), so its bodies take the world's.
            let mut frames = placed_frames(app, &bodies);
            frames.resize(names.len(), IDENTITY);
            self.cached = Some(Cached { document: app.session.doc.clone(), revision: app.session.revision, names: names.clone(), meshes, frames });
        }
        let cache = self.cached.as_ref()?;
        // The move in the world as a body in `frame` takes it: about the given origin and axis,
        // which are in the world; without an origin, about the frame's origin and, without an
        // axis too, its Z (`frames::map_local`). A component's copy has the world's frame.
        let world = |frame: &Mat| {
            let o = origin.unwrap_or_else(|| apply_point(frame, Vec3::ZERO));
            let a = given_axis.unwrap_or_else(|| if origin.is_some() { Vec3::Z } else { apply_vector(frame, Vec3::Z) });
            rigid(translate, o, a, angle)
        };
        // A solid.move splits by frame and takes the values into each body's own
        // (`frames::split_move`), so each body moves as `world` says. An edit takes them into the
        // first body's frame, and the feature applies that local move to every body in its own.
        let first = *cache.frames.first()?;
        let in_first = mat_mul(&mat_mul(&mat_inverse(&first)?, &world(&first)), &first);
        let edit = id == "timeline.redefine";
        let mut scene = GpuScene::default();
        let removed = scene.color(colors.removed);
        let mut bounds = Aabb3::EMPTY;
        for (mesh, frame) in cache.meshes.iter().zip(&cache.frames) {
            let m = if edit { mat_mul(&mat_mul(frame, &in_first), &mat_inverse(frame)?) } else { world(frame) };
            draw(&mut scene, mesh, &m, colors, &mut bounds);
            if !copy && colors.cut_tint {
                // Where the body was, as the scratch-design preview shows a body moved away.
                for t in &mesh.triangles {
                    for k in t {
                        if let (Some(p), Some(n)) = (mesh.positions.get(*k as usize), mesh.normals.get(*k as usize)) {
                            scene.ghost_tri(p.to_f32(), n.to_f32(), removed);
                        }
                    }
                }
            }
        }
        Some(Ok(Built {
            scene,
            sketches: Vec::new(),
            applied: None,
            stopped: None,
            mirror_meets: None,
            placed: None,
            tapered: Vec::new(),
            replaced: if copy { Vec::new() } else { bodies },
            error: None,
            bounds,
        }))
    }
}

/// `mesh` moved by `m`, in the colour of geometry the feature adds, or the body's own where
/// the preference leaves new surface untinted (a move makes none either way), with its shaded
/// edges.
fn draw(scene: &mut GpuScene, mesh: &Mesh, m: &Mat, colors: Colors, bounds: &mut Aabb3) {
    let added = scene.color(if colors.tint_new { colors.added } else { colors.body });
    for t in &mesh.triangles {
        for k in t {
            let i = *k as usize;
            if let (Some(p), Some(n)) = (mesh.positions.get(i), mesh.normals.get(i)) {
                let p = apply_point(m, *p);
                bounds.add(p);
                scene.tri(p.to_f32(), apply_vector(m, *n).to_f32(), added);
            }
        }
    }
    let edge = scene.color(colors.edge);
    for (ei, e) in mesh.edges.iter().enumerate() {
        if !mesh.shaded_edge_visible(ei) {
            continue;
        }
        for w in e.windows(2) {
            scene.line(apply_point(m, w[0]).to_f32(), apply_point(m, w[1]).to_f32(), edge, 1.3, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Services, SolveApp};
    use serde_json::json;
    use solidcraft_engine::Session;

    fn app() -> SolveApp {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 20, "width": 10, "height": 5, "body_name": "Plate"})).unwrap();
        SolveApp::new(s, Services::default())
    }

    fn colors(app: &SolveApp) -> Colors {
        let _ = app;
        Colors::from_tokens(&solidcraft_theme::Tokens::get())
    }

    fn near(a: Vec3, b: Vec3) -> bool {
        a.dist(b) < 1e-9
    }

    /// A move's preview is the body's own triangles moved as the Move feature moves the body:
    /// no scratch design, the original hidden (replaced), and where it lands is where the
    /// feature puts it.
    #[test]
    fn a_move_is_previewed_from_the_bodys_own_triangles() {
        let app = app();
        let c = colors(&app);
        let mut pv = MovePreview::default();
        let cmds =
            [("solid.move".to_string(), json!({"bodies": ["Plate"], "translate": ["30 mm", "0 mm", "0 mm"], "axis": [0, 0, 1], "angle": "90 deg"}))];
        let built = pv.compute(&app, &cmds, c).expect("taken here").expect("built");
        assert_eq!(built.replaced, vec!["Plate".to_string()]);
        // The Move feature on the same body: the preview's extent is the moved body's.
        let mut s = app.session.scratch();
        s.execute("solid.move", &cmds[0].1).unwrap();
        let moved = crate::open_meshing::session_world(&s).body("Plate").unwrap().mesh().bounds();
        assert!(near(built.bounds.min, moved.min) && near(built.bounds.max, moved.max), "{:?} vs {moved:?}", built.bounds);
        // A copy keeps the original shown.
        let copy = [("solid.move".to_string(), json!({"bodies": ["Plate"], "translate": [0, 0, 10], "copy": true}))];
        assert!(pv.compute(&app, &copy, c).unwrap().unwrap().replaced.is_empty());
    }

    /// A turn about a given origin is drawn about that point: the Move dialog names its pivot as
    /// `origin` and leaves `translate` alone, so a preview about the world origin drew the body
    /// far from where it lands. Checked at the root and on a body in a turned, placed component,
    /// where a given origin is in the world too.
    #[test]
    fn a_turn_about_a_given_origin_is_previewed_where_the_move_puts_it() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 20, "corner": [100, 0, 0], "body_name": "Plate"})).unwrap();
        let made = s.execute("component.create", &json!({"name": "Part", "grounded": false})).unwrap();
        s.execute("solid.box", &json!({"length": 20, "width": 10, "height": 15, "body_name": "Peg"})).unwrap();
        s.execute("component.activate", &json!({"component": "root"})).unwrap();
        let occurrence = made["occurrence"].as_u64().unwrap();
        s.execute("occurrence.move", &json!({"occurrence": occurrence, "axis": [0, 0, 1], "angle": "30 deg", "translate": [-80, 20, 20]})).unwrap();
        let peg = s.occurrence_bodies(&[occurrence], true).unwrap()[0].clone();
        let app = SolveApp::new(s, Services::default());
        for body in ["Plate".to_string(), peg] {
            let p =
                json!({"bodies": [body], "origin": ["120 mm", "15 mm", "10 mm"], "axis": [1, 0, 0], "angle": "25 deg", "translate": ["5 mm", 0, 0]});
            let built =
                MovePreview::default().compute(&app, &[("solid.move".to_string(), p.clone())], colors(&app)).expect("taken here").expect("built");
            let mut s = app.session.scratch();
            s.execute("solid.move", &p).unwrap();
            let moved = s.world_state().unwrap().body(&body).unwrap().mesh().bounds();
            assert!(built.bounds.min.dist(moved.min) < 1e-6 && built.bounds.max.dist(moved.max) < 1e-6, "{body}: {:?} vs {moved:?}", built.bounds);
        }
    }

    /// A body in a turned, placed component turns about its component's origin, not the world's:
    /// previewed about the world's, it was drawn far from where the feature puts it.
    #[test]
    fn a_placed_body_is_previewed_where_the_feature_puts_it() {
        let mut s = Session::default();
        s.execute("component.create", &json!({"name": "Part", "grounded": false})).unwrap();
        s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 20})).unwrap();
        s.execute("occurrence.move", &json!({"occurrence": "Part:1", "axis": [0, 0, 1], "angle": "30 deg", "translate": [130, 20, 20]})).unwrap();
        s.execute("component.activate", &json!({"component": "root"})).unwrap();
        let app = SolveApp::new(s, Services::default());
        let cmds =
            [("solid.move".to_string(), json!({"bodies": ["Body1"], "translate": ["10 mm", "-5 mm", "0 mm"], "axis": [1, 0, 0], "angle": "25 deg"}))];
        let built = MovePreview::default().compute(&app, &cmds, colors(&app)).expect("taken here").expect("built");
        let mut s = app.session.scratch();
        s.execute("solid.move", &cmds[0].1).unwrap();
        let moved = s.world_state().unwrap().body("Body1").unwrap().mesh().bounds();
        assert!(built.bounds.min.dist(moved.min) < 1e-6 && built.bounds.max.dist(moved.max) < 1e-6, "{:?} vs {moved:?}", built.bounds);
    }

    /// Anything it can't take is left to the scratch-design preview.
    #[test]
    fn what_it_cannot_take_goes_to_the_full_preview() {
        let app = app();
        let c = colors(&app);
        let mut pv = MovePreview::default();
        for p in [
            json!({"bodies": ["Nope"], "translate": [1, 0, 0]}),
            json!({"bodies": ["Plate"], "translate": ["width + ?", 0, 0]}),
            json!({"bodies": ["Plate"], "translate": [1, 0]}),
            json!({"bodies": [], "translate": [1, 0, 0]}),
            json!({"bodies": ["Plate"], "angle": "90 deg", "origin": ["width + ?", 0, 0]}),
            json!({"bodies": ["Plate"], "angle": "90 deg", "origin": [1, 0]}),
            json!({"bodies": ["Plate"], "angle": "90 deg", "origin": "here"}),
            json!({"bodies": ["Plate"], "matrix": [0, -1, 0, 30, 1, 0, 0, 0, 0, 0, 1, 0]}),
        ] {
            assert!(pv.compute(&app, &[("solid.move".to_string(), p.clone())], c).is_none(), "{p}");
        }
        assert!(pv.compute(&app, &[("solid.extrude".to_string(), json!({}))], c).is_none());
    }

    /// A copy of a component is drawn where the copy lands: the component's bodies moved, the
    /// component itself left where it was, so the preview is not the scratch design's.
    #[test]
    fn a_component_copy_is_previewed_where_the_copy_lands() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 20, "width": 10, "height": 5, "body_name": "Plate"})).unwrap();
        let made = s.execute("component.create", &json!({"name": "Part", "grounded": false})).unwrap();
        s.execute("solid.box", &json!({"length": 20, "width": 10, "height": 15, "body_name": "Peg"})).unwrap();
        s.execute("component.activate", &json!({"component": "root"})).unwrap();
        let occurrence = made["occurrence"].as_u64().unwrap();
        let app = SolveApp::new(s, Services::default());
        let c = colors(&app);
        let mut pv = MovePreview::default();
        // The component's box is 0..20 along X; the copy lands 50 mm along.
        let copy = [("solid.move".to_string(), json!({"bodies": [], "occurrences": [[occurrence]], "copy": true, "translate": [50, 0, 0]}))];
        let built = pv.compute(&app, &copy, c).expect("taken here").expect("built");
        assert!(built.replaced.is_empty(), "a copy keeps the original shown");
        assert!(near(Vec3::new(built.bounds.min.x, 0.0, 0.0), Vec3::new(50.0, 0.0, 0.0)), "{:?}", built.bounds);
        assert!(near(Vec3::new(built.bounds.max.x, 0.0, 0.0), Vec3::new(70.0, 0.0, 0.0)), "{:?}", built.bounds);
        // With bodies listed too, the component's copy is drawn alongside the body's: the
        // component's box is 15 mm tall, the Plate 5 mm, so the height shows it is there.
        let mixed = [("solid.move".to_string(), json!({"bodies": ["Plate"], "occurrences": [[occurrence]], "copy": true, "translate": [50, 0, 0]}))];
        let built = pv.compute(&app, &mixed, c).expect("taken here").expect("built");
        assert!(near(Vec3::new(built.bounds.min.x, 0.0, 0.0), Vec3::new(50.0, 0.0, 0.0)), "{:?}", built.bounds);
        assert!(near(Vec3::new(0.0, 0.0, built.bounds.max.z), Vec3::new(0.0, 0.0, 15.0)), "{:?}", built.bounds);
    }

    /// A turned copy of a placed component turns about the world origin, as `occurrence.move`
    /// turns it, not about the origin of the component's frame as a body of it would.
    #[test]
    fn a_turned_component_copy_is_previewed_where_the_copy_lands() {
        let mut s = Session::default();
        let made = s.execute("component.create", &json!({"name": "Part", "grounded": false})).unwrap();
        s.execute("solid.box", &json!({"length": 20, "width": 10, "height": 15, "body_name": "Peg"})).unwrap();
        s.execute("component.activate", &json!({"component": "root"})).unwrap();
        let occurrence = made["occurrence"].as_u64().unwrap();
        s.execute("occurrence.move", &json!({"occurrence": occurrence, "translate": [100, 0, 0]})).unwrap();
        let app = SolveApp::new(s, Services::default());
        let p = json!({"bodies": [], "occurrences": [[occurrence]], "copy": true, "translate": [10, 0, 0], "axis": [0, 0, 1], "angle": "90 deg"});
        let built = MovePreview::default().compute(&app, &[("solid.move".to_string(), p.clone())], colors(&app)).expect("taken here").expect("built");
        let mut s = app.session.scratch();
        let copy = s.execute("solid.move", &p).unwrap()["occurrences"][0].as_u64().unwrap();
        let mut landed = Aabb3::EMPTY;
        for name in s.occurrence_bodies(&[copy], true).unwrap() {
            landed = landed.union(&s.world_state().unwrap().body(&name).unwrap().mesh().bounds());
        }
        assert!(built.bounds.min.dist(landed.min) < 1e-6 && built.bounds.max.dist(landed.max) < 1e-6, "{:?} vs {landed:?}", built.bounds);
    }

    /// Bodies in differently turned frames move apart in the world (`frames::split_move`), each by
    /// the world vector: drawn with the first body's frame, the other moved along its own axes.
    #[test]
    fn bodies_in_differently_turned_frames_are_previewed_where_the_move_puts_them() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 20, "width": 10, "height": 5, "body_name": "Plate"})).unwrap();
        let made = s.execute("component.create", &json!({"name": "Part", "grounded": false})).unwrap();
        s.execute("solid.box", &json!({"length": 20, "width": 10, "height": 15, "body_name": "Peg"})).unwrap();
        s.execute("component.activate", &json!({"component": "root"})).unwrap();
        let occurrence = made["occurrence"].as_u64().unwrap();
        s.execute("occurrence.move", &json!({"occurrence": occurrence, "axis": [0, 0, 1], "angle": "90 deg", "translate": [100, 0, 0]})).unwrap();
        let peg = s.occurrence_bodies(&[occurrence], true).unwrap()[0].clone();
        let app = SolveApp::new(s, Services::default());
        let p = json!({"bodies": ["Plate", peg], "translate": [30, 0, 0]});
        let built = MovePreview::default().compute(&app, &[("solid.move".to_string(), p.clone())], colors(&app)).expect("taken here").expect("built");
        let mut s = app.session.scratch();
        s.execute("solid.move", &p).unwrap();
        let st = s.world_state().unwrap();
        let moved = st.body("Plate").unwrap().mesh().bounds().union(&st.body(&peg).unwrap().mesh().bounds());
        assert!(built.bounds.min.dist(moved.min) < 1e-6 && built.bounds.max.dist(moved.max) < 1e-6, "{:?} vs {moved:?}", built.bounds);
    }
}
