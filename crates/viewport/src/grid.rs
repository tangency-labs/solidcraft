//! The grid's spacing and reach for a view: one rule for every app that draws a grid on a plane,
//! so their grids step and reach alike as the view zooms.

/// Grid spacing (minor, major) for a view of the given half height: the minor step is 1, 2 or 5
/// times a power of ten near a tenth of it, the major five minor.
pub fn grid_step(half_height: f64) -> (f64, f64) {
    let raw = (half_height / 10.0).max(1e-3);
    let p = 10f64.powf(raw.log10().floor());
    let minor = if raw / p < 2.0 {
        p
    } else if raw / p < 5.0 {
        2.0 * p
    } else {
        5.0 * p
    };
    (minor, minor * 5.0)
}

/// The grid's half size for a view of the given half height, seen at the given slant (the cosine
/// of the angle between the view and the grid's plane): the grid reaches as far on its plane as
/// the view is wide, which stretches as the plane is seen at a slant (#3087).
///
/// The picture and the window share it, so that they draw the same grid (#4214).
pub fn grid_ext(half_height: f64, major: f64, slant: f64) -> f64 {
    let major = major.max(1e-3);
    let ext = (half_height * 3.0 / major).ceil() * major;
    (ext / slant.abs().max(0.25) / major).ceil() * major
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The minor step is 1, 2 or 5 times a power of ten near a tenth of the view's half height,
    /// the major five minor; a view with no height still has a step.
    #[test]
    fn the_grid_steps_by_one_two_and_five_with_the_view() {
        assert_eq!(grid_step(1000.0), (100.0, 500.0));
        assert_eq!(grid_step(300.0), (20.0, 100.0));
        assert_eq!(grid_step(600.0), (50.0, 250.0));
        assert_eq!(grid_step(10.0), (1.0, 5.0));
        let (minor, major) = grid_step(0.0);
        assert!(minor > 0.0 && major == minor * 5.0);
    }

    /// #4214: the grid's half size, the one the picture and the window share: the grid reaches as
    /// far on its plane as the view is wide, and further as the plane is seen at a slant.
    #[test]
    fn the_grid_reaches_further_as_its_plane_is_seen_at_a_slant() {
        let major = grid_step(40.0).1;
        let flat = grid_ext(40.0, major, 1.0);
        assert_eq!(flat, (40.0 * 3.0 / major).ceil() * major, "seen face on, the grid reaches as far as the view is wide");
        assert!(grid_ext(40.0, major, 0.5) > flat, "seen at a slant, the grid reaches further on its plane");
        // A plane seen edge-on keeps a usable size.
        assert!(grid_ext(40.0, major, 0.0).is_finite());
    }
}
