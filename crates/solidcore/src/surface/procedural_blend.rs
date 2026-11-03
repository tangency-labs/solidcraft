//! A blend surface stored as its definition (opt-in, not upstream): the two surfaces the
//! ball rolls on, its radius or radius law, the section's order and thumbweight rule and
//! the leader that fixes its parameter. Its net, a certified B-spline approximation, is
//! what bounds, seeds, the surface cache and tessellation use.
//!
//! Not in the pinned upstream: the SolidCraft fork's `source/SMLib/inc/SmProceduralBlend.h`,
//! `source/SMLib/src/SmProceduralBlend.cpp` and `source/SMLib/inc/SmBlendNetScope.h`
//! (tangency-labs/smlib@014f66f, c6d9b9a and bb9f45e, smlib#79, merged at 5e8c9c1). Only
//! [`crate::kernel::UpstreamFixes::fillet_round_as_definition`] builds or reads one.
//!
//! | Rust                                         | Fork                                         |
//! |----------------------------------------------|----------------------------------------------|
//! | [`ProceduralBlend`]                          | `SmProceduralBlend`                          |
//! | [`BlendDefinition`]                          | its definition members (`m_apSupport` .. `m_bSwapped`) |
//! | [`ProceduralBlend::create`]                  | `SmProceduralBlend::Create`                  |
//! | [`ProceduralBlend::net`]                     | the `SmBSplineSurface` base (its evaluators called explicitly) |
//! | [`ProceduralBlend::depth`]                   | `GetDepth`                                   |
//! | [`ProceduralBlend::definition_parameters`]   | `DefinitionParameters`                       |
//! | `reparametrize_map` (private)                | `ReparametrizeMap`                           |
//! | [`ProceduralBlend::toggle_swap_uv_bit`]      | `ToggleSwapUVBit`                            |
//! | [`ProceduralBlend::same_definition`]         | `operator==` (past the net's)                |
//! | [`ProceduralBlend::read_from_db`], `read_curve` (private) | `ReadFromDB`, `sm_ReadCurve`     |
//! | [`ProceduralBlend::evaluate_definition`]     | `EvaluateDefinition` ([`crate::blend::fillet_section::definition`]) |
//! | [`PROCEDURAL_BLEND_TYPE`], [`BLEND_MAX_DEPTH`] | `SmProceduralBlend_TYPE`, `SM_BLEND_MAX_DEPTH` |
//! | [`NetScope`], [`NetScope::is_active`]        | `SmBlendNetScope`, `IsActive`                |
//! | [`ProceduralBlend::recertify`], [`recertify_faces`] | not in the fork (#2388; the Ball's `SmBallPatch::IsOnSphere` is its counterpart) |
//! | [`record`], [`convert`]                      | not in the fork (#2388: the definition as plain data) |
//! | [`chamfer`], order 0                         | not in the fork (#2486: a chamfer, the chord, its sizes kept) |
//! | [`BlendCounts`]                              | `SmBlendCounts` (`GetCounts`, `ResetCounts` on [`crate::kernel::Kernel`]) |
//!
//! Deviations:
//! - **Kernel state.** The fork's `thread_local` net-scope depth and its process-wide
//!   evaluation counts are fields of [`crate::kernel::Kernel`] (`blend_net_scope`,
//!   `blend_counts`). They are `Cell`s: the evaluators that read the scope and count their
//!   work take `&Kernel` (every surface evaluation does), so they cannot take
//!   `&mut Kernel`. A `Cell` keeps the kernel `Send`, and the values are deterministic
//!   (one kernel, one thread at a time).
//! - **Identity.** The fork's serial (`m_lSerial`, from the process-wide `s_lLastSerial`)
//!   is a [`DefinitionIdentity`] the definition holds, compared by identity: unique
//!   among every definition alive in any kernel, so the station memo of one kernel never
//!   takes the station of a definition another kernel (another thread) made (#2388).
//! - **Ownership.** The supports, the leader, the seeds and the law are owned values, not
//!   pointers; a copy is a deep copy, as the fork's `CopyDefinition`.
//! - **Nesting.** A support may itself be a procedural blend, at most
//!   [`BLEND_MAX_DEPTH`] deep (the fork's cap, checked at [`ProceduralBlend::create`]),
//!   so every walk over the supports is bounded by 3.

pub mod chamfer;
pub mod convert;
pub mod corner;
pub mod exact;
pub mod record;

use std::cell::Cell;
use std::sync::Arc;

use crate::blend::fillet_section::section::GnSectionFsg;
use crate::curve::curve::{Curve, CurveKind};
use crate::error::{Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent2d::Extent2d;
use crate::geom::tol::are_same;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::{Point2, EFF_ZERO};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::{
    BSplineSurface, BoundingBoxRequest, BoundingBoxes, SurfParamType,
};
use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use crate::nurbs::knots::Side;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::database_io::{DatabaseReader, FileType, InputStream};
use chamfer::{ChamferDefinition, ChamferSize};

const OP_CREATE: &str = "surface::procedural_blend::ProceduralBlend::create";
const OP_EVAL: &str = "surface::procedural_blend::ProceduralBlend::evaluate";
const OP_EVAL_SIMPLE: &str = "surface::procedural_blend::ProceduralBlend::evaluate_simple";
const OP_REPARAM: &str = "surface::procedural_blend::ProceduralBlend::reparameterize";
const OP_TRANSFORM: &str = "surface::procedural_blend::ProceduralBlend::transform";
const OP_READ: &str = "surface::procedural_blend::ProceduralBlend::read_from_db";
const OP_RECERTIFY: &str = "surface::procedural_blend::recertify_faces";

/// `SURF_BASE_TYPE` (`SmTypes.h`).
const SURF_BASE_TYPE: i64 = 15000;

/// `SmProceduralBlend_TYPE` (`SURF_BASE_TYPE + 41`, the fork's `SmSurfTypes.h`).
pub const PROCEDURAL_BLEND_TYPE: i64 = SURF_BASE_TYPE + 41;

/// `SM_BLEND_MAX_DEPTH`: the most definitions a blend's supports may nest (a round on a
/// round on a round).
pub const BLEND_MAX_DEPTH: u32 = 3;

/// `SM_GN_MAX_ORDER`: the highest section order of a definition.
const MAX_ORDER: u32 = 4;

/// Validates a section order and thumbweight for a definition: the chord (0, not the
/// fork's, #2486), the ball's circle (1), or a G^n section (2 to 4) whose thumbweight
/// [`GnSectionFsg::is_valid`] takes.
#[must_use]
pub fn order_is_valid(order: u32, thumbweight: f64) -> bool {
    order <= 1 || GnSectionFsg::is_valid(order as usize, thumbweight)
}

/// `SmBlendCounts`: the work the definitions of a kernel's procedural blends did, machine
/// independent (unlike timings). Diagnostics only: no result reads them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BlendCounts {
    /// `lEvaluations`: points evaluated on a definition.
    pub evaluations: u64,
    /// `lSolves`: station solves (the spine point and the feet).
    pub solves: u64,
    /// `lNewtonSteps`: the Newton steps of those solves.
    pub newton_steps: u64,
    /// `lMostNewtonSteps`: the most one solve took.
    pub most_newton_steps: u64,
    /// `lAreaSums`: the equal area thumbweight's area sums I(w) a G^n certificate summed
    /// outside its cells (at a point of w, or over an interval of w off the cells' grid;
    /// b64eb30, smlib#109).
    pub area_sums: u64,
    /// `lAreaRoots`: the equal area thumbweights solved for (each bisects
    /// `I(w) = target(u)` on the kernel's first meeting of that order and u in an
    /// operation; fbba0e2, smlib#109).
    pub area_roots: u64,
}

/// `SmBlendNetScope`: while one lives, every procedural blend's `Evaluate`,
/// `EvaluateSimple` and `EvaluatePoint` on the kernel are its net's, within the net's
/// certified bound of the definition. The tessellator and the surface cache hold one.
/// Scopes nest; the kernel leaves the net's evaluation when the last one ends.
#[derive(Debug)]
pub struct NetScope<'k> {
    depth: &'k Cell<u32>,
}

