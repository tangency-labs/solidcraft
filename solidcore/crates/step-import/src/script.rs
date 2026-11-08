//! Constructor scripts: a sequence of `SmBrepConstructor` calls over a table of
//! curves and surfaces, as one flat `f64` array, so that the Rust port and the C++
//! reference run exactly the same calls on the same numbers (difftest unit
//! `topology.brep_constructor` and `difftest --step-corpus --constructor`).
//!
//! [`from_body`] turns an imported body into the script an importer would run: per
//! shell its region and shell, per face its surface and loops, every oriented edge
//! joined to the edge it shares with an earlier face, and edge limits from the vertex
//! points.
//!
//! Layout (version 1; integers and flags stored as `f64`, references `-1` for none):
//!
//! ```text
//! script  : 31 1 nc ns no  curve*nc  surface*ns  op*no
//! curve   : dim bounded lo hi <flat curve>         dim 2 (UV) or 3; 2D: line or B-spline
//! surface : <flat surface>
//! op      : 0                                      StartBrep
//!         | 1                                      EndBrep
//!         | 2 solid outer_region                   StartRegion (-1 none, -2 infinite, i)
//!         | 3                                      EndRegion
//!         | 4                                      StartShell
//!         | 5 stitch                               EndShell
//!         | 6 orient                               StartFace
//!         | 7                                      EndFace
//!         | 8 surface natural u0 v0 u1 v1          SetFaceSurface (natural: its natural domain)
//!         | 9 orient                               StartLoop
//!         | 10                                     EndLoop
//!         | 11 x y z                               StartAndEndSingleVertexLoop
//!         | 12 x y z                               StartVertexOfLoop
//!         | 13 orient edge start end               StartEdge (edge / vertex results, -1 none)
//!         | 14 curve uv                            SetEdgeCurve (-1 a null curve)
//!         | 15 node node                           SetLimits
//!         | 16                                     EndEdge
//!         | 17 x y z                               StartVertex
//!         | 18                                     EndVertex
//!         | 19                                     SwapRegions
//!         | 20 surface u0 v0 u1 v1                  surface->AdjustSTEPUVDomain (before SetFaceSurface takes it)
//! node    : kind a b c                             0 point (a,b,c), 1 parameter a, 2 UV point (a,b), 3 SmVertexNode()
//! orient  : 0 unknown, 1 same, 2 opposite
//! ```
//!
//! A region reference is the index of an earlier `StartRegion` result, an edge
//! reference of an earlier `StartEdge` result, a vertex reference of an earlier
//! `StartVertexOfLoop` / `StartVertex` result. Each curve and surface is used at most
//! once (the call takes it over).

use crate::flat::{decode_curve, decode_surface, encode_curve, encode_surface};
use crate::geom::{bspline_point, dot, scale, sub};
use crate::model::{Body, BodyKind, Bound, Curve, Surface, P3};

/// First word of an encoded script.
pub const MAGIC: f64 = 31.0;
/// Layout version.
pub const VERSION: f64 = 1.0;

/// An orientation argument (`SmOrientType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orient {
    /// `SM_OT_UNKNOWN`.
    Unknown,
    /// `SM_OT_SAME`.
    Same,
    /// `SM_OT_OPPOSITE`.
    Opposite,
}

impl Orient {
    fn code(self) -> f64 {
        match self {
            Orient::Unknown => 0.0,
            Orient::Same => 1.0,
            Orient::Opposite => 2.0,
        }
    }

    /// `Same` when `same`, else `Opposite`.
    pub fn from_same(same: bool) -> Self {
        if same {
            Orient::Same
        } else {
            Orient::Opposite
        }
    }
}

/// A curve of the table: its dimension (3, or 2 for a UV curve), the bounds it is
/// made over (`None`: unbounded / untrimmed), and its geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptCurve {
    /// 2 or 3.
    pub dim: u32,
    /// `Some((lo, hi))`: a line over `[lo, hi]`, a circle / ellipse over those angles,
    /// a B-spline trimmed to them when they are a strict sub-interval of its knots.
    pub bounds: Option<(f64, f64)>,
    /// The geometry (points of a 2D curve use `x`, `y`; `z` is ignored).
    pub curve: Curve,
}

/// A bound of an edge (`SmVertexNode`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Node {
    /// A 3D point.
    Point([f64; 3]),
    /// A curve parameter.
    Parameter(f64),
    /// A UV point.
    Uv([f64; 2]),
    /// `SmVertexNode()` (undefined type, read as a point at the origin).
    Empty,
}

/// A region argument of `StartRegion`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionRef {
    /// `NULL`.
    Null,
    /// The B-rep's infinite region.
    Infinite,
    /// The result of an earlier `StartRegion`.
    Index(usize),
}

