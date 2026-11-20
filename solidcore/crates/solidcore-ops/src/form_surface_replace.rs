//! Portable data adapter for exact support-based planar-face replacement.
use solidcore::eval::plan::EvalSurface;
use solidcore::geom::form_surface_replace as kernel;
use solidcore_convert::vec3;
use solidcore_record::Plane;
use std::collections::BTreeMap;

fn plane(plane: &Plane) -> kernel::HalfSpace {
    kernel::HalfSpace::from_axes(vec3(plane.origin), vec3(plane.x), vec3(plane.y))
}

/// Exact support and trim data for one face in canonical body order.
pub struct SupportFace {
    /// Native surface evaluator.
    pub surface: EvalSurface,
    /// Homogeneous trim controls if all edges supply a control hull.
    pub boundary: Option<Vec<[f64; 4]>>,
    /// The trim consists of one loop of straight edges with known controls.
    pub linear_loop: bool,
    /// Number of edge uses across all trim loops.
    pub edge_count: usize,
}

impl SupportFace {
    fn height(&self, support: kernel::HalfSpace) -> Option<f64> {
        kernel::maximum_height(
            &self.surface,
            self.boundary.as_deref().unwrap_or_default(),
            support,
        )
    }
}

/// Base wall indices mapped to the skin faces lying on each wall.
pub type WallContacts = BTreeMap<usize, Vec<usize>>;

/// Qualify a matched skin for sewing in place of a planar face. For subtraction,
/// supply every base wall plane in face order. Returned contacts identify the
/// coincident skin faces that must be differenced in the wall's plane before
/// sewing. An empty map calls for sewing the unmodified skin to the opening.
/// Read faces lazily so unsupported geometry can stop the operation early.
pub fn replacement_contacts<E>(
    opening: usize,
    selected: Plane,
    cut_planes: Option<&[Plane]>,
    size: f64,
    tolerance: f64,
    base_faces: impl Iterator<Item = Result<SupportFace, E>>,
    skin_faces: impl Iterator<Item = Result<SupportFace, E>>,
) -> Result<Option<WallContacts>, E> {
    if !tolerance.is_finite() || !(1e-9..=0.1).contains(&tolerance) {
        return Ok(None);
    }
    let selected = plane(&selected);
    let opposite = kernel::HalfSpace {
        normal: kernel::reverse_axis(selected.normal),
        ..selected
    };
    let epsilon = kernel::epsilon(size, 1e-9);
    let cut = cut_planes.map(|planes| planes.iter().map(plane).collect::<Vec<_>>());
    let mut corners = Vec::new();
    for face in base_faces {
        let face = face?;
        if cut.is_some() {
            if !face.linear_loop {
                return Ok(None);
            }
            let Some(boundary) = face.boundary.as_deref() else {
                return Ok(None);
            };
            if boundary.len() > 8192_usize.saturating_sub(corners.len()) {
                return Ok(None);
            }
            let Some(points) = kernel::points(boundary) else {
                return Ok(None);
            };
            corners.extend(points);
        } else if face
            .height(selected)
            .is_none_or(|height| height > epsilon.min(tolerance))
        {
            return Ok(None);
        }
    }
    if cut
        .as_ref()
        .is_some_and(|walls| !kernel::inside(&corners, walls, tolerance))
    {
        return Ok(None);
    }

    let mut contacts = WallContacts::new();
    let mut rim_count = 0usize;
    for (index, face) in skin_faces.enumerate() {
        let face = face?;
        rim_count = rim_count.saturating_add(face.edge_count);
        if rim_count > 16_384 {
            return Ok(None);
        }
        if let Some(walls) = &cut {
            let Some(hull) =
                kernel::hull(&face.surface, face.boundary.as_deref().unwrap_or_default())
            else {
                return Ok(None);
            };
            if !kernel::inside(&hull, walls, tolerance) {
                return Ok(None);
            }
            let mut on_wall = walls
                .iter()
                .enumerate()
                .filter(|(_, wall)| kernel::coplanar(&hull, **wall, epsilon));
            if let Some((wall, _)) = on_wall.next() {
                if wall == opening || on_wall.next().is_some() {
                    return Ok(None);
                }
                contacts.entry(wall).or_default().push(index);
            } else if !kernel::strictly_inside(
                &face.surface,
                &hull,
                walls,
                kernel::epsilon(size, 1e-10),
            ) {
                return Ok(None);
            }
        } else {
            if face
                .height(opposite)
                .is_none_or(|height| height > epsilon.min(tolerance))
            {
                return Ok(None);
            }
            let departs = match &face.surface {
                EvalSurface::Nurbs { ctrl, .. } => kernel::points(ctrl).is_some_and(|hull| {
                    kernel::strictly_inside(&face.surface, &hull, &[opposite], epsilon)
                }),
                _ => face.height(selected).is_some_and(|height| height > epsilon),
            };
            if !departs {
                return Ok(None);
            }
        }
    }
    Ok(Some(contacts))
}
