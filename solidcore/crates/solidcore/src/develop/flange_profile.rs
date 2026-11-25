//! Where an edge flange's profile sketch lies on its flange, and where its bottom line lands on
//! the edge the flange stands on: the frame the sketch is drawn in, in the chart of the flange's
//! panel, and the measures an application reads a profile into that chart with.
//!
//! A flange's height is measured from its bend's outer virtual sharp. The chart's anchor marks the
//! virtual sharp of the sheet's surface: the outer one where the bend turns toward the material,
//! the inner one where it turns away, which lies `T·tan(θ/2)` out along the flange from the outer
//! one ([`anchor_past_outer_sharp`]).
//!
//! Not upstream (SolidCraft's sheet metal, moved into the kernel so that the application computes
//! no geometry, docs/layers.md §1).
//!
//! | Rust                        | SolidCraft (`crates/doc`, `sheet_surface_profile`)   |
//! |-----------------------------|------------------------------------------------------|
//! | [`anchor_past_outer_sharp`] | `anchor_past_outer`                                  |
//! | [`profile_frame`]           | `SurfaceSheet::profile_frame`                        |
//! | [`profile_tilt`]            | `SurfaceSheet::profile_flange_on` (its plane check)  |
//! | [`edge_fractions`]          | `SurfaceSheet::profile_flange_on` (its stretch)      |
//! | [`shift_along`]             | `SurfaceSheet::profile_flange_on` (its chart's shift)|
//!
//! Deviations: none (no upstream counterpart). No recursion.

use super::plane::Planar;
use super::Anchor;
use crate::math::{Vec2, Vec3};
use crate::prism::profile::Loop2;

/// How far out along a flange its bend's anchor lies from the bend's outer virtual sharp: none
/// where the bend turns toward the material (the sheet's surface is its outside), else the
/// thickness's share of the sharp, `T·tan(θ/2)` for the bend's angle `angle` (radians) and the
/// sheet's `thickness`.
#[must_use]
pub fn anchor_past_outer_sharp(toward_material: bool, angle: f64, thickness: f64) -> f64 {
    if toward_material {
        0.0
    } else {
        thickness * (angle / 2.0).tan()
    }
}

/// The frame a flange's profile sketch is drawn in, in the chart of the flange's panel
/// ([`profile_frame`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProfileFrame {
    /// The sketch's origin: where the bend starts on its outer virtual sharp.
    pub origin: Vec2,
    /// The sketch's x: along the bend, the anchor's direction.
    pub x: Vec2,
    /// The sketch's y: out along the flange, away from the bend, where the panel's material lies.
    pub y: Vec2,
    /// How deep in the panel's material the flange's outer face lies, which the sketch is drawn
    /// on: 0 (the sheet's surface) where the bend turns toward the material, else the thickness.
    pub depth: f64,
    /// How far out along y the panel's outline reaches from the origin: the flange's height from
    /// the outer sharp (never less than the anchor lies past the sharp).
    pub height: f64,
}

/// The frame a profile sketch for a flange is drawn in, in the chart of the flange's panel: on
/// its outer face, its origin where the bend starts on its outer virtual sharp, its x along the
/// bend and its y out along the flange. `anchor` is the bend's anchor on the flange's panel, the
/// bend turns `angle` (radians) toward the material or away from it (`toward_material`), the
/// sheet is `thickness` thick, and `outline` is the panel's outline, which the flange's height is
/// read from.
#[must_use]
pub fn profile_frame(
    anchor: &Anchor,
    toward_material: bool,
    angle: f64,
    thickness: f64,
    outline: &Loop2,
) -> ProfileFrame {
    let (o, d) = (anchor.origin, anchor.dir);
    // Out along the flange, away from the bend: the panel's material lies there.
    let out = Vec2::new(d.y, -d.x);
    let past = anchor_past_outer_sharp(toward_material, angle, thickness);
    let height = outline
        .segs
        .iter()
        .map(|s| (s.start() - o).dot(out))
        .fold(0.0, f64::max)
        + past;
    ProfileFrame {
        origin: o - out * past,
        x: d,
        y: out,
        depth: if toward_material { 0.0 } else { thickness },
        height,
    }
}

/// How far a sketch's plane, with unit normal `normal`, tilts from a panel whose unit normal is
/// `panel`: the length of their cross product, the sine of the angle between them. 0 where the
/// sketch lies parallel to the panel.
#[must_use]
pub fn profile_tilt(normal: Vec3, panel: Vec3) -> f64 {
    normal.cross(panel).length()
}

/// Where the points `along` the line of `anchor` (lengths from its origin) land on the straight
/// edge from `a` to `b`, in the same chart: each as the fraction of the edge from its end `b`,
/// clamped to `[0, 1]`. `None` when the edge has no length.
#[must_use]
pub fn edge_fractions(a: Vec2, b: Vec2, anchor: &Anchor, along: [f64; 2]) -> Option<[f64; 2]> {
    let len = a.dist(b);
    let way = (b - a).normalized()?;
    Some(along.map(|u| 1.0 - ((anchor.origin + anchor.dir * u - a).dot(way) / len).clamp(0.0, 1.0)))
}

/// How far along the line of `from` the origin of `to` lies from `from`'s origin.
#[must_use]
pub fn shift_along(from: &Anchor, to: &Anchor) -> f64 {
    (to.origin - from.origin).dot(from.dir)
}

