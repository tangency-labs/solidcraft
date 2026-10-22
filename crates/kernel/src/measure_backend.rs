//! Mass properties through SMLib, when [`crate::backend`] is SMLib: the body goes to SMLib as its
//! schema 4 record and SMLib integrates its volume, area and centroid on the exact surfaces
//! (`solidcraft_smlib::Brep::mass_properties`). The mesh measure is always made too, as an
//! independent check: where SMLib disagrees with it beyond the mesh's own accuracy
//! (tangency-labs/solidcore#384), or can't take the body, the mesh measure stands and the
//! measurement says why in its notes.

use solidcraft_geom::Vec3;

use crate::body::Body;
use crate::measure::MassMeasure;

/// The mesh measure's own accuracy (relative, for volume and area; of the body's size, for the
/// centroid): SMLib must agree with it this closely for its numbers to be used.
const AGREE: f64 = 1e-3;

/// The relative accuracy SMLib integrates at: the finest of SmApiBrepComputeMassProperties'
/// documented range ("clamped to [1e-4, 1e-1]", SmApiQueries.h), ten times inside the 1e-3 the
/// measure is held to ([`AGREE`], and the regression rig's volume and area tolerance). Its cost is
/// "governed by dRelativeAccuracy": the finest the integrator allows, 1e-8, took tens of seconds
/// on a modelled thread's faces.
const ACCURACY: f64 = 1e-4;

/// Exact volume, area and centroid.
pub(crate) struct Exact {
    pub volume: f64,
    pub area: f64,
    pub centroid: Vec3,
}

/// What the measurement comes from, and what it says about it.
pub(crate) struct Choice {
    pub by: &'static str,
    pub notes: Vec<String>,
}

/// SMLib's mass properties of `b`, or why there are none.
#[cfg(feature = "smlib")]
fn exact(b: &Body) -> Result<Exact, String> {
    let sb = crate::native::smlib_body(b).map_err(|e| e.to_string())?;
    let m = sb.mass_properties(ACCURACY).map_err(|e| format!("SMLib mass properties: {e}"))?;
    let [x, y, z] = m.centroid;
    Ok(Exact { volume: m.volume, area: m.area, centroid: Vec3::new(x, y, z) })
}

#[cfg(not(feature = "smlib"))]
fn exact(_: &Body) -> Result<Exact, String> {
    Err("this build has no SMLib".into())
}

/// Where the kernel setting is SMLib, replaces `mesh`'s volume, area and centroid with SMLib's
/// exact ones when they agree with it; otherwise keeps the mesh measure and notes why. The box
/// stays the mesh's.
pub(crate) fn choose(b: &Body, mesh: &mut MassMeasure) -> Choice {
    if !matches!(crate::backend(), Ok(crate::Backend::Smlib)) {
        return Choice { by: "mesh", notes: Vec::new() };
    }
    let e = match crate::guard("SMLib mass properties", || Ok(exact(b))) {
        Ok(Ok(e)) => e,
        Ok(Err(why)) | Err(crate::KernelError::Internal(why)) => {
            return Choice { by: "mesh", notes: vec![format!("measured from the mesh: SMLib could not measure this body ({why})")] };
        }
        Err(e) => return Choice { by: "mesh", notes: vec![format!("measured from the mesh: SMLib could not measure this body ({e})")] },
    };
    match disagreement(&e, mesh, b.size()) {
        None => {
            mesh.volume = e.volume;
            mesh.area = e.area;
            mesh.centroid = e.centroid;
            Choice { by: "smlib", notes: Vec::new() }
        }
        Some(what) => Choice {
            by: "mesh",
            notes: vec![format!("measured from the mesh: SMLib's exact mass properties disagree with it ({what}; tangency-labs/solidcore#384)")],
        },
    }
}

/// Where SMLib's numbers and the mesh's differ by more than the mesh's accuracy, which and by how
/// much.
fn disagreement(e: &Exact, m: &MassMeasure, size: f64) -> Option<String> {
    let rel = |a: f64, b: f64| (a - b).abs() / a.abs().max(b.abs()).max(1e-12);
    let finite = e.volume.is_finite() && e.area.is_finite() && [e.centroid.x, e.centroid.y, e.centroid.z].iter().all(|x| x.is_finite());
    if !finite {
        return Some("SMLib's numbers are not finite".into());
    }
    let mut out = Vec::new();
    if rel(e.volume, m.volume) > AGREE {
        out.push(format!("volume {:.6} against {:.6}", e.volume, m.volume));
    }
    if rel(e.area, m.area) > AGREE {
        out.push(format!("area {:.6} against {:.6}", e.area, m.area));
    }
    let d = e.centroid.dist(m.centroid);
    if d > AGREE * size.max(1e-9) {
        out.push(format!("centroid {d:.6} mm apart"));
    }
    (!out.is_empty()).then(|| out.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_geom::Aabb3;

    fn mesh(volume: f64, area: f64, centroid: Vec3) -> MassMeasure {
        MassMeasure { volume, area, centroid, bbox: Aabb3::EMPTY, measured_by: "mesh", notes: Vec::new() }
    }

    #[test]
    fn agreement_is_within_the_mesh_accuracy() {
        let e = Exact { volume: 1000.0, area: 600.0, centroid: Vec3::new(5.0, 5.0, 5.0) };
        assert!(disagreement(&e, &mesh(1000.4, 600.2, Vec3::new(5.0, 5.0, 5.001)), 17.3).is_none());
        let off = disagreement(&e, &mesh(1015.0, 600.0, Vec3::new(5.0, 5.0, 5.0)), 17.3).unwrap();
        assert!(off.contains("volume 1000.000000 against 1015.000000"), "{off}");
        let moved = disagreement(&e, &mesh(1000.0, 600.0, Vec3::new(5.0, 5.0, 6.0)), 17.3).unwrap();
        assert!(moved.contains("centroid"), "{moved}");
        let nan = Exact { volume: f64::NAN, area: 600.0, centroid: Vec3::ZERO };
        assert!(disagreement(&nan, &mesh(1000.0, 600.0, Vec3::ZERO), 17.3).is_some());
    }
}
