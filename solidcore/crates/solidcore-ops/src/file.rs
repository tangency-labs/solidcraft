//! A body as the bytes of a native `.smb` file and back (`SmApiReadBrepFromFile`,
//! `SmApiWriteBrepToFile`). The file itself is the application's: ops touches no files, so that it
//! builds for the browser.

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::Kernel;
use solidcore::topology::api_files::{read_brep_from_file, write_brep_to_file};
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::loopuse::notify_ref as key_of;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::EdgeuseId;
use solidcore::{Brep, EdgeId, FaceId};

use crate::body::Body;
use crate::session::{read_body, run};
use crate::Result;

/// Where the keys of a file's curves and surfaces start, clear of the entity keys (`notify_ref`).
const FILE_KEYS: u64 = 1 << 50;

/// `b` as the contents of a `.smb` file, ASCII or binary.
///
/// # Errors
/// The kernel's.
pub(crate) fn bytes(k: &Kernel, b: &Brep, ascii: bool) -> solidcore::Result<Vec<u8>> {
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

impl Body {
    /// The body in a `.smb` file's contents, ASCII or binary. Its UV trims are checked, not
    /// rebuilt.
    ///
    /// # Errors
    /// The kernel's.
    pub fn from_smb(data: Vec<u8>, ascii: bool) -> Result<Body> {
        run("SmApiReadBrepFromFile", |k| {
            let mut attributes = AttributeStore::default();
            let mut records = NotifyRecords::new();
            let mut env = TopologyEnv {
                all_attributes: &[],
                attributes: &mut attributes,
                key_of: &key_of,
                records: &mut records,
                next_key: FILE_KEYS,
            };
            let mut b = read_brep_from_file(k, Some(data), ascii, false, &mut env)?;
            b.refresh_cached_topology();
            Ok(b)
        })
        .map(Body::new)
    }

    /// This body as a `.smb` file's contents, ASCII or binary.
    ///
    /// # Errors
    /// The kernel's.
    pub fn to_smb(&self, ascii: bool) -> Result<Vec<u8>> {
        read_body("SmApiWriteBrepToFile", self, |k, b| bytes(k, b, ascii))
    }
}
