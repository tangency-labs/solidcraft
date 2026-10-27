//! Runner for unit `geom.hash_table`: the voxel model `SmObjsInVoxels` and the
//! bucket hash table under it.
//!
//! C++: `solidcore_ffi::units::geom_hash_table` (upstream
//! `source/SMLib/inc/SmHashTable.h`). Rust: `solidcore::geom::hash_table`.
//!
//! A case is a program run against one voxel model on both sides
//! (`kind = "program"`), or a point set run through the `SmHealData::
//! Cache_CoinVertices` pattern (`kind = "heal"`: a 12x12x12 model over the
//! points' box with `7 * n` estimated items, every point added, all pairs
//! searched with a histogram, then optionally a second batch added with
//! tracked buckets and searched with the old vertex count as limit), expanded
//! into the same program form.
//!
//! Outputs: `ints` (voxel / item indices, counts, the full bucket structure on
//! `dump`; `SM_UNDEF_ULONG` = 123456789, `-1` = null) and `floats` (voxel
//! boxes, stored points and tolerances). Every run has at least
//! [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::hash_table::{FindPairsOptions, ObjsInVoxels, GAP_HISTOGRAM_BUCKETS};
use solidcore::geom::tol::zone_tol3d;
use solidcore::kernel::{Kernel, KernelOptions};
use solidcore::SolidError;
use solidcore_ffi::units::geom_hash_table as ffi;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::v3;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 600;

/// `SM_UNDEF_ULONG`.
const UNDEF_ULONG: i64 = 123_456_789;

/// `geom.hash_table` runner.
pub struct Runner;

