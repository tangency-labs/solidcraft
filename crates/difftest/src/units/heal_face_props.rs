//! Runner for unit `heal.face_props`: `SmFaceProps`, `SmLoopProps` and the
//! `SmLooptreeItem` tree.
//!
//! C++: `solidcore_ffi::units::heal_face_props` (upstream `SmFaceProps.cpp`,
//! `SmLoopProps.cpp`) on a one-face B-rep built with `SmBrepConstructor`. Rust:
//! `solidcore::heal::face_props` on the same B-rep, rebuilt record by record by
//! [`rust_brep`] (same surface, owned by the face, same vertex loop or edge loops and holes,
//! same face tolerance).
//!
//! Two kinds of case. `props`: a script of ops on one face props object (`SetProps` and its
//! three stages, `ReSet`, `HasProps`, `HasProblems`, `IsPoint3dOnPole`, field seeds, and
//! `SmLoopProps::SetProps` of every loop); the outputs are each op's result and a snapshot of
//! the props after it. `tree`: building a loop tree with `AddChild` / `AddSibling` /
//! `AddPartner` and `FindItem` queries (fake loop numbers).
//!
//! **Faces.** A plane or a surface of revolution (`CreateCanonical`: a cylinder, a cone with
//! or without apex, a sphere or a torus, with the swept angle either 360 degrees, a closed
//! surface, or below it, an open one that `AdjustSTEPUVDomain` trims), with a vertex loop
//! (at the origin, or at a pole) or edge loops (an outer loop and holes of line edges). The
//! face tolerance reaches `GetSingularities` through `SmFace::GetTolerance`.
//!
//! **Scripts are guarded.** [`ffi::validate`] runs on both sides and rejects what crashes or is
//! undefined upstream (including the scripts of UB-131: `SetProps_Stage2` / `Stage3` on a
//! stage past `GAPSa` with no face pointer, `plan/upstream-issues.md`). A fixture case with
//! `expect_rejected` is a regression case for the guard: both sides must reject it (the
//! error `rejected`); the C++ is never called for it.
//!
//! Deferred: closed faces (`SetProps_Stage2` reaches `SmFace::HasSeamProblem`,
//! `topology.face_p4`) and every `SetProps_Stage3` (`SmFace::ClassifyLoops`, `query.face_p4`).
//! The loop-props loop of Stage 3 is compared by op 8 instead, without the classification.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::curve::line::Line;
use solidcore::geom::attribute::ObjectKey;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::object::ObjectRef;
use solidcore::geom::vector2d::Vec2;
use solidcore::geom::vector3d::Vec3;
use solidcore::heal::face_props::{
    orient_code, FaceProps, HealerOpType, ItemId, LoopObject, LoopProps, Looptree, LooptreeItem,
    PropStage,
};
use solidcore::intersect::point_class::{ClassObject, ClassObjectKind};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::kernel::Kernel;
use solidcore::surface::plane::Plane;
use solidcore::surface::surf_of_revolution::SurfOfRevolution;
use solidcore::surface::surface::Surface;
use solidcore::topology::face::p2 as fp2;
use solidcore::topology::{
    Brep, Edge, EdgeId, Edgeuse, EdgeuseId, EdgeuseOwner, Face, FaceId, Faceuse, FaceuseId, Loop,
    LoopId, Loopuse, LoopuseId, LoopuseStart, Region, Shell, VertexId, Vertexuse,
    VertexuseId, VertexuseOwner,
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

/// The generator of a surface of revolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum GenIn {
    Line {
        a: [f64; 3],
        b: [f64; 3],
    },
    Circle {
        c: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        d: [f64; 2],
        r: f64,
    },
}

/// The face's surface when it is not the plane `o`, `x`, `y`, `domain` of [`FaceIn`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SurfIn {
    Revolution {
        gen: GenIn,
        point: [f64; 3],
        axis: [f64; 3],
        #[serde(default)]
        swap: bool,
        /// Swept angle in degrees; below 360 the angle domain is adjusted to it.
        sweep: f64,
    },
}

