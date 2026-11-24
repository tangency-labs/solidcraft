//! Lighten / Auto-Pocket recipe in `solidcore-ops`: configuration types and full B-rep
//! pocket generation pipeline (loop extraction, offsetting, corner fillets, cutter extrusion,
//! boolean subtraction, and edge fillet/chamfer blending).
//!
//! 2D geometric/trigonometric math is isolated in [`solver`]; this module sequences
//! `solidcore-ops` primitives (`prism`, `boolean`, `pick`, `blend`).

use serde::{Deserialize, Serialize};
use solidcore_record::{Plane, Region2, Vec3};

use crate::blend::{Blend, Corners, blend};
use crate::boolean::{BooleanOp, boolean, union_all};
use crate::pick::edges as pick_edges;
use crate::prism::extrude_planar_regions;
use crate::{Body, Error, Result};

mod solver;

/// Callback type for tapered/drafted prism extrusion when `draft_angle > 0` or bevel flares
/// are requested (`plane, region, from, to, foot, dir_sign, taper_rad`).
pub type TaperExtruder<'a> =
    &'a dyn Fn(&Plane, &Region2, f64, f64, f64, f64, f64) -> std::result::Result<Body, String>;

/// Manufacturing method preset controlling pocket corner and depth defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManufacturingMethod {
    /// CNC endmill pocketing with rounded interior corners.
    CncPocket,
    /// Laser or waterjet through-cut pocketing.
    LaserWaterjet,
    /// Additive manufacturing weight-reduction pocket.
    AdditiveWeightReduction,
}

/// Pocket depth specification relative to the selected planar face.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PocketDepthSpec {
    /// Cut to a fixed depth below the target face (mm).
    BlindDepth(f64),
    /// Leave a constant residual floor thickness (mm).
    FloorThickness(f64),
    /// Cut completely through the solid.
    ThroughHole,
}

/// Which pocket edge set receives the automatic fillet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FilletLocation {
    /// Fillet only pocket floor edges.
    #[default]
    Bottom,
    /// Fillet only pocket top entry edges.
    Top,
    /// Fillet both top and bottom pocket edges.
    All,
}

/// Which pocket edge set receives the automatic chamfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ChamferLocation {
    /// Chamfer only pocket top entry edges.
    #[default]
    Top,
    /// Chamfer only pocket floor edges.
    Bottom,
    /// Chamfer both top and bottom pocket edges.
    All,
}

/// Parametric configuration for `solid.lighten` auto-pocketing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LightenConfig {
    /// Inward offset from outer boundary and outward keep-out from holes (mm).
    pub wall_offset: f64,
    /// Cut depth mode and value.
    pub depth_spec: PocketDepthSpec,
    /// Interior pocket corner fillet radius (mm).
    pub corner_radius: f64,
    /// Side-wall draft angle (degrees).
    pub draft_angle: f64,
    /// Optional automatic pocket edge fillet radius (mm).
    pub auto_fillet_radius: Option<f64>,
    /// Optional automatic pocket edge chamfer size (mm).
    pub auto_chamfer_size: Option<f64>,
    /// Manufacturing process preset.
    pub method: ManufacturingMethod,
    /// Optional face IDs treated as keep-out islands.
    pub island_faces: Vec<u64>,
}

impl Default for LightenConfig {
    fn default() -> Self {
        Self {
            wall_offset: 4.0,
            depth_spec: PocketDepthSpec::BlindDepth(7.0),
            corner_radius: 3.0,
            draft_angle: 0.0,
            auto_fillet_radius: None,
            auto_chamfer_size: None,
            method: ManufacturingMethod::CncPocket,
            island_faces: Vec::new(),
        }
    }
}

impl LightenConfig {
    /// Validate configuration parameters prior to geometry generation.
    pub fn validate(&self) -> std::result::Result<(), String> {
        if !self.wall_offset.is_finite() || self.wall_offset <= 0.0 {
            return Err("wall_offset must be positive".into());
        }
        if !self.corner_radius.is_finite() || self.corner_radius < 0.0 {
            return Err("corner_radius must be non-negative".into());
        }
        if !self.draft_angle.is_finite() || !(0.0..45.0).contains(&self.draft_angle) {
            return Err("draft_angle must be in [0, 45) degrees".into());
        }
        match self.depth_spec {
            PocketDepthSpec::BlindDepth(d) if !d.is_finite() || d <= 0.0 => {
                return Err("blind pocket depth must be positive".into());
            }
            PocketDepthSpec::FloorThickness(t) if !t.is_finite() || t < 0.0 => {
                return Err("floor thickness must be non-negative".into());
            }
            _ => {}
        }
        if let Some(r) = self.auto_fillet_radius {
            if !r.is_finite() || r <= 0.0 {
                return Err("auto_fillet_radius must be positive".into());
            }
        }
        if let Some(c) = self.auto_chamfer_size {
            if !c.is_finite() || c <= 0.0 {
                return Err("auto_chamfer_size must be positive".into());
            }
        }
        Ok(())
    }
}

