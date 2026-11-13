//! Stable mirror partners are recorded when symmetry is enabled and survive sculpting.
use super::{Cage, Result, bad, key as edge_key};
use crate::prelude::*;
use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Symmetry {
    pub axis: usize,
    pub origin: f64,
    pub partners: Vec<usize>,
}
impl Symmetry {
    pub fn new(cage: &Cage, axis: usize) -> Result<Self> {
        Self::at(cage, axis, 0.0)
    }
    pub fn at(cage: &Cage, axis: usize, origin: f64) -> Result<Self> {
        cage.validate()?;
        if axis > 2 || !origin.is_finite() {
            return Err(bad("invalid symmetry plane"));
        }
        let key = |p: Vec3| ((p.x * 1e6).round() as i64, (p.y * 1e6).round() as i64, (p.z * 1e6).round() as i64);
        let mut lookup = BTreeMap::new();
        for (i, &p) in cage.points.iter().enumerate() {
            if lookup.insert(key(p), i).is_some() {
                return Err(bad("symmetry cannot distinguish coincident cage vertices"));
            }
        }
        let mut out = Self { axis, origin, partners: Vec::with_capacity(cage.points.len()) };
        for &p in &cage.points {
            let q = out.reflect(p);
            let i = *lookup.get(&key(q)).ok_or_else(|| bad("the current cage is not symmetric about this origin plane"))?;
            if cage.point(i)?.dist(q) > 2e-6 {
                return Err(bad("no matching symmetry partner"));
            }
            out.partners.push(i);
        }
        out.validate(cage)?;
        Ok(out)
    }
    pub fn reflect(&self, p: Vec3) -> Vec3 {
        match self.axis {
            0 => Vec3::new(2.0 * self.origin - p.x, p.y, p.z),
            1 => Vec3::new(p.x, 2.0 * self.origin - p.y, p.z),
            _ => Vec3::new(p.x, p.y, 2.0 * self.origin - p.z),
        }
    }
    pub fn reflect_vector(&self, v: Vec3) -> Vec3 {
        match self.axis {
            0 => Vec3::new(-v.x, v.y, v.z),
            1 => Vec3::new(v.x, -v.y, v.z),
            _ => Vec3::new(v.x, v.y, -v.z),
        }
    }
    /// Complete a logical face selection under the recorded vertex correspondence.
    pub fn mirror_face_selection(&self, faces: &[Vec<usize>], selected: &[usize]) -> Result<Vec<usize>> {
        let lookup: BTreeMap<BTreeSet<usize>, usize> =
            faces.iter().enumerate().filter(|(_, f)| !f.is_empty()).map(|(id, f)| (f.iter().copied().collect(), id)).collect();
        let mut out: BTreeSet<_> = selected.iter().copied().collect();
        for &id in selected {
            let face = faces.get(id).filter(|f| !f.is_empty()).ok_or_else(|| bad("select an existing logical face"))?;
            let reflected =
                face.iter().map(|&v| self.partners.get(v).copied().ok_or_else(|| bad("invalid mirrored corner"))).collect::<Result<BTreeSet<_>>>()?;
            out.insert(*lookup.get(&reflected).ok_or_else(|| bad("logical face has no matching mirrored region"))?);
        }
        Ok(out.into_iter().collect())
    }
    pub fn validate(&self, cage: &Cage) -> Result<()> {
        if self.axis > 2 || !self.origin.is_finite() || self.partners.len() != cage.points.len() {
            return Err(bad("invalid saved symmetry"));
        }
        for (i, &j) in self.partners.iter().enumerate() {
            if self.partners.get(j) != Some(&i) {
                return Err(bad("invalid saved mirror partner"));
            }
        }
        Ok(())
    }
    /// Check that subdivision masks and positions are invariant under reflection.
    pub fn validate_reflection(&self, cage: &Cage) -> Result<()> {
        self.validate(cage)?;
        let partner = |vertex: usize| self.partners.get(vertex).copied().ok_or_else(|| bad("invalid mirror partner"));
        for (i, &j) in self.partners.iter().enumerate() {
            if self.reflect(cage.point(i)?).dist(cage.point(j)?) > 2e-6 {
                return Err(bad("control positions must be mirrored"));
            }
        }
        let faces = cage
            .faces
            .iter()
            .map(|face| {
                let mut ids = face.clone();
                ids.sort_unstable();
                ids
            })
            .collect::<BTreeSet<_>>();
        for face in &cage.faces {
            let mut reflected = face.iter().map(|&v| partner(v)).collect::<Result<Vec<_>>>()?;
            reflected.sort_unstable();
            if !faces.contains(&reflected) {
                return Err(bad("cage topology must be mirrored"));
            }
        }
        let vertices: BTreeMap<_, _> = cage.vertex_creases.iter().map(|c| (c.vertex, c.sharpness)).collect();
        let edges: BTreeMap<_, _> = cage.edge_creases.iter().map(|c| (edge_key(c.edge[0], c.edge[1]), c.sharpness)).collect();
        for (&i, &sharpness) in &vertices {
            if vertices.get(&partner(i)?).copied().unwrap_or(0.0) != sharpness {
                return Err(bad("vertex creases must be mirrored"));
            }
        }
        for (&(a, b), &sharpness) in &edges {
            if edges.get(&edge_key(partner(a)?, partner(b)?)).copied().unwrap_or(0.0) != sharpness {
                return Err(bad("edge creases must be mirrored"));
            }
        }
        Ok(())
    }
    pub fn constrain(&self, cage: &Cage, targets: &mut BTreeMap<usize, Vec3>) -> Result<()> {
        self.validate(cage)?;
        let given = targets.clone();
        for (&i, &p) in &given {
            let j = *self.partners.get(i).ok_or_else(|| bad("invalid symmetry vertex"))?;
            let q = given.get(&j).map_or(p, |&q| (p + self.reflect(q)) / 2.0);
            targets.insert(i, q);
            targets.insert(j, self.reflect(q));
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partners_are_stable_and_center_vertices_stay_on_plane() {
        let c = super::super::primitives::Parameters::default().build().unwrap();
        let s = Symmetry::new(&c, 0).unwrap();
        let i = c.points.iter().position(|p| p.x > 1.0).unwrap();
        let j = s.partners[i];
        let mut values = BTreeMap::from([(i, c.points[i] + Vec3::new(2.0, 3.0, 4.0))]);
        s.constrain(&c, &mut values).unwrap();
        assert_eq!(values[&j], s.reflect(values[&i]));
        let i = c.points.iter().position(|p| p.x == 0.0).unwrap();
        let mut values = BTreeMap::from([(i, Vec3::new(9.0, 2.0, 3.0))]);
        s.constrain(&c, &mut values).unwrap();
        assert_eq!(values[&i].x, 0.0);
    }
}
