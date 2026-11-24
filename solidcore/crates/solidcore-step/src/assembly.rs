//! A B-rep assembled from loose faces, edges and vertices into the kernel-neutral record, schema
//! 4 ([`solidcore_record::brep`]), whatever file they were read from. A reader adds each vertex
//! ([`Assembly::add_vertex`]), each edge on its curve between two of them
//! ([`Assembly::add_edge`]) and each face on its surface with its bounds
//! ([`Assembly::add_face`]), then [`Assembly::finish`] makes the record: seams for faces that
//! wind around a periodic surface, frames turned so a round face's form covers it once, each
//! edge's label and NURBS form, every use's pcurve (closed forms where the geometry has them, the
//! kernel's [`Trims`] for the rest), outer loops and face sides checked against the pcurves,
//! loops of a face that touch at one vertex made one loop, tolerances raised to the gaps the
//! geometry has, regions, and the canonical order.
//!
//! A face on a surface whose record form depends on the face (a linear sweep or a revolution of
//! a record curve, a cone that may be the plane it nearly is) gets that surface from
//! [`Assembly::swept_surface`], by the code the STEP reader reads its own such surfaces with.
//!
//! `docs/import-assembly.md` (solidcore's docs) describes the stage as a whole.
//!
//! The STEP reader ([`crate::topology`]) is one such reader; it was where this was written, and
//! its notes and refusals still name a face by its file entity (`face #id`). Any other reader
//! gives each face an `origin` of its own to be named by.
//!
//! What a reader gives: curves and surfaces as record curves and surfaces, in millimetres and
//! radians; each edge's interval on its curve from min to max, its vertices at the interval's
//! start and end; each bound relative to the surface's natural normal (seen from that normal an
//! outer loop runs counter-clockwise), and whether the face's side is that normal.

use crate::vec::Vec3Geom as _;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::f64::consts::TAU;

use crate::kernel::{CurveEval, SurfaceEval, face_flux};
use solidcore_ops::pcurve::{self, EdgeUse, TrimEdge, TrimFace, TrimUse, Trims};
use solidcore_record::Vec3;
use solidcore_record::brep::{
    self, Brep, Curve as BCurve, Curve2, Edge, EdgeUseRef, Face, Frame, Loop, Natural, OrientedEdge, OrientedFace, Region, Shell,
    Surface as BSurface, Vertex,
};

use crate::geom::P3;
use crate::read::R;
use crate::topology::swept::Extent;

mod extend;
mod frames;
mod reseam;
pub(crate) mod seams;
mod slivers;
mod touching;
mod wound;

/// Most control points in one direction of a schema spline.
pub(crate) const MAX_SPLINE_POINTS: usize = 4096;
/// Highest schema spline degree.
pub(crate) const MAX_SPLINE_DEGREE: usize = 16;
/// The largest tolerance (mm) an edge or vertex is given: SMLib takes none larger (smlib.h's
/// EDGE_TOLERANCE and VERTEX_TOLERANCE). Geometry that
/// would need more is refused, with how far off it lies.
pub(crate) const MAX_TOLERANCE: f64 = 1.0;
/// A vertex's tolerance over its largest gap from its curves and surfaces.
pub(crate) const VERTEX_GAP_MARGIN: f64 = 1.5;
/// Intervals an edge's curve is measured over against each face's surface it bounds.
pub(crate) const EDGE_GAP_SAMPLES: usize = 16;
/// The least span of a face's parameter box along a direction of its surface that is not
/// closed on itself, where its natural range has room ([`face_domain`]): 50 times the 2e-5 mm
/// within which SMLib takes two of a surface's sides for one (#12757: a planar sliver face
/// 1e-5 mm wide).
pub(crate) const OPEN_DOMAIN_SPAN: f64 = 1e-3;
/// Most faces a refusal of failed pcurves names (`face #origin` each), so a reader can leave
/// them out of one read again together; the rest are named by the next read's refusal.
pub(crate) const MAX_NAMED_FAILURES: usize = 64;
/// An edge's tolerance over the largest gap measured between its pcurves and its curve, as the
/// record check measures it: with margin, as a vertex's.
pub(crate) const EDGE_GAP_MARGIN: f64 = 1.5;

/// A body (a solid or a sheet) assembled into a record ([`Assembly::finish`]).
#[derive(Clone, Debug)]
pub struct Solid {
    /// The reader's name for the body item the record was assembled from, as given to
    /// [`Assembly::finish`] (the STEP reader gives the solid item's entity id).
    pub item: u64,
    /// The body's name, as given to [`Assembly::finish`]; a reader may fill it in afterwards
    /// (the STEP reader puts its product's name there when the item has none).
    pub name: String,
    /// The name of the product that holds the body, when the reader knows it.
    pub product: Option<String>,
    /// The record, in the coordinates its reader gave (no placement applied).
    pub brep: Brep,
    /// Each record face's `origin` as given to [`Assembly::add_face`], in the record's face
    /// order: how the reader names the face it came from.
    pub face_origin: Vec<u64>,
    /// No shell bounds a region: a body of open shells only.
    pub sheet: bool,
    /// What was repaired or noted on the way: seams added, edges turned or merged, faces turned
    /// over, tolerances raised, and what the reader noted itself.
    pub notes: Vec<String>,
    /// The record faces turned over because their outer loop runs clockwise about the side the
    /// reader gave them, in increasing order.
    pub turned: Vec<u32>,
}

/// What a body's shells make ([`Assembly::finish`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A solid: its one shell, closed, bounds its material.
    Solid,
    /// A solid with voids: its first shell is the outer one, the others bound its cavities; all
    /// closed.
    Voids,
    /// A surface model: open shells in the infinite region, and closed ones each bounding a
    /// region of its own.
    Model,
    /// A region of a general body (CADIR's, a solid's regions beside a sheet's): a solid with
    /// voids ([`Kind::Voids`]) when it has several shells and every one is closed, else a
    /// surface model ([`Kind::Model`]).
    General,
    /// A solid with faces left out (`crate::topology::read_solid_without`): its shells are
    /// sheets, but for a shell still closed, which bounds a region of its own (a void's cavity,
    /// where `voids`: the solid's shells after the first are its voids).
    Open {
        /// The solid's shells after the first are its voids.
        voids: bool,
        /// The first shell is the solid's outer one. False where every face of the outer shell
        /// was left out: then each shell is one of its voids, a cavity still.
        outer: bool,
    },
}

/// A curve the edge can be read on, and how its parameter relates to its vertices.
#[derive(Clone)]
pub(crate) struct EdgeB {
    pub(crate) curve: BCurve,
    pub(crate) interval: [f64; 2],
    pub(crate) vertices: [u32; 2],
    pub(crate) tolerance: f64,
    /// Allowance for constructing pcurves: the importer's precision plus measured gaps,
    /// independent of the sending system's possibly coarse distance uncertainty.
    pub(crate) pcurve_tolerance: f64,
    /// The curve's NURBS form and its interval there (`form::edge`), set before the pcurves.
    pub(crate) form: Option<(BCurve, [f64; 2])>,
}

/// A face's bound: a closed chain of edge uses (each an edge and whether the loop runs along
/// it, from its start vertex to its end), or one vertex (a pole, or an apex).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Bound {
    /// A closed chain of edge uses, in order.
    Edges(Vec<(u32, bool)>),
    /// One vertex.
    Vertex(u32),
}

#[derive(Clone)]
pub(crate) struct FaceB {
    /// What the reader names the face by in notes and refusals (`face #origin`), and what
    /// [`Solid::face_origin`] gives back.
    pub(crate) origin: u64,
    pub(crate) surface: BSurface,
    /// Relative to the surface's natural normal.
    pub(crate) loops: Vec<Bound>,
    /// The index in `loops` of the outer bound, if the reader marks one.
    pub(crate) outer: Option<usize>,
    /// The face's normal is the surface's natural normal.
    pub(crate) forward: bool,
    /// A round face's box on its label (`crate::kernel::form::extent`), once measured.
    pub(crate) extent: Option<[[f64; 2]; 2]>,
}

/// A surface whose record form depends on the face on it, given by its definition
/// ([`Assembly::swept_surface`]): in millimetres and radians.
#[derive(Clone, Debug, PartialEq)]
pub enum SweptSurface {
    /// `profile` moved along `direction` (a vector of any length but zero): the surface
    /// `profile(s) + t direction`. Its own normal is ∂profile × `direction`.
    LinearSweep {
        /// The profile, a record curve.
        profile: BCurve,
        /// The sweep's direction.
        direction: P3,
    },
    /// `profile` turned about the axis through `origin` along `axis` (any length but zero), the
    /// angle counter-clockwise about `axis`. Its own normal is ∂angle × ∂profile.
    Revolution {
        /// The profile, a record curve.
        profile: BCurve,
        /// A point on the axis.
        origin: P3,
        /// The axis's direction.
        axis: P3,
    },
    /// The record's circular cone (`Surface::Cone`): `radius` at the frame's origin, opening at
    /// `half_angle` along the frame's z. Its own normal is the record cone's.
    Cone {
        /// The cone's frame.
        frame: Frame,
        /// The radius at the frame's origin.
        radius: f64,
        /// The half angle.
        half_angle: f64,
    },
}

/// Faces, edges and vertices being assembled into a record ([`Assembly::finish`]).
#[derive(Clone)]
pub struct Assembly<'t> {
    pub(crate) tol: f64,
    /// Makes the pcurves of uses that have no closed form; without it such a face is refused.
    pub(crate) trims: Option<&'t dyn Trims>,
    pub(crate) vertices: Vec<Vertex>,
    pub(crate) edges: Vec<EdgeB>,
    pub(crate) faces: Vec<FaceB>,
    /// Edges the reader found running against the direction its file gave them (noted).
    pub(crate) sense_mismatches: usize,
    /// Line edges whose vertices lie off their line, read as the segment between the vertices.
    pub(crate) lines_between_vertices: usize,
    /// Inner loops written running the way the outer loop runs, run the other way.
    pub(crate) inner_loops_turned: usize,
    /// Loops that touched another loop of their face at one vertex, made one loop with it
    /// (`touching::join`).
    pub(crate) loops_joined: usize,
    /// The faces whose loops touch and are left as written: each one's origin and the first
    /// vertex they touch at.
    pub(crate) loops_left_touching: Vec<(u64, P3)>,
    /// Spline patches continued past their knot range to their face's trims ([`extend`]).
    pub(crate) patches_extended: usize,
    /// Loops run along a side of a spline face the file wrote as one vertex, its tolerance raised.
    pub(crate) sides_in_vertices: usize,
    /// Faces on a closed spline patch whose seam was moved off them ([`reseam`]).
    pub(crate) reseamed: HashSet<usize>,
    /// Edges added with their vertices at the other ends of their intervals, turned round
    /// ([`Assembly::add_edge`]).
    pub(crate) edges_turned: usize,
    /// Spline surfaces whose first and last control lines met within the tolerance, closed.
    pub(crate) splines_closed: usize,
    /// Control points of spline curves and surfaces whose polygon turned back on itself within
    /// the tolerance, moved to take the turn out ([`crate::topology::unfolded_curve`],
    /// [`crate::topology::unfolded_surface`]).
    pub(crate) points_unfolded: usize,
    /// Sides of spline surfaces that collapsed to within the tolerance of a point, made exact
    /// poles ([`crate::topology::poles_within`]).
    pub(crate) poles_made_exact: usize,
    /// Edges whose tolerance was raised to the gap their curve has to a face's surface.
    pub(crate) raised_edges: HashSet<u32>,
}

impl<'t> Assembly<'t> {
    /// An empty assembly at tolerance `tolerance` (mm): the record's, and every vertex's and
    /// edge's to start with. `trims` makes the pcurves that have no closed form; without it a
    /// face with such a use is refused.
    pub fn new(tolerance: f64, trims: Option<&'t dyn Trims>) -> Self {
        Assembly {
            tol: tolerance,
            trims,
            vertices: Vec::new(),
            edges: Vec::new(),
            faces: Vec::new(),
            sense_mismatches: 0,
            lines_between_vertices: 0,
            inner_loops_turned: 0,
            loops_joined: 0,
            loops_left_touching: Vec::new(),
            patches_extended: 0,
            sides_in_vertices: 0,
            reseamed: HashSet::new(),
            edges_turned: 0,
            splines_closed: 0,
            points_unfolded: 0,
            poles_made_exact: 0,
            raised_edges: HashSet::new(),
        }
    }

    /// The assembly's tolerance (mm).
    pub fn tolerance(&self) -> f64 {
        self.tol
    }

    /// Where vertex `v` is, if there is one.
    pub fn vertex_point(&self, v: u32) -> Option<P3> {
        self.vertices.get(v as usize).map(|x| x.point)
    }

    /// A vertex at `point`, its index.
    pub fn add_vertex(&mut self, point: P3) -> R<u32> {
        let i = index(self.vertices.len())?;
        self.vertices.push(Vertex { point, tolerance: self.tol });
        Ok(i)
    }

    /// An edge between vertices `ends` on `curve` over `interval` (min to max), its index and
    /// whether it runs from `ends[0]` (at the interval's start) to `ends[1]`. The way round
    /// whose vertices lie nearer the curve's points at the interval's ends is taken, so an edge
    /// whose producer wrote it the other way round is turned (noted); each vertex then gets its
    /// gap (with margin) as its tolerance, and one too far off refuses the edge, named
    /// `edge #origin`.
    pub fn add_edge(&mut self, origin: u64, curve: BCurve, interval: [f64; 2], ends: [u32; 2]) -> R<(u32, bool)> {
        // A control polygon that turns back on itself within the tolerance is a wiggle of the
        // file's, taken out before anything is measured on the curve.
        let curve = match crate::topology::unfolded_curve(&curve, self.tol) {
            Some((unfolded, moved)) => {
                self.points_unfolded += moved;
                unfolded
            }
            None => curve,
        };
        let eval = CurveEval::new(&curve).map_err(|m| format!("edge #{origin}: {m}"))?;
        let mut at = [Vec3::new(0.0, 0.0, 0.0); 2];
        for (slot, t) in at.iter_mut().zip(interval) {
            *slot = eval.point(t).ok_or_else(|| format!("edge #{origin}: no point at {t}"))?;
        }
        let [a, z] = [v3(self.point(ends[0])?), v3(self.point(ends[1])?)];
        let with = at[0].dist(a).max(at[1].dist(z));
        let against = at[0].dist(z).max(at[1].dist(a));
        // Not a number is no nearer either way round: the edge is taken as written.
        let along = !(against < with);
        let vertices = if along { ends } else { [ends[1], ends[0]] };
        if !along {
            self.edges_turned += 1;
        }
        for (t, v) in at.into_iter().zip(vertices) {
            let p = v3(self.point(v)?);
            self.raise_vertex(v, t.dist(p), origin)?;
        }
        let i = index(self.edges.len())?;
        self.edges.push(EdgeB { curve, interval, vertices, tolerance: self.tol, pcurve_tolerance: self.pcurve_tolerance(), form: None });
        Ok((i, along))
    }

    /// A sphere face bounded by vertex loops alone, each at one of its poles (a face of a closed
    /// surface bounded by vertices is all of the surface): the whole sphere, its loop a meridian
    /// from the south pole to the north and back, which [`Assembly::complete_slits`] keeps as its
    /// seam. The sphere is about `origin`, `axis` its north, `reference` its meridian's plane. Any
    /// other face without an edge loop is refused.
    pub(crate) fn whole_sphere(&mut self, origin: P3, axis: P3, reference: P3, radius: f64, loops: &[Bound]) -> R<Vec<Bound>> {
        let unsupported_here = || unsupported("a face without an edge loop");
        let (o, z, x) = (v3(origin), v3(axis), v3(reference));
        let poles = [o - z * radius, o + z * radius];
        let mut at = [None, None];
        for l in loops {
            let Bound::Vertex(v) = l else { return Err(unsupported_here()) };
            let p = v3(self.point(*v)?);
            let tol = self.vertices.get(*v as usize).map_or(self.tol, |x| x.tolerance.max(self.tol));
            let k = poles.iter().position(|q| q.dist(p) <= tol).ok_or_else(unsupported_here)?;
            if let Some(slot) = at.get_mut(k) {
                *slot = Some(*v);
            }
        }
        let mut ends = [0u32; 2];
        for ((end, found), pole) in ends.iter_mut().zip(at).zip(poles) {
            *end = match found {
                Some(v) => v,
                None => self.add_vertex([pole.x, pole.y, pole.z])?,
            };
        }
        let e = index(self.edges.len())?;
        let frame = Frame { origin, x_axis: [x.x, x.y, x.z], y_axis: [z.x, z.y, z.z] };
        let half = std::f64::consts::FRAC_PI_2;
        self.edges.push(EdgeB {
            curve: BCurve::Circle { frame, radius },
            interval: [-half, half],
            vertices: ends,
            tolerance: self.tol,
            pcurve_tolerance: self.pcurve_tolerance(),
            form: None,
        });
        Ok(vec![Bound::Edges(vec![(e, true), (e, false)])])
    }

    /// An edge between vertices `ends` on `curve` where the producer gives no interval: the
    /// interval is where the vertices lie on the curve (the whole of a closed curve when both
    /// ends are one vertex or one point), the edge running from `ends[0]` along the curve, as a
    /// STEP edge is read. Then as [`Assembly::add_edge`].
    pub fn add_edge_on(&mut self, origin: u64, curve: BCurve, ends: [u32; 2]) -> R<(u32, bool)> {
        let (ps, pe) = (self.point(ends[0])?, self.point(ends[1])?);
        let closed = ends[0] == ends[1];
        let full = closed || ps == pe;
        let (curve, interval, with) =
            crate::topology::record_edge(curve, ps, pe, closed, full, self.tol).map_err(|m| format!("edge #{origin}: {m}"))?;
        let (e, along) = self.add_edge(origin, curve, interval, if with { ends } else { [ends[1], ends[0]] })?;
        Ok((e, along == with))
    }

