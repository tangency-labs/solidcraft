//! Runner for unit `boolean.api_brep`: the `SM_API` boolean entry points and the
//! non-manifold sweeps of a B-rep (`SmApiNonManifoldSweep`,
//! `SmApiNonManifoldRotationalSweep`, Rust `solidcore::sweep::api_brep`).
//!
//! C++: `solidcore_ffi::units::boolean_api_brep` (upstream
//! `source/SM_API/src/SmApiBrep.cpp`). Rust: `solidcore::boolean::api_brep`.
//!
//! A case is one API call (`api` plus its arguments). Operands are boxes
//! (`SmApiCreateBox`) or spheres (`SmApiCreateSphere`); fixture cases also use sheets
//! (`SmApiCreatePlane`), disks (`SmApiCreatePlanarCircle`), tori (`SmApiCreateTorus`),
//! cylinders (`SmApiCreateCylinder`), cones (`SmApiCreateCone`), operands moved by `SmApiRotate` /
//! `SmApiTranslate` steps, skin solids (`SmApiCreateSkinPrimitive` of exact NURB curves)
//! and the result of an `SmApiBoolean` of two operands. Both sides
//! build them with the same constructors (`primitive::api_primitives`,
//! `sweep::api_surfaces::create_skin_primitive` and
//! `boolean::api_brep::boolean` on the Rust side) in the case's one kernel / context, at the default `Kernel::fixes` (SMLib behaviour). A
//! constructor that fails is a harness error on both sides (the shim's `SC_ERR_ARG`);
//! one that stops at an unported callee defers the case. Both sides produce `status` (`0` = `SM_SUCCESS`, else the `SmStatus`:
//! `1007` `SM_ERR_INVALID_INPUT`; `-1` on the Rust side for an error upstream cannot
//! produce) and `record` (the op's outputs after the status, see the FFI module): on
//! success the result B-reps' face, edge, vertex and region counts and
//! `IsManifoldSolid`. A `measured` call (`SmApiBooleanWithOptions` with only the
//! post-process flag chosen; fixtures only) also compares `measures`: the result's volume
//! (`SmApiBrepComputeVolume`) and its face areas (`SmApiFaceComputeArea`) in ascending
//! order, so a result with the right topology but a wrongly trimmed face shows. A
//! `boolean_2d` result and a successful sweep likewise compare `areas`: the face areas in
//! ascending order (no volume: those results are sheets or non-manifold).
//!
//! `with_curves` also compares each intersection edge's index in the result's edge list
//! (`-1` when the result no longer has it). The shim matches the returned `SmEdge*`
//! by address, the port by `EdgeId`, which is never reused: a mismatch confined to
//! entries that are `-1` on the Rust side and an index on the C++ side means upstream
//! returned a pointer to an edge the merge deleted, whose address a new result edge
//! reuses (a dangling `SmEdge*`, against `SmApiBooleanWithCurves`' own contract);
//! `boolean_2d` whether the result was assigned (an operand without faces leaves both
//! unchanged); `piecewise_merge` the result and the imprinted second operand; the sweeps
//! the B-rep after the call, also when it is rejected with `SM_ERR_INVALID_INPUT` (it must
//! be untouched). A sweep `selection` lists indices into the operand's face, edge and
//! vertex lists (`null` for a NULL entry; a missing list is a NULL array). A sweep's
//! `region` is `pRegionBrep`: `swept` (the default) the swept B-rep, `null` NULL, `other`
//! a second B-rep (`SmApiCreateBox(origin, 1, 1, 1)` on both sides), which upstream
//! rejects (`SmApiBrep.cpp:1822`, `:1881`).
//!
//! An `attributed` call (fixtures only, issue #1462) tags every operand face and edge with
//! its own long attribute before `SmApiBoolean`, each side on its own operands (the C++
//! shim with `SmLongAttribute`s, the Rust side with `Brep::add_attribute`), and also
//! compares `attributes`: per result face (edge) its area (length) and the attributes it
//! carries, sorted by attributes, so attribute propagation through the merge shows.
//!
//! A `checked` call (fixtures only) runs `SmApiBoolean`, then the capi check sequence
//! (`SmBrep::ValidatePointers`, then `SmBrep::AssertValid`; Rust
//! `topology::brep::validation::check_brep`) on the result at `level` (default 2), and
//! also compares `check`: the pointer check's status, whether `AssertValid` ran and
//! passed, and for each list the failed tests (reporting type, list and test index) with
//! how many reports each failed. The entities a report names are not compared: the two
//! sides number a boolean result's entities differently.
//!
//! Compared: the input validation (`SM_BO_EXTRACT_SEPARATE` for the two-B-rep entry
//! points, short lists and operations other than union / intersection / difference /
//! merge for `SmApiMergeBreps`, which must leave the list untouched) and the merges
//! themselves. A Rust call that stops at an unported callee is deferred under that
//! callee. Every run has at least [`MIN_GENERATED`] generated cases, plus
//! [`MIN_GENERATED_MORE`] cases of the entry points added later (`more0000`, ...: their
//! own stream, so each `gen` case stays the one the baseline has).
//!
//! A `tessellate_boundaries` call (`SmApiTessellateBoundaries`) tessellates one operand's
//! edges, some of them first left without curve (`drop_curves`, `GetEdges` indices):
//! `status` (also `SM_ERR` with partial output), `record` (the edges as `GetEdges`
//! indices, then the vertex counts, each list after its length) and `points` (`x y z` per
//! point). Besides the fixtures, [`MIN_GENERATED_TESSELLATE`] such calls are generated on
//! their own stream (`tess0000`, ...): every operand kind at small, moderate and large
//! scales, angle tolerances from `0.5` to `90` degrees and invalid ones.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::boolean::api_brep as api;
use solidcore::boolean::merge::MergeOptions;
use solidcore::curve::curve::Curve as SmCurve;
use solidcore::geom::a_object::BooleanOperation;
use solidcore::geom::assert_array::AssertArray;
use solidcore::geom::attribute::{Attribute, AttributeStore, Behavior, ObjectKey};
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::kernel::Kernel;
use solidcore::mass::integrator::properties::{
    brep_volume, edge_length, face_area, PropertyAttributes,
};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::primitive::api_primitives as primitives;
use solidcore::sweep::api_brep::{
    non_manifold_rotational_sweep, non_manifold_sweep, RegionBrep, SweepSelection,
};
use solidcore::sweep::api_surfaces::create_skin_primitive;
use solidcore::error::SM_ERR;
use solidcore::topology::brep::p1::BoundaryTessellation;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::brep::{p1 as brep_p1, p2 as brep_p2};
use solidcore::topology::edgeuse::p1 as edgeuse_p1;
use solidcore::topology::proto_topology::p1::api_transform::{
    api_rotate, api_translate, TransformObject,
};
use solidcore::topology::shell::shell_region;
use solidcore::topology::{Brep, EdgeId, FaceId, FaceuseId, RegionId, VertexId};
use solidcore::{EntityRef, SolidError};
use solidcore_ffi::units::boolean_api_brep::{
    self as ffi, BrepSpec, Call, Flags, RegionArg, Selection,
};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 240;
/// Generated cases of the later entry points (with curves, 2D, non-manifold, piecewise
/// and the sweeps) per run, whatever `--seeds` says.
pub const MIN_GENERATED_MORE: usize = 160;

/// Generated `tessellate_boundaries` calls per run (their own stream).
pub const MIN_GENERATED_TESSELLATE: usize = 160;

/// `SM_ERR_INVALID_INPUT`.
const SM_ERR_INVALID_INPUT: i64 = 1007;

/// `boolean.api_brep` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -------------------------------------------------------------------------

type P3 = [f64; 3];

/// An operand.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum BrepIn {
    /// `SmApiCreateBox`.
    Box { origin: P3, size: P3 },
    /// `SmApiCreateSphere`.
    Sphere { center: P3, radius: f64 },
    /// `SmApiCreatePlane` (`SmBrep*&` overload).
    Plane { origin: P3, size: [f64; 2] },
    /// `SmApiCreateTorus` (`SmBrep*&` overload; axis +Z).
    Torus { center: P3, major: f64, minor: f64 },
    /// `SmApiCreateCylinder` (`SmBrep*&` overload; axis +Z, `origin` the base center).
    Cylinder { origin: P3, radius: f64, height: f64 },
    /// `brep` moved by `steps` in order (`SmApiRotate` / `SmApiTranslate`).
    Moved {
        steps: Vec<MoveIn>,
        brep: Box<BrepIn>,
    },
    /// `SmApiBoolean(a, b, operation)`'s result.
    Boolean {
        operation: i32,
        a: Box<BrepIn>,
        b: Box<BrepIn>,
    },
    /// `SmApiCreateSkinPrimitive(counts, curves, degree, cap_ends)` of 3D NURB curves.
    Skin {
        counts: Vec<u64>,
        curves: Vec<SkinCurveIn>,
        degree: u64,
        #[serde(default = "yes")]
        cap_ends: bool,
    },
    /// `SmApiCreatePlanarCircle` (a disk in the XY plane).
    Disk { origin: P3, radius: f64 },
    /// `SmApiCreateCone` (`SmBrep*&` overload; axis +Z, `origin` the base center).
    Cone {
        origin: P3,
        radius_base: f64,
        radius_top: f64,
        height: f64,
    },
}

/// The `pRegionBrep` of a non-manifold sweep.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RegionIn {
    /// `NULL`.
    Null,
    /// The swept B-rep.
    #[default]
    Swept,
    /// A second B-rep, `SmApiCreateBox(origin, 1, 1, 1)`.
    Other,
}

impl RegionIn {
    const fn ffi(self) -> RegionArg {
        match self {
            Self::Null => RegionArg::Null,
            Self::Swept => RegionArg::Swept,
            Self::Other => RegionArg::Other,
        }
    }
}

/// The selections of a non-manifold sweep: indices into the operand's `GetFaces`,
/// `GetEdges` and `GetVertices` lists, `null` for a NULL entry; a missing list is a NULL
/// array.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SelectionIn {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    faces: Option<Vec<Option<usize>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    edges: Option<Vec<Option<usize>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    vertices: Option<Vec<Option<usize>>>,
}

