//! A B-rep record ([`solidcore_record::brep`]) written into a CADIR model: the inverse of
//! [`crate::read_bodies`], following its conventions (CADIR's topology is STEP's) and the STEP
//! writer's choices (`solidcore_step::write::topology`), so that the record read back is the one
//! written.
//!
//! One record is one body: a solid when every face shell bounds material (a region of CADIR's
//! per material region, its outer shell first and then its voids'), a sheet when none does (a
//! region per piece of its open shells, those that touch sharing a vertex: a CADIR region is one
//! connected piece of its body), and a general body when it has both. Each record face, edge and
//! vertex is one CADIR face, edge and vertex; each loop and oriented edge one loop and coedge;
//! each surface and curve one carrier.
//!
//! Sides: a record keeps its loops relative to the natural normal of the surface its (u, v) are
//! on, and says per shell whether the face's normal there is that normal (`OrientedFace`). A
//! CADIR face is the side its shell uses: its sense compares that side with its carrier's normal,
//! and its loops run counter-clockwise about that side (each loop's coedges in traversal order),
//! so on the other side of the natural normal the record's loops are written reversed, each
//! coedge's sense turned. A coedge's sense compares its traversal with its edge's curve, which
//! runs from the record edge's start vertex (interval min) to its end. A loop the record's
//! pcurves show running counter-clockwise alone is classified the outer one.
//!
//! Geometry ([`geometry`]): each carrier is the exact form the record names (its `surface` or
//! `curve`). Where a record face or edge also holds the NURBS form its parameters are on (every
//! face of a valid record on a cylinder, cone, sphere or torus does), that form is its holder's
//! (the kernel's, or a reader's), which the reader makes again from the named surface or curve,
//! so the named one is written, as the STEP writer writes it: a face with no pcurves (they are in
//! the form's (u, v)), its sense from the form's normal against the named surface's
//! (`form_map`); an edge with its interval on the named curve, from where its start vertex lies
//! to where its end does, the way its form runs (a line named the other way is written turned, so
//! that it runs from the start vertex to the end, as every coedge's sense takes its edge's curve;
//! none for a line whose vertices give no interval: the reader then finds it so, as it does a STEP
//! edge's). A face on a ball patch, a blend or a placed revolution, a torus whose tube
//! reaches its axis, and a face whose form's side cannot be measured against its named surface,
//! are written as their NURBS form, with their pcurves; an edge on a traced intersection, or a
//! polyline held on a form, or a circle or ellipse whose form runs clockwise about its frame
//! (CADIR's arcs run counter-clockwise) or whose arc its vertices do not give, is written as its
//! form. A blend's form is its net: a procedural blend's is certified within its definition's
//! bound ε of the definition, which is not written (CADIR has no form for it; the STEP writer
//! writes the same net), so ε is written as the face's tolerance. What has no exact CADIR form
//! and no NURBS form refuses the record with the reason ([`WriteError`]); nothing else is
//! approximated. An arc of a circle or ellipse starts within its first turn, as CADIR has it: its
//! parameter is moved by whole turns; on a conic so large that the turn's rounding moves its ends
//! past the tolerance, it is written as its form (refused, with the reason, without one).
//!
//! Pcurves: each oriented edge's pcurve is written for the faces whose (u, v) are an affine
//! image of the record's (a plane on its axes' orthonormal frame, a spline on its own), mapped
//! through it and on the parameter its edge is written on (or, for an edge held on a form, the
//! form's); its range is its own interval there (the edge's, to a remap's rounding), in the
//! direction the coedge runs.
//!
//! Tolerances: each vertex's and edge's is written. The reader holds a record to its document's
//! linear tolerance (`cadmpeg_ir::document::CadIr::tolerances`), which the caller sets: a document of one record takes
//! the record's `tolerance`.
//!
//! Input is hostile: the record is checked first ([`solidcore_record::brep::Brep::validate_built`])
//! and every index looked up; a refusal leaves the model as it was. The record is one in memory
//! already (the kernel's, or one a reader made and checked as it came in), and the writing is
//! linear in it, so the check is the one for a built record, without [`Brep::validate`]'s item
//! budget, which bounds a record parsed from outside before anything walks it: a caller holding a
//! record it has not checked (pasted data, a file's) runs [`Brep::validate`] on it first.

mod geometry;

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::document::Model;
use cadmpeg_ir::geometry::pcurve::{Pcurve, PcurveMetadata};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, DirectedParameterRange, SolvedCurveGeometry, SolvedSurfaceGeometry,
    Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::scalar::PositiveReal;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, EdgeCarrier, Face, FaceLoops, Loop, LoopBoundary, LoopRing,
    PcurveUse, Point, Region, Sense, Shell, Vertex,
};
use solidcore_record::brep::{Brep, Curve as BCurve, Loop as BLoop, Shell as BShell};
use solidcore_step::write::topology::{Map, as_form, form_map, layout, outer_loop};

use geometry::{Chart, Param};
use solidcore::eval::spline::EvalCurve;

/// Why a record was not written. Nothing of it is in the model then.
#[derive(Clone, Debug, PartialEq)]
pub enum WriteError {
    /// The record fails its structural check (an index out of range, a number not finite, an
    /// empty loop…), with the check's reason.
    Invalid(String),
    /// An id made from the prefix is not a CADIR entity identity, or the model has the body's
    /// already.
    Id(String),
    /// Something of the record CADIR has no exact form for here, with what.
    Unsupported(String),
    /// A value CADIR refuses (a degenerate frame, a radius out of its range…), with why.
    Refused(String),
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriteError::Invalid(m) => write!(f, "an invalid record: {m}"),
            WriteError::Id(m) => write!(f, "an identity: {m}"),
            WriteError::Unsupported(m) => write!(f, "not supported yet: {m}"),
            WriteError::Refused(m) => write!(f, "refused by CADIR: {m}"),
        }
    }
}