/// One constructor call.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// `StartBrep`.
    StartBrep,
    /// `EndBrep`.
    EndBrep,
    /// `StartRegion(solid, outer)`.
    StartRegion {
        /// `bIsSolidRegion` (not used upstream).
        solid: bool,
        /// `pOuterRegion`.
        outer: RegionRef,
    },
    /// `EndRegion`.
    EndRegion,
    /// `StartShell`.
    StartShell,
    /// `EndShell(stitch)`.
    EndShell {
        /// `bStitchShell`.
        stitch: bool,
    },
    /// `StartFace(orient)`.
    StartFace(Orient),
    /// `EndFace`.
    EndFace,
    /// `SetFaceSurface(surface, domain)`.
    SetFaceSurface {
        /// Index into the surface table.
        surface: usize,
        /// Use the surface's natural UV domain instead of `domain`.
        natural: bool,
        /// `(u0, v0, u1, v1)`.
        domain: [f64; 4],
    },
    /// `StartLoop(orient)`.
    StartLoop(Orient),
    /// `EndLoop`.
    EndLoop,
    /// `StartAndEndSingleVertexLoop(point)`.
    SingleVertexLoop([f64; 3]),
    /// `StartVertexOfLoop(point)`.
    StartVertexOfLoop([f64; 3]),
    /// `StartEdge(orient, edge, start, end)`.
    StartEdge {
        /// `eOrientationInLoop`.
        orient: Orient,
        /// An earlier `StartEdge` result to join.
        edge: Option<usize>,
        /// An earlier vertex result.
        start: Option<usize>,
        /// An earlier vertex result.
        end: Option<usize>,
    },
    /// `SetEdgeCurve(curve, uv)`.
    SetEdgeCurve {
        /// Index into the curve table; `None` passes `NULL`.
        curve: Option<usize>,
        /// `bUVCurve`.
        uv: bool,
    },
    /// `SetLimits(start, end)`.
    SetLimits(Node, Node),
    /// `EndEdge`.
    EndEdge,
    /// `StartVertex(point)`.
    StartVertex([f64; 3]),
    /// `EndVertex`.
    EndVertex,
    /// `SwapRegions`.
    SwapRegions,
    /// `SmSurface::AdjustSTEPUVDomain(domain)` on a table surface not yet taken by
    /// `SetFaceSurface` (an importer's call, not the constructor's).
    AdjustStepUvDomain {
        /// Index into the surface table.
        surface: usize,
        /// `(u0, v0, u1, v1)` in STEP parameters (degrees for angles).
        domain: [f64; 4],
    },
}

/// A script: the geometry table and the calls.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Script {
    /// The curve table.
    pub curves: Vec<ScriptCurve>,
    /// The surface table.
    pub surfaces: Vec<Surface>,
    /// The calls, in order.
    pub ops: Vec<Op>,
}

fn flag(x: bool) -> f64 {
    if x {
        1.0
    } else {
        0.0
    }
}

fn opt(x: Option<usize>) -> f64 {
    x.map_or(-1.0, |i| i as f64)
}

/// Encodes a script (see the module docs for the layout).
pub fn encode(s: &Script) -> Vec<f64> {
    let mut o = vec![
        MAGIC,
        VERSION,
        s.curves.len() as f64,
        s.surfaces.len() as f64,
        s.ops.len() as f64,
    ];
    for c in &s.curves {
        o.push(f64::from(c.dim));
        let (b, lo, hi) = match c.bounds {
            Some((lo, hi)) => (1.0, lo, hi),
            None => (0.0, 0.0, 0.0),
        };
        o.extend_from_slice(&[b, lo, hi]);
        encode_curve(&mut o, &c.curve);
    }
    for sf in &s.surfaces {
        encode_surface(&mut o, sf);
    }
    for op in &s.ops {
        match op {
            Op::StartBrep => o.push(0.0),
            Op::EndBrep => o.push(1.0),
            Op::StartRegion { solid, outer } => {
                o.push(2.0);
                o.push(flag(*solid));
                o.push(match outer {
                    RegionRef::Null => -1.0,
                    RegionRef::Infinite => -2.0,
                    RegionRef::Index(i) => *i as f64,
                });
            }
            Op::EndRegion => o.push(3.0),
            Op::StartShell => o.push(4.0),
            Op::EndShell { stitch } => o.extend_from_slice(&[5.0, flag(*stitch)]),
            Op::StartFace(x) => o.extend_from_slice(&[6.0, x.code()]),
            Op::EndFace => o.push(7.0),
            Op::SetFaceSurface {
                surface,
                natural,
                domain,
            } => {
                o.extend_from_slice(&[8.0, *surface as f64, flag(*natural)]);
                o.extend_from_slice(domain);
            }
            Op::StartLoop(x) => o.extend_from_slice(&[9.0, x.code()]),
            Op::EndLoop => o.push(10.0),
            Op::SingleVertexLoop(p) => {
                o.push(11.0);
                o.extend_from_slice(p);
            }
            Op::StartVertexOfLoop(p) => {
                o.push(12.0);
                o.extend_from_slice(p);
            }
            Op::StartEdge {
                orient,
                edge,
                start,
                end,
            } => o.extend_from_slice(&[13.0, orient.code(), opt(*edge), opt(*start), opt(*end)]),
            Op::SetEdgeCurve { curve, uv } => o.extend_from_slice(&[14.0, opt(*curve), flag(*uv)]),
            Op::SetLimits(a, b) => {
                o.push(15.0);
                for n in [a, b] {
                    match n {
                        Node::Point(p) => o.extend_from_slice(&[0.0, p[0], p[1], p[2]]),
                        Node::Parameter(t) => o.extend_from_slice(&[1.0, *t, 0.0, 0.0]),
                        Node::Uv(p) => o.extend_from_slice(&[2.0, p[0], p[1], 0.0]),
                        Node::Empty => o.extend_from_slice(&[3.0, 0.0, 0.0, 0.0]),
                    }
                }
            }
            Op::EndEdge => o.push(16.0),
            Op::StartVertex(p) => {
                o.push(17.0);
                o.extend_from_slice(p);
            }
            Op::EndVertex => o.push(18.0),
            Op::SwapRegions => o.push(19.0),
            Op::AdjustStepUvDomain { surface, domain } => {
                o.extend_from_slice(&[20.0, *surface as f64]);
                o.extend_from_slice(domain);
            }
        }
    }
    o
}