/// A 3D NURB curve of a skin operand, as its NURB stores it: degree, control points
/// (weighted, `w x w y w z`, when `weights` is given) and the full knot vector. Both
/// sides build `SmBSplineCurve(gw_CURVE*, 3, FALSE)` from it.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SkinCurveIn {
    p: usize,
    cpts: Vec<P3>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    weights: Option<Vec<f64>>,
    knots: Vec<f64>,
}

impl SkinCurveIn {
    /// The `[x y z w]` points with `w` = `NOW` for a non-rational curve (a weight count
    /// that differs from the point count is left to the FFI's check as a missing weight).
    fn pw(&self) -> Vec<[f64; 4]> {
        self.cpts
            .iter()
            .enumerate()
            .map(|(i, q)| {
                let w = match &self.weights {
                    Some(w) => w.get(i).copied().unwrap_or(f64::NAN),
                    None => NOW,
                };
                [q[0], q[1], q[2], w]
            })
            .collect()
    }
}

/// One step of a moved operand.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum MoveIn {
    /// `SmApiRotate(brep, point, axis, angle_deg)`.
    Rotate { point: P3, axis: P3, angle_deg: f64 },
    /// `SmApiTranslate(brep, vector)`.
    Translate { vector: P3 },
}

/// `SmApiBooleanWithOptions` flags; missing ones take the `SmMergeOptions` defaults.
#[allow(clippy::struct_excessive_bools)] // upstream's five SmBoolean arguments
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct FlagsIn {
    #[serde(default)]
    cookie_cutter: bool,
    #[serde(default)]
    imprinting: bool,
    #[serde(default = "yes")]
    boolean_post_process: bool,
    #[serde(default)]
    imprint_and_classify_faces: bool,
    #[serde(default)]
    keep_other_brep: bool,
}

fn yes() -> bool {
    true
}

/// One API call.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "api", rename_all = "snake_case")]
enum Inputs {
    Boolean {
        a: BrepIn,
        b: BrepIn,
        operation: i32,
    },
    Union {
        a: BrepIn,
        b: BrepIn,
    },
    Difference {
        a: BrepIn,
        b: BrepIn,
    },
    Intersection {
        a: BrepIn,
        b: BrepIn,
    },
    Merge {
        a: BrepIn,
        b: BrepIn,
    },
    WithOptions {
        a: BrepIn,
        b: BrepIn,
        operation: i32,
        flags: FlagsIn,
    },
    MergeBreps {
        breps: Vec<BrepIn>,
        operation: i32,
    },
    Lists {
        breps1: Vec<BrepIn>,
        breps2: Vec<BrepIn>,
        operation: i32,
    },
    Trees {
        breps: Vec<BrepIn>,
        tree: Vec<i64>,
    },
    /// `SmApiBooleanWithOptions` with only `bBooleanPostProcess` chosen, then the
    /// result's volume and face areas.
    Measured {
        a: BrepIn,
        b: BrepIn,
        operation: i32,
        #[serde(default = "yes")]
        post_process: bool,
    },
    /// `SmApiBooleanWithCurves`.
    WithCurves {
        a: BrepIn,
        b: BrepIn,
        operation: i32,
    },
    /// `SmApiBoolean2d` (`operation` an `Sm2DBooleanOperationType`, 0 to 4).
    Boolean2d {
        a: BrepIn,
        b: BrepIn,
        operation: i32,
    },
    /// `SmApiNonManifoldBoolean`.
    NonManifold {
        a: BrepIn,
        b: BrepIn,
        operation: i32,
    },
    /// `SmApiPiecewiseMerge`.
    PiecewiseMerge {
        a: BrepIn,
        b: BrepIn,
        #[serde(default)]
        create_new: bool,
    },
    /// `SmApiNonManifoldSweep(brep, region, ...)`.
    Sweep {
        brep: BrepIn,
        dir: P3,
        dist: f64,
        #[serde(default = "yes")]
        stitch: bool,
        #[serde(default)]
        merge: bool,
        #[serde(default)]
        region: RegionIn,
        #[serde(default)]
        selection: SelectionIn,
        /// Tag every face and edge first (operand 0 of [`Inputs::Attributed`], this
        /// behaviour) and compare the swept B-rep's attribute rows too.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        attributed: Option<u8>,
    },
    /// `SmApiNonManifoldRotationalSweep(brep, region, ...)`.
    RotationalSweep {
        brep: BrepIn,
        base: P3,
        axis: P3,
        angle_deg: f64,
        #[serde(default = "yes")]
        stitch: bool,
        #[serde(default)]
        merge: bool,
        #[serde(default)]
        region: RegionIn,
        #[serde(default)]
        selection: SelectionIn,
        /// Tag every face and edge first (operand 0 of [`Inputs::Attributed`], this
        /// behaviour) and compare the swept B-rep's attribute rows too.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        attributed: Option<u8>,
    },
    /// Both operands' faces and edges tagged with their own long attribute (behaviour
    /// `behavior`, an `SmAttributeBehaviorType` value), then `SmApiBoolean`; the record
    /// adds the attributes the returned B-reps' faces and edges carry.
    Attributed {
        a: BrepIn,
        b: BrepIn,
        operation: i32,
        #[serde(default)]
        behavior: u8,
        /// `SmApiBooleanWithOptions` with these flags instead of `SmApiBoolean`.
        #[serde(default)]
        flags: Option<FlagsIn>,
        /// With `flags`: the first operand is first replaced by its `SmApiBrepCopy`.
        #[serde(default)]
        copy_first: bool,
    },
    /// `SmApiTessellateBoundaries` of `brep`, the edges at `drop_curves` (`GetEdges`
    /// indices) first left without curve.
    TessellateBoundaries {
        brep: BrepIn,
        angle_tol_deg: f64,
        #[serde(default)]
        drop_curves: Vec<u64>,
    },
    /// A `merge_breps`, `lists` or `trees` call whose input B-reps (in input order,
    /// `breps1` then `breps2`) are each tagged as operand `i`, as `attributed` tags its
    /// two; the record adds the attributes every output B-rep's faces and edges carry.
    AttributedList {
        call: Box<Inputs>,
        #[serde(default)]
        behavior: u8,
    },
    /// `SmApiBoolean`, then the capi check of its result at `level` (an
    /// `SmAssertTestLevel`, 0 to 2); the record adds the failed tests.
    Checked {
        a: BrepIn,
        b: BrepIn,
        operation: i32,
        #[serde(default = "level_2")]
        level: u8,
    },
}

fn level_2() -> u8 {
    2
}

fn spec(b: &BrepIn) -> BrepSpec {
    match b {
        &BrepIn::Box { origin, size } => BrepSpec::Box { origin, size },
        &BrepIn::Sphere { center, radius } => BrepSpec::Sphere { center, radius },
        &BrepIn::Plane { origin, size } => BrepSpec::Plane { origin, size },
        &BrepIn::Torus {
            center,
            major,
            minor,
        } => BrepSpec::Torus {
            center,
            major,
            minor,
        },
        &BrepIn::Cylinder {
            origin,
            radius,
            height,
        } => BrepSpec::Cylinder {
            origin,
            radius,
            height,
        },
        BrepIn::Moved { steps, brep } => BrepSpec::Moved {
            steps: steps
                .iter()
                .map(|m| match *m {
                    MoveIn::Rotate {
                        point,
                        axis,
                        angle_deg,
                    } => ffi::Move::Rotate {
                        point,
                        axis,
                        angle_deg,
                    },
                    MoveIn::Translate { vector } => ffi::Move::Translate { vector },
                })
                .collect(),
            brep: Box::new(spec(brep)),
        },
        BrepIn::Skin {
            counts,
            curves,
            degree,
            cap_ends,
        } => BrepSpec::Skin {
            counts: counts.clone(),
            curves: curves
                .iter()
                .map(|c| ffi::SkinCurve {
                    p: c.p,
                    cpts: c.pw(),
                    knots: c.knots.clone(),
                })
                .collect(),
            degree: *degree,
            cap: *cap_ends,
        },
        BrepIn::Boolean { operation, a, b } => BrepSpec::Boolean {
            operation: *operation,
            a: Box::new(spec(a)),
            b: Box::new(spec(b)),
        },
        &BrepIn::Disk { origin, radius } => BrepSpec::Disk { origin, radius },
        &BrepIn::Cone {
            origin,
            radius_base,
            radius_top,
            height,
        } => BrepSpec::Cone {
            origin,
            radius_base,
            radius_top,
            height,
        },
    }
}

fn selection(s: &SelectionIn) -> Selection {
    Selection {
        faces: s.faces.clone(),
        edges: s.edges.clone(),
        vertices: s.vertices.clone(),
    }
}

fn specs(l: &[BrepIn]) -> Vec<BrepSpec> {
    l.iter().map(spec).collect()
}

