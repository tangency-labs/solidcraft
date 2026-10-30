//! Complete circles recognized from the documented two-semicircle encoding.
//! This is structural semantics, not a tolerance for closing arbitrary arc joins.

use std::f64::consts::{PI, TAU};

use crate::Loop2;

use super::{Bound, Budget, ClippedLine, Event, Frame, Invalid, Key, MAX_LENGTH_MM, Region2, Result, Seg2, UNCERTAIN, Vec2, event, product, sweep};

#[derive(Clone, Copy, Debug)]
struct Circle {
    center: Vec2,
    radius: f64,
}

#[derive(Debug)]
pub(super) struct CircularRegion {
    outer: Circle,
    inner: Option<Circle>,
}

impl CircularRegion {
    pub(super) fn new(region: &Region2, budget: &mut Budget) -> Result<Self> {
        budget.work(1)?;
        if region.holes.len() > 1 {
            return Err(Invalid("not supported yet: multiple circular hatch holes"));
        }
        // Charge the original Arc records, before reducing a boundary to one
        // circle. No unchecked traversal or allocation precedes these charges.
        for boundary in region.loops() {
            budget.input_pieces(boundary.segs.len())?;
            budget.work(boundary.segs.len())?;
        }
        let outer = Circle::new(&region.outer, budget)?;
        let inner = region.holes.first().map(|boundary| Circle::new(boundary, budget)).transpose()?;
        if let Some(inner) = inner {
            budget.work(1)?;
            if inner.center != outer.center {
                return Err(Invalid("not supported yet: eccentric circular hatch hole"));
            }
            if inner.radius >= outer.radius {
                return Err(Invalid("circular hatch hole must be strictly inside outer circle"));
            }
        }
        Ok(Self { outer, inner })
    }

    pub(super) fn normal_bounds(&self, frame: &Frame, budget: &mut Budget) -> Result<(f64, f64)> {
        budget.work(1)?;
        let (_, height) = frame.project(self.outer.center)?;
        budget.work(1)?;
        let radius = square_root(self.outer.scaled_radius_squared(frame)?, budget)?;
        budget.work(1)?;
        Ok((height.sub(radius)?.lo, height.add(radius)?.hi))
    }

    pub(super) fn clip(&self, frame: &Frame, offset: Bound, budget: &mut Budget) -> Result<ClippedLine> {
        budget.work(1)?;
        let mut events = Vec::new();
        self.outer.events(frame, offset, 0, &mut events, budget)?;
        if let Some(inner) = self.inner {
            inner.events(frame, offset, 1, &mut events, budget)?;
        }
        sweep(events, budget)
    }
}

impl Circle {
    fn new(boundary: &Loop2, budget: &mut Budget) -> Result<Self> {
        budget.work(1)?;
        let [first, second] = boundary.segs.as_slice() else {
            return Err(Invalid("not supported yet: noncanonical circular hatch boundary"));
        };
        // These two checks are covered by the original-piece work charge.
        first.check(MAX_LENGTH_MM).map_err(Invalid)?;
        second.check(MAX_LENGTH_MM).map_err(Invalid)?;
        let (Seg2::Arc { center, radius, start: a, sweep: sa }, Seg2::Arc { center: other_center, radius: other_radius, start: b, sweep: sb }) =
            (*first, *second)
        else {
            return Err(Invalid("not supported yet: mixed circular hatch boundary"));
        };
        let forward = sa == PI && sb == PI && ((a == 0.0 && b == PI) || (b == 0.0 && a == PI));
        let reversed = sa == -PI && sb == -PI && ((a == TAU && b == PI) || (b == TAU && a == PI));
        if center != other_center || radius != other_radius || !(forward || reversed) {
            return Err(Invalid("not supported yet: noncanonical circular hatch boundary"));
        }
        // Evaluated sin/cos endpoints do not certify a closed circle. Only the
        // exact signatures above mean a full circle here, including signed zero
        // equivalence, reversal and cyclic order. No other seam is repaired.
        let radius_bound = Bound::point(radius)?;
        for component in [center.x, center.y] {
            budget.work(1)?;
            let center_bound = Bound::point(component)?;
            if center_bound.sub(radius_bound)?.lo < -MAX_LENGTH_MM || center_bound.add(radius_bound)?.hi > MAX_LENGTH_MM {
                return Err(Invalid("circular hatch boundary exceeds coordinate limit"));
            }
        }
        Ok(Self { center, radius })
    }

    fn scaled_radius_squared(self, frame: &Frame) -> Result<Bound> {
        // The represented u and u.perp are orthogonal but need not have unit
        // length. In this frame (t-tc)^2 + (h-hc)^2 = radius^2 / (u dot u).
        product(self.radius, self.radius)?.div(frame.norm_squared)
    }

    fn events(self, frame: &Frame, offset: Bound, boundary: usize, events: &mut Vec<Event>, budget: &mut Budget) -> Result<()> {
        budget.work(1)?;
        let (center, height) = frame.project(self.center)?;
        budget.work(1)?;
        let distance = offset.sub(height)?;
        let discriminant = self.scaled_radius_squared(frame)?.sub(distance.mul(distance)?)?;
        // Charge even a miss or a tangent that paints nothing. A discriminant
        // straddling zero is unresolved, not a reason to clamp it to zero.
        budget.work(1)?;
        match discriminant.sign()? {
            -1 => Ok(()),
            0 => event(events, Event { at: center, key: Key::Circle(boundary, 0), toggle: false, boundary: 0 }, budget),
            1 => {
                let reach = square_root(discriminant, budget)?;
                budget.work(2)?;
                let left = center.sub(reach)?;
                let right = center.add(reach)?;
                // Keep distinct root identities and enclosures. The shared
                // sweep certifies their order and splits at tangent contacts.
                event(events, Event { at: left, key: Key::Circle(boundary, 0), toggle: true, boundary: 0 }, budget)?;
                event(events, Event { at: right, key: Key::Circle(boundary, 1), toggle: true, boundary: 0 }, budget)
            }
            _ => Err(UNCERTAIN),
        }
    }
}

fn square_root(value: Bound, budget: &mut Budget) -> Result<Bound> {
    budget.work(1)?;
    if value.lo < 0.0 {
        return Err(UNCERTAIN);
    }
    Bound::new(root_endpoint(value.lo, false, budget)?, root_endpoint(value.hi, true, budget)?)
}

fn root_endpoint(value: f64, upper: bool, budget: &mut Budget) -> Result<f64> {
    budget.work(1)?;
    let mut root = value.sqrt();
    // sqrt supplies only a starting candidate. Each returned endpoint proves
    // its square inequality using conservative products; four attempts bound
    // the cost, not an assumed accuracy guarantee for the platform's sqrt.
    for _ in 0..4 {
        budget.work(1)?;
        if !root.is_finite() || root < 0.0 {
            return Err(UNCERTAIN);
        }
        let squared = product(root, root)?;
        let certified = if upper { squared.lo >= value } else { squared.hi <= value };
        if certified {
            return Ok(root);
        }
        root = if upper { root.next_up() } else { root.next_down() };
    }
    Err(UNCERTAIN)
}
