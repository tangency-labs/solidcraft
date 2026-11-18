//! Quantities (#3874): what one placement of a component means in a bill of materials, typed in as
//! its [`BOM_QUANTITY`](crate::metadata::BOM_QUANTITY) part property, and what a row's placements
//! add up to. The property is an expression evaluated with the design's parameters, so a cable's
//! length follows the design; without a unit it is a number of pieces (a pack of 4). Units come
//! from expressions: mass, volume and area wait for their expression units (#4811).

use std::cell::OnceCell;
use std::collections::BTreeMap;

use crate::expr::{self, Value};
use crate::{DocError, Document, Result};

/// The largest quantity of one placement, in pieces or millimetres.
const MAX_AMOUNT: f64 = 1e9;

/// The mass and volume units of #4811's expressions: until they reach expressions, a quantity
/// naming one is refused for what it is, not as an unknown parameter.
const MASS_AND_VOLUME: [&str; 7] = ["g", "kg", "lb", "ml", "mm³", "cm³", "m³"];

/// What one placement of a component counts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Amount {
    Pieces(f64),
    /// Millimetres.
    Length(f64),
}

/// A row's quantity: what its placements add up to.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Quantity {
    /// None of its definitions has a quantity: its count says it all.
    #[default]
    Count,
    Pieces(f64),
    /// Millimetres.
    Length(f64),
    Unknown(Uncounted),
}

/// Why a row's quantity isn't known.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Uncounted {
    /// Pieces and lengths can't be added (a placement without a quantity is one piece).
    Mixed,
    /// A definition's quantity doesn't evaluate: a parameter it names is in error (or missing from
    /// a damaged file).
    Invalid,
}

/// Evaluate a quantity expression with `doc`'s parameters.
pub fn amount(doc: &Document, text: &str) -> Result<Amount> {
    amount_in(doc, &OnceCell::new(), text)
}

/// [`amount`] with the parameters' values shared between calls, evaluated only when an expression
/// names one.
pub(super) fn amount_in(doc: &Document, values: &OnceCell<BTreeMap<String, Value>>, text: &str) -> Result<Amount> {
    let value = expr::eval_with(text, &|name| match values.get_or_init(|| doc.param_values().0).get(name) {
        Some(v) => Ok(*v),
        None if MASS_AND_VOLUME.contains(&name) => Err(mass_and_volume()),
        None => Err(DocError::Expr(format!("unknown parameter `{name}`"))),
    })?;
    let amount = match (value.len, value.ang) {
        (0, 0) => Amount::Pieces(value.v),
        (1, 0) => Amount::Length(value.v),
        (2, 0) => return Err(DocError::Invalid("not supported yet: area quantities".into())),
        (3, 0) => return Err(mass_and_volume()),
        _ => return Err(DocError::Invalid(format!("a quantity is a number of pieces or a length, not {}", value.describe()))),
    };
    let (Amount::Pieces(v) | Amount::Length(v)) = amount;
    if !(v > 0.0 && v <= MAX_AMOUNT) {
        return Err(DocError::Invalid(format!("a quantity is above 0 and at most {MAX_AMOUNT:e}")));
    }
    Ok(amount)
}

fn mass_and_volume() -> DocError {
    DocError::Invalid("not supported yet: mass and volume quantities need the expression units of #4811".into())
}

/// What placements add up to, from each one's entry in the definitions' amounts: none without a
/// quantity (one piece), `Some(None)` when its quantity doesn't evaluate.
pub(super) fn add_up<'a>(amounts: impl IntoIterator<Item = Option<&'a Option<Amount>>>) -> Quantity {
    let (mut pieces, mut length, mut typed) = (None, None, false);
    for amount in amounts {
        let (sum, n): (&mut Option<f64>, f64) = match amount {
            None => (&mut pieces, 1.0),
            Some(None) => return Quantity::Unknown(Uncounted::Invalid),
            Some(Some(Amount::Pieces(n))) => (&mut pieces, *n),
            Some(Some(Amount::Length(mm))) => (&mut length, *mm),
        };
        typed |= amount.is_some();
        *sum.get_or_insert(0.0) += n;
    }
    match (pieces, length) {
        _ if !typed => Quantity::Count,
        (Some(_), Some(_)) => Quantity::Unknown(Uncounted::Mixed),
        (None, Some(mm)) => Quantity::Length(mm),
        (n, None) => Quantity::Pieces(n.unwrap_or_default()),
    }
}
