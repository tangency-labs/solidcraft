//! Not upstream: moving faces of a solid along their normals, the tangent blends on them
//! following (the SolidCraft fork's `SmOffsetFacesExecutive`, WR-615).
//!
//! Fork: tangency-labs/smlib a96ce9f, 607d617, 758829c and 9504880 (opt-in), `source/SMLib/src/SmOffsetFaces.cpp`,
//! `source/SMLib/inc/SmOffsetFaces.h` and `SM_AI_FOLLOWED_BLEND` (`SmAttribute.h`). Pinned
//! SMLib has none of it; the pinned face move (`FaceuseOffset` and `SmBrep::LocalOperation`)
//! is unchanged.
//!
//! | Rust                                  | Fork                                              |
//! |---------------------------------------|---------------------------------------------------|
//! | [`OffsetFacesExecutive`]              | `SmOffsetFacesExecutive`                          |
//! | [`OffsetFacesExecutive::new`]         | `SmOffsetFacesExecutive::SmOffsetFacesExecutive`  |
//! | [`OffsetFacesExecutive::refusal`], [`OffsetFacesExecutive::reason`], [`OffsetFacesExecutive::followed_blends`] | `GetRefusal`, `GetReason`, `GetFollowedBlends` |
//! | [`OffsetFacesRefusal`]                | `SmOffsetFacesRefusal`                            |
//! | [`FollowedBlendMark`]                 | `SmFollowedBlendMark` (`Merge`, `On`, `Has`, `Add`, `Clear`) |
//! | [`Followed`]                          | `SmFollowed`                                      |
//! | [`OffsetFacesExecutive::collect`]     | `OffsetFaces` step 1 (the followed blends)        |
//! | [`OffsetFacesExecutive::keep_and_mark`] | `OffsetFaces` step 2 (attributes kept, marks)   |
//! | [`OffsetFacesExecutive::restore`]     | `OffsetFaces` step 6 (attributes put back)        |
//! | `keep` / `put` (private)              | `sm_Keep` / `sm_Put`                              |
//! | `face_across` (private)               | `sm_FaceAcross`                                   |
//! | [`OffsetFacesExecutive::move_faces`]  | `OffsetFaces` step 4 (the faces moved)            |
//! | [`OffsetFacesExecutive::reblend`]     | `OffsetFaces` step 5 (the sharp edges blended again) |
//! | `outer_faceuse` (private)             | `sm_OuterFaceuse`                                 |
//! | [`OffsetFacesExecutive::take_off`]    | `OffsetFaces` step 3 (the blends taken off)       |
//! | [`OffsetFacesExecutive::offset_faces`] | `OffsetFaces`                                    |
//!
//! Deviations:
//! - **Scope.** The marks, the collection of the followed blends, the
//!   kept attributes and their restoring (steps 1, 2 and 6 of `OffsetFaces`), the move
//!   (step 4), the re-blend (step 5), taking the blends off (step 3) and `OffsetFaces`.
//! - **Mitred groups.** Blends that meet each other at their ends come off together
//!   through [`FeatureExecutive`] (`SM_RB_EXTEND_SURFACE`), as in the fork. The fork's
//!   `DoDefeature` always succeeds and hands back the base B-rep itself, so only
//!   `GetBaseWasApplied` is tested. The fork then takes the B-rep in afresh: it copies it
//!   (`SmBrep` copy constructor), runs `RemoveAllTopology` and merges the copy back. The
//!   port does not, because an arena B-rep has no stale state to drop; it refreshes its
//!   cached topology fields and keeps its ids, so its marks stay on their objects.
//! - **Cached topology.** After each removal and before [`OffsetFacesExecutive::offset_faces`]
//!   returns, the port's cached topology fields are refreshed (upstream has none).
//! - **"Moved no face".** The fork marks the new surfaces and, after the local operation,
//!   asks whether some face's surface carries the mark. Surfaces here carry no attributes,
//!   and the local operation's extended offset copies them, so a face is taken to lie on a
//!   new surface when nine points of its surface over its UV domain drop onto that
//!   surface within the B-rep's tolerance.
//! - **Not exact.** The fork's fillet executive can fail with `SM_FILERR_NOT_EXACT`
//!   (`SM_OFR_NOT_EXACT` then); the port's has no such code, so that branch of step 5
//!   is never taken.
//! - **The local operation** runs with notification identities of its own
//!   ([`local_operation_simple`]), as the fork calls it on the B-rep directly.
//! - **Marks.** `SmFollowedBlendMark` is an application attribute
//!   ([`crate::geom::attribute::AttributeHandlers`]) whose merge unions the two objects'
//!   codes, as the fork's `Merge`. A code added to an object that already has a mark
//!   replaces the mark by one with the code appended (the fork appends in place); marks are
//!   only looked up by id.
//! - **Kept attributes** are owned copies ([`Attribute::make_copy`]) instead of heap
//!   pointers freed by a scope guard.
//! - **Refusals** return [`SolidError::Upstream`] `SM_ERR` with the refusal and its reason
//!   on the executive, as the fork's `Refuse` returns `SM_ERR`.
//! - **Closed walls (opt-in).** 9504880's removal of the edges between a moved face and
//!   one that stayed on one surface runs only with
//!   `Kernel::fixes.local_operation_closes_walls_to_nothing` (WR-566 addendum); a face is
//!   on a new surface there when it is a moved face ([`MARK_MOVED`]) lying on one.
//! - No recursion.

use crate::blend::feature_executive::{FeatureExecutive, RebuildBrepType};
use crate::blend::fillet_executive::p1::is_radius_supported;
use crate::blend::fillet_executive::session::FilletSession;
use crate::blend::fillet_geom::p1::FilletErrorType;
use crate::blend::fillet_removal::ring::RingFilletRemoval;
use crate::blend::fillet_removal::FilletRemoval;
use crate::blend::fillet_solver::p1::FilletSolverType;
use crate::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use crate::blend::identify_fillet::{identify_fillet, IdentifyFilletOptions};
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::attribute::{
    ids, ApplicationValue, Attribute, AttributeHandle, AttributeHandlers, AttributeValue,
    AttributeView, Behavior, ObjectKey, Placement,
};
use crate::geom::global_solver::SolverOperation;
use crate::kernel::Kernel;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::nurbs::knots::Side;
use crate::offset::offset_executive::p2::local_operation_simple;
use crate::offset::offset_geometry_creation::{FaceOffsetBrep, OffsetGeometryCreation};
use crate::surface::plane::Plane;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::p5::DropPointOptions;
use crate::surface::surface::Surface;
use crate::topology::brep::p5::{
    remove_topological_edges_and_vertices, stitch_and_orient, StitchAndOrientOptions,
};
use crate::topology::brep::validation::assert_valid;
use crate::topology::edgeuse::p1::primary_edgeuse;
use crate::topology::edgeuse::p3::normalized_evaluate;
use crate::topology::{edge, Brep, EdgeId, FaceId, FaceuseId};

const OP: &str = "offset::offset_faces";

fn sm_err() -> SolidError {
    SolidError::sm_err(OP)
}

/// `NER(p)`: a `NULL` pointer is `SM_ERR_NULL_POINTER`.
fn ner<T>(v: Option<T>) -> Result<T> {
    v.ok_or_else(|| SolidError::null_pointer(OP))
}

/// `s_dTangencyTolDeg`: the tangency tolerance the removals judge rails by
/// (`SmFilletRemoval::CollectFilletTopology`).
pub const TANGENCY_TOL_DEG: f64 = 2.0;

// What a mark says. A face or edge can carry several (a face both blends of a corner roll on).
/// `s_lTarget`: a surface a moved face is to go onto.
pub const MARK_TARGET: i64 = -1;
/// `s_lMoved`: a face to move.
pub const MARK_MOVED: i64 = -3;
/// `s_lBefore`: a face there before the blends were put back; an edge there before a
/// group came off.
pub const MARK_BEFORE: i64 = -2;
/// `s_lStride`: followed blend `i`'s codes are `i * MARK_STRIDE + role`.
pub const MARK_STRIDE: i64 = 64;
/// `s_lSupport`: `+ 0, 1`, the face across rail 0, rail 1.
pub const MARK_SUPPORT: i64 = 0;
/// `s_lSharp`: the sharp edge the blend became.
pub const MARK_SHARP: i64 = 2;
/// `s_lEnd`: `+ k`, the face across the blend's `k`-th other edge (an end face).
pub const MARK_END: i64 = 3;

/// Why [`OffsetFacesExecutive`] refused (`SmOffsetFacesRefusal`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OffsetFacesRefusal {
    /// `SM_OFR_NONE`: not refused (it succeeded, or failed for another reason: see the
    /// status).
    #[default]
    None,
    /// `SM_OFR_NO_ROOM`: a blend that follows a moved face no longer fits the faces it
    /// rolls on.
    NoRoom,
    /// `SM_OFR_UNSUPPORTED`: a blend on a moved face SMLib cannot take off and put back as
    /// it was.
    Unsupported,
    /// `SM_OFR_NOT_EXACT`: following a blend needs a surface or edge continued that SMLib
    /// cannot continue exactly.
    NotExact,
    /// `SM_OFR_NO_EFFECT`: the local operation reported success but moved no face.
    NoEffect,
}

