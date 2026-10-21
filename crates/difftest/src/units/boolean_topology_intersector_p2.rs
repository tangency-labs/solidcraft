//! Runner for unit `boolean.topology_intersector_p2`. C++:
//! `solidcore_ffi::units::boolean_topology_intersector_p2` (shim
//! `harness/cpp/shim/boolean_topology_intersector_p2.cpp`). Rust:
//! `solidcore::boolean::topology_intersector::p2`.
//!
//! **Shared script.** Part 2's members run on the same `SmTopologyIntersector` over the
//! same two real B-reps as part 1's, so a case is a part 1 script
//! ([`super::boolean_topology_intersector_p1`]: B-rep inputs, handle selectors, record
//! layout, prefix cutting, guards) whose ops may include part 2's (`remove_rel`,
//! `squeeze`, `delete_faces`, `fix_gaps`, `intersect_topo`, `set_rels`, `iir`,
//! `fixup_class`, `fixup_split`, `edge_edge`, `edge_face_coin`; shim ops 41 to 51). The
//! C++ side runs through part 2's entry point, the Rust side through the same
//! interpreter as part 1's, which dispatches part 2's ops to
//! [`solidcore::boolean::topology_intersector::p2`]. `RegisterExistingSSI` is part 1's
//! script op 16, now run by the port on both sides.
//!
//! **Scripts.** Each generated script relates a few vertex pairs of the two B-reps
//! (so mates exist and conflict), then runs part 2's ops; classification pairs of the
//! fix-ups are built on one edge's curve with fractions of its interval and vertex
//! classifications naming the related vertices. Edge cases built in code cover the
//! branches the generator rarely draws.
//!
//! **Deferred.** What reaches `SmBrep::GetFaces` (`topology.brep_p1`: every
//! `IntersectTopology` and `IntersectInsertRelate`), `CombineCoincidentVertices` /
//! `MakeEdgeInFace` (`topology.brep_p5`), `DeleteFace` (`topology.brep_p3`),
//! `DeleteEdge` / `DeleteVertex` (`topology.brep_p4`), `MakeVertexSplitEdge`
//! (`topology.brep_p9`), `SmFace::GetEdges` / `GetVertices` / `GetBrep`
//! (`topology.face_p4`), `Point3DClassify` (`query.face_p1`) and the manager's
//! construction at a zero approximation tolerance (`topology.brep_p1`).

use serde_json::Value;

use solidcore_ffi::units::boolean_topology_intersector_p2 as ffi;

use super::boolean_topology_intersector_p1::{
    gen_brep, gen_h, gen_list, gen_sel, gen_tol, run_cpp_with, run_rust, BrepIn, CcIn, Gen, Hin,
    Inputs, IvlIn, OpIn, PciIn, K_EDGE, K_EDGE_CURVE, K_FACE, K_NULL, K_VERTEX, MIN_GENERATED,
};
use crate::case::{CaseContext, CaseSource};
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// The difftest runner.
pub struct Runner;

// ---- generation -----------------------------------------------------------------------------

/// A vertex selector of B-rep `b`.
fn vtx(rng: &mut Rng, b: i64) -> Hin {
    [b, i64::from(K_VERTEX), gen_sel(rng)]
}

/// An edge selector of B-rep `b`.
fn edge(rng: &mut Rng, b: i64) -> Hin {
    [b, i64::from(K_EDGE), gen_sel(rng)]
}

/// A point classification input: mostly the vertices of the related pairs (so the
/// split-pair branch is reached), else an edge, face or nothing.
fn gen_pci(rng: &mut Rng, g: &Gen, verts: &[Hin], t: f64) -> PciIn {
    let (cls, h) = match rng.below(6) {
        0..=2 if !verts.is_empty() => (104, *rng.choose(verts).unwrap_or(&[-1, K_NULL, -1])),
        3 => (103, gen_h(rng, g, &[K_EDGE], 0.1)),
        4 => (102, gen_h(rng, g, &[K_FACE], 0.1)),
        _ => (100, [-1, K_NULL, -1]),
    };
    PciIn {
        cls,
        h,
        t: rng.chance(0.85).then_some(t),
        dev: *rng.choose(&[0.0, 1e-6, 1e-4, 1e-2]).unwrap_or(&0.0),
    }
}

