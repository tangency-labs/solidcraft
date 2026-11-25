//! The SolidCraft fork's tangency-labs/smlib@eb883ad (WR-703, #2273): a boss or a hole inside a
//! round's band stays, the round trimmed round it in place; the cases are the fork's ddbb782.
//!
//! The 40 x 30 x 20 block's top front edge (y = 0, z = 20) is rounded; the feature, a cylinder of
//! radius 2 about the upright x = 20, y = 4, stands 2 to 6 in from the front on the top. A round
//! of radius r takes the band y < r off the top and z > 20 - r off the front, so at r 8 the
//! footprint lies inside the band, at r 1.9 the band stops short of it.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test helpers

#[path = "support/volume_leak.rs"]
mod volume_leak;

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::{KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::gamma;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, PI};
use solidcore::primitive::api_primitives::{create_box, create_cone, create_cylinder};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel, Result};

/// The block with a boss 5 tall on its top, or a hole 5 deep in it.
fn featured_block(k: &mut Kernel, boss: bool) -> Brep {
    let block = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
    if boss {
        let c = create_cylinder(k, Point3::new(20.0, 4.0, 20.0), 2.0, 5.0).unwrap();
        boolean_union(k, block, c).unwrap()
    } else {
        let c = create_cylinder(k, Point3::new(20.0, 4.0, 15.0), 2.0, 6.0).unwrap();
        boolean_difference(k, block, c).unwrap()
    }
}

/// The edge nearest the point, within 0.1.
fn pick(k: &Kernel, b: &Brep, q: Point3) -> EdgeId {
    let mut best = (0.1f64, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance <= best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.expect("an edge there")
}

fn volume_of(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-7, &attrs).unwrap()
}

/// The material the round r 8 takes from under the footprint, down from the top to the round's
/// surface z = 12 + sqrt(64 - (8 - y)^2): over the disc, y = 4 + 2 sin t, its chord 4 cos t,
/// Int 8 cos^2 t (8 - sqrt(64 - (4 - 2 sin t)^2)) dt over t in [-pi/2, pi/2], by Simpson's rule.
fn column_under_footprint() -> f64 {
    let spans = 4000;
    let h = PI / f64::from(spans);
    let f = |t: f64| {
        let (c, d) = (t.cos(), 4.0 - 2.0 * t.sin());
        8.0 * c * c * (8.0 - (64.0 - d * d).sqrt())
    };
    let mut sum = f(-0.5 * PI) + f(0.5 * PI);
    for i in 1..spans {
        sum += f(-0.5 * PI + h * f64::from(i)) * if i % 2 == 1 { 4.0 } else { 2.0 };
    }
    sum * h / 3.0
}

/// The round's cross-section area taken off, for its 40 long edge.
fn spandrel(r: f64) -> f64 {
    (1.0 - PI / 4.0) * r * r
}

/// The block less its round r, with the boss's 20 pi on it or the hole's 20 pi out of it.
fn featured_rounded(boss: bool, r: f64) -> f64 {
    24000.0 - 40.0 * spandrel(r) + if boss { 20.0 * PI } else { -20.0 * PI }
}

/// How far round the feature's axis the edges lying on both its wall and the round r 8's surface
/// run, in radians: 2 pi where the wall comes down to the round all round its footprint.
fn wall_on_round(k: &Kernel, b: &Brep) -> f64 {
    let tol = 100.0 * b.tolerance();
    let mut turned = 0.0;
    for &e in b.edge_list() {
        let Some(curve) = edge::curve(b, e).unwrap() else {
            continue;
        };
        let ivl = edge::interval(b, e).unwrap();
        let (mut span, mut last) = (0.0, 0.0);
        let mut on = true;
        for s in 0..=256 {
            let p = curve
                .evaluate_point(k, ivl.evaluate(f64::from(s) / 256.0))
                .unwrap();
            on = ((p.x - 20.0).hypot(p.y - 4.0) - 2.0).abs() < tol
                && ((p.y - 8.0).hypot(p.z - 12.0) - 8.0).abs() < tol;
            if !on {
                break;
            }
            let a = (p.y - 4.0).atan2(p.x - 20.0);
            if s > 0 {
                let d: f64 = a - last;
                span += d - (2.0 * PI) * (d / (2.0 * PI)).round();
            }
            last = a;
        }
        if on {
            turned += f64::abs(span);
        }
    }
    turned
}