struct Rd<'a> {
    d: &'a [f64],
    i: usize,
}

impl Rd<'_> {
    fn num(&mut self) -> Result<f64, String> {
        let x = *self.d.get(self.i).ok_or("truncated")?;
        self.i += 1;
        if x.is_finite() {
            Ok(x)
        } else {
            Err("non-finite number".into())
        }
    }

    /// An integer in `[0, n)`.
    fn index(&mut self, n: usize) -> Result<usize, String> {
        let x = self.num()?;
        if x >= 0.0 && x.fract() == 0.0 && x < n as f64 {
            Ok(x as usize)
        } else {
            Err(format!("index {x} out of [0, {n})"))
        }
    }

    /// `-1` or an integer in `[0, n)`.
    fn opt_index(&mut self, n: usize) -> Result<Option<usize>, String> {
        let x = self.num()?;
        if x == -1.0 {
            return Ok(None);
        }
        if x >= 0.0 && x.fract() == 0.0 && x < n as f64 {
            Ok(Some(x as usize))
        } else {
            Err(format!("reference {x} out of [-1, {n})"))
        }
    }

    fn count(&mut self, min_words: usize) -> Result<usize, String> {
        let left = self.d.len().saturating_sub(self.i);
        self.index(left / min_words.max(1) + 1)
    }

    fn flag(&mut self) -> Result<bool, String> {
        Ok(self.index(2)? == 1)
    }

    fn orient(&mut self) -> Result<Orient, String> {
        Ok(match self.index(3)? {
            0 => Orient::Unknown,
            1 => Orient::Same,
            _ => Orient::Opposite,
        })
    }

    fn p3(&mut self) -> Result<[f64; 3], String> {
        Ok([self.num()?, self.num()?, self.num()?])
    }

    fn node(&mut self) -> Result<Node, String> {
        let kind = self.index(4)?;
        let [a, b, c] = self.p3()?;
        Ok(match kind {
            0 => Node::Point([a, b, c]),
            1 => Node::Parameter(a),
            2 => Node::Uv([a, b]),
            _ => Node::Empty,
        })
    }
}

