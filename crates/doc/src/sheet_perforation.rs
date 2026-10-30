//! The holes of a perforation field (#732): their shape (round, hexagonal or slotted), and the
//! room they leave between them and to the sheet's edges.

use std::f64::consts::{FRAC_PI_2, PI};

use super::*;

/// The shape of a perforation field's holes. They lie along the grid's rows (the flat's X).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PerforationShape {
    /// Round, `diameter` across.
    #[default]
    Round,
    /// A regular hexagon `diameter` across its flats, with a corner up: on a staggered grid each
    /// flat faces a neighbour, a honeycomb with bars of even width.
    Hex,
    /// A slot `diameter` wide and `length` long overall, with round ends.
    Slot { length: f64 },
}

impl PerforationShape {
    pub fn is_round(&self) -> bool {
        *self == PerforationShape::Round
    }
}

/// The distance between segments `ab` and `cd` (0 where they cross).
fn segment_distance(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> f64 {
    let side = |p: Vec2, q: Vec2, r: Vec2| (q - p).cross(r - p);
    let (d1, d2, d3, d4) = (side(c, d, a), side(c, d, b), side(a, b, c), side(a, b, d));
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0)) {
        return 0.0;
    }
    let to = |p: Vec2, s: Vec2, e: Vec2| {
        let v = e - s;
        let len2 = v.dot(v);
        let t = if len2 > 0.0 { ((p - s).dot(v) / len2).clamp(0.0, 1.0) } else { 0.0 };
        p.dist(s + v * t)
    };
    to(a, c, d).min(to(b, c, d)).min(to(c, a, b)).min(to(d, a, b))
}

impl SheetPerforation {
    /// The distance between rows: `row_pitch`, else the pitch's 60° rows (staggered) or the pitch (square).
    pub fn row_spacing(&self) -> f64 {
        self.row_pitch.unwrap_or(if self.square { self.pitch } else { self.pitch * 3f64.sqrt() / 2.0 })
    }

    /// How far a hole reaches from its centre.
    pub(super) fn reach(&self) -> f64 {
        match self.shape {
            PerforationShape::Round => self.diameter / 2.0,
            PerforationShape::Hex => self.diameter / 3f64.sqrt(),
            PerforationShape::Slot { length } => length / 2.0,
        }
    }

    /// Half a hole's width (along the rows) and height.
    pub(super) fn half_extent(&self) -> Vec2 {
        let r = self.diameter / 2.0;
        match self.shape {
            PerforationShape::Round => Vec2::new(r, r),
            PerforationShape::Hex => Vec2::new(r, self.diameter / 3f64.sqrt()),
            PerforationShape::Slot { length } => Vec2::new(length / 2.0, r),
        }
    }

    /// One hole's area.
    pub fn hole_area(&self) -> f64 {
        let d = self.diameter;
        match self.shape {
            PerforationShape::Round => PI * d * d / 4.0,
            PerforationShape::Hex => 3f64.sqrt() / 2.0 * d * d,
            PerforationShape::Slot { length } => d * (length - d) + PI * d * d / 4.0,
        }
    }

    /// One hole's perimeter: its wall's length round the hole.
    pub fn hole_perimeter(&self) -> f64 {
        let d = self.diameter;
        match self.shape {
            PerforationShape::Round => PI * d,
            PerforationShape::Hex => 2.0 * 3f64.sqrt() * d,
            PerforationShape::Slot { length } => 2.0 * (length - d) + PI * d,
        }
    }

    /// The grid's open share: one hole's area over the area each hole has (pitch × row spacing),
    /// the figure perforated sheet is sold by.
    pub fn nominal_open_share(&self) -> f64 {
        self.hole_area() / (self.pitch * self.row_spacing())
    }

    /// The corners of a hexagonal hole round `c`, counter-clockwise from the one at 30°.
    fn hex_corners(&self, c: Vec2) -> [Vec2; 6] {
        let r = self.diameter / 3f64.sqrt();
        std::array::from_fn(|k| c + Vec2::from_angle(PI / 6.0 + k as f64 * PI / 3.0) * r)
    }

    /// The straight part of a slot round `c`: its round ends' centres.
    fn slot_core(&self, c: Vec2, length: f64) -> (Vec2, Vec2) {
        let h = (length - self.diameter) / 2.0;
        (c - Vec2::new(h, 0.0), c + Vec2::new(h, 0.0))
    }