// ---- SmFollowedBlendMark ---------------------------------------------------------------

/// The marks on faces, edges and surfaces for the length of one face move
/// (`SmFollowedBlendMark`, attribute id [`ids::FOLLOWED_BLEND`], `SM_AB_COPY`): which
/// blend each face rolls on, which edge each blend became, which surfaces the moved faces
/// go onto. SMLib hands an attribute on through every split, merge and copy it makes and
/// builds new topology without any; a merge keeps what either object stood for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FollowedBlendMark {
    /// `m_vCodes`.
    pub codes: Vec<i64>,
}

/// The mark found on `obj` in `view` (`On`): the first attribute of the id, when it is a
/// mark.
fn view_mark<'a>(
    view: &AttributeView<'a>,
    obj: ObjectKey,
) -> Option<(AttributeHandle, &'a FollowedBlendMark)> {
    let (h, a) = view.find_attribute(obj, ids::FOLLOWED_BLEND)?;
    a.application_value::<FollowedBlendMark>().map(|m| (h, m))
}

impl AttributeHandlers for FollowedBlendMark {
    /// `Merge(pA, pB, pResult)`: every code of either object's mark added to the result.
    fn merge(
        &self,
        view: &AttributeView<'_>,
        orig1: ObjectKey,
        orig2: ObjectKey,
        merged: ObjectKey,
    ) -> Result<Vec<Placement>> {
        let mut all = Vec::new();
        for x in [orig1, orig2] {
            if let Some((_, m)) = view_mark(view, x) {
                all.extend(m.codes.iter().copied());
            }
        }
        if all.is_empty() {
            return Ok(Vec::new());
        }
        // (`Add` for each code: appended to the result's mark, or a new mark)
        let existing = view_mark(view, merged);
        let mut codes = existing.map(|(_, m)| m.codes.clone()).unwrap_or_default();
        for code in all {
            if !codes.contains(&code) {
                codes.push(code);
            }
        }
        Ok(match existing {
            Some((handle, _)) => vec![Placement::Edit {
                handle,
                value: AttributeValue::Application(ApplicationValue::new(Self { codes })),
            }],
            None => vec![Placement::Add {
                on: merged,
                attribute: Self::attribute(codes),
            }],
        })
    }
}

impl FollowedBlendMark {
    /// A mark holding `codes`.
    #[must_use]
    pub fn attribute(codes: Vec<i64>) -> Attribute {
        Attribute::application(ids::FOLLOWED_BLEND, Behavior::Copy, Self { codes })
    }

    /// `On(pX)`: the mark of `e` (the first attribute of the id, when it is a mark).
    #[must_use]
    pub fn on(b: &Brep, e: EntityRef) -> Option<(AttributeHandle, &Self)> {
        let (h, a) = b
            .entity_attributes(e)
            .into_iter()
            .find(|(_, a)| a.attribute_id() == ids::FOLLOWED_BLEND)?;
        a.application_value::<Self>().map(|m| (h, m))
    }

    /// `Has(pX, lCode)`.
    #[must_use]
    pub fn has(b: &Brep, e: Option<EntityRef>, code: i64) -> bool {
        e.and_then(|e| Self::on(b, e))
            .is_some_and(|(_, m)| m.codes.contains(&code))
    }

    /// `Add(pX, lCode)`: `code` added to `e`'s mark, or a new mark (nothing for `None`).
    ///
    /// # Errors
    ///
    /// [`SolidError::NotFound`] for a dead entity.
    pub fn add(b: &mut Brep, e: Option<EntityRef>, code: i64) -> Result<()> {
        let Some(e) = e else {
            return Ok(());
        };
        if Self::has(b, Some(e), code) {
            return Ok(());
        }
        let mut codes = Vec::new();
        if let Some((h, m)) = Self::on(b, e) {
            codes.clone_from(&m.codes);
            b.remove_attribute(e, h)?;
        }
        codes.push(code);
        b.add_attribute(e, Self::attribute(codes))?;
        Ok(())
    }

    /// `Clear(pX)`: `e`'s mark removed.
    ///
    /// # Errors
    ///
    /// [`SolidError::NotFound`] for a dead entity.
    pub fn clear(b: &mut Brep, e: EntityRef) -> Result<()> {
        if let Some((h, _)) = Self::on(b, e) {
            b.remove_attribute(e, h)?;
        }
        Ok(())
    }

    /// `SmBrep::RemoveAttributeFromTopology(SM_AI_FOLLOWED_BLEND)`: every mark of the
    /// B-rep's topology removed.
    ///
    /// # Errors
    ///
    /// Those of [`Brep::remove_attribute`].
    pub fn remove_from_topology(b: &mut Brep) -> Result<()> {
        for e in b.attributed_entities() {
            let marks: Vec<AttributeHandle> = b
                .entity_attributes(e)
                .into_iter()
                .filter(|(_, a)| a.attribute_id() == ids::FOLLOWED_BLEND)
                .map(|(h, _)| h)
                .collect();
            for h in marks {
                b.remove_attribute(e, h)?;
            }
        }
        Ok(())
    }
}

// ---- kept attributes -------------------------------------------------------------------

/// `sm_Keep`: copies of `e`'s attributes that are copied with it (`SM_AB_COPY`,
/// `SM_AB_STANDALONE_COPY`: history tags are), the marks left out.
fn keep(b: &Brep, e: EntityRef) -> Vec<Attribute> {
    b.entity_attributes(e)
        .into_iter()
        .filter(|(_, a)| a.attribute_id() != ids::FOLLOWED_BLEND && a.behavior().is_copy())
        .map(|(_, a)| a.make_copy())
        .collect()
}

/// `sm_Put`: the kept attributes put on `e`, replacing any it has of the same ids.
fn put(b: &mut Brep, kept: &[Attribute], e: Option<EntityRef>) -> Result<()> {
    let Some(e) = e else {
        return Ok(());
    };
    for a in kept {
        let old = b
            .entity_attributes(e)
            .into_iter()
            .find(|(_, x)| x.attribute_id() == a.attribute_id())
            .map(|(h, _)| h);
        if let Some(h) = old {
            b.remove_attribute(e, h)?;
        }
        b.add_attribute(e, a.make_copy())?;
    }
    Ok(())
}

/// `sm_FaceAcross`: the face across `e` from `f` (a manifold edge), or `None`.
fn face_across(b: &Brep, e: EdgeId, f: FaceId) -> Result<Option<FaceId>> {
    let faces = edge::faces(b, e, None)?;
    Ok(match faces.as_slice() {
        [a, c] if *a == f => Some(*c),
        [a, c] if *c == f => Some(*a),
        _ => None,
    })
}

/// `GetEdges` of a face.
fn face_edges(b: &Brep, f: FaceId) -> Result<Vec<EdgeId>> {
    crate::topology::face::p4::edges(b, f, None, false, None)
}

// ---- SmFollowed ------------------------------------------------------------------------

/// One blend that follows a moved face (`SmFollowed`).
#[derive(Debug, Clone)]
pub struct Followed {
    /// `pBlend`: the blend face (until it is taken off).
    pub blend: Option<FaceId>,
    /// `aRails`.
    pub rails: [Option<EdgeId>; 2],
    /// `eSolver`: [`FilletSolverType::ConstRadius`] or [`FilletSolverType::ConstDist`].
    pub solver: FilletSolverType,
    /// `dSize`: the radius, or the chord.
    pub size: f64,
    /// `sFaceAttrs`: the blend face's attributes, kept.
    pub face_attrs: Vec<Attribute>,
    /// `aRailAttrs`.
    pub rail_attrs: [Vec<Attribute>; 2],
    /// `sSeamAttrs`: its seam's.
    pub seam_attrs: Vec<Attribute>,
    /// `vEndAttrs`: its `k`-th other edge's (with the face [`MARK_END`] `+ k`).
    pub end_attrs: Vec<Vec<Attribute>>,
    /// `vEndAcross`: the followed blend across that edge, or `None` (`-1`).
    pub end_across: Vec<Option<usize>>,
    /// `bTakenOff`.
    pub taken_off: bool,
}

// ---- SmOffsetFacesExecutive ------------------------------------------------------------

/// The face move whose tangent blends follow (`SmOffsetFacesExecutive`, not upstream).
///
/// Moves faces of a solid along their outward normals (negative: into the body), each
/// onto its surface's offset, the faces around them extended or trimmed to meet them
/// (`SmBrep::LocalOperation`), and the tangent blends on the moved faces following them:
/// the body is `blend(move(unblend(body)))`. A blend follows a moved face when it is a
/// face beside it across a tangent edge, not moved itself, that `SmFace::IdentifyFillet`
/// recognises with that edge as a rail, the two faces it rolls on meeting at an angle.
#[derive(Debug, Clone, Default)]
pub struct OffsetFacesExecutive {
    /// `m_eRefusal`.
    refusal: OffsetFacesRefusal,
    /// `m_cReason`.
    reason: String,
    /// `m_lFollowed`.
    followed: usize,
}