    /// A face on `surface` bounded by `bounds`, its index. Each bound is relative to the
    /// surface's natural normal; `outer` is the outer bound's index in `bounds` when the reader
    /// knows it (the pcurves decide otherwise, [`Assembly::finish`]); `forward` says the face's
    /// side is the natural normal. A spline surface whose first and last control lines meet
    /// within the tolerance is closed on itself (noted), a side that closes to a point within it is
    /// made an exact pole (noted), and a control net that turns back on itself within it is
    /// unfolded (noted). `origin` names the face.
    pub fn add_face(&mut self, origin: u64, surface: BSurface, bounds: Vec<Bound>, outer: Option<usize>, forward: bool) -> R<u32> {
        // A sphere bounded by its poles alone is the whole sphere, with a meridian for its seam.
        let bounds = match &surface {
            BSurface::Sphere { frame, radius } if !bounds.iter().any(|b| matches!(b, Bound::Edges(_))) => {
                let (x, y) = (v3(frame.x_axis), v3(frame.y_axis));
                let z = x.cross(y);
                self.whole_sphere(frame.origin, [z.x, z.y, z.z], frame.x_axis, *radius, &bounds)?
            }
            _ => bounds,
        };
        let mut surface = match crate::topology::closed_within(&surface, self.tol) {
            Some(closed) => {
                self.splines_closed += 1;
                closed
            }
            None => surface,
        };
        // A side only close to a point is an exact pole before anything is measured on it.
        let corners: Vec<Vec3> = bounds
            .iter()
            .flat_map(|b| match b {
                Bound::Edges(es) => es.iter().filter_map(|(e, _)| self.edges.get(*e as usize)).flat_map(|e| e.vertices).collect::<Vec<u32>>(),
                Bound::Vertex(v) => vec![*v],
            })
            .filter_map(|v| self.vertices.get(v as usize).map(|p| v3(p.point)))
            .collect();
        if let Some((poled, sides)) = crate::topology::poles_within(&surface, self.tol, &corners) {
            self.poles_made_exact += sides;
            surface = poled;
        }
        // A control net that turns back on itself within the tolerance is unfolded.
        if let Some((unfolded, moved)) = crate::topology::unfolded_surface(&surface, self.tol) {
            self.points_unfolded += moved;
            surface = unfolded;
        }
        if bounds.len() == 2
            && let BSurface::Sphere { frame, .. } = &mut surface
        {
            let edges: Vec<&EdgeB> = bounds
                .iter()
                .filter_map(|b| match b {
                    Bound::Edges(es) => Some(es),
                    Bound::Vertex(_) => None,
                })
                .flatten()
                .filter_map(|(e, _)| self.edges.get(*e as usize))
                .collect();
            if let Some(EdgeB { curve: BCurve::Circle { frame: rim, .. }, .. }) = edges.first() {
                // Coaxial circular bounds are latitudes of the same sphere. Put its poles
                // on their axis so an arbitrary placement cannot put a pole on a rim.
                let aligned = Frame { origin: frame.origin, ..rim.clone() };
                if edges.iter().all(|e| crate::kernel::form::on_frame(&e.curve, e.interval, &aligned, self.tol).is_some()) {
                    *frame = aligned;
                }
            }
        }
        let i = index(self.faces.len())?;
        self.faces.push(FaceB { origin, surface, loops: bounds, outer, forward, extent: None });
        Ok(i)
    }
    /// Points along a face's bounds (each edge at nine points, and its vertex bounds), along the
    /// edges its bounds use twice (its seams), and each edge bound walked as it runs: where the
    /// face lies on a surface whose record form depends on it ([`Assembly::swept_surface`]).
    pub(crate) fn extent(&self, loops: &[Bound]) -> R<Extent> {
        let mut ext = Extent::default();
        let mut count: HashMap<u32, usize> = HashMap::new();
        for l in loops {
            match l {
                Bound::Edges(es) => {
                    let mut walk = Vec::new();
                    for &(e, forward) in es {
                        *count.entry(e).or_default() += 1;
                        let edge = self.edges.get(e as usize).ok_or("edge reference")?;
                        let c = CurveEval::new(&edge.curve).map_err(|m| m.to_string())?;
                        let [lo, hi] = edge.interval;
                        for k in 0..8 {
                            let s = f64::from(k) / 8.0;
                            walk.push(c.point(if forward { lo + (hi - lo) * s } else { hi - (hi - lo) * s }).ok_or("edge point")?);
                        }
                    }
                    if let Some(&p) = walk.first() {
                        walk.push(p);
                    }
                    ext.walks.push(walk);
                }
                Bound::Vertex(v) => ext.points.push(v3(self.point(*v)?)),
            }
        }
        let mut edges: Vec<u32> = count.keys().copied().collect();
        edges.sort_unstable();
        for e in edges {
            let edge = self.edges.get(e as usize).ok_or("edge reference")?;
            let c = CurveEval::new(&edge.curve).map_err(|m| m.to_string())?;
            let [lo, hi] = edge.interval;
            for k in 0..=8 {
                let p = c.point(lo + (hi - lo) * f64::from(k) / 8.0).ok_or("edge point")?;
                ext.points.push(p);
                if count.get(&e).is_some_and(|n| *n > 1) {
                    ext.seam.push(p);
                }
            }
        }
        Ok(ext)
    }

    /// The record surface of a surface whose record form depends on the face it bounds
    /// ([`SweptSurface`]), for a face with `bounds` (the edges and vertices already added, the
    /// bounds as the reader has them, either way round), and whether its natural normal is
    /// opposite the definition's own (stated on each kind). The reader then adds the face on it
    /// ([`Assembly::add_face`]) with its bounds and side relative to that natural normal. A
    /// sweep or revolution's profile is bounded where the face's boundary needs it, and a cone
    /// that stays within the tolerance of the plane across its axis over the boundary is that
    /// plane. The STEP reader reads its swept surfaces and cones by the same code.
    pub fn swept_surface(&self, surface: &SweptSurface, bounds: &[Bound]) -> R<(BSurface, bool)> {
        crate::topology::swept::map_neutral(surface, &self.extent(bounds)?, self.tol)
    }

    pub(crate) fn point(&self, v: u32) -> R<P3> {
        self.vertices.get(v as usize).map(|v| v.point).ok_or_else(|| "vertex reference".to_string())
    }

    /// A vertex `gap` mm from where its edge's curve puts it: refused if far off, else its
    /// tolerance covers the gap.
    pub(crate) fn raise_vertex(&mut self, v: u32, gap: f64, edge: u64) -> R<()> {
        if !(VERTEX_GAP_MARGIN * gap <= MAX_TOLERANCE) {
            return Err(format!("edge #{edge}: a vertex lies {gap} mm off its curve"));
        }
        if let Some(x) = self.vertices.get_mut(v as usize) {
            x.tolerance = x.tolerance.max(VERTEX_GAP_MARGIN * gap);
        }
        Ok(())
    }

    /// An edge's curve as the file has it may lie off a face's surface by more than the file's
    /// uncertainty states (a B-spline edge whose control points stand 9 µm off its plane, in a
    /// file that states 10 µm, gets no closed-form pcurve, which is checked within half that).
    /// On a surface whose nearest point is in closed form (a plane, cylinder, cone, sphere or
    /// torus) the edge's distance from it is measured along the edge before its pcurve is made,
    /// and the edge's tolerance covers it. A spline face's gaps are measured on the pcurves its
    /// kernel makes (`raise_edges`): an iterative nearest point may settle elsewhere on it.
    ///
    /// `form_box` is the face's box on its NURBS form where it has one (`form::surface_within`).
    /// The form ends on a quarter line its boundary runs past by up to the file's tolerance, so
    /// an edge that runs past lies off the face as the record keeps it (the label over the
    /// form's box) by more than off the label: that gap is measured too, to the label's point at
    /// the nearest parameters in the box, and the edge's pcurve tolerance covers it, as it covers
    /// a gap to the label ([`Self::cover_edge`]), up to the file's tolerance: the form ends short
    /// of the boundary by no more than that, and pcurves are never made looser than the file
    /// states. Its closed-form pcurve then has its ends put in the box, which the pcurves'
    /// construction asks for; the record's tolerances cover the move as before (`onto_form`,
    /// `raise_edges`).
    fn measure_edges(&mut self, surface: &BSurface, edge_loops: &[Vec<(u32, bool)>], form_box: Option<[[f64; 2]; 2]>) -> R<()> {
        // Only where the nearest point is in closed form: on a spline the search may settle on
        // another part of the surface, or on its boundary for a point a hair past it, and report a
        // gap the edge does not have (2.9 mm on Poppy Humanoid's leg shells, whose edges lie within
        // 0.003 mm), widening tolerances past what SMLib takes. A spline face's gaps are measured
        // on the pcurves its kernel makes (`raise_edges`).
        if !matches!(
            surface,
            BSurface::Plane { .. }
                | BSurface::PlaneThrough { .. }
                | BSurface::Cylinder { .. }
                | BSurface::Cone { .. }
                | BSurface::Sphere { .. }
                | BSurface::Torus { .. }
        ) {
            return Ok(());
        }
        let on = SurfaceEval::new(surface).map_err(|m| m.to_string())?;
        let starts = crate::kernel::Starts::default();
        let period = on.domain().period;
        // The form's box in the label's angles, along the directions that are angles (a cylinder's
        // or cone's height is the form's own parameter, and its form does not end short of it).
        let angular = match surface {
            BSurface::Cylinder { .. } | BSurface::Cone { .. } => [true, false],
            BSurface::Sphere { .. } | BSurface::Torus { .. } => [true, true],
            _ => [false, false],
        };
        let bounds = form_box.map(|d| {
            [0, 1].map(|k| match (angular.get(k), d.get(k)) {
                (Some(true), Some(&[a, b])) => Some([crate::kernel::form::angle(a), crate::kernel::form::angle(b)]),
                _ => None,
            })
        });
        // `q` moved to the nearest parameters in the box, where it lies past it.
        let in_box = |q: [f64; 2]| -> Option<[f64; 2]> {
            let bounds = bounds.as_ref()?;
            let mut c = q;
            let mut moved = false;
            for ((x, range), p) in c.iter_mut().zip(bounds).zip(period) {
                let Some([lo, hi]) = *range else { continue };
                let y = crate::kernel::unwrap(*x, 0.5 * (lo + hi), p);
                let z = if y < lo {
                    lo
                } else if y > hi {
                    hi
                } else {
                    y
                };
                if z != y {
                    (*x, moved) = (z, true);
                }
            }
            moved.then_some(c)
        };
        for l in edge_loops {
            for &(e, _) in l {
                let edge = self.edges.get(e as usize).ok_or("edge reference")?;
                let (curve, [t0, t1]) = match &edge.form {
                    Some((c, i)) => (c, *i),
                    None => (&edge.curve, edge.interval),
                };
                let eval = CurveEval::new(curve).map_err(|m| m.to_string())?;
                let (mut gap, mut off_form) = (0.0f64, 0.0f64);
                for t in gap_samples(curve, [t0, t1], matches!(surface, BSurface::Plane { .. } | BSurface::PlaneThrough { .. })) {
                    let Some(p) = eval.point(t) else { continue };
                    // A point with no nearest point is the pcurves' to refuse, with why.
                    let Some(q) = on.project_with(&starts, p, None, f64::INFINITY) else { continue };
                    let Some(s) = on.point(q) else { continue };
                    gap = gap.max(s.dist(p));
                    if let Some(s) = in_box(q).and_then(|c| on.point(c)) {
                        off_form = off_form.max(s.dist(p));
                    }
                }
                self.cover_edge(e, gap, pcurve::SURFACE_GAP_MARGIN)?;
                let allowance = (pcurve::SURFACE_GAP_MARGIN * off_form).min(self.tol);
                if let Some(x) = self.edges.get_mut(e as usize) {
                    x.pcurve_tolerance = x.pcurve_tolerance.max(allowance);
                }
            }
        }
        Ok(())
    }

    /// Edge `e`'s tolerance covers `gap` with `margin`: refused if far off.
    fn cover_edge(&mut self, e: u32, gap: f64, margin: f64) -> R<()> {
        let Some(x) = self.edges.get_mut(e as usize) else { return Err("edge reference".into()) };
        // A real gap must be covered even when it is smaller than the file's uncertainty.
        x.pcurve_tolerance = x.pcurve_tolerance.max(margin * gap);
        if margin * gap <= x.tolerance {
            return Ok(());
        }
        if !(margin * gap <= MAX_TOLERANCE) {
            return Err(format!("an edge lies {gap} mm off the face's surface"));
        }
        x.tolerance = margin * gap;
        let (tolerance, ends) = (x.tolerance, x.vertices);
        self.raised_edges.insert(e);
        // A vertex's tolerance covers every edge that meets it, as in any tolerant record.
        for v in ends {
            if let Some(vx) = self.vertices.get_mut(v as usize) {
                vx.tolerance = vx.tolerance.max(tolerance);
            }
        }
        Ok(())
    }

    /// The edges and vertices of `bound` hold `deviation` (mm): how far, as their producer states
    /// it, the curves or the surface they stand on may lie from what they stand for (a sampled
    /// curve's chordal deflection, a fitted cache's fit tolerance), so the record keeps that
    /// allowance instead of taking the fit for the exact geometry. More than [`MAX_TOLERANCE`]
    /// is refused, as a gap that large is.
    pub fn hold_deviation(&mut self, bound: &Bound, deviation: f64) -> R<()> {
        if !(deviation <= MAX_TOLERANCE) {
            return Err(format!("a fit stated within {deviation} mm, more than an edge's tolerance holds"));
        }
        let mut vertices = Vec::new();
        match bound {
            Bound::Edges(uses) => {
                for &(e, _) in uses {
                    let Some(x) = self.edges.get_mut(e as usize) else { return Err("edge reference".into()) };
                    x.tolerance = x.tolerance.max(deviation);
                    x.pcurve_tolerance = x.pcurve_tolerance.max(deviation);
                    vertices.extend(x.vertices);
                }
            }
            Bound::Vertex(v) => vertices.push(*v),
        }
        // A vertex's tolerance covers every edge that meets it, as in any tolerant record.
        for v in vertices {
            let Some(x) = self.vertices.get_mut(v as usize) else { return Err("vertex reference".into()) };
            x.tolerance = x.tolerance.max(deviation);
        }
        Ok(())
    }

    /// An edge's curve as the file has it may lie off a face's surface by more than the file's
    /// uncertainty states (an intersection curve written looser than its context). Each use's
    /// gap, between its pcurve on the surface and its edge's curve, is measured along the edge as
    /// the record check measures it, and the edge's tolerance covers it, as a vertex's covers its
    /// gap: the pcurves are as made and the record says how far its geometry agrees.
    fn raise_edges(&mut self, surface: &BSurface, edge_loops: &[Vec<(u32, bool)>], pcs: &[Vec<Curve2>]) -> R<()> {
        let on = SurfaceEval::new(surface).map_err(|m| m.to_string())?;
        for (l, pc) in edge_loops.iter().zip(pcs) {
            for (&(e, _), c) in l.iter().zip(pc) {
                let edge = self.edges.get(e as usize).ok_or("edge reference")?;
                let (curve, [t0, t1]) = match &edge.form {
                    Some((c, i)) => (c, *i),
                    None => (&edge.curve, edge.interval),
                };
                let eval = CurveEval::new(curve).map_err(|m| m.to_string())?;
                let mut gap = 0.0f64;
                for k in 0..=EDGE_GAP_SAMPLES {
                    let t = if k == EDGE_GAP_SAMPLES { t1 } else { t0 + (t1 - t0) * (k as f64 / EDGE_GAP_SAMPLES as f64) };
                    // A point a pcurve or surface lacks is the record check's to name.
                    let (Some(q), Some(p)) = (solidcore_ops::pcurve::eval(c, t), eval.point(t)) else { continue };
                    if let Some(s) = on.point(q) {
                        gap = gap.max(s.dist(p));
                    }
                }
                self.cover_edge(e, gap, EDGE_GAP_MARGIN)?;
            }
        }
        Ok(())
    }

    /// Pcurves, outer loops, domains and the canonical order: the finished record of body item
    /// `item` named `name`, whose shells are `shells` (each a list of faces and whether the shell
    /// uses each on its side), making a body of `kind`.
    ///
    /// A body with faces narrower than the tolerance ([`slivers`]) is read with them first; where
    /// that is refused, the slivers the refusal names (each refused on its own: such a face has
    /// no area in its own parameters) are left out, their sides made one, and it is read again. A
    /// sliver the refusal does not name reads, and is kept; a refusal that names none stands.
    pub fn finish(mut self, item: u64, name: String, shells: Vec<Vec<(u32, bool)>>, kind: Kind) -> R<Solid> {
        if !self.has_slivers()? {
            return self.finish_as_given(item, name, shells, kind, &mut Vec::new());
        }
        let mut named = Vec::new();
        let refusal = match self.clone().finish_as_given(item, name.clone(), shells.clone(), kind, &mut named) {
            Ok(solid) => return Ok(solid),
            Err(m) => m,
        };
        let (shells, slivers) = self.collapse_slivers(shells, &named)?;
        if slivers == 0 {
            return Err(refusal);
        }
        let note = format!("{slivers} faces narrower than {} mm left out, their two sides made one edge", self.tol);
        let mut solid = self.finish_as_given(item, name, shells, kind, &mut Vec::new())?;
        solid.notes.insert(0, note);
        Ok(solid)
    }

