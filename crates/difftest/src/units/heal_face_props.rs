//! Runner for unit `heal.face_props`: `SmFaceProps`, `SmLoopProps` and the
//! `SmLooptreeItem` tree.
//!
//! C++: `solidcore_ffi::units::heal_face_props` (upstream `SmFaceProps.cpp`,
//! `SmLoopProps.cpp`) on a one-face B-rep (a plane with one vertex loop). Rust:
//! `solidcore::heal::face_props`.
//!
//! Two kinds of case. `props`: a script of ops on one face props object (`SetProps` and its
//! three stages, `ReSet`, `HasProps`, `HasProblems`, `IsPoint3dOnPole`, field seeds); the outputs are
//! each op's result and a snapshot of the props after it. `tree`: building a loop tree with
//! `AddChild` / `AddSibling` / `AddPartner` and `FindItem` queries (fake loop numbers).
//!
//! Deferred: every stage 2 or 3 computation (`SmSurface::GetSingularities`,
//! `surface.surface_p3`) and loop props (`SmLoop::GetEdges`, `topology.loopuse`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::Vec2;
use solidcore::geom::vector3d::Vec3;
use solidcore::heal::face_props::{
    FaceProps, HealerOpType, ItemId, LoopObject, Looptree, LooptreeItem, PropStage,
};
use solidcore::intersect::point_class::{ClassObject, ClassObjectKind};
use solidcore::kernel::Kernel;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::{
    Brep, Face, FaceId, Faceuse, LoopId, Region, Shell, ShellId,
};
use solidcore_ffi::units::heal_face_props as ffi;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1500;

/// `heal.face_props` runner.
pub struct Runner;

#[derive(Debug, Clone, Copy, PartialEq)]
struct H(LoopId);

impl ClassObject for H {
    fn class_kind(&self) -> ClassObjectKind {
        ClassObjectKind::Loop
    }
}

