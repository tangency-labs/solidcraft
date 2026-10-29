//! Parametric lightening / pocketing on a planar face.
//!
//! Lightens a solid body by creating structural pocket cavities on a planar face:
//! - Insets the outer boundary by `wall_offset`
//! - Outsets interior hole loops and the central hub loop (if present) by `wall_offset`
//! - Constructs radial web bands between the hub and bolt holes to retain structural stiffness
//! - Subtracts the keep-out regions (hub, holes, webs) from the inset outer loop to form pocket profiles
//! - Fillets concave corners of pocket profiles in 2D with `corner_radius`
//! - Extrudes pocket profiles into the body by `pocket_depth` (with optional `draft_angle`)
//! - Subtracts pockets from the body via 3D boolean cut
//! - Fillets internal bottom floor perimeter edges with `floor_radius`, while strictly protecting pre-existing external edges.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use truck_modeling as mt;
use mt::{BoundedCurve, ParametricCurve};

use crate::body::{Body, from_p3};
use crate::prism::planar_regions_boolean;
use crate::{BoolOp, KernelError, Result};

/// Configuration options for the lightening feature.
#[derive(Clone, Debug, PartialEq)]
pub struct LightenOptions {
    /// Inset thickness from outer rim, and outset clearance around holes and hub (mm).
    pub wall_offset: f64,
    /// Depth of pocket cavity extruded into the body (mm).
    pub pocket_depth: f64,
    /// Wall draft angle (radians, >= 0).
    pub draft_angle: f64,
    /// 2D corner fillet radius at concave corners of pocket profiles (mm).
    pub corner_radius: f64,
    /// Auto Fillet enabled.
    pub auto_fillet: bool,
    /// 3D fillet radius (mm).
    pub fillet_radius: f64,
    /// Apply fillet to top pocket edges.
    pub fillet_top: bool,
    /// Apply fillet to bottom floor perimeter edges.
    pub fillet_bottom: bool,
    /// Auto Chamfer enabled.
    pub auto_chamfer: bool,
    /// 3D chamfer size (mm).
    pub chamfer_size: f64,
    /// Apply chamfer to top pocket edges.
    pub chamfer_top: bool,
    /// Apply chamfer to bottom floor perimeter edges.
    pub chamfer_bottom: bool,
    /// Backward compatibility floor radius.
    pub floor_radius: f64,
    /// Backward compatibility method.
    pub manufacturing_method: Option<String>,
}

impl Default for LightenOptions {
    fn default() -> Self {
        Self {
            wall_offset: 2.0,
            pocket_depth: 3.0,
            draft_angle: 0.0,
            corner_radius: 1.0,
            auto_fillet: false,
            fillet_radius: 1.0,
            fillet_top: false,
            fillet_bottom: false,
            auto_chamfer: false,
            chamfer_size: 0.5,
            chamfer_top: false,
            chamfer_bottom: false,
            floor_radius: 0.0,
            manufacturing_method: None,
        }
    }
}

