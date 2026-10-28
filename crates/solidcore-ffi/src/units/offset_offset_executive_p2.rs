//! Native operation fixtures for `offset.offset_executive_p2` (draft PR #201).
//!
//! Upstream (pinned `c9c5979`): `source/SMLib/inc/SmOffsetExecutive.h`,
//! `source/SMLib/src/SmOffsetExecutive.cpp`, `source/SM_API/src/SmApiBrep.cpp`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`Operation::StaticOffset`] | `SmOffsetExecutive::OffsetBrep` |
//! | [`Operation::StaticShell`] | ownership-reporting `SmOffsetExecutive::ShellBrep` |
//! | [`Operation::Solid`] | `SmOffsetExecutive::DoSolidOffset` |
//! | [`Operation::Extended`] | `SmOffsetExecutive::DoExtendedOffset` |
//! | [`Operation::ApiShell`] | `SmApiShellBrepFull` |
//! | [`Operation::ApiOffset`] | `SmApiOffsetBrepFull` |
//!
//! Deviations: bounded analytic fixture DTOs, caller-owned flat C buffers and
//! traversal IDs instead of pointers. No private native algorithm is reproduced.
//! All six modes call the named native function, including ordinary error paths.
//! These DTOs use only `std`; serialization and Rust-kernel deferrals belong to
//! the runner. Nontrivial native results do not establish Rust geometry parity.
//!
//! # Contracts and ownership
//!
//! Static offset/shell dereference their input before a null check: `None` is an
//! argument error for those two modes only. Member null inputs reach native NER
//! (`1006`); both API null inputs reach native validation (`1007`). Signed public
//! distances, including negative offsets and both zeros, are preserved. Only
//! member modes require a nonnegative internal geometry-constructor magnitude;
//! direction is independently supplied by `inset`. Extension checks the product
//! of the relevant magnitude by five for finiteness, except on proven native
//! early returns: null member/API inputs and API shell foreign/null selections.
//! Those paths never use that product; every DTO scalar must still be finite,
//! every index is still checked, and negative internal member magnitudes remain
//! invalid. All other non-null fixtures retain the conservative fivefold guard,
//! including empty/wire B-reps. `Solid` forwards
//! `skip_stitch_merge` unchanged; native extend-convex dispatch happens first and
//! therefore ignores that flag. `Extended` does not take that flag at all.
//!
//! Foreign selectors refer to a second real B-rep, not invented pointers. A
//! foreign face with index zero is NOT source face zero. Duplicates, nulls and
//! selector order are preserved. The API, not the FFI guard, rejects foreign/null
//! selections. Static/member selectors also reach native `SetShellFaces` without
//! an invented membership check. Indices are checked both before calling C++ and
//! against the actual native traversals after construction.
//!
//! The shim uses the upstream API context, retaining its normal defaults and
//! lifetime. The shared native-call mutex excludes all other shims from context
//! acquisition through destruction and callback/Boolean restoration, including
//! exception unwinding and every buffer-retry invocation. Each call restores its
//! previous doing-Boolean flag. A scoped native
//! construction/destruction observer owns distinct B-rep allocations, including
//! unreturned working results on early native errors. Executive-owned temporary
//! B-reps are observed while the executive lives, not independently deleted.
//! Static shell uses the explicit transfer report; a consumed source is never
//! dereferenced. Although static offset's comment says "consumed", its actual
//! solid-offset paths retain the source; lifetime observations verify this rather
//! than abandoning the allocation. No shim state survives a call.
//!
//! # C protocol, version 1
//!
//! `sc_offset_offset_executive_p2_eval(input, input_len, out, cap, out_len)`
//! returns transport `SC_OK = 0` separately from unmodified native status codes.
//! Input consists entirely of finite doubles, in this order:
//!
//! ```text
//! operation, tolerance,
//! offset_distance, member_distance, member_approx_tol, member_operation,
//! inset, extend_convex, merge_results, create_offset_solid, skip_stitch_merge,
//! source_fixture, foreign_fixture, selection_count, (origin, index)...
//! ```
//!
//! Fixture tags: `0` null; `1` empty; `2` wire followed by start xyz, end xyz,
//! domain min/max, trim min/max; `3` plane sheet followed by origin xyz, X axis
//! xyz, Y axis xyz, scale xy, domain `umin,vmin,umax,vmax`; `4` box followed by
//! origin xyz and size xyz. Selection origins are `0` source, `1` foreign,
//! `2` null (with index `-1`). There are no caller-supplied knots or weights.
//!
//! Output is `[-1, status]` ONLY for a fixture-construction rejection. A box
//! constructor can report success after ignoring face-creation failures; actual
//! face/edge/vertex counts other than 6/12/8 are rejected with `SM_ERR` (`1001`).
//! Sheet construction may succeed after merging/dropping boundary topology; its
//! face/loop/edge/edgeuse/vertex counts must be 1/1/4/8/4, and wire counts must be
//! 0/0/1/2/2. A difference is likewise `SM_ERR`. These post-construction checks
//! apply to source and foreign fixtures before selectors/operations, with no
//! size/tolerance cutoff. Unprojected shell/region counts are not restricted.
//! The observer reclaims rejected B-reps after pending geometry guards unwind;
//! no independent delete is added on rejection. Malformed inputs and adapter
//! exceptions remain transport errors, not rejections.
//! Built fixtures always produce:
//!
//! ```text
//! 0, version=1, operation, status, input_transferred, source_lifetime,
//! result_origin, doing_boolean_before, doing_boolean_after, has_executive,
//! [member_operation, inset, extend_convex, merge_results, create_offset_solid,
//!  distance, approx_tol, tangency_degrees] if has_executive,
//! selection_count, selected_before(origin,index)..., selected_after(origin,index)...,
//! source_before, source_after, foreign_before, foreign_after, result, executive_offset
//! ```
//!
//! Source lifetime: `0` null, `1` retained, `2` returned as result, `3` consumed.
//! Result origin: `0` null, `1` new allocation, `2` source, `3` executive temporary.
//! Selector observations: `0` source, `1` foreign, `2` null, `3` consumed source,
//! `4` non-null face absent from both current traversals. A consumed source keeps
//! its PRE-operation index; absent/null indices are `-1`. No address is emitted.
//!
//! Each optional B-rep is `0`, or `1, integer_count, integers..., value_count,
//! values...`. Integers and floats are separated by the safe decoder into
//! [`BrepSnapshot`]; counted lists below are `count, traversal IDs...`. IDs are
//! zero-based in native traversal order, `-1` null, `-2` not in that traversal.
//! B-rep integer layout:
//!
//! * `region_count,shell_count,face_count,loop_count,edge_count,edgeuse_count,
//!   vertex_count,infinite_region_id,editing_enabled,validate_only`;
//! * per region: `id,is_void`;
//! * per shell: `id,region_id`;
//! * per face: `id,surface_kind,up_shell,mate_shell,up_orientation,mate_orientation`,
//!   counted loop IDs, then three `(point_status,normal_status)` pairs;
//! * per loop: `id,face_id`, counted edgeuse IDs;
//! * per edge: `id,start_vertex,end_vertex,curve_kind`, counted edgeuse IDs,
//!   `start_evaluation_status,end_evaluation_status`;
//! * per edgeuse: `id,edge_id,face_id,shell_id,mate_id,radial_id,orientation`;
//! * per vertex: `id`.
//!
//! Surface kinds: `0` null, `1` plane, `2` B-spline, `3` other. Curve kinds:
//! `0` null, `1` line, `2` B-spline, `3` other. No manifold query is added to
//! snapshots: it would run extra native algorithms before API early validation.
//! Floating layout: B-rep tolerance; per face: tolerance, face domain[4], natural
//! surface domain[4], three `(xyz[3],normal[3])` samples at domain min, midpoint,
//! max (UV is `(1-f)*min + f*max`, `f=0,0.5,1`; normal side flags are both true);
//! per edge: tolerance, trim interval[2], natural interval[2], evaluated start/end
//! xyz[6]; per vertex: xyz[3], tolerance. Evaluation outputs start at zero; native
//! outputs/statuses are preserved, including partial outputs on errors. Status
//! `-1` means sampling was not attempted (null geometry/nonfinite parameter).
//! Samples are observations, not
//! an exact serialization of arbitrary returned geometry. In particular they
//! must not be advertised as complete offset/shell equivalence.

