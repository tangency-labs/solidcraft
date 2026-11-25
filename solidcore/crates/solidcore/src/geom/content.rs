//! Not upstream: the content digest of a geometry value, a 128-bit name for what the value
//! is (every field that decides how it evaluates, bit for bit), not which value it is.
//!
//! The kernel's operations build new geometry values as they go: a copy of each curve and
//! surface they keep, and a body read back from its flat arrays has none of the values it
//! was written from. A result worked out from a value's geometry alone is the same for any
//! value with the same content, so it can be kept by the content's digest and read back for
//! the next value that has it: the check of a body made from another meets the geometry it
//! checked before.
//!
//! Each kind writes its fields itself, destructuring its whole struct, so a field added to
//! the kind does not compile until it is written or said to change no evaluation. A kind
//! with no digest yet (curves and surfaces that hold other surfaces, volumes or caches of
//! their own, a blend's definition) answers `None`: what is worked out from it is never kept.
//! Floats are written as their bits, so `0.0` and `-0.0` differ (a value can only miss).
//! A digest names content within one process: the hasher's output is not kept anywhere.
//!
//! Upstream: none (solidcore's own).

use std::hash::{Hash, Hasher};

/// A 128-bit digest under construction: the bytes written, read as a polynomial over the
/// field of the Mersenne prime `P = 2^61 - 1` and evaluated at two fixed keys, each half of the
/// digest one evaluation.
///
/// The bytes are taken four at a time, as coefficients below `2^32`, the last ones padded with
/// zeros, and the count of bytes is the last coefficient, so two different byte strings give
/// different coefficient lists. Two lists of at most `n` coefficients that differ give a nonzero
/// difference polynomial of degree at most `n`, which has at most `n` roots in the field. For a
/// key drawn at random the two evaluate alike with probability at most `n / P`, and for two
/// independent keys at most `(n / P)^2`: under `2^-98` for contents of up to `2^12`
/// coefficients (a vertex is about 40, a large face a few thousand) and under `2^-82` up to
/// `2^20`. The keys are fixed constants, as the hasher this replaces (`SipHash` with fixed
/// keys) had: the bound holds for any contents not built from the keys themselves, which
/// geometry is not.
#[derive(Clone, Copy)]
pub(crate) struct Digest {
    keys: [u64; 2],
    acc: [u64; 2],
    /// Bytes not yet taken as a coefficient, little-endian, and their count (under 4).
    pending: u64,
    held: u32,
    /// Bytes written.
    len: u64,
}

/// A finished digest.
pub type ContentId = u128;

/// The field's prime, `2^61 - 1`.
const P: u64 = (1 << 61) - 1;

/// `a * b` mod `P` for `a, b <= P`, possibly `P` itself for a product of `0` mod `P`.
#[inline]
fn mul_mod(a: u64, b: u64) -> u64 {
    let z = u128::from(a) * u128::from(b);
    // z < 2^122: z = hi * 2^61 + lo, and 2^61 = 1 mod P
    let lo = (z as u64) & P;
    let hi = (z >> 61) as u64;
    let s = lo + hi;
    if s >= P {
        s - P
    } else {
        s
    }
}

/// The two keys: fixed, so a digest is the same in every run, every thread and every kernel
/// (a digest is still never kept outside the process). Each is in `1..P`: the first two
/// 64-bit words of the fractional part of pi, reduced mod `P`.
const KEYS: [u64; 2] = [0x243f_6a88_85a3_08d3 % P, 0x1319_8a2e_0370_7344 % P];

impl Digest {
    /// An empty digest.
    pub(crate) fn new() -> Self {
        Self {
            keys: KEYS,
            acc: [0; 2],
            pending: 0,
            held: 0,
            len: 0,
        }
    }

    /// Takes one coefficient (below `2^32`, or the byte count).
    #[inline]
    fn coefficient(&mut self, m: u64) {
        for (acc, &key) in self.acc.iter_mut().zip(&self.keys) {
            let a = mul_mod(*acc, key) + m;
            *acc = if a >= P { a - P } else { a };
        }
    }

    /// Writes bytes.
    fn bytes(&mut self, bytes: &[u8]) {
        self.len = self.len.wrapping_add(bytes.len() as u64);
        for &x in bytes {
            self.pending |= u64::from(x) << (8 * self.held);
            self.held += 1;
            if self.held == 4 {
                let m = self.pending;
                self.coefficient(m);
                self.pending = 0;
                self.held = 0;
            }
        }
    }

