//! STEP (ISO 10303) as data: the exchange structure a file is read into ([`p21`]) and the units
//! of its representation contexts ([`units`]). Building bodies from it is `solidcore-ops`' job
//! (docs/layers.md §2, "Importers are solidcore's").
pub mod p21;
pub mod units;
