//! A drawing view's cosmetic threads (#13050): each shown body's threads, drawn by ISO 6410's
//! convention where the view drew their crests ([`thread_lines::on_paper`] pairs them). A modelled
//! thread leaves no cosmetic record, so it draws only its own edges.

use solidcraft_doc::drawing::thread_lines::{self, Drawn, Thread, ThreadPiece};
use solidcraft_geom::drawing_view::ViewFrame;

use super::ShownSet;
use super::lines::{ViewLine, near};

/// One cosmetic thread's lines in a view, on paper.
#[derive(Clone, Debug, PartialEq)]
pub struct ThreadLine {
    /// The index of the body among the shown set's.
    pub shown: usize,
    /// The body is a reference part.
    pub reference: bool,
    pub pieces: Vec<ThreadPiece>,
}

/// The threads of the bodies `shown` holds, in the view `frame` places on paper, paired with the
/// `lines` the view drew of each body, sampled from meshes asked for chords of `model_chord_mm`. A
/// thread whose crest the view drew nowhere draws nothing; so does a mesh body's.
pub fn threads(shown: &ShownSet, frame: &ViewFrame, lines: &[ViewLine], model_chord_mm: f64) -> Vec<ThreadLine> {
    let factor = frame.scale().factor();
    let mut out = Vec::new();
    for (at, one) in shown.bodies.iter().enumerate() {
        let Some(body) = shown.body(one).filter(|b| !b.body.is_mesh()) else { continue };
        let mut own = shown.state.threads.iter().filter(|t| t.body == body.name).peekable();
        if own.peek().is_none() {
            continue;
        }
        let drawn: Vec<Drawn<'_>> =
            lines.iter().filter(|l| l.shown == at).flat_map(|l| l.pieces.iter().map(|p| Drawn { visibility: p.visibility, path: &p.path })).collect();
        for thread in own.filter_map(Thread::of) {
            // The crest's lines lie off it by up to the chord's sag, as their arcs' cut points do.
            let near_mm = near(model_chord_mm, thread.crest_radius.max(thread.root_radius)) * factor;
            let pieces = thread_lines::on_paper(frame, &thread, &drawn, near_mm);
            if !pieces.is_empty() {
                out.push(ThreadLine { shown: at, reference: one.reference, pieces });
            }
        }
    }
    out
}
