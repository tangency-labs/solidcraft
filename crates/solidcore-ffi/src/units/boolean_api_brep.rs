//! FFI wrappers for unit `boolean.api_brep`.
//!
//! Shim: `harness/cpp/shim/boolean_api_brep.cpp` (`sc_boolean_api_brep_run`).
//! Upstream: `source/SM_API/src/SmApiBrep.cpp` (`SmApiBoolean`, `SmApiBooleanUnion`,
//! `SmApiBooleanDifference`, `SmApiBooleanIntersection`, `SmApiBooleanMerge`,
//! `SmApiMergeBreps`, `SmApiBooleanWithOptions`, `SmApiBooleanLists`,
//! `SmApiBooleanTrees`, `SmApiBooleanWithCurves`, `SmApiBoolean2d`,
//! `SmApiNonManifoldBoolean`, `SmApiPiecewiseMerge`, `SmApiNonManifoldSweep`,
//! `SmApiNonManifoldRotationalSweep`, `SmApiTessellateBoundaries`).
//!
//! A [`Call`] is encoded as `op args...`. An operand block ([`BrepSpec`]) is `kind`
//! then its data: `1 O[3] length width height` (`SmApiCreateBox`), `2 C[3] radius`
//! (`SmApiCreateSphere`), `3 O[3] x y` (`SmApiCreatePlane`, the `SmBrep*&` overload),
//! `4 operation brep brep` (the `SmApiBoolean` result of two operand blocks) or
//! `5 C[3] major minor` (`SmApiCreateTorus`, the `SmBrep*&` overload) or
//! `6 n steps... brep` (the operand block moved by `n` steps, each `0 P[3] A[3] angle_deg`
//! for `SmApiRotate` or `1 V[3]` for `SmApiTranslate`, at most [`MAX_STEPS`]) or
//! `7 np counts... nc curves... degree cap` (`SmApiCreateSkinPrimitive` of `nc` NURB
//! curves, each `p n+1 (n+1) x [x y z w] (n+p+2) knots`, built as
//! `SmBSplineCurve(gw_CURVE*, 3, FALSE)`; at most [`MAX_SKIN_CURVES`]) or `8 O[3] radius`
//! (`SmApiCreatePlanarCircle`) or `9 O[3] radius
//! height` (`SmApiCreateCylinder`, the `SmBrep*&` overload). Nested blocks are at most
//! [`MAX_DEPTH`] deep.
//!
//! | [`Call`]        | op | args                                   | output after the status (on success)            |
//! |-----------------|----|----------------------------------------|-------------------------------------------------|
//! | `Boolean`       | 1  | brep brep `operation`                  | two-B-rep record                                |
//! | `Union`         | 2  | brep brep                              | two-B-rep record                                |
//! | `Difference`    | 3  | brep brep                              | two-B-rep record                                |
//! | `Intersection`  | 4  | brep brep                              | two-B-rep record                                |
//! | `Merge`         | 5  | brep brep                              | two-B-rep record                                |
//! | `WithOptions`   | 6  | brep brep `operation` 5 flags          | two-B-rep record                                |
//! | `MergeBreps`    | 7  | `n`, `n` breps, `operation`            | `list_len has [brep]`                           |
//! | `Lists`         | 8  | `n1`, breps, `n2`, breps, `operation`  | `nb`, `nb` x `has [brep]`, `ns`                 |
//! | `Trees`         | 9  | `n`, breps, `t`, `t` nodes             | `nb`, `nb` x `has [brep]`                       |
//! | `Measured`      | 10 | brep brep `operation` `post_process`   | two-B-rep record, then the measures             |
//! | `WithCurves`    | 11 | brep brep `operation`                  | two-B-rep record, then `n` edge indices         |
//! | `Boolean2d`     | 12 | brep brep `operation_2d` (0 to 4)      | `assigned`, the result or both operands         |
//! | `NonManifold`   | 13 | brep brep `operation`                  | two-B-rep record                                |
//! | `Piecewise`     | 14 | brep brep `create_new`                 | `has [is_first brep brep]`                      |
//! | `Sweep`         | 15 | brep `D[3] dist stitch merge region` sel | brep (also on `SM_ERR_INVALID_INPUT`)         |
//! | `RotationalSweep` | 16 | brep `P[3] A[3] angle stitch merge region` sel | brep (also on `SM_ERR_INVALID_INPUT`)  |
//! | `Attributed`    | 17 | brep brep `operation` `behavior`       | two-B-rep record, then the attribute rows       |
//! | `TessellateBoundaries` | 18 | brep `angle_tol_deg` `nd`, `nd` edge indices | (also after `SM_ERR`) `ne` edge indices, `nc` counts, `np` points `x y z` |
//!
//! The two-B-rep record is `has [is_first kept brep [brep]]` (the second B-rep when
//! upstream keeps it); a B-rep is `faces edges vertices regions manifold_solid`, then per
//! region (`GetRegions` order) `is_void is_infinite shells` and per shell (`GetShells` order)
//! `faceuses mate_region`, the `GetRegions` index of the region behind the shell's first
//! faceuse's mate (`-1` without one). A
//! rejected `MergeBreps` (`SM_ERR_INVALID_INPUT`) is followed by `cleared list_len`.
//! A failing `SmApiCreateTorus` operand (in any call) makes the whole record its status
//! then `2`, so both sides' radius guards are compared.
//! `Measured` is `SmApiBooleanWithOptions` with only `bBooleanPostProcess` chosen; on
//! success its record ends with the result's measures `volume_status volume faces areas...`
//! (`SmApiBrepComputeVolume` at [`VOLUME_ACCURACY`], then each face's `SmApiFaceComputeArea`
//! at [`AREA_ACCURACY`] in ascending order, a failed face as minus its status) and the
//! number of those values last ([`split_measures`]).
//! `WithCurves` ends with each intersection edge's index in the result's `GetEdges` list
//! (`-1` for one the result no longer has; matched by address, so a deleted edge whose
//! address a new result edge reuses reads as that edge's index). `Boolean2d` writes
//! `assigned`, then the result
//! and its areas, or both operands unchanged when an operand without faces leaves
//! `rpResult` unassigned. The areas tail is `faces`, each face's `SmApiFaceComputeArea` at
//! [`AREA_ACCURACY`] in ascending order (a failed face as minus its status) and the number
//! of those values last ([`split_measures`]); a successful sweep ends with it too.
//! `Piecewise` writes the result then the second operand, which upstream keeps. A sweep's
//! `region` is `pRegionBrep` ([`RegionArg`]: `0` NULL, `1` the swept B-rep, `2` another
//! B-rep, `SmApiCreateBox(origin, 1, 1, 1)`, which upstream rejects). A sweep
//! selection `sel` is three arrays (faces, edges, vertices), each `count` (`-1` for a NULL
//! array, at most [`MAX_SELECTION`]) then entries, each an index into the operand's
//! `GetFaces`/`GetEdges`/`GetVertices` list or `-1` for a NULL entry.
//! `TessellateBoundaries` first takes the curve away from the operand's edges at the given
//! `GetEdges` indices (`SetCurve(NULL, TRUE, FALSE, FALSE)`, as upstream's test; at most
//! [`MAX_DROPPED_CURVES`], each below 4096, the shim checks them against the edge list);
//! its edges are written as `GetEdges` indices. Every angle tolerance is accepted
//! (upstream rejects a non-finite or non-positive one).
//!
//! `Attributed` gives every face (`GetFaces` order) and edge (`GetEdges` order) of the first
//! (operand 0) and second (operand 1) B-rep its own `SmLongAttribute`: id
//! [`ATTRIBUTE_BASE`] `+ 1000 operand + 500 kind + index` (kind 0 face, 1 edge), value the
//! id, behaviour `behavior` (`SmAttributeBehaviorType`, 0 to 4); then `SmApiBoolean`. On
//! success the two-B-rep record is followed by `faces rows... edges rows...` of the result
//! and, when upstream keeps it, of the second B-rep, then the number of those values
//! ([`split_measures`]). A row is `measure n` then per attribute (in ascending order)
//! `id behavior value users` (value: the long value, else 0; users: `GetUsers`' size); the
//! measure is the face's `SmApiFaceComputeArea` at [`AREA_ACCURACY`] or the edge's
//! `SmApiEdgeComputeLength` at [`LENGTH_ACCURACY`] (minus the status on failure); rows are
//! sorted by their attribute part, then by measure ([`sort_rows`]).
//!
//! [`run`] accepts only what upstream handles without undefined behaviour: finite
//! operand data with positive box and plane dimensions and a nonzero sphere radius (torus
//! radii are not limited: `SmTorus::CreateCanonical` fails cleanly for a radius
//! `<= SM_EFF_ZERO` and accepts a spindle torus, `minor >= major`; any other failing
//! constructor or operand Boolean is `SC_ERR_ARG`); operand Booleans that are union, intersection, difference
//! or slice (upstream deletes their second operand); operation codes of `SmBooleanOperationType` (0 to 10);
//! at most [`MAX_LIST`] B-reps per list and [`MAX_TREE`] tree nodes in -64..=64; tree
//! executed B-rep nodes in range and binary operands non-null (`SmMerge::BooleanTrees`
//! indexes unchecked and clears each leaf slot). Safe repeated leaves remain accepted.
//! A disk radius is finite (`SmPrimitiveCreation::CreateCircle` fails cleanly otherwise);
//! sweep vectors, distances and angles are finite. Selection indices past the operand's
//! lists are `SC_ERR_ARG` from the shim.

