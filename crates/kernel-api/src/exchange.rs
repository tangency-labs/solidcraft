//! File exchange helpers ([`Exchange`]) that the application's own readers and writers use beside
//! a kernel's import and export: the text of a STEP string, and CADIR (cadmpeg's neutral CAD
//! model) read into records and written from them. The records are the body format
//! (`solidcore_record::brep::Brep`); CADIR's own types are cadmpeg's (`cadmpeg_ir`).

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::ids::{BodyId, EdgeId, FaceId, RegionId};
use solidcore_record::brep::Brep;

use crate::Backend;

/// One body region of a CADIR model read into a record ([`Exchange::read_cadir`]).
#[derive(Clone, Debug)]
pub struct CadirRead {
    /// The body's id in the model.
    pub body: String,
    /// Which of the body's regions this is, in its order (from 0).
    pub region: usize,
    /// The record, in the model's coordinates (no placement applied).
    pub brep: Brep,
    /// Each record face's face in the model, by its index in the model's faces, in the record's
    /// face order.
    pub face_origin: Vec<u64>,
    /// No shell bounds a region: a body of open shells only.
    pub sheet: bool,
    /// What was repaired or noted on the way, in words.
    pub notes: Vec<String>,
    /// Where the body's placement puts the record (column-major 4 x 4, millimetres), when it
    /// moves it.
    pub placement: Option<[[f64; 4]; 4]>,
    /// Why the region was refused whole, where it came in as an open surface of the faces that
    /// read; `None` where it read as given.
    pub opened: Option<String>,
}

/// A body region of a CADIR model that could not be read, and why.
#[derive(Clone, Debug, PartialEq)]
pub struct CadirFailure {
    /// The body's id in the model.
    pub body: String,
    /// Which of the body's regions this is, in its order (from 0).
    pub region: usize,
    /// Why the region was not read, in words.
    pub message: String,
}

/// Every body region of a CADIR model: those read and those refused.
#[derive(Clone, Debug, Default)]
pub struct CadirBodies {
    /// The regions read into records, in the model's order.
    pub read: Vec<CadirRead>,
    /// The regions that could not be read.
    pub failures: Vec<CadirFailure>,
}

/// A record written into a CADIR model as one body ([`Exchange::write_cadir`]).
#[derive(Clone, Debug)]
pub struct CadirWritten {
    /// The body.
    pub body: BodyId,
    /// Its regions, in the record's order.
    pub regions: Vec<RegionId>,
    /// Each record face's CADIR face, by the record's face index.
    pub faces: Vec<Option<FaceId>>,
    /// Each record edge's CADIR edge, by the record's edge index.
    pub edges: Vec<Option<EdgeId>>,
}

/// The exchange helpers of one kernel ([`crate::OpKind::Exchange`]).
pub trait Exchange: Backend {
    /// `text` as a STEP string literal: quoted, with its quotes and backslashes escaped and the
    /// characters outside STEP's basic set written as its escapes.
    fn step_string(&self, text: &str) -> String;
    /// Every body region of `ir` read into a record, its pcurves without a closed form made as
    /// this kernel's STEP import makes them.
    fn read_cadir(&self, ir: &CadIr) -> CadirBodies;
    /// `record` appended to `model` as one body named `name`, its ids `<id_prefix>:<kind>#<key>`.
    /// A record CADIR cannot hold, or a value it refuses, is the kernel's error.
    fn write_cadir(&self, model: &mut Model, record: &Brep, id_prefix: &str, name: Option<&str>) -> Result<CadirWritten, Self::Error>;
}
