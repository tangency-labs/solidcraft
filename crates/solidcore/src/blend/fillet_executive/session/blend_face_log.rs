//! The faces a fillet session made, with what made each, for the definition check.
//!
//! Not upstream (tangency-labs/solidcore#2489). While
//! [`crate::kernel::Kernel::blend_face_log`] is on, the session marks each face of its
//! intermediate body before the merge with what made it, as the corner marks do
//! (`corner_marks`): a tag attribute [`ids::FILLET_BLEND_FACE`] whose primary id is the
//! kind's code, travelling with the copy the merge gives the target. After filleting it
//! reads the marks back from the target's faces, appends those faces to the log and
//! deletes every mark, so the target's attributes are as they would be without the log.
//!
//! What made a face: a face on a corner's surfaces is that corner's
//! ([`CornerKind::of`] its blend corner); a face on a solver geometry's surface is a
//! chamfer when the solver's section is linear, an exact-class round when its geometry
//! was built by its class ([`FilletGeom::exact_class`]), a round otherwise.
//!
//! Deviations: none (not upstream). No recursion.

use super::{invalid, FilletSession};
use crate::blend::definition_check::{BlendFaceKind, CornerKind, MadeBlendFace};
use crate::blend::fillet_geom::p1::{FilletGeom, FilletSurfaceStorage};
use crate::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use crate::error::{EntityRef, Result};
use crate::geom::attribute::{ids, Attribute, AttributeValue, Behavior, TagData};
use crate::kernel::Kernel;
use crate::topology::brep::p1;
use crate::topology::FaceId;