/// Reads an encoded script back, validating everything the C++ shim relies on: the
/// header, counts against the data left, the geometry as [`crate::flat::decode`]
/// validates it (a 2D curve is a bounded line or a B-spline), each curve and surface
/// used at most once, and every reference to an earlier result.
pub fn decode(data: &[f64]) -> Result<Script, String> {
    let mut r = Rd { d: data, i: 0 };
    if r.num()? != MAGIC || r.num()? != VERSION {
        return Err("bad magic or version".into());
    }
    let nc = r.count(8)?;
    let ns = r.count(13)?;
    let no = r.count(1)?;
    let mut s = Script::default();
    for _ in 0..nc {
        let dim = r.index(4)?;
        if dim != 2 && dim != 3 {
            return Err(format!("curve dimension {dim}"));
        }
        let bounded = r.flag()?;
        let lo = r.num()?;
        let hi = r.num()?;
        let mut pos = r.i;
        let curve = decode_curve(data, &mut pos)?;
        r.i = pos;
        let bounds = bounded.then_some((lo, hi));
        if let Some((lo, hi)) = bounds {
            if lo > hi {
                return Err("decreasing curve bounds".into());
            }
        }
        if dim == 2 {
            match &curve {
                Curve::Line { .. } if bounded => {}
                Curve::BSpline(_) => {}
                _ => return Err("a 2D curve is a bounded line or a B-spline".into()),
            }
        }
        s.curves.push(ScriptCurve {
            dim: dim as u32,
            bounds,
            curve,
        });
    }
    for _ in 0..ns {
        let mut pos = r.i;
        let sf = decode_surface(data, &mut pos)?;
        r.i = pos;
        s.surfaces.push(sf);
    }
    let (mut regions, mut edges, mut vertices) = (0usize, 0usize, 0usize);
    let mut curve_used = vec![false; nc];
    let mut surface_used = vec![false; ns];
    for _ in 0..no {
        let op = match r.index(21)? {
            0 => Op::StartBrep,
            1 => Op::EndBrep,
            2 => {
                let solid = r.flag()?;
                let x = r.num()?;
                let outer = if x == -1.0 {
                    RegionRef::Null
                } else if x == -2.0 {
                    RegionRef::Infinite
                } else if x >= 0.0 && x.fract() == 0.0 && x < regions as f64 {
                    RegionRef::Index(x as usize)
                } else {
                    return Err(format!("region reference {x}"));
                };
                regions += 1;
                Op::StartRegion { solid, outer }
            }
            3 => Op::EndRegion,
            4 => Op::StartShell,
            5 => Op::EndShell { stitch: r.flag()? },
            6 => Op::StartFace(r.orient()?),
            7 => Op::EndFace,
            8 => {
                let surface = r.index(ns)?;
                let used = surface_used.get_mut(surface).ok_or("surface index")?;
                if *used {
                    return Err(format!("surface {surface} used twice"));
                }
                *used = true;
                let natural = r.flag()?;
                let domain = [r.num()?, r.num()?, r.num()?, r.num()?];
                Op::SetFaceSurface {
                    surface,
                    natural,
                    domain,
                }
            }
            9 => Op::StartLoop(r.orient()?),
            10 => Op::EndLoop,
            11 => Op::SingleVertexLoop(r.p3()?),
            12 => {
                vertices += 1;
                Op::StartVertexOfLoop(r.p3()?)
            }
            13 => {
                let orient = r.orient()?;
                let edge = r.opt_index(edges)?;
                let start = r.opt_index(vertices)?;
                let end = r.opt_index(vertices)?;
                edges += 1;
                Op::StartEdge {
                    orient,
                    edge,
                    start,
                    end,
                }
            }
            14 => {
                let curve = r.opt_index(nc)?;
                if let Some(c) = curve {
                    let used = curve_used.get_mut(c).ok_or("curve index")?;
                    if *used {
                        return Err(format!("curve {c} used twice"));
                    }
                    *used = true;
                }
                Op::SetEdgeCurve {
                    curve,
                    uv: r.flag()?,
                }
            }
            15 => Op::SetLimits(r.node()?, r.node()?),
            16 => Op::EndEdge,
            17 => {
                vertices += 1;
                Op::StartVertex(r.p3()?)
            }
            18 => Op::EndVertex,
            19 => Op::SwapRegions,
            _ => {
                let surface = r.index(ns)?;
                if surface_used.get(surface).copied().unwrap_or(true) {
                    return Err(format!("surface {surface} adjusted after it was taken"));
                }
                let domain = [r.num()?, r.num()?, r.num()?, r.num()?];
                Op::AdjustStepUvDomain { surface, domain }
            }
        };
        s.ops.push(op);
    }
    if r.i != data.len() {
        return Err("trailing data".into());
    }
    Ok(s)
}

/// Options of [`from_body`].
#[allow(clippy::struct_excessive_bools)] // independent switches of the recipe
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FromBodyOptions {
    /// Bound edges with their curve parameters instead of their vertex points.
    pub parameter_limits: bool,
    /// `EndShell(TRUE)`: stitch each shell when it is ended.
    pub stitch: bool,
    /// Bind every face surface over its natural UV domain, without the importer's
    /// `AdjustSTEPUVDomain` to the face's STEP parameter box ([`step_uv_domain`]).
    pub natural_domains: bool,
    /// Make a new edge (with its own curve, and vertices made by `EndLoop`) for every
    /// oriented edge instead of joining shared ones (to be stitched).
    pub separate_edges: bool,
}

