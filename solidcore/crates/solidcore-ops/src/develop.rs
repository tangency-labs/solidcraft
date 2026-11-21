//! Sheet metal development: a sheet metal part's surfaces (the record's
//! [`solidcore_record::develop::Sheet`]) laid out into its flat pattern ([`develop`]) and folded
//! back onto the part ([`fold`]), and the rigid motions and bend measures an application places
//! panels with. Each call converts through `solidcore-convert`'s `develop` and runs the kernel's
//! ([`solidcore::develop`]); nothing here computes.
//!
//! The record's motions and bends take their maths here as methods of extension traits
//! ([`Rigid2Ops`], [`Rigid3Ops`], [`BendOps`], [`LapOps`]): bring them into scope to call
//! `motion.point(p)` on a record value. A fold and its bends are this crate's values
//! ([`Fold`], [`Bent`]), kept in the kernel's form so their points are found without converting
//! the sheet again.
//!
//! A refusal is the kernel's typed [`Refusal`]: the panel or joint and the cause. An application
//! words it.
//!
//! [`hermite`] follows a record curve carried into the flat by cubic pieces, each proved within a
//! tolerance of it (the kernel's [`solidcore::develop::hermite`]).

use solidcore::develop as k;
use solidcore_convert::develop as cv;
use solidcore_convert::{
    record_region2, record_seg2, record_vec2, record_vec3, region2, seg2, vec2, vec3,
};
use solidcore_record::develop::{Bend, Development, Lap, Mismatch, Rigid2, Rigid3, Sheet, Surface};
use solidcore_record::{Loop2, Region2, Seg2, Vec2, Vec3};

pub use solidcore::develop::{
    JointRefusal, PanelRefusal, Refusal, RuleRefusal, MAX_JOINTS, MAX_PANELS,
};

/// Lays the sheet out flat ([`solidcore::develop::develop`]): `anchors` says where some panels'
/// neutral charts lie in the flat frame.
///
/// # Errors
/// The kernel's [`Refusal`]: the panel or joint it cannot lay out, and why.
pub fn develop(sheet: &Sheet, anchors: &[(usize, Rigid2)]) -> Result<Development, Refusal> {
    let anchors: Vec<(usize, k::Rigid2)> =
        anchors.iter().map(|(i, m)| (*i, cv::rigid2(m))).collect();
    k::develop(&cv::sheet(sheet), &anchors).map(|d| cv::record_development(&d))
}

/// Folds a developed sheet ([`solidcore::develop::fold`]): `roots` says where the first panel of
/// each of the development's pieces lies in space.
///
/// # Errors
/// The kernel's [`Refusal`]: a rule no sheet can have, a root for each piece missing, an anchor
/// with no frame on its panel.
pub fn fold(sheet: &Sheet, dev: &Development, roots: &[Rigid3]) -> Result<Fold, Refusal> {
    let roots: Vec<k::Rigid3> = roots.iter().map(cv::rigid3).collect();
    let inner = k::fold(&cv::sheet(sheet), &cv::development(dev), &roots)?;
    Ok(Fold {
        places: inner
            .places
            .iter()
            .map(|p| p.as_ref().map(cv::record_rigid3))
            .collect(),
        bends: inner.bends.iter().map(|b| b.map(Bent)).collect(),
        loops: inner.loops.iter().map(cv::record_mismatch).collect(),
        inner,
    })
}

/// Where the edges of `a` cross those of `b` ([`solidcore::develop::crossings`]): each
/// crossing's edge and parameter on either, and its point.
#[must_use]
pub fn crossings(a: &[Seg2], b: &[Seg2]) -> Vec<(usize, f64, usize, f64, Vec2)> {
    k::crossings(&cv::segs(a), &cv::segs(b))
        .into_iter()
        .map(|(ia, u, ib, v, p)| (ia, u, ib, v, record_vec2(p)))
        .collect()
}

/// A curve through `points` in order ([`solidcore::develop::curve_through`]): a line where they
/// lie on one, else a clamped cubic spline through every one of them.
#[must_use]
pub fn curve_through(points: &[Vec2]) -> Vec<Seg2> {
    let points: Vec<solidcore::math::Vec2> = points.iter().copied().map(vec2).collect();
    cv::record_segs(&k::curve_through(&points))
}

/// A developed sheet folded ([`fold`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Fold {
    /// Where each panel's frame lies in space.
    pub places: Vec<Option<Rigid3>>,
    /// Where each joint's bend lies in space (none for a seam or a rip).
    pub bends: Vec<Option<Bent>>,
    /// Bends and seams that close a ring, where the fold reached both panels another way: how
    /// far the joint itself would put its `b` panel from where the ring put it.
    pub loops: Vec<Mismatch>,
    inner: k::Fold,
}