    /// Writes a word.
    pub(crate) fn u64(&mut self, x: u64) {
        if self.held == 0 {
            // the same coefficients as its eight little-endian bytes
            self.len = self.len.wrapping_add(8);
            self.coefficient(x & 0xffff_ffff);
            self.coefficient(x >> 32);
        } else {
            self.bytes(&x.to_le_bytes());
        }
    }

    /// Writes a count or an index.
    pub(crate) fn usize(&mut self, x: usize) {
        self.u64(x as u64);
    }

    /// Writes a flag.
    pub(crate) fn bool(&mut self, x: bool) {
        self.u64(u64::from(x));
    }

    /// Writes a float, bit for bit.
    pub(crate) fn f64(&mut self, x: f64) {
        self.u64(x.to_bits());
    }

    /// Writes floats, their count first.
    pub(crate) fn f64s(&mut self, xs: &[f64]) {
        self.usize(xs.len());
        for &x in xs {
            self.f64(x);
        }
    }

    /// Writes a tag naming what follows, so two kinds with the same fields differ.
    pub(crate) fn tag(&mut self, t: u64) {
        self.u64(t);
    }

    /// Writes any value whose `Hash` writes all of it (a field-less enum, an integer).
    pub(crate) fn hashed(&mut self, v: &impl Hash) {
        v.hash(self);
    }

    /// The digest.
    pub(crate) fn finish(&self) -> ContentId {
        let mut d = *self;
        if d.held > 0 {
            let m = d.pending;
            d.coefficient(m);
        }
        // the byte count, below P for any real content
        d.coefficient(d.len % P);
        let [a, b] = d.acc.map(|x| if x >= P { x - P } else { x });
        (u128::from(a) << 64) | u128::from(b)
    }
}

/// The digest takes what a value's `Hash` writes as bytes ([`Digest::hashed`]).
impl Hasher for Digest {
    fn write(&mut self, bytes: &[u8]) {
        self.bytes(bytes);
    }

    fn write_u64(&mut self, x: u64) {
        self.u64(x);
    }

    /// Not a digest: the low half of one, for the `Hasher` contract.
    fn finish(&self) -> u64 {
        Digest::finish(self) as u64
    }
}

impl std::fmt::Write for Digest {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.bytes(s.as_bytes());
        Ok(())
    }
}

/// A value with a content digest: writes its content into `d`, or answers `None` for a
/// value that has none (see the module docs).
pub(crate) trait Content {
    fn content(&self, d: &mut Digest) -> Option<()>;
}

impl Content for crate::geom::vector3d::Vec3 {
    fn content(&self, d: &mut Digest) -> Option<()> {
        let Self { x, y, z } = *self;
        d.f64(x);
        d.f64(y);
        d.f64(z);
        Some(())
    }
}

impl Content for crate::geom::vector2d::Vec2 {
    fn content(&self, d: &mut Digest) -> Option<()> {
        let Self { x, y } = *self;
        d.f64(x);
        d.f64(y);
        Some(())
    }
}

impl Content for crate::nurbs::math::CPoint {
    fn content(&self, d: &mut Digest) -> Option<()> {
        let Self { x, y, z, w } = *self;
        d.f64(x);
        d.f64(y);
        d.f64(z);
        d.f64(w);
        Some(())
    }
}

impl<T: Content + ?Sized> Content for &T {
    fn content(&self, d: &mut Digest) -> Option<()> {
        (**self).content(d)
    }
}

impl<T: Content> Content for Option<T> {
    fn content(&self, d: &mut Digest) -> Option<()> {
        d.bool(self.is_some());
        self.as_ref().map_or(Some(()), |v| v.content(d))
    }
}

impl<T: Content> Content for [T] {
    fn content(&self, d: &mut Digest) -> Option<()> {
        d.usize(self.len());
        for v in self {
            v.content(d)?;
        }
        Some(())
    }
}

