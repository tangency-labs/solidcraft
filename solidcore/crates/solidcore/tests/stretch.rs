//! Stretch's geometry (`solidcore::stretch`): the cut planes, bands, passes, shrink and the
//! moved axes, with sizes derived by hand from a 40 x 20 x 10 mm box.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use solidcore::geom::vector3d::Vec3;
use solidcore::stretch::{cut_point, moved_axis, Matrix, Refusal, Stretch};

const X: [f64; 3] = [1.0, 0.0, 0.0];
const Z: [f64; 3] = [0.0, 0.0, 1.0];

fn apply(m: &Matrix, p: [f64; 3]) -> [f64; 3] {
    let mut out = [0.0; 3];
    for (i, o) in out.iter_mut().enumerate() {
        *o = p[0] * m[0][i] + p[1] * m[1][i] + p[2] * m[2][i] + m[3][i];
    }
    out
}

fn block() -> ([f64; 3], [f64; 3]) {
    ([0.0, 0.0, 0.0], [40.0, 20.0, 10.0])
}

#[test]
fn a_plane_that_misses_the_box_or_a_bad_number_is_refused() {
    let (lo, hi) = block();
    assert_eq!(
        Stretch::of_box(lo, hi, [50.0, 0.0, 0.0], X, 5.0),
        Err(Refusal::Misses)
    );
    assert_eq!(
        Stretch::of_box(lo, hi, [0.0, 0.0, 0.0], X, 5.0),
        Err(Refusal::Misses)
    );
    assert_eq!(
        Stretch::of_box(lo, hi, [20.0, 0.0, 0.0], [0.0; 3], 5.0),
        Err(Refusal::Argument)
    );
    assert_eq!(
        Stretch::of_box(lo, hi, [20.0, 0.0, 0.0], X, f64::NAN),
        Err(Refusal::Argument)
    );
}

#[test]
fn the_cut_planes_face_the_direction_and_cover_the_body() {
    let (lo, hi) = block();
    let s = Stretch::of_box(lo, hi, [20.0, 3.0, 4.0], X, 15.0).unwrap();
    let cut = s.cut();
    let (u, v) = (Vec3::from(cut.u), Vec3::from(cut.v));
    // u × v is the direction: the part behind the plane is the part that stays.
    assert!((u.cross(v) - Vec3::from(X)).length() < 1e-15);
    assert!((cut.origin[0] - 20.0).abs() < 1e-12 && cut.level == 20.0);
    // The plane's box covers the body's section and more.
    assert!(cut.domain[0][1] > 45.0 && cut.domain[1][1] > 45.0);
}

#[test]
fn a_long_stretch_takes_passes_of_at_most_eight_times_the_band() {
    let (lo, hi) = block();
    let s = Stretch::of_box(lo, hi, [20.0, 0.0, 0.0], X, 500.0).unwrap();
    // Half the body behind the cut first: 10 mm.
    let (depth, base) = s.band_depths()[0];
    assert_eq!((depth, base.level), (10.0, 10.0));
    let passes = s.passes(depth);
    // 70 mm a pass (8 x 10 less the 10 the band is), 500 mm in all: 8 passes.
    assert_eq!(passes.len(), 8);
    let mut grown = 0.0;
    for p in &passes {
        // The band's far side stays, its cut side goes to the cut plus the step.
        assert!((apply(&p.scale, [10.0, 5.0, 5.0])[0] - 10.0).abs() < 1e-12);
        let step = apply(&p.shift, [0.0; 3])[0];
        assert!((apply(&p.scale, [20.0, 5.0, 5.0])[0] - (20.0 + step)).abs() < 1e-9);
        assert!(step <= 70.0 + 1e-12);
        grown += step;
    }
    assert!((grown - 500.0).abs() < 1e-9);
}

#[test]
fn the_band_halves_down_to_a_thousandth_of_the_body() {
    let (lo, hi) = block();
    let s = Stretch::of_box(lo, hi, [20.0, 0.0, 0.0], X, 5.0).unwrap();
    let depths: Vec<f64> = s.band_depths().iter().map(|d| d.0).collect();
    assert_eq!(depths.first(), Some(&10.0));
    assert_eq!(depths.last(), Some(&0.04));
    assert!(depths.windows(2).all(|w| w[1] < w[0]));
}

#[test]
fn a_shrink_has_its_slab_and_move_and_too_deep_a_one_is_refused() {
    let (lo, hi) = block();
    let s = Stretch::of_box(lo, hi, [20.0, 0.0, 0.0], X, -5.0).unwrap();
    let shrink = s.shrink().unwrap();
    assert_eq!((shrink.cut.level, shrink.far.level), (20.0, 25.0));
    assert_eq!(apply(&shrink.shift, [25.0, 1.0, 2.0]), [20.0, 1.0, 2.0]);
    let s = Stretch::of_box(lo, hi, [20.0, 0.0, 0.0], X, -20.0).unwrap();
    assert_eq!(
        s.shrink(),
        Err(Refusal::TooDeep {
            shrink: 20.0,
            room: 20.0
        })
    );
    // The search halves towards the longest that builds.
    let s = Stretch::of_box(lo, hi, [20.0, 0.0, 0.0], X, -8.0).unwrap();
    let mut search = s.shrink_search();
    while let Some((mid, far)) = search.next(&s) {
        assert!((far.level - (20.0 + mid)).abs() < 1e-12);
        search.found(mid <= 3.0);
    }
    assert!((search.most() - 3.0).abs() < 8.0 / 4096.0 + 1e-12);
}

#[test]
fn the_cut_point_is_measured_from_a_face_or_the_fixed_side() {
    assert_eq!(cut_point(X, None, 0.0, 20.0), Some([20.0, 0.0, 0.0]));
    assert_eq!(
        cut_point(X, Some([30.0, 5.0, 5.0]), 0.0, -4.0),
        Some([26.0, 0.0, 0.0])
    );
    assert_eq!(cut_point([0.0; 3], None, 0.0, 1.0), None);
}

#[test]
fn a_thread_axis_moves_or_grows_with_the_stretch() {
    let cut = [50.0, 0.0, 0.0];
    // Upright beyond the cut: it moves 15 with the part.
    assert_eq!(
        moved_axis([80.0, 30.0, 0.0], Z, 0.0, 10.0, cut, X, 15.0),
        ([95.0, 30.0, 0.0], 0.0, 10.0)
    );
    // Upright before the cut: it stays.
    assert_eq!(
        moved_axis([20.0, 30.0, 0.0], Z, 0.0, 10.0, cut, X, 15.0),
        ([20.0, 30.0, 0.0], 0.0, 10.0)
    );
    // Along X across the cut: its far end moves, so it is 15 longer.
    assert_eq!(
        moved_axis([10.0, 30.0, 5.0], X, 0.0, 80.0, cut, X, 15.0),
        ([10.0, 30.0, 5.0], 0.0, 95.0)
    );
    // The same pointing the other way: its start is the end beyond the cut.
    assert_eq!(
        moved_axis(
            [90.0, 30.0, 5.0],
            [-1.0, 0.0, 0.0],
            -10.0,
            70.0,
            cut,
            X,
            15.0
        ),
        ([90.0, 30.0, 5.0], -25.0, 70.0)
    );
}
