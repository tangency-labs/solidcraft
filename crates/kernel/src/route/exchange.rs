//! The router's exchange helpers ([`solidcraft_kernel_api::Exchange`]).

use cadmpeg_ir::document::{CadIr, Model};
use solidcore_record::brep::Brep;
use solidcraft_kernel_api::{CadirBodies, CadirRefused, CadirWritten, Exchange, OpKind};

use super::{Which, which};
use crate::backend::Solidcore;

/// `text` as a STEP string literal, quoted and escaped.
pub fn step_string(text: &str) -> String {
    match which(OpKind::Exchange) {
        Which::Solidcore => Solidcore.step_string(text),
    }
}

/// Every body region of `ir` read into a record, its pcurves without a closed form made as a STEP
/// import's are ([`crate::import_trims`]).
pub fn read_cadir(ir: &CadIr) -> CadirBodies {
    match which(OpKind::Exchange) {
        Which::Solidcore => Solidcore.read_cadir(ir),
    }
}

/// `record` appended to `model` as one body named `name`, its ids `<id_prefix>:<kind>#<key>`. A
/// record CADIR cannot hold is [`CadirRefused`] with the kernel's words.
pub fn write_cadir(model: &mut Model, record: &Brep, id_prefix: &str, name: Option<&str>) -> Result<CadirWritten, CadirRefused> {
    match which(OpKind::Exchange) {
        Which::Solidcore => Solidcore.write_cadir(model, record, id_prefix, name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_routed_exchange_is_solidcores() {
        for text in ["plain", "it's", "back\\slash", "Grüße"] {
            assert_eq!(step_string(text), solidcore_step::write::string(text));
        }
        let made = crate::box_solid(solidcore_record::Vec3::new(0.0, 0.0, 0.0), solidcore_record::Vec3::new(10.0, 20.0, 30.0)).unwrap();
        let record = made.record();
        let mut routed = Model::default();
        let mut direct = Model::default();
        let w = write_cadir(&mut routed, record, "solidcraft:part-1", Some("box")).unwrap();
        let d = solidcore_cadir::write_body(&mut direct, record, "solidcraft:part-1", Some("box")).unwrap();
        assert_eq!(routed, direct);
        assert_eq!((&w.body, &w.regions, &w.faces, &w.edges), (&d.body, &d.regions, &d.faces, &d.edges));
        // a record CADIR refuses carries solidcore's words whole
        let mut bad = record.clone();
        bad.faces.clear();
        let words = solidcore_cadir::write_body(&mut Model::default(), &bad, "solidcraft:part-2", None).unwrap_err().to_string();
        assert_eq!(write_cadir(&mut Model::default(), &bad, "solidcraft:part-2", None).unwrap_err(), CadirRefused(words));
    }
}
