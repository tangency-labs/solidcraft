//! Component envelopes in the checks: a Bounding Solid made with `component` (`contents`) bounds
//! every body placed inside its component. `inspect.clearance` and `inspect.interference` leave
//! envelopes out unless a call names them, never pair an envelope with a body it bounds, and with
//! `envelopes: true` check each envelope instead of the bodies inside it: one box for an imported
//! assembly of hundreds of parts.

use std::collections::{HashMap, HashSet};

use serde_json::Value;
use solidcraft_doc::{FeatureKind, ModelState};

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
    /// The envelopes among `world`'s bodies (`world` is the session's world state).
    pub(super) fn of(s: &Session, world: &ModelState) -> Self {
        let made: HashSet<u64> =
            s.doc.features.iter().filter(|f| matches!(f.kind, FeatureKind::BoundingSolid { contents: true, .. })).map(|f| f.id).collect();
        if made.is_empty() {
            return Self::default();
        }
        let body_paths: HashMap<String, Vec<u64>> = s.doc.body_frames(&s.model.state()).into_iter().map(|(name, _, path, _)| (name, path)).collect();
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
            let outer = envelopes.iter().find(|(name, at)| *name != b.name && path.starts_with(at));
            if let Some((name, _)) = outer {
                holder.insert(b.name.clone(), name.clone());
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

/// The `envelopes` parameter: true checks each envelope instead of the bodies inside it.
pub(super) fn use_envelopes(p: &Value, cmd: &str) -> Result<bool> {
    match p.get("envelopes") {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(bad(cmd, crate::schema::clearance::ENVELOPES_MESSAGE)),
    }
}
