//! Runner for unit `geom.polar_box`: polar boxes bounding sets of directions.
//!
//! C++: `solidcore_ffi::units::geom_polar_box` (upstream `source/SMLib/src/SmPolarBox.cpp`).
//! Rust: `solidcore::geom::polar_box`.
//!
//! A case is a program of commands on four box registers (all start as
//! `SmPolarBox()` / [`PolarBox::new`]); both sides run it and every command's
//! output becomes its own key `cNNN_<name>`.
//!
//! | `cmd`               | Rust                                              | upstream |
//! |---------------------|---------------------------------------------------|----------|
//! | `reset`             | `reset`                                           | `ReSet` |
//! | `from_vector`       | `assign(&PolarBox::from_vector(v))`               | `operator=(SmPolarBox(v))` |
//! | `add`               | `add_vector`                                      | `AddVector3d` |
//! | `add_edges`         | `add_vector` of consecutive point differences     | `AddVector3d` (control-polygon boxes) |
//! | `dump`              | `state`, `basis`, `domain`                        | `GetPolarBoxState`, `GetBasis`, `GetDomain` |
//! | `polar`             | `compute_polar_coord`                             | `ComputePolarCoord` |
//! | `area`              | `polar_area`                                      | `GetPolarArea` |
//! | `eval`              | `evaluate_normalized`                             | `EvaluateNormalized` |
//! | `boundary`          | `boundary_vectors`                                | `GetBoundaryVectors` |
//! | `disjoint_cone`     | `is_disjoint_with_cone`                           | `IsDisjointWithCone` |
//! | `perp_disjoint_cone`| `is_perpendicular_disjoint_with_cone`             | `IsPerpendicularDisjointWithCone` |
//! | `are_disjoint`      | `are_disjoint`                                    | `AreDisjoint` |
//! | `cross`             | `cross` + `assign`                                | `Cross` + `operator=` |
//! | `union`             | `union` + `assign`                                | `Union` + `operator=` |
//! | `union_self`        | `union_assign`                                    | `Union(other, *this)` |
//! | `has_perp`          | `has_perpendicular_to_vector`                     | `HasPerpendicularToVector` |
//! | `expand`            | `expand_absolute`                                 | `ExpandAbsolute` |
//! | `assign`            | `assign`                                          | `operator=` |
//!
//! Statuses are compared as ints (0 = success, 1007 = `SM_ERR_INVALID_INPUT`, the
//! Rust error for `GetBoundaryVectors` on an empty/unbounded box and for `Union`'s
//! undefined-behaviour case, which the shim skips). Every run has at least
//! [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::polar_box::PolarBox;
use solidcore::geom::vector2d::Vec2;
use solidcore::geom::vector3d::Vec3;
use solidcore_ffi::units::geom_polar_box::{self as ffi, Cmd as FCmd, SM_ERR_INVALID_INPUT};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::{flag, v3};
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `geom.polar_box` runner.
pub struct Runner;

