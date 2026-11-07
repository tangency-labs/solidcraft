//! Exact, immutable opaque image bytes owned by a design. Interpretation belongs
//! to each saved map role; provenance is bounded inert text, never a permission grant.
use crate::{DocError, Result, render_resources::HdrId};
use serde::{Deserialize, Serialize, de::DeserializeSeed, ser::SerializeStruct};
use solidcraft_render::texture_input::{MapRole, TextureSource};
use std::{collections::BTreeMap, fmt, sync::Arc};

pub const MAX_TEXTURE_RESOURCES: usize = 32;
pub const MAX_TEXTURE_RESOURCE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_TEXTURE_BYTES: usize = solidcraft_render::texture_input::MAX_TEXTURE_BYTES;
pub const MAX_TEXTURE_EDGE: u32 = 1024;
const MAX_PROVENANCE_JSON: usize = 48 * 1024;
fn invalid(message: impl Into<String>) -> DocError {
    DocError::Invalid(message.into())
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TextureProvenance {
    #[serde(deserialize_with = "bounded_text")]
    pub author: String,
    #[serde(deserialize_with = "bounded_text")]
    pub source: String,
    #[serde(deserialize_with = "bounded_text")]
    pub license: String,
}
impl TextureProvenance {
    pub fn validate(&self) -> Result<()> {
        for (text, maximum) in [(&self.author, 512), (&self.source, 2048), (&self.license, 4096)] {
            if text.len() > maximum || text.chars().any(char::is_control) {
                return Err(invalid("texture provenance requires bounded inert author, source and license text without control characters"));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextureResource {
    id: HdrId,
    bytes: Arc<Vec<u8>>,
    dimensions: [u32; 2],
    provenance: TextureProvenance,
}
impl TextureResource {
    /// Import runs on the caller's existing worker. Full decode rejects corrupt streams;
    /// the temporary source and pixels are released before immutable publication.
    pub fn new(bytes: Vec<u8>, provenance: TextureProvenance) -> Result<Self> {
        provenance.validate()?;
        if bytes.is_empty() || bytes.len() > MAX_TEXTURE_BYTES || bytes.capacity() > MAX_TEXTURE_BYTES {
            return Err(invalid("encoded texture capacity must contain at most 16 MiB"));
        }
        reject_animation(&bytes)?;
        let mut checked = Vec::new();
        checked.try_reserve_exact(bytes.len()).map_err(|_| invalid("could not reserve texture validation bytes"))?;
        checked.extend_from_slice(&bytes);
        let source = TextureSource::new_portable(checked, MapRole::Roughness).map_err(|e| invalid(e.to_string()))?;
        let dimensions = [source.info().width, source.info().height];
        if dimensions.into_iter().any(|edge| edge == 0 || edge > MAX_TEXTURE_EDGE) {
            return Err(invalid("embedded texture dimensions exceed 1024x1024"));
        }
        source.validate_static_source().map_err(|e| invalid(e.to_string()))?;
        source.decode(|| false).map_err(|e| invalid(e.to_string()))?;
        Ok(Self { id: HdrId::of(&bytes), bytes: Arc::new(bytes), dimensions, provenance })
    }
    pub fn id(&self) -> HdrId {
        self.id
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn dimensions(&self) -> [u32; 2] {
        self.dimensions
    }
    /// The native cooperative row consumer has a narrower encoding contract
    /// than GPU preparation. The complete stream was validated at construction.
    pub fn validate_cpu_encoding(&self) -> Result<()> {
        if !self.bytes.starts_with(b"\x89PNG\r\n\x1a\n") || self.bytes.get(24).copied() != Some(8) || self.bytes.get(28).copied() != Some(0) {
            return Err(invalid(
                "CPU surface maps require noninterlaced 8-bit PNG; keep JPEG/interlaced images on the GPU or normalize them before import",
            ));
        }
        Ok(())
    }
    /// Binding roles are explicit; one encoded image may be color or linear data.
    pub fn validate_role(&self, role: MapRole) -> Result<()> {
        solidcraft_render::texture_input::validate_color_declarations(&self.bytes, role).map_err(|e| invalid(e.to_string()))
    }
    pub fn provenance(&self) -> &TextureProvenance {
        &self.provenance
    }
    pub fn retained_bytes(&self) -> usize {
        self.bytes.capacity()
    }
    pub fn base64(&self) -> Result<String> {
        crate::render_resources::encode_hdr_base64(&self.bytes)
    }
}
fn reject_animation(bytes: &[u8]) -> Result<()> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Ok(());
    }
    let mut offset = 8usize;
    while offset < bytes.len() {
        let header = bytes.get(offset..offset.saturating_add(8)).ok_or_else(|| invalid("truncated texture PNG chunk"))?;
        let length = header
            .get(..4)
            .and_then(|v| <[u8; 4]>::try_from(v).ok())
            .map(u32::from_be_bytes)
            .ok_or_else(|| invalid("invalid texture PNG chunk length"))? as usize;
        if header.get(4..8) == Some(b"acTL") {
            return Err(invalid("embedded textures require static PNG or JPEG images"));
        }
        offset = offset
            .checked_add(length)
            .and_then(|n| n.checked_add(12))
            .filter(|n| *n <= bytes.len())
            .ok_or_else(|| invalid("texture PNG chunk exceeds encoded bytes"))?;
    }
    Ok(())
}
impl Serialize for TextureResource {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut value = serializer.serialize_struct("TextureResource", 3)?;
        value.serialize_field("kind", "opaque_texture")?;
        value.serialize_field("data", &self.base64().map_err(serde::ser::Error::custom)?)?;
        value.serialize_field("provenance", &self.provenance)?;
        value.end()
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct TextureResources(BTreeMap<HdrId, TextureResource>);
impl TextureResources {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn iter(&self) -> impl Iterator<Item = (&HdrId, &TextureResource)> {
        self.0.iter()
    }
    pub fn get(&self, id: &HdrId) -> Option<&TextureResource> {
        self.0.get(id)
    }
    pub fn required(&self, id: &HdrId) -> Result<&TextureResource> {
        self.get(id).ok_or_else(|| invalid(format!("missing embedded texture resource {id}")))
    }
    pub fn total_bytes(&self) -> usize {
        self.0.values().map(TextureResource::retained_bytes).sum()
    }
    /// Identical bytes retain the first stored provenance; importing labels never
    /// changes content identity or another binding's provenance.
    pub fn insert(&mut self, resource: TextureResource) -> Result<HdrId> {
        let id = resource.id();
        if let Some(old) = self.get(&id) {
            if old.bytes() != resource.bytes() {
                return Err(invalid("texture SHA-256 collision"));
            }
            return Ok(id);
        }
        if self.len() >= MAX_TEXTURE_RESOURCES || resource.retained_bytes() > MAX_TEXTURE_RESOURCE_BYTES.saturating_sub(self.total_bytes()) {
            return Err(invalid("embedded textures exceed 32 entries or 64 MiB"));
        }
        self.0.insert(id, resource);
        Ok(id)
    }
    /// The engine checks saved users before removing a resource transactionally.
    pub fn remove(&mut self, id: &HdrId) -> Option<TextureResource> {
        self.0.remove(id)
    }
}
impl<'de> Deserialize<'de> for TextureResources {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct Resources;
        impl<'de> serde::de::Visitor<'de> for Resources {
            type Value = TextureResources;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("32 unique opaque texture sources totalling at most 64 MiB")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> std::result::Result<Self::Value, A::Error> {
                let mut result = TextureResources::default();
                while let Some(id) = map.next_key::<HdrId>()? {
                    if result.len() >= MAX_TEXTURE_RESOURCES || result.get(&id).is_some() {
                        return Err(serde::de::Error::custom("too many or duplicate embedded texture resources"));
                    }
                    let remaining = MAX_TEXTURE_RESOURCE_BYTES.saturating_sub(result.total_bytes()).min(MAX_TEXTURE_BYTES);
                    let resource = map.next_value_seed(ResourceSeed(remaining))?;
                    if resource.id() != id {
                        return Err(serde::de::Error::custom("embedded texture hash mismatch"));
                    }
                    result.insert(resource).map_err(serde::de::Error::custom)?;
                }
                Ok(result)
            }
        }
        deserializer.deserialize_map(Resources)
    }
}
struct ResourceSeed(usize);
impl<'de> DeserializeSeed<'de> for ResourceSeed {
    type Value = TextureResource;
    fn deserialize<D: serde::Deserializer<'de>>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error> {
        struct ResourceVisitor(usize);
        impl<'de> serde::de::Visitor<'de> for ResourceVisitor {
            type Value = TextureResource;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("opaque_texture kind, bounded base64 data and inert provenance")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> std::result::Result<Self::Value, A::Error> {
                let (mut kind, mut data, mut provenance) = (false, None, None);
                while let Some(field) = map.next_key::<Field>()? {
                    match field {
                        Field::Kind if !kind => {
                            map.next_value_seed(KindSeed)?;
                            kind = true;
                        }
                        Field::Data if data.is_none() => data = Some(map.next_value_seed(PayloadSeed(self.0))?),
                        Field::Provenance if provenance.is_none() => provenance = Some(map.next_value_seed(ProvenanceSeed)?),
                        _ => return Err(serde::de::Error::custom("duplicate embedded texture field")),
                    }
                }
                if !kind {
                    return Err(serde::de::Error::missing_field("kind"));
                }
                TextureResource::new(
                    data.ok_or_else(|| serde::de::Error::missing_field("data"))?,
                    provenance.ok_or_else(|| serde::de::Error::missing_field("provenance"))?,
                )
                .map_err(serde::de::Error::custom)
            }
        }
        deserializer.deserialize_map(ResourceVisitor(self.0))
    }
}
#[derive(Deserialize)]
#[serde(field_identifier, rename_all = "snake_case")]
enum Field {
    Kind,
    Data,
    Provenance,
}
struct KindSeed;
impl<'de> DeserializeSeed<'de> for KindSeed {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        struct Kind;
        impl serde::de::Visitor<'_> for Kind {
            type Value = ();
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("opaque_texture")
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> std::result::Result<(), E> {
                if value == "opaque_texture" { Ok(()) } else { Err(E::custom("opaque_texture kind required")) }
            }
        }
        d.deserialize_str(Kind)
    }
}
struct ProvenanceSeed;
impl<'de> DeserializeSeed<'de> for ProvenanceSeed {
    type Value = TextureProvenance;
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> std::result::Result<Self::Value, D::Error> {
        let value = TextureProvenance::deserialize(d)?;
        value.validate().map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}
struct PayloadSeed(usize);
impl<'de> DeserializeSeed<'de> for PayloadSeed {
    type Value = Vec<u8>;
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> std::result::Result<Self::Value, D::Error> {
        struct Payload(usize);
        impl serde::de::Visitor<'_> for Payload {
            type Value = Vec<u8>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("bounded canonical texture base64")
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> std::result::Result<Self::Value, E> {
                crate::render_resources::decode_base64(value, self.0).map_err(E::custom)
            }
        }
        d.deserialize_str(Payload(self.0))
    }
}
fn bounded_text<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<String, D::Error> {
    struct Text;
    impl serde::de::Visitor<'_> for Text {
        type Value = String;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("at most 4096 provenance bytes")
        }
        fn visit_str<E: serde::de::Error>(self, value: &str) -> std::result::Result<Self::Value, E> {
            if value.len() > 4096 {
                return Err(E::custom("texture provenance text exceeds its bound"));
            }
            Ok(value.to_owned())
        }
    }
    d.deserialize_str(Text)
}

