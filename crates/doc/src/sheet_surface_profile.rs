//! An edge flange shaped by a profile sketch (#3516). The sketch is drawn on the flange's own
//! plane with its bottom line on the edge: on the bend's outer virtual sharp, from which a
//! flange's height is measured. Read into the chart of the flange's panel (projected along the
//! panel's normal, so either of its faces will do), the profile is the panel's region and its
//! holes are the region's holes. Its bottom line is the stretch of the edge the bend runs along:
//! one run of [`SurfaceSheet::flanges_on`], whose free ends get the rule's bend relief where the
//! panel it stands on runs on past them, when the sheet is settled. Nothing else is made for it.
//!
//! The chart's anchor marks the virtual sharp of the sheet's surface: the outer sharp where the
//! bend turns toward the material, the inner one where it turns away, which lies `T·tan(θ/2)`
//! out along the flange from the outer one. There the profile is moved out by that much.
//!
//! Across the bend, out to its tangent line, the profile's sides run square to the edge. A side
//! that leans there is refused, naming it: when the sheet is settled, a bend's free end is read
//! from the panels' sides only as a blank's side laid on in the flat, its line running on through
//! the bend to the bend's end on the anchor (see `outline_ends` in `sheet_surface_convert`). A
//! profile's side is drawn on the folded flange instead, through the bend's end on the outer
//! sharp, so in the flat it runs on to the anchor somewhere else; read from the line it has on
//! the panel past the tangent line, its end is a free end that runs on, with the rule's bend
//! relief, and the body does not build (`sew: ambiguous edge contact`, for a flange flared past
//! a plain plate's corners). Building one would take a bend end read from a side that is not the
//! blank's, which is how every bend's end is settled, not a part of this feature. Past the
//! tangent line the profile may taper, step and hold holes as it likes.

use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_develop::prelude::*;
use solidcraft_develop::{Joint, JointKind, Rigid3, anchor_past_outer_sharp, edge_fractions, profile_frame, profile_tilt, shift_along};
use solidcraft_geom::prelude::*;

use crate::sheet::{HeightFrom, WHOLE_EDGE};
use crate::sheet_surface_convert::{fold_onto, rigid_region};
use crate::sheet_surface_cuts::seg_box;
use crate::sheet_surface_features::FreeEdge;
use crate::{DocError, Result, SurfaceSheet};

/// Refused: a profile sketch that does not lie on the flange (#3516).
pub const PROFILE_OFF_PLANE: &str = "the flange's profile sketch must lie on the flange: on the plane of one of its faces";

fn err<T>(msg: impl Into<String>) -> Result<T> {
    Err(DocError::Invalid(msg.into()))
}

/// A flange panel's place in space where the sheet's joints fold it: its chart's frame.
fn panel_place(sk: &SurfaceSheet, panel: usize) -> Result<Rigid3> {
    let dev = sk.develop().map_err(|e| DocError::Invalid(e.to_string()))?;
    let places = fold_onto(&sk.sheet, &dev, &sk.places)?.places;
    places.get(panel).copied().flatten().ok_or_else(|| DocError::Invalid("the flange has no place".into()))
}

/// The bend joint that panel `panel` stands on.
fn bend_onto(sk: &SurfaceSheet, panel: usize) -> Option<&Joint> {
    sk.sheet.joints.iter().find(|j| j.b.panel == panel && j.a.panel != panel && matches!(j.kind, JointKind::Bend(_)))
}

/// One of a profile's two sides, in the profile's frame: the far end of its first line (it stands
/// on an end of the bottom line), and whether it is the side at the bend's start (`x` least).
#[derive(Clone, Copy, Debug)]
struct Side {
    top: Vec2,
    at_start: bool,
}

/// Where a profile flange's profile sketch lies in the chart of the panel it made: the map that
/// put its holes there, by which their walls are named after the sketch's curves, as a cut's are
/// (see [`SurfaceSheet::name_hole`]).
#[derive(Clone, Copy)]
pub struct ProfileChart {
    plane: Plane,
    back: Rigid3,
    shift: f64,
}

