//! Periodic screw sections, land extinction and the surviving radial band.
//!
//! At fixed radius a regular meridian is a height graph G(R,d). Its derivative
//! along a moving cylinder is `G_d*d'+G_x*(rho'-R_d*d')/R_x`. Two such graphs and their
//! pitch translates stay disjoint exactly when their ordered gap stays in (0,P).
//! A fixed sign of the radial gap derivative reduces that test to both radial
//! ends. This checks a continuum of offsets; no sampled section is a certificate.
//! A land can vanish while the band survives, bounded by a unique flank crossing.
//! Its final section closure is distinct from a finite body's global fit limit.
//! The separate focal, global and boundary events follow the distinction in
//! Wallner et al., Self-Intersections of Offset Curves and Surfaces (2001).
//! Deviations: new construction. Interval inverses use at most 48 contractions;
//! family subdivision uses at most 4096 cells and depth 32, without recursion.

pub(super) mod cell;
mod family;

use super::{bad, Ivl, Result, ScrewOffsetSection, Vec3};
use crate::kernel::{CancelToken, Kernel};
use crate::surface::procedural_sweep::screw_offset::MeridianFamilyBounds;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::topology_traverser::face_edges;
use crate::topology::{Brep, FaceId};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy)]
pub(super) struct Affine {
    pub origin: Ivl,
    pub rate: Ivl,
}

impl Affine {
    pub fn at(self, t: Ivl) -> Ivl {
        self.origin + self.rate * t
    }

    /// Range the difference with its shared family parameter. Ranging the two
    /// radii separately loses common motion and can make ordered lands overlap.
    pub fn stays_above(self, other: Self, family: Ivl) -> bool {
        (self.origin - other.origin + (self.rate - other.rate) * family).lo > 0.0
    }
}

pub(super) struct Flank {
    pub section: ScrewOffsetSection,
    /// Original radial coordinate is radius.origin+radius.rate*x.
    pub radius: Affine,
    pub distance: Affine,
    pub radial: Vec3,
    /// The operation's cancel token, checked in each budgeted search.
    pub cancel: Option<CancelToken>,
}

const OP: &str = "offset::procedural_screw::groove";

/// Stops a budgeted search when the operation is cancelled.
fn cancelled(cancel: Option<&CancelToken>) -> Result<()> {
    cancel.map_or(Ok(()), |token| token.check(OP))
}

struct Inverse {
    source: Ivl,
    height: Ivl,
    height_rate: Ivl,
}

struct GraphDerivative {
    radius: Ivl,
    height_by_radius: Ivl,
}

impl Flank {
    fn enclosure(&self, x: Ivl, family: Ivl) -> Result<MeridianFamilyBounds> {
        self.section
            .meridian_family_bounds(x.lo, x.hi, self.radial, self.distance.at(family))
    }

    /// Prove the graph and its orientation on a covering of the source interval.
    /// A single wide interval may lose the normal's algebraic dependencies even
    /// though every point is regular. Subdivision tightens that enclosure; all
    /// accepted cells contribute to the global derivative hull.
    fn graph_derivative(&self, source: Ivl, family: Ivl) -> Result<GraphDerivative> {
        let increasing = self.radius.rate.lo > 0.0;
        if !increasing && self.radius.rate.hi >= 0.0 {
            return Err(bad("thread source radial direction is unresolved"));
        }
        let mut pending = vec![(source, 0_u32)];
        let mut budget = 4096_usize;
        let mut result: Option<GraphDerivative> = None;
        while let Some((cell, depth)) = pending.pop() {
            cancelled(self.cancel.as_ref())?;
            budget = budget
                .checked_sub(1)
                .ok_or_else(|| bad("thread graph certificate budget exceeded"))?;
            let value = self.section.meridian_family_chart_bounds(
                cell.lo,
                cell.hi,
                self.radial,
                self.distance.at(family),
            )?;
            if let Some(value) = value.filter(|value| {
                value.oriented_area.lo > 0.0
                    && if increasing {
                        value.section.radius_rate.lo > 0.0
                    } else {
                        value.section.radius_rate.hi < 0.0
                    }
            }) {
                let radius = value.section.radius_rate;
                let slope = value.section.height_rate / radius;
                result = Some(match result {
                    None => GraphDerivative {
                        radius,
                        height_by_radius: slope,
                    },
                    Some(previous) => GraphDerivative {
                        radius: previous.radius.hull(radius),
                        height_by_radius: previous.height_by_radius.hull(slope),
                    },
                });
                continue;
            }
            let mid = cell.mid();
            if depth >= 32 || !(cell.lo < mid && mid < cell.hi) {
                return Err(bad("thread radial graph or regularity is not proved"));
            }
            pending.push((Ivl::new(mid, cell.hi), depth + 1));
            pending.push((Ivl::new(cell.lo, mid), depth + 1));
        }
        result.ok_or_else(|| bad("thread graph has no source interval"))
    }

    /// ||C+d*n|| differs from the original radius by at most |d|. Add the
    /// operation's `displacement` ([`Band::displacement`]) once more to put the
    /// boundary roots strictly inside the search interval; this is the offset
    /// reach margin, not a body bounding-box multiplier. It is the whole
    /// operation's, not this flank's own move: a flank that stays, its section
    /// starting on a land's radius, has that root at its support's end, which
    /// no endpoint sign proves. Original support endpoints are retained.
    fn domain(&self, radii: [Affine; 2], family: Ivl, displacement: f64) -> Result<Ivl> {
        let [a, b] = radii.map(|r| r.at(family));
        let delta = self.distance.at(family).mag();
        let margin = (Ivl::from(displacement) * Ivl::from(family.mag().max(1.0))).hi;
        let padding = Ivl::from(delta) + Ivl::from(margin);
        let radial = Ivl::new(a.lo.min(b.lo), a.hi.max(b.hi)) + Ivl::new(-padding.hi, padding.hi);
        if radial.lo <= 0.0 || self.radius.rate.mig() <= 0.0 {
            return Err(bad(
                "thread section support reaches its axis or has constant radius",
            ));
        }
        let x = ((radial - self.radius.origin) / self.radius.rate).hull(Ivl::new(0.0, 1.0));
        if !x.lo.is_finite() || !x.hi.is_finite() || x.lo >= x.hi {
            return Err(bad("thread radial inverse has no finite support"));
        }
        Ok(x)
    }

