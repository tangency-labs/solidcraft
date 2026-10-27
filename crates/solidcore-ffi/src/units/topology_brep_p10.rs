//! FFI wrappers for unit `topology.brep_p10`.
//!
//! Shim: `harness/cpp/shim/topology_brep_p10.cpp`.
//! Upstream: `source/SMLib/src/SmBrep.cpp` (`SmBrep::MakeTopologyFromData`,
//! `TestClosureSplitRegion` (edge version), `RemoveAllTopology`, `ProjectAndTrim`).
//!
//! | Rust    | shim                         | upstream                                 |
//! |---------|------------------------------|------------------------------------------|
//! | [`run`] | `sc_topology_brep_p10_run`   | read B-rep data, patch, build, one op    |
//!
//! # Request
//!
//! `SmBrepData::ReadFromFile(path)` (ASCII, or binary with [`Request::binary`]), then
//! the [`Patch`]es in order, then `MakeTopologyFromData` on a new `SmBrep` with the six
//! options (`bMakeUVTrimCurves`, `bAddAnalytics`, `bForceSameTol`,
//! `bCheckUVTrimCurves`, `bAlwaysHeal`, `bHealerIsEnabled`), then the [`Op`].
//!
//! # Output
//!
//! A stream of [`Item`]s (the shim emits `(tag, value)` int64 pairs: tag 0 an integer,
//! tag 1 the bits of a double):
//!
//! * `-2` alone when the guard (below) keeps the data from upstream; else
//! * the `MakeTopologyFromData` status (`0` = success); on success the data's manifold
//!   flag and borrowed flag afterwards and the B-rep graph; then for
//!   * [`Op::Closure`]: per edge in graph order `status, new shells, new regions` of
//!     `TestClosureSplitRegion(flag, edge)`, then the graph again;
//!   * [`Op::RemoveAll`]: after setting `m_bEditingEnabled = flag`, the
//!     `RemoveAllTopology` status and the graph;
//!   * [`Op::Project`]: the `CreateLineSegment` status, and on success the
//!     `ProjectAndTrim` status and the graph.
//!
//! The graph is the B-rep tolerance (d), editing flag, the ten entity counts (regions,
//! shells, face uses, faces, loop uses, loops, edge uses, edges, vertex uses,
//! vertices) in traversal order from the B-rep's regions (each object numbered when
//! first seen; `-1` null, `-3` not reached), then per entity:
//!
//! * region: is infinite, on the B-rep's list, void flag, topology, shell list;
//! * shell: region, type code, topology, face use list (face use shells) or the list
//!   head as a one-element list;
//! * face use: shell, face, mate, orientation, topology, loop use list;
//! * face: surface, primary face use, UV domain (4 d), rectangular-trim flag,
//!   tolerance (d), topology;
//! * loop use: face use, loop, mate, type code, start, orientation, topology;
//! * loop: loop use, topology;
//! * edge use: edge, type code, loop use / shell, CW, CCW, vertex use, orientation,
//!   radial next and last, UV curve, topology;
//! * edge: tolerance (d), interval (2 d), edge use list, curve, topology;
//! * vertex use: vertex, type code, owner, topology;
//! * vertex: point (3 d), tolerance (d), vertex use list, topology.
//!
//! Topology is `flags, user index 1, user index 2, user pointer set`. A curve is `0`
//! (none) or `1, type, dim, natural interval (2 d)`, five evaluations at 0, 1/4, 1/2,
//! 3/4 and 1 of the interval (`status` and on success `x y z` (d)) and whether the
//! holding entity owns it; a surface likewise with its natural domain (4 d) and a
//! 3 x 3 grid of evaluations. Lists are a count followed by the items.
//!
//! # Guard
//!
//! Upstream indexes the record arrays unchecked and dereferences the null object of a
//! record it never built. The shim simulates `MakeTopologyFromData`'s record indexing
//! in order (stopping where upstream returns an error status first) and returns `-2`
//! without calling upstream for data on which upstream would index past an array,
//! dereference such a null object, index the (empty) attribute table, or build lists
//! the Rust arena model cannot hold (a face use on two shells, an edge use start on a
//! vertex loop use, a rewritten radial list that does not close at its head, crosses
//! edges, leaves out an edge use or has inconsistent back links). The Rust port returns
//! `InvalidInput` (or `Unported` for attributes) for the same data.

use crate::{check, FfiError, SC_ERR_BUFFER};