use crate::validate::{bad, NOW, NOZ};
use crate::{call_with_buffer, FfiError};

/// Largest B-rep list accepted by [`run`] (per list for [`Call::Lists`]).
pub const MAX_LIST: usize = 8;
/// Largest postfix tree accepted by [`run`].
pub const MAX_TREE: usize = 64;
/// Deepest operand block accepted by [`run`] (a plain primitive is depth 1).
pub const MAX_DEPTH: usize = 4;
/// Most steps of one [`BrepSpec::Moved`] operand.
pub const MAX_STEPS: usize = 8;
/// Most curves (and profiles) of one [`BrepSpec::Skin`] operand.
pub const MAX_SKIN_CURVES: usize = 16;
/// Largest control point count of one [`BrepSpec::Skin`] curve.
pub const MAX_SKIN_POINTS: usize = 512;
/// Most entries of one sweep selection array.
pub const MAX_SELECTION: usize = 16;
/// `dRelativeAccuracy` of the [`Call::Measured`] volume (`SmApiBrepComputeVolume`).
pub const VOLUME_ACCURACY: f64 = 1e-6;
/// Most edges whose curve [`Call::TessellateBoundaries`] takes away.
pub const MAX_DROPPED_CURVES: usize = 64;
/// `dRelativeAccuracy` of the [`Call::Measured`] face areas (`SmApiFaceComputeArea`).
pub const AREA_ACCURACY: f64 = 1e-4;
/// `dDesiredAccuracy` of the [`Call::Attributed`] edge lengths (`SmApiEdgeComputeLength`).
pub const LENGTH_ACCURACY: f64 = 1e-9;
/// The first attribute id [`Call::Attributed`] tags operands with.
pub const ATTRIBUTE_BASE: u64 = 10007;