impl<'k> NetScope<'k> {
    /// Enters a scope on `k` (`SmBlendNetScope()`).
    #[must_use]
    pub fn new(k: &'k Kernel) -> Self {
        let depth = &k.blend_net_scope;
        depth.set(depth.get().saturating_add(1));
        Self { depth }
    }

    /// `IsActive`: whether `k` is inside a scope.
    #[must_use]
    pub fn is_active(k: &Kernel) -> bool {
        k.blend_net_scope.get() > 0
    }
}

impl Drop for NetScope<'_> {
    fn drop(&mut self) {
        self.depth.set(self.depth.get().saturating_sub(1));
    }
}

/// The identity of a definition for the station memo (the fork's `m_lSerial`): a
/// definition made, copied or moved takes a new one, and two are the same only when they
/// are one allocation ([`Arc::ptr_eq`]). The memo holds one while it keeps a station, so
/// an identity is never reused while a station is kept for it.
#[derive(Debug, Default)]
pub struct DefinitionIdentity;

/// A blend's definition (the members of `SmProceduralBlend` past its net). A clone
/// keeps the identity: it is the same definition (the fork's copy constructor renews it,
/// which only matters once one of the two moves, and every move here renews).
#[derive(Debug, Clone)]
pub struct BlendDefinition {
    /// `m_apSupport`: the surfaces S1, S2 the ball rolls on (owned copies).
    pub(crate) supports: [Box<Surface>; 2],
    /// `m_adSide`: the ball's side of each support, +-1 along its normal.
    pub(crate) sides: [f64; 2],
    /// `m_adR`: the ball's offsets from S1 and S2 (its radius, both), when there is no law.
    pub(crate) offsets: [f64; 2],
    /// `m_pLaw`: the radius r(s) as the x of a curve over the leader's parameter, or none.
    pub(crate) law: Option<BSplineCurve>,
    /// `m_lOrder`: the section's order n, 0 to 4 (0 the chord, a chamfer: not the fork's,
    /// #2486).
    pub(crate) order: u32,
    /// `m_dThumbweight`: n >= 2, the G^n section's thumbweight rule (0 for n = 1).
    pub(crate) thumbweight: f64,
    /// `m_pLeader`: the leader L(s).
    pub(crate) leader: BSplineCurve,
    /// `m_apSeed`: the feet's uv on each support, over s.
    pub(crate) seeds: [BSplineCurve; 2],
    /// `m_dBound`: the net's certified distance from the definition, > 0.
    pub(crate) bound: f64,
    /// A chamfer's sizes (order 0 only; not the fork's, #2486): what its offsets were
    /// asked as.
    pub(crate) chamfer: Option<ChamferDefinition>,
    /// `m_adA`: `(s, t) = (a[0] + b[0] p, a[1] + b[1] q)`.
    pub(crate) map_a: [f64; 2],
    /// `m_adB`.
    pub(crate) map_b: [f64; 2],
    /// `m_bSwapped`: `(p, q)` is `(v, u)`.
    pub(crate) swapped: bool,
    /// `m_lSerial`: the definition's identity for the station memo, new whenever the
    /// definition is made or moved (`Renew`), never reused while it is held.
    pub(crate) identity: Arc<DefinitionIdentity>,
}