fn to_call(i: &Inputs) -> Call {
    match i {
        Inputs::Boolean { a, b, operation } => Call::Boolean {
            a: spec(a),
            b: spec(b),
            operation: *operation,
        },
        Inputs::Union { a, b } => Call::Union {
            a: spec(a),
            b: spec(b),
        },
        Inputs::Difference { a, b } => Call::Difference {
            a: spec(a),
            b: spec(b),
        },
        Inputs::Intersection { a, b } => Call::Intersection {
            a: spec(a),
            b: spec(b),
        },
        Inputs::Merge { a, b } => Call::Merge {
            a: spec(a),
            b: spec(b),
        },
        Inputs::WithOptions {
            a,
            b,
            operation,
            flags,
        } => Call::WithOptions {
            a: spec(a),
            b: spec(b),
            operation: *operation,
            flags: Flags {
                cookie_cutter: flags.cookie_cutter,
                imprinting: flags.imprinting,
                boolean_post_process: flags.boolean_post_process,
                imprint_and_classify_faces: flags.imprint_and_classify_faces,
                keep_other_brep: flags.keep_other_brep,
            },
        },
        Inputs::MergeBreps { breps, operation } => Call::MergeBreps {
            breps: specs(breps),
            operation: *operation,
        },
        Inputs::Lists {
            breps1,
            breps2,
            operation,
        } => Call::Lists {
            breps1: specs(breps1),
            breps2: specs(breps2),
            operation: *operation,
        },
        Inputs::Trees { breps, tree } => Call::Trees {
            breps: specs(breps),
            tree: tree.clone(),
        },
        Inputs::Measured {
            a,
            b,
            operation,
            post_process,
        } => Call::Measured {
            a: spec(a),
            b: spec(b),
            operation: *operation,
            post_process: *post_process,
        },
        Inputs::WithCurves { a, b, operation } => Call::WithCurves {
            a: spec(a),
            b: spec(b),
            operation: *operation,
        },
        Inputs::Boolean2d { a, b, operation } => Call::Boolean2d {
            a: spec(a),
            b: spec(b),
            operation: *operation,
        },
        Inputs::NonManifold { a, b, operation } => Call::NonManifold {
            a: spec(a),
            b: spec(b),
            operation: *operation,
        },
        Inputs::PiecewiseMerge { a, b, create_new } => Call::Piecewise {
            a: spec(a),
            b: spec(b),
            create_new: *create_new,
        },
        Inputs::Sweep {
            brep,
            dir,
            dist,
            stitch,
            merge,
            region,
            selection: sel,
            attributed,
        } => Call::Sweep {
            brep: spec(brep),
            dir: *dir,
            dist: *dist,
            stitch: *stitch,
            merge: *merge,
            region: region.ffi(),
            selection: selection(sel),
            attributed: *attributed,
        },
        Inputs::RotationalSweep {
            brep,
            base,
            axis,
            angle_deg,
            stitch,
            merge,
            region,
            selection: sel,
            attributed,
        } => Call::RotationalSweep {
            brep: spec(brep),
            base: *base,
            axis: *axis,
            angle_deg: *angle_deg,
            stitch: *stitch,
            merge: *merge,
            region: region.ffi(),
            selection: selection(sel),
            attributed: *attributed,
        },
        Inputs::Attributed {
            a,
            b,
            operation,
            behavior,
            flags,
            copy_first,
        } => Call::Attributed {
            a: spec(a),
            b: spec(b),
            operation: *operation,
            behavior: *behavior,
            options: flags.map(|f| {
                (
                    *copy_first,
                    Flags {
                        cookie_cutter: f.cookie_cutter,
                        imprinting: f.imprinting,
                        boolean_post_process: f.boolean_post_process,
                        imprint_and_classify_faces: f.imprint_and_classify_faces,
                        keep_other_brep: f.keep_other_brep,
                    },
                )
            }),
        },
        Inputs::TessellateBoundaries {
            brep,
            angle_tol_deg,
            drop_curves,
        } => Call::TessellateBoundaries {
            brep: spec(brep),
            angle_tol_deg: *angle_tol_deg,
            drop_curves: drop_curves.clone(),
        },
        Inputs::AttributedList { call, behavior } => Call::AttributedList {
            call: Box::new(to_call(call)),
            behavior: *behavior,
        },
        Inputs::Checked {
            a,
            b,
            operation,
            level,
        } => Call::Checked {
            a: spec(a),
            b: spec(b),
            operation: *operation,
            level: *level,
        },
    }
}

// ---- outputs ------------------------------------------------------------------------

fn to_output(o: &[f64]) -> Result<Output, HarnessError> {
    let (&status, rest) = o.split_first().ok_or_else(|| harness("empty record"))?;
    let ints = rest
        .iter()
        .map(|&x| {
            if x.fract() == 0.0 && x.abs() < 1e15 {
                Ok(x as i64)
            } else {
                Err(harness(format!("non-integer record entry {x}")))
            }
        })
        .collect::<Result<Vec<i64>, _>>()?;
    Ok(Output::new()
        .int("status", status as i64)
        .ints("record", ints))
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    if ub_242(i)? {
        return Ok(Output::rejected(
            "UB-242: IntersectInsertRelate ignores a failed imprint that edited a B-rep \
             and goes on with the half-edited B-reps; not run",
        ));
    }
    match ffi::run(&to_call(i)) {
        Ok(o) => match i {
            Inputs::Measured { .. } => measured_output(&o, "measures"),
            Inputs::Attributed { .. } => measured_output(&o, "attributes"),
            Inputs::AttributedList { .. } if o.first() == Some(&0.0) => {
                let (head, rows) = ffi::split_measures(&o)
                    .ok_or_else(|| harness("cpp: attributed record too short"))?;
                Ok(to_output(head)?.values("attributes", rows.to_vec()))
            }
            Inputs::Checked { .. } => checked_output(&o),
            Inputs::Boolean2d { .. } => {
                areas_output(&o, o.first() == Some(&0.0) && o.get(1) == Some(&1.0))
            }
            Inputs::Sweep {
                attributed: Some(_),
                ..
            }
            | Inputs::RotationalSweep {
                attributed: Some(_),
                ..
            } if o.first() == Some(&0.0) => {
                let (head, rows) = ffi::split_measures(&o)
                    .ok_or_else(|| harness("cpp: attributed sweep record too short"))?;
                Ok(areas_output(head, true)?.values("attributes", rows.to_vec()))
            }
            Inputs::Sweep { .. } | Inputs::RotationalSweep { .. } => {
                areas_output(&o, o.first() == Some(&0.0))
            }
            Inputs::TessellateBoundaries { .. } => tessellate_output(&o),
            _ => to_output(&o),
        },
        Err(e) => Err(harness(format!("cpp: {e}"))),
    }
}

/// A `measured` (`attributed`) record: the integer record, then on success (`status` 0
/// and a result) the measures (attribute rows) the shim appends under `key`
/// ([`ffi::split_measures`]).
fn measured_output(o: &[f64], key: &str) -> Result<Output, HarnessError> {
    let succeeded = o.first() == Some(&0.0) && o.get(1) == Some(&1.0);
    if !succeeded {
        return to_output(o);
    }
    let (head, measures) =
        ffi::split_measures(o).ok_or_else(|| harness("cpp: measured record too short"))?;
    Ok(to_output(head)?.values(key, measures.to_vec()))
}

/// A `checked` record: the integer record, then on success (`status` 0 and a result) the
/// check the shim appends, split off as the integers `check` ([`ffi::split_measures`]).
fn checked_output(o: &[f64]) -> Result<Output, HarnessError> {
    let succeeded = o.first() == Some(&0.0) && o.get(1) == Some(&1.0);
    if !succeeded {
        return to_output(o);
    }
    let (head, check) =
        ffi::split_measures(o).ok_or_else(|| harness("cpp: checked record too short"))?;
    let check = check
        .iter()
        .map(|&x| {
            if x.fract() == 0.0 && x.abs() < 1e15 {
                Ok(x as i64)
            } else {
                Err(harness(format!("non-integer check entry {x}")))
            }
        })
        .collect::<Result<Vec<i64>, _>>()?;
    Ok(to_output(head)?.ints("check", check))
}

/// A record that ends, when `has_areas`, with the shim's `put_areas` tail (split off as
/// `areas`, the same way as [`measured_output`]).
fn areas_output(o: &[f64], has_areas: bool) -> Result<Output, HarnessError> {
    if !has_areas {
        return to_output(o);
    }
    let (head, areas) =
        ffi::split_measures(o).ok_or_else(|| harness("cpp: areas record too short"))?;
    Ok(to_output(head)?.values("areas", areas.to_vec()))
}

/// A `tessellate_boundaries` record: `status ne edges.. nc counts.. np xyz..` (a failed
/// torus operand: `status 2`).
fn tessellate_output(o: &[f64]) -> Result<Output, HarnessError> {
    if o.len() == 2 && o.get(1) == Some(&2.0) {
        return to_output(o);
    }
    let short = || harness("cpp: tessellate record too short");
    let (&status, rest) = o.split_first().ok_or_else(short)?;
    let int = |x: f64| {
        if x.fract() == 0.0 && x.abs() < 1e15 {
            Ok(x as i64)
        } else {
            Err(harness(format!("non-integer record entry {x}")))
        }
    };
    let mut ints = Vec::new();
    let mut at = 0usize;
    // the edge and count lists, each after its length
    for _ in 0..2 {
        let n = int(*rest.get(at).ok_or_else(short)?)?;
        let len = usize::try_from(n).map_err(|_| short())?;
        let list = rest.get(at..at + 1 + len).ok_or_else(short)?;
        for &x in list {
            ints.push(int(x)?);
        }
        at += 1 + len;
    }
    let np = usize::try_from(int(*rest.get(at).ok_or_else(short)?)?).map_err(|_| short())?;
    let points = rest
        .get(at + 1..at + 1 + 3 * np)
        .ok_or_else(short)?
        .to_vec();
    Ok(Output::new()
        .int("status", int(status)?)
        .ints("record", ints)
        .values("points", points))
}

/// Whether the Boolean reaches UB-242 (`plan/upstream-issues/`): an imprint in
/// `IntersectInsertRelate` fails after it may have edited a B-rep, and upstream goes on
/// with the half-edited B-reps, so the input is undefined in C++ (AGENTS.md section 4,
/// step 5.6) and [`cpp`] rejects it. Decided from the inputs and the Rust side only: the
/// port stops there (tangency-labs/smlib af8db37) and records `"UB-242"` on the kernel
/// ([`Kernel::upstream_ub_passed`]). (Until the v1.0.1 pin this guard was UB-175's, the
/// proto topology manager teardown, which v1.0.1 defines.)
fn ub_242(i: &Inputs) -> Result<bool, HarnessError> {
    let mut k = Kernel::default();
    match rust_call_with(&mut k, i, &|_| {}) {
        Ok(_) | Err(Stop::Deferred(_) | Stop::Torus(_)) => {
            Ok(k.upstream_ub_passed.contains(&"UB-242"))
        }
        Err(Stop::Harness(e)) => Err(e),
    }
}

/// The `SmStatus` of a Rust error; `-1` for one upstream cannot return.
fn status(e: &SolidError) -> i64 {
    match e {
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => SM_ERR_INVALID_INPUT,
        SolidError::Upstream { code, .. } => i64::from(*code),
        _ => -1,
    }
}

fn op(code: i32) -> Result<BooleanOperation, HarnessError> {
    BooleanOperation::from_code(code).ok_or_else(|| harness(format!("operation {code}")))
}

fn options(f: FlagsIn) -> MergeOptions {
    MergeOptions::new(
        f.cookie_cutter,
        f.imprinting,
        f.boolean_post_process,
        f.imprint_and_classify_faces,
        f.keep_other_brep,
    )
}

