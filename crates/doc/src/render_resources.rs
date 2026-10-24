//! Immutable, content-addressed HDR payloads carried by the design, never external paths.

use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Serialize, de::DeserializeSeed, ser::SerializeStruct};
use sha2::{Digest, Sha256};
pub use solidcraft_render::lighting::MAX_HDR_BYTES;
use solidcraft_render::lighting::{HdrInfo, HdrSource};

use crate::{DocError, Result};

pub const MAX_HDR_RESOURCES: usize = 64;
pub const MAX_HDR_RESOURCE_BYTES: usize = 64 << 20;
pub const SRGB_D65_PRIMARIES: [f64; 8] = [0.64, 0.33, 0.30, 0.60, 0.15, 0.06, 0.3127, 0.3290];

fn invalid(message: impl Into<String>) -> DocError {
    DocError::Invalid(message.into())
}

/// Exact SHA-256 of encoded source bytes. A small value keeps numerical GPU intent Copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct HdrId([u8; 32]);

impl HdrId {
    pub fn of(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }
}

impl fmt::Display for HdrId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl Serialize for HdrId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for HdrId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct Id;
        impl serde::de::Visitor<'_> for Id {
            type Value = HdrId;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a lowercase 64-digit HDR SHA-256")
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> std::result::Result<HdrId, E> {
                if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
                    return Err(E::custom("HDR resource must be a lowercase 64-digit SHA-256"));
                }
                let mut bytes = [0; 32];
                for (out, pair) in bytes.iter_mut().zip(value.as_bytes().as_chunks::<2>().0) {
                    let digit = |b: u8| if b.is_ascii_digit() { b - b'0' } else { b - b'a' + 10 };
                    let high = pair.first().copied().ok_or_else(|| E::custom("missing HDR digest digit"))?;
                    let low = pair.get(1).copied().ok_or_else(|| E::custom("missing HDR digest digit"))?;
                    *out = digit(high) * 16 + digit(low);
                }
                Ok(HdrId(bytes))
            }
        }
        deserializer.deserialize_str(Id)
    }
}

/// Construction validates the complete bounded HDR stream. Cloning shares its source Arc.
#[derive(Clone)]
pub struct HdrResource {
    id: HdrId,
    source: HdrSource,
}

impl HdrResource {
    pub fn new(mut bytes: Vec<u8>) -> Result<Self> {
        if bytes.is_empty() || bytes.len() > MAX_HDR_BYTES || bytes.capacity() > MAX_HDR_BYTES {
            return Err(invalid("HDR source capacity must contain at most 16 MiB"));
        }
        if bytes.capacity() != bytes.len() {
            let mut compact = Vec::new();
            compact.try_reserve_exact(bytes.len()).map_err(|_| invalid("could not reserve immutable HDR source"))?;
            compact.extend_from_slice(&bytes);
            bytes = compact;
        }
        let source = HdrSource::new(bytes).map_err(|e| invalid(e.to_string()))?;
        source.validate(|| false).map_err(|e| invalid(e.to_string()))?;
        Ok(Self { id: HdrId::of(source.encoded_bytes()), source })
    }
    pub fn id(&self) -> HdrId {
        self.id
    }
    pub fn bytes(&self) -> &[u8] {
        self.source.encoded_bytes()
    }
    pub fn source(&self) -> HdrSource {
        self.source.clone()
    }
    pub fn info(&self) -> &HdrInfo {
        self.source.info()
    }
    pub fn retained_bytes(&self) -> usize {
        // HdrSource only admits capacities at most MAX_HDR_BYTES on every target.
        self.source.retained_encoded_bytes() as usize
    }
    pub fn base64(&self) -> Result<String> {
        encode_hdr_base64(self.bytes())
    }
}