/// Sorts [`Call::Attributed`] rows (`measure n [id behavior value users]...`) as the shim
/// does: by the attribute part, then by measure.
pub fn sort_rows(rows: &mut [Vec<f64>]) {
    rows.sort_by(|x, y| {
        let (mx, ax) = x.split_first().map_or((0.0, &[][..]), |(m, a)| (*m, a));
        let (my, ay) = y.split_first().map_or((0.0, &[][..]), |(m, a)| (*m, a));
        ax.iter()
            .zip(ay)
            .map(|(p, q)| p.total_cmp(q))
            .find(|o| o.is_ne())
            .unwrap_or_else(|| ax.len().cmp(&ay.len()))
            .then(mx.total_cmp(&my))
    });
}

mod sys {
    extern "C" {
        pub fn sc_boolean_api_brep_run(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// How an operand B-rep is built on the C++ side.
#[derive(Debug, Clone, PartialEq)]
pub enum BrepSpec {
    /// `SmApiCreateBox(origin, length, width, height)`.
    Box {
        /// Lower-left corner.
        origin: [f64; 3],
        /// Extents along x, y, z.
        size: [f64; 3],
    },
    /// `SmApiCreateSphere(center, radius)`.
    Sphere {
        /// Center.
        center: [f64; 3],
        /// Radius (nonzero; negative is accepted upstream).
        radius: f64,
    },
    /// `SmApiCreatePlane(origin, x, y)`: a rectangular sheet in the XY plane.
    Plane {
        /// Lower-left corner.
        origin: [f64; 3],
        /// Extents along x and y.
        size: [f64; 2],
    },
    /// `SmApiCreateTorus(center, major, minor)` (axis +Z).
    Torus {
        /// Center.
        center: [f64; 3],
        /// Major (ring) radius.
        major: f64,
        /// Minor (tube) radius.
        minor: f64,
    },
    /// `brep` moved by `steps` in order (`SmApiRotate` / `SmApiTranslate`).
    Moved {
        /// The steps.
        steps: Vec<Move>,
        /// The operand moved.
        brep: Box<BrepSpec>,
    },
    /// `SmApiBoolean(a, b, operation)`'s result.
    Boolean {
        /// `SmBooleanOperationType` value: union, intersection, difference or slice.
        operation: i32,
        /// First operand (modified into the result).
        a: Box<BrepSpec>,
        /// Second operand (deleted by upstream).
        b: Box<BrepSpec>,
    },
    /// `SmApiCreateCylinder(origin, radius, height)` (axis +Z, the `SmBrep*&` overload).
    Cylinder {
        /// Center of the base.
        origin: [f64; 3],
        /// Radius (positive).
        radius: f64,
        /// Height (positive).
        height: f64,
    },
    /// `SmApiCreateSkinPrimitive(counts, curves, degree, cap)` of 3D NURB curves.
    Skin {
        /// Curves in each profile (each at least 1, summing to `curves.len()`).
        counts: Vec<u64>,
        /// The profile curves, concatenated.
        curves: Vec<SkinCurve>,
        /// Loft degree (1 to 17).
        degree: u64,
        /// Cap the first and last profiles.
        cap: bool,
    },
    /// `SmApiCreatePlanarCircle(origin, radius)`: a disk in the XY plane.
    Disk {
        /// Center.
        origin: [f64; 3],
        /// Radius.
        radius: f64,
    },
}

/// The `pRegionBrep` of a non-manifold sweep.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RegionArg {
    /// `NULL`.
    Null,
    /// `pBrepToSweep` itself.
    #[default]
    Swept,
    /// Another B-rep (`SmApiCreateBox(origin, 1, 1, 1)`).
    Other,
}

impl RegionArg {
    fn code(self) -> f64 {
        match self {
            Self::Null => 0.0,
            Self::Swept => 1.0,
            Self::Other => 2.0,
        }
    }
}

/// The optional selections of a non-manifold sweep: `None` is a NULL array, a `None`
/// entry a NULL pointer, `Some(i)` the operand's `i`th face, edge or vertex.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    /// `pOptFaces`.
    pub faces: Option<Vec<Option<usize>>>,
    /// `pOptEdges`.
    pub edges: Option<Vec<Option<usize>>>,
    /// `pOptVertices`.
    pub vertices: Option<Vec<Option<usize>>>,
}

/// A 3D NURB curve of a [`BrepSpec::Skin`] operand: degree, weighted control points
/// `[w x, w y, w z, w]` (`w` = `NOW` for every point of a non-rational curve) and the
/// full knot vector.
#[derive(Debug, Clone, PartialEq)]
pub struct SkinCurve {
    /// Degree `p`.
    pub p: usize,
    /// Control points.
    pub cpts: Vec<[f64; 4]>,
    /// Knots (`cpts.len() + p + 1` of them).
    pub knots: Vec<f64>,
}

/// Checks a skin curve: `1 <= p <= min(n, 17)`, `m = n+p+1`, finite non-decreasing
/// knots, finite 3D points, uniformly rational (positive weights) or non-rational.
fn check_skin_curve(c: &SkinCurve) -> Result<(), FfiError> {
    let n1 = c.cpts.len();
    if !(2..=MAX_SKIN_POINTS).contains(&n1) || !(1..=17).contains(&c.p) || c.p >= n1 {
        return Err(bad(
            "skin curve: need 1 <= p <= n, p <= 17, 2 to 512 points",
        ));
    }
    if c.knots.len() != n1 + c.p + 1 {
        return Err(bad("skin curve: need m = n + p + 1"));
    }
    if c.knots.iter().any(|k| !k.is_finite())
        || c.knots.windows(2).any(|w| matches!(w, [a, b] if a > b))
    {
        return Err(bad("skin curve: knots must be finite and non-decreasing"));
    }
    let rational = c.cpts.first().is_some_and(|&[_, _, _, w]| w != NOW);
    for &[x, y, z, w] in &c.cpts {
        if !(x.is_finite() && y.is_finite() && z.is_finite()) || z == NOZ {
            return Err(bad("skin curve: control points must be finite and 3D"));
        }
        if (w != NOW) != rational || (rational && !(w.is_finite() && w > 0.0)) {
            return Err(bad(
                "skin curve: mixed rationality or a non-positive weight",
            ));
        }
    }
    Ok(())
}

/// One step of a [`BrepSpec::Moved`] operand.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Move {
    /// `SmApiRotate(brep, point, axis, angle_deg)` (a zero axis fails cleanly upstream).
    Rotate {
        /// A point on the axis.
        point: [f64; 3],
        /// The axis direction.
        axis: [f64; 3],
        /// The angle in degrees.
        angle_deg: f64,
    },
    /// `SmApiTranslate(brep, vector)`.
    Translate {
        /// The translation.
        vector: [f64; 3],
    },
}