use crate::{call_with_buffer, FfiError};

/// Adapter limit on selected faces, including duplicate and null selections.
pub const MAX_SELECTIONS: usize = 1024;
/// Aggregate input budget (geometry is fixed-size; selections are bounded).
pub const MAX_PACKED_VALUES: usize = 4096;
/// Adapter limit on a [`Fixture::File`] path, in bytes.
pub const MAX_FILE_PATH: usize = 1024;
/// Selection index bound of a [`Fixture::File`] source, whose face count is only
/// known once the shim has read it (a selection past its faces is then rejected
/// as a fixture-construction error).
pub const MAX_FILE_FACES: usize = 1024;
/// Maximum native snapshot size, bounding buffer retries and decoding allocation.
pub const MAX_OUTPUT_VALUES: usize = 262_144;
/// Unmodified `SM_SUCCESS`, not the transport status `SC_OK`.
pub const NATIVE_SUCCESS: i32 = 1000;
/// Unmodified native member NER error for a null original B-rep.
pub const NATIVE_NULL_POINTER: i32 = 1006;
/// Unmodified native API validation error.
pub const NATIVE_INVALID_INPUT: i32 = 1007;

/// One of the six actual native functions owned by this unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Operation {
    /// Static signed-distance offset.
    StaticOffset = 0,
    /// Static signed-distance shell with explicit input ownership reporting.
    StaticShell = 1,
    /// Direct member offset, including its native extend-convex dispatch.
    Solid = 2,
    /// Direct member extended offset.
    Extended = 3,
    /// Pure API shell, including native selection validation before copying.
    ApiShell = 4,
    /// API signed-distance offset.
    ApiOffset = 5,
}

impl Operation {
    fn is_member(self) -> bool {
        matches!(self, Self::Solid | Self::Extended)
    }
}

/// A small real native B-rep; `None` in [`Request`] means a null pointer.
#[derive(Debug, Clone, PartialEq)]
pub enum Fixture {
    /// A real `SmBrep`, including its infinite void region.
    EmptyBrep,
    /// One real wire edge backed by `SmLine::CreateLineSegment`.
    Wire {
        /// Lower natural-interval endpoint.
        start: [f64; 3],
        /// Upper natural-interval endpoint; coincidence reaches native construction.
        end: [f64; 3],
        /// Finite, strictly increasing natural curve interval.
        domain: [f64; 2],
        /// Ordered subinterval of `domain`; zero width is allowed.
        interval: [f64; 2],
    },
    /// A trimmed sheet face backed by a real analytic `SmPlane`.
    PlaneSheet {
        /// Parametric origin.
        origin: [f64; 3],
        /// Native constructor X axis.
        x_axis: [f64; 3],
        /// Native constructor Y axis.
        y_axis: [f64; 3],
        /// Native parameter scales.
        scale: [f64; 2],
        /// `umin,vmin,umax,vmax`, with positive finite extents.
        domain: [f64; 4],
    },
    /// Six-face box built by the actual `SmApiCreateBox` constructor.
    Box {
        /// Lower corner.
        origin: [f64; 3],
        /// Length, width, height; nonpositive dimensions reach native rejection.
        size: [f64; 3],
    },
    /// Three-face cylinder built by the actual `SmApiCreateCylinder` constructor
    /// (`SmBrep*&` overload): the side face is closed along a seam edge.
    Cylinder {
        /// Centre of the base circle.
        origin: [f64; 3],
        /// Radius; degenerate radii reach native rejection.
        radius: f64,
        /// Height along Z.
        height: f64,
    },
    /// One-face sphere built by the actual `SmApiCreateSphere` constructor.
    Sphere {
        /// Centre.
        origin: [f64; 3],
        /// Radius; degenerate radii reach native rejection.
        radius: f64,
    },
    /// An ASCII `.smb` file read by `SmBrep::ReadFromFile(ctx, path, SM_ASCII)`; the
    /// file's own tolerance is kept. A read failure is a construction rejection.
    File {
        /// Path of the file: non-empty, at most [`MAX_FILE_PATH`] bytes, no NUL.
        path: String,
    },
}

impl Fixture {
    fn face_count(&self) -> usize {
        match self {
            Self::EmptyBrep | Self::Wire { .. } => 0,
            Self::PlaneSheet { .. } | Self::Sphere { .. } => 1,
            Self::Box { .. } => 6,
            Self::Cylinder { .. } => 3,
            Self::File { .. } => MAX_FILE_FACES,
        }
    }
}

/// Origin-qualified face selection; duplicates and ordering are significant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaceSelection {
    /// Index in the source B-rep's actual face traversal.
    Source(usize),
    /// Index in an independently constructed foreign B-rep's face traversal.
    Foreign(usize),
    /// An actual native null face pointer (valid ABI input).
    Null,
}

/// Per-operation scalars and independent native flags.
// These are independent native setters, not states of one state machine.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// Raw SIGNED public distance, used by static/API operations, never pre-abs'd.
    pub offset_distance: f64,
    /// Direct-member geometry-constructor NONNEGATIVE magnitude, NOT signed distance.
    /// Both signed zeros are allowed; unused by static/API operations.
    pub member_distance: f64,
    /// Raw finite direct-member geometry-constructor approximation tolerance.
    /// Includes zero, negative values and the native default sentinel.
    pub member_approx_tol: f64,
    /// Native member operation type: 0 solid, 1 NMT, 2 shell, 3 local, 4 machine.
    pub member_operation: u32,
    /// Direct-member `SetInset`; independent of either distance scalar's sign.
    pub inset: bool,
    /// Static/API extended-offset argument; direct-member `SetExtendConvexEdges`.
    pub extend_convex: bool,
    /// Static/API self-intersection argument; direct-member `SetMergeResults`.
    pub merge_results: bool,
    /// Shell argument / direct-member `SetCreateOffsetSolid`.
    pub create_offset_solid: bool,
    /// `DoSolidOffset`'s real skip argument; ignored by all other entrypoints.
    pub skip_stitch_merge: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            offset_distance: 0.1,
            member_distance: 0.1,
            member_approx_tol: 1e-6,
            member_operation: 0,
            inset: false,
            extend_convex: false,
            merge_results: false,
            create_offset_solid: false,
            skip_stitch_merge: false,
        }
    }
}

/// One independent, fully owned native fixture invocation.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    /// Actual native entrypoint to call.
    pub operation: Operation,
    /// Source B-rep, or a real null input.
    pub source: Option<Fixture>,
    /// Optional independent B-rep supplying real foreign face pointers.
    pub foreign: Option<Fixture>,
    /// Source/foreign/null face pointers in caller order, without deduplication.
    pub faces: Vec<FaceSelection>,
    /// Finite value passed to native `SetTolerance(value, FALSE)` on both fixtures.
    /// Nonpositive values request native automatic tolerance calculation.
    pub tolerance: f64,
    /// Operation-specific settings; unused scalars remain finite but are not applied.
    pub config: Config,
}

/// Cheap observed topology and sampled numerical geometry, in native traversal order.
#[derive(Debug, Clone, PartialEq)]
pub struct BrepSnapshot {
    /// Integer counts, IDs, ordering, adjacency, flags and evaluation statuses.
    /// The exact layout is documented in the module header.
    pub integers: Vec<i64>,
    /// Actual tolerances, domains, intervals, points and sampled normals.
    /// The exact layout is documented in the module header.
    pub values: Vec<f64>,
}

/// Final inherited executive state, observed before its destructor runs.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq)]
pub struct ExecutiveState {
    /// Actual native operation type after the call.
    pub operation: i32,
    /// Actual native inset flag.
    pub inset: bool,
    /// Actual native extend-convex flag.
    pub extend_convex: bool,
    /// Actual protected merge-results flag, observed by a fixture subclass.
    pub merge_results: bool,
    /// Actual protected create-offset-solid flag.
    pub create_offset_solid: bool,
    /// Actual geometry creator's stored distance.
    pub distance: f64,
    /// Actual geometry creator's stored approximation tolerance.
    pub approx_tol: f64,
    /// Actual inherited tangency-tolerance getter, in degrees.
    pub tangency_degrees: f64,
}

