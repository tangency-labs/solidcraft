//! #3218: a defeature that keeps a feature on its base names why its patch is not built
//! ([`DefeatureFailure`], carried by the refusal's [`Reason::Defeature`] at the open loop's
//! edge, and by each [`KeptFeature::limit`]). Each limit is decided where the patch's
//! construction refuses, from what it found there:
//!
//! - `LoopNotPlanar`: a planar cap asked for a cylinder fillet's loop (two lines and two arcs
//!   on the cylinder), which no plane holds;
//! - `ExtensionRefused`: the faces along the loop asked to be extended by 0;
//! - `TangentNeighbours`: a patch piece cut from a neighbour's extension only along an edge
//!   the kernel proves smooth. `fillet1019`'s fillet that ends on the tangent corner round
//!   was refused so until #15247 corrected the classification that left its patch open; it
//!   now comes off, and the tests below prove the body it builds (no upstream file here
//!   reaches this limit now);
//! - `NotEnclosed`: a box face, whose neighbours, extended, run past it;
//! - `ExtensionWithinTolerance`: the faces along a fillet's loop extended by less than the
//!   distance within which the loop's vertices and the patch's are one point.
//!
//! The models are the upstream test files, read by the Rust reader when the pinned upstream
//! checkout is present.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::feature_executive::{FeatureExecutive, KeptFeature, RebuildBrepType};
use solidcore::error::defeature::DefeatureFailure;
use solidcore::error::{Reason, SM_ERR};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::mass::enclosure::enclose_volume;
use solidcore::surface::implicit::AnalyticSupport;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::brep::validation;
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, EntityRef, Kernel, SolidError};

/// An upstream test file, read and arena-named as the goldens reader does.
fn read(k: &mut Kernel, file: &str) -> Option<Brep> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles")
        .join(file);
    let Ok(bytes) = std::fs::read(&path) else {
        assert!(
            std::env::var_os("SOLIDCORE_REQUIRE_UPSTREAM").is_none(),
            "SOLIDCORE_REQUIRE_UPSTREAM is set and the upstream test file is missing: {}",
            path.display()
        );
        return None;
    };
    let mut b = Brep::new_with_infinite_region().unwrap();
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let key = |_: EntityRef| ObjectRef {
        key: ObjectKey(0),
        type_code: 0,
    };
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key,
        records: &mut records,
        next_key: 1 << 50,
    };
    read_from_file(k, &mut b, bytes, FileType::Ascii, false, &mut env).unwrap();
    b.use_arena_naming().unwrap();
    Some(b)
}

/// Face `face` of `file` defeatured alone with base rebuild `base_rb` (and the base extension
/// distance `ext`, when given): the refusal's limit, checked to be at an edge with the
/// status `SM_ERR`, and the base unchanged.
fn limit(
    file: &str,
    face: usize,
    base_rb: RebuildBrepType,
    ext: Option<f64>,
) -> Option<DefeatureFailure> {
    let mut k = Kernel::default();
    let mut b = read(&mut k, file)?;
    let faces = bp1::faces(&b, None).unwrap();
    let before = faces.len();
    let r = {
        let mut ex = FeatureExecutive::new(&mut k, &mut b).unwrap();
        ex.add_feature(
            &mut k,
            &[faces[face]],
            base_rb,
            RebuildBrepType::ExtendSurface,
        )
        .unwrap()
        .unwrap();
        if let Some(d) = ext {
            ex.set_base_ext_dist(d);
        }
        ex.do_defeature(&mut k)
    };
    assert_eq!(
        bp1::faces(&b, None).unwrap().len(),
        before,
        "{file} {face}: base changed"
    );
    match &r {
        Err(e @ SolidError::Failed(f)) => {
            assert!(matches!(f.entity(), Some(EntityRef::Edge(_))), "{r:?}");
            assert_eq!(e.sm_status(), Some(SM_ERR), "{r:?}");
            match f.reason {
                Reason::Defeature(d) => Some(d),
                _ => panic!("{file} {face}: {r:?}"),
            }
        }
        other => panic!("{file} {face}: a refusal naming its limit, got {other:?}"),
    }
}

