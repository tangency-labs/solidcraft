//! Runner for unit `geom.point_set`: point sets and point sequences with cached
//! centroid, axes, bounding boxes, area and point containment.
//!
//! C++: `solidcore_ffi::units::geom_point_set` (upstream `source/SMLib/src/SmPointSet.cpp`).
//! Rust: `solidcore::geom::point_set`.
//!
//! A case builds one point sequence (`pts`, `tol`, `closed`, optional `normal`) and
//! runs a program of commands on it in order (the cached properties depend on the
//! call order). The constructor-unitized normal is key `ctor_normal`; every
//! command's output becomes its own key `cNN_<cmd>`.
//!
//! | `cmd`         | Rust                               | upstream |
//! |---------------|------------------------------------|----------|
//! | (constructor) | `PointSequence::new`               | `SmPointSequence(..)` / `SmPointSet3d(..)` |
//! | `centroid`    | `PointSet3d::centroid`             | `GetCentroid` (`SetUpCentroid`) |
//! | `normal`      | `PointSet3d::normal`               | `GetNormal` (`SetUpAxes`) |
//! | `bbox`        | `PointSet3d::bbox`                 | `GetBBox` (`SetUpBBoxes`) |
//! | `tight`       | `PointSet3d::tight_pbox`           | `GetTightPBox` (`SetUpTightPseudoBox`) |
//! | `area`        | `PointSequence::area`              | `GetArea` (`SetUpArea`) |
//! | `contain`     | `PointSequence::point_containment` | `PointContainment` (`CalcQuadNum`) |
//! | `clear`       | `PointSet3d::clear_cache`          | `SmPointSet3d::ClearCache` |
//! | `size`        | `PointSet3d::points`               | `GetPoints().GetSize()` |
//!
//! Every run has at least [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::point_set::PointSequence;
use solidcore::geom::vector3d::Vec3;
use solidcore_ffi::units::geom_point_set::{self as ffi, Cmd as FCmd};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `geom.point_set` runner.
pub struct Runner;

type V3 = [f64; 3];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum Cmd {
    Centroid,
    Normal,
    Bbox {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        z: Option<V3>,
    },
    Tight {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        z: Option<V3>,
    },
    Area,
    Contain {
        p: V3,
    },
    Clear,
    Size,
}

impl Cmd {
    fn lower(&self) -> FCmd {
        match self {
            Cmd::Centroid => FCmd::Centroid,
            Cmd::Normal => FCmd::Normal,
            Cmd::Bbox { z } => FCmd::BBox(*z),
            Cmd::Tight { z } => FCmd::TightPBox(*z),
            Cmd::Area => FCmd::Area,
            Cmd::Contain { p } => FCmd::Containment(*p),
            Cmd::Clear => FCmd::ClearCache,
            Cmd::Size => FCmd::Size,
        }
    }

