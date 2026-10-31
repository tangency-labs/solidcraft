//! Copies and affine transforms on solidcore, against closed forms: every result keeps its counts,
//! its volume scales by the matrix's |det|, its bounds are the moved box's, and it exports to a
//! record that validates. With SMLib built, the same case on the C++ kernel gives the same
//! volume, counts, bounds and faces by surface kind, or the same error (parity only: the
//! expected values are the analytic ones).

use std::collections::BTreeMap;
use std::f64::consts::PI;

use super::transform::{copy, transform};
use super::{Body, bounds, counts, make_box, make_cylinder, to_arrays, volume};
use crate::Error;
use crate::export::native;

/// The identity, as the capi reads a matrix: rows 0-2 the images of x, y and z, row 3 the move.
const ID: [[f64; 4]; 4] = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

#[derive(Clone, Copy, Debug)]
enum Shape {
    /// A box from its lower corner.
    Box([f64; 3], [f64; 3]),
    /// A cylinder about +Z from its base centre.
    Cylinder([f64; 3], f64, f64),
}

impl Shape {
    fn core(self) -> Body {
        match self {
            Shape::Box(o, s) => make_box(o, s).unwrap(),
            Shape::Cylinder(o, r, h) => make_cylinder(o, r, h).unwrap(),
        }
    }

    fn volume(self) -> f64 {
        match self {
            Shape::Box(_, s) => s[0] * s[1] * s[2],
            Shape::Cylinder(_, r, h) => PI * r * r * h,
        }
    }

    fn counts(self) -> [usize; 3] {
        match self {
            Shape::Box(..) => [6, 12, 8],
            Shape::Cylinder(..) => [3, 3, 2],
        }
    }

    /// The corners of its bounding box.
    fn corners(self) -> Vec<[f64; 3]> {
        let (lo, hi) = match self {
            Shape::Box(o, s) => (o, [o[0] + s[0], o[1] + s[1], o[2] + s[2]]),
            Shape::Cylinder(o, r, h) => ([o[0] - r, o[1] - r, o[2]], [o[0] + r, o[1] + r, o[2] + h]),
        };
        (0..8)
            .map(|k| [if k & 1 == 0 { lo[0] } else { hi[0] }, if k & 2 == 0 { lo[1] } else { hi[1] }, if k & 4 == 0 { lo[2] } else { hi[2] }])
            .collect()
    }
}

/// `p·A + t`.
fn apply(m: &[[f64; 4]; 4], p: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|j| p[0] * m[0][j] + p[1] * m[1][j] + p[2] * m[2][j] + m[3][j])
}

fn det(m: &[[f64; 4]; 4]) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

fn rotation_z(a: f64) -> [[f64; 4]; 4] {
    let (s, c) = a.sin_cos();
    [[c, s, 0.0, 0.0], [-s, c, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]
}

struct Case {
    shape: Shape,
    matrix: [[f64; 4]; 4],
    /// `None`: the closed form (volume times |det|, the moved box, the same counts).
    fails: Option<Error>,
}

const BOX: Shape = Shape::Box([1.0, 2.0, 3.0], [10.0, 20.0, 30.0]);
const CYL: Shape = Shape::Cylinder([0.0, 0.0, 0.0], 5.0, 12.0);

fn case(name: &str) -> Case {
    let ok = |shape, matrix| Case { shape, matrix, fails: None };
    let mut moved = ID;
    moved[3] = [5.0, -7.0, 11.0, 1.0];
    match name {
        "identity" => ok(BOX, ID),
        "translation" => ok(BOX, moved),
        "rotation_about_z" => ok(BOX, rotation_z(PI / 2.0)),
        "uniform_scale" => ok(BOX, [[2.0, 0.0, 0.0, 0.0], [0.0, 2.0, 0.0, 0.0], [0.0, 0.0, 2.0, 0.0], [1.0, 1.0, 1.0, 1.0]]),
        "mirror_in_x" => ok(BOX, [[-1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0; 4]]),
        "non_uniform_scale" => ok(BOX, [[2.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 0.5, 0.0], [0.0; 4]]),
        "shear" => ok(BOX, [[1.0, 0.5, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [2.0, 3.0, 4.0, 1.0]]),
        "cylinder_moved" => ok(CYL, moved),
        "cylinder_rotated" => ok(CYL, rotation_z(0.3)),
        "cylinder_mirrored" => ok(CYL, [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, -1.0, 0.0], [0.0; 4]]),
        "cylinder_scaled" => ok(CYL, [[3.0, 0.0, 0.0, 0.0], [0.0, 3.0, 0.0, 0.0], [0.0, 0.0, 3.0, 0.0], [0.0; 4]]),
        _ => panic!("no case {name}"),
    }
}

#[derive(Debug, PartialEq)]
struct Measured {
    volume: f64,
    bounds: ([f64; 3], [f64; 3]),
    counts: [usize; 3],
    kinds: BTreeMap<String, usize>,
}

type Got = std::result::Result<Measured, Error>;

fn kinds(rec: &solidcraft_geom::brep::Brep) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for f in &rec.faces {
        let v = serde_json::to_value(&f.surface).unwrap();
        *out.entry(v["kind"].as_str().unwrap_or("?").to_owned()).or_default() += 1;
    }
    out
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs()
}

