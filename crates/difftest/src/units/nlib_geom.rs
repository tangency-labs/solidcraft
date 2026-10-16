//! Runner for unit `nlib.geom`: NLib vectors, lines, planes, intersections,
//! projections, distances, rotation / frame matrices, control-point reflection and
//! collinearity, polygon and net bounding boxes.
//!
//! C++: `solidcore_ffi::units::nlib_geom` (upstream `source/SMLib/src/NL_Geom.cpp`,
//! `NL_FrameBasic.cpp`). Rust: `solidcore::nurbs::geom`.
//!
//! Each case runs one *op* (`inputs.op`, a group of upstream calls). Both sides
//! produce the same flat record of doubles, split into keys by the op's layout
//! ([`layout`]). Statuses are inlined: `0` ok, else the `NL_*_ERR` number (`-1` on
//! the Rust side for a non-NLib error, which never matches). Outputs upstream
//! leaves unwritten are NaN on both sides; `N_DistPerpPtLineSeg`'s `-1.0` is the
//! Rust `None`.
//!
//! Generated cases avoid inputs for which upstream reads uninitialised memory
//! (NaN coordinates or zero weights in collinearity tests); see the module docs of
//! `solidcore::nurbs::geom`. Every run has at least [`MIN_GENERATED`] generated
//! cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::nurbs::geom::{
    cpt_reflect, cpts_are_colinear, dist_cpolygon, dist_cpt_cpt, dist_cpt_cpt_homo,
    dist_perp_pt_line_seg, dist_polygon, dist_pt_inf_line, dist_pt_line_seg, dist_pt_pt,
    dist_signed_pt_plane, dist_sq_pt_pt, isect_line_line, isect_line_plane, net_bbox,
    normalize, plane_pt_normal_to_implicit, polygon_bbox, project_pt_line, project_pt_plane,
    pt_along_vector, rotation_matrix_about_axis, transform_matrix_from_axes, vector_diff_cpts,
    vectors_angle, vectors_cos_angle, ColinearDim, Frame, Interval, LinePlaneIsect, LineSeg, MinMaxBox,
    Plane, Rectangle,
};
use solidcore::nurbs::math::{combine2_pts, CPoint, RealMatrix, NOW, NOZ};
use solidcore::{Result as SolidResult, SolidError, Vec3};
use solidcore_ffi::units::nlib_geom::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::{flag, v3};
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `nlib.geom` runner.
pub struct Runner;

/// A line in fixtures: start, direction, bounded flag.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Line {
    p: [f64; 3],
    v: [f64; 3],
    bounded: bool,
}

/// A plane in fixtures: point and normal.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Pl {
    p: [f64; 3],
    n: [f64; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    VectorOps {
        a: [f64; 3],
        b: [f64; 3],
        alpha: f64,
        beta: f64,
        pw: [f64; 4],
        qw: [f64; 4],
    },
    IsectLineLine {
        l1: Line,
        l2: Line,
    },
    IsectLinePlane {
        l: Line,
        plane: Pl,
    },
    ProjectPtLine {
        l: Line,
        pt: [f64; 3],
    },
    PlaneOps {
        plane: Pl,
        pt: [f64; 3],
        bounded: bool,
    },
    DistSeg {
        p: [f64; 3],
        a: [f64; 3],
        b: [f64; 3],
    },
    Polygon {
        pts: Vec<[f64; 3]>,
    },
    Cpolygon {
        pts: Vec<[f64; 4]>,
    },
    Net {
        rows: Vec<Vec<[f64; 3]>>,
    },
    Rotation {
        p: [f64; 3],
        v: [f64; 3],
        al: f64,
    },
    Transform {
        o1: [f64; 3],
        x1: [f64; 3],
        y1: [f64; 3],
        z1: [f64; 3],
        o2: [f64; 3],
        x2: [f64; 3],
        y2: [f64; 3],
        z2: [f64; 3],
    },
    CptOps {
        pw: [f64; 4],
        qw: [f64; 4],
        rw: [f64; 4],
        plane: Pl,
    },
}

