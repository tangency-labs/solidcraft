//! Runner for unit `geom.pseudo_box`: non-orthogonal bounding boxes.
//!
//! C++: `solidcore_ffi::units::geom_pseudo_box` (upstream `source/SMLib/src/SmPseudoBox.cpp`).
//! Rust: `solidcore::geom::pseudo_box`.
//!
//! A case is a program of commands on four box registers (all start as
//! `SmPseudoBox()` / [`PseudoBox::new`]); both sides run it and every command's
//! output becomes its own key `cNNN_<name>`.
//!
//! | `cmd`                  | Rust                                   | upstream |
//! |------------------------|----------------------------------------|----------|
//! | `init`, `init_basis`   | `init`, `init_basis`                   | `Init`, `InitBasis` |
//! | `from_extent`          | `from_extent3d`                        | `SmPseudoBox(const SmExtent3d&)` |
//! | `assign_extent`        | `assign_extent3d`                      | `operator=(const SmExtent3d&)` |
//! | `set_basis`            | `set_basis`                            | `SetBasis(b1, b2, b3)` |
//! | `set_intervals`        | `set_intervals`                        | `SetIntervals` |
//! | `set_interval`         | `set_interval`                         | `SetInterval` |
//! | `set_unbounded`        | `set_unbounded`                        | `SetUnbounded` |
//! | `add`, `add_points`    | `add_point`                            | `AddPoint3d` |
//! | `expand`               | `expand_absolute`                      | `ExpandAbsolute` |
//! | `dump`                 | `basis_vector`, `interval`             | `GetBasis(l)`, `GetInterval(l)` |
//! | `contains`             | `contains_point`                       | `ContainsPoint3d` |
//! | `eval`                 | `evaluate`                             | `Evaluate` |
//! | `invert`               | `invert_absolute`                      | `InvertAbsolute` |
//! | `union`                | `union_into`                           | `Union` |
//! | `distance`             | `distance_to_point`                    | `DistanceToPoint` |
//! | `line`                 | `intersect_line`                       | `IntersectLine` |
//! | `neg_volume`           | `has_negative_volume`                  | `HasNegativeVolume` |
//! | `measures`             | `max_dimension`, `xy_area`, `volume`   | `GetMaxDimension`, `GetXYArea`, `GetVolume` |
//! | `point_sized`, `linear`, `planar` | `is_point_sized`, `is_linear`, `is_planar` | `IsPointSized`, `IsLinear`, `IsPlanar` |
//! | `disjoint`             | `are_disjoint`                         | `AreDisjoint` |
//! | `project_disjoint`     | `are_project_disjoint`                 | `AreProjectDisjoint` |
//! | `contained_by`         | `is_contained_by`                      | `IsContainedBy` |
//! | `classify`             | `classify_point`                       | `ClassifyPoint3d` |
//! | `planes`               | `calc_planes`                          | `CalcPlanes` |
//! | `bounded`              | `is_bounded`, `boundary_types`         | `IsBounded` |
//! | `axis_aligned`, `aligned` | `is_axis_aligned`, `are_aligned`    | `IsAxisAligned`, `AreAligned` |
//! | `corners`, `edges`     | `corners`, `edges`                     | `GetCorners`, `GetEdges` |
//! | `defined`              | `assert_defined`                       | `AssertDefined` |
//! | `project`              | `project_to_plane_into`                | `ProjectToPlane` |
//! | `assign`               | `Copy`                                 | `operator=` |
//! | `from_basis_intervals` | `from_basis_intervals`                 | `SmPseudoBox(xDir, .., zIvl)` |
//! | `circumscribe`         | `Extent3d::from_pseudo_box`            | `SmExtent3d(const SmPseudoBox&)` |
//! | `scaled_zero`          | `ScaledZero::scaled_zero`              | `SmTol::GetScaledZero(const SmPseudoBox&)` |
//!
//! Statuses are compared as ints (0 = success, 1001 = `SM_ERR`, 1007 =
//! `SM_ERR_INVALID_INPUT`). Every run has at least [`MIN_GENERATED`] generated
//! cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::pseudo_box::{LineHit, Projection, PseudoBox};
use solidcore::geom::tol::ScaledZero;
use solidcore::geom::vector3d::Vec3;
use solidcore::math::INFINITE_PARAMETER;
use solidcore::SolidError;
use solidcore_ffi::units::geom_pseudo_box::{self as ffi, Cmd as FCmd, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::flag;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `geom.pseudo_box` runner.
pub struct Runner;

type V3 = [f64; 3];

/// A projection as case data.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Proj {
    /// `parallel`, `perspective` or `rotation`.
    kind: ProjKind,
    point: V3,
    vec: V3,
    #[serde(default)]
    aux: V3,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProjKind {
    Parallel,
    Perspective,
    Rotation,
}

impl Proj {
    fn flat(&self) -> Vec<f64> {
        let t = match self.kind {
            ProjKind::Parallel => 0.0,
            ProjKind::Perspective => 1.0,
            ProjKind::Rotation => 2.0,
        };
        let mut v = vec![t];
        v.extend(self.point);
        v.extend(self.vec);
        v.extend(self.aux);
        v
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum Cmd {
    Init { r: usize },
    InitBasis { r: usize },
    FromExtent { r: usize, e: [f64; 6] },
    AssignExtent { r: usize, e: [f64; 6] },
    SetBasis { r: usize, b: [V3; 3] },
    SetIntervals { r: usize, i: [[f64; 2]; 3] },
    SetInterval { r: usize, idx: usize, ivl: [f64; 2] },
    SetUnbounded { r: usize },
    Add { r: usize, p: V3 },
    AddPoints { r: usize, pts: Vec<V3> },
    Expand { r: usize, d: f64 },
    Dump { r: usize },
    Contains { r: usize, p: V3, tol: f64 },
    Eval { r: usize, n: V3 },
    Invert { r: usize, p: V3 },
    Union { r: usize, s: usize, t: usize, b: [V3; 3] },
    Distance { r: usize, p: V3 },
    Line { r: usize, p: V3, v: V3 },
    NegVolume { r: usize },
    Measures { r: usize },
    PointSized { r: usize, tol: f64 },
    Linear { r: usize, tol: f64 },
    Planar { r: usize, tol: f64 },
    Disjoint { r: usize, s: usize, #[serde(default, skip_serializing_if = "Option::is_none")] proj: Option<V3> },
    ProjectDisjoint { r: usize, s: usize, proj: Proj },
    ContainedBy { r: usize, s: usize, tol: f64 },
    Classify { r: usize, p: V3, tol: f64 },
    Planes { r: usize },
    Bounded { r: usize },
    AxisAligned { r: usize },
    Aligned { r: usize, s: usize },
    Corners { r: usize },
    Edges { r: usize },
    Defined { r: usize },
    Project { r: usize, t: usize, proj: Proj },
    Assign { t: usize, r: usize },
    FromBasisIntervals { r: usize, b: [V3; 3], i: [[f64; 2]; 3] },
    Circumscribe { r: usize },
    ScaledZero { r: usize },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    prog: Vec<Cmd>,
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

fn c(op: Op, regs: &[usize], args: Vec<f64>) -> FCmd {
    FCmd {
        op,
        regs: regs.to_vec(),
        args,
    }
}

fn flat3(b: &[V3; 3]) -> Vec<f64> {
    b.iter().flatten().copied().collect()
}

fn flat_ivls(i: &[[f64; 2]; 3]) -> Vec<f64> {
    i.iter().flatten().copied().collect()
}

/// Expands the case program into shim commands (`add_points` becomes `add`s).
fn lower(prog: &[Cmd]) -> Result<Vec<FCmd>, HarnessError> {
    let mut out = Vec::with_capacity(prog.len());
    for cmd in prog {
        match cmd.clone() {
            Cmd::Init { r } => out.push(c(Op::Init, &[r], vec![])),
            Cmd::InitBasis { r } => out.push(c(Op::InitBasis, &[r], vec![])),
            Cmd::FromExtent { r, e } => out.push(c(Op::FromExtent, &[r], e.to_vec())),
            Cmd::AssignExtent { r, e } => out.push(c(Op::AssignExtent, &[r], e.to_vec())),
            Cmd::SetBasis { r, b } => out.push(c(Op::SetBasis, &[r], flat3(&b))),
            Cmd::SetIntervals { r, i } => out.push(c(Op::SetIntervals, &[r], flat_ivls(&i))),
            Cmd::SetInterval { r, idx, ivl } => {
                out.push(c(Op::SetInterval, &[r], vec![idx as f64, ivl[0], ivl[1]]));
            }
            Cmd::SetUnbounded { r } => out.push(c(Op::SetUnbounded, &[r], vec![])),
            Cmd::Add { r, p } => out.push(c(Op::AddPoint, &[r], p.to_vec())),
            Cmd::AddPoints { r, pts } => {
                for p in pts {
                    out.push(c(Op::AddPoint, &[r], p.to_vec()));
                }
            }
            Cmd::Expand { r, d } => out.push(c(Op::Expand, &[r], vec![d])),
            Cmd::Dump { r } => out.push(c(Op::Dump, &[r], vec![])),
            Cmd::Contains { r, p, tol } => {
                out.push(c(Op::Contains, &[r], vec![p[0], p[1], p[2], tol]));
            }
            Cmd::Eval { r, n } => out.push(c(Op::Evaluate, &[r], n.to_vec())),
            Cmd::Invert { r, p } => out.push(c(Op::InvertAbsolute, &[r], p.to_vec())),
            Cmd::Union { r, s, t, b } => out.push(c(Op::Union, &[r, s, t], flat3(&b))),
            Cmd::Distance { r, p } => out.push(c(Op::Distance, &[r], p.to_vec())),
            Cmd::Line { r, p, v } => {
                out.push(c(Op::IntersectLine, &[r], vec![p[0], p[1], p[2], v[0], v[1], v[2]]));
            }
            Cmd::NegVolume { r } => out.push(c(Op::NegativeVolume, &[r], vec![])),
            Cmd::Measures { r } => out.push(c(Op::Measures, &[r], vec![])),
            Cmd::PointSized { r, tol } => out.push(c(Op::PointSized, &[r], vec![tol])),
            Cmd::Linear { r, tol } => out.push(c(Op::Linear, &[r], vec![tol])),
            Cmd::Planar { r, tol } => out.push(c(Op::Planar, &[r], vec![tol])),
            Cmd::Disjoint { r, s, proj } => {
                let v = proj.unwrap_or([0.0; 3]);
                let has = if proj.is_some() { 1.0 } else { 0.0 };
                out.push(c(Op::AreDisjoint, &[r, s], vec![has, v[0], v[1], v[2]]));
            }
            Cmd::ProjectDisjoint { r, s, proj } => {
                out.push(c(Op::ProjectDisjoint, &[r, s], proj.flat()));
            }
            Cmd::ContainedBy { r, s, tol } => out.push(c(Op::ContainedBy, &[r, s], vec![tol])),
            Cmd::Classify { r, p, tol } => {
                out.push(c(Op::Classify, &[r], vec![p[0], p[1], p[2], tol]));
            }
            Cmd::Planes { r } => out.push(c(Op::Planes, &[r], vec![])),
            Cmd::Bounded { r } => out.push(c(Op::Bounded, &[r], vec![])),
            Cmd::AxisAligned { r } => out.push(c(Op::AxisAligned, &[r], vec![])),
            Cmd::Aligned { r, s } => out.push(c(Op::AreAligned, &[r, s], vec![])),
            Cmd::Corners { r } => out.push(c(Op::Corners, &[r], vec![])),
            Cmd::Edges { r } => out.push(c(Op::Edges, &[r], vec![])),
            Cmd::Defined { r } => out.push(c(Op::AssertDefined, &[r], vec![])),
            Cmd::Project { r, t, proj } => out.push(c(Op::Project, &[r, t], proj.flat())),
            Cmd::Assign { t, r } => out.push(c(Op::Assign, &[t, r], vec![])),
            Cmd::FromBasisIntervals { r, b, i } => {
                let mut a = flat3(&b);
                a.extend(flat_ivls(&i));
                out.push(c(Op::FromBasisIntervals, &[r], a));
            }
            Cmd::Circumscribe { r } => out.push(c(Op::Circumscribe, &[r], vec![])),
            Cmd::ScaledZero { r } => out.push(c(Op::ScaledZero, &[r], vec![])),
        }
    }
    for (i, f) in out.iter().enumerate() {
        let (n_regs, n_args) = f.op.arity();
        if f.regs.len() != n_regs || f.args.len() != n_args {
            return Err(harness(format!("command {i}: bad operand count")));
        }
        if f.regs.iter().any(|&r| r >= ffi::REGISTERS) {
            return Err(harness(format!("command {i}: register out of range")));
        }
        if f.op == Op::SetInterval && f.args.first().is_none_or(|&x| x > 2.0) {
            return Err(harness(format!("command {i}: interval index out of range")));
        }
    }
    Ok(out)
}

/// Kind of an output slot.
#[derive(Clone, Copy)]
enum K {
    /// Floats.
    F,
    /// Integers (status, flags, enum values, counts).
    I,
}

/// Reads a flat result front to back into keyed outputs.
struct Cursor<'a> {
    rest: &'a [f64],
    out: Output,
}

impl Cursor<'_> {
    fn peek(&self) -> Option<f64> {
        self.rest.first().copied()
    }

    fn take(&mut self, key: &str, n: usize, k: K) -> Result<(), HarnessError> {
        let Some((head, tail)) = self.rest.split_at_checked(n) else {
            return Err(harness(format!("result too short for `{key}`")));
        };
        self.rest = tail;
        let out = core::mem::take(&mut self.out);
        self.out = match k {
            K::F => out.values(key, head.to_vec()),
            K::I => out.ints(key, head.iter().map(|&v| v as i64).collect::<Vec<_>>()),
        };
        Ok(())
    }

    /// An int slot; when it equals `when`, `n` more float values follow.
    fn take_then(
        &mut self,
        key: &str,
        when: f64,
        tail_key: &str,
        n: usize,
    ) -> Result<(), HarnessError> {
        let first = self.peek();
        self.take(key, 1, K::I)?;
        if first == Some(when) {
            self.take(tail_key, n, K::F)?;
        }
        Ok(())
    }
}

/// Splits the flat result into one key per output part of each command.
fn split(prog: &[FCmd], flat: &[f64]) -> Result<Output, HarnessError> {
    let mut cur = Cursor {
        rest: flat,
        out: Output::new(),
    };
    for (i, cmd) in prog.iter().enumerate() {
        let key = |name: &str| format!("c{i:03}_{name}");
        match cmd.op {
            Op::Init
            | Op::InitBasis
            | Op::FromExtent
            | Op::AssignExtent
            | Op::SetIntervals
            | Op::SetInterval
            | Op::SetUnbounded
            | Op::AddPoint
            | Op::Expand
            | Op::Union
            | Op::Assign
            | Op::FromBasisIntervals => {}
            Op::SetBasis => cur.take(&key("set_basis"), 1, K::I)?,
            Op::Dump => {
                cur.take(&key("basis"), 9, K::F)?;
                cur.take(&key("intervals"), 6, K::F)?;
            }
            Op::Contains => cur.take(&key("contains"), 1, K::I)?,
            Op::Evaluate => cur.take(&key("eval"), 3, K::F)?,
            Op::InvertAbsolute => cur.take_then(&key("invert_status"), 0.0, &key("invert"), 3)?,
            Op::Distance => cur.take(&key("distance"), 1, K::F)?,
            Op::IntersectLine => {
                let first = cur.peek();
                cur.take(&key("line_status"), 1, K::I)?;
                if first == Some(0.0) {
                    cur.take(&key("line_count"), 1, K::I)?;
                    cur.take(&key("line_t"), 2, K::F)?;
                }
            }
            Op::NegativeVolume => cur.take(&key("neg_volume"), 1, K::I)?,
            Op::Measures => cur.take(&key("measures"), 3, K::F)?,
            Op::PointSized => {
                cur.take(&key("point_sized"), 1, K::I)?;
                cur.take(&key("point_sized_out"), 4, K::F)?;
            }
            Op::Linear => {
                cur.take(&key("linear"), 1, K::I)?;
                cur.take(&key("linear_out"), 7, K::F)?;
            }
            Op::Planar => {
                cur.take(&key("planar"), 1, K::I)?;
                cur.take(&key("planar_out"), 7, K::F)?;
            }
            Op::AreDisjoint => cur.take(&key("disjoint"), 1, K::I)?,
            Op::ProjectDisjoint => cur.take(&key("project_disjoint"), 1, K::I)?,
            Op::ContainedBy => cur.take(&key("contained_by"), 1, K::I)?,
            Op::Classify => cur.take(&key("classify"), 3, K::I)?,
            Op::Planes => cur.take(&key("planes"), 36, K::F)?,
            Op::Bounded => cur.take(&key("bounded"), 4, K::I)?,
            Op::AxisAligned => {
                let first = cur.peek();
                cur.take(&key("axis_aligned"), 1, K::I)?;
                if first == Some(1.0) {
                    cur.take(&key("axis_map"), 3, K::I)?;
                }
            }
            Op::AreAligned => {
                let first = cur.peek();
                cur.take(&key("aligned"), 1, K::I)?;
                if first == Some(1.0) {
                    cur.take(&key("aligned_map"), 3, K::I)?;
                }
            }
            Op::Corners => cur.take(&key("corners"), 24, K::F)?,
            Op::Edges => cur.take(&key("edges"), 72, K::F)?,
            Op::AssertDefined => cur.take(&key("defined"), 1, K::I)?,
            Op::Project => cur.take(&key("project_status"), 1, K::I)?,
            Op::Circumscribe => cur.take(&key("circumscribe"), 6, K::F)?,
            Op::ScaledZero => cur.take(&key("scaled_zero"), 1, K::F)?,
        }
    }
    if !cur.rest.is_empty() {
        return Err(harness(format!("result too long by {}", cur.rest.len())));
    }
    Ok(cur.out)
}

// ---- Rust side -----------------------------------------------------------------------

fn put(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

/// The upstream status a Rust error stands for.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => 1007.0,
    }
}

fn v3_at(a: &[f64], i: usize) -> Result<Vec3, HarnessError> {
    match a.get(i..i + 3) {
        Some(&[x, y, z]) => Ok(Vec3::new(x, y, z)),
        _ => Err(harness("argument list too short")),
    }
}

fn f_at(a: &[f64], i: usize) -> Result<f64, HarnessError> {
    a.get(i).copied().ok_or_else(|| harness("argument list too short"))
}

fn ivl_at(a: &[f64], i: usize) -> Result<Extent1d, HarnessError> {
    Ok(Extent1d::from_min_max(f_at(a, i)?, f_at(a, i + 1)?))
}

fn proj_at(a: &[f64]) -> Result<Projection, HarnessError> {
    let point = v3_at(a, 1)?;
    let vec = v3_at(a, 4)?;
    let aux = v3_at(a, 7)?;
    match f_at(a, 0)? {
        0.0 => Ok(Projection::Parallel { point, normal: vec }),
        1.0 => Ok(Projection::Perspective {
            point,
            normal: vec,
            eye: aux,
        }),
        2.0 => Ok(Projection::Rotation {
            point,
            axis: vec,
            x_axis: aux,
        }),
        t => Err(harness(format!("bad projection type {t}"))),
    }
}

fn reg_at(reg: &[PseudoBox], regs: &[usize], k: usize) -> Result<PseudoBox, HarnessError> {
    regs.get(k)
        .and_then(|&r| reg.get(r))
        .copied()
        .ok_or_else(|| harness("register out of range"))
}

fn reg_mut<'a>(
    reg: &'a mut [PseudoBox],
    regs: &[usize],
    k: usize,
) -> Result<&'a mut PseudoBox, HarnessError> {
    regs.get(k)
        .and_then(|&r| reg.get_mut(r))
        .ok_or_else(|| harness("register out of range"))
}