/// One sound solid of volume `want` within `rel` with `faces` faces; with `reach`, the wall on
/// the round all round its footprint.
fn assert_feature_in_band(boss: bool, r: f64, want: f64, rel: f64, faces: usize, reach: bool) {
    let mut k = Kernel::default();
    let mut b = featured_block(&mut k, boss);
    let e = pick(&k, &b, Point3::new(20.0, 0.0, 20.0));
    fillet_edges(&mut k, &mut b, &[e], r, 1, 1, 1.0).unwrap();
    let v = volume_of(&k, &mut b);
    let turned = wall_on_round(&k, &b);
    println!(
        "boss {boss} r {r}: volume {v:.9}, want {want:.9}; {} faces, want {faces}; wall on the round {turned:.9}",
        b.face_list().len()
    );
    assert!((v - want).abs() <= rel * want, "volume {v}, want {want}");
    assert_eq!(b.face_list().len(), faces);
    if reach {
        assert!(
            (turned - 2.0 * PI).abs() < 1e-9,
            "wall on the round {turned}"
        );
    } else {
        assert!(turned.abs() < 1e-12, "wall on the round {turned}");
    }
    assert!(solidcore::topology::brep::p2::is_manifold_solid(&b).unwrap());
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
}

/// To 1e-7, as the fork's own case at 5e8c9c1 (its 1e-9 at ddbb782 went to 1e-7 in the same PR):
/// the crease is fitted within a tenth of the fillet's tolerance, which moves the column's volume
/// by about ten times that.
#[test]
fn the_boss_in_the_band_is_kept() {
    let want = featured_rounded(true, 8.0) + column_under_footprint();
    assert_feature_in_band(true, 8.0, want, 1e-7, 9, true);
}

#[test]
fn the_hole_in_the_band_is_kept() {
    let want = featured_rounded(false, 8.0) + column_under_footprint();
    assert_feature_in_band(false, 8.0, want, 1e-7, 9, true);
}

#[test]
fn a_band_short_of_the_feature_leaves_it() {
    for boss in [true, false] {
        assert_feature_in_band(boss, 1.9, featured_rounded(boss, 1.9), 1e-9, 9, false);
    }
}

// ---- With the fork's opt-ins at eb883ad (the figures: `assert_pinned`) ----

/// The opt-ins these figures rest on, found by removing every flag the figures don't need, so a
/// new flag on `main` can't move them. Not `UpstreamFixes::all()`: it holds fixes the fork doesn't
/// have.
fn fork_kernel() -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    let mut fixes = UpstreamFixes::default();
    fixes.linear_sweep_arc_is_cylinder = true;
    k.fixes = fixes;
    k
}

type Feature = fn(&mut Kernel, Brep) -> Result<Brep>;

/// The block with a feature, its top front edge rounded `r`, checked valid.
fn rounded_body(feature: Feature, r: f64) -> Result<(Kernel, Brep)> {
    let mut k = fork_kernel();
    let block = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0)?;
    let mut b = feature(&mut k, block)?;
    let e = pick(&k, &b, Point3::new(5.0, 0.0, 20.0));
    fillet_edges(&mut k, &mut b, &[e], r, 1, 1, 1.0)?;
    assert!(solidcore::topology::brep::p2::is_manifold_solid(&b).unwrap());
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
    Ok((k, b))
}

/// The block with a feature, its top front edge rounded `r`: the volume and the faces.
fn rounded_with(feature: Feature, r: f64) -> Result<(f64, usize)> {
    let (k, mut b) = rounded_body(feature, r)?;
    Ok((volume_of(&k, &mut b), b.face_list().len()))
}