/// Execute the lightening algorithm on target face of the body.
pub fn lighten(body: &Body, face_id: usize, options: &LightenOptions) -> Result<Body> {
    body.require_brep("lighten")?;

    if !options.wall_offset.is_finite() || options.wall_offset <= 1e-6 {
        return Err(KernelError::Invalid("wall_offset must be a positive finite number".into()));
    }
    if !options.pocket_depth.is_finite() || options.pocket_depth <= 1e-6 {
        return Err(KernelError::Invalid("pocket_depth must be a positive finite number".into()));
    }
    if !options.draft_angle.is_finite() || options.draft_angle < 0.0 || options.draft_angle >= FRAC_PI_2 - 1e-3 {
        return Err(KernelError::Invalid("draft_angle must be between 0 and 90 degrees".into()));
    }
    if !options.corner_radius.is_finite() || options.corner_radius < 0.0 {
        return Err(KernelError::Invalid("corner_radius must be non-negative and finite".into()));
    }
    if options.auto_fillet && (!options.fillet_radius.is_finite() || options.fillet_radius <= 1e-6) {
        return Err(KernelError::Invalid("fillet_radius must be a positive finite number".into()));
    }
    if options.auto_chamfer && (!options.chamfer_size.is_finite() || options.chamfer_size <= 1e-6) {
        return Err(KernelError::Invalid("chamfer_size must be a positive finite number".into()));
    }

    if face_id >= body.face_count() {
        return Err(KernelError::Invalid(format!("face {face_id} out of bounds")));
    }

    let plane = body
        .face_plane(face_id)
        .or_else(|| body.planar_face(face_id))
        .ok_or_else(|| KernelError::Invalid("lighten requires a planar face".into()))?;

    // Step a & b: Extract wires and project to 2D
    let loops = extract_face_loops(body, face_id, &plane)?;
    if loops.is_empty() {
        return Err(KernelError::Invalid("selected face has no boundary loops".into()));
    }

    // Step a: Classify outer boundary loop, inner hub loop (if present), and interior hole loops.
    // The loop with the largest absolute area is the outer boundary.
    let (outer_idx, _) = loops
        .iter()
        .enumerate()
        .map(|(i, lp)| (i, lp.signed_area().abs()))
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .ok_or_else(|| KernelError::Invalid("no boundary loops found".into()))?;

    let raw_outer = loops.get(outer_idx).ok_or_else(|| KernelError::Invalid("outer loop not found".into()))?;
    let outer_loop = if raw_outer.is_ccw() { raw_outer.clone() } else { raw_outer.reversed() };
    let outer_centroid = outer_loop.centroid();
    let outer_area = outer_loop.signed_area();
    let outer_r_est = (outer_area / PI).max(1e-6).sqrt();

    let mut interior_loops = Vec::new();
    for (i, lp) in loops.iter().enumerate() {
        if i == outer_idx {
            continue;
        }
        let ccw = if lp.is_ccw() { lp.clone() } else { lp.reversed() };
        interior_loops.push(ccw);
    }

    // Check for inner hub loop: an interior loop centered near the outer centroid.
    let mut hub_loop: Option<Loop2> = None;
    let mut hub_centroid = outer_centroid;
    let mut hole_loops: Vec<Loop2> = Vec::new();

    // Find the interior loop nearest to outer_centroid if within 0.15 * R_outer
    let mut hub_cand_idx: Option<usize> = None;
    let mut max_hub_area = 0.0;
    for (i, lp) in interior_loops.iter().enumerate() {
        let c = lp.centroid();
        let dist = c.dist(outer_centroid);
        if dist <= 0.15 * outer_r_est && lp.signed_area() > max_hub_area {
            max_hub_area = lp.signed_area();
            hub_cand_idx = Some(i);
        }
    }

    for (i, lp) in interior_loops.into_iter().enumerate() {
        if Some(i) == hub_cand_idx {
            hub_centroid = lp.centroid();
            hub_loop = Some(lp);
        } else {
            hole_loops.push(lp);
        }
    }

    // Step c: Offset the outer boundary loop inward by wall_offset
    let inset_outer = offset_outer_loop_inward(&outer_loop, options.wall_offset)?;

    // Step d: Offset hole loops and the hub loop outward by wall_offset
    let hub_outset = match &hub_loop {
        Some(hl) => Some(offset_interior_loop_outward(hl, options.wall_offset)?),
        None => None,
    };

    let mut hole_outsets = Vec::new();
    for hl in &hole_loops {
        hole_outsets.push(offset_interior_loop_outward(hl, options.wall_offset)?);
    }

    // Step e: For each hole center, construct radial web spoke bands that connect the hub
    // to the hole boss and the hole boss to the outer rim.
    // The spoke half-width bridges the hole without cutting gaps through the mounting spoke,
    // while allowing the hole boss (radius hole_r + wall_offset) to bulge into adjacent pockets.
    let mut web_loops = Vec::new();
    let reach_len = outer_r_est * 2.0 + options.wall_offset * 4.0 + 10.0;

    for hl in &hole_loops {
        let h_center = hl.centroid();
        let d = h_center - hub_centroid;
        let len = d.len();
        if len < 1e-4 {
            continue;
        }
        let u = d / len;
        let n = Vec2::new(-u.y, u.x);

        let hole_r = (hl.signed_area().abs() / PI).sqrt().max(0.5);
        let spoke_half_w = hole_r.max(0.5 * options.wall_offset);

        let p1 = hub_centroid - n * spoke_half_w;
        let p2 = hub_centroid + u * reach_len - n * spoke_half_w;
        let p3 = hub_centroid + u * reach_len + n * spoke_half_w;
        let p4 = hub_centroid + n * spoke_half_w;

        web_loops.push(Loop2::polygon(&[p1, p2, p3, p4]));
    }

    // Step f: Union the hole offsets, hub offset, and web bands to form a single solid keep-out region.
    let mut keep_out: Vec<Region2> = Vec::new();
    if let Some(hub_out) = hub_outset {
        keep_out.push(Region2 { outer: hub_out, holes: vec![] });
    }
    for ho in hole_outsets {
        let r = Region2 { outer: ho, holes: vec![] };
        keep_out = if keep_out.is_empty() {
            vec![r]
        } else {
            planar_regions_boolean(&keep_out, &[r], BoolOp::Union, 1e-4)?
        };
    }
    for wb in web_loops {
        let r = Region2 { outer: wb, holes: vec![] };
        keep_out = if keep_out.is_empty() {
            vec![r]
        } else {
            planar_regions_boolean(&keep_out, &[r], BoolOp::Union, 1e-4)?
        };
    }

    // Try building exact tangent disc pockets first (smooth tangent geometry)
    let tangent_pockets = try_build_tangent_disc_pockets(&outer_loop, hub_loop.as_ref(), hub_centroid, &hole_loops, options);
    let pockets = if let Some(pkts) = tangent_pockets {
        pkts
    } else {
        // Step g: Subtract the keep-out region from the inset outer loop to form discrete pocket profiles.
        let base_region = Region2 { outer: inset_outer, holes: vec![] };
        let mut pkts = if keep_out.is_empty() {
            vec![base_region]
        } else {
            planar_regions_boolean(&[base_region], &keep_out, BoolOp::Cut, 1e-4)?
        };

        // Filter out degenerate pockets
        pkts.retain(|p| p.area() > 1e-3);
        if pkts.is_empty() {
            return Err(KernelError::Invalid("lighten parameters collapsed pocket profiles".into()));
        }

        // Step h: Insert 2D tangent arc fillets of radius corner_radius at all concave corners of pocket profiles.
        if options.corner_radius > 1e-4 {
            for p in &mut pkts {
                p.outer = fillet_pocket_corners(&p.outer, options.corner_radius);
            }
        }
        pkts
    };

    // Step i: Extrude the pocket loops anti-normal into the part by pocket_depth
    // (using extrude_tapered if draft_angle > 0 or extrude).
    // The tool extends slightly outside the planar face into air to prevent coincident-face
    // singularities during boolean subtraction while preserving exact pocket depth.
    let air_extension = (options.pocket_depth * 0.5).min(1.0).max(0.1);
    let mut current_body = body.clone();

    if options.draft_angle > 1e-5 {
        for p in &pockets {
            let tool = match crate::build::extrude_tapered_between(&plane, p, -air_extension, options.pocket_depth, 0.0, -1.0, -options.draft_angle) {
                Ok(t) => t,
                Err(_) => {
                    let tools = crate::build::extrude(&plane, &[p.clone()], -options.pocket_depth, air_extension)?;
                    tools.into_iter().next().ok_or_else(|| KernelError::Failed("empty extrude".into()))?
                }
            };
            current_body = crate::boolean(&current_body, &tool, BoolOp::Cut)?
                .ok_or_else(|| KernelError::Failed("lighten pocket cut removed the entire body".into()))?;
        }
    } else {
        let tools = crate::build::extrude(&plane, &pockets, -options.pocket_depth, air_extension)?;
        for tool in &tools {
            current_body = crate::boolean(&current_body, tool, BoolOp::Cut)?
                .ok_or_else(|| KernelError::Failed("lighten pocket cut removed the entire body".into()))?;
        }
    }

    // Step j & k: Automatic 3D Fillet & Chamfer on detected pocket edges (Top and/or Bottom).
    // Pre-existing external edges from the original body are strictly protected.
    let do_fillet = (options.auto_fillet && options.fillet_radius > 1e-4 && (options.fillet_top || options.fillet_bottom))
        || (options.floor_radius > 1e-4);
    let fillet_r = if options.auto_fillet && options.fillet_radius > 1e-4 {
        options.fillet_radius
    } else {
        options.floor_radius
    };
    let fillet_on_top = options.auto_fillet && options.fillet_top;
    let fillet_on_bottom = (options.auto_fillet && options.fillet_bottom) || (options.floor_radius > 1e-4);

    let do_chamfer = options.auto_chamfer && options.chamfer_size > 1e-4 && (options.chamfer_top || options.chamfer_bottom);
    let chamfer_sz = options.chamfer_size;
    let chamfer_on_top = options.auto_chamfer && options.chamfer_top;
    let chamfer_on_bottom = options.auto_chamfer && options.chamfer_bottom;

    if do_fillet || do_chamfer {
        let orig_edge_mids: Vec<Vec3> = Body::unique_edges(&body.solid)
            .into_iter()
            .map(|e| {
                let c = e.curve();
                let (t0, t1) = c.range_tuple();
                from_p3(c.subs(0.5 * (t0 + t1)))
            })
            .collect();

        let top_plane = plane.clone();
        let floor_plane = plane.offset(-options.pocket_depth);

        let mut top_points = Vec::new();
        let mut floor_points = Vec::new();

        for e in Body::unique_edges(&current_body.solid) {
            let c = e.curve();
            let (t0, t1) = c.range_tuple();
            let p_start = from_p3(c.subs(t0));
            let p_mid = from_p3(c.subs(0.5 * (t0 + t1)));
            let p_end = from_p3(c.subs(t1));

            // Must NOT be a pre-existing edge from original body
            let is_preexisting = orig_edge_mids.iter().any(|&m| m.dist(p_mid) < 1e-3);
            if is_preexisting {
                continue;
            }

            // Must lie on floor plane or top plane
            let is_floor = floor_plane.height(p_start).abs() < 1e-3
                && floor_plane.height(p_mid).abs() < 1e-3
                && floor_plane.height(p_end).abs() < 1e-3;

            let is_top = top_plane.height(p_start).abs() < 1e-3
                && top_plane.height(p_mid).abs() < 1e-3
                && top_plane.height(p_end).abs() < 1e-3;

            if !is_floor && !is_top {
                continue;
            }

            if is_floor {
                floor_points.push(p_mid);
            } else if is_top {
                top_points.push(p_mid);
            }
        }

        // Mutual-exclusion: ensure fillet and chamfer don't fight on the same edges
        let mut fillet_targets = Vec::new();
        let mut chamfer_targets = Vec::new();

        if do_fillet {
            if fillet_on_top {
                fillet_targets.extend(top_points.iter().copied());
            }
            if fillet_on_bottom {
                fillet_targets.extend(floor_points.iter().copied());
            }
        }

        if do_chamfer {
            if chamfer_on_top && !fillet_on_top {
                chamfer_targets.extend(top_points.iter().copied());
            }
            if chamfer_on_bottom && !fillet_on_bottom {
                chamfer_targets.extend(floor_points.iter().copied());
            }
        }

        if !fillet_targets.is_empty() {
            if let Ok(b) = crate::fillet(&current_body, &fillet_targets, fillet_r) {
                current_body = b;
            }
        }

        if !chamfer_targets.is_empty() {
            if let Ok(b) = crate::chamfer(&current_body, &chamfer_targets, chamfer_sz) {
                current_body = b;
            }
        }
    }

    Ok(current_body)
}

