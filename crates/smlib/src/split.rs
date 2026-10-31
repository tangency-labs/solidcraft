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

#[cfg(all(test, feature = "native"))]
mod tests {
    use super::*;
    use crate::MEASURE_ACCURACY;

    /// The plane z = 3 (normal +z) through a 10 mm cube: the part behind it first (below, 300
    /// mm³), the part in front second (700 mm³), each closed; a plane past the cube leaves both parts the whole cube.
    #[test]
    fn a_plane_cuts_a_cube_into_its_back_and_front() {
        let cube = Brep::make_box([0.0; 3], [10.0; 3]).unwrap();
        let plane = |z: f64| Surface::plane([0.0, 0.0, z], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [-5.0, 15.0], [-5.0, 15.0]).unwrap();
        let (back, front) = cube.split_body(&plane(3.0)).unwrap().unwrap();
        for (part, volume, z) in [(&back, 300.0, [0.0, 3.0]), (&front, 700.0, [3.0, 10.0])] {
            let m = part.mass_properties(MEASURE_ACCURACY).unwrap();
            assert!((m.volume - volume).abs() < 1e-6, "{m:?}");
            assert_eq!(part.counts().unwrap()[0], 6);
            let (lo, hi) = part.bounds().unwrap();
            assert!((lo[2] - z[0]).abs() < 1e-9 && (hi[2] - z[1]).abs() < 1e-9, "{lo:?} {hi:?}");
        }
        // A plane past the cube: both parts the whole cube.
        let (back, front) = cube.split_body(&plane(12.0)).unwrap().unwrap();
        for part in [back, front] {
            assert!((part.volume(MEASURE_ACCURACY).unwrap() - 1000.0).abs() < 1e-6);
            assert_eq!(part.counts().unwrap(), [6, 12, 8]);
        }
    }

    /// The plane x = 4 across a cube splits the four faces it crosses into two each and leaves
    /// the two it misses; listing only the faces it misses splits nothing.
    #[test]
    fn a_plane_splits_the_faces_it_crosses() {
        let cube = Brep::make_box([0.0; 3], [10.0; 3]).unwrap();
        let plane = Surface::plane([4.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [-5.0, 15.0], [-5.0, 15.0]).unwrap();
        let (split, made) = cube.split_faces(&[0, 1, 2, 3, 4, 5], &[&plane]).unwrap().unwrap();
        assert_eq!(made.iter().sum::<usize>(), 4, "{made:?}");
        assert!(made.iter().all(|m| *m <= 1), "{made:?}");
        assert_eq!(split.counts().unwrap()[0], 10);
        let missed: Vec<usize> = (0..6).filter(|f| made[*f] == 0).collect();
        assert_eq!(missed.len(), 2);
        assert!(cube.split_faces(&missed, &[&plane]).unwrap().is_none());
        let v = split.mass_properties(MEASURE_ACCURACY).unwrap().volume;
        assert!((v - 1000.0).abs() < 1e-6, "{v}");
    }

    #[test]
    fn bad_face_or_tool_lists_are_argument_errors() {
        let cube = Brep::make_box([0.0; 3], [10.0; 3]).unwrap();
        let plane = Surface::plane([4.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [-5.0, 15.0], [-5.0, 15.0]).unwrap();
        assert_eq!(cube.split_faces(&[], &[&plane]).err(), Some(Error::Argument));
        assert_eq!(cube.split_faces(&[0], &[]).err(), Some(Error::Argument));
        let many = vec![0; ffi::split::MAX_FACES + 1];
        assert_eq!(cube.split_faces(&many, &[&plane]).err(), Some(Error::Argument));
        let tools = vec![&plane; ffi::split::MAX_TOOLS + 1];
        assert_eq!(cube.split_faces(&[0], &tools).err(), Some(Error::Argument));
        assert!(cube.split_faces(&[0, 6], &[&plane]).is_err(), "a face the cube does not have");
    }
}