impl Fold {
    /// Where a point of panel `i`'s flat material goes, `depth` into the material.
    #[must_use]
    pub fn panel_point(&self, i: usize, p: Vec2, depth: f64) -> Option<Vec3> {
        self.inner.panel_point(i, vec2(p), depth).map(record_vec3)
    }

    /// The point of panel `i`'s surface at the parameters `q`, `depth` into the material.
    #[must_use]
    pub fn surface_point(&self, i: usize, q: Vec2, depth: f64) -> Option<Vec3> {
        self.inner.surface_point(i, vec2(q), depth).map(record_vec3)
    }

    /// Where panel `i`'s neutral chart lies in the flat.
    #[must_use]
    pub fn placement(&self, i: usize) -> Option<Rigid2> {
        self.inner.placement(i).as_ref().map(cv::record_rigid2)
    }

    /// Where the point of panel `i`'s surface at the parameters `q` lies in the flat.
    #[must_use]
    pub fn flat_point(&self, i: usize, q: Vec2) -> Option<Vec2> {
        self.inner.flat_point(i, vec2(q)).map(record_vec2)
    }

    /// The parameters of panel `i`'s surface at a point of its flat material.
    #[must_use]
    pub fn param(&self, i: usize, p: Vec2) -> Option<Vec2> {
        self.inner.param(i, vec2(p)).map(record_vec2)
    }

    /// The surface panel `i` lies on.
    #[must_use]
    pub fn surface(&self, i: usize) -> Option<Surface> {
        self.inner.surface(i).map(cv::record_surface)
    }

    /// A roll's axis: a point of it and its direction. `None` for a plane.
    #[must_use]
    pub fn axis(&self, i: usize) -> Option<(Vec3, Vec3)> {
        self.inner
            .axis(i)
            .map(|(p, d)| (record_vec3(p), record_vec3(d)))
    }
}

/// A bend in space, and its flat zone wrapped onto it ([`Fold::bends`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bent(k::Bent);

impl Bent {
    /// Where a point of the bend's flat zone goes, `depth` into the material.
    #[must_use]
    pub fn point(&self, p: Vec2, depth: f64) -> Vec3 {
        record_vec3(self.0.point(vec2(p), depth))
    }

    /// The point `along` the joint, turned `phi` round the bend from the `a` panel's tangent
    /// line, `depth` into the material.
    #[must_use]
    pub fn at(&self, along: f64, phi: f64, depth: f64) -> Vec3 {
        record_vec3(self.0.at(along, phi, depth))
    }

    /// A point of the flat zone as its length along the joint and the angle it lies round the
    /// bend from the `a` panel's tangent line.
    #[must_use]
    pub fn wrapped(&self, p: Vec2) -> (f64, f64) {
        self.0.wrapped(vec2(p))
    }

    /// Where a point of space lies in the bend: the point of the flat zone whose fiber runs
    /// through it, how deep it lies, and the way deeper. `None` on the bend's axis.
    #[must_use]
    pub fn unwrapped(&self, p: Vec3) -> Option<(Vec2, f64, Vec3)> {
        self.0
            .unwrapped(vec3(p))
            .map(|(flat, depth, deeper)| (record_vec2(flat), depth, record_vec3(deeper)))
    }

    /// A point of the bend's axis and its direction.
    #[must_use]
    pub fn axis(&self) -> (Vec3, Vec3) {
        let (p, d) = self.0.axis();
        (record_vec3(p), record_vec3(d))
    }

    /// How far the bend turns, in radians.
    #[must_use]
    pub fn angle(&self) -> f64 {
        self.0.angle()
    }

    /// The joint's way in the flat (from the `a` panel's side).
    #[must_use]
    pub fn flat_way(&self) -> Vec2 {
        record_vec2(self.0.flat_way())
    }
}

/// The kernel's rigid motions of the plane on the record's [`Rigid2`]
/// ([`solidcore::develop::Rigid2`]).
pub trait Rigid2Ops: Sized {
    /// The turn by `angle` radians about the origin.
    fn turn(angle: f64) -> Self;
    /// The motion that puts the point `from` on `to` and turns `from_dir` to `to_dir`; `None`
    /// when either direction has no length.
    fn onto(from: Vec2, from_dir: Vec2, to: Vec2, to_dir: Vec2) -> Option<Self>;
    /// The turn it makes, in radians.
    fn angle(&self) -> f64;
    /// A vector turned.
    fn vector(&self, v: Vec2) -> Vec2;
    /// A point moved.
    fn point(&self, p: Vec2) -> Vec2;
    /// This motion, then `next`.
    #[must_use]
    fn then(&self, next: &Self) -> Self;
    /// The motion undone.
    #[must_use]
    fn inverse(&self) -> Self;
    /// A segment moved, exactly.
    fn seg(&self, s: &Seg2) -> Seg2;
    /// A loop moved.
    fn loop2(&self, l: &Loop2) -> Loop2;
    /// A region moved.
    fn region(&self, r: &Region2) -> Region2;
}

