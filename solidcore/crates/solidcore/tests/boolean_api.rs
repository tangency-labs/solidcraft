//! The public boolean API end to end: seven primitive booleans (through-hole, overlapping
//! union, sphere/box intersection, T-pipe, coplanar union, tangent union, torus minus box)
//! each succeed, give a closed manifold solid, and integrate to the analytic volume
//! (which the C++ SMLib reference also reproduces), with SMLib's face count. Unions of
//! two solids touching at one point, on a seam or off it, also check clean at Level2. A body
//! cut by an exact copy of itself leaves nothing, with STEP-sized tolerances too.
#![allow(clippy::unwrap_used)] // test helpers

use solidcore::boolean::api_brep::{boolean_difference, boolean_intersection, boolean_union};
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::p1::{BSplineCurveForm, CanonicalData, KnotType};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::primitive::api_primitives as api;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::Brep;
use solidcore::Kernel;
use std::f64::consts::PI;

/// Relative volume tolerance: the integrator runs at accuracy 1e-7.
const VOLUME_REL_TOL: f64 = 1e-6;

#[derive(Clone, Copy)]
enum Op {
    Union,
    Intersection,
    Difference,
}

struct Case {
    name: &'static str,
    /// Analytic volume of the result.
    volume: f64,
    /// Volume the C++ SMLib reference computes for the same boolean.
    smlib_volume: f64,
    /// Face count of the C++ SMLib reference result.
    smlib_faces: usize,
}

