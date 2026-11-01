//! Move feedback from the moved bodies' own display triangles, without a scratch design. A move
//! changes no shape, so its preview needs no evaluation: the bodies' meshes are drawn moved by
//! the move's transform. The preview from a scratch design evaluated the move and meshed the
//! bodies again for every change, so while the triad was dragged the part trailed the arrow by
//! one evaluation, longer for a larger part.
use std::sync::Arc;

use serde_json::Value;
use solidcraft_engine::doc::{Document, IDENTITY, Mat, apply_point, apply_vector, expr::Kind, mat_inverse, mat_mul, rigid};
use solidcraft_engine::geom::{Aabb3, Mesh, Vec3};

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

/// The listed bodies' world meshes and the frames their components place them in, for the design
/// and revision they were taken from.
struct Cached {
    document: Arc<Document>,
    revision: u64,
    bodies: Vec<String>,
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
        let axis = match params.get("axis") {
            Some(Value::Array(a)) if a.len() == 3 => Vec3::new(a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?),
            None => Vec3::Z,
            _ => return None,
        };
        let angle = match params.get("angle") {
            None | Some(Value::Null) => 0.0,
            Some(Value::String(e)) => doc.eval(e, Kind::Angle).ok()?,
            Some(Value::Number(n)) => doc.eval(&n.to_string(), Kind::Angle).ok()?,
            _ => return None,
        };
        if !(translate.is_finite() && axis.is_finite() && angle.is_finite()) || (angle != 0.0 && axis.normalized().is_none()) {
            return None;
        }
        let bodies: Vec<String> = params.get("bodies")?.as_array()?.iter().map(|b| b.as_str().map(str::to_string)).collect::<Option<_>>()?;
        if bodies.is_empty() {
            return None;
        }
        if self
            .cached
            .as_ref()
            .is_none_or(|c| c.revision != app.session.revision || !Arc::ptr_eq(&c.document, &app.session.doc) || c.bodies != bodies)
        {
            let state = app.session.world_state();
            let meshes: Vec<Arc<Mesh>> = bodies.iter().map(|n| state.body(n).map(|b| b.mesh())).collect::<Option<_>>()?;
            if meshes.iter().map(|m| m.triangles.len()).sum::<usize>() > MAX_TRIANGLES {
                return None;
            }
            let frames = placed_frames(app, &bodies);
            self.cached = Some(Cached { document: app.session.doc.clone(), revision: app.session.revision, bodies: bodies.clone(), meshes, frames });
        }
        let cache = self.cached.as_ref()?;
        // As the Move feature does (`kernel::transform`): about the origin of each body's frame,
        // with the values taken into the first body's.
        let back = mat_inverse(cache.frames.first()?)?;
        let turned = rigid(apply_vector(&back, translate), Vec3::ZERO, apply_vector(&back, axis), angle);
        let copy = params.get("copy").and_then(Value::as_bool) == Some(true);
        let mut scene = GpuScene::default();
        let removed = scene.color(colors.removed);
        let mut bounds = Aabb3::EMPTY;
        for (mesh, frame) in cache.meshes.iter().zip(&cache.frames) {
            let m = mat_mul(&mat_mul(frame, &turned), &mat_inverse(frame)?);
            draw(&mut scene, mesh, &m, colors, &mut bounds);
            if !copy {
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
            tapered: Vec::new(),
            replaced: if copy { Vec::new() } else { bodies },
            error: None,
            bounds,
        }))
    }
}

/// `mesh` moved by `m`, in the colour of geometry the feature adds, with its shaded edges.
fn draw(scene: &mut GpuScene, mesh: &Mesh, m: &Mat, colors: Colors, bounds: &mut Aabb3) {
    let added = scene.color(colors.added);
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
        Colors::from_tokens(&crate::theme::Tokens::get())
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
        let moved = s.world_state().body("Plate").unwrap().mesh().bounds();
        assert!(near(built.bounds.min, moved.min) && near(built.bounds.max, moved.max), "{:?} vs {moved:?}", built.bounds);
        // A copy keeps the original shown.
        let copy = [("solid.move".to_string(), json!({"bodies": ["Plate"], "translate": [0, 0, 10], "copy": true}))];
        assert!(pv.compute(&app, &copy, c).unwrap().unwrap().replaced.is_empty());
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
        let moved = s.world_state().body("Body1").unwrap().mesh().bounds();
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
        ] {
            assert!(pv.compute(&app, &[("solid.move".to_string(), p.clone())], c).is_none(), "{p}");
        }
        assert!(pv.compute(&app, &[("solid.extrude".to_string(), json!({}))], c).is_none());
    }
}