type P3 = [f64; 3];
type E6 = [f64; 6];

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Sizes {
    x: u32,
    y: u32,
    z: u32,
    bbox: Option<E6>,
    est: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Op {
    SetSize(Sizes),
    Add {
        item: i64,
        p: P3,
        tol: f64,
        #[serde(default)]
        track: bool,
    },
    Remove {
        item: i64,
    },
    Find {
        #[serde(default)]
        limit: Option<u64>,
        /// 0: all used buckets, 1: the tracked buckets, 2: `buckets`.
        #[serde(default)]
        mode: u8,
        #[serde(default)]
        buckets: Vec<u32>,
        #[serde(default)]
        hist: bool,
    },
    MapPoint {
        p: P3,
    },
    VoxelBbox {
        index: u64,
    },
    FindItems {
        item: i64,
    },
    Dump,
    IndicesToIndex {
        ix: u32,
        iy: u32,
        iz: u32,
    },
    IndexToIndices {
        index: u64,
    },
    ClearUpdated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Inputs {
    Program {
        ctor: Sizes,
        ops: Vec<Op>,
    },
    Heal {
        pts: Vec<P3>,
        tol: f64,
        #[serde(default)]
        dup: Vec<usize>,
        #[serde(default)]
        extra: Vec<P3>,
    },
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

fn put_ext(o: &mut Vec<f64>, e: &Extent3d) {
    let (a, b) = (e.min(), e.max());
    o.extend([a.x, a.y, a.z, b.x, b.y, b.z]);
}

fn opt(v: Option<usize>) -> i64 {
    v.map_or(-1, |i| i as i64)
}

fn bbox_of(pts: &[P3]) -> E6 {
    let mut e = Extent3d::new();
    for p in pts {
        e.add_point(v3(*p));
    }
    let (a, b) = (e.min(), e.max());
    [a.x, a.y, a.z, b.x, b.y, b.z]
}

/// The `SmHealData::Cache_CoinVertices` call pattern as a program.
fn heal_program(pts: &[P3], tol: f64, dup: &[usize], extra: &[P3]) -> (Sizes, Vec<Op>) {
    let mut all: Vec<P3> = pts.to_vec();
    for &d in dup {
        if let Some(p) = pts.get(d) {
            all.push([p[0] + tol * 0.25, p[1] - tol * 0.125, p[2]]);
        }
    }
    let ctor = Sizes {
        x: 12,
        y: 12,
        z: 12,
        bbox: Some(bbox_of(&all)),
        est: (all.len() * 7) as u32,
    };
    let mut ops: Vec<Op> = Vec::new();
    for (i, p) in all.iter().enumerate() {
        ops.push(Op::Add {
            item: i as i64,
            p: *p,
            tol,
            track: false,
        });
    }
    ops.push(Op::Find {
        limit: None,
        mode: 0,
        buckets: Vec::new(),
        hist: true,
    });
    ops.push(Op::Dump);
    if !extra.is_empty() {
        let old = all.len();
        for (i, p) in extra.iter().enumerate() {
            ops.push(Op::Add {
                item: (old + i) as i64,
                p: *p,
                tol,
                track: true,
            });
        }
        ops.push(Op::Find {
            limit: Some(old as u64),
            mode: 1,
            buckets: Vec::new(),
            hist: true,
        });
        ops.push(Op::Dump);
    }
    (ctor, ops)
}

fn program(i: &Inputs) -> (Sizes, Vec<Op>) {
    match i {
        Inputs::Program { ctor, ops } => (*ctor, ops.clone()),
        Inputs::Heal {
            pts,
            tol,
            dup,
            extra,
        } => heal_program(pts, *tol, dup, extra),
    }
}

fn push_sizes(v: &mut Vec<f64>, s: &Sizes) {
    v.extend([f64::from(s.x), f64::from(s.y), f64::from(s.z)]);
    match s.bbox {
        Some(b) => {
            v.push(1.0);
            v.extend(b);
        }
        None => v.extend([0.0; 7]),
    }
    v.push(f64::from(s.est));
}

fn encode(ctor: &Sizes, ops: &[Op]) -> Vec<f64> {
    let mut v = vec![0.0];
    push_sizes(&mut v, ctor);
    for op in ops {
        match op {
            Op::SetSize(s) => {
                v.push(1.0);
                push_sizes(&mut v, s);
            }
            Op::Add { item, p, tol, track } => {
                v.push(2.0);
                v.push(*item as f64);
                v.extend(p);
                v.push(*tol);
                v.push(if *track { 1.0 } else { 0.0 });
            }
            Op::Remove { item } => v.extend([3.0, *item as f64]),
            Op::Find {
                limit,
                mode,
                buckets,
                hist,
            } => {
                v.push(4.0);
                v.push(limit.map_or(-1.0, |l| l as f64));
                v.push(f64::from(*mode));
                v.push(if *hist { 1.0 } else { 0.0 });
                v.push(buckets.len() as f64);
                v.extend(buckets.iter().map(|&b| f64::from(b)));
            }
            Op::MapPoint { p } => {
                v.push(5.0);
                v.extend(p);
            }
            Op::VoxelBbox { index } => v.extend([6.0, *index as f64]),
            Op::FindItems { item } => v.extend([7.0, *item as f64]),
            Op::Dump => v.push(8.0),
            Op::IndicesToIndex { ix, iy, iz } => {
                v.extend([9.0, f64::from(*ix), f64::from(*iy), f64::from(*iz)]);
            }
            Op::IndexToIndices { index } => v.extend([10.0, *index as f64]),
            Op::ClearUpdated => v.push(11.0),
        }
    }
    v
}

type Model = ObjsInVoxels<i64>;

/// Item of the fake-pointer encoding (`-1` = NULL).
fn item_code(i: i64) -> i64 {
    if i < 0 {
        -1
    } else {
        i
    }
}

fn build(s: &Sizes) -> solidcore::Result<Model> {
    let b = s.bbox.map(|b| Extent3d::from_bounds(b[0], b[1], b[2], b[3], b[4], b[5]));
    Model::new(s.x as usize, s.y as usize, s.z as usize, b.as_ref(), s.est as usize)
}

fn dump(m: &Model, upd: &[usize], oi: &mut Vec<i64>, of: &mut Vec<f64>) {
    let h = m.hash_table();
    let pts = m.point_items();
    oi.extend([
        pts.len() as i64,
        m.point_capacity() as i64,
        h.capacity() as i64,
        m.removed_point_item_count() as i64,
        h.bucket_count() as i64,
        h.used_bucket_count() as i64,
        h.removed_bucket_item_count() as i64,
        h.total_bucket_item_count() as i64,
    ]);
    let first = if h.used_bucket_count() > 0 {
        h.first_used_bucket()
    } else {
        None
    };
    oi.push(opt(first));
    oi.extend(h.buckets().iter().map(|b| opt(*b)));
    for b in h.bucket_items() {
        oi.extend([
            opt(b.next_used_bucket),
            opt(b.next_bucket_item),
            b.bucket_index.map_or(UNDEF_ULONG, |i| i as i64),
            opt(b.item),
            b.item_id as i64,
        ]);
    }
    for p in pts {
        oi.push(item_code(p.item));
        of.extend([p.point.x, p.point.y, p.point.z, p.zone_tol]);
    }
    let mut walk = Vec::new();
    let mut cur = first;
    while let Some(c) = cur {
        walk.push(c as i64);
        cur = h.bucket_items().get(c).and_then(|r| r.next_used_bucket);
    }
    oi.push(walk.len() as i64);
    oi.extend(walk);
    oi.push(upd.len() as i64);
    oi.extend(upd.iter().map(|&u| u as i64));
}

fn run_rust(ctor: &Sizes, ops: &[Op]) -> solidcore::Result<(Vec<i64>, Vec<f64>)> {
    let k = Kernel::new(&KernelOptions::default());
    let mut m = build(ctor)?;
    let mut upd: Vec<usize> = Vec::new();
    let mut hist = [0u64; GAP_HISTOGRAM_BUCKETS];
    let mut oi = Vec::new();
    let mut of = Vec::new();
    for op in ops {
        match op {
            Op::SetSize(s) => {
                let b = s.bbox.map(|b| Extent3d::from_bounds(b[0], b[1], b[2], b[3], b[4], b[5]));
                m.set_size(s.x as usize, s.y as usize, s.z as usize, b.as_ref(), s.est as usize)?;
            }
            Op::Add { item: it, p, tol, track } => {
                let id = item_code(*it);
                let u = if *track { Some(&mut upd) } else { None };
                let r = m.add(id, v3(*p), zone_tol3d(&k, *tol), u)?;
                oi.push(r.map_or(UNDEF_ULONG, |v| v as i64));
            }
            Op::Remove { item: it } => oi.push(m.remove(item_code(*it)) as i64),
            Op::Find {
                limit,
                mode,
                buckets,
                hist: h,
            } => {
                let list: Vec<usize> = buckets.iter().map(|&b| b as usize).collect();
                let sel = match mode {
                    1 => Some(upd.as_slice()),
                    2 => Some(list.as_slice()),
                    _ => None,
                };
                let opts = FindPairsOptions {
                    limit_index: limit.map(|l| l as usize),
                    buckets: sel,
                    histogram: if *h { Some(&mut hist) } else { None },
                };
                let pairs = m.find_coincident_pairs(&k, opts)?;
                oi.push(pairs.first.len() as i64);
                for (a, b) in pairs.first.iter().zip(&pairs.second) {
                    oi.extend([*a as i64, *b as i64]);
                }
                if *h {
                    oi.extend(hist.iter().map(|&c| c as i64));
                }
            }
            Op::MapPoint { p } => {
                if let Some(hit) = m.map_point_to_voxel(v3(*p)) {
                    oi.extend([hit.index as i64, hit.ix as i64, hit.iy as i64, hit.iz as i64]);
                    put_ext(&mut of, &hit.extent);
                } else {
                    oi.extend([UNDEF_ULONG; 4]);
                    put_ext(&mut of, &Extent3d::new());
                }
            }
            Op::VoxelBbox { index } => put_ext(&mut of, &m.map_voxel_index_to_bbox(*index as usize)),
            Op::FindItems { item: it } => {
                let f = m.find_point_items(item_code(*it));
                oi.push(f.len() as i64);
                oi.extend(f.iter().map(|&i| i as i64));
            }
            Op::Dump => dump(&m, &upd, &mut oi, &mut of),
            Op::IndicesToIndex { ix, iy, iz } => {
                oi.push(m.map_indices_to_voxel_index(*ix as usize, *iy as usize, *iz as usize) as i64);
            }
            Op::IndexToIndices { index } => {
                let (x, y, z) = m.map_voxel_index_to_indices(*index as usize)?;
                oi.extend([x as i64, y as i64, z as i64]);
            }
            Op::ClearUpdated => upd.clear(),
        }
    }
    Ok((oi, of))
}

fn rust_err(e: &SolidError) -> Output {
    Output::unported(e).unwrap_or_else(|| Output::err(format!("rust_error: {e}")))
}

// ---- generation -------------------------------------------------------

fn gen_sizes(rng: &mut Rng, scale: f64, n_hint: usize) -> (Sizes, f64) {
    let c = |rng: &mut Rng| *rng.choose(&[1u32, 1, 2, 3, 4, 5, 8]).unwrap_or(&2);
    let (x, y, z) = (c(rng), c(rng), c(rng));
    let lo = rng.point3(-scale, scale);
    let len = |rng: &mut Rng| match rng.below(12) {
        0 => 0.0,
        1 => scale * 1e-9,
        _ => rng.uniform(0.1 * scale, 2.0 * scale),
    };
    let (lx, ly, lz) = (len(rng), len(rng), len(rng));
    let bbox = if rng.chance(0.04) {
        None
    } else if rng.chance(0.02) {
        // inverted box: voxel corners come out of order
        Some([lo[0] + lx, lo[1], lo[2], lo[0], lo[1] + ly, lo[2] + lz])
    } else {
        Some([lo[0], lo[1], lo[2], lo[0] + lx, lo[1] + ly, lo[2] + lz])
    };
    let est = match rng.below(4) {
        0 => 0,
        1 => rng.range_i64(1, 20) as u32,
        _ => (n_hint * 7) as u32,
    };
    let s = Sizes { x, y, z, bbox, est };
    let span = lx.max(ly).max(lz).max(scale * 0.1);
    (s, span)
}

fn rand_point(rng: &mut Rng, s: &Sizes, anchors: &[P3], tol: f64, span: f64) -> P3 {
    let Some(b) = s.bbox else {
        return rng.point3(-span, span);
    };
    let lo = [b[0], b[1], b[2]];
    let hi = [b[3], b[4], b[5]];
    let cnt = [s.x, s.y, s.z];
    let mut p = [0.0; 3];
    match rng.below(10) {
        // near an existing point (coincident or almost)
        0..=2 if !anchors.is_empty() => {
            let a = *rng.choose(anchors).unwrap_or(&[0.0; 3]);
            let f = *rng.choose(&[0.0, 0.1, 0.4, 0.9, 1.5, 3.0]).unwrap_or(&0.1);
            let d = rng.point3(-1.0, 1.0);
            for ((pk, ak), dk) in p.iter_mut().zip(a).zip(d) {
                *pk = ak + dk * tol * f;
            }
        }
        // on / near a voxel boundary
        3..=5 => {
            for (((pk, l), h), c) in p.iter_mut().zip(lo).zip(hi).zip(cnt) {
                let len = h - l;
                let j = rng.range_i64(0, i64::from(c)) as f64;
                let off = *rng.choose(&[0.0, 0.5, -0.5, 0.999, 1.001, 2.0]).unwrap_or(&0.0);
                *pk = if rng.chance(0.6) {
                    l + j * (len / f64::from(c)) + off * tol
                } else {
                    rng.uniform(l, h)
                };
            }
        }
        // outside the box
        6 => {
            for ((pk, l), h) in p.iter_mut().zip(lo).zip(hi) {
                *pk = rng.uniform(l - span, h + span);
            }
        }
        _ => {
            for ((pk, l), h) in p.iter_mut().zip(lo).zip(hi) {
                *pk = rng.uniform(l, h);
            }
        }
    }
    p
}

fn gen_program(rng: &mut Rng) -> Option<Inputs> {
    let scale = *rng.choose(&[1.0, 10.0, 1000.0, 1e-3]).unwrap_or(&1.0);
    let n_items = rng.range_i64(1, 40) as usize;
    let (ctor, span) = gen_sizes(rng, scale, n_items);
    let mut sizes = ctor;
    let tol = match rng.below(8) {
        0 => 0.0,
        1 => span * 0.3,
        _ => span * *rng.choose(&[1e-3, 1e-2, 5e-2]).unwrap_or(&1e-2),
    };
    let mut m = build(&ctor).ok()?;
    let mut upd: Vec<usize> = Vec::new();
    let mut anchors: Vec<P3> = Vec::new();
    let mut ops = Vec::new();
    let n_ops = rng.range_i64(4, 60);
    let mut next_item = 0i64;
    for _ in 0..n_ops {
        let r = rng.below(100);
        let voxels = sizes.x * sizes.y * sizes.z;
        let op = if r < 50 {
            let p = rand_point(rng, &sizes, &anchors, tol, span);
            let it = if rng.chance(0.08) {
                rng.range_i64(-1, next_item.max(1))
            } else {
                next_item += 1;
                next_item - 1
            };
            let t = if rng.chance(0.1) { tol * rng.uniform(0.0, 3.0) } else { tol };
            if sizes.bbox.is_some() && voxels == 0 {
                continue;
            }
            anchors.push(p);
            Op::Add {
                item: it,
                p,
                tol: t,
                track: rng.chance(0.4),
            }
        } else if r < 58 {
            Op::Remove {
                item: rng.range_i64(-1, next_item + 1),
            }
        } else if r < 75 {
            let mode = rng.below(3) as u8;
            let bucket_count = m.hash_table().bucket_count();
            if (mode == 1 && upd.is_empty()) || (mode == 2 && bucket_count == 0) {
                continue;
            }
            let buckets = if mode == 2 {
                (0..rng.range_i64(1, 6))
                    .map(|_| rng.below(bucket_count as u64) as u32)
                    .collect()
            } else {
                Vec::new()
            };
            Op::Find {
                limit: if rng.chance(0.3) {
                    Some(rng.below(m.len() as u64 + 2))
                } else {
                    None
                },
                mode,
                buckets,
                hist: rng.chance(0.5),
            }
        } else if r < 81 {
            Op::MapPoint {
                p: rand_point(rng, &sizes, &anchors, tol, span),
            }
        } else if r < 84 {
            Op::VoxelBbox {
                index: rng.below(u64::from(voxels) + 3),
            }
        } else if r < 88 {
            Op::FindItems {
                item: rng.range_i64(-1, next_item + 1),
            }
        } else if r < 91 {
            Op::Dump
        } else if r < 93 {
            Op::IndicesToIndex {
                ix: rng.below(u64::from(sizes.x) + 2) as u32,
                iy: rng.below(u64::from(sizes.y) + 2) as u32,
                iz: rng.below(u64::from(sizes.z) + 2) as u32,
            }
        } else if r < 95 {
            if sizes.x * sizes.y == 0 {
                continue;
            }
            Op::IndexToIndices {
                index: rng.below(u64::from(voxels) + 3),
            }
        } else if r < 97 {
            Op::ClearUpdated
        } else {
            let (s, _) = gen_sizes(rng, scale, n_items);
            sizes = s;
            Op::SetSize(s)
        };
        // keep the simulation in step (the generator's model is the Rust port)
        if apply(&mut m, &mut upd, &op).is_err() {
            return None;
        }
        ops.push(op);
    }
    ops.push(Op::Dump);
    Some(Inputs::Program { ctor, ops })
}

/// Applies one op to the generator's model.
fn apply(m: &mut Model, upd: &mut Vec<usize>, op: &Op) -> solidcore::Result<()> {
    let k = Kernel::new(&KernelOptions::default());
    match op {
        Op::SetSize(s) => *m = build(s)?,
        Op::Add { item: it, p, tol, track } => {
            let u = if *track { Some(upd) } else { None };
            m.add(item_code(*it), v3(*p), zone_tol3d(&k, *tol), u)?;
        }
        Op::Remove { item: it } => {
            m.remove(item_code(*it));
        }
        Op::Find {
            limit,
            mode,
            buckets,
            ..
        } => {
            let list: Vec<usize> = buckets.iter().map(|&b| b as usize).collect();
            let sel = match mode {
                1 => Some(upd.as_slice()),
                2 => Some(list.as_slice()),
                _ => None,
            };
            m.find_coincident_pairs(
                &k,
                FindPairsOptions {
                    limit_index: limit.map(|l| l as usize),
                    buckets: sel,
                    histogram: None,
                },
            )?;
        }
        Op::ClearUpdated => upd.clear(),
        _ => {}
    }
    Ok(())
}

impl UnitDiff for Runner {
    fn reference_platform(&self) -> Option<crate::platform::ReferencePlatform> {
        Some(crate::platform::X86_64_HASH_CASTS)
    }

    fn id(&self) -> &'static str {
        "geom.hash_table"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let (ctor, ops) = program(&i);
        let (oi, of) = ffi::run(&encode(&ctor, &ops)).map_err(|e| harness(e.to_string()))?;
        Ok(Output::new().ints("ints", oi).values("floats", of))
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let (ctor, ops) = program(&i);
        match run_rust(&ctor, &ops) {
            Ok((oi, of)) => Ok(Output::new().ints("ints", oi).values("floats", of)),
            Err(e) => Ok(rust_err(&e)),
        }
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..8 {
            if let Some(i) = gen_program(rng) {
                return serde_json::to_value(i).ok();
            }
        }
        None
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heal_program_shape() {
        let (c, ops) = heal_program(&[[0.0; 3], [1.0, 1.0, 1.0]], 0.01, &[0], &[[0.5; 3]]);
        assert_eq!(c.est, 21);
        assert_eq!(ops.len(), 3 + 2 + 1 + 2);
        let enc = encode(&c, &ops);
        assert_eq!(enc[0], 0.0);
    }

    /// The generated programs reach the interesting paths.
    #[test]
    fn generated_coverage() {
        let k = Kernel::new(&KernelOptions::default());
        let (mut pairs, mut removed, mut neighbours, mut tracked, mut cases) = (0, 0, 0, 0, 0);
        for i in 0..MIN_GENERATED {
            let mut rng = Rng::new(CaseContext::seed_for("geom.hash_table", i));
            let Some(v) = Runner.generate(&mut rng) else { continue };
            cases += 1;
            let Inputs::Program { ctor, ops } = serde_json::from_value(v).unwrap() else {
                panic!()
            };
            let mut m = build(&ctor).unwrap();
            let mut upd = Vec::new();
            let (mut p, mut r, mut nb, mut tr) = (false, false, false, false);
            for op in &ops {
                match op {
                    Op::Add { item: it, p: pt, tol, track } => {
                        let before = m.len();
                        let u = if *track { Some(&mut upd) } else { None };
                        m.add(*it, v3(*pt), *tol, u).unwrap();
                        nb |= m.len() > before + 1;
                    }
                    Op::Find { limit, mode, buckets, .. } => {
                        tr |= *mode != 0;
                        let list: Vec<usize> = buckets.iter().map(|&b| b as usize).collect();
                        let sel = match mode {
                            1 => Some(upd.as_slice()),
                            2 => Some(list.as_slice()),
                            _ => None,
                        };
                        let f = m
                            .find_coincident_pairs(
                                &k,
                                FindPairsOptions {
                                    limit_index: limit.map(|l| l as usize),
                                    buckets: sel,
                                    histogram: None,
                                },
                            )
                            .unwrap();
                        p |= !f.first.is_empty();
                    }
                    Op::Remove { item: it } => r |= m.remove(*it) > 0,
                    other => apply(&mut m, &mut upd, other).unwrap(),
                }
            }
            pairs += usize::from(p);
            removed += usize::from(r);
            neighbours += usize::from(nb);
            tracked += usize::from(tr);
        }
        eprintln!("cases {cases} pairs {pairs} removed {removed} neighbours {neighbours} tracked {tracked}");
        assert!(cases > 500 && pairs > 100 && removed > 100 && neighbours > 100 && tracked > 100);
    }

    #[test]
    fn item_helpers() {
        assert_eq!(item_code(-5), -1);
    }
}
