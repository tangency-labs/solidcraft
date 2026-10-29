//! Rust-side builders for the B-rep sources the topology shims build natively.
//!
//! The shims of `sc_edgeuse_graph.h` build an `SmBrep` per case from a source block
//! (`build`: a face from a surface, a wire edge, the `prog_test` wire network, ...),
//! walk it in a fixed traversal order and export the graph. The Rust side used to
//! rebuild its B-rep from that export, so its input was C++ output. [`build`] instead
//! makes the same B-rep with the Rust kernel, in the shim's call order, and
//! [`renumber_with`] copies it into a B-rep whose arena ids are the shim's traversal
//! order ([`walk`], the shim's `walk`), so selector indices taken modulo the entity
//! counts name the same entities on both sides. No C++ state reaches the Rust side.
//!
//! `.smb` file sources ([`BrepIn::File`]) are not built by [`build`] (it returns
//! `None` for them): [`read_with`] reads them with the Rust reader and renumbers the
//! result the same way.

use std::collections::BTreeMap;

use solidcore::curve::crv_on_surf::CrvOnSurf;
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::object::ObjectRef;
use solidcore::geom::tol;
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::brep_cache::{arena_identity, CacheObject};
use solidcore::topology::edge::{NotifyEnv, SetCurveOptions};
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::shell::ShellMembers;
use solidcore::topology::topology_traverser::Marks;
use solidcore::topology::{
    Brep, Edge, EdgeId, Edgeuse, EdgeuseId, EdgeuseOwner, Face, FaceId, Faceuse, FaceuseId, Loop,
    LoopEdge, LoopId, Loopuse, LoopuseId, LoopuseStart, Region, RegionId, Shell, ShellId, Vertex,
    VertexId, Vertexuse, VertexuseId, VertexuseOwner,
};
use solidcore::{EntityRef, SolidError};

use crate::units::topology_edgeuse_p1::{bspline_of, surface_of, BrepIn, SrfIn};
use crate::HarnessError;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

#[allow(clippy::needless_pass_by_value)] // used as `map_err(solid)`
fn solid(e: SolidError) -> HarnessError {
    harness(format!("own builder: {e}"))
}

/// A built B-rep with its geometry types and entity counts, or the output a failed
/// build reports.
pub(crate) type RustBrep<T> = Result<T, crate::Output>;

/// The outcome of building a source: the B-rep in the shim's traversal order, or the
/// error the Rust build returned (the shim's `build` returns NULL with a status).
#[derive(Debug)]
pub(crate) enum Built {
    /// The built B-rep, renumbered into the shim's traversal order.
    Brep(Box<Brep>),
    /// The build failed (an unported callee, or the error the port returns where the
    /// shim reports a non-success status).
    Failed(SolidError),
}

/// How a runner's shim orders, names and initialises the B-rep it builds: the
/// differences between `sc_edgeuse_graph.h` ([`Conventions::EDGEUSE_GRAPH`]) and
/// `sc_topology_fixture.h` as the `topology.brep_p2` shim uses it.
#[allow(clippy::struct_excessive_bools)] // independent per-shim switches
pub(crate) struct Conventions<'a> {
    /// Register the B-rep's face, edge and vertex lists (`GetFaces`, `GetEdges`,
    /// `GetVertices`) before the traversal (the `topology.brep_p2` shim).
    pub(crate) prefix_lists: bool,
    /// The `init_gaps` walk also calls `GetVertexEdgeGap` on vertex uses that have one
    /// (`sc_topology_fixture.h`).
    pub(crate) vertex_edge_gaps: bool,
    /// The traversal follows the vertex use lists after the loops' loop uses (the
    /// `topology.vertex` shim's own `walk`) instead of before them.
    pub(crate) vertex_lists_last: bool,
    /// The traversal does not register the B-rep's edge list (`GetEdges`) before the
    /// regions' contents (the `query.edge` shim's own `walk`).
    pub(crate) skip_edge_list: bool,
    /// The owner identity of face `f`'s surface (when the surface has an owner).
    pub(crate) face_owner: &'a dyn Fn(FaceId) -> ObjectRef,
    /// The owner identity (and its B-rep's) of edge `e`'s curve (when it has an owner).
    pub(crate) edge_owner: &'a dyn Fn(EdgeId) -> (ObjectRef, ObjectRef),
}

fn arena_face_owner(f: FaceId) -> ObjectRef {
    arena_identity(CacheObject::Face(f))
}

fn arena_edge_owner(e: EdgeId) -> (ObjectRef, ObjectRef) {
    (
        arena_identity(CacheObject::Edge(e)),
        arena_identity(CacheObject::Brep),
    )
}

impl Conventions<'static> {
    /// `sc_edgeuse_graph.h`: the traversal alone, geometry owned under the kernel's
    /// arena identities (as the `topology.brep_p1` import names them).
    pub(crate) const EDGEUSE_GRAPH: Self = Self {
        prefix_lists: false,
        vertex_edge_gaps: false,
        vertex_lists_last: false,
        skip_edge_list: false,
        face_owner: &arena_face_owner,
        edge_owner: &arena_edge_owner,
    };

    /// `sc_topology_fixture.h` (`build`, then `walk` for the export of its
    /// `run_brep_op`): the traversal alone, `GetVertexEdgeGap` in the `init_gaps` walk,
    /// geometry owned under the `topology.vertexuse` import's identities
    /// ([`fixture_key`]: a face `(SmFace_TYPE, 1 + index)`).
    pub(crate) const TOPOLOGY_FIXTURE: Self = Self {
        prefix_lists: false,
        vertex_edge_gaps: true,
        vertex_lists_last: false,
        skip_edge_list: false,
        face_owner: &fixture_face_owner,
        edge_owner: &fixture_edge_owner,
    };
}

/// The owner identity of an entity of kind `type_code` at arena index `index` under the
/// `sc_topology_fixture.h` runners' naming (as `topology.brep_p2`'s `key_of`): a face is
/// `(SmFace_TYPE, 1 + index)`, the surface owner the `topology.vertexuse` import gives;
/// every other kind gets its own key range `(type << 32) | (1 + index)`.
pub(crate) fn fixture_key(type_code: i64, index: u32) -> ObjectRef {
    let key = 1 + u64::from(index);
    ObjectRef {
        type_code,
        key: solidcore::geom::attribute::ObjectKey(
            if type_code == solidcore::topology::face::p2::TYPE {
                key
            } else {
                (type_code.unsigned_abs() << 32) | key
            },
        ),
    }
}

fn fixture_face_owner(f: FaceId) -> ObjectRef {
    fixture_key(solidcore::topology::face::p2::TYPE, f.index())
}

fn fixture_edge_owner(e: EdgeId) -> (ObjectRef, ObjectRef) {
    // `SmBrep_TYPE` has no Rust constant; 16999 is the B-rep code the runners' `key_of`
    // use
    (
        fixture_key(solidcore::topology::edge::TYPE, e.index()),
        fixture_key(16999, 0),
    )
}

/// [`build_with`] for a runner's own source type: any value whose JSON has the shape
/// of a [`BrepIn`] (the runners' `BrepIn` copies, `kind`-tagged).
///
/// # Errors
///
/// As [`build`]; a harness error when the value is not a [`BrepIn`].
pub(crate) fn build_as<T: serde::Serialize>(
    k: &mut Kernel,
    src: &T,
    validate_only: bool,
    init_gaps: bool,
    conv: &Conventions<'_>,
) -> Result<Option<Built>, HarnessError> {
    let src: BrepIn = serde_json::to_value(src)
        .and_then(serde_json::from_value)
        .map_err(|e| harness(format!("B-rep source: {e}")))?;
    build_with(k, &src, validate_only, init_gaps, conv)
}

/// The output of a failed build as the graph runners report the shim's NULL build:
/// the deferral of an unported callee, else `brep_build_status_<status>`.
pub(crate) fn failed_output(e: &SolidError) -> crate::Output {
    crate::Output::unported(e)
        .unwrap_or_else(|| crate::Output::err(format!("brep_build_status_{}", build_status(e))))
}

