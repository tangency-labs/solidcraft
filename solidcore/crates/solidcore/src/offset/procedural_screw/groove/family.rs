//! Compatible retained affine screw supports and their connected turn covers.
//!
//! Zero radial coefficients and no radial law define the same motion. All other
//! coefficients, signed offsets and charts are compared literally. A shared
//! station is an exact chart endpoint, not a snapped trim coordinate.
//! Deviations: new construction; iteration is bounded by the input members.

use super::{bad, Result, Surface, Vec3};
use crate::surface::procedural_sweep::{ProceduralSweep, ScrewMotion};
use crate::topology::{Brep, FaceId};

type Source = ([Vec3; 2], [f64; 2], f64);
pub(super) type Member<'a> = (FaceId, &'a ProceduralSweep, Source, Source);

pub(super) struct Family<'a> {
    pub members: Vec<Member<'a>>,
    pub turns: [f64; 2],
}

impl<'a> Family<'a> {
    pub fn first(&self) -> Result<&Member<'a>> {
        self.members
            .first()
            .ok_or_else(|| bad("empty thread family"))
    }

    fn cover(&mut self) -> Result<()> {
        self.members.sort_by(|a, b| {
            a.1.stations()
                .first()
                .copied()
                .unwrap_or(f64::NAN)
                .total_cmp(&b.1.stations().first().copied().unwrap_or(f64::NAN))
                .then(a.0.cmp(&b.0))
        });
        let mut previous: Option<&ProceduralSweep> = None;
        for (_, member, _, _) in &self.members {
            let (Some(&start), Some(&end)) = (member.stations().first(), member.stations().last())
            else {
                return Err(bad("thread member has no turn chart"));
            };
            if !start.is_finite() || !end.is_finite() || start >= end {
                return Err(bad("thread member has an invalid turn chart"));
            }
            if let Some(prior) = previous {
                if start > self.turns[1] {
                    return Err(bad("thread family has a gap in its turn cover"));
                }
                if start < self.turns[1] && !prior.same_screw_parameterization(member) {
                    return Err(bad("overlapping thread members have different charts"));
                }
                self.turns[1] = self.turns[1].max(end);
            } else {
                self.turns = [start, end];
            }
            if previous.is_none() || end >= self.turns[1] {
                previous = Some(member);
            }
        }
        Ok(())
    }
}

pub(super) fn same_motion(a: Option<&ScrewMotion>, b: Option<&ScrewMotion>) -> bool {
    let (Some(a), Some(b)) = (a, b) else {
        return false;
    };
    crate::offset::procedural_screw::same_motion(a, b)
}

pub(super) fn collect<'a>(
    body: &Brep,
    faces: &[FaceId],
    replacements: &'a [Surface],
) -> Result<Option<Vec<Family<'a>>>> {
    let mut families: Vec<Family<'a>> = Vec::new();
    for (&id, new) in faces.iter().zip(replacements) {
        let Some(sweep) = new.procedural_sweep() else {
            continue;
        };
        let Some(old) = body
            .face(id)?
            .surface
            .as_ref()
            .and_then(Surface::procedural_sweep)
        else {
            return Ok(None);
        };
        let old = old.definition();
        let new = sweep.definition();
        let (Some(source), Some(target)) =
            (old.retained_affine_source()?, new.retained_affine_source()?)
        else {
            return Ok(None);
        };
        if source.0 != target.0 || source.1 != target.1 || !same_motion(old.screw(), new.screw()) {
            return Ok(None);
        }
        if old.definition_map() != new.definition_map() || old.stations() != new.stations() {
            return Err(bad("thread replacement changes its retained chart"));
        }
        let (a, b, _) = new.definition_map();
        if a.into_iter().chain(b).any(|v| !v.is_finite()) || b.contains(&0.0) {
            return Err(bad("thread member has a singular parameter map"));
        }
        let mut group = None;
        for (i, family) in families.iter().enumerate() {
            let first = family.first()?;
            if first.2 == source && first.3 == target {
                if !same_motion(first.1.screw(), new.screw())
                    || first.1.definition_map() != new.definition_map()
                    || first.1.profile().natural_interval() != new.profile().natural_interval()
                {
                    return Err(bad("thread family has incompatible source charts"));
                }
                group = Some(i);
                break;
            }
        }
        let member = (id, new, source, target);
        if let Some(i) = group {
            families
                .get_mut(i)
                .ok_or_else(|| bad("missing thread family"))?
                .members
                .push(member);
        } else {
            if families.len() == 2 {
                return Ok(None);
            }
            families.push(Family {
                members: vec![member],
                turns: [0.0, 0.0],
            });
        }
    }
    for family in &mut families {
        family.cover()?;
    }
    Ok(Some(families))
}
