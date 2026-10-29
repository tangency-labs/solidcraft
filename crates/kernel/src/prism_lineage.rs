//! The history of a prism boolean's result (see [`super::prism_boolean`]), from what its faces
//! were built over: each wall over a section segment whose [`Tag`] says which operand wall it
//! was read beside, each cap over a section region lying on operand caps at its level. Edges
//! follow from the same tags: a wall's edge with a cap is the operand's edge between them (or
//! the meeting of the wall with the other operand's cap); an edge between walls at a section
//! vertex is the operand's edge there (or the meeting of two operands' walls).

use std::collections::{BTreeSet, HashMap};

use solidcraft_geom::{Plane, Region2, Vec2, Vec3};
use truck_modeling as mt;

use super::{Index, Section, Tag, dist_to, inside};
use crate::BoolOp;
use crate::body::{Body, edge_order, from_p3};
use crate::history::{EdgeOrigin, FaceOrigin, FaceOrigin::Face, FaceProv, Side};

/// A cap of an operand: a planar face square to the prism direction.
struct Cap {
    origin: FaceOrigin,
    /// Its level and whether it faces along the direction.
    z: f64,
    up: bool,
    outline: Outline,
}

/// A section's regions with the walls its outline was read beside and its segments' middles,
/// to tell whether two sections overlap.
struct Outline {
    regions: Vec<Region2>,
    walls: BTreeSet<(usize, usize)>,
    middles: Vec<Vec2>,
}

impl Outline {
    fn of(s: &Section) -> Outline {
        let (segs, tags) = s.segments();
        Outline {
            regions: s.regions.clone(),
            walls: tags.iter().filter_map(|t| Some((t.operand, t.wall?))).collect(),
            middles: segs.iter().map(|g| g.point_at(0.5)).collect(),
        }
    }
}

