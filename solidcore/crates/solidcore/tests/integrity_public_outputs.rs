//! Every public entry point that returns or edits a B-rep leaves it intact: no live
//! entity names a dead one, and the port's cached topology fields (`Edge::vertices`,
//! `Face::shell`, `Face::loops`, `Loop::face`, `Loop::edges`, `Shell::faces`) agree with
//! the use links upstream reads them through ([`solidcore::topology::integrity::check`];
//! each entry point ends with `Brep::refresh_cached_topology`). One fresh output per
//! family, so an entry point that misses the refresh fails here.
//!
//! The entry points and where they refresh:
//! - `primitive::api_primitives`: box, plane, planar circle, pyramid, cylindrical box,
//!   sphere without poles, cone with elliptic ends directly; sphere, cylinder, cone,
//!   torus and the partial / swung / blend primitives through
//!   `PrimitiveModel::build_new`.
//! - `sweep::api_brep` (linear, rotational, draft, pipe, planar-path, curve sweeps,
//!   taper extrude, repetitions) and `sweep::api_surfaces` (skin primitive, skin from
//!   faces): `PrimitiveModel::build_new` and their own returns.
//! - `boolean::api_brep` (boolean, union, difference, intersection, merge, with options,
//!   merge B-reps, lists, trees).
//! - `topology::brep::p3::api_brep_copy` (of a Boolean union's result).
//! - `heal::heal_data::p1::api_heal_brep` (in place).
//! - `heal::stitch::p1` (`api_stitch` and its six siblings, in place):
//!   covered by a test that lands with the #855 stitch change.
//! - `offset::offset_executive::p2` (`api_shell_brep_full`, `api_offset_brep_full`):
//!   checked by the offset tests' shared fixture (`tests/support/offset_fixture.rs`).
//! - Not covered yet: the fillet API (`blend::fillet_executive::api`), after #1127.
//!   `api_transform` moves geometry only and leaves topology (and the fields) as they
//!   are; the query, curve and surface APIs make no B-rep.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::{boolean_difference, boolean_intersection, boolean_union};
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::ObjectKey;
use solidcore::geom::object::ObjectRef;
use solidcore::heal::heal_data::p1::api_heal_brep;
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::crv_fit::p1::ParamMethod;
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::primitive::api_primitives as prim;
use solidcore::sweep::api_brep::{create_linear_sweep, create_rotational_sweep};
use solidcore::sweep::api_surfaces::create_skin_primitive;
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::brep::p3;
use solidcore::topology::integrity::check;
use solidcore::{Brep, Kernel};

fn p(x: f64, y: f64, z: f64) -> Point3 {
    Point3::new(x, y, z)
}

/// A closed cubic through 12 points of an ellipse at height `z`.
fn section(z: f64, a: f64, b: f64) -> Curve {
    let mut pts: Vec<Point3> = (0..12)
        .map(|j| {
            let t = 2.0 * std::f64::consts::PI * f64::from(j) / 12.0;
            p(a * t.cos(), b * t.sin(), z)
        })
        .collect();
    pts.extend(pts.first().copied());
    Curve::from(
        BSplineCurve::interpolate_points(&pts, None, 3, None, None, true, ParamMethod::ChordLength)
            .unwrap(),
    )
}

/// A closed square profile in XY from (x0, 0) of side `s`.
fn square(x0: f64, s: f64) -> Vec<Curve> {
    let c = [
        p(x0, 0.0, 0.0),
        p(x0 + s, 0.0, 0.0),
        p(x0 + s, s, 0.0),
        p(x0, s, 0.0),
    ];
    (0..4)
        .map(|i| Curve::from(create_line_segment(c[i], c[(i + 1) % 4]).unwrap()))
        .collect()
}

/// `SmApiBrepCopy` of `b`, called as the SolidCraft difftest runner does (its `copy`):
/// source and copy named apart in one notification environment.
fn copy(k: &mut Kernel, b: &Brep) -> solidcore::Result<Brep> {
    let named = |entity, domain: u64| {
        let r = p3::topology_ref(0, entity);
        ObjectRef {
            type_code: r.type_code,
            key: ObjectKey(domain + r.type_code as u64 * 1_000_000 + (r.key.0 & 0xffff_ffff)),
        }
    };
    let target_key = |entity| named(entity, 100_000_000);
    let mut state = ArenaNotifyState::new(&Brep::new());
    let mut env = state.env(None);
    env.key_of = &target_key;
    p3::api_brep_copy(k, &mut env, Some(b), &|entity| named(entity, 0))
}

type Build = fn(&mut Kernel) -> solidcore::Result<Brep>;

