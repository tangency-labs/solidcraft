//! Frames turned so that a round face's form covers its face once. A cylinder, cone, sphere or
//! torus face's NURBS form spans the quarters of its frame's quarter grid that its angle interval
//! needs (`brep::form`); a face of less than one turn that does not start on a quarter line and
//! has more than three quarters left spills into a fifth, so its form runs past one turn and
//! covers part of its surface twice, which no kernel takes as a face. A whole turn whose seam is
//! off a quarter line spills the same way: its form is the four quarters from the one it starts
//! in, and the seam's second use lies past their end, off the form. The file's reference
//! direction is the writer's: the face's frame is turned about its own axis (the same surface,
//! its angle parameter measured from further on) so that the face starts on a quarter line, and
//! its form spans the four quarters (or fewer) it covers.
//!
//! A circle edge about the axis of the round faces it bounds is labelled on one of their frames
//! (`brep::form::edge`), so its form and theirs share one quarter grid and its pcurves are
//! straight lines; the faces joined that way (a cylinder and the coaxial fillet beside it) are
//! turned together, by one angle about their common axis, and keep that. They are turned only
//! when one of them spills and one angle puts every one of them within four quarters (a start
//! on a quarter line, or where two of them spill, maybe an end); a face
//! whose frame cannot be turned (a placed revolution, its angle zero at its profile) keeps its
//! group as the file has it.

use solidcraft_geom::Vec3;
use solidcraft_geom::brep::eval::CurveEval;
use solidcraft_geom::brep::{self, Curve as BCurve, Frame, Surface as BSurface};

use super::{Builder, LoopB, R};

/// A circle edge bounds a face about the circle's own axis: the faces it joins share a grid.
fn about_axis(edge: &BCurve, interval: [f64; 2], frame: &brep::Frame, tol: f64) -> bool {
    matches!(edge, BCurve::Circle { .. }) && brep::form::on_frame(edge, interval, frame, tol).is_some()
}

/// The face's quarters about the axis with its frame turned by `angle` (its box shifted to
/// match), or `None` when its form cannot be measured.
fn quarters_turned(surface: &brep::Surface, [u, v]: [[f64; 2]; 2], angle: f64, tol: f64) -> Option<usize> {
    brep::form::turn_quarters(surface, [[u[0] - angle, u[1] - angle], v], tol)
}

/// Union-find root of `i`.
fn root(parent: &mut [usize], mut i: usize) -> usize {
    while let Some(&p) = parent.get(i) {
        if p == i {
            break;
        }
        let up = parent.get(p).copied().unwrap_or(p);
        if let Some(slot) = parent.get_mut(i) {
            *slot = up;
        }
        i = p;
    }
    i
}