fn put_map(o: &mut Vec<f64>, m: Option<[i32; 3]>) {
    match m {
        Some(m) => {
            o.push(1.0);
            o.extend(m.map(f64::from));
        }
        None => o.push(0.0),
    }
}

/// Runs the program on the Rust port, producing the shim's flat layout.
fn rust_flat(prog: &[FCmd]) -> Result<Vec<f64>, HarnessError> {
    let mut reg = [PseudoBox::new(); ffi::REGISTERS];
    let mut o = Vec::new();
    for cmd in prog {
        let a = cmd.args.as_slice();
        let rg = cmd.regs.as_slice();
        let x = reg_at(&reg, rg, 0)?;
        match cmd.op {
            Op::Init => reg_mut(&mut reg, rg, 0)?.init(),
            Op::InitBasis => reg_mut(&mut reg, rg, 0)?.init_basis(),
            Op::FromExtent | Op::AssignExtent => {
                let e = Extent3d::from_bounds(
                    f_at(a, 0)?,
                    f_at(a, 1)?,
                    f_at(a, 2)?,
                    f_at(a, 3)?,
                    f_at(a, 4)?,
                    f_at(a, 5)?,
                );
                let target = reg_mut(&mut reg, rg, 0)?;
                if cmd.op == Op::FromExtent {
                    *target = PseudoBox::from_extent3d(&e);
                } else {
                    target.assign_extent3d(&e);
                }
            }
            Op::SetBasis => {
                let r = reg_mut(&mut reg, rg, 0)?.set_basis(v3_at(a, 0)?, v3_at(a, 3)?, v3_at(a, 6)?);
                o.push(r.map_or_else(|e| status(&e), |()| 0.0));
            }
            Op::SetIntervals => {
                let (i0, i1, i2) = (ivl_at(a, 0)?, ivl_at(a, 2)?, ivl_at(a, 4)?);
                reg_mut(&mut reg, rg, 0)?.set_intervals(i0, i1, i2);
            }
            Op::SetInterval => {
                let idx = f_at(a, 0)? as usize;
                let ivl = ivl_at(a, 1)?;
                reg_mut(&mut reg, rg, 0)?
                    .set_interval(idx, ivl)
                    .map_err(|e| harness(e.to_string()))?;
            }
            Op::SetUnbounded => reg_mut(&mut reg, rg, 0)?.set_unbounded(),
            Op::AddPoint => reg_mut(&mut reg, rg, 0)?.add_point(v3_at(a, 0)?),
            Op::Expand => reg_mut(&mut reg, rg, 0)?.expand_absolute(f_at(a, 0)?),
            Op::Dump => {
                for k in 0..3 {
                    put(&mut o, x.basis_vector(k).unwrap_or(Vec3::UNDEF));
                }
                for k in 0..3 {
                    let ivl = x.interval(k).unwrap_or_default();
                    o.extend([ivl.min(), ivl.max()]);
                }
            }
            Op::Contains => o.push(flag(x.contains_point(v3_at(a, 0)?, f_at(a, 3)?))),
            Op::Evaluate => put(&mut o, x.evaluate(f_at(a, 0)?, f_at(a, 1)?, f_at(a, 2)?)),
            Op::InvertAbsolute => match x.invert_absolute(v3_at(a, 0)?) {
                Ok(p) => {
                    o.push(0.0);
                    put(&mut o, p);
                }
                Err(e) => o.push(status(&e)),
            },
            Op::Union => {
                let other = reg_at(&reg, rg, 1)?;
                let basis = [v3_at(a, 0)?, v3_at(a, 3)?, v3_at(a, 6)?];
                x.union_into(&other, &basis, reg_mut(&mut reg, rg, 2)?);
            }
            Op::Distance => o.push(x.distance_to_point(v3_at(a, 0)?)),
            Op::IntersectLine => match x.intersect_line(v3_at(a, 0)?, v3_at(a, 3)?) {
                Ok(hit) => {
                    o.extend([0.0, f64::from(hit.count())]);
                    match hit {
                        LineHit::Miss => o.extend([0.0, 0.0]),
                        LineHit::Graze(t) => o.extend([t, t]),
                        LineHit::Segment(t0, t1) => o.extend([t0, t1]),
                    }
                }
                Err(e) => o.push(status(&e)),
            },
            Op::NegativeVolume => o.push(flag(x.has_negative_volume())),
            Op::Measures => o.extend([x.max_dimension(), x.xy_area(), x.volume()]),
            Op::PointSized => {
                let r = x.is_point_sized(f_at(a, 0)?);
                o.push(flag(r.is_point_sized));
                put(&mut o, r.center);
                o.push(r.deviation);
            }
            Op::Linear => {
                let r = x.is_linear(f_at(a, 0)?);
                o.push(flag(r.is_linear));
                put(&mut o, r.point);
                put(&mut o, r.tangent);
                o.push(r.deviation);
            }
            Op::Planar => {
                let r = x.is_planar(f_at(a, 0)?);
                o.push(flag(r.is_planar));
                put(&mut o, r.point);
                put(&mut o, r.normal);
                o.push(r.deviation);
            }
            Op::AreDisjoint => {
                let other = reg_at(&reg, rg, 1)?;
                let proj = if f_at(a, 0)? == 0.0 {
                    None
                } else {
                    Some(v3_at(a, 1)?)
                };
                o.push(flag(x.are_disjoint(&other, proj)));
            }
            Op::ProjectDisjoint => {
                let other = reg_at(&reg, rg, 1)?;
                o.push(flag(x.are_project_disjoint(&other, &proj_at(a)?)));
            }
            Op::ContainedBy => {
                let other = reg_at(&reg, rg, 1)?;
                o.push(flag(x.is_contained_by(&other, f_at(a, 0)?)));
            }
            Op::Classify => {
                let t = x.classify_point(v3_at(a, 0)?, f_at(a, 3)?);
                o.extend(t.map(|e| f64::from(e.code())));
            }
            Op::Planes => {
                let (pts, vs) = x.calc_planes();
                for p in pts.into_iter().chain(vs) {
                    put(&mut o, p);
                }
            }
            Op::Bounded => {
                o.push(flag(x.is_bounded()));
                o.extend(x.boundary_types().map(|t| f64::from(t.code())));
            }
            Op::AxisAligned => put_map(&mut o, x.is_axis_aligned()),
            Op::AreAligned => put_map(&mut o, x.are_aligned(&reg_at(&reg, rg, 1)?)),
            Op::Corners => {
                for p in x.corners() {
                    put(&mut o, p);
                }
            }
            Op::Edges => {
                for p in x.edges() {
                    put(&mut o, p);
                }
            }
            Op::AssertDefined => o.push(flag(x.assert_defined())),
            Op::Project => {
                let proj = proj_at(a)?;
                let r = x.project_to_plane_into(&proj, reg_mut(&mut reg, rg, 1)?);
                o.push(r.map_or_else(|e| status(&e), |()| 0.0));
            }
            Op::Assign => {
                let src = reg_at(&reg, rg, 1)?;
                *reg_mut(&mut reg, rg, 0)? = src;
            }
            Op::FromBasisIntervals => {
                let b = [v3_at(a, 0)?, v3_at(a, 3)?, v3_at(a, 6)?];
                let i = [ivl_at(a, 9)?, ivl_at(a, 11)?, ivl_at(a, 13)?];
                *reg_mut(&mut reg, rg, 0)? = PseudoBox::from_basis_intervals(b, i);
            }
            Op::Circumscribe => {
                let e = Extent3d::from_pseudo_box(&x);
                put(&mut o, e.min());
                put(&mut o, e.max());
            }
            Op::ScaledZero => o.push(x.scaled_zero()),
        }
    }
    Ok(o)
}

