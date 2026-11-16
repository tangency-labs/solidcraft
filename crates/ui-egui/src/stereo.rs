//! The design view for a second pair of eyes (#2172): what a host that shows the model in a
//! headset needs from the frame the window draws. The window's own picture is not changed by it.
//!
//! A host switches [`Stereo::on`]; while it is on, each frame's design view leaves a [`Source`]
//! here: its frame for the renderer with the things a second camera does not share taken out, the
//! camera it was drawn with, and where in the window it is. The host takes the source after the
//! frame's UI and draws the same scenes from the headset's eyes
//! (`Renderer::draw_second_view`). A frame without a design view (the start page, a drawing
//! sheet, the render workspace) leaves none.

use solidcraft_viewport::Camera;
use solidcraft_viewport_gpu::view::Frame;

/// The design view of one frame, for a second camera.
pub struct Source {
    /// The window's frame: the same scenes, section, analysis and colours. The camera fields
    /// (`view_proj`, `turned_depth`, `eye`, `size_px`) are the window's and are replaced for each
    /// eye; `back` stays, so the light and the glass order are the same for both eyes and do not
    /// move with the head.
    pub frame: Frame,
    /// The camera the window drew with.
    pub camera: Camera,
    /// The design view's rectangle in the window (points).
    pub rect: egui::Rect,
    /// The view's background colours, top and bottom.
    pub background: [egui::Color32; 2],
    /// The model point under the mouse pointer (model units), when the pointer is over the model
    /// in the design view: a host draws the pointer there, at the model's depth, so it does not
    /// seem to sink into what it points at. `None` over empty space, over the UI, and while the
    /// view is being moved (nothing is picked then).
    pub pointer: Option<[f64; 3]>,
}

impl Source {
    /// The source for the design view drawn with `frame` and `camera` in `rect`, the mouse over
    /// no model point.
    ///
    /// Supersampling and ambient occlusion are taken out: both are prepared for the window's own
    /// camera and size (the occlusion's depth prepass is the window's view), and an eye's picture
    /// is filtered by the headset's own compositor.
    pub fn new(frame: &Frame, camera: Camera, rect: egui::Rect, background: [egui::Color32; 2]) -> Source {
        let mut frame = frame.clone();
        frame.ssaa = 1.0;
        frame.ao_radius = 0.0;
        frame.upload_error = None;
        Source { frame, camera, rect, background, pointer: None }
    }
}

/// Whether a host shows the design view in stereo, and this frame's [`Source`] for it.
#[derive(Default)]
pub struct Stereo {
    /// A host draws the design view for a headset: the view leaves its [`Source`] each frame.
    pub on: bool,
    source: Option<Source>,
    /// The model point under the pointer, from this frame's pick, until the view is published.
    pointer: Option<[f64; 3]>,
}

impl Stereo {
    /// What this frame's pick found under the pointer (nearest first; nothing while the view is
    /// moved or the pointer is elsewhere): the first hit that is a point on the model is where a
    /// host draws the pointer. Called every frame the design view picks, so a point from an
    /// earlier frame does not stay.
    pub(crate) fn pointing(&mut self, hits: &[crate::viewport::Hit]) {
        use crate::viewport::Hit;
        self.pointer = if self.on {
            hits.iter().find_map(|hit| match hit {
                Hit::Face { point, .. } | Hit::Vertex { point, .. } | Hit::Edge { point, .. } | Hit::SketchCurve { point, .. } => {
                    Some([point.x, point.y, point.z]).filter(|p| p.iter().all(|v| v.is_finite()))
                }
                _ => None,
            })
        } else {
            None
        };
    }

    /// The design view was drawn: keep what a second camera needs, when a host asks for it.
    pub(crate) fn publish(&mut self, frame: &Frame, camera: Camera, rect: egui::Rect, background: [egui::Color32; 2]) {
        let pointer = self.pointer.take();
        if self.on {
            self.source = Some(Source { pointer, ..Source::new(frame, camera, rect, background) });
        }
    }

    /// This frame's design view, once: `None` when no design view was drawn since the last take,
    /// or the host has not switched stereo on.
    pub fn take(&mut self) -> Option<Source> {
        self.pointer = None;
        self.source.take().filter(|_| self.on)
    }
}

/// A frame as the design view makes one, for tests: no scenes, a section, an outline, and
/// supersampling and ambient occlusion on.
#[cfg(test)]
pub(crate) fn test_frame() -> Frame {
    use std::sync::{Arc, Mutex};
    let none = || Arc::new(Mutex::new(None));
    Frame {
        product_lights: None,
        product: false,
        upload_error: Some(Arc::new(Mutex::new(None))),
        key: 7,
        slot: none(),
        sketch_key: 3,
        sketch_slot: none(),
        hl_key: 0,
        hl_slot: none(),
        pv_key: 0,
        pv_slot: none(),
        pv_hide: false,
        preview_bounds: None,
        view_proj: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]],
        turned_depth: None,
        back: [0.0, 0.0, 1.0],
        size_px: [640.0, 480.0],
        clip: [1.0, 0.0, 0.0, 5.0],
        cap: [0.0; 4],
        images: Vec::new(),
        clouds: Vec::new(),
        analysis: [0.0; 8],
        analysis_colors: [0.0; 36],
        access: None,
        outline: true,
        eye: [0.0, 0.0, 100.0, 1.0],
        ao_radius: 4.0,
        ao_samples: 32,
        ssaa: 2.0,
        hover_tint: [0.0; 3],
        selection_tint: [0.0; 3],
        hatch: Default::default(),
        sky: [0.0; 4],
        moving: None,
    }
}