    /// [`Self::finish`] with the faces as the reader gave them. The file faces a refusal names
    /// (each whose pcurves failed, up to [`MAX_NAMED_FAILURES`], or the one the record check
    /// disagrees on) are added to `named`.
    fn finish_as_given(mut self, item: u64, name: String, shells: Vec<Vec<(u32, bool)>>, kind: Kind, named: &mut Vec<u64>) -> R<Solid> {
        let mut notes = Vec::new();
        let mut shells = shells;
        let hairpins = self.drop_hairpin_faces(&mut shells)?;
        if hairpins > 0 {
            notes.push(format!("{hairpins} walls no wider than {} mm left out (an outline running out and back along itself)", self.tol));
        }
        let twins = self.merge_twin_edges()?;
        if twins > 0 {
            notes.push(format!("{twins} edges the file writes twice (two edges on the same vertices and one curve, each used by one face) made one"));
        }
        let paired = self.pair_slit_edges()?;
        if paired > 0 {
            notes.push(format!("{paired} loops of no area written on two coincident edges rewritten to run out and back on one"));
        }
        // A face bounded by a slit alone is the whole of its closed surface, closed by its seams
        // (a sphere's frame put on the slit), before anything is measured on the frame or split
        // at its poles.
        self.complete_slits()?;
        // A face on a sphere or torus that is the rest of it round a patch its loop bounds gets
        // the surface's seams, its loop a hole in it.
        let outsides = self.complete_outsides()?;
        if outsides > 0 {
            notes.push(format!("{outsides} faces on a sphere or torus that are the outside of their loops given the surface's seams"));
        }
        // A torus face's seams where its torus starts, so its (u, v) box is read whole.
        self.seat_torus_seams()?;
        let slits = self.drop_inner_slits();
        if slits > 0 {
            notes.push(format!("{slits} slits of no area left out of the faces they lie in"));
        }
        let at_poles = self.split_at_poles()?;
        if at_poles > 0 {
            notes.push(format!("{at_poles} edges split where they pass through a face's pole"));
        }
        self.start_splines_clear_of_faces()?;
        let seams = self.add_seams()?;
        if seams > 0 {
            notes.push(format!("{seams} seam edges added to faces that wind around their surface"));
        }
        let long = self.split_long_wound_edges()?;
        if long > 0 {
            notes.push(format!("{long} edges of faces that wind round their axis split so each one's pcurve fits in a spline"));
        }
        if self.splines_closed > 0 {
            notes.push(format!(
                "{} spline surfaces whose ends meet within {} mm closed on themselves (their end control lines made one)",
                self.splines_closed, self.tol
            ));
        }
        if self.points_unfolded > 0 {
            notes.push(format!(
                "{} control points of spline curves and surfaces whose polygon turned back on itself by less than {} mm moved to take the turn out",
                self.points_unfolded, self.tol
            ));
        }
        if self.poles_made_exact > 0 {
            notes.push(format!(
                "{} sides of spline surfaces that close to a point within {} mm made exact poles (their control points made one)",
                self.poles_made_exact, self.tol
            ));
        }
        if self.lines_between_vertices > 0 {
            notes.push(format!("{} line edges whose vertices lie off their LINE read as the segment between them", self.lines_between_vertices));
        }
        if self.edges_turned > 0 {
            notes.push(format!("{} edges whose vertices lie at the other ends of their intervals turned round", self.edges_turned));
        }
        if self.sense_mismatches > 0 {
            notes.push(format!("{} edges run against their same_sense (read by their vertices)", self.sense_mismatches));
        }
        // Round faces whose forms would run past one turn start on a quarter line of their
        // frames, before their edges are labelled on them.
        self.turn_frames()?;
        // Each edge's label and NURBS form: a circle about a revolved face's axis labelled on that
        // face's frame, so its form lies on the face's quarter grid.
        // A circle round a torus face's tube is labelled from its tube start likewise.
        let mut frames: Vec<Vec<brep::Frame>> = vec![Vec::new(); self.edges.len()];
        let mut tori: Vec<Vec<BSurface>> = vec![Vec::new(); self.edges.len()];
        for f in &self.faces {
            let Some(frame) = crate::kernel::form::frame_of(&f.surface) else { continue };
            let torus = matches!(f.surface, BSurface::Torus { .. });
            for l in &f.loops {
                if let Bound::Edges(es) = l {
                    for (e, _) in es {
                        if let Some(slot) = frames.get_mut(*e as usize) {
                            slot.push(frame.clone());
                        }
                        if torus && let Some(slot) = tori.get_mut(*e as usize) {
                            slot.push(f.surface.clone());
                        }
                    }
                }
            }
        }
        for ((edge, fr), tr) in self.edges.iter_mut().zip(&frames).zip(&tori) {
            let (label, interval, form) = crate::kernel::form::edge_on(&edge.curve, edge.interval, fr, tr, self.tol)
                .ok_or_else(|| unsupported("an arc of more than one turn"))?;
            (edge.curve, edge.interval, edge.form) = (label, interval, form);
        }
        // Each face's pcurves; a face whose loops disagree with its same_sense is turned over.
        let mut pcs: Vec<Vec<Vec<Curve2>>> = Vec::with_capacity(self.faces.len());
        let mut face_forms: Vec<Option<BSurface>> = Vec::with_capacity(self.faces.len());
        let mut domains = Vec::with_capacity(self.faces.len());
        let mut turned = vec![false; self.faces.len()];
        let (mut failed, mut first_failure, mut failed_faces) = (0usize, None, Vec::new());
        for fi in 0..self.faces.len() {
            match self.face_pcurves(fi) {
                Ok((pc, domain, flipped, form)) => {
                    face_forms.push(form);
                    if flipped && let Some(t) = turned.get_mut(fi) {
                        *t = true;
                    }
                    pcs.push(pc);
                    domains.push(domain);
                }
                Err(m) => {
                    failed += 1;
                    let origin = self.faces.get(fi).map_or(0, |f| f.origin);
                    if failed_faces.len() < MAX_NAMED_FAILURES {
                        failed_faces.push(format!("face #{origin}"));
                        named.push(origin);
                    }
                    first_failure.get_or_insert_with(|| format!("face #{origin}: {m}"));
                    pcs.push(Vec::new());
                    domains.push([[0.0; 2]; 2]);
                    face_forms.push(None);
                }
            }
        }
        if let Some(first) = first_failure {
            // Every face that failed is named (up to `MAX_NAMED_FAILURES`), the first first.
            let named = if failed > 1 { format!(" ({})", failed_faces.join(", ")) } else { String::new() };
            return Err(format!("pcurves failed on {failed} of {} faces{named}; first: {first}", self.faces.len()));
        }
        if !self.reseamed.is_empty() {
            notes.push(format!("{} closed spline patches started elsewhere, off the face across their seam", self.reseamed.len()));
        }
        if self.patches_extended > 0 {
            notes.push(format!("{} spline patches continued past their knot range to their faces' trims", self.patches_extended));
        }
        if self.inner_loops_turned > 0 {
            notes.push(format!("{} inner loops written the way their outer loop runs turned round", self.inner_loops_turned));
        }
        if self.sides_in_vertices > 0 {
            notes.push(format!(
                "{} sides of spline faces the file wrote as one vertex run along, the vertex's tolerance raised to reach along them",
                self.sides_in_vertices
            ));
        }
        if !self.raised_edges.is_empty() {
            let most = self.raised_edges.iter().filter_map(|&e| self.edges.get(e as usize)).map(|e| e.tolerance).fold(0.0, f64::max);
            notes.push(format!(
                "{} edges lie off their faces' surfaces by more than {} mm: their tolerances are the gaps they have (up to {most:.3e} mm)",
                self.raised_edges.len(),
                self.tol
            ));
        }
        if self.loops_joined > 0 {
            notes.push(format!(
                "{} loops that touch another loop of their face at one vertex (a hole that touches its face's edge) made one loop with it",
                self.loops_joined
            ));
        }
        if let Some((origin, [x, y, z])) = self.loops_left_touching.first() {
            let more = match self.loops_left_touching.len() {
                0 | 1 => String::new(),
                n => format!(", and {} more faces", n - 1),
            };
            notes.push(format!(
                "face #{origin}: its loops touch at ({x}, {y}, {z}) mm and are left as written{more}: they make no single loop through that point (three or more of their edges' ends meet there, or the loops touch at another point as well). A face whose loops share a vertex fails the body's check, so features on the body are refused until the loops are moved apart in the source model"
            ));
        }
        let flipped = turned.iter().filter(|t| **t).count();
        if flipped > 0 {
            notes.push(format!("{flipped} faces turned over: their loops disagree with their same_sense"));
        }
        let mut shells: Vec<Vec<(u32, bool)>> = shells
            .into_iter()
            .map(|s| s.into_iter().map(|(f, fwd)| (f, if turned.get(f as usize).copied().unwrap_or(false) { !fwd } else { fwd })).collect())
            .collect();

        // Regions: which shells are closed (every edge used at least twice among its faces).
        let closed: Vec<bool> = shells.iter().map(|s| self.closed(s)).collect();
        let kind = match kind {
            Kind::General if closed.len() > 1 && closed.iter().all(|c| *c) => Kind::Voids,
            Kind::General => Kind::Model,
            k => k,
        };
        // A closed solid's edge sides must cancel. Local loop normalization alone can
        // preserve an exporter's inconsistent shell sides (#17149).
        let mut reoriented = false;
        if matches!(kind, Kind::Solid | Kind::Voids) {
            for (shell, closed) in shells.iter_mut().zip(&closed) {
                if *closed {
                    reoriented |= self.orient_shell(shell)?;
                }
            }
        }
        // A void's faces face into the cavity, out of the material: it encloses volume of the
        // opposite sign to the outer shell's. The ORIENTED_CLOSED_SHELL flag alone doesn't say
        // so: some exporters write .F. over a shell already facing into the cavity (#3730),
        // SolidCraft .T.; read by the flag, such a void added its cavity's volume to the solid's.
        // A void enclosing volume of the outer shell's sign is turned inside out, the signs the
        // faces' own surfaces and pcurves give (`face_flux` of p / 3, the divergence theorem).
        let shells = if !reoriented && kind == Kind::Voids && closed.iter().all(|c| *c) {
            let volume = |s: &[(u32, bool)]| -> Option<f64> {
                let mut sum = 0.0;
                for &(f, forward) in s {
                    let face = self.faces.get(f as usize)?;
                    let on = face_forms.get(f as usize)?.as_ref().unwrap_or(&face.surface);
                    let eval = SurfaceEval::new(on).ok()?;
                    let edge_loops = face.loops.iter().filter_map(|l| match l {
                        Bound::Edges(es) => Some(es),
                        Bound::Vertex(_) => None,
                    });
                    let uses: Vec<Vec<(&Curve2, bool)>> =
                        edge_loops.zip(pcs.get(f as usize)?).map(|(es, pc)| pc.iter().zip(es).map(|(c, (_, fwd))| (c, *fwd)).collect()).collect();
                    let flux = face_flux(&eval, domains.get(f as usize)?[1][0], &uses, &|p| p * (1.0 / 3.0))?;
                    sum += if forward { flux } else { -flux };
                }
                sum.is_finite().then_some(sum)
            };
            let outside = shells.first().and_then(|s| volume(s));
            shells
                .into_iter()
                .enumerate()
                .map(|(k, s)| match (outside, k > 0) {
                    (Some(o), true) if volume(&s).is_some_and(|v| v * o > 0.0) => s.into_iter().map(|(f, fwd)| (f, !fwd)).collect(),
                    _ => s,
                })
                .collect()
        } else {
            shells
        };
        let mut regions = vec![Region { shells: Vec::new(), void: false }];
        match kind {
            Kind::Solid | Kind::Voids => {
                if let Some(k) = closed.iter().position(|c| !c) {
                    return Err(format!("{} shell is not closed", if k == 0 { "the outer" } else { "a void" }));
                }
                if let Some(r) = regions.first_mut() {
                    r.shells.push(0);
                }
                regions.push(Region { shells: (0..index(shells.len())?).collect(), void: false });
                for v in 1..shells.len() {
                    regions.push(Region { shells: vec![index(v)?], void: true });
                }
            }
            Kind::Open { voids, outer } => {
                if let Some(r) = regions.first_mut() {
                    r.shells = (0..index(shells.len())?).collect();
                }
                // A closed shell bounds two regions: the cavity inside a void still whole, or
                // the inside of an outer shell or a model's shell still whole.
                for (k, c) in closed.iter().enumerate() {
                    if *c {
                        regions.push(Region { shells: vec![index(k)?], void: voids && (k > 0 || !outer) });
                    }
                }
            }
            Kind::Model | Kind::General => {
                if let Some(r) = regions.first_mut() {
                    r.shells = (0..index(shells.len())?).collect();
                }
                for (k, c) in closed.iter().enumerate() {
                    if *c {
                        regions.push(Region { shells: vec![index(k)?], void: false });
                    }
                }
            }
        }
        let sheet = matches!(kind, Kind::Open { .. }) || !closed.iter().any(|c| *c);

        // Canonical order: faces walking regions → shells → oriented faces.
        let nf = self.faces.len();
        let mut face_new = vec![u32::MAX; nf];
        let mut face_order = Vec::with_capacity(nf);
        let mut direction = vec![true; nf];
        let mut seen_shell = vec![false; shells.len()];
        for r in &regions {
            for &s in &r.shells {
                let Some(seen) = seen_shell.get_mut(s as usize) else { continue };
                if std::mem::replace(seen, true) {
                    continue;
                }
                for &(f, fwd) in shells.get(s as usize).into_iter().flatten() {
                    let slot = face_new.get_mut(f as usize).ok_or("face reference")?;
                    if *slot == u32::MAX {
                        *slot = index(face_order.len())?;
                        face_order.push(f);
                        if let Some(d) = direction.get_mut(f as usize) {
                            *d = fwd;
                        }
                    }
                }
            }
        }
        // Edges walking faces → loops (outer first) → oriented edges, backwards in a face whose
        // first side is reversed; their uses in that order.
        let outer_first: Vec<Vec<usize>> = (0..nf).map(|f| self.loop_order(f)).collect();
        let mut edge_new = vec![u32::MAX; self.edges.len()];
        let mut edge_order = Vec::with_capacity(self.edges.len());
        let mut uses: Vec<Vec<EdgeUseRef>> = vec![Vec::new(); self.edges.len()];
        for (fnew, &f) in face_order.iter().enumerate() {
            let face = self.faces.get(f as usize).ok_or("face reference")?;
            let forward = direction.get(f as usize).copied().unwrap_or(true);
            for (lnew, &l) in outer_first.get(f as usize).into_iter().flatten().enumerate() {
                let Some(Bound::Edges(es)) = face.loops.get(l) else { continue };
                let n = es.len();
                for k in 0..n {
                    let p = if forward { k } else { n - 1 - k };
                    let &(e, fwd) = es.get(p).ok_or("edge reference")?;
                    let slot = edge_new.get_mut(e as usize).ok_or("edge reference")?;
                    if *slot == u32::MAX {
                        *slot = index(edge_order.len())?;
                        edge_order.push(e);
                    }
                    let at = EdgeUseRef { face: index(fnew)?, r#loop: index(lnew)?, position: index(p)?, upper: !fwd };
                    uses.get_mut(e as usize).ok_or("edge reference")?.push(at);
                }
            }
        }
        // Vertices walking edges (start, end), then vertex loops.
        let mut vertex_new = vec![u32::MAX; self.vertices.len()];
        let mut vertex_order = Vec::with_capacity(self.vertices.len());
        let mut visit = |v: u32, vertex_new: &mut Vec<u32>| -> R<u32> {
            let slot = vertex_new.get_mut(v as usize).ok_or("vertex reference")?;
            if *slot == u32::MAX {
                *slot = index(vertex_order.len())?;
                vertex_order.push(v);
            }
            Ok(*slot)
        };
        for &e in &edge_order {
            let edge = self.edges.get(e as usize).ok_or("edge reference")?;
            for v in edge.vertices {
                visit(v, &mut vertex_new)?;
            }
        }
        for &f in &face_order {
            for &l in outer_first.get(f as usize).into_iter().flatten() {
                if let Some(Bound::Vertex(v)) = self.faces.get(f as usize).and_then(|x| x.loops.get(l)) {
                    visit(*v, &mut vertex_new)?;
                }
            }
        }
        let vmap = |v: u32| vertex_new.get(v as usize).copied().filter(|x| *x != u32::MAX).ok_or("vertex reference".to_string());
        let emap = |e: u32| edge_new.get(e as usize).copied().filter(|x| *x != u32::MAX).ok_or("edge reference".to_string());
        let fmap = |f: u32| face_new.get(f as usize).copied().filter(|x| *x != u32::MAX).ok_or("face reference".to_string());
        let mut turned_faces = Vec::with_capacity(flipped);
        for (f, t) in turned.iter().enumerate() {
            if *t {
                turned_faces.push(fmap(index(f)?)?);
            }
        }
        turned_faces.sort_unstable();

        // Vertex loops: their vertex's distance from the surface is its tolerance.
        for &f in &face_order {
            let Some(face) = self.faces.get(f as usize) else { continue };
            let surface = SurfaceEval::new(&face.surface).map_err(|m| m.to_string())?;
            for l in &face.loops {
                if let Bound::Vertex(v) = l
                    && let Some(x) = self.vertices.get_mut(*v as usize)
                {
                    let p = v3(x.point);
                    let gap = surface.project(p, None, f64::INFINITY).and_then(|q| surface.point(q)).map_or(f64::INFINITY, |s| s.dist(p));
                    if !(VERTEX_GAP_MARGIN * gap <= MAX_TOLERANCE) {
                        return Err(format!("face #{}: a vertex loop's vertex lies {gap} mm off its surface", face.origin));
                    }
                    x.tolerance = x.tolerance.max(VERTEX_GAP_MARGIN * gap);
                }
            }
        }

        let mut vertices = Vec::with_capacity(vertex_order.len());
        for &v in &vertex_order {
            vertices.push(self.vertices.get(v as usize).cloned().ok_or("vertex reference")?);
        }
        let mut edges = Vec::with_capacity(edge_order.len());
        for &e in &edge_order {
            let edge = self.edges.get(e as usize).ok_or("edge reference")?;
            let (nurbs, interval) = match &edge.form {
                Some((c, i)) => (Some(c.clone()), *i),
                None => (None, edge.interval),
            };
            edges.push(Edge {
                nurbs,
                curve: edge.curve.clone(),
                interval,
                vertices: [vmap(edge.vertices[0])?, vmap(edge.vertices[1])?],
                tolerance: edge.tolerance,
                uses: uses.get(e as usize).cloned().unwrap_or_default(),
            });
        }
        let mut faces = Vec::with_capacity(face_order.len());
        let mut face_origin = Vec::with_capacity(face_order.len());
        for &f in &face_order {
            let face = self.faces.get(f as usize).ok_or("face reference")?;
            face_origin.push(face.origin);
            let pc = pcs.get(f as usize).ok_or("face reference")?;
            let mut loops = Vec::with_capacity(face.loops.len());
            let mut edge_loop = 0;
            // pcurves come in the face's edge-loop order; loops go out outer first.
            let mut fitted: Vec<Option<&Vec<Curve2>>> = Vec::with_capacity(face.loops.len());
            for l in &face.loops {
                fitted.push(match l {
                    Bound::Edges(_) => {
                        edge_loop += 1;
                        pc.get(edge_loop - 1)
                    }
                    Bound::Vertex(_) => None,
                });
            }
            for &l in outer_first.get(f as usize).into_iter().flatten() {
                match face.loops.get(l) {
                    Some(Bound::Edges(es)) => {
                        let curves = fitted.get(l).copied().flatten().ok_or("pcurves")?;
                        let mut out = Vec::with_capacity(es.len());
                        for (&(e, forward), c) in es.iter().zip(curves) {
                            out.push(OrientedEdge { edge: emap(e)?, forward, pcurve: c.clone() });
                        }
                        loops.push(Loop::Edges { edges: out });
                    }
                    Some(Bound::Vertex(v)) => loops.push(Loop::Vertex { vertex: vmap(*v)? }),
                    None => return Err("loop reference".into()),
                }
            }
            let domain = *domains.get(f as usize).ok_or("face reference")?;
            let nurbs = face_forms.get(f as usize).cloned().flatten();
            faces.push(Face { nurbs, surface: face.surface.clone(), domain, loops });
        }
        let mut out_shells = Vec::with_capacity(shells.len());
        for s in &shells {
            let mut faces = Vec::with_capacity(s.len());
            for &(f, forward) in s {
                faces.push(OrientedFace { face: fmap(f)?, forward });
            }
            out_shells.push(Shell::Faces { faces, wires: Vec::new() });
        }
        // The file lists faces, not their radial order. At an edge more than two
        // faces use, order their inward directions on the actual shared segment.
        // Keep two-use edges in their existing order, including periodic seams.
        for edge in &mut edges {
            if edge.uses.len() <= 2 {
                continue;
            }
            let radial_faces = edge
                .uses
                .iter()
                .map(|u| {
                    let face = faces.get(u.face as usize).ok_or("radial face reference")?;
                    let Some(Loop::Edges { edges }) = face.loops.get(u.r#loop as usize) else {
                        return Err("radial loop reference");
                    };
                    let oriented = edges.get(u.position as usize).ok_or("radial edge use reference")?;
                    Ok((face.parameters(), &oriented.pcurve, oriented.forward))
                })
                .collect::<Result<Vec<_>, &str>>()?;
            let order = crate::kernel::radial_order(edge.parameters(), edge.interval, &radial_faces)?;
            edge.uses = order.into_iter().map(|i| edge.uses.get(i).copied().ok_or("radial edge use reference")).collect::<Result<Vec<_>, &str>>()?;
        }
        let mut brep = Brep { version: brep::VERSION, tolerance: self.tol, regions, shells: out_shells, faces, edges, vertices };
        if reoriented {
            // Parity fixes relative sides only. Certify every shell's absolute side,
            // including cavities: the traversal's starting face may itself be reversed.
            for shell in 0..brep.shells.len() {
                let positive = crate::kernel::shell_volume_positive(&brep, shell)?;
                let cavity = kind == Kind::Voids && shell > 0;
                if positive == cavity {
                    let Some(Shell::Faces { faces, .. }) = brep.shells.get_mut(shell) else { return Err("shell reference".into()) };
                    for side in faces {
                        side.forward = !side.forward;
                    }
                }
            }
            // Face order is unchanged; shell sides also determine the edge/vertex walk.
            brep = brep.canonical().map_err(|e| e.to_string())?.brep;
        }
        // Trim construction returns a continuous lift on a periodic surface. Only now, with
        // the whole body mutable, can its crossed cuts become edges and its trim pieces be
        // moved into the base period. An internal seam used on both sides is replaced rather
        // than retained as a boundary that would divide the same material into sliver faces.
        if let Some((completed, from)) = pcurve::complete_periodic_seams(&brep).map_err(|e| e.to_string())? {
            face_origin = from
                .iter()
                .map(|&i| face_origin.get(i).copied().ok_or_else(|| pcurve::PeriodicSeamError::InputFaceLost(i).to_string()))
                .collect::<R<Vec<_>>>()?;
            turned_faces =
                from.iter().enumerate().filter(|(_, i)| turned_faces.contains(&(**i as u32))).map(|(i, _)| index(i)).collect::<R<Vec<_>>>()?;
            brep = completed;
        }
        solidcore_ops::record_check::validate(&brep).map_err(|e| format!("the record is invalid: {e}"))?;
        // A disagreement on a face names the file's face too, so a reader can leave it out.
        solidcore_ops::record_check::agreement(&brep).map_err(|e| {
            use solidcore_ops::record_check::Record;
            if let Record::Face(f) | Record::Use { face: f, .. } | Record::VertexLoop { face: f, .. } | Record::Loop { face: f, .. } = e.record
                && let Some(o) = face_origin.get(f as usize)
            {
                named.push(*o);
            }
            disagreement(&e, &face_origin)
        })?;
        Ok(Solid { item, name, product: None, brep, face_origin, sheet, notes, turned: turned_faces })
    }

    /// Leaves out each face that is no wider than the tolerance: one loop whose two open edges
    /// are the same curve record over the same interval between the same two vertices (its two
    /// sides one line), its other edges closed edges whose curves stay within the tolerance of
    /// their vertex. Such a face is a wall an outline makes where it runs out and back along
    /// itself (a hairpin some exporters write, tangency-labs/solidcore#3292's [T-24]); kept, its
    /// two sides are coincident edges of one face (`SmFace` tests #33 and #34) and the body
    /// fails the check before any operation. Left out, its closed edges (points at the
    /// tolerance) drop out of the faces beside it (a face they alone bound, the wall's cap, goes
    /// too), and its two sides, each now used by one face, are one edge
    /// ([`Self::merge_twin_edges`]). Faces are renumbered in `shells`. Returns how many walls were
    /// left out.
    ///
    /// [T-24]: https://tickets.solidcraft.io/t/24
    fn drop_hairpin_faces(&mut self, shells: &mut [Vec<(u32, bool)>]) -> R<usize> {
        let mut gone: Vec<usize> = Vec::new();
        let mut points: HashSet<u32> = HashSet::new();
        for (fi, face) in self.faces.iter().enumerate() {
            let [Bound::Edges(es)] = face.loops.as_slice() else { continue };
            let (mut open, mut closed): (Vec<u32>, Vec<u32>) = (Vec::new(), Vec::new());
            for &(e, _) in es {
                let edge = self.edges.get(e as usize).ok_or("edge reference")?;
                let [a, b] = edge.vertices;
                if a == b { closed.push(e) } else { open.push(e) }
            }
            let [x, y] = open.as_slice() else { continue };
            let (ex, ey) = (self.edges.get(*x as usize).ok_or("edge reference")?, self.edges.get(*y as usize).ok_or("edge reference")?);
            let same_ends = {
                let ([a, b], [c, d]) = (ex.vertices, ey.vertices);
                (a == c && b == d) || (a == d && b == c)
            };
            if x == y || !same_ends || ex.curve != ey.curve || ex.interval != ey.interval {
                continue;
            }
            let mut narrow = true;
            for &e in &closed {
                let edge = self.edges.get(e as usize).ok_or("edge reference")?;
                let at = v3(self.point(edge.vertices[0])?);
                let c = CurveEval::new(&edge.curve).map_err(|m| m.to_string())?;
                let [lo, hi] = edge.interval;
                for k in 0..=32 {
                    let p = c.point(lo + (hi - lo) * f64::from(k) / 32.0).ok_or("edge point")?;
                    if !(p.dist(at) <= self.tol) {
                        narrow = false;
                    }
                }
            }
            if narrow {
                gone.push(fi);
                points.extend(closed);
            }
        }
        if gone.is_empty() {
            return Ok(0);
        }
        let walls = gone.len();
        // A face bounded by those closed edges alone (the wall's cap, inside the outline's
        // excursion) is no wider than the tolerance either: it goes too.
        for (fi, face) in self.faces.iter().enumerate() {
            let edge_loops: Vec<&Vec<(u32, bool)>> = face
                .loops
                .iter()
                .filter_map(|l| match l {
                    Bound::Edges(es) => Some(es),
                    Bound::Vertex(_) => None,
                })
                .collect();
            if !gone.contains(&fi) && !edge_loops.is_empty() && edge_loops.iter().all(|es| es.iter().all(|(e, _)| points.contains(e))) {
                gone.push(fi);
            }
        }
        // The closed edges out of every other loop. A hole of nothing else (the outline's
        // excursion seen from the face it runs into) goes with them; an outer loop of nothing
        // else keeps its face whole.
        for (fi, face) in self.faces.iter_mut().enumerate() {
            if gone.contains(&fi) {
                continue;
            }
            let outer = face.outer.unwrap_or(0);
            let mut kept = Vec::new();
            let mut new_outer = None;
            for (k, mut l) in std::mem::take(&mut face.loops).into_iter().enumerate() {
                if let Bound::Edges(es) = &mut l {
                    if es.iter().all(|(e, _)| points.contains(e)) {
                        if k != outer {
                            continue;
                        }
                    } else {
                        es.retain(|(e, _)| !points.contains(e));
                    }
                }
                if k == outer && face.outer.is_some() {
                    new_outer = Some(kept.len());
                }
                kept.push(l);
            }
            face.loops = kept;
            if face.outer.is_some() {
                face.outer = new_outer;
            }
        }
        // The faces out, the rest renumbered in the shells.
        let mut new_index: Vec<Option<u32>> = Vec::new();
        let mut next = 0u32;
        for fi in 0..self.faces.len() {
            if gone.contains(&fi) {
                new_index.push(None);
            } else {
                new_index.push(Some(next));
                next += 1;
            }
        }
        let mut k = 0usize;
        self.faces.retain(|_| {
            let keep = !gone.contains(&k);
            k += 1;
            keep
        });
        for shell in shells.iter_mut() {
            shell.retain_mut(|(f, _)| match new_index.get(*f as usize).copied().flatten() {
                Some(n) => {
                    *f = n;
                    true
                }
                None => false,
            });
        }
        Ok(walls)
    }

    /// Edges the file writes twice: two EDGE_CURVEs on the same two vertices and the same curve
    /// record over the same interval (equal, not near), each used by one face of the solid. The
    /// faces share one boundary, so they share one edge: the second is the first, run the way its vertices run against the first's. An edge used more than once, or one whose
    /// twin is another curve, is left as the file has it. Returns how many.
    fn merge_twin_edges(&mut self) -> R<usize> {
        let mut uses: HashMap<u32, usize> = HashMap::new();
        for face in &self.faces {
            for l in &face.loops {
                if let Bound::Edges(es) = l {
                    for (e, _) in es {
                        *uses.entry(*e).or_default() += 1;
                    }
                }
            }
        }
        let mut by_vertices: HashMap<(u32, u32), Vec<u32>> = HashMap::new();
        for (&e, &n) in &uses {
            let edge = self.edges.get(e as usize).ok_or("edge reference")?;
            let [a, b] = edge.vertices;
            if n == 1 && a != b {
                by_vertices.entry((a.min(b), a.max(b))).or_default().push(e);
            }
        }
        let mut pairs: Vec<(u32, u32)> =
            by_vertices.into_values().filter_map(|mut v| if let [a, b] = v.as_mut_slice() { Some((*a.min(b), *a.max(b))) } else { None }).collect();
        pairs.sort_unstable();
        // Each edge is in at most one pair, so no redirect meets another: all are applied in one pass.
        let mut redirect: HashMap<u32, (u32, bool)> = HashMap::new();
        for (a, b) in pairs {
            let (ea, eb) = (self.edges.get(a as usize).ok_or("edge reference")?, self.edges.get(b as usize).ok_or("edge reference")?);
            let against = ea.vertices[0] != eb.vertices[0];
            // Only the same curve record over the same interval: the two edges then coincide
            // exactly, and nothing is decided by how near two different curves come.
            if ea.curve != eb.curve || ea.interval != eb.interval {
                continue;
            }
            redirect.insert(b, (a, against));
        }
        for face in &mut self.faces {
            for l in &mut face.loops {
                if let Bound::Edges(es) = l {
                    for u in es.iter_mut() {
                        if let Some(&(a, against)) = redirect.get(&u.0) {
                            *u = (a, u.1 != against);
                        }
                    }
                }
            }
        }
        Ok(redirect.len())
    }

    /// A two-edge loop that runs out along one line on one edge and back on another edge of the
    /// same vertices, curve record and interval (a bore that touches a flat face along a line
    /// writes the loop so), where each of the two edges is also used by exactly one other face.
    /// The loop is a slit of no area, so it is rewritten to use one edge twice, and the other
    /// faces then share the other edge: the pairing a file that writes the slit as one edge has.
    /// Without this the loop is not seen as a slit and the shell reads as contradictory.
    /// Returns how many loops were rewritten.
    fn pair_slit_edges(&mut self) -> R<usize> {
        let mut uses: HashMap<u32, Vec<(usize, usize, usize)>> = HashMap::new();
        for (fi, face) in self.faces.iter().enumerate() {
            for (li, l) in face.loops.iter().enumerate() {
                if let Bound::Edges(es) = l {
                    for (pos, (e, _)) in es.iter().enumerate() {
                        uses.entry(*e).or_default().push((fi, li, pos));
                    }
                }
            }
        }
        // (face, loop, the loop's use of x, the loop's use of y, x, y, y runs against x)
        let mut work: Vec<(usize, usize, usize, usize, u32, u32, bool)> = Vec::new();
        let mut touched: HashSet<u32> = HashSet::new();
        for (fi, face) in self.faces.iter().enumerate() {
            for (li, l) in face.loops.iter().enumerate() {
                let Bound::Edges(es) = l else { continue };
                let [(x, dx), (y, dy)] = es.as_slice() else { continue };
                let (x, y) = (*x, *y);
                if x == y {
                    continue;
                }
                let (ex, ey) = (self.edges.get(x as usize).ok_or("edge reference")?, self.edges.get(y as usize).ok_or("edge reference")?);
                let [a, b] = ex.vertices;
                let against = ex.vertices[0] != ey.vertices[0];
                let same_ends = a != b && ((ey.vertices == [a, b]) || (ey.vertices == [b, a]));
                if !same_ends || ex.curve != ey.curve || ex.interval != ey.interval {
                    continue;
                }
                // Out along one, back along the other: the two uses run against each other on
                // the line.
                if (dx == dy) != against {
                    continue;
                }
                let (Some(ux), Some(uy)) = (uses.get(&x), uses.get(&y)) else { continue };
                if ux.len() != 2 || uy.len() != 2 || touched.contains(&x) || touched.contains(&y) {
                    continue;
                }
                // Each edge's other use is a different face's, and not this loop's twin.
                let other = |u: &Vec<(usize, usize, usize)>| u.iter().find(|&&(f, l, _)| !(f == fi && l == li)).copied();
                let (Some(ox), Some(oy)) = (other(ux), other(uy)) else { continue };
                if ox.0 == fi || oy.0 == fi || ox.0 == oy.0 {
                    continue;
                }
                touched.insert(x);
                touched.insert(y);
                work.push((fi, li, 0, 1, x, y, against));
            }
        }
        let count = work.len();
        for (fi, li, px, _py, x, y, against) in work {
            let Some(oy) = uses.get(&y).and_then(|u| u.iter().find(|&&(f, l, _)| !(f == fi && l == li)).copied()) else { continue };
            // The other face that used y now uses x, run the way its vertices run against x's.
            let face = self.faces.get_mut(oy.0).ok_or("face reference")?;
            if let Some(Bound::Edges(es)) = face.loops.get_mut(oy.1)
                && let Some(u) = es.get_mut(oy.2)
            {
                *u = (x, u.1 != against);
            }
            // This loop's use of x becomes a use of y, so the loop is y out and y back.
            let face = self.faces.get_mut(fi).ok_or("face reference")?;
            if let Some(Bound::Edges(es)) = face.loops.get_mut(li)
                && let Some(u) = es.get_mut(px)
            {
                *u = (y, u.1 != against);
            }
        }
        Ok(count)
    }

    /// Orient a closed manifold shell by exact shared-edge parity. Across an edge,
    /// the two physical traversals oppose: equal directions require opposite flips.
    /// An already coherent shell keeps its sides, including a nonmanifold shell and one whose
    /// line where four faces meet is written as two edges paired either way; an inconsistent
    /// nonmanifold or disconnected shell has no supported repair.
    fn orient_shell(&self, shell: &mut [(u32, bool)]) -> R<bool> {
        let mut uses: BTreeMap<u32, Vec<(usize, bool)>> = BTreeMap::new();
        for (side, &(f, forward)) in shell.iter().enumerate() {
            let face = self.faces.get(f as usize).ok_or("face reference")?;
            for bound in &face.loops {
                if let Bound::Edges(edges) = bound {
                    for &(edge, along) in edges {
                        uses.entry(edge).or_default().push((side, along == forward));
                    }
                }
            }
        }
        // An edge the file writes twice where four faces meet along one line (two edges on the
        // same vertices, interval and curve, each used by two of them) counts as one edge: the
        // file may pair the faces across the twins either way, and the sides are coherent where
        // the line's uses cancel.
        let mut twins: HashMap<[u32; 2], Vec<u32>> = HashMap::new();
        let mut pooled: BTreeMap<u32, (usize, usize)> = BTreeMap::new();
        for (&e, sides) in &uses {
            let edge = self.edges.get(e as usize).ok_or("edge reference")?;
            let same = twins.entry(edge.vertices).or_default();
            let twin =
                same.iter().copied().find(|t| self.edges.get(*t as usize).is_some_and(|t| t.interval == edge.interval && t.curve == edge.curve));
            let key = twin.unwrap_or_else(|| {
                same.push(e);
                e
            });
            let along = sides.iter().filter(|(_, forward)| *forward).count();
            let count = pooled.entry(key).or_default();
            *count = (count.0 + along, count.1 + sides.len().saturating_sub(along));
        }
        if pooled.values().all(|(along, against)| along == against) {
            return Ok(false);
        }
        let mut adjacent: BTreeMap<usize, Vec<(usize, bool)>> = BTreeMap::new();
        for sides in uses.values() {
            let [(a, along_a), (b, along_b)] = sides.as_slice() else {
                return Err(unsupported("orienting an inconsistent shell with a nonmanifold edge"));
            };
            let opposite = along_a == along_b;
            adjacent.entry(*a).or_default().push((*b, opposite));
            adjacent.entry(*b).or_default().push((*a, opposite));
        }
        let mut flips = BTreeMap::new();
        let mut started = false;
        for seed in 0..shell.len() {
            if flips.contains_key(&seed) {
                continue;
            }
            if started {
                return Err(unsupported("orienting an inconsistent shell with disconnected faces"));
            }
            started = true;
            let mut pending = vec![(seed, false)];
            while let Some((side, flip)) = pending.pop() {
                if let Some(&known) = flips.get(&side) {
                    if known != flip {
                        return Err(unsupported("a shell with contradictory edge orientations"));
                    }
                    continue;
                }
                flips.insert(side, flip);
                for &(next, opposite) in adjacent.get(&side).into_iter().flatten() {
                    pending.push((next, flip != opposite));
                }
            }
        }
        for (side, (_, forward)) in shell.iter_mut().enumerate() {
            let flip = flips.get(&side).ok_or("face side reference")?;
            *forward ^= *flip;
        }
        Ok(true)
    }

    /// Every edge of the shell's faces used at least twice among them.
    fn closed(&self, shell: &[(u32, bool)]) -> bool {
        let mut count: HashMap<u32, u32> = HashMap::new();
        for &(f, _) in shell {
            for l in self.faces.get(f as usize).map(|f| f.loops.as_slice()).unwrap_or(&[]) {
                if let Bound::Edges(es) = l {
                    for (e, _) in es {
                        *count.entry(*e).or_default() += 1;
                    }
                }
            }
        }
        count.values().all(|n| *n >= 2)
    }

    /// The face's loops, outer first (the rest in the file's order).
    fn loop_order(&self, f: usize) -> Vec<usize> {
        let Some(face) = self.faces.get(f) else { return Vec::new() };
        let outer = face.outer.unwrap_or(0);
        std::iter::once(outer).chain((0..face.loops.len()).filter(|l| *l != outer)).collect()
    }

    /// A face's box on its label over its edge loops (`crate::kernel::form::extent`), a boundary
    /// point on a pole within the face's tolerance (the largest of its edges' and vertices': a
    /// cone's apex or a sphere's pole a file places a few µm off); `None` where the boundary does
    /// not project onto it.
    fn face_extent(&self, fi: usize) -> R<Option<[[f64; 2]; 2]>> {
        self.loops_extent(fi, None)
    }

    /// [`Self::face_extent`] over the face's loop `only` alone where given: each loop is followed
    /// from its own first point, so a loop's box is wider than a turn only where the loop itself
    /// goes round its axis more than once (a thread's helix and back).
    fn loops_extent(&self, fi: usize, only: Option<usize>) -> R<Option<[[f64; 2]; 2]>> {
        let face = self.faces.get(fi).ok_or("face reference")?;
        let vertex_tolerance = |v: u32| self.vertices.get(v as usize).map(|x| x.tolerance).ok_or("vertex reference");
        let (mut boundary, mut reach) = (Vec::new(), self.tol);
        for (li, l) in face.loops.iter().enumerate() {
            if only.is_some_and(|o| o != li) {
                continue;
            }
            let es = match l {
                Bound::Edges(es) => es,
                Bound::Vertex(v) => {
                    reach = reach.max(vertex_tolerance(*v)?);
                    continue;
                }
            };
            let mut b = Vec::new();
            for &(e, forward) in es {
                let edge = self.edges.get(e as usize).ok_or("edge reference")?;
                let [v0, v1] = edge.vertices;
                reach = reach.max(edge.tolerance).max(vertex_tolerance(v0)?).max(vertex_tolerance(v1)?);
                b.push((&edge.curve, edge.interval, forward));
            }
            boundary.push(b);
        }
        Ok(crate::kernel::form::extent(&face.surface, &boundary, reach))
    }

    /// The pcurves of a face's edge loops (in its edge-loop order), its domain, whether it was
    /// turned over, and its NURBS form. Picks the outer loop when the file
    /// marks none (or marks one that another loop encloses), and turns the face over when its
    /// outer loop runs clockwise about the natural normal on a surface where that cannot be a
    /// face's outer boundary.
    /// Loops that touch at one vertex are then one loop through it ([`touching::join`]).
    fn face_pcurves(&mut self, fi: usize) -> R<(Vec<Vec<Curve2>>, [[f64; 2]; 2], bool, Option<BSurface>)> {
        let face = self.faces.get(fi).ok_or("face reference")?;
        let label = face.surface.clone();
        let edge_loops: Vec<Vec<(u32, bool)>> = face
            .loops
            .iter()
            .filter_map(|l| match l {
                Bound::Edges(es) => Some(es.clone()),
                Bound::Vertex(_) => None,
            })
            .collect();
        // Which loop index in `face.loops` each edge loop is.
        let positions: Vec<usize> = face.loops.iter().enumerate().filter(|(_, l)| matches!(l, Bound::Edges(_))).map(|(i, _)| i).collect();
        let tol = self.tol;
        // A cylinder, cone, sphere, torus or revolution on its NURBS form, over the extent of its
        // boundary on its label; then every pcurve on the forms.
        let form = match crate::kernel::form::frame_of(&label) {
            Some(_) => {
                let extent = match face.extent {
                    Some(e) => Some(e),
                    None => self.face_extent(fi)?,
                };
                let extent = extent.ok_or_else(|| unsupported("a face whose boundary does not lie on its surface"))?;
                let turns = (extent[0][1] - extent[0][0]) / TAU;
                let (f, d) = if crate::kernel::form::winds(extent) {
                    if !matches!(label, BSurface::Cylinder { .. } | BSurface::Cone { .. }) {
                        return Err(unsupported(format!(
                            "a face that winds {turns:.1} turns about its axis on a surface that is not a cylinder or a cone"
                        )));
                    }
                    crate::kernel::form::wound_surface(&label, extent, tol)
                        .ok_or_else(|| unsupported(format!("a face that winds {turns:.1} turns about its axis, past the 64 a form takes")))?
                } else {
                    crate::kernel::form::surface_within(&label, extent, tol).ok_or_else(|| unsupported("a face of more than one turn"))?
                };
                Some((f, d, extent))
            }
            None => None,
        };
        let (form, box_on_form, extent) = match form {
            Some((f, d, e)) => (Some(f), Some(d), Some(e)),
            None => (None, None, None),
        };
        let mut surface = form.clone().unwrap_or_else(|| label.clone());
        // A face that winds round its axis more than a turn is on a form that does not close:
        // no analytic surface has it (SMLib's cylinder is one turn), so it is a spline face.
        let wound = extent.is_some_and(crate::kernel::form::winds);
        let mut extended = false;
        // How far each edge lies off this surface, so its closed form is checked at that.
        self.measure_edges(&label, &edge_loops, box_on_form.filter(|_| !wound))?;
        // Closed forms where the geometry has them; the kernel makes the rest (`trims`).
        let fit = self.pcurve_tolerance();
        let mut exact = pcurve::exact_pcurves(&surface, &uses_of(&self.edges, &edge_loops, None)?, fit, box_on_form).map_err(|e| e.to_string())?;
        // A form ends on the quarter line its face's boundary runs past by up to the tolerance,
        // so an edge that lies wholly past that line (a few µm of a tube circle beyond it) has no
        // closed form on it, and the kernel's trims are made for an edge off their surface. Its
        // closed form is taken on the face's form not cut short (the same quarter grid, so the
        // same parameters), and put on the range below as the others are ([`onto_form`]).
        let mut uncut_forms = false;
        if !wound
            && exact.iter().flatten().any(Option::is_none)
            && let (Some(e), Some(_)) = (extent, &form)
            && let Some((uncut, d)) = crate::kernel::form::surface_within(&label, e, 0.0)
        {
            let wide = pcurve::exact_pcurves(&uncut, &uses_of(&self.edges, &edge_loops, None)?, fit, Some(d)).map_err(|e| e.to_string())?;
            for (l, w) in exact.iter_mut().zip(wide) {
                for (pc, w) in l.iter_mut().zip(w) {
                    if pc.is_none() && w.is_some() {
                        *pc = w;
                        uncut_forms = true;
                    }
                }
            }
        }
        // On a form of many turns a closed form does not say which turn each of its ends is on:
        // an arc's two ends came out turns apart. An edge that goes round the axis (an arc, a
        // helix) takes its pcurve from its own angle, followed ([`wound::pcurve`]); a ruling,
        // at one angle, keeps its closed form, which the loop's unwinding moves by whole turns.
        if wound {
            for (l, given) in edge_loops.iter().zip(exact.iter_mut()) {
                for (&(e, _), pc) in l.iter().zip(given.iter_mut()) {
                    if !matches!(self.edges.get(e as usize).map(|x| &x.curve), Some(BCurve::Line { .. })) {
                        *pc = None;
                    }
                }
            }
        }

        // Without a kernel, a use with neither is refused below with why.
        let missing = exact.iter().flatten().any(Option::is_none);
        // Closed forms taken on the uncut form are given as they are: made again on the form,
        // they would have none.
        let made = match self.trims {
            _ if uncut_forms && !missing => {
                Some(exact.iter().map(|l| l.iter().cloned().collect::<Option<Vec<_>>>()).collect::<Option<Vec<_>>>().ok_or("a closed form")?)
            }
            // A face that winds round its axis is on a form SMLib's trims hop sheets on.
            _ if wound && missing => Some(self.wound_pcurves(&label, &edge_loops, &exact)?),
            Some(trims) if missing => {
                let domain = match box_on_form {
                    Some(d) => d,
                    None => match natural_box(&surface) {
                        Some(d) => d,
                        // An unbounded surface (a plane) is bounded where the face is: its
                        // boundary's box, with room round it.
                        None => self
                            .face_extent(fi)?
                            .and_then(|e| bounded_by(&surface, e, tol))
                            .ok_or_else(|| unsupported("a face on an unbounded surface without closed-form pcurves"))?,
                    },
                };
                Some(trims.trims(&self.trim_face(&label, form.as_ref(), domain, &edge_loops, exact)?)?)
            }
            _ => None,
        };
        let mut pcs =
            pcurve::pcurves_within(&surface, &uses_of(&self.edges, &edge_loops, made.as_ref())?, fit, box_on_form).map_err(|e| e.to_string())?;
        // Each loop's uses are moved by whole turns (four form parameters) to run on from one
        // another, on the form that does not close.
        if wound {
            let range = match solidcore_ops::label::surface_natural(&surface).first() {
                Some(Natural::Range(a, b)) => [*a, *b],
                _ => return Err(unsupported("a face that winds round a surface that closes on itself")),
            };
            wound::unwind(&edge_loops, &mut pcs, range)?;
        }
        // A form ends on a quarter line its face's boundary runs past by up to the tolerance: the
        // pcurves are put on its range, as the face's domain is ([`face_domain`]), and the edges'
        // tolerances cover the move.
        if let (Some(f), Some(e)) = (&form, extent) {
            onto_form(f, &label, e, tol, &mut pcs);
        }
        // A spline face's pcurve may end a little past the patch's knot range (a closed form
        // whose ends are where the edge's own lie, up to its tolerance past the corner): put on
        // the range, as the face's domain is, where that moves the surface within the edge's
        // tolerance ([`onto_knots`]).
        if form.is_none() {
            let tolerances = edge_loops
                .iter()
                .map(|l| {
                    l.iter().map(|&(e, _)| self.edges.get(e as usize).map(|x| x.tolerance).ok_or("edge reference")).collect::<Result<Vec<_>, _>>()
                })
                .collect::<Result<Vec<_>, _>>()?;
            onto_knots(&surface, &tolerances, &mut pcs);
            // Pcurves still past the range (moving them would move the surface more than their
            // edges' tolerance) are on the patch continued to reach them ([`extend`]).
            if let Some(longer) = extend::extended(&surface, pcurve_box(&pcs), tol) {
                surface = longer;
                extended = true;
                self.patches_extended += 1;
            }
        }
        // On a surface that closes on itself a pcurve may run past the seam by its fit's
        // rounding while the uses beside it end on it: put on the range ([`onto_seam`]).
        onto_seam(&surface, &edge_loops, &mut pcs);
        self.raise_edges(&surface, &edge_loops, &pcs)?;
        // Signed areas of the loops that close in (u, v).
        let periods = natural_periods(&surface);
        // Each use starts near where the one before it ends, along a bounded direction that
        // does not wrap: a loop around a closed spline surface without a seam edge jumps across
        // its domain.
        let natural = solidcore_ops::label::surface_natural(&surface);
        // At a pole (where the surface collapses to a point across a direction, to within the
        // tolerance of the vertex the two uses meet at) a loop may jump in that direction: the
        // seam's two uses meet there a period apart, and a side collapsed to within the vertex's
        // tolerance (a corner patch with one side shrunk to a few µm) is run along between them,
        // its boundary iso line. As the record check (`brep::geometry`) takes it.
        let poles = SurfaceEval::new(&surface).ok();
        let at_pole = |uv: [f64; 2], reach: f64| -> R<bool> {
            Ok(match &poles {
                Some(e) => e.pole_within(uv, reach)?.iter().any(|p| *p),
                None => false,
            })
        };
        for (l, pc) in edge_loops.iter().zip(&pcs) {
            let mut ends = Vec::with_capacity(l.len());
            for (&(_, forward), c) in l.iter().zip(pc) {
                let [a, b] = c.interval().ok_or("pcurve interval")?;
                let (s, e) = (solidcore_ops::pcurve::eval(c, a).ok_or("pcurve start")?, solidcore_ops::pcurve::eval(c, b).ok_or("pcurve end")?);
                ends.push(if forward { (s, e) } else { (e, s) });
            }
            for k in 0..ends.len() {
                let (Some((start, _)), Some((_, end))) = (ends.get(k), ends.get(if k == 0 { ends.len() - 1 } else { k - 1 })) else { continue };
                // The vertex use k starts at, where the one before it ends.
                let &(e, forward) = l.get(k).ok_or("edge use")?;
                let v = self.edges.get(e as usize).ok_or("edge reference")?.vertices;
                let v = if forward { v[0] } else { v[1] };
                let reach = self.vertices.get(v as usize).ok_or("vertex reference")?.tolerance.max(tol);
                if at_pole(*start, reach)? || at_pole(*end, reach)? {
                    continue;
                }
                for d in 0..2 {
                    // Only a bounded direction has a seam to jump; half its width is no gap.
                    let Some(Natural::Range(a, b)) = natural.get(d) else { continue };
                    if !(b - a).is_finite() {
                        continue;
                    }
                    let (Some(sd), Some(ed)) = (start.get(d), end.get(d)) else { continue };
                    if !((sd - ed).abs() <= 0.5 * (b - a)) {
                        // The two uses meet a jump apart along `d`, but the iso-line run between
                        // them lies within the vertex's tolerance of it (a narrow strip's end
                        // an exporter closed with one vertex, not an edge): the loop runs along
                        // it there, as at a pole. Certified on the iso-lines through both ends.
                        let centre = v3(self.vertices.get(v as usize).ok_or("vertex reference")?.point);
                        let within = |at: &[f64; 2]| -> bool {
                            poles.as_ref().is_some_and(|s| matches!(s.iso_within(*at, d, (*ed, *sd), centre, reach), Ok(true)))
                        };
                        if within(start) && within(end) {
                            continue;
                        }
                        // Whether the surface closes across `d` there: its two boundaries meet.
                        let ends_at = |x: f64| {
                            let mut q = *start;
                            if let Some(c) = q.get_mut(d) {
                                *c = x;
                            }
                            poles.as_ref().and_then(|s| s.point(q))
                        };
                        let closed = matches!((ends_at(*a), ends_at(*b)), (Some(p), Some(q)) if p.dist(q) <= reach);
                        // A side of a spline face the file made one vertex (a corner patch's
                        // side a few µm long, its two corners written as one): where every point
                        // of it lies within the vertex's tolerance of the vertex, the loop runs
                        // along it, as at a pole, the vertex's tolerance raised to reach along
                        // it from the loop's point there, as the record check measures it.
                        if !closed
                            && form.is_none()
                            && let Some(e) = &poles
                            && let Some(x) = self.vertices.get(v as usize)
                            && let Some(r) = side_reach(e, *start, d, [*a, *b], Vec3::from(x.point), reach)
                        {
                            if let Some(x) = self.vertices.get_mut(v as usize) {
                                x.tolerance = x.tolerance.max(r);
                            }
                            self.sides_in_vertices += 1;
                            continue;
                        }
                        // The patch started instead inside the widest gap the face's boundary
                        // leaves round it ([`reseam`]), and the face's pcurves made on that
                        // (once: a face whose loop still jumps is refused).
                        if closed && form.is_none() && !self.reseamed.contains(&fi) {
                            // Each pcurve's control hull along `d` encloses it there, so the gap
                            // the seam goes in is one no pcurve reaches into.
                            let along: Option<Vec<[f64; 2]>> = pcs.iter().flatten().map(|c| pcurve_span(c, d)).collect();
                            if let Some(moved) =
                                along.and_then(|along| reseam::gap_middle_of(&along, [*a, *b])).and_then(|at| reseam::reseamed(&surface, d, at))
                            {
                                self.reseamed.insert(fi);
                                self.faces.get_mut(fi).ok_or("face reference")?.surface = moved;
                                return self.face_pcurves(fi);
                            }
                        }
                        return Err(unsupported(if closed {
                            "a loop across a closed spline surface's seam without a seam edge".to_string()
                        } else {
                            format!(
                                "a loop that jumps across its spline surface's {} range where the surface neither closes nor collapses to a point",
                                if d == 0 { "u" } else { "v" }
                            )
                        }));
                    }
                }
            }
        }
        let shapes: Vec<Option<f64>> = edge_loops.iter().zip(&pcs).map(|(l, pc)| loop_area(l, pc, periods)).collect();
        let face = self.faces.get_mut(fi).ok_or("face reference")?;
        let marked = face.outer.and_then(|o| positions.iter().position(|p| *p == o));
        // Which loop is largest is decided on the areas the loops' pcurves enclose, integrated
        // along them, not on polygons: nested loops of nearly one size (a thin ring) must not swap
        // places by how finely, or in how many edges, each is sampled.
        // Each with an estimate of its integration error: `(area, error)`.
        let sizes: Vec<Option<(f64, f64)>> = if edge_loops.len() > 1 {
            edge_loops
                .iter()
                .zip(&pcs)
                .zip(&shapes)
                .map(|((l, pc), coarse)| coarse.and_then(|scale| loop_area_integrated(l, pc, scale.abs())))
                .collect()
        } else {
            shapes.iter().map(|a| a.map(|a| (a, 0.0))).collect()
        };
        // Certainly the largest; else the file's own outer bound; else, with nothing better, the
        // largest measured.
        let largest = largest_loop(&sizes).or(marked).or_else(|| {
            sizes.iter().enumerate().filter_map(|(i, a)| a.map(|(a, _)| (i, a.abs()))).max_by(|a, b| a.1.total_cmp(&b.1)).map(|(i, _)| i)
        });
        let outer = match (marked, largest) {
            // A marked outer loop that closes in (u, v) but another encloses more: that one.
            (Some(m), Some(l)) if shapes.get(m).copied().flatten().is_some() => l,
            (Some(m), _) => m,
            (None, Some(l)) if shapes.iter().all(Option::is_some) => l,
            (None, _) => 0,
        };
        face.outer = positions.get(outer).copied();
        if wound || extended {
            face.surface = surface.clone();
        }
        let can_turn = matches!(
            surface,
            BSurface::Plane { .. } | BSurface::Cylinder { .. } | BSurface::Cone { .. } | BSurface::Spline { .. } | BSurface::RationalSpline { .. }
        );
        let clockwise = shapes.get(outer).copied().flatten().is_some_and(|a| a < 0.0);
        // A face bounded by seam uses alone (a whole torus within its two seams) has loops that
        // run each edge both ways: run backwards they are the same uses, so they say nothing of
        // the face's side. One running clockwise is run the other way; the face keeps its side.
        let seams_only = edge_loops.iter().all(|l| seam_only(l));
        let flip = can_turn && clockwise && !seams_only;
        let rerun = flip || (clockwise && seams_only);
        if rerun {
            for l in &mut face.loops {
                if let Bound::Edges(es) = l {
                    reverse(es);
                }
            }
        }
        if flip {
            face.forward = !face.forward;
        }
        let mut pcs: Vec<Vec<Curve2>> = if rerun { pcs.into_iter().map(|l| l.into_iter().rev().collect()).collect() } else { pcs };
        // Each loop's area as it now runs.
        let mut areas: Vec<Option<f64>> = shapes.iter().map(|a| a.map(|a| if rerun { -a } else { a })).collect();
        // An inner bound written the way the outer one runs (its FACE_BOUND orientation wrong):
        // on a face whose loops all close in (u, v), each loop but the outer runs clockwise, so
        // such a loop is run the other way. Its uses and their pcurves are the same.
        if can_turn && !seams_only && shapes.iter().all(Option::is_some) {
            for (i, area) in shapes.iter().enumerate() {
                let Some(a) = *area else { continue };
                let a = if rerun { -a } else { a };
                if i == outer || !(a > 0.0) {
                    continue;
                }
                if let Some(Bound::Edges(es)) = positions.get(i).and_then(|&p| face.loops.get_mut(p)) {
                    reverse(es);
                }
                if let Some(l) = pcs.get_mut(i) {
                    l.reverse();
                }
                if let Some(slot) = areas.get_mut(i) {
                    *slot = Some(-a);
                }
                self.inner_loops_turned += 1;
            }
        }
        // Loops that touch at one vertex are one loop through it, where they bound one region
        // that way ([`touching::join`]); the others are noted, with where they touch.
        let mut starts: Vec<Vec<u32>> = Vec::new();
        let mut places: Vec<Vec<Option<[f64; 2]>>> = Vec::new();
        for (&p, pc) in positions.iter().zip(&pcs) {
            let Some(Bound::Edges(es)) = face.loops.get(p) else { return Err("loop reference".into()) };
            let mut vs = Vec::new();
            for &(e, forward) in es {
                let [a, z] = self.edges.get(e as usize).ok_or("edge reference")?.vertices;
                vs.push(if forward { a } else { z });
            }
            starts.push(vs);
            places.push(
                es.iter()
                    .zip(pc)
                    .map(|(&(_, forward), c)| {
                        let [a, b] = c.interval()?;
                        solidcore_ops::pcurve::eval(c, if forward { a } else { b })
                    })
                    .collect(),
            );
        }
        // The outer loop runs counter-clockwise and every other loop clockwise.
        let sound: Vec<bool> = areas.iter().enumerate().map(|(i, a)| a.is_some_and(|a| if i == outer { a > 0.0 } else { a < 0.0 })).collect();
        // Two uses that start at one vertex start at one place of (u, v), not a period apart.
        let place = |(l, k): (usize, usize)| places.get(l).and_then(|ps| ps.get(k)).copied().flatten();
        let same_place = |a: (usize, usize), b: (usize, usize)| {
            let (Some(p), Some(q)) = (place(a), place(b)) else { return false };
            natural.iter().zip(p.iter().zip(&q)).all(|(n, (x, y))| {
                let half = match n {
                    Natural::Periodic => 0.5 * TAU,
                    Natural::Range(lo, hi) => 0.5 * (hi - lo),
                };
                (x - y).abs() < half
            })
        };
        let joins = touching::join(&starts, Some(outer), &sound, &same_place);
        if let Some(&v) = joins.left.first() {
            let at = self.vertices.get(v as usize).ok_or("vertex reference")?.point;
            self.loops_left_touching.push((face.origin, at));
        }
        if !joins.walks.is_empty() {
            let written: Vec<Vec<(u32, bool)>> = positions
                .iter()
                .map(|&p| match face.loops.get(p) {
                    Some(Bound::Edges(es)) => es.clone(),
                    _ => Vec::new(),
                })
                .collect();
            for (stay, walk) in &joins.walks {
                let (mut uses, mut curves) = (Vec::new(), Vec::new());
                for &(l, k) in walk {
                    uses.push(*written.get(l).and_then(|es| es.get(k)).ok_or("edge use")?);
                    curves.push(pcs.get(l).and_then(|pc| pc.get(k)).cloned().ok_or("pcurve")?);
                }
                if let Some(Bound::Edges(es)) = positions.get(*stay).and_then(|&p| face.loops.get_mut(p)) {
                    *es = uses;
                }
                if let Some(pc) = pcs.get_mut(*stay) {
                    *pc = curves;
                }
            }
            // The loops now part of another leave the face, and the outer loop's index follows.
            let gone = |i: usize| joins.gone.get(i).copied().unwrap_or(false);
            let mut leaves = vec![false; face.loops.len()];
            for (i, &p) in positions.iter().enumerate() {
                if gone(i)
                    && let Some(slot) = leaves.get_mut(p)
                {
                    *slot = true;
                }
            }
            let outer_at = positions.get(outer).copied();
            face.outer = outer_at.map(|o| o - leaves.iter().take(o).filter(|g| **g).count());
            let mut at = 0;
            face.loops.retain(|_| {
                at += 1;
                !leaves.get(at - 1).copied().unwrap_or(false)
            });
            let mut at = 0;
            pcs.retain(|_| {
                at += 1;
                !gone(at - 1)
            });
            self.loops_joined += joins.gone.iter().filter(|g| **g).count();
        }
        let domain = face_domain(&surface, &pcs)?;
        Ok((pcs, domain, flip, if wound { None } else { form }))
    }

    /// Split each spline edge of a face that winds round its axis (a thread's helix, round it
    /// dozens of times) whose pcurve there ([`wound::pcurve`]) would take more control points than
    /// a spline holds: into the fewest parts of at most half that many each, at ends of its pieces
    /// spread evenly along it, so each part's pcurve fits with room to spare. The parts share
    /// their new vertices with every face the edge bounds. Returns how many edges were split. A
    /// pcurve that cannot be made is left for the face's own pcurves to refuse.
    fn split_long_wound_edges(&mut self) -> R<usize> {
        let mut split = 0;
        for fi in 0..self.faces.len() {
            let Some(face) = self.faces.get(fi) else { continue };
            if !matches!(face.surface, BSurface::Cylinder { .. } | BSurface::Cone { .. }) {
                continue;
            }
            let label = face.surface.clone();
            let mut edges: Vec<u32> =
                face.loops.iter().filter_map(|l| if let Bound::Edges(es) = l { Some(es) } else { None }).flatten().map(|u| u.0).collect();
            edges.sort_unstable();
            edges.dedup();
            if !matches!(self.face_extent(fi), Ok(Some(e)) if crate::kernel::form::winds(e)) {
                continue;
            }
            for e in edges {
                let edge = self.edges.get(e as usize).ok_or("edge reference")?;
                // A line or circle on a cylinder or cone has its pcurve in closed form.
                if !matches!(edge.curve, BCurve::Spline { .. } | BCurve::RationalSpline { .. }) {
                    continue;
                }
                let Ok(ends) = wound::piece_ends(&label, &edge.curve, edge.interval, edge.pcurve_tolerance) else { continue };
                let pieces = ends.len().saturating_sub(1);
                let half = MAX_SPLINE_POINTS >> 1;
                let parts = wound::points_of(pieces).saturating_add(half.saturating_sub(1)).checked_div(half).unwrap_or(0);
                if parts < 2 {
                    continue;
                }
                // From the last cut back, so the edge keeps the part before each cut.
                let [lo, hi] = edge.interval;
                let cuts: Vec<f64> = (1..parts)
                    .rev()
                    .filter_map(|j| pieces.checked_mul(j).and_then(|n| n.checked_div(parts)).and_then(|i| ends.get(i)).copied())
                    .filter(|t| lo < *t && *t < hi)
                    .collect();
                for t in cuts {
                    self.split_edge(e, t)?;
                    split += 1;
                }
            }
        }
        Ok(split)
    }

    /// The pcurve of every use of a face that winds round its axis: the closed form where `exact`
    /// has one, else the edge's own ([`wound::pcurve`]).
    fn wound_pcurves(&self, label: &BSurface, edge_loops: &[Vec<(u32, bool)>], exact: &[Vec<Option<Curve2>>]) -> R<Vec<Vec<Curve2>>> {
        let mut out = Vec::new();
        for (l, given) in edge_loops.iter().zip(exact) {
            let mut pcs = Vec::new();
            for (&(e, _), pc) in l.iter().zip(given) {
                pcs.push(match pc {
                    Some(c) => c.clone(),
                    None => {
                        let edge = self.edges.get(e as usize).ok_or("edge reference")?;
                        let (curve, interval) = match &edge.form {
                            Some((c, i)) => (c, *i),
                            None => (&edge.curve, edge.interval),
                        };
                        wound::pcurve(label, curve, interval, edge.pcurve_tolerance)?
                    }
                });
            }
            out.push(pcs);
        }
        Ok(out)
    }

    /// The face as a kernel takes it to make its missing pcurves: its edges and vertices
    /// numbered within it, each use with its closed-form pcurve where `exact` has one.
    fn trim_face<'s>(
        &'s self,
        surface: &'s BSurface,
        nurbs: Option<&'s BSurface>,
        domain: [[f64; 2]; 2],
        edge_loops: &[Vec<(u32, bool)>],
        exact: Vec<Vec<Option<Curve2>>>,
    ) -> R<TrimFace<'s>> {
        let (mut edge_at, mut vertex_at) = (HashMap::new(), HashMap::new());
        let (mut edges, mut vertices, mut vertex_tolerances) = (Vec::new(), Vec::new(), Vec::new());
        let mut loops = Vec::with_capacity(edge_loops.len());
        for (l, pcs) in edge_loops.iter().zip(exact) {
            let mut uses = Vec::with_capacity(l.len());
            for (&(e, forward), pcurve) in l.iter().zip(pcs) {
                let local = match edge_at.get(&e) {
                    Some(i) => *i,
                    None => {
                        let edge = self.edges.get(e as usize).ok_or("edge reference")?;
                        let mut ends = [0u32; 2];
                        for (end, v) in ends.iter_mut().zip(edge.vertices) {
                            *end = match vertex_at.get(&v) {
                                Some(i) => *i,
                                None => {
                                    let i = index(vertices.len())?;
                                    vertices.push(self.point(v)?);
                                    vertex_tolerances.push(self.vertices.get(v as usize).ok_or("vertex reference")?.tolerance);
                                    vertex_at.insert(v, i);
                                    i
                                }
                            };
                        }
                        let (curve, nurbs, interval) = match &edge.form {
                            Some((n, i)) => (&edge.curve, Some(n), *i),
                            None => (&edge.curve, None, edge.interval),
                        };
                        let i = index(edges.len())?;
                        edges.push(TrimEdge { curve, nurbs, interval, vertices: ends, tolerance: edge.pcurve_tolerance });
                        edge_at.insert(e, i);
                        i
                    }
                };
                uses.push(TrimUse { edge: local, forward, pcurve });
            }
            loops.push(uses);
        }
        Ok(TrimFace { surface, nurbs, domain, edges, vertices, vertex_tolerances, loops, tolerance: self.pcurve_tolerance() })
    }

    /// Source uncertainty bounds the data's gaps; it is not a target for fitting new curves.
    /// Keep finer declarations, but never fit more loosely than an import with no declaration.
    pub(crate) fn pcurve_tolerance(&self) -> f64 {
        self.tol.min(crate::topology::DEFAULT_TOLERANCE)
    }
}