impl std::error::Error for WriteError {}

impl WriteError {
    /// The same reason, said of record item `what` (`face 3`, `edge 7`).
    fn of(self, what: &str) -> WriteError {
        match self {
            WriteError::Invalid(m) => WriteError::Invalid(format!("{what}: {m}")),
            WriteError::Id(m) => WriteError::Id(m),
            WriteError::Unsupported(m) => WriteError::Unsupported(format!("{what}: {m}")),
            WriteError::Refused(m) => WriteError::Refused(format!("{what}: {m}")),
        }
    }
}

type W<T> = Result<T, WriteError>;

/// An edge's CADIR curve, the interval written on it, and the parameter its pcurves are on
/// ([`Writer::carried`]).
type Carried = (SolvedCurveGeometry, Option<[f64; 2]>, Param);

/// What a record became in the model.
#[derive(Clone, Debug, PartialEq)]
pub struct Written {
    /// The body.
    pub body: BodyId,
    /// Its regions: one per material region of the record, in its order, then one per piece of
    /// its open shells (those that touch, sharing a vertex), in the order of their first shell.
    pub regions: Vec<RegionId>,
    /// Each record face's CADIR face, by the record's face index (`None` for a face no shell
    /// uses, which a valid record has none of).
    pub faces: Vec<Option<FaceId>>,
    /// Each record edge's CADIR edge, by the record's edge index (`None` for an edge nothing
    /// uses).
    pub edges: Vec<Option<EdgeId>>,
}

/// Append `record` to `model` as one body named `name`, its ids `<id_prefix>:<kind>#<key>`:
/// `id_prefix` is the format and scope of a CADIR identity (`"solidcraft:part-3"`), unique to
/// this body in the model: the caller's namespace, under which the model holds no id yet. Only
/// the body's own id is checked against the model; checking every list would walk the whole
/// model on each write, so writing n bodies would cost n² (a caller that cannot promise a fresh
/// prefix writes into an empty model and merges it itself). Returns what each record face and edge became, or why nothing was
/// written.
pub fn write_body(
    model: &mut Model,
    record: &Brep,
    id_prefix: &str,
    name: Option<&str>,
) -> W<Written> {
    record
        .validate_built()
        .map_err(|e| WriteError::Invalid(e.0.to_string()))?;
    let layout = layout(record).map_err(|m| WriteError::Unsupported(plain(m)))?;
    let mut w = Writer {
        b: record,
        prefix: id_prefix,
        out: Model::default(),
        vertices: vec![None; record.vertices.len()],
        edges: vec![None; record.edges.len()],
        faces: vec![None; record.faces.len()],
        coedges: HashMap::new(),
        wire_shells: HashMap::new(),
    };
    let body: BodyId = w.id("body", 0)?;
    if model.bodies.iter().any(|b| b.id == body) {
        return Err(WriteError::Id(format!("the model has body {body} already")));
    }
    let mut regions = Vec::new();
    let mut region = |w: &mut Writer<'_>, shells: &[u32]| -> W<()> {
        let id: RegionId = w.id("region", regions.len())?;
        let mut ids = Vec::new();
        for &s in shells {
            ids.push(w.shell(s, &id)?);
        }
        w.out.regions.push(Region {
            id: id.clone(),
            body: body.clone(),
            shells: ids,
        });
        regions.push(id);
        Ok(())
    };
    for solid in &layout.solids {
        let mut shells = vec![solid.outer];
        shells.extend(&solid.voids);
        region(&mut w, &shells)?;
    }
    for sheets in touching(record, &layout.sheets) {
        region(&mut w, &sheets)?;
    }
    let kind = match (layout.solids.is_empty(), layout.sheets.is_empty()) {
        (false, true) => BodyKind::Solid,
        (true, false) => BodyKind::Sheet,
        (false, false) => BodyKind::General,
        (true, true) => return Err(WriteError::Unsupported("a record without faces".into())),
    };
    w.radial()?;
    w.out.bodies.push(Body {
        id: body.clone(),
        kind,
        regions: regions.clone(),
        transform: None,
        name: name.map(str::to_string),
        color: None,
        visible: None,
    });
    let written = Written {
        body,
        regions,
        faces: w.faces.clone(),
        edges: w
            .edges
            .iter()
            .map(|e| e.as_ref().map(|e| e.id.clone()))
            .collect(),
    };
    let out = w.out;
    model.bodies.extend(out.bodies);
    model.regions.extend(out.regions);
    model.shells.extend(out.shells);
    model.faces.extend(out.faces);
    model.loops.extend(out.loops);
    model.coedges.extend(out.coedges);
    model.edges.extend(out.edges);
    model.vertices.extend(out.vertices);
    model.points.extend(out.points);
    model.surfaces.extend(out.surfaces);
    model.curves.extend(out.curves);
    model.pcurves.extend(out.pcurves);
    Ok(written)
}