/// Why a Rust run stopped early: `Unported` defers the case; a failing torus operand is
/// the call's record (`status 2`, as the shim's `TorusFailed`); anything else is a
/// harness error, as the shim treats a failing operand constructor (`SC_ERR_ARG`).
enum Stop {
    Deferred(Box<Output>),
    Torus(Box<Output>),
    Harness(HarnessError),
}

impl From<HarnessError> for Stop {
    fn from(e: HarnessError) -> Self {
        Self::Harness(e)
    }
}

fn stop(what: &str, e: &SolidError) -> Stop {
    match Output::unported(e) {
        Some(d) => Stop::Deferred(Box::new(d)),
        None => Stop::Harness(harness(format!("rust: {what}: {e}"))),
    }
}

fn p3(p: P3) -> Point3 {
    Point3::new(p[0], p[1], p[2])
}

/// Builds an operand exactly as the shim does (`SmApiCreateBox`, `SmApiCreateSphere`,
/// `SmApiCreatePlane`, `SmApiBoolean`). Nested
/// operands are at most `ffi::MAX_DEPTH` deep (the FFI rejects deeper ones first).
fn build(k: &mut Kernel, b: &BrepIn) -> Result<Brep, Stop> {
    build_at(k, b, 0)
}

fn build_at(k: &mut Kernel, b: &BrepIn, depth: usize) -> Result<Brep, Stop> {
    if depth >= ffi::MAX_DEPTH {
        return Err(Stop::Harness(harness("operand nested too deep")));
    }
    let r = match b {
        &BrepIn::Box { origin, size } => {
            primitives::create_box(k, p3(origin), size[0], size[1], size[2])
        }
        &BrepIn::Sphere { center, radius } => primitives::create_sphere(k, p3(center), radius),
        &BrepIn::Cylinder {
            origin,
            radius,
            height,
        } => primitives::create_cylinder(k, p3(origin), radius, height),
        &BrepIn::Plane { origin, size } => {
            primitives::create_plane(k, p3(origin), size[0], size[1])
        }
        &BrepIn::Torus {
            center,
            major,
            minor,
        } => {
            return primitives::create_torus(k, p3(center), major, minor).map_err(|e| {
                match Output::unported(&e) {
                    Some(d) => Stop::Deferred(Box::new(d)),
                    None => Stop::Torus(Box::new(record(status(&e), vec![2]))),
                }
            });
        }
        BrepIn::Moved { steps, brep } => {
            let mut b = build_at(k, brep, depth + 1)?;
            for m in steps {
                let moved = match *m {
                    MoveIn::Rotate {
                        point,
                        axis,
                        angle_deg,
                    } => api_rotate(
                        k,
                        Some(TransformObject::Brep(&mut b)),
                        p3(point),
                        p3(axis),
                        angle_deg,
                    ),
                    MoveIn::Translate { vector } => {
                        api_translate(k, Some(TransformObject::Brep(&mut b)), p3(vector))
                    }
                };
                moved.map_err(|e| stop("operand move", &e))?;
            }
            Ok(b)
        }
        BrepIn::Boolean { operation, a, b } => {
            let (a, b) = (build_at(k, a, depth + 1)?, build_at(k, b, depth + 1)?);
            api::boolean(k, a, b, op(*operation)?).map(|out| out.result)
        }
        BrepIn::Skin {
            counts,
            curves,
            degree,
            cap_ends,
        } => {
            let cs = curves
                .iter()
                .map(|c| {
                    let pw = c
                        .pw()
                        .iter()
                        .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
                        .collect();
                    NurbCurve::new(pw, c.p, c.knots.clone())
                        .map(|n| SmCurve::from(BSplineCurve::from_nurb(n, 3, false)))
                        .map_err(|e| stop("skin curve", &e))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let refs: Vec<Option<&SmCurve>> = cs.iter().map(Some).collect();
            let counts: Vec<usize> = counts.iter().map(|&c| c as usize).collect();
            create_skin_primitive(k, &counts, &refs, *degree as usize, *cap_ends).map(|s| s.brep)
        }
        &BrepIn::Disk { origin, radius } => primitives::create_planar_circle(k, p3(origin), radius),
        &BrepIn::Cone {
            origin,
            radius_base,
            radius_top,
            height,
        } => primitives::create_cone(k, p3(origin), radius_base, radius_top, height),
    };
    r.map_err(|e| stop("operand constructor", &e))
}

fn build_all(k: &mut Kernel, l: &[BrepIn]) -> Result<Vec<Brep>, Stop> {
    l.iter().map(|b| build(k, b)).collect()
}

/// A Rust error as a record (status only); deferred for `Unported`.
fn err_output(e: &SolidError) -> Output {
    Output::unported(e).unwrap_or_else(|| record(status(e), Vec::new()))
}

fn record(status: i64, record: Vec<i64>) -> Output {
    Output::new().int("status", status).ints("record", record)
}

fn count(n: usize) -> Result<i64, HarnessError> {
    i64::try_from(n).map_err(|_| harness("count overflow"))
}

/// `faces edges vertices regions manifold_solid` of a B-rep, then per region
/// `is_void is_infinite shells` and per shell `faceuses mate_region` (the shim's `put_brep`).
fn put_brep(o: &mut Vec<i64>, b: &Brep) -> Result<(), Stop> {
    let regions = brep_p1::regions(b, None).map_err(|e| stop("GetRegions", &e))?;
    let manifold = brep_p2::is_manifold_solid(b).map_err(|e| stop("IsManifoldSolid", &e))?;
    o.push(count(b.face_list().len())?);
    o.push(count(b.edge_list().len())?);
    o.push(count(b.vertex_list().len())?);
    o.push(count(regions.len())?);
    o.push(i64::from(manifold));
    for &r in &regions {
        let region = b.region(r).map_err(|e| stop("GetRegion", &e))?;
        let shells = region
            .get_shells(None, |_, _| Ok(false))
            .map_err(|e| stop("GetShells", &e))?;
        o.push(i64::from(region.is_void()));
        o.push(i64::from(region.is_infinite_region(b, r)));
        o.push(count(shells.len())?);
        for s in shells {
            let shell = b.shell(s).map_err(|e| stop("GetShell", &e))?;
            let faceuses = shell.get_faceuses().map_err(|e| stop("GetFaceuses", &e))?;
            o.push(count(faceuses.len())?);
            o.push(mate_region(b, shell.first_faceuse(), &regions)?);
        }
    }
    Ok(())
}

/// Index in `regions` of the region on the other side of `fu` (the shell's first
/// faceuse), `-1` without one (the shim's `mate_region`).
fn mate_region(
    b: &Brep,
    fu: solidcore::Result<Option<FaceuseId>>,
    regions: &[RegionId],
) -> Result<i64, Stop> {
    let Some(fu) = fu.map_err(|e| stop("GetFirstFaceuse", &e))? else {
        return Ok(-1);
    };
    let Some(mate) = edgeuse_p1::faceuse_mate(b, fu).map_err(|e| stop("GetMate", &e))? else {
        return Ok(-1);
    };
    let shell = edgeuse_p1::faceuse_shell(b, mate).map_err(|e| stop("GetShell", &e))?;
    let region = match shell {
        Some(s) => shell_region(b, s).map_err(|e| stop("GetRegion", &e))?,
        None => None,
    };
    match region.and_then(|m| regions.iter().position(|&r| r == m)) {
        Some(k) => count(k),
        None => Ok(-1),
    }
    .map_err(Stop::from)
}

/// The shim's `put_measures`: `volume_status volume faces` and the face areas in
/// ascending order (a failed face as minus its status). An unported callee defers.
fn measures(k: &Kernel, b: &mut Brep) -> Result<Vec<f64>, Stop> {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let fail = |e: &SolidError| -> Result<f64, Stop> {
        match Output::unported(e) {
            Some(d) => Err(Stop::Deferred(Box::new(d))),
            None => Ok(status(e) as f64),
        }
    };
    let mut o = match brep_volume(k, b, ffi::VOLUME_ACCURACY, &attrs) {
        Ok(v) => vec![0.0, v],
        Err(e) => vec![fail(&e)?, 0.0],
    };
    let faces = brep_p1::faces(b, None).map_err(|e| stop("GetFaces", &e))?;
    o.push(faces.len() as f64);
    let mut areas = Vec::new();
    for f in faces {
        areas.push(match face_area(k, b, f, ffi::AREA_ACCURACY) {
            Ok(a) => a,
            Err(e) => -fail(&e)?,
        });
    }
    areas.sort_by(f64::total_cmp);
    o.extend(areas);
    Ok(o)
}

/// The shim's `put_failed_tests`: `groups` then per failed report's (reporting type,
/// list index, test index), in ascending order, `type list test count`.
fn failed_tests(o: &mut Vec<i64>, reports: &AssertArray) -> Result<(), Stop> {
    let mut groups: std::collections::BTreeMap<(i64, i64, i64), i64> =
        std::collections::BTreeMap::new();
    for r in reports.reports().iter().filter(|r| !r.ok) {
        let list = i64::try_from(r.list_index).map_err(|_| harness("list index overflow"))?;
        let test = i64::try_from(r.test_index).map_err(|_| harness("test index overflow"))?;
        *groups.entry((r.reporting_type, list, test)).or_insert(0) += 1;
    }
    o.push(count(groups.len())?);
    for ((t, l, i), n) in groups {
        o.extend([t, l, i, n]);
    }
    Ok(())
}

/// The shim's `run_checked` tail: the capi check of `b` at `level` (`check_brep`), as
/// `pointers_status failed... assert_valid failed...` (`assert_valid` `-1` when skipped).
fn check(k: &mut Kernel, b: &mut Brep, level: u8) -> Result<Vec<i64>, Stop> {
    let level = match level {
        0 => CheckLevel::Level0,
        1 => CheckLevel::Level1,
        2 => CheckLevel::Level2,
        _ => return Err(Stop::Harness(harness(format!("check level {level}")))),
    };
    let c = check_brep(k, b, level).map_err(|e| stop("check_brep", &e))?;
    let mut o = vec![i64::from(c.pointers_status)];
    failed_tests(&mut o, &c.pointer_reports)?;
    o.push(c.assert_valid.map_or(-1, i64::from));
    failed_tests(&mut o, &c.reports)?;
    Ok(o)
}

/// The shim's `tag_operand`: every face (`GetFaces` order) and edge (`GetEdges` order) of
/// operand `operand` gets its own long attribute, id = value =
/// `ATTRIBUTE_BASE + 1000 operand + 500 kind + index`, behaviour `behavior`.
fn tag_operand(b: &mut Brep, operand: u64, behavior: Behavior) -> Result<(), Stop> {
    let faces = brep_p1::faces(b, None).map_err(|e| stop("GetFaces", &e))?;
    let edges = brep_p1::edges(b, None).map_err(|e| stop("GetEdges", &e))?;
    let tagged = faces
        .into_iter()
        .enumerate()
        .map(|(i, f)| (EntityRef::Face(f), 0, i))
        .chain(
            edges
                .into_iter()
                .enumerate()
                .map(|(i, e)| (EntityRef::Edge(e), 1, i)),
        );
    for (entity, kind, index) in tagged {
        let id = ffi::ATTRIBUTE_BASE + 1000 * operand + 500 * kind + index as u64;
        let value = i64::try_from(id).map_err(|_| harness("attribute id overflow"))?;
        b.add_attribute(entity, Attribute::long(id, value, behavior))
            .map_err(|e| stop("AddAttribute", &e))?;
    }
    Ok(())
}

/// The shim's `attribute_row`: `measure n` then per attribute `id behavior value users`,
/// the attributes in ascending order (the set is compared, not the list order).
fn attribute_row(b: &Brep, entity: EntityRef, measure: f64) -> Vec<f64> {
    let mut entries: Vec<[f64; 4]> = b
        .entity_attributes(entity)
        .into_iter()
        .map(|(_, a)| {
            [
                a.attribute_id() as f64,
                f64::from(a.behavior() as u8),
                a.long_value().map_or(0.0, |v| v as f64),
                a.users().len() as f64,
            ]
        })
        .collect();
    entries.sort_by(|x, y| {
        x.iter()
            .zip(y)
            .map(|(p, q)| p.total_cmp(q))
            .find(|o| o.is_ne())
            .unwrap_or(core::cmp::Ordering::Equal)
    });
    let mut row = vec![measure, entries.len() as f64];
    row.extend(entries.into_iter().flatten());
    row
}

/// The shim's `put_attributes`: the sorted face rows (area) then edge rows (length).
fn put_attributes(k: &Kernel, o: &mut Vec<f64>, b: &mut Brep) -> Result<(), Stop> {
    let fail = |e: &SolidError| -> Result<f64, Stop> {
        match Output::unported(e) {
            Some(d) => Err(Stop::Deferred(Box::new(d))),
            None => Ok(-(status(e) as f64)),
        }
    };
    let faces = brep_p1::faces(b, None).map_err(|e| stop("GetFaces", &e))?;
    let mut rows = Vec::new();
    for f in faces {
        let area = match face_area(k, b, f, ffi::AREA_ACCURACY) {
            Ok(a) => a,
            Err(e) => fail(&e)?,
        };
        rows.push(attribute_row(b, EntityRef::Face(f), area));
    }
    ffi::sort_rows(&mut rows);
    o.push(rows.len() as f64);
    o.extend(rows.into_iter().flatten());
    let edges = brep_p1::edges(b, None).map_err(|e| stop("GetEdges", &e))?;
    let mut rows = Vec::new();
    for e in edges {
        let length = match edge_length(k, b, e, ffi::LENGTH_ACCURACY) {
            Ok(l) => l,
            Err(err) => fail(&err)?,
        };
        rows.push(attribute_row(b, EntityRef::Edge(e), length));
    }
    ffi::sort_rows(&mut rows);
    o.push(rows.len() as f64);
    o.extend(rows.into_iter().flatten());
    Ok(())
}

/// The shim's `tag_all`: B-rep `i` of `breps` tagged as operand `i`.
fn tag_all(breps: &mut [Brep], behavior: Behavior) -> Result<(), Stop> {
    for (i, b) in breps.iter_mut().enumerate() {
        tag_operand(b, i as u64, behavior)?;
    }
    Ok(())
}

/// An `attributed_list` call (the shim's `run_attributed_list`): the inner list call on
/// tagged inputs, a successful record followed by each output B-rep's attribute rows.
fn attributed_list(
    k: &mut Kernel,
    call: &Inputs,
    behavior: Behavior,
    seen: &dyn Fn(&SolidError),
) -> Result<Output, Stop> {
    let err_output = |e: &SolidError| {
        seen(e);
        err_output(e)
    };
    match call {
        Inputs::MergeBreps {
            breps: list,
            operation,
        } => {
            let mut v = build_all(k, list)?;
            tag_all(&mut v, behavior)?;
            match api::merge_breps(k, &mut v, op(*operation)?) {
                Ok(mut res) => {
                    let mut o = vec![count(v.len())?, 1];
                    put_brep(&mut o, &res)?;
                    let mut rows = Vec::new();
                    put_attributes(k, &mut rows, &mut res)?;
                    Ok(record(0, o).values("attributes", rows))
                }
                Err(e) if matches!(e, SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. }) => {
                    seen(&e);
                    Ok(record(status(&e), vec![1, count(v.len())?]))
                }
                Err(e) => Ok(err_output(&e)),
            }
        }
        Inputs::Lists {
            breps1,
            breps2,
            operation,
        } => {
            let mut all = build_all(k, breps1)?;
            all.extend(build_all(k, breps2)?);
            tag_all(&mut all, behavior)?;
            let b2 = all.split_off(breps1.len());
            match api::boolean_lists(k, &all, &[], &b2, &[], *operation) {
                Ok(mut res) => {
                    let mut o = vec![count(res.breps.len())?];
                    for b in &res.breps {
                        o.push(1);
                        put_brep(&mut o, b)?;
                    }
                    o.push(count(res.surfaces.len())?);
                    let mut rows = Vec::new();
                    for b in &mut res.breps {
                        put_attributes(k, &mut rows, b)?;
                    }
                    Ok(record(0, o).values("attributes", rows))
                }
                Err(e) => Ok(err_output(&e)),
            }
        }
        Inputs::Trees { breps: list, tree } => {
            let mut built = build_all(k, list)?;
            tag_all(&mut built, behavior)?;
            let mut v: Vec<Option<Brep>> = built.into_iter().map(Some).collect();
            match api::boolean_trees(k, &mut v, tree) {
                Ok(()) => {
                    let mut o = vec![count(v.len())?];
                    for b in &v {
                        o.push(i64::from(b.is_some()));
                        if let Some(b) = b {
                            put_brep(&mut o, b)?;
                        }
                    }
                    let mut rows = Vec::new();
                    for b in v.iter_mut().flatten() {
                        put_attributes(k, &mut rows, b)?;
                    }
                    Ok(record(0, o).values("attributes", rows))
                }
                Err(e) => Ok(err_output(&e)),
            }
        }
        _ => Err(Stop::Harness(harness(
            "attributed_list call is not merge_breps, lists or trees",
        ))),
    }
}

/// The shim's `put_areas`: `faces` and the face areas in ascending order (a failed face
/// as minus its status). An unported callee defers.
fn areas(k: &Kernel, b: &mut Brep) -> Result<Vec<f64>, Stop> {
    let faces = brep_p1::faces(b, None).map_err(|e| stop("GetFaces", &e))?;
    let mut o = vec![faces.len() as f64];
    let mut areas = Vec::new();
    for f in faces {
        areas.push(match face_area(k, b, f, ffi::AREA_ACCURACY) {
            Ok(a) => a,
            Err(e) => match Output::unported(&e) {
                Some(d) => return Err(Stop::Deferred(Box::new(d))),
                None => -(status(&e) as f64),
            },
        });
    }
    areas.sort_by(f64::total_cmp);
    o.extend(areas);
    Ok(o)
}

/// `Sm2DBooleanOperationType` (`SmPrimitiveCreation.h`).
fn op_2d(code: i32) -> Result<api::Boolean2d, HarnessError> {
    Ok(match code {
        0 => api::Boolean2d::Union,
        1 => api::Boolean2d::Intersection,
        2 => api::Boolean2d::Difference,
        3 => api::Boolean2d::ExclusiveOr,
        4 => api::Boolean2d::Merge,
        _ => return Err(harness(format!("2D operation {code}"))),
    })
}

/// The ids `sel` names in `b` (the shim's `read_selection`; an index past the list is a
/// harness error, as the shim's `SC_ERR_ARG`).
fn pick_ids<T: Copy>(
    sel: Option<&[Option<usize>]>,
    all: &[T],
) -> Result<Option<Vec<Option<T>>>, Stop> {
    let Some(sel) = sel else { return Ok(None) };
    if sel.len() > ffi::MAX_SELECTION {
        return Err(Stop::Harness(harness("selection too long")));
    }
    sel.iter()
        .map(|e| match e {
            None => Ok(None),
            Some(i) => all
                .get(*i)
                .copied()
                .map(Some)
                .ok_or_else(|| Stop::Harness(harness(format!("selection index {i}")))),
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

/// The selected faces, edges and vertices of `b`.
type Picked = (
    Option<Vec<Option<FaceId>>>,
    Option<Vec<Option<EdgeId>>>,
    Option<Vec<Option<VertexId>>>,
);

fn picked(b: &Brep, s: &SelectionIn) -> Result<Picked, Stop> {
    let faces = brep_p1::faces(b, None).map_err(|e| stop("GetFaces", &e))?;
    let edges = brep_p1::edges(b, None).map_err(|e| stop("GetEdges", &e))?;
    let vertices = brep_p1::vertices(b, None).map_err(|e| stop("GetVertices", &e))?;
    Ok((
        pick_ids(s.faces.as_deref(), &faces)?,
        pick_ids(s.edges.as_deref(), &edges)?,
        pick_ids(s.vertices.as_deref(), &vertices)?,
    ))
}

/// The second B-rep of an `other` region (the shim's `SmApiCreateBox(origin, 1, 1, 1)`;
/// a failing constructor is a harness error, as the shim's `SC_ERR_ARG`).
fn region_other(k: &mut Kernel, region: RegionIn) -> Result<Option<Brep>, Stop> {
    match region {
        RegionIn::Other => primitives::create_box(k, Point3::new(0.0, 0.0, 0.0), 1.0, 1.0, 1.0)
            .map(Some)
            .map_err(|e| stop("region B-rep", &e)),
        RegionIn::Null | RegionIn::Swept => Ok(None),
    }
}

/// The `pRegionBrep` argument of `region`, `other` its second B-rep.
fn region_arg(region: RegionIn, other: Option<&Brep>) -> Option<RegionBrep<'_>> {
    match (region, other) {
        (RegionIn::Null, _) => None,
        (RegionIn::Other, Some(b)) => Some(RegionBrep::Other(b)),
        _ => Some(RegionBrep::Swept),
    }
}

/// A sweep's record: the status, then the B-rep on success or `SM_ERR_INVALID_INPUT`
/// and its areas on success (the shim's `run_sweep`).
fn sweep_record(
    k: &Kernel,
    r: solidcore::Result<()>,
    b: &mut Brep,
    attributed: bool,
    seen: &dyn Fn(&SolidError),
) -> Result<Output, Stop> {
    match r {
        Ok(()) => {
            let mut o = Vec::new();
            put_brep(&mut o, b)?;
            let a = areas(k, b)?;
            let out = record(0, o).values("areas", a);
            if !attributed {
                return Ok(out);
            }
            let mut rows = Vec::new();
            put_attributes(k, &mut rows, b)?;
            Ok(out.values("attributes", rows))
        }
        Err(e) if matches!(e, SolidError::InvalidInput { .. }) => {
            seen(&e);
            let mut o = Vec::new();
            put_brep(&mut o, b)?;
            Ok(record(status(&e), o))
        }
        Err(e) => {
            seen(&e);
            Ok(err_output(&e))
        }
    }
}

/// Whether upstream keeps the second operand of a successful two-B-rep boolean
/// (`source/SM_API/inc/SmApiBrep.h`; the shim's `keeps_other`).
fn keeps_other(operation: i32, keep_option: bool) -> bool {
    matches!(
        BooleanOperation::from_code(operation),
        Some(BooleanOperation::PartialMerge | BooleanOperation::ImprintClassify)
    ) || keep_option
}

/// The two-B-rep record `has is_first kept brep [brep]`. The Rust result is the first
/// operand modified in place ([`api::BooleanOutcome::result`]), so `is_first` is 1.
fn two_record(out: &api::BooleanOutcome, kept: bool) -> Result<Output, Stop> {
    Ok(record(0, two_ints(out, kept)?))
}

/// The integers of [`two_record`].
fn two_ints(out: &api::BooleanOutcome, kept: bool) -> Result<Vec<i64>, Stop> {
    let mut o = vec![1, 1, i64::from(kept)];
    put_brep(&mut o, &out.result)?;
    if kept {
        let other = out
            .other
            .as_ref()
            .ok_or_else(|| harness("rust: kept operand missing"))?;
        put_brep(&mut o, other)?;
    }
    Ok(o)
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let mut k = Kernel::default();
    match rust_call(&mut k, i) {
        Ok(o) => Ok(o),
        Err(Stop::Deferred(o) | Stop::Torus(o)) => Ok(*o),
        Err(Stop::Harness(e)) => Err(e),
    }
}

fn rust_call(k: &mut Kernel, i: &Inputs) -> Result<Output, Stop> {
    rust_call_with(k, i, &|_| {})
}

/// [`rust_call`], showing every error the API returned to `seen`.
fn rust_call_with(k: &mut Kernel, i: &Inputs, seen: &dyn Fn(&SolidError)) -> Result<Output, Stop> {
    let err_output = |e: &SolidError| {
        seen(e);
        err_output(e)
    };
    let two = |r: solidcore::Result<api::BooleanOutcome>, kept: bool| match r {
        Ok(out) => two_record(&out, kept),
        Err(e) => Ok(err_output(&e)),
    };
    let lone = |r: solidcore::Result<Brep>| {
        two(
            r.map(|result| api::BooleanOutcome {
                result,
                other: None,
            }),
            false,
        )
    };
    match i {
        Inputs::Boolean { a, b, operation } => {
            let (a, b) = (build(k, a)?, build(k, b)?);
            two(
                api::boolean(k, a, b, op(*operation)?),
                keeps_other(*operation, false),
            )
        }
        Inputs::Union { a, b } => {
            let (a, b) = (build(k, a)?, build(k, b)?);
            lone(api::boolean_union(k, a, b))
        }
        Inputs::Difference { a, b } => {
            let (a, b) = (build(k, a)?, build(k, b)?);
            lone(api::boolean_difference(k, a, b))
        }
        Inputs::Intersection { a, b } => {
            let (a, b) = (build(k, a)?, build(k, b)?);
            lone(api::boolean_intersection(k, a, b))
        }
        Inputs::Merge { a, b } => {
            let (a, b) = (build(k, a)?, build(k, b)?);
            lone(api::boolean_merge(k, a, b))
        }
        Inputs::WithOptions {
            a,
            b,
            operation,
            flags,
        } => {
            let (a, b) = (build(k, a)?, build(k, b)?);
            two(
                api::boolean_with_options(k, a, b, op(*operation)?, options(*flags)),
                keeps_other(*operation, flags.keep_other_brep),
            )
        }
        Inputs::MergeBreps {
            breps: list,
            operation,
        } => {
            let mut v = build_all(k, list)?;
            match api::merge_breps(k, &mut v, op(*operation)?) {
                Ok(res) => {
                    let mut o = vec![count(v.len())?, 1];
                    put_brep(&mut o, &res)?;
                    Ok(record(0, o))
                }
                // Rejected: no result (`cleared`) and the list untouched.
                Err(e) if matches!(e, SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. }) => {
                    seen(&e);
                    Ok(record(status(&e), vec![1, count(v.len())?]))
                }
                Err(e) => Ok(err_output(&e)),
            }
        }
        Inputs::Lists {
            breps1,
            breps2,
            operation,
        } => {
            let b1 = build_all(k, breps1)?;
            let b2 = build_all(k, breps2)?;
            match api::boolean_lists(k, &b1, &[], &b2, &[], *operation) {
                Ok(res) => {
                    let mut o = vec![count(res.breps.len())?];
                    for b in &res.breps {
                        o.push(1);
                        put_brep(&mut o, b)?;
                    }
                    o.push(count(res.surfaces.len())?);
                    Ok(record(0, o))
                }
                Err(e) => Ok(err_output(&e)),
            }
        }
        Inputs::Measured {
            a,
            b,
            operation,
            post_process,
        } => {
            let (a, b) = (build(k, a)?, build(k, b)?);
            let flags = MergeOptions::new(false, false, *post_process, false, false);
            match api::boolean_with_options(k, a, b, op(*operation)?, flags) {
                Ok(mut out) => {
                    let rec = two_record(&out, keeps_other(*operation, false))?;
                    let m = measures(k, &mut out.result)?;
                    Ok(rec.values("measures", m))
                }
                Err(e) => Ok(err_output(&e)),
            }
        }
        Inputs::WithCurves { a, b, operation } => {
            let (a, b) = (build(k, a)?, build(k, b)?);
            match api::boolean_with_curves(k, a, b, op(*operation)?) {
                Ok(out) => {
                    // `n` and each intersection edge's index in the result's edges, -1
                    // when the result no longer has it (the shim's `edge_index`)
                    let mut o = two_ints(&out.outcome, keeps_other(*operation, false))?;
                    let edges = brep_p1::edges(&out.outcome.result, None)
                        .map_err(|e| stop("GetEdges", &e))?;
                    o.push(count(out.intersection_edges.len())?);
                    for e in &out.intersection_edges {
                        o.push(match edges.iter().position(|x| x == e) {
                            Some(i) => count(i)?,
                            None => -1,
                        });
                    }
                    Ok(record(0, o))
                }
                Err(e) => Ok(err_output(&e)),
            }
        }
        Inputs::Attributed {
            a,
            b,
            operation,
            behavior,
            flags,
            copy_first,
        } => {
            let behavior = Behavior::from_code(i32::from(*behavior))
                .map_err(|e| Stop::Harness(harness(format!("behavior: {e}"))))?;
            let (mut a, mut b) = (build(k, a)?, build(k, b)?);
            tag_operand(&mut a, 0, behavior)?;
            tag_operand(&mut b, 1, behavior)?;
            let kept = keeps_other(*operation, flags.is_some_and(|f| f.keep_other_brep));
            let run = match flags {
                None => api::boolean(k, a, b, op(*operation)?),
                Some(f) => {
                    let a = if *copy_first {
                        // SmApiBrepCopy of the tagged operand (an arena-only store,
                        // as the topology.brep_p3 copy runner's)
                        let mut state = solidcore::topology::brep::p2::ArenaNotifyState::default();
                        let mut env = state.env(None);
                        env.key_of = &super::topology_brep_p3::key_of;
                        match solidcore::topology::brep::p3::api_brep_copy(
                            k,
                            &mut env,
                            Some(&a),
                            &super::topology_brep_p3::source_key_of,
                        ) {
                            Ok(c) => c,
                            Err(e) => return Ok(err_output(&e)),
                        }
                    } else {
                        a
                    };
                    api::boolean_with_options(k, a, b, op(*operation)?, options(*f))
                }
            };
            match run {
                Ok(mut out) => {
                    let rec = two_record(&out, kept)?;
                    let mut rows = Vec::new();
                    put_attributes(k, &mut rows, &mut out.result)?;
                    if kept {
                        if let Some(other) = out.other.as_mut() {
                            put_attributes(k, &mut rows, other)?;
                        }
                    }
                    Ok(rec.values("attributes", rows))
                }
                Err(e) => Ok(err_output(&e)),
            }
        }
        Inputs::Checked {
            a,
            b,
            operation,
            level,
        } => {
            let (a, b) = (build(k, a)?, build(k, b)?);
            match api::boolean(k, a, b, op(*operation)?) {
                Ok(mut out) => {
                    let rec = two_record(&out, keeps_other(*operation, false))?;
                    let c = check(k, &mut out.result, *level)?;
                    Ok(rec.ints("check", c))
                }
                Err(e) => Ok(err_output(&e)),
            }
        }
        Inputs::NonManifold { a, b, operation } => {
            let (a, b) = (build(k, a)?, build(k, b)?);
            two(
                api::non_manifold_boolean(k, a, b, op(*operation)?),
                keeps_other(*operation, false),
            )
        }
        Inputs::Boolean2d { a, b, operation } => {
            let operation = op_2d(*operation)?;
            let (a, b) = (build(k, a)?, build(k, b)?);
            match api::boolean_2d(k, a, b, operation) {
                Ok(api::Boolean2dOutcome::Result(mut res)) => {
                    let mut o = vec![1];
                    put_brep(&mut o, &res)?;
                    let a = areas(k, &mut res)?;
                    Ok(record(0, o).values("areas", a))
                }
                Ok(api::Boolean2dOutcome::Unassigned { brep1, brep2 }) => {
                    let mut o = vec![0];
                    put_brep(&mut o, &brep1)?;
                    put_brep(&mut o, &brep2)?;
                    Ok(record(0, o))
                }
                Err(e) => Ok(err_output(&e)),
            }
        }
        Inputs::PiecewiseMerge { a, b, create_new } => {
            let (a, b) = (build(k, a)?, build(k, b)?);
            match api::piecewise_merge(k, a, b, *create_new) {
                Ok(out) => {
                    let mut o = vec![1, 1];
                    put_brep(&mut o, &out.result)?;
                    put_brep(&mut o, &out.other)?;
                    Ok(record(0, o))
                }
                Err(e) => Ok(err_output(&e)),
            }
        }
        Inputs::Sweep {
            brep,
            dir,
            dist,
            stitch,
            merge,
            region,
            selection: sel,
            attributed,
        } => {
            let mut b = build(k, brep)?;
            let (f, e, v) = picked(&b, sel)?;
            if let Some(behavior) = attributed {
                let behavior = Behavior::from_code(i32::from(*behavior))
                    .map_err(|e| Stop::Harness(harness(format!("behavior: {e}"))))?;
                tag_operand(&mut b, 0, behavior)?;
            }
            let other = region_other(k, *region)?;
            let selection = SweepSelection {
                faces: f.as_deref(),
                edges: e.as_deref(),
                vertices: v.as_deref(),
            };
            let dir = Vec3::new(dir[0], dir[1], dir[2]);
            let region = region_arg(*region, other.as_ref());
            let r = non_manifold_sweep(k, &mut b, region, dir, *dist, *stitch, *merge, selection);
            sweep_record(k, r, &mut b, attributed.is_some(), seen)
        }
        Inputs::RotationalSweep {
            brep,
            base,
            axis,
            angle_deg,
            stitch,
            merge,
            region,
            selection: sel,
            attributed,
        } => {
            let mut b = build(k, brep)?;
            let (f, e, v) = picked(&b, sel)?;
            if let Some(behavior) = attributed {
                let behavior = Behavior::from_code(i32::from(*behavior))
                    .map_err(|e| Stop::Harness(harness(format!("behavior: {e}"))))?;
                tag_operand(&mut b, 0, behavior)?;
            }
            let other = region_other(k, *region)?;
            let selection = SweepSelection {
                faces: f.as_deref(),
                edges: e.as_deref(),
                vertices: v.as_deref(),
            };
            let axis = Vec3::new(axis[0], axis[1], axis[2]);
            let region = region_arg(*region, other.as_ref());
            let r = non_manifold_rotational_sweep(
                k,
                &mut b,
                region,
                p3(*base),
                axis,
                *angle_deg,
                *stitch,
                *merge,
                selection,
            );
            sweep_record(k, r, &mut b, attributed.is_some(), seen)
        }
        Inputs::TessellateBoundaries {
            brep,
            angle_tol_deg,
            drop_curves,
        } => {
            let mut b = build(k, brep)?;
            let all = brep_p1::edges(&b, None).map_err(|e| stop("GetEdges", &e))?;
            for &d in drop_curves {
                let e = usize::try_from(d)
                    .ok()
                    .and_then(|d| all.get(d).copied())
                    .ok_or_else(|| Stop::Harness(harness("dropped edge index out of range")))?;
                // SetCurve(NULL, TRUE, FALSE, FALSE)
                b.edge_mut(e).map_err(|e| stop("SetCurve", &e))?.curve = None;
            }
            let (status, t) = match api::tessellate_boundaries(k, &b, *angle_tol_deg) {
                Ok(t) => (if t.complete { 0 } else { i64::from(SM_ERR) }, t),
                Err(e) if matches!(e, SolidError::InvalidInput { .. }) => {
                    seen(&e);
                    (status(&e), BoundaryTessellation::default())
                }
                Err(e) => return Ok(err_output(&e)),
            };
            let mut o = vec![count(t.edges.len())?];
            for e in &t.edges {
                let at = all.iter().position(|x| x == e).map_or(Ok(-1), count)?;
                o.push(at);
            }
            o.push(count(t.vertex_counts.len())?);
            for &n in &t.vertex_counts {
                o.push(count(n)?);
            }
            let points: Vec<f64> = t.points.iter().flat_map(|p| [p.x, p.y, p.z]).collect();
            Ok(record(status, o).values("points", points))
        }
        Inputs::AttributedList { call, behavior } => {
            let behavior = Behavior::from_code(i32::from(*behavior))
                .map_err(|e| Stop::Harness(harness(format!("behavior: {e}"))))?;
            attributed_list(k, call, behavior, seen)
        }
        Inputs::Trees { breps: list, tree } => {
            let mut v: Vec<Option<Brep>> = build_all(k, list)?.into_iter().map(Some).collect();
            match api::boolean_trees(k, &mut v, tree) {
                Ok(()) => {
                    let mut o = vec![count(v.len())?];
                    for b in &v {
                        o.push(i64::from(b.is_some()));
                        if let Some(b) = b {
                            put_brep(&mut o, b)?;
                        }
                    }
                    Ok(record(0, o))
                }
                Err(e) => Ok(err_output(&e)),
            }
        }
    }
}

// ---- generators ---------------------------------------------------------------------

/// A length scale: mostly moderate, sometimes tiny or huge. Tiny stops at `1e-3`:
/// upstream `SmApiCreateSphere` takes tens of seconds at radius `~5e-5`.
fn gen_scale(rng: &mut Rng) -> f64 {
    match rng.below(10) {
        0 => 10f64.powi(-(rng.range_i64(2, 3) as i32)),
        1 => 10f64.powi(rng.range_i64(2, 4) as i32),
        _ => rng.uniform(0.5, 20.0),
    }
}

/// An operand at scale `s` near `center`: a box or a sphere.
fn gen_brep(rng: &mut Rng, s: f64, center: P3) -> BrepIn {
    let jitter = rng.point3(-0.6 * s, 0.6 * s);
    let c = [
        center[0] + jitter[0],
        center[1] + jitter[1],
        center[2] + jitter[2],
    ];
    if rng.chance(0.65) {
        let size = [
            s * rng.uniform(0.3, 1.5),
            s * rng.uniform(0.3, 1.5),
            s * rng.uniform(0.3, 1.5),
        ];
        // Sometimes snap the corner onto the grid of the scale, so faces coincide.
        let origin = if rng.chance(0.2) {
            [center[0], center[1], center[2]]
        } else {
            c
        };
        BrepIn::Box { origin, size }
    } else {
        let r = s * rng.uniform(0.3, 1.0);
        BrepIn::Sphere {
            center: c,
            radius: if rng.chance(0.1) { -r } else { r },
        }
    }
}

fn gen_list(rng: &mut Rng, n: usize, s: f64) -> Vec<BrepIn> {
    let c = rng.point3(-2.0 * s, 2.0 * s);
    (0..n).map(|_| gen_brep(rng, s, c)).collect()
}

/// Operations other than union / intersection / difference / merge.
const NON_KERNEL_OPS: [i32; 7] = [0, 4, 6, 7, 8, 9, 10];
/// Operations the two-B-rep merge is exercised with (all but `SM_BO_EXTRACT_SEPARATE`).
const TWO_OPS: [i32; 10] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 10];

fn gen_flags(rng: &mut Rng) -> FlagsIn {
    FlagsIn {
        cookie_cutter: rng.chance(0.3),
        imprinting: rng.chance(0.2),
        boolean_post_process: rng.chance(0.7),
        imprint_and_classify_faces: rng.chance(0.2),
        keep_other_brep: rng.chance(0.3),
    }
}

fn pick(rng: &mut Rng, ops: &[i32]) -> i32 {
    rng.choose(ops).copied().unwrap_or(1)
}

fn gen_inputs(rng: &mut Rng) -> Inputs {
    let s = gen_scale(rng);
    let two = |rng: &mut Rng| {
        let c = rng.point3(-2.0 * s, 2.0 * s);
        (gen_brep(rng, s, c), gen_brep(rng, s, c))
    };
    match rng.below(20) {
        // ---- compared: input validation --------------------------------------------
        0..=2 => {
            let (a, b) = two(rng);
            Inputs::Boolean { a, b, operation: 9 }
        }
        3 | 4 => {
            let (a, b) = two(rng);
            Inputs::WithOptions {
                a,
                b,
                operation: 9,
                flags: gen_flags(rng),
            }
        }
        5 | 6 => {
            let n = rng.below(2) as usize;
            Inputs::MergeBreps {
                breps: gen_list(rng, n, s),
                operation: pick(rng, &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]),
            }
        }
        7..=9 => {
            let n = 2 + rng.below(4) as usize;
            Inputs::MergeBreps {
                breps: gen_list(rng, n, s),
                operation: pick(rng, &NON_KERNEL_OPS),
            }
        }
        // ---- compared: reach SmMerge ----------------------------------------------
        10 | 11 => {
            let (a, b) = two(rng);
            Inputs::Boolean {
                a,
                b,
                operation: pick(rng, &TWO_OPS),
            }
        }
        12 => {
            let (a, b) = two(rng);
            match rng.below(4) {
                0 => Inputs::Union { a, b },
                1 => Inputs::Difference { a, b },
                2 => Inputs::Intersection { a, b },
                _ => Inputs::Merge { a, b },
            }
        }
        13 | 14 => {
            let (a, b) = two(rng);
            Inputs::WithOptions {
                a,
                b,
                operation: pick(rng, &[1, 2, 3, 5]),
                flags: gen_flags(rng),
            }
        }
        15 | 16 => {
            let n = 2 + rng.below(2) as usize;
            Inputs::MergeBreps {
                breps: gen_list(rng, n, s),
                operation: pick(rng, &[1, 2, 3, 5]),
            }
        }
        17 | 18 => {
            let n1 = rng.below(3) as usize;
            let n2 = rng.below(3) as usize;
            Inputs::Lists {
                breps1: gen_list(rng, n1, s),
                breps2: gen_list(rng, n2, s),
                operation: rng.range_i64(-1, 5) as i32,
            }
        }
        _ => {
            let n = 2 + rng.below(2) as usize;
            let mut tree = vec![0, 1, -rng.range_i64(1, 4)];
            if n == 3 {
                tree.extend([2, -rng.range_i64(1, 4)]);
            }
            Inputs::Trees {
                breps: gen_list(rng, n, s),
                tree,
            }
        }
    }
}