#[cfg(test)]
mod tests {
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_3, PI};

    use super::*;
    use crate::develop::seg::{ccw, polygon};

    fn x_axis() -> Anchor {
        Anchor {
            panel: 1,
            origin: Vec2::ZERO,
            dir: Vec2::new(1.0, 0.0),
        }
    }

    /// A flange 30 along its anchor and 20 out from it: its chart's -y.
    fn outline() -> Loop2 {
        ccw(polygon(&[
            Vec2::new(0.0, 0.0),
            Vec2::new(30.0, 0.0),
            Vec2::new(30.0, -20.0),
            Vec2::new(0.0, -20.0),
        ]))
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-12
    }

    #[test]
    fn the_anchor_lies_past_the_outer_sharp_by_the_thickness_share_away_from_the_material() {
        assert_eq!(anchor_past_outer_sharp(true, FRAC_PI_2, 2.0), 0.0);
        assert!(close(anchor_past_outer_sharp(false, FRAC_PI_2, 2.0), 2.0));
        assert!(close(
            anchor_past_outer_sharp(false, 2.0 * FRAC_PI_3, 1.5),
            1.5 * 3f64.sqrt()
        ));
        assert_eq!(anchor_past_outer_sharp(false, 0.0, 2.0), 0.0);
    }

    #[test]
    fn a_profile_frame_stands_on_the_outer_sharp_and_reaches_the_flanges_height() {
        // Toward the material: the anchor is on the outer sharp, the sketch on the surface.
        let f = profile_frame(&x_axis(), true, FRAC_PI_2, 2.0, &outline());
        assert_eq!(f.origin, Vec2::ZERO);
        assert_eq!(f.x, Vec2::new(1.0, 0.0));
        assert_eq!(f.y, Vec2::new(0.0, -1.0));
        assert_eq!(f.depth, 0.0);
        assert_eq!(f.height, 20.0);
        // Away from it: the outer sharp lies T·tan(θ/2) = 2 back toward the bend, the sketch on
        // the face a thickness in, and the flange is that much higher from it.
        let f = profile_frame(&x_axis(), false, FRAC_PI_2, 2.0, &outline());
        assert!(close(f.origin.x, 0.0) && close(f.origin.y, 2.0), "{f:?}");
        assert_eq!(f.depth, 2.0);
        assert!(close(f.height, 22.0), "{f:?}");
        // An anchor along -y, starting at (5, 5): out is -x.
        let a = Anchor {
            panel: 0,
            origin: Vec2::new(5.0, 5.0),
            dir: Vec2::new(0.0, -1.0),
        };
        let l = ccw(polygon(&[
            Vec2::new(5.0, 5.0),
            Vec2::new(5.0, -5.0),
            Vec2::new(-7.0, -5.0),
            Vec2::new(-7.0, 5.0),
        ]));
        let f = profile_frame(&a, true, PI / 4.0, 1.0, &l);
        assert_eq!(f.y, Vec2::new(-1.0, 0.0));
        assert_eq!(f.height, 12.0);
    }

    #[test]
    fn an_outline_behind_its_anchor_has_no_height_past_the_sharp() {
        let behind = ccw(polygon(&[
            Vec2::new(0.0, 1.0),
            Vec2::new(30.0, 1.0),
            Vec2::new(30.0, 5.0),
            Vec2::new(0.0, 5.0),
        ]));
        assert_eq!(
            profile_frame(&x_axis(), true, FRAC_PI_2, 2.0, &behind).height,
            0.0
        );
    }

    #[test]
    fn a_profile_tilts_by_the_sine_of_its_angle_to_the_panel() {
        let z = Vec3::new(0.0, 0.0, 1.0);
        assert_eq!(profile_tilt(z, z), 0.0);
        assert_eq!(profile_tilt(Vec3::new(0.0, 0.0, -1.0), z), 0.0);
        assert_eq!(profile_tilt(Vec3::new(1.0, 0.0, 0.0), z), 1.0);
        let (s, c) = (PI / 6.0).sin_cos();
        assert!(close(profile_tilt(Vec3::new(s, 0.0, c), z), 0.5));
    }

    #[test]
    fn a_stretch_of_the_anchor_lands_on_the_edge_as_fractions_from_its_end_b() {
        // The edge runs from (0, 0) to (10, 0); the anchor from (1, 0) along it.
        let a = Anchor {
            panel: 1,
            origin: Vec2::new(1.0, 0.0),
            dir: Vec2::new(1.0, 0.0),
        };
        let [f1, f0] = edge_fractions(Vec2::ZERO, Vec2::new(10.0, 0.0), &a, [7.0, 1.0]).unwrap();
        assert!(close(f1, 0.2) && close(f0, 0.8), "{f1} {f0}");
        // Past either end: clamped.
        let [f1, f0] = edge_fractions(Vec2::ZERO, Vec2::new(10.0, 0.0), &a, [20.0, -5.0]).unwrap();
        assert_eq!((f1, f0), (0.0, 1.0));
        // The edge run the other way: from its end b at (0, 0).
        let [f1, _] = edge_fractions(Vec2::new(10.0, 0.0), Vec2::ZERO, &a, [7.0, 1.0]).unwrap();
        assert!(close(f1, 0.8), "{f1}");
        let p = Vec2::new(3.0, 3.0);
        assert_eq!(edge_fractions(p, p, &a, [0.0, 1.0]), None, "no length");
    }

    #[test]
    fn a_shift_along_is_measured_on_the_first_anchors_line() {
        let to = Anchor {
            panel: 2,
            origin: Vec2::new(4.0, 3.0),
            dir: Vec2::new(1.0, 0.0),
        };
        assert_eq!(shift_along(&x_axis(), &to), 4.0);
        assert_eq!(shift_along(&to, &x_axis()), -4.0);
    }
}