/// The record's open shells `sheets` in groups of those that touch (share a vertex, through
/// their faces' edges and vertex loops or their wire edges), each group CADIR's region of its own
/// (a region is one connected piece of its body): in the order of each group's first shell, each
/// group's shells in their order in `sheets`.
fn touching(b: &Brep, sheets: &[u32]) -> Vec<Vec<u32>> {
    // Each sheet's group, as the first sheet of it (a union of sheets sharing a vertex).
    // Each sheet's link points at a sheet before it or itself, so a group's root is its first
    // sheet; each walk halves its path, so a long chain is not walked again and again.
    let mut group: Vec<usize> = (0..sheets.len()).collect();
    let root = |group: &mut [usize], mut k: usize| loop {
        let Some(&g) = group.get(k) else { return k };
        if g == k {
            return k;
        }
        let up = group.get(g).copied().unwrap_or(g);
        if let Some(slot) = group.get_mut(k) {
            *slot = up;
        }
        k = up;
    };
    let mut owner: HashMap<u32, usize> = HashMap::new();
    for (k, &s) in sheets.iter().enumerate() {
        let Some(BShell::Faces { faces, wires }) = b.shells.get(s as usize) else {
            continue;
        };
        let mut vertices = Vec::new();
        let mut edges: Vec<u32> = wires.clone();
        for of in faces {
            for l in b
                .faces
                .get(of.face as usize)
                .into_iter()
                .flat_map(|f| &f.loops)
            {
                match l {
                    BLoop::Edges { edges: uses } => edges.extend(uses.iter().map(|u| u.edge)),
                    BLoop::Vertex { vertex } => vertices.push(*vertex),
                }
            }
        }
        vertices.extend(
            edges
                .iter()
                .filter_map(|e| b.edges.get(*e as usize))
                .flat_map(|e| e.vertices),
        );
        for v in vertices {
            match owner.get(&v) {
                Some(&o) => {
                    let (a, c) = (root(&mut group, o), root(&mut group, k));
                    // The later group joins the earlier one.
                    let (first, later) = (a.min(c), a.max(c));
                    if let Some(g) = group.get_mut(later) {
                        *g = first;
                    }
                }
                None => {
                    owner.insert(v, k);
                }
            }
        }
    }
    // Each group's place in the output, by its root, so a group is found without scanning.
    let mut out: Vec<Vec<u32>> = Vec::new();
    let mut place: HashMap<usize, usize> = HashMap::new();
    for (k, &s) in sheets.iter().enumerate() {
        let r = root(&mut group, k);
        let at = *place.entry(r).or_insert_with(|| {
            out.push(Vec::new());
            out.len() - 1
        });
        if let Some(shells) = out.get_mut(at) {
            shells.push(s);
        }
    }
    out
}

/// A refusal of the STEP writer's helpers, said of CADIR.
fn plain(m: String) -> String {
    m.replace("not supported yet: STEP export of ", "")
}

/// A record edge as written: its id, and the parameter its uses' pcurves are written on (CADIR's
/// of its curve, or its form's) with the record's interval there.
#[derive(Clone, Debug)]
struct WrittenEdge {
    id: EdgeId,
    param: Param,
    interval: [f64; 2],
}

struct Writer<'b> {
    b: &'b Brep,
    prefix: &'b str,
    /// What is written, moved into the caller's model once all of it is.
    out: Model,
    vertices: Vec<Option<VertexId>>,
    edges: Vec<Option<WrittenEdge>>,
    faces: Vec<Option<FaceId>>,
    /// Each oriented edge's coedge, by (face, loop, position) in the record, with its edge.
    coedges: HashMap<(u32, u32, u32), (CoedgeId, u32)>,
    /// Each wire edge's shell, so a wire edge two shells list is refused.
    wire_shells: HashMap<u32, u32>,
}