/// Pins solidcore's own volume (at accuracy 1e-7, on Linux, printed with 17 digits) and face
/// count of the block with `feature`, rounded `r`; these are not the fork's figures. They were
/// checked against two references. Against the fork's `smlib_brep_volume` at the same
/// accuracy: the face counts and statuses are the fork's; the prism boss's volume is the fork's
/// bit for bit, the prism pocket's and the taken features' within a last bit, and those of a
/// body with a cylinder or a cone face within the 1e-7 they are integrated to (by -2.4e-7 at
/// most, 1e-11 of it), since solidcore's precise properties integrate each iso-u line from the
/// trims' lowest v (WR-563, #1556) where the fork starts at the face UV domain's, and its steps
/// read a fifth-order error estimate (#1556, 5.0e-13 at most). Against the closed forms: the
/// r 1.9 boss and hole are off by +2.0e-7 and -3.6e-7 (the fork's +4.4e-7 and -1.9e-7), and the
/// r 8 kept cylinder boss and hole, cone boss and 19 degree lean stay 3.4e-5 to 4.8e-5 above
/// theirs, the crease's fit, as the fork's do; the cone pit has none. (Those figures were
/// before #16735: `brep_volume` now closes the tolerant edges' gaps, and the r 8 cylinder boss
/// and hole, cone boss and pit and 19 degree lean moved down by 3.4e-5 to 6.1e-5, the old
/// integrator's leak through their marched creases; the prisms and the r 1.9 hole by a last
/// bit. Each pin carries its figure from before, and the move is checked to be that leak.
/// Integrating the faces on their surfaces continued where their trims leave the domain
/// moved the r 8 cylinder boss and hole, cone boss and pit, 19 degree lean, prism boss and
/// pocket and r 1.9 hole by one more last bit; the prisms and the r 1.9 hole are back on
/// their figures from before.)
///
/// Since the trim pieces integrate by Gauss-Kronrod (#14460) every figure, `was` and pinned,
/// is that integrator's: forced to accuracy 1e-11 and 1e-12 it gives each pinned volume to
/// 7.6e-15 relative, and the earlier Runge-Kutta trim pieces forced to 1e-12 agree with that to
/// 7.8e-16. The Runge-Kutta figures at 1e-7 were 8.4e-12 to 4.5e-11 off. The prisms, the
/// r 1.9 boss and hole and the pocket astride the contact line move by nothing in #16735's
/// gap closure here, and the prism boss and pocket differ by exactly their two prisms' 160.
/// The comparisons above are of the Runge-Kutta figures. With these the prism boss is 3.8e-7
/// above the fork's (no longer bit for bit), the pocket astride the contact line 1.05e-6
/// below the fork's 5e8c9c1 figure, and the r 1.9 boss and hole move by -2.0e-7 and +3.5e-7,
/// onto their closed forms to within the rounding of the offsets quoted above (about 1e-8).
/// Summing each Gauss-Kronrod rule about its centre value (exact on a constant integrand)
/// moved the r 8 cylinder boss, cone boss and 19 degree lean, and the prism boss and r 1.9
/// boss (`was` too: the gap closure moves neither), by one last bit each.
/// Transposing a swapped face's trims exactly, where they were mirrored through a rounded
/// matrix (WR-729), moves trim coordinates by up to an ulp and no edge, vertex or surface:
/// the r 8 cylinder boss, cone boss and pit and 19 degree lean by one last bit each, every
/// one still within 3.2e-16 relative of its 70-digit reference (`was` is left as it was); the
/// three that move away from theirs stay within their volume sum's rounding bound
/// (`transposed_pins_lie_within_their_sums_rounding_of_the_references`).
/// Closing a plane's straight edges in one piece where their strips' correspondence does
/// not bend (the strip integrand a polynomial the Kronrod rule integrates exactly on any
/// piece, so only the sum's rounding changes) moves the r 8 cylinder boss and cone boss
/// back by their last bit, onto main's figures from before WR-729 (the cone boss onto its
/// 70-digit reference's nearest double), and the prism boss and r 1.9 cylinder boss by
/// their last bit.
///
/// `was` is the figure before `brep_volume` closed the tolerant edges' gaps (#16735); where it
/// differs, the move is checked to be that integrator's leak (`support/volume_leak.rs`).
/// Returns the volume, for [`assert_near_closed_form`].
fn assert_pinned(feature: Feature, r: f64, was: f64, volume: f64, faces: usize) -> f64 {
    let (k, mut b) = rounded_body(feature, r).unwrap();
    let (v, n) = (volume_of(&k, &mut b), b.face_list().len());
    if was != volume {
        volume_leak::assert_moved_by_the_old_leak(&k, &mut b, was, volume, 1e-7);
    }
    // Bit for bit where the figures were taken (Linux). Elsewhere the platform's libm can move
    // the last bit (one on Windows, the kept hole).
    if cfg!(target_os = "linux") {
        assert_eq!(
            v.to_bits(),
            volume.to_bits(),
            "volume {v:.17}, pinned {volume:.17}"
        );
    } else {
        assert!(
            (v - volume).abs() <= 1e-14 * volume,
            "volume {v:.17}, pinned {volume:.17}"
        );
    }
    assert_eq!(n, faces);
    v
}

/// How far a pinned volume may lie from its closed form, relative: the bound this file holds
/// the r 1.9 rounds to (`a_band_short_of_the_feature_leaves_it`). Each pin with a closed form
/// lies within 6.6e-16 relative of it since the Gauss-Kronrod trim pieces (MEASURED against
/// 70-digit references), so the bits are not all the test rests on.
const CLOSED_FORM_REL: f64 = 1e-9;

fn assert_near_closed_form(v: f64, want: f64) {
    assert!(
        (v - want).abs() <= CLOSED_FORM_REL * want,
        "volume {v:.17}, closed form {want:.17}"
    );
}

/// `int_2^6 zr(y) dy`, zr(y) = 12 + sqrt(64 - (8 - y)^2) the round r 8's surface over the
/// prisms' footprint 2 <= y <= 6: with u = 8 - y, `[u sqrt(64 - u^2) / 2 + 32 asin(u / 8)]`
/// from 2 to 6.
fn round_over_prism() -> f64 {
    let f = |u: f64| 0.5 * u * (64.0 - u * u).sqrt() + 32.0 * (u / 8.0).asin();
    48.0 + f(6.0) - f(2.0)
}