impl fmt::Debug for HdrResource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HdrResource").field("id", &self.id).field("info", self.info()).finish()
    }
}
impl PartialEq for HdrResource {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.bytes() == other.bytes()
    }
}
impl Serialize for HdrResource {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut value = serializer.serialize_struct("HdrResource", 2)?;
        value.serialize_field("kind", "radiance_hdr")?;
        value.serialize_field("data", &self.base64().map_err(serde::ser::Error::custom)?)?;
        value.end()
    }
}

/// One immutable payload per digest, shared by setups and document undo snapshots.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(transparent)]
pub struct HdrResources(BTreeMap<HdrId, HdrResource>);

impl HdrResources {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn iter(&self) -> impl Iterator<Item = (&HdrId, &HdrResource)> {
        self.0.iter()
    }
    pub fn get(&self, id: &HdrId) -> Option<&HdrResource> {
        self.0.get(id)
    }
    pub fn required(&self, id: &HdrId) -> Result<&HdrResource> {
        self.get(id).ok_or_else(|| invalid(format!("missing embedded HDR resource {id}")))
    }
    pub fn total_bytes(&self) -> usize {
        self.0.values().map(HdrResource::retained_bytes).sum()
    }
    pub fn insert(&mut self, resource: HdrResource) -> Result<HdrId> {
        let id = resource.id();
        if let Some(previous) = self.0.get(&id) {
            if previous != &resource {
                return Err(invalid("HDR digest collision with different bytes"));
            }
            return Ok(id);
        }
        if self.len() >= MAX_HDR_RESOURCES || resource.retained_bytes() > MAX_HDR_RESOURCE_BYTES.saturating_sub(self.total_bytes()) {
            return Err(invalid("embedded HDR resources exceed 64 entries or 64 MiB"));
        }
        self.0.insert(id, resource);
        Ok(id)
    }
}

impl<'de> Deserialize<'de> for HdrResources {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct Resources;
        impl<'de> serde::de::Visitor<'de> for Resources {
            type Value = HdrResources;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("at most 64 embedded HDR sources totalling at most 64 MiB")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> std::result::Result<HdrResources, A::Error> {
                let mut resources = HdrResources::default();
                while let Some(id) = map.next_key::<HdrId>()? {
                    if resources.len() == MAX_HDR_RESOURCES || resources.get(&id).is_some() {
                        return Err(serde::de::Error::custom("too many or duplicate embedded HDR resources"));
                    }
                    let remaining = MAX_HDR_RESOURCE_BYTES.saturating_sub(resources.total_bytes()).min(MAX_HDR_BYTES);
                    let resource = map.next_value_seed(ResourceSeed(remaining))?;
                    if resource.id() != id {
                        return Err(serde::de::Error::custom("embedded HDR resource hash mismatch"));
                    }
                    resources.insert(resource).map_err(serde::de::Error::custom)?;
                }
                Ok(resources)
            }
        }
        deserializer.deserialize_map(Resources)
    }
}

struct ResourceSeed(usize);
impl<'de> DeserializeSeed<'de> for ResourceSeed {
    type Value = HdrResource;
    fn deserialize<D: serde::Deserializer<'de>>(self, deserializer: D) -> std::result::Result<HdrResource, D::Error> {
        deserializer.deserialize_map(self)
    }
}
impl<'de> serde::de::Visitor<'de> for ResourceSeed {
    type Value = HdrResource;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a complete Radiance HDR resource with kind and base64 data")
    }
    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> std::result::Result<HdrResource, A::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum Kind {
            RadianceHdr,
        }
        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "snake_case")]
        enum Field {
            Kind,
            Data,
        }
        let (mut kind, mut data) = (false, None);
        while let Some(field) = map.next_key::<Field>()? {
            match field {
                Field::Kind if !kind => {
                    let Kind::RadianceHdr = map.next_value()?;
                    kind = true;
                }
                Field::Data if data.is_none() => data = Some(map.next_value_seed(PayloadSeed(self.0))?),
                _ => return Err(serde::de::Error::custom("duplicate HDR resource field")),
            }
        }
        if !kind {
            return Err(serde::de::Error::missing_field("kind"));
        }
        HdrResource::new(data.ok_or_else(|| serde::de::Error::missing_field("data"))?).map_err(serde::de::Error::custom)
    }
}

