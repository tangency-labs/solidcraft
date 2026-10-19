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