/// Subtract `cutters` from `target` in the kernel: when multiple cutters are given, union them
/// first (falling back to sequential subtraction if the union is refused).
///
/// # Errors
///
/// Returns [`Error::Argument`] if `cutters` is empty, or any kernel boolean refusal.
pub fn cut_pockets(target: Body, cutters: Vec<Body>) -> Result<Option<Body>> {
    if cutters.is_empty() {
        return Err(Error::Argument);
    }
    if cutters.len() > 1 {
        if let Ok(copied_target) = target.copy() {
            if let Ok(copied_cutters) = cutters.iter().map(Body::copy).collect::<Result<Vec<_>>>() {
                if let Ok(combined) = union_all(copied_cutters) {
                    if let Ok(cut_body) = boolean(copied_target, combined, BooleanOp::Subtract) {
                        return Ok(cut_body);
                    }
                }
            }
        }
    }
    let mut acc = Some(target);
    for cutter in cutters {
        let Some(current) = acc else {
            return Ok(None);
        };
        acc = boolean(current, cutter, BooleanOp::Subtract)?;
    }
    Ok(acc)
}

/// Resolve the pocket cut depth (in mm) along `-face_normal` for `depth_spec`.
pub fn resolve_cut_depth(
    body: &Body,
    plane: &Plane,
    depth_spec: PocketDepthSpec,
) -> std::result::Result<f64, String> {
    let (record, _) = body.to_record().map_err(|e| e.to_string())?;
    solver::resolve_cut_depth(&record, plane, depth_spec)
}

/// Extrude all 2D pocket regions into cutter prism bodies.
fn extrude_pocket_prisms(
    plane: &Plane,
    pocket_regions: &[Region2],
    cut_depth: f64,
    draft_angle: f64,
    taper_extrude: Option<TaperExtruder<'_>>,
) -> std::result::Result<Vec<Body>, String> {
    if solver::is_zero_angle(draft_angle) {
        let mut out = Vec::with_capacity(pocket_regions.len());
        let neg_depth = solver::negate(cut_depth);
        for region in pocket_regions {
            let body = extrude_planar_regions(plane, std::slice::from_ref(region), neg_depth, 0.02)
                .map_err(|e| e.to_string())?;
            out.push(body);
        }
        return Ok(out);
    }
    let Some(taper_fn) = taper_extrude else {
        return Err("tapered pocket extrusion requires a draft extruder".into());
    };
    let taper_rad = solver::deg_to_rad(draft_angle);
    let total_height = solver::prism_total_height(cut_depth);
    let mut cutters = Vec::with_capacity(pocket_regions.len());
    for region in pocket_regions {
        let body = taper_fn(plane, region, 0.0, total_height, 0.0, -1.0, taper_rad)?;
        cutters.push(body);
    }
    Ok(cutters)
}

/// Pick edges near `pts` and apply `Blend::Round` or `Blend::Chamfer`.
fn apply_edge_blend(
    body: &Body,
    pts: &[Vec3],
    kind: Blend,
    size: f64,
) -> std::result::Result<Body, String> {
    let coords: Vec<[f64; 3]> = pts.iter().map(|p| [p.x, p.y, p.z]).collect();
    let tol = solver::blend_pick_tolerance(size);
    let picked = pick_edges(body, &coords, tol)
        .map_err(|e| e.to_string())?
        .map_err(|m| format!("edge pick missed: {m:?}"))?;
    let sizes = vec![size; picked.len()];
    let out = body.copy().map_err(|e| e.to_string())?;
    blend(&out, kind, &picked, &sizes, &[], Corners::Filled).map_err(|e| e.to_string())?;
    Ok(out)
}