/// The merge option flags of `SmApiBooleanWithOptions`, in upstream argument order.
#[allow(clippy::struct_excessive_bools)] // upstream's five SmBoolean arguments
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flags {
    /// `bCookieCutter`.
    pub cookie_cutter: bool,
    /// `bImprinting`.
    pub imprinting: bool,
    /// `bBooleanPostProcess`.
    pub boolean_post_process: bool,
    /// `bImprintAndClassifyFaces`.
    pub imprint_and_classify_faces: bool,
    /// `bKeepOtherBrep`.
    pub keep_other_brep: bool,
}

/// One API call.
#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    /// `SmApiBoolean`.
    Boolean {
        /// First operand.
        a: BrepSpec,
        /// Second operand.
        b: BrepSpec,
        /// `SmBooleanOperationType` value.
        operation: i32,
    },
    /// `SmApiBooleanUnion`.
    Union {
        /// First operand.
        a: BrepSpec,
        /// Second operand.
        b: BrepSpec,
    },
    /// `SmApiBooleanDifference`.
    Difference {
        /// First operand.
        a: BrepSpec,
        /// Second operand.
        b: BrepSpec,
    },
    /// `SmApiBooleanIntersection`.
    Intersection {
        /// First operand.
        a: BrepSpec,
        /// Second operand.
        b: BrepSpec,
    },
    /// `SmApiBooleanMerge`.
    Merge {
        /// First operand.
        a: BrepSpec,
        /// Second operand.
        b: BrepSpec,
    },
    /// `SmApiBooleanWithOptions`.
    WithOptions {
        /// First operand.
        a: BrepSpec,
        /// Second operand.
        b: BrepSpec,
        /// `SmBooleanOperationType` value.
        operation: i32,
        /// Merge flags.
        flags: Flags,
    },
    /// `SmApiMergeBreps`.
    MergeBreps {
        /// The list.
        breps: Vec<BrepSpec>,
        /// `SmBooleanOperationType` value.
        operation: i32,
    },
    /// `SmApiBooleanLists` (empty surface lists).
    Lists {
        /// First list.
        breps1: Vec<BrepSpec>,
        /// Second list.
        breps2: Vec<BrepSpec>,
        /// `0` to `4` (any `int` is accepted upstream).
        operation: i32,
    },
    /// `SmApiBooleanTrees`.
    Trees {
        /// The B-reps.
        breps: Vec<BrepSpec>,
        /// Postfix nodes: `>= 0` a B-rep index, `< 0` an operation.
        tree: Vec<i64>,
    },
    /// `SmApiBooleanWithOptions` with only `bBooleanPostProcess` chosen, followed by the
    /// result's volume and face areas.
    Measured {
        /// First operand.
        a: BrepSpec,
        /// Second operand.
        b: BrepSpec,
        /// `SmBooleanOperationType` value.
        operation: i32,
        /// `bBooleanPostProcess`.
        post_process: bool,
    },
    /// `SmApiBooleanWithCurves`.
    WithCurves {
        /// First operand.
        a: BrepSpec,
        /// Second operand.
        b: BrepSpec,
        /// `SmBooleanOperationType` value.
        operation: i32,
    },
    /// `SmApiBoolean2d`.
    Boolean2d {
        /// First operand.
        a: BrepSpec,
        /// Second operand.
        b: BrepSpec,
        /// `Sm2DBooleanOperationType` value (0 to 4).
        operation: i32,
    },
    /// `SmApiNonManifoldBoolean`.
    NonManifold {
        /// First operand.
        a: BrepSpec,
        /// Second operand.
        b: BrepSpec,
        /// `SmBooleanOperationType` value.
        operation: i32,
    },
    /// `SmApiPiecewiseMerge`.
    Piecewise {
        /// First operand.
        a: BrepSpec,
        /// Second operand.
        b: BrepSpec,
        /// `bCreateNewBrepsForFaces`.
        create_new: bool,
    },
    /// `SmApiNonManifoldSweep(brep, region, ...)`.
    Sweep {
        /// The B-rep swept in place.
        brep: BrepSpec,
        /// `rSweepDir`.
        dir: [f64; 3],
        /// `dSweepDist`.
        dist: f64,
        /// `bDoStitching`.
        stitch: bool,
        /// `bDoMerge`.
        merge: bool,
        /// `pRegionBrep`.
        region: RegionArg,
        /// The optional selections.
        selection: Selection,
    },
    /// `SmApiNonManifoldRotationalSweep(brep, region, ...)`.
    RotationalSweep {
        /// The B-rep swept in place.
        brep: BrepSpec,
        /// `rBasePt`.
        base: [f64; 3],
        /// `rAxis`.
        axis: [f64; 3],
        /// `dAngleDeg`.
        angle_deg: f64,
        /// `bDoStitching`.
        stitch: bool,
        /// `bDoMerge`.
        merge: bool,
        /// `pRegionBrep`.
        region: RegionArg,
        /// The optional selections.
        selection: Selection,
    },
    /// Both operands' faces and edges tagged, then `SmApiBoolean`, followed by the
    /// attribute rows of the B-reps it returns.
    Attributed {
        /// First operand (operand 0).
        a: BrepSpec,
        /// Second operand (operand 1).
        b: BrepSpec,
        /// `SmBooleanOperationType` value.
        operation: i32,
        /// `SmAttributeBehaviorType` value of the tags (0 to 4).
        behavior: u8,
    },
    /// `SmApiTessellateBoundaries` of `brep`, the edges at `drop_curves` (`GetEdges`
    /// indices) first left without curve.
    TessellateBoundaries {
        /// The B-rep.
        brep: BrepSpec,
        /// `dAngleTolDeg`.
        angle_tol_deg: f64,
        /// `GetEdges` indices of the edges whose curve is taken away.
        drop_curves: Vec<u64>,
    },
}