const VECTOR_OPS_LAYOUT: &[(&str, usize)] = &[
    ("vector_dot", 1),
    ("vector_dot_ref", 1),
    ("vector_cross", 3),
    ("vector_cross_ref", 3),
    ("vector_scale", 3),
    ("vector_scale_ref", 3),
    ("vector_reverse", 3),
    ("vector_reverse_in_place", 3),
    ("vector_sum", 3),
    ("vector_combine", 3),
    ("vector_diff", 3),
    ("vector_diff_cpts", 3),
    ("vector_dir", 3),
    ("vector_create", 3),
    ("vector_copy", 3),
    ("vector_magnitude", 1),
    ("vector_magnitude_ref", 1),
    ("vector_pt_along_vector", 3),
    ("vector_normalize", 5),
    ("vector_normalize_ref", 4),
    ("vectors_cos_angle", 2),
    ("vectors_angle", 2),
    ("dist_pt_pt", 1),
    ("dist_sq_pt_pt", 1),
    ("pts_are_equal_ab", 1),
    ("pts_are_equal_aa", 1),
    ("dist_cpt_cpt", 1),
    ("dist_cpt_cpt_homo", 1),
    ("dist_signed_pt_plane", 1),
    ("interval", 2),
    ("rectangle", 4),
    ("bbox", 7),
    ("line_get_data", 7),
    ("plane_get_data", 9),
];