#[cfg(test)]
mod tests {
    use super::test_frame as frame;
    use super::*;

    const RECT: egui::Rect = egui::Rect { min: egui::pos2(100.0, 50.0), max: egui::pos2(900.0, 650.0) };
    const BACKGROUND: [egui::Color32; 2] = [egui::Color32::from_rgb(1, 2, 3), egui::Color32::from_rgb(4, 5, 6)];

    /// Off (the default), the view leaves nothing: a desktop frame does no work for a headset.
    #[test]
    fn nothing_is_kept_while_stereo_is_off() {
        let mut stereo = Stereo::default();
        assert!(!stereo.on);
        stereo.publish(&frame(), Camera::default(), RECT, BACKGROUND);
        assert!(stereo.take().is_none());
    }

    /// On, the view's frame is kept for a second camera: the same scenes, section and outline,
    /// without the supersampling and ambient occlusion that belong to the window's own camera.
    #[test]
    fn the_design_view_is_kept_for_a_second_camera() {
        let mut stereo = Stereo { on: true, ..Stereo::default() };
        let camera = Camera { distance: 321.0, ..Camera::default() };
        stereo.publish(&frame(), camera, RECT, BACKGROUND);
        let source = stereo.take().expect("the frame's design view");
        assert_eq!((source.frame.key, source.frame.sketch_key), (7, 3));
        assert_eq!(source.frame.clip, [1.0, 0.0, 0.0, 5.0]);
        assert!(source.frame.outline);
        assert_eq!((source.frame.ssaa, source.frame.ao_radius), (1.0, 0.0));
        assert!(source.frame.upload_error.is_none());
        assert_eq!((source.camera, source.rect, source.background), (camera, RECT, BACKGROUND));
    }

    /// A source is one frame's: taken once, and a frame without a design view leaves none. One
    /// kept while stereo was on is not handed out after it is switched off.
    #[test]
    fn a_source_is_taken_once_and_not_after_stereo_goes_off() {
        let mut stereo = Stereo { on: true, ..Stereo::default() };
        stereo.publish(&frame(), Camera::default(), RECT, BACKGROUND);
        assert!(stereo.take().is_some());
        assert!(stereo.take().is_none(), "the next frame drew no design view");
        stereo.publish(&frame(), Camera::default(), RECT, BACKGROUND);
        stereo.on = false;
        assert!(stereo.take().is_none());
    }

    /// The pointer is drawn on the model where the pick found a point on it: the first such hit,
    /// passing over hits that have no point of their own. Off, over nothing, with a point that is
    /// not a number, and in a later frame that picked nothing, there is none.
    #[test]
    fn the_pointer_is_at_the_first_model_point_the_pick_found() {
        use crate::viewport::Hit;
        use solidcore_record::Vec3;
        let axis = Hit::Axis { name: "X".into() };
        let face = Hit::Face { body: "b".into(), index: 2, point: Vec3::new(1.0, 2.0, 3.0) };
        let edge = Hit::Edge { body: "b".into(), index: 0, mid: Vec3::new(9.0, 9.0, 9.0), point: Vec3::new(4.0, 5.0, 6.0) };
        let pointer = |stereo: &mut Stereo, hits: &[Hit]| {
            stereo.pointing(hits);
            stereo.publish(&frame(), Camera::default(), RECT, BACKGROUND);
            stereo.take().and_then(|source| source.pointer)
        };
        let mut stereo = Stereo { on: true, ..Stereo::default() };
        assert_eq!(pointer(&mut stereo, &[axis.clone(), face.clone(), edge.clone()]), Some([1.0, 2.0, 3.0]));
        assert_eq!(pointer(&mut stereo, &[edge.clone(), face.clone()]), Some([4.0, 5.0, 6.0]), "an edge's point under the cursor, not its middle");
        assert_eq!(pointer(&mut stereo, std::slice::from_ref(&axis)), None);
        assert_eq!(pointer(&mut stereo, &[]), None, "nothing picked: no point kept from the frame before");
        let nan = Hit::Face { body: "b".into(), index: 0, point: Vec3::new(f64::NAN, 0.0, 0.0) };
        assert_eq!(pointer(&mut stereo, &[nan]), None);
        // A pick with no design view drawn after it does not reach a later frame's view.
        stereo.pointing(std::slice::from_ref(&face));
        assert!(stereo.take().is_none());
        stereo.publish(&frame(), Camera::default(), RECT, BACKGROUND);
        assert_eq!(stereo.take().and_then(|source| source.pointer), None);
        let mut off = Stereo::default();
        assert_eq!(pointer(&mut off, &[face]), None);
    }
}
