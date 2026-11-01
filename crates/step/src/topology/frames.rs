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
//!
//! A torus face's tube spills the same way along its tube angle, which STEP counts from the outer
//! equator: its tube start moves to where the face's tube begins, or a turn before where it ends
//! (`brep::form::tube_turned`), the same surface. Its circles round the tube are labelled from
//! the tube start (`brep::form::on_tube`), unless one is about another round face's axis (a
//! pipe's cylinder meeting the tube), which labels it on its own grid: the tube start is then
//! the one of the two that puts the tube's grid on that face's.

use std::f64::consts::TAU;

use solidcraft_geom::Vec3;
use solidcraft_geom::brep::{self, Curve as BCurve};

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
    /// each starts on a quarter line; a torus face whose tube would, its tube start too. Returns
    /// how many faces were turned.
    pub(super) fn turn_frames(&mut self) -> R<usize> {
        let about_axis = self.turn_about_axes()?;
        Ok(about_axis + self.turn_tubes())
    }

    /// The torus faces whose forms would run past one turn of the tube start it where their
    /// tube begins, or a turn before where it ends (either fits it in four quarters): the one that
    /// puts its tube circles on the quarter grid of the round faces they are about the axes of
    /// (a pipe's cylinder meeting the tube), so those circles' pcurves are exact on both; where
    /// neither does, where it begins.
    fn turn_tubes(&mut self) -> usize {
        let tol = self.tol;
        let frames: Vec<Option<brep::Frame>> = self.faces.iter().map(|f| brep::form::frame_of(&f.surface)).collect();
        // The round faces each edge bounds.
        let mut bounding: Vec<Vec<usize>> = vec![Vec::new(); self.edges.len()];
        for (fi, f) in self.faces.iter().enumerate() {
            for l in &f.loops {
                if let LoopB::Edges(es) = l {
                    for (e, _) in es {
                        if frames.get(fi).is_some_and(Option::is_some)
                            && let Some(b) = bounding.get_mut(*e as usize)
                            && !b.contains(&fi)
                        {
                            b.push(fi);
                        }
                    }
                }
            }
        }
        let mut turned = 0;
        for fi in 0..self.faces.len() {
            let Some(face) = self.faces.get(fi) else { continue };
            let Some(e) = face.extent else { continue };
            if brep::form::tube_quarters(&face.surface, e, tol).is_none_or(|q| q <= 4) {
                continue;
            }
            // Its tube circles about another round face's axis, with that face's frame.
            let mut joined = Vec::new();
            for l in &face.loops {
                let LoopB::Edges(es) = l else { continue };
                for (ei, _) in es {
                    let Some(edge) = self.edges.get(*ei as usize) else { continue };
                    for other in bounding.get(*ei as usize).into_iter().flatten().filter(|o| **o != fi) {
                        if let Some(Some(fr)) = frames.get(*other) {
                            joined.push((&edge.curve, edge.interval, fr));
                        }
                    }
                }
            }
            let on_grid = |start: f64| {
                brep::form::tube_turned(&face.surface, start)
                    .is_some_and(|torus| joined.iter().all(|(c, iv, fr)| brep::form::tube_on_grid(c, *iv, fr, &torus, tol).is_none_or(|on| on)))
            };
            let (begin, end) = (e[1][0], e[1][1] - TAU);
            let start = if !on_grid(begin) && on_grid(end) { end } else { begin };
            let Some(surface) = brep::form::tube_turned(&face.surface, start) else { continue };
            if let Some(face) = self.faces.get_mut(fi) {
                face.surface = surface;
                if let Some(e) = face.extent.as_mut() {
                    e[1] = [e[1][0] - start, e[1][1] - start];
                }
                turned += 1;
            }
        }
        turned
    }

    /// Measure the round faces' boxes and turn the groups that spill about their axes.
    fn turn_about_axes(&mut self) -> R<usize> {
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
            let turns: Vec<(usize, f64)> = match starts(false).chain(starts(true)).chain(ends).find(|phi| fits(*phi)) {
                Some(phi) => members.iter().map(|(fi, sign, _)| (*fi, sign * phi)).collect(),
                // No one turn fits them all: two whole turns whose seams are not a whole number
                // of quarters apart (a ball's two halves, each with its seam from a different
                // vertex to its pole). Each spilling face is turned on its own to start on a
                // quarter line; the circles between them are then on one face's grid only, and
                // their pcurves on the other are made as any other trim's. Left spilling, the
                // face's form ran past one turn, and a trim across the form's end crossed the
                // face (a torn ball on screen).
                None => members
                    .iter()
                    .filter(|(fi, _, _)| spilling(fi))
                    .filter_map(|(fi, _, e)| {
                        let (f, e) = (self.faces.get(*fi)?, (*e)?);
                        let fits_alone = |angle: &f64| quarters_turned(&f.surface, e, *angle, self.tol).is_some_and(|q| q <= 4);
                        [e[0][0], e[0][1]].into_iter().find(fits_alone).map(|angle| (*fi, angle))
                    })
                    .collect(),
            };
            for (fi, angle) in turns {
                let Some(face) = self.faces.get_mut(fi) else { continue };
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