#[test]
fn a_planar_cap_on_a_cylinder_fillet_s_loop_is_refused_as_not_planar() {
    let Some(l) = limit(
        "box_with_large_fillet.smb",
        6,
        RebuildBrepType::PlanarCap,
        None,
    ) else {
        return;
    };
    assert_eq!(l, DefeatureFailure::LoopNotPlanar);
}

#[test]
fn an_extension_by_nothing_is_refused_as_the_extension_s() {
    let Some(l) = limit(
        "box_with_large_fillet.smb",
        6,
        RebuildBrepType::ExtendSurface,
        Some(0.0),
    ) else {
        return;
    };
    assert_eq!(l, DefeatureFailure::ExtensionRefused { distance: 0.0 });
}

/// `fillet1019` defeatured by the features of `faces` (each a list of face indices, added
/// in turn; a list that makes no feature is skipped, as the goldens' runner does): no
/// feature kept. Returns the body.
fn defeatured(k: &mut Kernel, faces: &[&[usize]]) -> Option<Brep> {
    let mut b = read(k, "Fillet/fillet1019.smb")?;
    let all = bp1::faces(&b, None).unwrap();
    {
        let mut ex = FeatureExecutive::new(k, &mut b).unwrap();
        for set in faces {
            let fs: Vec<_> = set.iter().map(|&i| all[i]).collect();
            let _ = ex
                .add_feature(
                    k,
                    &fs,
                    RebuildBrepType::ExtendSurface,
                    RebuildBrepType::ExtendSurface,
                )
                .unwrap();
        }
        let kept = ex.do_defeature(k).unwrap();
        assert!(kept.is_empty(), "{faces:?}: {kept:?}");
    }
    Some(b)
}

/// #3218 first asked that `fillet1019`'s bottom fillet (face 3), which ends on the corner
/// round (face 6), be refused as [`DefeatureFailure::TangentNeighbours`]: the patch's piece
/// on the x- wall seemed cut from the round's extension only along an edge proved smooth,
/// so nothing bounded it. That reading came from #15247's misclassification: the boolean
/// took a point outside the round's extension's hole for outside that face, which left the
/// patch open. Classified correctly, the patch closes and the fillet comes off, so this
/// test now proves the body it builds is the right one ([`certify`]): the frustum with the
/// top fillet and the corner round, the round carried down to the bottom by its own
/// cylinder.
#[test]
fn a_fillet_ending_on_a_tangent_fillet_comes_off_to_the_rolling_ball_body() {
    let mut k = Kernel::default();
    let Some(mut b) = defeatured(&mut k, &[&[3]]) else {
        return;
    };
    certify(&mut k, &mut b, Removed::Bottom);
}

/// The top fillet (face 8), which ends on the corner round at the top, comes off the same
/// way: the frustum with the bottom fillet and the corner round ([`certify`]).
#[test]
fn a_top_fillet_ending_on_a_tangent_fillet_comes_off_to_the_rolling_ball_body() {
    let mut k = Kernel::default();
    let Some(mut b) = defeatured(&mut k, &[&[8]]) else {
        return;
    };
    certify(&mut k, &mut b, Removed::Top);
}

/// One feature per fillet face (the goldens' `fillet_each_fillet1019`): only the bottom
/// fillet makes a feature (the round and the top fillet, which touch it, make none), so the
/// body is the bottom fillet's ([`certify`]).
#[test]
fn a_feature_per_fillet_of_fillet1019_takes_off_the_bottom_fillet() {
    let mut k = Kernel::default();
    let Some(mut b) = defeatured(&mut k, &[&[3], &[6], &[8]]) else {
        return;
    };
    certify(&mut k, &mut b, Removed::Bottom);
}

#[test]
fn a_box_face_is_refused_as_not_enclosed() {
    let Some(l) = limit(
        "box_with_large_fillet.smb",
        0,
        RebuildBrepType::ExtendSurface,
        None,
    ) else {
        return;
    };
    assert!(matches!(l, DefeatureFailure::NotEnclosed { .. }), "{l:?}");
}