impl Writer<'_> {
    fn id<T: TryFrom<String>>(&self, kind: &str, key: impl std::fmt::Display) -> W<T> {
        let s = format!("{}:{kind}#{key}", self.prefix);
        T::try_from(s.clone()).map_err(|_| WriteError::Id(format!("{s} is not a CADIR identity")))
    }

    fn vertex(&mut self, v: u32) -> W<VertexId> {
        if let Some(Some(id)) = self.vertices.get(v as usize) {
            return Ok(id.clone());
        }
        let vertex = self
            .b
            .vertices
            .get(v as usize)
            .ok_or_else(|| WriteError::Invalid(format!("vertex {v}: no such vertex")))?;
        let (id, point): (VertexId, PointId) = (self.id("vertex", v)?, self.id("point", v)?);
        let at = geometry::finite(vertex.point).map_err(|e| e.of(&format!("vertex {v}")))?;
        self.out.points.push(Point::new(point.clone(), at, None));
        self.out.vertices.push(Vertex {
            id: id.clone(),
            point,
            tolerance: PositiveReal::new(vertex.tolerance),
        });
        if let Some(slot) = self.vertices.get_mut(v as usize) {
            *slot = Some(id.clone());
        }
        Ok(id)
    }

    fn edge(&mut self, e: u32) -> W<WrittenEdge> {
        if let Some(Some(w)) = self.edges.get(e as usize) {
            return Ok(w.clone());
        }
        let b = self.b;
        let edge = b
            .edges
            .get(e as usize)
            .ok_or_else(|| WriteError::Invalid(format!("edge {e}: no such edge")))?;
        let (start, end) = (
            self.vertex(edge.vertices[0])?,
            self.vertex(edge.vertices[1])?,
        );
        let what = format!("edge {e}");
        // The named curve, on the edge's interval when the edge's parameter is its own, and on
        // the one its vertices give on it when the edge is held on a NURBS form of it; a traced
        // intersection, or a polyline held so, as its form.
        let named = matches!(
            edge.curve,
            BCurve::Line { .. }
                | BCurve::Circle { .. }
                | BCurve::Ellipse { .. }
                | BCurve::Spline { .. }
                | BCurve::RationalSpline { .. }
        );
        // The interval on the named curve, where its vertices are, the curve running as the
        // form does; a circle or ellipse whose form runs the other way about its frame (or
        // whose arc is not worked out) is written as its form.
        let at = |v: u32| b.vertices.get(v as usize).map(|x| x.point);
        let ends = at(edge.vertices[0]).zip(at(edge.vertices[1]));
        let on_named = match &edge.nurbs {
            Some(_) if named => ends.and_then(|(p, q)| named_interval(edge, p, q)),
            _ => None,
        };
        // A line named the other way from its edge is written turned, from the start vertex to
        // the end: a coedge's sense is taken against its edge's curve, which then runs as the
        // edge does, on the interval its vertices give.
        let turned = match (&edge.curve, &edge.nurbs, on_named, ends) {
            (BCurve::Line { start, end }, Some(_), None, Some((p, q))) => {
                line_interval(*end, *start, p, q).map(|i| {
                    (
                        BCurve::Line {
                            start: *end,
                            end: *start,
                        },
                        i,
                    )
                })
            }
            _ => None,
        };
        let (named_curve, on_named) = match &turned {
            Some((line, i)) => (line, Some(*i)),
            None => (&edge.curve, on_named),
        };
        let conic = matches!(edge.curve, BCurve::Circle { .. } | BCurve::Ellipse { .. });
        let (carrier, on_form) = match &edge.nurbs {
            Some(form) if !named || (conic && on_named.is_none()) => (form, false),
            Some(_) => (named_curve, true),
            None => (&edge.curve, false),
        };
        let (g, range, param) = match self.carried(edge, carrier, on_form, on_named, &what)? {
            Some(written) => written,
            // An arc that cannot start within its first turn without moving its ends past the
            // tolerance is written as its form, which has no turns to take.
            None => match &edge.nurbs {
                Some(form) => self
                    .carried(edge, form, false, None, &what)?
                    .ok_or_else(|| WriteError::Invalid(format!("{what}: its form")))?,
                None => {
                    return Err(WriteError::Unsupported(format!(
                        "{what}: an arc on a conic too large to start it within its first turn \
                         without moving its ends past the tolerance, and without a NURBS form"
                    )));
                }
            },
        };
        let interval = edge.interval.map(|t| param.apply(t));
        let curve: CurveId = self.id("curve", e)?;
        self.out.curves.push(Curve {
            id: curve.clone(),
            geometry: CurveGeometry::Solved(g),
            parameter_range: None,
            source_object: None,
        });
        let carrier = EdgeCarrier::new(Some(curve), range)
            .map_err(|m| WriteError::Refused(format!("{what}: {m}")))?;
        let id: EdgeId = self.id("edge", e)?;
        self.out.edges.push(Edge {
            id: id.clone(),
            carrier,
            start,
            end,
            tolerance: PositiveReal::new(edge.tolerance),
        });
        let written = WrittenEdge {
            id,
            param,
            interval,
        };
        if let Some(slot) = self.edges.get_mut(e as usize) {
            *slot = Some(written.clone());
        }
        Ok(written)
    }

    /// Edge `edge` on `carrier` (its named curve, or a NURBS form): CADIR's curve, the interval
    /// written on it (`on_named` where `on_form`: the carrier is the named curve of an edge held
    /// on its form, whose pcurves stay on the form's parameter), and the parameter the pcurves are
    /// written on. An arc of a circle or ellipse starts within its first turn, as CADIR has it:
    /// its parameter is moved by whole turns. `None` where that moves its ends by more than the
    /// tolerance, or leaves it no interval: the turns added are `fl(2π)` and its sum with the
    /// angle, rounded (a few ulps of the turn each), and an angle within rounding below a whole
    /// turn starts at the turn, not past it (`turns_back`); on a conic large enough, either is
    /// past the tolerance, or the arc's two ends round to one angle.
    fn carried(
        &self,
        edge: &solidcore_record::brep::Edge,
        carrier: &BCurve,
        on_form: bool,
        on_named: Option<[f64; 2]>,
        what: &str,
    ) -> W<Option<Carried>> {
        use std::f64::consts::TAU;
        let (g, param) = geometry::curve(carrier)
            .map_err(|m| m.of(what))?
            .ok_or_else(|| {
                WriteError::Unsupported(format!(
                    "{what}: a traced intersection curve without its NURBS form"
                ))
            })?;
        let size = match carrier {
            BCurve::Circle { radius, .. } => radius.abs(),
            BCurve::Ellipse { radii, .. } => radii[0].abs().max(radii[1].abs()),
            _ => 0.0,
        };
        let periodic = matches!(
            g,
            SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_)
        );
        // The turns that start the arc at `t0` within its first turn.
        let turns = |p: Param, t0: f64| match periodic {
            true => turns_back(p.apply(t0)),
            false => 0.0,
        };
        let (range, param, turned) = if on_form {
            // The interval on the named curve (none for a line where that is not worked out:
            // the reader then finds it so); the pcurves stay on the form's parameter, which the
            // record gives them on.
            let turned = on_named.map_or(0.0, |i| turns(param, i[0]));
            let p = Param {
                shift: param.shift + turned,
                ..param
            };
            (
                on_named.map(|i| i.map(|t| p.apply(t))),
                Param::IDENTITY,
                turned,
            )
        } else {
            let turned = turns(param, edge.interval[0]);
            let p = Param {
                shift: param.shift + turned,
                ..param
            };
            (Some(edge.interval.map(|t| p.apply(t))), p, turned)
        };
        if turned != 0.0
            && let Some([s0, s1]) = range
        {
            // How far the turns move the arc's ends, in angle: how far `turned` is from whole
            // turns (none but where `turns_back` starts the arc at the turn), and the rounding of
            // `fl(2π)` and of adding it, a few ulps of the sum.
            let whole = (turned / TAU).round() * TAU;
            let off = (turned - whole).abs()
                + 4.0 * f64::EPSILON * (turned.abs() + s0.abs().max(s1.abs()));
            let tolerance = edge.tolerance.max(self.b.tolerance);
            if !(s0 < s1) || !(off * size <= tolerance) {
                return Ok(None);
            }
        }
        Ok(Some((g, range, param)))
    }

    /// Face `f`'s carrier, whether the record's natural normal is CADIR's, and the chart its
    /// pcurves go through when they are written.
    fn carrier(&self, f: u32) -> W<(SolvedSurfaceGeometry, bool, Option<Chart>)> {
        let b = self.b;
        let face = b
            .faces
            .get(f as usize)
            .ok_or_else(|| WriteError::Invalid(format!("face {f}: no such face")))?;
        let as_nurbs = || -> W<(SolvedSurfaceGeometry, bool, Option<Chart>)> {
            let form = face.parameters();
            let (c, chart) = geometry::surface(form)?.ok_or_else(|| {
                WriteError::Unsupported("a NURBS form CADIR has no form for".into())
            })?;
            Ok((c, chart.keeps_normal(), Some(chart)))
        };
        match &face.nurbs {
            Some(_) if as_form(face, b.tolerance) => as_nurbs(),
            Some(form) => {
                // The named surface, its side from its form's normal; the form itself where the
                // named surface has no CADIR form, or none whose normal the form's is compared
                // with.
                match geometry::surface(&face.surface) {
                    Ok(Some((c, chart))) => {
                        match form_map(
                            &face.surface,
                            form,
                            face.domain,
                            Map {
                                m: chart.m,
                                t: [0.0, 0.0],
                            },
                        ) {
                            Some(map) => Ok((c, map.keeps_normal(), None)),
                            None => as_nurbs(),
                        }
                    }
                    Ok(None) | Err(WriteError::Unsupported(_)) => as_nurbs(),
                    Err(e) => Err(e),
                }
            }
            None => {
                let (c, chart) = geometry::surface(&face.surface)?.ok_or_else(|| {
                    WriteError::Unsupported(
                        "a ball patch, blend or revolution without its NURBS form".into(),
                    )
                })?;
                Ok((c, chart.keeps_normal(), Some(chart)))
            }
        }
    }

    /// Face `f` as shell `shell` uses it, `side` true where its normal there is its natural one.
    fn face(&mut self, f: u32, side: bool, shell: &ShellId) -> W<FaceId> {
        let what = format!("face {f}");
        if self.faces.get(f as usize).is_some_and(Option::is_some) {
            return Err(WriteError::Unsupported(format!(
                "{what}: a face two shells use"
            )));
        }
        let b = self.b;
        let face = b
            .faces
            .get(f as usize)
            .ok_or_else(|| WriteError::Invalid(format!("{what}: no such face")))?;
        let (carrier, keeps, chart) = self.carrier(f).map_err(|e| e.of(&what))?;
        let surface: SurfaceId = self.id("surface", f)?;
        self.out.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Solved(carrier),
            source_object: None,
        });
        let id: FaceId = self.id("face", f)?;
        let mut loops = Vec::new();
        for (li, l) in face.loops.iter().enumerate() {
            let loop_id: LoopId = self.id("loop", format_args!("{f}.{li}"))?;
            let boundary = match l {
                BLoop::Edges { edges } => {
                    // Counter-clockwise about the side's normal: the record's order on the
                    // natural side, reversed on the other.
                    let mut order: Vec<usize> = (0..edges.len()).collect();
                    if !side {
                        order.reverse();
                    }
                    let mut ring = Vec::new();
                    for k in order {
                        let u = edges.get(k).ok_or_else(|| {
                            WriteError::Invalid(format!("{what}: loop {li} has no use {k}"))
                        })?;
                        let edge = self.edge(u.edge)?;
                        let along = u.forward == side;
                        let coedge: CoedgeId = self.id("coedge", format_args!("{f}.{li}.{k}"))?;
                        let mut pcurves = Vec::new();
                        if let Some(chart) = chart {
                            let g = geometry::pcurve(&u.pcurve, chart, edge.param)
                                .map_err(|e| e.of(&format!("{what}: loop {li}, use {k}")))?;
                            let pcurve: PcurveId =
                                self.id("pcurve", format_args!("{f}.{li}.{k}"))?;
                            self.out.pcurves.push(Pcurve {
                                id: pcurve.clone(),
                                geometry: g,
                                metadata: PcurveMetadata::default(),
                            });
                            // Its range is its own interval, on the edge's parameter: within
                            // a remap's rounding of the edge's (the record's check), and inside
                            // the pcurve's knots.
                            let [t0, t1] = u
                                .pcurve
                                .interval()
                                .map_or(edge.interval, |i| i.map(|t| edge.param.apply(t)));
                            pcurves.push(PcurveUse {
                                pcurve,
                                isoparametric: None,
                                parameter_range: DirectedParameterRange::new(if along {
                                    [t0, t1]
                                } else {
                                    [t1, t0]
                                })
                                .ok(),
                            });
                        }
                        self.out.coedges.push(Coedge {
                            id: coedge.clone(),
                            owner_loop: loop_id.clone(),
                            edge: edge.id.clone(),
                            radial_next: coedge.clone(),
                            sense: if along {
                                Sense::Forward
                            } else {
                                Sense::Reversed
                            },
                            pcurves,
                            use_curve: None,
                        });
                        let key = (f, index(li)?, index(k)?);
                        self.coedges.insert(key, (coedge.clone(), u.edge));
                        ring.push(coedge);
                    }
                    LoopBoundary::Ring(
                        LoopRing::new(ring, Vec::new())
                            .map_err(|m| WriteError::Invalid(format!("{what}: loop {li}: {m}")))?,
                    )
                }
                BLoop::Vertex { vertex } => LoopBoundary::Vertex {
                    vertex: self.vertex(*vertex)?,
                    pcurves: Vec::new(),
                },
            };
            self.out.loops.push(Loop {
                id: loop_id.clone(),
                face: id.clone(),
                boundary,
            });
            loops.push(loop_id);
        }
        let loops = match outer_loop(b, face) {
            Some(o) => match loops.get(o).cloned() {
                Some(outer) => {
                    let inner = loops
                        .into_iter()
                        .enumerate()
                        .filter_map(|(k, l)| (k != o).then_some(l))
                        .collect();
                    FaceLoops::classified(outer, inner)
                }
                None => FaceLoops::unspecified(loops),
            },
            None => FaceLoops::unspecified(loops),
        };
        self.out.faces.push(Face {
            id: id.clone(),
            shell: shell.clone(),
            surface,
            sense: if side == keeps {
                Sense::Forward
            } else {
                Sense::Reversed
            },
            loops,
            name: None,
            color: None,
            tolerance: certified(face),
        });
        if let Some(slot) = self.faces.get_mut(f as usize) {
            *slot = Some(id.clone());
        }
        Ok(id)
    }

    /// Record shell `s` in region `region`: its faces as it uses them, and its wire edges.
    fn shell(&mut self, s: u32, region: &RegionId) -> W<ShellId> {
        let b = self.b;
        let Some(BShell::Faces { faces, wires }) = b.shells.get(s as usize) else {
            return Err(WriteError::Unsupported(format!(
                "shell {s}: a wire or vertex shell"
            )));
        };
        let id: ShellId = self.id("shell", s)?;
        let mut face_ids = Vec::new();
        for of in faces {
            face_ids.push(self.face(of.face, of.forward, &id)?);
        }
        let mut wire_ids = Vec::new();
        for &e in wires {
            // CADIR's wire edge belongs to one shell.
            if let Some(&other) = self.wire_shells.get(&e).filter(|o| **o != s) {
                return Err(WriteError::Invalid(format!(
                    "shell {s}: wire edge {e} is shell {other}'s too, and a wire edge is one shell's"
                )));
            }
            self.wire_shells.insert(e, s);
            wire_ids.push(self.edge(e)?.id);
        }
        let shell = Shell::new(id.clone(), region.clone(), face_ids, wire_ids, Vec::new())
            .map_err(|m| WriteError::Invalid(format!("shell {s}: {m}")))?;
        self.out.shells.push(shell);
        Ok(id)
    }

    /// Each edge's coedges in a ring about it (`radial_next`), in the record's radial order of
    /// its uses.
    fn radial(&mut self) -> W<()> {
        let mut around: HashMap<u32, Vec<CoedgeId>> = HashMap::new();
        // Each ring's members, so a use is looked up, not searched for in its ring.
        let mut placed: HashSet<(u32, CoedgeId)> = HashSet::new();
        for (e, edge) in self.b.edges.iter().enumerate() {
            let e = index(e)?;
            for u in &edge.uses {
                if let Some((c, _)) = self.coedges.get(&(u.face, u.r#loop, u.position)) {
                    around.entry(e).or_default().push(c.clone());
                    placed.insert((e, c.clone()));
                }
            }
        }
        // A use the edge does not list (which a valid record has none of) joins its ring last.
        let mut keys: Vec<_> = self.coedges.keys().copied().collect();
        keys.sort_unstable();
        for key in keys {
            if let Some((c, e)) = self.coedges.get(&key)
                && placed.insert((*e, c.clone()))
            {
                around.entry(*e).or_default().push(c.clone());
            }
        }
        let mut next: HashMap<CoedgeId, CoedgeId> = HashMap::new();
        for ring in around.values() {
            // Each coedge's next is the one after it, the last's the first.
            for (c, n) in ring.iter().zip(ring.iter().cycle().skip(1)) {
                next.insert(c.clone(), n.clone());
            }
        }
        for c in &mut self.out.coedges {
            if let Some(n) = next.get(&c.id) {
                c.radial_next = n.clone();
            }
        }
        Ok(())
    }
}

