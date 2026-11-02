//! The print face: the flat face of a body that a slicer export lays on the print bed (#8429).
//! A body has at most one (`Document::print_faces`, by body name). It is held by the target a
//! sketch on a face holds ([`Document::flat_face_target`]), so a rebuild that adds, removes or
//! reorders faces finds the same face again, or says it is gone.

use solidcraft_geom::Vec3;

use crate::config::ConfigFaceTarget;
use crate::{DocError, Document, ModelBody, ModelState, Result};

/// Where a print face is: a point on it and its outward unit normal, in the coordinates of the
/// body it was found on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PrintFace {
    /// The face's index on the body (its first piece, when a change split it).
    pub index: usize,
    pub point: Vec3,
    pub normal: Vec3,
}

impl Document {
    /// The target that marks flat face `index` of `body` as its print face.
    pub fn print_face_target(&self, body: &ModelBody, index: usize) -> Result<ConfigFaceTarget> {
        if body.body.is_mesh() {
            return Err(DocError::Invalid("not supported yet: a print face on a mesh body".into()));
        }
        if body.body.planar_face(index).is_none() {
            return Err(DocError::Invalid("only a flat face can be the print face".into()));
        }
        // The sketch-on-a-face wording ("choose a fixed plane") offers what a print face has not.
        self.flat_face_target(body, index).map_err(|_| {
            DocError::Invalid(
                "this face can't be the print face: the body's history no longer tells it apart from its other faces (as after a shell of a shell body); pick a face of another body, or edit the feature that reshaped it".into(),
            )
        })
    }

    /// Where `body`'s print face is now. `None`: it has none. An error: the face can't be found
    /// any more, or is no longer flat.
    pub fn print_face(&self, state: &ModelState, body: &ModelBody) -> Option<Result<PrintFace>> {
        let target = self.print_faces.get(body.definition_name())?;
        Some(self.locate_print_face(state, body, target))
    }

    /// The print face a copy of `source` gets (Move/Copy, Mirror): the face of `copy` named after
    /// `source`'s print face (a copy's faces carry their source's names, followed by the copy's
    /// instance, `F1:+x@F2.1`). `None`:
    /// `source` has no print face.
    pub fn copied_print_face(&self, state: &ModelState, source: &ModelBody, copy: &ModelBody) -> Option<Result<ConfigFaceTarget>> {
        let target = self.print_faces.get(source.definition_name())?;
        let found = || {
            let (indices, _) = self.anchored_face_indices(state, source, target)?;
            let first = indices.first().copied().ok_or_else(|| DocError::Unknown("the print face no longer exists".into()))?;
            let names = crate::naming::face_names(source);
            let name =
                names.get(first).map(|n| crate::naming::strip_piece(n)).ok_or_else(|| DocError::Unknown("the print face has no name".into()))?;
            let index = crate::naming::face_names(copy)
                .iter()
                .position(|n| crate::naming::strip_piece(without_last_instance(n)) == name)
                .ok_or_else(|| DocError::Unknown(format!("the copy has no face `{name}`")))?;
            self.print_face_target(copy, index)
        };
        Some(found())
    }

    fn locate_print_face(&self, state: &ModelState, body: &ModelBody, target: &ConfigFaceTarget) -> Result<PrintFace> {
        if body.body.is_mesh() {
            return Err(DocError::Invalid("not supported yet: a print face on a mesh body".into()));
        }
        let (indices, _) = self.anchored_face_indices(state, body, target)?;
        let first = indices.first().copied().ok_or_else(|| DocError::Unknown("the print face no longer exists".into()))?;
        let plane = body.body.planar_face(first).ok_or_else(|| DocError::Invalid("the print face is no longer flat".into()))?;
        let scale = 1. + body.body.size();
        for index in indices.iter().skip(1) {
            let other = body.body.planar_face(*index).ok_or_else(|| DocError::Invalid("the print face is no longer flat".into()))?;
            if plane.normal().dot(other.normal()).abs() < 1. - 1e-8 || (other.origin - plane.origin).dot(plane.normal()).abs() > 1e-7 * scale {
                return Err(DocError::Invalid("the print face's pieces no longer lie on one plane".into()));
            }
        }
        // The surface's own orientation is how the kernel built it, so the side the material is
        // on is taken from the face's triangles, which point out of the body.
        let mesh = body.mesh();
        let mut area = Vec3::ZERO;
        for (t, f) in mesh.triangles.iter().zip(&mesh.tri_face) {
            if let (true, Some([a, b, c])) = (indices.contains(&(*f as usize)), mesh.tri(t)) {
                area += (b - a).cross(c - a);
            }
        }
        let normal = plane.normal().normalized().ok_or_else(|| DocError::Invalid("the print face's plane is degenerate".into()))?;
        let outward = area.dot(normal);
        if !(outward.abs() > 0.) {
            return Err(DocError::Invalid("the print face has no surface to lie on".into()));
        }
        Ok(PrintFace { index: first, point: plane.origin, normal: if outward < 0. { -normal } else { normal } })
    }
}

/// A face name without the copy instance at its end (`F1:+x@F2.1` is `F1:+x`; a copy of a copy,
/// `F1:+x@F2.1@F5.1`, is `F1:+x@F2.1`).
fn without_last_instance(n: &str) -> &str {
    let Some(at) = n.rfind('@') else { return n };
    let instance = n.get(at + 1..).and_then(|t| t.strip_prefix('F')).and_then(|t| t.split_once('.'));
    let digits = |t: &str| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit());
    if instance.is_some_and(|(feature, k)| digits(feature) && digits(k)) { n.get(..at).unwrap_or(n) } else { n }
}