impl Rigid2Ops for Rigid2 {
    fn turn(angle: f64) -> Rigid2 {
        cv::record_rigid2(&k::Rigid2::turn(angle))
    }
    fn onto(from: Vec2, from_dir: Vec2, to: Vec2, to_dir: Vec2) -> Option<Rigid2> {
        k::Rigid2::onto(vec2(from), vec2(from_dir), vec2(to), vec2(to_dir))
            .map(|m| cv::record_rigid2(&m))
    }
    fn angle(&self) -> f64 {
        cv::rigid2(self).angle()
    }
    fn vector(&self, v: Vec2) -> Vec2 {
        record_vec2(cv::rigid2(self).vector(vec2(v)))
    }
    fn point(&self, p: Vec2) -> Vec2 {
        record_vec2(cv::rigid2(self).point(vec2(p)))
    }
    fn then(&self, next: &Rigid2) -> Rigid2 {
        cv::record_rigid2(&cv::rigid2(self).then(&cv::rigid2(next)))
    }
    fn inverse(&self) -> Rigid2 {
        cv::record_rigid2(&cv::rigid2(self).inverse())
    }
    fn seg(&self, s: &Seg2) -> Seg2 {
        record_seg2(&cv::rigid2(self).seg(&seg2(s)))
    }
    fn loop2(&self, l: &Loop2) -> Loop2 {
        solidcore_convert::record_loop2(&cv::rigid2(self).loop2(&solidcore_convert::loop2(l)))
    }
    fn region(&self, r: &Region2) -> Region2 {
        record_region2(&cv::rigid2(self).region(&region2(r)))
    }
}

/// The kernel's rigid motions of space on the record's [`Rigid3`]
/// ([`solidcore::develop::Rigid3`]).
pub trait Rigid3Ops: Sized {
    /// The frame at `origin` whose axes are `x`, `y` and `x × y`; `None` unless `x` and `y` are
    /// unit vectors square to each other.
    fn frame(origin: Vec3, x: Vec3, y: Vec3) -> Option<Self>;
    /// A vector turned.
    fn vector(&self, v: Vec3) -> Vec3;
    /// A point moved.
    fn point(&self, p: Vec3) -> Vec3;
    /// This motion, then `next`.
    #[must_use]
    fn then(&self, next: &Self) -> Self;
    /// The motion undone.
    #[must_use]
    fn inverse(&self) -> Self;
    /// The angle of the turn from this motion's to `other`'s, in radians.
    fn angle_to(&self, other: &Self) -> f64;
}

impl Rigid3Ops for Rigid3 {
    fn frame(origin: Vec3, x: Vec3, y: Vec3) -> Option<Rigid3> {
        k::Rigid3::frame(vec3(origin), vec3(x), vec3(y)).map(|m| cv::record_rigid3(&m))
    }
    fn vector(&self, v: Vec3) -> Vec3 {
        record_vec3(cv::rigid3(self).vector(vec3(v)))
    }
    fn point(&self, p: Vec3) -> Vec3 {
        record_vec3(cv::rigid3(self).point(vec3(p)))
    }
    fn then(&self, next: &Rigid3) -> Rigid3 {
        cv::record_rigid3(&cv::rigid3(self).then(&cv::rigid3(next)))
    }
    fn inverse(&self) -> Rigid3 {
        cv::record_rigid3(&cv::rigid3(self).inverse())
    }
    fn angle_to(&self, other: &Rigid3) -> f64 {
        cv::rigid3(self).angle_to(&cv::rigid3(other))
    }
}

/// The kernel's closed forms of a bend on the record's [`Bend`].
pub trait BendOps {
    /// The flat width of the bend zone: its neutral arc's length, `θ (R + K T)`.
    fn allowance(&self, thickness: f64, k_factor: f64) -> f64;
    /// How far each anchor lies from its panel's tangent line, toward the other panel.
    fn setback(&self, thickness: f64) -> f64;
}

impl BendOps for Bend {
    fn allowance(&self, thickness: f64, k_factor: f64) -> f64 {
        cv::bend(self).allowance(thickness, k_factor)
    }
    fn setback(&self, thickness: f64) -> f64 {
        cv::bend(self).setback(thickness)
    }
}

