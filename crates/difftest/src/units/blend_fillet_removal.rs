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