/// The script an importer runs for `body`: one region per solid (a void region inside
/// it for each void shell of a solid with voids; no region for a shell model), each
/// face with the outward face use on the outer shell, its surface over the natural
/// domain, its outer loop first, every edge made at its first use (with its 3D curve
/// bounded to the edge as `difftest --step-corpus` makes it, and limits from its vertex
/// points) and joined at the next ones, vertices made at the first edge that uses
/// them.
pub fn from_body(body: &Body, options: FromBodyOptions) -> Script {
    let mut s = Script::default();
    let mut edge_ref: Vec<Option<usize>> = vec![None; body.edges.len()];
    let mut vertex_ref: Vec<Option<usize>> = vec![None; body.vertices.len()];
    let (mut n_edges, mut n_vertices) = (0usize, 0usize);
    s.ops.push(Op::StartBrep);
    let solid = !matches!(body.kind, BodyKind::ShellModel);
    let point = |v: usize| body.vertices.get(v).map_or([0.0; 3], |x| x.point);

    for (si, shell) in body.shells.iter().enumerate() {
        let void = solid && si > 0 && matches!(body.kind, BodyKind::SolidWithVoids);
        if solid {
            s.ops.push(Op::StartRegion {
                solid: true,
                outer: if void {
                    RegionRef::Index(0)
                } else {
                    RegionRef::Null
                },
            });
        }
        s.ops.push(Op::StartShell);
        for &fi in &shell.faces {
            let Some(face) = body.faces.get(fi) else {
                continue;
            };
            // the face normal (after the shell's orientation) points away from the
            // material: into the outer region of a solid's shell, into the void region
            // of a void shell
            let normal_is_surface_normal = face.same_sense == shell.orientation;
            let surface = s.surfaces.len();
            if !options.natural_domains {
                if let Some(domain) = step_uv_domain(body, face) {
                    s.ops.push(Op::AdjustStepUvDomain { surface, domain });
                }
            }
            s.ops.push(Op::StartFace(Orient::from_same(
                normal_is_surface_normal != void,
            )));
            s.surfaces.push(face.surface.clone());
            s.ops.push(Op::SetFaceSurface {
                surface,
                natural: true,
                domain: [0.0; 4],
            });
            // outer loop first
            let mut loops: Vec<&crate::model::Loop> =
                face.loops.iter().filter(|l| l.outer).collect();
            loops.extend(face.loops.iter().filter(|l| !l.outer));
            for l in loops {
                let es = match &l.bound {
                    Bound::Vertex(v) => {
                        s.ops.push(Op::SingleVertexLoop(point(*v)));
                        continue;
                    }
                    Bound::Edges(es) => es,
                };
                // the loop runs counter-clockwise about the face's own normal
                s.ops
                    .push(Op::StartLoop(Orient::from_same(face.same_sense)));
                let oes: Vec<(usize, bool)> = if l.orientation {
                    es.iter().map(|e| (e.edge, e.forward)).collect()
                } else {
                    es.iter().rev().map(|e| (e.edge, !e.forward)).collect()
                };
                for (ei, forward) in oes {
                    let Some(e) = body.edges.get(ei) else {
                        continue;
                    };
                    let orient = Orient::from_same(forward == e.same_sense);
                    let existing = if options.separate_edges {
                        None
                    } else {
                        edge_ref.get(ei).copied().flatten()
                    };
                    if let Some(x) = existing {
                        s.ops.push(Op::StartEdge {
                            orient,
                            edge: Some(x),
                            start: None,
                            end: None,
                        });
                        n_edges += 1;
                        s.ops.push(Op::EndEdge);
                        continue;
                    }
                    // the edge's vertices in its curve's direction, made at first use:
                    // StartEdge gives the start vertex to the first edge use exactly when
                    // that use runs with the curve (its doc's "relative to the loop
                    // orientation" notwithstanding; smTopoTest passes them so too)
                    let (a, b) = if e.same_sense {
                        (e.start, e.end)
                    } else {
                        (e.end, e.start)
                    };
                    let mut refs = [None, None];
                    for (slot, v) in refs.iter_mut().zip([a, b]) {
                        if options.separate_edges {
                            break;
                        }
                        if let Some(r) = vertex_ref.get_mut(v) {
                            if r.is_none() {
                                s.ops.push(Op::StartVertexOfLoop(point(v)));
                                *r = Some(n_vertices);
                                n_vertices += 1;
                            }
                            *slot = *r;
                        }
                    }
                    s.ops.push(Op::StartEdge {
                        orient,
                        edge: None,
                        start: refs[0],
                        end: refs[1],
                    });
                    if let Some(r) = edge_ref.get_mut(ei) {
                        *r = Some(n_edges);
                    }
                    n_edges += 1;
                    let (lo, hi) = if e.same_sense {
                        (e.t_start, e.t_end)
                    } else {
                        (e.t_end, e.t_start)
                    };
                    let curve = s.curves.len();
                    s.curves.push(ScriptCurve {
                        dim: 3,
                        bounds: Some((lo.min(hi), lo.max(hi))),
                        curve: e.curve.clone(),
                    });
                    s.ops.push(Op::SetEdgeCurve {
                        curve: Some(curve),
                        uv: false,
                    });
                    // the curve's start and end vertices
                    let (cs, ce) = if e.same_sense {
                        (e.start, e.end)
                    } else {
                        (e.end, e.start)
                    };
                    if options.parameter_limits {
                        s.ops.push(Op::SetLimits(
                            Node::Parameter(lo.min(hi)),
                            Node::Parameter(lo.max(hi)),
                        ));
                    } else {
                        s.ops.push(Op::SetLimits(
                            Node::Point(point(cs)),
                            Node::Point(point(ce)),
                        ));
                    }
                    s.ops.push(Op::EndEdge);
                }
                s.ops.push(Op::EndLoop);
            }
            s.ops.push(Op::EndFace);
        }
        s.ops.push(Op::EndShell {
            stitch: options.stitch,
        });
        if solid {
            s.ops.push(Op::EndRegion);
        }
    }
    s.ops.push(Op::EndBrep);
    s
}

