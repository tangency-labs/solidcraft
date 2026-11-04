//! STEP (ISO 10303) as data: the exchange structure a file is read into ([`p21`]), the units
//! of its representation contexts ([`units`]), its Part 42 curves and surfaces ([`geom`])
//! as the entity reader ([`read`]) maps them, and its product structure ([`product`]). Building
//! bodies from it is `solidcore-ops`' job (docs/layers.md §2, "Importers are solidcore's").
pub mod geom;
pub mod p21;
pub mod product;
pub mod read;
pub mod units;