/// A classification of the curve of edge `e` with `n` intervals over fractions of the
/// edge's interval.
fn gen_cc(rng: &mut Rng, g: &Gen, e: Hin, n: usize, verts: &[Hin]) -> CcIn {
    let mut cuts: Vec<f64> = (0..n.saturating_sub(1))
        .map(|_| rng.uniform(0.0, 1.0))
        .collect();
    if rng.chance(0.3) {
        // a short interval
        let c = rng.uniform(0.0, 0.9);
        cuts = vec![c, c + rng.uniform(1e-9, 1e-3)];
    }
    cuts.sort_by(f64::total_cmp);
    let mut bounds = vec![0.0];
    bounds.extend(cuts);
    bounds.push(1.0);
    let ivls = bounds
        .windows(2)
        .map(|w| {
            let (a, b) = (
                w.first().copied().unwrap_or(0.0),
                w.last().copied().unwrap_or(1.0),
            );
            IvlIn {
                a,
                b,
                pcs: [
                    gen_pci(rng, g, verts, a),
                    gen_pci(rng, g, verts, 0.5 * (a + b)),
                    gen_pci(rng, g, verts, b),
                ],
            }
        })
        .collect();
    CcIn {
        c: [e[0], i64::from(K_EDGE_CURVE), e[2]],
        t0: 0.0,
        t1: 1.0,
        tol: gen_tol(rng),
        ivls,
        relative: true,
    }
}

/// One of part 2's ops; `pairs` are the related (B-rep, other) vertex pairs.
#[allow(clippy::too_many_lines)]
fn gen_p2_op(rng: &mut Rng, g: &Gen, pairs: &[(Hin, Hin)], has_mgr: &mut bool) -> Vec<OpIn> {
    let has_b = g.has_b;
    let ob = i64::from(has_b);
    let mut ops = Vec::new();
    let verts: Vec<Hin> = pairs.iter().flat_map(|&(a, b)| [a, b]).collect();
    let faces_a = g.kinds[0].contains(&K_FACE);
    match rng.below(13) {
        0 => {
            let (a, b) = match rng.choose(pairs) {
                Some(&p) if rng.chance(0.7) => p,
                _ => (vtx(rng, 0), vtx(rng, ob)),
            };
            ops.push(OpIn::RemoveRel { a, b });
        }
        1 | 2 if has_b => {
            let (bv, ov) = match rng.choose(pairs) {
                Some(&(a, b)) => match rng.below(3) {
                    0 => (a, b),
                    1 => (a, vtx(rng, 1)),
                    _ => (vtx(rng, 0), b),
                },
                None => (vtx(rng, 0), vtx(rng, 1)),
            };
            ops.push(OpIn::Squeeze { bv, ov });
        }
        3 => {
            let faces = if faces_a && rng.chance(0.3) {
                vec![[0, i64::from(K_FACE), gen_sel(rng)]]
            } else {
                Vec::new()
            };
            ops.push(OpIn::DeleteFaces {
                faces,
                keep: rng.chance(0.5).then(Vec::new),
            });
        }
        4 | 5 => {
            let mut vs: Vec<Hin> = pairs.iter().map(|&(a, _)| a).collect();
            // a vertex next to its coincident copy in the other B-rep (a gap candidate)
            if has_b && rng.chance(0.4) {
                let v = vtx(rng, 0);
                vs.push(v);
                vs.push([1, v[1], v[2]]);
            }
            for _ in 0..rng.below(3) {
                vs.push(vtx(rng, 0));
            }
            if rng.chance(0.3) {
                if let Some(&v) = vs.first() {
                    vs.push(v);
                }
            }
            ops.push(OpIn::FixGaps { verts: vs });
        }
        6 if has_b => {
            if !*has_mgr {
                ops.push(OpIn::MgrNew {
                    capture: gen_tol(rng),
                    override_tol: 0.0,
                });
                *has_mgr = true;
            }
            ops.push(if rng.chance(0.5) {
                OpIn::IntersectTopo
            } else {
                OpIn::SetRels
            });
        }
        7 if has_b => ops.push(OpIn::Iir),
        8 | 9 => {
            let e = edge(rng, 0);
            let n = 1 + rng.below(3) as usize;
            let b = gen_cc(rng, g, e, n, &verts);
            let o = if rng.chance(0.85) {
                let mut o = gen_cc(rng, g, e, n, &verts);
                // the other classification shares the intervals (homogenised), mostly
                if rng.chance(0.8) {
                    for (x, y) in o.ivls.iter_mut().zip(&b.ivls) {
                        x.a = y.a;
                        x.b = y.b;
                    }
                }
                o
            } else {
                let e2 = edge(rng, 0);
                let n2 = 1 + rng.below(3) as usize;
                gen_cc(rng, g, e2, n2, &verts)
            };
            if rng.chance(0.5) {
                ops.push(OpIn::FixupClass { b, o });
            } else {
                let max = b.ivls.len().min(o.ivls.len());
                let index = rng.below(max.max(1) as u64) as usize;
                ops.push(OpIn::FixupSplit { index, b, o });
            }
        }
        10 | 11 => {
            let other = if rng.chance(0.6) { ob } else { 0 };
            let edges1 = (0..=rng.below(4)).map(|_| edge(rng, 0)).collect();
            let mut edges2: Vec<Hin> = (0..=rng.below(4)).map(|_| edge(rng, other)).collect();
            if rng.chance(0.15) {
                edges2.push([-1, K_NULL, -1]);
            }
            ops.push(OpIn::EdgeEdge {
                edges1,
                edges2,
                manifold: rng.chance(0.5),
                ignore: rng.chance(0.5),
            });
        }
        _ => {
            let mut vs: Vec<Hin> = (0..2 + rng.below(4)).map(|_| vtx(rng, 0)).collect();
            if rng.chance(0.1) {
                vs.push([-1, K_NULL, -1]);
            }
            ops.push(OpIn::EdgeFaceCoin {
                verts: vs,
                excluded: gen_list(rng, g, &[K_EDGE], 0.1, 2),
                ignore: rng.chance(0.5),
                nesting: rng.chance(0.5),
            });
        }
    }
    if ops.is_empty() {
        ops.push(OpIn::RemoveRel {
            a: vtx(rng, 0),
            b: vtx(rng, ob),
        });
    }
    ops
}