/// The area element of a surface as a function of its second parameter.
type AreaElement = Box<dyn Fn(f64) -> f64>;
/// A point's surface parameters.
type ParamMap = Box<dyn Fn(P3) -> (f64, f64)>;

/// Samples per edge for [`step_uv_domain`].
const EDGE_SAMPLES: usize = 32;

/// The point of edge `e` at fraction `s` of its parameter range, from `t_start` to
/// `t_end` (the mapper orders a periodic range in the edge's sense: `t_end > t_start`
/// along the curve, `t_end < t_start` against it).
pub fn edge_point(e: &crate::model::Edge, s: f64) -> Option<P3> {
    let (t0, t1) = (e.t_start, e.t_end);
    match &e.curve {
        Curve::Line {
            origin,
            dir,
            magnitude,
        } => {
            let t = t0 + (t1 - t0) * s;
            Some(add(*origin, scale(*dir, t * magnitude)))
        }
        Curve::Circle { frame, radius } => Some(conic(frame, *radius, *radius, t0 + (t1 - t0) * s)),
        Curve::Ellipse {
            frame,
            semi1,
            semi2,
        } => Some(conic(frame, *semi1, *semi2, t0 + (t1 - t0) * s)),
        Curve::BSpline(b) => bspline_point(b, t0 + (t1 - t0) * s),
    }
}

fn add(a: P3, b: P3) -> P3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn conic(f: &crate::model::Frame, a: f64, b: f64, deg: f64) -> P3 {
    let r = deg.to_radians();
    add(
        f.origin,
        add(scale(f.x, a * r.cos()), scale(f.y, b * r.sin())),
    )
}

/// Unwraps an angle (degrees) to within 180 of `prev`.
fn unwrap(prev: f64, x: f64) -> f64 {
    x + 360.0 * ((prev - x) / 360.0).round()
}

/// The face's box in its surface's STEP parameters (what an importer passes to
/// `AdjustSTEPUVDomain`): its edge loops sampled in loop order and mapped to the
/// surface's parameters (angles in degrees, unwrapped along each loop, later loops
/// moved into the first loop's period; a closed direction spanning a full turn is one
/// period, its start moved into `[0, 360)` and the range kept within `[-360, 360]`;
/// vertex loops add their point in the directions that are not periodic).
/// `None` for a face without loops and for surfaces whose STEP parameters are not
/// analytic here (revolution, extrusion, B-spline).
pub fn step_uv_domain(body: &Body, face: &crate::model::Face) -> Option<[f64; 4]> {
    let map: ParamMap = match &face.surface {
        Surface::Plane { frame } => {
            let f = *frame;
            Box::new(move |p| (dot(sub(p, f.origin), f.x), dot(sub(p, f.origin), f.y)))
        }
        Surface::Cylinder { frame, .. } | Surface::Cone { frame, .. } => {
            let f = *frame;
            Box::new(move |p| {
                let d = sub(p, f.origin);
                (dot(d, f.y).atan2(dot(d, f.x)).to_degrees(), dot(d, f.z))
            })
        }
        Surface::Sphere { frame, radius } => {
            let (f, r) = (*frame, *radius);
            Box::new(move |p| {
                let d = sub(p, f.origin);
                let lat = (dot(d, f.z) / r).clamp(-1.0, 1.0).asin().to_degrees();
                (dot(d, f.y).atan2(dot(d, f.x)).to_degrees(), lat)
            })
        }
        Surface::Torus { frame, major, .. } => {
            let (f, big) = (*frame, *major);
            Box::new(move |p| {
                let d = sub(p, f.origin);
                let (x, y) = (dot(d, f.x), dot(d, f.y));
                let rho = (x * x + y * y).sqrt();
                (
                    y.atan2(x).to_degrees(),
                    dot(d, f.z).atan2(rho - big).to_degrees(),
                )
            })
        }
        _ => return None,
    };
    let (u_periodic, v_periodic) = match &face.surface {
        Surface::Plane { .. } => (false, false),
        Surface::Torus { .. } => (true, true),
        _ => (true, false),
    };
    let mut loops_uv: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut vertex_points: Vec<(f64, f64)> = Vec::new();
    for l in &face.loops {
        let es = match &l.bound {
            Bound::Edges(es) => es,
            Bound::Vertex(v) => {
                // a vertex loop (an apex or a pole) bounds the face's box in the
                // directions that are not periodic (its angle is undefined)
                vertex_points.push(map(body.vertices.get(*v)?.point));
                continue;
            }
        };
        let mut pts = Vec::new();
        for oe in es {
            let e = body.edges.get(oe.edge)?;
            let along = oe.forward == e.same_sense;
            for j in 0..=EDGE_SAMPLES {
                let s = j as f64 / EDGE_SAMPLES as f64;
                pts.push(map(edge_point(e, if along { s } else { 1.0 - s })?));
            }
        }
        loops_uv.push(pts);
    }
    let axis = |pick: fn(&(f64, f64)) -> f64, periodic: bool| -> Option<(f64, f64)> {
        let loops: Vec<Vec<f64>> = loops_uv
            .iter()
            .map(|l| l.iter().map(pick).collect())
            .collect();
        let extra: Vec<f64> = if periodic {
            Vec::new()
        } else {
            vertex_points.iter().map(pick).collect()
        };
        axis_range(&loops, periodic, &extra)
    };
    let (u0, u1) = axis(|p| p.0, u_periodic)?;
    let (mut v0, mut v1) = axis(|p| p.1, v_periodic)?;
    // a cone face reaching its apex: the samples' rounding must not pass it
    if let Surface::Cone {
        radius,
        semi_angle_deg,
        ..
    } = &face.surface
    {
        let tan = semi_angle_deg.to_radians().tan();
        if tan != 0.0 {
            let apex = -radius / tan;
            v0 = v0.max(apex);
            v1 = v1.max(apex);
        }
    }
    Some([u0, v0, u1, v1])
}