/// `box_with_large_fillet`'s fillet (face 6), its faces extended by 0.001: the loop's
/// vertices carry tolerances of 2.374e-3, so a vertex of the extended boundary and the
/// loop's vertex are one point within their summed zone tolerances and the 0.001 extension
/// is no feature the kernel can resolve (the merged vertex's tolerance covers it). The
/// refusal names that identity radius, from the loop vertices' tolerances and the patch's:
/// a lower bound on the extension, not a size that builds a valid body (see the bracket test
/// below and #16457).
#[test]
fn an_extension_within_the_loop_s_tolerance_is_refused_naming_the_identity_radius() {
    let Some(l) = limit(
        "box_with_large_fillet.smb",
        6,
        RebuildBrepType::ExtendSurface,
        Some(0.001),
    ) else {
        return;
    };
    let DefeatureFailure::ExtensionWithinTolerance {
        extended_by,
        identity_radius,
    } = l
    else {
        panic!("a refusal naming the identity radius, got {l:?}");
    };
    assert_eq!(extended_by, 0.001);
    let mut k = Kernel::default();
    let mut b = read(&mut k, "box_with_large_fillet.smb").unwrap();
    let widest = bp1::vertices(&b, None)
        .unwrap()
        .into_iter()
        .map(|v| b.vertex(v).unwrap().tolerance)
        .fold(0.0_f64, f64::max);
    let patch = b.tolerance();
    assert!(
        identity_radius > widest && identity_radius <= (widest + patch) * (1.0 + 1e-12),
        "radius {identity_radius}, loop vertex tolerance {widest}, patch {patch}"
    );
    let faces = bp1::faces(&b, None).unwrap();
    let mut ex = FeatureExecutive::new(&mut k, &mut b).unwrap();
    ex.add_feature(
        &mut k,
        &[faces[6]],
        RebuildBrepType::ExtendSurface,
        RebuildBrepType::ExtendSurface,
    )
    .unwrap()
    .unwrap();
    ex.set_base_ext_dist(1.01 * identity_radius);
    let r = ex.do_defeature(&mut k);
    assert!(r.is_ok(), "extended by 1.01 x {identity_radius}: {r:?}");
}

/// The same fillet bracketed at its identity radius: extended by 0.9999 x the radius the
/// patch still fails and is refused naming the radius; extended by 1.0001 x it builds. On
/// this model the construction's own failure ends at the radius to within 1e-4 (measured:
/// it fails at 2.3843e-3 and builds at 2.3845e-3, the radius being 2.38440e-3), so the
/// refusal is named over no extension that would have built. The body built past it is not
/// valid: Level 2 reports 6 x `SmBrep` test 61 (an edge fails `ValidateGeometry`), as main's
/// body does at every extension from 0.001 to 0.01 (#16457). The radius is a lower bound on
/// the extension, never a size that fits; this asserts the defect as it is, so a fix shows.
#[test]
fn the_identity_radius_brackets_where_the_patch_starts_to_build() {
    // (no upstream test file: nothing to check)
    let Some(l) = limit(
        "box_with_large_fillet.smb",
        6,
        RebuildBrepType::ExtendSurface,
        Some(0.001),
    ) else {
        return;
    };
    let DefeatureFailure::ExtensionWithinTolerance {
        identity_radius, ..
    } = l
    else {
        panic!("a refusal naming the identity radius, got {l:?}");
    };
    let below = 0.9999 * identity_radius;
    let l = limit(
        "box_with_large_fillet.smb",
        6,
        RebuildBrepType::ExtendSurface,
        Some(below),
    );
    assert_eq!(
        l,
        Some(DefeatureFailure::ExtensionWithinTolerance {
            extended_by: below,
            identity_radius,
        })
    );
    let mut k = Kernel::default();
    let mut b = read(&mut k, "box_with_large_fillet.smb").unwrap();
    let faces = bp1::faces(&b, None).unwrap();
    let mut ex = FeatureExecutive::new(&mut k, &mut b).unwrap();
    ex.add_feature(
        &mut k,
        &[faces[6]],
        RebuildBrepType::ExtendSurface,
        RebuildBrepType::ExtendSurface,
    )
    .unwrap()
    .unwrap();
    ex.set_base_ext_dist(1.0001 * identity_radius);
    let r = ex.do_defeature(&mut k);
    assert!(r.is_ok(), "extended by 1.0001 x {identity_radius}: {r:?}");
    drop(ex);
    let check = validation::check_brep(&mut k, &mut b, validation::CheckLevel::Level2).unwrap();
    let lines = check.lines();
    let invalid_edges = lines.iter().filter(|l| l.contains("Test #61")).count();
    assert_eq!(
        invalid_edges, 6,
        "#16457: the body past the radius fails ValidateGeometry on 6 edges: {lines:?}"
    );
}

