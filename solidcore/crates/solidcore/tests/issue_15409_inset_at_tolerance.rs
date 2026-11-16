//! A cylinder inset until its side's leftover radius `r - d` reaches the body tolerance `t`
//! (#15409). Measured on main before the fix: the merge cuts the cap circle while `r - d >= t`.
//! Below `t`, the plane/cylinder face intersection returns only a point on the axis and no
//! circle, the caps keep their radius-5 edges, and the generic "not a manifold solid" check
//! refused the body. With the collapse taken at the body tolerance (strict: `r - d < t`
//! collapses), the inset builds exactly up to the largest `d` whose leftover is still `>= t`.
//! Past it, it is refused as `Collapses`, naming that `d`. Closed form: the named distance is
//! the largest f64 `n` with `5 - n >= t` (here 4.9999899999999995, 1 ulp under 4.99999).

#![allow(clippy::unwrap_used, clippy::panic)] // test helpers

use solidcore::error::offset::OffsetFailure;
use solidcore::error::Reason;
use solidcore::kernel::Kernel;
use solidcore::mass::enclosure::{enclose_volume, EnclosureStop};
use solidcore::math::Point3;
use solidcore::offset::offset_executive::p2::{offset_brep_simple, OffsetOptions};
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::{Brep, SolidError};

/// The cylinder's radius and height.
const R: f64 = 5.0;
const H: f64 = 20.0;

/// The cylinder inset by `d` with rounded edges, merged, and the body tolerance.
fn inset(d: f64) -> (Result<Brep, SolidError>, f64) {
    let mut k = Kernel::default();
    let mut b = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), R, H).unwrap();
    let t = b.tolerance();
    let options = OffsetOptions {
        distance: -d,
        extend_convex_edges: false,
        merge_results: true,
    };
    (offset_brep_simple(&mut k, &mut b, options), t)
}

/// The collapse refusal behind `e`: its distance asked and the distance it names.
fn collapse(e: &SolidError) -> Option<(f64, f64)> {
    std::iter::successors(Some(e), |e| e.cause()).find_map(|e| {
        match e.failure().map(|f| &f.reason) {
            Some(&Reason::Offset(OffsetFailure::Collapses {
                distance, radius, ..
            })) => Some((distance, radius)),
            _ => None,
        }
    })
}

/// The next f64 above the positive finite `x` (`f64::next_up` needs Rust 1.86; the MSRV is
/// 1.85). For positive finite `x` the bit patterns are ordered as the values.
fn up(x: f64) -> f64 {
    f64::from_bits(x.to_bits() + 1)
}

/// The next f64 below the positive finite, nonzero `x`.
fn down(x: f64) -> f64 {
    f64::from_bits(x.to_bits() - 1)
}

/// The largest f64 inset whose leftover radius `R - d` is still at least `t`.
fn last_building(t: f64) -> f64 {
    let mut n = R - t;
    while R - n < t {
        n = down(n);
    }
    while R - up(n) >= t {
        n = up(n);
    }
    n
}

#[test]
fn the_last_inset_whose_leftover_reaches_the_tolerance_builds() {
    let t = create_cylinder(&mut Kernel::default(), Point3::new(0.0, 0.0, 0.0), R, H)
        .unwrap()
        .tolerance();
    let n = last_building(t);
    // 1 ulp under 4.99999, where the leftover 5 - 4.99999 rounds below 1e-5
    assert_eq!(n, 4.999_989_999_999_999_5);
    let (built, _) = inset(n);
    let mut b = built.unwrap_or_else(|e| panic!("inset by {n}: {e}"));
    assert!(is_manifold_solid(&b).unwrap(), "inset by {n}");
    // The offset of the analytic cylinder is an exact cylinder of radius r = R - n and its caps
    // exact planes at n and H - n, so the volume is pi r^2 h, h = H - 2n. r is exact in f64
    // (Sterbenz: n is within a factor 2 of R); h = H - 2n rounds once (2n is exact), by at
    // most half an ulp of h; `PI` and the two products round by half an ulp each, so the
    // closed form holds to 4 half-ulps relative, within 8 ulps.
    let k = Kernel::default();
    let (r, h) = (R - n, H - 2.0 * n);
    let pi = std::f64::consts::PI;
    let exact = pi * r * r * h;
    let slack = 8.0 * f64::EPSILON * exact;
    // What the volume is asked to show: that the body has a volume (lo > 0) and is the cylinder
    // of radius r to within the body tolerance t, i.e. its volume lies in the band
    // [pi (r - t)^2 h, pi (r + t)^2 h], about 4 times the volume wide.
    // Each band end is a closed form of the same shape, 6 roundings at most: within 8 ulps.
    // At r ~ t the body's volume is only defined to within this band, so these volume checks
    // are a sanity check (it built, lo > 0, consistent with a radius-r cylinder), not an
    // accuracy check; the test's job is that the named inset builds and the next is refused.
    let (band_lo, band_hi) = (pi * (r - t) * (r - t) * h, pi * (r + t) * (r + t) * h);
    // An enclosure holding the volume V proves lo > 0 only if it is narrower than V: ask V / 2.
    let e = enclose_volume(&k, &mut b, exact / 2.0).unwrap();
    // The certified enclosure holds the volume, so it must hold the closed form (to its
    // rounding).
    assert!(
        e.lo - slack <= exact && exact <= e.hi + slack,
        "inset by {n}: the volume is in [{}, {}] ({:?}), the closed form {exact}",
        e.lo,
        e.hi,
        e.stop
    );
    assert_eq!(e.stop, EnclosureStop::Reached, "inset by {n}: {e:?}");
    // The body has a volume: the leftover radius did not collapse.
    assert!(e.lo > 0.0, "inset by {n}: {e:?}");
    // And it is the cylinder of radius r to within t.
    assert!(
        band_lo - 8.0 * f64::EPSILON * band_lo <= e.lo
            && e.hi <= band_hi + 8.0 * f64::EPSILON * band_hi,
        "inset by {n}: the volume is in [{}, {}], not within [{band_lo}, {band_hi}]",
        e.lo,
        e.hi
    );
}

#[test]
fn an_inset_leaving_less_than_the_tolerance_is_refused_naming_the_last_that_builds() {
    for d in [4.99999, 4.999991, 4.999995] {
        let (built, t) = inset(d);
        assert!(R - d < t, "inset by {d} leaves {}", R - d);
        let Err(e) = built else {
            panic!("inset by {d} (leftover {}) built", R - d);
        };
        let Some((asked, named)) = collapse(&e) else {
            panic!("inset by {d}: {e}");
        };
        assert_eq!(asked, -d);
        assert_eq!(named, last_building(t), "inset by {d}");
    }
}