/// The range of one parameter over the loops' samples (angles unwrapped along each loop
/// and later loops moved into the first loop's period when `periodic`; a full turn is
/// one period starting in `[0, 360)`), widened by `extra`; `None` without samples.
fn axis_range(loops: &[Vec<f64>], periodic: bool, extra: &[f64]) -> Option<(f64, f64)> {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    let mut first_mid: Option<f64> = None;
    for l in loops {
        let mut vals: Vec<f64> = Vec::new();
        for &x in l {
            let x = match (periodic, vals.last()) {
                (true, Some(&prev)) => unwrap(prev, x),
                _ => x,
            };
            vals.push(x);
        }
        // keep later loops in the first loop's period
        let shift = match (periodic, first_mid, vals.first()) {
            (true, Some(m), Some(&p)) => unwrap(m, p) - p,
            _ => 0.0,
        };
        let llo = vals.iter().fold(f64::INFINITY, |a, &x| a.min(x + shift));
        let lhi = vals
            .iter()
            .fold(f64::NEG_INFINITY, |a, &x| a.max(x + shift));
        if first_mid.is_none() {
            first_mid = Some((llo + lhi) / 2.0);
        }
        lo = lo.min(llo);
        hi = hi.max(lhi);
    }
    if !(lo.is_finite() && hi.is_finite()) {
        return None;
    }
    for &x in extra {
        lo = lo.min(x);
        hi = hi.max(x);
    }
    if periodic {
        if hi - lo > 360.0 - 1e-6 {
            hi = lo + 360.0;
        }
        // start the period in [0, 360) (a start within 1e-6 of a whole turn on it),
        // then keep it inside the [-360, 360] that AdjustSTEPUVDomain takes
        let mut turns = (lo / 360.0).floor();
        if (lo - 360.0 * (turns + 1.0)).abs() < 1e-6 {
            turns += 1.0;
        }
        lo -= 360.0 * turns;
        hi -= 360.0 * turns;
        if hi > 360.0 + 1e-6 {
            lo -= 360.0;
            hi -= 360.0;
        }
    }
    Some((lo, hi))
}