fn check_spec(s: &BrepSpec, depth: usize) -> Result<(), FfiError> {
    if depth >= MAX_DEPTH {
        return Err(bad("operand nested too deep"));
    }
    match s {
        BrepSpec::Box { origin, size } => {
            if !origin.iter().chain(size).all(|x| x.is_finite()) {
                return Err(bad("box: non-finite value"));
            }
            if size.iter().any(|&x| x <= 0.0) {
                return Err(bad("box: dimensions must be positive"));
            }
        }
        BrepSpec::Sphere { center, radius } => {
            if !center.iter().all(|x| x.is_finite()) || !radius.is_finite() {
                return Err(bad("sphere: non-finite value"));
            }
            if *radius == 0.0 {
                return Err(bad("sphere: zero radius"));
            }
        }
        BrepSpec::Plane { origin, size } => {
            if !origin.iter().chain(size).all(|x| x.is_finite()) {
                return Err(bad("plane: non-finite value"));
            }
            if size.iter().any(|&x| x <= 0.0) {
                return Err(bad("plane: dimensions must be positive"));
            }
        }
        BrepSpec::Torus {
            center,
            major,
            minor,
        } => {
            if !center.iter().all(|x| x.is_finite()) || !major.is_finite() || !minor.is_finite() {
                return Err(bad("torus: non-finite value"));
            }
        }
        BrepSpec::Cylinder {
            origin,
            radius,
            height,
        } => {
            if !origin.iter().all(|x| x.is_finite()) || !radius.is_finite() || !height.is_finite() {
                return Err(bad("cylinder: non-finite value"));
            }
            if *radius <= 0.0 || *height <= 0.0 {
                return Err(bad("cylinder: radius and height must be positive"));
            }
        }
        BrepSpec::Moved { steps, brep } => {
            if steps.len() > MAX_STEPS {
                return Err(bad("moved operand: too many steps"));
            }
            for m in steps {
                let finite = match m {
                    Move::Rotate {
                        point,
                        axis,
                        angle_deg,
                    } => point.iter().chain(axis).all(|x| x.is_finite()) && angle_deg.is_finite(),
                    Move::Translate { vector } => vector.iter().all(|x| x.is_finite()),
                };
                if !finite {
                    return Err(bad("moved operand: non-finite value"));
                }
            }
            check_spec(brep, depth + 1)?;
        }
        BrepSpec::Boolean { operation, a, b } => {
            if ![1, 2, 3, 10].contains(operation) {
                return Err(bad(
                    "operand boolean: not union, intersection, difference or slice",
                ));
            }
            check_spec(a, depth + 1)?;
            check_spec(b, depth + 1)?;
        }
        BrepSpec::Skin {
            counts,
            curves,
            degree,
            cap: _,
        } => {
            if !(2..=MAX_SKIN_CURVES).contains(&counts.len()) {
                return Err(bad("skin: need 2 to 16 profiles"));
            }
            if curves.len() > MAX_SKIN_CURVES || counts.contains(&0) {
                return Err(bad("skin: at most 16 curves, at least 1 per profile"));
            }
            if counts.iter().sum::<u64>() != curves.len() as u64 {
                return Err(bad("skin: profile counts must sum to the curve count"));
            }
            if !(1..=17).contains(degree) {
                return Err(bad("skin: loft degree must be 1 to 17"));
            }
            for c in curves {
                check_skin_curve(c)?;
            }
        }
        BrepSpec::Disk { origin, radius } => {
            if !origin.iter().all(|x| x.is_finite()) || !radius.is_finite() {
                return Err(bad("disk: non-finite value"));
            }
        }
    }
    Ok(())
}