/// Build cutter prisms for live 3D pocket preview, optionally adding bevel flares for auto fillet/chamfer.
pub fn preview_pockets_with_locations(
    body: &Body,
    face_id: u64,
    config: &LightenConfig,
    fillet_loc: FilletLocation,
    chamfer_loc: ChamferLocation,
    taper_extrude: Option<TaperExtruder<'_>>,
) -> std::result::Result<Vec<Body>, String> {
    config.validate()?;
    let (record, order) = body.to_record().map_err(|e| e.to_string())?;
    let face_idx = usize::try_from(face_id).map_err(|_| "face_id overflow".to_string())?;
    if face_idx >= record.faces.len() {
        return Err(format!("face_id {face_id} out of range (body has {} faces)", record.faces.len()));
    }
    let plane = solver::body_planar_face(&record, face_idx)
        .ok_or_else(|| format!("target face {face_id} is not planar"))?;
    let pocket_regions = solver::build_pocket_regions(body, &order, &record, face_idx, &plane, config)?;
    let cut_depth = solver::resolve_cut_depth(&record, &plane, config.depth_spec)?;

    let top_bevel = solver::top_bevel_size(config, fillet_loc, chamfer_loc);
    let bottom_bevel = solver::bottom_bevel_size(config, fillet_loc, chamfer_loc);

    if top_bevel.is_none() && bottom_bevel.is_none() {
        return extrude_pocket_prisms(&plane, &pocket_regions, cut_depth, config.draft_angle, taper_extrude);
    }

    let Some(taper_fn) = taper_extrude else {
        return extrude_pocket_prisms(&plane, &pocket_regions, cut_depth, config.draft_angle, None);
    };

    let mut cutters = Vec::with_capacity(pocket_regions.len());
    for region in &pocket_regions {
        let mut pieces = extrude_pocket_prisms(
            &plane,
            std::slice::from_ref(region),
            cut_depth,
            config.draft_angle,
            Some(taper_fn),
        )?;
        if let Some(top_s) = top_bevel {
            if let Some((dilated, bevel_h, taper)) = solver::top_bevel_params(region, top_s, cut_depth) {
                if let Ok(flare) = taper_fn(&plane, &dilated, 0.0, bevel_h, 0.0, -1.0, taper) {
                    pieces.push(flare);
                }
            }
        }
        if let Some(bot_s) = bottom_bevel {
            if let Some((flare_region, start, end, taper)) =
                solver::bottom_bevel_params(region, bot_s, cut_depth, config.draft_angle)
            {
                if let Ok(flare) = taper_fn(&plane, &flare_region, start, end, 0.0, 1.0, taper) {
                    pieces.push(flare);
                }
            }
        }
        if pieces.len() == 1 {
            if let Some(one) = pieces.pop() {
                cutters.push(one);
            }
        } else if !pieces.is_empty() {
            let joined = union_all(pieces).map_err(|e| e.to_string())?;
            cutters.push(joined);
        }
    }
    Ok(cutters)
}

