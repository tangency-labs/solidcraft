//! A tool drawn for the simulation view, display only: its cutting end turned from the exact
//! profile the simulation cuts with (flat, ball, bull nose, drill point, chamfer taper, face
//! mill corner; a thread mill with its rows of teeth), the rest of its flute length, the plain
//! shank up to the holder, and every holder section. The tip is at the origin, the axis +Z.
//! Nothing decides with these meshes. Ported from the author's own MIT-licensed G-code
//! simulator: the tool turned from its profile.

use std::f64::consts::TAU;

use solidcore_record::Vec3;
use solidcraft_geom::Mesh;

use crate::sim::Profile;
use crate::tool::{Tool, ToolKind};
use crate::{Result, invalid};

/// The tool's parts as meshes.
#[derive(Clone, Debug, Default)]
pub struct ToolMeshes {
    /// The flute length: the cutting end and the flutes above it.
    pub cutting: Mesh,
    /// The plain shank from the flutes to the holder (empty when the stickout is the flutes').
    pub shank: Mesh,
    /// The holder's sections (empty when it has none).
    pub holder: Mesh,
}

/// `profile` (radius, height pairs from the bottom of the axis up) turned about Z in `segments`
/// steps, closed at the axis where it starts or ends there.
fn lathe(profile: &[(f64, f64)], segments: usize) -> Mesh {
    let mut m = Mesh::default();
    let n = segments.clamp(3, 256);
    let ring = |k: usize| -> Vec<u32> { (0..n).map(|s| u32::try_from(k * n + s).unwrap_or(0)).collect() };
    for &(r, z) in profile {
        for s in 0..n {
            let (sa, ca) = (s as f64 * TAU / n as f64).sin_cos();
            m.positions.push(Vec3::new(r * ca, r * sa, z));
        }
    }
    for k in 0..profile.len().saturating_sub(1) {
        let (a, b) = (ring(k), ring(k + 1));
        for s in 0..n {
            let t = (s + 1) % n;
            if let (Some(&a0), Some(&a1), Some(&b0), Some(&b1)) = (a.get(s), a.get(t), b.get(s), b.get(t)) {
                m.triangles.push([a0, a1, b1]);
                m.triangles.push([a0, b1, b0]);
            }
        }
    }
    m
}

/// A closed cylinder of radius `r` from `z0` to `z1`.
fn cylinder(r: f64, z0: f64, z1: f64, segments: usize) -> Mesh {
    lathe(&[(0.0, z0), (r, z0), (r, z1), (0.0, z1)], segments)
}

fn append(into: &mut Mesh, m: Mesh) {
    let base = u32::try_from(into.positions.len()).unwrap_or(u32::MAX);
    into.positions.extend(m.positions);
    into.triangles.extend(m.triangles.into_iter().map(|t| t.map(|i| i.saturating_add(base))));
}

/// `tool`'s meshes, `segments` round (3 to 256).
pub fn tool_meshes(tool: &Tool, segments: usize) -> Result<ToolMeshes> {
    let p = Profile::of(tool)?;
    let r = p.radius;
    let flutes = tool.flute_length.max(p.height(r) + 1e-3);
    if !(r.is_finite() && flutes.is_finite()) {
        return invalid("the tool's sizes must be finite");
    }
    // The cutting end from the axis out: the exact profile, then up the side.
    let mut prof: Vec<(f64, f64)> = Vec::new();
    let steps = 24usize;
    for k in 0..=steps {
        let d = r * k as f64 / steps as f64;
        prof.push((d, p.height(d)));
    }
    match (tool.kind, tool.thread_pitch) {
        (ToolKind::ThreadMill, pitch) => {
            // Rows of teeth over the flute length: tips at the radius, roots in by the tooth height.
            let depth = tool.tooth_height.unwrap_or(r * 0.3).clamp(0.0, r * 0.9);
            let pitch = pitch.filter(|q| *q > 1e-3).unwrap_or(flutes);
            let rows = ((flutes / pitch).ceil() as usize).clamp(1, 200);
            for row in 0..rows {
                let z = row as f64 * pitch;
                prof.push((r - depth, (z + pitch * 0.5).min(flutes)));
                prof.push((r, (z + pitch).min(flutes)));
            }
        }
        _ => prof.push((r, flutes)),
    }
    prof.push((0.0, flutes));
    let cutting = lathe(&prof, segments);
    let stickout = tool.stickout.unwrap_or(tool.flute_length).max(tool.flute_length);
    let shank = if stickout > flutes + 1e-9 {
        cylinder(tool.shank_diameter.unwrap_or(tool.diameter) * 0.5, flutes, stickout, segments)
    } else {
        Mesh::default()
    };
    let mut holder = Mesh::default();
    let mut z = stickout;
    for s in &tool.holder {
        append(&mut holder, cylinder(s.diameter * 0.5, z, z + s.length, segments));
        z += s.length;
    }
    Ok(ToolMeshes { cutting, shank, holder })
}