impl OffsetFacesExecutive {
    /// `SmOffsetFacesExecutive(crContext, pBrep)`: no refusal, no reason, no blends.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `GetRefusal`.
    #[must_use]
    pub const fn refusal(&self) -> OffsetFacesRefusal {
        self.refusal
    }

    /// `GetReason`: why it was refused (empty when not).
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// `GetFollowedBlends`: how many blends follow the moved faces.
    #[must_use]
    pub const fn followed_blends(&self) -> usize {
        self.followed
    }

    /// `Refuse(eRefusal, cReason, cMore)`: notes why the operation is refused; the
    /// `SM_ERR` to return.
    fn refuse(
        &mut self,
        refusal: OffsetFacesRefusal,
        reason: &str,
        more: Option<&str>,
    ) -> SolidError {
        self.refusal = refusal;
        self.reason = match more {
            Some(m) if !m.is_empty() => format!("{reason}: {m}"),
            _ => reason.to_string(),
        };
        sm_err()
    }

    /// `OffsetFaces` step 1: the blends that follow `faces` (beside a moved face across a
    /// tangent manifold edge, not moved themselves, recognised as fillets with that edge
    /// as a rail, the two faces they roll on meeting at an angle), their kinds as SMLib
    /// recovers them exactly. Sets [`Self::followed_blends`].
    ///
    /// # Errors
    ///
    /// `SM_ERR` with [`OffsetFacesRefusal::Unsupported`] for a blend that cannot be taken
    /// off and put back (its rails in pieces, one face on both sides, a section that is
    /// no circle, a variable radius, too small a size); those of `IdentifyFillet`, the
    /// evaluations and drops (`SER`); `SM_ERR_NULL_POINTER` for a moved face or blend
    /// surface missing.
    #[allow(clippy::too_many_lines)] // one fork routine
    pub fn collect(
        &mut self,
        k: &Kernel,
        b: &mut Brep,
        faces: &[FaceId],
        tol: f64,
    ) -> Result<Vec<Followed>> {
        let mut blends: Vec<Followed> = Vec::new();
        for &moved in faces {
            for e in face_edges(b, moved)? {
                if !edge::is_manifold(b, e)?
                    || !edge::is_tangent_edge(k, b, e, TANGENCY_TOL_DEG, false)?
                {
                    continue;
                }
                let Some(blend) = face_across(b, e, moved)? else {
                    continue;
                };
                if faces.contains(&blend) {
                    continue;
                }
                if blends.iter().any(|x| x.blend == Some(blend)) {
                    continue;
                }

                let id = identify_fillet(
                    k,
                    b,
                    blend,
                    &IdentifyFilletOptions {
                        tol_3d: tol,
                        tangency_tol_deg: TANGENCY_TOL_DEG,
                        require_tangent: true,
                        require_manifold: true,
                        want_min_max_radii: false,
                    },
                )?;
                // A tangent neighbour that is no blend (a piece of the same surface, a
                // step) is the local operation's, as before.
                if !id.is_fillet
                    || !(id.rail_edges_min.contains(&e) || id.rail_edges_max.contains(&e))
                {
                    continue;
                }
                let (&[rail0], &[rail1]) =
                    (id.rail_edges_min.as_slice(), id.rail_edges_max.as_slice())
                else {
                    return Err(self.refuse(
                        OffsetFacesRefusal::Unsupported,
                        "a blend on a moved face has its contact line in pieces: it cannot be taken off and put back",
                        None,
                    ));
                };
                let supports = [
                    ner(face_across(b, rail0, blend)?)?,
                    ner(face_across(b, rail1, blend)?)?,
                ];
                if supports[0] == supports[1] {
                    return Err(self.refuse(
                        OffsetFacesRefusal::Unsupported,
                        "a blend on a moved face rolls on one face on both sides",
                        None,
                    ));
                }

                // The faces it rolls on must meet at an angle: a half round between
                // parallel faces (a slot's end) blends no edge.
                {
                    let prim = ner(primary_edgeuse(b, rail0)?)?;
                    let on0 = normalized_evaluate(k, b, prim, 0.5, false, false, true)?.point;
                    let rail1_curve = ner(edge::curve(b, rail1)?)?;
                    let d = rail1_curve.drop_point(
                        k,
                        &edge::interval(b, rail1)?,
                        on0,
                        None,
                        tol,
                        None,
                        SolverOperation::Minimize,
                    )?;
                    if !d.success {
                        return Err(sm_err());
                    }
                    let on1 = rail1_curve.evaluate_point(k, d.parameter)?;
                    let surf = ner(crate::topology::face::p2::surface(b, blend)?)?;
                    let domain = surf.natural_uv_domain_with_nurb(k)?;
                    let mut normals = [crate::math::Vec3::UNDEF; 2];
                    for (normal, on) in normals.iter_mut().zip([on0, on1]) {
                        let dp = surf.drop_point::<NoObject>(
                            k,
                            on,
                            &domain,
                            &DropPointOptions::default(),
                        )?;
                        if !dp.success {
                            return Err(sm_err());
                        }
                        *normal = surf.evaluate_normal(k, dp.result_uv, Side::Left, Side::Left)?;
                    }
                    let cos =
                        normals[0].dot(normals[1]) / (normals[0].length() * normals[1].length());
                    let limit = (TANGENCY_TOL_DEG * (crate::math::PI / 180.0)).cos();
                    // (`!(dCos > -limit) || !(dCos < limit)`: a NaN cosine skips too)
                    let at_an_angle = cos > -limit && cos < limit;
                    if !at_an_angle {
                        continue;
                    }
                }

                // Its kind, as SMLib recovers it exactly: a circular section at constant
                // radius (a round), or at constant chord.
                if id.cross_section != 0 {
                    return Err(self.refuse(
                        OffsetFacesRefusal::Unsupported,
                        if (3..=5).contains(&id.cross_section) {
                            "a curvature-continuous blend on a moved face: SMLib cannot recover its section exactly to put it back"
                        } else {
                            "a blend on a moved face whose section is no circle: SMLib cannot recover it exactly to put it back"
                        },
                        None,
                    ));
                }
                let (solver, size) = if id.solver_type == 0 && id.radius > 0.0 {
                    (FilletSolverType::ConstRadius, id.radius)
                } else if id.solver_type == 1 && id.dist > 0.0 {
                    (FilletSolverType::ConstDist, id.dist)
                } else {
                    return Err(self.refuse(
                        OffsetFacesRefusal::Unsupported,
                        "a variable round on a moved face: SMLib cannot recover its radius law exactly to put it back",
                        None,
                    ));
                };
                if !is_radius_supported(size, tol) {
                    return Err(self.refuse(
                        OffsetFacesRefusal::Unsupported,
                        "a blend on a moved face is too small for SMLib to put back",
                        None,
                    ));
                }
                blends.push(Followed {
                    blend: Some(blend),
                    rails: [Some(rail0), Some(rail1)],
                    solver,
                    size,
                    face_attrs: Vec::new(),
                    rail_attrs: [Vec::new(), Vec::new()],
                    seam_attrs: Vec::new(),
                    end_attrs: Vec::new(),
                    end_across: Vec::new(),
                    taken_off: false,
                });
            } // end iter every edge of a moved face
        } // end iter every moved face
        self.followed = blends.len();
        Ok(blends)
    }

    /// `OffsetFaces` step 2: what each followed blend stands for, kept (its face's, rails',
    /// seam's and other edges' copied attributes), and marks on the faces around it: the
    /// face across each rail ([`MARK_SUPPORT`]) and across each other edge ([`MARK_END`]).
    ///
    /// # Errors
    ///
    /// `SM_ERR` with [`OffsetFacesRefusal::Unsupported`] for a blend with more other edges
    /// than the codes hold; `SM_ERR_NULL_POINTER` for a blend already taken off; those of
    /// the topology queries and marks.
    pub fn keep_and_mark(&mut self, b: &mut Brep, blends: &mut [Followed]) -> Result<()> {
        let faces: Vec<Option<FaceId>> = blends.iter().map(|x| x.blend).collect();
        for (ii, rb) in blends.iter_mut().enumerate() {
            let blend = ner(rb.blend)?;
            let base = i64::try_from(ii).map_err(|_| sm_err())? * MARK_STRIDE;
            rb.face_attrs = keep(b, EntityRef::Face(blend));
            for jj in 0..2usize {
                let rail = ner(rb.rails.get(jj).copied().flatten())?;
                if let Some(slot) = rb.rail_attrs.get_mut(jj) {
                    *slot = keep(b, EntityRef::Edge(rail));
                }
                let across = face_across(b, rail, blend)?;
                let code = base + MARK_SUPPORT + i64::try_from(jj).map_err(|_| sm_err())?;
                FollowedBlendMark::add(b, across.map(EntityRef::Face), code)?;
            }
            for e in face_edges(b, blend)? {
                if rb.rails.contains(&Some(e)) {
                    continue;
                }
                let Some(across) = face_across(b, e, blend)? else {
                    // its seam (an edge whose only face it is)
                    rb.seam_attrs = keep(b, EntityRef::Edge(e));
                    continue;
                };
                let lk = i64::try_from(rb.end_attrs.len()).map_err(|_| sm_err())?;
                if lk >= MARK_STRIDE - MARK_END {
                    return Err(self.refuse(
                        OffsetFacesRefusal::Unsupported,
                        "a blend on a moved face has too many edges to put back",
                        None,
                    ));
                }
                rb.end_attrs.push(keep(b, EntityRef::Edge(e)));
                // (the last followed blend that face is, as the fork's loop leaves it)
                let across_blend = faces.iter().rposition(|&f| f == Some(across));
                rb.end_across.push(across_blend);
                FollowedBlendMark::add(b, Some(EntityRef::Face(across)), base + MARK_END + lk)?;
            }
        }
        Ok(())
    }

