//! Exact router cuts and edge picks through the B-rep modelling API.

use std::collections::BTreeSet;

use solidcore_record::Vec3;

use crate::{Body, KernelError, Result};
pub use solidcore_ops::edge_profile::{CornerProfile, CornerSizes};

/// An edge resolved on the input B-rep, independent of display tessellation.
#[derive(Clone, Copy, Debug)]
pub struct RouterEdgePick {
    pub edge: usize,
    pub point: Vec3,
    pub distance: f64,
}

/// Resolve picks on exact edge curves; a sharp edge wins a tie with a seam or smooth join.
/// Duplicate edges are routed once. Tangent continuation uses native B-rep topology.
pub fn routed_edge_picks(body: &Body, points: &[Vec3], chain: bool) -> Result<Option<Vec<RouterEdgePick>>> {
    crate::guard("router edge picks", || {
        if body.is_mesh() || points.is_empty() || points.len() > 256 || points.iter().any(|p| !p.is_finite()) {
            return Err(KernelError::Invalid("select 1 to 256 finite edge points on a B-rep body".into()));
        }
        let (native, order) = solidcore_ops::Body::from_record(body.record(), true).map_err(|e| KernelError::smlib("router stock", e))?;
        let tie = (body.size() * 1e-6).max(1e-6);
        let reach = body.size() * 0.05 + 1e-3;
        let ([lx, ly, lz], [hx, hy, hz]) = native.bounds().map_err(|e| KernelError::smlib("router bounds", e))?;
        let mut selected: Vec<(usize, f64)> = Vec::new();
        for p in points {
            if p.x < lx - reach || p.x > hx + reach || p.y < ly - reach || p.y > hy + reach || p.z < lz - reach || p.z > hz + reach {
                return Ok(None);
            }
            let mut hits = Vec::new();
            for i in 0..body.record().edges.len() {
                let hit = native
                    .edge_closest_point(&order, i, [p.x, p.y, p.z])
                    .map_err(|e| KernelError::smlib("router edge projection", e))?
                    .ok_or_else(|| KernelError::Invalid("the exact edge projection could not be decided".into()))?;
                hits.push((i, hit.distance));
            }
            let nearest =
                hits.iter().min_by(|a, b| a.1.total_cmp(&b.1)).copied().ok_or_else(|| KernelError::Invalid("the body has no edges".into()))?;
            let mut sharp: Option<(usize, f64)> = None;
            for &(i, distance) in &hits {
                if distance > nearest.1 + tie {
                    continue;
                }
                let edge = body.record().edges.get(i).ok_or_else(|| KernelError::Invalid("router edge missing".into()))?;
                let faces: BTreeSet<_> = edge.uses.iter().map(|u| u.face as usize).collect();
                if faces.len() != 2 {
                    continue;
                }
                let mut faces = faces.into_iter();
                let (Some(a), Some(b)) = (faces.next(), faces.next()) else { continue };
                let a = native.face_normal_at_edge(&order, i, a).map_err(|e| KernelError::smlib("router support", e))?;
                let b = native.face_normal_at_edge(&order, i, b).map_err(|e| KernelError::smlib("router support", e))?;
                let [ax, ay, az] = a.value;
                let [bx, by, bz] = b.value;
                let error = a.bound + b.bound + a.bound * b.bound;
                if (ax * bx + ay * by + az * bz).abs() + error < 0.5_f64.to_radians().cos() && sharp.is_none_or(|(_, d)| distance < d) {
                    sharp = Some((i, distance));
                }
            }
            let hit = sharp.unwrap_or(nearest);
            if hit.1 > reach {
                return Ok(None);
            }
            if !selected.iter().any(|(i, _)| *i == hit.0) {
                selected.push(hit);
            }
        }
        let indices: Vec<_> = selected.iter().map(|(i, _)| *i).collect();
        let indices = if chain {
            solidcore_ops::edge_profile::tangent_edges(&native, &order, &indices, 0.5_f64.to_radians())
                .map_err(|e| KernelError::smlib("router tangent chain", e))?
        } else {
            indices
        };
        if indices.len() > 256 {
            return Err(KernelError::Invalid("the router tangent chain exceeds 256 edges".into()));
        }
        let picks = indices
            .into_iter()
            .map(|edge| {
                let [x, y, z] = native.edge_middle(&order, edge).map_err(|e| KernelError::smlib("router edge point", e))?;
                Ok(RouterEdgePick { edge, point: Vec3::new(x, y, z), distance: selected.iter().find(|(i, _)| *i == edge).map_or(0.0, |(_, d)| *d) })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Some(picks))
    })
}

/// A point evaluated on a record edge, for an indexed command pick.
pub fn routed_edge_point(body: &Body, edge: usize) -> Result<Vec3> {
    let (native, order) = solidcore_ops::Body::from_record(body.record(), true).map_err(|e| KernelError::smlib("router stock", e))?;
    let [x, y, z] = native.edge_middle(&order, edge).map_err(|e| KernelError::smlib("router edge point", e))?;
    Ok(Vec3::new(x, y, z))
}

/// Route resolved record edges and report their lengths before the cut.
pub fn edge_profile(body: &Body, edges: &[usize], profile: CornerProfile, sizes: CornerSizes) -> Result<(Body, Vec<f64>)> {
    crate::guard("wood edge profile", || {
        let (native, order) = solidcore_ops::Body::from_record(body.record(), true).map_err(|e| KernelError::smlib("router stock", e))?;
        let (cut, lengths) =
            solidcore_ops::edge_profile::edge_profile(&native, &order, edges, profile, sizes).map_err(|e| KernelError::smlib("router section", e))?;
        let (record, _) = cut.to_record().map_err(|e| KernelError::smlib("router result", e))?;
        Ok((crate::smlib_sweeps::body_from_record(&record, "router edge profile")?, lengths))
    })
}

/// Length of each resolved edge on the unmodified stock, including an identity profile.
pub fn routed_edge_lengths(body: &Body, edges: &[usize]) -> Result<Vec<f64>> {
    let (native, order) = solidcore_ops::Body::from_record(body.record(), true).map_err(|e| KernelError::smlib("router stock", e))?;
    edges.iter().map(|&i| native.edge_length(&order, i, 1e-10).map_err(|e| KernelError::smlib("routed length", e))).collect()
}