pub(crate) fn unsupported(what: impl std::fmt::Display) -> String {
    format!("not supported yet: {what}")
}

pub(crate) fn index(i: usize) -> R<u32> {
    u32::try_from(i).map_err(|_| "too many items".to_string())
}

pub(crate) fn v3(p: P3) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

/// A face's uses on their edges' NURBS forms, with `given` pcurves where a kernel made them.
fn uses_of<'e>(edges: &'e [EdgeB], edge_loops: &[Vec<(u32, bool)>], given: Option<&'e Vec<Vec<Curve2>>>) -> R<Vec<Vec<EdgeUse<'e>>>> {
    let mut uses = Vec::with_capacity(edge_loops.len());
    for (li, l) in edge_loops.iter().enumerate() {
        let mut u = Vec::with_capacity(l.len());
        for (p, &(e, forward)) in l.iter().enumerate() {
            let edge = edges.get(e as usize).ok_or("edge reference")?;
            let (curve, interval) = match &edge.form {
                Some((c, i)) => (c, *i),
                _ => (&edge.curve, edge.interval),
            };
            let given = match given {
                Some(g) => Some(g.get(li).and_then(|l| l.get(p)).ok_or("a pcurve the kernel did not make")?),
                None => None,
            };
            u.push(EdgeUse { edge: e, curve, interval, forward, tolerance: edge.pcurve_tolerance, given });
        }
        uses.push(u);
    }
    Ok(uses)
}