struct PayloadSeed(usize);
impl<'de> DeserializeSeed<'de> for PayloadSeed {
    type Value = Vec<u8>;
    fn deserialize<D: serde::Deserializer<'de>>(self, deserializer: D) -> std::result::Result<Vec<u8>, D::Error> {
        deserializer.deserialize_str(self)
    }
}
impl serde::de::Visitor<'_> for PayloadSeed {
    type Value = Vec<u8>;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "canonical base64 of at most {} HDR bytes", self.0)
    }
    fn visit_str<E: serde::de::Error>(self, value: &str) -> std::result::Result<Vec<u8>, E> {
        decode_base64(value, self.0).map_err(E::custom)
    }
}

/// The container hydrator uses the same bounded canonical wire representation as JSON.
pub fn encode_hdr_base64(bytes: &[u8]) -> Result<String> {
    if bytes.is_empty() || bytes.len() > MAX_HDR_BYTES {
        return Err(invalid("HDR source must contain at most 16 MiB"));
    }
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::new();
    encoded.try_reserve_exact(bytes.len().div_ceil(3) * 4).map_err(|_| invalid("could not reserve HDR base64"))?;
    for triple in bytes.chunks(3) {
        let n = triple.iter().fold(0u32, |n, byte| (n << 8) | u32::from(*byte)) << ((3 - triple.len()) * 8);
        for i in 0..4 {
            if i <= triple.len() {
                let index = ((n >> (18 - i * 6)) & 63) as usize;
                let byte = ALPHABET.get(index).copied().ok_or_else(|| invalid("invalid base64 digit"))?;
                encoded.push(char::from(byte));
            } else {
                encoded.push('=');
            }
        }
    }
    Ok(encoded)
}

fn decode_base64(value: &str, maximum: usize) -> Result<Vec<u8>> {
    let length = base64_length(value, maximum)?;
    let padding = if value.ends_with("==") { 2 } else { usize::from(value.ends_with('=')) };
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length).map_err(|_| invalid("could not reserve embedded HDR bytes"))?;
    for (i, group) in value.as_bytes().as_chunks::<4>().0.iter().enumerate() {
        let last = (i + 1) * 4 == value.len();
        let mut n = 0u32;
        for (j, byte) in group.iter().copied().enumerate() {
            let digit = match byte {
                b'A'..=b'Z' => byte - b'A',
                b'a'..=b'z' => byte - b'a' + 26,
                b'0'..=b'9' => byte - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' if last && j >= 4 - padding => 0,
                _ => return Err(invalid("noncanonical HDR base64")),
            };
            n = (n << 6) | u32::from(digit);
        }
        if last && ((padding == 2 && n & 0xffff != 0) || (padding == 1 && n & 0xff != 0)) {
            return Err(invalid("noncanonical HDR base64 padding bits"));
        }
        for offset in 0..if last { 3 - padding } else { 3 } {
            bytes.push(((n >> (16 - offset * 8)) & 255) as u8);
        }
    }
    Ok(bytes)
}

fn base64_length(value: &str, maximum: usize) -> Result<usize> {
    if value.is_empty() || !value.len().is_multiple_of(4) || value.len() > maximum.div_ceil(3) * 4 {
        return Err(invalid("HDR base64 exceeds the resource budget or is not canonical"));
    }
    let padding = if value.ends_with("==") { 2 } else { usize::from(value.ends_with('=')) };
    let length = value.len() / 4 * 3 - padding;
    if length == 0 || length > maximum {
        return Err(invalid("HDR payload exceeds the remaining resource budget"));
    }
    Ok(length)
}

