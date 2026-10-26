//! A drawing view's shaded pictures (W17b S4, #5624): which pictures a view draws and how each body
//! takes part in each, from its two visual styles and whether reference parts hide the model.
//!
//! - The model is drawn when its style is shaded, reference parts
//!   ([`crate::drawing_view::Shown::reference`]) when theirs is.
//! - The model always hides reference parts behind it; reference parts hide the model only when
//!   `reference_hides_model` is on.
//! - Shaded reference parts that don't hide the model get a picture of their own, drawn first,
//!   under the model's, which leaves them out: so they never cover it.

use solidcraft_render::VisualStyle;

/// How a body takes part in one picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// Drawn in its own colours.
    Painted,
    /// Hides what lies behind it and draws nothing.
    DepthOnly,
    /// Not in the picture.
    Left,
}

/// The pictures a view draws, bottom first, each as one [`Part`] for every body, where
/// `is_reference[i]` says whether body `i` is a reference part. Empty when nothing shaded is there.
pub fn pictures(display: VisualStyle, reference: VisualStyle, hides_model: bool, is_reference: &[bool]) -> Vec<Vec<Part>> {
    let (model, references) = (display.is_shaded(), reference.is_shaded());
    let picture = |part: fn(bool, bool) -> Part| is_reference.iter().map(|r| part(*r, hides_model)).collect::<Vec<_>>();
    let pictures = match (model, references) {
        (false, false) => vec![],
        // Each hides the other: one picture. Otherwise the references' own, under the model's.
        (true, true) if hides_model => vec![picture(|_, _| Part::Painted)],
        (true, true) => vec![picture(|r, _| if r { Part::Painted } else { Part::Left }), picture(|r, _| if r { Part::Left } else { Part::Painted })],
        // Lines-only reference parts hide the model's picture only when they hide the model.
        (true, false) => vec![picture(|r, hides| match (r, hides) {
            (false, _) => Part::Painted,
            (true, true) => Part::DepthOnly,
            (true, false) => Part::Left,
        })],
        // A lines-only model still hides the reference parts behind it.
        (false, true) => vec![picture(|r, _| if r { Part::Painted } else { Part::DepthOnly })],
    };
    pictures.into_iter().filter(|p| p.contains(&Part::Painted)).collect()
}

#[cfg(test)]
#[path = "drawing_picture_tests.rs"]
mod tests;
