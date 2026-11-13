//! Persisted source topology of a qualified two-Form weld.
use crate::{DocError, Document, FeatureId, FeatureKind, Operation, Result};
use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use solidcraft_geom::subdivision::hierarchy::{FreeRimSide, Hierarchy};

const MAX_RIM_SIDES: usize = 256;

/// Stable source identities and one complete, oppositely wound rim pairing.
///
/// The source features remain independently editable. Each side has a logical
/// face and edge in that feature's namespace. Evaluator-depth paths are resolved
/// from the current source, so increasing depth away from the rim is harmless.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, solidcraft_derive::Walk)]
#[serde(deny_unknown_fields)]
pub struct RefinedWeldSeam {
    sources: [FeatureId; 2],
    #[serde(deserialize_with = "read_sides")]
    sides: Vec<[Side; 2]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, solidcraft_derive::Walk)]
#[serde(deny_unknown_fields)]
struct Side {
    edge: [usize; 2],
    owner: usize,
    root: usize,
    domain: [usize; 4],
}

struct ResolvedRims {
    paths: Vec<[FreeRimSide; 2]>,
    sides: Vec<[Side; 2]>,
}

impl RefinedWeldSeam {
    /// Bind the original free rims to their current chart owners. The caller
    /// must still qualify the native curves and sewn body before authoring.
    pub fn capture(document: &Document, sources: [FeatureId; 2], edges: [[usize; 2]; 2]) -> Result<Self> {
        let ResolvedRims { sides, .. } = resolve_rims(document, sources, edges)?;
        Ok(Self { sources, sides })
    }

    /// Match the fitted ends of the only free opening on each Form. The
    /// tolerance here only nominates a cyclic phase. The evaluator proves
    /// continuous coincidence and native sewing before accepting the seam.
    pub fn capture_aligned(document: &Document, sources: [FeatureId; 2], tolerances: [f64; 2]) -> Result<Self> {
        let [first, second] = sources;
        let (a, component) = source(document, first)?;
        let (b, other_component) = source(document, second)?;
        if first == second || component != other_component {
            return Err(invalid("refined Weld requires two different Forms in the same component"));
        }
        let a = a.single_refined_weld_rim().map_err(|error| invalid(error.to_string()))?;
        let b = b.single_refined_weld_rim().map_err(|error| invalid(error.to_string()))?;
        let count = a.len();
        if count != b.len() || !(3..=MAX_RIM_SIDES).contains(&count) {
            return Err(invalid("refined Weld requires equally segmented complete rims"));
        }
        let a_ends = fitted_ends(document, first, &a, tolerances[0])?;
        let b_ends = fitted_ends(document, second, &b, tolerances[1])?;
        use crate::kernel::form_curves::{RimPhase, reversed_rim_phase};
        let phase = match reversed_rim_phase(&a_ends, &b_ends, super::weld_native::SEW_TOLERANCE) {
            RimPhase::Unique(phase) => phase,
            RimPhase::Missing => return Err(invalid("the two refined Form rims are not aligned")),
            RimPhase::Ambiguous => return Err(invalid("the two refined Form rims do not have a unique alignment")),
        };
        Self::capture(document, sources, [a[0].edge, b[phase].edge])
    }

    pub fn sources(&self) -> [FeatureId; 2] {
        self.sources
    }

    /// Recheck both sources and every directed side. Never search for a
    /// substitute side, a new phase, or a nearby chart when one is retired.
    pub fn replay(&self, document: &Document) -> Result<Vec<[FreeRimSide; 2]>> {
        let [first, second] = self.sides.first().ok_or_else(|| invalid("missing saved weld rim"))?;
        let ResolvedRims { paths, sides } = resolve_rims(document, self.sources, [first.edge, second.edge])?;
        if self.sides != sides {
            return Err(invalid("refined Weld no longer has the saved rim ownership"));
        }
        Ok(paths)
    }
}