/// Points along a side of a spline face measured against the vertex it was made ([`side_reach`]).
const SIDE_SAMPLES: usize = 32;

/// The tolerance a vertex at `start` (its point `vertex`, its tolerance `tolerance`) needs for a
/// loop to run along the face's side through `start` in direction `d` over `[a, b]` as at a pole:
/// the least of a few steps past the side's farthest sampled point from `start` at which the
/// side, certified, stays within that of `start` ([`SurfaceEval::pole_within`], which the record
/// check asks again of the finished record: what the vertex's raised tolerance claims is that
/// certificate, not the samples). `None` unless the side's points at `SIDE_SAMPLES + 1`
/// parameters lie within `tolerance` of the vertex: a screen for a side the file made one vertex
/// (it decides only whether the certificate is asked for), and where no step's reach is certified
/// up to two and a half times the tolerance.
fn side_reach(surface: &SurfaceEval, start: [f64; 2], d: usize, [a, b]: [f64; 2], vertex: Vec3, tolerance: f64) -> Option<f64> {
    let at = |x: f64| {
        let mut q = start;
        *q.get_mut(d)? = x;
        surface.point(q)
    };
    let from = surface.point(start)?;
    let mut far = 0.0f64;
    for k in 0..=SIDE_SAMPLES {
        let p = at(a + (b - a) * (k as f64 / SIDE_SAMPLES as f64))?;
        if !(p.dist(vertex) <= tolerance) {
            return None;
        }
        far = far.max(p.dist(from));
    }
    [1.01, 1.1, 1.25, 1.5]
        .into_iter()
        .map(|m| m * far)
        .filter(|r| *r > 0.0 && *r <= 2.5 * tolerance)
        .find(|r| surface.pole_within(start, *r).ok().and_then(|p| p.get(d).copied()).unwrap_or(false))
}