/// A kept feature carries its limit too: of the two fillets of `cyl_on_box_w_fillet` one
/// comes off and the other is kept, naming why.
#[test]
fn a_kept_feature_names_its_limit() {
    let mut k = Kernel::default();
    let Some(mut b) = read(&mut k, "Fillet/cyl_on_box_w_fillet.smb") else {
        return;
    };
    let faces = bp1::faces(&b, None).unwrap();
    let kept: Vec<KeptFeature> = {
        let mut ex = FeatureExecutive::new(&mut k, &mut b).unwrap();
        for f in [3, 8] {
            ex.add_feature(
                &mut k,
                &[faces[f]],
                RebuildBrepType::ExtendSurface,
                RebuildBrepType::ExtendSurface,
            )
            .unwrap()
            .unwrap();
        }
        ex.do_defeature(&mut k).unwrap()
    };
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert!(kept[0].limit.is_some(), "{kept:?}");
}

// ---- the certificate for fillet1019 (#15247 / #3218) ------------------------------------

/// An interval `[lo, hi]` with outward rounding: each operation's result is widened by one
/// ulp each way, so it holds the exact result of the operation on any reals in its operands
/// (the IEEE operations `+ - * / sqrt` are correctly rounded).
#[derive(Clone, Copy, Debug)]
struct I {
    lo: f64,
    hi: f64,
}

fn up(x: f64) -> f64 {
    if x.is_nan() || x == f64::INFINITY {
        x
    } else if x == 0.0 {
        f64::from_bits(1)
    } else if x > 0.0 {
        f64::from_bits(x.to_bits() + 1)
    } else {
        f64::from_bits(x.to_bits() - 1)
    }
}

fn down(x: f64) -> f64 {
    -up(-x)
}

impl I {
    fn p(x: f64) -> Self {
        Self { lo: x, hi: x }
    }
    fn new(lo: f64, hi: f64) -> Self {
        assert!(lo <= hi, "{lo} > {hi}");
        Self { lo, hi }
    }
    fn width(self) -> f64 {
        self.hi - self.lo
    }
    fn sqr(self) -> Self {
        let m = self * self;
        if self.lo <= 0.0 && self.hi >= 0.0 {
            Self::new(0.0, m.hi)
        } else {
            m
        }
    }
    /// The square root of the part of `self` at or above 0: used where the exact value is
    /// known not to be negative (a discriminant of a line meeting a circle it is tangent to
    /// at worst), so a negative lower end is rounding.
    fn sqrt0(self) -> Self {
        assert!(self.hi >= 0.0, "{self:?}");
        Self::new(down(self.lo.max(0.0).sqrt()).max(0.0), up(self.hi.sqrt()))
    }
    fn max(self, o: Self) -> Self {
        Self::new(self.lo.max(o.lo), self.hi.max(o.hi))
    }
    fn min(self, o: Self) -> Self {
        Self::new(self.lo.min(o.lo), self.hi.min(o.hi))
    }
    fn contains(self, o: Self) -> bool {
        self.lo <= o.lo && o.hi <= self.hi
    }
    /// The measure of `[a, b]` (`0` when empty), for `a`, `b` known to their intervals.
    fn seg(a: Self, b: Self) -> Self {
        Self::new(down(b.lo - a.hi).max(0.0), up(b.hi - a.lo).max(0.0))
    }
}