/// Extract planar loops from a face's boundaries.
fn extract_face_loops(body: &Body, face_id: usize, plane: &Plane) -> Result<Vec<Loop2>> {
    crate::guard("extract face loops", || {
        let face = body
            .solid
            .face_iter()
            .nth(face_id)
            .ok_or_else(|| KernelError::Invalid(format!("face {face_id} out of bounds")))?;

        let mut loops = Vec::new();
        for wire in face.absolute_boundaries() {
            let mut segs: Vec<Seg2> = Vec::new();
            for edge in wire.edge_iter() {
                let c = edge.oriented_curve();
                let (t0, t1) = c.range_tuple();
                let pts: Vec<Vec2> = (0..=16)
                    .map(|k| {
                        let t = t0 + (t1 - t0) * (k as f64) / 16.0;
                        plane.to_local(from_p3(c.subs(t)))
                    })
                    .collect();

                let a = *pts.first().ok_or_else(|| KernelError::Invalid("empty edge".into()))?;
                let b = *pts.last().ok_or_else(|| KernelError::Invalid("empty edge".into()))?;
                let m = *pts.get(8).ok_or_else(|| KernelError::Invalid("empty edge".into()))?;

                let closed = a.dist(b) <= 1e-4;
                if closed {
                    // Full circular edge
                    let p1 = a;
                    let p2 = *pts.get(5).unwrap_or(&a);
                    let p3 = *pts.get(11).unwrap_or(&b);
                    let (u, v) = (p2 - p1, p3 - p1);
                    let den = 2.0 * u.cross(v);
                    if den.abs() > 1e-12 {
                        let center = p1 + Vec2::new(v.y * u.len2() - u.y * v.len2(), u.x * v.len2() - v.x * u.len2()) * (1.0 / den);
                        let radius = center.dist(p1);
                        segs.push(Seg2::Arc { center, radius, start: 0.0, sweep: PI });
                        segs.push(Seg2::Arc { center, radius, start: PI, sweep: PI });
                        continue;
                    }
                }

                let is_line = pts.iter().all(|p| point_segment_dist(*p, a, b) <= 1e-3);
                if is_line {
                    if a.dist(b) > 1e-6 {
                        segs.push(Seg2::Line { a, b });
                    }
                } else {
                    let (u, v) = (m - a, b - a);
                    let den = 2.0 * u.cross(v);
                    if den.abs() > 1e-12 {
                        let center = a + Vec2::new(v.y * u.len2() - u.y * v.len2(), u.x * v.len2() - v.x * u.len2()) * (1.0 / den);
                        let radius = center.dist(a);
                        let a0 = (a - center).angle();
                        let a1 = (b - center).angle();
                        let am = (m - center).angle();

                        let mut sweep_ccw = a1 - a0;
                        while sweep_ccw < 0.0 {
                            sweep_ccw += TAU;
                        }
                        let mut m_offset = am - a0;
                        while m_offset < 0.0 {
                            m_offset += TAU;
                        }
                        let sweep = if m_offset < sweep_ccw { sweep_ccw } else { sweep_ccw - TAU };

                        segs.push(Seg2::Arc { center, radius, start: a0, sweep });
                    } else if a.dist(b) > 1e-6 {
                        segs.push(Seg2::Line { a, b });
                    }
                }
            }

            if segs.is_empty() {
                continue;
            }

            // Snap adjacent endpoints within tolerance
            let n = segs.len();
            for i in 0..n {
                let next_i = (i + 1) % n;
                let p_end = segs[i].end();
                let p_start = segs[next_i].start();
                if p_end.dist(p_start) <= 1e-3 {
                    match &mut segs[next_i] {
                        Seg2::Line { a, .. } => *a = p_end,
                        _ => {}
                    }
                }
            }

            loops.push(Loop2 { segs });
        }

        Ok(loops)
    })
}