/// The frustum of radii `r0` and `r1`, `h` tall.
fn frustum(r0: f64, r1: f64, h: f64) -> f64 {
    PI * h / 3.0 * (r0 * r0 + r0 * r1 + r1 * r1)
}

/// The cone pit's void left under the round r 8: its slice at height z (radius
/// 1 + (z - 15) / 5.5 about y = 4) holds material only for y >= 8 - sqrt(64 - (z - 12)^2), a
/// circular segment; integrated over z by Simpson's rule, split where that line enters and
/// leaves the disc (its error there is 1.5e-11, against the 70-digit reference).
fn cone_pit_void() -> f64 {
    let radius = |z: f64| 1.0 + (z - 15.0) / 5.5;
    let front = |z: f64| 8.0 - (64.0 - (z - 12.0) * (z - 12.0)).max(0.0).sqrt();
    let segment = |z: f64| {
        let (r, d) = (radius(z), front(z) - 4.0);
        if d <= -r {
            PI * r * r
        } else if d >= r {
            0.0
        } else {
            r * r * (d / r).acos() - d * (r * r - d * d).sqrt()
        }
    };
    let crossing = |g: &dyn Fn(f64) -> f64| {
        let (mut lo, mut hi) = (15.0, 20.0);
        for _ in 0..200 {
            let mid = 0.5 * (lo + hi);
            if (g(mid) > 0.0) == (g(lo) > 0.0) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    };
    let enters = crossing(&|z| front(z) - 4.0 + radius(z));
    let leaves = crossing(&|z| front(z) - 4.0 - radius(z));
    let simpson = |a: f64, b: f64| {
        let spans = 20000;
        let h = (b - a) / f64::from(spans);
        let mut sum = segment(a) + segment(b);
        for i in 1..spans {
            sum += segment(a + h * f64::from(i)) * if i % 2 == 1 { 4.0 } else { 2.0 };
        }
        sum * h / 3.0
    };
    simpson(15.0, enters) + simpson(enters, leaves)
}

/// The block less its round r 8, with no feature.
fn rounded_r8() -> f64 {
    24000.0 - 40.0 * spandrel(8.0)
}

/// The block's round with neither feature; a kept feature's figure is this plus its own part.
const TAKEN: f64 = 23450.619298757134;

fn cylinder_boss(k: &mut Kernel, block: Brep) -> Result<Brep> {
    let c = create_cylinder(k, Point3::new(20.0, 4.0, 20.0), 2.0, 5.0)?;
    boolean_union(k, block, c)
}

fn cylinder_hole(k: &mut Kernel, block: Brep) -> Result<Brep> {
    let c = create_cylinder(k, Point3::new(20.0, 4.0, 15.0), 2.0, 6.0)?;
    boolean_difference(k, block, c)
}

fn prism_boss(k: &mut Kernel, block: Brep) -> Result<Brep> {
    let c = create_box(k, Point3::new(18.0, 2.0, 20.0), 4.0, 4.0, 5.0)?;
    boolean_union(k, block, c)
}

fn prism_pocket(k: &mut Kernel, block: Brep) -> Result<Brep> {
    let c = create_box(k, Point3::new(18.0, 2.0, 15.0), 4.0, 4.0, 6.0)?;
    boolean_difference(k, block, c)
}

fn cone_boss(k: &mut Kernel, block: Brep) -> Result<Brep> {
    let c = create_cone(k, Point3::new(20.0, 4.0, 20.0), 2.0, 1.0, 5.0)?;
    boolean_union(k, block, c)
}

fn cone_pit(k: &mut Kernel, block: Brep) -> Result<Brep> {
    let c = create_cone(k, Point3::new(20.0, 4.0, 15.0), 1.0, 2.0, 5.5)?;
    boolean_difference(k, block, c)
}

#[test]
fn cylinders_keep_their_pinned_volumes() {
    let column = column_under_footprint();
    let v = assert_pinned(
        cylinder_boss,
        8.0,
        23528.165527389272,
        23528.165493405155,
        9,
    );
    assert_near_closed_form(v, featured_rounded(true, 8.0) + column);
    let v = assert_pinned(
        cylinder_hole,
        8.0,
        23402.501834781691,
        23402.501787261564,
        9,
    );
    assert_near_closed_form(v, featured_rounded(false, 8.0) + column);
    let v = assert_pinned(
        cylinder_boss,
        1.9,
        24031.843347866372,
        24031.843347866376,
        9,
    );
    assert_near_closed_form(v, featured_rounded(true, 1.9));
    let v = assert_pinned(
        cylinder_hole,
        1.9,
        23906.179641722782,
        23906.179641722782,
        9,
    );
    assert_near_closed_form(v, featured_rounded(false, 1.9));
}

/// Planar walls: each crease is an exact section.
#[test]
fn prisms_keep_their_pinned_volumes() {
    // the boss's walls carried down to the round, the pocket's floor at z 15
    let under = round_over_prism();
    let v = assert_pinned(prism_boss, 8.0, 23549.89626015013, 23549.896260150133, 12);
    assert_near_closed_form(v, rounded_r8() + 4.0 * (25.0 * 4.0 - under));
    let v = assert_pinned(
        prism_pocket,
        8.0,
        23389.896260150133,
        23389.896260150133,
        12,
    );
    assert_near_closed_form(v, rounded_r8() - 4.0 * (under - 15.0 * 4.0));
}

/// WR-729 (#17902): the exact trim transpose moves three pins one last bit FARTHER from their
/// 70-digit references (the r 8 cylinder boss by 3.64e-12, the cone boss by 1.18e-12, the cone
/// pit by 3.19e-12, MEASURED on the exact doubles). Each pinned volume, main's and this
/// branch's, lies within the rounding bound of the volume's own sum, so the move is a change of
/// rounding, not of the integral:
///
/// `brep_volume` adds one term per face (and more for the gaps it closes, #16735) into
/// `total.volume`. For `m` terms `t_i` that recursive sum is off its exact value by at most
/// `R = gamma(m - 1) sum |t_i|` (Higham, Lemma 3.1, `solidcore::mass::integrator::gamma`),
/// before each term's own error and the inputs' (`L e_in`), which only add. With `n <= m` the
/// face count and `|v| <= (1 + gamma(m - 1)) sum |t_i|` (the computed sum is off `sum t_i` by
/// `R` too), `B = gamma(n - 1) |v|` is at most `(1 + gamma(m - 1)) R`, a factor within
/// 1e-14 of one. The test asserts `2 |v - exact| <= B`, so `|v - exact| <= R`: the margin of
/// two covers that factor. `exact` is the 70-digit reference (`pins-ref70-gk.py`, tanh-sinh at level 7,
/// which moves each reference by at most 5.7e-45 from level 6), held here as its nearest
/// double, whose own rounding (half an ulp) is added to the distance.
#[test]
fn transposed_pins_lie_within_their_sums_rounding_of_the_references() {
    // (feature, pinned volume, main's pin before WR-729, 70-digit reference, its nearest
    // double, faces)
    let pins: [(Feature, f64, f64, &str, f64, usize); 3] = [
        (
            cylinder_boss,
            23528.165493405155,
            23528.165493405155,
            "23528.1654934051575356764322355",
            23528.16549340516,
            9,
        ),
        (
            cone_boss,
            23501.985554625244,
            23501.985554625244,
            "23501.9855546252425920225768740",
            23501.985554625244,
            11,
        ),
        (
            cone_pit,
            23427.54082790698,
            23427.540827906978,
            "23427.5408279069781598768686995",
            23427.540827906978,
            9,
        ),
    ];
    for (feature, pinned, main, reference, exact, faces) in pins {
        let (v, n) = rounded_with(feature, 8.0).unwrap();
        assert_eq!(n, faces, "{reference}");
        assert!(
            (v - pinned).abs() <= 1e-14 * pinned,
            "volume {v:.17}, pinned {pinned:.17}"
        );
        let half_ulp = 0.5 * (f64::from_bits(exact.to_bits() + 1) - exact);
        for (side, x) in [("this branch's", v), ("main's", main)] {
            let off = (x - exact).abs() + half_ulp;
            let bound = gamma(u32::try_from(n - 1).unwrap()) * x.abs();
            println!("{side} {x:.17}: {off:e} off {reference}, sum's rounding bound {bound:e}");
            assert!(
                2.0 * off <= bound,
                "{side} volume {x:.17} is {off:e} off its reference {reference}, at least half \
                 the sum's rounding bound {bound:e}"
            );
        }
    }
}

/// Walls that lean: kept as the fork keeps them, the boss's with no refusal.
#[test]
fn cones_keep_their_pinned_volumes() {
    // the boss's footprint (radius 2) swept down to the round, as keep features continues a
    // boss's walls along the top's normal
    let v = assert_pinned(cone_boss, 8.0, 23501.985588609361, 23501.985554625244, 11);
    assert_near_closed_form(
        v,
        rounded_r8() + frustum(2.0, 1.0, 5.0) + column_under_footprint(),
    );
    let v = assert_pinned(cone_pit, 8.0, 23427.54088905338, 23427.54082790698, 9);
    assert_near_closed_form(v, rounded_r8() - cone_pit_void());
}

/// The round beside the cone pit is a B-spline face whose trims cross three of its surface's v
/// knot lines, where the trims' images have derivatives that are not smooth. The face's own
/// trim pieces and its tolerant edges' gap-closure strips both integrate along those
/// trims, and both end each piece at the crossings. So the volume at the default accuracy 1e-7
/// equals the volume at 1e-12, to within the rounding bound of the volume's sum (as in
/// `transposed_pins_lie_within_their_sums_rounding_of_the_references`).
///
/// A face whose pieces end at the crossings, with strips that run across them, is 1.7e-10 off
/// at 1e-7. That is eight times the bound (MEASURED).
#[test]
fn cone_pit_volume_at_the_default_accuracy_is_converged() {
    let (k, mut b) = rounded_body(cone_pit, 8.0).unwrap();
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let default = brep_volume(&k, &mut b, 1e-7, &attrs).unwrap();
    let tight = brep_volume(&k, &mut b, 1e-12, &attrs).unwrap();
    let n = b.face_list().len();
    let bound = gamma(u32::try_from(n - 1).unwrap()) * tight.abs();
    assert!(
        (default - tight).abs() <= bound,
        "volume {default:.17} at 1e-7, {tight:.17} at 1e-12: {:e} apart, bound {bound:e}",
        (default - tight).abs()
    );
}

/// A boss's walls count as upright within the fillet's angular tolerance: at 19 degrees off they
/// are carried down, at 30 and 45 the round is refused, as the fork.
#[test]
fn a_boss_leaning_past_the_angular_tolerance_is_refused_as_the_fork() {
    fn lean_19(k: &mut Kernel, block: Brep) -> Result<Brep> {
        let c = create_cone(k, Point3::new(20.0, 4.0, 20.0), 2.0, 1.0, 2.9)?;
        boolean_union(k, block, c)
    }
    fn lean_30(k: &mut Kernel, block: Brep) -> Result<Brep> {
        let c = create_cone(k, Point3::new(20.0, 4.0, 20.0), 2.0, 1.0, 1.732)?;
        boolean_union(k, block, c)
    }
    fn lean_45(k: &mut Kernel, block: Brep) -> Result<Brep> {
        let c = create_cone(k, Point3::new(20.0, 4.0, 20.0), 2.0, 1.0, 1.0)?;
        boolean_union(k, block, c)
    }
    // its footprint swept down to the round, as the cone boss's
    let v = assert_pinned(lean_19, 8.0, 23486.591790262424, 23486.59175062265, 11);
    assert_near_closed_form(
        v,
        rounded_r8() + frustum(2.0, 1.0, 2.9) + column_under_footprint(),
    );
    for steep in [lean_30 as Feature, lean_45] {
        assert!(rounded_with(steep, 8.0).is_err());
    }
}

/// A round on the rim of a round block (#16636): its one sheet face is a ring whose box covers the
/// whole top, so every boss on the top lay inside that box. A boss with leaning walls is refused
/// only where the round really reaches it, not wherever the boxes meet. A 20 x 10 cylinder, its
/// top rim rounded r 1.5, a boss 4 tall on the top about (8, 0), 12 in from the band: the round is
/// the closed form's, the boss stays whole.
fn rounded_rim_with_boss(boss: fn(&mut Kernel) -> Result<Brep>) -> (f64, f64) {
    let mut k = Kernel {
        fixes: UpstreamFixes::all(),
        ..Kernel::default()
    };
    let block = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 20.0, 10.0).unwrap();
    let feature = boss(&mut k).unwrap();
    let mut b = boolean_union(&mut k, block, feature).unwrap();
    let e = pick(&k, &b, Point3::new(0.0, 20.0, 10.0));
    fillet_edges(&mut k, &mut b, &[e], 1.5, 1, 1, 1.0).unwrap();
    assert!(solidcore::topology::brep::p2::is_manifold_solid(&b).unwrap());
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(
        (
            b.face_list().len(),
            b.edge_list().len(),
            b.vertex_list().len()
        ),
        (6, 8, 5)
    );
    // the spandrel's area, revolved about its centroid (Pappus)
    let r = 1.5;
    let area = (1.0 - PI / 4.0) * r * r;
    let centroid = 20.0 - r * (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI));
    (volume_of(&k, &mut b), 2.0 * PI * centroid * area)
}