fn generate(rng: &mut Rng) -> Option<Value> {
    let brep_a = gen_brep(rng);
    let has_b = rng.chance(0.85);
    let brep_b = if has_b {
        // sometimes the same B-rep twice (coincident vertices and edges)
        Some(if rng.chance(0.3) {
            brep_a.clone()
        } else {
            gen_brep(rng)
        })
    } else {
        None
    };
    let g = Gen::new(&brep_a, brep_b.as_ref());
    let ctor = u8::from(rng.chance(0.06));
    let approx = *rng
        .choose(&[1e-5, 1e-5, 1e-4, 1e-3, 1e-6, 1e-2])
        .unwrap_or(&1e-5);
    let ang = *rng
        .choose(&[20.0_f64.to_radians(), 0.1, 1e-3])
        .unwrap_or(&0.1);
    let ob = i64::from(has_b);
    let mut ops = Vec::new();
    // related vertex pairs (same selector on a copy of the B-rep: coincident vertices)
    let mut pairs = Vec::new();
    for _ in 0..rng.below(4) {
        let a = vtx(rng, 0);
        let b = if rng.chance(0.4) {
            [ob, a[1], a[2]]
        } else {
            vtx(rng, ob)
        };
        if ctor == 0 {
            ops.push(OpIn::Relate { a, b });
        }
        pairs.push((a, b));
    }
    let mut has_mgr = false;
    for _ in 0..=rng.below(3) {
        ops.extend(gen_p2_op(rng, &g, &pairs, &mut has_mgr));
    }
    serde_json::to_value(Inputs {
        brep_a,
        brep_b,
        ctor,
        approx,
        ang,
        ops,
    })
    .ok()
}

// ---- edge cases -----------------------------------------------------------------------------

fn file(name: &str) -> BrepIn {
    BrepIn::File {
        file: name.into(),
        rebuild: false,
    }
}

fn inputs(a: BrepIn, b: Option<BrepIn>, ctor: u8, ops: Vec<OpIn>) -> Option<Value> {
    serde_json::to_value(Inputs {
        brep_a: a,
        brep_b: b,
        ctor,
        approx: 1e-5,
        ang: 0.1,
        ops,
    })
    .ok()
}