// ---- generation ----------------------------------------------------------------------

fn norm3(a: V3) -> V3 {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    if l > 0.0 {
        [a[0] / l, a[1] / l, a[2] / l]
    } else {
        [0.0, 0.0, 1.0]
    }
}

fn cross3(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn unit3(rng: &mut Rng) -> V3 {
    loop {
        let p = rng.point3(-1.0, 1.0);
        let l2 = p[0] * p[0] + p[1] * p[1] + p[2] * p[2];
        if l2 > 0.01 && l2 <= 1.0 {
            return norm3(p);
        }
    }
}

fn axis(i: u64, sign: f64) -> V3 {
    match i {
        0 => [sign, 0.0, 0.0],
        1 => [0.0, sign, 0.0],
        _ => [0.0, 0.0, sign],
    }
}

/// A basis: identity, a signed axis permutation, a rotation, a skewed one, unscaled
/// vectors, a nearly or exactly singular one.
fn basis(rng: &mut Rng) -> [V3; 3] {
    let sign = |rng: &mut Rng| if rng.chance(0.3) { -1.0 } else { 1.0 };
    match rng.below(10) {
        0 => [axis(0, 1.0), axis(1, 1.0), axis(2, 1.0)],
        1 | 2 => {
            let perms = [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]];
            let p = *rng.choose(&perms).unwrap_or(&[0, 1, 2]);
            [axis(p[0], sign(rng)), axis(p[1], sign(rng)), axis(p[2], sign(rng))]
        }
        3 | 4 => {
            let x = unit3(rng);
            let z = norm3(cross3(x, unit3(rng)));
            let y = cross3(z, x);
            [x, y, z]
        }
        5 | 6 => [unit3(rng), unit3(rng), unit3(rng)],
        7 => {
            let s = *rng.choose(&[0.5, 2.0, 10.0, 1.0 + 1e-13]).unwrap_or(&2.0);
            let b = [unit3(rng), unit3(rng), unit3(rng)];
            [b[0].map(|v| v * s), b[1], b[2].map(|v| v / s)]
        }
        8 => {
            // nearly singular: third vector close to the plane of the first two
            let a = unit3(rng);
            let b = unit3(rng);
            let eps = *rng.choose(&[0.0, 1e-13, 1e-12, 1e-9, 1e-6]).unwrap_or(&0.0);
            let n = norm3(cross3(a, b));
            let s = rng.uniform(-1.0, 1.0);
            let c = norm3([
                a[0] * s + b[0] + eps * n[0],
                a[1] * s + b[1] + eps * n[1],
                a[2] * s + b[2] + eps * n[2],
            ]);
            [a, b, c]
        }
        _ => {
            // identity perturbed by tiny amounts (just orthogonal / just aligned)
            let e = *rng.choose(&[1e-14, 1e-13, 5e-13, 1e-11]).unwrap_or(&1e-13);
            [[1.0, e, 0.0], [0.0, 1.0, -e], [e, 0.0, 1.0]]
        }
    }
}

