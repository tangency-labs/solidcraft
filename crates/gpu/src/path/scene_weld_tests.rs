use solidcraft_geom::prelude::*;
use std::sync::Arc;

use super::{MediumWeld, Triangle, flatten, medium_preparation_bytes};
use solidcore_record::Vec3;
use solidcraft_geom::Mesh;
use solidcraft_render::{ProductMesh, ProductScene, job::CancelToken, transmission::Transmission};
use solidcraft_viewport::surface::OpaqueSurface;

fn seam_tetra(offset: Vec3) -> ProductScene {
    // Original outward tetrahedron. Only one face has a duplicated CAD seam
    // endpoint; its measured displacement comes from our revolved tumbler.
    let origin = Vec3::new(-81.00000000023796, -51.6269330409162, 592.0);
    let mesh = Mesh {
        positions: vec![origin, origin + Vec3::X, origin + Vec3::Y, origin + Vec3::Z, origin + offset],
        normals: vec![Vec3::Z; 5],
        triangles: vec![[4, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
        tri_face: vec![0; 4],
        ..Default::default()
    };
    let bounds = mesh.bounds();
    ProductScene {
        meshes: vec![ProductMesh {
            name: "original seam tetrahedron".into(),
            mesh: Arc::new(mesh),
            color: [255; 3],
            opacity: 1.0,
            surface: Some(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 1.5).unwrap()),
            transmission: Some(Transmission::new(Vec3::ZERO, None).unwrap()),
            faces: Vec::new(),
        }],
        bounds,
    }
}

#[test]
fn closed_cad_seam_across_rounding_cells_keeps_one_nanometre_tolerance() {
    let delta = Vec3::new(-0.00000054155099, 0.00000031266462, 0.0);
    assert!(delta.len() < 1e-6);
    let scene = seam_tetra(delta);
    assert!(flatten(&scene, Vec3::ZERO, &CancelToken::default()).is_ok());
    let open = seam_tetra(Vec3::new(1.01e-6, 0.0, 0.0));
    assert!(flatten(&open, Vec3::ZERO, &CancelToken::default()).is_err());
}

#[test]
fn spatial_weld_is_deterministic_without_transitive_or_diagonal_expansion() {
    for _ in 0..8 {
        let mut weld = MediumWeld::new(8).unwrap();
        let a = weld.key(Vec3::ZERO).unwrap();
        assert_eq!(a, weld.key(Vec3::new(0.75e-6, 0.0, 0.0)).unwrap());
        assert_ne!(a, weld.key(Vec3::new(1.5e-6, 0.0, 0.0)).unwrap());
        assert_ne!(a, weld.key(Vec3::new(0.9e-6, 0.9e-6, 0.9e-6)).unwrap());
        assert!(weld.key(Vec3::new(f64::NAN, 0.0, 0.0)).is_err());
    }
}

#[test]
fn spatial_weld_preserves_winding_and_connected_component_checks() {
    let mut inward = seam_tetra(Vec3::ZERO);
    for tri in &mut Arc::make_mut(&mut inward.meshes[0].mesh).triangles {
        tri.swap(1, 2);
    }
    assert!(flatten(&inward, Vec3::ZERO, &CancelToken::default()).is_err());

    let mut disconnected = seam_tetra(Vec3::ZERO);
    let mesh = Arc::make_mut(&mut disconnected.meshes[0].mesh);
    let second = mesh.clone();
    mesh.positions.extend(second.positions.iter().map(|p| *p + Vec3::new(4.0, 0.0, 0.0)));
    mesh.normals.extend(&second.normals);
    mesh.triangles.extend(second.triangles.iter().map(|t| t.map(|i| i + 5)));
    mesh.tri_face.extend(&second.tri_face);
    let error = flatten(&disconnected, Vec3::ZERO, &CancelToken::default()).err().unwrap();
    assert!(error.to_string().contains("one connected shell"), "{error}");
}

#[test]
fn weld_scratch_only_counts_the_largest_closed_medium() {
    let glass = seam_tetra(Vec3::ZERO);
    let triangle_bytes = 4 * std::mem::size_of::<Triangle>() as u64;
    let mut opaque = glass.clone();
    opaque.meshes[0].transmission = None;
    assert_eq!(medium_preparation_bytes(&opaque), triangle_bytes);
    assert!(medium_preparation_bytes(&opaque) <= 4 * 400);
    opaque.meshes[0].transmission = Some(Transmission::new(Vec3::ZERO, Some(1.0)).unwrap());
    assert_eq!(medium_preparation_bytes(&opaque), triangle_bytes);
    let mut two = glass.clone();
    two.meshes.push(glass.meshes[0].clone());
    assert_eq!(medium_preparation_bytes(&two), medium_preparation_bytes(&glass) + triangle_bytes);
}

#[test]
fn optical_upload_refuses_collapsed_and_reversed_faces_but_preserves_rebased_geometry() {
    let origin = Vec3::new(1_000_000.0, 0.0, 0.0);
    let mut source = seam_tetra(Vec3::ZERO);
    let mesh = Arc::make_mut(&mut source.meshes[0].mesh);
    mesh.positions = vec![origin, origin + Vec3::X * 0.01, origin + Vec3::Y * 0.01, origin + Vec3::Z * 0.01, origin];
    source.bounds = source.meshes[0].mesh.bounds();
    let cancel = CancelToken::default();
    let error = flatten(&source, Vec3::ZERO, &cancel).err().unwrap();
    assert!(error.message.contains("GPU position precision"), "{error}");
    assert!(flatten(&source, origin, &cancel).is_ok(), "camera-relative representable tetrahedron must retain its closed boundary");

    let mesh = Arc::make_mut(&mut source.meshes[0].mesh);
    mesh.positions =
        vec![Vec3::new(1_000_000.36, 1_000_000.29, 0.0), Vec3::new(1_000_000.33, 1_000_000.10, 0.0), Vec3::new(1_000_000.44, 1_000_000.56, 0.0)];
    mesh.normals = vec![Vec3::Z; 3];
    mesh.triangles = vec![[0, 1, 2]];
    mesh.tri_face = vec![0];
    source.meshes[0].transmission = Some(Transmission::new(Vec3::ZERO, Some(0.0)).unwrap());
    source.bounds = source.meshes[0].mesh.bounds();
    let error = flatten(&source, Vec3::ZERO, &cancel).err().unwrap();
    assert!(error.message.contains("reverses orientation"), "f64-positive area becomes f32-negative area: {error}");
    assert!(flatten(&source, Vec3::new(1_000_000.0, 1_000_000.0, 0.0), &cancel).is_ok());
}

#[test]
fn degenerate_optical_boundaries_refuse_without_erasing_redundant_pole_compatibility() {
    let cancel = CancelToken::default();
    let mut source = seam_tetra(Vec3::ZERO);
    // A redundant pole face does not alter the certified remaining shell.
    let mesh = Arc::make_mut(&mut source.meshes[0].mesh);
    mesh.triangles.push([0, 0, 1]);
    mesh.tri_face.push(0);
    assert!(flatten(&source, Vec3::ZERO, &cancel).is_ok());
    source.meshes[0].transmission = Some(Transmission::new(Vec3::ZERO, Some(0.5)).unwrap());
    assert!(flatten(&source, Vec3::ZERO, &cancel).is_ok(), "redundant source pole faces retain thin-sheet compatibility too");
    let mesh = Arc::make_mut(&mut source.meshes[0].mesh);
    mesh.triangles = vec![[0, 0, 1]];
    mesh.tri_face = vec![0];
    for thickness in [None, Some(0.5)] {
        source.meshes[0].transmission = Some(Transmission::new(Vec3::ZERO, thickness).unwrap());
        let error = flatten(&source, Vec3::ZERO, &cancel).err().unwrap();
        assert!(error.message.contains("no nondegenerate optical triangles"), "{error}");
    }
    source.meshes[0].transmission = None;
    assert!(flatten(&source, Vec3::ZERO, &cancel).is_ok(), "opaque degenerate skipping is unchanged");
}

#[test]
fn closed_glass_refuses_a_pinched_vertex_even_when_edges_and_global_connectivity_pass() {
    let mut source = seam_tetra(Vec3::ZERO);
    let mesh = Arc::make_mut(&mut source.meshes[0].mesh);
    mesh.positions = (0..8)
        .map(|i| Vec3::new(if i & 1 == 0 { -10.0 } else { 10.0 }, if i & 2 == 0 { -10.0 } else { 10.0 }, if i & 4 == 0 { 0.0 } else { 2.0 }))
        .collect();
    mesh.triangles.clear();
    for face in [[0, 2, 3, 1], [4, 5, 7, 6], [0, 4, 6, 2], [1, 3, 7, 5], [0, 1, 5, 4], [2, 6, 7, 3]] {
        let center = face.map(|i| mesh.positions[i]).into_iter().fold(Vec3::ZERO, |a, b| a + b) * 0.25;
        let index = mesh.positions.len() as u32;
        mesh.positions.push(center);
        for corner in 0..4 {
            mesh.triangles.push([index, face[corner] as u32, face[(corner + 1) % 4] as u32]);
        }
    }
    mesh.normals = vec![Vec3::Z; mesh.positions.len()];
    mesh.tri_face = vec![0; mesh.triangles.len()];
    source.bounds = mesh.bounds();
    let cancel = CancelToken::default();
    assert!(flatten(&source, Vec3::ZERO, &cancel).is_ok());
    let pinch = source.meshes[0].mesh.positions[0];
    Arc::make_mut(&mut source.meshes[0].mesh).positions[7] = pinch;
    let error = flatten(&source, Vec3::ZERO, &cancel).err().unwrap();
    assert!(error.message.contains("vertex link"), "{error}");
}

#[test]
fn optional_glass_material_lane_grows_only_authored_uploads() {
    use solidcraft_render::glass_model::{Dispersion, GlassModel, ThinFilm};
    let cancel = CancelToken::default();
    let plain = seam_tetra(Vec3::ZERO);
    let original = flatten(&plain, Vec3::ZERO, &cancel).unwrap();
    let word = |bytes: &[u8], index: usize| u32::from_le_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap());
    let mask = super::MATERIAL_OFFSET_MASK;
    assert_eq!(word(&original.geometry, 2) & !mask, 0);
    assert!(!original.active_glass_model);
    assert_eq!(super::model_bytes_per_triangle(&plain), 0);
    for (model, flags, active) in [
        (GlassModel::default(), 0x80000000, false),
        (GlassModel { thin_film: Some(ThinFilm { ior: 1.38, thickness_nm: 0.0 }), ..Default::default() }, 0x80000000, false),
        (GlassModel { roughness: 0.35, ..Default::default() }, 0x80000000, true),
        (GlassModel { dispersion: Some(Dispersion { abbe_number: 50.0 }), ..Default::default() }, 0xc0000000, true),
    ] {
        let mut modeled = plain.clone();
        modeled.meshes[0].transmission = Some(modeled.meshes[0].transmission.unwrap().with_model(model).unwrap());
        let uploaded = flatten(&modeled, Vec3::ZERO, &cancel).unwrap();
        assert_eq!(uploaded.geometry.len(), original.geometry.len() + plain.meshes[0].mesh.triangles.len() * 16);
        assert_eq!(word(&uploaded.geometry, 2) & !mask, flags, "model={model:?}");
        assert_eq!(word(&uploaded.geometry, 2) & mask, word(&original.geometry, 2));
        assert_eq!(uploaded.active_glass_model, active, "model={model:?}");
        assert_eq!(super::model_bytes_per_triangle(&modeled), 16);
    }
}