#[test]
fn a_leaning_boss_clear_of_a_rim_round_is_left_alone() {
    let (v, taken) =
        rounded_rim_with_boss(|k| create_cone(k, Point3::new(8.0, 0.0, 10.0), 3.0, 1.5, 4.0));
    let frustum = PI * 4.0 / 3.0 * (9.0 + 4.5 + 2.25);
    let want = PI * 400.0 * 10.0 + frustum - taken;
    println!("volume {v:.12}, want {want:.12}");
    assert!((v - want).abs() <= 1e-6 * want, "volume {v}, want {want}");
}

/// The same round past an upright boss: the control, the ordinary path.
#[test]
fn an_upright_boss_clear_of_a_rim_round_is_left_alone() {
    let (v, taken) =
        rounded_rim_with_boss(|k| create_cylinder(k, Point3::new(8.0, 0.0, 10.0), 3.0, 4.0));
    let want = PI * 400.0 * 10.0 + PI * 9.0 * 4.0 - taken;
    println!("volume {v:.12}, want {want:.12}");
    assert!((v - want).abs() <= 1e-6 * want, "volume {v}, want {want}");
}

/// The same round over a leaning boss standing partly on the band (the rim round takes the ring
/// r 18.5 to 20 off the top; the boss's base is r 2 about (17, 0), out to 19): refused, never an
/// open body with the boss's walls left unsewn.
#[test]
fn a_leaning_boss_on_a_rim_round_is_refused() {
    let mut k = Kernel {
        fixes: UpstreamFixes::all(),
        ..Kernel::default()
    };
    let block = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 20.0, 10.0).unwrap();
    let boss = create_cone(&mut k, Point3::new(17.0, 0.0, 10.0), 2.0, 1.0, 4.0).unwrap();
    let mut b = boolean_union(&mut k, block, boss).unwrap();
    let e = pick(&k, &b, Point3::new(0.0, 20.0, 10.0));
    let refusal = fillet_edges(&mut k, &mut b, &[e], 1.5, 1, 1, 1.0)
        .expect_err("a leaning boss over the band is refused");
    // the round reaches into the boss's footprint: refused as a round into material, not an
    // open body
    assert!(
        format!("{refusal:?}").contains("reason: Some(RoundIntoMaterial)"),
        "refused for another cause: {refusal:?}"
    );
}