impl LoopObject for H {
    fn from_loop(l: LoopId) -> Self {
        Self(l)
    }
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FaceIn {
    tol: f64,
    o: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    /// `[umin, vmin, umax, vmax]`.
    domain: [f64; 4],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CaseIn {
    Props {
        face: FaceIn,
        /// `[op, p0, p1, p2, p3]` per op.
        ops: Vec<[f64; 5]>,
    },
    Tree {
        /// `[loop, partner]` per item (`-1` pole vertex; partner `-2` none).
        items: Vec<[i64; 2]>,
        /// `[kind (0 child, 1 sibling), this, new]`.
        links: Vec<[i64; 3]>,
        /// `[start item, loop]`.
        finds: Vec<[i64; 2]>,
    },
}

fn ffi_face(f: &FaceIn) -> ffi::FaceSpec {
    ffi::FaceSpec {
        tol: f.tol,
        o: f.o,
        x: f.x,
        y: f.y,
        domain: f.domain,
    }
}

fn ffi_ops(ops: &[[f64; 5]]) -> Vec<ffi::Op> {
    ops.iter()
        .map(|o| ffi::Op {
            op: o[0] as i64,
            p: [o[1], o[2], o[3], o[4]],
        })
        .collect()
}

// ---- C++ ----------------------------------------------------------------------------

fn cpp(c: &CaseIn) -> Result<Output, HarnessError> {
    match c {
        CaseIn::Props { face, ops } => {
            let r = ffi::run(&ffi_face(face), &ffi_ops(ops)).map_err(|e| harness(e.to_string()))?;
            Ok(Output::new().values("v", r))
        }
        CaseIn::Tree {
            items,
            links,
            finds,
        } => {
            let r = tree_ffi(items, links, finds)?;
            Ok(Output::new().values("v", r))
        }
    }
}

fn tree_ffi(
    items: &[[i64; 2]],
    links: &[[i64; 3]],
    finds: &[[i64; 2]],
) -> Result<Vec<f64>, HarnessError> {
    let items: Vec<ffi::TreeItem> = items
        .iter()
        .map(|i| ffi::TreeItem {
            loop_: i[0],
            partner: i[1],
        })
        .collect();
    let links: Vec<(i64, i64, i64)> = links.iter().map(|l| (l[0], l[1], l[2])).collect();
    let finds: Vec<(i64, i64)> = finds.iter().map(|f| (f[0], f[1])).collect();
    ffi::tree(&items, &links, &finds).map_err(|e| harness(e.to_string()))
}

// ---- Rust ---------------------------------------------------------------------------

fn v3(a: [f64; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}

fn rust_brep(f: &FaceIn) -> Result<(Brep, FaceId), HarnessError> {
    let mut b = Brep::new();
    let e = |e: solidcore::SolidError| harness(e.to_string());
    let region = b
        .add_region_record(Region { in_brep: true })
        .map_err(e)?;
    let plane = Surface::from(Plane::new(
        v3(f.o),
        v3(f.x),
        v3(f.y),
        Vec2::new(1.0, 1.0),
        &Extent2d::from_bounds(f.domain[0], f.domain[1], f.domain[2], f.domain[3]),
    ));
    let shell = b
        .add_shell_record(Shell {
            faces: vec![FaceId::from_index(0)],
            region: Some(region),
        })
        .map_err(e)?;
    let face = b
        .add_face_record(Face {
            surface: Some(plane),
            faceuse: Some(solidcore::topology::FaceuseId::from_index(0)),
            tolerance: f.tol,
            ..Face::new(shell)
        })
        .map_err(e)?;
    let fu = |mate: u32| Faceuse {
        shell: Some(ShellId::from_index(0)),
        face: Some(face),
        mate: Some(solidcore::topology::FaceuseId::from_index(mate)),
        ..Faceuse::default()
    };
    b.add_faceuse_record(fu(1)).map_err(e)?;
    b.add_faceuse_record(fu(0)).map_err(e)?;
    Ok((b, face))
}

fn status(r: solidcore::Result<()>) -> Result<f64, solidcore::SolidError> {
    match r {
        Ok(()) => Ok(1000.0),
        Err(e) if e.is_unported() => Err(e),
        Err(solidcore::SolidError::Upstream { code, .. }) => Ok(f64::from(code)),
        Err(e) => Err(e),
    }
}

fn snapshot(p: &FaceProps<H>, face: FaceId) -> Vec<f64> {
    let t = |x: solidcore::heal::face_props::Tri| x.code() as f64;
    let mut o: Vec<f64> = vec![p.prop_stage.code() as f64];
    o.push(p.not_yet_face_type.code() as f64);
    o.push(t(p.bad_has_not_yet_prob));
    o.push(t(p.bad_degen_face));
    o.push(match p.face {
        None => 0.0,
        Some(f) if f == face => 1.0,
        Some(_) => 2.0,
    });
    o.push(p.face_index as f64);
    o.push(p.zone_tol_3d);
    o.push(p.orig_zone_tol_3d);
    o.push(t(p.bad_small_zone_tol_3d));
    o.push(t(p.bad_large_zone_tol_3d));
    o.push(f64::from(u8::from(p.has_surface)));
    o.push(p.sheet.code() as f64);
    o.push(t(p.closed_u));
    o.push(t(p.closed_v));
    o.push(t(p.closed_surf));
    o.push(f64::from(p.closed_val_u as i32));
    o.push(f64::from(p.closed_val_v as i32));
    o.push(p.poles_link.code() as f64);
    o.push(p.poles as f64);
    o.push(p.approx_poles as f64);
    o.push(p.no_vertex_poles as f64);
    o.push(p.bad_flat_corners as f64);
    o.push(f64::from(u8::from(p.crv_class_u.is_some())));
    o.push(f64::from(u8::from(p.crv_class_v.is_some())));
    o.push(t(p.bad_crossed_seam));
    o.push(f64::from(p.bad_crossed_seam_dir as i32));
    o.push(t(p.bad_missing_seam));
    o.push(f64::from(p.bad_missing_seam_dir as i32));
    o.push(t(p.bad_near_miss_seam));
    o.push(f64::from(p.bad_near_miss_seam_dir as i32));
    o.push(p.been_through_move_seam.code() as f64);
    o.push(p.been_through_split_edges_at_seam.code() as f64);
    o.push(p.been_through_split_faces_at_seam.code() as f64);
    o.push(p.closed_loop_cnt as f64);
    o.push(p.outer_loop_cnt as f64);
    o.push(t(p.bad_loop_problems));
    o.push(t(p.bad_outer_loop_order));
    o.push(p.bad_orient_loop_cnt as f64);
    o.push(p.bad_no_area_loop_cnt as f64);
    o.push(p.bad_nested_loop_cnt as f64);
    o.push(t(p.bad_unpaired_no_area_loop));
    o.push(p.bad_closed_3d_loop_cnt as f64);
    o.push(p.bad_closed_ptr_loop_cnt as f64);
    o.push(p.bad_missing_poles as f64);
    o.push(p.loop_props.len() as f64);
    o.push(p.pole_points.len() as f64);
    for i in 0..4 {
        match p.pole_points.get(i) {
            Some(v) => o.extend([v.x, v.y, v.z]),
            None => o.extend([0.0; 3]),
        }
    }
    o.push(p.natural_uv_domain.u_min());
    o.push(p.natural_uv_domain.v_min());
    o.push(p.natural_uv_domain.u_max());
    o.push(p.natural_uv_domain.v_max());
    o.push(p.flat_corner_points.len() as f64);
    o.resize(ffi::SNAP_LEN, 0.0);
    o
}

fn seed(p: &mut FaceProps<H>, face: FaceId, field: i64, idx: i64, v: f64) {
    use solidcore::heal::face_props::{MixedLinkType as M, Tri};
    let tri = |x: f64| match x as i64 {
        0 => Tri::False,
        1 => Tri::True,
        _ => Tri::Unsure,
    };
    let mixed = |x: f64| M::from_code(x as i64).unwrap_or(M::Undef);
    match field {
        0 => p.prop_stage = PropStage::from_code(v as i64).unwrap_or(PropStage::Stage0),
        1 => p.zone_tol_3d = v,
        2 => p.orig_zone_tol_3d = v,
        3 => p.poles = v as u64,
        4 => {
            if let Some(pt) = usize::try_from(idx).ok().and_then(|i| p.pole_points.get_mut(i)) {
                *pt = Vec3::new(v, v * 0.5, -v);
            }
        }
        5 => p.bad_small_zone_tol_3d = tri(v),
        6 => p.bad_large_zone_tol_3d = tri(v),
        7 => p.sheet = mixed(v),
        8 => p.poles_link = mixed(v),
        9 => p.bad_flat_corners = v as u64,
        10 => p.bad_crossed_seam = tri(v),
        11 => p.bad_missing_seam = tri(v),
        12 => p.outer_loop_cnt = v as u64,
        13 => p.bad_orient_loop_cnt = v as u64,
        14 => p.bad_no_area_loop_cnt = v as u64,
        15 => p.bad_nested_loop_cnt = v as u64,
        16 => p.bad_unpaired_no_area_loop = tri(v),
        17 => p.bad_closed_3d_loop_cnt = v as u64,
        18 => p.bad_closed_ptr_loop_cnt = v as u64,
        19 => p.bad_missing_poles = v as u64,
        20 => p.bad_outer_loop_order = tri(v),
        21 => {
            p.not_yet_face_type =
                solidcore::heal::face_props::HealNotYetFaceType::from_code(v as i64)
                    .unwrap_or_default();
        }
        22 => p.closed_u = tri(v),
        23 => p.closed_v = tri(v),
        24 => p.face = if v == 0.0 { None } else { Some(face) },
        25 => p.bad_has_not_yet_prob = tri(v),
        _ => {}
    }
}

fn rust_props(f: &FaceIn, ops: &[[f64; 5]]) -> Result<Output, HarnessError> {
    ffi::validate(&ffi_face(f), &ffi_ops(ops)).map_err(|e| harness(e.to_string()))?;
    let k = Kernel::default();
    let (b, face) = rust_brep(f)?;
    let mut p: FaceProps<H> = FaceProps::new();
    let mut out: Vec<f64> = Vec::new();
    for o in ops {
        let op = o[0] as i64;
        let heal = HealerOpType::from_code(o[2] as i64).unwrap_or_default();
        let stage = PropStage::from_code(o[1] as i64).unwrap_or_default();
        let res: Result<f64, solidcore::SolidError> = match op {
            0 => status(p.set_props(&k, &b, stage, face, o[3] as u64, heal)),
            1 => {
                p.reset(stage);
                Ok(0.0)
            }
            2 => Ok(f64::from(u8::from(
                p.has_props(stage, (o[2] != 0.0).then_some(face)),
            ))),
            3 => Ok(f64::from(u8::from(p.has_problems(o[1] as u64)))),
            4 => Ok(f64::from(u8::from(
                p.is_point3d_on_pole(Vec3::new(o[1], o[2], o[3])),
            ))),
            5 => {
                seed(&mut p, face, o[1] as i64, o[2] as i64, o[3]);
                Ok(0.0)
            }
            6 => match o[1] as i64 {
                1 => status(p.set_props_gaps(&k, &b, face, o[3] as u64, heal)),
                2 => status(p.set_props_stage2(&k, &b, face, o[3] as u64, heal)),
                _ => status(p.set_props_stage3(&k, &b, face, o[3] as u64, heal)),
            },
            7 => Ok(f64::from(u8::from(p.is_periodic_u())) + 2.0 * f64::from(u8::from(p.is_periodic_v()))),
            _ => Err(solidcore::SolidError::invalid_input("runner", "op")),
        };
        match res {
            Ok(r) => {
                out.push(r);
                out.extend(snapshot(&p, face));
            }
            Err(e) => {
                return Ok(Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string())));
            }
        }
    }
    Ok(Output::new().values("v", out))
}

fn rust_tree(
    items: &[[i64; 2]],
    links: &[[i64; 3]],
    finds: &[[i64; 2]],
) -> Result<Output, HarnessError> {
    // the same guards as the FFI wrapper
    let it: Vec<ffi::TreeItem> = items
        .iter()
        .map(|i| ffi::TreeItem {
            loop_: i[0],
            partner: i[1],
        })
        .collect();
    let lk: Vec<(i64, i64, i64)> = links.iter().map(|l| (l[0], l[1], l[2])).collect();
    let fd: Vec<(i64, i64)> = finds.iter().map(|f| (f[0], f[1])).collect();
    ffi::tree_validate(&it, &lk, &fd).map_err(|e| harness(e.to_string()))?;

    let lid = |i: i64| LoopId::from_index(i as u32);
    let mut t = Looptree::new();
    let mut ids: Vec<ItemId> = Vec::new();
    for i in items {
        let mut item = LooptreeItem::with_loop(if i[0] < 0 { None } else { Some(lid(i[0])) }, None, None);
        if i[1] != -2 {
            item.add_partner(if i[1] < 0 { None } else { Some(lid(i[1])) }, None, None);
        }
        ids.push(t.push(item));
    }
    let id = |i: i64| -> Result<ItemId, HarnessError> {
        usize::try_from(i)
            .ok()
            .and_then(|i| ids.get(i).copied())
            .ok_or_else(|| harness("item"))
    };
    for l in links {
        let (a, c) = (id(l[1])?, id(l[2])?);
        let r = if l[0] == 0 { t.add_child(a, c) } else { t.add_sibling(a, c) };
        r.map_err(|e| harness(e.to_string()))?;
    }
    let mut out: Vec<f64> = Vec::new();
    for f in finds {
        let found = t
            .find_item(id(f[0])?, if f[1] < 0 { None } else { Some(lid(f[1])) })
            .map_err(|e| harness(e.to_string()))?;
        let pos = |x: ItemId| ids.iter().position(|y| *y == x).map_or(-1.0, |p| p as f64);
        match found {
            Some(r) => {
                out.push(pos(r.item));
                out.push(r.parent.map_or(-1.0, pos));
            }
            None => out.extend([-1.0, -1.0]),
        }
    }
    for i in &ids {
        out.push(f64::from(u8::from(
            t.item(*i).map_err(|e| harness(e.to_string()))?.is_no_area_pair(),
        )));
    }
    Ok(Output::new().values("v", out))
}

fn rust(c: &CaseIn) -> Result<Output, HarnessError> {
    match c {
        CaseIn::Props { face, ops } => rust_props(face, ops),
        CaseIn::Tree {
            items,
            links,
            finds,
        } => rust_tree(items, links, finds),
    }
}

// ---- generation ---------------------------------------------------------------------

fn gen_face(rng: &mut Rng) -> FaceIn {
    let tols = [1e-5, 1e-6, 2e-5, 1e-4, 1e-3, 5e-6, 1e-7, 1e-5 + 1e-12];
    let x = [1.0, 0.0, 0.0];
    let y = [0.0, 1.0, 0.0];
    let a = rng.uniform(-3.0, 0.0);
    let b = rng.uniform(-3.0, 0.0);
    FaceIn {
        tol: *rng.choose(&tols).unwrap_or(&1e-5),
        o: rng.point3(-2.0, 2.0),
        x,
        y,
        domain: [a, b, a + rng.uniform(0.5, 4.0), b + rng.uniform(0.5, 4.0)],
    }
}

fn gen_op(rng: &mut Rng) -> [f64; 5] {
    let heal_ops = [0, 1, 4, 5, 6, 12, 17, 18, 19, 20, 21, 26];
    let heal = f64::from(*rng.choose(&heal_ops).unwrap_or(&26));
    let stage = rng.range_i64(0, 6) as f64;
    match rng.below(14) {
        0 | 1 => [0.0, stage, heal, rng.range_i64(0, 5) as f64, 0.0],
        2 => [1.0, stage, 0.0, 0.0, 0.0],
        3 => [2.0, stage, rng.range_i64(0, 1) as f64, 0.0, 0.0],
        4 => [3.0, rng.range_i64(0, 7) as f64, 0.0, 0.0, 0.0],
        5 => {
            let p = rng.point3(-2.0, 2.0);
            [4.0, p[0], p[1], p[2], 0.0]
        }
        6 => [6.0, rng.range_i64(1, 3) as f64, heal, rng.range_i64(0, 5) as f64, 0.0],
        7 => [7.0, 0.0, 0.0, 0.0, 0.0],
        _ => {
            // a seed; the stage seed is followed by a face seed (a stage past the gaps
            // with no face would be dereferenced upstream)
            let field = rng.range_i64(0, 25);
            let v = match field {
                1 | 2 => *rng.choose(&[1e-5, 1e-3, 0.0, 0.5]).unwrap_or(&1e-5),
                3 | 9 => rng.range_i64(0, 15) as f64,
                4 => rng.uniform(-1.0, 1.0),
                7 | 8 => rng.range_i64(0, 5) as f64,
                12..=15 | 17..=19 | 21 => rng.range_i64(0, 3) as f64,
                24 => 1.0,
                _ => rng.range_i64(0, 2) as f64,
            };
            [5.0, field as f64, rng.range_i64(0, 3) as f64, v, 0.0]
        }
    }
}

fn gen_props(rng: &mut Rng) -> CaseIn {
    let face = gen_face(rng);
    let n = rng.range_i64(1, 8);
    let mut ops: Vec<[f64; 5]> = Vec::new();
    for _ in 0..n {
        let mut o = gen_op(rng);
        // seeds of the stage keep the face (a stage past the gaps with no face would be
        // dereferenced upstream) and stay below stage 2 (seeded stage 2 props have no seam
        // classifications, which stage 3 upstream dereferences)
        if o[0] == 5.0 && o[1] == 0.0 {
            o[3] = rng.range_i64(0, 2) as f64;
            ops.push([5.0, 24.0, 0.0, 1.0, 0.0]);
        }
        if o[0] == 1.0 && o[1] >= 3.0 {
            // a reset past the gaps keeps the face pointer but leaves a stage past the gaps
            // (upstream then dereferences a null face on the next stage 2)
            ops.push([5.0, 24.0, 0.0, 1.0, 0.0]);
        }
        ops.push(o);
    }
    // seeded state is not what stage 3 upstream expects: scripts with seeds stop short of
    // the loop classification (heal op before Fix_SplitEdgesAtSeam)
    if ops.iter().any(|o| o[0] == 5.0) {
        for o in &mut ops {
            let stage3 = (o[0] == 0.0 && o[1] >= 5.0) || (o[0] == 6.0 && o[1] == 3.0);
            if stage3 && o[2] >= 19.0 {
                o[2] = 6.0;
            }
        }
    }
    CaseIn::Props { face, ops }
}

fn gen_tree(rng: &mut Rng) -> CaseIn {
    let n = rng.range_i64(1, 8);
    let mut items: Vec<[i64; 2]> = Vec::new();
    for i in 0..n {
        let loop_ = if rng.chance(0.1) { -1 } else { i };
        let partner = if rng.chance(0.3) {
            if rng.chance(0.2) { -1 } else { 20 + i }
        } else {
            -2
        };
        items.push([loop_, partner]);
    }
    // a forest: item i (i >= 1) is linked once under an earlier item
    let mut links: Vec<[i64; 3]> = Vec::new();
    for i in 1..n {
        if rng.chance(0.9) {
            links.push([rng.range_i64(0, 1), rng.range_i64(0, i - 1), i]);
        }
    }
    let mut finds: Vec<[i64; 2]> = Vec::new();
    for _ in 0..rng.range_i64(1, 6) {
        let l = if rng.chance(0.1) {
            -1
        } else if rng.chance(0.2) {
            20 + rng.range_i64(0, n)
        } else {
            rng.range_i64(0, n)
        };
        finds.push([rng.range_i64(0, n - 1), l]);
    }
    CaseIn::Tree {
        items,
        links,
        finds,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "heal.face_props"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let c = if rng.chance(0.8) {
            gen_props(rng)
        } else {
            gen_tree(rng)
        };
        serde_json::to_value(c).ok()
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