    /// `OffsetFaces` step 6: the new blend faces (faces without a [`MARK_BEFORE`] mark)
    /// beside both faces of a followed blend take the kept attributes of that blend, their
    /// edges those of the old ones: a rail for the rail on the same face, a seam for the
    /// seam, an end edge for the end edge on the same face (or, between two new blend
    /// faces, the edge where those two blends met).
    ///
    /// # Errors
    ///
    /// Those of the topology queries and of [`Brep::add_attribute`].
    pub fn restore(b: &mut Brep, blends: &[Followed]) -> Result<()> {
        let all = crate::topology::brep::p1::faces(b, None)?;
        let mut new_faces: Vec<(FaceId, usize)> = Vec::new();
        for &new in &all {
            if FollowedBlendMark::has(b, Some(EntityRef::Face(new)), MARK_BEFORE) {
                continue;
            }
            let bound = face_edges(b, new)?;
            for ii in 0..blends.len() {
                let base = i64::try_from(ii).map_err(|_| sm_err())? * MARK_STRIDE;
                // the blend's: beside both its faces
                let mut beside = [false; 2];
                for &e in &bound {
                    let across = face_across(b, e, new)?.map(EntityRef::Face);
                    for (jj, flag) in beside.iter_mut().enumerate() {
                        let code = base + MARK_SUPPORT + i64::try_from(jj).map_err(|_| sm_err())?;
                        if FollowedBlendMark::has(b, across, code) {
                            *flag = true;
                        }
                    }
                }
                if beside[0] && beside[1] {
                    new_faces.push((new, ii));
                    break;
                }
            }
        }
        for &(new, ii) in &new_faces {
            let base = i64::try_from(ii).map_err(|_| sm_err())? * MARK_STRIDE;
            let rb = blends.get(ii).ok_or_else(sm_err)?;
            put(b, &rb.face_attrs, Some(EntityRef::Face(new)))?;
            for e in face_edges(b, new)? {
                let Some(across) = face_across(b, e, new)? else {
                    put(b, &rb.seam_attrs, Some(EntityRef::Edge(e)))?;
                    continue;
                };
                for (jj, rail_attrs) in rb.rail_attrs.iter().enumerate() {
                    let code = base + MARK_SUPPORT + i64::try_from(jj).map_err(|_| sm_err())?;
                    if FollowedBlendMark::has(b, Some(EntityRef::Face(across)), code) {
                        put(b, rail_attrs, Some(EntityRef::Edge(e)))?;
                    }
                }
                // the blend across, if the face across is one of the new blend faces
                // (the last such, as the fork's loop leaves it)
                let across_blend = new_faces
                    .iter()
                    .rev()
                    .find(|(f, _)| *f == across)
                    .map(|&(_, i)| i);
                for (kk, end_attrs) in rb.end_attrs.iter().enumerate() {
                    let same = if let Some(i) = across_blend {
                        rb.end_across.get(kk).copied().flatten() == Some(i)
                    } else {
                        let code = base + MARK_END + i64::try_from(kk).map_err(|_| sm_err())?;
                        FollowedBlendMark::has(b, Some(EntityRef::Face(across)), code)
                    };
                    if same {
                        put(b, end_attrs, Some(EntityRef::Edge(e)))?;
                    }
                }
            }
        }
        Ok(())
    }
}

// ---- step 4: the faces moved ------------------------------------------------------------

/// `sm_OuterFaceuse`: the face use of `f` on the outside of the solid (bounding its
/// infinite region), or `None`.
fn outer_faceuse(b: &Brep, f: FaceId) -> Result<Option<FaceuseId>> {
    let Some(fu) = crate::topology::face::p2::upward_faceuse(b, f)? else {
        return Ok(None);
    };
    let outside = match b.faceuse(fu)?.shell {
        Some(shell) => b.shell(shell)?.region == b.infinite_region(),
        None => false,
    };
    Ok(if outside {
        Some(fu)
    } else {
        b.faceuse(fu)?.mate
    })
}

/// The objects `FaceuseOffset` names: the B-rep's face uses, faces and face surfaces,
/// and the surfaces and curves it makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MoveHandle {
    Faceuse(FaceuseId),
    Face(FaceId),
    FaceSurface(FaceId),
    NewSurface(usize),
    NewCurve(usize),
}

/// `SmOffsetGeometryCreation::FaceuseOffset` on a face use of a B-rep: the B-rep read,
/// the offset surface (and any curve) kept here.
struct MoveBrep<'a> {
    b: &'a Brep,
    surfaces: Vec<Surface>,
    curves: Vec<crate::curve::curve::Curve>,
}

fn bad_handle() -> SolidError {
    SolidError::invalid_input(OP, "not an object of the face move")
}

impl FaceOffsetBrep<MoveHandle> for MoveBrep<'_> {
    fn faceuse_orientation(&self, faceuse: MoveHandle) -> Result<OrientType> {
        match faceuse {
            MoveHandle::Faceuse(fu) => Ok(self.b.faceuse(fu)?.orientation),
            _ => Err(bad_handle()),
        }
    }

    fn faceuse_face(&self, faceuse: MoveHandle) -> Result<Option<MoveHandle>> {
        match faceuse {
            MoveHandle::Faceuse(fu) => Ok(self.b.faceuse(fu)?.face.map(MoveHandle::Face)),
            _ => Err(bad_handle()),
        }
    }

    fn face_surface(&self, face: MoveHandle) -> Result<Option<MoveHandle>> {
        match face {
            MoveHandle::Face(f) => Ok(self
                .b
                .face(f)?
                .surface
                .as_ref()
                .map(|_| MoveHandle::FaceSurface(f))),
            _ => Err(bad_handle()),
        }
    }

    fn surface(&self, surface: MoveHandle) -> Result<&Surface> {
        match surface {
            MoveHandle::FaceSurface(f) => ner(self.b.face(f)?.surface.as_ref()),
            MoveHandle::NewSurface(i) => self.surfaces.get(i).ok_or_else(bad_handle),
            _ => Err(bad_handle()),
        }
    }

    fn insert_surface(&mut self, surface: Surface) -> Result<MoveHandle> {
        self.surfaces.push(surface);
        Ok(MoveHandle::NewSurface(self.surfaces.len() - 1))
    }

    fn insert_curve(&mut self, curve: crate::curve::curve::Curve) -> Result<MoveHandle> {
        self.curves.push(curve);
        Ok(MoveHandle::NewCurve(self.curves.len() - 1))
    }
}

/// Whether face `f` lies on one of `targets` (the fork marks the target surfaces and
/// asks whether a face's surface carries the mark; see the module deviations): nine
/// points of the face's surface over its UV domain within `tol` of a target.
fn on_target(k: &Kernel, b: &Brep, f: FaceId, targets: &[Surface], tol: f64) -> Result<bool> {
    let Some(s) = crate::topology::face::p2::surface(b, f)? else {
        return Ok(false);
    };
    let domain = crate::topology::face::p2::uv_domain(b, f)?;
    let mut points = Vec::new();
    for i in 0..3u32 {
        for j in 0..3u32 {
            let uv = domain.evaluate(f64::from(i) / 2.0, f64::from(j) / 2.0);
            points.push(s.evaluate_point(k, uv)?);
        }
    }
    for t in targets {
        let t_domain = t.natural_uv_domain_with_nurb(k)?;
        let mut all = true;
        for &p in &points {
            let d = t.drop_point::<NoObject>(k, p, &t_domain, &DropPointOptions::default())?;
            if !(d.success && d.gap <= tol) {
                all = false;
                break;
            }
        }
        if all {
            return Ok(true);
        }
    }
    Ok(false)
}