fn check_op(op: i32) -> Result<(), FfiError> {
    if (0..=10).contains(&op) {
        Ok(())
    } else {
        Err(bad("operation is not an SmBooleanOperationType value"))
    }
}

fn push_spec(o: &mut Vec<f64>, s: &BrepSpec) -> Result<(), FfiError> {
    check_spec(s, 0)?;
    push_checked(o, s);
    Ok(())
}

fn push_checked(o: &mut Vec<f64>, s: &BrepSpec) {
    match s {
        BrepSpec::Box { origin, size } => {
            o.push(1.0);
            o.extend_from_slice(origin);
            o.extend_from_slice(size);
        }
        BrepSpec::Sphere { center, radius } => {
            o.push(2.0);
            o.extend_from_slice(center);
            o.push(*radius);
        }
        BrepSpec::Plane { origin, size } => {
            o.push(3.0);
            o.extend_from_slice(origin);
            o.extend_from_slice(size);
        }
        BrepSpec::Torus {
            center,
            major,
            minor,
        } => {
            o.push(5.0);
            o.extend_from_slice(center);
            o.push(*major);
            o.push(*minor);
        }
        BrepSpec::Cylinder {
            origin,
            radius,
            height,
        } => {
            o.push(9.0);
            o.extend_from_slice(origin);
            o.push(*radius);
            o.push(*height);
        }
        BrepSpec::Moved { steps, brep } => {
            o.push(6.0);
            o.push(steps.len() as f64);
            for m in steps {
                match m {
                    Move::Rotate {
                        point,
                        axis,
                        angle_deg,
                    } => {
                        o.push(0.0);
                        o.extend_from_slice(point);
                        o.extend_from_slice(axis);
                        o.push(*angle_deg);
                    }
                    Move::Translate { vector } => {
                        o.push(1.0);
                        o.extend_from_slice(vector);
                    }
                }
            }
            push_checked(o, brep);
        }
        BrepSpec::Boolean { operation, a, b } => {
            o.push(4.0);
            o.push(f64::from(*operation));
            push_checked(o, a);
            push_checked(o, b);
        }
        BrepSpec::Skin {
            counts,
            curves,
            degree,
            cap,
        } => {
            o.push(7.0);
            o.push(counts.len() as f64);
            o.extend(counts.iter().map(|&c| c as f64));
            o.push(curves.len() as f64);
            for c in curves {
                o.push(c.p as f64);
                o.push(c.cpts.len() as f64);
                o.extend(c.cpts.iter().flatten());
                o.extend(&c.knots);
            }
            o.push(*degree as f64);
            o.push(f64::from(u8::from(*cap)));
        }
        BrepSpec::Disk { origin, radius } => {
            o.push(8.0);
            o.extend_from_slice(origin);
            o.push(*radius);
        }
    }
}