/// A flat operand of the 2D boolean at scale `s` near `center`, in the plane `z`: a
/// rectangular sheet or a disk.
fn gen_flat(rng: &mut Rng, s: f64, center: P3, z: f64) -> BrepIn {
    let j = rng.point3(-0.6 * s, 0.6 * s);
    let origin = [center[0] + j[0], center[1] + j[1], z];
    if rng.chance(0.6) {
        BrepIn::Plane {
            origin,
            size: [s * rng.uniform(0.3, 1.5), s * rng.uniform(0.3, 1.5)],
        }
    } else {
        BrepIn::Disk {
            origin,
            radius: s * rng.uniform(0.2, 0.9),
        }
    }
}

/// A selection list of `n` of an operand's `len` entities, sometimes with a NULL entry.
fn gen_pick(rng: &mut Rng, len: usize) -> Option<Vec<Option<usize>>> {
    if len == 0 || rng.chance(0.5) {
        return None;
    }
    let n = 1 + rng.below(len.min(3) as u64) as usize;
    let mut l: Vec<Option<usize>> = (0..n)
        .map(|_| Some(rng.below(len as u64) as usize))
        .collect();
    if rng.chance(0.1) {
        l.push(None);
    }
    Some(l)
}

/// A sweep operand at scale `s` and its selection: a box (6 faces, 12 edges, 8 vertices)
/// or a sheet (1, 4, 4) with indexed selections, or a disk or sphere swept whole.
fn gen_swept(rng: &mut Rng, s: f64) -> (BrepIn, SelectionIn) {
    let c = rng.point3(-s, s);
    let (brep, counts) = match rng.below(6) {
        0..=2 => (
            BrepIn::Box {
                origin: c,
                size: [
                    s * rng.uniform(0.3, 1.5),
                    s * rng.uniform(0.3, 1.5),
                    s * rng.uniform(0.3, 1.5),
                ],
            },
            Some([6, 12, 8]),
        ),
        3 | 4 => (
            BrepIn::Plane {
                origin: c,
                size: [s * rng.uniform(0.3, 1.5), s * rng.uniform(0.3, 1.5)],
            },
            Some([1, 4, 4]),
        ),
        _ => (
            BrepIn::Disk {
                origin: c,
                radius: s * rng.uniform(0.2, 0.9),
            },
            None,
        ),
    };
    let sel = match counts {
        Some([f, e, v]) => SelectionIn {
            faces: gen_pick(rng, f),
            edges: gen_pick(rng, e),
            vertices: gen_pick(rng, v),
        },
        None => SelectionIn::default(),
    };
    (brep, sel)
}

