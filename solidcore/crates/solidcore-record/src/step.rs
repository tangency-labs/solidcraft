//! STEP (ISO 10303) as data: the exchange structure a file is read into ([`p21`]) and the neutral
//! plain-data B-rep model its bodies are mapped to ([`model`]). Building kernel bodies from it
//! is `solidcore-ops`' job (docs/layers.md §2, "Importers are solidcore's").
pub mod model;
pub mod p21;

pub use model::Model;
