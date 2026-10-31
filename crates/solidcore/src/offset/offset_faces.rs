//! Not upstream: moving faces of a solid along their normals, the tangent blends on them
//! following (the SolidCraft fork's `SmOffsetFacesExecutive`, WR-615).
//!
//! Fork: tangency-labs/smlib a96ce9f, 607d617 and 758829c, `source/SMLib/src/SmOffsetFaces.cpp`,
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
//!
//! Deviations:
//! - **Scope (this change).** The marks, the collection of the followed blends, the
//!   kept attributes and their restoring (steps 1, 2 and 6 of `OffsetFaces`); taking the
//!   blends off, the move and the re-blend (steps 3 to 5) and `OffsetFaces` itself follow
//!   in a later change (#1060 step 4, part 2).
//! - **Marks.** `SmFollowedBlendMark` is an application attribute
//!   ([`crate::geom::attribute::AttributeHandlers`]) whose merge unions the two objects'
//!   codes, as the fork's `Merge`. A code added to an object that already has a mark
//!   replaces the mark by one with the code appended (the fork appends in place); marks are
//!   only looked up by id.
//! - **Kept attributes** are owned copies ([`Attribute::make_copy`]) instead of heap
//!   pointers freed by a scope guard.
//! - **Refusals** return [`SolidError::Upstream`] `SM_ERR` with the refusal and its reason
//!   on the executive, as the fork's `Refuse` returns `SM_ERR`.
//! - No recursion.

use crate::blend::fillet_executive::p1::is_radius_supported;
use crate::blend::fillet_solver::p1::FilletSolverType;
use crate::blend::identify_fillet::{identify_fillet, IdentifyFilletOptions};
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::attribute::{
    ids, ApplicationValue, Attribute, AttributeHandle, AttributeHandlers, AttributeValue,
    AttributeView, Behavior, ObjectKey, Placement,
};
use crate::geom::global_solver::SolverOperation;
use crate::kernel::Kernel;
use crate::nurbs::knots::Side;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::p5::DropPointOptions;
use crate::topology::edgeuse::p1::primary_edgeuse;
use crate::topology::edgeuse::p3::normalized_evaluate;
use crate::topology::{edge, Brep, EdgeId, FaceId};

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
            .is_kind_of(crate::surface::plane::Plane::TYPE)
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
}