impl OffsetFacesExecutive {
    /// `OffsetFaces` step 4: the faces marked [`MARK_MOVED`] moved, each onto its
    /// surface's offset by `distance` along its outward normal (inward from its mate),
    /// the faces around extended or trimmed to meet it (`SmBrep::LocalOperation`); some
    /// face must then lie on one of the new surfaces.
    ///
    /// # Errors
    ///
    /// `SM_ERR_INVALID_INPUT` for a moved face without face use; `SM_ERR` for a face
    /// whose offset is no surface (collapsed onto its spine, or not made); those of
    /// `FaceuseOffset` and of the local operation; `SM_ERR` with
    /// [`OffsetFacesRefusal::NoEffect`] when no face lies on a new surface.
    pub fn move_faces(
        &mut self,
        k: &mut Kernel,
        b: &mut Brep,
        distance: f64,
        tol: f64,
    ) -> Result<()> {
        let make = OffsetGeometryCreation::new(distance.abs(), tol);
        let moved: Vec<FaceId> = crate::topology::brep::p1::faces(b, None)?
            .into_iter()
            .filter(|&f| FollowedBlendMark::has(b, Some(EntityRef::Face(f)), MARK_MOVED))
            .collect();
        let mut faces = Vec::new();
        let mut surfaces = Vec::new();
        for f in moved {
            let mut fu = outer_faceuse(b, f)?;
            if let (Some(x), true) = (fu, distance < 0.0) {
                fu = b.faceuse(x)?.mate;
            }
            let Some(fu) = fu else {
                return Err(SolidError::Upstream {
                    operation: OP,
                    code: crate::error::SM_ERR_INVALID_INPUT,
                    entity: None,
                });
            };
            let mut adapter = MoveBrep {
                b,
                surfaces: Vec::new(),
                curves: Vec::new(),
            };
            let (surface, _) =
                make.faceuse_offset_with_backend(k, &mut adapter, MoveHandle::Faceuse(fu), true)?;
            // A face whose offset collapses onto its spine has no surface to move to (and
            // since 758829c no other face's offset is missing without failing).
            let Some(MoveHandle::NewSurface(i)) = surface else {
                return Err(sm_err());
            };
            let surface = adapter.surfaces.into_iter().nth(i).ok_or_else(sm_err)?;
            faces.push(f);
            surfaces.push(surface);
        }
        let targets: Vec<Surface> = surfaces.clone();
        // The local operation takes the surfaces over, also when it fails.
        local_operation_simple(k, b, &faces, surfaces)?;

        if k.fixes.local_operation_closes_walls_to_nothing {
            return self.join_across_closed_walls(k, b, &targets, tol);
        }

        // Some face must lie on one of the surfaces, or the operation moved none.
        let mut moved_any = false;
        for f in crate::topology::brep::p1::faces(b, None)? {
            if on_target(k, b, f, &targets, tol)? {
                moved_any = true;
                break;
            }
        }
        if !moved_any {
            return Err(self.refuse(
                OffsetFacesRefusal::NoEffect,
                "the local operation reported success but moved no face onto its new surface",
                None,
            ));
        }
        Ok(())
    }

    /// The end of step 4 with `Kernel::fixes.local_operation_closes_walls_to_nothing`
    /// (fork 9504880, WR-566 addendum): the faces on the new surfaces collected, and the
    /// edges between such a face and one that stayed, which `SmEdge::IsTopological`
    /// finds removable, removed (`RemoveTopologicalEdgesAndVertices`), with the vertices
    /// at their ends where those join two edges on one curve. A face the move closed to
    /// nothing (a step wall) leaves the faces either side meeting on one surface; every
    /// other edge and vertex is kept. A face is on a new surface when it is a moved face
    /// ([`MARK_MOVED`]) lying on one ([`on_target`]): the fork's surface mark is on the
    /// moved faces' surfaces only, and a face that stayed may lie on the same plane. The
    /// "moved no face" refusal is unchanged (any face on a new surface).
    fn join_across_closed_walls(
        &mut self,
        k: &mut Kernel,
        b: &mut Brep,
        targets: &[Surface],
        tol: f64,
    ) -> Result<()> {
        let mut moved_any = false;
        let mut is_target = Vec::new();
        for f in crate::topology::brep::p1::faces(b, None)? {
            if on_target(k, b, f, targets, tol)? {
                moved_any = true;
                if FollowedBlendMark::has(b, Some(EntityRef::Face(f)), MARK_MOVED) {
                    is_target.push(f);
                }
            }
        }
        let mut joins: Vec<EdgeId> = Vec::new();
        for &f in &is_target {
            for e in face_edges(b, f)? {
                let beside = edge::faces(b, e, None)?;
                let to_stayed = beside.iter().any(|&g| g != f && !is_target.contains(&g));
                if to_stayed
                    && edge::is_topological(k, b, e, 1.0)?.is_topological
                    && !joins.contains(&e)
                {
                    joins.push(e);
                }
            }
        }
        if !moved_any {
            return Err(self.refuse(
                OffsetFacesRefusal::NoEffect,
                "the local operation reported success but moved no face onto its new surface",
                None,
            ));
        }
        if joins.is_empty() {
            return Ok(());
        }
        let mut keep_edges = crate::topology::brep::p1::edges(b, None)?;
        let all_vertices = crate::topology::brep::p1::vertices(b, None)?;
        let mut join_ends = Vec::new();
        for &e in &joins {
            if let Some(at) = keep_edges.iter().position(|&x| x == e) {
                keep_edges.remove(at);
            }
            let (s, t) = edge::start_end_vertices(b, e)?;
            for v in [s, t] {
                if !join_ends.contains(&v) {
                    join_ends.push(v);
                }
            }
        }
        let keep_vertices: Vec<_> = all_vertices
            .into_iter()
            .filter(|&v| !join_ends.contains(&Some(v)))
            .collect();
        remove_topological_edges_and_vertices(k, b, Some(&keep_edges), Some(&keep_vertices))
    }

    /// `OffsetFaces` step 5: the sharp edges each followed blend became ([`MARK_SHARP`]),
    /// where they still lie between its two faces, blended again in one fillet executive
    /// call at each blend's size and of its kind (circular sections), then
    /// `StitchAndOrient`. Every face is marked [`MARK_BEFORE`] first, for step 6.
    ///
    /// # Errors
    ///
    /// `SM_ERR` with [`OffsetFacesRefusal::NoRoom`] when a blend's edge is gone or the
    /// executive reports no room (its comment as the reason's detail); the executive's
    /// and the stitch's own errors.
    pub fn reblend(&mut self, k: &mut Kernel, b: &mut Brep, blends: &[Followed]) -> Result<()> {
        let mut edges = Vec::new();
        let mut sections = Vec::new();
        let mut solvers = Vec::new();
        let mut sizes = Vec::new();
        let all = crate::topology::brep::p1::edges(b, None)?;
        for (ii, rb) in blends.iter().enumerate() {
            let base = i64::try_from(ii).map_err(|_| sm_err())? * MARK_STRIDE;
            let mut found = 0usize;
            for &e in &all {
                if !FollowedBlendMark::has(b, Some(EntityRef::Edge(e)), base + MARK_SHARP) {
                    continue;
                }
                let on = edge::faces(b, e, None)?;
                let [f0, f1] = on.as_slice() else {
                    continue;
                };
                let (f0, f1) = (Some(EntityRef::Face(*f0)), Some(EntityRef::Face(*f1)));
                let s0 = base + MARK_SUPPORT;
                let s1 = base + MARK_SUPPORT + 1;
                // still where the blend's two faces meet
                if !edge::is_manifold(b, e)?
                    || !((FollowedBlendMark::has(b, f0, s0) && FollowedBlendMark::has(b, f1, s1))
                        || (FollowedBlendMark::has(b, f0, s1) && FollowedBlendMark::has(b, f1, s0)))
                {
                    continue;
                }
                edges.push(e);
                sections.push(FilletSurfaceGeneratorType::Circular);
                solvers.push(rb.solver);
                sizes.push(rb.size);
                found += 1;
            }
            if found == 0 {
                return Err(self.refuse(
                    OffsetFacesRefusal::NoRoom,
                    "the move took a blend's faces apart: the edge it rolls along is gone, so the blend no longer fits",
                    None,
                ));
            }
        }
        for f in crate::topology::brep::p1::faces(b, None)? {
            FollowedBlendMark::add(b, Some(EntityRef::Face(f)), MARK_BEFORE)?;
        }
        let (status, code, comment) = {
            let mut session = FilletSession::new(k, b)?;
            let status = session
                .prepare_per_edge_solvers(k, &edges, &sizes, &sections, &solvers, 1, 1.0)
                .and_then(|()| session.do_fillet(k));
            let info = session.error_info();
            (status, info.code, info.comment.clone())
        };
        if let Err(e) = status {
            if e.is_unported() {
                return Err(e);
            }
            if code == FilletErrorType::NoRoom as i32 {
                return Err(self.refuse(
                    OffsetFacesRefusal::NoRoom,
                    "a blend that follows a moved face no longer fits",
                    Some(&comment),
                ));
            }
            // (the fork's SM_FILERR_NOT_EXACT has no counterpart here: see the module
            // deviations)
            if !comment.is_empty() {
                self.reason = format!("putting back a blend that follows a moved face: {comment}");
            }
            return Err(e);
        }
        stitch_and_orient(k, b, &StitchAndOrientOptions::default())
    }
}

// ---- step 3 and OffsetFaces ------------------------------------------------------------