/// Admission before the migration reader builds a JSON Value. It must not collapse
/// duplicate resource keys or allocate a large payload tree before enforcing HDR quotas.
pub(crate) fn check_json_budget(text: &str) -> Result<()> {
    #[derive(Default, Deserialize)]
    struct Render {
        #[serde(default, deserialize_with = "resource_budgets")]
        resources: (),
    }
    #[derive(Deserialize)]
    struct Design {
        #[serde(default)]
        render: Render,
    }
    let Design { render: Render { resources: () } } = serde_json::from_str(text).map_err(|e| invalid(format!("HDR resource admission: {e}")))?;
    Ok(())
}

fn resource_budgets<'de, D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<(), D::Error> {
    struct Budgets;
    impl<'de> serde::de::Visitor<'de> for Budgets {
        type Value = ();
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a bounded map of unique HDR resources")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> std::result::Result<(), A::Error> {
            let mut keys = std::collections::BTreeSet::new();
            let mut total = 0usize;
            while let Some(id) = map.next_key::<HdrId>()? {
                if keys.len() == MAX_HDR_RESOURCES || !keys.insert(id) {
                    return Err(serde::de::Error::custom("too many or duplicate embedded HDR resources"));
                }
                let limit = MAX_HDR_RESOURCE_BYTES.saturating_sub(total).min(MAX_HDR_BYTES);
                total += map.next_value_seed(ResourceBudget(limit))?;
            }
            Ok(())
        }
    }
    deserializer.deserialize_map(Budgets)
}

struct ResourceBudget(usize);
impl<'de> DeserializeSeed<'de> for ResourceBudget {
    type Value = usize;
    fn deserialize<D: serde::Deserializer<'de>>(self, deserializer: D) -> std::result::Result<usize, D::Error> {
        deserializer.deserialize_map(self)
    }
}
impl<'de> serde::de::Visitor<'de> for ResourceBudget {
    type Value = usize;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("kind and bounded HDR base64 data")
    }
    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> std::result::Result<usize, A::Error> {
        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "snake_case")]
        enum Field {
            Kind,
            Data,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum Kind {
            RadianceHdr,
        }
        let (mut kind, mut size) = (false, None);
        while let Some(field) = map.next_key::<Field>()? {
            match field {
                Field::Kind if !kind => {
                    let raw: &serde_json::value::RawValue = map.next_value()?;
                    if raw.get().len() > 74 {
                        return Err(serde::de::Error::custom("invalid HDR kind"));
                    }
                    let Kind::RadianceHdr = serde_json::from_str(raw.get()).map_err(serde::de::Error::custom)?;
                    kind = true;
                }
                Field::Data if size.is_none() => size = Some(map.next_value_seed(EncodedBudget(self.0))?),
                _ => return Err(serde::de::Error::custom("duplicate HDR resource field")),
            }
        }
        if !kind {
            return Err(serde::de::Error::missing_field("kind"));
        }
        size.ok_or_else(|| serde::de::Error::missing_field("data"))
    }
}

struct EncodedBudget(usize);
impl<'de> DeserializeSeed<'de> for EncodedBudget {
    type Value = usize;
    fn deserialize<D: serde::Deserializer<'de>>(self, deserializer: D) -> std::result::Result<usize, D::Error> {
        let raw = <&serde_json::value::RawValue>::deserialize(deserializer)?;
        json_base64_length(raw.get(), self.0).map_err(serde::de::Error::custom)
    }
}

