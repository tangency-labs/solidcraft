//! Saved weathering intent uses stable body/occurrence identity. Captured mesh
//! ordinals exist only in a prepared runtime job, never in the design file.
use crate::{
    DocError, Result,
    render_surface::{RenderBodyTarget, bounded_sequence},
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use solidcraft_render::job::aging::{AgingLayer, AgingStack, BodyTarget, MAX_AGING_LAYERS};
use std::ops::{Deref, DerefMut};

fn invalid(message: impl Into<String>) -> DocError {
    DocError::Invalid(message.into())
}

#[derive(Clone, Debug, PartialEq)]
pub struct SavedAgingLayer {
    parameters: AgingLayer,
    pub target: Option<RenderBodyTarget>,
}
impl SavedAgingLayer {
    pub fn validate(&self) -> Result<()> {
        self.parameters.validate().map_err(|e| invalid(e.to_string()))?;
        if self.parameters.target.is_some() {
            return Err(invalid("captured aging body indices cannot be saved; bind a stable occurrence identity"));
        }
        if let Some(target) = &self.target {
            target.validate()?;
            if target.definition.starts_with("__render_stage__/") {
                return Err(invalid("aging cannot target passive render-stage geometry"));
            }
        }
        Ok(())
    }
}
impl Deref for SavedAgingLayer {
    type Target = AgingLayer;
    fn deref(&self) -> &AgingLayer {
        &self.parameters
    }
}
impl DerefMut for SavedAgingLayer {
    fn deref_mut(&mut self) -> &mut AgingLayer {
        &mut self.parameters
    }
}
impl Serialize for SavedAgingLayer {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        self.validate().map_err(serde::ser::Error::custom)?;
        let mut value = serde_json::to_value(self.parameters).map_err(serde::ser::Error::custom)?;
        if let Some(target) = &self.target {
            let object = value.as_object_mut().ok_or_else(|| serde::ser::Error::custom("aging parameters are not an object"))?;
            object.insert("target".into(), serde_json::to_value(target).map_err(serde::ser::Error::custom)?);
        }
        value.serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for SavedAgingLayer {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let mut value = serde_json::Value::deserialize(deserializer)?;
        let object = value.as_object_mut().ok_or_else(|| serde::de::Error::custom("saved aging layer must be an object"))?;
        if object.len() > 16 {
            return Err(serde::de::Error::custom("too many saved aging fields"));
        }
        let target = match object.remove("target") {
            None | Some(serde_json::Value::Null) => None,
            Some(value) => Some(serde_json::from_value(value).map_err(|_| serde::de::Error::custom(
                "saved aging requires definition/feature/component/occurrence_path; legacy captured body_index/body_name requires explicit rebinding"))?),
        };
        let parameters = serde_json::from_value(value).map_err(serde::de::Error::custom)?;
        let layer = Self { parameters, target };
        layer.validate().map_err(serde::de::Error::custom)?;
        Ok(layer)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(transparent)]
pub struct SavedAgingStack(Vec<SavedAgingLayer>);
impl SavedAgingStack {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn iter(&self) -> impl Iterator<Item = &SavedAgingLayer> {
        self.0.iter()
    }
    pub fn get(&self, index: usize) -> Option<&SavedAgingLayer> {
        self.0.get(index)
    }
    pub fn get_mut(&mut self, index: usize) -> Option<&mut SavedAgingLayer> {
        self.0.get_mut(index)
    }
    pub fn validate(&self) -> Result<()> {
        if self.0.len() > MAX_AGING_LAYERS || self.0.capacity() > MAX_AGING_LAYERS {
            return Err(invalid("at most eight saved aging layers"));
        }
        for layer in &self.0 {
            layer.validate()?;
        }
        Ok(())
    }
    pub fn push(&mut self, parameters: AgingLayer) -> Result<()> {
        let layer = SavedAgingLayer { parameters, target: None };
        layer.validate()?;
        if self.0.len() == MAX_AGING_LAYERS {
            return Err(invalid("at most eight saved aging layers"));
        }
        self.0.try_reserve_exact(1).map_err(|_| invalid("could not reserve saved aging layer"))?;
        self.0.push(layer);
        Ok(())
    }
    pub fn remove(&mut self, index: usize) -> Result<()> {
        if index >= self.0.len() {
            return Err(invalid("saved aging layer is missing"));
        }
        self.0.remove(index);
        Ok(())
    }
    pub fn move_layer(&mut self, from: usize, to: usize) -> Result<()> {
        if from >= self.0.len() || to >= self.0.len() {
            return Err(invalid("saved aging reorder index is missing"));
        }
        if from < to {
            self.0.get_mut(from..=to).ok_or_else(|| invalid("invalid aging reorder range"))?.rotate_left(1);
        } else if to < from {
            self.0.get_mut(to..=from).ok_or_else(|| invalid("invalid aging reorder range"))?.rotate_right(1);
        }
        Ok(())
    }
    /// Numerical validation only, without claiming any runtime target resolution.
    pub(crate) fn parameters_only(&self) -> Result<AgingStack> {
        self.validate()?;
        let mut result = AgingStack::default();
        for layer in &self.0 {
            result.push(layer.parameters).map_err(|e| invalid(e.to_string()))?;
        }
        Ok(result)
    }
    /// Only a scene owner can resolve a saved identity into a captured ordinal.
    pub fn resolve(&self, mut resolve: impl FnMut(&RenderBodyTarget) -> Result<BodyTarget>) -> Result<AgingStack> {
        self.validate()?;
        let mut result = AgingStack::default();
        for saved in &self.0 {
            let mut layer = saved.parameters;
            layer.target = saved.target.as_ref().map(&mut resolve).transpose()?;
            result.push(layer).map_err(|e| invalid(e.to_string()))?;
        }
        Ok(result)
    }
}
impl<'de> Deserialize<'de> for SavedAgingStack {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let result = Self(bounded_sequence::<D, SavedAgingLayer, MAX_AGING_LAYERS>(d)?);
        result.validate().map_err(serde::de::Error::custom)?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use solidcraft_render::job::aging::AgingKind;
    #[test]
    fn saved_layers_roundtrip_stable_occurrences_and_keep_untargeted_legacy_fields() {
        let original = AgingLayer::preset(AgingKind::Dust);
        let mut stack: SavedAgingStack = serde_json::from_value(json!([original])).unwrap();
        assert_eq!(serde_json::to_value(&stack).unwrap(), json!([original]));
        stack.get_mut(0).unwrap().target =
            Some(RenderBodyTarget { definition: "Panel".into(), feature: 7, component: 3, occurrence_path: vec![5, 8] });
        let wire = serde_json::to_value(&stack).unwrap();
        assert_eq!(serde_json::from_value::<SavedAgingStack>(wire.clone()).unwrap(), stack);
        assert!(wire[0]["target"].get("body_index").is_none());
        assert_eq!(wire[0]["target"]["occurrence_path"], json!([5, 8]));
        stack.push(AgingLayer::preset(AgingKind::Rust)).unwrap();
        let target = stack.get(0).unwrap().target.clone();
        stack.move_layer(0, 1).unwrap();
        assert_eq!(stack.get(1).unwrap().target, target);
        stack.remove(0).unwrap();
        assert_eq!(stack.get(0).unwrap().target, target);
    }
    #[test]
    fn captured_or_ambiguous_legacy_targets_never_silently_rebind() {
        let layer = serde_json::to_value(AgingLayer::default()).unwrap();
        for target in [json!({"body_index":0,"body_name":"Panel"}), json!({"definition":"Panel","feature":1,"component":3,"occurrence_path":[]})] {
            let mut value = layer.clone();
            value["target"] = target;
            assert!(serde_json::from_value::<SavedAgingStack>(json!([value])).is_err());
        }
        assert!(serde_json::from_value::<SavedAgingStack>(json!(vec![layer; 9])).is_err());
    }
}