fn coord(rng: &mut Rng, scale: f64) -> f64 {
    if rng.chance(0.15) {
        rng.range_i64(-3, 3) as f64
    } else {
        rng.uniform(-scale, scale)
    }
}

fn point(rng: &mut Rng, scale: f64) -> V3 {
    [coord(rng, scale), coord(rng, scale), coord(rng, scale)]
}

fn interval(rng: &mut Rng, scale: f64) -> [f64; 2] {
    let a = coord(rng, scale);
    match rng.below(10) {
        0 => [a, a],
        1 => [a, a + 1e-13],
        2 => [-INFINITE_PARAMETER, a],
        3 => [a, INFINITE_PARAMETER],
        _ => {
            let b = coord(rng, scale);
            [a, b]
        }
    }
}

fn proj(rng: &mut Rng, scale: f64) -> Proj {
    let kind = match rng.below(3) {
        0 => ProjKind::Parallel,
        1 => ProjKind::Perspective,
        _ => ProjKind::Rotation,
    };
    let vec = if rng.chance(0.3) {
        axis(rng.below(3), 1.0)
    } else {
        unit3(rng)
    };
    let aux = match kind {
        ProjKind::Parallel => [0.0; 3],
        ProjKind::Perspective => {
            let d = rng.uniform(2.0, 20.0) * scale;
            [vec[0] * d, vec[1] * d, vec[2] * d]
        }
        ProjKind::Rotation => norm3(cross3(vec, unit3(rng))),
    };
    Proj {
        kind,
        point: if rng.chance(0.4) { [0.0; 3] } else { point(rng, scale) },
        vec,
        aux,
    }
}