// RawValue borrows the JSON source without building serde_json's unescaped String scratch.
// Count base64 characters (including legal JSON slash/Unicode escapes) before any payload
// allocation. Full canonical padding, stream, radiance and digest checks still follow.
fn json_base64_length(raw: &str, maximum: usize) -> Result<usize> {
    let text = raw.strip_prefix('"').and_then(|s| s.strip_suffix('"')).ok_or_else(|| invalid("HDR data must be a base64 string"))?;
    let mut bytes = text.as_bytes().iter().copied();
    let mut count = 0usize;
    let mut last = [0u8; 2];
    while let Some(byte) = bytes.next() {
        let byte = if byte == b'\\' {
            match bytes.next() {
                Some(b'/') => b'/',
                Some(b'u') => {
                    let mut code = 0u16;
                    for _ in 0..4 {
                        let digit = bytes.next().and_then(|b| char::from(b).to_digit(16)).ok_or_else(|| invalid("invalid HDR JSON escape"))?;
                        code = code * 16 + digit as u16;
                    }
                    u8::try_from(code).map_err(|_| invalid("HDR base64 must be ASCII"))?
                }
                _ => return Err(invalid("HDR base64 contains an invalid escape")),
            }
        } else {
            byte
        };
        if !byte.is_ascii_alphanumeric() && !matches!(byte, b'+' | b'/' | b'=') {
            return Err(invalid("HDR base64 must be canonical ASCII"));
        }
        count += 1;
        if count > maximum.div_ceil(3) * 4 {
            return Err(invalid("HDR base64 exceeds the remaining resource budget"));
        }
        last = [last[1], byte];
    }
    if count == 0 || !count.is_multiple_of(4) {
        return Err(invalid("HDR base64 is not canonical"));
    }
    let padding = if last == *b"==" { 2 } else { usize::from(last[1] == b'=') };
    let length = count / 4 * 3 - padding;
    if length == 0 || length > maximum {
        return Err(invalid("HDR payload exceeds the remaining resource budget"));
    }
    Ok(length)
}

/// Persistent CPU controls; unlike GPU, CPU always requires declared sRGB/D65 primaries.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CpuHdrEnvironment {
    pub resource: HdrId,
    pub rotation_rad: f64,
    pub intensity: f64,
    pub samples: u32,
}

impl CpuHdrEnvironment {
    pub fn validate(&self) -> Result<()> {
        Self::check_controls(self.rotation_rad, self.intensity, self.samples)
    }
    pub fn check_controls(rotation_rad: f64, intensity: f64, samples: u32) -> Result<()> {
        validate_controls(rotation_rad, intensity, 64.)?;
        if !matches!(samples, 1 | 4 | 16 | 64) {
            return Err(invalid("saved CPU HDR samples must be 1, 4, 16 or 64"));
        }
        Ok(())
    }
    pub fn validate_resource(&self, resources: &HdrResources) -> Result<()> {
        self.validate()?;
        if resources.required(&self.resource)?.info().primaries != Some(SRGB_D65_PRIMARIES) {
            return Err(invalid("saved CPU HDR requires explicit sRGB/D65 primaries"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GpuHdrEnvironment {
    pub resource: HdrId,
    pub rotation_rad: f64,
    pub intensity: f64,
    pub assume_linear_srgb: bool,
}

impl GpuHdrEnvironment {
    pub fn validate(&self) -> Result<()> {
        Self::check_controls(self.rotation_rad, self.intensity)
    }
    pub fn check_controls(rotation_rad: f64, intensity: f64) -> Result<()> {
        validate_controls(rotation_rad, intensity, 100.)
    }
    pub fn validate_resource(&self, resources: &HdrResources) -> Result<()> {
        self.validate()?;
        match resources.required(&self.resource)?.info().primaries {
            Some(actual) if actual.iter().zip(SRGB_D65_PRIMARIES).all(|(a, b)| (a - b).abs() < 1e-5) => Ok(()),
            None if self.assume_linear_srgb => Ok(()),
            _ => Err(invalid("saved GPU HDR requires sRGB/D65 primaries or an explicit assumption for an undeclared source")),
        }
    }
}

fn validate_controls(rotation: f64, intensity: f64, maximum: f64) -> Result<()> {
    if !rotation.is_finite() || rotation.abs() > 1e6 || !intensity.is_finite() || !(0.0..=maximum).contains(&intensity) {
        return Err(invalid(format!("saved HDR requires finite rotation within +/-1000000 radians and intensity 0..{maximum}")));
    }
    Ok(())
}
