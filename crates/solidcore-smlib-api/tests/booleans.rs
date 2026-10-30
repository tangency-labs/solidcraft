//! The booleans: argument checks, the empty set, touching solids, the pairwise union, and
//! two of the C API's regression cases.
//!
//! Source: tangency-labs/smlib `capi/tests/booleans.cpp` (Apache-2.0, Copyright SolidCraft
//! contributors), replay/1.0.1 at f8fd83e: `expect`, `sound`, `sealed_void_join`, `equal_rods_join`.
//! Its other cases need the flat arrays, lofts or sweeps (later families) and are left for
//! those.

#![allow(clippy::unwrap_used, clippy::panic)] // test helpers

use solidcore_smlib_api::{
    Brep, Smlib, Status, BOOLEAN_INTERSECT, BOOLEAN_SUBTRACT, BOOLEAN_UNION,
};

fn bx(api: &mut Smlib, lo: [f64; 3], hi: [f64; 3]) -> Brep {
    let [x0, y0, z0] = lo;
    let [x1, y1, z1] = hi;
    api.make_box(lo, [x1 - x0, y1 - y0, z1 - z0]).unwrap()
}

fn volume(api: &mut Smlib, b: &Brep) -> f64 {
    api.brep_volume(b, 1e-7).unwrap()
}

/// SMLib's own validity check, every test (level 2), finds nothing wrong with `b`.
fn sound(api: &mut Smlib, b: &Brep) -> bool {
    let (report, bad) = api.brep_check(b, 2).unwrap();
    if bad != 0 {
        eprintln!("  {bad} failed validity tests:\n{report}");
    }
    bad == 0
}

fn near(got: f64, want: f64, rel: f64) -> bool {
    let ok = (got - want).abs() <= rel * want.abs();
    if !ok {
        eprintln!("  got {got:.9e}, want {want:.9e}");
    }
    ok
}

/// `a op b` is one valid body of volume `want` (relative `rel`) and `solids` solids.
fn expect(
    api: &mut Smlib,
    what: &str,
    a: &Brep,
    b: &Brep,
    op: i32,
    want: f64,
    rel: f64,
    solids: i64,
) {
    let r = match api.boolean(a, b, op) {
        Ok(r) => r,
        Err(s) => panic!("{what}: status {s}"),
    };
    assert!(sound(api, &r), "{what}: check");
    let v = volume(api, &r);
    assert!(near(v, want, rel), "{what}: volume");
    assert_eq!(api.brep_solid_count(&r).unwrap(), solids, "{what}: solids");
}

#[test]
fn overlapping_boxes() {
    let mut api = Smlib::default();
    let a = bx(&mut api, [0.0; 3], [10.0; 3]);
    let b = bx(&mut api, [5.0; 3], [15.0; 3]);
    expect(
        &mut api,
        "union",
        &a,
        &b,
        BOOLEAN_UNION,
        2000.0 - 125.0,
        1e-9,
        1,
    );
    expect(
        &mut api,
        "subtract",
        &a,
        &b,
        BOOLEAN_SUBTRACT,
        1000.0 - 125.0,
        1e-9,
        1,
    );
    expect(
        &mut api,
        "intersect",
        &a,
        &b,
        BOOLEAN_INTERSECT,
        125.0,
        1e-9,
        1,
    );
    // The inputs are left as they were.
    assert!(near(volume(&mut api, &a), 1000.0, 1e-12));
    assert!(near(volume(&mut api, &b), 1000.0, 1e-12));
}

#[test]
fn rejected_arguments() {
    let mut api = Smlib::default();
    let a = bx(&mut api, [0.0; 3], [1.0; 3]);
    assert_eq!(api.boolean(&a, &a, 3).err(), Some(Status::ERR_ARG));
    assert_eq!(
        api.boolean_with(&a, &a, BOOLEAN_UNION, 2).err(),
        Some(Status::ERR_ARG)
    );
    assert_eq!(api.boolean2d(&a, &a, -1).err(), Some(Status::ERR_ARG));
    assert_eq!(api.boolean_union_all(&[]).err(), Some(Status::ERR_ARG));
}

/// Apart boxes have no common part: the empty set.
#[test]
fn disjoint_intersection_is_empty() {
    let mut api = Smlib::default();
    let a = bx(&mut api, [0.0; 3], [1.0; 3]);
    let b = bx(&mut api, [5.0; 3], [6.0; 3]);
    assert_eq!(
        api.boolean(&a, &b, BOOLEAN_INTERSECT).err(),
        Some(Status::ERR_EMPTY)
    );
}

/// Cubes sharing one edge: closed but not manifold there. `smlib_boolean` (with
/// `SMLIB_RESULT_NONMANIFOLD`) takes it; without the flag it is `SMLIB_ERR_NOT_SOLID`.
#[test]
fn cubes_touching_along_an_edge() {
    let mut api = Smlib::default();
    let a = bx(&mut api, [0.0; 3], [1.0; 3]);
    let b = bx(&mut api, [1.0, 1.0, 0.0], [2.0, 2.0, 1.0]);
    let r = api.boolean(&a, &b, BOOLEAN_UNION).unwrap();
    assert!(near(volume(&mut api, &r), 2.0, 1e-9));
    assert_eq!(api.brep_solid_count(&r).unwrap(), 2);
    assert_eq!(
        api.boolean_with(&a, &b, BOOLEAN_UNION, 0).err(),
        Some(Status::ERR_NOT_SOLID)
    );
}

