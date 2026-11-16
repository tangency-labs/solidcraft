//! The check that every face a blend makes is a definition (tangency-labs/solidcore#2489).
//!
//! Not upstream. #2388's direction is that every blend face is its definition S with a
//! certified net B: a round, an exact-class round or a chamfer a
//! [`ProceduralBlend`](crate::surface::procedural_blend::ProceduralBlend), a corner a
//! corner definition ([`BallPatch`](crate::surface::ball_patch::BallPatch) today), and
//! the offset of a blend the blend of the offset definition. This module names the faces
//! that are still a bare surface.
//!
//! | Rust | Role |
//! |------|------|
//! | [`BlendFaceKind`], [`CornerKind`] | what made a face |
//! | [`MadeBlendFace`] | a face an operation made, with its kind |
//! | [`BlendFaceLog`] | the faces the blends on one kernel made, while it is switched on |
//! | [`is_definition`] | whether a surface is a blend's definition |
//! | [`bare_blend_faces`] | the made faces whose surface is not a definition |
//! | [`mark_blend_faces`], [`blend_face_marks`] | marks carried by a later operation's faces |
//! | [`offsets_of_blends`] | the faces of an offset that are offsets of blend faces |
//!
//! Where the faces come from: a fillet session marks each face of its intermediate body
//! with its kind (its solver's section and class, or its corner's event) before the merge,
//! reads the marks back from the target after it and appends the target's faces to
//! [`crate::kernel::Kernel::blend_face_log`] when that is on; nothing is marked or kept
//! when it is off (the default). A later operation on blend faces is followed by marking
//! them on its operand ([`mark_blend_faces`]): its result's faces made from them carry the
//! marks, as each offset face carries its source face's attributes
//! ([`offsets_of_blends`]).
//!
//! Deviations: none (not upstream). No recursion.

use crate::blend::blend_corner::{BlendCorner, BlendCornerEvent, BlendTurn};
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::attribute::{ids, Attribute, AttributeValue, Behavior, TagData};
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::{Brep, FaceId};

const OP: &str = "blend::definition_check";

/// The corner events of [`crate::blend::blend_corner`], as the kind of the faces a corner
/// makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CornerKind {
    /// The corner ball's patch (`SM_BCE_BALL`).
    Ball,
    /// The setback fill (`SM_BCE_SETBACK`).
    Setback,
    /// An End's faces (`SM_BCE_END`).
    End,
    /// A Join's faces (`SM_BCE_JOIN`).
    Join,
    /// A Mitred Turn's faces (`SM_BCE_TURN`, `SM_BT_MITRE`).
    Mitre,
    /// A Swept Turn's faces (`SM_BCE_TURN`, `SM_BT_SWEPT`).
    Swept,
    /// A Mixed corner's faces (`SM_BCE_MIXED`).
    Mixed,
    /// The chamfer event's faces (not the fork's; WR-716, #2345): a Facet's plane, or a
    /// corner patch's bilinear quadrilaterals round the crossings' centroid.
    Chamfer,
    /// A corner the blend corner does not build: the pinned corner hierarchy
    /// (`SmFilletCorner` and its subclasses, #2400), or a blend corner of no event.
    Pinned,
}

impl CornerKind {
    /// The kind of the faces of a corner whose blend corner is `blend` (`None` for a
    /// pinned corner).
    #[must_use]
    pub fn of(blend: Option<BlendCorner>) -> Self {
        let Some(blend) = blend else {
            return Self::Pinned;
        };
        match blend.event {
            BlendCornerEvent::Ball => Self::Ball,
            BlendCornerEvent::Setback => Self::Setback,
            BlendCornerEvent::End => Self::End,
            BlendCornerEvent::Join => Self::Join,
            BlendCornerEvent::Turn => match blend.turn {
                BlendTurn::Mitre => Self::Mitre,
                BlendTurn::Swept => Self::Swept,
            },
            BlendCornerEvent::Mixed => Self::Mixed,
            BlendCornerEvent::Chamfer => Self::Chamfer,
            BlendCornerEvent::None => Self::Pinned,
        }
    }

    /// The kind's name, as the ledger writes it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ball => "ball",
            Self::Setback => "setback",
            Self::End => "end",
            Self::Join => "join",
            Self::Mitre => "mitre",
            Self::Swept => "swept",
            Self::Mixed => "mixed",
            Self::Chamfer => "chamfer",
            Self::Pinned => "pinned",
        }
    }
}

/// What made a blend face.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BlendFaceKind {
    /// A traced round: a circular, blend-curve or Gⁿ section along its trace.
    Round,
    /// A round built by its exact class (`fillet_exact_round_classes`, WR-669).
    ExactRound,
    /// A chamfer: a linear section, traced or by its exact class.
    Chamfer,
    /// A face of a corner.
    Corner(CornerKind),
    /// The offset of a blend face.
    Offset,
}

