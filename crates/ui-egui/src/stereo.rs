//! The design view for a headset's eyes (#2172): what a host that shows the model in a headset
//! needs from the frame the window draws. The window's own picture is not changed by it.
//!
//! A host switches [`Stereo::on`]; while it is on, each frame's design view leaves a [`Source`]
//! here: its frame for the renderer with the things a second camera does not share taken out, and
//! the camera it was drawn with. The host takes the source after the frame's UI and draws the same
//! scenes from the headset's eyes (`Renderer::draw_second_view`). A frame without a design view
//! (the start page, a drawing sheet, the render workspace) leaves none.

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
    /// The view's background colours, top and bottom.
    pub background: [egui::Color32; 2],
}

impl Source {
    /// The source for the design view drawn with `frame` and `camera`.
    ///
    /// Supersampling and ambient occlusion are taken out: both are prepared for the window's own
    /// camera and size (the occlusion's depth prepass is the window's view), and an eye's picture
    /// is filtered by the headset's own compositor.
    pub fn new(frame: &Frame, camera: Camera, background: [egui::Color32; 2]) -> Source {
        let mut frame = frame.clone();
        frame.ssaa = 1.0;
        frame.ao_radius = 0.0;
        frame.upload_error = None;
        Source { frame, camera, background }
    }
}

/// Whether a host shows the design view in a headset, and this frame's [`Source`] for it.
#[derive(Default)]
pub struct Stereo {
    /// A host draws the design view for a headset: the view leaves its [`Source`] each frame.
    pub on: bool,
    source: Option<Source>,
}

impl Stereo {
    /// The design view was drawn: keep what a second camera needs, when a host asks for it.
    pub(crate) fn publish(&mut self, frame: &Frame, camera: Camera, background: [egui::Color32; 2]) {
        if self.on {
            self.source = Some(Source::new(frame, camera, background));
        }
    }

    /// This frame's design view, once: `None` when no design view was drawn since the last take,
    /// or the host has not switched it on.
    pub fn take(&mut self) -> Option<Source> {
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
        clip: {
            let mut clip = [[0.0; 4]; solidcraft_viewport_gpu::view::MAX_SECTION_PLANES];
            clip[0] = [1.0, 0.0, 0.0, 5.0];
            clip
        },
        clip_hatch: [0; solidcraft_viewport_gpu::view::MAX_SECTION_PLANES],
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

    const BACKGROUND: [egui::Color32; 2] = [egui::Color32::from_rgb(1, 2, 3), egui::Color32::from_rgb(4, 5, 6)];

    /// Off (the default), the view leaves nothing: a desktop frame does no work for a headset.
    #[test]
    fn nothing_is_kept_while_stereo_is_off() {
        let mut stereo = Stereo::default();
        assert!(!stereo.on);
        stereo.publish(&frame(), Camera::default(), BACKGROUND);
        assert!(stereo.take().is_none());
    }

    /// On, the view's frame is kept for a second camera: the same scenes, section and outline,
    /// without the supersampling and ambient occlusion that belong to the window's own camera.
    #[test]
    fn the_design_view_is_kept_for_a_second_camera() {
        let mut stereo = Stereo { on: true, ..Stereo::default() };
        let camera = Camera { distance: 321.0, ..Camera::default() };
        stereo.publish(&frame(), camera, BACKGROUND);
        let source = stereo.take().expect("the frame's design view");
        assert_eq!((source.frame.key, source.frame.sketch_key), (7, 3));
        assert_eq!(source.frame.clip[0], [1.0, 0.0, 0.0, 5.0]);
        assert!(source.frame.outline);
        assert_eq!((source.frame.ssaa, source.frame.ao_radius), (1.0, 0.0));
        assert!(source.frame.upload_error.is_none());
        assert_eq!((source.camera, source.background), (camera, BACKGROUND));
    }

    /// A source is one frame's: taken once, and a frame without a design view leaves none. One
    /// kept while stereo was on is not handed out after it is switched off.
    #[test]
    fn a_source_is_taken_once_and_not_after_stereo_goes_off() {
        let mut stereo = Stereo { on: true, ..Stereo::default() };
        stereo.publish(&frame(), Camera::default(), BACKGROUND);
        assert!(stereo.take().is_some());
        assert!(stereo.take().is_none(), "the next frame drew no design view");
        stereo.publish(&frame(), Camera::default(), BACKGROUND);
        stereo.on = false;
        assert!(stereo.take().is_none());
    }
}