fn push_selection(o: &mut Vec<f64>, sel: &Selection) -> Result<(), FfiError> {
    for list in [&sel.faces, &sel.edges, &sel.vertices] {
        match list {
            None => o.push(-1.0),
            Some(l) => {
                if l.len() > MAX_SELECTION {
                    return Err(bad(format!("more than {MAX_SELECTION} selected")));
                }
                o.push(l.len() as f64);
                o.extend(l.iter().map(|e| e.map_or(-1.0, |i| i as f64)));
            }
        }
    }
    Ok(())
}

fn push_list(o: &mut Vec<f64>, l: &[BrepSpec], max: usize) -> Result<(), FfiError> {
    if l.len() > max {
        return Err(bad(format!("more than {max} B-reps")));
    }
    o.push(l.len() as f64);
    for s in l {
        push_spec(o, s)?;
    }
    Ok(())
}

fn flag(b: bool) -> f64 {
    if b {
        1.0
    } else {
        0.0
    }
}

fn encode(call: &Call) -> Result<Vec<f64>, FfiError> {
    let mut o = Vec::new();
    match call {
        Call::Boolean { a, b, operation } => {
            check_op(*operation)?;
            o.push(1.0);
            push_spec(&mut o, a)?;
            push_spec(&mut o, b)?;
            o.push(f64::from(*operation));
        }
        Call::Union { a, b }
        | Call::Difference { a, b }
        | Call::Intersection { a, b }
        | Call::Merge { a, b } => {
            let op = match call {
                Call::Union { .. } => 2.0,
                Call::Difference { .. } => 3.0,
                Call::Intersection { .. } => 4.0,
                _ => 5.0,
            };
            o.push(op);
            push_spec(&mut o, a)?;
            push_spec(&mut o, b)?;
        }
        Call::WithOptions {
            a,
            b,
            operation,
            flags,
        } => {
            check_op(*operation)?;
            o.push(6.0);
            push_spec(&mut o, a)?;
            push_spec(&mut o, b)?;
            o.push(f64::from(*operation));
            o.extend([
                flag(flags.cookie_cutter),
                flag(flags.imprinting),
                flag(flags.boolean_post_process),
                flag(flags.imprint_and_classify_faces),
                flag(flags.keep_other_brep),
            ]);
        }
        Call::MergeBreps { breps, operation } => {
            check_op(*operation)?;
            o.push(7.0);
            push_list(&mut o, breps, 2 * MAX_LIST)?;
            o.push(f64::from(*operation));
        }
        Call::Lists {
            breps1,
            breps2,
            operation,
        } => {
            o.push(8.0);
            push_list(&mut o, breps1, MAX_LIST)?;
            push_list(&mut o, breps2, MAX_LIST)?;
            o.push(f64::from(*operation));
        }
        Call::Trees { breps, tree } => {
            if tree.len() > MAX_TREE {
                return Err(bad("tree too long"));
            }
            super::boolean_merge_p3::validate_tree_nodes(breps.len(), tree)?;
            o.push(9.0);
            push_list(&mut o, breps, 2 * MAX_LIST)?;
            o.push(tree.len() as f64);
            o.extend(tree.iter().map(|&n| n as f64));
        }
        Call::Measured {
            a,
            b,
            operation,
            post_process,
        } => {
            check_op(*operation)?;
            o.push(10.0);
            push_spec(&mut o, a)?;
            push_spec(&mut o, b)?;
            o.push(f64::from(*operation));
            o.push(flag(*post_process));
        }
        Call::WithCurves { a, b, operation } | Call::NonManifold { a, b, operation } => {
            check_op(*operation)?;
            o.push(if matches!(call, Call::WithCurves { .. }) {
                11.0
            } else {
                13.0
            });
            push_spec(&mut o, a)?;
            push_spec(&mut o, b)?;
            o.push(f64::from(*operation));
        }
        Call::Boolean2d { a, b, operation } => {
            if !(0..=4).contains(operation) {
                return Err(bad("operation is not an Sm2DBooleanOperationType value"));
            }
            o.push(12.0);
            push_spec(&mut o, a)?;
            push_spec(&mut o, b)?;
            o.push(f64::from(*operation));
        }
        Call::Piecewise { a, b, create_new } => {
            o.push(14.0);
            push_spec(&mut o, a)?;
            push_spec(&mut o, b)?;
            o.push(flag(*create_new));
        }
        Call::Sweep {
            brep,
            dir,
            dist,
            stitch,
            merge,
            region,
            selection,
        } => {
            if !dir.iter().all(|x| x.is_finite()) || !dist.is_finite() {
                return Err(bad("sweep: non-finite value"));
            }
            o.push(15.0);
            push_spec(&mut o, brep)?;
            o.extend_from_slice(dir);
            o.push(*dist);
            o.extend([flag(*stitch), flag(*merge), region.code()]);
            push_selection(&mut o, selection)?;
        }
        Call::RotationalSweep {
            brep,
            base,
            axis,
            angle_deg,
            stitch,
            merge,
            region,
            selection,
        } => {
            if !base.iter().chain(axis).all(|x| x.is_finite()) || !angle_deg.is_finite() {
                return Err(bad("rotational sweep: non-finite value"));
            }
            o.push(16.0);
            push_spec(&mut o, brep)?;
            o.extend_from_slice(base);
            o.extend_from_slice(axis);
            o.push(*angle_deg);
            o.extend([flag(*stitch), flag(*merge), region.code()]);
            push_selection(&mut o, selection)?;
        }
        Call::Attributed {
            a,
            b,
            operation,
            behavior,
        } => {
            check_op(*operation)?;
            if *behavior > 4 {
                return Err(bad("behavior is not an SmAttributeBehaviorType value"));
            }
            o.push(17.0);
            push_spec(&mut o, a)?;
            push_spec(&mut o, b)?;
            o.push(f64::from(*operation));
            o.push(f64::from(*behavior));
        }
        Call::TessellateBoundaries {
            brep,
            angle_tol_deg,
            drop_curves,
        } => {
            if drop_curves.len() > MAX_DROPPED_CURVES || drop_curves.iter().any(|&d| d > 4095) {
                return Err(bad("too many dropped curves or an index above 4095"));
            }
            o.push(18.0);
            push_spec(&mut o, brep)?;
            o.push(*angle_tol_deg);
            o.push(drop_curves.len() as f64);
            o.extend(drop_curves.iter().map(|&d| d as f64));
        }
    }
    Ok(o)
}