impl FilletSession<'_> {
    /// The kind of the intermediate body's face on `geom`'s surface.
    fn solver_face_kind(&self, geom: &FilletGeom) -> Result<BlendFaceKind> {
        let linear = match geom.solver {
            Some(id) => self
                .objects
                .solver(id)?
                .base()
                .fsg
                .as_ref()
                .map(crate::blend::fillet_solver::p1::FilletSurfaceGenerator::fillet_surface_generator_type)
                == Some(FilletSurfaceGeneratorType::Linear),
            None => false,
        };
        Ok(if linear {
            BlendFaceKind::Chamfer
        } else if geom.exact_class {
            BlendFaceKind::ExactRound
        } else {
            BlendFaceKind::Round
        })
    }

    /// The intermediate body's faces with what made each: the corners' faces first, then
    /// the solver geometries'; a face is listed once, with its first maker.
    fn made_intermediate_faces(&self) -> Result<Vec<(FaceId, BlendFaceKind)>> {
        let body = self
            .objects
            .intermediate
            .as_ref()
            .ok_or_else(|| invalid("intermediate body is not allocated"))?;
        let identity = self
            .objects
            .intermediate_identity(EntityRef::Brep(crate::topology::BrepId::from_index(0)))?;
        let mut out: Vec<(FaceId, BlendFaceKind)> = Vec::new();
        let mut add = |face: FaceId, kind: BlendFaceKind| {
            if body.face(face).is_ok() && !out.iter().any(|&(f, _)| f == face) {
                out.push((face, kind));
            }
        };
        for corner in &self.objects.corners {
            let kind = BlendFaceKind::Corner(CornerKind::of(corner.blend));
            for &face in &corner.surface_faces {
                add(face, kind);
            }
        }
        for geom in self.objects.pseudo.geoms() {
            let Some(FilletSurfaceStorage::ManagedFace { body: owner, face }) = geom.fillet_surface
            else {
                continue;
            };
            if owner.key != identity.key {
                continue;
            }
            add(face, self.solver_face_kind(geom)?);
        }
        Ok(out)
    }

    /// Before the merge, with the log on: mark each made face of the intermediate body
    /// with its kind (`SM_AB_COPY`, so the merge's copies carry it).
    pub(super) fn mark_blend_faces(&mut self, k: &Kernel) -> Result<()> {
        if k.blend_face_log.is_none() {
            return Ok(());
        }
        for (face, kind) in self.made_intermediate_faces()? {
            let mark = Attribute::tag(
                ids::FILLET_BLEND_FACE,
                TagData {
                    primary_id: kind.code(),
                    ..TagData::default()
                },
                Behavior::Copy,
            );
            let handle = self.objects.attributes.insert(mark)?;
            let key = self
                .objects
                .intermediate_identity(EntityRef::Face(face))?
                .key;
            self.objects.attributes.add_attribute(key, handle)?;
            self.objects.blend_face_marks.push(handle);
        }
        Ok(())
    }

    /// With the log on, the kind face `face` of the target is marked with (`None` when it
    /// is not marked, and always with the log off).
    ///
    /// # Errors
    ///
    /// A mark that is not live, not a tag or names no kind.
    pub(super) fn target_blend_face_kind(
        &self,
        k: &Kernel,
        face: FaceId,
    ) -> Result<Option<BlendFaceKind>> {
        if k.blend_face_log.is_none() {
            return Ok(None);
        }
        let key = self.target_identity(EntityRef::Face(face)).key;
        let Some(handle) = self
            .objects
            .attributes
            .find_attribute(key, ids::FILLET_BLEND_FACE)
        else {
            return Ok(None);
        };
        let attribute = self
            .objects
            .attributes
            .get(handle)
            .ok_or_else(|| invalid("blend face mark is not live"))?;
        let AttributeValue::Tag(tag) = attribute.value() else {
            return Err(invalid("blend face mark is not a tag attribute"));
        };
        BlendFaceKind::from_code(tag.primary_id)
            .map(Some)
            .ok_or_else(|| invalid("blend face mark names no kind"))
    }

    /// With the log on, marks face `face` of the target with `kind` in place of any mark it
    /// has: a face the session makes on the target after the merge, or one that replaces a
    /// marked face (the split's rebuilt faces at a radius law's corners).
    ///
    /// # Errors
    ///
    /// Those of the attribute store.
    pub(super) fn mark_target_blend_face(
        &mut self,
        k: &Kernel,
        face: FaceId,
        kind: BlendFaceKind,
    ) -> Result<()> {
        if k.blend_face_log.is_none() {
            return Ok(());
        }
        let key = self.target_identity(EntityRef::Face(face)).key;
        if let Some(old) = self
            .objects
            .attributes
            .find_attribute(key, ids::FILLET_BLEND_FACE)
        {
            self.objects.attributes.destroy(old)?;
        }
        let mark = Attribute::tag(
            ids::FILLET_BLEND_FACE,
            TagData {
                primary_id: kind.code(),
                ..TagData::default()
            },
            Behavior::Copy,
        );
        let handle = self.objects.attributes.insert(mark)?;
        self.objects.attributes.add_attribute(key, handle)?;
        self.objects.blend_face_marks.push(handle);
        Ok(())
    }

    /// After filleting, with the log on: the target's marked faces, in its face order, go
    /// to the log as this session's entry, and every mark is deleted. Runs whether
    /// filleting succeeded or not (only a success's entry is kept).
    pub(super) fn take_blend_faces(&mut self, k: &mut Kernel, succeeded: bool) -> Result<()> {
        let Some(log) = k.blend_face_log.as_mut() else {
            return Ok(());
        };
        let mut made = Vec::new();
        let mut found = Vec::new();
        for face in p1::faces(self.objects.target, None)? {
            let key = self.target_identity(EntityRef::Face(face)).key;
            let Some(handle) = self
                .objects
                .attributes
                .find_attribute(key, ids::FILLET_BLEND_FACE)
            else {
                continue;
            };
            let attribute = self
                .objects
                .attributes
                .get(handle)
                .ok_or_else(|| invalid("blend face mark is not live"))?;
            let AttributeValue::Tag(tag) = attribute.value() else {
                return Err(invalid("blend face mark is not a tag attribute"));
            };
            let kind = BlendFaceKind::from_code(tag.primary_id)
                .ok_or_else(|| invalid("blend face mark names no kind"))?;
            made.push(MadeBlendFace { face, kind });
            found.push(handle);
        }
        for handle in found
            .into_iter()
            .chain(std::mem::take(&mut self.objects.blend_face_marks))
        {
            if self.objects.attributes.get(handle).is_some() {
                self.objects.attributes.destroy(handle)?;
            }
        }
        if succeeded {
            log.sessions.push(made);
        }
        Ok(())
    }
}
