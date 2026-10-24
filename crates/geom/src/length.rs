//! A length a model can hold.

use std::fmt;
use std::ops::Deref;

use serde::{Deserialize, Serialize};

/// A length a model can hold: finite, above [`Length::MIN`] and below [`Length::MAX`] mm (a
/// blend's radius, a chamfer's distance). Made only by [`Length::new`], so a function taking one
/// need not check it again. It reads as the `f64` it holds (`*r`, `r.get()`).
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(into = "f64", try_from = "f64")]
pub struct Length(f64);

impl Length {
    /// Shorter is a point.
    pub const MIN: f64 = 1e-6;
    /// Longer is past any model.
    pub const MAX: f64 = 1e6;

    /// `x` when it is above [`Length::MIN`] and below [`Length::MAX`] (a range test also throws
    /// out NaN).
    pub const fn new(x: f64) -> Option<Length> {
        if x > Length::MIN && x < Length::MAX { Some(Length(x)) } else { None }
    }

    pub const fn get(self) -> f64 {
        self.0
    }
}

impl Deref for Length {
    type Target = f64;
    fn deref(&self) -> &f64 {
        &self.0
    }
}

impl From<Length> for f64 {
    fn from(x: Length) -> f64 {
        x.0
    }
}

impl TryFrom<f64> for Length {
    type Error = &'static str;
    fn try_from(x: f64) -> Result<Length, Self::Error> {
        Length::new(x).ok_or("a length between 1e-6 and 1e6 mm")
    }
}

impl fmt::Display for Length {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_lengths_a_model_holds() {
        assert_eq!(Length::new(2.5).map(Length::get), Some(2.5));
        for x in [0.0, -0.0, -1.0, 1e-6, 1e6, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(Length::new(x), None, "{x}");
        }
        assert_eq!(serde_json::from_str::<Length>("3.0").ok(), Length::new(3.0));
        assert!(serde_json::from_str::<Length>("-3.0").is_err());
    }
}