/// The digest of `v`, `None` when it has none.
pub(crate) fn content_id(v: &impl Content) -> Option<ContentId> {
    let mut d = Digest::new();
    v.content(&mut d)?;
    Some(d.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitive::api_primitives::{create_box, create_cylinder, create_sphere};
    use crate::topology::Brep;
    use crate::{Kernel, Point3};

    /// A word is the same bytes wherever it falls in the stream, so the word path and the
    /// byte path give one digest; a different stream, or the same bytes cut shorter, does not.
    #[test]
    fn the_digest_is_of_the_byte_stream() {
        let words = |xs: &[u64], lead: &[u8]| {
            let mut d = Digest::new();
            d.bytes(lead);
            for &x in xs {
                d.u64(x);
            }
            d.finish()
        };
        let bytes = |xs: &[u64], lead: &[u8]| {
            let mut d = Digest::new();
            d.bytes(lead);
            for &x in xs {
                d.bytes(&x.to_le_bytes());
            }
            d.finish()
        };
        let xs = [1u64, u64::MAX, 0, 1 << 61, 0x0123_4567_89ab_cdef];
        assert_eq!(words(&xs, &[]), bytes(&xs, &[]));
        assert_eq!(words(&xs, &[7, 8, 9]), bytes(&xs, &[7, 8, 9]));
        assert_ne!(words(&xs, &[]), words(&xs[..4], &[]));
        assert_ne!(words(&[0], &[]), words(&[0, 0], &[]));
        assert_ne!(words(&[0], &[]), Digest::new().finish());
        assert_ne!(words(&[1, 2], &[]), words(&[2, 1], &[]));
        let mut z = Digest::new();
        z.f64(0.0);
        let mut nz = Digest::new();
        nz.f64(-0.0);
        assert_ne!(z.finish(), nz.finish());
    }

    /// The digests of a body's face surfaces and edge curves, in arena order.
    fn digests(b: &Brep) -> (Vec<Option<ContentId>>, Vec<Option<ContentId>>) {
        let faces = b
            .faces()
            .iter()
            .map(|(_, f)| {
                f.surface
                    .as_ref()
                    .and_then(crate::surface::surface::Surface::content_id)
            })
            .collect();
        let edges = b
            .edges()
            .iter()
            .map(|(_, e)| {
                e.curve
                    .as_ref()
                    .and_then(crate::curve::curve::Curve::content_id)
            })
            .collect();
        (faces, edges)
    }

    #[test]
    fn a_body_built_twice_has_the_same_digests() {
        type Make = fn(&mut Kernel) -> crate::Result<Brep>;
        let mut k = Kernel::default();
        let makers: [(Make, bool); 3] = [
            (
                |k| create_box(k, Point3::new(1.0, -2.0, 3.0), 10.0, 20.0, 30.0),
                true,
            ),
            (
                |k| create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 4.0, 7.0),
                true,
            ),
            (|k| create_sphere(k, Point3::new(0.0, 1.0, 0.0), 3.0), false),
        ];
        for (make, every_edge) in makers {
            let a = make(&mut k).unwrap();
            let b = make(&mut k).unwrap();
            let (fa, ea) = digests(&a);
            assert!(fa.iter().all(Option::is_some), "{fa:?}");
            if every_edge {
                assert!(ea.iter().all(Option::is_some), "{ea:?}");
            }
            assert_eq!(digests(&a), digests(&b));
        }
    }

    #[test]
    fn a_copy_has_its_value_s_digest() {
        let mut k = Kernel::default();
        let a = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 4.0, 7.0).unwrap();
        for (_, f) in a.faces().iter() {
            let s = f.surface.as_ref().unwrap();
            assert_eq!(s.clone().content_id(), s.content_id());
        }
        for (_, e) in a.edges().iter() {
            let c = e.curve.as_ref().unwrap();
            assert_eq!(c.clone().content_id(), c.content_id());
        }
    }

    #[test]
    fn another_shape_has_other_digests() {
        let mut k = Kernel::default();
        let a = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 1.0, 1.0, 1.0).unwrap();
        let b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 1.0, 1.0, 1.0 + 1e-12).unwrap();
        let (fa, ea) = digests(&a);
        let (fb, eb) = digests(&b);
        // the top face and the four vertical edges moved; their digests differ
        assert_ne!(fa, fb);
        assert_ne!(ea, eb);
        let shared_faces = fa.iter().filter(|d| fb.contains(d)).count();
        assert!(shared_faces < fa.len(), "{shared_faces}");
    }

    #[test]
    fn a_float_is_written_bit_for_bit() {
        let id = |x: f64| {
            let mut d = Digest::new();
            d.f64(x);
            d.finish()
        };
        assert_ne!(id(0.0), id(-0.0));
        assert_ne!(id(1.0), id(1.0 + f64::EPSILON));
        assert_eq!(id(0.5), id(0.5));
    }
}
