//! Oriented stock bounds from planar face normals and straight B-rep edges.
//! Prismatic parts use their exact vertices; curved parts use transformed B-rep bounds
//! and are explicitly approximate. At most 96 distinct frames and 20,000 entities.

use crate::curve::curve::CurveKind;
use crate::error::{Result, SolidError};
use crate::query::api_queries::{brep_bounding_box, vertex_get_point};
use crate::surface::surface::SurfaceKind;
use crate::topology::brep::{mirror::transform_matrix, p1, p3::brep_copy};
use crate::topology::{edge, face, Brep};
use crate::{Kernel, Vec3};

/// Dimensions in descending order, with the axes they were measured along.
#[derive(Clone, Debug)]
pub struct StockBox {
    /// Dimensional classification: `panel`, `board` or `irregular`.
    pub kind: &'static str,
    /// Length, width and thickness in millimetres, in descending order.
    pub dimensions: [f64; 3],
    /// Unit axes in the body's coordinates, in the same order as the dimensions.
    pub axes: [[f64; 3]; 3],
    /// Whether the extents were measured on a straight-edged, planar B-rep's vertices.
    pub exact: bool,
}

/// The least-volume candidate box, exact for straight-edged planar prisms.
pub fn stock_box(k: &mut Kernel, b: &mut Brep) -> Result<StockBox> {
    let bad = || {
        SolidError::invalid_input(
            "stock_box",
            "no finite stock box, or stock measurement work limit exceeded",
        )
    };
    let faces = p1::faces(b, None)?;
    let edges = p1::edges(b, None)?;
    let vertices = p1::vertices(b, None)?;
    if faces
        .len()
        .saturating_add(edges.len())
        .saturating_add(vertices.len())
        > 20_000
    {
        return Err(bad());
    }
    let mut normals: Vec<Vec3> = Vec::new();
    let mut directions: Vec<Vec3> = Vec::new();
    let mut exact = true;
    for f in faces {
        let s = face::surface(b, f)?.ok_or_else(bad)?;
        if let SurfaceKind::Plane(p) = s.kind() {
            let n = p.evaluate_normal().unitize()?;
            if !normals.iter().any(|v| v.dot(n).abs() > 1.0 - 1e-12) {
                normals.push(n);
                if normals.len() > 64 {
                    return Err(bad());
                }
            }
        } else {
            exact = false;
        }
    }
    for e in edges {
        let c = edge::curve(b, e)?.ok_or_else(bad)?;
        exact &= matches!(c.kind(), CurveKind::Line(_));
        if let Some((_, d)) = c.is_line(k, 3, 1e-10)? {
            let d = d.unitize()?;
            if !directions.iter().any(|v| v.dot(d).abs() > 1.0 - 1e-12) {
                directions.push(d);
                if directions.len() > 64 {
                    return Err(bad());
                }
            }
        } else {
            exact = false;
        }
    }
    if normals.len().saturating_mul(directions.len()) > 4096 {
        return Err(bad());
    }
    let points = vertices
        .into_iter()
        .map(|v| vertex_get_point(b, v))
        .collect::<Result<Vec<_>>>()?;
    let mut frames: Vec<[Vec3; 3]> = Vec::new();
    for z in normals {
        for d in &directions {
            let x = *d - z * d.dot(z);
            if x.length_squared() < 1e-16 {
                continue;
            }
            let x = x.unitize()?;
            let y = z.cross(x).unitize()?;
            let frame = [x, y, z];
            if frames.iter().any(|f| {
                frame
                    .iter()
                    .all(|a| f.iter().any(|v| v.dot(*a).abs() > 1.0 - 1e-12))
            }) {
                continue;
            }
            if frames.len() == 96 {
                return Err(bad());
            }
            frames.push(frame);
        }
    }
    if frames.is_empty() {
        exact = false;
        let store = crate::geom::attribute::AttributeStore::default();
        let attrs = crate::mass::integrator::properties::PropertyAttributes {
            store: &store,
            key_of: &|_| crate::geom::attribute::ObjectKey(0),
        };
        let mass = crate::mass::integrator::properties::brep_mass_properties_estimated(
            k,
            b,
            1e-8,
            &crate::mass::integrator::properties::MassPropertyRequest::centroidal(),
            &attrs,
        )?;
        let mut matrix = crate::geom::matrix::Matrix::new(3, 3);
        for (i, row) in mass.inertia_tensor().iter().enumerate() {
            for (j, value) in row.iter().enumerate() {
                matrix.set_at(i, j, *value)?;
            }
        }
        let (_, [x, y, z]) = matrix.compute_3x3_eigen_vectors()?;
        let x = x.unitize()?;
        let y = y.unitize()?;
        let z = if x.cross(y).dot(z) < 0.0 { -z } else { z };
        frames.push([x, y, z.unitize()?]);
    }
    let mut best: Option<(f64, StockBox)> = None;
    for [x, y, z] in frames {
        let dimensions = if exact && !points.is_empty() {
            [x, y, z].map(|axis| {
                let mut lo = f64::INFINITY;
                let mut hi = f64::NEG_INFINITY;
                for p in &points {
                    let t = p.dot(axis);
                    lo = lo.min(t);
                    hi = hi.max(t);
                }
                hi - lo
            })
        } else {
            let matrix = [
                [x.x, y.x, z.x, 0.0],
                [x.y, y.y, z.y, 0.0],
                [x.z, y.z, z.z, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ];
            let mut copy = brep_copy(k, b)?;
            transform_matrix(k, &mut copy, &matrix, true)?;
            let size = brep_bounding_box(k, &copy, true)?.size();
            [size.x, size.y, size.z]
        };
        if !dimensions.iter().all(|d| d.is_finite() && *d > 0.0) {
            return Err(bad());
        }
        let volume = dimensions.iter().product::<f64>();
        if !volume.is_finite() {
            return Err(bad());
        }
        let [dx, dy, dz] = dimensions;
        let mut ordered = [(dx, x), (dy, y), (dz, z)];
        ordered.sort_by(|a, b| b.0.total_cmp(&a.0));
        let dimensions = ordered.map(|(d, _)| d);
        let axes = ordered.map(|(_, v)| [v.x, v.y, v.z]);
        let [length, width, thickness] = dimensions;
        let kind = if thickness <= width / 4.0 {
            "panel"
        } else if length >= width * 3.0 {
            "board"
        } else {
            "irregular"
        };
        if best
            .as_ref()
            .is_none_or(|(v, _)| volume < *v * (1.0 - 1e-12))
        {
            best = Some((
                volume,
                StockBox {
                    kind,
                    dimensions,
                    axes,
                    exact,
                },
            ));
        }
    }
    best.map(|(_, b)| b).ok_or_else(bad)
}

#[cfg(test)]
#[path = "stock_tests.rs"]
mod tests;