/// The refusal of a record whose geometry disagrees with its topology: on a face (not an edge
/// alone), naming the file's face too (`face #origin`, of `face_origin`), so a reader can leave it
/// out.
pub(crate) fn disagreement(e: &solidcore_ops::record_check::Mismatch, face_origin: &[u64]) -> String {
    use solidcore_ops::record_check::Record;
    let face = match e.record {
        Record::Face(f) | Record::Use { face: f, .. } | Record::VertexLoop { face: f, .. } | Record::Loop { face: f, .. } => Some(f),
        Record::Edge(_) => None,
    };
    match face.and_then(|f| face_origin.get(f as usize)) {
        Some(o) => format!("the record's geometry disagrees: {e}: face #{o}"),
        None => format!("the record's geometry disagrees: {e}"),
    }
}

/// The interval a pcurve's control points span along `d`: the pcurve lies within it (its control
/// hull). `None` for a rational pcurve with a weight that is not positive, whose hull does not
/// enclose it, or for no control points.
fn pcurve_span(c: &Curve2, d: usize) -> Option<[f64; 2]> {
    let xs: Vec<f64> = match c {
        Curve2::Spline { points, .. } => points.iter().map(|p| p.get(d).copied()).collect::<Option<_>>()?,
        Curve2::RationalSpline { homogeneous, .. } => {
            homogeneous.iter().map(|h| if h[2] > 0.0 { h.get(d).map(|x| x / h[2]) } else { None }).collect::<Option<_>>()?
        }
    };
    let out = xs.iter().fold([f64::INFINITY, f64::NEG_INFINITY], |o, x| [o[0].min(*x), o[1].max(*x)]);
    (out[0] <= out[1]).then_some(out)
}