/// The inputs of [`ProceduralBlend::create`] (`SmProceduralBlend::Create`'s arguments).
#[derive(Debug, Clone, Copy)]
pub struct BlendDefinitionInput<'a> {
    /// `apSupport`.
    pub supports: [&'a Surface; 2],
    /// `crSide`.
    pub sides: [f64; 2],
    /// `crOffset`: the ball's offsets from S1 and S2, when `law` is none.
    pub offsets: [f64; 2],
    /// `cpLaw`.
    pub law: Option<&'a BSplineCurve>,
    /// `lOrder`.
    pub order: u32,
    /// `dThumbweight`.
    pub thumbweight: f64,
    /// `crLeader`.
    pub leader: &'a BSplineCurve,
    /// `apSeed`.
    pub seeds: [&'a BSplineCurve; 2],
    /// `dBound`.
    pub bound: f64,
    /// A chamfer's sizes: present exactly for order 0, its offsets `offsets` (#2486).
    pub chamfer: Option<ChamferDefinition>,
}

/// `SmProceduralBlend`: a blend of order n between two surfaces, by its definition, on
/// its net (the `SmBSplineSurface` it derives from).
#[derive(Debug, Clone)]
pub struct ProceduralBlend {
    /// The net: the certified B-spline approximation (the base class).
    pub(crate) net: BSplineSurface,
    /// The definition.
    pub(crate) def: BlendDefinition,
}

/// `sm_ReadCurve`: a curve of a definition (its type header with its dimension, then its
/// record), which must be a B-spline; it evaluates past its ends.
fn read_curve(k: &Kernel, db: &mut DatabaseReader, version: u64) -> Result<BSplineCurve> {
    let header = db.read_type(true)?;
    let ty = header
        .type_code
        .ok_or_else(|| SolidError::invalid_input(OP_READ, "curve type not read"))?;
    let curve = Curve::read_from_db(k, ty, db, header.dim.unwrap_or(0), version)?;
    match curve.into_kind() {
        CurveKind::BSpline(mut b) => {
            b.set_out_of_bounds_enabled(true);
            Ok(b)
        }
        // "a curve of its definition is not a B-spline"
        _ => Err(SolidError::sm_err(OP_READ)),
    }
}

/// `sm_CopyCurve`: a copy of a curve that evaluates past its ends on its end spans.
fn copy_curve(curve: &BSplineCurve) -> BSplineCurve {
    let mut copy = curve.copy();
    copy.set_out_of_bounds_enabled(true);
    copy
}

/// A chamfer's sizes as the database holds them (#2486): kind 0 two distances, 1 a
/// distance and an angle, 2 a chord (its one size the first), then the wedge.
fn chamfer_of_db(kind: i64, sizes: [f64; 3]) -> Result<ChamferDefinition> {
    let [a, b, wedge] = sizes;
    let size = match kind {
        0 => ChamferSize::Distances([a, b]),
        1 => ChamferSize::DistanceAngle {
            distance: a,
            angle: b,
        },
        2 => ChamferSize::Chord(a),
        _ => {
            return Err(SolidError::invalid_input(
                OP_READ,
                "a chamfer's sizes are two distances, a distance and an angle, or a chord",
            ))
        }
    };
    ChamferDefinition::new(size, wedge)
}

/// The database's numbers of a chamfer's sizes ([`chamfer_of_db`]'s inverse).
#[must_use]
pub fn chamfer_to_db(c: &ChamferDefinition) -> (i64, [f64; 3]) {
    match c.size {
        ChamferSize::Distances([a, b]) => (0, [a, b, c.wedge]),
        ChamferSize::DistanceAngle { distance, angle } => (1, [distance, angle, c.wedge]),
        ChamferSize::Chord(l) => (2, [l, 0.0, c.wedge]),
    }
}

/// `GetDepth` of a support: 1 more than a procedural support's depth, else 0.
fn support_depth(surface: &Surface) -> u32 {
    match surface.kind() {
        SurfaceKind::ProceduralBlend(p) => p.depth(),
        _ => 0,
    }
}

/// `CountEvaluation`: one point evaluated on a definition.
pub(crate) fn count_evaluation(k: &Kernel) {
    let mut c = k.blend_counts.get();
    c.evaluations = c.evaluations.wrapping_add(1);
    k.blend_counts.set(c);
}

/// `CountSolve`: one station solve of `newton_steps` Newton steps.
pub(crate) fn count_solve(k: &Kernel, newton_steps: u32) {
    let mut c = k.blend_counts.get();
    c.solves = c.solves.wrapping_add(1);
    c.newton_steps = c.newton_steps.wrapping_add(u64::from(newton_steps));
    if u64::from(newton_steps) > c.most_newton_steps {
        c.most_newton_steps = u64::from(newton_steps);
    }
    k.blend_counts.set(c);
}

/// `CountAreaSum`: one area sum a certificate summed outside its cells.
pub(crate) fn count_area_sum(k: &Kernel) {
    let mut c = k.blend_counts.get();
    c.area_sums = c.area_sums.wrapping_add(1);
    k.blend_counts.set(c);
}

/// `CountAreaRoot`: one equal area thumbweight solved for.
pub(crate) fn count_area_root(k: &Kernel) {
    let mut c = k.blend_counts.get();
    c.area_roots = c.area_roots.wrapping_add(1);
    k.blend_counts.set(c);
}

/// `++s_lLastSerial`: a new identity.
fn new_identity() -> Arc<DefinitionIdentity> {
    Arc::new(DefinitionIdentity)
}

impl ProceduralBlend {
    /// `SmProceduralBlend::Create`: a blend by its definition on the net `net`, every
    /// input copied.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] (`SM_ERR_INVALID_INPUT`, named) for an order out of
    /// 1 to 4, n >= 2 with a law or an invalid thumbweight, sides other than +-1,
    /// offsets or a bound not finite and positive, a circle's offsets that differ, and
    /// supports nested deeper than [`BLEND_MAX_DEPTH`]; those of copying a support.
    pub fn create(
        k: &Kernel,
        net: &BSplineSurface,
        input: &BlendDefinitionInput<'_>,
        thumbweight_is_valid: impl Fn(u32, f64) -> bool,
    ) -> Result<Self> {
        let order = input.order;
        if order > MAX_ORDER {
            return Err(SolidError::invalid_input(
                OP_CREATE,
                "a section's order is 0 to 4",
            ));
        }
        // the chord is a chamfer: its sizes, of a constant pair of offsets (#2486)
        let chamfer_held = match (&input.chamfer, order, input.law) {
            (Some(c), 0, None) => c.holds(input.offsets),
            (None, 1..=MAX_ORDER, _) => true,
            _ => false,
        };
        if !chamfer_held {
            return Err(SolidError::invalid_input(
                OP_CREATE,
                "a chord is a chamfer of two offsets its sizes give, and only a chord is",
            ));
        }
        if order >= 2 && (input.law.is_some() || !thumbweight_is_valid(order, input.thumbweight)) {
            return Err(SolidError::invalid_input(
                OP_CREATE,
                "a G^n section needs a ball of one radius and a thumbweight below 1 or the equal area rule",
            ));
        }
        for side in input.sides {
            if side != 1.0 && side != -1.0 {
                return Err(SolidError::invalid_input(
                    OP_CREATE,
                    "a support's side is +1 or -1",
                ));
            }
        }
        if input.law.is_none() {
            for offset in input.offsets {
                if !(offset.is_finite() && offset > 0.0) {
                    return Err(SolidError::invalid_input(
                        OP_CREATE,
                        "the ball's radius is finite and positive",
                    ));
                }
            }
            if order == 1 && input.offsets[0] != input.offsets[1] {
                return Err(SolidError::invalid_input(
                    OP_CREATE,
                    "a circle's ball has one radius",
                ));
            }
        }
        if !(input.bound.is_finite() && input.bound > 0.0) {
            return Err(SolidError::invalid_input(
                OP_CREATE,
                "the net's bound is finite and positive",
            ));
        }
        let mut depth = 1;
        for support in input.supports {
            let inner = support_depth(support);
            if inner > 0 {
                depth = depth.max(inner + 1);
            }
        }
        if depth > BLEND_MAX_DEPTH {
            return Err(SolidError::invalid_input(
                OP_CREATE,
                "blends nest at most 3 definitions deep",
            ));
        }
        let [s0, s1] = input.supports;
        let supports = [Box::new(s0.copy(k)?), Box::new(s1.copy(k)?)];
        let [seed0, seed1] = input.seeds;
        let def = BlendDefinition {
            supports,
            sides: input.sides,
            offsets: if input.law.is_none() {
                input.offsets
            } else {
                [0.0, 0.0]
            },
            law: input.law.map(copy_curve),
            order,
            thumbweight: if order >= 2 { input.thumbweight } else { 0.0 },
            leader: copy_curve(input.leader),
            seeds: [copy_curve(seed0), copy_curve(seed1)],
            bound: input.bound,
            chamfer: input.chamfer,
            map_a: [0.0, 0.0],
            map_b: [1.0, 1.0],
            swapped: false,
            identity: new_identity(),
        };
        Ok(Self {
            net: net.copy(),
            def,
        })
    }

    /// The net (the `SmBSplineSurface` base, whose evaluators the fork calls explicitly).
    #[must_use]
    pub const fn net(&self) -> &BSplineSurface {
        &self.net
    }

    /// The net, mutably.
    pub(crate) fn net_mut(&mut self) -> &mut BSplineSurface {
        &mut self.net
    }

    /// The definition.
    #[must_use]
    pub const fn definition(&self) -> &BlendDefinition {
        &self.def
    }

    /// `GetDepth`: how deep definitions nest here, 1 or one more than the deepest
    /// procedural support (bounded by [`BLEND_MAX_DEPTH`], checked at creation).
    #[must_use]
    pub fn depth(&self) -> u32 {
        let mut depth = 1;
        for support in &self.def.supports {
            let inner = support_depth(support);
            if inner > 0 {
                depth = depth.max(inner + 1);
            }
        }
        depth
    }

    /// `GetBound`: the net's certified distance from the definition.
    #[must_use]
    pub const fn bound(&self) -> f64 {
        self.def.bound
    }

    /// `GetOrder`: the section's order n, 0 (the chord, #2486) to 4.
    #[must_use]
    pub const fn order(&self) -> u32 {
        self.def.order
    }

    /// A chamfer's sizes (order 0, #2486): how its offsets were asked for.
    #[must_use]
    pub const fn chamfer(&self) -> Option<&ChamferDefinition> {
        self.def.chamfer.as_ref()
    }

    /// `GetSupport(i)`: the surface S1 (0) or S2 (1) the ball rolls on.
    #[must_use]
    pub fn support(&self, i: usize) -> Option<&Surface> {
        self.def.supports.get(i).map(AsRef::as_ref)
    }

    /// `GetSeed(i)`: the feet's uv on support `i` over s.
    #[must_use]
    pub fn seed(&self, i: usize) -> Option<&BSplineCurve> {
        self.def.seeds.get(i)
    }

    /// `GetLeader`: the leader L(s).
    #[must_use]
    pub const fn leader(&self) -> &BSplineCurve {
        &self.def.leader
    }

    /// `GetLaw`: the radius law r(s), or none (a ball of one radius).
    #[must_use]
    pub const fn law(&self) -> Option<&BSplineCurve> {
        self.def.law.as_ref()
    }

    /// `GetOffset(i)`: the ball's offset from support `i` (0 with a law).
    #[must_use]
    pub fn offset(&self, i: usize) -> Option<f64> {
        self.def.offsets.get(i).copied()
    }

    /// `GetSide(i)`: the ball's side of support `i`, +-1 along its normal.
    #[must_use]
    pub fn side(&self, i: usize) -> Option<f64> {
        self.def.sides.get(i).copied()
    }

    /// `DefinitionParameters`: the definition's (s, t) at the surface's (u, v).
    #[must_use]
    pub fn definition_parameters(&self, uv: Point2) -> Point2 {
        let d = &self.def;
        let (p, q) = if d.swapped {
            (uv.y, uv.x)
        } else {
            (uv.x, uv.y)
        };
        Point2::new(d.map_a[0] + d.map_b[0] * p, d.map_a[1] + d.map_b[1] * q)
    }

    /// `ReparametrizeMap`: `u = alpha + beta u'` (`u`) or the same in v, the map's terms
    /// after the net's reparametrization.
    pub(crate) fn reparametrize_map(&mut self, u: bool, alpha: f64, beta: f64) {
        // u drives s unless swapped
        let idx = usize::from(u == self.def.swapped);
        if let (Some(a), Some(b)) = (
            self.def.map_a.get_mut(idx),
            self.def.map_b.get(idx).copied(),
        ) {
            *a += b * alpha;
        }
        if let Some(b) = self.def.map_b.get_mut(idx) {
            *b *= beta;
        }
    }

    /// `ToggleSwapUVBit`: `SwapUV` calls this once per swap of the net's parameters.
    pub fn toggle_swap_uv_bit(&mut self) {
        self.def.swapped = !self.def.swapped;
    }

    /// `operator==` past the net's comparison (`SmBSplineSurface::operator==`, which the
    /// caller makes): the same supports, seeds, leader, law, sides, offsets, map, order,
    /// thumbweight rule and bound (`sm_SameCurve`: both missing or equal curves).
    ///
    /// # Errors
    ///
    /// Those of comparing the supports ([`Surface::approx_eq`]).
    pub fn same_definition(&self, k: &Kernel, other: &Self) -> Result<bool> {
        let (a, b) = (&self.def, &other.def);
        for i in 0..2 {
            let (Some(sa), Some(sb)) = (a.supports.get(i), b.supports.get(i)) else {
                return Ok(false);
            };
            if !sa.approx_eq(k, sb)? {
                return Ok(false);
            }
            let same_seed = match (a.seeds.get(i), b.seeds.get(i)) {
                (Some(x), Some(y)) => x.approx_eq(y),
                _ => false,
            };
            if !same_seed
                || a.sides.get(i) != b.sides.get(i)
                || a.offsets.get(i) != b.offsets.get(i)
                || a.map_a.get(i) != b.map_a.get(i)
                || a.map_b.get(i) != b.map_b.get(i)
            {
                return Ok(false);
            }
        }
        let same_law = match (&a.law, &b.law) {
            (None, None) => true,
            (Some(x), Some(y)) => x.approx_eq(y),
            _ => false,
        };
        Ok(a.leader.approx_eq(&b.leader)
            && same_law
            && a.order == b.order
            && a.thumbweight == b.thumbweight
            && a.bound == b.bound
            && a.chamfer == b.chamfer
            && a.swapped == b.swapped)
    }

    /// `SmProceduralBlend_TYPE`.
    #[must_use]
    pub const fn type_code(&self) -> i64 {
        PROCEDURAL_BLEND_TYPE
    }

    /// `"SmProceduralBlend_TYPE"` (`GetTypeString`).
    #[must_use]
    pub const fn type_string(&self) -> &'static str {
        "SmProceduralBlend_TYPE"
    }

    /// `"SmProceduralBlend"` (`GetClassString`).
    #[must_use]
    pub const fn class_string(&self) -> &'static str {
        "SmProceduralBlend"
    }

    /// `IsKindOf`: this type, or the net's (`SmBSplineSurface` and its bases).
    ///
    /// # Errors
    ///
    /// Those of the net's `IsKindOf`.
    pub fn is_kind_of(&self, t: i64) -> Result<bool> {
        if t == PROCEDURAL_BLEND_TYPE {
            return Ok(true);
        }
        self.net.is_kind_of(t)
    }

    /// `Renew`: a new identity, so the station memo never takes another definition's.
    pub(crate) fn renew(&mut self) {
        self.def.identity = new_identity();
    }

    /// `Copy` (the copy constructor and `CopyDefinition`): a deep copy whose definition
    /// takes a new identity (the kernel, which the surfaces' copies take, is not needed).
    #[must_use]
    pub fn copy(&self, _k: &Kernel) -> Self {
        let mut c = Self {
            net: self.net.copy(),
            def: self.def.clone(),
        };
        c.renew();
        c
    }

    /// An evaluation copy (the supports copied as evaluation copies, one level deeper).
    ///
    /// # Errors
    ///
    /// Those of the supports' evaluation copies (nesting past
    /// [`crate::surface::surface::MAX_GEOMETRY_NESTING`]).
    pub(crate) fn clone_for_evaluation(&self, depth: usize) -> Result<Self> {
        let [a, b] = &self.def.supports;
        let supports = [
            Box::new(a.clone_for_evaluation_depth(depth)?),
            Box::new(b.clone_for_evaluation_depth(depth)?),
        ];
        Ok(Self {
            net: self.net.clone_exact(),
            def: self.def.with_supports(supports),
        })
    }

    /// An exact copy after the caller validated the geometry graph.
    pub(crate) fn clone_exact_bounded(&self) -> Self {
        let [a, b] = &self.def.supports;
        Self {
            net: self.net.clone_exact(),
            def: self.def.with_supports([
                Box::new(a.clone_exact_bounded()),
                Box::new(b.clone_exact_bounded()),
            ]),
        }
    }

    /// The supports (for walks over the owned geometry).
    pub(crate) fn supports(&self) -> [&Surface; 2] {
        let [a, b] = &self.def.supports;
        [a, b]
    }

    /// The supports, mutably.
    pub(crate) fn supports_mut(&mut self) -> Vec<&mut Surface> {
        let [a, b] = &mut self.def.supports;
        vec![a.as_mut(), b.as_mut()]
    }

    /// `EvaluateDefinition`: the definition's partials at `(s, t)`,
    /// `d[i][j] = d^(i+j) S / ds^i dt^j`, `i <= highest_s`, `j <= highest_t`
    /// ([`crate::blend::fillet_section::definition::evaluate_definition`]), on the side
    /// after s where s is one of its stations (`bBefore` FALSE).
    ///
    /// # Errors
    ///
    /// Those of [`crate::blend::fillet_section::definition::evaluate_definition`].
    pub fn evaluate_definition(
        &self,
        k: &Kernel,
        s: f64,
        t: f64,
        highest_s: usize,
        highest_t: usize,
    ) -> Result<Vec<Vec<Vec3>>> {
        self.evaluate_definition_sided(k, s, t, highest_s, highest_t, false)
    }

    /// `EvaluateDefinition` with its `bBefore` (c5b6bb7): where s is one of its stations
    /// (a support's knot line under a foot, a knot of the leader or the law) the
    /// definition is only as smooth as they are there, its partials on the side after s,
    /// or with `before` on the side before it.
    ///
    /// # Errors
    ///
    /// Those of [`crate::blend::fillet_section::definition::evaluate_definition`].
    pub fn evaluate_definition_sided(
        &self,
        k: &Kernel,
        s: f64,
        t: f64,
        highest_s: usize,
        highest_t: usize,
        before: bool,
    ) -> Result<Vec<Vec<Vec3>>> {
        crate::blend::fillet_section::definition::evaluate_definition(
            k, self, s, t, highest_s, highest_t, before,
        )
    }

    /// `GetDefinitionMap` (c5b6bb7): the map's terms, `(s, t) = (a[0] + b[0] p,
    /// a[1] + b[1] q)`, `(p, q)` the surface's `(u, v)`, or `(v, u)` when swapped.
    #[must_use]
    pub const fn definition_map(&self) -> ([f64; 2], [f64; 2], bool) {
        (self.def.map_a, self.def.map_b, self.def.swapped)
    }

    /// `ReadFromDB`: a blend's definition and net, as [`crate::topology::geometry_write`]
    /// writes them (the fork's `SmProceduralBlend::WriteToDB`). A hostile record (an order
    /// out of range, a side other than +-1, a radius or bound not positive, a number not
    /// finite, a singular map, a depth past the cap) is refused as `Create` refuses it.
    /// The supports are read through [`Surface::read_from_db`], which bounds the nesting
    /// ([`DatabaseReader::enter_nested`]); nothing is allocated from a count in the file.
    ///
    /// # Errors
    ///
    /// `SM_ERR` where the ASCII record cannot be read or a curve of the definition is not
    /// a B-spline; `SM_ERR_INVALID_INPUT` for a negative order, a number not finite, a
    /// singular map and those of [`ProceduralBlend::create`]; those of the readers.
    pub fn read_from_db(k: &Kernel, db: &mut DatabaseReader, version: u64) -> Result<Self> {
        let ascii = db.file_type() == FileType::Ascii;
        let (order, swapped, has_law, values) = if ascii {
            let st = db
                .in_stream_mut()
                .ok_or_else(|| SolidError::null_pointer(OP_READ))?;
            let order = st.extract_ulong();
            let swapped = st.extract_int();
            let has_law = st.extract_int();
            db.go_to_next_line();
            let mut values = [0.0_f64; 10];
            let st = db
                .in_stream_mut()
                .ok_or_else(|| SolidError::null_pointer(OP_READ))?;
            let mut read = true;
            for v in &mut values {
                match st.extract_double() {
                    Some(x) => *v = x,
                    None => read = false,
                }
            }
            db.go_to_next_line();
            let failed = db.in_stream().is_none_or(InputStream::fail);
            match (order, swapped, has_law) {
                (Some(o), Some(s), Some(h)) if read && !failed => (o, s != 0, h != 0, values),
                // "its definition could not be read"
                _ => return Err(SolidError::sm_err(OP_READ)),
            }
        } else {
            let order = db.read_long()?;
            if order < 0 {
                return Err(SolidError::invalid_input(
                    OP_READ,
                    "a section's order is 1 to 4",
                ));
            }
            let swapped = db.read_boolean()? != 0;
            let has_law = db.read_boolean()? != 0;
            let v = db.read_doubles(10)?;
            let mut values = [0.0_f64; 10];
            for (x, &y) in values.iter_mut().zip(&v) {
                *x = y;
            }
            (order as u64, swapped, has_law, values)
        };
        // a chord's sizes (order 0, not the fork's, #2486): its kind, two sizes and wedge
        let chamfer = if order == 0 {
            let (kind, sizes) = if ascii {
                let st = db
                    .in_stream_mut()
                    .ok_or_else(|| SolidError::null_pointer(OP_READ))?;
                let kind = st.extract_int();
                let mut sizes = [0.0_f64; 3];
                let mut read = true;
                for v in &mut sizes {
                    match st.extract_double() {
                        Some(x) => *v = x,
                        None => read = false,
                    }
                }
                db.go_to_next_line();
                let failed = db.in_stream().is_none_or(InputStream::fail);
                match kind {
                    Some(kd) if read && !failed => (i64::from(kd), sizes),
                    // "its definition could not be read"
                    _ => return Err(SolidError::sm_err(OP_READ)),
                }
            } else {
                let kind = db.read_long()?;
                let v = db.read_doubles(3)?;
                let mut sizes = [0.0_f64; 3];
                for (x, &y) in sizes.iter_mut().zip(&v) {
                    *x = y;
                }
                (kind, sizes)
            };
            Some(chamfer_of_db(kind, sizes)?)
        } else {
            None
        };
        let mut supports = Vec::new();
        for _ in 0..2 {
            if ascii {
                db.go_to_next_line();
            }
            let ty = db
                .read_type(false)?
                .type_code
                .ok_or_else(|| SolidError::invalid_input(OP_READ, "surface type not read"))?;
            supports.push(Surface::read_from_db(k, ty, db, version)?);
        }
        if ascii {
            db.go_to_next_line();
        }
        let leader = read_curve(k, db, version)?;
        let mut seeds = Vec::new();
        for _ in 0..2 {
            if ascii {
                db.go_to_next_line();
            }
            seeds.push(read_curve(k, db, version)?);
        }
        let law = if has_law {
            if ascii {
                db.go_to_next_line();
            }
            Some(read_curve(k, db, version)?)
        } else {
            None
        };
        let net = BSplineSurface::read_from_db(db, version)?;
        if values.iter().any(|v| !v.is_finite()) {
            return Err(SolidError::invalid_input(
                OP_READ,
                "its definition holds a number that is not finite",
            ));
        }
        let [side0, side1, r0, r1, thumbweight, bound, a0, b0, a1, b1] = values;
        if !(b0 != 0.0 && b1 != 0.0) {
            return Err(SolidError::invalid_input(
                OP_READ,
                "its parametrization is singular",
            ));
        }
        let (Some(s0), Some(s1), Some(seed0), Some(seed1)) = (
            supports.first(),
            supports.get(1),
            seeds.first(),
            seeds.get(1),
        ) else {
            return Err(SolidError::sm_err(OP_READ));
        };
        let input = BlendDefinitionInput {
            supports: [s0, s1],
            sides: [side0, side1],
            offsets: [r0, r1],
            law: law.as_ref(),
            // (a ULONG past u32 is no order: Create refuses it)
            order: u32::try_from(order).unwrap_or(u32::MAX),
            thumbweight,
            leader: &leader,
            seeds: [seed0, seed1],
            bound,
            chamfer,
        };
        let mut blend = Self::create(k, &net, &input, |n, w| {
            GnSectionFsg::is_valid(n as usize, w)
        })?;
        blend.def.map_a = [a0, a1];
        blend.def.map_b = [b0, b1];
        blend.def.swapped = swapped;
        Ok(blend)
    }

    /// Re-certifies the net against the definition (#2388): proves the net within the
    /// definition's bound of S again, as a blend rebuilt from a record must be before it
    /// is an operand ([`crate::blend::fillet_section::recertify`]). Returns the distance
    /// proved, at most the bound.
    ///
    /// # Errors
    ///
    /// Those of [`crate::blend::fillet_section::recertify::recertify`]: a net not proved
    /// within the bound, or not a net of its definition's stations, is refused.
    pub fn recertify(&self, k: &Kernel) -> Result<f64> {
        crate::blend::fillet_section::recertify::recertify(k, self)
    }

    /// The floor of the bound (#2388): a forward error bound, at part scale at rounding
    /// level, of how S and the net are computed
    /// ([`crate::blend::fillet_section::recertify::precision_floor`]).
    ///
    /// # Errors
    ///
    /// Those of the supports' and the net's NURBS data.
    pub fn precision_floor(&self, k: &Kernel) -> Result<f64> {
        crate::blend::fillet_section::recertify::precision_floor(
            k,
            self.supports(),
            self.def.order,
            &self.net,
        )
    }

    /// `Evaluate`: the definition's partials at `uv` in the surface's parameters (the
    /// net's inside a [`NetScope`]). `uv` is clamped to the domain as the net's
    /// evaluation clamps it; with `non_zero_tangents`, a first partial that vanishes
    /// (a degenerate section) is replaced by the net's.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for `only_upper_half` with different orders; those of the
    /// definition's and the net's evaluations.
    pub fn evaluate(
        &self,
        k: &Kernel,
        uv: Point2,
        highest_u_deriv: usize,
        highest_v_deriv: usize,
        flags: EvaluateFlags,
    ) -> Result<Vec<Vec<Vec3>>> {
        // meshing and display (SmBlendNetScope) take the net, the approximation made for them
        if NetScope::is_active(k) {
            return self
                .net
                .evaluate(uv, highest_u_deriv, highest_v_deriv, flags);
        }
        if flags.only_upper_half && highest_u_deriv != highest_v_deriv {
            return Err(SolidError::invalid_input(
                OP_EVAL,
                "only the upper half needs equal orders",
            ));
        }
        let mut s_uv = uv;
        let domain = self.net.natural_uv_domain();
        if !domain.contains_point(s_uv, 0.0) && !self.net.is_out_of_bounds_enabled() {
            s_uv = domain.clamp_point(s_uv);
        }
        let simple = EvaluateFlags {
            only_upper_half: false,
            ..flags
        };
        let mut d =
            self.evaluate_simple_definition(k, s_uv, highest_u_deriv, highest_v_deriv, simple)?;
        if flags.non_zero_tangents && (highest_u_deriv >= 1 || highest_v_deriv >= 1) {
            let zero = EFF_ZERO * (1.0 + entry(&d, 0, 0).max_dimension());
            let u = highest_u_deriv >= 1 && entry(&d, 1, 0).length() < zero;
            let v = highest_v_deriv >= 1 && entry(&d, 0, 1).length() < zero;
            if u || v {
                let net_flags = EvaluateFlags {
                    only_upper_half: false,
                    non_zero_tangents: true,
                    ..flags
                };
                let net = self.net.evaluate(s_uv, 1, 1, net_flags)?;
                if u {
                    set_entry(&mut d, 1, 0, entry(&net, 1, 0));
                }
                if v {
                    set_entry(&mut d, 0, 1, entry(&net, 0, 1));
                }
            }
        }
        Ok(d)
    }

    /// `EvaluateSimple`: the definition's partials at `uv` (the net's inside a
    /// [`NetScope`]), `uv` clamped to the natural domain.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for `only_upper_half` with different orders; those of the
    /// definition's and the net's evaluations.
    pub fn evaluate_simple(
        &self,
        k: &Kernel,
        uv: Point2,
        highest_u_deriv: usize,
        highest_v_deriv: usize,
        flags: EvaluateFlags,
    ) -> Result<Vec<Vec<Vec3>>> {
        if NetScope::is_active(k) {
            return self.net.evaluate_simple(
                uv,
                highest_u_deriv,
                highest_v_deriv,
                flags.u_side == Side::Left,
                flags.v_side == Side::Left,
                flags.only_upper_half,
            );
        }
        self.evaluate_simple_definition(k, uv, highest_u_deriv, highest_v_deriv, flags)
    }

    /// The body of `EvaluateSimple` past its net-scope test.
    fn evaluate_simple_definition(
        &self,
        k: &Kernel,
        uv: Point2,
        highest_u_deriv: usize,
        highest_v_deriv: usize,
        flags: EvaluateFlags,
    ) -> Result<Vec<Vec<Vec3>>> {
        if flags.only_upper_half && highest_u_deriv != highest_v_deriv {
            return Err(SolidError::invalid_input(
                OP_EVAL_SIMPLE,
                "only the upper half needs equal orders",
            ));
        }
        let mut s_uv = uv;
        let domain = self.net.natural_uv_domain();
        if !domain.contains_point(s_uv, 0.0) {
            s_uv = domain.clamp_point(s_uv);
        }
        let st = self.definition_parameters(s_uv);
        let swapped = self.def.swapped;
        let (ls, lt) = if swapped {
            (highest_v_deriv, highest_u_deriv)
        } else {
            (highest_u_deriv, highest_v_deriv)
        };
        // the side of s the partials are taken on: the upper interval of the parameter
        // driving s (FromLeft TRUE) is the side after s where s grows with it (c5b6bb7)
        let upper = if swapped {
            flags.v_side == Side::Left
        } else {
            flags.u_side == Side::Left
        };
        let before = upper != (self.def.map_b[0] > 0.0);
        let sd = self.evaluate_definition_sided(k, st.x, st.y, ls, lt, before)?;
        let [b0, b1] = self.def.map_b;
        let mut out = vec![vec![Vec3::ZERO; highest_v_deriv + 1]; highest_u_deriv + 1];
        for (i, row) in out.iter_mut().enumerate() {
            for (j, slot) in row.iter_mut().enumerate() {
                // d/du = b_s d/ds (b_t d/dt when swapped), d/dv the other
                let (a, b) = if swapped { (j, i) } else { (i, j) };
                let scale = pow_index(b0, a) * pow_index(b1, b);
                *slot = entry(&sd, a, b) * scale;
            }
        }
        Ok(out)
    }

    /// `EvaluatePoint`: the definition's point at `uv` (the net's inside a [`NetScope`]).
    ///
    /// # Errors
    ///
    /// Those of the definition's and the net's evaluations.
    pub fn evaluate_point(&self, k: &Kernel, uv: Point2) -> Result<Point3> {
        if NetScope::is_active(k) {
            return self.net.evaluate_point(uv);
        }
        let st = self.definition_parameters(uv);
        let d = self.evaluate_definition(k, st.x, st.y, 0, 0)?;
        Ok(entry(&d, 0, 0))
    }

    /// `CalculateBoundingBox`: the net's boxes (`SmBSplineSurface::CalculateBoundingBox`
    /// run for `this`, the surface holding the blend, whose virtual `IsKindOf` and
    /// `EvaluateNormal` the polar box takes) grown by the bound, so the normal and pseudo
    /// boxes hold the definition's points.
    ///
    /// # Errors
    ///
    /// Those of the net's boxes.
    pub fn calculate_bounding_box(
        &self,
        k: &Kernel,
        uv_domain: &Extent2d,
        req: &BoundingBoxRequest<'_>,
        this: &Surface,
    ) -> Result<BoundingBoxes> {
        let mut r = self
            .net
            .calculate_bounding_box_of(k, uv_domain, req, Some(this))?;
        if let Some(b) = r.normal.as_mut() {
            b.expand_absolute(self.def.bound);
        }
        if let Some(b) = r.pseudo.as_mut() {
            b.expand_absolute(self.def.bound);
        }
        Ok(r)
    }

    /// `Reparameterize`: the net's, the map following it.
    ///
    /// # Errors
    ///
    /// Those of the net's; `InvalidInput` for a new domain of zero width.
    pub fn reparameterize(&mut self, new_domain: &Extent2d) -> Result<()> {
        let old = self.net.natural_uv_domain();
        self.net.reparameterize(new_domain)?;
        for c in 0..2 {
            let (o0, o1) = if c == 0 {
                (old.min().x, old.max().x)
            } else {
                (old.min().y, old.max().y)
            };
            let (n0, n1) = if c == 0 {
                (new_domain.min().x, new_domain.max().x)
            } else {
                (new_domain.min().y, new_domain.max().y)
            };
            if !(n1 != n0) {
                return Err(SolidError::invalid_input(
                    OP_REPARAM,
                    "a domain of zero width",
                ));
            }
            // old = o0 + (new - n0) (o1 - o0) / (n1 - n0)
            let beta = (o1 - o0) / (n1 - n0);
            self.reparametrize_map(c == 0, o0 - n0 * beta, beta);
        }
        Ok(())
    }

    /// `Reverse`: the net's, the map following it.
    ///
    /// # Errors
    ///
    /// Those of the net's.
    pub fn reverse(&mut self, param: SurfParamType) -> Result<()> {
        let domain = self.net.natural_uv_domain();
        self.net.reverse(param)?;
        // old = min + max - new
        let u = param == SurfParamType::U;
        let sum = if u {
            domain.min().x + domain.max().x
        } else {
            domain.min().y + domain.max().y
        };
        self.reparametrize_map(u, sum, -1.0);
        Ok(())
    }

    /// `SplitAt`: the net's pieces, each with this definition and map (a new identity; the
    /// kernel, which the surfaces' splits take, is not needed).
    ///
    /// # Errors
    ///
    /// Those of the net's.
    pub fn split_at(
        &self,
        _k: &Kernel,
        param: f64,
        surf_param: SurfParamType,
    ) -> Result<(Self, Self)> {
        let (l, r) = self.net.split_at(param, surf_param)?;
        let mut left = Self {
            net: l,
            def: self.def.clone(),
        };
        left.renew();
        let mut right = Self {
            net: r,
            def: self.def.clone(),
        };
        right.renew();
        Ok((left, right))
    }

    /// The blend moved by `by` (its supports, leader and net), its bound as it is: the
    /// blend in another frame ([`crate::blend::fillet_section::recertify`]).
    ///
    /// # Errors
    ///
    /// Those of the supports', the leader's and the net's transforms.
    pub(crate) fn translated(&self, k: &Kernel, by: Vec3) -> Result<Self> {
        let place = Axis2Placement::from_stored(by, Vec3::X, Vec3::Y);
        let mut moved = self.clone();
        for support in &mut moved.def.supports {
            support.transform_at_depth(k, &place, None, 1)?;
        }
        moved.def.leader.transform(&place, None)?;
        moved.net.transform(&place, None)?;
        moved.renew();
        Ok(moved)
    }

    /// `Transform`: a rigid motion with a uniform scale; the supports, the leader and
    /// the net move alike, the radius and the bound scale, and the bound takes in the
    /// motion's rounding where the round now lies (#2388; the bound is of the round's own
    /// frame, which moves with it).
    ///
    /// # Errors
    ///
    /// `SM_ERR` for a scale that is not uniform and positive; those of the supports',
    /// the leader's, the law's and the net's transforms.
    pub fn transform_at_depth(
        &mut self,
        k: &Kernel,
        rotate_n_move: &Axis2Placement,
        scale: Option<Vec3>,
        depth: usize,
    ) -> Result<()> {
        let mut factor = 1.0;
        if let Some(sc) = scale {
            #[allow(clippy::neg_cmp_op_on_partial_ord)] // the fork's NaN-refusing test
            if !are_same(sc.x, sc.y) || !are_same(sc.x, sc.z) || !(sc.x > EFF_ZERO) {
                return Err(SolidError::sm_err(OP_TRANSFORM));
            }
            factor = sc.x;
        }
        for support in &mut self.def.supports {
            support.transform_at_depth(k, rotate_n_move, scale, depth + 1)?;
        }
        self.def.leader.transform(rotate_n_move, scale)?;
        if factor != 1.0 {
            if let Some(law) = self.def.law.as_mut() {
                law.transform(&Axis2Placement::default(), scale)?;
            }
        }
        for r in &mut self.def.offsets {
            *r *= factor;
        }
        self.def.chamfer = self.def.chamfer.map(|c| c.scaled(factor));
        self.renew();
        self.net.transform(rotate_n_move, scale)?;
        // (#2388) the bound is taken in the round's own frame, which moves with it: it
        // scales, and takes in the motion's own rounding where the round now lies
        self.def.bound = self.def.bound * factor
            + crate::blend::fillet_section::recertify::transform_rounding(&self.net)?;
        Ok(())
    }
}