impl BlendFaceKind {
    /// The kind's name, as the ledger writes it: `round`, `exact-round`, `chamfer`,
    /// `corner/<event>` or `offset`.
    #[must_use]
    pub fn name(self) -> String {
        match self {
            Self::Round => "round".into(),
            Self::ExactRound => "exact-round".into(),
            Self::Chamfer => "chamfer".into(),
            Self::Corner(corner) => format!("corner/{}", corner.name()),
            Self::Offset => "offset".into(),
        }
    }

    /// The code a fillet session's face mark carries (its tag's primary id).
    pub(crate) const fn code(self) -> i64 {
        match self {
            Self::Round => 1,
            Self::ExactRound => 2,
            Self::Chamfer => 3,
            Self::Offset => 4,
            Self::Corner(corner) => match corner {
                CornerKind::Ball => 10,
                CornerKind::Setback => 11,
                CornerKind::End => 12,
                CornerKind::Join => 13,
                CornerKind::Mitre => 14,
                CornerKind::Swept => 15,
                CornerKind::Mixed => 16,
                CornerKind::Pinned => 17,
                CornerKind::Chamfer => 18,
            },
        }
    }

    /// The kind whose [`Self::code`] is `code`.
    pub(crate) const fn from_code(code: i64) -> Option<Self> {
        Some(match code {
            1 => Self::Round,
            2 => Self::ExactRound,
            3 => Self::Chamfer,
            4 => Self::Offset,
            10 => Self::Corner(CornerKind::Ball),
            11 => Self::Corner(CornerKind::Setback),
            12 => Self::Corner(CornerKind::End),
            13 => Self::Corner(CornerKind::Join),
            14 => Self::Corner(CornerKind::Mitre),
            15 => Self::Corner(CornerKind::Swept),
            16 => Self::Corner(CornerKind::Mixed),
            17 => Self::Corner(CornerKind::Pinned),
            18 => Self::Corner(CornerKind::Chamfer),
            _ => return None,
        })
    }
}

/// A face an operation made, with what made it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MadeBlendFace {
    /// The face, in the operation's result.
    pub face: FaceId,
    /// What made it.
    pub kind: BlendFaceKind,
}

/// The blend faces the fillet sessions on one kernel made, in the order they were made:
/// one entry per session that finished filleting, its faces in the target's face order.
/// Kept only while [`crate::kernel::Kernel::blend_face_log`] is `Some`; it changes no
/// result.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BlendFaceLog {
    /// Per finished session, the faces it made in its target.
    pub sessions: Vec<Vec<MadeBlendFace>>,
}

impl BlendFaceLog {
    /// The faces the last finished session made (empty when none finished).
    #[must_use]
    pub fn last(&self) -> &[MadeBlendFace] {
        self.sessions.last().map_or(&[], Vec::as_slice)
    }
}

/// A blend face whose surface is not a definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BareBlendFace {
    /// The face.
    pub face: FaceId,
    /// What made it.
    pub kind: BlendFaceKind,
    /// The kind of its surface ([`surface_kind_name`]).
    pub surface: &'static str,
}

/// Whether `surface` is a blend's definition: the definition accessor's answer
/// ([`Surface::procedural_blend`]), whatever the surface's kind: a procedural blend, or a
/// round of an exact class (a chamfer between planes among them) keeping its definition
/// behind its analytic kind (#2485).
///
/// The one place the check decides it.
#[must_use]
pub fn is_definition(surface: &Surface) -> bool {
    surface.procedural_blend().is_some()
}

/// The name of `surface`'s kind, as [`BareBlendFace::surface`] gives it.
#[must_use]
pub fn surface_kind_name(surface: &Surface) -> &'static str {
    match surface.kind() {
        SurfaceKind::Cone(_) => "cone",
        SurfaceKind::BSpline(_) => "b-spline",
        SurfaceKind::CurveBounded(_) => "curve-bounded",
        SurfaceKind::Plane(_) => "plane",
        SurfaceKind::SurfOfRevolution(_) => "surface-of-revolution",
        SurfaceKind::Step(_) => "step",
        SurfaceKind::OffsetSurface(_) => "offset",
        SurfaceKind::SrfInVolume(_) => "surface-in-volume",
        SurfaceKind::Sphere(_) => "sphere",
        SurfaceKind::Torus(_) => "torus",
        SurfaceKind::SurfOfExtrusion(_) => "surface-of-extrusion",
        SurfaceKind::ProceduralBlend(_) => "procedural-blend",
        SurfaceKind::BallPatch(_) => "ball-patch",
    }
}