/// Builds the B-rep of `src` as the shim's `build` (`sc_edgeuse_graph.h`) does, with
/// `m_bValidateOnly` set and, with `init_gaps`, the shim's gap initialisation walk.
/// `None` for an `.smb` file source (not built here).
///
/// # Errors
///
/// A harness error for input the shim would not accept either (malformed curves or
/// surfaces) or when the renumbering finds a dangling reference.
pub(crate) fn build(
    k: &mut Kernel,
    src: &BrepIn,
    validate_only: bool,
    init_gaps: bool,
) -> Result<Option<Built>, HarnessError> {
    build_with(
        k,
        src,
        validate_only,
        init_gaps,
        &Conventions::EDGEUSE_GRAPH,
    )
}

/// [`build`] under a shim's [`Conventions`].
///
/// # Errors
///
/// As [`build`].
pub(crate) fn build_with(
    k: &mut Kernel,
    src: &BrepIn,
    validate_only: bool,
    init_gaps: bool,
    conv: &Conventions<'_>,
) -> Result<Option<Built>, HarnessError> {
    let raw = match src {
        BrepIn::File { .. } => return Ok(None),
        BrepIn::Face { surface, dom } => face(k, surface, *dom, false)?,
        BrepIn::CrvOnSurfFace { surface, dom } => face(k, surface, *dom, true)?,
        BrepIn::Wire { curve, t0, t1 } => {
            let c = Curve::from(bspline_of(curve)?);
            in_new_brep(k, |k, b| {
                solidcore::topology::brep::p5::create_wire_edge_from_curve(
                    k,
                    b,
                    c,
                    Extent1d::from_min_max(*t0, *t1),
                )
                .map(|_| ())
            })?
        }
        BrepIn::Network => in_new_brep(k, wire_network)?,
        BrepIn::Vertices { points } => in_new_brep(k, |_, b| {
            let mut state = ArenaNotifyState::new(b);
            let mut env = state.env(None);
            for p in points {
                let region = b.infinite_region();
                solidcore::topology::brep::p9::make_shell_vertex(
                    b,
                    region,
                    Vec3::from_array(*p),
                    &mut env,
                )?;
            }
            Ok(())
        })?,
        BrepIn::Mixed { face, networks } => {
            let face = face
                .as_ref()
                .map(|f| surface_of(&f.surface).map(|s| (s, f.dom)))
                .transpose()?;
            in_new_brep(k, |k, b| {
                if let Some((s, dom)) = face {
                    create_face(k, b, s, dom)?;
                }
                for _ in 0..*networks {
                    wire_network(k, b)?;
                }
                Ok(())
            })?
        }
        BrepIn::Box { origin, size } => solidcore::primitive::api_primitives::create_box(
            k,
            Vec3::from_array(*origin),
            size[0],
            size[1],
            size[2],
        ),
        BrepIn::Points { vertices, lines } => in_new_brep(k, |k, b| points(k, b, vertices, lines))?,
        BrepIn::EllipseFace {
            circle,
            origin,
            axis,
            radii,
            angles,
            natural,
            inside_out,
            oob,
        } => in_new_brep(k, |k, b| {
            ellipse_face(
                k,
                b,
                &EllipseIn {
                    circle: *circle,
                    origin: *origin,
                    axis: *axis,
                    radii: *radii,
                    angles: *angles,
                    natural: *natural,
                    inside_out: *inside_out,
                    oob: *oob,
                },
            )
        })?,
        BrepIn::HermiteSheetFace { scale, edge_tol } => {
            in_new_brep(k, |k, b| hermite_sheet_face(k, b, *scale, *edge_tol))?
        }
    };
    finish(k, raw, validate_only, init_gaps, conv).map(Some)
}

/// The tail of a shim's `build` for a B-rep a runner made itself (or its build's
/// error): `m_bValidateOnly` set, renumbered into the shim's traversal order under
/// `conv`, and with `init_gaps` the shim's gap initialisation walk.
///
/// # Errors
///
/// As [`build`].
pub(crate) fn finish(
    k: &mut Kernel,
    raw: Result<Brep, SolidError>,
    validate_only: bool,
    init_gaps: bool,
    conv: &Conventions<'_>,
) -> Result<Built, HarnessError> {
    let mut raw = match raw {
        Ok(b) => b,
        Err(e) => return Ok(Built::Failed(e)),
    };
    raw.set_validate_only(validate_only);
    // the edges' cached end vertices as their uses give them (upstream has no such
    // field; a merge inside the build, as `CreateWireEdgeFromCurve`'s glue of a
    // degenerate curve, rewires only the uses)
    raw.refresh_cached_topology();
    let mut b = renumber_with(&raw, conv)?;
    if init_gaps {
        if let Err(e) = init_gap_walk(k, &mut b, conv.vertex_edge_gaps) {
            return Ok(Built::Failed(e));
        }
    }
    Ok(Built::Brep(Box::new(b)))
}

/// The B-rep of an `.smb` file source as the shim's `build` makes it: `new SmBrep`,
/// `ReadFromFile(path, SM_ASCII, rebuild)` read by the Rust reader
/// ([`crate::own_reader::read_file_rebuild`]), `m_bValidateOnly`, then (with
/// `init_gaps`) the shim's gap initialisation walk, renumbered into the shim's
/// traversal order under `conv`. No C++ state reaches the Rust side.
///
/// # Errors
///
/// A harness error when the file cannot be opened or the renumbering finds a dangling
/// reference; a failed Rust read is [`Built::Failed`].
pub(crate) fn read_with(
    k: &mut Kernel,
    path: &str,
    rebuild: bool,
    validate_only: bool,
    init_gaps: bool,
    conv: &Conventions<'_>,
) -> Result<Built, HarnessError> {
    let mut raw = match crate::own_reader::read_file_rebuild(k, path, rebuild)? {
        Ok(b) => b,
        Err(e) => return Ok(Built::Failed(e)),
    };
    raw.set_validate_only(validate_only);
    let mut b = renumber_with(&raw, conv)?;
    if init_gaps {
        if let Err(e) = init_gap_walk(k, &mut b, conv.vertex_edge_gaps) {
            return Ok(Built::Failed(e));
        }
    }
    Ok(Built::Brep(Box::new(b)))
}

/// A shim's `.smb` file source (`SmBrep::ReadFromFile(ctx, path, SM_ASCII, rebuild)`
/// into `new (ctx) SmBrep()`), read by the Rust reader ([`crate::own_reader`]) and
/// finished as a built source ([`finish`]): renumbered into the shim's traversal order
/// under `conv`, so selector indices name the same entities on both sides. A failed
/// Rust read is the build's error, as the shim's NULL build reports its status.
///
/// # Errors
///
/// A harness error when the file cannot be opened, or as [`finish`].
pub(crate) fn read_as(
    k: &mut Kernel,
    path: &str,
    rebuild: bool,
    validate_only: bool,
    init_gaps: bool,
    conv: &Conventions<'_>,
) -> Result<Built, HarnessError> {
    let raw = crate::own_reader::read_file_rebuild(k, path, rebuild)?;
    finish(k, raw, validate_only, init_gaps, conv)
}

/// Any source of the shim's `build` under `conv`: an `.smb` file read by the Rust
/// reader ([`read_as`]), every other kind built by the Rust kernel ([`build_with`]).
///
/// # Errors
///
/// As [`read_as`] and [`build_with`].
pub(crate) fn build_any(
    k: &mut Kernel,
    src: &BrepIn,
    validate_only: bool,
    init_gaps: bool,
    conv: &Conventions<'_>,
) -> Result<Built, HarnessError> {
    if let BrepIn::File { rebuild, .. } = src {
        let path = crate::units::topology_edgeuse_p1::path_of(Some(src))?;
        return read_as(k, &path, *rebuild, validate_only, init_gaps, conv);
    }
    build_with(k, src, validate_only, init_gaps, conv)?
        .ok_or_else(|| harness("own builder: a non-file source was not built"))
}