/// A point near the box `r` described by `b` (unit basis) and a cloud `pts` (centre
/// and spread), at normalised-ish coordinates on, inside or just outside.
fn near(rng: &mut Rng, pts: &[V3], scale: f64) -> V3 {
    if pts.is_empty() || rng.chance(0.25) {
        return point(rng, scale);
    }
    let i = rng.below(pts.len() as u64) as usize;
    let j = rng.below(pts.len() as u64) as usize;
    let (p, q) = (pts.get(i).copied().unwrap_or_default(), pts.get(j).copied().unwrap_or_default());
    let t = *rng
        .choose(&[0.0, 1.0, 0.5, -1e-13, 1.0 + 1e-13, -0.3, 1.3, 2.0])
        .unwrap_or(&0.5);
    let t = if rng.chance(0.3) { rng.uniform(-0.5, 1.5) } else { t };
    [
        p[0] + t * (q[0] - p[0]),
        p[1] + t * (q[1] - p[1]),
        p[2] + t * (q[2] - p[2]),
    ]
}

fn tol(rng: &mut Rng) -> f64 {
    *rng.choose(&[0.0, 1e-12, 1e-9, 1e-6, 0.01, 0.5, -1e-6])
        .unwrap_or(&0.0)
}

/// Builds register `r`; returns the points that define it (for nearby queries).
fn build(rng: &mut Rng, prog: &mut Vec<Cmd>, r: usize, scale: f64) -> Vec<V3> {
    let center = point(rng, scale);
    let spread = *rng.choose(&[1e-13, 1e-6, 0.1, 1.0, 3.0]).unwrap_or(&1.0) * scale;
    let cloud = |rng: &mut Rng, n: usize| -> Vec<V3> {
        (0..n)
            .map(|_| {
                let d = rng.point3(-spread, spread);
                if rng.chance(0.2) {
                    // flat clouds give planar / linear boxes
                    [center[0] + d[0], center[1], center[2] + d[2] * 1e-14]
                } else {
                    [center[0] + d[0], center[1] + d[1], center[2] + d[2]]
                }
            })
            .collect()
    };
    match rng.below(10) {
        0..=4 => {
            prog.push(Cmd::SetBasis { r, b: basis(rng) });
            let n = 1 + rng.below(6) as usize;
            let pts = cloud(rng, n);
            prog.push(Cmd::AddPoints { r, pts: pts.clone() });
            pts
        }
        5 => {
            let e = [
                center[0] - spread,
                center[1] - spread,
                center[2],
                center[0] + spread,
                center[1] + spread,
                center[2] + spread * rng.uniform(-0.5, 2.0),
            ];
            if rng.chance(0.5) {
                prog.push(Cmd::FromExtent { r, e });
            } else {
                prog.push(Cmd::AssignExtent { r, e });
            }
            vec![[e[0], e[1], e[2]], [e[3], e[4], e[5]]]
        }
        6 | 7 => {
            let b = basis(rng);
            let i = [interval(rng, scale), interval(rng, scale), interval(rng, scale)];
            prog.push(Cmd::FromBasisIntervals { r, b, i });
            vec![center]
        }
        8 => {
            prog.push(Cmd::SetBasis { r, b: basis(rng) });
            prog.push(Cmd::SetIntervals {
                r,
                i: [interval(rng, scale), interval(rng, scale), interval(rng, scale)],
            });
            if rng.chance(0.3) {
                prog.push(Cmd::SetInterval {
                    r,
                    idx: rng.below(3) as usize,
                    ivl: interval(rng, scale),
                });
            }
            vec![center]
        }
        _ => {
            // inside-out or unbounded boxes
            prog.push(Cmd::SetBasis { r, b: basis(rng) });
            if rng.chance(0.5) {
                prog.push(Cmd::SetUnbounded { r });
            } else {
                prog.push(Cmd::Init { r });
                if rng.chance(0.3) {
                    prog.push(Cmd::SetInterval { r, idx: rng.below(3) as usize, ivl: interval(rng, scale) });
                }
            }
            vec![center]
        }
    }
}