    fn inverse(&self, x: Ivl, radius: Affine, family: Ivl) -> Result<Inverse> {
        self.inverse_cell(x, radius, family)?
            .ok_or_else(|| bad("thread radial inverse chart is unresolved"))
    }

    fn inverse_cell(&self, mut x: Ivl, radius: Affine, family: Ivl) -> Result<Option<Inverse>> {
        let enclose = |source: Ivl| {
            self.section.meridian_family_chart_bounds(
                source.lo,
                source.hi,
                self.radial,
                self.distance.at(family),
            )
        };
        let derivative = self.graph_derivative(x, family)?;
        let increasing = derivative.radius.lo > 0.0;
        let target = radius.at(family);
        let (Some(left), Some(right)) = (enclose(Ivl::from(x.lo))?, enclose(Ivl::from(x.hi))?)
        else {
            return Ok(None);
        };
        let (left, right) = (left.section.radius, right.section.radius);
        if !(if increasing {
            left.hi < target.lo && right.lo > target.hi
        } else {
            right.hi < target.lo && left.lo > target.hi
        }) {
            return Ok(None);
        }
        // Strict endpoint signs and R_x of one sign prove one root for every
        // family member. Interval Newton only contracts their common enclosure.
        for _ in 0..48 {
            let mid = x.mid();
            let Some(at) = enclose(Ivl::from(mid))? else {
                return Ok(None);
            };
            let next = Ivl::from(mid) - (at.section.radius - target) / derivative.radius;
            let next = Ivl::new(x.lo.max(next.lo), x.hi.min(next.hi));
            if next.lo > next.hi {
                return Err(bad("thread radial inverse lost its proved root"));
            }
            if next == x {
                break;
            }
            x = next;
        }
        let Some(value) = enclose(x)? else {
            return Ok(None);
        };
        // Both enclosures contain the actual derivative at every root. The
        // independently proved global graph bound preserves its strict sign.
        let root_derivative = Ivl::new(
            value.section.radius_rate.lo.max(derivative.radius.lo),
            value.section.radius_rate.hi.min(derivative.radius.hi),
        );
        if root_derivative.lo > root_derivative.hi {
            return Err(bad("thread radial derivative enclosures disagree"));
        }
        let dx = (radius.rate - value.radius_distance * self.distance.rate) / root_derivative;
        let height_rate =
            value.height_distance * self.distance.rate + value.section.height_rate * dx;
        Ok(Some(Inverse {
            source: x,
            height: value.section.height,
            height_rate,
        }))
    }
}

pub(super) struct Band {
    pub flanks: [Flank; 2],
    pub radii: [Affine; 2],
    pub pitch: f64,
    /// Integer pitch translate putting the second section above the first.
    pub winding: i64,
    /// The operation's cancel token, checked in each budgeted search.
    pub cancel: Option<CancelToken>,
}

/// Boundary of the retained section after the cylinder lands are clipped by
/// the periodic flank ordering. Wall parameters belong to the retained sources.
pub(super) enum SectionBoundary {
    Cylinder {
        index: usize,
        radius: Ivl,
        sources: [Ivl; 2],
    },
    Crossing {
        /// second height + winding*pitch = first height.
        winding: i64,
        radius: Ivl,
        sources: [Ivl; 2],
    },
}

struct State {
    gaps: [Ivl; 4],
    rates: [Ivl; 4],
    radial_rate: Ivl,
}

fn hull_four([a, b, c, d]: [Ivl; 4], [e, f, g, h]: [Ivl; 4]) -> [Ivl; 4] {
    [a.hull(e), b.hull(f), c.hull(g), d.hull(h)]
}

impl State {
    /// A continuous strictly increasing D(R) has some 0<D<P exactly when
    /// D(upper)>0 and D(lower)<P. Reverse the ends for a decreasing D.
    fn fit_indices(&self) -> Result<[usize; 2]> {
        if self.radial_rate.lo > 0.0 {
            Ok([2, 1])
        } else if self.radial_rate.hi < 0.0 {
            Ok([0, 3])
        } else {
            Err(bad("thread radial order changes over the offset family"))
        }
    }
}

impl Band {
    /// The operation's displacement per unit family: the largest rate of either
    /// flank's offset and of either land's radius.
    fn displacement(&self) -> f64 {
        self.flanks
            .iter()
            .map(|flank| flank.distance.rate.mag())
            .chain(self.radii.iter().map(|r| r.rate.mag()))
            .fold(0.0, f64::max)
    }

    /// Prove a nonempty ordered section throughout [0,end], including after a
    /// land vanishes, and return its two boundaries at end. Since `D_R` never
    /// vanishes, each clipping root is unique and continues by the implicit
    /// function theorem until it reaches the other radial boundary. Restricting
    /// to 0<D<P excludes every non-neighboring pitch translate as well.
    pub fn surviving_section(&self, end: f64) -> Result<[SectionBoundary; 2]> {
        if !end.is_finite() || end <= 0.0 {
            return Err(bad("invalid thread section family"));
        }
        if self.fit_event(end)?.is_some() {
            return Err(bad("offset thread section has disappeared"));
        }
        let state = self.state(Ivl::from(end))?;
        let [lower, _, upper, _] = state.gaps;
        let increasing = state.radial_rate.lo > 0.0;
        let bounds = [(0, lower), (1, upper)].map(|(index, gap)| {
            let radius = self
                .radii
                .get(index)
                .copied()
                .ok_or_else(|| bad("invalid thread radial boundary"))?
                .at(Ivl::from(end));
            let target = if gap.hi < 0.0 {
                Some(0.0)
            } else if gap.lo > self.pitch {
                Some(self.pitch)
            } else if gap.lo > 0.0 && gap.hi < self.pitch {
                None
            } else {
                return Err(bad("thread section is at an unresolved land event"));
            };
            match target {
                None => Ok(SectionBoundary::Cylinder {
                    index,
                    radius,
                    sources: self.sources_at(radius, end)?,
                }),
                Some(target) => {
                    // The strict endpoint signs and D_R prove one root; interval
                    // Newton contracts it without changing its branch identity.
                    if (index == 0) != (increasing == (target == 0.0)) {
                        return Err(bad("thread section has no surviving radial interval"));
                    }
                    let radius = self.crossing_radius(target, end, state.radial_rate)?;
                    Ok(SectionBoundary::Crossing {
                        winding: self.winding - i64::from(target != 0.0),
                        radius,
                        sources: self.sources_at(radius, end)?,
                    })
                }
            }
        });
        let [lower, upper] = bounds;
        let bounds = [lower?, upper?];
        for boundary in &bounds {
            let (radius, sources) = match boundary {
                SectionBoundary::Cylinder {
                    radius, sources, ..
                } => (radius, sources),
                SectionBoundary::Crossing {
                    winding,
                    radius,
                    sources,
                } => {
                    if *winding != self.winding && *winding != self.winding - 1 {
                        return Err(bad("thread crossing is not a neighboring periodic branch"));
                    }
                    (radius, sources)
                }
            };
            if !radius.lo.is_finite()
                || !radius.hi.is_finite()
                || sources
                    .iter()
                    .any(|s| !s.lo.is_finite() || !s.hi.is_finite())
            {
                return Err(bad("thread boundary enclosure is not finite"));
            }
        }
        Ok(bounds)
    }