/// The face's loops when they are not the one vertex loop at the origin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum LoopsIn {
    Vertex { at: [f64; 3] },
    /// Edge loops (the first one outer, the others holes): corner points joined by lines.
    Edges { loops: Vec<Vec<[f64; 3]>> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FaceIn {
    tol: f64,
    o: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    /// `[umin, vmin, umax, vmax]`.
    domain: [f64; 4],
    #[serde(default)]
    surf: Option<SurfIn>,
    #[serde(default)]
    loops: Option<LoopsIn>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)] // test inputs, built once per case
enum CaseIn {
    Props {
        face: FaceIn,
        /// `[op, p0, p1, p2, p3]` per op.
        ops: Vec<[f64; 5]>,
        /// A regression case for the FFI guard: both sides must reject the script.
        #[serde(default)]
        expect_rejected: bool,
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

fn ffi_gen(g: &GenIn) -> ffi::Gen {
    match g {
        GenIn::Line { a, b } => ffi::Gen::Line { a: *a, b: *b },
        GenIn::Circle { c, x, y, d, r } => ffi::Gen::Circle {
            c: *c,
            x: *x,
            y: *y,
            d: *d,
            r: *r,
        },
    }
}

fn ffi_face(f: &FaceIn) -> ffi::FaceSpec {
    let surf = match &f.surf {
        None => ffi::Surf::Plane {
            o: f.o,
            x: f.x,
            y: f.y,
            domain: f.domain,
        },
        Some(SurfIn::Revolution {
            gen,
            point,
            axis,
            swap,
            sweep,
        }) => ffi::Surf::Revolution {
            gen: ffi_gen(gen),
            point: *point,
            axis: *axis,
            swap: *swap,
            sweep: *sweep,
        },
    };
    let loops = match &f.loops {
        None => ffi::Loops::Vertex([0.0; 3]),
        Some(LoopsIn::Vertex { at }) => ffi::Loops::Vertex(*at),
        Some(LoopsIn::Edges { loops }) => ffi::Loops::Edges(loops.clone()),
    };
    ffi::FaceSpec {
        tol: f.tol,
        surf,
        loops,
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

/// The output of a regression case for the FFI guard: the script must be rejected (the C++
/// is not called: upstream crashes on these scripts).
fn rejection(face: &FaceIn, ops: &[[f64; 5]]) -> Result<Output, HarnessError> {
    match ffi::validate(&ffi_face(face), &ffi_ops(ops)) {
        Err(_) => Ok(Output::err("rejected")),
        Ok(()) => Err(harness("expected the FFI guard to reject the script")),
    }
}

fn cpp(c: &CaseIn) -> Result<Output, HarnessError> {
    match c {
        CaseIn::Props {
            face,
            ops,
            expect_rejected,
        } => {
            if *expect_rejected {
                return rejection(face, ops);
            }
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

fn rust_circle(
    k: &Kernel,
    c: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    d: [f64; 2],
    r: f64,
) -> Result<Curve, HarnessError> {
    let e = Ellipse::new_circle(
        k,
        v3(c),
        v3(x),
        v3(y),
        Extent1d::from_min_max(d[0], d[1]),
        r,
        &EllipseOptions::default(),
    )
    .map_err(|e| harness(format!("circle: {e}")))?;
    Ok(Curve::from(e))
}

/// The face's surface, as `read_surface` of the shim builds it.
fn rust_surface(k: &Kernel, f: &FaceIn) -> Result<Surface, HarnessError> {
    match &f.surf {
        None => Ok(Surface::from(Plane::new(
            v3(f.o),
            v3(f.x),
            v3(f.y),
            Vec2::new(1.0, 1.0),
            &Extent2d::from_bounds(f.domain[0], f.domain[1], f.domain[2], f.domain[3]),
        ))),
        Some(SurfIn::Revolution {
            gen,
            point,
            axis,
            swap,
            sweep,
        }) => {
            let g = match gen {
                GenIn::Line { a, b } => {
                    let mut l = Line::from_points(v3(*a), v3(*b), 3);
                    l.make_nurb()
                        .map_err(|e| harness(format!("line nurb: {e}")))?;
                    Curve::from(l)
                }
                GenIn::Circle { c, x, y, d, r } => rust_circle(k, *c, *x, *y, *d, *r)?,
            };
            let rev = SurfOfRevolution::create_canonical(k, g, v3(*point), v3(*axis), *swap)
                .map_err(|e| harness(format!("create_canonical: {e}")))?;
            let mut s = Surface::from(rev);
            if *sweep < 360.0 {
                let d = s.step_uv_domain();
                s.adjust_step_uv_domain(
                    k,
                    &Extent2d::from_bounds(d.u_min(), d.v_min(), d.u_min() + *sweep, d.v_max()),
                )
                .map_err(|e| harness(format!("adjust_step_uv_domain: {e}")))?;
            }
            Ok(s)
        }
    }
}

/// The id the next record of an arena gets, which the record's links name before it exists.
macro_rules! next_id {
    ($arena:expr, $id:ident) => {
        $id::from_index(u32::try_from($arena.len()).map_err(|_| harness("too many records"))?)
    };
}

/// The B-rep of the shim, record by record: `SmBrepConstructor::StartFace` (an up and a down
/// face use in the one shell, the face owning its surface), `StartAndEndSingleVertexLoop`
/// (a vertex loop use on a vertex) or `StartLoop` / `StartEdge` / `EndLoop` (per edge loop a
/// loop use whose edge use list starts at the first requested edge use, one `SmLine` edge per
/// edge use, a vertex per edge use at its edge's start; the first loop use outer, the others
/// inner), then `SetTolerance` of the face.
fn rust_brep(k: &Kernel, f: &FaceIn) -> Result<(Brep, FaceId), HarnessError> {
    let mut b = Brep::new();
    let e = |e: solidcore::SolidError| harness(e.to_string());
    let region = b.add_region_record(Region { in_brep: true }).map_err(e)?;
    let shell = b
        .add_shell_record(Shell {
            faces: Vec::new(),
            region: Some(region),
        })
        .map_err(e)?;

    // the face and its surface, which the face owns
    let mut surface = rust_surface(k, f)?;
    surface.set_owner(Some(ObjectRef {
        type_code: fp2::TYPE,
        key: ObjectKey(1),
    }));
    let uv_domain = surface.natural_uv_domain();
    let face = b
        .add_face_record(Face {
            surface: Some(surface),
            uv_domain,
            tolerance: f.tol,
            ..Face::new(shell)
        })
        .map_err(e)?;
    b.shell_mut(shell).map_err(e)?.faces.push(face);
    let up = next_id!(b.faceuses(), FaceuseId);
    let down = next_id!(b.faceuses(), FaceuseId);
    for (mate, orientation) in [(down, OrientType::Same), (up, OrientType::Opposite)] {
        b.add_faceuse_record(Faceuse {
            shell: Some(shell),
            face: Some(face),
            mate: Some(mate),
            orientation,
            ..Faceuse::default()
        })
        .map_err(e)?;
    }
    b.face_mut(face).map_err(e)?.faceuse = Some(up);

    match &f.loops {
        None => rust_vertex_loop(&mut b, face, up, [0.0; 3], true)?,
        Some(LoopsIn::Vertex { at }) => rust_vertex_loop(&mut b, face, up, *at, true)?,
        Some(LoopsIn::Edges { loops }) => {
            for (j, pts) in loops.iter().enumerate() {
                rust_edge_loop(&mut b, face, up, pts, j == 0)?;
            }
        }
    }
    Ok((b, face))
}

/// A loop record and its loop use on the up face use.
fn rust_loop_records(
    b: &mut Brep,
    face: FaceId,
    up: FaceuseId,
    start: LoopuseStart,
    outer: bool,
) -> Result<LoopuseId, HarnessError> {
    let e = |e: solidcore::SolidError| harness(e.to_string());
    let l = next_id!(b.loops(), LoopId);
    let lu = b
        .add_loopuse_record(Loopuse {
            faceuse: Some(up),
            loop_: Some(l),
            mate: None,
            start,
            orientation: if outer {
                OrientType::Same
            } else {
                OrientType::Opposite
            },
        })
        .map_err(e)?;
    b.add_loop_record(Loop {
        face,
        edges: Vec::new(),
        loopuse: Some(lu),
    })
    .map_err(e)?;
    b.faceuse_mut(up).map_err(e)?.loopuses.push(lu);
    b.face_mut(face).map_err(e)?.loops.push(l);
    Ok(lu)
}

fn rust_vertex_loop(
    b: &mut Brep,
    face: FaceId,
    up: FaceuseId,
    at: [f64; 3],
    outer: bool,
) -> Result<(), HarnessError> {
    let e = |e: solidcore::SolidError| harness(e.to_string());
    let lu = next_id!(b.loopuses(), LoopuseId);
    let vu = next_id!(b.vertexuses(), VertexuseId);
    let v = b.add_vertex(v3(at)).map_err(e)?;
    b.add_vertexuse_record(Vertexuse {
        vertex: Some(v),
        owner: VertexuseOwner::Loopuse(Some(lu)),
        ..Vertexuse::default()
    })
    .map_err(e)?;
    b.vertex_mut(v).map_err(e)?.vertexuses.push(vu);
    rust_loop_records(b, face, up, LoopuseStart::Vertexuse(Some(vu)), outer)?;
    Ok(())
}

fn rust_edge_loop(
    b: &mut Brep,
    face: FaceId,
    up: FaceuseId,
    pts: &[[f64; 3]],
    outer: bool,
) -> Result<(), HarnessError> {
    let e = |e: solidcore::SolidError| harness(e.to_string());
    let n = pts.len();
    let lu = next_id!(b.loopuses(), LoopuseId);
    let eu0 = next_id!(b.edgeuses(), EdgeuseId);
    let vu0 = next_id!(b.vertexuses(), VertexuseId);
    let e0 = next_id!(b.edges(), EdgeId);
    let v0 = next_id!(b.vertices(), VertexId);
    let at = |i: usize| -> Result<[f64; 3], HarnessError> {
        pts.get(i % n).copied().ok_or_else(|| harness("loop point"))
    };
    let index = |base: u32, i: usize| -> Result<u32, HarnessError> {
        base.checked_add(u32::try_from(i % n).map_err(|_| harness("loop size"))?)
            .ok_or_else(|| harness("too many records"))
    };

    // the vertices, at the start of each edge
    for i in 0..n {
        b.add_vertex(v3(at(i)?)).map_err(e)?;
    }
    // the edges: lines p[i] -> p[i + 1] over their natural interval
    for i in 0..n {
        let curve = Curve::from(Line::from_points(v3(at(i)?), v3(at(i + 1)?), 3));
        let interval = curve.natural_interval();
        let tolerance = b.tolerance();
        b.add_edge_record(Edge {
            vertices: [
                VertexId::from_index(index(v0.index(), i)?),
                VertexId::from_index(index(v0.index(), i + 1)?),
            ],
            edgeuses: vec![EdgeuseId::from_index(index(eu0.index(), i)?)],
            interval,
            tolerance,
            curve: Some(curve),
        })
        .map_err(e)?;
    }
    // the vertex uses and the edge uses (counter-clockwise: the next requested edge use)
    for i in 0..n {
        let eu = EdgeuseId::from_index(index(eu0.index(), i)?);
        let v = VertexId::from_index(index(v0.index(), i)?);
        b.add_vertexuse_record(Vertexuse {
            vertex: Some(v),
            owner: VertexuseOwner::Edgeuse(Some(eu)),
            ..Vertexuse::default()
        })
        .map_err(e)?;
        b.vertex_mut(v)
            .map_err(e)?
            .vertexuses
            .push(VertexuseId::from_index(index(vu0.index(), i)?));
    }
    for i in 0..n {
        b.add_edgeuse_record(Edgeuse {
            edge: Some(EdgeId::from_index(index(e0.index(), i)?)),
            owner: EdgeuseOwner::Loopuse(Some(lu)),
            ccw: Some(EdgeuseId::from_index(index(eu0.index(), i + 1)?)),
            cw: Some(EdgeuseId::from_index(index(eu0.index(), i + n - 1)?)),
            vertexuse: Some(VertexuseId::from_index(index(vu0.index(), i)?)),
            orientation: OrientType::Same,
            ..Edgeuse::default()
        })
        .map_err(e)?;
    }
    let made = rust_loop_records(b, face, up, LoopuseStart::Edgeuse(Some(eu0)), outer)?;
    if made != lu {
        return Err(harness("loop use id"));
    }
    Ok(())
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
    for i in 0..4 {
        match p.flat_corner_points.get(i) {
            Some(v) => o.extend([v.x, v.y, v.z]),
            None => o.extend([0.0; 3]),
        }
    }
    o.resize(ffi::SNAP_LEN, 0.0);
    o
}

/// Op 8: `SmLoopProps::SetProps` of every loop of the face (the loop-props loop of
/// `SetProps_Stage3`): the loop count, then the props of the first loops.
fn loop_snapshot(
    k: &Kernel,
    b: &Brep,
    p: &FaceProps<H>,
    face: FaceId,
) -> Result<(f64, Vec<f64>), solidcore::SolidError> {
    let t = |x: solidcore::heal::face_props::Tri| x.code() as f64;
    let loops = solidcore::topology::face::p2::loops(b, face, false, None)?;
    let mut o: Vec<f64> = Vec::new();
    for l in loops.iter().take(ffi::MAX_LOOP_SNAPS) {
        let mut lp = LoopProps::new();
        lp.set_props(k, b, *l, p)?;
        o.push(lp.loopuse_type.code() as f64);
        o.push(t(lp.vertex_loop_on_pole));
        o.push(lp.edge_cnt as f64);
        o.push(lp.edgeuse_cnt as f64);
        o.push(t(lp.closed_3d));
        o.push(t(lp.closed_ptrs));
        o.push(t(lp.good_closed));
        o.push(lp.seam_crossing_cnt_u as f64);
        o.push(lp.seam_winding_cnt_u as f64);
        o.push(orient_code(lp.winding_orient_u) as f64);
        o.push(lp.seam_crossing_cnt_v as f64);
        o.push(lp.seam_winding_cnt_v as f64);
        o.push(orient_code(lp.winding_orient_v) as f64);
        o.push(t(lp.area_loop));
    }
    o.resize(ffi::SNAP_LEN, 0.0);
    Ok((loops.len() as f64, o))
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
    let (b, face) = rust_brep(&k, f)?;
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
            8 => match loop_snapshot(&k, &b, &p, face) {
                Ok((n, snap)) => {
                    out.push(n);
                    out.extend(snap);
                    continue;
                }
                Err(e) => Err(e),
            },
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
        CaseIn::Props {
            face,
            ops,
            expect_rejected,
        } => {
            if *expect_rejected {
                rejection(face, ops)
            } else {
                rust_props(face, ops)
            }
        }
        CaseIn::Tree {
            items,
            links,
            finds,
        } => rust_tree(items, links, finds),
    }
}

// ---- generation ---------------------------------------------------------------------

/// The pole points of a generated surface (a sphere's ends, a cone's apex).
fn pole_points(surf: Option<&SurfIn>) -> Vec<[f64; 3]> {
    let Some(SurfIn::Revolution { gen, .. }) = surf else {
        return Vec::new();
    };
    match gen {
        GenIn::Line { b, .. } if b[0] == 0.0 => vec![*b],
        GenIn::Circle { c, d, r, .. } if c[0] == 0.0 => {
            // the arc ends on the axis at +-90 degrees
            let mut v = Vec::new();
            if (d[0] + 90.0).abs() < 1e-9 {
                v.push([0.0, 0.0, -*r]);
            }
            if (d[1] - 90.0).abs() < 1e-9 {
                v.push([0.0, 0.0, *r]);
            }
            v
        }
        _ => Vec::new(),
    }
}

/// A generated revolution: a cylinder, a cone (with or without apex), a sphere or a torus,
/// closed (swept 360 degrees) or open.
fn gen_revolution(rng: &mut Rng) -> SurfIn {
    let gs = *rng.choose(&[1.0, 1.0, 1.0, 1.0, 1e-3, 1e3]).unwrap_or(&1.0);
    let r = rng.uniform(0.5, 3.0) * gs;
    let gen = match rng.below(4) {
        // a cylinder
        0 => GenIn::Line {
            a: [r, 0.0, rng.uniform(-2.0, 0.0) * gs],
            b: [r, 0.0, rng.uniform(0.5, 3.0) * gs],
        },
        // a cone; one in three has its apex on the axis (a pole)
        1 => {
            let r2 = if rng.chance(0.4) {
                0.0
            } else {
                r * rng.uniform(0.2, 2.0)
            };
            GenIn::Line {
                a: [r, 0.0, rng.uniform(-2.0, 0.0) * gs],
                b: [r2, 0.0, rng.uniform(0.5, 3.0) * gs],
            }
        }
        // a sphere: the arc reaches the axis at none, one or both ends (poles)
        2 => {
            let ends = [[-90.0, 90.0], [-90.0, 40.0], [-30.0, 90.0], [-30.0, 70.0], [-90.0, 90.0]];
            let d = *rng.choose(&ends).unwrap_or(&[-90.0, 90.0]);
            GenIn::Circle {
                c: [0.0, 0.0, 0.0],
                x: [1.0, 0.0, 0.0],
                y: [0.0, 0.0, 1.0],
                d,
                r,
            }
        }
        // a torus: a full circle about an axis outside it, or an arc of it
        _ => {
            let d = if rng.chance(0.5) {
                [0.0, 360.0]
            } else {
                let d0 = rng.uniform(-150.0, 0.0);
                [d0, d0 + rng.uniform(60.0, 150.0)]
            };
            GenIn::Circle {
                c: [r * 2.0, 0.0, 0.0],
                x: [1.0, 0.0, 0.0],
                y: [0.0, 0.0, 1.0],
                d,
                r,
            }
        }
    };
    let sweep = if rng.chance(0.45) {
        360.0
    } else {
        *rng.choose(&[90.0, 180.0, 270.0, 300.0]).unwrap_or(&270.0)
    };
    SurfIn::Revolution {
        gen,
        point: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        swap: rng.chance(0.3),
        sweep,
    }
}

/// A closed polygon of `n` corners about `c`; `ccw` false runs it clockwise (a hole).
fn gen_polygon(rng: &mut Rng, c: [f64; 3], radius: f64, ccw: bool) -> Vec<[f64; 3]> {
    let n = 3 + rng.below(4) as usize;
    let mut pts: Vec<[f64; 3]> = (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * (i as f64 + rng.uniform(0.0, 0.4)) / n as f64;
            let r = radius * rng.uniform(0.8, 1.0);
            [c[0] + r * a.cos(), c[1] + r * a.sin(), c[2]]
        })
        .collect();
    if !ccw {
        pts.reverse();
    }
    pts
}

fn gen_loops(rng: &mut Rng, poles: &[[f64; 3]]) -> Option<LoopsIn> {
    let pick_pole = |rng: &mut Rng| -> Option<[f64; 3]> { rng.choose(poles).copied() };
    match rng.below(10) {
        // the one vertex loop at the origin
        0..=3 => None,
        // a vertex loop at a pole or a random point
        4 | 5 => {
            let at = match pick_pole(rng) {
                Some(p) if rng.chance(0.8) => p,
                _ => rng.point3(-2.0, 2.0),
            };
            Some(LoopsIn::Vertex { at })
        }
        // an outer edge loop, and often holes
        _ => {
            let c = rng.point3(-1.0, 1.0);
            let radius = rng.uniform(1.0, 3.0);
            let mut outer = gen_polygon(rng, c, radius, true);
            if let (Some(p), true) = (pick_pole(rng), rng.chance(0.4)) {
                if let Some(first) = outer.first_mut() {
                    *first = p;
                }
            }
            let mut loops = vec![outer];
            if rng.chance(0.6) {
                let radius = rng.uniform(0.2, 0.7);
                loops.push(gen_polygon(rng, c, radius, false));
                if rng.chance(0.25) {
                    let c2 = [c[0] + 0.1, c[1] - 0.1, c[2]];
                    loops.push(gen_polygon(rng, c2, 0.15, false));
                }
            }
            Some(LoopsIn::Edges { loops })
        }
    }
}

fn gen_face(rng: &mut Rng) -> FaceIn {
    let tols = [
        1e-5,
        1e-6,
        2e-5,
        1e-4,
        1e-3,
        5e-6,
        1e-7,
        1e-5 + 1e-12,
        1e-5,
        1e-5,
        // large tolerances make every side of the surface (approximately) singular
        1e-2,
        1.0,
        1e3,
        1e9,
    ];
    let x = [1.0, 0.0, 0.0];
    let y = [0.0, 1.0, 0.0];
    let a = rng.uniform(-3.0, 0.0);
    let b = rng.uniform(-3.0, 0.0);
    let mut domain = [a, b, a + rng.uniform(0.5, 4.0), b + rng.uniform(0.5, 4.0)];
    if rng.chance(0.08) {
        // a degenerate domain: every side is a pole
        let t = *rng.choose(&[1e-300, 1e-100, 1e-14, 1e-9]).unwrap_or(&1e-14);
        domain = [0.0, 0.0, t, t];
    }
    let surf = rng.chance(0.55).then(|| gen_revolution(rng));
    let loops = gen_loops(rng, &pole_points(surf.as_ref()));
    FaceIn {
        tol: *rng.choose(&tols).unwrap_or(&1e-5),
        o: rng.point3(-2.0, 2.0),
        x,
        y,
        domain,
        surf,
        loops,
    }
}

fn gen_op(rng: &mut Rng) -> [f64; 5] {
    let heal_ops = [0, 1, 4, 5, 6, 12, 17, 18, 19, 20, 21, 26];
    let heal = f64::from(*rng.choose(&heal_ops).unwrap_or(&26));
    let stage = rng.range_i64(0, 6) as f64;
    match rng.below(16) {
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
        8 | 9 => [8.0, 0.0, 0.0, 0.0, 0.0],
        _ => {
            // a seed; the stage seed is followed by a face seed (a stage past the gaps
            // with no face would be dereferenced upstream, UB-131)
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
    if rng.chance(0.4) {
        // the stage 2 props first (the surface, closed, pole and corner classification),
        // so that the ops after it see real values
        ops.push([0.0, 4.0, 26.0, rng.range_i64(0, 5) as f64, 0.0]);
    }
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
    if rng.chance(0.3) {
        ops.push([8.0, 0.0, 0.0, 0.0, 0.0]);
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
    CaseIn::Props {
        face,
        ops,
        expect_rejected: false,
    }
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
        // redraw scripts the FFI guard rejects (the guard cases are fixtures)
        for _ in 0..64 {
            let c = if rng.chance(0.85) {
                gen_props(rng)
            } else {
                gen_tree(rng)
            };
            if let CaseIn::Props { face, ops, .. } = &c {
                if ffi::validate(&ffi_face(face), &ffi_ops(ops)).is_err() {
                    continue;
                }
            }
            return serde_json::to_value(c).ok();
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

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}
