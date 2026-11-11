//! The reference geometry of an exact assembly placement, without timeline features.

use solidcore_record::{Plane, Vec3};
use solidcraft_doc::construct::{ConstructGeom, DatumItem, DatumRef};

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

/// An item key carries the complete path: a shared child in two parents has two origins. The
/// item reads the way the document's datum grammar parses it — case-insensitively, answered in
/// the canonical spelling (`yz` answers `YZ`) — so a resolver that takes a key in any spelling
/// and one that matches the stored spelling agree about what a key names, and a key no datum
/// grammar accepts is no datum anywhere (#7313).
pub fn item(name: &str) -> Option<(Vec<u64>, &'static str)> {
    let (key, item) = name.rsplit_once('/')?;
    let item = ITEMS.iter().find(|known| known.eq_ignore_ascii_case(item))?;
    Some((folder_path(key)?, *item))
}

/// The Browser key of a placement's origin item: the folder path, then the item
/// (`o:2/origin/XY`). Resolvers and the saved visibility speak these keys.
pub fn key(path: &[u64], item: &str) -> String {
    format!("{}/{item}", folder(path))
}

/// The datum an item key names, by the document's shared datum grammar: `o:<ids>/origin/<item>`
/// with its strict bounds. Resolvers save it, so the reference follows the placement.
pub fn datum(name: &str) -> Option<DatumRef> {
    DatumRef::parse_key(name)
}

impl Session {
    /// Resolve a Browser pick even when its origin is hidden. Includes pending placement moves.
    pub fn origin_geometry(&self, name: &str) -> Option<ConstructGeom> {
        let d = datum(name)?;
        let placement = self.occurrence_placement(&d.path).ok()?;
        Some(d.item.geom().placed(&placement.world))
    }

    /// The plane a component origin key names, placed where its component now stands:
    /// `None` for the axes and the point, and for a key whose placement is gone.
    pub fn origin_plane(&self, name: &str) -> Option<Plane> {
        match self.origin_geometry(name)? {
            ConstructGeom::Plane(p) => Some(p),
            _ => None,
        }
    }

    /// The axis a component origin key names, placed where its component now stands:
    /// `None` for the planes and the point, and for a key whose placement is gone.
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
                    && let Some(geom) = DatumItem::parse(item).map(|d| d.geom())
                {
                    out.push((name, geom.placed(&placement.world)));
                }
            }
        }
        out
    }
}