impl ProfileChart {
    /// The point `p` of the sketch (in its plane's coordinates) in the panel's chart.
    pub fn point(&self, p: Vec2) -> Vec2 {
        let q = self.back.point(self.plane.to_world(p));
        Vec2::new(q.x - self.shift, q.y)
    }
}

impl SurfaceSheet {
    /// Where a profile sketch for the flange panel `panel` is drawn (#3516), in the body's
    /// coordinates: on the flange's outer face, its origin where the bend starts on its outer
    /// virtual sharp (on the edge, for a flange in the default position), its x along the bend
    /// and its y out along the flange. With the bend's length and how far the flange reaches out
    /// from that sharp, its height.
    pub fn profile_frame(&self, panel: usize) -> Option<(Plane, f64, f64)> {
        let joint = bend_onto(self, panel)?;
        let JointKind::Bend(bend) = &joint.kind else { return None };
        let place = panel_place(self, panel).ok()?;
        let region = &self.sheet.panels.get(panel)?.region;
        // The frame in the panel's chart, on its outer face, is solidcore's: this carries it into
        // the body's coordinates where the sheet's joints fold the panel.
        let f = profile_frame(&joint.b, bend.toward_material, bend.angle, self.sheet.thickness, &region.outer);
        let plane = Plane::new(place.point(f.origin.extend(f.depth)), place.vector(f.x.extend(0.0)), place.vector(f.y.extend(0.0)))?;
        Some((plane, bend.length, f.height))
    }