/// A `pRegionBrep`: mostly the swept B-rep, sometimes `NULL`, rarely another B-rep.
fn gen_region(rng: &mut Rng) -> RegionIn {
    match rng.below(10) {
        0 | 1 => RegionIn::Null,
        2 => RegionIn::Other,
        _ => RegionIn::Swept,
    }
}

/// A unit-ish direction, sometimes axis-aligned, sometimes long or zero.
fn gen_dir(rng: &mut Rng) -> P3 {
    match rng.below(10) {
        0 => [0.0, 0.0, 0.0],
        1..=3 => {
            let mut d = [0.0; 3];
            if let Some(x) = d.get_mut(rng.below(3) as usize) {
                *x = if rng.chance(0.5) { 1.0 } else { -3.0 };
            }
            d
        }
        _ => rng.point3(-1.0, 1.0),
    }
}

/// A case of the entry points added after the first set (the `more` stream).
fn gen_more(rng: &mut Rng) -> Inputs {
    let s = gen_scale(rng);
    let two = |rng: &mut Rng| {
        let c = rng.point3(-2.0 * s, 2.0 * s);
        (gen_brep(rng, s, c), gen_brep(rng, s, c))
    };
    match rng.below(16) {
        0..=2 => {
            let (a, b) = two(rng);
            let operation = if rng.chance(0.1) {
                9
            } else {
                pick(rng, &TWO_OPS)
            };
            Inputs::WithCurves { a, b, operation }
        }
        3..=5 => {
            let (a, b) = two(rng);
            let operation = if rng.chance(0.1) {
                9
            } else {
                pick(rng, &TWO_OPS)
            };
            Inputs::NonManifold { a, b, operation }
        }
        6..=8 => {
            let c = rng.point3(-2.0 * s, 2.0 * s);
            let z = c[2];
            let a = gen_flat(rng, s, c, z);
            // sometimes on another plane: SmPrimitiveCreation::Boolean2D rejects it
            let zb = if rng.chance(0.15) { z + s } else { z };
            let b = gen_flat(rng, s, c, zb);
            Inputs::Boolean2d {
                a,
                b,
                operation: rng.below(5) as i32,
            }
        }
        9 | 10 => {
            let (a, b) = two(rng);
            Inputs::PiecewiseMerge {
                a,
                b,
                create_new: rng.chance(0.5),
            }
        }
        11..=13 => {
            let (brep, selection) = gen_swept(rng, s);
            Inputs::Sweep {
                brep,
                dir: gen_dir(rng),
                dist: s * rng.uniform(0.2, 1.5) * if rng.chance(0.15) { -1.0 } else { 1.0 },
                stitch: rng.chance(0.7),
                merge: rng.chance(0.3),
                region: gen_region(rng),
                selection,
                attributed: None,
            }
        }
        _ => {
            let (brep, selection) = gen_swept(rng, s);
            let mut axis = gen_dir(rng);
            if axis == [0.0; 3] {
                axis = [0.0, 0.0, 1.0];
            }
            // the axis off the operand, so the swept body does not cross it
            let base = [-3.0 * s, -3.0 * s, rng.uniform(-s, s)];
            let angle_deg = match rng.below(5) {
                0 => 90.0,
                1 => 180.0,
                2 => -45.0,
                _ => rng.uniform(5.0, 300.0),
            };
            Inputs::RotationalSweep {
                brep,
                base,
                axis,
                angle_deg,
                stitch: rng.chance(0.7),
                merge: rng.chance(0.3),
                region: gen_region(rng),
                selection,
                attributed: None,
            }
        }
    }
}