/// What a prism boolean's result is built from: the operands (target, tool), the direction and
/// the plane square to it that sections are drawn in, and for simple prisms each operand's
/// extent along the direction.
pub(super) struct Context<'a> {
    pub op: BoolOp,
    pub operands: [&'a Body; 2],
    pub index: [&'a Index; 2],
    pub plane: Plane,
    pub tol: f64,
    pub heights: Option<[(f64, f64); 2]>,
    /// Both operands' caps, read once (see [`Context::caps`]).
    caps: std::cell::OnceCell<Option<Vec<Cap>>>,
}

/// How one piece of the result was built.
pub(super) enum Built {
    /// One body of `crate::extrude` of a section's one region through `layers`: successive
    /// slabs (the section as read in each, between its two levels), bottom first, all of one
    /// outline. Several where operands stacked end to end share an outline (one wall then lies
    /// over each operand's wall in turn, and is made from all of them).
    Extruded { layers: Vec<(Section, f64, f64)> },
    /// Sewn (`sew_connected_sections`) from these faces, the sewing saying which each face of
    /// the piece was built from (`None`: it could not).
    Sewn { specs: Vec<Spec>, faces: Option<Vec<usize>> },
    /// Built some other way, carrying its own history over the operands (none: not known).
    Traced,
}

/// A face sewn into a stepped prism.
pub(super) enum Spec {
    /// A wall over one section segment through successive slabs, between successive `levels`
    /// (one more than `slabs`): what it is over in each slab.
    Wall { levels: Vec<f64>, slabs: Vec<Slab> },
    /// A cap over `section` at level `z`, facing along the direction when `up`.
    Cap { section: Section, z: f64, up: bool },
}

/// A wall in one slab: over a segment tagged `tag`; `next` is the wall over the next segment
/// of its loop in that slab, which it meets at `at`.
pub(super) struct Slab {
    pub tag: Tag,
    pub next: usize,
    pub at: Vec2,
}

impl Spec {
    /// The wall's slab between levels `lo` and `hi` (`hi` above `lo`).
    fn slab(&self, lo: f64, hi: f64) -> Option<&Slab> {
        let Spec::Wall { levels, slabs } = self else { return None };
        let mid = (lo + hi) / 2.0;
        slabs.iter().zip(levels.windows(2)).find(|(_, z)| z[0] < mid && mid < z[1]).map(|(s, _)| s)
    }

    /// The wall's slab at its bottom level, when that is `z`.
    fn bottom(&self, z: f64, tol: f64) -> Option<&Slab> {
        let Spec::Wall { levels, slabs } = self else { return None };
        ((levels.first()? - z).abs() <= tol).then_some(slabs.first()?)
    }

    /// The wall's slab at its top level, when that is `z`.
    fn top(&self, z: f64, tol: f64) -> Option<&Slab> {
        let Spec::Wall { levels, slabs } = self else { return None };
        ((levels.last()? - z).abs() <= tol).then_some(slabs.last()?)
    }

    /// The wall's slab at its bottom or top level `z`.
    fn end(&self, z: f64, tol: f64) -> Option<&Slab> {
        self.bottom(z, tol).or_else(|| self.top(z, tol))
    }
}

/// A face of an extruded piece: a wall or a cap.
#[derive(Clone, Copy)]
enum Kind {
    /// The wall over this segment of the section.
    Wall(usize),
    Cap(usize),
}

impl<'a> Context<'a> {
    pub(super) fn new(op: BoolOp, operands: [&'a Body; 2], index: [&'a Index; 2], plane: Plane, tol: f64, heights: Option<[(f64, f64); 2]>) -> Self {
        Context { op, operands, index, plane, tol, heights, caps: std::cell::OnceCell::new() }
    }

    /// `body`, made of `pieces` (their shells in order), with its history over the operands
    /// (none where it is not known).
    pub(super) fn attach<B: std::borrow::Borrow<Body>>(&self, body: Body, pieces: &[(B, Built)]) -> Body {
        let pieces: Vec<(&Body, &Built)> = pieces.iter().map(|(b, k)| (b.borrow(), k)).collect();
        let (faces, edges) = self.history(&pieces).unzip();
        crate::record::traced(body, &crate::record::counts(&self.operands), faces, edges.as_ref())
    }

    /// The faces (each a region of operand faces, and whether it is one untouched) and the edges
    /// (by identity) of the pieces, in order; `None` where something is not known.
    pub(super) fn history(&self, pieces: &[(&Body, &Built)]) -> Option<(Vec<FaceProv>, HashMap<mt::EdgeID, Vec<EdgeOrigin>>)> {
        let caps = self.caps.get_or_init(|| self.caps()).as_deref()?;
        let mut faces = Vec::new();
        let mut edges = HashMap::new();
        for (body, built) in pieces {
            let (f, e) = match built {
                Built::Extruded { layers } => self.extruded(caps, body, layers)?,
                Built::Sewn { specs, faces } => self.sewn(caps, body, specs, faces.as_deref()?)?,
                Built::Traced => {
                    let h = body.history()?;
                    faces.extend(h.faces.iter().cloned());
                    edges.extend(crate::record::edges_by_identity(body, h));
                    continue;
                }
            };
            faces.extend(f.into_iter().map(|(origins, whole)| crate::record::region(origins, whole)));
            edges.extend(e);
        }
        // Pieces of one body can share an origin.
        crate::record::mark_shared_split(&mut faces);
        Some((faces, edges))
    }

    /// Both operands' caps, their outlines tagged.
    fn caps(&self) -> Option<Vec<Cap>> {
        let d = self.plane.normal();
        let mut caps = Vec::new();
        let (mut work, mut segments) = (2_000_000usize, super::MAX_PLANAR_SEGMENTS * 2);
        for (operand, (body, index)) in self.operands.iter().zip(self.index).enumerate() {
            for (face, f) in body.solid.face_iter().enumerate() {
                let mt::Surface::Plane(p) = f.oriented_surface() else { continue };
                let n = p.normal();
                let n = Vec3::new(n.x, n.y, n.z).normalized()?;
                if n.dot(d).abs() < 1.0 - 1e-9 {
                    continue;
                }
                let section = Section::from_loops(index.loops(f, face, &self.plane, self.tol, &mut work, &mut segments)?)?;
                let (z, up) = (self.plane.height(from_p3(p.origin())), n.dot(d) > 0.0);
                caps.push(Cap { origin: Face { operand, face }, z, up, outline: Outline::of(&section.oriented()) });
            }
        }
        Some(caps)
    }

    /// The operand caps under a result cap over `section` at level `z` (facing along the
    /// direction when `up`): the target's caps there facing the same way, and the tool's facing
    /// the same way (a union or intersection) or the other way (a cut, whose tool's caps bound
    /// what it leaves), wherever they overlap the section.
    fn cap_origins(&self, caps: &[Cap], section: &Section, z: f64, up: bool) -> BTreeSet<FaceOrigin> {
        let outline = Outline::of(section);
        caps.iter()
            .filter(|c| {
                let facing = match c.origin {
                    Face { operand: 1, .. } if self.op == BoolOp::Cut => c.up != up,
                    _ => c.up == up,
                };
                facing && (c.z - z).abs() < self.tol && self.overlap(&outline, &c.outline)
            })
            .map(|c| c.origin)
            .collect()
    }

    /// Do two sections (at one level) overlap? Where their outlines share a wall, or a point of
    /// one's outline, clear of the other's, lies inside it.
    fn overlap(&self, x: &Outline, y: &Outline) -> bool {
        let within = |x: &Outline, y: &Outline| x.middles.iter().any(|m| dist_to(&y.regions, *m) > self.tol && inside(&y.regions, *m));
        !x.walls.is_disjoint(&y.walls)
            || within(x, y)
            || within(y, x)
            // Coincident outlines from different operands can have different
            // wall tags and every boundary midpoint on the other boundary.
            // A verified interior witness still proves positive area overlap.
            || x.regions.iter().chain(&y.regions).any(|region| {
                let p = region.interior_point();
                dist_to(&x.regions, p) > self.tol && dist_to(&y.regions, p) > self.tol && inside(&x.regions, p) && inside(&y.regions, p)
            })
    }

    /// The operand wall a segment was read beside.
    fn wall(t: Tag) -> Option<FaceOrigin> {
        Some(Face { operand: t.operand, face: t.wall? })
    }

    /// The operand's edges (by number) bounding exactly the faces `faces`.
    fn edges_between(&self, operand: usize, [f, g]: [usize; 2]) -> &[usize] {
        self.index.get(operand).and_then(|index| index.between.get(&(f.min(g), f.max(g)))).map_or(&[], Vec::as_slice)
    }

    /// Is the wall over a segment tagged `t` between `lo` and `hi` its operand wall untouched?
    /// The whole cap edge it was read off, the operand's whole height (simple prisms), and a
    /// wall with no other cap edges than its two.
    fn whole(&self, caps: &[Cap], t: Tag, lo: f64, hi: f64) -> bool {
        let (Some(heights), Some(wall), Some(_), Some(_)) = (self.heights, t.wall, t.start, t.end) else { return false };
        let Some((l, h)) = heights.get(t.operand) else { return false };
        let Some(index) = self.index.get(t.operand) else { return false };
        let cap_edges: usize = caps
            .iter()
            .filter_map(|c| match c.origin {
                Face { operand, face } if operand == t.operand => index.between.get(&(face.min(wall), face.max(wall))).map(Vec::len),
                _ => None,
            })
            .sum();
        (lo - l).abs() < self.tol && (hi - h).abs() < self.tol && cap_edges == 2
    }

    /// The origins of the edge between the wall over a segment tagged `t` and a cap lying on the
    /// operand faces `cap`: the operand's edge between the wall and its own cap there (the very
    /// edge the segment was read off, or the one it runs along where its slab ends: the one
    /// running through `at`, a point of the edge, where the wall meets its caps along several),
    /// else where the wall meets the cap (all of the faces it lies on).
    fn cap_edge(&self, t: Tag, cap: &BTreeSet<FaceOrigin>, at: Vec3) -> Option<Vec<EdgeOrigin>> {
        let (x, w) = (t.operand, t.wall?);
        let own: Vec<usize> =
            cap.iter().filter_map(|o| if let Face { operand, face } = *o { (operand == x).then_some(face) } else { None }).collect();
        let index = self.index.get(x)?;
        let beside = |e: &usize| index.bounds.get(*e).is_some_and(|(fs, _, _)| fs.contains(&w) && fs.iter().any(|f| own.contains(f)));
        if let Some(e) = [t.edge, t.far].into_iter().flatten().find(beside) {
            return Some(vec![EdgeOrigin::Edge { operand: x, edge: e }]);
        }
        let along: Vec<usize> = own.iter().flat_map(|c| self.edges_between(x, [w, *c])).copied().collect();
        match along.as_slice() {
            [e] => Some(vec![EdgeOrigin::Edge { operand: x, edge: *e }]),
            [] => Some(vec![EdgeOrigin::meet(Self::wall(t)?, Side::new(cap.iter().copied())?)]),
            // One wall with openings meets its caps along several edges (a side wall with ports
            // cut through it meets the ports' floors at each port): the one this edge lies on,
            // else (a new opening beside them) where the wall meets the cap.
            many => match self.edge_through(x, many, at)? {
                Some(edge) => Some(vec![EdgeOrigin::Edge { operand: x, edge }]),
                None => Some(vec![EdgeOrigin::meet(Self::wall(t)?, Side::new(cap.iter().copied())?)]),
            },
        }
    }

    /// Of the operand's edges `edges`, the one the point `at` lies on (`Some(None)` when none
    /// does; `None` when several do).
    fn edge_through(&self, operand: usize, edges: &[usize], at: Vec3) -> Option<Option<usize>> {
        if edges.len() > 4096 {
            return None;
        }
        let body = self.operands.get(operand)?;
        let order = crate::body::edge_order(body.solid.face_iter()).0;
        let mut on = edges.iter().filter(|e| {
            order.get(**e).is_some_and(|edge| {
                use mt::{BoundedCurve, ParametricCurve};
                let c = edge.curve();
                let (t0, t1) = c.range_tuple();
                // The edge as a fine polyline: a line exactly, an arc within its chords' sag.
                let n = if matches!(c, mt::Curve::Line(_)) { 1 } else { 64 };
                let pts: Vec<Vec3> = (0..=n).map(|k| from_p3(c.subs(t0 + (t1 - t0) * k as f64 / n as f64))).collect();
                let length: f64 = pts.windows(2).map(|w| w[0].dist(w[1])).sum();
                let near = self.tol * 10.0 + if n == 1 { 0.0 } else { length * 2e-3 };
                pts.windows(2).any(|w| at.dist_to_segment(w[0], w[1]) <= near)
            })
        });
        match (on.next(), on.next()) {
            (Some(e), None) => Some(Some(*e)),
            (None, _) => Some(None),
            _ => None,
        }
    }

    /// The origins of the edge between the walls over two segments where the first (tagged
    /// `t1`) runs into the second (`t2`), between levels `lo` and `hi`: the operand's edge there
    /// between its walls (along the line through the operand vertex the segments meet at, else
    /// the one edge between them), where two operands' walls meet, or none within one wall (a
    /// segment split there).
    fn junction(&self, t1: Tag, t2: Tag, lo: f64, hi: f64) -> Option<Vec<EdgeOrigin>> {
        let (p, q) = (Self::wall(t1)?, Self::wall(t2)?);
        if t1.operand != t2.operand {
            return Some(vec![EdgeOrigin::meet(p, q)]);
        }
        let (x, w1, w2) = (t1.operand, t1.wall?, t2.wall?);
        match (t1.end, t2.start) {
            // At that vertex: the operand's edge there (a seam, within one wall), else two edges
            // of one wall's cap outline running on through it.
            (Some(v), Some(u)) if v == u => self.edge_over(x, [w1, w2], Some(v), lo, hi).or_else(|| (w1 == w2).then(Vec::new)),
            _ if w1 == w2 => Some(Vec::new()),
            // Elsewhere (a segment split off there, or read off another cap): only the operand's
            // walls are around the edge, so the operand has its edge between them there, and
            // that is the one edge between them over these levels where there is one.
            _ => self.edge_over(x, [w1, w2], None, lo, hi),
        }
    }

    /// The operand's edge between the walls `faces` that runs over the levels `lo` to `hi`,
    /// when it has exactly one; with `at`, among the edges between them chained to that vertex.
    /// The segments' vertex was read at one level and carried through others, and the line
    /// between two walls is in several edges where vertices an earlier operation left (stations)
    /// split it: the levels say which of them a slab's edge runs along.
    fn edge_over(&self, operand: usize, faces: [usize; 2], at: Option<usize>, lo: f64, hi: f64) -> Option<Vec<EdgeOrigin>> {
        let index = self.index.get(operand)?;
        let between = self.edges_between(operand, faces);
        if between.len() > 4096 {
            return None;
        }
        let ends: Vec<(Option<usize>, Option<usize>)> =
            between.iter().map(|e| index.bounds.get(*e).map_or((None, None), |(_, front, back)| (*front, *back))).collect();
        // With `at`: the edges reached from it through shared vertices.
        let mut chained = vec![at.is_none(); between.len()];
        let mut reached: Vec<usize> = at.into_iter().collect();
        while let Some(p) = reached.pop() {
            for (k, (front, back)) in ends.iter().enumerate() {
                if !chained[k] && (*front == Some(p) || *back == Some(p)) {
                    chained[k] = true;
                    reached.extend(if *front == Some(p) { *back } else { *front });
                }
            }
        }
        let height = |v: Option<usize>| v.and_then(|v| index.points.get(v)).map(|p| self.plane.height(*p));
        let mut over = between.iter().zip(&ends).zip(chained).filter_map(|((e, (front, back)), chained)| {
            let (a, b) = (height(*front)?, height(*back)?);
            (chained && a.min(b) <= lo + self.tol && a.max(b) >= hi - self.tol).then_some(*e)
        });
        match (over.next(), over.next()) {
            (Some(edge), None) => Some(vec![EdgeOrigin::Edge { operand, edge }]),
            _ => None,
        }
    }

    /// The origins of the edge between walls over one segment stacked at a level (tagged `t1`
    /// below, `t2` above): within one wall (none), along the operand's edge between two of its
    /// walls, or where two operands' walls meet.
    fn stacked(&self, t1: Tag, t2: Tag) -> Option<Vec<EdgeOrigin>> {
        let (p, q) = (Self::wall(t1)?, Self::wall(t2)?);
        if p == q {
            return Some(Vec::new());
        }
        if t1.operand != t2.operand {
            return Some(vec![EdgeOrigin::meet(p, q)]);
        }
        // An edge across the direction at a level: the levels cannot tell edges apart here.
        self.one_edge(t1.operand, [t1.wall?, t2.wall?])
    }

    /// The operand's edge between the faces `faces`, when it has exactly one.
    fn one_edge(&self, operand: usize, faces: [usize; 2]) -> Option<Vec<EdgeOrigin>> {
        match self.edges_between(operand, faces) {
            [edge] => Some(vec![EdgeOrigin::Edge { operand, edge: *edge }]),
            _ => None,
        }
    }

    /// Faces and edges of a piece `crate::extrude` built from a section of one region through
    /// `layers` (see [`Built::Extruded`]). Its history says which faces are walls over which
    /// segment (numbered over the region's loops in order) and which are its caps; the start
    /// cap's wires run over the segments in loop order (as the extrude builds them), so the
    /// vertex after each wire edge is where one segment runs into the next, or splits. A wall
    /// through several layers is made from the wall each layer's segment was read beside, and
    /// an edge between two walls from the operand edges there in each layer.
    #[allow(clippy::type_complexity)]
    fn extruded(
        &self,
        caps: &[Cap],
        body: &Body,
        layers: &[(Section, f64, f64)],
    ) -> Option<(Vec<(BTreeSet<FaceOrigin>, bool)>, HashMap<mt::EdgeID, Vec<EdgeOrigin>>)> {
        let h = body.history()?;
        let ((first, lo, _), (last, _, hi)) = (layers.first()?, layers.last()?);
        let (lo, hi) = (*lo, *hi);
        let tags: Vec<Vec<Tag>> = layers.iter().map(|(section, ..)| section.segments().1).collect();
        let count = tags.first()?.len();
        if tags.iter().any(|t| t.len() != count) {
            return None;
        }
        let tag = |layer: usize, segment: usize| tags.get(layer).and_then(|t| t.get(segment)).copied();
        let tops = [self.cap_origins(caps, first, lo, false), self.cap_origins(caps, last, hi, true)];
        let kinds: Vec<Kind> = h
            .faces
            .iter()
            .map(|f| match f.origins.as_slice() {
                [FaceOrigin::Profile { segment }] => (*segment < count).then_some(Kind::Wall(*segment)),
                [FaceOrigin::Cap { end }] => Some(Kind::Cap(usize::from(*end))),
                _ => None,
            })
            .collect::<Option<_>>()?;
        let faces = kinds
            .iter()
            .map(|k| match *k {
                Kind::Wall(segment) => {
                    let walls = (0..layers.len()).map(|layer| Self::wall(tag(layer, segment)?)).collect::<Option<BTreeSet<_>>>()?;
                    let whole = match layers {
                        [(_, lo, hi)] => self.whole(caps, tag(0, segment)?, *lo, *hi),
                        _ => false,
                    };
                    Some((walls, whole))
                }
                Kind::Cap(end) => Some(tops.get(end).filter(|o| !o.is_empty())?.clone()).map(|o| (o, false)),
            })
            .collect::<Option<Vec<_>>>()?;
        let (edges, bounds) = edge_order(body.solid.face_iter());
        let position: HashMap<mt::EdgeID, usize> = edges.iter().enumerate().map(|(i, e)| (e.id(), i)).collect();
        // The segment of the wall beside an edge of the start cap.
        let segment_of = |e: &mt::Edge| -> Option<usize> {
            bounds.get(*position.get(&e.id())?)?.iter().find_map(|f| match h.faces.get(*f)?.origins.as_slice() {
                [FaceOrigin::Profile { segment }] => Some(*segment),
                _ => None,
            })
        };
        let start = kinds.iter().position(|k| matches!(k, Kind::Cap(0)))?;
        // At each vertex of the start cap: the segment running into it, the one running out,
        // and whether that is one segment the extrude split there.
        let mut junctions: HashMap<mt::VertexID, (usize, usize, bool)> = HashMap::new();
        for w in body.solid.face_iter().nth(start)?.absolute_boundaries() {
            let es: Vec<mt::Edge> = w.edge_iter().cloned().collect();
            let segs: Vec<usize> = es.iter().map(segment_of).collect::<Option<_>>()?;
            if segs.windows(2).any(|p| p.first() > p.get(1)) {
                return None;
            }
            for (k, e) in es.iter().enumerate() {
                let (s, next) = (*segs.get(k)?, *segs.get((k + 1) % segs.len())?);
                junctions.insert(e.back().id(), (s, next, s == next && k + 1 != segs.len()));
            }
        }
        let mut origins = HashMap::new();
        for (e, fs) in edges.iter().zip(&bounds) {
            let [f, g] = fs.as_slice() else { return None };
            let o = match (kinds.get(*f)?, kinds.get(*g)?) {
                (Kind::Wall(segment), Kind::Cap(end)) | (Kind::Cap(end), Kind::Wall(segment)) => {
                    let layer = if *end == 0 { 0 } else { layers.len() - 1 };
                    self.cap_edge(tag(layer, *segment)?, tops.get(*end)?, crate::heal::edge_points(e).get(2).copied()?)?
                }
                (Kind::Wall(..), Kind::Wall(..)) => {
                    let (s, next, split) = *[e.absolute_front().id(), e.absolute_back().id()].iter().find_map(|v| junctions.get(v))?;
                    let mut o = BTreeSet::new();
                    for (layer, (_, lo, hi)) in layers.iter().enumerate() {
                        let (t, u) = (tag(layer, s)?, tag(layer, next)?);
                        // A segment the extrude split in two runs on through the vertex between.
                        let (t, u) = if split { (Tag { end: None, ..t }, Tag { start: None, ..t }) } else { (t, u) };
                        o.extend(self.junction(t, u, *lo, *hi)?);
                    }
                    o.into_iter().collect()
                }
                _ => return None,
            };
            origins.insert(e.id(), o);
        }
        Some((faces, origins))
    }

    /// Faces and edges of a piece sewn from `specs`: the sewing reports which spec each face was
    /// built from. Walls meeting at a vertex of their loop (when two walls meet at both ends,
    /// the edge's vertex says which); walls over one segment meeting at a level; walls meeting
    /// caps.
    #[allow(clippy::type_complexity)]
    fn sewn(
        &self,
        caps: &[Cap],
        body: &Body,
        specs: &[Spec],
        built: &[usize],
    ) -> Option<(Vec<(BTreeSet<FaceOrigin>, bool)>, HashMap<mt::EdgeID, Vec<EdgeOrigin>>)> {
        let cap_of: Vec<Option<BTreeSet<FaceOrigin>>> = specs
            .iter()
            .map(|s| match s {
                Spec::Cap { section, z, up } => Some(self.cap_origins(caps, section, *z, *up)).filter(|o| !o.is_empty()),
                Spec::Wall { .. } => None,
            })
            .collect();
        let faces = built
            .iter()
            .map(|i| match specs.get(*i)? {
                Spec::Wall { slabs, .. } => Some((slabs.iter().map(|s| Self::wall(s.tag)).collect::<Option<_>>()?, false)),
                Spec::Cap { .. } => cap_of.get(*i)?.clone().map(|o| (o, false)),
            })
            .collect::<Option<Vec<_>>>()?;
        let (edges, bounds) = edge_order(body.solid.face_iter());
        let mut origins = HashMap::new();
        for (e, fs) in edges.iter().zip(&bounds) {
            let [f, g] = fs.as_slice() else { return None };
            let (i, j) = (*built.get(*f)?, *built.get(*g)?);
            let (x, y) = (specs.get(i)?, specs.get(j)?);
            // The edge's levels, lower first, and where it stands in plan.
            let (front, back) = (from_p3(e.absolute_front().point()), from_p3(e.absolute_back().point()));
            let (h1, h2) = (self.plane.height(front), self.plane.height(back));
            let (lo, hi) = (h1.min(h2), h1.max(h2));
            let middle = || crate::heal::edge_points(e).get(2).copied();
            let o = match (x, y) {
                (Spec::Wall { .. }, Spec::Cap { .. }) => self.cap_edge(x.end(lo, self.tol)?.tag, cap_of.get(j)?.as_ref()?, middle()?)?,
                (Spec::Cap { .. }, Spec::Wall { .. }) => self.cap_edge(y.end(lo, self.tol)?.tag, cap_of.get(i)?.as_ref()?, middle()?)?,
                (Spec::Wall { .. }, Spec::Wall { .. }) if hi - lo <= self.tol => {
                    // One over the other at a level: the lower first.
                    let (below, above) = match (x.top(lo, self.tol), y.bottom(lo, self.tol)) {
                        (Some(s), Some(t)) => (s, t),
                        _ => (y.top(lo, self.tol)?, x.bottom(lo, self.tol)?),
                    };
                    self.stacked(below.tag, above.tag)?
                }
                (Spec::Wall { .. }, Spec::Wall { .. }) => {
                    // Side by side within a slab: the vertex the edge stands on (in plan) says
                    // which way round they meet.
                    let (s, t) = (x.slab(lo, hi)?, y.slab(lo, hi)?);
                    let p = self.plane.to_local(front);
                    let at = |q: &Vec2| p.dist(*q) < self.tol * 10.0;
                    if s.next == j && at(&s.at) {
                        self.junction(s.tag, t.tag, lo, hi)?
                    } else if t.next == i && at(&t.at) {
                        self.junction(t.tag, s.tag, lo, hi)?
                    } else {
                        return None;
                    }
                }
                _ => return None,
            };
            origins.insert(e.id(), o);
        }
        Some((faces, origins))
    }
}
