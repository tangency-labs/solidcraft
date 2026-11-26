//! Regression test for WR-542 (the fork's tangency-labs/smlib@c0d74ee): both vertical
//! edges at one end of a 40 x 10 x 20 box rounded past half the box's width. Each
//! round's rail on the end face lies inside the band the other takes off it, and no
//! corner trims them (the edges share no vertex); upstream builds a body that is not
//! closed. The fillet used to be refused (no room); since #2784 the two rounds take what
//! each takes alone and meet in a ridge on the end face, as the volume shows.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::Kernel;

fn round_the_end(radius: f64) -> solidcore::Result<f64> {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 10.0, 20.0).unwrap();
    // the two vertical edges at x = 40
    let ends: Vec<_> = b
        .edges()
        .iter()
        .filter(|(_, e)| {
            let c = e.curve.as_ref().unwrap();
            let p = c.evaluate_point(&k, e.interval.min()).unwrap();
            let q = c.evaluate_point(&k, e.interval.max()).unwrap();
            (p.x - 40.0).abs() < 1e-9 && (q.x - 40.0).abs() < 1e-9 && (p.z - q.z).abs() > 1.0
        })
        .map(|(id, _)| id)
        .collect();
    assert_eq!(ends.len(), 2, "the two vertical edges at x = 40");
    fillet_edges(&mut k, &mut b, &ends, radius, 1, 1, 1.0)?;
    Ok(volume_of(&k, &mut b))
}

#[test]
fn wr_542_rounds_wider_than_half_the_end_build_where_they_meet() {
    // #2784: past half the end's width the two rounds take what each takes alone and meet in
    // a ridge on the end face, as below it they leave a strip of it
    for r in [4.0, 6.0] {
        let v = round_the_end(r).unwrap();
        let want = rounded_ends_volume(40.0, 10.0, 20.0, r, 1.0);
        assert!(
            (v - want).abs() <= 1e-6 * want,
            "r = {r}: volume {v}, want {want}"
        );
    }
}

/// The volume of an `l` x `w` x `h` box whose vertical edges at `ends` of its two ends (1 or
/// 2) are rounded `r`, each round taking what it takes alone: at `s` in from an end, each
/// corner's round keeps `y` past `r - sqrt(r^2 - (r - s)^2)` from its side, so the two
/// rounds on a `w` face keep what is left between them (none where they cross); Simpson's
/// rule over the end, the rest a plain block.
fn rounded_ends_volume(l: f64, w: f64, h: f64, r: f64, ends: f64) -> f64 {
    let kept = |s: f64| {
        let off = r - (r * r - (r - s) * (r - s)).max(0.0).sqrt();
        (w - 2.0 * off).max(0.0)
    };
    let n: u32 = 400_000;
    let step = r / f64::from(n);
    let mut sum = kept(0.0) + kept(r);
    for i in 1..n {
        sum += kept(step * f64::from(i)) * if i % 2 == 1 { 4.0 } else { 2.0 };
    }
    h * ((l - ends * r) * w + ends * sum * step / 3.0)
}

fn volume_of(k: &Kernel, b: &mut solidcore::Brep) -> f64 {
    let store = solidcore::geom::attribute::AttributeStore::default();
    let attrs = solidcore::mass::integrator::properties::PropertyAttributes {
        store: &store,
        key_of: &|_| solidcore::geom::attribute::ObjectKey(0),
    };
    solidcore::mass::integrator::properties::brep_volume(k, b, 1e-7, &attrs).unwrap()
}
