//! The nearest (or farthest) of a set of candidates, by an `f64` key.

/// Smallest or greatest item by a key, passing over items whose key is not finite (nothing
/// compares less than NaN, so a NaN must never become the best). Of equal keys the first wins.
pub trait MinByFinite: Iterator + Sized {
    /// The item with the smallest finite key; `None` when no key is finite.
    fn min_by_finite(self, mut key: impl FnMut(&Self::Item) -> f64) -> Option<Self::Item> {
        self.fold(None, |best: Option<(f64, Self::Item)>, x| {
            let k = key(&x);
            if k.is_finite() && best.as_ref().is_none_or(|(b, _)| k < *b) { Some((k, x)) } else { best }
        })
        .map(|(_, x)| x)
    }

    /// The item with the greatest finite key; `None` when no key is finite.
    fn max_by_finite(self, mut key: impl FnMut(&Self::Item) -> f64) -> Option<Self::Item> {
        self.min_by_finite(|x| -key(x))
    }
}

impl<I: Iterator> MinByFinite for I {}

#[cfg(test)]
#[path = "finite_min_tests.rs"]
mod tests;
