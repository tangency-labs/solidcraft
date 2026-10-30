//! Native B-rep construction calls and dependency-limited Rust entrypoints.
//!
//! The Rust side builds a wire edge, face or network source itself
//! ([`crate::own_builder`]), reads an `.smb` file with the Rust reader
//! ([`crate::own_builder::read_with`]), and repeats the shim's setup edits
//! ([`own_setup`]) on either, so no C++ state reaches it. Split refusals compare
//! their retained endpoint as well as the complete semantic topology/geometry
//! snapshot. Successful native mutations on deferred paths are not parity passes.
use super::boolean_merge_p1::{graph_reachable, Canon};
use super::topology_edgeuse_p1::{self as spec, BrepIn};
use super::topology_vertexuse::import_raw;
use crate::{Case, CaseContext, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use solidcore::curve::curve::Curve;
use solidcore::curve::line::Line;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::tol::zone_tol3d;
use solidcore::geom::vector2d::Point2;
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::topology::brep::p9 as port;
use solidcore::topology::{ArenaId, Brep, EdgeId, FaceId, RegionId, VertexuseId};
use solidcore::SolidError;
use solidcore_ffi::units::topology_brep_p9 as ffi;

/// Differential runner for `topology.brep_p9`.
pub struct Runner;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    op: u8,
    brep: BrepIn,
    #[serde(default)]
    selector: u8,
    #[serde(default)]
    variant: u8,
    #[serde(default = "yes")]
    editing: bool,
    #[serde(default = "yes")]
    same: bool,
    #[serde(default, with = "nf")]
    parameter: f64,
    #[serde(default, with = "nf_array")]
    point: [f64; 3],
    #[serde(default, with = "nf_array")]
    tolerances: [f64; 2],
}
// JSON numbers or the explicit strings "NaN", "inf" and "-inf".
mod nf {
    use serde::de::Error as _;
    use serde::{Deserialize, Deserializer, Serializer};
    use serde_json::Value;

    #[allow(clippy::trivially_copy_pass_by_ref)] // serde's `with` signature
    pub fn serialize<S: Serializer>(x: &f64, s: S) -> Result<S::Ok, S::Error> {
        if x.is_nan() {
            s.serialize_str("NaN")
        } else if x.is_infinite() {
            s.serialize_str(if *x > 0.0 { "inf" } else { "-inf" })
        } else {
            s.serialize_f64(*x)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
        match Value::deserialize(d)? {
            Value::Number(n) => n.as_f64().ok_or_else(|| D::Error::custom("bad number")),
            Value::String(s) if s == "NaN" => Ok(f64::NAN),
            Value::String(s) if s == "inf" => Ok(f64::INFINITY),
            Value::String(s) if s == "-inf" => Ok(f64::NEG_INFINITY),
            other => Err(D::Error::custom(format!("bad float {other}"))),
        }
    }
}
// Arrays of `nf` values.
mod nf_array {
    use serde::de::Error as _;
    use serde::ser::SerializeTuple;
    use serde::{Deserialize, Deserializer, Serializer};
    use serde_json::Value;

    struct One(f64);
    impl serde::Serialize for One {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            super::nf::serialize(&self.0, s)
        }
    }

    pub fn serialize<S: Serializer, const N: usize>(x: &[f64; N], s: S) -> Result<S::Ok, S::Error> {
        let mut t = s.serialize_tuple(N)?;
        for v in x {
            t.serialize_element(&One(*v))?;
        }
        t.end()
    }

    pub fn deserialize<'de, D: Deserializer<'de>, const N: usize>(
        d: D,
    ) -> Result<[f64; N], D::Error> {
        let values = Vec::<Value>::deserialize(d)?;
        let values = values
            .into_iter()
            .map(|v| super::nf::deserialize(v).map_err(D::Error::custom))
            .collect::<Result<Vec<f64>, D::Error>>()?;
        values
            .try_into()
            .map_err(|_| D::Error::custom(format!("expected {N} floats")))
    }
}

