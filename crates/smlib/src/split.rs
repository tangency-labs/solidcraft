//! Split Face and Split Body (`capi/include/smlib.h`, "Split"): faces cut where tool surfaces
//! meet them, and a solid cut in two by a surface. Tools are [`crate::ssi::Surface`]s, each
//! taken over its whole domain.

use crate::ssi::Surface;
use crate::{Brep, Error, Result, core};

impl Brep {
    /// `self` with faces `faces` (indices in SMLib's face order, distinct) cut where each of
    /// `tools` meets them, the tools in turn, each cutting the pieces the earlier ones left; and
    /// for each face how many faces it became besides itself (0: no tool split it). `None` when
    /// no face is split. Tagged operands ([`Brep::tagged`]) give the pieces' history.
    pub fn split_faces(&self, faces: &[usize], tools: &[&Surface]) -> Result<Option<(Brep, Vec<usize>)>> {
        if faces.is_empty() || faces.len() > core::split::MAX_FACES || tools.is_empty() || tools.len() > core::split::MAX_TOOLS {
            return Err(Error::Argument);
        }
        let specs: Vec<&crate::ssi::Spec> = tools.iter().map(|t| t.spec()).collect();
        core::split::split_faces(self.core(), faces, &specs).map(|r| r.map(|(b, made)| (Brep::made_core(b), made)))
    }

    /// The manifold solid `self` cut in two by `tool` (whose domain must reach past it): the
    /// part behind the surface (its normal points away) and the part in front, each closed by
    /// the tool's piece inside the body and each possibly several solids. A tool that misses
    /// leaves both parts the whole body, with no face from the tool (tag `self` to tell); `None`
    /// when a side is left empty.
    pub fn split_body(&self, tool: &Surface) -> Result<Option<(Brep, Brep)>> {
        core::split::split_body(self.core(), tool.spec()).map(|r| r.map(|(b, f)| (Brep::made_core(b), Brep::made_core(f))))
    }

    /// The manifold solid `self` cut in two by the sheet `sheet` as it is (its faces joined edge
    /// to edge; it must cross the body to cut it), as [`Brep::split_body`] cuts it: its faces'
    /// surfaces turned to agree with face `face`'s (in SMLib's face order), the part behind that
    /// normal and the part in front. A `sheet` that is no sheet, or a `face` it does not have, is
    /// [`Error::Argument`].
    pub fn split_body_by_sheet(&self, sheet: &Brep, face: usize) -> Result<Option<(Brep, Brep)>> {
        core::split::split_body_by_sheet(self.core(), sheet.core(), face)
            .map(|r| r.map(|(b, f)| (Brep::made_core(b), Brep::made_core(f))))
    }
}
