//! Differential coverage for `blend.fillet_removal` (`SmBrep::DeleteFilletFace`).
//!
//! The fixtures are upstream `.smb` solids with fillets, read by the native
//! `SmBrep::ReadFromFile` and by the Rust reader (no C++ state reaches the Rust side). A
//! case removes one face (`GetFaces` order) with `DeleteFilletFace` (default tolerance,
//! editing enabled for the call), on the native B-rep and by
//! `blend::fillet_removal::delete_fillet_face`; every face of every fixture solid is a case,
//! so the faces that are not removable four-sided fillets compare their refusal (the B-rep
//! left as it was, but for side-edge UV trim curves the removal extends on the way). A case
//! compares the status, the whole B-rep graph (topology, vertex points, tolerances,
//! intervals, UV domains) and samples of every edge curve, every edge use's UV trim curve
//! and every face surface (type code, natural domain, evaluated points). Inputs the native
//! reader cannot build are `rejected`, not failed.
//!
//! The raw graph the native side exports carries B-spline surfaces and planes only, so a
//! B-rep with any other face surface (an `SmOffsetSurface` in `Fillet/fillet1017.smb`)
//! cannot be imported for the graph comparison. Such a case compares the record alone
//! (status and the sampled geometry, which evaluates every surface kind) on both sides:
//! the native side when the import refuses the surface, the Rust side when a face of the
//! B-rep after the removal has a surface that is not an `SmBSplineSurface`.

use serde::{Deserialize, Serialize};
use solidcore::blend::fillet_removal::delete_fillet_face;
use solidcore::kernel::Kernel;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::Brep;
use solidcore::Result as SolidResult;
use solidcore_ffi::units::blend_fillet_removal as ffi;
use solidcore_ffi::FfiError;

use super::boolean_merge_p1::{graph_owned, OwnerSource};
use super::topology_brep_mirror::{geometry, herr, normalize_face_shells, status};
use super::topology_vertexuse::{import_raw_with_owners, path_of, BrepIn};
use crate::{Case, HarnessError, Output, UnitDiff};

/// The runner of `SmBrep::DeleteFilletFace`.
pub struct Runner;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Inputs {
    /// An upstream test file, relative to `upstream/TestFiles/pt_TestFiles`.
    file: String,
    /// Rebuild the B-rep's topology on reading (`ReadFromFile`'s flag).
    rebuild: bool,
    /// Index of the face to remove in `GetFaces` order.
    face: u32,
}

impl Default for Inputs {
    fn default() -> Self {
        Self {
            file: "Fillet/filleted_box.smb".into(),
            rebuild: false,
            face: 0,
        }
    }
}

/// The shared B-rep block of `ReadFromFile(file, rebuild)`; no validate-only, no gaps.
fn block(i: &Inputs) -> Vec<f64> {
    vec![0.0, f64::from(i.rebuild), 0.0, 0.0]
}

fn path(i: &Inputs) -> Result<String, HarnessError> {
    path_of(&BrepIn::File {
        file: i.file.clone(),
        rebuild: i.rebuild,
    })
}

/// The native import's refusal of a face surface that is neither a B-spline nor a plane.
const NOT_EXPORTABLE: &str = "a surface is not a B-spline or plane";