fn operands(k: &mut Kernel, name: &str) -> solidcore::Result<(Brep, Brep, Op)> {
    let p = Point3::new;
    Ok(match name {
        "box-cyl-hole" => (
            api::create_box(k, p(0.0, 0.0, 0.0), 10.0, 10.0, 10.0)?,
            api::create_cylinder(k, p(5.0, 5.0, -1.0), 2.0, 12.0)?,
            Op::Difference,
        ),
        "box-box-union" => (
            api::create_box(k, p(0.0, 0.0, 0.0), 10.0, 10.0, 10.0)?,
            api::create_box(k, p(5.0, 5.0, 5.0), 10.0, 10.0, 10.0)?,
            Op::Union,
        ),
        "sphere-and-box" => (
            api::create_sphere(k, p(0.0, 0.0, 0.0), 5.0)?,
            api::create_box(k, p(-10.0, -10.0, -10.0), 20.0, 20.0, 12.0)?,
            Op::Intersection,
        ),
        "tpipe" => {
            let a = api::create_cylinder(k, p(0.0, 0.0, -10.0), 2.0, 20.0)?;
            let mut b = api::create_cylinder(k, p(0.0, 0.0, 0.0), 1.5, 10.0)?;
            let pl = Axis2Placement::new(
                p(0.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(0.0, 0.0, 1.0),
            );
            solidcore::topology::brep::p5::transform(k, &mut b, &pl, None, false)?;
            (a, b, Op::Union)
        }
        "coplanar-union" => (
            api::create_box(k, p(0.0, 0.0, 0.0), 10.0, 10.0, 10.0)?,
            api::create_box(k, p(10.0, 0.0, 0.0), 10.0, 5.0, 10.0)?,
            Op::Union,
        ),
        "tangent-sphere-box" => (
            api::create_box(k, p(0.0, 0.0, 0.0), 10.0, 10.0, 10.0)?,
            api::create_sphere(k, p(5.0, 5.0, 13.0), 3.0)?,
            Op::Union,
        ),
        // The point contacts of `point_contacts`: two solids touching at one point.
        // r=7.5 sphere touching the box's x = 10 face at (10, 15, 12.5), on its seam
        // meridian (#2520), in both operand orders.
        "seam-sphere-box" => (
            api::create_sphere(k, p(2.5, 15.0, 12.5), 7.5)?,
            api::create_box(k, p(10.0, 0.0, 0.0), 5.0, 17.5, 15.0)?,
            Op::Union,
        ),
        "seam-box-sphere" => (
            api::create_box(k, p(10.0, 0.0, 0.0), 5.0, 17.5, 15.0)?,
            api::create_sphere(k, p(2.5, 15.0, 12.5), 7.5)?,
            Op::Union,
        ),
        // The same sphere touching the box's y = 0 face at longitude 90, off its seam.
        "off-seam-sphere-box" => (
            api::create_sphere(k, p(12.5, -7.5, 7.5), 7.5)?,
            api::create_box(k, p(10.0, 0.0, 0.0), 5.0, 17.5, 15.0)?,
            Op::Union,
        ),
        // An r=2 sphere touching an r=3 one at latitude 30 of its seam meridian.
        "seam-sphere-sphere" => {
            let (sin, cos) = (PI / 6.0).sin_cos();
            (
                api::create_sphere(k, p(0.0, 0.0, 0.0), 3.0)?,
                api::create_sphere(k, p(5.0 * cos, 0.0, 5.0 * sin), 2.0)?,
                Op::Union,
            )
        }
        // An r=3 sphere touching an r=2 h=10 cylinder halfway up its seam line, and a
        // quarter turn from it.
        "seam-cylinder-sphere" => (
            api::create_cylinder(k, p(0.0, 0.0, 0.0), 2.0, 10.0)?,
            api::create_sphere(k, p(5.0, 0.0, 5.0), 3.0)?,
            Op::Union,
        ),
        "off-seam-cylinder-sphere" => (
            api::create_cylinder(k, p(0.0, 0.0, 0.0), 2.0, 10.0)?,
            api::create_sphere(k, p(0.0, 5.0, 5.0), 3.0)?,
            Op::Union,
        ),
        // An r=2 sphere touching an R=5 r=1.5 torus at 45 degrees round its seam
        // meridian.
        "seam-torus-sphere" => {
            let a = PI / 4.0;
            let (x, z) = (5.0 + 3.5 * a.cos(), 3.5 * a.sin());
            (
                api::create_torus(k, p(0.0, 0.0, 0.0), 5.0, 1.5)?,
                api::create_sphere(k, p(x, 0.0, z), 2.0)?,
                Op::Union,
            )
        }
        // A box touching the same torus's outer equator (a seam circle) at (0, 6.5, 0),
        // and at (6.5, 0, 0), where its two seams cross (a seam vertex).
        "seam-torus-box" => (
            api::create_torus(k, p(0.0, 0.0, 0.0), 5.0, 1.5)?,
            api::create_box(k, p(-3.0, 6.5, -3.0), 6.0, 4.0, 6.0)?,
            Op::Union,
        ),
        "seam-vertex-torus-box" => (
            api::create_torus(k, p(0.0, 0.0, 0.0), 5.0, 1.5)?,
            api::create_box(k, p(6.5, -3.0, -3.0), 4.0, 6.0, 6.0)?,
            Op::Union,
        ),
        // The box less its inscribed r=5 sphere, which touches every face at its centre:
        // (10, 5, 5) on the sphere's seam meridian, as in `seam-sphere-box`.
        "seam-box-minus-sphere" => (
            api::create_box(k, p(0.0, 0.0, 0.0), 10.0, 10.0, 10.0)?,
            api::create_sphere(k, p(5.0, 5.0, 5.0), 5.0)?,
            Op::Difference,
        ),
        "torus-minus-box" => (
            api::create_torus(k, p(0.0, 0.0, 0.0), 5.0, 1.5)?,
            api::create_box(k, p(0.0, -10.0, -10.0), 20.0, 20.0, 20.0)?,
            Op::Difference,
        ),
        _ => unreachable!("unknown case {name}"),
    })
}

fn cases() -> Vec<Case> {
    vec![
        // 10^3 box minus an r=2 through-hole of length 10.
        Case {
            name: "box-cyl-hole",
            volume: 1000.0 - 40.0 * PI,
            smlib_volume: 874.336289861,
            smlib_faces: 7,
        },
        // Two 10^3 boxes overlapping in a 5^3 cube.
        Case {
            name: "box-box-union",
            volume: 1875.0,
            smlib_volume: 1875.0,
            smlib_faces: 12,
        },
        // r=5 sphere below z=2: sphere minus a cap of height 3.
        Case {
            name: "sphere-and-box",
            volume: 500.0 * PI / 3.0 - 36.0 * PI,
            smlib_volume: 410.501465112,
            smlib_faces: 2,
        },
        // r=2 h=20 pipe plus an r=1.5 h=10 branch, minus their overlap (numerical integral).
        Case {
            name: "tpipe",
            volume: 308.956664169885,
            smlib_volume: 308.956697417,
            smlib_faces: 5,
        },
        // 10^3 box plus a 10x5x10 box sharing the x=10 plane.
        Case {
            name: "coplanar-union",
            volume: 1500.0,
            smlib_volume: 1500.0,
            smlib_faces: 8,
        },
        // 10^3 box plus an r=3 sphere touching its top face at one point.
        Case {
            name: "tangent-sphere-box",
            volume: 1000.0 + 36.0 * PI,
            smlib_volume: 1113.09733917,
            smlib_faces: 7,
        },
        // Half of an R=5 r=1.5 torus.
        Case {
            name: "torus-minus-box",
            volume: 2.25 * PI * 5.0 * PI,
            smlib_volume: 111.033062089,
            smlib_faces: 3,
        },
    ]
}

fn run(case: &Case) -> Result<(), String> {
    let mut k = Kernel::default();
    let (a, b, op) = operands(&mut k, case.name).map_err(|e| format!("operands: {e}"))?;
    let result = match op {
        Op::Union => boolean_union(&mut k, a, b),
        Op::Intersection => boolean_intersection(&mut k, a, b),
        Op::Difference => boolean_difference(&mut k, a, b),
    };
    let mut brep = match result {
        Ok(b) => b,
        Err(e) if e.is_unported() => return Err(format!("Unported on the public path: {e}")),
        Err(e) => return Err(format!("boolean failed: {e}")),
    };
    solidcore::topology::integrity::check(&brep).map_err(|e| format!("integrity: {e}"))?;
    let solid = solidcore::topology::brep::p2::is_manifold_solid(&brep)
        .map_err(|e| format!("is_manifold_solid: {e}"))?;
    if !solid {
        return Err("result is not a closed manifold solid".into());
    }
    let faces = solidcore::topology::brep::p1::faces(&brep, None)
        .map_err(|e| format!("faces: {e}"))?
        .len();
    if faces != case.smlib_faces {
        return Err(format!("{faces} faces vs SMLib {}", case.smlib_faces));
    }
    let store = AttributeStore::new();
    let key_of = |_| ObjectKey(0);
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    let v = brep_volume(&k, &mut brep, 1e-7, &attrs).map_err(|e| format!("volume: {e}"))?;
    for (what, want) in [("analytic", case.volume), ("SMLib", case.smlib_volume)] {
        let rel = (v - want).abs() / want.abs();
        if rel > VOLUME_REL_TOL {
            return Err(format!("volume {v} vs {what} {want} (rel {rel:.2e})"));
        }
    }
    Ok(())
}

/// A union of two solids touching at one point, or a difference whose cavity touches
/// the outside.
struct PointContact {
    name: &'static str,
    /// Analytic volume.
    volume: f64,
    /// Face count: every face of both operands.
    faces: usize,
}

fn point_contacts() -> Vec<PointContact> {
    let sphere = |r: f64| 4.0 / 3.0 * PI * r.powi(3);
    let torus = 2.0 * PI * PI * 5.0 * 1.5 * 1.5;
    let contact = |name, volume, faces| PointContact {
        name,
        volume,
        faces,
    };
    // A `seam-` contact lies inside a seam edge: the cleanup must not join the seam
    // through the contact vertex, which would move the other face's vertex loop to the
    // seam's far end (WR-727, #2520).
    vec![
        // At the sphere's pole, a seam vertex (also in `cases`).
        contact("tangent-sphere-box", 1000.0 + sphere(3.0), 7),
        contact("seam-sphere-box", 1312.5 + sphere(7.5), 7),
        contact("seam-box-sphere", 1312.5 + sphere(7.5), 7),
        contact("off-seam-sphere-box", 1312.5 + sphere(7.5), 7),
        contact("seam-sphere-sphere", sphere(3.0) + sphere(2.0), 2),
        contact("seam-cylinder-sphere", 40.0 * PI + sphere(3.0), 4),
        contact("off-seam-cylinder-sphere", 40.0 * PI + sphere(3.0), 4),
        contact("seam-torus-sphere", torus + sphere(2.0), 2),
        contact("seam-torus-box", torus + 144.0, 7),
        contact("seam-vertex-torus-box", torus + 144.0, 7),
        contact("seam-box-minus-sphere", 1000.0 - sphere(5.0), 7),
    ]
}

/// The boolean of `case`: a closed manifold solid with every face of both operands, the
/// analytic volume and a clean Level2 check.
fn run_point_contact(case: &PointContact) -> Result<(), String> {
    let mut k = Kernel::default();
    let (a, b, op) = operands(&mut k, case.name).map_err(|e| format!("operands: {e}"))?;
    let result = match op {
        Op::Union => boolean_union(&mut k, a, b),
        Op::Intersection => boolean_intersection(&mut k, a, b),
        Op::Difference => boolean_difference(&mut k, a, b),
    };
    let mut brep = result.map_err(|e| format!("boolean failed: {e}"))?;
    let solid = solidcore::topology::brep::p2::is_manifold_solid(&brep)
        .map_err(|e| format!("is_manifold_solid: {e}"))?;
    if !solid {
        return Err("result is not a closed manifold solid".into());
    }
    let faces = solidcore::topology::brep::p1::faces(&brep, None)
        .map_err(|e| format!("faces: {e}"))?
        .len();
    if faces != case.faces {
        return Err(format!("{faces} faces vs {}", case.faces));
    }
    let store = AttributeStore::new();
    let key_of = |_| ObjectKey(0);
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    let v = brep_volume(&k, &mut brep, 1e-7, &attrs).map_err(|e| format!("volume: {e}"))?;
    let rel = (v - case.volume).abs() / case.volume;
    if rel > VOLUME_REL_TOL {
        return Err(format!(
            "volume {v} vs analytic {} (rel {rel:.2e})",
            case.volume
        ));
    }
    let check = check_brep(&mut k, &mut brep, CheckLevel::Level2)
        .map_err(|e| format!("check_brep: {e}"))?;
    if !check.is_valid() {
        return Err(format!("Level2 check: {:?}", check.lines()));
    }
    Ok(())
}

// #2520: two solids touching at one point, on a seam edge (sphere, cylinder, torus),
// at a seam vertex or off the seam, unite into a body that checks clean at Level2, and so
// does the box less the sphere inscribed in it.
#[test]
fn point_contact_unions_check_clean() {
    let failures: Vec<String> = point_contacts()
        .iter()
        .filter_map(|c| {
            run_point_contact(c)
                .err()
                .map(|e| format!("{}: {e}", c.name))
        })
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn public_booleans_never_return_unported() {
    let unported: Vec<String> = cases()
        .iter()
        .filter_map(|c| {
            let mut k = Kernel::default();
            let (a, b, op) = operands(&mut k, c.name).ok()?;
            let r = match op {
                Op::Union => boolean_union(&mut k, a, b),
                Op::Intersection => boolean_intersection(&mut k, a, b),
                Op::Difference => boolean_difference(&mut k, a, b),
            };
            match r {
                Err(e) if e.is_unported() => Some(format!("{}: {e}", c.name)),
                _ => None,
            }
        })
        .collect();
    assert!(unported.is_empty(), "{unported:#?}");
}

#[test]
fn public_booleans_give_closed_solids_with_analytic_volume() {
    let failures: Vec<String> = cases()
        .iter()
        .filter_map(|c| run(c).err().map(|e| format!("{}: {e}", c.name)))
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
}

/// A slice by a solid fails with a typed reason in solidcore's terms: the stage's path, a phrase,
/// the stable kind, and the comparison status (`SM_ERR`) the C++ reports for it.
#[test]
fn a_slice_by_a_solid_fails_with_a_typed_reason() {
    use solidcore::boolean::api_brep::boolean;
    use solidcore::error::{ErrorKind, SolidError};
    use solidcore::geom::a_object::BooleanOperation;

    let mut k = Kernel::default();
    let (a, b, _) = operands(&mut k, "box-box-union").unwrap();
    let error = boolean(&mut k, a, b, BooleanOperation::Slice).unwrap_err();
    assert!(matches!(error, SolidError::Failed(_)), "{error:?}");
    assert_eq!(error.kind(), ErrorKind::Failed);
    assert_eq!(error.sm_status(), Some(solidcore::error::SM_ERR));
    assert_eq!(error.operation(), "boolean::merge::p4::manifold_boolean");
    assert_eq!(
        error.to_string(),
        "boolean::merge::p4::manifold_boolean: a slice needs a sheet, and the B-rep is not one"
    );
    assert!(!error.is_port_failure());
}

/// The Boolean subsystem names no SMLib symbol in an error, a message or an operation path.
#[test]
fn boolean_sources_name_no_smlib_symbol_in_a_string() {
    fn visit(dir: &std::path::Path, found: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(&path, found);
            } else if path.extension().is_some_and(|e| e == "rs") {
                for (n, line) in std::fs::read_to_string(&path).unwrap().lines().enumerate() {
                    let code = line.trim_start();
                    if code.starts_with("//") {
                        continue;
                    }
                    let mut rest = code;
                    while let Some(i) = rest.find("\"Sm") {
                        if rest[i + 3..].starts_with(|c: char| c.is_ascii_uppercase()) {
                            found.push(format!("{}:{}: {line}", path.display(), n + 1));
                        }
                        rest = &rest[i + 3..];
                    }
                }
            }
        }
    }
    let mut found = Vec::new();
    visit(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/boolean"),
        &mut found,
    );
    assert!(found.is_empty(), "{found:#?}");
}

