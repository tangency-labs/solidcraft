//! The cliff overflow's session helpers (not upstream): the fork's `sm_HoldsItsLaw`
//! (`source/SMLib/src/SmFilletCliffOverflow.cpp`, tangency-labs/smlib `8ba4667`, `ae2a6de`,
//! rev `68fb9162`; WR-631, WR-638), the refusal notes and each solver's fillet faces, which
//! the trim at creases (`trim_at_creases`, WR-695) reads. The fork's
//! `SmFilletExecutive::SubtractRemovalRegion`, which subtracted the region the blends
//! remove, went with the flag `fillet_overflow_trim_at_creases`: the trim replaced it, as the
//! fork's smlib#79 step 6 deletes it. The construction steps are in
//! [`crate::blend::fillet_cliff_overflow`].
//!
//! | Rust                                       | Fork                                      |
//! |--------------------------------------------|-------------------------------------------|
//! | `SessionObjects::holds_its_law` (private)   | `sm_HoldsItsLaw`                          |
//!
//! Deviations: none.

use super::{invalid, FilletExecutive, FilletObject, FilletSession, SessionObjects, SolverObject};
use crate::blend::fillet_cliff_overflow::Refusal;
use crate::blend::fillet_geom::p1::{FilletGeomId, FilletSurfaceStorage};
use crate::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use crate::error::blend::{BlendFailure, OverflowInput};
use crate::error::{EntityRef, Failure, Result, SolidError, SM_ERR_NULL_POINTER};
use crate::kernel::Kernel;
use crate::math::det;
use crate::math::{Vec3, EFF_ZERO, EFF_ZERO_SQRT, PI};
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::p5::DropPointOptions;
use crate::topology::{edge, edgeuse::p1 as eu1, FaceId};

const OP: &str = "blend::fillet_executive::session::cliff_overflow";

/// A missing thing the executive built ([`BlendFailure::Defect`] keeping the upstream `NER`
/// status `SM_ERR_NULL_POINTER`), `why` saying why it is always there.
#[track_caller]
fn null_defect(why: &'static str) -> SolidError {
    Failure::new(OP, BlendFailure::Defect { why })
        .with_status(SM_ERR_NULL_POINTER)
        .into_error()
}

/// Why a solver's rolling edge use and its edge are there.
pub(super) const SOLVER_EDGE: &str =
    "a solver has no edge use, or its edge use no edge: a solver rolls \
     on an edge use of the filleted edge it is made for";

/// `smos_Max` of two doubles.
fn smos_max(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else {
        b
    }
}

/// `smos_Min` of two doubles.
fn smos_min(a: f64, b: f64) -> f64 {
    if a < b {
        a
    } else {
        b
    }
}

impl FilletSession<'_> {
    /// `GetFilletErrorInfo()->NoteError(code, comment)` of a [`Refusal`].
    pub(super) fn note_refusal(&mut self, refusal: Refusal) {
        self.executive.note_trim_refusal(refusal);
    }
}

impl FilletExecutive<FilletObject> {
    /// Preserve the cause and fit metadata of the sheet boundary construction.
    pub(super) fn note_trim_refusal(&mut self, refusal: Refusal) {
        self.error.code = refusal.code as i32;
        self.error.comment = refusal.comment.into();
        self.error.no_room = refusal.reason;
        self.error.no_room_without_site = true;
        // (not upstream) a refusal noted here is not an earlier classification's
        self.error.corner_cause = None;
    }
}