fn gen_inputs(rng: &mut Rng) -> Inputs {
    let scale = *rng.choose(&[1.0, 1.0, 10.0, 1e-3, 1e4]).unwrap_or(&1.0);
    let mut prog = Vec::new();
    let pts_a = build(rng, &mut prog, 0, scale);
    let pts_b = build(rng, &mut prog, 1, scale);
    if rng.chance(0.3) {
        let d = *rng.choose(&[0.0, 1e-12, 0.1, 1.0]).unwrap_or(&0.1) * scale;
        prog.push(Cmd::Expand { r: rng.below(2) as usize, d });
    }
    prog.push(Cmd::Dump { r: 0 });
    prog.push(Cmd::Dump { r: 1 });

    let n_q = 4 + rng.below(10) as usize;
    for _ in 0..n_q {
        // mostly the two built registers; 2 and 3 hold union/project/assign results
        let pick = |rng: &mut Rng| {
            let n = if rng.chance(0.85) { 2 } else { 4 };
            rng.below(n) as usize
        };
        let r = pick(rng);
        let s = pick(rng);
        let pts = if r == 0 { &pts_a } else { &pts_b };
        let p = near(rng, pts, scale);
        match rng.below(30) {
            0 => prog.push(Cmd::Contains { r, p, tol: tol(rng) }),
            1 => {
                let n = [rng.uniform(-0.2, 1.2), rng.below(2) as f64, rng.uniform(0.0, 1.0)];
                prog.push(Cmd::Eval { r, n });
            }
            2 => prog.push(Cmd::Invert { r, p }),
            3 => {
                let t = rng.below(4) as usize;
                let b = if rng.chance(0.5) {
                    let x = rng.below(2) as usize;
                    let bx = [basis(rng), basis(rng)];
                    *bx.get(x).unwrap_or(&[axis(0, 1.0), axis(1, 1.0), axis(2, 1.0)])
                } else {
                    [axis(0, 1.0), axis(1, 1.0), axis(2, 1.0)]
                };
                prog.push(Cmd::Union { r, s, t, b });
                prog.push(Cmd::Dump { r: t });
            }
            4 | 5 => prog.push(Cmd::Distance { r, p }),
            6 | 7 => {
                let v = match rng.below(4) {
                    0 => axis(rng.below(3), 1.0),
                    1 => [0.0, 0.0, 0.0],
                    _ => unit3(rng).map(|c| c * rng.uniform(0.1, 3.0)),
                };
                prog.push(Cmd::Line { r, p, v });
            }
            8 => prog.push(Cmd::NegVolume { r }),
            9 => prog.push(Cmd::Measures { r }),
            10 => {
                let tl = *rng.choose(&[1e-12, 1e-12, 1e-6, 0.1, 1.0]).unwrap_or(&1e-12);
                match rng.below(3) {
                    0 => prog.push(Cmd::PointSized { r, tol: tl }),
                    1 => prog.push(Cmd::Linear { r, tol: tl }),
                    _ => prog.push(Cmd::Planar { r, tol: tl }),
                }
            }
            11 | 12 => {
                let proj = if rng.chance(0.3) { Some(unit3(rng)) } else { None };
                prog.push(Cmd::Disjoint { r, s, proj });
            }
            13 => prog.push(Cmd::ProjectDisjoint { r, s, proj: proj(rng, scale) }),
            14 => prog.push(Cmd::ContainedBy { r, s, tol: tol(rng) }),
            15 => prog.push(Cmd::Classify { r, p, tol: *rng.choose(&[1e-12, 0.0, 1e-6, 0.1]).unwrap_or(&1e-12) }),
            16 => prog.push(Cmd::Planes { r }),
            17 => prog.push(Cmd::Bounded { r }),
            18 => prog.push(Cmd::AxisAligned { r }),
            19 => prog.push(Cmd::Aligned { r, s }),
            20 => prog.push(Cmd::Corners { r }),
            21 => prog.push(Cmd::Edges { r }),
            22 => prog.push(Cmd::Defined { r }),
            23..=25 => {
                let t = if rng.chance(0.3) { r } else { rng.below(4) as usize };
                prog.push(Cmd::Project { r, t, proj: proj(rng, scale) });
                prog.push(Cmd::Dump { r: t });
            }
            26 => {
                prog.push(Cmd::Assign { t: s, r });
                prog.push(Cmd::Dump { r: s });
            }
            27 => prog.push(Cmd::Circumscribe { r }),
            28 => prog.push(Cmd::ScaledZero { r }),
            _ => {
                prog.push(Cmd::Add { r, p });
                prog.push(Cmd::Dump { r });
            }
        }
    }
    Inputs { prog }
}

