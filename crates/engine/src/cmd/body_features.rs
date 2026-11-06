//! Which timeline features built each body (#11945): the data behind `model.body_features` and
//! the Browser's optional features-under-bodies tree.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::Value;

use crate::params::{bad, str_};
use crate::schema::inspect::{BodyFeature, BodyFeatures, ModelBodyFeatures};
use crate::{Result, Session};

impl Session {
    /// The features that built each body of the evaluated model, by the body's model name, in
    /// timeline order: the feature that created it, every later feature that changed its
    /// geometry, and the features of the bodies a feature consumed while it made or changed this
    /// one (a Combine's tools, a body joined in). A piece a cut split off keeps the features of
    /// the body it was cut from. Features behind the history marker, which built nothing yet,
    /// are not listed.
    ///
    /// Decided on exact body identity (a body a feature passed through unchanged is the same
    /// record), never on a mesh.
    pub fn body_features(&self) -> BTreeMap<String, Vec<u64>> {
        let order: HashMap<u64, usize> = self.doc.features.iter().enumerate().map(|(i, f)| (f.id, i)).collect();
        // Each body before the feature being looked at: its geometry, its creator and its features.
        let mut before: HashMap<String, (solidcraft_doc::kernel::Body, u64, Vec<u64>)> = HashMap::new();
        for r in &self.model.results {
            let id = r.id;
            let after = &r.state.bodies;
            let names: HashSet<&str> = after.iter().map(|b| b.name.as_str()).collect();
            // What the feature consumed: bodies there before and gone after.
            let consumed: Vec<u64> = before.iter().filter(|(name, _)| !names.contains(name.as_str())).flat_map(|(_, (_, _, list))| list.iter().copied()).collect();
            let mut next: HashMap<String, (solidcraft_doc::kernel::Body, u64, Vec<u64>)> = HashMap::with_capacity(after.len());
            for b in after {
                let list = match before.get(&b.name) {
                    // Passed through unchanged.
                    Some((old, creator, list)) if old.same(&b.body) && *creator == b.feature => list.clone(),
                    // Changed: it keeps what built it so far.
                    Some((_, creator, list)) if *creator == b.feature => touched(list.clone(), &consumed, id, &order),
                    // Made by this feature.
                    _ if b.feature == id => touched(Vec::new(), &consumed, id, &order),
                    // A piece of a body made earlier (a split): what built the bodies of its creator.
                    _ => {
                        let inherited = before.values().filter(|(_, creator, _)| *creator == b.feature).flat_map(|(_, _, list)| list.iter().copied()).collect();
                        touched(inherited, &consumed, id, &order)
                    }
                };
                next.insert(b.name.clone(), (b.body.clone(), b.feature, list));
            }
            before = next;
        }
        // The model as it stands (a sketch shown live keeps its last results).
        self.model.state().bodies.iter().map(|b| (b.name.clone(), before.get(&b.name).map(|(_, _, list)| list.clone()).unwrap_or_else(|| vec![b.feature]))).collect()
    }
}

/// `list` with the features of what was consumed and the feature `id` added, once each, in
/// timeline order.
fn touched(mut list: Vec<u64>, consumed: &[u64], id: u64, order: &HashMap<u64, usize>) -> Vec<u64> {
    list.extend(consumed.iter().copied());
    list.push(id);
    list.sort_by_key(|f| order.get(f).copied().unwrap_or(usize::MAX));
    list.dedup();
    list
}

/// `model.body_features`: the features that built each body, or one body (a placed copy's name
/// means the body it shows).
pub(super) fn model_body_features(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "model.body_features";
    let all = s.body_features();
    let wanted = match str_(p, "body") {
        Some(name) => {
            let st = s.world_state()?;
            let b = st.body(name).ok_or_else(|| bad(cmd, format!("no body `{name}`")))?;
            Some((b.name.clone(), b.definition_name().to_string()))
        }
        None => None,
    };
    let feature = |id: &u64| BodyFeature { id: *id, name: s.doc.feature(*id).map(|f| f.name.clone()).unwrap_or_default() };
    let bodies = match wanted {
        Some((shown, definition)) => {
            vec![BodyFeatures { body: shown, features: all.get(&definition).map(|l| l.iter().map(feature).collect()).unwrap_or_default() }]
        }
        None => s
            .model
            .state()
            .bodies
            .iter()
            .map(|b| BodyFeatures { body: b.name.clone(), features: all.get(&b.name).map(|l| l.iter().map(feature).collect()).unwrap_or_default() })
            .collect(),
    };
    serde_json::to_value(ModelBodyFeatures { bodies, param_warnings: Vec::new() }).map_err(|e| bad(cmd, e.to_string()))
}
