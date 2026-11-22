//! WR-605 on the upstream fixtures: the ring fillets of the `blend.fillet_removal`
//! goldens rows `defillet_face8` and `nurb_fillet_face18` are removed.
//!
//! - `defillet.smb` is the cube `[0, 2]^3` with a cylinder boss of radius `R = 1/2` on its
//!   top face (axis through `(1, 1)`, `z` from 2 to 3) and a concave ring fillet of radius
//!   `r = 1/5` round the boss's root (face 8). Removing the fillet leaves the cube and the
//!   boss, `8 + pi/4` (closed form). The input adds the fillet's ring, whose section (the
//!   square of side `r` less its quarter disc, `A = r^2 (1 - pi/4)`, centroid
//!   `d = r (10 - 3 pi) / (12 - 3 pi)` from the wall) turns once round the axis:
//!   `2 pi (R + d) A` by Pappus. The premises (the fixture's dimensions, a round section)
//!   are read from the fixture, not asserted.
//! - `nurb_fillet.smb` face 18 is a B-spline fillet of radius `1/5` between a B-spline
//!   extrusion's wall and the plane `z = 0` round a closed profile, with no closed form.
//!   The fixture's own faces leave gaps that hold its volume enclosure about `0.43` wide
//!   (the enclosure's floor), wider than the `0.17` the removal takes, so the enclosures
//!   before and after cannot be separated; the result's enclosure is asserted to sit
//!   strictly lower than the input's, to hold the result's volume estimate, and to be no
//!   wider at its floor than the input's (the removal opens no gap of its own).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use solidcore::blend::fillet_removal::delete_fillet_face;
use solidcore::geom::attribute::AttributeStore;
use solidcore::mass::enclosure::{enclose_volume, EnclosureStop, VolumeEnclosure};
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, Kernel};

/// The upstream fixture `file`. A missing file fails the test: these rows are the
/// fixtures' own, and a skip would leave them unchecked.
fn fixture_path(file: &str) -> std::path::PathBuf {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles")
        .join(file);
    assert!(
        path.is_file(),
        "the upstream test file is missing: {}",
        path.display()
    );
    path
}

fn fixture(k: &mut Kernel, path: &std::path::Path) -> Brep {
    let bytes = std::fs::read(path).unwrap();
    let mut b = Brep::new_with_infinite_region().unwrap();
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let key = solidcore::topology::loopuse::notify_ref;
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key,
        records: &mut records,
        next_key: 1 << 50,
    };
    read_from_file(k, &mut b, bytes, FileType::Ascii, false, &mut env).unwrap();
    b.use_arena_naming().unwrap();
    b
}

/// The relative width asked of each volume enclosure.
const REL_WIDTH: f64 = 1e-8;

/// A body's volume estimate (`brep_volume`) and its certified enclosure, asked `REL_WIDTH`
/// of the estimate wide.
struct Volume {
    estimate: f64,
    enclosure: VolumeEnclosure,
}

fn volume(k: &Kernel, b: &mut Brep, what: &str) -> Volume {
    let estimate = offset_fixture::volume(k, b).unwrap();
    let enclosure = enclose_volume(k, b, REL_WIDTH * estimate.abs()).unwrap();
    eprintln!("{what}: estimate {estimate}, enclosure {enclosure:?}");
    Volume {
        estimate,
        enclosure,
    }
}

/// The removal of a face: whether it succeeded, the face counts before and after,
/// whether the body is one closed manifold solid, its volumes before and after and its
/// Level 2 reports after.
struct Removal {
    ok: bool,
    faces: (usize, usize),
    manifold: bool,
    volumes: (Volume, Volume),
    level2: Vec<String>,
}

/// Removes face `index` of the fixture at `path`. The volume before is taken on a copy
/// read separately, since measuring it makes missing UV trim curves: the removal runs on
/// the body as read, as the goldens row runs it.
fn remove(path: &std::path::Path, index: usize) -> Removal {
    let what = format!("{} {index}", path.display());
    let mut k = Kernel::default();
    let mut input = fixture(&mut k, path);
    let before = volume(&k, &mut input, &format!("{what} before"));
    let mut k = Kernel::default();
    let mut b = fixture(&mut k, path);
    let f = bp1::faces(&b, None).unwrap()[index];
    let faces = b.face_list().len();
    b.set_editing_enabled(true);
    let r = delete_fillet_face(&mut k, &mut b, f, None);
    b.set_editing_enabled(false);
    eprintln!("{what}: {r:?}");
    let after = volume(&k, &mut b, &format!("{what} after"));
    let level2 = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    Removal {
        ok: r.is_ok(),
        faces: (faces, b.face_list().len()),
        manifold: brep_is_manifold_solid(&b).unwrap_or(false),
        volumes: (before, after),
        level2,
    }
}