/// On a plane also a spline's knot spans' middles, where its closed-form pcurve is checked:
/// crowded knots can lie off it only between the even samples, and a plane has no bounded
/// domain for the kernel's trims to fall back on (#11652).
fn gap_samples(curve: &BCurve, [t0, t1]: [f64; 2], on_plane: bool) -> Vec<f64> {
    let mut ts: Vec<f64> =
        (0..=EDGE_GAP_SAMPLES).map(|k| if k == EDGE_GAP_SAMPLES { t1 } else { t0 + (t1 - t0) * (k as f64 / EDGE_GAP_SAMPLES as f64) }).collect();
    if let (true, BCurve::Spline { knots, .. } | BCurve::RationalSpline { knots, .. }) = (on_plane, curve) {
        let mut cut = vec![t0];
        cut.extend(knots.iter().copied().filter(|k| *k > t0 && *k < t1));
        cut.push(t1);
        ts.extend(cut.windows(2).filter_map(|w| match w {
            [a, b] if a < b => Some(0.5 * (a + b)),
            _ => None,
        }));
    }
    ts
}

/// A spline surface's parameter box: its knot ranges.
fn natural_box(s: &BSurface) -> Option<[[f64; 2]; 2]> {
    match solidcore_ops::label::surface_natural(s) {
        [Natural::Range(a, b), Natural::Range(c, d)] if [a, b, c, d].iter().all(|x| x.is_finite()) => Some([[a, b], [c, d]]),
        _ => None,
    }
}

/// The box an unbounded surface's face is trimmed within: the face's `extent`, a quarter of its
/// size (and at least ten times `tol`) wider on every side along a direction the surface does not
/// bound, its natural range along one it does. `None` for an extent that is not finite.
fn bounded_by(s: &BSurface, extent: [[f64; 2]; 2], tol: f64) -> Option<[[f64; 2]; 2]> {
    let natural = solidcore_ops::label::surface_natural(s);
    let mut out = extent;
    for (d, o) in out.iter_mut().enumerate() {
        let [a, b] = *o;
        if !(a.is_finite() && b.is_finite() && a <= b) {
            return None;
        }
        *o = match natural.get(d) {
            Some(Natural::Range(lo, hi)) if lo.is_finite() && hi.is_finite() => [*lo, *hi],
            _ => {
                let pad = (0.25 * (b - a)).max(10.0 * tol);
                [a - pad, b + pad]
            }
        };
    }
    Some(out)
}

/// Every edge of the loop is used in it exactly twice, once each way: a loop of seam uses alone.
fn seam_only(uses: &[(u32, bool)]) -> bool {
    let mut ways: HashMap<u32, [u32; 2]> = HashMap::with_capacity(uses.len() / 2);
    for &(e, forward) in uses {
        if let Some(n) = ways.entry(e).or_default().get_mut(usize::from(forward)) {
            *n += 1;
        }
    }
    !uses.is_empty() && ways.values().all(|n| *n == [1, 1])
}

/// The loop run the other way.
pub(crate) fn reverse(uses: &mut [(u32, bool)]) {
    uses.reverse();
    for u in uses {
        u.1 = !u.1;
    }
}

fn natural_periods(s: &BSurface) -> [bool; 2] {
    solidcore_ops::label::surface_natural(s).map(|n| matches!(n, Natural::Periodic))
}

/// The signed area a loop encloses in (u, v) (positive counter-clockwise), or `None` when it
/// does not close there (it winds around a periodic direction) or is degenerate.
fn loop_area(l: &[(u32, bool)], pcs: &[Curve2], periodic: [bool; 2]) -> Option<f64> {
    loop_area_sampled(l, pcs, periodic, 16)
}

/// The loop that encloses the most, by `sizes` (`(area, error estimate)` per loop, `None` for one
/// that doesn't close): only when its lead over every other loop is more than their error
/// estimates together and rounding. `None` when the areas can't tell; the file's own outer bound
/// (`FACE_OUTER_BOUND`, the file's declaration of which loop is outer) then decides.
fn largest_loop(sizes: &[Option<(f64, f64)>]) -> Option<usize> {
    let known: Vec<(usize, f64, f64)> = sizes.iter().enumerate().filter_map(|(i, s)| s.map(|(a, e)| (i, a.abs(), e))).collect();
    let &(best, area, error) = known.iter().max_by(|a, b| a.1.total_cmp(&b.1))?;
    let certain = known.iter().all(|&(i, other, other_error)| i == best || area - error > other + other_error + 4.0 * f64::EPSILON * (area + other));
    certain.then_some(best)
}

/// The signed area a closed loop of pcurves encloses in (u, v), integrated along the curves, and
/// an estimate of the integration's error: half of the integral of `(u - u0) dv - (v - v0) du`
/// (Green), with `(u0, v0)` the loop's first point. Each edge is integrated knot span by knot span
/// (the curve is smooth within a span) by an adaptive eight-point Gauss–Legendre rule: a piece is
/// halved until its halves agree with it, so a rational arc is measured to rounding however its
/// parameter runs, and where the halving budget ends first the disagreement left is the reported
/// error. Halves agree once they differ by rounding: relative to the piece, or to `scale` (the
/// loop's rough area, [`loop_area`]) for a piece that adds little. A gap between one edge's end
/// and the next edge's start (within the file's tolerance) is closed by a straight chord. `None`
/// for a malformed pcurve. Only call it on a loop [`loop_area`] found closed.
fn loop_area_integrated(l: &[(u32, bool)], pcs: &[Curve2], scale: f64) -> Option<(f64, f64)> {
    let floor = 1e-15 * scale;
    let ends = |c: &Curve2, forward: bool| -> Option<([f64; 2], [f64; 2])> {
        let [a, b] = c.interval()?;
        let (p, q) = (solidcore_ops::pcurve::eval(c, a)?, solidcore_ops::pcurve::eval(c, b)?);
        Some(if forward { (p, q) } else { (q, p) })
    };
    let (&(_, first_forward), first_curve) = (l.first()?, pcs.first()?);
    let origin = ends(first_curve, first_forward)?.0;
    let cross = |p: [f64; 2], q: [f64; 2]| (p[0] - origin[0]) * (q[1] - origin[1]) - (q[0] - origin[0]) * (p[1] - origin[1]);
    let mut budget = GREEN_BUDGET;
    let (mut twice, mut error) = (0.0, 0.0);
    let mut previous_end: Option<[f64; 2]> = None;
    for (&(_, forward), c) in l.iter().zip(pcs) {
        let [t0, t1] = c.interval()?;
        let knots = match c {
            Curve2::Spline { knots, .. } | Curve2::RationalSpline { knots, .. } => knots,
        };
        let mut breaks = vec![t0];
        breaks.extend(knots.iter().copied().filter(|k| *k > t0 && *k < t1));
        breaks.push(t1);
        breaks.dedup();
        let mut edge = 0.0;
        for span in breaks.windows(2) {
            let (a, b) = (*span.first()?, *span.get(1)?);
            let whole = green_piece(c, origin, a, b)?;
            let (value, e) = green_adaptive(c, origin, [a, b], whole, floor, GREEN_DEPTH, &mut budget)?;
            edge += value;
            error += e;
        }
        let (start, end) = ends(c, forward)?;
        if let Some(prev) = previous_end {
            twice += cross(prev, start);
        }
        twice += if forward { edge } else { -edge };
        previous_end = Some(end);
    }
    twice += cross(previous_end?, origin);
    (twice.is_finite() && error.is_finite()).then_some((0.5 * twice, 0.5 * error))
}

/// Most halvings of one piece in [`loop_area_integrated`].
const GREEN_DEPTH: u32 = 40;
/// Most pieces [`loop_area_integrated`] integrates for one loop.
const GREEN_BUDGET: usize = 100_000;

/// `(u - u0) v' - (v - v0) u'` of `c` over `[a, b]` by eight-point Gauss–Legendre.
fn green_piece(c: &Curve2, origin: [f64; 2], a: f64, b: f64) -> Option<f64> {
    const NODES: &[f64] = &[0.1834346424956498, 0.525532409916329, 0.7966664774136267, 0.9602898564975363];
    const WEIGHTS: &[f64] = &[0.362683783378362, 0.3137066458778873, 0.2223810344533745, 0.1012285362903763];
    let (mid, half) = (0.5 * (a + b), 0.5 * (b - a));
    let mut sum = 0.0;
    for (x, w) in NODES.iter().zip(WEIGHTS) {
        for t in [mid - half * x, mid + half * x] {
            let (p, d) = solidcore_ops::pcurve::eval_d1(c, t)?;
            sum += w * half * ((p[0] - origin[0]) * d[1] - (p[1] - origin[1]) * d[0]);
        }
    }
    Some(sum)
}

/// [`green_piece`] over `[a, b]` (`whole` its value there) refined by halving until the halves
/// agree with the whole to rounding (relative, or within `floor`), at most `depth` times and within
/// `budget` pieces: the value, and the disagreement left where the refining ended early.
fn green_adaptive(c: &Curve2, origin: [f64; 2], [a, b]: [f64; 2], whole: f64, floor: f64, depth: u32, budget: &mut usize) -> Option<(f64, f64)> {
    let m = 0.5 * (a + b);
    let (left, right) = (green_piece(c, origin, a, m)?, green_piece(c, origin, m, b)?);
    *budget = budget.saturating_sub(2);
    let halves = left + right;
    let gap = (halves - whole).abs();
    if gap <= 1e-13 * (left.abs() + right.abs()) + floor || depth == 0 || *budget == 0 {
        return Some((halves, gap));
    }
    let (l, el) = green_adaptive(c, origin, [a, m], left, floor, depth - 1, budget)?;
    let (r, er) = green_adaptive(c, origin, [m, b], right, floor, depth - 1, budget)?;
    Some((l + r, el + er))
}

/// [`loop_area`] with `steps` chords on each edge. A rational arc sampled evenly in its parameter
/// is not sampled evenly in its angle, so a coarse polygon of one circle can be a percent smaller
/// than a polygon of another circle of nearly the same size; telling nested loops apart by their
/// size takes a finer one.
fn loop_area_sampled(l: &[(u32, bool)], pcs: &[Curve2], periodic: [bool; 2], steps: u32) -> Option<f64> {
    let steps = steps.max(1);
    let mut pts: Vec<[f64; 2]> = Vec::new();
    for (&(_, forward), c) in l.iter().zip(pcs) {
        let [a, b] = c.interval()?;
        for k in 0..=steps {
            let s = f64::from(k) / f64::from(steps);
            let t = if forward { a + (b - a) * s } else { b - (b - a) * s };
            pts.push(solidcore_ops::pcurve::eval(c, t)?);
        }
    }
    let (first, last) = (*pts.first()?, *pts.last()?);
    for d in 0..2 {
        if periodic.get(d).copied().unwrap_or(false) && (first.get(d)? - last.get(d)?).abs() > 1.0 {
            return None;
        }
    }
    let mut area = 0.0;
    let mut magnitude = 0.0;
    for w in pts.windows(2) {
        let (p, q) = (w.first()?, w.get(1)?);
        let c = (p[0] - first[0]) * (q[1] - first[1]) - (q[0] - first[0]) * (p[1] - first[1]);
        area += c;
        magnitude += c.abs();
    }
    (area.abs() > 1e-9 * magnitude && magnitude > 0.0).then_some(0.5 * area)
}

/// The parameters `tolerance` (mm) moves a face on `label` over the height `v` by, along its two
/// directions: the angle on the widest circle of the face about the axis (a cylinder or cone), and
/// on a sphere's meridian or a torus's tube; along a cylinder's or cone's height, the height that
/// moves the surface by `tolerance` along its line (the whole of it on a cylinder, its cosine of
/// the half angle on a cone). `form::surface_within` ends a face's form on a quarter line its
/// domain ends within these angles of, and at the heights its domain ends at; zero along another
/// direction.
fn form_reach(label: &BSurface, v: [f64; 2], tolerance: f64) -> [f64; 2] {
    let reach = |radius: f64| if tolerance > 0.0 && radius > 0.0 && radius.is_finite() { tolerance / radius } else { 0.0 };
    let along = |cosine: f64| if tolerance > 0.0 && cosine.is_finite() { tolerance * cosine } else { 0.0 };
    match label {
        BSurface::Cylinder { radius, .. } => [reach(radius.abs()), along(1.0)],
        BSurface::Cone { radius_at_origin, half_angle, .. } => {
            let slope = half_angle.tan();
            [reach(v.iter().map(|h| (radius_at_origin + h * slope).abs()).fold(0.0, f64::max)), along(half_angle.cos().abs())]
        }
        BSurface::Sphere { radius, .. } => [reach(radius.abs()), reach(radius.abs())],
        BSurface::Torus { major_radius, minor_radius, .. } => [reach(major_radius.abs() + minor_radius.abs()), reach(minor_radius.abs())],
        _ => [0.0, 0.0],
    }
}

/// A face's pcurves on the form `form::surface_within` made for it (`form`, over `domain` on
/// the label `label`), put on the form's range. Where the domain ends within `tolerance` of a
/// quarter's end the form ends there, but the boundary still lies where its edges do, as much as
/// `tolerance` past it: a pcurve control point past the form's end by no more than the form
/// parameter of that angle (less than the angle: a quarter's parameter changes by at most 1/√2
/// per radian, [`form_reach`]) is moved onto the end. Along a cylinder's or cone's height the form
/// ends where the domain does, and a pcurve's end fitted a few µm past it (an edge's end that lies
/// within its tolerance of the vertex, not on it) is moved onto it where that height moves the
/// surface by no more than `tolerance`. A B-spline is a convex combination of its
/// control points, so each pcurve then lies on the form, and the face's domain, which ends at
/// the form's, holds it; the pcurve moves by no more than its points did, a gap the edges'
/// tolerances are measured to cover (`raise_edges`). A point further out is left as it is, for
/// the record check to refuse. The number of control point coordinates moved.
pub(crate) fn onto_form(form: &BSurface, label: &BSurface, domain: [[f64; 2]; 2], tolerance: f64, pcs: &mut [Vec<Curve2>]) -> usize {
    let reach = form_reach(label, domain[1], tolerance);
    let ranges = solidcore_ops::label::surface_natural(form);
    let mut moved = 0;
    // A move along one direction by its whole reach moves the surface by up to `tolerance`; moves
    // along both add (the triangle inequality), so a point is moved along a second direction only
    // while the two moves' shares of their reaches sum to at most one.
    let mut onto = |p: &mut [f64; 2]| {
        let mut spent = 0.0;
        for (d, reach) in reach.iter().enumerate() {
            let (Some(Natural::Range(a, b)), Some(x)) = (ranges.get(d), p.get_mut(d)) else { continue };
            let end = if *x < *a {
                *a
            } else if *x > *b {
                *b
            } else {
                continue;
            };
            let share = (*x - end).abs() / *reach;
            if *reach > 0.0 && spent + share <= 1.0 {
                spent += share;
                *x = end;
                moved += 1;
            }
        }
    };
    for c in pcs.iter_mut().flatten() {
        match c {
            Curve2::Spline { points, .. } => points.iter_mut().for_each(&mut onto),
            Curve2::RationalSpline { homogeneous, .. } => {
                for h in homogeneous.iter_mut().filter(|h| h[2] > 0.0) {
                    let mut p = [h[0] / h[2], h[1] / h[2]];
                    let before = p;
                    onto(&mut p);
                    if p != before {
                        (h[0], h[1]) = (p[0] * h[2], p[1] * h[2]);
                    }
                }
            }
        }
    }
    moved
}

/// A spline face's pcurves put on the surface's knot range where they run a little past it, when
/// that moves the surface within their edge's tolerance (`tolerances`, as `pcs`): the parameter
/// tolerance that edge has on this patch. The kernel moves the control points and proves the
/// bound, along u and v at once ([`SurfaceEval::pcurve_onto_knots`]); the gap to the edge is then
/// measured and covered (`raise_edges`). A pcurve it refuses (farther past, past both ranges of
/// a rational patch, or on a closed patch) is left as it is: the patch is continued to reach it
/// where it can be ([`extend`]), else the record check refuses it. The number of moves made.
pub(crate) fn onto_knots(surface: &BSurface, tolerances: &[Vec<f64>], pcs: &mut [Vec<Curve2>]) -> usize {
    let Ok(on) = SurfaceEval::new(surface) else { return 0 };
    let mut moved = 0;
    for (tl, pc) in tolerances.iter().zip(pcs.iter_mut()) {
        for (&tolerance, c) in tl.iter().zip(pc.iter_mut()) {
            if let Ok(Some((onto, _))) = on.pcurve_onto_knots(c, tolerance) {
                *c = onto;
                moved += 1;
            }
        }
    }
    moved
}

/// The parameter box around every pcurve's control points (`[[u_lo, u_hi], [v_lo, v_hi]]`): a
/// pcurve lies within it. Empty (lows above highs) where there are none.
pub(crate) fn pcurve_box(pcs: &[Vec<Curve2>]) -> [[f64; 2]; 2] {
    let mut out = [[f64::INFINITY, f64::NEG_INFINITY]; 2];
    for c in pcs.iter().flatten() {
        let pts: Vec<[f64; 2]> = match c {
            Curve2::Spline { points, .. } => points.clone(),
            Curve2::RationalSpline { homogeneous, .. } => homogeneous.iter().filter(|h| h[2] > 0.0).map(|h| [h[0] / h[2], h[1] / h[2]]).collect(),
        };
        for p in pts {
            for (o, x) in out.iter_mut().zip(p) {
                o[0] = o[0].min(x);
                o[1] = o[1].max(x);
            }
        }
    }
    out
}

