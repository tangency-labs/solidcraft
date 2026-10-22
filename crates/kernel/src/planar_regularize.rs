//! Exact topological removal of construction-induced planar subdivisions.
//! Shared edge identity and complete construction lineage select the edges; SMLib joins the
//! faces either side of them where they are topological (`solidcraft_smlib::Brep::merge_faces`,
//! every other edge kept by a point at its middle).
use super::Body;
use crate::{KernelError, Result, guard};
use solidcraft_geom::Vec3;
use solidcraft_geom::brep::{self, eval::CurveEval};
use std::sync::Arc;

/// Points along a record edge (ends and three inside).
fn edge_points(e: &brep::Edge) -> Vec<Vec3> {
    let Ok(c) = CurveEval::new(e.parameters()) else { return Vec::new() };
    let [t0, t1] = e.interval;
    (0..=4).filter_map(|k| c.point(t0 + (t1 - t0) * k as f64 / 4.0)).collect()
}

/// The point halfway along a record edge's interval.
fn edge_middle(e: &brep::Edge) -> Option<Vec3> {
    let [t0, t1] = e.interval;
    CurveEval::new(e.parameters()).ok()?.point(0.5 * (t0 + t1))
}

fn failed(e: impl std::fmt::Display) -> KernelError {
    KernelError::Failed(format!("SMLib planar regularization: {e}"))
}

impl Body {
    pub(crate) fn regularize_certified_planar_faces(self) -> Result<Self> {
        if self.regularization_protected {
            return Ok(self);
        }
        let Some(sources) = self.face_sources() else { return Ok(self) };
        if sources.len() > 16_384 || self.paint().is_some_and(|p| !p.faces.is_empty()) {
            return Ok(self);
        }
        let split_points = self.split_keep().iter().try_fold(0usize, |sum, line| sum.checked_add(line.len()));
        let Some(split_points) = split_points.filter(|n| *n <= 65_536) else { return Ok(self) };
        let result = guard("certified planar topology regularization", || {
            if self.face_count() != sources.len() {
                return Ok(None);
            }
            let actual = super::planar_sources::actual_planes(&self)?;
            let valid: Vec<_> = actual
                .iter()
                .zip(sources)
                .map(|(actual, s)| {
                    s.as_ref()
                        .and_then(super::planar_sources::certified_plane)
                        .filter(|p| actual.is_some_and(|actual| super::planar_sources::same_support(p, &actual)))
                })
                .collect();
            let record = self.record();
            let mut shell_of = vec![usize::MAX; record.faces.len()];
            for (k, shell) in record.shells.iter().enumerate() {
                if let brep::Shell::Faces { faces, .. } = shell {
                    for o in faces {
                        if let Some(slot) = shell_of.get_mut(o.face as usize) {
                            *slot = k;
                        }
                    }
                }
            }
            // The edges between two faces of one shell on one certified plane, from one source,
            // off every authored split line.
            let mut eligible = Vec::new();
            let mut identity = 16_000_000usize;
            let mut work = 2_000_000usize;
            for (k, e) in record.edges.iter().enumerate() {
                let [ua, ub] = e.uses.as_slice() else { continue };
                let (a, b) = (ua.face as usize, ub.face as usize);
                if a == b || shell_of.get(a) != shell_of.get(b) {
                    continue;
                }
                let (Some(Some(pa)), Some(Some(pb))) = (valid.get(a), valid.get(b)) else { continue };
                if pa != pb {
                    continue;
                }
                let (Some(Some(sa)), Some(Some(sb))) = (sources.get(a), sources.get(b)) else { continue };
                let Some(cost) = super::planar_sources::descriptor_cost(sa)
                    .and_then(|a| super::planar_sources::descriptor_cost(sb).and_then(|b| a.checked_add(b)))
                else {
                    continue;
                };
                let Some(left) = identity.checked_sub(cost) else { return Ok(None) };
                identity = left;
                if sa != sb {
                    continue;
                }
                let Some(cost) = split_points.checked_mul(6).and_then(|n| n.checked_add(1)) else { return Ok(None) };
                let Some(left) = work.checked_sub(cost) else { return Ok(None) };
                work = left;
                if crate::heal::on_split_line(&edge_points(e), self.split_keep()) {
                    continue;
                }
                eligible.push(k);
            }
            if eligible.is_empty() {
                return Ok(None);
            }
            // SMLib removes the topological edges and vertices but those under the points kept:
            // every edge not chosen (at its middle) stays.
            let keep: Vec<[f64; 3]> = record
                .edges
                .iter()
                .enumerate()
                .filter(|(k, _)| eligible.binary_search(k).is_err())
                .map(|(_, e)| edge_middle(e).map(|p| [p.x, p.y, p.z]))
                .collect::<Option<_>>()
                .ok_or_else(|| failed("an edge of the body has no middle point"))?;
            if keep.len() > solidcraft_smlib::MAX_POINTS {
                return Ok(None);
            }
            let (sb, ob) = crate::native::smlib_body_ordered(&self)?;
            let ob = ob.ok_or_else(|| failed("SMLib numbers the body's edges unlike its record"))?;
            let r = sb.tagged(0).map_err(failed)?.merge_faces(&keep, (self.size() * 1e-6).max(1e-9)).map_err(failed)?;
            if r.counts().map_err(failed)?.first() == Some(&self.face_count()) {
                return Ok(None);
            }
            let check = r.check(solidcraft_smlib::CheckLevel::All).map_err(failed)?;
            if !check.is_valid() {
                return Err(failed(format!("SMLib's check refuses the result: {check}")));
            }
            let (body, or) = crate::native::body_from_smlib_ordered(&r)?;
            let or = or.ok_or_else(|| failed("the result's record is numbered unlike SMLib's body"))?;
            let h = r.history().map_err(failed)?.history(std::slice::from_ref(&ob), &or).map_err(failed)?;
            // Each face keeps the source of the first face it is made of.
            let mut metadata = 2_000_000usize;
            let mut output = Vec::with_capacity(h.faces.len());
            for f in &h.faces {
                let members: Vec<usize> =
                    f.origins.iter().filter_map(|o| if let crate::history::FaceOrigin::Face { face, .. } = o { Some(*face) } else { None }).collect();
                let source = members.iter().min().and_then(|m| sources.get(*m)).cloned().flatten();
                if let Some(s) = &source {
                    let Some(cost) = super::planar_sources::descriptor_cost(s) else { return Ok(None) };
                    let Some(left) = metadata.checked_sub(cost) else { return Ok(None) };
                    metadata = left;
                }
                output.push(source);
            }
            let body = body
                .with_color(self.color())
                .with_paint(self.paint().cloned())
                .with_split_lines(self.split_lines().cloned())
                .with_face_sources(Some(Arc::new(output)))
                .with_checked_history(Some(h), &crate::record::counts(&[&self]));
            // Over the operands of the operation that made `self`, as `self`'s history is.
            let history = crate::boolean_record::through(&body, 0, self.history());
            Ok(Some(Body { history: history.map(Arc::new), ..body }))
        })?;
        Ok(result.unwrap_or(self))
    }
}
