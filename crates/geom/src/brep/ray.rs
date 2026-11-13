//! Bounded native-face ray proofs over portable B-rep records.
//!
//! A hit needs a certified material witness before every possible different
//! identity. Unsupported and unresolved native faces remain possible blockers.
mod face;
use solidcore_ops::ray_tensor as tensor;

use solidcore_record::brep::Brep;
use std::borrow::Borrow;
use tensor::{Interval as I, spend};

pub use face::{FaceError, FaceProof, PreparedFace};
pub use tensor::ProofError;

pub const MAX_FACES: usize = 8_192;
pub const MAX_CONTEXT: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Front<Owner> {
    Miss,
    /// The depth encloses the nearest hit of the proven owner. This can cover
    /// several native patches and does not identify a single native hit point.
    Hit {
        owner: Owner,
        depth: [f64; 2],
    },
    /// The front native material has no qualified logical owner.
    Foreign {
        depth: [f64; 2],
    },
    /// The front material belongs to one of the retained context records.
    Occluded {
        depth: [f64; 2],
    },
    Unresolved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrontError {
    InvalidRay,
    InvalidOwnerMap,
    GeometryBudget,
    Proof(ProofError),
}

impl From<ProofError> for FrontError {
    fn from(error: ProofError) -> Self {
        Self::Proof(error)
    }
}

type Result<T> = std::result::Result<T, FrontError>;

pub use tensor::ClipDepth;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target<Owner> {
    Owned(Owner),
    Foreign,
    Context,
}

struct Face<Owner> {
    record: usize,
    native: usize,
    target: Target<Owner>,
    bounds: Option<[I; 3]>,
}

struct Candidate<'a, Owner> {
    face: &'a Face<Owner>,
    depth: I,
    possible: bool,
}

struct Ray {
    origin: [f64; 3],
    direction: [f64; 3],
    depth: I,
}

impl Ray {
    fn new(origin: [f64; 3], direction: [f64; 3], interval: [f64; 2]) -> Result<Self> {
        if origin.into_iter().chain(direction).chain(interval).any(|v| !v.is_finite())
            || direction.iter().all(|&d| d == 0.0)
            || interval[0] < 0.0
            || interval[0] >= interval[1]
        {
            return Err(FrontError::InvalidRay);
        }
        Ok(Self { origin, direction, depth: I { lo: interval[0], hi: interval[1] } })
    }

    fn band(&self, bounds: [I; 3]) -> Option<I> {
        tensor::ray_bounds(bounds, self.origin, self.direction, self.depth)
    }
}

/// Native identity and conservative world bounds, reusable across rays. Keep
/// each immutable record with its bounds, either borrowed or shared by `Arc`.
pub struct Scene<Record, Owner> {
    records: Vec<Record>,
    faces: Vec<Face<Owner>>,
}

impl<Record: Borrow<Brep> + Clone, Owner: Copy + Eq> Scene<Record, Owner> {
    /// `owners` indexes the edited record's native faces. `None` still blocks
    /// a rear pick. Every opaque context record uses the same world frame.
    pub fn prepare(edited: Record, owners: &[Option<Owner>], context: &[Record], work: &mut usize) -> Result<Self> {
        if edited.borrow().faces.len() != owners.len() {
            return Err(FrontError::InvalidOwnerMap);
        }
        if context.len() > MAX_CONTEXT {
            return Err(FrontError::GeometryBudget);
        }
        let count = context
            .iter()
            .try_fold(edited.borrow().faces.len(), |sum, record| sum.checked_add(record.borrow().faces.len()))
            .ok_or(FrontError::GeometryBudget)?;
        if count > MAX_FACES {
            return Err(FrontError::GeometryBudget);
        }
        spend(work, count)?;
        let mut faces = Vec::new();
        faces.try_reserve_exact(count).map_err(|_| ProofError::Allocation)?;
        append(&mut faces, 0, edited.borrow(), |native| owners[native].map_or(Target::Foreign, Target::Owned), work)?;
        for (index, record) in context.iter().enumerate() {
            append(&mut faces, index + 1, record.borrow(), |_| Target::Context, work)?;
        }
        let mut records = Vec::new();
        records.try_reserve_exact(1 + context.len()).map_err(|_| ProofError::Allocation)?;
        records.push(edited);
        records.extend(context.iter().cloned());
        Ok(Self { records, faces })
    }

