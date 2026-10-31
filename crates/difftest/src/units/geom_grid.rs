//! Runner for unit `geom.grid`: uniform voxel grids and their 3DDDA ray walk.
//!
//! C++: `solidcore_ffi::units::geom_grid` (upstream `source/SMLib/src/SmGrid.cpp`).
//! Rust: `solidcore::geom::grid`.
//!
//! A case is a grid (`SmGrid(bbox, lSize)`) plus a program run against it on both
//! sides: `add` (`AddElement` with poly B-rep or user data), `add_hier`
//! (`AddHierarchyElement` of a base or surface hierarchy element), `find`
//! (`FindVoxelsInBox`), `address` (`GetVoxelAddress`), `voxel_box`
//! (`GetVoxelBBox`), `setup` (`Setup3DDDA`), `step` (`Step3DDDA`), `dump` (every
//! voxel's element and hierarchy lists, `GetType`, `GetElement`, bounding spheres
//! and boxes of the elements, voxel size, voxel tolerance, current mark) and `expand`
//! (`ExpandHierarchy` of a base-class element).
//!
//! Outputs: `ints` (statuses, flags, voxel addresses, element type codes and
//! object ids, hierarchy indices, 3DDDA steps) and `floats` (boxes, spheres, ray
//! parameters and the 3DDDA state after every setup and step). Every run has at
//! least [`MIN_GENERATED`] generated cases.
//!
//! Surface elements need a real surface and its cache, so they come only from the
//! `define_surface` / `add_cached_hier` / `expand_cached` / `add_cached_leaf` /
//! `observe_cached` ops, which opt into real, explicitly defined surface caches
//! (`SurfaceGridElement::init`, `Grid::expand_hierarchy` and `Grid::add_surface_node`
//! run on the node of the actual cache). They do not resolve the opaque object labels
//! of `add_hier { surface: true }`, which can be dumped but not expanded. For those
//! labels, `expand` emits the shim's -2 skip sentinel without changing the grid.
//! The observer records cache leaf order, surface identities, pool order, voxel
//! memberships and all surface-leaf fields, in addition to the original dumps.

#[path = "geom_grid/native.rs"]
mod native;

#[cfg(test)]
#[path = "geom_grid/tests.rs"]
mod native_tests;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::grid::{
    ElementId, ElementSource, Grid, GridItem, HierarchyElement, HierarchyId, SurfaceGridElement,
};
use solidcore::geom::tree::NodeId;
use solidcore::kernel::Kernel;
use solidcore::math::{Point3, Vec3, BIG_DOUBLE};
use solidcore::SolidError;
use solidcore_ffi::units::geom_grid as ffi;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 600;

/// `geom.grid` runner.
pub struct Runner;