type V3 = [f64; 3];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum Cmd {
    Reset { r: usize },
    FromVector { r: usize, v: V3 },
    Add { r: usize, v: V3, #[serde(default, skip_serializing_if = "Option::is_none")] b2: Option<V3> },
    AddEdges { r: usize, pts: Vec<V3>, #[serde(default)] start: usize, #[serde(default)] count: Option<usize> },
    Dump { r: usize },
    Polar { r: usize, v: V3 },
    Area { r: usize },
    Eval { r: usize, a: f64, b: f64 },
    Boundary { r: usize },
    DisjointCone { r: usize, v: V3, ang: f64 },
    PerpDisjointCone { r: usize, v: V3, ang: f64 },
    AreDisjoint { r: usize, s: usize },
    Cross { r: usize, s: usize, t: usize },
    Union { r: usize, s: usize, t: usize },
    UnionSelf { r: usize, s: usize },
    HasPerp { r: usize, v: V3 },
    Expand { r: usize, d: f64 },
    Assign { t: usize, r: usize },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    prog: Vec<Cmd>,
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// The edge vectors `pts[i+1] - pts[i]` for `i` in `start..start+count` (all edges
/// from `start` when `count` is absent).
fn edges(pts: &[V3], start: usize, count: Option<usize>) -> Result<Vec<V3>, HarnessError> {
    let n_edges = pts.len().saturating_sub(1);
    let end = count.map_or(n_edges, |c| start + c);
    if start > end || end > n_edges {
        return Err(harness(format!("add_edges: edges {start}..{end} of {n_edges}")));
    }
    Ok(pts
        .windows(2)
        .skip(start)
        .take(end - start)
        .filter_map(|w| match w {
            [a, b] => Some([b[0] - a[0], b[1] - a[1], b[2] - a[2]]),
            _ => None,
        })
        .collect())
}

/// Expands the case program into shim commands (`add_edges` becomes `add`s).
fn lower(prog: &[Cmd]) -> Result<Vec<FCmd>, HarnessError> {
    let mut out = Vec::with_capacity(prog.len());
    for c in prog {
        let regs: &[usize] = match c {
            Cmd::Reset { r }
            | Cmd::FromVector { r, .. }
            | Cmd::Add { r, .. }
            | Cmd::AddEdges { r, .. }
            | Cmd::Dump { r }
            | Cmd::Polar { r, .. }
            | Cmd::Area { r }
            | Cmd::Eval { r, .. }
            | Cmd::Boundary { r }
            | Cmd::DisjointCone { r, .. }
            | Cmd::PerpDisjointCone { r, .. }
            | Cmd::HasPerp { r, .. }
            | Cmd::Expand { r, .. } => &[*r],
            Cmd::AreDisjoint { r, s } | Cmd::UnionSelf { r, s } => &[*r, *s],
            Cmd::Assign { t, r } => &[*t, *r],
            Cmd::Cross { r, s, t } | Cmd::Union { r, s, t } => &[*r, *s, *t],
        };
        if regs.iter().any(|&r| r >= ffi::REGISTERS) {
            return Err(harness(format!("register out of range in {c:?}")));
        }
        if let Cmd::UnionSelf { r, s } = c {
            if r == s {
                return Err(harness("union_self needs two registers"));
            }
        }
        match c.clone() {
            Cmd::Reset { r } => out.push(FCmd::Reset { r }),
            Cmd::FromVector { r, v } => out.push(FCmd::FromVector { r, v }),
            Cmd::Add { r, v, b2 } => out.push(FCmd::Add { r, v, b2 }),
            Cmd::AddEdges { r, pts, start, count } => {
                for v in edges(&pts, start, count)? {
                    out.push(FCmd::Add { r, v, b2: None });
                }
            }
            Cmd::Dump { r } => out.push(FCmd::Dump { r }),
            Cmd::Polar { r, v } => out.push(FCmd::PolarCoord { r, v }),
            Cmd::Area { r } => out.push(FCmd::Area { r }),
            Cmd::Eval { r, a, b } => out.push(FCmd::Evaluate { r, a, b }),
            Cmd::Boundary { r } => out.push(FCmd::BoundaryVectors { r }),
            Cmd::DisjointCone { r, v, ang } => out.push(FCmd::DisjointCone { r, v, ang }),
            Cmd::PerpDisjointCone { r, v, ang } => out.push(FCmd::PerpDisjointCone { r, v, ang }),
            Cmd::AreDisjoint { r, s } => out.push(FCmd::AreDisjoint { r, s }),
            Cmd::Cross { r, s, t } => out.push(FCmd::Cross { r, s, t }),
            Cmd::Union { r, s, t } => out.push(FCmd::Union { r, s, t }),
            Cmd::UnionSelf { r, s } => out.push(FCmd::UnionSelf { r, s }),
            Cmd::HasPerp { r, v } => out.push(FCmd::HasPerpendicular { r, v }),
            Cmd::Expand { r, d } => out.push(FCmd::Expand { r, d }),
            Cmd::Assign { t, r } => out.push(FCmd::Assign { t, r }),
        }
    }
    Ok(out)
}

/// Kind of an output slot.
#[derive(Clone, Copy)]
enum K {
    /// Floats.
    F,
    /// Integers (status, flags, state codes, counts).
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
}

/// Splits the flat result into one key per output part of each command.
fn split(prog: &[FCmd], flat: &[f64]) -> Result<Output, HarnessError> {
    let mut cur = Cursor {
        rest: flat,
        out: Output::new(),
    };
    for (i, c) in prog.iter().enumerate() {
        let key = |name: &str| format!("c{i:03}_{name}");
        match c {
            FCmd::Reset { .. }
            | FCmd::FromVector { .. }
            | FCmd::Add { .. }
            | FCmd::Expand { .. }
            | FCmd::Assign { .. } => {}
            FCmd::Dump { .. } => {
                cur.take(&key("state"), 1, K::I)?;
                cur.take(&key("basis"), 9, K::F)?;
                cur.take(&key("domain"), 4, K::F)?;
            }
            FCmd::PolarCoord { .. } => cur.take(&key("polar"), 2, K::F)?,
            FCmd::Area { .. } => cur.take(&key("area"), 1, K::F)?,
            FCmd::Evaluate { .. } => cur.take(&key("eval"), 3, K::F)?,
            FCmd::BoundaryVectors { .. } => {
                let status = cur.peek();
                cur.take(&key("boundary_status"), 1, K::I)?;
                if status == Some(0.0) {
                    let n = cur.peek().unwrap_or(0.0);
                    if !(0.0..=4.0).contains(&n) {
                        return Err(harness(format!("bad boundary vector count {n}")));
                    }
                    cur.take(&key("boundary_count"), 1, K::I)?;
                    cur.take(&key("boundary"), 3 * (n as usize), K::F)?;
                }
            }
            FCmd::DisjointCone { .. } => cur.take(&key("disjoint_cone"), 1, K::I)?,
            FCmd::PerpDisjointCone { .. } => cur.take(&key("perp_disjoint_cone"), 1, K::I)?,
            FCmd::AreDisjoint { .. } => cur.take(&key("are_disjoint"), 1, K::I)?,
            FCmd::Cross { .. } => cur.take(&key("cross_status"), 1, K::I)?,
            FCmd::Union { .. } => cur.take(&key("union_status"), 1, K::I)?,
            FCmd::UnionSelf { .. } => cur.take(&key("union_self_status"), 1, K::I)?,
            FCmd::HasPerpendicular { .. } => cur.take(&key("has_perp"), 1, K::I)?,
        }
    }
    if !cur.rest.is_empty() {
        return Err(harness(format!("result too long by {}", cur.rest.len())));
    }
    Ok(cur.out)
}

fn put(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn reg_mut(reg: &mut [PolarBox], r: usize) -> Result<&mut PolarBox, HarnessError> {
    reg.get_mut(r)
        .ok_or_else(|| harness(format!("register {r} out of range")))
}

fn reg_get(reg: &[PolarBox], r: usize) -> Result<PolarBox, HarnessError> {
    reg.get(r)
        .copied()
        .ok_or_else(|| harness(format!("register {r} out of range")))
}

/// Runs the program on the Rust port, producing the shim's flat layout.
fn rust_flat(prog: &[FCmd]) -> Result<Vec<f64>, HarnessError> {
    let mut reg = [PolarBox::new(); ffi::REGISTERS];
    let mut o = Vec::new();
    for c in prog {
        match *c {
            FCmd::Reset { r } => reg_mut(&mut reg, r)?.reset(),
            FCmd::FromVector { r, v } => {
                reg_mut(&mut reg, r)?.assign(&PolarBox::from_vector(v3(v)));
            }
            FCmd::Add { r, v, b2 } => reg_mut(&mut reg, r)?.add_vector(v3(v), b2.map(v3)),
            FCmd::Dump { r } => {
                let b = reg_get(&reg, r)?;
                o.push(f64::from(b.state().code()));
                for v in b.basis() {
                    put(&mut o, *v);
                }
                let d = b.domain();
                o.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
            }
            FCmd::PolarCoord { r, v } => {
                let p = reg_get(&reg, r)?
                    .compute_polar_coord(v3(v))
                    .unwrap_or(Vec2::UNDEF);
                o.extend([p.x, p.y]);
            }
            FCmd::Area { r } => o.push(reg_get(&reg, r)?.polar_area()),
            FCmd::Evaluate { r, a, b } => put(&mut o, reg_get(&reg, r)?.evaluate_normalized(a, b)),
            FCmd::BoundaryVectors { r } => match reg_get(&reg, r)?.boundary_vectors() {
                Ok(vs) => {
                    o.extend([0.0, vs.len() as f64]);
                    for v in vs {
                        put(&mut o, v);
                    }
                }
                Err(_) => o.push(SM_ERR_INVALID_INPUT),
            },
            FCmd::DisjointCone { r, v, ang } => {
                o.push(flag(reg_get(&reg, r)?.is_disjoint_with_cone(v3(v), ang)));
            }
            FCmd::PerpDisjointCone { r, v, ang } => {
                let b = reg_get(&reg, r)?;
                o.push(flag(b.is_perpendicular_disjoint_with_cone(v3(v), ang)));
            }
            FCmd::AreDisjoint { r, s } => {
                let other = reg_get(&reg, s)?;
                o.push(flag(reg_get(&reg, r)?.are_disjoint(&other)));
            }
            FCmd::Cross { r, s, t } => {
                let x = reg_get(&reg, r)?.cross(&reg_get(&reg, s)?);
                reg_mut(&mut reg, t)?.assign(&x);
                o.push(0.0);
            }
            FCmd::Union { r, s, t } => match reg_get(&reg, r)?.union(&reg_get(&reg, s)?) {
                Ok(x) => {
                    reg_mut(&mut reg, t)?.assign(&x);
                    o.push(0.0);
                }
                Err(_) => o.push(SM_ERR_INVALID_INPUT),
            },
            FCmd::UnionSelf { r, s } => {
                let other = reg_get(&reg, s)?;
                let target = reg_mut(&mut reg, r)?;
                let backup = *target;
                if target.union_assign(&other).is_ok() {
                    o.push(0.0);
                } else {
                    // the shim skips upstream's undefined-behaviour case
                    *target = backup;
                    o.push(SM_ERR_INVALID_INPUT);
                }
            }
            FCmd::HasPerpendicular { r, v } => {
                o.push(flag(reg_get(&reg, r)?.has_perpendicular_to_vector(v3(v))));
            }
            FCmd::Expand { r, d } => reg_mut(&mut reg, r)?.expand_absolute(d),
            FCmd::Assign { t, r } => {
                let src = reg_get(&reg, r)?;
                reg_mut(&mut reg, t)?.assign(&src);
            }
        }
    }
    Ok(o)
}

// ---- generation ----------------------------------------------------------------------

fn unit3(rng: &mut Rng) -> V3 {
    loop {
        let p = rng.point3(-1.0, 1.0);
        let l2 = p[0] * p[0] + p[1] * p[1] + p[2] * p[2];
        if l2 > 0.01 && l2 <= 1.0 {
            let l = l2.sqrt();
            return [p[0] / l, p[1] / l, p[2] / l];
        }
    }
}

fn add3(a: V3, b: V3, s: f64) -> V3 {
    [a[0] + s * b[0], a[1] + s * b[1], a[2] + s * b[2]]
}

fn scale3(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn cross3(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// A random direction family: centre, spread and whether it stays planar.
struct Family {
    center: V3,
    perp: V3,
    spread: f64,
    planar: bool,
    integer: bool,
}

impl Family {
    fn new(rng: &mut Rng) -> Self {
        let integer = rng.chance(0.15);
        let center = if integer {
            *rng
                .choose(&[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 1.0, 0.0], [0.0, -1.0, 1.0]])
                .unwrap_or(&[1.0, 0.0, 0.0])
        } else {
            unit3(rng)
        };
        let mut perp = cross3(center, unit3(rng));
        let l = (perp[0] * perp[0] + perp[1] * perp[1] + perp[2] * perp[2]).sqrt();
        if l > 1e-6 {
            perp = scale3(perp, 1.0 / l);
        } else {
            perp = [0.0, 0.0, 1.0];
        }
        let spread = *rng.choose(&[1e-9, 1e-7, 1e-4, 0.01, 0.1, 0.3, 0.6, 1.0, 2.0]).unwrap_or(&0.1);
        Self { center, perp, spread, planar: rng.chance(0.25), integer }
    }

    fn sample(&self, rng: &mut Rng) -> V3 {
        let roll = rng.below(100);
        let len = if self.integer { 1.0 } else { rng.uniform(0.1, 10.0) };
        if roll < 4 {
            return [0.0, 0.0, 0.0];
        }
        if roll < 7 {
            return scale3(self.center, -len);
        }
        if roll < 12 {
            return scale3(self.center, len);
        }
        if roll < 14 {
            return scale3(self.center, 1e-13);
        }
        let dir = if self.planar {
            add3(self.center, self.perp, rng.uniform(-self.spread, self.spread))
        } else {
            let u = unit3(rng);
            add3(self.center, u, self.spread * rng.unit())
        };
        if self.integer {
            [dir[0].round(), dir[1].round(), dir[2].round()]
        } else {
            scale3(dir, len)
        }
    }
}

fn build(rng: &mut Rng, prog: &mut Vec<Cmd>, r: usize, fam: &Family) {
    if rng.chance(0.3) {
        prog.push(Cmd::FromVector { r, v: fam.sample(rng) });
    }
    let n = rng.below(7) as usize;
    for k in 0..n {
        let v = fam.sample(rng);
        let b2 = if k == 0 && rng.chance(0.2) {
            Some(if rng.chance(0.5) { fam.perp } else { unit3(rng) })
        } else {
            None
        };
        prog.push(Cmd::Add { r, v, b2 });
    }
}

fn gen_inputs(rng: &mut Rng) -> Inputs {
    let mut prog = Vec::new();
    let fam_a = Family::new(rng);
    let fam_b = if rng.chance(0.5) { Family::new(rng) } else { Family { center: fam_a.center, perp: fam_a.perp, spread: fam_a.spread, planar: !fam_a.planar, integer: fam_a.integer } };
    build(rng, &mut prog, 0, &fam_a);
    build(rng, &mut prog, 1, &fam_b);
    if rng.chance(0.3) {
        prog.push(Cmd::Reset { r: 1 });
        build(rng, &mut prog, 1, &fam_b);
    }
    prog.push(Cmd::Dump { r: 0 });
    prog.push(Cmd::Dump { r: 1 });

    let n_q = 4 + rng.below(10) as usize;
    for _ in 0..n_q {
        let r = rng.below(4) as usize;
        let s = rng.below(4) as usize;
        let fam = if rng.chance(0.5) { &fam_a } else { &fam_b };
        let ang = *rng.choose(&[0.0, 1e-6, 0.01, 0.1, 0.5, 1.0, 1.6]).unwrap_or(&0.1);
        match rng.below(16) {
            0 => prog.push(Cmd::Polar { r, v: fam.sample(rng) }),
            1 => prog.push(Cmd::Area { r }),
            2 => {
                let (a, b) = if rng.chance(0.3) {
                    (rng.below(2) as f64, rng.below(2) as f64)
                } else {
                    (rng.uniform(-0.2, 1.2), rng.uniform(-0.2, 1.2))
                };
                prog.push(Cmd::Eval { r, a, b });
            }
            3 => prog.push(Cmd::Boundary { r }),
            4 => prog.push(Cmd::DisjointCone { r, v: fam.sample(rng), ang }),
            5 => prog.push(Cmd::PerpDisjointCone { r, v: fam.sample(rng), ang }),
            6 | 7 => prog.push(Cmd::AreDisjoint { r, s }),
            8 => {
                let t = rng.below(4) as usize;
                prog.push(Cmd::Cross { r, s, t });
                prog.push(Cmd::Dump { r: t });
            }
            9 | 10 => {
                let t = rng.below(4) as usize;
                prog.push(Cmd::Union { r, s, t });
                prog.push(Cmd::Dump { r: t });
            }
            11 => {
                if r != s {
                    prog.push(Cmd::UnionSelf { r, s });
                    prog.push(Cmd::Dump { r });
                }
            }
            12 => prog.push(Cmd::HasPerp { r, v: fam.sample(rng) }),
            13 => {
                let d = *rng.choose(&[0.0, 1e-6, 0.01, 0.1, 0.5, 1.5, 3.2, -0.01]).unwrap_or(&0.1);
                prog.push(Cmd::Expand { r, d });
                prog.push(Cmd::Dump { r });
            }
            14 => {
                prog.push(Cmd::Assign { t: s, r });
                prog.push(Cmd::Dump { r: s });
            }
            _ => {
                prog.push(Cmd::Add { r, v: fam.sample(rng), b2: None });
                prog.push(Cmd::Dump { r });
            }
        }
    }
    Inputs { prog }
}

// ---- runner --------------------------------------------------------------------------

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "geom.polar_box"
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

    /// The generated programs reach every box state and both union error paths.
    #[test]
    fn generated_cases_cover_states() {
        let mut states = [0_usize; 6];
        let (mut union_err, mut union_ok, mut boundary_err) = (0, 0, 0);
        for i in 0..MIN_GENERATED as u64 {
            let mut rng = Rng::new(fnv1a64("geom.polar_box").wrapping_add(i));
            let prog = lower(&gen_inputs(&mut rng).prog).unwrap();
            let out = split(&prog, &rust_flat(&prog).unwrap()).unwrap();
            for (k, v) in &out.ints {
                let first = v.first().copied().unwrap_or(-1);
                if k.ends_with("_state") {
                    states[usize::try_from(first).unwrap()] += 1;
                } else if k.ends_with("union_status") || k.ends_with("union_self_status") {
                    if first == 1007 {
                        union_err += 1;
                    } else {
                        union_ok += 1;
                    }
                } else if k.ends_with("boundary_status") && first == 1007 {
                    boundary_err += 1;
                }
            }
        }
        assert!(states.iter().all(|&n| n > 0), "states {states:?}");
        assert!(
            union_err > 0 && union_ok > 0 && boundary_err > 0,
            "states {states:?} union {union_ok}/{union_err} boundary_err {boundary_err}"
        );
    }
}
