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
        let value = SchemaValue { path: String::new(), record_at: None, depth: 0, property_order: &mut property_order }.deserialize(deserializer)?;
        Ok(Self { value, property_order })
    }
}

struct SchemaValue<'a> {
    path: String,
    /// This value is a `properties` object: record its keys at its containing schema's path.
    record_at: Option<String>,
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
            let child = SchemaValue {
                path: format!("{}/{escaped}", self.path),
                record_at: (key == "properties").then(|| self.path.clone()),
                depth: self.depth + 1,
                property_order: &mut *self.property_order,
            };
            value.insert(key, map.next_value_seed(child)?);
        }
        if let Some(path) = self.record_at {
            self.property_order.insert(path, names);
        }
        Ok(Value::Object(value))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(SchemaValue {
            path: format!("{}/{}", self.path, values.len()),
            record_at: None,
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
