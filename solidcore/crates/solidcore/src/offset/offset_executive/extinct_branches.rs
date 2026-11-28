//! Extinction of convex planar branches in a mitred inward offset.
//!
//! Upstream: none. A branch is a connected component of faces cut off by one inner
//! loop of a planar support. Its straight-edged faces and the attachment polygon
//! bound a convex polyhedron on the support's outward side. If two opposite exposed
//! supporting planes have crossed in the offset, their material halfspaces have
//! empty intersection, or meet only on one plane (inset by exactly half the branch's
//! width), where their common part has no volume. No cavity reaches the branch: its
//! offset faces AND its attachment loop disappear. Removing only the faces would leave
//! a hole.
//!
//! This is a halfspace contradiction certificate, not a winding/area test: reversing
//! two dimensions preserves a loop's orientation. It implements the empty-cell event
//! of the mitred construction (Rossignac and Requicha, 1986, solid offsetting as a set
//! operation; see docs/offset-and-shell.md). It does not approximate ball erosion.
//! Opposite normals must agree within the angular tolerance; the source cell is
//! reconstructed from exact plane intersections, and the crossing is a Farkas certificate on
//! that cell, decided exactly from the actual offset definitions (each sheet's stored origin
//! and axes), so a branch turned by a rotation whose cosines round is decided as it is
//! unturned. An unproved certificate makes no topology decision.
//! General infeasible cells without an opposite-plane certificate remain open work.
//!
//! Deviations: no recursion; component walks visit each source face at most once.

mod cell;

use std::collections::BTreeSet;

use super::p1::{forget_seconds, OffsetExecutive, OffsetOperation};
use super::p2::OffsetNotifications;
use crate::curve::curve::CurveKind;
use crate::error::{Result, SolidError};
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, EFF_ZERO_DEG};
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::query::api_queries::brep_is_manifold_solid;
use crate::surface::plane::Plane;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::brep::p1 as bp1;
use crate::topology::brep::p3::{delete_face_with_body_identity, DeleteFaceOptions};
use crate::topology::edge::{self, NotificationStores, NotifyState};
use crate::topology::edgeuse::p1 as eu;
use crate::topology::face::{p1 as face1, p2 as face, p4 as face4};
use crate::topology::{loopuse, Brep, FaceId, LoopId};

const OP: &str = "offset::offset_executive::extinct_branches";

fn missing(what: &'static str) -> SolidError {
    SolidError::invalid_input(OP, what)
}

#[derive(Clone, Copy)]
struct Halfspace {
    origin: Point3,
    outward: Vec3,
}

fn plane_of(b: &Brep, f: FaceId) -> Result<Option<Halfspace>> {
    let Some(SurfaceKind::Plane(plane)) = b.face(f)?.surface.as_ref().map(Surface::kind) else {
        return Ok(None);
    };
    let fu = face::outer_faceuse(b, f)?.ok_or_else(|| missing("face without outer use"))?;
    // This first construction treats branches on the exterior shell only.
    let Some(shell) = b.faceuse(fu)?.shell else {
        return Ok(None);
    };
    let Some(infinite) = b.infinite_region() else {
        return Ok(None);
    };
    if b.shell(shell)?.region != Some(infinite) {
        return Ok(None);
    }
    let scale = plane.uv_scale();
    if ![scale.x, scale.y]
        .into_iter()
        .all(|v| v.is_finite() && v != 0.0)
    {
        return Ok(None);
    }
    let origin = plane.position().origin();
    let normal = plane.evaluate_normal();
    if ![origin.x, origin.y, origin.z, normal.x, normal.y, normal.z]
        .into_iter()
        .all(f64::is_finite)
    {
        return Ok(None);
    }
    if !cell::normal_defines_plane(normal, plane.position().x_axis(), plane.position().y_axis()) {
        return Ok(None);
    }
    let sign = match eu::faceuse_orientation(b, fu)? {
        OrientType::Same => 1.0,
        OrientType::Opposite => -1.0,
        OrientType::Unknown => return Ok(None),
    };
    Ok(Some(Halfspace {
        origin,
        outward: normal * sign,
    }))
}