// ---- a body minus an exact copy of itself ----------------------------------------------

/// SolidCraft's fix list (`kernel_cases/fixes.json`).
fn rig_kernel() -> Kernel {
    let names: Vec<String> = serde_json::from_str(include_str!("kernel_cases/fixes.json")).unwrap();
    let mut fixes = solidcore::kernel::UpstreamFixes::default();
    for name in &names {
        if let Some(f) = fixes.flag_mut(name) {
            *f = true;
        }
    }
    Kernel {
        fixes,
        ..Kernel::default()
    }
}

/// `b` with the tolerances a STEP file carries (0.01 on the body and every face, edge and
/// vertex), as SolidCraft imports one.
fn with_step_tolerances(k: &Kernel, mut b: Brep) -> Brep {
    use solidcore::topology::brep::{p1, p2};
    p2::set_tolerance(k, &mut b, 0.01, true, true).unwrap();
    for f in p1::faces(&b, None).unwrap() {
        solidcore::topology::face::p3::set_tolerance(&mut b, f, 0.01, true, true).unwrap();
    }
    b
}

/// The faces left by cutting `b` with an exact copy of itself: none, for a closed result
/// of nothing.
fn faces_left_by_own_copy(k: &mut Kernel, b: Brep) -> usize {
    let copy = solidcore::topology::brep::p3::brep_copy(k, &b).unwrap();
    let r = boolean_difference(k, b, copy).unwrap();
    solidcore::topology::brep::p1::faces(&r, None)
        .unwrap()
        .len()
}

/// A disc with a hub, a bore and four pockets, every tolerance 0.01: the intersection
/// tolerance is half that and upstream's capture distance 2000 times it, 10 mm, which
/// captured vertices of the 30 mm part up to 10 mm apart into one proto vertex, and the cut
/// by its own copy kept faces. The capture stays below half the nearest two vertices'
/// distance, and nothing is left: with SolidCraft's fix list and with the default kernel
/// (every `UpstreamFixes` flag off), since the rule is no longer a flag.
#[test]
fn a_part_with_step_tolerances_minus_itself_is_empty() {
    for (kernel, mut k) in [
        ("SolidCraft's fixes", rig_kernel()),
        ("the default kernel", Kernel::default()),
    ] {
        let p = Point3::new;
        let disc = api::create_cylinder(&mut k, p(0.0, 0.0, 0.0), 15.0, 3.0).unwrap();
        let hub = api::create_cylinder(&mut k, p(0.0, 0.0, 0.0), 4.0, 8.0).unwrap();
        let mut b = boolean_union(&mut k, disc, hub).unwrap();
        let bore = api::create_cylinder(&mut k, p(0.0, 0.0, -1.0), 1.5, 10.0).unwrap();
        b = boolean_difference(&mut k, b, bore).unwrap();
        for (x, y) in [(7.0, -2.0), (-11.0, -2.0), (-2.0, 7.0), (-2.0, -11.0)] {
            let pocket = api::create_box(&mut k, p(x, y, 1.0), 4.0, 4.0, 3.0).unwrap();
            b = boolean_difference(&mut k, b, pocket).unwrap();
        }
        let b = with_step_tolerances(&k, b);
        assert_eq!(faces_left_by_own_copy(&mut k, b), 0, "{kernel}");
    }
}

/// A block with a boss and a bore through it, every tolerance 0.01: at the bore's rim the
/// radial sector test steps off the edge until it leaves a face. Upstream read the
/// coincident bore face's side against the edge's other face (the boss top) instead of the
/// face the bore face lies on, took the bore and its copy for opposite sides and kept the
/// bore face. The side is read against the face the bore face lies on, and nothing is left.
#[test]
fn a_bore_minus_its_copy_is_empty_when_the_sector_steps_off() {
    for (r, h) in [(0.3, 0.2), (0.1, 0.05), (0.05, 0.02)] {
        let mut k = rig_kernel();
        let p = Point3::new;
        let block = api::create_box(&mut k, p(-2.0, -2.0, 0.0), 4.0, 4.0, 1.0).unwrap();
        let boss = api::create_cylinder(&mut k, p(0.0, 0.0, 1.0), 3.0 * r, h).unwrap();
        let mut b = boolean_union(&mut k, block, boss).unwrap();
        let bore = api::create_cylinder(&mut k, p(0.0, 0.0, 0.5), r, 2.0).unwrap();
        b = boolean_difference(&mut k, b, bore).unwrap();
        let b = with_step_tolerances(&k, b);
        assert_eq!(faces_left_by_own_copy(&mut k, b), 0, "r {r}");
    }
}

// ---- a box edge cutting a small sphere's circle just inside its tangent ---------------

/// `|sphere(0, r) ∩ {y <= -delta} ∩ {z >= zmin}|`, by the midpoint rule over slices in y
/// (each slice a disc less the segment below `zmin`); its error is far below the test's.
fn sphere_box_corner_volume(r: f64, delta: f64, zmin: f64) -> f64 {
    let n = 200_000;
    let h = (r - delta) / f64::from(n);
    let d = -zmin;
    let slice = |y: f64| {
        let s2 = (r * r - y * y).max(0.0);
        let s = s2.sqrt();
        let below = if d < s {
            s2 * (d / s).acos() - d * (s2 - d * d).sqrt()
        } else {
            0.0
        };
        PI * s2 - below
    };
    (0..n)
        .map(|i| slice(-r + (f64::from(i) + 0.5) * h))
        .sum::<f64>()
        * h
}