    /// The hole round `c`, counter-clockwise. A round hole is two half circles, as a cut-out's.
    pub fn hole_loop(&self, c: Vec2) -> Loop2 {
        let r = self.diameter / 2.0;
        match self.shape {
            PerforationShape::Round => Loop2 {
                segs: vec![Seg2::Arc { center: c, radius: r, start: 0.0, sweep: PI }, Seg2::Arc { center: c, radius: r, start: PI, sweep: PI }],
            },
            PerforationShape::Hex => Loop2::polygon(&self.hex_corners(c)),
            PerforationShape::Slot { length } => {
                let (a, b) = self.slot_core(c, length);
                Loop2 {
                    segs: vec![
                        Seg2::Line { a: a - Vec2::new(0.0, r), b: b - Vec2::new(0.0, r) },
                        Seg2::Arc { center: b, radius: r, start: -FRAC_PI_2, sweep: PI },
                        Seg2::Line { a: b + Vec2::new(0.0, r), b: a + Vec2::new(0.0, r) },
                        Seg2::Arc { center: a, radius: r, start: FRAC_PI_2, sweep: PI },
                    ],
                }
            }
        }
    }

    /// How far the hole round `c` stays clear of segment `ab` (0 or less where it reaches it).
    pub(super) fn clearance(&self, c: Vec2, a: Vec2, b: Vec2) -> f64 {
        let r = self.diameter / 2.0;
        match self.shape {
            PerforationShape::Round => {
                let (d, l) = (b - a, (b - a).dot(b - a));
                let t = if l > 0.0 { ((c - a).dot(d) / l).clamp(0.0, 1.0) } else { 0.0 };
                c.dist(a + d * t) - r
            }
            PerforationShape::Slot { length } => {
                let (p, q) = self.slot_core(c, length);
                segment_distance(p, q, a, b) - r
            }
            PerforationShape::Hex => {
                let k = self.hex_corners(c);
                let inside = |p: Vec2| (0..6).all(|i| (k[(i + 1) % 6] - k[i]).cross(p - k[i]) >= 0.0);
                if inside(a) || inside(b) {
                    return 0.0;
                }
                (0..6).map(|i| segment_distance(k[i], k[(i + 1) % 6], a, b)).fold(f64::MAX, f64::min)
            }
        }
    }

    /// The bar between a hole and the hole `o` away from it (0 or less where they meet).
    fn gap_to(&self, o: Vec2) -> f64 {
        let c = Vec2::default();
        match self.shape {
            PerforationShape::Round => o.len() - self.diameter,
            PerforationShape::Slot { length } => {
                let (p, q) = self.slot_core(c, length);
                segment_distance(p, q, p + o, q + o) - self.diameter
            }
            PerforationShape::Hex => {
                let k = self.hex_corners(c);
                (0..6).map(|i| self.clearance(o, k[i], k[(i + 1) % 6])).fold(f64::MAX, f64::min)
            }
        }
    }

    /// Whether the holes leave a bar between neighbours, on their row and on the rows beside.
    pub fn check_spacing(&self) -> Result<()> {
        let (p, dy) = (self.pitch, self.row_spacing());
        let shift = if self.square { 0.0 } else { p / 2.0 };
        let mut near = vec![Vec2::new(p, 0.0)];
        for k in -3..=3 {
            near.push(Vec2::new(shift + k as f64 * p, dy));
            near.push(Vec2::new(k as f64 * p, 2.0 * dy));
        }
        let bar = near.iter().map(|o| self.gap_to(*o)).fold(f64::MAX, f64::min);
        if !(bar > LINEAR_TOL) {
            return Err(DocError::Invalid(format!(
                "perforation holes meet their neighbours: pitch {p} and row spacing {dy} leave no bar between them (smallest {bar:.4})"
            )));
        }
        Ok(())
    }

    /// The area the field covers: inside its outline where it has one, on the sheet's material.
    pub(super) fn field_area(&self, regions: &[Region2], work: &mut FlatWork) -> Result<f64> {
        let flat: f64 = regions.iter().map(Region2::area).sum();
        match &self.outline {
            None => Ok(flat),
            Some(o) => Ok(material_boolean(regions, o, kernel::BoolOp::Intersect, work)?.iter().map(Region2::area).sum()),
        }
    }
}

impl SheetBody {
    /// A perforation field's figures: its holes, their open area, the area of the field (the
    /// outline on the sheet's material, else the whole flat) and the open share of it.
    pub fn perforation_stats(&self, p: &SheetPerforation) -> Result<PerforationStats> {
        let holes = self.perforation_holes(p)?.len();
        let open = holes as f64 * p.hole_area();
        let regions = self.flat_regions()?;
        let field = p.field_area(&regions, &mut FlatWork::new())?;
        Ok(PerforationStats { holes, open_area: open, field_area: field, open_share: if field > 0.0 { open / field } else { 0.0 } })
    }
}