/// Distance from point to line segment.
fn point_segment_dist(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let ab = b - a;
    let l2 = ab.len2();
    if l2 < 1e-12 {
        return p.dist(a);
    }
    let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
    p.dist(a + ab * t)
}

/// Check if a loop is a circle within tolerance.
fn circle_of_loop(lp: &Loop2) -> Option<(Vec2, f64)> {
    if !lp.segs.is_empty() && lp.segs.iter().all(|s| matches!(s, Seg2::Arc { .. })) {
        if let Seg2::Arc { center: c0, radius: r0, .. } = lp.segs[0] {
            let all_same = lp.segs.iter().all(|s| match s {
                Seg2::Arc { center, radius, .. } => center.dist(c0) < 1e-3 && (radius - r0).abs() < 1e-3,
                _ => false,
            });
            if all_same && r0 > 1e-4 {
                let total_sweep: f64 = lp.segs.iter().map(|s| match s {
                    Seg2::Arc { sweep, .. } => sweep.abs(),
                    _ => 0.0,
                }).sum();
                if (total_sweep - TAU).abs() < 1e-2 {
                    return Some((c0, r0));
                }
            }
        }
    }

    let poly = lp.polyline(1e-3);
    if poly.len() < 3 {
        return None;
    }
    let c = lp.centroid();
    let r0 = poly.first()?.dist(c);
    if r0 < 1e-4 {
        return None;
    }
    let is_circle = poly.iter().all(|p| (p.dist(c) - r0).abs() < 0.03 * (1.0 + r0));
    if is_circle {
        let avg_r = poly.iter().map(|p| p.dist(c)).sum::<f64>() / poly.len() as f64;
        Some((c, avg_r))
    } else {
        None
    }
}

/// Inset outer boundary loop by offset distance.
fn offset_outer_loop_inward(lp: &Loop2, offset: f64) -> Result<Loop2> {
    if let Some((center, radius)) = circle_of_loop(lp) {
        let r = radius - offset;
        if r <= 1e-4 {
            return Err(KernelError::Invalid("wall_offset collapses outer boundary".into()));
        }
        return Ok(Loop2::circle(center, r));
    }
    offset_general_loop(lp, -offset)
}

/// Outset interior hole/hub loop by offset distance.
fn offset_interior_loop_outward(lp: &Loop2, offset: f64) -> Result<Loop2> {
    if let Some((center, radius)) = circle_of_loop(lp) {
        let r = radius + offset;
        return Ok(Loop2::circle(center, r));
    }
    offset_general_loop(lp, offset)
}

/// Offset a general polygon/arc loop.
fn offset_general_loop(lp: &Loop2, offset: f64) -> Result<Loop2> {
    let n = lp.segs.len();
    if n == 0 {
        return Err(KernelError::Invalid("empty loop".into()));
    }
    let mut moved: Vec<Seg2> = Vec::with_capacity(n);
    for s in &lp.segs {
        match *s {
            Seg2::Line { a, b } => {
                let dir = (b - a).normalized().unwrap_or(Vec2::X);
                // Inward normal for CCW loop is (-dir.y, dir.x). Outward is (dir.y, -dir.x).
                // Offset sign: positive = outward, negative = inward.
                let normal = Vec2::new(dir.y, -dir.x);
                let shift = normal * offset;
                moved.push(Seg2::Line { a: a + shift, b: b + shift });
            }
            Seg2::Arc { center, radius, start, sweep } => {
                let sign = sweep.signum();
                let r = radius + sign * offset;
                if r <= 1e-4 {
                    return Err(KernelError::Invalid("offset collapses arc".into()));
                }
                moved.push(Seg2::Arc { center, radius: r, start, sweep });
            }
            _ => return Err(KernelError::Failed("unsupported curve type in lightening loop".into())),
        }
    }

    // Intersect adjacent moved segments to ensure closedness
    let mut out_segs = Vec::with_capacity(n);
    for i in 0..n {
        let next_i = (i + 1) % n;
        let s1 = moved.get(i).copied().ok_or_else(|| KernelError::Failed("offset indexing".into()))?;
        let s2 = moved.get(next_i).copied().ok_or_else(|| KernelError::Failed("offset indexing".into()))?;

        let joint = match (s1, s2) {
            (Seg2::Line { a: a1, b: b1 }, Seg2::Line { a: a2, b: b2 }) => {
                let d1 = b1 - a1;
                let d2 = b2 - a2;
                let cross = d1.cross(d2);
                if cross.abs() > 1e-8 {
                    let t = (a2 - a1).cross(d2) / cross;
                    a1 + d1 * t
                } else {
                    (b1 + a2) * 0.5
                }
            }
            _ => (s1.end() + s2.start()) * 0.5,
        };

        match s1 {
            Seg2::Line { a, .. } => {
                out_segs.push(Seg2::Line { a, b: joint });
            }
            Seg2::Arc { center, radius, start, sweep } => {
                let a0 = start;
                let a1 = (joint - center).angle();
                let mut sw = a1 - a0;
                while sw * sweep < 0.0 {
                    sw += sweep.signum() * TAU;
                }
                out_segs.push(Seg2::Arc { center, radius, start: a0, sweep: sw });
            }
            _ => {}
        }
    }

    // Snap start of next to end of current
    for i in 0..n {
        let next_i = (i + 1) % n;
        let p_end = out_segs[i].end();
        match &mut out_segs[next_i] {
            Seg2::Line { a, .. } => *a = p_end,
            Seg2::Arc { center, radius: _, start, sweep } => {
                let a0 = (p_end - *center).angle();
                let a1 = *start + *sweep;
                let mut sw = a1 - a0;
                while sw * sweep.signum() < 0.0 {
                    sw += sweep.signum() * TAU;
                }
                *start = a0;
                *sweep = sw;
            }
            _ => {}
        }
    }

    Ok(Loop2 { segs: out_segs })
}

/// Insert 2D tangent arc fillets at concave (inner) corners of a pocket loop.
struct FilletCut {
    p1: Vec2,
    p2: Vec2,
    arc: Seg2,
}

