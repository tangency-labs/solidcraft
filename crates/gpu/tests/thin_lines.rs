//! Lines thinner than a pixel (the grid's minor lines are 0.7 px) are drawn wherever they fall,
//! without multisampling (#2823). A quad narrower than a pixel covers no pixel centre when it
//! falls between two rows, so such lines came and went as the view zoomed. Skipped, with the
//! reason printed, without a GPU adapter (`SOLIDCRAFT_REQUIRE_GPU=1` makes that a failure).

use solidcraft_engine::geom::Vec3;
use solidcraft_engine::render::{Camera, Rgb, Scene, SceneLine, StandardView};
use solidcraft_gpu::GpuError;
use solidcraft_gpu::capture::Capturer;

/// Pictures drawn with one sample per pixel: with 4× multisampling a thin line covers some
/// samples wherever it falls, which hides the fault.
fn capturer() -> Option<&'static Capturer> {
    static SINGLE: std::sync::OnceLock<solidcraft_gpu::Result<Capturer>> = std::sync::OnceLock::new();
    match SINGLE.get_or_init(Capturer::headless_single_sample).as_ref() {
        Ok(c) => {
            assert_eq!(c.samples(), 1);
            Some(c)
        }
        Err(e @ GpuError::NoAdapter(_)) if std::env::var_os("SOLIDCRAFT_REQUIRE_GPU").is_none() => {
            eprintln!("skipping: {e}");
            None
        }
        Err(e) => panic!("{e}"),
    }
}

/// Lines `width` px wide across a front view, each drawn alone at its own height so it falls at
/// its own fraction of a pixel (0.23 mm apart, about 1.15 px at 5 px per mm); returns the ones
/// that left no pixel.
fn missing(c: &Capturer, width: f64) -> Vec<usize> {
    const N: usize = 40;
    let mut cam = Camera { distance: 62.5, fov: 0.0, ..Default::default() };
    cam.set_view(StandardView::Front);
    (0..N)
        .filter(|&i| {
            let z = -4.0 + i as f64 * 0.23;
            let line =
                SceneLine { points: vec![Vec3::new(-8.0, 0.0, z), Vec3::new(8.0, 0.0, z)], color: Rgb(255, 0, 0), alpha: 1.0, width, on_top: true };
            let scene = Scene { lines: vec![line], radius: 30.0, ..Default::default() };
            let img = c.rgba(&scene, &cam, 200, 200).unwrap();
            !img.as_chunks::<4>().0.iter().any(|p| p[0] > 60 && p[0] > p[1].saturating_add(40) && p[0] > p[2].saturating_add(40))
        })
        .collect()
}

#[test]
fn lines_thinner_than_a_pixel_are_drawn_wherever_they_fall() {
    let Some(c) = capturer() else { return };
    for width in [0.4, 0.7, 1.0, 1.5] {
        let gone = missing(c, width);
        assert!(gone.is_empty(), "{width} px lines: {} of 40 left no pixel ({gone:?})", gone.len());
    }
}
