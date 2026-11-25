//! solidcore's exchange helpers ([`Exchange`]): each the solidcore call this crate's
//! `services::step` and `services::cadir` gave the application before the interface existed,
//! CADIR's results carried field for field.

use cadmpeg_ir::document::{CadIr, Model};
use solidcore_record::brep::Brep;
use solidcraft_kernel_api::{CadirBodies, CadirFailure, CadirRead, CadirWritten, Exchange};

use super::Solidcore;
use crate::{KernelError, Result};

impl Exchange for Solidcore {
    fn step_string(&self, text: &str) -> String {
        solidcore_step::write::string(text)
    }

    fn read_cadir(&self, ir: &CadIr) -> CadirBodies {
        let read = solidcore_cadir::read_bodies(ir, Some(crate::import_trims()));
        CadirBodies {
            read: read
                .read
                .into_iter()
                .map(|r| CadirRead {
                    body: r.body,
                    region: r.region,
                    brep: r.solid.brep,
                    face_origin: r.solid.face_origin,
                    sheet: r.solid.sheet,
                    notes: r.solid.notes,
                    placement: r.placement,
                    opened: r.opened,
                })
                .collect(),
            failures: read.failures.into_iter().map(|f| CadirFailure { body: f.body, region: f.region, message: f.message }).collect(),
        }
    }

    fn write_cadir(&self, model: &mut Model, record: &Brep, id_prefix: &str, name: Option<&str>) -> Result<CadirWritten> {
        let w = solidcore_cadir::write_body(model, record, id_prefix, name).map_err(|e| KernelError::Exchange(e.to_string()))?;
        Ok(CadirWritten { body: w.body, regions: w.regions, faces: w.faces, edges: w.edges })
    }
}
