//! Component envelopes in the checks: a Bounding Solid made with `component` (`contents`) bounds
//! every body placed inside its component. `inspect.clearance` and `inspect.interference` leave
//! envelopes out unless a call names them, never pair an envelope with a body it bounds, and with
//! `envelopes: true` check each envelope instead of the bodies inside it: one box for an imported
//! assembly of hundreds of parts.

use std::collections::{HashMap, HashSet};

use serde_json::Value;
use solidcore_record::Vec3;
use solidcraft_doc::{FeatureKind, Mat, ModelState};
use solidcraft_geom::Aabb3;

use crate::params::bad;
use crate::{Result, Session};

/// The envelopes of a world state, by world name: each envelope's occurrence path, and which
/// envelope stands in for each body inside one (the outermost, when envelopes nest).
#[derive(Default)]
pub(super) struct Envelopes {
    paths: HashMap<String, Vec<u64>>,
    body_paths: HashMap<String, Vec<u64>>,
    holder: HashMap<String, String>,
}

impl Envelopes {
    /// The envelopes among `world`'s bodies (`world` is the session's world state). A body
    /// inside an envelope's component is held by that envelope only when it lies inside the box:
    /// one a later feature added or grew past it is checked on its own. An envelope is held only
    /// by one of an outer component, never by another of its own.
    pub(super) fn of(s: &Session, world: &ModelState) -> Self {
        let made: HashSet<u64> =
            s.doc.features.iter().filter(|f| matches!(f.kind, FeatureKind::BoundingSolid { contents: true, .. })).map(|f| f.id).collect();
        if made.is_empty() {
            return Self::default();
        }
        let model = s.model.state();
        let frames = s.doc.body_frames_with_source(&model);
        let body_paths: HashMap<String, Vec<u64>> = frames.iter().map(|(name, _, _, path, _)| (name.clone(), path.clone())).collect();
        // Each envelope's box in its own frame, and the map from the world into that frame.
        let boxes: HashMap<&str, (Aabb3, Mat)> = frames
            .iter()
            .filter_map(|(name, source, _, _, m)| {
                let b = model.body(source).filter(|b| made.contains(&b.feature))?;
                Some((name.as_str(), (b.body.exact_bounds().ok()?, solidcraft_doc::mat_inverse(m)?)))
            })
            .collect();
        let mut envelopes: Vec<(String, Vec<u64>)> = world
            .bodies
            .iter()
            .filter(|b| made.contains(&b.feature))
            .map(|b| (b.name.clone(), body_paths.get(&b.name).cloned().unwrap_or_default()))
            .collect();
        // Outermost first: it stands in for everything inside it, inner envelopes included.
        envelopes.sort_by_key(|(_, path)| path.len());
        let mut holder = HashMap::new();
        for b in &world.bodies {
            let path = body_paths.get(&b.name).map_or(&[][..], Vec::as_slice);
            let is_envelope = made.contains(&b.feature);
            let candidates = envelopes.iter().filter(|(name, at)| *name != b.name && path.starts_with(at) && (!is_envelope || at.len() < path.len()));
            let mut world_box = None;
            for (name, _) in candidates {
                let bounds = *world_box.get_or_insert_with(|| b.body.exact_bounds().ok());
                if boxes.get(name.as_str()).zip(bounds).is_some_and(|((env, into), bb)| inside(bb, env, into, b.body.record().tolerance)) {
                    holder.insert(b.name.clone(), name.clone());
                    break;
                }
            }
        }
        Self { paths: envelopes.into_iter().collect(), body_paths, holder }
    }

    /// Whether `name` is an envelope.
    pub(super) fn is_envelope(&self, name: &str) -> bool {
        self.paths.contains_key(name)
    }

    /// The envelope that stands in for `name` with `envelopes: true`: the outermost one around
    /// it, else `name` itself.
    pub(super) fn standing_in<'a>(&'a self, name: &'a str) -> &'a str {
        self.holder.get(name).map_or(name, String::as_str)
    }

    /// Whether a body takes part in a check that names no bodies: with `use_envelopes`, each
    /// body not inside an envelope (envelopes included); without, every body but the envelopes.
    pub(super) fn by_default(&self, name: &str, use_envelopes: bool) -> bool {
        if use_envelopes { !self.holder.contains_key(name) } else { !self.is_envelope(name) }
    }

    /// Whether one of `a` and `b` is an envelope around the other: such a pair is never checked.
    pub(super) fn encloses(&self, a: &str, b: &str) -> bool {
        let inside = |env: &str, body: &str| {
            self.paths.get(env).is_some_and(|at| self.body_paths.get(body).is_some_and(|path| path.starts_with(at))) && env != body
        };
        inside(a, b) || inside(b, a)
    }
}

/// Whether the world box `bb` lies inside the envelope box `env` of a frame `into` maps the
/// world to, within `tol`: every corner of `bb` taken into that frame lies in `env`.
fn inside(bb: Aabb3, env: &Aabb3, into: &Mat, tol: f64) -> bool {
    if bb.is_empty() || env.is_empty() {
        return false;
    }
    let env = env.inflated(tol.max(1e-6));
    (0..8).all(|k| {
        let corner = Vec3::new(
            if k & 1 == 0 { bb.min.x } else { bb.max.x },
            if k & 2 == 0 { bb.min.y } else { bb.max.y },
            if k & 4 == 0 { bb.min.z } else { bb.max.z },
        );
        env.contains(solidcraft_doc::apply_point(into, corner))
    })
}

/// The `envelopes` parameter: true checks each envelope instead of the bodies inside it.
pub(super) fn use_envelopes(p: &Value, cmd: &str) -> Result<bool> {
    match p.get("envelopes") {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(bad(cmd, crate::schema::clearance::ENVELOPES_MESSAGE)),
    }
}