/// `new (ctx) SmBrep()` then `f` on it: the B-rep, or the build's error.
///
/// # Errors
///
/// A harness error when the empty B-rep cannot be made.
pub(crate) fn in_new_brep(
    k: &mut Kernel,
    f: impl FnOnce(&mut Kernel, &mut Brep) -> Result<(), SolidError>,
) -> Result<Result<Brep, SolidError>, HarnessError> {
    let mut b = solidcore::topology::brep::p3::new_brep(k, tol::USE_DEFAULT, tol::USE_DEFAULT)
        .map_err(solid)?;
    Ok(f(k, &mut b).map(|()| b))
}

/// `CreateFaceFromSurface(s, SmExtent2d(a, b), pFace)` on the infinite region.
pub(crate) fn create_face(
    k: &mut Kernel,
    b: &mut Brep,
    surface: Surface,
    dom: [f64; 4],
) -> Result<FaceId, SolidError> {
    let mut state = ArenaNotifyState::new(b);
    let mut env = state.env(None);
    let mut marks = Marks::default();
    let this_face = |face| arena_identity(CacheObject::Face(face));
    let mut fenv = solidcore::topology::brep::p8::FaceBuildEnv {
        env: &mut env,
        marks: &mut marks,
        this_face: &this_face,
    };
    let domain = Extent2d::from_points(Point2::new(dom[0], dom[1]), Point2::new(dom[2], dom[3]));
    solidcore::topology::brep::p8::create_face_from_surface(k, b, Some(surface), &domain, &mut fenv)
}

/// Kinds 2 and 6: a face from a surface; kind 6 then gives every edge an
/// `SmCrvOnSurf` of the face's surface over a copy of its primary edge use's UV trim
/// curve (`SetCurve(cs, TRUE, FALSE, FALSE)`), after `CreateUVTrimCurves(FALSE, ...)`
/// (status ignored).
fn face(
    k: &mut Kernel,
    s: &SrfIn,
    dom: [f64; 4],
    crv_on_surf: bool,
) -> Result<Result<Brep, SolidError>, HarnessError> {
    let surface = surface_of(s)?;
    in_new_brep(k, |k, b| {
        let f = create_face(k, b, surface, dom)?;
        if !crv_on_surf {
            return Ok(());
        }
        let Some(base) = solidcore::topology::face::p2::surface(b, f)?.cloned() else {
            return Ok(());
        };
        // status ignored: a face without UV curves just keeps its edge curves
        let _ = solidcore::topology::face::p3::create_uv_trim_curves(k, b, f, false, None);
        let g = walk(b)?;
        for e in g.edges.items {
            let Some(prim) = eu1::primary_edgeuse(b, e)? else {
                continue;
            };
            let Some(uv) = b.edgeuse(prim)?.uv_trim_curve.clone() else {
                continue;
            };
            let cs = CrvOnSurf::new(k, uv, base.clone(), None, 1)?;
            let mut state = ArenaNotifyState::new(b);
            let mut env = state.env(None);
            solidcore::topology::edge::set_curve(
                b,
                e,
                Some(Curve::from(cs)),
                SetCurveOptions {
                    delete_old_curve: true,
                    dbg_warn_leaks: false,
                    call_notify: false,
                },
                &mut env,
                [None, None],
            )?;
            set_curve_owner(b, e)?;
        }
        Ok(())
    })
}

/// `pCurve->SetOwner(pEdge)`.
fn set_curve_owner(b: &mut Brep, e: EdgeId) -> Result<(), SolidError> {
    if let Some(c) = b.edge_mut(e)?.curve.as_mut() {
        let _ = c.set_owner(
            Some(arena_identity(CacheObject::Edge(e))),
            Some(arena_identity(CacheObject::Brep)),
        );
    }
    Ok(())
}

/// `MakeEdgeTopology(region, a, b, edge)`, then the edge's curve as the shim sets it:
/// `SetCurve(line, TRUE)`, `SetOwner`, `SetInterval(natural)`, `SetOrientation(SAME)`.
pub(crate) fn edge_with_curve(
    k: &mut Kernel,
    b: &mut Brep,
    env: &mut NotifyEnv<'_>,
    from: VertexId,
    to: VertexId,
    curve: Curve,
) -> Result<EdgeId, SolidError> {
    let region = b.infinite_region();
    let e = solidcore::topology::brep::p2::make_edge_topology(k, b, region, from, to, env)?;
    let natural = curve.natural_interval();
    solidcore::topology::edge::set_curve(
        b,
        e,
        Some(curve),
        SetCurveOptions {
            delete_old_curve: true,
            ..SetCurveOptions::default()
        },
        env,
        [None, None],
    )?;
    set_curve_owner(b, e)?;
    solidcore::topology::edge::set_interval(b, e, &natural)?;
    solidcore::topology::edge::set_orientation(b, e, OrientType::Same)?;
    Ok(e)
}

/// `SmBSplineCurve::CreateLineSegment(ctx, 3, a, b, c)`.
pub(crate) fn line(a: Vec3, b: Vec3) -> Result<Curve, SolidError> {
    BSplineCurve::create_line_segment(3, a, b).map(Curve::from)
}

/// `build_wire_network`: the wire network of `prog_test` `my_test_topology`.
fn wire_network(k: &mut Kernel, b: &mut Brep) -> Result<(), SolidError> {
    use solidcore::topology::brep::{p2 as bp2, p3 as bp3, p9 as bp9};
    let mut state = ArenaNotifyState::new(b);
    let region = b.infinite_region();
    let (shell, v1) = {
        let mut env = state.env(None);
        bp9::make_shell_vertex(b, region, Vec3::new(0.0, 0.0, 0.0), &mut env)?
    };
    let (_, v2) = {
        let mut env = state.env(None);
        bp9::make_shell_vertex(b, region, Vec3::new(1.0, 1.0, 1.0), &mut env)?
    };
    state.seed(b);
    let p1 = b.vertex(v1)?.point;
    let c = line(p1, Vec3::new(1.0, 0.0, 0.0))?;
    let natural = c.natural_interval();
    let (_, v3) = {
        let mut env = state.env(None);
        bp3::make_wire_edge_vertex(
            k,
            b,
            &mut env,
            region,
            v1,
            c,
            &natural,
            OrientType::Same,
            Vec3::new(1.0, 0.0, 0.0),
        )?
    };
    state.seed(b);
    let c = line(b.vertex(v3)?.point, b.vertex(v2)?.point)?;
    let natural = c.natural_interval();
    {
        let mut env = state.env(None);
        bp3::make_wire_edge(
            k,
            b,
            &mut env,
            region,
            v3,
            v2,
            c,
            &natural,
            OrientType::Same,
        )?;
    }
    state.seed(b);
    let c = line(b.vertex(v2)?.point, b.vertex(v1)?.point)?;
    {
        let mut env = state.env(None);
        edge_with_curve(k, b, &mut env, v2, v1, c)?;
    }
    state.seed(b);
    let (shell4, v4) = bp2::make_shell_vertex_topology(b, region)?;
    let _ = (shell, shell4);
    state.seed(b);
    set_point(k, b, &mut state, v4, Vec3::new(0.0, 1.0, 0.0))?;
    let c = line(b.vertex(v2)?.point, b.vertex(v4)?.point)?;
    {
        let mut env = state.env(None);
        edge_with_curve(k, b, &mut env, v2, v4, c)?;
    }
    state.seed(b);
    let c = line(b.vertex(v4)?.point, b.vertex(v1)?.point)?;
    {
        let mut env = state.env(None);
        edge_with_curve(k, b, &mut env, v4, v1, c)?;
    }
    Ok(())
}

/// `SmVertex::SetPoint` (notifications into the arena state).
fn set_point(
    k: &mut Kernel,
    b: &mut Brep,
    state: &mut ArenaNotifyState,
    v: VertexId,
    p: Vec3,
) -> Result<(), SolidError> {
    let env = state.env(None);
    let key_of = env.key_of;
    solidcore::topology::vertex::set_point(
        b,
        v,
        p,
        env.records,
        env.attributes,
        Some(&mut k.context),
        key_of(EntityRef::Vertex(v)),
        &|vu| key_of(EntityRef::Vertexuse(vu)),
    )
}

