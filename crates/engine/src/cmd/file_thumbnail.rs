//! A portable picture from warm display meshes only; never evaluate on save.
use crate::Session;
use solidcraft_geom::Aabb3;
use solidcraft_render::{Camera, Rgb, Scene, SceneMesh, StandardView};

pub(super) fn saved(s: &Session) -> Option<Vec<u8>> {
    let (scene, camera) = scene(s)?;
    let [width, height] = solidcraft_io::THUMBNAIL_SIZE;
    // Drawn by the renderer the host installed (the GPU); none installed, no thumbnail.
    let renderer = crate::capture::installed()?;
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
    let mut triangles = 0usize;
    let mut points = 0usize;
    for body in world.bodies.iter().filter(|b| s.body_visible(&b.name)) {
        let mesh = body.cached_mesh()?;
        triangles = triangles.saturating_add(mesh.triangles.len());
        points = points.saturating_add(mesh.positions.len());
        if triangles > 100_000 || points > 100_000 {
            return None;
        }
        bounds = bounds.union(&mesh.bounds());
        let channel = |x: f32| (x.clamp(0., 1.) * 255.).round() as u8;
        let color = s.doc.body_color(body).map_or(crate::view::colors::BODY, |[r, g, b]| Rgb(channel(r), channel(g), channel(b)));
        let face_colors =
            s.doc.face_colors(body).into_iter().map(|(face, look)| (face as u32, Rgb(look.color[0], look.color[1], look.color[2]))).collect();
        let opacity = s.doc.model_body_opacity(body) as f32;
        scene.meshes.push(SceneMesh { mesh, color, face_colors, opacity, closed: body.is_closed(), hatch: Default::default() });
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
        let world = session.world_state().unwrap();
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
    fn portable_thumbnail_never_forces_cold_evaluation_or_meshing() {
        crate::capture::install_test_renderer();
        let mut session = Session::default();
        session.execute("solid.box", &serde_json::json!({"length":10,"width":20,"height":30})).unwrap();
        let world = session.world_state().unwrap();
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

    /// Without a renderer installed (a browser, a tool that draws nothing) a save has no
    /// thumbnail and does not fail.
    #[test]
    fn without_a_renderer_a_save_has_no_thumbnail() {
        let mut session = Session::default();
        session.execute("solid.box", &serde_json::json!({"length":10,"width":20,"height":30})).unwrap();
        for body in &session.world_state().unwrap().bodies {
            body.mesh();
        }
        let (scene, camera) = scene(&session).unwrap();
        assert!(crate::capture::png_with(None, &scene, &camera, 240, 150).is_err());
    }
}