    fn sources_at(&self, radius: Ivl, end: f64) -> Result<[Ivl; 2]> {
        let [a, b] = self.flanks.each_ref().map(|flank| {
            let family = Ivl::from(end);
            flank
                .inverse(
                    flank.domain(self.radii, family, self.displacement())?,
                    Affine {
                        origin: radius,
                        rate: Ivl::from(0.0),
                    },
                    family,
                )
                .map(|value| value.source)
        });
        Ok([a?, b?])
    }

    fn crossing_radius(&self, target: f64, end: f64, derivative: Ivl) -> Result<Ivl> {
        let family = Ivl::from(end);
        let [lower, upper] = self.radii.map(|r| r.at(family));
        let mut radius = Ivl::new(lower.lo, upper.hi);
        for _ in 0..48 {
            let mid = radius.mid();
            let [a, b] = self.flanks.each_ref().map(|flank| {
                flank
                    .inverse(
                        flank.domain(self.radii, family, self.displacement())?,
                        Affine {
                            origin: Ivl::from(mid),
                            rate: Ivl::from(0.0),
                        },
                        family,
                    )
                    .map(|value| value.height)
            });
            let gap = b? - a? + Ivl::from(self.winding as f64) * Ivl::from(self.pitch)
                - Ivl::from(target);
            let next = Ivl::from(mid) - gap / derivative;
            let next = Ivl::new(radius.lo.max(next.lo), radius.hi.min(next.hi));
            if next.lo > next.hi {
                return Err(bad("thread flank crossing lost its proved root"));
            }
            if next == radius {
                break;
            }
            radius = next;
        }
        Ok(radius)
    }

    /// Enclose the first of all four boundary extinction events through `end`.
    /// Every other boundary gap is proved positive up to that event, by its
    /// signed family derivative or a positive whole-family enclosure. `state`
    /// also proves radial ordering and local regularity over the entire path.
    pub fn first_event(&self, end: f64) -> Result<Option<Ivl>> {
        self.boundary_event(end, false)
    }

    /// The first disappearance of the surviving radial section, allowing a
    /// cylindrical land to be replaced by the flanks' unique helical meeting.
    /// This is a section event, not a largest offset for a finite B-rep: caps,
    /// other walls, and the construction of changed adjacency are separate.
    pub fn fit_event(&self, end: f64) -> Result<Option<Ivl>> {
        self.boundary_event(end, true)
    }

    fn boundary_event(&self, end: f64, surviving: bool) -> Result<Option<Ivl>> {
        if !end.is_finite()
            || end <= 0.0
            || !self.pitch.is_finite()
            || self.pitch <= 0.0
            || self.winding.unsigned_abs() > 12_000
        {
            return Err(bad("invalid thread event family"));
        }
        let initial = self.state(Ivl::from(0.0))?;
        let final_state = self.state(Ivl::from(end))?;
        let whole = self.state(Ivl::new(0.0, end))?;
        let mut result: Option<Ivl> = None;
        for (index, (((start, finish), gap), rate)) in initial
            .gaps
            .into_iter()
            .zip(final_state.gaps)
            .zip(whole.gaps)
            .zip(whole.rates)
            .enumerate()
        {
            if surviving && !whole.fit_indices()?.contains(&index) {
                continue;
            }
            if start.lo <= 0.0 {
                return Err(bad("original thread sections are not proved ordered"));
            }
            if gap.lo > 0.0 || rate.lo >= 0.0 || (rate.hi < 0.0 && finish.lo > 0.0) {
                continue;
            }
            if finish.lo > 0.0 {
                self.positive_gap_through(index, end)?;
                continue;
            }
            if finish.hi >= 0.0 {
                return Err(bad("first thread boundary event is not resolved"));
            }
            let event = self.event_bracket(index, end)?;
            result = Some(match result {
                None => event,
                Some(previous) => Ivl::new(previous.lo.min(event.lo), previous.hi.min(event.hi)),
            });
        }
        Ok(result)
    }

    /// A nonmonotone gap can still remain strictly positive. Cover the whole
    /// family by positive interval enclosures rather than infer its sign from
    /// endpoint evaluations or require one global derivative enclosure.
    fn positive_gap_through(&self, gap: usize, end: f64) -> Result<()> {
        let mut pending = vec![(Ivl::new(0.0, end), 0_u32)];
        let mut budget = 4096_usize;
        while let Some((family, depth)) = pending.pop() {
            cancelled(self.cancel.as_ref())?;
            budget = budget
                .checked_sub(1)
                .ok_or_else(|| bad("thread gap positivity budget exceeded"))?;
            let value = self
                .state(family)?
                .gaps
                .get(gap)
                .copied()
                .ok_or_else(|| bad("invalid thread event index"))?;
            if value.lo > 0.0 {
                continue;
            }
            let mid = family.mid();
            if value.hi <= 0.0 || depth >= 32 || !(family.lo < mid && mid < family.hi) {
                return Err(bad(
                    "thread gap is not proved positive throughout the family",
                ));
            }
            pending.push((Ivl::new(mid, family.hi), depth + 1));
            pending.push((Ivl::new(family.lo, mid), depth + 1));
        }
        Ok(())
    }