// ---- runner --------------------------------------------------------------------------

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "geom.pseudo_box"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let prog = lower(&i.prog)?;
        let flat = ffi::run(&prog).map_err(|e| harness(e.to_string()))?;
        split(&prog, &flat)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let prog = lower(&i.prog)?;
        split(&prog, &rust_flat(&prog)?)
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

    /// The generated programs reach every distance branch, every line outcome and
    /// the projection paths.
    #[test]
    fn generated_cases_cover_branches() {
        let (mut line_counts, mut line_err) = ([0_usize; 3], 0);
        let (mut aligned, mut basis_err, mut invert_err) = (0, 0, 0);
        for i in 0..MIN_GENERATED as u64 {
            let mut rng = Rng::new(fnv1a64("geom.pseudo_box").wrapping_add(i));
            let prog = lower(&gen_inputs(&mut rng).prog).unwrap();
            let out = split(&prog, &rust_flat(&prog).unwrap()).unwrap();
            for (k, v) in &out.ints {
                let first = v.first().copied().unwrap_or(-1);
                if k.ends_with("line_count") {
                    line_counts[usize::try_from(first).unwrap()] += 1;
                } else if k.ends_with("line_status") && first != 0 {
                    line_err += 1;
                } else if k.ends_with("axis_aligned") && first == 1 {
                    aligned += 1;
                } else if k.ends_with("set_basis") && first != 0 {
                    basis_err += 1;
                } else if k.ends_with("invert_status") && first != 0 {
                    invert_err += 1;
                }
            }
        }
        assert!(
            line_counts.iter().all(|&n| n > 0) && line_err > 0,
            "line {line_counts:?} err {line_err}"
        );
        assert!(aligned > 0 && basis_err > 0 && invert_err > 0, "{aligned} {basis_err} {invert_err}");
    }
}