/// All observations of a built case, even when the operation returned an error.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    /// Unmodified native operation status (`1000` is success).
    pub status: i32,
    /// Static shell's actual ownership report; false for other modes.
    pub input_transferred: bool,
    /// 0 null, 1 retained, 2 returned as result, 3 consumed (see module protocol).
    pub source_lifetime: i32,
    /// 0 null, 1 new, 2 source, 3 executive temporary.
    pub result_origin: i32,
    /// Native context doing-Boolean flag before and immediately after the operation.
    pub doing_boolean: [bool; 2],
    /// Present only for direct-member modes.
    pub executive: Option<ExecutiveState>,
    /// Actual pre-operation selector identities `(origin, traversal_index)`.
    pub selected_before: Vec<[i64; 2]>,
    /// Actual post-operation identities; consumed-source origin is 3, never dereferenced.
    pub selected_after: Vec<[i64; 2]>,
    /// Actual original source, before native execution.
    pub source_before: Option<BrepSnapshot>,
    /// Same source allocation afterward, or `None` if null/consumed.
    pub source_after: Option<BrepSnapshot>,
    /// Independent foreign fixture, before native execution.
    pub foreign_before: Option<BrepSnapshot>,
    /// Same foreign fixture afterward, including API early failures.
    pub foreign_after: Option<BrepSnapshot>,
    /// Native result topology and sampled numerical geometry, when provided.
    pub result: Option<BrepSnapshot>,
    /// Executive-owned intermediate, sampled before destruction; never deleted twice.
    pub executive_offset: Option<BrepSnapshot>,
}

/// Native fixture rejection is distinct from an ordinary operation failure.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Native fixture construction failed before execution, including incomplete boxes.
    Rejected {
        /// Constructor status, or `SM_ERR` (1001) for incomplete box topology.
        /// Never an FFI argument/exception status.
        status: i32,
    },
    /// Fixture built; includes actual before/after data even for native errors.
    Built(Box<Snapshot>),
}