/// Asserts what every ring removal gives: success, one face fewer, and one closed
/// manifold solid with no Level 2 report.
fn assert_valid_removal(what: &str, r: &Removal) {
    assert!(r.ok, "{what}: refused");
    assert_eq!(r.faces.1 + 1, r.faces.0, "{what}: faces");
    assert!(r.manifold, "{what}: not one closed manifold solid");
    assert!(r.level2.is_empty(), "{what}: Level 2 {:?}", r.level2);
}

/// Whether `e` holds `exact`, a closed form evaluated in f64 with its rounding charged
/// (`16 eps |V|`, as the p3 certificates charge it).
fn holds(e: &VolumeEnclosure, exact: f64) -> bool {
    let slack = 16.0 * f64::EPSILON * exact.abs();
    e.lo - slack <= exact && exact <= e.hi + slack
}

/// `defillet.smb` face 8, the ring fillet round the boss's root, is removed: the result
/// is one closed manifold solid with no Level 2 report whose certified volume enclosure
/// (at most `1e-8` of it wide) holds the cube and boss's `8 + pi/4`. The input's
/// enclosure holds that plus the ring's `2 pi (R + d) A`, and the two enclosures are
/// disjoint.
#[test]
fn defillet_ring_fillet_removal_leaves_cube_and_boss() {
    use core::f64::consts::{FRAC_PI_4, PI};
    let what = "defillet.smb 8";
    let r = remove(&fixture_path("Defeature/defillet.smb"), 8);
    let (before, after) = (&r.volumes.0.enclosure, &r.volumes.1.enclosure);
    for (e, when) in [(before, "before"), (after, "after")] {
        assert_eq!(e.stop, EnclosureStop::Reached, "{what} {when}: {e:?}");
    }
    let removed = {
        let (big_r, small_r) = (0.5, 0.2);
        let a = small_r * small_r * (1.0 - FRAC_PI_4);
        let d = small_r * (10.0 - 3.0 * PI) / (12.0 - 3.0 * PI);
        2.0 * PI * (big_r + d) * a
    };
    let cube_and_boss = 8.0 + FRAC_PI_4;
    let filleted = cube_and_boss + removed;
    assert!(
        holds(before, filleted),
        "{what}: the input's closed form {filleted} is outside [{}, {}]",
        before.lo,
        before.hi
    );
    assert!(
        holds(after, cube_and_boss),
        "{what}: the closed form 8 + pi/4 = {cube_and_boss} is outside [{}, {}]",
        after.lo,
        after.hi
    );
    assert!(
        after.hi < before.lo,
        "{what}: the volume after [{}, {}] is not below the volume before [{}, {}]",
        after.lo,
        after.hi,
        before.lo,
        before.hi
    );
    assert_valid_removal(what, &r);
}

/// `nurb_fillet.smb` face 18, a B-spline ring fillet, is removed: the result is one
/// closed manifold solid with no Level 2 report whose certified volume enclosure holds
/// its volume estimate, lies strictly lower at both ends than the input's, and is no
/// wider at its floor (the gaps no refinement narrows) than the input's; its volume
/// estimate is below the input's (MEASURED).
#[test]
fn nurb_fillet_ring_fillet_removal_takes_material_away() {
    let what = "nurb_fillet.smb 18";
    let r = remove(&fixture_path("Solids/nurb_fillet.smb"), 18);
    let (before, after) = (&r.volumes.0, &r.volumes.1);
    let (eb, ea) = (&before.enclosure, &after.enclosure);
    for v in [before, after] {
        let e = &v.enclosure;
        assert!(
            e.lo.is_finite() && e.hi.is_finite() && e.lo <= v.estimate && v.estimate <= e.hi,
            "{what}: the estimate {} is outside [{}, {}]",
            v.estimate,
            e.lo,
            e.hi
        );
    }
    assert!(
        ea.lo < eb.lo && ea.hi < eb.hi,
        "{what}: the volume after [{}, {}] does not lie lower than before [{}, {}]",
        ea.lo,
        ea.hi,
        eb.lo,
        eb.hi
    );
    // The estimates fall too (MEASURED: lower enclosure ends alone do not order the
    // values while the enclosures overlap, and neither does this).
    assert!(
        after.estimate < before.estimate,
        "{what}: the estimate after {} is not below the estimate before {}",
        after.estimate,
        before.estimate
    );
    assert!(
        ea.floor <= eb.floor,
        "{what}: the floor after {} is wider than before {}",
        ea.floor,
        eb.floor
    );
    assert_valid_removal(what, &r);
}
