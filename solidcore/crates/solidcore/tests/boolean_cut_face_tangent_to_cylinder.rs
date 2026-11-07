//! tangency-labs/solidcore#3091 (from tangency-labs/solidcraft#12858): a cut whose flat face is
//! exactly tangent to a cylindrical boss comes back with no faces, or fails with `SM_ERR`.
//!
//! The part is a 43.5 x 36 x 10 plate with a boss on top: a cylinder of radius
//! `R = sqrt(12.68)` (3.5609) at (27.2, 16), z 10..20. The tool is a D-shaped prism swept along
//! +y through the whole part: its profile, in the XZ plane, is the chord `x = 27.2 - R` from
//! z = 12.5435 to z = 17.6134 closed by an arc of radius 2.56398 about (23.2544, 15.0785) that
//! bulges towards +x, into the boss. The chord's face, the plane `x = 27.2 - R`, touches the boss
//! cylinder along the line `y = 16` without crossing it; the arc face cuts a notch out of the
//! boss.
//!
//! In SolidCraft 0.40.0-rc.2 (solidcore 8ddce9016) the difference returns a body with no faces
//! and the application deletes the part. The chord moved 0.001 mm either way (crossing the
//! cylinder, or clear of it) cuts normally.
//!
//! The expected volume is the plate plus the boss less the notch: the notch is the integral over
//! the D of the boss's chord along y, `2 sqrt(R^2 - (x - 27.2)^2)`, which lies wholly in the boss
//! (the D's z range is inside 10..20 and its x range starts at the silhouette). With
//! `x = v0 + r cos t` it is `int_0^a 4 r^2 sin^2 t sqrt(R^2 - (x - 27.2)^2) dt`,
//! `a = acos((xch - v0) / r)`: 36.5850571 (Simpson, 4e5 panels), so `V = 16058.3539 - 36.5851 =
//! 16021.7689`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use std::f64::consts::PI;

use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p1::faces;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::proto_topology::p1::api_transform::{
    api_rotate, api_translate, TransformObject,
};
use solidcore::{Brep, Kernel};

/// The boss: centre and radius (`R^2 = 12.68`).
const BOSS_X: f64 = 27.2;
const BOSS_Y: f64 = 16.0;
const BOSS_R: f64 = 3.560_898_762_952_973_6;
/// The D's arc: centre (x, z) and radius, from the design's sketch.
const ARC_X: f64 = 23.254_420_118_496_817;
const ARC_Z: f64 = 15.078_456_991_201_652;
const ARC_R: f64 = 2.563_975_512_604_723_5;

/// Volume of the plate and the boss.
fn part_volume() -> f64 {
    43.5 * 36.0 * 10.0 + PI * BOSS_R * BOSS_R * 10.0
}

/// Volume of the notch the D cuts out of the boss when its chord is at `x = xch`
/// (the module doc's integral, Simpson with 4e5 panels in t).
fn notch_volume(xch: f64) -> f64 {
    let a = ((xch - ARC_X) / ARC_R).acos();
    let n = 400_000;
    let mut s = 0.0;
    for i in 0..=n {
        let t = a * f64::from(i) / f64::from(n);
        let x = ARC_X + ARC_R * t.cos();
        let g = (BOSS_R * BOSS_R - (x - BOSS_X).powi(2)).max(0.0);
        let w = if i == 0 || i == n {
            1.0
        } else if i % 2 == 1 {
            4.0
        } else {
            2.0
        };
        s += w * 4.0 * ARC_R * ARC_R * t.sin() * t.sin() * g.sqrt();
    }
    s * a / f64::from(n) / 3.0
}

/// The plate with its boss.
fn part(k: &mut Kernel) -> Brep {
    let plate = create_box(k, Point3::new(0.0, 0.0, 0.0), 43.5, 36.0, 10.0).unwrap();
    let boss = create_cylinder(k, Point3::new(BOSS_X, BOSS_Y, 10.0), BOSS_R, 10.0).unwrap();
    boolean_union(k, plate, boss).unwrap()
}