/// The component on the other side of `attachment`, after cutting at `support`.
/// All boundary edges must be precisely that loop; reaching the outer loop, another
/// hole, or an opening is not a branch attached through this one polygon.
fn component(
    kernel: &Kernel,
    b: &Brep,
    support: FaceId,
    attachment: LoopId,
) -> Result<BTreeSet<FaceId>> {
    let mut boundary = BTreeSet::new();
    let mut pending = Vec::new();
    for e in loopuse::loop_edges(b, attachment)? {
        let e = e.ok_or_else(|| missing("attachment without edge"))?;
        boundary.insert(e);
        pending.extend(
            edge::faces(b, e, None)?
                .into_iter()
                .filter(|&f| f != support),
        );
    }
    let mut faces = BTreeSet::new();
    let mut found_boundary = BTreeSet::new();
    while let Some(f) = pending.pop() {
        kernel.check_cancelled(OP)?;
        if !faces.insert(f) {
            continue;
        }
        for e in face4::edges(b, f, None, false, None)? {
            for neighbour in edge::faces(b, e, None)? {
                if neighbour == support {
                    found_boundary.insert(e);
                } else if !faces.contains(&neighbour) {
                    pending.push(neighbour);
                }
            }
        }
    }
    if boundary != found_boundary {
        faces.clear();
    }
    Ok(faces)
}

/// A line definition or a nonrational degree-one B-spline with just two controls
/// bounds a straight segment. Higher degree curves and rational definitions are
/// outside this first polyhedral certificate, even if they look straight.
fn straight(b: &Brep, f: FaceId) -> Result<bool> {
    for e in face4::edges(b, f, None, false, None)? {
        let Some(curve) = b.edge(e)?.curve.as_ref() else {
            return Ok(false);
        };
        match curve.kind() {
            CurveKind::Line(_) => {}
            CurveKind::BSpline(c)
                if c.degree()? == 1 && c.number_control_points()? == 2 && !c.is_rational()? => {}
            _ => return Ok(false),
        }
    }
    Ok(true)
}

/// The offset sheet of a branch face whose source outward normal is `outward`, when its
/// definition is a finite plane parallel to the source within the angular tolerance.
fn offset_sheet(p: &Plane, outward: Vec3) -> Option<cell::Sheet> {
    let scale = p.uv_scale();
    let (origin, x, y) = p.position().canonical();
    let finite = [scale.x, scale.y]
        .into_iter()
        .all(|v| v.is_finite() && v != 0.0)
        && [origin.x, origin.y, origin.z, x.x, x.y, x.z, y.x, y.y, y.z]
            .into_iter()
            .all(f64::is_finite);
    (finite && p.evaluate_normal().is_parallel_to(outward, EFF_ZERO_DEG)).then_some(cell::Sheet {
        origin,
        x,
        y,
        outward,
    })
}

struct Extinction {
    support: FaceId,
    attachment: LoopId,
    faces: BTreeSet<FaceId>,
}

impl OffsetExecutive {
    /// Consume certified empty branch cells before any neighbour offsets are mated.
    /// The source remains intact for the shell difference and for provenance. Returns
    /// the source faces of the branches removed: their offsets hold no cavity, so walls
    /// meeting across them are not a refusal.
    pub(super) fn remove_extinct_branches(
        &mut self,
        kernel: &mut Kernel,
        original: &Brep,
        notifications: &mut OffsetNotifications<'_>,
    ) -> Result<BTreeSet<FaceId>> {
        if !self.inset()
            || !self.merge_results()
            || !self.local_faces.is_empty()
            || self.operation() != OffsetOperation::SolidOffset
            || !brep_is_manifold_solid(original)?
        {
            return Ok(BTreeSet::new());
        }
        let mut events = Vec::new();
        for support in bp1::faces(original, None)? {
            if self.shell_faces().contains(&support) {
                continue;
            }
            let Some(base) = plane_of(original, support)? else {
                continue;
            };
            for attachment in face::loops(original, support, false, None)?
                .into_iter()
                .skip(1)
            {
                kernel.check_cancelled(OP)?;
                let faces = component(kernel, original, support, attachment)?;
                if self.branch_is_empty(kernel, original, attachment, base, &faces)? {
                    events.push(Extinction {
                        support,
                        attachment,
                        faces,
                    });
                }
            }
        }
        if events.is_empty() {
            return Ok(BTreeSet::new());
        }
        let mut result = self
            .take_offset_brep()
            .ok_or_else(|| missing("no offset body"))?;
        let status = self.remove_branches_in(kernel, original, &mut result, &events, notifications);
        self.set_offset_brep(result);
        status?;
        Ok(events.into_iter().flat_map(|e| e.faces).collect())
    }