/// Five apart cubes and one bridging two of them: four solids, pairwise level by level.
#[test]
fn union_all() {
    let mut api = Smlib::default();
    let mut cubes: Vec<Brep> = (0..5)
        .map(|i| {
            let x = 3.0 * f64::from(i);
            bx(&mut api, [x, 0.0, 0.0], [x + 1.0, 1.0, 1.0])
        })
        .collect();
    cubes.push(bx(&mut api, [0.5, 0.25, 0.25], [3.5, 0.75, 0.75]));
    let all: Vec<&Brep> = cubes.iter().collect();
    let r = api.boolean_union_all(&all).unwrap();
    assert!(sound(&mut api, &r));
    assert!(near(volume(&mut api, &r), 5.0 + 2.0 * 0.25, 1e-9));
    assert_eq!(api.brep_solid_count(&r).unwrap(), 4);
    let one = api.boolean_union_all(&all[..1]).unwrap();
    assert!(near(volume(&mut api, &one), 1.0, 1e-9));
}

/// A union that seals a void: a cube 100 hollowed to an open cup (walls and floor 10) and a
/// lid 100 x 100 x 10 over its top 10, so the cavity 80 x 80 x 80 closes. Either operand
/// order, and a cut of the sealed body after it. V = 100^3 - 80^3.
#[test]
fn sealed_void_join() {
    let mut api = Smlib::default();
    let cube = bx(&mut api, [0.0; 3], [100.0; 3]);
    let hollow = bx(&mut api, [10.0, 10.0, 10.0], [90.0, 90.0, 110.0]);
    let lid = bx(&mut api, [0.0, 0.0, 90.0], [100.0; 3]);
    let corner = bx(&mut api, [-1.0; 3], [5.0; 3]);
    let cup = api.boolean(&cube, &hollow, BOOLEAN_SUBTRACT).unwrap();
    let v = 100.0 * 100.0 * 100.0 - 80.0 * 80.0 * 80.0;
    expect(
        &mut api,
        "cup joined with its lid",
        &cup,
        &lid,
        BOOLEAN_UNION,
        v,
        1e-9,
        1,
    );
    expect(
        &mut api,
        "lid joined with its cup",
        &lid,
        &cup,
        BOOLEAN_UNION,
        v,
        1e-9,
        1,
    );
    let sealed = api.boolean(&cup, &lid, BOOLEAN_UNION).unwrap();
    let want = v - 5.0 * 5.0 * 5.0;
    expect(
        &mut api,
        "sealed cube less a corner",
        &sealed,
        &corner,
        BOOLEAN_SUBTRACT,
        want,
        1e-9,
        1,
    );
}

/// SMLib's cylinder of radius `r` and height `h`, its seam turned `seam_deg` about its own
/// axis, then its axis turned from +z by `theta_deg` about +y (90 gives +x) and its base
/// moved to `base`.
fn turned_cylinder(
    api: &mut Smlib,
    r: f64,
    h: f64,
    base: [f64; 3],
    theta_deg: f64,
    seam_deg: f64,
) -> Brep {
    let c = api.make_cylinder([0.0; 3], r, h).unwrap();
    let (ss, cs) = seam_deg.to_radians().sin_cos();
    let seam = [
        [cs, ss, 0.0, 0.0],
        [-ss, cs, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let seamed = api.brep_transform(&c, &seam).unwrap();
    let (s, k) = theta_deg.to_radians().sin_cos();
    let [bx0, by0, bz0] = base;
    let moved = [
        [k, 0.0, -s, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [s, 0.0, k, 0.0],
        [bx0, by0, bz0, 1.0],
    ];
    api.brep_transform(&seamed, &moved).unwrap()
}

/// A cylinder C r 44.45 along x (-200..200) and two rods r 24.15 at +-45 deg whose axes meet
/// at (0, 0, 30), joined (C u A) u B, with the seam at 0, 35, 60 and 90 degrees. Before the
/// fork's 1.0.1 replay the union dropped B (status OK, check clean).
#[test]
fn equal_rods_join() {
    let mut api = Smlib::default();
    let cb = [-200.0, 0.0, 0.0];
    let ab = [-28.284271247462, 0.0, 1.715728752538];
    let bb = [28.284271247462, 0.0, 1.715728752538];
    for seam in [0.0, 35.0, 60.0, 90.0] {
        let c = turned_cylinder(&mut api, 44.45, 400.0, cb, 90.0, seam);
        let a = turned_cylinder(&mut api, 24.15, 240.0, ab, 45.0, seam);
        let b = turned_cylinder(&mut api, 24.15, 240.0, bb, -45.0, seam);
        let ca = api.boolean(&c, &a, BOOLEAN_UNION).unwrap();
        let what = format!("(C u A) u B, seam {seam} deg");
        expect(
            &mut api,
            &what,
            &ca,
            &b,
            BOOLEAN_UNION,
            3137849.373374,
            1e-7,
            1,
        );
    }
}