fn pci(cls: i64, h: Hin, t: Option<f64>) -> PciIn {
    PciIn {
        cls,
        h,
        t,
        dev: 0.0,
    }
}

/// A classification of edge 0's curve of B-rep 0 over `[0, 1]` (relative) with the
/// given intervals.
fn cc(ivls: Vec<IvlIn>) -> CcIn {
    CcIn {
        c: [0, i64::from(K_EDGE_CURVE), 0],
        t0: 0.0,
        t1: 1.0,
        tol: 1e-5,
        ivls,
        relative: true,
    }
}

fn ivl(a: f64, b: f64, pcs: [PciIn; 3]) -> IvlIn {
    IvlIn { a, b, pcs }
}

#[allow(clippy::too_many_lines, clippy::vec_init_then_push)]
fn edge_cases() -> Vec<(String, Value)> {
    let v = |b: i64, i: i64| -> Hin { [b, i64::from(K_VERTEX), i] };
    let e = |b: i64, i: i64| -> Hin { [b, i64::from(K_EDGE), i] };
    let null: Hin = [-1, K_NULL, -1];
    let unknown = || pci(100, [-1, K_NULL, -1], None);
    let box_ = || file("box_with_large_fillet.smb");
    let mut out: Vec<(String, Option<Value>)> = Vec::new();

    // RemoveRelationship: mated, not mated, null, context-only intersector
    out.push((
        "remove_mated".into(),
        inputs(
            box_(),
            Some(box_()),
            0,
            vec![
                OpIn::Relate {
                    a: v(0, 0),
                    b: v(1, 0),
                },
                OpIn::RemoveRel {
                    a: v(0, 0),
                    b: v(1, 0),
                },
                OpIn::RemoveRel {
                    a: v(0, 0),
                    b: v(1, 0),
                },
                OpIn::OtherMate { h: v(0, 0) },
            ],
        ),
    ));
    out.push((
        "remove_null_and_unmated".into(),
        inputs(
            box_(),
            Some(box_()),
            0,
            vec![
                OpIn::RemoveRel {
                    a: null,
                    b: v(1, 0),
                },
                OpIn::RemoveRel {
                    a: v(0, 1),
                    b: null,
                },
                OpIn::Relate {
                    a: e(0, 0),
                    b: e(1, 0),
                },
                OpIn::RemoveRel {
                    a: e(0, 0),
                    b: e(1, 1),
                },
                OpIn::RemoveRel {
                    a: e(0, 0),
                    b: e(1, 0),
                },
            ],
        ),
    ));
    out.push((
        "remove_context_only".into(),
        inputs(
            box_(),
            Some(box_()),
            1,
            vec![OpIn::RemoveRel {
                a: v(0, 0),
                b: v(1, 0),
            }],
        ),
    ));

    // CheckForSqueezeVertices: no mates, same mates, a better previous relationship
    out.push((
        "squeeze_no_mates".into(),
        inputs(
            box_(),
            Some(box_()),
            0,
            vec![OpIn::Squeeze {
                bv: v(0, 0),
                ov: v(1, 0),
            }],
        ),
    ));
    out.push((
        "squeeze_same_pair".into(),
        inputs(
            box_(),
            Some(box_()),
            0,
            vec![
                OpIn::Relate {
                    a: v(0, 2),
                    b: v(1, 2),
                },
                OpIn::Squeeze {
                    bv: v(0, 2),
                    ov: v(1, 2),
                },
            ],
        ),
    ));
    out.push((
        "squeeze_previous_better".into(),
        inputs(
            box_(),
            Some(box_()),
            0,
            vec![
                // the coincident copy is closer than any other vertex
                OpIn::Relate {
                    a: v(0, 3),
                    b: v(1, 3),
                },
                OpIn::Squeeze {
                    bv: v(0, 3),
                    ov: v(1, 5),
                },
            ],
        ),
    ));
    out.push((
        "squeeze_previous_better_other_side".into(),
        inputs(
            box_(),
            Some(box_()),
            0,
            vec![
                OpIn::Relate {
                    a: v(0, 4),
                    b: v(1, 4),
                },
                OpIn::Squeeze {
                    bv: v(0, 6),
                    ov: v(1, 4),
                },
            ],
        ),
    ));
    out.push((
        "squeeze_conflict_closer".into(),
        inputs(
            box_(),
            Some(box_()),
            0,
            vec![
                OpIn::Relate {
                    a: v(0, 3),
                    b: v(1, 5),
                },
                OpIn::Squeeze {
                    bv: v(0, 3),
                    ov: v(1, 3),
                },
            ],
        ),
    ));

    // DeleteFaces: nothing to delete, and a face
    out.push((
        "delete_faces_empty".into(),
        inputs(
            box_(),
            Some(box_()),
            0,
            vec![OpIn::DeleteFaces {
                faces: vec![],
                keep: None,
            }],
        ),
    ));
    out.push((
        "delete_faces_one".into(),
        inputs(
            box_(),
            Some(box_()),
            0,
            vec![OpIn::DeleteFaces {
                faces: vec![[0, i64::from(K_FACE), 0]],
                keep: Some(vec![e(0, 0)]),
            }],
        ),
    ));

    // FixGaps: far vertices, a vertex with itself (common edges), unmated
    out.push((
        "fix_gaps_far".into(),
        inputs(
            box_(),
            Some(box_()),
            0,
            vec![
                OpIn::Relate {
                    a: v(0, 0),
                    b: v(1, 0),
                },
                OpIn::Relate {
                    a: v(0, 1),
                    b: v(1, 1),
                },
                OpIn::FixGaps {
                    verts: vec![v(0, 0), v(0, 1)],
                },
            ],
        ),
    ));
    out.push((
        "fix_gaps_same_vertex".into(),
        inputs(
            box_(),
            Some(box_()),
            0,
            vec![OpIn::FixGaps {
                verts: vec![v(0, 2), v(0, 2)],
            }],
        ),
    ));
    out.push((
        "fix_gaps_coincident_copy_unmated".into(),
        inputs(
            box_(),
            Some(box_()),
            0,
            vec![
                OpIn::Relate {
                    a: v(0, 3),
                    b: v(1, 3),
                },
                OpIn::FixGaps {
                    verts: vec![v(0, 3), v(1, 3)],
                },
            ],
        ),
    ));
    out.push((
        "fix_gaps_wire_unmated".into(),
        inputs(
            BrepIn::Network,
            Some(BrepIn::Network),
            0,
            vec![OpIn::FixGaps {
                verts: vec![v(0, 0), v(0, 0), v(0, 1)],
            }],
        ),
    ));

    // SetRelationships on an empty manager; IntersectTopology / IntersectInsertRelate
    out.push((
        "set_relationships_empty".into(),
        inputs(
            box_(),
            Some(box_()),
            0,
            vec![
                OpIn::MgrNew {
                    capture: 0.01,
                    override_tol: 0.0,
                },
                OpIn::SetRels,
            ],
        ),
    ));
    out.push((
        "intersect_topology".into(),
        inputs(
            box_(),
            Some(file("sphere.smb")),
            0,
            vec![
                OpIn::MgrNew {
                    capture: 0.01,
                    override_tol: 0.0,
                },
                OpIn::IntersectTopo,
            ],
        ),
    ));
    out.push((
        "iir".into(),
        inputs(box_(), Some(file("torus.smb")), 0, vec![OpIn::Iir]),
    ));
    // SetRelationships without the maps (an empty manager: no definition reaches Relate)
    out.push((
        "set_relationships_no_maps".into(),
        inputs(
            box_(),
            Some(box_()),
            1,
            vec![
                OpIn::MgrNew {
                    capture: 0.01,
                    override_tol: 0.0,
                },
                OpIn::SetRels,
            ],
        ),
    ));
    // IntersectInsertRelate at a negative tolerance (only zero hangs, HANG-25)
    for (name, approx) in [("iir_negative_tol", -1e-5), ("iir_tiny_tol", 1e-200)] {
        out.push((
            name.into(),
            serde_json::to_value(Inputs {
                brep_a: box_(),
                brep_b: Some(file("torus.smb")),
                ctor: 0,
                approx,
                ang: 0.1,
                ops: vec![OpIn::Iir],
            })
            .ok(),
        ));
    }
    // CheckForSqueezeVertices: the other vertex's B-rep mate is an edge / a face, which
    // only the second block dereferences; the first one returns SM_ERR (the existing
    // relationship is closer)
    for (name, mate) in [
        ("squeeze_edge_mate", e(0, 0)),
        ("squeeze_face_mate", [0, i64::from(K_FACE), 0]),
    ] {
        out.push((
            name.into(),
            inputs(
                box_(),
                Some(box_()),
                0,
                vec![
                    OpIn::Relate {
                        a: v(0, 0),
                        b: v(1, 0),
                    },
                    OpIn::Relate {
                        a: mate,
                        b: v(1, 6),
                    },
                    OpIn::Squeeze {
                        bv: v(0, 0),
                        ov: v(1, 6),
                    },
                ],
            ),
        ));
    }

    // FixupClassMates / FixupSplitVertexPair on edge 0's curve
    let split_pair = |bv: Hin, ov: Hin| {
        (
            cc(vec![
                ivl(0.0, 0.001, [pci(104, bv, Some(0.0)), unknown(), unknown()]),
                ivl(
                    0.001,
                    1.0,
                    [
                        unknown(),
                        pci(102, [0, i64::from(K_FACE), 0], Some(0.5)),
                        unknown(),
                    ],
                ),
            ]),
            cc(vec![
                ivl(
                    0.0,
                    0.001,
                    [unknown(), unknown(), pci(104, ov, Some(0.001))],
                ),
                ivl(
                    0.001,
                    1.0,
                    [pci(104, ov, Some(0.001)), unknown(), unknown()],
                ),
            ]),
        )
    };
    for (name, rel) in [("split_pair_mated", true), ("split_pair_unmated", false)] {
        let (b, o) = split_pair(v(0, 0), v(1, 0));
        let mut ops = Vec::new();
        if rel {
            ops.push(OpIn::Relate {
                a: v(0, 0),
                b: v(1, 0),
            });
        }
        ops.push(OpIn::FixupSplit {
            index: 0,
            b: b.clone(),
            o: o.clone(),
        });
        ops.push(OpIn::FixupClass { b, o });
        out.push((name.into(), inputs(box_(), Some(box_()), 0, ops)));
    }
    {
        let (b, o) = split_pair(v(0, 0), v(1, 0));
        let mut o2 = o.clone();
        o2.ivls.truncate(1);
        out.push((
            "fixup_class_sizes_differ".into(),
            inputs(
                box_(),
                Some(box_()),
                0,
                vec![
                    OpIn::Relate {
                        a: v(0, 0),
                        b: v(1, 0),
                    },
                    OpIn::FixupClass { b, o: o2 },
                ],
            ),
        ));
    }
    {
        // FixupSplitVertexPair with a shorter other classification: upstream bounds the
        // index + 1 writes by the B-rep one's size only
        let (b, mut o) = split_pair(v(0, 0), v(1, 0));
        o.ivls.truncate(1);
        out.push((
            "split_oob".into(),
            inputs(
                box_(),
                Some(box_()),
                0,
                vec![
                    OpIn::Relate {
                        a: v(0, 0),
                        b: v(1, 0),
                    },
                    OpIn::FixupSplit { index: 0, b, o },
                ],
            ),
        ));
    }
    {
        // mid points on unrelated edges are reset; mated edges are kept
        let b = cc(vec![ivl(
            0.0,
            1.0,
            [unknown(), pci(103, e(0, 1), Some(0.5)), unknown()],
        )]);
        let o = cc(vec![ivl(
            0.0,
            1.0,
            [unknown(), pci(103, e(1, 2), Some(0.5)), unknown()],
        )]);
        out.push((
            "fixup_class_mid_edges".into(),
            inputs(
                box_(),
                Some(box_()),
                0,
                vec![
                    OpIn::Relate {
                        a: e(0, 1),
                        b: e(1, 1),
                    },
                    OpIn::FixupClass {
                        b: b.clone(),
                        o: o.clone(),
                    },
                    OpIn::Relate {
                        a: e(0, 3),
                        b: e(1, 2),
                    },
                    OpIn::FixupClass { b, o },
                ],
            ),
        ));
    }

    // EdgeEdgeIntersect: shared vertices, the same edge, disjoint boxes, empty lists
    out.push((
        "edge_edge_same_brep".into(),
        inputs(
            box_(),
            Some(box_()),
            0,
            vec![OpIn::EdgeEdge {
                edges1: vec![e(0, 0), e(0, 1), null],
                edges2: vec![e(0, 0), e(0, 2), e(0, 5)],
                manifold: false,
                ignore: true,
            }],
        ),
    ));
    out.push((
        "edge_edge_empty".into(),
        inputs(
            box_(),
            Some(box_()),
            0,
            vec![OpIn::EdgeEdge {
                edges1: vec![],
                edges2: vec![e(1, 0)],
                manifold: true,
                ignore: false,
            }],
        ),
    ));
    out.push((
        "edge_edge_far".into(),
        inputs(
            file("sphere.smb"),
            Some(file("torus.smb")),
            0,
            vec![OpIn::EdgeEdge {
                edges1: vec![e(0, 0), e(0, 1)],
                edges2: vec![e(1, 0), e(1, 1), e(1, 2)],
                manifold: false,
                ignore: false,
            }],
        ),
    ));

    // EdgeFaceCoincidence: edges already on the common faces
    out.push((
        "edge_face_coincidence_box".into(),
        inputs(
            box_(),
            Some(box_()),
            0,
            vec![OpIn::EdgeFaceCoin {
                verts: vec![v(0, 0), v(0, 1), v(0, 2), v(0, 3), null],
                excluded: vec![e(0, 0)],
                ignore: false,
                nesting: false,
            }],
        ),
    ));
    out.push((
        "edge_face_coincidence_network".into(),
        inputs(
            BrepIn::Network,
            None,
            0,
            vec![OpIn::EdgeFaceCoin {
                verts: vec![v(0, 0), v(0, 1), v(0, 2)],
                excluded: vec![],
                ignore: true,
                nesting: true,
            }],
        ),
    ));

    out.into_iter()
        .filter_map(|(n, v)| v.map(|v| (n, v)))
        .collect()
}

