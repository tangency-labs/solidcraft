//! Differential coverage for `blend.identify_fillet` (`SmFace::IdentifyFillet`).
//!
//! The fixtures are upstream `.smb` solids with fillets, read by the native
//! `SmBrep::ReadFromFile` and by the Rust reader (no C++ state reaches the Rust side). A
//! case identifies one face (`GetFaces` order) with `IdentifyFillet(SM_ZONE_TOL_3D, 5.0, ..)`
//! and the case's tangent / manifold rail requirements, with or without the min/max radii;
//! every face of every fixture solid is a case, so planes, cylinders and the other faces
//! next to the fillets compare their classification too. A case compares whether the call
//! failed and, when it did not, every output: whether the face is a fillet, its cross
//! section, solver type and rail direction, the radius, the distance, the min/max radii
//! and the rail edges as positions in the face's outer-loop edge list. Inputs the native
//! reader cannot build are `rejected`, not failed.

use serde::{Deserialize, Serialize};
use solidcore::blend::identify_fillet::{identify_fillet, IdentifyFilletOptions};
use solidcore::kernel::Kernel;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::{face, loopuse};
use solidcore_ffi::units::blend_identify_fillet as ffi;
use solidcore_ffi::FfiError;

use super::topology_brep_mirror::herr;
use super::topology_vertexuse::{path_of, BrepIn};
use crate::{Case, HarnessError, Output, UnitDiff};

/// The runner of `SmFace::IdentifyFillet`.
pub struct Runner;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[allow(clippy::struct_excessive_bools)] // independent ReadFromFile and identify options
struct Inputs {
    /// An upstream test file, relative to `upstream/TestFiles/pt_TestFiles`.
    file: String,
    /// Rebuild the B-rep's topology on reading (`ReadFromFile`'s flag).
    rebuild: bool,
    /// Index of the face to identify in `GetFaces` order.
    face: u32,
    /// `bRequireTangent`.
    require_tangent: bool,
    /// `bRequireManifold`.
    require_manifold: bool,
    /// Pass `pMinMaxRadii`.
    min_max_radii: bool,
}

impl Default for Inputs {
    fn default() -> Self {
        Self {
            file: "Fillet/filleted_box.smb".into(),
            rebuild: false,
            face: 0,
            require_tangent: true,
            require_manifold: false,
            min_max_radii: false,
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

/// The shim's flag bits.
fn flags(i: &Inputs) -> u32 {
    u32::from(i.require_tangent)
        | (u32::from(i.require_manifold) << 1)
        | (u32::from(i.min_max_radii) << 2)
}

/// `failed` alone, or `failed` and the outputs: `ints` are `is_fillet cross_section
/// solver_type rail_dir n_min idx* n_max idx*`, `values` `radius dist [radii_min
/// radii_max]`.
fn output(failed: bool, ints: Vec<i64>, values: Vec<f64>) -> Output {
    let out = Output::new().int("failed", i64::from(failed));
    if failed {
        out
    } else {
        out.ints("ints", ints).values("values", values)
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "blend.identify_fillet"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let raw = match ffi::eval(&path(&i)?, &block(&i), i.face, flags(&i)) {
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
        let short = || herr("short record");
        let status = *raw.first().ok_or_else(short)?;
        if status != 0.0 {
            return Ok(output(true, Vec::new(), Vec::new()));
        }
        let mut at = 1;
        let mut next = || -> Result<f64, HarnessError> {
            let v = *raw.get(at).ok_or_else(short)?;
            at += 1;
            Ok(v)
        };
        let mut ints = Vec::new();
        for _ in 0..4 {
            ints.push(next()? as i64);
        }
        let mut values = vec![next()?, next()?];
        if i.min_max_radii {
            values.push(next()?);
            values.push(next()?);
        }
        for _ in 0..2 {
            let n = next()?;
            ints.push(n as i64);
            for _ in 0..n as usize {
                ints.push(next()? as i64);
            }
        }
        Ok(output(false, ints, values))
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let mut k = Kernel::default();
        // the Rust reader's B-rep of the same file (no C++ export reaches the Rust side)
        let mut b = match crate::own_reader::read_file_rebuild(&mut k, &path(&i)?, i.rebuild)? {
            Ok(b) => b,
            Err(e) => {
                return Output::unported(&e).ok_or_else(|| herr(format!("rust read: {e}")));
            }
        };
        let faces = bp1::faces(&b, None).map_err(herr)?;
        let Some(&f) = faces.get(i.face as usize) else {
            return Ok(Output::rejected("face index past the fixture's faces"));
        };
        let options = IdentifyFilletOptions {
            require_tangent: i.require_tangent,
            require_manifold: i.require_manifold,
            want_min_max_radii: i.min_max_radii,
            ..IdentifyFilletOptions::default()
        };
        let r = match identify_fillet(&k, &mut b, f, &options) {
            Ok(r) => r,
            Err(e) if e.is_unported() => {
                return Output::unported(&e).ok_or_else(|| herr(e));
            }
            Err(_) => return Ok(output(true, Vec::new(), Vec::new())),
        };
        // the rail edges' positions in the outer loop's edge list (the shim's `edge_index`)
        let loops = face::p2::loops(&b, f, false, None).map_err(herr)?;
        let edges = match loops.first() {
            Some(&l) => loopuse::loop_edges(&b, l).map_err(herr)?,
            None => Vec::new(),
        };
        let index = |e| {
            edges
                .iter()
                .position(|&x| x == Some(e))
                .map_or(-1, |p| p as i64)
        };
        let mut ints = vec![
            i64::from(r.is_fillet),
            i64::from(r.cross_section),
            i64::from(r.solver_type),
            r.rail_direction as i64,
        ];
        let mut values = vec![r.radius, r.dist];
        if let Some(radii) = r.min_max_radii {
            values.push(radii.min());
            values.push(radii.max());
        }
        for rails in [&r.rail_edges_min, &r.rail_edges_max] {
            ints.push(rails.len() as i64);
            ints.extend(rails.iter().map(|&e| index(e)));
        }
        Ok(output(false, ints, values))
    }
}