fn solve_corner_fillet(s1: &Seg2, s2: &Seg2, radius: f64) -> Option<FilletCut> {
    let t1 = s1.end_tangent();
    let t2 = s2.start_tangent();

    // Concave corner in CCW loop turns left: t1 x t2 > 0
    let sin_theta = t1.cross(t2);
    let cos_theta = t1.dot(t2);
    if sin_theta <= 1e-3 {
        return None;
    }

    let tan_half = (1.0 - cos_theta) / sin_theta;
    if tan_half <= 1e-4 {
        return None;
    }

    let l1 = s1.length();
    let l2 = s2.length();
    let max_setback = 0.40 * l1.min(l2);
    let target_setback = radius * tan_half;
    let setback = target_setback.min(max_setback);
    let eff_r = setback / tan_half;
    if eff_r <= 1e-3 || setback <= 1e-4 {
        return None;
    }

    let v = (s1.end() + s2.start()) * 0.5;
    let n1 = Vec2::new(-t1.y, t1.x);
    let n2 = Vec2::new(-t2.y, t2.x);

    let (c_fillet, p1, p2) = match (*s1, *s2) {
        (Seg2::Line { .. }, Seg2::Line { .. }) => {
            let b = (n1 + n2).normalized()?;
            let cos_half = b.dot(n1);
            if cos_half.abs() < 1e-4 {
                return None;
            }
            let cf = v + b * (eff_r / cos_half);
            let p1 = cf - n1 * eff_r;
            let p2 = cf - n2 * eff_r;
            (cf, p1, p2)
        }
        (Seg2::Arc { center: c1, radius: r1, sweep: sw1, .. }, Seg2::Line { .. }) => {
            let d1 = if sw1 > 0.0 { r1 - eff_r } else { r1 + eff_r };
            if d1 <= 1e-4 {
                return None;
            }
            let a_line = v + n2 * eff_r;
            let w = a_line - c1;
            let b_term = w.dot(t2);
            let c_term = w.len2() - d1 * d1;
            let disc = b_term * b_term - c_term;
            if disc < 0.0 {
                return None;
            }
            let sqrt_disc = disc.sqrt();
            let tau1 = -b_term + sqrt_disc;
            let tau2 = -b_term - sqrt_disc;
            let cf1 = a_line + t2 * tau1;
            let cf2 = a_line + t2 * tau2;
            let cf = if cf1.dist(v) < cf2.dist(v) { cf1 } else { cf2 };
            let p2 = cf - n2 * eff_r;
            let p1 = c1 + (cf - c1).normalized()? * r1;
            (cf, p1, p2)
        }
        (Seg2::Line { .. }, Seg2::Arc { center: c2, radius: r2, sweep: sw2, .. }) => {
            let d2 = if sw2 > 0.0 { r2 - eff_r } else { r2 + eff_r };
            if d2 <= 1e-4 {
                return None;
            }
            let a_line = v + n1 * eff_r;
            let w = a_line - c2;
            let b_term = w.dot(t1);
            let c_term = w.len2() - d2 * d2;
            let disc = b_term * b_term - c_term;
            if disc < 0.0 {
                return None;
            }
            let sqrt_disc = disc.sqrt();
            let tau1 = -b_term + sqrt_disc;
            let tau2 = -b_term - sqrt_disc;
            let cf1 = a_line + t1 * tau1;
            let cf2 = a_line + t1 * tau2;
            let cf = if cf1.dist(v) < cf2.dist(v) { cf1 } else { cf2 };
            let p1 = cf - n1 * eff_r;
            let p2 = c2 + (cf - c2).normalized()? * r2;
            (cf, p1, p2)
        }
        (Seg2::Arc { center: c1, radius: r1, sweep: sw1, .. }, Seg2::Arc { center: c2, radius: r2, sweep: sw2, .. }) => {
            let d1 = if sw1 > 0.0 { r1 - eff_r } else { r1 + eff_r };
            let d2 = if sw2 > 0.0 { r2 - eff_r } else { r2 + eff_r };
            if d1 <= 1e-4 || d2 <= 1e-4 {
                return None;
            }
            let d_cc = c1.dist(c2);
            if d_cc > d1 + d2 || d_cc < (d1 - d2).abs() || d_cc < 1e-6 {
                return None;
            }
            let a = (d1 * d1 - d2 * d2 + d_cc * d_cc) / (2.0 * d_cc);
            let h = (d1 * d1 - a * a).max(0.0).sqrt();
            let p_mid = c1 + (c2 - c1) * (a / d_cc);
            let perp = Vec2::new(-(c2.y - c1.y), c2.x - c1.x) * (h / d_cc);
            let cf1 = p_mid + perp;
            let cf2 = p_mid - perp;
            let cf = if cf1.dist(v) < cf2.dist(v) { cf1 } else { cf2 };
            let p1 = c1 + (cf - c1).normalized()? * r1;
            let p2 = c2 + (cf - c2).normalized()? * r2;
            (cf, p1, p2)
        }
        _ => return None,
    };

    let a1 = (p1 - c_fillet).angle();
    let a2 = (p2 - c_fillet).angle();
    let mut sw = a2 - a1;
    while sw < 0.0 {
        sw += TAU;
    }
    while sw >= TAU {
        sw -= TAU;
    }
    // A fillet arc at a corner between adjacent meeting edges must sweep < PI
    if sw >= PI - 1e-4 || sw <= 1e-4 {
        return None;
    }

    Some(FilletCut {
        p1,
        p2,
        arc: Seg2::Arc {
            center: c_fillet,
            radius: eff_r,
            start: a1,
            sweep: sw,
        },
    })
}

fn trim_seg_end(s: &Seg2, p_end: Vec2) -> Seg2 {
    match *s {
        Seg2::Line { a, .. } => Seg2::Line { a, b: p_end },
        Seg2::Arc { center, start, sweep, radius } => {
            let a_end = (p_end - center).angle();
            let mut sw = a_end - start;
            while sw * sweep.signum() < 0.0 {
                sw += sweep.signum() * TAU;
            }
            if sw.abs() > sweep.abs() {
                sw = sweep;
            }
            Seg2::Arc { center, radius, start, sweep: sw }
        }
        other => other,
    }
}

fn trim_seg_start(s: &Seg2, p_start: Vec2) -> Seg2 {
    match *s {
        Seg2::Line { b, .. } => Seg2::Line { a: p_start, b },
        Seg2::Arc { center, start, sweep, radius } => {
            let a_end = start + sweep;
            let a_start = (p_start - center).angle();
            let mut sw = a_end - a_start;
            while sw * sweep.signum() < 0.0 {
                sw += sweep.signum() * TAU;
            }
            if sw.abs() > sweep.abs() {
                sw = sweep;
            }
            Seg2::Arc { center, radius, start: a_start, sweep: sw }
        }
        other => other,
    }
}

