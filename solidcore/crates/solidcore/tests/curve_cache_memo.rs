//! The curve cache memo (`Kernel::memoize_curve_caches`) is invisible: booleans of
//! primitives and of two lofted skins give bit-identical results with the memo on and
//! off (every value of the result B-rep, its volume and any error), and a second run on
//! the same kernel, which finds the memo filled, gives the same again.

use solidcore::boolean::api_brep::{boolean_difference, boolean_intersection, boolean_union};
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::crv_fit::p1::ParamMethod;
use solidcore::primitive::api_primitives as api;
use solidcore::sweep::api_surfaces::create_skin_primitive;
use solidcore::topology::Brep;
use solidcore::Kernel;

#[derive(Clone, Copy)]
enum Op {
    Union,
    Intersection,
    Difference,
}

/// A closed cubic section through `n` points of an ellipse at height `z`.
fn section(a: f64, b: f64, z: f64, n: usize) -> solidcore::Result<Curve> {
    let mut pts: Vec<Point3> = (0..n)
        .map(|i| {
            let t = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
            Point3::new(a * t.cos(), b * t.sin(), z)
        })
        .collect();
    pts.extend(pts.first().copied());
    BSplineCurve::interpolate_points(&pts, None, 3, None, None, true, ParamMethod::ChordLength)
        .map(Curve::from)
}

/// A capped ruled skin through `count` elliptic sections from `z0` to `z1`.
fn skin(k: &mut Kernel, a: f64, b: f64, z0: f64, z1: f64, count: usize) -> solidcore::Result<Brep> {
    let cs = (0..count)
        .map(|i| {
            let s = i as f64 / (count - 1) as f64;
            // the sections narrow toward the top, so the skin is not a cylinder
            section(
                a * (1.0 - 0.3 * s),
                b * (1.0 - 0.2 * s),
                z0 + s * (z1 - z0),
                12,
            )
        })
        .collect::<solidcore::Result<Vec<Curve>>>()?;
    let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
    let counts = vec![1usize; cs.len()];
    create_skin_primitive(k, &counts, &refs, 1, true).map(|s| s.brep)
}

/// A kernel with the memo on or off (whatever the `curve-memo-off` feature says).
fn kernel(memo: bool) -> Kernel {
    Kernel {
        memoize_curve_caches: memo,
        ..Kernel::default()
    }
}

fn operands(k: &mut Kernel, name: &str) -> solidcore::Result<(Brep, Brep, Op)> {
    let p = Point3::new;
    Ok(match name {
        "box-cyl-hole" => (
            api::create_box(k, p(0.0, 0.0, 0.0), 10.0, 10.0, 10.0)?,
            api::create_cylinder(k, p(5.0, 5.0, -1.0), 2.0, 12.0)?,
            Op::Difference,
        ),
        "sphere-and-box" => (
            api::create_sphere(k, p(0.0, 0.0, 0.0), 5.0)?,
            api::create_box(k, p(-10.0, -10.0, -10.0), 20.0, 20.0, 12.0)?,
            Op::Intersection,
        ),
        "torus-minus-box" => (
            api::create_torus(k, p(0.0, 0.0, 0.0), 5.0, 1.5)?,
            api::create_box(k, p(0.0, -10.0, -10.0), 20.0, 20.0, 20.0)?,
            Op::Difference,
        ),
        "cyl-union-cyl" => (
            api::create_cylinder(k, p(0.0, 0.0, 0.0), 3.0, 10.0)?,
            api::create_cylinder(k, p(2.0, 0.0, 2.0), 2.0, 10.0)?,
            Op::Union,
        ),
        "skin-minus-skin" => (
            skin(k, 20.0, 12.0, 0.0, 40.0, 5)?,
            skin(k, 18.0, 10.0, 2.0, 38.0, 5)?,
            Op::Difference,
        ),
        "skin-and-box" => (
            skin(k, 20.0, 12.0, 0.0, 40.0, 5)?,
            api::create_box(k, p(-30.0, -30.0, 10.0), 60.0, 60.0, 20.0)?,
            Op::Intersection,
        ),
        // analytic surfaces against B-spline surfaces (both memoise their boundaries)
        "skin-minus-cyl" => (
            skin(k, 20.0, 12.0, 0.0, 40.0, 5)?,
            api::create_cylinder(k, p(0.0, 0.0, -5.0), 4.0, 50.0)?,
            Op::Difference,
        ),
        "skin-and-sphere" => (
            skin(k, 20.0, 12.0, 0.0, 40.0, 5)?,
            api::create_sphere(k, p(5.0, 0.0, 20.0), 12.0)?,
            Op::Intersection,
        ),
        _ => unreachable!("unknown case {name}"),
    })
}

const CASES: [&str; 8] = [
    "box-cyl-hole",
    "sphere-and-box",
    "torus-minus-box",
    "cyl-union-cyl",
    "skin-minus-skin",
    "skin-and-box",
    "skin-minus-cyl",
    "skin-and-sphere",
];

/// Every observable output of one case as text (`{:?}` prints each `f64` so that it
/// parses back to the same bits).
fn observe(k: &mut Kernel, name: &str) -> String {
    let (a, b, op) = match operands(k, name) {
        Ok(o) => o,
        Err(e) => return format!("operands: {e:?}"),
    };
    let result = match op {
        Op::Union => boolean_union(k, a, b),
        Op::Intersection => boolean_intersection(k, a, b),
        Op::Difference => boolean_difference(k, a, b),
    };
    let mut brep = match result {
        Ok(b) => b,
        Err(e) => return format!("boolean: {e:?}"),
    };
    if let Err(e) = solidcore::topology::integrity::check(&brep) {
        return format!("integrity: {e:?}");
    }
    let store = AttributeStore::new();
    let key_of = |_| ObjectKey(0);
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    let volume = brep_volume(k, &mut brep, 1e-6, &attrs);
    format!("{volume:?}\n{brep:?}\n{:?}", k.upstream_ub_passed)
}

#[test]
fn memo_on_and_off_give_bit_identical_results() {
    let mut differ = Vec::new();
    for name in CASES {
        let mut off = kernel(false);
        let want = observe(&mut off, name);
        let mut on = kernel(true);
        let first = observe(&mut on, name);
        // the same kernel again: geometry made from the same inputs, fresh memos
        let second = observe(&mut on, name);
        let mut off_again = kernel(false);
        let _ = observe(&mut off_again, name);
        let second_off = observe(&mut off_again, name);
        // every case runs to a volume, so the comparison covers a whole boolean
        if !want.starts_with("Ok(") {
            differ.push(format!(
                "{name}: no result: {}",
                want.lines().next().unwrap_or("")
            ));
        }
        if first != want {
            differ.push(format!("{name}: memo on differs from memo off"));
        }
        if second != second_off {
            differ.push(format!("{name}: second run differs"));
        }
    }
    assert!(differ.is_empty(), "{differ:#?}");
}

#[test]
fn curves_and_breps_stay_send() {
    // `Brep` shares its edge arenas with a classification snapshot through an `Arc`,
    // which is `Send` only when the curves are `Sync` too: their interior caches
    // (`CrvOnSurf`'s breaks, the offset surface's distance and extension, the B-spline
    // surface's out-of-bounds flag) are atomics or mutexes, not `Cell`/`RefCell`
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    assert_send::<Curve>();
    assert_sync::<Curve>();
    assert_send::<Brep>();
}
