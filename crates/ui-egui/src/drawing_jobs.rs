//! A drawing's views drawn off the window's thread (#4892 B7): working a view's lines out takes long
//! enough on a large design that the window must keep drawing meanwhile. Each view, and each
//! shaded picture's setup, is worked out on a worker from a copy of the session, and taken up only
//! when nothing it was asked from has changed (`drawing_scene::finish`). The drawing page calls
//! [`tick`] each frame.

use solidcraft_engine::drawing_scene::{self, Done};

use crate::SolveApp;

/// A view being drawn on a worker thread.
pub struct Drawing {
    done: std::sync::mpsc::Receiver<Done>,
}

/// Takes up what a worker has done, then starts on the next view out of date or picture to set up,
/// and says whether any work is still waiting. In the browser, or when no thread can start, it does
/// that work here.
pub fn tick(app: &mut SolveApp) -> bool {
    if let Some(drawing) = &app.drawing_job {
        match drawing.done.try_recv() {
            Ok(done) => {
                drawing_scene::finish(&mut app.session, done);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => return true,
            // The worker stopped without lines: the view is asked for again below.
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {}
        }
        app.drawing_job = None;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Nothing long waiting: the dimensions out of date are read here (they are quick), and a
        // picture set up draws its bands for a few milliseconds, as `step` does.
        let Some(job) = drawing_scene::begin(&mut app.session) else { return drawing_scene::step(&mut app.session) };
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::Builder::new().name("drawing view".into()).spawn(move || {
            let _ = tx.send(job.run());
        });
        if worker.is_ok() {
            app.drawing_job = Some(Drawing { done: rx });
            return true;
        }
        // No thread to draw on: draw here, as before.
        drawing_scene::cancel(&mut app.session);
    }
    drawing_scene::step(&mut app.session)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use solidcraft_engine::doc::drawing::view::{View, ViewKind, ViewPlace};
    use solidcraft_engine::doc::drawing_view::{ViewDirection, ViewOverrides};
    use solidcraft_engine::drawing_scene;
    use solidcraft_engine::geom::drawing_view::NamedView;

    /// Ticked until nothing is being drawn, the window ends at the sheet built at once, byte for
    /// byte, its dimension read too, with the design and its undo steps as they were.
    #[test]
    fn ticks_draw_the_sheet_beside_the_window() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length": 100, "width": 80, "height": 20, "body_name": "Plate"})).unwrap();
        h.app.run("drawing.create", json!({})).unwrap();
        let mut d = (**h.app.session.drawing("test").unwrap()).clone();
        let mut ids = solidcraft_engine::doc::ids::IdSource::seeded(4892);
        let (sheet, source) = (d.sheets[0].id, d.sources[0].id);
        let mut front = 0;
        for (at, view) in [([120.0, 150.0], NamedView::Front), ([280.0, 150.0], NamedView::Right)] {
            let id = d.allocate_id(&mut ids).unwrap();
            if view == NamedView::Front {
                front = id;
            }
            let kind = ViewKind::Base { source, view: ViewDirection::Named(view) };
            d.add_views([Arc::new(View {
                id,
                sheet,
                kind,
                place: ViewPlace::At(at),
                settings: ViewOverrides { scale: Some("1:2".parse().unwrap()), ..ViewOverrides::default() },
                reference_shown_percent: 0.0,
            })]);
        }
        h.app.session.doc_mut().put_drawing(Arc::new(d));
        // The plate's top front edge, read once no view is waiting.
        let view = solidcraft_engine::doc::ids::encode(front);
        h.app.run("drawing.dimension.add", json!({"view": view, "kind": {"length": {"edge": [50, 0, 20]}}})).unwrap();
        let (doc, undo) = (h.app.session.doc.to_json(), h.app.session.undo.len());
        drawing_scene::extract(&mut h.app.session, sheet).unwrap();
        for _ in 0..10_000 {
            if !super::tick(&mut h.app) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let shown = drawing_scene::extract(&mut h.app.session, sheet).unwrap();
        let d = h.app.session.drawing("test").unwrap().clone();
        let built = drawing_scene::build(&h.app.session, &d, sheet).unwrap();
        assert!(shown.stale.is_empty() && shown.stale_dimensions.is_empty() && h.app.drawing_job.is_none());
        assert_eq!(serde_json::to_string(&*shown.scene).unwrap(), serde_json::to_string(&built.scene).unwrap());
        assert_eq!((h.app.session.doc.to_json(), h.app.session.undo.len()), (doc, undo));
    }
}
