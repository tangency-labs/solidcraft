//! A portable picture from warm display meshes only; never evaluate on save.
use crate::Session;
use solidcraft_geom::Aabb3;
use solidcraft_render::{Camera, Rgb, Scene, SceneMesh, StandardView};

mod meshes;

pub(crate) fn saved(s: &Session) -> Option<Vec<u8>> {
    // Drawn by the renderer the host installed (the GPU); none installed, no thumbnail.
    let renderer = crate::capture::installed()?;
    let (scene, camera) = scene(s)?;
    let [width, height] = solidcraft_io::THUMBNAIL_SIZE;
    crate::capture::png_with(Some(&*renderer), &scene, &camera, u32::try_from(width).ok()?, u32::try_from(height).ok()?).ok().map(|p| p.bytes)
}

/// What the thumbnail shows: the shown bodies' warm display meshes in their colours, iso, fitted.
fn scene(s: &Session) -> Option<(Scene, Camera)> {
    if s.active_sketch.is_some() || !s.pending_moves.is_empty() || s.model.results.iter().any(|r| r.error.is_some()) {
        return None;
    }
    let world = s.cached_world_state()?;
    if world.bodies.is_empty() || world.bodies.len() > 1024 {
        return None;
    }
    let mut scene = Scene::default();
    let mut bounds = Aabb3::EMPTY;
    let mut budget = meshes::Budget::default();
    for body in world.bodies.iter().filter(|b| s.body_visible(&b.name)) {
        let mesh = budget.placed(body)?;
        bounds = bounds.union(&mesh.bounds());
        let channel = |x: f32| (x.clamp(0., 1.) * 255.).round() as u8;
        let color = s.doc.body_color(body).map_or(crate::view::colors::BODY, |[r, g, b]| Rgb(channel(r), channel(g), channel(b)));
        let face_colors =
            s.doc.face_colors(body).into_iter().map(|(face, look)| (face as u32, Rgb(look.color[0], look.color[1], look.color[2]))).collect();
        let opacity = s.doc.model_body_opacity(body) as f32;
        scene.meshes.push(SceneMesh { mesh, color, face_colors, opacity });
    }
    if bounds.is_empty() {
        return None;
    }
    let mut camera = Camera::default();
    camera.set_view(StandardView::Iso);
    camera.fit(&bounds);
    camera.distance *= 0.7;
    scene.radius = (bounds.diagonal() * 0.5).max(camera.half_height()).max(10.);
    scene.background = Some((crate::view::colors::BG_TOP, crate::view::colors::BG_BOTTOM));
    Some((scene, camera))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_thumbnail_preserves_saved_visibility() {
        crate::capture::install_test_renderer();
        let mut session = Session::default();
        session.execute("solid.box", &serde_json::json!({"length":10,"width":20,"height":30,"body_name":"Visible"})).unwrap();
        session.execute("solid.box", &serde_json::json!({"length":100,"width":100,"height":100,"body_name":"Hidden"})).unwrap();
        session.execute("browser.visibility", &serde_json::json!({"items":["Hidden"],"visible":false})).unwrap();
        let world = session.world_state();
        for body in &world.bodies {
            body.mesh();
        }
        assert_eq!(scene(&session).unwrap().0.meshes.len(), 1, "the hidden body is not in the picture");
        let expected = saved(&session).unwrap();
        let bytes = session.design_container(solidcraft_io::DesignCompression::Fast, false, true).unwrap();
        let doc = solidcraft_io::read_design(&bytes).unwrap();
        assert_eq!(doc.visibility.hidden_bodies, vec!["Hidden"]);
        assert_eq!(doc, *session.design());
        assert_eq!(solidcraft_io::read_design_thumbnail(&bytes), Some(expected));
        let reopened = Session::new(doc);
        assert_eq!(reopened.visibility, session.visibility);
        let disabled = session.design_container(solidcraft_io::DesignCompression::Fast, false, false).unwrap();
        assert_eq!(solidcraft_io::read_design(&disabled).unwrap().visibility, session.visibility);
        assert!(solidcraft_io::read_design_thumbnail(&disabled).is_none());
    }

    #[test]
    fn assembly_thumbnail_uses_shared_meshes_in_their_world_placements() {
        crate::capture::install_test_renderer();
        let mut session = Session::default();
        let part = session.execute("component.create", &serde_json::json!({"name":"Bracket","grounded":false})).unwrap();
        session.execute("solid.box", &serde_json::json!({"length":10,"width":20,"height":30})).unwrap();
        session
            .execute("occurrence.move", &serde_json::json!({"occurrence":part["occurrence"],"translate":[40,10,5],"axis":[0,0,1],"angle":"30 deg"}))
            .unwrap();
        let world = session.world_state();
        assert!(scene(&session).is_none(), "cold geometry stays cold");
        assert!(world.bodies.iter().all(|b| b.cached_mesh().is_none()));
        let (shared, placement) = world.bodies[0].mesh_in_place();
        let (picture, _) = scene(&session).expect("a warm placed body gets a thumbnail");
        let shown = &picture.meshes[0].mesh;
        for (source, drawn) in shared.positions.iter().zip(&shown.positions) {
            assert!(solidcraft_doc::apply_point(&placement, *source).dist(*drawn) < 1e-9);
        }
        assert_eq!(shown.triangles, shared.triangles);
        assert!(saved(&session).is_some());
        assert!(world.bodies.iter().all(|b| b.cached_mesh().is_none()), "capture does not fill placed mesh caches");
    }

    #[test]
    fn large_warm_meshes_get_a_thumbnail_within_a_bounded_memory_budget() {
        crate::capture::install_test_renderer();
        let mut doc = solidcraft_doc::Document::new("Large mesh preview");
        doc.add_feature(
            solidcraft_doc::FeatureKind::MeshImport {
                file: "public-safe.stl".into(),
                meshes: vec![solidcraft_doc::MeshData {
                    name: "Panel".into(),
                    positions: vec![0., 0., 0., 10., 0., 0., 0., 10., 0.].into(),
                    triangles: [0, 1, 2].repeat(900_001).into(),
                    color: None,
                }],
            },
            None,
        )
        .unwrap();
        let session = Session::new(doc);
        for body in &session.world_state().bodies {
            body.mesh_in_place();
        }
        assert!(scene(&session).is_some(), "100,000 triangles is not a thumbnail format limit");
        assert!(saved(&session).is_some());
    }

    #[test]
    fn portable_thumbnail_never_forces_cold_evaluation_or_meshing() {
        crate::capture::install_test_renderer();
        let mut session = Session::default();
        session.execute("solid.box", &serde_json::json!({"length":10,"width":20,"height":30})).unwrap();
        let world = session.world_state();
        assert!(world.bodies.iter().all(|b| b.cached_mesh().is_none()));
        assert!(saved(&session).is_none());
        assert!(world.bodies.iter().all(|b| b.cached_mesh().is_none()));
        for body in &world.bodies {
            body.mesh();
        }
        let png = saved(&session).unwrap();
        let bytes = session.design_container(solidcraft_io::DesignCompression::Fast, false, true).unwrap();
        assert_eq!(solidcraft_io::read_design_thumbnail(&bytes), Some(png));
        assert_eq!(solidcraft_io::read_design(&bytes).unwrap(), *session.doc);
        let disabled = session.design_container(solidcraft_io::DesignCompression::Fast, false, false).unwrap();
        assert!(solidcraft_io::read_design_thumbnail(&disabled).is_none());
    }

    /// Without a renderer installed (a tool that draws nothing) a save has no
    /// thumbnail and does not fail.
    #[test]
    fn without_a_renderer_a_save_has_no_thumbnail() {
        let mut session = Session::default();
        session.execute("solid.box", &serde_json::json!({"length":10,"width":20,"height":30})).unwrap();
        for body in &session.world_state().bodies {
            body.mesh();
        }
        let (scene, camera) = scene(&session).unwrap();
        assert!(crate::capture::png_with(None, &scene, &camera, 240, 150).is_err());
    }
}
