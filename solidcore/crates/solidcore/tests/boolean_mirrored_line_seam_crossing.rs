//! tangency-labs/solidcraft#13004: SolidCraft joins a mirrored arm to a torso with slanted
//! sides, and the union fails (`boolean_union_refuses_solid_leak`) or, without that fix,
//! returns no solid; joined to the arm as made it succeeds.
//!
//! The bodies are SolidCraft's (binary `.smb`, written by SolidCraft from the issue's
//! script): the torso, a trapezoid extruded 8 mm with its sides slanted 13 degrees; the pin, a
//! cylinder of radius 2 along x from (5, 0, 26) to (7, 0, 26) through the torso's slanted
//! side; the arm, a box joined to that pin. Their straight edges are infinite lines (natural
//! interval [-1234567, 1234567]), as SolidCraft and STEP make them.
//!
//! Mirroring turns each line into a degree-1 B-spline over the line's NURB, whose two
//! control points lie about 1.2e6 mm out. A point of it near the part is a convex combination
//! of those far points, good to about 1e-16 * 1.2e6 mm. The pin's seam crosses the torso's
//! slanted side; the curve/curve solve found that crossing with a gap of 4.4e-11 and the
//! solution filter dropped it (its exact zero was scaled by the solution point, 2.8e-11), so
//! the intersection of the pin with the side was not split at the seam and the faces were
//! misclassified.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::boolean::api_brep::boolean_union;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{
    brep_properties, MassPropertyBehavior, PropertyAttributes,
};
use solidcore::query::brep::{calculate_bounding_box, BoundingBoxOptions};
use solidcore::topology::api_files::read_brep_from_file;
use solidcore::topology::brep::mirror::transform_matrix;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, Kernel};

/// The mirror in the plane x = 0 (row-vector convention: the fourth row is the translation).
const MIRROR_X: [[f64; 4]; 4] = [
    [-1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

const TORSO: &[u8] = include_bytes!("data/solidcraft_13004_torso.smb");
const PIN: &[u8] = include_bytes!("data/solidcraft_13004_pin.smb");
const ARM: &[u8] = include_bytes!("data/solidcraft_13004_arm.smb");

fn read(k: &mut Kernel, bytes: &[u8]) -> Brep {
    let fresh = Brep::new();
    let mut store = AttributeStore::new();
    let mut records = NotifyRecords::new();
    let key_of = |e| fresh.owner_identity(e);
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut store,
        key_of: &key_of,
        records: &mut records,
        next_key: 1 << 50,
    };
    let mut b = read_brep_from_file(k, Some(bytes.to_vec()), false, false, &mut env).unwrap();
    b.refresh_cached_topology();
    b
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let bbox = calculate_bounding_box(k, &mut *b, BoundingBoxOptions::default()).unwrap();
    let store = AttributeStore::default();
    let key_of = |_| ObjectKey(0);
    brep_properties(
        k,
        b,
        1e-9,
        bbox.mid(),
        MassPropertyBehavior::default(),
        &PropertyAttributes {
            store: &store,
            key_of: &key_of,
        },
    )
    .unwrap()
    .volume
}

/// The union of the torso with `tool` mirrored `mirrors` times in x = 0, run with `fixes`:
/// its volume when it is a closed solid, else why not. The torso is symmetric about x = 0,
/// so the union's volume does not depend on `mirrors`.
fn union_volume(tool: &[u8], mirrors: usize, fixes: UpstreamFixes) -> Result<f64, String> {
    let mut k = Kernel::default();
    k.fixes = fixes;
    let torso = read(&mut k, TORSO);
    let mut t = read(&mut k, tool);
    for _ in 0..mirrors {
        transform_matrix(&mut k, &mut t, &MIRROR_X, true).unwrap();
    }
    let mut u = boolean_union(&mut k, torso, t).map_err(|e| e.to_string())?;
    if !is_manifold_solid(&u).unwrap() {
        return Err("not a solid".into());
    }
    Ok(volume(&k, &mut u))
}

fn assert_same(what: &str, got: Result<f64, String>, want: f64) {
    match got {
        Ok(v) => assert!(
            (v - want).abs() <= 1e-9 * want,
            "{what}: {v}, expected {want}"
        ),
        Err(e) => panic!("{what}: {e}, expected a solid of {want}"),
    }
}

/// Torso and pin: 1352 + pi 2^2 2 less the pin's part inside the torso. Mirrored twice
/// (the same pin, its edges now B-splines) the union keeps that volume; it was 1348.66.
#[test]
fn torso_and_twice_mirrored_pin() {
    let made = union_volume(PIN, 0, UpstreamFixes::default()).unwrap();
    assert!((made - 1365.533).abs() < 1e-3, "{made}");
    assert_same(
        "default fixes",
        union_volume(PIN, 2, UpstreamFixes::default()),
        made,
    );
    assert_same(
        "all fixes",
        union_volume(PIN, 2, UpstreamFixes::all()),
        made,
    );
}

/// Torso and arm, as in the issue: the arm mirrored to the other side of the torso joins with
/// the volume of the arm as made, 1516.80 (by symmetry). With every fix on it was refused
/// (`boolean_union_refuses_solid_leak`, status 1001); with none it gave no solid.
#[test]
fn torso_and_mirrored_arm() {
    let made = union_volume(ARM, 0, UpstreamFixes::all()).unwrap();
    assert!((made - 1516.798).abs() < 1e-3, "{made}");
    assert_same(
        "all fixes",
        union_volume(ARM, 1, UpstreamFixes::all()),
        made,
    );
    assert_same(
        "default fixes",
        union_volume(ARM, 1, UpstreamFixes::default()),
        made,
    );
    assert_same(
        "twice, all fixes",
        union_volume(ARM, 2, UpstreamFixes::all()),
        made,
    );
}
