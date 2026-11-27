//! solidcore's blend sections ([`Sections`]): the solidcore-ops calls this crate's callers made
//! before the interface existed, their refusals' words kept whole.

use solidcore_ops::sweep::railed;
use solidcraft_kernel_api::sections::{BlendFrame, FramesRefused, Sections, TurnRefusal};

use super::Solidcore;

impl Sections for Solidcore {
    fn point_frames(&self, points: &[[f64; 3]], leave: [f64; 3], stations: usize) -> Result<Vec<BlendFrame>, FramesRefused> {
        let frames = railed::point_frames(points, leave, stations).map_err(|e| FramesRefused(e.to_string()))?;
        Ok(frames.into_iter().map(|f| BlendFrame { p: f.p, leave: f.leave, bend: f.bend, bend_change: f.bend_change }).collect())
    }

    fn open_section_turns(&self, ends: &[[[f64; 3]; 2]]) -> Result<Vec<bool>, TurnRefusal> {
        railed::open_section_turns(ends).map_err(|e| match e {
            solidcore_ops::Error::Unsupported(_) => TurnRefusal::Closed(e.to_string()),
            e => TurnRefusal::Refused(e.to_string()),
        })
    }
}