/// Splits a successful [`Call::Measured`] record into the integer part before the
/// measures and the measures themselves (`volume_status volume faces areas...`), using the
/// count the shim appends last. `None` for a record too short for its count.
#[must_use]
pub fn split_measures(record: &[f64]) -> Option<(&[f64], &[f64])> {
    let (&n, rest) = record.split_last()?;
    if !(n >= 0.0 && n.fract() == 0.0) {
        return None;
    }
    let n = n as usize;
    let at = rest.len().checked_sub(n)?;
    Some(rest.split_at(at))
}

/// Runs one API call on the C++ side. Returns the flat output record (status first,
/// see the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for inputs upstream cannot take safely;
/// [`FfiError::Status`] for a shim failure (`SC_ERR_ARG`: an operand constructor
/// failed).
pub fn run(call: &Call) -> Result<Vec<f64>, FfiError> {
    let input = encode(call)?;
    call_with_buffer(256, |buf: &mut [f64], len| {
        // SAFETY: input is valid for input.len() reads; buf for buf.len() writes; len is
        // a valid i64. The shim bounds-checks every read and copies at most buf.len().
        unsafe {
            sys::sc_boolean_api_brep_run(
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trees_accept_defined_null_slots_and_guard_only_reachable_dereferences() {
        let shape = BrepSpec::Box {
            origin: [0.0; 3],
            size: [1.0; 3],
        };
        let program = |tree| Call::Trees {
            breps: vec![shape.clone(), shape.clone()],
            tree,
        };
        assert!(encode(&program(vec![0, 0])).is_ok());
        assert!(encode(&program(vec![0, 0, -1])).is_err());
        assert!(encode(&program(vec![-1, 64])).is_ok());
    }

    #[test]
    fn operand_blocks_nest_only_to_max_depth() {
        let mut spec = BrepSpec::Box {
            origin: [0.0; 3],
            size: [1.0; 3],
        };
        let call = |a: &BrepSpec| Call::Union {
            a: a.clone(),
            b: a.clone(),
        };
        for depth in 1..=MAX_DEPTH {
            assert!(encode(&call(&spec)).is_ok(), "depth {depth}");
            spec = BrepSpec::Boolean {
                operation: 1,
                a: Box::new(spec.clone()),
                b: Box::new(spec),
            };
        }
        assert!(encode(&call(&spec)).is_err());
        let slice = |operation| BrepSpec::Boolean {
            operation,
            a: Box::new(BrepSpec::Sphere {
                center: [0.0; 3],
                radius: 1.0,
            }),
            b: Box::new(BrepSpec::Plane {
                origin: [0.0; 3],
                size: [1.0, 1.0],
            }),
        };
        assert!(encode(&call(&slice(10))).is_ok());
        assert!(encode(&call(&slice(6))).is_err());
    }
}