fn fitted_ends(document: &Document, id: FeatureId, rim: &[FreeRimSide], tolerance: f64) -> Result<Vec<[Vec3; 2]>> {
    let feature = document.feature(id.0).ok_or_else(|| invalid("refined Weld source Form is missing"))?;
    let FeatureKind::Form { definition, .. } = &feature.kind else { return Err(invalid("refined Weld requires retained source Forms")) };
    let edges: Vec<_> = rim.iter().map(|side| side.edge).collect();
    let curves = super::edge_curves(definition, tolerance, &edges)?;
    if curves.len() != edges.len() {
        return Err(invalid("refined Weld lost fitted rim ends"));
    }
    curves
        .iter()
        .zip(edges)
        .map(|(curve, edge)| {
            let mut points = [curve.at(0.0)?, curve.at(1.0)?];
            if curve.edge == [edge[1], edge[0]] {
                points.reverse();
            } else if curve.edge != edge {
                return Err(invalid("refined Weld lost directed fitted rim ends"));
            }
            Ok(points)
        })
        .collect()
}

fn resolve_rims(document: &Document, [first, second]: [FeatureId; 2], [first_edge, second_edge]: [[usize; 2]; 2]) -> Result<ResolvedRims> {
    if first == second {
        return Err(invalid("refined Weld requires two different source Forms"));
    }
    let (a, a_component) = source(document, first)?;
    let (b, b_component) = source(document, second)?;
    if a_component != b_component {
        return Err(invalid("refined Weld source Forms must belong to the same component"));
    }
    let rims = a.refined_weld_rims(b, first_edge, second_edge).map_err(|error| invalid(error.to_string()))?;
    if !(3..=MAX_RIM_SIDES).contains(&rims.len()) {
        return Err(invalid("refined Weld rim exceeds the saved side budget"));
    }
    let sides = rims.iter().map(|[first, second]| Ok([Side::capture(a, first)?, Side::capture(b, second)?])).collect::<Result<_>>()?;
    Ok(ResolvedRims { paths: rims, sides })
}

impl Side {
    fn capture(hierarchy: &Hierarchy, side: &FreeRimSide) -> Result<Self> {
        let leaf = hierarchy.leaves.get(side.owner).filter(|leaf| leaf.active).ok_or_else(|| invalid("refined Weld rim lost its active leaf"))?;
        Ok(Self { edge: side.edge, owner: side.owner, root: leaf.root, domain: leaf.domain })
    }
}

fn source(document: &Document, id: FeatureId) -> Result<(&Hierarchy, u64)> {
    let feature = document.feature(id.0).ok_or_else(|| invalid("refined Weld source Form is missing"))?;
    let FeatureKind::Form { definition, operation: Operation::NewBody, .. } = &feature.kind else {
        return Err(invalid("refined Weld requires standalone Form sources"));
    };
    if definition.matching.is_some() || definition.pipe.is_some() || definition.profile.is_some() || definition.symmetry.is_some() {
        return Err(invalid("refined Weld requires independent Form sources without symmetry"));
    }
    let hierarchy = definition.hierarchy.as_ref().ok_or_else(|| invalid("refined Weld requires retained local refinement"))?;
    if !hierarchy.dissolved_edges.is_empty() {
        return Err(invalid("refined Weld requires retained rectangular leaf owners"));
    }
    Ok((hierarchy, feature.component))
}

fn read_sides<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Vec<[Side; 2]>, D::Error> {
    struct Sides;
    impl<'de> serde::de::Visitor<'de> for Sides {
        type Value = Vec<[Side; 2]>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("three to 256 paired logical rim sides")
        }

        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut sequence: A) -> std::result::Result<Self::Value, A::Error> {
            let mut sides = Vec::new();
            while let Some(pair) = sequence.next_element()? {
                if sides.len() == MAX_RIM_SIDES {
                    return Err(serde::de::Error::custom("refined Weld rim exceeds the saved side budget"));
                }
                sides.push(pair);
            }
            if sides.len() < 3 {
                return Err(serde::de::Error::custom("refined Weld rim is incomplete"));
            }
            Ok(sides)
        }
    }
    d.deserialize_seq(Sides)
}

fn invalid(message: impl Into<String>) -> DocError {
    DocError::Invalid(message.into())
}
