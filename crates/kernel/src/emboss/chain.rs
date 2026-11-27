//! The wrap carried across a seam onto the planes tangent to a partial wall (#5358, Task 3). The
//! profiles are cut at the seam lines ([`super::strip`]); each piece is built on its own face of
//! the chain: on the wall as the wrap builds any segment, on a plane as an exact extrusion of the
//! sketch curve laid flat there. A profile across a seam stays one solid: its cap is a cylinder
//! piece and a plane piece meeting tangentially along a straight edge, and its footprint takes a
//! notch out of both faces, whose seam is split around it. Nothing is unioned: two pieces
//! touching along the seam are the case exact booleans get wrong (history 2026-11-20).
use std::collections::HashMap;

use super::flat::Flat;
use super::strip::{Crossing, Cut, Run, cut_loop, runs_in, trace};
use super::{Wrap, arr, fail, meets, refusal};
use crate::assemble::{AFace, AUse, Assembly};
use crate::body::Body;
use crate::history::{Change, FaceProv};
use crate::numbered::NumberedRegion;
use crate::{KernelError, Result};
use solidcore_record::brep::{Curve, Curve2, Frame, Surface};
use solidcore_record::{Loop2, Plane, Seg2, Vec2, Vec3};
use solidcraft_geom::prelude::*;

/// A vertex of the profiles' pieces: a loop's corner (region, loop, segment) or a crossing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Key {
    Corner(usize, usize, usize),
    Cross(usize),
}

/// What a zone of the strip lies on.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Zone {
    Wall,
    Flat(usize),
}

/// One boundary item of a loop in a zone: an edge use with its wall-coordinate segment, or a
/// stretch of a seam line between two crossings, made an edge once the loops are traced.
#[derive(Clone, Debug)]
enum Item {
    Use(AUse, Seg2, bool),
    Chord(Crossing, Crossing),
}

/// One piece of a profile loop as built: its footprint and cap edges, its side wall.
struct Built {
    base: AUse,
    top: AUse,
    seg: Seg2,
    exact: bool,
}

/// The wall and the planes the wrap reaches (sorted along x), seen in wall coordinates.
pub(super) struct Chain<'a> {
    wall: &'a Wrap,
    flats: Vec<Flat>,
    seams: Vec<f64>,
    zones: Vec<Zone>,
    rr: f64,
    depth: f64,
}

/// What the chain gives the emboss: the faces left of the wall and the planes (each with its
/// history), then the faces added (each with its history).
pub(super) struct Rebuilt {
    pub removed: Vec<usize>,
    pub faces: Vec<(AFace, FaceProv)>,
}

