//! Product extraction shares evaluated placement and appearance resolution, not viewport policy.

use crate::geom::Aabb3;
use crate::render::{FaceAppearance, ProductMesh, ProductScene};
use crate::{EngineError, Result, Session};

pub const MAX_PRODUCT_BODIES: usize = solidcraft_render::job::MAX_CPU_BODIES;

/// Extract authoritative, visible geometry. Does not settle a live gesture implicitly.
/// `render.scene` is the explicit model-reading command for that boundary.
pub fn extract(s: &Session) -> Result<ProductScene> {
    if s.model.is_stale() {
        return Err(EngineError::Other("render scene is stale; finish the modelling gesture before previewing".into()));
    }
    let world = s.world_state();
    let mut scene = ProductScene { meshes: Vec::new(), bounds: Aabb3::EMPTY };
    for body in world.bodies.iter().filter(|b| s.body_visible(&b.name)) {
        if scene.meshes.len() >= MAX_PRODUCT_BODIES {
            return Err(EngineError::Other("render scene exceeds 262144 visible bodies".into()));
        }
        let mesh = body.mesh();
        scene.bounds = scene.bounds.union(&mesh.bounds());
        let to8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let color = s.doc.body_color(body).map(|c| c.map(to8)).unwrap_or([176, 186, 198]);
        let faces = s
            .doc
            .face_colors(body)
            .into_iter()
            .map(|(face, look)| {
                Ok(FaceAppearance {
                    face: u32::try_from(face).map_err(|_| EngineError::Other("render face index exceeds the mesh contract".into()))?,
                    color: look.color,
                    opacity: look.opacity.clamp(0.0, 1.0),
                    surface: look.opaque_surface().map_err(|e| EngineError::Other(e.to_string()))?,
                    transmission: None,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let surface = s.doc.model_body_look(body).map(|l| l.opaque_surface()).transpose().map_err(|e| EngineError::Other(e.to_string()))?.flatten();
        scene.meshes.try_reserve(1).map_err(|_| EngineError::Other("could not reserve captured render bodies".into()))?;
        scene.meshes.push(ProductMesh {
            name: body.name.clone(),
            mesh,
            color,
            opacity: s.doc.model_body_opacity(body).clamp(0.0, 1.0),
            surface,
            transmission: None,
            faces,
        });
    }
    Ok(scene)
}

/// Most triangles a render's product meshes may total once [`refine_for_image`] has meshed them
/// for the picture (#9814), under every image executor's own cap: refinement stops short of it,
/// it never makes a scene the executor would refuse.
pub const IMAGE_TRIANGLE_BUDGET: u64 = 1_000_000;

/// How finely a render's curved faces are meshed for its camera and size (#9814): the reply's
/// `meshes` in `render.preflight` and `render.start`.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct ImageMeshes {
    /// Chord error aimed for, in output pixels.
    pub target_px: f64,
    /// Most triangles the scene's product meshes may total.
    pub triangle_budget: u64,
    /// Triangles of the captured meshes, and of the meshes rendered.
    pub reference_triangles: u64,
    pub triangles: u64,
    /// The bodies whose captured mesh was coarser than the target, most in need first.
    pub bodies: Vec<ImageMesh>,
}

/// A body [`refine_for_image`] looked at.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ImageMesh {
    pub body: String,
    /// Refinement steps taken past the captured mesh (each halves its chord), 0: kept.
    pub steps: u8,
    /// Its chord error in output pixels where it comes nearest the camera, as rendered.
    pub error_px: f64,
    /// Why it stays coarser than the target, if it does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coarser: Option<Coarser>,
}

/// Why a body is rendered coarser than [`ImageMeshes::target_px`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Coarser {
    /// Finer steps would take the scene past its triangle budget.
    TriangleBudget,
    /// It needs more than [`MAX_REFINE`](solidcraft_doc::display_quality::MAX_REFINE) steps.
    MostSteps,
    /// The display planner made no plan finer than the captured mesh.
    NoPlan,
}

/// Mesh the product bodies of `scene` (its first meshes, as [`extract`] captured them; anything
/// after them, a stage, is left alone) finely enough for a picture of `camera`, `width` ×
/// `height` pixels (#9814). The captured mesh's chord is about a thousandth of the body's size,
/// which shows as straight segments along a large round silhouette in a large picture. Each body
/// with a curved face that comes into view is meshed again with
/// [`ModelBody::refined_display`](solidcraft_doc::ModelBody::refined_display), the viewport's
/// view-dependent refinement, at the fewest steps that bring its chord under
/// [`IMAGE_TARGET_PX`](solidcraft_doc::display_quality::IMAGE_TARGET_PX) where it comes nearest
/// the camera, the bodies that need it most first, while the scene stays within
/// `max_triangles`. A body that cannot be meshed finer keeps the mesh it has. The refined mesh is
/// the same body's display plan: its faces keep their indices, so face appearances and bindings
/// still match. The design, the preview and picking keep the captured meshes.
pub fn refine_for_image(
    s: &Session,
    scene: &mut ProductScene,
    camera: &crate::render::Camera,
    width: u32,
    height: u32,
    max_triangles: u64,
) -> ImageMeshes {
    use solidcraft_doc::display_quality::{DisplayQuality, IMAGE_TARGET_PX, Lens, MAX_REFINE, steps_to};
    let triangles = |scene: &ProductScene| scene.meshes.iter().map(|m| m.mesh.triangles.len() as u64).sum::<u64>();
    let reference_triangles = triangles(scene);
    let mut report = ImageMeshes {
        target_px: IMAGE_TARGET_PX,
        triangle_budget: max_triangles,
        reference_triangles,
        triangles: reference_triangles,
        bodies: Vec::new(),
    };
    if !camera.is_valid() || width == 0 || height == 0 {
        return report;
    }
    // The level `ModelBody::mesh` is made at: its chord and angle.
    let q = DisplayQuality::Normal;
    let lens = Lens::new(camera, f64::from(height));
    let aspect = f64::from(width) / f64::from(height);
    let world = s.world_state();
    let mut needs = Vec::new();
    for (index, (product, body)) in scene.meshes.iter().zip(world.bodies.iter().filter(|b| s.body_visible(&b.name))).enumerate() {
        if product.name != body.name || body.body.is_mesh() || !curved(&product.mesh) {
            continue;
        }
        let bounds = product.mesh.bounds();
        if !in_view(camera, &bounds, aspect) {
            continue;
        }
        let err = q.chord(body.body.size()) * lens.px_per_mm(&bounds);
        let steps = steps_to(err, IMAGE_TARGET_PX, MAX_REFINE);
        if steps > 0 {
            needs.push((index, body, err, steps));
        }
    }
    needs.sort_by(|a, b| b.2.total_cmp(&a.2));
    // Triangles refinement may still add.
    let mut spare = max_triangles.saturating_sub(reference_triangles);
    for (index, body, err, wanted) in needs {
        let Some(product) = scene.meshes.get_mut(index) else { continue };
        let base = product.mesh.triangles.len() as u64;
        // The viewport's estimate: a step at most doubles a body's triangles. Start at the
        // finest step it allows, so a body far over the budget is not planned for nothing.
        let added = |k: u8| base.saturating_mul(1 << k).saturating_sub(base);
        let mut k = wanted;
        while k > 0 && added(k) > spare {
            k -= 1;
        }
        let mut coarser = if k < wanted {
            Some(Coarser::TriangleBudget)
        } else {
            (err / f64::from(1u32 << wanted) > IMAGE_TARGET_PX).then_some(Coarser::MostSteps)
        };
        let mut steps = 0;
        while k > 0 {
            match body.refined_display(q, k) {
                Some((_, mesh)) if (mesh.triangles.len() as u64).saturating_sub(base) <= spare => {
                    spare -= (mesh.triangles.len() as u64).saturating_sub(base);
                    product.mesh = mesh;
                    steps = k;
                    break;
                }
                Some(_) => coarser = Some(Coarser::TriangleBudget),
                None => coarser = Some(Coarser::NoPlan),
            }
            k -= 1;
        }
        if let Some(why) = coarser {
            log::info!("render: {} meshed {steps} of the {wanted} steps its picture needs ({why:?})", body.name);
        }
        report.bodies.push(ImageMesh { body: body.name.clone(), steps, error_px: err / f64::from(1u32 << steps), coarser });
    }
    if report.bodies.iter().any(|b| b.steps > 0) {
        report.triangles = triangles(scene);
        scene.bounds = scene.meshes.iter().fold(Aabb3::EMPTY, |b, m| b.union(&m.mesh.bounds()));
    }
    report
}

/// Whether a mesh has a curved face: a triangle whose corners carry different normals (every
/// corner of a flat face carries the face's normal). A body without one is meshed exactly.
fn curved(mesh: &crate::geom::Mesh) -> bool {
    mesh.triangles.iter().any(|t| match t.map(|i| mesh.normals.get(i as usize)) {
        [Some(a), Some(b), Some(c)] => a.dist(*b) > 1e-9 || a.dist(*c) > 1e-9,
        _ => false,
    })
}

/// Whether any of a box can show in a picture of `camera` `aspect` wide to 1 high: false only
/// for a box wholly behind the eye or wholly past one side of the picture.
fn in_view(camera: &crate::render::Camera, b: &Aabb3, aspect: f64) -> bool {
    use crate::geom::Vec3;
    if b.is_empty() || !b.min.is_finite() || !b.max.is_finite() {
        return false;
    }
    let (right, up, back) = camera.basis();
    let corners = [0, 1, 2, 3, 4, 5, 6, 7].map(|i| {
        Vec3::new(if i & 1 == 0 { b.min.x } else { b.max.x }, if i & 2 == 0 { b.min.y } else { b.max.y }, if i & 4 == 0 { b.min.z } else { b.max.z })
    });
    // Each corner across the picture, up it and in front of the eye: depth along the view
    // (perspective), or 1 for a parallel view, which shows the same width at every depth.
    let (origin, half_h) = if camera.fov > 0.0 { (camera.eye(), (camera.fov * 0.5).tan()) } else { (camera.target, camera.half_height()) };
    let view = corners.map(|p| {
        let d = p - origin;
        (d.dot(right), d.dot(up), if camera.fov > 0.0 { -d.dot(back) } else { 1.0 })
    });
    let half_w = half_h * aspect;
    let all = |outside: &dyn Fn(f64, f64, f64) -> bool| view.iter().all(|&(x, y, z)| outside(x, y, z));
    !(all(&|_, _, z| z <= 0.0)
        || all(&|x, _, z| x > half_w * z)
        || all(&|x, _, z| x < -half_w * z)
        || all(&|_, y, z| y > half_h * z)
        || all(&|_, y, z| y < -half_h * z))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Sel;
    use crate::geom::Vec3;
    use crate::render::*;
    use serde_json::json;
    use solidcraft_doc::display_quality::IMAGE_TARGET_PX;
    use std::sync::Arc;

    #[test]
    fn product_scene_keeps_placed_appearances_and_meshes_without_design_overlays() {
        let mut s = Session::default();
        let component = s.execute("component.create", &json!({"name":"Part","grounded":false})).unwrap();
        s.execute("solid.box", &json!({"length":10,"width":10,"height":10,"body_name":"Block"})).unwrap();
        s.execute("appearance.assign", &json!({"bodies":["Block"],"color":"#20a080","opacity":0.4})).unwrap();
        s.execute("appearance.assign", &json!({"faces":[[5,5,10]],"color":"#ff0000","opacity":0.7})).unwrap();
        s.execute("component.activate", &json!({"component":0})).unwrap();
        s.execute("occurrence.copy", &json!({"component":component["component"],"translate":[30,0,0]})).unwrap();
        s.execute("occurrence.move", &json!({"occurrence":component["occurrence"],"translate":[10,0,0]})).unwrap();
        let doc = s.doc.clone();
        s.evaluations = 0;
        let first = extract(&s).unwrap();
        assert_eq!(first.meshes.len(), 2);
        assert_eq!(first.bounds.min, Vec3::new(10.0, 0.0, 0.0));
        assert_eq!(first.bounds.max, Vec3::new(40.0, 10.0, 10.0));
        for m in &first.meshes {
            assert_eq!(m.color, [32, 160, 128]);
            assert_eq!(m.opacity, 0.4);
            assert!(m.faces.iter().any(|f| f.color == [255, 0, 0] && f.opacity == 0.7));
            assert!(Arc::ptr_eq(&m.mesh, &s.world_state().body(&m.name).unwrap().mesh()));
        }
        s.selection = vec![Sel::Body { name: first.meshes[0].name.clone() }];
        s.section = Some((Vec3::ZERO, Vec3::X));
        s.analysis = Some(crate::SurfaceAnalysis::Zebra { stripes: 10.0 });
        let after = extract(&s).unwrap();
        for (a, b) in first.meshes.iter().zip(&after.meshes) {
            assert!(Arc::ptr_eq(&a.mesh, &b.mesh));
            assert_eq!((a.color, a.opacity, &a.faces), (b.color, b.opacity, &b.faces));
        }
        s.active_component = component["component"].as_u64().unwrap();
        let active = extract(&s).unwrap();
        assert_eq!(active.bounds.min, first.bounds.min);
        assert_eq!(active.meshes[1].opacity, 0.4, "no active-component fading");
        s.visibility.hidden_bodies.push(first.meshes[1].name.clone());
        assert_eq!(extract(&s).unwrap().meshes.len(), 1);
        assert!(Arc::ptr_eq(&doc, &s.doc));
        assert_eq!(s.evaluations, 0);
    }

    #[test]
    fn stale_geometry_requires_an_explicit_model_read_and_camera_apply_preserves_staleness() {
        let mut s = Session::default();
        for (id, p) in [
            ("sketch.create", json!({"plane":"XY"})),
            ("sketch.rectangle.two_point", json!({"p0":[5,5],"p1":[25,20]})),
            ("sketch.dimension", json!({"entities":["l1"],"value":20})),
            ("sketch.dimension", json!({"entities":["l2"],"value":15})),
            ("sketch.finish", json!({})),
            ("solid.extrude", json!({"distance":5})),
            ("sketch.edit", json!({"sketch":"Sketch1"})),
            ("sketch.drag", json!({"entity":"l1.start","from":[5,5],"to":[4,5],"live":true})),
        ] {
            s.execute(id, &p).unwrap();
        }
        assert!(s.model.is_stale());
        assert!(extract(&s).is_err());
        s.evaluations = 0;
        let camera = Camera { up: UpAxis::Y, roll: 0.25, ..Default::default() };
        s.execute("render.settings.set", &json!({"name":"Photo","set":{"camera":camera}})).unwrap();
        let doc = s.doc.clone();
        let undo = s.undo.len();
        s.execute("render.settings.apply", &json!({"name":"Photo"})).unwrap();
        assert_eq!(s.camera, Some(camera));
        assert!(s.model.is_stale());
        assert_eq!(s.evaluations, 0);
        assert!(Arc::ptr_eq(&doc, &s.doc));
        assert_eq!(s.undo.len(), undo);
        let metadata = s.execute("render.scene", &json!({})).unwrap();
        assert!(!s.model.is_stale());
        assert!(s.evaluations > 0);
        assert_eq!(metadata["overlays"], false);
        assert_eq!(metadata["bodies"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn host_capabilities_are_validated_runtime_only_and_survive_new_design() {
        let mut s = Session::default();
        let viewport = ExecutorDescriptor {
            id: ExecutorId::new("gpu-viewport").unwrap(),
            availability: Availability::Available,
            profiles: vec![RenderProfile {
                mode: RenderMode::Preview,
                operation: RenderOperation::Viewport,
                formats: Vec::new(),
                features: Vec::new(),
            }],
            limits: RenderLimits::default(),
            scratch: ScratchLayout { sample_scale: 1, sample_bytes_per_pixel: 8, output_bytes_per_pixel: 8, encoding_bytes_per_pixel: 0 },
        };
        let doc = s.doc.clone();
        s.set_render_viewport(Some(viewport.clone())).unwrap();
        assert!(Arc::ptr_eq(&doc, &s.doc));
        assert_eq!(s.evaluations, 0);
        assert!(!s.is_dirty());
        let backend = s.render_backend();
        s.replace_design(Session::default());
        assert_eq!(s.render_viewport(), Some(&viewport));
        assert_eq!(s.render_backend(), backend);
        assert_eq!(backend.executors.len(), 2);
        assert_eq!(s.scratch().render_backend(), backend);
        let mut invalid = viewport;
        invalid.id = ExecutorId::new("cpu-raster").unwrap();
        assert!(s.set_render_viewport(Some(invalid)).is_err());
        assert_eq!(s.render_backend(), backend);
        s.execute("file.new", &json!({})).unwrap();
        assert_eq!(s.render_backend(), backend);
        assert!(!s.doc.to_json().contains("gpu-viewport"));
        s.set_render_viewport(None).unwrap();
        assert_eq!(s.render_backend(), builtin_backend());
    }

    #[test]
    fn imported_looks_survive_product_extraction_and_design_overrides() {
        let body = crate::kernel::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0))
            .unwrap()
            .with_color(Some([0.2, 0.6, 0.8]))
            .with_paint(Some(crate::kernel::Paint { opacity: 0.4, faces: Vec::new() }));
        let step = crate::kernel::step_export_bodies(
            &[crate::kernel::ExportBody { name: "Imported glass".into(), body: &body, color: body.color() }],
            &Default::default(),
        )
        .unwrap();
        let mut document = crate::doc::Document::default();
        document
            .add_feature(
                crate::doc::FeatureKind::Import {
                    file: "original-generated.step".into(),
                    step: step.into(),
                    components: Vec::new(),
                    skip: Vec::new(),
                },
                None,
            )
            .unwrap();
        let mut s = Session::new(document);
        let first = extract(&s).unwrap();
        assert_eq!(first.meshes[0].color, [51, 153, 204]);
        assert!((first.meshes[0].opacity - 0.4).abs() < 1e-6);
        let name = first.meshes[0].name.clone();
        s.execute("appearance.assign", &json!({"bodies":[name],"color":"#336699","opacity":0.7})).unwrap();
        let changed = extract(&s).unwrap();
        assert_eq!(changed.meshes[0].color, [51, 102, 153]);
        assert_eq!(changed.meshes[0].opacity, 0.7);
        assert!(Arc::ptr_eq(&first.meshes[0].mesh, &changed.meshes[0].mesh));
    }

    /// #9814's base: a disc 240 mm across and 40 mm high, its top face red.
    fn disc() -> Session {
        let mut s = Session::default();
        s.execute("solid.cylinder", &json!({"radius":120,"height":40,"body_name":"Base"})).unwrap();
        s.execute("appearance.assign", &json!({"faces":[[0,0,40]],"color":"#ff0000"})).unwrap();
        s
    }

    /// A three-quarter view with the disc about a third of a 16:9 picture's width.
    fn view() -> Camera {
        Camera {
            target: Vec3::new(0.0, 0.0, 20.0),
            yaw: 25f64.to_radians(),
            pitch: 28f64.to_radians(),
            distance: 800.0,
            fov: 0.5,
            ..Default::default()
        }
    }

    /// The worst sag (mm) of the disc's rim at height `z`: from the widest gap between its points
    /// on the circle, the chord a silhouette shows.
    fn rim_sag(mesh: &crate::geom::Mesh, z: f64) -> f64 {
        let mut angles: Vec<f64> =
            mesh.positions.iter().filter(|p| (p.x.hypot(p.y) - 120.0).abs() < 1e-6 && (p.z - z).abs() < 1e-9).map(|p| p.y.atan2(p.x)).collect();
        angles.sort_by(f64::total_cmp);
        angles.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
        assert!(angles.len() > 8, "a rim at {z}");
        let wrap = angles[0] + std::f64::consts::TAU - angles[angles.len() - 1];
        let gap = angles.windows(2).map(|w| w[1] - w[0]).fold(wrap, f64::max);
        120.0 * (1.0 - (gap / 2.0).cos())
    }

    /// The area of each face's triangles, by face index.
    fn face_areas(mesh: &crate::geom::Mesh) -> std::collections::BTreeMap<u32, f64> {
        let mut areas = std::collections::BTreeMap::new();
        for (t, face) in mesh.triangles.iter().zip(&mesh.tri_face) {
            let [a, b, c] = t.map(|i| mesh.positions[i as usize]);
            *areas.entry(*face).or_insert(0.0) += (b - a).cross(c - a).len() * 0.5;
        }
        areas
    }

    /// One closed, outward shell once points 1e-6 mm apart are welded: what the path tracer
    /// certifies before it renders a body as `closed_mesh` glass.
    fn closed_and_outward(mesh: &crate::geom::Mesh) -> bool {
        let mut ids = std::collections::HashMap::new();
        let mut edges = std::collections::HashMap::new();
        let mut six_volume = 0.0;
        for t in &mesh.triangles {
            let p = t.map(|i| mesh.positions[i as usize]);
            let k = p.map(|p| {
                let n = ids.len();
                *ids.entry([p.x, p.y, p.z].map(|v| (v * 1e6).round() as i64)).or_insert(n)
            });
            for (a, b) in [(0, 1), (1, 2), (2, 0)] {
                if k[a] == k[b] {
                    return false;
                }
                *edges.entry((k[a], k[b])).or_insert(0) += 1;
            }
            six_volume += p[0].dot(p[1].cross(p[2]));
        }
        six_volume > 0.0 && edges.iter().all(|(&(a, b), &n)| n == 1 && edges.get(&(b, a)) == Some(&1))
    }

    /// #9814: in a 3840 × 2160 picture the captured mesh shows its facets along the base's
    /// rims; the image meshes it finer, under half a pixel, and a small picture keeps it. The
    /// faces keep their indices (the red top is still the top), the mesh stays closed, and the
    /// model's own mesh does not change.
    #[test]
    fn a_large_picture_meshes_a_round_body_finer_and_keeps_its_faces() {
        let s = disc();
        let captured = extract(&s).unwrap();
        let mesh = captured.meshes[0].mesh.clone();
        let mut small = captured.clone();
        let report = refine_for_image(&s, &mut small, &view(), 640, 360, IMAGE_TRIANGLE_BUDGET);
        assert!(report.bodies.is_empty(), "{report:?}");
        assert!(Arc::ptr_eq(&small.meshes[0].mesh, &mesh));
        assert_eq!(report.triangles, mesh.triangles.len() as u64);

        let mut large = captured.clone();
        let report = refine_for_image(&s, &mut large, &view(), 3840, 2160, IMAGE_TRIANGLE_BUDGET);
        let refined = large.meshes[0].mesh.clone();
        let [body] = report.bodies.as_slice() else { panic!("{report:?}") };
        assert!(body.steps >= 2 && body.coarser.is_none() && body.error_px <= IMAGE_TARGET_PX, "{report:?}");
        assert!(refined.triangles.len() > mesh.triangles.len());
        assert_eq!((report.reference_triangles, report.triangles), (mesh.triangles.len() as u64, refined.triangles.len() as u64));
        let px = solidcraft_doc::display_quality::Lens::new(&view(), 2160.0).px_per_mm(&mesh.bounds());
        for z in [0.0, 40.0] {
            let (before, after) = (rim_sag(&mesh, z) * px, rim_sag(&refined, z) * px);
            assert!(before > 1.0 && after <= IMAGE_TARGET_PX, "rim at {z}: {before} px, then {after} px");
        }
        assert!(large.bounds.min.dist(refined.bounds().min) < 1e-12 && large.bounds.max.dist(refined.bounds().max) < 1e-12);

        let (before, after) = (face_areas(&mesh), face_areas(&refined));
        assert_eq!(before.keys().collect::<Vec<_>>(), after.keys().collect::<Vec<_>>());
        for (face, area) in &before {
            assert!((after[face] - area).abs() <= area * 0.01, "face {face}: {area} then {}", after[face]);
        }
        let red = large.meshes[0].faces.iter().find(|f| f.color == [255, 0, 0]).unwrap().face;
        assert_eq!(large.meshes[0].faces, captured.meshes[0].faces);
        for m in [&mesh, &refined] {
            let top: Vec<Vec3> =
                m.triangles.iter().zip(&m.tri_face).filter(|(_, f)| **f == red).flat_map(|(t, _)| t.map(|i| m.positions[i as usize])).collect();
            assert!(!top.is_empty() && top.iter().all(|p| (p.z - 40.0).abs() < 1e-9), "the red face is the top");
        }
        assert!(closed_and_outward(&refined));
        assert!(Arc::ptr_eq(&s.world_state().body("Base").unwrap().mesh(), &mesh));
    }

    /// Copies placed in an assembly are meshed finer where they stand, face by face.
    #[test]
    fn placed_copies_are_refined_where_they_stand() {
        let mut s = Session::default();
        let component = s.execute("component.create", &json!({"name":"Part","grounded":false})).unwrap();
        s.execute("solid.cylinder", &json!({"radius":120,"height":40,"body_name":"Base"})).unwrap();
        s.execute("component.activate", &json!({"component":0})).unwrap();
        s.execute("occurrence.copy", &json!({"component":component["component"],"translate":[300,0,0]})).unwrap();
        let mut scene = extract(&s).unwrap();
        assert_eq!(scene.meshes.len(), 2);
        let captured = scene.clone();
        let cam = Camera { target: Vec3::new(150.0, 0.0, 20.0), distance: 1100.0, ..view() };
        let report = refine_for_image(&s, &mut scene, &cam, 3840, 2160, IMAGE_TRIANGLE_BUDGET);
        assert_eq!(report.bodies.iter().filter(|b| b.steps > 0).count(), 2, "{report:?}");
        let centres = |m: &crate::geom::Mesh| {
            let mut sums = std::collections::BTreeMap::new();
            for (t, face) in m.triangles.iter().zip(&m.tri_face) {
                let [a, b, c] = t.map(|i| m.positions[i as usize]);
                let w = (b - a).cross(c - a).len();
                let e = sums.entry(*face).or_insert((Vec3::ZERO, 0.0));
                *e = (e.0 + (a + b + c) * (w / 3.0), e.1 + w);
            }
            sums.into_iter().map(|(face, (sum, w))| (face, sum * (1.0 / w))).collect::<Vec<_>>()
        };
        for (before, after) in captured.meshes.iter().zip(&scene.meshes) {
            assert!(after.mesh.triangles.len() > before.mesh.triangles.len(), "{}", before.name);
            let (a, b) = (centres(&before.mesh), centres(&after.mesh));
            assert_eq!(a.len(), b.len());
            for ((fa, ca), (fb, cb)) in a.iter().zip(&b) {
                assert!(fa == fb && ca.dist(*cb) < 0.5, "{}: face {fa} at {ca:?}, then face {fb} at {cb:?}", before.name);
            }
        }
    }

    /// The budget goes to the body that needs it most and the scene never passes it; with no
    /// room left the captured meshes stay and the report says why.
    #[test]
    fn the_budget_goes_to_the_largest_error_and_names_what_stays_coarser() {
        let mut s = disc();
        // The same disc behind it, further from the camera.
        s.execute("solid.cylinder", &json!({"base":[254,544,0],"radius":120,"height":40,"body_name":"Far"})).unwrap();
        let cam = Camera { target: Vec3::new(127.0, 272.0, 20.0), distance: 1000.0, ..view() };
        let captured = extract(&s).unwrap();
        let total = captured.meshes.iter().map(|m| m.mesh.triangles.len() as u64).sum::<u64>();
        let mut full = captured.clone();
        let unlimited = refine_for_image(&s, &mut full, &cam, 3840, 2160, IMAGE_TRIANGLE_BUDGET);
        assert_eq!(unlimited.bodies.len(), 2, "{unlimited:?}");
        assert!(unlimited.bodies.iter().all(|b| b.steps > 0 && b.coarser.is_none()), "{unlimited:?}");
        let near = &unlimited.bodies[0];
        let index = captured.meshes.iter().position(|m| m.name == near.body).unwrap();
        let added = (full.meshes[index].mesh.triangles.len() - captured.meshes[index].mesh.triangles.len()) as u64;

        let mut none = captured.clone();
        let report = refine_for_image(&s, &mut none, &cam, 3840, 2160, total);
        assert_eq!(report.triangles, total);
        assert!(report.bodies.iter().all(|b| b.steps == 0 && b.coarser == Some(Coarser::TriangleBudget)), "{report:?}");
        assert!(none.meshes.iter().zip(&captured.meshes).all(|(a, b)| Arc::ptr_eq(&a.mesh, &b.mesh)));

        let mut some = captured.clone();
        let report = refine_for_image(&s, &mut some, &cam, 3840, 2160, total + added);
        assert!(report.triangles <= total + added, "{report:?}");
        assert_eq!(report.bodies[0].body, near.body, "the largest error first");
        assert!(report.bodies[0].steps > 0, "{report:?}");
        for b in &report.bodies {
            let wanted = unlimited.bodies.iter().find(|u| u.body == b.body).unwrap().steps;
            assert_eq!(b.coarser.is_some(), b.steps < wanted, "{b:?}");
        }
    }

    /// Bodies with only flat faces are meshed exactly already, and bodies outside the picture
    /// are not seen: both keep their captured meshes.
    #[test]
    fn flat_bodies_and_bodies_outside_the_picture_keep_their_mesh() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"corner":[-120,-120,0],"length":240,"width":240,"height":40,"body_name":"Slab"})).unwrap();
        s.execute("solid.cylinder", &json!({"base":[5000,0,0],"radius":120,"height":40,"body_name":"Aside"})).unwrap();
        let captured = extract(&s).unwrap();
        let mut scene = captured.clone();
        let report = refine_for_image(&s, &mut scene, &view(), 3840, 2160, IMAGE_TRIANGLE_BUDGET);
        assert!(report.bodies.is_empty(), "{report:?}");
        assert!(scene.meshes.iter().zip(&captured.meshes).all(|(a, b)| Arc::ptr_eq(&a.mesh, &b.mesh)));
        // Turned toward it, the round one is refined.
        let mut scene = captured.clone();
        let report = refine_for_image(&s, &mut scene, &Camera { target: Vec3::new(5000.0, 0.0, 20.0), ..view() }, 3840, 2160, IMAGE_TRIANGLE_BUDGET);
        assert_eq!(report.bodies.iter().map(|b| (b.body.as_str(), b.steps > 0)).collect::<Vec<_>>(), [("Aside", true)]);
    }
}
