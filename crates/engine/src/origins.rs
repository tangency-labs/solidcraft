//! The reference geometry of an exact assembly placement, without timeline features.

use solidcraft_doc::construct::ConstructGeom;
use solidcraft_geom::{Plane, Vec3};

use crate::Session;

pub const ITEMS: [&str; 7] = ["O", "X", "Y", "Z", "XY", "XZ", "YZ"];
pub const MAX_SHOWN: usize = 10_000;

/// A component Origin folder uses the same scope as its Bodies and Sketches folders.
pub fn folder(path: &[u64]) -> String {
    format!("{}/origin", Session::folder_scope(path))
}

/// Parse a bounded, canonical placement folder key. The design's origin has its own controls.
pub fn folder_path(key: &str) -> Option<Vec<u64>> {
    if key.len() > 2048 {
        return None;
    }
    let ids = key.strip_prefix("o:")?.strip_suffix("/origin")?;
    if ids.is_empty() || ids.split('/').count() > 64 {
        return None;
    }
    ids.split('/').map(|id| id.parse::<u64>().ok().filter(|n| *n != 0 && n.to_string() == id)).collect()
}

/// An item key carries the complete path: a shared child in two parents has two origins.
pub fn item(name: &str) -> Option<(Vec<u64>, &str)> {
    let (key, item) = name.rsplit_once('/')?;
    ITEMS.contains(&item).then_some(())?;
    Some((folder_path(key)?, item))
}

pub fn key(path: &[u64], item: &str) -> String {
    format!("{}/{item}", folder(path))
}

fn local(item: &str) -> Option<ConstructGeom> {
    Some(match item {
        "O" => ConstructGeom::Point(Vec3::ZERO),
        "X" => ConstructGeom::Axis { origin: Vec3::ZERO, dir: Vec3::X },
        "Y" => ConstructGeom::Axis { origin: Vec3::ZERO, dir: Vec3::Y },
        "Z" => ConstructGeom::Axis { origin: Vec3::ZERO, dir: Vec3::Z },
        name => ConstructGeom::Plane(Plane::named(name)?),
    })
}

impl Session {
    /// Resolve a Browser pick even when its origin is hidden. Includes pending placement moves.
    pub fn origin_geometry(&self, name: &str) -> Option<ConstructGeom> {
        let (path, item) = item(name)?;
        let placement = self.occurrence_placement(&path).ok()?;
        Some(local(item)?.placed(&placement.world))
    }

    pub fn origin_plane(&self, name: &str) -> Option<Plane> {
        match self.origin_geometry(name)? {
            ConstructGeom::Plane(p) => Some(p),
            _ => None,
        }
    }

    pub fn origin_axis(&self, name: &str) -> Option<(Vec3, Vec3)> {
        match self.origin_geometry(name)? {
            ConstructGeom::Axis { origin, dir } => Some((origin, dir)),
            _ => None,
        }
    }

    /// Reference geometry explicitly shown by the user. No assembly expansion on every frame.
    pub fn shown_component_origins(&self) -> Vec<(String, ConstructGeom)> {
        let mut out = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for path in self.visibility.shown_component_origins.iter().take(MAX_SHOWN) {
            if path.is_empty() || path.len() > 64 || !seen.insert(path) || !self.occurrence_visible(path) {
                continue;
            }
            let Ok(placement) = self.occurrence_placement(path) else { continue };
            for item in ITEMS {
                let name = key(path, item);
                if !self.visibility.hidden_origin.contains(&name)
                    && let Some(geom) = local(item)
                {
                    out.push((name, geom.placed(&placement.world)));
                }
            }
        }
        out
    }
}