/// Kind 8: isolated vertices and wire line segments, each given its tolerance.
fn points(
    k: &mut Kernel,
    b: &mut Brep,
    vertices: &[[f64; 4]],
    lines: &[[f64; 7]],
) -> Result<(), SolidError> {
    for &[x, y, z, t] in vertices {
        let mut state = ArenaNotifyState::new(b);
        let mut env = state.env(None);
        let region = b.infinite_region();
        let (_, v) = solidcore::topology::brep::p9::make_shell_vertex(
            b,
            region,
            Vec3::new(x, y, z),
            &mut env,
        )?;
        solidcore::topology::vertex::set_tolerance(b, v, t, false, false)?;
    }
    for &[x0, y0, z0, x1, y1, z1, t] in lines {
        let c = line(Vec3::new(x0, y0, z0), Vec3::new(x1, y1, z1))?;
        let natural = c.natural_interval();
        let e = solidcore::topology::brep::p5::create_wire_edge_from_curve(k, b, c, natural)?;
        solidcore::topology::edge::set_tolerance(b, e, t, false, false)?;
    }
    Ok(())
}

/// The fields of [`BrepIn::EllipseFace`].
struct EllipseIn {
    circle: bool,
    origin: [f64; 3],
    axis: u8,
    radii: [f64; 2],
    angles: [f64; 2],
    natural: [f64; 2],
    inside_out: bool,
    oob: bool,
}

/// Kind 9 (`support/ellipse_face_fixture.h`): a planar face bounded by an ellipse or
/// circle arc (closed by a chord below 360 degrees), with exact UV trims attached
/// through `SetUVTrimCurve` after `MakeFaceWithCurves`.
fn ellipse_face(k: &mut Kernel, b: &mut Brep, i: &EllipseIn) -> Result<(), SolidError> {
    use solidcore::topology::brep::p7::{make_face_with_curves, FaceCurvesInput};
    let origin = Vec3::from_array(i.origin);
    let x = if i.axis == 0 {
        Vec3::new(0.0, 1.0, 0.0)
    } else {
        Vec3::new(1.0, 0.0, 0.0)
    };
    let y = if i.axis == 2 {
        Vec3::new(0.0, 1.0, 0.0)
    } else {
        Vec3::new(0.0, 0.0, 1.0)
    };
    let [rx, ry] = i.radii;
    let [a, bb] = i.angles;
    let [n0, n1] = i.natural;
    let domain = Extent1d::from_min_max(a, bb);
    let opts = EllipseOptions {
        inside_out: i.inside_out,
        ..EllipseOptions::default()
    };
    let mut edge = if i.circle {
        Ellipse::new_circle(k, origin, x, y, domain, rx, &opts)?
    } else {
        Ellipse::new(k, origin, x, y, domain, rx, ry, &opts)?
    };
    let local_opts = EllipseOptions {
        dim: 2,
        inside_out: i.inside_out,
        ..EllipseOptions::default()
    };
    let mut local = Ellipse::new(
        k,
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        domain,
        rx,
        ry,
        &local_opts,
    )?;
    let natural = Extent1d::from_min_max(n0, n1);
    edge.edit_parameterization(k, &natural, true, None)?;
    local.edit_parameterization(k, &natural, true, None)?;
    edge.set_out_of_bounds_enabled(i.oob);
    let nurb = local
        .base()
        .gw_nurb()
        .ok_or_else(|| SolidError::sm_err("own_builder::ellipse_face"))?;
    let uv = Curve::from(BSplineCurve::from_gw_nurb(2, nurb));
    let edge = Curve::from(edge);
    let mut curves = vec![Some(edge.clone())];
    let mut orientations = vec![OrientType::Same];
    let mut uv_chord = None;
    if bb - a < 360.0 {
        let start = edge.evaluate_point(k, n0)?;
        let end = edge.evaluate_point(k, n1)?;
        let uv_start = uv.evaluate_point(k, n0)?;
        let uv_end = uv.evaluate_point(k, n1)?;
        curves.push(Some(line(end, start)?));
        uv_chord = Some(Curve::from(BSplineCurve::create_line_segment(
            2, uv_end, uv_start,
        )?));
        orientations.push(OrientType::Same);
    }
    let input = FaceCurvesInput {
        loops: vec![curves.len()],
        curves_3d: Some(curves),
        uv_curves: None,
        orientations,
        loop_points: Vec::new(),
    };
    let dom = Extent2d::from_points(
        Point2::new(-2.0 * rx, -2.0 * ry),
        Point2::new(2.0 * rx, 2.0 * ry),
    );
    let plane = Surface::from(Plane::new(origin, x, y, Vec2::new(1.0, 1.0), &dom));
    let mut state = ArenaNotifyState::new(b);
    let this_face = |face| arena_identity(CacheObject::Face(face));
    let made = {
        let mut env = state.env(None);
        let region = b.infinite_region();
        make_face_with_curves(
            k,
            b,
            region,
            input,
            plane,
            &dom,
            OrientType::Same,
            None,
            &this_face,
            &mut env,
        )?
    };
    let f = made
        .face
        .ok_or_else(|| SolidError::sm_err("own_builder::ellipse_face"))?;
    let mut uv = Some(uv);
    for e in solidcore::topology::face::p4::edges(b, f, None, false, None)? {
        let eu = eu1::primary_edgeuse(b, e)?
            .ok_or_else(|| SolidError::sm_err("own_builder::ellipse_face"))?;
        let is_arc = matches!(
            b.edge(e)?.curve.as_ref().map(Curve::kind),
            Some(CurveKind::Ellipse(_))
        );
        let curve = if is_arc { uv.take() } else { uv_chord.take() };
        let curve = curve.ok_or_else(|| SolidError::sm_err("own_builder::ellipse_face"))?;
        eu1::set_uv_trim_curve(
            b,
            eu,
            eu1::UvTrimCurveArg::New(Box::new(curve)),
            0.0,
            true,
            None,
        )?;
    }
    if uv.is_none() && uv_chord.is_none() {
        Ok(())
    } else {
        Err(SolidError::sm_err("own_builder::ellipse_face"))
    }
}

/// The four boundary segments of the kind-15 sheet in loop order as their UV corner
/// pairs, the native fixture's `u`/`v` arrays: edge 0 is the diagonal
/// `(0.2, 0.2)`-`(0.8, 0.7)`, edge 1 the `u = 0.8` isoparametric curve, edge 2 the
/// diagonal back and edge 3 the `u = 0.2` isoparametric curve.
const HERMITE_SHEET_EDGES: [[[f64; 2]; 2]; 4] = [
    [[0.2, 0.2], [0.8, 0.7]],
    [[0.8, 0.7], [0.8, 0.9]],
    [[0.8, 0.9], [0.2, 0.4]],
    [[0.2, 0.4], [0.2, 0.2]],
];

/// The kind-15 face's Hermite data for `scale`: the start point and tangent, the end
/// point and tangent, and the extrusion vector (`Shape` in
/// `harness/cpp/shim/support/hermite_sheet_fixture.h`).
fn hermite_shape(scale: f64) -> (Vec3, Vec3, Vec3, Vec3, Vec3) {
    (
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(scale, 0.2 * scale, 0.1 * scale),
        Vec3::new(0.6 * scale, 1.2 * scale, 0.7 * scale),
        Vec3::new(1.2 * scale, -0.8 * scale, -0.4 * scale),
        Vec3::new(0.0, 0.0, scale),
    )
}

/// `Shape::at`: the shape curve's point at `u` (Hermite basis, the native fixture's
/// operation order).
fn hermite_shape_at(scale: f64, u: f64) -> Vec3 {
    let (p0, p1, d0, d1, _) = hermite_shape(scale);
    let u2 = u * u;
    let u3 = u2 * u;
    p0 * (2.0 * u3 - 3.0 * u2 + 1.0)
        + d0 * (u3 - 2.0 * u2 + u)
        + p1 * (-2.0 * u3 + 3.0 * u2)
        + d1 * (u3 - u2)
}