    fn state(&self, family: Ivl) -> Result<State> {
        let mut pending = vec![(family, 0_u32)];
        let mut budget = 4096_usize;
        let mut result: Option<State> = None;
        while let Some((cell, depth)) = pending.pop() {
            cancelled(self.cancel.as_ref())?;
            budget = budget
                .checked_sub(1)
                .ok_or_else(|| bad("thread family chart budget exceeded"))?;
            if let Some(state) = self.state_cell(cell)? {
                result = Some(match result {
                    None => state,
                    Some(previous) => State {
                        gaps: hull_four(previous.gaps, state.gaps),
                        rates: hull_four(previous.rates, state.rates),
                        radial_rate: previous.radial_rate.hull(state.radial_rate),
                    },
                });
                continue;
            }
            let mid = cell.mid();
            if depth >= 32 || !(cell.lo < mid && mid < cell.hi) {
                return Err(bad(
                    "thread radial ordering is not proved over its offset family",
                ));
            }
            pending.push((Ivl::new(mid, cell.hi), depth + 1));
            pending.push((Ivl::new(cell.lo, mid), depth + 1));
        }
        result.ok_or_else(|| bad("thread family has no interval"))
    }

    /// A chart enclosure can lose the sign of the radial gap derivative over
    /// a wide offset interval. None requests subdivision of that interval;
    /// every accepted cell still proves regularity and ordering everywhere.
    fn state_cell(&self, family: Ivl) -> Result<Option<State>> {
        let [lower, upper] = self.radii;
        if !upper.stays_above(lower, family) {
            return Ok(None);
        }
        let [a, b] = &self.flanks;
        let xa = a.domain(self.radii, family, self.displacement())?;
        let xb = b.domain(self.radii, family, self.displacement())?;
        let [a_lower, a_upper] = self.radii.map(|r| a.inverse_cell(xa, r, family));
        let [b_lower, b_upper] = self.radii.map(|r| b.inverse_cell(xb, r, family));
        let (Some(a_lower), Some(a_upper), Some(b_lower), Some(b_upper)) =
            (a_lower?, a_upper?, b_lower?, b_upper?)
        else {
            return Ok(None);
        };
        // The search support has padding to bracket the inverse. Radial gap
        // ordering is needed only between the two cylinder roots, not on that
        // padding, where even a regular continued wall can turn in height.
        // Monotone R(x) proves these endpoint-root hulls cover the entire band.
        let aa = a.graph_derivative(a_lower.source.hull(a_upper.source), family)?;
        let bb = b.graph_derivative(b_lower.source.hull(b_upper.source), family)?;
        let radial_rate = bb.height_by_radius - aa.height_by_radius;
        if radial_rate.mig() <= 0.0 {
            return Ok(None);
        }
        let [(lower, dlower), (upper, dupper)] =
            [(a_lower, b_lower), (a_upper, b_upper)].map(|(first, second)| {
                (
                    second.height - first.height
                        + Ivl::from(self.winding as f64) * Ivl::from(self.pitch),
                    second.height_rate - first.height_rate,
                )
            });
        let period = Ivl::from(self.pitch);
        Ok(Some(State {
            gaps: [lower, period - lower, upper, period - upper],
            rates: [
                dlower,
                Ivl::from(0.0) - dlower,
                dupper,
                Ivl::from(0.0) - dupper,
            ],
            radial_rate,
        }))
    }

    /// Proves every intermediate offset on [0,1] remains a regular, ordered
    /// periodic band. Returns the final roots needed for support continuation.
    pub fn certify(&self) -> Result<[[Ivl; 2]; 2]> {
        if !self.pitch.is_finite() || self.pitch <= 0.0 || self.winding.unsigned_abs() > 12_000 {
            return Err(bad(
                "thread band needs a finite positive pitch and bounded winding",
            ));
        }
        // This constructor still requires two cylindrical lands. The surviving
        // section certificate is also valid after their extinction, but using
        // its Crossing boundary requires the topology builder to change adjacency.
        let boundaries = self.surviving_section(1.0)?;
        for (expected, boundary) in boundaries.iter().enumerate() {
            match boundary {
                SectionBoundary::Cylinder {
                    index,
                    radius,
                    sources,
                } => {
                    if *index != expected
                        || !radius.lo.is_finite()
                        || sources
                            .iter()
                            .any(|s| !s.lo.is_finite() || !s.hi.is_finite())
                    {
                        return Err(bad("thread cylinder boundary is unresolved"));
                    }
                }
                SectionBoundary::Crossing { .. } => {
                    return Err(bad(
                        "thread land is replaced by a helical crossing; changed adjacency is required",
                    ));
                }
            }
        }
        let initial = self.state(Ivl::from(0.0))?;
        if initial.gaps.iter().any(|gap| gap.lo <= 0.0) {
            return Err(bad("original thread sections are not proved ordered"));
        }
        let mut pending = vec![(Ivl::new(0.0, 1.0), 0_u32)];
        let mut budget = 4096_usize;
        while let Some((family, depth)) = pending.pop() {
            cancelled(self.cancel.as_ref())?;
            budget = budget
                .checked_sub(1)
                .ok_or_else(|| bad("thread ordering certificate budget exceeded"))?;
            let state = self.state(family)?;
            if state.gaps.iter().all(|gap| gap.lo > 0.0) {
                continue;
            }
            if state.gaps.iter().any(|gap| gap.hi <= 0.0) {
                return Err(bad("offset thread sections reach a land-extinction event"));
            }
            let mid = family.mid();
            if depth >= 32 || !(family.lo < mid && mid < family.hi) {
                return Err(bad(
                    "thread section ordering is unresolved at a boundary event",
                ));
            }
            pending.push((Ivl::new(mid, family.hi), depth + 1));
            pending.push((Ivl::new(family.lo, mid), depth + 1));
        }
        let final_family = Ivl::from(1.0);
        let [first, second] = self.flanks.each_ref().map(|flank| -> Result<[Ivl; 2]> {
            let source = flank.domain(self.radii, Ivl::new(0.0, 1.0), self.displacement())?;
            let [a, b] = self.radii;
            Ok([
                flank.inverse(source, a, final_family)?.source,
                flank.inverse(source, b, final_family)?.source,
            ])
        });
        Ok([first?, second?])
    }