/// Native projection would extrude a raw curve above `NL_MAXDEG` into a surface
/// whose evaluation overruns NLib's fixed scratch arrays. Classification with an
/// empty face subset uses the separate SMLib evaluator and remains valid below 32.
pub const STATUS_UNDEFINED_PROJECTION_DEGREE: i32 = -1101;

mod sys {
    extern "C" {
        pub fn sc_topology_brep_p10_run(
            path: *const core::ffi::c_char,
            ints: *const i64,
            n_ints: i64,
            dbls: *const f64,
            n_dbls: i64,
            out: *mut i64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// The operation after `MakeTopologyFromData`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    /// Nothing more.
    Make,
    /// `TestClosureSplitRegion(do_infinite, edge)` on every edge.
    Closure {
        /// `bDoInfiniteRegionTesting`.
        do_infinite: bool,
    },
    /// `RemoveAllTopology` with `m_bEditingEnabled = editing`.
    RemoveAll {
        /// `m_bEditingEnabled`.
        editing: bool,
    },
    /// `ProjectAndTrim` of the line segment `a`-`b` along `dir` with reference
    /// point `reference`.
    Project {
        /// `SmTrimType` (0 keep point, 1 delete point, 2 split).
        trim: i64,
        /// Segment start.
        a: [f64; 3],
        /// Segment end.
        b: [f64; 3],
        /// Projection direction.
        dir: [f64; 3],
        /// Reference point.
        reference: [f64; 3],
        /// `cpdOpt3DApproximationTol`.
        approx_tol: Option<f64>,
        /// `cpdOptAngleTolRadians`.
        angle_tol: Option<f64>,
    },
}

/// A change to the B-rep data before the build.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Patch {
    /// `m_sZoneTol3d = v`, stored raw (as a read stores it, without `fabs`).
    ZoneTol(f64),
    /// `m_bManifold = v`.
    Manifold(i64),
    /// `SetHealerVersion(v)`.
    Healer(i64),
    /// `m_bIsGeomBorrowed = v`.
    Borrowed(bool),
    /// Every record's flags and user indices from a base value: record `i` of kind
    /// `k` (regions 0, shells 1, face uses 2, faces 3, loops 4, edge uses 5, edges 6,
    /// vertices 7) gets flags `base + 7 i + k`, user index 1 `3 base + i`, user
    /// index 2 `11 i + k`; edge use record `i` also mate flags `base + 5 i`, mate user
    /// indices `i + 1` and `2 i`.
    Stamp(u64),
    /// Keep only region 1 (with start shell 0) and its shells; manifold `TRUE`. A no-op
    /// unless there are two regions and region 1's shells exist.
    Iges,
    /// Scale every vertex point.
    Scale(f64),
    /// Every `n`-th edge use record (from 0) loses its UV curve (`999999`).
    DropUv(i64),
    /// Set one index field: `field` (0 face surface, 1 face start loop, 2 loop start
    /// edge use, 3 loop vertex, 4 edge use edge, 5 edge use loop, 6 edge use next, 7
    /// edge curve, 8 edge primary edge use, 9 shell type, 10 region start shell, 11
    /// edge use UV curve, 12 edge use type, 13 loop edge use count, 14 face use face,
    /// 15 edge start vertex) of record `index` to `value` (a no-op for an index past
    /// the array).
    Set {
        /// Field code.
        field: i64,
        /// Record index.
        index: i64,
        /// New value.
        value: i64,
    },
}

/// Exact 3D NURB passed by reference to `ProjectAndTrim`, without knot tuning.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectionCurve {
    /// Polynomial degree.
    pub degree: usize,
    /// Homogeneous control points.
    pub control_points: Vec<[f64; 4]>,
    /// Raw knot vector.
    pub knots: Vec<f64>,
}

/// One request to [`run`].
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    /// Path of the B-rep data file.
    pub path: String,
    /// `SM_BINARY` instead of `SM_ASCII`.
    pub binary: bool,
    /// The six `MakeTopologyFromData` flags in argument order.
    pub options: [bool; 6],
    /// Patches, in order.
    pub patches: Vec<Patch>,
    /// The operation.
    pub op: Op,
    /// Optional exact NURB instead of the operation's line segment.
    pub projection_curve: Option<ProjectionCurve>,
    /// Pass an empty face subset to projection, while retaining whole-B-rep classification.
    pub empty_projection_subset: bool,
}

/// One output value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Item {
    /// An integer.
    Int(i64),
    /// A double.
    Dbl(f64),
}

fn bad(m: impl Into<String>) -> FfiError {
    FfiError::InvalidArgument(m.into())
}