/// `Shape::tangent`: the shape curve's derivative at `u`.
fn hermite_shape_tangent(scale: f64, u: f64) -> Vec3 {
    let (p0, p1, d0, d1, _) = hermite_shape(scale);
    let u2 = u * u;
    p0 * (6.0 * u2 - 6.0 * u)
        + d0 * (3.0 * u2 - 4.0 * u + 1.0)
        + p1 * (-6.0 * u2 + 6.0 * u)
        + d1 * (3.0 * u2 - 2.0 * u)
}

/// Kind 15: the unit-domain sheet face on the cubic Hermite shape surface
/// `S(u, v) = C(u) + v * extrusion` (degree 3 x 1, clamped, rational weights 1),
/// bounded by four cubic `HermiteCurve`s of the affine UV segments of
/// [`HERMITE_SHEET_EDGES`]: one loop, every orientation `Same`, no UV trims, the
/// face tolerance `1e-7` and each edge's exactly `edge_tol`.
///
/// The surface, the curves and the tolerances come from the raw scalars here, never from
/// a native export; `finish` renumbers the result into the shim's traversal order as for
/// every other source.
fn hermite_sheet_face(
    k: &mut Kernel,
    b: &mut Brep,
    scale: f64,
    edge_tol: f64,
) -> Result<(), SolidError> {
    use solidcore::curve::hermite_curve::HermiteCurve;
    use solidcore::nurbs::b_spline_surface::BSplineSurface;
    use solidcore::nurbs::math::CPoint;
    use solidcore::topology::brep::p7::{make_face_with_curves, FaceCurvesInput};

    let (p0, p1, d0, d1, extrusion) = hermite_shape(scale);
    let control = [p0, p0 + d0 / 3.0, p1 - d1 / 3.0, p1];
    let mut net: Vec<Vec<CPoint>> = Vec::new();
    for c in &control {
        let mut row = Vec::new();
        for j in [0.0, 1.0] {
            let p = *c + extrusion * j;
            row.push(CPoint::new(p.x, p.y, p.z, 1.0));
        }
        net.push(row);
    }
    let u_knots = [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
    let v_knots = [0.0, 0.0, 1.0, 1.0];
    let nurb = BSplineSurface::from_data(3, 1, 3, 1, 7, 3, &net, &u_knots, &v_knots)?;
    let surface = Surface::from(nurb);
    let mut curves = Vec::new();
    for &[[ua, va], [ub, vb]] in &HERMITE_SHEET_EDGES {
        let du = ub - ua;
        let dv = vb - va;
        let start = hermite_shape_at(scale, ua) + extrusion * va;
        let end = hermite_shape_at(scale, ub) + extrusion * vb;
        let start_tangent = hermite_shape_tangent(scale, ua) * du + extrusion * dv;
        let end_tangent = hermite_shape_tangent(scale, ub) * du + extrusion * dv;
        let curve = HermiteCurve::new(start, start_tangent, end, end_tangent, 3);
        curves.push(Some(Curve::from(curve)));
    }
    let input = FaceCurvesInput {
        loops: vec![curves.len()],
        curves_3d: Some(curves),
        uv_curves: None,
        orientations: vec![OrientType::Same; HERMITE_SHEET_EDGES.len()],
        loop_points: Vec::new(),
    };
    let dom = Extent2d::from_points(Point2::new(0.0, 0.0), Point2::new(1.0, 1.0));
    let mut state = ArenaNotifyState::new(b);
    let this_face = |face| arena_identity(CacheObject::Face(face));
    let made = {
        let mut env = state.env(None);
        let region = b.infinite_region();
        make_face_with_curves(
            k,
            b,
            region,
            input,
            surface,
            &dom,
            OrientType::Same,
            None,
            &this_face,
            &mut env,
        )?
    };
    let face = made
        .face
        .ok_or_else(|| SolidError::sm_err("own_builder::hermite_sheet_face"))?;
    let edges = solidcore::topology::face::p4::edges(b, face, None, false, None)?;
    if edges.len() != 4 {
        return Err(SolidError::sm_err("own_builder::hermite_sheet_face"));
    }
    for edge in edges {
        if !b
            .edge(edge)?
            .curve
            .as_ref()
            .is_some_and(|curve| matches!(curve.kind(), CurveKind::Hermite(_)))
        {
            return Err(SolidError::sm_err("own_builder::hermite_sheet_face"));
        }
        solidcore::topology::edge::set_tolerance(b, edge, edge_tol, false, false)?;
    }
    solidcore::topology::face::p3::set_tolerance(b, face, 1e-7, false, false)?;
    Ok(())
}

/// The shim's `init_gaps` walk: `GetVertexFaceGap` on loop and edge vertex uses of a
/// face with a surface, then `GetMaxEdgeFaceGap` and `GetEdgeCCWEdgeGap` on loop edge
/// uses with a CCW neighbour, a face surface and an edge curve, in traversal order
/// (the arena order of a [`renumber_with`]ed B-rep); with `vertex_edge_gaps` each vertex
/// use's `GetVertexEdgeGap` follows when `HasVertexEdgeGap`. Their statuses are
/// ignored, as there; an unported callee is returned.
pub(crate) fn init_gap_walk(
    k: &Kernel,
    b: &mut Brep,
    vertex_edge_gaps: bool,
) -> Result<(), SolidError> {
    use solidcore::topology::vertexuse as vu1;
    let vus: Vec<VertexuseId> = b.vertexuses().ids().collect();
    for vu in vus {
        if vu1::is_loop_vertexuse(b, vu)? || vu1::is_edge_vertexuse(b, vu)? {
            let face = vu1::faceuse(b, vu)?.and_then(|fu| b.faceuse(fu).ok().and_then(|r| r.face));
            let has_surface = match face {
                Some(f) => solidcore::topology::face::p2::surface(b, f)?.is_some(),
                None => false,
            };
            if has_surface {
                keep_unported(vu1::vertex_face_gap(k, b, vu, false))?;
            }
        }
        if vertex_edge_gaps && vu1::has_vertex_edge_gap(b, vu)? {
            keep_unported(vu1::vertex_edge_gap(k, b, vu, false))?;
        }
    }
    let eus: Vec<EdgeuseId> = b.edgeuses().ids().collect();
    for eu in eus {
        if !eu1::is_loop_edgeuse(b, eu)? || eu1::ccw_edgeuse(b, eu)?.is_none() {
            continue;
        }
        let has_surface = match eu1::face(b, eu)? {
            Some(f) => solidcore::topology::face::p2::surface(b, f)?.is_some(),
            None => false,
        };
        let has_curve = match eu1::edge(b, eu)? {
            Some(e) => b.edge(e)?.curve.is_some(),
            None => false,
        };
        if has_surface && has_curve {
            keep_unported(eu1::max_edge_face_gap(k, b, eu, false))?;
            keep_unported(eu1::edge_ccw_edge_gap(k, b, eu, false))?;
        }
    }
    Ok(())
}

/// An ignored status, but an unported callee still defers the case.
fn keep_unported<T>(r: Result<T, SolidError>) -> Result<(), SolidError> {
    match r {
        Err(e) if e.is_unported() => Err(e),
        _ => Ok(()),
    }
}

// ---- traversal order -------------------------------------------------------------------

/// Ids in first-registration order with their positions.
pub(crate) struct Reg<I> {
    /// The ids in registration order.
    pub(crate) items: Vec<I>,
    index: BTreeMap<u32, u32>,
}

impl<I: Copy> Reg<I> {
    fn new() -> Self {
        Self {
            items: Vec::new(),
            index: BTreeMap::new(),
        }
    }

    fn add(&mut self, id: Option<I>, raw: impl Fn(I) -> u32) {
        let Some(id) = id else {
            return;
        };
        let r = raw(id);
        if self.index.contains_key(&r) {
            return;
        }
        let at = u32::try_from(self.items.len()).unwrap_or(u32::MAX);
        self.index.insert(r, at);
        self.items.push(id);
    }

    fn get(&self, raw: u32) -> Option<u32> {
        self.index.get(&raw).copied()
    }
}

/// The objects of a B-rep in the shim's traversal order.
pub(crate) struct Walk {
    pub(crate) regions: Reg<RegionId>,
    pub(crate) shells: Reg<ShellId>,
    pub(crate) faceuses: Reg<FaceuseId>,
    pub(crate) faces: Reg<FaceId>,
    pub(crate) loopuses: Reg<LoopuseId>,
    pub(crate) loops: Reg<LoopId>,
    pub(crate) edgeuses: Reg<EdgeuseId>,
    pub(crate) edges: Reg<EdgeId>,
    pub(crate) vertexuses: Reg<VertexuseId>,
    pub(crate) vertices: Reg<VertexId>,
}

/// The shim's `walk` (`sc_edgeuse_graph.h`): the regions (`GetRegions`), every edge in
/// the B-rep's edge list order (`GetEdges`), then everything reachable, registering
/// each object the first time it is seen.
///
/// # Errors
///
/// A dangling id.
pub(crate) fn walk(b: &Brep) -> Result<Walk, SolidError> {
    walk_with(b, false)
}

/// [`walk`]; with `prefix_lists` the B-rep's face, edge and vertex lists are registered
/// first.
///
/// # Errors
///
/// A dangling id.
pub(crate) fn walk_with(b: &Brep, prefix_lists: bool) -> Result<Walk, SolidError> {
    walk_order(b, prefix_lists, false, false)
}

/// [`walk_with`]; with `vertex_lists_last` the vertex use lists are followed after the
/// loops' loop uses ([`Conventions::vertex_lists_last`]); with `skip_edge_list` the edge
/// list is not registered ahead of the traversal ([`Conventions::skip_edge_list`]).
fn walk_order(
    b: &Brep,
    prefix_lists: bool,
    vertex_lists_last: bool,
    skip_edge_list: bool,
) -> Result<Walk, SolidError> {
    let mut g = Walk {
        regions: Reg::new(),
        shells: Reg::new(),
        faceuses: Reg::new(),
        faces: Reg::new(),
        loopuses: Reg::new(),
        loops: Reg::new(),
        edgeuses: Reg::new(),
        edges: Reg::new(),
        vertexuses: Reg::new(),
        vertices: Reg::new(),
    };
    if prefix_lists {
        for &id in b.face_list() {
            g.faces.add(Some(id), FaceId::index);
        }
        for &id in b.edge_list() {
            g.edges.add(Some(id), EdgeId::index);
        }
        for &id in b.vertex_list() {
            g.vertices.add(Some(id), VertexId::index);
        }
    }
    for id in solidcore::topology::brep::p1::regions(b, None)? {
        g.regions.add(Some(id), RegionId::index);
    }
    if !skip_edge_list {
        for &id in b.edge_list() {
            g.edges.add(Some(id), EdgeId::index);
        }
    }
    let (mut r, mut s, mut fu, mut lu, mut eu, mut vu, mut l, mut e, mut v) =
        (0, 0, 0, 0, 0, 0, 0, 0, 0);
    let bound = b.edgeuses().len() + 1;
    let mut more = true;
    while more {
        more = false;
        while let Some(&id) = g.regions.items.get(r) {
            for &x in &b.region(id)?.shells {
                g.shells.add(Some(x), ShellId::index);
            }
            r += 1;
            more = true;
        }
        while let Some(&id) = g.shells.items.get(s) {
            match &b.shell(id)?.members {
                ShellMembers::Faceuses(list) => {
                    for &x in list {
                        g.faceuses.add(Some(x), FaceuseId::index);
                    }
                }
                ShellMembers::Edgeuse(x) => g.edgeuses.add(Some(*x), EdgeuseId::index),
                ShellMembers::Vertexuse(x) => g.vertexuses.add(Some(*x), VertexuseId::index),
                ShellMembers::Empty => {}
            }
            s += 1;
            more = true;
        }
        while let Some(&id) = g.faceuses.items.get(fu) {
            let rec = b.faceuse(id)?;
            g.faces.add(rec.face, FaceId::index);
            g.faceuses.add(rec.mate, FaceuseId::index);
            for &x in &rec.loopuses {
                g.loopuses.add(Some(x), LoopuseId::index);
            }
            fu += 1;
            more = true;
        }
        while let Some(&id) = g.loopuses.items.get(lu) {
            let rec = b.loopuse(id)?;
            g.loops.add(rec.loop_, LoopId::index);
            g.loopuses.add(rec.mate, LoopuseId::index);
            match rec.start {
                LoopuseStart::Edgeuse(start) => {
                    g.edgeuses.add(start, EdgeuseId::index);
                    let mut q = start;
                    let mut n = 0;
                    while let Some(x) = q {
                        if n >= bound {
                            break;
                        }
                        g.edgeuses.add(Some(x), EdgeuseId::index);
                        q = b.edgeuse(x)?.ccw;
                        if q == start {
                            break;
                        }
                        n += 1;
                    }
                }
                LoopuseStart::Vertexuse(x) => g.vertexuses.add(x, VertexuseId::index),
                LoopuseStart::Unknown => {}
            }
            lu += 1;
            more = true;
        }
        while let Some(&id) = g.edgeuses.items.get(eu) {
            let rec = b.edgeuse(id)?;
            g.edges.add(rec.edge, EdgeId::index);
            match rec.owner {
                EdgeuseOwner::Loopuse(x) => g.loopuses.add(x, LoopuseId::index),
                EdgeuseOwner::Shell(x) => g.shells.add(x, ShellId::index),
                EdgeuseOwner::Unknown => {}
            }
            g.edgeuses.add(rec.cw, EdgeuseId::index);
            g.edgeuses.add(rec.ccw, EdgeuseId::index);
            g.vertexuses.add(rec.vertexuse, VertexuseId::index);
            g.edgeuses.add(eu1::edgeuse_next(b, id)?, EdgeuseId::index);
            g.edgeuses.add(eu1::edgeuse_last(b, id)?, EdgeuseId::index);
            eu += 1;
            more = true;
        }
        while let Some(&id) = g.edges.items.get(e) {
            for &x in &b.edge(id)?.edgeuses {
                g.edgeuses.add(Some(x), EdgeuseId::index);
            }
            e += 1;
            more = true;
        }
        while let Some(&id) = g.vertexuses.items.get(vu) {
            let rec = b.vertexuse(id)?;
            g.vertices.add(rec.vertex, VertexId::index);
            match rec.owner {
                VertexuseOwner::Edgeuse(x) => g.edgeuses.add(x, EdgeuseId::index),
                VertexuseOwner::Loopuse(x) => g.loopuses.add(x, LoopuseId::index),
                VertexuseOwner::Shell(x) => g.shells.add(x, ShellId::index),
                VertexuseOwner::Unknown => {}
            }
            vu += 1;
            more = true;
        }
        if !vertex_lists_last {
            while let Some(&id) = g.vertices.items.get(v) {
                for &x in &b.vertex(id)?.vertexuses {
                    g.vertexuses.add(Some(x), VertexuseId::index);
                }
                v += 1;
                more = true;
            }
        }
        while let Some(&id) = g.loops.items.get(l) {
            g.loopuses.add(b.loop_(id)?.loopuse, LoopuseId::index);
            l += 1;
            more = true;
        }
        if vertex_lists_last {
            while let Some(&id) = g.vertices.items.get(v) {
                for &x in &b.vertex(id)?.vertexuses {
                    g.vertexuses.add(Some(x), VertexuseId::index);
                }
                v += 1;
                more = true;
            }
        }
    }
    Ok(g)
}

// ---- renumbering -------------------------------------------------------------------------

/// Maps old ids to their traversal positions.
struct Map<'a>(&'a Walk);