fn yes() -> bool {
    true
}
fn error(e: &(impl ToString + ?Sized)) -> HarnessError {
    HarnessError::new(e.to_string())
}
fn id<I: ArenaId>(v: Option<I>) -> f64 {
    v.map_or(-1.0, |v| f64::from(v.index()))
}
fn status(e: &SolidError) -> Result<Output, HarnessError> {
    if let Some(out) = Output::unported(e) {
        return Ok(out);
    }
    Err(error(e))
}
fn ordinary<T>(r: solidcore::Result<T>) -> Result<i32, SolidError> {
    match r {
        Ok(_) => Ok(0),
        Err(SolidError::Upstream { code, .. }) => Ok(code),
        Err(e) => Err(e),
    }
}
fn request<'a>(i: &Inputs, path: &'a str, block: &'a [f64]) -> ffi::Request<'a> {
    ffi::Request {
        op: i.op,
        selector: i.selector,
        variant: i.variant,
        editing: i.editing,
        same: i.same,
        path,
        block,
        parameter: i.parameter,
        point: i.point,
        tolerances: i.tolerances,
    }
}
fn target<I: ArenaId>(count: usize, selector: u8) -> Option<I> {
    (count > 0).then(|| I::from_index(u32::from(selector) % count as u32))
}
/// The shim's build conventions (`sc_topology_fixture.h`'s `build` and `walk`; the
/// fixture's traversal, `GetVertexEdgeGap` only matters with `init_gaps`, which is off),
/// faces and edges owning their surfaces and curves under the arena identities op 0's
/// `BrepQuery::default()` names them by.
const CONVENTIONS: crate::own_builder::Conventions<'static> =
    crate::own_builder::Conventions::EDGEUSE_GRAPH;

