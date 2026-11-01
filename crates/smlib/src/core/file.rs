//! `.smb` files on solidcore (`smlib_brep_read`, `smlib_brep_write`): the B-rep's contents through
//! `SmApiReadBrepFromFile` and `SmApiWriteBrepToFile`, the file itself read and written here (the
//! library touches no files, so that it builds for the browser).

use std::path::Path;

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::Kernel;
use solidcore::topology::api_files::{read_brep_from_file, write_brep_to_file};
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{Brep, EdgeId, EdgeuseId, FaceId};

use super::convert::key_of;
use super::{Body, run, with_body};
use crate::{Error, Result};

/// Where the keys of a file's curves and surfaces start, clear of [`key_of`]'s entity keys.
const FILE_KEYS: u64 = 1 << 50;

fn io(what: &str, path: &Path, e: &std::io::Error) -> Error {
    Error::Solidcore(format!("{what} {}: {e}", path.display()))
}

/// The body in the file at `path`, ASCII or binary. Its UV trims are checked, not rebuilt, as the
/// C API reads them.
pub(crate) fn read(path: &Path, ascii: bool) -> Result<Body> {
    let data = std::fs::read(path).map_err(|e| io("reading", path, &e))?;
    let b = run("smlib_brep_read", |k| {
        let mut attributes = AttributeStore::default();
        let mut records = NotifyRecords::new();
        let mut env = TopologyEnv { all_attributes: &[], attributes: &mut attributes, key_of: &key_of, records: &mut records, next_key: FILE_KEYS };
        let mut b = read_brep_from_file(k, Some(data), ascii, false, &mut env)?;
        b.refresh_cached_topology();
        Ok(b)
    })?;
    Ok(Body::new(b))
}

/// `b` as the contents of a `.smb` file, ASCII or binary.
pub(super) fn bytes(k: &Kernel, b: &Brep, ascii: bool) -> solidcore::Result<Vec<u8>> {
    let store = AttributeStore::default();
    let records = NotifyRecords::new();
    let env = FromBrepEnv {
        attributes: &store,
        key_of: &key_of,
        records: &records,
        curve_key: &|_: EdgeId| ObjectKey(0),
        uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
        surface_key: &|_: FaceId| ObjectKey(0),
    };
    write_brep_to_file(k, Some(b), &env, ascii)
}

/// `body` written to the file at `path`, ASCII or binary.
pub(crate) fn write(body: &Body, path: &Path, ascii: bool) -> Result<()> {
    let bytes = with_body("smlib_brep_write", body, |k, b| bytes(k, b, ascii))?;
    std::fs::write(path, bytes).map_err(|e| io("writing", path, &e))
}