/// The kernel's cut of a rip's lap on the record's [`Lap`].
pub trait LapOps {
    /// What each panel `(a, b)` is cut back from the edge, where the sheet turns `turn` across
    /// it and is `thickness` thick.
    fn cuts(&self, turn: f64, thickness: f64) -> (f64, f64);
}

impl LapOps for Lap {
    fn cuts(&self, turn: f64, thickness: f64) -> (f64, f64) {
        cv::lap(*self).cuts(turn, thickness)
    }
}

/// Following a record curve carried into the flat by cubic pieces, each proved within a tolerance
/// of it ([`solidcore::develop::hermite`]): the pieces of its parameter over which it is smooth,
/// with bounds on its derivatives, the bound on a piece's distance from the curve, the nodes
/// between which pieces stand within the tolerance, and each piece. Each call converts through
/// `solidcore-convert` and runs the kernel's; nothing here computes.
pub mod hermite {
    use solidcore::develop::hermite as k;
    use solidcore_convert::{record_pair, record_seg2, vec2};
    use solidcore_record::brep::Curve;
    use solidcore_record::{Seg2, Vec2, Vec3};

    pub use solidcore::develop::hermite::ROUNDING;

    /// A piece of a curve's parameter over which the curve is smooth ([`smooth_pieces`]), kept in
    /// the kernel's form.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Smooth(k::Smooth);

    impl Smooth {
        /// The piece of the parameter.
        #[must_use]
        pub fn span(&self) -> [f64; 2] {
            self.0.span()
        }

        /// At least the length of each of the curve's first four derivatives anywhere on the
        /// piece.
        #[must_use]
        pub fn rates(&self) -> [f64; 4] {
            self.0.rates()
        }

        /// At most how far a point of the piece, as [`Smooth::at`] reads it, lies from the curve
        /// the record holds at the same parameter.
        #[must_use]
        pub fn slack(&self) -> f64 {
            self.0.slack()
        }

        /// The curve's point and first derivative at `t`, from the piece's own form: at an end of
        /// the piece, the rate from its side.
        #[must_use]
        pub fn at(&self, t: f64) -> Option<(Vec3, Vec3)> {
            self.0.at(t).map(record_pair)
        }
    }

    /// The pieces of `curve` from parameter `a` to `b` over which it is smooth, in order, each
    /// with bounds on its first four derivatives ([`solidcore::develop::hermite::smooth_pieces`]).
    /// `None` for a traced intersection, a spline whose pieces can't be had, a parameter range
    /// the curve doesn't cover, and a curve the conversion refuses (geometry too deep).
    #[must_use]
    pub fn smooth_pieces(curve: &Curve, span: [f64; 2]) -> Option<Vec<Smooth>> {
        let curve = solidcore_convert::curve(curve).ok()?;
        k::smooth_pieces(&curve, span).map(|pieces| pieces.into_iter().map(Smooth).collect())
    }

    /// A bound on the length of the fourth derivative of a map of a curve, from bounds on the
    /// curve's first four derivatives and the map's ([`solidcore::develop::hermite::fourth`]).
    #[must_use]
    pub fn fourth(c: [f64; 4], g: [f64; 4]) -> f64 {
        k::fourth(c, g)
    }

    /// How far a cubic piece over a step `h` lies at most from the carried curve it follows,
    /// where the curve's fourth derivative is at most `fourth` long over the step
    /// ([`solidcore::develop::hermite::remainder`]).
    #[must_use]
    pub fn remainder(h: f64, fourth: f64) -> f64 {
        k::remainder(h, fourth)
    }

    /// The parameters from `a` to `b`, in order, between which cubic pieces stand proved within
    /// `tol` of the curve they follow, `off(t₀, t₁)` bounding how far the piece from `t₀` to `t₁`
    /// lies from it ([`solidcore::develop::hermite::nodes`]).
    pub fn nodes(
        off: impl Fn(f64, f64) -> Option<f64>,
        span: [f64; 2],
        tol: f64,
    ) -> Option<Vec<f64>> {
        k::nodes(off, span, tol)
    }

    /// The cubic from `p0`, leaving it at the rate `d0`, to `p3`, reaching it at the rate `d3`,
    /// over a step `h` of the parameter ([`solidcore::develop::hermite::cubic`]).
    #[must_use]
    pub fn cubic(p0: Vec2, d0: Vec2, p3: Vec2, d3: Vec2, h: f64) -> Seg2 {
        record_seg2(&k::cubic(vec2(p0), vec2(d0), vec2(p3), vec2(d3), h))
    }
}