/// The shim's setup edits of the built B-rep `b` (in its walk order) before the
/// export: op 5 variant 0 with editing, two `MakeVertexLoop`s on the selected face at
/// fractions `(0.3, 0.4)` and `(0.7, 0.6)` of its UV domain, then a fresh walk; ops
/// 4, 6 and 7, on the selected edge, an `SmLine` between its interval ends
/// (`EditParameterization` to the interval, `SetCurve(line, TRUE)`, `SetOwner`) for
/// variants 1 and 3, the primary edge use's mate made primary for variants 2 and 3,
/// and the start and end vertex tolerances. The B-rep after the setup and the start
/// and end vertex uses' walk indices (`-1` for none), or `None` where the shim
/// returns `SC_ERR_ARG`.
fn own_setup(
    k: &mut Kernel,
    b: Brep,
    i: &Inputs,
) -> Result<Option<(Brep, [f64; 2])>, HarnessError> {
    use solidcore::topology::brep::p2::ArenaNotifyState;
    let fail = |e: SolidError| error(&e);
    let mut b = b;
    if i.op == 5 && i.variant == 0 && i.editing {
        let Some(face) = target::<FaceId>(b.faces().len(), i.selector) else {
            return Ok(None);
        };
        let d = solidcore::topology::face::p2::uv_domain(&b, face).map_err(fail)?;
        let Some(surface) = solidcore::topology::face::p2::surface(&b, face)
            .map_err(fail)?
            .cloned()
        else {
            return Ok(None);
        };
        let uv0 = Point2::new(d.min().x + 0.3 * d.x_length(), d.min().y + 0.4 * d.y_length());
        let uv1 = Point2::new(d.min().x + 0.7 * d.x_length(), d.min().y + 0.6 * d.y_length());
        let (Ok(p), Ok(q)) = (surface.evaluate_point(k, uv0), surface.evaluate_point(k, uv1))
        else {
            return Ok(None);
        };
        let mut state = ArenaNotifyState::new(&b);
        let mut loops = Vec::new();
        for point in [p, q] {
            let made = {
                let mut env = state.env(None);
                port::make_vertex_loop(&mut b, face, point, OrientType::Opposite, &mut env)
            };
            let Ok((l, _)) = made else {
                return Ok(None);
            };
            state.seed(&b);
            loops.push(l);
        }
        let mut vus = Vec::new();
        for l in loops {
            let lu = b.loop_(l).map_err(fail)?.loopuse;
            let vu = match lu.map(|lu| b.loopuse(lu).map(|r| r.start)) {
                Some(Ok(solidcore::topology::LoopuseStart::Vertexuse(vu))) => vu,
                _ => None,
            };
            vus.push(vu);
        }
        // the fresh walk: the walk order of the edited B-rep, with the new vertex uses'
        // indices in it
        let g = crate::own_builder::walk_with(&b, false).map_err(fail)?;
        let index = |vu: Option<VertexuseId>| {
            vu.and_then(|vu| g.vertexuses.items.iter().position(|&x| x == vu))
                .map_or(-1.0, |x| x as f64)
        };
        let ids = [
            index(vus.first().copied().flatten()),
            index(vus.get(1).copied().flatten()),
        ];
        let b = crate::own_builder::renumber_with(&b, &CONVENTIONS)?;
        return Ok(Some((b, ids)));
    }
    if matches!(i.op, 4 | 6 | 7) {
        let Some(edge) = target::<EdgeId>(b.edges().len(), i.selector) else {
            return Ok(None);
        };
        let rec = b.edge(edge).map_err(fail)?;
        let (Some(curve), Some(v0), Some(v1)) = (
            rec.curve.clone(),
            solidcore::topology::edge::start_vertex(&b, edge).map_err(fail)?,
            solidcore::topology::edge::end_vertex(&b, edge).map_err(fail)?,
        ) else {
            return Ok(None);
        };
        let ivl = rec.interval;
        if matches!(i.variant, 1 | 3) {
            let (Ok(p), Ok(q)) = (
                curve.evaluate_point(k, ivl.min()),
                curve.evaluate_point(k, ivl.max()),
            ) else {
                return Ok(None);
            };
            let mut line = Line::from_points(p, q, 3);
            if line.edit_parameterization(&ivl).is_err() {
                return Ok(None);
            }
            let mut state = ArenaNotifyState::new(&b);
            {
                let mut env = state.env(None);
                solidcore::topology::edge::set_curve(
                    &mut b,
                    edge,
                    Some(Curve::from(line)),
                    solidcore::topology::edge::SetCurveOptions {
                        delete_old_curve: true,
                        ..solidcore::topology::edge::SetCurveOptions::default()
                    },
                    &mut env,
                    [None, None],
                )
                .map_err(fail)?;
            }
            if let Some(c) = b.edge_mut(edge).map_err(fail)?.curve.as_mut() {
                let (owner, brep) = (CONVENTIONS.edge_owner)(edge);
                let _ = c.set_owner(Some(owner), Some(brep));
            }
        }
        if matches!(i.variant, 2 | 3) {
            let primary = solidcore::topology::edgeuse::p1::primary_edgeuse(&b, edge)
                .map_err(fail)?
                .ok_or_else(|| error("edge without primary edge use"))?;
            let mate = solidcore::topology::edgeuse::p1::mate(&b, primary).map_err(fail)?;
            solidcore::topology::edge::reset_primary_edgeuse(&mut b, edge, mate)
                .map_err(fail)?;
        }
        // `SetTolerance(SmZoneTol3d(raw), FALSE)`
        let [t0, t1] = i.tolerances.map(|t| zone_tol3d(k, t));
        solidcore::topology::vertex::set_tolerance(&mut b, v0, t0, false, true).map_err(fail)?;
        solidcore::topology::vertex::set_tolerance(&mut b, v1, t1, false, true).map_err(fail)?;
    }
    Ok(Some((b, [-1.0, -1.0])))
}

/// The Rust B-rep after the shim's setup and the start/end vertex use indices: the
/// source made by the Rust kernel (a shim-built source by [`crate::own_builder`], an
/// `.smb` file by the Rust reader, [`crate::own_builder::read_with`], both in the shim's
/// walk order), then [`own_setup`].
fn rust_input(i: &Inputs) -> Result<(Brep, [f64; 2]), HarnessError> {
    use crate::own_builder::{build_with, read_with, Built};
    let mut k = Kernel::default();
    let built = match &i.brep {
        BrepIn::File { rebuild, .. } => {
            let path = spec::path_of(Some(&i.brep))?;
            read_with(&mut k, &path, *rebuild, false, false, &CONVENTIONS)?
        }
        other => build_with(&mut k, other, false, false, &CONVENTIONS)?
            .ok_or_else(|| error("source not built by the Rust kernel"))?,
    };
    match built {
        Built::Brep(b) => {
            if i.op > 8 || i.variant > 5 {
                return Err(error("native request rejected"));
            }
            own_setup(&mut k, *b, i)?.ok_or_else(|| error("native request rejected"))
        }
        Built::Failed(e) => Err(error(&format!("native build failed: {e}"))),
    }
}