impl SessionObjects<'_> {
    /// The fork's `sm_HoldsItsLaw`: whether a variable round on planar faces holds its
    /// radius law between the filleted edge's ends.
    ///
    /// The round's point on the touch circle of the ball at each of seven points along the
    /// edge, in the wedge's bisector plane, must lie on the fillet surface, to twenty times
    /// the solver's tolerance and a ten-thousandth of the radius; of the two angles the
    /// planes make, and the bisector either way, the one that fits. Rounds of other solvers,
    /// with other than a circular section, on faces that are not both planes, with other
    /// than one fillet geometry, or along an edge that is not straight, pass.
    ///
    /// The point is on the round's touch circle, not the great circle (the fork's
    /// tangency-labs/smlib@8bedf89 = d01d7b0's 6007a3d, WR-685; its first `sm_HoldsItsLaw`
    /// took the great circle's point `r (1 / sin(a/2) - 1)` off the edge): the ball's
    /// centre `r / sin(a/2)` off the edge on the bisector, the circle square to the
    /// centres' line, its centre moved along that line by `-r dr/ds` (`s` the
    /// centres' arc length) and its radius `r sqrt(1 - (dr/ds)^2)`, its point in the
    /// bisector plane on the edge's side; the law's value and slope are read with `SER`.
    pub(super) fn holds_its_law(&self, k: &Kernel, solver: FilletObject) -> Result<bool> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        let object = self.solver(id)?;
        let SolverObject::VariableRadius(vr) = object else {
            return Ok(true);
        };
        let base = object.base();
        if base.fsg.as_ref().map(
            crate::blend::fillet_solver::p1::FilletSurfaceGenerator::fillet_surface_generator_type,
        ) != Some(FilletSurfaceGeneratorType::Circular)
        {
            return Ok(true);
        }
        let mut normals = [Vec3::UNDEF; 2];
        for (rail, normal) in normals.iter_mut().enumerate() {
            let offset = base.surface(rail)?.ok_or_else(|| {
                null_defect(
                    "a loaded solver has no offset surface for a rail: the solver set-up \
                         gives each rail the offset of its face's surface before the solver is \
                         loaded",
                )
            })?;
            let planar = offset.surface.base_surface().is_planar(k, EFF_ZERO)?;
            if !planar.is_planar {
                return Ok(true);
            }
            *normal = planar.normal;
        }
        let [n0, n1] = normals;
        let phi = det::acos(smos_max(-1.0, smos_min(1.0, n0.dot(n1))));
        let [geom] = base.fillet_geoms[..] else {
            return Ok(true);
        };
        let Some(fillet) = self.fillet_surface(geom)? else {
            return Ok(true);
        };
        let fillet = fillet.clone_for_evaluation()?;
        let eu = base.edgeuse(0)?.ok_or_else(|| null_defect(SOLVER_EDGE))?;
        let target = &*self.target;
        let e = eu1::edge(target, eu)?.ok_or_else(|| null_defect(SOLVER_EDGE))?;
        let curve = edge::curve(target, e)?.ok_or_else(|| {
            SolidError::blend(
                OP,
                BlendFailure::OverflowInputMissing(OverflowInput::EdgeCurve),
            )
        })?;
        if !curve.is_linear(k, EFF_ZERO, false)?.linear {
            return Ok(true);
        }
        let interval = edge::interval(target, e)?;
        let domain = fillet.natural_uv_domain_with_nurb(k)?;
        let tol = 20.0 * base.approx_tol_3d;
        // (the law evaluates through its own caches: a copy, the solver stays shared)
        let mut law = vr.law.clone();
        // the fork's tangency-labs/smlib@8bedf89 (= d01d7b0's 6007a3d; WR-685): the round
        // touches each ball along its touch circle, not its great circle
        let mut holds = true;
        let mut i = 1;
        while i < 8 && holds {
            let t = interval.evaluate(f64::from(i) / 8.0);
            i += 1;
            let edge_pv = curve.evaluate(k, t, 1, true, false)?;
            let [pnt, edge_d, ..] = *edge_pv.as_slice() else {
                return Err(SolidError::blend(
                    OP,
                    BlendFailure::Defect {
                        why: "the filleted edge's curve evaluated with one derivative gives no \
                              point and derivative: a curve's evaluation returns its point and \
                              each derivative asked for",
                    },
                ));
            };
            let [r, dr, _] = law.evaluate(t, &vr.law_interval, vr.law_orientation)?;
            let radius = r.abs();
            let slope = if r < 0.0 { -dr } else { dr }; // d|r|/dT
                                                        // The ball's centre on the wedge's bisector, r / sin(a/2) off the edge, and the
                                                        // touch circle's point on the bisector plane toward the edge. The planes' normals
                                                        // are both outward (or inward), the bisector along their sum and the angle pi
                                                        // less theirs, or one of each, the bisector along their difference.
            let mut fits = false;
            let mut c = 0;
            while c < 4 && !fits {
                let bisector = if c < 2 { n0 + n1 } else { n0 - n1 };
                let angle = if c < 2 { PI - phi } else { phi };
                let sign = if c % 2 == 1 { -1.0 } else { 1.0 };
                c += 1;
                let len = bisector.length();
                if len <= EFF_ZERO_SQRT || angle <= EFF_ZERO_SQRT {
                    continue;
                }
                let out = bisector * (sign / len); // edge to centre
                let off = 1.0 / det::sin(angle / 2.0);
                let centre = pnt + out * (radius * off);
                let along = edge_d + out * (slope * off); // dC/dT
                let speed = along.length();
                if speed <= EFF_ZERO {
                    continue;
                }
                let along = along / speed;
                let rate = slope / speed; // dr/ds
                if rate.abs() >= 1.0 {
                    continue;
                }
                let to_edge = -out - along * (-out).dot(along);
                let to_edge_len = to_edge.length();
                if to_edge_len <= EFF_ZERO {
                    continue;
                }
                let middle = centre
                    + along * (-radius * rate)
                    + to_edge * (radius * (1.0 - rate * rate).sqrt() / to_edge_len);
                let dropped = fillet.drop_point::<NoObject>(
                    k,
                    middle,
                    &domain,
                    &DropPointOptions::default(),
                )?;
                fits = dropped.success && dropped.gap <= tol + 1.0e-4 * radius;
            }
            holds = fits;
        }
        Ok(holds)
    }

    /// The face of the intermediate body each of `solver`'s fillet geometries' surfaces is
    /// on (`pFG->GetFilletSurface()->GetOwner()` in `m_pFilletBrep`), `None` for none.
    pub(super) fn fillet_faces(
        &self,
        solver: FilletObject,
    ) -> Result<Vec<(FilletGeomId, Option<FaceId>)>> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        let body =
            self.intermediate_identity(EntityRef::Brep(crate::topology::BrepId::from_index(0)))?;
        let mut out = Vec::new();
        for &g in &self.solver(id)?.base().fillet_geoms {
            let face = match self.pseudo.geom(g)?.fillet_surface {
                Some(FilletSurfaceStorage::ManagedFace { body: owner, face }) if owner == body => {
                    Some(face)
                }
                _ => None,
            };
            out.push((g, face));
        }
        Ok(out)
    }
}