/// A boss astride the round's contact line: refused, as the fork.
#[test]
fn a_boss_astride_the_contact_line_is_refused_as_the_fork() {
    fn astride(k: &mut Kernel, block: Brep) -> Result<Brep> {
        let c = create_box(k, Point3::new(18.0, 6.0, 20.0), 4.0, 4.0, 5.0)?;
        boolean_union(k, block, c)
    }
    assert!(rounded_with(astride, 8.0).is_err());
}

/// A pocket astride the contact line stands, as the fork. Its figure was the fork's at
/// 5e8c9c1 (with Runge-Kutta trim pieces; now 1.05e-6 below it, see [`assert_pinned`]): this round is a cliff overflow, trimmed at its creases since step 6
/// (WR-695), which runs none of keep features (23371.292359325398 at eb883ad).
#[test]
fn a_pocket_astride_the_contact_line_matches_the_fork() {
    fn astride(k: &mut Kernel, block: Brep) -> Result<Brep> {
        let c = create_box(k, Point3::new(18.0, 6.0, 15.0), 4.0, 4.0, 6.0)?;
        boolean_difference(k, block, c)
    }
    assert_pinned(astride, 8.0, 23371.292358869621, 23371.292358869621, 12);
}

// ---- Work ----

/// A pocket and a boss in one round's band, each as `prisms_match_the_fork` keeps it alone:
/// together they keep the same volumes. Keep features reads the walls of each from one copy of
/// the body they lie in (the block for the pocket's 4 walls, the sheet of rounds for the boss's
/// 8 continued walls), not one copy per wall.
#[test]
fn walls_are_read_from_one_copy_per_body() {
    fn pocket_and_boss(k: &mut Kernel, block: Brep) -> Result<Brep> {
        let pocket = create_box(k, Point3::new(8.0, 2.0, 15.0), 4.0, 4.0, 6.0)?;
        let block = boolean_difference(k, block, pocket)?;
        let boss = create_box(k, Point3::new(28.0, 2.0, 20.0), 4.0, 4.0, 5.0)?;
        boolean_union(k, block, boss)
    }
    let mut k = fork_kernel();
    let block = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
    let mut b = pocket_and_boss(&mut k, block).unwrap();
    let e = pick(&k, &b, Point3::new(20.0, 0.0, 20.0));
    k.work_counters.reset();
    fillet_edges(&mut k, &mut b, &[e], 8.0, 1, 1, 1.0).unwrap();
    let copies = k.work_counters.wall_body_copies();
    assert!(solidcore::topology::brep::p2::is_manifold_solid(&b).unwrap());
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
    let v = volume_of(&k, &mut b);
    let want = 23549.896259768986 + 23389.896259952176 - TAKEN;
    println!(
        "volume {v:.17}, want {want:.17}; {} faces; {copies} copies",
        b.face_list().len()
    );
    assert!((v - want).abs() <= 1e-9 * want, "volume {v}, want {want}");
    assert_eq!(b.face_list().len(), 17);
    assert_eq!(copies, 2);
}