impl core::ops::Add for I {
    type Output = I;
    fn add(self, o: I) -> I {
        I::new(down(self.lo + o.lo), up(self.hi + o.hi))
    }
}
impl core::ops::Sub for I {
    type Output = I;
    fn sub(self, o: I) -> I {
        I::new(down(self.lo - o.hi), up(self.hi - o.lo))
    }
}
impl core::ops::Neg for I {
    type Output = I;
    fn neg(self) -> I {
        I::new(-self.hi, -self.lo)
    }
}
impl core::ops::Mul for I {
    type Output = I;
    fn mul(self, o: I) -> I {
        let p = [
            self.lo * o.lo,
            self.lo * o.hi,
            self.hi * o.lo,
            self.hi * o.hi,
        ];
        let lo = p.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = p.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        I::new(down(lo), up(hi))
    }
}
impl core::ops::Div for I {
    type Output = I;
    fn div(self, o: I) -> I {
        assert!(o.lo > 0.0 || o.hi < 0.0, "division by {o:?}");
        let p = [
            self.lo / o.lo,
            self.lo / o.hi,
            self.hi / o.lo,
            self.hi / o.hi,
        ];
        let lo = p.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = p.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        I::new(down(lo), up(hi))
    }
}

/// `fillet1019` as a model: the frustum of the four walls drafted in by `t` from the 2W
/// square at z = 0 to height H, its x- wall's bottom and top edges and its (x-, y-) corner
/// rounded by rolling balls of radius r. Each round's gap (between its two planes and the
/// ball's cylinder) is a prism; R3 (bottom), R8 (top) run along y between the y walls, R6
/// along the corner's line (t, t, 1).
struct Model {
    w: I,
    h: I,
    t: I,
    r: I,
    s: I,  // sqrt(1 + t^2)
    d2: I, // 1 + 2 t^2
    cx3: I,
    cx8: I,
    cz8: I,
    ax: I, // the corner axis at z = 0: (ax, ax, 0)
    g: I,  // n5 . n1 = t^2 / s^2
}

impl Model {
    fn new(w: f64, h: f64, top_half: f64, r: f64) -> Self {
        let (w, h, r) = (I::p(w), I::p(h), I::p(r));
        let t = (w - I::p(top_half)) / h;
        let one = I::p(1.0);
        let s2 = one + t.sqr();
        let s = s2.sqrt0();
        let d2 = one + I::p(2.0) * t.sqr();
        let cx3 = -w + t * r + r * s;
        let cz8 = h - r;
        let cx8 = -w + t * cz8 + r * s;
        let ax = -w + r * s;
        let g = t.sqr() / s2;
        Self {
            w,
            h,
            t,
            r,
            s,
            d2,
            cx3,
            cx8,
            cz8,
            ax,
            g,
        }
    }

    /// The bottom gap's height along the line x = c + t z (parallel to the x- wall, c from
    /// -W at the wall to cx3 at the bottom tangent line): where the line meets the ball's
    /// circle, `z = e^2 / (b + sqrt(b^2 - s^2 e^2))`, `e = c - cx3 <= 0`, `b = r - t e`.
    fn h3(&self, c: I) -> I {
        let e = c - self.cx3;
        let e = I::new(e.lo, e.hi.min(0.0));
        let b = self.r - self.t * e;
        let disc = b.sqr() - (I::p(1.0) + self.t.sqr()) * e.sqr();
        e.sqr() / (b + disc.sqrt0())
    }

    /// The top gap's height along x = c + t z, from the circle up to z = H, and the part of
    /// the y walls' length 2 (W - t z) integrated over it: `(h, h (2W - t (H + z_up)))`.
    fn h8(&self, c: I) -> (I, I) {
        let one = I::p(1.0);
        let k2 = one + self.t.sqr();
        let e = c - self.cx8 + self.t * self.cz8;
        let disc = k2 * self.r.sqr() - e.sqr();
        let wv = (-(self.t * e) + disc.sqrt0()) / k2;
        let z_up = self.cz8 + wv;
        let h = self.h - z_up;
        let h = I::new(h.lo.max(0.0), h.hi.max(0.0));
        (h, h * (I::p(2.0) * self.w - self.t * (self.h + z_up)))
    }