// ---- the runner ---------------------------------------------------------------------------

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "boolean.topology_intersector_p2"
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        let mut cases = default_cases(self, &ctx)?;
        for (name, inputs) in edge_cases() {
            cases.push(Case {
                unit: self.id().into(),
                name,
                source: CaseSource::Inline,
                inputs,
            });
        }
        Ok(cases)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let k = &solidcore::kernel::Kernel::default();
        let i: Inputs = case.parse()?;
        run_cpp_with(k, &i, ffi::eval)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let k = &solidcore::kernel::Kernel::default();
        let i: Inputs = case.parse()?;
        run_rust(k, &i)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        generate(rng)
    }
}

#[cfg(test)]
mod tests {
    use super::super::boolean_topology_intersector_p1::rust_op_statuses;
    use super::*;
    use std::collections::BTreeMap;

    /// Coverage census (run with `--ignored --nocapture`): per part 2 op (and op 16), how
    /// often each status was reached on the Rust side over the cases difftest runs.
    #[test]
    #[ignore = "census, not a check"]
    fn census() {
        let ctx = CaseContext {
            root: crate::case::repo_root(),
            seeds: MIN_GENERATED,
        };
        let cases = Runner.cases(&ctx).expect("cases");
        let mut tally: BTreeMap<(i64, i64, i64), usize> = BTreeMap::new();
        let mut deferred = 0usize;
        for c in &cases {
            let i: Inputs = c.parse().expect("inputs");
            if i.ops.iter().any(|o| matches!(o, OpIn::Iir)) {
                println!("op47-case {}", c.name);
            }
            match rust_op_statuses(&i) {
                Some(v) => {
                    for (code, st, first) in v {
                        if code == 16.0 || code >= 41.0 {
                            // the first output: FixGaps' `modified`, FixupSplitVertexPair's
                            // `changed`, the size of the first list or classification
                            let first = if st == 0.0 { first as i64 } else { -1 };
                            *tally.entry((code as i64, st as i64, first)).or_default() += 1;
                        }
                    }
                }
                None => deferred += 1,
            }
        }
        println!("cases {} deferred-or-unrunnable {deferred}", cases.len());
        for ((code, st, first), n) in tally {
            println!("op {code} status {st} first {first}: {n}");
        }
    }
}