    const fn name(&self) -> &'static str {
        match self {
            Cmd::Centroid => "centroid",
            Cmd::Normal => "normal",
            Cmd::Bbox { .. } => "bbox",
            Cmd::Tight { .. } => "tight",
            Cmd::Area => "area",
            Cmd::Contain { .. } => "contain",
            Cmd::Clear => "clear",
            Cmd::Size => "size",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    pts: Vec<V3>,
    tol: f64,
    closed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    normal: Option<V3>,
    prog: Vec<Cmd>,
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

const fn v3(a: V3) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}

fn push(out: &mut Vec<f64>, v: Vec3) {
    out.extend([v.x, v.y, v.z]);
}

/// The Rust side of a case, in the shim's flat output layout.
fn rust_flat(i: &Inputs) -> solidcore::Result<Vec<f64>> {
    let pts: Vec<Vec3> = i.pts.iter().copied().map(v3).collect();
    let mut normal = i.normal.map(v3);
    let mut seq = PointSequence::new(pts, i.tol, i.closed, normal.as_mut());
    let mut out = Vec::new();
    push(&mut out, normal.unwrap_or(Vec3::ZERO));
    for c in &i.prog {
        match c {
            Cmd::Centroid => push(&mut out, seq.centroid()),
            Cmd::Normal => push(&mut out, seq.normal()?),
            Cmd::Bbox { z } => {
                let mut zv = z.map(v3);
                let e = seq.bbox(zv.as_mut())?;
                out.extend([
                    e.u_min(),
                    e.v_min(),
                    e.w_min(),
                    e.u_max(),
                    e.v_max(),
                    e.w_max(),
                ]);
                push(&mut out, zv.unwrap_or(Vec3::ZERO));
            }
            Cmd::Tight { z } => {
                let mut zv = z.map(v3);
                let pb = seq.tight_pbox(zv.as_mut())?;
                for b in pb.basis() {
                    push(&mut out, *b);
                }
                for ivl in pb.intervals() {
                    out.extend([ivl.min(), ivl.max()]);
                }
                push(&mut out, zv.unwrap_or(Vec3::ZERO));
            }
            Cmd::Area => out.push(seq.area()?),
            Cmd::Contain { p } => out.push(f64::from(seq.point_containment(v3(*p))?.code())),
            Cmd::Clear => seq.clear_cache(),
            Cmd::Size => out.push(seq.points().len() as f64),
        }
    }
    Ok(out)
}

/// Splits a flat output into one key per command.
fn split(i: &Inputs, prog: &[FCmd], flat: &[f64]) -> Result<Output, HarnessError> {
    let expected = 3 + prog.iter().map(|c| c.output_len()).sum::<usize>();
    if flat.len() != expected {
        return Err(harness(format!(
            "output length {} != {expected}",
            flat.len()
        )));
    }
    let (head, mut rest) = flat.split_at(3);
    let mut out = Output::new().values("ctor_normal", head.to_vec());
    for (k, (c, fc)) in i.prog.iter().zip(prog).enumerate() {
        let (chunk, tail) = rest.split_at(fc.output_len());
        rest = tail;
        let key = format!("c{k:02}_{}", c.name());
        out = match c {
            Cmd::Clear => out,
            Cmd::Contain { .. } | Cmd::Size => {
                out.ints(&key, chunk.iter().map(|&v| v as i64).collect::<Vec<_>>())
            }
            _ => out.values(&key, chunk.to_vec()),
        };
    }
    Ok(out)
}

fn lower(i: &Inputs) -> Vec<FCmd> {
    i.prog.iter().map(Cmd::lower).collect()
}

// ---- generation ------------------------------------------------------------------------

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

/// A random unit vector.
fn unit(rng: &mut Rng) -> V3 {
    loop {
        let p = rng.point3(-1.0, 1.0);
        let l = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
        if l > 0.1 {
            return scale(p, 1.0 / l);
        }
    }
}

/// An orthonormal pair spanning the plane with normal `n`.
fn plane_frame(n: V3) -> (V3, V3) {
    let v = Vec3::new(n[0], n[1], n[2]);
    match v.make_unit_ortho_vectors(None) {
        Ok([_, y, z]) => ([y.x, y.y, y.z], [z.x, z.y, z.z]),
        Err(_) => ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    }
}

/// A closed polygon of `n` vertices in the plane through `c` with normal `nrm`.
fn polygon(rng: &mut Rng, c: V3, nrm: V3, n: usize, star: bool) -> Vec<V3> {
    let (u, v) = plane_frame(nrm);
    let r0 = rng.uniform(0.5, 20.0);
    let ccw = rng.chance(0.6);
    let jitter = rng.chance(0.5);
    (0..n)
        .map(|i| {
            let mut t = std::f64::consts::TAU * i as f64 / n as f64;
            if jitter {
                t += rng.uniform(-0.2, 0.2) * std::f64::consts::TAU / n as f64;
            }
            if !ccw {
                t = -t;
            }
            let r = if star && i % 2 == 1 {
                r0 * rng.uniform(0.2, 0.8)
            } else {
                r0
            };
            add(c, add(scale(u, r * t.cos()), scale(v, r * t.sin())))
        })
        .collect()
}

/// An axis-aligned rectangle in a z = const plane with extra points on its edges.
fn rectangle(rng: &mut Rng) -> Vec<V3> {
    let (x0, y0, z) = (
        rng.uniform(-10.0, 10.0),
        rng.uniform(-10.0, 10.0),
        rng.uniform(-5.0, 5.0),
    );
    let (w, h) = (rng.uniform(0.5, 10.0), rng.uniform(0.5, 10.0));
    let k = 1 + rng.below(4) as usize;
    let mut pts = Vec::new();
    let corners = [[x0, y0], [x0 + w, y0], [x0 + w, y0 + h], [x0, y0 + h]];
    for (a, b) in corners.iter().zip(corners.iter().cycle().skip(1)) {
        for s in 0..k {
            let t = s as f64 / k as f64;
            pts.push([a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1]), z]);
        }
    }
    pts
}