    /// The corner gap's length along y at height 0 and x = c (the same at every height on
    /// x = c + t z: the gap is a prism along (t, t, 1)): the y with the point inside the
    /// y- wall (`Y >= -r s`), in the sector between the ball's two tangent rays
    /// (`Y >= X / g`, `Y <= g X`) and outside the ball's cylinder (`Q(Y) >= 0`), with
    /// `X = c - ax`, `Y = y - ax`.
    fn lambda(&self, c: I) -> I {
        let x = c - self.ax;
        let a = (-(self.r * self.s)).max(x / self.g);
        let b = self.g * x;
        let t2 = self.t.sqr();
        let q2 = I::p(1.0) - t2 / self.d2;
        let q1 = -(I::p(2.0) * t2 * x / self.d2);
        let q0 = x.sqr() * q2 - self.r.sqr();
        let disc = q1.sqr() - I::p(4.0) * q2 * q0;
        let root = disc.sqrt0();
        let y_minus = (-q1 - root) / (I::p(2.0) * q2);
        let y_plus = (-q1 + root) / (I::p(2.0) * q2);
        I::seg(a, b.min(y_minus)) + I::seg(a.max(y_plus), b)
    }

    /// The frustum's volume, `4 (W^3 - (W - t H)^3) / (3 t)`.
    fn frustum(&self) -> I {
        let top = self.w - self.t * self.h;
        I::p(4.0) * (self.w * self.w * self.w - top * top * top) / (I::p(3.0) * self.t)
    }
}

/// `int f` over `[a, b]`, enclosed: `n` cells, each `f` of the cell times its width.
fn integral(a: f64, b: f64, n: usize, f: impl Fn(I) -> I) -> I {
    let mut sum = I::p(0.0);
    let at = |i: usize| {
        if i == n {
            b
        } else {
            a + (b - a) * (i as f64) / (n as f64)
        }
    };
    let mut c0 = a;
    for i in 1..=n {
        let c1 = at(i);
        let cell = I::new(c0, c1);
        sum = sum + f(cell) * (I::p(c1) - I::p(c0));
        c0 = c1;
    }
    sum
}

/// The model's volumes: the file's, without the bottom round, without the top round.
struct Expected {
    base: I,
    without_bottom: I,
    without_top: I,
}

fn expected(m: &Model, n: usize) -> Expected {
    let w = -m.w.lo;
    let v_f = m.frustum();
    let v3 = integral(w, m.cx3.hi, n, |c| {
        let h = m.h3(c);
        I::p(2.0) * m.w * h - m.t * h.sqr()
    });
    let c8 = m.cx8 - m.t * m.h;
    let v8 = integral(w, c8.hi, n, |c| m.h8(c).1);
    let v6 = m.h * integral(w, m.ax.hi, n, |c| m.lambda(c));
    let v36 = integral(w, m.cx3.hi, n, |c| m.lambda(c) * m.h3(c));
    let v68 = integral(w, c8.hi, n, |c| m.lambda(c) * m.h8(c).0);
    Expected {
        base: v_f - v3 - v6 - v8 + v36 + v68,
        without_bottom: v_f - v6 - v8 + v68,
        without_top: v_f - v3 - v6 + v36,
    }
}

type V3 = [I; 3];

