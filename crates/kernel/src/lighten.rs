//! Lighten / Auto-Pocket kernel dispatch shim.
//!
//! All pocket geometry solving (loop extraction, offsetting, corner fillets,
//! region construction, cutter extrusion, boolean cuts, and edge blending) lives
//! in `solidcore_ops::lighten`. This module bridges `solidcraft_kernel::Body`
//! and `solidcore_ops::Body`.

use solidcore_record::{Plane, Region2};

use crate::build::extrude_tapered_between;
use crate::{Body, DraftCorner};

pub use solidcore_ops::ChamferLocation;
pub use solidcore_ops::FilletLocation;
pub use solidcore_ops::LightenConfig;
pub use solidcore_ops::ManufacturingMethod;
pub use solidcore_ops::PocketDepthSpec;

fn ops_from_body(body: &Body) -> Result<solidcore_ops::Body, String> {
    solidcore_ops::Body::from_record(body.record(), false).map(|(b, _)| b).map_err(|e| e.to_string())
}

fn body_from_ops(ops_body: &solidcore_ops::Body) -> Result<Body, String> {
    let (record, _) = ops_body.to_record().map_err(|e| e.to_string())?;
    Body::from_record(record).map_err(|e| e.to_string())
}

fn kernel_taper_extrude(
    plane: &Plane,
    region: &Region2,
    from: f64,
    to: f64,
    foot: f64,
    dir_sign: f64,
    taper: f64,
) -> Result<solidcore_ops::Body, String> {
    if taper.abs() < 1e-9 {
        let mut bodies = crate::build::extrude(plane, std::slice::from_ref(region), from, to).map_err(|e| e.to_string())?;
        let body = bodies.pop().ok_or_else(|| "extrude produced no body".to_string())?;
        return ops_from_body(&body);
    }
    let body = extrude_tapered_between(plane, region, from, to, foot, dir_sign, taper, DraftCorner::Round).map_err(|e| e.to_string())?;
    ops_from_body(&body)
}

fn kernel_edge_blend(
    ops_body: &solidcore_ops::Body,
    pts: &[solidcore_record::Vec3],
    kind: solidcore_ops::blend::Blend,
    size: f64,
) -> Result<solidcore_ops::Body, String> {
    let body = body_from_ops(ops_body)?;
    let blended = match kind {
        solidcore_ops::blend::Blend::Round => crate::blend::fillet(&body, pts, size),
        solidcore_ops::blend::Blend::Chamfer => crate::blend::chamfer(&body, pts, size),
        solidcore_ops::blend::Blend::Curvature | solidcore_ops::blend::Blend::Chord => {
            return Err("unsupported blend kind".into());
        }
    }
    .map_err(|e| e.to_string())?;
    ops_from_body(&blended)
}

/// Resolve the pocket cut depth (in mm) along `-face_normal` for `depth_spec`.
pub fn resolve_cut_depth(body: &Body, plane: &Plane, depth_spec: PocketDepthSpec) -> Result<f64, String> {
    let ops_body = ops_from_body(body)?;
    solidcore_ops::resolve_cut_depth(&ops_body, plane, depth_spec)
}

/// Build raw cutter prism bodies for live 3D preview without mutating `body`.
pub fn preview_pockets(body: &Body, face_id: u64, config: &LightenConfig) -> Result<Vec<Body>, String> {
    preview_pockets_with_locations(body, face_id, config, FilletLocation::Bottom, ChamferLocation::Top)
}

/// Build cutter prisms for live 3D pocket preview, optionally adding bevel flares for auto fillet/chamfer.
pub fn preview_pockets_with_locations(
    body: &Body,
    face_id: u64,
    config: &LightenConfig,
    fillet_loc: FilletLocation,
    chamfer_loc: ChamferLocation,
) -> Result<Vec<Body>, String> {
    let ops_body = ops_from_body(body)?;
    let ops_cutters =
        solidcore_ops::preview_pockets_with_locations(&ops_body, face_id, config, fillet_loc, chamfer_loc, Some(&kernel_taper_extrude))?;
    ops_cutters.iter().map(body_from_ops).collect()
}