fn gen_points(rng: &mut Rng) -> (Vec<V3>, Option<V3>) {
    let c = rng.point3(-20.0, 20.0);
    let nrm = unit(rng);
    match rng.below(10) {
        0 => {
            let n = rng.below(13) as usize;
            ((0..n).map(|_| rng.point3(-10.0, 10.0)).collect(), None)
        }
        1 | 2 => {
            let n = 3 + rng.below(30) as usize;
            let star = rng.chance(0.4);
            (polygon(rng, c, nrm, n, star), Some(nrm))
        }
        3 => (rectangle(rng), Some([0.0, 0.0, 1.0])),
        4 => {
            let d = unit(rng);
            let n = 2 + rng.below(9) as usize;
            let pts = (0..n)
                .map(|_| add(c, scale(d, rng.uniform(-10.0, 10.0))))
                .collect();
            (pts, None)
        }
        5 => {
            let n = 1 + rng.below(5) as usize;
            let e = rng.uniform(0.0, 1e-13);
            (
                (0..n)
                    .map(|_| add(c, scale(rng.point3(-1.0, 1.0), e)))
                    .collect(),
                None,
            )
        }
        6 => {
            // near-planar: a polygon with off-plane noise
            let n = 4 + rng.below(20) as usize;
            let noise = *rng.choose(&[1e-10, 1e-7, 1e-4]).unwrap_or(&1e-7);
            let pts = polygon(rng, c, nrm, n, false)
                .into_iter()
                .map(|p| add(p, scale(nrm, rng.uniform(-noise, noise))))
                .collect();
            (pts, Some(nrm))
        }
        7 => {
            // multiples of 50 (centroid block quirk) and neighbours
            let n = *rng.choose(&[49_usize, 50, 51, 100, 150]).unwrap_or(&50);
            (polygon(rng, c, nrm, n, false), Some(nrm))
        }
        8 => {
            // triangle or quad, possibly degenerate
            let n = 1 + rng.below(4) as usize;
            ((0..n).map(|_| rng.point3(-5.0, 5.0)).collect(), None)
        }
        _ => {
            // polygon in a coordinate plane (axis-aligned normal)
            let axis = [
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
                [0.0, 0.0, -1.0],
            ];
            let a = *rng.choose(&axis).unwrap_or(&[0.0, 0.0, 1.0]);
            let n = 3 + rng.below(12) as usize;
            let star = rng.chance(0.3);
            (polygon(rng, c, a, n, star), Some(a))
        }
    }
}

fn gen_z(rng: &mut Rng, plane: Option<V3>) -> Option<V3> {
    match rng.below(8) {
        0 => Some(unit(rng)),
        1 => Some(scale(
            plane.unwrap_or([0.0, 0.0, 1.0]),
            rng.uniform(0.5, 3.0),
        )),
        2 => Some([0.0, 0.0, 0.0]),
        3 => Some([0.0, 0.0, 1.0]),
        _ => None,
    }
}