/// A perforation field's figures (see [`SheetBody::perforation_stats`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PerforationStats {
    pub holes: usize,
    /// The holes' area (mm²).
    pub open_area: f64,
    /// The field's area (mm²).
    pub field_area: f64,
    /// The open area over the field's.
    pub open_share: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(t: f64) -> RuleValues {
        RuleValues {
            thickness: t,
            k_factor: 0.44,
            bend_radius: t,
            relief_width: t,
            relief_depth: t / 2.0,
            corner_relief: 10.0,
            corner_relief_shape: ReliefShape::Notch,
            hem_gap: 0.01,
            gap: t,
            angle_display: AngleDisplay::Bend,
        }
    }

    /// A flat plate `w` × `h`, `t` thick, on XY from the origin.
    fn plate(w: f64, h: f64, t: f64) -> SheetBody {
        let r = rect(Vec2::new(0.0, 0.0), Vec2::new(w, 0.0), Vec2::new(w, h), Vec2::new(0.0, h));
        base_flange("Body1", "Steel (mm)", &rule(t), Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Y }, r)
    }

    fn field(diameter: f64, pitch: f64, margin: f64, shape: PerforationShape) -> SheetPerforation {
        SheetPerforation { feature: 1, diameter, pitch, margin, square: false, from_right: false, outline: None, shape, row_pitch: None }
    }

    #[test]
    fn each_hole_shape_has_the_area_and_perimeter_of_its_loop() {
        for shape in [PerforationShape::Round, PerforationShape::Hex, PerforationShape::Slot { length: 7.0 }] {
            let p = field(3.0, 8.0, 1.0, shape);
            let l = p.hole_loop(Vec2::new(4.0, -2.0));
            assert!(l.is_ccw() && l.is_closed(1e-12), "{shape:?}");
            assert!((l.signed_area() - p.hole_area()).abs() < 1e-9, "{shape:?}: {} vs {}", l.signed_area(), p.hole_area());
            let perimeter: f64 = l.segs.iter().map(Seg2::length).sum();
            assert!((perimeter - p.hole_perimeter()).abs() < 1e-9, "{shape:?}: {perimeter} vs {}", p.hole_perimeter());
            let reach = l.polyline(1e-6).iter().map(|q| q.dist(Vec2::new(4.0, -2.0))).fold(0.0, f64::max);
            assert!((reach - p.reach()).abs() < 1e-6, "{shape:?}: reach {reach}");
        }
        // Ø5 on a 6 mm staggered pitch: the 63 % perforated sheet is sold as.
        assert!((field(5.0, 6.0, 1.0, PerforationShape::Round).nominal_open_share() - 0.6298).abs() < 1e-4);
    }

    /// Every hole's outline stays the margin clear of the edges, and the first row and column lie on it.
    #[test]
    fn holes_keep_the_margin_by_their_shape() {
        let s = plate(50.0, 30.0, 1.0);
        for shape in [PerforationShape::Hex, PerforationShape::Slot { length: 5.0 }] {
            let mut p = field(2.0, 6.0, 1.5, shape);
            p.row_pitch = Some(3.0);
            p.check_spacing().unwrap();
            let holes = s.perforation_holes(&p).unwrap();
            assert!(holes.len() > 40, "{shape:?}: {}", holes.len());
            let b: Aabb2 = holes.iter().flat_map(|c| p.hole_loop(*c).polyline(1e-6).iter().copied().collect::<Vec<_>>()).collect();
            assert!((b.min.x - 1.5).abs() < 1e-6 && (b.min.y - 1.5).abs() < 1e-6, "{shape:?}: {b:?}");
            assert!(b.max.x <= 48.5 + 1e-6 && b.max.y <= 28.5 + 1e-6, "{shape:?}: {b:?}");
        }
    }

    #[test]
    fn holes_that_meet_their_neighbours_are_refused() {
        let ok = |p: &SheetPerforation| p.check_spacing().is_ok();
        // A honeycomb: hexagons 5 across flats on a 6 pitch leave 1 between flats; on a 5 pitch none.
        assert!(ok(&field(5.0, 6.0, 1.0, PerforationShape::Hex)));
        assert!(!ok(&field(5.0, 5.0, 1.0, PerforationShape::Hex)));
        // Slots 8 long on an 8 pitch touch end to end; on 9 they don't, and their rows (7.8 apart) don't either.
        assert!(!ok(&field(2.0, 8.0, 1.0, PerforationShape::Slot { length: 8.0 })));
        assert!(ok(&field(2.0, 9.0, 1.0, PerforationShape::Slot { length: 8.0 })));
        // Rows 2 apart: slots 2 wide on staggered rows meet those beside, which reach over them.
        let mut p = field(2.0, 9.0, 1.0, PerforationShape::Slot { length: 8.0 });
        p.row_pitch = Some(2.0);
        assert!(!ok(&p));
        p.row_pitch = Some(2.5);
        assert!(ok(&p));
    }
}
