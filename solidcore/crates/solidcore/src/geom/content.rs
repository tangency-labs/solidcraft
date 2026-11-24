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

/// A 128-bit digest under construction: two SipHash runs started from different words, so
/// two contents share a digest only by a 128-bit collision.
pub(crate) struct Digest([std::collections::hash_map::DefaultHasher; 2]);

/// A finished digest.
pub type ContentId = u128;

impl Digest {
    /// An empty digest.
    pub(crate) fn new() -> Self {
        let mut h = [
            std::collections::hash_map::DefaultHasher::new(),
            std::collections::hash_map::DefaultHasher::new(),
        ];
        0x5ca1_ab1e_u64.hash(&mut h[0]);
        0x0dd_ba11_u64.hash(&mut h[1]);
        Self(h)
    }

    /// Writes a word.
    pub(crate) fn u64(&mut self, x: u64) {
        for h in &mut self.0 {
            h.write_u64(x);
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

    /// Writes an optional float.
    pub(crate) fn opt_f64(&mut self, x: Option<f64>) {
        match x {
            Some(x) => {
                self.bool(true);
                self.f64(x);
            }
            None => self.bool(false),
        }
    }

    /// Writes any value whose `Hash` writes all of it (a field-less enum, an integer).
    pub(crate) fn hashed(&mut self, v: &impl Hash) {
        for h in &mut self.0 {
            v.hash(h);
        }
    }

    /// The digest.
    pub(crate) fn finish(&self) -> ContentId {
        let [a, b] = &self.0;
        (u128::from(a.finish()) << 64) | u128::from(b.finish())
    }
}

impl std::fmt::Write for Digest {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        for h in &mut self.0 {
            h.write(s.as_bytes());
        }
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
        match self {
            Some(v) => {
                d.bool(true);
                v.content(d)
            }
            None => {
                d.bool(false);
                Some(())
            }
        }
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

    /// The digests of a body's face surfaces and edge curves, in arena order.
    fn digests(b: &Brep) -> (Vec<Option<ContentId>>, Vec<Option<ContentId>>) {
        let faces = b
            .faces()
            .iter()
            .map(|(_, f)| f.surface.as_ref().and_then(|s| s.content_id()))
            .collect();
        let edges = b
            .edges()
            .iter()
            .map(|(_, e)| e.curve.as_ref().and_then(|c| c.content_id()))
            .collect();
        (faces, edges)
    }

    #[test]
    fn a_body_built_twice_has_the_same_digests() {
        let mut k = Kernel::default();
        let makers: [(fn(&mut Kernel) -> crate::Result<Brep>, bool); 3] = [
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
