//! Plan the two passes of a selection made entirely of separate Ends.

use super::{independent_separate_ends, SectionKind, SeparateEndFit, SeparateEndPair};
use crate::error::blend::BlendFailure;
use crate::error::topology::TopologyFailure;
use crate::error::{EntityRef, Failure, Result, SolidError};
use crate::geom::attribute::{ids, Attribute, AttributeHandlers, Behavior};
use crate::kernel::Kernel;
use crate::topology::{edge, Brep, EdgeId, VertexId};

const OP: &str = "blend::blend_corner::separate_ends::plan";

/// How to handle the whole edge selection after checking its separate Ends.
pub enum SeparateEndPlan {
    /// Use the ordinary joint corner construction.
    NotApplicable,
    /// Build the two groups with the ordinary single-Edge End construction.
    Ends(SeparateEndGroups),
    /// The common size exceeds the proved room of the selection.
    Limited {
        /// The largest common size the supports and clear corridors admit.
        size: f64,
        /// The selected edge whose supports limit the size.
        edge: EdgeId,
    },
}

fn reserve<T>(vec: &mut Vec<T>, count: usize) -> Result<()> {
    vec.try_reserve_exact(count)
        .map_err(|_| SolidError::topology(OP, TopologyFailure::AllocationFailed))
}

/// Group the selected edges into pairs of separate Ends and check their fit.
///
/// Each pick must meet another pick at exactly one end, and each shared end
/// must have precisely two picks. Build first the edge that occurs earlier
/// in the body, so reversing the selection does not change either pass.
///
/// # Errors
/// Propagates cancellation, allocation and B-rep errors. A proved obstruction
/// refuses the construction at its vertex without a size.
pub fn plan(
    k: &Kernel,
    b: &mut Brep,
    picked: &[EdgeId],
    sizes: &[f64],
    kind: SectionKind,
) -> Result<SeparateEndPlan> {
    k.check_cancelled(OP)?;
    let count = picked.len();
    if count < 2 || count % 2 != 0 || (sizes.len() != 1 && sizes.len() != count) {
        return Ok(SeparateEndPlan::NotApplicable);
    }

    let endpoint_count = count
        .checked_mul(2)
        .ok_or_else(|| SolidError::topology(OP, TopologyFailure::AllocationFailed))?;
    let mut endpoints = Vec::new();
    reserve(&mut endpoints, endpoint_count)?;
    for (i, &edge) in picked.iter().enumerate() {
        k.check_cancelled(OP)?;
        let (start, end) = edge::start_end_vertices(b, edge)?;
        endpoints.extend(
            start
                .into_iter()
                .chain(end.filter(|&v| Some(v) != start))
                .map(|v| (v, i)),
        );
    }
    endpoints.sort_unstable_by_key(|&(v, _)| v);

    let mut paired = Vec::new();
    reserve(&mut paired, count)?;
    paired.resize(count, false);
    let mut pairs = Vec::new();
    let mut trailing = Vec::new();
    reserve(&mut pairs, count / 2)?;
    reserve(&mut trailing, count / 2)?;
    for &v in b.vertex_list() {
        k.check_cancelled(OP)?;
        let start = endpoints.partition_point(|&(at, _)| at < v);
        let end = endpoints.partition_point(|&(at, _)| at <= v);
        let Some([(_, i), (_, j)]) = endpoints.get(start..end) else {
            if end - start > 2 {
                return Ok(SeparateEndPlan::NotApplicable);
            }
            continue;
        };
        let (Some(&a), Some(&z), Some(&a_size), Some(&z_size)) = (
            picked.get(*i),
            picked.get(*j),
            sizes.get(*i).or_else(|| sizes.first()),
            sizes.get(*j).or_else(|| sizes.first()),
        ) else {
            return Ok(SeparateEndPlan::NotApplicable);
        };
        if a == z || paired.get(*i) != Some(&false) || paired.get(*j) != Some(&false) {
            return Ok(SeparateEndPlan::NotApplicable);
        }
        if let Some(used) = paired.get_mut(*i) {
            *used = true;
        }
        if let Some(used) = paired.get_mut(*j) {
            *used = true;
        }
        let a_order = b.edge_list().iter().position(|&edge| edge == a);
        let z_order = b.edge_list().iter().position(|&edge| edge == z);
        let (Some(a_order), Some(z_order)) = (a_order, z_order) else {
            return Ok(SeparateEndPlan::NotApplicable);
        };
        let (edges, sizes, last) = if a_order < z_order {
            ([a, z], [a_size, z_size], *j)
        } else {
            ([z, a], [z_size, a_size], *i)
        };
        pairs.push(SeparateEndPair {
            vertex: v,
            edges,
            sizes,
        });
        trailing.push((last, v));
    }
    if !paired.into_iter().all(|used| used) {
        return Ok(SeparateEndPlan::NotApplicable);
    }

    match independent_separate_ends(k, b, &pairs, kind)? {
        SeparateEndFit::NotApplicable => Ok(SeparateEndPlan::NotApplicable),
        SeparateEndFit::WithinSupports | SeparateEndFit::Fits => {
            trailing.sort_unstable_by_key(|&(index, _)| index);
            Ok(SeparateEndPlan::Ends(SeparateEndGroups { trailing }))
        }
        SeparateEndFit::Limited { size, edge } => Ok(SeparateEndPlan::Limited { size, edge }),
        SeparateEndFit::Obstructed { vertex } => Err(Failure::new(OP, BlendFailure::CornerRefused)
            .at(EntityRef::Vertex(vertex))
            .into_error()),
    }
}