/// The D prism: chord at `x = xch`, swept along +y from `y0` by `length`.
fn tool(k: &mut Kernel, xch: f64, y0: f64, length: f64) -> Brep {
    // The profile in the XY plane with u = x and v = z, then turned 90 degrees about X
    // (u stays x, v becomes z) and moved to y = y0.
    let h = (ARC_R * ARC_R - (xch - ARC_X).powi(2)).sqrt();
    let lo = Vec3::new(xch, ARC_Z - h, 0.0);
    let hi = Vec3::new(xch, ARC_Z + h, 0.0);
    let start = (ARC_Z - h - ARC_Z).atan2(xch - ARC_X).to_degrees();
    let end = (ARC_Z + h - ARC_Z).atan2(xch - ARC_X).to_degrees();
    let chord = Curve::from(create_line_segment(lo, hi).unwrap());
    let arc = Curve::from(
        create_arc(Vec3::new(ARC_X, ARC_Z, 0.0), ARC_R, start, end).unwrap(),
    );
    let mut curves = [chord, arc];
    for c in &mut curves {
        api_rotate(
            k,
            Some(TransformObject::Curve(c)),
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::X,
            90.0,
        )
        .unwrap();
        api_translate(k, Some(TransformObject::Curve(c)), Vec3::new(0.0, y0, 0.0)).unwrap();
    }
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_linear_sweep(k, &refs, Vec3::new(0.0, 1.0, 0.0), length, true).unwrap()
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::new();
    let key_of = |_| ObjectKey(0);
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    brep_volume(k, b, 1e-7, &attrs).unwrap()
}

/// SolidCraft's fix set (`tests/kernel_cases/fixes.json`), on `k`.
fn set_solidcraft_fixes(k: &mut Kernel) {
    let names: Vec<String> = serde_json::from_str(include_str!("kernel_cases/fixes.json")).unwrap();
    for name in &names {
        *k.fixes.flag_mut(name).unwrap_or_else(|| panic!("unknown fix {name}")) = true;
    }
}

/// Cuts the D prism with its chord at `xch` out of the part and checks the result: a closed
/// manifold solid of the expected volume.
fn cut(xch: f64, y0: f64, length: f64, solidcraft_fixes: bool) -> Result<(), String> {
    let mut k = Kernel::default();
    if solidcraft_fixes {
        set_solidcraft_fixes(&mut k);
    }
    let part = part(&mut k);
    let tool = tool(&mut k, xch, y0, length);
    let mut result = boolean_difference(&mut k, part, tool).map_err(|e| format!("difference: {e}"))?;
    result.refresh_cached_topology();
    let n = faces(&result, None).map_err(|e| e.to_string())?.len();
    if n == 0 {
        return Err("the result has no faces".into());
    }
    if !is_manifold_solid(&result).map_err(|e| e.to_string())? {
        return Err(format!("the result ({n} faces) is not a closed manifold solid"));
    }
    let want = part_volume() - notch_volume(xch);
    let v = volume(&k, &mut result);
    // 0.02 mm^3 on 16022: the integrator's 1e-7 relative accuracy is 1.6e-3.
    if (v - want).abs() > 0.02 {
        return Err(format!("volume {v} vs {want} ({n} faces)"));
    }
    Ok(())
}

/// The chord exactly on the boss's silhouette, `x = 27.2 - R`.
fn tangent_chord() -> f64 {
    BOSS_X - BOSS_R
}

#[test]
fn chord_tangent_to_the_boss_cuts_the_notch() {
    let runs = [
        // SolidCraft's through-all tool starts on the sketch plane, the plate's front face,
        // and runs well past the part (its reach: about 88 mm here).
        ("from the front face, SolidCraft's fixes", 0.0, 88.0, true),
        ("through both faces, SolidCraft's fixes", -2.0, 40.0, true),
        ("from the front face, no fixes", 0.0, 88.0, false),
        ("through both faces, no fixes", -2.0, 40.0, false),
    ];
    let failures: Vec<String> = runs
        .iter()
        .filter_map(|(what, y0, len, fixes)| {
            cut(tangent_chord(), *y0, *len, *fixes)
                .err()
                .map(|e| format!("{what}: {e}"))
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn chord_a_hair_into_or_clear_of_the_boss_cuts_the_notch() {
    let failures: Vec<String> = [-0.001, 0.001]
        .iter()
        .flat_map(|d| [(d, true), (d, false)])
        .filter_map(|(d, fixes)| {
            cut(tangent_chord() + d, -2.0, 40.0, fixes)
                .err()
                .map(|e| format!("chord moved {d} (fixes {fixes}): {e}"))
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