/// Re-certifies every face of `b` whose surface is a blend by its definition
/// ([`ProceduralBlend::recertify`]), as a body rebuilt from a record must be before its
/// faces are operands.
///
/// # Errors
///
/// `SM_ERR` naming the first face whose net is not proved within its bound of its
/// definition (or is not a net of its definition's stations); those of the face lookups.
pub fn recertify_faces(k: &Kernel, b: &crate::topology::Brep) -> Result<()> {
    for &f in b.face_list() {
        let Some(surface) = b.face(f)?.surface.as_ref() else {
            continue;
        };
        if let Some(blend) = surface.procedural_blend() {
            blend.recertify(k).map_err(|e| {
                // (a cancel ends the operation; it is no refusal of the face)
                if e.is_cancelled() {
                    return e;
                }
                SolidError::Upstream {
                    operation: OP_RECERTIFY,
                    code: crate::error::SM_ERR,
                    entity: Some(crate::error::EntityRef::Face(f)),
                }
            })?;
        }
    }
    Ok(())
}

/// A blend face's net (#2485): a certified B-spline approximation of its definition, or
/// its exact surface.
#[derive(Debug, Clone, Copy)]
pub enum BlendNet<'a> {
    /// A procedural blend's net, within its bound of the definition.
    Approx(&'a BSplineSurface),
    /// An exact round's surface, which is its definition (bound 0).
    Exact(&'a Surface),
}

/// A blend face by its definition (#2485), whatever its surface's kind: a procedural blend
/// ([`SurfaceKind::ProceduralBlend`]), or a round of an exact class, which keeps its
/// analytic kind and carries its definition ([`exact::ExactRound`]). Every blend face is
/// reached through [`Surface::procedural_blend`], not by matching its kind.
#[derive(Debug, Clone, Copy)]
pub enum BlendRef<'a> {
    /// A procedural blend.
    Procedural(&'a ProceduralBlend),
    /// An exact round and its surface.
    Exact {
        /// The definition.
        round: &'a exact::ExactRound,
        /// The surface, its net.
        net: &'a Surface,
    },
    /// A blend corner's face (#2488) and its surface.
    Corner {
        /// The definition.
        corner: &'a corner::CornerDefinition,
        /// The surface, its net.
        net: &'a Surface,
    },
}

/// The definition a surface keeps beside its kind ([`Surface::procedural_blend`]): a round
/// of an exact class (#2485) or a blend corner's face (#2488).
#[derive(Debug, Clone)]
pub enum KeptDefinition {
    /// An exact round's.
    Round(exact::ExactRound),
    /// A blend corner face's.
    Corner(corner::CornerDefinition),
}

impl KeptDefinition {
    /// Moves the definition with its net ([`exact::ExactRound::transform`],
    /// [`corner::CornerDefinition::transform`]).
    ///
    /// # Errors
    ///
    /// Those of the two motions.
    pub fn transform(
        &mut self,
        k: &Kernel,
        rotate_n_move: &Axis2Placement,
        scale: Option<Vec3>,
        depth: usize,
    ) -> Result<()> {
        match self {
            Self::Round(r) => r.transform(k, rotate_n_move, scale, depth),
            Self::Corner(c) => c.transform(k, rotate_n_move, scale, depth),
        }
    }
}

impl<'a> BlendRef<'a> {
    /// The net.
    #[must_use]
    pub fn net(&self) -> BlendNet<'a> {
        match *self {
            Self::Procedural(p) => BlendNet::Approx(&p.net),
            Self::Exact { net, .. } => BlendNet::Exact(net),
            Self::Corner { corner, net } => match (corner.is_exact(), net.kind()) {
                (false, SurfaceKind::BSpline(b)) => BlendNet::Approx(b),
                _ => BlendNet::Exact(net),
            },
        }
    }

    /// The net's certified distance from the definition (0 for an exact round).
    #[must_use]
    pub fn bound(&self) -> f64 {
        match *self {
            Self::Procedural(p) => p.def.bound,
            Self::Exact { round, .. } => round.bound(),
            Self::Corner { corner, .. } => corner.bound(),
        }
    }

    /// The section's order; for a corner's face, the order it meets its neighbours to (1: G1).
    #[must_use]
    pub fn order(&self) -> u32 {
        match *self {
            Self::Procedural(p) => p.def.order,
            Self::Exact { round, .. } => round.order(),
            Self::Corner { corner, .. } => corner.order(),
        }
    }

    /// The surface S1 (0) or S2 (1) the ball rolls on.
    #[must_use]
    pub fn support(&self, i: usize) -> Option<&'a Surface> {
        match *self {
            Self::Procedural(p) => p.support(i),
            Self::Exact { round, .. } => round.support(i),
            Self::Corner { corner, .. } => corner.support(i),
        }
    }

    /// The exact round's definition, for an exact round.
    #[must_use]
    pub const fn exact(&self) -> Option<&'a exact::ExactRound> {
        match *self {
            Self::Exact { round, .. } => Some(round),
            Self::Procedural(_) | Self::Corner { .. } => None,
        }
    }

    /// The corner face's definition, for a blend corner's face (#2488).
    #[must_use]
    pub const fn corner(&self) -> Option<&'a corner::CornerDefinition> {
        match *self {
            Self::Corner { corner, .. } => Some(corner),
            Self::Procedural(_) | Self::Exact { .. } => None,
        }
    }

    /// Re-certifies the net against the definition ([`ProceduralBlend::recertify`],
    /// [`exact::ExactRound::recertify`]): the distance proved.
    ///
    /// # Errors
    ///
    /// Those of the two re-certifications.
    pub fn recertify(&self, k: &Kernel) -> Result<f64> {
        match *self {
            Self::Procedural(p) => p.recertify(k),
            Self::Exact { round, net } => round.recertify(k, net),
            Self::Corner { corner, net } => corner.recertify(k, net),
        }
    }
}

