//! The public boolean API end to end: seven primitive booleans (through-hole, overlapping
//! union, sphere/box intersection, T-pipe, coplanar union, tangent union, torus minus box)
//! each succeed, give a closed manifold solid, and integrate to the analytic volume
//! (which the C++ SMLib reference also reproduces), with SMLib's face count. Unions of
//! two solids touching at one point, on a seam or off it, also check clean at Level2. A body
//! cut by an exact copy of itself leaves nothing, with STEP-sized tolerances too.
#![allow(clippy::unwrap_used)] // test helpers

use solidcore::boolean::api_brep::{boolean_difference, boolean_intersection, boolean_union};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives as api;
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

/// A union of two solids touching at one point.
struct PointContact {
    name: &'static str,
    /// Analytic volume: the sum of the operands'.
    volume: f64,
    /// Face count: every face of both operands.
    faces: usize,
    /// The contact lies inside a seam edge: the result needs
    /// `UpstreamFixes::topological_vertex_has_only_edge_uses` (WR-727, #2520), as the
    /// cleanup joins the seam through the contact vertex and moves the other face's
    /// vertex loop to the seam's far end.
    on_seam_edge: bool,
}

fn point_contacts() -> Vec<PointContact> {
    let sphere = |r: f64| 4.0 / 3.0 * PI * r.powi(3);
    let torus = 2.0 * PI * PI * 5.0 * 1.5 * 1.5;
    let contact = |name, volume, faces, on_seam_edge| PointContact {
        name,
        volume,
        faces,
        on_seam_edge,
    };
    vec![
        // At the sphere's pole, a seam vertex (also in `cases`).
        contact("tangent-sphere-box", 1000.0 + sphere(3.0), 7, false),
        contact("seam-sphere-box", 1312.5 + sphere(7.5), 7, true),
        contact("seam-box-sphere", 1312.5 + sphere(7.5), 7, true),
        contact("off-seam-sphere-box", 1312.5 + sphere(7.5), 7, false),
        contact("seam-sphere-sphere", sphere(3.0) + sphere(2.0), 2, true),
        contact("seam-cylinder-sphere", 40.0 * PI + sphere(3.0), 4, true),
        contact(
            "off-seam-cylinder-sphere",
            40.0 * PI + sphere(3.0),
            4,
            false,
        ),
        contact("seam-torus-sphere", torus + sphere(2.0), 2, true),
        contact("seam-torus-box", torus + 144.0, 7, true),
        contact("seam-vertex-torus-box", torus + 144.0, 7, false),
    ]
}

/// The union of `case`, with `UpstreamFixes::topological_vertex_has_only_edge_uses` set
/// to `fix`: a closed manifold solid with every face of both operands, the analytic
/// volume and a clean Level2 check.
fn run_point_contact(case: &PointContact, fix: bool) -> Result<(), String> {
    let mut k = Kernel::default();
    k.fixes.topological_vertex_has_only_edge_uses = fix;
    let (a, b, _) = operands(&mut k, case.name).map_err(|e| format!("operands: {e}"))?;
    let mut brep = boolean_union(&mut k, a, b).map_err(|e| format!("boolean failed: {e}"))?;
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
// at a seam vertex or off the seam, unite into a body that checks clean at Level2.
#[test]
fn point_contact_unions_check_clean() {
    let failures: Vec<String> = point_contacts()
        .iter()
        .filter_map(|c| {
            run_point_contact(c, true)
                .err()
                .map(|e| format!("{}: {e}", c.name))
        })
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
}

// The contacts away from a seam edge need no fix: the pinned behaviour keeps them.
#[test]
fn point_contacts_off_seam_edges_check_clean_by_default() {
    let failures: Vec<String> = point_contacts()
        .iter()
        .filter(|c| !c.on_seam_edge)
        .filter_map(|c| {
            run_point_contact(c, false)
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
