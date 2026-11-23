//! The Browser as the headset shows it (docs/vr.md): the window's own Browser, drawn into the
//! window on the belt by a context of the headset's own, with the controller's ray as its
//! pointer. Its rows, folds, eyes and selecting are the window's; what its pointer is on lights
//! up in the design.

use crate::SolveApp;
use crate::semantic_content::ContentOwner;

/// The headset's Browser among the Browser's presentations: its folds and scroll its own.
const OWNER: ContentOwner = ContentOwner(0x5652);

/// The Browser into `ui` (of the headset's context), between the window's frames. What its
/// pointer lights goes into `lit`, which the caller keeps between frames and passes to
/// [`crate::vr_pick::light`] with nothing when the window closes.
pub fn show(app: &mut SolveApp, ui: &mut egui::Ui, lit: &mut Vec<String>) {
    crate::semantic_content::show_detached(app, ui, OWNER, lit);
}

/// The headset's Browser closed: what Select held in it (a drag over the eyes, a row drag) ends,
/// and the window's frame treats it as not drawn from now on.
pub fn close(app: &mut SolveApp) {
    crate::semantic_content::close_detached(app, OWNER);
}