impl BlendDefinition {
    /// This definition on the given supports (copies that copy the supports their own
    /// way), the identity kept.
    fn with_supports(&self, supports: [Box<Surface>; 2]) -> Self {
        Self {
            supports,
            sides: self.sides,
            offsets: self.offsets,
            law: self.law.clone(),
            order: self.order,
            thumbweight: self.thumbweight,
            leader: self.leader.clone(),
            seeds: self.seeds.clone(),
            bound: self.bound,
            chamfer: self.chamfer,
            map_a: self.map_a,
            map_b: self.map_b,
            swapped: self.swapped,
            identity: Arc::clone(&self.identity),
        }
    }
}

/// `d[i][j]`, zero outside.
fn entry(d: &[Vec<Vec3>], i: usize, j: usize) -> Vec3 {
    d.get(i)
        .and_then(|row| row.get(j))
        .copied()
        .unwrap_or(Vec3::ZERO)
}

/// Sets `d[i][j]` when it is inside.
fn set_entry(d: &mut [Vec<Vec3>], i: usize, j: usize, v: Vec3) {
    if let Some(slot) = d.get_mut(i).and_then(|row| row.get_mut(j)) {
        *slot = v;
    }
}

/// `smos_Pow(x, (double)n)`.
fn pow_index(x: f64, n: usize) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let e = n as f64;
    x.powf(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(z: f64) -> BSplineSurface {
        BSplineSurface::create_bilinear_surface(
            Point3::new(0.0, 0.0, z),
            Point3::new(10.0, 0.0, z),
            Point3::new(0.0, 10.0, z),
            Point3::new(10.0, 10.0, z),
        )
        .unwrap()
    }

    fn line() -> BSplineCurve {
        BSplineCurve::create_line_segment(3, Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0))
            .unwrap()
    }

    fn blend(
        k: &Kernel,
        order: u32,
        sides: [f64; 2],
        offsets: [f64; 2],
        bound: f64,
    ) -> Result<ProceduralBlend> {
        let s0 = Surface::from(square(0.0));
        let s1 = Surface::from(square(1.0));
        let (leader, seed0, seed1) = (line(), line(), line());
        let input = BlendDefinitionInput {
            supports: [&s0, &s1],
            sides,
            offsets,
            law: None,
            order,
            thumbweight: 0.0,
            leader: &leader,
            seeds: [&seed0, &seed1],
            bound,
            chamfer: None,
        };
        ProceduralBlend::create(k, &square(0.5), &input, |_, w| w < 1.0)
    }

    #[test]
    fn create_refuses_hostile_definitions() {
        let k = Kernel::default();
        assert!(blend(&k, 1, [1.0, -1.0], [2.0, 2.0], 1e-6).is_ok());
        assert!(blend(&k, 0, [1.0, 1.0], [2.0, 2.0], 1e-6).is_err());
        assert!(blend(&k, 5, [1.0, 1.0], [2.0, 2.0], 1e-6).is_err());
        assert!(blend(&k, 1, [1.0, 0.5], [2.0, 2.0], 1e-6).is_err());
        assert!(blend(&k, 1, [1.0, 1.0], [2.0, 3.0], 1e-6).is_err());
        assert!(blend(&k, 2, [1.0, 1.0], [2.0, 3.0], 1e-6).is_ok());
        assert!(blend(&k, 1, [1.0, 1.0], [f64::NAN, f64::NAN], 1e-6).is_err());
        assert!(blend(&k, 1, [1.0, 1.0], [2.0, 2.0], 0.0).is_err());
        assert!(blend(&k, 1, [1.0, 1.0], [2.0, 2.0], f64::INFINITY).is_err());
    }

    fn chord(
        k: &Kernel,
        order: u32,
        offsets: [f64; 2],
        chamfer: Option<ChamferDefinition>,
    ) -> Result<ProceduralBlend> {
        let s0 = Surface::from(square(0.0));
        let s1 = Surface::from(square(1.0));
        let (leader, seed0, seed1) = (line(), line(), line());
        let input = BlendDefinitionInput {
            supports: [&s0, &s1],
            sides: [1.0, 1.0],
            offsets,
            law: None,
            order,
            thumbweight: 0.0,
            leader: &leader,
            seeds: [&seed0, &seed1],
            bound: 1e-6,
            chamfer,
        };
        ProceduralBlend::create(k, &square(0.5), &input, |_, w| w < 1.0)
    }

    /// The chord (#2486) is a chamfer, its sizes those of its offsets; nothing else is.
    #[test]
    fn a_chord_is_a_chamfer_of_its_offsets() {
        let k = Kernel::default();
        let right = 0.5 * crate::math::PI;
        let c = ChamferDefinition::new(ChamferSize::Distances([1.0, 2.0]), right).ok();
        let pb = chord(&k, 0, [2.0, 1.0], c).unwrap();
        assert_eq!(pb.order(), 0);
        assert_eq!(pb.chamfer(), c.as_ref());
        // its sizes kept through a scale
        let mut scaled = pb.clone();
        scaled
            .transform_at_depth(
                &k,
                &Axis2Placement::default(),
                Some(Vec3::new(2.0, 2.0, 2.0)),
                0,
            )
            .unwrap();
        let sc = scaled.chamfer().copied();
        assert_eq!(sc.map(|c| c.size), Some(ChamferSize::Distances([2.0, 4.0])));
        assert!(sc.is_some_and(|c| c.holds(scaled.def.offsets)));
        // a chord without sizes, sizes off its offsets, a chamfer of another order
        assert!(chord(&k, 0, [2.0, 1.0], None).is_err());
        assert!(chord(&k, 0, [1.0, 2.0], c).is_err());
        assert!(chord(&k, 1, [2.0, 1.0], c).is_err());
        assert!(chord(&k, 2, [2.0, 1.0], c).is_err());
    }

    #[test]
    fn supports_nest_at_most_three_deep() {
        let k = Kernel::default();
        let mut support = Surface::from(blend(&k, 1, [1.0, 1.0], [2.0, 2.0], 1e-6).unwrap());
        for depth in 2..=4 {
            let other = Surface::from(square(2.0));
            let (leader, seed0, seed1) = (line(), line(), line());
            let input = BlendDefinitionInput {
                supports: [&support, &other],
                sides: [1.0, 1.0],
                offsets: [1.0, 1.0],
                law: None,
                order: 1,
                thumbweight: 0.0,
                leader: &leader,
                seeds: [&seed0, &seed1],
                bound: 1e-6,
                chamfer: None,
            };
            let made = ProceduralBlend::create(&k, &square(0.5), &input, |_, w| w < 1.0);
            if depth <= 3 {
                let made = made.unwrap();
                assert_eq!(made.depth(), depth);
                support = Surface::from(made);
            } else {
                assert!(made.is_err());
            }
        }
    }

    #[test]
    fn the_map_follows_reverse_reparameterize_and_swap() {
        let k = Kernel::default();
        let mut b = blend(&k, 1, [1.0, 1.0], [2.0, 2.0], 1e-6).unwrap();
        // the net's domain is [0, 1]^2; (s, t) = (u, v)
        let at = Point2::new(0.25, 0.75);
        assert_eq!(b.definition_parameters(at), at);
        b.reverse(SurfParamType::U).unwrap();
        assert_eq!(b.definition_parameters(Point2::new(0.75, 0.75)), at);
        b.reparameterize(&Extent2d::from_points(
            Point2::new(0.0, 0.0),
            Point2::new(2.0, 4.0),
        ))
        .unwrap();
        assert_eq!(b.definition_parameters(Point2::new(1.5, 3.0)), at);
        b.toggle_swap_uv_bit();
        assert_eq!(b.definition_parameters(Point2::new(3.0, 1.5)), at);
    }

    #[test]
    fn net_scopes_nest() {
        let k = Kernel::default();
        assert!(!NetScope::is_active(&k));
        {
            let _outer = NetScope::new(&k);
            {
                let _inner = NetScope::new(&k);
                assert!(NetScope::is_active(&k));
            }
            assert!(NetScope::is_active(&k));
        }
        assert!(!NetScope::is_active(&k));
    }

    #[test]
    fn a_copy_takes_a_new_serial_and_a_split_keeps_the_map() {
        let k = Kernel::default();
        let b = blend(&k, 1, [1.0, 1.0], [2.0, 2.0], 1e-6).unwrap();
        let c = b.copy(&k);
        assert!(!Arc::ptr_eq(&b.def.identity, &c.def.identity));
        assert!(b.same_definition(&k, &c).unwrap());
        let (l, r) = b.split_at(&k, 0.5, SurfParamType::U).unwrap();
        assert_eq!(
            l.definition_parameters(Point2::new(0.25, 0.5)),
            Point2::new(0.25, 0.5)
        );
        assert_eq!(
            r.definition_parameters(Point2::new(0.75, 0.5)),
            Point2::new(0.75, 0.5)
        );
    }

    #[test]
    fn inside_a_net_scope_it_evaluates_as_its_net() {
        let k = Kernel::default();
        let b = Surface::from(blend(&k, 1, [1.0, 1.0], [2.0, 2.0], 1e-6).unwrap());
        let uv = Point2::new(0.5, 0.5);
        // outside a scope the definition is solved: two parallel planes 1 apart hold no
        // ball of radius 2, so its station does not solve
        assert!(b.evaluate_point(&k, uv).is_err());
        let _scope = NetScope::new(&k);
        let p = b.evaluate_point(&k, uv).unwrap();
        assert!((p.z - 0.5).abs() < 1e-12);
    }

    /// `TurnToNURBS`'s `new SmBSplineSurface(*pBSS)` slices a blend to its net (fork
    /// 0136260: type 15002, the net's point), and the offset surface's NURB of a blend
    /// base is the net's; before, a blend face was kept and an offset of one refused.
    #[test]
    fn turn_to_nurbs_takes_the_net() {
        let k = Kernel::default();
        let pb = blend(&k, 1, [1.0, -1.0], [2.0, 2.0], 1e-6).unwrap();
        let net = pb.net().clone();
        let s = Surface::from(pb);
        let got = crate::topology::brep::p3::with_nurb(&k, &s)
            .unwrap()
            .unwrap();
        assert!(matches!(got, std::borrow::Cow::Borrowed(_)));
        let uv = Point2::new(0.3, 0.4);
        assert_eq!(
            got.evaluate_point(uv).unwrap(),
            net.evaluate_point(uv).unwrap()
        );
    }
}