/// The deviation a face written as its NURBS form carries: a procedural blend's net is within
/// its definition's certified bound ε of the definition, which CADIR cannot hold, so ε is the
/// face's tolerance (CADIR's `Face::tolerance`, the most its geometry is off the face), and so is a
/// screw sweep's bound. An exact round's net is its definition, and every other face is written
/// exactly: `None`.
fn certified(face: &solidcore_record::brep::Face) -> Option<PositiveReal> {
    match &face.surface {
        solidcore_record::brep::Surface::Blend { definition } => match definition.as_ref() {
            solidcore_record::brep::blend::BlendDefinition::Procedural(d) => {
                PositiveReal::new(d.bound)
            }
            solidcore_record::brep::blend::BlendDefinition::ExactRound(_) => None,
        },
        // A screw sweep's net is within its bound of the wall.
        solidcore_record::brep::Surface::Sweep { definition } => {
            PositiveReal::new(definition.bound)
        }
        _ => None,
    }
}

/// Record edge `edge`'s interval on its named curve (a line, circle or ellipse its NURBS form
/// stands for), from its start vertex at `p` to its end at `q`, the curve running from one to the
/// other as its form does; `None` for another curve, ends that give no interval, and a circle or
/// ellipse whose form runs clockwise about its frame (CADIR's arc runs counter-clockwise).
///
/// A conic's sweep is the form's own, followed from `p` to `q` ([`sweep_along`]): a short arc
/// stays short and an arc keeps the side its form is on, however unevenly its form is
/// parameterized; an arc whose turning cannot be followed is `None` (written as its form). A
/// closed edge (one vertex at both ends) is a whole turn. A form that winds past one and a half
/// turns is `None` too: a range of the vertices' angles would drop its whole turns.
fn named_interval(
    edge: &solidcore_record::brep::Edge,
    p: [f64; 3],
    q: [f64; 3],
) -> Option<[f64; 2]> {
    use std::f64::consts::{PI, TAU};
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let (frame, radii) = match &edge.curve {
        BCurve::Line { start, end } => return line_interval(*start, *end, p, q),
        BCurve::Circle { frame, radius } => (frame, [*radius, *radius]),
        BCurve::Ellipse { frame, radii } => (frame, *radii),
        _ => return None,
    };
    let form = edge.nurbs.as_ref()?;
    let o = frame.origin;
    let norm = |a: [f64; 3]| dot(a, a).sqrt();
    // A homogeneous point's image on the conic's plane, scaled so the conic is the unit circle
    // (times the point's weight): its angle is the point's angle about the centre.
    let plane = |[x, y, z, w]: [f64; 4]| {
        let r = [x - w * o[0], y - w * o[1], z - w * o[2]];
        [
            dot(r, frame.x_axis) / radii[0],
            dot(r, frame.y_axis) / radii[1],
        ]
    };
    let angle = |[x, y, z]: [f64; 3]| {
        let [u, v] = plane([x, y, z, 1.0]);
        v.atan2(u)
    };
    // How far `plane`'s image of `h` can be from its image of the point `h` stands for, where
    // each of `h`'s coordinates is within `e` of that point's (x, y and z within the most of
    // theirs, w within its own): `r` moves at most `√3 e_xyz + e_w |o|`, each image coordinate
    // that times its axis over its radius; `plane`'s own rounding (a few ulps of its terms) is
    // the second sum, taken at 8 ulps.
    let scale = (norm(frame.x_axis) / radii[0]).hypot(norm(frame.y_axis) / radii[1]);
    let reach = |[x, y, z, w]: [f64; 4], e: [f64; 4]| {
        let e_xyz = e[0].max(e[1]).max(e[2]);
        let moved = 3f64.sqrt() * e_xyz + e[3] * norm(o);
        let rounding = 8.0 * f64::EPSILON * (norm([x, y, z]) + w.abs() * norm(o));
        scale * (moved + rounding)
    };
    let (a0, aq) = (angle(p), angle(q));
    let sweep = sweep_along(form, edge.interval, a0, aq, plane, reach)?;
    // A form that winds more than about a turn has no named range that keeps its turns (the end
    // is its vertex's angle, whole turns added by rounding): it is written as its form.
    if sweep > 3.0 * PI {
        return None;
    }
    let a1 = match edge.vertices[0] == edge.vertices[1] {
        true if sweep > PI => a0 + TAU,
        true => return None,
        // The end is the end vertex's own angle, the whole turns the sweep makes added: summing
        // the steps' turns would move a tiny arc's end on a huge circle off its vertex.
        false => aq + TAU * ((a0 + sweep.min(TAU) - aq) / TAU).round(),
    };
    (a0.is_finite() && a1.is_finite() && a1 > a0).then_some([a0, a1])
}