macro_rules! mapper {
    ($name:ident, $field:ident, $ty:ty) => {
        fn $name(&self, id: $ty) -> Result<$ty, HarnessError> {
            self.0
                .$field
                .get(id.index())
                .map(<$ty>::from_index)
                .ok_or_else(|| harness(concat!("renumber: unreached ", stringify!($name))))
        }
    };
}

impl Map<'_> {
    mapper!(region, regions, RegionId);
    mapper!(shell, shells, ShellId);
    mapper!(faceuse, faceuses, FaceuseId);
    mapper!(face, faces, FaceId);
    mapper!(loopuse, loopuses, LoopuseId);
    mapper!(loop_, loops, LoopId);
    mapper!(edgeuse, edgeuses, EdgeuseId);
    mapper!(edge, edges, EdgeId);
    mapper!(vertexuse, vertexuses, VertexuseId);
    mapper!(vertex, vertices, VertexId);

    /// An entity of the source B-rep under its new id (a whole B-rep keeps its id).
    fn entity(&self, e: EntityRef) -> Result<EntityRef, SolidError> {
        let unreached = |_| SolidError::invalid_input("renumber", "unreached cache entity");
        Ok(match e {
            EntityRef::Brep(x) => EntityRef::Brep(x),
            EntityRef::Region(x) => EntityRef::Region(self.region(x).map_err(unreached)?),
            EntityRef::Shell(x) => EntityRef::Shell(self.shell(x).map_err(unreached)?),
            EntityRef::Faceuse(x) => EntityRef::Faceuse(self.faceuse(x).map_err(unreached)?),
            EntityRef::Face(x) => EntityRef::Face(self.face(x).map_err(unreached)?),
            EntityRef::Loopuse(x) => EntityRef::Loopuse(self.loopuse(x).map_err(unreached)?),
            EntityRef::Loop(x) => EntityRef::Loop(self.loop_(x).map_err(unreached)?),
            EntityRef::Edgeuse(x) => EntityRef::Edgeuse(self.edgeuse(x).map_err(unreached)?),
            EntityRef::Edge(x) => EntityRef::Edge(self.edge(x).map_err(unreached)?),
            EntityRef::Vertexuse(x) => EntityRef::Vertexuse(self.vertexuse(x).map_err(unreached)?),
            EntityRef::Vertex(x) => EntityRef::Vertex(self.vertex(x).map_err(unreached)?),
        })
    }
}