/// Output layout `(key, doubles)` of each op, in shim order.
fn layout(i: &Inputs) -> &'static [(&'static str, usize)] {
    match i {
        Inputs::VectorOps { .. } => VECTOR_OPS_LAYOUT,
        Inputs::IsectLineLine { .. } => &[("status", 1), ("flag", 1), ("p", 3), ("t1_t2", 2)],
        Inputs::IsectLinePlane { .. } | Inputs::ProjectPtLine { .. } => {
            &[("status", 1), ("flag", 1), ("p", 3), ("t", 1)]
        }
        Inputs::PlaneOps { .. } => &[
            ("project_pt_plane", 4),
            ("dist_pt_inf_line", 2),
            ("plane_pt_normal_to_implicit", 5),
        ],
        Inputs::DistSeg { .. } => &[("dist_pt_line_seg", 2), ("dist_perp_pt_line_seg", 2)],
        Inputs::Polygon { .. } => &[("bbox", 7), ("dist_polygon", 1)],
        Inputs::Cpolygon { .. } => &[("dist_cpolygon", 1), ("n", 1)],
        Inputs::Net { .. } => &[("bbox", 7), ("n_m", 2)],
        Inputs::Rotation { .. } | Inputs::Transform { .. } => &[("status", 1), ("matrix", 16)],
        Inputs::CptOps { .. } => &[("cpt_reflect", 5), ("colinear_3d_4d", 2)],
    }
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

fn split(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let lay = layout(i);
    let total: usize = lay.iter().map(|(_, n)| n).sum();
    if flat.len() != total {
        return Err(harness(format!(
            "record has {} values, layout has {total}",
            flat.len()
        )));
    }
    let mut out = Output::new();
    let mut at = 0usize;
    for (key, n) in lay {
        let part = flat.get(at..at + n).ok_or_else(|| harness("layout"))?;
        out = out.values(key, part.to_vec());
        at += n;
    }
    Ok(out)
}

// ---- C++ side ---------------------------------------------------------------------

fn line_flat(l: &Line, o: &mut Vec<f64>) {
    o.extend_from_slice(&l.p);
    o.extend_from_slice(&l.v);
    o.push(if l.bounded { 1.0 } else { 0.0 });
}

fn plane_flat(p: &Pl, o: &mut Vec<f64>) {
    o.extend_from_slice(&p.p);
    o.extend_from_slice(&p.n);
}

fn cpp_input(i: &Inputs) -> (Op, Vec<f64>) {
    let mut o = Vec::new();
    let op = match i {
        Inputs::VectorOps {
            a,
            b,
            alpha,
            beta,
            pw,
            qw,
        } => {
            o.extend_from_slice(a);
            o.extend_from_slice(b);
            o.push(*alpha);
            o.push(*beta);
            o.extend_from_slice(pw);
            o.extend_from_slice(qw);
            Op::VectorOps
        }
        Inputs::IsectLineLine { l1, l2 } => {
            line_flat(l1, &mut o);
            line_flat(l2, &mut o);
            Op::IsectLineLine
        }
        Inputs::IsectLinePlane { l, plane } => {
            line_flat(l, &mut o);
            plane_flat(plane, &mut o);
            Op::IsectLinePlane
        }
        Inputs::ProjectPtLine { l, pt } => {
            line_flat(l, &mut o);
            o.extend_from_slice(pt);
            Op::ProjectPtLine
        }
        Inputs::PlaneOps { plane, pt, bounded } => {
            plane_flat(plane, &mut o);
            o.extend_from_slice(pt);
            o.push(if *bounded { 1.0 } else { 0.0 });
            Op::PlaneOps
        }
        Inputs::DistSeg { p, a, b } => {
            o.extend_from_slice(p);
            o.extend_from_slice(a);
            o.extend_from_slice(b);
            Op::DistSeg
        }
        Inputs::Polygon { pts } => {
            o.extend(pts.iter().flatten());
            Op::Polygon
        }
        Inputs::Cpolygon { pts } => {
            o.extend(pts.iter().flatten());
            Op::CPolygon
        }
        Inputs::Net { rows } => {
            o.push(rows.len() as f64);
            o.push(rows.first().map_or(0, Vec::len) as f64);
            o.extend(rows.iter().flatten().flatten());
            Op::Net
        }
        Inputs::Rotation { p, v, al } => {
            o.extend_from_slice(p);
            o.extend_from_slice(v);
            o.push(*al);
            Op::Rotation
        }
        Inputs::Transform {
            o1,
            x1,
            y1,
            z1,
            o2,
            x2,
            y2,
            z2,
        } => {
            for v in [o1, x1, y1, z1, o2, x2, y2, z2] {
                o.extend_from_slice(v);
            }
            Op::Transform
        }
        Inputs::CptOps {
            pw,
            qw,
            rw,
            plane,
        } => {
            o.extend_from_slice(pw);
            o.extend_from_slice(qw);
            o.extend_from_slice(rw);
            plane_flat(plane, &mut o);
            Op::CptOps
        }
    };
    (op, o)
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    if let Inputs::Net { rows } = i {
        let cols = rows.first().map_or(0, Vec::len);
        if rows.iter().any(|r| r.len() != cols) {
            return Err(harness("net rows differ in length"));
        }
    }
    let (op, input) = cpp_input(i);
    match ffi::eval(op, &input) {
        Ok(flat) => split(i, &flat),
        Err(FfiError::InvalidArgument(m)) => Err(harness(format!("ffi rejected: {m}"))),
        Err(e) => Err(harness(format!("shim failure: {e}"))),
    }
}

// ---- Rust side --------------------------------------------------------------------

fn c4(a: [f64; 4]) -> CPoint {
    CPoint::new(a[0], a[1], a[2], a[3])
}

fn line(l: &Line) -> LineSeg {
    LineSeg::new(v3(l.p), v3(l.v), l.bounded)
}

fn plane(p: &Pl) -> Plane {
    Plane::new(v3(p.p), v3(p.n))
}

fn push(o: &mut Vec<f64>, p: Vec3) {
    o.extend_from_slice(&p.to_array());
}

fn nan(o: &mut Vec<f64>, k: usize) {
    o.extend(std::iter::repeat_n(f64::NAN, k));
}

/// `0` for `Ok`, the `NL_*_ERR` number for an NLib error, `-1` otherwise.
fn status<T>(r: &SolidResult<T>) -> f64 {
    match r {
        Ok(_) => 0.0,
        Err(SolidError::Nlib { code, .. }) => f64::from(code.code()),
        Err(_) => -1.0,
    }
}

fn push_box(o: &mut Vec<f64>, b: &MinMaxBox) {
    o.extend_from_slice(&[b.xl, b.xr, b.yb, b.yt, b.zn, b.zf, b.diagonal()]);
}

fn push_lp(o: &mut Vec<f64>, r: &SolidResult<LinePlaneIsect>) {
    o.push(status(r));
    match r {
        Err(_) => nan(o, 5),
        Ok(LinePlaneIsect::Parallel) => {
            o.push(0.0);
            nan(o, 4);
        }
        Ok(LinePlaneIsect::OutOfBounds { t }) => {
            o.push(0.0);
            nan(o, 3);
            o.push(*t);
        }
        Ok(LinePlaneIsect::Hit { p, t }) => {
            o.push(1.0);
            push(o, *p);
            o.push(*t);
        }
    }
}

fn push_matrix(o: &mut Vec<f64>, r: &SolidResult<RealMatrix>) -> SolidResult<()> {
    o.push(status(r));
    match r {
        Ok(m) => {
            for i in 0..4 {
                for j in 0..4 {
                    o.push(m.get(i, j)?);
                }
            }
        }
        Err(_) => nan(o, 16),
    }
    Ok(())
}

/// `N_PtsAreEqual` is `==` on `Vec3` (false for NaN, even against itself).
fn pts_are_equal(p: Vec3, q: Vec3) -> bool {
    p == q
}

#[allow(clippy::too_many_lines)]
fn rust_flat(i: &Inputs) -> SolidResult<Vec<f64>> {
    let mut o = Vec::new();
    match i {
        Inputs::VectorOps {
            a,
            b,
            alpha,
            beta,
            pw,
            qw,
        } => {
            let (a, b, alpha, beta) = (v3(*a), v3(*b), *alpha, *beta);
            let (pw, qw) = (c4(*pw), c4(*qw));
            o.push(a.dot(b));
            o.push(a.dot(b));
            push(&mut o, a.cross(b));
            push(&mut o, a.cross(b));
            push(&mut o, alpha * a);
            push(&mut o, a * alpha);
            push(&mut o, -a);
            push(&mut o, -a);
            push(&mut o, a + b);
            push(&mut o, combine2_pts(alpha, a, beta, b));
            push(&mut o, a - b);
            push(&mut o, vector_diff_cpts(pw, qw));
            push(&mut o, b - a);
            push(&mut o, Vec3::new(alpha, beta, a.z));
            push(&mut o, b);
            o.push(a.length());
            o.push(b.length());
            push(&mut o, pt_along_vector(a, alpha, b));
            let r = normalize(a);
            o.push(status(&r));
            match r {
                Ok((u, m)) => {
                    push(&mut o, u);
                    o.push(m);
                }
                Err(_) => nan(&mut o, 4),
            }
            let r = normalize(b);
            o.push(status(&r));
            match r {
                Ok((u, _)) => push(&mut o, u),
                Err(_) => nan(&mut o, 3),
            }
            for r in [vectors_cos_angle(a, b), vectors_angle(a, b)] {
                o.push(status(&r));
                o.push(r.unwrap_or(f64::NAN));
            }
            o.push(dist_pt_pt(a, b));
            o.push(dist_sq_pt_pt(a, b));
            o.push(flag(pts_are_equal(a, b)));
            o.push(flag(pts_are_equal(a, a)));
            o.push(dist_cpt_cpt(pw, qw));
            o.push(dist_cpt_cpt_homo(pw, qw));
            o.push(dist_signed_pt_plane(a.x, a.y, a.z, alpha, b));
            let iv = Interval::new(alpha, beta);
            o.extend_from_slice(&[iv.ul, iv.ur]);
            let r = Rectangle::new(a.x, a.y, b.x, b.y);
            o.extend_from_slice(&[r.ul, r.ur, r.vb, r.vt]);
            push_box(&mut o, &MinMaxBox::new(a.x, b.x, a.y, b.y, a.z, b.z));
            let mut l = LineSeg::new(a, b, true);
            l.bounded = false;
            push(&mut o, l.p);
            push(&mut o, l.v);
            o.push(flag(l.bounded));
            let pl = Plane::new(a, b);
            push(&mut o, pl.p);
            push(&mut o, pl.n);
            push(&mut o, pl.n);
        }
        Inputs::IsectLineLine { l1, l2 } => {
            let r = isect_line_line(line(l1), line(l2));
            o.push(status(&r));
            match r {
                Err(_) => nan(&mut o, 6),
                Ok(None) => {
                    o.push(0.0);
                    nan(&mut o, 5);
                }
                Ok(Some(h)) => {
                    o.push(1.0);
                    push(&mut o, h.p);
                    o.push(h.t1);
                    o.push(h.t2);
                }
            }
        }
        Inputs::IsectLinePlane { l, plane: pl } => {
            push_lp(&mut o, &isect_line_plane(line(l), plane(pl)));
        }
        Inputs::ProjectPtLine { l, pt } => {
            push_lp(&mut o, &project_pt_line(line(l), v3(*pt)));
        }
        Inputs::PlaneOps {
            plane: pl,
            pt,
            bounded,
        } => {
            let (pln, p) = (plane(pl), v3(*pt));
            let r = project_pt_plane(pln, p);
            o.push(status(&r));
            match r {
                Ok(q) => push(&mut o, q),
                Err(_) => nan(&mut o, 3),
            }
            let r = dist_pt_inf_line(LineSeg::new(pln.p, pln.n, *bounded), p);
            o.push(status(&r));
            o.push(r.unwrap_or(f64::NAN));
            let r = plane_pt_normal_to_implicit(p, pln.n);
            o.push(status(&r));
            match r {
                Ok(c) => o.extend_from_slice(&c),
                Err(_) => nan(&mut o, 4),
            }
        }
        Inputs::DistSeg { p, a, b } => {
            let (p, a, b) = (v3(*p), v3(*a), v3(*b));
            let r = dist_pt_line_seg(p, a, b);
            o.push(status(&r));
            o.push(r.unwrap_or(f64::NAN));
            let r = dist_perp_pt_line_seg(p, a, b);
            o.push(status(&r));
            o.push(match r {
                Ok(Some(d)) => d,
                Ok(None) => -1.0,
                Err(_) => f64::NAN,
            });
        }
        Inputs::Polygon { pts } => {
            let p: Vec<Vec3> = pts.iter().copied().map(v3).collect();
            push_box(&mut o, &polygon_bbox(&p)?);
            o.push(dist_polygon(&p));
        }
        Inputs::Cpolygon { pts } => {
            let p: Vec<CPoint> = pts.iter().copied().map(c4).collect();
            o.push(dist_cpolygon(&p));
            o.push(p.len() as f64 - 1.0);
        }
        Inputs::Net { rows } => {
            let net: Vec<Vec<Vec3>> = rows
                .iter()
                .map(|r| r.iter().copied().map(v3).collect())
                .collect();
            push_box(&mut o, &net_bbox(&net)?);
            o.push(rows.len() as f64 - 1.0);
            o.push(rows.first().map_or(0, Vec::len) as f64 - 1.0);
        }
        Inputs::Rotation { p, v, al } => {
            push_matrix(
                &mut o,
                &rotation_matrix_about_axis(v3(*p), v3(*v), *al),
            )?;
        }
        Inputs::Transform {
            o1,
            x1,
            y1,
            z1,
            o2,
            x2,
            y2,
            z2,
        } => {
            push_matrix(
                &mut o,
                &transform_matrix_from_axes(
                    Frame::new(v3(*o1), v3(*x1), v3(*y1), v3(*z1)),
                    Frame::new(v3(*o2), v3(*x2), v3(*y2), v3(*z2)),
                ),
            )?;
        }
        Inputs::CptOps {
            pw,
            qw,
            rw,
            plane: pl,
        } => {
            let (pw, qw, rw) = (c4(*pw), c4(*qw), c4(*rw));
            let r = cpt_reflect(pw, plane(pl));
            o.push(status(&r));
            match r {
                Ok(q) => o.extend_from_slice(&q.to_array()),
                Err(_) => nan(&mut o, 4),
            }
            o.push(flag(cpts_are_colinear(pw, qw, rw, ColinearDim::Three)));
            o.push(flag(cpts_are_colinear(pw, qw, rw, ColinearDim::Four)));
        }
    }
    Ok(o)
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    match rust_flat(i) {
        Ok(flat) => split(i, &flat),
        Err(e) => Ok(Output::unported(&e).unwrap_or_else(|| Output::err(format!("rust: {e}")))),
    }
}

// ---- generator --------------------------------------------------------------------

/// A coordinate: mostly uniform, sometimes integral, zero, tiny or large.
fn coord(rng: &mut Rng) -> f64 {
    let r = rng.unit();
    if r < 0.6 {
        rng.uniform(-10.0, 10.0)
    } else if r < 0.75 {
        rng.range_i64(-5, 5) as f64
    } else if r < 0.82 {
        0.0
    } else if r < 0.9 {
        rng.uniform(-1.0, 1.0) * 1e-9
    } else {
        rng.uniform(-1.0, 1.0) * 1e6
    }
}

/// A coordinate that is sometimes extreme (overflowing products, subnormal results).
fn wild(rng: &mut Rng) -> f64 {
    let r = rng.unit();
    if r < 0.9 {
        coord(rng)
    } else if r < 0.95 {
        rng.uniform(-1.0, 1.0) * 1e200
    } else {
        rng.uniform(-1.0, 1.0) * 1e-160
    }
}

fn pt(rng: &mut Rng) -> [f64; 3] {
    [coord(rng), coord(rng), coord(rng)]
}

fn wild_pt(rng: &mut Rng) -> [f64; 3] {
    [wild(rng), wild(rng), wild(rng)]
}

/// A direction: random, an axis, zero, or tiny.
fn dir(rng: &mut Rng) -> [f64; 3] {
    let r = rng.unit();
    if r < 0.65 {
        pt(rng)
    } else if r < 0.85 {
        let mut d = [0.0; 3];
        if let Some(x) = d.get_mut(rng.below(3) as usize) {
            *x = if rng.chance(0.5) { 1.0 } else { -2.5 };
        }
        d
    } else if r < 0.92 {
        [0.0; 3]
    } else {
        let s = if rng.chance(0.5) { 1e-160 } else { 1e-200 };
        [
            rng.uniform(-1.0, 1.0) * s,
            rng.uniform(-1.0, 1.0) * s,
            rng.uniform(-1.0, 1.0) * s,
        ]
    }
}

/// `p + s*d` (a point on a given line), for near-degenerate configurations.
fn along(p: [f64; 3], s: f64, d: [f64; 3]) -> [f64; 3] {
    [p[0] + s * d[0], p[1] + s * d[1], p[2] + s * d[2]]
}

/// A control point with weight `w` (or `NOW`) and sometimes `NOZ`; `positive`
/// restricts weights to `(0.1, 5)` / `NOW`.
fn cpt(rng: &mut Rng, positive: bool) -> [f64; 4] {
    let z = if rng.chance(0.2) { NOZ } else { coord(rng) };
    let w = if rng.chance(0.35) {
        NOW
    } else if positive || rng.chance(0.85) {
        rng.uniform(0.1, 5.0)
    } else if rng.chance(0.5) {
        0.0
    } else {
        rng.uniform(-3.0, -0.1)
    };
    [coord(rng), coord(rng), z, w]
}

fn gen_line(rng: &mut Rng) -> Line {
    Line {
        p: wild_pt(rng),
        v: dir(rng),
        bounded: rng.chance(0.5),
    }
}

fn gen_plane(rng: &mut Rng) -> Pl {
    Pl {
        p: pt(rng),
        n: dir(rng),
    }
}

fn gen_vector_ops(rng: &mut Rng) -> Inputs {
    let a = wild_pt(rng);
    let b = match rng.below(5) {
        0 => a,
        1 => [-a[0] * 2.0, -a[1] * 2.0, -a[2] * 2.0],
        2 => dir(rng),
        _ => wild_pt(rng),
    };
    let a = if rng.chance(0.08) { [0.0; 3] } else { a };
    let pw = cpt(rng, false);
    let qw = if rng.chance(0.15) { pw } else { cpt(rng, false) };
    Inputs::VectorOps {
        a,
        b,
        alpha: wild(rng),
        beta: wild(rng),
        pw,
        qw,
    }
}

fn gen_isect_ll(rng: &mut Rng) -> Inputs {
    let l1 = gen_line(rng);
    let mut l2 = gen_line(rng);
    match rng.below(6) {
        // Parallel (scaled direction), possibly coincident.
        0 => {
            l2.v = [l1.v[0] * -3.0, l1.v[1] * -3.0, l1.v[2] * -3.0];
            if rng.chance(0.5) {
                l2.p = along(l1.p, rng.uniform(-2.0, 2.0), l1.v);
            }
        }
        // Truly intersecting: l2 passes through a point of l1.
        1 | 2 => {
            let x = along(l1.p, rng.uniform(-0.5, 1.5), l1.v);
            let s = rng.uniform(-0.5, 1.5);
            l2.p = along(x, -s, l2.v);
        }
        // Nearly parallel.
        3 => {
            l2.v = [
                l1.v[0] + rng.uniform(-1.0, 1.0) * 1e-7,
                l1.v[1],
                l1.v[2] + rng.uniform(-1.0, 1.0) * 1e-7,
            ];
        }
        _ => {}
    }
    Inputs::IsectLineLine { l1, l2 }
}

fn gen_isect_lp(rng: &mut Rng) -> Inputs {
    let l = gen_line(rng);
    let mut plane = gen_plane(rng);
    match rng.below(5) {
        // Plane containing the line direction (parallel).
        0 => {
            let n = Vec3::new(l.v[0], l.v[1], l.v[2]).cross(Vec3::new(
                rng.uniform(-1.0, 1.0),
                rng.uniform(-1.0, 1.0),
                rng.uniform(-1.0, 1.0),
            ));
            plane.n = n.to_array();
        }
        // Plane through a point of the segment.
        1 => plane.p = along(l.p, rng.uniform(-0.2, 1.2), l.v),
        // Normal along the line.
        2 => plane.n = l.v,
        _ => {}
    }
    Inputs::IsectLinePlane { l, plane }
}

fn gen_net(rng: &mut Rng) -> Inputs {
    let nr = 1 + rng.below(5) as usize;
    let nc = 1 + rng.below(5) as usize;
    let rows = (0..nr)
        .map(|_| (0..nc).map(|_| wild_pt(rng)).collect())
        .collect();
    Inputs::Net { rows }
}

fn angle(rng: &mut Rng) -> f64 {
    match rng.below(6) {
        0 => *rng
            .choose(&[0.0, 90.0, 180.0, 270.0, 360.0, -90.0, 45.0, 30.0])
            .unwrap_or(&0.0),
        1 => rng.uniform(-720.0, 720.0),
        2 => rng.uniform(-1.0, 1.0) * 1e-9,
        _ => rng.uniform(-180.0, 180.0),
    }
}

/// An orthonormal-ish frame: a rotated standard frame, or random vectors.
fn frame(rng: &mut Rng) -> [[f64; 3]; 3] {
    if rng.chance(0.3) {
        return [dir(rng), pt(rng), pt(rng)];
    }
    let ax = Vec3::new(
        rng.uniform(-1.0, 1.0),
        rng.uniform(-1.0, 1.0),
        rng.uniform(-1.0, 1.0),
    );
    let Ok(m) = rotation_matrix_about_axis(Vec3::ZERO, ax, rng.uniform(-180.0, 180.0)) else {
        return [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    };
    let col = |j: usize| {
        [
            m.get(0, j).unwrap_or(0.0),
            m.get(1, j).unwrap_or(0.0),
            m.get(2, j).unwrap_or(0.0),
        ]
    };
    [col(0), col(1), col(2)]
}

fn gen_case(rng: &mut Rng) -> Inputs {
    match rng.below(12) {
        0 => gen_vector_ops(rng),
        1 => gen_isect_ll(rng),
        2 => gen_isect_lp(rng),
        3 => {
            let l = gen_line(rng);
            let pt = if rng.chance(0.2) {
                along(l.p, rng.uniform(-1.0, 2.0), l.v)
            } else {
                wild_pt(rng)
            };
            Inputs::ProjectPtLine { l, pt }
        }
        4 => {
            let plane = gen_plane(rng);
            let pt = if rng.chance(0.15) { plane.p } else { wild_pt(rng) };
            Inputs::PlaneOps {
                plane,
                pt,
                bounded: rng.chance(0.5),
            }
        }
        5 => {
            let a = wild_pt(rng);
            let b = match rng.below(6) {
                0 => a,
                1 => along(a, 1.0, [rng.uniform(-1.0, 1.0) * 5e-8, 0.0, 0.0]),
                _ => wild_pt(rng),
            };
            let p = match rng.below(5) {
                0 => along(a, rng.uniform(-0.5, 1.5), [b[0] - a[0], b[1] - a[1], b[2] - a[2]]),
                1 => a,
                _ => wild_pt(rng),
            };
            Inputs::DistSeg { p, a, b }
        }
        6 => {
            let n = 1 + rng.below(12) as usize;
            Inputs::Polygon {
                pts: (0..n).map(|_| wild_pt(rng)).collect(),
            }
        }
        7 => {
            let n = rng.below(10) as usize;
            Inputs::Cpolygon {
                pts: (0..n).map(|_| cpt(rng, false)).collect(),
            }
        }
        8 => gen_net(rng),
        9 => Inputs::Rotation {
            p: wild_pt(rng),
            v: dir(rng),
            al: angle(rng),
        },
        10 => {
            let [x1, y1, z1] = frame(rng);
            let [x2, y2, z2] = frame(rng);
            Inputs::Transform {
                o1: wild_pt(rng),
                x1,
                y1,
                z1,
                o2: wild_pt(rng),
                x2,
                y2,
                z2,
            }
        }
        _ => {
            // Positive weights and moderate coordinates only: upstream reads an
            // unwritten distance when the 3D collinearity test meets NaN.
            let pw = cpt(rng, true);
            let mut qw = cpt(rng, true);
            let mut rw = cpt(rng, true);
            if rng.chance(0.4) {
                // Collinear (or nearly) Euclidean images.
                let (p, w) = (CPoint::new(pw[0], pw[1], pw[2], pw[3]).to_euclid(), pw[3]);
                let d = Vec3::new(coord(rng), coord(rng), coord(rng));
                let s = rng.uniform(-2.0, 2.0);
                let q = p + d * s;
                let r = p + d * rng.uniform(-2.0, 2.0);
                let wq = if rng.chance(0.5) { w } else { NOW };
                let mk = |x: Vec3, w: f64| {
                    if w == NOW {
                        [x.x, x.y, x.z, NOW]
                    } else {
                        [x.x * w, x.y * w, x.z * w, w]
                    }
                };
                qw = mk(q, wq);
                rw = mk(r, NOW);
                if rng.chance(0.3) {
                    qw[1] += rng.uniform(-1.0, 1.0) * 1e-7;
                }
            }
            let plane = if rng.chance(0.3) {
                Pl {
                    p: pt(rng),
                    n: [coord(rng), coord(rng), rng.uniform(-1.0, 1.0) * 1e-6],
                }
            } else {
                gen_plane(rng)
            };
            Inputs::CptOps { pw, qw, rw, plane }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.geom"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_case(rng)).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}
