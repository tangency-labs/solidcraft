//! Exact orthographic hidden lines for placed portable B-rep bodies, and the silhouettes of their
//! curved faces: solidcore's (`solidcore_ops::hlr`), through the kernel adapter. See
//! `docs/drawing-hlr.md` for the contract and its measured limits. The kernel starts no threads;
//! the application's [`hidden_lines`] runs its work on [`Threads`].

pub use solidcore_ops::hlr::*;

/// Worker threads for the kernel's hidden lines, named `solidcraft-hlr`. The browser build has
/// none: there the work runs on the calling thread.
#[derive(Clone, Copy, Debug, Default)]
pub struct Threads;

impl Workers for Threads {
    #[cfg(not(target_arch = "wasm32"))]
    fn run(&self, count: usize, work: &(dyn Fn() + Sync)) -> bool {
        std::thread::scope(|scope| {
            let started: Vec<_> =
                (0..count).filter_map(|_| std::thread::Builder::new().name("solidcraft-hlr".into()).spawn_scoped(scope, work).ok()).collect();
            let any = !started.is_empty();
            for worker in started {
                // A worker that panicked lost its items, which the kernel reports as an error.
                let _ = worker.join();
            }
            any
        })
    }

    #[cfg(target_arch = "wasm32")]
    fn run(&self, _count: usize, _work: &(dyn Fn() + Sync)) -> bool {
        false
    }
}

/// The hidden-line drawing of the scene ([`solidcore_ops::hlr::hidden_lines`]), its work on up
/// to [`Options::threads`] [`Threads`].
///
/// # Errors
/// The kernel's refusal: an invalid placement or view, a budget exceeded, cancelled.
pub fn hidden_lines(
    defs: &[Prepared],
    scene: &[Placement],
    view: &View,
    opts: &Options,
    cancel: &(dyn Fn() -> bool + Sync),
) -> Result<Drawing, HlrError> {
    hidden_lines_on(defs, scene, view, opts, cancel, &Threads)
}

/// A curve's accuracy on a view's paper: its deviation scaled as the view scales the curve
/// ([`scaled`] by the paper map's scale).
pub fn accuracy_on_paper(frame: &solidcraft_geom::drawing_view::ViewFrame, accuracy: CurveAccuracy) -> CurveAccuracy {
    scaled(accuracy, frame.paper_map().scale())
}

#[cfg(test)]
mod tests {
    /// A drawing's hidden lines (solidcore's) take as many items as a section: one aggregate limit.
    #[test]
    fn hidden_lines_share_the_section_item_limit() {
        use solidcraft_geom::section::MAX_ITEMS;
        assert_eq!(super::MAX_ITEMS, MAX_ITEMS);
        assert_eq!(super::Options::default().max_tracks, MAX_ITEMS);
    }

    /// On a 2:1 view a bound doubles, and an analytic curve stays analytic.
    #[test]
    fn an_accuracy_on_paper_scales_with_the_view() {
        use solidcore_record::Vec2;
        use solidcraft_geom::drawing_view::{NamedView, ViewFrame};
        use solidcraft_geom::section::CurveAccuracy;
        let frame = ViewFrame::new(NamedView::Front.basis(), "2:1".parse().unwrap(), Vec2::new(50.0, 10.0), Vec2::new(210.0, 148.5)).unwrap();
        let bounded = CurveAccuracy::Bounded { max_deviation_mm: 0.01 };
        assert_eq!(super::accuracy_on_paper(&frame, bounded), CurveAccuracy::Bounded { max_deviation_mm: 0.02 });
        assert_eq!(super::accuracy_on_paper(&frame, CurveAccuracy::Analytic), CurveAccuracy::Analytic);
    }
}