/// Execute the complete `solid.lighten` geometry pipeline on `body` at `face_id`
/// (loop extraction -> offset -> corner fillets -> cutter extrusion -> boolean cut ->
/// edge fillet/chamfer with fallbacks) and return `(resulting_body, warnings)`.
pub fn build_lighten_pockets(
    body: &Body,
    face_id: u64,
    config: &LightenConfig,
    fillet_loc: FilletLocation,
    chamfer_loc: ChamferLocation,
    taper_extrude: Option<TaperExtruder<'_>>,
) -> std::result::Result<(Body, Vec<String>), String> {
    config.validate()?;
    let (record, order) = body.to_record().map_err(|e| e.to_string())?;
    let face_idx = usize::try_from(face_id).map_err(|_| "face_id overflow".to_string())?;
    if face_idx >= record.faces.len() {
        return Err(format!("face_id {face_id} out of range (body has {} faces)", record.faces.len()));
    }
    let plane = solver::body_planar_face(&record, face_idx)
        .ok_or_else(|| format!("target face {face_id} is not planar"))?;

    let pocket_regions = solver::build_pocket_regions(body, &order, &record, face_idx, &plane, config)?;
    let cut_depth = solver::resolve_cut_depth(&record, &plane, config.depth_spec)?;
    let is_through = matches!(config.depth_spec, PocketDepthSpec::ThroughHole);

    let cutters = extrude_pocket_prisms(&plane, &pocket_regions, cut_depth, config.draft_angle, taper_extrude)?;
    if cutters.is_empty() {
        return Err("failed to build pocket cutter prisms".into());
    }

    let pre_edge_mids = solver::collect_face_edge_mids(&record, face_idx);
    let faces_before = record.faces.len();

    let target_copy = body.copy().map_err(|e| e.to_string())?;
    let mut current = cut_pockets(target_copy, cutters)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "auto-pocketing boolean cut consumed the entire solid".to_string())?;

    let counts_after = current.counts().map_err(|e| e.to_string())?;
    if counts_after[0] <= faces_before {
        let vol_before = body.volume(1e-3).unwrap_or(0.0);
        let vol_after = current.volume(1e-3).unwrap_or(0.0);
        if solver::volume_unchanged(vol_before, vol_after) {
            return Err("auto-pocketing cut removed no material".into());
        }
    }

    let mut warnings = Vec::new();
    let has_fillet = config.auto_fillet_radius.is_some();
    let has_chamfer = config.auto_chamfer_size.is_some();

    let mut cached_tracked = if has_fillet || has_chamfer {
        let (cur_rec, _) = current.to_record().map_err(|e| e.to_string())?;
        Some(solver::collect_tracked_pocket_edges(
            &cur_rec,
            &plane,
            cut_depth,
            config.draft_angle,
            &pocket_regions,
            &pre_edge_mids,
            is_through,
        ))
    } else {
        None
    };

    let min_arc_r = solver::min_pocket_arc_radius(&pocket_regions);
    let mut fillet_applied = false;
    if let Some(fillet_r) = config.auto_fillet_radius {
        if let Some(tracked) = cached_tracked.as_ref() {
            let pts = solver::select_fillet_edges(tracked, fillet_loc);
            let is_bottom_fillet = matches!(fillet_loc, FilletLocation::Bottom | FilletLocation::All);
            let primary_r = solver::primary_fillet_radius(fillet_r, min_arc_r, is_bottom_fillet);
            if !pts.is_empty() {
                match apply_edge_blend(&current, &pts, Blend::Round, primary_r) {
                    Ok(filleted) => {
                        current = filleted;
                        fillet_applied = true;
                    }
                    Err(err) => {
                        let fallback_r = solver::fallback_fillet_radius(fillet_r, min_arc_r, cut_depth);
                        if solver::can_retry_fillet(is_bottom_fillet, fillet_r, cut_depth, fallback_r, primary_r) {
                            match apply_edge_blend(&current, &pts, Blend::Round, fallback_r) {
                                Ok(filleted) => {
                                    current = filleted;
                                    fillet_applied = true;
                                }
                                Err(_) => {
                                    warnings.push(format!(
                                        "Auto Fillet ({fillet_r:.2} mm) fell back to unfilleted pocket cut: {err}"
                                    ));
                                }
                            }
                        } else {
                            warnings.push(format!(
                                "Auto Fillet ({fillet_r:.2} mm) fell back to unfilleted pocket cut: {err}"
                            ));
                        }
                    }
                }
            }
        }
    }

    if let Some(chamfer_s) = config.auto_chamfer_size {
        let locations_overlap = (matches!(fillet_loc, FilletLocation::Top | FilletLocation::All)
            && matches!(chamfer_loc, ChamferLocation::Top | ChamferLocation::All))
            || (matches!(fillet_loc, FilletLocation::Bottom | FilletLocation::All)
                && matches!(chamfer_loc, ChamferLocation::Bottom | ChamferLocation::All));
        if fillet_applied && locations_overlap {
            let (cur_rec, _) = current.to_record().map_err(|e| e.to_string())?;
            cached_tracked = Some(solver::collect_tracked_pocket_edges(
                &cur_rec,
                &plane,
                cut_depth,
                config.draft_angle,
                &pocket_regions,
                &pre_edge_mids,
                is_through,
            ));
        }
        if let Some(tracked) = cached_tracked.as_ref() {
            let pts = solver::select_chamfer_edges(tracked, chamfer_loc);
            if !pts.is_empty() {
                match apply_edge_blend(&current, &pts, Blend::Chamfer, chamfer_s) {
                    Ok(chamfered) => {
                        current = chamfered;
                    }
                    Err(err) => {
                        warnings.push(format!(
                            "Auto Chamfer ({chamfer_s:.2} mm) fell back to unchamfered pocket cut: {err}"
                        ));
                    }
                }
            }
        }
    }

    Ok((current, warnings))
}
