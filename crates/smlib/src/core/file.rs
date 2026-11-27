//! `.smb` files (`smlib_brep_read`, `smlib_brep_write`): the B-rep's contents are solidcore-ops'
//! ([`solidcore_ops::Body::from_smb`], [`solidcore_ops::Body::to_smb`]), the file itself read and
//! written here (the library touches no files, so that it builds for the browser).

use std::path::Path;

use super::Body;
use super::call::{ops, ops_on};
use crate::{Error, Result};

fn io(what: &str, path: &Path, e: &std::io::Error) -> Error {
    Error::Solidcore(format!("{what} {}: {e}", path.display()))
}

/// The body in the file at `path`, ASCII or binary. Its UV trims are checked, not rebuilt, as the
/// C API reads them.
pub(crate) fn read(path: &Path, ascii: bool) -> Result<Body> {
    let data = std::fs::read(path).map_err(|e| io("reading", path, &e))?;
    ops("smlib_brep_read", || Body::from_smb(data, ascii))
}

/// `body` written to the file at `path`, ASCII or binary.
pub(crate) fn write(body: &Body, path: &Path, ascii: bool) -> Result<()> {
    let bytes = ops_on("smlib_brep_write", &[("brep", body)], || body.to_smb(ascii))?;
    std::fs::write(path, bytes).map_err(|e| io("writing", path, &e))
}
