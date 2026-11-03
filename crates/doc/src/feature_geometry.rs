//! The geometry a feature holds, each piece with its role: the points that pick faces (moved
//! with the feature, and the source of its persistent face names), other positions, directions,
//! planes, construction references, a primitive's placement point and its profile selections.
//! The walk (`crate::walk`) is behind `transform_geometry`, `face_points`, `profile_sels` and
//! `point_expr`.

use solidcraft_geom::Vec3;

use crate::walk::Walk;
use crate::{FeatureKind, PointExpr, ProfileSel};

/// A piece of a feature's geometry: `V` is `&Vec3` or `&mut Vec3`, and so on for the others.
pub(crate) enum Geo<V, P, G, E, S> {
    /// A point that picks a face: it moves with the feature and names the face it picks.
    Pick(V),
    /// A point that picks a face but stays where it is when the feature is moved (an Emboss
    /// cylinder, chosen in its component's frame).
    Pinned(V),
    /// Any other position (an edge pick, an axis origin, a fold line): it moves with the feature.
    Pos(V),
    /// A direction: it turns with the feature.
    Dir(V),
    Plane(P),
    /// A construction axis's or point's reference to the model.
    Construct(G),
    /// A primitive's placement point (length expressions).
    Placement(E),
    /// A profile selection, with its sketch.
    Profiles(u64, S),
}

impl FeatureKind {
    /// The points that pick faces (shell, draft, offset and replace faces, a hole's placement and
    /// `to`, an extrude's `to` and `from`…), in the order of the feature's persistent face names.
    pub fn face_points_mut(&mut self) -> Vec<&mut Vec3> {
        let mut v = Vec::new();
        self.walk_geometry_mut(&mut |g| {
            if let Geo::Pick(p) | Geo::Pinned(p) = g {
                v.push(p);
            }
        });
        v
    }

    /// Read-only view of [`FeatureKind::face_points_mut`].
    pub fn face_points(&self) -> Vec<Vec3> {
        let mut v = Vec::new();
        self.walk_geometry(&mut |g| {
            if let Geo::Pick(p) | Geo::Pinned(p) = g {
                v.push(*p);
            }
        });
        v
    }

    /// The body a feature that works on one body's faces names (shell, draft, split, offset,
    /// delete and replace face, surface offset, the placed plastic features, sheet metal
    /// conversion, ruled surface, untrim): its face points pick faces of that body.
    pub fn face_body(&self) -> Option<&str> {
        match self {
            FeatureKind::Shell { body, .. }
            | FeatureKind::Draft { body, .. }
            | FeatureKind::SplitFace { body, .. }
            | FeatureKind::OffsetFace { body, .. }
            | FeatureKind::DeleteFace { body, .. }
            | FeatureKind::ReplaceFace { body, .. }
            | FeatureKind::SurfaceOffset { body, .. }
            | FeatureKind::Boss { body, .. }
            | FeatureKind::SnapFit { body, .. }
            | FeatureKind::Rest { body, .. } => body.as_deref(),
            FeatureKind::SheetConvert { body, .. } | FeatureKind::SurfaceRuled { body, .. } => Some(body.as_ref()),
            FeatureKind::SurfaceUntrim { face, .. } => Some(face.body.as_ref()),
            _ => None,
        }
    }

    /// The feature's profile selections, each with its sketch.
    pub fn profile_sels_mut(&mut self) -> Vec<(u64, &mut ProfileSel)> {
        let mut v = Vec::new();
        self.walk_geometry_mut(&mut |g| {
            if let Geo::Profiles(sketch, sel) = g {
                v.push((sketch, sel));
            }
        });
        v
    }

    /// Read-only view of [`FeatureKind::profile_sels_mut`].
    pub fn profile_sels(&self) -> Vec<(u64, &ProfileSel)> {
        let mut v = Vec::new();
        self.walk_geometry(&mut |g| {
            if let Geo::Profiles(sketch, sel) = g {
                v.push((sketch, sel));
            }
        });
        v
    }

    /// A primitive's placement point (length expressions). Its coordinates are not inputs:
    /// they get no parameter names and keep bare numbers in mm whatever the design's units, as
    /// every other position does; renames and users still find them.
    pub fn point_expr_mut(&mut self) -> Option<&mut PointExpr> {
        let mut found = None;
        self.walk_geometry_mut(&mut |g| {
            if let Geo::Placement(p) = g {
                found = Some(p);
            }
        });
        found
    }

    /// Read-only view of [`FeatureKind::point_expr_mut`].
    pub fn point_expr(&self) -> Option<&PointExpr> {
        let mut found = None;
        self.walk_geometry(&mut |g| {
            if let Geo::Placement(p) = g {
                found = Some(p);
            }
        });
        found
    }
}