/// The interval on the line from `start` (0) to `end` (1) from where `p` lies on it to where `q`
/// does; `None` where `q` is not past `p` along it (or either is not finite).
fn line_interval(start: [f64; 3], end: [f64; 3], p: [f64; 3], q: [f64; 3]) -> Option<[f64; 2]> {
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let d = sub(end, start);
    let t = |x: [f64; 3]| dot(sub(x, start), d) / dot(d, d);
    let (t0, t1) = (t(p), t(q));
    (t0.is_finite() && t1.is_finite() && t1 > t0).then_some([t0, t1])
}

/// The signed turn (radians, counter-clockwise positive) of `form` over `[t0, t1]`, from angle
/// `a0` at its start to `a1` at its end. `plane(h)` is homogeneous point `h`'s image on the
/// conic's plane about its centre (linear in `h`), whose angle (`atan2`) is the point's angle
/// about the centre where the weight is positive; `reach(h, e)` bounds how far that image, as
/// computed, is from the image of the point `h` stands for when each of `h`'s coordinates is
/// within `e` of it.
///
/// The interval is cut at the form's knots and into [`SAMPLES`] equal steps, and a step is
/// halved until it is proven to turn at most [`MAX_STEP`]: the control points of the form's
/// piece over the step (its Bézier points, positive weights) all lie within [`MAX_STEP`] of the
/// step's start angle. The piece lies in its control points' convex hull, so inside that wedge
/// (narrower than half a turn), and its turn is the short way between its ends, however the
/// form is parameterized. Samples alone prove nothing: a form can turn most of the way round
/// between any two of them.
///
/// The control points are the kernel's, each with its rounding bounded
/// (`solidcore::eval::spline::EvalCurve::bezier`): each is checked with its weight more than its
/// error, and its image's error at most [`ROUNDING_SHARE`] of its distance from the centre, so the point it stands for is within `asin(1/4)` of the
/// angle measured, and the wedge, `MAX_STEP + asin(1/4)` either side, still narrower than half a
/// turn. A step whose pieces cannot be so certified is halved like any other, and the form is
/// written as itself where none can. The step's own turn (from its ends' computed angles) needs
/// no bound: the steps' ends are shared, so their errors cancel but for the first and last,
/// which are the vertices' own angles.
///
/// `None` when that takes more than [`BUDGET`] pieces, a step between neighbouring parameters
/// is not proven, a weight is not positive, or a point is not on the form.
fn sweep_along(
    form: &BCurve,
    [t0, t1]: [f64; 2],
    a0: f64,
    a1: f64,
    plane: impl Fn([f64; 4]) -> [f64; 2],
    reach: impl Fn([f64; 4], [f64; 4]) -> f64,
) -> Option<f64> {
    use std::f64::consts::{PI, TAU};
    // The short way from one angle to another; a difference already that short is kept as it
    // is (adding and taking away half a turn would round a tiny one).
    let turn = |from: f64, to: f64| match to - from {
        d if d.abs() <= PI => d,
        d => (d + PI).rem_euclid(TAU) - PI,
    };
    if !(t0 < t1) || !a0.is_finite() || !a1.is_finite() {
        return None;
    }
    let (curve, [lo, hi]) = spline_of(form)?;
    let knots = curve.knots();
    // The cuts: the interval's ends, its equal steps and the knots inside it, in order.
    let mut cuts: Vec<f64> = (0..=SAMPLES)
        .map(|k| t0 + (t1 - t0) * (k as f64 / SAMPLES as f64))
        .chain(knots.iter().copied().filter(|&k| t0 < k && k < t1))
        .collect();
    if cuts.len() > BUDGET {
        return None;
    }
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    let last = cuts.len().checked_sub(1)?;
    let at = |t: f64| {
        // Within the form's knots (`lo < hi`, both finite: max/min, not clamp, cannot panic).
        curve.eval(t.max(lo).min(hi)).and_then(|(p, _)| {
            let [u, v] = plane([p.x, p.y, p.z, 1.0]);
            let a = v.atan2(u);
            a.is_finite().then_some(a)
        })
    };
    let mut steps = Vec::new();
    for (k, pair) in cuts.windows(2).enumerate() {
        let [ta, tb] = *<&[f64; 2]>::try_from(pair).ok()?;
        let aa = if k == 0 { a0 } else { at(ta)? };
        let ab = if k + 1 == last { a1 } else { at(tb)? };
        steps.push((ta, aa, tb, ab));
    }
    let (mut sweep, mut pieces) = (0.0, cuts.len());
    while let Some((ta, aa, tb, ab)) = steps.pop() {
        if pieces >= BUDGET {
            return None;
        }
        pieces += 1;
        let whole = turn(aa, ab);
        let within = |h: [f64; 4], e: [f64; 4]| {
            let [u, v] = plane(h);
            h[3] > e[3]
                && reach(h, e) <= ROUNDING_SHARE * u.hypot(v)
                && turn(aa, v.atan2(u)).abs() <= MAX_STEP
        };
        if whole.abs() <= MAX_STEP && curve.bezier(ta, tb)?.into_iter().all(|(h, e)| within(h, e)) {
            sweep += whole;
            continue;
        }
        let tm = 0.5 * (ta + tb);
        if !(ta < tm && tm < tb) {
            // Neighbouring parameters whose piece is not proven: no step is shorter.
            return None;
        }
        let am = at(tm)?;
        steps.push((ta, aa, tm, am));
        steps.push((tm, am, tb, ab));
    }
    sweep.is_finite().then_some(sweep)
}