/// The kernel's two groups of independent Ends.
pub struct SeparateEndGroups {
    trailing: Vec<(usize, VertexId)>,
}

// A temporary kernel attribute below the applications' range and the ops overflow mark.
const TRAILING_ID: u64 = ids::USER_1 - 2;

#[derive(Debug, Clone, PartialEq)]
struct Trailing(usize);

impl AttributeHandlers for Trailing {}

impl SeparateEndGroups {
    /// Run the two groups with the caller's ordinary single-group End construction.
    ///
    /// The first pass can replace or split an unprocessed edge. Each tagged edge
    /// must survive as exactly one edge for the second pass. The caller supplies
    /// its normal transaction so a refusal or cancellation undoes both groups.
    ///
    /// # Errors
    /// A cancelled operation, allocation or topology failure, or a refusal of
    /// either group. No fit size is given when the first group changes a pick.
    pub fn build<E>(
        self,
        k: &mut Kernel,
        b: &mut Brep,
        edges: &[EdgeId],
        sizes: &[f64],
        mut pass: impl FnMut(
            &mut Kernel,
            &mut Brep,
            &[EdgeId],
            &[f64],
        ) -> Result<std::result::Result<(), E>>,
    ) -> Result<std::result::Result<(), E>> {
        let mut group_edges = Vec::new();
        let mut group_sizes = Vec::new();
        let capacity = edges.len() / 2;
        reserve(&mut group_edges, capacity)?;
        reserve(&mut group_sizes, capacity)?;
        let mut trailing = self.trailing.iter().enumerate().peekable();
        for (i, &e) in edges.iter().enumerate() {
            k.check_cancelled(OP)?;
            if let Some(&(index, _)) = trailing
                .next_if(|(_, (index, _))| *index == i)
                .map(|(_, pick)| pick)
            {
                b.add_attribute(
                    EntityRef::Edge(e),
                    Attribute::application(TRAILING_ID, Behavior::Copy, Trailing(index)),
                )?;
            } else if let Some(&size) = sizes.get(i).or_else(|| sizes.first()) {
                group_edges.push(e);
                group_sizes.push(size);
            }
        }
        if let Err(refusal) = pass(k, b, &group_edges, &group_sizes)? {
            return Ok(Err(refusal));
        }

        let mut surviving = Vec::new();
        reserve(&mut surviving, self.trailing.len())?;
        surviving.resize(self.trailing.len(), None);
        for &e in b.edge_list() {
            k.check_cancelled(OP)?;
            let Some((handle, pick)) = b
                .entity_attributes(EntityRef::Edge(e))
                .into_iter()
                .filter(|(_, a)| a.attribute_id() == TRAILING_ID)
                .find_map(|(h, a)| a.application_value::<Trailing>().map(|p| (h, p.0)))
            else {
                continue;
            };
            let Ok(position) = self
                .trailing
                .binary_search_by_key(&pick, |&(index, _)| index)
            else {
                continue;
            };
            if let Some(slot) = surviving.get_mut(position) {
                if slot.replace((e, handle)).is_some() {
                    return Err(corner_refused(self.trailing.get(position).map(|&(_, v)| v)));
                }
            }
        }

        group_edges.clear();
        group_sizes.clear();
        for ((index, vertex), found) in self.trailing.into_iter().zip(surviving) {
            let Some((e, handle)) = found else {
                return Err(corner_refused(Some(vertex)));
            };
            if let Some(&size) = sizes.get(index).or_else(|| sizes.first()) {
                b.remove_attribute(EntityRef::Edge(e), handle)?;
                group_edges.push(e);
                group_sizes.push(size);
            }
        }
        k.check_cancelled(OP)?;
        pass(k, b, &group_edges, &group_sizes)
    }
}

fn corner_refused(vertex: Option<VertexId>) -> SolidError {
    let failure = Failure::new(OP, BlendFailure::CornerRefused);
    match vertex {
        Some(vertex) => failure.at(EntityRef::Vertex(vertex)).into_error(),
        None => failure.into_error(),
    }
}