/// The moved shape's bounding box: a box's moved corners bound it exactly; a cylinder's only for
/// the matrices here (a rotation about its axis, a move, a mirror in z, a uniform scale), whose
/// moved circle's box is the moved square's turned back to the axes.
fn expected_bounds(c: &Case) -> ([f64; 3], [f64; 3]) {
    let pts: Vec<[f64; 3]> = match c.shape {
        Shape::Cylinder(o, r, h) => {
            let s = (c.matrix[0][0] * c.matrix[0][0] + c.matrix[0][1] * c.matrix[0][1]).sqrt();
            let centre = [apply(&c.matrix, o), apply(&c.matrix, [o[0], o[1], o[2] + h])];
            centre.iter().flat_map(|p| [[p[0] - r * s, p[1] - r * s, p[2]], [p[0] + r * s, p[1] + r * s, p[2]]]).collect()
        }
        s => s.corners().into_iter().map(|p| apply(&c.matrix, p)).collect(),
    };
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in pts {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    (lo, hi)
}

fn judge(name: &str, who: &str, got: Got) -> Got {
    let c = case(name);
    match (&c.fails, &got) {
        (Some(e), Err(g)) => assert_eq!(g, e, "{name}: {who} failed otherwise"),
        (Some(e), Ok(m)) => panic!("{name}: {who} gave {m:?}, expected {e:?}"),
        (None, Err(g)) => panic!("{name}: {who} failed: {g}"),
        (None, Ok(m)) => {
            let v = c.shape.volume() * det(&c.matrix).abs();
            assert!(rel(m.volume, v) < 1e-9, "{name}: {who} volume {} vs analytic {v}", m.volume);
            assert_eq!(m.counts, c.shape.counts(), "{name}: {who} counts");
            let (lo, hi) = expected_bounds(&c);
            let near = |a: [f64; 3], b: [f64; 3]| a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-6);
            assert!(near(m.bounds.0, lo) && near(m.bounds.1, hi), "{name}: {who} bounds {:?}, expected {lo:?} {hi:?}", m.bounds);
        }
    }
    got
}

fn solidcore_case(name: &str) -> Got {
    let c = case(name);
    let got = transform(&c.shape.core(), &c.matrix).map(|b| {
        let (rec, _) = native(&to_arrays(&b).unwrap()).unwrap_or_else(|e| panic!("{name}: record: {e:?}"));
        rec.validate().unwrap_or_else(|e| panic!("{name}: record: {e}"));
        Measured { volume: volume(&b, 1e-8).unwrap(), bounds: bounds(&b).unwrap(), counts: counts(&b).unwrap(), kinds: kinds(&rec) }
    });
    judge(name, "solidcore", got)
}

/// The case on the C++ kernel, called directly (whatever the routing).
#[cfg(feature = "native")]
fn cpp_case(name: &str) -> Got {
    use crate::{Brep, error, ffi};
    crate::init().unwrap();
    let c = case(name);
    let body = match c.shape {
        Shape::Box(o, s) => ffi::make_box(&o, &s).unwrap(),
        Shape::Cylinder(o, r, h) => ffi::make_cylinder(&o, r, h).unwrap(),
    };
    let got = ffi::transform(&body, &c.matrix).map_err(error).map(|h| {
        let volume = ffi::volume(&h, 1e-8).unwrap();
        let n = ffi::counts(&h).unwrap().map(|x| usize::try_from(x).unwrap());
        let bx = ffi::measure::bounds(&h).unwrap();
        let (rec, _) = native(&Brep::made(h).cpp_arrays().unwrap()).unwrap();
        rec.validate().unwrap();
        Measured { volume, bounds: bx, counts: n, kinds: kinds(&rec) }
    });
    judge(name, "C++", got)
}