fn rust(i: &Inputs, mut b: Brep, initial: [f64; 2]) -> Result<Output, HarnessError> {
    // The build owns each edge's curve and each face's surface under the arena keys op
    // 0's `BrepQuery::default()` names them by ([`CONVENTIONS`]; no owner is added to
    // geometry the build left without one); a face-owned surface carries the face's
    // tolerance.
    for f in b.faces().ids().collect::<Vec<_>>() {
        b.sync_face_surface_tolerance(f).map_err(|e| error(&e))?;
    }
    // As the shim, after setup and export: `b->m_bEditingEnabled = editing`.
    b.set_editing_enabled(i.editing);
    let mut k = Kernel::default();
    let mut record = Vec::new();
    let mut records = solidcore::topology::topology::NotifyRecords::new();
    let mut attributes = solidcore::geom::attribute::AttributeStore::default();
    let mut env = solidcore::topology::edge::NotifyEnv {
        records: &mut records,
        attributes: &mut attributes,
        context: None,
        key_of: &object_ref,
    };
    let result: solidcore::Result<i32> = (|| {
        match i.op {
            0 => {
                record.push(id(port::find_infinite_region(
                    &mut k,
                    &mut b,
                    &solidcore::query::brep::BrepQuery::default(),
                    None,
                    None,
                )?));
                Ok(0)
            }
            1 => {
                let face = if i.variant == 1 {
                    None
                } else {
                    target::<FaceId>(b.faces().len(), i.selector)
                };
                ordinary(port::orient_sheet_surfaces(&mut k, &mut b, face, i.same))
            }
            2 => {
                let region = if i.variant == 1 {
                    None
                } else {
                    target::<RegionId>(b.regions().len(), i.selector)
                };
                let (shell, vertex) =
                    port::make_shell_vertex(&mut b, region, Vec3::from(i.point), &mut env)?;
                record.extend([id(Some(shell)), id(Some(vertex))]);
                Ok(0)
            }
            3 => {
                let face = target::<FaceId>(b.faces().len(), i.selector)
                    .ok_or_else(|| SolidError::invalid_input("difftest::brep_p9", "no face"))?;
                let (loop_, vertex) = port::make_vertex_loop(
                    &mut b,
                    face,
                    Vec3::from(i.point),
                    if i.same {
                        OrientType::Same
                    } else {
                        OrientType::Opposite
                    },
                    &mut env,
                )?;
                record.extend([id(Some(loop_)), id(Some(vertex))]);
                Ok(0)
            }
            4 => {
                let edge = target::<EdgeId>(b.edges().len(), i.selector)
                    .ok_or_else(|| SolidError::invalid_input("difftest::brep_p9", "no edge"))?;
                // Native tolerances were assigned before export. They have already
                // undergone SmZoneTol3d conversion and are imported verbatim.
                let split = port::make_vertex_split_edge(
                    &mut k,
                    &mut b,
                    edge,
                    i.parameter,
                    &mut env,
                    &curve_ref,
                )?;
                record.extend([
                    id(split.edges.map(|v| v[0])),
                    id(split.edges.map(|v| v[1])),
                    id(split.vertex),
                ]);
                Ok(split.status())
            }
            5 => {
                let face = if i.variant == 1 {
                    None
                } else {
                    target::<FaceId>(b.faces().len(), i.selector)
                };
                let vu = |slot: usize| {
                    initial
                        .get(slot)
                        .copied()
                        .filter(|v| *v >= 0.0)
                        .map(|v| VertexuseId::from_index(v as u32))
                };
                let start = vu(0);
                let end = vu(1);
                let point = |v: Option<VertexuseId>, default| -> solidcore::Result<Vec3> {
                    if let Some(v) = v {
                        let vertex = b
                            .vertexuse(v)?
                            .vertex
                            .ok_or_else(|| SolidError::null_pointer("difftest::brep_p9"))?;
                        Ok(b.vertex(vertex)?.point)
                    } else {
                        Ok(default)
                    }
                };
                let p = point(start, Vec3::new(0.0, 0.0, 0.0))?;
                let q = point(end, Vec3::new(1.0, 1.0, 0.0))?;
                let curve = (i.variant != 2).then(|| Curve::from(Line::from_points(p, q, 3)));
                let output = port::make_edge_in_face_with_vu(
                    &k,
                    &mut b,
                    port::EdgeInFaceInput {
                        face,
                        vertices: [
                            if i.variant == 3 { None } else { start },
                            if i.variant == 4 { None } else { end },
                        ],
                        curve,
                        uv_trim: None,
                        drop_fail: None,
                        interval: Extent1d::from_min_max(0.0, 1.0),
                        orientation: OrientType::Same,
                        curve_zone_tol_3d: zone_tol3d(&k, 1e-7),
                    },
                    &mut env,
                    &curve_ref,
                );
                match &output {
                    Ok(out) => record.extend([id(Some(out.edge)), id(out.loop_), id(out.face)]),
                    Err(_) => record.extend([-1.0, -1.0, -1.0]),
                }
                ordinary(output)
            }
            6 | 7 => {
                use solidcore::intersect::point_class::{
                    BrepMerge, PointClassType, PointClassification,
                };
                use solidcore::EntityRef;
                let edge = target::<EdgeId>(b.edges().len(), i.selector)
                    .ok_or_else(|| SolidError::sm_err("difftest::brep_p9"))?;
                let target = if i.op == 6 {
                    EntityRef::Edge(edge)
                } else {
                    EntityRef::Vertex(
                        solidcore::topology::edge::start_vertex(&b, edge)?
                            .ok_or_else(|| SolidError::sm_err("difftest::brep_p9"))?,
                    )
                };
                let mut pc = PointClassification::new(zone_tol3d(&k, 1e-7));
                pc.set_class_object(
                    if i.op == 6 {
                        PointClassType::Edge
                    } else {
                        PointClassType::Vertex
                    },
                    Some(Handle(target)),
                );
                pc.set_t_param(i.parameter, false);
                if i.variant == 4 {
                    pc.unset_parameters();
                }
                if !i.same {
                    pc.set_sss_point(Vec3::from(i.point));
                }
                let mut records = solidcore::topology::topology::NotifyRecords::new();
                let mut attributes = solidcore::geom::attribute::AttributeStore::default();
                let mut env = solidcore::topology::edge::NotifyEnv {
                    records: &mut records,
                    attributes: &mut attributes,
                    context: None,
                    key_of: &object_ref,
                };
                let result = pc.merge_into_brep(
                    &mut k,
                    Vec3::from(i.point),
                    BrepMerge {
                        brep: &mut b,
                        target,
                        handle: &Handle,
                        notifications: &mut env,
                        curve_ref: &curve_ref,
                    },
                );
                let vertex = match pc.object() {
                    Some(Handle(EntityRef::Vertex(v))) => Some(v),
                    _ => None,
                };
                let sizes = match &result {
                    Ok(out) => (out.new_edges.len(), out.new_vertices.len()),
                    Err(_) => (0, 0),
                };
                record.extend([
                    f64::from(pc.point_class().code()),
                    id(vertex),
                    sizes.0 as f64,
                    sizes.1 as f64,
                    f64::from(u8::from(pc.are_parameters_set())),
                    pc.t_param(),
                ]);
                ordinary(result)
            }
            8 => {
                let face = target::<FaceId>(b.faces().len(), i.selector)
                    .ok_or_else(|| SolidError::sm_err("difftest::brep_p9"))?;
                let f = b.face(face)?;
                let surface = f
                    .surface
                    .as_ref()
                    .ok_or_else(|| SolidError::sm_err("difftest::brep_p9"))?
                    .clone();
                let y = if i.variant == 1 { 2.0 } else { 0.5 };
                let (x0, x1) = if i.variant == 2 {
                    (0.25, 0.75)
                } else {
                    (-1.0, 2.0)
                };
                let interval = Extent1d::from_min_max(-3.0, 7.0);
                let mut line = Line::from_points(
                    surface.evaluate_point(&k, Point2::new(x0, y))?,
                    surface.evaluate_point(&k, Point2::new(x1, y))?,
                    3,
                );
                line.edit_parameterization(&interval)?;
                // the face-owned surface: the drop reaches its face
                let dropped =
                    port::drop_edge_curve_on_face(&k, &mut b, face, &Curve::from(line), &interval)?;
                record.extend([
                    dropped.max_drop_to_surf,
                    dropped.max_approx_dev,
                    dropped.uv_curves.len() as f64,
                ]);
                for curve in dropped.uv_curves {
                    let d = curve.natural_interval();
                    record.extend([d.min(), d.max()]);
                    for t in [d.min(), d.min() + 0.5 * (d.max() - d.min()), d.max()] {
                        let p = curve.evaluate_point(&k, t)?;
                        record.extend([p.x, p.y, p.z]);
                    }
                }
                Ok(0)
            }
            _ => Err(SolidError::invalid_input("difftest::brep_p9", "bad op")),
        }
    })();
    match result {
        Ok(code) => {
            record.insert(0, f64::from(code));
            canon_record(&b, i.op, &mut record);
            skeleton_caches(&mut b);
            Ok(graph_reachable(&b, record))
        }
        Err(SolidError::Upstream { code, .. }) if i.op <= 4 => {
            // These APIs clear their pointer outputs before the first fallible
            // operation. Compare defined native failures as statuses, not harness
            // errors; any partial graph changes are still compared normally.
            let slots = match i.op {
                0 => 1,
                2 | 3 => 2,
                4 => 3,
                _ => 0,
            };
            record.resize(slots, -1.0);
            record.insert(0, f64::from(code));
            canon_record(&b, i.op, &mut record);
            skeleton_caches(&mut b);
            Ok(graph_reachable(&b, record))
        }
        Err(e) => status(&e),
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Handle(solidcore::EntityRef);
impl solidcore::intersect::point_class::ClassObject for Handle {
    fn class_kind(&self) -> solidcore::intersect::point_class::ClassObjectKind {
        use solidcore::intersect::point_class::ClassObjectKind as K;
        use solidcore::EntityRef as E;
        match self.0 {
            E::Edge(_) => K::Edge,
            E::Vertex(_) => K::Vertex,
            E::Face(_) => K::Face,
            E::Region(_) => K::Region,
            _ => K::Other,
        }
    }
}
/// Empties the Rust-only cached lists the graph would otherwise compare: the face loop
/// and loop edge lists (`Face::loops`, `Loop::edges`, which `make_edge_in_face_with_vu`
/// and friends keep in step; the shared importer leaves them empty) and the shells'
/// face lists (`Shell::faces`: the Rust reader's refresh lists a face in every shell one
/// of its face uses is in, the importer only in its first face use's, so a B-rep whose
/// faces have face uses in two shells, as Mouse.smb's void and solid shells, lists them
/// twice). Upstream keeps none of these lists (`GetFaces` walks the face uses), so on
/// both sides the graph compares the upstream topology links.
fn skeleton_caches(b: &mut Brep) {
    let faces: Vec<FaceId> = b.faces().ids().collect();
    for f in faces {
        if let Ok(r) = b.face_mut(f) {
            r.loops.clear();
        }
    }
    let loops: Vec<solidcore::topology::LoopId> = b.loops().ids().collect();
    for l in loops {
        if let Ok(r) = b.loop_mut(l) {
            r.edges.clear();
        }
    }
    let shells: Vec<solidcore::topology::ShellId> = b.shells().ids().collect();
    for s in shells {
        if let Ok(r) = b.shell_mut(s) {
            r.faces.clear();
        }
    }
}

/// Puts the B-rep ids of `record` (status first) in [`Canon`] numbering, the shim's
/// `fresh.find` of a walk after the op.
fn canon_record(b: &Brep, op: u8, record: &mut [f64]) {
    let c = Canon::reachable(b);
    let mut map = |at: usize, f: &dyn Fn(f64) -> f64| {
        if let Some(x) = record.get_mut(at) {
            *x = f(*x);
        }
    };
    match op {
        0 => map(1, &|x| c.regions.of_slot(x)),
        2 => {
            map(1, &|x| c.shells.of_slot(x));
            map(2, &|x| c.vertices.of_slot(x));
        }
        3 => {
            map(1, &|x| c.loops.of_slot(x));
            map(2, &|x| c.vertices.of_slot(x));
        }
        4 => {
            map(1, &|x| c.edges.of_slot(x));
            map(2, &|x| c.edges.of_slot(x));
            map(3, &|x| c.vertices.of_slot(x));
        }
        5 => {
            map(1, &|x| c.edges.of_slot(x));
            map(2, &|x| c.loops.of_slot(x));
            map(3, &|x| c.faces.of_slot(x));
        }
        6 | 7 => map(2, &|x| c.vertices.of_slot(x)),
        _ => {}
    }
}

fn object_ref(e: solidcore::EntityRef) -> solidcore::geom::object::ObjectRef {
    super::boolean_topology_intersector_p1::p9_entity_ref(e)
}

fn curve_ref(edge: EdgeId, kind: i64) -> solidcore::geom::object::ObjectRef {
    solidcore::geom::object::ObjectRef {
        key: solidcore::geom::attribute::ObjectKey(((kind as u64) << 32) | u64::from(edge.index())),
        type_code: kind,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.brep_p9"
    }
    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let path = spec::path_of(Some(&i.brep))?;
        let block = spec::brep_block_of(Some(&i.brep), false, false)?;
        let raw = ffi::eval(request(&i, &path, &block), false).map_err(|e| error(&e))?;
        let n = *raw.get(1).ok_or_else(|| error("short native output"))? as usize;
        let mut record = vec![*raw.first().ok_or_else(|| error("missing native status"))?];
        record.extend(
            raw.get(2..2 + n)
                .ok_or_else(|| error("short native record"))?,
        );
        let (mut b, _) = import_raw(
            raw.get(2 + n..)
                .ok_or_else(|| error("short native graph"))?,
        )?;
        canon_record(&b, i.op, &mut record);
        // the same Rust-only lists emptied as on the Rust side
        skeleton_caches(&mut b);
        Ok(graph_reachable(&b, record))
    }
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let (b, initial) = rust_input(&i)?;
        rust(&i, b, initial)
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let scale = *[1e-6, 1.0, 1e6].get(rng.below(3) as usize)?;
        let origin = rng.point3(-scale, scale);
        let a: f64 = *[0.0, -7.0, 1000.0].get(rng.below(3) as usize)?;
        let width: f64 = *[1.0, 2.5, 10.0].get(rng.below(3) as usize)?;
        let t0 = a + 0.125 * width;
        let t1 = a + 0.875 * width;
        let p = *[
            t0 - 0.1 * width,
            t0,
            t0 + 1e-13 * (1.0 + t0.abs()),
            t0 + 0.001 * width,
            t0 + 0.5 * (t1 - t0),
            t1 - 0.001 * width,
            t1,
            t1 + 0.1 * width,
        ]
        .get(rng.below(8) as usize)?;
        let wire = json!({"kind":"wire","curve":{"dim":3,"cpts":[[origin[0],origin[1],origin[2],1.0],
            [origin[0]+scale,origin[1]+0.5*scale,origin[2],1.0]],"p":1,"u":[a,a,a+width,a+width]},"t0":t0,"t1":t1});
        Some(
            json!({"op":4,"brep":wire,"variant":rng.below(4),"parameter":p,
            "editing":rng.below(10)!=0,"tolerances":[scale* *[0.0,0.0001,0.1,2.0].get(rng.below(4) as usize)?,scale* *[0.0,0.0001,0.1,2.0].get(rng.below(4) as usize)?]}),
        )
    }
    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        crate::runner::default_cases(
            self,
            &CaseContext {
                root: ctx.root.clone(),
                seeds: ctx.seeds.max(512),
            },
        )
    }
}