    /// Query the forward interval of `origin + t * direction`. An incomplete
    /// search returns an error or an unresolved result, including depth ties.
    pub fn front(&self, origin: [f64; 3], direction: [f64; 3], ray_parameter: [f64; 2], work: &mut usize) -> Result<Front<Owner>> {
        let ray = Ray::new(origin, direction, ray_parameter)?;
        let mut candidates = Vec::new();
        candidates.try_reserve_exact(self.faces.len()).map_err(|_| ProofError::Allocation)?;
        for face in &self.faces {
            spend(work, 8)?;
            let depth = face.bounds.map_or(Some(ray.depth), |bounds| ray.band(bounds));
            if let Some(depth) = depth {
                candidates.push(Candidate { face, depth, possible: true });
            }
        }
        let sort_cost = usize::BITS - candidates.len().leading_zeros();
        spend(work, candidates.len() * (sort_cost as usize + 1))?;
        candidates.sort_unstable_by(|a, b| a.depth.lo.total_cmp(&b.depth.lo));
        choose(&self.records, &mut candidates, &ray, work)
    }
}

fn append<Owner>(faces: &mut Vec<Face<Owner>>, index: usize, record: &Brep, target: impl Fn(usize) -> Target<Owner>, work: &mut usize) -> Result<()> {
    for (native, face) in record.faces.iter().enumerate() {
        let bounds = qualified(face::world_bounds(face, work))?;
        faces.push(Face { record: index, native, target: target(native), bounds });
    }
    Ok(())
}

fn qualified<T>(outcome: std::result::Result<T, FaceError>) -> Result<Option<T>> {
    match outcome {
        Ok(value) => Ok(Some(value)),
        Err(FaceError::Proof(error @ (ProofError::WorkExhausted | ProofError::Allocation | ProofError::InvalidRay))) => Err(error.into()),
        Err(_) => Ok(None),
    }
}

fn choose<Record: Borrow<Brep>, Owner: Copy + Eq>(
    records: &[Record],
    candidates: &mut [Candidate<'_, Owner>],
    ray: &Ray,
    work: &mut usize,
) -> Result<Front<Owner>> {
    let mut best: Option<(Target<Owner>, f64)> = None;
    for candidate in candidates.iter_mut() {
        spend(work, 1)?;
        if best.is_some_and(|(_, upper)| upper < candidate.depth.lo) {
            break;
        }
        let record = records.get(candidate.face.record).ok_or(FrontError::InvalidOwnerMap)?.borrow();
        let Some(face) = qualified(PreparedFace::prepare(record, candidate.face.native, work))? else { continue };
        let proof = qualified(face.prove(ray.origin, ray.direction, ray.depth.pair(), work))?;
        let witness = match proof {
            Some(FaceProof::Miss) => {
                candidate.possible = false;
                continue;
            }
            Some(FaceProof::Unique { depth: [lo, hi], .. }) => candidate.depth.intersect(I { lo, hi }),
            // Multiple proves existence even when no one depth enclosure was
            // returned. The surface's complete ray band remains valid for all.
            Some(FaceProof::Multiple) => Some(candidate.depth),
            Some(FaceProof::Unresolved) | None => None,
        };
        if let Some(depth) = witness {
            candidate.depth = depth;
            if best.is_none_or(|(_, old)| depth.hi < old) {
                best = Some((candidate.face.target, depth.hi));
            }
        }
    }

    spend(work, candidates.len())?;
    let Some((front, upper)) = best else {
        return Ok(if candidates.iter().any(|c| c.possible) { Front::Unresolved } else { Front::Miss });
    };
    let mut lower = upper;
    for candidate in candidates.iter().filter(|c| c.possible) {
        if candidate.face.target == front {
            lower = lower.min(candidate.depth.lo);
        } else if candidate.depth.lo <= upper {
            // Different native ownership or context can tie or win. Do not
            // resolve overlapping intervals by native order or owner number.
            return Ok(Front::Unresolved);
        }
    }
    let depth = [lower, upper];
    Ok(match front {
        Target::Owned(owner) => Front::Hit { owner, depth },
        Target::Foreign => Front::Foreign { depth },
        Target::Context => Front::Occluded { depth },
    })
}