impl Builder<'_, '_> {
    /// Measure every round face's box on its label, and turn the frames of the faces whose
    /// forms would run past one turn, with the faces their circle edges join them to, so that
    /// each starts on a quarter line. Returns how many faces were turned.
    pub(super) fn turn_frames(&mut self) -> R<usize> {
        let n = self.faces.len();
        let frames: Vec<Option<brep::Frame>> = self.faces.iter().map(|f| brep::form::frame_of(&f.surface)).collect();
        for fi in 0..n {
            if frames.get(fi).is_some_and(Option::is_some) {
                let extent = self.face_extent(fi)?;
                if let Some(f) = self.faces.get_mut(fi) {
                    f.extent = extent;
                }
            }
        }
        // Each face's quarters about its axis as the file frames it: a spill is five.
        let spills: Vec<bool> =
            self.faces.iter().map(|f| f.extent.and_then(|e| quarters_turned(&f.surface, e, 0.0, self.tol)).is_some_and(|q| q > 4)).collect();
        if !spills.iter().any(|s| *s) {
            return Ok(0);
        }
        // Round faces a circle edge about both their axes joins.
        let mut parent: Vec<usize> = (0..n).collect();
        let mut bounding: Vec<Vec<usize>> = vec![Vec::new(); self.edges.len()];
        for (fi, f) in self.faces.iter().enumerate() {
            if frames.get(fi).is_none_or(Option::is_none) {
                continue;
            }
            for l in &f.loops {
                if let LoopB::Edges(es) = l {
                    for (e, _) in es {
                        if let Some(b) = bounding.get_mut(*e as usize)
                            && !b.contains(&fi)
                        {
                            b.push(fi);
                        }
                    }
                }
            }
        }
        for (edge, faces) in self.edges.iter().zip(&bounding) {
            let joined: Vec<usize> = faces
                .iter()
                .copied()
                .filter(|fi| frames.get(*fi).and_then(Option::as_ref).is_some_and(|fr| about_axis(&edge.curve, edge.interval, fr, self.tol)))
                .collect();
            for w in joined.windows(2) {
                let (a, b) = (root(&mut parent, w[0]), root(&mut parent, w[1]));
                if a != b
                    && let Some(slot) = parent.get_mut(a)
                {
                    *slot = b;
                }
            }
        }
        let mut groups: Vec<Vec<usize>> = Vec::new();
        let mut group_of: Vec<Option<usize>> = vec![None; n];
        for fi in 0..n {
            if frames.get(fi).is_none_or(Option::is_none) {
                continue;
            }
            let r = root(&mut parent, fi);
            let g = match group_of.get(r).copied().flatten() {
                Some(g) => g,
                None => {
                    groups.push(Vec::new());
                    if let Some(slot) = group_of.get_mut(r) {
                        *slot = Some(groups.len() - 1);
                    }
                    groups.len() - 1
                }
            };
            if let Some(members) = groups.get_mut(g) {
                members.push(fi);
            }
        }
        let mut turned = 0;
        for g in &groups {
            if !g.iter().any(|fi| spills.get(*fi).copied().unwrap_or(false)) {
                continue;
            }
            let Some(lead) = g.first().and_then(|fi| frames.get(*fi)).and_then(Option::as_ref) else { continue };
            let axis = Vec3::from(lead.x_axis).cross(Vec3::from(lead.y_axis));
            // Each face's turn about its own axis for a turn `phi` about the lead's: its axis
            // along the lead's or against it.
            let mut members = Vec::with_capacity(g.len());
            for &fi in g {
                let (Some(face), Some(Some(fr))) = (self.faces.get(fi), frames.get(fi)) else { continue };
                let sign = Vec3::from(fr.x_axis).cross(Vec3::from(fr.y_axis)).dot(axis).signum();
                members.push((fi, sign, face.extent));
            }
            // A face that cannot be turned, or measured, keeps its group's frames.
            if members.len() != g.len()
                || members.iter().any(|(fi, _, e)| e.is_none() || self.faces.get(*fi).and_then(|f| brep::form::turned(&f.surface, 0.0)).is_none())
            {
                continue;
            }
            let fits = |phi: f64| {
                members.iter().all(|(fi, sign, e)| match (self.faces.get(*fi), e) {
                    (Some(f), Some(e)) => quarters_turned(&f.surface, *e, sign * phi, self.tol).is_some_and(|q| q <= 4),
                    _ => false,
                })
            };
            // Each face fits for the turns that leave its start no further past a quarter line
            // than the room its sweep leaves in four quarters: an arc of turns from the one that
            // puts its start on a quarter line to the one that puts its end on one. Where the
            // group's arcs meet, the ends of their meeting are among those turns: a spilling
            // face's start on its frame's x axis first, then any face's start, then any's end.
            let spilling = |fi: &usize| spills.get(*fi).copied().unwrap_or(false);
            let starts =
                |all: bool| members.iter().filter(move |(fi, _, _)| all || spilling(fi)).filter_map(|(_, sign, e)| e.map(|e| sign * e[0][0]));
            let ends = members.iter().filter_map(|(_, sign, e)| e.map(|e| sign * e[0][1]));
            let Some(phi) = starts(false).chain(starts(true)).chain(ends).find(|phi| fits(*phi)) else {
                continue;
            };
            for &(fi, sign, _) in &members {
                let Some(face) = self.faces.get_mut(fi) else { continue };
                let angle = sign * phi;
                let Some(surface) = brep::form::turned(&face.surface, angle) else { continue };
                face.surface = surface;
                if let Some(e) = face.extent.as_mut() {
                    e[0] = [e[0][0] - angle, e[0][1] - angle];
                }
                turned += 1;
            }
        }
        Ok(turned)
    }
}

impl Builder<'_, '_> {
    /// Put the frame of each whole sphere bounded by one seam alone (a meridian from pole to
    /// pole, used both ways) on that seam: its axis through the seam's ends, its x axis through
    /// the seam's middle. A sphere's frame is the writer's choice (any right-handed frame about
    /// the centre is the same surface, its natural normal outward); one whose poles are not the
    /// seam's leaves the seam a slit across the form, not the form's own seam, and no kernel
    /// takes that as the whole ball. Returns how many frames were moved.
    pub(super) fn seat_spheres_on_seams(&mut self) -> R<usize> {
        let mut seated = 0;
        for fi in 0..self.faces.len() {
            let Some(face) = self.faces.get(fi) else { continue };
            let BSurface::Sphere { frame, radius } = &face.surface else { continue };
            let (frame, radius) = (frame.clone(), *radius);
            let [LoopB::Edges(uses)] = face.loops.as_slice() else { continue };
            let [(e, a), (f, b)] = uses.as_slice() else { continue };
            if e != f || a == b {
                continue;
            }
            let edge = self.edges.get(*e as usize).ok_or("edge reference")?;
            let point = |v: u32| self.vertices.get(v as usize).map(|x| (Vec3::from(x.point), x.tolerance));
            let (Some((start, t0)), Some((end, t1))) = (point(edge.vertices[0]), point(edge.vertices[1])) else { continue };
            let centre = Vec3::from(frame.origin);
            let reach = self.tol.max(edge.tolerance).max(t0).max(t1);
            // The seam runs between antipodes: its ends on the sphere, opposite each other.
            if (start.dist(centre) - radius.abs()).abs() > reach || (start - centre).dist(centre - end) > 2.0 * reach {
                continue;
            }
            let Some(z) = (start - end).normalized() else { continue };
            let axis = Vec3::from(frame.x_axis).cross(Vec3::from(frame.y_axis));
            // Already the frame's poles.
            if axis.cross(z).len() * radius.abs() <= reach {
                continue;
            }
            let c = CurveEval::new(&edge.curve).map_err(|m| m.to_string())?;
            let middle = c.point(0.5 * (edge.interval[0] + edge.interval[1])).ok_or("edge point")?;
            let off = middle - centre;
            let Some(x) = (off - z * off.dot(z)).normalized() else { continue };
            let y = z.cross(x);
            let Some(face) = self.faces.get_mut(fi) else { continue };
            face.surface = BSurface::Sphere { frame: Frame { origin: frame.origin, x_axis: [x.x, x.y, x.z], y_axis: [y.x, y.y, y.z] }, radius };
            face.extent = None;
            seated += 1;
        }
        Ok(seated)
    }
}
