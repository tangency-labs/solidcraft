//! The SolidCraft fork's tangency-labs/smlib@eb883ad (WR-703, #2273): a boss or a hole inside a
//! round's band stays, the round trimmed round it in place; the cases are the fork's ddbb782.
//!
//! The 40 x 30 x 20 block's top front edge (y = 0, z = 20) is rounded; the feature, a cylinder of
//! radius 2 about the upright x = 20, y = 4, stands 2 to 6 in from the front on the top. A round
//! of radius r takes the band y < r off the top and z > 20 - r off the front, so at r 8 the
//! footprint lies inside the band, at r 1.9 the band stops short of it.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::{KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, PI};
use solidcore::primitive::api_primitives::{create_box, create_cone, create_cylinder};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel, Result};

fn kernel(keep: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_keep_features = keep;
    k
}

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
fn assert_feature_in_band(
    boss: bool,
    keep: bool,
    r: f64,
    want: f64,
    rel: f64,
    faces: usize,
    reach: bool,
) {
    let mut k = kernel(keep);
    let mut b = featured_block(&mut k, boss);
    let e = pick(&k, &b, Point3::new(20.0, 0.0, 20.0));
    fillet_edges(&mut k, &mut b, &[e], r, 1, 1, 1.0).unwrap();
    let v = volume_of(&k, &mut b);
    let turned = wall_on_round(&k, &b);
    println!(
        "boss {boss} keep {keep} r {r}: volume {v:.9}, want {want:.9}; {} faces, want {faces}; wall on the round {turned:.9}",
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
    assert_feature_in_band(true, true, 8.0, want, 1e-7, 9, true);
}

#[test]
fn the_hole_in_the_band_is_kept() {
    let want = featured_rounded(false, 8.0) + column_under_footprint();
    assert_feature_in_band(false, true, 8.0, want, 1e-7, 9, true);
}

#[test]
fn with_keep_off_the_round_takes_the_boss() {
    assert_feature_in_band(
        true,
        false,
        8.0,
        24000.0 - 40.0 * spandrel(8.0),
        1e-9,
        7,
        false,
    );
}

#[test]
fn with_keep_off_the_round_takes_the_hole() {
    assert_feature_in_band(
        false,
        false,
        8.0,
        24000.0 - 40.0 * spandrel(8.0),
        1e-9,
        7,
        false,
    );
}

#[test]
fn a_band_short_of_the_feature_leaves_it_either_way() {
    for keep in [false, true] {
        for boss in [true, false] {
            assert_feature_in_band(boss, keep, 1.9, featured_rounded(boss, 1.9), 1e-9, 9, false);
        }
    }
}

// ---- With the fork's opt-ins at eb883ad (the figures: `assert_pinned`) ----

/// The opt-ins these figures rest on, found by removing every flag the figures don't need, so a
/// new flag on `main` can't move them. Not `UpstreamFixes::all()`: it holds fixes the fork doesn't
/// have.
fn fork_kernel(keep: bool) -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    let mut fixes = UpstreamFixes::default();
    fixes.fillet_refuse_rails_leaving_faces = true;
    fixes.fillet_cliff_overflow = true;
    fixes.fillet_overflow_trim_at_creases = true;
    fixes.linear_sweep_arc_is_cylinder = true;
    fixes.precise_properties_body_length_scale = true;
    fixes.fillet_keep_features = keep;
    k.fixes = fixes;
    k
}

type Feature = fn(&mut Kernel, Brep) -> Result<Brep>;

/// The block with a feature, its top front edge rounded `r`: the volume and the faces.
fn rounded_with(keep: bool, feature: Feature, r: f64) -> Result<(f64, usize)> {
    let mut k = fork_kernel(keep);
    let block = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0)?;
    let mut b = feature(&mut k, block)?;
    let e = pick(&k, &b, Point3::new(5.0, 0.0, 20.0));
    fillet_edges(&mut k, &mut b, &[e], r, 1, 1, 1.0)?;
    assert!(solidcore::topology::brep::p2::is_manifold_solid(&b).unwrap());
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
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
/// theirs, the crease's fit, as the fork's do; the cone pit has none.
fn assert_pinned(keep: bool, feature: Feature, r: f64, volume: f64, faces: usize) {
    let (v, n) = rounded_with(keep, feature, r).unwrap();
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
}

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
    assert_pinned(true, cylinder_boss, 8.0, 23528.165527753492, 9);
    assert_pinned(true, cylinder_hole, 8.0, 23402.501835298488, 9);
    assert_pinned(false, cylinder_boss, 8.0, TAKEN, 7);
    assert_pinned(false, cylinder_hole, 8.0, TAKEN, 7);
    for keep in [true, false] {
        assert_pinned(keep, cylinder_boss, 1.9, 24031.84334806833, 9);
        assert_pinned(keep, cylinder_hole, 1.9, 23906.179641369188, 9);
    }
}

/// Planar walls: each crease is an exact section.
#[test]
fn prisms_keep_their_pinned_volumes() {
    assert_pinned(true, prism_boss, 8.0, 23549.896259768986, 12);
    assert_pinned(true, prism_pocket, 8.0, 23389.896259952173, 12);
    assert_pinned(false, prism_boss, 8.0, TAKEN, 7);
    assert_pinned(false, prism_pocket, 8.0, TAKEN, 7);
}

/// Walls that lean: kept as the fork keeps them, the boss's with no refusal.
#[test]
fn cones_keep_their_pinned_volumes() {
    assert_pinned(true, cone_boss, 8.0, 23501.985588856085, 11);
    assert_pinned(true, cone_pit, 8.0, 23427.54088939281, 9);
    assert_pinned(false, cone_boss, 8.0, TAKEN, 7);
    assert_pinned(false, cone_pit, 8.0, TAKEN, 7);
}

/// A boss's walls count as upright within the fillet's angular tolerance: at 19 degrees off they
/// are carried down, at 30 and 45 the round is refused with the flag on and takes the boss with it
/// off, as the fork.
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
    assert_pinned(true, lean_19, 8.0, 23486.59179069601, 11);
    assert_pinned(false, lean_19, 8.0, TAKEN, 7);
    for steep in [lean_30 as Feature, lean_45] {
        assert!(rounded_with(true, steep, 8.0).is_err());
        assert_pinned(false, steep, 8.0, TAKEN, 7);
    }
}

/// A boss astride the round's contact line: refused with the flag on or off, as the fork.
#[test]
fn a_boss_astride_the_contact_line_is_refused_as_the_fork() {
    fn astride(k: &mut Kernel, block: Brep) -> Result<Brep> {
        let c = create_box(k, Point3::new(18.0, 6.0, 20.0), 4.0, 4.0, 5.0)?;
        boolean_union(k, block, c)
    }
    for keep in [true, false] {
        assert!(rounded_with(keep, astride, 8.0).is_err(), "keep {keep}");
    }
}

/// A pocket astride the contact line stands with the flag on or off, as the fork. This figure is
/// the fork's at 5e8c9c1: this round is a cliff overflow, trimmed at its creases since step 6
/// (WR-695), which runs none of keep features (23371.292359325398 at eb883ad).
#[test]
fn a_pocket_astride_the_contact_line_matches_the_fork() {
    fn astride(k: &mut Kernel, block: Brep) -> Result<Brep> {
        let c = create_box(k, Point3::new(18.0, 6.0, 15.0), 4.0, 4.0, 6.0)?;
        boolean_difference(k, block, c)
    }
    for keep in [true, false] {
        assert_pinned(keep, astride, 8.0, 23371.292359915951, 12);
    }
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
    let mut k = fork_kernel(true);
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
