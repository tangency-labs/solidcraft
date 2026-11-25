//! The contract of `drawing.hardware_callout.add` (`cmd/drawing_hardware_callout.rs`, #20112). Its
//! reply is `drawing.note.inspect`'s ([`super::drawing_note::NoteResult`]).

use schemars::JsonSchema;
use serde::Deserialize;

use super::drawing_dimension::CornerParam;
use super::drawing_view::ViewIdParam;
use super::hole_callout::HoleFeatureParam;

/// What `drawing.hardware_callout.add` takes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct AddParams {
    /// The view to put the callout on: a view of the part, or its flat pattern view.
    pub view: ViewIdParam,
    /// The feature whose hole carries the hardware: the Hole feature, or the pattern or mirror that copied it
    /// (`sheet.flat_pattern` lists each hardware hole's feature).
    pub feature: HoleFeatureParam,
    /// Which of that feature's holes, from 0, in the order `sheet.flat_pattern` lists them (default 0).
    #[serde(default)]
    pub copy: u32,
    /// Where the callout's text sits on paper, in millimetres from the view's centre.
    pub at: [f64; 2],
    /// The centre of the hole's circular edge, picked as `drawing.dimension.add` picks a centre: where the leader ends
    /// on a view of the model. Required there; refused on a flat pattern view, whose leader ends on the hole in the flat.
    #[serde(default)]
    pub corner: Option<CornerParam>,
}