fn v3(x: solidcore::math::Vec3) -> V3 {
    [I::p(x.x), I::p(x.y), I::p(x.z)]
}
fn sub3(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale3(a: V3, s: I) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot3(a: V3, b: V3) -> I {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross3(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm3(a: V3) -> I {
    (a[0].sqr() + a[1].sqr() + a[2].sqr()).sqrt0()
}
fn unit3(a: V3) -> V3 {
    scale3(a, I::p(1.0) / norm3(a))
}
/// `|x|`'s upper end.
fn abs_hi(x: I) -> f64 {
    x.hi.max(-x.lo)
}

/// A plane (unit normal `n`, `n . X = d`) or a cylinder (a point `p` of its axis, unit
/// axis `u`, radius `r`), known to intervals.
enum Sup {
    Plane { n: V3, d: I },
    Cyl { p: V3, u: V3, r: I },
}

/// A face support's implicit data: the placement's normal (or axis) `x × y`, unitised. A
/// cone of semi-angle exactly 0 is its cylinder.
fn sup_of(a: &AnalyticSupport) -> Sup {
    match a {
        AnalyticSupport::Plane { placement, .. } => {
            let n = unit3(cross3(v3(placement.x_axis()), v3(placement.y_axis())));
            Sup::Plane {
                n,
                d: dot3(n, v3(placement.origin())),
            }
        }
        AnalyticSupport::Cylinder { placement, radius }
        | AnalyticSupport::Cone {
            placement,
            base_radius: radius,
            semi_angle_deg: 0.0,
        } => Sup::Cyl {
            p: v3(placement.origin()),
            u: unit3(cross3(v3(placement.x_axis()), v3(placement.y_axis()))),
            r: I::p(*radius),
        },
        other => panic!("not a plane or cylinder: {other:?}"),
    }
}

/// How far a point of `b` within `R0` of the origin can be from `a` (an upper bound, from
/// interval arithmetic): planes `|n_a - s n_b| R0 + |s d_b - d_a|`; cylinders
/// `|r_a - r_b| + |(p_b - p_a)⊥u_a| + (R0 + |p_b|) |u_b - s u_a|`, `s` the sign aligning
/// the normals (axes). `None` for different kinds.
fn bound(a: &Sup, b: &Sup) -> Option<f64> {
    const R0: f64 = 50.0;
    let sign = |x: I| {
        if x.lo + x.hi >= 0.0 {
            I::p(1.0)
        } else {
            I::p(-1.0)
        }
    };
    match (a, b) {
        (Sup::Plane { n: na, d: da }, Sup::Plane { n: nb, d: db }) => {
            let s = sign(dot3(*na, *nb));
            let tilt = norm3(sub3(*na, scale3(*nb, s)));
            Some(up(up(tilt.hi * R0) + abs_hi(s * *db - *da)))
        }
        (
            Sup::Cyl {
                p: pa,
                u: ua,
                r: ra,
            },
            Sup::Cyl {
                p: pb,
                u: ub,
                r: rb,
            },
        ) => {
            let s = sign(dot3(*ua, *ub));
            let w = sub3(*pb, *pa);
            let perp = norm3(sub3(w, scale3(*ua, dot3(w, *ua))));
            let tilt = norm3(sub3(*ub, scale3(*ua, s)));
            let reach = I::p(R0) + norm3(*pb);
            Some((abs_hi(*ra - *rb) + perp.hi + (tilt * reach).hi) * (1.0 + 4.0 * f64::EPSILON))
        }
        _ => None,
    }
}

/// The model's surfaces, named: the six planes and the three rounds' cylinders.
fn model_supports(m: &Model) -> Vec<(&'static str, Sup)> {
    let (z, one) = (I::p(0.0), I::p(1.0));
    let inv_s = one / m.s;
    let wall = |n: V3, d: I| Sup::Plane {
        n: scale3(n, inv_s),
        d: d * inv_s,
    };
    let along_y = [z, one, z];
    let corner = scale3([m.t, m.t, one], one / m.d2.sqrt0());
    vec![
        (
            "bottom",
            Sup::Plane {
                n: [z, z, one],
                d: z,
            },
        ),
        (
            "top",
            Sup::Plane {
                n: [z, z, one],
                d: m.h,
            },
        ),
        ("wall x-", wall([one, z, -m.t], -m.w)),
        ("wall x+", wall([one, z, m.t], m.w)),
        ("wall y-", wall([z, one, -m.t], -m.w)),
        ("wall y+", wall([z, one, m.t], m.w)),
        (
            "bottom round",
            Sup::Cyl {
                p: [m.cx3, z, m.r],
                u: along_y,
                r: m.r,
            },
        ),
        (
            "top round",
            Sup::Cyl {
                p: [m.cx8, z, m.cz8],
                u: along_y,
                r: m.r,
            },
        ),
        (
            "corner round",
            Sup::Cyl {
                p: [m.ax, m.ax, z],
                u: corner,
                r: m.r,
            },
        ),
    ]
}

/// Each face's support, its NURBS form proved on it ([`AnalyticSupport::check`]), matched to
/// the nearest of `named`: the name and the certified distance bound.
fn match_supports(k: &Kernel, b: &Brep, named: &[(&'static str, Sup)]) -> Vec<(&'static str, f64)> {
    bp1::faces(b, None)
        .unwrap()
        .into_iter()
        .map(|f| {
            let s = b.face(f).unwrap().surface.as_ref().unwrap();
            let sup = sup_of(&AnalyticSupport::check(k, s).unwrap());
            named
                .iter()
                .filter_map(|(name, m)| bound(m, &sup).map(|d| (*name, d)))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .unwrap()
        })
        .collect()
}

/// Which round the defeature takes off.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Removed {
    Bottom,
    Top,
}

/// The proof that `b` is `fillet1019` with the `removed` round taken off.
///
/// 1. The model is `fillet1019`: every face of the file lies on one of the model's surfaces
///    ([`model_supports`]: the frustum's planes, rolling-ball cylinders of radius 5.08 tangent
///    to their two planes) within `delta` (an interval bound, under 1e-9), and the file's
///    certified volume enclosure holds the model's ([`expected`]: the frustum less the three
///    rounds' gaps, their overlaps added back, each a one-dimensional integral enclosed
///    with outward rounding).
/// 2. `b` is sound (manifold, its Level 2 check only the curve warnings the file has too) and
///    every face of it lies on a model surface within 1e-9, none on the removed round's: the
///    walls and the bottom (top) extended, the corner round carried on by its own cylinder.
/// 3. `b`'s certified volume enclosure holds the model's volume without the removed round's
///    gap (that gap less its overlap with the corner round's), widened by the surface area
///    bound times `delta`: the two volumes differ by at most the enclosure's width (0.2).
fn certify(k: &mut Kernel, b: &mut Brep, removed: Removed) {
    const ON: f64 = 1e-9;
    const WIDTH: f64 = 0.2;
    // a bound on the area of every surface of these bodies (the frustum's is about 9700 mm^2)
    const AREA: f64 = 2e4;
    let m = Model::new(25.4, 25.4, 23.177787946441534, 5.08);
    let named = model_supports(&m);
    let e = expected(&m, 100_000);

    // 1. the model is the file
    let mut base = read(k, "Fillet/fillet1019.smb").unwrap();
    let base_match = match_supports(k, &base, &named);
    let delta = base_match.iter().map(|p| p.1).fold(0.0, f64::max);
    assert!(delta <= ON, "the file is off the model: {base_match:?}");
    let mut names: Vec<_> = base_match.iter().map(|p| p.0).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 9, "{base_match:?}");
    let grow = |x: I| I::new(down(x.lo - AREA * delta), up(x.hi + AREA * delta));
    let kb = enclose_volume(k, &mut base, WIDTH).unwrap();
    let kb = I::new(kb.lo, kb.hi);
    assert!(kb.width() <= WIDTH, "{kb:?}");
    assert!(
        kb.contains(grow(e.base)),
        "base: model {:?}, file {kb:?}",
        e.base
    );

    // 2. sound, and on the model's surfaces less the removed round
    let mut fresh = read(k, "Fillet/fillet1019.smb").unwrap();
    let base_lines = validation::soundness(k, &mut fresh).unwrap().check.lines();
    let snd = validation::soundness(k, b).unwrap();
    assert!(snd.manifold);
    // its check: warnings, and no more of each error than the file has (an error's kind is
    // its text before the measured values)
    let kind = |l: &String| l.split(" (offset").next().unwrap_or(l).to_string();
    let errors = |lines: &[String]| {
        let mut v: Vec<String> = lines
            .iter()
            .filter(|l| !l.contains("Warning"))
            .map(kind)
            .collect();
        v.sort();
        v
    };
    let (built_errors, file_errors) = (errors(&snd.check.lines()), errors(&base_lines));
    for e in &built_errors {
        let n = |v: &[String]| v.iter().filter(|x| *x == e).count();
        assert!(n(&built_errors) <= n(&file_errors), "{removed:?}: {e}");
    }
    let (gone, want) = match removed {
        Removed::Bottom => ("bottom round", e.without_bottom),
        Removed::Top => ("top round", e.without_top),
    };
    let faces = match_supports(k, b, &named);
    for (name, d) in &faces {
        assert!(*d <= ON, "{faces:?}");
        assert_ne!(*name, gone, "{faces:?}");
    }

    // 3. the volume
    let kv = enclose_volume(k, b, WIDTH).unwrap();
    let kv = I::new(kv.lo, kv.hi);
    assert!(kv.width() <= WIDTH, "{kv:?}");
    assert!(
        kv.contains(grow(want)),
        "{removed:?}: model {want:?}, built {kv:?}"
    );
    eprintln!(
        "{removed:?}: delta {delta:e}; base model {:?} file {kb:?}; model {want:?} built {kv:?}; faces {faces:?}",
        e.base
    );
}