    /// A zero of a strictly decreasing endpoint gap is the unique first event
    /// for that gap. Returned root enclosures come from interval signs,
    /// never from rebuilding offsets or a sampled geometric predicate.
    pub fn event_bracket(&self, gap: usize, end: f64) -> Result<Ivl> {
        let rate = self.decreasing_gap_rate(gap, end)?;
        let value = |t: f64| -> Result<Ivl> {
            self.state(Ivl::from(t))?
                .gaps
                .get(gap)
                .copied()
                .ok_or_else(|| bad("invalid thread event index"))
        };
        if value(0.0)?.lo <= 0.0 || value(end)?.hi >= 0.0 {
            return Err(bad("thread event has no proved sign bracket"));
        }
        let (mut lo, mut hi) = (0.0, end);
        for _ in 0..64 {
            let mid = lo + (hi - lo) * 0.5;
            if !(lo < mid && mid < hi) {
                break;
            }
            let at = value(mid)?;
            if at.lo > 0.0 {
                lo = mid;
            } else if at.hi < 0.0 {
                hi = mid;
            } else {
                // The root enclosure follows from the mean-value theorem and
                // the proved derivative over the complete original bracket.
                let root = Ivl::from(mid) - at / rate;
                return Ok(Ivl::new(lo.max(root.lo), hi.min(root.hi)));
            }
        }
        Ok(Ivl::new(lo, hi))
    }

    /// The implicit radius root varies with the requested offset. Enclosing
    /// that entire motion in one interval can lose the sign of dG/dlambda.
    /// Prove that sign over a covering of the complete family, and retain the
    /// hull as the mean-value bound used by event root isolation.
    fn decreasing_gap_rate(&self, gap: usize, end: f64) -> Result<Ivl> {
        if !end.is_finite() || end <= 0.0 {
            return Err(bad("invalid thread event family"));
        }
        let mut pending = vec![(Ivl::new(0.0, end), 0_u32)];
        let mut budget = 4096_usize;
        let mut result: Option<Ivl> = None;
        while let Some((family, depth)) = pending.pop() {
            cancelled(self.cancel.as_ref())?;
            budget = budget
                .checked_sub(1)
                .ok_or_else(|| bad("thread event derivative budget exceeded"))?;
            let rate = self
                .state(family)?
                .rates
                .get(gap)
                .copied()
                .ok_or_else(|| bad("invalid thread event index"))?;
            if rate.hi < 0.0 {
                result = Some(result.map_or(rate, |old| old.hull(rate)));
                continue;
            }
            let mid = family.mid();
            if rate.lo >= 0.0 || depth >= 32 || !(family.lo < mid && mid < family.hi) {
                return Err(bad("thread event monotonicity is not proved"));
            }
            pending.push((Ivl::new(mid, family.hi), depth + 1));
            pending.push((Ivl::new(family.lo, mid), depth + 1));
        }
        result.ok_or_else(|| bad("thread event has no offset interval"))
    }
}

type Box3 = [Ivl; 3];
fn boxed(v: Vec3) -> Box3 {
    [Ivl::from(v.x), Ivl::from(v.y), Ivl::from(v.z)]
}
fn subtract(a: Box3, b: Box3) -> Box3 {
    let [x, y, z] = a;
    let [u, v, w] = b;
    [x - u, y - v, z - w]
}
fn dot(a: Box3, b: Box3) -> Ivl {
    let [x, y, z] = a;
    let [u, v, w] = b;
    x * u + y * v + z * w
}
fn cross(a: Box3, b: Box3) -> Box3 {
    let [x, y, z] = a;
    let [u, v, w] = b;
    [y * w - z * v, z * u - x * w, x * v - y * u]
}
fn norm(a: Box3) -> Ivl {
    let [x, y, z] = a.map(|v| Ivl::from(v.mag()));
    (x * x + y * y + z * z).sqrt()
}

/// Axial motion of a helix/plane endpoint. Unlike an interior fixed-azimuth
/// point, the endpoint also changes azimuth to stay on the tilted plane.
pub(super) fn cap_axial_motion(
    normal: [Ivl; 3],
    axis: Vec3,
    rise: Ivl,
    radius: Ivl,
    height_rate: Ivl,
    radius_rate: Ivl,
) -> Result<f64> {
    let axial = dot(normal, boxed(axis));
    let transverse = norm(cross(normal, boxed(axis)));
    let denominator =
        Ivl::from(rise.mig()) * Ivl::from(axial.mig()) - transverse * Ivl::from(radius.mag());
    if denominator.lo <= 0.0 {
        return Err(bad("thread cap crossing is not proved transverse"));
    }
    // Differentiate n.(rho*e(phi)+axis*(G+h*phi)-p)=0.
    // Cancellation of n_axis*h*G_lambda leaves this axial bound.
    let motion = transverse
        * (Ivl::from(radius.mag()) * Ivl::from(height_rate.mag())
            + Ivl::from(rise.mag()) * Ivl::from(radius_rate.mag()))
        / denominator;
    if !motion.hi.is_finite() {
        return Err(bad("thread cap endpoint motion is unbounded"));
    }
    Ok(motion.hi)
}

/// Enclose the cap's axial coordinates over the complete radial disk and prove
/// every screw orbit crosses it transversely. Two returned slabs must also be
/// disjoint before their cap crossings are known to be ordered.
pub(super) fn cap_slab(
    placement: &crate::geom::axis2_placement::Axis2Placement,
    origin: Vec3,
    axis: Vec3,
    rise: Ivl,
    radius_max: f64,
) -> Result<Ivl> {
    let n = cross(boxed(placement.x_axis()), boxed(placement.y_axis()));
    let axial = dot(n, boxed(axis));
    let normal_perp = cross(n, boxed(axis));
    let amplitude = norm(normal_perp) * Ivl::from(radius_max);
    if axial.mig() <= 0.0 || (Ivl::from(rise.mig()) * Ivl::from(axial.mig())).lo <= amplitude.hi {
        return Err(bad(
            "thread endcap is not proved transverse to every screw orbit",
        ));
    }
    let c = dot(n, subtract(boxed(placement.origin()), boxed(origin)));
    Ok((c + Ivl::new(-amplitude.hi, amplitude.hi)) / axial)
}