/// solidcore and C++ agree: volume within 1e-9, the same counts, bounds and faces by kind, or the
/// same error.
#[cfg(feature = "native")]
fn parity(name: &str) {
    let (ours, cpp) = (solidcore_case(name), cpp_case(name));
    match (ours, cpp) {
        (Ok(a), Ok(b)) => {
            assert!(rel(a.volume, b.volume) < 1e-9, "{name}: solidcore {} vs C++ {}", a.volume, b.volume);
            assert_eq!(a.counts, b.counts, "{name}: counts (solidcore vs C++)");
            assert_eq!(a.kinds, b.kinds, "{name}: faces by surface kind (solidcore vs C++)");
            let near = |x: [f64; 3], y: [f64; 3]| x.iter().zip(&y).all(|(p, q)| (p - q).abs() < 1e-6);
            assert!(near(a.bounds.0, b.bounds.0) && near(a.bounds.1, b.bounds.1), "{name}: bounds {:?} vs C++ {:?}", a.bounds, b.bounds);
        }
        (a, b) => assert_eq!(a.err(), b.err(), "{name}: errors (solidcore vs C++)"),
    }
}

macro_rules! cases {
    ($($m:ident $(: $ign:meta)?),* $(,)?) => {$(
        mod $m {
            #[test]
            $(#[$ign])?
            fn solidcore_matches_the_closed_form() {
                let _ = super::solidcore_case(stringify!($m));
            }
            #[cfg(feature = "native")]
            #[test]
            $(#[$ign])?
            fn solidcore_matches_the_cpp_kernel() {
                super::parity(stringify!($m));
            }
            #[cfg(feature = "native")]
            #[test]
            fn the_cpp_kernel_matches_the_closed_form() {
                let _ = super::cpp_case(stringify!($m));
            }
        }
    )*};
}

cases!(
    identity,
    translation,
    rotation_about_z,
    uniform_scale,
    mirror_in_x,
    non_uniform_scale,
    shear,
    cylinder_moved,
    cylinder_rotated,
    cylinder_mirrored,
    cylinder_scaled,
);

/// The capi's argument check, before any kernel call: every entry finite.
#[test]
fn hostile_matrices_are_refused() {
    let b = BOX.core();
    for (i, j) in [(0, 0), (1, 2), (3, 1), (2, 3)] {
        for x in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut m = ID;
            m[i][j] = x;
            assert_eq!(transform(&b, &m).err(), Some(Error::Argument), "m[{i}][{j}] = {x}");
        }
    }
}

/// A singular or huge matrix never panics: it gives a body or an error.
#[test]
fn degenerate_matrices_do_not_crash() {
    let b = BOX.core();
    let zero = [[0.0; 4]; 4];
    let flat = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0], [0.0; 4]];
    let huge = [[1e300, 0.0, 0.0, 0.0], [0.0, 1e300, 0.0, 0.0], [0.0, 0.0, 1e300, 0.0], [0.0; 4]];
    for m in [zero, flat, huge] {
        let _ = transform(&b, &m);
    }
}

/// The body moved, and the body copied, are never changed.
#[test]
fn the_body_stays_as_it_was() {
    let b = BOX.core();
    let moved = transform(&b, &rotation_z(1.0)).unwrap();
    let c = copy(&b).unwrap();
    drop(moved);
    for x in [&b, &c] {
        assert_eq!(counts(x).unwrap(), [6, 12, 8]);
        assert!(rel(volume(x, 1e-8).unwrap(), 6000.0) < 1e-12);
        assert_eq!(bounds(x).unwrap(), ([1.0, 2.0, 3.0], [11.0, 22.0, 33.0]));
    }
}

/// Face and edge tags (`Brep::tagged`) through a copy and a transform routed to solidcore, as on
/// C++: the box is tagged on the C++ kernel, its solidcore body converted from that and tagged the
/// same way, and the two results' histories must be the same.
#[cfg(feature = "native")]
mod history {
    use crate::{Brep, ffi};

    fn tagged_box() -> Brep {
        crate::init().unwrap();
        Brep::made(ffi::make_box(&[0.0; 3], &[20.0, 30.0, 10.0]).unwrap()).tagged(0).unwrap()
    }

    #[test]
    fn tags_cross_a_copy() {
        let a = tagged_box();
        let cpp = Brep::made(ffi::copy(a.cpp().unwrap()).unwrap()).history().unwrap();
        let core = Brep::made_core(super::copy(a.core().unwrap()).unwrap()).history().unwrap();
        assert_eq!(core.canonical(), cpp.canonical());
    }

    #[test]
    fn tags_cross_a_mirror_and_a_shear() {
        let a = tagged_box();
        let mirror = [[-1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0; 4]];
        let shear = [[1.0, 0.5, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [2.0, 3.0, 4.0, 1.0]];
        for m in [mirror, shear] {
            let cpp = Brep::made(ffi::transform(a.cpp().unwrap(), &m).unwrap()).history().unwrap();
            let core = Brep::made_core(super::transform(a.core().unwrap(), &m).unwrap()).history().unwrap();
            assert_eq!(core.canonical(), cpp.canonical(), "{m:?}");
        }
    }
}