/// The renumbered second edge use of edge use `id`'s cached CCW gap
/// (`m_sEdgeCCWEdgeGap3d.m_pEdgeuse1`, `edgeuse1`). A gap stays cached after
/// `SmBrep::SqueezeEdge` takes its CCW neighbour out of the loop and deletes it
/// (`ReadFromFile`'s healer on `Fillet/open_shell.smb`: the C++ gap then names a freed
/// edge use, the Rust one an unlinked arena record). No live entity corresponds, so
/// such a reference takes the convention of the C++ export import, which carries only
/// the gap's `IsInit` flag: the edge use's own CCW edge use (itself without one). Every
/// other unreached reference is still a renumbering error.
fn stale_ccw_gap_edgeuse(
    m: &Map<'_>,
    edgeuse1: Option<EdgeuseId>,
    ccw: Option<EdgeuseId>,
    id: EdgeuseId,
) -> Result<Option<EdgeuseId>, HarnessError> {
    match edgeuse1 {
        Some(x) if m.edgeuse(x).is_err() => Ok(Some(m.edgeuse(ccw.unwrap_or(id))?)),
        _ => opt(edgeuse1, |x| m.edgeuse(x)),
    }
}

fn opt<T, F: Fn(T) -> Result<T, HarnessError>>(
    x: Option<T>,
    f: F,
) -> Result<Option<T>, HarnessError> {
    x.map(f).transpose()
}