mod sys {
    extern "C" {
        pub fn sc_offset_offset_executive_p2_eval(
            input: *const f64,
            input_len: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

fn invalid(message: &str) -> FfiError {
    FfiError::InvalidArgument(message.into())
}

fn finite(values: &[f64]) -> Result<(), FfiError> {
    if values.iter().all(|v| v.is_finite()) {
        Ok(())
    } else {
        Err(invalid("nonfinite fixture scalar"))
    }
}

fn interval(range: [f64; 2], strict: bool) -> Result<(), FfiError> {
    finite(&range)?;
    let [a, b] = range;
    if a > b || (strict && a == b) || !(b - a).is_finite() {
        return Err(invalid("invalid finite parameter interval"));
    }
    Ok(())
}

fn pack_fixture(fixture: Option<&Fixture>, out: &mut Vec<f64>) -> Result<(), FfiError> {
    let Some(fixture) = fixture else {
        out.push(0.0);
        return Ok(());
    };
    match fixture {
        Fixture::EmptyBrep => out.push(1.0),
        Fixture::Wire {
            start,
            end,
            domain,
            interval: trim,
        } => {
            finite(start)?;
            finite(end)?;
            interval(*domain, true)?;
            interval(*trim, false)?;
            let [d0, d1] = *domain;
            let [t0, t1] = *trim;
            if t0 < d0 || t1 > d1 {
                return Err(invalid("wire trim outside curve domain"));
            }
            // Native line creation subtracts endpoints and computes their norm.
            let mut squared = 0.0;
            for (a, b) in start.iter().zip(end) {
                squared += (b - a) * (b - a);
            }
            finite(&[squared])?;
            out.push(2.0);
            out.extend_from_slice(start);
            out.extend_from_slice(end);
            out.extend_from_slice(domain);
            out.extend_from_slice(trim);
        }
        Fixture::PlaneSheet {
            origin,
            x_axis,
            y_axis,
            scale,
            domain,
        } => {
            finite(origin)?;
            finite(x_axis)?;
            finite(y_axis)?;
            finite(scale)?;
            let [u0, v0, u1, v1] = *domain;
            interval([u0, u1], true)?;
            interval([v0, v1], true)?;
            // Unitization and the bilinear backing net must not overflow.
            for axis in [x_axis, y_axis] {
                finite(&[axis.iter().map(|v| v * v).sum()])?;
            }
            let [su, sv] = *scale;
            finite(&[su * u0, su * u1, sv * v0, sv * v1])?;
            out.push(3.0);
            out.extend_from_slice(origin);
            out.extend_from_slice(x_axis);
            out.extend_from_slice(y_axis);
            out.extend_from_slice(scale);
            out.extend_from_slice(domain);
        }
        Fixture::Box { origin, size } => {
            finite(origin)?;
            finite(size)?;
            for (a, b) in origin.iter().zip(size) {
                finite(&[a + b, b * b])?;
            }
            out.push(4.0);
            out.extend_from_slice(origin);
            out.extend_from_slice(size);
        }
        Fixture::Cylinder {
            origin,
            radius,
            height,
        } => {
            primitive_finite(origin, *radius)?;
            finite(&[*height, origin[2] + height])?;
            out.push(5.0);
            out.extend_from_slice(origin);
            out.extend_from_slice(&[*radius, *height]);
        }
        Fixture::Sphere { origin, radius } => {
            primitive_finite(origin, *radius)?;
            out.push(6.0);
            out.extend_from_slice(origin);
            out.push(*radius);
        }
        Fixture::File { path } => {
            // kind 7, the byte count, then one value per byte (the protocol is f64s)
            let bytes = path.as_bytes();
            if bytes.is_empty() || bytes.len() > MAX_FILE_PATH || bytes.contains(&0) {
                return Err(invalid("file fixture path empty, too long or with NUL"));
            }
            out.push(7.0);
            out.push(bytes.len() as f64);
            out.extend(bytes.iter().map(|&b| f64::from(b)));
        }
    }
    Ok(())
}

/// The shim's finiteness checks of a cylinder or sphere fixture.
fn primitive_finite(origin: &[f64; 3], radius: f64) -> Result<(), FfiError> {
    finite(origin)?;
    finite(&[radius, radius * radius])?;
    for a in origin {
        finite(&[a + radius, a - radius])?;
    }
    Ok(())
}

fn pack(request: &Request) -> Result<Vec<f64>, FfiError> {
    let cfg = &request.config;
    finite(&[
        request.tolerance,
        cfg.offset_distance,
        cfg.member_distance,
        cfg.member_approx_tol,
    ])?;
    if cfg.member_operation > 4 {
        return Err(invalid("member operation outside native enum"));
    }
    if request.source.is_none()
        && matches!(
            request.operation,
            Operation::StaticOffset | Operation::StaticShell
        )
    {
        return Err(invalid(
            "static offset/shell dereferences null before validation",
        ));
    }
    if request.operation.is_member() && cfg.member_distance < 0.0 {
        return Err(invalid(
            "member constructor requires a nonnegative internal magnitude",
        ));
    }
    if request.faces.len() > MAX_SELECTIONS {
        return Err(invalid("too many selected faces"));
    }
    let mut out = vec![
        f64::from(request.operation as i32),
        request.tolerance,
        cfg.offset_distance,
        cfg.member_distance,
        cfg.member_approx_tol,
        f64::from(cfg.member_operation),
        f64::from(cfg.inset),
        f64::from(cfg.extend_convex),
        f64::from(cfg.merge_results),
        f64::from(cfg.create_offset_solid),
        f64::from(cfg.skip_stitch_merge),
    ];
    pack_fixture(request.source.as_ref(), &mut out)?;
    pack_fixture(request.foreign.as_ref(), &mut out)?;
    out.push(request.faces.len() as f64);
    for selection in &request.faces {
        let (origin, index, count) = match *selection {
            FaceSelection::Source(index) => (
                0.0,
                index,
                request.source.as_ref().map_or(0, Fixture::face_count),
            ),
            FaceSelection::Foreign(index) => (
                1.0,
                index,
                request.foreign.as_ref().map_or(0, Fixture::face_count),
            ),
            FaceSelection::Null => {
                out.extend_from_slice(&[2.0, -1.0]);
                continue;
            }
        };
        if index >= count {
            return Err(invalid("face index outside its origin fixture"));
        }
        out.extend_from_slice(&[origin, index as f64]);
    }
    if out.len() > MAX_PACKED_VALUES {
        return Err(invalid("aggregate fixture budget exceeded"));
    }
    // Match native validation order without synthesizing an operation status:
    // member NER/API null checks precede arithmetic, and API shell rejects any
    // foreign/null selector before copying. Classify by origin, not by an index
    // that may coincide between source and foreign traversals. All DTO checks
    // above still apply; only an irrelevant fivefold product is exempted.
    let returns_before_extension = request.source.is_none()
        || (request.operation == Operation::ApiShell
            && request
                .faces
                .iter()
                .any(|selection| !matches!(selection, FaceSelection::Source(_))));
    if !returns_before_extension && (request.operation == Operation::Extended || cfg.extend_convex)
    {
        let magnitude = if request.operation.is_member() {
            cfg.member_distance
        } else {
            cfg.offset_distance.abs()
        };
        finite(&[magnitude * 5.0])?;
    }
    Ok(out)
}

struct Reader<'a> {
    remaining: &'a [f64],
}

impl Reader<'_> {
    fn number(&mut self) -> Result<f64, FfiError> {
        let Some((first, rest)) = self.remaining.split_first() else {
            return Err(invalid("truncated native snapshot"));
        };
        self.remaining = rest;
        Ok(*first)
    }

    fn integer(&mut self, min: i64, max: i64) -> Result<i64, FfiError> {
        let value = self.number()?;
        if !value.is_finite() || value.fract() != 0.0 || value < min as f64 || value > max as f64 {
            return Err(invalid("invalid integer in native snapshot"));
        }
        Ok(value as i64)
    }

    fn status(&mut self) -> Result<i32, FfiError> {
        self.integer(i64::from(i32::MIN), i64::from(i32::MAX))
            .map(|v| v as i32)
    }

    fn flag(&mut self) -> Result<bool, FfiError> {
        Ok(self.integer(0, 1)? != 0)
    }

    fn count(&mut self) -> Result<usize, FfiError> {
        self.integer(0, MAX_OUTPUT_VALUES as i64)
            .map(|v| v as usize)
    }

    fn selectors(&mut self, n: usize) -> Result<Vec<[i64; 2]>, FfiError> {
        if n > MAX_SELECTIONS || n > self.remaining.len() / 2 {
            return Err(invalid("invalid native selector count"));
        }
        (0..n)
            .map(|_| {
                Ok([
                    self.integer(0, 4)?,
                    self.integer(-1, MAX_OUTPUT_VALUES as i64)?,
                ])
            })
            .collect()
    }

    fn brep(&mut self) -> Result<Option<BrepSnapshot>, FfiError> {
        if !self.flag()? {
            return Ok(None);
        }
        let count = self.count()?;
        if count > self.remaining.len() {
            return Err(invalid("truncated native topology"));
        }
        let integers = (0..count)
            .map(|_| self.integer(i64::from(i32::MIN), MAX_OUTPUT_VALUES as i64))
            .collect::<Result<_, _>>()?;
        let count = self.count()?;
        let Some(values) = self.remaining.get(..count) else {
            return Err(invalid("truncated native geometry"));
        };
        let values = values.to_vec();
        self.remaining = self
            .remaining
            .get(count..)
            .ok_or_else(|| invalid("truncated native geometry"))?;
        Ok(Some(BrepSnapshot { integers, values }))
    }
}

fn decode(raw: &[f64], operation: Operation) -> Result<Outcome, FfiError> {
    if raw.len() > MAX_OUTPUT_VALUES {
        return Err(invalid("native snapshot exceeds output budget"));
    }
    let mut rd = Reader { remaining: raw };
    let tag = rd.integer(-1, 0)?;
    if tag == -1 {
        let status = rd.status()?;
        if !rd.remaining.is_empty() || status == NATIVE_SUCCESS {
            return Err(invalid("invalid native fixture rejection"));
        }
        return Ok(Outcome::Rejected { status });
    }
    if rd.integer(1, 1)? != 1 || rd.integer(0, 5)? != i64::from(operation as i32) {
        return Err(invalid("native snapshot version/operation mismatch"));
    }
    let status = rd.status()?;
    let input_transferred = rd.flag()?;
    let source_lifetime = rd.integer(0, 3)? as i32;
    let result_origin = rd.integer(0, 3)? as i32;
    let doing_boolean = [rd.flag()?, rd.flag()?];
    let executive = if rd.flag()? {
        Some(ExecutiveState {
            operation: rd.integer(0, 4)? as i32,
            inset: rd.flag()?,
            extend_convex: rd.flag()?,
            merge_results: rd.flag()?,
            create_offset_solid: rd.flag()?,
            distance: rd.number()?,
            approx_tol: rd.number()?,
            tangency_degrees: rd.number()?,
        })
    } else {
        None
    };
    let n = rd.count()?;
    let selected_before = rd.selectors(n)?;
    let selected_after = rd.selectors(n)?;
    let snapshot = Snapshot {
        status,
        input_transferred,
        source_lifetime,
        result_origin,
        doing_boolean,
        executive,
        selected_before,
        selected_after,
        source_before: rd.brep()?,
        source_after: rd.brep()?,
        foreign_before: rd.brep()?,
        foreign_after: rd.brep()?,
        result: rd.brep()?,
        executive_offset: rd.brep()?,
    };
    if !rd.remaining.is_empty() {
        return Err(invalid("trailing native snapshot values"));
    }
    Ok(Outcome::Built(Box::new(snapshot)))
}

/// Run one actual native operation and decode its observed, pointer-free snapshot.
///
/// Invalid/unsafe ABI inputs are `Err`, NOT `Outcome::Rejected`. Operation failures
/// are `Outcome::Built` with their native status and actual before/after geometry.
/// As with other `SM_API` shims, invoke serially: upstream's API context is global.
pub fn eval(request: &Request) -> Result<Outcome, FfiError> {
    let input = pack(request)?;
    let raw = call_with_buffer(4096, |out, len| {
        if out.len() > MAX_OUTPUT_VALUES {
            return crate::SC_ERR_ARG;
        }
        // SAFETY: input is fully validated, finite and bounded. Its storage and
        // both caller-owned outputs remain live throughout this synchronous call.
        unsafe {
            sys::sc_offset_offset_executive_p2_eval(
                input.as_ptr(),
                input.len() as i64,
                out.as_mut_ptr(),
                out.len() as i64,
                &raw mut *len,
            )
        }
    })?;
    decode(&raw, request.operation)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(operation: Operation) -> Request {
        Request {
            operation,
            source: Some(Fixture::EmptyBrep),
            foreign: None,
            faces: Vec::new(),
            tolerance: 1e-6,
            config: Config::default(),
        }
    }

    fn sheet() -> Fixture {
        Fixture::PlaneSheet {
            origin: [1.0, -2.0, 3.0],
            x_axis: [1.0, 0.0, 0.0],
            y_axis: [0.0, 1.0, 0.0],
            scale: [2.0, 3.0],
            domain: [-1.0, -2.0, 4.0, 5.0],
        }
    }

    #[cfg(feature = "cpp")]
    fn unit_sheet() -> Fixture {
        Fixture::PlaneSheet {
            origin: [3.0, 4.0, 5.0],
            x_axis: [1.0, 0.0, 0.0],
            y_axis: [0.0, 0.0, 1.0],
            scale: [1.0, 1.0],
            domain: [0.0, 0.0, 1.0, 1.0],
        }
    }

    #[cfg(feature = "cpp")]
    fn measured_counts(block: &BrepSnapshot) -> [i64; 5] {
        let counts = &block.integers;
        [counts[2], counts[3], counts[4], counts[5], counts[6]]
    }

    #[test]
    fn signed_public_offsets_and_internal_magnitudes_are_distinct() {
        for operation in [
            Operation::StaticOffset,
            Operation::StaticShell,
            Operation::ApiShell,
            Operation::ApiOffset,
        ] {
            for distance in [-0.1, -f64::from_bits(1), -0.0, 0.0, f64::MAX] {
                let mut r = request(operation);
                r.config.offset_distance = distance;
                // Unused member constructor values must not constrain a public call.
                r.config.member_distance = -1.0;
                let packed = pack(&r).unwrap();
                assert_eq!(packed[2].to_bits(), distance.to_bits());
            }
        }
        for operation in [Operation::Solid, Operation::Extended] {
            for magnitude in [-0.1, -f64::from_bits(1), -f64::MAX] {
                let mut r = request(operation);
                r.config.member_distance = magnitude;
                assert!(pack(&r).is_err());
            }
            for magnitude in [-0.0, 0.0, f64::from_bits(1), 1e200] {
                let mut r = request(operation);
                r.config.member_distance = magnitude;
                let packed = pack(&r).unwrap();
                assert_eq!(packed[3].to_bits(), magnitude.to_bits());
            }
        }
    }

    #[test]
    fn null_guard_matches_the_actual_entrypoints() {
        for operation in [
            Operation::StaticOffset,
            Operation::StaticShell,
            Operation::Solid,
            Operation::Extended,
            Operation::ApiShell,
            Operation::ApiOffset,
        ] {
            let mut r = request(operation);
            r.source = None;
            assert_eq!(
                pack(&r).is_ok(),
                !matches!(operation, Operation::StaticOffset | Operation::StaticShell)
            );
        }
    }

    #[test]
    fn extension_only_checks_the_relevant_fivefold_distance() {
        let mut r = request(Operation::StaticOffset);
        r.config.offset_distance = -f64::MAX;
        assert!(pack(&r).is_ok());
        r.config.extend_convex = true;
        assert!(pack(&r).is_err());
        r.operation = Operation::Extended;
        // Public distance is unused for a direct member.
        assert!(pack(&r).is_ok());
        r.config.member_distance = f64::MAX;
        assert!(pack(&r).is_err());
        r.operation = Operation::Solid;
        r.config.extend_convex = false;
        assert!(pack(&r).is_ok());
    }

    #[test]
    fn null_early_returns_allow_finite_distances_with_overflowing_fivefold_products() {
        for operation in [
            Operation::StaticOffset,
            Operation::StaticShell,
            Operation::Solid,
            Operation::Extended,
            Operation::ApiShell,
            Operation::ApiOffset,
        ] {
            for distance in [-f64::MAX, -1e308, 1e308, f64::MAX] {
                for extend in [false, true] {
                    let mut r = request(operation);
                    r.source = None;
                    r.config.offset_distance = distance;
                    r.config.member_distance = distance.abs();
                    r.config.extend_convex = extend;
                    let packed = pack(&r);
                    if matches!(operation, Operation::StaticOffset | Operation::StaticShell) {
                        assert!(packed.is_err());
                    } else {
                        let packed = packed.unwrap();
                        assert_eq!(packed[2].to_bits(), distance.to_bits());
                        assert_eq!(packed[3].to_bits(), distance.abs().to_bits());
                    }
                }
            }
        }
        for operation in [Operation::Solid, Operation::Extended] {
            let mut r = request(operation);
            r.source = None;
            r.config.member_distance = -1.0;
            // The internal constructor contract remains unconditional.
            assert!(pack(&r).is_err());
        }
    }

    #[test]
    fn api_selection_early_returns_skip_only_irrelevant_distance_arithmetic() {
        let mut r = request(Operation::ApiShell);
        r.source = Some(sheet());
        r.foreign = Some(sheet());
        r.config.extend_convex = true;
        for distance in [-f64::MAX, -1e308, 1e308, f64::MAX] {
            r.config.offset_distance = distance;
            for faces in [
                vec![FaceSelection::Null],
                vec![FaceSelection::Foreign(0)],
                vec![FaceSelection::Foreign(0), FaceSelection::Null],
                vec![FaceSelection::Null, FaceSelection::Foreign(0)],
                vec![
                    FaceSelection::Source(0),
                    FaceSelection::Source(0),
                    FaceSelection::Foreign(0),
                ],
                vec![
                    FaceSelection::Source(0),
                    FaceSelection::Source(0),
                    FaceSelection::Null,
                ],
            ] {
                r.faces = faces;
                let packed = pack(&r).unwrap();
                assert_eq!(packed[2].to_bits(), distance.to_bits());
            }
            r.faces = vec![FaceSelection::Source(0), FaceSelection::Source(0)];
            // Duplicates from the source pass API validation: the guard still applies.
            assert!(pack(&r).is_err());
        }
        // Known early API rejection does not exempt malformed DTO references,
        // even if the bad index follows the first foreign/null selection.
        for faces in [
            vec![FaceSelection::Foreign(0), FaceSelection::Source(1)],
            vec![FaceSelection::Null, FaceSelection::Foreign(1)],
            vec![FaceSelection::Foreign(usize::MAX), FaceSelection::Null],
        ] {
            r.faces = faces;
            assert!(pack(&r).is_err());
        }
        r.faces = vec![FaceSelection::Null];
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            for scalar in 0..4 {
                let mut invalid = r.clone();
                match scalar {
                    0 => invalid.tolerance = value,
                    1 => invalid.config.offset_distance = value,
                    2 => invalid.config.member_distance = value,
                    _ => invalid.config.member_approx_tol = value,
                }
                assert!(pack(&invalid).is_err());
                invalid.source = None;
                // Null-source early validation also preserves all finite-scalar checks.
                assert!(pack(&invalid).is_err());
            }
        }
        r.faces = vec![FaceSelection::Foreign(0)];
        r.foreign = None;
        assert!(pack(&r).is_err());
        r.faces = vec![FaceSelection::Source(0), FaceSelection::Null];
        r.source = None;
        assert!(pack(&r).is_err());
    }