#[test]
fn every_public_brep_output_passes_the_integrity_check() {
    let cases: Vec<(&str, Build)> = vec![
        ("box", |k| {
            prim::create_box(k, p(0.0, 0.0, 0.0), 1.0, 2.0, 3.0)
        }),
        ("cylinder", |k| {
            prim::create_cylinder(k, p(0.0, 0.0, 0.0), 1.0, 2.0)
        }),
        ("cone", |k| {
            prim::create_cone(k, p(0.0, 0.0, 0.0), 2.0, 1.0, 3.0)
        }),
        ("sphere", |k| prim::create_sphere(k, p(0.0, 0.0, 0.0), 2.0)),
        ("torus", |k| {
            prim::create_torus(k, p(0.0, 0.0, 0.0), 3.0, 1.0)
        }),
        ("pyramid", |k| {
            prim::create_pyramid(k, p(0.0, 0.0, 0.0), 2.0, 3.0)
        }),
        ("plane", |k| {
            prim::create_plane(k, p(0.0, 0.0, 0.0), 2.0, 3.0)
        }),
        ("planar circle", |k| {
            prim::create_planar_circle(k, p(0.0, 0.0, 0.0), 2.0)
        }),
        ("partial sphere", |k| {
            prim::create_partial_sphere(k, p(0.0, 0.0, 0.0), 2.0, 0.0, 90.0)
        }),
        ("partial cylinder", |k| {
            prim::create_partial_cylinder(k, p(0.0, 0.0, 0.0), 2.0, 3.0, 0.0, 120.0)
        }),
        ("cylindrical box", |k| {
            prim::create_cylindrical_box(k, p(0.0, 0.0, 0.0), 3.0, 1.0, 2.0, 0.0, 90.0)
        }),
        ("linear sweep", |k| {
            let cs = square(0.0, 1.0);
            let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
            create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 2.0, true)
        }),
        ("rotational sweep", |k| {
            let cs = square(2.0, 1.0);
            let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
            create_rotational_sweep(
                k,
                &refs,
                Point3::new(0.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                360.0,
                true,
            )
        }),
        ("revolved dome", |k| {
            let at = |u: f64, v: f64| Point3::new(u, v, 0.0);
            let cs = [
                Curve::from(create_line_segment(at(0.0, 0.0), at(5.0, 0.0)).unwrap()),
                Curve::from(create_arc(at(0.0, 0.0), 5.0, 0.0, 90.0).unwrap()),
                Curve::from(create_line_segment(at(0.0, 5.0), at(0.0, 0.0)).unwrap()),
            ];
            let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
            create_rotational_sweep(
                k,
                &refs,
                Point3::new(0.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                360.0,
                true,
            )
        }),
        ("skin primitive", |k| {
            let cs: Vec<Curve> = (0..3)
                .map(|i| section(10.0 * f64::from(i), 20.0 - f64::from(i), 12.0))
                .collect();
            let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
            create_skin_primitive(k, &[1; 3], &refs, 1, true).map(|s| s.brep)
        }),
        ("boolean union", |k| {
            let a = prim::create_box(k, p(0.0, 0.0, 0.0), 2.0, 2.0, 2.0)?;
            let b = prim::create_sphere(k, p(2.0, 1.0, 1.0), 1.0)?;
            boolean_union(k, a, b)
        }),
        ("boolean difference", |k| {
            let a = prim::create_box(k, p(0.0, 0.0, 0.0), 2.0, 2.0, 2.0)?;
            let b = prim::create_cylinder(k, p(1.0, 1.0, -1.0), 0.5, 4.0)?;
            boolean_difference(k, a, b)
        }),
        ("boolean intersection", |k| {
            let a = prim::create_box(k, p(0.0, 0.0, 0.0), 2.0, 2.0, 2.0)?;
            let b = prim::create_sphere(k, p(1.0, 1.0, 1.0), 1.3)?;
            boolean_intersection(k, a, b)
        }),
        // the copy path that copied a Boolean result's stale end pairs (the downstream
        // bug): the original is checked inside, the copy by the loop below
        ("boolean union, then api_brep_copy", |k| {
            let a = prim::create_box(k, p(0.0, 0.0, 0.0), 2.0, 2.0, 2.0)?;
            let b = prim::create_box(k, p(1.0, 1.0, 1.0), 2.0, 2.0, 2.0)?;
            let u = boolean_union(k, a, b)?;
            check(&u)?;
            copy(k, &u)
        }),
        ("heal", |k| {
            let mut b = prim::create_box(k, p(0.0, 0.0, 0.0), 1.0, 1.0, 1.0)?;
            api_heal_brep(k, Some(&mut b))?;
            Ok(b)
        }),
    ];
    let mut failures = Vec::new();
    for (name, build) in cases {
        let mut k = Kernel::default();
        match build(&mut k) {
            Ok(b) => {
                if let Err(e) = check(&b) {
                    failures.push(format!("{name}: {e:?}"));
                }
            }
            // a construction error is another test's business; the check needs a body
            Err(e) => failures.push(format!("{name}: no body ({e:?})")),
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