fn gen_point(rng: &mut Rng, pts: &[V3], plane: Option<V3>) -> V3 {
    let pick = |rng: &mut Rng| {
        pts.get(rng.below(pts.len() as u64) as usize)
            .copied()
            .unwrap_or_default()
    };
    match rng.below(7) {
        0 => pick(rng),
        1 => {
            let (a, b) = (pick(rng), pick(rng));
            scale(add(a, b), 0.5)
        }
        2 => {
            let n = pts.len().max(1) as f64;
            scale(pts.iter().fold([0.0; 3], |s, p| add(s, *p)), 1.0 / n)
        }
        3 => {
            // off-plane offset of a centroid
            let n = pts.len().max(1) as f64;
            let c = scale(pts.iter().fold([0.0; 3], |s, p| add(s, *p)), 1.0 / n);
            add(
                c,
                scale(plane.unwrap_or([0.0, 0.0, 1.0]), rng.uniform(-1e-6, 1e-6)),
            )
        }
        4 => rng.point3(-50.0, 50.0),
        _ => {
            // a random convex combination of three vertices
            let (a, b, c) = (pick(rng), pick(rng), pick(rng));
            let (s, t) = (rng.unit(), rng.unit());
            let (s, t) = if s + t > 1.0 {
                (1.0 - s, 1.0 - t)
            } else {
                (s, t)
            };
            add(
                a,
                add(
                    scale(add(b, scale(a, -1.0)), s),
                    scale(add(c, scale(a, -1.0)), t),
                ),
            )
        }
    }
}

fn gen_inputs(rng: &mut Rng) -> Inputs {
    let (pts, plane) = gen_points(rng);
    let tol = *rng
        .choose(&[0.0, 1e-12, 1e-9, 1e-6, 1e-3, 0.1])
        .unwrap_or(&1e-9);
    let closed = rng.chance(0.85);
    let normal = if rng.chance(0.3) {
        match rng.below(4) {
            0 => Some(unit(rng)),
            1 => Some([0.0, 0.0, 0.0]),
            _ => Some(scale(
                plane.unwrap_or([0.0, 0.0, 1.0]),
                rng.uniform(0.5, 3.0),
            )),
        }
    } else {
        None
    };
    let k = 1 + rng.below(7) as usize;
    let mut prog = Vec::with_capacity(k);
    for _ in 0..k {
        // an empty set with a normal is planar: its tight box reads m_pData[0] upstream
        let tight_ok = !(pts.is_empty() && normal.is_some());
        let c = match rng.below(12) {
            0 => Cmd::Centroid,
            1 => Cmd::Normal,
            2 => Cmd::Bbox {
                z: gen_z(rng, plane),
            },
            3 | 4 if tight_ok => Cmd::Tight {
                z: gen_z(rng, plane),
            },
            5 => Cmd::Clear,
            6 => Cmd::Size,
            7 if !pts.is_empty() => Cmd::Area,
            8..=11 if !pts.is_empty() => Cmd::Contain {
                p: gen_point(rng, &pts, plane),
            },
            _ if tight_ok => Cmd::Tight { z: None },
            _ => Cmd::Normal,
        };
        prog.push(c);
    }
    Inputs {
        pts,
        tol,
        closed,
        normal,
        prog,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "geom.point_set"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let prog = lower(&i);
        let flat = ffi::run(&i.pts, i.tol, i.closed, i.normal, &prog)
            .map_err(|e| harness(e.to_string()))?;
        split(&i, &prog, &flat)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let prog = lower(&i);
        match rust_flat(&i) {
            Ok(flat) => split(&i, &prog, &flat),
            Err(e) => Ok(Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string()))),
        }
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_inputs(rng)).ok()
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
    use crate::rng::fnv1a64;

    /// The generated programs reach every containment outcome and both
    /// tight-box paths.
    #[test]
    fn generated_cases_cover_branches() {
        let mut contain = [0_usize; 4];
        for i in 0..MIN_GENERATED as u64 {
            let mut rng = Rng::new(fnv1a64("geom.point_set").wrapping_add(i));
            let inputs = gen_inputs(&mut rng);
            let prog = lower(&inputs);
            let out = split(&inputs, &prog, &rust_flat(&inputs).unwrap()).unwrap();
            for (k, v) in &out.ints {
                if k.ends_with("contain") {
                    contain[usize::try_from(v[0]).unwrap()] += 1;
                }
            }
        }
        assert!(
            contain[1] > 0 && contain[2] > 0 && contain[3] > 0,
            "{contain:?}"
        );
    }
}