/// Whether every face surface of `b` is an `SmBSplineSurface` (or absent), which the
/// native raw graph export carries.
fn graph_exportable(b: &Brep) -> SolidResult<bool> {
    for f in bp1::faces(b, None)? {
        if let Some(s) = b.face(f)?.surface.as_ref() {
            if !s.is_kind_of(solidcore::nurbs::b_spline_surface::BSplineSurface::TYPE)? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// Drops what the comparison does not record: the faces' `loops` and the loops' `edges`
/// lists (Rust-only logical records the native export walk never has, so the import
/// leaves them empty), as `topology.brep_local_operation`'s runner does.
fn strip_unrecorded(b: &mut Brep) -> SolidResult<()> {
    let faces: Vec<_> = b.faces().ids().collect();
    for id in faces {
        b.face_mut(id)?.loops.clear();
    }
    let loops: Vec<_> = b.loops().ids().collect();
    for id in loops {
        b.loop_mut(id)?.edges.clear();
    }
    Ok(())
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "blend.fillet_removal"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let raw = match ffi::eval(&path(&i)?, &block(&i), i.face) {
            Ok(raw) if raw.first() == Some(&-999.) => {
                return Ok(Output::rejected(format!(
                    "fixture constructor status {}",
                    raw.get(1).copied().unwrap_or(-1.)
                )))
            }
            Ok(raw) if raw.first() == Some(&-998.) => {
                return Ok(Output::rejected("face index past the fixture's faces"))
            }
            Ok(raw) => raw,
            Err(e @ FfiError::InvalidArgument(_)) => {
                return Ok(Output::rejected(format!("ffi guard: {e}")))
            }
            Err(e) => return Err(herr(e)),
        };
        let n = *raw.first().ok_or_else(|| herr("short record"))? as usize;
        let record = raw.get(1..=n).ok_or_else(|| herr("short record"))?.to_vec();
        let (mut b, owners) =
            match import_raw_with_owners(raw.get(n + 1..).ok_or_else(|| herr("short graph"))?) {
                Ok(imported) => imported,
                Err(e) if e.to_string().contains(NOT_EXPORTABLE) => {
                    return Ok(Output::new().values("record", record));
                }
                Err(e) => return Err(e),
            };
        strip_unrecorded(&mut b).map_err(herr)?;
        normalize_face_shells(&mut b).map_err(herr)?;
        graph_owned(&b, record, OwnerSource::Native(&owners))
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let mut k = Kernel::default();
        // the Rust reader's B-rep of the same file, its geometry owners named by the
        // Rust reader (no C++ export or owner reaches the Rust side)
        let mut b = match crate::own_reader::read_file_rebuild(&mut k, &path(&i)?, i.rebuild)? {
            Ok(b) => b,
            Err(e) => {
                return Output::unported(&e).ok_or_else(|| herr(format!("rust read: {e}")));
            }
        };
        let mut run = || -> SolidResult<Option<Vec<f64>>> {
            let faces = bp1::faces(&b, None)?;
            let Some(&face) = faces.get(i.face as usize) else {
                return Ok(None);
            };
            // (the shim's SmTemporaryChangeValue on m_bEditingEnabled for the call)
            let editing = b.editing_enabled();
            b.set_editing_enabled(true);
            let r = delete_fillet_face(&mut k, &mut b, face, None);
            b.set_editing_enabled(editing);
            let mut record = vec![status(r)?];
            record.extend(geometry(&k, &b, true)?);
            Ok(Some(record))
        };
        match run() {
            Ok(None) => Ok(Output::rejected("face index past the fixture's faces")),
            Ok(Some(record)) => {
                if !graph_exportable(&b).map_err(herr)? {
                    return Ok(Output::new().values("record", record));
                }
                strip_unrecorded(&mut b).map_err(herr)?;
                normalize_face_shells(&mut b).map_err(herr)?;
                // (the serial whose topology keys name the edit's new geometry)
                let serial = solidcore::topology::brep::p3::notify_serial(&mut k.context, &mut b)
                    .map_err(herr)?;
                graph_owned(&b, record, OwnerSource::Rust { serial: Some(serial) })
            }
            Err(e) => Output::unported(&e).ok_or_else(|| herr(e)),
        }
    }
}

/// WR-606 (the fork's tangency-labs/smlib 5b0534b): the exact-only removal on the
/// fixture solids, Rust only (the pinned C++ has no exact mode). Every face of every
/// fixture is tried; a removal refused as not exact leaves the solid as it was, one that
/// is not refused gives what the default removal gives, and some face of a fillet
/// between B-spline faces (whose new corner lies outside a surface's natural domain, or
/// whose side edge is a curve that stops short of it) is refused as not exact.
#[cfg(test)]
mod exact_only_tests {
    use super::{path, Inputs};
    use solidcore::blend::fillet_removal::FilletRemoval;
    use solidcore::kernel::Kernel;
    use solidcore::topology::brep::p1 as bp1;
    use solidcore::topology::face::p2 as face2;
    use solidcore::topology::{vertex, Brep};

    const FILES: [&str; 18] = [
        "Defeature/defillet.smb",
        "Defeature/inset_cube_1x1fillet.smb",
        "Defeature/inset_cube_1x2fillet.smb",
        "Defeature/inset_cube_1x3fillet.smb",
        "Defeature/inset_cube_3x1fillet.smb",
        "Fillet/cyl_on_box_w_fillet.smb",
        "Fillet/cyl_on_box_w_fillet1.smb",
        "Fillet/fillet0114.smb",
        "Fillet/fillet0227.smb",
        "Fillet/fillet1010.smb",
        "Fillet/fillet1017.smb",
        "Fillet/fillet1018.smb",
        "Fillet/fillet1019.smb",
        "Fillet/filleted_box.smb",
        "Fillet/shell_fillet.smb",
        "Fillet/torii_fillet.smb",
        "Solids/nurb_fillet.smb",
        "box_with_large_fillet.smb",
    ];

    /// Face, edge and vertex counts and the vertex points, sorted.
    fn snapshot(b: &Brep) -> (usize, usize, Vec<[f64; 3]>) {
        let mut points: Vec<[f64; 3]> = b
            .vertex_list()
            .iter()
            .map(|&v| vertex::point(b, v).unwrap().to_array())
            .collect();
        points.sort_by(|p, q| p.partial_cmp(q).unwrap());
        (b.face_list().len(), b.edges().ids().count(), points)
    }

    /// The fixture `file` read by the Rust reader.
    fn read(k: &mut Kernel, file: &str) -> Brep {
        let i = Inputs {
            file: file.into(),
            ..Inputs::default()
        };
        crate::own_reader::read_file_rebuild(k, &path(&i).unwrap(), false)
            .unwrap()
            .unwrap()
    }

    /// `SmFilletRemoval(face, face tolerance)` on face `index` of `file`, exact only or
    /// not, with editing enabled: whether it succeeded, `NotExact`, and the solid's
    /// snapshot before and after.
    #[allow(clippy::type_complexity)]
    fn remove(
        file: &str,
        index: usize,
        exact_only: bool,
    ) -> (bool, bool, (usize, usize, Vec<[f64; 3]>), (usize, usize, Vec<[f64; 3]>)) {
        let mut k = Kernel::default();
        let mut b = read(&mut k, file);
        let f = bp1::faces(&b, None).unwrap()[index];
        let before = snapshot(&b);
        b.set_editing_enabled(true);
        let tol = face2::tolerance(&b, f).unwrap();
        let mut removal = FilletRemoval::new(f, tol);
        removal.set_exact_only(exact_only);
        let r = removal.remove_face(&mut k, &mut b);
        b.refresh_cached_topology();
        (r.is_ok(), removal.not_exact(), before, snapshot(&b))
    }

    #[test]
    fn exact_only_refuses_inexact_removals_and_agrees_with_the_rest() {
        let mut refused = Vec::new();
        let mut exact = 0;
        for file in FILES {
            let mut k = Kernel::default();
            let count = bp1::faces(&read(&mut k, file), None).unwrap().len();
            for index in 0..count {
                let (ok, not_exact, before, after) = remove(file, index, true);
                if not_exact {
                    assert!(!ok, "{file} face {index}: not exact but removed");
                    assert_eq!(after, before, "{file} face {index}: refused but changed");
                    refused.push(format!("{file} face {index}"));
                } else if ok {
                    let (default_ok, _, _, default_after) = remove(file, index, false);
                    assert!(default_ok, "{file} face {index}: exact only removed, default not");
                    assert_eq!(after.0, default_after.0, "{file} face {index}: faces");
                    assert_eq!(after.1, default_after.1, "{file} face {index}: edges");
                    exact += 1;
                }
            }
        }
        eprintln!("exact-only: {exact} removed, refused as not exact: {refused:?}");
        assert!(!refused.is_empty(), "no fixture face was refused as not exact");
        assert!(exact > 0, "no fixture face was removed exactly");
    }
}
