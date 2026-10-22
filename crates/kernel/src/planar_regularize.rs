//! Exact topological removal of construction-induced planar subdivisions.
//! Shared edge identity and complete construction lineage select the edges; SMLib joins the
//! faces either side of them where they are topological (`solidcraft_smlib::Brep::merge_faces`).
use super::Body;
use crate::{KernelError, Result, guard};
use solidcraft_geom::Vec3;
use solidcraft_geom::brep::{self, eval::CurveEval};
use std::sync::Arc;

/// Points along a record edge (ends and three inside).
fn edge_points(e: &brep::Edge) -> Vec<Vec3> {
    let Ok(c) = CurveEval::new(&e.curve) else { return Vec::new() };
    let [t0, t1] = e.interval;
    (0..=4).filter_map(|k| c.point(t0 + (t1 - t0) * k as f64 / 4.0)).collect()
}

fn failed(e: impl std::fmt::Display) -> KernelError {
    KernelError::Failed(format!("SMLib planar regularization: {e}"))
}

impl Body {
    pub(crate) fn regularize_certified_planar_faces(self) -> Result<Self> {
        self.regularize_planar_faces(false)
    }

    /// Only newly produced Cut walls call this path. The temporary planes carry
    /// construction support, never persistent ancestor/source-role authority.
    pub(crate) fn regularize_generated_planar_faces(self) -> Result<Self> {
        let body = self.regularize_planar_faces(true)?;
        let sources = body.face_sources().map(|sources| {
            sources
                .iter()
                .map(|source| {
                    let mut source = source.clone();
                    if let Some(super::FaceSource::ExtrudeSide { plane, coalesced, .. }) = &mut source {
                        if plane.is_some() && !*coalesced {
                            source = None;
                        } else {
                            *plane = None;
                        }
                    }
                    source
                })
                .collect()
        });
        Ok(body.with_face_sources(sources.map(Arc::new)))
    }

    fn regularize_planar_faces(self, generated: bool) -> Result<Self> {
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
                        .and_then(|source| {
                            if generated {
                                match source {
                                    super::FaceSource::ExtrudeSide { segment: solidcraft_geom::Seg2::Line { .. }, plane: Some(p), .. } => Some(*p),
                                    _ => None,
                                }
                            } else {
                                super::planar_sources::certified_plane(source)
                            }
                        })
                        .filter(|p| actual.is_some_and(|actual| super::planar_sources::same_support(p, &actual)))
                })
                .collect();
            let record = crate::native::brep_record(&self)?;
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
            // The edges between two faces of one shell on one certified plane (from one source,
            // unless the faces are generated walls), off every authored split line.
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
                if !generated && sa != sb {
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
            let (sb, ob) = crate::native::smlib_body_ordered(&self)?;
            let ob = ob.ok_or_else(|| failed("SMLib numbers the body's edges unlike its record"))?;
            let at: Vec<usize> = eligible
                .iter()
                .map(|e| if ob.edges.is_empty() { Some(*e) } else { ob.edges.iter().position(|c| c == e) })
                .collect::<Option<_>>()
                .ok_or_else(|| failed("an edge is not in SMLib's body"))?;
            let (r, removed) = sb.tagged(0).map_err(failed)?.merge_faces(&at).map_err(failed)?;
            if removed == 0 {
                return Ok(None);
            }
            let check = r.check(solidcraft_smlib::CheckLevel::All).map_err(failed)?;
            if !check.is_valid() {
                return Err(failed(format!("SMLib's check refuses the result: {check}")));
            }
            let (body, or) = crate::native::body_from_smlib_ordered(&r)?;
            let or = or.ok_or_else(|| failed("the result's record is numbered unlike SMLib's body"))?;
            let h = r.history().map_err(failed)?.history(std::slice::from_ref(&ob), &or).map_err(failed)?;
            // Each face keeps the source of the first face it is made of; a generated wall made
            // of several is coalesced.
            let mut metadata = 2_000_000usize;
            let mut output = Vec::with_capacity(h.faces.len());
            for f in &h.faces {
                let members: Vec<usize> =
                    f.origins.iter().filter_map(|o| if let crate::history::FaceOrigin::Face { face, .. } = o { Some(*face) } else { None }).collect();
                let mut source = members.iter().min().and_then(|m| sources.get(*m)).cloned().flatten();
                if let Some(s) = &source {
                    let Some(cost) = super::planar_sources::descriptor_cost(s) else { return Ok(None) };
                    let Some(left) = metadata.checked_sub(cost) else { return Ok(None) };
                    metadata = left;
                }
                if generated
                    && members.len() > 1
                    && let Some(super::FaceSource::ExtrudeSide { coalesced, .. }) = &mut source
                {
                    *coalesced = true;
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