/// A face's pcurves on a surface that closes on itself (`surface`, its loops `edge_loops`, as
/// `pcs`) put on the surface's range along each closed direction where the fit leaves one past
/// it by more than a billionth of the period (as the base period holds a trim, and the seam
/// completion moves a trim further out by whole periods), but by no more than the use misses
/// the uses beside it in its loop at its own two vertices: its control points past an end move
/// onto that end. Left there, the seam completion moves the use a whole period, to the seam's
/// other side, a period from the uses it joins (face #553893 of the Pro Shop file, a period of
/// 0.0215 and a seam use 4.3e-11 past it). A use further past crosses the seam, and a loop of
/// one use has no neighbour: those stay. A B-spline is a convex combination of its control
/// points, so the pcurve then lies on the range, and moves by no more than its points did; the
/// gap to the edge is measured and covered (`raise_edges`). The number of uses moved.
pub(crate) fn onto_seam(surface: &BSurface, edge_loops: &[Vec<(u32, bool)>], pcs: &mut [Vec<Curve2>]) -> usize {
    let Ok(on) = SurfaceEval::new(surface) else { return 0 };
    let domain = on.domain();
    let mut moved = 0;
    for (l, pc) in edge_loops.iter().zip(pcs.iter_mut()) {
        let n = pc.len();
        if n < 2 || l.len() != n {
            continue;
        }
        // Where each use starts and ends, in the loop's direction.
        let ends: Vec<Option<([f64; 2], [f64; 2])>> = l
            .iter()
            .zip(pc.iter())
            .map(|(&(_, forward), c)| {
                let [a, b] = c.interval()?;
                let (s, e) = (solidcore_ops::pcurve::eval(c, a)?, solidcore_ops::pcurve::eval(c, b)?);
                Some(if forward { (s, e) } else { (e, s) })
            })
            .collect();
        for d in 0..2 {
            let (Some(Some(period)), Some(Some((lo, hi)))) = (domain.period.get(d), domain.range.get(d)) else { continue };
            let (lo, hi) = (lo.min(*hi), lo.max(*hi));
            let round = 1e-9 * period;
            if !(round > 0.0 && lo.is_finite() && hi.is_finite()) {
                continue;
            }
            for k in 0..n {
                let (before, after) = ((k + n - 1) % n, (k + 1) % n);
                let (Some(Some((start, end))), Some(Some((_, prev_end))), Some(Some((next_start, _)))) =
                    (ends.get(k), ends.get(before), ends.get(after))
                else {
                    continue;
                };
                // How far it misses the uses beside it at its vertices.
                let at = |q: &[f64; 2]| q.get(d).copied().unwrap_or(f64::NAN);
                let miss = (at(start) - at(prev_end)).abs().max((at(end) - at(next_start)).abs());
                let Some(c) = pc.get_mut(k) else { continue };
                let xs: Vec<f64> = control_points(c).iter().map(at).collect();
                let past = xs.iter().fold(0.0f64, |m, &x| m.max(lo - x).max(x - hi));
                if !(past > round && past <= miss) {
                    continue;
                }
                let onto = |x: f64| x.max(lo).min(hi);
                match c {
                    Curve2::Spline { points, .. } => points.iter_mut().filter_map(|p| p.get_mut(d)).for_each(|x| *x = onto(*x)),
                    Curve2::RationalSpline { homogeneous, .. } => {
                        for h in homogeneous.iter_mut().filter(|h| h[2] > 0.0) {
                            let w = h[2];
                            if let Some(x) = h.get_mut(d) {
                                let y = onto(*x / w);
                                if y != *x / w {
                                    *x = y * w;
                                }
                            }
                        }
                    }
                }
                moved += 1;
            }
        }
    }
    moved
}

/// The control points of `c` in (u, v).
fn control_points(c: &Curve2) -> Vec<[f64; 2]> {
    match c {
        Curve2::Spline { points, .. } => points.clone(),
        Curve2::RationalSpline { homogeneous, .. } => homogeneous.iter().map(|h| [h[0] / h[2], h[1] / h[2]]).collect(),
    }
}

/// The parameter box around every pcurve's control points, padded a little, within the
/// surface's natural domain (at most one turn where periodic).
pub(crate) fn face_domain(surface: &BSurface, pcs: &[Vec<Curve2>]) -> R<[[f64; 2]; 2]> {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for c in pcs.iter().flatten() {
        let pts: Vec<[f64; 2]> = match c {
            Curve2::Spline { points, .. } => points.clone(),
            Curve2::RationalSpline { homogeneous, .. } => homogeneous.iter().filter(|h| h[2] > 0.0).map(|h| [h[0] / h[2], h[1] / h[2]]).collect(),
        };
        for p in pts {
            for ((lo, hi), x) in lo.iter_mut().zip(hi.iter_mut()).zip(p) {
                *lo = lo.min(x);
                *hi = hi.max(x);
            }
        }
    }
    let natural = solidcore_ops::label::surface_natural(surface);
    let mut out = [[0.0; 2]; 2];
    for (d, (a, b)) in lo.into_iter().zip(hi).enumerate() {
        if !(a.is_finite() && b.is_finite() && a <= b) {
            return Err("a face without pcurves".into());
        }
        let pad = 1e-6 * (1.0 + (b - a)) + 1e-9 * a.abs().max(b.abs());
        let (mut x, mut y) = (a - pad, b + pad);
        match natural.get(d) {
            Some(Natural::Periodic) => {
                if y - x > TAU {
                    x = a;
                    y = a + TAU;
                }
            }
            // A direction not closed on itself: the box is the reader's to choose within the
            // natural range (all of it for a plane, or along a cylinder's axis), and SMLib
            // refuses a surface whose opposite sides lie within its tolerance of each other
            // (`SmSurface` test #6, "partially coincident boundaries"), so a sliver face's box
            // is at least `OPEN_DOMAIN_SPAN` across where its range has room: widened about its
            // middle, and shifted off a bound it would cross. Pcurves wholly outside the range
            // stay an empty box.
            Some(Natural::Range(n0, n1)) => {
                x = x.max(*n0);
                y = y.min(*n1);
                if x < y && y - x < OPEN_DOMAIN_SPAN {
                    let span = OPEN_DOMAIN_SPAN.min(*n1 - *n0);
                    // the box's latest start: the range's own where the range is narrower than
                    // the span (`n1 - span` there is `n0`, which rounding may put below it)
                    let last = if span < OPEN_DOMAIN_SPAN { *n0 } else { *n1 - span };
                    // the range is the surface's, from the file: refused where it leaves no
                    // room (crossed, or not a number), never clamped into one
                    if !(*n0 <= last) {
                        return Err(format!("a face's natural range [{n0}, {n1}] leaves no room for a box {span} across"));
                    }
                    let lo = (0.5 * (x + y) - 0.5 * span).clamp(*n0, last);
                    (x, y) = (lo, (lo + span).min(*n1));
                }
            }
            None => {}
        }
        if !(x < y) {
            return Err("an empty face domain".into());
        }
        if let Some(slot) = out.get_mut(d) {
            *slot = [x, y];
        }
    }
    Ok(out)
}

/// An edge on a closed (periodic) spline over `interval` in the spline's parameter, which may
/// lie a whole number of periods from the curve's own range and run across its seam (a producer
/// that evaluates a periodic spline modulo its period): the curve and interval the record takes,
/// the interval moved into the range by whole periods, and the curve restarted there (exactly:
/// knot insertion) when the edge runs across the seam, as the STEP reader reads such an edge.
/// An end a rounding from the seam is at the seam: an edge all the way round, start to end, is
/// the curve as it is.
pub fn periodic_edge(curve: BCurve, [t0, t1]: [f64; 2]) -> R<(BCurve, [f64; 2])> {
    let Natural::Range(a, b) = curve.natural() else { return Err("a periodic spline without a range".into()) };
    let period = b - a;
    // Rounding is allowed for (`eps`): an edge all the way round may be written a rounding long.
    let eps = 1e-9 * period;
    if !(period > 0.0) || !(t0 <= t1) || !(t1 - t0 <= period + eps) {
        return Err("an edge longer than its periodic spline's period".into());
    }
    let turns = ((t0 - a) / period).floor();
    let (mut lo, mut hi) = (t0 - turns * period, t1 - turns * period);
    // Moving by whole periods rounds: a start a rounding short of the range's end is the seam
    // (the range's start), and an end a rounding past it is that end, as the STEP reader takes an
    // end within a rounding of its seam. A restart is only ever strictly inside the range. An edge
    // that ends before the range does (a short one a rounding short of it) stays where it is.
    if b - lo <= eps && hi > b {
        (lo, hi) = (a, hi - period);
    }
    if lo - a <= eps {
        lo = a;
    }
    if hi <= b + eps {
        return Ok((curve, [lo, hi.min(b)]));
    }
    Ok((crate::kernel::restart_curve(&curve, lo)?, [lo, hi]))
}

/// The exact spline of a parabola edge over `interval` in the parabola's parameter: about
/// `vertex`, `axis` its plane's normal, `reference` its axis of symmetry, `focal` its focal
/// distance (`vertex + f t² x + 2 f t y`).
pub fn parabola_curve(vertex: P3, axis: P3, reference: P3, focal: f64, interval: [f64; 2]) -> BCurve {
    crate::topology::parabola_curve(&crate::geom::Placement { origin: vertex, axis, ref_direction: reference }, focal, interval)
}

/// The exact spline of a hyperbola edge over `interval` in the hyperbola's parameter: about
/// `center`, `axis` its plane's normal, `reference` its major axis (`center + a cosh t x +
/// b sinh t y`). Its ends and knots are at the hyperbola's parameters; between knots its
/// parameter is not the hyperbola's (see `topology::hyperbola_curve`).
pub fn hyperbola_curve(center: P3, axis: P3, reference: P3, radii: [f64; 2], interval: [f64; 2]) -> BCurve {
    crate::topology::hyperbola_curve(&crate::geom::Placement { origin: center, axis, ref_direction: reference }, radii[0], radii[1], interval)
}

/// A B-spline curve of `degree` on its full knot vector `knots` (one knot per entry, `points`
/// plus `degree` plus one of them) over `points`, rational with `weights` when given: the
/// record's spline, an unclamped knot vector (a periodic spline written with uniform knots)
/// clamped exactly ([`clamped`]).
pub fn spline_curve(degree: usize, knots: Vec<f64>, points: &[[f64; 3]], weights: Option<&[f64]>) -> R<BCurve> {
    check_spline(degree, knots.len(), points.len())?;
    check_values(degree, points.len(), &knots, points.iter(), weights.into_iter().flatten().copied())?;
    let homogeneous: Vec<[f64; 4]> = match weights {
        None => points.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect(),
        Some(w) => {
            if w.len() != points.len() {
                return Err("B-spline weights and points differ in number".into());
            }
            points.iter().zip(w).map(|(p, w)| [p[0] * w, p[1] * w, p[2] * w, *w]).collect()
        }
    };
    finite_homogeneous(&homogeneous)?;
    let (knots, homogeneous) = clamped(degree, knots, homogeneous).map_err(|e| format!("an unclamped B-spline curve: {e}"))?;
    finite_homogeneous(&homogeneous)?;
    Ok(match weights {
        None => BCurve::Spline { knots, points: homogeneous.iter().map(|h| [h[0], h[1], h[2]]).collect() },
        Some(_) => BCurve::RationalSpline { knots, homogeneous },
    })
}

/// A B-spline surface of degrees `[u, v]` on full knot vectors `[u, v]` over `points` (row i
/// along u, entry j along v), rational with `weights` when given: the record's spline surface,
/// unclamped knots in either direction clamped exactly, line by line.
pub fn spline_surface(
    [u_degree, v_degree]: [usize; 2],
    [u_knots, v_knots]: [Vec<f64>; 2],
    points: &[Vec<[f64; 3]>],
    weights: Option<&[Vec<f64>]>,
) -> R<BSurface> {
    let nu = points.len();
    let nv = points.first().map_or(0, Vec::len);
    check_spline(u_degree, u_knots.len(), nu)?;
    check_spline(v_degree, v_knots.len(), nv)?;
    check_values(u_degree, nu, &u_knots, std::iter::empty(), std::iter::empty())?;
    check_values(v_degree, nv, &v_knots, points.iter().flatten(), weights.into_iter().flatten().flatten().copied())?;
    if points.iter().any(|r| r.len() != nv) {
        return Err("ragged B-spline surface".into());
    }
    if let Some(w) = weights
        && (w.len() != nu || w.iter().any(|r| r.len() != nv))
    {
        return Err("B-spline surface weights and points differ in number".into());
    }
    // Control point (i, j) is row i (along u), entry j (along v), homogeneous.
    let weight = |i: usize, j: usize| weights.and_then(|w| w.get(i)?.get(j).copied()).unwrap_or(1.0);
    let homogeneous = |i: usize, j: usize, p: &[f64; 3]| {
        let w = weight(i, j);
        [p[0] * w, p[1] * w, p[2] * w, w]
    };
    let rows: Vec<Vec<[f64; 4]>> =
        points.iter().enumerate().map(|(i, r)| r.iter().enumerate().map(|(j, p)| homogeneous(i, j, p)).collect()).collect();
    // Unclamped knots in either direction are clamped exactly, line by line: every row is a
    // spline in v on the v knots, every column one in u on the u knots, and inserting the same
    // knots into each gives the surface's.
    finite_homogeneous(rows.iter().flatten())?;
    let (v_knots, rows) = clamped_lines(v_degree, v_knots, rows)?;
    let (u_knots, columns) = clamped_lines(u_degree, u_knots, transposed(&rows))?;
    let rows = transposed(&columns);
    finite_homogeneous(rows.iter().flatten())?;
    Ok(match weights {
        None => BSurface::Spline { u_knots, v_knots, points: rows.iter().map(|r| r.iter().map(|h| [h[0], h[1], h[2]]).collect()).collect() },
        Some(_) => BSurface::RationalSpline { u_knots, v_knots, homogeneous: rows },
    })
}

/// Every homogeneous control point a finite number: finite coordinates times finite weights can
/// still overflow (`f64::MAX` times 2), and so can clamping's knot insertion.
fn finite_homogeneous<'a>(points: impl IntoIterator<Item = &'a [f64; 4]>) -> R<()> {
    if points.into_iter().any(|h| h.iter().any(|x| !x.is_finite())) {
        return Err("a B-spline whose weighted control points are not finite numbers".into());
    }
    Ok(())
}

/// Knots that do not decrease and leave the spline of `degree` over `count` points a domain of
/// some width, control points and weights that are finite numbers, and weights above zero.
fn check_values<'a>(
    degree: usize,
    count: usize,
    knots: &[f64],
    points: impl Iterator<Item = &'a [f64; 3]>,
    weights: impl Iterator<Item = f64>,
) -> R<()> {
    if knots.iter().any(|k| !k.is_finite()) || knots.windows(2).any(|w| matches!(w, [a, b] if b < a)) {
        return Err("a B-spline whose knots are not finite and increasing".into());
    }
    if !matches!((knots.get(degree), knots.get(count)), (Some(a), Some(b)) if a < b) {
        return Err("a B-spline whose parameter domain is empty".into());
    }
    let mut points = points;
    if points.any(|p| p.iter().any(|x| !x.is_finite())) {
        return Err("a B-spline control point that is not a finite number".into());
    }
    let mut weights = weights;
    if weights.any(|w| !(w.is_finite() && w > 0.0)) {
        return Err("a B-spline weight that is not a positive number".into());
    }
    Ok(())
}

/// A spline of `degree` with `knots` knots over `count` control points, within the record's
/// budgets.
fn check_spline(degree: usize, knots: usize, count: usize) -> R<()> {
    if !(1..=MAX_SPLINE_DEGREE).contains(&degree) {
        return Err(unsupported(format!("a B-spline of degree {degree}")));
    }
    if !(2..=MAX_SPLINE_POINTS).contains(&count) {
        return Err(unsupported(format!("a B-spline with {count} control points")));
    }
    if Some(knots) != count.checked_add(degree + 1) {
        return Err(format!("B-spline with {knots} knots for {count} points of degree {degree}"));
    }
    Ok(())
}

/// A spline of `degree` on `knots` over `homogeneous` control points with an unclamped knot
/// vector (a periodic spline written with uniform knots, its first points repeated at its end) as
/// the same spline over [knots[degree], knots[n]] clamped there, exactly (knot insertion): its
/// parameters unchanged, its ends its own points, as every evaluator and pcurve takes a spline,
/// and a closed one's first and last points one. A clamped one comes back as it is.
pub(crate) fn clamped(degree: usize, knots: Vec<f64>, homogeneous: Vec<[f64; 4]>) -> Result<(Vec<f64>, Vec<[f64; 4]>), String> {
    let (p, n) = (degree, homogeneous.len());
    let (lo, hi) = (knots.get(p).copied(), knots.get(n).copied());
    let repeats = |at: &[f64], x: Option<f64>| x.is_some_and(|x| at.iter().all(|k| *k == x));
    match (knots.get(..=p), knots.get(n..), lo, hi) {
        (Some(head), Some(tail), Some(a), Some(z)) if !(repeats(head, lo) && repeats(tail, hi)) => {
            solidcore::eval::pcurve::segment(p, &knots, &homogeneous, a, z).map_err(|e| e.to_string())
        }
        _ => Ok((knots, homogeneous)),
    }
}

/// Each of `lines`, a spline of `degree` on `knots`, clamped ([`clamped`]), and the knots they
/// share after it.
fn clamped_lines(degree: usize, knots: Vec<f64>, lines: Vec<Vec<[f64; 4]>>) -> R<(Vec<f64>, Vec<Vec<[f64; 4]>>)> {
    let mut out = Vec::new();
    let mut shared = None;
    for line in lines {
        let (k, c) = clamped(degree, knots.clone(), line).map_err(|e| format!("an unclamped B-spline surface: {e}"))?;
        shared = Some(k);
        out.push(c);
    }
    Ok((shared.unwrap_or(knots), out))
}

/// `grid`'s columns as rows.
pub(crate) fn transposed(grid: &[Vec<[f64; 4]>]) -> Vec<Vec<[f64; 4]>> {
    let width = grid.first().map_or(0, Vec::len);
    (0..width).map(|j| grid.iter().filter_map(|r| r.get(j).copied()).collect()).collect()
}

#[cfg(test)]
#[path = "assembly_tests.rs"]
mod tests;
