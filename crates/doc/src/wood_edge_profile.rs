//! Woodworking feature contracts shared by evaluation and commands.

use crate::{DocError, ModelState, Result};
use solidcore_record::Vec3;
use solidcraft_kernel::{CornerProfile, RouterEdgePick};

/// Find the owner and edge on exact B-rep curves, refusing an ambiguous body.
pub fn pick(st: &ModelState, only: Option<&str>, point: Vec3) -> Result<(usize, RouterEdgePick)> {
    let mut best: Option<(usize, RouterEdgePick)> = None;
    let mut ambiguous = false;
    for (i, body) in st.bodies.iter().enumerate().filter(|(_, b)| !b.body.is_mesh() && only.is_none_or(|n| b.name == n)) {
        let hit = match solidcraft_kernel::routed_edge_picks(&body.body, &[point], false)? {
            Some(picks) => picks.into_iter().next().ok_or_else(|| DocError::Invalid("router edge missing".into()))?,
            None => continue,
        };
        let tie = (body.body.size() * 1e-6).max(1e-6);
        match best {
            Some((_, old)) if (hit.distance - old.distance).abs() <= tie => ambiguous = true,
            Some((_, old)) if hit.distance >= old.distance => {}
            _ => {
                best = Some((i, hit));
                ambiguous = false;
            }
        }
    }
    if ambiguous {
        return Err(DocError::Invalid("the router pick is on more than one body; name its body".into()));
    }
    best.ok_or_else(|| DocError::Invalid("no B-rep edge near the router pick".into()))
}

/// Resolve one body's picks and optional tangent continuation without a display mesh.
pub fn resolve(st: &ModelState, only: Option<&str>, points: &[Vec3], chain: bool) -> Result<(usize, Vec<RouterEdgePick>)> {
    let mut owner = None;
    for &point in points {
        let (body, _) = pick(st, only, point)?;
        if owner.is_some_and(|i| i != body) {
            return Err(DocError::Invalid("router edges must belong to one body".into()));
        }
        owner = Some(body);
    }
    let owner = owner.ok_or_else(|| DocError::Invalid("router edges are required".into()))?;
    let body = st.bodies.get(owner).ok_or_else(|| DocError::Invalid("router body missing".into()))?;
    let picks = solidcraft_kernel::routed_edge_picks(&body.body, points, chain)?
        .ok_or_else(|| DocError::Invalid("no B-rep edge near the router pick".into()))?;
    Ok((owner, picks))
}

/// Parse the saved router profile. None keeps the feature as an identity operation.
pub fn profile(name: &str) -> Result<Option<CornerProfile>> {
    Ok(Some(match name {
        "none" => return Ok(None),
        "chamfer" => CornerProfile::Chamfer,
        "bevel" => CornerProfile::Bevel,
        "roundover" => CornerProfile::Roundover,
        "cove" => CornerProfile::Cove,
        "rabbet" => CornerProfile::Rabbet,
        "ogee" => CornerProfile::Ogee,
        "bead" => CornerProfile::Bead,
        _ => return Err(DocError::Invalid(format!("unknown router profile `{name}`"))),
    }))
}