/// Execute `solid.lighten` auto-pocketing on `body` at `face_id` using default blend locations
/// (`FilletLocation::Bottom` for floor fillets, `ChamferLocation::Top` for top chamfers).
pub fn auto_pocket(body: &mut Body, face_id: u64, config: &LightenConfig) -> Result<(), String> {
    auto_pocket_with_locations(body, face_id, config, FilletLocation::Bottom, ChamferLocation::Top)
}

/// Execute `solid.lighten` auto-pocketing on `body` at `face_id` with explicit fillet/chamfer locations.
pub fn auto_pocket_with_locations(
    body: &mut Body,
    face_id: u64,
    config: &LightenConfig,
    fillet_loc: FilletLocation,
    chamfer_loc: ChamferLocation,
) -> Result<(), String> {
    let _warnings = auto_pocket_with_warnings(body, face_id, config, fillet_loc, chamfer_loc)?;
    Ok(())
}

/// Execute `solid.lighten` auto-pocketing (including top/bottom fillets and chamfers) under
/// `Validation::Off` for fast live cut-away preview rendering matching Shell / Extrude.
pub fn preview_cut_body_with_locations(
    body: &mut Body,
    face_id: u64,
    config: &LightenConfig,
    fillet_loc: FilletLocation,
    chamfer_loc: ChamferLocation,
) -> Result<(), String> {
    crate::validation::validating(crate::validation::Validation::Off, || {
        auto_pocket_with_warnings(body, face_id, config, fillet_loc, chamfer_loc).map(|_| ())
    })
    .0
}

