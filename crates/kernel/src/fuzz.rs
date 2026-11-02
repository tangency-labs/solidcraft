//! Robustness checks on random inputs (`solidcraft-cli fuzz`, docs/robustness.md): a small
//! seeded random generator, and the boolean volume identity on random pairs of primitives
//! placed on a coarse grid, so coincident faces and tangencies come up often.

use serde::Serialize;
use solidcraft_geom::Vec3;

use crate::{Body, BoolOp, boolean, box_solid, cylinder, measure, sphere, torus};

/// xorshift64*: small, fast, and the same everywhere (wasm included).
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        // Spread nearby seeds apart (splitmix64), never zero.
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        Rng((z ^ (z >> 31)) | 1)
    }
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    /// 0…n−1 (0 when n is 0).
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next_u64() % n as u64) as usize }
    }
    /// In [0, 1).
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }
    pub fn pick<T: Copy>(&mut self, xs: &[T]) -> Option<T> {
        xs.get(self.below(xs.len())).copied()
    }
    /// A multiple of `step` in [lo, hi].
    pub fn grid(&mut self, lo: f64, hi: f64, step: f64) -> f64 {
        let n = ((hi - lo) / step).floor().max(0.0) as usize;
        lo + step * self.below(n + 1) as f64
    }
}

/// A primitive used by the boolean checks, as plain data (so a failure can be written down).
#[derive(Clone, Debug, Serialize)]
pub enum Shape {
    Box { a: [f64; 3], b: [f64; 3] },
    Cylinder { base: [f64; 3], axis: [f64; 3], radius: f64, height: f64 },
    Sphere { center: [f64; 3], radius: f64 },
    Torus { center: [f64; 3], major: f64, minor: f64 },
}

impl Shape {
    pub fn body(&self) -> crate::Result<Body> {
        let v = |p: &[f64; 3]| Vec3::new(p[0], p[1], p[2]);
        match self {
            Shape::Box { a, b } => box_solid(v(a), v(b)),
            Shape::Cylinder { base, axis, radius, height } => cylinder(v(base), v(axis), *radius, *height),
            Shape::Sphere { center, radius } => sphere(v(center), *radius),
            Shape::Torus { center, major, minor } => torus(v(center), *major, *minor),
        }
    }

    fn random(r: &mut Rng) -> Shape {
        let g = 2.5;
        let mut p = || [r.grid(0.0, 30.0, g), r.grid(0.0, 30.0, g), r.grid(0.0, 30.0, g)];
        let at = p();
        match r.below(10) {
            0..=4 => {
                let size = [r.grid(5.0, 30.0, g), r.grid(5.0, 30.0, g), r.grid(5.0, 30.0, g)];
                Shape::Box { a: at, b: [at[0] + size[0], at[1] + size[1], at[2] + size[2]] }
            }
            5..=7 => {
                let axis = r.pick(&[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]).unwrap_or([0.0, 0.0, 1.0]);
                Shape::Cylinder { base: at, axis, radius: r.grid(2.5, 10.0, g), height: r.grid(5.0, 30.0, g) }
            }
            8 => Shape::Sphere { center: at, radius: r.grid(2.5, 12.5, g) },
            _ => {
                let minor = r.grid(2.5, 5.0, g);
                Shape::Torus { center: at, major: minor + r.grid(2.5, 10.0, g), minor }
            }
        }
    }
}

/// One boolean check: the shapes, what went wrong (none when the identity holds).
#[derive(Clone, Debug, Serialize)]
pub struct BoolCase {
    pub seed: u64,
    pub a: Shape,
    pub b: Shape,
    /// "ok", or the kind of failure: "error", "invalid", "identity".
    pub outcome: String,
    pub detail: String,
}

fn vol(b: &Option<Body>) -> crate::Result<f64> {
    match b {
        Some(b) => Ok(measure(b)?.volume),
        None => Ok(0.0),
    }
}

/// The two primitives [`boolean_case`] uses for a seed.
pub fn boolean_shapes(seed: u64) -> (Shape, Shape) {
    let mut r = Rng::new(seed);
    (Shape::random(&mut r), Shape::random(&mut r))
}

/// Union, intersection and cut of two random primitives: each result is a sound solid, and
/// V(A∪B) + V(A∩B) = V(A) + V(B) and V(A−B) = V(A) − V(A∩B), within 0.2 %.
pub fn boolean_case(seed: u64) -> BoolCase {
    let (a, b) = boolean_shapes(seed);
    let mut case = BoolCase { seed, a: a.clone(), b: b.clone(), outcome: "ok".into(), detail: String::new() };
    let fail = |c: &mut BoolCase, kind: &str, d: String| {
        c.outcome = kind.into();
        c.detail = d;
    };
    let (Ok(ba), Ok(bb)) = (a.body(), b.body()) else {
        fail(&mut case, "error", "a primitive could not be built".into());
        return case;
    };
    let mut res = Vec::new();
    for op in [BoolOp::Union, BoolOp::Intersect, BoolOp::Cut] {
        let result = boolean(&ba, &bb, op);
        match result {
            Ok(x) => {
                if let Some(body) = &x {
                    let bad = body.validity();
                    if !bad.is_empty() {
                        fail(&mut case, "invalid", format!("{op:?}: {}", bad.join("; ")));
                        return case;
                    }
                }
                res.push(x);
            }
            Err(e) => {
                fail(&mut case, "error", format!("{op:?}: {e}"));
                return case;
            }
        }
    }
    let v = |b: &Option<Body>| vol(b).unwrap_or(f64::NAN);
    let (va, vb) = (v(&Some(ba)), v(&Some(bb)));
    let [u, i, c] = &res[..] else { return case };
    let (vu, vi, vc) = (v(u), v(i), v(c));
    let tol = 2e-3 * (va + vb);
    let e1 = (vu + vi - va - vb).abs();
    let e2 = (vc - (va - vi)).abs();
    if !(e1 <= tol && e2 <= tol) {
        fail(&mut case, "identity", format!("A {va:.3}, B {vb:.3}, A∪B {vu:.3}, A∩B {vi:.3}, A−B {vc:.3}: off by {e1:.3} and {e2:.3}"));
    }
    case
}