/// The same pocket and boss: keep features classifies the crease pieces on each wall on one
/// snapshot of the body copy the wall lies in, not on a new copy of the whole body per piece
/// (34 snapshots when each piece made its own).
#[test]
fn crease_pieces_are_classified_on_one_snapshot_per_copy() {
    fn pocket_and_boss(k: &mut Kernel, block: Brep) -> Result<Brep> {
        let pocket = create_box(k, Point3::new(8.0, 2.0, 15.0), 4.0, 4.0, 6.0)?;
        let block = boolean_difference(k, block, pocket)?;
        let boss = create_box(k, Point3::new(28.0, 2.0, 20.0), 4.0, 4.0, 5.0)?;
        boolean_union(k, block, boss)
    }
    let mut k = fork_kernel();
    let block = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
    let mut b = pocket_and_boss(&mut k, block).unwrap();
    let e = pick(&k, &b, Point3::new(20.0, 0.0, 20.0));
    k.work_counters.reset();
    fillet_edges(&mut k, &mut b, &[e], 8.0, 1, 1, 1.0).unwrap();
    assert_eq!(k.work_counters.classification_snapshots(), 24);
}

/// One fillet r 1 on two chains that share no face (#22144): the block's top front edge, clear
/// of a boss r 2 at (20, 15) on the top, and the bottom rim of a blind hole r 1.5 under that
/// boss. The boss's walls continued down from the top pass through the
/// hole's round (it spans r 1.5 to 2.5 about the hole's axis), which rolls on no face the boss
/// stands on: the boss is in neither band, and the one fillet is the two fillets one after the
/// other.
fn boss_over_a_rounded_hole(k: &mut Kernel) -> Brep {
    let block = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
    let boss = create_cylinder(k, Point3::new(20.0, 15.0, 20.0), 2.0, 5.0).unwrap();
    let block = boolean_union(k, block, boss).unwrap();
    let hole = create_cylinder(k, Point3::new(20.0, 15.0, -1.0), 1.5, 7.0).unwrap();
    boolean_difference(k, block, hole).unwrap()
}

