//! Written units and the kind of quantity each one measures.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quantity {
    Length,
    Angle,
    Mass,
    Density,
}

pub const UNITS: &[(&str, Quantity)] = &[
    ("mm", Quantity::Length),
    ("cm", Quantity::Length),
    ("m", Quantity::Length),
    ("in", Quantity::Length),
    ("deg", Quantity::Angle),
    ("rad", Quantity::Angle),
    ("g", Quantity::Mass),
    ("kg", Quantity::Mass),
    ("g/cm3", Quantity::Density),
    ("kg/m3", Quantity::Density),
];

/// The kind a whole unit (`mm`, `g/cm3`) measures, or `None` when it isn't a unit.
pub fn unit(text: &str) -> Option<Quantity> {
    UNITS.iter().find(|(u, _)| *u == text).map(|(_, q)| *q)
}

/// Is `text` a word the parser takes as (part of) a unit right after a number? `cm3` and `m3`
/// are, though they only make a unit under a `/`.
pub fn is_unit_word(text: &str) -> bool {
    UNITS.iter().any(|(u, _)| u.split('/').any(|w| w == text))
}