fn encode(req: &Request) -> Result<(Vec<i64>, Vec<f64>), FfiError> {
    let mut ints = Vec::new();
    let mut dbls = Vec::new();
    let op = match req.op {
        Op::Make => 0,
        Op::Closure { .. } => 1,
        Op::RemoveAll { .. } => 2,
        Op::Project { .. } => 3,
    };
    ints.push(op);
    ints.push(i64::from(req.binary));
    ints.extend(req.options.iter().map(|&b| i64::from(b)));
    ints.push(i64::try_from(req.patches.len()).map_err(|_| bad("too many patches"))?);
    for p in &req.patches {
        match *p {
            Patch::ZoneTol(v) => {
                ints.push(0);
                dbls.push(v);
            }
            Patch::Manifold(v) => ints.extend([1, v]),
            Patch::Healer(v) => {
                if v < 0 {
                    return Err(bad("negative healer version"));
                }
                ints.extend([2, v]);
            }
            Patch::Borrowed(v) => ints.extend([3, i64::from(v)]),
            Patch::Stamp(v) => ints.extend([4, v as i64]),
            Patch::Iges => ints.push(5),
            Patch::Scale(v) => {
                ints.push(6);
                dbls.push(v);
            }
            Patch::DropUv(n) => {
                if n < 1 {
                    return Err(bad("drop-uv step below 1"));
                }
                ints.extend([7, n]);
            }
            Patch::Set {
                field,
                index,
                value,
            } => {
                if !(0..=15).contains(&field) || index < 0 {
                    return Err(bad("bad set patch"));
                }
                ints.extend([8, field, index, value]);
            }
        }
    }
    match req.op {
        Op::Make => {}
        Op::Closure { do_infinite } => ints.push(i64::from(do_infinite)),
        Op::RemoveAll { editing } => ints.push(i64::from(editing)),
        Op::Project {
            trim,
            a,
            b,
            dir,
            reference,
            approx_tol,
            angle_tol,
        } => {
            if !(0..=2).contains(&trim) {
                return Err(bad("trim type out of range"));
            }
            ints.extend([
                trim,
                i64::from(approx_tol.is_some()),
                i64::from(angle_tol.is_some()),
            ]);
            dbls.extend(a);
            dbls.extend(b);
            dbls.extend(dir);
            dbls.extend(reference);
            dbls.push(approx_tol.unwrap_or(0.0));
            dbls.push(angle_tol.unwrap_or(0.0));
            ints.extend([
                i64::from(req.empty_projection_subset),
                i64::from(req.projection_curve.is_some()),
            ]);
            if let Some(c) = &req.projection_curve {
                let n = c.control_points.len();
                // sm_BasisDerivs and sm_CrvSplit index through p in 32-wide scratch arrays.
                if !(2..=1024).contains(&n) || c.degree == 0 || c.degree >= 32 || c.degree >= n
                    || c.knots.len() != n + c.degree + 1
                    || c.knots.iter().any(|x| !x.is_finite())
                    || c.knots.windows(2).any(|w| w.first() > w.last())
                    || c.control_points.iter().flatten().any(|x| !x.is_finite())
                {
                    return Err(bad("invalid raw projection NURB"));
                }
                // The new raw path must not expose the native cache's sub-EFF_ZERO
                // span UB (UB-266). The regression gaps remain strictly above this.
                for pair in c.knots.windows(2) {
                    if let [a, b] = pair {
                        if b > a && b - a < 1e-12 {
                            return Err(bad("raw projection span below native EFF_ZERO"));
                        }
                    }
                }
                let p = c.degree;
                let m = c.knots.len() - 1;
                let a = *c.knots.first().ok_or_else(|| bad("empty knots"))?;
                let b = *c.knots.last().ok_or_else(|| bad("empty knots"))?;
                let head = c.knots.get(..=p).ok_or_else(|| bad("short knots"))?;
                let tail = c.knots.get(m - p..).ok_or_else(|| bad("short knots"))?;
                let inner = c.knots.get(p + 1..m - p).ok_or_else(|| bad("short knots"))?;
                if a >= b || head.iter().any(|&k| k != a) || tail.iter().any(|&k| k != b)
                    || inner.iter().any(|&k| k == a || k == b)
                {
                    return Err(bad("raw projection knots not clamped"));
                }
                let mut run = 1;
                for pair in inner.windows(2) {
                    run = if pair.first() == pair.last() { run + 1 } else { 1 };
                    if run > p {
                        return Err(bad("raw projection multiplicity above degree"));
                    }
                }
                ints.extend([(n - 1) as i64, c.degree as i64, (c.knots.len() - 1) as i64]);
                dbls.extend(c.control_points.iter().flatten().copied());
                dbls.extend_from_slice(&c.knots);
            }
        }
    }
    Ok((ints, dbls))
}

