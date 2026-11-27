//! The slit a tear bend relief cuts in the flat: a cut of no width along the end of a bend's
//! zone, where the panel that runs on past the bend's end parts from the bend.
//!
//! Not upstream (SolidCraft's sheet metal, moved into the kernel so that the application computes
//! no geometry, docs/layers.md §1).
//!
//! | Rust          | SolidCraft (`crates/doc`)              |
//! |---------------|----------------------------------------|
//! | [`tear_slit`] | `sheet_surface_convert::tear_slits`    |
//!
//! Deviations: none (no upstream counterpart). No recursion.

use super::plane::Planar;
use super::{Anchor, Rigid2};
use crate::math::Vec2;
use crate::prism::profile::{Loop2, Seg2};

/// A corner of a panel's outline this close (mm) to the line square to its anchor at the bend's
/// end lies on that line.
const ON_END: f64 = 1e-9;

/// A slit reaches out past its start by more than this (mm), or there is none.
const MIN_REACH: f64 = 1e-9;

/// Where the flat is cut at one end of a bend's zone that a tear bend relief parts: along the
/// zone's end, from `from` (where the tangent line of the panel that runs on past the end meets
/// it) toward `to` (where the other panel's tangent line meets it: the panel that ends there).
/// The cut starts `depth` back from `from`, inside the panel that runs on (as deep as a slot
/// relief would reach), and runs out to the farthest corner, along that way, of the ending
/// panel's outline `outline` on the line square to its anchor `ends` at `at` along it, the panel
/// laid in the flat by `place`. The cut has no width: the tear takes no metal.
///
/// `None` when `from` and `to` are one point (the zone's end has no way), or no corner of the
/// outline on that line reaches out past `from`.
#[must_use]
pub fn tear_slit(
    from: Vec2,
    to: Vec2,
    depth: f64,
    ends: &Anchor,
    at: f64,
    outline: &Loop2,
    place: &Rigid2,
) -> Option<[Vec2; 2]> {
    let n = (to - from).normalized()?;
    let corner = ends.origin + ends.dir * at;
    let reach = outline
        .segs
        .iter()
        .map(Seg2::start)
        .filter(|v| (*v - corner).dot(ends.dir).abs() <= ON_END)
        .map(|v| place.point(v).dot(n))
        .reduce(f64::max)?;
    let along = from.dot(n);
    (reach > along + MIN_REACH).then(|| [from - n * depth, from + n * (reach - along)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::develop::seg::{ccw, polygon};

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Loop2 {
        ccw(polygon(&[
            Vec2::new(x0, y0),
            Vec2::new(x1, y0),
            Vec2::new(x1, y1),
            Vec2::new(x0, y1),
        ]))
    }

    /// A flange 30 long along its anchor (its chart's x) and 20 out from it (its chart's y),
    /// laid in the flat with its anchor's start at (0, 5), so the flange lies out toward +y from
    /// y = 5.
    fn flange() -> (Anchor, Loop2, Rigid2) {
        let ends = Anchor {
            panel: 1,
            origin: Vec2::ZERO,
            dir: Vec2::new(1.0, 0.0),
        };
        let place = Rigid2 {
            cos: 1.0,
            sin: 0.0,
            shift: Vec2::new(0.0, 5.0),
        };
        (ends, rect(0.0, 0.0, 30.0, 20.0), place)
    }

    #[test]
    fn a_tear_slit_runs_from_inside_the_panel_that_runs_on_to_the_ending_panels_far_edge() {
        let (ends, outline, place) = flange();
        // The zone's end at x = 30 (the anchor's length): from the base's tangent line at y = 0
        // up to the flange's tangent line at y = 5, 2 deep into the base.
        let s = tear_slit(
            Vec2::new(30.0, 0.0),
            Vec2::new(30.0, 5.0),
            2.0,
            &ends,
            30.0,
            &outline,
            &place,
        )
        .unwrap();
        // The flange's corners on x = 30 are (30, 0) and (30, 20), placed at y = 5 and y = 25.
        assert_eq!(s, [Vec2::new(30.0, -2.0), Vec2::new(30.0, 25.0)]);
        // At the anchor's start, the corners on x = 0.
        let s = tear_slit(
            Vec2::new(0.0, 0.0),
            Vec2::new(0.0, 5.0),
            1.5,
            &ends,
            0.0,
            &outline,
            &place,
        )
        .unwrap();
        assert_eq!(s, [Vec2::new(0.0, -1.5), Vec2::new(0.0, 25.0)]);
    }

    #[test]
    fn a_tear_slit_is_as_the_application_cut_it() {
        // Off the axes: the same sums, in the order the application made them.
        let (ends, outline, _) = flange();
        let place = Rigid2 {
            cos: 0.6,
            sin: 0.8,
            shift: Vec2::new(1.25, -3.5),
        };
        let (from, to, depth, at) = (Vec2::new(17.0, -9.0), Vec2::new(14.0, -5.0), 0.75, 30.0);
        let n = (to - from).normalized().unwrap();
        let reach = [Vec2::new(30.0, 0.0), Vec2::new(30.0, 20.0)]
            .map(|v| place.point(v).dot(n))
            .into_iter()
            .reduce(f64::max)
            .unwrap();
        let along = from.dot(n);
        let want = [from - n * depth, from + n * (reach - along)];
        let got = tear_slit(from, to, depth, &ends, at, &outline, &place).unwrap();
        assert_eq!(got, want, "bit for bit");
    }

    #[test]
    fn no_tear_slit_without_a_way_or_a_reach() {
        let (ends, outline, place) = flange();
        let p = Vec2::new(30.0, 0.0);
        assert_eq!(
            tear_slit(p, p, 2.0, &ends, 30.0, &outline, &place),
            None,
            "from and to one point"
        );
        // No corner on the line at 12 along the anchor.
        assert_eq!(
            tear_slit(p, Vec2::new(30.0, 5.0), 2.0, &ends, 12.0, &outline, &place),
            None,
            "no corner on the line"
        );
        // The flange lies behind `from`: nothing reaches past it.
        assert_eq!(
            tear_slit(
                Vec2::new(30.0, 40.0),
                Vec2::new(30.0, 45.0),
                2.0,
                &ends,
                30.0,
                &outline,
                &place
            ),
            None,
            "nothing past from"
        );
        assert_eq!(
            tear_slit(
                Vec2::new(30.0, f64::NAN),
                Vec2::new(30.0, 5.0),
                2.0,
                &ends,
                30.0,
                &outline,
                &place
            ),
            None,
            "not finite"
        );
    }
}