/// One generated `tessellate_boundaries` call: any operand kind the generator makes, at
/// any scale, sometimes with edges left without curve, with a tolerance from `0.5` to
/// `90` degrees or an invalid one.
fn gen_tessellate(rng: &mut Rng) -> Inputs {
    let s = gen_scale(rng);
    let c = rng.point3(-2.0 * s, 2.0 * s);
    let brep = match rng.below(5) {
        0 | 1 => gen_brep(rng, s, c),
        2 => BrepIn::Plane {
            origin: c,
            size: [s * rng.uniform(0.3, 1.5), s * rng.uniform(0.3, 1.5)],
        },
        3 => {
            let major = s * rng.uniform(0.5, 1.5);
            BrepIn::Torus {
                center: c,
                major,
                minor: major * rng.uniform(0.1, 0.8),
            }
        }
        _ => BrepIn::Boolean {
            operation: 1,
            a: Box::new(gen_brep(rng, s, c)),
            b: Box::new(gen_brep(rng, s, c)),
        },
    };
    let angle_tol_deg = match rng.below(12) {
        0 => *rng.choose(&[0.0, -5.0, -1.0e-9]).unwrap_or(&0.0),
        1 => 90.0,
        _ => rng.uniform(0.5, 30.0),
    };
    let drop_curves: Vec<u64> = if rng.chance(0.2) {
        (0..=rng.below(3)).map(|_| rng.below(4)).collect()
    } else {
        Vec::new()
    };
    // only edges the operand has: a torus or a sphere has fewer than four, and an index
    // past `GetEdges` is a harness error on both sides, not an API answer (the draws
    // above are kept, so the rest of the stream is unchanged)
    let drop_curves = if drop_curves.is_empty() {
        drop_curves
    } else {
        let mut k = Kernel::default();
        let num_edges = build(&mut k, &brep)
            .ok()
            .and_then(|b| brep_p1::edges(&b, None).ok())
            .map_or(0, |edges| edges.len() as u64);
        drop_curves.into_iter().filter(|&d| d < num_edges).collect()
    };
    Inputs::TessellateBoundaries {
        brep,
        angle_tol_deg,
        drop_curves,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "boolean.api_brep"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_inputs(rng)).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let gen_ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        let mut v = default_cases(self, &gen_ctx)?;
        // the later entry points under their own names and seeds, so that each `gen` case
        // stays the one the baseline has
        let more_ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED_MORE),
            ..ctx.clone()
        };
        for mut c in more_ctx.generated_cases("boolean.api_brep:more", |rng| {
            serde_json::to_value(gen_more(rng)).ok()
        }) {
            c.unit = self.id().to_string();
            c.name = c.name.replacen("gen", "more", 1);
            v.push(c);
        }
        let tess_ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED_TESSELLATE),
            ..ctx.clone()
        };
        for mut c in tess_ctx.generated_cases("boolean.api_brep:tessellate", |rng| {
            serde_json::to_value(gen_tessellate(rng)).ok()
        }) {
            c.unit = self.id().to_string();
            c.name = c.name.replacen("gen", "tess", 1);
            v.push(c);
        }
        Ok(v)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}