/// Runs one request (see the module docs for the layouts). An op other than
/// [`Op::Make`] is refused after a [`Patch::Set`]: upstream walks a B-rep built from
/// malformed indices without checks.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed request, a path with a NUL byte, a
/// file upstream cannot read or a patch it cannot apply; the shim's status otherwise.
pub fn run(req: &Request) -> Result<Vec<Item>, FfiError> {
    if req.op != Op::Make && req.patches.iter().any(|p| matches!(p, Patch::Set { .. })) {
        return Err(bad("ops after the build need unmodified indices"));
    }
    let (ints, dbls) = encode(req)?;
    let path = std::ffi::CString::new(req.path.as_bytes()).map_err(|_| bad("NUL in path"))?;
    let n_ints = i64::try_from(ints.len()).map_err(|_| bad("request too long"))?;
    let n_dbls = i64::try_from(dbls.len()).map_err(|_| bad("request too long"))?;
    let mut cap = 1usize << 16;
    for _ in 0..2 {
        let mut buf = vec![0i64; cap];
        let mut len: i64 = 0;
        // SAFETY: every pointer is valid for the length passed with it; the shim
        // writes at most `cap` items to `buf` and one value to `len`.
        let st = unsafe {
            sys::sc_topology_brep_p10_run(
                path.as_ptr(),
                ints.as_ptr(),
                n_ints,
                dbls.as_ptr(),
                n_dbls,
                buf.as_mut_ptr(),
                cap as i64,
                &raw mut len,
            )
        };
        let got = usize::try_from(len).map_err(|_| bad("shim returned a negative length"))?;
        if st == SC_ERR_BUFFER && got > cap {
            cap = got;
            continue;
        }
        check(st)?;
        if got > cap || got % 2 != 0 {
            return Err(bad("shim returned a bad length"));
        }
        buf.truncate(got);
        return buf
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| match p {
                [0, v] => Ok(Item::Int(*v)),
                [1, v] => Ok(Item::Dbl(f64::from_bits(*v as u64))),
                _ => Err(bad("shim returned a bad tag")),
            })
            .collect();
    }
    Err(FfiError::Status(SC_ERR_BUFFER))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projection_request(degree: usize) -> Request {
        let count = degree + 1;
        Request {
            path: String::new(),
            binary: false,
            options: [false; 6],
            patches: Vec::new(),
            op: Op::Project {
                trim: 0,
                a: [0.0; 3],
                b: [1.0, 0.0, 0.0],
                dir: [0.0, 0.0, 1.0],
                reference: [0.0, -1.0, 0.0],
                approx_tol: None,
                angle_tol: None,
            },
            projection_curve: Some(ProjectionCurve {
                degree,
                control_points: (0..count)
                    .map(|i| [i as f64, 0.0, 0.0, 1.0])
                    .collect(),
                knots: std::iter::repeat_n(0.0, count)
                    .chain(std::iter::repeat_n(1.0, count))
                    .collect(),
            }),
            empty_projection_subset: true,
        }
    }

    #[test]
    fn raw_projection_degree_stays_below_native_scratch_width() {
        // Check admission only: degree 32 must never reach the native evaluator.
        for (degree, accepted) in [(31, true), (32, false)] {
            assert_eq!(
                encode(&projection_request(degree)).is_ok(),
                accepted,
                "degree {degree}"
            );
        }
    }

    #[test]
    fn raw_projection_degree_guard_checks_the_actual_projection_faces() {
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/topology.brep_p10");
        for (file, empty_subset, allowed) in [
            ("raw_vote_rectangle.smb", false, false),
            ("raw_vote_rectangle.smb", true, true),
            ("empty_brep.smb", false, true),
        ] {
            let mut req = projection_request(18);
            req.path = fixtures.join(file).to_string_lossy().into_owned();
            req.empty_projection_subset = empty_subset;
            let result = run(&req);
            if allowed {
                let items = result.expect("classification-only degree 18 is defined");
                assert_eq!(items.first(), Some(&Item::Int(0)), "fixture must build");
            } else {
                assert!(matches!(
                    result,
                    Err(FfiError::Status(STATUS_UNDEFINED_PROJECTION_DEGREE))
                ));
            }
        }
    }
}
