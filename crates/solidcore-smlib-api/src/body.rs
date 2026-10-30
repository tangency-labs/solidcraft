//! The B-rep basics: native files, copies, transforms, counts and the solid census.
//!
//! Source: tangency-labs/smlib `capi/src/smlib_c.cpp` (`smlib_brep_read`,
//! `smlib_brep_write`, `smlib_brep_copy`, `smlib_brep_transform`, `smlib_brep_counts`),
//! `capi/src/smlib_flat_api.cpp` (`smlib_brep_is_manifold_solid`) and
//! `capi/src/smlib_ops.cpp` (`smlib_brep_solid_count`), replay/1.0.1 at f8fd83e.
//!
//! | Rust | C API (`SM_API` / SMLib call) |
//! |------|-------|
//! | [`Smlib::brep_read`], [`Smlib::brep_read_bytes`] | `smlib_brep_read` (`SmApiReadBrepFromFile`) |
//! | [`Smlib::brep_write`], [`Smlib::brep_write_bytes`] | `smlib_brep_write` (`SmApiWriteBrepToFile`) |
//! | [`Smlib::brep_copy`] | `smlib_brep_copy` (`SmApiBrepCopy`) |
//! | [`Smlib::brep_transform`] | `smlib_brep_transform` (`SmBrep::Transform(double[4][4], TRUE)`) |
//! | [`Smlib::brep_counts`] | `smlib_brep_counts` |
//! | [`Smlib::brep_is_manifold_solid`] | `smlib_brep_is_manifold_solid` (`SmApiBrepIsManifoldSolid`) |
//! | [`Smlib::brep_solid_count`] | `smlib_brep_solid_count` (`SmApiBrepMaterialCensus`) |
//!
//! Deviations: the `_bytes` forms read and write memory (the C API takes paths only). The
//! matrix transform takes upstream's own first branch (`SmBrep::Transform(double[4][4])`,
//! `SmBrep.cpp:28891`): rows that are orthogonal and right-handed go through the
//! placement-and-scale `Transform`. The SVD branch for a shear or a reflection is not
//! ported in solidcore (`topology.brep_p5`); such a matrix is [`Status::ERR_UNSUPPORTED`].

use solidcore::geom::attribute::AttributeStore;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::math::{Vec3, EFF_ZERO, EFF_ZERO_RAD};
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::Brep;

use crate::session::{finite, take, Fail, Smlib};
use crate::status::{Result, Status};

impl Smlib {
    /// `smlib_brep_read`: the native SMLib `.smb` file at `path` (binary unless `ascii`).
    pub fn brep_read(&mut self, path: &str, ascii: bool) -> Result<Brep> {
        self.run(
            || format!("smlib_brep_read(path={path})"),
            &[],
            |api| {
                let data = std::fs::read(path).map_err(|_| Fail::Status(Status::SM_ERR))?;
                api.read_smb(data, ascii)
            },
        )
    }

    /// [`Smlib::brep_read`] of the file's bytes.
    pub fn brep_read_bytes(&mut self, data: &[u8], ascii: bool) -> Result<Brep> {
        self.run(
            || format!("smlib_brep_read(bytes={})", data.len()),
            &[],
            |api| api.read_smb(data.to_vec(), ascii),
        )
    }

    /// `SmApiReadBrepFromFile(path, ascii, FALSE, r)` on bytes, then `take`.
    fn read_smb(&mut self, data: Vec<u8>, ascii: bool) -> core::result::Result<Brep, Fail> {
        let mut attributes = AttributeStore::new();
        let mut records = NotifyRecords::new();
        // a new B-rep's own naming (`Brep::owner_identity`), which the read body takes
        let fresh = Brep::new();
        let key_of = |e| fresh.owner_identity(e);
        let mut env = TopologyEnv {
            all_attributes: &[],
            attributes: &mut attributes,
            key_of: &key_of,
            records: &mut records,
            next_key: 1 << 50,
        };
        let mut b = solidcore::topology::api_files::read_brep_from_file(
            self.kernel_mut(),
            Some(data),
            ascii,
            false,
            &mut env,
        )?;
        b.refresh_cached_topology();
        take(b)
    }

    /// `smlib_brep_write`: writes `brep` to the native SMLib `.smb` file `path` (binary
    /// unless `ascii`).
    pub fn brep_write(&mut self, brep: &Brep, path: &str, ascii: bool) -> Result<()> {
        self.run(
            || format!("smlib_brep_write(brep, path={path})"),
            &[("brep", brep)],
            |api| {
                let bytes = api.smb_bytes(brep, ascii)?;
                std::fs::write(path, bytes).map_err(|_| Fail::Status(Status::SM_ERR))
            },
        )
    }

