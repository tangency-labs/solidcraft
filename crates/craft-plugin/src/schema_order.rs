//! Read plugin schemas into ordinary JSON values while retaining property declaration order.

use std::collections::BTreeMap;
use std::fmt;

use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

#[derive(Serialize)]
#[serde(transparent)]
pub(crate) struct InputSchema {
    pub(crate) value: Value,
    #[serde(skip)]
    pub(crate) property_order: BTreeMap<String, Vec<String>>,
}

impl Default for InputSchema {
    fn default() -> Self {
        Self { value: crate::empty_input_schema(), property_order: BTreeMap::new() }
    }
}

impl<'de> Deserialize<'de> for InputSchema {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut property_order = BTreeMap::new();
        let value = SchemaValue { path: String::new(), record_at: None, mark: false, literal: false, depth: 0, property_order: &mut property_order }
            .deserialize(deserializer)?;
        Ok(Self { value, property_order })
    }
}

/// Key under which [`MarkedManifest`] stores a schema's declared property order inside the
/// ordinary JSON value, so the order travels with the schema when `$ref`s are inlined (the
/// inlining works on a sorted `serde_json::Value`). [`take_orders`] removes every marker.
const ORDER_MARKER: &str = "x-solidcraft-internal-property-order";

/// Keys whose values are literal data, never schemas.
const LITERAL_KEYS: [&str; 5] = ["const", "default", "enum", "examples", "x-solidcraft-defaults"];

/// A whole manifest read as an ordinary JSON value, except that every object holding a
/// `properties` map also holds the names of that map in the order the manifest declares them.
pub(crate) struct MarkedManifest(pub(crate) Value);

impl<'de> Deserialize<'de> for MarkedManifest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut property_order = BTreeMap::new();
        let value = SchemaValue { path: String::new(), record_at: None, mark: true, literal: false, depth: 0, property_order: &mut property_order }
            .deserialize(deserializer)?;
        Ok(Self(value))
    }
}

/// Remove every order marker from an (already `$ref`-expanded) manifest. Each entry's markers
/// become its `input_property_order`, keyed like [`InputSchema::property_order`], unless the
/// entry already carries one.
pub(crate) fn take_orders(manifest: &mut Value) {
    let mut discarded = BTreeMap::new();
    if let Some(entries) = manifest.get_mut("runtime").and_then(|runtime| runtime.get_mut("entries")).and_then(Value::as_array_mut) {
        for entry in entries {
            let mut orders = BTreeMap::new();
            if let Some(schema) = entry.get_mut("input_schema") {
                strip(schema, "", &mut orders);
            }
            if let Some(entry) = entry.as_object_mut()
                && !orders.is_empty()
                && !entry.contains_key("input_property_order")
            {
                let orders = orders.into_iter().map(|(path, names)| (path, Value::Array(names.into_iter().map(Value::String).collect())));
                entry.insert("input_property_order".into(), Value::Object(orders.collect()));
            }
        }
    }
    strip(manifest, "", &mut discarded);
}

fn strip(value: &mut Value, path: &str, orders: &mut BTreeMap<String, Vec<String>>) {
    match value {
        Value::Object(map) => {
            if let Some(Value::Array(names)) = map.remove(ORDER_MARKER) {
                orders.insert(path.to_string(), names.into_iter().filter_map(|name| name.as_str().map(str::to_string)).collect());
            }
            for (key, child) in map.iter_mut() {
                let escaped = key.replace('~', "~0").replace('/', "~1");
                strip(child, &format!("{path}/{escaped}"), orders);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter_mut().enumerate() {
                strip(item, &format!("{path}/{index}"), orders);
            }
        }
        _ => {}
    }
}

struct SchemaValue<'a> {
    path: String,
    /// This value is a `properties` object: record its keys at its containing schema's path.
    record_at: Option<String>,
    /// Also store each schema's property order inside its value (see [`ORDER_MARKER`]).
    mark: bool,
    /// Inside a literal value (`default`, `enum`...): its keys are data, not schema keywords.
    literal: bool,
    depth: usize,
    property_order: &'a mut BTreeMap<String, Vec<String>>,
}

impl<'de> DeserializeSeed<'de> for SchemaValue<'_> {
    type Value = Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        // Match JSON's usual depth bound even for values supplied by another deserializer.
        if self.depth > 128 {
            return Err(serde::de::Error::custom("input schema is nested too deeply"));
        }
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for SchemaValue<'_> {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON schema value")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut value = Map::new();
        let mut names = Vec::new();
        while let Some(key) = map.next_key::<String>()? {
            if self.record_at.is_some() {
                names.push(key.clone());
            }
            let escaped = key.replace('~', "~0").replace('/', "~1");
            // Keys of a `properties` map are field names, not keywords: a field may be called
            // `properties` or `default`.
            let keyword = self.record_at.is_none() && !self.literal;
            let child = SchemaValue {
                path: format!("{}/{escaped}", self.path),
                record_at: (keyword && key == "properties").then(|| self.path.clone()),
                mark: self.mark,
                literal: self.literal || (keyword && LITERAL_KEYS.contains(&key.as_str())),
                depth: self.depth + 1,
                property_order: &mut *self.property_order,
            };
            value.insert(key, map.next_value_seed(child)?);
        }
        if let Some(path) = self.record_at {
            self.property_order.insert(path, names);
        } else if self.mark
            && !self.literal
            && matches!(value.get("properties"), Some(Value::Object(_)))
            && let Some(names) = self.property_order.get(&self.path)
        {
            value.insert(ORDER_MARKER.into(), Value::Array(names.iter().cloned().map(Value::String).collect()));
        }
        Ok(Value::Object(value))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(SchemaValue {
            path: format!("{}/{}", self.path, values.len()),
            record_at: None,
            mark: self.mark,
            literal: self.literal,
            depth: self.depth + 1,
            property_order: &mut *self.property_order,
        })? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }

    fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Value, E> {
        Ok(value.into())
    }

    fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Value, E> {
        Ok(value.into())
    }

    fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Value, E> {
        Ok(value.into())
    }

    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Value, E> {
        Ok(value.into())
    }

    fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Value, E> {
        Ok(Value::String(value))
    }

    fn visit_unit<E: serde::de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E: serde::de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        self.deserialize(deserializer)
    }
}