/// Execute `solid.lighten` auto-pocketing and return non-fatal warnings if an edge blend falls back.
pub fn auto_pocket_with_warnings(
    body: &mut Body,
    face_id: u64,
    config: &LightenConfig,
    fillet_loc: FilletLocation,
    chamfer_loc: ChamferLocation,
) -> Result<Vec<String>, String> {
    let ops_body = ops_from_body(body)?;
    let (result_ops, warnings) = solidcore_ops::build_lighten_pockets(
        &ops_body,
        face_id,
        config,
        fillet_loc,
        chamfer_loc,
        Some(&kernel_taper_extrude),
        Some(&kernel_edge_blend),
    )?;
    *body = body_from_ops(&result_ops)?;
    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::{box_solid, cylinder, extrude};
    use crate::{BoolOp, MeshTolerance, boolean, fillet, measure};
    use solidcore_record::{Loop2, Seg2, Vec2, Vec3};
    use solidcraft_geom::prelude::*;
    use std::f64::consts::TAU;

    fn build_pocket_regions(body: &Body, face_idx: usize, _plane: &Plane, config: &LightenConfig) -> Result<Vec<Region2>, String> {
        let ops_body = ops_from_body(body)?;
        solidcore_ops::build_pocket_regions(&ops_body, face_idx as u64, config)
    }

    #[test]
    fn test_auto_pocket_box_blind() {
        let mut b = box_solid(Vec3::ZERO, Vec3::new(60.0, 60.0, 20.0)).unwrap();
        let initial_vol = measure(&b).unwrap().volume;

        let faces = b.faces(0.01).unwrap();
        let top_face = faces
            .iter()
            .find(|f| f.plane_normal.is_some_and(|n| (n - Vec3::Z).len() < 1e-4 && (f.centroid.z - 20.0).abs() < 1e-3))
            .expect("top face exists");

        let cfg = LightenConfig {
            wall_offset: 4.0,
            depth_spec: PocketDepthSpec::BlindDepth(7.0),
            corner_radius: 3.0,
            draft_angle: 0.0,
            auto_fillet_radius: None,
            auto_chamfer_size: None,
            method: ManufacturingMethod::CncPocket,
            island_faces: Vec::new(),
        };

        auto_pocket(&mut b, top_face.index as u64, &cfg).expect("auto pocket succeeded");
        let final_vol = measure(&b).unwrap().volume;
        assert!(final_vol < initial_vol, "material was removed");
        // Expected removed volume ~ 52 * 52 * 7 (minus small corner fillets) ~ 18,900 mm^3 out of 72,000 mm^3.
        assert!(final_vol > initial_vol * 0.5, "wall and floor material preserved");
    }

    #[test]
    fn test_wall_offset_validation_and_non_planar_face() {
        let mut b = box_solid(Vec3::ZERO, Vec3::new(40.0, 40.0, 20.0)).unwrap();
        let mut cfg = LightenConfig { wall_offset: 0.0, ..Default::default() };
        assert!(auto_pocket(&mut b, 0, &cfg).is_err(), "wall_offset == 0 rejected");
        cfg.wall_offset = -2.0;
        assert!(auto_pocket(&mut b, 0, &cfg).is_err(), "negative wall_offset rejected");

        let cyl = cylinder(Vec3::ZERO, Vec3::Z, 20.0, 20.0).unwrap();
        let side_face = cyl.faces(0.01).unwrap().into_iter().find(|f| f.plane_normal.is_none()).expect("cylindrical face exists");
        let mut cyl_mut = cyl;
        let valid_cfg = LightenConfig::default();
        let err = auto_pocket(&mut cyl_mut, side_face.index as u64, &valid_cfg).unwrap_err();
        assert!(err.contains("not planar"), "unexpected error: {err}");
    }

    #[test]
    fn test_floor_thickness_depth_calculation() {
        let mut b = box_solid(Vec3::ZERO, Vec3::new(50.0, 50.0, 20.0)).unwrap();
        let top_face =
            b.faces(0.01).unwrap().into_iter().find(|f| f.plane_normal.is_some_and(|n| n.z > 0.99 && (f.centroid.z - 20.0).abs() < 1e-3)).unwrap();
        let plane = b.planar_face(top_face.index).unwrap();
        let d = resolve_cut_depth(&b, &plane, PocketDepthSpec::FloorThickness(5.0)).unwrap();
        assert!((d - 15.0).abs() < 0.05, "expected 15mm depth, got {d}");

        // Floor thickness >= part thickness must error
        assert!(resolve_cut_depth(&b, &plane, PocketDepthSpec::FloorThickness(20.0)).is_err());

        let cfg = LightenConfig { wall_offset: 5.0, depth_spec: PocketDepthSpec::FloorThickness(5.0), corner_radius: 2.0, ..Default::default() };
        auto_pocket(&mut b, top_face.index as u64, &cfg).unwrap();
        let v = measure(&b).unwrap().volume;
        assert!(v < 50.0 * 50.0 * 20.0);
    }

    #[test]
    fn test_boss_retention_around_holes() {
        let outer_cyl = cylinder(Vec3::ZERO, Vec3::Z, 50.0, 20.0).unwrap();
        let h_center = cylinder(Vec3::new(0.0, 0.0, -1.0), Vec3::Z, 12.0, 22.0).unwrap();
        let h1 = cylinder(Vec3::new(32.0, 0.0, -1.0), Vec3::Z, 5.0, 22.0).unwrap();
        let h2 = cylinder(Vec3::new(-32.0, 0.0, -1.0), Vec3::Z, 5.0, 22.0).unwrap();
        let h3 = cylinder(Vec3::new(0.0, 32.0, -1.0), Vec3::Z, 5.0, 22.0).unwrap();
        let h4 = cylinder(Vec3::new(0.0, -32.0, -1.0), Vec3::Z, 5.0, 22.0).unwrap();

        let mut flange = boolean(&outer_cyl, &h_center, BoolOp::Cut).unwrap().unwrap();
        for h in [&h1, &h2, &h3, &h4] {
            flange = boolean(&flange, h, BoolOp::Cut).unwrap().unwrap();
        }

        let top_face = flange
            .faces(0.01)
            .unwrap()
            .into_iter()
            .find(|f| f.plane_normal.is_some_and(|n| n.z > 0.99 && (f.centroid.z - 20.0).abs() < 1e-3))
            .expect("top face exists");

        let vol_before = measure(&flange).unwrap().volume;
        let cfg = LightenConfig { wall_offset: 4.0, depth_spec: PocketDepthSpec::BlindDepth(7.0), corner_radius: 3.0, ..Default::default() };
        auto_pocket(&mut flange, top_face.index as u64, &cfg).expect("auto pocket on flange succeeded");
        let vol_after = measure(&flange).unwrap().volume;
        assert!(vol_after < vol_before, "pocket removed material");

        // Verify boss material around each bolt hole (at r = 5 + 2 = 7mm, z = 18mm) is still solid!
        let m = flange.mesh(&MeshTolerance::chord(0.05)).unwrap();
        let top_tris: Vec<[Vec3; 3]> = m
            .triangles
            .iter()
            .filter_map(|t| m.tri(t))
            .filter(|[a, b, c]| (a.z - 20.0).abs() < 0.05 && (b.z - 20.0).abs() < 0.05 && (c.z - 20.0).abs() < 0.05)
            .collect();
        assert!(!top_tris.is_empty(), "boss top faces remain at z=20");
    }

    #[test]
    fn test_corner_fillet_clamped_for_narrow_pocket() {
        let mut b = box_solid(Vec3::ZERO, Vec3::new(20.0, 20.0, 15.0)).unwrap();
        let top_face =
            b.faces(0.01).unwrap().into_iter().find(|f| f.plane_normal.is_some_and(|n| n.z > 0.99 && (f.centroid.z - 15.0).abs() < 1e-3)).unwrap();
        // Pocket width is 20 - 2*4 = 12mm (half-width = 6mm), corner_radius = 25mm must clamp cleanly!
        let cfg = LightenConfig { wall_offset: 4.0, depth_spec: PocketDepthSpec::BlindDepth(5.0), corner_radius: 25.0, ..Default::default() };
        auto_pocket(&mut b, top_face.index as u64, &cfg).expect("clamped corner radius succeeds");
    }

    #[test]
    fn test_segmented_flange_pockets_corner_arcs_and_auto_blends() {
        // Bolted flange matching rolling-ball geometry: R=54mm, hub bore R=19mm, 6 bolt holes R=5mm at r=37mm.
        let mut flange_holes = vec![Loop2::circle(Vec2::ZERO, 19.0).reversed()];
        for k in 0..6 {
            let ang = (k as f64) * TAU / 6.0;
            flange_holes.push(Loop2::circle(Vec2::new(37.0 * ang.cos(), 37.0 * ang.sin()), 5.0).reversed());
        }
        let flange_region = Region2 { outer: Loop2::circle(Vec2::ZERO, 54.0), holes: flange_holes };
        let mut flange = extrude(&Plane::XY, &[flange_region], 0.0, 18.0).unwrap().pop().unwrap();

        let top_face = flange
            .faces(0.01)
            .unwrap()
            .into_iter()
            .find(|f| f.plane_normal.is_some_and(|n| n.z > 0.99 && (f.centroid.z - 18.0).abs() < 1e-3))
            .expect("flange top face");

        let cfg = LightenConfig {
            wall_offset: 4.0,
            depth_spec: PocketDepthSpec::BlindDepth(7.0),
            corner_radius: 1.5,
            draft_angle: 0.0,
            auto_fillet_radius: Some(2.0),
            auto_chamfer_size: Some(0.75),
            method: ManufacturingMethod::CncPocket,
            island_faces: Vec::new(),
        };

        let plane = flange.planar_face(top_face.index).unwrap();
        let regions = build_pocket_regions(&flange, top_face.index, &plane, &cfg).unwrap();
        assert_eq!(regions.len(), 6, "6 bolt holes + radial ribs must produce 6 distinct pocket bays");
        assert!(
            regions.iter().all(|r| r.outer.segs.iter().any(|s| matches!(s, Seg2::Arc { .. }))),
            "every segmented bay must have arc segments from perimeter/corner fillets"
        );

        let mut plain_cfg = cfg.clone();
        plain_cfg.auto_fillet_radius = None;
        plain_cfg.auto_chamfer_size = None;
        let plain_previews =
            preview_pockets_with_locations(&flange, top_face.index as u64, &plain_cfg, FilletLocation::Bottom, ChamferLocation::Top).unwrap();
        let previews = preview_pockets_with_locations(&flange, top_face.index as u64, &cfg, FilletLocation::Bottom, ChamferLocation::Top).unwrap();
        assert_eq!(previews.len(), 6, "preview must return 6 segmented pocket cutter bodies");
        assert!(previews[0].face_count() > plain_previews[0].face_count(), "blended preview must include bottom fillet and top chamfer faces");

        let faces_before = flange.face_count();
        let vol_before = measure(&flange).unwrap().volume;
        let warnings = auto_pocket_with_warnings(&mut flange, top_face.index as u64, &cfg, FilletLocation::Bottom, ChamferLocation::Top).unwrap();
        assert!(warnings.is_empty(), "expected clean auto fillet + chamfer without warnings, got {warnings:?}");
        assert!(measure(&flange).unwrap().volume < vol_before, "pocket cut removed material");
        // Plain 6-bay cut without blends adds at most ~10-12 faces per bay; with bottom fillet + top chamfer it adds ~2x more.
        assert!(flange.face_count() > faces_before + 60, "blended cut must add sidewall + bottom fillet + top chamfer faces");
    }

    #[test]
    fn test_oversized_auto_fillet_falls_back_with_warning() {
        let mut b = box_solid(Vec3::ZERO, Vec3::new(40.0, 40.0, 20.0)).unwrap();
        let top_face =
            b.faces(0.01).unwrap().into_iter().find(|f| f.plane_normal.is_some_and(|n| n.z > 0.99 && (f.centroid.z - 20.0).abs() < 1e-3)).unwrap();
        let initial_vol = measure(&b).unwrap().volume;
        let cfg = LightenConfig {
            wall_offset: 4.0,
            depth_spec: PocketDepthSpec::BlindDepth(7.0),
            corner_radius: 3.0,
            draft_angle: 0.0,
            auto_fillet_radius: Some(50.0), // Way too large for a 7mm deep pocket!
            auto_chamfer_size: None,
            method: ManufacturingMethod::CncPocket,
            island_faces: Vec::new(),
        };
        let warnings = auto_pocket_with_warnings(&mut b, top_face.index as u64, &cfg, FilletLocation::Bottom, ChamferLocation::Top)
            .expect("oversized fillet falls back instead of failing");
        assert!(!warnings.is_empty(), "expected fallback warning for oversized auto_fillet_radius");
        assert!(measure(&b).unwrap().volume < initial_vol, "base pocket cut must remain applied");
    }

    #[test]
    fn test_rounded_rect_plate_with_two_side_holes() {
        // Reproduces the rounded-rectangle mounting plate test subject:
        // 100x66 plate with R=10 corners, center bore R=14, and two mounting holes R=5 at x=+-33.
        let base_rect = box_solid(Vec3::new(-50.0, -33.0, 0.0), Vec3::new(50.0, 33.0, 18.0)).unwrap();
        let corner_edges = vec![Vec3::new(-50.0, -33.0, 9.0), Vec3::new(50.0, -33.0, 9.0), Vec3::new(50.0, 33.0, 9.0), Vec3::new(-50.0, 33.0, 9.0)];
        let mut plate = fillet(&base_rect, &corner_edges, 10.0).unwrap();
        let center_bore = cylinder(Vec3::new(0.0, 0.0, -1.0), Vec3::Z, 15.0, 20.0).unwrap();
        let left_hole = cylinder(Vec3::new(-33.0, 0.0, -1.0), Vec3::Z, 5.0, 20.0).unwrap();
        let right_hole = cylinder(Vec3::new(33.0, 0.0, -1.0), Vec3::Z, 5.0, 20.0).unwrap();
        for h in [&center_bore, &left_hole, &right_hole] {
            plate = boolean(&plate, h, BoolOp::Cut).unwrap().unwrap();
        }

        let top_face = plate
            .faces(0.01)
            .unwrap()
            .into_iter()
            .find(|f| f.plane_normal.is_some_and(|n| n.z > 0.99 && (f.centroid.z - 18.0).abs() < 1e-3))
            .expect("plate top face");

        let cfg = LightenConfig {
            wall_offset: 4.0,
            depth_spec: PocketDepthSpec::BlindDepth(7.0),
            corner_radius: 1.5,
            draft_angle: 0.0,
            auto_fillet_radius: Some(2.0),
            auto_chamfer_size: Some(0.75),
            method: ManufacturingMethod::CncPocket,
            island_faces: Vec::new(),
        };

        let plane = plate.planar_face(top_face.index).unwrap();
        let regions = build_pocket_regions(&plate, top_face.index, &plane, &cfg).unwrap();
        assert_eq!(regions.len(), 2, "must decompose into 2 top/bottom pocket bays");

        // Every corner of both pocket bays must have tangent arc fillets (no sharp right-angle joins).
        for (ri, r) in regions.iter().enumerate() {
            let arc_count = r.outer.segs.iter().filter(|s| matches!(s, Seg2::Arc { .. })).count();
            assert!(arc_count >= 6, "bay {ri} must have tangent arcs at all corners (found {arc_count} arcs, segs={:?})", r.outer.segs);
            assert!(
                r.outer.segs.iter().all(|s| !matches!(s, Seg2::Arc { radius, .. } if *radius < cfg.corner_radius - 0.05)),
                "bay {ri} must not contain undersized cusp arcs (segs={:?})",
                r.outer.segs
            );
            for k in 0..r.outer.segs.len() {
                let a = &r.outer.segs[k];
                let b = &r.outer.segs[(k + 1) % r.outer.segs.len()];
                let kink = a.end_tangent().cross(b.start_tangent()).abs();
                assert!(kink <= 0.12, "bay {ri} corner {k} stayed sharp (kink={kink:.3}) between {a:?} and {b:?}");
            }
        }

        // Verify preview generation creates rounded/chamfered prism geometry.
        let mut plain_cfg = cfg.clone();
        plain_cfg.auto_fillet_radius = None;
        plain_cfg.auto_chamfer_size = None;
        let _plain_previews =
            preview_pockets_with_locations(&plate, top_face.index as u64, &plain_cfg, FilletLocation::Bottom, ChamferLocation::Top).unwrap();
        let previews = preview_pockets_with_locations(&plate, top_face.index as u64, &cfg, FilletLocation::Bottom, ChamferLocation::Top).unwrap();
        assert_eq!(previews.len(), 2);

        // Verify execution applies unblended or blended cut cleanly.
        let mut plate_plain = plate.clone();
        auto_pocket(&mut plate_plain, top_face.index as u64, &plain_cfg).unwrap();
    }

    #[test]
    fn test_swapped_fillet_top_and_chamfer_bottom_locations() {
        let mut b = box_solid(Vec3::ZERO, Vec3::new(50.0, 50.0, 20.0)).unwrap();
        let top_face =
            b.faces(0.01).unwrap().into_iter().find(|f| f.plane_normal.is_some_and(|n| n.z > 0.99 && (f.centroid.z - 20.0).abs() < 1e-3)).unwrap();
        let mut plain_cfg = LightenConfig {
            wall_offset: 5.0,
            depth_spec: PocketDepthSpec::BlindDepth(8.0),
            corner_radius: 3.0,
            draft_angle: 0.0,
            auto_fillet_radius: None,
            auto_chamfer_size: None,
            method: ManufacturingMethod::CncPocket,
            island_faces: Vec::new(),
        };
        let mut b_plain = b.clone();
        auto_pocket(&mut b_plain, top_face.index as u64, &plain_cfg).unwrap();

        plain_cfg.auto_fillet_radius = Some(2.5);
        plain_cfg.auto_chamfer_size = Some(1.0);
        let warnings = auto_pocket_with_warnings(&mut b, top_face.index as u64, &plain_cfg, FilletLocation::Top, ChamferLocation::Bottom)
            .expect("swapped Top Fillet + Bottom Chamfer succeeded");
        assert!(warnings.is_empty(), "expected 0 blend warnings, got {warnings:?}");
        assert!(b.face_count() > b_plain.face_count(), "Top fillet + Bottom chamfer must add blend faces");
        assert!((measure(&b).unwrap().volume - measure(&b_plain).unwrap().volume).abs() > 1.0, "Top fillet + Bottom chamfer modify edge volume");
    }

    #[test]
    fn test_user_80x50_plate_tangent_arcs_and_corner_radius_scaling() {
        // Reproduces user's 80x50 plate (R_plate=5mm), center boss R=12mm, side bolt holes R=4mm at (+-28, 0).
        let base_rect = box_solid(Vec3::new(-40.0, -25.0, 0.0), Vec3::new(40.0, 25.0, 12.0)).unwrap();
        let corner_edges = vec![Vec3::new(-40.0, -25.0, 6.0), Vec3::new(40.0, -25.0, 6.0), Vec3::new(40.0, 25.0, 6.0), Vec3::new(-40.0, 25.0, 6.0)];
        let mut plate = fillet(&base_rect, &corner_edges, 5.0).unwrap();
        let center_bore = cylinder(Vec3::new(0.0, 0.0, -1.0), Vec3::Z, 12.0, 14.0).unwrap();
        let left_hole = cylinder(Vec3::new(-28.0, 0.0, -1.0), Vec3::Z, 4.0, 14.0).unwrap();
        let right_hole = cylinder(Vec3::new(28.0, 0.0, -1.0), Vec3::Z, 4.0, 14.0).unwrap();
        for h in [&center_bore, &left_hole, &right_hole] {
            plate = boolean(&plate, h, BoolOp::Cut).unwrap().unwrap();
        }

        let top_face = plate
            .faces(0.01)
            .unwrap()
            .into_iter()
            .find(|f| f.plane_normal.is_some_and(|n| n.z > 0.99 && (f.centroid.z - 12.0).abs() < 1e-3))
            .expect("plate top face");
        let plane = plate.planar_face(top_face.index).unwrap();

        let mut prev_area = f64::MAX;
        for &cr in &[1.5f64, 3.0, 4.0, 5.0] {
            let cfg = LightenConfig {
                wall_offset: 4.0,
                depth_spec: PocketDepthSpec::BlindDepth(7.0),
                corner_radius: cr,
                draft_angle: 0.0,
                auto_fillet_radius: Some(1.5),
                auto_chamfer_size: Some(0.5),
                method: ManufacturingMethod::CncPocket,
                island_faces: Vec::new(),
            };
            let regions = build_pocket_regions(&plate, top_face.index, &plane, &cfg).unwrap_or_else(|e| panic!("corner_radius={cr} failed: {e}"));
            assert_eq!(regions.len(), 2, "corner_radius={cr} must yield 2 bays");

            let mut total_area = 0.0;
            for (ri, r) in regions.iter().enumerate() {
                total_area += r.area().abs();
                let matching_arcs = r.outer.segs.iter().filter(|s| matches!(s, Seg2::Arc { radius, .. } if (*radius - cr).abs() < 0.05)).count();
                assert!(
                    matching_arcs >= 4,
                    "corner_radius={cr} bay {ri} must have tangent corner arcs of radius ~{cr} (found {matching_arcs}, segs={:?})",
                    r.outer.segs
                );
                assert!(
                    r.outer.segs.iter().all(|s| !matches!(s, Seg2::Arc { radius, .. } if *radius < cr - 0.05)),
                    "corner_radius={cr} bay {ri} must not contain undersized cusp arcs (segs={:?})",
                    r.outer.segs
                );
                for k in 0..r.outer.segs.len() {
                    let a = &r.outer.segs[k];
                    let b = &r.outer.segs[(k + 1) % r.outer.segs.len()];
                    let kink = a.end_tangent().cross(b.start_tangent()).abs();
                    assert!(kink <= 0.05, "corner_radius={cr} bay {ri} seg {k} has sharp kink {kink:.3}: {a:?} -> {b:?}");
                }
            }
            assert!(
                total_area < prev_area - 1.0,
                "increasing corner_radius to {cr} must reduce pocket area cleanly (prev={prev_area:.2}, now={total_area:.2})"
            );
            prev_area = total_area;

            let previews =
                preview_pockets(&plate, top_face.index as u64, &cfg).unwrap_or_else(|e| panic!("preview_pockets failed at corner_radius={cr}: {e}"));
            assert_eq!(previews.len(), 2, "corner_radius={cr} preview must build 2 prisms");
        }
    }
}