type P3 = [f64; 3];
type E6 = [f64; 6];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Poly,
    User,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GridDesc {
    bbox: E6,
    size: [i64; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Op {
    Add {
        kind: Kind,
        obj: i64,
        bbox: E6,
    },
    AddHier {
        surface: bool,
        obj: i64,
        tol: f64,
        bbox: E6,
    },
    Find {
        bbox: E6,
    },
    Address {
        p: P3,
    },
    VoxelBox {
        a: [i64; 3],
    },
    Setup {
        p: P3,
        v: P3,
    },
    Step {
        t: f64,
    },
    Dump,
    Expand {
        h: i64,
    },
    DefineSurface {
        id: i64,
        origin: P3,
        u: P3,
        v: P3,
        domain: [f64; 4],
        spans: [usize; 2],
    },
    AddCachedHier {
        surface: i64,
        tol: f64,
        bbox: E6,
    },
    ExpandCached {
        h: usize,
    },
    AddCachedLeaf {
        surface: i64,
        leaf: usize,
        bbox: E6,
    },
    ObserveCached,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    grid: GridDesc,
    ops: Vec<Op>,
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

fn ext(e: E6) -> Extent3d {
    Extent3d::from_bounds(e[0], e[1], e[2], e[3], e[4], e[5])
}

fn p3(p: P3) -> Point3 {
    Vec3::new(p[0], p[1], p[2])
}

fn put3(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn put_ext(o: &mut Vec<f64>, e: &Extent3d) {
    put3(o, e.min());
    put3(o, e.max());
}

/// Upstream status of a Rust error.
fn status_of(e: &SolidError) -> i64 {
    match e {
        SolidError::InvalidInput { .. } => 1007,
        SolidError::Upstream { code, .. } => i64::from(*code),
        _ => 1001,
    }
}

fn prog(i: &Inputs) -> Vec<f64> {
    let mut v = vec![0.0];
    v.extend(i.grid.bbox);
    v.extend(i.grid.size.map(|s| s as f64));
    for op in &i.ops {
        match op {
            Op::Add { kind, obj, bbox } => {
                v.push(1.0);
                v.push(if *kind == Kind::Poly { 1.0 } else { 3.0 });
                v.push(*obj as f64);
                v.extend(bbox);
            }
            Op::AddHier {
                surface,
                obj,
                tol,
                bbox,
            } => {
                v.extend([2.0, f64::from(u8::from(*surface)), *obj as f64, *tol]);
                v.extend(bbox);
            }
            Op::Find { bbox } => {
                v.push(3.0);
                v.extend(bbox);
            }
            Op::Address { p } => {
                v.push(4.0);
                v.extend(p);
            }
            Op::VoxelBox { a } => {
                v.push(5.0);
                v.extend(a.map(|x| x as f64));
            }
            Op::Setup { p, v: d } => {
                v.push(6.0);
                v.extend(p);
                v.extend(d);
            }
            Op::Step { t } => v.extend([7.0, *t]),
            Op::Dump => v.push(8.0),
            Op::Expand { h } => v.extend([9.0, *h as f64]),
            Op::DefineSurface {
                id,
                origin,
                u,
                v: axis_v,
                domain,
                spans,
            } => {
                v.extend([11.0, *id as f64]);
                v.extend(origin);
                v.extend(u);
                v.extend(axis_v);
                v.extend(domain);
                v.extend(spans.map(|n| n as f64));
            }
            Op::AddCachedHier { surface, tol, bbox } => {
                v.extend([12.0, *surface as f64, *tol]);
                v.extend(bbox);
            }
            Op::ExpandCached { h } => v.extend([13.0, *h as f64]),
            Op::AddCachedLeaf {
                surface,
                leaf,
                bbox,
            } => {
                v.extend([14.0, *surface as f64, *leaf as f64]);
                v.extend(bbox);
            }
            Op::ObserveCached => v.push(15.0),
        }
    }
    v
}

/// `(type code, object id)` of a grid element.
fn put_elem(o: &mut Vec<i64>, g: &Grid<i64>, id: ElementId) -> solidcore::Result<()> {
    let e = g.element(id)?;
    o.push(e.type_code());
    o.push(match e.item() {
        GridItem::Polygon(n) => n.0 as i64,
        GridItem::User(obj) => obj,
        GridItem::Hierarchy(_) | GridItem::Surface(_) => -2,
    });
    Ok(())
}

fn dump_dda(g: &Grid<i64>, oi: &mut Vec<i64>, of: &mut Vec<f64>) {
    let d = g.dda();
    oi.extend(d.address);
    let next = [d.next_t.x, d.next_t.y, d.next_t.z];
    for (k, n) in next.iter().enumerate() {
        if *n < BIG_DOUBLE {
            oi.push(d.step.get(k).copied().unwrap_or(0));
            oi.push(d.out.get(k).copied().unwrap_or(0));
        }
    }
    put3(of, d.ray_point);
    put3(of, d.ray_vector);
    of.push(d.ray_t);
    put3(of, d.curr_pos);
    put3(of, d.next_t);
    put3(of, d.delta_t);
    put3(of, d.delta_x_vec);
    put3(of, d.delta_y_vec);
    put3(of, d.delta_z_vec);
    for (n, p) in next.iter().zip([d.next_x, d.next_y, d.next_z]) {
        if *n < BIG_DOUBLE {
            put3(of, p);
        }
    }
}

/// Coverage counters of a Rust run (used by the generator test).
#[derive(Debug, Default)]
struct Stats {
    hits: usize,
    multi_hits: usize,
    exits: usize,
    misses: usize,
    errors: usize,
}

fn run_rust(i: &Inputs, st: &mut Stats) -> solidcore::Result<(Vec<i64>, Vec<f64>)> {
    run_rust_controlled(i, st, true, |leaf| leaf)
}

// Unit-local mutation controls are used only by the native integration tests.
// Normal runs always expand and preserve every field. No kernel path is changed.
fn run_rust_controlled(
    i: &Inputs,
    st: &mut Stats,
    expand: bool,
    mut leaf_control: impl FnMut(SurfaceGridElement<i64>) -> SurfaceGridElement<i64>,
) -> solidcore::Result<(Vec<i64>, Vec<f64>)> {
    if i.ops
        .iter()
        .any(|op| matches!(op, Op::DefineSurface { .. }))
    {
        ffi::validate(&prog(i)).map_err(|_| native::invalid("invalid explicit cache program"))?;
    }
    let size = i.grid.size.map(|s| s as usize);
    let mut g: Grid<i64> = Grid::new(&ext(i.grid.bbox), size)?;
    let mut caches = native::Caches::default();
    let (mut oi, mut of) = (Vec::new(), Vec::new());
    let mut ready = false;
    for op in &i.ops {
        match op {
            Op::Add { kind, obj, bbox } => {
                let src = match kind {
                    Kind::Poly => ElementSource::PolyBrep(NodeId(*obj as usize)),
                    Kind::User => ElementSource::UserData(*obj),
                };
                g.add_element(src, &ext(*bbox))?;
                oi.push(0);
            }
            Op::AddHier {
                surface,
                obj,
                tol,
                bbox,
            } => {
                let h = if *surface {
                    HierarchyElement::new_surface(*obj, *tol)
                } else {
                    HierarchyElement::new(*tol)
                };
                g.add_hierarchy_element(h, &ext(*bbox))?;
                oi.push(0);
            }
            Op::Find { bbox } => {
                let v = g.find_voxels_in_box(&ext(*bbox));
                oi.push(0);
                oi.push(v.len() as i64);
                for a in v {
                    oi.extend(a.map(|x| x as i64));
                }
            }
            Op::Address { p } => {
                let (a, out) = g.voxel_address(p3(*p));
                oi.extend(a.map(|x| x as i64));
                oi.push(i64::from(out));
            }
            Op::VoxelBox { a } => put_ext(&mut of, &g.voxel_bbox(*a)),
            Op::Setup { p, v } => match g.setup_3ddda(p3(*p), p3(*v)) {
                Ok(miss) => {
                    oi.push(0);
                    oi.push(i64::from(miss));
                    st.misses += usize::from(miss);
                    ready = !miss;
                    if ready {
                        dump_dda(&g, &mut oi, &mut of);
                    }
                }
                Err(e @ SolidError::Unported { .. }) => return Err(e),
                Err(e) => {
                    oi.push(status_of(&e));
                    oi.push(0);
                    st.errors += 1;
                    ready = false;
                }
            },
            Op::Step { t } => {
                if !ready {
                    oi.push(-3);
                    continue;
                }
                match g.step_3ddda(*t) {
                    Ok(h) => {
                        st.hits += usize::from(!h.elements.is_empty());
                        st.multi_hits += usize::from(h.elements.len() > 1);
                        st.exits += usize::from(h.exited_grid);
                        oi.push(0);
                        oi.push(i64::from(h.exited_grid));
                        oi.push(h.elements.len() as i64);
                        for id in &h.elements {
                            put_elem(&mut oi, &g, *id)?;
                        }
                        of.extend(&h.min_distances);
                        of.extend(&h.max_distances);
                        if h.exited_grid {
                            ready = false;
                        } else {
                            dump_dda(&g, &mut oi, &mut of);
                        }
                    }
                    Err(e @ SolidError::Unported { .. }) => return Err(e),
                    Err(e) => {
                        oi.push(status_of(&e));
                        ready = false;
                    }
                }
            }
            Op::Dump => dump(&g, &mut oi, &mut of)?,
            Op::Expand { h } => {
                let id = HierarchyId(*h as usize);
                // Match shim operation 9: skip opaque surface labels;
                // real surfaces are expanded with `expand_cached`.
                if g.hierarchy_element(id).is_ok_and(|e| e.surface().is_some()) {
                    oi.push(-2);
                    continue;
                }
                // A base-class element fails before it reads the cache.
                let kernel = Kernel::default();
                let mut cache = native::placeholder_cache(&kernel)?;
                match g.expand_hierarchy(&kernel, id, &mut cache) {
                    Ok(()) => oi.push(0),
                    Err(e @ SolidError::Unported { .. }) => return Err(e),
                    Err(e) => oi.push(status_of(&e)),
                }
            }
            Op::DefineSurface {
                id,
                origin,
                u,
                v,
                domain,
                spans,
            } => {
                caches.define(*id, *origin, *u, *v, *domain, *spans)?;
                oi.push(0);
            }
            Op::AddCachedHier { surface, tol, bbox } => {
                caches.add_hierarchy(&mut g, *surface, *tol, &ext(*bbox))?;
                oi.push(0);
            }
            Op::ExpandCached { h } => {
                if expand {
                    caches.expand(&mut g, HierarchyId(*h), &mut leaf_control)?;
                }
                oi.push(0);
            }
            Op::AddCachedLeaf {
                surface,
                leaf,
                bbox,
            } => {
                caches.add_leaf(&mut g, *surface, *leaf, &ext(*bbox), &mut leaf_control)?;
                oi.push(0);
            }
            Op::ObserveCached => caches.observe(&g, &mut oi, &mut of)?,
        }
    }
    Ok((oi, of))
}

fn dump(g: &Grid<i64>, oi: &mut Vec<i64>, of: &mut Vec<f64>) -> solidcore::Result<()> {
    let [nx, ny, nz] = g.size();
    let mut order: Vec<ElementId> = Vec::new();
    for ix in 0..nx {
        for iy in 0..ny {
            for iz in 0..nz {
                let v = g.voxel([ix, iy, iz])?;
                match v.grid_elements() {
                    None => oi.push(-1),
                    Some(list) => {
                        oi.push(list.len() as i64);
                        for &id in list {
                            put_elem(oi, g, id)?;
                            if !order.contains(&id) {
                                order.push(id);
                            }
                        }
                    }
                }
                match v.hierarchy_elements() {
                    None => oi.push(-1),
                    Some(list) => {
                        oi.push(list.len() as i64);
                        oi.extend(list.iter().map(|h| h.0 as i64));
                    }
                }
            }
        }
    }
    for id in order {
        put_elem(oi, g, id)?;
        let e = g.element(id)?;
        put3(of, e.sphere_center());
        of.push(e.sphere_radius());
        put_ext(of, e.bounding_box());
    }
    for h in g.hierarchy_elements() {
        let ge = h.grid_element();
        oi.push(ge.type_code());
        oi.push(match ge.item() {
            GridItem::Hierarchy(Some(id)) => id.0 as i64,
            GridItem::Hierarchy(None) => -1,
            _ => -2,
        });
        put3(of, ge.sphere_center());
        of.push(ge.sphere_radius());
        put_ext(of, ge.bounding_box());
    }
    put3(of, g.voxel_size());
    of.push(g.voxel_tolerance());
    oi.push(g.current_mark() as i64);
    Ok(())
}

// ---- generation -------------------------------------------------------

fn at(a: &[f64], k: usize) -> f64 {
    a.get(k).copied().unwrap_or(0.0)
}

fn set(a: &mut [f64], k: usize, v: f64) {
    if let Some(x) = a.get_mut(k) {
        *x = v;
    }
}

struct Gen {
    lo: P3,
    hi: P3,
    size: [i64; 3],
}

impl Gen {
    fn ext_len(&self, k: usize) -> f64 {
        at(&self.hi, k) - at(&self.lo, k)
    }

    fn diag(&self) -> f64 {
        (0..3)
            .map(|k| self.ext_len(k) * self.ext_len(k))
            .sum::<f64>()
            .sqrt()
    }

    fn center(&self) -> P3 {
        [0, 1, 2].map(|k| 0.5 * (at(&self.lo, k) + at(&self.hi, k)))
    }

    /// A point inside the grid box grown by `grow` (fraction of each side).
    fn point_in(&self, rng: &mut Rng, grow: f64) -> P3 {
        [0, 1, 2].map(|k| {
            let e = self.ext_len(k) * grow;
            rng.uniform(at(&self.lo, k) - e, at(&self.hi, k) + e)
        })
    }

    /// A voxel-lattice point (a corner shared by voxels), possibly on the boundary.
    fn lattice_point(&self, rng: &mut Rng) -> P3 {
        [0, 1, 2].map(|k| {
            let n = self.size.get(k).copied().unwrap_or(1);
            let j = rng.range_i64(0, n);
            at(&self.lo, k) + j as f64 * (self.ext_len(k) / n as f64)
        })
    }

    fn rand_box(&self, rng: &mut Rng) -> E6 {
        let c = self.point_in(rng, 0.2);
        let kind = rng.below(20);
        let mut b = [0.0; 6];
        for k in 0..3 {
            let s = match kind {
                0 => 0.0,
                1 => -rng.uniform(0.0, 0.1) * self.ext_len(k),
                _ => rng.uniform(0.0, 0.4) * self.ext_len(k),
            };
            set(&mut b, k, at(&c, k));
            set(&mut b, k + 3, at(&c, k) + s);
        }
        b
    }
}

fn gen_dir(rng: &mut Rng, g: &Gen, start: P3, boxes: &[E6]) -> P3 {
    if !boxes.is_empty() && rng.chance(0.5) {
        let b = boxes
            .get(rng.below(boxes.len() as u64) as usize)
            .copied()
            .unwrap_or_default();
        let t = [0, 1, 2].map(|k| at(&b, k) + rng.unit() * (at(&b, k + 3) - at(&b, k)));
        let d = [t[0] - start[0], t[1] - start[1], t[2] - start[2]];
        if d.iter().any(|x| *x != 0.0) {
            return d;
        }
    }
    match rng.below(10) {
        0 | 1 => {
            let k = rng.below(3) as usize;
            let mut d = [0.0; 3];
            let sign = if rng.chance(0.5) { 1.0 } else { -1.0 };
            set(&mut d, k, sign * rng.uniform(0.1, 3.0));
            d
        }
        2 => {
            let s = |rng: &mut Rng| if rng.chance(0.5) { 1.0 } else { -1.0 };
            [s(rng), s(rng), s(rng)]
        }
        3 => {
            let k = rng.below(3) as usize;
            let mut d = [
                rng.uniform(-1.0, 1.0),
                rng.uniform(-1.0, 1.0),
                rng.uniform(-1.0, 1.0),
            ];
            set(&mut d, k, 0.0);
            d
        }
        4 if rng.chance(0.3) => [rng.uniform(-1e-7, 1e-7), 0.0, 0.0],
        _ => {
            let t = if rng.chance(0.5) {
                g.point_in(rng, 0.0)
            } else {
                g.lattice_point(rng)
            };
            let d = [t[0] - start[0], t[1] - start[1], t[2] - start[2]];
            if d.iter().all(|x| *x == 0.0) {
                [1.0, 0.5, 0.25]
            } else {
                d
            }
        }
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let scale = *rng.choose(&[1.0, 10.0, 1000.0, 1e-3]).unwrap_or(&1.0);
    let size = [
        rng.range_i64(1, 8),
        rng.range_i64(1, 8),
        rng.range_i64(1, 8),
    ];
    let (lo, hi) = if rng.chance(0.35) {
        let o = rng.choose(&[0.0, -1.0, 2.0]).copied().unwrap_or(0.0) * scale;
        ([o, o, o], size.map(|n| o + n as f64 * scale))
    } else {
        let c = rng.point3(-scale, scale);
        let e = [0, 1, 2].map(|_| rng.uniform(0.2, 2.0) * scale);
        (c, [c[0] + e[0], c[1] + e[1], c[2] + e[2]])
    };
    let g = Gen { lo, hi, size };
    let mut ops = Vec::new();

    let n_elems = rng.range_i64(0, 16);
    let mut boxes = Vec::new();
    for obj in 0..n_elems {
        let kind = if rng.chance(0.5) {
            Kind::Poly
        } else {
            Kind::User
        };
        let bbox = match boxes.last() {
            // a box overlapping the previous one (several hits in one voxel)
            Some(prev) if rng.chance(0.3) => {
                let p: E6 = *prev;
                let j = [0, 1, 2].map(|k| (at(&p, k + 3) - at(&p, k)) * rng.uniform(-0.5, 0.5));
                [
                    p[0] + j[0],
                    p[1] + j[1],
                    p[2] + j[2],
                    p[3] + j[0],
                    p[4] + j[1],
                    p[5] + j[2],
                ]
            }
            _ => g.rand_box(rng),
        };
        boxes.push(bbox);
        ops.push(Op::Add { kind, obj, bbox });
    }
    let n_hier = rng.range_i64(0, 3);
    for h in 0..n_hier {
        ops.push(Op::AddHier {
            surface: rng.chance(0.5),
            obj: 100 + h,
            tol: rng.uniform(0.0, 0.05) * scale,
            bbox: g.rand_box(rng),
        });
    }
    if n_hier > 0 && rng.chance(0.2) {
        // Only base-class elements: a surface element would defer the case.
        ops.push(Op::Expand { h: -1 });
    }
    for _ in 0..rng.range_i64(0, 3) {
        match rng.below(3) {
            0 => ops.push(Op::Find {
                bbox: g.rand_box(rng),
            }),
            1 => {
                let p = if rng.chance(0.3) {
                    g.lattice_point(rng)
                } else {
                    g.point_in(rng, 0.5)
                };
                ops.push(Op::Address { p });
            }
            _ => ops.push(Op::VoxelBox {
                a: size.map(|n| rng.range_i64(-1, n)),
            }),
        }
    }
    if rng.chance(0.3) {
        ops.push(Op::Dump);
    }

    let max_steps = size.iter().sum::<i64>() + 2;
    for _ in 0..rng.range_i64(1, 4) {
        let start = match rng.below(5) {
            0 | 1 => {
                // outside, on a sphere around the grid
                let c = g.center();
                let d = [
                    rng.uniform(-1.0, 1.0),
                    rng.uniform(-1.0, 1.0),
                    rng.uniform(-1.0, 1.0),
                ];
                let r = g.diag() * rng.uniform(0.6, 2.0);
                [c[0] + d[0] * r, c[1] + d[1] * r, c[2] + d[2] * r]
            }
            2 => g.point_in(rng, 0.0),
            3 => g.lattice_point(rng),
            _ => g.point_in(rng, 0.3),
        };
        let v = gen_dir(rng, &g, start, &boxes);
        ops.push(Op::Setup { p: start, v });
        for _ in 0..max_steps {
            let t = if rng.chance(0.8) {
                BIG_DOUBLE
            } else {
                rng.uniform(-0.5, 2.0) * g.diag()
            };
            ops.push(Op::Step { t });
        }
    }
    ops.push(Op::Dump);
    // Resolve the placeholder expand index.
    let mut nh = 0i64;
    let mut base = Vec::new();
    for op in &ops {
        if let Op::AddHier { surface, .. } = op {
            if !surface {
                base.push(nh);
            }
            nh += 1;
        }
    }
    ops.retain_mut(|op| match op {
        Op::Expand { h } => match base.first() {
            Some(b) => {
                *h = *b;
                true
            }
            None => false,
        },
        _ => true,
    });
    Inputs {
        grid: GridDesc {
            bbox: [lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]],
            size,
        },
        ops,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "geom.grid"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let (oi, of) = ffi::run(&prog(&i)).map_err(|e| harness(e.to_string()))?;
        Ok(Output::new().ints("ints", oi).values("floats", of))
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        match run_rust(&i, &mut Stats::default()) {
            Ok((oi, of)) => Ok(Output::new().ints("ints", oi).values("floats", of)),
            Err(e) => {
                Ok(Output::unported(&e).unwrap_or_else(|| Output::err(format!("rust_error: {e}"))))
            }
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generator_covers_walks() {
        let mut st = Stats::default();
        for k in 0..MIN_GENERATED as u64 {
            let mut rng = Rng::new(k);
            let i = gen_case(&mut rng);
            assert!(run_rust(&i, &mut st).is_ok());
        }
        assert!(st.hits > 300, "{st:?}");
        assert!(st.multi_hits > 80, "{st:?}");
        assert!(st.exits > 300, "{st:?}");
        assert!(st.misses > 20, "{st:?}");
        assert!(st.errors > 5, "{st:?}");
    }
}