/// Insert 2D tangent arc fillets at concave (inner) corners of a pocket loop.
fn fillet_pocket_corners(lp: &Loop2, radius: f64) -> Loop2 {
    let n = lp.segs.len();
    if n < 3 || radius <= 1e-4 {
        return lp.clone();
    }

    let mut cuts: Vec<Option<FilletCut>> = Vec::with_capacity(n);
    for i in 0..n {
        let next_i = (i + 1) % n;
        cuts.push(solve_corner_fillet(&lp.segs[i], &lp.segs[next_i], radius));
    }

    let mut result_segs = Vec::new();
    for i in 0..n {
        let prev_i = (i + n - 1) % n;
        let mut s = lp.segs[i];
        if let Some(prev_cut) = &cuts[prev_i] {
            s = trim_seg_start(&s, prev_cut.p2);
        }
        if let Some(this_cut) = &cuts[i] {
            s = trim_seg_end(&s, this_cut.p1);
        }
        result_segs.push(s);
        if let Some(this_cut) = &cuts[i] {
            result_segs.push(this_cut.arc);
        }
    }

    let res = Loop2 { segs: result_segs };
    if res.is_closed(1e-2) && res.signed_area() > 1e-3 {
        res
    } else {
        lp.clone()
    }
}

/// Helper to construct a circular arc between two points.
fn arc_from_to(p_start: Vec2, p_end: Vec2, center: Vec2, ccw: bool) -> Seg2 {
    let radius = p_start.dist(center);
    let a0 = (p_start - center).angle();
    let a1 = (p_end - center).angle();
    let mut sw = a1 - a0;
    if ccw {
        while sw <= 0.0 {
            sw += TAU;
        }
        while sw > TAU {
            sw -= TAU;
        }
    } else {
        while sw >= 0.0 {
            sw -= TAU;
        }
        while sw < -TAU {
            sw += TAU;
        }
    }
    Seg2::Arc { center, radius, start: a0, sweep: sw }
}

/// Helper to solve the center and contact points of a corner fillet arc tangent to:
/// 1) the outer concentric circle (internally tangent, radius R_out)
/// 2) a hole boss circle (externally tangent, center C, radius R_b)
///
/// On the side of the hole specified by `ccw_side` (+1.0 for CCW / pocket on left, -1.0 for CW / pocket on right).
fn solve_outer_fillet(
    o: Vec2,
    r_out: f64,
    c: Vec2,
    r_b: f64,
    target_rf: f64,
    max_angular_span: f64,
    ccw_side: f64,
) -> Option<(Vec2, Vec2, Vec2, f64)> {
    let d = c.dist(o);
    if d < 1e-4 {
        return None;
    }
    let u = (c - o) / d;
    let v = Vec2::new(-u.y, u.x);

    let gap_out = r_out - d - r_b;
    if gap_out <= 1e-4 {
        return None;
    }
    let rf_min = 0.5 * gap_out;
    let mut rf = target_rf.max(rf_min * 1.25);

    for _ in 0..16 {
        let d1 = r_out - rf;
        let d2 = r_b + rf;
        if d1 > 0.0 && d1 + d2 >= d && (d1 - d2).abs() <= d {
            let cos_alpha = ((d1 * d1 - d * d - d2 * d2) / (2.0 * d * d2)).clamp(-1.0, 1.0);
            let sin_alpha = (1.0 - cos_alpha * cos_alpha).max(0.0).sqrt();

            let f = c + (u * cos_alpha + v * (ccw_side * sin_alpha)) * d2;
            let f_dir = f - o;
            let f_dist = f_dir.len();
            if f_dist > 1e-4 {
                let delta_angle = (f_dir.angle() - (c - o).angle()).abs();
                let mut da = delta_angle;
                while da > PI {
                    da = (TAU - da).abs();
                }
                if da <= max_angular_span || rf <= rf_min * 1.05 {
                    let p_out = o + (f_dir / f_dist) * r_out;
                    let p_boss = c + (f - c).normalized()? * r_b;
                    return Some((f, p_out, p_boss, rf));
                }
            }
        }
        rf = (rf * 0.85).max(rf_min * 1.02);
    }
    None
}

/// Helper to solve the center and contact points of a corner fillet arc tangent to:
/// 1) the inner concentric hub circle (externally tangent, radius R_in)
/// 2) a hole boss circle (externally tangent, center C, radius R_b)
///
/// On the side of the hole specified by `ccw_side` (+1.0 for CCW / pocket on left, -1.0 for CW / pocket on right).
fn solve_inner_fillet(
    o: Vec2,
    r_in: f64,
    c: Vec2,
    r_b: f64,
    target_rf: f64,
    max_angular_span: f64,
    ccw_side: f64,
) -> Option<(Vec2, Vec2, Vec2, f64)> {
    let d = c.dist(o);
    if d < 1e-4 {
        return None;
    }
    let u = (c - o) / d;
    let v = Vec2::new(-u.y, u.x);

    let gap_in = d - r_b - r_in;
    if gap_in <= 1e-4 {
        return None;
    }
    let rf_min = 0.5 * gap_in;
    let mut rf = target_rf.max(rf_min * 1.25);

    for _ in 0..16 {
        let d1 = r_in + rf;
        let d2 = r_b + rf;
        if d1 + d2 >= d && (d1 - d2).abs() <= d {
            let cos_beta = ((d * d + d2 * d2 - d1 * d1) / (2.0 * d * d2)).clamp(-1.0, 1.0);
            let sin_beta = (1.0 - cos_beta * cos_beta).max(0.0).sqrt();

            let f = c + (-u * cos_beta + v * (ccw_side * sin_beta)) * d2;
            let f_dir = f - o;
            let f_dist = f_dir.len();
            if f_dist > 1e-4 {
                let delta_angle = (f_dir.angle() - (c - o).angle()).abs();
                let mut da = delta_angle;
                while da > PI {
                    da = (TAU - da).abs();
                }
                if da <= max_angular_span || rf <= rf_min * 1.05 {
                    let p_in = o + (f_dir / f_dist) * r_in;
                    let p_boss = c + (f - c).normalized()? * r_b;
                    return Some((f, p_in, p_boss, rf));
                }
            }
        }
        rf = (rf * 0.85).max(rf_min * 1.02);
    }
    None
}