/// Parameter enclosure of the entire cap disk. The carrier axis is one of the
/// signed coordinate axes, as checked by the thread-class recognizer. Inverting
/// the literal Gram matrix retains stored tilt and scale without normalization.
pub(super) fn cap_disk_parameters(
    placement: &crate::geom::axis2_placement::Axis2Placement,
    scale: [f64; 2],
    origin: Vec3,
    axis: Vec3,
    radius_max: f64,
    height: Ivl,
) -> Result<[Ivl; 2]> {
    let [kx, ky, kz] = [axis.x, axis.y, axis.z];
    let disk = [kx, ky, kz].map(|k| {
        if k == 0.0 {
            Ivl::new(-radius_max, radius_max)
        } else {
            Ivl::from(k) * height
        }
    });
    let [x, y, z] = boxed(origin);
    let [a, b, c] = disk;
    let q = subtract([x + a, y + b, z + c], boxed(placement.origin()));
    let x = boxed(placement.x_axis());
    let y = boxed(placement.y_axis());
    let xx = dot(x, x);
    let xy = dot(x, y);
    let yy = dot(y, y);
    let determinant = xx * yy - xy * xy;
    let [sx, sy] = scale;
    if determinant.lo <= 0.0 || sx == 0.0 || sy == 0.0 {
        return Err(bad("thread endcap placement is singular"));
    }
    let u = (yy * dot(x, q) - xy * dot(y, q)) / determinant / Ivl::from(sx);
    let v = (xx * dot(y, q) - xy * dot(x, q)) / determinant / Ivl::from(sy);
    Ok([u, v])
}

/// How far a periodic thread operation extends each face's surface
/// ([`operation_reach`]): one common length, an end cap's own longer margin
/// where its stored support stops short of the thread's radial disk, and the turns
/// a screw wall's support is continued by where its stored turns stop short of the
/// meetings at the caps.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ThreadReach {
    /// The certified reach every surface is extended by.
    pub reach: f64,
    /// The end caps that need more, each with its margin (at least `reach`).
    pub caps: Vec<(FaceId, f64)>,
    /// The angle, radians, a screw wall's support is continued past its stations by,
    /// beyond `reach`: how far the moved sections' meetings with the caps lie past the
    /// turns the walls' stored supports cover. A screw wall is the same surface along its
    /// turns, so it is continued along them by this angle, not by a length that would
    /// carry its profile as far.
    pub turns: f64,
}

impl ThreadReach {
    /// The margin the surface of original face `face` is extended by.
    pub(crate) fn margin(&self, face: FaceId) -> f64 {
        self.caps
            .iter()
            .find(|(id, _)| *id == face)
            .map_or(self.reach, |(_, m)| m.max(self.reach))
    }
}

