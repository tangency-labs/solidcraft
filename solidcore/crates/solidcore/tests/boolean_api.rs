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
