//! tangency-labs/solidcore#575 (WR-304): an r 37 disc, z -3.6..0, built as the disc
//! z -1.2..0 joined with the regions of a sketch with chords, z -3.6..-1.2, is joined with
//! an annulus r 35..37, z -5.6..-3.6, under it, and SMLib's check refuses the result
//! (`SmTrimmingTools` tests #3 and #4, "Face Loop has bad orientation", "zero area").
//!
//! `SmMerge::BooleanPostProcess` removes the topological edges between the faces it
//! merges, but an operand edge between two faces that this way become one stays, with
//! the merged face on both sides: a strut. Joining the regions to the disc leaves one at
//! each chord end on the wall, from the rim up to z = -1.2. Joining the annulus removes
//! the rim under them, and each strut becomes a loop of its two edge uses, of zero area.
//! The opt-in `Kernel::fixes.boolean_cleanup_removes_slits` (WR-304, #1120) also checks
//! the edges a face merge leaves with one face, and removes the struts.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Vec3;
use solidcore::primitive::api_curves::{create_arc, create_circle, create_line_segment};
use solidcore::sweep::api_brep::create_taper_extrude;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::{Brep, Kernel};

use std::f64::consts::PI;

const R: f64 = 37.0;

/// The volume of the joined body: the disc, z -3.6..0, and the annulus, z -5.6..-3.6.
fn expected_volume() -> f64 {
    PI * (R * R * 3.6 + (R * R - 35.0 * 35.0) * 2.0)
}

/// `curves` (in their plane z) extruded `height` along +z and capped.
fn extrude(k: &mut Kernel, curves: &[Curve], height: f64) -> Brep {
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_taper_extrude(k, &refs, height, 0.0, 3, false).unwrap()
}

fn circle(z: f64, r: f64) -> Curve {
    Curve::from(create_circle(Vec3::new(0.0, 0.0, z), r).unwrap())
}

fn line(z: f64, a: (f64, f64), b: (f64, f64)) -> Curve {
    Curve::from(create_line_segment(Vec3::new(a.0, a.1, z), Vec3::new(b.0, b.1, z)).unwrap())
}

/// The arc of the r 37 circle in z counterclockwise from `start` to `end` degrees, split
/// at 0 degrees when it passes it.
fn arcs(z: f64, start: f64, end: f64) -> Vec<Curve> {
    let arc = |s: f64, e: f64| Curve::from(create_arc(Vec3::new(0.0, 0.0, z), R, s, e).unwrap());
    if end > 360.0 {
        vec![arc(start, 360.0), arc(0.0, end - 360.0)]
    } else {
        vec![arc(start, end)]
    }
}

/// The region of the r 37 circle in z between the chords `y = lo` and `y = hi` (`None`:
/// the circle itself), counterclockwise.
fn region(z: f64, lo: Option<f64>, hi: Option<f64>) -> Vec<Curve> {
    let angle = |y: f64| (y / R).asin().to_degrees();
    let x = |y: f64| (R * R - y * y).sqrt();
    let mut curves = Vec::new();
    match (lo, hi) {
        (None, Some(h)) => {
            curves.extend(arcs(z, 180.0 - angle(h), 360.0 + angle(h)));
            curves.push(line(z, (x(h), h), (-x(h), h)));
        }
        (Some(l), None) => {
            curves.extend(arcs(z, angle(l), 180.0 - angle(l)));
            curves.push(line(z, (-x(l), l), (x(l), l)));
        }
        (Some(l), Some(h)) => {
            curves.extend(arcs(z, angle(l), angle(h)));
            curves.push(line(z, (x(h), h), (-x(h), h)));
            curves.extend(arcs(z, 180.0 - angle(h), 180.0 - angle(l)));
            curves.push(line(z, (-x(l), l), (x(l), l)));
        }
        (None, None) => curves.push(circle(z, R)),
    }
    curves
}

/// The issue's Body1: the disc z -1.2..0 joined with the regions of the sketch whose
/// chords are `chords` (ascending), each extruded z -3.6..-1.2 and joined in turn.
fn body1(k: &mut Kernel, chords: &[f64]) -> Brep {
    let mut bounds = vec![None];
    bounds.extend(chords.iter().map(|&y| Some(y)));
    bounds.push(None);
    let mut regions: Option<Brep> = None;
    for pair in bounds.windows(2) {
        let piece = extrude(k, &region(-3.6, pair[0], pair[1]), 2.4);
        regions = Some(match regions {
            None => piece,
            Some(b) => boolean_union(k, b, piece).unwrap(),
        });
    }
    let disc = extrude(k, &[circle(-1.2, R)], 1.2);
    boolean_union(k, disc, regions.unwrap()).unwrap()
}

/// The issue's Body2: the annulus r 35..37, z -5.6..-3.6.
fn body2(k: &mut Kernel) -> Brep {
    let outer = extrude(k, &[circle(-5.6, R)], 2.0);
    let inner = extrude(k, &[circle(-6.0, 35.0)], 3.0);
    boolean_difference(k, outer, inner).unwrap()
}

struct Joined {
    volume: f64,
    manifold: bool,
    check: Vec<String>,
}

/// Body1 of the sketch with `chords`, built with the strut clean-up `body_fix`, joined
/// with Body2 with the clean-up `join_fix`.
fn join(chords: &[f64], body_fix: bool, join_fix: bool) -> Joined {
    let mut k = Kernel::default();
    k.fixes.boolean_cleanup_removes_slits = body_fix;
    let b1 = body1(&mut k, chords);
    let b2 = body2(&mut k);
    k.fixes.boolean_cleanup_removes_slits = join_fix;
    let mut joined = boolean_union(&mut k, b1, b2).unwrap();
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let volume = brep_volume(&k, &mut joined, 1e-6, &attrs).unwrap();
    let manifold = is_manifold_solid(&joined).unwrap();
    let check = check_brep(&mut k, &mut joined, CheckLevel::Level2)
        .unwrap()
        .lines();
    Joined {
        volume,
        manifold,
        check,
    }
}

fn assert_volume(j: &Joined, what: &str) {
    let want = expected_volume();
    assert!(
        (j.volume - want).abs() <= 1e-6 * want,
        "{what}: volume {}, want {want}",
        j.volume
    );
}

#[test]
fn issue_575_join_is_refused_by_the_check_by_default() {
    // upstream: the right volume, but SMLib's check refuses the wall's strut loops
    let j = join(&[0.0, 8.0], false, false);
    assert_volume(&j, "default");
    assert!(
        j.check
            .iter()
            .any(|l| l.contains("Test #3 Loop Orientation") && l.contains("bad orientation"))
            && j.check
                .iter()
                .any(|l| l.contains("Test #4") && l.contains("zero area")),
        "default: {:?}",
        j.check
    );
}

#[test]
fn issue_575_join_is_valid_with_the_strut_clean_up() {
    // the issue's two chords and its other failing pair, with the clean-up throughout,
    // and with Body1 built without it (its struts already there) and only the join
    // cleaned up
    for (chords, body_fix) in [
        (&[0.0, 8.0][..], true),
        (&[32.0, 35.0][..], true),
        (&[0.0, 8.0][..], false),
    ] {
        let what = format!("chords {chords:?}, Body1 cleaned up {body_fix}");
        let j = join(chords, body_fix, true);
        assert_volume(&j, &what);
        assert!(j.manifold, "{what}: not a manifold solid");
        assert!(j.check.is_empty(), "{what}: {:?}", j.check);
    }
}