/// An r = 0.3 mm sphere and a box whose bottom face cuts a cap 1.2e-5 to 1.5e-5 high off
/// it (about one tolerance, 1e-5) and whose side face y = -delta passes near the centre,
/// away from the sphere's seam. The box's bottom edge crosses each of the two circles the
/// faces cut from the sphere twice, the two crossings 1.2e-4 to 1.6e-4 apart. The
/// circle/line intersector looks for a line within ten intersection tolerances (2e-4) of
/// the circle and also merged two crossings closer than that into one point mid-chord, off
/// the circle, so no crossing was found, the box edge was not split, and the boolean gave
/// nothing, the sphere alone (57 % off) or "no radial sector" (`gen0150` of the golden unit
/// `boolean.api_brep` is this case with the sphere's seam close to the side face). The
/// crossings are now merged only when their midpoint lies on the circle within the
/// intersection tolerance: union, intersection and difference integrate to the exact
/// volume, have the expected faces, edges and vertices, every vertex on the sphere or
/// the box, and check clean at Level2. The volume
/// tolerance (1e-3 relative) allows for tolerance-wide slivers at a 0.3 mm scale
/// (measured: below 1e-5).
#[test]
fn sphere_box_edge_cutting_a_shallow_cap() {
    let r: f64 = 3e-4;
    let sphere = 4.0 / 3.0 * PI * r.powi(3);
    let mut failures = Vec::new();
    for (delta, cap) in [(5e-5, 1.5e-5), (6e-5, 1.2e-5)] {
        let zmin = -r + cap;
        let size = [8e-4, 4e-4 - delta, 4e-4 - zmin];
        let block = size[0] * size[1] * size[2];
        let common = sphere_box_corner_volume(r, delta, zmin);
        // Faces, edges, vertices. Intersection: the sphere piece and the two box face
        // pieces, bounded by the two arcs and the box edge between its crossings (plus the
        // sphere's seam piece); union: the box's six faces and the sphere; difference: the
        // sphere and the two box face pieces. Measured, the same for both cases; a lost
        // crossing or an unintended split changes them.
        for (op, want, want_counts) in [
            (Op::Union, sphere + block - common, [7, 17, 13]),
            (Op::Intersection, common, [3, 4, 3]),
            (Op::Difference, sphere - common, [3, 5, 5]),
        ] {
            let name = match op {
                Op::Union => "union",
                Op::Intersection => "intersection",
                Op::Difference => "difference",
            };
            let mut k = Kernel::default();
            let a = api::create_sphere(&mut k, Point3::new(0.0, 0.0, 0.0), r).unwrap();
            let b = api::create_box(
                &mut k,
                Point3::new(-4e-4, -4e-4, zmin),
                size[0],
                size[1],
                size[2],
            )
            .unwrap();
            let result = match op {
                Op::Union => boolean_union(&mut k, a, b),
                Op::Intersection => boolean_intersection(&mut k, a, b),
                Op::Difference => boolean_difference(&mut k, a, b),
            };
            let mut brep = match result {
                Ok(b) => b,
                Err(e) => {
                    failures.push(format!("delta {delta:e} cap {cap:e} {name}: {e}"));
                    continue;
                }
            };
            let store = AttributeStore::new();
            let key_of = |_| ObjectKey(0);
            let attrs = PropertyAttributes {
                store: &store,
                key_of: &key_of,
            };
            let v = brep_volume(&k, &mut brep, 1e-9, &attrs).unwrap();
            let rel = (v - want).abs() / want;
            // Written to accept: a NaN volume fails.
            if rel.is_nan() || rel > 1e-3 {
                failures.push(format!(
                    "delta {delta:e} cap {cap:e} {name}: volume {v:e} vs {want:e} (rel {rel:.2e})"
                ));
            }
            let counts = [
                solidcore::topology::brep::p1::faces(&brep, None)
                    .unwrap()
                    .len(),
                solidcore::topology::brep::p1::edges(&brep, None)
                    .unwrap()
                    .len(),
                solidcore::topology::brep::p1::vertices(&brep, None)
                    .unwrap()
                    .len(),
            ];
            if counts != want_counts {
                failures.push(format!(
                    "delta {delta:e} cap {cap:e} {name}: faces/edges/vertices {counts:?} vs {want_counts:?}"
                ));
            }
            // Every vertex lies on the sphere or on the box within the tolerance band.
            let (lo, hi) = (
                Point3::new(-4e-4, -4e-4, zmin),
                Point3::new(-4e-4 + size[0], -delta, 4e-4),
            );
            for vid in solidcore::topology::brep::p1::vertices(&brep, None).unwrap() {
                let p = brep.vertex(vid).unwrap().point;
                let on_sphere = (p.length() - r).abs();
                let inside = Point3::new(
                    p.x.clamp(lo.x, hi.x),
                    p.y.clamp(lo.y, hi.y),
                    p.z.clamp(lo.z, hi.z),
                );
                let outside = p.distance(inside);
                let to_planes = [
                    p.x - lo.x,
                    hi.x - p.x,
                    p.y - lo.y,
                    hi.y - p.y,
                    p.z - lo.z,
                    hi.z - p.z,
                ]
                .into_iter()
                .map(f64::abs)
                .fold(f64::INFINITY, f64::min);
                let on_box = outside.max(to_planes);
                if on_sphere.is_nan() || on_box.is_nan() || on_sphere.min(on_box) > 2e-5 {
                    failures.push(format!(
                        "delta {delta:e} cap {cap:e} {name}: vertex {p:?} is {on_sphere:e} off the sphere and {on_box:e} off the box"
                    ));
                }
            }
            let check = check_brep(&mut k, &mut brep, CheckLevel::Level2).unwrap();
            if !check.is_valid() {
                failures.push(format!(
                    "delta {delta:e} cap {cap:e} {name}: Level2 {:?}",
                    check.lines()
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

// ---- a box edge crossing a small sphere twice, farther apart than the tolerance ---------

/// An r = 0.3 mm sphere and a box whose top face y = r - cap cuts a cap 1.6e-5 to 2.2e-5
/// high off it and whose face z = -delta passes near the centre: the box edge along x
/// where the two faces meet crosses the sphere twice, the crossings 1.4e-4 to 1.7e-4
/// apart, each fixed to the intersection tolerance (5e-6). The proto-vertex capture
/// distance (half the nearest vertex pair of either operand, here above the crossings'
/// distance) took the two for one point: the arc between them became a loop on one vertex
/// and the boolean gave the sphere alone, or "no radial sector of the edge takes the new
/// edge use". Two points farther apart than their tolerances are two vertices
/// (docs/booleans.md, "One point or two"): union, intersection and difference integrate
/// to the exact volume (measured: within 1.1e-8 relative), have the faces, edges and
/// vertices of the exact result, and check clean at Level2.
#[test]
fn sphere_box_edge_crossings_farther_apart_than_tolerance_are_two_vertices() {
    let r: f64 = 3e-4;
    let sphere = 4.0 / 3.0 * PI * r.powi(3);
    let mut failures = Vec::new();
    for (delta, cap) in [
        (6e-5, 1.6e-5),
        (6e-5, 1.8e-5),
        (7e-5, 1.8e-5),
        (7e-5, 2e-5),
        (8e-5, 2e-5),
        (8e-5, 2.2e-5),
    ] {
        // the box [-4e-4, 4e-4] x [-4e-4, r - cap] x [-4e-4, -delta]; its part of the
        // sphere is `{z <= -delta} and {y <= r - cap}`, the corner volume with y and z
        // exchanged
        let size = [8e-4, r - cap + 4e-4, 4e-4 - delta];
        let block = size[0] * size[1] * size[2];
        let common = sphere_box_corner_volume(r, delta, -(r - cap));
        // the half chord of the box edge in the sphere: the crossings are twice it apart
        let apart = 2.0 * (r * r - (r - cap).powi(2) - delta * delta).sqrt();
        assert!(
            apart > 1.3e-4 && apart < 1.8e-4,
            "crossings {apart:e} apart"
        );
        // Union: the box's six faces and the sphere; intersection and difference: the
        // sphere's piece and the two box face pieces.
        for (op, name, want, want_counts) in [
            (Op::Union, "union", sphere + block - common, [7, 17, 12]),
            (Op::Intersection, "intersection", common, [3, 5, 4]),
            (Op::Difference, "difference", sphere - common, [3, 5, 4]),
        ] {
            let mut k = Kernel::default();
            let a = api::create_sphere(&mut k, Point3::new(0.0, 0.0, 0.0), r).unwrap();
            let b = api::create_box(
                &mut k,
                Point3::new(-4e-4, -4e-4, -4e-4),
                size[0],
                size[1],
                size[2],
            )
            .unwrap();
            let result = match op {
                Op::Union => boolean_union(&mut k, a, b),
                Op::Intersection => boolean_intersection(&mut k, a, b),
                Op::Difference => boolean_difference(&mut k, a, b),
            };
            let mut brep = match result {
                Ok(b) => b,
                Err(e) => {
                    failures.push(format!("delta {delta:e} cap {cap:e} {name}: {e}"));
                    continue;
                }
            };
            let store = AttributeStore::new();
            let key_of = |_| ObjectKey(0);
            let attrs = PropertyAttributes {
                store: &store,
                key_of: &key_of,
            };
            let v = brep_volume(&k, &mut brep, 1e-9, &attrs).unwrap();
            let rel = (v - want).abs() / want;
            // Written to accept: a NaN volume fails.
            if rel.is_nan() || rel > VOLUME_REL_TOL {
                failures.push(format!(
                    "delta {delta:e} cap {cap:e} {name}: volume {v:e} vs {want:e} (rel {rel:.2e})"
                ));
            }
            let counts = [
                solidcore::topology::brep::p1::faces(&brep, None)
                    .unwrap()
                    .len(),
                solidcore::topology::brep::p1::edges(&brep, None)
                    .unwrap()
                    .len(),
                solidcore::topology::brep::p1::vertices(&brep, None)
                    .unwrap()
                    .len(),
            ];
            if counts != want_counts {
                failures.push(format!(
                    "delta {delta:e} cap {cap:e} {name}: faces/edges/vertices {counts:?} vs {want_counts:?}"
                ));
            }
            let check = check_brep(&mut k, &mut brep, CheckLevel::Level2).unwrap();
            if !check.is_valid() {
                failures.push(format!(
                    "delta {delta:e} cap {cap:e} {name}: Level2 {:?}",
                    check.lines()
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The area of the disc of radius `rho` about `(cx, cy)` inside the rectangle
/// `[x0, x1] x [y0, y1]`: the chord `min(cy + s, y1) - max(cy - s, y0)` at each x,
/// `s = sqrt(rho^2 - (x - cx)^2)`, integrated in closed form between the abscissae where
/// the circle meets a side of the rectangle.
fn disc_rect_area(rho: f64, c: [f64; 2], x: [f64; 2], y: [f64; 2]) -> f64 {
    let (a, b) = (x[0].max(c[0] - rho), x[1].min(c[0] + rho));
    if rho <= 0.0 || b <= a {
        return 0.0;
    }
    // the integral of s from the centre to x
    let half_disc = |x: f64| {
        let t = ((x - c[0]) / rho).clamp(-1.0, 1.0);
        0.5 * rho * rho * (t.asin() + t * (1.0 - t * t).sqrt())
    };
    let mut cuts = vec![a, b];
    for h in [y[1] - c[1], c[1] - y[0]] {
        if h > 0.0 && h < rho {
            let w = (rho * rho - h * h).sqrt();
            cuts.extend([c[0] - w, c[0] + w].into_iter().filter(|&v| v > a && v < b));
        }
    }
    cuts.sort_by(f64::total_cmp);
    let mut area = 0.0;
    for w in cuts.windows(2) {
        let &[u, v] = w else {
            continue;
        };
        let m = 0.5 * (u + v);
        let s = (rho * rho - (m - c[0]).powi(2)).max(0.0).sqrt();
        if (c[1] + s).min(y[1]) - (c[1] - s).max(y[0]) <= 0.0 {
            continue;
        }
        let arc = half_disc(v) - half_disc(u);
        let top = if c[1] + s <= y[1] {
            c[1] * (v - u) + arc
        } else {
            y[1] * (v - u)
        };
        let bottom = if c[1] - s >= y[0] {
            c[1] * (v - u) - arc
        } else {
            y[0] * (v - u)
        };
        area += top - bottom;
    }
    area
}

/// `|sphere(c, r) ∩ box(o, o + d)|`, by the midpoint rule over slices in z of
/// [`disc_rect_area`]; its error is far below the test's (5.424275463e-11 for the case
/// below, as an adaptive quadrature to 1e-12 gives).
fn sphere_box_volume(c: [f64; 3], r: f64, o: [f64; 3], d: [f64; 3]) -> f64 {
    let (z0, z1) = ((c[2] - r).max(o[2]), (c[2] + r).min(o[2] + d[2]));
    if z1 <= z0 {
        return 0.0;
    }
    let n = 200_000;
    let h = (z1 - z0) / f64::from(n);
    (0..n)
        .map(|i| {
            let z = z0 + (f64::from(i) + 0.5) * h;
            let rho = (r * r - (z - c[2]).powi(2)).max(0.0).sqrt();
            disc_rect_area(rho, [c[0], c[1]], [o[0], o[0] + d[0]], [o[1], o[1] + d[1]])
        })
        .sum::<f64>()
        * h
}

/// `gen0150` of the golden unit `boolean.api_brep` turned a quarter turn about x, so that
/// the sphere's seam is away from the box's side face: a sphere of radius 3.06e-4 (inside
/// out as given) and a 7.5e-4 x 1.44e-3 x 3.9e-4 box whose edge crosses it twice, the two
/// crossings 1.7e-4 apart. Taken for one point by the capture distance, the union came
/// out empty and the intersection and the difference were refused ("no radial sector of
/// the edge takes the new edge use"). As two vertices the three results integrate to the
/// exact volumes (measured: within 1.3e-7 relative), with the faces, edges and vertices of
/// the exact result, every vertex on the sphere or the box within its tolerance, and the
/// box edge's two crossings, each on the sphere and on the edge's two faces, 1.7e-4 apart.
/// The Level2 check is not asserted: it reports the circles' own STEP-to-NURB test at this
/// size (`SmEllipse` test 1, 5e-10 against 1e-10), on the operands' circles as on the
/// results'.
#[test]
fn a_tiny_sphere_and_a_box_whose_edge_crosses_it_twice_give_exact_volumes() {
    use solidcore::topology::vertex;
    let c = [
        0.000_540_068_099_454_042_4,
        -0.000_116_855_468_417_464_12,
        0.001_078_936_931_531_837_3,
    ];
    let r = -0.000_306_149_591_313_848_4_f64;
    let o = [
        0.000_213_119_293_820_846_02,
        -0.001_265_085_547_303_017_4,
        0.000_664_385_071_073_306_7,
    ];
    let d = [
        0.000_751_644_731_134_943_5,
        0.001_441_225_834_284_898_7,
        0.000_394_815_843_801_023_2,
    ];
    let sphere = 4.0 / 3.0 * PI * r.abs().powi(3);
    let block = d[0] * d[1] * d[2];
    let common = sphere_box_volume(c, r.abs(), o, d);
    assert!((common - 5.424_275_463e-11).abs() < 1e-19, "{common:e}");
    let mut failures = Vec::new();
    // how far `p` is from the sphere, and on how many of the box's six planes it lies within
    // `tol` (inside the box's slab on the others)
    let from_sphere = |p: Point3| (p - Point3::new(c[0], c[1], c[2])).length() - r.abs();
    let box_planes = |p: Point3, tol: f64| {
        let q = [p.x, p.y, p.z];
        let mut on = 0;
        for i in 0..3 {
            let (lo, hi) = (o[i], o[i] + d[i]);
            if q[i] < lo - tol || q[i] > hi + tol {
                return None;
            }
            on += usize::from((q[i] - lo).abs() <= tol) + usize::from((q[i] - hi).abs() <= tol);
        }
        Some(on)
    };
    for (op, name, want, want_counts) in [
        // the exact result's: union the box's six faces and the sphere; intersection and
        // difference the sphere's piece and the two box face pieces about the crossed edge
        (Op::Union, "union", sphere + block - common, [7, 17, 12]),
        (Op::Intersection, "intersection", common, [3, 5, 4]),
        (Op::Difference, "difference", sphere - common, [3, 5, 4]),
    ] {
        let mut k = Kernel::default();
        let a = api::create_sphere(&mut k, Point3::new(c[0], c[1], c[2]), r).unwrap();
        let b = api::create_box(&mut k, Point3::new(o[0], o[1], o[2]), d[0], d[1], d[2]).unwrap();
        let result = match op {
            Op::Union => boolean_union(&mut k, a, b),
            Op::Intersection => boolean_intersection(&mut k, a, b),
            Op::Difference => boolean_difference(&mut k, a, b),
        };
        let mut brep = match result {
            Ok(b) => b,
            Err(e) => {
                failures.push(format!("{name}: {e}"));
                continue;
            }
        };
        let store = AttributeStore::new();
        let key_of = |_| ObjectKey(0);
        let attrs = PropertyAttributes {
            store: &store,
            key_of: &key_of,
        };
        let v = brep_volume(&k, &mut brep, 1e-9, &attrs).unwrap();
        let rel = (v - want).abs() / want;
        // Written to accept: a NaN volume fails.
        if rel.is_nan() || rel > VOLUME_REL_TOL {
            failures.push(format!("{name}: volume {v:e} vs {want:e} (rel {rel:.2e})"));
        }
        let counts = [
            solidcore::topology::brep::p1::faces(&brep, None)
                .unwrap()
                .len(),
            solidcore::topology::brep::p1::edges(&brep, None)
                .unwrap()
                .len(),
            solidcore::topology::brep::p1::vertices(&brep, None)
                .unwrap()
                .len(),
        ];
        if counts != want_counts {
            failures.push(format!(
                "{name}: faces/edges/vertices {counts:?} vs {want_counts:?}"
            ));
        }
        // every vertex on the sphere or the box; the crossings on both, on two box faces
        let mut crossings = Vec::new();
        for v in solidcore::topology::brep::p1::vertices(&brep, None).unwrap() {
            let (p, tol) = (
                vertex::point(&brep, v).unwrap(),
                vertex::tolerance(&brep, v).unwrap(),
            );
            let on_sphere = from_sphere(p).abs() <= tol;
            let planes = box_planes(p, tol);
            if !on_sphere && planes.is_none_or(|n| n == 0) {
                failures.push(format!(
                    "{name}: vertex {p:?} (tol {tol:e}) on neither body"
                ));
            }
            if on_sphere && planes == Some(2) {
                crossings.push(p);
            }
        }
        let apart = match crossings.as_slice() {
            [p, q] => (*p - *q).length(),
            _ => f64::NAN,
        };
        if !(1.6e-4..1.8e-4).contains(&apart) {
            failures.push(format!("{name}: crossings {crossings:?}"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// A closed ellipse of semi-axes `b` (along x) and `a` (along y) about `(0, a)`, as the one
/// rational B-spline of its four quarters (nine control points, knots 0..4), starting at the
/// end of the major axis on +y and running counter-clockwise.
fn ellipse_spline(a: f64, b: f64) -> Curve {
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let p = |x: f64, y: f64| Point3::new(x, y, 0.0);
    let control = [
        p(0.0, 2.0 * a),
        p(-b, 2.0 * a),
        p(-b, a),
        p(-b, 0.0),
        p(0.0, 0.0),
        p(b, 0.0),
        p(b, a),
        p(b, 2.0 * a),
        p(0.0, 2.0 * a),
    ];
    let weights = [1.0, w, 1.0, w, 1.0, w, 1.0, w, 1.0];
    Curve::from(
        BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree: 2,
            control_points: &control,
            form: BSplineCurveForm::default(),
            knot_multiplicities: &[3, 2, 2, 2, 3],
            knots: &[0.0, 1.0, 2.0, 3.0, 4.0],
            knot_type: KnotType::default(),
            weights: Some(&weights),
            trim: None,
        })
        .unwrap(),
    )
}

/// The volume of `brep` at the accuracy the other cases use.
fn volume_of(k: &Kernel, brep: &mut Brep) -> f64 {
    let store = AttributeStore::new();
    let key_of = |_| ObjectKey(0);
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    brep_volume(k, brep, 1e-7, &attrs).unwrap()
}

/// A long thin elliptic prism (semi-axes 67.5 and 5, 4 thick, swept against +z) is a solid
/// facing out: its volume is the ellipse's area times the thickness, and a block cut by it is
/// the block less the part of it the block holds. The ray that picks the region outside is
/// aimed at an edge of the prism, where it meets the wall and the cap at one distance and no
/// edge; the region it found was taken from the wall alone, which put the outside on the
/// inside, and the prism came out with a negative volume.
#[test]
fn a_long_thin_elliptic_prism_faces_out_and_cuts_a_block() {
    let (a, b, thick) = (67.5, 5.0, 4.0);
    let want = PI * a * b * thick;
    let mut k = Kernel::default();
    let curve = ellipse_spline(a, b);
    let mut prism = create_linear_sweep(
        &mut k,
        &[Some(&curve)],
        Vec3::new(0.0, 0.0, -1.0),
        thick,
        true,
    )
    .unwrap();
    let got = volume_of(&k, &mut prism);
    assert!(
        (got - want).abs() < 1e-4 * want,
        "prism volume {got} vs {want}"
    );
    let check = check_brep(&mut k, &mut prism, CheckLevel::Level2).unwrap();
    assert!(check.is_valid(), "{:?}", check.lines());

    // A block whose bottom face cuts the prism through its middle: a groove, open below.
    let block =
        api::create_box(&mut k, Point3::new(-20.0, -20.0, -2.0), 40.0, 200.0, 22.0).unwrap();
    let mut cut = boolean_difference(&mut k, block, prism).unwrap();
    let got = volume_of(&k, &mut cut);
    let check = check_brep(&mut k, &mut cut, CheckLevel::Level2).unwrap();
    assert!(check.is_valid(), "{:?}", check.lines());
    // The block's six faces (the bottom one with a hole), the groove's wall and its ceiling: eight
    // faces; the block's twelve edges, the hole's rim, the ceiling's rim and the wall's seam:
    // fifteen; the block's eight corners and the two ends of the rims' loops: ten.
    let counts = [
        solidcore::topology::brep::p1::faces(&cut, None)
            .unwrap()
            .len(),
        solidcore::topology::brep::p1::edges(&cut, None)
            .unwrap()
            .len(),
        solidcore::topology::brep::p1::vertices(&cut, None)
            .unwrap()
            .len(),
    ];
    assert_eq!(counts, [8, 15, 10], "faces, edges, vertices");
    let want = 40.0 * 200.0 * 22.0 - PI * a * b * 2.0;
    assert!(
        (got - want).abs() < 1e-4 * want,
        "block less prism {got} vs {want}"
    );
}

// ---- an arc grazing a coplanar box face -----------------------------------------------

/// The profile of #16640 extruded 10 along z, then joined with the 5 x 25 x 10 box at
/// (5, -5, 0): `(0, 0)`, `(10, 0)`, `(10, 10)`, an arc about `(cx, cy)` of radius `r` from
/// `(10, 10)` counter-clockwise to `(px, py)`, `(0, py)`. The arc's end `(10, 10)` lies on the
/// box face `x = 10`, which also holds the profile's side from `(10, 0)` to `(10, 10)`.
/// The union is the box plus the part of the profile left of `x = 5`, so its volume is
/// `10 (5 py + 125)`: only a sliver of the arc past `x = 10` (in the grazing
/// case, 0.26 micron wide over 1.6 mm, 0.003 mm3 in all) is added.
fn arc_graze_union(
    k: &mut Kernel,
    centre: (f64, f64),
    r: f64,
    start_deg: f64,
    end_deg: f64,
    top: (f64, f64),
) -> solidcore::Result<Brep> {
    use solidcore::curve::curve::Curve;
    use solidcore::primitive::api_curves as curves;
    use solidcore::sweep::api_brep::create_linear_sweep;
    let p = Point3::new;
    let line = |a: Point3, b: Point3| Curve::from(curves::create_line_segment(a, b).unwrap());
    let profile = [
        line(p(0.0, 0.0, 0.0), p(10.0, 0.0, 0.0)),
        line(p(10.0, 0.0, 0.0), p(10.0, 10.0, 0.0)),
        Curve::from(curves::create_arc(p(centre.0, centre.1, 0.0), r, start_deg, end_deg).unwrap()),
        line(p(top.0, top.1, 0.0), p(0.0, top.1, 0.0)),
        line(p(0.0, top.1, 0.0), p(0.0, 0.0, 0.0)),
    ];
    let refs: Vec<Option<&Curve>> = profile.iter().map(Some).collect();
    let body = create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 10.0, true)?;
    let boxed = api::create_box(k, Point3::new(5.0, -5.0, 0.0), 5.0, 25.0, 10.0)?;
    boolean_union(k, body, boxed)
}

fn assert_closed_with_volume(k: &mut Kernel, mut brep: Brep, want: f64, slack: f64) {
    let store = AttributeStore::new();
    let key_of = |_| ObjectKey(0);
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    let v = brep_volume(k, &mut brep, 1e-9, &attrs).unwrap();
    assert!(
        (v - want).abs() <= slack,
        "volume {v} vs {want} (slack {slack})"
    );
    let check = check_brep(k, &mut brep, CheckLevel::Level2).unwrap();
    assert!(check.is_valid(), "Level2 {:?}", check.lines());
}

/// #16640: an arc, three-point as drawn, that crosses the plane of a coplanar box face
/// 0.26 micron past a shared vertex, 1.6 mm along, and stays within the tolerance of the
/// face's edge for as long again beyond the crossing. The definition of that second
/// stretch ended on no vertex, so its two edges were related whole to each other and to the
/// edges the crossing had already related: "another entity is already related". It makes
/// no relation of its own now; the union is closed and has the volume of the box and the
/// profile left of it.
#[test]
fn an_arc_grazing_a_coplanar_box_face_joins() {
    let mut k = Kernel::default();
    let (py, centre) = (12.941, (5.003_150_514_408_299, 10.001_605_029_505_038));
    let brep = arc_graze_union(
        &mut k,
        centre,
        4.996_849_743_366_094,
        -0.018_403_879_031_201_775,
        36.032_894_179_707_97,
        (9.044, py),
    )
    .unwrap();
    // the added sliver is about 0.003 mm3
    assert_closed_with_volume(&mut k, brep, 10.0 * (5.0 * py + 125.0), 0.01);
}

/// The control of #16640: the same profile with the arc exactly tangent to the face's plane
/// (centre `(5, 10)`, radius 5), which joined before.
#[test]
fn an_arc_tangent_to_a_coplanar_box_face_joins() {
    let mut k = Kernel::default();
    let theta = 36.0_f64;
    let (px, py) = (
        5.0 + 5.0 * theta.to_radians().cos(),
        10.0 + 5.0 * theta.to_radians().sin(),
    );
    let brep = arc_graze_union(&mut k, (5.0, 10.0), 5.0, 0.0, theta, (px, py)).unwrap();
    assert_closed_with_volume(&mut k, brep, 10.0 * (5.0 * py + 125.0), 1e-6 * 2000.0);
}

/// tangency-labs/solidcraft#16532: a 10 mm box with its edge at (0, 0) rounded with r 2, joined
/// by a slab whose x-face stands a hair past the round's tangent line x = 2 (where the round
/// meets the flat face), is one closed manifold solid that checks clean at Level2. The
/// offsets 2e-5 and 2.5e-5 are where the vertex the join leaves at the tangent line used to
/// stand off the slab's face by more than the intersection tolerance; 1e-5 and 1e-4 are
/// either side of them and joined before.
#[test]
fn a_slab_a_hair_past_a_fillets_tangent_line_joins_valid() {
    use solidcore::blend::fillet_executive::api::fillet_edges;
    use solidcore::topology::{edge, vertex};

    let mut failures = Vec::new();
    // (offset, vertices): the kernel's counts; one more where the slab clears the tangent line
    for (dx, want_vertices) in [(1e-5, 17), (2e-5, 17), (2.5e-5, 17), (1e-4, 18)] {
        let mut k = Kernel::default();
        let mut block =
            api::create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
        let corner_edge = block
            .edges()
            .ids()
            .find(|&e| {
                let Ok((Some(s), Some(t))) = edge::start_end_vertices(&block, e) else {
                    return false;
                };
                let (p, q) = (
                    vertex::point(&block, s).unwrap(),
                    vertex::point(&block, t).unwrap(),
                );
                p.x.abs() < 1e-9 && p.y.abs() < 1e-9 && q.x.abs() < 1e-9 && q.y.abs() < 1e-9
            })
            .unwrap();
        fillet_edges(&mut k, &mut block, &[corner_edge], 2.0, 1, 1, 1.0).unwrap();
        let slab =
            api::create_box(&mut k, Point3::new(2.0 + dx, -5.0, 0.0), 6.0, 10.0, 5.0).unwrap();
        let mut brep = match boolean_union(&mut k, block, slab) {
            Ok(b) => b,
            Err(e) => {
                failures.push(format!("dx {dx:e}: join refused: {e}"));
                continue;
            }
        };
        let check = check_brep(&mut k, &mut brep, CheckLevel::Level2).unwrap();
        if !check.is_valid() {
            failures.push(format!("dx {dx:e}: Level2 {:?}", check.lines()));
        }
        if !solidcore::topology::brep::p2::is_manifold_solid(&brep).unwrap() {
            failures.push(format!("dx {dx:e}: not a closed manifold solid"));
        }
        // the box less the round's 10 (4 - pi) plus the slab's 150 outside the box
        let want_volume = 1110.0 + 10.0 * PI;
        let store = AttributeStore::new();
        let key_of = |_| ObjectKey(0);
        let attrs = PropertyAttributes {
            store: &store,
            key_of: &key_of,
        };
        let volume = brep_volume(&k, &mut brep, 1e-7, &attrs).unwrap();
        if (volume - want_volume).abs() > VOLUME_REL_TOL * want_volume {
            failures.push(format!("dx {dx:e}: volume {volume} vs {want_volume}"));
        }
        // eleven faces; a closed genus 0 body has V - E + F = 2
        let faces = solidcore::topology::brep::p1::faces(&brep, None)
            .unwrap()
            .len();
        let vertices = brep.vertices().ids().count();
        let edges = brep.edges().ids().count();
        if (faces, vertices, edges) != (11, want_vertices, want_vertices + 9) {
            failures.push(format!(
                "dx {dx:e}: {faces} faces, {vertices} vertices, {edges} edges"
            ));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// A through bore whose cap is almost coincident with a cylinder's cap. The two
/// cylinder seams differ, and the bore axis tilts by 1e-5 radians. Both bore caps
/// lie outside the stock, so every slice normal to the stock axis loses the same
/// ellipse: area pi * r^2 / cos(theta).
#[test]
fn a_nearly_axial_bore_through_a_cylinder_with_step_tolerances() {
    use solidcore::curve::ellipse::Ellipse;
    use solidcore::topology::brep::{p1, p2, p5};

    let mut k = rig_kernel();
    let stock_frame = Axis2Placement::new(
        Point3::new(-22.0005005683, 27.3286637536, -43.3425576331),
        Vec3::new(
            0.33541020646821096,
            -0.4447535182170257,
            -0.8304783569910354,
        ),
        Vec3::new(
            0.7984072189295407,
            0.6021178562052287,
            -1.2617029737649332e-7,
        ),
    );
    let bore_frame = Axis2Placement::new(
        Point3::new(-22.000499999972, 27.328699999997, -43.342599999983),
        Vec3::new(0.459987304432186, 0.748373023696032, 0.4778592859464696),
        Vec3::new(-0.7337328686846232, 0.01728700041769954, 0.6792181071116936),
    );
    let mut stock = api::create_cylinder(&mut k, Point3::ZERO, 3.75, 11.0).unwrap();
    p5::transform(&mut k, &mut stock, &stock_frame, None, false).unwrap();
    // STEP's 0.005 mm intersection tolerance is a 0.0025 mm kernel zone.
    p2::set_tolerance(&k, &mut stock, 0.0025, true, true).unwrap();
    let circle =
        Curve::from(Ellipse::create_canonical_circle(&k, &bore_frame, 2.4, None, None).unwrap());
    let mut bore = create_linear_sweep(
        &mut k,
        &[Some(&circle)],
        Vec3::new(0.5000477548925153, -0.6630527709169285, 0.5570576862465307),
        12.9998,
        true,
    )
    .unwrap();
    p2::set_tolerance(&k, &mut bore, 1e-5, true, true).unwrap();
    for operand in [&mut stock, &mut bore] {
        solidcore::topology::integrity::check(operand).unwrap();
        let check = check_brep(&mut k, operand, CheckLevel::Level2).unwrap();
        assert!(check.is_valid(), "input: {:?}", check.lines());
    }

    let mut result = boolean_difference(&mut k, stock, bore).unwrap();
    solidcore::topology::integrity::check(&result).unwrap();
    let check = check_brep(&mut k, &mut result, CheckLevel::Level2).unwrap();
    assert!(check.is_valid(), "{:?}", check.lines());
    assert_eq!(p1::faces(&result, None).unwrap().len(), 4);
    // Edges: the stock seam and its two cap circles (each closed at the seam), the bore
    // seam, and each bore circle as two arcs: the circle is a closed curve that starts
    // 0.82 mm from where the bore seam meets the cap, so it is cut there and at the
    // seam crossing. Vertices: two on the stock seam, two per bore circle.
    assert_eq!(p1::edges(&result, None).unwrap().len(), 8);
    assert_eq!(p1::vertices(&result, None).unwrap().len(), 6);

    let store = AttributeStore::new();
    let key_of = |_| ObjectKey(0);
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    let volume = brep_volume(&k, &mut result, 1e-9, &attrs).unwrap();
    let expected = PI * 11.0 * (3.75_f64.powi(2) - 2.4_f64.powi(2) / 1e-5_f64.cos());
    assert!(
        (volume - expected).abs() / expected < VOLUME_REL_TOL,
        "volume {volume} vs {expected}"
    );
}

// ---- a pocket whose sides are tangent to a bore, a hair past the bore's axis --------------

/// #17098, with the report's numbers: a block with a bore of radius 4.5 along z through it,
/// cut by a pocket 2 deep at the block's end face `z = 26` (its end 1.5 micron short of the
/// face), from the block's bottom face to 0.005 above the bore's axis, whose two sides are
/// tangent to the bore. At the end face, the bore's circle and each of the pocket's edges
/// along a side run 0.005 side by side from the tangent point to where the pocket's top
/// meets the circle, 2.8 micron apart there, within the edges' tolerance. The boolean finds
/// each such stretch twice: as the circle's arc against the pocket's end face, and as the
/// pocket's edge against the block's end face. The second made a second edge in the block's
/// end face beside the arc, and the pocket's edge, related to the arc, was asked for it too:
/// "another entity is already related to the other one". On the side where the bore's seam
/// is the two are definitions of one proto edge; on the other, of two proto edges between
/// the same proto vertices.
#[test]
fn a_pocket_tangent_to_a_bore_just_past_its_axis_cuts() {
    let (cx, cy, r) = (-26.724_830_725_384_454, 4.363_269_403_785_069, 4.5);
    let bottom = -2.311_730_596_423_852;
    let (x0, x1) = (-31.224_830_725_366_328, -22.224_830_725_456_723);
    let (top, z1) = (bottom + 6.68, 25.999_998_476);
    let (height, length) = (14.55, 26.0);

    let mut k = rig_kernel();
    let block = api::create_box(
        &mut k,
        Point3::new(cx - 18.0, bottom, 0.0),
        36.0,
        height,
        length,
    )
    .unwrap();
    let bore = api::create_cylinder(&mut k, Point3::new(cx, cy, -1.0), r, length + 2.0).unwrap();
    let block = boolean_difference(&mut k, block, bore).unwrap();
    let pocket = api::create_box(
        &mut k,
        Point3::new(x0, bottom, z1 - 2.0),
        x1 - x0,
        top - bottom,
        2.0,
    )
    .unwrap();
    let mut result = boolean_difference(&mut k, block, pocket).unwrap();
    let check = check_brep(&mut k, &mut result, CheckLevel::Level2).unwrap();
    assert!(check.is_valid(), "Level2 {:?}", check.lines());
    // the pocket takes its box less the bore's part of it, the circle below the chord `h`
    // above its centre
    let h = top - cy;
    let below = PI * r * r / 2.0 + h * (r * r - h * h).sqrt() + r * r * (h / r).asin();
    let pocket_material = ((x1 - x0) * (top - bottom) - below) * 2.0;
    let want = 36.0 * height * length - PI * r * r * length - pocket_material;
    let got = volume_of(&k, &mut result);
    assert!(
        (got - want).abs() <= VOLUME_REL_TOL * want,
        "volume {got} vs {want}"
    );
}

// ---- a slot cut across a hole tangent to the face it starts on -------------------------

/// The block of #16937: the 40 x 20 rectangle `x` in `[-40, 0]`, `y` in `[-10, 10]` less
/// the circle of radius 8 about `(-20, -2)`, which touches the side `y = -10` at
/// `(-20, -10)`, swept 60 along z. Its profile is one loop that runs along that side to the
/// tangent point, round the circle and on along the side, as a sketch's region is, so the
/// block pinches to no thickness along the line `x = -20`, `y = -10`, where it has two
/// edges between the same two vertices: the edge between the side's two parts and the seam
/// of the hole's wall. The block checks clean at Level2.
fn block_with_a_tangent_hole(k: &mut Kernel) -> solidcore::Result<Brep> {
    use solidcore::primitive::api_curves as curves;
    let p = |x: f64, y: f64| Point3::new(x, y, 0.0);
    let line = |a: Point3, b: Point3| Curve::from(curves::create_line_segment(a, b).unwrap());
    let profile = [
        line(p(-40.0, -10.0), p(-20.0, -10.0)),
        Curve::from(curves::create_arc(p(-20.0, -2.0), 8.0, -90.0, 270.0)?),
        line(p(-20.0, -10.0), p(0.0, -10.0)),
        line(p(0.0, -10.0), p(0.0, 10.0)),
        line(p(0.0, 10.0), p(-40.0, 10.0)),
        line(p(-40.0, 10.0), p(-40.0, -10.0)),
    ];
    let refs: Vec<Option<&Curve>> = profile.iter().map(Some).collect();
    create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 60.0, true)
}

/// #16937: a slot cut from the block's side `y = -10`, 18 deep, 8 high (`z` in `[24, 32]`)
/// and past both of its ends. The cutter's face on that side is coplanar with the side on
/// both sides of the pinch line and tangent to the wall along it, so its one edge along the
/// line was defined against both of the block's edges there and related to both: "another entity is already
/// related to the other one". The hole lies within the slot's depth, so the cut takes the
/// slot's whole box out of the block, hole and all; the block keeps the pinch above and
/// below the slot.
#[test]
fn a_slot_across_a_hole_tangent_to_its_face_cuts() {
    let mut k = rig_kernel();
    let mut block = block_with_a_tangent_hole(&mut k).unwrap();
    let block_volume = volume_of(&k, &mut block);
    let want_block = 40.0 * 20.0 * 60.0 - PI * 64.0 * 60.0;
    assert!(
        (block_volume - want_block).abs() <= VOLUME_REL_TOL * want_block,
        "block volume {block_volume} vs {want_block}"
    );

    let slot = api::create_box(&mut k, Point3::new(-50.0, -10.0, 24.0), 60.0, 18.0, 8.0).unwrap();
    let mut result = boolean_difference(&mut k, block, slot).unwrap();
    // the slot's box within the block, less the hole's part of it, is gone
    let want = want_block - 8.0 * (40.0 * 18.0 - PI * 64.0);
    let check = check_brep(&mut k, &mut result, CheckLevel::Level2).unwrap();
    assert!(check.is_valid(), "Level2 {:?}", check.lines());
    let got = volume_of(&k, &mut result);
    assert!(
        (got - want).abs() <= VOLUME_REL_TOL * want,
        "volume {got} vs {want}"
    );
}