impl OffsetFacesExecutive {
    /// `OffsetFaces` step 3: each followed blend taken off, its faces met at the sharp
    /// edge it replaced (`SmRingFilletRemoval` for a ring, `SmFilletRemoval` exact only
    /// otherwise), in rounds while any comes off; the sharp edge marked [`MARK_SHARP`].
    /// Blends that meet each other at their ends (mitred rounds) come off afterwards,
    /// each connected group at once, through [`FeatureExecutive`] (the faces round the
    /// group extended until they meet), where every face round the group is a plane; the
    /// extended faces are joined again across the group's edges, and each blend's sharp
    /// edge is found where its two faces meet.
    ///
    /// # Errors
    ///
    /// `SM_ERR` with [`OffsetFacesRefusal::NotExact`] or
    /// [`OffsetFacesRefusal::Unsupported`] for a blend that could not be taken off (and
    /// for a mitred group that could not come off, that left no valid solid or that lost
    /// the edges it rounded); `SM_ERR` with
    /// [`OffsetFacesRefusal::Unsupported`] when the removals leave no closed solid; those
    /// of the topology queries and marks.
    #[allow(clippy::too_many_lines)] // one fork routine
    pub fn take_off(
        &mut self,
        k: &mut Kernel,
        b: &mut Brep,
        blends: &mut [Followed],
    ) -> Result<()> {
        let mut not_exact = false;
        let mut progress = true;
        while progress {
            progress = false;
            for (ii, rb) in blends.iter_mut().enumerate() {
                if rb.taken_off {
                    continue;
                }
                let blend = ner(rb.blend)?;
                let face_tol = crate::topology::face::p2::tolerance(b, blend)?;
                let removed = if RingFilletRemoval::is_ring(k, b, Some(blend), TANGENCY_TOL_DEG)? {
                    let mut removal = RingFilletRemoval::new(blend, face_tol);
                    let r = removal.remove_face(k, b);
                    // (not upstream: the port's cached topology fields)
                    b.refresh_cached_topology();
                    match r {
                        Ok(()) => Some(removal.sharp_edge()),
                        Err(e) if e.is_unported() => return Err(e),
                        Err(_) => {
                            not_exact = not_exact || removal.not_exact();
                            None
                        }
                    }
                } else {
                    let mut removal = FilletRemoval::new(blend, face_tol);
                    removal.set_exact_only(true);
                    let r = removal.remove_face(k, b);
                    b.refresh_cached_topology();
                    match r {
                        Ok(()) => Some(removal.sharp_edge()),
                        Err(e) if e.is_unported() => return Err(e),
                        Err(_) => {
                            not_exact = not_exact || removal.not_exact();
                            None
                        }
                    }
                };
                let Some(sharp) = removed else {
                    continue;
                };
                let sharp = ner(sharp)?;
                rb.blend = None; // deleted
                rb.rails = [None, None];
                rb.taken_off = true;
                let base = i64::try_from(ii).map_err(|_| sm_err())? * MARK_STRIDE;
                FollowedBlendMark::add(b, Some(EntityRef::Edge(sharp)), base + MARK_SHARP)?;
                progress = true;
            }
        }

        // Blends that meet each other at their ends (mitred rounds along the edges of a
        // face) cannot come off one by one exactly: each one's end lies on the next one's
        // surface. Each connected group of them comes off at once (SmFeatureExecutive: the
        // faces round the group extended until they meet), where every face round it is a
        // plane, which continues exactly.
        let left: Vec<usize> = (0..blends.len())
            .filter(|&i| blends.get(i).is_some_and(|x| !x.taken_off))
            .collect();
        let mut grouped = vec![false; blends.len()];
        for &seed in &left {
            if grouped.get(seed).copied().unwrap_or(true) {
                continue;
            }
            // the group: the blends left that share an edge with it
            let mut group = vec![seed];
            if let Some(g) = grouped.get_mut(seed) {
                *g = true;
            }
            let mut gg = 0;
            while let Some(&member) = group.get(gg) {
                let face = ner(blends.get(member).and_then(|x| x.blend))?;
                for e in face_edges(b, face)? {
                    let across = face_across(b, e, face)?;
                    for &other in &left {
                        let is_unseen = !grouped.get(other).copied().unwrap_or(true);
                        if is_unseen
                            && blends.get(other).and_then(|x| x.blend) == across
                            && across.is_some()
                        {
                            if let Some(g) = grouped.get_mut(other) {
                                *g = true;
                            }
                            group.push(other);
                        }
                    }
                }
                gg += 1;
            }
            if group.len() < 2 {
                continue;
            }
            let faces: Vec<FaceId> = group
                .iter()
                .filter_map(|&i| blends.get(i).and_then(|x| x.blend))
                .collect();
            // every face round the group a plane
            let mut planes = true;
            'faces: for &f in &faces {
                for e in face_edges(b, f)? {
                    match face_across(b, e, f)? {
                        None => {
                            planes = false;
                            break 'faces;
                        }
                        Some(across) if !faces.contains(&across) => {
                            let s = ner(crate::topology::face::p2::surface(b, across)?)?;
                            if !s.is_kind_of(Plane::TYPE)? {
                                planes = false;
                                break 'faces;
                            }
                        }
                        Some(_) => {}
                    }
                }
            }
            if !planes {
                not_exact = true;
                continue;
            }
            // the edges there before, but the group's own, keep their topology (faces split
            // on purpose stay apart; the faces round the group, extended, are joined across
            // its edges)
            {
                let all = crate::topology::brep::p1::edges(b, None)?;
                let mut group_edges = Vec::new();
                for &f in &faces {
                    group_edges.extend(face_edges(b, f)?);
                }
                for e in all {
                    if !group_edges.contains(&e) {
                        FollowedBlendMark::add(b, Some(EntityRef::Edge(e)), MARK_BEFORE)?;
                    }
                }
            }
            let applied = {
                let mut defeature = FeatureExecutive::new(k, b)?;
                let Some(feature) = defeature.add_feature(
                    k,
                    &faces,
                    RebuildBrepType::ExtendSurface,
                    RebuildBrepType::FromOtherBrep,
                )?
                else {
                    continue;
                };
                // (DoDefeature always succeeds and hands back the base B-rep itself)
                defeature.do_defeature(k)?;
                defeature
                    .features()
                    .get(feature)
                    .is_some_and(|f| f.base_was_applied)
            };
            // (not upstream: the port's cached topology fields)
            b.refresh_cached_topology();
            if !applied {
                return Err(self.refuse(
                    OffsetFacesRefusal::Unsupported,
                    "mitred blends on a moved face could not be taken off together",
                    None,
                ));
            }
            // the extended faces were split where the blends had been: join them again
            let mut keep = Vec::new();
            for e in crate::topology::brep::p1::edges(b, None)? {
                if FollowedBlendMark::has(b, Some(EntityRef::Edge(e)), MARK_BEFORE) {
                    keep.push(e);
                }
            }
            remove_topological_edges_and_vertices(k, b, Some(&keep), None)?;
            b.refresh_cached_topology();
            // as SmApiRemoveFillet's defeature: the rebuilt body valid, and taken in afresh
            let valid = {
                let naming = b.naming();
                let recorded = b.geometry_owner_keys().clone();
                let keys = |e: EntityRef| {
                    crate::topology::owner_identity_of(naming, recorded.get(&e).copied(), e)
                };
                assert_valid(k, b, None, 2, true, &keys)?
            };
            if !valid || !crate::topology::brep::p2::is_manifold_solid(b)? {
                return Err(self.refuse(
                    OffsetFacesRefusal::Unsupported,
                    "mitred blends on a moved face came off leaving no valid solid",
                    None,
                ));
            }
            // each blend's sharp edge: where its two faces meet now
            let now = crate::topology::brep::p1::edges(b, None)?;
            for &lb in &group {
                let base = i64::try_from(lb).map_err(|_| sm_err())? * MARK_STRIDE;
                let mut found = false;
                for &e in &now {
                    let on = edge::faces(b, e, None)?;
                    if let [f0, f1] = on.as_slice() {
                        let (f0, f1) = (Some(EntityRef::Face(*f0)), Some(EntityRef::Face(*f1)));
                        let s0 = base + MARK_SUPPORT;
                        let s1 = base + MARK_SUPPORT + 1;
                        if (FollowedBlendMark::has(b, f0, s0) && FollowedBlendMark::has(b, f1, s1))
                            || (FollowedBlendMark::has(b, f1, s0)
                                && FollowedBlendMark::has(b, f0, s1))
                        {
                            FollowedBlendMark::add(b, Some(EntityRef::Edge(e)), base + MARK_SHARP)?;
                            found = true;
                        }
                    }
                }
                if !found {
                    return Err(self.refuse(
                        OffsetFacesRefusal::Unsupported,
                        "mitred blends on a moved face came off without the edges they rounded",
                        None,
                    ));
                }
                if let Some(rb) = blends.get_mut(lb) {
                    rb.blend = None; // deleted
                    rb.rails = [None, None];
                    rb.taken_off = true;
                }
            }
        }
        if blends.iter().any(|x| !x.taken_off) {
            return Err(if not_exact {
                self.refuse(
                    OffsetFacesRefusal::NotExact,
                    "a blend on a moved face could not be taken off: its faces would have to be continued past their surfaces or edges, which SMLib cannot do exactly",
                    None,
                )
            } else {
                self.refuse(
                    OffsetFacesRefusal::Unsupported,
                    "a blend on a moved face could not be taken off its faces (its ends meet other blends or corners SMLib cannot take apart)",
                    None,
                )
            });
        }
        if !blends.is_empty() && !crate::topology::brep::p2::is_manifold_solid(b)? {
            return Err(self.refuse(
                OffsetFacesRefusal::Unsupported,
                "taking the blends off the moved faces left no closed solid",
                None,
            ));
        }
        Ok(())
    }

    /// `OffsetFaces(crFaces, dDistance)`: moves `faces` of `b` `distance` along their
    /// outward normals (negative: into the body), the tangent blends on them following
    /// (the steps of [`Self::collect`], [`Self::keep_and_mark`], [`Self::take_off`],
    /// [`Self::move_faces`], [`Self::reblend`] and [`Self::restore`]). Editing is enabled
    /// for the call. On failure the B-rep is left in an unspecified state (as for
    /// `SmBrep::LocalOperation`): work on a copy.
    ///
    /// # Errors
    ///
    /// `SM_ERR_INVALID_INPUT` for no faces or a zero (or not finite) distance; the
    /// refusals of the steps ([`Self::refusal`], [`Self::reason`]); those of the steps.
    pub fn offset_faces(
        &mut self,
        k: &mut Kernel,
        b: &mut Brep,
        faces: &[FaceId],
        distance: f64,
    ) -> Result<()> {
        self.refusal = OffsetFacesRefusal::None;
        self.reason.clear();
        self.followed = 0;
        let distance_ok = distance.abs() > 0.0;
        if faces.is_empty() || !distance_ok {
            return Err(SolidError::Upstream {
                operation: OP,
                code: crate::error::SM_ERR_INVALID_INPUT,
                entity: None,
            });
        }
        // (SmTemporaryChangeValue on m_bEditingEnabled)
        let editing = b.editing_enabled();
        b.set_editing_enabled(true);
        let r = self.offset_faces_editing(k, b, faces, distance);
        b.set_editing_enabled(editing);
        // (not upstream: the port's cached topology fields, true for the caller)
        b.refresh_cached_topology();
        r
    }

    fn offset_faces_editing(
        &mut self,
        k: &mut Kernel,
        b: &mut Brep,
        faces: &[FaceId],
        distance: f64,
    ) -> Result<()> {
        let tol = b.tolerance();

        // The faces to move, marked: taking blends off can replace a face by another
        // piece of it.
        for &f in faces {
            FollowedBlendMark::add(b, Some(EntityRef::Face(f)), MARK_MOVED)?;
        }

        // 1. The blends that follow.
        let mut blends = self.collect(k, b, faces, tol)?;
        // 2. What each stands for, kept, and marks on the faces around it.
        self.keep_and_mark(b, &mut blends)?;
        // 3. Each taken off, its faces met at the sharp edge.
        self.take_off(k, b, &mut blends)?;
        // 4. The faces moved.
        self.move_faces(k, b, distance, tol)?;
        if blends.is_empty() {
            return FollowedBlendMark::remove_from_topology(b);
        }
        // 5. The sharp edges blended again, all in one call.
        self.reblend(k, b, &blends)?;
        // 6. The new blend faces stand for the blends they replace.
        Self::restore(b, &blends)?;
        FollowedBlendMark::remove_from_topology(b)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;
    use crate::blend::fillet_executive::api::fillet_edges;
    use crate::boolean::api_brep::boolean_union;
    use crate::geom::attribute::AttributeStore;
    use crate::math::{Point3, Vec3};
    use crate::primitive::api_primitives::{create_box, create_cylinder};
    use crate::topology::vertex;

    /// The codes of `e`'s mark (empty without one).
    fn codes(b: &Brep, e: EntityRef) -> Vec<i64> {
        FollowedBlendMark::on(b, e)
            .map(|(_, m)| m.codes.clone())
            .unwrap_or_default()
    }

    /// The edge of `b` whose two vertices satisfy `on`.
    fn edge_where(b: &Brep, on: impl Fn(Point3) -> bool) -> EdgeId {
        b.edges()
            .ids()
            .find(|&e| {
                let (s, t) = edge::start_end_vertices(b, e).unwrap();
                on(vertex::point(b, s.unwrap()).unwrap())
                    && on(vertex::point(b, t.unwrap()).unwrap())
            })
            .unwrap()
    }

    /// The face of `b` all of whose vertices satisfy `on`.
    fn face_where(b: &Brep, on: impl Fn(Point3) -> bool) -> FaceId {
        *b.face_list()
            .iter()
            .find(|&&f| {
                face_edges(b, f).unwrap().iter().all(|&e| {
                    let (s, t) = edge::start_end_vertices(b, e).unwrap();
                    [s, t]
                        .into_iter()
                        .flatten()
                        .all(|v| on(vertex::point(b, v).unwrap()))
                })
            })
            .unwrap()
    }

    /// Whether `f`'s surface is a plane.
    fn face_surface_is_plane(b: &Brep, f: FaceId) -> bool {
        crate::topology::face::p2::surface(b, f)
            .unwrap()
            .unwrap()
            .is_kind_of(Plane::TYPE)
            .unwrap()
    }

    /// A 60 x 40 x 20 block, its top front edge rounded r 2.
    fn block_with_top_front_round(k: &mut Kernel) -> Brep {
        let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 20.0).unwrap();
        let e = edge_where(&b, |p| p.y.abs() < 1e-6 && (p.z - 20.0).abs() < 1e-6);
        fillet_edges(k, &mut b, &[e], 2.0, 1, 1, 1.0).unwrap();
        b
    }

    /// The 60 x 40 x 10 plate with the boss r 10, 10 high, at (30, 20), its foot rounded
    /// r 2 (the fork's rig case shell/3468).
    fn boss_with_foot_round(k: &mut Kernel) -> Brep {
        let plate = create_box(k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 10.0).unwrap();
        let boss = create_cylinder(k, Vec3::new(30.0, 20.0, 10.0), 10.0, 10.0).unwrap();
        let mut b = boolean_union(k, plate, boss).unwrap();
        let foot = b
            .edges()
            .ids()
            .find(|&e| {
                let v = edge::start_vertex(&b, e).unwrap().unwrap();
                edge::is_closed(&b, e).unwrap()
                    && (vertex::point(&b, v).unwrap().z - 10.0).abs() < 1e-9
            })
            .unwrap();
        fillet_edges(k, &mut b, &[foot], 2.0, 1, 1, 1.0).unwrap();
        b
    }

    #[test]
    fn merged_marks_keep_both_objects_codes() {
        let mut store = AttributeStore::new();
        let (a, c) = (ObjectKey(1), ObjectKey(2));
        let ha = store
            .insert(FollowedBlendMark::attribute(vec![1, 2]))
            .unwrap();
        store.add_attribute(a, ha).unwrap();
        let hc = store
            .insert(FollowedBlendMark::attribute(vec![2, 3]))
            .unwrap();
        store.add_attribute(c, hc).unwrap();
        store.merge(ha, a, c, a).unwrap();
        let h = store.find_attribute(a, ids::FOLLOWED_BLEND).unwrap();
        let m = store
            .get(h)
            .unwrap()
            .application_value::<FollowedBlendMark>()
            .unwrap();
        assert_eq!(m.codes, [1, 2, 3]);
    }

    #[test]
    fn marks_add_has_and_clear() {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 1.0, 1.0, 1.0).unwrap();
        let f = EntityRef::Face(b.face_list()[0]);
        FollowedBlendMark::add(&mut b, Some(f), MARK_MOVED).unwrap();
        FollowedBlendMark::add(&mut b, Some(f), 64 + MARK_SHARP).unwrap();
        FollowedBlendMark::add(&mut b, Some(f), MARK_MOVED).unwrap();
        assert_eq!(codes(&b, f), [MARK_MOVED, 64 + MARK_SHARP]);
        assert!(FollowedBlendMark::has(&b, Some(f), MARK_MOVED));
        assert!(!FollowedBlendMark::has(&b, Some(f), MARK_BEFORE));
        FollowedBlendMark::add(&mut b, None, MARK_BEFORE).unwrap();
        FollowedBlendMark::clear(&mut b, f).unwrap();
        assert!(codes(&b, f).is_empty());
        FollowedBlendMark::add(&mut b, Some(f), MARK_TARGET).unwrap();
        FollowedBlendMark::remove_from_topology(&mut b).unwrap();
        assert!(codes(&b, f).is_empty());
    }

    #[test]
    fn the_boss_foot_round_follows_the_boss_wall() {
        let mut k = Kernel::default();
        let mut b = boss_with_foot_round(&mut k);
        // the wall: the curved face all of whose vertices are at radius 10 above z 11.9
        // (its seam's ends at the top rim and at the round's rail; the top disc is flat)
        let wall = *b
            .face_list()
            .iter()
            .find(|&&f| {
                let curved = !face_surface_is_plane(&b, f);
                curved
                    && face_edges(&b, f).unwrap().iter().all(|&e| {
                        let (s, t) = edge::start_end_vertices(&b, e).unwrap();
                        [s, t].into_iter().flatten().all(|v| {
                            let p = vertex::point(&b, v).unwrap();
                            ((p.x - 30.0).powi(2) + (p.y - 20.0).powi(2)).sqrt() > 9.99
                                && p.z > 11.9
                        })
                    })
            })
            .unwrap();
        let tol = b.tolerance();
        let mut ex = OffsetFacesExecutive::new();
        let blends = ex.collect(&k, &mut b, &[wall], tol).unwrap();
        assert_eq!(ex.followed_blends(), 1);
        assert_eq!(ex.refusal(), OffsetFacesRefusal::None);
        let rb = &blends[0];
        assert_eq!(rb.solver, FilletSolverType::ConstRadius);
        assert!((rb.size - 2.0).abs() < 1e-9, "radius {}", rb.size);
        let blend = rb.blend.unwrap();
        let mut supports: Vec<FaceId> = rb
            .rails
            .iter()
            .map(|r| face_across(&b, r.unwrap(), blend).unwrap().unwrap())
            .collect();
        supports.sort();
        assert!(supports.contains(&wall));
        assert_eq!(supports.len(), 2);
    }

    #[test]
    fn the_top_front_round_follows_the_top_and_keeps_what_it_stood_for() {
        let mut k = Kernel::default();
        let mut b = block_with_top_front_round(&mut k);
        let top = face_where(&b, |p| (p.z - 20.0).abs() < 1e-6);
        let tol = b.tolerance();
        let mut ex = OffsetFacesExecutive::new();
        let mut blends = ex.collect(&k, &mut b, &[top], tol).unwrap();
        assert_eq!(blends.len(), 1);
        assert_eq!(blends[0].solver, FilletSolverType::ConstRadius);
        assert!(
            (blends[0].size - 2.0).abs() < 1e-9,
            "radius {}",
            blends[0].size
        );

        // a history tag on the round (copied with it) and a temporary one (not kept)
        let blend = blends[0].blend.unwrap();
        b.add_attribute(
            EntityRef::Face(blend),
            Attribute::long(20001, 7, Behavior::Copy),
        )
        .unwrap();
        b.add_attribute(
            EntityRef::Face(blend),
            Attribute::new(20002, Behavior::Temp),
        )
        .unwrap();
        ex.keep_and_mark(&mut b, &mut blends).unwrap();
        let kept: Vec<u64> = blends[0]
            .face_attrs
            .iter()
            .map(Attribute::attribute_id)
            .collect();
        assert_eq!(kept, [20001]);
        // the faces it rolls on and its two end faces are marked
        let front = face_where(&b, |p| (p.y - 0.0).abs() < 1e-6);
        assert!(
            FollowedBlendMark::has(&b, Some(EntityRef::Face(top)), MARK_SUPPORT)
                || FollowedBlendMark::has(&b, Some(EntityRef::Face(top)), MARK_SUPPORT + 1)
        );
        assert!(
            FollowedBlendMark::has(&b, Some(EntityRef::Face(front)), MARK_SUPPORT)
                || FollowedBlendMark::has(&b, Some(EntityRef::Face(front)), MARK_SUPPORT + 1)
        );
        assert_eq!(blends[0].end_attrs.len(), 2);
        for x in [0.0, 60.0] {
            let end = face_where(&b, |p| (p.x - x).abs() < 1e-6);
            let c = codes(&b, EntityRef::Face(end));
            assert!(
                c.contains(&MARK_END) || c.contains(&(MARK_END + 1)),
                "{c:?}"
            );
        }

        // restored onto a face beside both supports that was not there before: the
        // round itself, every other face marked as there before
        for f in b.face_list().to_vec() {
            if f != blend {
                FollowedBlendMark::add(&mut b, Some(EntityRef::Face(f)), MARK_BEFORE).unwrap();
            }
        }
        b.clear_attributes(EntityRef::Face(blend)).unwrap();
        OffsetFacesExecutive::restore(&mut b, &blends).unwrap();
        let tag = b.find_attribute(EntityRef::Face(blend), 20001).unwrap();
        assert_eq!(tag.long_value(), Some(7));
    }

    #[test]
    fn a_face_without_tangent_neighbours_has_no_followed_blend() {
        let mut k = Kernel::default();
        let mut b = block_with_top_front_round(&mut k);
        let bottom = face_where(&b, |p| (p.z - 0.0).abs() < 1e-6);
        let tol = b.tolerance();
        let mut ex = OffsetFacesExecutive::new();
        assert!(ex.collect(&k, &mut b, &[bottom], tol).unwrap().is_empty());
        assert_eq!(ex.followed_blends(), 0);
    }

    /// The volume at the integration tolerance 1e-9.
    fn volume(k: &Kernel, b: &mut Brep) -> f64 {
        let store = AttributeStore::default();
        let attrs = crate::mass::integrator::properties::PropertyAttributes {
            store: &store,
            key_of: &|_| ObjectKey(0),
        };
        crate::mass::integrator::properties::brep_volume(k, b, 1e-9, &attrs).unwrap()
    }

    #[test]
    fn moved_top_and_a_round_put_back_on_its_sharp_edge() {
        // The state after step 3 on the block's top front round: the sharp block, its
        // top front edge marked as the round's sharp edge, the top and front as the faces
        // it rolls on, the top to move.
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 20.0).unwrap();
        let top = face_where(&b, |p| (p.z - 20.0).abs() < 1e-6);
        let front = face_where(&b, |p| (p.y - 0.0).abs() < 1e-6);
        let sharp = edge_where(&b, |p| p.y.abs() < 1e-6 && (p.z - 20.0).abs() < 1e-6);
        FollowedBlendMark::add(&mut b, Some(EntityRef::Face(top)), MARK_MOVED).unwrap();
        FollowedBlendMark::add(&mut b, Some(EntityRef::Face(top)), MARK_SUPPORT).unwrap();
        FollowedBlendMark::add(&mut b, Some(EntityRef::Face(front)), MARK_SUPPORT + 1).unwrap();
        FollowedBlendMark::add(&mut b, Some(EntityRef::Edge(sharp)), MARK_SHARP).unwrap();
        let blends = [Followed {
            blend: None,
            rails: [None, None],
            solver: FilletSolverType::ConstRadius,
            size: 2.0,
            face_attrs: vec![Attribute::long(20001, 7, Behavior::Copy)],
            rail_attrs: [Vec::new(), Vec::new()],
            seam_attrs: Vec::new(),
            end_attrs: Vec::new(),
            end_across: Vec::new(),
            taken_off: true,
        }];
        let tol = b.tolerance();
        let mut ex = OffsetFacesExecutive::new();
        ex.move_faces(&mut k, &mut b, 1.0, tol).unwrap();
        ex.reblend(&mut k, &mut b, &blends).unwrap();
        OffsetFacesExecutive::restore(&mut b, &blends).unwrap();
        FollowedBlendMark::remove_from_topology(&mut b).unwrap();

        // the block 21 high, its top front edge rounded r 2
        assert_eq!(b.face_list().len(), 7);
        let want = 60.0 * 40.0 * 21.0 - 60.0 * 4.0 * (1.0 - crate::math::PI / 4.0);
        let got = volume(&k, &mut b);
        assert!(
            (got - want).abs() <= 1e-9 * want,
            "volume {got}, expected {want}"
        );
        // the new round stands for the old one
        let round = *b
            .face_list()
            .iter()
            .find(|&&f| !face_surface_is_plane(&b, f))
            .unwrap();
        let tag = b.find_attribute(EntityRef::Face(round), 20001).unwrap();
        assert_eq!(tag.long_value(), Some(7));
        assert!(b
            .attributed_entities()
            .iter()
            .all(|&e| FollowedBlendMark::on(&b, e).is_none()));
    }

    #[test]
    fn a_blend_whose_edge_is_gone_no_longer_fits() {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
        let blends = [Followed {
            blend: None,
            rails: [None, None],
            solver: FilletSolverType::ConstRadius,
            size: 1.0,
            face_attrs: Vec::new(),
            rail_attrs: [Vec::new(), Vec::new()],
            seam_attrs: Vec::new(),
            end_attrs: Vec::new(),
            end_across: Vec::new(),
            taken_off: true,
        }];
        let mut ex = OffsetFacesExecutive::new();
        assert!(ex.reblend(&mut k, &mut b, &blends).is_err());
        assert_eq!(ex.refusal(), OffsetFacesRefusal::NoRoom);
        assert!(ex.reason().contains("no longer fits"), "{}", ex.reason());
    }

    #[test]
    fn a_move_onto_the_offset_surface_lies_on_it() {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
        let top = face_where(&b, |p| (p.z - 10.0).abs() < 1e-6);
        FollowedBlendMark::add(&mut b, Some(EntityRef::Face(top)), MARK_MOVED).unwrap();
        let tol = b.tolerance();
        let mut ex = OffsetFacesExecutive::new();
        ex.move_faces(&mut k, &mut b, -2.0, tol).unwrap();
        let got = volume(&k, &mut b);
        assert!((got - 800.0).abs() <= 1e-9 * 800.0, "volume {got}");
    }
}