    #[test]
    fn other_nonnull_fixtures_keep_the_conservative_extension_overflow_guard() {
        for fixture in [
            Fixture::EmptyBrep,
            Fixture::Wire {
                start: [0.0; 3],
                end: [1.0, 0.0, 0.0],
                domain: [0.0, 1.0],
                interval: [0.0, 1.0],
            },
            sheet(),
            Fixture::Box {
                origin: [0.0; 3],
                size: [2.0, 3.0, 4.0],
            },
        ] {
            for operation in [
                Operation::StaticOffset,
                Operation::StaticShell,
                Operation::Solid,
                Operation::Extended,
                Operation::ApiShell,
                Operation::ApiOffset,
            ] {
                let mut r = request(operation);
                r.source = Some(fixture.clone());
                r.config.extend_convex = true;
                r.config.offset_distance = 1e308;
                r.config.member_distance = 1e308;
                assert!(pack(&r).is_err());
            }
        }
    }

    #[test]
    fn origin_qualified_selectors_preserve_duplicates_and_null_order() {
        let mut r = request(Operation::ApiShell);
        r.source = Some(sheet());
        r.foreign = Some(sheet());
        for faces in [
            vec![FaceSelection::Source(0), FaceSelection::Source(0)],
            vec![FaceSelection::Foreign(0), FaceSelection::Null],
            vec![FaceSelection::Null, FaceSelection::Foreign(0)],
            vec![
                FaceSelection::Source(0),
                FaceSelection::Foreign(0),
                FaceSelection::Null,
            ],
        ] {
            r.faces = faces;
            assert!(pack(&r).is_ok());
        }
        r.faces = vec![FaceSelection::Source(1)];
        assert!(pack(&r).is_err());
        r.faces = vec![FaceSelection::Foreign(usize::MAX)];
        assert!(pack(&r).is_err());
        r.foreign = None;
        r.faces = vec![FaceSelection::Foreign(0)];
        assert!(pack(&r).is_err());
        r.source = Some(Fixture::EmptyBrep);
        r.faces = vec![FaceSelection::Source(0)];
        assert!(pack(&r).is_err());
    }

    #[test]
    fn all_native_flag_combinations_and_enum_values_are_packed() {
        let mut r = request(Operation::Solid);
        for bits in 0..32 {
            r.config.inset = bits & 1 != 0;
            r.config.extend_convex = bits & 2 != 0;
            r.config.merge_results = bits & 4 != 0;
            r.config.create_offset_solid = bits & 8 != 0;
            r.config.skip_stitch_merge = bits & 16 != 0;
            for operation in 0..5 {
                r.config.member_operation = operation;
                let packed = pack(&r).unwrap();
                assert_eq!(packed[5], f64::from(operation));
                for bit in 0..5 {
                    assert_eq!(packed[6 + bit], f64::from(bits & (1 << bit) != 0));
                }
            }
        }
        r.config.member_operation = 5;
        assert!(pack(&r).is_err());
    }