/// An estimate of the face's area independent of either kernel: its edge loops
/// sampled (`samples` points per edge) in the surface's STEP parameters, unwrapped as
/// in [`step_uv_domain`], and the area element integrated over them by Green's theorem
/// (`|sum over loops of the closed integral of u f(v) dv|`, with `f` the area element:
/// `1` for a plane, `r` for a cylinder (u in radians), `(R + v tan a) / cos a` for a
/// cone, `r^2 cos v` for a sphere, `r (R + r cos v)` for a torus). Exact up to the
/// polygon approximation of the loops in parameter space. `None` where
/// [`step_uv_domain`] has no parameters.
pub fn step_face_area(body: &Body, face: &crate::model::Face, samples: usize) -> Option<f64> {
    step_uv_domain(body, face)?;
    let (f, map): (AreaElement, ParamMap) = match &face.surface {
        Surface::Plane { frame } => {
            let fr = *frame;
            (
                Box::new(|_| 1.0),
                Box::new(move |p| (dot(sub(p, fr.origin), fr.x), dot(sub(p, fr.origin), fr.y))),
            )
        }
        Surface::Cylinder { frame, radius } => {
            let (fr, r) = (*frame, *radius);
            (
                Box::new(move |_| r),
                Box::new(move |p| {
                    let d = sub(p, fr.origin);
                    (dot(d, fr.y).atan2(dot(d, fr.x)), dot(d, fr.z))
                }),
            )
        }
        Surface::Cone {
            frame,
            radius,
            semi_angle_deg,
        } => {
            let (fr, r, a) = (*frame, *radius, semi_angle_deg.to_radians());
            (
                Box::new(move |v| (r + v * a.tan()) / a.cos()),
                Box::new(move |p| {
                    let d = sub(p, fr.origin);
                    (dot(d, fr.y).atan2(dot(d, fr.x)), dot(d, fr.z))
                }),
            )
        }
        Surface::Sphere { frame, radius } => {
            let (fr, r) = (*frame, *radius);
            (
                Box::new(move |v| r * r * v.cos()),
                Box::new(move |p| {
                    let d = sub(p, fr.origin);
                    let lat = (dot(d, fr.z) / r).clamp(-1.0, 1.0).asin();
                    (dot(d, fr.y).atan2(dot(d, fr.x)), lat)
                }),
            )
        }
        Surface::Torus {
            frame,
            major,
            minor,
        } => {
            let (fr, big, r) = (*frame, *major, *minor);
            (
                Box::new(move |v| r * (big + r * v.cos())),
                Box::new(move |p| {
                    let d = sub(p, fr.origin);
                    let (x, y) = (dot(d, fr.x), dot(d, fr.y));
                    let rho = (x * x + y * y).sqrt();
                    (y.atan2(x), dot(d, fr.z).atan2(rho - big))
                }),
            )
        }
        _ => return None,
    };
    let tau = std::f64::consts::TAU;
    let unwrap_rad = |prev: f64, x: f64| x + tau * ((prev - x) / tau).round();
    let (pu, pv) = match &face.surface {
        Surface::Plane { .. } => (false, false),
        Surface::Torus { .. } => (true, true),
        _ => (true, false),
    };
    let mut total = 0.0;
    for l in &face.loops {
        let Bound::Edges(es) = &l.bound else {
            continue;
        };
        let mut pts: Vec<(f64, f64)> = Vec::new();
        // the loop's traversal (a bound with orientation false runs backwards)
        let oes: Vec<(usize, bool)> = if l.orientation {
            es.iter().map(|e| (e.edge, e.forward)).collect()
        } else {
            es.iter().rev().map(|e| (e.edge, !e.forward)).collect()
        };
        for (ei, forward) in oes {
            let e = body.edges.get(ei)?;
            let along = forward == e.same_sense;
            for j in 0..samples.max(1) {
                let t = j as f64 / samples.max(1) as f64;
                let (mut u, mut v) = map(edge_point(e, if along { t } else { 1.0 - t })?);
                if let Some(&(qu, qv)) = pts.last() {
                    if pu {
                        u = unwrap_rad(qu, u);
                    }
                    if pv {
                        v = unwrap_rad(qv, v);
                    }
                }
                pts.push((u, v));
            }
        }
        // close the loop (a loop around a periodic direction does not close in u: its
        // seam edges close it)
        for (&(u0, v0), &(u1, v1)) in pts.iter().zip(pts.iter().cycle().skip(1)) {
            let (mut u1, mut v1) = (u1, v1);
            if pu {
                u1 = unwrap_rad(u0, u1);
            }
            if pv {
                v1 = unwrap_rad(v0, v1);
            }
            // trapezoid in v of u f(v)
            total += 0.5 * (u0 * f(v0) + u1 * f(v1)) * (v1 - v0);
        }
    }
    Some(total.abs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_rejects() {
        let s = Script {
            curves: vec![ScriptCurve {
                dim: 3,
                bounds: Some((0.0, 1.0)),
                curve: Curve::Line {
                    origin: [0.0; 3],
                    dir: [1.0, 0.0, 0.0],
                    magnitude: 1.0,
                },
            }],
            surfaces: vec![],
            ops: vec![
                Op::StartBrep,
                Op::StartRegion {
                    solid: true,
                    outer: RegionRef::Infinite,
                },
                Op::StartVertex([1.0, 2.0, 3.0]),
                Op::StartEdge {
                    orient: Orient::Same,
                    edge: None,
                    start: Some(0),
                    end: None,
                },
                Op::SetEdgeCurve {
                    curve: Some(0),
                    uv: false,
                },
                Op::SetLimits(Node::Parameter(0.5), Node::Uv([1.0, 2.0])),
                Op::EndBrep,
            ],
        };
        let flat = encode(&s);
        assert_eq!(decode(&flat), Ok(s.clone()));
        // a reference to a later result
        let mut bad = s.clone();
        bad.ops.insert(
            1,
            Op::StartEdge {
                orient: Orient::Same,
                edge: Some(0),
                start: None,
                end: None,
            },
        );
        assert!(decode(&encode(&bad)).is_err());
        // a curve used twice
        let mut bad = s;
        bad.ops.push(Op::SetEdgeCurve {
            curve: Some(0),
            uv: true,
        });
        assert!(decode(&encode(&bad)).is_err());
        assert!(decode(&flat[..flat.len() - 1]).is_err());
    }
}