/// First pass borrows payload JSON and enforces count/unique/encoded budgets before
/// Document's migration Value can allocate decoded base64 strings or coalesce keys.
pub(crate) fn check_json_budget(text: &str) -> Result<()> {
    #[derive(Default, Deserialize)]
    struct Render {
        #[serde(default, deserialize_with = "budgets")]
        texture_resources: (),
    }
    #[derive(Deserialize)]
    struct Design {
        #[serde(default)]
        render: Render,
    }
    let Design { render: Render { texture_resources: () } } =
        serde_json::from_str(text).map_err(|e| invalid(format!("texture resource admission: {e}")))?;
    Ok(())
}
fn budgets<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<(), D::Error> {
    struct Budgets;
    impl<'de> serde::de::Visitor<'de> for Budgets {
        type Value = ();
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("bounded unique texture resources")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> std::result::Result<(), A::Error> {
            let mut keys = std::collections::BTreeSet::new();
            let mut total = 0usize;
            while let Some(id) = map.next_key::<HdrId>()? {
                if keys.len() >= MAX_TEXTURE_RESOURCES || !keys.insert(id) {
                    return Err(serde::de::Error::custom("too many or duplicate embedded texture resources"));
                }
                total += map.next_value_seed(ResourceBudget(MAX_TEXTURE_RESOURCE_BYTES.saturating_sub(total).min(MAX_TEXTURE_BYTES)))?;
            }
            Ok(())
        }
    }
    d.deserialize_map(Budgets)
}
struct ResourceBudget(usize);
impl<'de> DeserializeSeed<'de> for ResourceBudget {
    type Value = usize;
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> std::result::Result<Self::Value, D::Error> {
        struct ResourceVisitor(usize);
        impl<'de> serde::de::Visitor<'de> for ResourceVisitor {
            type Value = usize;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("bounded texture metadata and base64")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> std::result::Result<Self::Value, A::Error> {
                let (mut kind, mut size, mut provenance) = (false, None, false);
                while let Some(field) = map.next_key::<Field>()? {
                    match field {
                        Field::Kind if !kind => {
                            let raw = map.next_value::<&serde_json::value::RawValue>()?;
                            if raw.get().len() > 86 {
                                return Err(serde::de::Error::custom("texture kind exceeds its budget"));
                            }
                            let value: String = serde_json::from_str(raw.get()).map_err(serde::de::Error::custom)?;
                            if value != "opaque_texture" {
                                return Err(serde::de::Error::custom("opaque_texture kind required"));
                            }
                            kind = true;
                        }
                        Field::Data if size.is_none() => {
                            let raw = map.next_value::<&serde_json::value::RawValue>()?;
                            size = Some(crate::render_resources::json_base64_length(raw.get(), self.0).map_err(serde::de::Error::custom)?);
                        }
                        Field::Provenance if !provenance => {
                            let raw = map.next_value::<&serde_json::value::RawValue>()?;
                            if raw.get().len() > MAX_PROVENANCE_JSON {
                                return Err(serde::de::Error::custom("texture provenance exceeds its budget"));
                            }
                            let value: TextureProvenance = serde_json::from_str(raw.get()).map_err(serde::de::Error::custom)?;
                            value.validate().map_err(serde::de::Error::custom)?;
                            provenance = true;
                        }
                        _ => return Err(serde::de::Error::custom("duplicate embedded texture field")),
                    }
                }
                if !kind || !provenance {
                    return Err(serde::de::Error::custom("complete texture metadata required"));
                }
                size.ok_or_else(|| serde::de::Error::missing_field("data"))
            }
        }
        d.deserialize_map(ResourceVisitor(self.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render_settings::{GpuJob, RenderJob, RenderSettings, RenderSetup};
    use crate::render_surface::{EmbeddedSurfaceKind, EmbeddedSurfaceSource, RenderBodyTarget, SavedSurface, SurfaceSource};
    use serde_json::json;
    fn image() -> Vec<u8> {
        include_bytes!("../../../assets/render/starter/materials/exports/SURF-027/base_color.png").to_vec()
    }
    fn provenance() -> TextureProvenance {
        TextureProvenance { author: "SolidCraft contributors".into(), source: "original test asset".into(), license: "MIT OR Apache-2.0".into() }
    }
    fn resource() -> TextureResource {
        TextureResource::new(image(), provenance()).unwrap()
    }
    #[test]
    fn exact_encoded_texture_bytes_are_shared_deduplicated_and_portable() {
        let first = resource();
        let copy = first.clone();
        assert!(Arc::ptr_eq(&first.bytes, &copy.bytes));
        assert_eq!(first.id(), HdrId::of(&image()));
        assert_eq!(first.dimensions(), [512, 512]);
        let mut bank = TextureResources::default();
        let id = bank.insert(first).unwrap();
        let mut different_label = copy.clone();
        different_label.provenance.author = "Another declared author".into();
        assert_eq!(bank.insert(different_label).unwrap(), id);
        assert_eq!(bank.len(), 1);
        assert_eq!(bank.total_bytes(), copy.retained_bytes());
        assert_eq!(bank.required(&id).unwrap().provenance(), &provenance());
        let wire = serde_json::to_value(&bank).unwrap();
        let from_value: TextureResources = serde_json::from_value(wire.clone()).unwrap();
        let from_json: TextureResources = serde_json::from_str(&wire.to_string()).unwrap();
        assert_eq!(from_value, bank);
        assert_eq!(from_json, bank);
        assert!(bank.required(&HdrId::of(b"missing")).is_err());
        assert_eq!(bank.remove(&id).unwrap().bytes(), image());
        assert!(bank.is_empty());
    }
    #[test]
    fn embedded_roles_roundtrip_and_missing_references_refuse() {
        let resource = resource();
        let id = resource.id();
        let embedded = EmbeddedSurfaceSource {
            kind: EmbeddedSurfaceKind::Embedded,
            base_color: Some(id),
            normal_open_gl: None,
            roughness: Some(id),
            metallic: None,
            flip_v: true,
        };
        let mut setup = RenderSetup {
            job: Some(RenderJob::GpuPath { transparent_background: false, options: Box::new(GpuJob::default()) }),
            ..Default::default()
        };
        setup
            .surfaces
            .set(
                SavedSurface {
                    target: RenderBodyTarget { definition: "Wood test panel".into(), feature: 7, component: 0, occurrence_path: vec![] },
                    source: SurfaceSource::Embedded(embedded),
                    local_frame: solidcraft_render::job::textures::PlanarFrame { origin: [0.; 3], u: [240., 0., 0.], v: [0., 480., 0.] },
                    normal_strength: 0.5,
                },
                false,
            )
            .unwrap();
        let mut settings = RenderSettings::default();
        settings.setups.insert("Wood".into(), setup);
        assert!(settings.validate().unwrap_err().to_string().contains("missing embedded texture"));
        settings.texture_resources.insert(resource).unwrap();
        settings.validate().unwrap();
        let doc = crate::Document { render: Arc::new(settings.clone()), ..Default::default() };
        let wire = doc.to_json();
        assert_eq!(crate::Document::from_json(&wire).unwrap(), doc);
        // The saved design holds the texture's bytes under the resource's own key.
        fn resource_saved(v: &serde_json::Value, key: &str) -> bool {
            match v {
                serde_json::Value::Object(map) => {
                    map.get("texture_resources").and_then(|t| t.get(key)).is_some() || map.values().any(|v| resource_saved(v, key))
                }
                serde_json::Value::Array(items) => items.iter().any(|v| resource_saved(v, key)),
                _ => false,
            }
        }
        assert!(resource_saved(&serde_json::from_str(&wire).unwrap(), &id.to_string()));
        assert!(!wire.contains("builtin://"));
        assert!(!wire.contains("file://"));
        settings.texture_resources.remove(&id);
        assert!(settings.validate().is_err());
        assert!(serde_json::from_value::<EmbeddedSurfaceSource>(json!({"kind":"embedded","flip_v":false})).unwrap().validate().is_err());
        assert!(serde_json::from_value::<SurfaceSource>(json!({"kind":"automatic","base_color":id})).is_err());
    }
    #[test]
    fn corrupt_changed_digest_animated_or_malformed_texture_records_refuse() {
        let good = resource();
        let id = good.id().to_string();
        let entry = serde_json::to_value(&good).unwrap();
        let wire = json!({id.clone():entry.clone()});
        for (field, value) in [
            ("kind", json!("exr")),
            ("data", json!("AAAA")),
            ("provenance", json!({"author":"x\n","source":"","license":""})),
            ("path", json!("wood.png")),
        ] {
            let mut changed = wire.clone();
            changed[&id][field] = value;
            assert!(serde_json::from_value::<TextureResources>(changed).is_err(), "{field}");
        }
        let wrong_id = HdrId::of(b"wrong").to_string();
        assert!(serde_json::from_value::<TextureResources>(json!({wrong_id:entry.clone()})).unwrap_err().to_string().contains("hash mismatch"));
        let duplicate = format!("{{\"{id}\":{entry},\"{id}\":{entry}}}");
        assert!(serde_json::from_str::<TextureResources>(&duplicate).unwrap_err().to_string().contains("duplicate"));
        let mut truncated = image();
        truncated.truncate(truncated.len() - 1);
        assert!(TextureResource::new(truncated, provenance()).is_err());
        // APNG control is rejected before interpreting a potentially moving image.
        let mut animated = image();
        animated.splice(8..8, [0, 0, 0, 0, b'a', b'c', b'T', b'L', 0, 0, 0, 0]);
        assert!(TextureResource::new(animated, provenance()).unwrap_err().to_string().contains("static"));
        let mut corrupt_tail = image();
        let end = corrupt_tail.len() - 12;
        corrupt_tail.splice(end..end, [0, 0, 0, 1, b'v', b'p', b'A', b'g', 17, 0, 0, 0, 0]);
        assert!(TextureResource::new(corrupt_tail, provenance()).unwrap_err().to_string().contains("CRC"));
        let mut oversized_capacity = Vec::with_capacity(MAX_TEXTURE_BYTES + 1);
        oversized_capacity.extend_from_slice(&image());
        assert!(TextureResource::new(oversized_capacity, provenance()).is_err());
        let mut bad_provenance = provenance();
        bad_provenance.source = "x".repeat(2049);
        assert!(TextureResource::new(image(), bad_provenance).is_err());
    }
    #[test]
    fn json_texture_admission_preserves_duplicates_and_enforces_count_before_migration() {
        let entry = serde_json::to_value(resource()).unwrap();
        let mut resources = serde_json::Map::new();
        for n in 0..33 {
            resources.insert(HdrId::of(&[n]).to_string(), entry.clone());
        }
        let text = json!({"render":{"texture_resources":resources}}).to_string();
        assert!(check_json_budget(&text).unwrap_err().to_string().contains("too many"));
        let id = HdrId::of(&image()).to_string();
        let text = format!("{{\"render\":{{\"texture_resources\":{{\"{id}\":{entry},\"{id}\":{entry}}}}}}}");
        assert!(check_json_budget(&text).unwrap_err().to_string().contains("duplicate"));
        let escaped = "\\u0041".repeat(12);
        let text = format!("{{\"kind\":\"opaque_texture\",\"data\":\"{escaped}\",\"provenance\":{{}}}}");
        let mut d = serde_json::Deserializer::from_str(&text);
        assert!(ResourceBudget(1).deserialize(&mut d).is_err());
        let text = json!({"render":{"texture_resources":{id:{"kind":"opaque_texture","data":"AAAA","provenance":{"author":"x".repeat(MAX_PROVENANCE_JSON)}}}}}).to_string();
        assert!(check_json_budget(&text).unwrap_err().to_string().contains("provenance"));
    }
    #[test]
    fn encoded_resource_is_role_independent_and_conflicting_color_binding_is_rejected() {
        let mut bytes = image();
        let mut chunk = 4_u32.to_be_bytes().to_vec();
        chunk.extend(b"gAMA");
        chunk.extend(100_000_u32.to_be_bytes());
        let mut crc = u32::MAX;
        for byte in &chunk[4..] {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb8_8320_u32.wrapping_mul(crc & 1));
            }
        }
        chunk.extend((!crc).to_be_bytes());
        bytes.splice(33..33, chunk);
        let resource = TextureResource::new(bytes, provenance()).unwrap();
        assert!(resource.validate_role(MapRole::BaseColor).unwrap_err().to_string().contains("normalized sRGB"));
        resource.validate_role(MapRole::Roughness).unwrap();
        let id = resource.id();
        let mut setup = RenderSetup {
            job: Some(RenderJob::GpuPath { transparent_background: false, options: Box::new(GpuJob::default()) }),
            ..Default::default()
        };
        let mut binding = SavedSurface {
            target: RenderBodyTarget { definition: "Wood".into(), feature: 7, component: 0, occurrence_path: vec![] },
            source: SurfaceSource::Embedded(EmbeddedSurfaceSource {
                kind: EmbeddedSurfaceKind::Embedded,
                base_color: Some(id),
                normal_open_gl: None,
                roughness: None,
                metallic: None,
                flip_v: false,
            }),
            local_frame: solidcraft_render::job::textures::PlanarFrame { origin: [0.; 3], u: [1., 0., 0.], v: [0., 1., 0.] },
            normal_strength: 1.,
        };
        setup.surfaces.set(binding.clone(), false).unwrap();
        let mut settings = RenderSettings::default();
        settings.texture_resources.insert(resource).unwrap();
        assert!(settings.validate_resources(&setup).is_err());
        if let SurfaceSource::Embedded(source) = &mut binding.source {
            source.base_color = None;
            source.roughness = Some(id);
        }
        setup.surfaces.set(binding, true).unwrap();
        settings.validate_resources(&setup).unwrap();
    }
}