impl<'a> Chain<'a> {
    pub(super) fn new(wall: &'a Wrap, mut flats: Vec<Flat>, rr: f64, depth: f64) -> Chain<'a> {
        flats.sort_by(|a, b| a.x.total_cmp(&b.x));
        let seams = flats.iter().map(|f| f.x).collect();
        let zones = (0..=flats.len())
            .map(|i| {
                if flats.get(i).is_some_and(|f| f.side < 0.0) {
                    Zone::Flat(i)
                } else if i > 0 && flats.get(i - 1).is_some_and(|f| f.side > 0.0) {
                    Zone::Flat(i - 1)
                } else {
                    Zone::Wall
                }
            })
            .collect();
        Chain { wall, flats, seams, zones, rr, depth }
    }

    fn flat(&self, k: usize) -> Result<&Flat> {
        self.flats.get(k).ok_or_else(|| fail("tangent plane"))
    }

    /// The point at wall coordinates `p` in zone `zone`, `lift` out of the wall.
    fn point(&self, zone: Zone, p: Vec2, lift: f64) -> Result<Vec3> {
        Ok(match zone {
            Zone::Wall => self.wall.at(p, self.wall.r + lift),
            Zone::Flat(k) => {
                let f = self.flat(k)?;
                f.foot + f.dir * (f.side * (p.x - f.x)) + self.wall.axis * p.y + f.normal * lift
            }
        })
    }

    /// Plane `k` with wall coordinates as its own, `lift` out of it.
    fn plane(&self, k: usize, lift: f64) -> Result<Plane> {
        let f = self.flat(k)?;
        Plane::new(f.foot - f.dir * (f.side * f.x) + f.normal * lift, f.dir * f.side, self.wall.axis).ok_or_else(|| fail("tangent plane frame"))
    }

    /// Wall coordinates of a point on the face of zone `zone`.
    fn coords(&self, zone: Zone, p: Vec3, near: f64) -> Result<Vec2> {
        let w = self.wall;
        let z = (p - w.o).dot(w.axis);
        Ok(match zone {
            Zone::Wall => {
                let a = super::angle(p, w.o, w.axis, w.radial, w.tangent);
                // Onto the turn round `near` (the face's middle).
                let m = near / w.r;
                let a = m + (a - m + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
                Vec2::new(a * w.r, z)
            }
            Zone::Flat(k) => {
                let f = self.flat(k)?;
                Vec2::new(f.x + f.side * (p - f.foot).dot(f.dir), z)
            }
        })
    }

    /// One piece on plane `k`: its edges below and above, exact (the sketch curve laid on the
    /// plane), and its side wall, their extrusion along the plane's normal; `v` as in
    /// [`Wrap::fitted`].
    fn flat_piece(&self, asm: &mut Assembly, k: usize, seg: &Seg2, v: [u32; 6], shell: usize) -> Result<(AUse, AUse, AFace)> {
        let [ba, bb, ta, tb, ra, rb] = v;
        let f = *self.flat(k)?;
        let (below, above) = (self.plane(k, 0.0)?, self.plane(k, self.depth)?);
        let curve = |p: &Plane| crate::point_path::sketch_record_curve(p, seg).ok_or_else(|| fail("a profile curve on a tangent plane"));
        let ((cb, ib), (ct, it)) = (curve(&below)?, curve(&above)?);
        let eb = asm.edge(cb.clone(), ib, ba, bb)?;
        let et = asm.edge(ct, it, ta, tb)?;
        let up = f.normal * self.depth;
        let line = |a: [f64; 2], b: [f64; 2]| Some(Curve2::Spline { knots: vec![0.0, 0.0, 1.0, 1.0], points: vec![a, b] });
        let ruled_uses = || {
            vec![
                AUse { edge: eb, forward: true, given: line([0.0, 0.0], [1.0, 0.0]) },
                AUse { edge: rb, forward: true, given: line([1.0, 0.0], [1.0, 1.0]) },
                AUse { edge: et, forward: false, given: line([0.0, 1.0], [1.0, 1.0]) },
                AUse { edge: ra, forward: false, given: line([0.0, 0.0], [0.0, 1.0]) },
            ]
        };
        let plain_uses = || {
            vec![
                AUse { edge: eb, forward: true, given: None },
                AUse { edge: rb, forward: true, given: None },
                AUse { edge: et, forward: false, given: None },
                AUse { edge: ra, forward: false, given: None },
            ]
        };
        let (surface, natural_out, uses) = match (seg, &cb) {
            (Seg2::Line { a, b }, _) => {
                // Its natural normal along the loop's turning: out of the material, raised or sunk.
                let (pa, pb) = (below.to_world(*a), below.to_world(*b));
                let u = (pb - pa).normalized().ok_or_else(|| fail("a wall of no width"))?;
                let w = up.normalized().ok_or_else(|| fail("a wall of no height"))?;
                (Surface::Plane { origin: arr(pa), u_axis: arr(u), v_axis: arr(w) }, true, plain_uses())
            }
            (Seg2::Arc { center, .. }, Curve::Circle { frame, radius }) => {
                let c = below.to_world(*center);
                let x = Vec3::from(frame.x_axis);
                let y = Vec3::from(frame.y_axis);
                let mid = seg.point_at(0.5);
                let d = seg.derivative(0.5);
                let tangent = (below.x * d.x + below.y * d.y).normalized().ok_or_else(|| fail("an arc of no length"))?;
                let away = (below.to_world(mid) - c).normalized().ok_or_else(|| fail("an arc of no radius"))?;
                let surface = Surface::Cylinder { frame: Frame { origin: arr(c), x_axis: arr(x), y_axis: arr(y) }, radius: *radius };
                (surface, away.dot(tangent.cross(up)) > 0.0, plain_uses())
            }
            (_, Curve::Spline { knots, points }) => {
                let rows = points.iter().map(|p| vec![*p, arr(Vec3::from(*p) + up)]).collect();
                (Surface::Spline { u_knots: knots.clone(), v_knots: vec![0.0, 0.0, 1.0, 1.0], points: rows }, true, ruled_uses())
            }
            (_, Curve::RationalSpline { knots, homogeneous }) => {
                let rows = homogeneous.iter().map(|h| vec![*h, [h[0] + up.x * h[3], h[1] + up.y * h[3], h[2] + up.z * h[3], h[3]]]).collect();
                (Surface::RationalSpline { u_knots: knots.clone(), v_knots: vec![0.0, 0.0, 1.0, 1.0], homogeneous: rows }, true, ruled_uses())
            }
            _ => return Err(fail("a profile curve on a tangent plane")),
        };
        let face = AFace { surface, nurbs: None, natural_out, loops: vec![uses], shell, domain: None };
        Ok((AUse { edge: eb, forward: true, given: None }, AUse { edge: et, forward: true, given: None }, face))
    }

    /// The faces of the emboss across the chain: the wall face `selected` (record face `fi`) and
    /// the planes lose the footprints of the profiles `numbered` (wall coordinates), split where
    /// the profiles cross their seams, and each profile adds its side walls and its cap pieces.
    /// Each profile's solid, raised or sunk, must not meet the rest of `body`.
    pub(super) fn build(
        &self,
        asm: &mut Assembly,
        selected: &AFace,
        fi: usize,
        numbered: &[NumberedRegion],
        body: &Body,
        touch: f64,
    ) -> Result<Rebuilt> {
        let wall = self.wall;
        let shell = selected.shell;
        let raised = self.depth > 0.0;
        // The planes' faces, taken out of the assembly.
        let mut flat_faces = Vec::with_capacity(self.flats.len());
        for f in &self.flats {
            flat_faces.push(asm.faces.get_mut(f.face).and_then(Option::take).ok_or_else(|| fail("tangent plane face"))?);
        }
        // Cut every loop at the seams.
        let mut next_id = 0usize;
        let mut cuts: Vec<Vec<Vec<Cut>>> = Vec::with_capacity(numbered.len());
        for reg in numbered {
            let mut loops = Vec::new();
            for nl in reg.loops() {
                loops.push(cut_loop(&nl.lp, &self.seams, touch, &mut next_id)?);
            }
            cuts.push(loops);
        }
        // Vertices below and above every piece's start, and the line between them.
        let mut base_v: HashMap<Key, u32> = HashMap::new();
        let mut top_v: HashMap<Key, u32> = HashMap::new();
        let mut rise: HashMap<Key, u32> = HashMap::new();
        let mut start_keys: Vec<Vec<Vec<Key>>> = Vec::new();
        for (ri, (reg, loops)) in numbered.iter().zip(&cuts).enumerate() {
            let mut per_loop = Vec::new();
            for (li, (nl, cut)) in reg.loops().zip(loops).enumerate() {
                let n = cut.len();
                let mut keys = Vec::with_capacity(n);
                for k in 0..n {
                    let piece = cut.get(k).ok_or_else(|| fail("piece"))?;
                    let before = cut.get((k + n - 1) % n).ok_or_else(|| fail("piece"))?;
                    let (key, at, zone) = match before.ends {
                        Some(c) => {
                            let x = self.seams.get(c.line).copied().ok_or_else(|| fail("seam"))?;
                            (Key::Cross(c.id), Vec2::new(x, c.z), Zone::Wall)
                        }
                        None => {
                            let p = nl.lp.segs.get(piece.of).map(Seg2::start).ok_or_else(|| fail("corner"))?;
                            (Key::Corner(ri, li, piece.of), p, self.zone(piece.zone)?)
                        }
                    };
                    if let std::collections::hash_map::Entry::Vacant(e) = base_v.entry(key) {
                        let b = asm.vertex(self.point(zone, at, 0.0)?)?;
                        let t = asm.vertex(self.point(zone, at, self.depth)?)?;
                        e.insert(b);
                        top_v.insert(key, t);
                        rise.insert(key, asm.line(b, t)?);
                    }
                    keys.push(key);
                }
                per_loop.push(keys);
            }
            start_keys.push(per_loop);
        }
        let get = |m: &HashMap<Key, u32>, k: &Key| m.get(k).copied().ok_or_else(|| fail("piece vertex"));
        // Every piece's edges and side wall.
        let mut built: Vec<Vec<Vec<Built>>> = Vec::new();
        let mut added: Vec<(AFace, FaceProv)> = Vec::new();
        let mut walls_of: Vec<std::ops::Range<usize>> = Vec::new();
        for (ri, (reg, loops)) in numbered.iter().zip(&cuts).enumerate() {
            let start = added.len();
            let mut per_loop = Vec::new();
            for (li, (nl, cut)) in reg.loops().zip(loops).enumerate() {
                let keys = start_keys.get(ri).and_then(|l| l.get(li)).ok_or_else(|| fail("piece vertex"))?;
                let n = cut.len();
                let mut pieces = Vec::with_capacity(n);
                for (k, piece) in cut.iter().enumerate() {
                    let (Some(a), Some(b)) = (keys.get(k), keys.get((k + 1) % n)) else { return Err(fail("piece vertex")) };
                    let v = [get(&base_v, a)?, get(&base_v, b)?, get(&top_v, a)?, get(&top_v, b)?, get(&rise, a)?, get(&rise, b)?];
                    let seg = &piece.seg;
                    let (base, top, face, exact) = match self.zone(piece.zone)? {
                        Zone::Wall if wall.exact(seg) => {
                            let (b, t, f) = wall.exact_piece(asm, seg.start(), seg.end(), self.rr, v, shell)?;
                            (b, t, f, true)
                        }
                        Zone::Wall => {
                            wall.check_wall_form(selected, Some(seg))?;
                            let (b, t, f) = wall.fitted(asm, seg, seg.end(), self.rr, v, shell)?;
                            (b, t, f, false)
                        }
                        Zone::Flat(fk) => {
                            let (b, t, f) = self.flat_piece(asm, fk, seg, v, shell)?;
                            (b, t, f, true)
                        }
                    };
                    added.push((face, crate::record::profile(nl.numbers(piece.of))));
                    pieces.push(Built { base, top, seg: *seg, exact });
                }
                per_loop.push(pieces);
            }
            built.push(per_loop);
            walls_of.push(start..added.len());
        }
        // The caps: each profile's pieces above, zone by zone, closed along the seams.
        let mut top_chords: HashMap<(usize, usize), u32> = HashMap::new();
        let mut caps_of: Vec<Vec<usize>> = Vec::new();
        for (ri, loops) in cuts.iter().enumerate() {
            let mut caps = Vec::new();
            for (z, zone) in self.zones.iter().enumerate() {
                let (runs, closed) = self.zone_runs(loops, built.get(ri).ok_or_else(|| fail("pieces"))?, z, false, |b| (b.top.clone(), b.seg));
                let traced = self.close(asm, runs, closed, &top_v, &mut top_chords, None)?;
                for (outer, holes) in group(traced)? {
                    let fitted = outer.iter().chain(holes.iter().flatten()).any(|(_, _, exact)| !exact);
                    let loops: Vec<Vec<AUse>> =
                        std::iter::once(&outer).chain(&holes).map(|l| l.iter().map(|(u, _, _)| u.clone()).collect()).collect();
                    let face = match zone {
                        Zone::Wall => {
                            let (nurbs, domain) = if fitted {
                                let region = solidcore_record::Region2 {
                                    outer: Loop2 { segs: outer.iter().map(|(_, s, _)| *s).collect() },
                                    holes: holes.iter().map(|h| Loop2 { segs: h.iter().map(|(_, s, _)| *s).collect() }).collect(),
                                };
                                let (n, d) = wall.cap_form(&region, self.rr)?;
                                (Some(n), Some(d))
                            } else {
                                (None, None)
                            };
                            AFace { surface: wall.cylinder(self.rr), nurbs, natural_out: true, loops, shell, domain }
                        }
                        Zone::Flat(k) => {
                            let p = self.plane(*k, self.depth)?;
                            let f = self.flat(*k)?;
                            let natural_out = p.x.cross(p.y).dot(f.normal) > 0.0;
                            AFace {
                                surface: Surface::Plane { origin: arr(p.origin), u_axis: arr(p.x), v_axis: arr(p.y) },
                                nurbs: None,
                                natural_out,
                                loops,
                                shell,
                                domain: None,
                            }
                        }
                    };
                    caps.push(added.len());
                    added.push((face, crate::record::cap(true)));
                }
            }
            caps_of.push(caps);
        }
        // Each profile's solid must not meet the rest of the body.
        for (ri, loops) in cuts.iter().enumerate() {
            let pieces = built.get(ri).ok_or_else(|| fail("pieces"))?;
            let mut a = asm.clone();
            a.faces.fill(None);
            let mut chords = HashMap::new();
            for (z, zone) in self.zones.iter().enumerate() {
                let (runs, closed) = self.zone_runs(loops, pieces, z, false, |b| (b.base.clone(), b.seg));
                let traced = self.close(&mut a, runs, closed, &base_v, &mut chords, None)?;
                for (outer, holes) in group(traced)? {
                    let (surface, nurbs, natural_out) = match zone {
                        Zone::Wall => (selected.surface.clone(), selected.nurbs.clone(), selected.natural_out),
                        Zone::Flat(k) => {
                            let f = flat_faces.get(*k).ok_or_else(|| fail("tangent plane face"))?;
                            (f.surface.clone(), f.nurbs.clone(), f.natural_out)
                        }
                    };
                    // As the face's pieces would take it, facing away from the material it bounds.
                    let loops = std::iter::once(&outer)
                        .chain(&holes)
                        .map(|l| super::reversed(&l.iter().map(|(u, _, _)| u.clone()).collect::<Vec<_>>()))
                        .collect();
                    a.face(AFace { surface, nurbs, natural_out: !natural_out, loops, shell, domain: None });
                }
            }
            let range = walls_of.get(ri).cloned().ok_or_else(|| fail("profile faces"))?;
            for i in range.chain(caps_of.get(ri).into_iter().flatten().copied()) {
                a.face(added.get(i).map(|(f, _)| f.clone()).ok_or_else(|| fail("profile faces"))?);
            }
            if !raised {
                for f in a.faces.iter_mut().flatten() {
                    f.natural_out = !f.natural_out;
                    f.loops = f.loops.iter().map(|l| super::reversed(l)).collect();
                }
            }
            let solid = Body::from_record(a.finish().map_err(|_| refusal(raised))?.record).map_err(|_| refusal(raised))?;
            if !solid.validity().is_empty() || meets(body, &solid, raised)? {
                return Err(refusal(raised));
            }
        }
        // The faces left: each zone's face less the footprints, split at the seams around them.
        let mut base_chords: HashMap<(usize, usize), u32> = HashMap::new();
        let mut outline_ids: HashMap<u32, usize> = HashMap::new();
        let mut kept = Vec::new();
        for (z, zone) in self.zones.iter().enumerate() {
            let (face, record_face) = match zone {
                Zone::Wall => (selected, fi),
                Zone::Flat(k) => (flat_faces.get(*k).ok_or_else(|| fail("tangent plane face"))?, self.flat(*k)?.face),
            };
            let mut runs: Vec<Run<Item>> = Vec::new();
            let mut closed: Vec<Vec<Item>> = Vec::new();
            for (ri, loops) in cuts.iter().enumerate() {
                let pieces = built.get(ri).ok_or_else(|| fail("pieces"))?;
                let (r, c) = self.zone_runs(loops, pieces, z, true, |b| (b.base.clone(), b.seg));
                runs.extend(r);
                closed.extend(c);
            }
            let outline = self.outline(asm, *zone, face, &mut outline_ids, &mut next_id, &mut base_v)?;
            let outline_edges: Vec<u32> =
                outline.iter().flat_map(|r| &r.items).filter_map(|i| if let Item::Use(u, _, _) = i { Some(u.edge) } else { None }).collect();
            runs.extend(outline);
            let traced = self.close(asm, runs, closed, &base_v, &mut base_chords, Some(z))?;
            let pieces = group(traced)?;
            let change = if pieces.len() > 1 { Change::Split } else { Change::Modified };
            for (outer, holes) in pieces {
                let keeps_box = outer.iter().any(|(u, _, _)| outline_edges.contains(&u.edge));
                let loops = std::iter::once(&outer).chain(&holes).map(|l| l.iter().map(|(u, _, _)| u.clone()).collect()).collect();
                let f = AFace {
                    surface: face.surface.clone(),
                    nurbs: face.nurbs.clone(),
                    natural_out: face.natural_out,
                    loops,
                    shell,
                    domain: if keeps_box { face.domain } else { None },
                };
                kept.push((f, crate::record::from_face(0, record_face, change)));
            }
        }
        let mut removed = vec![fi];
        removed.extend(self.flats.iter().map(|f| f.face));
        kept.extend(added);
        Ok(Rebuilt { removed, faces: kept })
    }

    fn zone(&self, z: usize) -> Result<Zone> {
        self.zones.get(z).copied().ok_or_else(|| fail("zone"))
    }

    /// The runs and whole loops of one profile's loops in zone `z`, as items `item` makes of
    /// its pieces (`reverse`: each loop the other way round, as a face's hole).
    fn zone_runs(
        &self,
        loops: &[Vec<Cut>],
        pieces: &[Vec<Built>],
        z: usize,
        reverse: bool,
        item: impl Fn(&Built) -> (AUse, Seg2),
    ) -> (Vec<Run<Item>>, Vec<Vec<Item>>) {
        let mut runs = Vec::new();
        let mut closed = Vec::new();
        for (cut, built) in loops.iter().zip(pieces) {
            let make = |i: usize| -> Option<Item> {
                let b = built.get(i)?;
                let (u, s) = item(b);
                Some(if reverse {
                    Item::Use(AUse { edge: u.edge, forward: !u.forward, given: u.given }, s.reversed(), b.exact)
                } else {
                    Item::Use(u, s, b.exact)
                })
            };
            let (cut, map): (Vec<Cut>, Vec<usize>) = if reverse { reverse_cut(cut) } else { (cut.clone(), (0..cut.len()).collect()) };
            let (r, whole) = runs_in(&cut, z);
            for run in r {
                let items: Vec<Item> = run.items.iter().filter_map(|i| map.get(*i).and_then(|j| make(*j))).collect();
                runs.push(Run { items, from: run.from, to: run.to });
            }
            if let Some(w) = whole {
                closed.push(w.iter().filter_map(|i| map.get(*i).and_then(|j| make(*j))).collect());
            }
        }
        (runs, closed)
    }

    /// The zone face's own outline as runs between the seams it is cut at: its outer loop less
    /// its uses of those seams, each run from one seam's end to another's.
    fn outline(
        &self,
        asm: &Assembly,
        zone: Zone,
        face: &AFace,
        ids: &mut HashMap<u32, usize>,
        next_id: &mut usize,
        base_v: &mut HashMap<Key, u32>,
    ) -> Result<Vec<Run<Item>>> {
        let outer = face.loops.first().ok_or_else(|| fail("face loop"))?;
        let seam_line = |e: u32| self.flats.iter().position(|f| f.seam == e as usize);
        let middle = match zone {
            Zone::Wall => {
                let lo = self.flats.iter().filter(|f| f.side < 0.0).map(|f| f.x).next();
                let hi = self.flats.iter().filter(|f| f.side > 0.0).map(|f| f.x).next();
                match (lo, hi) {
                    (Some(a), Some(b)) => 0.5 * (a + b),
                    (Some(a), None) => a + 1.0,
                    (None, Some(b)) => b - 1.0,
                    (None, None) => 0.0,
                }
            }
            Zone::Flat(k) => self.flat(k)?.x,
        };
        let n = outer.len();
        let Some(first) = (0..n).find(|i| outer.get(*i).and_then(|u| seam_line(u.edge)).is_some()) else {
            return Err(fail("the face's seam"));
        };
        let mut runs = Vec::new();
        let mut crossing = |v: u32, line: usize| -> Result<Crossing> {
            let id = *ids.entry(v).or_insert_with(|| {
                let id = *next_id;
                *next_id += 1;
                id
            });
            base_v.insert(Key::Cross(id), v);
            let p = asm.point(v).ok_or_else(|| fail("vertex"))?;
            Ok(Crossing { line, z: (p - self.wall.o).dot(self.wall.axis), id })
        };
        let start = outer.get(first).ok_or_else(|| fail("face loop"))?;
        let line = seam_line(start.edge).ok_or_else(|| fail("the face's seam"))?;
        let mut current: Option<(Crossing, Vec<Item>)> = Some((crossing(asm.ends(start).ok_or_else(|| fail("face loop"))?.1, line)?, Vec::new()));
        for k in 1..=n {
            let u = outer.get((first + k) % n).ok_or_else(|| fail("face loop"))?;
            let (a, b) = asm.ends(u).ok_or_else(|| fail("face loop"))?;
            if let Some(line) = seam_line(u.edge) {
                if let Some((from, items)) = current.take() {
                    let to = crossing(a, line)?;
                    runs.push(Run { items, from, to });
                }
                current = Some((crossing(b, line)?, Vec::new()));
            } else {
                let (pa, pb) = (asm.point(a).ok_or_else(|| fail("vertex"))?, asm.point(b).ok_or_else(|| fail("vertex"))?);
                let seg = Seg2::Line { a: self.coords(zone, pa, middle)?, b: self.coords(zone, pb, middle)? };
                if let Some((_, items)) = current.as_mut() {
                    items.push(Item::Use(u.clone(), seg, true));
                }
            }
        }
        Ok(runs)
    }

    /// The loops runs close into along the seams, each item an edge use (its segment, and whether
    /// it is exact), the chords between crossings made line edges once (`chords`, by crossing
    /// pair) between the vertices `at` gives them. Below (`zone` given), a chord along the seam
    /// edge's whole length is the seam edge itself, and any other a piece of it.
    fn close(
        &self,
        asm: &mut Assembly,
        runs: Vec<Run<Item>>,
        closed: Vec<Vec<Item>>,
        at: &HashMap<Key, u32>,
        chords: &mut HashMap<(usize, usize), u32>,
        zone: Option<usize>,
    ) -> Result<Vec<Vec<(AUse, Seg2, bool)>>> {
        let mut loops = trace(&runs, Item::Chord)?;
        loops.extend(closed);
        let mut out = Vec::with_capacity(loops.len());
        for lp in loops {
            let mut items = Vec::with_capacity(lp.len());
            for it in lp {
                items.push(match it {
                    Item::Use(u, s, exact) => (u, s, exact),
                    Item::Chord(a, b) => {
                        let (va, vb) = (at.get(&Key::Cross(a.id)).copied(), at.get(&Key::Cross(b.id)).copied());
                        let (Some(va), Some(vb)) = (va, vb) else { return Err(fail("seam crossing vertex")) };
                        let key = (a.id.min(b.id), a.id.max(b.id));
                        let seam = self.flats.get(a.line).map(|f| f.seam as u32).ok_or_else(|| fail("seam"))?;
                        let edge = match chords.get(&key) {
                            Some(e) => *e,
                            None => {
                                let whole = zone.is_some()
                                    && asm.edges.get(seam as usize).is_some_and(|e| {
                                        let [s, t] = e.vertices;
                                        (s == va && t == vb) || (s == vb && t == va)
                                    });
                                let e = if whole {
                                    seam
                                } else {
                                    let e = asm.line(va, vb)?;
                                    if zone.is_some() {
                                        asm.piece(e, seam);
                                    }
                                    e
                                };
                                chords.insert(key, e);
                                e
                            }
                        };
                        let forward = asm.edges.get(edge as usize).is_some_and(|e| e.vertices[0] == va);
                        (AUse { edge, forward, given: None }, super::strip::chord_seg(&self.seams, a, b), true)
                    }
                });
            }
            out.push(items);
        }
        Ok(out)
    }
}

/// Loop `cut` the other way round, and for each of its pieces the piece of `cut` it is.
fn reverse_cut(cut: &[Cut]) -> (Vec<Cut>, Vec<usize>) {
    let n = cut.len();
    let mut out = Vec::with_capacity(n);
    let mut map = Vec::with_capacity(n);
    for j in 0..n {
        let k = n - 1 - j;
        let Some(c) = cut.get(k) else { continue };
        // The crossing it now ends on is the one the piece before it ended on.
        let ends = cut.get((k + n - 1) % n).and_then(|b| b.ends);
        out.push(Cut { seg: c.seg.reversed(), of: c.of, zone: c.zone, ends });
        map.push(k);
    }
    (out, map)
}

type Loop3 = Vec<(AUse, Seg2, bool)>;

/// Loops (wall coordinates: outer loops counter-clockwise, holes clockwise) grouped into faces:
/// each hole with the smallest outer loop round it.
fn group(loops: Vec<Loop3>) -> Result<Vec<(Loop3, Vec<Loop3>)>> {
    let shape = |l: &Loop3| Loop2 { segs: l.iter().map(|(_, s, _)| *s).collect() };
    let (mut outers, mut holes): (Vec<(Loop2, Loop3, Vec<Loop3>)>, Vec<(Loop2, Loop3)>) = (Vec::new(), Vec::new());
    for l in loops {
        let s = shape(&l);
        if s.signed_area() > 0.0 {
            outers.push((s, l, Vec::new()));
        } else {
            holes.push((s, l));
        }
    }
    for (s, l) in holes {
        let probe = s.segs.first().map(|g| g.point_at(0.5)).ok_or_else(|| fail("empty loop"))?;
        let owner = outers
            .iter_mut()
            .filter(|(o, _, _)| o.contains(probe))
            .min_by(|a, b| a.0.signed_area().total_cmp(&b.0.signed_area()))
            .ok_or_else(|| KernelError::Failed("cylinder emboss: a hole outside every face piece".into()))?;
        owner.2.push(l);
    }
    Ok(outers.into_iter().map(|(_, l, h)| (l, h)).collect())
}
