//! Shared point-cloud picking (#8933): exact world-space point picks against visible point
//! clouds, respecting active property filters, section clipping planes, and occlusion by solid
//! bodies. Used by headless `view.pick`, the running app's `view.pick`, and interactive viewport
//! picking so every path agrees on which point is picked.

use solidcore_record::Vec3;
use solidcraft_doc::point_cloud::PointCloud;
use solidcraft_geom::PointCloudIndex;
use solidcraft_geom::prelude::*;

use crate::SectionPlane;

/// Default perpendicular radius (mm) around a world ray when `cloud_radius` is omitted.
pub const DEFAULT_CLOUD_RADIUS_MM: f64 = 1.0;

/// Largest accepted `cloud_radius` (mm) for world-ray picks.
pub const MAX_CLOUD_RADIUS_MM: f64 = 1_000_000.0;

/// Section clipping tolerance matching `view_pick::SECTION_EPS`.
const SECTION_EPS: f64 = 1e-6;

/// Depth-band multiplier around the front-most hit: keeps points on the front surface (including
/// slanted faces) while rejecting back walls behind it.
const FRONT_BAND_FACTOR: f64 = 2.0;

/// One picked point of a point cloud.
#[derive(Clone, Debug, PartialEq)]
pub struct CloudPickHit {
    pub cloud: u64,
    pub cloud_name: String,
    pub index: usize,
    pub point: Vec3,
    pub depth: f64,
    pub ray_dist: f64,
}

/// Whether a `view.pick` call picks point-cloud points: `clouds` when given, otherwise only when
/// it names `cloud_radius` or `point_px`. A call written before clouds could be picked names none
/// of them and keeps its answer: a ray through a cloud still meets the body behind it.
pub fn clouds_requested(p: &serde_json::Value) -> std::result::Result<bool, String> {
    use serde_json::Value;
    match p.get("clouds") {
        Some(Value::Bool(b)) => Ok(*b),
        None | Some(Value::Null) => Ok(["cloud_radius", "point_px"].iter().any(|k| p.get(*k).is_some_and(|v| !v.is_null()))),
        Some(_) => Err("view.pick: `clouds` must be true or false".to_string()),
    }
}

/// Whether `p` is kept by every active section plane.
pub fn section_keeps(planes: &[SectionPlane], p: Vec3) -> bool {
    planes.iter().all(|c| c.keeps(p, SECTION_EPS))
}

/// Pick the best visible point of `cloud` along the world ray `(o, d)` within perpendicular radius
/// `a + b·t` at depth `t >= 0` (at most `max_depth` when given), respecting `cloud.filters` and
/// `section`.
///
/// Ranking: among points within `FRONT_BAND_FACTOR * (a + b·t_min)` of the front-most candidate
/// `t_min`, choose the point closest to the ray (normalized by local reach), breaking ties by
/// smaller depth `t` and then smaller point index.
#[allow(clippy::too_many_arguments)]
pub fn pick_cloud_ray(
    cloud: &PointCloud,
    index: Option<&PointCloudIndex>,
    o: Vec3,
    d: Vec3,
    a: f64,
    b: f64,
    max_depth: Option<f64>,
    section: &[SectionPlane],
) -> Option<CloudPickHit> {
    if cloud.is_empty() || ![a, b, o.x, o.y, o.z, d.x, d.y, d.z].iter().all(|v| v.is_finite()) {
        return None;
    }
    let a = a.max(0.0);
    let b = b.max(0.0);
    if a == 0.0 && b == 0.0 {
        return None;
    }
    let dir = d.normalized()?;
    let (lo, ld) = cloud.local_ray(o, dir)?;
    let filter = cloud.point_filter();
    let cap = max_depth.filter(|v| v.is_finite()).unwrap_or(f64::INFINITY);
    if cap < 0.0 {
        return None;
    }

    let mut min_t = f64::INFINITY;
    let mut prune_t = cap;
    let mut cands: Vec<(usize, Vec3, f64, f64, f64)> = Vec::new();

    let mut consider = |idx: u32, max_t: &mut f64| {
        let i = idx as usize;
        let Some(raw) = cloud.points.get(i) else { return };
        let lp = Vec3::new(f64::from(raw[0]), f64::from(raw[1]), f64::from(raw[2]));
        let rel = lp - lo;
        let t = rel.dot(ld);
        if !(0.0..=*max_t).contains(&t) {
            return;
        }
        let reach = a + b * t;
        let perp2 = (rel - ld * t).len2();
        if !(perp2.is_finite() && perp2 <= reach * reach) {
            return;
        }
        let Some(world) = cloud.world_point(i) else { return };
        if !filter.keeps(i, world) || !section_keeps(section, world) {
            return;
        }
        let perp = perp2.sqrt();
        if t < min_t {
            min_t = t;
            let band_end = min_t + FRONT_BAND_FACTOR * (a + b * min_t);
            if band_end < *max_t {
                *max_t = band_end;
            }
        }
        let norm = if reach > 1e-12 { perp / reach } else { perp };
        cands.push((i, world, t, perp, norm));
    };

    match index.filter(|ix| ix.point_count() == cloud.len()) {
        Some(ix) => ix.visit_near_ray(lo, ld, a, b, &mut prune_t, consider),
        None => {
            for i in 0..cloud.len() {
                consider(i as u32, &mut prune_t);
            }
        }
    }

    let cutoff = min_t + FRONT_BAND_FACTOR * (a + b * min_t);
    let best = cands
        .into_iter()
        .filter(|(_, _, t, _, _)| *t <= cutoff)
        .min_by(|a, b| a.4.total_cmp(&b.4).then_with(|| a.2.total_cmp(&b.2)).then_with(|| a.0.cmp(&b.0)))?;

    Some(CloudPickHit { cloud: cloud.id, cloud_name: cloud.name.clone(), index: best.0, point: best.1, depth: best.2, ray_dist: best.3 })
}

