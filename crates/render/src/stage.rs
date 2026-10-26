//! Original analytic passive staging, separate from engineering geometry. Panels
//! are finite, two-sided, non-emissive diffuse rectangles in world millimetres.
//! They are appended only to a captured render scene, after conservative clearance.
use crate::{
    ProductMesh, ProductScene,
    job::{CancelToken, ErrorCode, JobError, Result},
    surface::OpaqueSurface,
};
use serde::{Deserialize, Serialize};
use solidcraft_geom::{Aabb3, Mesh, Vec3};
use std::{collections::BTreeSet, fmt, sync::Arc};

pub const MAX_STAGE_PANELS: usize = 32;
fn invalid(message: impl Into<String>) -> JobError {
    JobError::new(ErrorCode::InvalidRequest, message)
}
fn v(a: [f64; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StagePanel {
    pub id: String,
    pub center_mm: [f64; 3],
    pub half_u_mm: [f64; 3],
    pub half_v_mm: [f64; 3],
    pub reflectance_linear: [f64; 3],
}
impl StagePanel {
    pub fn validate(&self) -> Result<()> {
        let (u, vv) = (v(self.half_u_mm), v(self.half_v_mm));
        let (ul, vl) = (u.len(), vv.len());
        if self.id.is_empty()
            || self.id.len() > 64
            || !self.id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
            || !v(self.center_mm).is_finite()
            || !u.is_finite()
            || !vv.is_finite()
            || !(1e-4..=1e6).contains(&ul)
            || !(1e-4..=1e6).contains(&vl)
            || u.dot(vv).abs() > 1e-8 * ul * vl
            || self.reflectance_linear.iter().any(|x| !x.is_finite() || !(0.0..=1.0).contains(x))
        {
            return Err(invalid("stage panel requires bounded identity, finite orthogonal half-edges and linear reflectance 0..1"));
        }
        for p in self.corners() {
            if !p.is_finite() || [p.x, p.y, p.z].iter().any(|x| x.abs() > 1e9) {
                return Err(invalid("stage panel corner exceeds world bounds"));
            }
        }
        Ok(())
    }
    pub fn corners(&self) -> [Vec3; 4] {
        let (c, u, vv) = (v(self.center_mm), v(self.half_u_mm), v(self.half_v_mm));
        [c - u - vv, c + u - vv, c + u + vv, c - u + vv]
    }
    /// Rectangle/AABB separating axes, not a B-rep contact or collision oracle.
    /// Touching or ambiguous clearance refuses; an AABB false positive is explicit.
    pub fn separated_from(&self, bounds: Aabb3) -> Result<bool> {
        self.validate()?;
        if bounds.is_empty() {
            return Ok(true);
        }
        if !bounds.min.is_finite() || !bounds.max.is_finite() {
            return Err(invalid("nonfinite stage subject bounds"));
        }
        let (u, vv) = (v(self.half_u_mm), v(self.half_v_mm));
        let delta = v(self.center_mm) - bounds.center();
        let half = (bounds.max - bounds.min) * 0.5;
        let axes = [
            Vec3::X,
            Vec3::Y,
            Vec3::Z,
            u.cross(vv),
            u.cross(Vec3::X),
            u.cross(Vec3::Y),
            u.cross(Vec3::Z),
            vv.cross(Vec3::X),
            vv.cross(Vec3::Y),
            vv.cross(Vec3::Z),
        ];
        for axis in axes {
            let length = axis.len();
            if length <= 1e-20 {
                continue;
            }
            let box_radius = axis.x.abs() * half.x + axis.y.abs() * half.y + axis.z.abs() * half.z;
            let panel_radius = axis.dot(u).abs() + axis.dot(vv).abs();
            if delta.dot(axis).abs() > box_radius + panel_radius + 1e-7 * length {
                return Ok(true);
            }
        }
        Ok(false)
    }
    fn mesh(&self) -> Result<ProductMesh> {
        self.validate()?;
        let u = v(self.half_u_mm);
        let vv = v(self.half_v_mm);
        let normal = u.cross(vv).normalized().ok_or_else(|| invalid("degenerate stage panel"))?;
        let surface = OpaqueSurface::new(v(self.reflectance_linear), 0.0, 1.0, 1.0).map_err(|e| invalid(e.to_string()))?;
        let mut mesh = Mesh::default();
        mesh.positions.try_reserve_exact(4).map_err(|_| invalid("could not reserve stage vertices"))?;
        mesh.normals.try_reserve_exact(4).map_err(|_| invalid("could not reserve stage normals"))?;
        mesh.triangles.try_reserve_exact(2).map_err(|_| invalid("could not reserve stage triangles"))?;
        mesh.tri_face.try_reserve_exact(2).map_err(|_| invalid("could not reserve stage face tags"))?;
        mesh.positions.extend(self.corners());
        mesh.normals.extend([normal; 4]);
        mesh.triangles.extend([[0, 1, 2], [0, 2, 3]]);
        mesh.tri_face.extend([0, 0]);
        // The surface carries exact linear reflectance. This display colour is not
        // used as an additional multiplier by PBR consumers.
        let color = self.reflectance_linear.map(solidcraft_geom::srgb::encode8);
        Ok(ProductMesh {
            name: format!("__render_stage__/{}", self.id),
            mesh: Arc::new(mesh),
            color,
            opacity: 1.0,
            surface: Some(surface),
            transmission: None,
            faces: Vec::new(),
        })
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Stage(Vec<StagePanel>);
impl Stage {
    pub fn new(panels: Vec<StagePanel>) -> Result<Self> {
        let stage = Self(panels);
        stage.validate()?;
        Ok(stage)
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn iter(&self) -> impl Iterator<Item = &StagePanel> {
        self.0.iter()
    }
    pub fn validate(&self) -> Result<()> {
        if self.len() > MAX_STAGE_PANELS || self.0.capacity() > MAX_STAGE_PANELS {
            return Err(invalid("at most 32 stage panels and reserved slots"));
        }
        let mut seen = BTreeSet::new();
        for panel in &self.0 {
            panel.validate()?;
            if !seen.insert(&panel.id) {
                return Err(invalid("duplicate stage panel id"));
            }
        }
        Ok(())
    }
    /// Atomic with respect to the captured scene. Product ordinals are preserved.
    pub fn append_to(&self, scene: &mut ProductScene, cancel: &CancelToken) -> Result<()> {
        self.validate()?;
        cancel.check()?;
        if self.is_empty() {
            return Ok(());
        }
        if scene.meshes.len() > crate::job::MAX_CPU_BODIES {
            return Err(invalid("stage subject exceeds the body admission ceiling"));
        }
        // Mesh bounds walk vertices: compute them once, not once for every card.
        let mut bounds = Vec::new();
        bounds.try_reserve_exact(scene.meshes.len()).map_err(|_| invalid("could not reserve stage clearance bounds"))?;
        for body in &scene.meshes {
            cancel.check()?;
            bounds.push(body.mesh.bounds());
        }
        let mut addition = Vec::new();
        addition.try_reserve_exact(self.len()).map_err(|_| invalid("could not reserve passive stage"))?;
        for panel in &self.0 {
            cancel.check()?;
            let name = format!("__render_stage__/{}", panel.id);
            for (body, bounds) in scene.meshes.iter().zip(&bounds) {
                cancel.check()?;
                if body.name == name {
                    return Err(invalid("stage panel identity conflicts with a product body"));
                }
                if !panel.separated_from(*bounds)? {
                    return Err(invalid(format!(
                        "stage panel {} intersects or touches product bounds for {}; move it explicitly",
                        panel.id, body.name
                    )));
                }
            }
            addition.push(panel.mesh()?);
        }
        scene.meshes.try_reserve_exact(addition.len()).map_err(|_| invalid("could not reserve captured stage geometry"))?;
        cancel.check()?;
        for body in addition {
            scene.bounds = scene.bounds.union(&body.mesh.bounds());
            scene.meshes.push(body);
        }
        Ok(())
    }
}
impl<'de> Deserialize<'de> for Stage {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct Panels;
        impl<'de> serde::de::Visitor<'de> for Panels {
            type Value = Stage;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("at most 32 passive stage panels")
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<Stage, A::Error> {
                let mut panels = Vec::new();
                while let Some(panel) = seq.next_element::<StagePanel>()? {
                    if panels.len() == MAX_STAGE_PANELS {
                        return Err(serde::de::Error::custom("at most 32 stage panels"));
                    }
                    panel.validate().map_err(serde::de::Error::custom)?;
                    panels.try_reserve_exact(1).map_err(serde::de::Error::custom)?;
                    panels.push(panel);
                }
                Stage::new(panels).map_err(serde::de::Error::custom)
            }
        }
        d.deserialize_seq(Panels)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn panel() -> StagePanel {
        StagePanel { id: "floor".into(), center_mm: [0., 0., -1.], half_u_mm: [20., 0., 0.], half_v_mm: [0., 20., 0.], reflectance_linear: [0.18; 3] }
    }
    #[test]
    fn analytic_clearance_covers_touching_separation_and_rotated_cards() {
        let bounds = Aabb3 { min: Vec3::new(-1., -1., 0.), max: Vec3::new(1., 1., 2.) };
        let mut p = panel();
        assert!(p.separated_from(bounds).unwrap());
        p.center_mm[2] = 0.;
        assert!(!p.separated_from(bounds).unwrap());
        p.center_mm = [40., 0., 1.];
        assert!(p.separated_from(bounds).unwrap());
        p.center_mm = [0., 0., 1.];
        p.half_u_mm = [2., 2., 0.];
        p.half_v_mm = [0., 0., 4.];
        assert!(!p.separated_from(bounds).unwrap());
        p.center_mm = [20., 0., 1.];
        assert!(p.separated_from(bounds).unwrap());
    }
    #[test]
    fn stage_is_passive_atomic_and_preserves_existing_ordinals() {
        let stage = Stage::new(vec![panel()]).unwrap();
        let mut scene = ProductScene { meshes: vec![], bounds: Aabb3::EMPTY };
        stage.append_to(&mut scene, &CancelToken::default()).unwrap();
        assert_eq!(scene.meshes.len(), 1);
        assert_eq!(scene.meshes[0].mesh.triangles.len(), 2);
        assert_eq!(scene.meshes[0].surface.unwrap().dielectric_ior(), 1.0);
        assert!(scene.meshes[0].transmission.is_none());
        let before = scene.meshes[0].mesh.clone();
        assert!(stage.append_to(&mut scene, &CancelToken::default()).is_err());
        assert_eq!(scene.meshes.len(), 1);
        assert!(Arc::ptr_eq(&before, &scene.meshes[0].mesh));
        let token = CancelToken::default();
        token.cancel();
        assert!(stage.append_to(&mut scene, &token).is_err());
    }
    #[test]
    fn staged_metadata_roundtrips_and_hostile_inputs_refuse() {
        let stage = Stage::new(vec![panel()]).unwrap();
        let wire = serde_json::to_string(&stage).unwrap();
        assert_eq!(serde_json::from_str::<Stage>(&wire).unwrap(), stage);
        let mut p = panel();
        p.reflectance_linear[0] = 1.1;
        assert!(Stage::new(vec![p]).is_err());
        assert!(Stage::new(vec![panel(), panel()]).is_err());
        assert!(serde_json::from_value::<Stage>(serde_json::json!(vec![panel(); 33])).is_err());
        let mut wire = serde_json::to_value(&stage).unwrap();
        wire[0]["emission"] = serde_json::json!(5);
        assert!(serde_json::from_value::<Stage>(wire).is_err());
    }
}