/// Build exact 8-arc tangent pocket profiles for circular disc flanges with mounting holes.
/// Every boundary is an exact circular arc meeting tangentially with no sharp corners.
fn try_build_tangent_disc_pockets(
    outer_loop: &Loop2,
    hub_loop: Option<&Loop2>,
    hub_centroid: Vec2,
    hole_loops: &[Loop2],
    options: &LightenOptions,
) -> Option<Vec<Region2>> {
    let (_, r_outer) = circle_of_loop(outer_loop)?;
    if hole_loops.len() < 2 {
        return None;
    }

    let mut hole_circles = Vec::new();
    for hl in hole_loops {
        let (c, r) = circle_of_loop(hl)?;
        let theta = (c - hub_centroid).angle();
        hole_circles.push((theta, c, r));
    }
    hole_circles.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let r_hub = hub_loop.and_then(circle_of_loop).map(|(_, r)| r).unwrap_or(0.0);
    let r_out = r_outer - options.wall_offset;
    let min_hole_dist = hole_circles.iter().map(|(_, c, r)| c.dist(hub_centroid) - r).fold(f64::INFINITY, f64::min);
    let r_in = if r_hub > 1e-4 {
        r_hub + options.wall_offset
    } else {
        (min_hole_dist - options.wall_offset).max(r_outer * 0.25)
    };

    if r_out <= r_in + 1.0 {
        return None;
    }

    let n = hole_circles.len();
    let mut pockets = Vec::new();
    let target_rf = options.corner_radius.max(0.5);

    for i in 0..n {
        let next_i = (i + 1) % n;
        let (theta_a, c_a, r_ha) = hole_circles[i];
        let (theta_b, c_b, r_hb) = hole_circles[next_i];

        let mut span = theta_b - theta_a;
        while span <= 0.0 {
            span += TAU;
        }
        if span > PI + 1e-3 {
            continue;
        }
        let max_angular_span = span * 0.42;

        let r_ba = r_ha + options.wall_offset;
        let r_bb = r_hb + options.wall_offset;

        // Hole A is at theta_a, pocket is on CCW side of Hole A (+1.0)
        let (f_out_a, p_out_a, p_boss_out_a, _) = solve_outer_fillet(
            hub_centroid, r_out, c_a, r_ba, target_rf, max_angular_span, 1.0
        )?;
        let (f_in_a, p_in_a, p_boss_in_a, _) = solve_inner_fillet(
            hub_centroid, r_in, c_a, r_ba, target_rf, max_angular_span, 1.0
        )?;

        // Hole B is at theta_b, pocket is on CW side of Hole B (-1.0)
        let (f_out_b, p_out_b, p_boss_out_b, _) = solve_outer_fillet(
            hub_centroid, r_out, c_b, r_bb, target_rf, max_angular_span, -1.0
        )?;
        let (f_in_b, p_in_b, p_boss_in_b, _) = solve_inner_fillet(
            hub_centroid, r_in, c_b, r_bb, target_rf, max_angular_span, -1.0
        )?;

        // 8 continuous circular arcs forming the pocket loop (CCW):
        // 1. Outer concentric arc: p_out_a -> p_out_b around hub_centroid (CCW)
        let seg1 = arc_from_to(p_out_a, p_out_b, hub_centroid, true);
        // 2. Corner fillet at Hole B outer: p_out_b -> p_boss_out_b around f_out_b (CCW)
        let seg2 = arc_from_to(p_out_b, p_boss_out_b, f_out_b, true);
        // 3. Hole B boss arc: p_boss_out_b -> p_boss_in_b around c_b (CW)
        let seg3 = arc_from_to(p_boss_out_b, p_boss_in_b, c_b, false);
        // 4. Corner fillet at Hole B inner: p_boss_in_b -> p_in_b around f_in_b (CCW)
        let seg4 = arc_from_to(p_boss_in_b, p_in_b, f_in_b, true);
        // 5. Inner concentric arc: p_in_b -> p_in_a around hub_centroid (CW)
        let seg5 = arc_from_to(p_in_b, p_in_a, hub_centroid, false);
        // 6. Corner fillet at Hole A inner: p_in_a -> p_boss_in_a around f_in_a (CCW)
        let seg6 = arc_from_to(p_in_a, p_boss_in_a, f_in_a, true);
        // 7. Hole A boss arc: p_boss_in_a -> p_boss_out_a around c_a (CW)
        let seg7 = arc_from_to(p_boss_in_a, p_boss_out_a, c_a, false);
        // 8. Corner fillet at Hole A outer: p_boss_out_a -> p_out_a around f_out_a (CCW)
        let seg8 = arc_from_to(p_boss_out_a, p_out_a, f_out_a, true);

        let lp = Loop2 { segs: vec![seg1, seg2, seg3, seg4, seg5, seg6, seg7, seg8] };
        if lp.is_closed(1e-3) {
            let outer = if lp.is_ccw() { lp } else { lp.reversed() };
            pockets.push(Region2 { outer, holes: vec![] });
        }
    }

    if pockets.is_empty() { None } else { Some(pockets) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{cylinder, boolean, measure};

    fn make_test_disc() -> (Body, usize) {
        // Outer disc: radius 50 mm, height 10 mm
        let mut disc = cylinder(Vec3::ZERO, Vec3::Z, 50.0, 10.0).unwrap();
        // Central hub hole: radius 10 mm, through
        let hub = cylinder(Vec3::new(0.0, 0.0, -1.0), Vec3::Z, 10.0, 12.0).unwrap();
        disc = boolean(&disc, &hub, BoolOp::Cut).unwrap().unwrap();

        // 4 bolt holes: radius 4 mm, at BCD radius 30 mm
        for k in 0..4 {
            let theta = k as f64 * FRAC_PI_2;
            let pos = Vec3::new(30.0 * theta.cos(), 30.0 * theta.sin(), -1.0);
            let hole = cylinder(pos, Vec3::Z, 4.0, 12.0).unwrap();
            disc = boolean(&disc, &hole, BoolOp::Cut).unwrap().unwrap();
        }

        // Find the top planar face at Z = 10.0
        let top_face_id = (0..disc.face_count())
            .find(|&i| {
                if let Some(plane) = disc.face_plane(i).or_else(|| disc.planar_face(i)) {
                    plane.normal().dot(Vec3::Z) > 0.9 && (plane.origin.z - 10.0).abs() < 1e-3
                } else {
                    false
                }
            })
            .expect("top planar face must exist");

        (disc, top_face_id)
    }

    #[test]
    fn disc_with_bolt_holes_lightening_pockets() {
        let (disc, face_id) = make_test_disc();
        let initial_vol = measure(&disc).unwrap().volume;

        let options = LightenOptions {
            wall_offset: 2.5,
            pocket_depth: 4.0,
            draft_angle: 0.0,
            corner_radius: 1.5,
            floor_radius: 0.5,
            auto_fillet: true,
            fillet_radius: 0.5,
            fillet_bottom: true,
            ..Default::default()
        };

        let lightened = lighten(&disc, face_id, &options).expect("lighten should succeed");
        let lightened_vol = measure(&lightened).unwrap().volume;

        // Material must have been removed
        assert!(lightened_vol < initial_vol);
        let removed = initial_vol - lightened_vol;
        // 4 pockets with significant volume removed
        assert!(removed > 1000.0, "expected significant volume removed, got {removed}");

        // Guard rail check: External outer rim edge (radius 50 at Z=10) and hub rim (radius 10 at Z=10)
        // must remain untouched and present on the lightened body.
        let lightened_edges = Body::unique_edges(&lightened.solid);
        let has_outer_rim = lightened_edges.iter().any(|e| {
            let c = e.curve();
            let (t0, t1) = c.range_tuple();
            let pm = from_p3(c.subs(0.5 * (t0 + t1)));
            (pm.z - 10.0).abs() < 1e-3 && ((pm.x * pm.x + pm.y * pm.y).sqrt() - 50.0).abs() < 1e-3
        });
        let has_hub_rim = lightened_edges.iter().any(|e| {
            let c = e.curve();
            let (t0, t1) = c.range_tuple();
            let pm = from_p3(c.subs(0.5 * (t0 + t1)));
            (pm.z - 10.0).abs() < 1e-3 && ((pm.x * pm.x + pm.y * pm.y).sqrt() - 10.0).abs() < 1e-3
        });

        assert!(has_outer_rim, "pre-existing outer rim must not be modified or filleted");
        assert!(has_hub_rim, "pre-existing hub rim must not be modified or filleted");
    }

    #[test]
    fn lighten_collapsing_offsets_return_error() {
        let (disc, face_id) = make_test_disc();

        // Excessive wall offset will collapse the pocket profiles
        let options = LightenOptions {
            wall_offset: 25.0,
            pocket_depth: 3.0,
            ..Default::default()
        };

        let err = lighten(&disc, face_id, &options).unwrap_err();
        match err {
            KernelError::Invalid(msg) => {
                assert!(msg.contains("collapsed") || msg.contains("collapses"));
            }
            other => panic!("expected KernelError::Invalid, got {other:?}"),
        }
    }

    #[test]
    fn lighten_with_draft_angle() {
        let (disc, face_id) = make_test_disc();
        let initial_vol = measure(&disc).unwrap().volume;

        let options = LightenOptions {
            wall_offset: 3.0,
            pocket_depth: 3.0,
            draft_angle: 0.05, // ~2.86 degrees
            corner_radius: 1.0,
            ..Default::default()
        };

        let lightened = lighten(&disc, face_id, &options).expect("tapered lighten should succeed");
        let lightened_vol = measure(&lightened).unwrap().volume;
        assert!(lightened_vol < initial_vol);
    }

    #[test]
    fn disc_with_6_bolt_holes_tangent_pockets() {
        // Outer disc: radius 50 mm, height 10 mm
        let mut disc = cylinder(Vec3::ZERO, Vec3::Z, 50.0, 10.0).unwrap();
        // Central hub hole: radius 10 mm, through
        let hub = cylinder(Vec3::new(0.0, 0.0, -1.0), Vec3::Z, 10.0, 12.0).unwrap();
        disc = boolean(&disc, &hub, BoolOp::Cut).unwrap().unwrap();

        // 6 bolt holes: radius 4 mm, at BCD radius 30 mm
        for k in 0..6 {
            let theta = k as f64 * PI / 3.0;
            let pos = Vec3::new(30.0 * theta.cos(), 30.0 * theta.sin(), -1.0);
            let hole = cylinder(pos, Vec3::Z, 4.0, 12.0).unwrap();
            disc = boolean(&disc, &hole, BoolOp::Cut).unwrap().unwrap();
        }

        let top_face_id = (0..disc.face_count())
            .find(|&i| {
                if let Some(plane) = disc.face_plane(i).or_else(|| disc.planar_face(i)) {
                    plane.normal().dot(Vec3::Z) > 0.9 && (plane.origin.z - 10.0).abs() < 1e-3
                } else {
                    false
                }
            })
            .expect("top planar face must exist");

        let plane = disc.face_plane(top_face_id).unwrap();
        let loops = extract_face_loops(&disc, top_face_id, &plane).unwrap();
        let (outer_idx, _) = loops.iter().enumerate().max_by(|a, b| a.1.signed_area().abs().total_cmp(&b.1.signed_area().abs())).unwrap();
        let outer_loop = loops[outer_idx].clone();
        let mut holes = Vec::new();
        let mut hub_lp = None;
        for (i, lp) in loops.iter().enumerate() {
            if i == outer_idx { continue; }
            if lp.centroid().dist(outer_loop.centroid()) < 2.0 {
                hub_lp = Some(lp.clone());
            } else {
                holes.push(lp.clone());
            }
        }
        assert_eq!(holes.len(), 6);

        let options = LightenOptions {
            wall_offset: 2.0,
            pocket_depth: 3.0,
            draft_angle: 0.0,
            corner_radius: 1.0,
            ..Default::default()
        };

        let pockets = try_build_tangent_disc_pockets(&outer_loop, hub_lp.as_ref(), outer_loop.centroid(), &holes, &options)
            .expect("tangent pockets must succeed for 6 bolt holes");
        assert_eq!(pockets.len(), 6);
        for p in &pockets {
            assert_eq!(p.outer.segs.len(), 8, "each tangent pocket must have 8 segments");
            for s in &p.outer.segs {
                assert!(matches!(s, Seg2::Arc { .. }), "every segment must be an exact circular arc");
            }
            // Check tangency at each joint: end tangent of seg i matches start tangent of seg (i+1)
            let n = p.outer.segs.len();
            for i in 0..n {
                let next_i = (i + 1) % n;
                let t_end = p.outer.segs[i].end_tangent();
                let t_start = p.outer.segs[next_i].start_tangent();
                let dot = t_end.dot(t_start);
                assert!(dot > 0.99, "adjacent segments must be tangent: dot = {dot}");
            }
        }

        let lightened = lighten(&disc, top_face_id, &options).expect("lighten with 6 holes should succeed");
        let initial_vol = measure(&disc).unwrap().volume;
        let lightened_vol = measure(&lightened).unwrap().volume;
        assert!(lightened_vol < initial_vol);
    }
}