    /// An edge flange on the free edge `edge` whose outline is the closed `profile` drawn on
    /// `plane` (#3516), bent `angle` at inside radius `radius`, placed as `position` says and
    /// to the side `flip` gives, as [`SurfaceSheet::flanges_on`] makes one. The profile lies on
    /// the plane of a face of the flange a plain flange on the whole edge would make, its bottom
    /// line on the bend's outer virtual sharp. The bend runs along the edge as far as that line
    /// does, the flange is as high as the profile, and the profile is the flange's panel, its
    /// holes the panel's holes. Nothing of it but its bottom line and its two sides, straight
    /// lines running square to the edge from the bottom line's ends out to the bend's tangent
    /// line or past it, may lie in the bend (see the module's note on leaning sides). The new
    /// bend's joint, its corners not closed yet (see [`SurfaceSheet::close_flange_corners`]), and
    /// where the profile lies in the new panel's chart.
    pub fn profile_flange_on(
        &mut self,
        edge: FreeEdge,
        plane: &Plane,
        profile: &Region2,
        angle: f64,
        radius: f64,
        position: &str,
        flip: bool,
    ) -> Result<(usize, ProfileChart)> {
        let t = self.sheet.thickness;
        let from = HeightFrom::Outer;
        // How far the bend reaches out along the flange from its outer sharp: its tangent line.
        let band = from.before_leg(angle, radius, t);
        // The plain flange on the whole edge, in a copy of the sheet: the profile is read in its
        // panel's chart, which neither the flange's height nor its stretch of the edge moves
        // (but along the edge).
        let mut probe = self.clone();
        let (made, _) = probe.flanges_on(edge, &[(WHOLE_EDGE.0, WHOLE_EDGE.1, band + 1.0, angle, flip)], radius, position, from)?;
        let Some(Joint { a: pa, b: pb, kind: JointKind::Bend(pbend) }) = made.first().and_then(|&j| probe.sheet.joints.get(j)).cloned() else {
            return err("the flange has no bend");
        };
        if pb.origin.len() > 1e-12 || (pb.dir - Vec2::new(1.0, 0.0)).len() > 1e-12 {
            return err("not supported yet: a flange whose anchor is not its chart's x axis");
        }
        let place = panel_place(&probe, pb.panel)?;
        let back = place.inverse();
        if profile_tilt(plane.normal(), place.z) > 1e-9 {
            return err(PROFILE_OFF_PLANE);
        }
        // How deep in the panel's material the sketch's plane lies: on its face on the sheet's
        // surface (0) or on its other face (the thickness). A plane anywhere else, parallel to
        // them, would build the same flange from a sketch that is not on it.
        let z = back.point(plane.origin).z;
        let near_plane = 1e-6 * (1.0 + plane.origin.len());
        if z.abs() > near_plane && (z - t).abs() > near_plane {
            return err(PROFILE_OFF_PLANE);
        }
        // The profile's frame: the chart's, its x along the bend from the anchor's start and its
        // y = 0 on the outer sharp, the flange out toward -y.
        let past = anchor_past_outer_sharp(pbend.toward_material, angle, t);
        let to_frame = |p: Vec2| {
            let q = back.point(plane.to_world(p));
            Vec2::new(q.x, q.y - past)
        };
        // A point of the frame in the sketch, for what is said about it.
        let in_sketch = |c: Vec2| plane.to_local(place.point(Vec3::new(c.x, c.y + past, z)));
        let shown = |c: Vec2| {
            let p = in_sketch(c);
            format!("({:.3}, {:.3})", p.x, p.y)
        };
        // The profile's sides as the sketch shows them: of the two, the one with the smaller
        // sketch x is its left side.
        let start_left = in_sketch(Vec2::ZERO).x <= in_sketch(Vec2::new(1.0, 0.0)).x;
        let side_name = |at_start: bool, foot: Vec2| {
            let which = if at_start == start_left { "left" } else { "right" };
            format!("its {which} side (standing on the edge at {} in the sketch)", shown(foot))
        };
        let read = rigid_region(profile, to_frame);
        let all = || std::iter::once(&read.outer).chain(&read.holes).flat_map(|l| &l.segs);
        let (mut lo, mut hi) = (Vec2::new(f64::INFINITY, f64::INFINITY), Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY));
        for s in all() {
            let (a, b) = seg_box(s);
            lo = Vec2::new(lo.x.min(a.x), lo.y.min(a.y));
            hi = Vec2::new(hi.x.max(b.x), hi.y.max(b.y));
        }
        if !(lo.x.is_finite() && lo.y.is_finite() && hi.x.is_finite() && hi.y.is_finite()) {
            return err("the flange's profile sketch has no closed profile");
        }
        let tol = 1e-7 * (hi - lo).len().max(1.0);
        if hi.y > tol {
            return err(format!("the flange's profile runs {:.3} mm below the edge, into the base: its bottom line belongs on the edge", hi.y));
        }
        if hi.y < -tol {
            return err(format!("the flange's profile must start on the edge: its bottom line lies {:.3} mm above it", -hi.y));
        }
        let height = -lo.y;
        if !(height > band + tol) {
            return err(format!("the flange's profile must reach past the bend: more than {band:.3} mm out from the edge"));
        }
        // Its bottom line: the outline's lines on the outer sharp, one after another.
        let segs = &read.outer.segs;
        let n = segs.len();
        let flat = |s: &Seg2| matches!(*s, Seg2::Line { a, b } if a.y.abs() <= tol && b.y.abs() <= tol);
        let on: Vec<usize> = (0..n).filter(|&i| segs.get(i).is_some_and(flat)).collect();
        // The first of them, after one that is not on the line.
        let Some(&first) = on.iter().find(|&&i| !on.contains(&((i + n - 1) % n))) else {
            return err("the flange's profile must start on the edge: its bottom line runs along the bend");
        };
        let run = (0..n).map(|k| (first + k) % n).take_while(|i| on.contains(i)).count();
        if run != on.len() {
            return err(
                "the flange's profile must run straight across the bend: its bottom line on the edge is one line, with nothing cut out of it",
            );
        }
        let bottom: Vec<usize> = (0..run).map(|k| (first + k) % n).collect();
        let (u0, u1) = bottom
            .iter()
            .filter_map(|&i| segs.get(i))
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), s| (a.min(s.start().x).min(s.end().x), b.max(s.start().x).max(s.end().x)));
        let len = pbend.length;
        if u0 < -tol || u1 > len + tol {
            let beyond = (-u0).max(u1 - len);
            return err(format!(
                "the flange's profile runs {beyond:.3} mm past the end of its edge: its bottom line lies along the edge, {len:.3} mm long"
            ));
        }
        // Its two sides, before and after the bottom line in the loop, each from one of the bottom
        // line's ends out across the bend's tangent line.
        let before = (first + n - 1) % n;
        let after = (first + run) % n;
        let side_at = |i: usize| -> Result<Side> {
            let s = segs.get(i).ok_or_else(|| DocError::Invalid("the flange's profile has no sides".into()))?;
            let (p, q) = (s.start(), s.end());
            let (foot, top) = if p.y.abs() <= tol { (p, q) } else { (q, p) };
            let at_start = (foot.x - u0).abs() <= (foot.x - u1).abs();
            let name = side_name(at_start, foot);
            if !matches!(s, Seg2::Line { .. }) {
                return err(format!("the flange's profile must run straight across the bend: {name} is no straight line there"));
            }
            if top.y > -band + tol {
                return err(format!(
                    "the flange's profile must run straight across the bend: {name} must run on as one straight line until it is \
                     {band:.3} mm out from the edge (the bend's tangent line), not stop {:.3} mm out",
                    -top.y
                ));
            }
            // How far the side has moved along the edge by the time it is out of the bend: in
            // toward the profile's middle, or out past the bend's end.
            let shift = (top.x - foot.x) * (band / -top.y);
            let inward = if at_start { shift } else { -shift };
            if inward.abs() > tol {
                let way = if inward > 0.0 { "in toward the middle" } else { "out past the end of the bottom line" };
                return err(format!(
                    "not supported yet: the flange's profile leans across the bend at {name}, {:.3} mm {way} by the bend's \
                     tangent line, {band:.3} mm out from the edge. Draw that side square to the edge out to there; past it the \
                     profile may taper",
                    inward.abs()
                ));
            }
            Ok(Side { top, at_start })
        };
        let sides = [side_at(before)?, side_at(after)?];
        if sides.iter().all(|s| s.at_start) || sides.iter().all(|s| !s.at_start) {
            return err("the flange's profile must run straight across the bend: one side from each end of its bottom line");
        }
        // Nothing else of the outline, and none of its holes, reaches into the bend.
        let rest = segs.iter().enumerate().filter(|(i, _)| !bottom.contains(i) && *i != before && *i != after).map(|(_, s)| s);
        for s in rest.chain(read.holes.iter().flat_map(|h| &h.segs)) {
            let (a, b) = seg_box(s);
            if b.y > -band + tol {
                // The point of it nearest the edge.
                let at = [s.start(), s.end()].into_iter().find(|p| p.y >= b.y - tol).unwrap_or(Vec2::new((a.x + b.x) / 2.0, b.y));
                return err(format!(
                    "the flange's profile cuts into its bend at {} in the sketch: only its bottom line and its two sides may lie within \
                     {band:.3} mm of the edge (the bend's tangent line); move the rest out past that",
                    shown(at)
                ));
            }
        }
        // The stretch of the edge the bottom line runs along, as fractions of the edge from its
        // end `b` (see `flanges_on`): where the line's ends along the probe's anchor land on it.
        let Some([at_u1, at_u0]) = edge_fractions(edge.a, edge.b, &pa, [u1, u0]) else { return err("the edge has no length") };
        let stretch = (if u1 >= len - tol { WHOLE_EDGE.0 } else { at_u1 }, if u0 <= tol { WHOLE_EDGE.1 } else { at_u0 });
        // The flange is as high as the profile from the outer sharp, as a plain flange's height
        // is measured; its outline is the profile's.
        let (made, _) = self.flanges_on(edge, &[(stretch.0, stretch.1, height, angle, flip)], radius, position, from)?;
        let Some((j, Some(Joint { a: ra, b: rb, kind: JointKind::Bend(rbend) }))) = made.first().map(|&j| (j, self.sheet.joints.get(j).cloned()))
        else {
            return err("the flange has no bend");
        };
        // The flange made is the one read, its chart moved along the edge to its stretch.
        let shift = shift_along(&pa, &ra);
        if (shift - u0).abs() > tol || (rbend.length - (u1 - u0)).abs() > tol {
            return err(format!(
                "not supported yet: this edge's flange from a profile (its bend is {:.3} mm long from {shift:.3} mm, the profile's {:.3} mm from {u0:.3} mm)",
                rbend.length,
                u1 - u0
            ));
        }
        let bend_len = rbend.length;
        // The profile in the new panel's chart: along the edge to the stretch's start, and out by
        // as far as the anchor lies past the outer sharp.
        // With `to_frame`, the sketch's point `p` lies at `back(p) - (shift, 0)`: `ProfileChart`.
        let to_chart = |p: Vec2| Vec2::new(p.x - shift, p.y + past);
        let moved = |s: &Seg2| match *s {
            Seg2::Arc { center, radius, start, sweep } => Seg2::Arc { center: to_chart(center), radius, start, sweep },
            other => other.map_controls(to_chart),
        };
        // The outline from the bottom line on, in the loop's order: the bottom line runs from the
        // side `before` it to the side `after` it, each side square to the edge from the bend's
        // end on the anchor's line (exactly: the zone's ends are read from them) to its top.
        let [side_in, side_out] = sides;
        let corner = |side: &Side| if side.at_start { Vec2::ZERO } else { Vec2::new(bend_len, 0.0) };
        let mut outer: Vec<Seg2> = vec![Seg2::Line { a: corner(&side_in), b: corner(&side_out) }];
        outer.push(Seg2::Line { a: corner(&side_out), b: to_chart(side_out.top) });
        outer.extend((1..n - run - 1).map(|k| (after + k) % n).filter_map(|i| segs.get(i)).map(moved));
        outer.push(Seg2::Line { a: to_chart(side_in.top), b: corner(&side_in) });
        let region = Region2 {
            outer: Loop2 { segs: outer }.ccw(),
            holes: read.holes.iter().map(|h| Loop2 { segs: h.segs.iter().map(moved).collect() }).collect(),
        };
        match self.sheet.panels.get_mut(rb.panel) {
            Some(panel) => panel.region = region,
            None => return err("the flange has no panel"),
        }
        Ok((j, ProfileChart { plane: *plane, back, shift }))
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::PI;

    use solidcore_record::{Loop2, Plane, Region2, Vec2, Vec3};
    use solidcraft_geom::prelude::*;

    /// The flat's outline, its corners only (a corner between two lines that run on in one line
    /// is none), in the base sketch's frame: the flat lies in the base panel's chart, whose y runs
    /// the other way (see `SurfaceSheet::base`).
    fn corners(sk: &crate::SurfaceSheet) -> Vec<Vec2> {
        let Some(r) = sk.regions.first() else { return Vec::new() };
        let pts: Vec<Vec2> = r.outer.segs.iter().map(|s| s.start()).collect();
        let n = pts.len();
        (0..n)
            .filter(|&i| {
                let (p, v, q) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
                (v - p).cross(q - v).abs() > 1e-9
            })
            .map(|i| Vec2::new(pts[i].x, -pts[i].y))
            .collect()
    }

    #[test]
    fn a_profile_flange_short_of_its_edge_gets_the_bend_relief_where_the_base_runs_on() {
        // A 41.5 x 34.5 plate (rule: R 1, T 2, K 0.5, bend relief 2 wide and 1 deep) and a 90°
        // flange 53 high from a profile on part of its y = 0 edge. Under the flange the base ends
        // at the bend's tangent line, (R + T)·tan 45° = 3 in (y = 3); the zone is
        // (π/2)·(R + K·T) = π wide and the flange 53 − 3 = 50 past its own tangent line. Where the
        // base runs on past an end of the flange a slot 2 wide is cut beside it, from 1 inside the
        // tangent line (y = 4) out through the edge; an end at the plate's corner gets none. A
        // split edge's segments never get one at their junctions (they butt), so the profile
        // flange, though made as one run of the edge, must.
        let w = 41.5;
        let rule = crate::ConvertRule {
            thickness: 2.0,
            k_factor: 0.5,
            bend_radius: 1.0,
            gap: 0.1,
            corner_relief: crate::sheet::ReliefShape::Notch,
            corner_relief_size: 0.0,
            relief_width: 2.0,
            relief_depth: 1.0,
        };
        let plane = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Z };
        let top = -(50.0 + PI - 3.0);
        for (x0, x1) in [(0.0, 35.0), (5.0, 35.0)] {
            let what = format!("profile x {x0}…{x1}");
            let pts = [Vec2::new(0.0, 0.0), Vec2::new(w, 0.0), Vec2::new(w, 34.5), Vec2::new(0.0, 34.5)];
            let plate = Region2 { outer: Loop2::polygon(&pts).ccw(), holes: Vec::new() };
            let (mut s, _) = crate::SurfaceSheet::base("Plate", "R", &rule, Plane::XY, plate, &[], 1).unwrap();
            let edge = s.free_edge_at(Vec3::new(20.0, 0.0, 2.0)).unwrap().unwrap();
            let pts = [Vec2::new(x0, 0.0), Vec2::new(x1, 0.0), Vec2::new(x1, 53.0), Vec2::new(x0, 53.0)];
            let profile = Region2 { outer: Loop2::polygon(&pts).ccw(), holes: Vec::new() };
            let (j, _) = s.profile_flange_on(edge, &plane, &profile, PI / 2.0, 1.0, "inside", false).unwrap();
            s.close_flange_corners(&[j]).unwrap();
            s.build().unwrap();
            assert!(s.notes.is_empty(), "{what}: {:?}", s.notes);
            let mut want = vec![
                Vec2::new(x0, top),
                Vec2::new(x1, top),
                Vec2::new(x1, 4.0),
                Vec2::new(x1 + 2.0, 4.0),
                Vec2::new(x1 + 2.0, 0.0),
                Vec2::new(w, 0.0),
                Vec2::new(w, 34.5),
                Vec2::new(0.0, 34.5),
            ];
            if x0 > 0.0 {
                want.extend([Vec2::new(0.0, 0.0), Vec2::new(x0 - 2.0, 0.0), Vec2::new(x0 - 2.0, 4.0), Vec2::new(x0, 4.0)]);
            }
            let got = corners(&s);
            assert_eq!(s.regions.len(), 1, "{what}: {:?}", s.regions);
            assert_eq!(got.len(), want.len(), "{what}: {got:?}");
            for p in &want {
                assert!(got.iter().any(|q| q.dist(*p) < 1e-9), "{what}: no corner at {p:?} in {got:?}");
            }
            // Its area, by hand: the plate, less the 3 under the bend and each relief's 2 x 4,
            // with the zone and the flange.
            let l = x1 - x0;
            let reliefs = if x0 > 0.0 { 2.0 } else { 1.0 };
            let area = w * 34.5 - 3.0 * l - 8.0 * reliefs + (PI + 50.0) * l;
            let got_area: f64 =
                s.regions.iter().map(|r| r.outer.signed_area().abs() - r.holes.iter().map(|h| h.signed_area().abs()).sum::<f64>()).sum();
            // The flat is laid out by turning each panel's chart into place, and joined: its corners
            // agree with these to 1e-9 above, so its area to within that times its perimeter (about
            // 300), 3e-7.
            assert!((got_area - area).abs() < 3e-7, "{what}: {got_area} vs {area}");
        }
    }
}