fn not_a_face(face: FaceId) -> SolidError {
    SolidError::NotFound {
        operation: OP,
        entity: EntityRef::Face(face),
    }
}

fn no_surface(face: FaceId) -> SolidError {
    SolidError::Degenerate {
        operation: OP,
        entity: Some(EntityRef::Face(face)),
    }
}

/// The faces of `made` (faces of `brep`) whose surface is not a definition
/// ([`is_definition`]), in `made`'s order, each with its kind and its surface's kind.
///
/// # Errors
/// [`SolidError::NotFound`] for a face of `made` that is not a live face of `brep`, and
/// [`SolidError::Degenerate`] naming one without a surface: `made` does not describe
/// `brep`.
pub fn bare_blend_faces(brep: &Brep, made: &[MadeBlendFace]) -> Result<Vec<BareBlendFace>> {
    let mut out = Vec::new();
    for m in made {
        let face = brep.face(m.face).map_err(|_| not_a_face(m.face))?;
        let surface = face.surface.as_ref().ok_or_else(|| no_surface(m.face))?;
        if !is_definition(surface) {
            out.push(BareBlendFace {
                face: m.face,
                kind: m.kind,
                surface: surface_kind_name(surface),
            });
        }
    }
    Ok(out)
}

/// Marks each face of `made` (faces of `brep`) with what made it: a tag attribute
/// [`ids::FILLET_BLEND_FACE`] whose primary id is the kind's code, passed on to the faces
/// a later operation makes from it ([`Behavior::Copy`]), so [`blend_face_marks`] finds
/// them in that operation's result. Each offset face is a copy of its source face's
/// attributes, so an offset's faces made from blend faces carry their marks.
///
/// # Errors
/// [`SolidError::NotFound`] for a face of `made` that is not a live face of `brep`.
pub fn mark_blend_faces(brep: &mut Brep, made: &[MadeBlendFace]) -> Result<()> {
    for m in made {
        let mark = Attribute::tag(
            ids::FILLET_BLEND_FACE,
            TagData {
                primary_id: m.kind.code(),
                ..TagData::default()
            },
            Behavior::Copy,
        );
        brep.add_attribute(EntityRef::Face(m.face), mark)?;
    }
    Ok(())
}

/// The faces of `brep` carrying a [`mark_blend_faces`] mark, in its face order, each
/// with the kind its mark names.
///
/// # Errors
/// [`SolidError::InvalidInput`] for a mark that is not a tag or names no kind.
pub fn blend_face_marks(brep: &Brep) -> Result<Vec<MadeBlendFace>> {
    let mut out = Vec::new();
    for &face in brep.face_list() {
        let Some(mark) = brep.find_attribute(EntityRef::Face(face), ids::FILLET_BLEND_FACE) else {
            continue;
        };
        let AttributeValue::Tag(tag) = mark.value() else {
            return Err(SolidError::invalid_input(
                OP,
                "a blend face mark is not a tag",
            ));
        };
        let kind = BlendFaceKind::from_code(tag.primary_id)
            .ok_or_else(|| SolidError::invalid_input(OP, "a blend face mark names no kind"))?;
        out.push(MadeBlendFace { face, kind });
    }
    Ok(out)
}

/// The faces of `offset`, the offset of a body whose blend faces were marked
/// ([`mark_blend_faces`]), that are offsets of blend faces: each marked face of `offset`
/// as a [`BlendFaceKind::Offset`] face, in its face order.
///
/// # Errors
/// Those of [`blend_face_marks`].
pub fn offsets_of_blends(offset: &Brep) -> Result<Vec<MadeBlendFace>> {
    Ok(blend_face_marks(offset)?
        .into_iter()
        .map(|m| MadeBlendFace {
            face: m.face,
            kind: BlendFaceKind::Offset,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_s_code_reads_back() {
        let corners = [
            CornerKind::Ball,
            CornerKind::Setback,
            CornerKind::End,
            CornerKind::Join,
            CornerKind::Mitre,
            CornerKind::Swept,
            CornerKind::Mixed,
            CornerKind::Chamfer,
            CornerKind::Pinned,
        ];
        let mut kinds = vec![
            BlendFaceKind::Round,
            BlendFaceKind::ExactRound,
            BlendFaceKind::Chamfer,
            BlendFaceKind::Offset,
        ];
        kinds.extend(corners.map(BlendFaceKind::Corner));
        for kind in kinds {
            assert_eq!(BlendFaceKind::from_code(kind.code()), Some(kind));
        }
        assert_eq!(BlendFaceKind::from_code(0), None);
    }
}
