//! The router's blend sections ([`solidcraft_kernel_api::sections::Sections`]), as
//! `solidcraft_kernel::sections`: the frames of a cross-section given by its points, and which
//! open sections of a blend to turn round.

use solidcraft_kernel_api::OpKind;
use solidcraft_kernel_api::sections::Sections;

pub use solidcraft_kernel_api::sections::{BlendFrame, FramesRefused, TurnRefusal};

use super::{Which, which};
use crate::backend::Solidcore;

/// The frames of a cross-section given by its points in order (the polyline through them), at
/// `stations` stations of even arc length, leaving along `leave` with no bend.
///
/// # Errors
/// The kernel's refusal, in its words.
pub fn point_frames(points: &[[f64; 3]], leave: [f64; 3], stations: usize) -> Result<Vec<BlendFrame>, FramesRefused> {
    match which(OpKind::Sections) {
        Which::Solidcore => Solidcore.point_frames(points, leave, stations),
    }
}

/// For each open section, given its first and last point in the blend's order, whether to turn
/// it round so that it runs the way the one before it does.
///
/// # Errors
/// [`TurnRefusal::Closed`] for a closed section; the kernel's other refusals.
pub fn open_section_turns(ends: &[[[f64; 3]; 2]]) -> Result<Vec<bool>, TurnRefusal> {
    match which(OpKind::Sections) {
        Which::Solidcore => Solidcore.open_section_turns(ends),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_ops::sweep::railed;

    #[test]
    fn routed_sections_are_solidcores() {
        let pts = [[0.0, 0.0, 0.0], [5.0, 0.0, 0.0], [10.0, 2.0, 0.0]];
        let routed = point_frames(&pts, [0.0, 0.0, 1.0], 5).unwrap();
        let direct = railed::point_frames(&pts, [0.0, 0.0, 1.0], 5).unwrap();
        assert_eq!(routed.len(), direct.len());
        for (r, d) in routed.iter().zip(&direct) {
            assert_eq!((r.p, r.leave, r.bend, r.bend_change), (d.p, d.leave, d.bend, d.bend_change));
        }
        let ends = [[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]], [[10.0, 0.0, 5.0], [0.0, 0.0, 5.0]]];
        assert_eq!(open_section_turns(&ends).unwrap(), railed::open_section_turns(&ends).unwrap());
        // a closed section is told apart, in solidcore's words
        let closed = [[[0.0, 0.0, 0.0], [0.0, 0.0, 0.0]], [[0.0, 0.0, 5.0], [10.0, 0.0, 5.0]]];
        let words = railed::open_section_turns(&closed).unwrap_err().to_string();
        assert_eq!(open_section_turns(&closed), Err(TurnRefusal::Closed(words)));
        // a refusal carries solidcore's words whole
        let words = railed::point_frames(&pts, [0.0, 0.0, 1.0], 0).unwrap_err().to_string();
        assert_eq!(point_frames(&pts, [0.0, 0.0, 1.0], 0), Err(FramesRefused(words)));
    }
}
