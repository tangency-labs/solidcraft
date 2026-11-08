//! The public boolean API end to end: seven primitive booleans (through-hole, overlapping
//! union, sphere/box intersection, T-pipe, coplanar union, tangent union, torus minus box)
//! each succeed, give a closed manifold solid, and integrate to the analytic volume
//! (which the C++ SMLib reference also reproduces), with SMLib's face count. Unions of
//! two solids touching at one point, on a seam or off it, also check clean at Level2.

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