    #[test]
    fn finite_and_aggregate_guards_run_before_native_allocation() {
        let mut r = request(Operation::Solid);
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            r.config.member_approx_tol = value;
            assert!(pack(&r).is_err());
        }
        r.config.member_approx_tol = -1.0;
        r.faces = vec![FaceSelection::Null; MAX_SELECTIONS];
        assert!(pack(&r).is_ok());
        r.faces.push(FaceSelection::Null);
        assert!(pack(&r).is_err());
        r.faces.clear();
        r.source = Some(Fixture::Wire {
            start: [0.0; 3],
            end: [1.0, 0.0, 0.0],
            domain: [0.0, 1.0],
            interval: [0.0, 1.1],
        });
        assert!(pack(&r).is_err());
        r.source = Some(Fixture::Box {
            origin: [0.0; 3],
            size: [1.0, -1.0, 0.0],
        });
        // Actual box construction, not the FFI, owns dimension rejection.
        assert!(pack(&r).is_ok());
    }

    #[test]
    fn decoder_does_not_conflate_transport_success_and_native_status() {
        assert_eq!(
            decode(&[-1.0, 1007.0], Operation::ApiShell).unwrap(),
            Outcome::Rejected { status: 1007 }
        );
        assert!(decode(&[-1.0, 1000.0], Operation::ApiShell).is_err());
        assert!(decode(&[0.0, 1.0, 4.0], Operation::ApiShell).is_err());
        assert!(decode(&[f64::NAN], Operation::ApiShell).is_err());
    }

    #[cfg(feature = "cpp")]
    #[test]
    fn native_null_statuses_are_not_synthesized() {
        for (operation, status) in [
            (Operation::Solid, NATIVE_NULL_POINTER),
            (Operation::Extended, NATIVE_NULL_POINTER),
            (Operation::ApiShell, NATIVE_INVALID_INPUT),
            (Operation::ApiOffset, NATIVE_INVALID_INPUT),
        ] {
            for distance in [0.1, -1e308, 1e308, f64::MAX] {
                for extend in [false, true] {
                    let mut r = request(operation);
                    r.source = None;
                    r.config.offset_distance = distance;
                    r.config.member_distance = distance.abs();
                    r.config.extend_convex = extend;
                    let Outcome::Built(snapshot) = eval(&r).unwrap() else {
                        panic!("null input is not a fixture rejection")
                    };
                    assert_eq!(snapshot.status, status);
                    assert_eq!(snapshot.source_lifetime, 0);
                    assert_eq!(snapshot.doing_boolean[0], snapshot.doing_boolean[1]);
                    assert!(snapshot.source_before.is_none());
                    assert!(snapshot.source_after.is_none());
                    assert!(snapshot.result.is_none());
                    assert!(snapshot.executive_offset.is_none());
                    if operation.is_member() {
                        let executive = snapshot.executive.as_ref().unwrap();
                        assert_eq!(executive.distance.to_bits(), distance.abs().to_bits());
                        assert_eq!(executive.extend_convex, extend);
                    }
                }
            }
        }
    }

    #[cfg(feature = "cpp")]
    #[test]
    fn native_api_invalid_selection_preserves_real_geometry_before_tolerance_check() {
        let mut r = request(Operation::ApiShell);
        r.source = Some(sheet());
        r.foreign = Some(sheet());
        // Invalid selection must win over both shell tolerance and irrelevant
        // extension overflow. The two fixtures deliberately share face index 0.
        for (distance, extend) in [
            (0.0, false),
            (0.0, true),
            (1e308, true),
            (-1e308, true),
            (f64::MAX, true),
        ] {
            r.config.offset_distance = distance;
            r.config.extend_convex = extend;
            for faces in [
                vec![FaceSelection::Null],
                vec![FaceSelection::Foreign(0)],
                vec![FaceSelection::Foreign(0), FaceSelection::Null],
                vec![FaceSelection::Null, FaceSelection::Foreign(0)],
                vec![
                    FaceSelection::Source(0),
                    FaceSelection::Source(0),
                    FaceSelection::Foreign(0),
                ],
                vec![
                    FaceSelection::Source(0),
                    FaceSelection::Source(0),
                    FaceSelection::Null,
                ],
            ] {
                r.faces = faces;
                let Outcome::Built(snapshot) = eval(&r).unwrap() else {
                    panic!("sheet construction rejected")
                };
                assert_eq!(snapshot.status, NATIVE_INVALID_INPUT);
                assert_eq!(snapshot.source_before, snapshot.source_after);
                assert_eq!(snapshot.foreign_before, snapshot.foreign_after);
                let expected: Vec<[i64; 2]> = r
                    .faces
                    .iter()
                    .map(|selection| match selection {
                        FaceSelection::Source(i) => [0, *i as i64],
                        FaceSelection::Foreign(i) => [1, *i as i64],
                        FaceSelection::Null => [2, -1],
                    })
                    .collect();
                assert_eq!(snapshot.selected_before, expected);
                assert_eq!(snapshot.selected_before, snapshot.selected_after);
                assert!(!snapshot.source_before.as_ref().unwrap().values.is_empty());
                assert_eq!(snapshot.source_lifetime, 1);
                assert!(snapshot.result.is_none());
            }
        }
    }

    #[cfg(feature = "cpp")]
    #[test]
    fn native_empty_source_boxes_reject_only_face_selection() {
        for (origin, size) in [([0.0; 3], [1e-6; 3]), ([1e6; 3], [1e-3; 3])] {
            for selection in [FaceSelection::Null, FaceSelection::Source(0)] {
                let mut r = request(Operation::ApiShell);
                r.source = Some(Fixture::Box { origin, size });
                r.faces = vec![selection];
                r.tolerance = 1e-9;
                r.config.offset_distance = 0.5;
                // The tiny box is an empty B-rep: a null selection still reaches the
                // API's own 1007 validation; face 0 does not exist, so nothing runs.
                if selection == FaceSelection::Null {
                    let Outcome::Built(empty) = eval(&r).unwrap() else {
                        panic!("empty box with a null selection must reach API validation")
                    };
                    assert_eq!(empty.status, NATIVE_INVALID_INPUT);
                } else {
                    assert_eq!(eval(&r).unwrap(), Outcome::Rejected { status: 1001 });
                }

                // A valid call after each rejection also exercises observer/lock
                // restoration. Null selection is an operation error, not rejection.
                r.source = Some(Fixture::Box {
                    origin: [0.0; 3],
                    size: [2.0, 3.0, 4.0],
                });
                r.faces = vec![FaceSelection::Null];
                let Outcome::Built(snapshot) = eval(&r).unwrap() else {
                    panic!("valid box must reach API selection validation")
                };
                assert_eq!(snapshot.status, NATIVE_INVALID_INPUT);
                let counts = &snapshot.source_before.as_ref().unwrap().integers;
                assert_eq!([counts[2], counts[4], counts[6]], [6, 12, 8]);
                assert_eq!(snapshot.source_before, snapshot.source_after);
                assert_eq!(snapshot.selected_before, vec![[2, -1]]);
                assert_eq!(snapshot.selected_before, snapshot.selected_after);
                assert_eq!(snapshot.source_lifetime, 1);
                assert_eq!(snapshot.doing_boolean[0], snapshot.doing_boolean[1]);
                assert!(snapshot.result.is_none());
            }
        }
    }

    #[cfg(feature = "cpp")]
    #[test]
    fn native_empty_foreign_boxes_reject_only_face_selection() {
        for (origin, size) in [([0.0; 3], [1e-6; 3]), ([1e6; 3], [1e-3; 3])] {
            for selection in [FaceSelection::Null, FaceSelection::Foreign(0)] {
                let mut r = request(Operation::ApiShell);
                r.source = Some(Fixture::Box {
                    origin: [0.0; 3],
                    size: [2.0, 3.0, 4.0],
                });
                r.foreign = Some(Fixture::Box { origin, size });
                r.faces = vec![selection];
                r.tolerance = 1e-9;
                r.config.offset_distance = 0.5;
                // The tiny box is an empty B-rep: a null selection still reaches the
                // API's own 1007 validation; face 0 does not exist, so nothing runs.
                if selection == FaceSelection::Null {
                    let Outcome::Built(empty) = eval(&r).unwrap() else {
                        panic!("empty box with a null selection must reach API validation")
                    };
                    assert_eq!(empty.status, NATIVE_INVALID_INPUT);
                } else {
                    assert_eq!(eval(&r).unwrap(), Outcome::Rejected { status: 1001 });
                }

                // Foreign construction rejects after allocating the source. Both
                // must be reclaimed without poisoning the next valid invocation.
                r.foreign = r.source.clone();
                let Outcome::Built(snapshot) = eval(&r).unwrap() else {
                    panic!("valid foreign box must reach API selection validation")
                };
                assert_eq!(snapshot.status, NATIVE_INVALID_INPUT);
                let counts = &snapshot.foreign_before.as_ref().unwrap().integers;
                assert_eq!([counts[2], counts[4], counts[6]], [6, 12, 8]);
                assert_eq!(snapshot.source_before, snapshot.source_after);
                assert_eq!(snapshot.foreign_before, snapshot.foreign_after);
                assert_eq!(snapshot.selected_before, snapshot.selected_after);
                assert_eq!(snapshot.source_lifetime, 1);
                assert_eq!(snapshot.doing_boolean[0], snapshot.doing_boolean[1]);
                assert!(snapshot.result.is_none());
            }
        }
    }

    #[cfg(feature = "cpp")]
    #[test]
    fn native_incomplete_box_does_not_hide_malformed_adapter_input() {
        let mut r = request(Operation::ApiShell);
        r.source = Some(Fixture::Box {
            origin: [0.0; 3],
            size: [1e-6; 3],
        });
        r.faces = vec![FaceSelection::Source(0)];
        r.tolerance = 1e-9;
        r.config.offset_distance = 0.5;
        let base = pack(&r).unwrap();
        let mut invalid_selector = base.clone();
        *invalid_selector.last_mut().unwrap() = 6.0;
        let mut trailing_data = base;
        trailing_data.push(0.0);
        for input in [invalid_selector, trailing_data] {
            let mut out = [42.0; 2];
            let mut len = -1;
            // SAFETY: buffers are live and correctly sized; only the encoded
            // selector or trailing data deliberately violates the DTO contract.
            let status = unsafe {
                sys::sc_offset_offset_executive_p2_eval(
                    input.as_ptr(),
                    input.len() as i64,
                    out.as_mut_ptr(),
                    out.len() as i64,
                    &raw mut len,
                )
            };
            assert_eq!(status, crate::SC_ERR_ARG);
            assert_eq!(out, [42.0; 2]);
            assert_eq!(len, -1);
        }
    }

    #[cfg(feature = "cpp")]
    #[test]
    fn native_degenerate_sheets_are_built() {
        for tolerance in [0.9, 1e3] {
            for foreign in [false, true] {
                let indexed = if foreign {
                    FaceSelection::Foreign(0)
                } else {
                    FaceSelection::Source(0)
                };
                for selection in [FaceSelection::Null, indexed] {
                    let mut r = request(Operation::ApiShell);
                    if foreign {
                        r.foreign = Some(unit_sheet());
                    } else {
                        r.source = Some(unit_sheet());
                    }
                    r.faces = vec![selection];
                    r.tolerance = tolerance;
                    r.config.offset_distance = 10.0;
                    // Merged/dropped-face sheets are defined native results.
                    assert!(matches!(eval(&r).unwrap(), Outcome::Built(_)));

                    // A foreign rejection also cleans the already allocated source.
                    // The next call must retain real geometry and selector identity.
                    r.tolerance = 0.5;
                    r.faces = vec![FaceSelection::Null, indexed];
                    let Outcome::Built(snapshot) = eval(&r).unwrap() else {
                        panic!("representable sheet must reach API selection validation")
                    };
                    let block = if foreign {
                        snapshot.foreign_before.as_ref()
                    } else {
                        snapshot.source_before.as_ref()
                    };
                    assert_eq!(measured_counts(block.unwrap()), [1, 1, 4, 8, 4]);
                    assert_eq!(snapshot.status, NATIVE_INVALID_INPUT);
                    assert_eq!(snapshot.source_before, snapshot.source_after);
                    assert_eq!(snapshot.foreign_before, snapshot.foreign_after);
                    assert_eq!(
                        snapshot.selected_before,
                        vec![[2, -1], [i64::from(foreign), 0]]
                    );
                    assert_eq!(snapshot.selected_before, snapshot.selected_after);
                    assert_eq!(snapshot.source_lifetime, 1);
                    assert_eq!(snapshot.doing_boolean[0], snapshot.doing_boolean[1]);
                    assert!(!snapshot.input_transferred);
                    assert!(snapshot.result.is_none() && snapshot.executive_offset.is_none());
                }
            }
        }
    }

    #[cfg(feature = "cpp")]
    #[test]
    fn native_degenerate_sheet_reaches_member_null_guard_with_extreme_scalars() {
        let mut r = request(Operation::Extended);
        r.source = None;
        r.foreign = Some(unit_sheet());
        r.tolerance = 1e3;
        r.config.member_distance = 1e-300;
        r.config.member_approx_tol = 1e300;
        r.config.inset = true;
        r.config.extend_convex = true;
        r.config.create_offset_solid = true;
        assert!(matches!(eval(&r).unwrap(), Outcome::Built(_)));

        r.tolerance = 0.5;
        let Outcome::Built(snapshot) = eval(&r).unwrap() else {
            panic!("representable foreign sheet must reach the member null guard")
        };
        assert_eq!(snapshot.status, NATIVE_NULL_POINTER);
        assert_eq!(snapshot.source_lifetime, 0);
        assert!(snapshot.source_before.is_none() && snapshot.source_after.is_none());
        assert_eq!(snapshot.foreign_before, snapshot.foreign_after);
        assert_eq!(
            measured_counts(snapshot.foreign_before.as_ref().unwrap()),
            [1, 1, 4, 8, 4]
        );
        let executive = snapshot.executive.as_ref().unwrap();
        assert_eq!(executive.distance, 1e-300);
        assert_eq!(executive.approx_tol, 1e300);
        assert!(executive.inset && executive.extend_convex && executive.create_offset_solid);
        assert_eq!(snapshot.doing_boolean[0], snapshot.doing_boolean[1]);
        assert!(!snapshot.input_transferred);
        assert!(snapshot.result.is_none() && snapshot.executive_offset.is_none());
    }

    #[cfg(feature = "cpp")]
    #[test]
    fn native_stable_wires_do_not_require_matching_shell_counts_or_tolerance_cutoffs() {
        let wire = |end, interval| Fixture::Wire {
            start: [0.0; 3],
            end,
            domain: [0.0, 1.0],
            interval,
        };
        for (fixture, tolerance) in [
            (wire([1.0, 0.0, 0.0], [0.0, 1.0]), 1e3),
            (wire([1.0, 0.0, 0.0], [0.5, 0.5]), 1e-6),
            (wire([1e-9, 0.0, 0.0], [0.0, 1.0]), 1e3),
            (
                Fixture::Wire {
                    start: [-1e100, 2e100, -3e100],
                    end: [1e100, -2e100, 3e100],
                    domain: [-2.0, 3.0],
                    interval: [-1.5, 2.25],
                },
                1e-6,
            ),
        ] {
            for foreign in [false, true] {
                let mut r = request(Operation::ApiShell);
                if foreign {
                    r.foreign = Some(fixture.clone());
                } else {
                    r.source = Some(fixture.clone());
                }
                r.faces = vec![FaceSelection::Null];
                r.tolerance = tolerance;
                let Outcome::Built(snapshot) = eval(&r).unwrap() else {
                    panic!("stable wire must not be rejected for unprojected shell counts")
                };
                let block = if foreign {
                    snapshot.foreign_before.as_ref()
                } else {
                    snapshot.source_before.as_ref()
                };
                assert_eq!(measured_counts(block.unwrap()), [0, 0, 1, 2, 2]);
                assert_eq!(snapshot.status, NATIVE_INVALID_INPUT);
                assert_eq!(snapshot.source_before, snapshot.source_after);
                assert_eq!(snapshot.foreign_before, snapshot.foreign_after);
                assert_eq!(snapshot.selected_before, vec![[2, -1]]);
                assert_eq!(snapshot.selected_before, snapshot.selected_after);
                assert_eq!(snapshot.source_lifetime, 1);
                assert_eq!(snapshot.doing_boolean[0], snapshot.doing_boolean[1]);
                assert!(!snapshot.input_transferred);
                assert!(snapshot.result.is_none() && snapshot.executive_offset.is_none());
            }
        }
    }

    #[cfg(feature = "cpp")]
    #[test]
    fn native_incomplete_sheets_do_not_hide_malformed_adapter_input() {
        for foreign in [false, true] {
            let mut r = request(Operation::ApiShell);
            if foreign {
                r.foreign = Some(unit_sheet());
            } else {
                r.source = Some(unit_sheet());
            }
            r.faces = vec![if foreign {
                FaceSelection::Foreign(0)
            } else {
                FaceSelection::Source(0)
            }];
            r.tolerance = 0.9;
            let base = pack(&r).unwrap();
            let mut invalid_inputs = Vec::new();
            let mut invalid_selector = base.clone();
            *invalid_selector.last_mut().unwrap() = 1.0;
            invalid_inputs.push(invalid_selector);
            let mut trailing_data = base.clone();
            trailing_data.push(0.0);
            invalid_inputs.push(trailing_data);
            invalid_inputs.push(base[..base.len() - 1].to_vec());
            for (index, value) in [(1, f64::NAN), (4, f64::INFINITY), (7, 2.0)] {
                let mut input = base.clone();
                input[index] = value;
                invalid_inputs.push(input);
            }
            r.tolerance = 0.5;
            r.faces = vec![FaceSelection::Null];
            for input in invalid_inputs {
                let mut out = [42.0; 2];
                let mut len = -1;
                // SAFETY: buffers and lengths are valid; only encoded DTO contents
                // are deliberately malformed. Parsing must finish before allocation.
                let status = unsafe {
                    sys::sc_offset_offset_executive_p2_eval(
                        input.as_ptr(),
                        input.len() as i64,
                        out.as_mut_ptr(),
                        out.len() as i64,
                        &raw mut len,
                    )
                };
                assert_eq!(status, crate::SC_ERR_ARG);
                assert_eq!(out, [42.0; 2]);
                assert_eq!(len, -1);
                let Outcome::Built(snapshot) = eval(&r).unwrap() else {
                    panic!("malformed input must not poison the subsequent valid call")
                };
                assert_eq!(snapshot.status, NATIVE_INVALID_INPUT);
                assert_eq!(snapshot.source_before, snapshot.source_after);
                assert_eq!(snapshot.foreign_before, snapshot.foreign_after);
                assert_eq!(snapshot.doing_boolean[0], snapshot.doing_boolean[1]);
            }
        }
    }

    #[cfg(feature = "cpp")]
    #[test]
    fn native_observer_is_isolated_from_concurrent_p2_and_other_api_shims() {
        use crate::units::geom_object::{shape, ShapeSelection, ShapeSource};
        use std::sync::Barrier;

        let mut r = request(Operation::ApiShell);
        r.source = Some(Fixture::Box {
            origin: [0.0; 3],
            size: [2.0, 3.0, 4.0],
        });
        r.foreign = Some(sheet());
        // Exceed eval's initial output capacity: each retry must acquire the
        // same guard and restore the observer before releasing it.
        r.faces = vec![FaceSelection::Null; MAX_SELECTIONS];
        r.faces[0] = FaceSelection::Foreign(0);
        let expected = eval(&r).unwrap();
        let Outcome::Built(snapshot) = &expected else {
            panic!("box/sheet construction rejected")
        };
        assert_eq!(snapshot.status, NATIVE_INVALID_INPUT);
        assert_eq!(snapshot.source_before, snapshot.source_after);
        assert_eq!(snapshot.foreign_before, snapshot.foreign_after);

        // the shared fixture's SmApiCreateBox solid (kind 7: origin, size, then the
        // validate-only and init-gaps flags)
        let source = ShapeSource::Fixture {
            path: String::new(),
            block: vec![7.0, 0.0, 0.0, 0.0, 2.0, 3.0, 4.0, 0.0, 0.0],
        };
        let selection = ShapeSelection {
            whole_brep: true,
            ..ShapeSelection::default()
        };
        // This independently compiled wrapper also allocates and destroys real
        // API-context B-reps. A p2-only mutex would not protect its live objects.
        let other_expected = shape(&source, &selection).unwrap();
        let barrier = Barrier::new(6);
        std::thread::scope(|scope| {
            let mut workers = Vec::new();
            for worker in 0..6 {
                let (r, expected, source, selection, other_expected, barrier) = (
                    &r,
                    &expected,
                    &source,
                    &selection,
                    &other_expected,
                    &barrier,
                );
                workers.push(scope.spawn(move || {
                    barrier.wait();
                    for _ in 0..32 {
                        if worker % 2 == 0 {
                            assert_eq!(&eval(r).unwrap(), expected);
                        } else {
                            assert_eq!(&shape(source, selection).unwrap(), other_expected);
                        }
                    }
                }));
            }
            for worker in workers {
                worker.join().unwrap();
            }
        });
        assert_eq!(eval(&r).unwrap(), expected);
        assert_eq!(shape(&source, &selection).unwrap(), other_expected);
    }

    #[cfg(feature = "cpp")]
    #[test]
    fn native_early_return_exemptions_preserve_all_other_abi_guards() {
        let mut r = request(Operation::Solid);
        r.source = None;
        r.config.extend_convex = true;
        r.config.member_distance = 1e308;
        let base = pack(&r).unwrap();
        let mut invalid_inputs = Vec::new();
        for (index, value) in [
            (1, f64::NAN),
            (2, f64::INFINITY), // unused public scalar still must be finite
            (3, -1.0),          // negative internal magnitude remains invalid even on null
            (4, f64::INFINITY),
            (11, 1.0), // turn null into a real empty B-rep: fivefold guard applies again
        ] {
            let mut input = base.clone();
            input[index] = value;
            invalid_inputs.push(input);
        }
        // Public API null checking must not hide the static null dereference guard.
        for operation in [Operation::StaticOffset, Operation::StaticShell] {
            let mut input = base.clone();
            input[0] = f64::from(operation as i32);
            invalid_inputs.push(input);
        }
        for operation in [
            Operation::StaticOffset,
            Operation::StaticShell,
            Operation::Solid,
            Operation::Extended,
            Operation::ApiShell,
            Operation::ApiOffset,
        ] {
            let mut r = request(operation);
            r.config.extend_convex = true;
            let mut input = pack(&r).unwrap();
            input[if operation.is_member() { 3 } else { 2 }] = 1e308;
            invalid_inputs.push(input);
        }
        let mut r = request(Operation::ApiShell);
        r.source = Some(sheet());
        r.foreign = Some(sheet());
        r.config.offset_distance = 1e308;
        r.config.extend_convex = true;
        r.faces = vec![FaceSelection::Foreign(0), FaceSelection::Source(0)];
        let mut input = pack(&r).unwrap();
        // Malformed later selectors remain rejected despite the first foreign face.
        *input.last_mut().unwrap() = 1.0;
        invalid_inputs.push(input);
        for input in invalid_inputs {
            let mut out = [42.0; 2];
            let mut len = -1;
            // SAFETY: both buffers and len are live and correctly sized. Only
            // scalar/selector contents are deliberately outside the DTO contract.
            let status = unsafe {
                sys::sc_offset_offset_executive_p2_eval(
                    input.as_ptr(),
                    input.len() as i64,
                    out.as_mut_ptr(),
                    out.len() as i64,
                    &raw mut len,
                )
            };
            assert_eq!(status, crate::SC_ERR_ARG);
            assert_eq!(out, [42.0; 2]);
            assert_eq!(len, -1);
        }
    }
}