/// A common periodic thread operation supplies its geometric reach, rather than
/// interpreting the body's bounding sphere as a displacement. The class consists
/// of two retained affine screw sections, their coaxial cylindrical lands, and
/// two unchanged transverse end planes. Segmented land faces are grouped by their
/// literal canonical radius; all their actual placement scales are enclosed.
pub(crate) fn operation_reach(
    k: &Kernel,
    body: &Brep,
    faces: &[FaceId],
    replacements: &[Surface],
) -> Result<Option<ThreadReach>> {
    if faces.len() != replacements.len() {
        return Err(bad("thread replacement count does not match its face list"));
    }
    let Some(families) = family::collect(body, faces, replacements)? else {
        return Ok(None);
    };
    let [family_a, family_b] = families.as_slice() else {
        return Ok(None);
    };
    let first = family_a.first()?;
    let second = family_b.first()?;
    if !first.1.is_normal_offset() && !second.1.is_normal_offset() {
        return Ok(None);
    }
    let Some(motion) = first.1.screw() else {
        return Ok(None);
    };
    if !family::same_motion(second.1.screw(), Some(motion)) {
        return Ok(None);
    }
    let origin = motion.origin();
    let axis = motion.axis();
    let axes = [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z];
    if !axes.contains(&axis) {
        return Ok(None);
    }
    let radial = axes
        .into_iter()
        .find(|&e| {
            if axis.dot(e) != 0.0 {
                return false;
            }
            let f = axis.cross(e);
            [first, second].into_iter().all(|wall| {
                wall.2 .0.into_iter().all(|p| {
                    let q = subtract(boxed(p), boxed(origin));
                    dot(q, boxed(e)).lo > 0.0
                        && [p.x, p.y, p.z]
                            .into_iter()
                            .zip([origin.x, origin.y, origin.z])
                            .zip([f.x, f.y, f.z])
                            .all(|((p, o), f)| f == 0.0 || p == o)
                })
            })
        })
        .ok_or_else(|| bad("thread sources have no common exact radial half-plane"))?;
    let mut adjoining = BTreeSet::new();
    for wall in families.iter().flat_map(|family| &family.members) {
        adjoining.extend(face_edges(body, wall.0)?);
    }
    let mut cylinders: Vec<(f64, f64, Affine)> = Vec::new();
    let mut planes = Vec::new();
    let mut cap_ids = Vec::new();
    let mut cylinder_ids = BTreeMap::new();
    let mut outside = Vec::new();
    for (&id, new) in faces.iter().zip(replacements) {
        if new.procedural_sweep().is_some() {
            continue;
        }
        let old = body
            .face(id)?
            .surface
            .as_ref()
            .ok_or_else(|| bad("thread face has no surface"))?;
        let adjacent = face_edges(body, id)?.iter().any(|e| adjoining.contains(e));
        match (old.kind_with_nurb(k)?, new.kind_with_nurb(k)?) {
            (SurfaceKind::Cone(old), SurfaceKind::Cone(new)) => {
                let a = old.canonical();
                let b = new.canonical();
                if a.origin != b.origin || a.semi_angle_deg != 0.0 || b.semi_angle_deg != 0.0 {
                    return Ok(None);
                }
                // Radius changes preserve the axial chart. Arbitrary shortening
                // or reversal is a different local operation and has no reach
                // proof from the original boundary displacement alone.
                let axial_chart = |cone: &crate::surface::cone::Cone| -> Result<_> {
                    let domain = cone.rev().base().natural_uv_domain();
                    let native = if cone.canonical().swap_uv {
                        domain.u_interval()
                    } else {
                        domain.v_interval()
                    };
                    Ok((
                        native,
                        crate::surface::cone::checked::HeightMap::new(k, cone)?
                            .exact_coefficients(),
                    ))
                };
                if axial_chart(old)? != axial_chart(new)? {
                    return Ok(None);
                }
                let (Some(ra), Some(rb)) = (
                    old.coaxial_radius_enclosure(origin, axis)?,
                    new.coaxial_radius_enclosure(origin, axis)?,
                ) else {
                    return Ok(None);
                };
                let radius = Affine {
                    origin: ra,
                    rate: rb - ra,
                };
                if !super::cylinder::complete_angular_support(k, new, axis, body.tolerance())? {
                    return Ok(None);
                }
                if adjacent {
                    cylinder_ids.insert(id, a.radius);
                    if let Some((_, target, entry)) =
                        cylinders.iter_mut().find(|(r, _, _)| *r == a.radius)
                    {
                        if *target != b.radius {
                            return Ok(None);
                        }
                        entry.origin = entry.origin.hull(radius.origin);
                        entry.rate = entry.rate.hull(radius.rate);
                    } else {
                        cylinders.push((a.radius, b.radius, radius));
                    }
                } else {
                    outside.push(radius);
                }
            }
            (SurfaceKind::Plane(old), SurfaceKind::Plane(new)) => {
                if old.position() != new.position() || old.uv_scale() != new.uv_scale() {
                    return Ok(None);
                }
                if !adjacent {
                    return Ok(None);
                }
                planes.push(new);
                cap_ids.push(id);
            }
            _ => return Ok(None),
        }
    }
    cylinders.sort_by(|a, b| a.0.total_cmp(&b.0));
    let [(_, _, lower), (_, _, upper)] = cylinders.as_slice() else {
        return Ok(None);
    };
    let [cap_a, cap_b] = planes.as_slice() else {
        return Ok(None);
    };
    let mut labels = BTreeMap::new();
    for (id, radius) in cylinder_ids {
        let label = cylinders
            .iter()
            .position(|entry| entry.0 == radius)
            .ok_or_else(|| bad("thread cylinder has no support group"))?;
        labels.insert(id, label);
    }
    for (label, &id) in cap_ids.iter().enumerate() {
        labels.insert(id, label + 2);
    }
    for family in &families {
        let members: Vec<_> = family.members.iter().map(|wall| wall.0).collect();
        cell::four_chains(body, &members, &labels)?;
    }
    let radii = [*lower, *upper];
    let family = Ivl::new(0.0, 1.0);
    if !upper.stays_above(*lower, family) {
        return Err(bad(
            "thread cylindrical lands are not proved radially ordered",
        ));
    }
    for radius in &outside {
        if !(lower.stays_above(*radius, family) || radius.stays_above(*upper, family)) {
            return Err(bad("another cylindrical wall reaches the thread band"));
        }
    }
    let flanks = [first, second].map(|wall| -> Result<Flank> {
        let ([a, b], _, d0) = wall.2;
        let d1 = wall.3 .2;
        Ok(Flank {
            section: ScrewOffsetSection::new(a, b, motion, d1)?,
            radius: Affine {
                origin: dot(subtract(boxed(a), boxed(origin)), boxed(radial)),
                rate: dot(subtract(boxed(b), boxed(a)), boxed(radial)),
            },
            distance: Affine {
                origin: Ivl::from(d0),
                rate: Ivl::from(d1) - Ivl::from(d0),
            },
            radial,
            cancel: k.cancel.clone(),
        })
    });
    let [a, b] = flanks;
    let mut band = Band {
        flanks: [a?, b?],
        radii,
        pitch: motion.pitch().abs(),
        winding: 0,
        cancel: k.cancel.clone(),
    };
    let pad = band.displacement();
    let [a, b] = &band.flanks;
    let x = a.domain(radii, family, pad)?;
    let y = b.domain(radii, family, pad)?;
    let ga = a.inverse(x, *lower, Ivl::from(0.0))?.height;
    let gb = b.inverse(y, *lower, Ivl::from(0.0))?.height;
    // This chooses an integer chart only; the complete interval ordering below
    // proves that the chosen translate is the neighboring periodic branch.
    let winding = -((gb - ga).mid() / band.pitch).floor();
    if !winding.is_finite() || winding.abs() > 12_000.0 {
        return Err(bad("thread neighbor winding exceeds its budget"));
    }
    band.winding = winding as i64;
    let mut radius_max = upper.at(family).hi;
    for r in &outside {
        radius_max = radius_max.max(r.at(family).hi);
    }
    let rise = Ivl::from(motion.pitch()) / Ivl::checked(super::TAU, super::TAU);
    let ca = cap_slab(cap_a.position(), origin, axis, rise, radius_max)?;
    let cb = cap_slab(cap_b.position(), origin, axis, rise, radius_max)?;
    let caps = if ca.hi < cb.lo {
        ca.hull(cb)
    } else if cb.hi < ca.lo {
        cb.hull(ca)
    } else {
        return Err(bad("thread endcaps are not proved disjoint"));
    };
    let mut reach = 0.0_f64;
    let mut turns = 0.0_f64;
    let mut displacement = 0.0_f64;
    // A full circular cylinder has no angular support end. At fixed world
    // azimuth, each boundary helix's height changes by G_lambda, so this bound
    // covers every segmented land's axial continuation, including its cap ends.
    for flank in &band.flanks {
        let domain = flank.domain(radii, family, pad)?;
        for radius in radii {
            let height_rate = flank.inverse(domain, radius, family)?.height_rate;
            reach = reach.max(height_rate.mag());
            for plane in [cap_a, cap_b] {
                let position = plane.position();
                let n = cross(boxed(position.x_axis()), boxed(position.y_axis()));
                reach = reach.max(cap_axial_motion(
                    n,
                    axis,
                    rise,
                    Ivl::from(radius_max),
                    height_rate,
                    radius.rate,
                )?);
            }
        }
    }
    // The unchanged caps must cover the entire radial disk. Invert the literal
    // placement's Gram matrix; no orthonormality assumption discards its stored
    // tilt. This avoids demanding an irrelevant azimuthal extension proportional
    // to radius/pitch at a moving helix/cap corner. A plane continues exactly, so
    // a cap whose stored support stops short of the disk (a support kept at its
    // face's box) is extended over it: by its overshoot past each side, at the
    // plane's speed along that axis, as that cap's own margin.
    let mut cap_needs = Vec::new();
    for ((plane, height), &id) in [(*cap_a, ca), (*cap_b, cb)].iter().zip(&cap_ids) {
        let (plane, height) = (*plane, *height);
        let scale = plane.uv_scale();
        let [u, v] = cap_disk_parameters(
            plane.position(),
            [scale.x, scale.y],
            origin,
            axis,
            radius_max,
            height,
        )?;
        let domain = plane.step_uv_domain();
        let over = |x: Ivl, lo: f64, hi: f64| {
            (Ivl::from(lo) - Ivl::from(x.lo))
                .hi
                .max((Ivl::from(x.hi) - Ivl::from(hi)).hi)
                .max(0.0)
        };
        let position = plane.position();
        let along = |over: f64, axis: Vec3, scale: f64| {
            (Ivl::from(over) * norm(boxed(axis)) * Ivl::from(scale.abs())).hi
        };
        let need = along(
            over(u, domain.u_min(), domain.u_max()),
            position.x_axis(),
            scale.x,
        )
        .max(along(
            over(v, domain.v_min(), domain.v_max()),
            position.y_axis(),
            scale.y,
        ));
        if !need.is_finite() {
            return Err(bad(
                "thread endcap extension over its radial disk is not finite",
            ));
        }
        if need > 0.0 {
            cap_needs.push((id, need));
        }
    }
    if let Some(event) = band.first_event(1.0)? {
        let mut magnitude = Ivl::from(0.0);
        for rate in band
            .flanks
            .iter()
            .map(|flank| flank.distance.rate)
            .chain(radii.into_iter().map(|r| r.rate))
        {
            magnitude = Ivl::new(magnitude.lo.max(rate.mig()), magnitude.hi.max(rate.mag()));
        }
        let limit = event * magnitude;
        return Err(crate::error::Failure::new(
            super::OP,
            crate::error::offset::OffsetFailure::SectionOrderLost {
                faces: [first.0, second.0],
                distance: magnitude.mid(),
                limit: [limit.lo, limit.hi],
            },
        )
        .into_error());
    }
    let roots = band.certify()?;
    for (((wall, family), flank), root) in [first, second]
        .into_iter()
        .zip(&families)
        .zip(&band.flanks)
        .zip(roots)
    {
        displacement = displacement.max(flank.distance.rate.mag());
        let ([pa, pb], [w0, w1], _) = wall.2;
        let support = wall.1.profile().natural_interval();
        let old = (Ivl::new(support.min(), support.max()) - Ivl::from(w0))
            / (Ivl::from(w1) - Ivl::from(w0));
        let [r0, r1] = root;
        let required = r0.hull(r1);
        let extra = (Ivl::from(old.lo) - Ivl::from(required.lo))
            .hi
            .max((Ivl::from(required.hi) - Ivl::from(old.hi)).hi)
            .max(0.0);
        reach = reach.max((norm(subtract(boxed(pb), boxed(pa))) * Ivl::from(extra)).hi);
        let source = required.hull(old);
        let point = flank.section.point_enclosure(source)?;
        let height = dot(subtract(point, boxed(origin)), boxed(axis));
        let angles = (caps - height) / rise;
        let [start, end] = family.turns;
        let extra = (Ivl::from(start) - Ivl::from(angles.lo))
            .hi
            .max((Ivl::from(angles.hi) - Ivl::from(end)).hi)
            .max(0.0);
        turns = turns.max(extra);
    }
    for radius in radii {
        displacement = displacement.max(radius.rate.mag());
    }
    // One actual operation displacement beyond the proved meeting keeps it in
    // the support interior, as required by the exact extension contract.
    reach = (Ivl::from(reach) + Ivl::from(displacement)).hi;
    if !reach.is_finite() || reach <= 0.0 {
        return Err(bad("thread support reach is not finite and positive"));
    }
    // The executive uses this common length in both parametric directions.
    // Check the resulting full profile rectangle, not only the active band:
    // no support may cross the axis or acquire an offset fold on continuation.
    for (wall, flank) in [first, second].into_iter().zip(&band.flanks) {
        let (_, [w0, w1], _) = wall.2;
        let support = wall.1.profile().natural_interval();
        let width = Ivl::from(w1) - Ivl::from(w0);
        let speed = (Ivl::from(flank.section.profile_speed_lower_bound()) / width).lo;
        if !speed.is_finite() || speed <= 0.0 {
            return Err(bad("continued thread source interval is singular"));
        }
        let extension = (Ivl::from(reach) / Ivl::from(speed)).hi;
        let from = (Ivl::from(support.min()) - Ivl::from(extension)).lo;
        let to = (Ivl::from(support.max()) + Ivl::from(extension)).hi;
        let source = (Ivl::new(from, to) - Ivl::from(w0)) / width;
        if flank.radius.at(source).lo <= 0.0 {
            return Err(bad("continued thread support reaches the screw axis"));
        }
        let bounds = flank.enclosure(source, Ivl::from(1.0))?;
        if bounds.oriented_area.lo <= 0.0 || bounds.section.radius_rate.mig() <= 0.0 {
            return Err(bad("continued thread support is not proved regular"));
        }
    }
    // A cap's own margin keeps the disk one displacement inside its extension,
    // as the common reach keeps its meetings.
    let caps = cap_needs
        .into_iter()
        .map(|(id, need)| {
            (
                id,
                (Ivl::from(need) + Ivl::from(displacement)).hi.max(reach),
            )
        })
        .collect();
    if !turns.is_finite() {
        return Err(bad("thread support turns are not finite"));
    }
    Ok(Some(ThreadReach { reach, caps, turns }))
}