/// Pick the best visible point of `cloud` near a screen pixel using `screen_dist` (which returns
/// the logical pixel distance if the world point lies in front of the camera and within `pick_px`).
/// Uses the same front-cluster depth band so front surfaces win over back walls without biasing
/// toward the near rim of a slanted face.
#[allow(clippy::too_many_arguments)]
pub fn pick_cloud_screen(
    cloud: &PointCloud,
    index: Option<&PointCloudIndex>,
    o: Vec3,
    d: Vec3,
    a: f64,
    b: f64,
    max_depth: Option<f64>,
    section: &[SectionPlane],
    mut screen_dist: impl FnMut(Vec3) -> Option<f32>,
) -> Option<(CloudPickHit, f32)> {
    if cloud.is_empty() || ![a, b, o.x, o.y, o.z, d.x, d.y, d.z].iter().all(|v| v.is_finite()) {
        return None;
    }
    let a = a.max(0.0);
    let b = b.max(0.0);
    let dir = d.normalized()?;
    let (lo, ld) = cloud.local_ray(o, dir)?;
    let filter = cloud.point_filter();
    let cap = max_depth.filter(|v| v.is_finite()).unwrap_or(f64::INFINITY);
    if cap < 0.0 {
        return None;
    }

    let mut min_t = f64::INFINITY;
    let mut prune_t = cap;
    let mut cands: Vec<(usize, Vec3, f64, f64, f32)> = Vec::new();

    let mut consider = |idx: u32, max_t: &mut f64| {
        let i = idx as usize;
        let Some(raw) = cloud.points.get(i) else { return };
        let lp = Vec3::new(f64::from(raw[0]), f64::from(raw[1]), f64::from(raw[2]));
        let rel = lp - lo;
        let t = rel.dot(ld);
        if !(0.0..=*max_t).contains(&t) {
            return;
        }
        let reach = a + b * t;
        let perp2 = (rel - ld * t).len2();
        if !(perp2.is_finite() && perp2 <= reach * reach) {
            return;
        }
        let Some(world) = cloud.world_point(i) else { return };
        if !filter.keeps(i, world) || !section_keeps(section, world) {
            return;
        }
        let Some(dist_px) = screen_dist(world) else { return };
        if !dist_px.is_finite() {
            return;
        }
        if t < min_t {
            min_t = t;
            let band_end = min_t + FRONT_BAND_FACTOR * (a + b * min_t);
            if band_end < *max_t {
                *max_t = band_end;
            }
        }
        cands.push((i, world, t, perp2.sqrt(), dist_px));
    };

    match index.filter(|ix| ix.point_count() == cloud.len()) {
        Some(ix) => ix.visit_near_ray(lo, ld, a, b, &mut prune_t, consider),
        None => {
            for i in 0..cloud.len() {
                consider(i as u32, &mut prune_t);
            }
        }
    }

    let cutoff = min_t + FRONT_BAND_FACTOR * (a + b * min_t);
    let best = cands
        .into_iter()
        .filter(|(_, _, t, _, _)| *t <= cutoff)
        .min_by(|a, b| a.4.total_cmp(&b.4).then_with(|| a.2.total_cmp(&b.2)).then_with(|| a.0.cmp(&b.0)))?;

    Some((CloudPickHit { cloud: cloud.id, cloud_name: cloud.name.clone(), index: best.0, point: best.1, depth: best.2, ray_dist: best.3 }, best.4))
}