/// A copy of `src` whose arena ids are the shim's traversal order ([`walk`]): every
/// record, list order, cached gap and the B-rep's own lists and state, with every id
/// remapped; owned face and edge geometry relabelled to the new ids under the
/// [`Conventions`]' owner identities. Objects the traversal does not reach are left
/// out, as the shim's export leaves them out.
///
/// # Errors
///
/// A reached object referring to one the traversal did not reach.
pub(crate) fn renumber_with(src: &Brep, conv: &Conventions<'_>) -> Result<Brep, HarnessError> {
    let g = walk_order(
        src,
        conv.prefix_lists,
        conv.vertex_lists_last,
        conv.skip_edge_list,
    )
    .map_err(solid)?;
    // the geometry `src` itself counts as owned by its face / edge (`GetSurfaces` /
    // `GetCurves`, under its own naming; either errs on a face without surface or an
    // edge without curve, then any recorded owner counts): relabelled to the new ids
    let owned_surfaces: Vec<*const Surface> = solidcore::topology::brep::p1::surfaces(src, None)
        .unwrap_or_default()
        .into_iter()
        .map(std::ptr::from_ref)
        .collect();
    let owned_curves: Vec<*const Curve> = solidcore::topology::brep::p1::curves(src, None)
        .unwrap_or_default()
        .into_iter()
        .map(std::ptr::from_ref)
        .collect();
    let m = Map(&g);
    let mut b = Brep::new();
    b.set_validate_only(src.validate_only());
    b.set_tolerance(src.tolerance());
    b.set_key(src.key());
    // geometry owners are relabelled under arena identities below, the naming the
    // B-rep keeps by default (`NamingScheme::Unknown` falls back to it)
    b.set_editing_enabled(src.editing_enabled());
    let probe = Kernel::default();
    if src.this_model_size_estimate(&probe, false) != tol::USE_DEFAULT
        || src.this_large_small_size_ratio(&probe, false) != tol::USE_DEFAULT
    {
        return Err(harness("renumber: a B-rep size estimate is set"));
    }
    for &id in &g.regions.items {
        let r = src.region(id).map_err(solid)?;
        b.add_region_record(Region {
            in_brep: r.in_brep,
            shells: r
                .shells
                .iter()
                .map(|&x| m.shell(x))
                .collect::<Result<_, _>>()?,
            is_void: r.is_void,
        })
        .map_err(solid)?;
    }
    b.set_infinite_region_record(opt(src.infinite_region(), |x| m.region(x))?);
    for &id in &g.shells.items {
        let s = src.shell(id).map_err(solid)?;
        let members = match &s.members {
            ShellMembers::Empty => ShellMembers::Empty,
            ShellMembers::Faceuses(v) => {
                ShellMembers::Faceuses(v.iter().map(|&x| m.faceuse(x)).collect::<Result<_, _>>()?)
            }
            ShellMembers::Edgeuse(x) => ShellMembers::Edgeuse(m.edgeuse(*x)?),
            ShellMembers::Vertexuse(x) => ShellMembers::Vertexuse(m.vertexuse(*x)?),
        };
        b.add_shell_record(Shell {
            faces: s
                .faces
                .iter()
                .map(|&x| m.face(x))
                .collect::<Result<_, _>>()?,
            region: opt(s.region, |x| m.region(x))?,
            shell_type: s.shell_type,
            members,
        })
        .map_err(solid)?;
    }
    for &id in &g.faces.items {
        let f = src.face(id).map_err(solid)?;
        let owned = f.surface.as_ref().is_some_and(|x| {
            x.owner().is_some() || owned_surfaces.contains(&std::ptr::from_ref(x))
        });
        let mut surface = f.surface.clone();
        let new = m.face(id)?;
        if let Some(s) = surface.as_mut().filter(|_| owned) {
            let _ = s.set_owner(Some((conv.face_owner)(new)));
        }
        // the face surface's cache slot as the Rust read left it (its tree state and
        // kept trimmed cache, renamed into the new ids): the copy stands for the same
        // surface object, and `Surface::clone` gives it an empty slot
        if let (Some(s), Some(from)) = (surface.as_ref(), f.surface.as_ref()) {
            s.copy_cache_slot_of(from, |e| m.entity(e)).map_err(solid)?;
        }
        b.add_face_record(Face {
            shell: m.shell(f.shell)?,
            loops: f
                .loops
                .iter()
                .map(|&x| m.loop_(x))
                .collect::<Result<_, _>>()?,
            surface,
            faceuse: opt(f.faceuse, |x| m.faceuse(x))?,
            uv_domain: f.uv_domain,
            rectangular_trim: f.rectangular_trim,
            tolerance: f.tolerance,
        })
        .map_err(solid)?;
    }
    for &id in &g.loops.items {
        let l = src.loop_(id).map_err(solid)?;
        b.add_loop_record(Loop {
            face: m.face(l.face)?,
            edges: l
                .edges
                .iter()
                .map(|x| {
                    Ok(LoopEdge {
                        edge: m.edge(x.edge)?,
                        forward: x.forward,
                    })
                })
                .collect::<Result<_, HarnessError>>()?,
            loopuse: opt(l.loopuse, |x| m.loopuse(x))?,
        })
        .map_err(solid)?;
    }
    for &id in &g.edges.items {
        let e = src.edge(id).map_err(solid)?;
        let owned = e
            .curve
            .as_ref()
            .is_some_and(|x| x.owner().is_some() || owned_curves.contains(&std::ptr::from_ref(x)));
        let mut curve = e.curve.clone();
        let new = m.edge(id)?;
        if let Some(c) = curve.as_mut().filter(|_| owned) {
            let (owner, brep) = (conv.edge_owner)(new);
            let _ = c.set_owner(Some(owner), Some(brep));
        }
        b.add_edge_record(Edge {
            vertices: [m.vertex(e.vertices[0])?, m.vertex(e.vertices[1])?],
            edgeuses: e
                .edgeuses
                .iter()
                .map(|&x| m.edgeuse(x))
                .collect::<Result<_, _>>()?,
            interval: e.interval,
            tolerance: e.tolerance,
            curve,
        })
        .map_err(solid)?;
    }
    for &id in &g.vertices.items {
        let v = src.vertex(id).map_err(solid)?;
        b.add_vertex_record(Vertex {
            point: v.point,
            tolerance: v.tolerance,
            vertexuses: v
                .vertexuses
                .iter()
                .map(|&x| m.vertexuse(x))
                .collect::<Result<_, _>>()?,
            poly_vertex: v.poly_vertex,
        })
        .map_err(solid)?;
    }
    for &id in &g.faceuses.items {
        let f = src.faceuse(id).map_err(solid)?;
        b.add_faceuse_record(Faceuse {
            shell: opt(f.shell, |x| m.shell(x))?,
            face: opt(f.face, |x| m.face(x))?,
            mate: opt(f.mate, |x| m.faceuse(x))?,
            orientation: f.orientation,
            loopuses: f
                .loopuses
                .iter()
                .map(|&x| m.loopuse(x))
                .collect::<Result<_, _>>()?,
            sibling_links: f
                .sibling_links
                .map(|(a, c)| -> Result<_, HarnessError> {
                    Ok((opt(a, |x| m.faceuse(x))?, opt(c, |x| m.faceuse(x))?))
                })
                .transpose()?,
        })
        .map_err(solid)?;
    }
    for &id in &g.loopuses.items {
        let l = src.loopuse(id).map_err(solid)?;
        b.add_loopuse_record(Loopuse {
            faceuse: opt(l.faceuse, |x| m.faceuse(x))?,
            loop_: opt(l.loop_, |x| m.loop_(x))?,
            mate: opt(l.mate, |x| m.loopuse(x))?,
            start: match l.start {
                LoopuseStart::Unknown => LoopuseStart::Unknown,
                LoopuseStart::Edgeuse(x) => LoopuseStart::Edgeuse(opt(x, |y| m.edgeuse(y))?),
                LoopuseStart::Vertexuse(x) => LoopuseStart::Vertexuse(opt(x, |y| m.vertexuse(y))?),
            },
            orientation: l.orientation,
        })
        .map_err(solid)?;
    }
    for &id in &g.edgeuses.items {
        let e = src.edgeuse(id).map_err(solid)?;
        let mut max_edge_face_gap = e.max_edge_face_gap;
        max_edge_face_gap.edgeuse = opt(max_edge_face_gap.edgeuse, |x| m.edgeuse(x))?;
        let mut edge_ccw_edge_gap = e.edge_ccw_edge_gap;
        edge_ccw_edge_gap.edgeuse0 = opt(edge_ccw_edge_gap.edgeuse0, |x| m.edgeuse(x))?;
        edge_ccw_edge_gap.edgeuse1 =
            stale_ccw_gap_edgeuse(&m, edge_ccw_edge_gap.edgeuse1, e.ccw, id)?;
        b.add_edgeuse_record(Edgeuse {
            edge: opt(e.edge, |x| m.edge(x))?,
            owner: match e.owner {
                EdgeuseOwner::Unknown => EdgeuseOwner::Unknown,
                EdgeuseOwner::Loopuse(x) => EdgeuseOwner::Loopuse(opt(x, |y| m.loopuse(y))?),
                EdgeuseOwner::Shell(x) => EdgeuseOwner::Shell(opt(x, |y| m.shell(y))?),
            },
            ccw: opt(e.ccw, |x| m.edgeuse(x))?,
            cw: opt(e.cw, |x| m.edgeuse(x))?,
            vertexuse: opt(e.vertexuse, |x| m.vertexuse(x))?,
            orientation: e.orientation,
            uv_trim_curve: e.uv_trim_curve.clone(),
            max_edge_face_gap,
            edge_ccw_edge_gap,
            drop_curve_fail: e.drop_curve_fail.clone(),
        })
        .map_err(solid)?;
    }
    for &id in &g.vertexuses.items {
        let v = src.vertexuse(id).map_err(solid)?;
        let mut vertex_edge_gap = v.vertex_edge_gap;
        vertex_edge_gap.vertexuse = opt(vertex_edge_gap.vertexuse, |x| m.vertexuse(x))?;
        let mut vertex_face_gap = v.vertex_face_gap;
        vertex_face_gap.vertexuse = opt(vertex_face_gap.vertexuse, |x| m.vertexuse(x))?;
        b.add_vertexuse_record(Vertexuse {
            vertex: opt(v.vertex, |x| m.vertex(x))?,
            owner: match v.owner {
                VertexuseOwner::Unknown => VertexuseOwner::Unknown,
                VertexuseOwner::Edgeuse(x) => VertexuseOwner::Edgeuse(opt(x, |y| m.edgeuse(y))?),
                VertexuseOwner::Loopuse(x) => VertexuseOwner::Loopuse(opt(x, |y| m.loopuse(y))?),
                VertexuseOwner::Shell(x) => VertexuseOwner::Shell(opt(x, |y| m.shell(y))?),
            },
            vertex_edge_gap,
            vertex_face_gap,
            uv_point: v.uv_point,
        })
        .map_err(solid)?;
    }
    // the B-rep's own lists, in their list order
    b.set_region_list_head(opt(src.region_list_head(), |x| m.region(x))?);
    b.set_face_list(
        src.face_list()
            .iter()
            .map(|&x| m.face(x))
            .collect::<Result<_, _>>()?,
    )
    .map_err(solid)?;
    b.set_edge_list(
        src.edge_list()
            .iter()
            .map(|&x| m.edge(x))
            .collect::<Result<_, _>>()?,
    )
    .map_err(solid)?;
    b.set_vertex_list(
        src.vertex_list()
            .iter()
            .map(|&x| m.vertex(x))
            .collect::<Result<_, _>>()?,
    )
    .map_err(solid)?;
    let faces: Vec<FaceId> = b.faces().ids().collect();
    for f in faces {
        b.sync_face_surface_tolerance(f).map_err(solid)?;
    }
    Ok(b)
}

/// The entity counts of a B-rep in the order of the shim's export header (regions,
/// shells, face uses, faces, loop uses, loops, edge uses, edges, vertex uses,
/// vertices).
pub(crate) fn counts(b: &Brep) -> [usize; 10] {
    [
        b.regions().len(),
        b.shells().len(),
        b.faceuses().len(),
        b.faces().len(),
        b.loopuses().len(),
        b.loops().len(),
        b.edgeuses().len(),
        b.edges().len(),
        b.vertexuses().len(),
        b.vertices().len(),
    ]
}

/// The `SmStatus` the shim reports for a failed build of the port's error `e`: an
/// upstream code as is, `SM_ERR` otherwise.
pub(crate) fn build_status(e: &SolidError) -> f64 {
    match e {
        SolidError::Upstream { code, .. } => f64::from(*code),
        SolidError::InvalidInput { .. } => f64::from(solidcore::error::SM_ERR_INVALID_INPUT),
        _ => f64::from(solidcore::error::SM_ERR),
    }
}
