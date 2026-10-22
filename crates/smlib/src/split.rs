//! Split Face and Split Body (`capi/include/smlib.h`, "Split"): faces cut where tool surfaces
//! meet them, and a solid cut in two by a surface. Tools are [`crate::ssi::Surface`]s, each
//! taken over its whole domain.

use crate::ssi::Surface;
use crate::{Brep, Error, Result, error, ffi, init};

impl Brep {
    /// `self` with faces `faces` (indices in SMLib's face order, distinct) cut where each of
    /// `tools` meets them, the tools in turn, each cutting the pieces the earlier ones left; and
    /// for each face how many faces it became besides itself (0: no tool split it). `None` when
    /// no face is split. Tagged operands ([`Brep::tagged`]) give the pieces' history.
    pub fn split_faces(&self, faces: &[usize], tools: &[&Surface]) -> Result<Option<(Brep, Vec<usize>)>> {
        init()?;
        if faces.is_empty() || faces.len() > ffi::split::MAX_FACES || tools.is_empty() || tools.len() > ffi::split::MAX_TOOLS {
            return Err(Error::Argument);
        }
        let faces: Vec<i64> = faces.iter().map(|f| i64::try_from(*f).map_err(|_| Error::Argument)).collect::<Result<_>>()?;
        let handles: Vec<&ffi::ssi::SurfaceHandle> = tools.iter().map(|t| t.handle()).collect();
        match ffi::split::split_faces(&self.0, &faces, &handles) {
            Ok((h, made)) => Ok(Some((Brep(h), made.into_iter().map(|m| usize::try_from(m).unwrap_or(0)).collect()))),
            Err(ffi::ops::ERR_NO_CUT) => Ok(None),
            Err(s) => Err(error(s)),
        }
    }

    /// The manifold solid `self` cut in two by `tool` (whose domain must reach past it): the
    /// part behind the surface (its normal points away) and the part in front, each closed by
    /// the tool's piece inside the body and each possibly several solids. A tool that misses
    /// leaves both parts the whole body, with no face from the tool (tag `self` to tell); `None`
    /// when a side is left empty.
    pub fn split_body(&self, tool: &Surface) -> Result<Option<(Brep, Brep)>> {
        init()?;
        match ffi::split::split_body(&self.0, tool.handle()) {
            Ok((b, f)) => Ok(Some((Brep(b), Brep(f)))),
            Err(ffi::ops::ERR_NO_CUT) => Ok(None),
            Err(s) => Err(error(s)),
        }
    }
}