/// The faces, edges and vertices of a sound solid, its rounds (faces on a procedural blend)
/// and its volume.
fn sound_counts(k: &mut Kernel, b: &mut Brep) -> ((usize, usize, usize, usize), f64) {
    assert!(solidcore::topology::brep::p2::is_manifold_solid(b).unwrap());
    let lines = check_brep(k, b, CheckLevel::Level2).unwrap().lines();
    assert!(lines.is_empty(), "{lines:?}");
    let rounds = b
        .face_list()
        .iter()
        .filter(|&&f| {
            b.face(f)
                .unwrap()
                .surface
                .as_ref()
                .is_some_and(|s| s.procedural_blend().is_some())
        })
        .count();
    let counts = (
        b.face_list().len(),
        b.edge_list().len(),
        b.vertex_list().len(),
        rounds,
    );
    (counts, volume_of(k, b))
}

#[test]
fn a_boss_beside_one_chain_is_not_reached_by_another_chain_s_round() {
    let (front, rim) = (Point3::new(20.0, 0.0, 20.0), Point3::new(20.0, 16.5, 0.0));
    // the two rounds one after the other
    let mut k = Kernel::default();
    let mut apart = boss_over_a_rounded_hole(&mut k);
    let e = pick(&k, &apart, front);
    fillet_edges(&mut k, &mut apart, &[e], 1.0, 1, 1, 1.0).unwrap();
    let e = pick(&k, &apart, rim);
    fillet_edges(&mut k, &mut apart, &[e], 1.0, 1, 1, 1.0).unwrap();
    let (want_counts, want) = sound_counts(&mut k, &mut apart);
    // the closed forms: the front round's spandrel along its 40, and the rim round's revolved
    // about the hole's axis at its centroid (Pappus)
    let (a, r) = (1.5, 1.0);
    let centroid = a + r * (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI));
    let closed =
        24000.0 + 20.0 * PI - 13.5 * PI - 40.0 * spandrel(r) - 2.0 * PI * centroid * spandrel(r);
    assert!(
        (want - closed).abs() <= 1e-9 * closed,
        "one after the other {want:.12}, closed form {closed:.12}"
    );
    // the two rounds in one fillet
    let mut k = Kernel::default();
    let mut b = boss_over_a_rounded_hole(&mut k);
    let edges = [pick(&k, &b, front), pick(&k, &b, rim)];
    fillet_edges(&mut k, &mut b, &edges, r, 1, 1, 1.0)
        .expect("two chains clear of each other and of the boss");
    let (counts, v) = sound_counts(&mut k, &mut b);
    println!("one fillet {counts:?} {v:.12}; one after the other {want_counts:?} {want:.12}");
    assert_eq!(counts, want_counts);
    assert!((v - want).abs() <= 1e-9 * want, "volume {v}, want {want}");
}