/// The steps [`sweep_along`] starts a form with, ends included, before halving any.
const SAMPLES: usize = 16;

/// The most a step of [`sweep_along`] may turn, and its piece's control points stray from its
/// start angle: well short of half a turn, so the wedge they lie in is convex and the step's turn
/// is the short way between its ends.
const MAX_STEP: f64 = std::f64::consts::FRAC_PI_4;

/// The most a control point's computed image may be off, as a share of its distance from the
/// centre, for [`sweep_along`] to take its angle: the point it stands for is then within
/// `asin(1/4)` (under 0.26 rad) of the angle measured.
const ROUNDING_SHARE: f64 = 0.25;

/// The most pieces [`sweep_along`] takes of one form before it gives up on it.
const BUDGET: usize = 4096;

/// NURBS curve `c` as the kernel evaluates it (`solidcore::eval::spline::EvalCurve`), with its
/// knot interval; `None` for another curve or knots that do not fit its points.
fn spline_of(c: &BCurve) -> Option<(EvalCurve, [f64; 2])> {
    let (knots, ctrl): (&[f64], Vec<[f64; 4]>) = match c {
        BCurve::Spline { knots, points } => (
            knots,
            points.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect(),
        ),
        BCurve::RationalSpline { knots, homogeneous } => (knots, homogeneous.clone()),
        _ => return None,
    };
    let degree = knots.len().checked_sub(ctrl.len().checked_add(1)?)?;
    let curve = EvalCurve::new(degree, knots.to_vec(), ctrl);
    let (lo, hi) = curve.interval()?;
    (degree < curve.ctrl().len() && lo < hi).then_some((curve, [lo, hi]))
}

/// The whole turns that take angle `t` into `[0, 2π)`, CADIR's canonical start of an arc's
/// interval on a circle or ellipse.
fn turns_back(t: f64) -> f64 {
    use std::f64::consts::TAU;
    let s = -(t / TAU).floor() * TAU;
    // An angle within rounding of a whole turn rounds to the next one: it starts at 0.
    match t + s {
        start if !s.is_finite() || !start.is_finite() => 0.0,
        start if (0.0..TAU).contains(&start) => s,
        _ => -t,
    }
}

fn index(i: usize) -> W<u32> {
    u32::try_from(i).map_err(|_| WriteError::Invalid("too many items".into()))
}

#[cfg(test)]
mod tests;