    /// [`Smlib::brep_write`] into bytes.
    pub fn brep_write_bytes(&mut self, brep: &Brep, ascii: bool) -> Result<Vec<u8>> {
        self.run(
            || "smlib_brep_write(brep, bytes)".to_string(),
            &[("brep", brep)],
            |api| Ok(api.smb_bytes(brep, ascii)?),
        )
    }

    /// `smlib_brep_copy`: a deep copy of `brep`.
    pub fn brep_copy(&mut self, brep: &Brep) -> Result<Brep> {
        self.run(
            || "smlib_brep_copy(brep)".to_string(),
            &[("brep", brep)],
            |api| take(api.copy(brep)?),
        )
    }

    /// `smlib_brep_transform`: `brep` moved by the affine `matrix`: rows 0-2 the images of
    /// the x, y and z axes, row 3 the translation (column 3 is ignored). Tolerances scale
    /// with the model. A matrix whose rows are not orthogonal and right-handed (a shear or
    /// a reflection) is [`Status::ERR_UNSUPPORTED`] (see the module's deviations); that
    /// branch calls solidcore's `SmBrep::Transform(double[4][4])` port once
    /// tangency-labs/solidcore#1595 merges.
    pub fn brep_transform(&mut self, brep: &Brep, matrix: &[[f64; 4]; 4]) -> Result<Brep> {
        if !matrix.iter().all(|row| finite(row)) {
            return Err(Status::ERR_ARG);
        }
        self.run(
            || "smlib_brep_transform(brep)".to_string(),
            &[("brep", brep)],
            |api| {
                let mut r = api.copy(brep)?;
                transform_matrix(api, &mut r, matrix)?;
                take(r)
            },
        )
    }

    /// `smlib_brep_counts`: the face, edge and vertex counts, in that order.
    pub fn brep_counts(&mut self, brep: &Brep) -> Result<[i64; 3]> {
        self.run(
            || "smlib_brep_counts(brep)".to_string(),
            &[],
            |_| {
                Ok([
                    brep.face_list().len() as i64,
                    brep.edge_list().len() as i64,
                    brep.vertex_list().len() as i64,
                ])
            },
        )
    }

    /// `smlib_brep_is_manifold_solid`: SMLib's own check that `brep` is one closed,
    /// manifold solid.
    pub fn brep_is_manifold_solid(&mut self, brep: &Brep) -> Result<bool> {
        self.run(
            || "smlib_brep_is_manifold_solid(brep)".to_string(),
            &[("brep", brep)],
            |_| Ok(solidcore::query::api_queries::brep_is_manifold_solid(brep)?),
        )
    }

    /// `smlib_brep_solid_count`: the number of separate solid regions of `brep` (voids not
    /// counted).
    pub fn brep_solid_count(&mut self, brep: &Brep) -> Result<i64> {
        self.run(
            || "smlib_brep_solid_count(brep)".to_string(),
            &[("brep", brep)],
            |_| {
                let (solids, _voids) = solidcore::query::api_queries::brep_material_census(brep)?;
                Ok(solids as i64)
            },
        )
    }
}

/// `SmBrep::Transform(const double sTransMatrix[4][4], TRUE)`'s first branch
/// (`SmBrep.cpp:28891`): with no shear or reflection, the rows' lengths are the scale and
/// their directions the placement.
fn transform_matrix(
    api: &mut Smlib,
    b: &mut Brep,
    m: &[[f64; 4]; 4],
) -> core::result::Result<(), Fail> {
    let row = |i: usize| {
        m.get(i)
            .map_or(Vec3::ZERO, |&[x, y, z, _]| Vec3::new(x, y, z))
    };
    let (row0, row1, row2) = (row(0), row(1), row(2));
    let disp = row(3);
    if row0.dot(row1).abs() < EFF_ZERO {
        // (`AngleBetween`'s status is ignored upstream: a failed angle stays 0)
        let angle = row0.cross(row1).angle_between(row2).unwrap_or(0.0);
        if angle <= EFF_ZERO_RAD {
            let (len0, len1, len2) = (row0.length(), row1.length(), row2.length());
            let scale = Vec3::new(len0, len1, len2);
            let placement = Axis2Placement::new(disp, row0 / len0, row1 / len1);
            solidcore::topology::brep::p5::transform(
                api.kernel_mut(),
                b,
                &placement,
                Some(scale),
                true,
            )?;
            return Ok(());
        }
    }
    // UNPORTED(topology.brep_p5): SmBrep::Transform(double[4][4]) SVD branch (shear, reflection)
    Err(Fail::Status(Status::ERR_UNSUPPORTED))
}