    fn branch_is_empty(
        &self,
        kernel: &Kernel,
        original: &Brep,
        attachment: LoopId,
        base: Halfspace,
        faces: &BTreeSet<FaceId>,
    ) -> Result<bool> {
        if faces.is_empty() {
            return Ok(false);
        }
        let mut planes = Vec::new();
        let mut facets = Vec::new();
        for &f in faces {
            kernel.check_cancelled(OP)?;
            if self.shell_faces().contains(&f)
                || face::loops(original, f, false, None)?.len() != 1
                || !straight(original, f)?
            {
                return Ok(false);
            }
            let Some(plane) = plane_of(original, f)? else {
                return Ok(false);
            };
            planes.push((f, plane));
            let loops = face::loops(original, f, false, None)?;
            let Some(&l) = loops.first() else {
                return Ok(false);
            };
            facets.push((l, plane));
        }
        let Some(cell) = cell::convex(kernel, original, attachment, base, facets)? else {
            return Ok(false);
        };
        // Use the actual offset definitions: each sheet's plane through its stored
        // origin and spanned by its stored axes, decided exactly. Source normals that
        // are opposite only within the angular tolerance (a branch turned by a
        // rotation, whose cosines round) still pair; the crossing is then certified
        // on the cell itself rather than from bit-equal normals.
        let result = self
            .offset_brep()
            .ok_or_else(|| missing("no offset body"))?;
        for (i, &(a, pa)) in planes.iter().enumerate() {
            kernel.check_cancelled(OP)?;
            for &(b, pb) in planes.iter().skip(i + 1) {
                if pa.outward.parallel_sense(pb.outward, EFF_ZERO_DEG) != (true, Some(true)) {
                    continue;
                }
                let sheet = |f: FaceId, outward: Vec3| -> Result<Option<cell::Sheet>> {
                    let f = self.single_offset_face(original, f)?;
                    let Some(SurfaceKind::Plane(p)) =
                        result.face(f)?.surface.as_ref().map(Surface::kind)
                    else {
                        return Err(missing("planar branch lost its plane"));
                    };
                    Ok(offset_sheet(p, outward))
                };
                let (Some(sa), Some(sb)) = (sheet(a, pa.outward)?, sheet(b, pb.outward)?) else {
                    continue;
                };
                if cell.separates(&sa, &sb) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn single_offset_face(&self, original: &Brep, f: FaceId) -> Result<FaceId> {
        let mut found = BTreeSet::new();
        let (a, b) = face::faceuses(original, f)?;
        for fu in [a, b].into_iter().flatten() {
            found.extend(self.faceuse_to_faces.seconds(fu));
        }
        match (found.len(), found.first()) {
            (1, Some(f)) => Ok(*f),
            _ => Err(missing("branch face has no unique offset")),
        }
    }

    fn remove_branches_in(
        &mut self,
        kernel: &mut Kernel,
        original: &Brep,
        result: &mut Brep,
        events: &[Extinction],
        notifications: &mut OffsetNotifications<'_>,
    ) -> Result<()> {
        let mut gone = BTreeSet::new();
        for event in events {
            kernel.check_cancelled(OP)?;
            let support = self.single_offset_face(original, event.support)?;
            let (lu, mate) = loopuse::loop_loopuses(original, event.attachment)?;
            let mut loops = BTreeSet::new();
            for lu in std::iter::once(lu).chain(mate) {
                for u in eu::loopuse_edgeuses(original, lu)? {
                    for mapped in self.edgeuse_to_edgeuses.seconds(u) {
                        if eu::face(result, mapped)? == Some(support) {
                            let lu = eu::loopuse(result, mapped)?
                                .ok_or_else(|| missing("offset edge without loop"))?;
                            loops.insert(
                                result
                                    .loopuse(lu)?
                                    .loop_
                                    .ok_or_else(|| missing("offset loop use without loop"))?,
                            );
                        }
                    }
                }
            }
            let attachment = match (loops.len(), loops.first()) {
                (1, Some(l)) => *l,
                _ => return Err(missing("attachment has no unique offset loop")),
            };
            for &f in &event.faces {
                let f = self.single_offset_face(original, f)?;
                let mut stores = NotificationStores {
                    records: &mut *notifications.records,
                    attributes: &mut *notifications.attributes,
                    key_of: notifications.result_key_of,
                };
                NotifyState::from_kernel_stores(&mut stores).with_kernel_env(
                    kernel,
                    |kernel, env| {
                        delete_face_with_body_identity(
                            kernel,
                            result,
                            env,
                            f,
                            DeleteFaceOptions {
                                delete_edges_vertices: true,
                                ..DeleteFaceOptions::default()
                            },
                            notifications.result_brep,
                        )
                    },
                )?;
                gone.insert(f);
            }
            let mut stores = NotificationStores {
                records: &mut *notifications.records,
                attributes: &mut *notifications.attributes,
                key_of: notifications.result_key_of,
            };
            NotifyState::from_kernel_stores(&mut stores)
                .with_kernel_env(kernel, |kernel, env| {
                    face1::delete_inner_loop(kernel, result, env, support, attachment)
                })?;
        }
        self.forget_faces(&gone)?;
        let live = result.edgeuses().ids().collect();
        forget_seconds(&mut self.edgeuse_to_edgeuses, &BTreeSet::new(), Some(&live))
    }
}
