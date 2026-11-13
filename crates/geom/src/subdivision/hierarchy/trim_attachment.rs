//! Resolve a logical trim without modifying either side of its subdivision stencil.
use super::*;
use crate::bspline::NurbsSurface;

mod ribbon;

const MAX_ATTACHMENTS: usize = 256;
type LogicalSide = (usize, [usize; 2]);

/// Oriented boundary of an active leaf next to a hidden evaluator face.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrimAttachment {
    /// Directed in the active region's winding.
    pub edge: [usize; 2],
    pub owner: usize,
    pub root: usize,
    /// The same directed edge in the active fitted root's parameter space.
    pub uv: [[f64; 2]; 2],
}

impl TrimAttachment {
    /// The base of this wall is the exact fitted trim, with opposite edge winding.
    /// The caller supplies the same fitted root that is used for the active skin.
    pub fn ruled_patch(&self, skin: &NurbsSurface, delta: Vec3) -> Result<NurbsSurface> {
        ribbon::ruled_patch(skin, self.uv, delta)
    }
}

fn sides(layout: &Layout, keys: &BTreeSet<(usize, usize)>) -> Result<BTreeMap<(usize, usize), LogicalSide>> {
    let mut sides = BTreeMap::new();
    for (owner, face) in layout.faces.iter().enumerate() {
        for (&a, &b) in face.iter().zip(face.iter().cycle().skip(1)).take(face.len()) {
            let k = key(a, b);
            if keys.contains(&k) && sides.insert(k, (owner, [a, b])).is_some() {
                return Err(bad("select a boundary of the visible Form"));
            }
        }
    }
    Ok(sides)
}

impl Hierarchy {
    /// Qualify chart attachments whose active side is adjacent to a hidden
    /// subdivision cell. Genuine free cage edges use ordinary edge extrusion.
    pub fn trim_attachments(&self, selected: &[[usize; 2]]) -> Result<Vec<TrimAttachment>> {
        if selected.is_empty() || selected.len() > MAX_ATTACHMENTS {
            return Err(bad("select 1..256 distinct trim edges"));
        }
        if !self.extrusions.is_empty() {
            return Err(bad("trim attachment requires the original fitted root charts"));
        }
        if self.has_local_sharpness() {
            return Err(bad("trim attachment requires a source without local crease overrides"));
        }
        let keys: BTreeSet<_> = selected.iter().map(|&[a, b]| key(a, b)).collect();
        if keys.len() != selected.len() {
            return Err(bad("select distinct trim edges"));
        }
        let raw = self.raw_layout()?;
        let dissolved = if self.dissolved_edges.is_empty() { None } else { Some(self.layout()?) };
        let logical = dissolved.as_ref().unwrap_or(&raw);
        let visible = sides(logical, &keys)?;
        let source = sides(&raw, &keys)?;
        let paths: BTreeMap<_, _> = logical.paths.iter().map(|([a, b], p)| (key(*a, *b), p)).collect();
        let mut incidence = BTreeMap::<_, [usize; 2]>::new();
        for k in &keys {
            let path = paths.get(k).filter(|path| path.len() >= 2).ok_or_else(|| bad("missing trim edge path"))?;
            for pair in path.windows(2) {
                incidence.insert(key(pair[0], pair[1]), [0, 0]);
            }
        }
        let cage = self.evaluate()?;
        if cage.faces.len() != raw.owners.len() {
            return Err(bad("trim ownership disagrees with the evaluator"));
        }
        for (face, owner) in cage.faces.iter().zip(&raw.owners) {
            for (&a, &b) in face.iter().zip(face.iter().cycle().skip(1)).take(face.len()) {
                if let Some(counts) = incidence.get_mut(&key(a, b)) {
                    counts[usize::from(owner.is_some())] += 1;
                }
            }
        }
        if incidence.values().any(|&uses| uses != [1, 1]) {
            return Err(bad("trim attachment needs one visible cell and one hidden stencil cell per segment"));
        }

        let (locations, _, _) = self.locations()?;
        let uv: BTreeMap<_, _> =
            locations.into_iter().map(|((root, u, v), vertex)| ((root, vertex), [u as f64 / GRID as f64, v as f64 / GRID as f64])).collect();
        let parameter = |root, vertex| uv.get(&(root, vertex)).copied().ok_or_else(|| bad("missing trim boundary parameter"));
        selected
            .iter()
            .map(|&[a, b]| {
                let k = key(a, b);
                let &(leaf, _) = source.get(&k).ok_or_else(|| bad("missing active trim leaf"))?;
                let &(owner, edge) = visible.get(&k).ok_or_else(|| bad("missing visible trim owner"))?;
                let root = self.leaves.get(leaf).ok_or_else(|| bad("missing trim root chart"))?.root;
                Ok(TrimAttachment { edge, owner, root, uv: [parameter(root, edge[0])?, parameter(root, edge[1])?] })
            })
            .collect()
    }
}
